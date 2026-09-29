//! Fresh authenticated immutable result transfer. No execution or import authority.
use super::*;
const QUERY_SCHEMA: &str = "mesh.worker-result-transfer-query/v1";
const REPLY_SCHEMA: &str = "mesh.worker-result-transfer-reply/v1";
const QUERY_SIGNING: DomainSeparator = DomainSeparator::new("mesh.v1.worker-result-transfer-query");
const REPLY_SIGNING: DomainSeparator = DomainSeparator::new("mesh.v1.worker-result-transfer-reply");

/// Single-use query bound to an exact current assignment and known checkpoint identity.
pub struct RemoteResultTransferChallenge {
    context: RemoteWorkerStatusChallenge,
    body: Json,
}
impl RemoteResultTransferChallenge {
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
    ) -> Result<RemoteResultTransferQuery, Error> {
        self.context.context(runtime, now()?)?;
        let message = payload(QUERY_SIGNING, &self.body);
        let signature = sign(&message).map_err(|_| invalid())?;
        Ed25519::verify(&self.context.coordinator, message.as_bytes(), &signature)
            .map_err(|_| invalid())?;
        self.context.context(runtime, now()?)?;
        Ok(RemoteResultTransferQuery {
            body: self.body.clone(),
            signature,
        })
    }
    /// Verify the fresh signed header. Absence is unknown, never permission for a new attempt.
    pub fn verify_reply(
        &self,
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
pub struct RemoteResultTransferQuery {
    body: Json,
    signature: Signature,
}
impl RemoteResultTransferQuery {
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
    ) -> Result<VerifiedRemoteResultTransferQuery, Error> {
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
        Ok(VerifiedRemoteResultTransferQuery {
            query: self,
            limits,
            worker: policy.worker,
        })
    }
}
/// Native policy verified read-only access to one original assignment's known checkpoint.
pub struct VerifiedRemoteResultTransferQuery {
    query: RemoteResultTransferQuery,
    limits: Limits,
    worker: PublicKey,
}
impl VerifiedRemoteResultTransferQuery {
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

fn transfer_error() -> io::Error {
    io::Error::other("remote saved result transfer unavailable")
}
fn chunks(manifest: &RemoteInputManifest) -> std::collections::BTreeMap<mesh_cas::Digest32, u64> {
    manifest
        .entries()
        .iter()
        .flat_map(|entry| match entry {
            crate::fleet::RemoteInputEntry::File { chunks, .. } => chunks.as_slice(),
            _ => &[],
        })
        .map(|chunk| (chunk.digest, chunk.bytes))
        .collect()
}
fn control(value: Json) -> RemoteFrame {
    RemoteFrame::Control(value.encode().into_bytes())
}
fn end() -> Json {
    Json::object([("schema", Json::text("mesh.worker-result-transfer-end/v1"))])
}
impl VerifiedRemoteResultTransferQuery {
    /// Serve one fresh read-only transfer from an exact native reopened saved review. The stream
    /// owner supplies authenticated transport and I/O deadlines. End means only peer observation,
    /// never durable import, process completion or release of execution capacity.
    pub fn serve<R: Read, W: Write>(
        &self,
        registry: &RemoteAdmissionRegistry,
        destination: &crate::fleet::RemoteInputDestination,
        mut input: R,
        mut output: W,
        sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
    ) -> io::Result<()> {
        let map = |_| transfer_error();
        fresh(self.context(), now().map_err(map)?).map_err(map)?;
        let expected = self.offer(registry).map_err(map)?;
        let (offer, source) = registry
            .reopen_saved_result(
                destination,
                text(self.target(), "assignment").map_err(map)?,
                text(&self.query.body, "checkpoint").map_err(map)?,
                &crate::TrustedReviewers::default(),
            )
            .map_err(map)?
            .ok_or_else(transfer_error)?;
        if expected != Json::text(offer.encode()) {
            return Err(transfer_error());
        }
        let reply = self.reply(registry, sign).map_err(map)?;
        source.verify_roots()?;
        let mut writer = RemoteFrameWriter::new(&mut output);
        writer.write_frame(&reply)?;
        writer.write_frame(&RemoteFrame::Manifest(
            source.manifest().encoded().as_bytes().to_vec(),
        ))?;
        let declared = chunks(source.manifest());
        let mut previous = None;
        let mut reader = RemoteFrameReader::new(&mut input);
        loop {
            let Some(RemoteFrame::Control(raw)) = reader.read_frame()? else {
                return Err(transfer_error());
            };
            fresh(self.context(), now().map_err(map)?).map_err(map)?;
            source.verify_roots()?;
            if self.offer(registry).map_err(map)? != expected {
                return Err(transfer_error());
            }
            let raw = std::str::from_utf8(&raw).map_err(|_| transfer_error())?;
            if raw == end().encode() {
                return Ok(());
            }
            let parsed = Json::parse(raw).map_err(|_| transfer_error())?;
            let body = closed(&parsed, &["digest", "offset"]).map_err(map)?;
            let digest = mesh_cas::Digest32::from_bytes(
                bytes::<32>(text(&body, "digest").map_err(map)?).map_err(map)?,
            );
            let offset = number(&body, "offset").map_err(map)?;
            let size = *declared.get(&digest).ok_or_else(transfer_error)?;
            if raw != body.encode() || previous.is_some_and(|p| p >= digest) || offset >= size {
                return Err(transfer_error());
            }
            previous = Some(digest);
            // Hash once, then retain at most one 4 MiB chunk while emitting bounded parts.
            let bytes = source.read_chunk(digest)?;
            let mut position = offset as usize;
            while position < bytes.len() {
                fresh(self.context(), now().map_err(map)?).map_err(map)?;
                source.verify_roots()?;
                let next = (position + 65_536).min(bytes.len());
                writer.write_frame(&RemoteFrame::Chunk {
                    digest,
                    offset: position as u64,
                    final_part: next == bytes.len(),
                    bytes: bytes[position..next].to_vec(),
                })?;
                position = next;
            }
        }
    }
}

/// Receive one known signed saved result through already authenticated, deadline-bound streams.
/// Partial offsets survive disconnect. Success retains a durable content receipt; this is not
/// process completion, candidate import, a retention pin or permission to retry work.
pub fn receive_remote_saved_result<R: Read, W: Write>(
    destination: &crate::fleet::RemoteInputDestination,
    request: crate::fleet::RemoteWorkerStatusRequest<'_>,
    checkpoint: &str,
    input: R,
    output: W,
    sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
) -> io::Result<RemoteSavedResultOffer> {
    receive_remote_saved_result_selected(
        destination,
        request,
        checkpoint,
        None,
        input,
        output,
        sign,
    )
}

pub(in crate::fleet) fn receive_remote_saved_result_selected<R: Read, W: Write>(
    destination: &crate::fleet::RemoteInputDestination,
    request: crate::fleet::RemoteWorkerStatusRequest<'_>,
    checkpoint: &str,
    expected: Option<&str>,
    mut input: R,
    mut output: W,
    sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
) -> io::Result<RemoteSavedResultOffer> {
    let map = |_| transfer_error();
    let runtime = request.runtime;
    let challenge = RemoteResultTransferChallenge::issue(
        crate::fleet::RemoteWorkerStatusRequest {
            runtime,
            lane: request.lane,
            run: request.run,
            coordinator: request.coordinator,
            worker: request.worker,
        },
        checkpoint,
    )
    .map_err(map)?;
    let query = challenge.signed_query(runtime, sign).map_err(map)?;
    let mut writer = RemoteFrameWriter::new(&mut output);
    let mut reader = RemoteFrameReader::new(&mut input);
    writer.write_frame(&query.frame().map_err(map)?)?;
    let Some(RemoteFrame::Control(reply)) = reader.read_frame()? else {
        return Err(transfer_error());
    };
    let offer = challenge
        .verify_reply(
            runtime,
            std::str::from_utf8(&reply).map_err(|_| transfer_error())?,
        )
        .map_err(map)?
        .ok_or_else(transfer_error)?;
    if expected.is_some_and(|selected| offer.encode() != selected) {
        return Err(transfer_error());
    }
    let Some(RemoteFrame::Manifest(raw)) = reader.read_frame()? else {
        return Err(transfer_error());
    };
    challenge
        .context
        .context(runtime, now().map_err(map)?)
        .map_err(map)?;
    let digest = |field| {
        RecordDigest::parse_hex(text(&offer.body, field).map_err(map)?)
            .map_err(|_| transfer_error())
    };
    let manifest = RemoteInputManifest::decode(
        std::str::from_utf8(&raw).map_err(|_| transfer_error())?,
        digest("version")?,
        digest("manifest")?,
    )
    .map_err(map)?;
    let declared = chunks(&manifest);
    let mut receiver = crate::fleet::NativeRemoteResultReceiver::new(
        destination,
        manifest,
        &offer.encode(),
        crate::fleet::RemoteWorkerStatusRequest {
            runtime,
            lane: request.lane,
            run: request.run,
            coordinator: request.coordinator,
            worker: request.worker,
        },
    )
    .map_err(map)?;
    for (digest, size) in declared {
        challenge
            .context
            .context(runtime, now().map_err(map)?)
            .map_err(map)?;
        let (mut offset, complete) = receiver.status(runtime, digest).map_err(map)?;
        if complete {
            continue;
        }
        writer.write_frame(&control(Json::object([
            ("digest", Json::text(digest.to_string())),
            ("offset", Json::Number(offset)),
        ])))?;
        while offset < size {
            let Some(RemoteFrame::Chunk {
                digest: received,
                offset: start,
                final_part,
                bytes,
            }) = reader.read_frame()?
            else {
                return Err(transfer_error());
            };
            challenge
                .context
                .context(runtime, now().map_err(map)?)
                .map_err(map)?;
            if received != digest || start != offset {
                return Err(transfer_error());
            }
            receiver
                .accept(runtime, digest, offset, &bytes, final_part)
                .map_err(map)?;
            offset += bytes.len() as u64;
        }
    }
    challenge
        .context
        .context(runtime, now().map_err(map)?)
        .map_err(map)?;
    receiver.record_content_receipt(runtime).map_err(map)?;
    writer.write_frame(&control(end()))?;
    challenge
        .context
        .context(runtime, now().map_err(map)?)
        .map_err(map)?;
    Ok(offer)
}

/// One bounded native SSH result transfer, without automatic reconnection or authorization renewal.
pub fn receive_remote_saved_result_over_ssh(
    peer: &NativeSshDestination,
    destination: &crate::fleet::RemoteInputDestination,
    request: crate::fleet::RemoteWorkerStatusRequest<'_>,
    checkpoint: &str,
    budget: Duration,
    sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
) -> io::Result<RemoteSavedResultOffer> {
    let mut connection = peer.connect(budget)?;
    let (input, output) = connection.streams()?;
    receive_remote_saved_result(destination, request, checkpoint, input, output, sign)
}

#[cfg(test)]
#[path = "transfer/tests.rs"]
mod tests;
