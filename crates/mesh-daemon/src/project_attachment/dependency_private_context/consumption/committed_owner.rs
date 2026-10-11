//! Exact historical owner receipt, independently joined with complete current native history.
use super::owner_recovery::{exact_name, journal_bytes};
use super::*;
use crate::project_attachment::{
    dependency_decision::pending_prefix,
    dependency_owner_context::pending_name,
    dependency_transaction::{read_payload, text},
};
use mesh_cas::{Blake3, Cas};
use mesh_store::{frame_record, DependencyKind, DependencyRecord, StoredRecord};
use std::{fs::File, os::unix::fs::MetadataExt as _, path::Path};

pub(super) struct VerifiedConsumptionOwnerReceipt {
    raw: String,
    name: String,
    request: RecordDigest,
    metadata: std::path::PathBuf,
    store: (u64, u64),
    sidecar: (u64, u64),
    proof: VerifiedPrivateHistory,
    length: usize,
    record: DependencyRecord,
    payload: Vec<u8>,
}
impl VerifiedConsumptionOwnerReceipt {
    pub(super) fn read(owner: &ProvisionedAttachment, request: RecordDigest) -> io::Result<Self> {
        let name = exact_name(&owner.store, request)?;
        let raw = read_private_in_store(&owner.store, &name)?;
        let value = Json::parse(&raw).map_err(error)?;
        let payload = digest(text(&value, "payload")?)?;
        if name != pending_name(request, payload)
            || text(&value, "schema")? != "mesh.native-consumption-owner-commit/v1"
            || digest(text(&value, "request")?)? != request
        {
            return Err(invalid("historical owner receipt identity differs"));
        }
        let (_, proof) = owner
            .project()
            .read_publication_private_history(owner.metadata_path(), &owner.store)?;
        let record = proof
            .policy()
            .native_request(request)
            .filter(|record| {
                record.kind == DependencyKind::Consumption && record.payload == payload
            })
            .ok_or_else(|| invalid("historical owner receipt is not committed"))?;
        if proof.pending().is_some() {
            return Err(invalid(
                "historical owner receipt requires complete owner history",
            ));
        }
        let mut file = owner
            .store
            .filesystem()
            .read_only()
            .read_file(Path::new(crate::RECORD_FILE_NAME))?;
        let bytes = journal_bytes(&mut file)?;
        proof.verify(&owner.store, &file, &bytes)?;
        let length = value
            .get("journal_bytes")
            .and_then(Json::as_u64)
            .filter(|n| *n <= bytes.len() as u64)
            .ok_or_else(|| invalid("historical receipt prefix exceeds journal"))?
            as usize;
        let metadata = owner
            .store
            .filesystem()
            .inspect_entry(Path::new(&name))?
            .metadata()?;
        let cas = Cas::<_, Blake3>::with_filesystem(
            owner.metadata_path(),
            owner.store.filesystem().read_only(),
        )
        .map_err(error)?;
        let payload = read_payload(&cas, record.payload, 65536)?;
        let selected = Self {
            raw,
            name,
            request,
            metadata: owner.metadata_path().to_owned(),
            store: owner.store.identity()?,
            sidecar: (metadata.dev(), metadata.ino()),
            proof,
            length,
            record,
            payload,
        };
        selected.verify(owner, &file, &bytes)?;
        Ok(selected)
    }
    fn verify(&self, owner: &ProvisionedAttachment, file: &File, bytes: &[u8]) -> io::Result<()> {
        owner.store.ensure_namespace_identity()?;
        let sidecar = owner
            .store
            .filesystem()
            .inspect_entry(Path::new(&self.name))?
            .metadata()?;
        if owner.metadata_path() != self.metadata
            || owner.store.identity()? != self.store
            || exact_name(&owner.store, self.request)? != self.name
            || (sidecar.dev(), sidecar.ino()) != self.sidecar
            || read_private_in_store(&owner.store, &self.name)? != self.raw
            || self.proof.pending().is_some()
            || self.proof.policy().native_request(self.request) != Some(self.record)
        {
            return Err(invalid("historical owner receipt evidence changed"));
        }
        self.proof.verify(&owner.store, file, bytes)?;
        let end = self
            .length
            .checked_add(frame_record(&StoredRecord::Dependency(self.record)).len())
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| invalid("historical owner receipt frame missing"))?;
        let prefix = mesh_store::scan_journal(&bytes[..self.length]).map_err(error)?;
        if prefix.tail().is_fragment() {
            return Err(invalid(
                "historical owner receipt is not at a journal boundary",
            ));
        }
        let cas =
            Cas::<_, Blake3>::with_filesystem(&self.metadata, owner.store.filesystem().read_only())
                .map_err(error)?;
        let metadata = file.metadata()?;
        let current = pending_prefix(
            &cas,
            &self.raw,
            (metadata.dev(), metadata.ino()),
            &bytes[..end],
        )?;
        if current != (self.length, self.record, self.payload.clone()) {
            return Err(invalid("historical owner receipt original prefix differs"));
        }
        Ok(())
    }
    pub(super) fn verify_context(
        &self,
        owner: &ProvisionedAttachment,
        proof: &VerifiedPrivateHistory,
    ) -> io::Result<()> {
        if *proof != self.proof {
            return Err(invalid(
                "historical owner receipt belongs to another native context",
            ));
        }
        self.raw_for(owner)?;
        Ok(())
    }
    fn raw_for(&self, owner: &ProvisionedAttachment) -> io::Result<&str> {
        let mut file = owner
            .store
            .filesystem()
            .read_only()
            .read_file(Path::new(crate::RECORD_FILE_NAME))?;
        let bytes = journal_bytes(&mut file)?;
        self.verify(owner, &file, &bytes)?;
        Ok(&self.raw)
    }
    pub(super) fn body_for(&self, owner: &ProvisionedAttachment) -> io::Result<Json> {
        self.raw_for(owner)?;
        Json::parse(std::str::from_utf8(&self.payload).map_err(error)?)
            .map_err(error)?
            .get("body")
            .cloned()
            .ok_or_else(|| invalid("historical owner receipt body missing"))
    }
    pub(super) fn record(&self) -> DependencyRecord {
        self.record
    }
    fn retention_facts(&self, owner: &ProvisionedAttachment) -> io::Result<Json> {
        let mut file = owner
            .store
            .filesystem()
            .read_only()
            .read_file(Path::new(crate::RECORD_FILE_NAME))?;
        let bytes = journal_bytes(&mut file)?;
        self.verify(owner, &file, &bytes)?;
        let metadata = file.metadata()?;
        let frame = frame_record(&StoredRecord::Dependency(self.record));
        Ok(Json::object([
            (
                "schema",
                Json::text("mesh.native-consumption-committed-owner-frame-retention/v1"),
            ),
            ("collection_authority", Json::Bool(false)),
            ("request", Json::text(self.request.to_hex())),
            ("payload", Json::text(self.record.payload.to_hex())),
            ("sidecar", Json::text(&self.name)),
            (
                "sidecar_digest",
                Json::text(hash(self.raw.as_bytes()).to_hex()),
            ),
            (
                "sidecar_device",
                Json::text(format!("{:x}", self.sidecar.0)),
            ),
            ("sidecar_inode", Json::text(format!("{:x}", self.sidecar.1))),
            (
                "journal_device",
                Json::text(format!("{:x}", metadata.dev())),
            ),
            ("journal_inode", Json::text(format!("{:x}", metadata.ino()))),
            ("journal_digest", Json::text(hash(&bytes).to_hex())),
            ("prefix_bytes", Json::Number(self.length as u64)),
            (
                "prefix_digest",
                Json::text(hash(&bytes[..self.length]).to_hex()),
            ),
            ("frame_bytes", Json::Number(frame.len() as u64)),
            (
                "later_journal_bytes",
                Json::Number((bytes.len() - self.length - frame.len()) as u64),
            ),
        ]))
    }
    pub(super) fn qualify_history(
        &self,
        owner: &ProvisionedAttachment,
        proof: &VerifiedPrivateHistory,
        mut history: Json,
    ) -> io::Result<Json> {
        self.verify_context(owner, proof)?;
        if history.get("device") != Some(&Json::text(format!("{:x}", self.store.0)))
            || history.get("inode") != Some(&Json::text(format!("{:x}", self.store.1)))
            || history.get("collection_authority") != Some(&Json::Bool(false))
        {
            return Err(invalid(
                "historical receipt inventory belongs to another store",
            ));
        }
        let Json::Array(payloads) = history
            .get("payloads")
            .ok_or_else(|| invalid("historical receipt inventory roots missing"))?
        else {
            return Err(invalid("invalid historical receipt roots"));
        };
        if !payloads.contains(&Json::text(self.record.payload.to_hex())) {
            return Err(invalid(
                "historical owner payload is not a local history root",
            ));
        }
        let Json::Object(fields) = &mut history else {
            return Err(invalid("invalid historical receipt inventory"));
        };
        let mut joined = false;
        for (name, value) in fields.iter_mut() {
            match name.as_str() {
                "schema" => {
                    *value =
                        Json::text("mesh.native-history-committed-consumption-owner-retention/v1")
                }
                "scope" => *value = Json::text("owner-store-committed-consumption-recovery"),
                "sidecars" => {
                    let Json::Array(sidecars) = value else {
                        return Err(invalid("invalid historical receipt sidecars"));
                    };
                    if sidecars
                        .iter()
                        .any(|sidecar| sidecar.get("name") == Some(&Json::text(&self.name)))
                    {
                        return Err(invalid("historical receipt sidecar conflicts"));
                    }
                    sidecars.push(Json::object([
                        ("name", Json::text(&self.name)),
                        ("digest", Json::text(hash(self.raw.as_bytes()).to_hex())),
                    ]));
                    joined = true;
                }
                _ => {}
            }
        }
        if !joined {
            return Err(invalid("historical receipt inventory has no sidecars"));
        }
        fields.push((
            "consumption_receipt".to_owned(),
            self.retention_facts(owner)?,
        ));
        Ok(history)
    }
}
