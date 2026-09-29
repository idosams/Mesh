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

#[test]
fn durable_evidence_survives_restart_retains_first_attestation_and_refuses_missing_metadata() {
    use crate::fleet::{NativeRemoteResultReceiver, RemoteWorkerStatusRequest, Runtime};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    let setup = Setup::new();
    let mut runtime = setup.f.runtime(true);
    let before = runtime.state().revision;
    let initial = initial(&setup);
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
    fn status<'a>(setup: &Setup, runtime: &'a mut Runtime) -> RemoteWorkerStatusRequest<'a> {
        RemoteWorkerStatusRequest {
            runtime,
            lane: "lane",
            run: "run",
            coordinator: public(&setup.f.coordinator),
            worker: public(&setup.f.worker),
        }
    }
    macro_rules! request {
        ($runtime:expr) => {
            RemoteResultEvidenceRequest {
                status: status(&setup, $runtime),
                offer: &offer,
                input: &setup.manifest,
                result: &setup.manifest,
            }
        };
    }
    let attest = |raw: &str, nonce: u8, key: &SigningKey| {
        let body = Json::object([
            ("query", Json::text(format!("{nonce:02x}").repeat(32))),
            ("observed_ms", Json::Number(1)),
            ("offer", Json::text(&offer)),
            (
                "evidence",
                Json::text(Blake3::digest_bytes(raw.as_bytes()).to_string()),
            ),
            ("bytes", Json::Number(raw.len() as u64)),
        ]);
        let signature = sign(key, &payload(REPLY_SIGNING, &body)).unwrap();
        envelope(REPLY_SCHEMA, body, &signature)
    };
    let first = attest(evidence.encoded(), 1, &setup.f.worker);
    let authenticated = AuthenticatedRemoteResultEvidence::verify_retained(
        request!(&mut runtime),
        &first,
        evidence.encoded(),
    )
    .unwrap();
    let wrong = attest(evidence.encoded(), 1, &setup.f.coordinator);
    assert!(AuthenticatedRemoteResultEvidence::verify_retained(
        request!(&mut runtime),
        &wrong,
        evidence.encoded(),
    )
    .is_err());
    let mut receiver = NativeRemoteResultReceiver::new(
        &setup.destination,
        setup.manifest.clone(),
        &offer,
        status(&setup, &mut runtime),
    )
    .unwrap();
    assert!(receiver
        .record_evidence_receipt(&mut runtime, &setup.manifest, &authenticated)
        .is_err());
    receiver
        .accept(&mut runtime, setup.digest, 0, &setup.bytes, true)
        .unwrap();
    let metadata = setup
        .f
        .path
        .join("store")
        .join(format!("result-evidence-{}.json", evidence.digest()));
    fs::write(&metadata, b"partial evidence").unwrap();
    fs::set_permissions(&metadata, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(receiver
        .record_evidence_receipt(&mut runtime, &setup.manifest, &authenticated)
        .is_err());
    assert_eq!(fs::read(&metadata).unwrap(), b"partial evidence");
    let preserved = metadata.with_extension("preserved");
    fs::rename(&metadata, &preserved).unwrap();
    let receipt = receiver
        .record_evidence_receipt(&mut runtime, &setup.manifest, &authenticated)
        .unwrap();
    let receipt_digest = receipt.digest();
    let second = attest(evidence.encoded(), 2, &setup.f.worker);
    let replay = AuthenticatedRemoteResultEvidence::verify_retained(
        request!(&mut runtime),
        &second,
        evidence.encoded(),
    )
    .unwrap();
    let repeated = receiver
        .record_evidence_receipt(&mut runtime, &setup.manifest, &replay)
        .unwrap();
    assert_eq!(repeated.digest(), receipt_digest);
    assert_eq!(repeated.evidence().attestation(), first);
    // A different, validly signed provenance claim for the same content cannot replace the first.
    let changed = evidence
        .encoded()
        .replace("\"input_path\":\"result.txt\"", "\"input_path\":null");
    assert_ne!(changed, evidence.encoded());
    let conflict = AuthenticatedRemoteResultEvidence::verify_retained(
        request!(&mut runtime),
        &attest(&changed, 3, &setup.f.worker),
        &changed,
    )
    .unwrap();
    assert!(receiver
        .record_evidence_receipt(&mut runtime, &setup.manifest, &conflict)
        .is_err());
    drop(receiver);
    drop(runtime);
    let mut runtime = Runtime::open(
        crate::fleet::FleetStore::open(setup.f.path.join("coordinator.sqlite")).unwrap(),
        "objective",
    )
    .unwrap();
    let (receiver, restored) = NativeRemoteResultReceiver::reopen_evidence_receipt(
        &setup.destination,
        request!(&mut runtime),
    )
    .unwrap();
    assert_eq!(restored.digest(), receipt_digest);
    assert_eq!(restored.evidence().attestation(), first);
    let allocation = receiver
        .materialize_result(
            &mut runtime,
            &setup.manifest,
            &restored,
            "0123456789abcdef0123456789abcdef",
        )
        .unwrap();
    assert_eq!(allocation.evidence_receipt(), receipt_digest);
    assert_eq!(allocation.manifest(), &setup.manifest);
    assert_eq!(
        fs::read(allocation.path().join("result.txt")).unwrap(),
        setup.bytes
    );
    assert_eq!(
        allocation.path().parent().unwrap().file_name().unwrap(),
        "result-0123456789abcdef0123456789abcdef"
    );
    allocation.verify().unwrap();
    assert!(
        receiver
            .materialize_result(
                &mut runtime,
                &setup.manifest,
                &restored,
                "0123456789abcdef0123456789abcdef",
            )
            .is_err(),
        "existing output cannot be adopted or overwritten"
    );
    fs::write(
        allocation.path().join("result.txt"),
        b"changed local candidate",
    )
    .unwrap();
    assert!(allocation.verify().is_err());
    assert!(receiver
        .materialize_result(
            &mut runtime,
            &setup.manifest,
            &restored,
            "0123456789abcdef0123456789abcdef",
        )
        .is_err());
    assert_eq!(
        fs::read(allocation.path().join("result.txt")).unwrap(),
        b"changed local candidate"
    );
    // Native CAS is independent of the new candidate copy.
    receiver.verify_complete(&mut runtime).unwrap();
    assert!(
        allocation
            .into_result_workspace(
                crate::TrustedReviewers::default(),
                crate::CheckpointRuntimeParameters::selected_defaults(),
            )
            .is_err(),
        "changed copy cannot become saved native history"
    );
    let clean = receiver
        .materialize_result(
            &mut runtime,
            &setup.manifest,
            &restored,
            "22222222222222222222222222222222",
        )
        .unwrap();
    let local_parent = clean.path().parent().unwrap().to_path_buf();
    let local = clean
        .into_result_workspace(
            crate::TrustedReviewers::default(),
            crate::CheckpointRuntimeParameters::selected_defaults(),
        )
        .unwrap();
    local.verify().unwrap();
    assert_eq!(local.binding().source_version, setup.manifest.input());
    assert_ne!(
        local.binding().starting_version,
        Some(setup.manifest.input())
    );
    assert_eq!(
        local
            .receipt()
            .get("context")
            .unwrap()
            .get("evidence_receipt")
            .unwrap()
            .as_text(),
        Some(receipt_digest.to_string().as_str())
    );
    let local_receipt = local_parent.join("result-workspace.json");
    let retained_local = local_parent.join("preserved-result-workspace.json");
    fs::rename(&local_receipt, &retained_local).unwrap();
    assert!(local.verify().is_err());
    assert!(!local_receipt.exists());
    fs::rename(&retained_local, &local_receipt).unwrap();
    local.verify().unwrap();
    fs::hard_link(&local_receipt, &retained_local).unwrap();
    assert!(local.verify().is_err());
    // Restore the test's extra hard link before creating a review under native custody.
    fs::remove_file(&retained_local).unwrap();
    let review = local.record_review(public(&setup.f.coordinator)).unwrap();
    assert_eq!(
        local.record_review(public(&setup.f.coordinator)).unwrap(),
        review
    );
    let correlation = receiver
        .record_local_review(
            &mut runtime,
            &setup.manifest,
            &restored,
            &local,
            public(&setup.f.coordinator),
        )
        .unwrap();
    assert_eq!(correlation.review(), review);
    assert_eq!(
        receiver
            .record_local_review(
                &mut runtime,
                &setup.manifest,
                &restored,
                &local,
                public(&setup.f.coordinator),
            )
            .unwrap(),
        correlation
    );
    let offer_id = RecordDigest::from_bytes(*Blake3::digest_bytes(offer.as_bytes()).as_bytes());
    let alternative = receiver
        .materialize_result(
            &mut runtime,
            &setup.manifest,
            &restored,
            "33333333333333333333333333333333",
        )
        .unwrap()
        .into_result_workspace(
            crate::TrustedReviewers::default(),
            crate::CheckpointRuntimeParameters::selected_defaults(),
        )
        .unwrap();
    assert!(
        receiver
            .record_local_review(
                &mut runtime,
                &setup.manifest,
                &restored,
                &alternative,
                public(&setup.f.coordinator),
            )
            .is_err(),
        "another allocation cannot replace the committed correlation"
    );
    assert_eq!(
        runtime
            .retained_remote_local_review(offer_id)
            .unwrap()
            .unwrap(),
        correlation
    );
    drop(alternative);
    let review_root = local.binding().root.clone();
    let review_installation = local.binding().installation.clone();
    fs::hard_link(&local_receipt, &retained_local).unwrap();
    assert!(local.record_review(public(&setup.f.coordinator)).is_err());
    let mapping = local.receipt_digest();
    let local_operation = local.binding().starting_version.unwrap();
    drop(local);
    let reopen = |mapping| {
        setup.destination.reopen_result_history(
            "22222222222222222222222222222222",
            mapping,
            &restored,
            &setup.manifest,
            &crate::TrustedReviewers::default(),
        )
    };
    assert!(
        reopen(mapping).is_err(),
        "linked retained mapping must refuse"
    );
    fs::remove_file(&retained_local).unwrap();
    assert!(reopen(RecordDigest::from_bytes([77; 32])).is_err());
    let source = reopen(mapping).unwrap();
    let historical = Runtime::open(
        crate::fleet::FleetStore::open(setup.f.path.join("coordinator.sqlite")).unwrap(),
        "objective",
    )
    .unwrap();
    let discovered = historical.remote_local_reviews(0, None).unwrap();
    assert_eq!(discovered.entries.len(), 1);
    assert_eq!(
        discovered.entries[0].receipt.as_ref().unwrap(),
        &correlation
    );
    let durable = historical
        .retained_remote_local_review(offer_id)
        .unwrap()
        .unwrap();
    assert_eq!(durable, correlation);
    assert_eq!(
        durable
            .reopen(
                &setup.destination,
                &setup.manifest,
                &crate::TrustedReviewers::default()
            )
            .unwrap()
            .read_chunk(setup.digest)
            .unwrap(),
        setup.bytes
    );

    let open = crate::workspace::OpenWorkspace::reopen_history(
        std::path::Path::new(&review_root),
        &review_installation,
        crate::ProtectedWorkspaceRoot::inspect(&local_parent).unwrap(),
        &crate::TrustedReviewers::default(),
    )
    .unwrap();
    assert_eq!(
        open.review(&review).unwrap().subject_operation,
        local_operation
    );
    assert!(
        open.shared_version().is_none(),
        "review creation cannot approve protected main"
    );
    drop(open);

    assert_eq!(source.manifest().input(), local_operation);
    assert_eq!(source.read_chunk(setup.digest).unwrap(), setup.bytes);
    fs::rename(&local_receipt, &retained_local).unwrap();
    assert!(reopen(mapping).is_err());
    assert!(
        !local_receipt.exists(),
        "history-only reopen cannot repair metadata"
    );
    fs::rename(&retained_local, &local_receipt).unwrap();
    let moved = local_parent.with_extension("preserved");
    fs::rename(&local_parent, &moved).unwrap();
    assert!(
        source.read_chunk(setup.digest).is_err(),
        "retained source must keep allocation custody"
    );
    fs::rename(&moved, &local_parent).unwrap();

    let retained = metadata.with_extension("retained");
    fs::rename(&metadata, &retained).unwrap();
    assert!(receiver
        .record_evidence_receipt(&mut runtime, &setup.manifest, &authenticated)
        .is_err());
    assert!(!metadata.exists());
    assert!(
        receiver
            .materialize_result(
                &mut runtime,
                &setup.manifest,
                &restored,
                "11111111111111111111111111111111",
            )
            .is_err(),
        "missing evidence refuses before allocation"
    );
    assert!(!setup
        .f
        .path
        .join("allocations/result-11111111111111111111111111111111")
        .exists());

    drop(receiver);
    assert!(NativeRemoteResultReceiver::reopen_evidence_receipt(
        &setup.destination,
        request!(&mut runtime)
    )
    .is_err());
    fs::rename(&retained, &metadata).unwrap();
    fs::hard_link(&metadata, &retained).unwrap();
    assert!(NativeRemoteResultReceiver::reopen_evidence_receipt(
        &setup.destination,
        request!(&mut runtime)
    )
    .is_err());
    assert_eq!(runtime.state().revision, before);
    runtime
        .record("cancel-after-review", crate::fleet::Command::Cancel)
        .unwrap();
    assert_eq!(
        runtime
            .retained_remote_local_review(offer_id)
            .unwrap()
            .unwrap(),
        correlation
    );
    assert_eq!(
        correlation
            .reopen(
                &setup.destination,
                &setup.manifest,
                &crate::TrustedReviewers::default()
            )
            .unwrap()
            .read_chunk(setup.digest)
            .unwrap(),
        setup.bytes
    );
    runtime
        .store
        .append_with_outcome(&format!("result-local-review-{offer_id}"), 1, "extra", "{}")
        .unwrap();
    assert!(runtime.retained_remote_local_review(offer_id).is_err());

    assert_eq!(
        fs::read_dir(setup.f.path.join("allocations"))
            .unwrap()
            .count(),
        3
    );
}
