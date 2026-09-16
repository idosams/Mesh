//! Real folder-rescan bytes composed through the durable checkpoint runtime.

#![cfg(unix)]

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use mesh_chunking::ChunkingConfig;
use mesh_daemon::folder_watch::watch::{DetectedFolderFile, DetectedFolderFileError, Snapshot};
use mesh_daemon::ipc::surface::{nothing_to_recover, Operations, StartupSummary};
use mesh_daemon::{FileVersionCheckpointRequest, LiveDaemon, ManifestPagingPolicy};
use mesh_materializer::{AttributionConfidence, EventSequence};
use mesh_operations::{
    ActorId, ActorSequence, CausalParents, HeadDerivation, HeadId, Hlc, ObjectId, PolicyEpoch,
    PortableMetadata, SessionId, Signature, TransitionCommitment, VersionId, WorkspaceId,
};
use mesh_store::{CheckpointRuntimeParameters, RecoverySequence, RecoveryTransition};
use mesh_types::{Blake3, ContentDigest as _};

fn scratch(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "mesh-folder-checkpoint-{name}-{}-{:?}",
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
        maximum_uncheckpointed_bytes: Some(64 * 1024),
        maximum_uncheckpointed_interval: Some(Duration::from_millis(20)),
    }
}

fn chunking() -> ChunkingConfig {
    ChunkingConfig::always_chunked(64, 256, 1024).expect("explicit policy")
}

struct CanonicalHead;

impl HeadDerivation for CanonicalHead {
    fn resulting_head(&self, commitment: &TransitionCommitment) -> HeadId {
        HeadId::from_bytes(*Blake3::digest_bytes(&commitment.canonical_bytes()).as_bytes())
    }
}

fn request() -> FileVersionCheckpointRequest {
    FileVersionCheckpointRequest::new(
        WorkspaceId::from_bytes([1; 16]),
        ActorId::from_bytes([2; 32]),
        SessionId::from_bytes([3; 16]),
        ActorSequence::new(1),
        CausalParents::genesis(),
        HeadId::from_bytes([4; 32]),
        PolicyEpoch::new(5),
        Hlc::new(1_700_000_000_001, 6),
        ObjectId::from_bytes([7; 16]),
        VersionId::from_bytes([8; 32]),
        Vec::new(),
        PortableMetadata::new(false),
        Signature::from_bytes([10; 64]),
    )
}

#[test]
fn detected_folder_bytes_survive_restart_and_only_become_meaningful_after_idle_settling() {
    let root = scratch("restart");
    let _ = fs::remove_dir_all(&root);
    let folder = root.join("project");
    fs::create_dir_all(&folder).expect("project");
    let file = folder.join("src.rs");
    fs::write(&file, b"fn old() {}\n").expect("initial file");
    let before = Snapshot::of(&folder);
    fs::write(&file, b"fn main() { println!(\"mesh\"); }\n").expect("edit");
    let after = Snapshot::of(&folder);
    let detected =
        DetectedFolderFile::from_rescan(&folder, &before, &after, EventSequence::new(1), "src.rs")
            .expect("verified recovery-detected bytes");
    assert_eq!(
        detected.confidence(),
        AttributionConfidence::RecoveryDetected
    );

    {
        let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
        daemon.open_at_start(&root).expect("workspace");
        daemon
            .save_detected_folder_file_version(
                &detected,
                &chunking(),
                ManifestPagingPolicy::flat(),
                request(),
                &CanonicalHead,
            )
            .expect("durable recovery-detected file state");
        let snapshot = daemon.checkpoint_snapshot().expect("checkpoint state");
        let window = snapshot.open_window().expect("activity is open");
        assert_eq!(
            (window.from(), window.last()),
            (
                RecoverySequence::new(1).expect("sequence"),
                RecoverySequence::new(1).expect("sequence")
            )
        );
        assert!(
            window.latest_evidence().is_none(),
            "rescan invented a boundary"
        );
        assert!(snapshot.last_meaningful().is_none());
        std::thread::sleep(Duration::from_millis(8));
        assert!(
            daemon
                .checkpoint_snapshot()
                .expect("settling alone changes nothing")
                .last_meaningful()
                .is_none(),
            "elapsed time alone must not claim Saved privately"
        );
    };

    {
        let restarted =
            LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("restart config");
        restarted.open_at_start(&root).expect("restart workspace");
        assert_eq!(
            fs::read(&file).expect("original folder remains"),
            detected.bytes()
        );
        assert_eq!(
            restarted.workspace_state().expect("journal state").records,
            2
        );
        let restored = restarted.checkpoint_snapshot().expect("restored activity");
        assert_eq!(
            restored.open_window().expect("open window survives").last(),
            RecoverySequence::new(1).expect("sequence")
        );
        assert!(restored.last_meaningful().is_none());

        assert!(matches!(
            restarted
                .schedule_pending_checkpoint()
                .expect("pending acknowledgement schedules")
                .join()
                .expect("settlement worker does not panic")
                .expect("verified restart settlement")
                .expect("pending acknowledgement exists"),
            RecoveryTransition::MeaningfulSaved { .. }
        ));
    }

    let restarted =
        LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("second restart");
    restarted.open_at_start(&root).expect("workspace");
    let restored = restarted.checkpoint_snapshot().expect("meaningful state");
    assert!(restored.open_window().is_none());
    assert_eq!(
        restored
            .last_meaningful()
            .expect("settled acknowledgement survives")
            .through(),
        RecoverySequence::new(1).expect("sequence")
    );
    assert_eq!(fs::read(&file).expect("folder bytes"), detected.bytes());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_file_changed_after_rescan_is_refused_before_any_checkpoint_mutation() {
    let root = scratch("changed-after-rescan");
    let _ = fs::remove_dir_all(&root);
    let folder = root.join("project");
    fs::create_dir_all(&folder).expect("project");
    let file = folder.join("src.rs");
    fs::write(&file, b"before\n").expect("before");
    let before = Snapshot::of(&folder);
    fs::write(&file, b"after reading\n").expect("after");
    let after = Snapshot::of(&folder);
    fs::write(&file, b"raced after snapshot\n").expect("raced");

    assert!(matches!(
        DetectedFolderFile::from_rescan(
            &folder,
            &before,
            &after,
            EventSequence::new(1),
            "src.rs",
        ),
        Err(DetectedFolderFileError::ChangedAfterRescan { ref relative_path })
            if relative_path == "src.rs"
    ));

    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&root).expect("workspace");
    assert_eq!(
        daemon
            .workspace_state()
            .expect("no journal mutation")
            .records,
        0
    );
    assert!(daemon
        .checkpoint_snapshot()
        .expect("no checkpoint mutation")
        .open_window()
        .is_none());
    assert_eq!(
        fs::read(&file).expect("raced bytes remain"),
        b"raced after snapshot\n"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn event_sequence_zero_is_refused_before_the_durable_save() {
    let root = scratch("zero-sequence");
    let _ = fs::remove_dir_all(&root);
    let folder = root.join("project");
    fs::create_dir_all(&folder).expect("project");
    let file = folder.join("src.rs");
    fs::write(&file, b"before\n").expect("before");
    let before = Snapshot::of(&folder);
    fs::write(&file, b"after\n").expect("after");
    let after = Snapshot::of(&folder);
    let detected =
        DetectedFolderFile::from_rescan(&folder, &before, &after, EventSequence::new(0), "src.rs")
            .expect("file candidate");

    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&root).expect("workspace");
    assert!(matches!(
        daemon.save_detected_folder_file_version(
            &detected,
            &chunking(),
            ManifestPagingPolicy::flat(),
            request(),
            &CanonicalHead,
        ),
        Err(mesh_daemon::LiveCheckpointSaveError::InvalidFolderEventSequence)
    ));
    assert_eq!(
        daemon
            .workspace_state()
            .expect("no journal mutation")
            .records,
        0
    );
    assert!(daemon
        .checkpoint_snapshot()
        .expect("no checkpoint mutation")
        .open_window()
        .is_none());
    assert_eq!(fs::read(&file).expect("folder bytes"), b"after\n");
    let _ = fs::remove_dir_all(root);
}
