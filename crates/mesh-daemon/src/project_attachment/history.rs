//! Captured project history uses external native storage, never a managed source working folder.

use super::{
    external_store, invalid, read_receipt, CapturedProjectInput, ProjectAttachment, RECEIPT,
};
use crate::authenticated_changeset::CHANGESET_SIGNATURE_DOMAIN;
use crate::checkpoint_storage::{
    authenticated_checkpoint_identity, operation_checkpoint_signing_body,
    save_authenticated_checkpoint, AuthenticatedOperationCheckpointRequest, PreparedCheckpointFile,
};
use crate::ipc::Json;
use crate::root_authority::PinnedWorkspaceRoot;
use crate::workspace::{ManagedAuthoringBasis, OpenWorkspace};
use crate::ManifestPagingPolicy;
use mesh_cas::{Cas, DurableFs as _};
use mesh_chunking::ChunkingConfig;
use mesh_crypto::SigningPayload;
use mesh_operations::{
    ActorId, ActorSequence, CausalParents, HeadDerivation, HeadId, Hlc, ManifestId, NormalizedName,
    ObjectId, Operation, PolicyEpoch, PortableMetadata, SessionId, TransitionCommitment, VersionId,
    WorkspaceId,
};
use mesh_store::RecordDigest;
use mesh_types::{Blake3, ContentDigest as _, Digest32, PublicKey, Signature};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read as _};
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

pub(super) const HISTORY: &str = "attachment-history.json";
const ROOT: ObjectId = ObjectId::from_bytes([0; 16]);

/// Exact native history point. This conveys no approval or authority to modify the source project.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SavedAttachmentVersion {
    operation: RecordDigest,
}
impl SavedAttachmentVersion {
    pub(in crate::project_attachment) fn from_verified_history(
        history: &OpenWorkspace,
        operation: RecordDigest,
    ) -> io::Result<Self> {
        if !history
            .workspace_versions()
            .iter()
            .any(|v| v.operation() == operation)
        {
            return Err(invalid("saved operation is absent from verified history"));
        }
        Ok(Self { operation })
    }
    /// Immutable ChangeSet identity used by saved history and review.
    pub fn operation(&self) -> RecordDigest {
        self.operation
    }
}

impl ProjectAttachment {
    /// Save one complete admitted capture into its external store. Signing attests to the capture,
    /// not the identity of the person or tool that edited the observed files. The original files
    /// are never opened for writing, locked, copied into a working folder, or reread for content.
    pub fn save_capture<F, E>(
        &self,
        metadata: &Path,
        input: &CapturedProjectInput,
        actor: PublicKey,
        sign: F,
    ) -> io::Result<SavedAttachmentVersion>
    where
        F: FnOnce(&SigningPayload) -> Result<Signature, E>,
        E: std::fmt::Display,
    {
        let store = self.history_store(metadata)?;
        self.save_capture_in_store(metadata, input, actor, sign, store)
    }

    pub(super) fn save_capture_in_store<F, E>(
        &self,
        metadata: &Path,
        input: &CapturedProjectInput,
        actor: PublicKey,
        sign: F,
        store: PinnedWorkspaceRoot,
    ) -> io::Result<SavedAttachmentVersion>
    where
        F: FnOnce(&SigningPayload) -> Result<Signature, E>,
        E: std::fmt::Display,
    {
        self.ensure_current()?;
        store.ensure_namespace_identity()?;
        if read_receipt(&store)? != self.receipt()?.encode() {
            return Err(invalid("history is not registered to this project"));
        }
        if input.root() != self.root() || input.identity() != (self.device, self.inode) {
            return Err(invalid("capture belongs to a different attached project"));
        }
        let _guard =
            crate::workspace_custody::lock_workspace_initialization(&store).map_err(error)?;
        super::detachment::ensure_attached(&store)?;
        let (configuration, created) =
            self.history_configuration(&store, Some(input.exclusion_digest()))?;
        let workspace_id = WorkspaceId::from_bytes(short_id(configuration.as_bytes()));
        let mut workspace = OpenWorkspace::open_attachment_store(metadata, store.clone(), created)
            .map_err(error)?;
        let line = super::capture_line::CaptureLine::load(&store, &workspace, &configuration)?;
        line.persist(&store, &configuration)?;
        self.enable_capture_line(&store, &configuration, &workspace, line.head)?;
        let basis = if let Some(head) = line.head {
            let basis = workspace
                .historical_authoring_basis(head, actor)
                .map_err(error)?;
            if basis.workspace_id != workspace_id {
                return Err(invalid(
                    "attachment history identity differs from its binding",
                ));
            }
            basis
        } else {
            ManagedAuthoringBasis {
                workspace_id,
                actor_id: ActorId::from_bytes(*actor.as_bytes()),
                session_id: SessionId::from_bytes(short_id(actor.as_bytes())),
                actor_sequence: ActorSequence::FIRST,
                causal_parents: CausalParents::genesis(),
                base_head: HeadId::from_bytes([0; 32]),
                policy_epoch: PolicyEpoch::new(1),
                hybrid_logical_time: Hlc::new(0, 0),
            }
        };
        let (operations, files) = prepare_snapshot(&workspace, input, &basis, line.head)?;
        if operations.is_empty() {
            return line
                .head
                .map(|operation| SavedAttachmentVersion { operation })
                .ok_or_else(|| {
                    invalid("an initial empty project has no representable history point yet")
                });
        }
        if let Some(head) = line.head {
            workspace
                .prepare_historical_operations(head, actor, &operations)
                .map_err(error)?;
        }
        let make_request = |signature| {
            AuthenticatedOperationCheckpointRequest::new(
                basis.workspace_id,
                basis.actor_id,
                basis.session_id,
                basis.actor_sequence,
                basis.causal_parents.clone(),
                basis.base_head,
                basis.policy_epoch,
                basis.hybrid_logical_time,
                operations.clone(),
                actor,
                signature,
            )
        };
        let unsigned = make_request(Signature::from_bytes([0; 64]));
        let signature = sign(&SigningPayload::new(
            CHANGESET_SIGNATURE_DOMAIN,
            &operation_checkpoint_signing_body(&unsigned, &CaptureHead),
        ))
        .map_err(error)?;
        // The signer can yield to the UI or an external key service. Recheck all retained authority
        // after it returns, without replacing captured content with newer source bytes.
        self.ensure_current()?;
        store.ensure_namespace_identity()?;
        if read_receipt(&store)? != self.receipt()?.encode() {
            return Err(invalid("attachment registration changed"));
        }
        if self
            .history_configuration(&store, Some(input.exclusion_digest()))?
            .0
            != configuration
        {
            return Err(invalid("attachment history binding changed while signing"));
        }
        super::detachment::ensure_attached(&store)?;
        let cas =
            Cas::with_filesystem(metadata.to_path_buf(), store.filesystem()).map_err(error)?;
        let request = make_request(signature);
        let exact = authenticated_checkpoint_identity(&request, &CaptureHead).map_err(error)?;
        line.begin(exact, &store, &configuration)?;
        let saved =
            save_authenticated_checkpoint(&mut workspace, &cas, request, files, &CaptureHead)
                .map_err(error)?;
        drop(workspace);
        let reopened =
            OpenWorkspace::open_attachment_store(metadata, store.clone(), false).map_err(error)?;
        let expected = input
            .directories()
            .iter()
            .map(|p| path_text(p).map(str::to_owned))
            .chain(
                input
                    .files()
                    .iter()
                    .map(|f| path_text(f.path()).map(str::to_owned)),
            )
            .collect::<io::Result<BTreeSet<_>>>()?;
        let actual = reopened
            .historical_capture_entries(saved.changeset_id())
            .map_err(error)?
            .into_keys()
            .collect::<BTreeSet<_>>();
        if exact != saved.changeset_id()
            || expected != actual
            || !reopened.has_operation(&saved.changeset_id())
        {
            return Err(invalid("saved attachment history needs reconciliation"));
        }
        line.finish(saved.changeset_id(), &store, &configuration)?;
        store.ensure_namespace_identity()?;
        Ok(SavedAttachmentVersion {
            operation: saved.changeset_id(),
        })
    }

    /// Reopen immutable history points after restart without creating a missing store or journal.
    pub fn saved_versions(&self, metadata: &Path) -> io::Result<Vec<SavedAttachmentVersion>> {
        let store = self.history_store(metadata)?;
        self.saved_versions_in_store(metadata, store)
    }

    pub(super) fn saved_versions_in_store(
        &self,
        metadata: &Path,
        store: PinnedWorkspaceRoot,
    ) -> io::Result<Vec<SavedAttachmentVersion>> {
        self.with_read_history(
            metadata,
            store,
            &crate::TrustedReviewers::default(),
            |workspace, store, configuration| {
                let line = super::capture_line::CaptureLine::load(store, workspace, configuration)?;
                Ok(workspace
                    .linear_history(line.head)
                    .map_err(error)?
                    .into_iter()
                    .map(|operation| SavedAttachmentVersion { operation })
                    .collect())
            },
        )
    }

    /// Read exact saved bytes from external history; this never falls back to a current source file.
    pub fn saved_file(
        &self,
        metadata: &Path,
        version: SavedAttachmentVersion,
        relative: &str,
    ) -> io::Result<Option<Vec<u8>>> {
        let store = self.history_store(metadata)?;
        self.with_read_history(
            metadata,
            store,
            &crate::TrustedReviewers::default(),
            |workspace, _, _| {
                Ok(workspace
                    .historical_workspace_file(version.operation, relative)
                    .map_err(error)?
                    .map(|file| file.bytes))
            },
        )
    }

    fn history_store(&self, metadata: &Path) -> io::Result<PinnedWorkspaceRoot> {
        self.ensure_current()?;
        let store = external_store(metadata, self)?;
        if read_receipt(&store)? != self.receipt()?.encode() {
            return Err(invalid("history is not registered to this project"));
        }
        Ok(store)
    }

    pub(super) fn enable_capture_line(
        &self,
        store: &PinnedWorkspaceRoot,
        configuration: &str,
        workspace: &OpenWorkspace,
        head: Option<RecordDigest>,
    ) -> io::Result<()> {
        let file = store.filesystem().inspect_entry(Path::new(HISTORY))?;
        let mut bytes = String::new();
        file.take(65_537).read_to_string(&mut bytes)?;
        let next = capture_history_binding(configuration).encode();
        if bytes == next {
            return store.ensure_namespace_identity();
        }
        if bytes != configuration
            || workspace.linear_history(head).map_err(error)?.len() != workspace.operations()
        {
            return Err(invalid("legacy capture history changed before separation"));
        }
        let temporary = Path::new("attachment-history.pending");
        let filesystem = store.filesystem();
        match filesystem.inspect_entry(temporary) {
            Ok(file) => {
                let metadata = file.metadata()?;
                if !metadata.is_file()
                    || metadata.nlink() != 1
                    || metadata.permissions().mode() & 0o077 != 0
                    || metadata.len() > 65_536
                {
                    return Err(invalid("history migration is not a bounded private file"));
                }
                let mut retained = String::new();
                file.take(65_537).read_to_string(&mut retained)?;
                if retained != next {
                    return Err(invalid("history migration needs reconciliation"));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => filesystem.write_new_file(
                temporary,
                next.as_bytes(),
                fs::Permissions::from_mode(0o600),
            )?,
            Err(error) => return Err(error),
        }
        store.ensure_namespace_identity()?;
        filesystem.rename(temporary, Path::new(HISTORY))?;
        store.sync()?;
        store.ensure_namespace_identity()
    }

    pub(super) fn history_configuration(
        &self,
        store: &PinnedWorkspaceRoot,
        policy: Option<Digest32>,
    ) -> io::Result<(String, bool)> {
        let previous = match store.filesystem().inspect_entry(Path::new(HISTORY)) {
            Ok(file) => {
                if !file.metadata()?.is_file() {
                    return Err(invalid("attachment history binding is not a regular file"));
                }
                let mut bytes = Vec::new();
                file.take(65_537).read_to_end(&mut bytes)?;
                if bytes.len() > 65_536 {
                    return Err(invalid("attachment history binding is too large"));
                }
                Some(String::from_utf8(bytes).map_err(error)?)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        self.history_configuration_with_previous(store, policy, previous)
    }

    pub(super) fn history_configuration_with_previous(
        &self,
        store: &PinnedWorkspaceRoot,
        policy: Option<Digest32>,
        previous: Option<String>,
    ) -> io::Result<(String, bool)> {
        let previous = previous
            .map(|previous| -> io::Result<String> {
                let value = Json::parse(&previous).map_err(error)?;
                if value.get("schema").and_then(Json::as_text) == Some("mesh.attachment-history/v2")
                {
                    let basis = value
                        .get("capture_basis")
                        .and_then(Json::as_text)
                        .ok_or_else(|| invalid("missing original history binding"))?;
                    if previous != capture_history_binding(basis).encode()
                        || !super::capture_line::CaptureLine::exists(store)?
                    {
                        return Err(invalid("separated capture history needs reconciliation"));
                    }
                    Ok(basis.to_owned())
                } else {
                    Ok(previous)
                }
            })
            .transpose()?;
        let policy = match (policy, previous.as_ref()) {
            (Some(policy), _) => policy.to_string(),
            (None, Some(previous)) => Json::parse(previous)
                .map_err(error)?
                .get("exclusions")
                .and_then(Json::as_text)
                .ok_or_else(|| invalid("history exclusions are missing"))?
                .to_owned(),
            (None, None) => return Err(invalid("attachment history has not been initialized")),
        };
        let identity = store.identity()?;
        let expected = Json::object([
            ("schema", Json::text("mesh.attachment-history/v1")),
            ("attachment", self.receipt()?),
            ("store_device", Json::text(format!("{:016x}", identity.0))),
            ("store_inode", Json::text(format!("{:016x}", identity.1))),
            ("exclusions", Json::text(policy)),
        ])
        .encode();
        if let Some(previous) = previous {
            if previous != expected {
                return Err(invalid(
                    "attachment history identity or exclusion policy changed",
                ));
            }
            return Ok((expected, false));
        }
        let names = store
            .filesystem()
            .read_directory_names_bounded(Path::new(""), 2)?;
        if names.len() != 1 || names[0] != RECEIPT {
            return Err(invalid(
                "unrecognized files occupy the attachment history store",
            ));
        }
        store.filesystem().write_new_file(
            Path::new(HISTORY),
            expected.as_bytes(),
            fs::Permissions::from_mode(0o600),
        )?;
        Ok((expected, true))
    }
}

pub(super) fn verify_history_binding(
    workspace: &OpenWorkspace,
    configuration: &str,
) -> io::Result<()> {
    if workspace.operations() > 0
        && workspace.journal_workspace_id().map_err(error)?
            != WorkspaceId::from_bytes(short_id(configuration.as_bytes()))
    {
        return Err(invalid(
            "attachment history identity differs from its binding",
        ));
    }
    Ok(())
}

fn capture_history_binding(configuration: &str) -> Json {
    Json::object([
        ("schema", Json::text("mesh.attachment-history/v2")),
        ("capture_basis", Json::text(configuration)),
    ])
}

fn prepare_snapshot(
    workspace: &OpenWorkspace,
    input: &CapturedProjectInput,
    basis: &ManagedAuthoringBasis,
    predecessor: Option<RecordDigest>,
) -> io::Result<(Vec<Operation>, Vec<PreparedCheckpointFile>)> {
    let desired = input
        .directories()
        .iter()
        .map(|path| Ok((path_text(path)?.to_owned(), true)))
        .chain(
            input
                .files()
                .iter()
                .map(|file| Ok((path_text(file.path())?.to_owned(), false))),
        )
        .collect::<io::Result<BTreeMap<_, _>>>()?;
    let captured = predecessor
        .map(|head| workspace.historical_capture_entries(head).map_err(error))
        .transpose()?
        .unwrap_or_default();
    let existing = captured
        .iter()
        .map(|(path, entry)| (path.as_str(), entry.binding.is_directory))
        .collect::<BTreeMap<_, _>>();
    let mut removed = existing
        .iter()
        .filter(|(path, directory)| desired.get(**path) != Some(directory))
        .map(|(path, _)| *path)
        .collect::<Vec<_>>();
    removed.sort_by_key(|path| std::cmp::Reverse((path.matches('/').count(), *path)));
    let mut operations = Vec::new();
    for path in removed {
        let binding = &captured[path].binding;
        operations.push(Operation::UnlinkDirectoryEntry {
            directory_id: binding.parent_id,
            name: binding.name.clone(),
            object_id: binding.object_id,
        });
    }
    let mut seed = Vec::new();
    seed.extend_from_slice(basis.workspace_id.as_bytes());
    seed.extend_from_slice(basis.actor_id.as_bytes());
    seed.extend_from_slice(&basis.actor_sequence.value().to_be_bytes());
    seed.extend_from_slice(basis.base_head.as_bytes());
    let object = |path: &Path, kind: u8| -> io::Result<ObjectId> {
        let mut bytes = seed.clone();
        bytes.push(kind);
        bytes.extend_from_slice(path_text(path)?.as_bytes());
        Ok(ObjectId::from_bytes(short_id(&bytes)))
    };
    let mut directories = input.directories().to_vec();
    directories.sort_by_key(|path| (path.components().count(), path.clone()));
    let mut ids = BTreeMap::from([(PathBuf::new(), ROOT)]);
    for path in directories {
        if existing.get(path_text(&path)?) == Some(&true) {
            let binding = &captured[path_text(&path)?].binding;
            ids.insert(path, binding.object_id);
            continue;
        }
        let id = object(&path, 0)?;
        operations.push(Operation::CreateDirectory { object_id: id });
        operations.push(Operation::LinkDirectoryEntry {
            directory_id: parent_id(&path, &ids)?,
            name: name(&path)?,
            object_id: id,
            version_id: VersionId::from_bytes([0; 32]),
        });
        ids.insert(path, id);
    }
    let mut files = Vec::new();
    for file in input.files() {
        let path = path_text(file.path())?;
        let previous = captured
            .get(path)
            .filter(|entry| !entry.binding.is_directory);
        let previous_version = previous.and_then(|entry| entry.file);
        if let Some((_, digest, executable)) = previous_version {
            if digest.as_bytes() == file.digest().as_bytes() && executable == file.executable() {
                continue;
            }
        }
        let id = match previous {
            Some(entry) => entry.binding.object_id,
            None => object(file.path(), 1)?,
        };
        let prepared = PreparedCheckpointFile::from_bytes(
            file.bytes(),
            &ChunkingConfig::default(),
            ManifestPagingPolicy::flat(),
        )
        .map_err(error)?;
        let mut version_input = seed.clone();
        version_input.extend_from_slice(id.as_bytes());
        version_input.extend_from_slice(file.digest().as_bytes());
        version_input.push(u8::from(file.executable()));
        let version = VersionId::from_bytes(*Blake3::digest_bytes(&version_input).as_bytes());
        if previous.is_none() {
            operations.push(Operation::CreateFile { object_id: id });
        }
        operations.push(Operation::WriteFileVersion {
            object_id: id,
            version_id: version,
            parent_versions: previous_version
                .into_iter()
                .map(|(version, _, _)| version)
                .collect(),
            manifest_id: ManifestId::from_bytes(*prepared.manifest().id.as_bytes()),
            portable_metadata: PortableMetadata::new(file.executable()),
        });
        if previous.is_none() {
            operations.push(Operation::LinkDirectoryEntry {
                directory_id: parent_id(file.path(), &ids)?,
                name: name(file.path())?,
                object_id: id,
                version_id: version,
            });
        }
        files.push(prepared);
    }
    Ok((operations, files))
}
fn parent_id(path: &Path, ids: &BTreeMap<PathBuf, ObjectId>) -> io::Result<ObjectId> {
    ids.get(path.parent().unwrap_or(Path::new("")))
        .copied()
        .ok_or_else(|| invalid("captured parent directory is absent"))
}
fn name(path: &Path) -> io::Result<NormalizedName> {
    NormalizedName::new(
        path.file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| invalid("invalid captured name"))?
            .to_owned(),
    )
    .map_err(error)
}
fn path_text(path: &Path) -> io::Result<&str> {
    path.to_str()
        .ok_or_else(|| invalid("invalid captured path"))
}
pub(super) fn short_id(bytes: &[u8]) -> [u8; 16] {
    let mut framed = b"mesh.attachment-history/v1\0".to_vec();
    framed.extend_from_slice(bytes);
    Blake3::digest_bytes(&framed).as_bytes()[..16]
        .try_into()
        .expect("fixed digest width")
}
fn error(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}
struct CaptureHead;
impl HeadDerivation for CaptureHead {
    fn resulting_head(&self, commitment: &TransitionCommitment) -> HeadId {
        HeadId::from_bytes(*Blake3::digest_bytes(&commitment.canonical_bytes()).as_bytes())
    }
}

#[cfg(test)]
#[path = "capture_line_tests.rs"]
mod capture_line_tests;

#[path = "dependency_capture.rs"]
pub(super) mod dependency_capture;
pub use dependency_capture::{NativeCaptureRetention, PreparedNativeCapture};
