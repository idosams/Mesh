//! Exact local completion framing. A local receipt never substitutes for owner verification.
use super::{
    dependency_transaction::{digest, hash, read_payload, text},
    invalid,
};
use crate::{ipc::Json, root_authority::PinnedRootFs};
use mesh_cas::{Blake3, Cas};
use mesh_store::{frame_record, DependencyKind, DependencyRecord, RecordDigest, StoredRecord};
use std::io;
pub(super) const PENDING: &str = "consumption-complete.pending";
fn error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
pub(super) fn payload(
    start: DependencyRecord,
    owner: RecordDigest,
) -> io::Result<(DependencyRecord, Vec<u8>)> {
    if start.kind != DependencyKind::ConsumptionStart || owner == RecordDigest::from_bytes([0; 32])
    {
        return Err(invalid("invalid completion selection"));
    }
    let revision = start
        .revision
        .checked_add(1)
        .ok_or_else(|| invalid("completion ordinal exhausted"))?;
    let bytes = Json::object([
        ("schema", Json::text("mesh.dependency-policy/v1")),
        ("authority", Json::text(start.authority.to_hex())),
        ("revision", Json::Number(revision)),
        ("previous", Json::text(start.payload.to_hex())),
        (
            "kind",
            Json::Number(u64::from(DependencyKind::ConsumptionComplete.code())),
        ),
        (
            "body",
            Json::object([
                ("start", Json::text(start.payload.to_hex())),
                ("owner_receipt", Json::text(owner.to_hex())),
            ]),
        ),
    ])
    .encode()
    .into_bytes();
    Ok((
        DependencyRecord {
            authority: start.authority,
            revision,
            previous: start.payload,
            payload: hash(&bytes),
            kind: DependencyKind::ConsumptionComplete,
        },
        bytes,
    ))
}
pub(super) fn intent(
    request: RecordDigest,
    identity: (u64, u64),
    before: &[u8],
    payload: RecordDigest,
) -> String {
    Json::object([
        (
            "schema",
            Json::text("mesh.native-consumption-complete-intent/v1"),
        ),
        ("request", Json::text(request.to_hex())),
        ("journal_device", Json::text(format!("{:016x}", identity.0))),
        ("journal_inode", Json::text(format!("{:016x}", identity.1))),
        ("journal_bytes", Json::Number(before.len() as u64)),
        ("journal_digest", Json::text(hash(before).to_hex())),
        ("payload", Json::text(payload.to_hex())),
    ])
    .encode()
}
pub(super) fn verify_prefix(
    cas: &Cas<PinnedRootFs, Blake3>,
    request: RecordDigest,
    identity: (u64, u64),
    journal: &[u8],
    prefix: usize,
    start: DependencyRecord,
    raw: &str,
) -> io::Result<DependencyRecord> {
    if prefix > journal.len() {
        return Err(invalid("completion predates checkpoint"));
    }
    let value = Json::parse(raw).map_err(error)?;
    let id = digest(text(&value, "payload")?)?;
    if intent(request, identity, &journal[..prefix], id) != raw {
        return Err(invalid("completion intent differs from exact checkpoint"));
    }
    let bytes = read_payload(cas, id, 65536)?;
    let decoded = Json::parse(std::str::from_utf8(&bytes).map_err(error)?).map_err(error)?;
    let owner = digest(text(
        decoded
            .get("body")
            .ok_or_else(|| invalid("completion body missing"))?,
        "owner_receipt",
    )?)?;
    let (record, expected) = payload(start, owner)?;
    let frame = frame_record(&StoredRecord::Dependency(record));
    if bytes != expected
        || prefix.saturating_add(frame.len()) > 80 * 1024 * 1024
        || !frame.starts_with(&journal[prefix..])
    {
        return Err(invalid("foreign consumption completion suffix"));
    }
    Ok(record)
}
