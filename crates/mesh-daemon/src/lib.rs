//! The local daemon: a process you can start, its IPC surface, and its crash diagnostics.
//!
//! # It runs
//!
//! ```text
//! cargo run -p mesh-daemon -- --workspace ./my-workspace
//! ```
//!
//! `src/main.rs` is `meshd`, the background service. `src/bin/meshctl.rs` is a client for it.
//! Everything below is the library both of them are built out of, and everything the service
//! answers is folded from records that are actually on disk.
//!
//! # Five things are here, and they are one story
//!
//! **A workspace on disk** ([`workspace`]) is the daemon's one durable resource: a folder with one
//! append-only file of framed records in it, forced to disk on every append. Opening it scans that
//! file with `mesh_store::scan_journal` and folds it with `mesh_store::rebuild`. `metadata.sqlite`
//! is the rebuildable local index and checkpoint-state store; the framed journal remains the
//! authority, and every open can reconstruct the index from it rather than trusting SQLite as a
//! second source of canonical truth.
//!
//! **Start-up recovery** ([`recover_on_start`], [`RecoveryDiagnostic`], [`DiagnosticsFeed`]) is
//! what a caller with a `mesh_store::Store` uses on the path where the process has just been
//! killed: recover the index, judge how long it took, report it. The recovery itself belongs to
//! `mesh-store` and nothing here re-implements any of it. [`CrashReport`] is that recovery read as
//! an answer to the question a person actually asks after a crash — *what survived* — with the
//! work that was reported as saved privately kept apart from the bytes of a save that never
//! finished, and with a section a support bundle can carry.
//!
//! **The local IPC surface** ([`ipc`]) is how anything else learns any of that. Plan §8.3's last
//! bullet says the user interface may not write the database directly; [`ipc::METHODS`] is the
//! narrow door it goes through instead, over a Unix-domain socket, with an explicitly negotiated
//! surface version. [`user_messages`] holds every sentence that reaches a person, in one file,
//! because `tools/program/vocab-lint/surfaces.json` lints that path and cannot lint a string
//! written inline somewhere else.
//!
//! **The running daemon** ([`LiveDaemon`]) composes the first three: it is the one
//! [`ipc::Operations`] with a real workspace behind it, and the one the binary serves.
//!
//! **The folder-watching fallback** ([`fallback`], [`folder_watch`]) is plan §7.4's escape hatch:
//! a `mesh_materializer::WorkspaceAdapter` that presents a folder on the real filesystem when no
//! direct file-system connection is available, and the product surface that says what it gives up.
//! [`choose_backend`] is the only way to select it and there is no argument that would let a caller
//! prefer it over an available direct connection; choosing it carries [`FallbackRestriction::ALL`]
//! and an announcement, unconditionally. `meshd` prints both when it starts and `meshctl
//! restrictions` answers them with nothing running.
//!
//! # What is not here
//!
//! No write path — nothing on the socket appends a record, because advancing canonical state
//! before `mesh-policy` is consulted is not something a local socket should offer, and
//! `mesh-policy` is not consulted anywhere here. File names and folders are materialized from
//! verified ChangeSet payloads in the workspace content store, never from the working directory;
//! missing payloads are published as recoverable partial-answer conditions. The shared version is
//! derived only from a checked receipt plus a durable HumanHeld policy decision. A configured
//! reviewer public key alone is not that decision, so shared publication remains named in
//! [`workspace::OpenWorkspace::not_yet`]. **The
//! folder-watching backend is not on the socket either**: [`folder_watch::DirectoryAdapter`] is
//! compiled into this crate and graded by `crates/mesh-daemon/tests/folder-watch.rs`, and no
//! [`ipc::METHODS`] entry mounts a view through it.
//!
//! **The performance counters** ([`counters`]) are here, under **E14 observability**: one counter
//! behind every metric of plan §12.3, deterministic — a count or a byte total — wherever the metric
//! admits one. This build feeds the counters it has facts for and names the rest with their reasons
//! in [`counters::CounterSnapshot::not_yet`], the same way the workspace summary does. The
//! telemetry export is still ahead; follow the public roadmap and issue tracker for that work.
//!
//! Plan §8.3 puts this crate at the service layer, above the engine, so it is the composition root
//! and every edge in the manifest points downward. [`version_state`] is where that shows: it fills
//! `mesh-state`'s digest seam with `mesh-types`' BLAKE3 — the seam both core crates leave open for
//! exactly this layer — and folds the records on disk into a version identifier this replica
//! derived rather than a counter it took on trust.

// Private modules with a flat re-export at the crate root, following `mesh-types` and `mesh-store`:
// a module path is a second name for the same item that no terminology register row covers. `ipc`
// is the exception and is public, because its five modules are five distinct subjects and a flat
// re-export of all of them would be a worse name than `ipc::message::ClientMessage`. `workspace`
// is public for the same reason: `workspace::RecordFile` says which file, and a bare `RecordFile`
// at the crate root would not. `counters` is public on the same ground — a bare `Family` or `Unit`
// at the crate root would name nothing, and `counters::Family` names plan §12.3's grouping.
mod authenticated_changeset;
mod checkpoint_runtime;
mod checkpoint_storage;
pub mod counters;
mod crash_report;
mod exclusions;
pub mod fallback;
pub mod fleet;
mod folder_import;
#[cfg(unix)]
pub mod project_attachment;
// The folder-watching fallback backend. `cfg(unix)` because an object identity here is the device
// and inode the kernel reports; `ipc::server` is gated the same way.
#[cfg(unix)]
pub mod folder_watch;
pub mod ipc;
#[cfg(unix)]
mod live;
mod managed_file;
mod managed_mutation;
mod manifest_paging;
mod publication;
mod pull_back_receipt;
mod recovery;
mod root_authority;
mod support_bundle;
pub mod user_messages;
pub mod version_state;
pub mod workspace;
mod workspace_custody;

pub use crate::authenticated_changeset::AuthenticatedChangeSetError;
pub use crate::checkpoint_runtime::{
    route_boundary, AutomaticCheckpointError, AutomaticCheckpointRuntime,
    AutomaticCheckpointStatus, BoundaryRouting, RecoveryRuntimeSignal,
};
pub use crate::checkpoint_storage::{
    save_file_version, save_file_version_with_journal, CasChunkPromoter, CasChunkPromoterError,
    CheckpointSaveError, CheckpointSigner, FileVersionCheckpointRequest, JournaledPrivateSave,
    PreparedCheckpointFile,
};
pub use crate::crash_report::CrashReport;
pub use crate::exclusions::{
    EffectiveExclusions, ExclusionLoadFailure, LoadedSource, REPOSITORY_IGNORE_FILE_NAME,
};
pub use crate::fallback::{
    choose_backend, Availability, BackendChoice, FallbackRestriction, WorkspaceBackend,
};
pub use crate::folder_import::{
    preview_folder_import, recover_pending_import, ConfirmedFolderImport, FolderImportError,
    ImportSummary, ImportedFile, ManagedImportOutcome, PreparedFolderImport,
};
#[cfg(unix)]
pub use crate::live::{
    AgentFileCheckpointRequest, AgentFinishPreflight, AgentLiveFileSnapshot,
    AgentWorkspaceCheckpoint, AgentWorkspaceCheckpointRequest, DurableHumanApproval,
    HumanApprovalPreview, LiveCheckpointSaveError, LiveDaemon, OrphanCleanupOutcome,
    OrphanCleanupStatus, OrphanCleanupWorker, ReviewArtifact, VerifiedManagedWorkspaceEntry,
    VerifiedManagedWorkspacePath, WorkspaceAgentSetupGuard, WorkspaceVersionForkRequest,
};
pub use crate::managed_file::{
    ManagedDirectoryExport, ManagedDirectoryExportBatchPreview, ManagedDirectoryExportPreview,
    ManagedEntryChange, ManagedFileExport, ManagedFileExportPreview, ManagedFileInspection,
    ManagedPrivateSave, ManagedRetiredExportPreview, ManagedRetiredExportRemoval, ManagedTextFile,
    ManagedTextFileError, ManagedTextSave, ManagedVersionRestore, NativeDirectoryInspection,
    NativeFileInspection, NativeMissingFile, LOCAL_EDIT_IDLE_MILLIS, MAX_MANAGED_TEXT_BYTES,
};
pub use crate::manifest_paging::{
    reconstruct_paged_manifest, ManifestPagingError, ManifestPagingPolicy, PageRoute,
    PagedManifest, PhysicalManifestIndex, PhysicalManifestPage, MANIFEST_PAGE_REFERENCES,
};
pub use crate::publication::{TrustedReviewers, GENESIS_SHARED_HEAD};
pub use crate::recovery::{
    outcome_of, recover_on_start, BudgetVerdict, DiagnosticsFeed, RecoveryBudget,
    RecoveryDiagnostic, RecoveryOutcome, Severity, RECOVERY_BUDGET,
};
pub use crate::root_authority::ProtectedWorkspaceRoot;
pub use crate::support_bundle::{PinnedSupportTarget, SupportBundle};
pub use crate::version_state::{OrderAgreement, PrivateVersion, WaitingChange};
pub use crate::workspace::{
    FileRestorePreview, NativeUnsupportedEntry, OpenFailure, OpenWorkspace, RestorePreviewFailure,
    RestoreVersionIdentity, RetiredWorkspaceEntry, WorkspaceCondition, WorkspaceEntry,
    WorkspaceFileHistory, RECORD_FILE_NAME,
};
pub use crate::workspace_custody::{
    DependencyEnrollmentFence, WorkspaceAgentCustody, WorkspaceAgentCustodyError,
};
pub use mesh_store::{CheckpointRuntimeParameters, RecordDigest as ManagedContentDigest};

/// The crate's name, so a placeholder still carries one verifiable behaviour.
pub const CRATE_NAME: &str = "mesh-daemon";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_stable() {
        assert_eq!(CRATE_NAME, "mesh-daemon");
    }
}
