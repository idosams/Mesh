//! Whole-workspace history opens as a new native folder instead of rewinding the current one.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::time::Duration;

use ed25519_dalek::{Signer as _, SigningKey};
use mesh_daemon::ipc::surface::{nothing_to_recover, Operations, StartupSummary};
use mesh_daemon::{
    CheckpointRuntimeParameters, LiveDaemon, OpenWorkspace, ProtectedWorkspaceRoot,
    WorkspaceVersionForkRequest, RECORD_FILE_NAME,
};
use mesh_types::{PublicKey, Signature};

fn scratch(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "mesh-workspace-version-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ))
}

fn new_daemon() -> LiveDaemon {
    LiveDaemon::with_checkpoint_runtime(
        StartupSummary::from(&nothing_to_recover()),
        CheckpointRuntimeParameters {
            idle_interval: Some(Duration::from_millis(1)),
            maximum_uncheckpointed_bytes: Some(65_536),
            maximum_uncheckpointed_interval: Some(Duration::from_secs(60)),
        },
    )
    .expect("checkpoint configuration")
}

#[test]
fn an_imported_executable_stays_executable_in_a_loaded_workspace_version() {
    let root = scratch("fork-executable");
    let source = root.join("source");
    let private = root.join("current.mesh");
    let fork_private = root.join("saved.mesh");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&source).expect("source");
    let script = source.join("run.sh");
    fs::write(&script, b"#!/bin/sh\nprintf 'native mesh\\n'\n").expect("script");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("source executable");

    let daemon = new_daemon();
    let preview = daemon
        .preview_folder_import(&source.to_string_lossy())
        .expect("preview");
    let summary = preview
        .get("summary")
        .and_then(mesh_daemon::ipc::Json::as_text)
        .expect("summary");
    let imported = daemon
        .confirm_folder_import(
            &source.to_string_lossy(),
            &private.to_string_lossy(),
            summary,
        )
        .expect("import");
    let working = PathBuf::from(
        imported
            .get("destination")
            .and_then(mesh_daemon::ipc::Json::as_text)
            .expect("working folder"),
    );
    assert_ne!(
        fs::metadata(working.join("run.sh"))
            .expect("working script")
            .permissions()
            .mode()
            & 0o111,
        0
    );

    let state = daemon.workspace_state().expect("state");
    let saved = state.workspace_versions[0].operation().to_string();
    fs::set_permissions(working.join("run.sh"), fs::Permissions::from_mode(0o644))
        .expect("unsaved native mode edit");
    let forked = daemon
        .fork_workspace_version(
            &saved,
            &fork_private.to_string_lossy(),
            &state.root,
            &state.digest,
            &state.installation,
        )
        .expect("load saved version");
    let fork = PathBuf::from(
        forked
            .get("destination")
            .and_then(mesh_daemon::ipc::Json::as_text)
            .expect("loaded working folder"),
    );
    assert_ne!(
        fs::metadata(fork.join("run.sh"))
            .expect("loaded script")
            .permissions()
            .mode()
            & 0o111,
        0,
        "the durable saved version forgot its executable bit"
    );
    let output = std::process::Command::new(fork.join("run.sh"))
        .output()
        .expect("run loaded script natively");
    assert!(output.status.success());
    assert_eq!(output.stdout, b"native mesh\n");
    assert_ne!(
        fs::metadata(&script)
            .expect("original script")
            .permissions()
            .mode()
            & 0o111,
        0,
        "loading a Mesh version changed the unmanaged original"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn an_earlier_workspace_opens_as_an_independent_native_folder() {
    let root = scratch("fork");
    let source = root.join("source");
    let private = root.join("current.mesh");
    let fork_private = root.join("earlier.mesh");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&source).expect("source");
    fs::write(source.join("original.txt"), b"first version\n").expect("source file");

    let daemon = new_daemon();
    let preview = daemon
        .preview_folder_import(&source.to_string_lossy())
        .expect("preview");
    let summary = preview
        .get("summary")
        .and_then(mesh_daemon::ipc::Json::as_text)
        .expect("summary");
    let imported = daemon
        .confirm_folder_import(
            &source.to_string_lossy(),
            &private.to_string_lossy(),
            summary,
        )
        .expect("import");
    let current = PathBuf::from(
        imported
            .get("destination")
            .and_then(mesh_daemon::ipc::Json::as_text)
            .expect("working folder"),
    );
    let first = daemon.workspace_state().expect("state").workspace_versions[0]
        .operation()
        .to_string();

    let signing = SigningKey::from_bytes(&[0x63; 32]);
    let public = PublicKey::from_bytes(signing.verifying_key().to_bytes());
    daemon
        .create_managed_text_file("later.txt", "second version\n", public, |payload| {
            Ok::<_, core::convert::Infallible>(Signature::from_bytes(
                signing.sign(payload.as_bytes()).to_bytes(),
            ))
        })
        .expect("second durable version");
    assert!(current.join("later.txt").is_file());
    assert_eq!(
        daemon
            .workspace_state()
            .expect("new state")
            .workspace_versions
            .len(),
        2
    );
    let current_state = daemon
        .workspace_state()
        .expect("displayed current workspace");
    let previewed = daemon
        .preview_workspace_version_for_workspace(
            &current_state.root,
            &current_state.digest,
            &current_state.installation,
            &first,
        )
        .expect("preview historical version");
    assert_eq!(
        previewed
            .get("action")
            .and_then(mesh_daemon::ipc::Json::as_text),
        Some("workspace-version-preview")
    );
    assert_eq!(
        previewed
            .get("source_version")
            .and_then(mesh_daemon::ipc::Json::as_text),
        Some(first.as_str())
    );
    assert_eq!(
        previewed
            .get("files")
            .and_then(mesh_daemon::ipc::Json::as_u64),
        Some(1)
    );
    assert_eq!(
        previewed
            .get("folders")
            .and_then(mesh_daemon::ipc::Json::as_u64),
        Some(0)
    );
    assert_eq!(
        previewed
            .get("total_bytes")
            .and_then(mesh_daemon::ipc::Json::as_text),
        Some("14")
    );
    assert!(matches!(
        previewed.get("basis_ordinal"),
        Some(mesh_daemon::ipc::Json::Null)
    ));
    assert_eq!(
        previewed
            .get("change_basis")
            .and_then(mesh_daemon::ipc::Json::as_text),
        Some("initial")
    );
    let first_changes = previewed
        .get("changes")
        .and_then(mesh_daemon::ipc::Json::as_array)
        .expect("initial content changes");
    assert_eq!(first_changes.len(), 1);
    assert_eq!(
        first_changes[0]
            .get("path")
            .and_then(mesh_daemon::ipc::Json::as_text),
        Some("original.txt")
    );
    assert_eq!(
        first_changes[0]
            .get("effect")
            .and_then(mesh_daemon::ipc::Json::as_text),
        Some("added")
    );
    let preview_json = previewed.encode();
    assert!(preview_json.contains("original.txt"));
    assert!(!preview_json.contains("later.txt"));
    assert!(!preview_json.contains("first version"));
    assert!(!fork_private.exists(), "preview created a workspace folder");
    let second = current_state.workspace_versions[1].operation().to_string();
    let current_preview = daemon
        .preview_workspace_version_for_workspace(
            &current_state.root,
            &current_state.digest,
            &current_state.installation,
            &second,
        )
        .expect("preview current version");
    assert_eq!(
        current_preview
            .get("basis_ordinal")
            .and_then(mesh_daemon::ipc::Json::as_u64),
        Some(1)
    );
    assert_eq!(
        current_preview
            .get("change_basis")
            .and_then(mesh_daemon::ipc::Json::as_text),
        Some("previous-point")
    );
    let current_changes = current_preview
        .get("changes")
        .and_then(mesh_daemon::ipc::Json::as_array)
        .expect("current changes");
    assert_eq!(current_changes.len(), 1);
    assert_eq!(
        current_changes[0]
            .get("path")
            .and_then(mesh_daemon::ipc::Json::as_text),
        Some("later.txt")
    );
    assert_eq!(
        current_changes[0]
            .get("effect")
            .and_then(mesh_daemon::ipc::Json::as_text),
        Some("added")
    );
    let verified_current = daemon
        .verified_managed_workspace_path(
            &current_state.root,
            &current_state.digest,
            &current_state.installation,
        )
        .expect("verify presented workspace");
    assert!(verified_current.is_presented());
    assert_eq!(
        verified_current.path(),
        fs::canonicalize(&current).expect("canonical current workspace")
    );

    let forked = daemon
        .fork_workspace_version(
            &first,
            &fork_private.to_string_lossy(),
            &current_state.root,
            &current_state.digest,
            &current_state.installation,
        )
        .expect("historical fork");
    assert_eq!(
        forked
            .get("source_ordinal")
            .and_then(mesh_daemon::ipc::Json::as_u64),
        Some(1),
        "the checkout response must bind its readable label to durable causal order"
    );
    let fork = PathBuf::from(
        forked
            .get("destination")
            .and_then(mesh_daemon::ipc::Json::as_text)
            .expect("fork working folder"),
    );
    assert_eq!(fork, fork_private.join("mounts"));
    assert_eq!(
        fs::read(fork.join("original.txt")).unwrap(),
        b"first version\n"
    );
    assert!(!fork.join("later.txt").exists());
    assert!(
        current.join("later.txt").is_file(),
        "the current workspace was rewound"
    );
    assert!(fork_private.join(RECORD_FILE_NAME).is_file());
    assert!(!fork.join(RECORD_FILE_NAME).exists());

    fs::write(fork.join("original.txt"), b"agent changed the fork\n").expect("native edit");
    let inspected = daemon
        .inspect_managed_file("original.txt")
        .expect("fork inspection");
    assert!(inspected.modified_from_current_version());
    assert_eq!(
        fs::read(current.join("original.txt")).unwrap(),
        b"first version\n",
        "editing the fork changed the source workspace"
    );

    let restarted = new_daemon();
    restarted.open_at_start(&fork).expect("reopen fork");
    assert_eq!(
        restarted
            .workspace_state()
            .expect("restarted state")
            .entries
            .iter()
            .map(mesh_daemon::WorkspaceEntry::path)
            .collect::<Vec<_>>(),
        vec!["original.txt"]
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn a_fork_inherits_only_exact_original_object_authority() {
    let root = scratch("fork-origin-authority");
    let source = root.join("source");
    let private = root.join("current.mesh");
    let fork_private = root.join("earlier.mesh");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&source).expect("source");
    fs::write(source.join("original.txt"), b"first version\n").expect("source file");

    let daemon = new_daemon();
    let preview = daemon
        .preview_folder_import(&source.to_string_lossy())
        .expect("preview");
    let summary = preview
        .get("summary")
        .and_then(mesh_daemon::ipc::Json::as_text)
        .expect("summary");
    daemon
        .confirm_folder_import(
            &source.to_string_lossy(),
            &private.to_string_lossy(),
            summary,
        )
        .expect("import");
    let signing = SigningKey::from_bytes(&[0x64; 32]);
    let public = PublicKey::from_bytes(signing.verifying_key().to_bytes());
    daemon
        .create_managed_text_file("private-only.txt", "private bytes\n", public, |payload| {
            Ok::<_, core::convert::Infallible>(Signature::from_bytes(
                signing.sign(payload.as_bytes()).to_bytes(),
            ))
        })
        .expect("save private-only file");
    fs::write(source.join("private-only.txt"), b"private bytes\n")
        .expect("create unrelated identical ordinary file");
    let state = daemon.workspace_state().expect("state");
    let selected = state
        .workspace_versions
        .last()
        .expect("latest saved version")
        .operation()
        .to_string();
    let forked = daemon
        .fork_workspace_version_with_origin(
            &selected,
            &fork_private.to_string_lossy(),
            &state.root,
            &state.digest,
            &state.installation,
            Some(&source),
        )
        .expect("historical fork");
    let fork = PathBuf::from(
        forked
            .get("destination")
            .and_then(mesh_daemon::ipc::Json::as_text)
            .expect("fork working folder"),
    );
    let opened = OpenWorkspace::open(&fork).expect("fork history");
    let versions = opened
        .file_histories()
        .iter()
        .map(|history| {
            (
                history.path().to_owned(),
                history
                    .current()
                    .expect("current version")
                    .version()
                    .to_string(),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    for path in ["original.txt", "private-only.txt"] {
        fs::remove_file(fork.join(path)).expect("delete from fork");
        daemon
            .adopt_native_file_deletion_privately(path, &versions[path], public, |payload| {
                Ok::<_, core::convert::Infallible>(Signature::from_bytes(
                    signing.sign(payload.as_bytes()).to_bytes(),
                ))
            })
            .expect("save fork deletion");
    }

    let original_cleanup = daemon
        .preview_retired_export("original.txt", &source)
        .expect("preview original cleanup");
    assert!(original_cleanup.removable());
    assert_eq!(original_cleanup.status(), "unchanged-old-file");
    daemon
        .remove_retired_export(
            original_cleanup.path(),
            &source,
            original_cleanup.entry_type(),
            original_cleanup.source_version(),
            original_cleanup.source_content_digest(),
            original_cleanup.source_executable(),
            original_cleanup.target_installation(),
            original_cleanup.target_parent_installation(),
            original_cleanup.target_entry_installation(),
            original_cleanup.target_content_digest(),
            original_cleanup.target_executable(),
        )
        .expect("remove exact imported original after working on the fork");
    let private_cleanup = daemon
        .preview_retired_export("private-only.txt", &source)
        .expect("preview private-only cleanup");
    assert!(!private_cleanup.removable());
    assert_eq!(private_cleanup.status(), "unproven-preserved");
    assert!(!source.join("original.txt").exists());
    assert_eq!(
        fs::read(source.join("private-only.txt")).expect("unrelated file retained"),
        b"private bytes\n"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn a_legacy_or_in_folder_private_layout_is_not_an_isolated_native_folder() {
    let root = scratch("legacy-native-folder");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("workspace");
    let daemon = new_daemon();
    let state = daemon
        .open_workspace(&root.to_string_lossy())
        .expect("open namespaced workspace");
    let verified = daemon
        .verified_managed_workspace_path(&state.root, &state.digest, &state.installation)
        .expect("verify workspace");
    assert_eq!(verified.path(), fs::canonicalize(&root).unwrap());
    assert!(!verified.is_presented());
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn a_verified_native_path_detects_same_path_directory_replacement() {
    let root = scratch("replaced-native-folder");
    let private = root.join("workspace.mesh");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("scratch root");
    let presented = mesh_daemon::workspace::OpenWorkspace::open_presented(&private)
        .expect("create presented workspace")
        .root()
        .as_path()
        .to_path_buf();
    let daemon = new_daemon();
    let state = daemon
        .open_workspace(&presented.to_string_lossy())
        .expect("open presented workspace");
    let verified = daemon
        .verified_managed_workspace_path(&state.root, &state.digest, &state.installation)
        .expect("verify presented workspace");
    verified
        .ensure_current()
        .expect("original directory identity");

    let displaced = private.join("displaced-mounts");
    fs::rename(&presented, &displaced).expect("displace verified directory");
    fs::create_dir(&presented).expect("same-path replacement");
    fs::write(
        presented.join("foreign.txt"),
        b"not the verified workspace\n",
    )
    .expect("replacement content");

    let error = verified
        .ensure_current()
        .expect_err("replacement must not retain navigation authority");
    assert!(error.to_string().contains("was replaced"));
    assert_eq!(
        fs::read(presented.join("foreign.txt")).unwrap(),
        b"not the verified workspace\n"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn an_unknown_version_creates_no_destination() {
    let root = scratch("unknown");
    let source = root.join("source");
    let private = root.join("current.mesh");
    let refused = root.join("refused.mesh");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&source).expect("source");
    fs::write(source.join("file.txt"), b"bytes\n").expect("source file");
    let daemon = new_daemon();
    let preview = daemon
        .preview_folder_import(&source.to_string_lossy())
        .expect("preview");
    let summary = preview
        .get("summary")
        .and_then(mesh_daemon::ipc::Json::as_text)
        .expect("summary");
    daemon
        .confirm_folder_import(
            &source.to_string_lossy(),
            &private.to_string_lossy(),
            summary,
        )
        .expect("import");
    let current_state = daemon.workspace_state().expect("displayed workspace");

    let preview_failure = daemon
        .preview_workspace_version_for_workspace(
            &current_state.root,
            &current_state.digest,
            &current_state.installation,
            &"ff".repeat(32),
        )
        .expect_err("unknown version preview");
    assert_eq!(preview_failure.code, "workspace-version-history-incomplete");
    assert!(!refused.exists());

    let failure = daemon
        .fork_workspace_version(
            &"ff".repeat(32),
            &refused.to_string_lossy(),
            &current_state.root,
            &current_state.digest,
            &current_state.installation,
        )
        .expect_err("unknown version");
    assert_eq!(failure.code, "workspace-version-history-incomplete");
    assert!(!refused.exists());
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn a_workspace_version_preview_is_bounded_without_hiding_the_total() {
    let root = scratch("bounded-preview");
    let source = root.join("source");
    let private = root.join("current.mesh");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&source).expect("source");
    for index in 0..30 {
        fs::create_dir(source.join(format!("folder-{index:02}"))).expect("source folder");
    }

    let daemon = new_daemon();
    let imported = daemon
        .preview_folder_import(&source.to_string_lossy())
        .and_then(|preview| {
            let summary = preview
                .get("summary")
                .and_then(mesh_daemon::ipc::Json::as_text)
                .expect("summary");
            daemon.confirm_folder_import(
                &source.to_string_lossy(),
                &private.to_string_lossy(),
                summary,
            )
        })
        .expect("import");
    let state = daemon.workspace_state().expect("state");
    let operation = state.workspace_versions[0].operation().to_string();
    let preview = daemon
        .preview_workspace_version_for_workspace(
            &state.root,
            &state.digest,
            &state.installation,
            &operation,
        )
        .expect("bounded preview");
    assert_eq!(
        preview
            .get("folders")
            .and_then(mesh_daemon::ipc::Json::as_u64),
        Some(30)
    );
    assert_eq!(
        preview
            .get("entries")
            .and_then(mesh_daemon::ipc::Json::as_array)
            .map(<[mesh_daemon::ipc::Json]>::len),
        Some(24)
    );
    assert_eq!(
        preview
            .get("entries_not_listed")
            .and_then(mesh_daemon::ipc::Json::as_u64),
        Some(6)
    );
    assert_eq!(
        preview
            .get("changes")
            .and_then(mesh_daemon::ipc::Json::as_array)
            .map(<[mesh_daemon::ipc::Json]>::len),
        Some(24)
    );
    assert_eq!(
        preview
            .get("changes_not_listed")
            .and_then(mesh_daemon::ipc::Json::as_u64),
        Some(6)
    );
    assert_eq!(
        preview
            .get("creates_folder")
            .and_then(mesh_daemon::ipc::Json::as_bool),
        Some(false)
    );
    let expected_destination = private.join("mounts").to_string_lossy().into_owned();
    assert_eq!(
        imported
            .get("destination")
            .and_then(mesh_daemon::ipc::Json::as_text),
        Some(expected_destination.as_str())
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn a_version_destination_inside_the_current_working_folder_is_refused_without_mutation() {
    let root = scratch("nested-destination");
    let source = root.join("source");
    let private = root.join("current.mesh");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&source).expect("source");
    fs::write(source.join("original.txt"), b"first version\n").expect("source file");

    let daemon = new_daemon();
    let preview = daemon
        .preview_folder_import(&source.to_string_lossy())
        .expect("preview");
    let summary = preview
        .get("summary")
        .and_then(mesh_daemon::ipc::Json::as_text)
        .expect("summary");
    let imported = daemon
        .confirm_folder_import(
            &source.to_string_lossy(),
            &private.to_string_lossy(),
            summary,
        )
        .expect("import");
    let current = PathBuf::from(
        imported
            .get("destination")
            .and_then(mesh_daemon::ipc::Json::as_text)
            .expect("working folder"),
    );
    let first = daemon.workspace_state().expect("state").workspace_versions[0]
        .operation()
        .to_string();
    let current_state = daemon
        .workspace_state()
        .expect("displayed current workspace");
    let before = fs::read(current.join("original.txt")).expect("current bytes");
    for destination in [
        current.join("nested-version.mesh"),
        private.join("nested-private-store.mesh"),
    ] {
        let failure = daemon
            .fork_workspace_version(
                &first,
                &destination.to_string_lossy(),
                &current_state.root,
                &current_state.digest,
                &current_state.installation,
            )
            .expect_err("a version folder must be independent of the current workspace");
        assert_eq!(
            failure.code,
            "workspace-version-destination-overlaps-current"
        );
        assert!(!destination.exists(), "the refused destination was created");
        assert_eq!(
            fs::read(current.join("original.txt")).expect("current bytes after refusal"),
            before
        );
        assert_eq!(
            daemon.workspace_state().expect("state after refusal").root,
            current.display().to_string(),
            "the refused fork replaced the open workspace"
        );
    }

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn a_version_destination_inside_the_original_project_is_refused_without_mutation() {
    let root = scratch("original-destination");
    let source = root.join("source");
    let private = root.join("current.mesh");
    let destination = source.join("saved-version.mesh");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&source).expect("source");
    fs::write(source.join("original.txt"), b"first version\n").expect("source file");

    let daemon = new_daemon();
    let preview = daemon
        .preview_folder_import(&source.to_string_lossy())
        .expect("preview");
    let summary = preview
        .get("summary")
        .and_then(mesh_daemon::ipc::Json::as_text)
        .expect("summary");
    daemon
        .confirm_folder_import(
            &source.to_string_lossy(),
            &private.to_string_lossy(),
            summary,
        )
        .expect("import");
    let state = daemon.workspace_state().expect("state");
    let saved = state.workspace_versions[0].operation().to_string();
    let original_before = fs::read(source.join("original.txt")).expect("original bytes");

    let failure = daemon
        .fork_workspace_version_with_origin(
            &saved,
            &destination.to_string_lossy(),
            &state.root,
            &state.digest,
            &state.installation,
            Some(&source),
        )
        .expect_err("a saved-version checkout must stay outside the original project");
    assert_eq!(
        failure.code,
        "workspace-version-destination-overlaps-original"
    );
    assert!(!destination.exists(), "the refused destination was created");
    assert_eq!(
        fs::read(source.join("original.txt")).expect("original bytes after refusal"),
        original_before
    );
    assert_eq!(
        daemon.workspace_state().expect("state after refusal").root,
        state.root,
        "the refused fork replaced the open workspace"
    );

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn a_version_destination_inside_an_exact_remembered_workspace_is_refused_before_creation() {
    let root = scratch("remembered-destination");
    let source = root.join("source");
    let private = root.join("current.mesh");
    let agent_workspace = root.join("agent-workspace");
    let destination = agent_workspace.join("saved-version.mesh");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&source).expect("source");
    fs::create_dir_all(&agent_workspace).expect("agent workspace");
    fs::write(source.join("original.txt"), b"first version\n").expect("source file");
    fs::write(agent_workspace.join("agent.txt"), b"agent work\n").expect("agent file");

    let daemon = new_daemon();
    let preview = daemon
        .preview_folder_import(&source.to_string_lossy())
        .expect("preview");
    let summary = preview
        .get("summary")
        .and_then(mesh_daemon::ipc::Json::as_text)
        .expect("summary");
    daemon
        .confirm_folder_import(
            &source.to_string_lossy(),
            &private.to_string_lossy(),
            summary,
        )
        .expect("import");
    let state = daemon.workspace_state().expect("state");
    let saved = state.workspace_versions[0].operation().to_string();
    let protected = ProtectedWorkspaceRoot::inspect(&agent_workspace).expect("protected identity");

    let failure = daemon
        .fork_workspace_version_protected(
            WorkspaceVersionForkRequest::new(
                &saved,
                &destination.to_string_lossy(),
                &state.root,
                &state.digest,
                &state.installation,
                None,
            )
            .protecting(&[protected]),
        )
        .expect_err("a saved version cannot enter a remembered agent workspace");
    assert_eq!(
        failure.code,
        "workspace-version-destination-overlaps-remembered"
    );
    assert!(!destination.exists(), "the refused destination was created");
    assert_eq!(
        fs::read(agent_workspace.join("agent.txt")).expect("agent bytes"),
        b"agent work\n"
    );
    assert_eq!(
        fs::read_dir(&agent_workspace)
            .expect("agent directory")
            .count(),
        1,
        "no temporary export entered the agent workspace"
    );
    assert_eq!(
        daemon.workspace_state().expect("state after refusal").root,
        state.root
    );

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn a_parent_renamed_after_preflight_cannot_redirect_a_version_into_an_agent_workspace() {
    let root = scratch("remembered-parent-swap");
    let source = root.join("source");
    let private = root.join("current.mesh");
    let safe_parent = root.join("safe-parent");
    let displaced_safe_parent = root.join("displaced-safe-parent");
    let agent_workspace = root.join("agent-workspace");
    let destination = safe_parent.join("saved-version.mesh");
    let _ = fs::remove_dir_all(&root);
    for directory in [&source, &safe_parent, &agent_workspace] {
        fs::create_dir_all(directory).expect("fixture directory");
    }
    fs::write(source.join("original.txt"), b"first version\n").expect("source file");
    fs::write(agent_workspace.join("agent.txt"), b"agent work\n").expect("agent file");

    let daemon = new_daemon();
    let preview = daemon
        .preview_folder_import(&source.to_string_lossy())
        .expect("preview");
    let summary = preview
        .get("summary")
        .and_then(mesh_daemon::ipc::Json::as_text)
        .expect("summary");
    daemon
        .confirm_folder_import(
            &source.to_string_lossy(),
            &private.to_string_lossy(),
            summary,
        )
        .expect("import");
    let state = daemon.workspace_state().expect("state");
    let saved = state.workspace_versions[0].operation().to_string();

    // This is the identity the desktop captured while `destination` was still an independent
    // sibling. Swap that exact object into the destination namespace before the daemon call.
    let protected = ProtectedWorkspaceRoot::inspect(&agent_workspace).expect("protected identity");
    fs::rename(&safe_parent, &displaced_safe_parent).expect("move safe parent");
    fs::rename(&agent_workspace, &safe_parent).expect("redirect destination parent");

    let failure = daemon
        .fork_workspace_version_protected(
            WorkspaceVersionForkRequest::new(
                &saved,
                &destination.to_string_lossy(),
                &state.root,
                &state.digest,
                &state.installation,
                None,
            )
            .protecting(&[protected]),
        )
        .expect_err("descriptor identity must win over the renamed path");
    assert_eq!(
        failure.code,
        "workspace-version-destination-overlaps-remembered"
    );
    assert!(!destination.exists(), "the refused destination was created");
    assert_eq!(
        fs::read(safe_parent.join("agent.txt")).expect("agent bytes after rename"),
        b"agent work\n"
    );
    assert_eq!(
        fs::read_dir(&safe_parent)
            .expect("renamed agent directory")
            .count(),
        1,
        "no temporary export entered the renamed agent workspace"
    );
    assert_eq!(
        daemon.workspace_state().expect("state after refusal").root,
        state.root
    );

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn a_fork_request_cannot_cross_into_a_byte_identical_replacement_workspace() {
    let root = scratch("stale-source");
    let source = root.join("source");
    let first_private = root.join("first.mesh");
    let second_private = root.join("second.mesh");
    let refused = root.join("refused.mesh");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&source).expect("source");
    fs::write(source.join("original.txt"), b"same bytes\n").expect("source file");

    let daemon = new_daemon();
    let preview = daemon
        .preview_folder_import(&source.to_string_lossy())
        .expect("first preview");
    let summary = preview
        .get("summary")
        .and_then(mesh_daemon::ipc::Json::as_text)
        .expect("summary");
    daemon
        .confirm_folder_import(
            &source.to_string_lossy(),
            &first_private.to_string_lossy(),
            summary,
        )
        .expect("first import");
    let displayed = daemon.workspace_state().expect("displayed first workspace");
    let operation = displayed.workspace_versions[0].operation().to_string();

    let preview = daemon
        .preview_folder_import(&source.to_string_lossy())
        .expect("second preview");
    let summary = preview
        .get("summary")
        .and_then(mesh_daemon::ipc::Json::as_text)
        .expect("summary");
    daemon
        .confirm_folder_import(
            &source.to_string_lossy(),
            &second_private.to_string_lossy(),
            summary,
        )
        .expect("second import replaces the open workspace");
    let replacement = daemon.workspace_state().expect("replacement workspace");
    assert_eq!(replacement.digest, displayed.digest);
    assert_eq!(
        replacement.workspace_versions[0].operation().to_string(),
        operation
    );
    assert_ne!(replacement.installation, displayed.installation);

    let failure = daemon
        .fork_workspace_version(
            &operation,
            &refused.to_string_lossy(),
            &displayed.root,
            &displayed.digest,
            &displayed.installation,
        )
        .expect_err("the stale displayed workspace cannot authorize a fork of its clone");
    assert_eq!(failure.code, "workspace-version-source-changed");
    assert!(!refused.exists(), "the stale request created a destination");
    let preview_failure = daemon
        .preview_workspace_version_for_workspace(
            &displayed.root,
            &displayed.digest,
            &displayed.installation,
            &operation,
        )
        .expect_err("the stale displayed workspace cannot authorize a preview of its clone");
    assert_eq!(preview_failure.code, "workspace-version-source-changed");
    assert_eq!(
        daemon.workspace_state().expect("state after refusal").root,
        replacement.root
    );

    fs::remove_dir_all(root).expect("cleanup");
}
