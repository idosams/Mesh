//! Sealed exact owner Consumption prefix; immutable facts, never ordinary admission.
use super::*;
use crate::project_attachment::{
    dependency_decision::pending_prefix,
    dependency_owner_context::pending_name,
    dependency_transaction::{read_payload, text},
};
use crate::root_authority::PinnedWorkspaceRoot;
use mesh_cas::{Blake3, Cas};
use mesh_store::{frame_record, DependencyKind, DependencyRecord, StoredRecord};
use std::{
    fs::File,
    io::Read as _,
    os::unix::{ffi::OsStrExt as _, fs::MetadataExt as _},
    path::Path,
};
const MAX_JOURNAL: usize = 64 * 1024 * 1024;

pub(in crate::project_attachment) struct VerifiedConsumptionOwnerPrefix {
    raw: String,
    name: String,
    request: RecordDigest,
    metadata: std::path::PathBuf,
    store: (u64, u64),
    sidecar: (u64, u64),
    journal: (u64, u64),
    observed: RecordDigest,
    length: usize,
    record: DependencyRecord,
    payload: Vec<u8>,
}
pub(super) fn exact_name(store: &PinnedWorkspaceRoot, request: RecordDigest) -> io::Result<String> {
    let prefix = format!("consumption-owner-{}-", request.to_hex());
    let names = store
        .filesystem()
        .read_directory_names_bounded(Path::new(""), 16384)?;
    let selected = names
        .iter()
        .filter(|name| name.as_bytes().starts_with(prefix.as_bytes()))
        .collect::<Vec<_>>();
    let [name] = selected.as_slice() else {
        return Err(invalid("consumption owner attempt missing or ambiguous"));
    };
    name.to_str()
        .map(str::to_owned)
        .ok_or_else(|| invalid("invalid consumption owner attempt name"))
}
pub(super) fn journal_bytes(file: &mut File) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    file.take((MAX_JOURNAL + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_JOURNAL {
        return Err(invalid("consumption owner journal exceeds bound"));
    }
    Ok(bytes)
}
impl VerifiedConsumptionOwnerPrefix {
    pub(super) fn sidecar_identity(&self, owner: &ProvisionedAttachment) -> io::Result<(u64, u64)> {
        self.raw_for(owner)?;
        Ok(self.sidecar)
    }

    pub(super) fn read(owner: &ProvisionedAttachment, request: RecordDigest) -> io::Result<Self> {
        let name = exact_name(&owner.store, request)?;
        let raw = read_private_in_store(&owner.store, &name)?;
        let value = Json::parse(&raw).map_err(error)?;
        let payload_id = digest(text(&value, "payload")?)?;
        if name != pending_name(request, payload_id)
            || text(&value, "schema")? != "mesh.native-consumption-owner-commit/v1"
            || digest(text(&value, "request")?)? != request
        {
            return Err(invalid("consumption owner attempt identity differs"));
        }
        let mut file = owner
            .store
            .filesystem()
            .read_only()
            .read_file(Path::new(crate::RECORD_FILE_NAME))?;
        let bytes = journal_bytes(&mut file)?;
        let metadata = file.metadata()?;
        let journal = (metadata.dev(), metadata.ino());
        let cas = Cas::<_, Blake3>::with_filesystem(
            owner.metadata_path(),
            owner.store.filesystem().read_only(),
        )
        .map_err(error)?;
        let (length, record, payload) = pending_prefix(&cas, &raw, journal, &bytes)?;
        if record.kind != DependencyKind::Consumption {
            return Err(invalid("owner attempt is not consumption"));
        }
        let metadata = owner
            .store
            .filesystem()
            .inspect_entry(Path::new(&name))?
            .metadata()?;
        let proof = Self {
            raw,
            name,
            request,
            metadata: owner.metadata_path().to_owned(),
            store: owner.store.identity()?,
            sidecar: (metadata.dev(), metadata.ino()),
            journal,
            observed: hash(&bytes),
            length,
            record,
            payload,
        };
        proof.verify(&owner.store, &file, &bytes)?;
        Ok(proof)
    }
    pub(in crate::project_attachment::dependency_private_context) fn raw_for(
        &self,
        owner: &ProvisionedAttachment,
    ) -> io::Result<&str> {
        if owner.metadata_path() != self.metadata || owner.store.identity()? != self.store {
            return Err(invalid("consumption owner proof belongs to another store"));
        }
        let mut file = owner
            .store
            .filesystem()
            .read_only()
            .read_file(Path::new(crate::RECORD_FILE_NAME))?;
        let bytes = journal_bytes(&mut file)?;
        self.verify(&owner.store, &file, &bytes)?;
        Ok(&self.raw)
    }
    pub(in crate::project_attachment::dependency_private_context) fn body_for(
        &self,
        owner: &ProvisionedAttachment,
    ) -> io::Result<Json> {
        self.raw_for(owner)?;
        Json::parse(std::str::from_utf8(&self.payload).map_err(error)?)
            .map_err(error)?
            .get("body")
            .cloned()
            .ok_or_else(|| invalid("consumption owner body missing"))
    }
    pub(in crate::project_attachment::dependency_private_context) fn name(&self) -> &str {
        &self.name
    }
    pub(in crate::project_attachment::dependency_private_context) fn retention_facts(
        &self,
        owner: &ProvisionedAttachment,
    ) -> io::Result<Json> {
        self.raw_for(owner)?;
        let mut file = owner
            .store
            .filesystem()
            .read_only()
            .read_file(Path::new(crate::RECORD_FILE_NAME))?;
        let bytes = journal_bytes(&mut file)?;
        self.verify(&owner.store, &file, &bytes)?;
        Ok(Json::object([
            (
                "schema",
                Json::text("mesh.native-consumption-owner-frame-retention/v1"),
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
                Json::text(format!("{:x}", self.journal.0)),
            ),
            ("journal_inode", Json::text(format!("{:x}", self.journal.1))),
            ("journal_digest", Json::text(self.observed.to_hex())),
            ("prefix_bytes", Json::Number(self.length as u64)),
            (
                "written_frame_bytes",
                Json::Number((bytes.len() - self.length) as u64),
            ),
            (
                "frame_bytes",
                Json::Number(frame_record(&StoredRecord::Dependency(self.record)).len() as u64),
            ),
        ]))
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
        let sidecar = store
            .filesystem()
            .inspect_entry(Path::new(&self.name))?
            .metadata()?;
        if store.identity()? != self.store
            || !held.is_file()
            || held.nlink() != 1
            || (held.dev(), held.ino()) != self.journal
            || (named.dev(), named.ino()) != self.journal
            || (sidecar.dev(), sidecar.ino()) != self.sidecar
            || hash(bytes) != self.observed
            || exact_name(store, self.request)? != self.name
            || read_private_in_store(store, &self.name)? != self.raw
        {
            return Err(invalid("consumption owner recovery evidence changed"));
        }
        let cas = Cas::<_, Blake3>::with_filesystem(&self.metadata, store.filesystem().read_only())
            .map_err(error)?;
        if read_payload(&cas, self.record.payload, 65536)? != self.payload {
            return Err(invalid("consumption owner recovery payload changed"));
        }
        let current = pending_prefix(&cas, &self.raw, self.journal, bytes)?;
        if current != (self.length, self.record, self.payload.clone()) {
            return Err(invalid("consumption owner recovery prefix changed"));
        }
        Ok(current)
    }
}
