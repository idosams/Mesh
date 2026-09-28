//! Opening a real workspace on disk, and what the daemon can honestly say about it.
//!
//! # This is the daemon's one durable resource
//!
//! A workspace is a directory. Inside it, one file — [`RECORD_FILE_NAME`] — holds every
//! immutable record ever appended, framed by `mesh_store::frame_record`. That file is the whole
//! of the durable state: [`RecordFile`] appends whole frames and forces each one to disk before
//! returning, which is [`mesh_store::RecordJournal`]'s stated contract and the reason
//! `mesh_store::scan_journal` can decide where durability stops.
//!
//! # Payloads and names
//!
//! Operation records carry content digests, not ChangeSet bytes. Opening a workspace therefore
//! opens its `mesh-cas` store, verifies and decodes every available ChangeSet, and gives its exact
//! `mesh-operations::Operation` sequence to `mesh-materializer`. Durable names come only from that
//! fold. A separate, explicitly nonauthoritative discovery answer may enumerate the native folder
//! so a person can inspect and adopt agent-created files; it never changes materialized state by
//! itself. Missing or unreadable payloads remain recoverable conditions and keep the answer
//! visibly partial rather than turning it into an empty folder.
//!
//! # The database is a disposable durable index
//!
//! Opening creates `metadata.sqlite`, applies the forward migrations through
//! [`mesh_store::Store`], and rebuilds every record-derived table from `records.mesh` before the
//! workspace is served. A missing or corrupt database is deleted with its WAL sidecars and rebuilt;
//! the immutable record file is never repaired from the index. The digest and row counts below are
//! therefore facts about both the fold and the database transaction that accepted it.
//!
//! # Why the daemon owns the workspace lifetime
//!
//! `mesh-store` owns the schema, migrations, fold, digest and SQLite driver. The daemon owns the
//! workspace path and connection lifetime, so it selects the database file and decides when a
//! disposable index must be replaced from the immutable journal. No storage meaning is
//! re-implemented here.

#[cfg(test)]
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read as _, Seek as _, Write as _};
use std::path::{Path, PathBuf};
use std::time::Instant;

use mesh_cas::{Cas, Digest32 as CasDigest, DurableFs as _};
use mesh_operations::{
    ActorId, ActorSequence, CanonicalValue, CausalParents, ChangeSetId, HeadId, Hlc,
    NormalizedName, ObjectId, Operation, PolicyEpoch, PortableMetadata, SessionId, VersionId,
    WorkspaceId,
};
use mesh_store::{
    journal_records, scan_journal, ApprovalRecord, Checkpoint, DatabasePath, DurableBoundary,
    Index, JournalDamage, OperationRecord, RecordDigest, RecordJournal, RecordKind, ReviewRecord,
    ReviewVerdict, Sqlite, Store, StoreError, StoredRecord, TailResidue, WorkspaceRoot, TABLES,
};
use mesh_types::{Blake3, ContentDigest as _, DigestHasher as _, PublicKey};

use crate::exclusions::{EffectiveExclusions, ExclusionLoadFailure};
use crate::publication::{self, SharedVersion};
use crate::recovery::{RecoveryDiagnostic, RecoveryOutcome, RECOVERY_BUDGET};
use crate::root_authority::PinnedWorkspaceRoot;
use crate::user_messages;
use crate::version_state::{self, PrivateVersion};

/// The one durable file inside a workspace directory.
///
/// Named for what it holds rather than for how it is written: a person who finds this file wants
/// to know it is their saved work, and `mesh-store`'s framing is an implementation detail of the
/// bytes inside it.
pub const RECORD_FILE_NAME: &str = "records.mesh";

/// The single private namespace inside a user-facing workspace folder.
///
/// New workspaces keep every journal, database and content-store artifact here so the workspace
/// root remains an ordinary project folder. Existing root-layout workspaces remain readable; no
/// multi-file migration is guessed during open.
pub const STORAGE_DIRECTORY_NAME: &str = ".mesh";

const PRIVATE_MANAGED_TOP_LEVEL: &[&str] = &[
    STORAGE_DIRECTORY_NAME,
    "records.mesh",
    "metadata.sqlite",
    "metadata.sqlite-wal",
    "metadata.sqlite-shm",
    ".mesh-recovery.sqlite",
    ".mesh-recovery.sqlite-wal",
    ".mesh-recovery.sqlite-shm",
    "chunks",
    "scratch",
    "incoming",
    "quarantine",
    "logs",
    crate::pull_back_receipt::RECEIPT_DIRECTORY_NAME,
    "mount",
    crate::workspace_custody::LOCK_FILE,
    crate::workspace_custody::RECORD_FILE,
    crate::workspace_custody::TEMP_FILE,
];

/// Canonical marker for the external-store/presented-folder layout.
///
/// The store directory owns this marker and its human-named child is the ordinary folder handed
/// to editors and agents. Reopening that child consults only this exact regular file; no symlink
/// or caller-supplied pointer can redirect storage authority.
pub const PRESENTED_LAYOUT_MARKER_NAME: &str = ".mesh-presented-workspace";
pub(crate) const PRESENTED_LAYOUT_MARKER_BYTES: &[u8] = b"mesh.presented-workspace/1\n";

/// Filesystem-visible name of a managed historical working folder.
///
/// This is intentionally product language rather than a storage term: it remains visible when
/// Finder, an editor, a terminal, or a system file picker opens the folder outside Mesh.
pub const PRESENTED_DIRECTORY_NAME: &str = "Mesh Version - Working Folder";

/// Whether one exact leaf name is a current or legacy externally presented workspace.
#[must_use]
pub fn is_presented_directory_name(name: &std::ffi::OsStr) -> bool {
    name == std::ffi::OsStr::new(PRESENTED_DIRECTORY_NAME)
        || name == std::ffi::OsStr::new(mesh_store::MOUNT_DIRECTORY_NAME)
}

/// Resolve the one presented child below a private store without following links.
///
/// New stores use [`PRESENTED_DIRECTORY_NAME`]. Existing alpha stores named `mounts` remain
/// readable, while two competing children are refused rather than guessed between.
///
/// # Errors
///
/// Returns an I/O error when either candidate cannot be inspected or both names exist.
pub fn presented_workspace_path(storage_root: &Path) -> io::Result<PathBuf> {
    let current = storage_root.join(PRESENTED_DIRECTORY_NAME);
    let legacy = storage_root.join(mesh_store::MOUNT_DIRECTORY_NAME);
    let exists = |path: &Path| match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    };
    let current_exists = exists(&current)?;
    let legacy_exists = exists(&legacy)?;
    match (current_exists, legacy_exists) {
        (true, true) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the private store contains two competing presented folders",
        )),
        (true, false) => Ok(current),
        (false, true) => Ok(legacy),
        (false, false) => Ok(current),
    }
}

#[cfg(test)]
thread_local! {
    static HISTORICAL_RECONSTRUCTED_BYTES: Cell<u64> = const { Cell::new(0) };
    static HISTORICAL_PATH_RESOLUTIONS: Cell<u64> = const { Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_historical_reconstructed_bytes() {
    HISTORICAL_RECONSTRUCTED_BYTES.set(0);
}

#[cfg(test)]
pub(crate) fn historical_reconstructed_bytes() -> u64 {
    HISTORICAL_RECONSTRUCTED_BYTES.get()
}

#[cfg(test)]
pub(crate) fn reset_historical_path_resolutions() {
    HISTORICAL_PATH_RESOLUTIONS.set(0);
}

#[cfg(test)]
pub(crate) fn historical_path_resolutions() -> u64 {
    HISTORICAL_PATH_RESOLUTIONS.get()
}

#[cfg(test)]
fn record_historical_path_resolution() {
    HISTORICAL_PATH_RESOLUTIONS.set(HISTORICAL_PATH_RESOLUTIONS.get().saturating_add(1));
}

#[cfg(not(test))]
fn record_historical_path_resolution() {}

/// The durable metadata index created beside [`RECORD_FILE_NAME`].
pub const DATABASE_FILE_NAME: &str = mesh_store::DATABASE_FILE_NAME;

/// Resolve private storage without following a linked namespace or choosing between two journals.
pub(crate) fn presented_workspace_storage_root(root: &Path) -> io::Result<Option<PathBuf>> {
    if root.file_name().is_some_and(is_presented_directory_name) {
        let parent = root.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "the presented workspace has no private-store parent",
            )
        })?;
        let marker = parent.join(PRESENTED_LAYOUT_MARKER_NAME);
        match fs::symlink_metadata(&marker) {
            Ok(metadata) => {
                if !metadata.is_file()
                    || metadata.file_type().is_symlink()
                    || fs::read(&marker)? != PRESENTED_LAYOUT_MARKER_BYTES
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "the presented workspace marker is not canonical",
                    ));
                }
                return Ok(Some(parent.to_path_buf()));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

/// Resolve private storage without following a linked namespace or choosing between two journals.
pub(crate) fn workspace_storage_root(root: &Path) -> io::Result<PathBuf> {
    if let Some(storage_root) = presented_workspace_storage_root(root)? {
        return Ok(storage_root);
    }
    let legacy_exists = fs::symlink_metadata(root.join(RECORD_FILE_NAME)).is_ok();
    let private = root.join(STORAGE_DIRECTORY_NAME);
    let private_exists = match fs::symlink_metadata(&private) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => true,
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the private Mesh workspace namespace is not a real directory",
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(error),
    };
    let namespaced_record_exists =
        private_exists && fs::symlink_metadata(private.join(RECORD_FILE_NAME)).is_ok();
    if private_exists && !namespaced_record_exists {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "an existing .mesh directory is not an initialized Mesh private namespace",
        ));
    }
    if legacy_exists && namespaced_record_exists {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "both legacy and namespaced Mesh journals exist; refusing to guess which is current",
        ));
    }
    Ok(if legacy_exists {
        root.to_path_buf()
    } else {
        private
    })
}

/// Exact journal path for read-only support and crash inspection.
pub(crate) fn workspace_record_file(root: &Path) -> io::Result<PathBuf> {
    workspace_storage_root(root).map(|storage| storage.join(RECORD_FILE_NAME))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WorkspaceDirectoryIdentity {
    device: u64,
    inode: u64,
}

fn workspace_directory_identity(path: &Path) -> io::Result<WorkspaceDirectoryIdentity> {
    use std::os::unix::fs::MetadataExt as _;

    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(io::Error::other(
            "the pinned workspace root is no longer the opened directory",
        ));
    }
    Ok(WorkspaceDirectoryIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

pub(crate) fn workspace_installation(physical: (u64, u64), storage: (u64, u64)) -> String {
    let mut bytes = b"mesh.workspace.installation/1\0".to_vec();
    bytes.extend_from_slice(&physical.0.to_be_bytes());
    bytes.extend_from_slice(&physical.1.to_be_bytes());
    bytes.extend_from_slice(&storage.0.to_be_bytes());
    bytes.extend_from_slice(&storage.1.to_be_bytes());
    format!("blake3:{}", Blake3::digest_bytes(&bytes).to_hex())
}

/// One materialized path in an open workspace.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct WorkspaceEntry {
    path: String,
    entry_type: &'static str,
}

/// One native path Mesh deliberately cannot admit as a managed file or folder.
///
/// The path is discovery-only. It never grants read, traversal, or mutation authority; it exists
/// so a client cannot mistake an omitted symbolic link or special filesystem object for a fully
/// reviewed workspace. It also names a path whose exclusion rules claim it is versioned beneath
/// an excluded parent, because the durable directory model cannot preserve that path without
/// silently inventing the excluded ancestor.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NativeUnsupportedEntry {
    path: String,
    kind: &'static str,
}

impl NativeUnsupportedEntry {
    /// Root-relative native path, using `/` between names.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Closed presentation kind: `symbolic-link`, `special`, or `excluded-ancestor`.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        self.kind
    }
}

#[derive(Debug, Default)]
pub(crate) struct NativeDiscovery {
    pub files: Vec<String>,
    pub directories: Vec<String>,
    pub unsupported: Vec<NativeUnsupportedEntry>,
    pub complete: bool,
}

impl WorkspaceEntry {
    /// The root-relative path, with `/` between normalized names.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// `file` or `folder`.
    #[must_use]
    pub const fn entry_type(&self) -> &'static str {
        self.entry_type
    }
}

/// A recoverable reason the names-and-folders answer is partial or unavailable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceCondition {
    code: &'static str,
    message: &'static str,
    related: Vec<String>,
}

/// One immutable file version named by a restore preview.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RestoreVersionIdentity {
    version: mesh_materializer::VersionId,
    manifest: mesh_materializer::ManifestId,
}

/// Retained immutable versions for one currently materialized file.
///
/// This is discovery metadata only. It lets a read-only client choose the exact identities that
/// [`OpenWorkspace::preview_file_restore`] already validates; it carries no operation, signature,
/// policy decision, or append capability.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceFileHistory {
    path: String,
    object: mesh_materializer::ObjectId,
    current: Option<RestoreVersionIdentity>,
    retained: Vec<RestoreVersionIdentity>,
}

/// One durable path that existed earlier in this workspace but is absent from its current tree.
///
/// This is discovery metadata for a reviewed export cleanup. It is not deletion authority: a
/// caller must reconstruct and compare the retained file bytes (when this is a file), inspect the
/// exact ordinary-folder entry, and revalidate both facts immediately before removing anything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetiredWorkspaceEntry {
    object: mesh_materializer::ObjectId,
    path: String,
    entry_type: &'static str,
    last_file_version: Option<RestoreVersionIdentity>,
    last_executable: Option<bool>,
    imported_with_workspace: bool,
}

impl RetiredWorkspaceEntry {
    /// Stable saved object whose final visible binding occupied this retired path.
    #[must_use]
    pub(crate) const fn object(&self) -> mesh_materializer::ObjectId {
        self.object
    }

    /// The root-relative path that is no longer part of the current saved tree.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// `file` or `folder`.
    #[must_use]
    pub const fn entry_type(&self) -> &'static str {
        self.entry_type
    }

    /// The last immutable file version visible at this path, when the entry was a file.
    #[must_use]
    pub const fn last_file_version(&self) -> Option<RestoreVersionIdentity> {
        self.last_file_version
    }

    /// Portable executable state of the last file version, when the entry was a file.
    #[must_use]
    pub const fn last_executable(&self) -> Option<bool> {
        self.last_executable
    }

    /// Whether this path's retired object was part of the immutable genesis import.
    ///
    /// This is ancestry, not deletion authority. The export boundary must also prove that the
    /// selected ordinary directory is the exact source of that genesis import. Matching bytes or
    /// this flag alone cannot authorize removing an independently created ordinary entry. A path
    /// outside the genesis import needs a separate exact Pull-back installation receipt.
    #[must_use]
    pub(crate) const fn imported_with_workspace(&self) -> bool {
        self.imported_with_workspace
    }
}

/// One durable whole-workspace point that can be opened as an independent native working folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkspaceVersion {
    operation: RecordDigest,
    ordinal: u64,
    actor_sequence: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HistoricalWorkspaceFile {
    pub object: mesh_materializer::ObjectId,
    pub path: String,
    pub bytes: Vec<u8>,
    pub executable: bool,
}

/// Exact saved bytes for one side of a review change, re-derived on demand.
///
/// This value is deliberately not part of `workspace.state`: office documents can be large and
/// the visual renderer is an optional local convenience. The desktop asks for one side only after
/// a person opens its visual preview, while the daemon revalidates the bundle, object, path,
/// version and content digest against immutable journal/CAS truth.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VerifiedReviewArtifact {
    pub path: String,
    pub version: mesh_approval::VersionId,
    pub digest: mesh_approval::Digest32,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReviewArtifactSide {
    Before,
    After,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HistoricalWorkspaceDirectory {
    pub object: mesh_materializer::ObjectId,
    pub path: String,
}

#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HistoricalWorkspaceSnapshot {
    pub operation: RecordDigest,
    pub directories: Vec<HistoricalWorkspaceDirectory>,
    pub files: Vec<HistoricalWorkspaceFile>,
}

/// Content-verified metadata for one historical file, without retaining its reconstructed bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HistoricalWorkspacePreviewFile {
    pub object: mesh_materializer::ObjectId,
    pub path: String,
    pub manifest_id: RecordDigest,
    pub byte_length: u64,
    pub content_digest: RecordDigest,
    pub executable: bool,
}

/// A bounded-memory view of one durable workspace point for the version picker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HistoricalWorkspacePreview {
    pub operation: RecordDigest,
    pub directories: Vec<HistoricalWorkspaceDirectory>,
    pub files: Vec<HistoricalWorkspacePreviewFile>,
}

pub(crate) enum HistoricalWorkspaceWriteFailure {
    Retained(WorkspaceVersionFailure),
    Output(io::Error),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkspaceVersionChangeBasis {
    Initial,
    Previous(RecordDigest),
    CombinedHistory,
}

/// Why a durable whole-workspace point cannot be opened as a native fork.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceVersionFailure {
    /// The requested operation identifier is absent from durable history.
    UnknownOperation,
    /// Some operation payload or materialization fact needed by the point is unavailable.
    IncompleteHistory(Vec<&'static str>),
    /// A file version names a manifest record the journal does not contain.
    MissingManifest(String),
    /// A retained chunk or reconstructed file failed exact verification.
    RetainedContent(String),
}

impl core::fmt::Display for WorkspaceVersionFailure {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnknownOperation => write!(
                formatter,
                "the selected workspace version is not in durable history"
            ),
            Self::IncompleteHistory(codes) => write!(
                formatter,
                "the selected workspace version is incomplete: {}",
                codes.join(", ")
            ),
            Self::MissingManifest(id) => write!(
                formatter,
                "the selected workspace version is missing manifest {id}"
            ),
            Self::RetainedContent(reason) => write!(
                formatter,
                "the selected workspace version could not be reconstructed exactly: {reason}"
            ),
        }
    }
}

impl WorkspaceVersion {
    /// Durable operation at this point.
    #[must_use]
    pub const fn operation(self) -> RecordDigest {
        self.operation
    }

    /// One-based position in deterministic causal order.
    #[must_use]
    pub const fn ordinal(self) -> u64 {
        self.ordinal
    }

    /// Author-local sequence carried by the operation.
    #[must_use]
    pub const fn actor_sequence(self) -> u64 {
        self.actor_sequence
    }
}

/// Exact durable context for the next authenticated local file-version append.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ManagedCheckpointBasis {
    pub workspace_id: WorkspaceId,
    pub actor_id: ActorId,
    pub session_id: SessionId,
    pub actor_sequence: ActorSequence,
    pub causal_parents: CausalParents,
    pub base_head: HeadId,
    pub policy_epoch: PolicyEpoch,
    pub hybrid_logical_time: Hlc,
    pub object_id: ObjectId,
    pub parent_version: VersionId,
    pub parent_metadata: PortableMetadata,
}

/// Durable authoring context shared by every authenticated native workspace mutation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ManagedAuthoringBasis {
    pub workspace_id: WorkspaceId,
    pub actor_id: ActorId,
    pub session_id: SessionId,
    pub actor_sequence: ActorSequence,
    pub causal_parents: CausalParents,
    pub base_head: HeadId,
    pub policy_epoch: PolicyEpoch,
    pub hybrid_logical_time: Hlc,
}

/// A checked operation proposal against one exact saved history point.
/// This value is neither a signed ChangeSet nor permission to append, approve or apply files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoricalOperationPlan {
    target: RecordDigest,
    basis: ManagedAuthoringBasis,
    operations: Vec<Operation>,
}
impl HistoricalOperationPlan {
    /// Exact saved predecessor. Later journal branches are not part of this proposal.
    #[must_use]
    pub const fn target(&self) -> RecordDigest {
        self.target
    }

    /// Native operation vocabulary checked against the selected historical materialization.
    #[must_use]
    pub fn operations(&self) -> &[Operation] {
        &self.operations
    }

    /// Bounded context metadata only; no saved file bytes or signing authority.
    #[must_use]
    pub fn context(&self) -> crate::ipc::Json {
        use crate::ipc::Json;
        Json::object([
            ("schema", Json::text("mesh.historical-operation-plan/v1")),
            ("target", Json::text(self.target.to_string())),
            ("workspace", Json::text(self.basis.workspace_id.to_string())),
            ("actor", Json::text(self.basis.actor_id.to_string())),
            ("session", Json::text(self.basis.session_id.to_string())),
            (
                "actor_sequence",
                Json::text(self.basis.actor_sequence.value().to_string()),
            ),
            ("base_head", Json::text(self.basis.base_head.to_string())),
            (
                "policy_epoch",
                Json::text(self.basis.policy_epoch.value().to_string()),
            ),
            (
                "clock_millis",
                Json::text(self.basis.hybrid_logical_time.physical_millis().to_string()),
            ),
            (
                "clock_counter",
                Json::Number(u64::from(self.basis.hybrid_logical_time.logical())),
            ),
            ("operations", Json::Number(self.operations.len() as u64)),
            ("approval_authority", Json::Bool(false)),
        ])
    }
}

pub(crate) struct HistoricalCaptureEntry {
    pub binding: ManagedEntryBasis,
    pub file: Option<(VersionId, RecordDigest, bool)>,
}

/// Exact existing entry identity and binding resolved from complete durable materialization.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ManagedEntryBasis {
    pub object_id: ObjectId,
    pub parent_id: ObjectId,
    pub name: NormalizedName,
    pub is_directory: bool,
}

/// Exact free target binding for a new or moved managed entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ManagedTargetBasis {
    pub parent_id: ObjectId,
    pub name: NormalizedName,
}

impl WorkspaceFileHistory {
    /// Current root-relative path of the file.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Stable file identity used by the restore planner.
    #[must_use]
    pub const fn object(&self) -> mesh_materializer::ObjectId {
        self.object
    }

    /// Version currently visible at this path, when the file has content.
    #[must_use]
    pub const fn current(&self) -> Option<RestoreVersionIdentity> {
        self.current
    }

    /// Every retained immutable version of this file, in stable identifier order.
    #[must_use]
    pub fn retained(&self) -> &[RestoreVersionIdentity] {
        &self.retained
    }
}

impl RestoreVersionIdentity {
    /// The immutable object-version identifier.
    #[must_use]
    pub const fn version(&self) -> mesh_materializer::VersionId {
        self.version
    }

    /// The content-addressed manifest that version names.
    #[must_use]
    pub const fn manifest(&self) -> mesh_materializer::ManifestId {
        self.manifest
    }
}

/// A read-only, checked projection of restoring one file to an earlier version.
///
/// This value carries ordinary operations but no ChangeSet identity, signature, policy decision,
/// or append capability. Constructing it cannot authorize or execute the restore.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileRestorePreview {
    object: mesh_materializer::ObjectId,
    current: RestoreVersionIdentity,
    target: RestoreVersionIdentity,
    operations: Vec<mesh_materializer::Operation>,
    undo_possible: bool,
}

impl FileRestorePreview {
    /// Stable object whose visible version would change.
    #[must_use]
    pub const fn object(&self) -> mesh_materializer::ObjectId {
        self.object
    }

    /// Version and manifest visible at the durable boundary being previewed.
    #[must_use]
    pub const fn current(&self) -> RestoreVersionIdentity {
        self.current
    }

    /// Retained version and manifest the plan would make visible.
    #[must_use]
    pub const fn target(&self) -> RestoreVersionIdentity {
        self.target
    }

    /// Ordinary owner-vocabulary operations, in their required order.
    #[must_use]
    pub fn operations(&self) -> &[mesh_materializer::Operation] {
        &self.operations
    }

    /// Whether applying this exact operation sequence would leave a checked inverse plan.
    #[must_use]
    pub const fn undo_possible(&self) -> bool {
        self.undo_possible
    }

    /// Stable user-facing JSON projection used by the local `meshctl` command.
    #[must_use]
    pub fn to_json(&self) -> crate::ipc::Json {
        crate::ipc::Json::object([
            ("schema", crate::ipc::Json::text("mesh.restore-preview/v1")),
            ("canonical_state_read_only", crate::ipc::Json::Bool(true)),
            ("object_id", crate::ipc::Json::text(self.object.to_string())),
            ("current", version_json(self.current)),
            ("target", version_json(self.target)),
            (
                "operations",
                crate::ipc::Json::Array(self.operations.iter().map(operation_json).collect()),
            ),
            ("undo_possible", crate::ipc::Json::Bool(self.undo_possible)),
            ("execution_authorized", crate::ipc::Json::Bool(false)),
        ])
    }
}

/// Why a workspace cannot provide an exact restore preview.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RestorePreviewFailure {
    /// Some saved operation payload was unavailable or invalid, so the visible state is partial.
    IncompleteProjection {
        /// Stable condition codes explaining what could not be folded.
        conditions: Vec<&'static str>,
    },
    /// The materializer's exact restore planner refused the request.
    Refused(mesh_materializer::RestoreRefusal),
    /// The already-checked plan could not be replayed for the undo projection.
    InconsistentPlan {
        /// The operation index that unexpectedly failed.
        index: usize,
        /// The materializer refusal.
        rejection: mesh_materializer::Rejection,
    },
}

impl RestorePreviewFailure {
    /// Stable machine code for a user interface or command-line caller.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::IncompleteProjection { .. } => "restore-preview-incomplete",
            Self::Refused(_) => "restore-preview-refused",
            Self::InconsistentPlan { .. } => "restore-preview-inconsistent",
        }
    }

    /// Stable refusal shape for a user-facing local command.
    #[must_use]
    pub fn to_json(&self) -> crate::ipc::Json {
        crate::ipc::Json::object([
            ("schema", crate::ipc::Json::text("mesh.restore-preview/v1")),
            ("canonical_state_read_only", crate::ipc::Json::Bool(true)),
            ("refused", crate::ipc::Json::Bool(true)),
            ("code", crate::ipc::Json::text(self.code())),
            ("message", crate::ipc::Json::text(self.to_string())),
            ("execution_authorized", crate::ipc::Json::Bool(false)),
        ])
    }
}

impl core::fmt::Display for RestorePreviewFailure {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::IncompleteProjection { conditions } => write!(
                formatter,
                "restore preview refused: the recovered workspace projection is incomplete ({})",
                conditions.join(", ")
            ),
            Self::Refused(refusal) => refusal.fmt(formatter),
            Self::InconsistentPlan { index, rejection } => write!(
                formatter,
                "restore preview refused: checked operation {index} no longer applies: {rejection}"
            ),
        }
    }
}

impl std::error::Error for RestorePreviewFailure {}

impl WorkspaceCondition {
    /// The sanitized condition used when restart settlement cannot safely finish.
    pub(crate) fn checkpoint_recovery_needs_attention() -> Self {
        condition(
            "checkpoint-recovery-needs-attention",
            crate::user_messages::CHECKPOINT_RECOVERY_NEEDS_ATTENTION,
            Vec::new(),
        )
    }

    pub(crate) fn exclusion_rules_unavailable() -> Self {
        condition(
            "exclusion-rules-unavailable",
            crate::user_messages::EXCLUSION_RULES_UNAVAILABLE,
            Vec::new(),
        )
    }

    pub(crate) fn unversioned_native_content() -> Self {
        condition(
            "unversioned-native-content",
            crate::user_messages::UNVERSIONED_NATIVE_CONTENT,
            Vec::new(),
        )
    }

    /// Stable machine code; callers branch on this rather than prose.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }

    /// The user-facing recovery sentence.
    #[must_use]
    pub const fn message(&self) -> &'static str {
        self.message
    }

    /// Stable identifiers involved, such as a saved change, payload, or root candidate.
    #[must_use]
    pub fn related(&self) -> &[String] {
        &self.related
    }

    /// These conditions preserve the open workspace and its partial answer.
    #[must_use]
    pub const fn recoverable(&self) -> bool {
        true
    }
}

/// An append-only file of framed records, forced to disk on every append.
///
/// The durability is the contract, not a nicety: `mesh_store::scan_journal` reports the last
/// boundary it can *see*, so an implementation that buffers moves the real boundary somewhere the
/// scan cannot reach and turns "saved privately" into a guess. [`Self::append`] calls
/// `File::sync_all` before it returns, every time.
#[derive(Debug)]
pub struct RecordFile {
    path: PathBuf,
    file: File,
}

impl RecordFile {
    /// Open the record file at `path`, creating an empty one when there is none.
    ///
    /// # Errors
    ///
    /// Any I/O error from creating the parent directory or opening the file.
    pub fn open(path: &Path) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            file,
        })
    }

    fn open_pinned(root: &PinnedWorkspaceRoot, relative: &Path, path: PathBuf) -> io::Result<Self> {
        let file = root.open_record_file(relative)?;
        Ok(Self { path, file })
    }

    fn open_existing_pinned(
        root: &PinnedWorkspaceRoot,
        relative: &Path,
        path: PathBuf,
    ) -> io::Result<Self> {
        let file = root.open_existing_record_file(relative)?;
        Ok(Self { path, file })
    }

    /// Where this file is.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// How many bytes it holds.
    ///
    /// # Errors
    ///
    /// Any I/O error from reading the file's metadata.
    pub fn byte_len(&self) -> io::Result<u64> {
        Ok(self.file.metadata()?.len())
    }

    #[cfg(unix)]
    fn support_identity(&self) -> io::Result<Option<(u64, u64)>> {
        use std::os::unix::fs::MetadataExt as _;

        let metadata = self.file.metadata()?;
        Ok(Some((metadata.dev(), metadata.ino())))
    }

    #[cfg(not(unix))]
    fn support_identity(&self) -> io::Result<Option<(u64, u64)>> {
        Ok(None)
    }
}

impl RecordJournal for RecordFile {
    type Error = io::Error;

    fn read_all(&mut self) -> Result<Vec<u8>, Self::Error> {
        let mut bytes = Vec::new();
        // A separate handle, so the append cursor this struct holds is never disturbed by a read.
        let mut reader = self.file.try_clone()?;
        reader.seek(io::SeekFrom::Start(0))?;
        reader.read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    fn append(&mut self, framed: &[u8]) -> Result<(), Self::Error> {
        self.file.write_all(framed)?;
        self.file.sync_all()
    }
}

/// Why a workspace could not be opened.
#[derive(Debug)]
pub enum OpenFailure {
    /// The directory or the record file could not be reached.
    Unreachable(io::Error),
    /// The workspace content-addressed store could not be opened.
    PayloadStore(mesh_cas::CasError),
    /// The disposable metadata index could neither be opened nor rebuilt from records.
    Index {
        /// The database failure after a clean rebuild was attempted.
        detail: String,
    },
    /// A whole record in the file is wrong — a checksum that does not match, a frame this build
    /// does not know. Distinct from an interrupted append, which is not damage.
    Damaged(JournalDamage),
    /// There are bytes in the record file and not one whole record among them, so there is no
    /// durable boundary to open at.
    ///
    /// A refusal rather than an empty workspace, and that is the whole point of the variant: a
    /// folder holding an unfinished save is not a new folder, and serving it as one reports a
    /// crash as a clean start. Nothing is removed and nothing is truncated — the bytes stay where
    /// they are for whoever looks next.
    NothingReadable {
        /// How many bytes lie in the file with no whole record among them.
        unfinished_bytes: u64,
    },
    /// The records contradict each other, so no index can be folded from them.
    Contradictory {
        /// What the fold said.
        detail: String,
        /// How far the file read back as whole records before the fold refused them. Carried so a
        /// report can still say where durability stopped when nothing could be folded from it.
        readable: DurableBoundary,
    },
}

impl core::fmt::Display for OpenFailure {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unreachable(error) => write!(formatter, "{error}"),
            Self::PayloadStore(error) => write!(formatter, "{error}"),
            Self::Index { detail } => write!(formatter, "{detail}"),
            Self::Damaged(damage) => write!(formatter, "{damage}"),
            Self::NothingReadable { unfinished_bytes } => write!(
                formatter,
                "{unfinished_bytes} bytes of an unfinished save, and no whole record before them, \
                 so there is no last durable boundary to open at"
            ),
            Self::Contradictory { detail, readable } => {
                write!(formatter, "{detail} (whole for {readable})")
            }
        }
    }
}

/// A stable machine code for each failure, so a client can branch without reading prose.
impl OpenFailure {
    /// The code a client sees.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Unreachable(_) => "workspace-unreachable",
            Self::PayloadStore(_) => "workspace-payload-store-unreachable",
            Self::Index { .. } => "workspace-index-unavailable",
            Self::Damaged(_) => "workspace-damaged",
            Self::NothingReadable { .. } => "workspace-nothing-readable",
            Self::Contradictory { .. } => "workspace-contradictory",
        }
    }

    /// How far the record file read back as whole, verified records before this failure.
    ///
    /// The last durable boundary is a fact about the bytes and it survives every one of these:
    /// damage names the prefix it stopped at, a contradiction carries the boundary the scan
    /// reached, and the two that found nothing whole report nothing rather than a guess.
    #[must_use]
    pub const fn readable_boundary(&self) -> DurableBoundary {
        match self {
            Self::Unreachable(_)
            | Self::PayloadStore(_)
            | Self::Index { .. }
            | Self::NothingReadable { .. } => DurableBoundary {
                records: 0,
                byte_offset: 0,
            },
            Self::Damaged(damage) => damage.intact_prefix(),
            Self::Contradictory { readable, .. } => *readable,
        }
    }
}

/// One workspace, open, with its index folded from the records on disk.
#[derive(Debug)]
pub struct OpenWorkspace {
    /// The spelling selected by the caller, retained for truthful user-facing summaries.
    root: WorkspaceRoot,
    /// The directory target resolved once when this workspace was opened.
    ///
    /// Every durable or filesystem operation uses this pinned path. Otherwise a symlink selected
    /// by the caller can be retargeted after authoring context is derived and redirect the file,
    /// CAS and reopen steps away from the already-open journal and SQLite handles.
    physical_root: WorkspaceRoot,
    /// Private journal, database and content-store namespace for this workspace generation.
    storage_root: WorkspaceRoot,
    /// Descriptor authority for the private namespace.
    ///
    /// This differs from `pinned_root` for a presented workspace, where user content is under the
    /// store's presented child and private state is its sibling. Keeping both descriptors is the
    /// structural separation: neither side reaches the other with a relative path.
    storage_pinned_root: PinnedWorkspaceRoot,
    /// Stable identity of the private namespace selected at open.
    storage_identity: WorkspaceDirectoryIdentity,
    physical_identity: WorkspaceDirectoryIdentity,
    pinned_root: PinnedWorkspaceRoot,
    record_file: PathBuf,
    journal: RecordFile,
    database_file: PathBuf,
    store: Store<Sqlite>,
    payload_store: Cas<crate::root_authority::PinnedRootFs, mesh_cas::Blake3>,
    record_index: Index,
    boundary: DurableBoundary,
    tail: TailResidue,
    diagnostic: RecoveryDiagnostic,
    records_by_kind: Vec<(RecordKind, usize)>,
    rows_per_table: Vec<(&'static str, usize)>,
    operations: usize,
    actors: usize,
    manifests: usize,
    peers: usize,
    reviews: usize,
    digest: mesh_store::Digest16,
    private_version: PrivateVersion,
    shared_version: SharedVersion,
    shared_history: Vec<mesh_approval::HeadId>,
    review_base_hints: BTreeMap<RecordDigest, mesh_approval::HeadId>,
    materialized_state: Option<mesh_materializer::WorkspaceState>,
    entries: Vec<WorkspaceEntry>,
    file_histories: Vec<WorkspaceFileHistory>,
    retired_entries: Vec<RetiredWorkspaceEntry>,
    conditions: Vec<WorkspaceCondition>,
    names_answered: bool,
}

#[derive(Clone, Copy)]
enum WorkspaceOpenRecovery {
    WorkingFiles,
    MetadataOnly,
    HistoryOnly,
}

impl OpenWorkspace {
    /// Maximum review cards returned in one workspace-state response.
    const MAX_REVIEW_ITEMS: usize = 32;

    /// Maximum durable operations rendered for one review subject.
    const MAX_REVIEW_OPERATIONS: usize = 128;

    /// Maximum exact bundle changes rendered for one review card.
    const MAX_REVIEW_CHANGES: usize = 128;

    /// Open the workspace rooted at `root`: read the records, find the durable boundary, fold.
    ///
    /// A directory with no record file is a new workspace, not an error: opening it produces an
    /// empty one and creates the file, which is what a person expects the first time they point
    /// Mesh at a folder.
    ///
    /// # Errors
    ///
    /// [`OpenFailure`] — see its variants. A record file whose *last* append was interrupted is
    /// **not** a failure *as long as a whole record precedes it*: the unfinished bytes are
    /// reported and everything before them is served, because an interrupted append never
    /// produced a record anybody was told about. Unfinished bytes with **nothing** before them
    /// are [`OpenFailure::NothingReadable`], because there is no boundary left to serve and a
    /// folder served as empty is a crash reported as a clean start.
    pub fn open(root: &Path) -> Result<Self, OpenFailure> {
        Self::open_with_trusted_reviewers(root, &crate::TrustedReviewers::default())
    }

    /// Create or reopen an external private store and present only its human-named child.
    ///
    /// The returned workspace root is the ordinary folder a person should open in Finder, an
    /// editor or an agent. The journal, SQLite databases and CAS remain structurally outside it.
    /// An existing directory is adopted only when its canonical marker already exists.
    pub fn open_presented(storage_root: &Path) -> Result<Self, OpenFailure> {
        Self::open_presented_with_trusted_reviewers(
            storage_root,
            &crate::TrustedReviewers::default(),
        )
    }

    /// [`Self::open_presented`] with explicit reviewer trust.
    pub fn open_presented_with_trusted_reviewers(
        storage_root: &Path,
        trusted_reviewers: &crate::TrustedReviewers,
    ) -> Result<Self, OpenFailure> {
        let created = match fs::create_dir(storage_root) {
            Ok(()) => true,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let metadata =
                    fs::symlink_metadata(storage_root).map_err(OpenFailure::Unreachable)?;
                if !metadata.is_dir() || metadata.file_type().is_symlink() {
                    return Err(OpenFailure::Unreachable(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "the private-store path is not a real directory",
                    )));
                }
                false
            }
            Err(error) => return Err(OpenFailure::Unreachable(error)),
        };
        let storage_root = storage_root
            .canonicalize()
            .map_err(OpenFailure::Unreachable)?;
        let pinned =
            PinnedWorkspaceRoot::open(storage_root.clone()).map_err(OpenFailure::Unreachable)?;
        let filesystem = pinned.filesystem();
        let marker = storage_root.join(PRESENTED_LAYOUT_MARKER_NAME);
        if created {
            filesystem
                .stage(&marker, PRESENTED_LAYOUT_MARKER_BYTES)
                .and_then(|()| filesystem.sync_file(&marker))
                .and_then(|()| filesystem.sync_dir(&storage_root))
                .map_err(OpenFailure::Unreachable)?;
        } else {
            let bytes = filesystem.read(&marker).map_err(|error| {
                OpenFailure::Unreachable(io::Error::new(
                    error.kind(),
                    "an existing private-store directory has no readable presented-workspace marker",
                ))
            })?;
            if bytes != PRESENTED_LAYOUT_MARKER_BYTES {
                return Err(OpenFailure::Unreachable(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "an existing private-store directory has a noncanonical presented-workspace marker",
                )));
            }
        }
        let presented =
            presented_workspace_path(&storage_root).map_err(OpenFailure::Unreachable)?;
        filesystem
            .create_dir_all(&presented)
            .map_err(OpenFailure::Unreachable)?;
        Self::open_layout(&presented, Some(&storage_root), trusted_reviewers, true)
    }

    /// Open a workspace and derive its protected shared version under an explicit reviewer trust
    /// configuration.
    ///
    /// Legacy reviewer keys identify readable historical signatures but cannot make the answer
    /// available. The v1 fold requires one trusted ES256 credential, an exact review, native user
    /// presence, and a durable approval record.
    ///
    /// # Errors
    ///
    /// The same [`OpenFailure`] variants as [`Self::open`]. Invalid or stale receipts are not open
    /// failures: they simply have no authority to advance the shared version.
    pub fn open_with_trusted_reviewers(
        root: &Path,
        trusted_reviewers: &crate::TrustedReviewers,
    ) -> Result<Self, OpenFailure> {
        Self::open_layout(root, None, trusted_reviewers, true)
    }

    /// Reopen one initialized workspace without creating a missing root, namespace, or journal.
    ///
    /// This is the restart/navigation counterpart to [`Self::open_with_trusted_reviewers`]. A
    /// remembered path is a hint, not authority to initialize an empty replacement when prior
    /// state disappeared.
    ///
    /// # Errors
    ///
    /// The same [`OpenFailure`] variants as [`Self::open`], including an unreachable refusal when
    /// any required existing path is absent.
    pub fn reopen_with_trusted_reviewers(
        root: &Path,
        trusted_reviewers: &crate::TrustedReviewers,
    ) -> Result<Self, OpenFailure> {
        Self::open_layout(root, None, trusted_reviewers, false)
    }

    /// Reconstruct retained history with an ephemeral index and no working-file recovery.
    /// The expected installation is checked before opening journal or payload storage.
    pub(crate) fn reopen_history(
        root: &Path,
        expected_installation: &str,
        parent: crate::ProtectedWorkspaceRoot,
        trusted: &crate::TrustedReviewers,
    ) -> Result<Self, OpenFailure> {
        let pinned =
            PinnedWorkspaceRoot::open(root.to_path_buf()).map_err(OpenFailure::Unreachable)?;
        let storage_path = workspace_storage_root(root).map_err(OpenFailure::Unreachable)?;
        let storage =
            PinnedWorkspaceRoot::open(storage_path.clone()).map_err(OpenFailure::Unreachable)?;
        let installation = workspace_installation(
            pinned.identity().map_err(OpenFailure::Unreachable)?,
            storage.identity().map_err(OpenFailure::Unreachable)?,
        );
        if installation != expected_installation
            || !pinned.is_within(parent).map_err(OpenFailure::Unreachable)?
            || !storage
                .is_within(parent)
                .map_err(OpenFailure::Unreachable)?
        {
            return Err(OpenFailure::Unreachable(io::Error::other(
                "saved lane installation changed",
            )));
        }
        Self::open_layout_inner(
            root,
            Some(&storage_path),
            trusted,
            false,
            Some((pinned, storage)),
            WorkspaceOpenRecovery::HistoryOnly,
        )
    }

    /// Open a newly prepared workspace through directory descriptors already held by its creator.
    ///
    /// Folder import uses this path between copy verification and receipt publication. Reopening
    /// either namespace by pathname in that interval would let a same-user rename replace the
    /// verified directory after the last identity check and redirect journal or CAS creation.
    pub(crate) fn open_prepared_layout(
        root: &Path,
        pinned_root: PinnedWorkspaceRoot,
        storage_root: &Path,
        storage_pinned_root: PinnedWorkspaceRoot,
        trusted_reviewers: &crate::TrustedReviewers,
    ) -> Result<Self, OpenFailure> {
        Self::open_layout_inner(
            root,
            Some(storage_root),
            trusted_reviewers,
            true,
            Some((pinned_root, storage_pinned_root)),
            WorkspaceOpenRecovery::WorkingFiles,
        )
    }

    fn open_layout(
        root: &Path,
        explicit_storage_root: Option<&Path>,
        trusted_reviewers: &crate::TrustedReviewers,
        create_missing: bool,
    ) -> Result<Self, OpenFailure> {
        Self::open_layout_inner(
            root,
            explicit_storage_root,
            trusted_reviewers,
            create_missing,
            None,
            WorkspaceOpenRecovery::WorkingFiles,
        )
    }

    /// Open only an attachment's external history store. No observed source path is accepted.
    /// The caller holds the private-store initialization guard for the entire read/write operation.
    pub(crate) fn open_attachment_store(
        metadata: &Path,
        pinned: PinnedWorkspaceRoot,
        create_missing: bool,
    ) -> Result<Self, OpenFailure> {
        Self::open_attachment_store_with_trusted_reviewers(
            metadata,
            pinned,
            create_missing,
            &crate::TrustedReviewers::default(),
        )
    }

    pub(crate) fn open_attachment_store_with_trusted_reviewers(
        metadata: &Path,
        pinned: PinnedWorkspaceRoot,
        create_missing: bool,
        trusted: &crate::TrustedReviewers,
    ) -> Result<Self, OpenFailure> {
        Self::open_layout_inner(
            metadata,
            Some(metadata),
            trusted,
            create_missing,
            Some((pinned.clone(), pinned)),
            WorkspaceOpenRecovery::MetadataOnly,
        )
    }

    fn open_layout_inner(
        root: &Path,
        explicit_storage_root: Option<&Path>,
        trusted_reviewers: &crate::TrustedReviewers,
        create_missing: bool,
        prepared: Option<(PinnedWorkspaceRoot, PinnedWorkspaceRoot)>,
        recovery: WorkspaceOpenRecovery,
    ) -> Result<Self, OpenFailure> {
        let started = Instant::now();
        if create_missing && prepared.is_none() {
            fs::create_dir_all(root).map_err(OpenFailure::Unreachable)?;
        }
        let uses_prepared_authority = prepared.is_some();
        let selected_root = WorkspaceRoot::new(root.to_path_buf());
        let (
            resolved_root,
            pinned_root,
            physical_identity,
            storage_path,
            storage_pinned_root,
            storage_identity,
            _initialization_guard,
        ) = if let Some((pinned_root, storage_pinned_root)) = prepared {
            let initialization_guard = crate::workspace_custody::lock_workspace_initialization(
                &pinned_root,
            )
            .map_err(|error| OpenFailure::Unreachable(io::Error::other(error.to_string())))?;
            pinned_root
                .ensure_namespace_identity()
                .and_then(|()| storage_pinned_root.ensure_namespace_identity())
                .map_err(OpenFailure::Unreachable)?;
            let storage_path = explicit_storage_root
                .ok_or_else(|| {
                    OpenFailure::Unreachable(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "a prepared workspace has no private-store path",
                    ))
                })?
                .to_path_buf();
            let (physical_device, physical_inode) =
                pinned_root.identity().map_err(OpenFailure::Unreachable)?;
            let (storage_device, storage_inode) = storage_pinned_root
                .identity()
                .map_err(OpenFailure::Unreachable)?;
            (
                root.to_path_buf(),
                pinned_root,
                WorkspaceDirectoryIdentity {
                    device: physical_device,
                    inode: physical_inode,
                },
                storage_path,
                storage_pinned_root,
                WorkspaceDirectoryIdentity {
                    device: storage_device,
                    inode: storage_inode,
                },
                initialization_guard,
            )
        } else {
            let resolved_root = root.canonicalize().map_err(OpenFailure::Unreachable)?;
            let physical_identity =
                workspace_directory_identity(&resolved_root).map_err(OpenFailure::Unreachable)?;
            let pinned_root = PinnedWorkspaceRoot::open(resolved_root.clone())
                .map_err(OpenFailure::Unreachable)?;
            let initialization_guard = crate::workspace_custody::lock_workspace_initialization(
                &pinned_root,
            )
            .map_err(|error| OpenFailure::Unreachable(io::Error::other(error.to_string())))?;
            let storage_path = explicit_storage_root
                .map_or_else(
                    || workspace_storage_root(&resolved_root),
                    |storage| storage.canonicalize(),
                )
                .map_err(OpenFailure::Unreachable)?;
            if workspace_directory_identity(&resolved_root).map_err(OpenFailure::Unreachable)?
                != physical_identity
            {
                return Err(OpenFailure::Unreachable(io::Error::other(
                    "the workspace directory changed while private storage was selected",
                )));
            }
            if storage_path != resolved_root && storage_path.starts_with(&resolved_root) {
                if create_missing {
                    pinned_root
                        .filesystem()
                        .create_dir_all(&storage_path)
                        .map_err(OpenFailure::Unreachable)?;
                } else if !storage_path.is_dir() {
                    return Err(OpenFailure::Unreachable(io::Error::new(
                        io::ErrorKind::NotFound,
                        "the remembered workspace private namespace is missing",
                    )));
                }
            } else if !storage_path.is_dir() {
                return Err(OpenFailure::Unreachable(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "the private store is not a real directory",
                )));
            }
            let storage_pinned_root = if storage_path == resolved_root {
                pinned_root.clone()
            } else {
                PinnedWorkspaceRoot::open(storage_path.clone()).map_err(OpenFailure::Unreachable)?
            };
            let storage_identity =
                workspace_directory_identity(&storage_path).map_err(OpenFailure::Unreachable)?;
            (
                resolved_root,
                pinned_root,
                physical_identity,
                storage_path,
                storage_pinned_root,
                storage_identity,
                initialization_guard,
            )
        };
        let physical_root = WorkspaceRoot::new(resolved_root.clone());
        let storage_root = WorkspaceRoot::new(storage_path.clone());
        let record_file = storage_path.join(RECORD_FILE_NAME);
        let record_relative = record_file.strip_prefix(&storage_path).map_err(|_| {
            OpenFailure::Unreachable(io::Error::other(
                "the record file escaped the pinned private store",
            ))
        })?;
        let mut file = if create_missing {
            RecordFile::open_pinned(&storage_pinned_root, record_relative, record_file.clone())
        } else if matches!(recovery, WorkspaceOpenRecovery::HistoryOnly) {
            storage_pinned_root
                .filesystem()
                .read_only()
                .read_file(record_relative)
                .map(|file| RecordFile {
                    path: record_file.clone(),
                    file,
                })
        } else {
            RecordFile::open_existing_pinned(
                &storage_pinned_root,
                record_relative,
                record_file.clone(),
            )
        }
        .map_err(OpenFailure::Unreachable)?;
        let bytes = file.read_all().map_err(OpenFailure::Unreachable)?;

        let scan = scan_journal(&bytes).map_err(OpenFailure::Damaged)?;
        let boundary = scan.boundary();
        let tail = scan.tail();
        if boundary.records == 0 && tail.is_fragment() {
            return Err(OpenFailure::NothingReadable {
                unfinished_bytes: tail.discarded_bytes(),
            });
        }
        let mut records_by_kind: Vec<(RecordKind, usize)> =
            RecordKind::ALL.iter().map(|kind| (*kind, 0usize)).collect();
        for record in scan.records() {
            let kind = record.kind();
            if let Some(entry) = records_by_kind.iter_mut().find(|(known, _)| *known == kind) {
                entry.1 += 1;
            }
        }

        let records = scan.into_records();
        let database = storage_root.database();
        let store = if uses_prepared_authority {
            open_and_rebuild_transient_index(&records, boundary)?
        } else {
            open_and_rebuild_index(&database, &records, boundary)?
        };
        let index = store.index();
        let rows_per_table: Vec<(&'static str, usize)> = TABLES
            .iter()
            .map(|table| {
                (
                    table.name,
                    index.rows(table.name).map_or(0, |rows| rows.len()),
                )
            })
            .collect();
        let total_rows = rows_per_table.iter().map(|(_, rows)| rows).sum();
        let digest = index.default_digest();
        let record_index = index.clone();

        let filesystem = storage_pinned_root.filesystem();
        let filesystem = if matches!(recovery, WorkspaceOpenRecovery::HistoryOnly) {
            filesystem.read_only()
        } else {
            filesystem
        };
        let payload_store = Cas::<_, mesh_cas::Blake3>::with_filesystem(storage_path, filesystem)
            .map_err(OpenFailure::PayloadStore)?;
        let names = materialize_names(index, &payload_store);
        let private_version = version_state::fold(index);
        let shared_version = publication::fold(&records, &payload_store, trusted_reviewers);
        let operations = index.operation_count();
        let actors = index.actors().len();
        let manifests = index.manifest_ids().len();
        let peers = index.peer_ids().len();
        let reviews = index.review_bundles().len();

        let elapsed = started.elapsed();
        let outcome = if tail.is_fragment() {
            RecoveryOutcome::RebuiltAfterAnInterruptedSave {
                records: boundary.records,
                rows: total_rows,
                digest,
                discarded_bytes: tail.discarded_bytes(),
            }
        } else {
            RecoveryOutcome::Rebuilt {
                records: boundary.records,
                rows: total_rows,
                digest,
            }
        };

        let mut opened = Self {
            root: selected_root,
            physical_root,
            storage_root,
            storage_pinned_root,
            storage_identity,
            physical_identity,
            pinned_root,
            record_file,
            journal: file,
            database_file: database.as_path().to_path_buf(),
            store,
            payload_store,
            record_index,
            boundary,
            tail,
            diagnostic: RecoveryDiagnostic::new(outcome, elapsed, RECOVERY_BUDGET),
            records_by_kind,
            rows_per_table,
            operations,
            actors,
            manifests,
            peers,
            reviews,
            digest,
            private_version,
            shared_version,
            shared_history: Vec::new(),
            review_base_hints: BTreeMap::new(),
            materialized_state: names.state,
            entries: names.entries,
            file_histories: names.file_histories,
            retired_entries: names.retired_entries,
            conditions: names.conditions,
            names_answered: names.complete,
        };
        let shared_version = publication::fold_human(
            &records,
            &opened.payload_store,
            trusted_reviewers,
            |review, current| opened.human_approval_context_at(review, current).ok(),
        );
        opened.shared_version = shared_version.version;
        opened.shared_history = shared_version.heads;
        opened.review_base_hints = shared_version.review_bases;
        if matches!(recovery, WorkspaceOpenRecovery::WorkingFiles) {
            match crate::managed_mutation::reconcile_pending_mutation(root, |id| {
                opened.has_operation(id)
            }) {
                Ok(Some(recovery)) => opened.conditions.push(condition(
                    recovery.code(),
                    recovery.message(),
                    Vec::new(),
                )),
                Ok(None) => {}
                Err(error) => {
                    opened
                        .conditions
                        .push(condition(error.code(), error.message(), Vec::new()))
                }
            }
        }
        opened
            .ensure_physical_root()
            .map_err(OpenFailure::Unreachable)?;
        Ok(opened)
    }

    /// Where this workspace lives.
    #[must_use]
    pub fn root(&self) -> &WorkspaceRoot {
        &self.root
    }

    /// The workspace directory target pinned when this instance was opened.
    ///
    /// This is deliberately separate from [`Self::root`], which preserves the caller's spelling
    /// for display. Product operations must use this path so a later alias retarget cannot split
    /// one mutation across two workspace directories.
    pub(crate) fn physical_root(&self) -> &WorkspaceRoot {
        &self.physical_root
    }

    /// Device and inode captured for the physical workspace directory at open time.
    ///
    /// Native navigation may use this only to prove that an ordinary path still names the same
    /// directory object. It is not a portable workspace identity and must never be serialized.
    pub(crate) const fn physical_directory_identity(&self) -> (u64, u64) {
        (self.physical_identity.device, self.physical_identity.inode)
    }

    /// The private storage namespace selected when this workspace was opened.
    pub(crate) fn storage_root(&self) -> &WorkspaceRoot {
        &self.storage_root
    }

    /// Whether private state is structurally outside the native folder shown to the person.
    ///
    /// In this layout a top-level user entry may truthfully be called `records.mesh`, `chunks`,
    /// or `metadata.sqlite`: the actual private objects are siblings of the presented folder, not entries
    /// below it. Legacy and `.mesh/` layouts keep their historical name reservation.
    fn has_external_private_store(&self) -> bool {
        self.physical_root
            .as_path()
            .file_name()
            .is_some_and(is_presented_directory_name)
            && self.physical_root.as_path().parent() == Some(self.storage_root.as_path())
    }

    /// Whether `link` is the one navigation-only link Mesh created for Codex integration.
    ///
    /// This exception is deliberately structural and exact. A caller cannot hide a general link
    /// by naming it `.codex`, changing its target, weakening the private directory permissions, or
    /// adding another private input beside the generated configuration.
    pub(crate) fn exact_private_codex_link(&self, relative: &Path, link: &Path) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;

            const PROJECT_LINK: &str = ".codex";
            const PROJECT_LINK_TARGET: &str = "../integrations/codex";

            if relative != Path::new(PROJECT_LINK)
                || !fs::read_link(link).is_ok_and(|target| target == Path::new(PROJECT_LINK_TARGET))
            {
                return false;
            }
            let integrations = self.storage_root.as_path().join("integrations");
            let private = integrations.join("codex");
            let config = private.join("config.toml");
            for directory in [&integrations, &private] {
                let Ok(metadata) = fs::symlink_metadata(directory) else {
                    return false;
                };
                if !metadata.is_dir()
                    || metadata.file_type().is_symlink()
                    || metadata.permissions().mode() & 0o077 != 0
                {
                    return false;
                }
            }
            let Ok(config_metadata) = fs::symlink_metadata(&config) else {
                return false;
            };
            if !config_metadata.is_file()
                || config_metadata.file_type().is_symlink()
                || config_metadata.permissions().mode() & 0o077 != 0
            {
                return false;
            }
            let Ok(mut entries) = fs::read_dir(&private) else {
                return false;
            };
            let Some(Ok(only)) = entries.next() else {
                return false;
            };
            if only.file_name() != std::ffi::OsStr::new("config.toml") || entries.next().is_some() {
                return false;
            }
            fs::canonicalize(link)
                .and_then(|resolved| fs::canonicalize(private).map(|expected| resolved == expected))
                .unwrap_or(false)
        }
        #[cfg(not(unix))]
        {
            let _ = (relative, link);
            false
        }
    }

    /// Opaque identity of the physical directory object admitted by this open workspace.
    ///
    /// The record-fold digest alone cannot distinguish an exact byte-for-byte clone installed at
    /// the same pathname. Desktop mutations bind this value together with the root and fold digest
    /// so a second local client cannot redirect a stale control into that replacement directory.
    #[must_use]
    pub(crate) fn installation(&self) -> String {
        workspace_installation(
            (self.physical_identity.device, self.physical_identity.inode),
            (self.storage_identity.device, self.storage_identity.inode),
        )
    }

    pub(crate) fn pinned_root(&self) -> &PinnedWorkspaceRoot {
        &self.pinned_root
    }

    /// Descriptor authority for the private journal/database/CAS namespace.
    pub(crate) fn storage_pinned_root(&self) -> &PinnedWorkspaceRoot {
        &self.storage_pinned_root
    }

    /// Refuse a workspace directory that was replaced after this instance opened.
    pub(crate) fn ensure_physical_root(&self) -> io::Result<()> {
        if workspace_directory_identity(self.physical_root.as_path())? == self.physical_identity
            && workspace_directory_identity(self.storage_root.as_path())? == self.storage_identity
        {
            Ok(())
        } else {
            Err(io::Error::other(
                "the workspace or its private-store directory changed after it was opened",
            ))
        }
    }

    /// Refresh all record-derived answers through the already-open journal, index and root.
    ///
    /// This deliberately does not reopen the selected pathname. A directory moved away after
    /// admission remains the authority for the lifetime of this value; only a later explicit
    /// `workspace.open` may select a different directory object.
    pub(crate) fn refresh_with_trusted_reviewers(
        &mut self,
        trusted_reviewers: &crate::TrustedReviewers,
    ) -> Result<(), OpenFailure> {
        let started = Instant::now();
        let bytes = self.journal.read_all().map_err(OpenFailure::Unreachable)?;
        let scan = scan_journal(&bytes).map_err(OpenFailure::Damaged)?;
        let boundary = scan.boundary();
        let tail = scan.tail();
        if boundary.records == 0 && tail.is_fragment() {
            return Err(OpenFailure::NothingReadable {
                unfinished_bytes: tail.discarded_bytes(),
            });
        }
        let mut records_by_kind: Vec<(RecordKind, usize)> =
            RecordKind::ALL.iter().map(|kind| (*kind, 0usize)).collect();
        for record in scan.records() {
            if let Some(entry) = records_by_kind
                .iter_mut()
                .find(|(known, _)| *known == record.kind())
            {
                entry.1 += 1;
            }
        }
        let records = scan.into_records();
        let ledger = self
            .store
            .index()
            .rows("schema_version")
            .unwrap_or_default();
        let (record_index, _) =
            mesh_store::rebuild(records.iter().cloned(), ledger).map_err(|error| {
                OpenFailure::Contradictory {
                    detail: error.to_string(),
                    readable: boundary,
                }
            })?;
        let index = &record_index;
        let rows_per_table: Vec<(&'static str, usize)> = TABLES
            .iter()
            .map(|table| {
                (
                    table.name,
                    index.rows(table.name).map_or(0, |rows| rows.len()),
                )
            })
            .collect();
        let total_rows = rows_per_table.iter().map(|(_, rows)| rows).sum();
        let digest = index.default_digest();
        let payload_store = Cas::<_, mesh_cas::Blake3>::with_filesystem(
            self.storage_root.as_path().to_path_buf(),
            self.storage_pinned_root.filesystem(),
        )
        .map_err(OpenFailure::PayloadStore)?;
        let names = materialize_names(index, &payload_store);

        self.boundary = boundary;
        self.tail = tail;
        self.diagnostic = RecoveryDiagnostic::new(
            if tail.is_fragment() {
                RecoveryOutcome::RebuiltAfterAnInterruptedSave {
                    records: boundary.records,
                    rows: total_rows,
                    digest,
                    discarded_bytes: tail.discarded_bytes(),
                }
            } else {
                RecoveryOutcome::Rebuilt {
                    records: boundary.records,
                    rows: total_rows,
                    digest,
                }
            },
            started.elapsed(),
            RECOVERY_BUDGET,
        );
        self.records_by_kind = records_by_kind;
        self.rows_per_table = rows_per_table;
        self.operations = index.operation_count();
        self.actors = index.actors().len();
        self.manifests = index.manifest_ids().len();
        self.peers = index.peer_ids().len();
        self.reviews = index.review_bundles().len();
        self.digest = digest;
        self.private_version = version_state::fold(index);
        self.record_index = record_index;
        self.payload_store = payload_store;
        self.materialized_state = names.state;
        self.entries = names.entries;
        self.file_histories = names.file_histories;
        self.retired_entries = names.retired_entries;
        self.conditions = names.conditions;
        self.names_answered = names.complete;
        let shared = publication::fold_human(
            &records,
            &self.payload_store,
            trusted_reviewers,
            |review, current| self.human_approval_context_at(review, current).ok(),
        );
        self.shared_version = shared.version;
        self.shared_history = shared.heads;
        self.review_base_hints = shared.review_bases;
        Ok(())
    }

    /// The record file inside it.
    #[must_use]
    pub fn record_file(&self) -> &Path {
        &self.record_file
    }

    /// Stable operating-system identity of the already-open journal generation, when available.
    ///
    /// Support correlation reads this from the pinned file descriptor rather than looking the
    /// pathname up again after composing diagnostic facts.
    pub(crate) fn support_file_identity(&self) -> io::Result<Option<(u64, u64)>> {
        self.journal.support_identity()
    }

    /// The durable SQLite index beside the immutable record journal.
    #[must_use]
    pub fn database_file(&self) -> &Path {
        &self.database_file
    }

    /// The live record-derived store used by the daemon's checkpoint composition.
    ///
    /// Crate-private because callers must also append the corresponding immutable records before
    /// exposing an acknowledgement; [`crate::checkpoint_storage`] owns that ordering.
    pub(crate) fn checkpoint_store_mut(&mut self) -> &mut Store<Sqlite> {
        &mut self.store
    }

    pub(crate) fn checkpoint_parts_mut(&mut self) -> (&mut Store<Sqlite>, &mut RecordFile) {
        (&mut self.store, &mut self.journal)
    }

    pub(crate) fn checkpoint_journal_mut(&mut self) -> &mut RecordFile {
        &mut self.journal
    }

    /// Reproduce a process-lost private-save acknowledgement from the index rebuilt from the
    /// immutable journal. The pending ChangeSet itself must also be present in that journal.
    pub(crate) fn verify_pending_private_save(
        &self,
        pending: mesh_store::PendingMeaningfulSave,
    ) -> Result<mesh_store::PrivateSaved, mesh_store::PendingPrivateSaveError> {
        self.store.verify_pending_private_save(pending)
    }

    /// Reproduce a pending acknowledgement at an exact earlier journal boundary.
    pub(crate) fn verify_pending_private_save_at_boundary(
        &mut self,
        pending: mesh_store::PendingMeaningfulSave,
        boundary: DurableBoundary,
    ) -> Result<mesh_store::PrivateSaved, mesh_store::PendingPrivateSaveBoundaryError<io::Error>>
    {
        self.store
            .verify_pending_private_save_at_boundary(&mut self.journal, pending, boundary)
    }

    /// The migration version the live database connection verified.
    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.store.schema_version()
    }

    /// How far the record file is durable.
    #[must_use]
    pub const fn boundary(&self) -> DurableBoundary {
        self.boundary
    }

    /// What lies after the boundary — nothing, or an interrupted append.
    #[must_use]
    pub const fn tail(&self) -> TailResidue {
        self.tail
    }

    /// What this open found, in the shape `startup.report` answers from.
    #[must_use]
    pub const fn diagnostic(&self) -> &RecoveryDiagnostic {
        &self.diagnostic
    }

    /// How many records of each kind are on disk, in `RecordKind::ALL` order.
    #[must_use]
    pub fn records_by_kind(&self) -> &[(RecordKind, usize)] {
        &self.records_by_kind
    }

    /// How many rows each index table holds after the fold.
    #[must_use]
    pub fn rows_per_table(&self) -> &[(&'static str, usize)] {
        &self.rows_per_table
    }

    /// How many operations the fold indexed.
    #[must_use]
    pub const fn operations(&self) -> usize {
        self.operations
    }

    /// How many distinct actors those operations name.
    #[must_use]
    pub const fn actors(&self) -> usize {
        self.actors
    }

    /// How many file manifests are indexed.
    #[must_use]
    pub const fn manifests(&self) -> usize {
        self.manifests
    }

    /// How many peers are in this workspace's replication set.
    #[must_use]
    pub const fn peers(&self) -> usize {
        self.peers
    }

    /// How many review bundles are open.
    #[must_use]
    pub const fn reviews(&self) -> usize {
        self.reviews
    }

    /// The digest of the record fold committed to `metadata.sqlite` during this open.
    #[must_use]
    pub const fn digest(&self) -> mesh_store::Digest16 {
        self.digest
    }

    /// This replica's own version state, derived from the records this open folded.
    ///
    /// The subject the `shared and private version state` refusal used to cover, for the private
    /// half. See [`crate::version_state`] for what is derived and what is only claimed.
    #[must_use]
    pub const fn private_version(&self) -> &PrivateVersion {
        &self.private_version
    }

    /// The protected shared version derived from durable HumanHeld decisions and their receipts.
    ///
    /// `None` remains the answer until the exact review and its user-verified approval can be
    /// folded. A legacy public key or software signature cannot remove [`Self::not_yet`]'s
    /// refusal.
    #[must_use]
    pub const fn shared_version(&self) -> Option<mesh_approval::HeadId> {
        self.shared_version.head()
    }

    /// Only heads admitted by the trusted approval fold, not merely a stored signed envelope.
    pub(crate) fn has_verified_shared_head(&self, head: mesh_approval::HeadId) -> bool {
        self.shared_history.contains(&head)
    }

    /// Whether the immutable record fold contains this exact operation.
    #[must_use]
    pub fn has_operation(&self, id: &RecordDigest) -> bool {
        self.record_index.operation(id).is_some()
    }

    /// Compute the exact next-publication review bundle for the current private head.
    ///
    /// The selected operation must be the sole current causal tip and descend from the protected
    /// shared head. Both states are rebuilt entirely from journal and verified CAS truth rather
    /// than the mutable working tree; genesis is used only before the first approval.
    pub(crate) fn first_publication_review_bundle(
        &self,
        target: RecordDigest,
    ) -> Result<RecordDigest, String> {
        let canonical_head = self
            .shared_version()
            .unwrap_or(crate::publication::GENESIS_SHARED_HEAD);
        self.publication_review(target, canonical_head, true)
            .map(|(bundle, _, _)| RecordDigest::from_bytes(*bundle.id().digest().as_bytes()))
    }

    /// Pin a private saved version for inspection, even after later private work has started.
    /// This creates no approval authority. Publication still requires its current-head checks.
    pub(crate) fn saved_publication_review_bundle(
        &self,
        target: RecordDigest,
    ) -> Result<RecordDigest, String> {
        let canonical = self
            .shared_version()
            .unwrap_or(crate::publication::GENESIS_SHARED_HEAD);
        self.publication_review(target, canonical, false)
            .map(|(bundle, _, _)| RecordDigest::from_bytes(*bundle.id().digest().as_bytes()))
    }

    /// Resolve durable review identity from the complete index, never a bounded UI projection.
    pub(crate) fn recorded_review_for_actor(
        &self,
        target: RecordDigest,
        actor: RecordDigest,
    ) -> Result<Option<RecordDigest>, String> {
        let mut found = None;
        for bundle in self.record_index.review_bundles() {
            if self.review(&bundle).is_some_and(|review| {
                review.subject_operation == target && review.opened_by == actor
            }) {
                if found.is_some() {
                    return Err("multiple saved reviews match the same actor and target".into());
                }
                found = Some(bundle);
            }
        }
        Ok(found)
    }

    pub(crate) fn accepted_main_review(&self) -> Result<Option<ReviewRecord>, String> {
        let Some(head) = self.shared_version() else {
            return Ok(None);
        };
        for bundle in self.record_index.review_bundles() {
            let Some(review) = self.review(&bundle) else {
                continue;
            };
            if self.approved_envelope(&bundle).is_ok()
                && self.human_approval_context(&review)?.reviewed_actor_head() == head
            {
                return Ok(Some(review));
            }
        }
        Err("verified main has no exact retained review".to_owned())
    }

    /// Refuse challenge reuse before append, rather than poisoning a previously valid main.
    pub(crate) fn approval_challenge_used(&self, challenge: &[u8; 32]) -> Result<bool, String> {
        for bundle in self.record_index.review_bundles() {
            for approval in self
                .record_index
                .approvals_for_bundle(&bundle)
                .filter(|approval| approval.verdict == ReviewVerdict::Approved)
            {
                let bytes = self.approval_receipt(approval.approval)?;
                let receipt = mesh_approval::HumanApprovalReceipt::from_canonical_bytes(&bytes)
                    .map_err(|error| error.to_string())?;
                if receipt.draft().expected().challenge() == challenge {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// Recompute the exact v1 human-approval context for one durable review record.
    pub(crate) fn human_approval_context(
        &self,
        review: &ReviewRecord,
    ) -> Result<mesh_approval::HumanApprovalContext, String> {
        let canonical_head = self.canonical_head_for_review(review)?;
        self.human_approval_artifact_at(review, canonical_head)
            .map(|(context, _bundle, _state)| context)
    }

    /// Recompute a context while the publication fold holds the exact predecessor head.
    ///
    /// This avoids consulting `self.shared_version` while that value is itself being rebuilt and
    /// makes every approval in a multi-version chain prove the precise head it advances from.
    pub(crate) fn human_approval_context_at(
        &self,
        review: &ReviewRecord,
        canonical_head: mesh_approval::HeadId,
    ) -> Result<mesh_approval::HumanApprovalContext, String> {
        self.human_approval_artifact_at(review, canonical_head)
            .map(|(context, _bundle, _state)| context)
    }

    /// Recompute the exact v1 context and a native, caller-independent summary of its bundle.
    ///
    /// The summary deliberately contains no actor-supplied prose. Paths are debug-escaped so a
    /// filename cannot manufacture another apparent field in the operating-system dialog.
    pub(crate) fn human_approval_preview(
        &self,
        review: &ReviewRecord,
    ) -> Result<
        (
            mesh_approval::HumanApprovalContext,
            String,
            mesh_approval::Digest32,
            mesh_approval::ReviewBundle,
            mesh_approval::WorkspaceState,
        ),
        String,
    > {
        let canonical_head = self.canonical_head_for_review(review)?;
        let (context, bundle, state) = self.human_approval_artifact_at(review, canonical_head)?;
        let presentation = bundle.presentation();
        let mut summary = format!("Exact reviewed changes: {}\n", presentation.len());
        for (index, change) in presentation.entries().iter().enumerate() {
            use std::fmt::Write as _;

            let before_path = change
                .path_before()
                .map_or_else(|| "not present".to_owned(), |path| format!("{path:?}"));
            let after_path = change
                .path_after()
                .map_or_else(|| "removed".to_owned(), |path| format!("{path:?}"));
            writeln!(
                summary,
                "{}. {} · {} -> {}\n   object {}\n   before {}\n   after {}",
                index + 1,
                change.effect(),
                before_path,
                after_path,
                change.object(),
                native_content_summary(change.before()),
                native_content_summary(change.after()),
            )
            .expect("writing to a String cannot fail");
        }
        let presentation_digest = presentation.digest();
        Ok((context, summary, presentation_digest, bundle, state))
    }

    fn human_approval_artifact_at(
        &self,
        review: &ReviewRecord,
        canonical_head: mesh_approval::HeadId,
    ) -> Result<
        (
            mesh_approval::HumanApprovalContext,
            mesh_approval::ReviewBundle,
            mesh_approval::WorkspaceState,
        ),
        String,
    > {
        let (bundle, _, state) =
            self.publication_review(review.subject_operation, canonical_head, false)?;
        if bundle.id().digest().as_bytes() != review.bundle.as_bytes() {
            return Err(
                "the durable review identifier does not match its recomputed bundle".into(),
            );
        }
        let operation = self
            .record_index
            .operation(&review.subject_operation)
            .ok_or_else(|| "the reviewed operation is absent".to_owned())?;
        let payload = self
            .payload_store
            .read(&CasDigest::from_bytes(*operation.payload_digest.as_bytes()))
            .map_err(|error| format!("the reviewed ChangeSet is unavailable: {error}"))?;
        let fields = decode_changeset_fields(&payload)
            .ok_or_else(|| "the reviewed payload is not a canonical ChangeSet".to_owned())?;
        let field = |name: &str| {
            mesh_operations::CHANGESET_SCHEMA
                .fields
                .iter()
                .position(|candidate| candidate.name == name)
                .and_then(|index| fields.get(index))
        };
        let workspace_id = match field("workspace_id") {
            Some(CanonicalValue::Bytes(bytes)) if bytes.len() == 16 => {
                let mut exact = [0_u8; 16];
                exact.copy_from_slice(bytes);
                mesh_approval::ApprovalWorkspaceId::from_bytes(exact)
            }
            _ => return Err("the reviewed ChangeSet has no workspace identifier".into()),
        };
        let target_epoch = match field("policy_epoch") {
            Some(CanonicalValue::Unsigned(epoch)) => *epoch,
            _ => return Err("the reviewed ChangeSet has no policy epoch".into()),
        };
        let current_epoch = self
            .record_index
            .highest_policy_epoch()
            .ok_or_else(|| "the workspace has no current policy epoch".to_owned())?;
        if target_epoch != current_epoch {
            return Err("the reviewed ChangeSet was authored under a stale policy epoch".into());
        }
        let context = mesh_approval::HumanApprovalContext::from_bundle(
            workspace_id,
            mesh_types::PolicyEpoch::new(current_epoch),
            &bundle,
        )
        .map_err(|error| error.to_string())?;
        Ok((context, bundle, state))
    }

    /// Recover the exact base committed by the immutable bundle. For an unapproved request,
    /// journal-order hints are tried first, then verified publication history (including genesis).
    /// Arrival order never substitutes for matching the recomputed bundle identity. Approval
    /// continues to check the current protected head independently of historical presentation.
    fn canonical_head_for_review(
        &self,
        review: &ReviewRecord,
    ) -> Result<mesh_approval::HeadId, String> {
        let mut approvals = self
            .record_index
            .approvals_for_bundle(&review.bundle)
            .filter(|approval| approval.verdict == ReviewVerdict::Approved);
        let Some(approval) = approvals.next() else {
            let current = self
                .shared_version()
                .unwrap_or(crate::publication::GENESIS_SHARED_HEAD);
            let candidates = self
                .review_base_hints
                .get(&review.bundle)
                .copied()
                .into_iter()
                .chain(std::iter::once(current))
                .chain(self.shared_history.iter().rev().copied())
                .chain(std::iter::once(crate::publication::GENESIS_SHARED_HEAD));
            let mut checked = BTreeSet::new();
            for base in candidates {
                if !checked.insert(base) {
                    continue;
                }
                if self
                    .publication_review(review.subject_operation, base, false)
                    .is_ok_and(|(bundle, _, _)| {
                        bundle.id().digest().as_bytes() == review.bundle.as_bytes()
                    })
                {
                    return Ok(base);
                }
            }
            // Preserve the existing mismatch/unavailable diagnostics downstream. A forged bundle
            // still fails exact recomputation; no unmatched historical base becomes authority.
            return Ok(current);
        };
        if approvals.next().is_some() {
            return Err("the review has more than one approved envelope".to_owned());
        }
        let bytes = self.approval_receipt(approval.approval)?;
        let receipt = mesh_approval::HumanApprovalReceipt::from_canonical_bytes(&bytes)
            .map_err(|_| "the durable approval receipt is invalid".to_owned())?;
        Ok(receipt
            .draft()
            .expected()
            .context()
            .expected_canonical_head())
    }

    /// Recompute one publication bundle from the exact causal closure it names.
    ///
    /// Opening a new review requires the target to remain the sole current head and its exact
    /// canonical predecessor to remain current. Rendering an already durable review deliberately
    /// does not: its historical closure is immutable, and a later private save must not make the
    /// earlier card disappear or silently replace its presentation with the newest state.
    fn publication_review(
        &self,
        target: RecordDigest,
        canonical_head: mesh_approval::HeadId,
        require_current: bool,
    ) -> Result<
        (
            mesh_approval::ReviewBundle,
            BTreeMap<mesh_approval::ObjectId, crate::ipc::Json>,
            mesh_approval::WorkspaceState,
        ),
        String,
    > {
        self.ensure_physical_root()
            .map_err(|error| format!("the workspace directory changed: {error}"))?;
        if require_current {
            if !self.names_answered {
                return Err("the saved workspace projection is incomplete".to_owned());
            }
            let current_shared = self
                .shared_version()
                .unwrap_or(crate::publication::GENESIS_SHARED_HEAD);
            if canonical_head != current_shared {
                return Err("the selected canonical review head is stale".to_owned());
            }

            let ready = self.record_index.causally_ready_operations();
            let ready_set = ready.iter().copied().collect::<BTreeSet<_>>();
            let referenced = ready
                .iter()
                .filter_map(|id| self.record_index.operation(id))
                .flat_map(|operation| operation.parents.iter().copied())
                .filter(|parent| ready_set.contains(parent))
                .collect::<BTreeSet<_>>();
            let tips = ready
                .iter()
                .copied()
                .filter(|id| !referenced.contains(id))
                .collect::<Vec<_>>();
            if tips.as_slice() != [target] {
                return Err(
                    "the selected review target is not the sole current private head".to_owned(),
                );
            }
        }

        let all = operation_records(&self.record_index);
        let selected = causal_operation_closure(&all, target)?;
        let subject = selected
            .get(&target)
            .ok_or_else(|| "the selected review target is absent".to_owned())?;
        let subject_actor = subject.actor;
        let (actor, actor_text) = self.review_workspace_state(selected.clone(), true)?;
        let private_head = review_head_for_records(&selected)?;
        let (canonical, canonical_text) =
            if canonical_head == crate::publication::GENESIS_SHARED_HEAD {
                (
                    mesh_approval::WorkspaceState::new(actor.root()),
                    BTreeMap::new(),
                )
            } else {
                let canonical_target = self.review_target_for_head(canonical_head)?;
                let canonical_records = causal_operation_closure(&all, canonical_target)?;
                if !canonical_records.keys().all(|id| selected.contains_key(id)) {
                    return Err(
                        "the selected private target does not descend from the shared version"
                            .to_owned(),
                    );
                }
                let (canonical, canonical_text) =
                    self.review_workspace_state(canonical_records, true)?;
                if canonical.root() != actor.root() {
                    return Err(
                        "the selected review versions name different workspace roots".to_owned(),
                    );
                }
                (canonical, canonical_text)
            };
        let approved_state = actor.clone();
        let request = mesh_approval::BundleRequest::new(
            canonical.clone(),
            canonical,
            canonical_head,
            actor,
            private_head,
            mesh_approval::ActorId::from_bytes(*subject_actor.as_bytes()),
        );
        let bundle = mesh_approval::compute_bundle(&request)
            .map_err(|error| format!("the review bundle cannot be computed: {error}"))?;
        let verified_text = bundle
            .presentation()
            .entries()
            .iter()
            .filter_map(|change| {
                let before = canonical_text.get(&change.object());
                let after = actor_text.get(&change.object());
                if (change.before().is_some() && before.is_none())
                    || (change.after().is_some() && after.is_none())
                {
                    return None;
                }
                verified_text_json(before, after).map(|diff| (change.object(), diff))
            })
            .collect();
        Ok((bundle, verified_text, approved_state))
    }

    pub(crate) fn review_target_for_head(
        &self,
        head: mesh_approval::HeadId,
    ) -> Result<RecordDigest, String> {
        let all = operation_records(&self.record_index);
        let mut matches = BTreeSet::new();
        for bundle in self.record_index.review_bundles() {
            let Some(review) = self.record_index.review(&bundle) else {
                continue;
            };
            let selected = causal_operation_closure(&all, review.subject_operation)?;
            if review_head_for_records(&selected)? == head {
                matches.insert(review.subject_operation);
            }
        }
        match matches.into_iter().collect::<Vec<_>>().as_slice() {
            [target] => Ok(*target),
            [] => Err("the shared version has no exact reviewed operation".to_owned()),
            _ => Err("the shared version names more than one reviewed operation".to_owned()),
        }
    }

    fn review_workspace_state(
        &self,
        selected: BTreeMap<RecordDigest, OperationRecord>,
        include_verified_text: bool,
    ) -> Result<
        (
            mesh_approval::WorkspaceState,
            BTreeMap<mesh_approval::ObjectId, VerifiedReviewText>,
        ),
        String,
    > {
        let materialized = materialize_operation_records(selected, &self.payload_store);
        if !materialized.complete {
            return Err(format!(
                "the saved workspace projection is incomplete: {}",
                materialized
                    .conditions
                    .iter()
                    .map(WorkspaceCondition::code)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        let materialized = materialized
            .state
            .ok_or_else(|| "the saved workspace projection is unavailable".to_owned())?;
        let root = mesh_approval::ObjectId::from_bytes(*materialized.root().as_bytes());
        let mut state = mesh_approval::WorkspaceState::new(root);
        let mut verified_text = BTreeMap::new();
        for (object_id, object) in materialized.objects() {
            if *object_id == materialized.root() || object.is_deleted() {
                continue;
            }
            // Unlink retains objects and their versions for historical recovery. Only objects
            // reachable through the saved namespace belong to this version's visible review,
            // including when an entire ancestor directory was unlinked.
            if materialized.path_of(*object_id).is_none() {
                continue;
            }
            let (directory, name) = materialized
                .binding_of(*object_id)
                .ok_or_else(|| format!("saved object {object_id} has no exact visible binding"))?;
            let review_object = mesh_approval::ObjectId::from_bytes(*object_id.as_bytes());
            let review_directory = mesh_approval::ObjectId::from_bytes(*directory.as_bytes());
            let review_name =
                mesh_approval::NormalizedName::new(name.as_str()).map_err(|error| {
                    format!("saved object {object_id} has an invalid review name: {error}")
                })?;
            state = match object.kind() {
                mesh_materializer::ObjectKind::Directory => {
                    state.with_directory(review_object, review_directory, review_name)
                }
                mesh_materializer::ObjectKind::File => match object.current_version() {
                    None => state.with_record(
                        review_object,
                        mesh_approval::StateObject::new(
                            mesh_approval::ObjectKind::File,
                            Some(review_directory),
                            Some(review_name),
                            None,
                        ),
                    ),
                    Some(version) => {
                        let file = materialized.file_version(version).ok_or_else(|| {
                            format!("saved object {object_id} names an unavailable file version")
                        })?;
                        let manifest_id = RecordDigest::from_bytes(*file.manifest_id().as_bytes());
                        let manifest =
                            self.record_index.manifest(&manifest_id).ok_or_else(|| {
                                format!(
                                    "saved object {object_id} names an unavailable file manifest"
                                )
                            })?;
                        let bytes = reconstruct_manifest(&self.payload_store, manifest)
                            .map_err(|error| error.to_string())?;
                        let version = mesh_approval::VersionId::from_bytes(*version.as_bytes());
                        let digest = mesh_types::Blake3::digest_bytes(&bytes);
                        if include_verified_text {
                            if let Some(preview) = verified_review_text(
                                version,
                                mesh_approval::Digest32::from_bytes(*digest.as_bytes()),
                                &bytes,
                            ) {
                                verified_text.insert(review_object, preview);
                            }
                        }
                        let content = mesh_approval::Content::Binary {
                            version,
                            digest: *digest.as_bytes(),
                            byte_length: u64::try_from(bytes.len())
                                .map_err(|_| "a saved file is too large to review".to_owned())?,
                        };
                        state.with_file(review_object, review_directory, review_name, content)
                    }
                },
            };
        }
        Ok((state, verified_text))
    }

    pub(crate) fn dismiss_managed_mutation_recovery_condition(&mut self) {
        self.conditions.retain(|condition| {
            !matches!(
                condition.code(),
                "managed-mutation-rolled-back" | "managed-mutation-completed"
            )
        });
    }

    pub(crate) fn managed_mutation_recovery_needed(&self) -> bool {
        self.conditions.iter().any(|condition| {
            matches!(
                condition.code(),
                "managed-mutation-recovery-needed" | "checkpoint-recovery-needs-attention"
            )
        })
    }

    /// Whether this process has already exposed the checkpoint recovery refusal.
    pub(crate) fn checkpoint_recovery_needs_attention(&self) -> bool {
        self.conditions
            .iter()
            .any(|condition| condition.code() == "checkpoint-recovery-needs-attention")
    }

    /// Add the checkpoint recovery condition once without exposing internal verification detail.
    pub(crate) fn record_checkpoint_recovery_attention(&mut self) {
        if !self.checkpoint_recovery_needs_attention() {
            self.conditions
                .push(WorkspaceCondition::checkpoint_recovery_needs_attention());
        }
    }

    /// Remove the process-local condition only after verified settlement succeeds.
    pub(crate) fn dismiss_checkpoint_recovery_attention(&mut self) {
        self.conditions
            .retain(|condition| condition.code() != "checkpoint-recovery-needs-attention");
    }

    pub(crate) fn record_managed_mutation_recovery_error(
        &mut self,
        error: &crate::managed_mutation::ManagedMutationRecoveryError,
    ) {
        if !self
            .conditions
            .iter()
            .any(|condition| condition.code() == error.code())
        {
            self.conditions
                .push(condition(error.code(), error.message(), Vec::new()));
        }
    }

    /// The immutable review record for a bundle, when one has already been opened.
    #[must_use]
    pub fn review(&self, bundle: &RecordDigest) -> Option<ReviewRecord> {
        self.record_index.review(bundle).copied()
    }

    /// The sole approved envelope for a durable review bundle.
    ///
    /// Multiple approved envelopes are ambiguous even when their payloads happen to agree. A
    /// retrying publication surface must fail closed instead of selecting one by map order.
    pub(crate) fn approved_envelope(
        &self,
        bundle: &RecordDigest,
    ) -> Result<ApprovalRecord, String> {
        let mut approvals = self
            .record_index
            .approvals_for_bundle(bundle)
            .filter(|approval| approval.verdict == ReviewVerdict::Approved);
        let approval = approvals
            .next()
            .copied()
            .ok_or_else(|| "the review has no durable approval envelope".to_owned())?;
        if approvals.next().is_some() {
            return Err("the review has more than one approved envelope".to_owned());
        }
        Ok(approval)
    }

    /// Read one approval payload through its content-addressed name.
    pub(crate) fn approval_receipt(&self, approval: RecordDigest) -> Result<Vec<u8>, String> {
        self.payload_store
            .read(&CasDigest::from_bytes(*approval.as_bytes()))
            .map_err(|_| "the durable approval receipt is unavailable".to_owned())
    }

    /// A bounded, read-only projection of the current automatic card and durable review records.
    ///
    /// The complete causal closure is replayed and every payload and file byte is re-read through
    /// its verified CAS name before a bundle identifier is accepted. The current private head is
    /// projected first when it has no durable review record, so a calm card appears after a save
    /// without appending a false claim that a person opened it. Subject operations remain
    /// diagnostic detail, but the user-facing change list comes from the bundle's deterministic
    /// presentation. Missing, invalid, mismatched, or truncated durable content keeps its card
    /// visible with `content_complete: false`; an unavailable change must never look like an empty
    /// change. These values carry no signer, receipt, or append capability and cannot authorize
    /// publication.
    #[must_use]
    pub fn review_items(
        &self,
        automatic_target: Option<RecordDigest>,
    ) -> (Vec<crate::ipc::Json>, u64) {
        let bundles = self.record_index.review_bundles();
        let mut items = Vec::new();

        let candidate = automatic_target.and_then(|target| {
            let bundle = self.first_publication_review_bundle(target).ok()?;
            if self.review(&bundle).is_some() {
                return None;
            }
            let actor_public_key = self
                .record_index
                .operation(&target)
                .map(|operation| PublicKey::from_bytes(*operation.actor.as_bytes()))?;
            let opened_by = RecordDigest::from_bytes(
                *actor_public_key.actor_id::<Blake3>().digest().as_bytes(),
            );
            Some((
                bundle,
                ReviewRecord {
                    bundle,
                    subject_operation: target,
                    opened_by,
                },
            ))
        });
        if let Some((bundle, review)) = candidate {
            items.push(self.review_item(bundle, review, false));
        }

        let durable_limit = Self::MAX_REVIEW_ITEMS.saturating_sub(items.len());
        let not_listed = bundles.len().saturating_sub(durable_limit) as u64;

        for bundle in bundles.into_iter().take(durable_limit) {
            let Some(review) = self.record_index.review(&bundle) else {
                continue;
            };
            items.push(self.review_item(bundle, *review, true));
        }

        (items, not_listed)
    }

    /// Reconstruct one exact file side from a displayed review bundle.
    ///
    /// This is a read-only presentation seam. It accepts both an already recorded review and the
    /// sole current automatic candidate, but in either case recomputes the bundle and requires its
    /// exact object/path/content identities before returning bytes. The renderer never supplies a
    /// filesystem path and this method never reads the mutable presented folder.
    pub(crate) fn verified_review_artifact(
        &self,
        bundle: RecordDigest,
        target: RecordDigest,
        object: mesh_materializer::ObjectId,
        side: ReviewArtifactSide,
    ) -> Result<VerifiedReviewArtifact, String> {
        const MAX_ARTIFACT_BYTES: usize = 32 * 1024 * 1024;

        let canonical_head = if let Some(review) = self.review(&bundle) {
            if review.subject_operation != target {
                return Err("the review bundle names another saved version".to_owned());
            }
            self.canonical_head_for_review(&review)?
        } else {
            let expected = self.first_publication_review_bundle(target)?;
            if expected != bundle {
                return Err("the automatic review bundle is stale".to_owned());
            }
            self.shared_version()
                .unwrap_or(crate::publication::GENESIS_SHARED_HEAD)
        };
        let (computed, _, _) = self.publication_review(target, canonical_head, false)?;
        if RecordDigest::from_bytes(*computed.id().digest().as_bytes()) != bundle {
            return Err("the review bundle presentation changed".to_owned());
        }
        let review_object = mesh_approval::ObjectId::from_bytes(*object.as_bytes());
        let presentation = computed.presentation();
        let change = presentation
            .entries()
            .iter()
            .find(|change| change.object() == review_object)
            .ok_or_else(|| "the review bundle does not contain that object".to_owned())?;
        let (path, summary, operation) = match side {
            ReviewArtifactSide::After => (change.path_after(), change.after(), Some(target)),
            ReviewArtifactSide::Before => (
                change.path_before(),
                change.before(),
                if canonical_head == crate::publication::GENESIS_SHARED_HEAD {
                    None
                } else {
                    Some(self.review_target_for_head(canonical_head)?)
                },
            ),
        };
        let path =
            path.ok_or_else(|| "that side of the reviewed file does not exist".to_owned())?;
        let summary = summary.ok_or_else(|| "that side has no reviewed content".to_owned())?;
        let operation = operation
            .ok_or_else(|| "the first shared version has no previous file bytes".to_owned())?;
        let relative_path = path.strip_prefix('/').unwrap_or(path);
        let file = self
            .historical_workspace_file(operation, relative_path)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "the reviewed file is absent from that saved version".to_owned())?;
        if file.object != object || file.path != relative_path {
            return Err("the reviewed file identity does not match the saved version".to_owned());
        }
        if file.bytes.len() > MAX_ARTIFACT_BYTES {
            return Err("the reviewed artifact is over the 32 MiB visual-preview limit".to_owned());
        }
        let digest = mesh_types::Blake3::digest_bytes(&file.bytes);
        let expected_digest = summary
            .digest()
            .ok_or_else(|| "the reviewed artifact has no exact content digest".to_owned())?;
        if digest.as_bytes() != expected_digest.as_bytes()
            || u64::try_from(file.bytes.len()).ok() != summary.byte_length()
        {
            return Err(
                "the reconstructed artifact does not match the reviewed content".to_owned(),
            );
        }
        Ok(VerifiedReviewArtifact {
            path: path.to_owned(),
            version: summary.version(),
            digest: expected_digest,
            bytes: file.bytes,
        })
    }

    /// Read one durable review independently of the bounded overview page.
    pub(crate) fn recorded_review_item(&self, bundle: RecordDigest) -> Option<crate::ipc::Json> {
        self.review(&bundle)
            .map(|review| self.review_item(bundle, review, true))
    }

    fn review_item(
        &self,
        bundle: RecordDigest,
        review: ReviewRecord,
        recorded: bool,
    ) -> crate::ipc::Json {
        let Some(subject) = self.record_index.operation(&review.subject_operation) else {
            return review_item_json(
                bundle,
                review,
                recorded,
                None,
                ReviewItemProjection::unavailable("review-subject-missing"),
            );
        };

        let decoded = self
            .payload_store
            .read(&CasDigest::from_bytes(*subject.payload_digest.as_bytes()))
            .map_err(|_| "review-subject-payload-unavailable")
            .and_then(|bytes| {
                decode_changeset_operations(&bytes).ok_or("review-subject-payload-invalid")
            });
        let (operations, operations_not_listed, operation_failure) = match decoded {
            Ok(operations) => {
                let not_listed =
                    operations.len().saturating_sub(Self::MAX_REVIEW_OPERATIONS) as u64;
                (
                    operations
                        .iter()
                        .take(Self::MAX_REVIEW_OPERATIONS)
                        .map(operation_json)
                        .collect(),
                    not_listed,
                    None,
                )
            }
            Err(code) => (Vec::new(), 0, Some(code)),
        };

        let projection = self
            .canonical_head_for_review(&review)
            .and_then(|canonical_head| {
                self.publication_review(review.subject_operation, canonical_head, false)
            })
            .map_err(|_| "review-bundle-presentation-unavailable")
            .and_then(|(computed, verified_text, _state)| {
                let computed_id = RecordDigest::from_bytes(*computed.id().digest().as_bytes());
                if computed_id != bundle {
                    return Err("review-bundle-presentation-mismatch");
                }
                Ok((
                    computed.actor_head().to_string(),
                    computed.presentation(),
                    verified_text,
                ))
            });
        let (
            reviewed_head,
            presentation_digest,
            bundle_changes,
            bundle_changes_not_listed,
            projection_failure,
        ) = match projection {
            Ok((reviewed_head, presentation, verified_text)) => {
                let not_listed = presentation.len().saturating_sub(Self::MAX_REVIEW_CHANGES) as u64;
                (
                    Some(reviewed_head),
                    Some(presentation.digest().to_string()),
                    presentation
                        .entries()
                        .iter()
                        .take(Self::MAX_REVIEW_CHANGES)
                        .map(|change| {
                            review_change_json(change, verified_text.get(&change.object()))
                        })
                        .collect(),
                    not_listed,
                    None,
                )
            }
            Err(code) => (None, None, Vec::new(), 0, Some(code)),
        };

        review_item_json(
            bundle,
            review,
            recorded,
            Some(subject),
            ReviewItemProjection {
                reviewed_head,
                operations,
                operations_not_listed,
                presentation_digest,
                bundle_changes,
                bundle_changes_not_listed,
                unavailable_code: projection_failure.or(operation_failure),
            },
        )
    }

    /// Append one immutable record to the journal's durable boundary.
    ///
    /// Callers must refresh the workspace before answering from it: this method advances the
    /// retained index and journal but does not mutate the derived presentation fields in place.
    ///
    /// # Errors
    ///
    /// Any error advancing the retained index, appending, or syncing the record file.
    pub fn append_record(&mut self, record: &StoredRecord) -> io::Result<()> {
        let checkpoint = match record {
            StoredRecord::Operation(value) => Checkpoint {
                operations: vec![value.clone()],
                ..Checkpoint::default()
            },
            StoredRecord::Manifest(value) => Checkpoint {
                manifests: vec![value.clone()],
                ..Checkpoint::default()
            },
            StoredRecord::Peer(value) => Checkpoint {
                peers: vec![*value],
                ..Checkpoint::default()
            },
            StoredRecord::Acknowledgement(value) => Checkpoint {
                acknowledgements: vec![*value],
                ..Checkpoint::default()
            },
            StoredRecord::Review(value) => Checkpoint {
                reviews: vec![*value],
                ..Checkpoint::default()
            },
            StoredRecord::Approval(value) => Checkpoint {
                approvals: vec![*value],
                ..Checkpoint::default()
            },
            StoredRecord::ContextEntry(value) => Checkpoint {
                context_entries: vec![*value],
                ..Checkpoint::default()
            },
        };
        self.store
            .commit(&checkpoint)
            .map_err(|error| io::Error::other(error.to_string()))?;
        journal_records(&mut self.journal, [record]).map(|_| ())
    }

    /// Promote an already-verified approval receipt through this workspace's pinned CAS.
    pub(crate) fn promote_approval_receipt(&self, bytes: Vec<u8>) -> io::Result<RecordDigest> {
        self.payload_store
            .promote(bytes)
            .map(|promoted| RecordDigest::from_bytes(*promoted.digest().as_bytes()))
            .map_err(|error| io::Error::other(error.to_string()))
    }

    /// File and folder paths derived from saved ChangeSet payloads.
    #[must_use]
    pub fn entries(&self) -> &[WorkspaceEntry] {
        &self.entries
    }

    /// Regular files visible in the native folder but absent from durable workspace history.
    ///
    /// This is discovery, not an authoritative creation event: the folder fallback can miss
    /// short-lived or unreadable entries. A returned file may sit below a native-only directory
    /// that is present on disk but not durable yet, so the desktop can review the complete tree.
    /// Explicit adoption still requires those directories to become durable parent first.
    pub fn native_untracked_files(&self) -> Result<Vec<String>, ExclusionLoadFailure> {
        self.native_discovery().map(|discovery| discovery.files)
    }

    /// One metadata-only pass over the native folder for regular, directory, and unsupported
    /// entries. Unsupported links and special objects are surfaced but never followed.
    pub(crate) fn native_discovery(&self) -> Result<NativeDiscovery, ExclusionLoadFailure> {
        let exclusions = EffectiveExclusions::load(self.physical_root.as_path(), None)?;
        let durable = self
            .entries
            .iter()
            .map(|entry| entry.path().to_owned())
            .collect::<BTreeSet<_>>();
        let reserve_private_top_level = !self.has_external_private_store();
        let discovered = discover_native_entries(self, reserve_private_top_level, &exclusions);
        let files = discovered
            .files
            .into_iter()
            .filter(|path| !durable.contains(path))
            .filter(|path| {
                self.managed_discovery_target_with_exclusions(path, &exclusions)
                    .is_ok()
            })
            .collect();
        let directories = discovered
            .directories
            .into_iter()
            .filter(|path| !durable.contains(path))
            .filter(|path| {
                self.managed_discovery_target_with_exclusions(path, &exclusions)
                    .is_ok()
            })
            .collect();
        let unsupported = discovered
            .unsupported
            .into_iter()
            .filter(|entry| {
                let verdict = if reserve_private_top_level {
                    exclusions.versions_path(entry.path())
                } else {
                    exclusions.versions_presented_path(entry.path())
                };
                verdict.unwrap_or(false)
            })
            .collect();
        Ok(NativeDiscovery {
            files,
            directories,
            unsupported,
            complete: discovered.complete,
        })
    }

    /// Native directories absent from durable history, returned for complete read-only review.
    ///
    /// Mutation remains parent-first: a nested directory cannot be adopted until its immediate
    /// parent is durable. Returning the complete tree lets one reviewed queue satisfy that rule
    /// without asking the person to discover and approve one depth at a time.
    pub fn native_untracked_directories(&self) -> Result<Vec<String>, ExclusionLoadFailure> {
        self.native_discovery()
            .map(|discovery| discovery.directories)
    }

    /// Whether a zero-history workspace contains ordinary content that must be imported first.
    ///
    /// There is deliberately no adoption authority in this answer. With no materialized root,
    /// Mesh cannot assign object identities or parent directories to these native names. The
    /// caller can only use the signal to offer the existing verified import journey.
    pub(crate) fn has_unversioned_native_content(&self) -> Result<bool, ExclusionLoadFailure> {
        if self.boundary.records != 0 {
            return Ok(false);
        }
        let exclusions = EffectiveExclusions::load(self.physical_root.as_path(), None)?;
        let reserve_private_top_level = !self.has_external_private_store();
        Ok(has_discoverable_native_content(
            self.physical_root.as_path(),
            reserve_private_top_level,
            &exclusions,
        ))
    }

    /// Discoverable retained file versions for the read-only history surface.
    ///
    /// An incomplete materialization returns no history rather than identities derived from only
    /// the readable prefix. [`Self::conditions`] explains why that answer is unavailable.
    #[must_use]
    pub fn file_histories(&self) -> &[WorkspaceFileHistory] {
        &self.file_histories
    }

    /// Durable paths that disappeared from the current tree, in stable path order.
    ///
    /// These are candidates for an explicitly reviewed ordinary-folder cleanup. Merely appearing
    /// here never authorizes removal.
    #[must_use]
    pub fn retired_entries(&self) -> &[RetiredWorkspaceEntry] {
        &self.retired_entries
    }

    /// Every causally complete operation point, in deterministic causal order.
    ///
    /// Each point can be opened as a new independent workspace. It is not an in-place checkout:
    /// the current folder and its append-only history remain unchanged.
    #[must_use]
    pub fn workspace_versions(&self) -> Vec<WorkspaceVersion> {
        let records = operation_records(&self.record_index);
        let ready = self
            .record_index
            .causally_ready_operations()
            .into_iter()
            .filter_map(|id| records.get(&id))
            .map(|record| {
                mesh_materializer::AppliedChangeSet::new(
                    mesh_materializer::ChangeSetId::from_bytes(*record.id.as_bytes()),
                    record
                        .parents
                        .iter()
                        .map(|parent| {
                            mesh_materializer::ChangeSetId::from_bytes(*parent.as_bytes())
                        })
                        .collect(),
                    Vec::new(),
                )
            })
            .collect::<Vec<_>>();
        mesh_materializer::causal_order(&ready)
            .into_iter()
            .enumerate()
            .filter_map(|(index, id)| {
                let operation = RecordDigest::from_bytes(*id.as_bytes());
                let record = records.get(&operation)?;
                Some(WorkspaceVersion {
                    operation,
                    ordinal: u64::try_from(index + 1).unwrap_or(u64::MAX),
                    actor_sequence: record.actor_sequence,
                })
            })
            .collect()
    }

    fn historical_workspace_materialization(
        &self,
        operation: RecordDigest,
    ) -> Result<(mesh_materializer::WorkspaceState, Vec<WorkspaceEntry>), WorkspaceVersionFailure>
    {
        self.ensure_physical_root()
            .map_err(|error| WorkspaceVersionFailure::RetainedContent(error.to_string()))?;
        let all = operation_records(&self.record_index);
        if !all.contains_key(&operation) {
            return Err(WorkspaceVersionFailure::UnknownOperation);
        }
        let mut selected = BTreeMap::new();
        let mut pending = vec![operation];
        while let Some(id) = pending.pop() {
            if selected.contains_key(&id) {
                continue;
            }
            let record = all
                .get(&id)
                .ok_or(WorkspaceVersionFailure::UnknownOperation)?
                .clone();
            pending.extend(record.parents.iter().copied());
            selected.insert(id, record);
        }
        let materialized = materialize_operation_records(selected, &self.payload_store);
        if !materialized.complete {
            return Err(WorkspaceVersionFailure::IncompleteHistory(
                materialized
                    .conditions
                    .iter()
                    .map(WorkspaceCondition::code)
                    .collect(),
            ));
        }
        let state = materialized.state.ok_or_else(|| {
            WorkspaceVersionFailure::IncompleteHistory(vec!["workspace-history-empty"])
        })?;
        Ok((state, materialized.entries))
    }

    /// Reconstruct one file at an exact historical workspace point without hydrating every other
    /// file in that point. Pull-back uses this to compare an ordinary destination with the genesis
    /// import while keeping whole-tree preview cost proportional to the files being inspected.
    pub(crate) fn historical_workspace_file(
        &self,
        operation: RecordDigest,
        relative_path: &str,
    ) -> Result<Option<HistoricalWorkspaceFile>, WorkspaceVersionFailure> {
        let (state, entries) = self.historical_workspace_materialization(operation)?;
        let Some(entry) = entries
            .into_iter()
            .find(|entry| entry.path == relative_path && entry.entry_type == "file")
        else {
            return Ok(None);
        };
        let object = state.objects().iter().find_map(|(id, object)| {
            state.path_of(*id).and_then(|path| {
                let rendered = path
                    .iter()
                    .map(mesh_operations::NormalizedName::as_str)
                    .collect::<Vec<_>>()
                    .join("/");
                (rendered == entry.path).then_some((*id, object))
            })
        });
        let Some((object_id, object)) = object else {
            return Err(WorkspaceVersionFailure::IncompleteHistory(vec![
                "materialization-incomplete",
            ]));
        };
        let version = object.current_version().ok_or_else(|| {
            WorkspaceVersionFailure::IncompleteHistory(vec!["materialization-incomplete"])
        })?;
        let file_version = state.file_version(version).ok_or_else(|| {
            WorkspaceVersionFailure::IncompleteHistory(vec!["materialization-incomplete"])
        })?;
        let manifest_id = RecordDigest::from_bytes(*file_version.manifest_id().as_bytes());
        let manifest = self
            .record_index
            .manifest(&manifest_id)
            .ok_or_else(|| WorkspaceVersionFailure::MissingManifest(manifest_id.to_string()))?;
        Ok(Some(HistoricalWorkspaceFile {
            object: object_id,
            path: entry.path,
            bytes: reconstruct_manifest(&self.payload_store, manifest)?,
            executable: file_version.portable_metadata().is_executable(),
        }))
    }

    /// Verify one exact causal point while retaining only bounded per-chunk bytes.
    ///
    /// The same metadata plan drives both the picker and the native-folder writer. It retains no
    /// file body: opening a large workspace re-reads and verifies one CAS chunk at a time instead
    /// of turning the selected point into a workspace-sized memory allocation.
    pub(crate) fn historical_workspace_preview(
        &self,
        operation: RecordDigest,
    ) -> Result<HistoricalWorkspacePreview, WorkspaceVersionFailure> {
        let (state, entries) = self.historical_workspace_materialization(operation)?;
        let mut objects_by_path = BTreeMap::new();
        for (id, object) in state.objects() {
            // Retained deleted or unlinked objects belong to history, not to this saved tree.
            // Match visible-entry materialization; every required entry is still resolved below.
            if *id == state.root() || object.is_deleted() {
                continue;
            }
            record_historical_path_resolution();
            let Some(path) = state.path_of(*id) else {
                continue;
            };
            let rendered = path
                .iter()
                .map(mesh_operations::NormalizedName::as_str)
                .collect::<Vec<_>>()
                .join("/");
            if objects_by_path.insert(rendered, (*id, object)).is_some() {
                return Err(WorkspaceVersionFailure::IncompleteHistory(vec![
                    "materialization-incomplete",
                ]));
            }
        }
        let mut directories = Vec::new();
        let mut files = Vec::new();
        for entry in entries {
            let object = objects_by_path.get(&entry.path).copied().ok_or_else(|| {
                WorkspaceVersionFailure::IncompleteHistory(vec!["materialization-incomplete"])
            })?;
            match entry.entry_type {
                "folder" => directories.push(HistoricalWorkspaceDirectory {
                    object: object.0,
                    path: entry.path,
                }),
                "file" => {
                    let version = object.1.current_version().ok_or_else(|| {
                        WorkspaceVersionFailure::IncompleteHistory(vec![
                            "materialization-incomplete",
                        ])
                    })?;
                    let file_version = state.file_version(version).ok_or_else(|| {
                        WorkspaceVersionFailure::IncompleteHistory(vec![
                            "materialization-incomplete",
                        ])
                    })?;
                    let manifest_id =
                        RecordDigest::from_bytes(*file_version.manifest_id().as_bytes());
                    let manifest = self.record_index.manifest(&manifest_id).ok_or_else(|| {
                        WorkspaceVersionFailure::MissingManifest(manifest_id.to_string())
                    })?;
                    verify_manifest(&self.payload_store, manifest)?;
                    files.push(HistoricalWorkspacePreviewFile {
                        object: object.0,
                        path: entry.path,
                        manifest_id,
                        byte_length: manifest.byte_length,
                        content_digest: manifest.content_digest,
                        executable: file_version.portable_metadata().is_executable(),
                    });
                }
                _ => unreachable!("workspace entries have a closed type vocabulary"),
            }
        }
        directories.sort_by(|left, right| {
            left.path
                .matches('/')
                .count()
                .cmp(&right.path.matches('/').count())
                .then_with(|| left.path.cmp(&right.path))
        });
        files.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(HistoricalWorkspacePreview {
            operation,
            directories,
            files,
        })
    }

    /// Stream one file from a verified historical plan into an already-confined output.
    ///
    /// The manifest is looked up again rather than trusted from the plan. Every chunk name,
    /// length, offset and the final content digest are rechecked while the bytes are written, so
    /// corruption after preview cannot publish a plausible native checkout.
    pub(crate) fn write_historical_workspace_file(
        &self,
        file: &HistoricalWorkspacePreviewFile,
        output: &mut File,
    ) -> Result<(), HistoricalWorkspaceWriteFailure> {
        let manifest = self
            .record_index
            .manifest(&file.manifest_id)
            .ok_or_else(|| WorkspaceVersionFailure::MissingManifest(file.manifest_id.to_string()))
            .map_err(HistoricalWorkspaceWriteFailure::Retained)?;
        if manifest.byte_length != file.byte_length
            || manifest.content_digest != file.content_digest
        {
            return Err(HistoricalWorkspaceWriteFailure::Retained(
                WorkspaceVersionFailure::RetainedContent(
                    "a retained manifest changed after the workspace point was verified".to_owned(),
                ),
            ));
        }
        let mut output_failure = None;
        let result = visit_verified_manifest_chunks(&self.payload_store, manifest, |chunk| {
            output.write_all(chunk).map_err(|error| {
                output_failure = Some(error);
                WorkspaceVersionFailure::RetainedContent(
                    "the native workspace file could not be written".to_owned(),
                )
            })
        });
        match (result, output_failure) {
            (_, Some(error)) => Err(HistoricalWorkspaceWriteFailure::Output(error)),
            (Err(failure), None) => Err(HistoricalWorkspaceWriteFailure::Retained(failure)),
            (Ok(()), None) => Ok(()),
        }
    }

    /// Identify the only workspace point that can truthfully serve as a visible-change basis.
    ///
    /// A one-parent save follows that exact point. A root save has no predecessor. A merge has a
    /// set of causal predecessors rather than one prior workspace, so callers must describe its
    /// complete combined contents instead of comparing it with an arbitrary total-order neighbor.
    pub(crate) fn workspace_version_change_basis(
        &self,
        operation: RecordDigest,
    ) -> Result<WorkspaceVersionChangeBasis, WorkspaceVersionFailure> {
        let record = self
            .record_index
            .operation(&operation)
            .ok_or(WorkspaceVersionFailure::UnknownOperation)?;
        match record.parents.as_slice() {
            [] => Ok(WorkspaceVersionChangeBasis::Initial),
            [parent] if self.record_index.operation(parent).is_some() => {
                Ok(WorkspaceVersionChangeBasis::Previous(*parent))
            }
            [_] => Err(WorkspaceVersionFailure::UnknownOperation),
            _ => Ok(WorkspaceVersionChangeBasis::CombinedHistory),
        }
    }

    /// Reconstruct the current durable bytes for one materialized file.
    ///
    /// This reads immutable manifest/chunk truth, not the mutable native working copy. Callers that
    /// export these bytes must separately prove the working copy still agrees so a person cannot
    /// accidentally publish an older version while looking at an unsaved edit.
    pub(crate) fn current_durable_file(
        &self,
        relative_path: &str,
    ) -> Result<(String, HistoricalWorkspaceFile), WorkspaceVersionFailure> {
        self.ensure_physical_root()
            .map_err(|error| WorkspaceVersionFailure::RetainedContent(error.to_string()))?;
        let history = self
            .file_histories
            .iter()
            .find(|history| history.path == relative_path)
            .ok_or_else(|| {
                WorkspaceVersionFailure::IncompleteHistory(vec!["managed-file-not-materialized"])
            })?;
        let current = history.current.ok_or_else(|| {
            WorkspaceVersionFailure::IncompleteHistory(vec!["managed-file-version-missing"])
        })?;
        let manifest_id = RecordDigest::from_bytes(*current.manifest.as_bytes());
        let manifest = self
            .record_index
            .manifest(&manifest_id)
            .ok_or_else(|| WorkspaceVersionFailure::MissingManifest(manifest_id.to_string()))?;
        let metadata = self
            .materialized_state
            .as_ref()
            .and_then(|state| state.file_version(current.version))
            .ok_or_else(|| {
                WorkspaceVersionFailure::IncompleteHistory(vec!["managed-file-version-missing"])
            })?
            .portable_metadata();
        Ok((
            current.version.to_string(),
            HistoricalWorkspaceFile {
                object: history.object,
                path: relative_path.to_owned(),
                bytes: reconstruct_manifest(&self.payload_store, manifest)?,
                executable: metadata.is_executable(),
            },
        ))
    }

    /// Reconstruct bounded UTF-8 text from the current durable file version.
    ///
    /// The mutable native file is intentionally not consulted. This gives a caller an exact
    /// saved baseline for a working-copy comparison while refusing to allocate an unbounded
    /// retained body. Binary or over-limit durable content returns `None` after its manifest has
    /// been resolved, without reconstructing the payload.
    pub(crate) fn current_durable_text(
        &self,
        relative_path: &str,
        byte_limit: usize,
    ) -> Result<Option<String>, WorkspaceVersionFailure> {
        self.ensure_physical_root()
            .map_err(|error| WorkspaceVersionFailure::RetainedContent(error.to_string()))?;
        let history = self
            .file_histories
            .iter()
            .find(|history| history.path == relative_path)
            .ok_or_else(|| {
                WorkspaceVersionFailure::IncompleteHistory(vec!["managed-file-not-materialized"])
            })?;
        let current = history.current.ok_or_else(|| {
            WorkspaceVersionFailure::IncompleteHistory(vec!["managed-file-version-missing"])
        })?;
        let manifest_id = RecordDigest::from_bytes(*current.manifest.as_bytes());
        let manifest = self
            .record_index
            .manifest(&manifest_id)
            .ok_or_else(|| WorkspaceVersionFailure::MissingManifest(manifest_id.to_string()))?;
        if manifest.byte_length > u64::try_from(byte_limit).unwrap_or(u64::MAX) {
            return Ok(None);
        }
        Ok(String::from_utf8(reconstruct_manifest(&self.payload_store, manifest)?).ok())
    }

    /// Identify retained versions whose exact bytes and portable executable bit still occupy an
    /// ordinary destination.
    ///
    /// A match is not replacement authority by itself. The export boundary combines each match
    /// with an immutable Pull-back receipt bound to the current destination inode. This helper
    /// merely avoids trusting a caller-supplied version identity while checking that receipt.
    pub(crate) fn retained_durable_file_versions_matching(
        &self,
        relative_path: &str,
        bytes: &[u8],
        executable: bool,
    ) -> Result<Vec<String>, WorkspaceVersionFailure> {
        self.ensure_physical_root()
            .map_err(|error| WorkspaceVersionFailure::RetainedContent(error.to_string()))?;
        let history = self
            .file_histories
            .iter()
            .find(|history| history.path == relative_path)
            .ok_or_else(|| {
                WorkspaceVersionFailure::IncompleteHistory(vec!["managed-file-not-materialized"])
            })?;
        let content_digest = RecordDigest::from_bytes(*Blake3::digest_bytes(bytes).as_bytes());
        let byte_length = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        let mut matches = Vec::new();
        for retained in &history.retained {
            let metadata = self
                .materialized_state
                .as_ref()
                .and_then(|state| state.file_version(retained.version))
                .ok_or_else(|| {
                    WorkspaceVersionFailure::IncompleteHistory(vec!["managed-file-version-missing"])
                })?
                .portable_metadata();
            if metadata.is_executable() != executable {
                continue;
            }
            let manifest_id = RecordDigest::from_bytes(*retained.manifest.as_bytes());
            let manifest = self
                .record_index
                .manifest(&manifest_id)
                .ok_or_else(|| WorkspaceVersionFailure::MissingManifest(manifest_id.to_string()))?;
            if manifest.content_digest == content_digest && manifest.byte_length == byte_length {
                matches.push(retained.version.to_string());
            }
        }
        matches.sort();
        matches.dedup();
        Ok(matches)
    }

    /// Reconstruct the last durable bytes that occupied a path absent from the current tree.
    pub(crate) fn retired_durable_file(
        &self,
        relative_path: &str,
    ) -> Result<(String, HistoricalWorkspaceFile), WorkspaceVersionFailure> {
        self.ensure_physical_root()
            .map_err(|error| WorkspaceVersionFailure::RetainedContent(error.to_string()))?;
        let retired = self
            .retired_entries
            .iter()
            .find(|entry| entry.path == relative_path && entry.entry_type == "file")
            .ok_or_else(|| {
                WorkspaceVersionFailure::IncompleteHistory(vec!["retired-file-not-found"])
            })?;
        let version = retired.last_file_version.ok_or_else(|| {
            WorkspaceVersionFailure::IncompleteHistory(vec!["retired-file-version-missing"])
        })?;
        let manifest_id = RecordDigest::from_bytes(*version.manifest.as_bytes());
        let manifest = self
            .record_index
            .manifest(&manifest_id)
            .ok_or_else(|| WorkspaceVersionFailure::MissingManifest(manifest_id.to_string()))?;
        Ok((
            version.version.to_string(),
            HistoricalWorkspaceFile {
                object: retired.object,
                path: relative_path.to_owned(),
                bytes: reconstruct_manifest(&self.payload_store, manifest)?,
                executable: retired.last_executable.ok_or_else(|| {
                    WorkspaceVersionFailure::IncompleteHistory(vec![
                        "retired-file-metadata-missing",
                    ])
                })?,
            },
        ))
    }

    /// Whether a manifest was present in the journal at the last open or refresh. The mutable
    /// checkpoint index can be ahead of the journal after an interrupted save and is insufficient
    /// evidence for reusing content without appending its manifest again.
    pub(crate) fn has_journaled_manifest(&self, id: RecordDigest) -> bool {
        self.record_index.manifest(&id).is_some()
    }

    /// Exact retained manifest record used by the native managed-folder restore boundary.
    pub fn manifest_record(
        &self,
        id: mesh_materializer::ManifestId,
    ) -> Option<&mesh_store::ManifestRecord> {
        self.store
            .index()
            .manifest(&mesh_store::RecordDigest::from_bytes(*id.as_bytes()))
    }

    /// Portable metadata carried by one complete materialized file version.
    #[allow(dead_code)]
    pub(crate) fn file_version_metadata(
        &self,
        id: mesh_materializer::VersionId,
    ) -> Option<PortableMetadata> {
        self.materialized_state
            .as_ref()?
            .file_version(id)
            .map(mesh_materializer::FileVersion::portable_metadata)
    }

    /// Exact workspace identity agreed by every causally ready journal operation.
    pub(crate) fn journal_workspace_id(&self) -> Result<WorkspaceId, String> {
        self.ensure_physical_root()
            .map_err(|error| error.to_string())?;
        self.workspace_id_for_operations(&self.record_index.causally_ready_operations())
    }

    fn workspace_id_for_operations(&self, ready: &[RecordDigest]) -> Result<WorkspaceId, String> {
        let cas = &self.payload_store;
        let mut workspace_ids = BTreeSet::new();
        for id in ready {
            let record = self
                .store
                .index()
                .operation(id)
                .ok_or_else(|| "a ready operation disappeared from the index".to_owned())?;
            let payload = cas
                .read(&CasDigest::from_bytes(*record.payload_digest.as_bytes()))
                .map_err(|error| format!("an authoring payload could not be read: {error}"))?;
            let fields = decode_changeset_fields(&payload)
                .ok_or_else(|| "an authoring payload is not a verified ChangeSet".to_owned())?;
            let workspace = match fields.first() {
                Some(CanonicalValue::Bytes(bytes)) if bytes.len() == WorkspaceId::WIDTH => {
                    let mut exact = [0_u8; WorkspaceId::WIDTH];
                    exact.copy_from_slice(bytes);
                    WorkspaceId::from_bytes(exact)
                }
                _ => return Err("a ChangeSet has no exact workspace identifier".to_owned()),
            };
            workspace_ids.insert(workspace);
        }
        if workspace_ids.len() != 1 {
            return Err("durable ChangeSets disagree about the workspace identifier".to_owned());
        }
        Ok(*workspace_ids
            .first()
            .expect("one workspace identifier was checked above"))
    }

    /// Derive the next authenticated local authoring context from durable workspace truth.
    ///
    /// The supplied public key is the actor identity. A new desktop process may therefore start a
    /// new actor without persisting or exporting a secret; an existing process advances its own
    /// actor sequence. Every causally ready tip is included, and every readable payload must agree
    /// on the workspace identifier before a new record can be authored.
    pub(crate) fn managed_authoring_basis(
        &self,
        actor_public_key: PublicKey,
    ) -> Result<ManagedAuthoringBasis, String> {
        self.ensure_physical_root()
            .map_err(|error| format!("the workspace directory changed: {error}"))?;
        if self.managed_mutation_recovery_needed() {
            return Err(
                "an interrupted local file change needs attention before more changes are authored"
                    .to_owned(),
            );
        }
        let ready = self.record_index.causally_ready_operations();
        if ready.is_empty() {
            return Err("the workspace has no causally ready operation".to_owned());
        }

        let workspace_id = self.workspace_id_for_operations(&ready)?;

        let ready_set = ready.iter().copied().collect::<BTreeSet<_>>();
        let referenced = ready
            .iter()
            .filter_map(|id| self.record_index.operation(id))
            .flat_map(|operation| operation.parents.iter().copied())
            .filter(|parent| ready_set.contains(parent))
            .collect::<BTreeSet<_>>();
        let mut tips = ready
            .iter()
            .copied()
            .filter(|id| !referenced.contains(id))
            .collect::<Vec<_>>();
        tips.sort();
        let first = tips
            .first()
            .copied()
            .ok_or_else(|| "the ready operation graph has no causal tip".to_owned())?;
        let causal_parents = CausalParents::after(
            ChangeSetId::from_bytes(*first.as_bytes()),
            tips.into_iter()
                .skip(1)
                .map(|id| ChangeSetId::from_bytes(*id.as_bytes()))
                .collect(),
        );

        let base_head = HeadId::parse(
            self.private_version
                .version()
                .ok_or_else(|| "the workspace has no derived private head".to_owned())?,
        )
        .map_err(|error| format!("the derived private head is malformed: {error}"))?;

        self.authoring_basis_for_history(
            actor_public_key,
            workspace_id,
            &ready,
            causal_parents,
            base_head,
        )
    }

    fn authoring_basis_for_history(
        &self,
        actor_public_key: PublicKey,
        workspace_id: WorkspaceId,
        ready: &[RecordDigest],
        causal_parents: CausalParents,
        base_head: HeadId,
    ) -> Result<ManagedAuthoringBasis, String> {
        let actor = RecordDigest::from_bytes(*actor_public_key.as_bytes());
        let actor_head = self.record_index.actor_head(&actor);
        let actor_sequence = match actor_head {
            Some(head) => head
                .actor_sequence
                .checked_add(1)
                .filter(|next| *next > head.actor_sequence)
                .map(ActorSequence::new)
                .ok_or_else(|| "the local actor sequence is exhausted".to_owned())?,
            None => ActorSequence::FIRST,
        };
        let session_id = actor_head.map_or_else(
            || {
                let mut bytes = Vec::with_capacity(80);
                bytes.extend_from_slice(b"mesh.local-desktop-session/1\0");
                bytes.extend_from_slice(workspace_id.as_bytes());
                bytes.extend_from_slice(actor_public_key.as_bytes());
                let digest = Blake3::digest_bytes(&bytes);
                let mut session = [0_u8; 16];
                session.copy_from_slice(&digest.as_bytes()[..16]);
                SessionId::from_bytes(session)
            },
            |head| SessionId::from_bytes(*head.session.as_bytes()),
        );

        let latest_clock = ready
            .iter()
            .filter_map(|id| self.record_index.operation(id))
            .max_by_key(|operation| (operation.hlc_millis, operation.hlc_counter, operation.id))
            .expect("the ready set is non-empty");
        let logical = u32::try_from(latest_clock.hlc_counter).map_err(|_| {
            "the durable HLC counter cannot be represented by the protocol".to_owned()
        })?;
        let (physical_millis, logical) = match logical.checked_add(1) {
            Some(next) => (latest_clock.hlc_millis, next),
            None => (
                latest_clock
                    .hlc_millis
                    .checked_add(1)
                    .ok_or_else(|| "the durable HLC is exhausted".to_owned())?,
                0,
            ),
        };
        Ok(ManagedAuthoringBasis {
            workspace_id,
            actor_id: ActorId::from_bytes(*actor_public_key.as_bytes()),
            session_id,
            actor_sequence,
            causal_parents,
            base_head,
            policy_epoch: PolicyEpoch::new(
                self.store
                    .index()
                    .highest_policy_epoch()
                    .ok_or_else(|| "the workspace has no policy epoch".to_owned())?,
            ),
            hybrid_logical_time: Hlc::new(physical_millis, logical),
        })
    }

    /// Check a native proposal using only one exact saved predecessor and its causal history.
    ///
    /// The current policy epoch still applies. An actor whose latest durable operation is outside
    /// the selected history must use a separate native identity, rather than fork its sequence or
    /// silently pull unrelated work into the proposal. The returned plan is read-only. A future
    /// writer must reopen the current journal under custody, then rederive and compare the plan
    /// immediately before append. This read does not reserve an actor sequence or policy epoch.
    ///
    /// # Errors
    /// Refuses incomplete history, changed physical identity, interrupted mutations, exhausted
    /// clocks/sequences, an actor outside the selected ancestry, or invalid proposed operations.
    pub fn prepare_historical_operations(
        &self,
        target: RecordDigest,
        actor_public_key: PublicKey,
        operations: &[Operation],
    ) -> Result<HistoricalOperationPlan, String> {
        if operations.is_empty() || operations.len() > 100_000 {
            return Err("historical operation plan is empty or exceeds its limit".into());
        }
        let basis = self.historical_authoring_basis(target, actor_public_key)?;
        let (mut state, _) = self
            .historical_workspace_materialization(target)
            .map_err(|error| format!("historical materialization is unavailable: {error:?}"))?;
        let validation_change = mesh_materializer::ChangeSetId::from_bytes([0xA5; 32]);
        for operation in operations {
            mesh_materializer::apply_operation(&mut state, validation_change, operation)
                .map_err(|error| format!("historical operation was refused: {error}"))?;
        }
        self.ensure_physical_root()
            .map_err(|error| error.to_string())?;
        Ok(HistoricalOperationPlan {
            target,
            basis,
            operations: operations.to_vec(),
        })
    }

    pub(crate) fn historical_authoring_basis(
        &self,
        target: RecordDigest,
        actor_public_key: PublicKey,
    ) -> Result<ManagedAuthoringBasis, String> {
        self.ensure_physical_root()
            .map_err(|error| error.to_string())?;
        if self.managed_mutation_recovery_needed() {
            return Err("an interrupted local file change needs reconciliation".into());
        }
        let ready = self.record_index.causally_ready_operations();
        if !ready.contains(&target) {
            return Err("historical predecessor is not causally complete".into());
        }
        let selected = causal_operation_closure(&operation_records(&self.record_index), target)?;
        if self
            .record_index
            .actor_head(&RecordDigest::from_bytes(*actor_public_key.as_bytes()))
            .is_some_and(|head| !selected.contains_key(&head.id))
        {
            return Err("the authoring actor has advanced outside this saved history".into());
        }
        let selected_ids = selected.keys().copied().collect::<Vec<_>>();
        let workspace = self.workspace_id_for_operations(&selected_ids)?;
        let head = review_head_for_records(&selected)?;
        self.authoring_basis_for_history(
            actor_public_key,
            workspace,
            &selected_ids,
            CausalParents::after(ChangeSetId::from_bytes(*target.as_bytes()), Vec::new()),
            HeadId::from_bytes(*head.as_bytes()),
        )
    }

    /// Exact linear ancestry, used only by the native observation cursor. No newest-tip inference.
    pub(crate) fn linear_history(
        &self,
        target: Option<RecordDigest>,
    ) -> Result<Vec<RecordDigest>, String> {
        let Some(mut current) = target else {
            return Ok(Vec::new());
        };
        let ready = self
            .record_index
            .causally_ready_operations()
            .into_iter()
            .collect::<BTreeSet<_>>();
        let mut seen = BTreeSet::new();
        let mut history = Vec::new();
        loop {
            if !ready.contains(&current) || !seen.insert(current) {
                return Err("capture ancestry is incomplete".into());
            }
            history.push(current);
            let record = self
                .record_index
                .operation(&current)
                .ok_or("capture record is missing")?;
            match record.parents.as_slice() {
                [] => break,
                [parent] => current = *parent,
                _ => return Err("capture ancestry is not a single observation line".into()),
            }
        }
        history.reverse();
        Ok(history)
    }

    pub(crate) fn historical_capture_entries(
        &self,
        target: RecordDigest,
    ) -> Result<BTreeMap<String, HistoricalCaptureEntry>, String> {
        let (state, entries) = self
            .historical_workspace_materialization(target)
            .map_err(|error| error.to_string())?;
        entries
            .into_iter()
            .map(|entry| {
                let mut components = managed_path_components(&entry.path, false)?;
                let name = components.pop().ok_or("capture path is empty")?;
                let parent_id = resolve_directory(&state, &components)?;
                let binding = state
                    .directory(parent_id)
                    .and_then(|dir| dir.entry(&name))
                    .ok_or("capture binding is missing")?;
                let object = state
                    .object(binding.object_id())
                    .ok_or("capture object is missing")?;
                let is_directory = object.kind() == mesh_materializer::ObjectKind::Directory;
                let file = if is_directory {
                    None
                } else {
                    let version = object
                        .current_version()
                        .ok_or("capture file version is missing")?;
                    let value = state
                        .file_version(version)
                        .ok_or("capture file metadata is missing")?;
                    let manifest = self
                        .record_index
                        .manifest(&RecordDigest::from_bytes(*value.manifest_id().as_bytes()))
                        .ok_or("capture manifest is missing")?;
                    Some((
                        version,
                        manifest.content_digest,
                        value.portable_metadata().is_executable(),
                    ))
                };
                Ok((
                    entry.path,
                    HistoricalCaptureEntry {
                        binding: ManagedEntryBasis {
                            object_id: binding.object_id(),
                            parent_id,
                            name,
                            is_directory,
                        },
                        file,
                    },
                ))
            })
            .collect()
    }

    pub(crate) fn managed_checkpoint_basis(
        &self,
        relative_path: &str,
        actor_public_key: PublicKey,
    ) -> Result<ManagedCheckpointBasis, String> {
        let history = self
            .file_histories
            .iter()
            .find(|history| history.path == relative_path)
            .ok_or_else(|| "the managed path is absent from complete file history".to_owned())?;
        let current = history
            .current
            .ok_or_else(|| "the managed file has no current durable version".to_owned())?;
        let basis = self.managed_authoring_basis(actor_public_key)?;
        let parent_metadata = self
            .materialized_state
            .as_ref()
            .and_then(|state| state.file_version(current.version))
            .ok_or_else(|| "the current managed file version is not materialized".to_owned())?
            .portable_metadata();
        Ok(ManagedCheckpointBasis {
            workspace_id: basis.workspace_id,
            actor_id: basis.actor_id,
            session_id: basis.session_id,
            actor_sequence: basis.actor_sequence,
            causal_parents: basis.causal_parents,
            base_head: basis.base_head,
            policy_epoch: basis.policy_epoch,
            hybrid_logical_time: basis.hybrid_logical_time,
            object_id: ObjectId::from_bytes(*history.object.as_bytes()),
            parent_version: VersionId::from_bytes(*current.version.as_bytes()),
            parent_metadata,
        })
    }

    pub(crate) fn managed_create_target(
        &self,
        relative_path: &str,
    ) -> Result<ManagedTargetBasis, String> {
        let exclusions = EffectiveExclusions::load(self.physical_root.as_path(), None)
            .map_err(|error| format!("workspace ignore rules are unavailable: {error}"))?;
        self.managed_create_target_with_exclusions(relative_path, &exclusions)
    }

    /// Validate one native-only path for read-only review before all of its parents are durable.
    ///
    /// Mutation still goes through [`Self::managed_create_target`], which requires the immediate
    /// parent in the materialized state. This weaker seam only lets the desktop review an entire
    /// agent-created tree before admitting its directories parent first.
    pub(crate) fn managed_discovery_target(&self, relative_path: &str) -> Result<(), String> {
        let exclusions = EffectiveExclusions::load(self.physical_root.as_path(), None)
            .map_err(|error| format!("workspace ignore rules are unavailable: {error}"))?;
        self.managed_discovery_target_with_exclusions(relative_path, &exclusions)
    }

    fn managed_discovery_target_with_exclusions(
        &self,
        relative_path: &str,
        exclusions: &EffectiveExclusions,
    ) -> Result<(), String> {
        let reserve_private_top_level = !self.has_external_private_store();
        let components = managed_path_components(relative_path, reserve_private_top_level)?;
        let versioned = if reserve_private_top_level {
            exclusions.versions_path(relative_path)
        } else {
            exclusions.versions_presented_path(relative_path)
        };
        if !versioned.map_err(|error| error.to_string())? {
            return Err("the managed path is excluded by workspace rules".to_owned());
        }
        self.materialized_state.as_ref().ok_or_else(|| {
            "managed names and folders are not completely materialized".to_owned()
        })?;
        if self
            .entries
            .iter()
            .any(|entry| entry.path() == relative_path)
        {
            return Err("the managed target name already exists".to_owned());
        }
        for depth in 1..components.len() {
            let ancestor = components[..depth]
                .iter()
                .map(NormalizedName::as_str)
                .collect::<Vec<_>>()
                .join("/");
            if self
                .entries
                .iter()
                .any(|entry| entry.path() == ancestor && entry.entry_type() != "folder")
            {
                return Err("a managed path ancestor is not a directory".to_owned());
            }
        }
        Ok(())
    }

    fn managed_create_target_with_exclusions(
        &self,
        relative_path: &str,
        exclusions: &EffectiveExclusions,
    ) -> Result<ManagedTargetBasis, String> {
        let reserve_private_top_level = !self.has_external_private_store();
        let mut components = managed_path_components(relative_path, reserve_private_top_level)?;
        let versioned = if reserve_private_top_level {
            exclusions.versions_path(relative_path)
        } else {
            exclusions.versions_presented_path(relative_path)
        };
        if !versioned.map_err(|error| error.to_string())? {
            return Err("the managed path is excluded by workspace rules".to_owned());
        }
        let name = components
            .pop()
            .ok_or_else(|| "the managed path has no final name".to_owned())?;
        let state = self.materialized_state.as_ref().ok_or_else(|| {
            "managed names and folders are not completely materialized".to_owned()
        })?;
        let parent_id = resolve_directory(state, &components)?;
        if state
            .directory(parent_id)
            .and_then(|directory| directory.entry(&name))
            .is_some()
        {
            return Err("the managed target name already exists".to_owned());
        }
        Ok(ManagedTargetBasis { parent_id, name })
    }

    pub(crate) fn managed_entry_basis(
        &self,
        relative_path: &str,
    ) -> Result<ManagedEntryBasis, String> {
        let mut components =
            managed_path_components(relative_path, !self.has_external_private_store())?;
        let name = components
            .pop()
            .ok_or_else(|| "the managed path has no final name".to_owned())?;
        let state = self.materialized_state.as_ref().ok_or_else(|| {
            "managed names and folders are not completely materialized".to_owned()
        })?;
        let parent_id = resolve_directory(state, &components)?;
        let entry = state
            .directory(parent_id)
            .and_then(|directory| directory.entry(&name))
            .ok_or_else(|| "the managed entry does not exist in durable history".to_owned())?;
        let object = state
            .object(entry.object_id())
            .ok_or_else(|| "the managed entry names an unknown object".to_owned())?;
        Ok(ManagedEntryBasis {
            object_id: entry.object_id(),
            parent_id,
            name,
            is_directory: object.kind() == mesh_materializer::ObjectKind::Directory,
        })
    }

    pub(crate) fn validate_managed_operations(
        &self,
        operations: &[Operation],
    ) -> Result<(), String> {
        if operations.is_empty() {
            return Err("the managed operation set is empty".to_owned());
        }
        let mut state = self.materialized_state.clone().ok_or_else(|| {
            "managed names and folders are not completely materialized".to_owned()
        })?;
        let change = mesh_materializer::ChangeSetId::from_bytes([0xA5; 32]);
        for operation in operations {
            mesh_materializer::apply_operation(&mut state, change, operation)
                .map_err(|error| format!("the managed operation was refused: {error}"))?;
        }
        Ok(())
    }

    /// Preview an exact append-only file restore without writing or authorizing anything.
    ///
    /// The projection is available only when every saved operation payload contributed to the
    /// recovered materialized state. It deliberately returns owner-vocabulary operations rather
    /// than a signed ChangeSet: execution still needs a caller-owned identity, policy decision,
    /// causal parents, and the existing durable append path.
    ///
    /// # Errors
    ///
    /// [`RestorePreviewFailure`] when recovery is partial, the exact planner refuses the source or
    /// target, or replaying a checked plan unexpectedly diverges.
    pub fn preview_file_restore(
        &self,
        object: mesh_materializer::ObjectId,
        target: mesh_materializer::VersionId,
    ) -> Result<FileRestorePreview, RestorePreviewFailure> {
        let state = self.materialized_state.as_ref().ok_or_else(|| {
            let mut conditions: Vec<&'static str> = self
                .conditions
                .iter()
                .map(WorkspaceCondition::code)
                .collect();
            if conditions.is_empty() {
                conditions.push("workspace-history-empty");
            }
            RestorePreviewFailure::IncompleteProjection { conditions }
        })?;
        let plan = mesh_materializer::plan_file_restore(state, object, target)
            .map_err(RestorePreviewFailure::Refused)?;
        let current = version_identity(state, plan.from_version());
        let target = version_identity(state, plan.to_version());

        let mut restored = state.clone();
        let check_id = mesh_materializer::ChangeSetId::from_bytes([0xfe; 32]);
        for (index, operation) in plan.operations().iter().enumerate() {
            mesh_materializer::apply_operation(&mut restored, check_id, operation).map_err(
                |rejection| RestorePreviewFailure::InconsistentPlan { index, rejection },
            )?;
        }
        let undo_possible = plan.undo(&restored).is_ok();

        Ok(FileRestorePreview {
            object,
            current,
            target,
            operations: plan.operations().to_vec(),
            undo_possible,
        })
    }

    /// Recoverable conditions that made the names-and-folders answer partial or unavailable.
    #[must_use]
    pub fn conditions(&self) -> &[WorkspaceCondition] {
        &self.conditions
    }

    /// Whether every saved change contributed to a complete names-and-folders answer.
    #[must_use]
    pub const fn names_answered(&self) -> bool {
        self.names_answered
    }

    /// What this build cannot answer about a workspace, and why.
    ///
    /// Returned on the wire so a user interface can grey an affordance out with a reason rather
    /// than render an empty list that looks like an answer. Each entry is `(subject, reason)`.
    #[must_use]
    pub fn not_yet(&self) -> Vec<(&'static str, &'static str)> {
        let mut entries = if self.shared_version.is_answered() {
            Vec::new()
        } else {
            NOT_YET.to_vec()
        };
        if !self.names_answered {
            entries.push(FILE_NAMES_NOT_YET);
        }
        entries
    }

    /// Every subject `workspace.state` speaks about, and which side of the line it is on.
    ///
    /// One list rather than two, because the failure worth preventing is a subject drifting onto
    /// both sides or off both. [`subjects_are_partitioned`] is where that becomes a test instead
    /// of a habit.
    #[must_use]
    pub fn subjects(&self) -> Vec<(&'static str, bool)> {
        let mut subjects = SUBJECTS.to_vec();
        subjects.push(("shared version", self.shared_version.is_answered()));
        subjects.push(("file names and folders", self.names_answered));
        subjects
    }
}

fn open_store(database: &DatabasePath) -> Result<Store<Sqlite>, String> {
    let driver = Sqlite::open(database.as_path()).map_err(|error| error.to_string())?;
    Store::open(driver).map_err(|error| error.to_string())
}

fn remove_index(database: &DatabasePath) -> Result<(), OpenFailure> {
    for path in database.files() {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(OpenFailure::Index {
                    detail: format!("could not replace {}: {error}", path.display()),
                });
            }
        }
    }
    Ok(())
}

fn fresh_store(database: &DatabasePath) -> Result<Store<Sqlite>, OpenFailure> {
    remove_index(database)?;
    open_store(database).map_err(|detail| OpenFailure::Index { detail })
}

fn rebuild_into(
    store: &mut Store<Sqlite>,
    records: &[mesh_store::StoredRecord],
    readable: DurableBoundary,
) -> Result<mesh_store::Digest16, OpenFailure> {
    store
        .rebuild_from(records.iter().cloned())
        .map_err(|error| match error {
            StoreError::Fold(error) => OpenFailure::Contradictory {
                detail: error.to_string(),
                readable,
            },
            other => OpenFailure::Index {
                detail: other.to_string(),
            },
        })
}

fn open_and_rebuild_index(
    database: &DatabasePath,
    records: &[mesh_store::StoredRecord],
    readable: DurableBoundary,
) -> Result<Store<Sqlite>, OpenFailure> {
    let mut store = match open_store(database) {
        Ok(store) => store,
        Err(_) => fresh_store(database)?,
    };
    match rebuild_into(&mut store, records, readable) {
        Ok(_) => Ok(store),
        Err(OpenFailure::Contradictory { detail, readable }) => {
            Err(OpenFailure::Contradictory { detail, readable })
        }
        Err(OpenFailure::Index { .. }) => {
            drop(store);
            let mut rebuilt = fresh_store(database)?;
            rebuild_into(&mut rebuilt, records, readable)?;
            Ok(rebuilt)
        }
        Err(other) => Err(other),
    }
}

fn open_and_rebuild_transient_index(
    records: &[mesh_store::StoredRecord],
    readable: DurableBoundary,
) -> Result<Store<Sqlite>, OpenFailure> {
    let driver = Sqlite::open_in_memory().map_err(|error| OpenFailure::Index {
        detail: error.to_string(),
    })?;
    let mut store = Store::open(driver).map_err(|error| OpenFailure::Index {
        detail: error.to_string(),
    })?;
    rebuild_into(&mut store, records, readable)?;
    Ok(store)
}

struct MaterializedNames {
    state: Option<mesh_materializer::WorkspaceState>,
    entries: Vec<WorkspaceEntry>,
    file_histories: Vec<WorkspaceFileHistory>,
    retired_entries: Vec<RetiredWorkspaceEntry>,
    conditions: Vec<WorkspaceCondition>,
    complete: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct VisibleEntryBasis {
    object: mesh_materializer::ObjectId,
    entry_type: &'static str,
    last_file_version: Option<RestoreVersionIdentity>,
    last_executable: Option<bool>,
}

fn visible_entry_bases(
    state: &mesh_materializer::WorkspaceState,
) -> BTreeMap<String, VisibleEntryBasis> {
    state
        .objects()
        .iter()
        .filter_map(|(object, record)| {
            if *object == state.root() || record.is_deleted() {
                return None;
            }
            let path = state
                .path_of(*object)?
                .iter()
                .map(mesh_operations::NormalizedName::as_str)
                .collect::<Vec<_>>()
                .join("/");
            match record.kind() {
                mesh_materializer::ObjectKind::Directory => Some((
                    path,
                    VisibleEntryBasis {
                        object: *object,
                        entry_type: "folder",
                        last_file_version: None,
                        last_executable: None,
                    },
                )),
                mesh_materializer::ObjectKind::File => {
                    let version = record.current_version()?;
                    let file = state.file_version(version)?;
                    Some((
                        path,
                        VisibleEntryBasis {
                            object: *object,
                            entry_type: "file",
                            last_file_version: Some(RestoreVersionIdentity {
                                version,
                                manifest: file.manifest_id(),
                            }),
                            last_executable: Some(file.portable_metadata().is_executable()),
                        },
                    ))
                }
            }
        })
        .collect()
}

fn operation_can_retire_path(operation: &mesh_materializer::Operation) -> bool {
    matches!(
        operation,
        mesh_materializer::Operation::UnlinkDirectoryEntry { .. }
            | mesh_materializer::Operation::RenameEntry { .. }
            | mesh_materializer::Operation::MoveEntry { .. }
            | mesh_materializer::Operation::DeleteObject { .. }
            | mesh_materializer::Operation::ResolveNameConflict { .. }
    )
}

/// Replay once and remember the last durable occupant of every path that disappeared. Only
/// structural operations snapshot the visible path map, avoiding a full historical rebuild for
/// every durable workspace point and avoiding scans for content-only operations.
fn retired_entry_bases(
    root: mesh_materializer::ObjectId,
    set: &[mesh_materializer::AppliedChangeSet],
) -> Vec<RetiredWorkspaceEntry> {
    let by_id = set
        .iter()
        .map(|change| (change.id(), change))
        .collect::<BTreeMap<_, _>>();
    let mut state = mesh_materializer::WorkspaceState::empty(root);
    let mut retired = BTreeMap::new();
    let roots = set
        .iter()
        .filter(|change| change.causal_parents().is_empty())
        .collect::<Vec<_>>();
    let imported_objects = match roots.as_slice() {
        [initial] => initial
            .operations()
            .iter()
            .filter_map(|operation| match operation {
                mesh_materializer::Operation::CreateFile { object_id }
                | mesh_materializer::Operation::CreateDirectory { object_id } => Some(*object_id),
                _ => None,
            })
            .collect::<BTreeSet<_>>(),
        _ => BTreeSet::new(),
    };
    for id in mesh_materializer::causal_order(set) {
        let Some(change) = by_id.get(&id) else {
            continue;
        };
        for operation in change.operations() {
            let before = operation_can_retire_path(operation).then(|| visible_entry_bases(&state));
            if mesh_materializer::apply_operation(&mut state, id, operation).is_err() {
                continue;
            }
            if let Some(before) = before {
                let after = visible_entry_bases(&state);
                for (path, basis) in before {
                    if after
                        .get(&path)
                        .is_none_or(|current| current.object != basis.object)
                    {
                        retired.insert(path, basis);
                    }
                }
            }
        }
    }
    let current = visible_entry_bases(&state);
    retired.retain(|path, _| !current.contains_key(path));
    retired
        .into_iter()
        .map(|(path, basis)| RetiredWorkspaceEntry {
            object: basis.object,
            path,
            entry_type: basis.entry_type,
            last_file_version: basis.last_file_version,
            last_executable: basis.last_executable,
            imported_with_workspace: imported_objects.contains(&basis.object),
        })
        .collect()
}

fn condition(
    code: &'static str,
    message: &'static str,
    related: Vec<String>,
) -> WorkspaceCondition {
    WorkspaceCondition {
        code,
        message,
        related,
    }
}

/// Rebuild visible paths solely from saved payloads. Nothing here reads a directory entry from the
/// workspace's working folder; the only filesystem reads are verified CAS reads by digest.
fn materialize_names<F: mesh_cas::DurableFs>(
    index: &Index,
    store: &mesh_cas::Cas<F, mesh_cas::Blake3>,
) -> MaterializedNames {
    materialize_operation_records(operation_records(index), store)
}

fn materialize_operation_records<F: mesh_cas::DurableFs>(
    records: BTreeMap<mesh_store::RecordDigest, OperationRecord>,
    store: &mesh_cas::Cas<F, mesh_cas::Blake3>,
) -> MaterializedNames {
    if records.is_empty() {
        return MaterializedNames {
            state: None,
            entries: Vec::new(),
            file_histories: Vec::new(),
            retired_entries: Vec::new(),
            conditions: Vec::new(),
            complete: true,
        };
    }

    let mut decoded = Vec::new();
    let mut conditions = Vec::new();
    for record in records.values() {
        let digest = mesh_cas::Digest32::from_bytes(*record.payload_digest.as_bytes());
        let bytes = match store.read(&digest) {
            Ok(bytes) => bytes,
            Err(mesh_cas::CasError::Absent { .. }) => {
                conditions.push(condition(
                    "operation-payload-missing",
                    user_messages::OPERATION_PAYLOAD_MISSING,
                    vec![record.id.to_string(), digest.to_string()],
                ));
                continue;
            }
            Err(_error) => {
                conditions.push(condition(
                    "operation-payload-unreadable",
                    user_messages::OPERATION_PAYLOAD_UNREADABLE,
                    vec![record.id.to_string(), digest.to_string()],
                ));
                continue;
            }
        };
        match decode_changeset_operations(&bytes) {
            Some(operations) => decoded.push((record, operations)),
            None => conditions.push(condition(
                "operation-payload-invalid",
                user_messages::OPERATION_PAYLOAD_INVALID,
                vec![record.id.to_string(), digest.to_string()],
            )),
        }
    }

    let mut created_directories = BTreeSet::new();
    let mut referenced_directories = BTreeSet::new();
    for (_, operations) in &decoded {
        for operation in operations {
            directory_facts(
                operation,
                &mut created_directories,
                &mut referenced_directories,
            );
        }
    }
    let candidates: Vec<mesh_materializer::ObjectId> = referenced_directories
        .difference(&created_directories)
        .copied()
        .collect();

    let root = match candidates.as_slice() {
        [root] => *root,
        [] if conditions.is_empty() => {
            conditions.push(condition(
                "workspace-root-missing",
                user_messages::WORKSPACE_ROOT_MISSING,
                Vec::new(),
            ));
            return MaterializedNames {
                state: None,
                entries: Vec::new(),
                file_histories: Vec::new(),
                retired_entries: Vec::new(),
                conditions,
                complete: false,
            };
        }
        [] => {
            return MaterializedNames {
                state: None,
                entries: Vec::new(),
                file_histories: Vec::new(),
                retired_entries: Vec::new(),
                conditions,
                complete: false,
            };
        }
        roots if conditions.is_empty() => {
            conditions.push(condition(
                "workspace-root-ambiguous",
                user_messages::WORKSPACE_ROOT_AMBIGUOUS,
                roots.iter().map(ToString::to_string).collect(),
            ));
            return MaterializedNames {
                state: None,
                entries: Vec::new(),
                file_histories: Vec::new(),
                retired_entries: Vec::new(),
                conditions,
                complete: false,
            };
        }
        _ => {
            return MaterializedNames {
                state: None,
                entries: Vec::new(),
                file_histories: Vec::new(),
                retired_entries: Vec::new(),
                conditions,
                complete: false,
            };
        }
    };

    let set: Vec<mesh_materializer::AppliedChangeSet> = decoded
        .into_iter()
        .map(|(record, operations)| {
            mesh_materializer::AppliedChangeSet::new(
                mesh_materializer::ChangeSetId::from_bytes(*record.id.as_bytes()),
                record
                    .parents
                    .iter()
                    .map(|parent| mesh_materializer::ChangeSetId::from_bytes(*parent.as_bytes()))
                    .collect(),
                operations,
            )
        })
        .collect();
    let retired_entries = retired_entry_bases(root, &set);
    let materialized = mesh_materializer::materialize(root, &set);
    if !materialized.rejections().is_empty() {
        conditions.push(condition(
            "materialization-incomplete",
            user_messages::MATERIALIZATION_INCOMPLETE,
            materialized
                .rejections()
                .iter()
                .map(|rejection| rejection.kind().as_str().to_owned())
                .collect(),
        ));
    }

    let mut entries = Vec::new();
    let mut file_histories = Vec::new();
    for (object_id, object) in materialized.state().objects() {
        if *object_id == root || object.is_deleted() {
            continue;
        }
        let Some(path) = materialized.state().path_of(*object_id) else {
            continue;
        };
        let path = path
            .iter()
            .map(mesh_operations::NormalizedName::as_str)
            .collect::<Vec<_>>()
            .join("/");
        entries.push(WorkspaceEntry {
            path: path.clone(),
            entry_type: match object.kind() {
                mesh_materializer::ObjectKind::File => "file",
                mesh_materializer::ObjectKind::Directory => "folder",
            },
        });
        if object.kind() == mesh_materializer::ObjectKind::File {
            let retained = materialized
                .state()
                .file_versions()
                .iter()
                .filter(|(_, version)| version.object_id() == *object_id)
                .map(|(version, _)| version_identity(materialized.state(), *version))
                .collect();
            file_histories.push(WorkspaceFileHistory {
                path,
                object: *object_id,
                current: object
                    .current_version()
                    .map(|version| version_identity(materialized.state(), version)),
                retained,
            });
        }
    }
    entries.sort();
    file_histories.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.object.cmp(&right.object))
    });

    let complete = conditions.is_empty();
    let state = complete.then(|| materialized.state().clone());
    MaterializedNames {
        state,
        entries,
        file_histories: if complete { file_histories } else { Vec::new() },
        retired_entries: if complete {
            retired_entries
        } else {
            Vec::new()
        },
        conditions,
        complete,
    }
}

fn reconstruct_manifest<F: mesh_cas::DurableFs>(
    store: &mesh_cas::Cas<F, mesh_cas::Blake3>,
    manifest: &mesh_store::ManifestRecord,
) -> Result<Vec<u8>, WorkspaceVersionFailure> {
    #[cfg(test)]
    HISTORICAL_RECONSTRUCTED_BYTES.set(
        HISTORICAL_RECONSTRUCTED_BYTES
            .get()
            .saturating_add(manifest.byte_length),
    );
    let capacity = usize::try_from(manifest.byte_length).map_err(|_| {
        WorkspaceVersionFailure::RetainedContent(
            "a retained manifest length does not fit this platform".to_owned(),
        )
    })?;
    let mut bytes = Vec::with_capacity(capacity);
    visit_verified_manifest_chunks(store, manifest, |chunk| {
        bytes.extend_from_slice(chunk);
        Ok(())
    })?;
    Ok(bytes)
}

fn verify_manifest<F: mesh_cas::DurableFs>(
    store: &mesh_cas::Cas<F, mesh_cas::Blake3>,
    manifest: &mesh_store::ManifestRecord,
) -> Result<(), WorkspaceVersionFailure> {
    visit_verified_manifest_chunks(store, manifest, |_| Ok(()))
}

fn visit_verified_manifest_chunks<F: mesh_cas::DurableFs>(
    store: &mesh_cas::Cas<F, mesh_cas::Blake3>,
    manifest: &mesh_store::ManifestRecord,
    mut visit: impl FnMut(&[u8]) -> Result<(), WorkspaceVersionFailure>,
) -> Result<(), WorkspaceVersionFailure> {
    let mut expected_offset = 0_u64;
    let mut content = Blake3::hasher();
    for chunk in &manifest.chunks {
        if chunk.byte_offset != expected_offset {
            return Err(WorkspaceVersionFailure::RetainedContent(
                "a retained manifest has a gap or overlap".to_owned(),
            ));
        }
        let chunk_bytes = store
            .read(&mesh_cas::Digest32::from_bytes(*chunk.digest.as_bytes()))
            .map_err(|error| WorkspaceVersionFailure::RetainedContent(error.to_string()))?;
        if u64::try_from(chunk_bytes.len()).ok() != Some(chunk.byte_length) {
            return Err(WorkspaceVersionFailure::RetainedContent(
                "a retained chunk length does not match its manifest".to_owned(),
            ));
        }
        content.update(&chunk_bytes);
        visit(&chunk_bytes)?;
        expected_offset = expected_offset
            .checked_add(chunk.byte_length)
            .ok_or_else(|| {
                WorkspaceVersionFailure::RetainedContent(
                    "a retained manifest length overflowed".to_owned(),
                )
            })?;
    }
    if expected_offset != manifest.byte_length
        || mesh_store::RecordDigest::from_bytes(*content.finalize().as_bytes())
            != manifest.content_digest
    {
        return Err(WorkspaceVersionFailure::RetainedContent(
            "the reconstructed bytes do not match the retained manifest".to_owned(),
        ));
    }
    Ok(())
}

fn version_identity(
    state: &mesh_materializer::WorkspaceState,
    version: mesh_materializer::VersionId,
) -> RestoreVersionIdentity {
    let manifest = state
        .file_version(version)
        .expect("the checked restore plan only names retained file versions")
        .manifest_id();
    RestoreVersionIdentity { version, manifest }
}

fn version_json(identity: RestoreVersionIdentity) -> crate::ipc::Json {
    crate::ipc::Json::object([
        (
            "version_id",
            crate::ipc::Json::text(identity.version.to_string()),
        ),
        (
            "manifest_id",
            crate::ipc::Json::text(identity.manifest.to_string()),
        ),
    ])
}

struct ReviewItemProjection<'a> {
    reviewed_head: Option<String>,
    operations: Vec<crate::ipc::Json>,
    operations_not_listed: u64,
    presentation_digest: Option<String>,
    bundle_changes: Vec<crate::ipc::Json>,
    bundle_changes_not_listed: u64,
    unavailable_code: Option<&'a str>,
}

impl<'a> ReviewItemProjection<'a> {
    fn unavailable(code: &'a str) -> Self {
        Self {
            reviewed_head: None,
            operations: Vec::new(),
            operations_not_listed: 0,
            presentation_digest: None,
            bundle_changes: Vec::new(),
            bundle_changes_not_listed: 0,
            unavailable_code: Some(code),
        }
    }
}

fn review_item_json(
    bundle: RecordDigest,
    review: ReviewRecord,
    recorded: bool,
    subject: Option<&OperationRecord>,
    projection: ReviewItemProjection<'_>,
) -> crate::ipc::Json {
    crate::ipc::Json::object([
        ("bundle", crate::ipc::Json::text(bundle.to_string())),
        (
            "subject_operation",
            crate::ipc::Json::text(review.subject_operation.to_string()),
        ),
        (
            "reviewed_head",
            projection
                .reviewed_head
                .map_or(crate::ipc::Json::Null, crate::ipc::Json::text),
        ),
        (
            "opened_by",
            if recorded {
                crate::ipc::Json::text(review.opened_by.to_string())
            } else {
                crate::ipc::Json::Null
            },
        ),
        ("recorded", crate::ipc::Json::Bool(recorded)),
        (
            "author",
            subject.map_or(crate::ipc::Json::Null, |record| {
                crate::ipc::Json::text(record.actor.to_string())
            }),
        ),
        (
            "actor_sequence",
            subject.map_or(crate::ipc::Json::Null, |record| {
                // Decimal text preserves the full u64 across the JavaScript IPC boundary.
                crate::ipc::Json::text(record.actor_sequence.to_string())
            }),
        ),
        (
            "subject_operations",
            crate::ipc::Json::Array(projection.operations),
        ),
        (
            "subject_operations_not_listed",
            crate::ipc::Json::Number(projection.operations_not_listed),
        ),
        (
            "presentation_digest",
            projection
                .presentation_digest
                .map_or(crate::ipc::Json::Null, crate::ipc::Json::text),
        ),
        (
            "bundle_changes",
            crate::ipc::Json::Array(projection.bundle_changes),
        ),
        (
            "bundle_changes_not_listed",
            crate::ipc::Json::Number(projection.bundle_changes_not_listed),
        ),
        (
            "content_complete",
            crate::ipc::Json::Bool(
                projection.unavailable_code.is_none()
                    && projection.operations_not_listed == 0
                    && projection.bundle_changes_not_listed == 0,
            ),
        ),
        (
            "unavailable_code",
            projection
                .unavailable_code
                .map_or(crate::ipc::Json::Null, crate::ipc::Json::text),
        ),
        (
            "projection_authorizes_approval",
            crate::ipc::Json::Bool(false),
        ),
    ])
}

fn review_change_json(
    change: &mesh_approval::PresentedChange,
    verified_text: Option<&crate::ipc::Json>,
) -> crate::ipc::Json {
    let opaque_reason = match change.body() {
        mesh_approval::ChangeBody::Opaque { reason } => Some(reason.label()),
        mesh_approval::ChangeBody::Placement
        | mesh_approval::ChangeBody::Text { .. }
        | mesh_approval::ChangeBody::Binary => None,
    };
    crate::ipc::Json::object([
        (
            "object_id",
            crate::ipc::Json::text(change.object().to_string()),
        ),
        (
            "path_before",
            change
                .path_before()
                .map_or(crate::ipc::Json::Null, crate::ipc::Json::text),
        ),
        (
            "path_after",
            change
                .path_after()
                .map_or(crate::ipc::Json::Null, crate::ipc::Json::text),
        ),
        ("effect", crate::ipc::Json::text(change.effect())),
        (
            "before",
            change
                .before()
                .map_or(crate::ipc::Json::Null, review_content_summary_json),
        ),
        (
            "after",
            change
                .after()
                .map_or(crate::ipc::Json::Null, review_content_summary_json),
        ),
        ("body", crate::ipc::Json::text(change.body().label())),
        (
            "opaque_reason",
            opaque_reason.map_or(crate::ipc::Json::Null, crate::ipc::Json::text),
        ),
        (
            "verified_text",
            verified_text.cloned().unwrap_or(crate::ipc::Json::Null),
        ),
    ])
}

fn native_content_summary(summary: Option<mesh_approval::ContentSummary>) -> String {
    match summary {
        None => "not present".to_owned(),
        Some(summary @ mesh_approval::ContentSummary::Text { .. }) => format!(
            "text version {} · {} lines",
            summary.version(),
            summary.line_count().expect("text carries a line count")
        ),
        Some(summary @ mesh_approval::ContentSummary::Binary { .. }) => format!(
            "binary version {} · digest {} · {} bytes",
            summary.version(),
            summary.digest().expect("binary carries a digest"),
            summary.byte_length().expect("binary carries a byte count")
        ),
    }
}

/// Bounded UTF-8 content independently reconstructed from one exact manifest.
#[derive(Clone, Debug)]
struct VerifiedReviewText {
    version: mesh_approval::VersionId,
    digest: mesh_approval::Digest32,
    lines: Vec<String>,
}

fn verified_review_text(
    version: mesh_approval::VersionId,
    digest: mesh_approval::Digest32,
    bytes: &[u8],
) -> Option<VerifiedReviewText> {
    const MAX_PREVIEW_BYTES: usize = 256 * 1024;

    if bytes.len() > MAX_PREVIEW_BYTES {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    if text.chars().any(|character| {
        (character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
            || matches!(
                character,
                '\u{061c}'
                    | '\u{200e}'
                    | '\u{200f}'
                    | '\u{202a}'..='\u{202e}'
                    | '\u{2066}'..='\u{2069}'
            )
    }) {
        return None;
    }
    Some(VerifiedReviewText {
        version,
        digest,
        lines: text.lines().map(str::to_owned).collect(),
    })
}

/// A bounded convenience diff over content already authenticated by the binary review summary.
///
/// Both sides are reconstructed from their exact CAS manifests. Their identities are repeated in
/// this projection so a client can refuse a stale or mismatched view before rendering it. This
/// projection cannot change what is approved and disappears for large, binary, unsafe, or
/// complexity-bounded content.
fn verified_text_json(
    before: Option<&VerifiedReviewText>,
    after: Option<&VerifiedReviewText>,
) -> Option<crate::ipc::Json> {
    let before_lines = before.map_or(&[][..], |text| text.lines.as_slice());
    let after_lines = after.map_or(&[][..], |text| text.lines.as_slice());
    let hunks = mesh_approval::text_hunks(before_lines, after_lines)?;
    if hunks.is_empty() {
        return None;
    }
    let identity = |text: &VerifiedReviewText| {
        crate::ipc::Json::object([
            (
                "version_id",
                crate::ipc::Json::text(text.version.to_string()),
            ),
            (
                "content_digest",
                crate::ipc::Json::text(text.digest.to_string()),
            ),
        ])
    };
    Some(crate::ipc::Json::object([
        ("source", crate::ipc::Json::text("before-after")),
        ("before", before.map_or(crate::ipc::Json::Null, identity)),
        ("after", after.map_or(crate::ipc::Json::Null, identity)),
        (
            "hunks",
            crate::ipc::Json::Array(hunks.iter().map(review_text_hunk_json).collect()),
        ),
    ]))
}

fn review_text_hunk_json(hunk: &mesh_approval::TextHunk) -> crate::ipc::Json {
    crate::ipc::Json::object([
        (
            "before_start",
            crate::ipc::Json::Number(hunk.before_start() as u64),
        ),
        (
            "before_len",
            crate::ipc::Json::Number(hunk.before_len() as u64),
        ),
        (
            "after_start",
            crate::ipc::Json::Number(hunk.after_start() as u64),
        ),
        (
            "after_len",
            crate::ipc::Json::Number(hunk.after_len() as u64),
        ),
        (
            "lines",
            crate::ipc::Json::Array(hunk.lines().iter().map(review_diff_line_json).collect()),
        ),
    ])
}

fn review_diff_line_json(line: &mesh_approval::DiffLine) -> crate::ipc::Json {
    let (before, after) = match line {
        mesh_approval::DiffLine::Context { before, after, .. } => {
            (Some(*before as u64), Some(*after as u64))
        }
        mesh_approval::DiffLine::Removed { before, .. } => (Some(*before as u64), None),
        mesh_approval::DiffLine::Added { after, .. } => (None, Some(*after as u64)),
    };
    crate::ipc::Json::object([
        ("kind", crate::ipc::Json::text(line.label())),
        (
            "before",
            before.map_or(crate::ipc::Json::Null, crate::ipc::Json::Number),
        ),
        (
            "after",
            after.map_or(crate::ipc::Json::Null, crate::ipc::Json::Number),
        ),
        ("text", crate::ipc::Json::text(line.text())),
    ])
}

fn review_content_summary_json(summary: mesh_approval::ContentSummary) -> crate::ipc::Json {
    crate::ipc::Json::object([
        (
            "kind",
            crate::ipc::Json::text(if summary.is_binary() {
                "binary"
            } else {
                "text"
            }),
        ),
        (
            "version_id",
            crate::ipc::Json::text(summary.version().to_string()),
        ),
        (
            "content_digest",
            summary.digest().map_or(crate::ipc::Json::Null, |digest| {
                crate::ipc::Json::text(digest.to_string())
            }),
        ),
        (
            "byte_length",
            summary
                .byte_length()
                .map_or(crate::ipc::Json::Null, |length| {
                    crate::ipc::Json::text(length.to_string())
                }),
        ),
        (
            "line_count",
            summary
                .line_count()
                .map_or(crate::ipc::Json::Null, |lines| {
                    crate::ipc::Json::text(lines.to_string())
                }),
        ),
    ])
}

fn operation_json(operation: &mesh_materializer::Operation) -> crate::ipc::Json {
    let mut fields = vec![(
        "kind".to_owned(),
        crate::ipc::Json::text(operation.kind().as_str()),
    )];
    match operation {
        mesh_materializer::Operation::UnlinkDirectoryEntry {
            directory_id,
            name,
            object_id,
        } => {
            fields.push((
                "directory_id".to_owned(),
                crate::ipc::Json::text(directory_id.to_string()),
            ));
            fields.push(("name".to_owned(), crate::ipc::Json::text(name.as_str())));
            fields.push((
                "object_id".to_owned(),
                crate::ipc::Json::text(object_id.to_string()),
            ));
        }
        mesh_materializer::Operation::DeleteObject { object_id } => fields.push((
            "object_id".to_owned(),
            crate::ipc::Json::text(object_id.to_string()),
        )),
        mesh_materializer::Operation::RestoreObject {
            object_id,
            restored_version_id,
        } => {
            fields.push((
                "object_id".to_owned(),
                crate::ipc::Json::text(object_id.to_string()),
            ));
            fields.push((
                "restored_version_id".to_owned(),
                crate::ipc::Json::text(restored_version_id.to_string()),
            ));
        }
        mesh_materializer::Operation::LinkDirectoryEntry {
            directory_id,
            name,
            object_id,
            version_id,
        } => {
            fields.push((
                "directory_id".to_owned(),
                crate::ipc::Json::text(directory_id.to_string()),
            ));
            fields.push(("name".to_owned(), crate::ipc::Json::text(name.as_str())));
            fields.push((
                "object_id".to_owned(),
                crate::ipc::Json::text(object_id.to_string()),
            ));
            fields.push((
                "version_id".to_owned(),
                crate::ipc::Json::text(version_id.to_string()),
            ));
        }
        _ => {}
    }
    let encoded = mesh_operations::encode_operations(core::slice::from_ref(operation));
    fields.push((
        "canonical_hex".to_owned(),
        crate::ipc::Json::text(hex_bytes(&encoded[0])),
    ));
    crate::ipc::Json::Object(fields)
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use core::fmt::Write as _;
        write!(text, "{byte:02x}").expect("writing into String cannot fail");
    }
    text
}

fn managed_path_components(
    relative: &str,
    reserve_private_top_level: bool,
) -> Result<Vec<NormalizedName>, String> {
    let path = Path::new(relative);
    if relative.is_empty() || path.is_absolute() {
        return Err("the managed path must be relative".to_owned());
    }
    let raw = path.components().collect::<Vec<_>>();
    if raw.is_empty()
        || raw
            .iter()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err("the managed path contains a traversal or non-name component".to_owned());
    }
    let mut names = Vec::with_capacity(raw.len());
    for (index, component) in raw.into_iter().enumerate() {
        let std::path::Component::Normal(value) = component else {
            return Err("the managed path contains a non-name component".to_owned());
        };
        let text = value
            .to_str()
            .ok_or_else(|| "the managed path is not valid UTF-8".to_owned())?;
        if is_private_managed_component_in_layout(index, text, reserve_private_top_level) {
            return Err("the managed path is reserved for private workspace state".to_owned());
        }
        names.push(NormalizedName::new(text).map_err(|error| error.to_string())?);
    }
    Ok(names)
}

pub(crate) fn is_private_managed_component(index: usize, name: &str) -> bool {
    is_private_managed_component_in_layout(index, name, true)
}

pub(crate) fn is_private_managed_component_in_layout(
    index: usize,
    name: &str,
    reserve_private_top_level: bool,
) -> bool {
    // Git metadata belongs to the native adapter at every nesting depth, not to workspace
    // content. Keeping this structural (rather than an ignore-rule default) means a
    // repository-local configuration can never accidentally re-include Git's mutable object
    // database, including one created by an agent in a nested repository.
    name == ".git"
        || (reserve_private_top_level && index == 0 && PRIVATE_MANAGED_TOP_LEVEL.contains(&name))
}

/// Enumerate native names without reading file contents or following links.
///
/// Discovery is deliberately nonauthoritative: the later inspection/adoption path performs the
/// exact confined read and identity checks that authorize a durable append. Links and special
/// objects are nevertheless returned as unsupported so omission cannot look like a complete
/// review. Keeping all classifications in one metadata pass avoids an extra workspace walk.
fn discover_native_entries(
    open: &OpenWorkspace,
    reserve_private_top_level: bool,
    exclusions: &EffectiveExclusions,
) -> NativeDiscovery {
    let root = open.physical_root.as_path();
    let mut files = BTreeSet::new();
    let mut directories = BTreeSet::new();
    let mut unsupported = BTreeSet::new();
    let mut complete = true;
    let mut pending = vec![(root.to_path_buf(), Vec::<String>::new(), None::<String>)];
    while let Some((directory, components, excluded_ancestor)) = pending.pop() {
        let Ok(entries) = fs::read_dir(directory) else {
            complete = false;
            continue;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                complete = false;
                continue;
            };
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                complete = false;
                continue;
            };
            if is_private_managed_component_in_layout(
                components.len(),
                &name,
                reserve_private_top_level,
            ) {
                continue;
            }
            let mut relative = components.clone();
            relative.push(name);
            let relative_path = relative.join("/");
            let versioned = if reserve_private_top_level {
                exclusions.versions_path(&relative_path)
            } else {
                exclusions.versions_presented_path(&relative_path)
            };
            let versioned = match versioned {
                Ok(versioned) => versioned,
                Err(_) => {
                    complete = false;
                    continue;
                }
            };
            if versioned && excluded_ancestor.is_some() {
                unsupported.insert(NativeUnsupportedEntry {
                    path: relative_path,
                    kind: "excluded-ancestor",
                });
                continue;
            }
            if !versioned && !exclusions.has_reinclusion_rules() {
                // An excluded subtree with no possible re-inclusion is outside the reviewable
                // inventory. Do not pay to traverse it, and do not let inaccessible generated
                // output turn an otherwise complete user review into a false failure.
                continue;
            }
            let Ok(metadata) = fs::symlink_metadata(entry.path()) else {
                complete = false;
                continue;
            };
            if metadata.file_type().is_symlink() {
                if !open.exact_private_codex_link(Path::new(&relative_path), &entry.path()) {
                    unsupported.insert(NativeUnsupportedEntry {
                        path: relative_path,
                        kind: "symbolic-link",
                    });
                }
                continue;
            }
            if metadata.is_dir() {
                if versioned {
                    directories.insert(relative_path);
                    pending.push((entry.path(), relative, None));
                } else {
                    let excluded_ancestor = excluded_ancestor.clone().or(Some(relative_path));
                    pending.push((entry.path(), relative, excluded_ancestor));
                }
            } else if metadata.is_file() {
                if versioned {
                    files.insert(relative_path);
                }
            } else {
                unsupported.insert(NativeUnsupportedEntry {
                    path: relative_path,
                    kind: "special",
                });
            }
        }
    }
    NativeDiscovery {
        files: files.into_iter().collect(),
        directories: directories.into_iter().collect(),
        unsupported: unsupported.into_iter().collect(),
        complete,
    }
}

/// Stop at the first ordinary file or folder that zero-history onboarding needs to explain.
///
/// Unlike the complete change inventory, this does not allocate every path or read file bodies.
/// An unusual path that the exclusion parser cannot classify still counts as content: hiding it
/// would incorrectly tell the person that the folder is empty.
fn has_discoverable_native_content(
    root: &Path,
    reserve_private_top_level: bool,
    exclusions: &EffectiveExclusions,
) -> bool {
    let mut pending = vec![(root.to_path_buf(), Vec::<String>::new())];
    while let Some((directory, components)) = pending.pop() {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(metadata) = fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if metadata.file_type().is_symlink() || (!metadata.is_dir() && !metadata.is_file()) {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                return true;
            };
            if is_private_managed_component_in_layout(
                components.len(),
                &name,
                reserve_private_top_level,
            ) {
                continue;
            }
            let mut relative = components.clone();
            relative.push(name);
            let path = relative.join("/");
            let versioned = if reserve_private_top_level {
                exclusions.versions_path(&path)
            } else {
                exclusions.versions_presented_path(&path)
            };
            if versioned.unwrap_or(true) {
                return true;
            }
            if metadata.is_dir() && exclusions.has_reinclusion_rules() {
                pending.push((entry.path(), relative));
            }
        }
    }
    false
}

fn resolve_directory(
    state: &mesh_materializer::WorkspaceState,
    components: &[NormalizedName],
) -> Result<ObjectId, String> {
    let mut at = state.root();
    for name in components {
        let entry = state
            .directory(at)
            .and_then(|directory| directory.entry(name))
            .ok_or_else(|| format!("managed parent folder {} does not exist", name.as_str()))?;
        let object = state
            .object(entry.object_id())
            .ok_or_else(|| "a managed folder binding names an unknown object".to_owned())?;
        if object.kind() != mesh_materializer::ObjectKind::Directory || object.is_deleted() {
            return Err(format!(
                "managed parent {} is not a live folder",
                name.as_str()
            ));
        }
        at = entry.object_id();
    }
    Ok(at)
}

/// Decode one canonical ChangeSet envelope and then its nested operation records.
///
/// `OperationRecord` is one ChangeSet, not one operation. Preserving the nested sequence is what
/// lets one durable journal record carry an atomic create/write/link transition.
fn decode_changeset_operations(bytes: &[u8]) -> Option<Vec<mesh_operations::Operation>> {
    let fields = decode_changeset_fields(bytes)?;
    let operations_index = mesh_operations::CHANGESET_SCHEMA
        .fields
        .iter()
        .position(|field| field.name == "operations")?;
    let mesh_operations::CanonicalValue::Sequence(encoded) = fields.get(operations_index)? else {
        return None;
    };
    encoded
        .iter()
        .map(|value| {
            let mesh_operations::CanonicalValue::Record(bytes) = value else {
                return None;
            };
            mesh_operations::decode_operation(bytes).ok()
        })
        .collect()
}

fn decode_changeset_fields(bytes: &[u8]) -> Option<Vec<mesh_operations::CanonicalValue>> {
    if let Ok(fields) = mesh_operations::decode_canonical(&mesh_operations::CHANGESET_SCHEMA, bytes)
    {
        return Some(fields);
    }
    let envelope =
        crate::authenticated_changeset::AuthenticatedChangeSet::from_canonical_bytes(bytes).ok()?;
    mesh_operations::decode_canonical(&mesh_operations::CHANGESET_SCHEMA, envelope.changeset()).ok()
}

fn operation_records(index: &Index) -> BTreeMap<mesh_store::RecordDigest, OperationRecord> {
    let mut records = BTreeMap::new();
    for actor in index.actors() {
        for record in index.operations_of(&actor) {
            records.insert(record.id, record.clone());
        }
    }
    records
}

fn causal_operation_closure(
    all: &BTreeMap<RecordDigest, OperationRecord>,
    target: RecordDigest,
) -> Result<BTreeMap<RecordDigest, OperationRecord>, String> {
    let mut selected = BTreeMap::new();
    let mut pending = vec![target];
    while let Some(id) = pending.pop() {
        if selected.contains_key(&id) {
            continue;
        }
        let record = all
            .get(&id)
            .ok_or_else(|| "the selected review target has incomplete causal history".to_owned())?
            .clone();
        pending.extend(record.parents.iter().copied());
        selected.insert(id, record);
    }
    Ok(selected)
}

fn review_head_for_records(
    selected: &BTreeMap<RecordDigest, OperationRecord>,
) -> Result<mesh_approval::HeadId, String> {
    let mut selected_index = Index::new();
    for record in selected.values() {
        selected_index
            .apply(StoredRecord::Operation(record.clone()))
            .map_err(|error| format!("the selected review history is contradictory: {error}"))?;
    }
    let private_version = version_state::fold(&selected_index);
    let private_head = private_version
        .version()
        .ok_or_else(|| "the workspace has no private head to review".to_owned())?;
    let private_head = HeadId::parse(private_head)
        .map_err(|error| format!("the private head is malformed: {error}"))?;
    Ok(mesh_approval::HeadId::from_bytes(*private_head.as_bytes()))
}

/// Collect the root-inference facts with an exhaustive match over the owner vocabulary.
fn directory_facts(
    operation: &mesh_operations::Operation,
    created: &mut BTreeSet<mesh_materializer::ObjectId>,
    referenced: &mut BTreeSet<mesh_materializer::ObjectId>,
) {
    use mesh_operations::Operation;

    match operation {
        Operation::CreateDirectory { object_id } => {
            created.insert(*object_id);
        }
        Operation::LinkDirectoryEntry { directory_id, .. }
        | Operation::UnlinkDirectoryEntry { directory_id, .. }
        | Operation::RenameEntry { directory_id, .. }
        | Operation::ResolveNameConflict { directory_id, .. } => {
            referenced.insert(*directory_id);
        }
        Operation::MoveEntry {
            from_directory_id,
            to_directory_id,
            ..
        } => {
            referenced.insert(*from_directory_id);
            referenced.insert(*to_directory_id);
        }
        Operation::CreateFile { .. }
        | Operation::WriteFileVersion { .. }
        | Operation::DeleteObject { .. }
        | Operation::RestoreObject { .. }
        | Operation::SetPortableMetadata { .. }
        | Operation::ResolveContentConflict { .. }
        | Operation::AdvanceActorHead { .. }
        | Operation::RecordReadObservation { .. }
        | Operation::RecordDerivedNode { .. }
        | Operation::CreateReviewBundle { .. }
        | Operation::RecordValidation { .. }
        | Operation::AdvanceCanonicalHead { .. } => {}
    }
}

/// The honest refusals, in one place so the wire answer and the documentation cannot drift.
///
/// Every line names a subject a user interface will ask for and the reason this build has no
/// answer. **No entry here names a missing crate.** Two once did — "materialising a workspace's
/// files needs mesh-materializer, which is not a dependency of this crate" and "the six-state
/// model over an actor's work needs mesh-state, which is not a dependency of this crate" — and
/// task 01KZFSF7M3A4348TZQD17H8EBV took both edges, which made both sentences false. What is left
/// is a record the file does not hold or a check this build does not run, and each of those is
/// something a person can act on.
const NOT_YET: &[(&str, &str)] = &[(
    "shared version",
    "no user-verified approval has advanced this workspace yet; record the exact review, then use \
     a supported human-approval provider to confirm that saved version",
)];

const FILE_NAMES_NOT_YET: (&str, &str) = (
    "file names and folders",
    "one or more saved changes could not be used to derive a complete names-and-folders answer; \
     see the recoverable conditions beside the partial result",
);

/// Every subject `workspace.state` speaks about, and whether it is answered.
///
/// `true` is answered, `false` is refused. Both halves live in one list so that a subject cannot
/// be answered and still advertised as unanswerable, and cannot quietly fall off both.
const SUBJECTS: &[(&str, bool)] = &[
    ("records on disk", true),
    ("saved changes", true),
    ("who authored them", true),
    ("file manifests", true),
    ("peers", true),
    ("open reviews", true),
    ("private version", true),
    ("durable index", true),
];

/// Whether every refused subject is published and no answered subject is also refused.
///
/// A free function rather than a test body so that the rule is available to the IPC surface's own
/// tests as well; a rule that only one test file can reach is a rule the next surface will drift
/// away from.
#[must_use]
pub fn subjects_are_partitioned(workspace: &OpenWorkspace) -> bool {
    let not_yet = workspace.not_yet();
    let subjects = workspace.subjects();
    let refused: Vec<&str> = not_yet.iter().map(|(subject, _)| *subject).collect();
    subjects.iter().all(|(subject, answered)| {
        if *answered {
            !refused.contains(subject)
        } else {
            refused.contains(subject)
        }
    }) && refused
        .iter()
        .all(|subject| subjects.iter().any(|(known, _)| known == subject))
}

#[cfg(test)]
mod tests {
    use super::*;

    use mesh_store::{
        frame_record, journal_records, no_session, OperationRecord, RecordDigest, StoredRecord,
    };

    fn scratch(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "mesh-workspace-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&path);
        path
    }

    fn digest(byte: u8) -> RecordDigest {
        RecordDigest::from_bytes([byte; 32])
    }

    fn operation(id: u8, actor: u8, sequence: u64) -> StoredRecord {
        StoredRecord::Operation(OperationRecord {
            id: digest(id),
            actor: digest(actor),
            actor_sequence: sequence,
            hlc_millis: 1_700_000_000_000,
            hlc_counter: 0,
            policy_epoch: 1,
            session: no_session(),
            payload_digest: digest(id),
            parents: Vec::new(),
        })
    }

    #[test]
    fn verified_review_text_diff_binds_both_exact_versions() {
        let before = VerifiedReviewText {
            version: mesh_approval::VersionId::from_bytes([0x21; 32]),
            digest: mesh_approval::Digest32::from_bytes([0x31; 32]),
            lines: vec!["old line".to_owned(), "kept line".to_owned()],
        };
        let after = VerifiedReviewText {
            version: mesh_approval::VersionId::from_bytes([0x41; 32]),
            digest: mesh_approval::Digest32::from_bytes([0x51; 32]),
            lines: vec!["new line".to_owned(), "kept line".to_owned()],
        };

        let rendered = verified_text_json(Some(&before), Some(&after))
            .expect("the bounded text change has an exact diff")
            .to_string();

        assert!(rendered.contains("\"source\":\"before-after\""));
        assert!(rendered.contains(&format!("\"version_id\":\"{}\"", before.version)));
        assert!(rendered.contains(&format!("\"content_digest\":\"{}\"", before.digest)));
        assert!(rendered.contains(&format!("\"version_id\":\"{}\"", after.version)));
        assert!(rendered.contains(&format!("\"content_digest\":\"{}\"", after.digest)));
        assert!(rendered
            .contains("\"kind\":\"removed\",\"before\":1,\"after\":null,\"text\":\"old line\""));
        assert!(rendered
            .contains("\"kind\":\"added\",\"before\":null,\"after\":1,\"text\":\"new line\""));
        assert!(rendered
            .contains("\"kind\":\"context\",\"before\":2,\"after\":2,\"text\":\"kept line\""));
    }

    struct FixtureHead;

    impl mesh_operations::HeadDerivation for FixtureHead {
        fn resulting_head(
            &self,
            _commitment: &mesh_operations::TransitionCommitment,
        ) -> mesh_operations::HeadId {
            mesh_operations::HeadId::from_bytes([0x77; 32])
        }
    }

    fn saved_changeset(root: &Path, operations: Vec<mesh_operations::Operation>) {
        let store = mesh_cas::Cas::open(root).expect("payload store");
        let changeset = mesh_operations::ChangeSetDraft::new(
            mesh_operations::WorkspaceId::from_bytes([0x01; 16]),
            mesh_operations::ActorId::from_bytes([0xa0; 32]),
            mesh_operations::SessionId::from_bytes([0x02; 16]),
            mesh_operations::ActorSequence::new(1),
            mesh_operations::Hlc::new(1_700_000_000_000, 0),
        )
        .causal_parents(mesh_operations::CausalParents::genesis())
        .base_head(mesh_operations::HeadId::from_bytes([0x03; 32]))
        .policy_epoch(mesh_operations::PolicyEpoch::new(1))
        .seal(
            operations,
            &FixtureHead,
            mesh_operations::Signature::from_bytes([0; 64]),
        );
        let bytes = mesh_operations::encode_canonical(&changeset);
        let payload = store.promote(bytes).expect("promote payload").digest();
        let payload_digest = RecordDigest::from_bytes(*payload.as_bytes());
        let record = StoredRecord::Operation(OperationRecord {
            id: payload_digest,
            actor: digest(0xa0),
            actor_sequence: 1,
            hlc_millis: 1_700_000_000_000,
            hlc_counter: 0,
            policy_epoch: 1,
            session: mesh_store::EntityUuid::from_bytes([0x02; 16]),
            payload_digest,
            parents: Vec::new(),
        });
        let mut file = RecordFile::open(&root.join(RECORD_FILE_NAME)).expect("record file");
        journal_records(&mut file, [&record]).expect("append fixture");
    }

    #[test]
    fn a_directory_with_nothing_in_it_opens_as_an_empty_workspace() {
        let root = scratch("empty");
        let open = OpenWorkspace::open(&root).expect("open");
        assert_eq!(open.boundary().records, 0);
        assert_eq!(open.operations(), 0);
        assert_eq!(open.tail(), TailResidue::Whole);
        assert!(open.entries().is_empty());
        assert!(open.conditions().is_empty());
        assert!(open.names_answered());
        assert!(!open
            .not_yet()
            .iter()
            .any(|(subject, _)| *subject == "file names and folders"));
        let shared_reason = open
            .not_yet()
            .into_iter()
            .find_map(|(subject, reason)| (subject == "shared version").then_some(reason))
            .expect("shared version refusal");
        assert_eq!(
            shared_reason,
            "no user-verified approval has advanced this workspace yet; record the exact review, then use a supported human-approval provider to confirm that saved version"
        );
        assert!(!shared_reason.contains("set up approvals"));
        assert!(!shared_reason.contains("Touch ID"));
        assert!(open.record_file().exists(), "the record file is created");
        assert!(
            open.database_file().exists(),
            "the durable index is created"
        );
        let canonical_root = root.canonicalize().expect("canonical root");
        assert_eq!(
            open.record_file(),
            canonical_root.join(".mesh/records.mesh")
        );
        assert_eq!(
            open.database_file(),
            canonical_root.join(".mesh/metadata.sqlite")
        );
        assert!(!root.join(RECORD_FILE_NAME).exists());
        assert!(!root.join(DATABASE_FILE_NAME).exists());
        assert_eq!(open.schema_version(), mesh_store::CURRENT_VERSION);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn remembered_reopen_refuses_an_uninitialized_directory_without_creating_state() {
        let root = scratch("remembered-empty");
        fs::create_dir(&root).expect("empty remembered directory");

        let refusal = OpenWorkspace::reopen_with_trusted_reviewers(
            &root,
            &crate::TrustedReviewers::default(),
        )
        .expect_err("remembered navigation must not initialize an empty directory");

        assert_eq!(refusal.code(), "workspace-unreachable");
        assert!(
            fs::read_dir(&root).expect("empty root").next().is_none(),
            "refused reopen must create no private namespace"
        );
        let opened = OpenWorkspace::open(&root).expect("explicit open still initializes");
        assert!(opened.record_file().is_file());
        drop(opened);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn remembered_reopen_refuses_a_missing_presented_journal_without_recreating_it() {
        let storage = scratch("remembered-presented-missing-journal");
        let opened = OpenWorkspace::open_presented(&storage).expect("create presented workspace");
        let presented = opened.root().as_path().to_path_buf();
        let journal = opened.record_file().to_path_buf();
        drop(opened);
        fs::remove_file(&journal).expect("remove journal");

        OpenWorkspace::reopen_with_trusted_reviewers(
            &presented,
            &crate::TrustedReviewers::default(),
        )
        .expect_err("remembered reopen must refuse the missing journal");

        assert!(!journal.exists(), "refusal must not recreate the journal");
        assert!(presented.is_dir(), "refusal must preserve working files");
        let _ = fs::remove_dir_all(&storage);
    }

    #[test]
    fn git_metadata_is_never_a_managed_workspace_path() {
        for candidate in [".git", ".git/HEAD", ".git/objects/ab/object"] {
            let refusal =
                managed_path_components(candidate, true).expect_err("Git metadata refused");
            assert_eq!(
                refusal,
                "the managed path is reserved for private workspace state"
            );
        }
        assert!(managed_path_components("project/.git/config", true).is_err());
    }

    #[test]
    fn a_presented_workspace_keeps_every_private_byte_outside_the_working_folder() {
        let storage = scratch("presented");
        let open = OpenWorkspace::open_presented(&storage).expect("create presented workspace");
        let canonical_storage = storage.canonicalize().expect("canonical private store");
        let presented = canonical_storage.join(PRESENTED_DIRECTORY_NAME);

        assert_eq!(open.root().as_path(), presented);
        assert_eq!(open.physical_root().as_path(), presented);
        assert_eq!(open.storage_root().as_path(), canonical_storage);
        assert_eq!(open.record_file(), canonical_storage.join(RECORD_FILE_NAME));
        assert_eq!(
            open.database_file(),
            canonical_storage.join(DATABASE_FILE_NAME)
        );
        assert!(storage.join(PRESENTED_LAYOUT_MARKER_NAME).is_file());
        assert!(!presented.join(STORAGE_DIRECTORY_NAME).exists());
        assert_eq!(
            fs::read_dir(&presented).expect("presented listing").count(),
            0
        );
        drop(open);

        let reopened = OpenWorkspace::open(&presented).expect("reopen from presented path");
        assert_eq!(reopened.storage_root().as_path(), canonical_storage);
        assert_eq!(
            reopened.record_file(),
            canonical_storage.join(RECORD_FILE_NAME)
        );
        assert!(!presented.join(STORAGE_DIRECTORY_NAME).exists());
        drop(reopened);
        let _ = fs::remove_dir_all(&storage);
    }

    #[test]
    fn a_legacy_mounts_workspace_reopens_without_losing_its_original_path() {
        let storage = scratch("legacy-presented-name");
        fs::create_dir_all(storage.join(mesh_store::MOUNT_DIRECTORY_NAME))
            .expect("legacy presented directory");
        fs::write(
            storage.join(PRESENTED_LAYOUT_MARKER_NAME),
            PRESENTED_LAYOUT_MARKER_BYTES,
        )
        .expect("presented marker");

        let open = OpenWorkspace::open_presented(&storage).expect("reopen legacy presentation");
        assert_eq!(
            open.root().as_path(),
            storage
                .canonicalize()
                .expect("canonical store")
                .join(mesh_store::MOUNT_DIRECTORY_NAME)
        );
        drop(open);
        let _ = fs::remove_dir_all(&storage);
    }

    #[test]
    fn two_presented_folder_names_are_refused_instead_of_guessed_between() {
        let storage = scratch("ambiguous-presented-name");
        fs::create_dir_all(storage.join(PRESENTED_DIRECTORY_NAME))
            .expect("current presented directory");
        fs::create_dir_all(storage.join(mesh_store::MOUNT_DIRECTORY_NAME))
            .expect("legacy presented directory");
        fs::write(
            storage.join(PRESENTED_LAYOUT_MARKER_NAME),
            PRESENTED_LAYOUT_MARKER_BYTES,
        )
        .expect("presented marker");

        assert!(OpenWorkspace::open_presented(&storage).is_err());
        let _ = fs::remove_dir_all(&storage);
    }

    #[cfg(unix)]
    #[test]
    fn native_discovery_exempts_only_the_exact_private_codex_navigation_link() {
        use std::os::unix::fs::{symlink, PermissionsExt as _};

        let storage = scratch("presented-codex-link");
        let open = OpenWorkspace::open_presented(&storage).expect("create presented workspace");
        let presented = open.physical_root().as_path();
        let integrations = open.storage_root().as_path().join("integrations");
        let codex = integrations.join("codex");
        fs::create_dir_all(&codex).expect("private Codex directory");
        fs::set_permissions(&integrations, fs::Permissions::from_mode(0o700))
            .expect("private integrations permissions");
        fs::set_permissions(&codex, fs::Permissions::from_mode(0o700))
            .expect("private Codex permissions");
        fs::write(codex.join("config.toml"), b"[mcp_servers.mesh]\n").expect("Codex configuration");
        fs::set_permissions(codex.join("config.toml"), fs::Permissions::from_mode(0o600))
            .expect("private configuration permissions");
        symlink("../integrations/codex", presented.join(".codex")).expect("app navigation link");
        fs::write(presented.join("agent.txt"), b"agent output\n").expect("agent output");
        symlink("agent.txt", presented.join("agent-latest")).expect("agent-created symbolic link");

        let discovery = open.native_discovery().expect("native inventory");
        assert!(discovery.complete);
        assert!(
            discovery.files.is_empty(),
            "a zero-history workspace does not invent file adoption authority"
        );
        assert!(discovery.directories.is_empty());
        assert_eq!(
            discovery
                .unsupported
                .iter()
                .map(|entry| (entry.path(), entry.kind()))
                .collect::<Vec<_>>(),
            vec![("agent-latest", "symbolic-link")]
        );

        fs::write(codex.join("agent-rule.md"), b"unreviewed private input\n")
            .expect("mutated private Codex directory");
        assert_eq!(
            open.native_discovery()
                .expect("mutated inventory")
                .unsupported
                .iter()
                .map(NativeUnsupportedEntry::path)
                .collect::<Vec<_>>(),
            vec![".codex", "agent-latest"],
            "any extra private input revokes the navigation-only exception"
        );
        let unreadable = presented.join("agent-private-output");
        fs::create_dir(&unreadable).expect("agent private output directory");
        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000))
            .expect("make native directory unreadable");
        assert!(
            !open
                .native_discovery()
                .expect("incomplete inventory")
                .complete,
            "an unreadable native directory must make the inventory incomplete"
        );
        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o700))
            .expect("restore cleanup access");

        fs::write(presented.join(".meshignore"), b"ignored-output/\n")
            .expect("exclude generated output");
        let ignored = presented.join("ignored-output");
        fs::create_dir(&ignored).expect("ignored output directory");
        fs::set_permissions(&ignored, fs::Permissions::from_mode(0o000))
            .expect("make ignored output unreadable");
        assert!(
            open.native_discovery()
                .expect("excluded directory does not participate in inventory")
                .complete,
            "an explicitly excluded subtree must not make the reviewable inventory incomplete"
        );
        fs::set_permissions(&ignored, fs::Permissions::from_mode(0o700))
            .expect("restore ignored cleanup access");

        fs::create_dir(ignored.join("keep")).expect("re-included directory");
        symlink("../../agent.txt", ignored.join("keep/link"))
            .expect("re-included unsupported link");
        fs::write(
            presented.join(".meshignore"),
            b"ignored-output/\n!ignored-output/keep\n",
        )
        .expect("re-include selected output");
        assert!(
            open.native_discovery()
                .expect("re-included inventory")
                .unsupported
                .iter()
                .any(|entry| {
                    entry.path() == "ignored-output/keep" && entry.kind() == "excluded-ancestor"
                }),
            "a re-included descendant below an excluded parent must stay visibly unsupported"
        );
        drop(open);
        let _ = fs::remove_dir_all(&storage);
    }

    #[test]
    fn a_foreign_or_linked_store_is_not_adopted_as_a_presented_workspace() {
        let foreign = scratch("foreign-presented");
        fs::create_dir_all(&foreign).expect("foreign directory");
        fs::write(foreign.join("owned.txt"), b"not Mesh").expect("foreign file");
        assert!(OpenWorkspace::open_presented(&foreign).is_err());
        assert_eq!(fs::read(foreign.join("owned.txt")).unwrap(), b"not Mesh");
        assert!(!foreign.join(mesh_store::MOUNT_DIRECTORY_NAME).exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            let target = scratch("presented-link-target");
            let linked = scratch("presented-link");
            fs::create_dir_all(&target).expect("link target");
            symlink(&target, &linked).expect("linked store path");
            assert!(OpenWorkspace::open_presented(&linked).is_err());
            assert!(fs::read_dir(&target).unwrap().next().is_none());
            let _ = fs::remove_file(&linked);
            let _ = fs::remove_dir_all(&target);
        }
        let _ = fs::remove_dir_all(&foreign);
    }

    #[test]
    fn a_noncanonical_presented_marker_refuses_reopen_without_creating_private_state() {
        let storage = scratch("bad-presented-marker");
        let presented = storage.join(mesh_store::MOUNT_DIRECTORY_NAME);
        fs::create_dir_all(&presented).expect("presented directory");
        fs::write(storage.join(PRESENTED_LAYOUT_MARKER_NAME), b"not canonical")
            .expect("bad marker");

        assert!(OpenWorkspace::open(&presented).is_err());
        assert!(!storage.join(RECORD_FILE_NAME).exists());
        assert!(!storage.join(DATABASE_FILE_NAME).exists());
        assert!(!presented.join(STORAGE_DIRECTORY_NAME).exists());
        let _ = fs::remove_dir_all(&storage);
    }

    #[test]
    fn replacing_only_private_storage_invalidates_the_open_workspace() {
        let root = scratch("replaced-private-storage");
        let open = OpenWorkspace::open(&root).expect("open namespaced workspace");
        let installation = open.installation();
        let private = root.join(STORAGE_DIRECTORY_NAME);
        let displaced = root.join(".mesh-displaced");

        fs::rename(&private, &displaced).expect("displace admitted private store");
        fs::create_dir(&private).expect("replacement private store");
        fs::write(private.join(RECORD_FILE_NAME), b"").expect("replacement journal");

        assert!(open.ensure_physical_root().is_err());
        let replacement = OpenWorkspace::open(&root).expect("open replacement generation");
        assert_ne!(replacement.installation(), installation);

        drop(replacement);
        drop(open);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_legacy_root_layout_remains_readable_without_implicit_migration() {
        let root = scratch("legacy-layout");
        fs::create_dir_all(&root).expect("root");
        fs::write(root.join(RECORD_FILE_NAME), []).expect("legacy journal");

        let open = OpenWorkspace::open(&root).expect("legacy open");

        let canonical_root = root.canonicalize().expect("canonical root");
        assert_eq!(open.record_file(), canonical_root.join(RECORD_FILE_NAME));
        assert_eq!(
            open.database_file(),
            canonical_root.join(DATABASE_FILE_NAME)
        );
        assert!(!root.join(STORAGE_DIRECTORY_NAME).exists());
    }

    #[test]
    fn ambiguous_legacy_and_namespaced_storage_refuses_without_mutation() {
        let root = scratch("ambiguous-layout");
        fs::create_dir_all(root.join(STORAGE_DIRECTORY_NAME)).expect("private namespace");
        fs::write(root.join(RECORD_FILE_NAME), []).expect("legacy journal");
        fs::write(root.join(STORAGE_DIRECTORY_NAME).join(RECORD_FILE_NAME), [])
            .expect("namespaced journal");

        let failure = OpenWorkspace::open(&root).expect_err("ambiguous layouts must refuse");

        assert_eq!(failure.code(), "workspace-unreachable");
        assert!(failure.to_string().contains("refusing to guess"));
    }

    #[test]
    fn an_existing_unowned_dot_mesh_directory_is_not_adopted() {
        let root = scratch("foreign-private-layout");
        fs::create_dir_all(root.join(STORAGE_DIRECTORY_NAME)).expect("foreign namespace");
        fs::write(root.join(STORAGE_DIRECTORY_NAME).join("user-data"), b"keep")
            .expect("foreign file");

        let failure = OpenWorkspace::open(&root).expect_err("foreign .mesh must refuse");

        assert_eq!(failure.code(), "workspace-unreachable");
        assert_eq!(
            fs::read(root.join(STORAGE_DIRECTORY_NAME).join("user-data")).expect("foreign bytes"),
            b"keep"
        );
        assert!(!root
            .join(STORAGE_DIRECTORY_NAME)
            .join(RECORD_FILE_NAME)
            .exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_private_namespace_is_never_followed() {
        use std::os::unix::fs::symlink;

        let root = scratch("linked-private-layout");
        let outside = scratch("linked-private-target");
        fs::create_dir_all(&root).expect("root");
        fs::create_dir_all(&outside).expect("outside");
        symlink(&outside, root.join(STORAGE_DIRECTORY_NAME)).expect("linked namespace");

        let failure = OpenWorkspace::open(&root).expect_err("linked storage must refuse");

        assert_eq!(failure.code(), "workspace-unreachable");
        assert!(!outside.join(RECORD_FILE_NAME).exists());
    }

    #[test]
    fn restart_reuses_or_rebuilds_the_durable_index_from_the_record_journal() {
        fn persisted_operation_rows(database: &Path) -> usize {
            let mut driver = Sqlite::open(database).expect("inspect durable index");
            mesh_store::read_all_tables(&mut driver)
                .expect("read durable index")
                .into_iter()
                .find_map(|(table, rows)| (table == "operation").then_some(rows.len()))
                .expect("operation table")
        }

        let root = scratch("durable-index-restart");
        let records = [operation(1, 9, 1), operation(2, 9, 2)];
        let mut file = RecordFile::open(&root.join(RECORD_FILE_NAME)).expect("open file");
        journal_records(&mut file, records.iter()).expect("append");
        drop(file);

        let first = OpenWorkspace::open(&root).expect("first open");
        let expected_digest = first.digest();
        let expected_rows = first.rows_per_table().to_vec();
        let database = first.database_file().to_path_buf();
        assert_eq!(first.operations(), 2);
        drop(first);
        assert_eq!(persisted_operation_rows(&database), 2);

        let restarted = OpenWorkspace::open(&root).expect("restart");
        assert_eq!(restarted.digest(), expected_digest);
        assert_eq!(restarted.rows_per_table(), expected_rows);
        drop(restarted);

        fs::write(&database, b"not a sqlite database").expect("corrupt disposable index");
        let rebuilt = OpenWorkspace::open(&root).expect("rebuild corrupt index");
        assert_eq!(rebuilt.operations(), 2);
        assert_eq!(rebuilt.digest(), expected_digest);
        assert_eq!(rebuilt.rows_per_table(), expected_rows);
        assert_eq!(rebuilt.schema_version(), mesh_store::CURRENT_VERSION);
        assert!(
            fs::read(&database)
                .expect("rebuilt database")
                .starts_with(b"SQLite format 3\0"),
            "the corrupt index was replaced with SQLite"
        );
        drop(rebuilt);
        assert_eq!(persisted_operation_rows(&database), 2);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn records_appended_to_the_file_are_read_back_as_state() {
        let root = scratch("records");
        fs::create_dir_all(&root).expect("mkdir");
        let mut file = RecordFile::open(&root.join(RECORD_FILE_NAME)).expect("open file");
        let records = [operation(1, 9, 1), operation(2, 9, 2), operation(3, 8, 1)];
        journal_records(&mut file, records.iter()).expect("append");

        let open = OpenWorkspace::open(&root).expect("open");
        assert_eq!(open.boundary().records, 3);
        assert_eq!(open.operations(), 3);
        assert_eq!(open.actors(), 2, "two distinct actors wrote those three");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_interrupted_append_is_reported_and_everything_before_it_is_served() {
        let root = scratch("fragment");
        fs::create_dir_all(&root).expect("mkdir");
        let path = root.join(RECORD_FILE_NAME);
        let mut file = RecordFile::open(&path).expect("open file");
        journal_records(&mut file, [&operation(1, 9, 1)]).expect("append");
        // Half of a second record, as a kill during an append leaves behind.
        let half = frame_record(&operation(2, 9, 2));
        file.append(&half[..half.len() / 2]).expect("partial");

        let open = OpenWorkspace::open(&root).expect("open");
        assert_eq!(open.boundary().records, 1);
        assert!(open.tail().is_fragment());
        assert!(open.diagnostic().is_serving(), "a fragment is not damage");
        let _ = fs::remove_dir_all(&root);
    }

    /// The bytes a crash during the *first* save leaves: a fragment with nothing before it. An
    /// empty folder and this one differ by the file's content and by nothing a person can see, so
    /// the difference has to be in what Mesh says.
    #[test]
    fn unfinished_bytes_with_no_whole_record_before_them_are_refused_rather_than_served_as_empty() {
        let root = scratch("nothing-readable");
        fs::create_dir_all(&root).expect("mkdir");
        let half = frame_record(&operation(1, 9, 1));
        let kept = half.len() / 2;
        fs::write(root.join(RECORD_FILE_NAME), &half[..kept]).expect("write");

        let failure = OpenWorkspace::open(&root).expect_err("no boundary to open at");
        assert_eq!(failure.code(), "workspace-nothing-readable");
        assert_eq!(failure.readable_boundary().records, 0);
        let OpenFailure::NothingReadable { unfinished_bytes } = failure else {
            panic!("expected a refusal naming the unfinished bytes");
        };
        assert_eq!(unfinished_bytes, kept as u64);

        // The bytes are still there: a refusal never truncates.
        assert_eq!(
            fs::read(root.join(RECORD_FILE_NAME)).expect("read").len(),
            kept
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_record_whose_bytes_are_wrong_is_a_typed_failure_and_not_a_panic() {
        let root = scratch("damaged");
        fs::create_dir_all(&root).expect("mkdir");
        let path = root.join(RECORD_FILE_NAME);
        let mut framed = frame_record(&operation(1, 9, 1));
        let last = framed.len() - 1;
        framed[last] ^= 0xFF;
        fs::write(&path, &framed).expect("write");

        let failure = OpenWorkspace::open(&root).expect_err("damaged");
        assert_eq!(failure.code(), "workspace-damaged");
        assert_eq!(
            failure.readable_boundary().records,
            0,
            "the damage is the first record, so nothing precedes it"
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// Damage after some whole records still reports how far the file was good, which is the
    /// answer to "what survived" in the one case where nothing can be folded.
    #[test]
    fn damage_after_whole_records_reports_the_boundary_before_it() {
        let root = scratch("damaged-tail");
        fs::create_dir_all(&root).expect("mkdir");
        let mut bytes = frame_record(&operation(1, 9, 1));
        let intact = bytes.len();
        let mut second = frame_record(&operation(2, 9, 2));
        let last = second.len() - 1;
        second[last] ^= 0xFF;
        bytes.extend_from_slice(&second);
        fs::write(root.join(RECORD_FILE_NAME), &bytes).expect("write");

        let failure = OpenWorkspace::open(&root).expect_err("damaged");
        assert_eq!(failure.code(), "workspace-damaged");
        assert_eq!(failure.readable_boundary().records, 1);
        assert_eq!(failure.readable_boundary().byte_offset, intact as u64);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn every_refusal_names_a_subject_and_a_reason() {
        assert!(!NOT_YET.is_empty());
        for (subject, reason) in NOT_YET.iter().copied().chain([FILE_NAMES_NOT_YET]) {
            assert!(!subject.is_empty());
            assert!(reason.len() > 20, "`{subject}` has no reason worth reading");
        }
    }

    #[test]
    fn no_subject_is_both_answered_and_refused_and_none_falls_off_both_lists() {
        let root = scratch("subject-partition");
        let workspace = OpenWorkspace::open(&root).expect("open");
        assert!(
            subjects_are_partitioned(&workspace),
            "SUBJECTS and NOT_YET disagree about what this build can answer"
        );
        assert!(workspace.subjects().iter().any(|(_, answered)| *answered));
        assert!(workspace.subjects().iter().any(|(_, answered)| !*answered));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn no_refusal_still_blames_a_crate_this_build_now_depends_on() {
        // The defect this task existed to remove: a refusal that tells a person about our build
        // instead of about their workspace. `mesh-state` and `mesh-materializer` are dependencies
        // now, so naming either as the obstacle would be false as well as unhelpful.
        for (subject, reason) in NOT_YET.iter().copied().chain([FILE_NAMES_NOT_YET]) {
            for crate_name in ["mesh-materializer", "mesh-state", "mesh-types"] {
                assert!(
                    !reason.contains(crate_name),
                    "`{subject}` blames `{crate_name}`, which this crate depends on"
                );
            }
            assert!(
                !reason.contains("not a dependency"),
                "`{subject}` still refuses by naming a dependency edge"
            );
        }
    }

    #[test]
    fn an_opened_workspace_answers_a_version_derived_from_its_own_records() {
        let root = scratch("private-version");
        let records = [operation(0x04, 0x02, 1), operation(0x05, 0x02, 2)];
        let mut file = RecordFile::open(&root.join(RECORD_FILE_NAME)).expect("open file");
        journal_records(&mut file, records.iter()).expect("append");

        let open = OpenWorkspace::open(&root).expect("open");
        let empty_root = scratch("private-version-empty");
        let empty = OpenWorkspace::open(&empty_root).expect("open");

        assert_eq!(open.private_version().changes_applied(), 2);
        assert_ne!(
            open.private_version().version(),
            empty.private_version().version(),
            "two different record sets do not name one version"
        );
        assert!(open.private_version().order().agreed());

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&empty_root);
    }

    #[test]
    fn saved_payloads_materialize_names_and_folders_without_working_directory_files() {
        use mesh_operations::{
            ManifestId, NormalizedName, ObjectId, Operation, PortableMetadata, VersionId,
        };

        let root = scratch("materialized-names");
        let root_object = ObjectId::from_bytes([0x10; 16]);
        let folder = ObjectId::from_bytes([0x20; 16]);
        let file = ObjectId::from_bytes([0x30; 16]);
        let earlier = VersionId::from_bytes([0x3f; 32]);
        let version = VersionId::from_bytes([0x40; 32]);
        saved_changeset(
            &root,
            vec![
                Operation::CreateDirectory { object_id: folder },
                Operation::LinkDirectoryEntry {
                    directory_id: root_object,
                    name: NormalizedName::new("docs").unwrap(),
                    object_id: folder,
                    version_id: VersionId::from_bytes([0x21; 32]),
                },
                Operation::CreateFile { object_id: file },
                Operation::WriteFileVersion {
                    object_id: file,
                    version_id: earlier,
                    parent_versions: vec![],
                    manifest_id: ManifestId::from_bytes([0x4f; 32]),
                    portable_metadata: PortableMetadata::default(),
                },
                Operation::WriteFileVersion {
                    object_id: file,
                    version_id: version,
                    parent_versions: vec![earlier],
                    manifest_id: ManifestId::from_bytes([0x50; 32]),
                    portable_metadata: PortableMetadata::default(),
                },
                Operation::LinkDirectoryEntry {
                    directory_id: folder,
                    name: NormalizedName::new("readme.md").unwrap(),
                    object_id: file,
                    version_id: version,
                },
            ],
        );

        assert!(!root.join("docs").exists());
        assert!(!root.join("readme.md").exists());
        let recovery = crate::ipc::nothing_to_recover();
        let daemon = crate::LiveDaemon::new(crate::ipc::StartupSummary::from(&recovery));
        let opened = crate::ipc::Operations::open_workspace(&daemon, &root.display().to_string())
            .expect("workspace.open");
        let state = crate::ipc::Operations::workspace_state(&daemon).expect("workspace.state");

        assert_eq!(opened.records, 1, "one ChangeSet is one raw record");
        assert_eq!(state.records, 1, "workspace.state retains the raw count");
        assert_eq!(state.operations, 1);
        assert_eq!(
            state.entries,
            vec![
                WorkspaceEntry {
                    path: "docs".to_owned(),
                    entry_type: "folder",
                },
                WorkspaceEntry {
                    path: "docs/readme.md".to_owned(),
                    entry_type: "file",
                },
            ]
        );
        assert_eq!(state.file_histories.len(), 1);
        assert_eq!(state.file_histories[0].path(), "docs/readme.md");
        assert_eq!(state.file_histories[0].object(), file);
        assert_eq!(
            state.file_histories[0]
                .current()
                .map(|value| value.version()),
            Some(version)
        );
        assert_eq!(
            state.file_histories[0]
                .retained()
                .iter()
                .map(|value| value.version())
                .collect::<Vec<_>>(),
            vec![earlier, version]
        );
        assert!(state.conditions.is_empty());
        let subjects: Vec<&str> = state
            .not_yet
            .iter()
            .map(|(subject, _)| subject.as_str())
            .collect();
        assert_eq!(subjects, vec!["shared version"]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_payload_is_a_named_recoverable_partial_answer() {
        use mesh_operations::{
            ManifestId, NormalizedName, ObjectId, Operation, PortableMetadata, VersionId,
        };

        let root = scratch("missing-payload");
        let file = ObjectId::from_bytes([0x31; 16]);
        let version = VersionId::from_bytes([0x41; 32]);
        saved_changeset(
            &root,
            vec![
                Operation::CreateFile { object_id: file },
                Operation::WriteFileVersion {
                    object_id: file,
                    version_id: version,
                    parent_versions: vec![],
                    manifest_id: ManifestId::from_bytes([0x51; 32]),
                    portable_metadata: PortableMetadata::default(),
                },
                Operation::LinkDirectoryEntry {
                    directory_id: ObjectId::from_bytes([0x11; 16]),
                    name: NormalizedName::new("kept.txt").unwrap(),
                    object_id: file,
                    version_id: version,
                },
            ],
        );
        let mut journal = RecordFile::open(&root.join(RECORD_FILE_NAME)).expect("record file");
        journal_records(&mut journal, [&operation(0xee, 9, 1)]).expect("append missing fixture");

        let open = OpenWorkspace::open(&root).expect("workspace stays open");
        assert_eq!(
            open.entries(),
            &[WorkspaceEntry {
                path: "kept.txt".to_owned(),
                entry_type: "file",
            }],
            "the readable saved change remains useful"
        );
        assert_eq!(open.conditions().len(), 1);
        assert_eq!(open.conditions()[0].code(), "operation-payload-missing");
        assert!(open.conditions()[0].recoverable());
        assert!(!open.names_answered());
        assert!(
            open.file_histories().is_empty(),
            "partial payload recovery cannot publish a plausible history list"
        );
        assert!(open
            .not_yet()
            .iter()
            .any(|(subject, _)| *subject == "file names and folders"));
        assert!(matches!(
            open.preview_file_restore(file, version),
            Err(RestorePreviewFailure::IncompleteProjection { ref conditions })
                if conditions == &["operation-payload-missing"]
        ));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_verified_payload_with_invalid_changeset_bytes_is_named_and_recoverable() {
        let root = scratch("invalid-payload");
        let store = mesh_cas::Cas::open(&root).expect("payload store");
        let promoted = store
            .promote(vec![0xff])
            .expect("promote invalid canonical bytes")
            .digest();
        let payload_digest = RecordDigest::from_bytes(*promoted.as_bytes());
        let record = StoredRecord::Operation(OperationRecord {
            id: digest(0xd1),
            actor: digest(0xa0),
            actor_sequence: 1,
            hlc_millis: 1_700_000_000_000,
            hlc_counter: 0,
            policy_epoch: 1,
            session: no_session(),
            payload_digest,
            parents: Vec::new(),
        });
        let mut file = RecordFile::open(&root.join(RECORD_FILE_NAME)).expect("record file");
        journal_records(&mut file, [&record]).expect("append fixture");

        let open = OpenWorkspace::open(&root).expect("workspace stays open");
        assert!(open.entries().is_empty());
        assert_eq!(open.conditions().len(), 1);
        assert_eq!(open.conditions()[0].code(), "operation-payload-invalid");
        assert!(open.conditions()[0].recoverable());
        assert!(!open.names_answered());
        assert!(open
            .not_yet()
            .iter()
            .any(|(subject, _)| *subject == "file names and folders"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_complete_payload_set_with_no_root_is_reported_loudly() {
        let root = scratch("root-missing");
        saved_changeset(
            &root,
            vec![mesh_operations::Operation::CreateFile {
                object_id: mesh_operations::ObjectId::from_bytes([1; 16]),
            }],
        );

        let open = OpenWorkspace::open(&root).expect("open with condition");
        assert_eq!(open.conditions()[0].code(), "workspace-root-missing");
        assert!(!open.names_answered());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn more_than_one_uncreated_referenced_directory_is_not_guessed() {
        use mesh_operations::{NormalizedName, ObjectId, Operation, VersionId};

        let root = scratch("root-ambiguous");
        saved_changeset(
            &root,
            vec![
                Operation::LinkDirectoryEntry {
                    directory_id: ObjectId::from_bytes([1; 16]),
                    name: NormalizedName::new("one").unwrap(),
                    object_id: ObjectId::from_bytes([3; 16]),
                    version_id: VersionId::from_bytes([4; 32]),
                },
                Operation::LinkDirectoryEntry {
                    directory_id: ObjectId::from_bytes([2; 16]),
                    name: NormalizedName::new("two").unwrap(),
                    object_id: ObjectId::from_bytes([5; 16]),
                    version_id: VersionId::from_bytes([6; 32]),
                },
            ],
        );

        let open = OpenWorkspace::open(&root).expect("open with condition");
        assert_eq!(open.conditions()[0].code(), "workspace-root-ambiguous");
        assert!(open.entries().is_empty());
        let _ = fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod historical_authoring_tests;
