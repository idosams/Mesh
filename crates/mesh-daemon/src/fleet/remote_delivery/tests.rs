use super::*;
use crate::fleet::{input_transfer::tests::Export, receiving_session::tests::Setup, *};
use crate::ProtectedWorkspaceRoot;
use ed25519_dalek::{Signer as _, SigningKey};
use mesh_types::PublicKey;
use std::fs;
use std::os::unix::{fs::PermissionsExt as _, net::UnixStream};
fn public(key: &SigningKey) -> PublicKey {
    PublicKey::from_bytes(key.verifying_key().to_bytes())
}
fn sign(key: &SigningKey, body: &SigningPayload) -> Result<Signature, String> {
    Ok(Signature::from_bytes(key.sign(body.as_bytes()).to_bytes()))
}
fn request<'a>(
    s: &Setup,
    runtime: &'a mut Runtime,
    export: &'a Export,
) -> RemoteInputTransferRequest<'a> {
    RemoteInputTransferRequest {
        runtime,
        lane: "lane",
        run: "run",
        source: &export.source,
        coordinator: public(&s.f.coordinator),
        worker: public(&s.f.worker),
    }
}
fn claim(s: &Setup) -> RemoteInputDeliveryIntent {
    RemoteInputDeliveryIntent::Claim {
        assignment: s.f.work.assignment.clone(),
        request: "native-claim".into(),
    }
}
fn limits() -> Limits {
    Limits {
        lanes: 2,
        concurrency: 1,
        depth: 1,
        retries: 1,
    }
}
fn pair() -> (UnixStream, UnixStream) {
    let pair = UnixStream::pair().unwrap();
    for stream in [&pair.0, &pair.1] {
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
    }
    pair
}
fn installation(s: &Setup) -> NativeWorkerInstallation {
    let path = s.f.path.join("installation");
    fs::create_dir(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    NativeWorkerInstallation::provision(
        &path,
        ProtectedWorkspaceRoot::inspect(&path).unwrap(),
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
        maximum: limits(),
        max_lease_ms: 60_000,
    }
}
struct FailSecondChunk {
    stream: UnixStream,
    chunks: usize,
}
impl Write for FailSecondChunk {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() == 10 && &bytes[..4] == b"MSHR" && bytes[5] == 3 {
            self.chunks += 1;
            if self.chunks == 2 {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "injected connection loss",
                ));
            }
        }
        self.stream.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}
#[test]
fn signed_delivery_disconnect_then_explicit_reconnect_preserves_one_assignment() {
    let export = Export::new();
    let s = export.setup();
    let installation = installation(&s);
    let mut hub =
        NativeWorkerConnections::new(&installation, &s.destination, policy(&s), 1).unwrap();
    let mut runtime = s.f.runtime(false);
    let (client1, server1) = pair();
    let (client2, server2) = pair();
    let mut signs = 0;
    let mut signer = |body: &SigningPayload| {
        signs += 1;
        sign(&s.f.coordinator, body)
    };
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            assert!(matches!(
                hub.serve(server1.try_clone().unwrap(), server1, |p| sign(
                    &s.f.worker,
                    p
                ))
                .unwrap(),
                WorkerConnectionOutcome::Disconnected
            ));
            hub.serve(server2.try_clone().unwrap(), server2, |p| {
                sign(&s.f.worker, p)
            })
            .unwrap()
        });
        let mut first = request(&s, &mut runtime, &export);
        let prepared = prepare(&mut first, claim(&s), &mut signer).unwrap();
        assert!(deliver(
            first,
            prepared,
            client1.try_clone().unwrap(),
            FailSecondChunk {
                stream: client1,
                chunks: 0
            },
            &mut signer
        )
        .is_err());
        assert!(runtime.state().lanes["lane"]
            .runs
            .last()
            .unwrap()
            .remote
            .is_some());
        let mut resumed = request(&s, &mut runtime, &export);
        assert!(
            prepare(&mut resumed, claim(&s), &mut signer).is_err(),
            "loss cannot grant a second initial claim"
        );
        let prepared = prepare(
            &mut resumed,
            RemoteInputDeliveryIntent::Reconnect,
            &mut signer,
        )
        .unwrap();
        assert!(matches!(
            deliver(
                resumed,
                prepared,
                client2.try_clone().unwrap(),
                client2,
                &mut signer
            )
            .unwrap(),
            RemoteInputTransferOutcome::Materialized(_)
        ));
        let WorkerConnectionOutcome::Materialized { admission, .. } = worker.join().unwrap() else {
            panic!("one materialization expected");
        };
        assert_eq!(admission.work().assignment, s.f.work.assignment);
        assert_eq!(
            installation
                .registry(admission.coordinator(), admission.objective(), limits())
                .unwrap()
                .receipts()
                .unwrap()
                .len(),
            1
        );
        let path =
            s.f.path
                .join("allocations")
                .join(format!("input-{}", admission.allocation()))
                .join("files");
        assert_eq!(fs::read(path.join("binary")).unwrap(), vec![0xff; 140_000]);
        assert_eq!(fs::read(path.join("zero")).unwrap(), Vec::<u8>::new());
        assert!(path.join("empty").is_dir());
    });
    assert_eq!(
        signs, 4,
        "only dispatch and admission for each explicit connection"
    );
    assert_eq!(runtime.state().lanes["lane"].runs.len(), 1);
}
#[test]
fn mismatched_saved_input_refuses_before_signing_or_opening_transport() {
    let export = Export::new();
    let s = export.setup();
    let mut runtime = s.f.runtime(false);
    let mut req = request(&s, &mut runtime, &export);
    let mut assignment = s.f.work.assignment.clone();
    assignment.bundle = mesh_store::RecordDigest::from_bytes([91; 32]);
    assert!(prepare(
        &mut req,
        RemoteInputDeliveryIntent::Claim {
            assignment,
            request: "claim".into()
        },
        &mut |_| panic!("mismatched input must not sign")
    )
    .is_err());
    assert!(prepare(
        &mut req,
        RemoteInputDeliveryIntent::Reconnect,
        &mut |_| panic!("unclaimed work cannot reconnect")
    )
    .is_err());
    assert!(runtime.state().lanes["lane"]
        .runs
        .last()
        .unwrap()
        .remote
        .is_none());
}
#[test]
fn invalid_worker_proof_never_claims_or_signs_receiving_admission() {
    let export = Export::new();
    let s = export.setup();
    let mut runtime = s.f.runtime(false);
    let mut req = request(&s, &mut runtime, &export);
    let prepared = prepare(&mut req, claim(&s), &mut |p| sign(&s.f.coordinator, p)).unwrap();
    let (client, mut server) = pair();
    std::thread::scope(|scope| {
        let setup = &s;
        let peer = scope.spawn(move || {
            let Some(RemoteFrame::Control(bytes)) =
                RemoteFrameReader::new(&mut server).read_frame().unwrap()
            else {
                panic!("dispatch expected");
            };
            let verified = RemoteDispatch::decode(std::str::from_utf8(&bytes).unwrap())
                .unwrap()
                .verify(&policy(setup))
                .unwrap();
            let RemoteFrame::Control(reply) =
                verified.worker_reply(|p| sign(&setup.f.worker, p)).unwrap()
            else {
                panic!("worker proof expected");
            };
            let crate::ipc::Json::Object(mut fields) =
                crate::ipc::Json::parse(std::str::from_utf8(&reply).unwrap()).unwrap()
            else {
                panic!("canonical proof expected");
            };
            fields
                .iter_mut()
                .find(|(name, _)| name == "signature")
                .unwrap()
                .1 = crate::ipc::Json::text("00".repeat(64));
            RemoteFrameWriter::new(&mut server)
                .write_frame(&RemoteFrame::Control(
                    crate::ipc::Json::Object(fields).encode().into_bytes(),
                ))
                .unwrap();
        });
        assert!(deliver(
            req,
            prepared,
            client.try_clone().unwrap(),
            client,
            |_| panic!("unverified peer cannot request admission signing")
        )
        .is_err());
        peer.join().unwrap();
    });
    assert!(runtime.state().lanes["lane"]
        .runs
        .last()
        .unwrap()
        .remote
        .is_none());
}
