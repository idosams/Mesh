//! Independent ordinary folders from exact attached history. Providers and exclusive custody are
//! optional; this records allocation ancestry, never the author of later filesystem changes.
use super::{
    invalid, provisioning::valid_id, recovery, AttachmentStorage, ObservationLimits,
    ProvisionedAttachment,
};
use crate::ipc::Json;
use crate::managed_file::retained_replacement::observe_file;
use crate::root_authority::PinnedWorkspaceRoot;
use mesh_cas::DurableFs as _;
use mesh_types::{Blake3, ContentDigest as _};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

const ROOT: &str = "work-lanes";
const INTENT: &str = "intent.json";
const READY: &str = "ready.json";

#[cfg(test)]
type AllocationRace = Box<dyn FnOnce(&Path)>;

#[cfg(test)]
thread_local! {
    static AFTER_COPY: std::cell::RefCell<Option<AllocationRace>> = std::cell::RefCell::new(None);
}

fn failure(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}
fn request_valid(request: &str) -> bool {
    request.len() == 32
        && request
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn name(project: &str, request: &str) -> String {
    format!(
        "lane-{}",
        Blake3::digest_bytes(format!("{project}:{request}").as_bytes())
    )
}
fn private_child(parent: &PinnedWorkspaceRoot, name: &str) -> io::Result<PinnedWorkspaceRoot> {
    let child = match parent.create_child_directory(OsStr::new(name)) {
        Ok(child) => child,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            parent.open_child_directory(OsStr::new(name))?
        }
        Err(error) => return Err(error),
    };
    if child
        .try_clone_directory()?
        .metadata()?
        .permissions()
        .mode()
        & 0o077
        != 0
    {
        return Err(invalid("lane allocation storage must be private"));
    }
    Ok(child)
}
fn intent(
    root: &PinnedWorkspaceRoot,
    source: &str,
    version: &str,
    request: &str,
) -> io::Result<Json> {
    let (device, inode) = root.identity()?;
    Ok(Json::object([
        ("schema", Json::text("mesh.attachment-lane-intent/v1")),
        ("source_project", Json::text(source)),
        ("source_version", Json::text(version)),
        ("request", Json::text(request)),
        ("allocation_device", Json::text(format!("{device:016x}"))),
        ("allocation_inode", Json::text(format!("{inode:016x}"))),
    ]))
}
fn ready(child: &ProvisionedAttachment, intent: &str) -> io::Result<Json> {
    Ok(Json::object([
        ("schema", Json::text("mesh.attachment-lane-ready/v1")),
        (
            "intent_digest",
            Json::text(Blake3::digest_bytes(intent.as_bytes()).to_string()),
        ),
        ("project", Json::text(child.id())),
        ("attachment", child.project().receipt()?),
    ]))
}
fn write(root: &PinnedWorkspaceRoot, name: &str, value: &Json) -> io::Result<()> {
    root.filesystem().write_new_file(
        Path::new(name),
        value.encode().as_bytes(),
        fs::Permissions::from_mode(0o600),
    )
}
fn inventory(root: &PinnedWorkspaceRoot, limit: usize) -> io::Result<BTreeMap<String, bool>> {
    let mut found = BTreeMap::new();
    let mut pending = vec![PathBuf::new()];
    while let Some(directory) = pending.pop() {
        let entries = root
            .filesystem()
            .read_directory_names_bounded(&directory, limit.saturating_sub(found.len()))?;
        for name in entries {
            let path = directory.join(name);
            let text = path
                .to_str()
                .ok_or_else(|| invalid("lane path is not text"))?;
            let file = root.filesystem().inspect_entry(&path)?;
            let metadata = file.metadata()?;
            if found.len() >= limit || found.insert(text.to_owned(), metadata.is_dir()).is_some() {
                return Err(invalid("lane inventory exceeds its bound"));
            }
            if metadata.is_dir() {
                pending.push(path);
            } else if !metadata.is_file() {
                return Err(invalid("unsupported lane entry"));
            }
        }
    }
    root.ensure_namespace_identity()?;
    Ok(found)
}

impl AttachmentStorage {
    fn canonical_lane_storage(&self) -> io::Result<PathBuf> {
        self.pinned.ensure_namespace_identity()?;
        let path = self.path.canonicalize()?;
        let current = super::pin_absolute_directory(&path)?;
        if current.identity()? != self.pinned.identity()? {
            return Err(invalid("lane storage identity changed"));
        }
        self.pinned.ensure_namespace_identity()?;
        Ok(path)
    }
    /// Allocate an independent ordinary folder from exact verified saved content. The native
    /// storage root chooses every destination. A stable request retries only the same allocation;
    /// partial allocations are retained and refused, never overwritten or silently duplicated.
    pub fn open_version_lane(
        &self,
        source: &ProvisionedAttachment,
        version: &str,
        request: &str,
        limits: ObservationLimits,
    ) -> io::Result<ProvisionedAttachment> {
        limits.validate()?;
        if !request_valid(request) {
            return Err(invalid("invalid lane request"));
        }
        let admitted = self.reopen(source.id())?;
        if admitted.store.identity()? != source.store.identity()? {
            return Err(invalid("lane source belongs to another storage root"));
        }
        source.attachment.inspect_saved(
            source.metadata_path(),
            source.store.clone(),
            version,
            |workspace, operation| {
                let snapshot = workspace
                    .historical_workspace_preview(operation)
                    .map_err(failure)?;
                check_budget(&snapshot, limits)?;
                self.pinned.ensure_namespace_identity()?;
                let lanes = private_child(&self.pinned, ROOT)?;
                let allocation_name = name(source.id(), request);
                let path = self
                    .canonical_lane_storage()?
                    .join(ROOT)
                    .join(&allocation_name)
                    .join("files");
                let allocation = match lanes.create_child_directory(OsStr::new(&allocation_name)) {
                    Ok(root) => root,
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                        let root = lanes.open_child_directory(OsStr::new(&allocation_name))?;
                        let expected = intent(&root, source.id(), version, request)?.encode();
                        let (_, original) = recovery::read_json(&root, INTENT)?;
                        if expected != original {
                            return Err(invalid("lane request was reused with different input"));
                        }
                        // A ready record must precede reuse. Never turn a partial allocation into a
                        // fresh registration, or copy old bytes over work created after a lost reply.
                        let (recorded, _) = recovery::read_json(&root, READY)?;
                        let id = recovery::text(&recorded, "project")?;
                        let child = self.reopen(id)?;
                        if child.project().root() != path || recorded != ready(&child, &original)? {
                            return Err(invalid("lane allocation binding changed"));
                        }
                        root.ensure_namespace_identity()?;
                        return Ok(child);
                    }
                    Err(error) => return Err(error),
                };
                let started = intent(&allocation, source.id(), version, request)?;
                write(&allocation, INTENT, &started)?;
                let files = allocation.create_child_directory(OsStr::new("files"))?;
                materialize(workspace, &snapshot, &files, &path, limits)?;
                let child = self.provision(&path)?;
                write(&allocation, READY, &ready(&child, &started.encode())?)?;
                files.ensure_namespace_identity()?;
                allocation.ensure_namespace_identity()?;
                lanes.ensure_namespace_identity()?;
                self.pinned.ensure_namespace_identity()?;
                Ok(child)
            },
        )
    }

    /// Recorded allocation ancestry for a registered child. This conveys no authorship or
    /// provider/session association. Missing or changed receipts refuse instead of inventing one.
    pub fn lane_origin(&self, child: &ProvisionedAttachment) -> io::Result<Option<Json>> {
        let root = self.canonical_lane_storage()?.join(ROOT);
        let Ok(relative) = child.project().root().strip_prefix(&root) else {
            return Ok(None);
        };
        let parts: Vec<_> = relative.components().collect();
        if parts.len() != 2 || parts[1].as_os_str() != "files" {
            return Err(invalid("invalid lane location"));
        }
        let lanes = self.pinned.open_child_directory(OsStr::new(ROOT))?;
        let allocation = lanes.open_child_directory(parts[0].as_os_str())?;
        let (recorded, raw) = recovery::read_json(&allocation, INTENT)?;
        let project = recovery::text(&recorded, "source_project")?;
        let version = recovery::text(&recorded, "source_version")?;
        let request = recovery::text(&recorded, "request")?;
        if !valid_id(project)
            || !valid_id(version)
            || !request_valid(request)
            || parts[0].as_os_str() != OsStr::new(&name(project, request))
            || raw != intent(&allocation, project, version, request)?.encode()
        {
            return Err(invalid("lane origin identity changed"));
        }
        let (finished, _) = recovery::read_json(&allocation, READY)?;
        if finished != ready(child, &raw)? {
            return Err(invalid("lane origin binding changed"));
        }
        child.project().ensure_current()?;
        allocation.ensure_namespace_identity()?;
        Ok(Some(Json::object([
            ("schema", Json::text("mesh.attachment-lane-origin/v1")),
            ("source_project", Json::text(project)),
            ("source_version", Json::text(version)),
            ("request", Json::text(request)),
            ("attribution", Json::text("unknown")),
            ("provider", Json::Null),
        ])))
    }
}

pub(super) fn check_budget(
    snapshot: &crate::workspace::HistoricalWorkspacePreview,
    limits: ObservationLimits,
) -> io::Result<()> {
    if snapshot
        .files
        .len()
        .saturating_add(snapshot.directories.len())
        > limits.entries
        || snapshot
            .files
            .iter()
            .any(|file| file.byte_length > limits.file_bytes)
        || snapshot
            .files
            .iter()
            .try_fold(0_u64, |bytes, file| bytes.checked_add(file.byte_length))
            .is_none_or(|bytes| bytes > limits.bytes)
    {
        return Err(invalid("saved lane content exceeds the allocation budget"));
    }
    Ok(())
}

pub(super) fn materialize(
    workspace: &crate::workspace::OpenWorkspace,
    snapshot: &crate::workspace::HistoricalWorkspacePreview,
    files: &PinnedWorkspaceRoot,
    path: &Path,
    limits: ObservationLimits,
) -> io::Result<()> {
    #[cfg(not(test))]
    let _ = path;
    let mut directories = snapshot.directories.iter().collect::<Vec<_>>();
    directories.sort_by_key(|directory| directory.path.split('/').count());
    for directory in &directories {
        files
            .filesystem()
            .create_dir_all(Path::new(&directory.path))?;
    }
    for file in &snapshot.files {
        files.filesystem().write_new_file_with(
            Path::new(&file.path),
            fs::Permissions::from_mode(if file.executable { 0o700 } else { 0o600 }),
            |output| {
                workspace
                    .write_historical_workspace_file(file, output)
                    .map_err(|_| invalid("saved lane content could not be verified while writing"))
            },
        )?;
    }
    #[cfg(test)]
    AFTER_COPY.with(|hook| {
        if let Some(change) = hook.borrow_mut().take() {
            change(path);
        }
    });
    verify_materialized(snapshot, files, limits)?;
    files.sync()?;
    files.ensure_namespace_identity()?;
    Ok(())
}

pub(super) fn verify_materialized(
    snapshot: &crate::workspace::HistoricalWorkspacePreview,
    files: &PinnedWorkspaceRoot,
    limits: ObservationLimits,
) -> io::Result<()> {
    let expected: BTreeMap<_, _> = snapshot
        .directories
        .iter()
        .map(|entry| (entry.path.clone(), true))
        .chain(
            snapshot
                .files
                .iter()
                .map(|entry| (entry.path.clone(), false)),
        )
        .collect();
    if inventory(files, limits.entries)? != expected {
        return Err(invalid("lane entries changed during allocation"));
    }
    for file in &snapshot.files {
        let observed = observe_file(files, Path::new(&file.path), limits.file_bytes)?;
        if observed.digest != file.content_digest.to_string()
            || observed.bytes != file.byte_length
            || (observed.mode & 0o111 != 0) != file.executable
        {
            return Err(invalid("lane content changed during allocation"));
        }
    }
    files.ensure_namespace_identity()?;
    Ok(())
}

impl ProvisionedAttachment {
    /// Inspect the exact attached input and verified main under one retained history lock.
    /// This exposes no source bytes, mutation or approval authority to fleet callers.
    pub(crate) fn with_fleet_input<T>(
        &self,
        version: mesh_store::RecordDigest,
        trusted: &crate::TrustedReviewers,
        read: impl FnOnce(&crate::workspace::OpenWorkspace, Json) -> io::Result<T>,
    ) -> io::Result<T> {
        self.attachment.with_review_history(
            self.metadata_path(),
            self.store.clone(),
            trusted,
            |workspace, _| {
                if !workspace
                    .workspace_versions()
                    .iter()
                    .any(|v| v.operation() == version)
                {
                    return Err(invalid("fleet input is not an attached project version"));
                }
                let main = match super::approval::main_head(workspace)? {
                    None => Json::Null,
                    Some(head) => {
                        let review = workspace
                            .accepted_main_review()
                            .map_err(failure)?
                            .ok_or_else(|| invalid("verified main review unavailable"))?;
                        Json::object([
                            ("head", Json::text(head.to_string())),
                            ("bundle", Json::text(review.bundle.to_string())),
                            ("target", Json::text(review.subject_operation.to_string())),
                        ])
                    }
                };
                read(workspace, main)
            },
        )
    }

    pub(crate) fn validate_lane_version(&self, version: &str) -> io::Result<()> {
        self.attachment.inspect_saved(
            self.metadata_path(),
            self.store.clone(),
            version,
            |workspace, operation| {
                let snapshot = workspace
                    .historical_workspace_preview(operation)
                    .map_err(failure)?;
                check_budget(&snapshot, ObservationLimits::default())
            },
        )
    }

    pub(crate) fn protected_source(&self) -> io::Result<crate::ProtectedWorkspaceRoot> {
        self.attachment.ensure_current()?;
        crate::ProtectedWorkspaceRoot::from_directory_token(&format!(
            "{:016x}:{:016x}",
            self.attachment.device, self.attachment.inode
        ))
    }

    /// Internal create-only export for managed agents. The verified snapshot remains the authority
    /// for subsequent import verification; changed staging bytes cannot become a different input.
    pub(crate) fn materialize_saved_version(
        &self,
        version: &str,
        files: &PinnedWorkspaceRoot,
        path: &Path,
    ) -> io::Result<crate::workspace::HistoricalWorkspacePreview> {
        if files.is_within(self.protected_source()?)? {
            return Err(invalid(
                "agent allocation must remain outside the original project",
            ));
        }
        let limits = ObservationLimits::default();
        self.attachment.inspect_saved(
            self.metadata_path(),
            self.store.clone(),
            version,
            |workspace, operation| {
                let snapshot = workspace
                    .historical_workspace_preview(operation)
                    .map_err(failure)?;
                check_budget(&snapshot, limits)?;
                materialize(workspace, &snapshot, files, path, limits)?;
                Ok(snapshot)
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer as _, SigningKey};

    #[test]
    fn allocation_refuses_an_empty_directory_replaced_during_materialization() {
        let root = std::env::temp_dir().join(format!("mesh-lane-type-race-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let source = root.join("source");
        fs::create_dir(&source).unwrap();
        fs::create_dir(source.join("empty")).unwrap();
        fs::write(source.join("work"), "saved").unwrap();
        let storage_path = root.join("metadata");
        fs::create_dir(&storage_path).unwrap();
        let storage = AttachmentStorage::open(&storage_path).unwrap();
        let source = storage.provision(&source).unwrap();
        let key = SigningKey::from_bytes(&[72; 32]);
        let capture = source
            .project()
            .capture_inputs(ObservationLimits::default())
            .unwrap();
        let version = source
            .project()
            .save_capture(
                source.metadata_path(),
                &capture,
                mesh_types::PublicKey::from_bytes(key.verifying_key().to_bytes()),
                |body| {
                    Ok::<_, String>(mesh_types::Signature::from_bytes(
                        key.sign(body.as_bytes()).to_bytes(),
                    ))
                },
            )
            .unwrap()
            .operation()
            .to_string();
        AFTER_COPY.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(|files| {
                fs::remove_dir(files.join("empty")).unwrap();
                fs::write(files.join("empty"), "concurrent work").unwrap();
            }))
        });
        let result = storage.open_version_lane(
            &source,
            &version,
            &"a".repeat(32),
            ObservationLimits::default(),
        );
        let allocation = storage_path
            .join(ROOT)
            .join(name(source.id(), &"a".repeat(32)));
        let refused = result.is_err();
        let preserved = fs::read(allocation.join("files/empty")).unwrap();
        let ready = allocation.join(READY).exists();
        fs::remove_dir_all(root).unwrap();
        assert!(
            refused,
            "same path with a different entry type must refuse allocation"
        );
        assert!(!ready);
        assert_eq!(preserved, b"concurrent work");
    }
}
