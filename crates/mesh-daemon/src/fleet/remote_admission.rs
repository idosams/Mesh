//! Worker-wide admission facts, separate from authentication, allocation and process launch.
use super::{goal_valid, id_valid, refuse, Error, Limits, RemoteAssignment, State};
use crate::ipc::Json;
use mesh_store::fleet::{FleetAppendOutcome, FleetEvent, FleetStore};
use mesh_store::RecordDigest;
use mesh_types::{Blake3, ContentDigest as _};

/// Immutable work already authenticated and authorized by the native worker service.
/// These fields are correlation, never credentials or proof of admission by themselves.
#[derive(Clone, PartialEq, Eq)]
pub struct RemoteWork {
    /// Coordinator objective's lane identity.
    pub lane: String,
    /// Exact coordinator attempt identity.
    pub run: String,
    /// Immutable source input, bundle, worker and initial lease.
    pub assignment: RemoteAssignment,
    /// Native-admitted provider, fixed before allocation.
    pub provider: String,
    /// Authorized task, kept out of diagnostics.
    pub goal: String,
}

/// Durable admission receipt. Reading or cloning this fact cannot allocate or launch anything.
#[derive(Clone, PartialEq, Eq)]
pub struct RemoteAdmissionReceipt {
    coordinator: String,
    objective: String,
    work: RemoteWork,
    allocation: String,
    revision: u64,
}
impl RemoteAdmissionReceipt {
    /// Configured coordinator identity that owns this admission namespace.
    pub fn coordinator(&self) -> &str {
        &self.coordinator
    }
    /// Exact objective within that coordinator's namespace.
    pub fn objective(&self) -> &str {
        &self.objective
    }
    /// Immutable coordinator work retained by the worker.
    pub fn work(&self) -> &RemoteWork {
        &self.work
    }
    /// Native-selected allocation name, never a peer-selected pathname.
    pub fn allocation(&self) -> &str {
        &self.allocation
    }
    /// Exact admission revision, unchanged by replay.
    pub fn revision(&self) -> u64 {
        self.revision
    }
}

/// Original successful admission only. Consumed by native materialization, including on failure.
/// This is not a provider launch permit, actor credential or workspace authority.
pub struct RemoteInputReservation(RemoteAdmissionReceipt);
impl RemoteInputReservation {
    /// Inspect retained facts without manufacturing another reservation.
    pub fn receipt(&self) -> &RemoteAdmissionReceipt {
        &self.0
    }
    pub(super) fn consume(
        self,
        assignment: &RemoteAssignment,
    ) -> Result<RemoteAdmissionReceipt, Error> {
        if &self.0.work.assignment != assignment {
            return refuse("remote-admission-assignment-mismatch");
        }
        Ok(self.0)
    }
}

/// Only a new atomic insertion carries a single-use input reservation.
pub enum RemoteAdmissionOutcome {
    /// This invocation committed admission and passed final ledger authority validation.
    Reserved(RemoteInputReservation),
    /// Retained admission, including after lost acknowledgment or restart. Never reserve again.
    Retained(RemoteAdmissionReceipt),
}

/// One coordinator/objective view of the worker's shared native-owned admission ledger.
///
/// The native supervisor must supply the SAME guarded worker ledger across all allocations and
/// connections, independently configured keys and already-authorized limits. Opening a database
/// per destination defeats that contract. This model does not authenticate keys or provision paths.
/// All admissions retain a concurrency slot; terminal reconciliation/release is not implemented.
pub struct RemoteAdmissionRegistry {
    store: FleetStore,
    coordinator: String,
    worker: String,
    objective: String,
    limits: Limits,
    stream: String,
}
impl RemoteAdmissionRegistry {
    /// Bind native configuration without admitting work or touching an allocation.
    pub fn new(
        store: FleetStore,
        coordinator: &str,
        worker: &str,
        objective: &str,
        limits: Limits,
    ) -> Result<Self, Error> {
        id_valid(objective)?;
        for key in [coordinator, worker] {
            if !RecordDigest::parse_hex(key).is_ok_and(|digest| digest.to_string() == key) {
                return refuse("remote-admission-key-invalid");
            }
        }
        State::default().apply(&super::Command::Start {
            goal: "remote admission limits".into(),
            limits: limits.clone(),
        })?;
        // Delimited canonical encoding avoids ambiguous identity concatenation. Worker, lane,
        // run, destination and provider intentionally cannot create another uniqueness namespace.
        let identity = Json::object([
            ("schema", Json::text("mesh.remote-admission-key/v1")),
            ("coordinator", Json::text(coordinator)),
            ("objective", Json::text(objective)),
        ])
        .encode();
        let value = Self {
            store,
            coordinator: coordinator.into(),
            worker: worker.into(),
            objective: objective.into(),
            limits,
            stream: format!(
                "remote-admissions-{}",
                Blake3::digest_bytes(identity.as_bytes())
            ),
        };
        value.receipts()?;
        Ok(value)
    }

    /// Recover bounded admission facts. Expiry, disconnect and restart release no slots.
    pub fn receipts(&self) -> Result<Vec<RemoteAdmissionReceipt>, Error> {
        let events = self.store.events(&self.stream, 0, 65)?;
        if events.len() as u64 > self.limits.concurrency {
            return Err(Error::InvalidHistory);
        }
        let mut receipts: Vec<RemoteAdmissionReceipt> = Vec::with_capacity(events.len());
        for event in events {
            let receipt = self.decode(&event)?;
            if event.revision != receipts.len() as u64 + 1
                || receipts.iter().any(|old| {
                    old.work.assignment.id == receipt.work.assignment.id
                        || old.work.lane == receipt.work.lane
                        || old.work.run == receipt.work.run
                        || old.allocation == receipt.allocation
                })
            {
                return Err(Error::InvalidHistory);
            }
            receipts.push(receipt);
        }
        Ok(receipts)
    }

    /// Reserve one input allocation before any materialization. `now_ms` is the native clock,
    /// not a peer timestamp. Exact replay recovers facts even after expiry, but never a reservation.
    /// A changed assignment body or allocation under the same identity refuses. Competing distinct
    /// writes return a stale-revision error; reload before deciding again, never blindly retry effects.
    pub fn reserve(
        &mut self,
        work: RemoteWork,
        allocation: &str,
        now_ms: u64,
    ) -> Result<RemoteAdmissionOutcome, Error> {
        self.validate(&work, allocation)?;
        let payload = self.encode(&work, allocation);
        let receipts = self.receipts()?;
        let prior = receipts
            .iter()
            .find(|old| old.work.assignment.id == work.assignment.id);
        let expected = if let Some(prior) = prior {
            if prior.work != work || prior.allocation != allocation {
                return refuse("remote-admission-conflict");
            }
            prior.revision - 1
        } else {
            if now_ms == 0 || work.assignment.lease_until_ms <= now_ms {
                return refuse("remote-admission-expired");
            }
            if receipts.len() as u64 >= self.limits.concurrency {
                return refuse("remote-admission-capacity");
            }
            if receipts.iter().any(|old| {
                old.work.lane == work.lane
                    || old.work.run == work.run
                    || old.allocation == allocation
            }) {
                return refuse("remote-admission-conflict");
            }
            receipts.len() as u64
        };
        // Replay lookup and commit must remain in one writer transaction. A pre-read alone would
        // let two concurrent identical connections manufacture two materialization reservations.
        match self.store.append_with_outcome(
            &self.stream,
            expected,
            &work.assignment.id,
            &payload,
        )? {
            FleetAppendOutcome::Inserted(event) => Ok(RemoteAdmissionOutcome::Reserved(
                RemoteInputReservation(self.decode(&event)?),
            )),
            FleetAppendOutcome::Replayed(event) => {
                Ok(RemoteAdmissionOutcome::Retained(self.decode(&event)?))
            }
        }
    }

    fn validate(&self, work: &RemoteWork, allocation: &str) -> Result<(), Error> {
        work.assignment.validate()?;
        id_valid(&work.lane)?;
        id_valid(&work.run)?;
        id_valid(&work.provider)?;
        goal_valid(&work.goal)?;
        if work.assignment.worker_key != self.worker
            || allocation.len() != 32
            || !allocation
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return refuse("remote-admission-invalid");
        }
        Ok(())
    }
    fn encode(&self, work: &RemoteWork, allocation: &str) -> String {
        admission_json(
            &self.coordinator,
            &self.worker,
            &self.objective,
            &self.limits,
            work,
            allocation,
        )
        .encode()
    }
    fn decode(&self, event: &FleetEvent) -> Result<RemoteAdmissionReceipt, Error> {
        let value = Json::parse(&event.payload).map_err(|_| Error::InvalidHistory)?;
        let text = |key| {
            value
                .get(key)
                .and_then(Json::as_text)
                .map(String::from)
                .ok_or(Error::InvalidHistory)
        };
        let number = |key| {
            value
                .get(key)
                .and_then(Json::as_u64)
                .ok_or(Error::InvalidHistory)
        };
        let digest = |key| RecordDigest::parse_hex(&text(key)?).map_err(|_| Error::InvalidHistory);
        let work = RemoteWork {
            lane: text("lane")?,
            run: text("run")?,
            provider: text("provider")?,
            goal: text("goal")?,
            assignment: RemoteAssignment {
                id: text("assignment")?,
                worker_key: text("worker")?,
                input: digest("input")?,
                bundle: digest("bundle")?,
                lease_sequence: number("lease_sequence")?,
                lease_until_ms: number("lease_until_ms")?,
            },
        };
        let allocation = text("allocation")?;
        self.validate(&work, &allocation)
            .map_err(|_| Error::InvalidHistory)?;
        // Byte-exact canonical encoding also rejects unknown/missing fields, schema changes,
        // noncanonical digests and changed native key/objective/limit configuration on reopen.
        if event.stream != self.stream
            || event.request != work.assignment.id
            || self.encode(&work, &allocation) != event.payload
        {
            return Err(Error::InvalidHistory);
        }
        Ok(RemoteAdmissionReceipt {
            coordinator: self.coordinator.clone(),
            objective: self.objective.clone(),
            work,
            allocation,
            revision: event.revision,
        })
    }
}

#[cfg(test)]
mod tests;

#[cfg(unix)]
pub(in crate::fleet) mod launch;
#[cfg(unix)]
pub use launch::{RemoteLaunchOutcome, RemoteLaunchReceipt, RemoteLaunchReservation};

// Shared exact admission encoding for durable records and coordinator proof comparison.
fn admission_json(
    coordinator: &str,
    worker: &str,
    objective: &str,
    limits: &Limits,
    work: &RemoteWork,
    allocation: &str,
) -> Json {
    let a = &work.assignment;
    Json::object([
        ("schema", Json::text("mesh.remote-admission/v1")),
        ("coordinator", Json::text(coordinator)),
        ("worker", Json::text(worker)),
        ("objective", Json::text(objective)),
        ("lanes", Json::Number(limits.lanes)),
        ("concurrency", Json::Number(limits.concurrency)),
        ("depth", Json::Number(limits.depth)),
        ("retries", Json::Number(limits.retries)),
        ("lane", Json::text(&work.lane)),
        ("run", Json::text(&work.run)),
        ("provider", Json::text(&work.provider)),
        ("goal", Json::text(&work.goal)),
        ("assignment", Json::text(&a.id)),
        ("input", Json::text(a.input.to_string())),
        ("bundle", Json::text(a.bundle.to_string())),
        ("lease_sequence", Json::Number(a.lease_sequence)),
        ("lease_until_ms", Json::Number(a.lease_until_ms)),
        ("allocation", Json::text(allocation)),
    ])
}

#[cfg(unix)]
mod authentication;
#[cfg(unix)]
pub use authentication::{RemoteAdmissionChallenge, RemoteAdmissionProof};
