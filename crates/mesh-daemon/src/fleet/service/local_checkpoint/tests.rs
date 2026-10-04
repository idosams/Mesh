use super::super::progress::tests::{fixture, Fixture};
use super::*;
use ed25519_dalek::Signer as _;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

struct Signer {
    key: ed25519_dalek::SigningKey,
    calls: AtomicUsize,
    callback: Box<dyn Fn(usize) -> Result<(), String> + Send + Sync>,
}
impl CheckpointSigner for Signer {
    fn public_key(&self) -> mesh_types::PublicKey {
        mesh_types::PublicKey::from_bytes(self.key.verifying_key().to_bytes())
    }
    fn sign(&self, payload: &mesh_crypto::SigningPayload) -> Result<mesh_types::Signature, String> {
        let count = self.calls.fetch_add(1, Ordering::SeqCst);
        (self.callback)(count)?;
        Ok(mesh_types::Signature::from_bytes(
            self.key.sign(payload.as_bytes()).to_bytes(),
        ))
    }
}
fn signer(callback: impl Fn(usize) -> Result<(), String> + Send + Sync + 'static) -> Arc<Signer> {
    Arc::new(Signer {
        key: ed25519_dalek::SigningKey::from_bytes(&[0x72; 32]),
        calls: AtomicUsize::new(0),
        callback: Box::new(callback),
    })
}
fn credential(f: &Fixture, signer: Arc<Signer>) -> AgentCredential {
    f.service
        .grant_with_signer(&f.lane, "run", "capture", signer)
        .unwrap()
}
fn capture(service: &FleetService, credential: &AgentCredential) -> Result<Json, Unavailable> {
    service.agent_call(
        credential.transport_value(),
        "checkpoint",
        &Json::object([("request", Json::text("capture"))]),
    )
}

#[test]
fn local_checkpoint_signing_keeps_parallel_reads_and_exact_retry() {
    let f = fixture();
    std::fs::write(f.root.join("note.txt"), b"saved progress\n").unwrap();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (resume_tx, resume_rx) = std::sync::mpsc::channel();
    let resume_rx = Mutex::new(resume_rx);
    let signer = signer(move |count| {
        if count == 0 {
            entered_tx.send(()).unwrap();
            resume_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
        }
        Ok(())
    });
    let credential = credential(&f, signer.clone());
    let result = std::thread::scope(|scope| {
        let save = scope.spawn(|| capture(&f.service, &credential));
        entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let (read_tx, read_rx) = std::sync::mpsc::channel();
        let service = &f.service;
        scope.spawn(move || {
            read_tx.send(service.native_state()).unwrap();
        });
        let read = read_rx.recv_timeout(Duration::from_secs(2));
        resume_tx.send(()).unwrap();
        let result = save.join().unwrap().unwrap();
        assert!(read.expect("fleet read blocked behind signer").is_ok());
        result
    });
    assert_eq!(result.get("complete"), Some(&Json::Bool(true)));
    let count = signer.calls.load(Ordering::SeqCst);
    assert!(count > 0);
    std::fs::write(f.root.join("note.txt"), b"later work\n").unwrap();
    assert_eq!(capture(&f.service, &credential).unwrap(), result);
    assert_eq!(signer.calls.load(Ordering::SeqCst), count);
    assert_eq!(
        std::fs::read(f.root.join("note.txt")).unwrap(),
        b"later work\n"
    );
    assert_eq!(f.service.native_state().unwrap().checkpoints.len(), 1);
}

#[test]
fn local_checkpoint_refuses_authority_changed_while_signer_waited() {
    for action in 0..2 {
        let f = fixture();
        let weak = Arc::downgrade(&f.service);
        let retained = Arc::new(Mutex::new(None::<String>));
        let callback_credential = retained.clone();
        std::fs::write(f.root.join("note.txt"), b"unsaved work\n").unwrap();
        let signer = signer(move |count| {
            if count == 0 {
                let service = weak.upgrade().unwrap();
                assert!(service.inner.try_lock().is_ok());
                match action {
                    0 => service
                        .native_command("cancel-save", Command::Cancel)
                        .unwrap(),
                    _ => {
                        let token = callback_credential
                            .lock()
                            .unwrap()
                            .as_ref()
                            .unwrap()
                            .clone();
                        service.revoke(&AgentCredential(token)).unwrap();
                    }
                }
            }
            Ok(())
        });
        let credential = credential(&f, signer);
        *retained.lock().unwrap() = Some(credential.transport_value().to_owned());
        let state = exact_state(&f.service.lock().unwrap().workspaces[&f.lane]).unwrap();
        let result = capture(&f.service, &credential).unwrap();
        assert_eq!(result.get("complete"), Some(&Json::Bool(false)));
        assert_eq!(result.get("saved_changes"), Some(&Json::Number(0)));
        assert_eq!(
            exact_state(&f.service.lock().unwrap().workspaces[&f.lane]).unwrap(),
            state
        );
        assert_eq!(
            std::fs::read(f.root.join("note.txt")).unwrap(),
            b"unsaved work\n"
        );
        let persisted = f.service.native_state().unwrap();
        assert_eq!(persisted.checkpoints.len(), 1);
        assert!(
            !persisted
                .checkpoints
                .values()
                .next()
                .unwrap()
                .result
                .as_ref()
                .unwrap()
                .complete
        );
    }
}

#[test]
fn local_checkpoint_native_custody_never_waits_for_the_fleet_mutex() {
    let f = fixture();
    let inner = f.service.lock().unwrap();
    let key = token_key(f.credential.transport_value());
    let grant = inner.grants[&key].clone();
    let workspace = inner.workspaces[&f.lane].clone();
    let state = exact_state(&workspace).unwrap();
    let _authority = workspace
        .daemon()
        .lock_workspace_agent_setup(
            &state.root,
            &state.digest,
            &state.installation,
            &grant.generation,
        )
        .unwrap();
    std::thread::scope(|scope| {
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let service = &f.service;
        let key = &key;
        let grant = &grant;
        let workspace = &workspace;
        scope.spawn(move || {
            done_tx
                .send(
                    service
                        .try_checkpoint_authority(key, grant, workspace)
                        .is_err(),
                )
                .unwrap();
        });
        let refused = done_rx.recv_timeout(Duration::from_secs(2));
        drop(inner);
        assert!(refused.expect("authority check waited behind the fleet mutex"));
    });
    assert!(f
        .service
        .try_checkpoint_authority(&key, &grant, &workspace)
        .is_ok());
}

#[test]
fn local_checkpoint_real_rotation_cannot_deadlock_with_native_capture() {
    let f = fixture();
    std::fs::write(f.root.join("note.txt"), b"unsaved work\n").unwrap();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (resume_tx, resume_rx) = std::sync::mpsc::channel();
    let resume_rx = Mutex::new(resume_rx);
    let signer = signer(move |count| {
        if count == 0 {
            entered_tx.send(()).unwrap();
            resume_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
        }
        Ok(())
    });
    let credential = credential(&f, signer);
    std::thread::scope(|scope| {
        let save = scope.spawn(|| capture(&f.service, &credential));
        entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let rotation = scope.spawn(|| {
            f.service
                .grant(&f.lane, "run", "rotated-actor", "rotated-session")
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let busy = loop {
            if f.service.inner.try_lock().is_err() {
                break true;
            }
            if std::time::Instant::now() >= deadline {
                break false;
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        resume_tx.send(()).unwrap();
        let result = save.join().unwrap().unwrap();
        let replacement = rotation.join().unwrap().unwrap();
        assert!(busy, "rotation did not enter its native authority wait");
        assert_eq!(result.get("complete"), Some(&Json::Bool(false)));
        assert_eq!(result.get("saved_changes"), Some(&Json::Number(0)));
        assert!(f.service.inspect_worker_progress(&credential).is_err());
        assert_eq!(
            f.service.inspect_worker_progress(&replacement).unwrap(),
            WorkerProgress::Changed
        );
        assert_eq!(
            std::fs::read(f.root.join("note.txt")).unwrap(),
            b"unsaved work\n"
        );
    });
}
