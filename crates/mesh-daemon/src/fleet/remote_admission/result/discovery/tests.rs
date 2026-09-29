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
fn challenge(f: &Fixture, r: &mut Runtime, after: u64) -> RemoteResultDiscoveryChallenge {
    RemoteResultDiscoveryChallenge::issue(
        RemoteWorkerStatusRequest {
            runtime: r,
            lane: "lane",
            run: "run",
            coordinator: public(&f.coordinator),
            worker: public(&f.worker),
        },
        after,
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
    c: &RemoteResultDiscoveryChallenge,
) -> VerifiedRemoteResultDiscoveryQuery {
    RemoteResultDiscoveryQuery::decode(
        &c.signed_query(r, |p| sign(&f.coordinator, p))
            .unwrap()
            .encode(),
    )
    .unwrap()
    .verify(&policy(f))
    .unwrap()
}
#[test]
fn discovery_query_null_is_read_only_and_old_reply_cannot_answer_a_new_challenge() {
    let f = Fixture::new();
    let mut r = f.runtime(true);
    let registry = f.registry();
    let revision = r.state().revision;
    let c = challenge(&f, &mut r, 0);
    let q = request(&f, &mut r, &c);
    let reply = encoded(q.reply(&registry, |p| sign(&f.worker, p)).unwrap());
    assert!(c.verify_reply(&mut r, &reply).unwrap().is_none());
    let next = challenge(&f, &mut r, 0);
    assert!(next.verify_reply(&mut r, &reply).is_err());
    assert_eq!(r.state().revision, revision);
    assert!(registry.receipts().unwrap().is_empty());
}
#[test]
fn discovery_query_refuses_wrong_keys_domains_expired_requests_and_noncanonical_fields() {
    let f = Fixture::new();
    let mut r = f.runtime(true);
    let registry = f.registry();
    let c = challenge(&f, &mut r, 0);
    assert!(c.signed_query(&mut r, |p| sign(&f.worker, p)).is_err());
    let original = c.signed_query(&mut r, |p| sign(&f.coordinator, p)).unwrap();
    let wrong = RemoteResultDiscoveryQuery {
        body: original.body.clone(),
        signature: sign(&f.coordinator, &payload(QUERY_DOMAIN, &original.body)).unwrap(),
    };
    assert!(RemoteResultDiscoveryQuery::decode(&wrong.encode())
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
        RemoteResultDiscoveryQuery::decode(&envelope(QUERY_SCHEMA, body, &signature))
            .unwrap()
            .verify(&policy(&f))
            .is_err()
    );
    assert!(RemoteResultDiscoveryQuery::decode(&(original.encode() + " ")).is_err());
    assert!(RemoteResultDiscoveryQuery::decode(&"x".repeat(MAX + 1)).is_err());
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
fn resident_discovery_recovers_catalog_after_lost_reply_and_refuses_invalid_signed_pages() {
    use crate::fleet::{
        receiving_session::tests::Setup, NativeRemoteInputReceiver, NativeWorkerConnections,
        NativeWorkerInstallation, RemoteLaunchOutcome, WorkerConnectionOutcome,
    };
    use crate::{CheckpointRuntimeParameters, ProtectedWorkspaceRoot, TrustedReviewers};
    use std::os::unix::fs::PermissionsExt;
    let s = Setup::new();
    let mut r = s.f.runtime(true);
    let root = s.f.path.join("installation");
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let (installation, ()) = NativeWorkerInstallation::provision(
        &root,
        ProtectedWorkspaceRoot::inspect(&root).unwrap(),
        &[],
        |_| Ok((public(&s.f.worker), ())),
    )
    .unwrap();
    let mut registry = installation
        .registry(
            &hex(public(&s.f.coordinator).as_bytes()),
            "objective",
            policy(&s.f).maximum,
        )
        .unwrap();
    let RemoteAdmissionOutcome::Reserved(input) = registry
        .reserve(
            s.f.work.clone(),
            "0123456789abcdef0123456789abcdef",
            now().unwrap(),
        )
        .unwrap()
    else {
        panic!("original");
    };
    let mut receiver =
        NativeRemoteInputReceiver::new(&s.destination, s.manifest.clone(), &s.f.work.assignment)
            .unwrap();
    receiver.accept(s.digest, 0, &s.bytes, true).unwrap();
    let workspace = receiver
        .materialize_reserved(input)
        .unwrap()
        .into_worker_workspace(
            TrustedReviewers::default(),
            CheckpointRuntimeParameters::selected_defaults(),
        )
        .unwrap();
    let RemoteLaunchOutcome::Reserved(launch) = registry
        .reserve_launch(workspace, "codex", now().unwrap())
        .unwrap()
    else {
        panic!("launch");
    };
    let receipt = launch.receipt().clone();
    drop(launch);
    let mut registry = installation
        .registry(
            &hex(public(&s.f.coordinator).as_bytes()),
            "objective",
            policy(&s.f).maximum,
        )
        .unwrap();
    // Protocol fixture: actual saved-tree/export/sign composition is exercised by received-host tests.
    let manifest = RemoteInputManifest::new(RecordDigest::from_bytes([0x95; 32]), vec![]).unwrap();
    let body = RemoteSavedResultOffer::body(
        &receipt,
        "checkpoint",
        RecordDigest::from_bytes([0x96; 32]),
        &manifest,
    )
    .unwrap();
    let expected = RemoteSavedResultOffer::sign(body, |p| sign(&s.f.worker, p))
        .unwrap()
        .persist(&mut registry.store)
        .unwrap()
        .encode();
    let mut hub =
        NativeWorkerConnections::new(&installation, &s.destination, policy(&s.f), 1).unwrap();
    struct Lost;
    impl Write for Lost {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let c = challenge(&s.f, &mut r, 0);
    let query = c
        .signed_query(&mut r, |p| sign(&s.f.coordinator, p))
        .unwrap();
    let mut bytes = Vec::new();
    RemoteFrameWriter::new(&mut bytes)
        .write_frame(&query.frame().unwrap())
        .unwrap();
    assert!(hub
        .serve(std::io::Cursor::new(bytes), Lost, |p| sign(&s.f.worker, p))
        .is_err());
    let c = challenge(&s.f, &mut r, 0);
    let query = c
        .signed_query(&mut r, |p| sign(&s.f.coordinator, p))
        .unwrap();
    let mut bytes = Vec::new();
    RemoteFrameWriter::new(&mut bytes)
        .write_frame(&query.frame().unwrap())
        .unwrap();
    let mut out = Vec::new();
    assert!(matches!(
        hub.serve(std::io::Cursor::new(bytes), &mut out, |p| sign(
            &s.f.worker,
            p
        ))
        .unwrap(),
        WorkerConnectionOutcome::ResultsDiscovered
    ));
    let reply = encoded(
        RemoteFrameReader::new(std::io::Cursor::new(out))
            .read_frame()
            .unwrap()
            .unwrap(),
    );
    let mut page = c.verify_reply(&mut r, &reply).unwrap().unwrap();
    assert_eq!((page.revision, page.after, page.has_more), (1, 1, false));
    let offer = page.offers.pop().unwrap();
    assert_eq!(offer.encode(), expected);
    RemoteSavedResultOffer::verify(
        &mut r,
        "lane",
        "run",
        public(&s.f.coordinator),
        public(&s.f.worker),
        &offer.encode(),
        &manifest,
    )
    .unwrap();
    let c = challenge(&s.f, &mut r, 1);
    let body = Json::object([
        (
            "query",
            Json::text(Blake3::digest_bytes(c.body.encode().as_bytes()).to_string()),
        ),
        ("observed_ms", Json::Number(now().unwrap())),
        (
            "page",
            encode_page(RemoteSavedResultPage {
                revision: 1,
                after: 1,
                has_more: false,
                offers: vec![RemoteSavedResultOffer::decode(&expected).unwrap()],
            }),
        ),
    ]);
    let signature = sign(&s.f.worker, &payload(REPLY_SIGNING, &body)).unwrap();
    assert!(c
        .verify_reply(&mut r, &envelope(REPLY_SCHEMA, body, &signature))
        .is_err());
    // A valid worker signature cannot excuse a duplicate checkpoint or an offer for another run.
    for wrong_scope in [false, true] {
        let c = challenge(&s.f, &mut r, 0);
        let original = RemoteSavedResultOffer::decode(&expected).unwrap();
        let (revision, offers) = if wrong_scope {
            let mut body = original.body.clone();
            let Json::Object(fields) = &mut body else {
                panic!("object");
            };
            let Json::Object(target) =
                &mut fields.iter_mut().find(|(k, _)| k == "target").unwrap().1
            else {
                panic!("target");
            };
            target.iter_mut().find(|(k, _)| k == "run").unwrap().1 = Json::text("other-run");
            (
                1,
                vec![RemoteSavedResultOffer::sign(body, |p| sign(&s.f.worker, p)).unwrap()],
            )
        } else {
            (
                2,
                vec![original, RemoteSavedResultOffer::decode(&expected).unwrap()],
            )
        };
        let body = Json::object([
            (
                "query",
                Json::text(Blake3::digest_bytes(c.body.encode().as_bytes()).to_string()),
            ),
            ("observed_ms", Json::Number(now().unwrap())),
            (
                "page",
                encode_page(RemoteSavedResultPage {
                    revision,
                    after: revision,
                    has_more: false,
                    offers,
                }),
            ),
        ]);
        let signature = sign(&s.f.worker, &payload(REPLY_SIGNING, &body)).unwrap();
        assert!(c
            .verify_reply(&mut r, &envelope(REPLY_SCHEMA, body, &signature))
            .is_err());
    }
    let c = challenge(&s.f, &mut r, 0);
    let q = request(&s.f, &mut r, &c);
    let stream =
        RemoteSavedResultOffer::stream(&RemoteSavedResultOffer::decode(&expected).unwrap().body)
            .unwrap();
    assert!(q
        .reply(&registry, |p| {
            let mut other = installation
                .registry(
                    &hex(public(&s.f.coordinator).as_bytes()),
                    "objective",
                    policy(&s.f).maximum,
                )
                .unwrap();
            other
                .store
                .append_with_outcome(&stream, 1, "unexpected", "{}")
                .unwrap();
            sign(&s.f.worker, p)
        })
        .is_err());
    assert_eq!(registry.receipts().unwrap().len(), 1);
    assert!(registry.launch_receipt("assignment").unwrap().unwrap() == receipt);
    assert_eq!(r.state().lanes["lane"].runs.len(), 1);
}
