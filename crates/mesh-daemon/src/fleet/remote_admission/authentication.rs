//! Coordinator proof before receiving admission. Native configuration, not a peer, supplies keys.
use super::*;
use crate::fleet::{RunState, Runtime};
use mesh_crypto::{DomainSeparator, Ed25519, SignatureScheme, SigningPayload};
use mesh_types::{PublicKey, Signature};
use std::io::Read as _;
use std::time::{SystemTime, UNIX_EPOCH};

const DOMAIN: DomainSeparator = DomainSeparator::new("mesh.v1.fleet-coordinator-admission-proof");
const LIFETIME_MS: u64 = 30_000;
const MAX_PROOF_BYTES: usize = 65_536;

/// Task-bearing, bounded challenge facts. Cloning/decoding grants no receiving or launch authority.
/// No Debug implementation: the task is not diagnostic output.
#[derive(Clone)]
pub struct RemoteAdmissionProof {
    admission: Json,
    nonce: RecordDigest,
    issued_ms: u64,
    expires_ms: u64,
}
impl RemoteAdmissionProof {
    /// Canonical control body. A transport must enforce the byte bound before allocating frames.
    pub fn encode(&self) -> String {
        Json::object([
            ("schema", Json::text("mesh.remote-admission-challenge/v1")),
            ("admission", self.admission.clone()),
            ("nonce", Json::text(self.nonce.to_string())),
            ("issued_ms", Json::Number(self.issued_ms)),
            ("expires_ms", Json::Number(self.expires_ms)),
        ])
        .encode()
    }

    /// Parse bounded canonical facts; the coordinator must still compare them with native state.
    pub fn decode(encoded: &str) -> Result<Self, Error> {
        if encoded.len() > MAX_PROOF_BYTES {
            return refuse("remote-admission-proof-too-large");
        }
        let value =
            Json::parse(encoded).map_err(|_| Error::Refused("remote-admission-proof-invalid"))?;
        let number = |key| {
            value
                .get(key)
                .and_then(Json::as_u64)
                .ok_or(Error::Refused("remote-admission-proof-invalid"))
        };
        let proof = Self {
            admission: value
                .get("admission")
                .cloned()
                .ok_or(Error::Refused("remote-admission-proof-invalid"))?,
            nonce: RecordDigest::parse_hex(
                value.get("nonce").and_then(Json::as_text).unwrap_or(""),
            )
            .map_err(|_| Error::Refused("remote-admission-proof-invalid"))?,
            issued_ms: number("issued_ms")?,
            expires_ms: number("expires_ms")?,
        };
        if proof.encode() != encoded
            || proof.issued_ms == 0
            || proof.expires_ms <= proof.issued_ms
            || proof.expires_ms - proof.issued_ms > LIFETIME_MS
        {
            return refuse("remote-admission-proof-invalid");
        }
        Ok(proof)
    }

    /// Derive signing bytes ONLY for the current native, already-claimed remote attempt. The
    /// configured public keys come from native policy, not the challenge. This cannot sign an
    /// arbitrary peer task and does not itself hold a private key or mutate coordinator state.
    pub fn signing_payload_for(
        &self,
        runtime: &mut Runtime,
        lane: &str,
        run: &str,
        configured_coordinator: &PublicKey,
        configured_worker: &PublicKey,
    ) -> Result<SigningPayload, Error> {
        self.payload_for_at(
            runtime,
            lane,
            run,
            configured_coordinator,
            configured_worker,
            now_ms()?,
        )
    }

    fn payload_for_at(
        &self,
        runtime: &mut Runtime,
        lane: &str,
        run: &str,
        coordinator: &PublicKey,
        worker: &PublicKey,
        now: u64,
    ) -> Result<SigningPayload, Error> {
        self.fresh(now)?;
        runtime.refresh()?;
        let state = runtime.state();
        let selected = state
            .lanes
            .get(lane)
            .ok_or(Error::Refused("lane-missing"))?;
        let attempt = selected
            .runs
            .last()
            .ok_or(Error::Refused("remote-run-missing"))?;
        let assignment = attempt
            .remote
            .as_ref()
            .ok_or(Error::Refused("remote-assignment-missing"))?;
        if state.cancelled
            || selected.workspace.is_none()
            || attempt.id != run
            || attempt.state != RunState::Launching
            || attempt.launch_owner.as_deref() != Some(format!("remote:{}", assignment.id).as_str())
            || selected.base != assignment.input
            || assignment.lease_until_ms <= now
            || assignment.worker_key != key(worker)
        {
            return refuse("remote-admission-proof-context-changed");
        }
        assignment.validate()?;
        let work = RemoteWork {
            lane: lane.into(),
            run: run.into(),
            assignment: assignment.clone(),
            provider: selected.provider.clone(),
            goal: selected.goal.clone(),
        };
        let allocation = self
            .admission
            .get("allocation")
            .and_then(Json::as_text)
            .ok_or(Error::Refused("remote-admission-proof-invalid"))?;
        if allocation.len() != 32
            || !allocation
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || self.expires_ms > assignment.lease_until_ms
        {
            return refuse("remote-admission-proof-invalid");
        }
        let expected = admission_json(
            &key(coordinator),
            &key(worker),
            runtime.objective(),
            state.limits.as_ref().ok_or(Error::InvalidHistory)?,
            &work,
            allocation,
        );
        if self.admission != expected {
            return refuse("remote-admission-proof-context-changed");
        }
        Ok(self.payload())
    }

    fn payload(&self) -> SigningPayload {
        SigningPayload::new(DOMAIN, self.encode().as_bytes())
    }
    fn fresh(&self, now: u64) -> Result<(), Error> {
        if now < self.issued_ms || now >= self.expires_ms {
            return refuse("remote-admission-proof-expired");
        }
        Ok(())
    }
}

/// Worker-held single-use challenge and guarded ledger. Lost/restarted challenges cannot be
/// reconstructed from wire facts. Failed signatures consume the challenge without reserving work.
pub struct RemoteAdmissionChallenge {
    registry: RemoteAdmissionRegistry,
    work: RemoteWork,
    allocation: String,
    proof: RemoteAdmissionProof,
    coordinator: PublicKey,
}
impl RemoteAdmissionChallenge {
    /// Task-bearing facts for the expected coordinator; not generic signing permission.
    pub fn proof(&self) -> &RemoteAdmissionProof {
        &self.proof
    }

    /// Authenticate the configured coordinator, then use the existing atomic admission path.
    /// Return the same native registry for subsequent materialization/launch composition. Even
    /// a valid second proof returns only retained facts for an already-admitted assignment.
    pub fn verify_and_reserve(
        self,
        signature: &Signature,
    ) -> Result<(RemoteAdmissionRegistry, RemoteAdmissionOutcome), Error> {
        self.verify_at(signature, now_ms()?)
    }

    fn verify_at(
        self,
        signature: &Signature,
        now: u64,
    ) -> Result<(RemoteAdmissionRegistry, RemoteAdmissionOutcome), Error> {
        let (registry, outcome) = self.verify_retaining_at(signature, Ok(now));
        Ok((registry, outcome?))
    }

    // Native supervisor recovery of ledger ownership, never authentication or a reservation.
    pub(in crate::fleet) fn abandon(self) -> RemoteAdmissionRegistry {
        self.registry
    }

    pub(in crate::fleet) fn verify_retaining(
        self,
        signature: &Signature,
    ) -> (
        RemoteAdmissionRegistry,
        Result<RemoteAdmissionOutcome, Error>,
    ) {
        self.verify_retaining_at(signature, now_ms())
    }

    fn verify_retaining_at(
        self,
        signature: &Signature,
        now: Result<u64, Error>,
    ) -> (
        RemoteAdmissionRegistry,
        Result<RemoteAdmissionOutcome, Error>,
    ) {
        let Self {
            mut registry,
            work,
            allocation,
            proof,
            coordinator,
        } = self;
        let outcome = (|| {
            let now = now?;
            proof.fresh(now)?;
            Ed25519::verify(&coordinator, proof.payload().as_bytes(), signature)
                .map_err(|_| Error::Refused("remote-admission-proof-signature"))?;
            registry.reserve(work, &allocation, now)
        })();
        (registry, outcome)
    }
}

impl RemoteAdmissionRegistry {
    /// Issue a fresh bounded challenge without admitting work or allocating files. Registry keys
    /// and limits were independently configured by the worker; peers cannot replace them here.
    pub fn challenge(
        self,
        work: RemoteWork,
        allocation: &str,
    ) -> Result<RemoteAdmissionChallenge, Error> {
        let mut nonce = [0; 32];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut file| file.read_exact(&mut nonce))
            .map_err(|_| Error::Refused("remote-admission-proof-entropy"))?;
        self.challenge_at(work, allocation, now_ms()?, nonce)
    }

    fn challenge_at(
        self,
        work: RemoteWork,
        allocation: &str,
        now: u64,
        nonce: [u8; 32],
    ) -> Result<RemoteAdmissionChallenge, Error> {
        self.validate(&work, allocation)?;
        self.receipts()?; // Recheck guarded ledger authority without writing a reservation.
        if now == 0 || work.assignment.lease_until_ms <= now {
            return refuse("remote-admission-expired");
        }
        let proof = RemoteAdmissionProof {
            admission: admission_json(
                &self.coordinator,
                &self.worker,
                &self.objective,
                &self.limits,
                &work,
                allocation,
            ),
            nonce: RecordDigest::from_bytes(nonce),
            issued_ms: now,
            expires_ms: now
                .checked_add(LIFETIME_MS)
                .ok_or(Error::Refused("remote-admission-proof-clock"))?
                .min(work.assignment.lease_until_ms),
        };
        if proof.encode().len() > MAX_PROOF_BYTES {
            return refuse("remote-admission-proof-too-large");
        }
        let coordinator = PublicKey::from_bytes(
            *RecordDigest::parse_hex(&self.coordinator)
                .map_err(|_| Error::InvalidHistory)?
                .as_bytes(),
        );
        Ok(RemoteAdmissionChallenge {
            registry: self,
            work,
            allocation: allocation.into(),
            proof,
            coordinator,
        })
    }
}
fn key(key: &PublicKey) -> String {
    RecordDigest::from_bytes(*key.as_bytes()).to_string()
}
fn now_ms() -> Result<u64, Error> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|value| u64::try_from(value.as_millis()).ok())
        .ok_or(Error::Refused("remote-admission-proof-clock"))
}

#[cfg(test)]
pub(in crate::fleet) mod tests;
