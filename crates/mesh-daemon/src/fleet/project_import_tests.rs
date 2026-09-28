use super::*;
use crate::authenticated_changeset::CHANGESET_SIGNATURE_DOMAIN;
use crate::checkpoint_storage::{
    operation_checkpoint_signing_body, save_authenticated_checkpoint,
    AuthenticatedOperationCheckpointRequest,
};
use crate::workspace::ManagedAuthoringBasis;
use ed25519_dalek::{Signer as _, SigningKey};
use mesh_cas::Cas;
use mesh_crypto::SigningPayload;
use mesh_operations::{
    ActorId, ActorSequence, CausalParents, HeadDerivation, HeadId, Hlc, ManifestId, PolicyEpoch,
    SessionId, TransitionCommitment, WorkspaceId,
};
use mesh_types::Signature;
use std::fs;

struct DerivedHead;
impl HeadDerivation for DerivedHead {
    fn resulting_head(&self, commitment: &TransitionCommitment) -> HeadId {
        HeadId::from_bytes(*Blake3::digest_bytes(&commitment.canonical_bytes()).as_bytes())
    }
}
fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}
fn public(key: &SigningKey) -> PublicKey {
    PublicKey::from_bytes(key.verifying_key().to_bytes())
}
fn obj(id: u8) -> ObjectId {
    ObjectId::from_bytes([id; 16])
}
fn link(parent: ObjectId, name: &str, object: ObjectId, version: VersionId) -> Operation {
    Operation::LinkDirectoryEntry {
        directory_id: parent,
        name: NormalizedName::new(name).unwrap(),
        object_id: object,
        version_id: version,
    }
}
fn unlink(parent: u8, name: &str, object: u8) -> Operation {
    Operation::UnlinkDirectoryEntry {
        directory_id: obj(parent),
        name: NormalizedName::new(name).unwrap(),
        object_id: obj(object),
    }
}
fn directory(id: u8, parent: u8, name: &str) -> Vec<Operation> {
    vec![
        Operation::CreateDirectory { object_id: obj(id) },
        link(obj(parent), name, obj(id), VersionId::from_bytes([0; 32])),
    ]
}
fn file(
    id: u8,
    parent: u8,
    name: &str,
    content: &[u8],
) -> (Vec<Operation>, PreparedCheckpointFile) {
    let prepared = PreparedCheckpointFile::from_bytes(
        content,
        &ChunkingConfig::default(),
        ManifestPagingPolicy::flat(),
    )
    .unwrap();
    let version = VersionId::from_bytes([id; 32]);
    (
        vec![
            Operation::CreateFile { object_id: obj(id) },
            Operation::WriteFileVersion {
                object_id: obj(id),
                version_id: version,
                parent_versions: vec![],
                manifest_id: ManifestId::from_bytes(*prepared.manifest().id.as_bytes()),
                portable_metadata: PortableMetadata::new(false),
            },
            link(obj(parent), name, obj(id), version),
        ],
        prepared,
    )
}
fn append(
    open: &mut OpenWorkspace,
    basis: &ManagedAuthoringBasis,
    key: &SigningKey,
    operations: Vec<Operation>,
    files: Vec<PreparedCheckpointFile>,
) -> RecordDigest {
    let request = |signature| {
        AuthenticatedOperationCheckpointRequest::new(
            basis.workspace_id,
            basis.actor_id,
            basis.session_id,
            basis.actor_sequence,
            basis.causal_parents.clone(),
            basis.base_head,
            basis.policy_epoch,
            basis.hybrid_logical_time,
            operations.clone(),
            public(key),
            signature,
        )
    };
    let unsigned = request(Signature::from_bytes([0; 64]));
    let payload = SigningPayload::new(
        CHANGESET_SIGNATURE_DOMAIN,
        &operation_checkpoint_signing_body(&unsigned, &DerivedHead),
    );
    let signature = Signature::from_bytes(key.sign(payload.as_bytes()).to_bytes());
    let cas = Cas::with_filesystem(
        open.storage_root().as_path().to_owned(),
        open.storage_pinned_root().filesystem(),
    )
    .unwrap();
    save_authenticated_checkpoint(open, &cas, request(signature), files, &DerivedHead)
        .unwrap()
        .changeset_id()
}
fn initial(open: &mut OpenWorkspace, actor: &SigningKey, offset: u8) -> RecordDigest {
    let basis = ManagedAuthoringBasis {
        workspace_id: WorkspaceId::from_bytes([offset + 1; 16]),
        actor_id: ActorId::from_bytes(*public(actor).as_bytes()),
        session_id: SessionId::from_bytes([offset + 2; 16]),
        actor_sequence: ActorSequence::FIRST,
        causal_parents: CausalParents::genesis(),
        base_head: HeadId::from_bytes([0; 32]),
        policy_epoch: PolicyEpoch::new(1),
        hybrid_logical_time: Hlc::new(10, 0),
    };
    let mut operations = directory(offset + 1, 0, "docs");
    let mut files = Vec::new();
    for (id, parent, name, content) in [
        (offset + 2, offset + 1, "edit.txt", b"edit".as_slice()),
        (offset + 3, 0, "keep.txt", b"keep"),
        (offset + 4, 0, "remove.txt", b"remove"),
        (offset + 5, 0, "replace.txt", b"old"),
    ] {
        let (ops, prepared) = file(id, parent, name, content);
        operations.extend(ops);
        files.push(prepared);
    }
    append(open, &basis, actor, operations, files)
}
#[test]
fn compiled_candidates_preserve_original_identity_and_exclude_later_work() {
    let root = std::env::temp_dir().join(format!("mesh-import-compiler-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let project_root = root.join("project");
    let lane_root = root.join("lane");
    fs::create_dir(&project_root).unwrap();
    fs::create_dir(&lane_root).unwrap();
    fs::write(project_root.join("editor.txt"), b"ongoing user work").unwrap();
    let capture = key(121);
    let agent = key(122);
    let importer = key(123);
    let mut project = OpenWorkspace::open(&project_root).unwrap();
    let base = initial(&mut project, &capture, 0);
    drop(project);
    let mut project = OpenWorkspace::open(&project_root).unwrap();
    let mut target = OpenWorkspace::open(&lane_root).unwrap();
    let input = initial(&mut target, &agent, 10);
    drop(target);
    let mut target = OpenWorkspace::open(&lane_root).unwrap();
    let basis = target
        .historical_authoring_basis(input, public(&agent))
        .unwrap();
    let changed = PreparedCheckpointFile::from_bytes(
        b"agent edit",
        &ChunkingConfig::default(),
        ManifestPagingPolicy::flat(),
    )
    .unwrap();
    let mut operations = vec![
        unlink(0, "docs", 11),
        link(ROOT, "archive", obj(11), VersionId::from_bytes([0; 32])),
        Operation::WriteFileVersion {
            object_id: obj(12),
            version_id: VersionId::from_bytes([90; 32]),
            parent_versions: vec![VersionId::from_bytes([12; 32])],
            manifest_id: ManifestId::from_bytes(*changed.manifest().id.as_bytes()),
            portable_metadata: PortableMetadata::new(true),
        },
        unlink(0, "keep.txt", 13),
        link(
            obj(11),
            "kept.txt",
            obj(13),
            VersionId::from_bytes([13; 32]),
        ),
        unlink(0, "remove.txt", 14),
        unlink(0, "replace.txt", 15),
    ];
    let (ops, replacement) = file(25, 0, "replace.txt", b"replacement");
    operations.extend(ops);
    operations.extend(directory(26, 0, "new"));
    let (ops, addition) = file(27, 26, "added.txt", b"addition");
    operations.extend(ops);
    let result = append(
        &mut target,
        &basis,
        &agent,
        operations,
        vec![changed, replacement, addition],
    );
    drop(target);
    let target = OpenWorkspace::open(&lane_root).unwrap();
    let snapshot = target.historical_workspace_preview(result).unwrap();
    let origins = super::super::project_mapping::import_correspondence(
        project.historical_workspace_preview(base).unwrap(),
        vec![(
            target.historical_workspace_preview(input).unwrap(),
            snapshot.clone(),
        )],
    )
    .unwrap();
    assert_eq!(origins.len(), 3);
    let basis = project
        .historical_authoring_basis(base, public(&capture))
        .unwrap();
    let (later_ops, later_file) = file(40, 0, "later.txt", b"later user work");
    append(&mut project, &basis, &capture, later_ops, vec![later_file]);
    drop(project);
    let mut project = OpenWorkspace::open(&project_root).unwrap();
    let before = fs::read(project.record_file()).unwrap();
    let candidate = Json::object([("candidate", Json::text("exact candidate receipt"))]);
    let plan = compile(
        &project,
        base,
        &target,
        &snapshot,
        &origins,
        &candidate,
        public(&importer),
    )
    .unwrap();
    let mut shuffled = snapshot.clone();
    shuffled.files.reverse();
    shuffled.directories.reverse();
    assert_eq!(
        plan.context(),
        compile(
            &project,
            base,
            &target,
            &shuffled,
            &origins,
            &candidate,
            public(&importer)
        )
        .unwrap()
        .context()
    );
    let other_candidate = Json::object([("candidate", Json::text("different retained receipt"))]);
    assert_ne!(
        plan.context().get("digest"),
        compile(
            &project,
            base,
            &target,
            &snapshot,
            &origins,
            &other_candidate,
            public(&importer)
        )
        .unwrap()
        .context()
        .get("digest")
    );
    let initial_snapshot = target.historical_workspace_preview(input).unwrap();
    let initial_origins = super::super::project_mapping::import_correspondence(
        project.historical_workspace_preview(base).unwrap(),
        vec![(initial_snapshot.clone(), initial_snapshot.clone())],
    )
    .unwrap();
    assert!(
        compile(
            &project,
            base,
            &target,
            &initial_snapshot,
            &initial_origins,
            &candidate,
            public(&importer)
        )
        .is_err(),
        "unchanged content must not invent an import operation"
    );
    assert_eq!(plan.files.len(), 3);
    assert_eq!(fs::read(project.record_file()).unwrap(), before);
    assert!(compile(
        &project,
        base,
        &target,
        &snapshot,
        &origins,
        &candidate,
        public(&capture)
    )
    .is_err());
    let mut wrong = origins.clone();
    wrong.insert(obj(99).to_string(), obj(1).to_string());
    assert!(compile(
        &project,
        base,
        &target,
        &snapshot,
        &wrong,
        &candidate,
        public(&importer)
    )
    .is_err());
    let mut wrong = origins.clone();
    wrong.insert(obj(12).to_string(), obj(1).to_string());
    assert!(compile(
        &project,
        base,
        &target,
        &snapshot,
        &wrong,
        &candidate,
        public(&importer)
    )
    .is_err());
    let mut corrupted = snapshot.clone();
    corrupted.files[0].byte_length += 1;
    assert!(compile(
        &project,
        base,
        &target,
        &corrupted,
        &origins,
        &candidate,
        public(&importer)
    )
    .is_err());
    let mut oversized = snapshot.clone();
    oversized.files[0].byte_length = u64::MAX;
    assert!(compile(
        &project,
        base,
        &target,
        &oversized,
        &origins,
        &candidate,
        public(&importer)
    )
    .is_err());
    let basis = project
        .historical_authoring_basis(base, public(&importer))
        .unwrap();
    let imported = append(
        &mut project,
        &basis,
        &importer,
        plan.historical.operations().to_vec(),
        plan.files,
    );
    drop(project);
    let project = OpenWorkspace::open(&project_root).unwrap();
    let actual = project.historical_capture_entries(imported).unwrap();
    assert_eq!(actual["archive"].binding.object_id, obj(1));
    assert_eq!(actual["archive/edit.txt"].binding.object_id, obj(2));
    assert_eq!(actual["archive/kept.txt"].binding.object_id, obj(3));
    assert_ne!(actual["replace.txt"].binding.object_id, obj(5));
    assert!(!actual.contains_key("remove.txt"));
    assert!(!actual.contains_key("later.txt"));
    for file in &snapshot.files {
        let saved = project
            .historical_workspace_file(imported, &file.path)
            .unwrap()
            .unwrap();
        assert_eq!(
            Blake3::digest_bytes(&saved.bytes).as_bytes(),
            file.content_digest.as_bytes()
        );
        assert_eq!(saved.executable, file.executable);
    }
    assert!(project.shared_version().is_none());
    assert_eq!(
        fs::read(project_root.join("editor.txt")).unwrap(),
        b"ongoing user work"
    );
    assert_eq!(
        project
            .historical_workspace_file(base, "docs/edit.txt")
            .unwrap()
            .unwrap()
            .bytes,
        b"edit"
    );
    assert!(project.saved_publication_review_bundle(imported).is_ok());
    drop(project);
    drop(target);
    fs::remove_dir_all(root).unwrap();
}
