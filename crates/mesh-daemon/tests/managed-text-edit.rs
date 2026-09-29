//! Native desktop edit coverage: real managed-folder bytes plus durable recovery truth.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Barrier};
use std::time::Duration;

use ed25519_dalek::{Signer as _, SigningKey};
use mesh_cas::{Cas, Digest32 as CasDigest};
use mesh_chunking::ChunkingConfig;
use mesh_daemon::ipc::surface::{nothing_to_recover, Operations, StartupSummary};
use mesh_daemon::{
    CheckpointRuntimeParameters, FileVersionCheckpointRequest, LiveDaemon, ManagedTextFileError,
    ManifestPagingPolicy, OpenWorkspace, PreparedFolderImport, MAX_MANAGED_TEXT_BYTES,
};
use mesh_operations::{
    ActorId, ActorSequence, CausalParents, HeadDerivation, HeadId, Hlc, ObjectId, PolicyEpoch,
    PortableMetadata, SessionId, Signature, TransitionCommitment, VersionId, WorkspaceId,
};
use mesh_store::{
    scan_journal, RecordDigest, RecoverySequence, SqlExecutor as _, Sqlite, SqliteRecoveryState,
    StoredRecord, RECOVERY_DATABASE_FILE_NAME,
};
use mesh_types::{Blake3, ContentDigest as _, PublicKey, Signature as MeshSignature};

fn scratch(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "mesh-managed-edit-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ))
}

fn startup() -> StartupSummary {
    StartupSummary::from(&nothing_to_recover())
}

fn parameters() -> CheckpointRuntimeParameters {
    CheckpointRuntimeParameters {
        idle_interval: Some(Duration::from_millis(50)),
        maximum_uncheckpointed_bytes: Some(65_536),
        maximum_uncheckpointed_interval: Some(Duration::from_millis(25)),
    }
}

/// Keep the automatic worker outside tests whose assertion is that a refused operation changes
/// no checkpoint byte. The production 25 ms maximum is deliberately exercised elsewhere; using
/// it here lets a legitimate background recovery transition race the before/after snapshot and
/// makes an unchanged refusal look like a mutation under suite load.
fn refusal_parameters() -> CheckpointRuntimeParameters {
    CheckpointRuntimeParameters {
        idle_interval: Some(Duration::from_secs(60)),
        maximum_uncheckpointed_bytes: Some(u64::MAX),
        maximum_uncheckpointed_interval: Some(Duration::from_secs(60)),
    }
}

fn imported(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    imported_with_bytes(name, b"first\n")
}

fn imported_with_bytes(name: &str, bytes: &[u8]) -> (PathBuf, PathBuf, PathBuf) {
    let parent = scratch(name);
    let _ = fs::remove_dir_all(&parent);
    let source = parent.join("source");
    let managed = parent.join("managed");
    fs::create_dir_all(source.join("docs")).expect("source folders");
    fs::write(source.join("docs/note.txt"), bytes).expect("source file");
    PreparedFolderImport::prepare(&source, &managed)
        .expect("verified import")
        .confirm_into_workspace()
        .expect("managed workspace");
    (parent, source, managed)
}

fn private_storage(managed: &Path) -> PathBuf {
    managed.join(".mesh")
}

fn inspected_digest(daemon: &LiveDaemon, path: &str) -> RecordDigest {
    daemon
        .inspect_managed_file(path)
        .expect("managed-file inspection")
        .content_digest()
}

fn inspected_executable(daemon: &LiveDaemon, path: &str) -> bool {
    daemon
        .inspect_managed_file(path)
        .expect("managed-file inspection")
        .executable()
}

struct CanonicalHead;

impl HeadDerivation for CanonicalHead {
    fn resulting_head(&self, commitment: &TransitionCommitment) -> HeadId {
        HeadId::from_bytes(*Blake3::digest_bytes(&commitment.canonical_bytes()).as_bytes())
    }
}

fn append_second_version(daemon: &LiveDaemon, managed: &Path, bytes: &[u8]) -> (String, String) {
    let opened = OpenWorkspace::open(managed).expect("history before second version");
    let history = &opened.file_histories()[0];
    let object = history.object();
    let earlier = history.current().expect("imported version").version();
    let records =
        scan_journal(&fs::read(private_storage(managed).join("records.mesh")).expect("journal"))
            .expect("whole journal");
    let parent = records
        .records()
        .iter()
        .find_map(|record| match record {
            StoredRecord::Operation(operation) => Some(operation.id),
            _ => None,
        })
        .expect("import operation");
    let current = VersionId::from_bytes([0x42; 32]);
    daemon
        .save_file_version(
            RecoverySequence::new(1).expect("sequence"),
            bytes,
            &ChunkingConfig::default(),
            ManifestPagingPolicy::flat(),
            FileVersionCheckpointRequest::new(
                WorkspaceId::from_bytes([0x11; 16]),
                ActorId::from_bytes([0x22; 32]),
                SessionId::from_bytes([0x33; 16]),
                ActorSequence::new(1),
                CausalParents::after(
                    mesh_operations::ChangeSetId::from_bytes(*parent.as_bytes()),
                    Vec::new(),
                ),
                HeadId::from_bytes([0x44; 32]),
                PolicyEpoch::new(1),
                Hlc::new(1, 0),
                ObjectId::from_bytes(*object.as_bytes()),
                current,
                vec![VersionId::from_bytes(*earlier.as_bytes())],
                PortableMetadata::default(),
                Signature::from_bytes([0x55; 64]),
            ),
            &CanonicalHead,
        )
        .expect("second durable version");
    fs::write(managed.join("docs/note.txt"), bytes).expect("materialized second version");
    (earlier.to_string(), current.to_string())
}

fn append_version_for_path(
    daemon: &LiveDaemon,
    managed: &Path,
    relative_path: &str,
    bytes: &[u8],
    version_byte: u8,
    recovery_sequence: u64,
) -> String {
    let opened = OpenWorkspace::open(managed).expect("history before file version");
    let history = opened
        .file_histories()
        .iter()
        .find(|history| history.path() == relative_path)
        .expect("selected imported file");
    let earlier = history.current().expect("imported version").version();
    let parent = opened
        .workspace_versions()
        .last()
        .expect("causally ready parent")
        .operation();
    let current = VersionId::from_bytes([version_byte; 32]);
    daemon
        .save_file_version(
            RecoverySequence::new(recovery_sequence).expect("sequence"),
            bytes,
            &ChunkingConfig::default(),
            ManifestPagingPolicy::flat(),
            FileVersionCheckpointRequest::new(
                WorkspaceId::from_bytes([version_byte; 16]),
                ActorId::from_bytes([version_byte.wrapping_add(1); 32]),
                SessionId::from_bytes([version_byte.wrapping_add(2); 16]),
                ActorSequence::new(1),
                CausalParents::after(
                    mesh_operations::ChangeSetId::from_bytes(*parent.as_bytes()),
                    Vec::new(),
                ),
                HeadId::from_bytes([version_byte.wrapping_add(3); 32]),
                PolicyEpoch::new(1),
                Hlc::new(u64::from(version_byte), 0),
                ObjectId::from_bytes(*history.object().as_bytes()),
                current,
                vec![VersionId::from_bytes(*earlier.as_bytes())],
                PortableMetadata::default(),
                Signature::from_bytes([version_byte.wrapping_add(4); 64]),
            ),
            &CanonicalHead,
        )
        .expect("durable file version");
    fs::write(managed.join(relative_path), bytes).expect("materialized file version");
    current.to_string()
}

#[test]
fn native_edit_changes_exact_os_bytes_and_restores_recovery_after_restart() {
    let (parent, _source, managed) = imported("restart");
    let records_before = fs::read(private_storage(&managed).join("records.mesh")).expect("journal");
    let current = {
        let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
        daemon.open_at_start(&managed).expect("open");
        let opened = daemon
            .read_managed_text_file("docs/note.txt")
            .expect("managed text");
        assert_eq!(opened.text(), "first\n");
        assert!(!opened.modified_from_current_version());
        let current = opened.current_version().to_owned();

        let saved = daemon
            .preserve_managed_text_edit(
                "docs/note.txt",
                "second\n",
                inspected_digest(&daemon, "docs/note.txt"),
                inspected_executable(&daemon, "docs/note.txt"),
            )
            .expect("recovery-preserved edit");
        assert!(saved.stable_after_idle());
        assert_eq!(saved.path(), "docs/note.txt");
        assert_eq!(
            saved.content_digest(),
            RecordDigest::from_bytes(*Blake3::digest_bytes(b"second\n").as_bytes())
        );
        assert_eq!(
            fs::read(managed.join("docs/note.txt")).unwrap(),
            b"second\n"
        );
        assert_eq!(
            fs::read(private_storage(&managed).join("records.mesh")).expect("journal after edit"),
            records_before,
            "unsigned native recovery must not masquerade as a ChangeSet"
        );
        let snapshot = daemon.checkpoint_snapshot().expect("checkpoint snapshot");
        assert_eq!(
            snapshot.open_window().expect("working window").last().get(),
            1
        );
        let recovery = snapshot.latest_recovery().expect("recovery bytes");
        assert_eq!(recovery.through().get(), 1);
        assert!(recovery.bytes().ends_with(b"second\n"));
        assert!(snapshot.last_meaningful().is_none());
        current
    };

    let restarted = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    restarted.open_at_start(&managed).expect("restart");
    let reopened = restarted
        .read_managed_text_file("docs/note.txt")
        .expect("managed bytes after restart");
    assert_eq!(reopened.text(), "second\n");
    assert_eq!(reopened.current_version(), current);
    let snapshot = restarted.checkpoint_snapshot().expect("restored recovery");
    assert_eq!(
        snapshot
            .latest_recovery()
            .expect("recovery")
            .through()
            .get(),
        1
    );
    assert!(snapshot.last_meaningful().is_none());
    assert_eq!(restarted.workspace_state().expect("state").records, 2);
    let _ = fs::remove_dir_all(parent);
}

#[test]
fn stale_native_edit_does_not_overwrite_a_newer_external_change() {
    let (parent, _source, managed) = imported("stale-native-edit");
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");
    let inspected = daemon
        .inspect_managed_file("docs/note.txt")
        .expect("initial inspection");
    assert_eq!(inspected.text(), Some("first\n"));

    let path = managed.join("docs/note.txt");
    fs::write(&path, b"newer external edit\n").expect("external edit after inspection");
    let result = daemon.preserve_managed_text_edit(
        "docs/note.txt",
        "stale desktop edit\n",
        inspected.content_digest(),
        inspected.executable(),
    );

    assert!(
        matches!(result, Err(ManagedTextFileError::StaleInspection)),
        "a stale desktop edit overwrote newer bytes"
    );
    assert_eq!(
        fs::read(&path).expect("newer bytes remain"),
        b"newer external edit\n"
    );
    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn stale_private_save_does_not_sign_unreviewed_external_bytes() {
    let (parent, _source, managed) = imported("stale-private-save");
    let signing = SigningKey::from_bytes(&[0x7a; 32]);
    let public = PublicKey::from_bytes(signing.verifying_key().to_bytes());
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");
    let inspected = daemon
        .inspect_managed_file("docs/note.txt")
        .expect("initial inspection");
    assert_eq!(inspected.text(), Some("first\n"));
    let records_before =
        fs::read(private_storage(&managed).join("records.mesh")).expect("journal before save");

    fs::write(managed.join("docs/note.txt"), b"unreviewed external edit\n")
        .expect("external edit after inspection");
    let result = daemon.save_managed_file_privately(
        "docs/note.txt",
        inspected.content_digest(),
        inspected.executable(),
        public,
        |payload| {
            Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                signing.sign(payload.as_bytes()).to_bytes(),
            ))
        },
    );

    assert!(
        matches!(result, Err(ManagedTextFileError::StaleInspection)),
        "Mesh signed bytes that were not inspected"
    );
    assert_eq!(
        fs::read(private_storage(&managed).join("records.mesh")).expect("journal after refusal"),
        records_before,
        "a stale save must append nothing"
    );
    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn an_external_editor_change_is_detected_and_can_be_saved_directly() {
    let (parent, _source, managed) = imported("external-direct-save");
    let signing = SigningKey::from_bytes(&[0x74; 32]);
    let public = PublicKey::from_bytes(signing.verifying_key().to_bytes());
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");

    let initial = daemon
        .read_managed_text_file("docs/note.txt")
        .expect("initial file");
    assert!(!initial.modified_from_current_version());
    fs::write(
        managed.join("docs/note.txt"),
        "changed in an ordinary local editor\n",
    )
    .expect("external edit");
    let detected = daemon
        .inspect_managed_file("docs/note.txt")
        .expect("changed file");
    assert!(detected.modified_from_current_version());
    assert_eq!(detected.current_version(), initial.current_version());

    let saved = daemon
        .save_managed_file_privately(
            "docs/note.txt",
            detected.content_digest(),
            detected.executable(),
            public,
            |payload| {
                Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                    signing.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .expect("direct authenticated save");
    assert!(saved.author_authenticated());
    assert!(saved.stable_after_idle());
    assert!(saved.meaningful_saved());
    assert!(!daemon
        .read_managed_text_file("docs/note.txt")
        .expect("saved file")
        .modified_from_current_version());

    drop(daemon);
    let restarted =
        LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("restart config");
    restarted.open_at_start(&managed).expect("restart");
    let reopened = restarted
        .read_managed_text_file("docs/note.txt")
        .expect("restarted file");
    assert_eq!(reopened.text(), "changed in an ordinary local editor\n");
    assert_eq!(reopened.current_version(), saved.version());
    assert!(!reopened.modified_from_current_version());
    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn desktop_edit_inspection_pairs_working_text_with_its_exact_saved_baseline() {
    let (parent, _source, managed) = imported("desktop-working-diff-baseline");
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");
    fs::write(managed.join("docs/note.txt"), "changed outside Mesh\n").expect("external edit");

    let (working, baseline) = daemon
        .inspect_managed_file_with_durable_text("docs/note.txt", MAX_MANAGED_TEXT_BYTES)
        .expect("desktop comparison inspection");

    assert_eq!(working.text(), Some("changed outside Mesh\n"));
    assert!(working.modified_from_current_version());
    assert_eq!(baseline.as_deref(), Some("first\n"));
    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn desktop_edit_inspection_never_reconstructs_an_over_limit_saved_baseline() {
    let retained = vec![b'x'; MAX_MANAGED_TEXT_BYTES + 1];
    let (parent, _source, managed) =
        imported_with_bytes("desktop-working-diff-bounded-baseline", &retained);
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");
    fs::write(managed.join("docs/note.txt"), "small working text\n").expect("external edit");

    let (working, baseline) = daemon
        .inspect_managed_file_with_durable_text("docs/note.txt", MAX_MANAGED_TEXT_BYTES)
        .expect("bounded desktop comparison inspection");

    assert_eq!(working.text(), Some("small working text\n"));
    assert_eq!(baseline, None);
    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn an_agent_created_native_file_is_discovered_adopted_and_restart_exact() {
    let (parent, source, managed) = imported("agent-created-file");
    let path = managed.join("agent-note.txt");
    fs::write(&path, b"created by an agent\n").expect("agent file");
    symlink(&path, managed.join("linked-agent-note.txt")).expect("agent symlink");
    let socket_alias = PathBuf::from(format!("/tmp/mesh-agent-socket-{}", std::process::id()));
    let _ = fs::remove_file(&socket_alias);
    symlink(&managed, &socket_alias).expect("short socket parent alias");
    let agent_socket = std::os::unix::net::UnixListener::bind(socket_alias.join("agent.sock"))
        .expect("agent socket");
    fs::create_dir_all(managed.join(".git/objects/ab")).expect("agent git metadata folders");
    fs::write(managed.join(".git/HEAD"), b"ref: refs/heads/agent\n").expect("agent git head");
    fs::write(managed.join(".git/objects/ab/object"), b"opaque git object")
        .expect("agent git object");
    fs::create_dir_all(managed.join("docs/.git")).expect("nested agent git metadata folder");
    fs::write(
        managed.join("docs/.git/config"),
        b"[core]\n\tbare = false\n",
    )
    .expect("nested agent git config");
    let signing = SigningKey::from_bytes(&[0x75; 32]);
    let public = PublicKey::from_bytes(signing.verifying_key().to_bytes());
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");

    assert_eq!(
        daemon
            .workspace_state()
            .expect("discovery state")
            .native_untracked_files,
        vec!["agent-note.txt"],
        "Git metadata belongs to the native adapter and must never enter workspace review"
    );
    assert_eq!(
        daemon
            .workspace_state()
            .expect("unsupported discovery state")
            .native_unsupported_entries
            .iter()
            .map(|entry| (entry.path(), entry.kind()))
            .collect::<Vec<_>>(),
        vec![
            ("agent.sock", "special"),
            ("linked-agent-note.txt", "symbolic-link"),
        ],
        "agent-created links and special objects must remain visible even though Mesh never follows them"
    );
    assert!(
        daemon
            .native_untracked_directories()
            .expect("directory discovery")
            .iter()
            .all(|directory| !directory.path().split('/').any(|part| part == ".git")),
        "Git metadata directories must not be offered for adoption"
    );
    let inspected = daemon
        .inspect_native_untracked_file("agent-note.txt")
        .expect("exact native inspection");
    assert_eq!(inspected.text(), Some("created by an agent\n"));
    let saved = daemon
        .adopt_native_file_privately(
            "agent-note.txt",
            inspected.content_digest(),
            inspected.executable(),
            public,
            |payload| {
                Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                    signing.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .expect("authenticated native adoption");
    assert!(saved.author_authenticated());
    assert!(saved.stable_after_idle());
    assert!(saved.meaningful_saved());
    let state = daemon.workspace_state().expect("adopted state");
    assert!(state.native_untracked_files.is_empty());
    assert!(state
        .entries
        .iter()
        .any(|entry| entry.path() == "agent-note.txt"));
    assert!(
        !source.join("agent-note.txt").exists(),
        "adopting native work changed the original import source"
    );

    drop(daemon);
    let restarted =
        LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("restart config");
    restarted.open_at_start(&managed).expect("restart");
    let reopened = restarted
        .read_managed_text_file("agent-note.txt")
        .expect("restarted adopted file");
    assert_eq!(reopened.text(), "created by an agent\n");
    assert_eq!(reopened.current_version(), saved.version());
    assert!(!reopened.modified_from_current_version());

    let journal = private_storage(&managed).join("records.mesh");
    let records_before = fs::read(&journal).expect("journal before agent export");
    let missing = restarted
        .preview_managed_file_export("agent-note.txt", &source)
        .expect("preview absent ordinary file");
    assert!(!missing.target_exists());
    assert_eq!(missing.target_byte_count(), None);
    assert_eq!(missing.target_content_digest(), None);
    assert_eq!(missing.target_executable(), None);

    fs::write(
        source.join("agent-note.txt"),
        b"ordinary file won the race\n",
    )
    .expect("concurrent ordinary create");
    let refused = restarted.export_managed_file(
        missing.path(),
        &source,
        missing.target_installation(),
        missing.target_parent_installation(),
        missing.target_file_installation(),
        missing.source_version(),
        missing.source_content_digest(),
        missing.source_executable(),
        missing.target_content_digest(),
        missing.target_executable(),
    );
    assert!(matches!(
        refused,
        Err(ManagedTextFileError::StaleExportTarget)
    ));
    assert_eq!(
        fs::read(source.join("agent-note.txt")).unwrap(),
        b"ordinary file won the race\n"
    );

    fs::remove_file(source.join("agent-note.txt")).expect("reset absent export name");
    let preview = restarted
        .preview_managed_file_export("agent-note.txt", &source)
        .expect("fresh absent preview");
    let exported = restarted
        .export_managed_file(
            preview.path(),
            &source,
            preview.target_installation(),
            preview.target_parent_installation(),
            preview.target_file_installation(),
            preview.source_version(),
            preview.source_content_digest(),
            preview.source_executable(),
            preview.target_content_digest(),
            preview.target_executable(),
        )
        .expect("create absent ordinary file");
    assert!(exported.created());
    assert_eq!(
        fs::read(source.join("agent-note.txt")).unwrap(),
        b"created by an agent\n"
    );
    assert_eq!(
        fs::metadata(source.join("agent-note.txt"))
            .expect("exported metadata")
            .permissions()
            .mode()
            & 0o777,
        0o644,
        "a non-executable export should use the same portable native mode as workspace versions"
    );
    assert_eq!(
        fs::read(&journal).expect("journal after agent export"),
        records_before,
        "exporting an agent-created saved file rewrote private history"
    );
    drop(agent_socket);
    fs::remove_file(socket_alias).expect("remove socket parent alias");
    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn presented_private_store_names_support_native_save_agent_adoption_and_pull_back() {
    let parent = scratch("presented-private-names");
    let _ = fs::remove_dir_all(&parent);
    let source = parent.join("source");
    let store = parent.join("private-store");
    fs::create_dir_all(source.join("chunks")).expect("ordinary project directory");
    fs::write(source.join("records.mesh"), b"ordinary project record\n")
        .expect("ordinary project file");
    fs::write(source.join("chunks/seed"), b"seed\n").expect("ordinary nested file");
    let (confirmed, _) = PreparedFolderImport::prepare_presented(&source, &store)
        .expect("presented import")
        .confirm_into_workspace()
        .expect("confirmed import");
    let managed = confirmed.destination().to_path_buf();
    drop(confirmed);

    let signing = SigningKey::from_bytes(&[0x35; 32]);
    let public = PublicKey::from_bytes(signing.verifying_key().to_bytes());
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon
        .open_at_start(&managed)
        .expect("open presented workspace");
    assert_eq!(
        daemon
            .read_managed_text_file("records.mesh")
            .expect("ordinary managed record name")
            .text(),
        "ordinary project record\n"
    );

    fs::write(managed.join("records.mesh"), b"saved ordinary record\n").expect("native edit");
    let inspected = daemon
        .inspect_managed_file("records.mesh")
        .expect("inspect ordinary record name");
    daemon
        .save_managed_file_privately(
            "records.mesh",
            inspected.content_digest(),
            inspected.executable(),
            public,
            |payload| {
                Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                    signing.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .expect("save ordinary record name");

    fs::write(managed.join("chunks/agent-output"), b"agent output\n").expect("new agent file");
    assert!(daemon
        .workspace_state()
        .expect("native discovery")
        .native_untracked_files
        .contains(&"chunks/agent-output".to_owned()));
    let inspected = daemon
        .inspect_native_untracked_file("chunks/agent-output")
        .expect("inspect agent output");
    daemon
        .adopt_native_file_privately(
            "chunks/agent-output",
            inspected.content_digest(),
            inspected.executable(),
            public,
            |payload| {
                Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                    signing.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .expect("adopt agent output");

    let preview = daemon
        .preview_managed_file_export("records.mesh", &source)
        .expect("preview pull-back of ordinary record name");
    daemon
        .export_managed_file(
            preview.path(),
            &source,
            preview.target_installation(),
            preview.target_parent_installation(),
            preview.target_file_installation(),
            preview.source_version(),
            preview.source_content_digest(),
            preview.source_executable(),
            preview.target_content_digest(),
            preview.target_executable(),
        )
        .expect("pull back ordinary record name");
    assert_eq!(
        fs::read(source.join("records.mesh")).expect("pulled-back bytes"),
        b"saved ordinary record\n"
    );
    assert_ne!(
        fs::read(store.join("records.mesh")).expect("private history"),
        b"saved ordinary record\n",
        "the external private journal must remain structurally distinct"
    );

    drop(daemon);
    let restarted =
        LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("restart config");
    restarted.open_at_start(&managed).expect("restart");
    assert_eq!(
        restarted
            .read_managed_text_file("records.mesh")
            .expect("reopened ordinary record name")
            .text(),
        "saved ordinary record\n"
    );
    assert_eq!(
        restarted
            .read_managed_text_file("chunks/agent-output")
            .expect("reopened agent output")
            .text(),
        "agent output\n"
    );
    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn native_agent_discovery_obeys_workspace_ignore_rules() {
    let parent = scratch("agent-ignore-rules");
    let _ = fs::remove_dir_all(&parent);
    let source = parent.join("source");
    let managed = parent.join("managed");
    fs::create_dir_all(source.join("docs")).expect("source folders");
    fs::write(source.join("docs/note.txt"), b"tracked\n").expect("source file");
    fs::write(source.join(".gitignore"), b"target/\n*.tmp\n").expect("repository ignores");
    fs::write(source.join(".meshignore"), b"generated/\n").expect("workspace ignores");
    PreparedFolderImport::prepare(&source, &managed)
        .expect("verified import")
        .confirm_into_workspace()
        .expect("managed workspace");

    fs::create_dir_all(managed.join("target/debug")).expect("ignored build output folder");
    fs::write(managed.join("target/debug/artifact"), b"build output")
        .expect("ignored build output");
    fs::create_dir_all(managed.join("generated")).expect("ignored generated folder");
    fs::write(managed.join("generated/report.txt"), b"generated output")
        .expect("ignored generated output");
    fs::write(managed.join("trace.tmp"), b"temporary output").expect("ignored temporary file");
    fs::write(managed.join("agent-note.txt"), b"review me\n").expect("agent result");

    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");
    assert_eq!(
        daemon
            .workspace_state()
            .expect("filtered discovery")
            .native_untracked_files,
        vec!["agent-note.txt"]
    );
    assert!(
        daemon
            .native_untracked_directories()
            .expect("filtered directory discovery")
            .is_empty(),
        "ignored directory trees must not become adoption frontiers"
    );
    assert!(
        daemon
            .inspect_native_untracked_file("target/debug/artifact")
            .is_err(),
        "a caller must not bypass discovery and adopt ignored build output"
    );

    fs::write(managed.join(".meshignore"), b"**/unsupported\n")
        .expect("malformed workspace ignores");
    let unavailable = daemon.workspace_state().expect("fail-closed state");
    assert!(unavailable.native_untracked_files.is_empty());
    assert!(unavailable
        .conditions
        .iter()
        .any(|condition| condition.code() == "exclusion-rules-unavailable"));
    assert!(daemon.native_untracked_directories().is_err());

    fs::write(managed.join(".meshignore"), b"generated/\n").expect("restore workspace ignores");
    fs::write(managed.join("late.log"), b"late agent result\n").expect("late agent file");
    let inspected = daemon
        .inspect_native_untracked_file("late.log")
        .expect("initially included file");
    let signing = SigningKey::from_bytes(&[0x78; 32]);
    let public = PublicKey::from_bytes(signing.verifying_key().to_bytes());
    let journal = private_storage(&managed).join("records.mesh");
    let journal_before = fs::read(&journal).expect("journal before ignore race");
    let refused = daemon.adopt_native_file_privately(
        "late.log",
        inspected.content_digest(),
        inspected.executable(),
        public,
        |payload| {
            fs::write(managed.join(".meshignore"), b"generated/\n*.log\n")
                .expect("exclude during signing");
            Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                signing.sign(payload.as_bytes()).to_bytes(),
            ))
        },
    );
    assert!(
        matches!(refused, Err(ManagedTextFileError::Authoring(_))),
        "an ignore rule added during signing must win before the append"
    );
    assert_eq!(fs::read(&journal).unwrap(), journal_before);

    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn agent_created_directory_tree_is_reviewed_whole_and_adopted_parent_first_without_rewriting_it() {
    let (parent, source, managed) = imported("agent-created-directory");
    let generated = managed.join("generated");
    let nested = generated.join("reports");
    fs::create_dir_all(&nested).expect("agent directory tree");
    fs::write(nested.join("result.txt"), b"agent result\n").expect("agent nested file");
    let signing = SigningKey::from_bytes(&[0x76; 32]);
    let public = PublicKey::from_bytes(signing.verifying_key().to_bytes());
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");

    let first = daemon
        .native_untracked_directories()
        .expect("complete native directory tree");
    assert_eq!(
        first.iter().map(|entry| entry.path()).collect::<Vec<_>>(),
        ["generated", "generated/reports"]
    );
    assert_eq!(
        daemon
            .workspace_state()
            .expect("initial state")
            .native_untracked_files,
        vec!["generated/reports/result.txt"]
    );
    let initially_inspected = daemon
        .inspect_native_untracked_file("generated/reports/result.txt")
        .expect("inspect the complete reviewed tree before its parents are durable");
    assert_eq!(initially_inspected.text(), Some("agent result\n"));

    let journal = private_storage(&managed).join("records.mesh");
    let before_race = fs::read(&journal).expect("journal before identity race");
    let displaced = managed.join("generated-before-race");
    let refused = daemon.adopt_native_directory_privately(
        first[0].path(),
        first[0].installation(),
        public,
        |payload| {
            fs::rename(&generated, &displaced).expect("displace inspected directory");
            fs::create_dir(&generated).expect("replacement directory");
            Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                signing.sign(payload.as_bytes()).to_bytes(),
            ))
        },
    );
    assert!(matches!(
        refused,
        Err(ManagedTextFileError::StaleInspection)
    ));
    assert_eq!(fs::read(&journal).unwrap(), before_race);
    fs::remove_dir(&generated).expect("remove replacement directory");
    fs::rename(&displaced, &generated).expect("restore inspected directory");

    let first = daemon
        .native_untracked_directories()
        .expect("restored native directory frontier");
    let adopted = daemon
        .adopt_native_directory_privately(
            first[0].path(),
            first[0].installation(),
            public,
            |payload| {
                Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                    signing.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .expect("adopt top-level directory");
    assert_eq!(adopted.action(), "adopt_folder");
    assert_eq!(adopted.to_path(), Some("generated"));
    assert!(adopted.author_authenticated());
    assert!(adopted.meaningful_saved());
    assert!(generated.is_dir());
    let installed = daemon
        .inspect_managed_directory_installation("generated")
        .expect("reopen adopted directory installation");
    assert_eq!(installed.path(), "generated");
    assert_eq!(installed.installation(), first[0].installation());
    let adopted_before_replacement = managed.join("generated-adopted-before-replacement");
    fs::rename(&generated, &adopted_before_replacement)
        .expect("displace adopted directory after durable append");
    fs::create_dir(&generated).expect("create post-adoption replacement directory");
    let replacement = daemon
        .inspect_managed_directory_installation("generated")
        .expect("reopen replacement directory installation");
    assert_ne!(replacement.installation(), first[0].installation());
    fs::remove_dir(&generated).expect("remove post-adoption replacement directory");
    fs::rename(&adopted_before_replacement, &generated)
        .expect("restore adopted directory after read-back check");
    assert_eq!(
        fs::read(nested.join("result.txt")).unwrap(),
        b"agent result\n"
    );

    let second = daemon
        .native_untracked_directories()
        .expect("second native directory frontier");
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].path(), "generated/reports");
    daemon
        .adopt_native_directory_privately(
            second[0].path(),
            second[0].installation(),
            public,
            |payload| {
                Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                    signing.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .expect("adopt nested directory");

    let state = daemon.workspace_state().expect("nested file discovery");
    assert_eq!(
        state.native_untracked_files,
        vec!["generated/reports/result.txt"]
    );
    let inspected = daemon
        .inspect_native_untracked_file("generated/reports/result.txt")
        .expect("inspect nested agent file");
    daemon
        .adopt_native_file_privately(
            inspected.path(),
            inspected.content_digest(),
            inspected.executable(),
            public,
            |payload| {
                Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                    signing.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .expect("adopt nested agent file");

    drop(daemon);
    let restarted = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("restart");
    restarted.open_at_start(&managed).expect("reopen");
    assert_eq!(
        restarted
            .read_managed_text_file("generated/reports/result.txt")
            .expect("restarted nested file")
            .text(),
        "agent result\n"
    );
    assert!(!source.join("generated").exists());

    let records_before_export = fs::read(&journal).expect("journal before tree export");
    let directory_batch = restarted
        .preview_managed_directory_exports(&source)
        .expect("preview missing directory batch");
    assert_eq!(directory_batch.target_root(), source.display().to_string());
    assert!(!directory_batch.target_installation().is_empty());
    assert_eq!(
        directory_batch.missing_paths(),
        ["generated", "generated/reports"]
    );
    let generated_preview = restarted
        .preview_managed_directory_export("generated", &source)
        .expect("preview top-level folder export");
    assert!(!generated_preview.target_exists());
    fs::write(source.join("generated"), b"ordinary file won folder race\n")
        .expect("competing ordinary file");
    let raced = restarted.export_managed_directory(
        generated_preview.path(),
        &source,
        generated_preview.source_directory_installation(),
        generated_preview.target_installation(),
        generated_preview.target_parent_installation(),
        generated_preview.target_directory_installation(),
    );
    assert!(matches!(
        raced,
        Err(ManagedTextFileError::StaleExportTarget)
    ));
    assert_eq!(
        fs::read(source.join("generated")).unwrap(),
        b"ordinary file won folder race\n"
    );
    fs::remove_file(source.join("generated")).expect("clear competing file");

    let generated_preview = restarted
        .preview_managed_directory_export("generated", &source)
        .expect("fresh top-level folder preview");
    let generated_export = restarted
        .export_managed_directory(
            generated_preview.path(),
            &source,
            generated_preview.source_directory_installation(),
            generated_preview.target_installation(),
            generated_preview.target_parent_installation(),
            generated_preview.target_directory_installation(),
        )
        .expect("create top-level exported folder");
    assert!(generated_export.created());
    assert!(source.join("generated").is_dir());

    let reports_preview = restarted
        .preview_managed_directory_export("generated/reports", &source)
        .expect("preview nested folder after parent exists");
    let reports_export = restarted
        .export_managed_directory(
            reports_preview.path(),
            &source,
            reports_preview.source_directory_installation(),
            reports_preview.target_installation(),
            reports_preview.target_parent_installation(),
            reports_preview.target_directory_installation(),
        )
        .expect("create nested exported folder");
    assert!(reports_export.created());
    assert!(restarted
        .preview_managed_directory_exports(&source)
        .expect("all directory exports now exist")
        .missing_paths()
        .is_empty());

    let file_preview = restarted
        .preview_managed_file_export("generated/reports/result.txt", &source)
        .expect("preview nested file after folder pass");
    restarted
        .export_managed_file(
            file_preview.path(),
            &source,
            file_preview.target_installation(),
            file_preview.target_parent_installation(),
            file_preview.target_file_installation(),
            file_preview.source_version(),
            file_preview.source_content_digest(),
            file_preview.source_executable(),
            file_preview.target_content_digest(),
            file_preview.target_executable(),
        )
        .expect("export nested agent result");
    assert_eq!(
        fs::read(source.join("generated/reports/result.txt")).unwrap(),
        b"agent result\n"
    );
    assert_eq!(
        fs::read(&journal).unwrap(),
        records_before_export,
        "folder and file export rewrote Mesh history"
    );

    let existing_preview = restarted
        .preview_managed_directory_export("generated", &source)
        .expect("preview existing exported directory");
    assert!(existing_preview.target_exists());
    let existing = restarted
        .export_managed_directory(
            existing_preview.path(),
            &source,
            existing_preview.source_directory_installation(),
            existing_preview.target_installation(),
            existing_preview.target_parent_installation(),
            existing_preview.target_directory_installation(),
        )
        .expect("confirm existing structural directory");
    assert!(!existing.created());

    let stale_preview = restarted
        .preview_managed_directory_export("generated", &source)
        .expect("preview directory before identity replacement");
    fs::rename(source.join("generated"), source.join("generated-displaced"))
        .expect("displace exported directory");
    fs::create_dir(source.join("generated")).expect("replacement exported directory");
    let stale = restarted.export_managed_directory(
        stale_preview.path(),
        &source,
        stale_preview.source_directory_installation(),
        stale_preview.target_installation(),
        stale_preview.target_parent_installation(),
        stale_preview.target_directory_installation(),
    );
    assert!(matches!(
        stale,
        Err(ManagedTextFileError::StaleExportTarget)
    ));
    assert!(source.join("generated").is_dir());
    assert_eq!(
        fs::read(source.join("generated-displaced/reports/result.txt")).unwrap(),
        b"agent result\n"
    );
    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn direct_export_mutations_refuse_shared_agent_custody_until_release() {
    let (parent, source, managed) = imported("custody-refuses-direct-exports");
    let signing = SigningKey::from_bytes(&[0xa7; 32]);
    let public = PublicKey::from_bytes(signing.verifying_key().to_bytes());
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters())
        .expect("checkpoint configuration");
    daemon
        .open_at_start(&managed)
        .expect("open managed workspace");
    daemon
        .move_managed_entry_privately("docs/note.txt", "docs/renamed.txt", public, |payload| {
            Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                signing.sign(payload.as_bytes()).to_bytes(),
            ))
        })
        .expect("save rename before handoff");
    let assigned = daemon.workspace_state().expect("state after saved rename");

    let directory = daemon
        .preview_managed_directory_export("docs", &source)
        .expect("preview directory export");
    let file = daemon
        .preview_managed_file_export("docs/renamed.txt", &source)
        .expect("preview file export");
    let retired = daemon
        .preview_retired_export("docs/note.txt", &source)
        .expect("preview retired removal");
    assert!(retired.removable());

    let generation = daemon
        .acquire_workspace_agent_custody(
            &assigned.root,
            &assigned.digest,
            &assigned.installation,
            false,
            None,
        )
        .expect("assign workspace to agent");
    let directory_refusal = daemon.export_managed_directory(
        directory.path(),
        &source,
        directory.source_directory_installation(),
        directory.target_installation(),
        directory.target_parent_installation(),
        directory.target_directory_installation(),
    );
    let file_refusal = daemon.export_managed_file(
        file.path(),
        &source,
        file.target_installation(),
        file.target_parent_installation(),
        file.target_file_installation(),
        file.source_version(),
        file.source_content_digest(),
        file.source_executable(),
        file.target_content_digest(),
        file.target_executable(),
    );
    let removal_refusal = daemon.remove_retired_export(
        retired.path(),
        &source,
        retired.entry_type(),
        retired.source_version(),
        retired.source_content_digest(),
        retired.source_executable(),
        retired.target_installation(),
        retired.target_parent_installation(),
        retired.target_entry_installation(),
        retired.target_content_digest(),
        retired.target_executable(),
    );
    assert!(matches!(
        directory_refusal,
        Err(ManagedTextFileError::Recovery(_))
    ));
    assert!(matches!(
        file_refusal,
        Err(ManagedTextFileError::Recovery(_))
    ));
    assert!(matches!(
        removal_refusal,
        Err(ManagedTextFileError::Recovery(_))
    ));
    assert!(source.join("docs/note.txt").is_file());
    assert!(!source.join("docs/renamed.txt").exists());

    daemon
        .release_workspace_agent_custody(
            &assigned.root,
            &assigned.digest,
            &assigned.installation,
            &generation,
        )
        .expect("finish agent handoff");
    daemon
        .export_managed_directory(
            directory.path(),
            &source,
            directory.source_directory_installation(),
            directory.target_installation(),
            directory.target_parent_installation(),
            directory.target_directory_installation(),
        )
        .expect("directory export after release");
    daemon
        .export_managed_file(
            file.path(),
            &source,
            file.target_installation(),
            file.target_parent_installation(),
            file.target_file_installation(),
            file.source_version(),
            file.source_content_digest(),
            file.source_executable(),
            file.target_content_digest(),
            file.target_executable(),
        )
        .expect("file export after release");
    daemon
        .remove_retired_export(
            retired.path(),
            &source,
            retired.entry_type(),
            retired.source_version(),
            retired.source_content_digest(),
            retired.source_executable(),
            retired.target_installation(),
            retired.target_parent_installation(),
            retired.target_entry_installation(),
            retired.target_content_digest(),
            retired.target_executable(),
        )
        .expect("retired removal after release");
    assert_eq!(
        fs::read(source.join("docs/renamed.txt")).expect("exported renamed file"),
        b"first\n"
    );
    assert!(!source.join("docs/note.txt").exists());
    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn exact_agent_finish_preflight_reads_complete_folder_without_releasing_custody() {
    let parent = scratch("exact-agent-finish-preflight");
    let _ = fs::remove_dir_all(&parent);
    let source = parent.join("source");
    let managed = parent.join("managed");
    fs::create_dir_all(source.join("docs")).expect("source folders");
    fs::write(source.join("docs/note.txt"), b"first\n").expect("tracked source file");
    fs::write(source.join("docs/missing.txt"), b"later missing\n")
        .expect("second tracked source file");
    PreparedFolderImport::prepare(&source, &managed)
        .expect("verified import")
        .confirm_into_workspace()
        .expect("managed workspace");
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), refusal_parameters())
        .expect("checkpoint configuration");
    daemon
        .open_at_start(&managed)
        .expect("open managed workspace");
    fs::write(
        managed.join("docs/note.txt"),
        b"agent changed tracked file\n",
    )
    .expect("agent tracked edit");
    fs::create_dir(managed.join("generated")).expect("agent directory");
    fs::write(managed.join("generated/result.txt"), b"agent result\n").expect("agent native file");
    fs::remove_file(managed.join("docs/missing.txt")).expect("agent removed tracked file");
    symlink("docs/note.txt", managed.join("agent-link")).expect("unsupported agent link");
    let assigned = daemon
        .workspace_state()
        .expect("refreshed native inventory");
    assert!(assigned.native_inventory_complete);
    let generation = daemon
        .acquire_workspace_agent_custody(
            &assigned.root,
            &assigned.digest,
            &assigned.installation,
            false,
            None,
        )
        .expect("assign exact workspace");

    assert!(daemon
        .with_verified_managed_workspace(
            &assigned.root,
            &assigned.digest,
            &assigned.installation,
            || daemon.inspect_managed_file("docs/note.txt"),
        )
        .is_err());
    let preflight = daemon
        .inspect_agent_finish_preflight(
            &assigned.root,
            &assigned.digest,
            &assigned.installation,
            &generation,
        )
        .expect("exact assigned preflight");
    assert_eq!(preflight.root(), assigned.root);
    assert_eq!(preflight.digest(), assigned.digest);
    assert_eq!(preflight.installation(), assigned.installation);
    assert_eq!(preflight.generation(), generation);
    assert!(preflight
        .managed_files()
        .iter()
        .any(|file| { file.path() == "docs/note.txt" && file.modified_from_current_version() }));
    assert!(preflight
        .native_files()
        .iter()
        .any(|file| file.path() == "generated/result.txt"));
    assert!(preflight
        .native_directories()
        .iter()
        .any(|directory| directory.path() == "generated"));
    assert!(preflight
        .missing_files()
        .iter()
        .any(|file| file.path() == "docs/missing.txt"));
    assert!(preflight
        .unsupported_entries()
        .iter()
        .any(|entry| entry.path() == "agent-link" && entry.kind() == "symbolic-link"));
    assert!(preflight
        .managed_files()
        .iter()
        .all(|file| file.text().is_none()));
    assert!(preflight
        .native_files()
        .iter()
        .all(|file| file.text().is_none()));
    assert!(daemon
        .inspect_agent_finish_preflight(
            &assigned.root,
            &assigned.digest,
            &assigned.installation,
            &"00".repeat(16),
        )
        .is_err());
    assert!(daemon
        .workspace_agent_custody_for_workspace(
            &assigned.root,
            &assigned.digest,
            &assigned.installation,
        )
        .expect("custody after preflight")
        .is_assigned());

    daemon
        .release_workspace_agent_custody(
            &assigned.root,
            &assigned.digest,
            &assigned.installation,
            &generation,
        )
        .expect("release after preflight");
    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn a_saved_file_exports_atomically_to_the_original_without_rewriting_mesh_history() {
    let (parent, source, managed) = imported("export-original");
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");
    let (_earlier, current) = append_second_version(&daemon, &managed, b"saved second\n");
    let journal = private_storage(&managed).join("records.mesh");
    let records_before = fs::read(&journal).expect("journal before export");

    let preview = daemon
        .preview_managed_file_export("docs/note.txt", &source)
        .expect("exact export preview");
    assert_eq!(preview.path(), "docs/note.txt");
    assert_eq!(preview.source_version(), current);
    assert_eq!(preview.source_text(), Some("saved second\n"));
    assert_eq!(preview.target_text(), Some("first\n"));
    assert!(!preview.identical());

    let exported = daemon
        .export_managed_file(
            preview.path(),
            &source,
            preview.target_installation(),
            preview.target_parent_installation(),
            preview.target_file_installation(),
            preview.source_version(),
            preview.source_content_digest(),
            preview.source_executable(),
            preview.target_content_digest(),
            preview.target_executable(),
        )
        .expect("atomic export");
    assert_eq!(exported.path(), "docs/note.txt");
    assert_eq!(exported.target_root(), source.display().to_string());
    assert!(!exported.created());
    assert_eq!(
        fs::read(source.join("docs/note.txt")).unwrap(),
        b"saved second\n"
    );
    assert_eq!(
        fs::read(managed.join("docs/note.txt")).unwrap(),
        b"saved second\n"
    );
    assert_eq!(
        fs::read(&journal).expect("journal after export"),
        records_before,
        "copying a saved version out must not append or rewrite Mesh history"
    );

    drop(daemon);
    let restarted = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    restarted.open_at_start(&managed).expect("restart");
    let reopened = restarted
        .read_managed_text_file("docs/note.txt")
        .expect("saved managed file");
    assert_eq!(reopened.current_version(), current);
    assert_eq!(reopened.text(), "saved second\n");
    assert!(!reopened.modified_from_current_version());
    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn independent_agents_pull_back_disjoint_changes_without_reverting_each_other() {
    let parent = scratch("independent-agent-pull-back");
    let _ = fs::remove_dir_all(&parent);
    let source = parent.join("source");
    let managed_a = parent.join("managed-a");
    let managed_b = parent.join("managed-b");
    fs::create_dir_all(&source).expect("source");
    fs::write(source.join("a.txt"), b"baseline a\n").expect("a baseline");
    fs::write(source.join("b.txt"), b"baseline b\n").expect("b baseline");
    PreparedFolderImport::prepare(&source, &managed_a)
        .expect("agent A import")
        .confirm_into_workspace()
        .expect("agent A workspace");
    PreparedFolderImport::prepare(&source, &managed_b)
        .expect("agent B import")
        .confirm_into_workspace()
        .expect("agent B workspace");

    let agent_a = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("agent A");
    agent_a.open_at_start(&managed_a).expect("open A");
    append_version_for_path(&agent_a, &managed_a, "a.txt", b"agent A result\n", 0xa1, 1);
    let a = agent_a
        .preview_managed_file_export("a.txt", &source)
        .expect("agent A preview");
    assert_eq!(a.target_relation(), "imported-unchanged");
    assert!(a.replace_allowed());
    agent_a
        .export_managed_file(
            a.path(),
            &source,
            a.target_installation(),
            a.target_parent_installation(),
            a.target_file_installation(),
            a.source_version(),
            a.source_content_digest(),
            a.source_executable(),
            a.target_content_digest(),
            a.target_executable(),
        )
        .expect("agent A pull-back");

    let agent_b = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("agent B");
    agent_b.open_at_start(&managed_b).expect("open B");
    append_version_for_path(&agent_b, &managed_b, "b.txt", b"agent B result\n", 0xb1, 1);
    let batch = agent_b
        .preview_all_managed_file_exports(&source)
        .expect("one authoritative Pull-back preview batch");
    assert_eq!(
        batch
            .iter()
            .map(|preview| preview.path())
            .collect::<Vec<_>>(),
        ["a.txt", "b.txt"]
    );
    assert_eq!(batch[0].target_relation(), "external-or-other-workspace");
    assert!(!batch[0].replace_allowed());
    assert_eq!(batch[0].source_text(), None);
    assert_eq!(batch[0].target_text(), None);
    assert_eq!(batch[1].target_relation(), "imported-unchanged");
    assert!(batch[1].replace_allowed());
    assert_eq!(batch[1].source_text(), None);
    assert_eq!(batch[1].target_text(), None);
    let stale_a = agent_b
        .preview_managed_file_export("a.txt", &source)
        .expect("agent B sees agent A result");
    assert_eq!(stale_a.target_relation(), "external-or-other-workspace");
    assert!(!stale_a.replace_allowed());
    let refused = agent_b.export_managed_file(
        stale_a.path(),
        &source,
        stale_a.target_installation(),
        stale_a.target_parent_installation(),
        stale_a.target_file_installation(),
        stale_a.source_version(),
        stale_a.source_content_digest(),
        stale_a.source_executable(),
        stale_a.target_content_digest(),
        stale_a.target_executable(),
    );
    assert!(matches!(
        refused,
        Err(ManagedTextFileError::UnprovenExportReplacement)
    ));

    let b = agent_b
        .preview_managed_file_export("b.txt", &source)
        .expect("agent B preview");
    assert_eq!(b.target_relation(), "imported-unchanged");
    assert!(b.replace_allowed());
    agent_b
        .export_managed_file(
            b.path(),
            &source,
            b.target_installation(),
            b.target_parent_installation(),
            b.target_file_installation(),
            b.source_version(),
            b.source_content_digest(),
            b.source_executable(),
            b.target_content_digest(),
            b.target_executable(),
        )
        .expect("agent B pull-back");

    append_version_for_path(
        &agent_b,
        &managed_b,
        "b.txt",
        b"agent B refined result\n",
        0xb2,
        2,
    );
    let refined_b = agent_b
        .preview_managed_file_export("b.txt", &source)
        .expect("agent B repeat preview");
    assert_eq!(refined_b.target_relation(), "prior-pull-back");
    assert!(refined_b.replace_allowed());
    agent_b
        .export_managed_file(
            refined_b.path(),
            &source,
            refined_b.target_installation(),
            refined_b.target_parent_installation(),
            refined_b.target_file_installation(),
            refined_b.source_version(),
            refined_b.source_content_digest(),
            refined_b.source_executable(),
            refined_b.target_content_digest(),
            refined_b.target_executable(),
        )
        .expect("repeat pull-back from the same workspace");

    assert_eq!(fs::read(source.join("a.txt")).unwrap(), b"agent A result\n");
    assert_eq!(
        fs::read(source.join("b.txt")).unwrap(),
        b"agent B refined result\n"
    );
    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn pull_back_receipt_failure_preserves_an_existing_ordinary_file_identity() {
    let (parent, source, managed) = imported("export-receipt-failure-existing");
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");
    append_second_version(&daemon, &managed, b"saved second\n");
    let preview = daemon
        .preview_managed_file_export("docs/note.txt", &source)
        .expect("exact replacement preview");
    let target = source.join("docs/note.txt");
    let identity_before = fs::metadata(&target)
        .expect("ordinary target metadata")
        .ino();
    fs::remove_dir_all(private_storage(&managed).join("pull-back-receipts"))
        .expect("remove existing origin receipt directory");
    fs::write(
        private_storage(&managed).join("pull-back-receipts"),
        b"blocks receipt directory creation",
    )
    .expect("block receipt publication");

    let refusal = daemon.export_managed_file(
        preview.path(),
        &source,
        preview.target_installation(),
        preview.target_parent_installation(),
        preview.target_file_installation(),
        preview.source_version(),
        preview.source_content_digest(),
        preview.source_executable(),
        preview.target_content_digest(),
        preview.target_executable(),
    );

    assert!(matches!(
        refusal,
        Err(ManagedTextFileError::UnprovenExportReplacement)
    ));
    assert_eq!(
        fs::read(&target).expect("ordinary target remains"),
        b"first\n"
    );
    assert_eq!(
        fs::metadata(&target).expect("ordinary target metadata").ino(),
        identity_before,
        "failed provenance preparation replaced and rolled back instead of stopping before exchange"
    );
    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn export_refuses_stale_destination_and_unsaved_managed_bytes() {
    let (parent, source, managed) = imported("export-stale");
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");
    append_second_version(&daemon, &managed, b"saved second\n");
    let preview = daemon
        .preview_managed_file_export("docs/note.txt", &source)
        .expect("preview");
    fs::write(source.join("docs/note.txt"), b"newer ordinary edit\n")
        .expect("destination changes after preview");
    let stale = daemon.export_managed_file(
        preview.path(),
        &source,
        preview.target_installation(),
        preview.target_parent_installation(),
        preview.target_file_installation(),
        preview.source_version(),
        preview.source_content_digest(),
        preview.source_executable(),
        preview.target_content_digest(),
        preview.target_executable(),
    );
    assert!(matches!(
        stale,
        Err(ManagedTextFileError::StaleExportTarget)
    ));
    assert_eq!(
        fs::read(source.join("docs/note.txt")).unwrap(),
        b"newer ordinary edit\n"
    );

    fs::write(managed.join("docs/note.txt"), b"unsaved managed edit\n")
        .expect("managed file changes without private save");
    let unsaved = daemon.preview_managed_file_export("docs/note.txt", &source);
    assert!(matches!(
        unsaved,
        Err(ManagedTextFileError::UnsavedWorkingCopy)
    ));
    assert_eq!(
        fs::read(source.join("docs/note.txt")).unwrap(),
        b"newer ordinary edit\n"
    );
    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn export_refuses_replaced_destination_root_links_and_managed_subtrees() {
    let (parent, source, managed) = imported("export-confinement");
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");
    append_second_version(&daemon, &managed, b"saved second\n");
    let preview = daemon
        .preview_managed_file_export("docs/note.txt", &source)
        .expect("preview");

    let parent_swap = parent.join("parent-swap");
    fs::create_dir_all(parent_swap.join("docs")).expect("nested export parent");
    fs::write(parent_swap.join("docs/note.txt"), b"first\n").expect("nested export target");
    let parent_preview = daemon
        .preview_managed_file_export("docs/note.txt", &parent_swap)
        .expect("nested preview");
    fs::rename(parent_swap.join("docs"), parent_swap.join("displaced-docs"))
        .expect("displace exact nested parent");
    fs::create_dir(parent_swap.join("docs")).expect("replacement nested parent");
    fs::write(parent_swap.join("docs/note.txt"), b"first\n")
        .expect("byte-identical replacement target");
    let replaced_parent = daemon.export_managed_file(
        parent_preview.path(),
        &parent_swap,
        parent_preview.target_installation(),
        parent_preview.target_parent_installation(),
        parent_preview.target_file_installation(),
        parent_preview.source_version(),
        parent_preview.source_content_digest(),
        parent_preview.source_executable(),
        parent_preview.target_content_digest(),
        parent_preview.target_executable(),
    );
    assert!(matches!(
        replaced_parent,
        Err(ManagedTextFileError::StaleExportTarget)
    ));
    assert_eq!(
        fs::read(parent_swap.join("docs/note.txt")).unwrap(),
        b"first\n"
    );

    let displaced = parent.join("displaced-source");
    fs::rename(&source, &displaced).expect("displace exact preview root");
    fs::create_dir_all(source.join("docs")).expect("replacement root");
    fs::write(source.join("docs/note.txt"), b"replacement root bytes\n")
        .expect("replacement target");
    let replaced = daemon.export_managed_file(
        preview.path(),
        &source,
        preview.target_installation(),
        preview.target_parent_installation(),
        preview.target_file_installation(),
        preview.source_version(),
        preview.source_content_digest(),
        preview.source_executable(),
        preview.target_content_digest(),
        preview.target_executable(),
    );
    assert!(matches!(
        replaced,
        Err(ManagedTextFileError::StaleExportTarget)
    ));
    assert_eq!(
        fs::read(source.join("docs/note.txt")).unwrap(),
        b"replacement root bytes\n"
    );

    let linked = parent.join("linked-target");
    fs::create_dir_all(linked.join("docs")).expect("link target root");
    fs::write(linked.join("outside.txt"), b"outside\n").expect("outside file");
    symlink(linked.join("outside.txt"), linked.join("docs/note.txt")).expect("destination symlink");
    assert!(daemon
        .preview_managed_file_export("docs/note.txt", &linked)
        .is_err());
    assert_eq!(fs::read(linked.join("outside.txt")).unwrap(), b"outside\n");

    let nested = managed.join("export-target");
    fs::create_dir_all(nested.join("docs")).expect("nested target");
    fs::write(nested.join("docs/note.txt"), b"nested\n").expect("nested file");
    assert!(matches!(
        daemon.preview_managed_file_export("docs/note.txt", &nested),
        Err(ManagedTextFileError::UnsafeExportTarget)
    ));
    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn export_refuses_private_storage_siblings_of_a_presented_working_folder() {
    let parent = scratch("export-private-storage");
    let _ = fs::remove_dir_all(&parent);
    let source = parent.join("source");
    let storage = parent.join("managed.mesh");
    fs::create_dir_all(&source).expect("source");
    fs::write(source.join("note.txt"), b"saved bytes\n").expect("source file");
    PreparedFolderImport::prepare_presented(&source, &storage)
        .expect("verified presented import")
        .confirm_into_workspace()
        .expect("presented workspace");
    let managed = storage.join(mesh_daemon::workspace::PRESENTED_DIRECTORY_NAME);
    let private_target = storage.join("chunks");
    assert!(private_target.is_dir(), "the private CAS target exists");

    let daemon =
        LiveDaemon::with_checkpoint_runtime(startup(), refusal_parameters()).expect("config");
    daemon
        .open_at_start(&managed)
        .expect("open presented workspace");
    assert!(matches!(
        daemon.preview_managed_file_export("note.txt", &private_target),
        Err(ManagedTextFileError::UnsafeExportTarget)
    ));
    assert!(
        !private_target.join("note.txt").exists(),
        "a private storage directory was accepted as an ordinary export target"
    );

    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn an_agent_file_changed_after_inspection_is_not_adopted() {
    let (parent, _source, managed) = imported("agent-file-race");
    let path = managed.join("agent-note.txt");
    fs::write(&path, b"inspected bytes\n").expect("agent file");
    let signing = SigningKey::from_bytes(&[0x76; 32]);
    let public = PublicKey::from_bytes(signing.verifying_key().to_bytes());
    let daemon =
        LiveDaemon::with_checkpoint_runtime(startup(), refusal_parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");
    let inspected = daemon
        .inspect_native_untracked_file("agent-note.txt")
        .expect("native inspection");
    let journal = private_storage(&managed).join("records.mesh");
    let before = fs::read(&journal).expect("journal before refusal");

    let refused = daemon.adopt_native_file_privately(
        "agent-note.txt",
        inspected.content_digest(),
        inspected.executable(),
        public,
        |payload| {
            fs::write(&path, b"newer agent bytes\n").expect("concurrent agent edit");
            Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                signing.sign(payload.as_bytes()).to_bytes(),
            ))
        },
    );
    assert!(matches!(
        refused,
        Err(ManagedTextFileError::StaleInspection)
    ));
    assert_eq!(
        fs::read(&journal).expect("journal after refusal"),
        before,
        "a file changed after inspection was durably adopted"
    );
    assert_eq!(fs::read(&path).unwrap(), b"newer agent bytes\n");
    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn executable_metadata_changed_while_a_private_save_is_signed_stays_working() {
    let (parent, _source, managed) = imported("private-save-mode-race");
    let path = managed.join("docs/note.txt");
    fs::write(&path, "content selected for private save\n").expect("external edit");
    let mut permissions = fs::metadata(&path).expect("metadata").permissions();
    permissions.set_mode(0o644);
    fs::set_permissions(&path, permissions).expect("start non-executable");

    let signing = SigningKey::from_bytes(&[0x76; 32]);
    let public = PublicKey::from_bytes(signing.verifying_key().to_bytes());
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");

    let saved = daemon
        .save_managed_file_privately(
            "docs/note.txt",
            inspected_digest(&daemon, "docs/note.txt"),
            inspected_executable(&daemon, "docs/note.txt"),
            public,
            |payload| {
                let mut permissions = fs::metadata(&path)
                    .expect("metadata during signing")
                    .permissions();
                permissions.set_mode(0o755);
                fs::set_permissions(&path, permissions).expect("ordinary editor changes mode");
                Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                    signing.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .expect("durable version remains valid but cannot be acknowledged as current");

    assert!(
        fs::metadata(&path)
            .expect("final metadata")
            .permissions()
            .mode()
            & 0o111
            != 0
    );
    assert!(
        !saved.stable_after_idle(),
        "byte equality hid executable metadata that no longer matched the signed version"
    );
    assert!(!saved.meaningful_saved());
    let checkpoint = daemon.checkpoint_snapshot().expect("checkpoint");
    let pending = checkpoint
        .pending_meaningful()
        .expect("mismatched working copy stays pending");
    assert!(
        checkpoint.open_window().is_some(),
        "mismatched working-copy metadata was incorrectly acknowledged"
    );
    assert!(
        checkpoint
            .latest_recovery()
            .is_some_and(|recovery| recovery.through() == pending.through()),
        "the maximum interval must preserve the pending version even when stability later fails"
    );

    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn an_executable_metadata_only_change_can_be_saved_privately() {
    let (parent, _source, managed) = imported("private-save-mode-only");
    let path = managed.join("docs/note.txt");
    let before = fs::read(&path).expect("original bytes");
    let mut permissions = fs::metadata(&path).expect("metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).expect("ordinary editor changes mode");

    let signing = SigningKey::from_bytes(&[0x77; 32]);
    let public = PublicKey::from_bytes(signing.verifying_key().to_bytes());
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");

    assert!(
        daemon
            .inspect_managed_file("docs/note.txt")
            .expect("desktop inspection")
            .modified_from_current_version(),
        "the desktop must expose a metadata-only version change as saveable work"
    );
    assert!(
        daemon
            .read_managed_text_file("docs/note.txt")
            .expect("desktop text read")
            .modified_from_current_version(),
        "the text-editor surface must agree with full file inspection"
    );

    let saved = daemon
        .save_managed_file_privately(
            "docs/note.txt",
            inspected_digest(&daemon, "docs/note.txt"),
            inspected_executable(&daemon, "docs/note.txt"),
            public,
            |payload| {
                Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                    signing.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .expect("portable metadata is part of a file version, not an unchanged byte shortcut");

    assert_eq!(fs::read(&path).expect("same bytes"), before);
    assert!(saved.stable_after_idle());
    assert!(saved.meaningful_saved());
    assert_eq!(
        OpenWorkspace::open(&managed)
            .expect("history")
            .file_histories()[0]
            .retained()
            .len(),
        2
    );

    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn retained_version_restore_roundtrips_executable_metadata_without_rewriting_history() {
    let (parent, _source, managed) = imported("restore-executable-metadata");
    let path = managed.join("docs/note.txt");
    let signing = SigningKey::from_bytes(&[0x78; 32]);
    let public = PublicKey::from_bytes(signing.verifying_key().to_bytes());
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");
    let initial = OpenWorkspace::open(&managed).expect("initial history");
    let history = &initial.file_histories()[0];
    let object = history.object().to_string();
    let non_executable = history
        .current()
        .expect("initial version")
        .version()
        .to_string();

    let mut permissions = fs::metadata(&path).expect("metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).expect("make executable");
    let executable = daemon
        .save_managed_file_privately(
            "docs/note.txt",
            inspected_digest(&daemon, "docs/note.txt"),
            true,
            public,
            |payload| {
                Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                    signing.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .expect("save executable metadata version");
    assert!(executable.meaningful_saved());
    let journal = fs::read(private_storage(&managed).join("records.mesh")).expect("journal");
    let summary = daemon
        .workspace_state()
        .expect("state before restore preview");
    let preview = daemon
        .preview_managed_working_copy_restore_for_workspace(
            &summary.root,
            &summary.digest,
            &summary.installation,
            &object,
            &non_executable,
        )
        .expect("physical metadata preview")
        .encode();
    assert!(preview.contains("\"working_copy\":{"));
    assert!(preview.contains("\"executable\":true"));
    assert!(preview.contains("\"target\":{"));
    assert!(preview.contains("\"executable\":false"));

    daemon
        .restore_managed_file_version(
            &object,
            &non_executable,
            inspected_digest(&daemon, "docs/note.txt"),
            true,
        )
        .expect("restore non-executable version");
    assert_eq!(
        fs::metadata(&path)
            .expect("restored metadata")
            .permissions()
            .mode()
            & 0o111,
        0
    );
    assert_eq!(
        fs::read(private_storage(&managed).join("records.mesh")).unwrap(),
        journal
    );

    daemon
        .restore_managed_file_version(
            &object,
            executable.version(),
            inspected_digest(&daemon, "docs/note.txt"),
            false,
        )
        .expect("undo to executable version");
    assert_ne!(
        fs::metadata(&path)
            .expect("undo metadata")
            .permissions()
            .mode()
            & 0o111,
        0
    );
    assert_eq!(
        fs::read(private_storage(&managed).join("records.mesh")).unwrap(),
        journal
    );

    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn binary_and_large_local_changes_are_inspected_saved_and_restored_without_text_decoding() {
    let (parent, _source, managed) = imported("binary-large-inspection");
    let signing = SigningKey::from_bytes(&[0x75; 32]);
    let public = PublicKey::from_bytes(signing.verifying_key().to_bytes());
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");
    let path = managed.join("docs/note.txt");

    let binary = vec![0x00, 0xff, 0x80, b'M', b'e', b's', b'h'];
    fs::write(&path, &binary).expect("binary external edit");
    let inspected = daemon
        .inspect_managed_file("docs/note.txt")
        .expect("binary inspection");
    assert_eq!(inspected.byte_count(), binary.len() as u64);
    assert_eq!(
        inspected.content_digest().as_bytes(),
        Blake3::digest_bytes(&binary).as_bytes()
    );
    assert!(inspected.text().is_none());
    assert!(inspected.modified_from_current_version());
    let binary_saved = daemon
        .save_managed_file_privately(
            "docs/note.txt",
            inspected.content_digest(),
            inspected.executable(),
            public,
            |payload| {
                Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                    signing.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .expect("binary private save");
    assert!(binary_saved.meaningful_saved());
    assert!(!daemon
        .inspect_managed_file("docs/note.txt")
        .expect("saved binary")
        .modified_from_current_version());

    let large = vec![b'x'; MAX_MANAGED_TEXT_BYTES + 1];
    fs::write(&path, &large).expect("large external edit");
    let inspected = daemon
        .inspect_managed_file("docs/note.txt")
        .expect("large inspection");
    assert_eq!(inspected.byte_count(), large.len() as u64);
    assert!(inspected.text().is_none());
    assert!(inspected.modified_from_current_version());
    let large_saved = daemon
        .save_managed_file_privately(
            "docs/note.txt",
            inspected.content_digest(),
            inspected.executable(),
            public,
            |payload| {
                Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                    signing.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .expect("large private save");
    assert!(large_saved.meaningful_saved());

    drop(daemon);
    let restarted =
        LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("restart config");
    restarted.open_at_start(&managed).expect("restart");
    let reopened = restarted
        .inspect_managed_file("docs/note.txt")
        .expect("restarted large file");
    assert_eq!(reopened.current_version(), large_saved.version());
    assert_eq!(reopened.byte_count(), large.len() as u64);
    assert!(reopened.text().is_none());
    assert!(!reopened.modified_from_current_version());
    assert_eq!(fs::read(path).expect("large bytes survive"), large);
    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn authenticated_private_save_appends_history_and_restores_saved_state() {
    let (parent, _source, managed) = imported("authenticated-private-save");
    let signing = SigningKey::from_bytes(&[0x71; 32]);
    let public = PublicKey::from_bytes(signing.verifying_key().to_bytes());
    let saved_version;
    let saved_changeset;
    {
        let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
        daemon.open_at_start(&managed).expect("open");
        daemon
            .preserve_managed_text_edit(
                "docs/note.txt",
                "second private version\n",
                inspected_digest(&daemon, "docs/note.txt"),
                inspected_executable(&daemon, "docs/note.txt"),
            )
            .expect("recovery first");
        let saved = daemon
            .save_managed_file_privately(
                "docs/note.txt",
                inspected_digest(&daemon, "docs/note.txt"),
                inspected_executable(&daemon, "docs/note.txt"),
                public,
                |payload| {
                    Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                        signing.sign(payload.as_bytes()).to_bytes(),
                    ))
                },
            )
            .expect("authenticated private save");
        assert!(saved.author_authenticated());
        assert!(saved.stable_after_idle());
        assert!(saved.meaningful_saved());
        assert_eq!(saved.path(), "docs/note.txt");
        let first_private_version = saved.version().to_owned();

        daemon
            .preserve_managed_text_edit(
                "docs/note.txt",
                "third private version\n",
                inspected_digest(&daemon, "docs/note.txt"),
                inspected_executable(&daemon, "docs/note.txt"),
            )
            .expect("second recovery");
        let saved = daemon
            .save_managed_file_privately(
                "docs/note.txt",
                inspected_digest(&daemon, "docs/note.txt"),
                inspected_executable(&daemon, "docs/note.txt"),
                public,
                |payload| {
                    Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                        signing.sign(payload.as_bytes()).to_bytes(),
                    ))
                },
            )
            .expect("second authenticated private save");
        assert_ne!(saved.version(), first_private_version);
        saved_version = saved.version().to_owned();
        saved_changeset = saved.changeset().to_owned();

        let snapshot = daemon.checkpoint_snapshot().expect("checkpoint");
        assert!(snapshot.open_window().is_none());
        assert!(snapshot.last_meaningful().is_some());
        let opened = daemon
            .read_managed_text_file("docs/note.txt")
            .expect("new current file");
        assert_eq!(opened.text(), "third private version\n");
        assert_eq!(opened.current_version(), saved_version);
    }

    let records =
        scan_journal(&fs::read(private_storage(&managed).join("records.mesh")).expect("journal"))
            .expect("whole journal");
    assert_eq!(
        records
            .records()
            .iter()
            .filter(|record| matches!(record, StoredRecord::Operation(_)))
            .count(),
        3
    );
    assert!(records.records().iter().any(|record| matches!(
        record,
        StoredRecord::Operation(operation) if operation.id.to_string() == saved_changeset
    )));

    let restarted =
        LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("restart config");
    restarted.open_at_start(&managed).expect("restart open");
    let file = restarted
        .read_managed_text_file("docs/note.txt")
        .expect("restart file");
    assert_eq!(file.current_version(), saved_version);
    assert_eq!(file.text(), "third private version\n");
    let snapshot = restarted.checkpoint_snapshot().expect("restart checkpoint");
    assert!(snapshot.open_window().is_none());
    assert!(snapshot.last_meaningful().is_some());
    let history = OpenWorkspace::open(&managed).expect("history");
    assert_eq!(history.file_histories()[0].retained().len(), 3);

    let authored = records
        .records()
        .iter()
        .filter_map(|record| match record {
            StoredRecord::Operation(operation)
                if operation.actor.as_bytes() == public.as_bytes() =>
            {
                Some(operation.actor_sequence)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(authored, vec![1, 2]);

    let replacement_signing = SigningKey::from_bytes(&[0x72; 32]);
    let replacement_public = PublicKey::from_bytes(replacement_signing.verifying_key().to_bytes());
    restarted
        .preserve_managed_text_edit(
            "docs/note.txt",
            "after desktop restart\n",
            inspected_digest(&restarted, "docs/note.txt"),
            inspected_executable(&restarted, "docs/note.txt"),
        )
        .expect("restart recovery");
    let replacement = restarted
        .save_managed_file_privately(
            "docs/note.txt",
            inspected_digest(&restarted, "docs/note.txt"),
            inspected_executable(&restarted, "docs/note.txt"),
            replacement_public,
            |payload| {
                Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                    replacement_signing.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .expect("new process actor can continue causally");
    assert!(replacement.meaningful_saved());
    let final_open = OpenWorkspace::open(&managed).expect("final history");
    assert_eq!(final_open.file_histories()[0].retained().len(), 4);
    assert_eq!(
        restarted
            .read_managed_text_file("docs/note.txt")
            .expect("final file")
            .text(),
        "after desktop restart\n"
    );

    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn managed_private_save_uses_the_configured_idle_interval() {
    let (parent, _source, managed) = imported("configured-idle");
    let signing = SigningKey::from_bytes(&[0x73; 32]);
    let public = PublicKey::from_bytes(signing.verifying_key().to_bytes());
    let mut configured = parameters();
    configured.idle_interval = Some(Duration::from_millis(80));
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), configured).expect("config");
    daemon.open_at_start(&managed).expect("open");
    daemon
        .preserve_managed_text_edit(
            "docs/note.txt",
            "configured idle version\n",
            inspected_digest(&daemon, "docs/note.txt"),
            inspected_executable(&daemon, "docs/note.txt"),
        )
        .expect("recovery first");

    let saved = daemon
        .save_managed_file_privately(
            "docs/note.txt",
            inspected_digest(&daemon, "docs/note.txt"),
            inspected_executable(&daemon, "docs/note.txt"),
            public,
            |payload| {
                Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                    signing.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .expect("authenticated private save");

    assert!(saved.stable_after_idle());
    assert!(
        saved.meaningful_saved(),
        "the managed save used a separate 50 ms constant instead of the configured 80 ms interval"
    );
    assert!(
        daemon
            .checkpoint_snapshot()
            .expect("checkpoint")
            .open_window()
            .is_none(),
        "the pending window was left open with no later worker scheduled"
    );
    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn managed_private_save_preserves_recovery_at_maximum_before_idle() {
    let (parent, _source, managed) = imported("maximum-before-idle");
    let signing = SigningKey::from_bytes(&[0x74; 32]);
    let public = PublicKey::from_bytes(signing.verifying_key().to_bytes());
    let mut configured = parameters();
    configured.idle_interval = Some(Duration::from_millis(500));
    configured.maximum_uncheckpointed_interval = Some(Duration::from_millis(20));
    let daemon = Arc::new(
        LiveDaemon::with_checkpoint_runtime(startup(), configured).expect("checkpoint config"),
    );
    daemon.open_at_start(&managed).expect("open");
    daemon
        .preserve_managed_text_edit(
            "docs/note.txt",
            "maximum recovery version\n",
            inspected_digest(&daemon, "docs/note.txt"),
            inspected_executable(&daemon, "docs/note.txt"),
        )
        .expect("working recovery");

    let (saved_tx, saved_rx) = mpsc::channel();
    let saving = Arc::clone(&daemon);
    let save = std::thread::spawn(move || {
        let result = saving.save_managed_file_privately(
            "docs/note.txt",
            inspected_digest(&saving, "docs/note.txt"),
            inspected_executable(&saving, "docs/note.txt"),
            public,
            |payload| {
                Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                    signing.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        );
        saved_tx.send(result).expect("report managed save");
    });

    let pending_deadline = std::time::Instant::now() + Duration::from_secs(2);
    let pending_through = loop {
        if let Some(pending) = daemon
            .checkpoint_snapshot()
            .expect("checkpoint while saving")
            .pending_meaningful()
        {
            break pending.through();
        }
        assert!(
            std::time::Instant::now() < pending_deadline,
            "the managed save never published its pending sequence"
        );
        std::thread::yield_now();
    };

    let recovery_deadline = std::time::Instant::now() + Duration::from_millis(250);
    loop {
        let snapshot = daemon
            .checkpoint_snapshot()
            .expect("checkpoint before idle settlement");
        if snapshot
            .latest_recovery()
            .is_some_and(|recovery| recovery.through() == pending_through)
        {
            break;
        }
        assert!(
            std::time::Instant::now() < recovery_deadline,
            "the managed operation missed its maximum recovery deadline"
        );
        std::thread::yield_now();
    }
    assert!(
        saved_rx.try_recv().is_err(),
        "maximum recovery must not bypass the later stability check and meaningful settlement"
    );

    let saved = saved_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("managed save completes after idle")
        .expect("managed save succeeds");
    assert!(saved.stable_after_idle());
    assert!(saved.meaningful_saved());
    save.join().expect("save thread");
    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn managed_recovery_stability_uses_the_configured_idle_interval() {
    let (parent, _source, managed) = imported("configured-recovery-idle");
    let idle = Duration::from_millis(250);
    let mut configured = parameters();
    configured.idle_interval = Some(idle);
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), configured).expect("config");
    daemon.open_at_start(&managed).expect("open");

    // Measure the configured minimum directly. A second thread sleeping 125 ms can mutate
    // before preservation starts or after its final read when the host delays either thread.
    // External edits during settling have their own observable-replacement regression below.
    let digest = inspected_digest(&daemon, "docs/note.txt");
    let executable = inspected_executable(&daemon, "docs/note.txt");
    let started = std::time::Instant::now();
    let preserved = daemon
        .preserve_managed_text_edit("docs/note.txt", "candidate recovery\n", digest, executable)
        .expect("recovery preservation");
    let elapsed = started.elapsed();

    assert!(
        elapsed >= idle,
        "recovery stability returned in {elapsed:?}, before the configured {idle:?} idle interval"
    );
    assert!(preserved.stable_after_idle());
    assert_eq!(
        fs::read(managed.join("docs/note.txt")).expect("preserved bytes"),
        b"candidate recovery\n"
    );
    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn a_managed_save_cannot_settle_a_replacement_workspace_with_the_same_sequence() {
    let (first_parent, _first_source, first) = imported("settle-original");
    let (second_parent, _second_source, second) = imported("settle-replacement");

    {
        let mut replacement_parameters = parameters();
        replacement_parameters.idle_interval = Some(Duration::from_secs(5));
        let replacement =
            LiveDaemon::with_checkpoint_runtime(startup(), replacement_parameters).expect("config");
        replacement
            .open_at_start(&second)
            .expect("replacement opens");
        append_second_version(&replacement, &second, b"pending replacement\n");
        let snapshot = replacement
            .checkpoint_snapshot()
            .expect("replacement checkpoint");
        assert_eq!(
            snapshot
                .pending_meaningful()
                .expect("replacement starts pending")
                .through()
                .get(),
            1
        );
        assert!(snapshot.last_meaningful().is_none());
    }

    let daemon = Arc::new(
        LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("original config"),
    );
    daemon.open_at_start(&first).expect("original opens");
    fs::write(first.join("docs/note.txt"), "changed in original\n").expect("external edit");
    let signing = SigningKey::from_bytes(&[0x79; 32]);
    let public = PublicKey::from_bytes(signing.verifying_key().to_bytes());
    let (signing_tx, signing_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let saving = Arc::clone(&daemon);
    let save = std::thread::spawn(move || {
        saving.save_managed_file_privately(
            "docs/note.txt",
            inspected_digest(&saving, "docs/note.txt"),
            inspected_executable(&saving, "docs/note.txt"),
            public,
            |payload| {
                // Hold the real save inside its workspace authority guard. Observing pending
                // state alone leaves only a 50 ms window: the save can legitimately finish
                // while this test thread is descheduled before it starts workspace.open.
                signing_tx.send(()).expect("report save holds authority");
                release_rx.recv().expect("release original save");
                Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                    signing.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
    });

    signing_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("the original save reached its signing boundary");
    let (opened_tx, opened_rx) = mpsc::channel();
    let opening = {
        let daemon = Arc::clone(&daemon);
        let second = second.clone();
        std::thread::spawn(move || {
            let result = daemon.open_at_start(&second);
            opened_tx.send(result).expect("report replacement open");
        })
    };
    let blocked = opened_rx.recv_timeout(Duration::from_millis(10));
    release_tx
        .send(())
        .expect("allow original save to complete");
    assert!(
        matches!(blocked, Err(mpsc::RecvTimeoutError::Timeout)),
        "workspace.open must wait while the original managed save holds authority"
    );

    let saved = save.join().expect("save thread").expect("original save");
    assert!(
        saved.stable_after_idle(),
        "the original bytes stayed stable"
    );
    assert!(
        saved.meaningful_saved(),
        "the original save must settle its own workspace before replacement"
    );
    opened_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("replacement open resumes after save")
        .expect("replacement opens");
    opening.join().expect("open thread");
    let replacement = daemon
        .checkpoint_snapshot()
        .expect("replacement checkpoint remains current");
    assert!(replacement.last_meaningful().is_none());
    assert_eq!(
        replacement
            .pending_meaningful()
            .expect("replacement remains pending")
            .through()
            .get(),
        1
    );

    let reopened_original =
        LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("reopen config");
    reopened_original
        .open_at_start(&first)
        .expect("original reopens");
    let original = reopened_original
        .checkpoint_snapshot()
        .expect("original checkpoint");
    assert_eq!(
        original
            .last_meaningful()
            .expect("original settled before replacement opened")
            .through()
            .get(),
        1
    );
    assert!(original.pending_meaningful().is_none());

    fs::remove_dir_all(first_parent).expect("first cleanup");
    fs::remove_dir_all(second_parent).expect("second cleanup");
}

#[test]
fn invalid_private_save_signature_writes_no_version_or_checkpoint_transition() {
    let (parent, _source, managed) = imported("invalid-private-signature");
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");
    daemon
        .preserve_managed_text_edit(
            "docs/note.txt",
            "unsigned attempt\n",
            inspected_digest(&daemon, "docs/note.txt"),
            inspected_executable(&daemon, "docs/note.txt"),
        )
        .expect("recovery first");
    let journal_before =
        fs::read(private_storage(&managed).join("records.mesh")).expect("journal before");
    let snapshot_before = daemon.checkpoint_snapshot().expect("snapshot before");
    let signing = SigningKey::from_bytes(&[0x73; 32]);
    let public = PublicKey::from_bytes(signing.verifying_key().to_bytes());
    let result = daemon.save_managed_file_privately(
        "docs/note.txt",
        inspected_digest(&daemon, "docs/note.txt"),
        inspected_executable(&daemon, "docs/note.txt"),
        public,
        |_payload| Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes([0; 64])),
    );
    assert!(matches!(result, Err(ManagedTextFileError::Authoring(_))));
    assert_eq!(
        fs::read(private_storage(&managed).join("records.mesh")).expect("journal after"),
        journal_before
    );
    assert_eq!(
        daemon.checkpoint_snapshot().expect("snapshot after"),
        snapshot_before
    );
    assert_eq!(
        OpenWorkspace::open(&managed)
            .expect("history")
            .file_histories()[0]
            .retained()
            .len(),
        1
    );
    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn failed_recovery_persistence_rolls_back_bytes_and_checkpoint_activity() {
    let (parent, _source, managed) = imported("recovery-persistence-rollback");
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");
    let snapshot_before = daemon.checkpoint_snapshot().expect("snapshot before");
    let bytes_before = fs::read(managed.join("docs/note.txt")).expect("bytes before");

    let mut database = Sqlite::open(private_storage(&managed).join(RECOVERY_DATABASE_FILE_NAME))
        .expect("recovery sqlite");
    database
        .execute_batch(&format!(
            "CREATE TRIGGER refuse_managed_recovery
             BEFORE INSERT ON {}
             BEGIN SELECT RAISE(ABORT, 'planted managed recovery persistence failure'); END;",
            SqliteRecoveryState::table_name(),
        ))
        .expect("failure injection");
    drop(database);

    let result = daemon.preserve_managed_text_edit(
        "docs/note.txt",
        "rejected bytes\n",
        inspected_digest(&daemon, "docs/note.txt"),
        inspected_executable(&daemon, "docs/note.txt"),
    );
    assert!(matches!(result, Err(ManagedTextFileError::Checkpoint(_))));
    assert_eq!(
        fs::read(managed.join("docs/note.txt")).expect("rolled-back bytes"),
        bytes_before,
        "a failed recovery transition restores the materialized file"
    );
    assert_eq!(
        daemon.checkpoint_snapshot().expect("snapshot after"),
        snapshot_before,
        "a rejected edit must not leave phantom checkpoint activity in memory"
    );

    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn traversal_links_binary_and_oversize_refuse_without_workspace_mutation() {
    let (parent, _source, managed) = imported("refusals");
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");
    let journal = fs::read(private_storage(&managed).join("records.mesh")).expect("journal");
    let original = fs::read(managed.join("docs/note.txt")).expect("file");

    assert!(matches!(
        daemon.preserve_managed_text_edit(
            "../outside",
            "bad",
            RecordDigest::from_bytes([0; 32]),
            false,
        ),
        Err(ManagedTextFileError::InvalidPath)
    ));
    assert!(matches!(
        daemon.preserve_managed_text_edit(
            "records.mesh",
            "bad",
            RecordDigest::from_bytes([0; 32]),
            false,
        ),
        Err(ManagedTextFileError::NotRegularFile | ManagedTextFileError::ReservedPath)
    ));
    assert!(matches!(
        daemon.preserve_managed_text_edit(
            ".mesh/records.mesh",
            "bad",
            RecordDigest::from_bytes([0; 32]),
            false,
        ),
        Err(ManagedTextFileError::ReservedPath)
    ));
    assert!(matches!(
        daemon.preserve_managed_text_edit(
            "docs/note.txt",
            &"x".repeat(MAX_MANAGED_TEXT_BYTES + 1),
            inspected_digest(&daemon, "docs/note.txt"),
            inspected_executable(&daemon, "docs/note.txt"),
        ),
        Err(ManagedTextFileError::TooLarge { .. })
    ));
    fs::write(managed.join("docs/note.txt"), [0xff, 0xfe]).expect("binary replacement");
    assert!(matches!(
        daemon.read_managed_text_file("docs/note.txt"),
        Err(ManagedTextFileError::NotUtf8)
    ));
    fs::write(managed.join("docs/note.txt"), &original).expect("restore text");
    fs::remove_file(managed.join("docs/note.txt")).expect("remove file");
    symlink("../../outside", managed.join("docs/note.txt")).expect("symlink mutation");
    assert!(matches!(
        daemon.read_managed_text_file("docs/note.txt"),
        Err(ManagedTextFileError::NotRegularFile)
    ));

    assert_eq!(
        fs::read(private_storage(&managed).join("records.mesh")).unwrap(),
        journal
    );
    assert!(daemon
        .checkpoint_snapshot()
        .expect("checkpoint")
        .open_window()
        .is_none());
    let _ = fs::remove_dir_all(parent);
}

#[test]
fn a_newer_external_edit_during_settling_is_not_reported_as_stable_or_meaningful() {
    let (parent, _source, managed) = imported("external-race");
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");
    let path = managed.join("docs/note.txt");
    let ready = Arc::new(Barrier::new(2));
    let racer_ready = Arc::clone(&ready);
    let racer = std::thread::spawn(move || {
        racer_ready.wait();
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        loop {
            if fs::read(&path).is_ok_and(|bytes| bytes == b"mesh edit\n") {
                fs::write(path, "external\n").expect("external editor");
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the managed replacement was never observable"
            );
            std::thread::yield_now();
        }
    });
    ready.wait();
    let saved = daemon
        .preserve_managed_text_edit(
            "docs/note.txt",
            "mesh edit\n",
            inspected_digest(&daemon, "docs/note.txt"),
            inspected_executable(&daemon, "docs/note.txt"),
        )
        .expect("recovery is still preserved");
    racer.join().expect("external editor joined");
    assert!(!saved.stable_after_idle());
    assert_eq!(
        fs::read(managed.join("docs/note.txt")).unwrap(),
        b"external\n"
    );
    let snapshot = daemon.checkpoint_snapshot().expect("checkpoint");
    assert!(snapshot.last_meaningful().is_none());
    assert!(snapshot
        .latest_recovery()
        .expect("preserved Mesh edit")
        .bytes()
        .ends_with(b"mesh edit\n"));
    let _ = fs::remove_dir_all(parent);
}

#[test]
fn retained_binary_version_restores_undoes_and_survives_restart_without_rewriting_history() {
    let (parent, _source, managed) = imported("restore-undo");
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");
    let current_bytes = b"second\0binary\xff";
    let (earlier, current) = append_second_version(&daemon, &managed, current_bytes);
    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    let meaningful_before_restore = loop {
        if let Some(checkpoint) = daemon
            .checkpoint_snapshot()
            .expect("checkpoint before restore")
            .last_meaningful()
        {
            break checkpoint.through();
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the public durable save never reached its automatic meaningful boundary"
        );
        std::thread::sleep(Duration::from_millis(2));
    };
    let history = OpenWorkspace::open(&managed).expect("complete history");
    let object = history.file_histories()[0].object().to_string();
    let journal =
        fs::read(private_storage(&managed).join("records.mesh")).expect("journal before restore");
    let before_restore = inspected_digest(&daemon, "docs/note.txt");

    let restored = daemon
        .restore_managed_file_version(
            &object,
            &earlier,
            before_restore,
            inspected_executable(&daemon, "docs/note.txt"),
        )
        .expect("restore earlier bytes");
    assert_eq!(restored.path(), "docs/note.txt");
    assert_eq!(restored.target_version(), earlier);
    assert_eq!(
        restored.content_digest(),
        inspected_digest(&daemon, "docs/note.txt")
    );
    assert!(restored.stable_after_idle());
    assert_eq!(fs::read(managed.join("docs/note.txt")).unwrap(), b"first\n");
    assert_eq!(
        fs::read(private_storage(&managed).join("records.mesh")).unwrap(),
        journal
    );
    drop(daemon);

    let restarted = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    restarted.open_at_start(&managed).expect("restart");
    let snapshot = restarted
        .checkpoint_snapshot()
        .expect("restored checkpoint");
    assert!(snapshot.open_window().is_some());
    assert!(snapshot.latest_recovery().is_some());
    assert_eq!(
        snapshot
            .last_meaningful()
            .expect("restore retains the prior meaningful checkpoint")
            .through(),
        meaningful_before_restore,
        "recovery-only restore must not relabel itself as meaningful"
    );

    let summary = restarted.workspace_state().expect("restarted state");
    let generic_current = restarted
        .preview_file_restore_for_workspace(
            &summary.root,
            &summary.digest,
            &summary.installation,
            &object,
            &current,
        )
        .expect_err("generic append-only preview must refuse the logical current version");
    assert_eq!(generic_current.code, "restore-preview-refused");
    assert!(generic_current.message.contains("is already visible"));
    let preview = restarted
        .preview_managed_working_copy_restore_for_workspace(
            &summary.root,
            &summary.digest,
            &summary.installation,
            &object,
            &current,
        )
        .expect("formerly current version is a valid working-copy target")
        .encode();
    assert!(preview.contains("\"schema\":\"mesh.managed-working-copy-restore-preview/v1\""));
    assert!(preview.contains(&format!("\"object_id\":\"{object}\"")));
    assert!(preview.contains(&format!("\"version_id\":\"{current}\"")));
    assert!(preview.contains("\"path\":\"docs/note.txt\""));
    assert!(preview.contains("\"modified_from_current_version\":true"));
    assert!(preview.contains("\"history_unchanged\":true"));
    assert!(!preview.contains("\"operations\""));
    assert!(preview.contains("\"execution_authorized\":false"));

    let undone = restarted
        .restore_managed_file_version(
            &object,
            &current,
            inspected_digest(&restarted, "docs/note.txt"),
            inspected_executable(&restarted, "docs/note.txt"),
        )
        .expect("undo to formerly current version");
    assert_eq!(undone.target_version(), current);
    assert!(undone.stable_after_idle());
    assert_eq!(
        fs::read(managed.join("docs/note.txt")).unwrap(),
        current_bytes
    );
    assert_eq!(
        fs::read(private_storage(&managed).join("records.mesh")).unwrap(),
        journal
    );
    let final_history = OpenWorkspace::open(&managed).expect("history after undo");
    assert_eq!(final_history.file_histories()[0].retained().len(), 2);
    let _ = fs::remove_dir_all(parent);
}

#[test]
fn current_working_copy_preview_refuses_noop_and_never_invents_an_undo_for_unknown_bytes() {
    let (parent, _source, managed) = imported("current-restore-preview");
    let daemon =
        LiveDaemon::with_checkpoint_runtime(startup(), refusal_parameters()).expect("config");
    let summary = daemon.open_at_start(&managed).expect("open");
    let history = OpenWorkspace::open(&managed).expect("history");
    let file = &history.file_histories()[0];
    let object = file.object().to_string();
    let current = file
        .current()
        .expect("current version")
        .version()
        .to_string();

    let unchanged = daemon
        .preview_managed_working_copy_restore_for_workspace(
            &summary.root,
            &summary.digest,
            &summary.installation,
            &object,
            &current,
        )
        .expect_err("an already-equal working copy is not a restore");
    assert!(matches!(unchanged, ManagedTextFileError::Unchanged));

    fs::write(managed.join("docs/note.txt"), b"unknown\0working\xffbytes")
        .expect("plant unretained binary bytes");
    let preview = daemon
        .preview_managed_working_copy_restore_for_workspace(
            &summary.root,
            &summary.digest,
            &summary.installation,
            &object,
            &current,
        )
        .expect("current retained bytes are a valid recovery target")
        .encode();
    assert!(preview.contains("\"modified_from_current_version\":true"));
    assert!(preview.contains("\"undo_target\":null"));
    assert!(!preview.contains("\"operations\""));
    let _ = fs::remove_dir_all(parent);
}

#[test]
fn working_copy_preview_streams_multichunk_target_and_undo_and_rejects_corrupt_content() {
    let earlier_bytes: Vec<u8> = (0..128 * 1024)
        .map(|index| ((index * 17 + index / 97) % 251) as u8)
        .collect();
    let current_bytes: Vec<u8> = (0..128 * 1024)
        .map(|index| ((index * 29 + index / 53 + 11) % 251) as u8)
        .collect();
    let (parent, _source, managed) =
        imported_with_bytes("streamed-restore-preview", &earlier_bytes);
    let daemon =
        LiveDaemon::with_checkpoint_runtime(startup(), refusal_parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");
    let (earlier, current) = append_second_version(&daemon, &managed, &current_bytes);
    let history = OpenWorkspace::open(&managed).expect("multi-chunk history");
    let file = &history.file_histories()[0];
    let earlier_identity = file
        .retained()
        .iter()
        .find(|identity| identity.version().to_string() == earlier)
        .expect("earlier retained version");
    let current_identity = file
        .retained()
        .iter()
        .find(|identity| identity.version().to_string() == current)
        .expect("current retained version");
    let earlier_manifest = history
        .manifest_record(earlier_identity.manifest())
        .expect("earlier manifest");
    let current_manifest = history
        .manifest_record(current_identity.manifest())
        .expect("current manifest");
    assert!(
        earlier_manifest.chunks.len() > 1,
        "target fixture must be multi-chunk"
    );
    assert!(
        current_manifest.chunks.len() > 1,
        "undo fixture must be multi-chunk"
    );

    let summary = daemon.workspace_state().expect("workspace binding");
    let preview = daemon
        .preview_managed_working_copy_restore_for_workspace(
            &summary.root,
            &summary.digest,
            &summary.installation,
            &file.object().to_string(),
            &earlier,
        )
        .expect("streamed target and undo verification")
        .encode();
    assert!(preview.contains(&format!("\"version_id\":\"{earlier}\"")));
    assert!(preview.contains(&format!("\"undo_target\":{{\"version_id\":\"{current}\"")));

    let undo_only_chunk = current_manifest
        .chunks
        .iter()
        .find(|current_chunk| {
            !earlier_manifest
                .chunks
                .iter()
                .any(|earlier_chunk| earlier_chunk.digest == current_chunk.digest)
        })
        .expect("the different retained file has an undo-only chunk");
    let cas = Cas::open(private_storage(&managed)).expect("CAS");
    fs::remove_file(
        cas.layout()
            .chunk_path(&CasDigest::from_bytes(*undo_only_chunk.digest.as_bytes())),
    )
    .expect("remove one undo-only chunk");
    let preview_without_undo = daemon
        .preview_managed_working_copy_restore_for_workspace(
            &summary.root,
            &summary.digest,
            &summary.installation,
            &file.object().to_string(),
            &earlier,
        )
        .expect("missing undo bytes do not invalidate the verified target")
        .encode();
    assert!(preview_without_undo.contains("\"undo_target\":null"));

    let corrupt_target = earlier_manifest.chunks[0].digest;
    fs::write(
        cas.layout()
            .chunk_path(&CasDigest::from_bytes(*corrupt_target.as_bytes())),
        b"corrupt retained chunk",
    )
    .expect("corrupt target chunk");
    let error = daemon
        .preview_managed_working_copy_restore_for_workspace(
            &summary.root,
            &summary.digest,
            &summary.installation,
            &file.object().to_string(),
            &earlier,
        )
        .expect_err("corrupt target content cannot support an exact-byte preview");
    assert!(matches!(error, ManagedTextFileError::RetainedContent(_)));
    let _ = fs::remove_dir_all(parent);
}

#[test]
fn working_copy_preview_verifier_never_reserves_the_retained_manifest_length() {
    let source = include_str!("../src/live.rs");
    let helper_start = source
        .find("fn verify_retained_manifest")
        .expect("streaming verifier");
    let helper_end = source[helper_start..]
        .find("fn workspace_version_refusal")
        .map(|offset| helper_start + offset)
        .expect("next helper boundary");
    let preview_start = source
        .find("pub fn preview_managed_working_copy_restore_for_workspace")
        .expect("working-copy preview");
    let preview_end = source[preview_start..]
        .find("/// The feed")
        .map(|offset| preview_start + offset)
        .expect("preview boundary");
    let implementation = format!(
        "{}{}",
        &source[helper_start..helper_end],
        &source[preview_start..preview_end],
    );
    assert!(implementation.contains("Blake3::hasher()"));
    assert!(implementation.contains("content.update(&bytes)"));
    assert!(implementation.contains("verify_retained_manifest(&cas, target_manifest)?"));
    assert!(!implementation.contains("Vec::with_capacity"));
    assert!(!implementation.contains("extend_from_slice"));
}

#[test]
fn missing_retained_content_refuses_before_the_working_copy_or_recovery_moves() {
    let (parent, _source, managed) = imported("restore-missing-content");
    let daemon =
        LiveDaemon::with_checkpoint_runtime(startup(), refusal_parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");
    let current_bytes = b"new durable bytes";
    let (earlier, _current) = append_second_version(&daemon, &managed, current_bytes);
    let history = OpenWorkspace::open(&managed).expect("complete history");
    let file = &history.file_histories()[0];
    let target = file
        .retained()
        .iter()
        .find(|version| version.version().to_string() == earlier)
        .expect("earlier version");
    let manifest = history
        .manifest_record(target.manifest())
        .expect("manifest");
    let missing = manifest.chunks[0].digest;
    let cas = Cas::open(private_storage(&managed)).expect("CAS");
    fs::remove_file(
        cas.layout()
            .chunk_path(&CasDigest::from_bytes(*missing.as_bytes())),
    )
    .expect("plant missing retained chunk");

    let journal = fs::read(private_storage(&managed).join("records.mesh")).expect("journal");
    let checkpoint = daemon.checkpoint_snapshot().expect("checkpoint");
    let expected_content = inspected_digest(&daemon, "docs/note.txt");
    let summary = daemon
        .workspace_state()
        .expect("state before missing preview");
    let preview_error = daemon
        .preview_managed_working_copy_restore_for_workspace(
            &summary.root,
            &summary.digest,
            &summary.installation,
            &file.object().to_string(),
            &earlier,
        )
        .expect_err("preview verifies retained CAS before making an exact-byte claim");
    assert!(matches!(
        preview_error,
        ManagedTextFileError::RetainedContent(_)
    ));
    let error = daemon
        .restore_managed_file_version(
            &file.object().to_string(),
            &earlier,
            expected_content,
            inspected_executable(&daemon, "docs/note.txt"),
        )
        .expect_err("missing content refuses");
    assert!(matches!(error, ManagedTextFileError::RetainedContent(_)));
    assert_eq!(
        fs::read(managed.join("docs/note.txt")).unwrap(),
        current_bytes
    );
    assert_eq!(
        fs::read(private_storage(&managed).join("records.mesh")).unwrap(),
        journal
    );
    assert_eq!(daemon.checkpoint_snapshot().unwrap(), checkpoint);
    let _ = fs::remove_dir_all(parent);
}

#[test]
fn restore_refuses_when_working_copy_changed_after_inspection() {
    let (parent, _source, managed) = imported("restore-stale-inspection");
    let daemon =
        LiveDaemon::with_checkpoint_runtime(startup(), refusal_parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");
    let (earlier, _current) = append_second_version(&daemon, &managed, b"second\n");
    let history = OpenWorkspace::open(&managed).expect("complete history");
    let object = history.file_histories()[0].object().to_string();
    let expected_content = inspected_digest(&daemon, "docs/note.txt");
    let checkpoint = daemon.checkpoint_snapshot().expect("checkpoint");
    let journal = fs::read(private_storage(&managed).join("records.mesh")).expect("journal");

    fs::write(managed.join("docs/note.txt"), b"external edit\n").expect("external edit");
    let error = daemon
        .restore_managed_file_version(&object, &earlier, expected_content, false)
        .expect_err("stale inspection must refuse");

    assert!(matches!(error, ManagedTextFileError::StaleInspection));
    assert_eq!(
        fs::read(managed.join("docs/note.txt")).unwrap(),
        b"external edit\n"
    );
    assert_eq!(
        fs::read(private_storage(&managed).join("records.mesh")).unwrap(),
        journal
    );
    assert_eq!(daemon.checkpoint_snapshot().unwrap(), checkpoint);
    let _ = fs::remove_dir_all(parent);
}

#[test]
fn externally_renamed_file_is_explicitly_adopted_without_a_second_os_move() {
    let (parent, source, managed) = imported("adopt-native-move");
    let from = managed.join("docs/note.txt");
    let to = managed.join("docs/renamed.txt");
    let signing = SigningKey::from_bytes(&[0x91; 32]);
    let public = PublicKey::from_bytes(signing.verifying_key().to_bytes());
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");
    let before = OpenWorkspace::open(&managed).expect("history before move");
    let object = before.file_histories()[0].object();
    let version = before.file_histories()[0]
        .current()
        .expect("current version")
        .version()
        .to_string();

    fs::rename(&from, &to).expect("external native rename");
    let missing = daemon.native_missing_files().expect("missing tracked file");
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].path(), "docs/note.txt");
    assert_eq!(missing[0].current_version(), version);
    let destination = daemon
        .inspect_native_untracked_file("docs/renamed.txt")
        .expect("native destination");
    assert_eq!(destination.content_digest(), missing[0].content_digest());
    assert_eq!(destination.executable(), missing[0].executable());

    let changed = daemon
        .adopt_native_file_move_privately(
            "docs/note.txt",
            "docs/renamed.txt",
            missing[0].current_version(),
            destination.content_digest(),
            destination.executable(),
            public,
            |payload| {
                Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                    signing.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .expect("adopt native move");
    assert_eq!(changed.action(), "adopt_move");
    assert_eq!(changed.from_path(), Some("docs/note.txt"));
    assert_eq!(changed.to_path(), Some("docs/renamed.txt"));
    assert!(!from.exists());
    assert_eq!(fs::read(&to).unwrap(), b"first\n");

    drop(daemon);
    let restarted = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("restart");
    restarted.open_at_start(&managed).expect("reopen");
    let after = OpenWorkspace::open(&managed).expect("history after move");
    assert_eq!(after.file_histories().len(), 1);
    assert_eq!(after.file_histories()[0].path(), "docs/renamed.txt");
    assert_eq!(after.file_histories()[0].object(), object);
    assert_eq!(after.retired_entries().len(), 1);
    assert_eq!(after.retired_entries()[0].path(), "docs/note.txt");
    assert_eq!(after.retired_entries()[0].entry_type(), "file");
    assert!(restarted.native_missing_files().unwrap().is_empty());

    // Pull the saved rename back to the original ordinary folder in two reviewed steps: install
    // the new path first, then remove only the exact unchanged former path. Unrelated files stay.
    fs::write(source.join("docs/keep.txt"), b"ordinary-only\n").expect("unrelated file");
    let new_path = restarted
        .preview_managed_file_export("docs/renamed.txt", &source)
        .expect("preview renamed destination");
    restarted
        .export_managed_file(
            new_path.path(),
            &source,
            new_path.target_installation(),
            new_path.target_parent_installation(),
            new_path.target_file_installation(),
            new_path.source_version(),
            new_path.source_content_digest(),
            new_path.source_executable(),
            new_path.target_content_digest(),
            new_path.target_executable(),
        )
        .expect("export renamed path before cleanup");
    let retired = restarted
        .preview_retired_export("docs/note.txt", &source)
        .expect("preview old path cleanup");
    assert!(retired.removable());
    assert_eq!(retired.status(), "unchanged-old-file");
    restarted
        .remove_retired_export(
            retired.path(),
            &source,
            retired.entry_type(),
            retired.source_version(),
            retired.source_content_digest(),
            retired.source_executable(),
            retired.target_installation(),
            retired.target_parent_installation(),
            retired.target_entry_installation(),
            retired.target_content_digest(),
            retired.target_executable(),
        )
        .expect("remove unchanged former path");
    assert!(!source.join("docs/note.txt").exists());
    assert_eq!(
        fs::read(source.join("docs/renamed.txt")).unwrap(),
        b"first\n"
    );
    assert_eq!(
        fs::read(source.join("docs/keep.txt")).unwrap(),
        b"ordinary-only\n"
    );
    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn saved_directory_rename_pulls_back_new_tree_before_nonrecursive_old_tree_cleanup() {
    let (parent, source, managed) = imported("export-directory-rename");
    let signing = SigningKey::from_bytes(&[0x9a; 32]);
    let public = PublicKey::from_bytes(signing.verifying_key().to_bytes());
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");
    daemon
        .move_managed_entry_privately("docs", "archive", public, |payload| {
            Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                signing.sign(payload.as_bytes()).to_bytes(),
            ))
        })
        .expect("save directory rename");

    let reopened = OpenWorkspace::open(&managed).expect("renamed history");
    assert_eq!(
        reopened
            .retired_entries()
            .iter()
            .map(|entry| (entry.path(), entry.entry_type()))
            .collect::<Vec<_>>(),
        vec![("docs", "folder"), ("docs/note.txt", "file")]
    );

    let folder = daemon
        .preview_managed_directory_export("archive", &source)
        .expect("preview renamed folder");
    daemon
        .export_managed_directory(
            folder.path(),
            &source,
            folder.source_directory_installation(),
            folder.target_installation(),
            folder.target_parent_installation(),
            folder.target_directory_installation(),
        )
        .expect("create renamed folder");
    let file = daemon
        .preview_managed_file_export("archive/note.txt", &source)
        .expect("preview renamed file");
    daemon
        .export_managed_file(
            file.path(),
            &source,
            file.target_installation(),
            file.target_parent_installation(),
            file.target_file_installation(),
            file.source_version(),
            file.source_content_digest(),
            file.source_executable(),
            file.target_content_digest(),
            file.target_executable(),
        )
        .expect("create renamed file");

    let stale_file = daemon
        .preview_retired_export("docs/note.txt", &source)
        .expect("preview old file");
    daemon
        .remove_retired_export(
            stale_file.path(),
            &source,
            stale_file.entry_type(),
            stale_file.source_version(),
            stale_file.source_content_digest(),
            stale_file.source_executable(),
            stale_file.target_installation(),
            stale_file.target_parent_installation(),
            stale_file.target_entry_installation(),
            stale_file.target_content_digest(),
            stale_file.target_executable(),
        )
        .expect("remove unchanged old file");
    fs::write(source.join("docs/ordinary-only.txt"), b"keep me\n")
        .expect("unrelated old-folder content");
    let preserved_folder = daemon
        .preview_retired_export("docs", &source)
        .expect("preview nonempty old folder");
    assert!(!preserved_folder.removable());
    assert_eq!(preserved_folder.status(), "nonempty-preserved");
    assert_eq!(
        fs::read(source.join("docs/ordinary-only.txt")).unwrap(),
        b"keep me\n"
    );
    fs::remove_file(source.join("docs/ordinary-only.txt")).expect("make old folder empty");
    let stale_folder = daemon
        .preview_retired_export("docs", &source)
        .expect("preview old empty folder");
    assert!(stale_folder.removable());
    assert_eq!(stale_folder.status(), "empty-old-folder");
    daemon
        .remove_retired_export(
            stale_folder.path(),
            &source,
            stale_folder.entry_type(),
            stale_folder.source_version(),
            stale_folder.source_content_digest(),
            stale_folder.source_executable(),
            stale_folder.target_installation(),
            stale_folder.target_parent_installation(),
            stale_folder.target_entry_installation(),
            stale_folder.target_content_digest(),
            stale_folder.target_executable(),
        )
        .expect("remove exact empty old folder");

    assert!(!source.join("docs").exists());
    assert_eq!(
        fs::read(source.join("archive/note.txt")).unwrap(),
        b"first\n"
    );
    assert_eq!(
        fs::read(managed.join("archive/note.txt")).unwrap(),
        b"first\n"
    );
    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn imported_history_never_authorizes_removing_an_identical_file_from_an_unrelated_folder() {
    let (parent, source, managed) = imported("retired-imported-unrelated-folder");
    let unrelated = parent.join("unrelated");
    fs::create_dir_all(unrelated.join("docs")).expect("unrelated folder");
    fs::write(unrelated.join("docs/note.txt"), b"first\n").expect("unrelated twin");
    let unrelated_empty = parent.join("unrelated-empty");
    fs::create_dir_all(unrelated_empty.join("docs")).expect("unrelated empty folder");

    let signing = SigningKey::from_bytes(&[0x9b; 32]);
    let public = PublicKey::from_bytes(signing.verifying_key().to_bytes());
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");
    let current = OpenWorkspace::open(&managed).expect("current history");
    let version = current.file_histories()[0]
        .current()
        .expect("current version")
        .version()
        .to_string();
    fs::remove_file(managed.join("docs/note.txt")).expect("delete managed file");
    daemon
        .adopt_native_file_deletion_privately("docs/note.txt", &version, public, |payload| {
            Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                signing.sign(payload.as_bytes()).to_bytes(),
            ))
        })
        .expect("save managed deletion");
    daemon
        .delete_managed_entry_privately("docs", None, None, public, |payload| {
            Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                signing.sign(payload.as_bytes()).to_bytes(),
            ))
        })
        .expect("save managed directory deletion");

    let file_preview = daemon
        .preview_retired_export("docs/note.txt", &unrelated)
        .expect("preview unrelated twin");
    assert!(!file_preview.removable());
    assert_eq!(file_preview.status(), "unproven-preserved");
    let exact_origin_preview = daemon
        .preview_retired_export("docs/note.txt", &source)
        .expect("preview exact import origin");
    assert!(exact_origin_preview.removable());
    assert_eq!(exact_origin_preview.status(), "unchanged-old-file");
    fs::remove_dir_all(private_storage(&managed).join("pull-back-receipts"))
        .expect("simulate a legacy workspace without origin provenance");
    let legacy_preview = daemon
        .preview_retired_export("docs/note.txt", &source)
        .expect("preview legacy import origin");
    assert!(!legacy_preview.removable());
    assert_eq!(legacy_preview.status(), "unproven-preserved");
    assert!(matches!(
        daemon.remove_retired_export(
            file_preview.path(),
            &unrelated,
            file_preview.entry_type(),
            file_preview.source_version(),
            file_preview.source_content_digest(),
            file_preview.source_executable(),
            file_preview.target_installation(),
            file_preview.target_parent_installation(),
            file_preview.target_entry_installation(),
            file_preview.target_content_digest(),
            file_preview.target_executable(),
        ),
        Err(ManagedTextFileError::StaleExportTarget)
    ));
    let directory_preview = daemon
        .preview_retired_export("docs", &unrelated_empty)
        .expect("preview unrelated empty directory");
    assert!(!directory_preview.removable());
    assert_eq!(directory_preview.status(), "unproven-preserved");
    assert!(matches!(
        daemon.remove_retired_export(
            directory_preview.path(),
            &unrelated_empty,
            directory_preview.entry_type(),
            directory_preview.source_version(),
            directory_preview.source_content_digest(),
            directory_preview.source_executable(),
            directory_preview.target_installation(),
            directory_preview.target_parent_installation(),
            directory_preview.target_entry_installation(),
            directory_preview.target_content_digest(),
            directory_preview.target_executable(),
        ),
        Err(ManagedTextFileError::StaleExportTarget)
    ));
    assert_eq!(
        fs::read(unrelated.join("docs/note.txt")).unwrap(),
        b"first\n"
    );
    assert!(unrelated_empty.join("docs").is_dir());
    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn externally_deleted_file_is_explicitly_adopted_and_restart_exact() {
    let (parent, source, managed) = imported("adopt-native-delete");
    let path = managed.join("docs/note.txt");
    let signing = SigningKey::from_bytes(&[0x92; 32]);
    let public = PublicKey::from_bytes(signing.verifying_key().to_bytes());
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");
    fs::remove_file(&path).expect("external native delete");
    let missing = daemon.native_missing_files().expect("missing tracked file");
    assert_eq!(missing.len(), 1);

    let changed = daemon
        .adopt_native_file_deletion_privately(
            missing[0].path(),
            missing[0].current_version(),
            public,
            |payload| {
                Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                    signing.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .expect("adopt native deletion");
    assert_eq!(changed.action(), "adopt_delete");
    assert_eq!(changed.from_path(), Some("docs/note.txt"));
    assert!(!path.exists());

    drop(daemon);
    let restarted = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("restart");
    restarted.open_at_start(&managed).expect("reopen");
    assert!(OpenWorkspace::open(&managed)
        .expect("history after delete")
        .file_histories()
        .is_empty());
    assert!(restarted.native_missing_files().unwrap().is_empty());
    let retired = restarted
        .preview_retired_export("docs/note.txt", &source)
        .expect("preview deleted path cleanup");
    assert!(retired.removable());
    fs::write(source.join("docs/note.txt"), b"new ordinary work\n")
        .expect("ordinary folder changes after preview");
    let refusal = restarted.remove_retired_export(
        retired.path(),
        &source,
        retired.entry_type(),
        retired.source_version(),
        retired.source_content_digest(),
        retired.source_executable(),
        retired.target_installation(),
        retired.target_parent_installation(),
        retired.target_entry_installation(),
        retired.target_content_digest(),
        retired.target_executable(),
    );
    assert!(matches!(
        refusal,
        Err(ManagedTextFileError::StaleExportTarget)
    ));
    assert_eq!(
        fs::read(source.join("docs/note.txt")).unwrap(),
        b"new ordinary work\n"
    );
    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn retired_private_only_file_never_authorizes_deleting_an_unmanaged_twin() {
    let (parent, source, managed) = imported("retired-private-only-file");
    let path = managed.join("private-only.txt");
    let bytes = b"private bytes that may independently appear outside Mesh\n";
    fs::write(&path, bytes).expect("agent creates private-only file");
    let signing = SigningKey::from_bytes(&[0xa3; 32]);
    let public = PublicKey::from_bytes(signing.verifying_key().to_bytes());
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");

    let inspected = daemon
        .inspect_native_untracked_file("private-only.txt")
        .expect("inspect private-only file");
    let saved = daemon
        .adopt_native_file_privately(
            "private-only.txt",
            inspected.content_digest(),
            inspected.executable(),
            public,
            |payload| {
                Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                    signing.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .expect("save private-only file");
    fs::remove_file(&path).expect("agent removes private-only file");
    daemon
        .adopt_native_file_deletion_privately(
            "private-only.txt",
            saved.version(),
            public,
            |payload| {
                Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                    signing.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .expect("save private-only deletion");

    assert!(
        !source.join("private-only.txt").exists(),
        "the private file was never pulled back"
    );
    fs::write(source.join("private-only.txt"), bytes).expect("independent unmanaged twin appears");
    let preview = daemon
        .preview_retired_export("private-only.txt", &source)
        .expect("preview retired private-only path");

    assert!(!preview.removable());
    assert_eq!(preview.status(), "unproven-preserved");
    let refusal = daemon.remove_retired_export(
        preview.path(),
        &source,
        preview.entry_type(),
        preview.source_version(),
        preview.source_content_digest(),
        preview.source_executable(),
        preview.target_installation(),
        preview.target_parent_installation(),
        preview.target_entry_installation(),
        preview.target_content_digest(),
        preview.target_executable(),
    );
    assert!(matches!(
        refusal,
        Err(ManagedTextFileError::StaleExportTarget)
    ));
    assert_eq!(fs::read(source.join("private-only.txt")).unwrap(), bytes);
    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn pulled_back_private_file_has_durable_exact_cleanup_provenance_after_restart() {
    let (parent, source, managed) = imported("retired-pulled-back-private-file");
    let path = managed.join("private-only.txt");
    let bytes = b"private bytes explicitly pulled back before retirement\n";
    fs::write(&path, bytes).expect("agent creates private-only file");
    let signing = SigningKey::from_bytes(&[0xa4; 32]);
    let public = PublicKey::from_bytes(signing.verifying_key().to_bytes());
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");

    let inspected = daemon
        .inspect_native_untracked_file("private-only.txt")
        .expect("inspect private-only file");
    let saved = daemon
        .adopt_native_file_privately(
            "private-only.txt",
            inspected.content_digest(),
            inspected.executable(),
            public,
            |payload| {
                Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                    signing.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .expect("save private-only file");
    let pull_back = daemon
        .preview_managed_file_export("private-only.txt", &source)
        .expect("preview explicit Pull-back");
    let installed = daemon
        .export_managed_file(
            pull_back.path(),
            &source,
            pull_back.target_installation(),
            pull_back.target_parent_installation(),
            pull_back.target_file_installation(),
            pull_back.source_version(),
            pull_back.source_content_digest(),
            pull_back.source_executable(),
            pull_back.target_content_digest(),
            pull_back.target_executable(),
        )
        .expect("explicit Pull-back installs exact file");
    assert!(installed.created());
    assert_eq!(fs::read(source.join("private-only.txt")).unwrap(), bytes);

    drop(daemon);
    let restarted = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("restart");
    restarted.open_at_start(&managed).expect("reopen");
    fs::remove_file(&path).expect("agent removes pulled-back private file");
    restarted
        .adopt_native_file_deletion_privately(
            "private-only.txt",
            saved.version(),
            public,
            |payload| {
                Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                    signing.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .expect("save pulled-back private deletion");

    let preview = restarted
        .preview_retired_export("private-only.txt", &source)
        .expect("preview exact pulled-back cleanup after restart");
    assert!(preview.removable());
    assert_eq!(preview.status(), "unchanged-pulled-back-file");
    restarted
        .remove_retired_export(
            preview.path(),
            &source,
            preview.entry_type(),
            preview.source_version(),
            preview.source_content_digest(),
            preview.source_executable(),
            preview.target_installation(),
            preview.target_parent_installation(),
            preview.target_entry_installation(),
            preview.target_content_digest(),
            preview.target_executable(),
        )
        .expect("remove exact file installed by Pull-back");
    assert!(!source.join("private-only.txt").exists());
    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn pull_back_receipt_failure_never_installs_an_unproven_file() {
    let (parent, source, managed) = imported("pull-back-receipt-failure");
    let managed_path = managed.join("private-only.txt");
    let target_path = source.join("private-only.txt");
    fs::write(
        &managed_path,
        b"private bytes awaiting reviewed Pull back\n",
    )
    .expect("agent creates private-only file");
    let signing = SigningKey::from_bytes(&[0xa7; 32]);
    let public = PublicKey::from_bytes(signing.verifying_key().to_bytes());
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");
    let inspected = daemon
        .inspect_native_untracked_file("private-only.txt")
        .expect("inspect private file");
    daemon
        .adopt_native_file_privately(
            "private-only.txt",
            inspected.content_digest(),
            inspected.executable(),
            public,
            |payload| {
                Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                    signing.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .expect("save private file");
    let pull_back = daemon
        .preview_managed_file_export("private-only.txt", &source)
        .expect("preview Pull-back");

    // A regular file at the private receipt-directory name deterministically makes provenance
    // publication fail. Pull back must discover that before its create-only rename becomes live;
    // otherwise the caller receives an error after ordinary-folder bytes already changed and an
    // identical retry has no safe way to infer who created them.
    fs::remove_dir_all(private_storage(&managed).join("pull-back-receipts"))
        .expect("remove existing origin receipt directory");
    fs::write(
        private_storage(&managed).join("pull-back-receipts"),
        b"blocks receipt directory creation",
    )
    .expect("block receipt publication");
    let refusal = daemon.export_managed_file(
        pull_back.path(),
        &source,
        pull_back.target_installation(),
        pull_back.target_parent_installation(),
        pull_back.target_file_installation(),
        pull_back.source_version(),
        pull_back.source_content_digest(),
        pull_back.source_executable(),
        pull_back.target_content_digest(),
        pull_back.target_executable(),
    );

    assert!(matches!(refusal, Err(ManagedTextFileError::Io { .. })));
    assert!(
        !target_path.exists(),
        "a failed provenance write installed ordinary-folder bytes without cleanup authority"
    );
    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn pull_back_receipt_never_transfers_to_a_recreated_identical_unmanaged_file() {
    let (parent, source, managed) = imported("recreated-pull-back-target");
    let managed_path = managed.join("private-only.txt");
    let target_path = source.join("private-only.txt");
    let bytes = b"exact bytes do not transfer destination identity\n";
    fs::write(&managed_path, bytes).expect("agent creates private-only file");
    let signing = SigningKey::from_bytes(&[0xa5; 32]);
    let public = PublicKey::from_bytes(signing.verifying_key().to_bytes());
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");
    let inspected = daemon
        .inspect_native_untracked_file("private-only.txt")
        .expect("inspect private file");
    let saved = daemon
        .adopt_native_file_privately(
            "private-only.txt",
            inspected.content_digest(),
            inspected.executable(),
            public,
            |payload| {
                Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                    signing.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .expect("save private file");
    let pull_back = daemon
        .preview_managed_file_export("private-only.txt", &source)
        .expect("preview Pull-back");
    daemon
        .export_managed_file(
            pull_back.path(),
            &source,
            pull_back.target_installation(),
            pull_back.target_parent_installation(),
            pull_back.target_file_installation(),
            pull_back.source_version(),
            pull_back.source_content_digest(),
            pull_back.source_executable(),
            pull_back.target_content_digest(),
            pull_back.target_executable(),
        )
        .expect("Pull back file");

    fs::remove_file(&target_path).expect("remove exact installed target outside Mesh");
    fs::write(&target_path, bytes).expect("create independent byte-identical replacement");
    fs::remove_file(&managed_path).expect("retire private file");
    daemon
        .adopt_native_file_deletion_privately(
            "private-only.txt",
            saved.version(),
            public,
            |payload| {
                Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                    signing.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .expect("save private deletion");

    let preview = daemon
        .preview_retired_export("private-only.txt", &source)
        .expect("preview recreated target");
    assert!(!preview.removable());
    assert_eq!(preview.status(), "unproven-preserved");
    assert_eq!(fs::read(&target_path).unwrap(), bytes);
    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn created_pull_back_folder_has_exact_cleanup_provenance_after_private_rename() {
    let (parent, source, managed) = imported("retired-pulled-back-private-folder");
    fs::create_dir(managed.join("private-folder")).expect("agent creates private folder");
    let signing = SigningKey::from_bytes(&[0xa6; 32]);
    let public = PublicKey::from_bytes(signing.verifying_key().to_bytes());
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");
    let folders = daemon
        .native_untracked_directories()
        .expect("discover private folder");
    assert_eq!(folders.len(), 1);
    daemon
        .adopt_native_directory_privately(
            folders[0].path(),
            folders[0].installation(),
            public,
            |payload| {
                Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                    signing.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .expect("save private folder");

    let pull_back = daemon
        .preview_managed_directory_export("private-folder", &source)
        .expect("preview private folder Pull-back");
    let installed = daemon
        .export_managed_directory(
            pull_back.path(),
            &source,
            pull_back.source_directory_installation(),
            pull_back.target_installation(),
            pull_back.target_parent_installation(),
            pull_back.target_directory_installation(),
        )
        .expect("create private folder in ordinary destination");
    assert!(installed.created());

    daemon
        .move_managed_entry_privately("private-folder", "renamed-folder", public, |payload| {
            Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                signing.sign(payload.as_bytes()).to_bytes(),
            ))
        })
        .expect("save private folder rename");
    drop(daemon);
    let restarted = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("restart");
    restarted.open_at_start(&managed).expect("reopen");

    let preview = restarted
        .preview_retired_export("private-folder", &source)
        .expect("preview pulled-back folder cleanup");
    assert!(preview.removable());
    assert_eq!(preview.status(), "empty-pulled-back-folder");
    restarted
        .remove_retired_export(
            preview.path(),
            &source,
            preview.entry_type(),
            preview.source_version(),
            preview.source_content_digest(),
            preview.source_executable(),
            preview.target_installation(),
            preview.target_parent_installation(),
            preview.target_entry_installation(),
            preview.target_content_digest(),
            preview.target_executable(),
        )
        .expect("remove exact folder created by Pull-back");
    assert!(!source.join("private-folder").exists());
    assert!(managed.join("renamed-folder").is_dir());
    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn externally_renamed_and_edited_file_keeps_identity_but_stays_working() {
    let (parent, _source, managed) = imported("adopt-native-move-edit");
    let signing = SigningKey::from_bytes(&[0x94; 32]);
    let public = PublicKey::from_bytes(signing.verifying_key().to_bytes());
    let daemon = LiveDaemon::with_checkpoint_runtime(startup(), parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");
    let before = OpenWorkspace::open(&managed).expect("history before move");
    let object = before.file_histories()[0].object();
    let version = before.file_histories()[0]
        .current()
        .unwrap()
        .version()
        .to_string();
    fs::rename(
        managed.join("docs/note.txt"),
        managed.join("docs/renamed-and-edited.txt"),
    )
    .expect("external rename");
    fs::write(
        managed.join("docs/renamed-and-edited.txt"),
        b"renamed and edited by agent\n",
    )
    .expect("external edit");
    let destination = daemon
        .inspect_native_untracked_file("docs/renamed-and-edited.txt")
        .expect("destination inspection");

    let changed = daemon
        .adopt_native_file_move_privately(
            "docs/note.txt",
            "docs/renamed-and-edited.txt",
            &version,
            destination.content_digest(),
            destination.executable(),
            public,
            |payload| {
                Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                    signing.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .expect("adopt rename plus edit");
    assert_eq!(changed.action(), "adopt_move");
    assert!(!changed.meaningful_saved());
    let inspected = daemon
        .inspect_managed_file("docs/renamed-and-edited.txt")
        .expect("moved working file");
    assert!(inspected.modified_from_current_version());
    assert_eq!(
        OpenWorkspace::open(&managed).unwrap().file_histories()[0].object(),
        object
    );
    fs::remove_dir_all(parent).expect("cleanup");
}

#[test]
fn native_structural_adoption_rechecks_after_signing_before_append() {
    let (parent, _source, managed) = imported("adopt-native-races");
    let original = managed.join("docs/note.txt");
    let moved = managed.join("docs/moved.txt");
    let signing = SigningKey::from_bytes(&[0x93; 32]);
    let public = PublicKey::from_bytes(signing.verifying_key().to_bytes());
    let daemon =
        LiveDaemon::with_checkpoint_runtime(startup(), refusal_parameters()).expect("config");
    daemon.open_at_start(&managed).expect("open");
    let version = OpenWorkspace::open(&managed).unwrap().file_histories()[0]
        .current()
        .unwrap()
        .version()
        .to_string();
    let journal = private_storage(&managed).join("records.mesh");

    fs::rename(&original, &moved).expect("external move");
    let inspected = daemon
        .inspect_native_untracked_file("docs/moved.txt")
        .expect("destination inspection");
    let before_move = fs::read(&journal).expect("journal before move race");
    let move_error = daemon
        .adopt_native_file_move_privately(
            "docs/note.txt",
            "docs/moved.txt",
            &version,
            inspected.content_digest(),
            inspected.executable(),
            public,
            |payload| {
                fs::write(&moved, b"changed while signing\n").expect("race destination");
                Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                    signing.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .expect_err("move race refuses");
    assert!(matches!(move_error, ManagedTextFileError::StaleInspection));
    assert_eq!(fs::read(&journal).unwrap(), before_move);

    fs::remove_file(&moved).expect("prepare missing source");
    let before_delete = fs::read(&journal).expect("journal before delete race");
    let _delete_error = daemon
        .adopt_native_file_deletion_privately("docs/note.txt", &version, public, |payload| {
            fs::write(&original, b"creator won the race\n").expect("race source");
            Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                signing.sign(payload.as_bytes()).to_bytes(),
            ))
        })
        .expect_err("delete race refuses");
    assert_eq!(fs::read(&journal).unwrap(), before_delete);
    assert_eq!(fs::read(&original).unwrap(), b"creator won the race\n");
    fs::remove_dir_all(parent).expect("cleanup");
}
