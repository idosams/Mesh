use super::*;
use crate::fleet::remote_admission::authentication::tests::Fixture;
use crate::fleet::RemoteWorkerStatusRequest;
use ed25519_dalek::{Signer as _, SigningKey};
fn public(k: &SigningKey) -> PublicKey {
    PublicKey::from_bytes(k.verifying_key().to_bytes())
}
fn sign(k: &SigningKey, p: &SigningPayload) -> Result<Signature, String> {
    Ok(Signature::from_bytes(k.sign(p.as_bytes()).to_bytes()))
}
fn policy(f: &Fixture) -> RemoteDispatchPolicy<'_> {
    RemoteDispatchPolicy {
        coordinator: public(&f.coordinator),
        worker: public(&f.worker),
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
fn challenge(f: &Fixture, r: &mut Runtime, checkpoint: &str) -> RemoteResultTransferChallenge {
    RemoteResultTransferChallenge::issue(
        RemoteWorkerStatusRequest {
            runtime: r,
            lane: "lane",
            run: "run",
            coordinator: public(&f.coordinator),
            worker: public(&f.worker),
        },
        checkpoint,
    )
    .unwrap()
}
fn encoded(frame: RemoteFrame) -> String {
    let RemoteFrame::Control(v) = frame else {
        panic!("control");
    };
    String::from_utf8(v).unwrap()
}
fn request(
    f: &Fixture,
    r: &mut Runtime,
    c: &RemoteResultTransferChallenge,
) -> VerifiedRemoteResultTransferQuery {
    RemoteResultTransferQuery::decode(
        &c.signed_query(r, |p| sign(&f.coordinator, p))
            .unwrap()
            .encode(),
    )
    .unwrap()
    .verify(&policy(f))
    .unwrap()
}
#[test]
fn result_transfer_query_null_is_read_only_and_old_reply_cannot_answer_a_new_challenge() {
    let f = Fixture::new();
    let mut r = f.runtime(true);
    let registry = f.registry();
    let revision = r.state().revision;
    let c = challenge(&f, &mut r, "checkpoint");
    let q = request(&f, &mut r, &c);
    let reply = encoded(q.reply(&registry, |p| sign(&f.worker, p)).unwrap());
    assert!(c.verify_reply(&mut r, &reply).unwrap().is_none());
    let next = challenge(&f, &mut r, "checkpoint");
    assert!(next.verify_reply(&mut r, &reply).is_err());
    assert_eq!(r.state().revision, revision);
    assert!(registry.receipts().unwrap().is_empty());
}
#[test]
fn result_transfer_query_refuses_wrong_keys_domains_expired_requests_and_noncanonical_fields() {
    let f = Fixture::new();
    let mut r = f.runtime(true);
    let registry = f.registry();
    let c = challenge(&f, &mut r, "checkpoint");
    assert!(c.signed_query(&mut r, |p| sign(&f.worker, p)).is_err());
    let original = c.signed_query(&mut r, |p| sign(&f.coordinator, p)).unwrap();
    let wrong = RemoteResultTransferQuery {
        body: original.body.clone(),
        signature: sign(&f.coordinator, &payload(QUERY_DOMAIN, &original.body)).unwrap(),
    };
    assert!(RemoteResultTransferQuery::decode(&wrong.encode())
        .unwrap()
        .verify(&policy(&f))
        .is_err());
    let mut body = original.body.clone();
    let Json::Object(fields) = &mut body else {
        panic!("object");
    };
    let Json::Object(query) = &mut fields.iter_mut().find(|(k, _)| k == "query").unwrap().1 else {
        panic!("query");
    };
    query.iter_mut().find(|(k, _)| k == "issued_ms").unwrap().1 = Json::Number(1);
    query.iter_mut().find(|(k, _)| k == "expires_ms").unwrap().1 = Json::Number(30_001);
    let signature = sign(&f.coordinator, &payload(QUERY_SIGNING, &body)).unwrap();
    assert!(
        RemoteResultTransferQuery::decode(&envelope(QUERY_SCHEMA, body, &signature))
            .unwrap()
            .verify(&policy(&f))
            .is_err()
    );
    assert!(RemoteResultTransferQuery::decode(&(original.encode() + " ")).is_err());
    assert!(RemoteResultTransferQuery::decode(&"x".repeat(MAX + 1)).is_err());
    let q = request(&f, &mut r, &c);
    assert!(q.reply(&registry, |p| sign(&f.coordinator, p)).is_err());
    let reply = encoded(q.reply(&registry, |p| sign(&f.worker, p)).unwrap());
    let Json::Object(mut fields) = Json::parse(&reply).unwrap() else {
        panic!("reply");
    };
    fields.push(("unknown".into(), Json::Null));
    assert!(c
        .verify_reply(&mut r, &Json::Object(fields).encode())
        .is_err());
}

#[test]
fn authenticated_result_stream_resumes_partial_content_and_refuses_wrong_frames() {
    use crate::fleet::{receive_remote_saved_result, NativeRemoteResultReceiver};
    use std::os::unix::net::UnixStream;
    let setup = crate::fleet::receiving_session::tests::Setup::new();
    let mut runtime = setup.f.runtime(true);
    let revision = runtime.state().revision;
    // Signed protocol fixture; exact native worker history reopening is covered by received-host tests.
    let data = vec![0xff; 140_000];
    let digest = mesh_cas::Digest32::from_bytes(*Blake3::digest_bytes(&data).as_bytes());
    let manifest = RemoteInputManifest::new(
        RecordDigest::from_bytes([0x95; 32]),
        vec![crate::fleet::RemoteInputEntry::File {
            path: "result.bin".into(),
            executable: false,
            digest,
            chunks: vec![crate::fleet::RemoteInputChunk {
                digest,
                bytes: data.len() as u64,
            }],
        }],
    )
    .unwrap();
    let mut encoded_offer = None;
    for attempt in 0..3 {
        let (client, server) = UnixStream::pair().unwrap();
        for stream in [&client, &server] {
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(5)))
                .unwrap();
        }
        let outcome = std::thread::scope(|scope| {
            let worker = scope.spawn(|| {
                let mut reader = RemoteFrameReader::new(&server);
                let mut writer = RemoteFrameWriter::new(&server);
                let Some(RemoteFrame::Control(raw)) = reader.read_frame().unwrap() else {
                    panic!("query");
                };
                let query = RemoteResultTransferQuery::decode(std::str::from_utf8(&raw).unwrap())
                    .unwrap()
                    .verify(&policy(&setup.f))
                    .unwrap();
                let body = Json::object([
                    ("target", query.target().clone()),
                    ("owner", Json::text("01".repeat(32))),
                    ("mapping", Json::text("02".repeat(32))),
                    ("initial", Json::text("03".repeat(32))),
                    ("installation", Json::text("fixture")),
                    ("checkpoint", Json::text("checkpoint")),
                    ("review", Json::text("04".repeat(32))),
                    ("version", Json::text(manifest.input().to_string())),
                    ("manifest", Json::text(manifest.bundle().to_string())),
                ]);
                let offer = RemoteSavedResultOffer::sign(body, |p| sign(&setup.f.worker, p))
                    .unwrap()
                    .encode();
                let reply = Json::object([
                    (
                        "query",
                        Json::text(
                            Blake3::digest_bytes(query.query.body.encode().as_bytes()).to_string(),
                        ),
                    ),
                    ("observed_ms", Json::Number(now().unwrap())),
                    ("offer", Json::text(&offer)),
                ]);
                let signature = sign(&setup.f.worker, &payload(REPLY_SIGNING, &reply)).unwrap();
                writer
                    .write_frame(&RemoteFrame::Control(
                        envelope(REPLY_SCHEMA, reply, &signature).into_bytes(),
                    ))
                    .unwrap();
                writer
                    .write_frame(&RemoteFrame::Manifest(
                        manifest.encoded().as_bytes().to_vec(),
                    ))
                    .unwrap();
                let Some(RemoteFrame::Control(raw)) = reader.read_frame().unwrap() else {
                    panic!("request");
                };
                let request = Json::parse(std::str::from_utf8(&raw).unwrap()).unwrap();
                let digest = mesh_cas::Digest32::from_bytes(
                    bytes::<32>(text(&request, "digest").unwrap()).unwrap(),
                );
                let offset = number(&request, "offset").unwrap();
                assert_eq!(offset, if attempt == 0 { 0 } else { 65_536 });

                if attempt == 1 {
                    writer
                        .write_frame(&RemoteFrame::Chunk {
                            digest,
                            offset: 0,
                            final_part: false,
                            bytes: vec![0; 10],
                        })
                        .unwrap();
                } else {
                    let mut position = offset as usize;
                    while position < data.len() {
                        let next = (position + 65_536).min(data.len());
                        writer
                            .write_frame(&RemoteFrame::Chunk {
                                digest,
                                offset: position as u64,
                                final_part: next == data.len(),
                                bytes: data[position..next].to_vec(),
                            })
                            .unwrap();
                        position = next;
                        if attempt == 0 {
                            break;
                        }
                    }
                    if attempt == 2 {
                        let Some(RemoteFrame::Control(raw)) = reader.read_frame().unwrap() else {
                            panic!("end");
                        };
                        assert_eq!(raw, end().encode().as_bytes());
                    }
                }
                // The stream closes here, including the interrupted first transfer.
                server.shutdown(std::net::Shutdown::Both).unwrap();
                offer
            });
            let result = receive_remote_saved_result(
                &setup.destination,
                RemoteWorkerStatusRequest {
                    runtime: &mut runtime,
                    lane: "lane",
                    run: "run",
                    coordinator: public(&setup.f.coordinator),
                    worker: public(&setup.f.worker),
                },
                "checkpoint",
                &client,
                &client,
                |p| sign(&setup.f.coordinator, p),
            );
            let offer = worker.join().unwrap();
            if let Some(expected) = &encoded_offer {
                assert_eq!(expected, &offer);
            }
            encoded_offer = Some(offer);
            result
        });
        if attempt < 2 {
            assert!(outcome.is_err());
        } else {
            assert_eq!(outcome.unwrap().encode(), *encoded_offer.as_ref().unwrap());
        }
    }
    let receiver = NativeRemoteResultReceiver::new(
        &setup.destination,
        manifest.clone(),
        encoded_offer.as_ref().unwrap(),
        RemoteWorkerStatusRequest {
            runtime: &mut runtime,
            lane: "lane",
            run: "run",
            coordinator: public(&setup.f.coordinator),
            worker: public(&setup.f.worker),
        },
    )
    .unwrap();
    receiver.verify_complete(&mut runtime).unwrap();
    drop(receiver);
    let (_, receipt) = NativeRemoteResultReceiver::reopen_content_receipt(
        &setup.destination,
        encoded_offer.as_ref().unwrap(),
        RemoteWorkerStatusRequest {
            runtime: &mut runtime,
            lane: "lane",
            run: "run",
            coordinator: public(&setup.f.coordinator),
            worker: public(&setup.f.worker),
        },
    )
    .unwrap();
    assert_ne!(receipt.digest(), RecordDigest::from_bytes([0; 32]));
    assert_eq!(runtime.state().revision, revision);
    assert!(setup.f.registry().receipts().unwrap().is_empty());
}
