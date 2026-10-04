use super::super::progress::tests::{fixture, Fixture};
use super::*;
use ed25519_dalek::Signer as _;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
struct Signer(AtomicUsize);
impl CheckpointSigner for Signer {
    fn public_key(&self) -> mesh_types::PublicKey {
        mesh_types::PublicKey::from_bytes(
            ed25519_dalek::SigningKey::from_bytes(&[0x76; 32])
                .verifying_key()
                .to_bytes(),
        )
    }
    fn sign(&self, payload: &mesh_crypto::SigningPayload) -> Result<mesh_types::Signature, String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(mesh_types::Signature::from_bytes(
            ed25519_dalek::SigningKey::from_bytes(&[0x76; 32])
                .sign(payload.as_bytes())
                .to_bytes(),
        ))
    }
}
fn running(f: &Fixture) -> (AgentCredential, Arc<Signer>) {
    f.service
        .native_command(
            "running",
            Command::Observe {
                lane: f.lane.clone(),
                run: "run".into(),
                state: RunState::Running,
            },
        )
        .unwrap();
    let signer = Arc::new(Signer(AtomicUsize::new(0)));
    (
        f.service
            .grant_with_signer(&f.lane, "run", "automatic", signer.clone())
            .unwrap(),
        signer,
    )
}
#[test]
fn automatic_progress_uses_native_history_without_handoff_events_or_empty_poll_events() {
    let f = fixture();
    let (credential, signer) = running(&f);
    let before = f.service.native_state().unwrap();
    for _ in 0..4 {
        let result = f.service.save_worker_progress(&credential).unwrap();
        assert!(result.complete);
        assert_eq!(result.saved_changes, 0);
    }
    assert_eq!(f.service.native_state().unwrap(), before);
    assert_eq!(signer.0.load(Ordering::SeqCst), 0);
    std::fs::write(f.root.join("note.txt"), b"automatic progress\n").unwrap();
    let saved = f.service.save_worker_progress(&credential).unwrap();
    assert!(saved.complete && !saved.observation_pending && saved.saved_changes > 0);
    let after = f.service.native_state().unwrap();
    assert!(after.checkpoints.is_empty());
    assert_eq!(after.lanes[&f.lane].saved, Some(saved.version));
    let calls = signer.0.load(Ordering::SeqCst);
    for _ in 0..4 {
        assert!(
            f.service
                .save_worker_progress(&credential)
                .unwrap()
                .complete
        );
    }
    assert_eq!(f.service.native_state().unwrap(), after);
    assert_eq!(signer.0.load(Ordering::SeqCst), calls);
    assert_eq!(
        std::fs::read(f.root.join("note.txt")).unwrap(),
        b"automatic progress\n"
    );
    let handoff = f
        .service
        .agent_call(
            credential.transport_value(),
            "checkpoint",
            &Json::object([("request", Json::text("handoff"))]),
        )
        .unwrap();
    assert_eq!(handoff.get("complete"), Some(&Json::Bool(true)));
    assert_eq!(f.service.native_state().unwrap().checkpoints.len(), 1);
}
#[test]
fn automatic_progress_retries_a_missed_fleet_observation_without_signing_again() {
    let f = fixture();
    let (credential, signer) = running(&f);
    std::fs::write(f.root.join("note.txt"), b"retained progress\n").unwrap();
    let (start_tx, start_rx) = std::sync::mpsc::channel();
    let (held_tx, held_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let saved = std::thread::scope(|scope| {
        let service = &f.service;
        scope.spawn(move || {
            start_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            let _held = service.lock().unwrap();
            held_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        });
        let saved = f
            .service
            .save_worker_progress_then(&credential, || {
                start_tx.send(()).unwrap();
                held_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            })
            .unwrap();
        release_tx.send(()).unwrap();
        saved
    });
    assert!(saved.complete && saved.observation_pending);
    assert!(f.service.native_state().unwrap().lanes[&f.lane]
        .saved
        .is_none());
    let calls = signer.0.load(Ordering::SeqCst);
    let retry = f.service.save_worker_progress(&credential).unwrap();
    assert!(retry.complete && !retry.observation_pending);
    assert_eq!(retry.version, saved.version);
    assert_eq!(retry.saved_changes, 0);
    assert_eq!(signer.0.load(Ordering::SeqCst), calls);
    assert_eq!(
        f.service.native_state().unwrap().lanes[&f.lane].saved,
        Some(saved.version)
    );
}
#[test]
fn automatic_progress_preserves_missing_entries_and_refuses_revoked_sessions() {
    let f = fixture();
    let (credential, signer) = running(&f);
    std::fs::remove_file(f.root.join("note.txt")).unwrap();
    let report = f.service.save_worker_progress(&credential).unwrap();
    assert!(!report.complete);
    assert_eq!(report.saved_changes, 0);
    assert!(f.service.native_state().unwrap().checkpoints.is_empty());
    assert_eq!(signer.0.load(Ordering::SeqCst), 0);
    std::fs::write(f.root.join("note.txt"), b"not authorized\n").unwrap();
    f.service.revoke(&credential).unwrap();
    assert!(f.service.save_worker_progress(&credential).is_err());
    assert_eq!(signer.0.load(Ordering::SeqCst), 0);
}

#[test]
fn automatic_progress_cannot_replace_a_newer_explicit_checkpoint() {
    let f = fixture();
    let (credential, signer) = running(&f);
    std::fs::write(f.root.join("note.txt"), b"first private version\n").unwrap();
    let older = f
        .service
        .save_worker_progress_then(&credential, || {
            std::fs::write(f.root.join("note.txt"), b"newer explicit version\n").unwrap();
            let handoff = f
                .service
                .agent_call(
                    credential.transport_value(),
                    "checkpoint",
                    &Json::object([("request", Json::text("newer"))]),
                )
                .unwrap();
            assert_eq!(handoff.get("complete"), Some(&Json::Bool(true)));
        })
        .unwrap();
    assert!(older.complete && older.observation_pending);
    let newer = f.service.native_state().unwrap().lanes[&f.lane]
        .saved
        .unwrap();
    assert_ne!(newer, older.version);
    let calls = signer.0.load(Ordering::SeqCst);
    let retried = f.service.save_worker_progress(&credential).unwrap();
    assert!(retried.complete && !retried.observation_pending);
    assert_eq!(retried.version, newer);
    assert_eq!(retried.saved_changes, 0);
    assert_eq!(signer.0.load(Ordering::SeqCst), calls);
    assert_eq!(
        f.service.native_state().unwrap().lanes[&f.lane].saved,
        Some(newer)
    );
}

#[test]
fn automatic_progress_rechecks_cancellation_after_signing() {
    struct Cancelling(std::sync::Weak<FleetService>);
    impl CheckpointSigner for Cancelling {
        fn public_key(&self) -> mesh_types::PublicKey {
            Signer(AtomicUsize::new(0)).public_key()
        }
        fn sign(
            &self,
            payload: &mesh_crypto::SigningPayload,
        ) -> Result<mesh_types::Signature, String> {
            self.0
                .upgrade()
                .unwrap()
                .native_command("cancel-autosave", Command::Cancel)
                .unwrap();
            Signer(AtomicUsize::new(0)).sign(payload)
        }
    }
    let f = fixture();
    let _ = running(&f);
    let credential = f
        .service
        .grant_with_signer(
            &f.lane,
            "run",
            "cancel-save",
            Arc::new(Cancelling(Arc::downgrade(&f.service))),
        )
        .unwrap();
    let before = exact_state(&f.service.lock().unwrap().workspaces[&f.lane]).unwrap();
    std::fs::write(f.root.join("note.txt"), b"preserved unsaved work\n").unwrap();
    let result = f.service.save_worker_progress(&credential).unwrap();
    assert!(!result.complete);
    assert_eq!(result.saved_changes, 0);
    assert_eq!(
        exact_state(&f.service.lock().unwrap().workspaces[&f.lane]).unwrap(),
        before
    );
    assert!(f.service.native_state().unwrap().lanes[&f.lane]
        .saved
        .is_none());
    assert_eq!(
        std::fs::read(f.root.join("note.txt")).unwrap(),
        b"preserved unsaved work\n"
    );
}
