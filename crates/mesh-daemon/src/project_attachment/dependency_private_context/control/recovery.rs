//! Exact torn-frame evidence. This is neither workspace admission nor approval authority.
use super::*;
use crate::project_attachment::dependency_transaction::{read_payload, text};
use crate::root_authority::PinnedWorkspaceRoot;

pub(in crate::project_attachment) struct VerifiedControlPrefix {
    raw: String,
    metadata: std::path::PathBuf,
    store: (u64, u64),
    journal: (u64, u64),
    observed: RecordDigest,
    length: usize,
    record: DependencyRecord,
    payload: Vec<u8>,
}

impl VerifiedControlPrefix {
    pub(super) fn read(
        owner: &ProvisionedAttachment,
        request: RecordDigest,
        kind: DependencyKind,
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
        let (length, record, decoded_payload) =
            super::super::super::dependency_decision::pending_prefix(&cas, &raw, identity, &bytes)?;
        if !matches!(
            kind,
            DependencyKind::Eligibility | DependencyKind::ReviewSnapshot
        ) || record.kind != kind
            || digest(text(&value, "request")?)? != request
            || decoded_payload != payload
        {
            return Err(invalid(
                "pending native decision differs from exact input request",
            ));
        }
        let frame = frame_record(&StoredRecord::Dependency(record));
        let written = &bytes[length..];
        if written.is_empty() || written.starts_with(&frame) {
            // Staged-only and complete frames use the existing complete-history path.
            return Ok(None);
        }
        if !frame.starts_with(written) {
            return Err(invalid("foreign bytes after native decision prefix"));
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
            return Err(invalid("native decision recovery evidence changed"));
        }
        // Reopen objects through the pinned root; no cached bytes can substitute for lost data.
        let cas = Cas::<_, Blake3>::with_filesystem(&self.metadata, store.filesystem().read_only())
            .map_err(error)?;
        if read_payload(&cas, self.record.payload, 65536)? != self.payload {
            return Err(invalid("native decision recovery objects changed"));
        }
        Ok((self.length, self.record, self.payload.clone()))
    }
}
