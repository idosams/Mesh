//! Product composition coverage for typed saves routed through `LiveDaemon`.

#![cfg(unix)]

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use mesh_cas::Cas;
use mesh_chunking::ChunkingConfig;
use mesh_daemon::ipc::surface::{nothing_to_recover, Operations, StartupSummary};
use mesh_daemon::workspace::OpenWorkspace;
use mesh_daemon::{
    save_file_version, AutomaticCheckpointError, FileVersionCheckpointRequest,
    LiveCheckpointSaveError, LiveDaemon, ManagedTextFileError, ManifestPagingPolicy,
    PreparedFolderImport,
};
use mesh_operations::{
    ActorId, ActorSequence, CausalParents, HeadDerivation, HeadId, Hlc, ObjectId, PolicyEpoch,
    PortableMetadata, SessionId, Signature, TransitionCommitment, VersionId, WorkspaceId,
};
use mesh_store::{
    BoundaryEvidenceKind, CheckpointRuntimeParameters, RecordDigest, RecoveryBoundaryEvidence,
    RecoveryEventUlid, RecoveryPreserved, RecoverySequence, RecoveryStamp, RecoveryTransition,
    RecoveryTrigger, SqlExecutor as _, Sqlite, SqliteRecoveryState, RECOVERY_DATABASE_FILE_NAME,
};
use mesh_types::{Blake3, ContentDigest as _, PublicKey};

fn scratch(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "mesh-live-save-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ))
}

fn startup() -> StartupSummary {
    StartupSummary::from(&nothing_to_recover())
}

// Failure-only evidence from this synthetic fixture. Keep the diagnostic limited to the
// scheduler instead of printing unrelated workspace state, and do not acquire new authority.
fn worker_diagnostic(daemon: &LiveDaemon) -> String {
    let diagnostic = format!("{daemon:?}");
    let scheduler = diagnostic
        .split_once(", checkpoint_idle: ")
        .and_then(|(_, tail)| tail.split_once(", workspace_open: "))
        .map_or("scheduler debug state unavailable", |(scheduler, _)| {
            scheduler
        });
    format!("observed_at={:?}; {scheduler}", std::time::Instant::now())
}

fn parameters() -> CheckpointRuntimeParameters {
    CheckpointRuntimeParameters {
        idle_interval: Some(Duration::from_millis(8)),
        maximum_uncheckpointed_bytes: Some(64 * 1024),
        maximum_uncheckpointed_interval: Some(Duration::from_millis(20)),
    }
}

fn chunking() -> ChunkingConfig {
    ChunkingConfig::always_chunked(64, 256, 1024).expect("explicit policy")
}

fn content(seed: u8) -> Vec<u8> {
    (0..8192)
        .map(|index| seed.wrapping_add((index as u8).rotate_left((index % 7) as u32)))
        .collect()
}

struct CanonicalHead;

impl HeadDerivation for CanonicalHead {
    fn resulting_head(&self, commitment: &TransitionCommitment) -> HeadId {
        HeadId::from_bytes(*Blake3::digest_bytes(&commitment.canonical_bytes()).as_bytes())
    }
}

#[test]
fn shared_agent_custody_refuses_background_file_version_save_until_release() {
    let root = scratch("custody-refuses-checkpoint");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("workspace");
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    let summary = daemon.open_at_start(&root).expect("workspace");
    let generation = daemon
        .acquire_workspace_agent_custody(
            &summary.root,
            &summary.digest,
            &summary.installation,
            false,
            None,
        )
        .expect("acquire shared custody");

    let refused = daemon.save_file_version(
        RecoverySequence::new(1).expect("sequence"),
        &content(41),
        &chunking(),
        ManifestPagingPolicy::flat(),
        request(1, 41),
        &CanonicalHead,
    );
    assert!(matches!(
        refused,
        Err(LiveCheckpointSaveError::WorkspaceAuthority(
            ManagedTextFileError::Recovery(_)
        ))
    ));
    assert_eq!(
        daemon
            .current_workspace_summary()
            .expect("unchanged workspace")
            .records,
        summary.records
    );

    daemon
        .release_workspace_agent_custody(
            &summary.root,
            &summary.digest,
            &summary.installation,
            &generation,
        )
        .expect("release shared custody");
    daemon
        .save_file_version(
            RecoverySequence::new(1).expect("sequence"),
            &content(41),
            &chunking(),
            ManifestPagingPolicy::flat(),
            request(1, 41),
            &CanonicalHead,
        )
        .expect("save after release");
    fs::remove_dir_all(root).unwrap();
}

fn request(sequence: u64, version: u8) -> FileVersionCheckpointRequest {
    request_for_actor(sequence, version, 2)
}

fn request_for_actor(sequence: u64, version: u8, actor: u8) -> FileVersionCheckpointRequest {
    FileVersionCheckpointRequest::new(
        WorkspaceId::from_bytes([1; 16]),
        ActorId::from_bytes([actor; 32]),
        SessionId::from_bytes([actor.wrapping_add(1); 16]),
        ActorSequence::new(sequence),
        CausalParents::genesis(),
        HeadId::from_bytes([4; 32]),
        PolicyEpoch::new(5),
        Hlc::new(1_700_000_000_000 + sequence, 6),
        ObjectId::from_bytes([7; 16]),
        VersionId::from_bytes([version; 32]),
        Vec::new(),
        PortableMetadata::new(true),
        Signature::from_bytes([10; 64]),
    )
}

const PROCESS_LOSS_ROOT: &str = "MESH_PENDING_SETTLEMENT_PROCESS_LOSS_ROOT";

#[test]
fn process_loses_private_saved_after_the_durable_observation_child() {
    let Ok(root) = std::env::var(PROCESS_LOSS_ROOT) else {
        return;
    };
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon
        .open_at_start(PathBuf::from(root).as_path())
        .expect("workspace");
    daemon
        .save_file_version(
            RecoverySequence::new(1).expect("sequence"),
            &content(31),
            &chunking(),
            ManifestPagingPolicy::flat(),
            request(1, 31),
            &CanonicalHead,
        )
        .expect("journal and pending acknowledgement become durable");
    std::process::exit(73);
}

#[test]
fn a_real_process_loss_resumes_the_pending_save_without_the_old_token() {
    let root = scratch("real-process-loss");
    let _ = fs::remove_dir_all(&root);
    let output = Command::new(std::env::current_exe().expect("current test binary"))
        .args([
            "--exact",
            "process_loses_private_saved_after_the_durable_observation_child",
            "--nocapture",
        ])
        .env(PROCESS_LOSS_ROOT, &root)
        .output()
        .expect("spawn process-loss child");
    assert_eq!(output.status.code(), Some(73), "{output:?}");

    let restarted = Arc::new(
        LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("restart config"),
    );
    restarted.open_at_start(&root).expect("restart workspace");
    let recovered = restarted
        .checkpoint_snapshot()
        .expect("restart recovery state");
    assert_eq!(
        recovered
            .latest_recovery()
            .expect("restart immediately preserves the pending durable prefix")
            .through(),
        RecoverySequence::new(1).expect("sequence")
    );
    assert!(recovered.last_meaningful().is_none());
    assert!(restarted
        .schedule_pending_checkpoint()
        .expect("startup sees pending save")
        .join()
        .expect("worker does not panic")
        .expect("rebuild verifies pending save")
        .is_some());
    let snapshot = restarted.checkpoint_snapshot().expect("settled state");
    assert!(snapshot.open_window().is_none());
    assert!(snapshot.pending_meaningful().is_none());
    assert_eq!(
        snapshot
            .last_meaningful()
            .expect("meaningful save recovered")
            .through(),
        RecoverySequence::new(1).expect("sequence")
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_successful_public_durable_save_schedules_its_idle_settlement() {
    let root = scratch("automatic-idle-settlement");
    let _ = fs::remove_dir_all(&root);
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&root).expect("workspace");
    let sequence = RecoverySequence::new(1).expect("sequence");

    daemon
        .save_file_version(
            sequence,
            &content(32),
            &chunking(),
            ManifestPagingPolicy::flat(),
            request(1, 32),
            &CanonicalHead,
        )
        .expect("durable save");

    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    loop {
        let snapshot = daemon.checkpoint_snapshot().expect("checkpoint snapshot");
        if snapshot
            .last_meaningful()
            .is_some_and(|checkpoint| checkpoint.through() == sequence)
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the public save returned but no automatic idle worker settled it"
        );
        std::thread::sleep(Duration::from_millis(2));
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_caller_cannot_fabricate_the_idle_interval() {
    let root = scratch("caller-fabricated-idle");
    let _ = fs::remove_dir_all(&root);
    let runtime = CheckpointRuntimeParameters {
        idle_interval: Some(Duration::from_millis(200)),
        ..parameters()
    };
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), runtime).expect("config");
    daemon.open_at_start(&root).expect("workspace");
    let sequence = RecoverySequence::new(1).expect("sequence");

    daemon
        .save_file_version(
            sequence,
            &content(32),
            &chunking(),
            ManifestPagingPolicy::flat(),
            request(1, 32),
            &CanonicalHead,
        )
        .expect("durable save");

    std::thread::sleep(Duration::from_millis(20));
    assert!(
        daemon
            .checkpoint_snapshot()
            .expect("checkpoint state")
            .last_meaningful()
            .is_none(),
        "the daemon settled before its monotonic idle timer elapsed"
    );

    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    loop {
        if daemon
            .checkpoint_snapshot()
            .expect("checkpoint state")
            .last_meaningful()
            .is_some_and(|checkpoint| checkpoint.through() == sequence)
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the scheduler never settled after the real idle interval"
        );
        std::thread::sleep(Duration::from_millis(2));
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn repeated_saves_share_one_resettable_idle_worker() {
    let root = scratch("coalesced-idle-worker");
    let _ = fs::remove_dir_all(&root);
    let runtime = CheckpointRuntimeParameters {
        idle_interval: Some(Duration::from_millis(80)),
        ..parameters()
    };
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), runtime).expect("config");
    daemon.open_at_start(&root).expect("workspace");

    daemon
        .save_file_version(
            RecoverySequence::new(1).expect("first sequence"),
            &content(33),
            &chunking(),
            ManifestPagingPolicy::flat(),
            request_for_actor(1, 33, 2),
            &CanonicalHead,
        )
        .expect("first durable save starts the worker");
    daemon
        .save_file_version(
            RecoverySequence::new(2).expect("second sequence"),
            &content(34),
            &chunking(),
            ManifestPagingPolicy::flat(),
            request_for_actor(1, 34, 13),
            &CanonicalHead,
        )
        .expect("second durable save resets the same worker");

    assert!(
        daemon.schedule_pending_checkpoint().is_none(),
        "one pending extent must not create another detached sleeper"
    );

    let recovery_deadline = std::time::Instant::now() + Duration::from_millis(70);
    loop {
        let snapshot = daemon.checkpoint_snapshot().expect("checkpoint snapshot");
        if snapshot
            .latest_recovery()
            .is_some_and(|recovery| recovery.through().get() == 2)
        {
            assert!(snapshot.last_meaningful().is_none());
            break;
        }
        assert!(
            std::time::Instant::now() < recovery_deadline,
            "the coalesced worker preserved an old extent or missed the maximum interval"
        );
        std::thread::sleep(Duration::from_millis(2));
    }

    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    loop {
        let snapshot = daemon.checkpoint_snapshot().expect("checkpoint snapshot");
        if snapshot
            .last_meaningful()
            .is_some_and(|checkpoint| checkpoint.through().get() == 2)
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the coalesced worker never settled the newest extent"
        );
        std::thread::sleep(Duration::from_millis(2));
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_new_pending_save_restarts_with_the_prior_recovery_snapshot_retained() {
    let root = scratch("prior-recovery-next-save");
    let _ = fs::remove_dir_all(&root);
    let first = RecoverySequence::new(1).expect("first sequence");
    let second = RecoverySequence::new(2).expect("second sequence");

    {
        let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
        daemon.open_at_start(&root).expect("workspace");
        daemon
            .save_file_version(
                first,
                &content(51),
                &chunking(),
                ManifestPagingPolicy::flat(),
                request(1, 51),
                &CanonicalHead,
            )
            .expect("first durable save");
        daemon
            .preserve_recovery(
                RecoveryTrigger::ActorDisconnected,
                RecoveryPreserved::from_verified_bytes(
                    stamp(5),
                    first,
                    vec![5],
                    RecordDigest::from_bytes([5; 32]),
                )
                .expect("verified recovery"),
            )
            .expect("retain recovery for the first prefix");
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        loop {
            if daemon
                .checkpoint_snapshot()
                .expect("checkpoint state")
                .last_meaningful()
                .is_some_and(|checkpoint| checkpoint.through() == first)
            {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the first save never became meaningful"
            );
            std::thread::sleep(Duration::from_millis(2));
        }

        daemon
            .save_file_version(
                second,
                &content(52),
                &chunking(),
                ManifestPagingPolicy::flat(),
                request(2, 52),
                &CanonicalHead,
            )
            .expect("next durable save opens a new pending window");
        let snapshot = daemon.checkpoint_snapshot().expect("pending state");
        assert_eq!(
            snapshot
                .latest_recovery()
                .expect("prior retained recovery")
                .through(),
            first
        );
        assert_eq!(snapshot.open_window().expect("next window").from(), second);
    }

    let restarted = Arc::new(
        LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("restart config"),
    );
    restarted
        .open_at_start(&root)
        .expect("the reachable pending state must decode after process loss");
    restarted
        .schedule_pending_checkpoint()
        .expect("pending save schedules")
        .join()
        .expect("worker")
        .expect("verified restart settlement")
        .expect("pending save settles");
    let snapshot = restarted.checkpoint_snapshot().expect("settled restart");
    assert_eq!(
        snapshot
            .last_meaningful()
            .expect("new meaningful checkpoint")
            .through(),
        second
    );
    assert_eq!(
        snapshot
            .latest_recovery()
            .expect("the higher-stamped prior recovery remains retained")
            .through(),
        first
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_workspace_opened_after_startup_resumes_its_pending_save_without_an_extra_call() {
    let root = scratch("post-startup-open");
    let _ = fs::remove_dir_all(&root);
    {
        let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
        daemon.open_at_start(&root).expect("workspace");
        daemon
            .save_file_version(
                RecoverySequence::new(1).expect("sequence"),
                &content(41),
                &chunking(),
                ManifestPagingPolicy::flat(),
                request(1, 41),
                &CanonicalHead,
            )
            .expect("pending durable save");
    }

    let restarted =
        LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("restart config");
    Operations::open_workspace(&restarted, &root.display().to_string())
        .expect("workspace opens through the shipped post-startup surface");

    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    loop {
        let snapshot = restarted.checkpoint_snapshot().expect("checkpoint state");
        if snapshot.pending_meaningful().is_none() {
            assert!(snapshot.open_window().is_none());
            assert_eq!(
                snapshot
                    .last_meaningful()
                    .expect("pending save became meaningful")
                    .through(),
                RecoverySequence::new(1).expect("sequence")
            );
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "post-startup workspace.open restored the pending save but never scheduled it"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn maximum_byte_bound_preserves_recovery_before_the_idle_boundary() {
    let root = scratch("automatic-byte-recovery");
    let _ = fs::remove_dir_all(&root);
    let runtime = CheckpointRuntimeParameters {
        idle_interval: Some(Duration::from_millis(200)),
        maximum_uncheckpointed_bytes: Some(1),
        maximum_uncheckpointed_interval: Some(Duration::from_millis(150)),
    };
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), runtime).expect("config");
    daemon.open_at_start(&root).expect("workspace");

    daemon
        .save_file_version(
            RecoverySequence::new(1).expect("sequence"),
            &content(81),
            &chunking(),
            ManifestPagingPolicy::flat(),
            request(1, 81),
            &CanonicalHead,
        )
        .expect("durable save");

    let snapshot = daemon.checkpoint_snapshot().expect("checkpoint state");
    assert_eq!(
        snapshot
            .latest_recovery()
            .expect("the byte bound preserves recovery without another caller action")
            .through(),
        RecoverySequence::new(1).expect("sequence")
    );
    assert!(snapshot.last_meaningful().is_none());
    assert!(snapshot.open_window().is_some());
    assert!(snapshot.pending_meaningful().is_some());
    drop(daemon);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn maximum_time_bound_preserves_recovery_before_the_idle_boundary() {
    let root = scratch("automatic-time-recovery");
    let _ = fs::remove_dir_all(&root);
    let runtime = CheckpointRuntimeParameters {
        idle_interval: Some(Duration::from_millis(200)),
        maximum_uncheckpointed_bytes: Some(u64::MAX),
        maximum_uncheckpointed_interval: Some(Duration::from_millis(20)),
    };
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), runtime).expect("config");
    daemon.open_at_start(&root).expect("workspace");

    daemon
        .save_file_version(
            RecoverySequence::new(1).expect("sequence"),
            &content(82),
            &chunking(),
            ManifestPagingPolicy::flat(),
            request(1, 82),
            &CanonicalHead,
        )
        .expect("durable save");

    let deadline = std::time::Instant::now() + Duration::from_millis(150);
    loop {
        let snapshot = daemon.checkpoint_snapshot().expect("checkpoint state");
        if snapshot.latest_recovery().is_some() {
            assert!(snapshot.last_meaningful().is_none());
            assert!(snapshot.open_window().is_some());
            assert!(snapshot.pending_meaningful().is_some());
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the time bound elapsed without automatic recovery preservation; snapshot={snapshot:?}; worker={}; conditions={:?}",
            worker_diagnostic(&daemon),
            daemon.workspace_state().map(|state| state.conditions.iter().map(|condition| condition.code().to_owned()).collect::<Vec<_>>())
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    drop(daemon);

    let restarted =
        LiveDaemon::with_checkpoint_runtime(startup(), runtime).expect("restart config");
    restarted
        .open_at_start(&root)
        .expect("the recovery pointer authenticates its immutable journal prefix");
    let snapshot = restarted.checkpoint_snapshot().expect("restored state");
    assert_eq!(
        snapshot
            .latest_recovery()
            .expect("recovery survives process loss")
            .through(),
        RecoverySequence::new(1).expect("sequence")
    );
    assert!(snapshot.last_meaningful().is_none());
    assert!(snapshot.open_window().is_some());
    assert!(snapshot.pending_meaningful().is_some());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn continuous_activity_preserves_the_newest_extent_at_the_maximum_time_bound() {
    let root = scratch("automatic-time-recovery-continuous");
    let _ = fs::remove_dir_all(&root);
    let runtime = CheckpointRuntimeParameters {
        // The scheduler-state unit regression proves that later activity cannot move the original
        // maximum deadline. This process-level case separately proves that the worker preserves
        // the newest extent; give a loaded suite enough time to schedule the operating-system
        // thread instead of trying to infer internal deadline state from a narrow timing band.
        idle_interval: Some(Duration::from_secs(2)),
        maximum_uncheckpointed_bytes: Some(u64::MAX),
        maximum_uncheckpointed_interval: Some(Duration::from_secs(1)),
    };
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), runtime).expect("config");
    daemon.open_at_start(&root).expect("workspace");
    daemon
        .save_file_version(
            RecoverySequence::new(1).expect("sequence"),
            &content(84),
            &chunking(),
            ManifestPagingPolicy::flat(),
            request_for_actor(1, 84, 2),
            &CanonicalHead,
        )
        .expect("first durable save");
    std::thread::sleep(Duration::from_millis(500));
    daemon
        .save_file_version(
            RecoverySequence::new(2).expect("sequence"),
            &content(85),
            &chunking(),
            ManifestPagingPolicy::flat(),
            request_for_actor(1, 85, 13),
            &CanonicalHead,
        )
        .expect("continued activity resets idle but not the recovery maximum");

    let deadline = std::time::Instant::now() + Duration::from_secs(4);
    loop {
        let snapshot = daemon.checkpoint_snapshot().expect("checkpoint state");
        if snapshot
            .latest_recovery()
            .is_some_and(|recovery| recovery.through().get() == 2)
        {
            assert!(snapshot.last_meaningful().is_none());
            assert!(snapshot.open_window().is_some());
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "continued activity never preserved the newest extent at the maximum deadline"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    drop(daemon);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn opening_review_refuses_recovery_only_work_without_making_it_meaningful() {
    let root = scratch("review-open-recovery");
    let _ = fs::remove_dir_all(&root);
    let source = root.with_extension("source");
    fs::create_dir_all(&source).expect("source");
    fs::write(source.join("notes.txt"), b"first draft\n").expect("source file");
    let prepared = PreparedFolderImport::prepare(&source, &root).expect("prepare import");
    let (confirmed, _) = prepared.confirm_into_workspace().expect("confirm import");
    drop(confirmed);
    let runtime = CheckpointRuntimeParameters {
        idle_interval: Some(Duration::from_secs(2)),
        maximum_uncheckpointed_bytes: Some(u64::MAX),
        maximum_uncheckpointed_interval: Some(Duration::from_secs(60)),
    };
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), runtime).expect("config");
    daemon.open_at_start(&root).expect("workspace");
    let inspected = daemon
        .inspect_managed_file("notes.txt")
        .expect("imported file");
    daemon
        .preserve_managed_text_edit(
            "notes.txt",
            "second draft\n",
            inspected.content_digest(),
            inspected.executable(),
        )
        .expect("pending edit is preserved");
    assert!(daemon
        .checkpoint_snapshot()
        .expect("checkpoint state")
        .latest_recovery()
        .is_some());

    let before = daemon.workspace_state().expect("before review").records;
    let opened_by = PublicKey::from_bytes([92; 32]);
    let target = daemon
        .workspace_state()
        .expect("review target")
        .workspace_versions
        .last()
        .expect("import operation")
        .operation();
    let forged = daemon
        .open_review(
            &RecordDigest::from_bytes([91; 32]).to_string(),
            &target.to_string(),
            &RecordDigest::from_bytes([92; 32]).to_string(),
        )
        .expect_err("a caller-supplied bundle must not become review authority");
    assert_eq!(forged.code, "publication-review-bundle-mismatch");
    assert_eq!(
        daemon
            .workspace_state()
            .expect("forged review refused")
            .records,
        before,
        "a forged bundle must not append"
    );
    let shown = daemon.workspace_state().expect("review source");
    let refusal = daemon
        .open_current_review_for_workspace(
            &shown.root,
            &shown.digest,
            &shown.installation,
            opened_by,
        )
        .expect_err("recovery-only native work is not a reviewable saved version");
    assert_eq!(refusal.code, "publication-native-work-pending");

    let preserved = daemon.checkpoint_snapshot().expect("preserved checkpoint");
    assert_eq!(
        preserved
            .latest_recovery()
            .expect("review-open recovery")
            .through(),
        RecoverySequence::new(1).expect("sequence")
    );
    assert!(preserved.last_meaningful().is_none());
    assert!(preserved.open_window().is_some());
    assert_eq!(
        daemon.workspace_state().expect("review recorded").records,
        before,
        "refusing a stale saved-version review must not append"
    );

    let inspected = daemon
        .inspect_managed_file("notes.txt")
        .expect("first pending edit");
    daemon
        .preserve_managed_text_edit(
            "notes.txt",
            "third draft\n",
            inspected.content_digest(),
            inspected.executable(),
        )
        .expect("new native work extends the pending prefix");
    assert_eq!(
        daemon
            .checkpoint_snapshot()
            .expect("extended checkpoint")
            .latest_recovery()
            .expect("second native edit advances recovery")
            .through(),
        RecoverySequence::new(2).expect("second sequence")
    );
    let records_before_reopen = daemon
        .workspace_state()
        .expect("before reopening review")
        .records;
    let shown = daemon.workspace_state().expect("updated review source");
    let refusal = daemon
        .open_current_review_for_workspace(
            &shown.root,
            &shown.digest,
            &shown.installation,
            opened_by,
        )
        .expect_err("newer recovery-only work remains outside the saved review target");
    assert_eq!(refusal.code, "publication-native-work-pending");
    let reopened = daemon
        .checkpoint_snapshot()
        .expect("reopened checkpoint state");
    assert_eq!(
        reopened
            .latest_recovery()
            .expect("review retry preserves the extended prefix")
            .through(),
        RecoverySequence::new(2).expect("second sequence")
    );
    assert_eq!(
        daemon
            .workspace_state()
            .expect("review refusal remains non-mutating")
            .records,
        records_before_reopen,
        "refusing the stale saved version must not append a review record"
    );
    drop(daemon);

    let restarted = LiveDaemon::with_checkpoint_runtime(startup(), runtime).expect("restart");
    restarted.open_at_start(&root).expect("restart workspace");
    let restored = restarted
        .checkpoint_snapshot()
        .expect("restored checkpoint");
    assert_eq!(
        restored
            .latest_recovery()
            .expect("review recovery survives restart")
            .through(),
        RecoverySequence::new(2).expect("sequence")
    );
    assert!(restored.last_meaningful().is_none());
    assert!(restored.open_window().is_some());
    drop(restarted);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn review_open_refuses_an_already_preserved_pending_prefix_without_a_second_write() {
    let root = scratch("review-open-recovery-refusal");
    let _ = fs::remove_dir_all(&root);
    let source = root.with_extension("source");
    fs::create_dir_all(&source).expect("source");
    fs::write(source.join("notes.txt"), b"first draft\n").expect("source file");
    let prepared = PreparedFolderImport::prepare(&source, &root).expect("prepare import");
    let (confirmed, _) = prepared.confirm_into_workspace().expect("confirm import");
    drop(confirmed);
    let runtime = CheckpointRuntimeParameters {
        idle_interval: Some(Duration::from_millis(250)),
        maximum_uncheckpointed_bytes: Some(u64::MAX),
        maximum_uncheckpointed_interval: Some(Duration::from_secs(60)),
    };
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), runtime).expect("config");
    daemon.open_at_start(&root).expect("workspace");
    let inspected = daemon
        .inspect_managed_file("notes.txt")
        .expect("imported file");
    daemon
        .preserve_managed_text_edit(
            "notes.txt",
            "pending draft\n",
            inspected.content_digest(),
            inspected.executable(),
        )
        .expect("pending edit is preserved");
    let before = daemon.workspace_state().expect("before review").records;

    let mut database =
        Sqlite::open(root.join(".mesh").join(RECOVERY_DATABASE_FILE_NAME)).expect("sqlite");
    database
        .execute_batch(&format!(
            "CREATE TRIGGER refuse_review_recovery
             BEFORE INSERT ON {}
             BEGIN SELECT RAISE(ABORT, 'planted review recovery failure'); END;",
            SqliteRecoveryState::table_name(),
        ))
        .expect("failure injection");
    drop(database);

    let shown = daemon.workspace_state().expect("review source");
    let refusal = daemon
        .open_current_review_for_workspace(
            &shown.root,
            &shown.digest,
            &shown.installation,
            PublicKey::from_bytes([94; 32]),
        )
        .expect_err("recovery-only work must be saved before it becomes reviewable");
    assert_eq!(
        refusal.code, "publication-native-work-pending",
        "the native-work guard must win without retrying recovery persistence"
    );
    let snapshot = daemon.checkpoint_snapshot().expect("prior checkpoint");
    assert_eq!(
        snapshot
            .latest_recovery()
            .expect("pending edit remains preserved")
            .through(),
        RecoverySequence::new(1).expect("sequence")
    );
    assert!(snapshot.last_meaningful().is_none());
    assert!(snapshot.open_window().is_some());
    assert_eq!(
        daemon
            .workspace_state()
            .expect("refusal leaves workspace readable")
            .records,
        before,
        "the refusal must append neither recovery nor review records"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn maximum_time_persistence_failure_keeps_the_window_open_and_surfaces_attention() {
    let root = scratch("automatic-time-recovery-refusal");
    let _ = fs::remove_dir_all(&root);
    let runtime = CheckpointRuntimeParameters {
        idle_interval: Some(Duration::from_millis(200)),
        maximum_uncheckpointed_bytes: Some(u64::MAX),
        maximum_uncheckpointed_interval: Some(Duration::from_millis(40)),
    };
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), runtime).expect("config");
    daemon.open_at_start(&root).expect("workspace");
    // Arm before save launches the detached worker. Installing a trigger after save races
    // the 40 ms recovery timer: a slow CREATE TRIGGER can otherwise miss the write entirely.
    // Admit exactly the initial observation, then refuse every subsequent state persistence.
    let mut database =
        Sqlite::open(root.join(".mesh").join(RECOVERY_DATABASE_FILE_NAME)).expect("sqlite");
    database
        .execute_batch(&format!(
            "CREATE TABLE test_maximum_recovery_writes (count INTEGER NOT NULL);
             INSERT INTO test_maximum_recovery_writes VALUES (0);
             CREATE TRIGGER refuse_maximum_recovery
             BEFORE INSERT ON {table}
             WHEN (SELECT count FROM test_maximum_recovery_writes) >= 1
             BEGIN SELECT RAISE(ABORT, 'planted maximum recovery failure'); END;
             CREATE TRIGGER count_initial_observation
             AFTER INSERT ON {table}
             BEGIN UPDATE test_maximum_recovery_writes SET count = count + 1; END;
             CREATE TRIGGER count_initial_observation_update
             AFTER UPDATE ON {table}
             BEGIN UPDATE test_maximum_recovery_writes SET count = count + 1; END;",
            table = SqliteRecoveryState::table_name(),
        ))
        .expect("failure injection before worker launch");
    drop(database);

    daemon
        .save_file_version(
            RecoverySequence::new(1).expect("sequence"),
            &content(83),
            &chunking(),
            ManifestPagingPolicy::flat(),
            request(1, 83),
            &CanonicalHead,
        )
        .expect("durable save");

    let deadline = std::time::Instant::now() + Duration::from_millis(150);
    loop {
        let state = daemon
            .workspace_state()
            .expect("workspace remains readable");
        if state
            .conditions
            .iter()
            .any(|condition| condition.code() == "checkpoint-recovery-needs-attention")
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the detached maximum-recovery refusal was not surfaced; snapshot={:?}; worker={}; conditions={:?}",
            daemon.checkpoint_snapshot(),
            worker_diagnostic(&daemon),
            state.conditions.iter().map(|condition| condition.code()).collect::<Vec<_>>()
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let snapshot = daemon
        .checkpoint_snapshot()
        .expect("prior state remains visible");
    assert!(snapshot.latest_recovery().is_none());
    assert!(snapshot.last_meaningful().is_none());
    assert!(snapshot.open_window().is_some());
    assert!(snapshot.pending_meaningful().is_some());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn an_old_timer_cannot_close_a_newer_window_early() {
    let root = scratch("live-generation");
    let _ = fs::remove_dir_all(&root);
    let runtime = CheckpointRuntimeParameters {
        idle_interval: Some(Duration::from_millis(500)),
        maximum_uncheckpointed_bytes: Some(64 * 1024),
        maximum_uncheckpointed_interval: Some(Duration::from_millis(20)),
    };
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), runtime).expect("config");
    daemon.open_at_start(&root).expect("workspace");
    daemon
        .save_file_version(
            RecoverySequence::new(1).expect("sequence"),
            &content(51),
            &chunking(),
            ManifestPagingPolicy::flat(),
            request_for_actor(1, 51, 2),
            &CanonicalHead,
        )
        .expect("first save");
    assert!(
        daemon.schedule_pending_checkpoint().is_none(),
        "the public save already owns the daemon's one idle worker"
    );

    std::thread::sleep(Duration::from_millis(300));
    daemon
        .save_file_version(
            RecoverySequence::new(2).expect("sequence"),
            &content(52),
            &chunking(),
            ManifestPagingPolicy::flat(),
            request_for_actor(1, 52, 13),
            &CanonicalHead,
        )
        .expect("second save replaces the pending extent");

    // Cross the first save's original deadline while remaining short of the complete interval
    // measured from the second save. A worker that failed to reset would close this window now.
    std::thread::sleep(Duration::from_millis(300));
    let still_open = daemon.checkpoint_snapshot().expect("checkpoint state");
    assert!(still_open.last_meaningful().is_none());
    assert_eq!(
        still_open
            .pending_meaningful()
            .expect("newer save remains pending")
            .through(),
        RecoverySequence::new(2).expect("sequence")
    );

    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    while daemon
        .checkpoint_snapshot()
        .expect("checkpoint while waiting")
        .last_meaningful()
        .is_none()
    {
        assert!(
            std::time::Instant::now() < deadline,
            "the newer save's automatic worker did not settle it"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let snapshot = daemon
        .checkpoint_snapshot()
        .expect("settled checkpoint state");
    assert_eq!(
        snapshot
            .last_meaningful()
            .expect("latest stable extent becomes meaningful")
            .through(),
        RecoverySequence::new(2).expect("sequence")
    );
    assert!(snapshot.open_window().is_none());
    assert!(snapshot.pending_meaningful().is_none());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_post_startup_recovery_worker_cannot_settle_a_replacement_workspace() {
    let first = scratch("post-startup-switch-first");
    let second = scratch("post-startup-switch-second");
    let _ = fs::remove_dir_all(&first);
    let _ = fs::remove_dir_all(&second);
    let switched_parameters = CheckpointRuntimeParameters {
        idle_interval: Some(Duration::from_millis(80)),
        ..parameters()
    };
    {
        let daemon =
            LiveDaemon::with_checkpoint_runtime(startup(), switched_parameters).expect("config");
        daemon.open_at_start(&first).expect("first workspace");
        daemon
            .save_file_version(
                RecoverySequence::new(1).expect("sequence"),
                &content(42),
                &chunking(),
                ManifestPagingPolicy::flat(),
                request(1, 42),
                &CanonicalHead,
            )
            .expect("first workspace has a pending save");
    }

    let restarted = LiveDaemon::with_checkpoint_runtime(startup(), switched_parameters)
        .expect("restart config");
    Operations::open_workspace(&restarted, &first.display().to_string())
        .expect("first workspace opens and schedules");
    Operations::open_workspace(&restarted, &second.display().to_string())
        .expect("replacement workspace opens before the worker wakes");
    std::thread::sleep(Duration::from_millis(160));

    let current = restarted
        .checkpoint_snapshot()
        .expect("replacement checkpoint state");
    assert!(current.open_window().is_none());
    assert!(current.last_meaningful().is_none());

    let first_inspector = LiveDaemon::with_checkpoint_runtime(startup(), switched_parameters)
        .expect("inspect config");
    first_inspector
        .open_at_start(&first)
        .expect("first workspace reopens for inspection");
    let first_snapshot = first_inspector
        .checkpoint_snapshot()
        .expect("first checkpoint state");
    assert!(first_snapshot.pending_meaningful().is_some());
    assert!(first_snapshot.last_meaningful().is_none());

    let _ = fs::remove_dir_all(first);
    let _ = fs::remove_dir_all(second);
}

#[test]
fn a_recovery_worker_cannot_settle_a_different_workspace_reopened_at_the_same_path() {
    let root = scratch("same-path-generation");
    let replacement = scratch("same-path-generation-replacement");
    let displaced = scratch("same-path-generation-displaced");
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&replacement);
    let _ = fs::remove_dir_all(&displaced);
    let switched_parameters = CheckpointRuntimeParameters {
        idle_interval: Some(Duration::from_millis(200)),
        ..parameters()
    };

    for (workspace, seed, actor) in [(&root, 61, 2), (&replacement, 62, 13)] {
        let daemon =
            LiveDaemon::with_checkpoint_runtime(startup(), switched_parameters).expect("config");
        daemon.open_at_start(workspace).expect("workspace");
        daemon
            .save_file_version(
                RecoverySequence::new(1).expect("sequence"),
                &content(seed),
                &chunking(),
                ManifestPagingPolicy::flat(),
                request_for_actor(1, seed, actor),
                &CanonicalHead,
            )
            .expect("workspace has a pending save");
    }

    let daemon =
        LiveDaemon::with_checkpoint_runtime(startup(), switched_parameters).expect("config");
    daemon.open_at_start(&root).expect("first workspace");
    let old_timer = daemon
        .schedule_pending_checkpoint()
        .expect("first workspace schedules");

    fs::rename(&root, &displaced).expect("move the first workspace aside");
    fs::rename(&replacement, &root).expect("replace it at the identical path");
    daemon
        .open_at_start(&root)
        .expect("replacement reopens at the same path");

    assert!(old_timer
        .join()
        .expect("old timer does not panic")
        .expect("old timer reads state")
        .is_none());
    let replacement_pending = daemon
        .checkpoint_snapshot()
        .expect("replacement checkpoint state");
    assert!(replacement_pending.last_meaningful().is_none());
    assert_eq!(
        replacement_pending
            .pending_meaningful()
            .expect("replacement remains pending")
            .through(),
        RecoverySequence::new(1).expect("sequence")
    );

    assert!(daemon
        .schedule_pending_checkpoint()
        .expect("replacement schedules its own timer")
        .join()
        .expect("replacement timer does not panic")
        .expect("replacement timer verifies state")
        .is_some());

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(displaced);
}

#[test]
fn restart_refuses_a_pending_acknowledgement_when_new_journal_truth_changes_the_index() {
    let root = scratch("pending-digest-mismatch");
    let _ = fs::remove_dir_all(&root);
    {
        let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
        daemon.open_at_start(&root).expect("workspace");
        daemon
            .save_file_version(
                RecoverySequence::new(1).expect("sequence"),
                &content(11),
                &chunking(),
                ManifestPagingPolicy::flat(),
                request(1, 8),
                &CanonicalHead,
            )
            .expect("pending durable save");
    }

    // Simulate durable journal progress outside the checkpoint coordinator. Recovery must not
    // bless the older process-lost acknowledgement against this newer rebuilt index.
    {
        let mut workspace = OpenWorkspace::open(&root).expect("workspace rebuild");
        let cas = Cas::open(&root).expect("content store");
        save_file_version(
            &mut workspace,
            &cas,
            &content(12),
            &chunking(),
            ManifestPagingPolicy::flat(),
            request_for_actor(1, 9, 13),
            &CanonicalHead,
        )
        .expect("newer out-of-band journal truth");
    }

    let restarted =
        LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("restart config");
    restarted.open_at_start(&root).expect("restart workspace");
    assert!(matches!(
        restarted
            .schedule_pending_checkpoint()
            .expect("pending acknowledgement schedules")
            .join()
            .expect("settlement worker does not panic"),
        Err(AutomaticCheckpointError::PendingAcknowledgementInvalid(_))
    ));
    let snapshot = restarted.checkpoint_snapshot().expect("fail-closed state");
    assert!(snapshot.open_window().is_some());
    assert!(snapshot.last_meaningful().is_none());
    assert!(snapshot.pending_meaningful().is_some());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_detached_recovery_refusal_is_visible_in_workspace_state() {
    let root = scratch("pending-digest-attention");
    let _ = fs::remove_dir_all(&root);
    {
        let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
        daemon.open_at_start(&root).expect("workspace");
        daemon
            .save_file_version(
                RecoverySequence::new(1).expect("sequence"),
                &content(11),
                &chunking(),
                ManifestPagingPolicy::flat(),
                request(1, 8),
                &CanonicalHead,
            )
            .expect("pending durable save");
    }

    {
        let mut workspace = OpenWorkspace::open(&root).expect("workspace rebuild");
        let cas = Cas::open(&root).expect("content store");
        save_file_version(
            &mut workspace,
            &cas,
            &content(12),
            &chunking(),
            ManifestPagingPolicy::flat(),
            request_for_actor(1, 9, 13),
            &CanonicalHead,
        )
        .expect("newer out-of-band journal truth");
    }

    let restarted =
        LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("restart config");
    let before_open = restarted.feed().latest();
    Operations::open_workspace(&restarted, &root.display().to_string())
        .expect("workspace opens and schedules recovery");

    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    loop {
        let state = restarted
            .workspace_state()
            .expect("workspace stays readable");
        if let Some(condition) = state
            .conditions
            .iter()
            .find(|condition| condition.code() == "checkpoint-recovery-needs-attention")
        {
            assert!(condition.recoverable());
            assert!(condition.related().is_empty());
            assert!(!condition.message().contains(&root.display().to_string()));
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the detached recovery refusal remained silent in workspace.state"
        );
        std::thread::sleep(Duration::from_millis(2));
    }

    let journal_before_refused_save =
        fs::read(root.join(".mesh/records.mesh")).expect("journal before");
    assert!(matches!(
        restarted.save_file_version(
            RecoverySequence::new(2).expect("later sequence"),
            &content(13),
            &chunking(),
            ManifestPagingPolicy::flat(),
            request_for_actor(2, 14, 14),
            &CanonicalHead,
        ),
        Err(LiveCheckpointSaveError::Checkpoint(
            AutomaticCheckpointError::RecoveryNeedsAttention
        ))
    ));
    assert!(matches!(
        restarted
            .observe_checkpoint_activity(RecoverySequence::new(2).expect("later observation"), 1,),
        Err(AutomaticCheckpointError::RecoveryNeedsAttention)
    ));
    assert_eq!(
        fs::read(root.join(".mesh/records.mesh")).expect("journal after"),
        journal_before_refused_save,
        "the refused save changes no immutable journal byte"
    );

    let snapshot = restarted.checkpoint_snapshot().expect("fail-closed state");
    assert!(snapshot.open_window().is_some());
    assert!(snapshot.last_meaningful().is_none());
    assert!(snapshot.pending_meaningful().is_some());
    let attention_events = restarted
        .feed()
        .since(before_open)
        .entries
        .iter()
        .filter(|event| event.kind.word() == "checkpoint-recovery-needs-attention")
        .count();
    assert_eq!(attention_events, 1, "one refusal produces one notification");

    assert!(matches!(
        restarted
            .schedule_pending_checkpoint()
            .expect("pending save still schedules")
            .join()
            .expect("worker does not panic"),
        Err(AutomaticCheckpointError::PendingAcknowledgementInvalid(_))
    ));
    let repeated_attention_events = restarted
        .feed()
        .since(before_open)
        .entries
        .iter()
        .filter(|event| event.kind.word() == "checkpoint-recovery-needs-attention")
        .count();
    assert_eq!(
        repeated_attention_events, 1,
        "retrying the same refusal does not flood subscribers"
    );

    let replacement = scratch("pending-digest-attention-replacement");
    let _ = fs::remove_dir_all(&replacement);
    Operations::open_workspace(&restarted, &replacement.display().to_string())
        .expect("replacement workspace opens");
    assert!(restarted
        .workspace_state()
        .expect("replacement state")
        .conditions
        .iter()
        .all(|condition| condition.code() != "checkpoint-recovery-needs-attention"));
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(replacement);
}

fn stamp(value: u8) -> RecoveryStamp {
    RecoveryStamp::new(
        u64::from(value),
        RecoveryEventUlid::from_bytes([value; 16]),
        RecordDigest::from_bytes([value; 32]),
    )
}

#[test]
fn journaled_save_observes_settles_and_restores_after_restart_without_a_process_token() {
    let root = scratch("restart");
    let _ = fs::remove_dir_all(&root);
    let sequence = RecoverySequence::new(1).expect("issued sequence");

    {
        let mut slow_parameters = parameters();
        slow_parameters.idle_interval = Some(Duration::from_secs(60));
        let daemon =
            LiveDaemon::with_checkpoint_runtime(startup(), slow_parameters).expect("config");
        daemon.open_at_start(&root).expect("workspace");
        daemon
            .save_file_version(
                sequence,
                &content(11),
                &chunking(),
                ManifestPagingPolicy::flat(),
                request(1, 8),
                &CanonicalHead,
            )
            .expect("typed save");

        let state = daemon.workspace_state().expect("reloaded journal truth");
        assert_eq!(
            (state.records, state.operations, state.manifests),
            (2, 1, 1)
        );
        assert!(state.private_version.version().is_some());
        assert_eq!(
            daemon
                .checkpoint_snapshot()
                .expect("checkpoint state")
                .open_window()
                .expect("save observed")
                .last(),
            sequence
        );
        daemon
            .preserve_recovery(
                RecoveryTrigger::ActorDisconnected,
                RecoveryPreserved::from_verified_bytes(
                    stamp(7),
                    sequence,
                    vec![7],
                    RecordDigest::from_bytes([7; 32]),
                )
                .expect("verified recovery bytes"),
            )
            .expect("recovery-only pointer");
        let report = daemon.crash_report().expect("open workspace report");
        assert!(report.checkpoint_state_available());
        assert_eq!(report.meaningful_checkpoint_through(), None);
        assert_eq!(report.recovery_preserved_through(), Some(1));
        assert_eq!(report.open_activity(), Some((1, 1)));
        let bundle = report.to_bundle_section().to_string();
        assert!(bundle.contains("\"checkpoint_state_available\":true"));
        assert!(bundle.contains("\"recovery_preserved_through\":\"1\""));
        assert!(bundle.contains("\"open_activity_from\":\"1\""));
        assert!(bundle.contains("\"open_activity_through\":\"1\""));
    };

    {
        let restarted = Arc::new(
            LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("restart config"),
        );
        restarted.open_at_start(&root).expect("restart workspace");
        let restored = restarted
            .checkpoint_snapshot()
            .expect("journal-backed observation restored");
        if let Some(window) = restored.open_window() {
            assert_eq!(window.last(), sequence);
        } else {
            assert_eq!(
                restored
                    .last_meaningful()
                    .expect("startup worker already settled the restored window")
                    .through(),
                sequence
            );
        }
        if let Some(worker) = restarted.schedule_pending_checkpoint() {
            let transition = worker
                .join()
                .expect("settlement worker does not panic")
                .expect("journal and rebuilt index recover the process-lost acknowledgement")
                .expect("the pending save settles");
            assert!(matches!(
                transition,
                RecoveryTransition::MeaningfulSaved { .. }
            ));
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while restarted
            .checkpoint_snapshot()
            .expect("settlement state")
            .last_meaningful()
            .is_none()
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            restarted
                .checkpoint_snapshot()
                .expect("settled state")
                .last_meaningful()
                .expect("the restored pending save settles within its configured interval")
                .through(),
            sequence
        );
    }

    let restarted =
        LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("restart config");
    restarted.open_at_start(&root).expect("restart workspace");
    let state = restarted.workspace_state().expect("journal restored");
    assert_eq!(
        (state.records, state.operations, state.manifests),
        (2, 1, 1)
    );
    let snapshot = restarted.checkpoint_snapshot().expect("recovery restored");
    assert!(snapshot.open_window().is_none());
    assert_eq!(
        snapshot
            .last_meaningful()
            .expect("meaningful checkpoint restored")
            .through(),
        sequence
    );
    let report = restarted.crash_report().expect("restart report");
    assert!(report.checkpoint_state_available());
    assert_eq!(report.meaningful_checkpoint_through(), Some(1));
    assert_eq!(report.recovery_preserved_through(), Some(1));
    assert_eq!(report.open_activity(), None);
    assert!(report.to_string().contains(
        "checkpoint state: meaningful through 1, recovery through 1, open activity none"
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn disabled_runtime_and_stale_sequence_refuse_before_a_new_journal_append() {
    let disabled_root = scratch("disabled");
    let _ = fs::remove_dir_all(&disabled_root);
    let disabled = LiveDaemon::new(startup());
    disabled.open_at_start(&disabled_root).expect("workspace");
    let refused = disabled.save_file_version(
        RecoverySequence::new(1).expect("sequence"),
        &content(1),
        &chunking(),
        ManifestPagingPolicy::flat(),
        request(1, 1),
        &CanonicalHead,
    );
    assert!(matches!(
        refused,
        Err(LiveCheckpointSaveError::Checkpoint(_))
    ));
    assert_eq!(disabled.workspace_state().expect("state").records, 0);

    let ordered_root = scratch("ordered");
    let _ = fs::remove_dir_all(&ordered_root);
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&ordered_root).expect("workspace");
    daemon
        .save_file_version(
            RecoverySequence::new(2).expect("sequence"),
            &content(2),
            &chunking(),
            ManifestPagingPolicy::flat(),
            request(1, 2),
            &CanonicalHead,
        )
        .expect("first save");
    let stale = daemon.save_file_version(
        RecoverySequence::new(1).expect("stale sequence"),
        &content(3),
        &chunking(),
        ManifestPagingPolicy::flat(),
        request(2, 3),
        &CanonicalHead,
    );
    assert!(matches!(stale, Err(LiveCheckpointSaveError::Checkpoint(_))));
    let replayed_sequence = daemon.save_file_version(
        RecoverySequence::new(2).expect("replayed sequence"),
        &content(4),
        &chunking(),
        ManifestPagingPolicy::flat(),
        request_for_actor(1, 4, 19),
        &CanonicalHead,
    );
    assert!(matches!(
        replayed_sequence,
        Err(LiveCheckpointSaveError::Checkpoint(_))
    ));
    let state = daemon.workspace_state().expect("unchanged journal truth");
    assert_eq!(
        (state.records, state.operations, state.manifests),
        (2, 1, 1)
    );
    assert_eq!(
        daemon
            .checkpoint_snapshot()
            .expect("checkpoint")
            .open_window()
            .expect("window")
            .last(),
        RecoverySequence::new(2).expect("sequence")
    );
    let _ = fs::remove_dir_all(disabled_root);
    let _ = fs::remove_dir_all(ordered_root);
}

#[test]
fn durable_save_failure_does_not_observe_activity_or_return_an_acknowledgement() {
    let root = scratch("save-failure");
    let _ = fs::remove_dir_all(&root);
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&root).expect("workspace");
    let result = daemon.save_file_version(
        RecoverySequence::new(1).expect("sequence"),
        &content(4),
        &chunking(),
        ManifestPagingPolicy::flat(),
        request(0, 4),
        &CanonicalHead,
    );
    assert!(matches!(
        result,
        Err(LiveCheckpointSaveError::DurableSave(_))
    ));
    assert_eq!(
        daemon.workspace_state().expect("journal unchanged").records,
        0
    );
    assert!(daemon
        .checkpoint_snapshot()
        .expect("checkpoint unchanged")
        .open_window()
        .is_none());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn primary_observation_failure_recovers_pending_checkpoint_across_restart() {
    let root = scratch("observation-recovery");
    let _ = fs::remove_dir_all(&root);
    let mut slow_parameters = parameters();
    slow_parameters.idle_interval = Some(Duration::from_secs(60));
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), slow_parameters).expect("config");
    daemon.open_at_start(&root).expect("workspace");

    let mut database =
        Sqlite::open(root.join(".mesh").join(RECOVERY_DATABASE_FILE_NAME)).expect("sqlite");
    database
        .execute_batch(
            "CREATE TRIGGER refuse_recovery_observation
             BEFORE INSERT ON mesh_recovery_state
             BEGIN SELECT RAISE(ABORT, 'planted observation persistence failure'); END;",
        )
        .expect("failure injection");
    drop(database);

    let sequence = RecoverySequence::new(1).expect("sequence");
    daemon
        .save_file_version(
            sequence,
            &content(5),
            &chunking(),
            ManifestPagingPolicy::flat(),
            request(1, 5),
            &CanonicalHead,
        )
        .expect("the journal-backed recovery slot admits the prospective snapshot");
    let state = daemon
        .workspace_state()
        .expect("journal truth was reloaded");
    assert_eq!(
        (state.records, state.operations, state.manifests),
        (2, 1, 1),
        "the primary-state refusal cannot roll back the already durable journal"
    );
    assert_eq!(
        daemon
            .checkpoint_snapshot()
            .expect("fallback checkpoint state")
            .open_window()
            .expect("durable pending window")
            .last(),
        sequence,
        "the in-memory state advances only after the fallback slot commits"
    );
    let mut database =
        Sqlite::open(root.join(".mesh").join(RECOVERY_DATABASE_FILE_NAME)).expect("sqlite");
    database
        .execute_batch("DROP TRIGGER refuse_recovery_observation;")
        .expect("the planted primary failure was transient");
    drop(database);
    drop(daemon);

    let restarted = Arc::new(
        LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("restart config"),
    );
    restarted.open_at_start(&root).expect("restart workspace");
    assert_eq!(
        restarted
            .workspace_state()
            .expect("journal restored")
            .records,
        2
    );
    assert_eq!(
        restarted
            .checkpoint_snapshot()
            .expect("recovered observation")
            .open_window()
            .expect("pending window survives restart")
            .last(),
        sequence
    );
    assert!(restarted
        .schedule_pending_checkpoint()
        .expect("recovered save schedules settlement")
        .join()
        .expect("worker does not panic")
        .expect("journal verifies recovered acknowledgement")
        .is_some());
    let settled = restarted.checkpoint_snapshot().expect("settled snapshot");
    assert!(settled.open_window().is_none());
    assert_eq!(
        settled
            .last_meaningful()
            .expect("meaningful checkpoint")
            .through(),
        sequence
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn repeated_primary_observation_failures_recover_the_latest_journaled_save() {
    let root = scratch("observation-recovery-consecutive");
    let _ = fs::remove_dir_all(&root);
    let mut slow_parameters = parameters();
    slow_parameters.idle_interval = Some(Duration::from_secs(60));
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), slow_parameters).expect("config");
    daemon.open_at_start(&root).expect("workspace");

    let mut database =
        Sqlite::open(root.join(".mesh").join(RECOVERY_DATABASE_FILE_NAME)).expect("sqlite");
    database
        .execute_batch(
            "CREATE TRIGGER refuse_recovery_observation
             BEFORE INSERT ON mesh_recovery_state
             BEGIN SELECT RAISE(ABORT, 'planted persistent primary failure'); END;",
        )
        .expect("failure injection");
    drop(database);

    for sequence in 1..=2 {
        daemon
            .save_file_version(
                RecoverySequence::new(sequence).expect("sequence"),
                &content(sequence as u8),
                &chunking(),
                ManifestPagingPolicy::flat(),
                request(sequence, sequence as u8),
                &CanonicalHead,
            )
            .expect("each journaled save advances the durable recovery slot");
    }
    assert_eq!(
        daemon
            .checkpoint_snapshot()
            .expect("fallback checkpoint state")
            .open_window()
            .expect("durable pending window")
            .last(),
        RecoverySequence::new(2).expect("sequence")
    );
    assert_eq!(daemon.workspace_state().expect("journal truth").records, 4);

    drop(daemon);
    let restarted = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("restart");
    restarted.open_at_start(&root).expect("restart workspace");
    assert_eq!(
        restarted
            .checkpoint_snapshot()
            .expect("latest journal-backed observation restored")
            .open_window()
            .expect("pending window")
            .last(),
        RecoverySequence::new(2).expect("sequence")
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn evidence_between_primary_failures_does_not_strand_the_later_journaled_save() {
    let root = scratch("observation-recovery-with-evidence");
    let _ = fs::remove_dir_all(&root);
    let mut slow_parameters = parameters();
    slow_parameters.idle_interval = Some(Duration::from_secs(60));
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), slow_parameters).expect("config");
    daemon.open_at_start(&root).expect("workspace");

    let mut database =
        Sqlite::open(root.join(".mesh").join(RECOVERY_DATABASE_FILE_NAME)).expect("sqlite");
    database
        .execute_batch(
            "CREATE TRIGGER refuse_recovery_observation
             BEFORE INSERT ON mesh_recovery_state
             BEGIN SELECT RAISE(ABORT, 'planted persistent primary failure'); END;",
        )
        .expect("failure injection");
    drop(database);

    let first = RecoverySequence::new(1).expect("sequence");
    daemon
        .save_file_version(
            first,
            &content(1),
            &chunking(),
            ManifestPagingPolicy::flat(),
            request(1, 1),
            &CanonicalHead,
        )
        .expect("first save enters the journal-backed recovery slot");
    daemon
        .record_checkpoint_boundary(
            RecoveryTrigger::FsyncCompleted,
            RecoveryBoundaryEvidence::new(first, BoundaryEvidenceKind::Synced),
        )
        .expect("ordinary evidence remains inside the open window");

    let second = RecoverySequence::new(2).expect("sequence");
    daemon
        .save_file_version(
            second,
            &content(2),
            &chunking(),
            ManifestPagingPolicy::flat(),
            request(2, 2),
            &CanonicalHead,
        )
        .expect("later journaled save advances the same recovery window despite newer evidence");
    assert_eq!(daemon.workspace_state().expect("journal truth").records, 4);
    assert_eq!(
        daemon
            .checkpoint_snapshot()
            .expect("latest fallback")
            .open_window()
            .expect("pending window")
            .last(),
        second
    );

    drop(daemon);
    let restarted = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("restart");
    restarted.open_at_start(&root).expect("restart workspace");
    let restored = restarted
        .checkpoint_snapshot()
        .expect("latest fallback restored");
    assert_eq!(
        restored.open_window().expect("pending window").last(),
        second
    );
    assert_eq!(
        restored
            .open_window()
            .expect("pending window")
            .latest_evidence(),
        Some(RecoveryBoundaryEvidence::new(
            first,
            BoundaryEvidenceKind::Synced,
        )),
        "evidence remains part of the continued recovery window"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn both_observation_persistence_failures_return_no_ack_but_keep_journal_truth() {
    let root = scratch("observation-double-failure");
    let _ = fs::remove_dir_all(&root);
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&root).expect("workspace");

    let mut database =
        Sqlite::open(root.join(".mesh").join(RECOVERY_DATABASE_FILE_NAME)).expect("sqlite");
    database
        .execute_batch(&format!(
            "CREATE TRIGGER refuse_primary_observation
             BEFORE INSERT ON {}
             BEGIN SELECT RAISE(ABORT, 'planted primary persistence failure'); END;
             CREATE TRIGGER refuse_recovery_observation
             BEFORE INSERT ON {}
             BEGIN SELECT RAISE(ABORT, 'planted recovery persistence failure'); END;",
            SqliteRecoveryState::table_name(),
            SqliteRecoveryState::observation_table_name(),
        ))
        .expect("failure injection");
    drop(database);

    let result = daemon.save_file_version(
        RecoverySequence::new(1).expect("sequence"),
        &content(6),
        &chunking(),
        ManifestPagingPolicy::flat(),
        request(1, 6),
        &CanonicalHead,
    );
    assert!(matches!(
        result,
        Err(LiveCheckpointSaveError::ObservationAfterJournal(_))
    ));
    assert_eq!(
        daemon.workspace_state().expect("journal truth").records,
        2,
        "the immutable save remains authoritative"
    );
    assert!(daemon
        .checkpoint_snapshot()
        .expect("prior checkpoint state")
        .open_window()
        .is_none());
    drop(daemon);

    let restarted =
        LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("restart config");
    restarted.open_at_start(&root).expect("restart workspace");
    assert_eq!(
        restarted
            .workspace_state()
            .expect("journal restored")
            .records,
        2
    );
    assert!(restarted
        .checkpoint_snapshot()
        .expect("observation remains unknown")
        .open_window()
        .is_none());
    let _ = fs::remove_dir_all(root);
}
