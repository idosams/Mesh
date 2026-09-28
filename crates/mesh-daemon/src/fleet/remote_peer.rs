//! Single-use proof of a configured worker key for one pending remote assignment.
//! Optional signed dispatch authenticates coordinator intent before the worker proves its key.
//! Neither direction transfers input or launches a provider.
use super::{refuse, wire, Command, Error, Lane, RemoteAssignment, RunState, Runtime};
use crate::ipc::Json;
use mesh_crypto::{DomainSeparator, Ed25519, SignatureScheme, SigningPayload};
use mesh_store::{fleet::FleetEvent, RecordDigest};
use mesh_types::{PublicKey, Signature};
use std::io::Read;
use std::time::{SystemTime, UNIX_EPOCH};

const DOMAIN: DomainSeparator = DomainSeparator::new("mesh.v1.fleet-worker-assignment-proof");
const LIFETIME_MS: u64 = 30_000;
mod dispatch;
pub use dispatch::{RemoteDispatch, RemoteDispatchPolicy, VerifiedRemoteDispatch};

/// Native-created, non-cloneable challenge. Consuming a reply consumes its nonce even on refusal.
/// The caller supplies an independently configured worker key, never a key chosen by the reply.
/// Dropping or losing this object requires a fresh challenge; no session is adopted after restart.
pub struct RemotePeerChallenge {
    objective: String,
    lane: Lane,
    command: Command,
    peer: PublicKey,
    payload: SigningPayload,
    body: Json,
    issued_ms: u64,
    expires_ms: u64,
}
impl RemotePeerChallenge {
    /// Issue a fresh OS-random challenge for an allocated, dispatched, still-unclaimed attempt.
    /// Native code must verify/admit the immutable transfer bundle independently of this proof.
    pub fn issue(
        runtime: &mut Runtime,
        lane: &str,
        run: &str,
        assignment: RemoteAssignment,
        configured_peer: PublicKey,
    ) -> Result<Self, Error> {
        let mut nonce = [0; 32];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut source| source.read_exact(&mut nonce))
            .map_err(|_| Error::Refused("remote-peer-entropy-unavailable"))?;
        Self::build(
            runtime,
            lane,
            run,
            assignment,
            configured_peer,
            now_ms()?,
            nonce,
        )
    }
    fn build(
        runtime: &mut Runtime,
        lane: &str,
        run: &str,
        assignment: RemoteAssignment,
        configured_peer: PublicKey,
        issued_ms: u64,
        nonce: [u8; 32],
    ) -> Result<Self, Error> {
        runtime.refresh()?;
        assignment.validate()?;
        let key = RecordDigest::parse_hex(&assignment.worker_key)
            .map_err(|_| Error::Refused("remote-peer-key-mismatch"))?;
        if key.as_bytes() != configured_peer.as_bytes() {
            return refuse("remote-peer-key-mismatch");
        }
        if assignment.lease_until_ms <= issued_ms {
            return refuse("remote-peer-expired");
        }
        if runtime.state().cancelled {
            return refuse("objective-cancelled");
        }
        let selected = runtime
            .state()
            .lanes
            .get(lane)
            .ok_or(Error::Refused("lane-missing"))?;
        if selected.base != assignment.input {
            return refuse("remote-input-mismatch");
        }
        let current = selected
            .runs
            .last()
            .ok_or(Error::Refused("remote-run-missing"))?;
        if current.id != run
            || current.state != RunState::Launching
            || current.launch_owner.is_some()
        {
            return refuse("launch-needs-reconciliation");
        }
        let expires_ms = issued_ms
            .checked_add(LIFETIME_MS)
            .ok_or(Error::Refused("remote-peer-clock"))?
            .min(assignment.lease_until_ms);
        let command = Command::ClaimRemoteLaunch {
            lane: lane.into(),
            run: run.into(),
            assignment,
        };
        let body = Json::object([
            ("schema", Json::text("mesh.fleet-worker-challenge/v1")),
            ("objective", Json::text(runtime.objective())),
            ("claim", Json::text(wire::encode(&command))),
            ("provider", Json::text(&selected.provider)),
            ("goal", Json::text(&selected.goal)),
            (
                "nonce",
                Json::text(RecordDigest::from_bytes(nonce).to_string()),
            ),
            ("issued_ms", Json::Number(issued_ms)),
            ("expires_ms", Json::Number(expires_ms)),
        ]);
        Ok(Self {
            objective: runtime.objective().into(),
            lane: selected.clone(),
            command,
            peer: configured_peer,
            payload: SigningPayload::new(DOMAIN, body.encode().as_bytes()),
            body,
            issued_ms,
            expires_ms,
        })
    }
    /// Exact domain-framed bytes for the assigned worker to sign; do not log the task-bearing body.
    pub fn signing_payload(&self) -> &SigningPayload {
        &self.payload
    }

    /// Verify the configured peer, freshness and current pending context, then durably claim once.
    /// Success reserves ownership only. It grants no process spawn, output trust or main approval.
    pub fn verify_and_claim(
        self,
        runtime: &mut Runtime,
        request: &str,
        signature: &Signature,
    ) -> Result<FleetEvent, Error> {
        self.verify_at(runtime, request, signature, now_ms()?)
    }
    fn verify_at(
        self,
        runtime: &mut Runtime,
        request: &str,
        signature: &Signature,
        now: u64,
    ) -> Result<FleetEvent, Error> {
        if now < self.issued_ms || now >= self.expires_ms {
            return refuse("remote-peer-expired");
        }
        Ed25519::verify(&self.peer, self.payload.as_bytes(), signature)
            .map_err(|_| Error::Refused("remote-peer-signature"))?;
        runtime.refresh()?;
        if runtime.objective() != self.objective
            || runtime.state().cancelled
            || runtime.state().lanes.get(&self.lane.id) != Some(&self.lane)
        {
            return refuse("remote-peer-context-changed");
        }
        // submit's revision check also closes a competing writer between refresh and commit.
        runtime.submit(runtime.state().revision, request, self.command)
    }
}
fn now_ms() -> Result<u64, Error> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .ok_or(Error::Refused("remote-peer-clock"))
}

#[cfg(test)]
mod tests {
    use super::super::{Limits, WorkspaceBinding};
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use mesh_store::fleet::FleetStore;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture {
        path: std::path::PathBuf,
        runtime: Runtime,
    }
    impl Fixture {
        fn new(objective: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "mesh-remote-peer-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            let mut value = Self {
                runtime: Runtime::open(
                    FleetStore::open(path.join("fleet.sqlite")).unwrap(),
                    objective,
                )
                .unwrap(),
                path,
            };
            value.command(Command::Start {
                goal: "Remote work".into(),
                limits: Limits {
                    lanes: 2,
                    concurrency: 1,
                    depth: 0,
                    retries: 0,
                },
            });
            value.command(Command::CreateLane {
                id: "lane".into(),
                parent: None,
                goal: "Exact task".into(),
                provider: "codex".into(),
                base: RecordDigest::from_bytes([1; 32]),
            });
            value.command(Command::BindWorkspace {
                lane: "lane".into(),
                binding: WorkspaceBinding {
                    source_version: RecordDigest::from_bytes([1; 32]),
                    starting_version: None,
                    root: "/native/verified".into(),
                    digest: "digest".into(),
                    installation: "installation".into(),
                },
            });
            value.command(Command::Dispatch {
                lane: "lane".into(),
                run: "run".into(),
            });
            value
        }
        fn command(&mut self, command: Command) {
            let revision = self.runtime.state().revision;
            self.runtime
                .submit(revision, &format!("request-{revision}"), command)
                .unwrap();
        }
        fn challenge(&mut self, nonce: u8) -> RemotePeerChallenge {
            RemotePeerChallenge::build(
                &mut self.runtime,
                "lane",
                "run",
                assignment(),
                peer(),
                100,
                [nonce; 32],
            )
            .unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
    fn signer() -> SigningKey {
        SigningKey::from_bytes(&[43; 32])
    }
    fn peer() -> PublicKey {
        PublicKey::from_bytes(signer().verifying_key().to_bytes())
    }
    fn assignment() -> RemoteAssignment {
        RemoteAssignment {
            id: "assignment".into(),
            worker_key: RecordDigest::from_bytes(*peer().as_bytes()).to_string(),
            input: RecordDigest::from_bytes([1; 32]),
            bundle: RecordDigest::from_bytes([2; 32]),
            lease_sequence: 1,
            lease_until_ms: 40_000,
        }
    }
    fn signature(challenge: &RemotePeerChallenge) -> Signature {
        Signature::from_bytes(
            signer()
                .sign(challenge.signing_payload().as_bytes())
                .to_bytes(),
        )
    }
    #[test]
    fn verified_peer_claim_is_durable_but_never_starts_or_regrants_execution() {
        let mut f = Fixture::new("objective");
        let challenge = f.challenge(1);
        let signed = signature(&challenge);
        let event = challenge
            .verify_at(&mut f.runtime, "peer-proof", &signed, 101)
            .unwrap();
        let mut reopened = Runtime::open(
            FleetStore::open(f.path.join("fleet.sqlite")).unwrap(),
            "objective",
        )
        .unwrap();
        assert_eq!(reopened.state().revision, event.revision);
        let run = &reopened.state().lanes["lane"].runs[0];
        assert_eq!(run.state, RunState::Launching);
        assert_eq!(run.remote.as_ref(), Some(&assignment()));
        assert!(RemotePeerChallenge::build(
            &mut reopened,
            "lane",
            "run",
            assignment(),
            peer(),
            102,
            [2; 32]
        )
        .is_err());
        assert_eq!(reopened.state().lanes["lane"].runs.len(), 1);
    }
    #[test]
    fn signatures_cannot_cross_nonce_key_domain_or_bundle_and_fail_without_writes() {
        let mut f = Fixture::new("objective");
        let original = f.challenge(1);
        let signed = signature(&original);
        let before = f.runtime.state().clone();
        assert!(f
            .challenge(2)
            .verify_at(&mut f.runtime, "nonce", &signed, 101)
            .is_err());
        let challenge = f.challenge(3);
        let wrong = Signature::from_bytes(
            SigningKey::from_bytes(&[44; 32])
                .sign(challenge.signing_payload().as_bytes())
                .to_bytes(),
        );
        assert!(challenge
            .verify_at(&mut f.runtime, "key", &wrong, 101)
            .is_err());
        let challenge = f.challenge(4);
        let wrong_domain = SigningPayload::new(
            DomainSeparator::CAPABILITY_TOKEN,
            challenge.signing_payload().as_bytes(),
        );
        let wrong = Signature::from_bytes(signer().sign(wrong_domain.as_bytes()).to_bytes());
        assert!(challenge
            .verify_at(&mut f.runtime, "domain", &wrong, 101)
            .is_err());
        let mut changed = assignment();
        changed.bundle = RecordDigest::from_bytes([3; 32]);
        let challenge = RemotePeerChallenge::build(
            &mut f.runtime,
            "lane",
            "run",
            changed,
            peer(),
            100,
            [1; 32],
        )
        .unwrap();
        assert!(challenge
            .verify_at(&mut f.runtime, "bundle", &signed, 101)
            .is_err());
        assert_eq!(f.runtime.state(), &before);
    }
    #[test]
    fn stale_clock_expired_proof_wrong_objective_and_changed_pending_run_refuse() {
        let mut f = Fixture::new("objective");
        for now in [99, 30_100, 40_000] {
            let challenge = f.challenge(1);
            let signed = signature(&challenge);
            let before = f.runtime.state().clone();
            assert!(challenge
                .verify_at(&mut f.runtime, "expired", &signed, now)
                .is_err());
            assert_eq!(f.runtime.state(), &before);
        }
        let challenge = f.challenge(1);
        let signed = signature(&challenge);
        let mut other = Fixture::new("other");
        assert!(challenge
            .verify_at(&mut other.runtime, "other", &signed, 101)
            .is_err());
        let challenge = f.challenge(1);
        let signed = signature(&challenge);
        f.command(Command::Observe {
            lane: "lane".into(),
            run: "run".into(),
            state: RunState::Running,
        });
        let before = f.runtime.state().clone();
        assert!(challenge
            .verify_at(&mut f.runtime, "changed", &signed, 101)
            .is_err());
        assert_eq!(f.runtime.state(), &before);
    }
    #[test]
    fn concurrent_proofs_cancelled_work_expired_lease_and_wrong_pin_refuse() {
        let mut f = Fixture::new("objective");
        let first = f.challenge(1);
        let second = f.challenge(2);
        let signed_first = signature(&first);
        let signed_second = signature(&second);
        first
            .verify_at(&mut f.runtime, "first", &signed_first, 101)
            .unwrap();
        assert!(second
            .verify_at(&mut f.runtime, "second", &signed_second, 101)
            .is_err());
        let mut f = Fixture::new("objective");
        let challenge = f.challenge(1);
        let signed = signature(&challenge);
        f.command(Command::Cancel);
        let before = f.runtime.state().clone();
        assert!(challenge
            .verify_at(&mut f.runtime, "cancelled", &signed, 101)
            .is_err());
        assert_eq!(f.runtime.state(), &before);
        let mut f = Fixture::new("objective");
        assert!(RemotePeerChallenge::build(
            &mut f.runtime,
            "lane",
            "run",
            assignment(),
            PublicKey::from_bytes([0; 32]),
            100,
            [1; 32]
        )
        .is_err());
        assert!(RemotePeerChallenge::build(
            &mut f.runtime,
            "lane",
            "run",
            assignment(),
            peer(),
            40_000,
            [1; 32]
        )
        .is_err());
    }
    #[test]
    fn weak_configured_key_cannot_manufacture_a_worker_proof() {
        let mut f = Fixture::new("objective");
        let mut assignment = assignment();
        assignment.worker_key = "00".repeat(32);
        let challenge = RemotePeerChallenge::build(
            &mut f.runtime,
            "lane",
            "run",
            assignment,
            PublicKey::from_bytes([0; 32]),
            100,
            [1; 32],
        )
        .unwrap();
        let before = f.runtime.state().clone();
        assert!(challenge
            .verify_at(
                &mut f.runtime,
                "weak-key",
                &Signature::from_bytes([0; 64]),
                101
            )
            .is_err());
        assert_eq!(f.runtime.state(), &before);
    }

    #[test]
    fn production_issue_uses_fresh_nonces_and_never_exposes_native_paths() {
        let mut f = Fixture::new("objective");
        let mut assignment = assignment();
        assignment.lease_until_ms = now_ms().unwrap() + 60_000;
        let first =
            RemotePeerChallenge::issue(&mut f.runtime, "lane", "run", assignment.clone(), peer())
                .unwrap();
        let second =
            RemotePeerChallenge::issue(&mut f.runtime, "lane", "run", assignment, peer()).unwrap();
        let nonce = |challenge: &RemotePeerChallenge| {
            let bytes = challenge.signing_payload().as_bytes();
            let body = std::str::from_utf8(&bytes[16 + DOMAIN.as_str().len()..]).unwrap();
            Json::parse(body).unwrap().get("nonce").unwrap().clone()
        };
        assert_ne!(
            nonce(&first),
            nonce(&second),
            "freshness must come from OS randomness, not differing timestamps"
        );
        assert!(!String::from_utf8_lossy(first.signing_payload().as_bytes())
            .contains("/native/verified"));
        let signed = signature(&first);
        first
            .verify_and_claim(&mut f.runtime, "real-clock", &signed)
            .unwrap();
    }
}
