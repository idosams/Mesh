//! The local SQLite WAL index: plan §6.1's local transactional store.
//!
//! # One sentence this crate is built around
//!
//! **This database is an index over immutable records. It is never the source of truth, and it is
//! never workspace content.** Everything below follows from that, including the parts that look
//! like restrictions.
//!
//! # The four properties, and where each one is actually checked
//!
//! | Property | Where it is made true | Where it is proved |
//! |---|---|---|
//! | Every table is reconstructable from immutable operations and manifests alone | [`Provenance`] has no shape for a fact with no source; [`Index`] is a pure fold | `tests/reconstruction.rs` — the live path and a from-scratch replay must digest identically, checked through a real database |
//! | Concurrent readers never block on the writer under WAL | [`PRAGMAS`] sets `journal_mode = WAL`; [`CommitPlan`] uses `BEGIN IMMEDIATE` | `tests/wal_concurrency.rs` — readers run against a *held, uncommitted* write transaction, with a rollback-journal control that shows them blocking when WAL is off |
//! | The database path is excluded from materialization by construction | Naming: [`WorkspaceRelativePath`] cannot *spell* anything outside [`MountRoot`], and the database is [`MountRoot`]'s sibling. Resolution: [`MountRoot::resolve`] refuses a path any component of which is a symbolic link, and is the only source of a [`ResolvedPath`] | `tests/materialization_exclusion.rs` — including the `-wal` and `-shm` sidecars, and a link planted inside the mount root by a writer who is allowed to write there |
//! | Migrations are forward-only, with a version table, each tested against a populated database | [`Migration`] has no reverse; [`verify_applied`] refuses a rewritten one | `tests/migrations.rs` — every migration runs against a database the previous one populated |
//! | "Saved privately" is never reported before the transaction returns | [`PrivateSaved`] has no public constructor and is produced by one step, after step 9 returned `Ok` | `tests/crash-commit-sequence.rs` — a real `SIGKILL` after each of plan §6.3's eleven steps, with the acknowledgement observed across the process death |
//! | A crashed index is rebuilt from records alone, to the digest the acknowledgement carried | [`Store::recover_verified`] reads only the [`RecordJournal`]; [`scan_journal`] decides where durability stops | `tests/recovery.rs` — the database is destroyed and rebuilt, and a real `SIGKILL` at each of the eleven steps is followed by a recovery whose boundary is checked |
//!
//! # The acknowledgement boundary
//!
//! Plan §6.3's eleven steps span two crates. Steps 1 to 4 are the content-addressed store's and
//! reach this crate through the [`ChunkPromoter`] seam; steps 5 to 9 are [`CommitPlan`]; steps 10
//! and 11 are the acknowledgement and the replication handoff. [`DurableCommit`] is the whole of
//! it, performed one interruptible step at a time, and [`SequenceStep::residue_if_killed_after`]
//! carries the plan's crash behaviour as data so the harness and the code cannot drift apart.
//!
//! # The production driver and the seam
//!
//! [`SqlExecutor`] remains the semantic boundary: migrations, recovery and commit ordering depend
//! on four operations rather than on a driver API. [`Sqlite`] is the production implementation,
//! backed by bundled SQLite so opening a workspace never depends on a separately installed
//! executable or system library. The process-based test driver remains an independent second
//! implementation for concurrency and crash campaigns.
//!
//! # Where to start
//!
//! ```
//! use mesh_store::{
//!     Checkpoint, CommitPlan, Index, OperationRecord, RecordDigest, StoredRecord, rebuild,
//! };
//!
//! let operation = OperationRecord {
//!     id: RecordDigest::from_bytes([1; 32]),
//!     actor: RecordDigest::from_bytes([2; 32]),
//!     actor_sequence: 1,
//!     hlc_millis: 1_700_000_000_000,
//!     hlc_counter: 0,
//!     policy_epoch: 1,
//!     session: mesh_store::no_session(),
//!     payload_digest: RecordDigest::from_bytes([3; 32]),
//!     parents: Vec::new(),
//! };
//!
//! // The live path: fold a checkpoint in and get the transaction that persists it.
//! let checkpoint = Checkpoint { operations: vec![operation.clone()], ..Checkpoint::default() };
//! let (plan, live) = CommitPlan::stage(&Index::new(), &checkpoint).unwrap();
//! assert!(plan.sql().starts_with("BEGIN IMMEDIATE;"));
//!
//! // The recovery path: replay the same records into a fresh index. The two must agree.
//! let (_, report) = rebuild([StoredRecord::Operation(operation)], Vec::new()).unwrap();
//! assert_eq!(report.digest, live.default_digest());
//! ```

// Private modules with a flat re-export at the crate root, following `mesh-types`: `docs/protocol.md`
// §3.10 requires every public item to resolve to a register term, and a module path is a second
// name for the same item that no register row covers.
mod checkpoint_runtime;
mod collect;
mod commit;
mod digest;
mod exclusion;
pub mod fleet;
mod ids;
mod index;
mod migration;
mod paths;
mod rebuild;
mod recovery;
mod recovery_state;
mod retention;
mod row;
mod schema;
mod sequence;
mod sql;
mod sqlite_driver;

pub use crate::collect::{CollectionPlan, CollectionReason, Doomed, Kept};
pub use crate::retention::{
    Reachability, RetainedRoot, RetainedRoots, RetentionError, RetentionPolicy,
};

pub use crate::checkpoint_runtime::{
    CheckpointCoordinator, CheckpointRuntimeConfig, CheckpointRuntimeConfigError,
    CheckpointRuntimeError, CheckpointRuntimeParameters, SettledWindow,
    SELECTED_CHECKPOINT_IDLE_INTERVAL, SELECTED_MAXIMUM_UNCHECKPOINTED_BYTES,
    SELECTED_MAXIMUM_UNCHECKPOINTED_INTERVAL,
};
pub use crate::commit::{Checkpoint, CommitPlan, CommitStep, Statement};
pub use crate::digest::{Digest16, Fnv1a128, IndexDigest};
pub use crate::exclusion::{
    Admission, Exclusion, ExclusionError, ExclusionRule, ExclusionSet, ExclusionSource,
    WORKSPACE_EXCLUSION_FILE_NAME,
};
pub use crate::ids::{EntityUuid, IdError, RecordDigest};
pub use crate::index::{no_session, FoldError, Index};
pub use crate::migration::{
    full_schema_sql, plan_migrations, verify_applied, AppliedMigration, Migration, MigrationError,
    MigrationPlan, CURRENT_VERSION, MIGRATIONS,
};
pub use crate::paths::{
    DatabasePath, MountRoot, PathError, ResolutionError, ResolvedPath, WorkspaceRelativePath,
    WorkspaceRoot, DATABASE_FILE_NAME, MOUNT_DIRECTORY_NAME,
};
pub use crate::rebuild::{
    indexed_record_kinds, rebuild, tables_fed_by, tables_outside_the_fold, RebuildReport,
};
pub use crate::recovery::{
    frame_record, journal_records, scan_journal, DamageKind, DurableBoundary, JournalDamage,
    JournalScan, PendingPrivateSaveBoundaryError, RecordJournal, RecoveryError, RecoveryReport,
    TailResidue,
};
pub use crate::recovery_state::{
    BoundaryEvidenceKind, MeaningfulCheckpoint, PendingMeaningfulSave, RecoveryBoundaryEvidence,
    RecoveryEventUlid, RecoveryMachine, RecoveryMachineError, RecoveryPreserved,
    RecoveryProductStatus, RecoverySequence, RecoverySnapshot, RecoveryStamp, RecoveryStateError,
    RecoveryStatePersistence, RecoveryTransition, RecoveryTrigger, RecoveryTriggerInput,
    RecoveryWindow, TriggerEffect, RECOVERY_PRESERVATION_CONTRACT,
};
pub use crate::row::{Row, Value};
pub use crate::schema::{
    table, table_names_in_ddl, Column, ColumnDomain, ColumnType, Provenance, Table, TABLES,
    UNINDEXED_MESH_TYPES_IDS,
};
pub use crate::sequence::{
    ChunkPromoter, CrashResidue, DurableCommit, OutboxEntry, PrivateSaved, ReplicationHandoff,
    Saved, SequenceError, SequenceStep,
};
pub use crate::sql::{
    connection_pragmas_sql, read_all_tables, PendingPrivateSaveError, Pragma, SqlExecutor, Store,
    StoreError, PRAGMAS,
};
pub use crate::sqlite_driver::{
    Sqlite, SqliteError, SqliteRecoveryState, SqliteRecoveryStateError, RECOVERY_DATABASE_FILE_NAME,
};

pub use crate::record::{
    AckRecord, ApprovalRecord, ChunkSlice, ContextAccess, ContextRecord, DependencyKind,
    DependencyRecord, ManifestRecord, OperationRecord, PeerRecord, RecordKind, ReviewRecord,
    ReviewVerdict, StoredRecord,
};

mod record;

/// The crate's name, so a placeholder still carries one verifiable behaviour.
pub const CRATE_NAME: &str = "mesh-store";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_stable() {
        assert_eq!(CRATE_NAME, "mesh-store");
    }
}
