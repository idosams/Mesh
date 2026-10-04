//! Reconstruct materialization intent from the signed initial snapshot, never from mutable paths.
use super::{dependency_transaction::hash, invalid, ObservationLimits};
use crate::{checkpoint_storage::PreparedAuthenticatedCheckpoint, workspace::OpenWorkspace};
use mesh_operations::{ObjectId, Operation, VersionId, WorkspaceId};
use mesh_store::RecordDigest;
use mesh_types::{Blake3, DigestHasher as _};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum InitialEntry {
    Directory,
    File {
        manifest: RecordDigest,
        executable: bool,
    },
}
#[derive(Debug, PartialEq, Eq)]
pub(super) struct InitialPlan {
    pub(super) entries: BTreeMap<String, InitialEntry>,
}
impl InitialPlan {
    pub(super) fn verify(
        prepared: &PreparedAuthenticatedCheckpoint,
        workspace: WorkspaceId,
        limits: ObservationLimits,
    ) -> io::Result<Self> {
        limits.validate()?;
        let checkpoint = &prepared.checkpoint;
        let [record] = checkpoint.operations.as_slice() else {
            return Err(invalid("initial snapshot must have one operation"));
        };
        if prepared.changeset_id != record.id
            || checkpoint.records().len() != checkpoint.manifests.len() + 1
        {
            return Err(invalid("initial snapshot contains unrelated records"));
        }
        let mut object_budget = limits.bytes.saturating_add(16 * 1024 * 1024);
        let mut objects = BTreeMap::new();
        for bytes in &prepared.objects {
            object_budget = object_budget
                .checked_sub(bytes.len() as u64)
                .ok_or_else(|| invalid("staged initial objects exceed byte budget"))?;
            if objects.insert(hash(bytes), bytes.as_slice()).is_some() {
                return Err(invalid("duplicate staged object"));
            }
        }
        let payload = objects
            .get(&record.payload_digest)
            .ok_or_else(|| invalid("initial signed operation is missing"))?;
        let operations =
            OpenWorkspace::verify_dependency_initial_operations(record, payload, workspace)
                .map_err(io::Error::other)?;
        let plan = Self::operations(&operations, limits.entries)?;
        let manifests = checkpoint
            .manifests
            .iter()
            .map(|m| (m.id, m))
            .collect::<BTreeMap<_, _>>();
        if manifests.len() != checkpoint.manifests.len() {
            return Err(invalid("duplicate initial manifest"));
        }
        let mut used_manifests = BTreeSet::new();
        let mut used_objects = BTreeSet::from([record.payload_digest]);
        let mut remaining = limits.bytes;
        for entry in plan.entries.values() {
            let InitialEntry::File { manifest: id, .. } = entry else {
                continue;
            };
            let manifest = manifests
                .get(id)
                .ok_or_else(|| invalid("initial manifest missing"))?;
            remaining = remaining
                .checked_sub(manifest.byte_length)
                .ok_or_else(|| invalid("initial content exceeds total limit"))?;
            if manifest.byte_length > limits.file_bytes
                || manifest.chunks.len() > 65536
                || crate::manifest_paging::logical_manifest_id(
                    manifest.byte_length,
                    manifest.content_digest,
                    &manifest.chunks,
                ) != *id
            {
                return Err(invalid("invalid or oversized initial manifest"));
            }
            used_manifests.insert(*id);
            let mut content = <Blake3 as mesh_types::ContentDigest>::hasher();
            let mut offset = 0u64;
            for chunk in &manifest.chunks {
                let bytes = objects
                    .get(&chunk.digest)
                    .ok_or_else(|| invalid("initial chunk missing"))?;
                if chunk.byte_offset != offset
                    || chunk.byte_length == 0
                    || chunk.byte_length != bytes.len() as u64
                {
                    return Err(invalid("invalid initial chunk layout"));
                }
                offset = offset
                    .checked_add(chunk.byte_length)
                    .filter(|n| *n <= manifest.byte_length)
                    .ok_or_else(|| invalid("initial chunks exceed file size"))?;
                content.update(bytes);
                used_objects.insert(chunk.digest);
            }
            if offset != manifest.byte_length
                || content.finalize().as_bytes() != manifest.content_digest.as_bytes()
            {
                return Err(invalid("initial content digest or length differs"));
            }
        }
        if used_manifests.len() != manifests.len() || used_objects.len() != objects.len() {
            return Err(invalid("unreferenced initial staged content"));
        }
        Ok(plan)
    }

    // Accept the complete, deliberately small initial-tree grammar produced by consumption_start.
    // No ignored operations, dangling objects, aliases, overwrites or implicit parent directories.
    fn operations(operations: &[Operation], limit: usize) -> io::Result<Self> {
        Self::operations_bounded(operations, limit, 64 * 1024 * 1024)
    }
    fn operations_bounded(
        operations: &[Operation],
        limit: usize,
        mut path_bytes: usize,
    ) -> io::Result<Self> {
        let root = ObjectId::from_bytes([0; 16]);
        if !matches!(operations.first(), Some(Operation::InitializeWorkspace { root_id }) if *root_id == root)
        {
            return Err(invalid("initial tree lacks its explicit root"));
        }
        let mut directories = BTreeMap::from([(root, String::new())]);
        let mut objects = BTreeSet::from([root]);
        let mut entries = BTreeMap::new();
        let mut next = 1;
        while next < operations.len() {
            if entries.len() >= limit {
                return Err(invalid("initial tree exceeds entry limit"));
            }
            let (object, version, entry, link) = match &operations[next..] {
                [Operation::CreateDirectory { object_id }, Operation::LinkDirectoryEntry { .. }, ..] => {
                    (
                        *object_id,
                        VersionId::from_bytes([0; 32]),
                        InitialEntry::Directory,
                        next + 1,
                    )
                }
                [Operation::CreateFile { object_id }, Operation::WriteFileVersion {
                    object_id: written,
                    version_id,
                    parent_versions,
                    manifest_id,
                    portable_metadata,
                }, Operation::LinkDirectoryEntry { .. }, ..]
                    if object_id == written && parent_versions.is_empty() =>
                {
                    (
                        *object_id,
                        *version_id,
                        InitialEntry::File {
                            manifest: RecordDigest::from_bytes(*manifest_id.as_bytes()),
                            executable: portable_metadata.is_executable(),
                        },
                        next + 2,
                    )
                }
                _ => return Err(invalid("invalid initial tree operation sequence")),
            };
            let Operation::LinkDirectoryEntry {
                directory_id,
                name,
                object_id,
                version_id,
            } = &operations[link]
            else {
                unreachable!()
            };
            if *object_id != object || *version_id != version || !objects.insert(object) {
                return Err(invalid("initial entry identity differs or repeats"));
            }
            let parent = directories
                .get(directory_id)
                .ok_or_else(|| invalid("initial parent missing"))?;
            let length = parent
                .len()
                .checked_add(name.as_str().len())
                .and_then(|n| n.checked_add(usize::from(!parent.is_empty())))
                .ok_or_else(|| invalid("initial path size overflow"))?;
            path_bytes = path_bytes
                .checked_sub(length)
                .ok_or_else(|| invalid("initial path plan exceeds byte budget"))?;
            let path = if parent.is_empty() {
                name.as_str().to_owned()
            } else {
                format!("{parent}/{}", name.as_str())
            };
            if matches!(entry, InitialEntry::Directory) {
                directories.insert(object, path.clone());
            }
            if entries.insert(path, entry).is_some() {
                return Err(invalid("duplicate initial path"));
            }
            next = link + 1;
        }
        Ok(Self { entries })
    }
}

#[cfg(test)]
pub(super) fn assert_staged_refusals(
    prepared: &PreparedAuthenticatedCheckpoint,
    workspace: WorkspaceId,
    key: &ed25519_dalek::SigningKey,
) {
    let limits = ObservationLimits::default();
    let plan = InitialPlan::verify(prepared, workspace, limits).unwrap();
    assert_eq!(
        plan.entries.keys().map(String::as_str).collect::<Vec<_>>(),
        vec![".gitignore", "kept"]
    );
    let clone = || PreparedAuthenticatedCheckpoint {
        checkpoint: prepared.checkpoint.clone(),
        objects: prepared.objects.clone(),
        changeset_id: prepared.changeset_id,
    };
    assert!(InitialPlan::verify(prepared, WorkspaceId::from_bytes([42; 16]), limits).is_err());
    let mut bounded = limits;
    bounded.entries = 1;
    assert!(InitialPlan::verify(prepared, workspace, bounded).is_err());
    bounded = limits;
    bounded.bytes = 1;
    assert!(InitialPlan::verify(prepared, workspace, bounded).is_err());
    bounded = limits;
    bounded.file_bytes = 1;
    assert!(InitialPlan::verify(prepared, workspace, bounded).is_err());
    for n in 0..prepared.objects.len() {
        let mut missing = clone();
        missing.objects.remove(n);
        assert!(InitialPlan::verify(&missing, workspace, limits).is_err());
        let mut corrupt = clone();
        corrupt.objects[n].push(0);
        assert!(InitialPlan::verify(&corrupt, workspace, limits).is_err());
    }
    let mut changed = clone();
    changed.objects.push(b"unreferenced staged bytes".to_vec());
    assert!(InitialPlan::verify(&changed, workspace, limits).is_err());
    let mut changed = clone();
    changed.checkpoint.manifests[0].byte_length += 1;
    assert!(InitialPlan::verify(&changed, workspace, limits).is_err());
    let mut changed = clone();
    changed
        .checkpoint
        .manifests
        .push(changed.checkpoint.manifests[0].clone());
    assert!(InitialPlan::verify(&changed, workspace, limits).is_err());
    let mut changed = clone();
    changed.checkpoint.operations[0].actor_sequence += 1;
    assert!(InitialPlan::verify(&changed, workspace, limits).is_err());
    let mut changed = clone();
    changed.changeset_id = RecordDigest::from_bytes([44; 32]);
    assert!(InitialPlan::verify(&changed, workspace, limits).is_err());
    // A valid signature cannot make a non-genesis base or false derived head an initial tree.
    use crate::authenticated_changeset::{AuthenticatedChangeSet, CHANGESET_SIGNATURE_DOMAIN};
    use ed25519_dalek::Signer as _;
    use mesh_operations::{CanonicalEncode, CanonicalValue};
    struct Fields(Vec<CanonicalValue>);
    impl CanonicalEncode for Fields {
        fn schema(&self) -> &'static mesh_operations::RecordSchema {
            &mesh_operations::CHANGESET_SCHEMA
        }
        fn canonical_fields(&self) -> Vec<CanonicalValue> {
            self.0.clone()
        }
    }
    for field in ["base_head", "resulting_head"] {
        let mut changed = clone();
        let old = changed.changeset_id;
        let slot = changed.objects.iter().position(|b| hash(b) == old).unwrap();
        let envelope =
            AuthenticatedChangeSet::from_canonical_bytes(&changed.objects[slot]).unwrap();
        let mut fields = mesh_operations::decode_canonical(
            &mesh_operations::CHANGESET_SCHEMA,
            envelope.changeset(),
        )
        .unwrap();
        let index = mesh_operations::CHANGESET_SCHEMA
            .fields
            .iter()
            .position(|f| f.name == field)
            .unwrap();
        fields[index] = CanonicalValue::Bytes(vec![71; 32]);
        let inner = mesh_operations::encode_canonical(&Fields(fields));
        let payload = mesh_crypto::SigningPayload::new(CHANGESET_SIGNATURE_DOMAIN, &inner);
        let signed = AuthenticatedChangeSet::verified(
            inner,
            mesh_types::PublicKey::from_bytes(key.verifying_key().to_bytes()),
            mesh_types::Signature::from_bytes(key.sign(payload.as_bytes()).to_bytes()),
        )
        .unwrap()
        .canonical_bytes();
        let id = hash(&signed);
        changed.objects[slot] = signed;
        changed.checkpoint.operations[0].id = id;
        changed.checkpoint.operations[0].payload_digest = id;
        changed.changeset_id = id;
        assert!(
            InitialPlan::verify(&changed, workspace, limits).is_err(),
            "{field}"
        );
    }
    assert_eq!(
        InitialPlan::verify(prepared, workspace, limits).unwrap(),
        plan
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use mesh_operations::{ManifestId, NormalizedName, PortableMetadata};
    fn root() -> Operation {
        Operation::InitializeWorkspace {
            root_id: ObjectId::from_bytes([0; 16]),
        }
    }
    fn directory(id: u8, parent: u8, name: &str) -> Vec<Operation> {
        vec![
            Operation::CreateDirectory {
                object_id: ObjectId::from_bytes([id; 16]),
            },
            Operation::LinkDirectoryEntry {
                directory_id: ObjectId::from_bytes([parent; 16]),
                name: NormalizedName::new(name).unwrap(),
                object_id: ObjectId::from_bytes([id; 16]),
                version_id: VersionId::from_bytes([0; 32]),
            },
        ]
    }
    #[test]
    fn explicit_empty_tree_and_nested_executable_plan_are_exact() {
        assert!(InitialPlan::operations(&[root()], 0)
            .unwrap()
            .entries
            .is_empty());
        let mut ops = vec![root()];
        ops.extend(directory(1, 0, "folder"));
        ops.extend(directory(2, 1, "empty"));
        ops.extend([
            Operation::CreateFile {
                object_id: ObjectId::from_bytes([3; 16]),
            },
            Operation::WriteFileVersion {
                object_id: ObjectId::from_bytes([3; 16]),
                version_id: VersionId::from_bytes([4; 32]),
                parent_versions: vec![],
                manifest_id: ManifestId::from_bytes([5; 32]),
                portable_metadata: PortableMetadata::new(true),
            },
            Operation::LinkDirectoryEntry {
                directory_id: ObjectId::from_bytes([1; 16]),
                name: NormalizedName::new("run").unwrap(),
                object_id: ObjectId::from_bytes([3; 16]),
                version_id: VersionId::from_bytes([4; 32]),
            },
        ]);
        let plan = InitialPlan::operations(&ops, 3).unwrap();
        assert_eq!(plan.entries.len(), 3);
        assert_eq!(plan.entries["folder/empty"], InitialEntry::Directory);
        assert_eq!(
            plan.entries["folder/run"],
            InitialEntry::File {
                manifest: RecordDigest::from_bytes([5; 32]),
                executable: true
            }
        );
        assert!(InitialPlan::operations(&ops, 2).is_err());
        let exact_path_bytes = "folder".len() + "folder/empty".len() + "folder/run".len();
        assert_eq!(
            InitialPlan::operations_bounded(&ops, 3, exact_path_bytes).unwrap(),
            plan
        );
        assert!(InitialPlan::operations_bounded(&ops, 3, exact_path_bytes - 1).is_err());
        for end in [0, 2, 4, 6, 7] {
            assert!(InitialPlan::operations(&ops[..end], 3).is_err());
        }
        let mut mismatch = ops.clone();
        if let Operation::LinkDirectoryEntry { version_id, .. } = mismatch.last_mut().unwrap() {
            *version_id = VersionId::from_bytes([7; 32]);
        }
        assert!(InitialPlan::operations(&mismatch, 3).is_err());
    }
    #[test]
    fn aliases_duplicate_paths_unknown_parents_and_extra_roots_refuse() {
        for (id, parent, name) in [
            (1, 0, "other"),
            (2, 0, "folder"),
            (2, 9, "other"),
            (0, 0, "other"),
        ] {
            let mut ops = vec![root()];
            ops.extend(directory(1, 0, "folder"));
            ops.extend(directory(id, parent, name));
            assert!(InitialPlan::operations(&ops, 4).is_err());
        }
        assert!(InitialPlan::operations(&[root(), root()], 4).is_err());
    }
}
