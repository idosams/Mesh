//! Fresh authenticated discovery of a bounded saved-result catalog page. No content transfer.
use super::catalog::RemoteSavedResultPage;
use super::*;
const QUERY_SCHEMA: &str = "mesh.worker-result-discovery-query/v1";
const REPLY_SCHEMA: &str = "mesh.worker-result-discovery-reply/v1";
const QUERY_SIGNING: DomainSeparator =
    DomainSeparator::new("mesh.v1.worker-result-discovery-query");
const REPLY_SIGNING: DomainSeparator =
    DomainSeparator::new("mesh.v1.worker-result-discovery-reply");

/// Single-use discovery query bound to an exact current assignment and catalog revision cursor.
pub struct RemoteResultDiscoveryChallenge {
    context: RemoteWorkerStatusChallenge,
    body: Json,
}
impl RemoteResultDiscoveryChallenge {
    /// Expired work remains readable; discovery does not renew leases or prove completion.
    pub fn issue(
        request: crate::fleet::RemoteWorkerStatusRequest<'_>,
        after: u64,
    ) -> Result<Self, Error> {
        if after > 4096 {
            return Err(invalid());
        }
        let context = RemoteWorkerStatusChallenge::issue(
            request.runtime,
            request.lane,
            request.run,
            request.coordinator,
            request.worker,
        )?;
        let body = Json::object([
            ("query", context.body.clone()),
            ("after", Json::Number(after)),
        ]);
        Ok(Self { context, body })
    }
    /// Sign the prepared exact query under its own domain, with context checks around the callback.
    pub fn signed_query(
        &self,
        runtime: &mut Runtime,
        sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
    ) -> Result<RemoteResultDiscoveryQuery, Error> {
        self.context.context(runtime, now()?)?;
        let message = payload(QUERY_SIGNING, &self.body);
        let signature = sign(&message).map_err(|_| invalid())?;
        Ed25519::verify(&self.context.coordinator, message.as_bytes(), &signature)
            .map_err(|_| invalid())?;
        self.context.context(runtime, now()?)?;
        Ok(RemoteResultDiscoveryQuery {
            body: self.body.clone(),
            signature,
        })
    }
    /// Consume a fresh signed observation. Absence is unknown, never permission for a new attempt.
    pub fn verify_reply(
        self,
        runtime: &mut Runtime,
        encoded: &str,
    ) -> Result<Option<RemoteSavedResultPage>, Error> {
        self.context.context(runtime, now()?)?;
        let outer = parse_envelope(encoded, REPLY_SCHEMA)?;
        let body = closed(
            outer.get("body").ok_or_else(invalid)?,
            &["query", "observed_ms", "page"],
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
        let page = decode_page(
            body.get("page").ok_or_else(invalid)?,
            number(&self.body, "after")?,
            self.context.body.get("target").ok_or_else(invalid)?,
        )?;
        self.context.context(runtime, now()?)?;
        Ok(page)
    }
}
/// Canonical private query bytes; parsing alone provides no access to a worker ledger.
pub struct RemoteResultDiscoveryQuery {
    body: Json,
    signature: Signature,
}
impl RemoteResultDiscoveryQuery {
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
        let raw = closed(outer.get("body").ok_or_else(invalid)?, &["query", "after"])?;
        let query = query_body(raw.get("query").ok_or_else(invalid)?)?;
        let after = number(&raw, "after")?;
        if after > 4096 {
            return Err(invalid());
        }
        let body = Json::object([("query", query), ("after", Json::Number(after))]);
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
    ) -> Result<VerifiedRemoteResultDiscoveryQuery, Error> {
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
        Ok(VerifiedRemoteResultDiscoveryQuery {
            query: self,
            limits,
            worker: policy.worker,
        })
    }
}
/// Native policy verified read-only access to one original assignment's result catalog.
pub struct VerifiedRemoteResultDiscoveryQuery {
    query: RemoteResultDiscoveryQuery,
    limits: Limits,
    worker: PublicKey,
}
impl VerifiedRemoteResultDiscoveryQuery {
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
    fn page(&self, registry: &RemoteAdmissionRegistry) -> Result<Json, Error> {
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
            match registry.saved_result_page(assignment, number(&self.query.body, "after")?)? {
                Some(page) => encode_page(page),
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
        let page = self.page(registry)?;
        let body = Json::object([
            (
                "query",
                Json::text(Blake3::digest_bytes(self.query.body.encode().as_bytes()).to_string()),
            ),
            ("observed_ms", Json::Number(now()?)),
            ("page", page.clone()),
        ]);
        let message = payload(REPLY_SIGNING, &body);
        let signature = sign(&message).map_err(|_| invalid())?;
        Ed25519::verify(&self.worker, message.as_bytes(), &signature).map_err(|_| invalid())?;
        fresh(self.context(), now()?)?;
        if self.page(registry)? != page {
            return Err(invalid());
        }
        let encoded = envelope(REPLY_SCHEMA, body, &signature);
        if encoded.len() > MAX {
            return Err(invalid());
        }
        Ok(RemoteFrame::Control(encoded.into_bytes()))
    }
}
/// Discover one result catalog page over a bounded native SSH connection, without automatic retries.
pub fn discover_remote_saved_results_over_ssh(
    destination: &NativeSshDestination,
    request: crate::fleet::RemoteWorkerStatusRequest<'_>,
    after: u64,
    budget: Duration,
    sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
) -> io::Result<Option<RemoteSavedResultPage>> {
    let err = |_| io::Error::other("remote result discovery unavailable");
    let runtime = request.runtime;
    let challenge = RemoteResultDiscoveryChallenge::issue(
        crate::fleet::RemoteWorkerStatusRequest {
            runtime,
            lane: request.lane,
            run: request.run,
            coordinator: request.coordinator,
            worker: request.worker,
        },
        after,
    )
    .map_err(err)?;
    let query = challenge.signed_query(runtime, sign).map_err(err)?;
    let mut connection = destination.connect(budget)?;
    let (input, output) = connection.streams()?;
    RemoteFrameWriter::new(output).write_frame(&query.frame().map_err(err)?)?;
    let Some(RemoteFrame::Control(reply)) = RemoteFrameReader::new(input).read_frame()? else {
        return Err(io::Error::other("remote result discovery unavailable"));
    };
    challenge
        .verify_reply(
            runtime,
            std::str::from_utf8(&reply)
                .map_err(|_| io::Error::other("remote result discovery unavailable"))?,
        )
        .map_err(err)
}

fn encode_page(page: RemoteSavedResultPage) -> Json {
    Json::object([
        ("revision", Json::Number(page.revision)),
        ("after", Json::Number(page.after)),
        ("has_more", Json::Bool(page.has_more)),
        (
            "offers",
            Json::Array(
                page.offers
                    .into_iter()
                    .map(|o| Json::text(o.encode()))
                    .collect(),
            ),
        ),
    ])
}
fn decode_page(
    value: &Json,
    before: u64,
    target: &Json,
) -> Result<Option<RemoteSavedResultPage>, Error> {
    if *value == Json::Null {
        return Ok(None);
    }
    let page = closed(value, &["revision", "after", "has_more", "offers"])?;
    let revision = number(&page, "revision")?;
    let after = number(&page, "after")?;
    let has_more = page
        .get("has_more")
        .and_then(Json::as_bool)
        .ok_or_else(invalid)?;
    let rows = page
        .get("offers")
        .and_then(Json::as_array)
        .ok_or_else(invalid)?;
    if revision > 4096
        || before > after
        || after > revision
        || rows.len() > 16
        || after - before != rows.len() as u64
        || has_more != (after < revision)
        || rows.len() as u64 != (revision - before).min(16)
    {
        return Err(invalid());
    }
    let mut offers = Vec::new();
    let mut checkpoints = std::collections::BTreeSet::new();
    for row in rows {
        let offer = RemoteSavedResultOffer::decode(row.as_text().ok_or_else(invalid)?)?;
        if offer.body.get("target") != Some(target)
            || !checkpoints.insert(text(&offer.body, "checkpoint")?.to_owned())
        {
            return Err(invalid());
        }
        offers.push(offer);
    }
    Ok(Some(RemoteSavedResultPage {
        revision,
        after,
        has_more,
        offers,
    }))
}

#[cfg(test)]
#[path = "discovery/tests.rs"]
mod tests;
