//! The non-default coordinator drives each of the nine closed trigger classes.

#[path = "support/mod.rs"]
mod support;

use std::time::Duration;

use mesh_store::{
    BoundaryEvidenceKind, CheckpointCoordinator, CheckpointRuntimeConfigError,
    CheckpointRuntimeError, CheckpointRuntimeParameters, RecordDigest, RecoveryBoundaryEvidence,
    RecoveryEventUlid, RecoveryPreserved, RecoveryProductStatus, RecoverySequence, RecoveryStamp,
    RecoveryTrigger, SELECTED_CHECKPOINT_IDLE_INTERVAL, SELECTED_MAXIMUM_UNCHECKPOINTED_BYTES,
    SELECTED_MAXIMUM_UNCHECKPOINTED_INTERVAL,
};
use support::checkpoint_runtime::{private_saved, MemoryPersistence};

fn sequence(value: u64) -> RecoverySequence {
    RecoverySequence::new(value).expect("non-zero sequence")
}

fn digest(value: u8) -> RecordDigest {
    RecordDigest::from_bytes([value; 32])
}

fn stamp(value: u8) -> RecoveryStamp {
    RecoveryStamp::new(
        u64::from(value),
        RecoveryEventUlid::from_bytes([value; 16]),
        digest(value),
    )
}

fn recovery(value: u8, through: u64) -> RecoveryPreserved {
    RecoveryPreserved::from_verified_bytes(
        stamp(value),
        sequence(through),
        vec![value],
        digest(value),
    )
    .expect("verified recovery")
}

fn parameters() -> CheckpointRuntimeParameters {
    CheckpointRuntimeParameters {
        idle_interval: Some(Duration::from_secs(3)),
        maximum_uncheckpointed_bytes: Some(8),
        maximum_uncheckpointed_interval: Some(Duration::from_secs(9)),
    }
}

#[test]
fn configuration_has_no_implicit_or_zero_values() {
    assert!(matches!(
        CheckpointCoordinator::open(
            MemoryPersistence::default(),
            CheckpointRuntimeParameters::default()
        ),
        Err(CheckpointRuntimeError::Configuration(
            CheckpointRuntimeConfigError::MissingIdleInterval
        ))
    ));
    let mut invalid = parameters();
    invalid.maximum_uncheckpointed_bytes = Some(0);
    assert!(matches!(
        CheckpointCoordinator::open(MemoryPersistence::default(), invalid),
        Err(CheckpointRuntimeError::Configuration(
            CheckpointRuntimeConfigError::ZeroMaximumBytes
        ))
    ));
}

#[test]
fn selected_defaults_are_the_exact_ratified_scheduler_values() {
    let selected = CheckpointRuntimeParameters::selected_defaults();
    assert_eq!(
        selected.idle_interval,
        Some(SELECTED_CHECKPOINT_IDLE_INTERVAL)
    );
    assert_eq!(
        selected.maximum_uncheckpointed_bytes,
        Some(SELECTED_MAXIMUM_UNCHECKPOINTED_BYTES)
    );
    assert_eq!(
        selected.maximum_uncheckpointed_interval,
        Some(SELECTED_MAXIMUM_UNCHECKPOINTED_INTERVAL)
    );
    assert_eq!(SELECTED_CHECKPOINT_IDLE_INTERVAL, Duration::from_millis(50));
    assert_eq!(SELECTED_MAXIMUM_UNCHECKPOINTED_BYTES, 65_536);
    assert_eq!(
        SELECTED_MAXIMUM_UNCHECKPOINTED_INTERVAL,
        Duration::from_millis(25)
    );
    CheckpointCoordinator::open(MemoryPersistence::default(), selected)
        .expect("ratified values are valid");
}

#[test]
fn all_nine_triggers_are_routed_without_early_meaningful_acknowledgement() {
    let mut coordinator =
        CheckpointCoordinator::open(MemoryPersistence::default(), parameters()).unwrap();
    for value in 1..=10 {
        coordinator.observe(sequence(value), 1).unwrap();
    }

    for (trigger, kind) in [
        (
            RecoveryTrigger::ModifiedFileHandleClosed,
            BoundaryEvidenceKind::Closed,
        ),
        (
            RecoveryTrigger::FsyncCompleted,
            BoundaryEvidenceKind::Synced,
        ),
        (
            RecoveryTrigger::AtomicReplacementCompleted,
            BoundaryEvidenceKind::RenamedIntoPlace,
        ),
    ] {
        let transition = coordinator
            .record_boundary(trigger, RecoveryBoundaryEvidence::new(sequence(7), kind))
            .unwrap();
        assert!(transition.private_saved().is_none());
    }

    for (value, trigger) in [
        RecoveryTrigger::IntegratedAgentRequestsFlush,
        RecoveryTrigger::ActorProcessExited,
        RecoveryTrigger::UserOpenedReview,
        RecoveryTrigger::ActorDisconnected,
    ]
    .into_iter()
    .enumerate()
    {
        let transition = coordinator
            .preserve(trigger, recovery(value as u8 + 1, value as u64 + 1))
            .unwrap();
        assert!(transition.private_saved().is_none());
    }

    assert!(!coordinator.recovery_due(Duration::ZERO));
    coordinator.observe(sequence(10), 8).unwrap();
    assert!(coordinator.recovery_due(Duration::ZERO));
    let maximum = coordinator
        .preserve_at_maximum(Duration::ZERO, recovery(8, 8))
        .unwrap()
        .expect("byte maximum fired");
    assert!(maximum.private_saved().is_none());
    assert_eq!(
        coordinator.machine().status(),
        RecoveryProductStatus::Working
    );

    assert!(coordinator.settled_window(Duration::from_secs(2)).is_none());
    let settled_window = coordinator
        .settled_window(Duration::from_secs(3))
        .expect("configured interval reached");
    let settled = coordinator
        .save_settled(settled_window, stamp(20), private_saved())
        .unwrap();
    assert!(settled.private_saved().is_some());
    assert_eq!(
        coordinator.machine().status(),
        RecoveryProductStatus::SavedPrivately
    );
}

#[test]
fn recovery_only_state_restores_with_the_window_still_open() {
    let persistence = MemoryPersistence::default();
    let mut coordinator = CheckpointCoordinator::open(persistence.clone(), parameters()).unwrap();
    coordinator.observe(sequence(1), 4).unwrap();
    coordinator.observe(sequence(2), 4).unwrap();
    coordinator
        .preserve_at_maximum(Duration::from_secs(9), recovery(1, 2))
        .unwrap()
        .expect("configured interval preserves recovery state");
    drop(coordinator);

    let mut restarted = CheckpointCoordinator::open(persistence, parameters()).unwrap();
    assert_eq!(
        restarted.machine().snapshot().open_window().unwrap().last(),
        sequence(2)
    );
    assert!(restarted.machine().snapshot().latest_recovery().is_some());
    let settled = restarted
        .settled_window(Duration::from_secs(3))
        .expect("restart window settles");
    let transition = restarted
        .save_settled(settled, stamp(2), private_saved())
        .unwrap();
    assert!(transition.private_saved().is_some());
}

#[test]
fn a_pending_durable_save_rejects_a_caller_supplied_stamp_after_restart() {
    let persistence = MemoryPersistence::default();
    let acknowledgement = private_saved();
    let mut coordinator = CheckpointCoordinator::open(persistence.clone(), parameters()).unwrap();
    coordinator
        .observe_durable(sequence(1), 4, stamp(1), acknowledgement)
        .expect("journal-backed save observation");
    drop(coordinator);

    let mut restarted = CheckpointCoordinator::open(persistence, parameters()).unwrap();
    let settled = restarted
        .settled_window(Duration::from_secs(3))
        .expect("pending extent settles");
    assert!(matches!(
        restarted.save_settled(settled, stamp(2), acknowledgement),
        Err(CheckpointRuntimeError::PendingAcknowledgementMismatch)
    ));
    assert!(restarted.machine().snapshot().open_window().is_some());
    assert!(restarted
        .save_verified_pending(settled, acknowledgement)
        .expect("persisted stamp and verified acknowledgement settle")
        .private_saved()
        .is_some());
}

#[test]
fn a_prior_recovery_snapshot_remains_valid_during_the_next_durable_save() {
    let persistence = MemoryPersistence::default();
    let acknowledgement = private_saved();
    let mut coordinator =
        CheckpointCoordinator::open(persistence.clone(), parameters()).expect("coordinator");

    coordinator.observe(sequence(1), 4).expect("first window");
    coordinator
        .preserve(RecoveryTrigger::ActorDisconnected, recovery(1, 1))
        .expect("recovery-only checkpoint");
    let first = coordinator
        .settled_window(Duration::from_secs(3))
        .expect("first window settles");
    coordinator
        .save_settled(first, stamp(2), acknowledgement)
        .expect("first meaningful save");

    // A direct journal-backed save can open the next window without first producing a newer
    // recovery-only snapshot. The prior recovery remains a valid fallback for the already
    // meaningful prefix while this new save is pending. Process loss here must therefore reopen
    // the durable state instead of treating a normal product sequence as corruption.
    coordinator
        .observe_durable(sequence(2), 4, stamp(3), acknowledgement)
        .expect("second durable save observation");
    drop(coordinator);

    let restarted = CheckpointCoordinator::open(persistence, parameters())
        .expect("prior recovery and a newer open window are compatible");
    assert_eq!(
        restarted
            .machine()
            .snapshot()
            .latest_recovery()
            .expect("retained recovery")
            .through(),
        sequence(1)
    );
    assert_eq!(
        restarted
            .machine()
            .snapshot()
            .open_window()
            .expect("new pending window")
            .from(),
        sequence(2)
    );
}
