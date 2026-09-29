//! Fresh authenticated recovery of one known saved-result offer. No content or execution transfer.
use super::*;
const QUERY_SCHEMA: &str = "mesh.worker-result-query/v1";
const REPLY_SCHEMA: &str = "mesh.worker-result-reply/v1";
const QUERY_SIGNING: DomainSeparator = DomainSeparator::new("mesh.v1.worker-result-query");
const REPLY_SIGNING: DomainSeparator = DomainSeparator::new("mesh.v1.worker-result-reply");

/// Single-use query bound to an exact current assignment and known checkpoint identity.
pub struct RemoteSavedResultChallenge {
    context: RemoteWorkerStatusChallenge,
    body: Json,
}
impl RemoteSavedResultChallenge {
    /// Expired work remains readable; this does not renew a lease or discover checkpoint names.
    pub fn issue(
        request: crate::fleet::RemoteWorkerStatusRequest<'_>,
        checkpoint: &str,
    ) -> Result<Self, Error> {
        id_valid(checkpoint)?;
        let context = RemoteWorkerStatusChallenge::issue(
            request.runtime,
            request.lane,
            request.run,
            request.coordinator,
            request.worker,
        )?;
        let body = Json::object([
            ("query", context.body.clone()),
            ("checkpoint", Json::text(checkpoint)),
        ]);
        Ok(Self { context, body })
    }
    /// Sign the prepared exact query under its own domain, with context checks around the callback.
    pub fn signed_query(
        &self,
        runtime: &mut Runtime,
        sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
    ) -> Result<RemoteSavedResultQuery, Error> {
        self.context.context(runtime, now()?)?;
        let message = payload(QUERY_SIGNING, &self.body);
        let signature = sign(&message).map_err(|_| invalid())?;
        Ed25519::verify(&self.context.coordinator, message.as_bytes(), &signature)
            .map_err(|_| invalid())?;
        self.context.context(runtime, now()?)?;
        Ok(RemoteSavedResultQuery {
            body: self.body.clone(),
            signature,
        })
    }
    /// Consume a fresh signed observation. Absence is unknown, never permission for a new attempt.
    pub fn verify_reply(
        self,
        runtime: &mut Runtime,
        encoded: &str,
    ) -> Result<Option<RemoteSavedResultOffer>, Error> {
        self.context.context(runtime, now()?)?;
        let outer = parse_envelope(encoded, REPLY_SCHEMA)?;
        let body = closed(
            outer.get("body").ok_or_else(invalid)?,
            &["query", "observed_ms", "offer"],
        )?;
        let observed = number(&body, "observed_ms")?;
        if text(&body, "query")? != Blake3::digest_bytes(self.body.encode().as_bytes()).to_string()
            || observed < number(&self.context.body, "issued_ms")?
            || observed > now()?
        {
            return Err(invalid());
        }
        let signature = Signature::from_bytes(bytes::<64>(text(&outer, "signature")?)?);
        if envelope(REPLY_SCHEMA, body.clone(), &signature) != encoded {
            return Err(invalid());
        }
        Ed25519::verify(
            &self.context.worker,
            payload(REPLY_SIGNING, &body).as_bytes(),
            &signature,
        )
        .map_err(|_| invalid())?;
        let offer = match body.get("offer").ok_or_else(invalid)? {
            Json::Null => None,
            value => Some(RemoteSavedResultOffer::decode(
                value.as_text().ok_or_else(invalid)?,
            )?),
        };
        if let Some(offer) = &offer {
            if offer.body.get("target") != self.context.body.get("target")
                || text(&offer.body, "checkpoint")? != text(&self.body, "checkpoint")?
            {
                return Err(invalid());
            }
        }
        self.context.context(runtime, now()?)?;
        Ok(offer)
    }
}
/// Canonical private query bytes; parsing alone provides no access to a worker ledger.
pub struct RemoteSavedResultQuery {
    body: Json,
    signature: Signature,
}
impl RemoteSavedResultQuery {
    /// Canonical private control bytes; never log task correlation.
    pub fn encode(&self) -> String {
        envelope(QUERY_SCHEMA, self.body.clone(), &self.signature)
    }
    /// One bounded frame for an independently authenticated transport.
    pub fn frame(&self) -> Result<RemoteFrame, Error> {
        let v = self.encode().into_bytes();
        if v.len() > MAX {
            return Err(invalid());
        }
        Ok(RemoteFrame::Control(v))
    }
    /// Reject unsupported schemas, unknown fields and noncanonical encodings.
    pub fn decode(encoded: &str) -> Result<Self, Error> {
        let outer = parse_envelope(encoded, QUERY_SCHEMA)?;
        let raw = closed(
            outer.get("body").ok_or_else(invalid)?,
            &["query", "checkpoint"],
        )?;
        let query = query_body(raw.get("query").ok_or_else(invalid)?)?;
        let checkpoint = text(&raw, "checkpoint")?;
        id_valid(checkpoint)?;
        let body = Json::object([("query", query), ("checkpoint", Json::text(checkpoint))]);
        let signature = Signature::from_bytes(bytes::<64>(text(&outer, "signature")?)?);
        if envelope(QUERY_SCHEMA, body.clone(), &signature) != encoded {
            return Err(invalid());
        }
        Ok(Self { body, signature })
    }
    /// Authenticate freshness, configured peer keys, provider and objective limits.
    pub fn verify(
        self,
        policy: &RemoteDispatchPolicy<'_>,
    ) -> Result<VerifiedRemoteSavedResultQuery, Error> {
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
        {
            return Err(invalid());
        }
        Ed25519::verify(
            &policy.coordinator,
            payload(QUERY_SIGNING, &self.body).as_bytes(),
            &self.signature,
        )
        .map_err(|_| invalid())?;
        Ok(VerifiedRemoteSavedResultQuery {
            query: self,
            limits,
            worker: policy.worker,
        })
    }
}
/// Native policy verified read-only access to one original assignment's known checkpoint.
pub struct VerifiedRemoteSavedResultQuery {
    query: RemoteSavedResultQuery,
    limits: Limits,
    worker: PublicKey,
}
impl VerifiedRemoteSavedResultQuery {
    fn context(&self) -> &Json {
        self.query.body.get("query").unwrap()
    }
    fn target(&self) -> &Json {
        self.context().get("target").unwrap()
    }
    /// Verified coordinator namespace; never a filesystem path.
    pub fn coordinator(&self) -> &str {
        text(self.target(), "coordinator").unwrap()
    }
    /// Verified objective namespace for the guarded worker ledger.
    pub fn objective(&self) -> &str {
        text(self.target(), "objective").unwrap()
    }
    /// Signed objective limits already bounded by native worker policy.
    pub fn limits(&self) -> &Limits {
        &self.limits
    }
    fn offer(&self, registry: &RemoteAdmissionRegistry) -> Result<Json, Error> {
        if registry.coordinator != self.coordinator()
            || registry.objective != self.objective()
            || registry.worker != hex(self.worker.as_bytes())
            || registry.limits != self.limits
        {
            return Err(invalid());
        }
        let assignment = text(self.target(), "assignment")?;
        let admission = registry
            .receipts()?
            .into_iter()
            .find(|r| r.work.assignment.id == assignment);
        let Some(admission) = admission else {
            return Ok(Json::Null);
        };
        if identity(&registry.coordinator, &registry.objective, &admission.work) != *self.target() {
            return Err(invalid());
        }
        Ok(
            match registry.saved_result_offer(assignment, text(&self.query.body, "checkpoint")?)? {
                Some(offer) => Json::text(offer.encode()),
                None => Json::Null,
            },
        )
    }
    /// Recheck retained facts after signing; no result capture, ledger write or slot release.
    pub fn reply(
        &self,
        registry: &RemoteAdmissionRegistry,
        sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
    ) -> Result<RemoteFrame, Error> {
        fresh(self.context(), now()?)?;
        let offer = self.offer(registry)?;
        let body = Json::object([
            (
                "query",
                Json::text(Blake3::digest_bytes(self.query.body.encode().as_bytes()).to_string()),
            ),
            ("observed_ms", Json::Number(now()?)),
            ("offer", offer.clone()),
        ]);
        let message = payload(REPLY_SIGNING, &body);
        let signature = sign(&message).map_err(|_| invalid())?;
        Ed25519::verify(&self.worker, message.as_bytes(), &signature).map_err(|_| invalid())?;
        fresh(self.context(), now()?)?;
        if self.offer(registry)? != offer {
            return Err(invalid());
        }
        let encoded = envelope(REPLY_SCHEMA, body, &signature);
        if encoded.len() > MAX {
            return Err(invalid());
        }
        Ok(RemoteFrame::Control(encoded.into_bytes()))
    }
}
/// Read a known result offer over one bounded native SSH connection, without automatic retries.
pub fn inspect_remote_saved_result_over_ssh(
    destination: &NativeSshDestination,
    request: crate::fleet::RemoteWorkerStatusRequest<'_>,
    checkpoint: &str,
    budget: Duration,
    sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
) -> io::Result<Option<RemoteSavedResultOffer>> {
    let err = |_| io::Error::other("remote saved result unavailable");
    let runtime = request.runtime;
    let challenge = RemoteSavedResultChallenge::issue(
        crate::fleet::RemoteWorkerStatusRequest {
            runtime,
            lane: request.lane,
            run: request.run,
            coordinator: request.coordinator,
            worker: request.worker,
        },
        checkpoint,
    )
    .map_err(err)?;
    let query = challenge.signed_query(runtime, sign).map_err(err)?;
    let mut connection = destination.connect(budget)?;
    let (input, output) = connection.streams()?;
    RemoteFrameWriter::new(output).write_frame(&query.frame().map_err(err)?)?;
    let Some(RemoteFrame::Control(reply)) = RemoteFrameReader::new(input).read_frame()? else {
        return Err(io::Error::other("remote saved result unavailable"));
    };
    challenge
        .verify_reply(
            runtime,
            std::str::from_utf8(&reply)
                .map_err(|_| io::Error::other("remote saved result unavailable"))?,
        )
        .map_err(err)
}

#[cfg(test)]
#[path = "query/tests.rs"]
mod tests;
