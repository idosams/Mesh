//! Real `meshctl` preview, confirmation, restart, rollback and refusal journeys.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use mesh_daemon::{
    ipc::{nothing_to_recover, DaemonMessage, IpcServer, Json, StartupSummary},
    LiveDaemon, OpenWorkspace,
};

fn scratch(name: &str) -> PathBuf {
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    let serial = SERIAL.fetch_add(1, Ordering::SeqCst);
    let root = std::env::temp_dir().join(format!(
        "mesh-folder-import-cli-{name}-{}-{serial}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("scratch directory");
    root
}

fn fixture(root: &Path) -> PathBuf {
    let source = root.join("existing-project");
    fs::create_dir_all(source.join("src/empty")).expect("source directories");
    fs::write(source.join("README.md"), b"hello mesh\n").expect("readme");
    fs::write(source.join("src/main.rs"), b"fn main() {}\n").expect("source file");
    source
}

fn tree_bytes(root: &Path) -> Vec<(PathBuf, Option<Vec<u8>>)> {
    fn visit(root: &Path, relative: &Path, found: &mut Vec<(PathBuf, Option<Vec<u8>>)>) {
        let directory = root.join(relative);
        let mut entries: Vec<_> = fs::read_dir(&directory)
            .expect("read directory")
            .map(|entry| entry.expect("directory entry"))
            .collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let child = relative.join(entry.file_name());
            let metadata = entry.file_type().expect("entry type");
            if metadata.is_dir() {
                found.push((child.clone(), None));
                visit(root, &child, found);
            } else {
                found.push((
                    child.clone(),
                    Some(fs::read(root.join(&child)).expect("file bytes")),
                ));
            }
        }
    }

    let mut found = Vec::new();
    visit(root, Path::new(""), &mut found);
    found
}

fn assert_imported_tree(root: &Path, expected: &[(PathBuf, Option<Vec<u8>>)]) {
    for (relative, bytes) in expected {
        let path = root.join(relative);
        match bytes {
            Some(bytes) => assert_eq!(fs::read(&path).expect("imported file bytes"), *bytes),
            None => assert!(path.is_dir(), "imported directory {}", path.display()),
        }
    }
}

fn meshctl(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_meshctl"))
        .args(arguments)
        .output()
        .expect("meshctl starts")
}

fn success_json(output: &Output) -> Json {
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    Json::parse(std::str::from_utf8(&output.stdout).expect("utf8").trim()).expect("one JSON reply")
}

fn wire_result(output: &Output) -> Json {
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let messages = std::str::from_utf8(&output.stdout)
        .expect("utf8 wire output")
        .lines()
        .map(|line| DaemonMessage::decode(line).expect("canonical daemon reply"))
        .collect::<Vec<_>>();
    assert_eq!(
        messages.len(),
        2,
        "one welcome and one result: {messages:?}"
    );
    let DaemonMessage::Result { value, .. } = messages.last().expect("result") else {
        panic!("endpoint import did not return a result: {messages:?}");
    };
    value.clone()
}

fn preview(source: &Path) -> String {
    let output = meshctl(&["import-preview", source.to_str().expect("utf8 path")]);
    let report = success_json(&output);
    assert_eq!(
        report.get("action").and_then(Json::as_text),
        Some("folder-import-preview")
    );
    report
        .get("summary")
        .and_then(Json::as_text)
        .expect("summary digest")
        .to_owned()
}

fn confirm(source: &Path, managed: &Path, summary: &str) -> Json {
    success_json(&meshctl(&[
        "import-confirm",
        source.to_str().expect("utf8 source"),
        managed.to_str().expect("utf8 destination"),
        summary,
    ]))
}

fn endpoint(name: &str) -> PathBuf {
    // GitHub's macOS runner refuses Unix sockets directly under /private/tmp,
    // while allowing them in an owned per-job directory. IpcServer also secures
    // its parent to 0700, so never point it at the shared temporary root.
    let directory =
        std::env::temp_dir().join(format!("mesh-import-ipc-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).expect("endpoint directory");
    directory.join("daemon.sock")
}

#[test]
fn endpoint_import_uses_the_daemon_authenticated_same_folder_scope() {
    let root = scratch("endpoint-same-folder");
    let source = fixture(&root);
    let managed = root.join("managed-project");
    let daemon = Arc::new(LiveDaemon::new(StartupSummary::from(&nothing_to_recover())));
    daemon
        .open_at_start(&source)
        .expect("open the old zero-history workspace");
    fs::create_dir_all(source.join("chunks/aa")).expect("old private content store");
    fs::write(source.join("chunks/aa/private"), b"private payload").expect("old private payload");
    let endpoint = endpoint("import");
    let _ = fs::remove_file(&endpoint);
    let server = IpcServer::bind(&endpoint)
        .expect("bind daemon")
        .spawn(daemon)
        .expect("serve daemon");

    let preview = wire_result(&meshctl(&[
        "--endpoint",
        endpoint.to_str().expect("utf8 endpoint"),
        "import-preview",
        source.to_str().expect("utf8 source"),
    ]));
    assert_eq!(
        preview.get("source_scope").and_then(Json::as_text),
        Some("open-zero-history-workspace")
    );
    assert_eq!(preview.get("files").and_then(Json::as_u64), Some(2));
    let summary = preview
        .get("summary")
        .and_then(Json::as_text)
        .expect("daemon-scoped summary")
        .to_owned();

    let confirmed = wire_result(&meshctl(&[
        "--endpoint",
        endpoint.to_str().expect("utf8 endpoint"),
        "import-confirm",
        source.to_str().expect("utf8 source"),
        managed.to_str().expect("utf8 destination"),
        &summary,
    ]));
    assert_eq!(
        confirmed
            .get("workspace")
            .and_then(|workspace| workspace.get("records"))
            .and_then(Json::as_u64),
        Some(3),
        "the daemon opened the install, actor and operation records"
    );
    assert_eq!(
        confirmed
            .get("workspace")
            .and_then(|workspace| workspace.get("operations"))
            .and_then(Json::as_u64),
        Some(1),
        "the imported workspace has one content operation"
    );
    let working = managed.join(mesh_store::MOUNT_DIRECTORY_NAME);
    assert!(working.join("README.md").is_file());
    assert!(working.join("src/main.rs").is_file());
    assert!(!working.join("records.mesh").exists());
    assert!(!working.join("metadata.sqlite").exists());
    assert!(!working.join("chunks").exists());
    assert!(source.join("chunks/aa/private").is_file());

    server.shutdown();
    let _ = fs::remove_file(endpoint);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn offline_preview_confirms_through_the_live_daemon_without_a_false_drift() {
    let root = scratch("offline-to-endpoint");
    let source = fixture(&root);
    let managed = root.join("managed-project");
    let before = tree_bytes(&source);

    let offline = success_json(&meshctl(&[
        "import-preview",
        source.to_str().expect("utf8 source"),
    ]));
    assert_eq!(
        offline.get("source_scope").and_then(Json::as_text),
        Some("ordinary-folder")
    );
    assert_eq!(
        offline
            .get("file_entries")
            .and_then(Json::as_array)
            .map(|entries| entries.len()),
        Some(2),
        "the recovery preview should remain human-reviewable"
    );
    let summary = offline
        .get("summary")
        .and_then(Json::as_text)
        .expect("ordinary-folder confirmation token")
        .to_owned();
    assert_eq!(
        tree_bytes(&source),
        before,
        "offline preview changed the source"
    );

    let daemon = Arc::new(LiveDaemon::new(StartupSummary::from(&nothing_to_recover())));
    let endpoint = endpoint("offline");
    let _ = fs::remove_file(&endpoint);
    let server = IpcServer::bind(&endpoint)
        .expect("bind daemon")
        .spawn(daemon)
        .expect("serve daemon");

    let confirmed = wire_result(&meshctl(&[
        "--endpoint",
        endpoint.to_str().expect("utf8 endpoint"),
        "import-confirm",
        source.to_str().expect("utf8 source"),
        managed.to_str().expect("utf8 destination"),
        &summary,
    ]));
    assert_eq!(
        confirmed
            .get("workspace")
            .and_then(|workspace| workspace.get("records"))
            .and_then(Json::as_u64),
        Some(3)
    );
    assert_eq!(
        confirmed
            .get("workspace")
            .and_then(|workspace| workspace.get("operations"))
            .and_then(Json::as_u64),
        Some(1)
    );
    assert_eq!(
        tree_bytes(&source),
        before,
        "live confirmation changed the source"
    );
    assert!(managed
        .join(mesh_store::MOUNT_DIRECTORY_NAME)
        .join("README.md")
        .is_file());

    server.shutdown();
    let _ = fs::remove_file(endpoint);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn support_bundle_emits_exactly_one_document() {
    let root = scratch("support-bundle-single-line");
    let output = meshctl(&["support-bundle", root.to_str().expect("utf8 workspace")]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let text = std::str::from_utf8(&output.stdout).expect("utf8");
    let lines: Vec<_> = text.lines().collect();
    assert_eq!(lines.len(), 1, "one command must emit one document: {text}");
    Json::parse(lines[0]).expect("the one line is JSON");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn separate_processes_preview_confirm_restart_and_rollback_without_touching_source() {
    let root = scratch("roundtrip");
    let source = fixture(&root);
    let managed = root.join("managed-project");
    let before = tree_bytes(&source);

    let summary = preview(&source);
    assert!(!managed.exists(), "preview performs no writes");
    assert_eq!(tree_bytes(&source), before);

    let confirmation = confirm(&source, &managed, &summary);
    assert_eq!(
        confirmation.get("action").and_then(Json::as_text),
        Some("folder-import-confirmed")
    );
    let receipt = PathBuf::from(
        confirmation
            .get("receipt")
            .and_then(Json::as_text)
            .expect("durable receipt path"),
    );
    assert!(receipt.exists());
    assert_eq!(confirmation.get("private_history"), Some(&Json::Bool(true)));
    assert_eq!(
        confirmation.get("shared_version_advanced"),
        Some(&Json::Bool(false))
    );
    assert_eq!(confirmation.get("manifests"), Some(&Json::Number(2)));
    assert_eq!(
        confirmation.get("materialized_entries"),
        Some(&Json::Number(4))
    );
    assert!(confirmation
        .get("operation")
        .and_then(Json::as_text)
        .is_some());
    let working = PathBuf::from(
        confirmation
            .get("destination")
            .and_then(Json::as_text)
            .expect("presented working folder"),
    );
    assert_eq!(working, managed.join(mesh_store::MOUNT_DIRECTORY_NAME));
    assert_imported_tree(&working, &before);
    assert!(managed.join("records.mesh").is_file());
    assert!(managed.join("metadata.sqlite").is_file());
    assert!(!working.join("records.mesh").exists());
    assert!(!working.join("metadata.sqlite").exists());
    assert!(!working.join(".mesh").exists());
    let mut top_level = fs::read_dir(&working)
        .expect("managed root")
        .map(|entry| entry.expect("managed entry").file_name())
        .collect::<Vec<_>>();
    top_level.sort();
    assert_eq!(top_level, ["README.md", "src"]);

    let first_open = OpenWorkspace::open(&working).expect("first managed restart");
    assert_eq!(first_open.operations(), 1);
    assert_eq!(first_open.manifests(), 2);
    assert!(first_open.names_answered());
    assert!(first_open.shared_version().is_none());
    let first_digest = first_open.digest();
    let first_entries = first_open
        .entries()
        .iter()
        .map(|entry| entry.path().to_owned())
        .collect::<Vec<_>>();
    drop(first_open);
    let second_open = OpenWorkspace::open(&working).expect("second managed restart");
    assert_eq!(second_open.digest(), first_digest);
    assert_eq!(
        second_open
            .entries()
            .iter()
            .map(|entry| entry.path().to_owned())
            .collect::<Vec<_>>(),
        first_entries
    );
    assert_eq!(tree_bytes(&source), before);

    let custody_daemon = LiveDaemon::new(StartupSummary::from(&nothing_to_recover()));
    let custody_workspace = custody_daemon
        .reopen_existing_workspace(&working)
        .expect("open workspace for shared custody");
    let generation = custody_daemon
        .acquire_workspace_agent_custody(
            &custody_workspace.root,
            &custody_workspace.digest,
            &custody_workspace.installation,
            false,
            None,
        )
        .expect("acquire shared custody");

    // A third process must observe the same workspace-native custody and never remove the folder.
    let refused = meshctl(&[
        "import-rollback",
        working.to_str().expect("utf8 destination"),
    ]);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(managed.is_dir(), "assigned managed workspace is preserved");
    assert!(receipt.is_file(), "rollback receipt remains available");

    custody_daemon
        .release_workspace_agent_custody(
            &custody_workspace.root,
            &custody_workspace.digest,
            &custody_workspace.installation,
            &generation,
        )
        .expect("release exact shared custody");

    // Once released, a third process can still reconstruct the exact receipt and roll it back.
    let rollback = success_json(&meshctl(&[
        "import-rollback",
        working.to_str().expect("utf8 destination"),
    ]));
    assert_eq!(
        rollback.get("action").and_then(Json::as_text),
        Some("folder-import-rolled-back")
    );
    assert!(!managed.exists());
    assert!(!receipt.exists());
    assert_eq!(tree_bytes(&source), before);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn confirmation_with_a_stale_preview_rolls_back_its_copy_and_changes_no_source_byte() {
    let root = scratch("stale-preview");
    let source = fixture(&root);
    let managed = root.join("managed-project");
    let before = tree_bytes(&source);
    let wrong = "00".repeat(32);

    let output = meshctl(&[
        "import-confirm",
        source.to_str().expect("utf8 source"),
        managed.to_str().expect("utf8 destination"),
        &wrong,
    ]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("changed since preview"),
        "{output:?}"
    );
    assert!(!managed.exists());
    assert_eq!(tree_bytes(&source), before);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn rollback_after_restart_refuses_changed_managed_work_and_a_corrupt_receipt() {
    let root = scratch("refusals");
    let source = fixture(&root);
    let before = tree_bytes(&source);

    let changed = root.join("changed-managed");
    let changed_summary = preview(&source);
    let changed_confirmation = confirm(&source, &changed, &changed_summary);
    let changed_working = PathBuf::from(
        changed_confirmation
            .get("destination")
            .and_then(Json::as_text)
            .expect("changed working folder"),
    );
    fs::write(
        changed_working.join("new-work.txt"),
        b"must survive rollback",
    )
    .expect("new managed work");
    let refused = meshctl(&[
        "import-rollback",
        changed_working.to_str().expect("utf8 destination"),
    ]);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert_eq!(
        fs::read(changed_working.join("new-work.txt")).expect("retained work"),
        b"must survive rollback"
    );
    assert!(Path::new(
        changed_confirmation
            .get("receipt")
            .and_then(Json::as_text)
            .expect("receipt")
    )
    .exists());

    let corrupt = root.join("corrupt-receipt-managed");
    let corrupt_summary = preview(&source);
    let corrupt_confirmation = confirm(&source, &corrupt, &corrupt_summary);
    let corrupt_working = PathBuf::from(
        corrupt_confirmation
            .get("destination")
            .and_then(Json::as_text)
            .expect("corrupt working folder"),
    );
    let corrupt_tree = tree_bytes(&corrupt_working);
    let receipt = Path::new(
        corrupt_confirmation
            .get("receipt")
            .and_then(Json::as_text)
            .expect("receipt"),
    );
    fs::write(receipt, b"not a canonical Mesh import receipt").expect("corrupt receipt");
    let refused = meshctl(&[
        "import-rollback",
        corrupt_working.to_str().expect("utf8 destination"),
    ]);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert_eq!(tree_bytes(&corrupt_working), corrupt_tree);
    assert_imported_tree(&corrupt_working, &before);

    let replacement = root.join("replacement-managed");
    let replacement_summary = preview(&source);
    let replacement_confirmation = confirm(&source, &replacement, &replacement_summary);
    let replacement_working = PathBuf::from(
        replacement_confirmation
            .get("destination")
            .and_then(Json::as_text)
            .expect("replacement working folder"),
    );
    let displaced = root.join("displaced-confirmed-import");
    fs::rename(&replacement_working, &displaced).expect("displace imported directory instance");
    fs::create_dir(&replacement_working).expect("replacement directory");
    fs::write(replacement_working.join("keep.txt"), b"foreign replacement")
        .expect("replacement work");
    let refused = meshctl(&[
        "import-rollback",
        replacement_working.to_str().expect("utf8 destination"),
    ]);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert_eq!(
        fs::read(replacement_working.join("keep.txt")).expect("replacement retained"),
        b"foreign replacement"
    );
    assert!(
        displaced.exists(),
        "original directory instance is retained"
    );
    assert!(Path::new(
        replacement_confirmation
            .get("receipt")
            .and_then(Json::as_text)
            .expect("receipt")
    )
    .exists());
    assert_eq!(tree_bytes(&source), before);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn presented_import_keeps_a_user_records_mesh_separate_from_private_history() {
    let root = scratch("ordinary-records-name");
    let source = fixture(&root);
    fs::write(source.join("records.mesh"), b"user-owned journal name\n")
        .expect("ordinary source file");
    let before = tree_bytes(&source);
    let store = root.join("managed-project");

    let summary = preview(&source);
    assert!(!store.exists());
    let confirmation = confirm(&source, &store, &summary);
    let working = PathBuf::from(
        confirmation
            .get("destination")
            .and_then(Json::as_text)
            .expect("presented folder"),
    );
    assert_eq!(
        fs::read(working.join("records.mesh")).expect("ordinary imported file"),
        b"user-owned journal name\n"
    );
    assert_ne!(working.join("records.mesh"), store.join("records.mesh"));
    let opened = OpenWorkspace::open(&working).expect("reopen imported workspace");
    assert!(opened
        .entries()
        .iter()
        .any(|entry| entry.path() == "records.mesh"));
    drop(opened);

    success_json(&meshctl(&[
        "import-rollback",
        working.to_str().expect("utf8 destination"),
    ]));
    assert!(!store.exists());
    assert_eq!(tree_bytes(&source), before);
    let _ = fs::remove_dir_all(root);
}
