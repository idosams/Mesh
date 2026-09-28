use super::*;
use crate::fleet::receiving_session::tests::Setup;
use crate::fleet::{RemoteAdmissionProof, Runtime};
use ed25519_dalek::Signer as _;
use mesh_types::{PublicKey, Signature};
use std::os::unix::net::UnixStream;
use std::time::Duration;

fn pair() -> (UnixStream, UnixStream) {
    let (left, right) = UnixStream::pair().unwrap();
    for stream in [&left, &right] {
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
    }
    (left, right)
}
fn send(stream: &mut UnixStream, frame: RemoteFrame) {
    RemoteFrameWriter::new(stream).write_frame(&frame).unwrap();
}
fn read(stream: &mut UnixStream) -> RemoteFrame {
    RemoteFrameReader::new(stream)
        .read_frame()
        .unwrap()
        .unwrap()
}
fn response(stream: &mut UnixStream, kind: &str) -> Json {
    let RemoteFrame::Control(bytes) = read(stream) else {
        panic!("control reply expected")
    };
    let value = Json::parse(std::str::from_utf8(&bytes).unwrap()).unwrap();
    assert_eq!(
        value.get("schema").and_then(Json::as_text),
        Some("mesh.receiving-reply/v1")
    );
    assert_eq!(value.get("kind").and_then(Json::as_text), Some(kind));
    value
}
fn authenticate(s: &Setup, runtime: &mut Runtime, stream: &mut UnixStream) {
    let RemoteFrame::Control(bytes) = read(stream) else {
        panic!("proof expected")
    };
    let proof = RemoteAdmissionProof::decode(std::str::from_utf8(&bytes).unwrap()).unwrap();
    let payload = proof
        .signing_payload_for(
            runtime,
            "lane",
            "run",
            &PublicKey::from_bytes(s.f.coordinator.verifying_key().to_bytes()),
            &PublicKey::from_bytes(s.f.worker.verifying_key().to_bytes()),
        )
        .unwrap();
    let signature = Signature::from_bytes(s.f.coordinator.sign(payload.as_bytes()).to_bytes());
    send(
        stream,
        RemoteReceivingCommand::Authenticate {
            request: "authenticate".into(),
            signature,
        }
        .frame()
        .unwrap(),
    );
    let reply = response(stream, "authenticated");
    assert_eq!(
        reply.get("request").and_then(Json::as_text),
        Some("authenticate")
    );
    let admission = reply.get("admission").unwrap();
    assert_eq!(
        admission.get("assignment").and_then(Json::as_text),
        Some("assignment")
    );
    assert_eq!(
        admission.get("input").and_then(Json::as_text),
        Some(s.manifest.input().to_string().as_str())
    );
    assert_eq!(
        admission.get("bundle").and_then(Json::as_text),
        Some(s.manifest.bundle().to_string().as_str())
    );
}
fn finish(stream: &mut UnixStream) {
    send(
        stream,
        RemoteReceivingCommand::Materialize {
            request: "finish".into(),
        }
        .frame()
        .unwrap(),
    );
}

#[test]
fn real_broker_stream_disconnect_resumes_authenticated_offsets_and_hands_off_once() {
    let s = Setup::new();
    let mut runtime = s.f.runtime(true);
    let mut session = s.session();
    let (mut first, server_first) = pair();
    let (mut second, server_second) = pair();
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            assert!(matches!(
                serve_remote_receiving(
                    &mut session,
                    server_first.try_clone().unwrap(),
                    server_first
                )
                .unwrap(),
                RemoteReceivingBrokerOutcome::Disconnected
            ));
            let outcome = serve_remote_receiving(
                &mut session,
                server_second.try_clone().unwrap(),
                server_second,
            )
            .unwrap();
            let RemoteReceivingBrokerOutcome::Materialized {
                handoff,
                reply_written,
            } = outcome
            else {
                panic!("handoff expected")
            };
            assert!(reply_written);
            assert_eq!(
                std::fs::read(handoff.allocation.path().join("result.txt")).unwrap(),
                s.bytes
            );
            assert_eq!(handoff.registry.receipts().unwrap().len(), 1);
            assert!(session.connect().is_err());
        });
        authenticate(&s, &mut runtime, &mut first);
        send(&mut first, s.manifest_frame());
        response(&mut first, "manifest");
        send(&mut first, s.part(0, 7));
        response(&mut first, "chunk");
        drop(first);
        authenticate(&s, &mut runtime, &mut second);
        send(
            &mut second,
            RemoteReceivingCommand::Status {
                request: "offset".into(),
                digest: s.digest,
            }
            .frame()
            .unwrap(),
        );
        let status = response(&mut second, "chunk");
        assert_eq!(
            status
                .get("detail")
                .unwrap()
                .get("offset")
                .and_then(Json::as_u64),
            Some(7)
        );
        send(&mut second, s.part(7, s.bytes.len()));
        response(&mut second, "chunk");
        finish(&mut second);
        let ready = response(&mut second, "materialized");
        assert_eq!(ready.get("request").and_then(Json::as_text), Some("finish"));
        worker.join().unwrap();
    });
}

struct LoseFinalReply {
    stream: UnixStream,
    replies: usize,
}
impl Write for LoseFinalReply {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.replies == 4 {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        self.stream.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()?;
        self.replies += 1;
        Ok(())
    }
}
#[test]
fn lost_final_reply_returns_native_handoff_instead_of_dropping_it_or_regranting() {
    let s = Setup::new();
    let mut runtime = s.f.runtime(true);
    let mut session = s.session();
    let (mut client, server) = pair();
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            let output = LoseFinalReply {
                stream: server.try_clone().unwrap(),
                replies: 0,
            };
            let outcome = serve_remote_receiving(&mut session, server, output).unwrap();
            let RemoteReceivingBrokerOutcome::Materialized {
                handoff,
                reply_written,
            } = outcome
            else {
                panic!("retained handoff expected")
            };
            assert!(!reply_written);
            handoff.allocation.verify().unwrap();
            assert_eq!(
                std::fs::read(handoff.allocation.path().join("result.txt")).unwrap(),
                s.bytes
            );
            assert_eq!(handoff.registry.receipts().unwrap().len(), 1);
            assert!(session.connect().is_err());
        });
        authenticate(&s, &mut runtime, &mut client);
        send(&mut client, s.manifest_frame());
        response(&mut client, "manifest");
        send(&mut client, s.part(0, s.bytes.len()));
        response(&mut client, "chunk");
        finish(&mut client);
        assert!(RemoteFrameReader::new(&mut client)
            .read_frame()
            .unwrap()
            .is_none());
        worker.join().unwrap();
    });
}

#[test]
fn unauthenticated_control_or_data_cannot_create_admission_or_storage() {
    for frame in [
        RemoteReceivingCommand::Materialize {
            request: "premature".into(),
        }
        .frame()
        .unwrap(),
        RemoteFrame::Manifest(b"{}".to_vec()),
    ] {
        let s = Setup::new();
        let mut session = s.session();
        let mut wire = Vec::new();
        RemoteFrameWriter::new(&mut wire)
            .write_frame(&frame)
            .unwrap();
        assert!(serve_remote_receiving(&mut session, wire.as_slice(), Vec::new()).is_err());
        assert!(s.f.registry().receipts().unwrap().is_empty());
        s.assert_empty_store();
        // The abandoned challenge did not destroy the supervisor's registry.
        assert!(session.connect().is_ok());
    }
}

#[test]
fn repeated_request_id_refuses_without_losing_original_transfer() {
    let s = Setup::new();
    let mut runtime = s.f.runtime(true);
    let mut session = s.session();
    let (mut client, server) = pair();
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            assert!(
                serve_remote_receiving(&mut session, server.try_clone().unwrap(), server).is_err()
            )
        });
        authenticate(&s, &mut runtime, &mut client);
        send(&mut client, s.manifest_frame());
        response(&mut client, "manifest");
        for round in 0..2 {
            send(
                &mut client,
                RemoteReceivingCommand::Status {
                    request: "same-id".into(),
                    digest: s.digest,
                }
                .frame()
                .unwrap(),
            );
            if round == 0 {
                response(&mut client, "chunk");
            }
        }
        worker.join().unwrap();
    });
    assert_eq!(s.f.registry().receipts().unwrap().len(), 1);
    assert!(session.connect().is_ok());
}

#[test]
fn connection_frame_budget_ends_stream_without_repeating_admission() {
    let s = Setup::new();
    let mut runtime = s.f.runtime(true);
    let mut session = s.session();
    let (mut client, server) = pair();
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            let error = serve_with_limits(
                &mut session,
                server.try_clone().unwrap(),
                server,
                1,
                MAX_CONNECTION_BYTES,
            )
            .err()
            .unwrap();
            assert_eq!(error.to_string(), "remote-receiving-frame-budget");
        });
        authenticate(&s, &mut runtime, &mut client);
        worker.join().unwrap();
    });
    assert_eq!(s.f.registry().receipts().unwrap().len(), 1);
    s.assert_empty_store();
}

#[test]
fn command_codec_is_closed_canonical_bounded_and_rejects_unknown_fields_and_versions() {
    let commands = [
        RemoteReceivingCommand::Authenticate {
            request: "auth".into(),
            signature: Signature::from_bytes([0xab; 64]),
        },
        RemoteReceivingCommand::Status {
            request: "offset".into(),
            digest: mesh_cas::Digest32::from_bytes([0xab; 32]),
        },
        RemoteReceivingCommand::Materialize {
            request: "finish".into(),
        },
    ];
    for command in commands {
        let RemoteFrame::Control(bytes) = command.frame().unwrap() else {
            unreachable!()
        };
        let decoded = RemoteReceivingCommand::decode(&bytes).unwrap();
        let RemoteFrame::Control(roundtrip) = decoded.frame().unwrap() else {
            unreachable!()
        };
        assert_eq!(roundtrip, bytes);
        let text = String::from_utf8(bytes).unwrap();
        for bad in [
            format!(" {text}"),
            text.replace("/v1", "/v2"),
            text.replacen('{', "{\"extra\":0,", 1),
            text.replace("\"request\":", "\"request\":\"duplicate\",\"request\":"),
            text.replace("\"operation\":", "\"operation\":null,\"old\":"),
        ] {
            assert!(RemoteReceivingCommand::decode(bad.as_bytes()).is_err());
        }
    }
    assert!(RemoteReceivingCommand::decode(&vec![b' '; 65_537]).is_err());
    assert!(RemoteReceivingCommand::decode(&[0xff]).is_err());
    assert!(RemoteReceivingCommand::Materialize { request: "".into() }
        .frame()
        .is_err());
    let RemoteFrame::Control(bytes) = (RemoteReceivingCommand::Authenticate {
        request: "auth".into(),
        signature: Signature::from_bytes([0xab; 64]),
    })
    .frame()
    .unwrap() else {
        unreachable!()
    };
    assert!(RemoteReceivingCommand::decode(
        String::from_utf8(bytes)
            .unwrap()
            .replace("abab", "ABAB")
            .as_bytes()
    )
    .is_err());
}

#[test]
fn total_byte_budget_refuses_next_frame_before_it_can_initialize_storage() {
    let s = Setup::new();
    let mut runtime = s.f.runtime(true);
    let mut session = s.session();
    let (mut client, server) = pair();
    let auth_bytes = RemoteReceivingCommand::Authenticate {
        request: "authenticate".into(),
        signature: Signature::from_bytes([0; 64]),
    }
    .frame()
    .unwrap()
    .encoded_len()
    .unwrap();
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            let error = serve_with_limits(
                &mut session,
                server.try_clone().unwrap(),
                server,
                10,
                auth_bytes,
            )
            .err()
            .unwrap();
            assert_eq!(error.to_string(), "remote-receiving-byte-budget");
        });
        authenticate(&s, &mut runtime, &mut client);
        send(&mut client, s.manifest_frame());
        worker.join().unwrap();
    });
    assert_eq!(s.f.registry().receipts().unwrap().len(), 1);
    s.assert_empty_store();
    assert!(session.connect().is_ok());
}
