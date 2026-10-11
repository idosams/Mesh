//! Immutable authoring roots from a sealed exact control or publication frame.
use super::*;
use crate::project_attachment::{dependency_transaction::read_payload, history::short_id};
use crate::workspace::OpenWorkspace;
use mesh_operations::WorkspaceId;
use mesh_store::{frame_record, StoredRecord};
use std::{io::Read as _, path::Path};
const MAX_JOURNAL: usize = 64 * 1024 * 1024;

pub(super) struct NativeRecoveryHistoryObjects {
    pub(super) operations: BTreeSet<RecordDigest>,
    pub(super) manifests: BTreeSet<RecordDigest>,
    pub(super) payloads: BTreeSet<RecordDigest>,
}

impl NativeHistoryRecovery<'_> {
    pub(super) fn raw_for(&self, owner: &ProvisionedAttachment) -> io::Result<&str> {
        match self {
            Self::Consumption(prefix) => prefix.raw_for(owner),
            Self::Control(prefix) => prefix.raw_for(owner),
            Self::Publication(prefix) => prefix.raw_for(owner),
            Self::Capture(_) => Err(invalid("capture has no dependency-frame retention prefix")),
        }
    }

    pub(super) fn history_roots(
        &self,
        owner: &ProvisionedAttachment,
        configuration: &str,
        proof: &VerifiedPrivateHistory,
        payload_budget: &mut usize,
        content_budget: &mut u64,
    ) -> io::Result<NativeRecoveryHistoryObjects> {
        self.raw_for(owner)?;
        proof.verify_configuration(configuration)?;
        let mut file = owner
            .store
            .filesystem()
            .read_only()
            .read_file(Path::new(crate::RECORD_FILE_NAME))?;
        let mut observed = Vec::new();
        (&mut file)
            .take((MAX_JOURNAL + 1) as u64)
            .read_to_end(&mut observed)?;
        if observed.len() > MAX_JOURNAL {
            return Err(invalid("native recovery journal exceeds bound"));
        }
        let (length, record, pending_payload) = match self {
            Self::Consumption(prefix) => prefix.verify(&owner.store, &file, &observed)?,
            Self::Control(prefix) => prefix.verify(&owner.store, &file, &observed)?,
            Self::Publication(prefix) => prefix.verify(&owner.store, &file, &observed)?,
            Self::Capture(_) => {
                return Err(invalid(
                    "capture requires its own authenticated reconstruction",
                ))
            }
        };
        proof.verify(&owner.store, &file, &observed)?;
        if proof.pending() != Some((length, record)) {
            return Err(invalid(
                "native recovery history prefix differs from verified private context",
            ));
        }
        let mut complete = observed[..length].to_vec();
        complete.extend_from_slice(&frame_record(&StoredRecord::Dependency(record)));
        if complete.len() > MAX_JOURNAL {
            return Err(invalid(
                "native recovery history reconstruction exceeds bound",
            ));
        }
        let scan = mesh_store::scan_journal(&complete).map_err(error)?;
        if scan.tail().is_fragment() {
            return Err(invalid(
                "native recovery history reconstruction is incomplete",
            ));
        }
        let mut index = mesh_store::Index::new();
        let (mut operation_rows, mut manifest_rows) = (0usize, 0usize);
        for record in scan.records() {
            operation_rows += usize::from(matches!(record, StoredRecord::Operation(_)));
            manifest_rows += usize::from(matches!(record, StoredRecord::Manifest(_)));
            if operation_rows > 4096 || manifest_rows > 65536 {
                return Err(invalid("native recovery history record bound exceeded"));
            }
            index.apply(record.clone()).map_err(error)?;
        }
        let operations = index
            .causally_ready_operations()
            .into_iter()
            .collect::<BTreeSet<_>>();
        let manifests = index.manifest_ids();
        if operations.len() != index.operation_count() {
            return Err(invalid(
                "native recovery history has unresolved causal parents",
            ));
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
                    "legacy recovery retention needs explicit migration evidence",
                ));
            }
            let record = index
                .operation(operation)
                .ok_or_else(|| invalid("native recovery history operation missing"))?;
            let bytes = read_payload(
                &cas,
                record.payload_digest,
                (*payload_budget).min(16 * 1024 * 1024),
            )?;
            *payload_budget = payload_budget
                .checked_sub(bytes.len())
                .ok_or_else(|| invalid("native recovery history payload budget exceeded"))?;
            let fact =
                OpenWorkspace::verify_dependency_staged_operation(record, &bytes).map_err(error)?;
            if fact.workspace != workspace || !fact.manifests.is_subset(&manifests) {
                return Err(invalid(
                    "native recovery history operation has foreign identity or missing manifests",
                ));
            }
            payloads.insert(record.payload_digest);
        }
        for manifest in &manifests {
            let record = index
                .manifest(manifest)
                .ok_or_else(|| invalid("native recovery history manifest missing"))?;
            payloads.extend(
                OpenWorkspace::verify_dependency_manifest_content(&cas, record, content_budget)
                    .map_err(error)?,
            );
            if payloads.len() > 65536 {
                return Err(invalid("native recovery history object bound exceeded"));
            }
        }
        let mut policy = proof.policy().clone();
        policy.apply(record, &pending_payload).map_err(error)?;
        payloads.extend(policy.policy_payloads());
        payloads.insert(proof.binding().authority);
        if payloads.len() > 65536 {
            return Err(invalid("native recovery history object bound exceeded"));
        }
        self.raw_for(owner)?;
        proof.verify_current(&owner.store)?;
        Ok(NativeRecoveryHistoryObjects {
            operations,
            manifests,
            payloads,
        })
    }
}
