//! Fresh authenticated bounded result correspondence; no import or execution authority.
use super::*;
const QUERY_SCHEMA: &str = "mesh.worker-result-evidence-query/v1";
const REPLY_SCHEMA: &str = "mesh.worker-result-evidence-reply/v1";
const QUERY_SIGNING: DomainSeparator = DomainSeparator::new("mesh.v1.worker-result-evidence-query");
const REPLY_SIGNING: DomainSeparator = DomainSeparator::new("mesh.v1.worker-result-evidence-reply");

/// Single-use query bound to an exact current assignment and known checkpoint identity.
pub struct RemoteResultEvidenceChallenge {
    context: RemoteWorkerStatusChallenge,
    body: Json,
}
impl RemoteResultEvidenceChallenge {
    /// Expired work remains readable; this does not renew a lease or discover checkpoint names.
    pub fn issue(
        request: crate::fleet::RemoteWorkerStatusRequest<'_>,
        checkpoint: &str,
        offer_digest: &str,
    ) -> Result<Self, Error> {
        id_valid(checkpoint)?;
        bytes::<32>(offer_digest)?;
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
            ("offer_digest", Json::text(offer_digest)),
        ]);
        Ok(Self { context, body })
    }
    /// Sign the prepared exact query under its own domain, with context checks around the callback.
    pub fn signed_query(
        &self,
        runtime: &mut Runtime,
        sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
    ) -> Result<RemoteResultEvidenceQuery, Error> {
        self.context.context(runtime, now()?)?;
        let message = payload(QUERY_SIGNING, &self.body);
        let signature = sign(&message).map_err(|_| invalid())?;
        Ed25519::verify(&self.context.coordinator, message.as_bytes(), &signature)
            .map_err(|_| invalid())?;
        self.context.context(runtime, now()?)?;
        Ok(RemoteResultEvidenceQuery {
            body: self.body.clone(),
            signature,
        })
    }
    /// Consume a fresh signed observation. Absence is unknown, never permission for a new attempt.
    pub fn verify_reply(
        &self,
        runtime: &mut Runtime,
        encoded: &str,
    ) -> Result<(RemoteSavedResultOffer, RecordDigest, u64), Error> {
        self.context.context(runtime, now()?)?;
        let outer = parse_envelope(encoded, REPLY_SCHEMA)?;
        let body = closed(
            outer.get("body").ok_or_else(invalid)?,
            &["query", "observed_ms", "offer", "evidence", "bytes"],
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
        let offer = RemoteSavedResultOffer::decode(text(&body, "offer")?)?;
        if offer.body.get("target") != self.context.body.get("target")
            || text(&offer.body, "checkpoint")? != text(&self.body, "checkpoint")?
            || Blake3::digest_bytes(offer.encode().as_bytes()).to_string()
                != text(&self.body, "offer_digest")?
        {
            return Err(invalid());
        }
        let digest = RecordDigest::parse_hex(text(&body, "evidence")?).map_err(|_| invalid())?;
        let length = number(&body, "bytes")?;
        if !(1..=4_194_304).contains(&length) {
            return Err(invalid());
        }
        self.context.context(runtime, now()?)?;
        Ok((offer, digest, length))
    }
}
/// Canonical private query bytes; parsing alone provides no access to a worker ledger.
pub struct RemoteResultEvidenceQuery {
    body: Json,
    signature: Signature,
}
impl RemoteResultEvidenceQuery {
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
            &["query", "checkpoint", "offer_digest"],
        )?;
        let query = query_body(raw.get("query").ok_or_else(invalid)?)?;
        let checkpoint = text(&raw, "checkpoint")?;
        id_valid(checkpoint)?;
        let offer_digest = text(&raw, "offer_digest")?;
        bytes::<32>(offer_digest)?;
        let body = Json::object([
            ("query", query),
            ("checkpoint", Json::text(checkpoint)),
            ("offer_digest", Json::text(offer_digest)),
        ]);
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
    ) -> Result<VerifiedRemoteResultEvidenceQuery, Error> {
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
        Ok(VerifiedRemoteResultEvidenceQuery {
            query: self,
            limits,
            worker: policy.worker,
        })
    }
}
/// Native policy verified read-only access to one original assignment's known checkpoint.
pub struct VerifiedRemoteResultEvidenceQuery {
    query: RemoteResultEvidenceQuery,
    limits: Limits,
    worker: PublicKey,
}
impl VerifiedRemoteResultEvidenceQuery {
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
                Some(offer) => {
                    if Blake3::digest_bytes(offer.encode().as_bytes()).to_string()
                        != text(&self.query.body, "offer_digest")?
                    {
                        return Err(invalid());
                    }
                    Json::text(offer.encode())
                }
                None => Json::Null,
            },
        )
    }
    /// Recheck retained facts after signing; no result capture, ledger write or slot release.
    fn reply(
        &self,
        registry: &RemoteAdmissionRegistry,
        correspondence: &crate::fleet::RemoteResultCorrespondence,
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
            ("evidence", Json::text(correspondence.digest().to_string())),
            ("bytes", Json::Number(correspondence.encoded().len() as u64)),
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

fn unavailable() -> io::Error {
    io::Error::other("remote result evidence unavailable")
}
impl VerifiedRemoteResultEvidenceQuery {
    pub(in crate::fleet) fn serve<W: Write>(
        &self,
        registry: &RemoteAdmissionRegistry,
        destination: &crate::fleet::RemoteInputDestination,
        mut output: W,
        sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
    ) -> io::Result<()> {
        let map = |_| unavailable();
        fresh(self.context(), now().map_err(map)?).map_err(map)?;
        let expected = self.offer(registry).map_err(map)?;
        let export = registry
            .reopen_saved_result_with_correspondence(
                destination,
                text(self.target(), "assignment").map_err(map)?,
                text(&self.query.body, "checkpoint").map_err(map)?,
                &crate::TrustedReviewers::default(),
            )
            .map_err(map)?
            .ok_or_else(unavailable)?;
        if expected != Json::text(export.offer.encode()) {
            return Err(unavailable());
        }
        let header = self
            .reply(registry, &export.correspondence, sign)
            .map_err(map)?;
        export.source.verify_roots()?;
        let mut writer = RemoteFrameWriter::new(&mut output);
        writer.write_frame(&header)?;
        let bytes = export.correspondence.encoded().as_bytes();
        let digest = mesh_cas::Digest32::from_bytes(*export.correspondence.digest().as_bytes());
        for (index, part) in bytes.chunks(65_536).enumerate() {
            fresh(self.context(), now().map_err(map)?).map_err(map)?;
            export.source.verify_roots()?;
            if self.offer(registry).map_err(map)? != expected {
                return Err(unavailable());
            }
            let offset = index * 65_536;
            writer.write_frame(&RemoteFrame::Chunk {
                digest,
                offset: offset as u64,
                final_part: offset + part.len() == bytes.len(),
                bytes: part.to_vec(),
            })?;
        }
        export.source.verify_roots()?;
        fresh(self.context(), now().map_err(map)?).map_err(map)
    }
}
/// Native expected manifests and original signed offer for a fresh correspondence request.
pub struct RemoteResultEvidenceRequest<'a> {
    /// Current exact coordinator assignment and configured peer keys.
    pub status: crate::fleet::RemoteWorkerStatusRequest<'a>,
    /// Original durable worker-signed result offer, never a renderer-selected path.
    pub offer: &'a str,
    /// Complete original input manifest, verified against the assignment.
    pub input: &'a RemoteInputManifest,
    /// Complete saved result manifest, verified against the signed offer.
    pub result: &'a RemoteInputManifest,
}
/// Fresh authenticated evidence for one exact offer. This is not persisted import authority.
pub struct AuthenticatedRemoteResultEvidence {
    correspondence: crate::fleet::RemoteResultCorrespondence,
    offer_digest: RecordDigest,
}
impl AuthenticatedRemoteResultEvidence {
    /// Exact original signed-offer identity bound by the authenticated query and reply.
    pub fn offer_digest(&self) -> RecordDigest {
        self.offer_digest
    }
    /// Complete validated private evidence; import still needs native original-project checks.
    pub fn correspondence(&self) -> &crate::fleet::RemoteResultCorrespondence {
        &self.correspondence
    }
}
/// Receive one bounded fresh metadata response through authenticated deadline-bound streams.
/// EOF, partial frames, replay and mismatched content refuse. No CAS/import state is written.
pub fn receive_remote_result_evidence<R: Read, W: Write>(
    request: RemoteResultEvidenceRequest<'_>,
    mut input: R,
    mut output: W,
    sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
) -> io::Result<AuthenticatedRemoteResultEvidence> {
    let map = |_| unavailable();
    let status = request.status;
    let runtime = status.runtime;
    let offer = RemoteSavedResultOffer::verify(
        runtime,
        status.lane,
        status.run,
        status.coordinator,
        status.worker,
        request.offer,
        request.result,
    )
    .map_err(map)?;
    let target = offer.body.get("target").ok_or_else(unavailable)?;
    if text(target, "input").map_err(map)? != request.input.input().to_string()
        || text(target, "bundle").map_err(map)? != request.input.bundle().to_string()
    {
        return Err(unavailable());
    }
    let offer_digest =
        RecordDigest::from_bytes(*Blake3::digest_bytes(request.offer.as_bytes()).as_bytes());
    let challenge = RemoteResultEvidenceChallenge::issue(
        crate::fleet::RemoteWorkerStatusRequest {
            runtime,
            lane: status.lane,
            run: status.run,
            coordinator: status.coordinator,
            worker: status.worker,
        },
        text(&offer.body, "checkpoint").map_err(map)?,
        &offer_digest.to_string(),
    )
    .map_err(map)?;
    let query = challenge.signed_query(runtime, sign).map_err(map)?;
    RemoteFrameWriter::new(&mut output).write_frame(&query.frame().map_err(map)?)?;
    let mut reader = RemoteFrameReader::new(&mut input);
    let Some(RemoteFrame::Control(raw)) = reader.read_frame()? else {
        return Err(unavailable());
    };
    let (observed, digest, length) = challenge
        .verify_reply(
            runtime,
            std::str::from_utf8(&raw).map_err(|_| unavailable())?,
        )
        .map_err(map)?;
    if observed.encode() != request.offer {
        return Err(unavailable());
    }
    let mut bytes = Vec::with_capacity(length as usize);
    while bytes.len() < length as usize {
        let Some(RemoteFrame::Chunk {
            digest: actual,
            offset,
            final_part,
            bytes: part,
        }) = reader.read_frame()?
        else {
            return Err(unavailable());
        };
        challenge
            .context
            .context(runtime, now().map_err(map)?)
            .map_err(map)?;
        if actual.as_bytes() != digest.as_bytes()
            || offset != bytes.len() as u64
            || part.len() > length as usize - bytes.len()
            || final_part != (bytes.len() + part.len() == length as usize)
        {
            return Err(unavailable());
        }
        bytes.extend_from_slice(&part);
    }
    if Blake3::digest_bytes(&bytes).as_bytes() != digest.as_bytes() {
        return Err(unavailable());
    }
    let initial = RecordDigest::parse_hex(text(&offer.body, "initial").map_err(map)?)
        .map_err(|_| unavailable())?;
    let correspondence = crate::fleet::RemoteResultCorrespondence::decode_bound(
        std::str::from_utf8(&bytes).map_err(|_| unavailable())?,
        request.input,
        request.result,
        initial,
    )?;
    challenge
        .context
        .context(runtime, now().map_err(map)?)
        .map_err(map)?;
    Ok(AuthenticatedRemoteResultEvidence {
        correspondence,
        offer_digest,
    })
}
/// One native SSH metadata request, without automatic retries or lease renewal.
pub fn receive_remote_result_evidence_over_ssh(
    peer: &NativeSshDestination,
    request: RemoteResultEvidenceRequest<'_>,
    budget: Duration,
    sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
) -> io::Result<AuthenticatedRemoteResultEvidence> {
    let mut connection = peer.connect(budget)?;
    let (input, output) = connection.streams()?;
    receive_remote_result_evidence(request, input, output, sign)
}

#[cfg(test)]
#[path = "evidence/tests.rs"]
mod tests;
