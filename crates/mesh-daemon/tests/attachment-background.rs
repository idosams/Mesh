//! Native background capture without provider execution or ownership of the user's working folder.
#![cfg(unix)]

use ed25519_dalek::{Signer as _, SigningKey};
use mesh_daemon::project_attachment::{
    AttachmentCaptureService, CaptureOutcome, CapturePhase, CaptureSchedule, CaptureStatus,
    ObservationLimits, ProjectAttachment,
};
use mesh_daemon::CheckpointSigner;
use mesh_types::{PublicKey, Signature};
use std::fs;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};
use std::time::{Duration, Instant};

struct Fixture {
    root: PathBuf,
    source: PathBuf,
    metadata: PathBuf,
}
impl Fixture {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("mesh-background-{name}-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let source = root.join("project");
        let metadata = root.join("metadata");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&metadata).unwrap();
        fs::write(source.join("work"), b"one").unwrap();
        ProjectAttachment::register(&source, &metadata).unwrap();
        Self {
            root,
            source,
            metadata,
        }
    }
    fn attached(&self) -> ProjectAttachment {
        ProjectAttachment::reopen(&self.metadata).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

type Gate = (mpsc::Sender<()>, mpsc::Receiver<()>);
#[derive(Default)]
struct Signer {
    fail: AtomicBool,
    gate: Mutex<Option<Gate>>,
}
impl Signer {
    fn gated() -> (Arc<Self>, mpsc::Receiver<()>, mpsc::Sender<()>) {
        let (entered, started) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        (
            Arc::new(Self {
                fail: AtomicBool::new(false),
                gate: Mutex::new(Some((entered, gate))),
            }),
            started,
            release,
        )
    }
}
impl CheckpointSigner for Signer {
    fn public_key(&self) -> PublicKey {
        PublicKey::from_bytes(SigningKey::from_bytes(&[63; 32]).verifying_key().to_bytes())
    }
    fn sign(&self, payload: &mesh_crypto::SigningPayload) -> Result<Signature, String> {
        let gate = self.gate.lock().unwrap().take();
        if let Some((entered, release)) = gate {
            entered.send(()).map_err(|_| "gate closed")?;
            release
                .recv_timeout(Duration::from_secs(10))
                .map_err(|_| "gate timeout")?;
        }
        if self.fail.load(Ordering::SeqCst) {
            return Err("fixture-secret must not reach status".to_owned());
        }
        Ok(Signature::from_bytes(
            SigningKey::from_bytes(&[63; 32])
                .sign(payload.as_bytes())
                .to_bytes(),
        ))
    }
}
fn schedule() -> CaptureSchedule {
    CaptureSchedule {
        reconciliation_interval: Duration::from_secs(30),
        ..CaptureSchedule::default()
    }
}
fn wait(
    service: &AttachmentCaptureService,
    predicate: impl Fn(&CaptureStatus) -> bool,
) -> CaptureStatus {
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut status = service.status();
    while !predicate(&status) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "capture did not reach expected state: {status:?}"
        );
        status = service.wait_for_update(status.revision, remaining);
    }
    status
}
fn outcome(
    service: &AttachmentCaptureService,
    expected: CaptureOutcome,
    after: u64,
) -> CaptureStatus {
    wait(service, |status| {
        status.phase == CapturePhase::Waiting
            && status.attempts > after
            && status.last_outcome == expected
    })
}

#[test]
fn periodic_capture_and_restart_reconcile_edits_without_any_event_signal() {
    let f = Fixture::new("restart");
    let signer = Arc::new(Signer::default());
    let policy = CaptureSchedule {
        reconciliation_interval: Duration::from_millis(250),
        ..schedule()
    };
    let service = AttachmentCaptureService::start(&f.metadata, signer.clone(), policy).unwrap();
    let first = outcome(&service, CaptureOutcome::Saved, 0);
    fs::write(f.source.join("work"), b"two").unwrap();
    let second = wait(&service, |status| {
        status.saved_version.is_some() && status.saved_version != first.saved_version
    });
    assert_eq!(
        f.attached()
            .saved_file(&f.metadata, first.saved_version.unwrap(), "work")
            .unwrap()
            .unwrap(),
        b"one"
    );
    assert!(second.last_attempt_duration.is_some());
    assert!(second.last_complete_capture_age.is_some());
    assert_eq!(
        service.stop_and_join().unwrap().phase,
        CapturePhase::Stopped
    );
    fs::write(f.source.join("work"), b"edited while stopped").unwrap();
    let restarted = AttachmentCaptureService::start(&f.metadata, signer, policy).unwrap();
    let third = outcome(&restarted, CaptureOutcome::Saved, 0);
    assert_ne!(third.saved_version, second.saved_version);
    assert_eq!(
        third.versions_saved, 1,
        "recovered history was not counted as newly saved"
    );
    restarted.stop_and_join().unwrap();
    assert_eq!(f.attached().saved_versions(&f.metadata).unwrap().len(), 3);
    assert_eq!(
        f.attached()
            .saved_file(&f.metadata, third.saved_version.unwrap(), "work")
            .unwrap()
            .unwrap(),
        b"edited while stopped"
    );
    assert_eq!(fs::read_dir(&f.source).unwrap().count(), 1);
}

#[test]
fn incomplete_and_failed_attempts_retain_the_last_saved_version_and_retry() {
    let f = Fixture::new("recovery");
    let signer = Arc::new(Signer::default());
    let service = AttachmentCaptureService::start(
        &f.metadata,
        signer.clone(),
        CaptureSchedule {
            limits: ObservationLimits {
                entries: 10,
                bytes: 4,
                file_bytes: 4,
            },
            ..schedule()
        },
    )
    .unwrap();
    let first = outcome(&service, CaptureOutcome::Saved, 0);
    fs::write(f.source.join("work"), b"too large").unwrap();
    service.request_capture();
    let incomplete = outcome(&service, CaptureOutcome::Incomplete, first.attempts);
    assert_eq!(incomplete.saved_version, first.saved_version);
    fs::write(f.source.join("work"), b"two").unwrap();
    service.request_capture();
    let second = outcome(&service, CaptureOutcome::Saved, incomplete.attempts);
    signer.fail.store(true, Ordering::SeqCst);
    fs::write(f.source.join("work"), b"tri").unwrap();
    service.request_capture();
    let failed = outcome(&service, CaptureOutcome::SaveUnavailable, second.attempts);
    assert_eq!(failed.saved_version, second.saved_version);
    assert!(!format!("{failed:?}").contains("fixture-secret"));
    signer.fail.store(false, Ordering::SeqCst);
    service.request_capture();
    outcome(&service, CaptureOutcome::Saved, failed.attempts);
    service.stop_and_join().unwrap();
    assert_eq!(f.attached().saved_versions(&f.metadata).unwrap().len(), 3);
}

#[test]
fn repeated_signals_coalesce_while_a_save_is_in_flight() {
    let f = Fixture::new("coalesce");
    let (signer, entered, release) = Signer::gated();
    let service = AttachmentCaptureService::start(&f.metadata, signer, schedule()).unwrap();
    entered.recv_timeout(Duration::from_secs(8)).unwrap();
    fs::write(f.source.join("work"), b"newer input").unwrap();
    for _ in 0..1000 {
        assert!(service.request_capture());
    }
    release.send(()).unwrap();
    let second = outcome(&service, CaptureOutcome::Saved, 1);
    assert_eq!(second.attempts, 2);
    assert_eq!(second.versions_saved, 2);
    service.request_stop();
    assert!(!service.request_capture());
    assert_eq!(
        service.stop_and_join().unwrap().phase,
        CapturePhase::Stopped
    );
    assert_eq!(f.attached().saved_versions(&f.metadata).unwrap().len(), 2);
}

#[test]
fn stopping_during_signing_prevents_that_capture_from_being_committed() {
    let f = Fixture::new("stop");
    let (signer, entered, release) = Signer::gated();
    let service = AttachmentCaptureService::start(&f.metadata, signer, schedule()).unwrap();
    entered.recv_timeout(Duration::from_secs(8)).unwrap();
    service.request_stop();
    assert_eq!(service.status().phase, CapturePhase::Stopping);
    release.send(()).unwrap();
    let stopped = service.stop_and_join().unwrap();
    assert_eq!(stopped.phase, CapturePhase::Stopped);
    assert!(stopped.saved_version.is_none());
    assert_eq!(stopped.last_outcome, CaptureOutcome::Cancelled);
    assert!(f.attached().saved_versions(&f.metadata).unwrap().is_empty());
    assert_eq!(fs::read(f.source.join("work")).unwrap(), b"one");
}

#[test]
fn a_replaced_store_during_signing_is_never_adopted_by_the_worker() {
    let f = Fixture::new("store-replaced");
    let (signer, entered, release) = Signer::gated();
    let service = AttachmentCaptureService::start(&f.metadata, signer, schedule()).unwrap();
    entered.recv_timeout(Duration::from_secs(8)).unwrap();
    let original = f.root.join("original-metadata");
    fs::rename(&f.metadata, &original).unwrap();
    fs::create_dir(&f.metadata).unwrap();
    fs::copy(
        original.join("attachment.json"),
        f.metadata.join("attachment.json"),
    )
    .unwrap();
    release.send(()).unwrap();
    let failed = outcome(&service, CaptureOutcome::SaveUnavailable, 0);
    assert!(failed.saved_version.is_none());
    service.request_capture();
    outcome(&service, CaptureOutcome::StoreUnavailable, failed.attempts);
    service.stop_and_join().unwrap();
    assert_eq!(
        fs::read_dir(&f.metadata).unwrap().count(),
        1,
        "replacement received no Mesh writes"
    );
    assert!(fs::read(original.join(mesh_daemon::RECORD_FILE_NAME))
        .unwrap()
        .is_empty());
}

#[test]
fn a_replaced_project_is_reported_without_capturing_the_replacement() {
    let f = Fixture::new("source-replaced");
    let service =
        AttachmentCaptureService::start(&f.metadata, Arc::new(Signer::default()), schedule())
            .unwrap();
    let first = outcome(&service, CaptureOutcome::Saved, 0);
    let original = f.root.join("original-project");
    fs::rename(&f.source, &original).unwrap();
    fs::create_dir(&f.source).unwrap();
    fs::write(f.source.join("work"), b"unrelated project").unwrap();
    service.request_capture();
    let unavailable = outcome(&service, CaptureOutcome::SourceUnavailable, first.attempts);
    assert_eq!(unavailable.saved_version, first.saved_version);
    service.stop_and_join().unwrap();
    fs::remove_dir_all(&f.source).unwrap();
    fs::rename(&original, &f.source).unwrap();
    assert_eq!(
        f.attached().saved_versions(&f.metadata).unwrap(),
        vec![first.saved_version.unwrap()]
    );
}

#[test]
fn unchanged_capture_updates_health_without_adding_a_version_or_exposing_content() {
    use mesh_daemon::ipc::Json;
    let f = Fixture::new("unchanged");
    let service =
        AttachmentCaptureService::start(&f.metadata, Arc::new(Signer::default()), schedule())
            .unwrap();
    let first = outcome(&service, CaptureOutcome::Saved, 0);
    service.request_capture();
    let same = outcome(&service, CaptureOutcome::Unchanged, first.attempts);
    assert_eq!(same.saved_version, first.saved_version);
    assert_eq!(same.versions_saved, 1);
    let projection = same.to_json();
    assert_eq!(
        projection.get("last_outcome").and_then(Json::as_text),
        Some("unchanged")
    );
    assert_eq!(
        projection.get("attribution").and_then(Json::as_text),
        Some("unknown")
    );
    assert_eq!(projection.get("atomic_snapshot"), Some(&Json::Bool(false)));
    assert!(projection
        .get("last_complete_capture_age_ms")
        .and_then(Json::as_u64)
        .is_some());
    assert!(!projection.encode().contains(f.source.to_str().unwrap()));
    service.stop_and_join().unwrap();
}
