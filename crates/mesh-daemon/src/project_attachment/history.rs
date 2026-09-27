//! Captured project history uses external native storage, never a managed source working folder.

use super::{
    external_store, invalid, read_receipt, CapturedProjectInput, ProjectAttachment, RECEIPT,
};
use crate::authenticated_changeset::CHANGESET_SIGNATURE_DOMAIN;
use crate::checkpoint_storage::{
    operation_checkpoint_signing_body, save_authenticated_checkpoint,
    AuthenticatedOperationCheckpointRequest, PreparedCheckpointFile,
};
use crate::ipc::Json;
use crate::root_authority::PinnedWorkspaceRoot;
use crate::workspace::{ManagedAuthoringBasis, OpenWorkspace};
use crate::ManifestPagingPolicy;
use mesh_cas::Cas;
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
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

pub(super) const HISTORY: &str = "attachment-history.json";
const ROOT: ObjectId = ObjectId::from_bytes([0; 16]);

/// Exact native history point. This conveys no approval or authority to modify the source project.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SavedAttachmentVersion {
    operation: RecordDigest,
}
impl SavedAttachmentVersion {
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
        let basis = if workspace.operations() == 0 {
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
        } else {
            if !workspace.names_answered() {
                return Err(invalid("attachment history is incomplete"));
            }
            let basis = workspace.managed_authoring_basis(actor).map_err(error)?;
            if basis.workspace_id != workspace_id {
                return Err(invalid(
                    "attachment history identity differs from its binding",
                ));
            }
            basis
        };
        let (operations, files) = prepare_snapshot(&workspace, input, &basis)?;
        if operations.is_empty() {
            return workspace
                .workspace_versions()
                .last()
                .map(|version| SavedAttachmentVersion {
                    operation: version.operation(),
                })
                .ok_or_else(|| {
                    invalid("an initial empty project has no representable history point yet")
                });
        }
        if workspace.operations() > 0 {
            workspace
                .validate_managed_operations(&operations)
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
        let saved = save_authenticated_checkpoint(
            &mut workspace,
            &cas,
            make_request(signature),
            files,
            &CaptureHead,
        )
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
            .entries()
            .iter()
            .map(|entry| entry.path().to_owned())
            .collect::<BTreeSet<_>>();
        if !reopened.names_answered()
            || expected != actual
            || !reopened.has_operation(&saved.changeset_id())
        {
            return Err(invalid("saved attachment history needs reconciliation"));
        }
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
        self.ensure_current()?;
        store.ensure_namespace_identity()?;
        if read_receipt(&store)? != self.receipt()?.encode() {
            return Err(invalid("history is not registered to this project"));
        }
        let _guard =
            crate::workspace_custody::lock_workspace_initialization(&store).map_err(error)?;
        let (configuration, _) = self.history_configuration(&store, None)?;
        let workspace =
            OpenWorkspace::open_attachment_store(metadata, store.clone(), false).map_err(error)?;
        verify_history_binding(&workspace, &configuration)?;
        let versions = workspace
            .workspace_versions()
            .into_iter()
            .map(|version| SavedAttachmentVersion {
                operation: version.operation(),
            })
            .collect();
        store.ensure_namespace_identity()?;
        Ok(versions)
    }

    /// Read exact saved bytes from external history; this never falls back to a current source file.
    pub fn saved_file(
        &self,
        metadata: &Path,
        version: SavedAttachmentVersion,
        relative: &str,
    ) -> io::Result<Option<Vec<u8>>> {
        let store = self.history_store(metadata)?;
        let _guard =
            crate::workspace_custody::lock_workspace_initialization(&store).map_err(error)?;
        let (configuration, _) = self.history_configuration(&store, None)?;
        let workspace =
            OpenWorkspace::open_attachment_store(metadata, store.clone(), false).map_err(error)?;
        verify_history_binding(&workspace, &configuration)?;
        let file = workspace
            .historical_workspace_file(version.operation, relative)
            .map_err(error)?;
        store.ensure_namespace_identity()?;
        Ok(file.map(|file| file.bytes))
    }

    fn history_store(&self, metadata: &Path) -> io::Result<PinnedWorkspaceRoot> {
        self.ensure_current()?;
        let store = external_store(metadata, self)?;
        if read_receipt(&store)? != self.receipt()?.encode() {
            return Err(invalid("history is not registered to this project"));
        }
        Ok(store)
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

fn prepare_snapshot(
    workspace: &OpenWorkspace,
    input: &CapturedProjectInput,
    basis: &ManagedAuthoringBasis,
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
    let existing = workspace
        .entries()
        .iter()
        .map(|entry| (entry.path(), entry.entry_type() == "folder"))
        .collect::<BTreeMap<_, _>>();
    let mut removed = existing
        .iter()
        .filter(|(path, directory)| desired.get(**path) != Some(directory))
        .map(|(path, _)| *path)
        .collect::<Vec<_>>();
    removed.sort_by_key(|path| std::cmp::Reverse((path.matches('/').count(), *path)));
    let mut operations = Vec::new();
    for path in removed {
        let binding = workspace.managed_entry_basis(path).map_err(error)?;
        operations.push(Operation::UnlinkDirectoryEntry {
            directory_id: binding.parent_id,
            name: binding.name,
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
            let binding = workspace
                .managed_entry_basis(path_text(&path)?)
                .map_err(error)?;
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
        let previous = if existing.get(path) == Some(&false) {
            Some(
                workspace
                    .file_histories()
                    .iter()
                    .find(|history| history.path() == path)
                    .ok_or_else(|| invalid("saved file history is missing"))?,
            )
        } else {
            None
        };
        let previous_version = previous
            .map(|history| {
                history
                    .current()
                    .ok_or_else(|| invalid("saved file has no current version"))
            })
            .transpose()?;
        if let Some(previous) = previous_version {
            let manifest = workspace
                .manifest_record(previous.manifest())
                .ok_or_else(|| invalid("saved file manifest is missing"))?;
            let metadata = workspace
                .file_version_metadata(previous.version())
                .ok_or_else(|| invalid("saved file mode is missing"))?;
            if manifest.content_digest.as_bytes() == file.digest().as_bytes()
                && metadata == PortableMetadata::new(file.executable())
            {
                continue;
            }
        }
        let id = match previous {
            Some(history) => history.object(),
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
            parent_versions: previous_version.into_iter().map(|v| v.version()).collect(),
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
fn short_id(bytes: &[u8]) -> [u8; 16] {
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
