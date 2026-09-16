//! Production composition and restart coverage for the automatic checkpoint coordinator.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use mesh_daemon::ipc::surface::{nothing_to_recover, StartupSummary};
use mesh_daemon::{AutomaticCheckpointError, AutomaticCheckpointStatus, LiveDaemon};
use mesh_store::{
    CheckpointRuntimeConfigError, CheckpointRuntimeParameters, RecordDigest, RecoveryEventUlid,
    RecoveryPreserved, RecoverySequence, RecoveryStamp, RecoveryTransition, RecoveryTrigger,
};

fn scratch(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "mesh-daemon-checkpoint-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ))
}

fn startup() -> StartupSummary {
    StartupSummary::from(&nothing_to_recover())
}

fn parameters() -> CheckpointRuntimeParameters {
    CheckpointRuntimeParameters {
        idle_interval: Some(Duration::from_millis(8)),
        maximum_uncheckpointed_bytes: Some(16),
        maximum_uncheckpointed_interval: Some(Duration::from_millis(20)),
    }
}

fn stamp(byte: u8) -> RecoveryStamp {
    RecoveryStamp::new(
        u64::from(byte),
        RecoveryEventUlid::from_bytes([byte; 16]),
        RecordDigest::from_bytes([byte; 32]),
    )
}

#[test]
fn absent_or_partial_configuration_never_enables_a_default() {
    let root = scratch("disabled");
    let _ = fs::remove_dir_all(&root);
    let daemon = LiveDaemon::new(startup());
    daemon.open_at_start(&root).expect("workspace opens");
    assert_eq!(
        daemon.automatic_checkpoint_status(),
        AutomaticCheckpointStatus::DisabledNoConfiguration
    );
    assert!(matches!(
        daemon.observe_checkpoint_activity(RecoverySequence::new(1).expect("sequence"), 1),
        Err(AutomaticCheckpointError::DisabledNoConfiguration)
    ));

    let partial = CheckpointRuntimeParameters {
        idle_interval: Some(Duration::from_millis(1)),
        ..CheckpointRuntimeParameters::default()
    };
    assert!(matches!(
        LiveDaemon::with_checkpoint_runtime(startup(), partial),
        Err(CheckpointRuntimeConfigError::MissingMaximumBytes)
    ));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn daemon_restart_restores_open_recovery_without_fabricating_a_meaningful_acknowledgement() {
    let root = scratch("restart");
    let _ = fs::remove_dir_all(&root);
    let first = RecoverySequence::new(1).expect("sequence");
    let last = RecoverySequence::new(2).expect("sequence");

    {
        let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
        assert_eq!(
            daemon.automatic_checkpoint_status(),
            AutomaticCheckpointStatus::WaitingForWorkspace
        );
        daemon.open_at_start(&root).expect("workspace opens");
        assert_eq!(
            daemon.automatic_checkpoint_status(),
            AutomaticCheckpointStatus::Active
        );
        daemon
            .observe_checkpoint_activity(first, 8)
            .expect("first event");
        daemon
            .observe_checkpoint_activity(last, 8)
            .expect("last event");
        let recovery = RecoveryPreserved::from_verified_bytes(
            stamp(7),
            last,
            b"verified recovery".to_vec(),
            RecordDigest::from_bytes([7; 32]),
        )
        .expect("verified bytes");
        assert!(daemon
            .preserve_recovery_at_maximum(Duration::ZERO, recovery)
            .expect("maximum trigger")
            .is_some());
    }

    {
        let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
        daemon.open_at_start(&root).expect("workspace reopens");
        let restored = daemon.checkpoint_snapshot().expect("restored snapshot");
        let window = restored.open_window().expect("open window survives");
        assert_eq!((window.from(), window.last()), (first, last));
        assert_eq!(
            restored
                .latest_recovery()
                .expect("recovery survives")
                .bytes(),
            b"verified recovery"
        );

        assert!(restored.last_meaningful().is_none());
    }

    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&root).expect("second reopen");
    let restored = daemon.checkpoint_snapshot().expect("restored snapshot");
    assert_eq!(
        restored.open_window().expect("window remains open").last(),
        last
    );
    assert!(restored.last_meaningful().is_none());
    assert_eq!(
        restored
            .latest_recovery()
            .expect("recovery remains")
            .through(),
        last
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn immediate_recovery_trigger_is_durable_through_live_daemon() {
    let root = scratch("immediate");
    let _ = fs::remove_dir_all(&root);
    let sequence = RecoverySequence::new(1).expect("sequence");
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&root).expect("workspace");
    daemon
        .observe_checkpoint_activity(sequence, 1)
        .expect("event");
    let transition = daemon
        .preserve_recovery(
            RecoveryTrigger::ActorDisconnected,
            RecoveryPreserved::from_verified_bytes(
                stamp(5),
                sequence,
                vec![5],
                RecordDigest::from_bytes([5; 32]),
            )
            .expect("verified"),
        )
        .expect("preserved");
    assert!(matches!(
        transition,
        RecoveryTransition::RecoveryPreserved(_)
    ));
    drop(daemon);

    let reopened = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    reopened.open_at_start(&root).expect("workspace reopens");
    assert_eq!(
        reopened
            .checkpoint_snapshot()
            .expect("snapshot")
            .latest_recovery()
            .expect("recovery")
            .bytes(),
        &[5]
    );
    let _ = fs::remove_dir_all(&root);
}
