use super::*;
use crate::fleet::remote_admission::authentication::tests::Fixture;
use crate::fleet::{RemoteAdmissionOutcome, RemoteFrameReader, RemoteFrameWriter};
use ed25519_dalek::{Signer as _, SigningKey};

fn public(key: &SigningKey) -> PublicKey {
    PublicKey::from_bytes(key.verifying_key().to_bytes())
}
fn sign(key: &SigningKey, payload: &SigningPayload) -> Result<Signature, String> {
    Ok(Signature::from_bytes(
        key.sign(payload.as_bytes()).to_bytes(),
    ))
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
fn challenge(f: &Fixture, runtime: &mut Runtime) -> RemotePeerChallenge {
    RemotePeerChallenge::issue(
        runtime,
        "lane",
        "run",
        f.work.assignment.clone(),
        public(&f.worker),
    )
    .unwrap()
}
fn signed(f: &Fixture, challenge: &RemotePeerChallenge, runtime: &mut Runtime) -> RemoteDispatch {
    challenge
        .signed_dispatch(runtime, &public(&f.coordinator), |body| {
            sign(&f.coordinator, body)
        })
        .unwrap()
}
fn wire(frame: RemoteFrame) -> String {
    let mut bytes = Vec::new();
    RemoteFrameWriter::new(&mut bytes)
        .write_frame(&frame)
        .unwrap();
    let Some(RemoteFrame::Control(bytes)) = RemoteFrameReader::new(bytes.as_slice())
        .read_frame()
        .unwrap()
    else {
        panic!("control expected")
    };
    String::from_utf8(bytes).unwrap()
}
fn set(value: &mut Json, name: &str, replacement: Json) {
    let Json::Object(fields) = value else {
        panic!("object expected")
    };
    fields.iter_mut().find(|(key, _)| key == name).unwrap().1 = replacement;
}
fn resign(f: &Fixture, dispatch: &mut RemoteDispatch) {
    dispatch.signature = sign(&f.coordinator, &dispatch.payload()).unwrap();
}

#[test]
fn authenticated_dispatch_roundtrip_proves_both_keys_then_uses_existing_receiving_admission() {
    let f = Fixture::new();
    let mut runtime = f.runtime(false);
    let challenge = challenge(&f, &mut runtime);
    let dispatch = signed(&f, &challenge, &mut runtime);
    let decoded = RemoteDispatch::decode(&wire(dispatch.frame().unwrap())).unwrap();
    let verified = decoded.verify(&policy(&f)).unwrap();
    assert_eq!(verified.objective(), "objective");
    assert_eq!(verified.coordinator(), key(&public(&f.coordinator)));
    assert!(verified.work() == &f.work);
    assert_eq!(verified.limits(), &policy(&f).maximum);
    assert_eq!(
        verified.payload.as_bytes(),
        challenge.signing_payload().as_bytes()
    );
    // A verified dispatch and even a repeated worker reply still create no receiving reservation.
    let reply = wire(verified.worker_reply(|body| sign(&f.worker, body)).unwrap());
    assert_eq!(
        reply,
        wire(verified.worker_reply(|body| sign(&f.worker, body)).unwrap())
    );
    assert!(f.registry().receipts().unwrap().is_empty());
    let before = runtime.state().revision;
    challenge
        .verify_dispatch_reply(&mut runtime, "bootstrap-worker", &reply)
        .unwrap();
    assert_eq!(runtime.state().revision, before + 1);
    assert!(RemotePeerChallenge::issue(
        &mut runtime,
        "lane",
        "run",
        f.work.assignment.clone(),
        public(&f.worker)
    )
    .is_err());
    assert!(f.registry().receipts().unwrap().is_empty());
    let receiving = f
        .registry()
        .challenge(verified.work().clone(), "0123456789abcdef0123456789abcdef")
        .unwrap();
    let payload = receiving
        .proof()
        .signing_payload_for(
            &mut runtime,
            "lane",
            "run",
            &public(&f.coordinator),
            &public(&f.worker),
        )
        .unwrap();
    let (_, outcome) = receiving
        .verify_and_reserve(&sign(&f.coordinator, &payload).unwrap())
        .unwrap();
    assert!(matches!(outcome, RemoteAdmissionOutcome::Reserved(_)));
    assert_eq!(f.registry().receipts().unwrap().len(), 1);
}

#[test]
fn configured_keys_provider_budget_and_lease_caps_refuse_before_worker_signing() {
    let f = Fixture::new();
    let mut runtime = f.runtime(false);
    let challenge = challenge(&f, &mut runtime);
    let dispatch = signed(&f, &challenge, &mut runtime);
    for variant in 0..6 {
        let mut configured = policy(&f);
        match variant {
            0 => configured.coordinator = public(&SigningKey::from_bytes(&[41; 32])),
            1 => configured.worker = public(&SigningKey::from_bytes(&[42; 32])),
            2 => configured.provider = "claude",
            3 => configured.maximum.lanes = 1,
            4 => configured.maximum.depth = 0,
            _ => configured.max_lease_ms = 1,
        }
        assert!(dispatch.verify(&configured).is_err());
    }
    let mut wrong_domain = RemoteDispatch::decode(&dispatch.encode()).unwrap();
    wrong_domain.signature = sign(&f.coordinator, challenge.signing_payload()).unwrap();
    assert!(wrong_domain.verify(&policy(&f)).is_err());
    assert!(challenge
        .signed_dispatch(&mut runtime, &public(&f.coordinator), |body| sign(
            &f.worker, body
        ))
        .is_err());
    let verified = dispatch.verify(&policy(&f)).unwrap();
    assert!(verified
        .worker_reply(|body| sign(&f.coordinator, body))
        .is_err());
    assert!(f.registry().receipts().unwrap().is_empty());
}

#[test]
fn closed_canonical_dispatch_refuses_unsigned_tampering_and_signed_unknown_or_wrong_commands() {
    let f = Fixture::new();
    let mut runtime = f.runtime(false);
    let challenge = challenge(&f, &mut runtime);
    let original = signed(&f, &challenge, &mut runtime).encode();
    for bad in [
        format!(" {original}"),
        original.replace("mesh.remote-dispatch/v1", "mesh.remote-dispatch/v2"),
        original.replacen('{', "{\"unknown\":0,", 1),
        original.replace("\"signature\":", "\"signature\":null,\"old\":"),
        " ".repeat(MAX_BYTES + 1),
    ] {
        assert!(RemoteDispatch::decode(&bad).is_err());
    }
    let mut tampered = RemoteDispatch::decode(&original).unwrap();
    set(&mut tampered.body, "worker", Json::text("00".repeat(32)));
    assert!(tampered.verify(&policy(&f)).is_err());
    for variant in 0..3 {
        let mut altered = RemoteDispatch::decode(&original).unwrap();
        let mut body = altered.body.get("challenge").unwrap().clone();
        match variant {
            0 => {
                let Json::Object(fields) = &mut body else {
                    unreachable!()
                };
                fields.push(("unknown".into(), Json::Null));
            }
            1 => set(
                &mut body,
                "claim",
                Json::text(wire::encode(&Command::Cancel)),
            ),
            _ => set(&mut body, "provider", Json::text("claude")),
        }
        set(&mut altered.body, "challenge", body);
        resign(&f, &mut altered);
        assert!(altered.verify(&policy(&f)).is_err());
    }
    assert!(f.registry().receipts().unwrap().is_empty());
}

#[test]
fn wrong_or_noncanonical_worker_reply_consumes_challenge_without_claiming() {
    for variant in 0..4 {
        let f = Fixture::new();
        let mut runtime = f.runtime(false);
        let challenge = challenge(&f, &mut runtime);
        let dispatch = signed(&f, &challenge, &mut runtime);
        let verified = dispatch.verify(&policy(&f)).unwrap();
        let original = wire(verified.worker_reply(|body| sign(&f.worker, body)).unwrap());
        let changed = match variant {
            0 => format!(" {original}"),
            1 => original.replace(&verified.nonce.to_string(), &"ff".repeat(32)),
            2 => original.replace("/v1", "/v2"),
            _ => reply(
                verified.nonce,
                &sign(&f.coordinator, &verified.payload).unwrap(),
            )
            .encode(),
        };
        let before = runtime.state().revision;
        assert!(challenge
            .verify_dispatch_reply(&mut runtime, "bad-proof", &changed)
            .is_err());
        assert_eq!(runtime.state().revision, before);
        assert!(runtime.state().lanes["lane"].runs[0].launch_owner.is_none());
    }
}

#[test]
fn native_cancellation_during_coordinator_signing_refuses_dispatch() {
    let f = Fixture::new();
    let mut runtime = f.runtime(false);
    let challenge = challenge(&f, &mut runtime);
    assert!(challenge
        .signed_dispatch(&mut runtime, &public(&f.coordinator), |body| {
            let mut other = Runtime::open(
                mesh_store::fleet::FleetStore::open(f.path.join("coordinator.sqlite")).unwrap(),
                "objective",
            )
            .unwrap();
            other.record("cancel-bootstrap", Command::Cancel).unwrap();
            sign(&f.coordinator, body)
        })
        .is_err());
    assert!(challenge
        .signed_dispatch(&mut runtime, &public(&f.coordinator), |_| panic!(
            "stale context reached signer"
        ))
        .is_err());
    assert!(f.registry().receipts().unwrap().is_empty());
}

#[test]
fn expired_dispatch_and_worker_reply_never_reach_signer_or_create_admission() {
    let f = Fixture::new();
    let mut runtime = f.runtime(false);
    let challenge = challenge(&f, &mut runtime);
    let dispatch = signed(&f, &challenge, &mut runtime);
    assert!(dispatch
        .verify_at(&policy(&f), challenge.issued_ms - 1)
        .is_err());
    assert!(dispatch
        .verify_at(&policy(&f), challenge.expires_ms)
        .is_err());
    let mut verified = dispatch.verify(&policy(&f)).unwrap();
    verified.expires_ms = now_ms().unwrap();
    assert!(verified
        .worker_reply(|_| panic!("expired proof reached signer"))
        .is_err());
    assert!(f.registry().receipts().unwrap().is_empty());
}
