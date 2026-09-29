use super::*;
use crate::fleet::{receiving_session::tests::Setup, RemoteResultCorrespondence};
use crate::workspace::{HistoricalWorkspacePreview, HistoricalWorkspacePreviewFile};
use ed25519_dalek::{Signer as _, SigningKey};
use std::os::unix::net::UnixStream;
fn public(key: &SigningKey) -> PublicKey {
    PublicKey::from_bytes(key.verifying_key().to_bytes())
}
fn sign(key: &SigningKey, value: &SigningPayload) -> Result<Signature, String> {
    Ok(Signature::from_bytes(key.sign(value.as_bytes()).to_bytes()))
}
fn policy(setup: &Setup) -> RemoteDispatchPolicy<'_> {
    RemoteDispatchPolicy {
        coordinator: public(&setup.f.coordinator),
        worker: public(&setup.f.worker),
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
fn initial(setup: &Setup) -> HistoricalWorkspacePreview {
    HistoricalWorkspacePreview {
        operation: RecordDigest::from_bytes([0x92; 32]),
        directories: vec![],
        files: vec![HistoricalWorkspacePreviewFile {
            object: mesh_materializer::ObjectId::from_bytes([1; 16]),
            path: "result.txt".into(),
            manifest_id: RecordDigest::from_bytes([3; 32]),
            byte_length: setup.bytes.len() as u64,
            content_digest: RecordDigest::from_bytes(*setup.digest.as_bytes()),
            executable: false,
        }],
    }
}
#[test]
fn fresh_signed_evidence_crosses_real_socket_and_refuses_bad_header_or_part() {
    let mut setup = Setup::new();
    let mut initial = initial(&setup);
    let template = initial.files[0].clone();
    initial.files = (1u128..=700)
        .map(|n| {
            let mut entry = template.clone();
            entry.object = mesh_materializer::ObjectId::from_bytes(n.to_be_bytes());
            entry.path = format!("result-{n:04}.txt");
            entry
        })
        .collect();
    setup.manifest = RemoteInputManifest::new(
        setup.manifest.input(),
        initial
            .files
            .iter()
            .map(|entry| crate::fleet::RemoteInputEntry::File {
                path: entry.path.clone(),
                executable: false,
                digest: setup.digest,
                chunks: vec![crate::fleet::RemoteInputChunk {
                    digest: setup.digest,
                    bytes: setup.bytes.len() as u64,
                }],
            })
            .collect(),
    )
    .unwrap();
    setup.f.work.assignment.bundle = setup.manifest.bundle();
    let mut runtime = setup.f.runtime(true);
    let revision = runtime.state().revision;
    let mut saved = initial.clone();
    saved.operation = setup.manifest.input();
    let evidence =
        RemoteResultCorrespondence::derive(&setup.manifest, &initial, &setup.manifest, &saved)
            .unwrap();
    let context = RemoteWorkerStatusChallenge::issue(
        &mut runtime,
        "lane",
        "run",
        public(&setup.f.coordinator),
        public(&setup.f.worker),
    )
    .unwrap();
    let offer = RemoteSavedResultOffer::sign(
        Json::object([
            ("target", context.body.get("target").unwrap().clone()),
            ("owner", Json::text("01".repeat(32))),
            ("mapping", Json::text("02".repeat(32))),
            ("initial", Json::text(initial.operation.to_string())),
            ("installation", Json::text("fixture")),
            ("checkpoint", Json::text("checkpoint")),
            ("review", Json::text("03".repeat(32))),
            ("version", Json::text(setup.manifest.input().to_string())),
            ("manifest", Json::text(setup.manifest.bundle().to_string())),
        ]),
        |p| sign(&setup.f.worker, p),
    )
    .unwrap()
    .encode();
    let offer_digest = Blake3::digest_bytes(offer.as_bytes()).to_string();
    assert!(evidence.encoded().len() > 65_536);
    let mut previous_reply: Option<String> = None;
    for mutation in 0..6 {
        let (client, server) = UnixStream::pair().unwrap();
        for stream in [&client, &server] {
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(5)))
                .unwrap();
        }
        let old_reply = previous_reply.clone();
        let result = std::thread::scope(|scope| {
            let peer = scope.spawn(|| {
                let Some(RemoteFrame::Control(raw)) =
                    RemoteFrameReader::new(&server).read_frame().unwrap()
                else {
                    panic!("query");
                };
                let query = RemoteResultEvidenceQuery::decode(std::str::from_utf8(&raw).unwrap())
                    .unwrap()
                    .verify(&policy(&setup))
                    .unwrap();
                assert_eq!(
                    text(&query.query.body, "offer_digest").unwrap(),
                    offer_digest
                );
                let body = Json::object([
                    (
                        "query",
                        Json::text(
                            Blake3::digest_bytes(query.query.body.encode().as_bytes()).to_string(),
                        ),
                    ),
                    ("observed_ms", Json::Number(now().unwrap())),
                    ("offer", Json::text(&offer)),
                    ("evidence", Json::text(evidence.digest().to_string())),
                    (
                        "bytes",
                        Json::Number(if mutation == 3 {
                            4_194_305
                        } else {
                            evidence.encoded().len() as u64
                        }),
                    ),
                ]);
                let signature = sign(
                    if mutation == 1 {
                        &setup.f.coordinator
                    } else {
                        &setup.f.worker
                    },
                    &payload(REPLY_SIGNING, &body),
                )
                .unwrap();
                let reply = envelope(REPLY_SCHEMA, body, &signature);
                let reply = if mutation == 4 {
                    old_reply.as_ref().unwrap()
                } else {
                    &reply
                };
                let mut writer = RemoteFrameWriter::new(&server);
                writer
                    .write_frame(&RemoteFrame::Control(reply.as_bytes().to_vec()))
                    .unwrap();
                if ![1, 3, 4].contains(&mutation) {
                    let mut data = evidence.encoded().as_bytes().to_vec();
                    if mutation == 5 {
                        data[0] ^= 1;
                    }
                    let size = data.len();
                    for (index, part) in data.chunks(65_536).enumerate() {
                        let offset = index * 65_536;
                        writer
                            .write_frame(&RemoteFrame::Chunk {
                                digest: mesh_cas::Digest32::from_bytes(
                                    *evidence.digest().as_bytes(),
                                ),
                                offset: if mutation == 2 { 1 } else { offset as u64 },
                                final_part: offset + part.len() == size,
                                bytes: part.to_vec(),
                            })
                            .unwrap();
                        if mutation == 2 {
                            break;
                        }
                    }
                }
                reply.clone()
            });
            let result = receive_remote_result_evidence(
                RemoteResultEvidenceRequest {
                    status: crate::fleet::RemoteWorkerStatusRequest {
                        runtime: &mut runtime,
                        lane: "lane",
                        run: "run",
                        coordinator: public(&setup.f.coordinator),
                        worker: public(&setup.f.worker),
                    },
                    offer: &offer,
                    input: &setup.manifest,
                    result: &setup.manifest,
                },
                &client,
                &client,
                |p| sign(&setup.f.coordinator, p),
            );
            let reply = peer.join().unwrap();
            if mutation == 0 {
                previous_reply = Some(reply);
            }
            result
        });
        if mutation == 0 {
            let result = result.unwrap();
            assert_eq!(result.correspondence().digest(), evidence.digest());
            assert_eq!(result.offer_digest().to_string(), offer_digest);
        } else {
            assert!(result.is_err(), "mutation {mutation}");
        }
    }
    assert_eq!(runtime.state().revision, revision);
    assert!(setup.f.registry().receipts().unwrap().is_empty());
}
#[test]
fn metadata_query_rejects_other_domains_wrong_keys_and_noncanonical_forms() {
    let setup = Setup::new();
    let mut runtime = setup.f.runtime(true);
    let challenge = RemoteResultEvidenceChallenge::issue(
        crate::fleet::RemoteWorkerStatusRequest {
            runtime: &mut runtime,
            lane: "lane",
            run: "run",
            coordinator: public(&setup.f.coordinator),
            worker: public(&setup.f.worker),
        },
        "checkpoint",
        &"01".repeat(32),
    )
    .unwrap();
    assert!(challenge
        .signed_query(&mut runtime, |p| sign(&setup.f.worker, p))
        .is_err());
    let query = challenge
        .signed_query(&mut runtime, |p| sign(&setup.f.coordinator, p))
        .unwrap();
    assert!(RemoteResultEvidenceQuery::decode(&(query.encode() + " ")).is_err());
    let wrong = RemoteResultEvidenceQuery {
        body: query.body.clone(),
        signature: sign(&setup.f.coordinator, &payload(QUERY_DOMAIN, &query.body)).unwrap(),
    };
    assert!(wrong.verify(&policy(&setup)).is_err());
    let mut expired = query.body.clone();
    let Json::Object(fields) = &mut expired else {
        panic!("body");
    };
    let Json::Object(context) = &mut fields.iter_mut().find(|(k, _)| k == "query").unwrap().1
    else {
        panic!("context");
    };
    context
        .iter_mut()
        .find(|(k, _)| k == "issued_ms")
        .unwrap()
        .1 = Json::Number(1);
    context
        .iter_mut()
        .find(|(k, _)| k == "expires_ms")
        .unwrap()
        .1 = Json::Number(30_001);
    let signature = sign(&setup.f.coordinator, &payload(QUERY_SIGNING, &expired)).unwrap();
    assert!(
        RemoteResultEvidenceQuery::decode(&envelope(QUERY_SCHEMA, expired, &signature))
            .unwrap()
            .verify(&policy(&setup))
            .is_err()
    );
}
