//! Existing-folder onboarding as a verified copy transaction, never a mount over original data.
//!
//! The source is read, hashed, copied into a newly claimed managed path, then both sides are read
//! and hashed again. The managed path is not confirmed while any path differs. Sibling marker
//! files make an unconfirmed copy recoverable after process restart without making a heuristic
//! guess about whether an arbitrary directory belongs to Mesh. Confirmation replaces that pending
//! proof with a canonical sibling receipt that binds the same directory identity and exact
//! snapshot, so a later process can roll back without broadening deletion authority.

use core::fmt;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, Read as _, Write as _};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use mesh_cas::{Cas, DurableFs as _};
use mesh_chunking::ChunkingConfig;
use mesh_operations::{
    encode_canonical, ActorId, ActorSequence, CausalParents, ChangeSetDraft, HeadDerivation,
    HeadId, Hlc, ManifestId, NormalizedName, ObjectId, Operation, PolicyEpoch, PortableMetadata,
    SessionId, Signature, TransitionCommitment, VersionId, WorkspaceId,
};
use mesh_store::{
    journal_records, Checkpoint, DurableCommit, EntityUuid, OperationRecord, RecordDigest,
    DATABASE_FILE_NAME,
};
use mesh_types::{Blake3, ContentDigest as _, Digest32, DigestHasher as _};

#[cfg(test)]
use mesh_store::MOUNT_DIRECTORY_NAME;

use crate::checkpoint_storage::{CasChunkPromoter, PreparedCheckpointFile};
use crate::exclusions::{EffectiveExclusions, REPOSITORY_IGNORE_FILE_NAME};
use crate::manifest_paging::ManifestPagingPolicy;
use crate::root_authority::PinnedWorkspaceRoot;
use crate::root_authority::ProtectedWorkspaceRoot;
use crate::workspace::OpenWorkspace;

mod received_genesis;

const CLAIM_MARKER: &[u8] = b"mesh-folder-import/1 claim\n";
const OWNED_MARKER_PREFIX: &str = "mesh-folder-import/1 owned";
const PRESENTED_OWNED_MARKER_PREFIX: &str = "mesh-folder-import/2 presented-owned";
const RECEIPT_MAGIC: &[u8] = b"mesh-folder-import/1 receipt\0";
const MANAGED_RECEIPT_MAGIC: &[u8] = b"mesh-folder-import/2 receipt\0";
const PRESENTED_RECEIPT_MAGIC: &[u8] = b"mesh-folder-import/3 presented-receipt\0";
const METADATA_RECEIPT_MAGIC: &[u8] = b"mesh-folder-import/4 receipt\0";
const METADATA_MANAGED_RECEIPT_MAGIC: &[u8] = b"mesh-folder-import/4 managed-receipt\0";
const METADATA_PRESENTED_RECEIPT_MAGIC: &[u8] = b"mesh-folder-import/4 presented-receipt\0";
const IMPORT_DOMAIN: &[u8] = b"mesh.local-folder-import/1\0";
const FILE_IO_BUFFER_BYTES: usize = 64 * 1024;
const MAX_EXCLUSION_FILE_BYTES: u64 = 1024 * 1024;
const MAX_IMPORT_PREVIEW_FILE_ENTRIES: usize = 24;

#[cfg(test)]
type SnapshotEntryHook = Box<dyn FnOnce(&Path)>;

#[cfg(test)]
type BeforeWorkspaceIngestHook = Box<dyn FnOnce()>;

#[cfg(test)]
type BeforeWorkspaceCommitHook = Box<dyn FnOnce()>;

#[cfg(test)]
thread_local! {
    static BEFORE_SNAPSHOT_ENTRY_OPEN: std::cell::RefCell<Option<SnapshotEntryHook>> =
        std::cell::RefCell::new(None);
    static BEFORE_PREPARED_COPY: std::cell::RefCell<Option<BeforeWorkspaceIngestHook>> =
        std::cell::RefCell::new(None);
    static BEFORE_WORKSPACE_INGEST: std::cell::RefCell<Option<BeforeWorkspaceIngestHook>> =
        std::cell::RefCell::new(None);
    static BEFORE_WORKSPACE_COMMIT: std::cell::RefCell<Option<BeforeWorkspaceCommitHook>> =
        std::cell::RefCell::new(None);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DirectoryIdentity {
    device: u64,
    inode: u64,
}

impl DirectoryIdentity {
    fn token(self) -> String {
        format!("{:016x}:{:016x}", self.device, self.inode)
    }
}

/// One regular file in the exact confirmation summary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportedFile {
    relative_path: PathBuf,
    bytes: u64,
    digest: Digest32,
    executable: bool,
}

impl ImportedFile {
    /// Its path relative to the selected folder.
    #[must_use]
    pub fn relative_path(&self) -> &Path {
        &self.relative_path
    }

    /// Its exact byte length.
    #[must_use]
    pub const fn bytes(&self) -> u64 {
        self.bytes
    }

    /// BLAKE3 over its bytes.
    #[must_use]
    pub const fn digest(&self) -> Digest32 {
        self.digest
    }

    /// Whether the imported regular file carries the portable executable bit.
    #[must_use]
    pub const fn executable(&self) -> bool {
        self.executable
    }
}

/// What a person confirms before the managed copy becomes usable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportSummary {
    files: Vec<ImportedFile>,
    directories: u64,
    total_bytes: u64,
    digest: Digest32,
}

impl ImportSummary {
    /// Every file and its hash, sorted by relative path.
    #[must_use]
    pub fn files(&self) -> &[ImportedFile] {
        &self.files
    }

    /// Exact regular-file count.
    #[must_use]
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Exact directory count below the root, including empty directories.
    #[must_use]
    pub const fn directory_count(&self) -> u64 {
        self.directories
    }

    /// Sum of regular-file lengths.
    #[must_use]
    pub const fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// One digest binding every relative path, length, content digest, and executable bit.
    #[must_use]
    pub const fn digest(&self) -> Digest32 {
        self.digest
    }

    /// Stable projection used by local management clients.
    #[must_use]
    pub fn to_json(&self, action: &str) -> crate::ipc::Json {
        self.to_json_with_source_scope(action, None, self.digest())
    }

    fn to_json_with_source_scope(
        &self,
        action: &str,
        source_scope: Option<&str>,
        summary: Digest32,
    ) -> crate::ipc::Json {
        let mut fields = vec![
            ("action", crate::ipc::Json::text(action)),
            (
                "files",
                crate::ipc::Json::Number(u64::try_from(self.file_count()).unwrap_or(u64::MAX)),
            ),
            (
                "directories",
                crate::ipc::Json::Number(self.directory_count()),
            ),
            ("bytes", crate::ipc::Json::Number(self.total_bytes())),
            ("summary", crate::ipc::Json::text(summary.to_string())),
        ];
        if let Some(source_scope) = source_scope {
            fields.push(("source_scope", crate::ipc::Json::text(source_scope)));
        }
        crate::ipc::Json::object(fields)
    }

    /// Stable preview projection with the daemon-authoritative source layout decision.
    #[must_use]
    pub fn to_preview_json(&self, source_private_fence: bool) -> crate::ipc::Json {
        let summary = self.preview_confirmation_digest(source_private_fence);
        let mut preview = self.to_json_with_source_scope(
            "folder-import-preview",
            Some(if source_private_fence {
                "open-zero-history-workspace"
            } else {
                "ordinary-folder"
            }),
            summary,
        );
        let file_entries = self
            .files
            .iter()
            .filter_map(|file| {
                let path = file.relative_path.to_str()?;
                Some((file, path))
            })
            .take(MAX_IMPORT_PREVIEW_FILE_ENTRIES)
            .map(|(file, path)| {
                crate::ipc::Json::object([
                    ("path", crate::ipc::Json::text(path)),
                    ("bytes", crate::ipc::Json::text(file.bytes.to_string())),
                    ("executable", crate::ipc::Json::Bool(file.executable)),
                ])
            })
            .collect::<Vec<_>>();
        let not_listed = self.files.len().saturating_sub(file_entries.len());
        let crate::ipc::Json::Object(fields) = &mut preview else {
            unreachable!("folder import summaries are JSON objects");
        };
        fields.push((
            "file_entries".to_owned(),
            crate::ipc::Json::Array(file_entries),
        ));
        fields.push((
            "files_not_listed".to_owned(),
            crate::ipc::Json::Number(u64::try_from(not_listed).unwrap_or(u64::MAX)),
        ));
        preview
    }

    /// Opaque confirmation token binding both the exact file summary and the daemon-selected
    /// source namespace. A concurrent workspace switch must invalidate confirmation even when
    /// both namespace modes happen to expose byte-identical user content.
    #[must_use]
    pub fn preview_confirmation_digest(&self, source_private_fence: bool) -> Digest32 {
        let mut hasher = Blake3::hasher();
        hasher.update(b"mesh-folder-import-preview/2\0");
        hasher.update(if source_private_fence {
            b"open-zero-history-workspace"
        } else {
            b"ordinary-folder"
        });
        hasher.update(b"\0");
        hasher.update(self.digest().as_bytes());
        let mut token = *hasher.finalize().as_bytes();
        // App-owned import destinations historically use the first 12 hex characters of the
        // content summary as a bounded recovery lookup hint. Preserve those six bytes while the
        // remaining 208 bits bind the selected scope, so a newer app can still find and strictly
        // validate an older alpha import that committed before navigation publication.
        token[..6].copy_from_slice(&self.digest().as_bytes()[..6]);
        Digest32::from_bytes(token)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    summary: ImportSummary,
    directories: BTreeSet<PathBuf>,
}

#[derive(Clone, Debug)]
struct ImportMarkers {
    claim: PathBuf,
    owned: PathBuf,
    receipt: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ImportReceipt {
    encoding: ReceiptEncoding,
    identity: DirectoryIdentity,
    presented_store: Option<PresentedStore>,
    imported: Snapshot,
    managed: Snapshot,
    allows_derived_index_changes: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReceiptEncoding {
    LegacyPlain,
    LegacyManaged,
    LegacyPresented,
    MetadataPlain,
    MetadataManaged,
    MetadataPresented,
}

impl ReceiptEncoding {
    const fn magic(self) -> &'static [u8] {
        match self {
            Self::LegacyPlain => RECEIPT_MAGIC,
            Self::LegacyManaged => MANAGED_RECEIPT_MAGIC,
            Self::LegacyPresented => PRESENTED_RECEIPT_MAGIC,
            Self::MetadataPlain => METADATA_RECEIPT_MAGIC,
            Self::MetadataManaged => METADATA_MANAGED_RECEIPT_MAGIC,
            Self::MetadataPresented => METADATA_PRESENTED_RECEIPT_MAGIC,
        }
    }

    const fn includes_executable(self) -> bool {
        matches!(
            self,
            Self::MetadataPlain | Self::MetadataManaged | Self::MetadataPresented
        )
    }

    const fn managed(self) -> bool {
        matches!(self, Self::LegacyManaged | Self::MetadataManaged)
    }

    const fn presented(self) -> bool {
        matches!(self, Self::LegacyPresented | Self::MetadataPresented)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PresentedStore {
    root: PathBuf,
    identity: DirectoryIdentity,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OwnedDestination {
    destination: DirectoryIdentity,
    presented_store: Option<PresentedStore>,
}

/// Read and hash an existing folder without creating a managed copy.
///
/// This is the preview half of the import journey. It applies the same entry
/// rules and computes the same summary that [`PreparedFolderImport::prepare`]
/// later binds to a verified copy. Repository ignore rules and `.meshignore` prune conclusively
/// excluded trees before entries are opened; when a later rule could re-include a descendant, the
/// walk stays conservative and refuses any included path whose required parent remains excluded
/// rather than silently omitting it. Git metadata is structurally excluded. Names used by Mesh's
/// private store remain ordinary user content because confirmed imports place that store outside
/// the presented native folder.
///
/// # Errors
///
/// Refuses non-directories, included links or special files, unusable exclusion rules, and
/// filesystem failures.
pub fn preview_folder_import(source: &Path) -> Result<ImportSummary, FolderImportError> {
    let source = validated_source(source)?;
    let snapshot = import_snapshot(&source)?;
    require_import_entries(&source, &snapshot)?;
    Ok(snapshot.summary)
}

/// Preview user content from the exact zero-history workspace already opened by the daemon.
///
/// Unlike an ordinary project import, the source has a co-located private namespace whose
/// reserved top-level names must not become project files in the new external-store layout.
pub(crate) fn preview_open_workspace_import(
    source: &Path,
) -> Result<ImportSummary, FolderImportError> {
    let source = validated_source(source)?;
    let snapshot = import_snapshot_with_private_fence(&source)?;
    require_import_entries(&source, &snapshot)?;
    Ok(snapshot.summary)
}

/// Received allocations retain every partial import for purpose-specific native recovery.
/// Ordinary user imports keep their existing rollback contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ImportPurpose {
    User,
    Received,
}
impl ImportPurpose {
    fn failed_copy<'a>(
        self,
        problem: FolderImportError,
        destination: &Path,
        identity: DirectoryIdentity,
        markers: impl IntoIterator<Item = &'a PathBuf>,
    ) -> FolderImportError {
        match self {
            Self::Received => problem,
            Self::User => cleanup_or(problem, destination, identity, markers),
        }
    }
    fn failed_store<'a>(
        self,
        problem: FolderImportError,
        storage_root: &Path,
        identity: DirectoryIdentity,
        destination: &Path,
        markers: impl IntoIterator<Item = &'a PathBuf>,
    ) -> FolderImportError {
        match self {
            Self::Received => problem,
            Self::User => {
                cleanup_presented_prepare_or(problem, storage_root, identity, destination, markers)
            }
        }
    }
}

/// A verified managed copy waiting for the user to confirm its exact summary.
#[derive(Debug)]
pub struct PreparedFolderImport {
    source: PathBuf,
    source_identity: DirectoryIdentity,
    destination: PathBuf,
    owned_marker: PathBuf,
    receipt: PathBuf,
    destination_identity: DirectoryIdentity,
    destination_pinned: PinnedWorkspaceRoot,
    destination_parent_pinned: PinnedWorkspaceRoot,
    presented_store: Option<PresentedStore>,
    snapshot: Snapshot,
    source_private_fence: bool,
    active: bool,
    purpose: ImportPurpose,
}

impl PreparedFolderImport {
    /// Copy and verify one existing folder into a new managed path.
    ///
    /// # Errors
    ///
    /// Refuses non-directories, links and special files, a destination inside the source, marker
    /// collisions, any filesystem failure, or any source/destination verification difference.
    /// The source is never opened for writing. Any failure after destination creation attempts to
    /// remove only the marker-owned destination.
    pub fn prepare(source: &Path, destination: &Path) -> Result<Self, FolderImportError> {
        Self::prepare_inner(source, destination, None, false, None, ImportPurpose::User)
    }

    /// Copy one existing folder into the `mounts/` child of a new external private store.
    ///
    /// The returned destination is an ordinary directory containing only user files. Mesh's
    /// journal, databases, CAS and import receipt remain in its parent store. The store path must
    /// be absent; existing directories are never adopted by this operation.
    pub fn prepare_presented(
        source: &Path,
        storage_root: &Path,
    ) -> Result<Self, FolderImportError> {
        Self::prepare_presented_inner(source, storage_root, false, &[], None, ImportPurpose::User)
    }

    /// Copy into a new presented store only when its pinned parent remains outside every exact
    /// protected workspace directory.
    pub(crate) fn prepare_presented_outside(
        source: &Path,
        storage_root: &Path,
        protected: &[ProtectedWorkspaceRoot],
    ) -> Result<Self, FolderImportError> {
        Self::prepare_presented_inner(
            source,
            storage_root,
            false,
            protected,
            None,
            ImportPurpose::User,
        )
    }

    /// Copy only the user content of an exact zero-history workspace into a presented folder.
    pub(crate) fn prepare_presented_open_workspace(
        source: &Path,
        storage_root: &Path,
    ) -> Result<Self, FolderImportError> {
        Self::prepare_presented_inner(source, storage_root, true, &[], None, ImportPurpose::User)
    }

    /// Copy an admitted zero-history workspace while keeping the new store outside every exact
    /// directory object protected by the native caller.
    pub(crate) fn prepare_presented_open_workspace_outside(
        source: &Path,
        storage_root: &Path,
        protected: &[ProtectedWorkspaceRoot],
    ) -> Result<Self, FolderImportError> {
        Self::prepare_presented_inner(
            source,
            storage_root,
            true,
            protected,
            None,
            ImportPurpose::User,
        )
    }

    /// Require the native caller's retained destination parent before creating any entry.
    pub(crate) fn prepare_presented_with_parent(
        source: &Path,
        storage_root: &Path,
        protected: &[ProtectedWorkspaceRoot],
        expected_parent: Option<ProtectedWorkspaceRoot>,
    ) -> Result<Self, FolderImportError> {
        Self::prepare_presented_inner(
            source,
            storage_root,
            false,
            protected,
            expected_parent,
            ImportPurpose::User,
        )
    }

    /// Native received baselines may be empty: their complete manifest is checked after ingestion.
    /// Errors and dropped handles preserve partial files, journal and markers for native recovery.
    /// Ordinary user imports retain their rollback and no-importable-entries behavior.
    pub(crate) fn prepare_received_with_parent(
        source: &Path,
        storage_root: &Path,
        protected: &[ProtectedWorkspaceRoot],
        expected_parent: ProtectedWorkspaceRoot,
    ) -> Result<Self, FolderImportError> {
        Self::prepare_presented_inner(
            source,
            storage_root,
            false,
            protected,
            Some(expected_parent),
            ImportPurpose::Received,
        )
    }

    fn prepare_presented_inner(
        source: &Path,
        storage_root: &Path,
        source_private_fence: bool,
        protected: &[ProtectedWorkspaceRoot],
        expected_parent: Option<ProtectedWorkspaceRoot>,
        purpose: ImportPurpose,
    ) -> Result<Self, FolderImportError> {
        let source = validated_source(source)?;
        let storage_root = absolute_destination(storage_root)?;
        let parent = storage_root
            .parent()
            .ok_or_else(|| FolderImportError::InvalidDestination {
                path: storage_root.clone(),
            })?
            .to_path_buf();
        let pinned_parent = PinnedWorkspaceRoot::open(parent.clone())
            .map_err(|error| FolderImportError::io("pin destination parent", &parent, error))?;
        if let Some(expected) = expected_parent {
            pinned_parent
                .ensure_protected_identity(expected)
                .map_err(|error| {
                    FolderImportError::io("verify admitted destination parent", &parent, error)
                })?;
        }
        if protected
            .iter()
            .copied()
            .try_fold(false, |found, root| {
                if found {
                    Ok(true)
                } else {
                    pinned_parent.is_within(root)
                }
            })
            .map_err(|error| FolderImportError::io("inspect destination parent", &parent, error))?
        {
            return Err(FolderImportError::DestinationInsideProtectedRoot {
                destination: storage_root,
            });
        }
        if parent.starts_with(&source) {
            return Err(FolderImportError::DestinationInsideSource {
                source,
                destination: storage_root,
            });
        }

        let leaf =
            storage_root
                .file_name()
                .ok_or_else(|| FolderImportError::InvalidDestination {
                    path: storage_root.clone(),
                })?;
        let pinned_store = pinned_parent
            .create_child_directory(leaf)
            .map_err(|error| {
                if error.kind() == io::ErrorKind::AlreadyExists {
                    FolderImportError::DestinationExists {
                        path: storage_root.clone(),
                    }
                } else {
                    FolderImportError::io("create_dir", &storage_root, error)
                }
            })?;
        let (device, inode) = pinned_store.identity().map_err(|error| {
            FolderImportError::io("inspect created store", &storage_root, error)
        })?;
        let identity = DirectoryIdentity { device, inode };
        let marker = storage_root.join(crate::workspace::PRESENTED_LAYOUT_MARKER_NAME);
        if let Err(error) = pinned_store.filesystem().write_new_file(
            Path::new(crate::workspace::PRESENTED_LAYOUT_MARKER_NAME),
            crate::workspace::PRESENTED_LAYOUT_MARKER_BYTES,
            fs::Permissions::from_mode(0o600),
        ) {
            let problem = FolderImportError::io("create marker", &marker, error);
            return Err(purpose.failed_copy(problem, &storage_root, identity, [&marker]));
        }
        if let Err(error) = pinned_parent.sync() {
            let problem = FolderImportError::io("sync_dir", &parent, error);
            return Err(purpose.failed_copy(problem, &storage_root, identity, [&marker]));
        }
        let presented = PresentedStore {
            root: storage_root.clone(),
            identity,
        };
        let destination = storage_root.join(crate::workspace::PRESENTED_DIRECTORY_NAME);
        match Self::prepare_inner(
            &source,
            &destination,
            Some(presented),
            source_private_fence,
            Some(pinned_store),
            purpose,
        ) {
            Ok(prepared) => Ok(prepared),
            Err(problem) => {
                Err(purpose.failed_store(problem, &storage_root, identity, &destination, [&marker]))
            }
        }
    }

    fn prepare_inner(
        source: &Path,
        destination: &Path,
        presented_store: Option<PresentedStore>,
        source_private_fence: bool,
        pinned_parent: Option<PinnedWorkspaceRoot>,
        purpose: ImportPurpose,
    ) -> Result<Self, FolderImportError> {
        let source = validated_source(source)?;
        let source_identity = directory_identity(&source)?;

        let destination = absolute_destination(destination)?;
        let parent = destination
            .parent()
            .ok_or_else(|| FolderImportError::InvalidDestination {
                path: destination.clone(),
            })?;
        let parent = parent.to_path_buf();
        let destination_parent_pinned = match pinned_parent {
            Some(parent) => parent,
            None => PinnedWorkspaceRoot::open(parent.clone())
                .map_err(|error| FolderImportError::io("pin destination parent", &parent, error))?,
        };
        destination_parent_pinned
            .ensure_namespace_identity()
            .map_err(|error| FolderImportError::io("verify destination parent", &parent, error))?;
        let resolved_parent = parent
            .canonicalize()
            .map_err(|error| FolderImportError::io("canonicalize", &parent, error))?;
        if resolved_parent.starts_with(&source) {
            return Err(FolderImportError::DestinationInsideSource {
                source,
                destination,
            });
        }
        if fs::symlink_metadata(&destination).is_ok() {
            return Err(FolderImportError::DestinationExists { path: destination });
        }

        let before = import_snapshot_for_layout(&source, source_private_fence)?;
        if purpose == ImportPurpose::User {
            require_import_entries(&source, &before)?;
        }
        if presented_store.is_none() {
            reject_reserved_workspace_path(&before)?;
        }
        let markers = markers(&destination)?;
        for marker in [&markers.claim, &markers.owned, &markers.receipt] {
            if fs::symlink_metadata(marker).is_ok() {
                return Err(FolderImportError::DestinationExists {
                    path: marker.clone(),
                });
            }
        }
        let parent_filesystem = destination_parent_pinned.filesystem();
        parent_filesystem
            .write_new_file(
                &markers.claim,
                CLAIM_MARKER,
                fs::Permissions::from_mode(0o600),
            )
            .map_err(|error| FolderImportError::io("create marker", &markers.claim, error))?;
        let destination_name =
            destination
                .file_name()
                .ok_or_else(|| FolderImportError::InvalidDestination {
                    path: destination.clone(),
                })?;
        let destination_pinned =
            match destination_parent_pinned.create_child_directory(destination_name) {
                Ok(root) => root,
                Err(error) => {
                    if purpose == ImportPurpose::User {
                        let _ = parent_filesystem.remove_file(&markers.claim);
                    }
                    return Err(FolderImportError::io("create_dir", &destination, error));
                }
            };
        let (device, inode) = destination_pinned
            .identity()
            .map_err(|error| FolderImportError::io("inspect destination", &destination, error))?;
        let destination_identity = DirectoryIdentity { device, inode };
        if let Err(error) = parent_filesystem.write_new_file(
            &markers.owned,
            owned_marker_bytes(destination_identity, presented_store.as_ref()).as_bytes(),
            fs::Permissions::from_mode(0o600),
        ) {
            let problem = FolderImportError::io("create marker", &markers.owned, error);
            return Err(purpose.failed_copy(
                problem,
                &destination,
                destination_identity,
                [&markers.claim],
            ));
        }
        if let Err(error) = parent_filesystem.remove_file(&markers.claim) {
            return Err(purpose.failed_copy(
                FolderImportError::io("remove_file", &markers.claim, error),
                &destination,
                destination_identity,
                [&markers.claim, &markers.owned, &markers.receipt],
            ));
        }

        #[cfg(test)]
        BEFORE_PREPARED_COPY.with(|hook| {
            if let Some(hook) = hook.borrow_mut().take() {
                hook();
            }
        });
        if let Err(problem) = copy_snapshot_with_destination_root(
            &source,
            source_identity,
            &destination,
            destination_identity,
            &destination_pinned,
            &before,
        ) {
            return Err(purpose.failed_copy(
                problem,
                &destination,
                destination_identity,
                [&markers.claim, &markers.owned, &markers.receipt],
            ));
        }
        let after_source = match import_snapshot_for_layout(&source, source_private_fence) {
            Ok(value) => value,
            Err(problem) => {
                return Err(purpose.failed_copy(
                    problem,
                    &destination,
                    destination_identity,
                    [&markers.claim, &markers.owned, &markers.receipt],
                ))
            }
        };
        let copied = match snapshot_with_pinned_root(
            &destination,
            destination_identity,
            &destination_pinned,
            None,
            false,
        ) {
            Ok(value) => value,
            Err(problem) => {
                return Err(purpose.failed_copy(
                    problem,
                    &destination,
                    destination_identity,
                    [&markers.claim, &markers.owned, &markers.receipt],
                ))
            }
        };
        let differences = differences(&before, &after_source, &copied);
        if !differences.is_empty() {
            return Err(purpose.failed_copy(
                FolderImportError::VerificationMismatch { paths: differences },
                &destination,
                destination_identity,
                [&markers.claim, &markers.owned, &markers.receipt],
            ));
        }
        if let Err(error) = destination_parent_pinned.sync() {
            let problem = FolderImportError::io("sync_dir", &parent, error);
            return Err(purpose.failed_copy(
                problem,
                &destination,
                destination_identity,
                [&markers.owned, &markers.receipt],
            ));
        }
        Ok(Self {
            source,
            source_identity,
            destination,
            owned_marker: markers.owned,
            receipt: markers.receipt,
            destination_identity,
            destination_pinned,
            destination_parent_pinned,
            presented_store,
            snapshot: before,
            source_private_fence,
            active: true,
            purpose,
        })
    }

    /// The exact summary awaiting confirmation.
    #[must_use]
    pub const fn summary(&self) -> &ImportSummary {
        &self.snapshot.summary
    }

    /// The original folder, which this transaction never writes.
    #[must_use]
    pub fn source(&self) -> &Path {
        &self.source
    }

    /// The new managed copy.
    #[must_use]
    pub fn destination(&self) -> &Path {
        &self.destination
    }

    /// Check the created folder against exact directory objects protected by the caller.
    ///
    /// This runs after the create-only destination transaction has pinned the actual parent and
    /// child. It therefore remains authoritative when a pathname was renamed after an earlier UI
    /// preflight.
    pub(crate) fn destination_is_within_any(
        &self,
        protected: &[ProtectedWorkspaceRoot],
    ) -> Result<bool, FolderImportError> {
        self.destination_parent_pinned
            .ensure_namespace_identity()
            .and_then(|()| self.destination_pinned.ensure_namespace_identity())
            .map_err(|error| {
                FolderImportError::io("verify prepared destination", &self.destination, error)
            })?;
        for root in protected {
            if self.destination_pinned.is_within(*root).map_err(|error| {
                FolderImportError::io(
                    "inspect prepared destination ancestors",
                    &self.destination,
                    error,
                )
            })? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Re-verify both folders and make the managed copy usable by removing its pending marker.
    ///
    /// # Errors
    ///
    /// Any difference aborts and rolls back the managed copy. The differing relative paths are
    /// returned. The original remains untouched.
    pub fn confirm(mut self) -> Result<ConfirmedFolderImport, FolderImportError> {
        self.destination_parent_pinned
            .ensure_namespace_identity()
            .and_then(|()| self.destination_pinned.ensure_namespace_identity())
            .map_err(|error| {
                FolderImportError::io("verify prepared destination", &self.destination, error)
            })?;
        let source = import_snapshot_for_layout(&self.source, self.source_private_fence)?;
        let copied = snapshot_with_pinned_root(
            &self.destination,
            self.destination_identity,
            &self.destination_pinned,
            None,
            false,
        )?;
        let differences = differences(&self.snapshot, &source, &copied);
        if !differences.is_empty() {
            let problem = FolderImportError::VerificationMismatch { paths: differences };
            self.cleanup_on_failure()?;
            return Err(problem);
        }
        self.destination_parent_pinned
            .filesystem()
            .write_new_file(
                &self.receipt,
                &confirmed_receipt_bytes(
                    self.destination_identity,
                    self.presented_store.as_ref(),
                    &self.snapshot,
                    &self.snapshot,
                    false,
                ),
                fs::Permissions::from_mode(0o600),
            )
            .map_err(|error| FolderImportError::io("create marker", &self.receipt, error))?;
        self.destination_parent_pinned
            .filesystem()
            .remove_file(&self.owned_marker)
            .map_err(|error| FolderImportError::io("remove_file", &self.owned_marker, error))?;
        self.destination_parent_pinned
            .sync()
            .map_err(|error| FolderImportError::io("sync_dir", &self.destination, error))?;
        self.active = false;
        Ok(ConfirmedFolderImport {
            destination: self.destination.clone(),
            receipt: self.receipt.clone(),
            destination_identity: self.destination_identity,
            presented_store: self.presented_store.clone(),
            imported_snapshot: self.snapshot.clone(),
            managed_snapshot: self.snapshot.clone(),
            allows_derived_index_changes: false,
        })
    }

    /// Confirm the verified copy and ingest its complete supported tree as one private ChangeSet.
    ///
    /// The import is still pending and marker-owned while content, manifests, the ChangeSet
    /// payload, SQLite index, and immutable journal are written. Any failure therefore follows the
    /// existing rollback path and can remove only the exact directory instance this transaction
    /// created. The final metadata-aware receipt binds both the bytes and executable state the
    /// person approved and the complete post-ingest managed tree, so an immediate rollback
    /// remains exact after process restart.
    ///
    /// This is private local history. It does not advance the protected shared version and does
    /// not manufacture an approval receipt.
    pub fn confirm_into_workspace(
        self,
    ) -> Result<(ConfirmedFolderImport, ManagedImportOutcome), FolderImportError> {
        let paths = imported_path_objects(&self.snapshot)
            .into_keys()
            .collect::<BTreeSet<_>>();
        let source = self.source.clone();
        let source_installation = self.source_identity.token();
        self.confirm_into_workspace_with_origin(&source, &source_installation, &paths)
    }

    /// Confirm a historical copy without granting its temporary export deletion authority over
    /// any ordinary folder.
    pub(crate) fn confirm_into_workspace_without_origin(
        self,
    ) -> Result<(ConfirmedFolderImport, ManagedImportOutcome), FolderImportError> {
        self.confirm_into_workspace_inner(None)
    }

    /// Confirm a historical copy while carrying only exact, previously proven object origins.
    pub(crate) fn confirm_into_workspace_with_origin(
        self,
        origin_root: &Path,
        expected_origin_installation: &str,
        authorized_paths: &BTreeSet<PathBuf>,
    ) -> Result<(ConfirmedFolderImport, ManagedImportOutcome), FolderImportError> {
        let canonical_origin = origin_root
            .canonicalize()
            .map_err(|error| FolderImportError::io("canonicalize", origin_root, error))?;
        let origin_identity = directory_identity(&canonical_origin)?;
        if origin_identity.token() != expected_origin_installation {
            return Err(FolderImportError::OwnershipMismatch {
                destination: canonical_origin,
            });
        }
        let available = imported_path_objects(&self.snapshot);
        if let Some(path) = authorized_paths
            .iter()
            .find(|path| !available.contains_key(path.as_path()))
        {
            return Err(FolderImportError::UnsupportedEntry { path: path.clone() });
        }
        self.confirm_into_workspace_inner(Some((
            canonical_origin,
            origin_identity,
            authorized_paths.clone(),
        )))
    }

    fn confirm_into_workspace_inner(
        mut self,
        origin: Option<(PathBuf, DirectoryIdentity, BTreeSet<PathBuf>)>,
    ) -> Result<(ConfirmedFolderImport, ManagedImportOutcome), FolderImportError> {
        self.destination_parent_pinned
            .ensure_namespace_identity()
            .and_then(|()| self.destination_pinned.ensure_namespace_identity())
            .map_err(|error| {
                FolderImportError::io("verify prepared destination", &self.destination, error)
            })?;
        let source = import_snapshot_for_layout(&self.source, self.source_private_fence)?;
        let copied = snapshot_with_pinned_root(
            &self.destination,
            self.destination_identity,
            &self.destination_pinned,
            None,
            false,
        )?;
        let differences = differences(&self.snapshot, &source, &copied);
        if !differences.is_empty() {
            let problem = FolderImportError::VerificationMismatch { paths: differences };
            self.cleanup_on_failure()?;
            return Err(problem);
        }

        if self.presented_store.is_none() {
            if let Err(problem) = reject_reserved_workspace_path(&self.snapshot) {
                self.cleanup_on_failure()?;
                return Err(problem);
            }
        }

        let (storage_root, storage_pinned) = if let Some(store) = &self.presented_store {
            (store.root.clone(), self.destination_parent_pinned.clone())
        } else {
            let storage_root = self
                .destination
                .join(crate::workspace::STORAGE_DIRECTORY_NAME);
            let storage_pinned = self
                .destination_pinned
                .create_child_directory(std::ffi::OsStr::new(
                    crate::workspace::STORAGE_DIRECTORY_NAME,
                ))
                .map_err(|error| {
                    FolderImportError::io("create private store", &storage_root, error)
                })?;
            (storage_root, storage_pinned)
        };

        #[cfg(test)]
        BEFORE_WORKSPACE_INGEST.with(|hook| {
            if let Some(hook) = hook.borrow_mut().take() {
                hook();
            }
        });
        let outcome = ingest_private_workspace(
            &self.destination,
            &self.destination_pinned,
            &storage_root,
            &storage_pinned,
            &self.snapshot,
            self.purpose,
        )?;
        verify_directory_identity(&self.source, self.source_identity)?;
        let opened = OpenWorkspace::open_prepared_layout(
            &self.destination,
            self.destination_pinned.clone(),
            &storage_root,
            storage_pinned,
            &crate::TrustedReviewers::default(),
        )
        .map_err(|error| workspace_import_error(&self.destination, error))?;
        if let Some((origin_root, origin_identity, authorized_paths)) = origin {
            verify_directory_identity(&origin_root, origin_identity)?;
            let filesystem = opened.storage_pinned_root().filesystem();
            let workspace_installation = opened.installation();
            let target_root_installation = origin_identity.token();
            let objects = imported_path_objects(&self.snapshot);
            let receipts = authorized_paths
                .iter()
                .filter_map(|path| objects.get(path).copied().map(|object| (path, object)))
                .map(
                    |(path, object)| crate::pull_back_receipt::ImportOriginReceipt {
                        workspace_installation: &workspace_installation,
                        target_root: &origin_root,
                        target_root_installation: &target_root_installation,
                        object,
                        path,
                    },
                )
                .collect::<Vec<_>>();
            crate::pull_back_receipt::record_import_origins(
                &filesystem,
                opened.storage_root().as_path(),
                &receipts,
            )
            .map_err(|error| workspace_import_error(&self.destination, error))?;
        }
        let source_after = import_snapshot_for_layout(&self.source, self.source_private_fence)?;
        let complete_managed_snapshot = snapshot_with_pinned_root(
            &self.destination,
            self.destination_identity,
            &self.destination_pinned,
            None,
            false,
        )?;
        let mut changed = snapshot_difference(&self.snapshot, &source_after);
        changed.extend(imported_subset_difference(
            &self.snapshot,
            &complete_managed_snapshot,
        ));
        changed.sort();
        changed.dedup();
        if !changed.is_empty() {
            let problem = FolderImportError::VerificationMismatch { paths: changed };
            self.cleanup_on_failure()?;
            return Err(problem);
        }
        let managed_snapshot =
            without_derived_index(complete_managed_snapshot, self.presented_store.is_some());
        self.destination_parent_pinned
            .filesystem()
            .write_new_file(
                &self.receipt,
                &confirmed_receipt_bytes(
                    self.destination_identity,
                    self.presented_store.as_ref(),
                    &self.snapshot,
                    &managed_snapshot,
                    true,
                ),
                fs::Permissions::from_mode(0o600),
            )
            .map_err(|error| FolderImportError::io("create marker", &self.receipt, error))?;
        self.destination_parent_pinned
            .filesystem()
            .remove_file(&self.owned_marker)
            .map_err(|error| FolderImportError::io("remove_file", &self.owned_marker, error))?;
        self.destination_parent_pinned
            .sync()
            .map_err(|error| FolderImportError::io("sync_dir", &self.destination, error))?;
        self.active = false;
        Ok((
            ConfirmedFolderImport {
                destination: self.destination.clone(),
                receipt: self.receipt.clone(),
                destination_identity: self.destination_identity,
                presented_store: self.presented_store.clone(),
                imported_snapshot: self.snapshot.clone(),
                managed_snapshot,
                allows_derived_index_changes: true,
            },
            outcome,
        ))
    }

    /// Remove the unconfirmed managed copy and restore the pre-import absence of the destination.
    pub fn rollback(mut self) -> Result<(), FolderImportError> {
        self.rollback_inner()
    }

    fn cleanup_on_failure(&mut self) -> Result<(), FolderImportError> {
        if self.purpose == ImportPurpose::Received {
            return Ok(());
        }
        self.rollback_inner()
    }

    fn rollback_inner(&mut self) -> Result<(), FolderImportError> {
        if self.purpose == ImportPurpose::Received {
            return Err(FolderImportError::io(
                "retain received initialization",
                &self.destination,
                io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "received initialization requires purpose-specific native recovery",
                ),
            ));
        }
        if !self.active {
            return Ok(());
        }
        cleanup_import(
            &self.destination,
            self.destination_identity,
            self.presented_store.as_ref(),
            [&self.owned_marker, &self.receipt],
        )?;
        self.active = false;
        Ok(())
    }
}

impl Drop for PreparedFolderImport {
    fn drop(&mut self) {
        let _ = self.cleanup_on_failure();
    }
}

/// A confirmed managed copy. The original still exists at its original path.
#[derive(Clone, Debug)]
pub struct ConfirmedFolderImport {
    destination: PathBuf,
    receipt: PathBuf,
    destination_identity: DirectoryIdentity,
    presented_store: Option<PresentedStore>,
    imported_snapshot: Snapshot,
    managed_snapshot: Snapshot,
    allows_derived_index_changes: bool,
}

/// Durable private-history facts produced while confirming a managed import.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ManagedImportOutcome {
    operation: RecordDigest,
    manifests: usize,
    entries: usize,
    linked_bytes: u64,
}

impl ManagedImportOutcome {
    /// The one imported ChangeSet now present in `records.mesh`.
    #[must_use]
    pub const fn operation(self) -> RecordDigest {
        self.operation
    }

    /// Distinct canonical file manifests committed by the import.
    #[must_use]
    pub const fn manifests(self) -> usize {
        self.manifests
    }

    /// Materialized files and directories below the workspace root.
    #[must_use]
    pub const fn entries(self) -> usize {
        self.entries
    }

    /// Bytes newly linked into the local content-addressed store.
    #[must_use]
    pub const fn linked_bytes(self) -> u64 {
        self.linked_bytes
    }
}

impl ConfirmedFolderImport {
    /// Reopen a confirmed import from its durable sibling receipt.
    ///
    /// # Errors
    ///
    /// Refuses a missing, malformed or non-canonical receipt and a destination
    /// whose stable directory identity no longer matches the receipt.
    pub fn open(destination: &Path) -> Result<Self, FolderImportError> {
        let destination = absolute_destination(destination)?;
        let receipt = markers(&destination)?.receipt;
        let parsed = read_receipt(&receipt)?;
        let destination_identity = parsed.identity;
        verify_directory_identity(&destination, destination_identity)?;
        if let Some(store) = &parsed.presented_store {
            verify_presented_store(&destination, store)?;
        }
        Ok(Self {
            destination,
            receipt,
            destination_identity,
            presented_store: parsed.presented_store,
            imported_snapshot: parsed.imported,
            managed_snapshot: parsed.managed,
            allows_derived_index_changes: parsed.allows_derived_index_changes,
        })
    }

    /// Prove that this confirmed managed copy came from this exact ordinary folder and that the
    /// folder still has the bytes the person previewed.
    ///
    /// This is intentionally stronger than matching the public import-summary digest. Two
    /// unrelated folders can contain identical bytes. The private import-origin receipts bind
    /// every imported object to the canonical source directory identity, so a native client can
    /// recover an interrupted app-navigation commit without adopting a same-content workspace
    /// that belongs to another source.
    pub(crate) fn proves_exact_origin(
        &self,
        source: &Path,
        expected_summary: &str,
    ) -> Result<bool, FolderImportError> {
        let canonical_source = validated_source(source)?;
        let source_identity = directory_identity(&canonical_source)?;
        let summary = &self.imported_snapshot.summary;
        let ordinary_token = summary.preview_confirmation_digest(false).to_string();
        let protected_token = summary.preview_confirmation_digest(true).to_string();
        let legacy_token = summary.digest().to_string();
        let source_matches = if expected_summary == ordinary_token {
            import_snapshot(&canonical_source)
                .is_ok_and(|snapshot| snapshot == self.imported_snapshot)
        } else if expected_summary == protected_token {
            import_snapshot_with_private_fence(&canonical_source)? == self.imported_snapshot
        } else if expected_summary == legacy_token {
            // Compatibility for an import confirmed by an older alpha before native navigation
            // was committed. That preview token did not encode its scope, so retain the original
            // conservative proof: one of the two exact scanners must reproduce the receipt.
            import_snapshot(&canonical_source)
                .is_ok_and(|snapshot| snapshot == self.imported_snapshot)
                || import_snapshot_with_private_fence(&canonical_source)? == self.imported_snapshot
        } else {
            false
        };
        if !source_matches {
            return Ok(false);
        }

        let opened = OpenWorkspace::open(&self.destination)
            .map_err(|error| workspace_import_error(&self.destination, error))?;
        let filesystem = opened.storage_pinned_root().filesystem();
        let workspace_installation = opened.installation();
        let source_installation = source_identity.token();
        let imported_objects = imported_path_objects(&self.imported_snapshot);
        if imported_objects.is_empty() {
            // The current receipt format binds origins per imported object. An empty folder has
            // no such object, so its public summary cannot safely distinguish two sources.
            return Ok(false);
        }
        Ok(imported_objects.iter().all(|(path, object)| {
            crate::pull_back_receipt::proves_import_origin(
                &filesystem,
                opened.storage_root().as_path(),
                &crate::pull_back_receipt::ImportOriginReceipt {
                    workspace_installation: &workspace_installation,
                    target_root: &canonical_source,
                    target_root_installation: &source_installation,
                    object: *object,
                    path,
                },
            )
        }))
    }

    /// The confirmed summary.
    #[must_use]
    pub const fn summary(&self) -> &ImportSummary {
        &self.imported_snapshot.summary
    }

    /// The managed path.
    #[must_use]
    pub fn destination(&self) -> &Path {
        &self.destination
    }

    /// Durable sibling receipt that allows an exact rollback after restart.
    #[must_use]
    pub fn receipt(&self) -> &Path {
        &self.receipt
    }

    /// Whether `path` resolves to the exact directory bound by this import receipt.
    ///
    /// The daemon may have opened the managed workspace through an alias. Rollback still deletes
    /// only [`Self::destination`], whose non-link identity is verified again by [`Self::rollback`],
    /// but lifecycle state must be uninstalled for every spelling of that same directory.
    pub(crate) fn refers_to_destination(&self, path: &Path) -> Result<bool, FolderImportError> {
        Ok(resolved_directory_identity(path)? == self.destination_identity)
    }

    /// Remove an unchanged managed copy, restoring the pre-import absence of that path.
    ///
    /// If anything changed after confirmation, rollback refuses without deleting any of it.
    pub fn rollback(self) -> Result<(), FolderImportError> {
        self.rollback_with_custody_hook(|| {})
    }

    fn rollback_with_custody_hook<F>(self, after_lock: F) -> Result<(), FolderImportError>
    where
        F: FnOnce(),
    {
        // The physical directory flock exists before a plain confirmed copy has private storage.
        // Acquire it before classifying the journal, then retain it through snapshot + delete. A
        // concurrent first open may initialize private state, but cannot acquire custody or begin
        // a managed mutation before this exact receipt either deletes or refuses the copy.
        let _custody = crate::workspace_custody::require_unassigned_path(&self.destination)
            .map_err(|error| FolderImportError::CustodyRefused(error.to_string()))?;
        after_lock();
        self.rollback_after_custody()
    }

    /// Complete rollback while the caller holds this workspace's shared custody lock.
    pub(crate) fn rollback_after_custody(self) -> Result<(), FolderImportError> {
        let durable = read_receipt(&self.receipt)?;
        if durable.identity != self.destination_identity
            || durable.presented_store != self.presented_store
            || durable.imported != self.imported_snapshot
            || durable.managed != self.managed_snapshot
        {
            return Err(FolderImportError::UnrecognizedMarker { path: self.receipt });
        }
        verify_directory_identity(&self.destination, self.destination_identity)?;
        let current = if self.allows_derived_index_changes {
            without_derived_index(snapshot(&self.destination)?, self.presented_store.is_some())
        } else {
            snapshot(&self.destination)?
        };
        let paths = snapshot_difference(&self.managed_snapshot, &current);
        if !paths.is_empty() {
            return Err(FolderImportError::RollbackRefused { paths });
        }
        cleanup_import(
            &self.destination,
            self.destination_identity,
            self.presented_store.as_ref(),
            [&self.receipt],
        )
    }
}

/// Remove a marker-owned unconfirmed import left by a stopped process.
///
/// Returns `false` when no marker exists. A claim marker without an ownership marker is removed,
/// but its destination is retained because the stopped process may not have created it.
pub fn recover_pending_import(destination: &Path) -> Result<bool, FolderImportError> {
    let destination = absolute_destination(destination)?;
    let markers = markers(&destination)?;
    if marker_exists(&markers.owned)? {
        let owned = verify_owned_marker(&markers.owned, &destination)?;
        cleanup_import(
            &destination,
            owned.destination,
            owned.presented_store.as_ref(),
            [&markers.claim, &markers.owned, &markers.receipt],
        )?;
        return Ok(true);
    }
    if marker_exists(&markers.claim)? {
        verify_marker(&markers.claim, CLAIM_MARKER)?;
        fs::remove_file(&markers.claim)
            .map_err(|error| FolderImportError::io("remove_file", &markers.claim, error))?;
        sync_directory(markers.claim.parent().unwrap_or(Path::new(".")))?;
        return Ok(true);
    }
    Ok(false)
}

/// Why an existing-folder import was refused or rolled back.
#[derive(Debug)]
pub enum FolderImportError {
    /// No included entries can establish a durable root in the current import format.
    NoImportableEntries {
        /// The selected source, after applying import exclusions.
        path: PathBuf,
    },
    /// The selected source is not a real directory.
    SourceNotDirectory {
        /// The selected path.
        path: PathBuf,
    },
    /// The destination has no usable parent or file name.
    InvalidDestination {
        /// The refused destination.
        path: PathBuf,
    },
    /// The new path already exists and is never overwritten.
    DestinationExists {
        /// The path that would have been overwritten.
        path: PathBuf,
    },
    /// Creating the managed path here would mutate the original folder.
    DestinationInsideSource {
        /// The original folder.
        source: PathBuf,
        /// The proposed managed path.
        destination: PathBuf,
    },
    /// The pinned destination parent is inside a workspace object protected by the caller.
    DestinationInsideProtectedRoot {
        /// The proposed managed path.
        destination: PathBuf,
    },
    /// A link or special file has no preservation contract in this bounded import.
    UnsupportedEntry {
        /// The entry with no preservation contract.
        path: PathBuf,
    },
    /// A rule claims a descendant is versioned while one of its required parent folders is not.
    ReincludedPathBelowExcludedAncestor {
        /// The path the effective predicate says is versioned.
        path: PathBuf,
        /// Its excluded parent, which the current durable directory model cannot silently invent.
        excluded_ancestor: PathBuf,
    },
    /// Import exclusion rules could not be read safely or parsed exactly.
    ExclusionRulesUnavailable {
        /// The rules file or source root involved.
        path: PathBuf,
        /// Why its rules cannot safely decide import membership.
        detail: String,
    },
    /// A top-level source name belongs to Mesh's private workspace layout.
    ReservedWorkspacePath {
        /// The relative source path that would collide with private state.
        path: PathBuf,
    },
    /// Source and managed copy did not verify as the same snapshot.
    VerificationMismatch {
        /// Exact relative paths that differed.
        paths: Vec<PathBuf>,
    },
    /// A confirmed managed copy changed, so rollback deleted nothing.
    RollbackRefused {
        /// Exact relative paths changed since confirmation.
        paths: Vec<PathBuf>,
    },
    /// The workspace-native authority says an agent still owns this managed directory.
    CustodyRefused(String),
    /// A recovery marker exists but is not one this version wrote.
    UnrecognizedMarker {
        /// The marker whose bytes were not this version's.
        path: PathBuf,
    },
    /// A marker named a different directory instance, so deletion was refused.
    OwnershipMismatch {
        /// The destination whose current identity was not authorized.
        destination: PathBuf,
    },
    /// A filesystem operation failed.
    Io {
        /// The filesystem operation.
        operation: &'static str,
        /// Its target.
        path: PathBuf,
        /// The filesystem's error.
        source: io::Error,
    },
    /// Import failed and its managed destination could not be removed completely.
    CleanupFailed {
        /// The managed path that may remain.
        destination: PathBuf,
        /// Why cleanup failed.
        reason: String,
    },
}

impl FolderImportError {
    fn io(operation: &'static str, path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            operation,
            path: path.into(),
            source,
        }
    }
}

impl fmt::Display for FolderImportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoImportableEntries { path } => write!(
                formatter,
                "{} has no importable files or folders after exclusions. Choose a folder containing project files. Empty-folder import is not supported yet; no workspace was created",
                path.display()
            ),
            Self::SourceNotDirectory { path } => {
                write!(formatter, "{} is not a directory", path.display())
            }
            Self::InvalidDestination { path } => {
                write!(formatter, "{} is not a usable managed path", path.display())
            }
            Self::DestinationExists { path } => {
                write!(
                    formatter,
                    "{} already exists and was not changed",
                    path.display()
                )
            }
            Self::DestinationInsideSource {
                source,
                destination,
            } => write!(
                formatter,
                "managed path {} is inside original folder {}",
                destination.display(),
                source.display()
            ),
            Self::DestinationInsideProtectedRoot { destination } => write!(
                formatter,
                "managed path {} is inside a protected workspace",
                destination.display()
            ),
            Self::UnsupportedEntry { path } => write!(
                formatter,
                "{} is a link or special file and cannot be preserved by this import",
                path.display()
            ),
            Self::ReincludedPathBelowExcludedAncestor {
                path,
                excluded_ancestor,
            } => write!(
                formatter,
                "{} is re-included below excluded parent {}; include the parent too or keep the descendant excluded before importing",
                path.display(),
                excluded_ancestor.display()
            ),
            Self::ExclusionRulesUnavailable { path, detail } => write!(
                formatter,
                "import exclusion rules at {} are unavailable: {detail}",
                path.display()
            ),
            Self::ReservedWorkspacePath { path } => write!(
                formatter,
                "{} is reserved for Mesh private workspace state and cannot be imported at the workspace root",
                path.display()
            ),
            Self::VerificationMismatch { paths } => {
                write!(
                    formatter,
                    "import verification differed at {} path(s)",
                    paths.len()
                )
            }
            Self::RollbackRefused { paths } => write!(
                formatter,
                "managed copy changed at {} path(s), so rollback deleted nothing",
                paths.len()
            ),
            Self::CustodyRefused(detail) => write!(formatter, "rollback refused: {detail}"),
            Self::UnrecognizedMarker { path } => {
                write!(formatter, "{} is not a Mesh import marker", path.display())
            }
            Self::OwnershipMismatch { destination } => write!(
                formatter,
                "{} is not the directory instance created by this import, so it was not deleted",
                destination.display()
            ),
            Self::Io {
                operation,
                path,
                source,
            } => write!(formatter, "{operation} {} failed: {source}", path.display()),
            Self::CleanupFailed {
                destination,
                reason,
            } => write!(
                formatter,
                "import failed and managed path {} could not be rolled back: {reason}",
                destination.display()
            ),
        }
    }
}

impl std::error::Error for FolderImportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn validated_source(source: &Path) -> Result<PathBuf, FolderImportError> {
    let source = source
        .canonicalize()
        .map_err(|error| FolderImportError::io("canonicalize", source, error))?;
    let metadata = fs::symlink_metadata(&source)
        .map_err(|error| FolderImportError::io("metadata", &source, error))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(FolderImportError::SourceNotDirectory { path: source });
    }
    Ok(source)
}

fn reserved_workspace_path(snapshot: &Snapshot) -> Option<PathBuf> {
    snapshot
        .directories
        .iter()
        .chain(
            snapshot
                .summary
                .files
                .iter()
                .map(|file| &file.relative_path),
        )
        .find(|path| {
            path.components()
                .next()
                .and_then(|component| component.as_os_str().to_str())
                .is_some_and(|name| crate::workspace::is_private_managed_component(0, name))
        })
        .cloned()
}

fn reject_reserved_workspace_path(snapshot: &Snapshot) -> Result<(), FolderImportError> {
    match reserved_workspace_path(snapshot) {
        Some(path) => Err(FolderImportError::ReservedWorkspacePath { path }),
        None => Ok(()),
    }
}

fn snapshot(root: &Path) -> Result<Snapshot, FolderImportError> {
    let expected_root = directory_identity(root)?;
    let pinned = PinnedWorkspaceRoot::open(root.to_path_buf())
        .map_err(|error| FolderImportError::io("pin snapshot root", root, error))?;
    pinned
        .ensure_identity(expected_root.device, expected_root.inode)
        .map_err(|error| FolderImportError::io("verify snapshot root", root, error))?;
    snapshot_with_pinned_root(root, expected_root, &pinned, None, false)
}

fn import_snapshot(root: &Path) -> Result<Snapshot, FolderImportError> {
    import_snapshot_for_layout(root, false)
}

fn import_snapshot_with_private_fence(root: &Path) -> Result<Snapshot, FolderImportError> {
    import_snapshot_for_layout(root, true)
}

fn import_snapshot_for_layout(
    root: &Path,
    reserve_private_top_level: bool,
) -> Result<Snapshot, FolderImportError> {
    let expected_root = directory_identity(root)?;
    let pinned = PinnedWorkspaceRoot::open(root.to_path_buf())
        .map_err(|error| FolderImportError::io("pin snapshot root", root, error))?;
    pinned
        .ensure_identity(expected_root.device, expected_root.inode)
        .map_err(|error| FolderImportError::io("verify snapshot root", root, error))?;
    let filesystem = pinned.filesystem();
    let repository_path = root.join(REPOSITORY_IGNORE_FILE_NAME);
    let workspace_path = root.join(mesh_store::WORKSPACE_EXCLUSION_FILE_NAME);
    let repository_text = read_confined_exclusion_file(
        &filesystem,
        Path::new(REPOSITORY_IGNORE_FILE_NAME),
        &repository_path,
    )?;
    let workspace_text = read_confined_exclusion_file(
        &filesystem,
        Path::new(mesh_store::WORKSPACE_EXCLUSION_FILE_NAME),
        &workspace_path,
    )?;
    let exclusions = EffectiveExclusions::from_texts(
        Some(repository_path),
        repository_text,
        None,
        Some(workspace_path),
        workspace_text,
    )
    .map_err(|error| FolderImportError::ExclusionRulesUnavailable {
        path: root.to_path_buf(),
        detail: error.to_string(),
    })?;
    snapshot_with_pinned_root(
        root,
        expected_root,
        &pinned,
        Some(&exclusions),
        reserve_private_top_level,
    )
}

fn snapshot_with_pinned_root(
    root: &Path,
    expected_root: DirectoryIdentity,
    pinned: &PinnedWorkspaceRoot,
    exclusions: Option<&EffectiveExclusions>,
    reserve_private_top_level: bool,
) -> Result<Snapshot, FolderImportError> {
    pinned
        .ensure_identity(expected_root.device, expected_root.inode)
        .map_err(|error| FolderImportError::io("verify snapshot root", root, error))?;
    let filesystem = pinned.filesystem();
    let mut pending = vec![(PathBuf::new(), None::<PathBuf>)];
    let mut directories = BTreeSet::new();
    let mut file_paths = Vec::new();
    while let Some((relative, excluded_ancestor)) = pending.pop() {
        let directory = root.join(&relative);
        let entries = filesystem
            .read_directory_names(&relative)
            .map_err(|error| FolderImportError::io("read_dir", &directory, error))?;
        for name in entries {
            let relative = relative.join(name);
            let path = root.join(&relative);
            let versioned = if let Some(exclusions) = exclusions {
                let candidate = relative
                    .to_str()
                    .ok_or_else(|| FolderImportError::UnsupportedEntry { path: path.clone() })?;
                let versioned = if reserve_private_top_level {
                    exclusions.versions_path(candidate)
                } else {
                    exclusions.versions_presented_path(candidate)
                };
                versioned.map_err(|error| FolderImportError::ExclusionRulesUnavailable {
                    path: path.clone(),
                    detail: error.to_string(),
                })?
            } else {
                true
            };
            if versioned {
                if let Some(excluded_ancestor) = &excluded_ancestor {
                    return Err(FolderImportError::ReincludedPathBelowExcludedAncestor {
                        path,
                        excluded_ancestor: root.join(excluded_ancestor),
                    });
                }
            } else if !exclusions.is_some_and(EffectiveExclusions::has_reinclusion_rules) {
                continue;
            }
            #[cfg(test)]
            BEFORE_SNAPSHOT_ENTRY_OPEN.with(|hook| {
                if let Some(hook) = hook.borrow_mut().take() {
                    hook(&relative);
                }
            });
            let entry = match filesystem.inspect_entry(&relative) {
                Ok(entry) => entry,
                Err(error) if is_no_follow_refusal(&error) && !versioned => {
                    continue;
                }
                Err(error) if is_no_follow_refusal(&error) => {
                    return Err(FolderImportError::UnsupportedEntry { path });
                }
                Err(error) => {
                    return Err(FolderImportError::io("open snapshot entry", &path, error));
                }
            };
            let metadata = entry
                .metadata()
                .map_err(|error| FolderImportError::io("metadata", &path, error))?;
            if !versioned {
                if metadata.is_dir() {
                    let excluded_ancestor = excluded_ancestor
                        .clone()
                        .unwrap_or_else(|| relative.clone());
                    pending.push((relative, Some(excluded_ancestor)));
                }
                continue;
            }
            if metadata.is_dir() {
                directories.insert(relative.clone());
                pending.push((relative, None));
            } else if metadata.is_file() {
                file_paths.push(relative);
            } else {
                return Err(FolderImportError::UnsupportedEntry { path });
            }
        }
    }
    file_paths.sort();
    let mut files = Vec::with_capacity(file_paths.len());
    for relative in file_paths {
        let path = root.join(&relative);
        let entry = match filesystem.inspect_entry(&relative) {
            Ok(entry) => entry,
            Err(error) if is_no_follow_refusal(&error) => {
                return Err(FolderImportError::UnsupportedEntry { path });
            }
            Err(error) => {
                return Err(FolderImportError::io("open snapshot file", &path, error));
            }
        };
        let metadata = entry
            .metadata()
            .map_err(|error| FolderImportError::io("metadata", &path, error))?;
        if !metadata.is_file() {
            return Err(FolderImportError::UnsupportedEntry { path });
        }
        let executable = metadata_is_executable(&metadata);
        let (bytes, digest) = hash_open_file(entry, &path)?;
        files.push(ImportedFile {
            relative_path: relative,
            bytes,
            digest,
            executable,
        });
    }
    let total_bytes = files.iter().try_fold(0_u64, |total, file| {
        total.checked_add(file.bytes()).ok_or_else(|| {
            FolderImportError::io(
                "sum imported bytes",
                root,
                io::Error::new(io::ErrorKind::InvalidData, "folder byte count exceeds u64"),
            )
        })
    })?;
    let digest = summary_digest(&files, &directories);
    Ok(Snapshot {
        summary: ImportSummary {
            files,
            directories: u64::try_from(directories.len()).unwrap_or(u64::MAX),
            total_bytes,
            digest,
        },
        directories,
    })
}

fn read_confined_exclusion_file(
    filesystem: &crate::root_authority::PinnedRootFs,
    relative: &Path,
    display_path: &Path,
) -> Result<Option<String>, FolderImportError> {
    let mut file = match filesystem.inspect_entry(relative) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(FolderImportError::ExclusionRulesUnavailable {
                path: display_path.to_path_buf(),
                detail: error.to_string(),
            })
        }
    };
    let metadata =
        file.metadata()
            .map_err(|error| FolderImportError::ExclusionRulesUnavailable {
                path: display_path.to_path_buf(),
                detail: error.to_string(),
            })?;
    if !metadata.is_file() {
        return Err(FolderImportError::ExclusionRulesUnavailable {
            path: display_path.to_path_buf(),
            detail: "the rules source is not a regular file".to_owned(),
        });
    }
    if metadata.len() > MAX_EXCLUSION_FILE_BYTES {
        return Err(FolderImportError::ExclusionRulesUnavailable {
            path: display_path.to_path_buf(),
            detail: format!(
                "the rules source exceeds the {} byte safety bound",
                MAX_EXCLUSION_FILE_BYTES
            ),
        });
    }
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
    (&mut file)
        .take(MAX_EXCLUSION_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| FolderImportError::ExclusionRulesUnavailable {
            path: display_path.to_path_buf(),
            detail: error.to_string(),
        })?;
    if bytes.len() as u64 > MAX_EXCLUSION_FILE_BYTES {
        return Err(FolderImportError::ExclusionRulesUnavailable {
            path: display_path.to_path_buf(),
            detail: format!(
                "the rules source exceeds the {} byte safety bound",
                MAX_EXCLUSION_FILE_BYTES
            ),
        });
    }
    if bytes.len() as u64 != metadata.len() {
        return Err(FolderImportError::ExclusionRulesUnavailable {
            path: display_path.to_path_buf(),
            detail: "the rules source changed while it was read".to_owned(),
        });
    }
    String::from_utf8(bytes).map(Some).map_err(|error| {
        FolderImportError::ExclusionRulesUnavailable {
            path: display_path.to_path_buf(),
            detail: format!("the rules source is not UTF-8: {error}"),
        }
    })
}

fn is_no_follow_refusal(error: &io::Error) -> bool {
    #[cfg(target_os = "macos")]
    const LOOP_ERROR: i32 = 62;
    #[cfg(target_os = "linux")]
    const LOOP_ERROR: i32 = 40;
    error.raw_os_error() == Some(LOOP_ERROR)
}

fn hash_open_file(mut file: File, path: &Path) -> Result<(u64, Digest32), FolderImportError> {
    let mut hasher = Blake3::hasher();
    let mut total = 0_u64;
    let mut buffer = [0_u8; FILE_IO_BUFFER_BYTES];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| FolderImportError::io("read", path, error))?;
        if read == 0 {
            break;
        }
        total = total.checked_add(read as u64).ok_or_else(|| {
            FolderImportError::io(
                "read",
                path,
                io::Error::new(io::ErrorKind::InvalidData, "file length exceeds u64"),
            )
        })?;
        hasher.update(&buffer[..read]);
    }
    Ok((total, hasher.finalize()))
}

fn summary_digest(files: &[ImportedFile], directories: &BTreeSet<PathBuf>) -> Digest32 {
    let mut hasher = Blake3::hasher();
    hasher.update(b"mesh-folder-import-summary/2\0");
    for directory in directories {
        hasher.update(b"d");
        update_path(&mut hasher, directory);
    }
    for file in files {
        hasher.update(b"f");
        update_path(&mut hasher, &file.relative_path);
        hasher.update(&file.bytes.to_be_bytes());
        hasher.update(file.digest.as_bytes());
        hasher.update(&[u8::from(file.executable)]);
    }
    hasher.finalize()
}

#[cfg(unix)]
fn metadata_is_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn metadata_is_executable(_metadata: &fs::Metadata) -> bool {
    false
}

fn without_derived_index(mut snapshot: Snapshot, presented_layout: bool) -> Snapshot {
    if presented_layout {
        return snapshot;
    }
    snapshot.summary.files.retain(|file| {
        let path = file.relative_path.to_str();
        let namespaced = file
            .relative_path
            .strip_prefix(crate::workspace::STORAGE_DIRECTORY_NAME)
            .ok()
            .and_then(Path::to_str);
        ![path, namespaced].into_iter().flatten().any(|candidate| {
            matches!(
                candidate,
                DATABASE_FILE_NAME
                    | "metadata.sqlite-wal"
                    | "metadata.sqlite-shm"
                    | ".mesh-recovery.sqlite"
                    | ".mesh-recovery.sqlite-wal"
                    | ".mesh-recovery.sqlite-shm"
                    | crate::workspace_custody::LOCK_FILE
                    | crate::workspace_custody::RECORD_FILE
                    | crate::workspace_custody::TEMP_FILE
            )
        })
    });
    snapshot.summary.total_bytes = snapshot.summary.files.iter().map(ImportedFile::bytes).sum();
    snapshot.summary.digest = summary_digest(&snapshot.summary.files, &snapshot.directories);
    snapshot
}

fn update_path(hasher: &mut mesh_types::Blake3Hasher, path: &Path) {
    let bytes = path_bytes(path);
    hasher.update(&(bytes.len() as u64).to_be_bytes());
    hasher.update(bytes.as_ref());
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> std::borrow::Cow<'_, [u8]> {
    use std::os::unix::ffi::OsStrExt as _;
    std::borrow::Cow::Borrowed(path.as_os_str().as_bytes())
}

#[cfg(not(unix))]
fn path_bytes(path: &Path) -> std::borrow::Cow<'_, [u8]> {
    std::borrow::Cow::Owned(path.as_os_str().to_string_lossy().as_bytes().to_vec())
}

#[cfg(test)]
fn copy_snapshot(
    source: &Path,
    source_identity: DirectoryIdentity,
    destination: &Path,
    destination_identity: DirectoryIdentity,
    snapshot: &Snapshot,
) -> Result<(), FolderImportError> {
    let destination_root = PinnedWorkspaceRoot::open(destination.to_path_buf())
        .map_err(|error| FolderImportError::io("pin destination directory", destination, error))?;
    copy_snapshot_with_destination_root(
        source,
        source_identity,
        destination,
        destination_identity,
        &destination_root,
        snapshot,
    )
}

fn copy_snapshot_with_destination_root(
    source: &Path,
    source_identity: DirectoryIdentity,
    destination: &Path,
    destination_identity: DirectoryIdentity,
    destination_root: &PinnedWorkspaceRoot,
    snapshot: &Snapshot,
) -> Result<(), FolderImportError> {
    let source_root = PinnedWorkspaceRoot::open(source.to_path_buf())
        .map_err(|error| FolderImportError::io("pin source directory", source, error))?;
    source_root
        .ensure_identity(source_identity.device, source_identity.inode)
        .map_err(|error| FolderImportError::io("verify source directory", source, error))?;
    let source_filesystem = source_root.filesystem();
    destination_root
        .ensure_identity(destination_identity.device, destination_identity.inode)
        .map_err(|error| {
            FolderImportError::io("verify destination directory", destination, error)
        })?;
    let destination_filesystem = destination_root.filesystem();
    for relative in &snapshot.directories {
        destination_filesystem
            .create_dir_all(relative)
            .map_err(|error| {
                FolderImportError::io(
                    "create destination directory",
                    destination.join(relative),
                    error,
                )
            })?;
    }
    for file in &snapshot.summary.files {
        let from = source.join(&file.relative_path);
        let to = destination.join(&file.relative_path);
        let mut input = source_filesystem
            .read_file(&file.relative_path)
            .map_err(|error| FolderImportError::io("open source file", &from, error))?;
        let metadata = input
            .metadata()
            .map_err(|error| FolderImportError::io("inspect source file", &from, error))?;
        if !metadata.is_file() {
            return Err(FolderImportError::UnsupportedEntry { path: from });
        }
        if metadata_is_executable(&metadata) != file.executable {
            return Err(FolderImportError::VerificationMismatch {
                paths: vec![file.relative_path.clone()],
            });
        }
        let mut copied_bytes = 0_u64;
        let mut copied_hasher = Blake3::hasher();
        destination_filesystem
            .write_new_file_with(&file.relative_path, metadata.permissions(), |output| {
                let mut buffer = [0_u8; FILE_IO_BUFFER_BYTES];
                loop {
                    let read = input.read(&mut buffer)?;
                    if read == 0 {
                        break;
                    }
                    copied_bytes = copied_bytes.checked_add(read as u64).ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "file length exceeds u64")
                    })?;
                    copied_hasher.update(&buffer[..read]);
                    output.write_all(&buffer[..read])?;
                }
                Ok(())
            })
            .map_err(|error| FolderImportError::io("write destination file", &to, error))?;
        if copied_bytes != file.bytes || copied_hasher.finalize() != file.digest {
            return Err(FolderImportError::VerificationMismatch {
                paths: vec![file.relative_path.clone()],
            });
        }
    }
    Ok(())
}

fn differences(before: &Snapshot, source: &Snapshot, copied: &Snapshot) -> Vec<PathBuf> {
    let mut paths = snapshot_difference(before, source);
    paths.extend(snapshot_difference(before, copied));
    paths.sort();
    paths.dedup();
    paths
}

fn snapshot_difference(left: &Snapshot, right: &Snapshot) -> Vec<PathBuf> {
    let left_files: BTreeMap<_, _> = left
        .summary
        .files
        .iter()
        .map(|file| {
            (
                &file.relative_path,
                (file.bytes, file.digest, file.executable),
            )
        })
        .collect();
    let right_files: BTreeMap<_, _> = right
        .summary
        .files
        .iter()
        .map(|file| {
            (
                &file.relative_path,
                (file.bytes, file.digest, file.executable),
            )
        })
        .collect();
    let mut paths: BTreeSet<PathBuf> = left_files
        .keys()
        .chain(right_files.keys())
        .filter(|path| left_files.get(*path) != right_files.get(*path))
        .map(|path| (*path).clone())
        .collect();
    paths.extend(
        left.directories
            .symmetric_difference(&right.directories)
            .cloned(),
    );
    paths.into_iter().collect()
}

fn imported_subset_difference(imported: &Snapshot, managed: &Snapshot) -> Vec<PathBuf> {
    let managed_files: BTreeMap<_, _> = managed
        .summary
        .files
        .iter()
        .map(|file| {
            (
                &file.relative_path,
                (file.bytes, file.digest, file.executable),
            )
        })
        .collect();
    let mut paths = imported
        .summary
        .files
        .iter()
        .filter(|file| {
            managed_files.get(&file.relative_path)
                != Some(&(file.bytes, file.digest, file.executable))
        })
        .map(|file| file.relative_path.clone())
        .collect::<Vec<_>>();
    paths.extend(
        imported
            .directories
            .iter()
            .filter(|directory| !managed.directories.contains(*directory))
            .cloned(),
    );
    paths
}

struct ImportHead;

impl HeadDerivation for ImportHead {
    fn resulting_head(&self, commitment: &TransitionCommitment) -> HeadId {
        HeadId::from_bytes(*Blake3::digest_bytes(&commitment.canonical_bytes()).as_bytes())
    }
}

fn ingest_private_workspace(
    destination: &Path,
    destination_pinned: &PinnedWorkspaceRoot,
    storage_root: &Path,
    storage_pinned: &PinnedWorkspaceRoot,
    imported: &Snapshot,
    purpose: ImportPurpose,
) -> Result<ManagedImportOutcome, FolderImportError> {
    let mut directories = imported.directories.iter().cloned().collect::<Vec<_>>();
    directories.sort_by(|left, right| {
        left.components()
            .count()
            .cmp(&right.components().count())
            .then_with(|| left.cmp(right))
    });

    let root = import_object_id(b"root", Path::new(""));
    let mut directory_ids = BTreeMap::new();
    directory_ids.insert(PathBuf::new(), root);
    let mut operations = Vec::new();
    for relative in directories {
        let object = import_object_id(b"directory", &relative);
        let parent = relative.parent().unwrap_or(Path::new(""));
        let parent_id = *directory_ids.get(parent).ok_or_else(|| {
            workspace_import_error(
                destination,
                format!("directory parent {} was not prepared", parent.display()),
            )
        })?;
        let name = normalized_component(&relative)?;
        operations.push(Operation::CreateDirectory { object_id: object });
        operations.push(Operation::LinkDirectoryEntry {
            directory_id: parent_id,
            name,
            object_id: object,
            version_id: VersionId::from_bytes([0; 32]),
        });
        directory_ids.insert(relative, object);
    }

    let config = ChunkingConfig::default();
    let mut manifests = BTreeMap::new();
    let mut cas_objects = BTreeMap::new();
    let destination_filesystem = destination_pinned.filesystem();
    for file in &imported.summary.files {
        let path = destination.join(&file.relative_path);
        let mut reader = destination_filesystem
            .read_file(&file.relative_path)
            .map_err(|error| FolderImportError::io("read", &path, error))?;
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
            .map_err(|error| FolderImportError::io("read", &path, error))?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) != file.bytes
            || Blake3::digest_bytes(&bytes) != file.digest
        {
            return Err(FolderImportError::VerificationMismatch {
                paths: vec![file.relative_path.clone()],
            });
        }

        let prepared =
            PreparedCheckpointFile::from_bytes(&bytes, &config, ManifestPagingPolicy::flat())
                .map_err(|error| workspace_import_error(destination, error))?;
        let manifest_id = prepared.manifest().id;
        let (manifest, chunks, paged) = prepared.into_parts();
        debug_assert!(
            paged.is_none(),
            "flat import policy produced physical pages"
        );
        manifests.entry(manifest.id).or_insert(manifest);
        for chunk in chunks {
            let digest = RecordDigest::from_bytes(*Blake3::digest_bytes(&chunk).as_bytes());
            cas_objects.entry(digest).or_insert(chunk);
        }

        let object = import_object_id(b"file", &file.relative_path);
        let version = import_version_id(&file.relative_path, file.digest, file.executable);
        let parent = file.relative_path.parent().unwrap_or(Path::new(""));
        let parent_id = *directory_ids.get(parent).ok_or_else(|| {
            workspace_import_error(
                destination,
                format!("file parent {} was not prepared", parent.display()),
            )
        })?;
        operations.push(Operation::CreateFile { object_id: object });
        operations.push(Operation::WriteFileVersion {
            object_id: object,
            version_id: version,
            parent_versions: Vec::new(),
            manifest_id: ManifestId::from_bytes(*manifest_id.as_bytes()),
            portable_metadata: PortableMetadata::new(file.executable),
        });
        operations.push(Operation::LinkDirectoryEntry {
            directory_id: parent_id,
            name: normalized_component(&file.relative_path)?,
            object_id: object,
            version_id: version,
        });
    }

    // An empty received tree still needs a durable, explicit root identity.
    // Ordinary user imports refuse empty trees before reaching ingestion.
    if operations.is_empty() {
        operations.push(Operation::InitializeWorkspace { root_id: root });
    }

    let workspace_id = WorkspaceId::from_bytes(import_id16(b"workspace", imported.summary.digest));
    let actor_bytes = *Blake3::digest_bytes(b"mesh.local-folder-import.actor/1").as_bytes();
    let actor_id = ActorId::from_bytes(actor_bytes);
    let session_id = SessionId::from_bytes(import_id16(b"session", imported.summary.digest));
    let changeset = ChangeSetDraft::new(
        workspace_id,
        actor_id,
        session_id,
        ActorSequence::new(1),
        Hlc::new(0, 0),
    )
    .causal_parents(CausalParents::genesis())
    .base_head(HeadId::from_bytes([0; 32]))
    .policy_epoch(PolicyEpoch::new(1))
    .seal(operations, &ImportHead, Signature::from_bytes([0; 64]));
    let payload = encode_canonical(&changeset);
    let operation_id = RecordDigest::from_bytes(*Blake3::digest_bytes(&payload).as_bytes());
    cas_objects.entry(operation_id).or_insert(payload);

    let operation = OperationRecord {
        id: operation_id,
        actor: RecordDigest::from_bytes(actor_bytes),
        actor_sequence: 1,
        hlc_millis: 0,
        hlc_counter: 0,
        policy_epoch: 1,
        session: EntityUuid::from_bytes(*session_id.as_bytes()),
        payload_digest: operation_id,
        parents: Vec::new(),
    };
    let checkpoint = Checkpoint {
        manifests: manifests.into_values().collect(),
        operations: vec![operation],
        ..Checkpoint::default()
    };
    let manifest_count = checkpoint.manifests.len();
    let records = checkpoint.records();
    let linked_bytes = if purpose == ImportPurpose::Received {
        #[cfg(test)]
        BEFORE_WORKSPACE_COMMIT.with(|hook| {
            if let Some(hook) = hook.borrow_mut().take() {
                hook();
            }
        });
        received_genesis::finish(
            storage_pinned,
            storage_root,
            checkpoint,
            cas_objects.into_values().collect(),
        )
        .map_err(|error| workspace_import_error(destination, error))?
    } else {
        let mut workspace = OpenWorkspace::open_prepared_layout(
            destination,
            destination_pinned.clone(),
            storage_root,
            storage_pinned.clone(),
            &crate::TrustedReviewers::default(),
        )
        .map_err(|error| workspace_import_error(destination, error))?;
        #[cfg(test)]
        BEFORE_WORKSPACE_COMMIT.with(|hook| {
            if let Some(hook) = hook.borrow_mut().take() {
                hook();
            }
        });
        let cas = Cas::<_, mesh_cas::Blake3>::with_filesystem(
            workspace.storage_root().as_path().to_path_buf(),
            workspace.storage_pinned_root().filesystem(),
        )
        .map_err(|error| workspace_import_error(destination, error))?;
        let mut promoter = CasChunkPromoter::new(&cas);
        DurableCommit::new(
            workspace.checkpoint_store_mut(),
            &mut promoter,
            cas_objects.into_values().collect(),
            checkpoint,
        )
        .finish()
        .map_err(|error| workspace_import_error(destination, error))?;
        let linked_bytes = promoter.linked_bytes();
        journal_records(workspace.checkpoint_journal_mut(), records.iter())
            .map_err(|error| FolderImportError::io("append", workspace.record_file(), error))?;
        drop(workspace);

        linked_bytes
    };

    let reopened = OpenWorkspace::open_prepared_layout(
        destination,
        destination_pinned.clone(),
        storage_root,
        storage_pinned.clone(),
        &crate::TrustedReviewers::default(),
    )
    .map_err(|error| workspace_import_error(destination, error))?;
    let expected = imported_entry_paths(imported)?;
    let actual = reopened
        .entries()
        .iter()
        .map(|entry| entry.path().to_owned())
        .collect::<BTreeSet<_>>();
    if !reopened.names_answered() || expected != actual || reopened.operations() != 1 {
        return Err(workspace_import_error(
            destination,
            "the imported journal did not materialize the verified tree exactly",
        ));
    }
    let entries = actual.len();
    drop(reopened);

    // The SQLite index is derived entirely from the immutable journal. During the unconfirmed
    // transaction it stays in memory so a path-only SQLite open cannot be redirected by replacing
    // the displayed parent. Publish only an empty, descriptor-confined placeholder here; the
    // first confirmed open rebuilds it from the journal through the ordinary recovery path.
    let storage_filesystem = storage_pinned.filesystem();
    let database_relative = Path::new(DATABASE_FILE_NAME);
    storage_filesystem
        .write_new_file(database_relative, &[], fs::Permissions::from_mode(0o600))
        .and_then(|()| storage_filesystem.sync_file(database_relative))
        .and_then(|()| storage_filesystem.sync_dir(Path::new("")))
        .map_err(|error| {
            FolderImportError::io(
                "prepare derived index",
                storage_root.join(DATABASE_FILE_NAME),
                error,
            )
        })?;

    Ok(ManagedImportOutcome {
        operation: operation_id,
        manifests: manifest_count,
        entries,
        linked_bytes,
    })
}

fn require_import_entries(source: &Path, snapshot: &Snapshot) -> Result<(), FolderImportError> {
    if snapshot.summary.files.is_empty() && snapshot.directories.is_empty() {
        return Err(FolderImportError::NoImportableEntries {
            path: source.to_path_buf(),
        });
    }
    Ok(())
}

fn normalized_component(path: &Path) -> Result<NormalizedName, FolderImportError> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| FolderImportError::UnsupportedEntry {
            path: path.to_path_buf(),
        })?;
    NormalizedName::new(name.to_owned()).map_err(|_| FolderImportError::UnsupportedEntry {
        path: path.to_path_buf(),
    })
}

fn import_object_id(kind: &[u8], path: &Path) -> ObjectId {
    let mut hasher = Blake3::hasher();
    hasher.update(IMPORT_DOMAIN);
    hasher.update(kind);
    hasher.update(&[0]);
    hasher.update(path_bytes(path).as_ref());
    let digest = hasher.finalize();
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest.as_bytes()[..16]);
    ObjectId::from_bytes(bytes)
}

fn imported_path_objects(imported: &Snapshot) -> BTreeMap<PathBuf, ObjectId> {
    imported
        .directories
        .iter()
        .map(|path| (path.clone(), import_object_id(b"directory", path)))
        .chain(imported.summary.files.iter().map(|file| {
            (
                file.relative_path.clone(),
                import_object_id(b"file", &file.relative_path),
            )
        }))
        .collect()
}

fn import_version_id(path: &Path, content: Digest32, executable: bool) -> VersionId {
    let mut hasher = Blake3::hasher();
    hasher.update(IMPORT_DOMAIN);
    hasher.update(b"version\0");
    hasher.update(path_bytes(path).as_ref());
    hasher.update(content.as_bytes());
    hasher.update(&[u8::from(executable)]);
    VersionId::from_bytes(*hasher.finalize().as_bytes())
}

fn import_id16(kind: &[u8], summary: Digest32) -> [u8; 16] {
    let mut hasher = Blake3::hasher();
    hasher.update(IMPORT_DOMAIN);
    hasher.update(kind);
    hasher.update(summary.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest.as_bytes()[..16]);
    bytes
}

fn imported_entry_paths(imported: &Snapshot) -> Result<BTreeSet<String>, FolderImportError> {
    imported
        .directories
        .iter()
        .chain(
            imported
                .summary
                .files
                .iter()
                .map(|file| &file.relative_path),
        )
        .map(|path| {
            path.to_str()
                .map(|path| path.replace(std::path::MAIN_SEPARATOR, "/"))
                .ok_or_else(|| FolderImportError::UnsupportedEntry { path: path.clone() })
        })
        .collect()
}

fn workspace_import_error(destination: &Path, error: impl fmt::Display) -> FolderImportError {
    FolderImportError::io(
        "initialize_workspace",
        destination,
        io::Error::other(error.to_string()),
    )
}

fn absolute_destination(destination: &Path) -> Result<PathBuf, FolderImportError> {
    let absolute = if destination.is_absolute() {
        destination.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| FolderImportError::io("current_dir", ".", error))?
            .join(destination)
    };
    if absolute
        .file_name()
        .and_then(|name| name.to_str())
        .is_none()
    {
        return Err(FolderImportError::InvalidDestination { path: absolute });
    }
    Ok(absolute)
}

fn markers(destination: &Path) -> Result<ImportMarkers, FolderImportError> {
    let name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| FolderImportError::InvalidDestination {
            path: destination.to_path_buf(),
        })?;
    let parent = destination
        .parent()
        .ok_or_else(|| FolderImportError::InvalidDestination {
            path: destination.to_path_buf(),
        })?;
    Ok(ImportMarkers {
        claim: parent.join(format!(".{name}.mesh-import-claim")),
        owned: parent.join(format!(".{name}.mesh-import-owned")),
        receipt: parent.join(format!(".{name}.mesh-import-receipt")),
    })
}

fn receipt_bytes(identity: DirectoryIdentity, snapshot: &Snapshot) -> Vec<u8> {
    encode_receipt(
        ReceiptEncoding::MetadataPlain,
        identity,
        None,
        snapshot,
        snapshot,
        false,
    )
}

fn managed_receipt_bytes(
    identity: DirectoryIdentity,
    imported: &Snapshot,
    managed: &Snapshot,
) -> Vec<u8> {
    encode_receipt(
        ReceiptEncoding::MetadataManaged,
        identity,
        None,
        imported,
        managed,
        true,
    )
}

fn presented_receipt_bytes(
    identity: DirectoryIdentity,
    store: &PresentedStore,
    imported: &Snapshot,
    managed: &Snapshot,
    allows_derived_index_changes: bool,
) -> Vec<u8> {
    encode_receipt(
        ReceiptEncoding::MetadataPresented,
        identity,
        Some(store),
        imported,
        managed,
        allows_derived_index_changes,
    )
}

fn encode_receipt(
    encoding: ReceiptEncoding,
    identity: DirectoryIdentity,
    store: Option<&PresentedStore>,
    imported: &Snapshot,
    managed: &Snapshot,
    allows_derived_index_changes: bool,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(encoding.magic());
    push_u64(&mut bytes, identity.device);
    push_u64(&mut bytes, identity.inode);
    if encoding.presented() {
        let store = store.expect("presented receipt requires a store");
        push_u64(&mut bytes, store.identity.device);
        push_u64(&mut bytes, store.identity.inode);
        bytes.push(u8::from(allows_derived_index_changes));
    }
    push_snapshot(&mut bytes, imported, encoding.includes_executable());
    if encoding.managed() || encoding.presented() && allows_derived_index_changes {
        push_snapshot(&mut bytes, managed, encoding.includes_executable());
    }
    bytes
}

fn confirmed_receipt_bytes(
    identity: DirectoryIdentity,
    store: Option<&PresentedStore>,
    imported: &Snapshot,
    managed: &Snapshot,
    allows_derived_index_changes: bool,
) -> Vec<u8> {
    match store {
        Some(store) => presented_receipt_bytes(
            identity,
            store,
            imported,
            managed,
            allows_derived_index_changes,
        ),
        None if allows_derived_index_changes => managed_receipt_bytes(identity, imported, managed),
        None => receipt_bytes(identity, imported),
    }
}

fn push_snapshot(bytes: &mut Vec<u8>, snapshot: &Snapshot, includes_executable: bool) {
    push_u64(
        bytes,
        u64::try_from(snapshot.directories.len()).unwrap_or(u64::MAX),
    );
    for directory in &snapshot.directories {
        push_path(bytes, directory);
    }
    push_u64(
        bytes,
        u64::try_from(snapshot.summary.files.len()).unwrap_or(u64::MAX),
    );
    for file in &snapshot.summary.files {
        push_path(bytes, &file.relative_path);
        push_u64(bytes, file.bytes);
        bytes.extend_from_slice(file.digest.as_bytes());
        if includes_executable {
            bytes.push(u8::from(file.executable));
        }
    }
}

fn push_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn push_path(bytes: &mut Vec<u8>, path: &Path) {
    let path = path_bytes(path);
    push_u64(bytes, u64::try_from(path.len()).unwrap_or(u64::MAX));
    bytes.extend_from_slice(path.as_ref());
}

fn read_receipt(path: &Path) -> Result<ImportReceipt, FolderImportError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| FolderImportError::io("metadata", path, error))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(FolderImportError::UnrecognizedMarker {
            path: path.to_path_buf(),
        });
    }
    let bytes = fs::read(path).map_err(|error| FolderImportError::io("read", path, error))?;
    let parsed = parse_receipt(path, &bytes)?;
    let canonical = encode_receipt(
        parsed.encoding,
        parsed.identity,
        parsed.presented_store.as_ref(),
        &parsed.imported,
        &parsed.managed,
        parsed.allows_derived_index_changes,
    );
    if canonical != bytes {
        return Err(FolderImportError::UnrecognizedMarker {
            path: path.to_path_buf(),
        });
    }
    Ok(parsed)
}

fn parse_receipt(path: &Path, bytes: &[u8]) -> Result<ImportReceipt, FolderImportError> {
    let encoding = if bytes.starts_with(RECEIPT_MAGIC) {
        ReceiptEncoding::LegacyPlain
    } else if bytes.starts_with(MANAGED_RECEIPT_MAGIC) {
        ReceiptEncoding::LegacyManaged
    } else if bytes.starts_with(PRESENTED_RECEIPT_MAGIC) {
        ReceiptEncoding::LegacyPresented
    } else if bytes.starts_with(METADATA_RECEIPT_MAGIC) {
        ReceiptEncoding::MetadataPlain
    } else if bytes.starts_with(METADATA_MANAGED_RECEIPT_MAGIC) {
        ReceiptEncoding::MetadataManaged
    } else if bytes.starts_with(METADATA_PRESENTED_RECEIPT_MAGIC) {
        ReceiptEncoding::MetadataPresented
    } else {
        return Err(unrecognized(path));
    };
    let mut reader = ReceiptReader {
        bytes,
        at: encoding.magic().len(),
    };
    let identity = DirectoryIdentity {
        device: reader.u64(path)?,
        inode: reader.u64(path)?,
    };
    let (presented_store, managed) = if encoding.presented() {
        let store = PresentedStore {
            root: path
                .parent()
                .ok_or_else(|| unrecognized(path))?
                .to_path_buf(),
            identity: DirectoryIdentity {
                device: reader.u64(path)?,
                inode: reader.u64(path)?,
            },
        };
        let flag = reader.take(path, 1)?[0];
        if flag > 1 {
            return Err(unrecognized(path));
        }
        (Some(store), flag == 1)
    } else {
        (None, encoding.managed())
    };
    let imported = reader.snapshot(path, encoding.includes_executable())?;
    let managed_snapshot = if managed {
        reader.snapshot(path, encoding.includes_executable())?
    } else {
        imported.clone()
    };
    if reader.at != bytes.len() {
        return Err(unrecognized(path));
    }
    Ok(ImportReceipt {
        encoding,
        identity,
        presented_store,
        imported,
        managed: managed_snapshot,
        allows_derived_index_changes: managed,
    })
}

fn parse_snapshot(
    reader: &mut ReceiptReader<'_>,
    path: &Path,
    includes_executable: bool,
) -> Result<Snapshot, FolderImportError> {
    let directory_count = reader.count(path, 8)?;
    let mut directories = BTreeSet::new();
    for _ in 0..directory_count {
        let directory = reader.path(path)?;
        if directory.as_os_str().is_empty()
            || directory.is_absolute()
            || !directories.insert(directory)
        {
            return Err(unrecognized(path));
        }
    }
    let file_count = reader.count(path, if includes_executable { 49 } else { 48 })?;
    let mut files = Vec::with_capacity(file_count);
    let mut total_bytes = 0u64;
    for _ in 0..file_count {
        let relative_path = reader.path(path)?;
        if relative_path.as_os_str().is_empty() || relative_path.is_absolute() {
            return Err(unrecognized(path));
        }
        let length = reader.u64(path)?;
        total_bytes = total_bytes
            .checked_add(length)
            .ok_or_else(|| unrecognized(path))?;
        let digest = Digest32::from_bytes(reader.array(path)?);
        let executable = if includes_executable {
            match reader.take(path, 1)?[0] {
                0 => false,
                1 => true,
                _ => return Err(unrecognized(path)),
            }
        } else {
            false
        };
        files.push(ImportedFile {
            relative_path,
            bytes: length,
            digest,
            executable,
        });
    }
    if files
        .windows(2)
        .any(|pair| pair[0].relative_path >= pair[1].relative_path)
    {
        return Err(unrecognized(path));
    }
    let digest = summary_digest(&files, &directories);
    Ok(Snapshot {
        summary: ImportSummary {
            files,
            directories: u64::try_from(directories.len()).unwrap_or(u64::MAX),
            total_bytes,
            digest,
        },
        directories,
    })
}

struct ReceiptReader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl ReceiptReader<'_> {
    fn snapshot(
        &mut self,
        path: &Path,
        includes_executable: bool,
    ) -> Result<Snapshot, FolderImportError> {
        parse_snapshot(self, path, includes_executable)
    }

    fn take<'a>(&'a mut self, path: &Path, length: usize) -> Result<&'a [u8], FolderImportError> {
        let end = self
            .at
            .checked_add(length)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| unrecognized(path))?;
        let value = &self.bytes[self.at..end];
        self.at = end;
        Ok(value)
    }

    fn array(&mut self, path: &Path) -> Result<[u8; 32], FolderImportError> {
        self.take(path, 32)?
            .try_into()
            .map_err(|_| unrecognized(path))
    }

    fn u64(&mut self, path: &Path) -> Result<u64, FolderImportError> {
        self.take(path, 8)?
            .try_into()
            .map(u64::from_be_bytes)
            .map_err(|_| unrecognized(path))
    }

    fn count(&mut self, path: &Path, minimum_bytes: usize) -> Result<usize, FolderImportError> {
        let count = usize::try_from(self.u64(path)?).map_err(|_| unrecognized(path))?;
        if count > self.bytes.len().saturating_sub(self.at) / minimum_bytes {
            return Err(unrecognized(path));
        }
        Ok(count)
    }

    fn path(&mut self, receipt: &Path) -> Result<PathBuf, FolderImportError> {
        let length = usize::try_from(self.u64(receipt)?).map_err(|_| unrecognized(receipt))?;
        path_from_bytes(self.take(receipt, length)?).ok_or_else(|| unrecognized(receipt))
    }
}

#[cfg(unix)]
fn path_from_bytes(bytes: &[u8]) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt as _;
    Some(std::ffi::OsString::from_vec(bytes.to_vec()).into())
}

#[cfg(not(unix))]
fn path_from_bytes(bytes: &[u8]) -> Option<PathBuf> {
    String::from_utf8(bytes.to_vec()).ok().map(PathBuf::from)
}

fn unrecognized(path: &Path) -> FolderImportError {
    FolderImportError::UnrecognizedMarker {
        path: path.to_path_buf(),
    }
}

fn verify_marker(path: &Path, expected: &[u8]) -> Result<(), FolderImportError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| FolderImportError::io("metadata", path, error))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(FolderImportError::UnrecognizedMarker {
            path: path.to_path_buf(),
        });
    }
    let contents = fs::read(path).map_err(|error| FolderImportError::io("read", path, error))?;
    if contents == expected {
        Ok(())
    } else {
        Err(FolderImportError::UnrecognizedMarker {
            path: path.to_path_buf(),
        })
    }
}

fn marker_exists(path: &Path) -> Result<bool, FolderImportError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(FolderImportError::io("metadata", path, error)),
    }
}

fn owned_marker_bytes(
    identity: DirectoryIdentity,
    presented_store: Option<&PresentedStore>,
) -> String {
    presented_store.map_or_else(
        || {
            format!(
                "{OWNED_MARKER_PREFIX} {}:{}\n",
                identity.device, identity.inode
            )
        },
        |store| {
            format!(
                "{PRESENTED_OWNED_MARKER_PREFIX} {}:{} {}:{}\n",
                identity.device, identity.inode, store.identity.device, store.identity.inode
            )
        },
    )
}

fn verify_owned_marker(
    marker: &Path,
    destination: &Path,
) -> Result<OwnedDestination, FolderImportError> {
    let metadata = fs::symlink_metadata(marker)
        .map_err(|error| FolderImportError::io("metadata", marker, error))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(FolderImportError::UnrecognizedMarker {
            path: marker.to_path_buf(),
        });
    }
    let contents =
        fs::read_to_string(marker).map_err(|error| FolderImportError::io("read", marker, error))?;
    let text = contents
        .strip_suffix('\n')
        .ok_or_else(|| unrecognized(marker))?;
    let (destination_text, store_text) = if let Some(value) = text
        .strip_prefix(PRESENTED_OWNED_MARKER_PREFIX)
        .and_then(|value| value.strip_prefix(' '))
    {
        let (destination, store) = value.split_once(' ').ok_or_else(|| unrecognized(marker))?;
        (destination, Some(store))
    } else {
        (
            text.strip_prefix(OWNED_MARKER_PREFIX)
                .and_then(|value| value.strip_prefix(' '))
                .ok_or_else(|| unrecognized(marker))?,
            None,
        )
    };
    let parse_identity = |value: &str| -> Result<DirectoryIdentity, FolderImportError> {
        let (device, inode) = value.split_once(':').ok_or_else(|| unrecognized(marker))?;
        Ok(DirectoryIdentity {
            device: device.parse().map_err(|_| unrecognized(marker))?,
            inode: inode.parse().map_err(|_| unrecognized(marker))?,
        })
    };
    let marker_identity = parse_identity(destination_text)?;
    if directory_identity(destination)? != marker_identity {
        Err(FolderImportError::OwnershipMismatch {
            destination: destination.to_path_buf(),
        })
    } else {
        let presented_store = store_text
            .map(|store| {
                Ok(PresentedStore {
                    root: destination
                        .parent()
                        .ok_or_else(|| unrecognized(marker))?
                        .to_path_buf(),
                    identity: parse_identity(store)?,
                })
            })
            .transpose()?;
        if let Some(store) = &presented_store {
            verify_presented_store(destination, store)?;
        }
        Ok(OwnedDestination {
            destination: marker_identity,
            presented_store,
        })
    }
}

fn verify_presented_store(
    destination: &Path,
    store: &PresentedStore,
) -> Result<(), FolderImportError> {
    if destination.parent() != Some(store.root.as_path()) {
        return Err(FolderImportError::OwnershipMismatch {
            destination: store.root.clone(),
        });
    }
    verify_directory_identity(&store.root, store.identity)?;
    verify_marker(
        &store
            .root
            .join(crate::workspace::PRESENTED_LAYOUT_MARKER_NAME),
        crate::workspace::PRESENTED_LAYOUT_MARKER_BYTES,
    )
}

#[cfg(unix)]
fn directory_identity(path: &Path) -> Result<DirectoryIdentity, FolderImportError> {
    use std::os::unix::fs::MetadataExt as _;

    let metadata = fs::symlink_metadata(path)
        .map_err(|error| FolderImportError::io("metadata", path, error))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(FolderImportError::OwnershipMismatch {
            destination: path.to_path_buf(),
        });
    }
    Ok(DirectoryIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(unix)]
fn resolved_directory_identity(path: &Path) -> Result<DirectoryIdentity, FolderImportError> {
    use std::os::unix::fs::MetadataExt as _;

    let metadata =
        fs::metadata(path).map_err(|error| FolderImportError::io("metadata", path, error))?;
    if !metadata.is_dir() {
        return Err(FolderImportError::OwnershipMismatch {
            destination: path.to_path_buf(),
        });
    }
    Ok(DirectoryIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(not(unix))]
fn directory_identity(path: &Path) -> Result<DirectoryIdentity, FolderImportError> {
    Err(FolderImportError::io(
        "directory_identity",
        path,
        io::Error::new(
            io::ErrorKind::Unsupported,
            "folder import requires stable directory identity on this platform",
        ),
    ))
}

#[cfg(not(unix))]
fn resolved_directory_identity(path: &Path) -> Result<DirectoryIdentity, FolderImportError> {
    directory_identity(path)
}

fn verify_directory_identity(
    destination: &Path,
    expected: DirectoryIdentity,
) -> Result<(), FolderImportError> {
    if directory_identity(destination)? == expected {
        Ok(())
    } else {
        Err(FolderImportError::OwnershipMismatch {
            destination: destination.to_path_buf(),
        })
    }
}

fn cleanup_owned<'a>(
    destination: &Path,
    identity: DirectoryIdentity,
    markers: impl IntoIterator<Item = &'a PathBuf>,
) -> Result<(), FolderImportError> {
    verify_directory_identity(destination, identity)?;
    fs::remove_dir_all(destination)
        .map_err(|error| FolderImportError::io("remove_dir_all", destination, error))?;
    for marker in markers {
        match fs::remove_file(marker) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(FolderImportError::io("remove_file", marker, error)),
        }
    }
    sync_directory(destination.parent().unwrap_or(Path::new(".")))
}

fn cleanup_import<'a>(
    destination: &Path,
    identity: DirectoryIdentity,
    presented_store: Option<&PresentedStore>,
    markers: impl IntoIterator<Item = &'a PathBuf>,
) -> Result<(), FolderImportError> {
    if let Some(store) = presented_store {
        verify_directory_identity(destination, identity)?;
        verify_presented_store(destination, store)?;
        fs::remove_dir_all(&store.root)
            .map_err(|error| FolderImportError::io("remove_dir_all", &store.root, error))?;
        sync_directory(store.root.parent().unwrap_or(Path::new(".")))
    } else {
        cleanup_owned(destination, identity, markers)
    }
}

fn cleanup_or<'a>(
    problem: FolderImportError,
    destination: &Path,
    identity: DirectoryIdentity,
    markers: impl IntoIterator<Item = &'a PathBuf>,
) -> FolderImportError {
    match cleanup_owned(destination, identity, markers) {
        Ok(()) => problem,
        Err(cleanup) => FolderImportError::CleanupFailed {
            destination: destination.to_path_buf(),
            reason: cleanup.to_string(),
        },
    }
}

fn cleanup_presented_prepare_or<'a>(
    problem: FolderImportError,
    storage_root: &Path,
    storage_identity: DirectoryIdentity,
    destination: &Path,
    markers: impl IntoIterator<Item = &'a PathBuf>,
) -> FolderImportError {
    match fs::symlink_metadata(destination) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            cleanup_or(problem, storage_root, storage_identity, markers)
        }
        Ok(_) => FolderImportError::CleanupFailed {
            destination: storage_root.to_path_buf(),
            reason: format!(
                "{problem}; the presented working directory still exists, so the private store was preserved"
            ),
        },
        Err(error) => FolderImportError::CleanupFailed {
            destination: storage_root.to_path_buf(),
            reason: format!(
                "{problem}; metadata {} failed during cleanup: {error}",
                destination.display()
            ),
        },
    }
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), FolderImportError> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|error| FolderImportError::io("sync_dir", path, error))
}

#[cfg(not(unix))]
fn sync_directory(path: &Path) -> Result<(), FolderImportError> {
    let _ = path;
    Err(FolderImportError::io(
        "sync_dir",
        path,
        io::Error::new(
            io::ErrorKind::Unsupported,
            "folder import requires durable directory synchronization on this platform",
        ),
    ))
}

#[cfg(test)]
mod copy_security_tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::sync::mpsc;
    use std::time::Duration;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "mesh-folder-import-{name}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn snapshot_does_not_follow_a_directory_replaced_after_enumeration() {
        let root = scratch("snapshot-directory-swap");
        let source = root.join("source");
        let retained = root.join("retained");
        let outside = root.join("outside");
        fs::create_dir_all(source.join("nested")).expect("source tree");
        fs::create_dir(&outside).expect("outside root");
        fs::write(source.join("nested/result.txt"), b"selected bytes\n").expect("source file");
        fs::write(outside.join("secret.txt"), b"outside bytes\n").expect("outside file");

        let source_for_hook = source.clone();
        let retained_for_hook = retained.clone();
        let outside_for_hook = outside.clone();
        BEFORE_SNAPSHOT_ENTRY_OPEN.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move |relative| {
                assert_eq!(relative, Path::new("nested"));
                fs::rename(source_for_hook.join("nested"), &retained_for_hook)
                    .expect("retain enumerated directory");
                symlink(&outside_for_hook, source_for_hook.join("nested"))
                    .expect("replace directory with link");
            }));
        });

        let refused = snapshot(&source).expect_err("linked replacement must be refused");
        assert!(matches!(
            refused,
            FolderImportError::UnsupportedEntry { path } if path == source.join("nested")
        ));
        assert_eq!(
            fs::read(outside.join("secret.txt")).expect("outside bytes retained"),
            b"outside bytes\n"
        );
        let _ = fs::remove_file(source.join("nested"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn import_hashes_and_copies_files_larger_than_its_fixed_io_buffer() {
        let root = scratch("streamed-file");
        let source = root.join("source");
        let destination = root.join("destination");
        fs::create_dir_all(&source).expect("source tree");
        fs::create_dir(&destination).expect("destination root");
        let bytes = (0..(FILE_IO_BUFFER_BYTES * 3 + 17))
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();
        fs::write(source.join("large.bin"), &bytes).expect("large source file");

        let captured = snapshot(&source).expect("streaming snapshot");
        assert_eq!(captured.summary.total_bytes(), bytes.len() as u64);
        copy_snapshot(
            &source,
            directory_identity(&source).expect("source identity"),
            &destination,
            directory_identity(&destination).expect("destination identity"),
            &captured,
        )
        .expect("streaming copy");
        assert_eq!(
            fs::read(destination.join("large.bin")).expect("copied bytes"),
            bytes
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn import_preview_lists_a_bounded_exact_file_sample() {
        let root = scratch("human-readable-preview");
        let source = root.join("source");
        fs::create_dir_all(&source).expect("source tree");
        for index in 0..26 {
            let path = source.join(format!("file-{index:02}.txt"));
            fs::write(path, format!("{index}\n")).expect("preview file");
        }
        let script = source.join("file-00.txt");
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("executable");

        let preview = preview_folder_import(&source)
            .expect("preview")
            .to_preview_json(false);
        let entries = preview
            .get("file_entries")
            .and_then(crate::ipc::Json::as_array)
            .expect("bounded entries");
        assert_eq!(entries.len(), MAX_IMPORT_PREVIEW_FILE_ENTRIES);
        assert_eq!(
            preview
                .get("files_not_listed")
                .and_then(crate::ipc::Json::as_u64),
            Some(2)
        );
        assert_eq!(
            entries[0].get("path").and_then(crate::ipc::Json::as_text),
            Some("file-00.txt")
        );
        assert_eq!(
            entries[0].get("bytes").and_then(crate::ipc::Json::as_text),
            Some("2")
        );
        assert_eq!(
            entries[0]
                .get("executable")
                .and_then(crate::ipc::Json::as_bool),
            Some(true)
        );
        assert_eq!(
            entries[23].get("path").and_then(crate::ipc::Json::as_text),
            Some("file-23.txt")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn metadata_receipts_bind_executable_state_and_legacy_receipts_remain_readable() {
        let root = scratch("receipt-executable-compatibility");
        let source = root.join("source");
        fs::create_dir_all(&source).expect("source tree");
        let script = source.join("run.sh");
        fs::write(&script, b"#!/bin/sh\nexit 0\n").expect("script");
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("executable");
        let captured = snapshot(&source).expect("metadata snapshot");
        assert!(captured.summary.files[0].executable());

        let identity = DirectoryIdentity {
            device: 0x1234,
            inode: 0x5678,
        };
        let current = encode_receipt(
            ReceiptEncoding::MetadataPlain,
            identity,
            None,
            &captured,
            &captured,
            false,
        );
        let current_path = root.join("current-receipt");
        let parsed = parse_receipt(&current_path, &current).expect("current receipt");
        assert!(parsed.imported.summary.files[0].executable());
        assert_eq!(
            encode_receipt(
                parsed.encoding,
                parsed.identity,
                parsed.presented_store.as_ref(),
                &parsed.imported,
                &parsed.managed,
                parsed.allows_derived_index_changes,
            ),
            current
        );

        let legacy = encode_receipt(
            ReceiptEncoding::LegacyPlain,
            identity,
            None,
            &captured,
            &captured,
            false,
        );
        let legacy_path = root.join("legacy-receipt");
        let parsed_legacy = parse_receipt(&legacy_path, &legacy).expect("legacy receipt");
        assert!(!parsed_legacy.imported.summary.files[0].executable());
        assert_eq!(
            encode_receipt(
                parsed_legacy.encoding,
                parsed_legacy.identity,
                parsed_legacy.presented_store.as_ref(),
                &parsed_legacy.imported,
                &parsed_legacy.managed,
                parsed_legacy.allows_derived_index_changes,
            ),
            legacy
        );

        let mut malformed = current;
        *malformed.last_mut().expect("executable byte") = 2;
        assert!(matches!(
            parse_receipt(&current_path, &malformed),
            Err(FolderImportError::UnrecognizedMarker { .. })
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn destination_symlink_cannot_redirect_snapshot_copy() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "mesh-folder-import-no-follow-{}-{nonce}",
            std::process::id()
        ));
        let source = root.join("source");
        let destination = root.join("destination");
        let outside = root.join("outside");
        fs::create_dir_all(source.join("nested")).expect("source tree");
        fs::create_dir(&destination).expect("destination root");
        fs::create_dir(&outside).expect("outside root");
        fs::write(source.join("nested/result.txt"), b"private agent result\n")
            .expect("source file");
        let captured = snapshot(&source).expect("capture source");

        symlink(&outside, destination.join("nested")).expect("replace nested destination");
        let refused = copy_snapshot(
            &source,
            directory_identity(&source).expect("source identity"),
            &destination,
            directory_identity(&destination).expect("destination identity"),
            &captured,
        );

        assert!(refused.is_err(), "a linked destination was accepted");
        assert!(
            !outside.join("result.txt").exists(),
            "the import copier followed a nested link and wrote outside its owned destination"
        );
        let _ = fs::remove_file(destination.join("nested"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn source_symlink_cannot_substitute_bytes_after_snapshot() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "mesh-folder-import-source-no-follow-{}-{nonce}",
            std::process::id()
        ));
        let source = root.join("source");
        let destination = root.join("destination");
        let outside = root.join("outside.txt");
        fs::create_dir_all(source.join("nested")).expect("source tree");
        fs::create_dir(&destination).expect("destination root");
        fs::write(source.join("nested/result.txt"), b"captured result\n").expect("source file");
        fs::write(&outside, b"substituted outside bytes\n").expect("outside file");
        let captured = snapshot(&source).expect("capture source");

        fs::remove_file(source.join("nested/result.txt")).expect("remove captured file");
        symlink(&outside, source.join("nested/result.txt")).expect("substitute source link");
        let refused = copy_snapshot(
            &source,
            directory_identity(&source).expect("source identity"),
            &destination,
            directory_identity(&destination).expect("destination identity"),
            &captured,
        );

        assert!(refused.is_err(), "a linked source was accepted");
        assert!(
            !destination.join("nested/result.txt").exists(),
            "bytes reached the destination through a substituted source link"
        );
        assert_eq!(
            fs::read(&outside).expect("outside bytes retained"),
            b"substituted outside bytes\n"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn replacement_destination_root_cannot_inherit_copy_authority() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "mesh-folder-import-replaced-root-{}-{nonce}",
            std::process::id()
        ));
        let source = root.join("source");
        let destination = root.join("destination");
        let displaced = root.join("displaced");
        fs::create_dir_all(&source).expect("source tree");
        fs::create_dir(&destination).expect("destination root");
        fs::write(source.join("result.txt"), b"private agent result\n").expect("source file");
        let captured = snapshot(&source).expect("capture source");
        let source_identity = directory_identity(&source).expect("source identity");
        let destination_identity = directory_identity(&destination).expect("destination identity");
        fs::rename(&destination, &displaced).expect("displace destination");
        fs::create_dir(&destination).expect("replacement destination");

        let refused = copy_snapshot(
            &source,
            source_identity,
            &destination,
            destination_identity,
            &captured,
        );

        assert!(refused.is_err(), "a replacement destination was accepted");
        assert!(!destination.join("result.txt").exists());
        assert!(!displaced.join("result.txt").exists());
        let _ = fs::remove_dir_all(root);
    }

    fn received_fixture(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let root = scratch(name);
        let source = root.join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("work.txt"), b"original input\n").unwrap();
        let store = root.join("worker.mesh");
        (root, source, store)
    }

    fn prepare_received(source: &Path, store: &Path) -> PreparedFolderImport {
        PreparedFolderImport::prepare_received_with_parent(
            source,
            store,
            &[],
            ProtectedWorkspaceRoot::inspect(store.parent().unwrap()).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn received_drop_retains_original_copy_and_ownership_marker() {
        let (root, source, store) = received_fixture("received-drop");
        let prepared = prepare_received(&source, &store);
        let destination = prepared.destination.clone();
        let identity = directory_identity(&destination).unwrap();
        let marker = prepared.owned_marker.clone();
        let marker_bytes = fs::read(&marker).unwrap();
        drop(prepared);
        assert_eq!(directory_identity(&destination).unwrap(), identity);
        assert_eq!(
            fs::read(destination.join("work.txt")).unwrap(),
            b"original input\n"
        );
        assert_eq!(fs::read(marker).unwrap(), marker_bytes);
        assert!(
            PreparedFolderImport::prepare_received_with_parent(
                &source,
                &store,
                &[],
                ProtectedWorkspaceRoot::inspect(&root).unwrap(),
            )
            .is_err(),
            "retention must not authorize an automatic retry"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn received_copy_error_retains_partial_destination_and_markers() {
        let (root, source, store) = received_fixture("received-copy-error");
        let source_file = source.join("work.txt");
        BEFORE_PREPARED_COPY.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                fs::remove_file(source_file).unwrap();
            }));
        });
        assert!(PreparedFolderImport::prepare_received_with_parent(
            &source,
            &store,
            &[],
            ProtectedWorkspaceRoot::inspect(&root).unwrap(),
        )
        .is_err());
        let destination = store.join(crate::workspace::PRESENTED_DIRECTORY_NAME);
        assert!(destination.is_dir());
        let marker = markers(&destination).unwrap().owned;
        verify_owned_marker(&marker, &destination).unwrap();
        assert!(!markers(&destination).unwrap().receipt.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn received_changed_copy_is_preserved_when_confirmation_refuses() {
        let (root, source, store) = received_fixture("received-changed-copy");
        let prepared = prepare_received(&source, &store);
        let destination = prepared.destination.clone();
        let marker = prepared.owned_marker.clone();
        fs::write(destination.join("work.txt"), b"later working edit\n").unwrap();
        assert!(matches!(
            prepared.confirm_into_workspace_without_origin(),
            Err(FolderImportError::VerificationMismatch { .. })
        ));
        assert_eq!(
            fs::read(destination.join("work.txt")).unwrap(),
            b"later working edit\n"
        );
        assert_eq!(
            fs::read(source.join("work.txt")).unwrap(),
            b"original input\n"
        );
        assert!(marker.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn received_post_commit_error_preserves_original_journal_and_work() {
        let (root, source, store) = received_fixture("received-post-commit");
        let prepared = prepare_received(&source, &store);
        let destination = prepared.destination.clone();
        let marker = prepared.owned_marker.clone();
        let source_file = source.join("work.txt");
        BEFORE_WORKSPACE_COMMIT.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                fs::write(source_file, b"source changed during commit\n").unwrap();
            }));
        });
        assert!(matches!(
            prepared.confirm_into_workspace_without_origin(),
            Err(FolderImportError::VerificationMismatch { .. })
        ));
        assert!(
            fs::metadata(store.join(crate::workspace::RECORD_FILE_NAME))
                .unwrap()
                .len()
                > 0
        );
        assert_eq!(
            fs::read(destination.join("work.txt")).unwrap(),
            b"original input\n"
        );
        assert_eq!(
            fs::read(source.join("work.txt")).unwrap(),
            b"source changed during commit\n"
        );
        assert!(marker.exists());
        assert!(!markers(&destination).unwrap().receipt.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn received_generic_rollback_cannot_delete_retained_work() {
        let (root, source, store) = received_fixture("received-rollback-refusal");
        let prepared = prepare_received(&source, &store);
        let destination = prepared.destination.clone();
        assert!(prepared.rollback().is_err());
        assert_eq!(
            fs::read(destination.join("work.txt")).unwrap(),
            b"original input\n"
        );
        assert!(markers(&destination).unwrap().owned.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn replacement_presented_parent_cannot_redirect_workspace_ingest() {
        let root = scratch("presented-parent-ingest-swap");
        let source = root.join("source");
        let versions = root.join("versions");
        let store = versions.join("point.mesh");
        let displaced = root.join("displaced-store");
        let outside = root.join("outside-store");
        fs::create_dir_all(&source).expect("source tree");
        fs::create_dir(&versions).expect("version parent");
        fs::create_dir_all(outside.join(MOUNT_DIRECTORY_NAME)).expect("outside presented tree");
        fs::write(source.join("work.txt"), b"private agent result\n").expect("source file");
        fs::write(
            outside.join(MOUNT_DIRECTORY_NAME).join("work.txt"),
            b"private agent result\n",
        )
        .expect("outside decoy file");
        fs::write(
            outside.join(crate::workspace::PRESENTED_LAYOUT_MARKER_NAME),
            crate::workspace::PRESENTED_LAYOUT_MARKER_BYTES,
        )
        .expect("outside presented marker");

        let prepared =
            PreparedFolderImport::prepare_presented(&source, &store).expect("prepare import");
        let store_for_hook = store.clone();
        let displaced_for_hook = displaced.clone();
        let outside_for_hook = outside.clone();
        BEFORE_WORKSPACE_INGEST.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                fs::rename(&store_for_hook, &displaced_for_hook).expect("displace prepared store");
                symlink(&outside_for_hook, &store_for_hook).expect("redirect prepared namespace");
            }));
        });

        let result = prepared.confirm_into_workspace_without_origin();
        assert!(
            result.is_err(),
            "a parent replaced after verification redirected workspace ingestion"
        );
        assert!(
            !outside.join(crate::workspace::RECORD_FILE_NAME).exists(),
            "workspace journal escaped into the replacement store"
        );
        assert!(
            !outside.join("chunks").exists(),
            "workspace content escaped into the replacement store"
        );

        let _ = fs::remove_file(&store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn replacement_presented_parent_cannot_redirect_workspace_commit() {
        let root = scratch("presented-parent-commit-swap");
        let source = root.join("source");
        let versions = root.join("versions");
        let store = versions.join("point.mesh");
        let displaced = root.join("displaced-store");
        let outside = root.join("outside-store");
        fs::create_dir_all(&source).expect("source tree");
        fs::create_dir(&versions).expect("version parent");
        fs::create_dir_all(outside.join(MOUNT_DIRECTORY_NAME)).expect("outside presented tree");
        fs::write(source.join("work.txt"), b"private agent result\n").expect("source file");
        fs::write(
            outside.join(MOUNT_DIRECTORY_NAME).join("work.txt"),
            b"private agent result\n",
        )
        .expect("outside decoy file");
        fs::write(
            outside.join(crate::workspace::PRESENTED_LAYOUT_MARKER_NAME),
            crate::workspace::PRESENTED_LAYOUT_MARKER_BYTES,
        )
        .expect("outside presented marker");

        let prepared =
            PreparedFolderImport::prepare_presented(&source, &store).expect("prepare import");
        let store_for_hook = store.clone();
        let displaced_for_hook = displaced.clone();
        let outside_for_hook = outside.clone();
        BEFORE_WORKSPACE_COMMIT.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                fs::rename(&store_for_hook, &displaced_for_hook).expect("displace prepared store");
                symlink(&outside_for_hook, &store_for_hook).expect("redirect prepared namespace");
            }));
        });

        let result = prepared.confirm_into_workspace_without_origin();
        assert!(
            result.is_err(),
            "a parent replaced at durable commit was accepted as the displayed workspace"
        );
        assert!(
            !outside.join(crate::workspace::RECORD_FILE_NAME).exists(),
            "workspace journal escaped into the replacement store"
        );
        assert!(
            !outside.join(crate::workspace::DATABASE_FILE_NAME).exists(),
            "workspace index escaped into the replacement store"
        );
        assert!(
            !outside.join("chunks").exists(),
            "workspace content escaped into the replacement store"
        );
        let _ = fs::remove_file(&store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn plain_rollback_and_first_workspace_custody_share_one_physical_directory_serial() {
        let root = scratch("plain-rollback-first-open");
        let source = root.join("source");
        let managed = root.join("managed");
        fs::create_dir_all(&source).expect("source tree");
        fs::write(source.join("work.txt"), b"source bytes\n").expect("source file");
        let confirmed = PreparedFolderImport::prepare(&source, &managed)
            .expect("prepare plain copy")
            .confirm()
            .expect("confirm plain copy");
        assert!(!managed
            .join(crate::workspace::STORAGE_DIRECTORY_NAME)
            .exists());

        let (locked_tx, locked_rx) = mpsc::channel();
        let (continue_tx, continue_rx) = mpsc::channel();
        let rollback = std::thread::spawn(move || {
            confirmed.rollback_with_custody_hook(|| {
                locked_tx.send(()).unwrap();
                continue_rx.recv().unwrap();
            })
        });
        locked_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("rollback owns physical directory serial");
        let opening_path = managed.clone();
        let (opened_tx, opened_rx) = mpsc::channel();
        let opening = std::thread::spawn(move || {
            let result = OpenWorkspace::open(&opening_path).map(|open| open.installation());
            opened_tx.send(result).unwrap();
        });
        assert!(
            opened_rx.recv_timeout(Duration::from_millis(30)).is_err(),
            "first initialization must wait before creating private state"
        );
        assert!(
            !managed
                .join(crate::workspace::STORAGE_DIRECTORY_NAME)
                .exists(),
            "a waiting first open created private state before winning the physical serial"
        );
        continue_tx.send(()).unwrap();
        rollback
            .join()
            .unwrap()
            .expect("plain rollback removes exact unchanged copy");
        assert!(
            opened_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("first open returns after rollback")
                .is_err(),
            "a waiting first open must not initialize the deleted pinned directory"
        );
        opening.join().unwrap();
        assert!(!managed.exists());

        let managed = root.join("managed-custody-first");
        let (confirmed, _) = PreparedFolderImport::prepare(&source, &managed)
            .expect("prepare second plain copy")
            .confirm_into_workspace()
            .expect("confirm initialized workspace");
        let open = OpenWorkspace::open(&managed).expect("initialize workspace first");
        let installation = open.installation();
        let locked = crate::workspace_custody::lock_for_workspace_path(&managed, &installation)
            .expect("lock initialized workspace");
        let generation = locked.acquire(false, None).expect("assign agent custody");
        let (rollback_tx, rollback_rx) = mpsc::channel();
        let refusing = std::thread::spawn(move || {
            rollback_tx.send(confirmed.rollback()).unwrap();
        });
        assert!(
            rollback_rx.recv_timeout(Duration::from_millis(30)).is_err(),
            "rollback must wait behind an in-flight custody transaction"
        );
        drop(locked);
        assert!(matches!(
            rollback_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("rollback returns after custody transaction"),
            Err(FolderImportError::CustodyRefused(_))
        ));
        refusing.join().unwrap();
        assert!(managed.exists());
        crate::workspace_custody::lock_for_workspace_path(&managed, &installation)
            .unwrap()
            .release(&generation)
            .expect("release exact agent generation");
        drop(open);
        ConfirmedFolderImport::open(&managed)
            .expect("reopen exact receipt")
            .rollback()
            .expect("rollback resumes after release");
        assert!(!managed.exists());
        fs::remove_dir_all(root).expect("cleanup");
    }
}
