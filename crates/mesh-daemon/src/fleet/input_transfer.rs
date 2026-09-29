//! Coordinator-side immutable input transfer over already authenticated, bounded streams.
use super::{
    Lane, Limits, RemoteAdmissionProof, RemoteFrame, RemoteFrameReader, RemoteFrameWriter,
    RemoteInputEntry, RemoteInputSource, RemoteReceivingCommand, Runtime,
};
use crate::ipc::Json;
use mesh_crypto::SigningPayload;
use mesh_types::{PublicKey, Signature};
use std::io::{self, Read, Write};

/// Native-selected attempt and immutable source. Streams must already belong to the admitted worker.
/// Neither this configuration nor a receiving reply grants worker execution or protected-main authority.
pub struct RemoteInputTransferRequest<'a> {
    /// Current native coordinator ledger; the worker proof must already have claimed this attempt.
    pub runtime: &'a mut Runtime,
    /// Exact lane selected by native policy.
    pub lane: &'a str,
    /// Exact current run selected by native policy.
    pub run: &'a str,
    /// Saved-history export, never the mutable project folder.
    pub source: &'a RemoteInputSource,
    /// Independently configured coordinator key.
    pub coordinator: PublicKey,
    /// Independently configured worker key, not a value chosen by the reply.
    pub worker: PublicKey,
}

/// Correlated receiving facts only. These unsigned facts are not a launch grant or a saved result.
pub struct RemoteInputTransferReceipt(Json);
impl RemoteInputTransferReceipt {
    /// Coordinator, objective, lane/run, assignment, input/bundle, allocation and admission revision.
    pub fn correlation(&self) -> &Json {
        &self.0
    }
}
/// Disposition of one serial transfer connection. Failure/EOF never implies completion or safe retry.
pub enum RemoteInputTransferOutcome {
    /// The peer acknowledged input materialization. This does not establish provider startup.
    Materialized(RemoteInputTransferReceipt),
    /// The peer retains admission facts but cannot receive using this connection. Reconcile natively.
    Retained(RemoteInputTransferReceipt),
}

/// Authenticate only the exact native attempt, send immutable input with bounded backpressure and
/// resume from verified, correlated peer offsets. Native context and source identity are rechecked
/// around every exchange. No automatic retry, launch, assignment mutation or shared publication occurs.
///
/// The owner supplies authenticated transport, read/write deadlines, cancellation and bounded stderr.
/// Every error ends this connection; close both streams and reconcile uncertainty before reconnecting.
/// The signer receives only a fresh proof already compared to current native work, never arbitrary bytes.
pub fn transfer_remote_input<R: Read, W: Write>(
    request: RemoteInputTransferRequest<'_>,
    input: R,
    output: W,
    sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
) -> io::Result<RemoteInputTransferOutcome> {
    let mut reader = RemoteFrameReader::new(input);
    let mut writer = RemoteFrameWriter::new(output);
    let bytes = control(&mut reader)?;
    let proof = RemoteAdmissionProof::decode(std::str::from_utf8(&bytes).map_err(|_| refused())?)
        .map_err(|_| refused())?;
    let payload = proof
        .signing_payload_for(
            request.runtime,
            request.lane,
            request.run,
            &request.coordinator,
            &request.worker,
        )
        .map_err(|_| refused())?;
    let lane = request
        .runtime
        .state()
        .lanes
        .get(request.lane)
        .cloned()
        .ok_or_else(refused)?;
    let assignment = lane
        .runs
        .last()
        .and_then(|run| run.remote.as_ref())
        .ok_or_else(refused)?;
    if request.source.manifest().input() != assignment.input
        || request.source.manifest().bundle() != assignment.bundle
    {
        return Err(refused());
    }
    let limits = request.runtime.state().limits.clone().ok_or_else(refused)?;
    let mut context = Context {
        request,
        lane,
        limits,
    };
    context.check()?;
    let signature = sign(&payload).map_err(|_| refused())?;
    let proof_json = Json::parse(&proof.encode()).map_err(|_| refused())?;
    let admission = proof_json.get("admission").ok_or_else(refused)?;
    context.check()?;
    writer.write_frame(
        &RemoteReceivingCommand::Authenticate {
            request: "authenticate".into(),
            signature,
        }
        .frame()?,
    )?;
    let bytes = control(&mut reader)?;
    let value = parse(&bytes)?;
    let revision = value
        .get("admission")
        .and_then(|v| v.get("revision"))
        .and_then(Json::as_u64)
        .filter(|r| *r > 0)
        .ok_or_else(refused)?;
    let mut fields = Vec::new();
    for key in [
        "coordinator",
        "objective",
        "lane",
        "run",
        "assignment",
        "input",
        "bundle",
        "allocation",
    ] {
        fields.push((key, admission.get(key).cloned().ok_or_else(refused)?));
    }
    fields.push(("revision", Json::Number(revision)));
    let receipt = RemoteInputTransferReceipt(Json::object(fields));
    let access = value
        .get("detail")
        .and_then(Json::as_text)
        .ok_or_else(refused)?;
    if !matches!(access, "receiving" | "retained") {
        return Err(refused());
    }
    verify_reply(
        &bytes,
        &receipt,
        "authenticated",
        Some("authenticate"),
        Json::text(access),
    )?;
    context.check()?;
    if access == "retained" {
        return Ok(RemoteInputTransferOutcome::Retained(receipt));
    }
    writer.write_frame(&RemoteFrame::Manifest(
        context
            .request
            .source
            .manifest()
            .encoded()
            .as_bytes()
            .to_vec(),
    ))?;
    verify_reply(
        &control(&mut reader)?,
        &receipt,
        "manifest",
        None,
        Json::Null,
    )?;
    context.check()?;
    let mut chunks = std::collections::BTreeMap::new();
    for entry in context.request.source.manifest().entries() {
        if let RemoteInputEntry::File { chunks: parts, .. } = entry {
            for part in parts {
                chunks.insert(part.digest, part.bytes);
            }
        }
    }
    for (index, (digest, length)) in chunks.into_iter().enumerate() {
        context.check()?;
        let request = format!("chunk-{index}");
        writer.write_frame(
            &RemoteReceivingCommand::Status {
                request: request.clone(),
                digest,
            }
            .frame()?,
        )?;
        let bytes = control(&mut reader)?;
        let value = parse(&bytes)?;
        let detail = value.get("detail").ok_or_else(refused)?;
        let mut offset = detail
            .get("offset")
            .and_then(Json::as_u64)
            .ok_or_else(refused)?;
        let complete = detail
            .get("complete")
            .and_then(Json::as_bool)
            .ok_or_else(refused)?;
        if offset > length || complete != (offset == length) {
            return Err(refused());
        }
        verify_reply(
            &bytes,
            &receipt,
            "chunk",
            Some(&request),
            chunk(digest, offset, complete),
        )?;
        context.check()?;
        if complete {
            continue;
        }
        let bytes = context.request.source.read_chunk(digest)?;
        // The native source checks the digest and declared complete length before returning bytes.
        if bytes.len() as u64 != length {
            return Err(refused());
        }
        while offset < length {
            let end = (offset + super::remote_input::MAX_PART_BYTES as u64).min(length);
            context.check()?;
            writer.write_frame(&RemoteFrame::Chunk {
                digest,
                offset,
                final_part: end == length,
                bytes: bytes[offset as usize..end as usize].to_vec(),
            })?;
            verify_reply(
                &control(&mut reader)?,
                &receipt,
                "chunk",
                None,
                chunk(digest, end, end == length),
            )?;
            context.check()?;
            offset = end;
        }
    }
    context.check()?;
    writer.write_frame(
        &RemoteReceivingCommand::Materialize {
            request: "materialize".into(),
        }
        .frame()?,
    )?;
    verify_reply(
        &control(&mut reader)?,
        &receipt,
        "materialized",
        Some("materialize"),
        Json::Null,
    )?;
    context.check()?;
    Ok(RemoteInputTransferOutcome::Materialized(receipt))
}
struct Context<'a> {
    request: RemoteInputTransferRequest<'a>,
    lane: Lane,
    limits: Limits,
}
impl Context<'_> {
    fn check(&mut self) -> io::Result<()> {
        self.request.runtime.refresh().map_err(|_| refused())?;
        let state = self.request.runtime.state();
        let expiry = self
            .lane
            .runs
            .last()
            .and_then(|r| r.remote.as_ref())
            .ok_or_else(refused)?
            .lease_until_ms;
        let now = super::service::received_clock().map_err(|_| refused())?;
        if state.cancelled
            || state.limits.as_ref() != Some(&self.limits)
            || state.lanes.get(self.request.lane) != Some(&self.lane)
            || now >= expiry
        {
            return Err(refused());
        }
        self.request.source.verify_roots()
    }
}
fn control<R: Read>(reader: &mut RemoteFrameReader<R>) -> io::Result<Vec<u8>> {
    match reader.read_frame()? {
        Some(RemoteFrame::Control(bytes)) => Ok(bytes),
        _ => Err(refused()),
    }
}
fn parse(bytes: &[u8]) -> io::Result<Json> {
    Json::parse(std::str::from_utf8(bytes).map_err(|_| refused())?).map_err(|_| refused())
}
fn chunk(digest: mesh_cas::Digest32, offset: u64, complete: bool) -> Json {
    Json::object([
        ("digest", Json::text(digest.to_hex())),
        ("offset", Json::Number(offset)),
        ("complete", Json::Bool(complete)),
    ])
}
fn verify_reply(
    bytes: &[u8],
    receipt: &RemoteInputTransferReceipt,
    kind: &str,
    request: Option<&str>,
    detail: Json,
) -> io::Result<()> {
    let expected = Json::object([
        ("schema", Json::text("mesh.receiving-reply/v1")),
        ("kind", Json::text(kind)),
        ("request", request.map(Json::text).unwrap_or(Json::Null)),
        ("admission", receipt.0.clone()),
        ("detail", detail),
    ])
    .encode();
    if expected.as_bytes() == bytes {
        Ok(())
    } else {
        Err(refused())
    }
}
fn refused() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "remote-input-transfer-refused")
}
#[cfg(test)]
pub(in crate::fleet) mod tests;
