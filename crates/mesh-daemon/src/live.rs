//! The running daemon: the one [`crate::ipc::Operations`] that has a real workspace behind it.
//!
//! # What this composes
//!
//! [`crate::workspace::OpenWorkspace`] on one side — records on disk, folded by `mesh-store` —
//! and [`crate::ipc::EventFeed`] on the other, so that opening a workspace is both an answer to
//! the caller and a thing every subscribed connection hears about. Nothing else. The transport
//! does not appear here and this does not appear in the transport; [`crate::ipc::Operations`] is
//! the whole of the seam between them.
//!
//! # Why the workspace is behind a lock
//!
//! `Operations` takes `&self` and is `Send + Sync`, because the transport serves one connection
//! per thread. One `Mutex` around the open workspace is the smallest thing that satisfies that
//! and keeps `workspace.open` a single-writer operation: two connections opening two folders at
//! once serialize, and the second one wins the way any last write does.
//!
//! A poisoned lock is recovered rather than propagated. A panicked connection thread must not turn
//! into a daemon that answers nothing for the rest of its life — the surface that reports damage
//! is the one thing that has to survive it.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read as _;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use mesh_cas::{Cas, CasError, DurableFs as _};
use mesh_chunking::ChunkingConfig;
use mesh_crypto::SigningPayload;
use mesh_operations::{
    HeadDerivation, HeadId as OperationHeadId, ObjectId, Operation, PortableMetadata,
    Signature as OperationSignature, TransitionCommitment, VersionId,
};
use mesh_store::{
    ApprovalRecord, BoundaryEvidenceKind, CheckpointCoordinator, CheckpointRuntimeConfig,
    CheckpointRuntimeConfigError, CheckpointRuntimeParameters, DurableBoundary,
    PendingMeaningfulSave, RecordDigest, RecoveryBoundaryEvidence, RecoveryEventUlid,
    RecoveryPreserved, RecoverySequence, RecoverySnapshot, RecoveryStamp, RecoveryTransition,
    RecoveryTrigger, ReviewRecord, ReviewVerdict, SqliteRecoveryState, StoredRecord,
    DATABASE_FILE_NAME,
};
use mesh_types::{Blake3, ContentDigest as _, DigestHasher as _, PublicKey, Signature};

use crate::checkpoint_runtime::{
    recovery_database, snapshot as checkpoint_snapshot, AutomaticCheckpointError,
    AutomaticCheckpointRuntime, AutomaticCheckpointStatus, RecoveryRuntimeSignal,
    LIVE_WORKSPACE_VIEW,
};
use crate::checkpoint_storage::{
    file_version_signing_body, operation_checkpoint_signing_body, save_authenticated_operations,
    AuthenticatedOperationCheckpointRequest, CheckpointSaveError, FileVersionCheckpointRequest,
    JournaledPrivateMutation, JournaledPrivateSave, PreparedCheckpointFile,
};
use crate::counters::{CounterSnapshot, Counters};
use crate::crash_report::CrashReport;
use crate::folder_watch::watch::DetectedFolderFile;
use crate::ipc::events::{EventBacklog, EventFeed, EventKind};
use crate::ipc::surface::{Operations, StartupSummary, Unavailable, WorkspaceSummary};
use crate::managed_file::{
    atomic_rename_noreplace_at, atomic_replace_with_mode, atomic_replace_with_mode_prepared,
    confined_existing_entry, confined_free_path, create_export_directory_in_exact_parent_prepared,
    create_export_file_in_exact_parent_prepared, inspect_managed_directory,
    inspect_managed_directory_export_target, inspect_managed_export_target, inspect_managed_file,
    inspect_managed_file_bounded, inspect_native_file_bounded, managed_directory_identity,
    read_export_replacement, read_managed_bytes, read_managed_replacement, read_managed_text,
    recover_prepared_export_directory_in_exact_parent,
    recover_prepared_export_file_in_exact_parent, remove_exact_export_directory,
    remove_exact_export_file, ManagedDirectoryExport, ManagedDirectoryExportBatchPreview,
    ManagedDirectoryExportPreview, ManagedDirectoryExportTarget, ManagedDirectoryIdentity,
    ManagedEntryChange, ManagedExportTarget, ManagedFileExport, ManagedFileExportPreview,
    ManagedFileInspection, ManagedPrivateSave, ManagedReplacementTarget,
    ManagedRetiredExportPreview, ManagedRetiredExportRemoval, ManagedTextFile,
    ManagedTextFileError, ManagedTextSave, ManagedVersionRestore, NativeDirectoryInspection,
    NativeFileInspection, NativeMissingFile, MAX_MANAGED_TEXT_BYTES,
};
use crate::managed_mutation::{ManagedMutationIntent, ManagedMutationSource};
use crate::pull_back_receipt::{
    proves_directory as pull_back_proves_directory, proves_file as pull_back_proves_file,
    proves_import_origin as pull_back_proves_import_origin,
    record_directory as record_pull_back_directory, record_file as record_pull_back_file,
    DirectoryReceipt as PullBackDirectoryReceipt, FileReceipt as PullBackFileReceipt,
    ImportOriginReceipt as PullBackImportOriginReceipt,
};
use crate::root_authority::PinnedWorkspaceRoot;
use crate::user_messages;

const MAX_AGENT_LIVE_PREVIEW_BYTES: usize = 32 * 1024 * 1024;
#[cfg(test)]
use crate::workspace::HistoricalWorkspaceSnapshot;
use crate::workspace::{
    workspace_storage_root, HistoricalWorkspacePreview, HistoricalWorkspaceWriteFailure,
    NativeUnsupportedEntry, OpenFailure, OpenWorkspace, RetiredWorkspaceEntry, ReviewArtifactSide,
    WorkspaceCondition, WorkspaceVersionChangeBasis,
};
use crate::TrustedReviewers;

struct LocalChangesetHead;

/// One exact native workspace directory verified against the desktop's displayed state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedManagedWorkspacePath {
    path: PathBuf,
    presented: bool,
    device: u64,
    inode: u64,
}

/// One exact regular file or directory reopened beneath a verified managed workspace.
///
/// The renderer never receives this path. Desktop-only launchers use the recorded kernel identity
/// to avoid handing a mutable caller-supplied spelling to Finder or LaunchServices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedManagedWorkspaceEntry {
    path: PathBuf,
    directory: bool,
    device: u64,
    inode: u64,
}

/// Complete read-only inspection performed while one exact agent generation remains assigned.
///
/// Bodies are deliberately omitted. The result proves that every currently discovered regular
/// file was readable and classifiable; after release the desktop performs its ordinary fresh scan
/// and reinspection before any authenticated private save.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentFinishPreflight {
    root: String,
    digest: String,
    installation: String,
    generation: String,
    managed_files: Vec<ManagedFileInspection>,
    native_files: Vec<NativeFileInspection>,
    native_directories: Vec<NativeDirectoryInspection>,
    missing_files: Vec<NativeMissingFile>,
    unsupported_entries: Vec<NativeUnsupportedEntry>,
}

/// One stable, read-only live-agent file snapshot admitted under exact custody.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentLiveFileSnapshot {
    root: String,
    digest: String,
    installation: String,
    generation: String,
    path: String,
    kind: &'static str,
    byte_count: u64,
    content_digest: String,
    executable: bool,
    text: Option<String>,
    bytes: Vec<u8>,
}

impl AgentLiveFileSnapshot {
    /// Canonical managed workspace root held during both reads.
    #[must_use]
    pub fn root(&self) -> &str {
        &self.root
    }
    /// Exact folded workspace digest held during both reads.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }
    /// Opaque physical workspace installation held during both reads.
    #[must_use]
    pub fn installation(&self) -> &str {
        &self.installation
    }
    /// Exact active agent-custody generation held during both reads.
    #[must_use]
    pub fn generation(&self) -> &str {
        &self.generation
    }
    /// Normalized root-relative file path admitted by native inventory.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
    /// Whether the file is tracked-and-modified or newly native.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        self.kind
    }
    /// Exact byte count shared by the two stable reads.
    #[must_use]
    pub const fn byte_count(&self) -> u64 {
        self.byte_count
    }
    /// BLAKE3 digest shared by the two stable reads.
    #[must_use]
    pub fn content_digest(&self) -> &str {
        &self.content_digest
    }
    /// Whether any executable bit was present during both reads.
    #[must_use]
    pub const fn executable(&self) -> bool {
        self.executable
    }
    /// Bounded UTF-8 text when safe for an inert renderer preview.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }
    /// Exact confined bytes available only to native inert preview code.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl AgentFinishPreflight {
    /// Canonical path of the exact inspected workspace.
    #[must_use]
    pub fn root(&self) -> &str {
        &self.root
    }

    /// Record-fold digest held throughout inspection.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// Opaque physical workspace installation.
    #[must_use]
    pub fn installation(&self) -> &str {
        &self.installation
    }

    /// Exact active custody generation that admitted this inspection.
    #[must_use]
    pub fn generation(&self) -> &str {
        &self.generation
    }

    /// Every present tracked regular file, without display text bodies.
    #[must_use]
    pub fn managed_files(&self) -> &[ManagedFileInspection] {
        &self.managed_files
    }

    /// Every native-only regular file, without display text bodies.
    #[must_use]
    pub fn native_files(&self) -> &[NativeFileInspection] {
        &self.native_files
    }

    /// Every native-only directory bound to its physical installation.
    #[must_use]
    pub fn native_directories(&self) -> &[NativeDirectoryInspection] {
        &self.native_directories
    }

    /// Every tracked regular file absent at its durable path.
    #[must_use]
    pub fn missing_files(&self) -> &[NativeMissingFile] {
        &self.missing_files
    }

    /// Native entries that cannot be represented or safely followed.
    #[must_use]
    pub fn unsupported_entries(&self) -> &[NativeUnsupportedEntry] {
        &self.unsupported_entries
    }
}

/// Caller-independent facts rendered by the native approval ceremony.
///
/// The browser chooses which recorded review to ask about, but it cannot supply this text or any
/// signed field. Both are recomputed while the daemon holds the exact displayed workspace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HumanApprovalPreview {
    context: mesh_approval::HumanApprovalContext,
    presentation_digest: mesh_approval::Digest32,
    change_summary: String,
    review_bundle: mesh_approval::ReviewBundle,
    approved_state: mesh_approval::WorkspaceState,
}

/// Exact immutable bytes admitted for a local, nonauthoritative artifact preview.
///
/// The desktop may ask macOS to render these bytes, but the rendering is never approval input.
/// Version and digest let the webview match the returned image to the binary review summary it
/// already displayed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewArtifact {
    path: String,
    version: mesh_approval::VersionId,
    digest: mesh_approval::Digest32,
    bytes: Vec<u8>,
}

impl ReviewArtifact {
    /// Reviewed relative path, used only to select a closed artifact family and file suffix.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Exact version repeated in the review presentation.
    #[must_use]
    pub const fn version(&self) -> mesh_approval::VersionId {
        self.version
    }

    /// Exact content digest repeated in the review presentation.
    #[must_use]
    pub const fn digest(&self) -> mesh_approval::Digest32 {
        self.digest
    }

    /// Content re-read through the reviewed version's immutable manifest and CAS chunks.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// One durable, re-verified human approval suitable for retrying a local export.
///
/// Receipt bytes come from the workspace CAS named by its immutable approval record. The browser
/// cannot supply or replace any field in this value.
pub struct DurableHumanApproval {
    receipt: Vec<u8>,
    expected: mesh_approval::ExpectedHumanApproval,
    preview: HumanApprovalPreview,
}

impl DurableHumanApproval {
    /// Canonical signed receipt bytes recovered from durable workspace content.
    #[must_use]
    pub fn receipt(&self) -> &[u8] {
        &self.receipt
    }

    /// Expected approval facts recomputed from the current immutable review.
    #[must_use]
    pub const fn expected(&self) -> &mesh_approval::ExpectedHumanApproval {
        &self.expected
    }

    /// Exact reviewed bundle and materialized state bound by the receipt.
    #[must_use]
    pub const fn preview(&self) -> &HumanApprovalPreview {
        &self.preview
    }
}

impl HumanApprovalPreview {
    pub(crate) fn from_record(
        workspace: &OpenWorkspace,
        review: &mesh_store::ReviewRecord,
    ) -> Result<Self, String> {
        let (context, change_summary, presentation_digest, review_bundle, approved_state) =
            workspace.human_approval_preview(review)?;
        Ok(Self {
            context,
            change_summary,
            presentation_digest,
            review_bundle,
            approved_state,
        })
    }

    /// Exact fields that the native receipt statement will bind.
    #[must_use]
    pub const fn context(&self) -> &mesh_approval::HumanApprovalContext {
        &self.context
    }

    /// Digest of the deterministic human-readable bundle presentation shown in the web review.
    #[must_use]
    pub const fn presentation_digest(&self) -> mesh_approval::Digest32 {
        self.presentation_digest
    }

    /// Escaped, complete path/effect/content-identity summary derived from that presentation.
    #[must_use]
    pub fn change_summary(&self) -> &str {
        &self.change_summary
    }

    /// Exact bundle recomputed from the immutable review closure.
    #[must_use]
    pub const fn review_bundle(&self) -> &mesh_approval::ReviewBundle {
        &self.review_bundle
    }

    /// Exact materialized state whose digest is bound by the review bundle.
    #[must_use]
    pub const fn approved_state(&self) -> &mesh_approval::WorkspaceState {
        &self.approved_state
    }
}

impl VerifiedManagedWorkspacePath {
    /// Canonical real directory pinned by the daemon.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether private Mesh state lives outside this ordinary project folder.
    #[must_use]
    pub const fn is_presented(&self) -> bool {
        self.presented
    }

    /// Stable kernel identity of the directory admitted with this workspace snapshot.
    ///
    /// The desktop persists this independently from the logical workspace installation so a
    /// pathname replacement cannot inherit an agent handoff.
    #[must_use]
    pub fn directory_token(&self) -> String {
        format!("{:016x}:{:016x}", self.device, self.inode)
    }

    /// Stable macOS file reference for handing the exact admitted directory to another process.
    ///
    /// The ordinary namespace remains user-facing, but a launcher must not resolve that mutable
    /// name after verification. macOS exposes each live local filesystem object through `/.vol`
    /// by device and inode, so a concurrent rename continues to identify the admitted directory
    /// and a replacement at the old name cannot inherit the launch.
    pub fn stable_agent_reference(&self) -> std::io::Result<PathBuf> {
        #[cfg(target_os = "macos")]
        let reference = PathBuf::from(format!("/.vol/{}/{}", self.device, self.inode));
        #[cfg(not(target_os = "macos"))]
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "this platform has no supported persistent directory reference",
        ));

        #[cfg(target_os = "macos")]
        {
            let metadata = fs::symlink_metadata(&reference)?;
            if metadata.file_type().is_dir()
                && !metadata.file_type().is_symlink()
                && metadata.dev() == self.device
                && metadata.ino() == self.inode
            {
                Ok(reference)
            } else {
                Err(std::io::Error::other(
                    "the stable agent reference does not name the admitted workspace",
                ))
            }
        }
    }

    /// Recheck that the native path still names the exact directory admitted by the daemon.
    ///
    /// This closes the verification-to-navigation gap for a desktop stable link. The link remains
    /// navigation only: every later mutation still binds the full workspace installation.
    pub fn ensure_current(&self) -> std::io::Result<()> {
        let metadata = fs::symlink_metadata(&self.path)?;
        if metadata.file_type().is_dir()
            && !metadata.file_type().is_symlink()
            && metadata.dev() == self.device
            && metadata.ino() == self.inode
        {
            Ok(())
        } else {
            Err(std::io::Error::other(
                "the verified native workspace directory was replaced",
            ))
        }
    }
}

impl VerifiedManagedWorkspaceEntry {
    /// Canonical display path resolved by the daemon beneath the verified workspace.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether the exact reopened entry is a directory rather than a regular file.
    #[must_use]
    pub const fn is_directory(&self) -> bool {
        self.directory
    }

    /// Recheck the mutable display spelling immediately before a native launch.
    pub fn ensure_current(&self) -> std::io::Result<()> {
        let metadata = fs::symlink_metadata(&self.path)?;
        let expected_kind = if self.directory {
            metadata.file_type().is_dir()
        } else {
            metadata.file_type().is_file()
        };
        if expected_kind
            && !metadata.file_type().is_symlink()
            && metadata.dev() == self.device
            && metadata.ino() == self.inode
        {
            Ok(())
        } else {
            Err(std::io::Error::other(
                "the verified workspace entry was replaced",
            ))
        }
    }

    /// Stable macOS filesystem reference for the exact admitted file or directory.
    pub fn stable_reference(&self) -> std::io::Result<PathBuf> {
        #[cfg(target_os = "macos")]
        let reference = PathBuf::from(format!("/.vol/{}/{}", self.device, self.inode));
        #[cfg(not(target_os = "macos"))]
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "this platform has no supported persistent entry reference",
        ));

        #[cfg(target_os = "macos")]
        {
            let metadata = fs::symlink_metadata(&reference)?;
            let expected_kind = if self.directory {
                metadata.file_type().is_dir()
            } else {
                metadata.file_type().is_file()
            };
            if expected_kind
                && !metadata.file_type().is_symlink()
                && metadata.dev() == self.device
                && metadata.ino() == self.inode
            {
                Ok(reference)
            } else {
                Err(std::io::Error::other(
                    "the stable workspace-entry reference does not name the admitted entry",
                ))
            }
        }
    }
}

static HISTORICAL_EXPORT_SERIAL: AtomicU64 = AtomicU64::new(1);
const MAX_WORKSPACE_VERSION_PREVIEW_ENTRIES: usize = 24;
const MAX_WORKSPACE_VERSION_PREVIEW_CHANGES: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorkspaceVersionPreviewEntry {
    Folder {
        object: mesh_materializer::ObjectId,
    },
    File {
        object: mesh_materializer::ObjectId,
        content_digest: RecordDigest,
        byte_length: u64,
        executable: bool,
    },
}

impl WorkspaceVersionPreviewEntry {
    const fn entry_type(self) -> &'static str {
        match self {
            Self::Folder { .. } => "folder",
            Self::File { .. } => "file",
        }
    }

    const fn object(self) -> mesh_materializer::ObjectId {
        match self {
            Self::Folder { object } | Self::File { object, .. } => object,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WorkspaceVersionPreviewChange {
    path: String,
    entry_type: &'static str,
    effect: &'static str,
}

fn workspace_version_preview_entries(
    snapshot: &HistoricalWorkspacePreview,
) -> BTreeMap<&str, WorkspaceVersionPreviewEntry> {
    snapshot
        .directories
        .iter()
        .map(|entry| {
            (
                entry.path.as_str(),
                WorkspaceVersionPreviewEntry::Folder {
                    object: entry.object,
                },
            )
        })
        .chain(snapshot.files.iter().map(|entry| {
            (
                entry.path.as_str(),
                WorkspaceVersionPreviewEntry::File {
                    object: entry.object,
                    content_digest: entry.content_digest,
                    byte_length: entry.byte_length,
                    executable: entry.executable,
                },
            )
        }))
        .collect()
}

fn workspace_version_preview_changes(
    basis: Option<&HistoricalWorkspacePreview>,
    selected: &HistoricalWorkspacePreview,
) -> Vec<WorkspaceVersionPreviewChange> {
    let before = basis
        .map(workspace_version_preview_entries)
        .unwrap_or_default();
    let after = workspace_version_preview_entries(selected);
    let paths = before
        .keys()
        .copied()
        .chain(after.keys().copied())
        .collect::<BTreeSet<_>>();
    paths
        .into_iter()
        .filter_map(|path| match (before.get(path), after.get(path)) {
            (None, Some(entry)) => Some(WorkspaceVersionPreviewChange {
                path: path.to_owned(),
                entry_type: entry.entry_type(),
                effect: "added",
            }),
            (Some(entry), None) => Some(WorkspaceVersionPreviewChange {
                path: path.to_owned(),
                entry_type: entry.entry_type(),
                effect: "removed",
            }),
            (Some(left), Some(right)) if left != right => Some(WorkspaceVersionPreviewChange {
                path: path.to_owned(),
                entry_type: right.entry_type(),
                effect: if left.entry_type() != right.entry_type()
                    || left.object() != right.object()
                {
                    "replaced"
                } else {
                    "changed"
                },
            }),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
thread_local! {
    static BEFORE_HISTORICAL_EXPORT_REMOVE: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
    static AFTER_REOPEN_DIRECTORY_LOCK: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
    static AFTER_MANAGED_MOVE_PERSIST: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
    static AFTER_VERSION_FORK_CONFIRM: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
    static BETWEEN_AGENT_LIVE_FILE_READS: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
}

#[cfg(test)]
fn run_before_historical_export_remove() {
    BEFORE_HISTORICAL_EXPORT_REMOVE.with(|hook| {
        if let Some(hook) = hook.borrow_mut().take() {
            hook();
        }
    });
}

#[cfg(test)]
fn run_after_reopen_directory_lock() {
    AFTER_REOPEN_DIRECTORY_LOCK.with(|hook| {
        if let Some(hook) = hook.borrow_mut().take() {
            hook();
        }
    });
}

#[cfg(test)]
fn run_between_agent_live_file_reads() {
    BETWEEN_AGENT_LIVE_FILE_READS.with(|hook| {
        if let Some(hook) = hook.borrow_mut().take() {
            hook();
        }
    });
}

#[cfg(not(test))]
fn run_after_reopen_directory_lock() {}

#[cfg(test)]
fn run_after_version_fork_confirm() {
    AFTER_VERSION_FORK_CONFIRM.with(|hook| {
        if let Some(hook) = hook.borrow_mut().take() {
            hook();
        }
    });
}

#[cfg(not(test))]
fn run_after_version_fork_confirm() {}

#[cfg(not(test))]
fn run_before_historical_export_remove() {}

struct TemporaryHistoricalExport {
    root: PathBuf,
    pinned_root: PinnedWorkspaceRoot,
    pinned_parent: PinnedWorkspaceRoot,
    device: u64,
    inode: u64,
}

impl TemporaryHistoricalExport {
    fn create_root(
        destination: &Path,
        protected_roots: &[crate::ProtectedWorkspaceRoot],
        expected_parent: Option<crate::ProtectedWorkspaceRoot>,
    ) -> Result<Self, Unavailable> {
        let destination = if destination.is_absolute() {
            destination.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|_| workspace_version_refusal("workspace-version-destination-invalid"))?
                .join(destination)
        };
        let parent = destination
            .parent()
            .ok_or_else(|| workspace_version_refusal("workspace-version-destination-invalid"))?
            .to_path_buf();
        let pinned_parent = PinnedWorkspaceRoot::open(parent.clone())
            .map_err(|_| workspace_version_refusal("workspace-version-destination-invalid"))?;
        if let Some(expected) = expected_parent {
            pinned_parent
                .ensure_protected_identity(expected)
                .map_err(|_| workspace_version_refusal("workspace-version-parent-changed"))?;
        }
        for protected in protected_roots {
            if pinned_parent
                .is_within(*protected)
                .map_err(|_| workspace_version_refusal("workspace-version-destination-invalid"))?
            {
                return Err(Unavailable::new(
                    "workspace-version-destination-overlaps-remembered",
                    "Choose a new location outside every workspace and original project already remembered by Mesh. No folder was created.",
                ));
            }
        }
        let mut created = None;
        for _ in 0..64 {
            let serial = HISTORICAL_EXPORT_SERIAL.fetch_add(1, Ordering::Relaxed);
            let name = format!(".mesh-version-export-{}-{serial}", std::process::id());
            match pinned_parent.create_child_directory(std::ffi::OsStr::new(&name)) {
                Ok(pinned_root) => {
                    created = Some((parent.join(name), pinned_root));
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => {
                    return Err(workspace_version_refusal(
                        "workspace-version-export-unavailable",
                    ))
                }
            }
        }
        let (root, pinned_root) = created
            .ok_or_else(|| workspace_version_refusal("workspace-version-export-unavailable"))?;
        let (device, inode) = pinned_root
            .identity()
            .map_err(|_| workspace_version_refusal("workspace-version-export-unavailable"))?;
        Ok(Self {
            root,
            pinned_root,
            pinned_parent,
            device,
            inode,
        })
    }

    fn create_streaming(
        destination: &Path,
        source: &OpenWorkspace,
        snapshot: &HistoricalWorkspacePreview,
        protected_roots: &[crate::ProtectedWorkspaceRoot],
        expected_parent: Option<crate::ProtectedWorkspaceRoot>,
    ) -> Result<Self, Unavailable> {
        let export = Self::create_root(destination, protected_roots, expected_parent)?;
        export.write_streaming(source, snapshot)?;
        Ok(export)
    }

    #[cfg(test)]
    fn create(
        destination: &Path,
        snapshot: &HistoricalWorkspaceSnapshot,
    ) -> Result<Self, Unavailable> {
        let export = Self::create_root(destination, &[], None)?;
        export.write(snapshot)?;
        Ok(export)
    }

    #[cfg(test)]
    fn write(&self, snapshot: &HistoricalWorkspaceSnapshot) -> Result<(), Unavailable> {
        self.pinned_root
            .ensure_identity(self.device, self.inode)
            .map_err(|_| workspace_version_refusal("workspace-version-export-unavailable"))?;
        self.pinned_root
            .ensure_namespace_identity()
            .map_err(|_| workspace_version_refusal("workspace-version-export-unavailable"))?;
        let filesystem = self.pinned_root.filesystem();
        for directory in &snapshot.directories {
            let path = safe_export_path(&directory.path)?;
            filesystem
                .create_dir_all(&path)
                .map_err(|_| workspace_version_refusal("workspace-version-export-unavailable"))?;
        }
        for file in &snapshot.files {
            let path = safe_export_path(&file.path)?;
            if let Some(parent) = path.parent() {
                filesystem.create_dir_all(parent).map_err(|_| {
                    workspace_version_refusal("workspace-version-export-unavailable")
                })?;
            }
            filesystem
                .write_new_file(
                    &path,
                    &file.bytes,
                    fs::Permissions::from_mode(if file.executable { 0o755 } else { 0o644 }),
                )
                .map_err(|_| workspace_version_refusal("workspace-version-export-unavailable"))?;
        }
        filesystem
            .sync_dir(Path::new(""))
            .map_err(|_| workspace_version_refusal("workspace-version-export-unavailable"))
    }

    fn write_streaming(
        &self,
        source: &OpenWorkspace,
        snapshot: &HistoricalWorkspacePreview,
    ) -> Result<(), Unavailable> {
        self.pinned_root
            .ensure_identity(self.device, self.inode)
            .map_err(|_| workspace_version_refusal("workspace-version-export-unavailable"))?;
        self.pinned_root
            .ensure_namespace_identity()
            .map_err(|_| workspace_version_refusal("workspace-version-export-unavailable"))?;
        let filesystem = self.pinned_root.filesystem();
        for directory in &snapshot.directories {
            let path = safe_export_path(&directory.path)?;
            filesystem
                .create_dir_all(&path)
                .map_err(|_| workspace_version_refusal("workspace-version-export-unavailable"))?;
        }
        for file in &snapshot.files {
            let path = safe_export_path(&file.path)?;
            if let Some(parent) = path.parent() {
                filesystem.create_dir_all(parent).map_err(|_| {
                    workspace_version_refusal("workspace-version-export-unavailable")
                })?;
            }
            let mut retained_failure = None;
            let write = filesystem.write_new_file_with(
                &path,
                fs::Permissions::from_mode(if file.executable { 0o755 } else { 0o644 }),
                |output| {
                    source
                        .write_historical_workspace_file(file, output)
                        .map_err(|failure| match failure {
                            HistoricalWorkspaceWriteFailure::Retained(failure) => {
                                retained_failure = Some(failure);
                                std::io::Error::other(
                                    "retained workspace content verification failed",
                                )
                            }
                            HistoricalWorkspaceWriteFailure::Output(error) => error,
                        })
                },
            );
            if retained_failure.is_some() {
                return Err(workspace_version_refusal(
                    "workspace-version-history-incomplete",
                ));
            }
            write.map_err(|_| workspace_version_refusal("workspace-version-export-unavailable"))?;
        }
        filesystem
            .sync_dir(Path::new(""))
            .map_err(|_| workspace_version_refusal("workspace-version-export-unavailable"))
    }

    fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for TemporaryHistoricalExport {
    fn drop(&mut self) {
        let Ok(metadata) = fs::symlink_metadata(&self.root) else {
            return;
        };
        if metadata.file_type().is_dir()
            && !metadata.file_type().is_symlink()
            && metadata.dev() == self.device
            && metadata.ino() == self.inode
        {
            run_before_historical_export_remove();
            let _ = quarantine_and_remove_historical_export(
                &self.root,
                &self.pinned_parent,
                self.device,
                self.inode,
            );
        }
    }
}

fn quarantine_and_remove_historical_export(
    root: &Path,
    pinned_parent: &PinnedWorkspaceRoot,
    expected_device: u64,
    expected_inode: u64,
) -> std::io::Result<()> {
    let parent_path = root
        .parent()
        .ok_or_else(|| std::io::Error::other("historical export has no parent"))?;
    let root_name = root
        .file_name()
        .ok_or_else(|| std::io::Error::other("historical export has no name"))?;
    let parent = pinned_parent.try_clone_directory()?;

    let mut quarantine = None;
    for _ in 0..64 {
        let serial = HISTORICAL_EXPORT_SERIAL.fetch_add(1, Ordering::Relaxed);
        let candidate_name = format!(".mesh-version-cleanup-{}-{serial}", std::process::id());
        match atomic_rename_noreplace_at(
            &parent,
            root_name,
            &parent,
            std::ffi::OsStr::new(&candidate_name),
        ) {
            Ok(()) => {
                quarantine = Some((candidate_name.clone(), parent_path.join(candidate_name)));
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    let (quarantine_name, quarantine_path) = quarantine
        .ok_or_else(|| std::io::Error::other("historical export cleanup name unavailable"))?;
    let moved = fs::symlink_metadata(&quarantine_path)?;
    if !moved.file_type().is_dir()
        || moved.file_type().is_symlink()
        || moved.dev() != expected_device
        || moved.ino() != expected_inode
    {
        atomic_rename_noreplace_at(
            &parent,
            std::ffi::OsStr::new(&quarantine_name),
            &parent,
            root_name,
        )?;
        return Err(std::io::Error::other(
            "historical export changed before cleanup",
        ));
    }

    fs::remove_dir_all(&quarantine_path)?;
    parent.sync_all()
}

fn safe_export_path(relative: &str) -> Result<PathBuf, Unavailable> {
    let path = Path::new(relative);
    if path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err(workspace_version_refusal(
            "workspace-version-history-invalid",
        ));
    }
    Ok(path.to_path_buf())
}

fn import_origin_proves_target(
    open: &OpenWorkspace,
    target_root: &Path,
    target_root_installation: &str,
    object: mesh_materializer::ObjectId,
    relative_path: &str,
) -> bool {
    let Ok(canonical_target) = target_root.canonicalize() else {
        return false;
    };
    let filesystem = open.storage_pinned_root().filesystem();
    let workspace_installation = open.installation();
    pull_back_proves_import_origin(
        &filesystem,
        open.storage_root().as_path(),
        &PullBackImportOriginReceipt {
            workspace_installation: &workspace_installation,
            target_root: &canonical_target,
            target_root_installation,
            object,
            path: Path::new(relative_path),
        },
    )
}

fn existing_export_target_relation(
    open: &OpenWorkspace,
    target_root: &Path,
    target_root_installation: &str,
    relative_path: &str,
    target_entry_installation: &str,
    target_bytes: &[u8],
    target_executable: bool,
) -> (&'static str, bool) {
    let imported_baseline = open
        .workspace_versions()
        .first()
        .and_then(|version| {
            open.historical_workspace_file(version.operation(), relative_path)
                .ok()
                .flatten()
        })
        .is_some_and(|file| {
            file.bytes == target_bytes
                && file.executable == target_executable
                && import_origin_proves_target(
                    open,
                    target_root,
                    target_root_installation,
                    file.object,
                    relative_path,
                )
        });
    if imported_baseline {
        return ("imported-unchanged", true);
    }

    let target_digest = RecordDigest::from_bytes(*Blake3::digest_bytes(target_bytes).as_bytes());
    let filesystem = open.storage_pinned_root().filesystem();
    let workspace_installation = open.installation();
    let pulled_back = open
        .retained_durable_file_versions_matching(relative_path, target_bytes, target_executable)
        .is_ok_and(|versions| {
            versions.iter().any(|version| {
                pull_back_proves_file(
                    &filesystem,
                    open.storage_root().as_path(),
                    &PullBackFileReceipt {
                        workspace_installation: &workspace_installation,
                        target_root_installation,
                        path: relative_path,
                        source_version: version,
                        source_digest: target_digest,
                        source_executable: target_executable,
                        target_entry_installation,
                    },
                )
            })
        });
    if pulled_back {
        ("prior-pull-back", true)
    } else {
        ("external-or-other-workspace", false)
    }
}

fn snapshot_origin_paths(
    open: &OpenWorkspace,
    snapshot: &HistoricalWorkspacePreview,
    target: &Path,
    target_installation: &str,
) -> BTreeSet<PathBuf> {
    snapshot
        .directories
        .iter()
        .map(|directory| (directory.object, directory.path.as_str()))
        .chain(
            snapshot
                .files
                .iter()
                .map(|file| (file.object, file.path.as_str())),
        )
        .filter(|(object, path)| {
            import_origin_proves_target(open, target, target_installation, *object, path)
        })
        .map(|(_, path)| PathBuf::from(path))
        .collect()
}

fn workspace_version_candidate_is_exact(
    candidate: &OpenWorkspace,
    source: &HistoricalWorkspacePreview,
    expected_origin: Option<&(PathBuf, String, BTreeSet<PathBuf>)>,
) -> bool {
    if !workspace_version_candidate_content_is_exact(candidate, source, expected_origin) {
        return false;
    }
    let recovery = SqliteRecoveryState::inspect_isolated_read_only(
        recovery_database(candidate.database_file()),
        candidate.database_file(),
        LIVE_WORKSPACE_VIEW,
    );
    match recovery {
        Ok(None) => true,
        Ok(Some(value)) => value == RecoverySnapshot::default(),
        Err(_) => false,
    }
}

fn workspace_version_candidate_content_is_exact(
    candidate: &OpenWorkspace,
    source: &HistoricalWorkspacePreview,
    expected_origin: Option<&(PathBuf, String, BTreeSet<PathBuf>)>,
) -> bool {
    if !candidate.names_answered()
        || !candidate.conditions().is_empty()
        || candidate.operations() != 1
        || candidate.private_version().concurrent_changes() != 1
    {
        return false;
    }
    let versions = candidate.workspace_versions();
    let Some(version) = versions
        .as_slice()
        .first()
        .copied()
        .filter(|_| versions.len() == 1)
    else {
        return false;
    };
    let Ok(snapshot) = candidate.historical_workspace_preview(version.operation()) else {
        return false;
    };
    if !visible_snapshots_match(source, &snapshot)
        || !native_tree_matches_snapshot(candidate, &snapshot)
    {
        return false;
    }
    if let Some((target, installation, expected_paths)) = expected_origin {
        let actual = snapshot_origin_paths(candidate, &snapshot, target, installation);
        if &actual != expected_paths {
            return false;
        }
    }
    true
}

fn visible_snapshots_match(
    left: &HistoricalWorkspacePreview,
    right: &HistoricalWorkspacePreview,
) -> bool {
    left.directories
        .iter()
        .map(|entry| entry.path.as_str())
        .eq(right.directories.iter().map(|entry| entry.path.as_str()))
        && left
            .files
            .iter()
            .map(|entry| {
                (
                    entry.path.as_str(),
                    entry.byte_length,
                    entry.content_digest,
                    entry.executable,
                )
            })
            .eq(right.files.iter().map(|entry| {
                (
                    entry.path.as_str(),
                    entry.byte_length,
                    entry.content_digest,
                    entry.executable,
                )
            }))
}

fn native_tree_matches_snapshot(
    candidate: &OpenWorkspace,
    snapshot: &HistoricalWorkspacePreview,
) -> bool {
    let root = candidate.physical_root().as_path();
    let expected_directories = snapshot
        .directories
        .iter()
        .map(|entry| PathBuf::from(&entry.path))
        .collect::<BTreeSet<_>>();
    let mut found_directories = BTreeSet::new();
    let mut found_files = BTreeSet::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            return false;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                return false;
            };
            let path = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                return false;
            };
            let Ok(relative) = path.strip_prefix(root) else {
                return false;
            };
            // Git interoperability owns one real top-level metadata directory. It is never Mesh
            // content, never traversed here, and every managed read/write path reserves `.git`.
            // Admitting only a directory (not a file or link) lets an exact saved checkout remain
            // reusable without trusting or following its independently mutable Git internals.
            if relative == Path::new(".git")
                && metadata.is_dir()
                && !metadata.file_type().is_symlink()
            {
                continue;
            }
            if metadata.file_type().is_symlink() {
                // Open in Codex adds one navigation-only link whose target is below this exact
                // workspace's private store. It is not user content, and treating it as a dirty
                // edit makes merely launching Codex defeat clean historical-checkout reuse.
                // Admit only the complete app-owned shape; every other link still makes the
                // candidate inexact and forces the caller to preserve it as a separate copy.
                if candidate.exact_private_codex_link(relative, &path) {
                    continue;
                }
                return false;
            }
            if metadata.is_dir() {
                if !expected_directories.contains(relative)
                    || !found_directories.insert(relative.to_path_buf())
                {
                    return false;
                }
                pending.push(path);
            } else if metadata.is_file() {
                let Some(expected) = snapshot
                    .files
                    .iter()
                    .find(|file| Path::new(&file.path) == relative)
                else {
                    return false;
                };
                if !found_files.insert(relative.to_path_buf())
                    || !native_file_matches_snapshot(candidate, relative, expected)
                {
                    return false;
                }
            } else {
                return false;
            }
        }
    }
    candidate.ensure_physical_root().is_ok()
        && found_directories == expected_directories
        && found_files.len() == snapshot.files.len()
}

/// Whether the current ordinary folder still exactly presents one saved point.
///
/// Unlike historical-checkout reuse, this path also supports the older co-located private layout.
/// Native discovery applies that layout's private-name fence before reporting extra content, while
/// every durable folder and file is still reopened through the confined no-follow boundary below.
fn native_review_scope_matches_snapshot(
    open: &OpenWorkspace,
    snapshot: &HistoricalWorkspacePreview,
) -> bool {
    let Ok(discovery) = open.native_discovery() else {
        return false;
    };
    if !discovery.complete
        || !discovery.files.is_empty()
        || !discovery.directories.is_empty()
        || !discovery.unsupported.is_empty()
    {
        return false;
    }

    let expected_entries = snapshot
        .directories
        .iter()
        .map(|entry| (entry.path.as_str(), "folder"))
        .chain(
            snapshot
                .files
                .iter()
                .map(|entry| (entry.path.as_str(), "file")),
        )
        .collect::<BTreeSet<_>>();
    let current_entries = open
        .entries()
        .iter()
        .map(|entry| (entry.path(), entry.entry_type()))
        .collect::<BTreeSet<_>>();
    if expected_entries != current_entries {
        return false;
    }

    let root = open.physical_root().as_path();
    if snapshot
        .directories
        .iter()
        .any(|entry| inspect_managed_directory(root, &entry.path).is_err())
    {
        return false;
    }
    snapshot.files.iter().all(|entry| {
        inspect_managed_file(
            root,
            &entry.path,
            String::new(),
            entry.content_digest,
            PortableMetadata::new(entry.executable),
        )
        .is_ok_and(|file| {
            !file.modified_from_current_version() && file.byte_count() == entry.byte_length
        })
    })
}

fn require_current_native_review_scope(
    open: &OpenWorkspace,
    target: RecordDigest,
) -> Result<(), Unavailable> {
    // Requiring the current first-publication bundle prevents a historical review whose visible
    // bytes happen to equal a later operation from being mistaken for the current saved point.
    open.first_publication_review_bundle(target).map_err(|_| {
        publication_refusal(
            "publication-review-not-computable",
            user_messages::PUBLICATION_REVIEW_NOT_COMPUTABLE,
        )
    })?;
    let snapshot = open.historical_workspace_preview(target).map_err(|_| {
        publication_refusal(
            "publication-review-not-computable",
            user_messages::PUBLICATION_REVIEW_NOT_COMPUTABLE,
        )
    })?;
    if native_review_scope_matches_snapshot(open, &snapshot) {
        Ok(())
    } else {
        Err(publication_refusal(
            "publication-native-work-pending",
            user_messages::PUBLICATION_NATIVE_WORK_PENDING,
        ))
    }
}

fn native_file_matches_snapshot(
    candidate: &OpenWorkspace,
    relative: &Path,
    expected: &crate::workspace::HistoricalWorkspacePreviewFile,
) -> bool {
    let Ok(mut file) = candidate.pinned_root().filesystem().read_file(relative) else {
        return false;
    };
    let Ok(metadata) = file.metadata() else {
        return false;
    };
    if !metadata.is_file()
        || (metadata.permissions().mode() & 0o111 != 0) != expected.executable
        || metadata.len() != expected.byte_length
    {
        return false;
    }
    let mut digest = Blake3::hasher();
    let mut byte_length = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let Ok(read) = file.read(&mut buffer) else {
            return false;
        };
        if read == 0 {
            break;
        }
        let read_length = read;
        let Ok(read) = u64::try_from(read_length) else {
            return false;
        };
        let Some(next) = byte_length.checked_add(read) else {
            return false;
        };
        byte_length = next;
        digest.update(&buffer[..read_length]);
    }
    byte_length == expected.byte_length
        && RecordDigest::from_bytes(*digest.finalize().as_bytes()) == expected.content_digest
}

fn verify_retained_manifest<F: mesh_cas::DurableFs>(
    cas: &Cas<F, mesh_cas::Blake3>,
    manifest: &mesh_store::ManifestRecord,
) -> Result<(), ManagedTextFileError> {
    let mut expected_offset = 0_u64;
    let mut content = Blake3::hasher();
    for chunk in &manifest.chunks {
        if chunk.byte_offset != expected_offset {
            return Err(ManagedTextFileError::RetainedContent(
                "the retained manifest has a gap or overlapping chunk".to_owned(),
            ));
        }
        let bytes = cas
            .read(&mesh_cas::Digest32::from_bytes(*chunk.digest.as_bytes()))
            .map_err(|error| ManagedTextFileError::RetainedContent(error.to_string()))?;
        if u64::try_from(bytes.len()).ok() != Some(chunk.byte_length) {
            return Err(ManagedTextFileError::RetainedContent(
                "a retained chunk length does not match its manifest".to_owned(),
            ));
        }
        content.update(&bytes);
        expected_offset = expected_offset
            .checked_add(chunk.byte_length)
            .ok_or_else(|| {
                ManagedTextFileError::RetainedContent(
                    "the retained manifest length overflowed".to_owned(),
                )
            })?;
    }
    if expected_offset != manifest.byte_length
        || RecordDigest::from_bytes(*content.finalize().as_bytes()) != manifest.content_digest
    {
        return Err(ManagedTextFileError::RetainedContent(
            "the reconstructed bytes do not match the retained manifest".to_owned(),
        ));
    }
    Ok(())
}

fn workspace_version_refusal(code: &'static str) -> Unavailable {
    Unavailable::new(code, "Mesh could not open that saved workspace version as a new working folder. The current workspace was not changed.")
}

fn agent_custody_refusal(error: ManagedTextFileError) -> Unavailable {
    match error {
        ManagedTextFileError::NoWorkspace => Unavailable::no_workspace_open(),
        ManagedTextFileError::StaleWorkspace => Unavailable::new(
            "stale-workspace",
            ManagedTextFileError::StaleWorkspace.to_string(),
        ),
        error => Unavailable::new("workspace-agent-custody-active", error.to_string()),
    }
}

/// One exact saved-version fork request plus the directory objects it must stay outside.
pub struct WorkspaceVersionForkRequest<'a> {
    operation: &'a str,
    destination: &'a str,
    expected_root: &'a str,
    expected_digest: &'a str,
    expected_installation: &'a str,
    origin_target: Option<&'a Path>,
    protected_roots: &'a [crate::ProtectedWorkspaceRoot],
    expected_destination_parent: Option<crate::ProtectedWorkspaceRoot>,
}

impl<'a> WorkspaceVersionForkRequest<'a> {
    /// Bind the fork to the version and exact workspace state shown to the caller.
    #[must_use]
    pub const fn new(
        operation: &'a str,
        destination: &'a str,
        expected_root: &'a str,
        expected_digest: &'a str,
        expected_installation: &'a str,
        origin_target: Option<&'a Path>,
    ) -> Self {
        Self {
            operation,
            destination,
            expected_root,
            expected_digest,
            expected_installation,
            origin_target,
            protected_roots: &[],
            expected_destination_parent: None,
        }
    }

    /// Add exact remembered workspace and project identities to the pinned transaction.
    #[must_use]
    pub const fn protecting(mut self, roots: &'a [crate::ProtectedWorkspaceRoot]) -> Self {
        self.protected_roots = roots;
        self
    }
    /// Bind creation to an admitted native parent identity, independent of its mutable pathname.
    #[must_use]
    pub const fn within_parent(mut self, parent: crate::ProtectedWorkspaceRoot) -> Self {
        self.expected_destination_parent = Some(parent);
        self
    }
}

fn validate_independent_workspace_version_destination(
    open: &OpenWorkspace,
    destination: &Path,
    original_project: Option<&Path>,
) -> Result<(), Unavailable> {
    let destination = if destination.is_absolute() {
        destination.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|_| workspace_version_refusal("workspace-version-destination-invalid"))?
            .join(destination)
    };
    let name = destination
        .file_name()
        .ok_or_else(|| workspace_version_refusal("workspace-version-destination-invalid"))?;
    let parent = destination
        .parent()
        .ok_or_else(|| workspace_version_refusal("workspace-version-destination-invalid"))?
        .canonicalize()
        .map_err(|_| workspace_version_refusal("workspace-version-destination-invalid"))?;
    let destination = parent.join(name);

    for current in [
        open.physical_root().as_path(),
        open.storage_root().as_path(),
    ] {
        let current = current
            .canonicalize()
            .map_err(|_| workspace_version_refusal("workspace-version-current-unavailable"))?;
        if destination == current
            || destination.starts_with(&current)
            || current.starts_with(&destination)
        {
            return Err(Unavailable::new(
                "workspace-version-destination-overlaps-current",
                "Choose a new location outside the workspace currently open in Mesh, then try again. No folder was created.",
            ));
        }
    }
    if let Some(original_project) = original_project {
        let original_project = original_project
            .canonicalize()
            .map_err(|_| workspace_version_refusal("workspace-version-origin-unavailable"))?;
        if destination == original_project
            || destination.starts_with(&original_project)
            || original_project.starts_with(&destination)
        {
            return Err(Unavailable::new(
                "workspace-version-destination-overlaps-original",
                "Choose a new location outside the original project, then try again. Opening a saved version never changes the original folder.",
            ));
        }
    }
    Ok(())
}

fn workspace_source_is_exact(
    open: &OpenWorkspace,
    expected_root: &str,
    expected_digest: &str,
    expected_installation: &str,
) -> bool {
    open.root().as_path() == Path::new(expected_root)
        && open.digest().to_string() == expected_digest
        && open.installation() == expected_installation
        && open.ensure_physical_root().is_ok()
}

struct ManagedReplacement<'a> {
    prior: &'a [u8],
    bytes: &'a [u8],
    executable: bool,
}

const fn checkpoint_attention_became_visible(was_visible: bool, is_visible: bool) -> bool {
    !was_visible && is_visible
}

impl HeadDerivation for LocalChangesetHead {
    fn resulting_head(&self, commitment: &TransitionCommitment) -> OperationHeadId {
        OperationHeadId::from_bytes(*Blake3::digest_bytes(&commitment.canonical_bytes()).as_bytes())
    }
}

fn authenticated_changeset_id(
    signing_body: &[u8],
    actor_public_key: PublicKey,
    signature: Signature,
) -> Result<RecordDigest, ManagedTextFileError> {
    let envelope = crate::authenticated_changeset::AuthenticatedChangeSet::verified(
        signing_body.to_vec(),
        actor_public_key,
        signature,
    )
    .map_err(|error| ManagedTextFileError::Authoring(error.to_string()))?
    .canonical_bytes();
    Ok(RecordDigest::from_bytes(
        *Blake3::digest_bytes(&envelope).as_bytes(),
    ))
}

fn local_object_id(
    domain: &[u8],
    workspace: &[u8],
    actor: &[u8],
    sequence: u64,
    path: &str,
) -> ObjectId {
    let mut statement =
        Vec::with_capacity(domain.len() + workspace.len() + actor.len() + path.len() + 16);
    statement.extend_from_slice(domain);
    statement.extend_from_slice(workspace);
    statement.extend_from_slice(actor);
    statement.extend_from_slice(&sequence.to_be_bytes());
    statement.extend_from_slice(&(path.len() as u64).to_be_bytes());
    statement.extend_from_slice(path.as_bytes());
    let digest = Blake3::digest_bytes(&statement);
    let mut id = [0_u8; 16];
    id.copy_from_slice(&digest.as_bytes()[..16]);
    ObjectId::from_bytes(id)
}

fn local_version_id(workspace: &[u8], object: &[u8], manifest: &[u8], path: &str) -> VersionId {
    let mut statement =
        Vec::with_capacity(workspace.len() + object.len() + manifest.len() + path.len() + 64);
    statement.extend_from_slice(b"mesh.local-managed-file-version/1\0");
    statement.extend_from_slice(workspace);
    statement.extend_from_slice(object);
    statement.extend_from_slice(manifest);
    statement.extend_from_slice(&(path.len() as u64).to_be_bytes());
    statement.extend_from_slice(path.as_bytes());
    VersionId::from_bytes(*Blake3::digest_bytes(&statement).as_bytes())
}

/// Exact native admission for saving one observed agent file into private history.
/// The host supplies the workspace/session identity; none of these fields grants authority alone.
pub struct AgentFileCheckpointRequest<'a> {
    /// Canonical workspace root admitted by the host.
    pub root: &'a str,
    /// Exact workspace fold before this save.
    pub digest: &'a str,
    /// Exact physical installation.
    pub installation: &'a str,
    /// Current native agent custody generation.
    pub generation: &'a str,
    /// Confined relative path discovered by native inspection.
    pub path: &'a str,
    /// Observed content identity, rechecked before durable save.
    pub content_digest: RecordDigest,
    /// Observed executable metadata.
    pub executable: bool,
    /// Whether native inventory identified a new, previously untracked regular file.
    pub new_file: bool,
}

/// Native identity for capturing the working folder of one active agent assignment.
pub struct AgentWorkspaceCheckpointRequest<'a> {
    /// Canonical admitted workspace root.
    pub root: &'a str,
    /// Exact record fold before capture begins.
    pub digest: &'a str,
    /// Exact physical installation.
    pub installation: &'a str,
    /// Current native agent custody generation.
    pub generation: &'a str,
}

/// Whole-folder observation after a bounded private capture. Never an approval receipt.
pub struct AgentWorkspaceCheckpoint {
    /// State after capture, including any durable partial progress.
    pub workspace: WorkspaceSummary,
    /// Authenticated changes saved during this invocation, in order.
    pub saved_changes: Vec<String>,
    /// True only when final native inspection found no uncaptured or unsupported changes.
    pub complete: bool,
    /// Stable explanation when complete is false. Successfully saved history is retained.
    pub issue: Option<&'static str>,
}

/// A daemon with a real workspace behind it.
#[derive(Debug)]
pub struct LiveDaemon {
    fleet: Mutex<std::collections::BTreeMap<String, Arc<crate::fleet::service::FleetService>>>,
    open: Arc<Mutex<Option<OpenWorkspace>>>,
    feed: Arc<EventFeed>,
    startup: Mutex<StartupSummary>,
    trusted_reviewers: TrustedReviewers,
    checkpoint: Arc<Mutex<LiveCheckpointRuntime>>,
    checkpoint_idle: Arc<IdleCheckpointScheduler>,
    workspace_open: Mutex<()>,
    managed_edit: Mutex<()>,
    counters: Counters,
}

/// Global managed-workspace mutation authority.
///
/// Lock order across every surface is: desktop RecentWorkspace (when present), workspace-native
/// custody, `workspace_open`, then `managed_edit`/checkpoint/open. Acquisition and release never
/// wait on RecentWorkspace. Keeping the shared lock through the process-local serial prevents a
/// rollback, workspace replacement, or agent handoff from crossing one mutation.
enum ManagedWorkspaceMutationAuthority<'a> {
    Owned {
        _context: VerifiedMutationContext,
        _custody: crate::workspace_custody::UnassignedWorkspaceGuard,
        _workspace_open: MutexGuard<'a, ()>,
    },
    VerifiedContext,
}

/// Exact workspace-native agent assignment held across one bounded native-host setup operation.
///
/// The guard keeps shared custody and the daemon's workspace replacement serial together, so a
/// concurrent release, reacquisition, rollback, or workspace switch cannot cross a pre-launch
/// filesystem write. It grants no daemon mutation API; the native host uses it only while
/// preparing the already-assigned folder for the exact generation it just acquired.
pub struct WorkspaceAgentSetupGuard<'a> {
    _custody: crate::workspace_custody::LockedAuthority,
    _workspace_open: MutexGuard<'a, ()>,
}

#[derive(Clone)]
enum MutationContext {
    Authorized(usize, String),
    Signing,
}

thread_local! {
    static VERIFIED_MUTATION_CONTEXT: RefCell<Option<MutationContext>> = const { RefCell::new(None) };
}

// A signing callback receives bytes to sign, never the enclosing capture's mutation authority.
// Restore on unwind too, before the outer authority context and custody guards are dropped.
struct SuspendedMutationContext(Option<MutationContext>);
impl SuspendedMutationContext {
    fn enter() -> Self {
        Self(
            VERIFIED_MUTATION_CONTEXT.with(|active| active.replace(Some(MutationContext::Signing))),
        )
    }
}
impl Drop for SuspendedMutationContext {
    fn drop(&mut self) {
        VERIFIED_MUTATION_CONTEXT.with(|active| active.replace(self.0.take()));
    }
}

struct VerifiedMutationContext;

impl VerifiedMutationContext {
    fn enter(daemon: &LiveDaemon, installation: &str) -> Result<Self, ManagedTextFileError> {
        VERIFIED_MUTATION_CONTEXT.with(|active| {
            let mut active = active.borrow_mut();
            if active.is_some() {
                return Err(ManagedTextFileError::Recovery(
                    "nested verified workspace mutation context was refused".to_owned(),
                ));
            }
            *active = Some(MutationContext::Authorized(
                daemon as *const LiveDaemon as usize,
                installation.to_owned(),
            ));
            Ok(Self)
        })
    }
}

impl Drop for VerifiedMutationContext {
    fn drop(&mut self) {
        VERIFIED_MUTATION_CONTEXT.with(|active| *active.borrow_mut() = None);
    }
}

/// One resettable two-deadline worker for the daemon's current pending checkpoint extent.
///
/// A successful save can arrive faster than the idle interval. Spawning one detached operating-
/// system thread for every save makes the number of sleepers proportional to edit throughput even
/// though only the newest extent may recover or settle. This state keeps exactly one worker alive:
/// it first preserves recovery at the configured maximum interval, then closes meaningfully only
/// at the later idle deadline. Every later extent resets the idle deadline through the condition
/// variable without postponing the maximum-uncheckpointed deadline.
#[derive(Debug, Default)]
struct IdleCheckpointScheduler {
    state: Mutex<IdleCheckpointSchedulerState>,
    wake: Condvar,
    drained: Condvar,
    #[cfg(test)]
    worker_gate: Mutex<Option<CheckpointWorkerTestGate>>,
}

#[cfg(test)]
#[derive(Debug)]
struct CheckpointWorkerTestGate {
    entered: std::sync::mpsc::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
    panic_after_release: bool,
}

// Declared outside the worker loop, so every upgraded database/workspace reference is dropped
// before completion is acknowledged, including when the worker unwinds.
struct CheckpointWorkerLifetime(Arc<IdleCheckpointScheduler>);

impl Drop for CheckpointWorkerLifetime {
    fn drop(&mut self) {
        let mut idle = self.0.state.lock().unwrap_or_else(PoisonError::into_inner);
        idle.active_workers -= 1;
        self.0.drained.notify_all();
    }
}

#[derive(Debug, Default)]
struct IdleCheckpointSchedulerState {
    generation: u64,
    worker_running: bool,
    active_workers: usize,
    shutdown: bool,
    scheduled: Option<IdleCheckpointSchedule>,
    maximum_started_at: Option<Instant>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IdleScheduleUpdate {
    Duplicate,
    Stale,
    WakeWorker,
    StartWorker,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct IdleCheckpointSchedule {
    through: RecoverySequence,
    installation: CheckpointInstallation,
    root: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PendingCheckpointIntervals {
    idle: Duration,
    maximum: Duration,
}

impl IdleCheckpointSchedulerState {
    fn publish(&mut self, scheduled: IdleCheckpointSchedule) -> IdleScheduleUpdate {
        self.publish_at(scheduled, Instant::now())
    }

    fn publish_at(
        &mut self,
        scheduled: IdleCheckpointSchedule,
        published_at: Instant,
    ) -> IdleScheduleUpdate {
        if self.worker_running {
            if let Some(current) = &self.scheduled {
                if scheduled.installation < current.installation {
                    return IdleScheduleUpdate::Stale;
                }
                if scheduled.installation == current.installation {
                    if scheduled.root != current.root || scheduled.through < current.through {
                        return IdleScheduleUpdate::Stale;
                    }
                    if scheduled.through == current.through {
                        return IdleScheduleUpdate::Duplicate;
                    }
                }
            }
        }
        self.generation = self.generation.saturating_add(1);
        self.scheduled = Some(scheduled);
        if self.maximum_started_at.is_none() {
            self.maximum_started_at = Some(published_at);
        }
        if self.worker_running {
            IdleScheduleUpdate::WakeWorker
        } else {
            self.worker_running = true;
            IdleScheduleUpdate::StartWorker
        }
    }

    fn retire_unpublished_replacement(
        &mut self,
        observed_generation: u64,
        current: &IdleCheckpointSchedule,
    ) -> bool {
        if self.generation != observed_generation || self.scheduled.as_ref() == Some(current) {
            return false;
        }
        self.worker_running = false;
        self.scheduled = None;
        self.maximum_started_at = None;
        true
    }

    // Pure deadline selection, shared by the real worker and deterministic clock tests.
    fn next_wait_at(
        &self,
        intervals: PendingCheckpointIntervals,
        now: Instant,
    ) -> (Duration, bool) {
        let maximum_remaining = self.maximum_remaining_at(intervals.maximum, now);
        (
            intervals.idle.min(maximum_remaining),
            maximum_remaining <= intervals.idle,
        )
    }

    fn maximum_remaining_at(&self, maximum: Duration, now: Instant) -> Duration {
        self.maximum_started_at.map_or(maximum, |started| {
            maximum.saturating_sub(now.saturating_duration_since(started))
        })
    }
}

/// A typed file-version save was refused before its acknowledgement could be returned.
#[derive(Debug)]
pub enum LiveCheckpointSaveError {
    /// Automatic checkpointing was not explicitly configured or has no open workspace.
    Checkpoint(AutomaticCheckpointError),
    /// Journal truth is durable, but its checkpoint observation did not become durable.
    ///
    /// No acknowledgement is returned. The saved file version remains authoritative on restart,
    /// while checkpoint activity for this attempt must be treated as unknown.
    ObservationAfterJournal(AutomaticCheckpointError),
    /// No workspace is currently open for the save.
    NoWorkspace,
    /// Shared workspace authority refused the save before any durable mutation.
    WorkspaceAuthority(ManagedTextFileError),
    /// This platform cannot represent the byte count in the coordinator's counter.
    ByteCountTooLarge,
    /// Adapter event sequence zero cannot name an automatic-checkpoint event.
    InvalidFolderEventSequence,
    /// The workspace content-addressed store could not be opened.
    ContentStore(CasError),
    /// CAS, metadata, or immutable journal durability refused the typed save.
    DurableSave(CheckpointSaveError<std::io::Error>),
    /// A refused save also prevented restoration from the still-authoritative journal.
    DurableSaveAndReload {
        /// The original save refusal.
        save: Box<CheckpointSaveError<std::io::Error>>,
        /// The failure to restore live state from journal truth.
        reload: Box<OpenFailure>,
    },
    /// Journal truth was durable but could not be folded back into the live workspace.
    Reload(Box<OpenFailure>),
}

impl std::fmt::Display for LiveCheckpointSaveError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Checkpoint(error) => error.fmt(formatter),
            Self::ObservationAfterJournal(error) => write!(
                formatter,
                "the file version is durable in the journal, but checkpoint activity is unknown: {error}"
            ),
            Self::NoWorkspace => formatter.write_str("no workspace is open for a typed save"),
            Self::WorkspaceAuthority(error) => error.fmt(formatter),
            Self::ByteCountTooLarge => {
                formatter.write_str("the saved byte count does not fit the checkpoint counter")
            }
            Self::InvalidFolderEventSequence => {
                formatter.write_str("folder event sequence zero cannot be checkpointed")
            }
            Self::ContentStore(error) => error.fmt(formatter),
            Self::DurableSave(error) => error.fmt(formatter),
            Self::DurableSaveAndReload { save, reload } => write!(
                formatter,
                "the save was refused ({save}) and journal truth could not be restored: {reload}"
            ),
            Self::Reload(error) => write!(
                formatter,
                "the durable journal could not be folded after the save: {error}"
            ),
        }
    }
}

impl std::error::Error for LiveCheckpointSaveError {}

#[derive(Debug)]
struct LiveCheckpointRuntime {
    parameters: Option<CheckpointRuntimeParameters>,
    active: Option<AutomaticCheckpointRuntime>,
    active_database: Option<PathBuf>,
    active_installation: Option<CheckpointInstallation>,
    next_installation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct CheckpointInstallation(u64);

impl Drop for LiveDaemon {
    fn drop(&mut self) {
        let mut idle = self
            .checkpoint_idle
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        idle.shutdown = true;
        self.checkpoint_idle.wake.notify_all();
        // Requesting shutdown does not release the worker's SQLite connections. Wait until all
        // generations have released their strong references before a caller can reopen the path.
        drop(
            self.checkpoint_idle
                .drained
                .wait_while(idle, |state| state.active_workers != 0)
                .unwrap_or_else(PoisonError::into_inner),
        );
    }
}

impl LiveCheckpointRuntime {
    const fn disabled() -> Self {
        Self {
            parameters: None,
            active: None,
            active_database: None,
            active_installation: None,
            next_installation: 0,
        }
    }

    fn configured(
        parameters: CheckpointRuntimeParameters,
    ) -> Result<Self, CheckpointRuntimeConfigError> {
        CheckpointRuntimeConfig::from_parameters(parameters)?;
        Ok(Self {
            parameters: Some(parameters),
            active: None,
            active_database: None,
            active_installation: None,
            next_installation: 0,
        })
    }

    /// Build an uninstalled successor without disturbing the currently paired workspace.
    ///
    /// Workspace open performs checks after the recovery database itself has opened. Keeping the
    /// successor private until every check passes prevents a refused replacement from leaving the
    /// old workspace paired with the replacement's coordinator.
    fn replacement_candidate(&self) -> Self {
        Self {
            parameters: self.parameters,
            active: None,
            active_database: None,
            active_installation: None,
            next_installation: self.next_installation,
        }
    }

    const fn status(&self) -> AutomaticCheckpointStatus {
        match (self.parameters, self.active.is_some()) {
            (None, _) => AutomaticCheckpointStatus::DisabledNoConfiguration,
            (Some(_), false) => AutomaticCheckpointStatus::WaitingForWorkspace,
            (Some(_), true) => AutomaticCheckpointStatus::Active,
        }
    }

    fn install(&mut self, database: &Path) -> Result<(), String> {
        let Some(parameters) = self.parameters else {
            self.active = None;
            self.active_database = None;
            self.active_installation = None;
            return Ok(());
        };
        let next_installation = self
            .next_installation
            .checked_add(1)
            .ok_or_else(|| "checkpoint workspace installation identity exhausted".to_owned())?;
        let persistence = SqliteRecoveryState::open_isolated(
            recovery_database(database),
            database,
            LIVE_WORKSPACE_VIEW,
        )
        .map_err(|error| error.to_string())?;
        let runtime = CheckpointCoordinator::open(persistence, parameters)
            .map_err(|error| error.to_string())?;
        self.active = Some(runtime);
        self.active_database = Some(database.to_path_buf());
        self.active_installation = Some(CheckpointInstallation(next_installation));
        self.next_installation = next_installation;
        Ok(())
    }

    fn validate_journal_recovery_pointer(&self, open: &mut OpenWorkspace) -> Result<(), String> {
        let Some(runtime) = self.active.as_ref() else {
            return Ok(());
        };
        let snapshot = runtime.machine().snapshot();
        let Some(recovery) = snapshot.latest_recovery() else {
            return Ok(());
        };
        let pending = snapshot.pending_meaningful();
        let boundary = verify_journal_recovery_pointer(open, pending, recovery)
            .map_err(|error| error.to_string())?;
        if let (Some(pending), Some(boundary)) = (pending, boundary) {
            open.verify_pending_private_save_at_boundary(pending, boundary)
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn preserve_pending_recovery_for_signal(
        &mut self,
        open: &mut OpenWorkspace,
        signal: RecoveryRuntimeSignal,
    ) -> Result<(), AutomaticCheckpointError> {
        match self.status() {
            AutomaticCheckpointStatus::DisabledNoConfiguration => return Ok(()),
            AutomaticCheckpointStatus::WaitingForWorkspace => {
                return Err(AutomaticCheckpointError::NoWorkspace);
            }
            AutomaticCheckpointStatus::Active => {}
        }
        open.ensure_physical_root()
            .map_err(|_| AutomaticCheckpointError::WorkspaceChanged)?;
        self.ensure_installed_for(open.database_file())?;
        if open.checkpoint_recovery_needs_attention() {
            return Err(AutomaticCheckpointError::RecoveryNeedsAttention);
        }
        let runtime = self.runtime_mut()?;
        let Some(pending) = runtime.machine().snapshot().pending_meaningful() else {
            return Ok(());
        };
        let recovery = journal_recovery_for_pending(open, pending)?;
        runtime
            .preserve(signal.trigger(), recovery)
            .map_err(AutomaticCheckpointError::Runtime)?;
        Ok(())
    }

    fn preserve_pending_recovery_after_restart(&mut self, open: &mut OpenWorkspace) {
        let Some(runtime) = self.active.as_mut() else {
            return;
        };
        if runtime.machine().snapshot().pending_meaningful().is_none() {
            return;
        }
        let elapsed = runtime.config().maximum_uncheckpointed_interval();
        if preserve_pending_recovery_if_due(runtime, open, elapsed).is_err() {
            open.record_checkpoint_recovery_attention();
        }
    }

    /// Copy legacy runtime truth before workspace open is allowed to replace a damaged index.
    fn preserve_legacy_before_index_repair(&self, root: &Path) -> Result<(), String> {
        preserve_legacy_checkpoint_before_index_repair(root)
    }

    fn uninstall(&mut self) {
        self.active = None;
        self.active_database = None;
        self.active_installation = None;
    }

    fn is_installed_for(&self, database: &Path) -> bool {
        self.active_database.as_deref() == Some(database)
    }

    fn ensure_installed_for(&self, database: &Path) -> Result<(), AutomaticCheckpointError> {
        match self.status() {
            AutomaticCheckpointStatus::DisabledNoConfiguration => {
                return Err(AutomaticCheckpointError::DisabledNoConfiguration);
            }
            AutomaticCheckpointStatus::WaitingForWorkspace => {
                return Err(AutomaticCheckpointError::NoWorkspace);
            }
            AutomaticCheckpointStatus::Active => {}
        }
        if self.is_installed_for(database) {
            Ok(())
        } else {
            Err(AutomaticCheckpointError::WorkspaceChanged)
        }
    }

    fn installation_for(
        &self,
        database: &Path,
    ) -> Result<CheckpointInstallation, AutomaticCheckpointError> {
        self.ensure_installed_for(database)?;
        self.active_installation
            .ok_or(AutomaticCheckpointError::WorkspaceChanged)
    }

    fn runtime_mut(&mut self) -> Result<&mut AutomaticCheckpointRuntime, AutomaticCheckpointError> {
        let status = self.status();
        self.active
            .as_mut()
            .ok_or_else(|| AutomaticCheckpointError::snapshot_unavailable(status))
    }

    fn runtime(&self) -> Result<&AutomaticCheckpointRuntime, AutomaticCheckpointError> {
        let status = self.status();
        self.active
            .as_ref()
            .ok_or_else(|| AutomaticCheckpointError::snapshot_unavailable(status))
    }

    fn pending_idle_schedule(
        &self,
    ) -> Option<(
        PendingCheckpointIntervals,
        RecoverySequence,
        CheckpointInstallation,
    )> {
        let runtime = self.active.as_ref()?;
        let pending = runtime.machine().snapshot().pending_meaningful()?;
        Some((
            PendingCheckpointIntervals {
                idle: runtime.config().idle_interval(),
                maximum: runtime.config().maximum_uncheckpointed_interval(),
            },
            pending.through(),
            self.active_installation?,
        ))
    }
}

fn preserve_legacy_checkpoint_before_index_repair(root: &Path) -> Result<(), String> {
    let index = workspace_storage_root(root)
        .map_err(|error| error.to_string())?
        .join(DATABASE_FILE_NAME);
    let recovery = recovery_database(&index);
    if !index.exists() && !recovery.exists() {
        return Ok(());
    }
    SqliteRecoveryState::preserve_legacy_if_present(recovery, index, LIVE_WORKSPACE_VIEW)
        .map_err(|error| error.to_string())?;
    Ok(())
}

impl LiveDaemon {
    /// Register a native-created fleet host. Agent IPC cannot install or replace hosts.
    pub fn register_fleet(
        &self,
        service: Arc<crate::fleet::service::FleetService>,
    ) -> Result<(), Unavailable> {
        let objective = service.objective()?;
        let mut hosts = self.fleet.lock().map_err(|_| {
            Unavailable::new("fleet-host-needs-recovery", "Fleet routing needs recovery.")
        })?;
        if hosts
            .get(&objective)
            .is_some_and(|registered| Arc::ptr_eq(registered, &service))
        {
            return Ok(());
        }
        if hosts.contains_key(&objective) || hosts.len() >= 16 {
            return Err(Unavailable::new(
                "fleet-host-registration-refused",
                "This fleet is already registered or the host limit was reached.",
            ));
        }
        hosts.insert(objective, service);
        Ok(())
    }

    fn lock_current_managed_workspace_mutation(
        &self,
    ) -> Result<ManagedWorkspaceMutationAuthority<'_>, ManagedTextFileError> {
        let (root, installation) = {
            let held = self.held();
            let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
            (
                open.physical_root().as_path().to_path_buf(),
                open.installation(),
            )
        };
        let verified = VERIFIED_MUTATION_CONTEXT.with(|active| active.borrow().clone());
        if matches!(verified, Some(MutationContext::Signing)) {
            return Err(ManagedTextFileError::Recovery(
                "a checkpoint signer cannot perform workspace mutations".to_owned(),
            ));
        }
        if let Some(MutationContext::Authorized(daemon, verified)) = verified {
            if daemon != self as *const LiveDaemon as usize || verified != installation {
                return Err(ManagedTextFileError::StaleWorkspace);
            }
            return Ok(ManagedWorkspaceMutationAuthority::VerifiedContext);
        }
        let custody = crate::workspace_custody::lock_for_workspace_path(&root, &installation)
            .and_then(crate::workspace_custody::LockedAuthority::require_unassigned)
            .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
        let workspace_open = self
            .workspace_open
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        {
            let held = self.held();
            let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
            if open.physical_root().as_path() != root || open.installation() != installation {
                return Err(ManagedTextFileError::StaleWorkspace);
            }
            open.ensure_physical_root()
                .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
        }
        let context = VerifiedMutationContext::enter(self, &installation)?;
        Ok(ManagedWorkspaceMutationAuthority::Owned {
            _context: context,
            _custody: custody,
            _workspace_open: workspace_open,
        })
    }
    /// A daemon that is serving and has no workspace open.
    ///
    /// `startup` is what this process found when it started. A daemon that has not opened anything
    /// still has a truthful answer to `startup.report`, and that answer is not a workspace.
    #[must_use]
    pub fn new(startup: StartupSummary) -> Self {
        Self::with_trusted_reviewers(startup, TrustedReviewers::default())
    }

    /// A daemon with an explicit set of human reviewer public keys trusted for publication.
    #[must_use]
    pub fn with_trusted_reviewers(
        startup: StartupSummary,
        trusted_reviewers: TrustedReviewers,
    ) -> Self {
        Self::compose(
            startup,
            trusted_reviewers,
            LiveCheckpointRuntime::disabled(),
        )
    }

    /// A daemon whose automatic checkpointing is enabled by three explicit non-zero parameters.
    ///
    /// # Errors
    ///
    /// [`CheckpointRuntimeConfigError`] when any parameter is absent or zero. There is no default
    /// fallback.
    pub fn with_checkpoint_runtime(
        startup: StartupSummary,
        parameters: CheckpointRuntimeParameters,
    ) -> Result<Self, CheckpointRuntimeConfigError> {
        Self::with_trusted_reviewers_and_checkpoint_runtime(
            startup,
            TrustedReviewers::default(),
            parameters,
        )
    }

    /// Compose reviewer trust and explicitly configured automatic checkpointing.
    ///
    /// # Errors
    ///
    /// [`CheckpointRuntimeConfigError`] when any parameter is absent or zero.
    pub fn with_trusted_reviewers_and_checkpoint_runtime(
        startup: StartupSummary,
        trusted_reviewers: TrustedReviewers,
        parameters: CheckpointRuntimeParameters,
    ) -> Result<Self, CheckpointRuntimeConfigError> {
        Ok(Self::compose(
            startup,
            trusted_reviewers,
            LiveCheckpointRuntime::configured(parameters)?,
        ))
    }

    /// Trust the public credential just enrolled by the native platform ceremony.
    ///
    /// This accepts no private material and is not exposed by daemon IPC. The desktop host calls
    /// it only after the fixed-tag Secure Enclave adapter returns the public key.
    pub fn trust_human_approval_credential(
        &self,
        credential: mesh_approval::HumanApprovalCredential,
    ) {
        self.trusted_reviewers.enroll_human_credential(credential);
    }

    fn compose(
        startup: StartupSummary,
        trusted_reviewers: TrustedReviewers,
        checkpoint: LiveCheckpointRuntime,
    ) -> Self {
        let daemon = Self {
            fleet: Mutex::new(std::collections::BTreeMap::new()),
            open: Arc::new(Mutex::new(None)),
            feed: Arc::new(EventFeed::new()),
            startup: Mutex::new(startup),
            trusted_reviewers,
            checkpoint: Arc::new(Mutex::new(checkpoint)),
            checkpoint_idle: Arc::new(IdleCheckpointScheduler::default()),
            workspace_open: Mutex::new(()),
            managed_edit: Mutex::new(()),
            counters: Counters::new(),
        };
        daemon.feed.publish(EventKind::Serving);
        daemon
    }

    /// Run one desktop-managed mutation only while the daemon still holds the exact workspace
    /// summary the person reviewed.
    ///
    /// The local IPC surface can replace the open workspace independently of the webview. A UI
    /// generation counter therefore cannot authorize a later native mutation by itself: the
    /// daemon must first acquire workspace-native custody, then bind the displayed root and
    /// record-fold digest under the same `workspace_open` serial that every replacement uses.
    /// Root and record-fold digest are insufficient when an exact clone is installed at the same
    /// path, so the opaque physical installation is part of the comparison. The operation then
    /// acquires `managed_edit`, preserving `custody -> workspace_open -> managed_edit`.
    pub fn with_verified_managed_workspace<T, F>(
        &self,
        expected_root: &str,
        expected_digest: &str,
        expected_installation: &str,
        operation: F,
    ) -> Result<T, ManagedTextFileError>
    where
        F: FnOnce() -> Result<T, ManagedTextFileError>,
    {
        self.with_verified_managed_workspace_requirement(
            expected_root,
            expected_digest,
            expected_installation,
            false,
            operation,
        )
    }

    /// Run one original-project export only while the exact current private version is also the
    /// protected shared version.
    ///
    /// Browser controls are presentation, not authority. This native guard composes the exact
    /// workspace binding with the durable approval fold and workspace-native custody under the
    /// same workspace replacement serial. The alpha policy conservatively refuses every export
    /// while an agent owns the working folder, even though retained export bytes are immutable:
    /// this callback API can invoke arbitrary daemon code, so it cannot safely grant a narrower
    /// read-only capability. Web content or another local caller therefore cannot smuggle an
    /// assigned-root mutation through an export authorization closure.
    pub fn with_verified_shared_managed_workspace<T, F>(
        &self,
        expected_root: &str,
        expected_digest: &str,
        expected_installation: &str,
        operation: F,
    ) -> Result<T, ManagedTextFileError>
    where
        F: FnOnce() -> Result<T, ManagedTextFileError>,
    {
        self.with_verified_managed_workspace_requirement(
            expected_root,
            expected_digest,
            expected_installation,
            true,
            operation,
        )
    }

    fn ensure_current_shared_version(open: &OpenWorkspace) -> Result<(), ManagedTextFileError> {
        let shared = open.shared_version().map(|version| version.to_string());
        if shared.as_deref() != open.private_version().version()
            || open.private_version().version().is_none()
        {
            return Err(ManagedTextFileError::OriginalExportRequiresSharedVersion);
        }
        Ok(())
    }

    fn with_verified_managed_workspace_requirement<T, F>(
        &self,
        expected_root: &str,
        expected_digest: &str,
        expected_installation: &str,
        require_current_shared_version: bool,
        operation: F,
    ) -> Result<T, ManagedTextFileError>
    where
        F: FnOnce() -> Result<T, ManagedTextFileError>,
    {
        let _custody = crate::workspace_custody::lock_for_workspace_path(
            Path::new(expected_root),
            expected_installation,
        )
        .and_then(crate::workspace_custody::LockedAuthority::require_unassigned)
        .map_err(|error| {
            if error.is_stale_workspace() {
                ManagedTextFileError::StaleWorkspace
            } else {
                ManagedTextFileError::Recovery(error.to_string())
            }
        })?;
        let _open_serial = self
            .workspace_open
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        {
            let held = self.held();
            let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
            if open.root().as_path() != Path::new(expected_root)
                || open.digest().to_string() != expected_digest
                || open.installation() != expected_installation
            {
                return Err(ManagedTextFileError::StaleWorkspace);
            }
            open.ensure_physical_root()
                .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
            if require_current_shared_version {
                Self::ensure_current_shared_version(open)?;
            }
        }
        let _context = VerifiedMutationContext::enter(self, expected_installation)?;
        operation()
    }

    /// Return the pinned physical directory for the exact managed workspace the desktop showed.
    ///
    /// The display spelling is not sufficient authority: a path may be removed and replaced, or
    /// a linked spelling may be retargeted after the UI rendered it.  This method holds the same
    /// workspace replacement serial as every native mutation, compares the record fold and opaque
    /// installation identity, rechecks the opened directory object, and only then returns the
    /// canonical directory captured by [`OpenWorkspace`].  Callers may reveal this path to an
    /// ordinary editor, but may not use the caller-supplied spelling as a filesystem capability.
    pub fn verified_managed_workspace_path(
        &self,
        expected_root: &str,
        expected_digest: &str,
        expected_installation: &str,
    ) -> Result<VerifiedManagedWorkspacePath, ManagedTextFileError> {
        let _open_serial = self
            .workspace_open
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let held = self.held();
        let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
        if open.root().as_path() != Path::new(expected_root)
            || open.digest().to_string() != expected_digest
            || open.installation() != expected_installation
        {
            return Err(ManagedTextFileError::StaleWorkspace);
        }
        open.ensure_physical_root().map_err(|source| {
            ManagedTextFileError::io(
                "verify managed workspace directory",
                open.physical_root().as_path(),
                source,
            )
        })?;
        let (device, inode) = open.physical_directory_identity();
        Ok(VerifiedManagedWorkspacePath {
            path: open.physical_root().as_path().to_path_buf(),
            presented: open
                .physical_root()
                .as_path()
                .file_name()
                .is_some_and(crate::workspace::is_presented_directory_name)
                && open.physical_root().as_path().parent() == Some(open.storage_root().as_path()),
            device,
            inode,
        })
    }

    /// Resolve one renderer-selected relative path beneath the exact managed workspace.
    ///
    /// Custody is acquired before the workspace-open serial. An unassigned caller must still be
    /// unassigned; a live-review caller must name the exact active handoff generation. The path is
    /// then reopened component-by-component without following links and its physical identity is
    /// retained for the desktop launcher.
    pub fn verified_managed_workspace_entry(
        &self,
        expected_root: &str,
        expected_digest: &str,
        expected_installation: &str,
        expected_agent_generation: Option<&str>,
        relative_path: &str,
        is_directory: bool,
    ) -> Result<VerifiedManagedWorkspaceEntry, ManagedTextFileError> {
        let custody = crate::workspace_custody::lock_for_workspace_path(
            Path::new(expected_root),
            expected_installation,
        )
        .map_err(|error| {
            if error.is_stale_workspace() {
                ManagedTextFileError::StaleWorkspace
            } else {
                ManagedTextFileError::Recovery(error.to_string())
            }
        })?;
        let resolve = || {
            let _open_serial = self
                .workspace_open
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let held = self.held();
            let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
            if open.root().as_path() != Path::new(expected_root)
                || open.digest().to_string() != expected_digest
                || open.installation() != expected_installation
            {
                return Err(ManagedTextFileError::StaleWorkspace);
            }
            open.ensure_physical_root()
                .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
            let path = confined_existing_entry(
                open.physical_root().as_path(),
                relative_path,
                is_directory,
            )?;
            let metadata = fs::symlink_metadata(&path)
                .map_err(|error| ManagedTextFileError::io("verify managed entry", &path, error))?;
            let expected_kind = if is_directory {
                metadata.file_type().is_dir()
            } else {
                metadata.file_type().is_file()
            };
            if !expected_kind || metadata.file_type().is_symlink() {
                return Err(ManagedTextFileError::NotRegularFile);
            }
            Ok(VerifiedManagedWorkspaceEntry {
                path,
                directory: is_directory,
                device: metadata.dev(),
                inode: metadata.ino(),
            })
        };
        let map_custody = |error: crate::WorkspaceAgentCustodyError| {
            if error.is_stale_workspace() {
                ManagedTextFileError::StaleWorkspace
            } else {
                ManagedTextFileError::Recovery(error.to_string())
            }
        };
        match expected_agent_generation {
            Some(generation) => {
                let _custody = custody
                    .require_generation(generation)
                    .map_err(map_custody)?;
                resolve()
            }
            None => {
                let _custody = custody.require_unassigned().map_err(map_custody)?;
                resolve()
            }
        }
    }

    /// Read the workspace-native agent custody for the exact displayed installation.
    pub fn workspace_agent_custody_for_workspace(
        &self,
        expected_root: &str,
        expected_digest: &str,
        expected_installation: &str,
    ) -> Result<crate::WorkspaceAgentCustody, ManagedTextFileError> {
        let custody = crate::workspace_custody::lock_for_workspace_path(
            Path::new(expected_root),
            expected_installation,
        )
        .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
        let _open_serial = self
            .workspace_open
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let held = self.held();
        let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
        if open.root().as_path() != Path::new(expected_root)
            || open.digest().to_string() != expected_digest
            || open.installation() != expected_installation
        {
            return Err(ManagedTextFileError::StaleWorkspace);
        }
        open.ensure_physical_root()
            .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
        custody
            .status()
            .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))
    }

    /// Inspect the complete native working folder while one exact agent assignment remains live.
    ///
    /// This is the sole assigned-workspace read exception. It is closed over the inventory and
    /// file-inspection operations needed by the read-only live monitor and Finish agent handoff;
    /// callers cannot supply a callback or route a mutation through it. The shared custody lock remains held before
    /// `workspace_open` for the entire inspection, and the separate release transaction must
    /// compare the same generation again.
    pub fn inspect_agent_finish_preflight(
        &self,
        expected_root: &str,
        expected_digest: &str,
        expected_installation: &str,
        expected_generation: &str,
    ) -> Result<AgentFinishPreflight, ManagedTextFileError> {
        let _custody = crate::workspace_custody::lock_for_workspace_path(
            Path::new(expected_root),
            expected_installation,
        )
        .and_then(|authority| authority.require_generation(expected_generation))
        .map_err(|error| {
            if error.is_stale_workspace() {
                ManagedTextFileError::StaleWorkspace
            } else {
                ManagedTextFileError::Recovery(error.to_string())
            }
        })?;
        let _open_serial = self
            .workspace_open
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        self.inspect_agent_folder_locked(
            expected_root,
            expected_digest,
            expected_installation,
            expected_generation,
        )
    }

    // Caller holds exact custody and workspace_open throughout this closed inventory operation.
    fn inspect_agent_folder_locked(
        &self,
        expected_root: &str,
        expected_digest: &str,
        expected_installation: &str,
        expected_generation: &str,
    ) -> Result<AgentFinishPreflight, ManagedTextFileError> {
        let (managed_paths, native_paths, unsupported_entries) = {
            let held = self.held();
            let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
            if open.root().as_path() != Path::new(expected_root)
                || open.digest().to_string() != expected_digest
                || open.installation() != expected_installation
            {
                return Err(ManagedTextFileError::StaleWorkspace);
            }
            open.ensure_physical_root()
                .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
            let summary = summarise(open, None);
            if !summary.native_inventory_complete {
                return Err(ManagedTextFileError::Recovery(
                    "the complete native inventory could not be inspected".to_owned(),
                ));
            }
            (
                summary
                    .file_histories
                    .iter()
                    .map(|history| history.path().to_owned())
                    .collect::<Vec<_>>(),
                summary.native_untracked_files,
                summary.native_unsupported_entries,
            )
        };
        let native_directories = self.native_untracked_directories()?;
        let missing_files = self.native_missing_files()?;
        let missing_paths = missing_files
            .iter()
            .map(|file| file.path())
            .collect::<BTreeSet<_>>();
        let managed_files = managed_paths
            .iter()
            .filter(|path| !missing_paths.contains(path.as_str()))
            .map(|path| {
                self.inspect_managed_file(path)
                    .map(ManagedFileInspection::without_text)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let native_files = native_paths
            .iter()
            .map(|path| {
                self.inspect_native_untracked_file(path)
                    .map(NativeFileInspection::without_text)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(AgentFinishPreflight {
            root: expected_root.to_owned(),
            digest: expected_digest.to_owned(),
            installation: expected_installation.to_owned(),
            generation: expected_generation.to_owned(),
            managed_files,
            native_files,
            native_directories,
            missing_files,
            unsupported_entries,
        })
    }

    /// Read one current file twice while exact agent custody and workspace identity remain held.
    /// Only an identical pair is returned, so the desktop never labels a concurrently changing
    /// read as a stable live snapshot. Unchanged tracked files remain previewable under custody;
    /// the returned kind keeps that read distinct from unrecorded agent work.
    pub fn inspect_agent_live_file(
        &self,
        expected_root: &str,
        expected_digest: &str,
        expected_installation: &str,
        expected_generation: &str,
        relative_path: &str,
    ) -> Result<AgentLiveFileSnapshot, ManagedTextFileError> {
        let _custody = crate::workspace_custody::lock_for_workspace_path(
            Path::new(expected_root),
            expected_installation,
        )
        .and_then(|authority| authority.require_generation(expected_generation))
        .map_err(|error| {
            if error.is_stale_workspace() {
                ManagedTextFileError::StaleWorkspace
            } else {
                ManagedTextFileError::Recovery(error.to_string())
            }
        })?;
        let _open_serial = self
            .workspace_open
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let (managed, native) = {
            let held = self.held();
            let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
            if open.root().as_path() != Path::new(expected_root)
                || open.digest().to_string() != expected_digest
                || open.installation() != expected_installation
            {
                return Err(ManagedTextFileError::StaleWorkspace);
            }
            open.ensure_physical_root()
                .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
            let summary = summarise(open, None);
            if !summary.native_inventory_complete {
                return Err(ManagedTextFileError::Recovery(
                    "the complete native inventory could not be inspected".to_owned(),
                ));
            }
            (
                summary
                    .file_histories
                    .iter()
                    .any(|history| history.path() == relative_path),
                summary
                    .native_untracked_files
                    .iter()
                    .any(|path| path == relative_path),
            )
        };
        let (kind, byte_count, content_digest, executable, text, bytes) = if managed {
            let first =
                self.inspect_managed_file_bounded(relative_path, MAX_AGENT_LIVE_PREVIEW_BYTES)?;
            #[cfg(test)]
            run_between_agent_live_file_reads();
            let second =
                self.inspect_managed_file_bounded(relative_path, MAX_AGENT_LIVE_PREVIEW_BYTES)?;
            if first != second {
                return Err(ManagedTextFileError::Recovery(
                    "the live file changed while Mesh was reading it; retry when the write finishes"
                        .to_owned(),
                ));
            }
            (
                if first.modified_from_current_version() {
                    "modified-file"
                } else {
                    "current-file"
                },
                first.byte_count(),
                first.content_digest().to_string(),
                first.executable(),
                first.text().map(str::to_owned),
                first.bytes().to_vec(),
            )
        } else if native {
            let first = self.inspect_native_untracked_file_bounded(
                relative_path,
                MAX_AGENT_LIVE_PREVIEW_BYTES,
            )?;
            #[cfg(test)]
            run_between_agent_live_file_reads();
            let second = self.inspect_native_untracked_file_bounded(
                relative_path,
                MAX_AGENT_LIVE_PREVIEW_BYTES,
            )?;
            if first != second {
                return Err(ManagedTextFileError::Recovery(
                    "the live file changed while Mesh was reading it; retry when the write finishes"
                        .to_owned(),
                ));
            }
            (
                "new-file",
                first.byte_count(),
                first.content_digest().to_string(),
                first.executable(),
                first.text().map(str::to_owned),
                first.bytes().to_vec(),
            )
        } else {
            return Err(ManagedTextFileError::NotRegularFile);
        };
        Ok(AgentLiveFileSnapshot {
            root: expected_root.to_owned(),
            digest: expected_digest.to_owned(),
            installation: expected_installation.to_owned(),
            generation: expected_generation.to_owned(),
            path: relative_path.to_owned(),
            kind,
            byte_count,
            content_digest,
            executable,
            text,
            bytes,
        })
    }

    /// Atomically migrate a pre-workspace-authority Recent assignment or read current custody.
    ///
    /// `projected_active` carries no generation authority. It only says an older owner-only Recent
    /// record conservatively marked this exact installation assigned. When the shared record is
    /// absent, this transaction mints a fresh shared generation before Recent may be rewritten.
    pub fn reconcile_workspace_agent_custody(
        &self,
        expected_root: &str,
        expected_digest: &str,
        expected_installation: &str,
        projected_active: bool,
    ) -> Result<crate::WorkspaceAgentCustody, ManagedTextFileError> {
        let custody = crate::workspace_custody::lock_for_workspace_path(
            Path::new(expected_root),
            expected_installation,
        )
        .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
        let _open_serial = self
            .workspace_open
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let held = self.held();
        let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
        if open.root().as_path() != Path::new(expected_root)
            || open.digest().to_string() != expected_digest
            || open.installation() != expected_installation
        {
            return Err(ManagedTextFileError::StaleWorkspace);
        }
        open.ensure_physical_root()
            .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
        let current = custody
            .status()
            .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
        if current.is_assigned() || !projected_active {
            return Ok(current);
        }
        custody
            .acquire(false, None)
            .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
        custody
            .status()
            .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))
    }

    /// Snapshot the current open workspace for native navigation reconciliation.
    pub fn current_workspace_summary(&self) -> Result<WorkspaceSummary, Unavailable> {
        let _open_serial = self
            .workspace_open
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let held = self.held();
        let open = held.as_ref().ok_or_else(Unavailable::no_workspace_open)?;
        open.ensure_physical_root().map_err(|_| {
            Unavailable::new(
                "workspace-directory-changed",
                user_messages::WORKSPACE_DIRECTORY_CHANGED,
            )
        })?;
        Ok(summarise(open, None))
    }

    /// Acquire or explicitly reopen the workspace-native agent custody.
    ///
    /// Desktop callers hold their RecentWorkspace lock before entering this method. The shared
    /// authority is locked before process-local daemon state, preserving
    /// `Recent -> shared custody -> workspace_open` and blocking rollback through verification.
    pub fn acquire_workspace_agent_custody(
        &self,
        expected_root: &str,
        expected_digest: &str,
        expected_installation: &str,
        confirmed_reopen: bool,
        expected_generation: Option<&str>,
    ) -> Result<String, ManagedTextFileError> {
        let custody = crate::workspace_custody::lock_for_workspace_path(
            Path::new(expected_root),
            expected_installation,
        )
        .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
        let _open_serial = self
            .workspace_open
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let held = self.held();
        let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
        if open.root().as_path() != Path::new(expected_root)
            || open.digest().to_string() != expected_digest
            || open.installation() != expected_installation
        {
            return Err(ManagedTextFileError::StaleWorkspace);
        }
        open.ensure_physical_root()
            .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
        custody
            .acquire(confirmed_reopen, expected_generation)
            .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))
    }

    /// Release only the exact workspace-native generation the person finished.
    pub fn release_workspace_agent_custody(
        &self,
        expected_root: &str,
        expected_digest: &str,
        expected_installation: &str,
        expected_generation: &str,
    ) -> Result<bool, ManagedTextFileError> {
        let custody = crate::workspace_custody::lock_for_workspace_path(
            Path::new(expected_root),
            expected_installation,
        )
        .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
        let _open_serial = self
            .workspace_open
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let held = self.held();
        let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
        if open.root().as_path() != Path::new(expected_root)
            || open.digest().to_string() != expected_digest
            || open.installation() != expected_installation
        {
            return Err(ManagedTextFileError::StaleWorkspace);
        }
        open.ensure_physical_root()
            .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
        custody
            .release(expected_generation)
            .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))
    }

    /// Hold the exact active agent generation across bounded native-host setup and launch.
    ///
    /// Global order remains workspace-native custody then `workspace_open`; callers must not hold
    /// or acquire desktop Recent state while this guard is alive. A stale generation, malformed
    /// record, replaced installation, or changed daemon workspace is refused before the caller can
    /// touch the assigned folder.
    pub fn lock_workspace_agent_setup(
        &self,
        expected_root: &str,
        expected_digest: &str,
        expected_installation: &str,
        expected_generation: &str,
    ) -> Result<WorkspaceAgentSetupGuard<'_>, ManagedTextFileError> {
        let custody = crate::workspace_custody::lock_for_workspace_path(
            Path::new(expected_root),
            expected_installation,
        )
        .map_err(|error| {
            if error.is_stale_workspace() {
                ManagedTextFileError::StaleWorkspace
            } else {
                ManagedTextFileError::Recovery(error.to_string())
            }
        })?;
        let status = custody
            .status()
            .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
        if status.generation() != Some(expected_generation) {
            return Err(ManagedTextFileError::Recovery(
                "workspace agent custody changed before native setup".to_owned(),
            ));
        }
        let workspace_open = self
            .workspace_open
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        {
            let held = self.held();
            let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
            if open.root().as_path() != Path::new(expected_root)
                || open.digest().to_string() != expected_digest
                || open.installation() != expected_installation
            {
                return Err(ManagedTextFileError::StaleWorkspace);
            }
            open.ensure_physical_root()
                .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
        }
        Ok(WorkspaceAgentSetupGuard {
            _custody: custody,
            _workspace_open: workspace_open,
        })
    }

    /// Reconstruct and summarize one durable workspace point without creating or switching a
    /// working folder.
    ///
    /// The summary is bound to the exact root, record fold and physical installation displayed by
    /// the desktop. It reads and verifies every retained file needed by the selected causal point,
    /// but returns only bounded path metadata: no file content crosses into the webview. Opening a
    /// point remains a separate explicit action.
    pub fn preview_workspace_version_for_workspace(
        &self,
        expected_root: &str,
        expected_digest: &str,
        expected_installation: &str,
        operation: &str,
    ) -> Result<crate::ipc::Json, Unavailable> {
        let operation = RecordDigest::parse_hex(operation)
            .map_err(|_| workspace_version_refusal("workspace-version-invalid"))?;
        let _open_serial = self
            .workspace_open
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let held = self.held();
        let open = held
            .as_ref()
            .ok_or_else(|| workspace_version_refusal("workspace-version-no-workspace"))?;
        if open.root().as_path() != Path::new(expected_root)
            || open.digest().to_string() != expected_digest
            || open.installation() != expected_installation
        {
            return Err(Unavailable::new(
                "workspace-version-source-changed",
                "The open workspace changed after this saved version was shown. Refresh and choose the version again; no folder was created.",
            ));
        }
        let versions = open.workspace_versions();
        let version_index = versions
            .iter()
            .position(|version| version.operation() == operation)
            .ok_or_else(|| workspace_version_refusal("workspace-version-history-incomplete"))?;
        let version = versions[version_index];
        let snapshot = open
            .historical_workspace_preview(operation)
            .map_err(|_| workspace_version_refusal("workspace-version-history-incomplete"))?;
        let (change_basis, basis_ordinal, basis) = match open
            .workspace_version_change_basis(operation)
            .map_err(|_| workspace_version_refusal("workspace-version-history-incomplete"))?
        {
            WorkspaceVersionChangeBasis::Initial => ("initial", None, None),
            WorkspaceVersionChangeBasis::Previous(previous) => {
                let previous_version = versions
                    .iter()
                    .find(|candidate| candidate.operation() == previous)
                    .ok_or_else(|| {
                        workspace_version_refusal("workspace-version-history-incomplete")
                    })?;
                let previous_snapshot =
                    open.historical_workspace_preview(previous).map_err(|_| {
                        workspace_version_refusal("workspace-version-history-incomplete")
                    })?;
                (
                    "previous-point",
                    Some(previous_version.ordinal()),
                    Some(previous_snapshot),
                )
            }
            WorkspaceVersionChangeBasis::CombinedHistory => ("combined-history", None, None),
        };
        let mut changes = if change_basis == "combined-history" {
            Vec::new()
        } else {
            workspace_version_preview_changes(basis.as_ref(), &snapshot)
        };
        let change_count = changes.len();
        changes.truncate(MAX_WORKSPACE_VERSION_PREVIEW_CHANGES);
        let changes_not_listed =
            u64::try_from(change_count.saturating_sub(MAX_WORKSPACE_VERSION_PREVIEW_CHANGES))
                .map_err(|_| workspace_version_refusal("workspace-version-preview-unavailable"))?;
        let total_bytes = snapshot.files.iter().try_fold(0_u64, |total, file| {
            total
                .checked_add(file.byte_length)
                .ok_or_else(|| workspace_version_refusal("workspace-version-preview-unavailable"))
        })?;
        let mut entries = snapshot
            .directories
            .iter()
            .map(|directory| (directory.path.as_str(), "folder", None))
            .chain(
                snapshot
                    .files
                    .iter()
                    .map(|file| (file.path.as_str(), "file", Some(file.byte_length))),
            )
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.0.cmp(right.0).then_with(|| left.1.cmp(right.1)));
        let entry_count = entries.len();
        entries.truncate(MAX_WORKSPACE_VERSION_PREVIEW_ENTRIES);
        let folder_count = u64::try_from(snapshot.directories.len())
            .map_err(|_| workspace_version_refusal("workspace-version-preview-unavailable"))?;
        let file_count = u64::try_from(snapshot.files.len())
            .map_err(|_| workspace_version_refusal("workspace-version-preview-unavailable"))?;
        let entries_not_listed =
            u64::try_from(entry_count.saturating_sub(MAX_WORKSPACE_VERSION_PREVIEW_ENTRIES))
                .map_err(|_| workspace_version_refusal("workspace-version-preview-unavailable"))?;
        Ok(crate::ipc::Json::object([
            (
                "action",
                crate::ipc::Json::text("workspace-version-preview"),
            ),
            (
                "source_version",
                crate::ipc::Json::text(snapshot.operation.to_string()),
            ),
            ("ordinal", crate::ipc::Json::Number(version.ordinal())),
            (
                "basis_ordinal",
                basis_ordinal.map_or(crate::ipc::Json::Null, crate::ipc::Json::Number),
            ),
            ("change_basis", crate::ipc::Json::text(change_basis)),
            (
                "actor_sequence",
                crate::ipc::Json::text(version.actor_sequence().to_string()),
            ),
            ("folders", crate::ipc::Json::Number(folder_count)),
            ("files", crate::ipc::Json::Number(file_count)),
            (
                "total_bytes",
                crate::ipc::Json::text(total_bytes.to_string()),
            ),
            (
                "entries",
                crate::ipc::Json::Array(
                    entries
                        .into_iter()
                        .map(|(path, entry_type, bytes)| {
                            crate::ipc::Json::object([
                                ("path", crate::ipc::Json::text(path)),
                                ("type", crate::ipc::Json::text(entry_type)),
                                (
                                    "bytes",
                                    bytes.map_or(crate::ipc::Json::Null, |value| {
                                        crate::ipc::Json::text(value.to_string())
                                    }),
                                ),
                            ])
                        })
                        .collect(),
                ),
            ),
            (
                "entries_not_listed",
                crate::ipc::Json::Number(entries_not_listed),
            ),
            (
                "changes",
                crate::ipc::Json::Array(
                    changes
                        .into_iter()
                        .map(|change| {
                            crate::ipc::Json::object([
                                ("path", crate::ipc::Json::text(change.path)),
                                ("type", crate::ipc::Json::text(change.entry_type)),
                                ("effect", crate::ipc::Json::text(change.effect)),
                            ])
                        })
                        .collect(),
                ),
            ),
            (
                "changes_not_listed",
                crate::ipc::Json::Number(changes_not_listed),
            ),
            ("content_verified", crate::ipc::Json::Bool(true)),
            ("creates_folder", crate::ipc::Json::Bool(false)),
        ]))
    }

    /// Compute and open the exact first-publication review for the workspace the desktop showed.
    ///
    /// The desktop supplies no bundle or target identifier. Both are derived while the workspace
    /// replacement serial is held, then the immutable review record is appended only after the
    /// configured recovery trigger has durably preserved the current pending prefix. The software
    /// actor identifies who opened the local review; it gains no approval or publication power.
    pub fn open_current_review_for_workspace(
        &self,
        expected_root: &str,
        expected_digest: &str,
        expected_installation: &str,
        opened_by: PublicKey,
    ) -> Result<WorkspaceSummary, Unavailable> {
        self.open_current_review_for_workspace_actor(
            expected_root,
            expected_digest,
            expected_installation,
            RecordDigest::from_bytes(*opened_by.actor_id::<Blake3>().digest().as_bytes()),
        )
    }

    /// Record an immutable saved-version review for the exact assigned agent workspace.
    ///
    /// Newer working bytes are irrelevant to the selected immutable closure. This closed operation
    /// retains custody and supplies no approval/signing capability. Repeating the same target and
    /// native actor returns the original record, including after later private saves.
    pub fn submit_agent_saved_review(
        &self,
        request: AgentWorkspaceCheckpointRequest<'_>,
        target: RecordDigest,
        actor: PublicKey,
    ) -> Result<RecordDigest, Unavailable> {
        if VERIFIED_MUTATION_CONTEXT.with(|active| active.borrow().is_some()) {
            return Err(publication_refusal(
                "agent-review-nested",
                "A signing callback cannot submit a review.",
            ));
        }
        let _authority = self
            .lock_workspace_agent_setup(
                request.root,
                request.digest,
                request.installation,
                request.generation,
            )
            .map_err(|_| {
                publication_refusal(
                    "agent-review-custody-changed",
                    "The agent workspace assignment changed.",
                )
            })?;
        let mut checkpoint = self
            .checkpoint
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut held = self.held();
        let open = held.as_mut().ok_or_else(Unavailable::no_workspace_open)?;
        let opened_by = RecordDigest::from_bytes(*actor.actor_id::<Blake3>().digest().as_bytes());
        if let Some(bundle) = open
            .recorded_review_for_actor(target, opened_by)
            .map_err(|_| {
                publication_refusal(
                    "agent-review-ambiguous",
                    "More than one saved review matches this request.",
                )
            })?
        {
            return Ok(bundle);
        }
        let bundle = open.saved_publication_review_bundle(target).map_err(|_| {
            publication_refusal(
                "agent-review-unavailable",
                "The exact saved review could not be reconstructed.",
            )
        })?;
        if let Some(review) = open.review(&bundle) {
            if review.subject_operation == target {
                return Ok(bundle);
            }
            return Err(publication_refusal(
                "agent-review-conflict",
                "The saved review identity conflicts with its target.",
            ));
        }
        checkpoint
            .preserve_pending_recovery_for_signal(open, RecoveryRuntimeSignal::ReviewOpened)
            .map_err(|_| {
                publication_refusal(
                    "publication-recovery-preservation-failed",
                    user_messages::PUBLICATION_RECOVERY_PRESERVATION_FAILED,
                )
            })?;
        open.append_record(&StoredRecord::Review(ReviewRecord {
            bundle,
            subject_operation: target,
            opened_by,
        }))
        .map_err(|_| publication_save_failed())?;
        reopen(&mut held, &self.trusted_reviewers)?;
        Ok(bundle)
    }

    /// Recompute the exact approval context for one recorded review in the displayed workspace.
    pub fn human_approval_context_for_workspace(
        &self,
        expected_root: &str,
        expected_digest: &str,
        expected_installation: &str,
        bundle: &str,
        target: &str,
    ) -> Result<mesh_approval::HumanApprovalContext, Unavailable> {
        let _open_serial = self
            .workspace_open
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let held = self.held();
        let open = held.as_ref().ok_or_else(Unavailable::no_workspace_open)?;
        if open.root().as_path() != Path::new(expected_root)
            || open.digest().to_string() != expected_digest
            || open.installation() != expected_installation
        {
            return Err(Unavailable::new(
                "stale-workspace",
                ManagedTextFileError::StaleWorkspace.to_string(),
            ));
        }
        open.ensure_physical_root()
            .map_err(|_| publication_save_failed())?;
        let bundle = parse_digest(bundle)?;
        let target = parse_digest(target)?;
        let review = open.review(&bundle).ok_or_else(|| {
            publication_refusal(
                "publication-review-absent",
                user_messages::PUBLICATION_REVIEW_ABSENT,
            )
        })?;
        if review.subject_operation != target {
            return Err(publication_refusal(
                "publication-review-conflict",
                user_messages::PUBLICATION_REVIEW_CONFLICT,
            ));
        }
        open.human_approval_context(&review).map_err(|_| {
            publication_refusal(
                "publication-review-not-computable",
                user_messages::PUBLICATION_REVIEW_NOT_COMPUTABLE,
            )
        })
    }

    /// Recompute both the signed context and the complete trusted-native review summary.
    pub fn human_approval_preview_for_workspace(
        &self,
        expected_root: &str,
        expected_digest: &str,
        expected_installation: &str,
        bundle: &str,
        target: &str,
    ) -> Result<HumanApprovalPreview, Unavailable> {
        let _open_serial = self
            .workspace_open
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let held = self.held();
        let open = held.as_ref().ok_or_else(Unavailable::no_workspace_open)?;
        if open.root().as_path() != Path::new(expected_root)
            || open.digest().to_string() != expected_digest
            || open.installation() != expected_installation
        {
            return Err(Unavailable::new(
                "stale-workspace",
                ManagedTextFileError::StaleWorkspace.to_string(),
            ));
        }
        open.ensure_physical_root()
            .map_err(|_| publication_save_failed())?;
        let bundle = parse_digest(bundle)?;
        let target = parse_digest(target)?;
        let review = open.review(&bundle).ok_or_else(|| {
            publication_refusal(
                "publication-review-absent",
                user_messages::PUBLICATION_REVIEW_ABSENT,
            )
        })?;
        if review.subject_operation != target {
            return Err(publication_refusal(
                "publication-review-conflict",
                user_messages::PUBLICATION_REVIEW_CONFLICT,
            ));
        }
        require_current_native_review_scope(open, target)?;
        let (context, change_summary, presentation_digest, review_bundle, approved_state) =
            open.human_approval_preview(&review).map_err(|_| {
                publication_refusal(
                    "publication-review-not-computable",
                    user_messages::PUBLICATION_REVIEW_NOT_COMPUTABLE,
                )
            })?;
        Ok(HumanApprovalPreview {
            context,
            presentation_digest,
            change_summary,
            review_bundle,
            approved_state,
        })
    }

    /// Read only a durable review through native lane identity. The mutable working fold is not
    /// approval input here: newer private saves cannot retarget this immutable review selection.
    pub(crate) fn recorded_lane_review(
        &self,
        root: &str,
        installation: &str,
        bundle: RecordDigest,
        target: RecordDigest,
    ) -> Result<crate::ipc::Json, Unavailable> {
        self.with_recorded_lane_review(root, installation, bundle, target, |open| {
            open.recorded_review_item(bundle).ok_or_else(|| {
                publication_refusal(
                    "fleet-review-unavailable",
                    "The exact saved lane review is unavailable.",
                )
            })
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn recorded_lane_artifact(
        &self,
        root: &str,
        installation: &str,
        bundle: RecordDigest,
        target: RecordDigest,
        object: &str,
        side: &str,
    ) -> Result<ReviewArtifact, Unavailable> {
        let object = ObjectId::parse(object).map_err(|_| {
            publication_refusal(
                "fleet-review-object-invalid",
                "The saved review object is invalid.",
            )
        })?;
        let side = match side {
            "before" => ReviewArtifactSide::Before,
            "after" => ReviewArtifactSide::After,
            _ => {
                return Err(publication_refusal(
                    "fleet-review-side-invalid",
                    "The saved review side is invalid.",
                ))
            }
        };
        self.with_recorded_lane_review(root, installation, bundle, target, |open| {
            let artifact = open
                .verified_review_artifact(bundle, target, object, side)
                .map_err(|_| {
                    publication_refusal(
                        "fleet-review-artifact-unavailable",
                        "The exact saved lane artifact could not be verified.",
                    )
                })?;
            Ok(ReviewArtifact {
                path: artifact.path,
                version: artifact.version,
                digest: artifact.digest,
                bytes: artifact.bytes,
            })
        })
    }

    fn with_recorded_lane_review<T>(
        &self,
        root: &str,
        installation: &str,
        bundle: RecordDigest,
        target: RecordDigest,
        read: impl FnOnce(&OpenWorkspace) -> Result<T, Unavailable>,
    ) -> Result<T, Unavailable> {
        let _serial = self
            .workspace_open
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let held = self.held();
        let open = held.as_ref().ok_or_else(Unavailable::no_workspace_open)?;
        if open.root().as_path() != Path::new(root) || open.installation() != installation {
            return Err(publication_refusal(
                "fleet-review-workspace-changed",
                "The lane workspace identity changed.",
            ));
        }
        open.ensure_physical_root()
            .map_err(|_| publication_save_failed())?;
        // Unlike the selected-workspace preview API, this seam never admits an automatic candidate.
        open.review(&bundle)
            .filter(|review| review.subject_operation == target && review.bundle == bundle)
            .ok_or_else(|| {
                publication_refusal(
                    "fleet-review-not-recorded",
                    "This exact review is not recorded in the lane.",
                )
            })?;
        let result = read(open)?;
        open.ensure_physical_root()
            .map_err(|_| publication_save_failed())?;
        Ok(result)
    }

    /// Reconstruct one side of one displayed file change for a local artifact renderer.
    ///
    /// Root, fold digest and installation bind this read to the exact workspace snapshot shown by
    /// the desktop. Bundle, target and object are then re-derived from immutable review history;
    /// the mutable working folder is never consulted.
    #[allow(clippy::too_many_arguments)]
    pub fn review_artifact_for_workspace(
        &self,
        expected_root: &str,
        expected_digest: &str,
        expected_installation: &str,
        bundle: &str,
        target: &str,
        object: &str,
        side: &str,
    ) -> Result<ReviewArtifact, Unavailable> {
        let _open_serial = self
            .workspace_open
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let held = self.held();
        let open = held.as_ref().ok_or_else(Unavailable::no_workspace_open)?;
        if open.root().as_path() != Path::new(expected_root)
            || open.digest().to_string() != expected_digest
            || open.installation() != expected_installation
        {
            return Err(Unavailable::new(
                "stale-workspace",
                ManagedTextFileError::StaleWorkspace.to_string(),
            ));
        }
        open.ensure_physical_root().map_err(|_| {
            Unavailable::new(
                "review-artifact-unavailable",
                "The saved artifact preview is unavailable because the workspace directory changed.",
            )
        })?;
        let bundle = parse_digest(bundle)?;
        let target = parse_digest(target)?;
        let object = ObjectId::parse(object).map_err(|_| {
            Unavailable::new(
                "publication-identifier-invalid",
                user_messages::PUBLICATION_IDENTIFIER_INVALID,
            )
        })?;
        let side = match side {
            "before" => ReviewArtifactSide::Before,
            "after" => ReviewArtifactSide::After,
            _ => {
                return Err(Unavailable::new(
                    "review-artifact-side-invalid",
                    "The saved artifact preview side is invalid.",
                ));
            }
        };
        let artifact = open
            .verified_review_artifact(bundle, target, object, side)
            .map_err(|_| {
                Unavailable::new(
                    "review-artifact-unavailable",
                    "The exact saved artifact preview could not be verified.",
                )
            })?;
        Ok(ReviewArtifact {
            path: artifact.path,
            version: artifact.version,
            digest: artifact.digest,
            bytes: artifact.bytes,
        })
    }

    /// Verify and append one native user-presence approval only to the exact workspace the
    /// desktop displayed and only while its ordinary folder still equals the reviewed saved point.
    ///
    /// The native dialog can remain open while another local client switches the daemon or an
    /// editor writes newer bytes. Root plus fold digest are insufficient for a byte-identical
    /// replacement, so the physical installation is checked under the workspace-open serial. The
    /// complete native tree is then re-read immediately before the durable approval append.
    #[allow(clippy::too_many_arguments)]
    pub fn approve_review_for_workspace(
        &self,
        expected_root: &str,
        expected_digest: &str,
        expected_installation: &str,
        bundle: &str,
        target: &str,
        receipt_hex: &str,
    ) -> Result<WorkspaceSummary, Unavailable> {
        let bundle = parse_digest(bundle)?;
        let target = parse_digest(target)?;
        let _workspace_authority = self
            .lock_current_managed_workspace_mutation()
            .map_err(agent_custody_refusal)?;
        let mut held = self.held();
        let open = held.as_ref().ok_or_else(Unavailable::no_workspace_open)?;
        if open.root().as_path() != Path::new(expected_root)
            || open.digest().to_string() != expected_digest
            || open.installation() != expected_installation
        {
            return Err(Unavailable::new(
                "stale-workspace",
                ManagedTextFileError::StaleWorkspace.to_string(),
            ));
        }
        self.approve_review_locked(&mut held, bundle, target, receipt_hex)
    }

    fn approve_review_locked(
        &self,
        held: &mut Option<OpenWorkspace>,
        bundle: RecordDigest,
        target: RecordDigest,
        receipt_hex: &str,
    ) -> Result<WorkspaceSummary, Unavailable> {
        if !self.trusted_reviewers.is_configured() {
            return Err(publication_refusal(
                "publication-trust-absent",
                user_messages::PUBLICATION_TRUST_ABSENT,
            ));
        }
        let open = held.as_mut().ok_or_else(Unavailable::no_workspace_open)?;
        let Some(review) = open.review(&bundle) else {
            return Err(publication_refusal(
                "publication-review-absent",
                user_messages::PUBLICATION_REVIEW_ABSENT,
            ));
        };
        if review.subject_operation != target {
            return Err(publication_refusal(
                "publication-review-conflict",
                user_messages::PUBLICATION_REVIEW_CONFLICT,
            ));
        }
        if !self.trusted_reviewers.has_human_credentials() {
            return Err(publication_refusal(
                "publication-human-authority-unavailable",
                user_messages::PUBLICATION_HUMAN_AUTHORITY_UNAVAILABLE,
            ));
        }
        let bytes = decode_hex(receipt_hex).ok_or_else(|| {
            publication_refusal(
                "publication-receipt-invalid",
                user_messages::PUBLICATION_RECEIPT_INVALID,
            )
        })?;
        let receipt =
            mesh_approval::HumanApprovalReceipt::from_canonical_bytes(&bytes).map_err(|_| {
                publication_refusal(
                    "publication-receipt-invalid",
                    user_messages::PUBLICATION_RECEIPT_INVALID,
                )
            })?;
        let carried = receipt.draft().expected();
        let Some(credential) = self
            .trusted_reviewers
            .human_credential(carried.credential().id())
        else {
            return Err(publication_refusal(
                "publication-reviewer-untrusted",
                user_messages::PUBLICATION_REVIEWER_UNTRUSTED,
            ));
        };
        let context = open.human_approval_context(&review).map_err(|_| {
            publication_refusal(
                "publication-review-not-computable",
                user_messages::PUBLICATION_REVIEW_NOT_COMPUTABLE,
            )
        })?;
        let expected =
            mesh_approval::ExpectedHumanApproval::new(context, credential, *carried.challenge());
        mesh_approval::verify_human_approval_receipt(&bytes, &expected).map_err(|_| {
            publication_refusal(
                "publication-approval-invalid",
                user_messages::PUBLICATION_APPROVAL_INVALID,
            )
        })?;
        let current_shared = open
            .shared_version()
            .unwrap_or(crate::publication::GENESIS_SHARED_HEAD);
        if open.shared_version() == Some(expected.context().reviewed_actor_head()) {
            return Err(publication_refusal(
                "publication-already-shared",
                user_messages::PUBLICATION_ALREADY_SHARED,
            ));
        }
        if expected.context().expected_canonical_head() != current_shared {
            return Err(publication_refusal(
                "publication-review-conflict",
                user_messages::PUBLICATION_REVIEW_CONFLICT,
            ));
        }

        // Receipt verification and a user-presence ceremony can take long enough for an editor to
        // write newer native work. Make the final exact-folder observation the last read before
        // promoting receipt bytes and appending the protected shared-version record.
        require_current_native_review_scope(open, target)?;
        let receipt_digest = open
            .promote_approval_receipt(bytes)
            .map_err(|_| publication_save_failed())?;
        open.append_record(&StoredRecord::Approval(ApprovalRecord {
            approval: receipt_digest,
            bundle,
            approver: RecordDigest::from_bytes(*carried.credential().id().as_bytes()),
            verdict: ReviewVerdict::Approved,
        }))
        .map_err(|_| publication_save_failed())?;
        reopen(held, &self.trusted_reviewers)?;
        let reopened = held.as_ref().expect("reopen installs workspace");
        if reopened.shared_version() != Some(expected.context().reviewed_actor_head()) {
            return Err(publication_save_failed());
        }
        Ok(summarise(reopened, None))
    }

    /// Recover and re-verify the exact durable approval for one shared review.
    ///
    /// This is the restart-safe half of local Git export retry. The caller supplies only
    /// identifiers already visible in the review card; the approval envelope and receipt bytes
    /// come from the immutable workspace record stream and its verified content-addressed store.
    pub fn durable_human_approval_for_workspace(
        &self,
        expected_root: &str,
        expected_digest: &str,
        expected_installation: &str,
        bundle: &str,
        target: &str,
    ) -> Result<DurableHumanApproval, Unavailable> {
        let _open_serial = self
            .workspace_open
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let held = self.held();
        let open = held.as_ref().ok_or_else(Unavailable::no_workspace_open)?;
        if open.root().as_path() != Path::new(expected_root)
            || open.digest().to_string() != expected_digest
            || open.installation() != expected_installation
        {
            return Err(Unavailable::new(
                "stale-workspace",
                ManagedTextFileError::StaleWorkspace.to_string(),
            ));
        }
        open.ensure_physical_root()
            .map_err(|_| publication_save_failed())?;
        let bundle = parse_digest(bundle)?;
        let target = parse_digest(target)?;
        let review = open.review(&bundle).ok_or_else(|| {
            publication_refusal(
                "publication-review-absent",
                user_messages::PUBLICATION_REVIEW_ABSENT,
            )
        })?;
        if review.subject_operation != target {
            return Err(publication_refusal(
                "publication-review-conflict",
                user_messages::PUBLICATION_REVIEW_CONFLICT,
            ));
        }
        let approval = open.approved_envelope(&bundle).map_err(|_| {
            publication_refusal(
                "publication-approval-invalid",
                user_messages::PUBLICATION_APPROVAL_INVALID,
            )
        })?;
        let bytes = open.approval_receipt(approval.approval).map_err(|_| {
            publication_refusal(
                "publication-receipt-invalid",
                user_messages::PUBLICATION_RECEIPT_INVALID,
            )
        })?;
        let receipt =
            mesh_approval::HumanApprovalReceipt::from_canonical_bytes(&bytes).map_err(|_| {
                publication_refusal(
                    "publication-receipt-invalid",
                    user_messages::PUBLICATION_RECEIPT_INVALID,
                )
            })?;
        let carried = receipt.draft().expected();
        let Some(credential) = self
            .trusted_reviewers
            .human_credential(carried.credential().id())
        else {
            return Err(publication_refusal(
                "publication-reviewer-untrusted",
                user_messages::PUBLICATION_REVIEWER_UNTRUSTED,
            ));
        };
        if approval.approver.as_bytes() != carried.credential().id().as_bytes() {
            return Err(publication_refusal(
                "publication-approval-invalid",
                user_messages::PUBLICATION_APPROVAL_INVALID,
            ));
        }
        let (context, change_summary, presentation_digest, review_bundle, approved_state) =
            open.human_approval_preview(&review).map_err(|_| {
                publication_refusal(
                    "publication-review-not-computable",
                    user_messages::PUBLICATION_REVIEW_NOT_COMPUTABLE,
                )
            })?;
        let expected = mesh_approval::ExpectedHumanApproval::new(
            context.clone(),
            credential,
            *carried.challenge(),
        );
        mesh_approval::verify_human_approval_receipt(&bytes, &expected).map_err(|_| {
            publication_refusal(
                "publication-approval-invalid",
                user_messages::PUBLICATION_APPROVAL_INVALID,
            )
        })?;
        if open.shared_version() != Some(context.reviewed_actor_head()) {
            return Err(publication_refusal(
                "publication-review-conflict",
                user_messages::PUBLICATION_REVIEW_CONFLICT,
            ));
        }
        Ok(DurableHumanApproval {
            receipt: bytes,
            expected,
            preview: HumanApprovalPreview {
                context,
                presentation_digest,
                change_summary,
                review_bundle,
                approved_state,
            },
        })
    }

    fn open_current_review_for_workspace_actor(
        &self,
        expected_root: &str,
        expected_digest: &str,
        expected_installation: &str,
        opened_by: RecordDigest,
    ) -> Result<WorkspaceSummary, Unavailable> {
        let _workspace_authority = self
            .lock_current_managed_workspace_mutation()
            .map_err(agent_custody_refusal)?;
        let mut checkpoint = self
            .checkpoint
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut held = self.held();
        let open = held.as_mut().ok_or_else(Unavailable::no_workspace_open)?;
        if open.root().as_path() != Path::new(expected_root)
            || open.digest().to_string() != expected_digest
            || open.installation() != expected_installation
        {
            return Err(Unavailable::new(
                "stale-workspace",
                ManagedTextFileError::StaleWorkspace.to_string(),
            ));
        }
        open.ensure_physical_root()
            .map_err(|_| publication_save_failed())?;
        let target = open
            .workspace_versions()
            .last()
            .map(|version| (*version).operation())
            .ok_or_else(|| {
                publication_refusal(
                    "publication-review-not-computable",
                    user_messages::PUBLICATION_REVIEW_NOT_COMPUTABLE,
                )
            })?;
        let bundle = open.first_publication_review_bundle(target).map_err(|_| {
            publication_refusal(
                "publication-review-not-computable",
                user_messages::PUBLICATION_REVIEW_NOT_COMPUTABLE,
            )
        })?;
        require_current_native_review_scope(open, target)?;
        let review_already_exists = open.review(&bundle).is_some_and(|existing| {
            existing.subject_operation == target && existing.opened_by == opened_by
        });
        if open.review(&bundle).is_some() && !review_already_exists {
            return Err(publication_refusal(
                "publication-review-conflict",
                user_messages::PUBLICATION_REVIEW_CONFLICT,
            ));
        }
        if checkpoint
            .preserve_pending_recovery_for_signal(open, RecoveryRuntimeSignal::ReviewOpened)
            .is_err()
        {
            open.record_checkpoint_recovery_attention();
            return Err(publication_refusal(
                "publication-recovery-preservation-failed",
                user_messages::PUBLICATION_RECOVERY_PRESERVATION_FAILED,
            ));
        }
        if review_already_exists {
            return Ok(summarise(open, None));
        }
        open.append_record(&StoredRecord::Review(ReviewRecord {
            bundle,
            subject_operation: target,
            opened_by,
        }))
        .map_err(|_| publication_save_failed())?;
        reopen(&mut held, &self.trusted_reviewers)?;
        Ok(summarise(
            held.as_ref().expect("reopen installs workspace"),
            None,
        ))
    }

    /// Preview a retained-version restore against the exact workspace summary the desktop showed.
    ///
    /// The generic IPC preview predates native workspace-bound mutations and intentionally remains
    /// a read-only compatibility surface. The desktop needs a stronger guarantee: a second local
    /// client can replace the daemon's open workspace while the webview still displays the prior
    /// project. Hold the same workspace-open serial used by replacement and compare the root,
    /// record-fold digest and physical installation before deriving any preview operations.
    pub fn preview_file_restore_for_workspace(
        &self,
        expected_root: &str,
        expected_digest: &str,
        expected_installation: &str,
        object: &str,
        target: &str,
    ) -> Result<crate::ipc::Json, Unavailable> {
        let _open_serial = self
            .workspace_open
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        {
            let held = self.held();
            let open = held.as_ref().ok_or_else(Unavailable::no_workspace_open)?;
            if open.root().as_path() != Path::new(expected_root)
                || open.digest().to_string() != expected_digest
                || open.installation() != expected_installation
            {
                return Err(Unavailable::new(
                    "stale-workspace",
                    ManagedTextFileError::StaleWorkspace.to_string(),
                ));
            }
        }
        self.preview_file_restore(object, target)
    }

    /// Preview an exact physical working-copy replacement without inventing an append-only plan.
    ///
    /// Desktop restore rewrites only the native file and deliberately leaves logical history
    /// unchanged. Unlike [`Self::preview_file_restore_for_workspace`], this projection therefore
    /// accepts the durable current version as a retained target. The workspace identity, retained
    /// manifests, portable executable bits and current physical bytes are all read under the
    /// workspace replacement serial. Execution still rechecks the returned physical digest and
    /// executable bit before reconstructing target bytes from CAS.
    pub fn preview_managed_working_copy_restore_for_workspace(
        &self,
        expected_root: &str,
        expected_digest: &str,
        expected_installation: &str,
        object: &str,
        target: &str,
    ) -> Result<crate::ipc::Json, ManagedTextFileError> {
        self.with_verified_managed_workspace(
            expected_root,
            expected_digest,
            expected_installation,
            || {
                let _managed_edit = self
                    .managed_edit
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                let held = self.held();
                let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
                if open.managed_mutation_recovery_needed() {
                    return Err(ManagedTextFileError::Recovery(
                        "an interrupted local file change needs attention".to_owned(),
                    ));
                }
                let history = open
                    .file_histories()
                    .iter()
                    .find(|history| history.object().to_string() == object)
                    .ok_or(ManagedTextFileError::UnknownRetainedVersion)?;
                let current = history
                    .current()
                    .ok_or(ManagedTextFileError::UnknownRetainedVersion)?;
                let target_identity = history
                    .retained()
                    .iter()
                    .find(|identity| identity.version().to_string() == target)
                    .copied()
                    .ok_or(ManagedTextFileError::UnknownRetainedVersion)?;
                let current_manifest = open
                    .manifest_record(current.manifest())
                    .ok_or(ManagedTextFileError::UnknownRetainedVersion)?;
                let target_manifest = open
                    .manifest_record(target_identity.manifest())
                    .ok_or(ManagedTextFileError::UnknownRetainedVersion)?;
                let current_metadata = open
                    .file_version_metadata(current.version())
                    .ok_or(ManagedTextFileError::UnknownRetainedVersion)?;
                let target_metadata = open
                    .file_version_metadata(target_identity.version())
                    .ok_or(ManagedTextFileError::UnknownRetainedVersion)?;
                let cas = Cas::<_, mesh_cas::Blake3>::with_filesystem(
                    open.storage_root().as_path().to_path_buf(),
                    open.storage_pinned_root().filesystem(),
                )
                .map_err(|error| ManagedTextFileError::RetainedContent(error.to_string()))?;
                verify_retained_manifest(&cas, target_manifest)?;
                open.ensure_physical_root()
                    .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
                let (physical_target, physical_bytes) =
                    read_managed_replacement(open.physical_root().as_path(), history.path())?;
                let physical_digest =
                    RecordDigest::from_bytes(*Blake3::digest_bytes(&physical_bytes).as_bytes());
                let physical_executable = physical_target.executable();
                let physical_byte_count = u64::try_from(physical_bytes.len()).map_err(|_| {
                    ManagedTextFileError::RetainedContent(
                        "the working file length does not fit the preview".to_owned(),
                    )
                })?;
                let modified = physical_digest != current_manifest.content_digest
                    || physical_executable != current_metadata.is_executable();
                if physical_digest == target_manifest.content_digest
                    && physical_executable == target_metadata.is_executable()
                {
                    return Err(ManagedTextFileError::Unchanged);
                }
                let undo_target = history.retained().iter().find(|identity| {
                    let Some(manifest) = open.manifest_record(identity.manifest()) else {
                        return false;
                    };
                    let Some(metadata) = open.file_version_metadata(identity.version()) else {
                        return false;
                    };
                    manifest.content_digest == physical_digest
                        && manifest.byte_length == physical_byte_count
                        && metadata.is_executable() == physical_executable
                        && verify_retained_manifest(&cas, manifest).is_ok()
                });
                let undo_target_json = undo_target.map_or(crate::ipc::Json::Null, |identity| {
                    crate::ipc::Json::object([
                        (
                            "version_id",
                            crate::ipc::Json::text(identity.version().to_string()),
                        ),
                        (
                            "manifest_id",
                            crate::ipc::Json::text(identity.manifest().to_string()),
                        ),
                    ])
                });

                Ok(crate::ipc::Json::object([
                    (
                        "schema",
                        crate::ipc::Json::text("mesh.managed-working-copy-restore-preview/v1"),
                    ),
                    ("canonical_state_read_only", crate::ipc::Json::Bool(true)),
                    (
                        "workspace",
                        crate::ipc::Json::object([
                            ("root", crate::ipc::Json::text(expected_root)),
                            ("digest", crate::ipc::Json::text(expected_digest)),
                            (
                                "installation",
                                crate::ipc::Json::text(expected_installation),
                            ),
                        ]),
                    ),
                    ("object_id", crate::ipc::Json::text(object)),
                    ("path", crate::ipc::Json::text(history.path())),
                    (
                        "current",
                        crate::ipc::Json::object([
                            (
                                "version_id",
                                crate::ipc::Json::text(current.version().to_string()),
                            ),
                            (
                                "manifest_id",
                                crate::ipc::Json::text(current.manifest().to_string()),
                            ),
                        ]),
                    ),
                    (
                        "target",
                        crate::ipc::Json::object([
                            (
                                "version_id",
                                crate::ipc::Json::text(target_identity.version().to_string()),
                            ),
                            (
                                "manifest_id",
                                crate::ipc::Json::text(target_identity.manifest().to_string()),
                            ),
                            (
                                "content_digest",
                                crate::ipc::Json::text(target_manifest.content_digest.to_string()),
                            ),
                            (
                                "byte_count",
                                crate::ipc::Json::text(target_manifest.byte_length.to_string()),
                            ),
                            (
                                "executable",
                                crate::ipc::Json::Bool(target_metadata.is_executable()),
                            ),
                        ]),
                    ),
                    (
                        "working_copy",
                        crate::ipc::Json::object([
                            (
                                "content_digest",
                                crate::ipc::Json::text(physical_digest.to_string()),
                            ),
                            (
                                "byte_count",
                                crate::ipc::Json::text(physical_byte_count.to_string()),
                            ),
                            ("executable", crate::ipc::Json::Bool(physical_executable)),
                            (
                                "modified_from_current_version",
                                crate::ipc::Json::Bool(modified),
                            ),
                        ]),
                    ),
                    ("history_unchanged", crate::ipc::Json::Bool(true)),
                    ("undo_target", undo_target_json),
                    ("execution_authorized", crate::ipc::Json::Bool(false)),
                ]))
            },
        )
    }

    /// The feed, for the process that owns this daemon's lifetime.
    #[must_use]
    pub fn feed(&self) -> &EventFeed {
        self.feed.as_ref()
    }

    /// This daemon's performance counters, for a caller that wants the live numbers.
    ///
    /// Readable at any time, with no benchmark and no workload: plan §3.5 OBS-004 asks for the
    /// counters to be *exposed*, and a process that is running is the cheapest place to read them.
    #[must_use]
    pub const fn counters(&self) -> &Counters {
        &self.counters
    }

    /// One reading of every counter, with what collection cost to take it.
    #[must_use]
    pub fn counter_snapshot(&self) -> CounterSnapshot {
        self.counters.snapshot()
    }

    /// Record what one completed open did, in counts and bytes.
    ///
    /// Called after the open, never inside it: `## Scope` asks for collection that does not perturb
    /// the measurement, and the cheapest way to keep that true is to take every number from the
    /// finished [`OpenWorkspace`] rather than to instrument the path that produced it. Nothing here
    /// touches the filesystem — the byte count is the durable boundary plus whatever an interrupted
    /// append left, both of which the scan already computed.
    ///
    /// The elapsed span covers reading the file *and* folding the index. It is charged once, to
    /// `workspace_operations.index_reconstruction.ns`; `local_filesystem.sequential_read.ns` is
    /// deliberately left unfed rather than charged the same nanoseconds a second time.
    fn record_open(&self, open: &OpenWorkspace) {
        let boundary = open.boundary();
        let tail = open.tail();
        let bytes_read = boundary.byte_offset.saturating_add(tail.discarded_bytes());
        let elapsed_ns = u64::try_from(open.diagnostic().elapsed().as_nanos()).unwrap_or(u64::MAX);
        let rows: u64 = open
            .rows_per_table()
            .iter()
            .map(|(_, count)| *count as u64)
            .sum();

        if tail.is_fragment() {
            self.counters.record_group_by_key(&[
                ("local_filesystem.sequential_read.ops", 1),
                ("local_filesystem.sequential_read.bytes", bytes_read),
                ("workspace_operations.index_reconstruction.ops", 1),
                ("workspace_operations.index_reconstruction.rows", rows),
                ("workspace_operations.index_reconstruction.ns", elapsed_ns),
                ("workspace_operations.crash_recovery.ops", 1),
                (
                    "workspace_operations.crash_recovery.records",
                    boundary.records,
                ),
                ("workspace_operations.crash_recovery.ns", elapsed_ns),
            ]);
        } else {
            self.counters.record_group_by_key(&[
                ("local_filesystem.sequential_read.ops", 1),
                ("local_filesystem.sequential_read.bytes", bytes_read),
                ("workspace_operations.index_reconstruction.ops", 1),
                ("workspace_operations.index_reconstruction.rows", rows),
                ("workspace_operations.index_reconstruction.ns", elapsed_ns),
            ]);
        }
    }

    /// Record one checkpoint only after its CAS, index, journal, reload, and activity observation
    /// all completed. `linked_bytes` is the exact number of new content and payload bytes the
    /// durable promoter linked; already-present content contributes zero without hiding the save.
    fn record_checkpoint_creation(&self, linked_bytes: u64) {
        self.counters.record_group_by_key(&[
            ("workspace_operations.checkpoint_creation.ops", 1),
            (
                "workspace_operations.checkpoint_creation.bytes",
                linked_bytes,
            ),
        ]);
    }

    /// Open a workspace at start-up, before any connection is served.
    ///
    /// Separate from [`Operations::open_workspace`] only in that it returns the failure rather
    /// than a wire refusal, so the process can decide whether to keep running.
    ///
    /// **A failure sets the start-up report too.** This call *is* the start-up, so a folder that
    /// could not be opened is what this start-up found. Leaving the summary at
    /// [`crate::ipc::nothing_to_recover`] would answer `startup.report` with the sentence a clean
    /// start produces, which is the one reading a crash must never have.
    ///
    /// # Errors
    ///
    /// Whatever [`OpenWorkspace::open`] reports.
    pub fn open_at_start(&self, root: &Path) -> Result<WorkspaceSummary, OpenFailure> {
        self.open_at_start_with(root, true)
    }

    /// Reopen a remembered workspace at startup without creating missing workspace state.
    ///
    /// Unlike [`Self::open_at_start`], this method never initializes a missing root, private
    /// namespace, or journal. It is for app-owned navigation hints whose disappearance must be
    /// reported rather than translated into a plausible blank workspace.
    ///
    /// # Errors
    ///
    /// Whatever [`OpenWorkspace::reopen_with_trusted_reviewers`] reports.
    pub fn reopen_at_start(&self, root: &Path) -> Result<WorkspaceSummary, OpenFailure> {
        self.open_at_start_with(root, false)
    }

    fn open_at_start_with(
        &self,
        root: &Path,
        create_missing: bool,
    ) -> Result<WorkspaceSummary, OpenFailure> {
        let _custody =
            crate::workspace_custody::lock_workspace_path_initialization(root, create_missing)
                .map_err(|error| {
                    OpenFailure::Unreachable(std::io::Error::other(error.to_string()))
                })?;
        let _open_serial = self
            .workspace_open
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let started = Instant::now();
        let (summary, startup) = match self.open_and_install_workspace_with(root, create_missing) {
            Ok(installed) => installed,
            Err(failure) => {
                let report = CrashReport::of_failure(&failure, started.elapsed());
                *self.startup.lock().unwrap_or_else(PoisonError::into_inner) =
                    StartupSummary::from(report.diagnostic());
                return Err(failure);
            }
        };
        *self.startup.lock().unwrap_or_else(PoisonError::into_inner) = startup;
        self.feed.publish(EventKind::WorkspaceOpened {
            records: summary.records,
        });
        if summary
            .conditions
            .iter()
            .any(|condition| condition.code() == "checkpoint-recovery-needs-attention")
        {
            self.feed
                .publish(EventKind::CheckpointRecoveryNeedsAttention);
        }
        Ok(summary)
    }

    /// Whether automatic checkpointing is disabled, waiting for a workspace, or active.
    #[must_use]
    pub fn automatic_checkpoint_status(&self) -> AutomaticCheckpointStatus {
        self.checkpoint
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .status()
    }

    /// Extend the current recovery window with one observed event.
    pub fn observe_checkpoint_activity(
        &self,
        sequence: RecoverySequence,
        changed_bytes: u64,
    ) -> Result<(), AutomaticCheckpointError> {
        let (mut checkpoint, _held) = self.checkpoint_mutation_guards()?;
        checkpoint
            .runtime_mut()?
            .observe(sequence, changed_bytes)
            .map_err(AutomaticCheckpointError::Runtime)
    }

    /// Save one caller-authorized file version and admit its activity into the coordinator.
    ///
    /// The caller owns the sequence, complete typed request, chunking and manifest paging policies,
    /// and head derivation.
    /// This method holds the coordinator exclusively, preflights its ordering before any write,
    /// then returns the acknowledgement only after CAS promotion, metadata commit, immutable
    /// journal append, live-workspace reload, and the post-journal activity observation all
    /// succeed. No threshold or policy value is selected here.
    pub fn save_file_version<D: HeadDerivation + ?Sized>(
        &self,
        sequence: RecoverySequence,
        bytes: &[u8],
        chunking: &ChunkingConfig,
        paging_policy: crate::ManifestPagingPolicy,
        request: FileVersionCheckpointRequest,
        derivation: &D,
    ) -> Result<JournaledPrivateSave, LiveCheckpointSaveError> {
        let (saved, _installation) = self.save_file_version_with_installation(
            sequence,
            bytes,
            chunking,
            paging_policy,
            request,
            derivation,
        )?;
        let _idle_worker = self.schedule_pending_checkpoint();
        Ok(saved)
    }

    fn save_file_version_with_installation<D: HeadDerivation + ?Sized>(
        &self,
        sequence: RecoverySequence,
        bytes: &[u8],
        chunking: &ChunkingConfig,
        paging_policy: crate::ManifestPagingPolicy,
        request: FileVersionCheckpointRequest,
        derivation: &D,
    ) -> Result<(JournaledPrivateSave, CheckpointInstallation), LiveCheckpointSaveError> {
        let _workspace_authority =
            self.lock_current_managed_workspace_mutation()
                .map_err(|error| match error {
                    ManagedTextFileError::NoWorkspace => LiveCheckpointSaveError::NoWorkspace,
                    ManagedTextFileError::StaleWorkspace => LiveCheckpointSaveError::Checkpoint(
                        AutomaticCheckpointError::WorkspaceChanged,
                    ),
                    error => LiveCheckpointSaveError::WorkspaceAuthority(error),
                })?;
        let changed_bytes =
            u64::try_from(bytes.len()).map_err(|_| LiveCheckpointSaveError::ByteCountTooLarge)?;
        // Lock order is checkpoint then workspace, matching `open_at_start`/`open_workspace`,
        // which install the checkpoint runtime before replacing the held workspace.
        let mut checkpoint = self
            .checkpoint
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut held = self.held();
        let open = held.as_ref().ok_or(LiveCheckpointSaveError::NoWorkspace)?;
        open.ensure_physical_root().map_err(|_| {
            LiveCheckpointSaveError::Checkpoint(AutomaticCheckpointError::WorkspaceChanged)
        })?;
        let installation = checkpoint
            .installation_for(open.database_file())
            .map_err(LiveCheckpointSaveError::Checkpoint)?;
        if open.checkpoint_recovery_needs_attention() {
            return Err(LiveCheckpointSaveError::Checkpoint(
                AutomaticCheckpointError::RecoveryNeedsAttention,
            ));
        }
        let runtime = checkpoint
            .runtime_mut()
            .map_err(LiveCheckpointSaveError::Checkpoint)?;
        runtime
            .validate_observation(sequence, changed_bytes)
            .map_err(|error| {
                LiveCheckpointSaveError::Checkpoint(AutomaticCheckpointError::Runtime(error))
            })?;
        let open = held.as_ref().ok_or(LiveCheckpointSaveError::NoWorkspace)?;
        let root = open.storage_root().as_path().to_path_buf();
        let cas = Cas::with_filesystem(root, open.storage_pinned_root().filesystem())
            .map_err(LiveCheckpointSaveError::ContentStore)?;
        let saved = crate::checkpoint_storage::save_file_version(
            held.as_mut().expect("the workspace was checked above"),
            &cas,
            bytes,
            chunking,
            paging_policy,
            request,
            derivation,
        );
        let saved = match saved {
            Ok(saved) => saved,
            Err(save) => {
                match held
                    .as_mut()
                    .expect("the workspace was checked above")
                    .refresh_with_trusted_reviewers(&self.trusted_reviewers)
                {
                    Ok(()) => {}
                    Err(reload) => {
                        return Err(LiveCheckpointSaveError::DurableSaveAndReload {
                            save: Box::new(save),
                            reload: Box::new(reload),
                        });
                    }
                }
                return Err(LiveCheckpointSaveError::DurableSave(save));
            }
        };

        held.as_mut()
            .expect("the workspace was checked above")
            .refresh_with_trusted_reviewers(&self.trusted_reviewers)
            .map_err(|error| LiveCheckpointSaveError::Reload(Box::new(error)))?;
        runtime
            .observe_durable(
                sequence,
                changed_bytes,
                checkpoint_stamp(sequence, saved.changeset_id()),
                saved.acknowledgement(),
            )
            .map_err(|error| {
                LiveCheckpointSaveError::ObservationAfterJournal(AutomaticCheckpointError::Runtime(
                    error,
                ))
            })?;
        self.record_checkpoint_creation(saved.linked_bytes());
        if let Err(error) = preserve_pending_recovery_if_due(
            runtime,
            held.as_mut().expect("the workspace was checked above"),
            Duration::ZERO,
        ) {
            held.as_mut()
                .expect("the workspace was checked above")
                .record_checkpoint_recovery_attention();
            return Err(LiveCheckpointSaveError::ObservationAfterJournal(error));
        }
        Ok((saved, installation))
    }

    fn save_operation_set<D: HeadDerivation + ?Sized>(
        &self,
        sequence: RecoverySequence,
        request: AuthenticatedOperationCheckpointRequest,
        derivation: &D,
    ) -> Result<(JournaledPrivateMutation, CheckpointInstallation), LiveCheckpointSaveError> {
        let _workspace_authority =
            self.lock_current_managed_workspace_mutation()
                .map_err(|error| match error {
                    ManagedTextFileError::NoWorkspace => LiveCheckpointSaveError::NoWorkspace,
                    ManagedTextFileError::StaleWorkspace => LiveCheckpointSaveError::Checkpoint(
                        AutomaticCheckpointError::WorkspaceChanged,
                    ),
                    error => LiveCheckpointSaveError::WorkspaceAuthority(error),
                })?;
        let mut checkpoint = self
            .checkpoint
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut held = self.held();
        let open = held.as_ref().ok_or(LiveCheckpointSaveError::NoWorkspace)?;
        open.ensure_physical_root().map_err(|_| {
            LiveCheckpointSaveError::Checkpoint(AutomaticCheckpointError::WorkspaceChanged)
        })?;
        let installation = checkpoint
            .installation_for(open.database_file())
            .map_err(LiveCheckpointSaveError::Checkpoint)?;
        if open.checkpoint_recovery_needs_attention() {
            return Err(LiveCheckpointSaveError::Checkpoint(
                AutomaticCheckpointError::RecoveryNeedsAttention,
            ));
        }
        let runtime = checkpoint
            .runtime_mut()
            .map_err(LiveCheckpointSaveError::Checkpoint)?;
        runtime.validate_observation(sequence, 1).map_err(|error| {
            LiveCheckpointSaveError::Checkpoint(AutomaticCheckpointError::Runtime(error))
        })?;
        let open = held.as_ref().ok_or(LiveCheckpointSaveError::NoWorkspace)?;
        let root = open.storage_root().as_path().to_path_buf();
        let cas = Cas::with_filesystem(root, open.storage_pinned_root().filesystem())
            .map_err(LiveCheckpointSaveError::ContentStore)?;
        let saved = save_authenticated_operations(
            held.as_mut().expect("the workspace was checked above"),
            &cas,
            request,
            derivation,
        );
        let saved = match saved {
            Ok(saved) => saved,
            Err(save) => {
                match held
                    .as_mut()
                    .expect("the workspace was checked above")
                    .refresh_with_trusted_reviewers(&self.trusted_reviewers)
                {
                    Ok(()) => {}
                    Err(reload) => {
                        return Err(LiveCheckpointSaveError::DurableSaveAndReload {
                            save: Box::new(save),
                            reload: Box::new(reload),
                        });
                    }
                }
                return Err(LiveCheckpointSaveError::DurableSave(save));
            }
        };
        held.as_mut()
            .expect("the workspace was checked above")
            .refresh_with_trusted_reviewers(&self.trusted_reviewers)
            .map_err(|error| LiveCheckpointSaveError::Reload(Box::new(error)))?;
        runtime
            .observe_durable(
                sequence,
                1,
                checkpoint_stamp(sequence, saved.changeset_id()),
                saved.acknowledgement(),
            )
            .map_err(|error| {
                LiveCheckpointSaveError::ObservationAfterJournal(AutomaticCheckpointError::Runtime(
                    error,
                ))
            })?;
        self.record_checkpoint_creation(saved.linked_bytes());
        if let Err(error) = preserve_pending_recovery_if_due(
            runtime,
            held.as_mut().expect("the workspace was checked above"),
            Duration::ZERO,
        ) {
            held.as_mut()
                .expect("the workspace was checked above")
                .record_checkpoint_recovery_attention();
            return Err(LiveCheckpointSaveError::ObservationAfterJournal(error));
        }
        Ok((saved, installation))
    }

    /// Save one file state that the nonauthoritative folder fallback recovered by rescanning.
    ///
    /// The candidate already proved that its bytes still match one appeared or content-changed
    /// entry in the later folder reading. This routes those exact bytes through the existing
    /// durable file-version transaction and records activity at the adapter's own sequence. It
    /// does not add boundary evidence, claim the rescan captured every change, or close the open
    /// window as meaningful. Only [`Self::save_settled_checkpoint`] can do the latter, after an
    /// explicit inactivity interval and with the real acknowledgement returned here. As in
    /// [`Self::save_file_version`], the caller remains responsible for the typed object/version,
    /// actor, parent, policy and signature fields in `request`; a folder path cannot invent them.
    pub fn save_detected_folder_file_version<D: HeadDerivation + ?Sized>(
        &self,
        detected: &DetectedFolderFile,
        chunking: &ChunkingConfig,
        paging_policy: crate::ManifestPagingPolicy,
        request: FileVersionCheckpointRequest,
        derivation: &D,
    ) -> Result<JournaledPrivateSave, LiveCheckpointSaveError> {
        let sequence = RecoverySequence::new(detected.sequence().number())
            .ok_or(LiveCheckpointSaveError::InvalidFolderEventSequence)?;
        // A rescan proves bytes, not a meaningful-save boundary. The ordinary public save path
        // schedules its own idle worker because the direct writer supplied an authenticated
        // acknowledgement. A recovery-detected observation must leave that decision explicit:
        // callers may schedule only after separately establishing the settling boundary.
        self.save_file_version_with_installation(
            sequence,
            detected.bytes(),
            chunking,
            paging_policy,
            request,
            derivation,
        )
        .map(|(saved, _installation)| saved)
    }

    /// Record exact candidate boundary evidence without moving a durable pointer.
    pub fn record_checkpoint_boundary(
        &self,
        trigger: RecoveryTrigger,
        evidence: RecoveryBoundaryEvidence,
    ) -> Result<RecoveryTransition, AutomaticCheckpointError> {
        let (mut checkpoint, _held) = self.checkpoint_mutation_guards()?;
        checkpoint
            .runtime_mut()?
            .record_boundary(trigger, evidence)
            .map_err(AutomaticCheckpointError::Runtime)
    }

    /// Preserve verified bytes for one immediate recovery-only trigger.
    pub fn preserve_recovery(
        &self,
        trigger: RecoveryTrigger,
        recovery: RecoveryPreserved,
    ) -> Result<RecoveryTransition, AutomaticCheckpointError> {
        let (mut checkpoint, _held) = self.checkpoint_mutation_guards()?;
        checkpoint
            .runtime_mut()?
            .preserve(trigger, recovery)
            .map_err(AutomaticCheckpointError::Runtime)
    }

    /// Preserve verified bytes if either configured maximum was reached.
    pub fn preserve_recovery_at_maximum(
        &self,
        elapsed: Duration,
        recovery: RecoveryPreserved,
    ) -> Result<Option<RecoveryTransition>, AutomaticCheckpointError> {
        let (mut checkpoint, _held) = self.checkpoint_mutation_guards()?;
        checkpoint
            .runtime_mut()?
            .preserve_at_maximum(elapsed, recovery)
            .map_err(AutomaticCheckpointError::Runtime)
    }

    /// Schedule the exact currently pending extent at both configured runtime deadlines.
    ///
    /// No thread is created when the snapshot has no pending durable save. At most one thread is
    /// alive for this daemon: a later extent resets that worker's full idle interval without
    /// postponing the maximum-uncheckpointed deadline, while a duplicate request for the same
    /// extent is a no-op. The maximum deadline preserves verified recovery only; the idle deadline
    /// separately verifies and closes the meaningful checkpoint. The exact workspace and final
    /// sequence are captured after every wake, so replacing the workspace or observing a newer
    /// event cannot let an old timer mutate a new activity window.
    pub fn schedule_pending_checkpoint(
        &self,
    ) -> Option<std::thread::JoinHandle<Result<Option<RecoveryTransition>, AutomaticCheckpointError>>>
    {
        let scheduled = {
            let checkpoint = self
                .checkpoint
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let (_, through, installation) = checkpoint.pending_idle_schedule()?;
            let held = self.held();
            let open = held.as_ref()?;
            if open.ensure_physical_root().is_err() {
                return None;
            }
            if !checkpoint.is_installed_for(open.database_file()) {
                return None;
            }
            IdleCheckpointSchedule {
                through,
                installation,
                root: open.physical_root().as_path().to_path_buf(),
            }
        };
        {
            let mut idle = self
                .checkpoint_idle
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if idle.shutdown {
                return None;
            }
            match idle.publish(scheduled) {
                IdleScheduleUpdate::Duplicate | IdleScheduleUpdate::Stale => return None,
                IdleScheduleUpdate::WakeWorker => {
                    self.checkpoint_idle.wake.notify_one();
                    return None;
                }
                IdleScheduleUpdate::StartWorker => {
                    idle.active_workers += 1;
                }
            }
        }
        let checkpoint = Arc::downgrade(&self.checkpoint);
        let open = Arc::downgrade(&self.open);
        let feed = Arc::downgrade(&self.feed);
        let checkpoint_idle = Arc::clone(&self.checkpoint_idle);
        let lifetime = CheckpointWorkerLifetime(Arc::clone(&checkpoint_idle));
        let worker = move || loop {
            let (Some(checkpoint), Some(open), Some(feed)) =
                (checkpoint.upgrade(), open.upgrade(), feed.upgrade())
            else {
                let mut idle = checkpoint_idle
                    .state
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                idle.worker_running = false;
                idle.scheduled = None;
                idle.maximum_started_at = None;
                return Ok(None);
            };
            #[cfg(test)]
            {
                let gate = checkpoint_idle.worker_gate.lock().unwrap().take();
                if let Some(gate) = gate {
                    gate.entered.send(()).unwrap();
                    gate.release.recv().unwrap();
                    assert!(!gate.panic_after_release, "injected worker unwind");
                }
            }
            let (intervals, scheduled, generation) = {
                let checkpoint = checkpoint.lock().unwrap_or_else(PoisonError::into_inner);
                let held = open.lock().unwrap_or_else(PoisonError::into_inner);
                let schedule = checkpoint.pending_idle_schedule().and_then(
                    |(intervals, through, installation)| {
                        let workspace = held.as_ref()?;
                        workspace.ensure_physical_root().ok()?;
                        checkpoint
                            .is_installed_for(workspace.database_file())
                            .then(|| {
                                (
                                    intervals,
                                    through,
                                    installation,
                                    workspace.physical_root().as_path().to_path_buf(),
                                )
                            })
                    },
                );
                let mut idle = checkpoint_idle
                    .state
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                let Some((intervals, through, installation, root)) = schedule else {
                    idle.worker_running = false;
                    idle.scheduled = None;
                    idle.maximum_started_at = None;
                    return Ok(None);
                };
                if idle.shutdown {
                    idle.worker_running = false;
                    idle.scheduled = None;
                    idle.maximum_started_at = None;
                    return Ok(None);
                }
                (
                    intervals,
                    IdleCheckpointSchedule {
                        through,
                        installation,
                        root,
                    },
                    idle.generation,
                )
            };

            let mut idle = checkpoint_idle
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if idle.shutdown {
                return Ok(None);
            }
            if idle.generation != generation {
                continue;
            }
            if idle.retire_unpublished_replacement(generation, &scheduled) {
                // A workspace can be replaced before this newly spawned worker gets its first
                // turn. Startup opens deliberately do not publish a replacement schedule, so the
                // checkpoint's installed workspace may already differ while the scheduler still
                // carries this worker's old generation. Continuing here would compare the same
                // two schedules forever. Retire the stale worker; an explicit schedule for the
                // replacement can then start a fresh one with its exact installation identity.
                return Ok(None);
            }
            let (first_interval, maximum_first) = idle.next_wait_at(intervals, Instant::now());
            let (idle, waited) = checkpoint_idle
                .wake
                .wait_timeout(idle, first_interval)
                .unwrap_or_else(PoisonError::into_inner);
            if idle.shutdown {
                return Ok(None);
            }
            if !waited.timed_out()
                || idle.generation != generation
                || idle.scheduled.as_ref() != Some(&scheduled)
            {
                continue;
            }
            drop(idle);

            if maximum_first {
                let mut checkpoint = checkpoint.lock().unwrap_or_else(PoisonError::into_inner);
                let mut held = open.lock().unwrap_or_else(PoisonError::into_inner);
                let mut idle = checkpoint_idle
                    .state
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                if idle.shutdown
                    || idle.generation != generation
                    || idle.scheduled.as_ref() != Some(&scheduled)
                {
                    continue;
                }
                let attention_was_visible = held
                    .as_ref()
                    .is_some_and(OpenWorkspace::checkpoint_recovery_needs_attention);
                let recovery = Self::preserve_pending_recovery_locked(
                    &mut checkpoint,
                    &mut held,
                    intervals.maximum,
                    &scheduled.root,
                    scheduled.installation,
                    scheduled.through,
                );
                let attention_is_visible = held
                    .as_ref()
                    .is_some_and(OpenWorkspace::checkpoint_recovery_needs_attention);
                if checkpoint_attention_became_visible(attention_was_visible, attention_is_visible)
                {
                    feed.publish(EventKind::CheckpointRecoveryNeedsAttention);
                }
                if let Err(error) = recovery {
                    idle.worker_running = false;
                    idle.scheduled = None;
                    idle.maximum_started_at = None;
                    return Err(error);
                }
                idle.maximum_started_at = None;
                drop(idle);
                drop(held);
                drop(checkpoint);
            }

            let remaining = intervals.idle.saturating_sub(first_interval);
            if !remaining.is_zero() {
                let idle = checkpoint_idle
                    .state
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                if idle.shutdown {
                    return Ok(None);
                }
                if idle.generation != generation || idle.scheduled.as_ref() != Some(&scheduled) {
                    continue;
                }
                let (idle, waited) = checkpoint_idle
                    .wake
                    .wait_timeout(idle, remaining)
                    .unwrap_or_else(PoisonError::into_inner);
                if idle.shutdown {
                    return Ok(None);
                }
                if !waited.timed_out()
                    || idle.generation != generation
                    || idle.scheduled.as_ref() != Some(&scheduled)
                {
                    continue;
                }
                drop(idle);
            }

            let mut checkpoint = checkpoint.lock().unwrap_or_else(PoisonError::into_inner);
            let mut held = open.lock().unwrap_or_else(PoisonError::into_inner);
            let mut idle = checkpoint_idle
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if idle.shutdown
                || idle.generation != generation
                || idle.scheduled.as_ref() != Some(&scheduled)
            {
                continue;
            }
            let attention_was_visible = held
                .as_ref()
                .is_some_and(OpenWorkspace::checkpoint_recovery_needs_attention);
            let result = Self::settle_pending_checkpoint_locked(
                &mut checkpoint,
                &mut held,
                intervals.idle,
                Some(&scheduled.root),
                Some(scheduled.installation),
                Some(scheduled.through),
            );
            let attention_is_visible = held
                .as_ref()
                .is_some_and(OpenWorkspace::checkpoint_recovery_needs_attention);
            if checkpoint_attention_became_visible(attention_was_visible, attention_is_visible) {
                feed.publish(EventKind::CheckpointRecoveryNeedsAttention);
            }
            idle.worker_running = false;
            idle.scheduled = None;
            idle.maximum_started_at = None;
            return result;
        };
        Some(std::thread::spawn(move || {
            let _lifetime = lifetime;
            worker()
        }))
    }

    fn settle_pending_checkpoint_for(
        &self,
        elapsed: Duration,
        expected_root: Option<&Path>,
        expected_installation: Option<CheckpointInstallation>,
        expected_through: Option<RecoverySequence>,
    ) -> Result<Option<RecoveryTransition>, AutomaticCheckpointError> {
        // Keep the established checkpoint -> workspace order so restart settlement cannot race a
        // save or workspace replacement into validating one workspace and mutating another.
        let mut checkpoint = self
            .checkpoint
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut held = self.held();
        Self::settle_pending_checkpoint_locked(
            &mut checkpoint,
            &mut held,
            elapsed,
            expected_root,
            expected_installation,
            expected_through,
        )
    }

    fn preserve_pending_recovery_for(
        &self,
        elapsed: Duration,
        expected_root: &Path,
        expected_installation: CheckpointInstallation,
        expected_through: RecoverySequence,
    ) -> Result<Option<RecoveryTransition>, AutomaticCheckpointError> {
        // Match the worker's checkpoint -> workspace lock order. Managed mutations wait for their
        // own stability check instead of delegating meaningful settlement to that worker, but the
        // maximum recovery deadline still needs the same exact-workspace guard and attention
        // projection.
        let mut checkpoint = self
            .checkpoint
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut held = self.held();
        let attention_was_visible = held
            .as_ref()
            .is_some_and(OpenWorkspace::checkpoint_recovery_needs_attention);
        let result = Self::preserve_pending_recovery_locked(
            &mut checkpoint,
            &mut held,
            elapsed,
            expected_root,
            expected_installation,
            expected_through,
        );
        let attention_is_visible = held
            .as_ref()
            .is_some_and(OpenWorkspace::checkpoint_recovery_needs_attention);
        if checkpoint_attention_became_visible(attention_was_visible, attention_is_visible) {
            self.feed
                .publish(EventKind::CheckpointRecoveryNeedsAttention);
        }
        result
    }

    fn checkpoint_settled_for(
        &self,
        expected_root: &Path,
        expected_installation: CheckpointInstallation,
        expected_through: RecoverySequence,
    ) -> Result<bool, AutomaticCheckpointError> {
        let checkpoint = self
            .checkpoint
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let held = self.held();
        let Some(open) = held.as_ref() else {
            return Ok(false);
        };
        if open.ensure_physical_root().is_err() {
            return Ok(false);
        }
        if checkpoint.active_installation != Some(expected_installation)
            || open.physical_root().as_path() != expected_root
            || !checkpoint.is_installed_for(open.database_file())
        {
            return Ok(false);
        }
        Ok(checkpoint
            .runtime()?
            .machine()
            .snapshot()
            .last_meaningful()
            .is_some_and(|checkpoint| checkpoint.through() == expected_through))
    }

    fn settle_pending_checkpoint_locked(
        checkpoint: &mut LiveCheckpointRuntime,
        held: &mut Option<OpenWorkspace>,
        elapsed: Duration,
        expected_root: Option<&Path>,
        expected_installation: Option<CheckpointInstallation>,
        expected_through: Option<RecoverySequence>,
    ) -> Result<Option<RecoveryTransition>, AutomaticCheckpointError> {
        if expected_installation
            .is_some_and(|expected| checkpoint.active_installation != Some(expected))
        {
            return Ok(None);
        }
        let open = held.as_mut().ok_or(AutomaticCheckpointError::NoWorkspace)?;
        open.ensure_physical_root()
            .map_err(|_| AutomaticCheckpointError::WorkspaceChanged)?;
        if expected_root.is_some_and(|expected| open.physical_root().as_path() != expected) {
            return Ok(None);
        }
        // `workspace.open` installs the new checkpoint runtime before it swaps the held workspace.
        // A worker that wakes inside that narrow handoff must settle neither the old workspace with
        // the new runtime nor the new workspace with the old runtime.
        if !checkpoint.is_installed_for(open.database_file()) {
            return Ok(None);
        }
        let runtime = checkpoint.runtime_mut()?;
        let Some((settled, pending)) = runtime.pending_settlement(elapsed) else {
            return Ok(None);
        };
        if expected_through.is_some_and(|expected| pending.through() != expected) {
            return Ok(None);
        }
        if !open.has_operation(&pending.stamp().content_hash()) {
            open.record_checkpoint_recovery_attention();
            return Err(AutomaticCheckpointError::PendingAcknowledgementInvalid(
                "the pending ChangeSet is absent from the immutable journal".to_owned(),
            ));
        }
        let acknowledgement = match open.verify_pending_private_save(pending) {
            Ok(acknowledgement) => acknowledgement,
            Err(current_error) => {
                let historical =
                    runtime
                        .machine()
                        .snapshot()
                        .latest_recovery()
                        .and_then(|recovery| {
                            verify_journal_recovery_pointer(open, Some(pending), recovery)
                                .transpose()
                        });
                match historical {
                    Some(Ok(boundary)) => open
                        .verify_pending_private_save_at_boundary(pending, boundary)
                        .map_err(|error| error.to_string()),
                    Some(Err(error)) => Err(error.to_string()),
                    None => Err(current_error.to_string()),
                }
                .map_err(|detail| {
                    open.record_checkpoint_recovery_attention();
                    AutomaticCheckpointError::PendingAcknowledgementInvalid(detail)
                })?
            }
        };
        match runtime.save_verified_pending(settled, acknowledgement) {
            Ok(transition) => {
                open.dismiss_checkpoint_recovery_attention();
                Ok(Some(transition))
            }
            Err(error) => {
                open.record_checkpoint_recovery_attention();
                Err(AutomaticCheckpointError::Runtime(error))
            }
        }
    }

    fn preserve_pending_recovery_locked(
        checkpoint: &mut LiveCheckpointRuntime,
        held: &mut Option<OpenWorkspace>,
        elapsed: Duration,
        expected_root: &Path,
        expected_installation: CheckpointInstallation,
        expected_through: RecoverySequence,
    ) -> Result<Option<RecoveryTransition>, AutomaticCheckpointError> {
        if checkpoint.active_installation != Some(expected_installation) {
            return Ok(None);
        }
        let open = held.as_mut().ok_or(AutomaticCheckpointError::NoWorkspace)?;
        open.ensure_physical_root()
            .map_err(|_| AutomaticCheckpointError::WorkspaceChanged)?;
        if open.physical_root().as_path() != expected_root
            || !checkpoint.is_installed_for(open.database_file())
        {
            return Ok(None);
        }
        let runtime = checkpoint.runtime_mut()?;
        if runtime
            .machine()
            .snapshot()
            .pending_meaningful()
            .is_none_or(|pending| pending.through() != expected_through)
        {
            return Ok(None);
        }
        match preserve_pending_recovery_if_due(runtime, open, elapsed) {
            Ok(transition) => Ok(transition),
            Err(error) => {
                open.record_checkpoint_recovery_attention();
                Err(error)
            }
        }
    }

    /// Clone the current restored recovery truth for diagnostics and restart tests.
    pub fn checkpoint_snapshot(&self) -> Result<RecoverySnapshot, AutomaticCheckpointError> {
        let checkpoint = self.checkpoint_ref()?;
        Ok(checkpoint_snapshot(checkpoint.runtime()?))
    }

    /// Clone checkpoint truth together with the exact workspace root and fold digest it belongs to.
    ///
    /// The checkpoint and workspace locks are held together in the same order used by workspace
    /// installation. A desktop reader therefore cannot combine a summary from one workspace with
    /// recovery state from another one when `workspace.open` wins between two separate reads. The
    /// digest is load-bearing when a directory is replaced and reopened at the same path.
    pub fn checkpoint_snapshot_for_open_workspace(
        &self,
    ) -> Result<(String, String, String, RecoverySnapshot), AutomaticCheckpointError> {
        let checkpoint = self.checkpoint_mut()?;
        let held = self.held();
        let open = held.as_ref().ok_or(AutomaticCheckpointError::NoWorkspace)?;
        checkpoint.ensure_installed_for(open.database_file())?;
        let root = open.root().as_path().display().to_string();
        let digest = open.digest().to_string();
        let installation = open.installation();
        let snapshot = checkpoint_snapshot(checkpoint.runtime()?);
        Ok((root, digest, installation, snapshot))
    }

    /// Read one UTF-8 file through the native desktop boundary.
    ///
    /// The path must name an exact file in complete durable history and still resolve through
    /// regular, non-symlink filesystem components. This is intentionally not daemon IPC.
    pub fn read_managed_text_file(
        &self,
        relative_path: &str,
    ) -> Result<ManagedTextFile, ManagedTextFileError> {
        let held = self.held();
        let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
        open.ensure_physical_root()
            .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
        let history = open
            .file_histories()
            .iter()
            .find(|history| history.path() == relative_path)
            .ok_or(ManagedTextFileError::NotRegularFile)?;
        let current = history
            .current()
            .ok_or(ManagedTextFileError::NotRegularFile)?;
        let manifest = open
            .manifest_record(current.manifest())
            .ok_or(ManagedTextFileError::NotRegularFile)?;
        let metadata = open
            .file_version_metadata(current.version())
            .ok_or(ManagedTextFileError::NotRegularFile)?;
        read_managed_text(
            open.physical_root().as_path(),
            relative_path,
            current.version().to_string(),
            manifest.content_digest,
            metadata,
        )
    }

    /// Inspect one tracked regular file without requiring it to be text-editor compatible.
    ///
    /// The path and durable version come from complete private history. The operating-system read
    /// uses the same confined, non-symlink boundary as editing, and its BLAKE3 digest is compared
    /// with the current manifest. Binary and oversized files return no text but remain eligible
    /// for the existing authenticated private-save operation.
    pub fn inspect_managed_file(
        &self,
        relative_path: &str,
    ) -> Result<ManagedFileInspection, ManagedTextFileError> {
        let held = self.held();
        let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
        open.ensure_physical_root()
            .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
        let history = open
            .file_histories()
            .iter()
            .find(|history| history.path() == relative_path)
            .ok_or(ManagedTextFileError::NotRegularFile)?;
        let current = history
            .current()
            .ok_or(ManagedTextFileError::NotRegularFile)?;
        let manifest = open
            .manifest_record(current.manifest())
            .ok_or(ManagedTextFileError::NotRegularFile)?;
        let metadata = open
            .file_version_metadata(current.version())
            .ok_or(ManagedTextFileError::NotRegularFile)?;
        inspect_managed_file(
            open.physical_root().as_path(),
            relative_path,
            current.version().to_string(),
            manifest.content_digest,
            metadata,
        )
    }

    /// Inspect mutable working bytes together with the exact bounded text of their saved basis.
    ///
    /// This is the desktop editor comparison seam. General scans intentionally use
    /// [`Self::inspect_managed_file`] so a multi-file scan never reconstructs retained bodies it
    /// does not display.
    pub fn inspect_managed_file_with_durable_text(
        &self,
        relative_path: &str,
        text_byte_limit: usize,
    ) -> Result<(ManagedFileInspection, Option<String>), ManagedTextFileError> {
        let held = self.held();
        let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
        open.ensure_physical_root()
            .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
        let history = open
            .file_histories()
            .iter()
            .find(|history| history.path() == relative_path)
            .ok_or(ManagedTextFileError::NotRegularFile)?;
        let current = history
            .current()
            .ok_or(ManagedTextFileError::NotRegularFile)?;
        let manifest = open
            .manifest_record(current.manifest())
            .ok_or(ManagedTextFileError::NotRegularFile)?;
        let metadata = open
            .file_version_metadata(current.version())
            .ok_or(ManagedTextFileError::NotRegularFile)?;
        let file = inspect_managed_file(
            open.physical_root().as_path(),
            relative_path,
            current.version().to_string(),
            manifest.content_digest,
            metadata,
        )?;
        let bounded_byte_limit = text_byte_limit.min(MAX_MANAGED_TEXT_BYTES);
        let durable_text = if file.text().is_some()
            && file.byte_count() <= u64::try_from(bounded_byte_limit).unwrap_or(u64::MAX)
        {
            open.current_durable_text(relative_path, bounded_byte_limit)
                .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?
        } else {
            None
        };
        Ok((file, durable_text))
    }

    fn inspect_managed_file_bounded(
        &self,
        relative_path: &str,
        byte_limit: usize,
    ) -> Result<ManagedFileInspection, ManagedTextFileError> {
        let held = self.held();
        let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
        open.ensure_physical_root()
            .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
        let history = open
            .file_histories()
            .iter()
            .find(|history| history.path() == relative_path)
            .ok_or(ManagedTextFileError::NotRegularFile)?;
        let current = history
            .current()
            .ok_or(ManagedTextFileError::NotRegularFile)?;
        let manifest = open
            .manifest_record(current.manifest())
            .ok_or(ManagedTextFileError::NotRegularFile)?;
        let metadata = open
            .file_version_metadata(current.version())
            .ok_or(ManagedTextFileError::NotRegularFile)?;
        inspect_managed_file_bounded(
            open.physical_root().as_path(),
            relative_path,
            current.version().to_string(),
            manifest.content_digest,
            metadata,
            byte_limit,
        )
    }

    /// Inspect one native regular file that is not yet present in durable workspace history.
    ///
    /// Discovery is deliberately nonauthoritative: a folder rescan cannot prove when the file was
    /// created. This method reopens the exact confined file and returns exact bytes for review,
    /// including a descendant whose new parent is present on disk but not durable yet. Adoption
    /// still requires that parent to have been admitted first.
    pub fn inspect_native_untracked_file(
        &self,
        relative_path: &str,
    ) -> Result<NativeFileInspection, ManagedTextFileError> {
        let held = self.held();
        let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
        open.ensure_physical_root()
            .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
        open.managed_discovery_target(relative_path)
            .map_err(|_| ManagedTextFileError::TargetExists)?;
        let root = open.physical_root().as_path();
        let (target, bytes) = read_managed_replacement(root, relative_path)?;
        Ok(NativeFileInspection::from_owned_bytes(
            relative_path.to_owned(),
            bytes,
            target.executable(),
        ))
    }

    fn inspect_native_untracked_file_bounded(
        &self,
        relative_path: &str,
        byte_limit: usize,
    ) -> Result<NativeFileInspection, ManagedTextFileError> {
        let held = self.held();
        let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
        open.ensure_physical_root()
            .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
        open.managed_discovery_target(relative_path)
            .map_err(|_| ManagedTextFileError::TargetExists)?;
        inspect_native_file_bounded(open.physical_root().as_path(), relative_path, byte_limit)
    }

    /// Reopen one durable managed directory and report the exact current operating-system object.
    ///
    /// This is read-back evidence for an ambiguous directory-adoption reply. Unlike native-only
    /// discovery, the path must already be a durable folder in the currently open workspace.
    pub fn inspect_managed_directory_installation(
        &self,
        relative_path: &str,
    ) -> Result<NativeDirectoryInspection, ManagedTextFileError> {
        let held = self.held();
        let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
        open.ensure_physical_root()
            .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
        if !open
            .entries()
            .iter()
            .any(|entry| entry.path() == relative_path && entry.entry_type() == "folder")
        {
            return Err(ManagedTextFileError::NotRegularFile);
        }
        let (_path, identity) =
            inspect_managed_directory(open.physical_root().as_path(), relative_path)?;
        Ok(NativeDirectoryInspection::new(
            relative_path.to_owned(),
            identity.token(),
        ))
    }

    /// Discover the complete native-only directory tree and bind every row to its exact OS object.
    ///
    /// This is review evidence, not mutation authority. The later adoption still requires the
    /// immediate parent in durable history, so callers must consume this list parent first.
    pub fn native_untracked_directories(
        &self,
    ) -> Result<Vec<NativeDirectoryInspection>, ManagedTextFileError> {
        let held = self.held();
        let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
        open.ensure_physical_root()
            .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
        let root = open.physical_root().as_path();
        open.native_untracked_directories()
            .map_err(|error| ManagedTextFileError::Authoring(error.to_string()))?
            .into_iter()
            .map(|relative| {
                // Reconfirm durable-name absence and obtain identity from an opened no-follow
                // directory descriptor. The later parent-first adoption must reproduce this token.
                open.managed_discovery_target(&relative)
                    .map_err(ManagedTextFileError::Authoring)?;
                let (_path, identity) = inspect_managed_directory(root, &relative)?;
                Ok(NativeDirectoryInspection::new(relative, identity.token()))
            })
            .collect()
    }

    /// Discover tracked files whose exact durable name is absent from its confined native parent.
    ///
    /// This does not authorize a deletion or infer a rename. Existing non-regular entries,
    /// symlinks, missing parents and unreadable paths are omitted so they continue through the
    /// ordinary fail-closed inspection path. The later mutation binds the exact durable version
    /// and rechecks absence under the workspace replacement serial.
    pub fn native_missing_files(&self) -> Result<Vec<NativeMissingFile>, ManagedTextFileError> {
        let held = self.held();
        let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
        open.ensure_physical_root()
            .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
        let root = open.physical_root().as_path();
        let mut missing = Vec::new();
        for history in open.file_histories() {
            let Some(current) = history.current() else {
                continue;
            };
            if confined_free_path(root, history.path()).is_err() {
                continue;
            }
            let manifest = open
                .manifest_record(current.manifest())
                .ok_or(ManagedTextFileError::NotRegularFile)?;
            let metadata = open
                .file_version_metadata(current.version())
                .ok_or(ManagedTextFileError::NotRegularFile)?;
            missing.push(NativeMissingFile::new(
                history.path().to_owned(),
                current.version().to_string(),
                manifest.content_digest,
                metadata.is_executable(),
            ));
        }
        Ok(missing)
    }

    /// Preview copying one current saved file into an ordinary folder.
    ///
    /// The selected working file must exactly match private history. The destination is confined
    /// beneath a separately pinned directory. Its exact regular-file state is captured when it
    /// exists; otherwise the exact existing parent is captured and directory ancestry is never
    /// created or guessed.
    pub fn preview_managed_file_export(
        &self,
        relative_path: &str,
        target_root: &Path,
    ) -> Result<ManagedFileExportPreview, ManagedTextFileError> {
        let held = self.held();
        let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
        let managed_root = open.physical_root().as_path();
        validate_independent_export_root(managed_root, open.storage_root().as_path(), target_root)?;
        let (version, source) = open
            .current_durable_file(relative_path)
            .map_err(|error| ManagedTextFileError::RetainedContent(error.to_string()))?;
        let (working_target, working_bytes) =
            read_managed_replacement(managed_root, relative_path)?;
        if working_bytes != source.bytes || working_target.executable() != source.executable {
            return Err(ManagedTextFileError::UnsavedWorkingCopy);
        }
        let target_installation = managed_directory_identity(target_root)
            .map_err(|error| ManagedTextFileError::io("inspect export folder", target_root, error))?
            .token();
        let target = inspect_managed_export_target(target_root, relative_path)?;
        let target_parent_installation = target.parent_installation();
        let target_file_installation = target.file_installation();
        let (target_relation, replace_allowed) = match &target {
            ManagedExportTarget::Existing(target, bytes)
                if bytes == &source.bytes && target.executable() == source.executable =>
            {
                ("identical", false)
            }
            ManagedExportTarget::Existing(target, bytes) => existing_export_target_relation(
                open,
                target_root,
                &target_installation,
                relative_path,
                &target.file_installation(),
                bytes,
                target.executable(),
            ),
            ManagedExportTarget::Missing { .. } => ("absent", true),
        };
        let target = match &target {
            ManagedExportTarget::Existing(target, bytes) => {
                Some((bytes.as_slice(), target.executable()))
            }
            ManagedExportTarget::Missing { .. } => None,
        };
        Ok(ManagedFileExportPreview::new(
            relative_path.to_owned(),
            version,
            &source.bytes,
            source.executable,
            target_root.display().to_string(),
            target_installation,
            target_parent_installation,
            target_file_installation,
            target,
            target_relation,
            replace_allowed,
        ))
    }

    /// Preview every current saved file for one whole-workspace Pull-back.
    ///
    /// The returned rows are sorted by durable relative path. The desktop can therefore cross the
    /// native bridge once without weakening the exact per-file source, working-copy, destination,
    /// and provenance checks performed by [`Self::preview_managed_file_export`]. Workspace
    /// replacement remains serialized by the caller's [`Self::with_verified_managed_workspace`]
    /// guard; this method retains only one file preview's reconstructed bytes at a time.
    pub fn preview_all_managed_file_exports(
        &self,
        target_root: &Path,
    ) -> Result<Vec<ManagedFileExportPreview>, ManagedTextFileError> {
        let mut paths = {
            let held = self.held();
            let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
            open.file_histories()
                .iter()
                .map(|history| history.path().to_owned())
                .collect::<Vec<_>>()
        };
        paths.sort();
        paths
            .into_iter()
            .map(|path| {
                self.preview_managed_file_export(&path, target_root)
                    .map(ManagedFileExportPreview::without_text)
            })
            .collect()
    }

    /// Preview one saved directory for create-only export into an ordinary folder.
    ///
    /// Existing ordinary directories are treated as structural no-ops. Files, links, missing
    /// ancestry, and any source or destination outside the exact confined roots are refused.
    pub fn preview_managed_directory_export(
        &self,
        relative_path: &str,
        target_root: &Path,
    ) -> Result<ManagedDirectoryExportPreview, ManagedTextFileError> {
        let held = self.held();
        let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
        let managed_root = open.physical_root().as_path();
        validate_independent_export_root(managed_root, open.storage_root().as_path(), target_root)?;
        let basis = open
            .managed_entry_basis(relative_path)
            .map_err(ManagedTextFileError::RetainedContent)?;
        if !basis.is_directory {
            return Err(ManagedTextFileError::NotRegularFile);
        }
        let (_, source_identity) = inspect_managed_directory(managed_root, relative_path)?;
        let target_installation = managed_directory_identity(target_root)
            .map_err(|error| ManagedTextFileError::io("inspect export folder", target_root, error))?
            .token();
        let target = inspect_managed_directory_export_target(target_root, relative_path)?;
        Ok(ManagedDirectoryExportPreview::new(
            relative_path.to_owned(),
            source_identity.token(),
            target_root.display().to_string(),
            target_installation,
            target.parent_installation(),
            target.directory_installation(),
        ))
    }

    /// Preview every missing saved directory through one whole-workspace native call.
    ///
    /// Paths are returned parent before child. Once a missing ancestor is observed, descendants
    /// are known to require creation without probing through an absent parent. The exact ordinary
    /// destination root is checked before and after the scan so confirmation cannot silently move
    /// the reviewed plan to a replacement directory.
    pub fn preview_managed_directory_exports(
        &self,
        target_root: &Path,
    ) -> Result<ManagedDirectoryExportBatchPreview, ManagedTextFileError> {
        let (managed_root, storage_root, mut paths) = {
            let held = self.held();
            let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
            let paths = open
                .entries()
                .iter()
                .filter(|entry| entry.entry_type() == "folder")
                .map(|entry| entry.path().to_owned())
                .collect::<Vec<_>>();
            (
                open.physical_root().as_path().to_owned(),
                open.storage_root().as_path().to_owned(),
                paths,
            )
        };
        validate_independent_export_root(&managed_root, &storage_root, target_root)?;
        let target_installation = managed_directory_identity(target_root)
            .map_err(|error| ManagedTextFileError::io("inspect export folder", target_root, error))?
            .token();
        paths.sort_by(|left, right| {
            let depth = left.split('/').count().cmp(&right.split('/').count());
            depth.then_with(|| left.cmp(right))
        });
        let mut missing = Vec::new();
        let mut missing_frontiers = Vec::new();
        for path in paths {
            if missing_frontiers.iter().any(|ancestor: &String| {
                path.len() > ancestor.len()
                    && path.starts_with(ancestor)
                    && path.as_bytes()[ancestor.len()] == b'/'
            }) {
                missing.push(path);
                continue;
            }
            let preview = self.preview_managed_directory_export(&path, target_root)?;
            if preview.target_installation() != target_installation {
                return Err(ManagedTextFileError::StaleExportTarget);
            }
            if !preview.target_exists() {
                missing_frontiers.push(path.clone());
                missing.push(path);
            }
        }
        let verified_target_installation = managed_directory_identity(target_root)
            .map_err(|error| ManagedTextFileError::io("inspect export folder", target_root, error))?
            .token();
        if verified_target_installation != target_installation {
            return Err(ManagedTextFileError::StaleExportTarget);
        }
        Ok(ManagedDirectoryExportBatchPreview::new(
            target_root.display().to_string(),
            target_installation,
            missing,
        ))
    }

    /// Confirm one exactly previewed create-only directory export.
    ///
    /// This creates no ancestry and copies no contents. A fresh file preview after successful
    /// folder creation binds every file to the newly created exact parent before bytes can move.
    #[allow(clippy::too_many_arguments)]
    pub fn export_managed_directory(
        &self,
        relative_path: &str,
        target_root: &Path,
        expected_source_directory_installation: &str,
        expected_target_installation: &str,
        expected_target_parent_installation: &str,
        expected_target_directory_installation: Option<&str>,
    ) -> Result<ManagedDirectoryExport, ManagedTextFileError> {
        self.export_managed_directory_requirement(
            relative_path,
            target_root,
            expected_source_directory_installation,
            expected_target_installation,
            expected_target_parent_installation,
            expected_target_directory_installation,
            false,
        )
    }

    /// Confirm one directory export only while its source is the exact current shared version.
    #[allow(clippy::too_many_arguments)]
    pub fn export_shared_managed_directory(
        &self,
        relative_path: &str,
        target_root: &Path,
        expected_source_directory_installation: &str,
        expected_target_installation: &str,
        expected_target_parent_installation: &str,
        expected_target_directory_installation: Option<&str>,
    ) -> Result<ManagedDirectoryExport, ManagedTextFileError> {
        self.export_managed_directory_requirement(
            relative_path,
            target_root,
            expected_source_directory_installation,
            expected_target_installation,
            expected_target_parent_installation,
            expected_target_directory_installation,
            true,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn export_managed_directory_requirement(
        &self,
        relative_path: &str,
        target_root: &Path,
        expected_source_directory_installation: &str,
        expected_target_installation: &str,
        expected_target_parent_installation: &str,
        expected_target_directory_installation: Option<&str>,
        require_shared: bool,
    ) -> Result<ManagedDirectoryExport, ManagedTextFileError> {
        let _workspace_authority = self.lock_current_managed_workspace_mutation()?;
        let _serial = self
            .managed_edit
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let held = self.held();
        let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
        if require_shared {
            Self::ensure_current_shared_version(open)?;
        }
        let managed_root = open.physical_root().as_path();
        validate_independent_export_root(managed_root, open.storage_root().as_path(), target_root)?;
        let basis = open
            .managed_entry_basis(relative_path)
            .map_err(ManagedTextFileError::RetainedContent)?;
        if !basis.is_directory {
            return Err(ManagedTextFileError::NotRegularFile);
        }
        let (_, source_identity) = inspect_managed_directory(managed_root, relative_path)?;
        if source_identity.token() != expected_source_directory_installation {
            return Err(ManagedTextFileError::UnsavedWorkingCopy);
        }
        let target_installation = managed_directory_identity(target_root)
            .map_err(|error| ManagedTextFileError::io("inspect export folder", target_root, error))?
            .token();
        if target_installation != expected_target_installation {
            return Err(ManagedTextFileError::StaleExportTarget);
        }
        let target = inspect_managed_directory_export_target(target_root, relative_path).map_err(
            |error| {
                if matches!(error, ManagedTextFileError::NotRegularFile) {
                    ManagedTextFileError::StaleExportTarget
                } else {
                    error
                }
            },
        )?;
        if target.parent_installation() != expected_target_parent_installation
            || target.directory_installation().as_deref() != expected_target_directory_installation
        {
            return Err(ManagedTextFileError::StaleExportTarget);
        }
        let filesystem = open.storage_pinned_root().filesystem();
        let workspace_installation = open.installation();
        let (created, created_identity) = match target {
            ManagedDirectoryExportTarget::Existing { .. } => (false, None),
            ManagedDirectoryExportTarget::Missing { ref path, .. } => {
                if expected_target_directory_installation.is_some() {
                    return Err(ManagedTextFileError::StaleExportTarget);
                }
                let recovered = recover_prepared_export_directory_in_exact_parent(
                    path,
                    target.parent_identity(),
                    |identity| {
                        let installation = identity.token();
                        pull_back_proves_directory(
                            &filesystem,
                            open.storage_root().as_path(),
                            &PullBackDirectoryReceipt {
                                workspace_installation: &workspace_installation,
                                target_root_installation: &target_installation,
                                path: relative_path,
                                target_entry_installation: &installation,
                            },
                        )
                    },
                )
                .map_err(|error| {
                    if error.kind() == std::io::ErrorKind::AlreadyExists {
                        ManagedTextFileError::StaleExportTarget
                    } else {
                        ManagedTextFileError::io("export folder", path, error)
                    }
                })?;
                let identity = if let Some(identity) = recovered {
                    identity
                } else {
                    create_export_directory_in_exact_parent_prepared(
                        path,
                        target.parent_identity(),
                        |identity| {
                            let installation = identity.token();
                            record_pull_back_directory(
                                &filesystem,
                                open.storage_root().as_path(),
                                &PullBackDirectoryReceipt {
                                    workspace_installation: &workspace_installation,
                                    target_root_installation: &target_installation,
                                    path: relative_path,
                                    target_entry_installation: &installation,
                                },
                            )
                        },
                    )
                    .map_err(|error| {
                        if error.kind() == std::io::ErrorKind::AlreadyExists {
                            ManagedTextFileError::StaleExportTarget
                        } else {
                            ManagedTextFileError::io("export folder", path, error)
                        }
                    })?
                };
                (true, Some(identity.token()))
            }
        };
        let verified = inspect_managed_directory_export_target(target_root, relative_path)?;
        let installation = verified
            .directory_installation()
            .ok_or(ManagedTextFileError::StaleExportTarget)?;
        if created_identity
            .as_deref()
            .is_some_and(|identity| identity != installation)
            || (!created && Some(installation.as_str()) != expected_target_directory_installation)
        {
            return Err(ManagedTextFileError::StaleExportTarget);
        }
        if created
            && !pull_back_proves_directory(
                &filesystem,
                open.storage_root().as_path(),
                &PullBackDirectoryReceipt {
                    workspace_installation: &workspace_installation,
                    target_root_installation: &target_installation,
                    path: relative_path,
                    target_entry_installation: &installation,
                },
            )
        {
            return Err(ManagedTextFileError::StaleExportTarget);
        }
        Ok(ManagedDirectoryExport::new(
            relative_path.to_owned(),
            target_root.display().to_string(),
            installation,
            created,
        ))
    }

    /// Atomically export one exactly previewed saved file into an ordinary folder.
    ///
    /// This does not mutate Mesh history or the managed working file. Both source and destination
    /// are re-derived after the person confirms; any intervening change refuses before replacement
    /// or create-new installation.
    #[allow(clippy::too_many_arguments)]
    pub fn export_managed_file(
        &self,
        relative_path: &str,
        target_root: &Path,
        expected_target_installation: &str,
        expected_target_parent_installation: &str,
        expected_target_file_installation: Option<&str>,
        expected_source_version: &str,
        expected_source_digest: RecordDigest,
        expected_source_executable: bool,
        expected_target_digest: Option<RecordDigest>,
        expected_target_executable: Option<bool>,
    ) -> Result<ManagedFileExport, ManagedTextFileError> {
        self.export_managed_file_requirement(
            relative_path,
            target_root,
            expected_target_installation,
            expected_target_parent_installation,
            expected_target_file_installation,
            expected_source_version,
            expected_source_digest,
            expected_source_executable,
            expected_target_digest,
            expected_target_executable,
            false,
        )
    }

    /// Export one file only while its source is the exact current shared version.
    #[allow(clippy::too_many_arguments)]
    pub fn export_shared_managed_file(
        &self,
        relative_path: &str,
        target_root: &Path,
        expected_target_installation: &str,
        expected_target_parent_installation: &str,
        expected_target_file_installation: Option<&str>,
        expected_source_version: &str,
        expected_source_digest: RecordDigest,
        expected_source_executable: bool,
        expected_target_digest: Option<RecordDigest>,
        expected_target_executable: Option<bool>,
    ) -> Result<ManagedFileExport, ManagedTextFileError> {
        self.export_managed_file_requirement(
            relative_path,
            target_root,
            expected_target_installation,
            expected_target_parent_installation,
            expected_target_file_installation,
            expected_source_version,
            expected_source_digest,
            expected_source_executable,
            expected_target_digest,
            expected_target_executable,
            true,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn export_managed_file_requirement(
        &self,
        relative_path: &str,
        target_root: &Path,
        expected_target_installation: &str,
        expected_target_parent_installation: &str,
        expected_target_file_installation: Option<&str>,
        expected_source_version: &str,
        expected_source_digest: RecordDigest,
        expected_source_executable: bool,
        expected_target_digest: Option<RecordDigest>,
        expected_target_executable: Option<bool>,
        require_shared: bool,
    ) -> Result<ManagedFileExport, ManagedTextFileError> {
        let _workspace_authority = self.lock_current_managed_workspace_mutation()?;
        let _serial = self
            .managed_edit
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let held = self.held();
        let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
        if require_shared {
            Self::ensure_current_shared_version(open)?;
        }
        let managed_root = open.physical_root().as_path();
        validate_independent_export_root(managed_root, open.storage_root().as_path(), target_root)?;
        let (version, source) = open
            .current_durable_file(relative_path)
            .map_err(|error| ManagedTextFileError::RetainedContent(error.to_string()))?;
        let (working_target, working_bytes) =
            read_managed_replacement(managed_root, relative_path)?;
        let source_digest =
            RecordDigest::from_bytes(*Blake3::digest_bytes(&source.bytes).as_bytes());
        if working_bytes != source.bytes
            || working_target.executable() != source.executable
            || version != expected_source_version
            || source_digest != expected_source_digest
            || source.executable != expected_source_executable
        {
            return Err(ManagedTextFileError::UnsavedWorkingCopy);
        }
        let target_installation = managed_directory_identity(target_root)
            .map_err(|error| ManagedTextFileError::io("inspect export folder", target_root, error))?
            .token();
        if target_installation != expected_target_installation {
            return Err(ManagedTextFileError::StaleExportTarget);
        }
        let target = inspect_managed_export_target(target_root, relative_path)?;
        if target.parent_installation() != expected_target_parent_installation
            || target.file_installation().as_deref() != expected_target_file_installation
        {
            return Err(ManagedTextFileError::StaleExportTarget);
        }
        let filesystem = open.storage_pinned_root().filesystem();
        let workspace_installation = open.installation();
        let (created, installed) = match target {
            ManagedExportTarget::Existing(target, target_bytes) => {
                let target_digest =
                    RecordDigest::from_bytes(*Blake3::digest_bytes(&target_bytes).as_bytes());
                if Some(target_digest) != expected_target_digest
                    || Some(target.executable()) != expected_target_executable
                {
                    return Err(ManagedTextFileError::StaleExportTarget);
                }
                if source.bytes == target_bytes && source.executable == target.executable() {
                    return Err(ManagedTextFileError::Unchanged);
                }
                if !existing_export_target_relation(
                    open,
                    target_root,
                    &target_installation,
                    relative_path,
                    &target.file_installation(),
                    &target_bytes,
                    target.executable(),
                )
                .1
                {
                    return Err(ManagedTextFileError::UnprovenExportReplacement);
                }
                let installed = atomic_replace_with_mode_prepared(
                    &target,
                    &target_bytes,
                    &source.bytes,
                    target.mode_with_executable(source.executable),
                    |identity| {
                        let installation = identity.token();
                        record_pull_back_file(
                            &filesystem,
                            open.storage_root().as_path(),
                            &PullBackFileReceipt {
                                workspace_installation: &workspace_installation,
                                target_root_installation: &target_installation,
                                path: relative_path,
                                source_version: &version,
                                source_digest,
                                source_executable: source.executable,
                                target_entry_installation: &installation,
                            },
                        )
                    },
                )
                .map_err(|error| ManagedTextFileError::io("export file", &target.path, error))?;
                (false, installed)
            }
            ManagedExportTarget::Missing { path, parent } => {
                if expected_target_digest.is_some() || expected_target_executable.is_some() {
                    return Err(ManagedTextFileError::StaleExportTarget);
                }
                let recovered = recover_prepared_export_file_in_exact_parent(
                    &path,
                    &source.bytes,
                    source.executable,
                    parent,
                    |identity| {
                        let installation = identity.token();
                        pull_back_proves_file(
                            &filesystem,
                            open.storage_root().as_path(),
                            &PullBackFileReceipt {
                                workspace_installation: &workspace_installation,
                                target_root_installation: &target_installation,
                                path: relative_path,
                                source_version: &version,
                                source_digest,
                                source_executable: source.executable,
                                target_entry_installation: &installation,
                            },
                        )
                    },
                )
                .map_err(|error| {
                    if error.kind() == std::io::ErrorKind::AlreadyExists {
                        ManagedTextFileError::StaleExportTarget
                    } else {
                        ManagedTextFileError::io("export file", &path, error)
                    }
                })?;
                if !recovered {
                    create_export_file_in_exact_parent_prepared(
                        &path,
                        &source.bytes,
                        source.executable,
                        parent,
                        |identity| {
                            let installation = identity.token();
                            record_pull_back_file(
                                &filesystem,
                                open.storage_root().as_path(),
                                &PullBackFileReceipt {
                                    workspace_installation: &workspace_installation,
                                    target_root_installation: &target_installation,
                                    path: relative_path,
                                    source_version: &version,
                                    source_digest,
                                    source_executable: source.executable,
                                    target_entry_installation: &installation,
                                },
                            )
                        },
                    )
                    .map_err(|error| {
                        if error.kind() == std::io::ErrorKind::AlreadyExists {
                            ManagedTextFileError::StaleExportTarget
                        } else {
                            ManagedTextFileError::io("export file", &path, error)
                        }
                    })?;
                }
                let (installed, installed_bytes) =
                    read_export_replacement(target_root, relative_path)?;
                if installed_bytes != source.bytes || installed.executable() != source.executable {
                    return Err(ManagedTextFileError::StaleExportTarget);
                }
                (true, installed)
            }
        };
        let (verified, bytes) = read_export_replacement(target_root, relative_path)?;
        if !installed.same_file_as(&verified)
            || bytes != source.bytes
            || verified.executable() != source.executable
        {
            return Err(ManagedTextFileError::StaleExportTarget);
        }
        let target_entry_installation = verified.file_installation();
        if !pull_back_proves_file(
            &filesystem,
            open.storage_root().as_path(),
            &PullBackFileReceipt {
                workspace_installation: &workspace_installation,
                target_root_installation: &target_installation,
                path: relative_path,
                source_version: &version,
                source_digest,
                source_executable: source.executable,
                target_entry_installation: &target_entry_installation,
            },
        ) {
            return Err(ManagedTextFileError::StaleExportTarget);
        }
        Ok(ManagedFileExport::new(
            relative_path.to_owned(),
            target_root.display().to_string(),
            &bytes,
            source.executable,
            created,
        ))
    }

    /// Preview one path that durable history says disappeared from the current saved tree.
    ///
    /// Files are removable only when their object came from the sole genesis import and a durable
    /// origin receipt proves this is the exact ordinary directory that supplied that import, or an
    /// exact Pull-back receipt proves the current ordinary entry is one Mesh installed. The
    /// destination must still equal the last durable bytes and portable executable state.
    /// Directories require the same provenance, are identity-bound here, and must still be empty
    /// when confirmed. Missing paths are returned as already absent no-ops.
    pub fn preview_retired_export(
        &self,
        relative_path: &str,
        target_root: &Path,
    ) -> Result<ManagedRetiredExportPreview, ManagedTextFileError> {
        let held = self.held();
        let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
        validate_independent_export_root(
            open.physical_root().as_path(),
            open.storage_root().as_path(),
            target_root,
        )?;
        let retired = open
            .retired_entries()
            .iter()
            .find(|entry| entry.path() == relative_path)
            .ok_or(ManagedTextFileError::StaleExportTarget)?;
        let target_installation = managed_directory_identity(target_root)
            .map_err(|error| ManagedTextFileError::io("inspect export folder", target_root, error))?
            .token();
        match retired.entry_type() {
            "file" => {
                let (version, source) = open
                    .retired_durable_file(relative_path)
                    .map_err(|error| ManagedTextFileError::RetainedContent(error.to_string()))?;
                let source_digest =
                    RecordDigest::from_bytes(*Blake3::digest_bytes(&source.bytes).as_bytes());
                let target = inspect_managed_export_target(target_root, relative_path)?;
                let parent = target.parent_installation();
                match target {
                    ManagedExportTarget::Missing { .. } => Ok(ManagedRetiredExportPreview::new(
                        relative_path.to_owned(),
                        "file",
                        Some(version),
                        Some(source_digest),
                        Some(source.executable),
                        target_root.display().to_string(),
                        target_installation,
                        parent,
                        None,
                        None,
                        None,
                        false,
                        "already-absent",
                    )),
                    ManagedExportTarget::Existing(target, bytes) => {
                        let target_digest =
                            RecordDigest::from_bytes(*Blake3::digest_bytes(&bytes).as_bytes());
                        let executable = target.executable();
                        let content_unchanged =
                            bytes == source.bytes && executable == source.executable;
                        let target_entry_installation = target.file_installation();
                        let filesystem = open.storage_pinned_root().filesystem();
                        let workspace_installation = open.installation();
                        let pulled_back = pull_back_proves_file(
                            &filesystem,
                            open.storage_root().as_path(),
                            &PullBackFileReceipt {
                                workspace_installation: &workspace_installation,
                                target_root_installation: &target_installation,
                                path: relative_path,
                                source_version: &version,
                                source_digest,
                                source_executable: source.executable,
                                target_entry_installation: &target_entry_installation,
                            },
                        );
                        let imported_from_target = retired.imported_with_workspace()
                            && import_origin_proves_target(
                                open,
                                target_root,
                                &target_installation,
                                retired.object(),
                                retired.path(),
                            );
                        let removable = (imported_from_target || pulled_back) && content_unchanged;
                        Ok(ManagedRetiredExportPreview::new(
                            relative_path.to_owned(),
                            "file",
                            Some(version),
                            Some(source_digest),
                            Some(source.executable),
                            target_root.display().to_string(),
                            target_installation,
                            parent,
                            Some(target_entry_installation),
                            Some(target_digest),
                            Some(executable),
                            removable,
                            if removable {
                                if imported_from_target {
                                    "unchanged-old-file"
                                } else {
                                    "unchanged-pulled-back-file"
                                }
                            } else if content_unchanged {
                                "unproven-preserved"
                            } else {
                                "changed-preserved"
                            },
                        ))
                    }
                }
            }
            "folder" => {
                let target = inspect_managed_directory_export_target(target_root, relative_path)?;
                let parent = target.parent_installation();
                let installation = target.directory_installation();
                let empty = target.is_empty();
                let filesystem = open.storage_pinned_root().filesystem();
                let workspace_installation = open.installation();
                let pulled_back = installation.as_deref().is_some_and(|installation| {
                    pull_back_proves_directory(
                        &filesystem,
                        open.storage_root().as_path(),
                        &PullBackDirectoryReceipt {
                            workspace_installation: &workspace_installation,
                            target_root_installation: &target_installation,
                            path: relative_path,
                            target_entry_installation: installation,
                        },
                    )
                });
                let imported_from_target = retired.imported_with_workspace()
                    && import_origin_proves_target(
                        open,
                        target_root,
                        &target_installation,
                        retired.object(),
                        retired.path(),
                    );
                let proven = imported_from_target || pulled_back;
                Ok(ManagedRetiredExportPreview::new(
                    relative_path.to_owned(),
                    "folder",
                    None,
                    None,
                    None,
                    target_root.display().to_string(),
                    target_installation,
                    parent,
                    installation.clone(),
                    None,
                    None,
                    proven && empty == Some(true),
                    match (imported_from_target, pulled_back, empty) {
                        (true, _, Some(true)) => "empty-old-folder",
                        (false, true, Some(true)) => "empty-pulled-back-folder",
                        (false, false, Some(true)) => "unproven-preserved",
                        (_, _, Some(false)) => "nonempty-preserved",
                        (_, _, None) => "already-absent",
                    },
                ))
            }
            _ => unreachable!("retired entries use a closed type vocabulary"),
        }
    }

    /// List durable paths absent from the current saved tree for a separate cleanup review.
    pub fn retired_export_entries(
        &self,
    ) -> Result<Vec<RetiredWorkspaceEntry>, ManagedTextFileError> {
        let held = self.held();
        let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
        Ok(open.retired_entries().to_vec())
    }

    /// Remove one exact reviewed stale path from an ordinary export folder.
    ///
    /// This never changes Mesh history. Files are quarantined, verified against their last durable
    /// bytes, then unlinked. Directories use the same identity-bound quarantine and only an empty
    /// directory can be removed; recursive deletion is deliberately absent.
    #[allow(clippy::too_many_arguments)]
    pub fn remove_retired_export(
        &self,
        relative_path: &str,
        target_root: &Path,
        expected_entry_type: &str,
        expected_source_version: Option<&str>,
        expected_source_digest: Option<RecordDigest>,
        expected_source_executable: Option<bool>,
        expected_target_installation: &str,
        expected_target_parent_installation: &str,
        expected_target_entry_installation: Option<&str>,
        expected_target_digest: Option<RecordDigest>,
        expected_target_executable: Option<bool>,
    ) -> Result<ManagedRetiredExportRemoval, ManagedTextFileError> {
        self.remove_retired_export_requirement(
            relative_path,
            target_root,
            expected_entry_type,
            expected_source_version,
            expected_source_digest,
            expected_source_executable,
            expected_target_installation,
            expected_target_parent_installation,
            expected_target_entry_installation,
            expected_target_digest,
            expected_target_executable,
            false,
        )
    }

    /// Remove one retired original-folder entry only while the exact current version is shared.
    #[allow(clippy::too_many_arguments)]
    pub fn remove_shared_retired_export(
        &self,
        relative_path: &str,
        target_root: &Path,
        expected_entry_type: &str,
        expected_source_version: Option<&str>,
        expected_source_digest: Option<RecordDigest>,
        expected_source_executable: Option<bool>,
        expected_target_installation: &str,
        expected_target_parent_installation: &str,
        expected_target_entry_installation: Option<&str>,
        expected_target_digest: Option<RecordDigest>,
        expected_target_executable: Option<bool>,
    ) -> Result<ManagedRetiredExportRemoval, ManagedTextFileError> {
        self.remove_retired_export_requirement(
            relative_path,
            target_root,
            expected_entry_type,
            expected_source_version,
            expected_source_digest,
            expected_source_executable,
            expected_target_installation,
            expected_target_parent_installation,
            expected_target_entry_installation,
            expected_target_digest,
            expected_target_executable,
            true,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn remove_retired_export_requirement(
        &self,
        relative_path: &str,
        target_root: &Path,
        expected_entry_type: &str,
        expected_source_version: Option<&str>,
        expected_source_digest: Option<RecordDigest>,
        expected_source_executable: Option<bool>,
        expected_target_installation: &str,
        expected_target_parent_installation: &str,
        expected_target_entry_installation: Option<&str>,
        expected_target_digest: Option<RecordDigest>,
        expected_target_executable: Option<bool>,
        require_shared: bool,
    ) -> Result<ManagedRetiredExportRemoval, ManagedTextFileError> {
        let _workspace_authority = self.lock_current_managed_workspace_mutation()?;
        let _serial = self
            .managed_edit
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let held = self.held();
        let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
        if require_shared {
            Self::ensure_current_shared_version(open)?;
        }
        validate_independent_export_root(
            open.physical_root().as_path(),
            open.storage_root().as_path(),
            target_root,
        )?;
        let retired = open
            .retired_entries()
            .iter()
            .find(|entry| entry.path() == relative_path)
            .ok_or(ManagedTextFileError::StaleExportTarget)?;
        if retired.entry_type() != expected_entry_type {
            return Err(ManagedTextFileError::StaleExportTarget);
        }
        let target_installation = managed_directory_identity(target_root)
            .map_err(|error| ManagedTextFileError::io("inspect export folder", target_root, error))?
            .token();
        if target_installation != expected_target_installation {
            return Err(ManagedTextFileError::StaleExportTarget);
        }
        match retired.entry_type() {
            "file" => {
                let (version, source) = open
                    .retired_durable_file(relative_path)
                    .map_err(|error| ManagedTextFileError::RetainedContent(error.to_string()))?;
                let source_digest =
                    RecordDigest::from_bytes(*Blake3::digest_bytes(&source.bytes).as_bytes());
                if Some(version.as_str()) != expected_source_version
                    || Some(source_digest) != expected_source_digest
                    || Some(source.executable) != expected_source_executable
                {
                    return Err(ManagedTextFileError::StaleExportTarget);
                }
                let target = inspect_managed_export_target(target_root, relative_path)?;
                if target.parent_installation() != expected_target_parent_installation {
                    return Err(ManagedTextFileError::StaleExportTarget);
                }
                let ManagedExportTarget::Existing(target, target_bytes) = target else {
                    return Err(ManagedTextFileError::Unchanged);
                };
                let target_digest =
                    RecordDigest::from_bytes(*Blake3::digest_bytes(&target_bytes).as_bytes());
                let target_entry_installation = target.file_installation();
                if Some(target_entry_installation.as_str()) != expected_target_entry_installation
                    || Some(target_digest) != expected_target_digest
                    || Some(target.executable()) != expected_target_executable
                    || target_bytes != source.bytes
                    || target.executable() != source.executable
                {
                    return Err(ManagedTextFileError::StaleExportTarget);
                }
                let filesystem = open.storage_pinned_root().filesystem();
                let workspace_installation = open.installation();
                let imported_from_target = retired.imported_with_workspace()
                    && import_origin_proves_target(
                        open,
                        target_root,
                        &target_installation,
                        retired.object(),
                        retired.path(),
                    );
                if !imported_from_target
                    && !pull_back_proves_file(
                        &filesystem,
                        open.storage_root().as_path(),
                        &PullBackFileReceipt {
                            workspace_installation: &workspace_installation,
                            target_root_installation: &target_installation,
                            path: relative_path,
                            source_version: &version,
                            source_digest,
                            source_executable: source.executable,
                            target_entry_installation: &target_entry_installation,
                        },
                    )
                {
                    return Err(ManagedTextFileError::StaleExportTarget);
                }
                remove_exact_export_file(&target, &source.bytes).map_err(|error| {
                    ManagedTextFileError::io("remove unchanged old export", &target.path, error)
                })?;
            }
            "folder" => {
                if expected_source_version.is_some()
                    || expected_source_digest.is_some()
                    || expected_source_executable.is_some()
                    || expected_target_digest.is_some()
                    || expected_target_executable.is_some()
                {
                    return Err(ManagedTextFileError::StaleExportTarget);
                }
                let target = inspect_managed_directory_export_target(target_root, relative_path)?;
                let target_entry_installation = target.directory_installation();
                if target.parent_installation() != expected_target_parent_installation
                    || target_entry_installation.as_deref() != expected_target_entry_installation
                {
                    return Err(ManagedTextFileError::StaleExportTarget);
                }
                let filesystem = open.storage_pinned_root().filesystem();
                let workspace_installation = open.installation();
                let imported_from_target = retired.imported_with_workspace()
                    && import_origin_proves_target(
                        open,
                        target_root,
                        &target_installation,
                        retired.object(),
                        retired.path(),
                    );
                if !imported_from_target
                    && !target_entry_installation
                        .as_deref()
                        .is_some_and(|installation| {
                            pull_back_proves_directory(
                                &filesystem,
                                open.storage_root().as_path(),
                                &PullBackDirectoryReceipt {
                                    workspace_installation: &workspace_installation,
                                    target_root_installation: &target_installation,
                                    path: relative_path,
                                    target_entry_installation: installation,
                                },
                            )
                        })
                {
                    return Err(ManagedTextFileError::StaleExportTarget);
                }
                let ManagedDirectoryExportTarget::Existing {
                    parent, directory, ..
                } = target
                else {
                    return Err(ManagedTextFileError::Unchanged);
                };
                remove_exact_export_directory(target_root, relative_path, parent, directory)
                    .map_err(|error| {
                        ManagedTextFileError::io(
                            "remove empty old export folder",
                            target_root.join(relative_path),
                            error,
                        )
                    })?;
            }
            _ => unreachable!("retired entries use a closed type vocabulary"),
        }
        Ok(ManagedRetiredExportRemoval::new(
            relative_path.to_owned(),
            retired.entry_type(),
            target_root.display().to_string(),
        ))
    }

    /// Replace one managed UTF-8 file and durably preserve the exact edit for recovery.
    ///
    /// This native-only phase does not fabricate a ChangeSet author or a signature. It updates the
    /// managed operating-system file atomically, records the real replacement boundary, and stores
    /// a verified recovery envelope in the existing checkpoint runtime. This operation remains
    /// `Working`; [`Self::save_managed_file_privately`] performs the distinct authenticated append
    /// and meaningful-save transition.
    pub fn preserve_managed_text_edit(
        &self,
        relative_path: &str,
        text: &str,
        expected_content_digest: RecordDigest,
        expected_executable: bool,
    ) -> Result<ManagedTextSave, ManagedTextFileError> {
        if text.len() > MAX_MANAGED_TEXT_BYTES {
            return Err(ManagedTextFileError::TooLarge { bytes: text.len() });
        }
        let _workspace_authority = self.lock_current_managed_workspace_mutation()?;
        let _serial = self
            .managed_edit
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let (root, target, prior) = {
            let held = self.held();
            let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
            if open.managed_mutation_recovery_needed() {
                return Err(ManagedTextFileError::Recovery(
                    "an interrupted local file change needs attention".to_owned(),
                ));
            }
            open.ensure_physical_root()
                .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
            let root = open.physical_root().as_path().to_path_buf();
            let (target, prior) = read_managed_replacement(&root, relative_path)?;
            if !open
                .file_histories()
                .iter()
                .any(|history| history.path() == relative_path && history.current().is_some())
            {
                return Err(ManagedTextFileError::NotRegularFile);
            }
            (root, target, prior)
        };
        if RecordDigest::from_bytes(*Blake3::digest_bytes(&prior).as_bytes())
            != expected_content_digest
            || target.executable() != expected_executable
        {
            return Err(ManagedTextFileError::StaleInspection);
        }
        if prior.len() > MAX_MANAGED_TEXT_BYTES {
            return Err(ManagedTextFileError::TooLarge { bytes: prior.len() });
        }
        if prior == text.as_bytes() {
            return Err(ManagedTextFileError::Unchanged);
        }

        let mut envelope = Vec::with_capacity(relative_path.len() + text.len() + 64);
        envelope.extend_from_slice(b"mesh.local-managed-text-recovery/1\0");
        envelope.extend_from_slice(&(relative_path.len() as u64).to_be_bytes());
        envelope.extend_from_slice(relative_path.as_bytes());
        envelope.extend_from_slice(&(text.len() as u64).to_be_bytes());
        envelope.extend_from_slice(text.as_bytes());
        let (recovery, stable_after_idle) = self.preserve_managed_bytes(
            &root,
            &target,
            relative_path,
            ManagedReplacement {
                prior: &prior,
                bytes: text.as_bytes(),
                executable: expected_executable,
            },
            envelope,
        )?;
        let content_digest =
            RecordDigest::from_bytes(*Blake3::digest_bytes(text.as_bytes()).as_bytes());
        Ok(ManagedTextSave::new(
            relative_path.to_owned(),
            recovery,
            content_digest,
            stable_after_idle,
        ))
    }

    /// Commit the current managed file bytes as an authenticated local private version.
    ///
    /// The desktop adapter supplies its actor public key and custody-backed signer. This method
    /// derives every other field from durable workspace truth, signs the exact canonical
    /// ChangeSet body, verifies and persists the authenticated envelope, reloads the workspace,
    /// and advances `Saved privately` only after the measured idle interval and the real durable
    /// acknowledgement. It exposes no IPC mutation method and grants no publication authority.
    pub fn save_managed_file_privately<F, E>(
        &self,
        relative_path: &str,
        expected_content_digest: RecordDigest,
        expected_executable: bool,
        actor_public_key: PublicKey,
        sign: F,
    ) -> Result<ManagedPrivateSave, ManagedTextFileError>
    where
        F: FnOnce(&SigningPayload) -> Result<Signature, E>,
        E: std::fmt::Display,
    {
        let _workspace_authority = self.lock_current_managed_workspace_mutation()?;
        let _serial = self
            .managed_edit
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let (root, bytes, basis, current_manifest, current_metadata, executable) = {
            let held = self.held();
            let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
            let history = open
                .file_histories()
                .iter()
                .find(|history| history.path() == relative_path)
                .ok_or(ManagedTextFileError::NotRegularFile)?;
            let current = history
                .current()
                .ok_or(ManagedTextFileError::NotRegularFile)?;
            let manifest = open
                .manifest_record(current.manifest())
                .ok_or(ManagedTextFileError::NotRegularFile)?;
            let current_metadata = open
                .file_version_metadata(current.version())
                .ok_or(ManagedTextFileError::NotRegularFile)?;
            open.ensure_physical_root()
                .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
            let root = open.physical_root().as_path().to_path_buf();
            let (path, bytes) = read_managed_bytes(&root, relative_path)?;
            let metadata = std::fs::metadata(&path)
                .map_err(|error| ManagedTextFileError::io("metadata", &path, error))?;
            (
                root,
                bytes,
                open.managed_checkpoint_basis(relative_path, actor_public_key)
                    .map_err(ManagedTextFileError::Authoring)?,
                manifest.clone(),
                current_metadata,
                metadata.permissions().mode() & 0o111 != 0,
            )
        };
        if RecordDigest::from_bytes(*Blake3::digest_bytes(&bytes).as_bytes())
            != expected_content_digest
            || executable != expected_executable
        {
            return Err(ManagedTextFileError::StaleInspection);
        }
        if RecordDigest::from_bytes(*Blake3::digest_bytes(&bytes).as_bytes())
            == current_manifest.content_digest
            && PortableMetadata::new(executable) == current_metadata
        {
            return Err(ManagedTextFileError::Unchanged);
        }

        let config = ChunkingConfig::default();
        let paging = crate::ManifestPagingPolicy::flat();
        let prepared = PreparedCheckpointFile::from_bytes(&bytes, &config, paging)
            .map_err(|error| ManagedTextFileError::Authoring(error.to_string()))?;
        let manifest_id = prepared.manifest().id;
        let mut version_statement = Vec::with_capacity(160);
        version_statement.extend_from_slice(b"mesh.local-file-version/1\0");
        version_statement.extend_from_slice(basis.workspace_id.as_bytes());
        version_statement.extend_from_slice(basis.object_id.as_bytes());
        version_statement.extend_from_slice(basis.parent_version.as_bytes());
        version_statement.extend_from_slice(manifest_id.as_bytes());
        version_statement.push(u8::from(executable));
        let version_id =
            VersionId::from_bytes(*Blake3::digest_bytes(&version_statement).as_bytes());

        let snapshot = self
            .checkpoint_snapshot()
            .map_err(|error| ManagedTextFileError::Checkpoint(error.to_string()))?;
        let last_sequence = [
            snapshot.open_window().map(|window| window.last().get()),
            snapshot
                .latest_recovery()
                .map(|recovery| recovery.through().get()),
            snapshot
                .last_meaningful()
                .map(|checkpoint| checkpoint.through().get()),
        ]
        .into_iter()
        .flatten()
        .max()
        .unwrap_or(0);
        let sequence = last_sequence
            .checked_add(1)
            .and_then(RecoverySequence::new)
            .ok_or_else(|| {
                ManagedTextFileError::Checkpoint("recovery sequence exhausted".to_owned())
            })?;

        let unsigned = FileVersionCheckpointRequest::new(
            basis.workspace_id,
            basis.actor_id,
            basis.session_id,
            basis.actor_sequence,
            basis.causal_parents,
            basis.base_head,
            basis.policy_epoch,
            basis.hybrid_logical_time,
            basis.object_id,
            version_id,
            vec![basis.parent_version],
            PortableMetadata::new(executable),
            OperationSignature::from_bytes([0; 64]),
        );
        let signing_body =
            file_version_signing_body(&bytes, &config, paging, &unsigned, &LocalChangesetHead)
                .map_err(|error| ManagedTextFileError::Authoring(error.to_string()))?;
        let payload = SigningPayload::new(
            crate::authenticated_changeset::CHANGESET_SIGNATURE_DOMAIN,
            &signing_body,
        );
        let signature =
            sign(&payload).map_err(|error| ManagedTextFileError::Authoring(error.to_string()))?;
        let request = unsigned.authenticated(actor_public_key, signature);
        let (saved, installation) = self
            .save_file_version_with_installation(
                sequence,
                &bytes,
                &config,
                paging,
                request,
                &LocalChangesetHead,
            )
            .map_err(|error| ManagedTextFileError::Authoring(error.to_string()))?;
        let (stable_after_idle, meaningful_saved) =
            self.settle_managed_change(&root, installation, sequence, || {
                read_managed_replacement(&root, relative_path).is_ok_and(|(target, current)| {
                    current == bytes && target.executable() == executable
                })
            })?;
        Ok(ManagedPrivateSave::new(
            relative_path.to_owned(),
            version_id.to_string(),
            saved.manifest_id().to_string(),
            saved.changeset_id().to_string(),
            stable_after_idle,
            meaningful_saved,
            true,
        ))
    }

    /// Save one exact agent-owned file without releasing custody or granting publication power.
    ///
    /// This closed operation only records private file content. The native signer retains secret
    /// custody outside the daemon and receives no inherited mutation context. Reuse existing
    /// confinement, authenticated journal, content recheck and settling guarantees. A returned
    /// per-file receipt is not proof that the entire working folder has been checkpointed.
    pub fn checkpoint_agent_file<F, E>(
        &self,
        request: AgentFileCheckpointRequest<'_>,
        actor_public_key: PublicKey,
        sign: F,
    ) -> Result<ManagedPrivateSave, ManagedTextFileError>
    where
        F: FnOnce(&SigningPayload) -> Result<Signature, E>,
        E: std::fmt::Display,
    {
        if VERIFIED_MUTATION_CONTEXT.with(|active| active.borrow().is_some()) {
            return Err(ManagedTextFileError::Recovery(
                "nested agent checkpoint was refused".to_owned(),
            ));
        }
        let _authority = self.lock_workspace_agent_setup(
            request.root,
            request.digest,
            request.installation,
            request.generation,
        )?;
        let _context = VerifiedMutationContext::enter(self, request.installation)?;
        let sign = |payload: &SigningPayload| {
            let _suspended = SuspendedMutationContext::enter();
            sign(payload)
        };
        if request.new_file {
            self.adopt_native_file_privately(
                request.path,
                request.content_digest,
                request.executable,
                actor_public_key,
                sign,
            )
        } else {
            self.save_managed_file_privately(
                request.path,
                request.content_digest,
                request.executable,
                actor_public_key,
                sign,
            )
        }
    }

    /// Capture supported edits and additions under one exact native assignment.
    ///
    /// Native files are never rewritten. Missing/unsupported entries require explicit resolution;
    /// a rename is never guessed from a disappearance plus an addition. Each successful append is
    /// durable even if a later save fails. Only a clean final inventory yields complete=true.
    pub fn checkpoint_agent_workspace<F, E>(
        &self,
        request: AgentWorkspaceCheckpointRequest<'_>,
        actor_public_key: PublicKey,
        mut sign: F,
    ) -> Result<AgentWorkspaceCheckpoint, ManagedTextFileError>
    where
        F: FnMut(&SigningPayload) -> Result<Signature, E>,
        E: std::fmt::Display,
    {
        if VERIFIED_MUTATION_CONTEXT.with(|active| active.borrow().is_some()) {
            return Err(ManagedTextFileError::Recovery(
                "nested agent checkpoint was refused".to_owned(),
            ));
        }
        let _authority = self.lock_workspace_agent_setup(
            request.root,
            request.digest,
            request.installation,
            request.generation,
        )?;
        let _context = VerifiedMutationContext::enter(self, request.installation)?;
        let before = self.inspect_agent_folder_locked(
            request.root,
            request.digest,
            request.installation,
            request.generation,
        )?;
        let mut saved_changes = Vec::new();
        let mut capture = || -> Result<(), &'static str> {
            if !before.unsupported_entries.is_empty() || !before.missing_files.is_empty() {
                return Err("checkpoint-entry-resolution-required");
            }
            let changed = before
                .managed_files
                .iter()
                .filter(|file| file.modified_from_current_version());
            let count = changed.clone().count()
                + before.native_files.len()
                + before.native_directories.len();
            if count > 1024 {
                return Err("checkpoint-change-limit");
            }
            let mut directories = before.native_directories.iter().collect::<Vec<_>>();
            directories.sort_by_key(|entry| (entry.path().split('/').count(), entry.path()));
            for directory in directories {
                let receipt = self
                    .adopt_native_directory_privately(
                        directory.path(),
                        directory.installation(),
                        actor_public_key,
                        |payload| {
                            let _suspended = SuspendedMutationContext::enter();
                            sign(payload)
                        },
                    )
                    .map_err(|_| "checkpoint-directory-save-failed")?;
                saved_changes.push(receipt.changeset().to_owned());
                if !receipt.meaningful_saved() {
                    return Err("checkpoint-content-not-settled");
                }
            }
            for file in changed {
                let receipt = self
                    .save_managed_file_privately(
                        file.path(),
                        file.content_digest(),
                        file.executable(),
                        actor_public_key,
                        |payload| {
                            let _suspended = SuspendedMutationContext::enter();
                            sign(payload)
                        },
                    )
                    .map_err(|_| "checkpoint-file-save-failed")?;
                saved_changes.push(receipt.changeset().to_owned());
                if !receipt.meaningful_saved() {
                    return Err("checkpoint-content-not-settled");
                }
            }
            for file in &before.native_files {
                let receipt = self
                    .adopt_native_file_privately(
                        file.path(),
                        file.content_digest(),
                        file.executable(),
                        actor_public_key,
                        |payload| {
                            let _suspended = SuspendedMutationContext::enter();
                            sign(payload)
                        },
                    )
                    .map_err(|_| "checkpoint-file-save-failed")?;
                saved_changes.push(receipt.changeset().to_owned());
                if !receipt.meaningful_saved() {
                    return Err("checkpoint-content-not-settled");
                }
            }
            let digest = self
                .held()
                .as_ref()
                .ok_or("checkpoint-workspace-unavailable")?
                .digest()
                .to_string();
            let after = self
                .inspect_agent_folder_locked(
                    request.root,
                    &digest,
                    request.installation,
                    request.generation,
                )
                .map_err(|_| "checkpoint-final-inspection-failed")?;
            if !after.unsupported_entries.is_empty()
                || !after.missing_files.is_empty()
                || !after.native_files.is_empty()
                || !after.native_directories.is_empty()
                || after
                    .managed_files
                    .iter()
                    .any(|file| file.modified_from_current_version())
            {
                return Err("checkpoint-working-folder-changed");
            }
            if self
                .checkpoint_snapshot()
                .map_err(|_| "checkpoint-recovery-unavailable")?
                .pending_meaningful()
                .is_some()
            {
                return Err("checkpoint-recovery-pending");
            }
            Ok(())
        };
        let issue = capture().err();
        let held = self.held();
        let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
        Ok(AgentWorkspaceCheckpoint {
            workspace: summarise(open, None),
            complete: issue.is_none(),
            saved_changes,
            issue,
        })
    }

    /// Adopt an existing native regular file as a new authenticated private file.
    ///
    /// The operating-system file is never rewritten. The caller must first inspect its exact
    /// digest and executable metadata. This method rechecks the same physical file after signing,
    /// appends CreateFile + WriteFileVersion + LinkDirectoryEntry durably, and reports meaningful
    /// completion only if that exact native identity and bytes remain unchanged through settling.
    pub fn adopt_native_file_privately<F, E>(
        &self,
        relative_path: &str,
        expected_content_digest: RecordDigest,
        expected_executable: bool,
        actor_public_key: PublicKey,
        sign: F,
    ) -> Result<ManagedPrivateSave, ManagedTextFileError>
    where
        F: FnOnce(&SigningPayload) -> Result<Signature, E>,
        E: std::fmt::Display,
    {
        let _workspace_authority = self.lock_current_managed_workspace_mutation()?;
        let _serial = self
            .managed_edit
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let (root, bytes, basis, binding, target, executable) = {
            let held = self.held();
            let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
            if open.managed_mutation_recovery_needed() {
                return Err(ManagedTextFileError::Recovery(
                    "an interrupted local file change needs attention".to_owned(),
                ));
            }
            open.ensure_physical_root()
                .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
            let root = open.physical_root().as_path().to_path_buf();
            let binding = open
                .managed_create_target(relative_path)
                .map_err(|_| ManagedTextFileError::TargetExists)?;
            let (target, bytes) = read_managed_replacement(&root, relative_path)?;
            (
                root,
                bytes,
                open.managed_authoring_basis(actor_public_key)
                    .map_err(ManagedTextFileError::Authoring)?,
                binding,
                target.clone(),
                target.executable(),
            )
        };
        if RecordDigest::from_bytes(*Blake3::digest_bytes(&bytes).as_bytes())
            != expected_content_digest
            || executable != expected_executable
        {
            return Err(ManagedTextFileError::StaleInspection);
        }

        let object_id = local_object_id(
            b"mesh.local-adopted-file/1\0",
            basis.workspace_id.as_bytes(),
            actor_public_key.as_bytes(),
            basis.actor_sequence.value(),
            relative_path,
        );
        let config = ChunkingConfig::default();
        let paging = crate::ManifestPagingPolicy::flat();
        let prepared = PreparedCheckpointFile::from_bytes(&bytes, &config, paging)
            .map_err(|error| ManagedTextFileError::Authoring(error.to_string()))?;
        let version_id = local_version_id(
            basis.workspace_id.as_bytes(),
            object_id.as_bytes(),
            prepared.manifest().id.as_bytes(),
            relative_path,
        );
        let before = vec![Operation::CreateFile { object_id }];
        let after = vec![Operation::LinkDirectoryEntry {
            directory_id: binding.parent_id,
            name: binding.name,
            object_id,
            version_id,
        }];
        let metadata = PortableMetadata::new(executable);
        {
            let held = self.held();
            held.as_ref()
                .ok_or(ManagedTextFileError::NoWorkspace)?
                .validate_managed_operations(&[
                    before[0].clone(),
                    Operation::WriteFileVersion {
                        object_id,
                        version_id,
                        parent_versions: Vec::new(),
                        manifest_id: mesh_operations::ManifestId::from_bytes(
                            *prepared.manifest().id.as_bytes(),
                        ),
                        portable_metadata: metadata,
                    },
                    after[0].clone(),
                ])
                .map_err(ManagedTextFileError::Authoring)?;
        }
        let unsigned = FileVersionCheckpointRequest::new(
            basis.workspace_id,
            basis.actor_id,
            basis.session_id,
            basis.actor_sequence,
            basis.causal_parents,
            basis.base_head,
            basis.policy_epoch,
            basis.hybrid_logical_time,
            object_id,
            version_id,
            Vec::new(),
            metadata,
            OperationSignature::from_bytes([0; 64]),
        )
        .around_write(before, after);
        let signing_body =
            file_version_signing_body(&bytes, &config, paging, &unsigned, &LocalChangesetHead)
                .map_err(|error| ManagedTextFileError::Authoring(error.to_string()))?;
        let signature = sign(&SigningPayload::new(
            crate::authenticated_changeset::CHANGESET_SIGNATURE_DOMAIN,
            &signing_body,
        ))
        .map_err(|error| ManagedTextFileError::Authoring(error.to_string()))?;
        let intended = authenticated_changeset_id(&signing_body, actor_public_key, signature)?;
        let request = unsigned.authenticated(actor_public_key, signature);
        {
            // Ignore files are ordinary native files and may change while the user or agent is
            // reviewing the signing request. Re-read the effective rules after signing and before
            // obtaining an author sequence or appending anything, so an exclusion observable at
            // the authority boundary always wins.
            let held = self.held();
            held.as_ref()
                .ok_or(ManagedTextFileError::NoWorkspace)?
                .managed_create_target(relative_path)
                .map_err(ManagedTextFileError::Authoring)?;
        }
        let sequence = self.next_managed_sequence()?;

        let (rechecked_target, rechecked_bytes) = read_managed_replacement(&root, relative_path)?;
        if !target.same_file_as(&rechecked_target)
            || rechecked_bytes != bytes
            || rechecked_target.executable() != executable
        {
            return Err(ManagedTextFileError::StaleInspection);
        }

        let saved = match self.save_file_version_with_installation(
            sequence,
            &bytes,
            &config,
            paging,
            request,
            &LocalChangesetHead,
        ) {
            Ok(saved) => Some(saved),
            Err(_error) if self.durable_operation(intended) == Some(true) => None,
            Err(error) => return Err(ManagedTextFileError::Authoring(error.to_string())),
        };
        let (stable, meaningful) = match saved {
            Some((_saved, installation)) => {
                self.settle_managed_change(&root, installation, sequence, || {
                    read_managed_replacement(&root, relative_path).is_ok_and(
                        |(current_target, current_bytes)| {
                            target.same_file_as(&current_target)
                                && current_bytes == bytes
                                && current_target.executable() == executable
                        },
                    )
                })?
            }
            None => (false, false),
        };
        Ok(ManagedPrivateSave::new(
            relative_path.to_owned(),
            version_id.to_string(),
            prepared.manifest().id.to_string(),
            intended.to_string(),
            stable,
            meaningful,
            true,
        ))
    }

    /// Create one UTF-8 file as an authenticated durable private version and materialize it in the
    /// selected operating-system folder.
    pub fn create_managed_text_file<F, E>(
        &self,
        relative_path: &str,
        text: &str,
        actor_public_key: PublicKey,
        sign: F,
    ) -> Result<ManagedPrivateSave, ManagedTextFileError>
    where
        F: FnOnce(&SigningPayload) -> Result<Signature, E>,
        E: std::fmt::Display,
    {
        if text.len() > MAX_MANAGED_TEXT_BYTES {
            return Err(ManagedTextFileError::TooLarge { bytes: text.len() });
        }
        let _workspace_authority = self.lock_current_managed_workspace_mutation()?;
        let _serial = self
            .managed_edit
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let (root, basis, binding, target_parent, filesystem) = {
            let held = self.held();
            let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
            let root = open.physical_root().as_path().to_path_buf();
            let target = confined_free_path(&root, relative_path)?;
            let target_parent_path = target.parent().ok_or(ManagedTextFileError::InvalidPath)?;
            let target_parent =
                managed_directory_identity(target_parent_path).map_err(|error| {
                    ManagedTextFileError::io("capture parent identity", target_parent_path, error)
                })?;
            (
                root,
                open.managed_authoring_basis(actor_public_key)
                    .map_err(ManagedTextFileError::Authoring)?,
                open.managed_create_target(relative_path)
                    .map_err(ManagedTextFileError::Authoring)?,
                target_parent,
                open.pinned_root().filesystem(),
            )
        };
        let object_id = local_object_id(
            b"mesh.local-managed-file/1\0",
            basis.workspace_id.as_bytes(),
            actor_public_key.as_bytes(),
            basis.actor_sequence.value(),
            relative_path,
        );
        let config = ChunkingConfig::default();
        let paging = crate::ManifestPagingPolicy::flat();
        let prepared = PreparedCheckpointFile::from_bytes(text.as_bytes(), &config, paging)
            .map_err(|error| ManagedTextFileError::Authoring(error.to_string()))?;
        let version_id = local_version_id(
            basis.workspace_id.as_bytes(),
            object_id.as_bytes(),
            prepared.manifest().id.as_bytes(),
            relative_path,
        );
        let before = vec![Operation::CreateFile { object_id }];
        let after = vec![Operation::LinkDirectoryEntry {
            directory_id: binding.parent_id,
            name: binding.name,
            object_id,
            version_id,
        }];
        {
            let held = self.held();
            held.as_ref()
                .ok_or(ManagedTextFileError::NoWorkspace)?
                .validate_managed_operations(&[
                    before[0].clone(),
                    Operation::WriteFileVersion {
                        object_id,
                        version_id,
                        parent_versions: Vec::new(),
                        manifest_id: mesh_operations::ManifestId::from_bytes(
                            *prepared.manifest().id.as_bytes(),
                        ),
                        portable_metadata: PortableMetadata::default(),
                    },
                    after[0].clone(),
                ])
                .map_err(ManagedTextFileError::Authoring)?;
        }
        let unsigned = FileVersionCheckpointRequest::new(
            basis.workspace_id,
            basis.actor_id,
            basis.session_id,
            basis.actor_sequence,
            basis.causal_parents,
            basis.base_head,
            basis.policy_epoch,
            basis.hybrid_logical_time,
            object_id,
            version_id,
            Vec::new(),
            PortableMetadata::default(),
            OperationSignature::from_bytes([0; 64]),
        )
        .around_write(before, after);
        let signing_body = file_version_signing_body(
            text.as_bytes(),
            &config,
            paging,
            &unsigned,
            &LocalChangesetHead,
        )
        .map_err(|error| ManagedTextFileError::Authoring(error.to_string()))?;
        let signature = sign(&SigningPayload::new(
            crate::authenticated_changeset::CHANGESET_SIGNATURE_DOMAIN,
            &signing_body,
        ))
        .map_err(|error| ManagedTextFileError::Authoring(error.to_string()))?;
        let intended = authenticated_changeset_id(&signing_body, actor_public_key, signature)?;
        let request = unsigned.authenticated(actor_public_key, signature);
        {
            let held = self.held();
            held.as_ref()
                .ok_or(ManagedTextFileError::NoWorkspace)?
                .managed_create_target(relative_path)
                .map_err(ManagedTextFileError::Authoring)?;
        }
        let sequence = self.next_managed_sequence()?;

        let mut intent = ManagedMutationIntent::begin_create_file_guarded(
            &root,
            relative_path,
            intended,
            text.as_bytes(),
            target_parent,
            Some(filesystem),
        )
        .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
        let target = match intent.apply_create_file(text.as_bytes()) {
            Ok(target) => target,
            Err(error) => {
                let _ = intent.clear_after_failed_apply(&error);
                return Err(ManagedTextFileError::Recovery(error.to_string()));
            }
        };
        intent
            .bind_created(&target)
            .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
        let saved = match self.save_file_version_with_installation(
            sequence,
            text.as_bytes(),
            &config,
            paging,
            request,
            &LocalChangesetHead,
        ) {
            Ok(saved) => Some(saved),
            Err(_error) if self.durable_operation(intended) == Some(true) => None,
            Err(error) => {
                if self.durable_operation(intended) == Some(false) {
                    intent.roll_back_nondurable().map_err(|rollback| {
                        ManagedTextFileError::Rollback {
                            checkpoint: error.to_string(),
                            rollback: std::io::Error::other(rollback.to_string()),
                        }
                    })?;
                }
                return Err(ManagedTextFileError::Authoring(error.to_string()));
            }
        };
        self.finish_managed_intent(&intent);
        let (stable, meaningful) = match saved {
            Some((_saved, installation)) => {
                self.settle_managed_change(&root, installation, sequence, || {
                    intent.created_file_matches().is_ok_and(|matches| matches)
                })?
            }
            None => (false, false),
        };
        Ok(ManagedPrivateSave::new(
            relative_path.to_owned(),
            version_id.to_string(),
            prepared.manifest().id.to_string(),
            intended.to_string(),
            stable,
            meaningful,
            true,
        ))
    }

    /// Adopt one existing native directory as an authenticated private directory.
    ///
    /// The filesystem is not changed a second time. Discovery supplies an opaque directory
    /// identity; this method reopens the confined directory before and after signing, then keeps
    /// the exact same identity through checkpoint settling. Reviewed descendants remain unsaved;
    /// callers must admit every directory parent first and re-inspect each file before saving it.
    pub fn adopt_native_directory_privately<F, E>(
        &self,
        relative_path: &str,
        expected_installation: &str,
        actor_public_key: PublicKey,
        sign: F,
    ) -> Result<ManagedEntryChange, ManagedTextFileError>
    where
        F: FnOnce(&SigningPayload) -> Result<Signature, E>,
        E: std::fmt::Display,
    {
        let _workspace_authority = self.lock_current_managed_workspace_mutation()?;
        let _serial = self
            .managed_edit
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let (root, basis, binding, identity) = {
            let held = self.held();
            let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
            if open.managed_mutation_recovery_needed() {
                return Err(ManagedTextFileError::Recovery(
                    "an interrupted local file change needs attention".to_owned(),
                ));
            }
            open.ensure_physical_root()
                .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
            let root = open.physical_root().as_path().to_path_buf();
            let (_path, identity) = inspect_managed_directory(&root, relative_path)?;
            if identity.token() != expected_installation {
                return Err(ManagedTextFileError::StaleInspection);
            }
            (
                root,
                open.managed_authoring_basis(actor_public_key)
                    .map_err(ManagedTextFileError::Authoring)?,
                open.managed_create_target(relative_path)
                    .map_err(ManagedTextFileError::Authoring)?,
                identity,
            )
        };
        let object_id = local_object_id(
            b"mesh.local-adopted-directory/1\0",
            basis.workspace_id.as_bytes(),
            actor_public_key.as_bytes(),
            basis.actor_sequence.value(),
            relative_path,
        );
        let operations = vec![
            Operation::CreateDirectory { object_id },
            Operation::LinkDirectoryEntry {
                directory_id: binding.parent_id,
                name: binding.name,
                object_id,
                version_id: VersionId::from_bytes([0; 32]),
            },
        ];
        let (request, intended) =
            self.signed_operation_request(basis, operations, actor_public_key, sign)?;
        {
            let held = self.held();
            held.as_ref()
                .ok_or(ManagedTextFileError::NoWorkspace)?
                .managed_create_target(relative_path)
                .map_err(ManagedTextFileError::Authoring)?;
        }
        let (_path, after_signing) = inspect_managed_directory(&root, relative_path)?;
        if after_signing != identity {
            return Err(ManagedTextFileError::StaleInspection);
        }
        let sequence = self.next_managed_sequence()?;
        let saved = self.persist_managed_operation(sequence, request, intended, || Ok(()))?;
        let meaningful = match saved {
            Some((_saved, installation)) => {
                self.settle_managed_change(&root, installation, sequence, || {
                    inspect_managed_directory(&root, relative_path)
                        .is_ok_and(|(_path, current)| current == identity)
                })?
                .1
            }
            None => false,
        };
        Ok(ManagedEntryChange::new(
            "adopt_folder",
            None,
            Some(relative_path.to_owned()),
            intended.to_string(),
            meaningful,
        ))
    }

    /// Create an empty folder as one authenticated private ChangeSet.
    pub fn create_managed_folder<F, E>(
        &self,
        relative_path: &str,
        actor_public_key: PublicKey,
        sign: F,
    ) -> Result<ManagedEntryChange, ManagedTextFileError>
    where
        F: FnOnce(&SigningPayload) -> Result<Signature, E>,
        E: std::fmt::Display,
    {
        let _workspace_authority = self.lock_current_managed_workspace_mutation()?;
        let _serial = self
            .managed_edit
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let (root, basis, binding, target_parent, filesystem) = {
            let held = self.held();
            let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
            let root = open.physical_root().as_path().to_path_buf();
            let target = confined_free_path(&root, relative_path)?;
            let target_parent_path = target.parent().ok_or(ManagedTextFileError::InvalidPath)?;
            let target_parent =
                managed_directory_identity(target_parent_path).map_err(|error| {
                    ManagedTextFileError::io("capture parent identity", target_parent_path, error)
                })?;
            (
                root,
                open.managed_authoring_basis(actor_public_key)
                    .map_err(ManagedTextFileError::Authoring)?,
                open.managed_create_target(relative_path)
                    .map_err(ManagedTextFileError::Authoring)?,
                target_parent,
                open.pinned_root().filesystem(),
            )
        };
        let object_id = local_object_id(
            b"mesh.local-managed-folder/1\0",
            basis.workspace_id.as_bytes(),
            actor_public_key.as_bytes(),
            basis.actor_sequence.value(),
            relative_path,
        );
        let operations = vec![
            Operation::CreateDirectory { object_id },
            Operation::LinkDirectoryEntry {
                directory_id: binding.parent_id,
                name: binding.name,
                object_id,
                version_id: VersionId::from_bytes([0; 32]),
            },
        ];
        let (request, intended) =
            self.signed_operation_request(basis, operations, actor_public_key, sign)?;
        {
            let held = self.held();
            held.as_ref()
                .ok_or(ManagedTextFileError::NoWorkspace)?
                .managed_create_target(relative_path)
                .map_err(ManagedTextFileError::Authoring)?;
        }
        let sequence = self.next_managed_sequence()?;
        let mut intent = ManagedMutationIntent::begin_create_directory_guarded(
            &root,
            relative_path,
            intended,
            target_parent,
            Some(filesystem),
        )
        .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
        let target = match intent.apply_create_directory() {
            Ok(target) => target,
            Err(error) => {
                let _ = intent.clear_after_failed_apply(&error);
                return Err(ManagedTextFileError::Recovery(error.to_string()));
            }
        };
        intent
            .bind_created(&target)
            .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
        let saved = self.persist_managed_operation(sequence, request, intended, || {
            intent
                .roll_back_nondurable()
                .map_err(|error| std::io::Error::other(error.to_string()))
        })?;
        self.finish_managed_intent(&intent);
        let meaningful = match saved {
            Some((_saved, installation)) => {
                self.settle_managed_change(&root, installation, sequence, || {
                    intent
                        .created_directory_matches()
                        .is_ok_and(|matches| matches)
                })?
                .1
            }
            None => false,
        };
        Ok(ManagedEntryChange::new(
            "create_folder",
            None,
            Some(relative_path.to_owned()),
            intended.to_string(),
            meaningful,
        ))
    }

    /// Rename or move one existing durable entry without changing its stable object identity.
    pub fn move_managed_entry_privately<F, E>(
        &self,
        from_path: &str,
        to_path: &str,
        actor_public_key: PublicKey,
        sign: F,
    ) -> Result<ManagedEntryChange, ManagedTextFileError>
    where
        F: FnOnce(&SigningPayload) -> Result<Signature, E>,
        E: std::fmt::Display,
    {
        if from_path == to_path {
            return Err(ManagedTextFileError::Unchanged);
        }
        let _workspace_authority = self.lock_current_managed_workspace_mutation()?;
        let _serial = self
            .managed_edit
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let (root, from, basis, entry, target, source, target_parent, filesystem) = {
            let held = self.held();
            let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
            let root = open.physical_root().as_path().to_path_buf();
            let entry = open
                .managed_entry_basis(from_path)
                .map_err(ManagedTextFileError::Authoring)?;
            let from = confined_existing_entry(&root, from_path, entry.is_directory)?;
            let target_path = confined_free_path(&root, to_path)?;
            let source = ManagedMutationSource::capture(&root, from_path, entry.is_directory)
                .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
            let target_parent_path = target_path
                .parent()
                .ok_or(ManagedTextFileError::InvalidPath)?;
            let target_parent =
                managed_directory_identity(target_parent_path).map_err(|error| {
                    ManagedTextFileError::io(
                        "capture destination parent identity",
                        target_parent_path,
                        error,
                    )
                })?;
            (
                root,
                from,
                open.managed_authoring_basis(actor_public_key)
                    .map_err(ManagedTextFileError::Authoring)?,
                entry,
                open.managed_create_target(to_path)
                    .map_err(ManagedTextFileError::Authoring)?,
                source,
                target_parent,
                open.pinned_root().filesystem(),
            )
        };
        let operation = if entry.parent_id == target.parent_id {
            Operation::RenameEntry {
                directory_id: entry.parent_id,
                from_name: entry.name,
                to_name: target.name,
                object_id: entry.object_id,
            }
        } else {
            Operation::MoveEntry {
                from_directory_id: entry.parent_id,
                from_name: entry.name,
                to_directory_id: target.parent_id,
                to_name: target.name,
                object_id: entry.object_id,
            }
        };
        let (request, intended) =
            self.signed_operation_request(basis, vec![operation], actor_public_key, sign)?;
        {
            let held = self.held();
            held.as_ref()
                .ok_or(ManagedTextFileError::NoWorkspace)?
                .managed_create_target(to_path)
                .map_err(ManagedTextFileError::Authoring)?;
        }
        let sequence = self.next_managed_sequence()?;
        let intent = ManagedMutationIntent::begin_move_guarded(
            &root,
            from_path,
            to_path,
            intended,
            entry.is_directory,
            source,
            target_parent,
            Some(filesystem),
        )
        .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
        if let Err(error) = intent.apply_move() {
            let _ = intent.clear_after_failed_apply(&error);
            return Err(ManagedTextFileError::Recovery(error.to_string()));
        }
        let saved = self.persist_managed_operation(sequence, request, intended, || {
            intent
                .roll_back_nondurable()
                .map_err(|error| std::io::Error::other(error.to_string()))
        })?;
        self.finish_managed_intent(&intent);
        // Only test builds can inject an OS replacement at this exact durable-to-settling boundary.
        #[cfg(test)]
        AFTER_MANAGED_MOVE_PERSIST.with(|hook| {
            if let Some(hook) = hook.borrow_mut().take() {
                hook();
            }
        });
        let meaningful = match saved {
            Some((_saved, installation)) => {
                self.settle_managed_change(&root, installation, sequence, || {
                    intent.moved_entry_matches().is_ok_and(|matches| matches) && !from.exists()
                })?
                .1
            }
            None => false,
        };
        Ok(ManagedEntryChange::new(
            "move",
            Some(from_path.to_owned()),
            Some(to_path.to_owned()),
            intended.to_string(),
            meaningful,
        ))
    }

    /// Delete one durable file or empty folder while retaining every immutable file version.
    pub fn delete_managed_entry_privately<F, E>(
        &self,
        relative_path: &str,
        expected_content_digest: Option<RecordDigest>,
        expected_executable: Option<bool>,
        actor_public_key: PublicKey,
        sign: F,
    ) -> Result<ManagedEntryChange, ManagedTextFileError>
    where
        F: FnOnce(&SigningPayload) -> Result<Signature, E>,
        E: std::fmt::Display,
    {
        let _workspace_authority = self.lock_current_managed_workspace_mutation()?;
        let _serial = self
            .managed_edit
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let (root, path, basis, entry, source, filesystem) = {
            let held = self.held();
            let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
            let entry = open
                .managed_entry_basis(relative_path)
                .map_err(ManagedTextFileError::Authoring)?;
            let path = confined_existing_entry(
                open.physical_root().as_path(),
                relative_path,
                entry.is_directory,
            )?;
            if entry.is_directory
                && std::fs::read_dir(&path)
                    .map_err(|error| ManagedTextFileError::io("read directory", &path, error))?
                    .next()
                    .is_some()
            {
                return Err(ManagedTextFileError::DirectoryNotEmpty);
            }
            let source = ManagedMutationSource::capture(
                open.physical_root().as_path(),
                relative_path,
                entry.is_directory,
            )
            .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
            (
                open.physical_root().as_path().to_path_buf(),
                path,
                open.managed_authoring_basis(actor_public_key)
                    .map_err(ManagedTextFileError::Authoring)?,
                entry,
                source,
                open.pinned_root().filesystem(),
            )
        };
        let operations = vec![
            Operation::UnlinkDirectoryEntry {
                directory_id: entry.parent_id,
                name: entry.name,
                object_id: entry.object_id,
            },
            Operation::DeleteObject {
                object_id: entry.object_id,
            },
        ];
        let expected_file_state = if entry.is_directory {
            if expected_content_digest.is_some() || expected_executable.is_some() {
                return Err(ManagedTextFileError::StaleInspection);
            }
            None
        } else {
            Some((
                expected_content_digest.ok_or(ManagedTextFileError::StaleInspection)?,
                expected_executable.ok_or(ManagedTextFileError::StaleInspection)?,
            ))
        };
        if let Some((expected_content_digest, expected_executable)) = expected_file_state {
            let current_bytes = std::fs::read(&path)
                .map_err(|error| ManagedTextFileError::io("inspect before delete", &path, error))?;
            let actual = RecordDigest::from_bytes(*Blake3::digest_bytes(&current_bytes).as_bytes());
            let executable = std::fs::metadata(&path)
                .map_err(|error| ManagedTextFileError::io("metadata before delete", &path, error))?
                .permissions()
                .mode()
                & 0o111
                != 0;
            if actual != expected_content_digest || executable != expected_executable {
                return Err(ManagedTextFileError::StaleInspection);
            }
        }
        let (request, intended) =
            self.signed_operation_request(basis, operations, actor_public_key, sign)?;
        let sequence = self.next_managed_sequence()?;
        let intent = ManagedMutationIntent::begin_delete_guarded(
            &root,
            relative_path,
            intended,
            entry.is_directory,
            source,
            Some(filesystem),
        )
        .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
        let staged = intent
            .staged_delete_path()
            .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
        if let Err(error) = intent.stage_delete() {
            let _ = intent.clear_after_failed_apply(&error);
            return Err(ManagedTextFileError::Recovery(error.to_string()));
        }
        let restore_staged = |checkpoint: String| -> Result<(), ManagedTextFileError> {
            intent
                .roll_back_nondurable()
                .map_err(|rollback| ManagedTextFileError::Rollback {
                    checkpoint,
                    rollback: std::io::Error::other(rollback.to_string()),
                })
        };
        if entry.is_directory {
            if !intent
                .staged_delete_directory_is_empty(&staged)
                .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?
            {
                restore_staged(ManagedTextFileError::DirectoryNotEmpty.to_string())?;
                return Err(ManagedTextFileError::DirectoryNotEmpty);
            }
        } else if let Some((expected_content_digest, expected_executable)) = expected_file_state {
            let staged_bytes = match std::fs::read(&staged) {
                Ok(bytes) => bytes,
                Err(error) => {
                    restore_staged(error.to_string())?;
                    return Err(ManagedTextFileError::io(
                        "verify staged delete",
                        &staged,
                        error,
                    ));
                }
            };
            let actual = RecordDigest::from_bytes(*Blake3::digest_bytes(&staged_bytes).as_bytes());
            let executable = match std::fs::metadata(&staged) {
                Ok(metadata) => metadata.permissions().mode() & 0o111 != 0,
                Err(error) => {
                    restore_staged(error.to_string())?;
                    return Err(ManagedTextFileError::io(
                        "verify staged delete metadata",
                        &staged,
                        error,
                    ));
                }
            };
            if actual != expected_content_digest || executable != expected_executable {
                restore_staged(ManagedTextFileError::StaleInspection.to_string())?;
                return Err(ManagedTextFileError::StaleInspection);
            }
        }
        let saved = self.persist_managed_operation(sequence, request, intended, || {
            intent
                .roll_back_nondurable()
                .map_err(|error| std::io::Error::other(error.to_string()))
        })?;
        self.finish_managed_intent(&intent);
        let meaningful = match saved {
            Some((_saved, installation)) => {
                self.settle_managed_change(&root, installation, sequence, || {
                    !path.exists() && !staged.exists()
                })?
                .1
            }
            None => false,
        };
        Ok(ManagedEntryChange::new(
            "delete",
            Some(relative_path.to_owned()),
            None,
            intended.to_string(),
            meaningful,
        ))
    }

    /// Append an authenticated deletion after an editor or agent already removed a tracked file.
    ///
    /// The native absence is never inferred from a path string. The exact durable version,
    /// workspace installation, confined parent identity and absent final name are checked before
    /// and after signing. A concurrent creator wins: the append is refused before durability, or
    /// the resulting checkpoint remains Working when the creator races after the append.
    pub fn adopt_native_file_deletion_privately<F, E>(
        &self,
        relative_path: &str,
        expected_current_version: &str,
        actor_public_key: PublicKey,
        sign: F,
    ) -> Result<ManagedEntryChange, ManagedTextFileError>
    where
        F: FnOnce(&SigningPayload) -> Result<Signature, E>,
        E: std::fmt::Display,
    {
        let _workspace_authority = self.lock_current_managed_workspace_mutation()?;
        let _serial = self
            .managed_edit
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let (root, basis, entry, source_parent) = {
            let held = self.held();
            let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
            let entry = open
                .managed_entry_basis(relative_path)
                .map_err(ManagedTextFileError::Authoring)?;
            if entry.is_directory {
                return Err(ManagedTextFileError::NotRegularFile);
            }
            let current = open
                .file_histories()
                .iter()
                .find(|history| history.path() == relative_path)
                .and_then(|history| history.current())
                .ok_or(ManagedTextFileError::NotRegularFile)?;
            if current.version().to_string() != expected_current_version {
                return Err(ManagedTextFileError::StaleInspection);
            }
            let root = open.physical_root().as_path().to_path_buf();
            let missing = confined_free_path(&root, relative_path)?;
            let parent = missing.parent().ok_or(ManagedTextFileError::InvalidPath)?;
            let source_parent = managed_directory_identity(parent).map_err(|error| {
                ManagedTextFileError::io("capture missing-file parent", parent, error)
            })?;
            (
                root,
                open.managed_authoring_basis(actor_public_key)
                    .map_err(ManagedTextFileError::Authoring)?,
                entry,
                source_parent,
            )
        };
        let operations = vec![
            Operation::UnlinkDirectoryEntry {
                directory_id: entry.parent_id,
                name: entry.name,
                object_id: entry.object_id,
            },
            Operation::DeleteObject {
                object_id: entry.object_id,
            },
        ];
        let (request, intended) =
            self.signed_operation_request(basis, operations, actor_public_key, sign)?;
        let missing = confined_free_path(&root, relative_path)?;
        let parent = missing.parent().ok_or(ManagedTextFileError::InvalidPath)?;
        if managed_directory_identity(parent).map_err(|error| {
            ManagedTextFileError::io("recheck missing-file parent", parent, error)
        })? != source_parent
        {
            return Err(ManagedTextFileError::StaleInspection);
        }
        let sequence = self.next_managed_sequence()?;
        let saved = self.persist_managed_operation(sequence, request, intended, || Ok(()))?;
        let meaningful = match saved {
            Some((_saved, installation)) => {
                self.settle_managed_change(&root, installation, sequence, || {
                    let Ok(missing) = confined_free_path(&root, relative_path) else {
                        return false;
                    };
                    missing
                        .parent()
                        .and_then(|parent| managed_directory_identity(parent).ok())
                        == Some(source_parent)
                })?
                .1
            }
            None => false,
        };
        Ok(ManagedEntryChange::new(
            "adopt_delete",
            Some(relative_path.to_owned()),
            None,
            intended.to_string(),
            meaningful,
        ))
    }

    /// Append an authenticated move after an editor or agent already renamed a tracked file.
    ///
    /// The person supplies the intended old/new pairing. Mesh accepts it only when the old name
    /// is absent, the destination is absent from durable history, and the exact destination inode,
    /// bytes and executable state still match the inspected candidate after signing.
    #[allow(clippy::too_many_arguments)]
    pub fn adopt_native_file_move_privately<F, E>(
        &self,
        from_path: &str,
        to_path: &str,
        expected_current_version: &str,
        expected_destination_digest: RecordDigest,
        expected_destination_executable: bool,
        actor_public_key: PublicKey,
        sign: F,
    ) -> Result<ManagedEntryChange, ManagedTextFileError>
    where
        F: FnOnce(&SigningPayload) -> Result<Signature, E>,
        E: std::fmt::Display,
    {
        if from_path == to_path {
            return Err(ManagedTextFileError::Unchanged);
        }
        let _workspace_authority = self.lock_current_managed_workspace_mutation()?;
        let _serial = self
            .managed_edit
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let (root, basis, entry, target, source_parent, destination, destination_matches_current) = {
            let held = self.held();
            let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
            let entry = open
                .managed_entry_basis(from_path)
                .map_err(ManagedTextFileError::Authoring)?;
            if entry.is_directory {
                return Err(ManagedTextFileError::NotRegularFile);
            }
            let current = open
                .file_histories()
                .iter()
                .find(|history| history.path() == from_path)
                .and_then(|history| history.current())
                .ok_or(ManagedTextFileError::NotRegularFile)?;
            if current.version().to_string() != expected_current_version {
                return Err(ManagedTextFileError::StaleInspection);
            }
            let source_manifest = open
                .manifest_record(current.manifest())
                .ok_or(ManagedTextFileError::NotRegularFile)?;
            let source_metadata = open
                .file_version_metadata(current.version())
                .ok_or(ManagedTextFileError::NotRegularFile)?;
            let root = open.physical_root().as_path().to_path_buf();
            let missing = confined_free_path(&root, from_path)?;
            let source_parent_path = missing.parent().ok_or(ManagedTextFileError::InvalidPath)?;
            let source_parent =
                managed_directory_identity(source_parent_path).map_err(|error| {
                    ManagedTextFileError::io(
                        "capture missing move source parent",
                        source_parent_path,
                        error,
                    )
                })?;
            let target = open
                .managed_create_target(to_path)
                .map_err(ManagedTextFileError::Authoring)?;
            let (destination, bytes) = read_managed_replacement(&root, to_path)?;
            let actual_digest = RecordDigest::from_bytes(*Blake3::digest_bytes(&bytes).as_bytes());
            if actual_digest != expected_destination_digest
                || destination.executable() != expected_destination_executable
            {
                return Err(ManagedTextFileError::StaleInspection);
            }
            let destination_matches_current = actual_digest == source_manifest.content_digest
                && destination.executable() == source_metadata.is_executable();
            (
                root,
                open.managed_authoring_basis(actor_public_key)
                    .map_err(ManagedTextFileError::Authoring)?,
                entry,
                target,
                source_parent,
                destination,
                destination_matches_current,
            )
        };
        let operation = if entry.parent_id == target.parent_id {
            Operation::RenameEntry {
                directory_id: entry.parent_id,
                from_name: entry.name,
                to_name: target.name,
                object_id: entry.object_id,
            }
        } else {
            Operation::MoveEntry {
                from_directory_id: entry.parent_id,
                from_name: entry.name,
                to_directory_id: target.parent_id,
                to_name: target.name,
                object_id: entry.object_id,
            }
        };
        let (request, intended) =
            self.signed_operation_request(basis, vec![operation], actor_public_key, sign)?;
        {
            let held = self.held();
            held.as_ref()
                .ok_or(ManagedTextFileError::NoWorkspace)?
                .managed_create_target(to_path)
                .map_err(ManagedTextFileError::Authoring)?;
        }
        verify_native_move_state(
            &root,
            from_path,
            to_path,
            source_parent,
            &destination,
            expected_destination_digest,
            expected_destination_executable,
        )?;
        let sequence = self.next_managed_sequence()?;
        let saved = self.persist_managed_operation(sequence, request, intended, || Ok(()))?;
        let meaningful = match saved {
            Some((_saved, installation)) => {
                self.settle_managed_change(&root, installation, sequence, || {
                    destination_matches_current
                        && verify_native_move_state(
                            &root,
                            from_path,
                            to_path,
                            source_parent,
                            &destination,
                            expected_destination_digest,
                            expected_destination_executable,
                        )
                        .is_ok()
                })?
                .1
            }
            None => false,
        };
        Ok(ManagedEntryChange::new(
            "adopt_move",
            Some(from_path.to_owned()),
            Some(to_path.to_owned()),
            intended.to_string(),
            meaningful,
        ))
    }

    fn signed_operation_request<F, E>(
        &self,
        basis: crate::workspace::ManagedAuthoringBasis,
        operations: Vec<Operation>,
        actor_public_key: PublicKey,
        sign: F,
    ) -> Result<(AuthenticatedOperationCheckpointRequest, RecordDigest), ManagedTextFileError>
    where
        F: FnOnce(&SigningPayload) -> Result<Signature, E>,
        E: std::fmt::Display,
    {
        {
            let held = self.held();
            held.as_ref()
                .ok_or(ManagedTextFileError::NoWorkspace)?
                .validate_managed_operations(&operations)
                .map_err(ManagedTextFileError::Authoring)?;
        }
        let unsigned = AuthenticatedOperationCheckpointRequest::new(
            basis.workspace_id,
            basis.actor_id,
            basis.session_id,
            basis.actor_sequence,
            basis.causal_parents.clone(),
            basis.base_head,
            basis.policy_epoch,
            basis.hybrid_logical_time,
            operations.clone(),
            actor_public_key,
            Signature::from_bytes([0; 64]),
        );
        let signing_body = operation_checkpoint_signing_body(&unsigned, &LocalChangesetHead);
        let signature = sign(&SigningPayload::new(
            crate::authenticated_changeset::CHANGESET_SIGNATURE_DOMAIN,
            &signing_body,
        ))
        .map_err(|error| ManagedTextFileError::Authoring(error.to_string()))?;
        let intended = authenticated_changeset_id(&signing_body, actor_public_key, signature)?;
        Ok((
            AuthenticatedOperationCheckpointRequest::new(
                basis.workspace_id,
                basis.actor_id,
                basis.session_id,
                basis.actor_sequence,
                basis.causal_parents,
                basis.base_head,
                basis.policy_epoch,
                basis.hybrid_logical_time,
                operations,
                actor_public_key,
                signature,
            ),
            intended,
        ))
    }

    fn persist_managed_operation<R>(
        &self,
        sequence: RecoverySequence,
        request: AuthenticatedOperationCheckpointRequest,
        intended: RecordDigest,
        rollback: R,
    ) -> Result<Option<(JournaledPrivateMutation, CheckpointInstallation)>, ManagedTextFileError>
    where
        R: FnOnce() -> std::io::Result<()>,
    {
        match self.save_operation_set(sequence, request, &LocalChangesetHead) {
            Ok(saved) => Ok(Some(saved)),
            Err(_error) if self.durable_operation(intended) == Some(true) => Ok(None),
            Err(error) => {
                if self.durable_operation(intended) == Some(false) {
                    rollback().map_err(|rollback| ManagedTextFileError::Rollback {
                        checkpoint: error.to_string(),
                        rollback,
                    })?;
                }
                Err(ManagedTextFileError::Authoring(error.to_string()))
            }
        }
    }

    fn finish_managed_intent(&self, intent: &ManagedMutationIntent) {
        match intent.complete_durable() {
            Ok(()) => {
                if let Some(open) = self.held().as_mut() {
                    open.dismiss_managed_mutation_recovery_condition();
                }
            }
            Err(error) => {
                if let Some(open) = self.held().as_mut() {
                    open.record_managed_mutation_recovery_error(&error);
                }
            }
        }
    }

    fn next_managed_sequence(&self) -> Result<RecoverySequence, ManagedTextFileError> {
        let snapshot = self
            .checkpoint_snapshot()
            .map_err(|error| ManagedTextFileError::Checkpoint(error.to_string()))?;
        [
            snapshot.open_window().map(|window| window.last().get()),
            snapshot
                .latest_recovery()
                .map(|recovery| recovery.through().get()),
            snapshot
                .last_meaningful()
                .map(|checkpoint| checkpoint.through().get()),
        ]
        .into_iter()
        .flatten()
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .and_then(RecoverySequence::new)
        .ok_or_else(|| ManagedTextFileError::Checkpoint("recovery sequence exhausted".to_owned()))
    }

    fn settle_managed_change<V>(
        &self,
        root: &Path,
        installation: CheckpointInstallation,
        sequence: RecoverySequence,
        stable: V,
    ) -> Result<(bool, bool), ManagedTextFileError>
    where
        V: FnOnce() -> bool,
    {
        let intervals = self
            .checkpoint_intervals_for(root, Some(installation))
            .map_err(|error| ManagedTextFileError::Checkpoint(error.to_string()))?;
        let first_interval = intervals.idle.min(intervals.maximum);
        std::thread::sleep(first_interval);
        if intervals.maximum <= intervals.idle {
            self.preserve_pending_recovery_for(intervals.maximum, root, installation, sequence)
                .map_err(|error| ManagedTextFileError::Checkpoint(error.to_string()))?;
        }
        std::thread::sleep(intervals.idle.saturating_sub(first_interval));
        if !stable() {
            return Ok((false, false));
        }
        let transition = self
            .settle_pending_checkpoint_for(
                intervals.idle,
                Some(root),
                Some(installation),
                Some(sequence),
            )
            .map_err(|error| ManagedTextFileError::Checkpoint(error.to_string()))?;
        let already_settled = self
            .checkpoint_settled_for(root, installation, sequence)
            .map_err(|error| ManagedTextFileError::Checkpoint(error.to_string()))?;
        Ok((
            true,
            transition.is_some_and(|transition| transition.private_saved().is_some())
                || already_settled,
        ))
    }

    fn durable_operation(&self, id: RecordDigest) -> Option<bool> {
        self.held().as_ref().map(|open| open.has_operation(&id))
    }

    /// Restore one retained immutable version into the managed operating-system working copy.
    ///
    /// The selected manifest and every chunk are re-read and verified from CAS. History remains
    /// append-only: this produces a recovery-preserved `Working` state and can be undone by
    /// restoring the previously current retained version through the same method.
    pub fn restore_managed_file_version(
        &self,
        object_id: &str,
        target_version: &str,
        expected_content_digest: RecordDigest,
        expected_executable: bool,
    ) -> Result<ManagedVersionRestore, ManagedTextFileError> {
        let _workspace_authority = self.lock_current_managed_workspace_mutation()?;
        let _serial = self
            .managed_edit
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let (
            root,
            storage_root,
            relative_path,
            target,
            prior,
            manifest_id,
            manifest,
            target_metadata,
            filesystem,
        ) = {
            let held = self.held();
            let open = held.as_ref().ok_or(ManagedTextFileError::NoWorkspace)?;
            if open.managed_mutation_recovery_needed() {
                return Err(ManagedTextFileError::Recovery(
                    "an interrupted local file change needs attention".to_owned(),
                ));
            }
            let history = open
                .file_histories()
                .iter()
                .find(|history| history.object().to_string() == object_id)
                .ok_or(ManagedTextFileError::UnknownRetainedVersion)?;
            let restore_identity = history
                .retained()
                .iter()
                .find(|version| version.version().to_string() == target_version)
                .ok_or(ManagedTextFileError::UnknownRetainedVersion)?;
            let manifest = open
                .manifest_record(restore_identity.manifest())
                .cloned()
                .ok_or(ManagedTextFileError::UnknownRetainedVersion)?;
            let manifest_id = restore_identity.manifest();
            let target_metadata = open
                .file_version_metadata(restore_identity.version())
                .ok_or(ManagedTextFileError::UnknownRetainedVersion)?;
            open.ensure_physical_root()
                .map_err(|error| ManagedTextFileError::Recovery(error.to_string()))?;
            let root = open.physical_root().as_path().to_path_buf();
            let storage_root = open.storage_root().as_path().to_path_buf();
            let (target, prior) = read_managed_replacement(&root, history.path())?;
            (
                root,
                storage_root,
                history.path().to_owned(),
                target,
                prior,
                manifest_id,
                manifest,
                target_metadata,
                open.storage_pinned_root().filesystem(),
            )
        };
        if RecordDigest::from_bytes(*Blake3::digest_bytes(&prior).as_bytes())
            != expected_content_digest
            || target.executable() != expected_executable
        {
            return Err(ManagedTextFileError::StaleInspection);
        }
        let cas = Cas::<_, mesh_cas::Blake3>::with_filesystem(storage_root, filesystem)
            .map_err(|error| ManagedTextFileError::RetainedContent(error.to_string()))?;
        let capacity = usize::try_from(manifest.byte_length).map_err(|_| {
            ManagedTextFileError::RetainedContent(
                "the retained file length does not fit this platform".to_owned(),
            )
        })?;
        let mut restored = Vec::with_capacity(capacity);
        let mut expected_offset = 0_u64;
        for chunk in &manifest.chunks {
            if chunk.byte_offset != expected_offset {
                return Err(ManagedTextFileError::RetainedContent(
                    "the retained manifest has a gap or overlapping chunk".to_owned(),
                ));
            }
            let bytes = cas
                .read(&mesh_cas::Digest32::from_bytes(*chunk.digest.as_bytes()))
                .map_err(|error| ManagedTextFileError::RetainedContent(error.to_string()))?;
            if u64::try_from(bytes.len()).ok() != Some(chunk.byte_length) {
                return Err(ManagedTextFileError::RetainedContent(
                    "a retained chunk length does not match its manifest".to_owned(),
                ));
            }
            restored.extend_from_slice(&bytes);
            expected_offset = expected_offset
                .checked_add(chunk.byte_length)
                .ok_or_else(|| {
                    ManagedTextFileError::RetainedContent(
                        "the retained manifest length overflowed".to_owned(),
                    )
                })?;
        }
        if expected_offset != manifest.byte_length
            || RecordDigest::from_bytes(*Blake3::digest_bytes(&restored).as_bytes())
                != manifest.content_digest
        {
            return Err(ManagedTextFileError::RetainedContent(
                "the reconstructed bytes do not match the retained manifest".to_owned(),
            ));
        }
        if prior == restored && target.executable() == target_metadata.is_executable() {
            return Err(ManagedTextFileError::Unchanged);
        }

        let mut envelope = Vec::with_capacity(relative_path.len() + 128);
        envelope.extend_from_slice(b"mesh.local-managed-version-restore/1\0");
        envelope.extend_from_slice(&(relative_path.len() as u64).to_be_bytes());
        envelope.extend_from_slice(relative_path.as_bytes());
        envelope.extend_from_slice(target_version.as_bytes());
        envelope.extend_from_slice(manifest_id.as_bytes());
        let (recovery, stable_after_idle) = self.preserve_managed_bytes(
            &root,
            &target,
            &relative_path,
            ManagedReplacement {
                prior: &prior,
                bytes: &restored,
                executable: target_metadata.is_executable(),
            },
            envelope,
        )?;
        Ok(ManagedVersionRestore::new(
            relative_path,
            target_version.to_owned(),
            manifest.content_digest,
            target_metadata.is_executable(),
            recovery,
            stable_after_idle,
        ))
    }

    fn preserve_managed_bytes(
        &self,
        root: &Path,
        target: &ManagedReplacementTarget,
        relative_path: &str,
        replacement: ManagedReplacement<'_>,
        recovery_envelope: Vec<u8>,
    ) -> Result<(RecordDigest, bool), ManagedTextFileError> {
        let snapshot = self
            .checkpoint_snapshot()
            .map_err(|error| ManagedTextFileError::Checkpoint(error.to_string()))?;
        let last = [
            snapshot.open_window().map(|window| window.last().get()),
            snapshot
                .latest_recovery()
                .map(|recovery| recovery.through().get()),
            snapshot
                .last_meaningful()
                .map(|checkpoint| checkpoint.through().get()),
        ]
        .into_iter()
        .flatten()
        .max()
        .unwrap_or(0);
        let next = last
            .checked_add(1)
            .and_then(RecoverySequence::new)
            .ok_or_else(|| {
                ManagedTextFileError::Checkpoint("recovery sequence exhausted".to_owned())
            })?;

        let intervals = self
            .checkpoint_intervals_for(root, None)
            .map_err(|error| ManagedTextFileError::Checkpoint(error.to_string()))?;
        let replacement_target = atomic_replace_with_mode(
            target,
            replacement.prior,
            replacement.bytes,
            target.mode_with_executable(replacement.executable),
        )
        .map_err(|error| ManagedTextFileError::io("replace", &target.path, error))?;
        let checkpoint = (|| {
            let digest =
                RecordDigest::from_bytes(*Blake3::digest_bytes(&recovery_envelope).as_bytes());
            let mut event = [0u8; 16];
            event.copy_from_slice(&digest.as_bytes()[..16]);
            let preserved = RecoveryPreserved::from_verified_bytes(
                RecoveryStamp::new(next.get(), RecoveryEventUlid::from_bytes(event), digest),
                next,
                recovery_envelope,
                digest,
            )
            .map_err(|error| error.to_string())?;
            let (mut checkpoint, _held) = self
                .checkpoint_mutation_guards()
                .map_err(|error| error.to_string())?;
            checkpoint
                .runtime_mut()
                .map_err(|error| error.to_string())?
                .observe_evidence_and_recover(
                    next,
                    u64::try_from(replacement.bytes.len())
                        .map_err(|_| "managed byte count is too large".to_owned())?,
                    RecoveryTrigger::AtomicReplacementCompleted,
                    RecoveryBoundaryEvidence::new(next, BoundaryEvidenceKind::RenamedIntoPlace),
                    RecoveryTrigger::IntegratedAgentRequestsFlush,
                    preserved,
                )
                .map_err(|error| error.to_string())?;
            Ok::<_, String>(digest)
        })();
        let recovery = match checkpoint {
            Ok(digest) => digest,
            Err(checkpoint) => {
                if let Err(rollback) = atomic_replace_with_mode(
                    &replacement_target,
                    replacement.bytes,
                    replacement.prior,
                    target.mode_with_executable(target.executable()),
                ) {
                    return Err(ManagedTextFileError::Rollback {
                        checkpoint,
                        rollback,
                    });
                }
                return Err(ManagedTextFileError::Checkpoint(checkpoint));
            }
        };

        std::thread::sleep(intervals.idle);
        let stable_after_idle =
            read_managed_replacement(root, relative_path).is_ok_and(|(target, current)| {
                current == replacement.bytes && target.executable() == replacement.executable
            });
        Ok((recovery, stable_after_idle))
    }

    /// The last-durable-boundary report for the workspace this daemon holds.
    ///
    /// `None` when nothing is open: a daemon with no workspace has no boundary, and a report full
    /// of zeroes would read like one that found nothing.
    #[must_use]
    pub fn crash_report(&self) -> Option<CrashReport> {
        // Match the save path's checkpoint -> workspace order. A diagnostic must not introduce a
        // lock inversion with the path it is diagnosing.
        let checkpoint = self
            .checkpoint
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let snapshot = checkpoint
            .active
            .as_ref()
            .map(|runtime| runtime.machine().snapshot());
        let held = self.held();
        held.as_ref().map(|open| match snapshot {
            Some(snapshot) => CrashReport::of_with_checkpoint(open, snapshot),
            None => CrashReport::of(open),
        })
    }

    /// Say that this daemon is stopping, so every subscriber hears it before the socket closes.
    pub fn announce_stopping(&self) {
        self.feed.publish(EventKind::Stopping);
    }

    fn held(&self) -> MutexGuard<'_, Option<OpenWorkspace>> {
        self.open.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn open_and_install_workspace_with(
        &self,
        root: &Path,
        create_missing: bool,
    ) -> Result<(WorkspaceSummary, StartupSummary), OpenFailure> {
        // A managed mutation derives its signed basis and operating-system target from the held
        // workspace before it enters the journal transaction. Letting an open cross that interval
        // can split one user action across two byte-identical workspace copies: filesystem bytes
        // land in the old root while the signed record is admitted into the replacement. Opens
        // therefore join the same mutation serial before taking the established checkpoint ->
        // workspace pair. Existing-root callers acquire the pinned physical workspace serial
        // before `workspace_open`; create-only version/import destinations are independently
        // fenced from the assigned root. The remaining order is
        // `workspace_open -> managed_edit -> checkpoint -> open`.
        let _managed_serial = self
            .managed_edit
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        // Reading, installing the coordinator, and swapping the held workspace are one state
        // transition. Reading before acquiring this pair lets a same-workspace save complete in
        // between and then replaces its newer live view with the older snapshot. Keep the
        // established checkpoint -> workspace lock order and publish only after all three steps.
        let mut checkpoint = self
            .checkpoint
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut held = self.held();
        checkpoint
            .preserve_legacy_before_index_repair(root)
            .map_err(|detail| OpenFailure::Index { detail })?;
        let opened = if create_missing {
            OpenWorkspace::open_with_trusted_reviewers(root, &self.trusted_reviewers)
        } else {
            OpenWorkspace::reopen_with_trusted_reviewers(root, &self.trusted_reviewers)
        }?;
        self.install_opened_workspace(&mut checkpoint, &mut held, opened)
    }

    fn install_opened_workspace(
        &self,
        checkpoint: &mut LiveCheckpointRuntime,
        held: &mut Option<OpenWorkspace>,
        mut opened: OpenWorkspace,
    ) -> Result<(WorkspaceSummary, StartupSummary), OpenFailure> {
        let startup = StartupSummary::from(opened.diagnostic());
        let mut replacement_checkpoint = checkpoint.replacement_candidate();
        replacement_checkpoint
            .install(opened.database_file())
            .map_err(|detail| OpenFailure::Index { detail })?;
        replacement_checkpoint
            .validate_journal_recovery_pointer(&mut opened)
            .map_err(|detail| OpenFailure::Index { detail })?;
        replacement_checkpoint.preserve_pending_recovery_after_restart(&mut opened);
        let automatic_review_target = replacement_checkpoint
            .runtime()
            .ok()
            .and_then(|runtime| runtime.machine().snapshot().last_meaningful())
            .map(|checkpoint| checkpoint.stamp().content_hash());
        let summary = summarise(&opened, automatic_review_target);
        // Installation and recovery admission are part of the workspace-open transition. A
        // failure leaves the daemon holding its prior workspace, so counting the preceding fold
        // as a completed open would publish work that the daemon refused to admit.
        self.record_open(&opened);
        *checkpoint = replacement_checkpoint;
        *held = Some(opened);
        Ok((summary, startup))
    }

    fn open_workspace_after_serial(&self, path: &Path) -> Result<WorkspaceSummary, Unavailable> {
        self.open_workspace_after_serial_with(path, true)
    }

    /// Reopen an existing managed workspace during the daemon's serving lifetime.
    ///
    /// This is the runtime counterpart to [`Self::reopen_at_start`]: it never creates missing
    /// workspace state, but a refusal also leaves the current workspace and start-up report
    /// untouched. Desktop navigation uses this boundary so choosing an ordinary project in the
    /// "Open existing" form cannot initialize a plausible empty history inside it.
    ///
    /// # Errors
    ///
    /// Returns the same user-facing refusal as a runtime workspace open.
    pub fn reopen_existing_workspace(&self, path: &Path) -> Result<WorkspaceSummary, Unavailable> {
        let _custody = crate::workspace_custody::lock_workspace_path_initialization(path, false)
            .map_err(|_| {
                Unavailable::new(
                    "workspace-directory-changed",
                    user_messages::WORKSPACE_DIRECTORY_CHANGED,
                )
            })?;
        run_after_reopen_directory_lock();
        let _open_serial = self
            .workspace_open
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        self.open_workspace_after_serial_with(path, false)
    }

    fn open_workspace_after_serial_with(
        &self,
        path: &Path,
        create_missing: bool,
    ) -> Result<WorkspaceSummary, Unavailable> {
        match self.open_and_install_workspace_with(path, create_missing) {
            Ok((summary, startup)) => {
                *self.startup.lock().unwrap_or_else(PoisonError::into_inner) = startup;
                self.feed.publish(EventKind::WorkspaceOpened {
                    records: summary.records,
                });
                let recovery_needs_attention = summary
                    .conditions
                    .iter()
                    .any(|condition| condition.code() == "checkpoint-recovery-needs-attention");
                if recovery_needs_attention {
                    self.feed
                        .publish(EventKind::CheckpointRecoveryNeedsAttention);
                } else {
                    let _checkpoint_resume = self.schedule_pending_checkpoint();
                }
                Ok(summary)
            }
            Err(failure) => {
                let refusal = refusal_for(&failure);
                self.feed.publish(EventKind::WorkspaceRefused {
                    code: refusal.code.clone(),
                });
                Err(refusal)
            }
        }
    }

    fn checkpoint_mut(
        &self,
    ) -> Result<MutexGuard<'_, LiveCheckpointRuntime>, AutomaticCheckpointError> {
        let guard = self
            .checkpoint
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        match guard.status() {
            AutomaticCheckpointStatus::Active => Ok(guard),
            status => Err(AutomaticCheckpointError::snapshot_unavailable(status)),
        }
    }

    fn checkpoint_mutation_guards(
        &self,
    ) -> Result<
        (
            MutexGuard<'_, LiveCheckpointRuntime>,
            MutexGuard<'_, Option<OpenWorkspace>>,
        ),
        AutomaticCheckpointError,
    > {
        let checkpoint = self.checkpoint_mut()?;
        let held = self.held();
        let open = held.as_ref().ok_or(AutomaticCheckpointError::NoWorkspace)?;
        checkpoint.ensure_installed_for(open.database_file())?;
        if open.checkpoint_recovery_needs_attention() {
            return Err(AutomaticCheckpointError::RecoveryNeedsAttention);
        }
        Ok((checkpoint, held))
    }

    fn checkpoint_intervals_for(
        &self,
        expected_root: &Path,
        expected_installation: Option<CheckpointInstallation>,
    ) -> Result<PendingCheckpointIntervals, AutomaticCheckpointError> {
        // Keep the checkpoint -> workspace lock order used by every mutation path. The configured
        // intervals belong to one installed workspace, so a path or installation mismatch must
        // not silently inherit either selected defaults or another workspace's overrides.
        let checkpoint = self.checkpoint_mut()?;
        let held = self.held();
        let open = held.as_ref().ok_or(AutomaticCheckpointError::NoWorkspace)?;
        open.ensure_physical_root()
            .map_err(|_| AutomaticCheckpointError::WorkspaceChanged)?;
        checkpoint.ensure_installed_for(open.database_file())?;
        if open.physical_root().as_path() != expected_root
            || expected_installation
                .is_some_and(|expected| checkpoint.active_installation != Some(expected))
        {
            return Err(AutomaticCheckpointError::WorkspaceChanged);
        }
        let config = checkpoint.runtime()?.config();
        Ok(PendingCheckpointIntervals {
            idle: config.idle_interval(),
            maximum: config.maximum_uncheckpointed_interval(),
        })
    }

    fn checkpoint_ref(
        &self,
    ) -> Result<MutexGuard<'_, LiveCheckpointRuntime>, AutomaticCheckpointError> {
        self.checkpoint_mut()
    }

    /// Open one immutable saved version as an independent native folder. When the desktop still
    /// remembers the exact ordinary folder that supplied the workspace, carry deletion authority
    /// only for historical objects whose own import-origin receipts prove that same directory.
    pub fn fork_workspace_version_with_origin(
        &self,
        operation: &str,
        destination: &str,
        expected_root: &str,
        expected_digest: &str,
        expected_installation: &str,
        origin_target: Option<&Path>,
    ) -> Result<crate::ipc::Json, Unavailable> {
        self.fork_workspace_version_protected(WorkspaceVersionForkRequest::new(
            operation,
            destination,
            expected_root,
            expected_digest,
            expected_installation,
            origin_target,
        ))
    }

    /// Open a saved version while keeping the destination outside every exact protected root.
    ///
    /// The protected identities are checked after the create-only import has pinned the actual
    /// destination. This closes the rename window between a desktop pathname preflight and the
    /// daemon transaction without making those remembered paths mutation authority.
    pub fn fork_workspace_version_protected(
        &self,
        request: WorkspaceVersionForkRequest<'_>,
    ) -> Result<crate::ipc::Json, Unavailable> {
        let WorkspaceVersionForkRequest {
            operation,
            destination,
            expected_root,
            expected_digest,
            expected_installation,
            origin_target,
            protected_roots,
            expected_destination_parent,
        } = request;
        let operation = RecordDigest::parse_hex(operation)
            .map_err(|_| workspace_version_refusal("workspace-version-invalid"))?;
        // The source plan is immutable after this scope. Destination creation and its own
        // workspace opens must never run while `workspace_open` is held: ordinary navigation
        // takes the destination's physical-directory serial first.
        let (snapshot, source_ordinal, inherited_origin, export) = {
            let _open_serial = self
                .workspace_open
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let _managed_serial = self
                .managed_edit
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let held = self.held();
            let open = held
                .as_ref()
                .ok_or_else(|| workspace_version_refusal("workspace-version-no-workspace"))?;
            if open.root().as_path() != Path::new(expected_root)
                || open.digest().to_string() != expected_digest
                || open.installation() != expected_installation
            {
                return Err(Unavailable::new(
                    "workspace-version-source-changed",
                    "The open workspace changed after this saved version was shown. Refresh and choose the version again; no folder was created.",
                ));
            }
            validate_independent_workspace_version_destination(
                open,
                Path::new(destination),
                origin_target,
            )?;
            let snapshot = open
                .historical_workspace_preview(operation)
                .map_err(|_| workspace_version_refusal("workspace-version-history-incomplete"))?;
            if snapshot.directories.is_empty() && snapshot.files.is_empty() {
                return Err(Unavailable::new(
                    "workspace-version-empty",
                    "This saved version contains no files or folders. Mesh cannot open an empty saved version as a new working folder yet. Choose a saved version containing files or folders. The current workspace was not changed and no folder was created.",
                ));
            }
            let source_ordinal = open
                .workspace_versions()
                .into_iter()
                .find(|version| version.operation() == operation)
                .map(crate::workspace::WorkspaceVersion::ordinal)
                .ok_or_else(|| workspace_version_refusal("workspace-version-history-incomplete"))?;
            let inherited_origin = origin_target
                .map(|target| {
                    let target = target.canonicalize().map_err(|_| {
                        workspace_version_refusal("workspace-version-origin-unavailable")
                    })?;
                    let installation = managed_directory_identity(&target)
                        .map_err(|_| {
                            workspace_version_refusal("workspace-version-origin-unavailable")
                        })?
                        .token();
                    let paths = snapshot
                        .directories
                        .iter()
                        .filter(|directory| {
                            import_origin_proves_target(
                                open,
                                &target,
                                &installation,
                                directory.object,
                                &directory.path,
                            )
                        })
                        .map(|directory| PathBuf::from(&directory.path))
                        .chain(
                            snapshot
                                .files
                                .iter()
                                .filter(|file| {
                                    import_origin_proves_target(
                                        open,
                                        &target,
                                        &installation,
                                        file.object,
                                        &file.path,
                                    )
                                })
                                .map(|file| PathBuf::from(&file.path)),
                        )
                        .collect::<BTreeSet<_>>();
                    Ok::<_, Unavailable>((target, installation, paths))
                })
                .transpose()?;
            let export = TemporaryHistoricalExport::create_streaming(
                Path::new(destination),
                open,
                &snapshot,
                protected_roots,
                expected_destination_parent,
            )?;
            (snapshot, source_ordinal, inherited_origin, export)
        };

        let prepared = crate::PreparedFolderImport::prepare_presented_with_parent(
            export.path(),
            Path::new(destination),
            protected_roots,
            expected_destination_parent,
        )
        .map_err(|error| match error {
            crate::FolderImportError::DestinationInsideProtectedRoot { .. } => Unavailable::new(
                "workspace-version-destination-overlaps-remembered",
                "Choose a new location outside every workspace and original project already remembered by Mesh. No folder was created.",
            ),
            _ => workspace_version_refusal("workspace-version-destination-refused"),
        })?;
        if prepared
            .destination_is_within_any(protected_roots)
            .map_err(|_| workspace_version_refusal("workspace-version-destination-refused"))?
        {
            return Err(Unavailable::new(
                "workspace-version-destination-overlaps-remembered",
                "Choose a new location outside every workspace and original project already remembered by Mesh. No folder was created.",
            ));
        }
        let (confirmed, imported) = match &inherited_origin {
            Some((target, installation, paths)) => {
                prepared.confirm_into_workspace_with_origin(target, installation, paths)
            }
            None => prepared.confirm_into_workspace_without_origin(),
        }
        .map_err(|_| workspace_version_refusal("workspace-version-import-failed"))?;
        let presented = confirmed.destination().to_path_buf();
        {
            // Confirmation may leave the newly created disposable index in WAL mode before an
            // isolated recovery owner exists. While the exact destination is still unshared,
            // migrate only its default recovery value and close it into a quiescent family. Any
            // later opener recreates a WAL family, which the immutable full predicate below
            // refuses instead of mistaking concurrent checkpoint activity for a clean copy.
            let _fresh_destination_serial =
                crate::workspace_custody::lock_workspace_path_initialization(&presented, false)
                    .map_err(|_| workspace_version_refusal("workspace-version-import-failed"))?;
            let index = workspace_storage_root(&presented)
                .map_err(|_| workspace_version_refusal("workspace-version-destination-refused"))?
                .join(DATABASE_FILE_NAME);
            let default = SqliteRecoveryState::quiesce_default_isolated(
                recovery_database(&index),
                &index,
                LIVE_WORKSPACE_VIEW,
            )
            .map_err(|_| workspace_version_refusal("workspace-version-destination-refused"))?;
            if !default {
                return Err(workspace_version_refusal(
                    "workspace-version-destination-refused",
                ));
            }
        }
        run_after_version_fork_confirm();
        // Reopen the confirmed destination while its physical-directory serial is outermost.
        // `OpenWorkspace` recognizes this exact device/inode as the already-held initialization
        // authority and does not take a second flock. Only the descriptor-pinned candidate crosses
        // into the daemon serial below.
        let _destination_serial =
            crate::workspace_custody::lock_workspace_path_initialization(&presented, false)
                .map_err(|_| workspace_version_refusal("workspace-version-import-failed"))?;
        let candidate =
            OpenWorkspace::reopen_with_trusted_reviewers(&presented, &self.trusted_reviewers)
                .map_err(|_| workspace_version_refusal("workspace-version-import-failed"))?;
        let (workspace, startup) = {
            let _open_serial = self
                .workspace_open
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let _managed_serial = self
                .managed_edit
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let mut checkpoint = self
                .checkpoint
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let mut held = self.held();
            let source = held
                .as_ref()
                .ok_or_else(|| workspace_version_refusal("workspace-version-no-workspace"))?;
            if !workspace_source_is_exact(
                source,
                expected_root,
                expected_digest,
                expected_installation,
            ) {
                return Err(Unavailable::new(
                    "workspace-version-source-changed",
                    "The open workspace changed after this saved version was shown. The verified copy remains available at the chosen destination, but Mesh did not switch to it.",
                ));
            }
            validate_independent_workspace_version_destination(source, &presented, origin_target)?;
            if !workspace_version_candidate_is_exact(
                &candidate,
                &snapshot,
                inherited_origin.as_ref(),
            ) {
                return Err(workspace_version_refusal(
                    "workspace-version-destination-refused",
                ));
            }
            self.install_opened_workspace(&mut checkpoint, &mut held, candidate)
                .map_err(|_| workspace_version_refusal("workspace-version-import-failed"))?
        };
        *self.startup.lock().unwrap_or_else(PoisonError::into_inner) = startup;
        self.feed.publish(EventKind::WorkspaceOpened {
            records: workspace.records,
        });
        let recovery_needs_attention = workspace
            .conditions
            .iter()
            .any(|condition| condition.code() == "checkpoint-recovery-needs-attention");
        if recovery_needs_attention {
            self.feed
                .publish(EventKind::CheckpointRecoveryNeedsAttention);
        } else {
            let _checkpoint_resume = self.schedule_pending_checkpoint();
        }
        drop(confirmed);
        drop(export);
        Ok(crate::ipc::Json::object([
            (
                "action",
                crate::ipc::Json::text("workspace-version-opened-as-copy"),
            ),
            (
                "source_version",
                crate::ipc::Json::text(snapshot.operation.to_string()),
            ),
            ("source_ordinal", crate::ipc::Json::Number(source_ordinal)),
            (
                "destination",
                crate::ipc::Json::text(presented.to_string_lossy()),
            ),
            (
                "private_store",
                crate::ipc::Json::text(Path::new(destination).to_string_lossy()),
            ),
            (
                "materialized_entries",
                crate::ipc::Json::Number(imported.entries() as u64),
            ),
            ("workspace", workspace.to_json()),
        ]))
    }

    /// Reopen an app-owned historical checkout only when it is still the exact clean native
    /// materialization of the selected source point.
    ///
    /// A refusal is reported as `Ok(None)`: the caller should preserve the existing folder and
    /// allocate a fresh checkout. The currently open source workspace is replaced only after the
    /// candidate's durable history, CAS bytes, native tree, recovery state, workspace-native
    /// agent custody and inherited Pull-back authority have all agreed while the open/mutation
    /// locks are held.
    pub fn reopen_workspace_version_if_exact(
        &self,
        operation: &str,
        candidate_store: &Path,
        expected_root: &str,
        expected_digest: &str,
        expected_installation: &str,
        origin_target: Option<&Path>,
    ) -> Result<Option<crate::ipc::Json>, Unavailable> {
        let operation = RecordDigest::parse_hex(operation)
            .map_err(|_| workspace_version_refusal("workspace-version-invalid"))?;
        let presented = match crate::workspace::presented_workspace_path(candidate_store) {
            Ok(presented) => presented,
            Err(_) => return Ok(None),
        };
        // Match ordinary navigation's global order. Open and retain the descriptor-pinned
        // candidate under its exact device/inode guard before entering `workspace_open`; the
        // candidate is then revalidated against the still-current source before installation.
        let _candidate_serial =
            match crate::workspace_custody::lock_workspace_path_initialization(&presented, false) {
                Ok(serial) => serial,
                Err(_) => return Ok(None),
            };
        if preserve_legacy_checkpoint_before_index_repair(&presented).is_err() {
            return Ok(None);
        }
        let candidate =
            match OpenWorkspace::reopen_with_trusted_reviewers(&presented, &self.trusted_reviewers)
            {
                Ok(candidate) => candidate,
                Err(_) => return Ok(None),
            };
        if crate::workspace_custody::require_unassigned_while_initialized(
            candidate.physical_root().as_path(),
            &candidate.installation(),
        )
        .is_err()
        {
            return Ok(None);
        }
        let _open_serial = self
            .workspace_open
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let _managed_serial = self
            .managed_edit
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut checkpoint = self
            .checkpoint
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut held = self.held();
        let source = held
            .as_ref()
            .ok_or_else(|| workspace_version_refusal("workspace-version-no-workspace"))?;
        if source.root().as_path() != Path::new(expected_root)
            || source.digest().to_string() != expected_digest
            || source.installation() != expected_installation
        {
            return Err(Unavailable::new(
                "workspace-version-source-changed",
                "The open workspace changed after this saved version was shown. Refresh and choose the version again; no folder was created.",
            ));
        }
        validate_independent_workspace_version_destination(source, candidate_store, None)?;
        let source_snapshot = source
            .historical_workspace_preview(operation)
            .map_err(|_| workspace_version_refusal("workspace-version-history-incomplete"))?;
        let source_ordinal = source
            .workspace_versions()
            .into_iter()
            .find(|version| version.operation() == operation)
            .map(crate::workspace::WorkspaceVersion::ordinal)
            .ok_or_else(|| workspace_version_refusal("workspace-version-history-incomplete"))?;
        let expected_origin = origin_target
            .map(|target| {
                let target = target.canonicalize().map_err(|_| {
                    workspace_version_refusal("workspace-version-origin-unavailable")
                })?;
                let installation = managed_directory_identity(&target)
                    .map_err(|_| workspace_version_refusal("workspace-version-origin-unavailable"))?
                    .token();
                let paths = snapshot_origin_paths(source, &source_snapshot, &target, &installation);
                Ok::<_, Unavailable>((target, installation, paths))
            })
            .transpose()?;

        if !workspace_version_candidate_is_exact(
            &candidate,
            &source_snapshot,
            expected_origin.as_ref(),
        ) {
            return Ok(None);
        }
        let (summary, startup) =
            match self.install_opened_workspace(&mut checkpoint, &mut held, candidate) {
                Ok(installed) => installed,
                Err(_) => return Ok(None),
            };
        drop(held);
        drop(checkpoint);
        *self.startup.lock().unwrap_or_else(PoisonError::into_inner) = startup;
        self.feed.publish(EventKind::WorkspaceOpened {
            records: summary.records,
        });
        Ok(Some(crate::ipc::Json::object([
            (
                "action",
                crate::ipc::Json::text("workspace-version-reopened-existing-copy"),
            ),
            (
                "source_version",
                crate::ipc::Json::text(operation.to_string()),
            ),
            ("source_ordinal", crate::ipc::Json::Number(source_ordinal)),
            (
                "destination",
                crate::ipc::Json::text(presented.to_string_lossy()),
            ),
            (
                "private_store",
                crate::ipc::Json::text(candidate_store.to_string_lossy()),
            ),
            (
                "materialized_entries",
                crate::ipc::Json::Number(
                    u64::try_from(source_snapshot.directories.len() + source_snapshot.files.len())
                        .unwrap_or(u64::MAX),
                ),
            ),
            ("reused", crate::ipc::Json::Bool(true)),
            ("workspace", summary.to_json()),
        ])))
    }

    /// Install a freshly imported attached snapshot into an independent, still-empty lane daemon.
    /// This is native-only and carries no original-folder writeback authority.
    pub(crate) fn install_attached_lane(
        &self,
        prepared: crate::PreparedFolderImport,
        snapshot: &HistoricalWorkspacePreview,
    ) -> Result<WorkspaceSummary, Unavailable> {
        let (confirmed, _) = prepared
            .confirm_into_workspace_without_origin()
            .map_err(|_| workspace_version_refusal("fleet-attachment-import-failed"))?;
        let presented = confirmed.destination();
        let _custody =
            crate::workspace_custody::lock_workspace_path_initialization(presented, false)
                .map_err(|_| workspace_version_refusal("fleet-attachment-import-failed"))?;
        let index = workspace_storage_root(presented)
            .map_err(|_| workspace_version_refusal("fleet-attachment-import-failed"))?
            .join(DATABASE_FILE_NAME);
        if !SqliteRecoveryState::quiesce_default_isolated(
            recovery_database(&index),
            &index,
            LIVE_WORKSPACE_VIEW,
        )
        .map_err(|_| workspace_version_refusal("fleet-attachment-import-failed"))?
        {
            return Err(workspace_version_refusal("fleet-attachment-import-failed"));
        }
        let candidate =
            OpenWorkspace::reopen_with_trusted_reviewers(presented, &self.trusted_reviewers)
                .map_err(|_| workspace_version_refusal("fleet-attachment-import-failed"))?;
        if !workspace_version_candidate_is_exact(&candidate, snapshot, None) {
            return Err(workspace_version_refusal(
                "fleet-attachment-content-changed",
            ));
        }
        let _open_serial = self
            .workspace_open
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let _managed_serial = self
            .managed_edit
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut checkpoint = self
            .checkpoint
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut held = self.held();
        if held.is_some() {
            return Err(workspace_version_refusal(
                "fleet-attachment-context-not-empty",
            ));
        }
        let (workspace, startup) = self
            .install_opened_workspace(&mut checkpoint, &mut held, candidate)
            .map_err(|_| workspace_version_refusal("fleet-attachment-import-failed"))?;
        *self.startup.lock().unwrap_or_else(PoisonError::into_inner) = startup;
        Ok(workspace)
    }

    /// Recover a confirmed first import that became durable before the native application could
    /// publish its recent-workspace and stable-folder navigation.
    ///
    /// The app-owned pathname is only a lookup hint. Adoption requires the canonical confirmed
    /// import receipt, an unchanged exact source snapshot, and private per-object import-origin
    /// receipts bound to that source directory identity. A same-content folder at another path or
    /// a replaced source directory therefore cannot claim the managed workspace.
    pub fn reopen_confirmed_folder_import_if_exact(
        &self,
        source: &Path,
        candidate_store: &Path,
        expected_summary: &str,
    ) -> Result<Option<crate::ipc::Json>, Unavailable> {
        let presented = match crate::workspace::presented_workspace_path(candidate_store) {
            Ok(presented) => presented,
            Err(_) => return Ok(None),
        };
        let confirmed = match crate::ConfirmedFolderImport::open(&presented) {
            Ok(confirmed) => confirmed,
            Err(_) => return Ok(None),
        };
        if !confirmed
            .proves_exact_origin(source, expected_summary)
            .map_err(folder_management_refusal)?
        {
            return Ok(None);
        }

        let import = confirmed.summary().to_json("folder-import-recovered");
        let receipt = confirmed.receipt().to_string_lossy().into_owned();
        let destination = confirmed.destination().to_path_buf();
        let imported_entries = confirmed.summary().file_count()
            + usize::try_from(confirmed.summary().directory_count()).unwrap_or(usize::MAX);
        let _custody =
            crate::workspace_custody::lock_workspace_path_initialization(&destination, false)
                .map_err(|_| {
                    Unavailable::new(
                        "workspace-directory-changed",
                        user_messages::WORKSPACE_DIRECTORY_CHANGED,
                    )
                })?;
        let _open_serial = self
            .workspace_open
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let workspace = self.open_workspace_after_serial(&destination)?;
        Ok(Some(crate::ipc::Json::object([
            ("import", import),
            (
                "destination",
                crate::ipc::Json::text(destination.to_string_lossy()),
            ),
            (
                "private_store",
                crate::ipc::Json::text(candidate_store.to_string_lossy()),
            ),
            ("receipt", crate::ipc::Json::text(receipt)),
            ("private_history", crate::ipc::Json::Bool(true)),
            (
                "materialized_entries",
                crate::ipc::Json::Number(u64::try_from(imported_entries).unwrap_or(u64::MAX)),
            ),
            ("shared_version_advanced", crate::ipc::Json::Bool(false)),
            ("recovered_after_interruption", crate::ipc::Json::Bool(true)),
            ("workspace", workspace.to_json()),
        ])))
    }

    /// Whether an import source is the exact zero-history workspace currently held by this
    /// daemon. Only that already-admitted identity may use the co-located private-name fence.
    fn import_source_is_open_zero_history_workspace(&self, source: &Path) -> bool {
        let Ok(source) = fs::canonicalize(source) else {
            return false;
        };
        let held = self.held();
        held.as_ref().is_some_and(|open| {
            open.boundary().records == 0
                && open.ensure_physical_root().is_ok()
                && open.physical_root().as_path() == source
        })
    }

    fn confirm_folder_import_with_protected_roots(
        &self,
        source: &str,
        destination: &str,
        expected_summary: &str,
        protected_roots: &[crate::ProtectedWorkspaceRoot],
    ) -> Result<crate::ipc::Json, Unavailable> {
        let source = Path::new(source);
        let destination = Path::new(destination);
        let source_private_fence = self.import_source_is_open_zero_history_workspace(source);
        let prepared = if source_private_fence && protected_roots.is_empty() {
            crate::PreparedFolderImport::prepare_presented_open_workspace(source, destination)
        } else if source_private_fence {
            crate::PreparedFolderImport::prepare_presented_open_workspace_outside(
                source,
                destination,
                protected_roots,
            )
        } else if protected_roots.is_empty() {
            crate::PreparedFolderImport::prepare_presented(source, destination)
        } else {
            crate::PreparedFolderImport::prepare_presented_outside(
                source,
                destination,
                protected_roots,
            )
        }
        .map_err(|error| match error {
            crate::FolderImportError::DestinationInsideProtectedRoot { .. } => Unavailable::new(
                "folder-import-destination-overlaps-remembered",
                "Choose a new folder outside every workspace and original project already remembered by Mesh. No folder was created.",
            ),
            _ => folder_management_refusal(error),
        })?;
        let found = prepared
            .summary()
            .preview_confirmation_digest(source_private_fence)
            .to_string();
        let legacy_offline_summary = prepared.summary().digest().to_string();
        let legacy_offline_match =
            !source_private_fence && legacy_offline_summary == expected_summary;
        if found != expected_summary && !legacy_offline_match {
            prepared.rollback().map_err(folder_management_refusal)?;
            return Err(Unavailable::new(
                "folder-import-preview-changed",
                format!(
                    "The folder changed since preview. Expected {expected_summary}, found {found}; the managed copy was removed."
                ),
            ));
        }
        let (confirmed, managed) = prepared
            .confirm_into_workspace()
            .map_err(folder_management_refusal)?;
        let destination = confirmed.destination().to_path_buf();
        let receipt = confirmed.receipt().to_string_lossy().into_owned();
        let summary = confirmed.summary().to_json("folder-import-confirmed");
        let workspace = self.open_workspace(&destination.to_string_lossy())?;
        Ok(crate::ipc::Json::object([
            ("import", summary),
            (
                "destination",
                crate::ipc::Json::text(destination.to_string_lossy()),
            ),
            ("receipt", crate::ipc::Json::text(receipt)),
            ("private_history", crate::ipc::Json::Bool(true)),
            (
                "operation",
                crate::ipc::Json::text(managed.operation().to_string()),
            ),
            (
                "manifests",
                crate::ipc::Json::Number(managed.manifests() as u64),
            ),
            (
                "materialized_entries",
                crate::ipc::Json::Number(managed.entries() as u64),
            ),
            (
                "linked_bytes",
                crate::ipc::Json::Number(managed.linked_bytes()),
            ),
            ("shared_version_advanced", crate::ipc::Json::Bool(false)),
            ("workspace", workspace.to_json()),
        ]))
    }

    /// Roll back only the exact managed workspace the desktop presented for confirmation.
    ///
    /// The daemon acquires workspace-native custody before `workspace_open -> managed_edit`, then
    /// rechecks root/fold/installation while all three guards are held. That order prevents either
    /// an agent acquisition or a concurrent local workspace switch from turning a confirmed
    /// rollback into deletion of a different managed copy.
    pub fn rollback_folder_import_for_workspace(
        &self,
        expected_root: &str,
        expected_digest: &str,
        expected_installation: &str,
    ) -> Result<crate::ipc::Json, Unavailable> {
        let _workspace_authority = self
            .lock_current_managed_workspace_mutation()
            .map_err(agent_custody_refusal)?;
        let _managed_serial = self
            .managed_edit
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let confirmed = crate::ConfirmedFolderImport::open(Path::new(expected_root))
            .map_err(folder_management_refusal)?;
        let destination = confirmed.destination().to_path_buf();
        let mut checkpoint = self
            .checkpoint
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut held = self.held();
        let open = held.as_ref().ok_or_else(|| {
            Unavailable::new(
                "no-workspace-open",
                "No managed workspace is open; nothing was removed.",
            )
        })?;
        if open.root().as_path() != Path::new(expected_root)
            || open.digest().to_string() != expected_digest
            || open.installation() != expected_installation
        {
            return Err(Unavailable::new(
                "stale-workspace",
                "The managed workspace changed after rollback confirmation; nothing was removed.",
            ));
        }
        if !confirmed
            .refers_to_destination(open.physical_root().as_path())
            .map_err(folder_management_refusal)?
        {
            return Err(Unavailable::new(
                "stale-workspace",
                "The confirmed import receipt does not identify the open managed workspace; nothing was removed.",
            ));
        }
        confirmed
            .rollback_after_custody()
            .map_err(folder_management_refusal)?;
        *held = None;
        checkpoint.uninstall();
        Ok(crate::ipc::Json::object([
            (
                "action",
                crate::ipc::Json::text("folder-import-rolled-back"),
            ),
            (
                "destination",
                crate::ipc::Json::text(destination.to_string_lossy()),
            ),
            ("workspace_root", crate::ipc::Json::text(expected_root)),
            ("original_preserved", crate::ipc::Json::Bool(true)),
        ]))
    }
}

impl Operations for LiveDaemon {
    fn fleet_agent_call(
        &self,
        objective: &str,
        credential: &str,
        action: &str,
        arguments: &crate::ipc::Json,
    ) -> Result<crate::ipc::Json, Unavailable> {
        let service = self
            .fleet
            .lock()
            .map_err(|_| {
                Unavailable::new("fleet-host-needs-recovery", "Fleet routing needs recovery.")
            })?
            .get(objective)
            .cloned()
            .ok_or_else(|| {
                Unavailable::new("fleet-session-refused", "The fleet session is unavailable.")
            })?;
        service.agent_call(credential, action, arguments)
    }

    fn serving(&self) -> bool {
        // The daemon serves whether or not a workspace is open: a person whose workspace is
        // damaged needs the surface that can tell them so to still be answering.
        true
    }

    fn startup(&self) -> StartupSummary {
        self.startup
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn open_workspace(&self, path: &str) -> Result<WorkspaceSummary, Unavailable> {
        let _custody =
            crate::workspace_custody::lock_workspace_path_initialization(Path::new(path), true)
                .map_err(|_| {
                    Unavailable::new(
                        "workspace-directory-changed",
                        user_messages::WORKSPACE_DIRECTORY_CHANGED,
                    )
                })?;
        let _open_serial = self
            .workspace_open
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        self.open_workspace_after_serial(Path::new(path))
    }

    fn workspace_state(&self) -> Result<WorkspaceSummary, Unavailable> {
        // Match the save path's checkpoint -> workspace order. Besides avoiding inversion, this
        // makes the state summary and its embedded support document one coherent observation.
        let checkpoint = self
            .checkpoint
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let snapshot = checkpoint
            .active
            .as_ref()
            .map(|runtime| runtime.machine().snapshot());
        let held = self.held();
        let open = held.as_ref().ok_or_else(Unavailable::no_workspace_open)?;
        open.ensure_physical_root().map_err(|_| {
            Unavailable::new(
                "workspace-directory-changed",
                user_messages::WORKSPACE_DIRECTORY_CHANGED,
            )
        })?;
        let report = snapshot.as_ref().map_or_else(
            || CrashReport::of(open),
            |snapshot| CrashReport::of_with_checkpoint(open, snapshot),
        );
        let source_identity = open.support_file_identity().map_err(|_| {
            Unavailable::new(
                "workspace-journal-changed",
                user_messages::WORKSPACE_DIRECTORY_CHANGED,
            )
        })?;
        let automatic_review_target = snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.last_meaningful())
            .map(|checkpoint| checkpoint.stamp().content_hash());
        let mut summary = summarise(open, automatic_review_target);
        summary.support_bundle = Some(
            crate::SupportBundle::from_live_report(open.root().as_path(), source_identity, &report)
                .document()
                .clone(),
        );
        Ok(summary)
    }

    fn open_current_review(&self, opened_by: &str) -> Result<WorkspaceSummary, Unavailable> {
        let opened_by = parse_digest(opened_by)?;
        let summary = self.workspace_state()?;
        self.open_current_review_for_workspace_actor(
            &summary.root,
            &summary.digest,
            &summary.installation,
            opened_by,
        )
    }

    fn performance_counters(&self) -> Result<crate::ipc::Json, Unavailable> {
        Ok(self.counter_snapshot().to_json())
    }

    fn preview_folder_import(&self, source: &str) -> Result<crate::ipc::Json, Unavailable> {
        let source = Path::new(source);
        let source_private_fence = self.import_source_is_open_zero_history_workspace(source);
        let preview = if source_private_fence {
            crate::folder_import::preview_open_workspace_import(source)
        } else {
            crate::preview_folder_import(source)
        };
        preview
            .map(|summary| summary.to_preview_json(source_private_fence))
            .map_err(folder_management_refusal)
    }

    fn confirm_folder_import(
        &self,
        source: &str,
        destination: &str,
        expected_summary: &str,
    ) -> Result<crate::ipc::Json, Unavailable> {
        self.confirm_folder_import_with_protected_roots(source, destination, expected_summary, &[])
    }

    fn confirm_folder_import_protected(
        &self,
        source: &str,
        destination: &str,
        expected_summary: &str,
        protected_root_tokens: &[String],
    ) -> Result<crate::ipc::Json, Unavailable> {
        if protected_root_tokens.len() > 32 {
            return Err(Unavailable::new(
                "import-protected-roots-invalid",
                "The protected workspace location list is invalid.",
            ));
        }
        let protected_roots = protected_root_tokens
            .iter()
            .map(|token| {
                crate::ProtectedWorkspaceRoot::from_directory_token(token).map_err(|_| {
                    Unavailable::new(
                        "import-protected-roots-invalid",
                        "The protected workspace location list is invalid.",
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.confirm_folder_import_with_protected_roots(
            source,
            destination,
            expected_summary,
            &protected_roots,
        )
    }

    fn rollback_folder_import(&self, destination: &str) -> Result<crate::ipc::Json, Unavailable> {
        let confirmed = crate::ConfirmedFolderImport::open(Path::new(destination))
            .map_err(folder_management_refusal)?;
        let _custody = crate::workspace_custody::require_unassigned_path(confirmed.destination())
            .map_err(|error| {
            agent_custody_refusal(ManagedTextFileError::Recovery(error.to_string()))
        })?;
        let _open_serial = self
            .workspace_open
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let _managed_serial = self
            .managed_edit
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let destination = confirmed.destination().to_path_buf();
        // A rollback of the held workspace is the inverse of installation: block checkpoint
        // mutations, verify and remove the receipt-owned tree, then clear both halves before
        // another open can begin. A coordinator must never remain active for a deleted database.
        let mut checkpoint = self
            .checkpoint
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut held = self.held();
        let open_root = held
            .as_ref()
            .map(|open| {
                confirmed
                    .refers_to_destination(open.physical_root().as_path())
                    .map(|matches| matches.then(|| open.physical_root().as_path().to_path_buf()))
            })
            .transpose()
            .map_err(folder_management_refusal)?
            .flatten();
        confirmed
            .rollback_after_custody()
            .map_err(folder_management_refusal)?;
        if open_root.is_some() {
            *held = None;
            checkpoint.uninstall();
        }
        let workspace_root = open_root.unwrap_or_else(|| destination.clone());
        Ok(crate::ipc::Json::object([
            (
                "action",
                crate::ipc::Json::text("folder-import-rolled-back"),
            ),
            (
                "destination",
                crate::ipc::Json::text(destination.to_string_lossy()),
            ),
            (
                "workspace_root",
                crate::ipc::Json::text(workspace_root.to_string_lossy()),
            ),
            ("original_preserved", crate::ipc::Json::Bool(true)),
        ]))
    }

    fn fork_workspace_version(
        &self,
        operation: &str,
        destination: &str,
        expected_root: &str,
        expected_digest: &str,
        expected_installation: &str,
    ) -> Result<crate::ipc::Json, Unavailable> {
        self.fork_workspace_version_with_origin(
            operation,
            destination,
            expected_root,
            expected_digest,
            expected_installation,
            None,
        )
    }

    fn preview_file_restore(
        &self,
        object: &str,
        target: &str,
    ) -> Result<crate::ipc::Json, Unavailable> {
        let object = mesh_materializer::ObjectId::parse(object).map_err(|error| {
            Unavailable::new(
                "restore-object-invalid",
                format!("The file identity is invalid: {error}"),
            )
        })?;
        let target = mesh_materializer::VersionId::parse(target).map_err(|error| {
            Unavailable::new(
                "restore-target-invalid",
                format!("The earlier version identity is invalid: {error}"),
            )
        })?;
        let held = self.held();
        let open = held.as_ref().ok_or_else(Unavailable::no_workspace_open)?;
        open.preview_file_restore(object, target)
            .map(|preview| preview.to_json())
            .map_err(|failure| Unavailable::new(failure.code(), failure.to_string()))
    }

    fn open_review(
        &self,
        bundle: &str,
        target: &str,
        opened_by: &str,
    ) -> Result<WorkspaceSummary, Unavailable> {
        let _workspace_authority = self
            .lock_current_managed_workspace_mutation()
            .map_err(agent_custody_refusal)?;
        let bundle = parse_digest(bundle)?;
        let target = parse_digest(target)?;
        let opened_by = parse_digest(opened_by)?;
        // Keep the checkpoint -> workspace order used by every workspace mutation. Opening a
        // review is itself a recovery-only trigger, so the exact pending prefix must be preserved
        // before the immutable review record is appended. A failed preservation therefore cannot
        // leave a review that claims the trigger happened without its required recovery truth.
        let mut checkpoint = self
            .checkpoint
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut held = self.held();
        let open = held.as_mut().ok_or_else(Unavailable::no_workspace_open)?;
        open.ensure_physical_root()
            .map_err(|_| publication_save_failed())?;
        if !open.has_operation(&target) {
            return Err(publication_refusal(
                "publication-target-absent",
                user_messages::PUBLICATION_TARGET_ABSENT,
            ));
        }
        let computed_bundle = open.first_publication_review_bundle(target).map_err(|_| {
            publication_refusal(
                "publication-review-not-computable",
                user_messages::PUBLICATION_REVIEW_NOT_COMPUTABLE,
            )
        })?;
        if computed_bundle != bundle {
            return Err(publication_refusal(
                "publication-review-bundle-mismatch",
                user_messages::PUBLICATION_REVIEW_BUNDLE_MISMATCH,
            ));
        }
        require_current_native_review_scope(open, target)?;
        let review_already_exists = if let Some(existing) = open.review(&bundle) {
            if existing.subject_operation != target {
                return Err(publication_refusal(
                    "publication-review-conflict",
                    user_messages::PUBLICATION_REVIEW_CONFLICT,
                ));
            }
            true
        } else {
            false
        };
        if let Err(_error) = checkpoint
            .preserve_pending_recovery_for_signal(open, RecoveryRuntimeSignal::ReviewOpened)
        {
            open.record_checkpoint_recovery_attention();
            return Err(publication_refusal(
                "publication-recovery-preservation-failed",
                user_messages::PUBLICATION_RECOVERY_PRESERVATION_FAILED,
            ));
        }
        // Reopening an immutable review is record-idempotent, but it is still a new recovery
        // trigger. New durable work may have extended the open checkpoint window since this
        // bundle was first opened, so returning before preservation would leave that newer prefix
        // unprotected even though the user explicitly opened the review again.
        if review_already_exists {
            return Ok(summarise(open, None));
        }
        open.append_record(&StoredRecord::Review(ReviewRecord {
            bundle,
            subject_operation: target,
            opened_by,
        }))
        .map_err(|_| publication_save_failed())?;
        reopen(&mut held, &self.trusted_reviewers)?;
        Ok(summarise(
            held.as_ref().expect("reopen installs workspace"),
            None,
        ))
    }

    fn approve_review(
        &self,
        bundle: &str,
        target: &str,
        receipt_hex: &str,
    ) -> Result<WorkspaceSummary, Unavailable> {
        let summary = self.workspace_state()?;
        self.approve_review_for_workspace(
            &summary.root,
            &summary.digest,
            &summary.installation,
            bundle,
            target,
            receipt_hex,
        )
    }

    fn event_cursor(&self) -> u64 {
        self.feed.latest()
    }

    fn events_since(&self, cursor: u64) -> EventBacklog {
        self.feed.since(cursor)
    }
}

fn folder_management_refusal(failure: crate::FolderImportError) -> Unavailable {
    Unavailable::new("folder-import-refused", failure.to_string())
}

fn checkpoint_stamp(sequence: RecoverySequence, changeset: RecordDigest) -> RecoveryStamp {
    let mut event = [0_u8; 16];
    event.copy_from_slice(&changeset.as_bytes()[..16]);
    RecoveryStamp::new(
        sequence.get(),
        RecoveryEventUlid::from_bytes(event),
        changeset,
    )
}

const JOURNAL_RECOVERY_POINTER_DOMAIN: &[u8] = b"mesh.journal-recovery-pointer/1\0";

fn recovery_field<const N: usize>(
    bytes: &[u8],
    cursor: &mut usize,
) -> Result<[u8; N], AutomaticCheckpointError> {
    let end = cursor.checked_add(N).ok_or_else(|| {
        AutomaticCheckpointError::RecoveryArtifactInvalid(
            "the recovery pointer length overflowed".to_owned(),
        )
    })?;
    let field = bytes.get(*cursor..end).ok_or_else(|| {
        AutomaticCheckpointError::RecoveryArtifactInvalid(
            "the recovery pointer ended before its declared fields".to_owned(),
        )
    })?;
    let mut out = [0_u8; N];
    out.copy_from_slice(field);
    *cursor = end;
    Ok(out)
}

fn verify_journal_recovery_pointer(
    open: &mut OpenWorkspace,
    pending: Option<PendingMeaningfulSave>,
    recovery: &RecoveryPreserved,
) -> Result<Option<DurableBoundary>, AutomaticCheckpointError> {
    let bytes = recovery.bytes();
    if !bytes.starts_with(JOURNAL_RECOVERY_POINTER_DOMAIN) {
        return Ok(None);
    }
    if RecordDigest::from_bytes(*Blake3::digest_bytes(bytes).as_bytes())
        != recovery.verified_content_hash()
    {
        return Err(AutomaticCheckpointError::RecoveryArtifactInvalid(
            "the recovery pointer bytes do not match their admitted digest".to_owned(),
        ));
    }
    let mut cursor = JOURNAL_RECOVERY_POINTER_DOMAIN.len();
    let view_len = u64::from_be_bytes(recovery_field(bytes, &mut cursor)?);
    let view_len = usize::try_from(view_len).map_err(|_| {
        AutomaticCheckpointError::RecoveryArtifactInvalid(
            "the recovery view length does not fit this platform".to_owned(),
        )
    })?;
    let view_end = cursor.checked_add(view_len).ok_or_else(|| {
        AutomaticCheckpointError::RecoveryArtifactInvalid(
            "the recovery view length overflowed".to_owned(),
        )
    })?;
    if bytes.get(cursor..view_end) != Some(LIVE_WORKSPACE_VIEW) {
        return Err(AutomaticCheckpointError::RecoveryArtifactInvalid(
            "the recovery pointer names a different workspace view".to_owned(),
        ));
    }
    cursor = view_end;
    let through = u64::from_be_bytes(recovery_field(bytes, &mut cursor)?);
    let lamport = u64::from_be_bytes(recovery_field(bytes, &mut cursor)?);
    let event = RecoveryEventUlid::from_bytes(recovery_field(bytes, &mut cursor)?);
    let changeset = RecordDigest::from_bytes(recovery_field(bytes, &mut cursor)?);
    let index_digest = mesh_store::Digest16::from_bytes(recovery_field(bytes, &mut cursor)?);
    let operations = u64::from_be_bytes(recovery_field(bytes, &mut cursor)?);
    let manifests = u64::from_be_bytes(recovery_field(bytes, &mut cursor)?);
    let chunks = u64::from_be_bytes(recovery_field(bytes, &mut cursor)?);
    let records = u64::from_be_bytes(recovery_field(bytes, &mut cursor)?);
    let byte_offset = u64::from_be_bytes(recovery_field(bytes, &mut cursor)?);
    if cursor != bytes.len()
        || through != recovery.through().get()
        || lamport != recovery.stamp().lamport()
        || event != recovery.stamp().event_ulid()
        || !open.has_operation(&changeset)
    {
        return Err(AutomaticCheckpointError::RecoveryArtifactInvalid(
            "the recovery pointer identity does not match durable workspace truth".to_owned(),
        ));
    }
    if let Some(pending) = pending.filter(|pending| pending.through().get() == through) {
        let (expected_operations, expected_manifests, expected_chunks) = pending.counts();
        let boundary = DurableBoundary {
            records,
            byte_offset,
        };
        if changeset != pending.stamp().content_hash()
            || index_digest != pending.index_digest()
            || u64::try_from(expected_operations).ok() != Some(operations)
            || u64::try_from(expected_manifests).ok() != Some(manifests)
            || u64::try_from(expected_chunks).ok() != Some(chunks)
            || (open.boundary() == boundary && open.digest() != index_digest)
        {
            return Err(AutomaticCheckpointError::RecoveryArtifactInvalid(
                "the recovery pointer does not match its pending durable acknowledgement"
                    .to_owned(),
            ));
        }
    }
    let current = open.boundary();
    if records > current.records || byte_offset > current.byte_offset {
        return Err(AutomaticCheckpointError::RecoveryArtifactInvalid(
            "the recovery pointer lies beyond the verified append-only journal".to_owned(),
        ));
    }
    Ok(Some(DurableBoundary {
        records,
        byte_offset,
    }))
}

/// Bind the exact durable journal prefix and pending acknowledgement into one recovery record.
///
/// The journal and CAS already own the immutable workspace bytes. Duplicating the entire growing
/// journal into SQLite every 25 ms would turn the recovery pointer into an unbounded second log.
/// This compact record instead names the verified append-only boundary, the rebuilt whole-view
/// digest, and acknowledgement counts. Normal workspace open authenticates the record frames and
/// fold before any meaningful acknowledgement can be reproduced.
fn journal_recovery_for_pending(
    open: &mut OpenWorkspace,
    pending: PendingMeaningfulSave,
) -> Result<RecoveryPreserved, AutomaticCheckpointError> {
    let boundary = open.boundary();
    if open.digest() != pending.index_digest() {
        return Err(AutomaticCheckpointError::RecoveryArtifactInvalid(
            "the pending acknowledgement does not match the verified workspace fold".to_owned(),
        ));
    }
    let (operations, manifests, chunks) = pending.counts();
    let operations = u64::try_from(operations).map_err(|_| {
        AutomaticCheckpointError::RecoveryArtifactInvalid(
            "the operation count does not fit the canonical recovery record".to_owned(),
        )
    })?;
    let manifests = u64::try_from(manifests).map_err(|_| {
        AutomaticCheckpointError::RecoveryArtifactInvalid(
            "the manifest count does not fit the canonical recovery record".to_owned(),
        )
    })?;
    let chunks = u64::try_from(chunks).map_err(|_| {
        AutomaticCheckpointError::RecoveryArtifactInvalid(
            "the chunk count does not fit the canonical recovery record".to_owned(),
        )
    })?;

    let stamp = pending.stamp();
    let mut record = Vec::with_capacity(192);
    record.extend_from_slice(JOURNAL_RECOVERY_POINTER_DOMAIN);
    record.extend_from_slice(&(LIVE_WORKSPACE_VIEW.len() as u64).to_be_bytes());
    record.extend_from_slice(LIVE_WORKSPACE_VIEW);
    record.extend_from_slice(&pending.through().get().to_be_bytes());
    record.extend_from_slice(&stamp.lamport().to_be_bytes());
    record.extend_from_slice(stamp.event_ulid().as_bytes());
    record.extend_from_slice(stamp.content_hash().as_bytes());
    record.extend_from_slice(pending.index_digest().as_bytes());
    record.extend_from_slice(&operations.to_be_bytes());
    record.extend_from_slice(&manifests.to_be_bytes());
    record.extend_from_slice(&chunks.to_be_bytes());
    record.extend_from_slice(&boundary.records.to_be_bytes());
    record.extend_from_slice(&boundary.byte_offset.to_be_bytes());

    let digest = RecordDigest::from_bytes(*Blake3::digest_bytes(&record).as_bytes());
    RecoveryPreserved::from_verified_bytes(
        RecoveryStamp::new(stamp.lamport(), stamp.event_ulid(), digest),
        pending.through(),
        record,
        digest,
    )
    .map_err(|error| AutomaticCheckpointError::RecoveryArtifactInvalid(error.to_string()))
}

fn preserve_pending_recovery_if_due(
    runtime: &mut AutomaticCheckpointRuntime,
    open: &mut OpenWorkspace,
    elapsed: Duration,
) -> Result<Option<RecoveryTransition>, AutomaticCheckpointError> {
    if !runtime.recovery_due(elapsed) {
        return Ok(None);
    }
    let Some(pending) = runtime.machine().snapshot().pending_meaningful() else {
        return Ok(None);
    };
    let recovery = journal_recovery_for_pending(open, pending)?;
    runtime
        .preserve_at_maximum(elapsed, recovery)
        .map_err(AutomaticCheckpointError::Runtime)
}

fn parse_digest(text: &str) -> Result<RecordDigest, Unavailable> {
    RecordDigest::parse_hex(text).map_err(|_| {
        publication_refusal(
            "publication-identifier-invalid",
            user_messages::PUBLICATION_IDENTIFIER_INVALID,
        )
    })
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let digit = |byte| match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                b'A'..=b'F' => Some(byte - b'A' + 10),
                _ => None,
            };
            Some((digit(pair[0])? << 4) | digit(pair[1])?)
        })
        .collect()
}

fn publication_refusal(code: &str, message: &str) -> Unavailable {
    Unavailable::new(code, message)
}

fn publication_save_failed() -> Unavailable {
    publication_refusal(
        "publication-save-failed",
        user_messages::PUBLICATION_SAVE_FAILED,
    )
}

fn reopen(held: &mut Option<OpenWorkspace>, trusted: &TrustedReviewers) -> Result<(), Unavailable> {
    held.as_mut()
        .expect("reopen requires an open workspace")
        .refresh_with_trusted_reviewers(trusted)
        .map_err(|_| publication_save_failed())
}

/// The wire summary of one open workspace.
///
/// A free function rather than a method on `OpenWorkspace` so that the storage type does not have
/// to know what the socket's field names are: `workspace.rs` answers about records, this file
/// answers about a message.
#[allow(clippy::too_many_arguments)]
fn verify_native_move_state(
    root: &Path,
    from_path: &str,
    to_path: &str,
    expected_source_parent: ManagedDirectoryIdentity,
    expected_destination: &ManagedReplacementTarget,
    expected_destination_digest: RecordDigest,
    expected_destination_executable: bool,
) -> Result<(), ManagedTextFileError> {
    let missing = confined_free_path(root, from_path)?;
    let source_parent = missing.parent().ok_or(ManagedTextFileError::InvalidPath)?;
    if managed_directory_identity(source_parent).map_err(|error| {
        ManagedTextFileError::io("recheck missing move source parent", source_parent, error)
    })? != expected_source_parent
    {
        return Err(ManagedTextFileError::StaleInspection);
    }
    let (destination, bytes) = read_managed_replacement(root, to_path)?;
    let actual_digest = RecordDigest::from_bytes(*Blake3::digest_bytes(&bytes).as_bytes());
    if !expected_destination.same_file_as(&destination)
        || actual_digest != expected_destination_digest
        || destination.executable() != expected_destination_executable
    {
        return Err(ManagedTextFileError::StaleInspection);
    }
    Ok(())
}

fn validate_independent_export_root(
    managed_root: &Path,
    storage_root: &Path,
    target_root: &Path,
) -> Result<(), ManagedTextFileError> {
    if !target_root.is_absolute() {
        return Err(ManagedTextFileError::UnsafeExportTarget);
    }
    let metadata = fs::symlink_metadata(target_root)
        .map_err(|error| ManagedTextFileError::io("inspect export folder", target_root, error))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(ManagedTextFileError::UnsafeExportTarget);
    }
    let target = fs::canonicalize(target_root)
        .map_err(|error| ManagedTextFileError::io("resolve export folder", target_root, error))?;
    for current in [managed_root, storage_root] {
        let current = fs::canonicalize(current)
            .map_err(|error| ManagedTextFileError::io("resolve managed folder", current, error))?;
        if current == target || current.starts_with(&target) || target.starts_with(&current) {
            return Err(ManagedTextFileError::UnsafeExportTarget);
        }
    }
    Ok(())
}

fn summarise(
    open: &OpenWorkspace,
    automatic_review_target: Option<RecordDigest>,
) -> WorkspaceSummary {
    let (review_items, review_items_not_listed) = open.review_items(automatic_review_target);
    let mut conditions = open.conditions().to_vec();
    let mut native_unsupported_entries = Vec::new();
    let mut native_inventory_complete = false;
    let native_untracked_files = if open.boundary().records == 0 {
        match open.native_discovery() {
            Ok(discovery) => {
                native_inventory_complete = discovery.complete;
                native_unsupported_entries = discovery.unsupported;
            }
            Err(_) => conditions.push(WorkspaceCondition::exclusion_rules_unavailable()),
        }
        match open.has_unversioned_native_content() {
            Ok(true) => conditions.push(WorkspaceCondition::unversioned_native_content()),
            Ok(false) => {}
            Err(_) => {
                if !conditions
                    .iter()
                    .any(|condition| condition.code() == "exclusion-rules-unavailable")
                {
                    conditions.push(WorkspaceCondition::exclusion_rules_unavailable());
                }
            }
        }
        Vec::new()
    } else {
        match open.native_discovery() {
            Ok(discovery) => {
                native_inventory_complete = discovery.complete;
                native_unsupported_entries = discovery.unsupported;
                discovery.files
            }
            Err(_) => {
                conditions.push(WorkspaceCondition::exclusion_rules_unavailable());
                Vec::new()
            }
        }
    };
    WorkspaceSummary {
        root: open.root().as_path().display().to_string(),
        installation: open.installation(),
        records: open.boundary().records,
        unfinished_bytes: open.tail().discarded_bytes(),
        operations: open.operations() as u64,
        actors: open.actors() as u64,
        manifests: open.manifests() as u64,
        peers: open.peers() as u64,
        reviews: open.reviews() as u64,
        review_items,
        review_items_not_listed,
        digest: open.digest().to_string(),
        private_version: open.private_version().clone(),
        shared_version: open.shared_version().map(|head| head.to_string()),
        entries: open.entries().to_vec(),
        native_untracked_files,
        native_unsupported_entries,
        native_inventory_complete,
        file_histories: open.file_histories().to_vec(),
        workspace_versions: open.workspace_versions(),
        conditions,
        not_yet: open
            .not_yet()
            .iter()
            .map(|(subject, reason)| ((*subject).to_owned(), (*reason).to_owned()))
            .collect(),
        support_bundle: None,
    }
}

/// The sentence a person reads for each way an open can fail.
///
/// The failure's own `Display` is never sent: it carries a byte offset, an ordinal and a driver
/// string, which belong in a diagnostic and not in a window. The machine code is what a client
/// branches on, and it comes from [`OpenFailure::code`] so the two cannot drift.
fn refusal_for(failure: &OpenFailure) -> Unavailable {
    let sentence = match failure {
        OpenFailure::Unreachable(_) => user_messages::WORKSPACE_UNREACHABLE,
        OpenFailure::PayloadStore(_) => user_messages::PAYLOAD_STORE_UNREACHABLE,
        OpenFailure::Index { .. } => user_messages::WORKSPACE_INDEX_UNAVAILABLE,
        OpenFailure::Damaged(_) => user_messages::WORKSPACE_DAMAGED,
        OpenFailure::NothingReadable { .. } => user_messages::WORKSPACE_NOTHING_READABLE,
        OpenFailure::Contradictory { .. } => user_messages::WORKSPACE_CONTRADICTORY,
    };
    Unavailable::new(failure.code(), sentence)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::ipc::surface::nothing_to_recover;
    use crate::recovery::RecoveryDiagnostic;
    use crate::workspace::{RecordFile, RECORD_FILE_NAME};
    use ed25519_dalek::SigningKey;
    use mesh_store::{
        journal_records, no_session, OperationRecord, RecoverySnapshot,
        RecoveryStatePersistence as _, SqlExecutor as _, Sqlite, RECOVERY_DATABASE_FILE_NAME,
    };

    #[test]
    fn a_newer_os_change_during_move_settling_stays_working() {
        use ed25519_dalek::Signer as _;
        use std::cell::Cell;
        use std::rc::Rc;

        let parent = scratch("move-settling");
        let source = parent.join("source");
        let managed = parent.join("managed");
        fs::create_dir_all(source.join("existing")).unwrap();
        fs::write(source.join("existing/keep.txt"), "keep\n").unwrap();
        crate::PreparedFolderImport::prepare(&source, &managed)
            .unwrap()
            .confirm_into_workspace()
            .unwrap();
        let daemon = LiveDaemon::with_checkpoint_runtime(
            started(),
            CheckpointRuntimeParameters {
                idle_interval: Some(Duration::from_millis(50)),
                maximum_uncheckpointed_bytes: Some(65_536),
                maximum_uncheckpointed_interval: Some(Duration::from_millis(25)),
            },
        )
        .unwrap();
        daemon.open_at_start(&managed).unwrap();
        let key = SigningKey::from_bytes(&[0x63; 32]);
        let public = PublicKey::from_bytes(key.verifying_key().to_bytes());
        let sign = |payload: &SigningPayload| -> Result<Signature, std::convert::Infallible> {
            Ok(Signature::from_bytes(
                key.sign(payload.as_bytes()).to_bytes(),
            ))
        };
        daemon
            .create_managed_text_file("draft.txt", "durable bytes\n", public, sign)
            .unwrap();
        let target = managed.join("final.txt");
        let displaced = managed.join("externally-moved.txt");
        let called = Rc::new(Cell::new(false));
        AFTER_MANAGED_MOVE_PERSIST.with(|hook| {
            let target = target.clone();
            let displaced = displaced.clone();
            let managed = managed.clone();
            let called = Rc::clone(&called);
            *hook.borrow_mut() = Some(Box::new(move || {
                assert!(!managed.join("draft.txt").exists());
                assert!(!managed.join(".mesh-managed-mutation").exists());
                assert_eq!(fs::read(&target).unwrap(), b"durable bytes\n");
                fs::rename(&target, &displaced).expect("external move");
                fs::create_dir(&target).expect("newer external folder");
                called.set(true);
            }));
        });
        let moved = daemon
            .move_managed_entry_privately("draft.txt", "final.txt", public, sign)
            .unwrap();
        assert!(
            called.get(),
            "the replacement must precede the settling check"
        );
        assert_eq!(
            daemon.durable_operation(RecordDigest::parse_hex(moved.changeset()).unwrap()),
            Some(true)
        );
        assert!(!moved.meaningful_saved());
        assert!(target.is_dir());
        assert_eq!(fs::read(displaced).unwrap(), b"durable bytes\n");
        assert!(daemon
            .checkpoint_snapshot()
            .unwrap()
            .open_window()
            .is_some());
        assert!(
            daemon.workspace_state().unwrap().review_items.is_empty(),
            "a newer unsettled operating-system change must suppress the stale automatic card"
        );
        drop(daemon);
        fs::remove_dir_all(parent).unwrap();
    }

    fn started() -> StartupSummary {
        StartupSummary::from(&nothing_to_recover())
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("mesh-live-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    #[test]
    fn daemon_drop_waits_for_checkpoint_database_owners_even_when_worker_unwinds() {
        use mesh_operations as op;
        for panic_after_release in [false, true] {
            let root = scratch(if panic_after_release {
                "shutdown-unwind"
            } else {
                "shutdown-worker"
            });
            let managed = root.join("managed");
            let daemon = LiveDaemon::with_checkpoint_runtime(
                started(),
                CheckpointRuntimeParameters {
                    idle_interval: Some(Duration::from_secs(60)),
                    maximum_uncheckpointed_bytes: Some(65_536),
                    maximum_uncheckpointed_interval: Some(Duration::from_secs(30)),
                },
            )
            .unwrap();
            daemon.open_at_start(&managed).unwrap();
            let (entered_tx, entered_rx) = std::sync::mpsc::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            *daemon.checkpoint_idle.worker_gate.lock().unwrap() = Some(CheckpointWorkerTestGate {
                entered: entered_tx,
                release: release_rx,
                panic_after_release,
            });
            let request = FileVersionCheckpointRequest::new(
                op::WorkspaceId::from_bytes([1; 16]),
                op::ActorId::from_bytes([2; 32]),
                op::SessionId::from_bytes([3; 16]),
                op::ActorSequence::new(1),
                op::CausalParents::genesis(),
                op::HeadId::from_bytes([4; 32]),
                op::PolicyEpoch::new(5),
                op::Hlc::new(1_700_000_000_001, 6),
                op::ObjectId::from_bytes([7; 16]),
                op::VersionId::from_bytes([8; 32]),
                Vec::new(),
                PortableMetadata::new(true),
                op::Signature::from_bytes([10; 64]),
            );
            daemon
                .save_file_version(
                    RecoverySequence::new(1).unwrap(),
                    b"durable saved work",
                    &ChunkingConfig::default(),
                    crate::ManifestPagingPolicy::flat(),
                    request,
                    &LocalChangesetHead,
                )
                .unwrap();
            entered_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("real worker owns database references");
            let checkpoint = Arc::downgrade(&daemon.checkpoint);
            let open = Arc::downgrade(&daemon.open);
            let idle = Arc::clone(&daemon.checkpoint_idle);
            let (finished_tx, finished_rx) = std::sync::mpsc::channel();
            let dropping = std::thread::spawn(move || {
                drop(daemon);
                finished_tx.send(()).unwrap();
            });
            {
                let state = idle.state.lock().unwrap();
                let (state, _) = idle
                    .wake
                    .wait_timeout_while(state, Duration::from_secs(5), |state| !state.shutdown)
                    .unwrap();
                assert!(state.shutdown, "drop must request shutdown");
            }
            // The worker is explicitly parked while owning both databases. Always release it
            // before asserting so a regression failure cannot strand the drop thread.
            let returned_while_owned = finished_rx.recv_timeout(Duration::from_millis(100)).is_ok();
            assert!(checkpoint.upgrade().is_some());
            assert!(open.upgrade().is_some());
            release_tx.send(()).unwrap();
            dropping.join().unwrap();
            assert!(
                !returned_while_owned,
                "daemon drop returned before its worker released database owners"
            );
            assert!(checkpoint.upgrade().is_none());
            assert!(open.upgrade().is_none());
            let reopened =
                LiveDaemon::with_checkpoint_runtime(started(), checkpoint_parameters()).unwrap();
            reopened
                .open_at_start(&managed)
                .expect("immediate same-path restart");
            assert_eq!(reopened.workspace_state().unwrap().records, 2);
            drop(reopened);
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn worker_wait_uses_the_exact_remaining_maximum_without_restarting_it() {
        let started = Instant::now();
        let mut scheduler = IdleCheckpointSchedulerState::default();
        scheduler.publish_at(
            IdleCheckpointSchedule {
                through: RecoverySequence::new(1).unwrap(),
                installation: CheckpointInstallation(1),
                root: PathBuf::from("/workspace"),
            },
            started,
        );
        for maximum_ms in [20, 40] {
            let intervals = PendingCheckpointIntervals {
                idle: Duration::from_secs(2),
                maximum: Duration::from_millis(maximum_ms),
            };
            for (elapsed, remaining) in [
                (0, maximum_ms),
                (maximum_ms - 1, 1),
                (maximum_ms, 0),
                (maximum_ms + 100, 0),
            ] {
                assert_eq!(
                    scheduler.next_wait_at(intervals, started + Duration::from_millis(elapsed)),
                    (Duration::from_millis(remaining), true)
                );
            }
        }
    }

    #[test]
    fn worker_selects_idle_only_while_it_precedes_the_remaining_maximum() {
        let started = Instant::now();
        let mut scheduler = IdleCheckpointSchedulerState::default();
        scheduler.publish_at(
            IdleCheckpointSchedule {
                through: RecoverySequence::new(1).unwrap(),
                installation: CheckpointInstallation(1),
                root: PathBuf::from("/workspace"),
            },
            started,
        );
        let intervals = PendingCheckpointIntervals {
            idle: Duration::from_millis(20),
            maximum: Duration::from_millis(200),
        };
        assert_eq!(
            scheduler.next_wait_at(intervals, started),
            (Duration::from_millis(20), false)
        );
        assert_eq!(
            scheduler.next_wait_at(intervals, started + Duration::from_millis(180)),
            (Duration::from_millis(20), true)
        );
        assert_eq!(
            scheduler.next_wait_at(intervals, started + Duration::from_millis(190)),
            (Duration::from_millis(10), true)
        );
    }

    #[test]
    fn an_unpublished_workspace_replacement_retires_the_stale_idle_worker() {
        let stale = IdleCheckpointSchedule {
            through: RecoverySequence::new(1).expect("sequence"),
            installation: CheckpointInstallation(1),
            root: PathBuf::from("/stale/workspace"),
        };
        let replacement = IdleCheckpointSchedule {
            through: RecoverySequence::new(1).expect("sequence"),
            installation: CheckpointInstallation(2),
            root: PathBuf::from("/replacement/workspace"),
        };
        let mut scheduler = IdleCheckpointSchedulerState::default();
        assert_eq!(scheduler.publish(stale), IdleScheduleUpdate::StartWorker);
        let generation = scheduler.generation;

        assert!(scheduler.retire_unpublished_replacement(generation, &replacement));
        assert!(!scheduler.worker_running);
        assert!(scheduler.scheduled.is_none());
        assert!(scheduler.maximum_started_at.is_none());
    }

    #[test]
    fn later_activity_resets_idle_generation_without_postponing_the_maximum_deadline() {
        let first = IdleCheckpointSchedule {
            through: RecoverySequence::new(1).expect("sequence"),
            installation: CheckpointInstallation(1),
            root: PathBuf::from("/workspace"),
        };
        let second = IdleCheckpointSchedule {
            through: RecoverySequence::new(2).expect("sequence"),
            installation: CheckpointInstallation(1),
            root: PathBuf::from("/workspace"),
        };
        let started = Instant::now();
        let later = started + Duration::from_millis(500);
        let observed = started + Duration::from_millis(800);
        let mut scheduler = IdleCheckpointSchedulerState::default();

        assert_eq!(
            scheduler.publish_at(first, started),
            IdleScheduleUpdate::StartWorker
        );
        assert_eq!(
            scheduler.publish_at(second, later),
            IdleScheduleUpdate::WakeWorker
        );
        assert_eq!(scheduler.maximum_started_at, Some(started));
        assert_eq!(
            scheduler.maximum_remaining_at(Duration::from_secs(1), observed),
            Duration::from_millis(200)
        );
    }

    #[test]
    fn same_workspace_open_waits_before_workspace_open_instead_of_inverting_custody() {
        let root = scratch("open-custody-order");
        std::fs::create_dir_all(&root).expect("workspace");
        let daemon = Arc::new(LiveDaemon::new(started()));
        daemon.open_at_start(&root).expect("initial open");

        let (held_tx, held_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let mutating = Arc::clone(&daemon);
        let mutation = std::thread::spawn(move || {
            let authority = mutating
                .lock_current_managed_workspace_mutation()
                .expect("mutation authority");
            held_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            drop(authority);
        });
        held_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("mutation holds custody before workspace_open");

        let opening = Arc::clone(&daemon);
        let root_text = root.display().to_string();
        let (opened_tx, opened_rx) = std::sync::mpsc::channel();
        let reopen = std::thread::spawn(move || {
            opened_tx
                .send(Operations::open_workspace(&*opening, &root_text))
                .unwrap();
        });
        assert!(
            opened_rx.recv_timeout(Duration::from_millis(30)).is_err(),
            "same-workspace open must wait on physical custody before process-local open state"
        );
        release_tx.send(()).unwrap();
        opened_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("open completes after mutation releases")
            .expect("same workspace reopens without deadlock");
        mutation.join().unwrap();
        reopen.join().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn saved_version_reopen_and_ordinary_reopen_take_candidate_directory_before_open_serial() {
        let root = scratch("saved-version-reopen-order");
        let source = root.join("source");
        let managed = root.join("managed.mesh");
        let candidate_store = root.join("candidate.mesh");
        std::fs::create_dir_all(&source).expect("source folder");
        std::fs::write(source.join("note.txt"), b"saved version\n").expect("source file");
        let daemon = Arc::new(
            LiveDaemon::with_checkpoint_runtime(started(), checkpoint_parameters())
                .expect("checkpoint configuration"),
        );
        let preview = daemon
            .preview_folder_import(&source.display().to_string())
            .expect("preview import");
        let preview_digest = preview
            .get("summary")
            .and_then(crate::ipc::Json::as_text)
            .expect("preview digest");
        daemon
            .confirm_folder_import(
                &source.display().to_string(),
                &managed.display().to_string(),
                preview_digest,
            )
            .expect("confirm import");
        let source_state = daemon.workspace_state().expect("source state");
        let operation = source_state.workspace_versions[0].operation().to_string();
        daemon
            .fork_workspace_version(
                &operation,
                &candidate_store.display().to_string(),
                &source_state.root,
                &source_state.digest,
                &source_state.installation,
            )
            .expect("create reusable candidate");
        daemon
            .reopen_existing_workspace(Path::new(&source_state.root))
            .expect("return to source");
        let source_state = daemon.workspace_state().expect("reopened source state");
        let presented = candidate_store.join(crate::workspace::PRESENTED_DIRECTORY_NAME);

        let (candidate_locked_tx, candidate_locked_rx) = std::sync::mpsc::channel();
        let (release_candidate_tx, release_candidate_rx) = std::sync::mpsc::channel();
        let ordinary_daemon = Arc::clone(&daemon);
        let ordinary_presented = presented.clone();
        let (ordinary_tx, ordinary_rx) = std::sync::mpsc::channel();
        let ordinary = std::thread::spawn(move || {
            AFTER_REOPEN_DIRECTORY_LOCK.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(move || {
                    candidate_locked_tx.send(()).unwrap();
                    release_candidate_rx.recv().unwrap();
                }));
            });
            ordinary_tx
                .send(ordinary_daemon.reopen_existing_workspace(&ordinary_presented))
                .unwrap();
        });
        candidate_locked_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("ordinary reopen holds candidate directory before workspace_open");

        let saved_daemon = Arc::clone(&daemon);
        let saved_candidate_store = candidate_store.clone();
        let saved_operation = operation.clone();
        let saved_root = source_state.root.clone();
        let saved_digest = source_state.digest.clone();
        let saved_installation = source_state.installation.clone();
        let (saved_tx, saved_rx) = std::sync::mpsc::channel();
        let saved = std::thread::spawn(move || {
            saved_tx
                .send(saved_daemon.reopen_workspace_version_if_exact(
                    &saved_operation,
                    &saved_candidate_store,
                    &saved_root,
                    &saved_digest,
                    &saved_installation,
                    None,
                ))
                .unwrap();
        });
        assert!(
            saved_rx.recv_timeout(Duration::from_millis(30)).is_err(),
            "saved-version reuse must wait on candidate custody before process-local open state"
        );
        release_candidate_tx.send(()).unwrap();
        ordinary_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("ordinary reopen completes")
            .expect("ordinary candidate open succeeds");
        let refusal = saved_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("saved-version reuse completes without an inverted-lock deadlock")
            .expect_err("the competing open changes the exact source");
        assert_eq!(refusal.code, "workspace-version-source-changed");
        ordinary.join().unwrap();
        saved.join().unwrap();

        daemon
            .reopen_existing_workspace(Path::new(&source_state.root))
            .expect("return to source for create-only race");
        let source_state = daemon.workspace_state().expect("source state for fork");
        let racing_store = root.join("racing-candidate.mesh");
        let racing_presented = racing_store.join(crate::workspace::PRESENTED_DIRECTORY_NAME);
        let (confirmed_tx, confirmed_rx) = std::sync::mpsc::channel();
        let (release_fork_tx, release_fork_rx) = std::sync::mpsc::channel();
        let fork_daemon = Arc::clone(&daemon);
        let fork_store = racing_store.clone();
        let fork_operation = operation.clone();
        let fork_root = source_state.root.clone();
        let fork_digest = source_state.digest.clone();
        let fork_installation = source_state.installation.clone();
        let (fork_tx, fork_rx) = std::sync::mpsc::channel();
        let fork = std::thread::spawn(move || {
            AFTER_VERSION_FORK_CONFIRM.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(move || {
                    confirmed_tx.send(()).unwrap();
                    release_fork_rx.recv().unwrap();
                }));
            });
            fork_tx
                .send(fork_daemon.fork_workspace_version(
                    &fork_operation,
                    &fork_store.display().to_string(),
                    &fork_root,
                    &fork_digest,
                    &fork_installation,
                ))
                .unwrap();
        });
        confirmed_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("create-only fork confirms before daemon installation");

        let (racing_locked_tx, racing_locked_rx) = std::sync::mpsc::channel();
        let (release_racing_tx, release_racing_rx) = std::sync::mpsc::channel();
        let racing_daemon = Arc::clone(&daemon);
        let ordinary_racing_presented = racing_presented.clone();
        let (racing_tx, racing_rx) = std::sync::mpsc::channel();
        let racing = std::thread::spawn(move || {
            AFTER_REOPEN_DIRECTORY_LOCK.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(move || {
                    racing_locked_tx.send(()).unwrap();
                    release_racing_rx.recv().unwrap();
                }));
            });
            racing_tx
                .send(racing_daemon.reopen_existing_workspace(&ordinary_racing_presented))
                .unwrap();
        });
        racing_locked_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("ordinary reopen locks newly confirmed candidate");
        release_fork_tx.send(()).unwrap();
        assert!(
            fork_rx.recv_timeout(Duration::from_millis(30)).is_err(),
            "create-only fork must wait on destination custody without holding workspace_open"
        );
        release_racing_tx.send(()).unwrap();
        racing_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("ordinary open of confirmed candidate completes")
            .expect("ordinary open succeeds");
        let refusal = fork_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("fork completes without an inverted-lock deadlock")
            .expect_err("competing destination open invalidates the source");
        assert_eq!(refusal.code, "workspace-version-source-changed");
        assert!(
            racing_presented.is_dir(),
            "ambiguous post-confirm failure must preserve the exact created candidate"
        );
        racing.join().unwrap();
        fork.join().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn saved_version_fork_refuses_checkpoint_activity_committed_after_confirmation() {
        let root = scratch("saved-version-post-confirm-recovery");
        let source = root.join("source");
        let managed = root.join("managed.mesh");
        let candidate_store = root.join("candidate.mesh");
        let presented = candidate_store.join(crate::workspace::PRESENTED_DIRECTORY_NAME);
        std::fs::create_dir_all(&source).expect("source folder");
        std::fs::write(source.join("note.txt"), b"saved version\n").expect("source file");
        let daemon = Arc::new(
            LiveDaemon::with_checkpoint_runtime(started(), checkpoint_parameters())
                .expect("checkpoint configuration"),
        );
        let preview = daemon
            .preview_folder_import(&source.display().to_string())
            .expect("preview import");
        let preview_digest = preview
            .get("summary")
            .and_then(crate::ipc::Json::as_text)
            .expect("preview digest");
        daemon
            .confirm_folder_import(
                &source.display().to_string(),
                &managed.display().to_string(),
                preview_digest,
            )
            .expect("confirm import");
        let source_state = daemon.workspace_state().expect("source state");
        let operation = source_state.workspace_versions[0].operation().to_string();
        let expected_root = source_state.root.clone();
        let expected_digest = source_state.digest.clone();
        let expected_installation = source_state.installation.clone();

        let (confirmed_tx, confirmed_rx) = std::sync::mpsc::channel();
        let (resume_tx, resume_rx) = std::sync::mpsc::channel();
        let fork_daemon = Arc::clone(&daemon);
        let fork_store = candidate_store.clone();
        let fork_root = expected_root.clone();
        let fork_digest = expected_digest.clone();
        let fork_installation = expected_installation.clone();
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        let fork = std::thread::spawn(move || {
            AFTER_VERSION_FORK_CONFIRM.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(move || {
                    confirmed_tx.send(()).unwrap();
                    resume_rx.recv().unwrap();
                }));
            });
            result_tx
                .send(fork_daemon.fork_workspace_version(
                    &operation,
                    &fork_store.display().to_string(),
                    &fork_root,
                    &fork_digest,
                    &fork_installation,
                ))
                .unwrap();
        });
        confirmed_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("fork confirms candidate before installation");

        let second = LiveDaemon::with_checkpoint_runtime(started(), checkpoint_parameters())
            .expect("second checkpoint configuration");
        second
            .reopen_at_start(&presented)
            .expect("second daemon opens confirmed candidate");
        let sequence = RecoverySequence::new(1).expect("sequence one");
        second
            .observe_checkpoint_activity(sequence, 1)
            .expect("second daemon persists checkpoint activity");
        let recovery_bytes = b"concurrent candidate recovery".to_vec();
        let recovery_digest =
            RecordDigest::from_bytes(*Blake3::digest_bytes(&recovery_bytes).as_bytes());
        second
            .preserve_recovery(
                RecoveryTrigger::ActorDisconnected,
                RecoveryPreserved::from_verified_bytes(
                    RecoveryStamp::new(
                        1,
                        RecoveryEventUlid::from_bytes([0x51; 16]),
                        recovery_digest,
                    ),
                    sequence,
                    recovery_bytes,
                    recovery_digest,
                )
                .expect("verified candidate recovery"),
            )
            .expect("persist observed candidate recovery");
        drop(second);
        resume_tx.send(()).unwrap();

        let refusal = result_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("fork resumes after checkpoint writer exits")
            .expect_err("non-default destination recovery must refuse installation");
        assert_eq!(refusal.code, "workspace-version-destination-refused");
        let current = daemon.workspace_state().expect("source stays current");
        assert_eq!(current.root, expected_root);
        assert_eq!(current.digest, expected_digest);
        assert_eq!(current.installation, expected_installation);
        assert!(
            presented.is_dir(),
            "a refused post-confirm candidate remains available for explicit recovery"
        );
        fork.join().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn verified_mutation_context_cannot_cross_live_daemon_instances() {
        let root = scratch("verified-context-instance");
        std::fs::create_dir_all(&root).expect("workspace");
        let first = LiveDaemon::new(started());
        let second = LiveDaemon::new(started());
        let summary = first.open_at_start(&root).expect("first open");
        second.open_at_start(&root).expect("second open");
        let refused = first.with_verified_managed_workspace(
            &summary.root,
            &summary.digest,
            &summary.installation,
            || second.lock_current_managed_workspace_mutation().map(drop),
        );
        assert!(matches!(refused, Err(ManagedTextFileError::StaleWorkspace)));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn stable_agent_reference_keeps_the_admitted_directory_after_replacement() {
        let root = scratch("stable-agent-reference");
        std::fs::create_dir_all(&root).expect("workspace");
        std::fs::write(root.join("admitted.txt"), b"admitted\n").expect("admitted file");
        let daemon = LiveDaemon::new(started());
        let summary = daemon.open_at_start(&root).expect("open workspace");
        let verified = daemon
            .verified_managed_workspace_path(&summary.root, &summary.digest, &summary.installation)
            .expect("verified workspace");
        let reference = verified.stable_agent_reference().expect("stable reference");
        let displaced = root.with_extension("displaced");
        std::fs::rename(&root, &displaced).expect("displace workspace");
        std::fs::create_dir(&root).expect("replacement workspace");
        std::fs::write(root.join("replacement.txt"), b"replacement\n").expect("replacement file");

        assert_eq!(
            std::fs::read(reference.join("admitted.txt")).expect("read admitted through reference"),
            b"admitted\n"
        );
        assert!(!reference.join("replacement.txt").exists());
        assert!(verified.ensure_current().is_err());
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(displaced).unwrap();
    }

    #[test]
    fn verified_workspace_entry_refuses_traversal_links_and_kind_mismatches() {
        use std::os::unix::fs::symlink;

        let root = scratch("verified-workspace-entry");
        let outside = scratch("verified-workspace-entry-outside");
        std::fs::create_dir_all(root.join("nested")).expect("workspace tree");
        std::fs::create_dir_all(&outside).expect("outside tree");
        std::fs::write(root.join("nested/report.txt"), b"report\n").expect("workspace file");
        std::fs::write(outside.join("secret.txt"), b"secret\n").expect("outside file");
        symlink(outside.join("secret.txt"), root.join("linked.txt")).expect("linked entry");
        let daemon = LiveDaemon::new(started());
        let summary = daemon.open_at_start(&root).expect("open workspace");

        let file = daemon
            .verified_managed_workspace_entry(
                &summary.root,
                &summary.digest,
                &summary.installation,
                None,
                "nested/report.txt",
                false,
            )
            .expect("regular file");
        assert_eq!(
            file.path(),
            std::fs::canonicalize(&root)
                .expect("canonical workspace")
                .join("nested/report.txt")
        );
        assert!(!file.is_directory());
        let folder = daemon
            .verified_managed_workspace_entry(
                &summary.root,
                &summary.digest,
                &summary.installation,
                None,
                "nested",
                true,
            )
            .expect("regular folder");
        assert!(folder.is_directory());
        assert!(daemon
            .verified_managed_workspace_entry(
                &summary.root,
                &summary.digest,
                &summary.installation,
                None,
                "../verified-workspace-entry-outside/secret.txt",
                false,
            )
            .is_err());
        assert!(daemon
            .verified_managed_workspace_entry(
                &summary.root,
                &summary.digest,
                &summary.installation,
                None,
                "linked.txt",
                false,
            )
            .is_err());
        assert!(daemon
            .verified_managed_workspace_entry(
                &summary.root,
                &summary.digest,
                &summary.installation,
                None,
                "nested/report.txt",
                true,
            )
            .is_err());

        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn exact_agent_setup_blocks_release_and_refuses_stale_generation() {
        let root = scratch("exact-agent-setup");
        std::fs::create_dir_all(&root).expect("workspace");
        let first = Arc::new(LiveDaemon::new(started()));
        let second = Arc::new(LiveDaemon::new(started()));
        let summary = first.open_at_start(&root).expect("first open");
        second.open_at_start(&root).expect("second open");
        let generation = first
            .acquire_workspace_agent_custody(
                &summary.root,
                &summary.digest,
                &summary.installation,
                false,
                None,
            )
            .expect("acquire exact agent custody");

        assert!(
            first
                .lock_workspace_agent_setup(
                    &summary.root,
                    &summary.digest,
                    &summary.installation,
                    "ffffffffffffffffffffffffffffffff",
                )
                .is_err(),
            "stale setup generation must refuse"
        );
        let setup = first
            .lock_workspace_agent_setup(
                &summary.root,
                &summary.digest,
                &summary.installation,
                &generation,
            )
            .expect("exact setup generation");

        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (released_tx, released_rx) = std::sync::mpsc::channel();
        let release_root = summary.root.clone();
        let release_digest = summary.digest.clone();
        let release_installation = summary.installation.clone();
        let release_generation = generation.clone();
        let releasing = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            released_tx
                .send(second.release_workspace_agent_custody(
                    &release_root,
                    &release_digest,
                    &release_installation,
                    &release_generation,
                ))
                .unwrap();
        });
        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("release thread starts");
        assert!(
            released_rx.recv_timeout(Duration::from_millis(30)).is_err(),
            "release crossed a bounded pre-launch setup"
        );
        drop(setup);
        assert!(released_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("release resumes after setup")
            .expect("release result"));
        releasing.join().unwrap();

        assert!(
            first
                .lock_workspace_agent_setup(
                    &summary.root,
                    &summary.digest,
                    &summary.installation,
                    &generation,
                )
                .is_err(),
            "released generation must not regain setup authority"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn live_agent_file_snapshot_tracks_appearance_change_disappearance_and_refuses_mid_read_write()
    {
        let root = scratch("live-agent-file");
        std::fs::create_dir_all(&root).expect("workspace");
        let source = root.join("source");
        let managed = root.join("managed");
        std::fs::create_dir(&source).expect("source");
        std::fs::write(source.join("base.txt"), "saved\n").expect("saved source file");
        crate::PreparedFolderImport::prepare(&source, &managed)
            .expect("prepare workspace")
            .confirm_into_workspace()
            .expect("confirm workspace");
        let daemon = LiveDaemon::new(started());
        let summary = daemon.open_at_start(&managed).expect("open workspace");
        let physical_root = daemon
            .verified_managed_workspace_path(&summary.root, &summary.digest, &summary.installation)
            .expect("verified physical workspace")
            .path()
            .to_path_buf();
        let generation = daemon
            .acquire_workspace_agent_custody(
                &summary.root,
                &summary.digest,
                &summary.installation,
                false,
                None,
            )
            .expect("assign agent");
        let unchanged = daemon
            .inspect_agent_live_file(
                &summary.root,
                &summary.digest,
                &summary.installation,
                &generation,
                "base.txt",
            )
            .expect("stable unchanged file");
        assert_eq!(unchanged.kind(), "current-file");
        assert_eq!(unchanged.text(), Some("saved\n"));
        let live_path = physical_root.join("live.txt");
        std::fs::write(&live_path, "appeared\n").expect("new live file");
        let appeared = daemon
            .inspect_agent_live_file(
                &summary.root,
                &summary.digest,
                &summary.installation,
                &generation,
                "live.txt",
            )
            .expect("stable appearance");
        assert_eq!(appeared.kind(), "new-file");
        assert_eq!(appeared.text(), Some("appeared\n"));

        std::fs::write(&live_path, "changed again\n").expect("change live file");
        let changed = daemon
            .inspect_agent_live_file(
                &summary.root,
                &summary.digest,
                &summary.installation,
                &generation,
                "live.txt",
            )
            .expect("stable change");
        assert_eq!(changed.text(), Some("changed again\n"));
        assert_ne!(appeared.content_digest(), changed.content_digest());

        let changing_path = live_path.clone();
        BETWEEN_AGENT_LIVE_FILE_READS.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                std::fs::write(changing_path, "changed during read\n").expect("mid-read write");
            }));
        });
        let changing = daemon.inspect_agent_live_file(
            &summary.root,
            &summary.digest,
            &summary.installation,
            &generation,
            "live.txt",
        );
        assert!(
            matches!(changing, Err(ManagedTextFileError::Recovery(message)) if message.contains("changed while Mesh was reading"))
        );

        let oversized = vec![0_u8; MAX_AGENT_LIVE_PREVIEW_BYTES + 1];
        std::fs::write(&live_path, oversized).expect("write oversized live file");
        assert!(matches!(
            daemon.inspect_agent_live_file(
                &summary.root,
                &summary.digest,
                &summary.installation,
                &generation,
                "live.txt",
            ),
            Err(ManagedTextFileError::PreviewTooLarge { bytes, limit })
                if bytes == MAX_AGENT_LIVE_PREVIEW_BYTES + 1
                    && limit == MAX_AGENT_LIVE_PREVIEW_BYTES
        ));

        for index in 0..3 {
            std::fs::write(
                physical_root.join(format!("large-{index}.bin")),
                vec![u8::try_from(index).unwrap(); 4 * 1024 * 1024],
            )
            .expect("write large finish-preflight file");
        }
        let finish = daemon
            .inspect_agent_finish_preflight(
                &summary.root,
                &summary.digest,
                &summary.installation,
                &generation,
            )
            .expect("bounded finish preflight");
        assert!(finish
            .managed_files()
            .iter()
            .all(|file| file.retained_preview_capacity() == 0));
        assert!(finish
            .native_files()
            .iter()
            .all(|file| file.retained_preview_capacity() == 0));

        std::fs::remove_file(&live_path).expect("remove live file");
        assert!(daemon
            .inspect_agent_live_file(
                &summary.root,
                &summary.digest,
                &summary.installation,
                &generation,
                "live.txt",
            )
            .is_err());
        assert!(daemon
            .release_workspace_agent_custody(
                &summary.root,
                &summary.digest,
                &summary.installation,
                &generation,
            )
            .expect("release agent"));
        std::fs::remove_dir_all(root).unwrap();
    }

    fn checkpoint_parameters() -> CheckpointRuntimeParameters {
        CheckpointRuntimeParameters {
            idle_interval: Some(Duration::from_millis(10)),
            maximum_uncheckpointed_bytes: Some(1024),
            maximum_uncheckpointed_interval: Some(Duration::from_millis(20)),
        }
    }

    #[test]
    fn saved_workspace_preview_names_every_visible_change_without_content() {
        let basis = HistoricalWorkspacePreview {
            operation: RecordDigest::from_bytes([0x10; 32]),
            directories: vec![
                crate::workspace::HistoricalWorkspaceDirectory {
                    object: mesh_materializer::ObjectId::from_bytes([0x11; 16]),
                    path: "folder".to_owned(),
                },
                crate::workspace::HistoricalWorkspaceDirectory {
                    object: mesh_materializer::ObjectId::from_bytes([0x12; 16]),
                    path: "replaced".to_owned(),
                },
                crate::workspace::HistoricalWorkspaceDirectory {
                    object: mesh_materializer::ObjectId::from_bytes([0x18; 16]),
                    path: "replaced-folder".to_owned(),
                },
            ],
            files: vec![
                crate::workspace::HistoricalWorkspacePreviewFile {
                    object: mesh_materializer::ObjectId::from_bytes([0x13; 16]),
                    manifest_id: RecordDigest::from_bytes([0x41; 32]),
                    path: "changed.txt".to_owned(),
                    byte_length: 14,
                    content_digest: RecordDigest::from_bytes([0x31; 32]),
                    executable: false,
                },
                crate::workspace::HistoricalWorkspacePreviewFile {
                    object: mesh_materializer::ObjectId::from_bytes([0x14; 16]),
                    manifest_id: RecordDigest::from_bytes([0x42; 32]),
                    path: "removed.txt".to_owned(),
                    byte_length: 23,
                    content_digest: RecordDigest::from_bytes([0x32; 32]),
                    executable: false,
                },
                crate::workspace::HistoricalWorkspacePreviewFile {
                    object: mesh_materializer::ObjectId::from_bytes([0x15; 16]),
                    manifest_id: RecordDigest::from_bytes([0x43; 32]),
                    path: "same.txt".to_owned(),
                    byte_length: 20,
                    content_digest: RecordDigest::from_bytes([0x33; 32]),
                    executable: false,
                },
                crate::workspace::HistoricalWorkspacePreviewFile {
                    object: mesh_materializer::ObjectId::from_bytes([0x19; 16]),
                    manifest_id: RecordDigest::from_bytes([0x44; 32]),
                    path: "same-bytes-new-object.txt".to_owned(),
                    byte_length: 20,
                    content_digest: RecordDigest::from_bytes([0x33; 32]),
                    executable: false,
                },
            ],
        };
        let selected = HistoricalWorkspacePreview {
            operation: RecordDigest::from_bytes([0x20; 32]),
            directories: vec![
                crate::workspace::HistoricalWorkspaceDirectory {
                    object: mesh_materializer::ObjectId::from_bytes([0x11; 16]),
                    path: "folder".to_owned(),
                },
                crate::workspace::HistoricalWorkspaceDirectory {
                    object: mesh_materializer::ObjectId::from_bytes([0x1a; 16]),
                    path: "replaced-folder".to_owned(),
                },
            ],
            files: vec![
                crate::workspace::HistoricalWorkspacePreviewFile {
                    object: mesh_materializer::ObjectId::from_bytes([0x16; 16]),
                    manifest_id: RecordDigest::from_bytes([0x45; 32]),
                    path: "added.txt".to_owned(),
                    byte_length: 21,
                    content_digest: RecordDigest::from_bytes([0x34; 32]),
                    executable: false,
                },
                crate::workspace::HistoricalWorkspacePreviewFile {
                    object: mesh_materializer::ObjectId::from_bytes([0x13; 16]),
                    manifest_id: RecordDigest::from_bytes([0x46; 32]),
                    path: "changed.txt".to_owned(),
                    byte_length: 13,
                    content_digest: RecordDigest::from_bytes([0x35; 32]),
                    executable: false,
                },
                crate::workspace::HistoricalWorkspacePreviewFile {
                    object: mesh_materializer::ObjectId::from_bytes([0x17; 16]),
                    manifest_id: RecordDigest::from_bytes([0x47; 32]),
                    path: "replaced".to_owned(),
                    byte_length: 27,
                    content_digest: RecordDigest::from_bytes([0x36; 32]),
                    executable: false,
                },
                crate::workspace::HistoricalWorkspacePreviewFile {
                    object: mesh_materializer::ObjectId::from_bytes([0x15; 16]),
                    manifest_id: RecordDigest::from_bytes([0x48; 32]),
                    path: "same.txt".to_owned(),
                    byte_length: 20,
                    content_digest: RecordDigest::from_bytes([0x33; 32]),
                    executable: false,
                },
                crate::workspace::HistoricalWorkspacePreviewFile {
                    object: mesh_materializer::ObjectId::from_bytes([0x1b; 16]),
                    manifest_id: RecordDigest::from_bytes([0x49; 32]),
                    path: "same-bytes-new-object.txt".to_owned(),
                    byte_length: 20,
                    content_digest: RecordDigest::from_bytes([0x33; 32]),
                    executable: false,
                },
            ],
        };

        let changes = workspace_version_preview_changes(Some(&basis), &selected);
        assert_eq!(
            changes
                .iter()
                .map(|change| (change.path.as_str(), change.entry_type, change.effect))
                .collect::<Vec<_>>(),
            vec![
                ("added.txt", "file", "added"),
                ("changed.txt", "file", "changed"),
                ("removed.txt", "file", "removed"),
                ("replaced", "file", "replaced"),
                ("replaced-folder", "folder", "replaced"),
                ("same-bytes-new-object.txt", "file", "replaced"),
            ]
        );
        let rendered = format!("{changes:?}");
        assert!(!rendered.contains("private-content"));
        assert!(!rendered.contains("private-before"));
        assert!(!rendered.contains("private-after"));
    }

    #[test]
    fn saved_workspace_preview_and_open_stream_without_reconstructing_whole_files() {
        let root = scratch("streaming-workspace-preview");
        let source = root.join("source");
        let managed = root.join("managed.mesh");
        let opened = root.join("opened.mesh");
        fs::create_dir_all(&source).expect("source folder");
        let file_bytes = vec![0x5a; 512 * 1024];
        fs::write(source.join("large.bin"), &file_bytes).expect("source file");
        for index in 0..128 {
            fs::write(source.join(format!("empty-{index:03}.txt")), b"")
                .expect("small source file");
        }
        let daemon = LiveDaemon::with_checkpoint_runtime(started(), checkpoint_parameters())
            .expect("checkpoint configuration");
        let import = daemon
            .preview_folder_import(&source.display().to_string())
            .expect("preview import");
        assert_eq!(
            import
                .get("source_scope")
                .and_then(crate::ipc::Json::as_text),
            Some("ordinary-folder")
        );
        let summary = import
            .get("summary")
            .and_then(crate::ipc::Json::as_text)
            .expect("summary");
        daemon
            .confirm_folder_import(
                &source.display().to_string(),
                &managed.display().to_string(),
                summary,
            )
            .expect("confirmed import");
        let state = daemon.workspace_state().expect("workspace state");
        let operation = state.workspace_versions[0].operation().to_string();

        crate::workspace::reset_historical_reconstructed_bytes();
        crate::workspace::reset_historical_path_resolutions();
        let preview = daemon
            .preview_workspace_version_for_workspace(
                &state.root,
                &state.digest,
                &state.installation,
                &operation,
            )
            .expect("saved workspace preview");

        assert_eq!(
            preview
                .get("total_bytes")
                .and_then(crate::ipc::Json::as_text),
            Some("524288")
        );
        assert_eq!(
            crate::workspace::historical_reconstructed_bytes(),
            0,
            "the read-only picker retained the selected workspace's complete file bytes"
        );
        assert!(
            crate::workspace::historical_path_resolutions() <= 130,
            "saved-workspace preview repeatedly resolved every object for every visible path: {} resolutions",
            crate::workspace::historical_path_resolutions(),
        );

        daemon
            .fork_workspace_version(
                &operation,
                &opened.display().to_string(),
                &state.root,
                &state.digest,
                &state.installation,
            )
            .expect("open selected workspace");
        assert_eq!(
            crate::workspace::historical_reconstructed_bytes(),
            0,
            "opening the version retained the selected workspace's complete file bytes"
        );

        daemon
            .open_workspace(&state.root)
            .expect("return to source workspace before clean reuse");
        let source = daemon.workspace_state().expect("reopened source state");
        crate::workspace::reset_historical_reconstructed_bytes();
        assert!(
            daemon
                .reopen_workspace_version_if_exact(
                    &operation,
                    &opened,
                    &source.root,
                    &source.digest,
                    &source.installation,
                    None,
                )
                .expect("validate reusable native checkout")
                .is_some(),
            "the exact streamed checkout was not reusable"
        );
        assert_eq!(
            crate::workspace::historical_reconstructed_bytes(),
            0,
            "clean-checkout reuse retained the selected workspace's complete file bytes"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn saved_version_reuse_refuses_workspace_native_agent_custody_without_recent_hint() {
        let root = scratch("saved-version-reuse-shared-custody");
        let source = root.join("source");
        let managed = root.join("managed.mesh");
        let candidate_store = root.join("candidate.mesh");
        let presented = candidate_store.join(crate::workspace::PRESENTED_DIRECTORY_NAME);
        fs::create_dir_all(&source).expect("source folder");
        fs::write(source.join("note.txt"), b"saved version\n").expect("source file");
        let daemon = LiveDaemon::with_checkpoint_runtime(started(), checkpoint_parameters())
            .expect("checkpoint configuration");
        let import = daemon
            .preview_folder_import(&source.display().to_string())
            .expect("preview import");
        let summary = import
            .get("summary")
            .and_then(crate::ipc::Json::as_text)
            .expect("summary");
        daemon
            .confirm_folder_import(
                &source.display().to_string(),
                &managed.display().to_string(),
                summary,
            )
            .expect("confirmed import");
        let source_state = daemon.workspace_state().expect("source state");
        let operation = source_state.workspace_versions[0].operation().to_string();
        let forked = daemon
            .fork_workspace_version(
                &operation,
                &candidate_store.display().to_string(),
                &source_state.root,
                &source_state.digest,
                &source_state.installation,
            )
            .expect("create reusable candidate");
        let candidate_installation = forked
            .get("workspace")
            .and_then(|workspace| workspace.get("installation"))
            .and_then(crate::ipc::Json::as_text)
            .expect("candidate installation")
            .to_owned();
        daemon
            .reopen_existing_workspace(Path::new(&source_state.root))
            .expect("return to source");
        let source_state = daemon.workspace_state().expect("reopened source state");

        let custody =
            crate::workspace_custody::lock_for_workspace_path(&presented, &candidate_installation)
                .expect("candidate custody lock");
        let generation = custody
            .acquire(false, None)
            .expect("agent acquires candidate without a desktop Recent mirror");
        drop(custody);

        assert!(
            daemon
                .reopen_workspace_version_if_exact(
                    &operation,
                    &candidate_store,
                    &source_state.root,
                    &source_state.digest,
                    &source_state.installation,
                    None,
                )
                .expect("assigned candidate is a safe reuse miss")
                .is_none(),
            "workspace-native custody must defeat a stale unassigned Recent hint"
        );
        assert_eq!(
            daemon
                .workspace_state()
                .expect("source remains current")
                .root,
            source_state.root
        );

        let custody =
            crate::workspace_custody::lock_for_workspace_path(&presented, &candidate_installation)
                .expect("candidate custody release lock");
        assert!(custody.release(&generation).expect("exact agent release"));
        drop(custody);

        let custody_record = workspace_storage_root(&presented)
            .expect("candidate private storage")
            .join(crate::workspace_custody::RECORD_FILE);
        fs::write(&custody_record, b"{broken").expect("malformed candidate custody");
        fs::set_permissions(&custody_record, fs::Permissions::from_mode(0o600))
            .expect("owner-only malformed custody");
        assert!(
            daemon
                .reopen_workspace_version_if_exact(
                    &operation,
                    &candidate_store,
                    &source_state.root,
                    &source_state.digest,
                    &source_state.installation,
                    None,
                )
                .expect("malformed custody is a safe reuse miss")
                .is_none(),
            "malformed workspace-native custody must fail closed for reuse"
        );
        fs::remove_file(custody_record).expect("remove malformed custody fixture");
        assert!(daemon
            .reopen_workspace_version_if_exact(
                &operation,
                &candidate_store,
                &source_state.root,
                &source_state.digest,
                &source_state.installation,
                None,
            )
            .expect("released candidate can be reused")
            .is_some());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn historical_export_drop_never_deletes_a_replacement_directory() {
        let root = scratch("historical-export-replacement");
        fs::create_dir_all(&root).expect("parent");
        let snapshot = HistoricalWorkspaceSnapshot {
            operation: RecordDigest::from_bytes([0x42; 32]),
            directories: Vec::new(),
            files: vec![crate::workspace::HistoricalWorkspaceFile {
                object: mesh_materializer::ObjectId::from_bytes([0x11; 16]),
                path: "old.txt".to_owned(),
                bytes: b"historical bytes\n".to_vec(),
                executable: false,
            }],
        };
        let export = TemporaryHistoricalExport::create(&root.join("target.mesh"), &snapshot)
            .expect("temporary export");
        let replacement = export.path().to_path_buf();
        let displaced = root.join("displaced");
        fs::rename(&replacement, &displaced).expect("move owned directory");
        fs::create_dir(&replacement).expect("replacement directory");
        fs::write(replacement.join("keep.txt"), b"unowned\n").expect("replacement contents");

        drop(export);

        assert_eq!(
            fs::read(replacement.join("keep.txt")).unwrap(),
            b"unowned\n",
            "drop deleted a directory that replaced the owned export"
        );
        assert_eq!(
            fs::read(displaced.join("old.txt")).unwrap(),
            b"historical bytes\n"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn historical_export_drop_never_deletes_a_replacement_after_identity_check() {
        let root = scratch("historical-export-removal-race");
        fs::create_dir_all(&root).expect("parent");
        let snapshot = HistoricalWorkspaceSnapshot {
            operation: RecordDigest::from_bytes([0x47; 32]),
            directories: Vec::new(),
            files: vec![crate::workspace::HistoricalWorkspaceFile {
                object: mesh_materializer::ObjectId::from_bytes([0x12; 16]),
                path: "old.txt".to_owned(),
                bytes: b"historical bytes\n".to_vec(),
                executable: false,
            }],
        };
        let export = TemporaryHistoricalExport::create(&root.join("target.mesh"), &snapshot)
            .expect("temporary export");
        let replacement = export.path().to_path_buf();
        let displaced = root.join("displaced");
        let replacement_for_race = replacement.clone();
        let displaced_for_race = displaced.clone();
        BEFORE_HISTORICAL_EXPORT_REMOVE.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                fs::rename(&replacement_for_race, &displaced_for_race)
                    .expect("move owned directory after identity check");
                fs::create_dir(&replacement_for_race).expect("replacement directory");
                fs::write(replacement_for_race.join("keep.txt"), b"unowned\n")
                    .expect("replacement contents");
            }));
        });

        drop(export);

        assert_eq!(
            fs::read(replacement.join("keep.txt")).unwrap(),
            b"unowned\n",
            "drop deleted a directory installed after its identity check"
        );
        assert_eq!(
            fs::read(displaced.join("old.txt")).unwrap(),
            b"historical bytes\n"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn historical_export_symlink_cannot_redirect_snapshot_write() {
        use std::os::unix::fs::symlink;

        let root = scratch("historical-export-linked-child");
        let outside = scratch("historical-export-linked-outside");
        fs::create_dir_all(&root).expect("parent");
        fs::create_dir_all(&outside).expect("outside");
        let empty = HistoricalWorkspaceSnapshot {
            operation: RecordDigest::from_bytes([0x43; 32]),
            directories: Vec::new(),
            files: Vec::new(),
        };
        let export = TemporaryHistoricalExport::create(&root.join("target.mesh"), &empty)
            .expect("empty temporary export");
        symlink(&outside, export.path().join("nested")).expect("linked child");
        let snapshot = HistoricalWorkspaceSnapshot {
            operation: RecordDigest::from_bytes([0x44; 32]),
            directories: vec![crate::workspace::HistoricalWorkspaceDirectory {
                object: mesh_materializer::ObjectId::from_bytes([0x13; 16]),
                path: "nested".to_owned(),
            }],
            files: vec![crate::workspace::HistoricalWorkspaceFile {
                object: mesh_materializer::ObjectId::from_bytes([0x14; 16]),
                path: "nested/escaped.txt".to_owned(),
                bytes: b"must stay confined\n".to_vec(),
                executable: false,
            }],
        };

        let refusal = export
            .write(&snapshot)
            .expect_err("a linked child redirected a historical export");

        assert_eq!(refusal.code, "workspace-version-export-unavailable");
        assert!(
            !outside.join("escaped.txt").exists(),
            "historical bytes escaped the descriptor-pinned export"
        );
        drop(export);
        fs::remove_dir_all(root).expect("cleanup parent");
        fs::remove_dir_all(outside).expect("cleanup outside");
    }

    #[test]
    fn historical_export_replacement_root_cannot_inherit_write_authority() {
        let root = scratch("historical-export-replaced-root");
        fs::create_dir_all(&root).expect("parent");
        let empty = HistoricalWorkspaceSnapshot {
            operation: RecordDigest::from_bytes([0x45; 32]),
            directories: Vec::new(),
            files: Vec::new(),
        };
        let export = TemporaryHistoricalExport::create(&root.join("target.mesh"), &empty)
            .expect("empty temporary export");
        let replacement = export.path().to_path_buf();
        let displaced = root.join("displaced");
        fs::rename(&replacement, &displaced).expect("displace admitted root");
        fs::create_dir(&replacement).expect("replacement root");
        let snapshot = HistoricalWorkspaceSnapshot {
            operation: RecordDigest::from_bytes([0x46; 32]),
            directories: Vec::new(),
            files: vec![crate::workspace::HistoricalWorkspaceFile {
                object: mesh_materializer::ObjectId::from_bytes([0x15; 16]),
                path: "escaped.txt".to_owned(),
                bytes: b"must remain unwritten\n".to_vec(),
                executable: false,
            }],
        };

        let refusal = export
            .write(&snapshot)
            .expect_err("a replacement root inherited export authority");

        assert_eq!(refusal.code, "workspace-version-export-unavailable");
        assert!(!replacement.join("escaped.txt").exists());
        assert!(!displaced.join("escaped.txt").exists());
        drop(export);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn a_daemon_with_nothing_open_refuses_state_rather_than_answering_zero() {
        let daemon = LiveDaemon::new(started());
        let refusal = daemon.workspace_state().expect_err("nothing is open");
        assert_eq!(refusal.code, "no-workspace-open");
    }

    #[test]
    fn opening_a_folder_answers_and_publishes_one_entry() {
        let root = scratch("open");
        let daemon = LiveDaemon::new(started());
        let before = daemon.event_cursor();
        let summary = daemon
            .open_workspace(&root.display().to_string())
            .expect("open");
        assert_eq!(summary.records, 0);
        assert!(!summary.not_yet.is_empty(), "the refusals are published");
        assert!(summary
            .conditions
            .iter()
            .all(|condition| condition.code() != "unversioned-native-content"));
        assert_eq!(daemon.event_cursor(), before + 1);
        assert_eq!(
            daemon.events_since(before).entries[0].kind,
            EventKind::WorkspaceOpened { records: 0 }
        );
        assert_eq!(daemon.workspace_state().expect("state").root, summary.root);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn zero_history_with_ordinary_content_points_to_import_instead_of_looking_empty() {
        let root = scratch("unversioned-native-content");
        let imported_store = root.with_extension("imported.mesh");
        let _ = std::fs::remove_dir_all(&imported_store);
        std::fs::create_dir_all(root.join("docs")).expect("ordinary folder");
        std::fs::write(root.join("docs/note.txt"), b"not saved yet\n").expect("ordinary file");
        let daemon = LiveDaemon::new(started());

        let summary = daemon.open_at_start(&root).expect("workspace opens");
        std::fs::create_dir_all(root.join("chunks/aa")).expect("old private content store");
        std::fs::write(root.join("chunks/aa/private"), b"private payload")
            .expect("old private payload");
        std::fs::write(root.join("metadata.sqlite"), b"old private index")
            .expect("old private index");

        assert_eq!(summary.records, 0);
        assert!(summary.native_untracked_files.is_empty());
        let condition = summary
            .conditions
            .iter()
            .find(|condition| condition.code() == "unversioned-native-content")
            .expect("ordinary content condition");
        assert!(condition.message().contains("Preview this folder"));
        assert!(condition.related().is_empty());
        assert!(condition.recoverable());
        let preview = daemon
            .preview_folder_import(&root.display().to_string())
            .expect("migration preview");
        assert_eq!(
            preview
                .get("source_scope")
                .and_then(crate::ipc::Json::as_text),
            Some("open-zero-history-workspace")
        );
        assert_eq!(
            preview.get("files").and_then(crate::ipc::Json::as_u64),
            Some(1)
        );
        assert_eq!(
            preview
                .get("directories")
                .and_then(crate::ipc::Json::as_u64),
            Some(1)
        );
        let digest = preview
            .get("summary")
            .and_then(crate::ipc::Json::as_text)
            .expect("migration summary")
            .to_owned();
        let confirmed = daemon
            .confirm_folder_import(
                &root.display().to_string(),
                &imported_store.display().to_string(),
                &digest,
            )
            .expect("migration confirm");
        let receipt_summary = confirmed
            .get("import")
            .and_then(|import| import.get("summary"))
            .and_then(crate::ipc::Json::as_text)
            .expect("content summary in confirmed receipt");
        assert_eq!(
            &digest[..12],
            &receipt_summary[..12],
            "the scope-bound token keeps the legacy recovery lookup prefix"
        );
        let presented = PathBuf::from(
            confirmed
                .get("destination")
                .and_then(crate::ipc::Json::as_text)
                .expect("presented folder"),
        );
        assert_eq!(
            std::fs::read(presented.join("docs/note.txt")).expect("imported user file"),
            b"not saved yet\n"
        );
        assert!(!presented.join(".mesh").exists());
        assert!(!presented.join("chunks").exists());
        assert!(!presented.join("metadata.sqlite").exists());
        assert!(root.join("chunks/aa/private").is_file());
        assert!(root.join("metadata.sqlite").is_file());
        let recovered = daemon
            .reopen_confirmed_folder_import_if_exact(&root, &imported_store, &digest)
            .expect("recovery check")
            .expect("the scope-bound preview token still proves this exact imported source");
        assert_eq!(
            recovered
                .get("recovered_after_interruption")
                .and_then(crate::ipc::Json::as_bool),
            Some(true)
        );
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(imported_store);
    }

    #[test]
    fn switching_workspaces_between_import_preview_and_confirm_refuses_the_stale_mode() {
        let source = scratch("import-preview-workspace-switch");
        let replacement = scratch("import-preview-workspace-switch-replacement");
        let imported_store = source.with_extension("imported.mesh");
        let _ = std::fs::remove_dir_all(&imported_store);
        std::fs::create_dir_all(&source).expect("source folder");
        std::fs::write(source.join("note.txt"), b"person-owned bytes\n").expect("source file");
        let daemon = LiveDaemon::new(started());
        daemon.open_at_start(&source).expect("source opens");
        std::fs::create_dir_all(source.join("chunks/aa")).expect("old private content store");
        std::fs::write(source.join("chunks/aa/private"), b"private payload")
            .expect("old private payload");

        let preview = daemon
            .preview_folder_import(&source.display().to_string())
            .expect("private-fenced preview");
        let digest = preview
            .get("summary")
            .and_then(crate::ipc::Json::as_text)
            .expect("preview summary")
            .to_owned();
        assert_eq!(
            preview.get("files").and_then(crate::ipc::Json::as_u64),
            Some(1)
        );

        daemon
            .open_workspace(&replacement.display().to_string())
            .expect("replacement workspace opens");
        let refusal = daemon
            .confirm_folder_import(
                &source.display().to_string(),
                &imported_store.display().to_string(),
                &digest,
            )
            .expect_err("the preview mode changed with the open workspace");
        assert_eq!(refusal.code, "folder-import-preview-changed");
        assert!(
            !imported_store.exists(),
            "the stale managed copy was removed"
        );
        assert_eq!(
            std::fs::read(source.join("note.txt")).expect("source survives"),
            b"person-owned bytes\n"
        );
        assert!(source.join("chunks/aa/private").is_file());

        let _ = std::fs::remove_dir_all(source);
        let _ = std::fs::remove_dir_all(replacement);
    }

    #[test]
    fn live_ordinary_import_accepts_the_pre_scope_offline_token_only_outside_private_fence() {
        let root = scratch("legacy-offline-import-token");
        let source = root.join("ordinary");
        let managed = root.join("ordinary.mesh");
        std::fs::create_dir_all(&source).expect("ordinary source");
        std::fs::write(source.join("note.txt"), b"person-owned bytes\n").expect("source file");
        let raw = crate::preview_folder_import(&source)
            .expect("historical offline preview")
            .digest()
            .to_string();
        let daemon = LiveDaemon::new(started());

        daemon
            .confirm_folder_import(
                &source.display().to_string(),
                &managed.display().to_string(),
                &raw,
            )
            .expect("ordinary legacy token remains transition-compatible");

        let fenced_source = root.join("open-zero-history");
        let fenced_managed = root.join("fopen-zero-history.mesh");
        std::fs::create_dir_all(&fenced_source).expect("fenced source");
        std::fs::write(fenced_source.join("note.txt"), b"person-owned bytes\n")
            .expect("f source file");
        let fenced_daemon = LiveDaemon::new(started());
        fenced_daemon
            .open_at_start(&fenced_source)
            .expect("zero-history source opens");
        let fenced_raw = crate::folder_import::preview_open_workspace_import(&fenced_source)
            .expect("historical raw fenced preview")
            .digest()
            .to_string();

        let refusal = fenced_daemon
            .confirm_folder_import(
                &fenced_source.display().to_string(),
                &fenced_managed.display().to_string(),
                &fenced_raw,
            )
            .expect_err("a raw token must not bypass the private namespace fence");
        assert_eq!(refusal.code, "folder-import-preview-changed");
        assert!(!fenced_managed.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn switching_import_scope_refuses_even_when_visible_file_summary_is_identical() {
        let source_store = scratch("import-preview-equal-summary-switch");
        let replacement = scratch("import-preview-equal-summary-switch-replacement");
        let source = crate::workspace::OpenWorkspace::open_presented(&source_store)
            .expect("presented zero-history workspace")
            .root()
            .as_path()
            .to_path_buf();
        let imported_store = source_store.with_extension("imported.mesh");
        let _ = std::fs::remove_dir_all(&imported_store);
        std::fs::write(source.join("note.txt"), b"only ordinary content\n").expect("source file");
        let daemon = LiveDaemon::new(started());
        daemon.open_at_start(&source).expect("source opens");

        let preview = daemon
            .preview_folder_import(&source.display().to_string())
            .expect("zero-history preview");
        assert_eq!(
            preview
                .get("source_scope")
                .and_then(crate::ipc::Json::as_text),
            Some("open-zero-history-workspace")
        );
        let digest = preview
            .get("summary")
            .and_then(crate::ipc::Json::as_text)
            .expect("preview summary")
            .to_owned();

        daemon
            .open_workspace(&replacement.display().to_string())
            .expect("replacement workspace opens");
        let refusal = daemon
            .confirm_folder_import(
                &source.display().to_string(),
                &imported_store.display().to_string(),
                &digest,
            )
            .expect_err("the scope change must invalidate an equal-content preview");
        assert_eq!(refusal.code, "folder-import-preview-changed");
        assert!(
            !imported_store.exists(),
            "the stale managed copy was removed"
        );
        assert_eq!(
            std::fs::read(source.join("note.txt")).expect("source survives"),
            b"only ordinary content\n"
        );

        let _ = std::fs::remove_dir_all(source_store);
        let _ = std::fs::remove_dir_all(replacement);
    }

    #[test]
    fn a_restore_preview_is_bound_before_it_parses_another_workspaces_identity() {
        let root = scratch("bound-restore-preview");
        let daemon = LiveDaemon::new(started());
        let summary = daemon.open_at_start(&root).expect("workspace opens");

        for (expected_root, expected_digest, expected_installation) in [
            (
                "/another/workspace",
                summary.digest.as_str(),
                summary.installation.as_str(),
            ),
            (
                summary.root.as_str(),
                "another-fold-digest",
                summary.installation.as_str(),
            ),
            (
                summary.root.as_str(),
                summary.digest.as_str(),
                "another-installation",
            ),
        ] {
            let refusal = daemon
                .preview_file_restore_for_workspace(
                    expected_root,
                    expected_digest,
                    expected_installation,
                    "not-an-object-id",
                    "not-a-version-id",
                )
                .expect_err("a stale workspace cannot produce a preview");
            assert_eq!(refusal.code, "stale-workspace");
        }

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_managed_mutation_is_bound_to_the_exact_workspace_summary_the_person_reviewed() {
        let first = scratch("managed-binding-first");
        let second = scratch("managed-binding-second");
        let daemon = LiveDaemon::new(started());
        let reviewed = daemon
            .open_workspace(&first.display().to_string())
            .expect("open first workspace");

        daemon
            .open_workspace(&second.display().to_string())
            .expect("replace workspace through the local surface");
        let ran = std::cell::Cell::new(false);
        let refusal = daemon
            .with_verified_managed_workspace(
                &reviewed.root,
                &reviewed.digest,
                &reviewed.installation,
                || {
                    ran.set(true);
                    Ok(())
                },
            )
            .expect_err("the stale first-workspace summary must not authorize a mutation");
        assert!(matches!(refusal, ManagedTextFileError::StaleWorkspace));
        assert!(
            !ran.get(),
            "the guarded mutation ran against the replacement"
        );

        let current = daemon.workspace_state().expect("current workspace state");
        daemon
            .with_verified_managed_workspace(
                &current.root,
                &current.digest,
                &current.installation,
                || {
                    ran.set(true);
                    Ok(())
                },
            )
            .expect("the exact current summary authorizes the bounded operation");
        assert!(ran.get());

        let _ = std::fs::remove_dir_all(&first);
        let _ = std::fs::remove_dir_all(&second);
    }

    #[test]
    fn an_exact_clone_at_the_same_path_cannot_reuse_a_desktop_mutation_binding() {
        let root = scratch("managed-binding-exact-clone");
        let displaced = scratch("managed-binding-exact-clone-displaced");
        let daemon = LiveDaemon::new(started());
        let reviewed = daemon
            .open_workspace(&root.display().to_string())
            .expect("open original workspace");

        std::fs::rename(&root, &displaced).expect("move original directory aside");
        std::fs::create_dir(&root).expect("install replacement directory");
        std::fs::create_dir(root.join(crate::workspace::STORAGE_DIRECTORY_NAME))
            .expect("replacement private namespace");
        std::fs::copy(
            displaced
                .join(crate::workspace::STORAGE_DIRECTORY_NAME)
                .join(crate::workspace::RECORD_FILE_NAME),
            root.join(crate::workspace::STORAGE_DIRECTORY_NAME)
                .join(crate::workspace::RECORD_FILE_NAME),
        )
        .expect("clone exact record bytes");
        let replacement = daemon
            .open_workspace(&root.display().to_string())
            .expect("open exact clone");
        assert_eq!(replacement.root, reviewed.root);
        assert_eq!(replacement.digest, reviewed.digest);
        assert_ne!(replacement.installation, reviewed.installation);

        let ran = std::cell::Cell::new(false);
        let refusal = daemon
            .with_verified_managed_workspace(
                &reviewed.root,
                &reviewed.digest,
                &reviewed.installation,
                || {
                    ran.set(true);
                    Ok(())
                },
            )
            .expect_err("the prior physical installation must not authorize the exact clone");
        assert!(matches!(refusal, ManagedTextFileError::StaleWorkspace));
        assert!(!ran.get(), "the stale mutation ran against the exact clone");

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(displaced);
    }

    #[test]
    fn checkpoint_mutation_refuses_a_coordinator_from_another_workspace() {
        let first = scratch("checkpoint-workspace-first");
        let second = scratch("checkpoint-workspace-second");
        let daemon = LiveDaemon::with_checkpoint_runtime(started(), checkpoint_parameters())
            .expect("checkpoint configuration");
        daemon.open_at_start(&first).expect("first workspace");
        let (root, digest, installation, _) = daemon
            .checkpoint_snapshot_for_open_workspace()
            .expect("matching workspace snapshot");
        assert_eq!(root, first.display().to_string());
        let state = daemon.workspace_state().expect("state");
        assert_eq!(digest, state.digest);
        assert_eq!(installation, state.installation);

        let second_open = OpenWorkspace::open(&second).expect("second workspace");
        {
            let mut checkpoint = daemon
                .checkpoint
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            checkpoint
                .install(second_open.database_file())
                .expect("install mismatched coordinator");
        }
        let before = daemon.checkpoint_snapshot().expect("second snapshot");
        assert!(matches!(
            daemon.checkpoint_snapshot_for_open_workspace(),
            Err(AutomaticCheckpointError::WorkspaceChanged)
        ));

        let error = daemon
            .observe_checkpoint_activity(RecoverySequence::new(1).expect("sequence"), 1)
            .expect_err("mixed workspace state must fail closed");
        assert!(matches!(error, AutomaticCheckpointError::WorkspaceChanged));
        assert_eq!(
            daemon.checkpoint_snapshot().expect("unchanged snapshot"),
            before,
            "a mismatched workspace still mutated the installed coordinator"
        );
        assert_eq!(
            daemon.workspace_state().expect("first remains open").root,
            first.display().to_string()
        );

        let _ = std::fs::remove_dir_all(first);
        let _ = std::fs::remove_dir_all(second);
    }

    #[test]
    fn configured_open_migrates_recovery_before_index_repair_and_refuses_corrupt_owner() {
        let root = scratch("checkpoint-index-repair-isolation");
        let first = OpenWorkspace::open(&root).expect("initial workspace");
        let index_database = first.database_file().to_path_buf();
        drop(first);

        let mut legacy =
            SqliteRecoveryState::open(&index_database, LIVE_WORKSPACE_VIEW).expect("legacy owner");
        legacy
            .persist(&RecoverySnapshot::default())
            .expect("legacy durable marker");
        drop(legacy);
        let mut index = Sqlite::open(&index_database).expect("index connection");
        index
            .execute_batch("DROP TABLE operation;")
            .expect("damage only the rebuildable index schema");
        drop(index);

        let daemon = LiveDaemon::with_checkpoint_runtime(started(), checkpoint_parameters())
            .expect("checkpoint configuration");
        daemon
            .open_at_start(&root)
            .expect("legacy is copied before index replacement");
        drop(daemon);
        let recovery_database = index_database.with_file_name(RECOVERY_DATABASE_FILE_NAME);
        let mut isolated =
            SqliteRecoveryState::open(&recovery_database, LIVE_WORKSPACE_VIEW).expect("isolated");
        assert!(isolated.load().expect("load isolated").is_some());
        drop(isolated);

        // A later rebuild is free to replace metadata.sqlite and cannot reach the separate owner.
        let mut index = Sqlite::open(&index_database).expect("index connection");
        index
            .execute_batch("DROP TABLE operation;")
            .expect("damage index again");
        drop(index);
        let restarted = LiveDaemon::with_checkpoint_runtime(started(), checkpoint_parameters())
            .expect("checkpoint configuration");
        restarted
            .open_at_start(&root)
            .expect("isolated state survives index repair");
        drop(restarted);
        let mut isolated =
            SqliteRecoveryState::open(&recovery_database, LIVE_WORKSPACE_VIEW).expect("isolated");
        assert!(isolated.load().expect("load isolated").is_some());
        drop(isolated);

        std::fs::write(&recovery_database, b"not a sqlite database")
            .expect("corrupt authoritative recovery database");
        let index_before = std::fs::read(&index_database).expect("index bytes before refusal");
        let refused = LiveDaemon::with_checkpoint_runtime(started(), checkpoint_parameters())
            .expect("checkpoint configuration")
            .open_at_start(&root)
            .expect_err("corrupt recovery owner must fail closed");
        assert!(matches!(refused, OpenFailure::Index { .. }));
        assert_eq!(
            std::fs::read(&index_database).expect("index bytes after refusal"),
            index_before,
            "recovery refusal happens before any index repair"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn disabled_open_preserves_legacy_recovery_before_index_repair() {
        let root = scratch("disabled-checkpoint-index-repair-isolation");
        let first = OpenWorkspace::open(&root).expect("initial workspace");
        let index_database = first.database_file().to_path_buf();
        drop(first);

        let mut legacy =
            SqliteRecoveryState::open(&index_database, LIVE_WORKSPACE_VIEW).expect("legacy owner");
        legacy
            .persist(&RecoverySnapshot::default())
            .expect("legacy durable marker");
        drop(legacy);
        let mut index = Sqlite::open(&index_database).expect("index connection");
        index
            .execute_batch("DROP TABLE operation;")
            .expect("damage only the rebuildable index schema");
        drop(index);

        // Configuration is a runtime choice, not deletion authority over recovery truth left by
        // an earlier configured process. Opening once without thresholds must still copy the
        // legacy row before the disposable index is replaced.
        let daemon = LiveDaemon::new(started());
        daemon
            .open_at_start(&root)
            .expect("disabled runtime still preserves legacy recovery before repair");
        drop(daemon);

        let recovery_database = index_database.with_file_name(RECOVERY_DATABASE_FILE_NAME);
        let mut isolated =
            SqliteRecoveryState::open(&recovery_database, LIVE_WORKSPACE_VIEW).expect("isolated");
        assert!(
            isolated.load().expect("load isolated").is_some(),
            "the disabled open erased the only non-reconstructible recovery row"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn workspace_replacement_keeps_the_coordinator_and_event_on_the_installed_workspace() {
        let first = scratch("atomic-open-first");
        let second = scratch("atomic-open-second");
        let daemon = LiveDaemon::with_checkpoint_runtime(started(), checkpoint_parameters())
            .expect("checkpoint configuration");
        daemon.open_at_start(&first).expect("first workspace");
        let before = daemon.event_cursor();

        let summary = daemon
            .open_workspace(&second.display().to_string())
            .expect("second workspace");
        let checkpoint = daemon
            .checkpoint
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let held = daemon.held();
        let open = held.as_ref().expect("workspace installed");
        assert_eq!(open.root().as_path(), second);
        assert!(checkpoint.is_installed_for(open.database_file()));
        assert_eq!(summary.root, second.display().to_string());
        assert_eq!(daemon.event_cursor(), before + 1);
        assert_eq!(
            daemon.events_since(before).entries[0].kind,
            EventKind::WorkspaceOpened {
                records: summary.records
            }
        );

        drop(held);
        drop(checkpoint);
        let _ = std::fs::remove_dir_all(first);
        let _ = std::fs::remove_dir_all(second);
    }

    #[test]
    fn reopening_the_same_workspace_cannot_replace_a_newer_durable_view() {
        let root = scratch("same-workspace-reopen");
        let daemon = Arc::new(
            LiveDaemon::with_checkpoint_runtime(started(), checkpoint_parameters())
                .expect("checkpoint configuration"),
        );
        daemon.open_at_start(&root).expect("initial workspace");

        // Hold the first lock in the checkpoint -> workspace pair. On the buggy path,
        // `open_workspace` reads a stale OpenWorkspace before it waits for this lock. On the
        // repaired path it waits before reading, so the durable append below is part of its view.
        let checkpoint = daemon
            .checkpoint
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let opening = Arc::clone(&daemon);
        let path = root.display().to_string();
        let join = std::thread::spawn(move || opening.open_workspace(&path));
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            match daemon.workspace_open.try_lock() {
                Err(std::sync::TryLockError::WouldBlock) => break,
                Err(std::sync::TryLockError::Poisoned(error)) => {
                    drop(error.into_inner());
                    break;
                }
                Ok(guard) => drop(guard),
            }
            assert!(Instant::now() < deadline, "the reopen never started");
            std::thread::yield_now();
        }
        std::thread::sleep(Duration::from_millis(20));

        {
            let mut held = daemon.held();
            held.as_mut()
                .expect("workspace held")
                .append_record(&StoredRecord::Operation(OperationRecord {
                    id: RecordDigest::from_bytes([7; 32]),
                    actor: RecordDigest::from_bytes([2; 32]),
                    actor_sequence: 1,
                    hlc_millis: 1,
                    hlc_counter: 0,
                    policy_epoch: 0,
                    session: no_session(),
                    payload_digest: RecordDigest::from_bytes([3; 32]),
                    parents: Vec::new(),
                }))
                .expect("durable append");
            reopen(&mut held, &daemon.trusted_reviewers).expect("new durable view");
            assert_eq!(
                summarise(held.as_ref().expect("reopened"), None).records,
                1,
                "the durable view did not advance"
            );
        }

        drop(checkpoint);
        let summary = join.join().expect("reopen thread").expect("reopen result");
        assert_eq!(summary.records, 1, "reopen returned an older snapshot");
        assert_eq!(
            daemon.workspace_state().expect("live state").records,
            1,
            "reopen replaced the newer durable view with an older snapshot"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_failed_replacement_preserves_the_current_workspace_and_coordinator() {
        let first = scratch("failed-replacement-first");
        let damaged = scratch("failed-replacement-damaged");
        let daemon = LiveDaemon::with_checkpoint_runtime(started(), checkpoint_parameters())
            .expect("checkpoint configuration");
        daemon.open_at_start(&first).expect("first workspace");
        std::fs::create_dir_all(&damaged).expect("damaged folder");
        std::fs::write(damaged.join(RECORD_FILE_NAME), fragment()).expect("partial record");

        let refusal = daemon
            .open_workspace(&damaged.display().to_string())
            .expect_err("damaged replacement must be refused");
        assert_eq!(refusal.code, "workspace-nothing-readable");
        let held = daemon.held();
        let open = held.as_ref().expect("first workspace retained");
        assert_eq!(open.root().as_path(), first);
        let checkpoint = daemon
            .checkpoint
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        assert!(checkpoint.is_installed_for(open.database_file()));
        drop(checkpoint);
        drop(held);
        daemon
            .observe_checkpoint_activity(RecoverySequence::new(1).expect("sequence"), 1)
            .expect("retained coordinator remains usable");

        let _ = std::fs::remove_dir_all(first);
        let _ = std::fs::remove_dir_all(damaged);
    }

    #[test]
    fn a_replacement_refused_after_checkpoint_install_preserves_the_current_coordinator() {
        let first = scratch("failed-post-install-first");
        let refused = scratch("failed-post-install-refused");
        let daemon = LiveDaemon::with_checkpoint_runtime(started(), checkpoint_parameters())
            .expect("checkpoint configuration");
        daemon.open_at_start(&first).expect("first workspace");

        // Build a syntactically valid replacement whose persisted recovery envelope cannot name
        // its empty journal. Opening reaches coordinator installation and then fails the recovery
        // pointer validation that deliberately runs before the workspace swap.
        {
            let candidate = LiveDaemon::with_checkpoint_runtime(started(), checkpoint_parameters())
                .expect("candidate checkpoint configuration");
            candidate
                .open_at_start(&refused)
                .expect("candidate workspace");
            let sequence = RecoverySequence::new(1).expect("sequence");
            candidate
                .observe_checkpoint_activity(sequence, 1)
                .expect("candidate activity");
            let recovery_bytes = JOURNAL_RECOVERY_POINTER_DOMAIN.to_vec();
            let recovery_digest =
                RecordDigest::from_bytes(*Blake3::digest_bytes(&recovery_bytes).as_bytes());
            candidate
                .preserve_recovery(
                    RecoveryTrigger::ActorDisconnected,
                    RecoveryPreserved::from_verified_bytes(
                        RecoveryStamp::new(
                            1,
                            RecoveryEventUlid::from_bytes([7; 16]),
                            recovery_digest,
                        ),
                        sequence,
                        recovery_bytes,
                        recovery_digest,
                    )
                    .expect("opaque recovery bytes"),
                )
                .expect("persist candidate recovery");
        }

        let failure = daemon
            .open_workspace(&refused.display().to_string())
            .expect_err("invalid replacement recovery pointer must be refused");
        assert_eq!(failure.code, "workspace-index-unavailable");

        let held = daemon.held();
        let open = held.as_ref().expect("first workspace retained");
        assert_eq!(open.root().as_path(), first);
        let checkpoint = daemon
            .checkpoint
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        assert!(
            checkpoint.is_installed_for(open.database_file()),
            "a refused replacement split the retained workspace from its coordinator"
        );
        drop(checkpoint);
        drop(held);
        daemon
            .observe_checkpoint_activity(RecoverySequence::new(1).expect("sequence"), 1)
            .expect("retained coordinator remains usable");

        let _ = std::fs::remove_dir_all(first);
        let _ = std::fs::remove_dir_all(refused);
    }

    #[test]
    fn rolling_back_the_open_import_uninstalls_its_checkpoint_coordinator() {
        let root = scratch("rollback-open-import");
        let source = root.join("source");
        let managed = root.join("managed");
        std::fs::create_dir_all(&source).expect("source folder");
        std::fs::write(source.join("file.txt"), b"managed bytes\n").expect("source file");
        let daemon = LiveDaemon::with_checkpoint_runtime(started(), checkpoint_parameters())
            .expect("checkpoint configuration");
        let preview = daemon
            .preview_folder_import(&source.display().to_string())
            .expect("preview");
        let summary = preview
            .get("summary")
            .and_then(crate::ipc::Json::as_text)
            .expect("summary");
        let workspace = daemon
            .confirm_folder_import(
                &source.display().to_string(),
                &managed.display().to_string(),
                summary,
            )
            .expect("confirm and open");
        let presented = workspace
            .get("destination")
            .and_then(crate::ipc::Json::as_text)
            .expect("presented destination")
            .to_owned();
        let open = daemon.workspace_state().expect("open imported workspace");
        assert_eq!(open.root, presented);
        assert_eq!(
            daemon.automatic_checkpoint_status(),
            AutomaticCheckpointStatus::Active
        );

        let stale = daemon
            .rollback_folder_import_for_workspace(&open.root, &open.digest, "stale-installation")
            .expect_err("stale desktop authority must not remove the managed copy");
        assert_eq!(stale.code, "stale-workspace");
        assert!(managed.exists());
        daemon
            .rollback_folder_import_for_workspace(&open.root, &open.digest, &open.installation)
            .expect("verified desktop rollback");
        assert_eq!(
            daemon.automatic_checkpoint_status(),
            AutomaticCheckpointStatus::WaitingForWorkspace
        );
        assert!(daemon.workspace_state().is_err());
        assert!(matches!(
            daemon.checkpoint_snapshot(),
            Err(AutomaticCheckpointError::NoWorkspace)
        ));
        assert!(!managed.exists());

        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn rolling_back_an_aliased_open_import_uninstalls_the_same_workspace() {
        use std::os::unix::fs::symlink;

        let root = scratch("rollback-aliased-open-import");
        let source = root.join("source");
        let managed = root.join("managed");
        let alias = root.join("managed-alias");
        std::fs::create_dir_all(&source).expect("source folder");
        std::fs::write(source.join("file.txt"), b"managed bytes\n").expect("source file");
        crate::PreparedFolderImport::prepare(&source, &managed)
            .expect("verified import")
            .confirm_into_workspace()
            .expect("confirmed managed workspace");
        symlink(&managed, &alias).expect("workspace alias");

        let daemon = LiveDaemon::with_checkpoint_runtime(started(), checkpoint_parameters())
            .expect("checkpoint configuration");
        daemon.open_at_start(&alias).expect("open through alias");
        assert_eq!(
            daemon.automatic_checkpoint_status(),
            AutomaticCheckpointStatus::Active
        );

        daemon
            .rollback_folder_import(&managed.display().to_string())
            .expect("rollback through real destination");
        assert_eq!(
            daemon.automatic_checkpoint_status(),
            AutomaticCheckpointStatus::WaitingForWorkspace
        );
        assert!(daemon.workspace_state().is_err());
        assert!(matches!(
            daemon.checkpoint_snapshot(),
            Err(AutomaticCheckpointError::NoWorkspace)
        ));
        assert!(!managed.exists());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn checkpoint_attention_is_published_only_when_a_workspace_gains_the_condition() {
        assert!(checkpoint_attention_became_visible(false, true));
        assert!(!checkpoint_attention_became_visible(false, false));
        assert!(!checkpoint_attention_became_visible(true, true));
        assert!(!checkpoint_attention_became_visible(true, false));
    }

    #[test]
    fn configured_reviewer_key_does_not_claim_humanheld_publication_authority() {
        let root = scratch("trusted-empty");
        let daemon = LiveDaemon::with_trusted_reviewers(
            started(),
            TrustedReviewers::new([mesh_types::PublicKey::from_bytes([7; 32])]),
        );

        let summary = daemon
            .open_workspace(&root.display().to_string())
            .expect("open");

        assert_eq!(summary.shared_version, None);
        assert!(summary
            .not_yet
            .iter()
            .any(|(subject, _)| subject == "shared version"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn publication_refuses_missing_or_mismatched_review_and_unreadable_receipt_without_appending() {
        let root = scratch("publication-refusals");
        let target = RecordDigest::from_bytes([4; 32]);
        let other = RecordDigest::from_bytes([5; 32]);
        let bundle = RecordDigest::from_bytes([9; 32]);
        let missing_bundle = RecordDigest::from_bytes([10; 32]);
        let mut file = RecordFile::open(&root.join(RECORD_FILE_NAME)).expect("record file");
        journal_records(
            &mut file,
            [
                StoredRecord::Operation(OperationRecord {
                    id: target,
                    actor: RecordDigest::from_bytes([2; 32]),
                    actor_sequence: 1,
                    hlc_millis: 1,
                    hlc_counter: 0,
                    policy_epoch: 0,
                    session: no_session(),
                    payload_digest: RecordDigest::from_bytes([3; 32]),
                    parents: Vec::new(),
                }),
                StoredRecord::Operation(OperationRecord {
                    id: other,
                    actor: RecordDigest::from_bytes([2; 32]),
                    actor_sequence: 2,
                    hlc_millis: 2,
                    hlc_counter: 0,
                    policy_epoch: 0,
                    session: no_session(),
                    payload_digest: RecordDigest::from_bytes([6; 32]),
                    parents: vec![target],
                }),
                StoredRecord::Review(ReviewRecord {
                    bundle,
                    subject_operation: target,
                    opened_by: RecordDigest::from_bytes([8; 32]),
                }),
            ]
            .iter(),
        )
        .expect("seed operations");
        let signer = SigningKey::from_bytes(&[7; 32]);
        let trusted = mesh_types::PublicKey::from_bytes(signer.verifying_key().to_bytes());
        let daemon =
            LiveDaemon::with_trusted_reviewers(started(), TrustedReviewers::new([trusted]));
        daemon.open_at_start(&root).expect("open");

        let missing = daemon
            .approve_review(&missing_bundle.to_string(), &target.to_string(), "00")
            .expect_err("review absent");
        assert_eq!(missing.code, "publication-review-absent");
        let mismatch = daemon
            .approve_review(&bundle.to_string(), &other.to_string(), "00")
            .expect_err("target mismatch");
        assert_eq!(mismatch.code, "publication-review-conflict");
        let unavailable = daemon
            .approve_review(&bundle.to_string(), &target.to_string(), "00")
            .expect_err("human authority unavailable");
        assert_eq!(unavailable.code, "publication-human-authority-unavailable");
        assert_eq!(daemon.workspace_state().expect("state").records, 3);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_refused_open_is_published_too_so_a_subscriber_learns_about_it() {
        let root = scratch("refused");
        std::fs::create_dir_all(&root).expect("mkdir");
        // A whole frame with one byte flipped: damage, not an interrupted append. Bytes that are
        // merely too short are a fragment, and a fragment *behind a whole record* is served rather
        // than refused; a fragment with nothing behind it is `workspace-nothing-readable`, which
        // the next case covers.
        let mut framed =
            mesh_store::frame_record(&mesh_store::StoredRecord::Peer(mesh_store::PeerRecord {
                peer: mesh_store::RecordDigest::from_bytes([7; 32]),
                joined_at: mesh_store::RecordDigest::from_bytes([1; 32]),
            }));
        let last = framed.len() - 1;
        framed[last] ^= 0xFF;
        std::fs::write(root.join(crate::workspace::RECORD_FILE_NAME), &framed).expect("write");
        let daemon = LiveDaemon::new(started());
        let before = daemon.event_cursor();
        let refusal = daemon
            .open_workspace(&root.display().to_string())
            .expect_err("damaged");
        assert_eq!(refusal.code, "workspace-damaged");
        let published = daemon.events_since(before);
        assert_eq!(
            published.entries[0].kind,
            EventKind::WorkspaceRefused {
                code: "workspace-damaged".to_owned()
            }
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_startup_answer_follows_the_workspace_that_was_opened() {
        let root = scratch("startup");
        let daemon = LiveDaemon::new(started());
        daemon.open_at_start(&root).expect("open");
        let summary = daemon.startup();
        assert!(summary.serving);
        assert!(!summary.sentence.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The bytes a crash during the very first save leaves, put through the start-up path.
    ///
    /// `startup.report` is the one answer a person gets before anything else, so the failure this
    /// checks for is the quiet one: a refused open leaving the summary at the value a daemon with
    /// no workspace carries, which reads as a clean start.
    #[test]
    fn a_start_up_that_found_no_boundary_never_answers_with_the_clean_start_sentence() {
        let root = scratch("no-boundary");
        std::fs::create_dir_all(&root).expect("mkdir");
        let framed = fragment();
        std::fs::write(root.join(crate::workspace::RECORD_FILE_NAME), &framed).expect("write");

        let daemon = LiveDaemon::new(started());
        let clean = daemon.startup().sentence;
        let failure = daemon
            .open_at_start(&root)
            .expect_err("no boundary to open at");
        assert_eq!(failure.code(), "workspace-nothing-readable");

        let summary = daemon.startup();
        assert!(
            !summary.serving,
            "a refused open reported itself as serving"
        );
        assert_ne!(
            summary.sentence, clean,
            "the start-up answer stayed at the sentence a daemon with no workspace carries"
        );
        assert!(
            daemon.crash_report().is_none(),
            "a daemon holding no workspace reported a boundary anyway"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The boundary report exists exactly when a workspace does.
    #[test]
    fn the_boundary_report_arrives_with_the_workspace_and_not_before() {
        let root = scratch("report");
        let daemon = LiveDaemon::new(started());
        assert!(daemon.crash_report().is_none());

        daemon.open_at_start(&root).expect("open");
        let report = daemon
            .crash_report()
            .expect("an open workspace has a boundary");
        assert_eq!(report.saved_records(), 0);
        assert_eq!(report.unfinished_bytes(), 0);
        assert!(report.is_serving());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Half of one whole frame: an interrupted append, with nothing whole in front of it.
    fn fragment() -> Vec<u8> {
        let framed =
            mesh_store::frame_record(&mesh_store::StoredRecord::Peer(mesh_store::PeerRecord {
                peer: mesh_store::RecordDigest::from_bytes([7; 32]),
                joined_at: mesh_store::RecordDigest::from_bytes([1; 32]),
            }));
        let kept = framed.len() / 2;
        framed[..kept].to_vec()
    }

    #[test]
    fn a_daemon_is_serving_even_when_the_workspace_is_not() {
        let daemon = LiveDaemon::new(StartupSummary::from(&RecoveryDiagnostic::new(
            crate::recovery::RecoveryOutcome::Unrecoverable {
                detail: "a driver said no".to_owned(),
            },
            core::time::Duration::from_millis(1),
            crate::recovery::RECOVERY_BUDGET,
        )));
        assert!(
            daemon.serving(),
            "the surface that reports damage must answer"
        );
        assert!(!daemon.startup().serving);
    }
}
