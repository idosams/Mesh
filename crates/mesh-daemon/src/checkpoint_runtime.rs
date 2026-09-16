//! Daemon-side routing into the store-owned checkpoint coordinator.
//!
//! The production composition is opt-in: `LiveDaemon` installs this coordinator only after all
//! three non-zero parameters were supplied. Its state is owned by `mesh-store` in a separate
//! `.mesh-recovery.sqlite` database so repair of the record-derived `metadata.sqlite` cannot erase
//! it; an absent configuration remains observably disabled.

use core::fmt;

use mesh_materializer::{BoundaryObservationV1, BoundaryReasonV1};
use mesh_store::{
    BoundaryEvidenceKind, CheckpointCoordinator, CheckpointRuntimeError, RecoveryBoundaryEvidence,
    RecoverySequence, RecoverySnapshot, RecoveryTrigger, SqliteRecoveryState,
    SqliteRecoveryStateError, RECOVERY_DATABASE_FILE_NAME,
};

/// The single workspace view currently composed by the daemon.
pub(crate) const LIVE_WORKSPACE_VIEW: &[u8] = b"live-daemon/workspace";

pub(crate) fn recovery_database(index_database: &std::path::Path) -> std::path::PathBuf {
    index_database.with_file_name(RECOVERY_DATABASE_FILE_NAME)
}

/// The store-owned production coordinator installed by [`crate::LiveDaemon`].
pub type AutomaticCheckpointRuntime = CheckpointCoordinator<SqliteRecoveryState>;

/// Whether automatic checkpointing has explicit authority to run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutomaticCheckpointStatus {
    /// No threshold configuration was supplied; nothing is scheduled or persisted.
    DisabledNoConfiguration,
    /// Configuration is valid, but no workspace database is open yet.
    WaitingForWorkspace,
    /// A configured coordinator is restored and ready for trigger facts.
    Active,
}

/// Why an automatic-checkpoint operation could not be routed.
#[derive(Debug)]
pub enum AutomaticCheckpointError {
    /// No threshold configuration was supplied.
    DisabledNoConfiguration,
    /// No workspace has been opened for the configured runtime.
    NoWorkspace,
    /// The store-owned coordinator refused the operation.
    Runtime(CheckpointRuntimeError<SqliteRecoveryStateError>),
    /// Persisted pending acknowledgement data did not match immutable journal/index truth.
    PendingAcknowledgementInvalid(String),
    /// The immutable journal prefix could not be bound into a recovery-only record.
    RecoveryArtifactInvalid(String),
    /// A prior restart-settlement refusal must be repaired before checkpoint state changes again.
    RecoveryNeedsAttention,
    /// The installed coordinator and held workspace do not name the same database.
    WorkspaceChanged,
}

impl fmt::Display for AutomaticCheckpointError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DisabledNoConfiguration => {
                formatter.write_str("automatic checkpointing has no threshold configuration")
            }
            Self::NoWorkspace => {
                formatter.write_str("automatic checkpointing has no open workspace")
            }
            Self::Runtime(error) => error.fmt(formatter),
            Self::PendingAcknowledgementInvalid(detail) => write!(
                formatter,
                "the pending private save could not be verified after restart: {detail}"
            ),
            Self::RecoveryArtifactInvalid(detail) => write!(
                formatter,
                "the durable workspace could not be bound into a recovery record: {detail}"
            ),
            Self::RecoveryNeedsAttention => {
                formatter.write_str(crate::user_messages::CHECKPOINT_RECOVERY_NEEDS_ATTENTION)
            }
            Self::WorkspaceChanged => {
                formatter.write_str(crate::user_messages::CHECKPOINT_WORKSPACE_CHANGED)
            }
        }
    }
}

impl std::error::Error for AutomaticCheckpointError {}

impl AutomaticCheckpointError {
    pub(crate) fn snapshot_unavailable(status: AutomaticCheckpointStatus) -> Self {
        match status {
            AutomaticCheckpointStatus::DisabledNoConfiguration => Self::DisabledNoConfiguration,
            AutomaticCheckpointStatus::WaitingForWorkspace => Self::NoWorkspace,
            AutomaticCheckpointStatus::Active => unreachable!("active runtime has a snapshot"),
        }
    }
}

pub(crate) fn snapshot(runtime: &AutomaticCheckpointRuntime) -> RecoverySnapshot {
    runtime.machine().snapshot().clone()
}

/// Daemon facts that immediately preserve recovery bytes without closing a meaningful window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryRuntimeSignal {
    /// An integrated agent asked the daemon to flush its work.
    IntegratedFlush,
    /// The actor process ended.
    ActorProcessExited,
    /// Review was opened for the current work.
    ReviewOpened,
    /// The actor connection ended.
    ActorDisconnected,
}

impl RecoveryRuntimeSignal {
    /// The fixed store trigger for this daemon signal.
    #[must_use]
    pub const fn trigger(self) -> RecoveryTrigger {
        match self {
            Self::IntegratedFlush => RecoveryTrigger::IntegratedAgentRequestsFlush,
            Self::ActorProcessExited => RecoveryTrigger::ActorProcessExited,
            Self::ReviewOpened => RecoveryTrigger::UserOpenedReview,
            Self::ActorDisconnected => RecoveryTrigger::ActorDisconnected,
        }
    }
}

/// Result of routing adapter boundary truth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoundaryRouting {
    /// The event carried no candidate.
    None,
    /// Exact candidate evidence for the store coordinator.
    Evidence {
        /// The corresponding recovery trigger.
        trigger: RecoveryTrigger,
        /// Exact sequence and reason.
        evidence: RecoveryBoundaryEvidence,
    },
    /// The backend explicitly could not prove the candidate.
    Unsupported,
}

/// Convert adapter-v1 boundary truth without inventing an event or sequence.
#[must_use]
pub fn route_boundary(observation: &BoundaryObservationV1) -> BoundaryRouting {
    let BoundaryObservationV1::Candidate(candidate) = observation else {
        return match observation {
            BoundaryObservationV1::None => BoundaryRouting::None,
            BoundaryObservationV1::Unsupported(_) => BoundaryRouting::Unsupported,
            BoundaryObservationV1::Candidate(_) => unreachable!(),
        };
    };
    let Some(through) = RecoverySequence::new(candidate.through().number()) else {
        return BoundaryRouting::Unsupported;
    };
    let (trigger, kind) = match candidate.reason() {
        BoundaryReasonV1::Closed => (
            RecoveryTrigger::ModifiedFileHandleClosed,
            BoundaryEvidenceKind::Closed,
        ),
        BoundaryReasonV1::Synced => (
            RecoveryTrigger::FsyncCompleted,
            BoundaryEvidenceKind::Synced,
        ),
        BoundaryReasonV1::RenamedIntoPlace => (
            RecoveryTrigger::AtomicReplacementCompleted,
            BoundaryEvidenceKind::RenamedIntoPlace,
        ),
    };
    BoundaryRouting::Evidence {
        trigger,
        evidence: RecoveryBoundaryEvidence::new(through, kind),
    }
}
