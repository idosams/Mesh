use super::*;
use crate::fleet::receiving_session::tests::Setup;
use crate::fleet::{serve_remote_receiving, RemoteReceivedHandoff, RemoteReceivingBrokerOutcome};
use crate::project_attachment::{AttachmentStorage, ObservationLimits};
use ed25519_dalek::{Signer as _, SigningKey};
use std::fs;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static NEXT: AtomicU64 = AtomicU64::new(0);
pub(in crate::fleet) struct Export {
    root: PathBuf,
    pub(in crate::fleet) source: RemoteInputSource,
    live_bytes: Vec<u8>,
}
impl Export {
    pub(in crate::fleet) fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "mesh-transfer-client-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::create_dir_all(root.join("project/empty")).unwrap();
        fs::write(root.join("project/binary"), vec![0xff; 140_000]).unwrap();
        fs::write(root.join("project/zero"), []).unwrap();
        fs::create_dir(root.join("metadata")).unwrap();
        let storage = AttachmentStorage::open(&root.join("metadata")).unwrap();
        let attached = storage.provision(&root.join("project")).unwrap();
        let capture = attached
            .project()
            .capture_inputs(ObservationLimits::default())
            .unwrap();
        let key = SigningKey::from_bytes(&[74; 32]);
        let saved = attached
            .project()
            .save_capture(
                attached.metadata_path(),
                &capture,
                PublicKey::from_bytes(key.verifying_key().to_bytes()),
                |body| Ok::<_, String>(Signature::from_bytes(key.sign(body.as_bytes()).to_bytes())),
            )
            .unwrap();
        let source = attached
            .prepare_remote_input(&saved.operation().to_string())
            .unwrap();
        Self {
            root,
            source,
            live_bytes: vec![0xff; 140_000],
        }
    }
    pub(in crate::fleet) fn setup(&self) -> Setup {
        let mut setup = Setup::new();
        setup.f.work.assignment.input = self.source.manifest().input();
        setup.f.work.assignment.bundle = self.source.manifest().bundle();
        setup.manifest = self.source.manifest().clone();
        setup
    }
    fn verify(&self, handoff: &RemoteReceivedHandoff) {
        assert_eq!(
            fs::read(handoff.allocation.path().join("binary")).unwrap(),
            vec![0xff; 140_000]
        );
        assert_eq!(
            fs::read(handoff.allocation.path().join("zero")).unwrap(),
            Vec::<u8>::new()
        );
        assert!(handoff.allocation.path().join("empty").is_dir());
        assert_eq!(
            fs::read(self.root.join("project/binary")).unwrap(),
            self.live_bytes
        );
        assert_eq!(handoff.registry.receipts().unwrap().len(), 1);
    }
}
impl Drop for Export {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
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
fn request<'a>(
    setup: &Setup,
    runtime: &'a mut Runtime,
    export: &'a Export,
) -> RemoteInputTransferRequest<'a> {
    RemoteInputTransferRequest {
        runtime,
        lane: "lane",
        run: "run",
        source: &export.source,
        coordinator: PublicKey::from_bytes(setup.f.coordinator.verifying_key().to_bytes()),
        worker: PublicKey::from_bytes(setup.f.worker.verifying_key().to_bytes()),
    }
}
fn sign(setup: &Setup, payload: &SigningPayload) -> Result<Signature, String> {
    Ok(Signature::from_bytes(
        setup.f.coordinator.sign(payload.as_bytes()).to_bytes(),
    ))
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
                return Err(io::ErrorKind::BrokenPipe.into());
            }
        }
        if self.chunks == 1 && bytes.len() == 41 {
            assert_eq!(u64::from_be_bytes(bytes[32..40].try_into().unwrap()), 7);
        }
        self.stream.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

#[test]
fn native_saved_input_resumes_after_stream_failure_and_materializes_exactly_once() {
    let mut export = Export::new();
    export.live_bytes = b"User continues editing the existing project".to_vec();
    fs::write(export.root.join("project/binary"), &export.live_bytes).unwrap();
    let setup = export.setup();
    let mut runtime = setup.f.runtime(true);
    let mut session = setup.session();
    let mut chunks = std::collections::BTreeMap::new();
    for entry in export.source.manifest().entries() {
        if let RemoteInputEntry::File { chunks: parts, .. } = entry {
            for part in parts {
                chunks.insert(part.digest, part.bytes);
            }
        }
    }
    let (&first_digest, &first_length) = chunks.first_key_value().unwrap();
    assert!(first_length > 7 && chunks.len() > 1);
    // Native saved-history chunks are at most 32 KiB here. Seed a genuine partial receipt
    // from an earlier connection to exercise a nonzero resume offset; do not assume a
    // 140 KiB file is one chunk. The following actual stream interruption then leaves
    // this first chunk complete and a second distinct chunk still missing.
    {
        let mut connection = session.connect().unwrap();
        let payload = connection
            .proof()
            .unwrap()
            .signing_payload_for(
                &mut runtime,
                "lane",
                "run",
                &PublicKey::from_bytes(setup.f.coordinator.verifying_key().to_bytes()),
                &PublicKey::from_bytes(setup.f.worker.verifying_key().to_bytes()),
            )
            .unwrap();
        connection
            .authenticate(&sign(&setup, &payload).unwrap())
            .unwrap();
        connection.receive(setup.manifest_frame()).unwrap();
        let bytes = export.source.read_chunk(first_digest).unwrap();
        connection
            .receive(RemoteFrame::Chunk {
                digest: first_digest,
                offset: 0,
                final_part: false,
                bytes: bytes[..7].to_vec(),
            })
            .unwrap();
        assert_eq!(connection.status(first_digest).unwrap(), (7, false));
    }
    let (client1, server1) = pair();
    let (client2, server2) = pair();
    std::thread::scope(|scope| {
        let server = scope.spawn(|| {
            assert!(matches!(
                serve_remote_receiving(&mut session, server1.try_clone().unwrap(), server1)
                    .unwrap(),
                RemoteReceivingBrokerOutcome::Disconnected
            ));
            let complete =
                mesh_cas::StoreLayout::new(setup.f.path.join("store")).chunk_path(&first_digest);
            assert_eq!(fs::metadata(complete).unwrap().len(), first_length);
            let RemoteReceivingBrokerOutcome::Materialized {
                handoff,
                reply_written,
            } = serve_remote_receiving(&mut session, server2.try_clone().unwrap(), server2)
                .unwrap()
            else {
                panic!("handoff expected")
            };
            assert!(reply_written);
            export.verify(&handoff);
            assert!(session.connect().is_err());
        });
        assert!(transfer_remote_input(
            request(&setup, &mut runtime, &export),
            client1.try_clone().unwrap(),
            FailSecondChunk {
                stream: client1,
                chunks: 0
            },
            |body| sign(&setup, body)
        )
        .is_err());
        let RemoteInputTransferOutcome::Materialized(receipt) = transfer_remote_input(
            request(&setup, &mut runtime, &export),
            client2.try_clone().unwrap(),
            client2,
            |body| sign(&setup, body),
        )
        .unwrap() else {
            panic!("input acknowledgment expected")
        };
        assert_eq!(
            receipt
                .correlation()
                .get("assignment")
                .and_then(Json::as_text),
            Some("assignment")
        );
        server.join().unwrap();
    });
    assert_eq!(runtime.state().lanes["lane"].runs.len(), 1);
    assert_eq!(
        runtime.state().lanes["lane"].runs[0].state,
        super::super::RunState::Launching
    );
}

struct ChangeReply {
    stream: UnixStream,
    lose_final: bool,
}
impl Write for ChangeReply {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let text = std::str::from_utf8(bytes).unwrap_or("");
        if self.lose_final && text.contains("\"kind\":\"materialized\"") {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        if !self.lose_final && text.contains("\"kind\":\"authenticated\"") {
            // Same length, valid JSON and correct framing; only request correlation is wrong.
            let changed = text.replace(
                "\"request\":\"authenticate\"",
                "\"request\":\"authenticatX\"",
            );
            assert_ne!(changed.as_bytes(), bytes);
            self.stream.write_all(changed.as_bytes())?;
            return Ok(bytes.len());
        }
        self.stream.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}
#[test]
fn lost_final_reply_is_uncertain_to_client_but_worker_retains_one_native_handoff() {
    let export = Export::new();
    let setup = export.setup();
    let mut runtime = setup.f.runtime(true);
    let mut session = setup.session();
    let (client, server) = pair();
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            let output = ChangeReply {
                stream: server.try_clone().unwrap(),
                lose_final: true,
            };
            let RemoteReceivingBrokerOutcome::Materialized {
                handoff,
                reply_written,
            } = serve_remote_receiving(&mut session, server, output).unwrap()
            else {
                panic!("native handoff expected")
            };
            assert!(!reply_written);
            export.verify(&handoff);
        });
        assert!(transfer_remote_input(
            request(&setup, &mut runtime, &export),
            client.try_clone().unwrap(),
            client,
            |body| sign(&setup, body)
        )
        .is_err());
        worker.join().unwrap();
    });
    assert_eq!(
        runtime.state().lanes["lane"].runs[0].state,
        super::super::RunState::Launching
    );
}
#[test]
fn wrong_reply_correlation_refuses_before_manifest_or_file_export() {
    let export = Export::new();
    let setup = export.setup();
    let mut runtime = setup.f.runtime(true);
    let mut session = setup.session();
    let (client, server) = pair();
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            let output = ChangeReply {
                stream: server.try_clone().unwrap(),
                lose_final: false,
            };
            assert!(matches!(
                serve_remote_receiving(&mut session, server, output).unwrap(),
                RemoteReceivingBrokerOutcome::Disconnected
            ));
        });
        assert!(transfer_remote_input(
            request(&setup, &mut runtime, &export),
            client.try_clone().unwrap(),
            client,
            |body| sign(&setup, body)
        )
        .is_err());
        worker.join().unwrap();
    });
    setup.assert_empty_store();
}
#[test]
fn changed_native_context_during_signing_refuses_before_sending_authentication() {
    let export = Export::new();
    let setup = export.setup();
    let mut runtime = setup.f.runtime(true);
    let mut session = setup.session();
    let (client, server) = pair();
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            assert!(matches!(
                serve_remote_receiving(&mut session, server.try_clone().unwrap(), server).unwrap(),
                RemoteReceivingBrokerOutcome::Disconnected
            ))
        });
        assert!(transfer_remote_input(
            request(&setup, &mut runtime, &export),
            client.try_clone().unwrap(),
            client,
            |body| {
                let mut other = Runtime::open(
                    mesh_store::fleet::FleetStore::open(setup.f.path.join("coordinator.sqlite"))
                        .unwrap(),
                    "objective",
                )
                .unwrap();
                other
                    .record("cancel-during-sign", super::super::Command::Cancel)
                    .unwrap();
                sign(&setup, body)
            }
        )
        .is_err());
        worker.join().unwrap();
    });
    assert!(setup.f.registry().receipts().unwrap().is_empty());
    setup.assert_empty_store();
}
#[test]
fn reply_verification_rejects_noncanonical_unknown_and_cross_attempt_facts() {
    let receipt = RemoteInputTransferReceipt(Json::object([
        ("assignment", Json::text("expected")),
        ("revision", Json::Number(2)),
    ]));
    let bytes = Json::object([
        ("schema", Json::text("mesh.receiving-reply/v1")),
        ("kind", Json::text("manifest")),
        ("request", Json::Null),
        ("admission", receipt.0.clone()),
        ("detail", Json::Null),
    ])
    .encode();
    verify_reply(bytes.as_bytes(), &receipt, "manifest", None, Json::Null).unwrap();
    for changed in [
        format!(" {bytes}"),
        bytes.replace("/v1", "/v2"),
        bytes.replace("expected", "different"),
        bytes.replace(":2", ":3"),
        bytes.replacen('{', "{\"unknown\":0,", 1),
        bytes.replace("\"request\":null", "\"request\":\"other\""),
    ] {
        assert!(verify_reply(changed.as_bytes(), &receipt, "manifest", None, Json::Null).is_err());
    }
}

#[test]
fn saved_source_must_match_native_assignment_before_signing_or_export() {
    let export = Export::new();
    let setup = Setup::new(); // Deliberately assigns another immutable source.
    let mut runtime = setup.f.runtime(true);
    let mut session = setup.session();
    let (client, server) = pair();
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            assert!(matches!(
                serve_remote_receiving(&mut session, server.try_clone().unwrap(), server).unwrap(),
                RemoteReceivingBrokerOutcome::Disconnected
            ))
        });
        assert!(transfer_remote_input(
            request(&setup, &mut runtime, &export),
            client.try_clone().unwrap(),
            client,
            |_| panic!("mismatched source must not reach signer")
        )
        .is_err());
        worker.join().unwrap();
    });
    assert!(setup.f.registry().receipts().unwrap().is_empty());
    setup.assert_empty_store();
}

#[test]
fn receipt_only_peer_returns_retained_without_sending_manifest_or_allocating_again() {
    let export = Export::new();
    let setup = export.setup();
    let mut runtime = setup.f.runtime(true);
    // Lose the original reservation, as after a supervisor restart; only saved facts remain.
    drop(
        setup
            .f
            .registry()
            .reserve(
                setup.f.work.clone(),
                "0123456789abcdef0123456789abcdef",
                crate::fleet::service::received_clock().unwrap(),
            )
            .unwrap(),
    );
    let mut session = setup.session();
    let (client, server) = pair();
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            assert!(matches!(
                serve_remote_receiving(&mut session, server.try_clone().unwrap(), server).unwrap(),
                RemoteReceivingBrokerOutcome::Disconnected
            ))
        });
        assert!(matches!(
            transfer_remote_input(
                request(&setup, &mut runtime, &export),
                client.try_clone().unwrap(),
                client,
                |body| sign(&setup, body)
            )
            .unwrap(),
            RemoteInputTransferOutcome::Retained(_)
        ));
        worker.join().unwrap();
    });
    assert_eq!(setup.f.registry().receipts().unwrap().len(), 1);
    setup.assert_empty_store();
}

#[test]
fn coordinator_signed_bootstrap_then_saved_input_transfer_share_one_authenticated_stream() {
    use crate::fleet::{
        RemoteDispatch, RemoteDispatchPolicy, RemotePeerChallenge, RemoteReceivingSession,
    };
    let export = Export::new();
    let setup = export.setup();
    let mut runtime = setup.f.runtime(false);
    let challenge = RemotePeerChallenge::issue(
        &mut runtime,
        "lane",
        "run",
        setup.f.work.assignment.clone(),
        PublicKey::from_bytes(setup.f.worker.verifying_key().to_bytes()),
    )
    .unwrap();
    let dispatch = challenge
        .signed_dispatch(
            &mut runtime,
            &PublicKey::from_bytes(setup.f.coordinator.verifying_key().to_bytes()),
            |body| sign(&setup, body),
        )
        .unwrap();
    let maximum = runtime.state().limits.clone().unwrap();
    let (mut client, mut server) = pair();
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            let Some(RemoteFrame::Control(bytes)) =
                RemoteFrameReader::new(&mut server).read_frame().unwrap()
            else {
                panic!("dispatch expected")
            };
            let dispatch = RemoteDispatch::decode(std::str::from_utf8(&bytes).unwrap()).unwrap();
            let verified = dispatch
                .verify(&RemoteDispatchPolicy {
                    coordinator: PublicKey::from_bytes(
                        setup.f.coordinator.verifying_key().to_bytes(),
                    ),
                    worker: PublicKey::from_bytes(setup.f.worker.verifying_key().to_bytes()),
                    provider: "codex",
                    maximum,
                    max_lease_ms: 120_000,
                })
                .unwrap();
            assert_eq!(verified.objective(), "objective");
            assert!(setup.f.registry().receipts().unwrap().is_empty());
            let reply = verified
                .worker_reply(|body| {
                    Ok(Signature::from_bytes(
                        setup.f.worker.sign(body.as_bytes()).to_bytes(),
                    ))
                })
                .unwrap();
            RemoteFrameWriter::new(&mut server)
                .write_frame(&reply)
                .unwrap();
            // Authenticated dispatch supplies work facts, but the subsequent fresh coordinator
            // proof and original durable receiving reservation are still mandatory.
            let mut session = RemoteReceivingSession::new(
                setup.f.registry(),
                verified.work().clone(),
                "0123456789abcdef0123456789abcdef",
                &setup.destination,
            );
            let RemoteReceivingBrokerOutcome::Materialized {
                handoff,
                reply_written,
            } = serve_remote_receiving(&mut session, server.try_clone().unwrap(), server).unwrap()
            else {
                panic!("native handoff expected")
            };
            assert!(reply_written);
            export.verify(&handoff);
        });
        RemoteFrameWriter::new(&mut client)
            .write_frame(&dispatch.frame().unwrap())
            .unwrap();
        let Some(RemoteFrame::Control(bytes)) =
            RemoteFrameReader::new(&mut client).read_frame().unwrap()
        else {
            panic!("worker proof expected")
        };
        challenge
            .verify_dispatch_reply(
                &mut runtime,
                "bootstrap-worker",
                std::str::from_utf8(&bytes).unwrap(),
            )
            .unwrap();
        assert!(matches!(
            transfer_remote_input(
                request(&setup, &mut runtime, &export),
                client.try_clone().unwrap(),
                client,
                |body| sign(&setup, body)
            )
            .unwrap(),
            RemoteInputTransferOutcome::Materialized(_)
        ));
        worker.join().unwrap();
    });
    assert_eq!(setup.f.registry().receipts().unwrap().len(), 1);
    assert_eq!(runtime.state().lanes["lane"].runs.len(), 1);
}
