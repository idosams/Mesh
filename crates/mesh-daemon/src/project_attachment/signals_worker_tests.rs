use super::*;
use ed25519_dalek::{Signer as _, SigningKey};
use mesh_types::{PublicKey, Signature};
use std::fs;
use std::sync::mpsc;

struct Signer;
impl CheckpointSigner for Signer {
    fn public_key(&self) -> PublicKey {
        PublicKey::from_bytes(SigningKey::from_bytes(&[73; 32]).verifying_key().to_bytes())
    }
    fn sign(&self, payload: &mesh_crypto::SigningPayload) -> Result<Signature, String> {
        Ok(Signature::from_bytes(
            SigningKey::from_bytes(&[73; 32])
                .sign(payload.as_bytes())
                .to_bytes(),
        ))
    }
}
struct Fixture {
    root: PathBuf,
    source: PathBuf,
    metadata: PathBuf,
}
impl Fixture {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("mesh-signals-{name}-{}", std::process::id()));
        let source = root.join("project");
        let metadata = root.join("metadata");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir(&metadata).unwrap();
        fs::write(source.join("work"), "one").unwrap();
        ProjectAttachment::register(&source, &metadata).unwrap();
        Self {
            root,
            source,
            metadata,
        }
    }
    fn start<F>(&self, interval: Duration, start: F) -> AttachmentCaptureService
    where
        F: FnOnce(&Arc<Shared>, &Path) + Send + 'static,
    {
        let attachment = ProjectAttachment::reopen(&self.metadata).unwrap();
        let store = external_store(&self.metadata, &attachment).unwrap();
        AttachmentCaptureService::start_pinned_with_signals(
            &self.metadata,
            attachment,
            store,
            Arc::new(Signer),
            CaptureSchedule {
                native_signals: true,
                reconciliation_interval: interval,
                ..CaptureSchedule::default()
            },
            start,
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn wait(
    service: &AttachmentCaptureService,
    predicate: impl Fn(&CaptureStatus) -> bool,
) -> CaptureStatus {
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut state = service.status();
    while !predicate(&state) && Instant::now() < deadline {
        state = service.wait_for_update(
            state.revision,
            deadline.saturating_duration_since(Instant::now()),
        );
    }
    state
}

#[test]
fn blocked_native_registration_does_not_block_capture_periodic_progress_or_stop() {
    let f = Fixture::new("blocked-register");
    let budget = signals_worker::Budget::new(1);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (helper_tx, helper_rx) = mpsc::channel();
    let bound = Arc::clone(&budget);
    let service = f.start(Duration::from_millis(250), move |shared, _| {
        let helper = signals_worker::start_with(shared, bound, move || {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok(())
        })
        .unwrap();
        helper_tx.send(helper).unwrap();
    });
    entered_rx.recv_timeout(Duration::from_secs(8)).unwrap();
    let first = wait(&service, |state| state.saved_version.is_some());
    fs::write(f.source.join("work"), "two").unwrap();
    let second = wait(&service, |state| {
        state.saved_version.is_some() && state.saved_version != first.saved_version
    });
    let shared = Arc::clone(&service.shared);
    let (stopped_tx, stopped_rx) = mpsc::channel();
    let stopping = thread::spawn(move || stopped_tx.send(service.stop_and_join()).unwrap());
    let stopped_before_release = stopped_rx.recv_timeout(Duration::from_secs(8)).ok();
    let capacity_retained = budget.occupied() == 1;
    // Always release the parked helper before asserting, including under a deliberately broken
    // synchronous-start mutant. The probe cannot leave a live thread behind after failure.
    release_tx.send(()).unwrap();
    stopping.join().unwrap();
    helper_rx.recv().unwrap().join().unwrap();
    assert!(
        first.saved_version.is_some(),
        "initial capture waited for optional registration"
    );
    assert_eq!(first.native_signal_state, NativeSignalState::Starting);
    assert_ne!(
        second.saved_version, first.saved_version,
        "periodic capture waited for registration"
    );
    let stopped = stopped_before_release
        .expect("capture stop waited for optional registration")
        .unwrap();
    assert_eq!(stopped.phase, CapturePhase::Stopped);
    assert!(!stopped.native_events);
    assert_eq!(stopped.native_signal_state, NativeSignalState::Stopping);
    assert!(
        capacity_retained,
        "blocked registrations must retain their capacity slot"
    );
    let final_state = Shared::snapshot(&shared.lock());
    assert_eq!(final_state.phase, CapturePhase::Stopped);
    assert_eq!(final_state.native_signal_state, NativeSignalState::Stopped);
    assert_eq!(
        final_state.attempts, stopped.attempts,
        "late registration restarted capture"
    );
    assert!(!final_state.native_events);
    assert_eq!(budget.occupied(), 0);
}

#[test]
fn native_activation_reconciles_edits_made_while_registration_was_pending() {
    let f = Fixture::new("activation-rescan");
    let (release_tx, release_rx) = mpsc::channel();
    let (helper_tx, helper_rx) = mpsc::channel();
    let service = f.start(Duration::from_secs(300), move |shared, _| {
        helper_tx
            .send(
                signals_worker::start_with(shared, signals_worker::Budget::new(1), move || {
                    release_rx.recv().unwrap();
                    Ok(())
                })
                .unwrap(),
            )
            .unwrap();
    });
    let first = wait(&service, |state| state.saved_version.is_some());
    fs::write(f.source.join("work"), "edited before native activation").unwrap();
    release_tx.send(()).unwrap();
    let second = wait(&service, |state| {
        state.saved_version.is_some() && state.saved_version != first.saved_version
    });
    service.stop_and_join().unwrap();
    helper_rx.recv().unwrap().join().unwrap();
    assert!(first.saved_version.is_some());
    assert_ne!(
        first.saved_version, second.saved_version,
        "activation did not reconcile the pending gap"
    );
    let saved = ProjectAttachment::reopen(&f.metadata)
        .unwrap()
        .saved_file(&f.metadata, second.saved_version.unwrap(), "work")
        .unwrap();
    assert_eq!(saved, Some(b"edited before native activation".to_vec()));
}

struct CleanupGate {
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}
impl Drop for CleanupGate {
    fn drop(&mut self) {
        self.entered.send(()).unwrap();
        self.release.recv().unwrap();
    }
}
#[test]
fn blocked_native_cleanup_keeps_its_slot_and_live_status_after_capture_join() {
    let f = Fixture::new("blocked-cleanup");
    let budget = signals_worker::Budget::new(1);
    let bound = Arc::clone(&budget);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (helper_tx, helper_rx) = mpsc::channel();
    let mut service = f.start(Duration::from_secs(300), move |shared, _| {
        helper_tx
            .send(
                signals_worker::start_with(shared, bound, move || {
                    Ok(CleanupGate {
                        entered: entered_tx,
                        release: release_rx,
                    })
                })
                .unwrap(),
            )
            .unwrap();
    });
    let first = wait(&service, |state| {
        state.saved_version.is_some() && state.native_events
    });
    let stopped = service.stop_capture_and_join().unwrap();
    entered_rx.recv_timeout(Duration::from_secs(8)).unwrap();
    let pending = service.status();
    // Use a different capture's status: exhausting optional monitoring cannot stop its captures.
    let other = Fixture::new("capacity-fallback");
    let fallback_budget = Arc::clone(&budget);
    let fallback = other.start(Duration::from_millis(250), move |shared, _| {
        assert!(signals_worker::start_with(shared, fallback_budget, || Ok(())).is_err());
    });
    let saved = wait(&fallback, |state| state.saved_version.is_some());
    fallback.stop_and_join().unwrap();
    release_tx.send(()).unwrap();
    helper_rx.recv().unwrap().join().unwrap();
    assert!(first.native_events);
    assert_eq!(stopped.phase, CapturePhase::Stopped);
    assert_eq!(pending.native_signal_state, NativeSignalState::Stopping);
    assert!(saved.saved_version.is_some());
    assert_eq!(saved.native_signal_state, NativeSignalState::Unavailable);
    assert_eq!(
        service.status().native_signal_state,
        NativeSignalState::Stopped
    );
    assert_eq!(budget.occupied(), 0);
}

#[test]
fn native_registration_failure_and_unwind_leave_capture_available() {
    for panic in [false, true] {
        let f = Fixture::new(if panic {
            "registration-unwind"
        } else {
            "registration-error"
        });
        let (helper_tx, helper_rx) = mpsc::channel();
        let service = f.start(Duration::from_secs(300), move |shared, _| {
            helper_tx
                .send(
                    signals_worker::start_with(
                        shared,
                        signals_worker::Budget::new(1),
                        move || -> io::Result<()> {
                            assert!(!panic, "injected native registration unwind");
                            Err(io::Error::other("private native failure"))
                        },
                    )
                    .unwrap(),
                )
                .unwrap();
        });
        helper_rx.recv().unwrap().join().unwrap();
        let saved = wait(&service, |state| state.saved_version.is_some());
        service.stop_and_join().unwrap();
        assert!(saved.saved_version.is_some());
        assert_eq!(saved.native_signal_state, NativeSignalState::Unavailable);
        assert!(!saved.to_json().encode().contains("private native failure"));
    }
}
