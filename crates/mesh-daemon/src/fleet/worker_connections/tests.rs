use super::*;
use crate::fleet::receiving_broker::tests::{authenticate, finish, read, response, send};
use crate::fleet::receiving_session::tests::Setup;
use ed25519_dalek::{Signer as _, SigningKey};
use mesh_types::PublicKey;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::sync::{mpsc, Arc};
fn public(key: &SigningKey) -> PublicKey {
    PublicKey::from_bytes(key.verifying_key().to_bytes())
}
fn sign(key: &SigningKey, payload: &SigningPayload) -> Result<Signature, String> {
    Ok(Signature::from_bytes(
        key.sign(payload.as_bytes()).to_bytes(),
    ))
}
fn installation(s: &Setup) -> NativeWorkerInstallation {
    let path = s.f.path.join("installation");
    fs::create_dir(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    NativeWorkerInstallation::provision(
        &path,
        crate::ProtectedWorkspaceRoot::inspect(&path).unwrap(),
        &[],
        |_| Ok((public(&s.f.worker), ())),
    )
    .unwrap()
    .0
}
fn policy(s: &Setup) -> RemoteDispatchPolicy<'static> {
    RemoteDispatchPolicy {
        coordinator: public(&s.f.coordinator),
        worker: public(&s.f.worker),
        provider: "codex",
        maximum: Limits {
            lanes: 2,
            concurrency: 1,
            depth: 1,
            retries: 1,
        },
        max_lease_ms: 60_000,
    }
}
fn control(frame: RemoteFrame) -> String {
    let RemoteFrame::Control(bytes) = frame else {
        panic!("control expected")
    };
    String::from_utf8(bytes).unwrap()
}
struct TestSigner(SigningKey);
impl service::CheckpointSigner for TestSigner {
    fn public_key(&self) -> PublicKey {
        public(&self.0)
    }
    fn sign(&self, payload: &SigningPayload) -> Result<Signature, String> {
        sign(&self.0, payload)
    }
}
struct Signers(SigningKey);
impl host::WorkerSignerFactory for Signers {
    fn signer(
        &self,
        _: &str,
        _: &str,
    ) -> Result<Arc<dyn service::CheckpointSigner>, crate::ipc::Unavailable> {
        Ok(Arc::new(TestSigner(self.0.clone())))
    }
}
fn launch(s: &Setup) -> ReceivedWorkerLaunch {
    let executable = s.f.path.join("provider");
    if !executable.exists() {
        fs::write(&executable,b"#!/bin/sh\ncat >/dev/null\nprintf 'one\\n' >> launches.txt\nn=0\nwhile [ ! -f finish ]; do n=$((n+1)); [ $n -lt 1000 ] || exit 91; sleep 0.01; done\nprintf '%s\\n' '{\"type\":\"turn.completed\"}'\n").unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    }
    ReceivedWorkerLaunch {
        adapter: provider::CodexAdapter::new(&executable, &executable)
            .unwrap()
            .into(),
        endpoint: crate::ProtectedWorkspaceRoot::inspect(&s.f.path.join("bridge"))
            .unwrap()
            .stable_reference()
            .unwrap()
            .join("provider.sock"),
        signers: Arc::new(Signers(s.f.worker.clone())),
        reviewers: crate::TrustedReviewers::default(),
        checkpoint: crate::CheckpointRuntimeParameters::selected_defaults(),
    }
}
fn connect(
    root: &std::path::Path,
    endpoint: &NativeWorkerEndpoint,
) -> (NativeWorkerStream, NativeWorkerStream) {
    let client = NativeWorkerEndpoint::connect(
        root,
        crate::ProtectedWorkspaceRoot::inspect(root).unwrap(),
        std::time::Duration::from_secs(5),
    )
    .unwrap();
    let server = endpoint
        .accept(std::time::Duration::from_secs(5))
        .unwrap()
        .unwrap();
    (client, server)
}
#[test]
fn authenticated_reconnect_preserves_input_and_delivers_original_handoff_once() {
    let s = Setup::new();
    let installation = installation(&s);
    let mut hub =
        NativeWorkerConnections::new(&installation, &s.destination, policy(&s), 1).unwrap();
    let endpoint_root = s.f.path.join("bridge");
    fs::create_dir(&endpoint_root).unwrap();
    fs::set_permissions(&endpoint_root, fs::Permissions::from_mode(0o700)).unwrap();
    let endpoint = NativeWorkerEndpoint::bind(
        &endpoint_root,
        crate::ProtectedWorkspaceRoot::inspect(&endpoint_root).unwrap(),
        &[],
    )
    .unwrap();
    let mut runtime = s.f.runtime(false);
    let challenge = RemotePeerChallenge::issue(
        &mut runtime,
        "lane",
        "run",
        s.f.work.assignment.clone(),
        public(&s.f.worker),
    )
    .unwrap();
    let dispatch = challenge
        .signed_dispatch(&mut runtime, &public(&s.f.coordinator), |p| {
            sign(&s.f.coordinator, p)
        })
        .unwrap();
    let (mut client, server) = connect(&endpoint_root, &endpoint);
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            hub.serve(server.try_clone().unwrap(), server, |p| {
                sign(&s.f.worker, p)
            })
        });
        send(&mut client, dispatch.frame().unwrap());
        challenge
            .verify_dispatch_reply(&mut runtime, "claim", &control(read(&mut client)))
            .unwrap();
        authenticate(&s, &mut runtime, &mut client);
        send(&mut client, s.manifest_frame());
        response(&mut client, "manifest");
        send(&mut client, s.part(0, 7));
        response(&mut client, "chunk");
        drop(client);
        assert!(matches!(
            worker.join().unwrap().unwrap(),
            WorkerConnectionOutcome::Disconnected
        ));
    });
    assert_eq!(hub.transfers.len(), 1);
    // A fresh, correctly signed message still cannot change an already retained assignment,
    // or allocate a second assignment beyond native capacity.
    let mut other = Setup::new();
    other.f.work.goal = "Changed task".into();
    other.f.work.assignment = s.f.work.assignment.clone();
    let mut other_runtime = other.f.runtime(false);
    for assignment_id in [s.f.work.assignment.id.as_str(), "second-assignment"] {
        let mut assignment = other.f.work.assignment.clone();
        assignment.id = assignment_id.into();
        let challenge = RemotePeerChallenge::issue(
            &mut other_runtime,
            "lane",
            "run",
            assignment,
            public(&other.f.worker),
        )
        .unwrap();
        let dispatch = challenge
            .signed_dispatch(&mut other_runtime, &public(&other.f.coordinator), |p| {
                sign(&other.f.coordinator, p)
            })
            .unwrap();
        let mut wire = Vec::new();
        RemoteFrameWriter::new(&mut wire)
            .write_frame(&dispatch.frame().unwrap())
            .unwrap();
        assert!(hub
            .serve(wire.as_slice(), Vec::new(), |_| panic!(
                "changed or excess work cannot sign"
            ))
            .is_err());
        assert_eq!(hub.transfers.len(), 1);
    }

    let reconnect =
        RemoteInputReconnectChallenge::issue(&mut runtime, "lane", "run", public(&s.f.worker))
            .unwrap();
    let dispatch = reconnect
        .signed_dispatch(&mut runtime, &public(&s.f.coordinator), |p| {
            sign(&s.f.coordinator, p)
        })
        .unwrap();
    let replay = dispatch.frame().unwrap();
    let (mut client, server) = connect(&endpoint_root, &endpoint);
    let admission = std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            hub.serve(server.try_clone().unwrap(), server, |p| {
                sign(&s.f.worker, p)
            })
        });
        send(&mut client, dispatch.frame().unwrap());
        reconnect
            .verify_dispatch_reply(&mut runtime, &control(read(&mut client)))
            .unwrap();
        authenticate(&s, &mut runtime, &mut client);
        send(
            &mut client,
            RemoteReceivingCommand::Status {
                request: "offset".into(),
                digest: s.digest,
            }
            .frame()
            .unwrap(),
        );
        assert_eq!(
            response(&mut client, "chunk")
                .get("detail")
                .unwrap()
                .get("offset")
                .and_then(|value| match value {
                    crate::ipc::Json::Number(n) => Some(*n),
                    _ => None,
                }),
            Some(7)
        );
        send(&mut client, s.part(7, s.bytes.len()));
        response(&mut client, "chunk");
        finish(&mut client);
        response(&mut client, "materialized");
        drop(client);
        let WorkerConnectionOutcome::Materialized {
            admission,
            reply_written,
        } = worker.join().unwrap().unwrap()
        else {
            panic!("materialization expected")
        };
        assert!(reply_written);
        *admission
    });
    let retained = hub.transfers[0].handoff.as_ref().unwrap();
    assert_eq!(
        fs::read(retained.allocation.path().join("result.txt")).unwrap(),
        s.bytes
    );
    assert_eq!(retained.registry.receipts().unwrap().len(), 1);
    let mut encoded = Vec::new();
    RemoteFrameWriter::new(&mut encoded)
        .write_frame(&replay)
        .unwrap();
    assert!(hub
        .serve(encoded.as_slice(), Vec::new(), |_| panic!(
            "terminal input must not sign another proof"
        ))
        .is_err());
    let (closed, receiver) = mpsc::sync_channel(1);
    drop(receiver);
    let (reply, _replies) = mpsc::sync_channel(1);
    assert!(!hub
        .queue_received(&admission, launch(&s), reply.clone(), &closed)
        .unwrap());
    assert!(hub.transfers[0].handoff.is_none());
    assert!(hub.transfers[0].pending.is_some());
    assert!(hub
        .queue_received(&admission, launch(&s), reply, &closed)
        .is_err());
    let (sender, receiver) = mpsc::sync_channel(1);
    let (cancel_reply, _cancel_replies) = mpsc::sync_channel(1);
    sender
        .try_send(ReceivedWorkerRequest::Cancel {
            admission: admission.clone(),
            reply: cancel_reply,
        })
        .unwrap();
    assert_eq!(hub.flush_pending(&sender).unwrap(), 0);
    assert!(hub.transfers[0].pending.is_some());
    assert!(matches!(
        receiver.recv().unwrap(),
        ReceivedWorkerRequest::Cancel { .. }
    ));
    assert_eq!(hub.flush_pending(&sender).unwrap(), 1);
    assert_eq!(hub.flush_pending(&sender).unwrap(), 0);
    let ReceivedWorkerRequest::Start {
        handoff, launch, ..
    } = receiver.recv().unwrap()
    else {
        panic!("original start expected")
    };
    assert!(handoff.allocation.admission.as_ref() == Some(&admission));
    let original_input = handoff.allocation.path().to_owned();
    let mut supervisor = ReceivedWorkerSupervisor::new(1, public(&s.f.worker)).unwrap();
    assert!(supervisor.start_received(*handoff, *launch).unwrap() == admission);
    let snapshot = supervisor.snapshot(&admission).unwrap();
    let workspace = std::path::PathBuf::from(
        snapshot.get("lanes").unwrap().as_array().unwrap()[0]
            .get("workspace")
            .unwrap()
            .get("root")
            .unwrap()
            .as_text()
            .unwrap(),
    );
    assert_ne!(workspace, original_input);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while fs::read_to_string(workspace.join("launches.txt"))
        .ok()
        .as_deref()
        != Some("one\n")
    {
        assert!(
            std::time::Instant::now() < deadline,
            "fixture provider did not acknowledge startup"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let (commands, mailbox) = ReceivedWorkerMailbox::bounded();
    drop(commands);
    let (observations, observed) = mpsc::sync_channel(1);
    let stop = std::sync::atomic::AtomicBool::new(false);
    struct StopOnDrop<'a>(&'a std::sync::atomic::AtomicBool);
    impl Drop for StopOnDrop<'_> {
        fn drop(&mut self) {
            self.0.store(true, std::sync::atomic::Ordering::Release);
        }
    }
    std::thread::scope(|scope| {
        let _stop_on_unwind = StopOnDrop(&stop);
        let stop_ref = &stop;
        let supervisor_ref = &mut supervisor;
        scope.spawn(move || supervisor_ref.serve(&mailbox, stop_ref, &observations));
        fs::write(workspace.join("finish"), b"finish").unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut completed = false;
        while std::time::Instant::now() < deadline {
            if let Ok(samples) = observed.recv_timeout(std::time::Duration::from_millis(100)) {
                if samples.iter().any(|sample| {
                    sample.admission == admission
                        && sample
                            .observation
                            .as_ref()
                            .is_ok_and(|items| items.iter().any(|item| item.outcome == Some(true)))
                }) {
                    completed = true;
                    break;
                }
            }
        }
        stop.store(true, std::sync::atomic::Ordering::Release);
        assert!(
            completed,
            "provider must finish after both bridge connections and control senders close"
        );
    });
    assert_eq!(
        fs::read_to_string(workspace.join("launches.txt")).unwrap(),
        "one\n"
    );
    assert!(!original_input.join("launches.txt").exists());
    assert_eq!(supervisor.admissions().len(), 1);
}
#[test]
fn malformed_dispatch_and_wrong_native_identity_cannot_allocate_or_sign() {
    let s = Setup::new();
    let installation = installation(&s);
    let mut wrong = policy(&s);
    wrong.worker = PublicKey::from_bytes([1; 32]);
    assert!(NativeWorkerConnections::new(&installation, &s.destination, wrong, 1).is_err());
    for maximum in [0, 65] {
        assert!(
            NativeWorkerConnections::new(&installation, &s.destination, policy(&s), maximum)
                .is_err()
        );
    }
    let mut hub =
        NativeWorkerConnections::new(&installation, &s.destination, policy(&s), 1).unwrap();
    let mut wire = Vec::new();
    RemoteFrameWriter::new(&mut wire)
        .write_frame(&RemoteFrame::Control(b"{}".to_vec()))
        .unwrap();
    assert!(hub
        .serve(wire.as_slice(), Vec::new(), |_| panic!(
            "invalid dispatch cannot sign"
        ))
        .is_err());
    assert!(hub.transfers.is_empty());
    s.assert_empty_store();
}

#[test]
fn explicit_signed_input_inspection_routes_without_reserving_a_transfer() {
    let s = Setup::new();
    let installation = installation(&s);
    let mut hub =
        NativeWorkerConnections::new(&installation, &s.destination, policy(&s), 1).unwrap();
    let mut runtime = s.f.runtime(true);
    let challenge = RemoteWorkerStatusChallenge::issue_with_input_inspection(
        &mut runtime,
        "lane",
        "run",
        public(&s.f.coordinator),
        public(&s.f.worker),
    )
    .unwrap();
    let query = challenge
        .signed_query(&mut runtime, |p| sign(&s.f.coordinator, p))
        .unwrap();
    let mut input = Vec::new();
    RemoteFrameWriter::new(&mut input)
        .write_frame(&query.frame().unwrap())
        .unwrap();
    let mut output = Vec::new();
    assert!(matches!(
        hub.serve(input.as_slice(), &mut output, |p| sign(&s.f.worker, p))
            .unwrap(),
        WorkerConnectionOutcome::StatusReplied
    ));
    let frame = RemoteFrameReader::new(output.as_slice())
        .read_frame()
        .unwrap()
        .unwrap();
    let receipt = challenge
        .verify_reply(&mut runtime, &control(frame))
        .unwrap();
    assert_eq!(receipt.input_inspection(), Some("unrecorded"));
    assert!(hub.transfers.is_empty());
    assert_eq!(
        fs::read_dir(s.f.path.join("allocations")).unwrap().count(),
        0
    );
}
