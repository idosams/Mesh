//! Complete immutable authoring roots from an exact control-frame reconstruction.
use super::*;
use crate::project_attachment::history::short_id;
use crate::workspace::OpenWorkspace;
use mesh_operations::WorkspaceId;

pub(in crate::project_attachment::dependency_private_context) struct ControlHistoryObjects {
    pub(in crate::project_attachment::dependency_private_context) operations:
        BTreeSet<RecordDigest>,
    pub(in crate::project_attachment::dependency_private_context) manifests: BTreeSet<RecordDigest>,
    pub(in crate::project_attachment::dependency_private_context) payloads: BTreeSet<RecordDigest>,
}

impl VerifiedControlPrefix {
    pub(in crate::project_attachment::dependency_private_context) fn history_roots(
        &self,
        owner: &ProvisionedAttachment,
        configuration: &str,
        proof: &VerifiedPrivateHistory,
        payload_budget: &mut usize,
        content_budget: &mut u64,
    ) -> io::Result<ControlHistoryObjects> {
        self.raw_for(owner)?;
        proof.verify_configuration(configuration)?;
        let mut file = owner
            .store
            .filesystem()
            .read_only()
            .read_file(Path::new(crate::RECORD_FILE_NAME))?;
        let observed = journal_bytes(&mut file)?;
        let (length, record, pending_payload) = self.verify(&owner.store, &file, &observed)?;
        proof.verify(&owner.store, &file, &observed)?;
        if proof.pending() != Some((length, record)) {
            return Err(invalid(
                "control history prefix differs from verified private context",
            ));
        }
        let mut complete = observed[..length].to_vec();
        complete.extend_from_slice(&frame_record(&StoredRecord::Dependency(record)));
        if complete.len() > MAX_JOURNAL {
            return Err(invalid("control history reconstruction exceeds bound"));
        }
        let scan = mesh_store::scan_journal(&complete).map_err(error)?;
        if scan.tail().is_fragment() {
            return Err(invalid("control history reconstruction is incomplete"));
        }
        let mut index = mesh_store::Index::new();
        let (mut operation_rows, mut manifest_rows) = (0usize, 0usize);
        for record in scan.records() {
            operation_rows += usize::from(matches!(record, StoredRecord::Operation(_)));
            manifest_rows += usize::from(matches!(record, StoredRecord::Manifest(_)));
            if operation_rows > 4096 || manifest_rows > 65536 {
                return Err(invalid("control history record bound exceeded"));
            }
            index.apply(record.clone()).map_err(error)?;
        }
        let operations = index
            .causally_ready_operations()
            .into_iter()
            .collect::<BTreeSet<_>>();
        let manifests = index.manifest_ids();
        if operations.len() != index.operation_count() {
            return Err(invalid("control history has unresolved causal parents"));
        }
        let cas = Cas::<_, Blake3>::with_filesystem(
            owner.metadata_path(),
            owner.store.filesystem().read_only(),
        )
        .map_err(error)?;
        let workspace = WorkspaceId::from_bytes(short_id(configuration.as_bytes()));
        let mut payloads = BTreeSet::new();
        for operation in &operations {
            if proof.is_legacy_operation(*operation) {
                return Err(invalid(
                    "legacy control retention needs explicit migration evidence",
                ));
            }
            let record = index
                .operation(operation)
                .ok_or_else(|| invalid("control history operation missing"))?;
            let bytes = read_payload(
                &cas,
                record.payload_digest,
                (*payload_budget).min(16 * 1024 * 1024),
            )?;
            *payload_budget = payload_budget
                .checked_sub(bytes.len())
                .ok_or_else(|| invalid("control history payload budget exceeded"))?;
            let fact =
                OpenWorkspace::verify_dependency_staged_operation(record, &bytes).map_err(error)?;
            if fact.workspace != workspace || !fact.manifests.is_subset(&manifests) {
                return Err(invalid(
                    "control history operation has foreign identity or missing manifests",
                ));
            }
            payloads.insert(record.payload_digest);
        }
        for manifest in &manifests {
            let record = index
                .manifest(manifest)
                .ok_or_else(|| invalid("control history manifest missing"))?;
            payloads.extend(
                OpenWorkspace::verify_dependency_manifest_content(&cas, record, content_budget)
                    .map_err(error)?,
            );
            if payloads.len() > 65536 {
                return Err(invalid("control history object bound exceeded"));
            }
        }
        let mut policy = proof.policy().clone();
        policy.apply(record, &pending_payload).map_err(error)?;
        payloads.extend(policy.policy_payloads());
        payloads.insert(proof.binding().authority);
        if payloads.len() > 65536 {
            return Err(invalid("control history object bound exceeded"));
        }
        self.raw_for(owner)?;
        proof.verify_current(&owner.store)?;
        Ok(ControlHistoryObjects {
            operations,
            manifests,
            payloads,
        })
    }
}
