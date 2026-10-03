use super::*;
use crate::fleet::receiving_session::tests::{
    recovery::{guarded, materialized},
    Setup,
};
use crate::fleet::Command;
use ed25519_dalek::{Signer as _, SigningKey};

fn public(k: &SigningKey) -> PublicKey {
    PublicKey::from_bytes(k.verifying_key().to_bytes())
}
fn sign(k: &SigningKey, p: &SigningPayload) -> Result<Signature, String> {
    Ok(Signature::from_bytes(k.sign(p.as_bytes()).to_bytes()))
}
fn prepared() -> (Setup, Runtime, RemoteAdmissionRegistry) {
    let s = Setup::new();
    let mut runtime = s.f.runtime(true);
    let (input, registry) = materialized(&s, &mut runtime);
    drop(input);
    (s, runtime, registry)
}
fn signed(s: &Setup, runtime: &mut Runtime, proof: &RemoteRecoveryProof) -> Signature {
    RemoteRecoveryProof::decode(&proof.encode())
        .unwrap()
        .sign_for(
            runtime,
            "lane",
            "run",
            &public(&s.f.coordinator),
            &public(&s.f.worker),
            |p| sign(&s.f.coordinator, p),
        )
        .unwrap()
}
fn renew(s: &Setup, runtime: &mut Runtime, registry: &mut RemoteAdmissionRegistry) {
    let admission = registry.receipts().unwrap().remove(0);
    let current = registry.effective_lease(&admission).unwrap();
    registry
        .renew_lease(
            &admission,
            current.sequence,
            current.until_ms + 30_000,
            now().unwrap(),
            240_000,
        )
        .unwrap();
    runtime
        .record(
            &format!("renew-{}", current.sequence),
            Command::AdvanceRemoteLease {
                lane: "lane".into(),
                run: "run".into(),
                assignment: "assignment".into(),
                worker_key: key(&public(&s.f.worker)),
                expected_sequence: current.sequence,
                lease_until_ms: current.until_ms + 30_000,
            },
        )
        .unwrap();
}

#[test]
fn renewed_recovery_keeps_original_admission_and_initial_history() {
    let (s, mut runtime, mut registry) = prepared();
    let admission = registry.receipts().unwrap().remove(0);
    renew(&s, &mut runtime, &mut registry);
    let challenge = registry.recovery_challenge("assignment").unwrap();
    let proof = challenge.proof();
    assert_eq!(number(&proof.admission, "lease_sequence").unwrap(), 1);
    assert_eq!(proof.lease.sequence, 2);
    let signature = signed(&s, &mut runtime, proof);
    let (workspace, registry) = challenge
        .recover(
            &signature,
            &s.destination,
            TrustedReviewers::default(),
            CheckpointRuntimeParameters::selected_defaults(),
        )
        .unwrap();
    workspace.verify().unwrap();
    assert!(workspace.admission() == &admission);
    assert!(registry.receipts().unwrap() == vec![admission]);
    assert!(registry.launch_receipt("assignment").unwrap().is_none());
    let initial = workspace.binding().starting_version();
    let receipt = workspace.receipt().encode();
    drop((workspace, registry));
    let challenge = guarded(&s).recovery_challenge("assignment").unwrap();
    let signature = signed(&s, &mut runtime, challenge.proof());
    let (again, _) = challenge
        .recover(
            &signature,
            &s.destination,
            TrustedReviewers::default(),
            CheckpointRuntimeParameters::selected_defaults(),
        )
        .unwrap();
    assert_eq!(again.binding().starting_version(), initial);
    assert_eq!(again.receipt().encode(), receipt);
}

#[test]
fn current_lease_authenticates_after_initial_deadline_without_changing_initial_admission() {
    let (s, mut runtime, mut registry) = prepared();
    renew(&s, &mut runtime, &mut registry);
    let time = s.f.work.assignment.lease_until_ms + 1;
    let challenge = registry
        .recovery_challenge_at("assignment", time, [21; 32])
        .unwrap();
    let proof = RemoteRecoveryProof::decode(&challenge.proof().encode()).unwrap();
    proof
        .context(
            &mut runtime,
            "lane",
            "run",
            &public(&s.f.coordinator),
            &public(&s.f.worker),
            time,
        )
        .unwrap();
    let signature = sign(&s.f.coordinator, &proof.payload()).unwrap();
    challenge.authenticate(&signature, time).unwrap();
    assert!(challenge
        .authenticate(&signature, proof.expires_ms)
        .is_err());
    assert!(challenge.authenticate(&signature, time - 1).is_err());
    assert_eq!(
        number(&proof.admission, "lease_until_ms").unwrap(),
        s.f.work.assignment.lease_until_ms
    );
}

#[test]
fn nonce_replay_wrong_signer_and_changed_lease_refuse_before_recovery() {
    let (s, mut runtime, registry) = prepared();
    let challenge = registry.recovery_challenge("assignment").unwrap();
    let signature = signed(&s, &mut runtime, challenge.proof());
    let other = guarded(&s).recovery_challenge("assignment").unwrap();
    assert!(other.authenticate(&signature, now().unwrap()).is_err());
    assert!(challenge
        .authenticate(
            &sign(&s.f.worker, &challenge.proof.payload()).unwrap(),
            now().unwrap()
        )
        .is_err());
    let mut update = guarded(&s);
    renew(&s, &mut runtime, &mut update);
    assert!(challenge
        .recover(
            &signature,
            &s.destination,
            TrustedReviewers::default(),
            CheckpointRuntimeParameters::selected_defaults()
        )
        .is_err());
    let root =
        s.f.path
            .join("allocations")
            .join("input-0123456789abcdef0123456789abcdef");
    assert!(!root.join("initialization.json").exists());
    assert!(!root.join("workspace.mesh").exists());
}

#[test]
fn cancellation_during_signing_discards_the_signature() {
    let (s, mut runtime, registry) = prepared();
    let challenge = registry.recovery_challenge("assignment").unwrap();
    let mut other = Runtime::open(
        FleetStore::open(s.f.path.join("coordinator.sqlite")).unwrap(),
        "objective",
    )
    .unwrap();
    let result = challenge.proof().sign_for(
        &mut runtime,
        "lane",
        "run",
        &public(&s.f.coordinator),
        &public(&s.f.worker),
        |p| {
            other.record("cancel-during-sign", Command::Cancel).unwrap();
            sign(&s.f.coordinator, p)
        },
    );
    assert!(result.is_err());
    assert!(challenge
        .proof()
        .sign_for(
            &mut runtime,
            "lane",
            "run",
            &public(&s.f.coordinator),
            &public(&s.f.worker),
            |_| panic!("cancelled work must never reach signer")
        )
        .is_err());
}

#[test]
fn bounded_canonical_proof_and_exact_native_scope_are_required() {
    let (s, mut runtime, registry) = prepared();
    let challenge = registry.recovery_challenge("assignment").unwrap();
    let wire = challenge.proof().encode();
    for bad in [
        format!(" {wire}"),
        format!("{wire} "),
        wire.replace("challenge/v1", "challenge/v2"),
        wire.replacen("\"admission_revision\":1", "\"admission_revision\":0", 1),
        " ".repeat(MAX_BYTES + 1),
        wire.replacen("\"parent\":\"", "\"parent\":\"bad", 1),
    ] {
        assert!(RemoteRecoveryProof::decode(&bad).is_err());
    }
    let mut changed = challenge.proof().clone();
    changed.admission = Json::parse(
        &changed
            .admission
            .encode()
            .replace("Private assigned task", "Different private task"),
    )
    .unwrap();
    assert!(changed
        .sign_for(
            &mut runtime,
            "lane",
            "run",
            &public(&s.f.coordinator),
            &public(&s.f.worker),
            |_| panic!("changed task must not be signed")
        )
        .is_err());
    assert!(challenge
        .proof()
        .sign_for(
            &mut runtime,
            "lane",
            "other-run",
            &public(&s.f.coordinator),
            &public(&s.f.worker),
            |_| panic!("wrong run must not be signed")
        )
        .is_err());
    assert!(s.f.registry().recovery_challenge("assignment").is_err());
}

#[test]
fn lease_change_during_signing_discards_the_signature() {
    let (s, mut runtime, registry) = prepared();
    let challenge = registry.recovery_challenge("assignment").unwrap();
    let mut other = Runtime::open(
        FleetStore::open(s.f.path.join("coordinator.sqlite")).unwrap(),
        "objective",
    )
    .unwrap();
    let mut update = guarded(&s);
    let result = challenge.proof().sign_for(
        &mut runtime,
        "lane",
        "run",
        &public(&s.f.coordinator),
        &public(&s.f.worker),
        |p| {
            renew(&s, &mut other, &mut update);
            sign(&s.f.coordinator, p)
        },
    );
    assert!(result.is_err());
    assert!(challenge.check(now().unwrap()).is_err());
}
