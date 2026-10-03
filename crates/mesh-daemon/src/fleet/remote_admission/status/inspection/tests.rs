use super::*;
use crate::fleet::receiving_session::tests::Setup;
use crate::fleet::{NativeRemoteInputReceiver, RemoteAdmissionOutcome};
use ed25519_dalek::{Signer as _, SigningKey};
const ID: &str = "0123456789abcdef0123456789abcdef";
fn public(k: &SigningKey) -> PublicKey {
    PublicKey::from_bytes(k.verifying_key().to_bytes())
}
fn sign(k: &SigningKey, p: &SigningPayload) -> Result<Signature, String> {
    Ok(Signature::from_bytes(k.sign(p.as_bytes()).to_bytes()))
}
fn retained(setup: &Setup) {
    let mut registry = setup.f.registry();
    let RemoteAdmissionOutcome::Reserved(reservation) = registry
        .reserve(setup.f.work.clone(), ID, now().unwrap())
        .unwrap()
    else {
        panic!("reservation")
    };
    let mut receiver = NativeRemoteInputReceiver::new(
        &setup.destination,
        setup.manifest.clone(),
        &setup.f.work.assignment,
    )
    .unwrap();
    receiver
        .accept(setup.digest, 0, &setup.bytes, true)
        .unwrap();
    let allocation = receiver.materialize_reserved(reservation).unwrap();
    registry.retain_materialization(&allocation).unwrap();
}
fn query(
    setup: &Setup,
    runtime: &mut Runtime,
    version: StatusVersion,
) -> (RemoteWorkerStatusChallenge, VerifiedRemoteWorkerStatusQuery) {
    let mut challenge = RemoteWorkerStatusChallenge::issue_with_input_inspection(
        runtime,
        "lane",
        "run",
        public(&setup.f.coordinator),
        public(&setup.f.worker),
    )
    .unwrap();
    challenge.version = version;
    let encoded = challenge
        .signed_query(runtime, |p| sign(&setup.f.coordinator, p))
        .unwrap()
        .encode();
    let policy = RemoteDispatchPolicy {
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
    };
    let verified = RemoteWorkerStatusQuery::decode(&encoded)
        .unwrap()
        .verify(&policy)
        .unwrap();
    (challenge, verified)
}
fn encoded(frame: RemoteFrame) -> String {
    let RemoteFrame::Control(v) = frame else {
        panic!("control")
    };
    String::from_utf8(v).unwrap()
}
#[test]
fn signed_restart_inspection_distinguishes_verified_changed_and_unrecorded_without_execution() {
    for state in ["unrecorded", "verified", "unavailable"] {
        let setup = Setup::new();
        if state != "unrecorded" {
            retained(&setup);
        }
        if state == "unavailable" {
            std::fs::write(
                setup
                    .f
                    .path
                    .join(format!("allocations/input-{ID}/files/result.txt")),
                b"changed",
            )
            .unwrap();
        }
        let mut runtime = setup.f.runtime(true);
        let (c, q) = query(&setup, &mut runtime, StatusVersion::Inspected);
        let registry = setup.f.registry();
        assert!(q.reply(&registry, |p| sign(&setup.f.worker, p)).is_err());
        let reply = encoded(
            q.reply_with_input_inspection(&registry, &setup.destination, |p| {
                sign(&setup.f.worker, p)
            })
            .unwrap(),
        );
        let receipt = c.verify_reply(&mut runtime, &reply).unwrap();
        assert_eq!(receipt.input_inspection(), Some(state));
        assert!(receipt.reports_effective_lease());
        if state == "unrecorded" {
            assert!(registry.receipts().unwrap().is_empty());
        } else {
            assert!(registry.launch_receipt("assignment").unwrap().is_none());
        }
        assert!(!setup
            .f
            .path
            .join(format!("allocations/input-{ID}/initialization.json"))
            .exists());
        assert_eq!(runtime.state().lanes["lane"].runs.len(), 1);
    }
}
#[test]
fn changed_input_during_signing_refuses_signed_verified_reply() {
    let setup = Setup::new();
    retained(&setup);
    let mut runtime = setup.f.runtime(true);
    let (_, q) = query(&setup, &mut runtime, StatusVersion::Inspected);
    let file = setup
        .f
        .path
        .join(format!("allocations/input-{ID}/files/result.txt"));
    assert!(q
        .reply_with_input_inspection(&setup.f.registry(), &setup.destination, |p| {
            std::fs::write(&file, b"changed during signing").unwrap();
            sign(&setup.f.worker, p)
        })
        .is_err());
    assert_eq!(std::fs::read(&file).unwrap(), b"changed during signing");
}
#[test]
fn old_status_remains_ledger_only_and_cannot_satisfy_explicit_inspection() {
    let setup = Setup::new();
    retained(&setup);
    std::fs::rename(
        setup.f.path.join("allocations"),
        setup.f.path.join("preserved-allocations"),
    )
    .unwrap();
    let mut runtime = setup.f.runtime(true);
    for version in [StatusVersion::Initial, StatusVersion::Effective] {
        let (c, q) = query(&setup, &mut runtime, version);
        let reply = encoded(
            q.reply_with_input_inspection(&setup.f.registry(), &setup.destination, |p| {
                sign(&setup.f.worker, p)
            })
            .unwrap(),
        );
        let (explicit, _) = query(&setup, &mut runtime, StatusVersion::Inspected);
        assert!(explicit.verify_reply(&mut runtime, &reply).is_err());
        assert_eq!(
            c.verify_reply(&mut runtime, &reply)
                .unwrap()
                .input_inspection(),
            None
        );
    }
}
#[test]
fn inspection_fields_are_closed_and_cannot_assert_verified_without_admission() {
    let base = Json::object([
        ("admission", Json::Null),
        ("launch", Json::Null),
        ("effective_lease", Json::Null),
        ("input_inspection", Json::text("unrecorded")),
    ]);
    assert!(canonical_inspected_facts(&base).is_ok());
    for value in ["verified", "unavailable", "running", "", "unrecorded "] {
        assert!(canonical_inspected_facts(
            &Json::parse(&base.encode().replace("unrecorded", value)).unwrap()
        )
        .is_err());
    }
    assert!(canonical_inspected_facts(
        &Json::parse(&base.encode().replacen('{', "{\"extra\":true,", 1)).unwrap()
    )
    .is_err());
    assert!(Json::parse(
        &base
            .encode()
            .replacen('{', "{\"input_inspection\":\"unrecorded\",", 1)
    )
    .is_err());
}
