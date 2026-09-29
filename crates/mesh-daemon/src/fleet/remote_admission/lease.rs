//! Monotonic retained leases; reading a lease never reconstructs launch authority.
use super::*;

const MAX_RENEWALS: u64 = 4096;

/// Historical worker acknowledgment. This is neither a reservation nor process liveness.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteWorkerLease {
    /// Starts at one for the original immutable admission.
    pub sequence: u64,
    /// Authorized deadline; expiration never frees the retained admission slot.
    pub until_ms: u64,
    /// Native acceptance time of the latest renewal; zero means initial admission only.
    pub accepted_ms: u64,
}

impl RemoteAdmissionRegistry {
    fn lease_stream(&self, admission: &RemoteAdmissionReceipt) -> String {
        lease_stream(
            &self.stream,
            &self.encode(&admission.work, &admission.allocation),
            admission.revision,
        )
    }

    fn check_lease_admission(&self, admission: &RemoteAdmissionReceipt) -> Result<(), Error> {
        if self
            .receipts()?
            .iter()
            .any(|retained| retained == admission)
        {
            Ok(())
        } else {
            refuse("remote-lease-admission-mismatch")
        }
    }

    /// Recover the current retained lease, including after expiration. No authority is granted.
    pub fn effective_lease(
        &self,
        admission: &RemoteAdmissionReceipt,
    ) -> Result<RemoteWorkerLease, Error> {
        self.check_lease_admission(admission)?;
        read_lease(
            &self.store,
            &self.lease_stream(admission),
            &admission.work.assignment,
        )
    }

    /// Commit an already-authenticated native renewal. The embedding service must verify the
    /// coordinator's signed exact scope and supply its own clock and configured lease cap.
    /// Exact replay recovers the acknowledgment even after expiration, without extending it again.
    /// Any uncertainty retains the original admission and launch owner; this cannot create either.
    pub fn renew_lease(
        &mut self,
        admission: &RemoteAdmissionReceipt,
        expected_sequence: u64,
        until_ms: u64,
        now_ms: u64,
        maximum_ms: u64,
    ) -> Result<RemoteWorkerLease, Error> {
        let current = self.effective_lease(admission)?;
        let stream = self.lease_stream(admission);
        let request = expected_sequence.to_string();
        if let Some(event) = self.store.request(&stream, &request)? {
            let value = Json::parse(&event.payload).map_err(|_| Error::InvalidHistory)?;
            if value.get("until_ms").and_then(Json::as_u64) != Some(until_ms)
                || expected_sequence == 0
                || expected_sequence >= current.sequence
            {
                return refuse("remote-lease-conflict");
            }
            return Ok(RemoteWorkerLease {
                sequence: expected_sequence + 1,
                until_ms,
                accepted_ms: value
                    .get("accepted_ms")
                    .and_then(Json::as_u64)
                    .ok_or(Error::InvalidHistory)?,
            });
        }
        if expected_sequence != current.sequence
            || expected_sequence > MAX_RENEWALS
            || now_ms < current.accepted_ms
            || now_ms == 0
            || now_ms >= current.until_ms
            || until_ms <= current.until_ms
            || maximum_ms == 0
            || until_ms.saturating_sub(now_ms) > maximum_ms
        {
            return refuse("remote-lease-stale-or-expired");
        }
        // Compare-and-append handles competing writers atomically. A competing identical request
        // can carry a different observation time and refuse; a fresh explicit replay then reads the
        // winner. Never silently retry a competing write.
        let payload = lease_payload(expected_sequence, until_ms, now_ms, maximum_ms);
        self.store
            .append_with_outcome(&stream, expected_sequence - 1, &request, &payload)?;
        self.check_lease_admission(admission)?;
        Ok(RemoteWorkerLease {
            sequence: expected_sequence + 1,
            until_ms,
            accepted_ms: now_ms,
        })
    }
}

fn lease_payload(sequence: u64, until: u64, accepted: u64, maximum: u64) -> String {
    Json::object([
        ("schema", Json::text("mesh.remote-worker-lease/v1")),
        ("expected_sequence", Json::Number(sequence)),
        ("until_ms", Json::Number(until)),
        ("accepted_ms", Json::Number(accepted)),
        ("maximum_ms", Json::Number(maximum)),
    ])
    .encode()
}

fn lease_stream(stream: &str, payload: &str, revision: u64) -> String {
    let identity = Json::object([
        ("stream", Json::text(stream)),
        ("admission", Json::text(payload)),
        ("revision", Json::Number(revision)),
    ])
    .encode();
    format!(
        "remote-leases-{}",
        Blake3::digest_bytes(identity.as_bytes())
    )
}

// The received session holds this exact admitted event and verifies its immutable receipt before
// calling. Retain the guarded store and recheck the event; no path or reopened unguarded database.
pub(in crate::fleet) fn received_lease(
    store: &FleetStore,
    admission: &FleetEvent,
    assignment: &RemoteAssignment,
) -> Result<RemoteWorkerLease, Error> {
    if store
        .request(&admission.stream, &admission.request)?
        .as_ref()
        != Some(admission)
    {
        return Err(Error::InvalidHistory);
    }
    read_lease(
        store,
        &lease_stream(&admission.stream, &admission.payload, admission.revision),
        assignment,
    )
}

fn read_lease(
    store: &FleetStore,
    stream: &str,
    assignment: &RemoteAssignment,
) -> Result<RemoteWorkerLease, Error> {
    let mut lease = RemoteWorkerLease {
        sequence: assignment.lease_sequence,
        until_ms: assignment.lease_until_ms,
        accepted_ms: 0,
    };
    let revision = store.revision(stream)?;
    if revision > MAX_RENEWALS {
        return Err(Error::InvalidHistory);
    }
    let mut events = Vec::with_capacity(revision as usize);
    while events.len() < revision as usize {
        let page = store.events(
            stream,
            events.len() as u64,
            ((revision as usize) - events.len()).min(mesh_store::fleet::MAX_FLEET_EVENT_PAGE),
        )?;
        if page.is_empty() {
            return Err(Error::InvalidHistory);
        }
        events.extend(page);
    }
    if store.revision(stream)? != revision {
        return refuse("remote-lease-changed-during-read");
    }
    for event in events {
        let value = Json::parse(&event.payload).map_err(|_| Error::InvalidHistory)?;
        let until = value
            .get("until_ms")
            .and_then(Json::as_u64)
            .ok_or(Error::InvalidHistory)?;
        let accepted = value
            .get("accepted_ms")
            .and_then(Json::as_u64)
            .ok_or(Error::InvalidHistory)?;
        let maximum = value
            .get("maximum_ms")
            .and_then(Json::as_u64)
            .ok_or(Error::InvalidHistory)?;
        if event.revision != lease.sequence
            || event.request != lease.sequence.to_string()
            || accepted < lease.accepted_ms
            || accepted == 0
            || accepted >= lease.until_ms
            || until <= lease.until_ms
            || maximum == 0
            || until.saturating_sub(accepted) > maximum
            || event.payload != lease_payload(lease.sequence, until, accepted, maximum)
        {
            return Err(Error::InvalidHistory);
        }
        lease.sequence += 1;
        lease.until_ms = until;
        lease.accepted_ms = accepted;
    }
    Ok(lease)
}
