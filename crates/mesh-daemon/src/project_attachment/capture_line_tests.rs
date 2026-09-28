use super::*;
use crate::project_attachment::{capture_line::CaptureLine, ObservationLimits};
use ed25519_dalek::{Signer as _, SigningKey};

struct Fixture {
    root: PathBuf,
    source: PathBuf,
    metadata: PathBuf,
    project: ProjectAttachment,
    key: SigningKey,
}
impl Fixture {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("mesh-capture-line-{name}-{}", std::process::id()));
        let source = root.join("source");
        let metadata = root.join("history");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&source).unwrap();
        fs::create_dir(&metadata).unwrap();
        fs::write(source.join("work.txt"), "initial capture\n").unwrap();
        let project = ProjectAttachment::register(&source, &metadata).unwrap();
        Self {
            root,
            source,
            metadata,
            project,
            key: SigningKey::from_bytes(&[81; 32]),
        }
    }
    fn input(&self) -> CapturedProjectInput {
        self.project
            .capture_inputs(crate::project_attachment::ObservationLimits::default())
            .unwrap()
    }
    fn save(&self) -> io::Result<SavedAttachmentVersion> {
        self.project.save_capture(
            &self.metadata,
            &self.input(),
            public(&self.key),
            |payload| -> Result<Signature, String> {
                Ok(Signature::from_bytes(
                    self.key.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
    }
    fn open(&self) -> (PinnedWorkspaceRoot, OpenWorkspace, String) {
        let store = external_store(&self.metadata, &self.project).unwrap();
        let configuration = self.project.history_configuration(&store, None).unwrap().0;
        let open =
            OpenWorkspace::open_attachment_store(&self.metadata, store.clone(), false).unwrap();
        (store, open, configuration)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn public(key: &SigningKey) -> PublicKey {
    PublicKey::from_bytes(key.verifying_key().to_bytes())
}
fn request(
    basis: &ManagedAuthoringBasis,
    key: &SigningKey,
    operations: Vec<Operation>,
) -> AuthenticatedOperationCheckpointRequest {
    let build = |signature| {
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
    let unsigned = build(Signature::from_bytes([0; 64]));
    let payload = SigningPayload::new(
        CHANGESET_SIGNATURE_DOMAIN,
        &operation_checkpoint_signing_body(&unsigned, &CaptureHead),
    );
    build(Signature::from_bytes(
        key.sign(payload.as_bytes()).to_bytes(),
    ))
}
fn append(
    store: &PinnedWorkspaceRoot,
    open: &mut OpenWorkspace,
    f: &Fixture,
    request: AuthenticatedOperationCheckpointRequest,
    files: Vec<PreparedCheckpointFile>,
) -> RecordDigest {
    let cas = Cas::with_filesystem(f.metadata.clone(), store.filesystem()).unwrap();
    save_authenticated_checkpoint(open, &cas, request, files, &CaptureHead)
        .unwrap()
        .changeset_id()
}
#[test]
fn observation_continues_on_its_own_history_after_a_signed_candidate_branch() {
    let f = Fixture::new("branches");
    let first = f.save().unwrap();
    let (store, mut open, _) = f.open();
    let importer = SigningKey::from_bytes(&[82; 32]);
    let basis = open
        .historical_authoring_basis(first.operation(), public(&importer))
        .unwrap();
    let entries = open.historical_capture_entries(first.operation()).unwrap();
    let file = &entries["work.txt"];
    let content = PreparedCheckpointFile::from_bytes(
        b"agent proposal\n",
        &ChunkingConfig::default(),
        ManifestPagingPolicy::flat(),
    )
    .unwrap();
    let folder = ObjectId::from_bytes([90; 16]);
    let operations = vec![
        Operation::WriteFileVersion {
            object_id: file.binding.object_id,
            version_id: VersionId::from_bytes([91; 32]),
            parent_versions: vec![file.file.unwrap().0],
            manifest_id: ManifestId::from_bytes(*content.manifest().id.as_bytes()),
            portable_metadata: PortableMetadata::new(false),
        },
        Operation::CreateDirectory { object_id: folder },
        Operation::LinkDirectoryEntry {
            directory_id: ROOT,
            name: NormalizedName::new("agent-only").unwrap(),
            object_id: folder,
            version_id: VersionId::from_bytes([0; 32]),
        },
    ];
    open.prepare_historical_operations(first.operation(), public(&importer), &operations)
        .unwrap();
    let candidate = append(
        &store,
        &mut open,
        &f,
        request(&basis, &importer, operations),
        vec![content],
    );
    drop(open);
    assert_eq!(
        f.save().unwrap(),
        first,
        "unchanged observation must not adopt a candidate"
    );
    assert_eq!(f.project.saved_versions(&f.metadata).unwrap(), vec![first]);
    fs::write(f.source.join("work.txt"), "human continued\n").unwrap();
    fs::create_dir(f.source.join("human-only")).unwrap();
    let second = f.save().unwrap();
    assert_eq!(
        f.project.saved_versions(&f.metadata).unwrap(),
        vec![first, second]
    );
    let (_, open, _) = f.open();
    let saved = open
        .historical_workspace_preview(second.operation())
        .unwrap();
    assert_eq!(
        saved
            .directories
            .iter()
            .map(|entry| entry.path.as_str())
            .collect::<Vec<_>>(),
        vec!["human-only"]
    );
    assert_eq!(
        open.historical_workspace_file(second.operation(), "work.txt")
            .unwrap()
            .unwrap()
            .bytes,
        b"human continued\n"
    );
    assert_eq!(
        open.historical_workspace_file(candidate, "work.txt")
            .unwrap()
            .unwrap()
            .bytes,
        b"agent proposal\n"
    );
    assert_eq!(
        open.linear_history(Some(second.operation())).unwrap(),
        vec![first.operation(), second.operation()]
    );
    // A legacy reader cannot safely infer an observation tip from branched history.
    let configuration = f.project.history_configuration(&store, None).unwrap().0;
    fs::write(f.metadata.join(HISTORY), &configuration).unwrap();
    fs::remove_file(f.metadata.join("attachment-capture-line.json")).unwrap();
    assert!(f.project.saved_versions(&f.metadata).is_err());
    assert!(f.save().is_err());
    assert!(!f.metadata.join("attachment-capture-line.json").exists());
    assert_eq!(
        fs::read_to_string(f.metadata.join(HISTORY)).unwrap(),
        configuration
    );
    assert_eq!(open.operations(), 3);
    assert!(open.shared_version().is_none());
    assert!(!f.source.join("agent-only").exists());
    assert_eq!(
        fs::read(f.source.join("work.txt")).unwrap(),
        b"human continued\n"
    );
}
#[test]
fn pending_capture_recovers_from_journal_truth_without_duplicate_appends() {
    let f = Fixture::new("pending");
    let first = f.save().unwrap();
    fs::write(f.source.join("work.txt"), "second saved capture\n").unwrap();
    let (store, mut open, configuration) = f.open();
    let line = CaptureLine::load(&store, &open, &configuration).unwrap();
    let basis = open
        .historical_authoring_basis(first.operation(), public(&f.key))
        .unwrap();
    let (operations, files) = prepare_snapshot(&open, &f.input(), &basis, line.head).unwrap();
    let request = request(&basis, &f.key, operations);
    let exact = authenticated_checkpoint_identity(&request, &CaptureHead).unwrap();
    line.begin(exact, &store, &configuration).unwrap();
    let pending = fs::read(f.metadata.join("attachment-capture-line.json")).unwrap();
    assert_eq!(append(&store, &mut open, &f, request, files), exact);
    drop(open);
    let versions = f.project.saved_versions(&f.metadata).unwrap();
    assert_eq!(versions.last().unwrap().operation(), exact);
    assert_eq!(
        fs::read(f.metadata.join("attachment-capture-line.json")).unwrap(),
        pending,
        "reading recovery must not rewrite metadata"
    );
    assert_eq!(f.save().unwrap().operation(), exact);
    let (_, open, _) = f.open();
    assert_eq!(open.operations(), 2);
    drop(open);
    assert!(
        Json::parse(&fs::read_to_string(f.metadata.join("attachment-capture-line.json")).unwrap())
            .unwrap()
            .get("pending")
            == Some(&Json::Null)
    );
    // A valid metadata transition interrupted before rename is replayed, then an uncommitted intent is cleared.
    let (store, open, configuration) = f.open();
    let line = CaptureLine::load(&store, &open, &configuration).unwrap();
    let previous = fs::read(f.metadata.join("attachment-capture-line.json")).unwrap();
    line.begin(RecordDigest::from_bytes([97; 32]), &store, &configuration)
        .unwrap();
    fs::rename(
        f.metadata.join("attachment-capture-line.json"),
        f.metadata.join("attachment-capture-line.pending"),
    )
    .unwrap();
    fs::write(f.metadata.join("attachment-capture-line.json"), &previous).unwrap();
    fs::set_permissions(
        f.metadata.join("attachment-capture-line.json"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    drop(open);
    assert_eq!(f.save().unwrap().operation(), exact);
    assert!(!f.metadata.join("attachment-capture-line.pending").exists());
    let (_, open, _) = f.open();
    assert_eq!(open.operations(), 2);
}
#[test]
fn legacy_capture_migration_preserves_identity_and_corrupt_or_missing_positions_refuse() {
    let f = Fixture::new("migration");
    let first = f.save().unwrap();
    let binding = Json::parse(&fs::read_to_string(f.metadata.join(HISTORY)).unwrap()).unwrap();
    let original = binding.get("capture_basis").unwrap().as_text().unwrap();
    fs::write(f.metadata.join(HISTORY), original).unwrap();
    fs::remove_file(f.metadata.join("attachment-capture-line.json")).unwrap();
    assert_eq!(f.project.saved_versions(&f.metadata).unwrap(), vec![first]);
    assert_eq!(
        fs::read_to_string(f.metadata.join(HISTORY)).unwrap(),
        original
    );
    assert_eq!(f.save().unwrap(), first);
    let protected_binding = fs::read(f.metadata.join(HISTORY)).unwrap();
    assert_ne!(protected_binding, original.as_bytes());
    let record = f.metadata.join("attachment-capture-line.json");
    let bytes = fs::read(&record).unwrap();
    fs::remove_file(&record).unwrap();
    assert!(f.project.saved_versions(&f.metadata).is_err());
    assert!(f.save().is_err());
    assert!(!record.exists());
    assert_eq!(
        fs::read(f.metadata.join(HISTORY)).unwrap(),
        protected_binding
    );
    fs::write(&record, &bytes).unwrap();
    fs::set_permissions(&record, fs::Permissions::from_mode(0o600)).unwrap();
    let temporary = f.metadata.join("attachment-capture-line.pending");
    fs::write(&temporary, b"interrupted malformed metadata").unwrap();
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(f.save().is_err());
    assert_eq!(
        fs::read(&temporary).unwrap(),
        b"interrupted malformed metadata"
    );
    assert_eq!(fs::read(&record).unwrap(), bytes);
    assert!(f
        .project
        .capture_inputs(ObservationLimits::default())
        .is_ok());
    assert_eq!(
        fs::read(f.source.join("work.txt")).unwrap(),
        b"initial capture\n"
    );
}

#[test]
fn capture_position_links_and_unrecognized_transitions_preserve_retained_work() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new("position-identity");
    let first = f.save().unwrap();
    let record = f.metadata.join("attachment-capture-line.json");
    let bytes = fs::read(&record).unwrap();
    let alias = f.metadata.join("capture-alias");
    fs::hard_link(&record, &alias).unwrap();
    assert!(f.save().is_err());
    assert!(f.project.saved_versions(&f.metadata).is_err());
    fs::remove_file(&record).unwrap();
    symlink(&alias, &record).unwrap();
    assert!(f.save().is_err());
    assert_eq!(fs::read(&alias).unwrap(), bytes);
    fs::remove_file(&record).unwrap();
    fs::remove_file(&alias).unwrap();
    fs::write(&record, &bytes).unwrap();
    fs::set_permissions(&record, fs::Permissions::from_mode(0o600)).unwrap();
    let changed = String::from_utf8(bytes.clone())
        .unwrap()
        .replace(&first.operation().to_string(), &"9".repeat(64));
    let temporary = f.metadata.join("attachment-capture-line.pending");
    fs::write(&temporary, &changed).unwrap();
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(f.save().is_err());
    assert_eq!(fs::read(&record).unwrap(), bytes);
    assert_eq!(fs::read_to_string(&temporary).unwrap(), changed);
    fs::remove_file(&temporary).unwrap();
    assert_eq!(f.save().unwrap(), first);
}

#[test]
fn nonprivate_oversized_and_rebound_positions_refuse_without_repair() {
    let f = Fixture::new("position-bounds");
    let first = f.save().unwrap();
    let record = f.metadata.join("attachment-capture-line.json");
    let original = fs::read(&record).unwrap();
    let binding = fs::read(f.metadata.join(HISTORY)).unwrap();
    let (_, open, configuration) = f.open();
    let journal_path = open.record_file().to_owned();
    let journal = fs::read(&journal_path).unwrap();
    drop(open);
    let encoded = String::from_utf8(original.clone()).unwrap();
    let wrong_binding = encoded.replace(
        &Blake3::digest_bytes(configuration.as_bytes()).to_string(),
        &"0".repeat(64),
    );
    assert_ne!(wrong_binding.as_bytes(), original);
    for (bytes, mode) in [
        (original.clone(), 0o644),
        (vec![b' '; 4097], 0o600),
        (wrong_binding.into_bytes(), 0o600),
        (
            encoded.replacen('{', "{\"extra\":null,", 1).into_bytes(),
            0o600,
        ),
    ] {
        fs::write(&record, &bytes).unwrap();
        fs::set_permissions(&record, fs::Permissions::from_mode(mode)).unwrap();
        assert!(f.project.saved_versions(&f.metadata).is_err());
        assert!(f.save().is_err());
        assert_eq!(fs::read(&record).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&record).unwrap().permissions().mode() & 0o777,
            mode
        );
        assert_eq!(fs::read(f.metadata.join(HISTORY)).unwrap(), binding);
        assert_eq!(fs::read(&journal_path).unwrap(), journal);
        assert_eq!(
            fs::read(f.source.join("work.txt")).unwrap(),
            b"initial capture\n"
        );
    }
    fs::write(&record, &original).unwrap();
    fs::set_permissions(&record, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(f.save().unwrap(), first);
    assert_eq!(fs::read(&journal_path).unwrap(), journal);
}
