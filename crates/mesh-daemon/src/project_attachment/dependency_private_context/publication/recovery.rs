//! Exact torn-frame evidence. This is neither workspace admission nor approval authority.
use super::*;
use crate::project_attachment::dependency_transaction::{read_payload, text};
use crate::root_authority::PinnedWorkspaceRoot;

pub(in crate::project_attachment) struct VerifiedPublicationPrefix {
    raw: String,
    metadata: std::path::PathBuf,
    store: (u64, u64),
    journal: (u64, u64),
    observed: RecordDigest,
    length: usize,
    record: DependencyRecord,
    payload: Vec<u8>,
    receipt: Vec<u8>,
}

impl VerifiedPublicationPrefix {
    pub(super) fn read(
        owner: &ProvisionedAttachment,
        request: RecordDigest,
        review: RecordDigest,
        receipt: &[u8],
    ) -> io::Result<Option<Self>> {
        let raw = match read_private_in_store(&owner.store, PENDING) {
            Ok(raw) => raw,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let mut journal = owner
            .store
            .filesystem()
            .read_only()
            .read_file(Path::new(crate::RECORD_FILE_NAME))?;
        let bytes = journal_bytes(&mut journal)?;
        let metadata = journal.metadata()?;
        let identity = (metadata.dev(), metadata.ino());
        let value = Json::parse(&raw).map_err(error)?;
        let payload_id = digest(text(&value, "payload")?)?;
        let cas = Cas::<_, Blake3>::with_filesystem(
            owner.metadata_path(),
            owner.store.filesystem().read_only(),
        )
        .map_err(error)?;
        let payload = read_payload(&cas, payload_id, 65536)?;
        let (length, record) = decode(&raw, identity, &bytes, &payload, request, review, receipt)?;
        let frame = frame_record(&StoredRecord::Dependency(record));
        let written = &bytes[length..];
        if written.is_empty() || written.starts_with(&frame) {
            // Staged-only and complete frames use the existing complete-history path.
            return Ok(None);
        }
        if !frame.starts_with(written) {
            return Err(invalid("foreign bytes after publication prefix"));
        }
        let proof = Self {
            metadata: owner.metadata_path().to_owned(),
            raw,
            store: owner.store.identity()?,
            journal: identity,
            observed: hash(&bytes),
            length,
            record,
            payload,
            receipt: receipt.to_vec(),
        };
        proof.verify(&owner.store, &journal, &bytes)?;
        Ok(Some(proof))
    }

    pub(in crate::project_attachment) fn verify(
        &self,
        store: &PinnedWorkspaceRoot,
        file: &File,
        bytes: &[u8],
    ) -> io::Result<(usize, DependencyRecord, Vec<u8>)> {
        store.ensure_namespace_identity()?;
        let held = file.metadata()?;
        let named = store
            .filesystem()
            .inspect_entry(Path::new(crate::RECORD_FILE_NAME))?
            .metadata()?;
        if store.identity()? != self.store
            || !held.is_file()
            || held.nlink() != 1
            || (held.dev(), held.ino()) != self.journal
            || (named.dev(), named.ino()) != self.journal
            || hash(bytes) != self.observed
            || read_private_in_store(store, PENDING)? != self.raw
        {
            return Err(invalid("publication recovery evidence changed"));
        }
        // Reopen objects through the pinned root; no cached bytes can substitute for lost data.
        let cas = Cas::<_, Blake3>::with_filesystem(&self.metadata, store.filesystem().read_only())
            .map_err(error)?;
        if read_payload(&cas, self.record.payload, 65536)? != self.payload
            || read_payload(&cas, hash(&self.receipt), 65536)? != self.receipt
        {
            return Err(invalid("publication recovery objects changed"));
        }
        Ok((self.length, self.record, self.payload.clone()))
    }
}

#[allow(clippy::too_many_arguments)]
fn decode(
    raw: &str,
    identity: (u64, u64),
    journal: &[u8],
    payload: &[u8],
    request: RecordDigest,
    review: RecordDigest,
    receipt: &[u8],
) -> io::Result<(usize, DependencyRecord)> {
    let value = Json::parse(raw).map_err(error)?;
    let length = value
        .get("journal_bytes")
        .and_then(Json::as_u64)
        .filter(|n| *n <= journal.len() as u64)
        .ok_or_else(|| invalid("invalid publication prefix"))? as usize;
    if intent(request, identity, &journal[..length], hash(payload)) != raw {
        return Err(invalid(
            "publication intent differs from request or pinned prefix",
        ));
    }
    let decoded = Json::parse(std::str::from_utf8(payload).map_err(error)?).map_err(error)?;
    if decoded.get("schema").and_then(Json::as_text) != Some("mesh.dependency-policy/v5")
        || decoded.get("kind").and_then(Json::as_u64)
            != Some(DependencyKind::Publication.code().into())
    {
        return Err(invalid("pending publication has another record format"));
    }
    let body = decoded
        .get("body")
        .ok_or_else(|| invalid("missing publication body"))?;
    if digest(text(body, "request")?)? != request
        || digest(text(body, "review")?)? != review
        || digest(text(body, "receipt")?)? != hash(receipt)
    {
        return Err(invalid(
            "publication recovery differs from original ceremony",
        ));
    }
    let record = DependencyRecord {
        authority: digest(text(&decoded, "authority")?)?,
        revision: decoded
            .get("revision")
            .and_then(Json::as_u64)
            .ok_or_else(|| invalid("missing publication ordinal"))?,
        previous: RecordDigest::parse_hex(text(&decoded, "previous")?).map_err(error)?,
        payload: hash(payload),
        kind: DependencyKind::Publication,
    };
    let frame = frame_record(&StoredRecord::Dependency(record));
    let tail = &journal[length..];
    if !frame.starts_with(tail) && !tail.starts_with(&frame) {
        return Err(invalid("foreign bytes after publication prefix"));
    }
    Ok((length, record))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn id(n: u8) -> RecordDigest {
        RecordDigest::from_bytes([n; 32])
    }
    #[test]
    fn publication_intent_matches_every_frame_boundary_and_rejects_foreign_evidence() {
        let receipt = b"structural fixture only; not a trusted approval";
        let payload = Json::object([
            ("schema", Json::text("mesh.dependency-policy/v5")),
            ("authority", Json::text(id(1).to_hex())),
            ("revision", Json::Number(2)),
            ("previous", Json::text(id(2).to_hex())),
            (
                "kind",
                Json::Number(DependencyKind::Publication.code().into()),
            ),
            (
                "body",
                Json::object([
                    ("request", Json::text(id(3).to_hex())),
                    ("review", Json::text(id(4).to_hex())),
                    ("receipt", Json::text(hash(receipt).to_hex())),
                ]),
            ),
        ])
        .encode()
        .into_bytes();
        let prefix = b"independently verified native prefix belongs to the reader";
        let raw = intent(id(3), (11, 12), prefix, hash(&payload));
        let (_, record) = decode(&raw, (11, 12), prefix, &payload, id(3), id(4), receipt).unwrap();
        let frame = frame_record(&StoredRecord::Dependency(record));
        for cut in 0..=frame.len() {
            let mut journal = prefix.to_vec();
            journal.extend_from_slice(&frame[..cut]);
            assert_eq!(
                decode(&raw, (11, 12), &journal, &payload, id(3), id(4), receipt).unwrap(),
                (prefix.len(), record)
            );
            if cut > 0 {
                *journal.last_mut().unwrap() ^= 1;
                assert!(
                    decode(&raw, (11, 12), &journal, &payload, id(3), id(4), receipt).is_err(),
                    "changed byte at cut {cut}"
                );
            }
        }
        for (identity, request, review, supplied) in [
            ((11, 13), id(3), id(4), receipt.as_slice()),
            ((11, 12), id(5), id(4), receipt.as_slice()),
            ((11, 12), id(3), id(5), receipt.as_slice()),
            ((11, 12), id(3), id(4), b"different receipt".as_slice()),
        ] {
            assert!(decode(&raw, identity, prefix, &payload, request, review, supplied).is_err());
        }
        let mut changed = prefix.to_vec();
        changed[0] ^= 1;
        assert!(decode(&raw, (11, 12), &changed, &payload, id(3), id(4), receipt).is_err());
        assert!(decode(
            &(raw.clone() + " "),
            (11, 12),
            prefix,
            &payload,
            id(3),
            id(4),
            receipt
        )
        .is_err());
        let mut complete = prefix.to_vec();
        complete.extend(&frame);
        complete.extend(b"later history must be checked by complete replay");
        assert!(decode(&raw, (11, 12), &complete, &payload, id(3), id(4), receipt).is_ok());
    }
}
