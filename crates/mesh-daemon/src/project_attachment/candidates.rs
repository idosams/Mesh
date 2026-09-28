//! Create-only immutable fleet candidate staging outside ordinary project content and capture history.
use super::{invalid, lanes, read_receipt, recovery, ObservationLimits, ProvisionedAttachment};
use crate::ipc::Json;
use crate::root_authority::PinnedWorkspaceRoot;
use crate::workspace::{HistoricalWorkspacePreview, OpenWorkspace};
use mesh_types::{Blake3, ContentDigest as _};
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Read as _};
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;

const CANDIDATES: &str = "fleet-candidates";
const MAX_CANDIDATES: usize = 128;

#[derive(Clone, Copy)]
pub(crate) enum CandidateAdmission {
    Inspect,
    Stage { main_matches: bool },
}
fn digest(bytes: &[u8]) -> String {
    Blake3::digest_bytes(bytes).to_string()
}
fn identity(root: &PinnedWorkspaceRoot) -> io::Result<Json> {
    let (device, inode) = root.identity()?;
    Ok(Json::object([
        ("device", Json::text(format!("{device:016x}"))),
        ("inode", Json::text(format!("{inode:016x}"))),
    ]))
}
fn private(root: PinnedWorkspaceRoot) -> io::Result<PinnedWorkspaceRoot> {
    if root.try_clone_directory()?.metadata()?.permissions().mode() & 0o077 != 0 {
        return Err(invalid("candidate storage must be private"));
    }
    root.ensure_namespace_identity()?;
    Ok(root)
}
fn write(root: &PinnedWorkspaceRoot, name: &str, bytes: &[u8]) -> io::Result<()> {
    root.filesystem()
        .write_new_file(Path::new(name), bytes, fs::Permissions::from_mode(0o600))
}
fn manifest(snapshot: &HistoricalWorkspacePreview) -> Json {
    Json::object([
        ("schema", Json::text("mesh.fleet-candidate-content/v1")),
        ("version", Json::text(snapshot.operation.to_string())),
        (
            "directories",
            Json::Array(
                snapshot
                    .directories
                    .iter()
                    .map(|dir| {
                        Json::object([
                            ("object", Json::text(dir.object.to_string())),
                            ("path", Json::text(&dir.path)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "files",
            Json::Array(
                snapshot
                    .files
                    .iter()
                    .map(|file| {
                        Json::object([
                            ("object", Json::text(file.object.to_string())),
                            ("path", Json::text(&file.path)),
                            ("digest", Json::text(file.content_digest.to_string())),
                            ("bytes", Json::Number(file.byte_length)),
                            ("executable", Json::Bool(file.executable)),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}
fn verify_manifest(root: &PinnedWorkspaceRoot, expected: &str) -> io::Result<()> {
    let file = root.filesystem().inspect_entry(Path::new("content.json"))?;
    if !file.metadata()?.is_file() {
        return Err(invalid("candidate manifest is not a file"));
    }
    let mut actual = String::new();
    file.take(expected.len() as u64 + 1)
        .read_to_string(&mut actual)?;
    if actual != expected {
        return Err(invalid("candidate manifest changed"));
    }
    Ok(())
}
fn ready(
    allocation: &PinnedWorkspaceRoot,
    files: &PinnedWorkspaceRoot,
    intent: &str,
) -> io::Result<Json> {
    Ok(Json::object([
        ("schema", Json::text("mesh.fleet-candidate-ready/v1")),
        ("intent_digest", Json::text(digest(intent.as_bytes()))),
        ("allocation", identity(allocation)?),
        ("files", identity(files)?),
    ]))
}
impl ProvisionedAttachment {
    /// Stage a native-verified exact result. Existing receipts recover only identical arguments and
    /// unchanged retained content. Partial allocations are preserved and refused, never overwritten.
    pub(crate) fn stage_fleet_candidate(
        &self,
        request: &str,
        provenance: &Json,
        open: &OpenWorkspace,
        snapshot: &HistoricalWorkspacePreview,
        admission: CandidateAdmission,
        verify_fresh: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<Json> {
        if request.len() != 32
            || !request
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(invalid("invalid candidate request"));
        }
        let limits = ObservationLimits::default();
        lanes::check_budget(snapshot, limits)?;
        let content = manifest(snapshot).encode();
        let content_digest = digest(content.as_bytes());
        let candidate = format!(
            "candidate-{}",
            digest(format!("{}:{request}", self.id()).as_bytes())
        );
        let ensure_project = || {
            self.project().ensure_current()?;
            self.store.ensure_namespace_identity()?;
            if read_receipt(&self.store)? != self.project().receipt()?.encode() {
                return Err(invalid("candidate project registration changed"));
            }
            Ok(())
        };
        ensure_project()?;
        let (container, allocation, encoded_intent, created) = {
            // Serialize only bounded admission and intent creation, not content copying or callbacks.
            let _guard = crate::workspace_custody::lock_workspace_initialization(&self.store)
                .map_err(io::Error::other)?;
            ensure_project()?;
            if matches!(admission, CandidateAdmission::Stage { .. }) {
                super::detachment::ensure_attached(&self.store)?;
            }
            let container = private(
                match self.store.open_child_directory(OsStr::new(CANDIDATES)) {
                    Ok(root) => root,
                    Err(error)
                        if error.kind() == io::ErrorKind::NotFound
                            && matches!(
                                admission,
                                CandidateAdmission::Stage { main_matches: true }
                            ) =>
                    {
                        self.store.create_child_directory(OsStr::new(CANDIDATES))?
                    }
                    Err(error) => return Err(error),
                },
            )?;
            let (allocation, created) = match container.open_child_directory(OsStr::new(&candidate))
            {
                Ok(root) => (private(root)?, false),
                Err(error)
                    if error.kind() == io::ErrorKind::NotFound
                        && matches!(
                            admission,
                            CandidateAdmission::Stage { main_matches: true }
                        ) =>
                {
                    if container
                        .filesystem()
                        .read_directory_names_bounded(Path::new(""), MAX_CANDIDATES)?
                        .len()
                        >= MAX_CANDIDATES
                    {
                        return Err(invalid("candidate limit reached"));
                    }
                    (
                        private(container.create_child_directory(OsStr::new(&candidate))?)?,
                        true,
                    )
                }
                Err(error) => return Err(error),
            };
            let intent = Json::object([
                ("schema", Json::text("mesh.fleet-candidate-intent/v1")),
                ("candidate", Json::text(&candidate)),
                ("request", Json::text(request)),
                ("project", Json::text(self.id())),
                ("provenance", provenance.clone()),
                ("allocation", identity(&allocation)?),
                ("content_digest", Json::text(&content_digest)),
            ])
            .encode();
            if intent.len() > 65_536 {
                return Err(invalid("candidate provenance exceeds limit"));
            }
            if created {
                write(&allocation, "intent.json", intent.as_bytes())?;
            } else if recovery::read_json(&allocation, "intent.json")?.1 != intent {
                return Err(invalid("candidate request has different input"));
            }
            (container, allocation, intent, created)
        };
        let files = if created {
            write(&allocation, "content.json", content.as_bytes())?;
            let files = private(allocation.create_child_directory(OsStr::new("files"))?)?;
            let path = self
                .metadata_path()
                .join(CANDIDATES)
                .join(&candidate)
                .join("files");
            lanes::materialize(open, snapshot, &files, &path, limits)?;
            verify_fresh()?;
            ensure_project()?;
            super::detachment::ensure_attached(&self.store)?;
            container.ensure_namespace_identity()?;
            allocation.ensure_namespace_identity()?;
            files.ensure_namespace_identity()?;
            verify_manifest(&allocation, &content)?;
            lanes::verify_materialized(snapshot, &files, limits)?;
            write(
                &allocation,
                "ready.json",
                ready(&allocation, &files, &encoded_intent)?
                    .encode()
                    .as_bytes(),
            )?;
            files
        } else {
            private(allocation.open_child_directory(OsStr::new("files"))?)?
        };
        // Reopen the durable outcome even on the first acknowledgment. A copied-but-unfinished
        // candidate remains uncertain, rather than being silently completed during a retry.
        if recovery::read_json(&allocation, "ready.json")?.0
            != ready(&allocation, &files, &encoded_intent)?
        {
            return Err(invalid("candidate retained identity changed"));
        }
        verify_manifest(&allocation, &content)?;
        lanes::verify_materialized(snapshot, &files, limits)?;
        allocation.ensure_namespace_identity()?;
        container.ensure_namespace_identity()?;
        ensure_project()?;
        Ok(Json::object([
            ("schema", Json::text("mesh.fleet-project-candidate/v1")),
            ("candidate", Json::text(candidate)),
            ("project", Json::text(self.id())),
            ("provenance", provenance.clone()),
            ("content_digest", Json::text(content_digest)),
            ("files", Json::Number(snapshot.files.len() as u64)),
            (
                "directories",
                Json::Number(snapshot.directories.len() as u64),
            ),
            (
                "bytes",
                Json::Number(snapshot.files.iter().map(|file| file.byte_length).sum()),
            ),
            ("state", Json::text("staged")),
            ("approval_authority", Json::Bool(false)),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_attachment::AttachmentStorage;
    use ed25519_dalek::{Signer as _, SigningKey};
    use std::os::unix::fs::symlink;

    #[test]
    fn interrupted_candidates_and_changed_retained_content_are_never_repaired_by_retry() {
        let root = std::env::temp_dir().join(format!(
            "mesh-candidate-staging-fault-{}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let source = root.join("source");
        let metadata = root.join("metadata");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&metadata).unwrap();
        fs::write(source.join("work"), "exact saved content").unwrap();
        let history = AttachmentStorage::open(&metadata)
            .unwrap()
            .provision(&source)
            .unwrap();
        let captured = history
            .project()
            .capture_inputs(ObservationLimits::default())
            .unwrap();
        let key = SigningKey::from_bytes(&[72; 32]);
        let version = history
            .project()
            .save_capture(
                history.metadata_path(),
                &captured,
                mesh_types::PublicKey::from_bytes(key.verifying_key().to_bytes()),
                |payload| {
                    Ok::<_, String>(mesh_types::Signature::from_bytes(
                        key.sign(payload.as_bytes()).to_bytes(),
                    ))
                },
            )
            .unwrap()
            .operation();
        let open = OpenWorkspace::open_attachment_store(
            history.metadata_path(),
            history.store.clone(),
            false,
        )
        .unwrap();
        let snapshot = open.historical_workspace_preview(version).unwrap();
        let provenance = Json::object([("fixture", Json::Bool(true))]);
        let request = "a".repeat(32);
        let container = history.metadata_path().join(CANDIDATES);
        let interrupted = history.stage_fleet_candidate(
            &request,
            &provenance,
            &open,
            &snapshot,
            CandidateAdmission::Stage { main_matches: true },
            || Err(invalid("input changed after copy")),
        );
        assert!(interrupted.is_err());
        let partial = container.join(format!(
            "candidate-{}",
            digest(format!("{}:{request}", history.id()).as_bytes())
        ));
        assert!(partial.join("intent.json").exists());
        assert!(!partial.join("ready.json").exists());
        assert_eq!(
            fs::read(partial.join("files/work")).unwrap(),
            b"exact saved content"
        );
        let no_confirmation =
            || -> io::Result<()> { panic!("a retry must not finish an uncertain operation") };
        assert!(history
            .stage_fleet_candidate(
                &request,
                &provenance,
                &open,
                &snapshot,
                CandidateAdmission::Stage { main_matches: true },
                no_confirmation
            )
            .is_err());
        assert!(!partial.join("ready.json").exists());
        let request = "b".repeat(32);
        let complete = history
            .stage_fleet_candidate(
                &request,
                &provenance,
                &open,
                &snapshot,
                CandidateAdmission::Stage { main_matches: true },
                || Ok(()),
            )
            .unwrap();
        let retained = container.join(complete.get("candidate").unwrap().as_text().unwrap());
        // A lost acknowledgement can be recovered even when main has since moved.
        assert_eq!(
            history
                .stage_fleet_candidate(
                    &request,
                    &provenance,
                    &open,
                    &snapshot,
                    CandidateAdmission::Stage {
                        main_matches: false
                    },
                    no_confirmation
                )
                .unwrap(),
            complete
        );
        let original_intent = fs::read(retained.join("intent.json")).unwrap();
        let original_ready = fs::read(retained.join("ready.json")).unwrap();
        assert!(history
            .stage_fleet_candidate(
                &request,
                &Json::object([("fixture", Json::Bool(false))]),
                &open,
                &snapshot,
                CandidateAdmission::Stage { main_matches: true },
                no_confirmation,
            )
            .is_err());
        assert_eq!(
            fs::read(retained.join("intent.json")).unwrap(),
            original_intent
        );
        assert_eq!(
            fs::read(retained.join("ready.json")).unwrap(),
            original_ready
        );
        let saved_permissions = fs::metadata(retained.join("files/work"))
            .unwrap()
            .permissions();
        fs::set_permissions(
            retained.join("files/work"),
            <fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o700),
        )
        .unwrap();
        assert!(history
            .stage_fleet_candidate(
                &request,
                &provenance,
                &open,
                &snapshot,
                CandidateAdmission::Inspect,
                no_confirmation,
            )
            .is_err());
        assert_eq!(
            fs::read(retained.join("files/work")).unwrap(),
            b"exact saved content"
        );
        assert_eq!(
            std::os::unix::fs::PermissionsExt::mode(
                &fs::metadata(retained.join("files/work"))
                    .unwrap()
                    .permissions()
            ) & 0o777,
            0o700
        );
        assert_eq!(
            fs::read(retained.join("ready.json")).unwrap(),
            original_ready
        );
        fs::set_permissions(retained.join("files/work"), saved_permissions).unwrap();
        let original_manifest = fs::read(retained.join("content.json")).unwrap();
        fs::write(retained.join("content.json"), "changed manifest").unwrap();
        assert!(history
            .stage_fleet_candidate(
                &request,
                &provenance,
                &open,
                &snapshot,
                CandidateAdmission::Inspect,
                no_confirmation
            )
            .is_err());
        assert_eq!(
            fs::read(retained.join("content.json")).unwrap(),
            b"changed manifest"
        );
        fs::write(retained.join("content.json"), original_manifest).unwrap();
        let moved = retained.join("preserved-files");
        fs::rename(retained.join("files"), &moved).unwrap();
        symlink(&source, retained.join("files")).unwrap();
        assert!(history
            .stage_fleet_candidate(
                &request,
                &provenance,
                &open,
                &snapshot,
                CandidateAdmission::Inspect,
                no_confirmation
            )
            .is_err());
        assert_eq!(
            fs::read(source.join("work")).unwrap(),
            b"exact saved content"
        );
        fs::remove_file(retained.join("files")).unwrap();
        fs::rename(moved, retained.join("files")).unwrap();
        assert_eq!(
            history
                .stage_fleet_candidate(
                    &request,
                    &provenance,
                    &open,
                    &snapshot,
                    CandidateAdmission::Inspect,
                    no_confirmation
                )
                .unwrap(),
            complete
        );
        let raced_request = "d".repeat(32);
        let raced = container.join(format!(
            "candidate-{}",
            digest(format!("{}:{raced_request}", history.id()).as_bytes())
        ));
        assert!(history
            .stage_fleet_candidate(
                &raced_request,
                &provenance,
                &open,
                &snapshot,
                CandidateAdmission::Stage { main_matches: true },
                || {
                    fs::write(raced.join("files/work"), "concurrent retained edit")?;
                    Ok(())
                }
            )
            .is_err());
        assert!(!raced.join("ready.json").exists());
        assert_eq!(
            fs::read(raced.join("files/work")).unwrap(),
            b"concurrent retained edit"
        );
        for index in 3..MAX_CANDIDATES {
            fs::create_dir(container.join(format!("reserved-{index}"))).unwrap();
        }
        assert!(history
            .stage_fleet_candidate(
                &"c".repeat(32),
                &provenance,
                &open,
                &snapshot,
                CandidateAdmission::Stage { main_matches: true },
                no_confirmation
            )
            .is_err());
        assert_eq!(fs::read_dir(&container).unwrap().count(), MAX_CANDIDATES);
        assert_eq!(
            history
                .project()
                .saved_versions(history.metadata_path())
                .unwrap()
                .len(),
            1
        );
        fs::remove_dir_all(root).unwrap();
    }
}
