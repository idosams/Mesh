use super::*;
use crate::fleet::{Command, RemotePeerChallenge, WorkspaceBinding};
use ed25519_dalek::{Signer as _, SigningKey};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

const ALLOCATION: &str = "0123456789abcdef0123456789abcdef";
static NEXT: AtomicU64 = AtomicU64::new(0);
pub(in crate::fleet) struct Fixture {
    pub(in crate::fleet) path: PathBuf,
    pub(in crate::fleet) coordinator: SigningKey,
    pub(in crate::fleet) worker: SigningKey,
    pub(in crate::fleet) work: RemoteWork,
    now: u64,
}
impl Fixture {
    pub(in crate::fleet) fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "mesh-admission-auth-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        let coordinator = SigningKey::from_bytes(&[5; 32]);
        let worker = SigningKey::from_bytes(&[7; 32]);
        let now = now_ms().unwrap();
        let work = RemoteWork {
            lane: "lane".into(),
            run: "run".into(),
            provider: "codex".into(),
            goal: "Private assigned task".into(),
            assignment: crate::fleet::RemoteAssignment {
                id: "assignment".into(),
                worker_key: key(&public(&worker)),
                input: RecordDigest::from_bytes([1; 32]),
                bundle: RecordDigest::from_bytes([2; 32]),
                lease_sequence: 1,
                lease_until_ms: now + 60_000,
            },
        };
        Self {
            path,
            coordinator,
            worker,
            work,
            now,
        }
    }
    pub(in crate::fleet) fn reopen_existing(path: PathBuf) -> Self {
        assert!(path.join("worker.sqlite").is_file());
        let coordinator = SigningKey::from_bytes(&[5; 32]);
        let worker = SigningKey::from_bytes(&[7; 32]);
        let registry = RemoteAdmissionRegistry::new(
            FleetStore::open(path.join("worker.sqlite")).unwrap(),
            &key(&public(&coordinator)),
            &key(&public(&worker)),
            "objective",
            limits(),
        )
        .unwrap();
        let receipts = registry.receipts().unwrap();
        assert_eq!(receipts.len(), 1);
        Self {
            path,
            coordinator,
            worker,
            work: receipts[0].work().clone(),
            now: now_ms().unwrap(),
        }
    }
    pub(in crate::fleet) fn registry(&self) -> RemoteAdmissionRegistry {
        RemoteAdmissionRegistry::new(
            FleetStore::open(self.path.join("worker.sqlite")).unwrap(),
            &key(&public(&self.coordinator)),
            &key(&public(&self.worker)),
            "objective",
            limits(),
        )
        .unwrap()
    }
    pub(in crate::fleet) fn runtime(&self, claimed: bool) -> Runtime {
        let mut runtime = Runtime::open(
            FleetStore::open(self.path.join("coordinator.sqlite")).unwrap(),
            "objective",
        )
        .unwrap();
        runtime
            .record(
                "start",
                Command::Start {
                    goal: "Coordinate".into(),
                    limits: limits(),
                },
            )
            .unwrap();
        runtime
            .record(
                "lane",
                Command::CreateLane {
                    id: "lane".into(),
                    parent: None,
                    goal: self.work.goal.clone(),
                    provider: self.work.provider.clone(),
                    base: self.work.assignment.input,
                },
            )
            .unwrap();
        runtime
            .record(
                "bind",
                Command::BindWorkspace {
                    lane: "lane".into(),
                    binding: WorkspaceBinding {
                        source_version: self.work.assignment.input,
                        starting_version: Some(RecordDigest::from_bytes([3; 32])),
                        root: "native-root".into(),
                        digest: "native-digest".into(),
                        installation: "native-installation".into(),
                    },
                },
            )
            .unwrap();
        runtime
            .record(
                "dispatch",
                Command::Dispatch {
                    lane: "lane".into(),
                    run: "run".into(),
                },
            )
            .unwrap();
        if claimed {
            let proof = RemotePeerChallenge::issue(
                &mut runtime,
                "lane",
                "run",
                self.work.assignment.clone(),
                public(&self.worker),
            )
            .unwrap();
            let signature = sign(&self.worker, proof.signing_payload());
            proof
                .verify_and_claim(&mut runtime, "worker-proof", &signature)
                .unwrap();
        }
        runtime
    }
    fn challenge(&self, nonce: u8) -> RemoteAdmissionChallenge {
        self.registry()
            .challenge_at(self.work.clone(), ALLOCATION, self.now, [nonce; 32])
            .unwrap()
    }
    fn signature(&self, proof: &RemoteAdmissionProof, runtime: &mut Runtime) -> Signature {
        sign(
            &self.coordinator,
            &proof
                .payload_for_at(
                    runtime,
                    "lane",
                    "run",
                    &public(&self.coordinator),
                    &public(&self.worker),
                    self.now,
                )
                .unwrap(),
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
fn public(key: &SigningKey) -> PublicKey {
    PublicKey::from_bytes(key.verifying_key().to_bytes())
}
fn sign(key: &SigningKey, payload: &SigningPayload) -> Signature {
    Signature::from_bytes(key.sign(payload.as_bytes()).to_bytes())
}
fn limits() -> Limits {
    Limits {
        lanes: 2,
        concurrency: 1,
        depth: 1,
        retries: 1,
    }
}

#[test]
fn both_configured_identities_are_proven_before_original_receiving_admission() {
    let f = Fixture::new();
    let mut runtime = f.runtime(true);
    let before = runtime.state().revision;
    let challenge = f.registry().challenge(f.work.clone(), ALLOCATION).unwrap();
    assert!(f.registry().receipts().unwrap().is_empty());
    let wire = challenge.proof().encode();
    let received = RemoteAdmissionProof::decode(&wire).unwrap();
    let payload = received
        .signing_payload_for(
            &mut runtime,
            "lane",
            "run",
            &public(&f.coordinator),
            &public(&f.worker),
        )
        .unwrap();
    let (registry, outcome) = challenge
        .verify_and_reserve(&sign(&f.coordinator, &payload))
        .unwrap();
    let RemoteAdmissionOutcome::Reserved(reservation) = outcome else {
        panic!("original proof must reserve once")
    };
    assert!(reservation.receipt().work() == &f.work);
    assert_eq!(registry.receipts().unwrap().len(), 1);
    assert_eq!(runtime.state().revision, before);
}

#[test]
fn wrong_signer_and_signature_for_another_nonce_cannot_admit() {
    let f = Fixture::new();
    let mut runtime = f.runtime(true);
    let first = f.challenge(1);
    let valid = f.signature(first.proof(), &mut runtime);
    let wrong = sign(&f.worker, &first.proof().payload());
    assert!(first.verify_at(&wrong, f.now).is_err());
    assert!(f.challenge(2).verify_at(&valid, f.now).is_err());
    let other_domain = SigningPayload::new(
        DomainSeparator::new("mesh.v1.fleet-worker-assignment-proof"),
        f.challenge(3).proof().encode().as_bytes(),
    );
    assert!(f
        .challenge(3)
        .verify_at(&sign(&f.coordinator, &other_domain), f.now)
        .is_err());
    assert!(f.registry().receipts().unwrap().is_empty());
}

#[test]
fn fresh_valid_proof_after_lost_receipt_returns_facts_without_another_reservation() {
    let f = Fixture::new();
    let mut runtime = f.runtime(true);
    let challenge = f.challenge(1);
    let signature = f.signature(challenge.proof(), &mut runtime);
    let (registry, outcome) = challenge.verify_at(&signature, f.now).unwrap();
    assert!(matches!(outcome, RemoteAdmissionOutcome::Reserved(_)));
    drop(registry);
    let retry = f.challenge(2);
    let signature = f.signature(retry.proof(), &mut runtime);
    let (_, outcome) = retry.verify_at(&signature, f.now).unwrap();
    assert!(matches!(outcome, RemoteAdmissionOutcome::Retained(_)));
    assert_eq!(f.registry().receipts().unwrap().len(), 1);
}

#[test]
fn coordinator_refuses_peer_selected_context_or_unknown_admission_fields() {
    let f = Fixture::new();
    let mut runtime = f.runtime(true);
    let challenge = f.challenge(1);
    for (field, replacement) in [
        ("coordinator", Json::text("11".repeat(32))),
        ("worker", Json::text("11".repeat(32))),
        ("objective", Json::text("other")),
        ("lane", Json::text("other")),
        ("run", Json::text("other")),
        ("goal", Json::text("Peer-selected task")),
        ("provider", Json::text("claude")),
        ("assignment", Json::text("other")),
        ("input", Json::text("11".repeat(32))),
        ("bundle", Json::text("11".repeat(32))),
        ("concurrency", Json::Number(2)),
        ("lease_sequence", Json::Number(2)),
        ("allocation", Json::text("../escape")),
    ] {
        let mut proof = challenge.proof().clone();
        let Json::Object(fields) = &mut proof.admission else {
            unreachable!()
        };
        fields.iter_mut().find(|(key, _)| key == field).unwrap().1 = replacement;
        let proof = RemoteAdmissionProof::decode(&proof.encode()).unwrap();
        assert!(
            proof
                .payload_for_at(
                    &mut runtime,
                    "lane",
                    "run",
                    &public(&f.coordinator),
                    &public(&f.worker),
                    f.now
                )
                .is_err(),
            "changed {field}"
        );
    }
    let mut proof = challenge.proof().clone();
    let Json::Object(fields) = &mut proof.admission else {
        unreachable!()
    };
    fields.push(("unknown".into(), Json::Bool(true)));
    assert!(proof
        .payload_for_at(
            &mut runtime,
            "lane",
            "run",
            &public(&f.coordinator),
            &public(&f.worker),
            f.now
        )
        .is_err());
    assert!(f.registry().receipts().unwrap().is_empty());
}

#[test]
fn canonical_bounds_and_expiry_refuse_without_mutating_history() {
    let f = Fixture::new();
    let mut runtime = f.runtime(true);
    let proof = f.challenge(1).proof().clone();
    let wire = proof.encode();
    assert!(RemoteAdmissionProof::decode(&format!(" {wire}")).is_err());
    assert!(RemoteAdmissionProof::decode(&" ".repeat(MAX_PROOF_BYTES + 1)).is_err());
    assert!(RemoteAdmissionProof::decode(&wire.replacen(
        "\"schema\":",
        "\"extra\":0,\"schema\":",
        1
    ))
    .is_err());
    for now in [f.now - 1, proof.expires_ms, proof.expires_ms + 1] {
        assert!(proof
            .payload_for_at(
                &mut runtime,
                "lane",
                "run",
                &public(&f.coordinator),
                &public(&f.worker),
                now
            )
            .is_err());
        let challenge = f.challenge(1);
        let signature = f.signature(challenge.proof(), &mut runtime);
        assert!(challenge.verify_at(&signature, now).is_err());
    }
    assert!(f.registry().receipts().unwrap().is_empty());
}

#[test]
fn unclaimed_or_cancelled_native_attempt_cannot_authorize_the_proof() {
    for claimed in [false, true] {
        let f = Fixture::new();
        let mut runtime = f.runtime(claimed);
        if claimed {
            runtime.record("cancel", Command::Cancel).unwrap();
        }
        let proof = f.challenge(1);
        assert!(proof
            .proof()
            .payload_for_at(
                &mut runtime,
                "lane",
                "run",
                &public(&f.coordinator),
                &public(&f.worker),
                f.now
            )
            .is_err());
        assert!(f.registry().receipts().unwrap().is_empty());
    }
}

#[test]
fn valid_signature_cannot_overrule_concurrent_worker_capacity() {
    let f = Fixture::new();
    let mut runtime = f.runtime(true);
    let challenge = f.challenge(1);
    let signature = f.signature(challenge.proof(), &mut runtime);
    let mut other = f.work.clone();
    other.lane = "other-lane".into();
    other.run = "other-run".into();
    other.assignment.id = "other".into();
    f.registry()
        .reserve(other, &"ab".repeat(16), f.now)
        .unwrap();
    assert!(challenge.verify_at(&signature, f.now).is_err());
    let receipts = f.registry().receipts().unwrap();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].work().assignment.id, "other");
}

#[test]
fn guarded_authority_loss_refuses_even_an_authentic_proof() {
    use mesh_store::fleet::{FleetStoreAuthority, FleetStoreError};
    use std::sync::{atomic::AtomicBool, Arc};
    #[derive(Debug)]
    struct Authority(AtomicBool);
    impl FleetStoreAuthority for Authority {
        fn check(&self) -> Result<(), FleetStoreError> {
            if self.0.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err(FleetStoreError::AuthorityChanged)
            }
        }
    }
    let f = Fixture::new();
    let mut runtime = f.runtime(true);
    let authority = Arc::new(Authority(AtomicBool::new(true)));
    let path = f.path.canonicalize().unwrap().join("worker.sqlite");
    let registry = RemoteAdmissionRegistry::new(
        FleetStore::open_guarded(&path, true, authority.clone()).unwrap(),
        &key(&public(&f.coordinator)),
        &key(&public(&f.worker)),
        "objective",
        limits(),
    )
    .unwrap();
    let challenge = registry
        .challenge_at(f.work.clone(), ALLOCATION, f.now, [1; 32])
        .unwrap();
    let signature = f.signature(challenge.proof(), &mut runtime);
    authority.0.store(false, Ordering::SeqCst);
    assert!(matches!(
        challenge.verify_at(&signature, f.now),
        Err(Error::Store(FleetStoreError::AuthorityChanged))
    ));
    assert!(f.registry().receipts().unwrap().is_empty());
}
