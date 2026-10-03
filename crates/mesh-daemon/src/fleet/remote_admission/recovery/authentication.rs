//! Single-use recovery authentication binds original provenance and the exact current lease.
use super::*;
use crate::fleet::{Lane, RunState, Runtime};
use mesh_crypto::{DomainSeparator, Ed25519, SignatureScheme, SigningPayload};
use mesh_types::{PublicKey, Signature};
use std::io::Read as _;

const DOMAIN: DomainSeparator = DomainSeparator::new("mesh.v1.fleet-original-recovery");
const MAX_BYTES: usize = 65_536;
const LIFETIME_MS: u64 = 30_000;
fn invalid() -> Error {
    Error::Refused("remote-recovery-proof-refused")
}
fn now() -> Result<u64, Error> {
    crate::fleet::service::received_clock().map_err(|_| invalid())
}
fn number(v: &Json, k: &str) -> Result<u64, Error> {
    v.get(k).and_then(Json::as_u64).ok_or_else(invalid)
}
fn text<'a>(v: &'a Json, k: &str) -> Result<&'a str, Error> {
    v.get(k).and_then(Json::as_text).ok_or_else(invalid)
}
fn key(k: &PublicKey) -> String {
    RecordDigest::from_bytes(*k.as_bytes()).to_string()
}

/// Bounded task-bearing facts. Decoding or cloning these facts grants no mutable authority.
/// The original admission stays immutable; the effective lease is a separately bound fact.
#[derive(Clone)]
pub struct RemoteRecoveryProof {
    admission: Json,
    revision: u64,
    roots: [String; 3],
    lease: RemoteWorkerLease,
    nonce: RecordDigest,
    issued_ms: u64,
    expires_ms: u64,
}
impl RemoteRecoveryProof {
    /// Canonical wire facts; callers must not log the private task or treat this as a permit.
    pub fn encode(&self) -> String {
        Json::object([
            ("schema", Json::text("mesh.original-recovery-challenge/v1")),
            ("admission", self.admission.clone()),
            ("admission_revision", Json::Number(self.revision)),
            ("parent", Json::text(&self.roots[0])),
            ("allocation", Json::text(&self.roots[1])),
            ("files", Json::text(&self.roots[2])),
            ("lease_sequence", Json::Number(self.lease.sequence)),
            ("lease_until_ms", Json::Number(self.lease.until_ms)),
            ("lease_accepted_ms", Json::Number(self.lease.accepted_ms)),
            ("nonce", Json::text(self.nonce.to_string())),
            ("issued_ms", Json::Number(self.issued_ms)),
            ("expires_ms", Json::Number(self.expires_ms)),
        ])
        .encode()
    }
    /// Decode only the bounded canonical envelope. Native coordinator state must still match.
    pub fn decode(encoded: &str) -> Result<Self, Error> {
        if encoded.len() > MAX_BYTES {
            return Err(invalid());
        }
        let v = Json::parse(encoded).map_err(|_| invalid())?;
        let proof = Self {
            admission: v.get("admission").cloned().ok_or_else(invalid)?,
            revision: number(&v, "admission_revision")?,
            roots: [
                text(&v, "parent")?.into(),
                text(&v, "allocation")?.into(),
                text(&v, "files")?.into(),
            ],
            lease: RemoteWorkerLease {
                sequence: number(&v, "lease_sequence")?,
                until_ms: number(&v, "lease_until_ms")?,
                accepted_ms: number(&v, "lease_accepted_ms")?,
            },
            nonce: RecordDigest::parse_hex(text(&v, "nonce")?).map_err(|_| invalid())?,
            issued_ms: number(&v, "issued_ms")?,
            expires_ms: number(&v, "expires_ms")?,
        };
        if proof.encode() != encoded
            || proof.revision == 0
            || proof.lease.sequence == 0
            || proof.lease.sequence > 4097
            || proof.issued_ms == 0
            || proof.expires_ms <= proof.issued_ms
            || proof.expires_ms - proof.issued_ms > LIFETIME_MS
            || proof.expires_ms > proof.lease.until_ms
            || proof.lease.accepted_ms > proof.issued_ms
        {
            return Err(invalid());
        }
        for root in &proof.roots {
            let parsed =
                crate::ProtectedWorkspaceRoot::from_directory_token(root).map_err(|_| invalid())?;
            if parsed.directory_token() != *root {
                return Err(invalid());
            }
        }
        if proof.roots[0] == proof.roots[1]
            || proof.roots[0] == proof.roots[2]
            || proof.roots[1] == proof.roots[2]
        {
            return Err(invalid());
        }
        Ok(proof)
    }
    fn fresh(&self, time: u64) -> Result<(), Error> {
        if time < self.issued_ms || time >= self.expires_ms {
            return Err(invalid());
        }
        Ok(())
    }
    fn payload(&self) -> SigningPayload {
        SigningPayload::new(DOMAIN, self.encode().as_bytes())
    }
    fn context(
        &self,
        runtime: &mut Runtime,
        lane: &str,
        run: &str,
        coordinator: &PublicKey,
        worker: &PublicKey,
        time: u64,
    ) -> Result<(Lane, Limits), Error> {
        self.fresh(time)?;
        runtime.refresh()?;
        let state = runtime.state();
        let selected = state.lanes.get(lane).ok_or_else(invalid)?;
        let attempt = selected.runs.last().ok_or_else(invalid)?;
        let assignment = attempt.remote.as_ref().ok_or_else(invalid)?;
        if state.cancelled
            || selected.workspace.is_none()
            || attempt.id != run
            || attempt.state != RunState::Launching
            || attempt.launch_owner.as_deref() != Some(format!("remote:{}", assignment.id).as_str())
            || selected.base != assignment.input
            || assignment.worker_key != key(worker)
            || assignment.lease_sequence != self.lease.sequence
            || assignment.lease_until_ms != self.lease.until_ms
            || assignment.lease_until_ms <= time
        {
            return Err(invalid());
        }
        let mut original = assignment.clone();
        original.lease_sequence = number(&self.admission, "lease_sequence")?;
        original.lease_until_ms = number(&self.admission, "lease_until_ms")?;
        original.validate()?;
        if original.lease_until_ms > self.lease.until_ms
            || (self.lease.sequence == 1
                && (original.lease_until_ms != self.lease.until_ms || self.lease.accepted_ms != 0))
            || (self.lease.sequence > 1
                && (original.lease_until_ms >= self.lease.until_ms || self.lease.accepted_ms == 0))
        {
            return Err(invalid());
        }
        let work = RemoteWork {
            lane: lane.into(),
            run: run.into(),
            assignment: original,
            provider: selected.provider.clone(),
            goal: selected.goal.clone(),
        };
        let allocation = text(&self.admission, "allocation")?;
        if allocation.len() != 32
            || !allocation
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid());
        }
        let limits = state.limits.as_ref().ok_or_else(invalid)?;
        if self.admission
            != admission_json(
                &key(coordinator),
                &key(worker),
                runtime.objective(),
                limits,
                &work,
                allocation,
            )
        {
            return Err(invalid());
        }
        Ok((selected.clone(), limits.clone()))
    }
    /// Sign only this exact claimed prelaunch attempt. Recheck cancellation, lease and the entire
    /// selected lane after the signer returns, so a slow native signer cannot authorize stale work.
    pub fn sign_for(
        &self,
        runtime: &mut Runtime,
        lane: &str,
        run: &str,
        coordinator: &PublicKey,
        worker: &PublicKey,
        sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
    ) -> Result<Signature, Error> {
        let before = self.context(runtime, lane, run, coordinator, worker, now()?)?;
        let signature = sign(&self.payload()).map_err(|_| invalid())?;
        if before != self.context(runtime, lane, run, coordinator, worker, now()?)? {
            return Err(invalid());
        }
        Ok(signature)
    }
}

/// Worker-owned, non-cloneable, single-use challenge. It retains the original guarded ledger;
/// no proof bytes or historical receipt can reconstruct this ownership or an input reservation.
pub struct RemoteRecoveryChallenge {
    registry: RemoteAdmissionRegistry,
    materialization: RemoteMaterializationReceipt,
    proof: RemoteRecoveryProof,
}
impl RemoteRecoveryChallenge {
    /// Facts for the configured coordinator; not a general signing request.
    pub fn proof(&self) -> &RemoteRecoveryProof {
        &self.proof
    }
    fn check(&self, time: u64) -> Result<(), Error> {
        self.proof.fresh(time)?;
        let _guarded = self.registry.store.reopen_guarded_connection()?;
        let admission = self.materialization.admission();
        if self.registry.effective_lease(admission)? != self.proof.lease
            || self
                .registry
                .materialization_receipt(&admission.work.assignment.id)?
                .as_ref()
                != Some(&self.materialization)
            || self
                .registry
                .launch_receipt(&admission.work.assignment.id)?
                .is_some()
        {
            return Err(invalid());
        }
        Ok(())
    }
    fn authenticate(&self, signature: &Signature, time: u64) -> Result<(), Error> {
        self.check(time)?;
        let coordinator = PublicKey::from_bytes(
            *RecordDigest::parse_hex(&self.registry.coordinator)
                .map_err(|_| invalid())?
                .as_bytes(),
        );
        Ed25519::verify(&coordinator, self.proof.payload().as_bytes(), signature)
            .map_err(|_| invalid())
    }
    /// Consume fresh authentication to continue only original initialization. Each recovery phase
    /// rechecks proof expiry and the exact effective lease; renewal requires a new challenge.
    /// Returns original native ownership even though it grants no process launch permission.
    pub fn recover(
        self,
        signature: &Signature,
        destination: &RemoteInputDestination,
        reviewers: TrustedReviewers,
        checkpoint: CheckpointRuntimeParameters,
    ) -> Result<(ReceivedWorkerWorkspace, RemoteAdmissionRegistry), Error> {
        self.authenticate(signature, now()?)?;
        let workspace = self.registry.recover_initialization_guarded(
            destination,
            self.materialization.admission(),
            reviewers,
            checkpoint,
            || self.check(now()?),
        )?;
        self.check(now()?)?;
        Ok((workspace, self.registry))
    }
}
impl RemoteAdmissionRegistry {
    /// Challenge the original acknowledged input under the current effective lease. No mutation,
    /// renewed reservation or new allocation occurs. Uncertain launch records refuse intact.
    pub fn recovery_challenge(self, assignment: &str) -> Result<RemoteRecoveryChallenge, Error> {
        let mut nonce = [0; 32];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut nonce))
            .map_err(|_| invalid())?;
        self.recovery_challenge_at(assignment, now()?, nonce)
    }
    fn recovery_challenge_at(
        self,
        assignment: &str,
        time: u64,
        nonce: [u8; 32],
    ) -> Result<RemoteRecoveryChallenge, Error> {
        let _guarded = self.store.reopen_guarded_connection()?;
        let materialization = self
            .materialization_receipt(assignment)?
            .ok_or_else(invalid)?;
        let admission = materialization.admission();
        let lease = self.effective_lease(admission)?;
        if time == 0 || time < lease.accepted_ms || time >= lease.until_ms {
            return Err(invalid());
        }
        let proof = RemoteRecoveryProof {
            admission: admission_json(
                &self.coordinator,
                &self.worker,
                &self.objective,
                &self.limits,
                &admission.work,
                &admission.allocation,
            ),
            revision: admission.revision,
            roots: materialization.directory_identities().clone(),
            expires_ms: time
                .checked_add(LIFETIME_MS)
                .ok_or_else(invalid)?
                .min(lease.until_ms),
            lease,
            nonce: RecordDigest::from_bytes(nonce),
            issued_ms: time,
        };
        RemoteRecoveryProof::decode(&proof.encode())?;
        let challenge = RemoteRecoveryChallenge {
            registry: self,
            materialization,
            proof,
        };
        challenge.check(time)?;
        Ok(challenge)
    }
}

#[cfg(test)]
mod tests;
