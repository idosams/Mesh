//! Local CAS and allocation roots for an exact start, with no installed destination entries.
use super::*;
use crate::project_attachment::dependency_transaction::{digest, read_payload, text};
use crate::workspace_custody::WorkspaceInitializationGuard;
use mesh_cas::{Blake3, Cas};
use mesh_store::{frame_record, DependencyKind, StoredRecord};
use std::collections::BTreeSet;

impl PreparedNativeConsumedStart {
    pub(in crate::project_attachment) fn retained_start_roots(
        &self,
        storage: &AttachmentStorage,
        staged: &StagedNativeConsumedStart,
        graph: &crate::project_attachment::NativeDependencyGraph,
        guard: &WorkspaceInitializationGuard,
    ) -> io::Result<Json> {
        guard
            .require_roots(&[
                self.destination.store.clone(),
                self.destination.project().pinned.clone(),
            ])
            .map_err(error)?;
        let raw = read_private_in_store(&self.destination.store, super::super::START_PENDING)?;
        let (configuration, facts) = self.destination.project().read_native_facts(
            self.destination.metadata_path(),
            &self.destination.store,
            Some(&raw),
            None,
        )?;
        let facts = facts.ok_or_else(|| invalid("retention destination enrollment missing"))?;
        let (length, record) = facts
            .pending()
            .ok_or_else(|| invalid("retention start intent missing"))?;
        if configuration != self.basis.configuration
            || facts.binding() != self.basis.enrollment
            || record.kind != DependencyKind::ConsumptionStart
            || record.revision != 2
        {
            return Err(invalid("retention start configuration or record differs"));
        }
        let mut file = self
            .destination
            .store
            .filesystem()
            .read_only()
            .read_file(Path::new(crate::RECORD_FILE_NAME))?;
        let mut observed = Vec::new();
        (&mut file)
            .take(80 * 1024 * 1024 + 1)
            .read_to_end(&mut observed)?;
        if observed.len() > 80 * 1024 * 1024 {
            return Err(invalid("retention start journal exceeds bound"));
        }
        facts.verify(&self.destination.store, &file, &observed)?;
        let frame = frame_record(&StoredRecord::Dependency(record));
        if length > observed.len()
            || observed.len() - length > frame.len()
            || !frame.starts_with(&observed[length..])
        {
            return Err(invalid("retention start has a foreign journal suffix"));
        }
        let scan = mesh_store::scan_journal(&observed[..length]).map_err(error)?;
        if scan.tail().is_fragment()
            || !matches!(scan.records(), [StoredRecord::Dependency(enrollment)] if enrollment.kind == DependencyKind::Enrollment)
            || facts.policy().native_head().map(|head| head.0) != Some(1)
        {
            return Err(invalid(
                "retention start requires only its physical enrollment prefix",
            ));
        }
        let cas = Cas::<_, Blake3>::with_filesystem(
            self.destination.metadata_path(),
            self.destination.store.filesystem().read_only(),
        )
        .map_err(error)?;
        let payload = read_payload(&cas, record.payload, 65536)?;
        let value = Json::parse(std::str::from_utf8(&payload).map_err(error)?).map_err(error)?;
        let body = value
            .get("body")
            .ok_or_else(|| invalid("retention start body missing"))?;
        let descriptor = read_payload(&cas, digest(text(body, "staged")?)?, 4096)?;
        let descriptor_value =
            Json::parse(std::str::from_utf8(&descriptor).map_err(error)?).map_err(error)?;
        if digest(text(&descriptor_value, "stage")?)? != staged.receipt {
            return Err(invalid("retention stage selection changed"));
        }
        let stage = read(&staged.root, RECEIPT, MAX_RECEIPT)?;
        let closure = graph.to_json().encode();
        let mut payloads = facts.policy().policy_payloads().collect::<BTreeSet<_>>();
        payloads.insert(facts.binding().authority);
        let mut budget = 1024 * 1024 * 1024usize;
        for bytes in self.checkpoint.objects.iter().map(Vec::as_slice).chain([
            payload.as_slice(),
            descriptor.as_slice(),
            stage.as_slice(),
            self.basis.configuration.as_bytes(),
            self.basis.prospective.as_bytes(),
            closure.as_bytes(),
        ]) {
            if read_payload(&cas, hash(bytes), budget.min(bytes.len()))? != bytes {
                return Err(invalid("retention local start object differs"));
            }
            budget = budget
                .checked_sub(bytes.len())
                .ok_or_else(|| invalid("retention local object budget exceeded"))?;
            payloads.insert(hash(bytes));
        }
        for id in &payloads {
            let bytes = read_payload(&cas, *id, budget.min(64 * 1024 * 1024))?;
            budget = budget
                .checked_sub(bytes.len())
                .ok_or_else(|| invalid("retention local object budget exceeded"))?;
        }
        if payloads.len() > 65536 || self.checkpoint.checkpoint.manifests.len() > 65536 {
            return Err(invalid("retention local start root bound exceeded"));
        }
        let origin = storage
            .lane_origin_bound(&self.destination)?
            .ok_or_else(|| invalid("retention allocation missing"))?;
        guard
            .require_roots(std::slice::from_ref(&origin.allocation))
            .map_err(error)?;
        let identity = self.destination.store.identity()?;
        let journal_identity = file.metadata()?;
        let array = |ids: BTreeSet<RecordDigest>| {
            Json::Array(ids.into_iter().map(|id| Json::text(id.to_hex())).collect())
        };
        let result = Json::object([
            ("scope", Json::text("destination-store-start-recovery")),
            ("collection_authority", Json::Bool(false)),
            ("work", Json::text(self.basis.destination.work().to_hex())),
            (
                "installation",
                Json::text(self.basis.destination.installation().to_hex()),
            ),
            ("device", Json::text(format!("{:x}", identity.0))),
            ("inode", Json::text(format!("{:x}", identity.1))),
            (
                "staged_operations",
                array(BTreeSet::from([self.operation()])),
            ),
            (
                "staged_manifests",
                array(
                    self.checkpoint
                        .checkpoint
                        .manifests
                        .iter()
                        .map(|m| m.id)
                        .collect(),
                ),
            ),
            ("payloads", array(payloads)),
            (
                "sidecars",
                Json::Array(vec![Json::object([
                    ("name", Json::text(super::super::START_PENDING)),
                    ("digest", Json::text(hash(raw.as_bytes()).to_hex())),
                ])]),
            ),
            (
                "journal",
                Json::object([
                    (
                        "device",
                        Json::text(format!("{:x}", journal_identity.dev())),
                    ),
                    ("inode", Json::text(format!("{:x}", journal_identity.ino()))),
                    ("digest", Json::text(hash(&observed).to_hex())),
                    ("prefix_bytes", Json::Number(length as u64)),
                    (
                        "written_frame_bytes",
                        Json::Number((observed.len() - length) as u64),
                    ),
                    ("frame_bytes", Json::Number(frame.len() as u64)),
                ]),
            ),
            (
                "stage",
                Json::object([
                    ("identity", Json::text(identity_of_stage(staged)?)),
                    (
                        "allocation_identity",
                        Json::text(super::identity(&origin.allocation)?),
                    ),
                    ("receipt", Json::text(staged.receipt.to_hex())),
                    ("frames", Json::text(hash(&self.frames()).to_hex())),
                ]),
            ),
        ]);
        if read_private_in_store(&self.destination.store, super::super::START_PENDING)? != raw {
            return Err(invalid("retention start sidecar changed"));
        }
        guard.ensure_current().map_err(error)?;
        Ok(result)
    }
}
fn identity_of_stage(staged: &StagedNativeConsumedStart) -> io::Result<String> {
    identity(&staged.root)
}
