//! Canonical pending eligibility-frame validation. No journal writes or repair occur here.
use super::*;
use crate::project_attachment::dependency_transaction::{digest, text};
pub(in crate::project_attachment) fn pending_prefix(
    cas: &Cas<PinnedRootFs, Blake3>,
    intent: &str,
    identity: (u64, u64),
    journal: &[u8],
) -> io::Result<(usize, DependencyRecord, Vec<u8>)> {
    let value = Json::parse(intent).map_err(error)?;
    let request = digest(text(&value, "request")?)?;
    let length = value
        .get("journal_bytes")
        .and_then(Json::as_u64)
        .filter(|n| *n <= journal.len() as u64)
        .ok_or_else(|| invalid("invalid decision prefix"))? as usize;
    let payload = digest(text(&value, "payload")?)?;
    if transaction_intent(request, identity, &journal[..length], payload) != intent {
        return Err(invalid(
            "decision intent does not match pinned journal prefix",
        ));
    }
    let bytes = read_payload(cas, payload, 65_536)?;
    let decoded = Json::parse(std::str::from_utf8(&bytes).map_err(error)?).map_err(error)?;
    if decoded.get("kind").and_then(Json::as_u64)
        != Some(u64::from(DependencyKind::Eligibility.code()))
    {
        return Err(invalid("pending record is not an eligibility decision"));
    }
    let record = DependencyRecord {
        authority: digest(text(&decoded, "authority")?)?,
        revision: decoded
            .get("revision")
            .and_then(Json::as_u64)
            .ok_or_else(|| invalid("missing decision ordinal"))?,
        previous: RecordDigest::parse_hex(text(&decoded, "previous")?).map_err(error)?,
        payload,
        kind: DependencyKind::Eligibility,
    };
    if decoded
        .get("body")
        .and_then(|body| body.get("request"))
        .and_then(Json::as_text)
        != Some(request.to_hex().as_str())
    {
        return Err(invalid("pending decision request does not match payload"));
    }
    let frame = frame_record(&StoredRecord::Dependency(record));
    if !frame.starts_with(&journal[length..]) {
        return Err(invalid("foreign bytes after decision prefix"));
    }
    Ok((length, record, bytes))
}
