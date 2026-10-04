use super::*;
use ed25519_dalek::{Signer as _, SigningKey};
use mesh_cas::ContentDigest as _;
use mesh_crypto::SigningPayload;
use mesh_operations::NormalizedName;
use std::fs;
use std::path::PathBuf;

struct Fixture(PathBuf);
impl Fixture {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "mesh-batch-checkpoint-{name}-{}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct Head;
impl HeadDerivation for Head {
    fn resulting_head(&self, commitment: &mesh_operations::TransitionCommitment) -> HeadId {
        HeadId::from_bytes(*Blake3::digest_bytes(&commitment.canonical_bytes()).as_bytes())
    }
}
fn prepare(bytes: &[u8]) -> PreparedCheckpointFile {
    PreparedCheckpointFile::from_bytes(
        bytes,
        &ChunkingConfig::default(),
        ManifestPagingPolicy::flat(),
    )
    .unwrap()
}
fn request(files: &[PreparedCheckpointFile]) -> AuthenticatedOperationCheckpointRequest {
    let key = SigningKey::from_bytes(&[91; 32]);
    let public = PublicKey::from_bytes(key.verifying_key().to_bytes());
    let mut operations = Vec::new();
    for (index, file) in files.iter().enumerate() {
        let object = ObjectId::from_bytes([index as u8 + 1; 16]);
        let version = VersionId::from_bytes([index as u8 + 1; 32]);
        operations.push(Operation::CreateFile { object_id: object });
        operations.push(Operation::WriteFileVersion {
            object_id: object,
            version_id: version,
            parent_versions: Vec::new(),
            manifest_id: ManifestId::from_bytes(*file.manifest().id.as_bytes()),
            portable_metadata: PortableMetadata::new(index == 1),
        });
        operations.push(Operation::LinkDirectoryEntry {
            directory_id: ObjectId::from_bytes([0; 16]),
            name: NormalizedName::new(format!("file-{index}")).unwrap(),
            object_id: object,
            version_id: version,
        });
    }
    let mut request = AuthenticatedOperationCheckpointRequest::new(
        WorkspaceId::from_bytes([8; 16]),
        ActorId::from_bytes(*public.as_bytes()),
        SessionId::from_bytes([9; 16]),
        ActorSequence::FIRST,
        CausalParents::genesis(),
        HeadId::from_bytes([0; 32]),
        PolicyEpoch::new(1),
        Hlc::new(1, 0),
        operations,
        public,
        Signature::from_bytes([0; 64]),
    );
    sign(&mut request);
    request
}
fn sign(request: &mut AuthenticatedOperationCheckpointRequest) {
    let key = SigningKey::from_bytes(&[91; 32]);
    request.signature = Signature::from_bytes(
        key.sign(
            SigningPayload::new(
                crate::authenticated_changeset::CHANGESET_SIGNATURE_DOMAIN,
                &operation_checkpoint_signing_body(request, &Head),
            )
            .as_bytes(),
        )
        .to_bytes(),
    );
}

#[test]
fn batch_reopens_as_one_exact_version_with_shared_content_deduplicated() {
    let f = Fixture::new("roundtrip");
    let source = f.0.join("source");
    let metadata = f.0.join("metadata");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&metadata).unwrap();
    let bytes = [
        b"first file".as_slice(),
        b"\0second file\xff".as_slice(),
        b"first file".as_slice(),
    ];
    for (index, bytes) in bytes.iter().enumerate() {
        fs::write(source.join(format!("file-{index}")), bytes).unwrap();
    }
    let attachment =
        crate::project_attachment::ProjectAttachment::register(&source, &metadata).unwrap();
    let captured = attachment
        .capture_inputs(crate::project_attachment::ObservationLimits::default())
        .unwrap();
    fs::write(source.join("file-0"), b"user continues editing").unwrap();
    let files = captured
        .files()
        .iter()
        .map(|file| prepare(file.bytes()))
        .collect::<Vec<_>>();
    // This exercises the storage operation against an external store, not an attached-workspace
    // initializer. The production identity/history owner is a separate attachment integration.
    let mut workspace = OpenWorkspace::open(&metadata).unwrap();
    let cas = Cas::with_filesystem(
        workspace.storage_root().as_path().to_path_buf(),
        workspace.storage_pinned_root().filesystem(),
    )
    .unwrap();
    let saved =
        save_authenticated_checkpoint(&mut workspace, &cas, request(&files), files, &Head).unwrap();
    assert_eq!(saved.acknowledgement().manifests(), 2);
    assert_eq!(saved.acknowledgement().operations(), 1);
    drop(workspace);
    let reopened = OpenWorkspace::open(&metadata).unwrap();
    assert_eq!(
        fs::read(source.join("file-0")).unwrap(),
        b"user continues editing"
    );
    assert_eq!(fs::read_dir(&source).unwrap().count(), 3);
    assert_eq!(reopened.workspace_versions().len(), 1);
    assert_eq!(reopened.entries().len(), 3);
    assert!(reopened.shared_version().is_none());
    for (index, expected) in bytes.iter().enumerate() {
        let file = reopened
            .historical_workspace_file(saved.changeset_id(), &format!("file-{index}"))
            .unwrap()
            .unwrap();
        assert_eq!(&file.bytes, expected);
        assert_eq!(file.executable, index == 1);
    }
}

#[test]
fn unrelated_missing_or_unsigned_content_is_refused_before_storage_changes() {
    let f = Fixture::new("refusal");
    let mut workspace = OpenWorkspace::open(&f.0).unwrap();
    let cas = Cas::with_filesystem(
        workspace.storage_root().as_path().to_path_buf(),
        workspace.storage_pinned_root().filesystem(),
    )
    .unwrap();
    let before = fs::read(workspace.record_file()).unwrap();
    let files = vec![prepare(b"first"), prepare(b"second")];
    let err = save_authenticated_checkpoint(
        &mut workspace,
        &cas,
        request(&files),
        vec![files[0].clone()],
        &Head,
    )
    .unwrap_err();
    assert!(matches!(err, CheckpointSaveError::ManifestSetMismatch));
    let mut unrelated = files.clone();
    unrelated.push(prepare(b"unrelated"));
    let err =
        save_authenticated_checkpoint(&mut workspace, &cas, request(&files), unrelated, &Head)
            .unwrap_err();
    assert!(matches!(err, CheckpointSaveError::ManifestSetMismatch));
    let mut unsigned = request(&files);
    unsigned.signature = Signature::from_bytes([0; 64]);
    let err = save_authenticated_checkpoint(&mut workspace, &cas, unsigned, files.clone(), &Head)
        .unwrap_err();
    assert!(matches!(err, CheckpointSaveError::Authentication(_)));
    assert_eq!(fs::read(workspace.record_file()).unwrap(), before);
    for file in files {
        for chunk in file.manifest().chunks.iter() {
            assert!(cas
                .read(&CasDigest::from_bytes(*chunk.digest.as_bytes()))
                .is_err());
        }
    }
}

#[test]
fn interrupted_batch_journal_exposes_no_partial_workspace_version() {
    let f = Fixture::new("interrupted");
    let mut workspace = OpenWorkspace::open(&f.0).unwrap();
    let cas = Cas::with_filesystem(
        workspace.storage_root().as_path().to_path_buf(),
        workspace.storage_pinned_root().filesystem(),
    )
    .unwrap();
    let files = vec![prepare(b"first"), prepare(b"second")];
    save_authenticated_checkpoint(&mut workspace, &cas, request(&files), files, &Head).unwrap();
    let record_file = workspace.record_file().to_path_buf();
    let journal = fs::read(&record_file).unwrap();
    let records = mesh_store::scan_journal(&journal).unwrap().into_records();
    drop(workspace);
    // Both complete manifests and a torn operation represent an interrupted batch, even when
    // its CAS and derived index previously completed. Reopening must use journal truth alone.
    let manifest_bytes = records
        .iter()
        .take(2)
        .flat_map(mesh_store::frame_record)
        .collect::<Vec<_>>();
    for bytes in [
        manifest_bytes.clone(),
        journal[..journal.len() - 1].to_vec(),
    ] {
        fs::write(&record_file, bytes).unwrap();
        let reopened = OpenWorkspace::open(&f.0).unwrap();
        assert!(reopened.workspace_versions().is_empty());
        assert!(reopened.entries().is_empty());
        assert!(reopened.shared_version().is_none());
    }
    fs::write(&record_file, &journal).unwrap();
    let complete = OpenWorkspace::open(&f.0).unwrap();
    assert_eq!(complete.workspace_versions().len(), 1);
    assert_eq!(complete.entries().len(), 2);
}

#[test]
fn later_operation_can_reuse_a_durable_manifest_without_resupplying_content() {
    let f = Fixture::new("reuse");
    let mut workspace = OpenWorkspace::open(&f.0).unwrap();
    let cas = Cas::with_filesystem(
        workspace.storage_root().as_path().to_path_buf(),
        workspace.storage_pinned_root().filesystem(),
    )
    .unwrap();
    let files = vec![prepare(b"retained bytes")];
    let first =
        save_authenticated_checkpoint(&mut workspace, &cas, request(&files), files.clone(), &Head)
            .unwrap();
    drop(workspace);
    let mut workspace = OpenWorkspace::open(&f.0).unwrap();
    let mut next = request(&files);
    next.actor_sequence = ActorSequence::new(2);
    next.causal_parents = CausalParents::after(
        mesh_operations::ChangeSetId::from_bytes(*first.changeset_id().as_bytes()),
        Vec::new(),
    );
    next.operations = vec![Operation::WriteFileVersion {
        object_id: ObjectId::from_bytes([1; 16]),
        version_id: VersionId::from_bytes([7; 32]),
        parent_versions: vec![VersionId::from_bytes([1; 32])],
        manifest_id: ManifestId::from_bytes(*files[0].manifest().id.as_bytes()),
        portable_metadata: PortableMetadata::new(true),
    }];
    sign(&mut next);
    let saved =
        save_authenticated_checkpoint(&mut workspace, &cas, next, Vec::new(), &Head).unwrap();
    assert_eq!(saved.acknowledgement().manifests(), 0);
    drop(workspace);
    let reopened = OpenWorkspace::open(&f.0).unwrap();
    assert_eq!(reopened.workspace_versions().len(), 2);
    let old = reopened
        .historical_workspace_file(first.changeset_id(), "file-0")
        .unwrap()
        .unwrap();
    let new = reopened
        .historical_workspace_file(saved.changeset_id(), "file-0")
        .unwrap()
        .unwrap();
    assert_eq!(old.bytes, new.bytes);
    assert!(!old.executable);
    assert!(new.executable);
}

#[test]
fn a_manifest_only_in_the_mutable_index_cannot_be_reused_as_durable_history() {
    let f = Fixture::new("unjournaled");
    let mut workspace = OpenWorkspace::open(&f.0).unwrap();
    let cas = Cas::with_filesystem(
        workspace.storage_root().as_path().to_path_buf(),
        workspace.storage_pinned_root().filesystem(),
    )
    .unwrap();
    let files = vec![prepare(b"not journaled")];
    let mut promoter = CasChunkPromoter::new(&cas);
    DurableCommit::new(
        workspace.checkpoint_store_mut(),
        &mut promoter,
        files[0].chunks().to_vec(),
        Checkpoint {
            manifests: vec![files[0].manifest().clone()],
            ..Checkpoint::default()
        },
    )
    .finish()
    .unwrap();
    assert!(workspace
        .manifest_record(mesh_materializer::ManifestId::from_bytes(
            *files[0].manifest().id.as_bytes()
        ))
        .is_some());
    let before = fs::read(workspace.record_file()).unwrap();
    let err =
        save_authenticated_checkpoint(&mut workspace, &cas, request(&files), Vec::new(), &Head)
            .unwrap_err();
    assert!(matches!(err, CheckpointSaveError::ManifestSetMismatch));
    assert_eq!(fs::read(workspace.record_file()).unwrap(), before);
}

#[test]
fn authenticated_preparation_is_exact_and_does_not_persist_before_native_intent() {
    let f = Fixture::new("prepare-only");
    let mut workspace = OpenWorkspace::open(&f.0).unwrap();
    let cas = Cas::with_filesystem(
        workspace.storage_root().as_path().to_path_buf(),
        workspace.storage_pinned_root().filesystem(),
    )
    .unwrap();
    let before = fs::read(workspace.record_file()).unwrap();
    let files = vec![prepare(b"private first"), prepare(b"\0private second\xff")];
    let expected = authenticated_checkpoint_identity(&request(&files), &Head).unwrap();
    let prepared =
        prepare_authenticated_checkpoint(&workspace, request(&files), files.clone(), &Head)
            .unwrap();
    assert_eq!(prepared.changeset_id, expected);
    assert_eq!(workspace.operations(), 0);
    assert_eq!(fs::read(workspace.record_file()).unwrap(), before);
    for bytes in &prepared.objects {
        assert!(cas.read(&CasBlake3::digest_bytes(bytes)).is_err());
    }
    let frames = prepared
        .checkpoint
        .records()
        .iter()
        .flat_map(mesh_store::frame_record)
        .collect::<Vec<_>>();
    let saved =
        save_authenticated_checkpoint(&mut workspace, &cas, request(&files), files, &Head).unwrap();
    assert_eq!(saved.changeset_id(), expected);
    let after = fs::read(workspace.record_file()).unwrap();
    assert_eq!(&after[..before.len()], before);
    assert_eq!(&after[before.len()..], frames);
    for bytes in prepared.objects {
        assert_eq!(cas.read(&CasBlake3::digest_bytes(&bytes)).unwrap(), bytes);
    }
}
