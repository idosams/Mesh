//! Exact destination checkpoint suffix facts. These never mint ordinary workspace admission.
use super::{
    dependency_decision,
    dependency_transaction::{digest, hash, read_payload, text},
    invalid,
};
use crate::{ipc::Json, root_authority::PinnedRootFs};
use mesh_cas::{Blake3, Cas};
use mesh_store::{
    frame_record, scan_journal, DependencyKind, DependencyRecord, RecordDigest, StoredRecord,
};
use std::io;
pub(super) const PENDING: &str = "consumption-history.pending";
const MAX: usize = 80 * 1024 * 1024;
fn error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
pub(super) fn intent(
    request: RecordDigest,
    identity: (u64, u64),
    before: &[u8],
    start: RecordDigest,
    frames: RecordDigest,
) -> String {
    Json::object([
        (
            "schema",
            Json::text("mesh.native-consumption-history-intent/v1"),
        ),
        ("request", Json::text(request.to_hex())),
        ("journal_device", Json::text(format!("{:016x}", identity.0))),
        ("journal_inode", Json::text(format!("{:016x}", identity.1))),
        ("journal_bytes", Json::Number(before.len() as u64)),
        ("journal_digest", Json::text(hash(before).to_hex())),
        ("start", Json::text(start.to_hex())),
        ("frames", Json::text(frames.to_hex())),
    ])
    .encode()
}
pub(super) fn pending_prefix(
    cas: &Cas<PinnedRootFs, Blake3>,
    start_intent: &str,
    raw: &str,
    identity: (u64, u64),
    journal: &[u8],
    completion: Option<&str>,
) -> io::Result<(usize, DependencyRecord, Vec<u8>)> {
    let value = Json::parse(raw).map_err(error)?;
    let length = value
        .get("journal_bytes")
        .and_then(Json::as_u64)
        .filter(|n| *n <= journal.len() as u64 && *n <= MAX as u64)
        .ok_or_else(|| invalid("invalid consumption checkpoint prefix"))? as usize;
    let (prefix, record, payload) =
        dependency_decision::pending_prefix(cas, start_intent, identity, &journal[..length])?;
    if record.kind != DependencyKind::ConsumptionStart
        || length != prefix + frame_record(&StoredRecord::Dependency(record)).len()
    {
        return Err(invalid("checkpoint prefix lacks the exact complete start"));
    }
    let start = Json::parse(std::str::from_utf8(&payload).map_err(error)?).map_err(error)?;
    let body = start
        .get("body")
        .ok_or_else(|| invalid("start body missing"))?;
    let request = digest(text(body, "request")?)?;
    let descriptor = read_payload(cas, digest(text(body, "staged")?)?, 4096)?;
    let descriptor =
        Json::parse(std::str::from_utf8(&descriptor).map_err(error)?).map_err(error)?;
    let stage = read_payload(cas, digest(text(&descriptor, "stage")?)?, 64 * 1024 * 1024)?;
    let stage = Json::parse(std::str::from_utf8(&stage).map_err(error)?).map_err(error)?;
    let frames_id = digest(text(&stage, "frames")?)?;
    if intent(
        request,
        identity,
        &journal[..length],
        record.payload,
        frames_id,
    ) != raw
    {
        return Err(invalid("consumption checkpoint intent differs"));
    }
    let frames = read_payload(cas, frames_id, MAX)?;
    let end = length.saturating_add(frames.len());
    if end > MAX {
        return Err(invalid("consumption checkpoint exceeds bound"));
    }
    if let Some(completion) = completion {
        if end > journal.len() || journal[length..end] != frames {
            return Err(invalid("completion lacks exact complete checkpoint"));
        }
        super::consumption_complete::verify_prefix(
            cas, request, identity, journal, end, record, completion,
        )?;
    } else if !frames.starts_with(&journal[length..]) {
        return Err(invalid("foreign consumption checkpoint suffix"));
    }

    let scan = scan_journal(&frames).map_err(error)?;
    if scan.tail().is_fragment() || scan.records().is_empty() {
        return Err(invalid("incomplete retained consumption checkpoint"));
    }
    let operation = digest(text(body, "operation")?)?;
    for (n, r) in scan.records().iter().enumerate() {
        match r {
            StoredRecord::Manifest(_) if n + 1 < scan.records().len() => {}
            StoredRecord::Operation(op)
                if n + 1 == scan.records().len()
                    && op.id == operation
                    && op.payload_digest == operation => {}
            _ => return Err(invalid("unrelated consumption checkpoint record")),
        }
    }
    // The caller still reconstructs and independently authenticates the original InitialPlan.
    // Returning this prefix only inspects pending local facts; it cannot admit consumed history.
    Ok((prefix, record, payload))
}
