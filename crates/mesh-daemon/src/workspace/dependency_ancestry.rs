//! Verified immutable operation facts for native dependency traversal. No access or approval grant.
use super::*;
use crate::authenticated_changeset::AuthenticatedChangeSet;
use mesh_operations::CanonicalValue;
use std::collections::BTreeSet;
const MAX_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;
const MAX_PARENTS: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NativeOperationFact {
    pub(crate) operation: RecordDigest,
    pub(crate) workspace: WorkspaceId,
    pub(crate) parents: Vec<RecordDigest>,
    pub(crate) manifests: BTreeSet<RecordDigest>,
}
fn refused() -> String {
    "native operation ancestry is unavailable or inconsistent".to_owned()
}
impl NativeOperationFact {
    fn verify(record: &OperationRecord, bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_PAYLOAD_BYTES
            || record.actor_sequence == 0
            || record.id != record.payload_digest
            || Blake3::digest_bytes(bytes).as_bytes() != record.id.as_bytes()
            || record.parents.len() > MAX_PARENTS
        {
            return Err(refused());
        }
        let envelope =
            AuthenticatedChangeSet::from_canonical_bytes(bytes).map_err(|_| refused())?;
        if !envelope.signed_by(PublicKey::from_bytes(*record.actor.as_bytes())) {
            return Err(refused());
        }
        let fields = mesh_operations::decode_canonical(
            &mesh_operations::CHANGESET_SCHEMA,
            envelope.changeset(),
        )
        .map_err(|_| refused())?;
        let field = |name: &str| -> Result<&CanonicalValue, String> {
            mesh_operations::CHANGESET_SCHEMA
                .fields
                .iter()
                .position(|f| f.name == name)
                .and_then(|n| fields.get(n))
                .ok_or_else(refused)
        };
        let expected_parents = CanonicalValue::Sequence(
            record
                .parents
                .iter()
                .map(|parent| CanonicalValue::Bytes(parent.as_bytes().to_vec()))
                .collect(),
        );
        if field("actor_id")? != &CanonicalValue::Bytes(record.actor.as_bytes().to_vec())
            || field("session_id")? != &CanonicalValue::Bytes(record.session.as_bytes().to_vec())
            || field("actor_sequence")? != &CanonicalValue::Unsigned(record.actor_sequence)
            || field("causal_parents")? != &expected_parents
            || field("policy_epoch")? != &CanonicalValue::Unsigned(record.policy_epoch)
            || field("hybrid_logical_time")?
                != &CanonicalValue::Group(vec![
                    CanonicalValue::Unsigned(record.hlc_millis),
                    CanonicalValue::Unsigned(record.hlc_counter),
                ])
            || record.parents.iter().collect::<BTreeSet<_>>().len() != record.parents.len()
            || record.parents.contains(&record.id)
        {
            return Err(refused());
        }
        let CanonicalValue::Bytes(workspace) = field("workspace_id")? else {
            return Err(refused());
        };
        let workspace: [u8; 16] = workspace.as_slice().try_into().map_err(|_| refused())?;
        let operations = decode_changeset_operations(bytes).ok_or_else(refused)?;
        let manifests = operations
            .iter()
            .filter_map(|op| match op {
                Operation::WriteFileVersion { manifest_id, .. } => {
                    Some(RecordDigest::from_bytes(*manifest_id.as_bytes()))
                }
                _ => None,
            })
            .collect();
        Ok(Self {
            operation: record.id,
            workspace: WorkspaceId::from_bytes(workspace),
            parents: record.parents.clone(),
            manifests,
        })
    }
}
impl OpenWorkspace {
    /// Verify immutable historical content without quarantine or index-derived authority.
    /// The caller supplies one shared byte budget for the entire dependency traversal.
    pub(crate) fn dependency_manifest_chunks(
        &self,
        id: RecordDigest,
        remaining_bytes: &mut u64,
    ) -> Result<BTreeSet<RecordDigest>, String> {
        self.ensure_physical_root().map_err(|_| refused())?;
        let manifest = self.record_index.manifest(&id).ok_or_else(refused)?;
        Self::verify_dependency_manifest_content(&self.payload_store, manifest, remaining_bytes)
    }

    pub(crate) fn verify_dependency_staged_operation(
        record: &OperationRecord,
        bytes: &[u8],
    ) -> Result<NativeOperationFact, String> {
        NativeOperationFact::verify(record, bytes)
    }

    /// Decode only an authenticated genesis candidate for the exact prospective workspace.
    /// This verifies immutable content; it does not grant consumption or filesystem authority.
    pub(crate) fn verify_dependency_initial_operations(
        record: &OperationRecord,
        bytes: &[u8],
        workspace: WorkspaceId,
    ) -> Result<Vec<Operation>, String> {
        let fact = NativeOperationFact::verify(record, bytes)?;
        if fact.workspace != workspace
            || !fact.parents.is_empty()
            || record.actor_sequence != 1
            || record.policy_epoch != 1
            || record.hlc_millis != 0
            || record.hlc_counter != 0
        {
            return Err(refused());
        }
        let envelope =
            AuthenticatedChangeSet::from_canonical_bytes(bytes).map_err(|_| refused())?;
        let fields = mesh_operations::decode_canonical(
            &mesh_operations::CHANGESET_SCHEMA,
            envelope.changeset(),
        )
        .map_err(|_| refused())?;
        let base = mesh_operations::CHANGESET_SCHEMA
            .fields
            .iter()
            .position(|f| f.name == "base_head")
            .and_then(|n| fields.get(n));
        if base != Some(&CanonicalValue::Bytes(vec![0; 32])) {
            return Err(refused());
        }
        struct Commitment(Vec<CanonicalValue>);
        impl mesh_operations::CanonicalEncode for Commitment {
            fn schema(&self) -> &'static mesh_operations::RecordSchema {
                &mesh_operations::TRANSITION_SCHEMA
            }
            fn canonical_fields(&self) -> Vec<CanonicalValue> {
                self.0.clone()
            }
        }
        let mut commitment = Vec::new();
        for field in mesh_operations::TRANSITION_SCHEMA.fields {
            let index = mesh_operations::CHANGESET_SCHEMA
                .fields
                .iter()
                .position(|f| f.name == field.name)
                .ok_or_else(refused)?;
            commitment.push(fields.get(index).ok_or_else(refused)?.clone());
        }
        let expected =
            Blake3::digest_bytes(&mesh_operations::encode_canonical(&Commitment(commitment)));
        let head = mesh_operations::CHANGESET_SCHEMA
            .fields
            .iter()
            .position(|f| f.name == "resulting_head")
            .and_then(|n| fields.get(n));
        if head != Some(&CanonicalValue::Bytes(expected.as_bytes().to_vec())) {
            return Err(refused());
        }
        decode_changeset_operations(bytes).ok_or_else(refused)
    }

    /// Shared immutable verification for journaled and staged recovery manifests.
    pub(crate) fn verify_dependency_manifest_content(
        store: &Cas<crate::root_authority::PinnedRootFs, mesh_cas::Blake3>,
        manifest: &mesh_store::ManifestRecord,
        remaining_bytes: &mut u64,
    ) -> Result<BTreeSet<RecordDigest>, String> {
        use mesh_cas::{ContentDigest as _, DigestHasher as _};
        let id = manifest.id;
        if manifest.chunks.len() > 65536
            || crate::manifest_paging::logical_manifest_id(
                manifest.byte_length,
                manifest.content_digest,
                &manifest.chunks,
            ) != id
        {
            return Err(refused());
        }
        *remaining_bytes = remaining_bytes
            .checked_sub(manifest.byte_length)
            .ok_or_else(refused)?;
        let mut offset = 0u64;
        let mut content = mesh_cas::Blake3::hasher();
        let mut roots = BTreeSet::new();
        let mut buffer = [0u8; 65536];
        for chunk in &manifest.chunks {
            if chunk.byte_offset != offset || chunk.byte_length == 0 {
                return Err(refused());
            }
            let digest = CasDigest::from_bytes(*chunk.digest.as_bytes());
            let mut file = store
                .filesystem()
                .read_file(&store.layout().chunk_path(&digest))
                .map_err(|_| refused())?;
            let metadata = file.metadata().map_err(|_| refused())?;
            if !metadata.is_file() || metadata.len() != chunk.byte_length {
                return Err(refused());
            }
            let mut chunk_hash = mesh_cas::Blake3::hasher();
            let mut left = chunk.byte_length;
            while left > 0 {
                let take = left.min(buffer.len() as u64) as usize;
                file.read_exact(&mut buffer[..take])
                    .map_err(|_| refused())?;
                chunk_hash.update(&buffer[..take]);
                content.update(&buffer[..take]);
                left -= take as u64;
            }
            if file.read(&mut buffer[..1]).map_err(|_| refused())? != 0
                || chunk_hash.finalize() != digest
            {
                return Err(refused());
            }
            offset = offset.checked_add(chunk.byte_length).ok_or_else(refused)?;
            roots.insert(chunk.digest);
        }
        if offset != manifest.byte_length
            || content.finalize().as_bytes() != manifest.content_digest.as_bytes()
        {
            return Err(refused());
        }
        Ok(roots)
    }

    /// Read one bounded authenticated fact from this exact opened journal and verified CAS.
    /// The dependency resolver still must visit every parent, bind workspace identity, verify
    /// referenced content and consumption edges, and retain complete native custody.
    pub(crate) fn dependency_operation_fact(
        &self,
        operation: RecordDigest,
    ) -> Result<NativeOperationFact, String> {
        self.ensure_physical_root().map_err(|_| refused())?;
        let record = self
            .record_index
            .operation(&operation)
            .ok_or_else(refused)?;
        let digest = CasDigest::from_bytes(*record.payload_digest.as_bytes());
        let file = self
            .payload_store
            .filesystem()
            .read_file(&self.payload_store.layout().chunk_path(&digest))
            .map_err(|_| refused())?;
        if !file.metadata().map_err(|_| refused())?.is_file() {
            return Err(refused());
        }
        let mut bytes = Vec::new();
        file.take((MAX_PAYLOAD_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| refused())?;
        NativeOperationFact::verify(record, &bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checkpoint_storage::{
        operation_checkpoint_signing_body, prepare_authenticated_checkpoint,
        AuthenticatedOperationCheckpointRequest,
    };
    use ed25519_dalek::{Signer as _, SigningKey};
    use mesh_crypto::SigningPayload;
    use mesh_operations::{HeadDerivation, TransitionCommitment};
    use mesh_types::Signature;
    struct Head;
    impl HeadDerivation for Head {
        fn resulting_head(&self, c: &TransitionCommitment) -> HeadId {
            HeadId::from_bytes(*Blake3::digest_bytes(&c.canonical_bytes()).as_bytes())
        }
    }
    fn prepared(parents: CausalParents, name: &str) -> (OperationRecord, Vec<u8>) {
        let root = std::env::temp_dir().join(format!(
            "mesh-dependency-fact-{name}-{}",
            std::process::id()
        ));
        let open = OpenWorkspace::open(&root).unwrap();
        let key = SigningKey::from_bytes(&[98; 32]);
        let actor = PublicKey::from_bytes(key.verifying_key().to_bytes());
        let request = |signature| {
            AuthenticatedOperationCheckpointRequest::new(
                WorkspaceId::from_bytes([1; 16]),
                ActorId::from_bytes(*actor.as_bytes()),
                SessionId::from_bytes([2; 16]),
                ActorSequence::FIRST,
                parents.clone(),
                HeadId::from_bytes([0; 32]),
                PolicyEpoch::new(1),
                Hlc::new(7, 3),
                vec![Operation::CreateDirectory {
                    object_id: ObjectId::from_bytes([5; 16]),
                }],
                actor,
                signature,
            )
        };
        let signature = Signature::from_bytes(
            key.sign(
                SigningPayload::new(
                    crate::authenticated_changeset::CHANGESET_SIGNATURE_DOMAIN,
                    &operation_checkpoint_signing_body(
                        &request(Signature::from_bytes([0; 64])),
                        &Head,
                    ),
                )
                .as_bytes(),
            )
            .to_bytes(),
        );
        let prepared =
            prepare_authenticated_checkpoint(&open, request(signature), vec![], &Head).unwrap();
        let record = prepared
            .checkpoint
            .records()
            .into_iter()
            .find_map(|r| match r {
                mesh_store::StoredRecord::Operation(r) => Some(r),
                _ => None,
            })
            .unwrap();
        let payload = prepared
            .objects
            .into_iter()
            .find(|bytes| {
                Blake3::digest_bytes(bytes).as_bytes() == record.payload_digest.as_bytes()
            })
            .unwrap();
        drop(open);
        std::fs::remove_dir_all(root).unwrap();
        (record, payload)
    }
    #[test]
    fn native_fact_reads_exact_journal_and_preserves_corrupt_or_oversized_payloads() {
        let (record, payload) = prepared(CausalParents::genesis(), "native-payload");
        let root =
            std::env::temp_dir().join(format!("mesh-native-fact-read-{}", std::process::id()));
        let mut open = OpenWorkspace::open(&root).unwrap();
        let cas = Cas::<_, mesh_cas::Blake3>::with_filesystem(
            open.storage_root().as_path(),
            open.storage_pinned_root().filesystem(),
        )
        .unwrap();
        cas.promote(payload.clone()).unwrap();
        let records = [mesh_store::StoredRecord::Operation(record.clone())];
        mesh_store::journal_records(open.checkpoint_journal_mut(), records.iter()).unwrap();
        let path = cas
            .layout()
            .chunk_path(&CasDigest::from_bytes(*record.payload_digest.as_bytes()));
        let path = open.storage_root().as_path().join(path);
        drop(open);
        let open = OpenWorkspace::open(&root).unwrap();
        assert_eq!(
            open.dependency_operation_fact(record.id).unwrap().operation,
            record.id
        );
        assert!(open
            .dependency_operation_fact(RecordDigest::from_bytes([99; 32]))
            .is_err());
        let mut corrupt = payload;
        corrupt[0] ^= 1;
        fs::write(&path, &corrupt).unwrap();
        assert!(open.dependency_operation_fact(record.id).is_err());
        assert_eq!(fs::read(&path).unwrap(), corrupt);
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len((MAX_PAYLOAD_BYTES + 1) as u64)
            .unwrap();
        assert!(open.dependency_operation_fact(record.id).is_err());
        assert_eq!(
            fs::metadata(&path).unwrap().len(),
            (MAX_PAYLOAD_BYTES + 1) as u64
        );
        drop(open);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn signed_parent_facts_preserve_multiple_parents_and_exact_context() {
        let parents = CausalParents::after(
            ChangeSetId::from_bytes([8; 32]),
            vec![ChangeSetId::from_bytes([9; 32])],
        );
        let (record, payload) = prepared(parents, "parents");
        let fact = NativeOperationFact::verify(&record, &payload).unwrap();
        assert_eq!(fact.operation, record.id);
        assert_eq!(fact.parents, record.parents);
        assert_eq!(fact.parents.len(), 2);
        assert_eq!(fact.workspace, WorkspaceId::from_bytes([1; 16]));
        assert!(fact.manifests.is_empty());
    }
    #[test]
    fn changed_journal_headers_cannot_rewrite_signed_ancestry_or_context() {
        let (record, payload) = prepared(
            CausalParents::after(ChangeSetId::from_bytes([8; 32]), vec![]),
            "headers",
        );
        for mode in 0..8 {
            let mut changed = record.clone();
            match mode {
                0 => changed.parents.clear(),
                1 => changed.parents[0] = RecordDigest::from_bytes([99; 32]),
                2 => changed.actor = RecordDigest::from_bytes([99; 32]),
                3 => changed.actor_sequence += 1,
                4 => changed.session = mesh_store::EntityUuid::from_bytes([99; 16]),
                5 => changed.policy_epoch += 1,
                6 => changed.hlc_millis += 1,
                7 => changed.hlc_counter += 1,
                _ => unreachable!(),
            }
            assert!(
                NativeOperationFact::verify(&changed, &payload).is_err(),
                "changed header {mode}"
            );
        }
        let mut corrupt = payload;
        corrupt[0] ^= 1;
        assert!(NativeOperationFact::verify(&record, &corrupt).is_err());
    }
}
