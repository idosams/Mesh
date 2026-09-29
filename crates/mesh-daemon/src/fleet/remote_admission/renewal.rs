//! Signed continuation of one retained attempt, with durable coordinator intent before transport.
use super::*;
use crate::fleet::{Command, RemoteWorkerLease};

const REQUEST: &str = "mesh.worker-lease-renewal/v1";
const ACK: &str = "mesh.worker-lease-ack/v1";
const REQUEST_DOMAIN: DomainSeparator = DomainSeparator::new("mesh.v1.worker-lease-renewal");
const ACK_DOMAIN: DomainSeparator = DomainSeparator::new("mesh.v1.worker-lease-ack");

/// Native-selected lease continuation and local extension cap.
#[derive(Clone, Copy)]
pub struct RemoteLeaseRenewalPlan {
    /// Last worker-acknowledged sequence.
    pub expected_sequence: u64,
    /// Requested absolute native deadline.
    pub until_ms: u64,
    /// Maximum permitted duration from the coordinator's current clock.
    pub maximum_ms: u64,
}

/// Prepared native continuation. The separate intent record is not a worker acknowledgment.
pub struct RemoteLeaseRenewal {
    context: RemoteWorkerStatusChallenge,
    body: Json,
    intent: FleetEvent,
}
impl RemoteLeaseRenewal {
    /// Retain the exact request before any connection. After a lost reply, the same request may
    /// receive a fresh challenge even after expiry; only a worker's already-committed renewal can
    /// then succeed. A different deadline at the same sequence refuses instead of guessing.
    pub fn prepare(
        runtime: &mut Runtime,
        lane: &str,
        run: &str,
        coordinator: PublicKey,
        worker: PublicKey,
        plan: RemoteLeaseRenewalPlan,
    ) -> Result<Self, Error> {
        let RemoteLeaseRenewalPlan {
            expected_sequence,
            until_ms,
            maximum_ms,
        } = plan;
        let context = RemoteWorkerStatusChallenge::issue(runtime, lane, run, coordinator, worker)?;
        let current = context.lane.runs.last().ok_or_else(invalid)?;
        let assignment = current.remote.as_ref().ok_or_else(invalid)?;
        if runtime.state().cancelled
            || !current.state.occupies_slot()
            || current.state == crate::fleet::RunState::Stopping
            || expected_sequence == 0
            || expected_sequence > 4096
            || assignment.lease_sequence != expected_sequence
            || until_ms <= assignment.lease_until_ms
        {
            return Err(invalid());
        }
        let target = context.body.get("target").ok_or_else(invalid)?.clone();
        let stream = format!(
            "remote-renewals-{}",
            Blake3::digest_bytes(target.encode().as_bytes())
        );
        let request = expected_sequence.to_string();
        let intended = Json::object([
            ("schema", Json::text("mesh.remote-renewal-intent/v1")),
            ("target", target),
            ("expected_sequence", Json::Number(expected_sequence)),
            ("until_ms", Json::Number(until_ms)),
        ])
        .encode();
        let intent = if let Some(event) = runtime.store.request(&stream, &request)? {
            if event.revision != expected_sequence || event.payload != intended {
                return Err(invalid());
            }
            event
        } else {
            let time = now()?;
            if time >= assignment.lease_until_ms
                || maximum_ms == 0
                || until_ms.saturating_sub(time) > maximum_ms
            {
                return Err(invalid());
            }
            runtime
                .store
                .append_with_outcome(&stream, expected_sequence - 1, &request, &intended)?
                .into_event()
        };
        context.context(runtime, now()?)?;
        let body = Json::object([
            ("query", context.body.clone()),
            ("expected_sequence", Json::Number(expected_sequence)),
            ("until_ms", Json::Number(until_ms)),
        ]);
        Ok(Self {
            context,
            body,
            intent,
        })
    }
    fn check(&self, runtime: &mut Runtime) -> Result<(), Error> {
        self.context.context(runtime, now()?)?;
        if runtime
            .store
            .request(&self.intent.stream, &self.intent.request)?
            .as_ref()
            != Some(&self.intent)
        {
            return Err(invalid());
        }
        Ok(())
    }
    /// Sign only the prepared exact continuation, under a mutation-specific domain.
    pub fn signed_request(
        &self,
        runtime: &mut Runtime,
        sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
    ) -> Result<RemoteLeaseRenewalRequest, Error> {
        self.check(runtime)?;
        let message = payload(REQUEST_DOMAIN, &self.body);
        let signature = sign(&message).map_err(|_| invalid())?;
        Ed25519::verify(&self.context.coordinator, message.as_bytes(), &signature)
            .map_err(|_| invalid())?;
        self.check(runtime)?;
        Ok(RemoteLeaseRenewalRequest {
            body: self.body.clone(),
            signature,
        })
    }
    /// Exchange this exact persisted intent over one independently configured SSH connection.
    /// Errors retain uncertainty and the intent; the caller may explicitly prepare its exact
    /// replay. No automatic retry, new launch, lease extension by I/O budget or process adoption.
    pub fn exchange_over_ssh(
        self,
        runtime: &mut Runtime,
        destination: &NativeSshDestination,
        budget: Duration,
        sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
    ) -> io::Result<RemoteWorkerLease> {
        let err = |_| io::Error::other("remote lease renewal unavailable");
        let request = self.signed_request(runtime, sign).map_err(err)?;
        let mut connection = destination.connect(budget)?;
        let (input, output) = connection.streams()?;
        RemoteFrameWriter::new(output).write_frame(&request.frame().map_err(err)?)?;
        let Some(RemoteFrame::Control(reply)) = RemoteFrameReader::new(input).read_frame()? else {
            return Err(io::Error::other("remote lease renewal unavailable"));
        };
        self.accept(
            runtime,
            std::str::from_utf8(&reply)
                .map_err(|_| io::Error::other("remote lease renewal unavailable"))?,
        )
        .map_err(err)
    }

    /// Verify the worker's durable acknowledgment before advancing the coordinator projection.
    /// A failure after the worker commit leaves the original intent available for reconciliation.
    pub fn accept(self, runtime: &mut Runtime, encoded: &str) -> Result<RemoteWorkerLease, Error> {
        self.check(runtime)?;
        let outer = parse_envelope(encoded, ACK)?;
        let body = outer.get("body").ok_or_else(invalid)?;
        closed(
            body,
            &["request", "observed_ms", "acknowledged", "effective"],
        )?;
        let acknowledged = parse_lease(body.get("acknowledged").ok_or_else(invalid)?)?;
        let effective = parse_lease(body.get("effective").ok_or_else(invalid)?)?;
        let observed = number(body, "observed_ms")?;
        if text(body, "request")? != Blake3::digest_bytes(self.body.encode().as_bytes()).to_string()
            || observed < number(&self.context.body, "issued_ms")?
            || observed > now()?
            || acknowledged.sequence != number(&self.body, "expected_sequence")? + 1
            || acknowledged.until_ms != number(&self.body, "until_ms")?
            || acknowledged.accepted_ms > observed
            || effective != acknowledged
        {
            return Err(invalid());
        }
        let canonical = Json::object([
            ("request", Json::text(text(body, "request")?)),
            ("observed_ms", Json::Number(observed)),
            ("acknowledged", lease_json(&acknowledged)),
            ("effective", lease_json(&effective)),
        ]);
        let signature = Signature::from_bytes(bytes::<64>(text(&outer, "signature")?)?);
        if envelope(ACK, canonical, &signature) != encoded {
            return Err(invalid());
        }
        Ed25519::verify(
            &self.context.worker,
            payload(ACK_DOMAIN, body).as_bytes(),
            &signature,
        )
        .map_err(|_| invalid())?;
        self.check(runtime)?;
        let target = self.context.body.get("target").ok_or_else(invalid)?;
        let request = format!(
            "renewed-{}",
            Blake3::digest_bytes(self.intent.payload.as_bytes())
        );
        runtime.record(
            &request,
            Command::AdvanceRemoteLease {
                lane: text(target, "lane")?.into(),
                run: text(target, "run")?.into(),
                assignment: text(target, "assignment")?.into(),
                worker_key: text(target, "worker")?.into(),
                expected_sequence: number(&self.body, "expected_sequence")?,
                lease_until_ms: acknowledged.until_ms,
            },
        )?;
        Ok(acknowledged)
    }
}

/// Closed wire request; decoding alone never grants mutation authority.
pub struct RemoteLeaseRenewalRequest {
    body: Json,
    signature: Signature,
}
impl RemoteLeaseRenewalRequest {
    /// Canonical bounded encoding for a configured native transport.
    pub fn encode(&self) -> String {
        envelope(REQUEST, self.body.clone(), &self.signature)
    }
    /// One bounded control frame.
    pub fn frame(&self) -> Result<RemoteFrame, Error> {
        let encoded = self.encode();
        if encoded.len() > MAX {
            return Err(invalid());
        }
        Ok(RemoteFrame::Control(encoded.into_bytes()))
    }
    /// Strict closed schema and canonical encoding check, before signature verification.
    pub fn decode(encoded: &str) -> Result<Self, Error> {
        let outer = parse_envelope(encoded, REQUEST)?;
        let body = outer.get("body").ok_or_else(invalid)?;
        closed(body, &["query", "expected_sequence", "until_ms"])?;
        let query = query_body(body.get("query").ok_or_else(invalid)?)?;
        let sequence = number(body, "expected_sequence")?;
        if sequence == 0 || sequence > 4096 || number(body, "until_ms")? == 0 {
            return Err(invalid());
        }
        let request = Self {
            body: Json::object([
                ("query", query),
                ("expected_sequence", Json::Number(sequence)),
                ("until_ms", Json::Number(number(body, "until_ms")?)),
            ]),
            signature: Signature::from_bytes(bytes::<64>(text(&outer, "signature")?)?),
        };
        if request.encode() != encoded {
            return Err(invalid());
        }
        Ok(request)
    }
    /// Authenticate the separate renewal domain against native configured identities and caps.
    pub fn verify(
        self,
        policy: &RemoteDispatchPolicy<'_>,
    ) -> Result<VerifiedRemoteLeaseRenewal, Error> {
        let query = self.body.get("query").ok_or_else(invalid)?;
        fresh(query, now()?)?;
        let target = query.get("target").ok_or_else(invalid)?;
        let limits = parse_limits(query.get("limits").ok_or_else(invalid)?)?;
        crate::fleet::limits_valid(&policy.maximum)?;
        if text(target, "coordinator")? != hex(policy.coordinator.as_bytes())
            || text(target, "worker")? != hex(policy.worker.as_bytes())
            || text(target, "provider")? != policy.provider
            || limits.lanes > policy.maximum.lanes
            || limits.concurrency > policy.maximum.concurrency
            || limits.depth > policy.maximum.depth
            || limits.retries > policy.maximum.retries
            || policy.max_lease_ms == 0
        {
            return Err(invalid());
        }
        Ed25519::verify(
            &policy.coordinator,
            payload(REQUEST_DOMAIN, &self.body).as_bytes(),
            &self.signature,
        )
        .map_err(|_| invalid())?;
        Ok(VerifiedRemoteLeaseRenewal {
            request: self,
            limits,
            worker: policy.worker,
            maximum: policy.max_lease_ms,
        })
    }
}
/// Authenticated continuation. It can only renew an existing exact admission.
pub struct VerifiedRemoteLeaseRenewal {
    request: RemoteLeaseRenewalRequest,
    limits: Limits,
    worker: PublicKey,
    maximum: u64,
}
impl VerifiedRemoteLeaseRenewal {
    fn query(&self) -> &Json {
        self.request.body.get("query").unwrap()
    }
    fn target(&self) -> &Json {
        self.query().get("target").unwrap()
    }
    /// Verified coordinator identity, never a path.
    pub fn coordinator(&self) -> &str {
        text(self.target(), "coordinator").unwrap()
    }
    /// Verified objective identity, never a path.
    pub fn objective(&self) -> &str {
        text(self.target(), "objective").unwrap()
    }
    /// Signed objective limits bounded by native policy.
    pub fn limits(&self) -> &Limits {
        &self.limits
    }
    /// Persist renewal before signing its acknowledgment. A lost reply retains the same record.
    pub fn commit(
        self,
        registry: &mut RemoteAdmissionRegistry,
        sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
    ) -> Result<RemoteFrame, Error> {
        fresh(self.query(), now()?)?;
        if registry.coordinator != self.coordinator()
            || registry.objective != self.objective()
            || registry.worker != hex(self.worker.as_bytes())
            || registry.limits != self.limits
        {
            return Err(invalid());
        }
        let receipt = registry
            .receipts()?
            .into_iter()
            .find(|r| r.work.assignment.id == text(self.target(), "assignment").unwrap())
            .ok_or_else(invalid)?;
        if identity(&registry.coordinator, &registry.objective, &receipt.work) != *self.target() {
            return Err(invalid());
        }
        let acknowledged = registry.renew_lease(
            &receipt,
            number(&self.request.body, "expected_sequence")?,
            number(&self.request.body, "until_ms")?,
            now()?,
            self.maximum,
        )?;
        let effective = registry.effective_lease(&receipt)?;
        let body = Json::object([
            (
                "request",
                Json::text(Blake3::digest_bytes(self.request.body.encode().as_bytes()).to_string()),
            ),
            ("observed_ms", Json::Number(now()?)),
            ("acknowledged", lease_json(&acknowledged)),
            ("effective", lease_json(&effective)),
        ]);
        let message = payload(ACK_DOMAIN, &body);
        let signature = sign(&message).map_err(|_| invalid())?;
        Ed25519::verify(&self.worker, message.as_bytes(), &signature).map_err(|_| invalid())?;
        fresh(self.query(), now()?)?;
        if registry.effective_lease(&receipt)? != effective {
            return Err(invalid());
        }
        let encoded = envelope(ACK, body, &signature);
        if encoded.len() > MAX {
            return Err(invalid());
        }
        Ok(RemoteFrame::Control(encoded.into_bytes()))
    }
}
fn lease_json(lease: &RemoteWorkerLease) -> Json {
    Json::object([
        ("sequence", Json::Number(lease.sequence)),
        ("until_ms", Json::Number(lease.until_ms)),
        ("accepted_ms", Json::Number(lease.accepted_ms)),
    ])
}
fn parse_lease(value: &Json) -> Result<RemoteWorkerLease, Error> {
    closed(value, &["sequence", "until_ms", "accepted_ms"])?;
    let lease = RemoteWorkerLease {
        sequence: number(value, "sequence")?,
        until_ms: number(value, "until_ms")?,
        accepted_ms: number(value, "accepted_ms")?,
    };
    if lease.sequence < 2
        || lease.sequence > 4097
        || lease.accepted_ms == 0
        || lease.until_ms <= lease.accepted_ms
    {
        return Err(invalid());
    }
    Ok(lease)
}

#[cfg(test)]
#[path = "renewal/tests.rs"]
mod tests;
