use super::*;
use crate::fleet::remote_admission::authentication::tests::Fixture;
use ed25519_dalek::{Signer as _, SigningKey};
fn public(k: &SigningKey) -> PublicKey {
    PublicKey::from_bytes(k.verifying_key().to_bytes())
}
fn sign(k: &SigningKey, p: &SigningPayload) -> Result<Signature, String> {
    Ok(Signature::from_bytes(k.sign(p.as_bytes()).to_bytes()))
}
fn policy(f: &Fixture) -> RemoteDispatchPolicy<'static> {
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
        max_lease_ms: 120_000,
    }
}
fn prepare(f: &Fixture, runtime: &mut Runtime) -> RemoteLeaseRenewal {
    RemoteLeaseRenewal::prepare(
        runtime,
        "lane",
        "run",
        public(&f.coordinator),
        public(&f.worker),
        RemoteLeaseRenewalPlan {
            expected_sequence: 1,
            until_ms: f.work.assignment.lease_until_ms + 30_000,
            maximum_ms: 120_000,
        },
    )
    .unwrap()
}
fn encoded(frame: RemoteFrame) -> String {
    let RemoteFrame::Control(bytes) = frame else {
        panic!("control expected")
    };
    String::from_utf8(bytes).unwrap()
}
fn request(
    f: &Fixture,
    runtime: &mut Runtime,
    prepared: &RemoteLeaseRenewal,
) -> VerifiedRemoteLeaseRenewal {
    RemoteLeaseRenewalRequest::decode(
        &prepared
            .signed_request(runtime, |p| sign(&f.coordinator, p))
            .unwrap()
            .encode(),
    )
    .unwrap()
    .verify(&policy(f))
    .unwrap()
}
#[test]
fn lost_reply_reconciles_same_intent_after_reopen_without_advancing_before_ack() {
    let f = Fixture::new();
    let mut runtime = f.runtime(true);
    let mut registry = f.registry();
    registry
        .reserve(
            f.work.clone(),
            "0123456789abcdef0123456789abcdef",
            now().unwrap(),
        )
        .unwrap();
    let prepared = prepare(&f, &mut runtime);
    let reply = encoded(
        request(&f, &mut runtime, &prepared)
            .commit(&mut registry, |p| sign(&f.worker, p))
            .unwrap(),
    );
    assert_eq!(
        runtime.state().lanes["lane"].runs[0]
            .remote
            .as_ref()
            .unwrap()
            .lease_sequence,
        1
    );
    drop(prepared);
    drop(runtime);
    drop(registry);
    let mut runtime = Runtime::open(
        FleetStore::open(f.path.join("coordinator.sqlite")).unwrap(),
        "objective",
    )
    .unwrap();
    let mut registry = f.registry();
    let other = prepare(&f, &mut runtime);
    assert!(other.accept(&mut runtime, &reply).is_err());
    let prepared = prepare(&f, &mut runtime);
    let reply = encoded(
        request(&f, &mut runtime, &prepared)
            .commit(&mut registry, |p| sign(&f.worker, p))
            .unwrap(),
    );
    let receipt = prepared.accept(&mut runtime, &reply).unwrap();
    assert_eq!(receipt.sequence, 2);
    assert_eq!(
        runtime.state().lanes["lane"].runs[0]
            .remote
            .as_ref()
            .unwrap()
            .lease_sequence,
        2
    );
    assert_eq!(registry.receipts().unwrap().len(), 1);
    assert_eq!(runtime.state().lanes["lane"].runs.len(), 1);
    let reopened = Runtime::open(
        FleetStore::open(f.path.join("coordinator.sqlite")).unwrap(),
        "objective",
    )
    .unwrap();
    assert_eq!(
        reopened.state().lanes["lane"].runs[0]
            .remote
            .as_ref()
            .unwrap()
            .lease_sequence,
        2
    );
}
#[test]
fn conflicting_intent_wrong_key_and_unknown_fields_cannot_renew() {
    let f = Fixture::new();
    let mut runtime = f.runtime(true);
    let mut registry = f.registry();
    registry
        .reserve(
            f.work.clone(),
            "0123456789abcdef0123456789abcdef",
            now().unwrap(),
        )
        .unwrap();
    let prepared = prepare(&f, &mut runtime);
    assert!(RemoteLeaseRenewal::prepare(
        &mut runtime,
        "lane",
        "run",
        public(&f.coordinator),
        public(&f.worker),
        RemoteLeaseRenewalPlan {
            expected_sequence: 1,
            until_ms: f.work.assignment.lease_until_ms + 40_000,
            maximum_ms: 120_000
        }
    )
    .is_err());
    let encoded = prepared
        .signed_request(&mut runtime, |p| sign(&f.coordinator, p))
        .unwrap()
        .encode();
    let mut wrong = policy(&f);
    wrong.coordinator = public(&SigningKey::from_bytes(&[90; 32]));
    assert!(RemoteLeaseRenewalRequest::decode(&encoded)
        .unwrap()
        .verify(&wrong)
        .is_err());
    assert!(RemoteLeaseRenewalRequest::decode(&encoded.replacen(
        "\"expected_sequence\":1",
        "\"extra\":1,\"expected_sequence\":1",
        1
    ))
    .is_err());
    assert_eq!(
        registry
            .effective_lease(&registry.receipts().unwrap()[0])
            .unwrap()
            .sequence,
        1
    );
    runtime.record("cancel", Command::Cancel).unwrap();
    assert!(prepared
        .signed_request(&mut runtime, |p| sign(&f.coordinator, p))
        .is_err());
}
#[test]
fn failed_ack_signer_keeps_committed_lease_for_exact_reconciliation() {
    let f = Fixture::new();
    let mut runtime = f.runtime(true);
    let mut registry = f.registry();
    registry
        .reserve(
            f.work.clone(),
            "0123456789abcdef0123456789abcdef",
            now().unwrap(),
        )
        .unwrap();
    let prepared = prepare(&f, &mut runtime);
    assert!(request(&f, &mut runtime, &prepared)
        .commit(&mut registry, |_| Err("signer unavailable".into()))
        .is_err());
    assert_eq!(
        registry
            .effective_lease(&registry.receipts().unwrap()[0])
            .unwrap()
            .sequence,
        2
    );
    assert_eq!(
        runtime.state().lanes["lane"].runs[0]
            .remote
            .as_ref()
            .unwrap()
            .lease_sequence,
        1
    );
    let again = prepare(&f, &mut runtime);
    let reply = encoded(
        request(&f, &mut runtime, &again)
            .commit(&mut registry, |p| sign(&f.worker, p))
            .unwrap(),
    );
    again.accept(&mut runtime, &reply).unwrap();
    assert_eq!(runtime.state().lanes["lane"].runs.len(), 1);
}

#[test]
fn resident_route_recovers_a_dropped_ack_without_another_admission_or_launch() {
    use crate::fleet::{
        receiving_session::tests::Setup, NativeWorkerConnections, NativeWorkerInstallation,
        WorkerConnectionOutcome,
    };
    use crate::ProtectedWorkspaceRoot;
    use std::os::unix::fs::PermissionsExt as _;
    struct LostReply;
    impl Write for LostReply {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let s = Setup::new();
    let root = s.f.path.join("renewal-installation");
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
    registry
        .reserve(
            s.f.work.clone(),
            "0123456789abcdef0123456789abcdef",
            now().unwrap(),
        )
        .unwrap();
    let mut runtime = s.f.runtime(true);
    let mut hub =
        NativeWorkerConnections::new(&installation, &s.destination, policy(&s.f), 1).unwrap();
    let prepared = prepare(&s.f, &mut runtime);
    let wire = prepared
        .signed_request(&mut runtime, |p| sign(&s.f.coordinator, p))
        .unwrap();
    let mut bytes = Vec::new();
    RemoteFrameWriter::new(&mut bytes)
        .write_frame(&wire.frame().unwrap())
        .unwrap();
    assert!(hub
        .serve(std::io::Cursor::new(bytes), LostReply, |p| sign(
            &s.f.worker,
            p
        ))
        .is_err());
    assert_eq!(
        runtime.state().lanes["lane"].runs[0]
            .remote
            .as_ref()
            .unwrap()
            .lease_sequence,
        1
    );
    assert_eq!(
        registry
            .effective_lease(&registry.receipts().unwrap()[0])
            .unwrap()
            .sequence,
        2
    );
    let replay = prepare(&s.f, &mut runtime);
    let wire = replay
        .signed_request(&mut runtime, |p| sign(&s.f.coordinator, p))
        .unwrap();
    let mut bytes = Vec::new();
    RemoteFrameWriter::new(&mut bytes)
        .write_frame(&wire.frame().unwrap())
        .unwrap();
    let mut output = Vec::new();
    assert!(matches!(
        hub.serve(std::io::Cursor::new(bytes), &mut output, |p| sign(
            &s.f.worker,
            p
        ))
        .unwrap(),
        WorkerConnectionOutcome::LeaseReplied
    ));
    let response = RemoteFrameReader::new(std::io::Cursor::new(output))
        .read_frame()
        .unwrap()
        .unwrap();
    replay.accept(&mut runtime, &encoded(response)).unwrap();
    assert_eq!(registry.receipts().unwrap().len(), 1);
    assert!(registry.launch_receipt("assignment").unwrap().is_none());
    assert_eq!(runtime.state().lanes["lane"].runs.len(), 1);
}

#[test]
fn expired_signed_request_and_forged_ack_do_not_advance_coordinator() {
    let f = Fixture::new();
    let mut runtime = f.runtime(true);
    let mut registry = f.registry();
    registry
        .reserve(
            f.work.clone(),
            "0123456789abcdef0123456789abcdef",
            now().unwrap(),
        )
        .unwrap();
    let prepared = prepare(&f, &mut runtime);
    let mut request = prepared
        .signed_request(&mut runtime, |p| sign(&f.coordinator, p))
        .unwrap();
    let Json::Object(fields) = &mut request.body else {
        unreachable!()
    };
    let query = fields.iter_mut().find(|(k, _)| k == "query").unwrap();
    let Json::Object(fields) = &mut query.1 else {
        unreachable!()
    };
    for (key, value) in fields {
        if key == "issued_ms" {
            *value = Json::Number(1);
        }
        if key == "expires_ms" {
            *value = Json::Number(30_001);
        }
    }
    request.signature = sign(&f.coordinator, &payload(REQUEST_DOMAIN, &request.body)).unwrap();
    assert!(RemoteLeaseRenewalRequest::decode(&request.encode())
        .unwrap()
        .verify(&policy(&f))
        .is_err());
    assert_eq!(
        registry
            .effective_lease(&registry.receipts().unwrap()[0])
            .unwrap()
            .sequence,
        1
    );
    let reply = encoded(
        super::tests::request(&f, &mut runtime, &prepared)
            .commit(&mut registry, |p| sign(&f.worker, p))
            .unwrap(),
    );
    let parsed = Json::parse(&reply).unwrap();
    let forged = envelope(
        ACK,
        parsed.get("body").unwrap().clone(),
        &Signature::from_bytes([0; 64]),
    );
    assert!(prepared.accept(&mut runtime, &forged).is_err());
    assert_eq!(
        runtime.state().lanes["lane"].runs[0]
            .remote
            .as_ref()
            .unwrap()
            .lease_sequence,
        1
    );
}
