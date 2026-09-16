//! Real-filesystem existing-folder import, confirmation, rollback, and restart recovery.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;

use mesh_daemon::{
    preview_folder_import, recover_pending_import, FolderImportError, PreparedFolderImport,
};

fn scratch(name: &str) -> PathBuf {
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    let serial = SERIAL.fetch_add(1, Ordering::SeqCst);
    let mut root = std::env::temp_dir();
    root.push(format!(
        "mesh-folder-import-{name}-{}-{serial}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("mkdir");
    root
}

fn fixture(root: &Path) -> PathBuf {
    let source = root.join("existing-project");
    fs::create_dir_all(source.join("src/nested-empty")).expect("directories");
    fs::write(source.join("README.md"), b"hello mesh\n").expect("readme");
    fs::write(source.join("src/main.rs"), b"fn main() {}\n").expect("main");
    source
}

#[cfg(unix)]
#[test]
fn executable_mode_is_part_of_the_confirmed_import_snapshot() {
    let root = scratch("executable-mode-confirmation");
    let source = fixture(&root);
    let script = source.join("run.sh");
    fs::write(&script, b"#!/bin/sh\nprintf 'mesh\\n'\n").expect("script");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("executable source");
    let store = root.join("managed.mesh");

    let preview = preview_folder_import(&source).expect("preview");
    let previewed_script = preview
        .files()
        .iter()
        .find(|file| file.relative_path() == Path::new("run.sh"))
        .expect("previewed script");
    assert!(previewed_script.executable());

    let prepared =
        PreparedFolderImport::prepare_presented(&source, &store).expect("prepared import");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o644)).expect("mode-only edit");
    let result = prepared.confirm_into_workspace();

    assert!(matches!(
        result,
        Err(FolderImportError::VerificationMismatch { ref paths })
            if paths == &[PathBuf::from("run.sh")]
    ));
    assert!(source.is_dir(), "the source folder was mutated or removed");
    assert!(!store.exists(), "the refused managed copy was retained");
    let _ = fs::remove_dir_all(root);
}

fn source_bytes(source: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut found = vec![
        (
            PathBuf::from("README.md"),
            fs::read(source.join("README.md")).expect("read"),
        ),
        (
            PathBuf::from("src/main.rs"),
            fs::read(source.join("src/main.rs")).expect("read"),
        ),
    ];
    found.sort();
    found
}

#[test]
fn import_restart_child() {
    let Ok(source) = std::env::var("MESH_IMPORT_CHILD_SOURCE") else {
        return;
    };
    let destination = std::env::var("MESH_IMPORT_CHILD_DESTINATION").expect("destination");
    let prepared = if std::env::var_os("MESH_IMPORT_CHILD_PRESENTED").is_some() {
        PreparedFolderImport::prepare_presented(Path::new(&source), Path::new(&destination))
    } else {
        PreparedFolderImport::prepare(Path::new(&source), Path::new(&destination))
    }
    .expect("child prepare");
    assert!(prepared.destination().exists());
    std::process::exit(0); // deliberately skip Drop, as a stopped process does
}

#[test]
fn a_presented_import_keeps_private_state_outside_the_native_working_folder() {
    let root = scratch("presented-roundtrip");
    let source = fixture(&root);
    let before = source_bytes(&source);
    let store = root.join("mesh-private-workspace");

    let prepared =
        PreparedFolderImport::prepare_presented(&source, &store).expect("prepare presented");
    let working = store.join(mesh_store::MOUNT_DIRECTORY_NAME);
    assert_eq!(prepared.destination(), working);
    assert!(store.join(".mesh-presented-workspace").is_file());
    assert!(!working.join(".mesh").exists());

    let (confirmed, outcome) = prepared
        .confirm_into_workspace()
        .expect("confirm presented workspace");
    assert_eq!(confirmed.destination(), working);
    assert_eq!(outcome.entries(), 4);
    assert!(store.join(mesh_daemon::RECORD_FILE_NAME).is_file());
    assert!(store.join(mesh_store::DATABASE_FILE_NAME).is_file());
    assert!(!working.join(mesh_daemon::RECORD_FILE_NAME).exists());
    assert!(!working.join(mesh_store::DATABASE_FILE_NAME).exists());
    assert!(!working.join(".mesh").exists());
    assert_eq!(source_bytes(&source), before);

    drop(confirmed);
    let reopened = mesh_daemon::OpenWorkspace::open(&working).expect("reopen working folder");
    assert_eq!(reopened.root().as_path(), working);
    assert_eq!(
        reopened.record_file(),
        store
            .canonicalize()
            .expect("canonical store")
            .join(mesh_daemon::RECORD_FILE_NAME)
    );
    drop(reopened);
    mesh_daemon::ConfirmedFolderImport::open(&working)
        .expect("reopen receipt")
        .rollback()
        .expect("rollback exact presented import");
    assert!(!store.exists());
    assert_eq!(source_bytes(&source), before);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn restart_recovery_removes_the_exact_presented_store_and_nothing_else() {
    let root = scratch("presented-restart");
    let source = fixture(&root);
    let before = source_bytes(&source);
    let store = root.join("mesh-private-workspace");
    let status = Command::new(std::env::current_exe().expect("test executable"))
        .arg("--exact")
        .arg("import_restart_child")
        .arg("--nocapture")
        .env("MESH_IMPORT_CHILD_SOURCE", &source)
        .env("MESH_IMPORT_CHILD_DESTINATION", &store)
        .env("MESH_IMPORT_CHILD_PRESENTED", "1")
        .status()
        .expect("start presented import child");
    assert!(status.success(), "child failed: {status}");
    let working = store.join(mesh_store::MOUNT_DIRECTORY_NAME);
    assert!(working.exists());

    assert!(recover_pending_import(&working).expect("recover presented"));
    assert!(!store.exists());
    assert!(!recover_pending_import(&working).expect("idempotent"));
    assert_eq!(source_bytes(&source), before);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_real_folder_is_verified_confirmed_and_rolled_back_without_touching_the_original() {
    let root = scratch("roundtrip");
    let source = fixture(&root);
    let before = source_bytes(&source);
    let managed = root.join("managed-project");

    let prepared = PreparedFolderImport::prepare(&source, &managed).expect("prepare");
    assert_eq!(prepared.summary().file_count(), 2);
    assert_eq!(prepared.summary().directory_count(), 2);
    assert_eq!(prepared.summary().total_bytes(), 24);
    assert_eq!(
        prepared.summary().files()[0].relative_path(),
        Path::new("README.md")
    );
    assert!(managed.exists());
    assert_eq!(source_bytes(&source), before);

    let confirmed = prepared.confirm().expect("confirm");
    let receipt = confirmed.receipt().to_path_buf();
    assert!(receipt.exists(), "confirmation is durable across restart");
    assert_eq!(confirmed.summary().file_count(), 2);
    assert_eq!(
        fs::read(managed.join("README.md")).expect("copy"),
        b"hello mesh\n"
    );
    drop(confirmed);
    let reopened = mesh_daemon::ConfirmedFolderImport::open(&managed).expect("reopen receipt");
    reopened.rollback().expect("rollback");
    assert!(!managed.exists(), "pre-import destination absence restored");
    assert!(!receipt.exists(), "rollback consumes the durable receipt");
    assert_eq!(source_bytes(&source), before);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_verification_difference_names_the_path_and_rolls_back_only_the_managed_copy() {
    let root = scratch("mismatch");
    let source = fixture(&root);
    let before = source_bytes(&source);
    let managed = root.join("managed-project");
    let prepared = PreparedFolderImport::prepare(&source, &managed).expect("prepare");
    fs::write(managed.join("src/main.rs"), b"changed behind import\n").expect("mutate copy");

    let failure = prepared.confirm().expect_err("verification refuses");
    assert!(matches!(
        failure,
        FolderImportError::VerificationMismatch { ref paths }
            if paths == &[PathBuf::from("src/main.rs")]
    ));
    assert!(!managed.exists(), "failed import rolled its copy back");
    assert_eq!(source_bytes(&source), before);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn restart_recovery_removes_only_a_marker_owned_unconfirmed_copy() {
    let root = scratch("restart");
    let source = fixture(&root);
    let before = source_bytes(&source);
    let managed = root.join("managed-project");
    let status = Command::new(std::env::current_exe().expect("test executable"))
        .arg("--exact")
        .arg("import_restart_child")
        .arg("--nocapture")
        .env("MESH_IMPORT_CHILD_SOURCE", &source)
        .env("MESH_IMPORT_CHILD_DESTINATION", &managed)
        .status()
        .expect("start import child");
    assert!(status.success(), "child failed: {status}");
    assert!(managed.exists());

    assert!(recover_pending_import(&managed).expect("recover"));
    assert!(!managed.exists());
    assert!(!recover_pending_import(&managed).expect("idempotent"));
    assert_eq!(source_bytes(&source), before);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn restart_recovery_refuses_an_unknown_marker_without_deleting_the_destination() {
    let root = scratch("foreign-marker");
    let managed = root.join("managed-project");
    fs::create_dir(&managed).expect("foreign destination");
    fs::write(managed.join("keep.txt"), b"not owned by this import").expect("foreign work");
    let marker = root.join(".managed-project.mesh-import-owned");
    fs::write(&marker, b"not a mesh import marker").expect("foreign marker");

    assert!(matches!(
        recover_pending_import(&managed),
        Err(FolderImportError::UnrecognizedMarker { path }) if path == marker
    ));
    assert_eq!(
        fs::read(managed.join("keep.txt")).expect("retained destination"),
        b"not owned by this import"
    );
    assert!(marker.exists(), "unknown marker is retained for diagnosis");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn an_exact_owned_marker_cannot_delete_a_replacement_directory() {
    let root = scratch("replaced-destination");
    let source = fixture(&root);
    let managed = root.join("managed-project");
    let prepared = PreparedFolderImport::prepare(&source, &managed).expect("prepare");
    std::mem::forget(prepared); // model a stopped process while retaining its exact marker

    let displaced = root.join("displaced-owned-directory");
    fs::rename(&managed, &displaced).expect("replace original directory instance");
    fs::create_dir(&managed).expect("replacement destination");
    fs::write(managed.join("keep.txt"), b"foreign replacement").expect("replacement work");

    assert!(matches!(
        recover_pending_import(&managed),
        Err(FolderImportError::OwnershipMismatch { destination })
            if destination == managed
    ));
    assert_eq!(
        fs::read(managed.join("keep.txt")).expect("replacement retained"),
        b"foreign replacement"
    );
    assert!(
        displaced.exists(),
        "the originally owned instance is untouched too"
    );
    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn an_unsupported_link_is_refused_before_destination_or_markers_exist() {
    use std::os::unix::fs::symlink;

    let root = scratch("link");
    let source = fixture(&root);
    symlink("README.md", source.join("readme-link")).expect("symlink");
    let managed = root.join("managed-project");

    assert!(matches!(
        PreparedFolderImport::prepare(&source, &managed),
        Err(FolderImportError::UnsupportedEntry { .. })
    ));
    assert!(!managed.exists());
    assert_eq!(
        fs::read(source.join("README.md")).expect("original"),
        b"hello mesh\n"
    );
    assert!(fs::symlink_metadata(source.join("readme-link"))
        .expect("link remains")
        .file_type()
        .is_symlink());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_destination_inside_the_original_is_refused_before_mutation() {
    let root = scratch("inside");
    let source = fixture(&root);
    let managed = source.join("managed-project");
    assert!(matches!(
        PreparedFolderImport::prepare(&source, &managed),
        Err(FolderImportError::DestinationInsideSource { .. })
    ));
    assert!(!managed.exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn rollback_refuses_to_delete_a_confirmed_workspace_that_changed() {
    let root = scratch("changed-after-confirm");
    let source = fixture(&root);
    let before = source_bytes(&source);
    let managed = root.join("managed-project");
    let confirmed = PreparedFolderImport::prepare(&source, &managed)
        .expect("prepare")
        .confirm()
        .expect("confirm");
    drop(confirmed);
    fs::write(managed.join("new-work.txt"), b"do not delete me").expect("new work");

    let confirmed = mesh_daemon::ConfirmedFolderImport::open(&managed).expect("reopen receipt");
    assert!(matches!(
        confirmed.rollback(),
        Err(FolderImportError::RollbackRefused { ref paths })
            if paths == &[PathBuf::from("new-work.txt")]
    ));
    assert_eq!(
        fs::read(managed.join("new-work.txt")).expect("retained"),
        b"do not delete me"
    );
    assert_eq!(source_bytes(&source), before);
    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn repository_exclusions_skip_git_metadata_and_ignored_links_without_losing_source_data() {
    use std::os::unix::fs::symlink;

    let root = scratch("repository-exclusions");
    let source = fixture(&root);
    fs::write(source.join(".gitignore"), b"node_modules/\ntarget/\n").expect("ignore rules");
    fs::write(source.join(".git"), b"gitdir: /private/original/worktree\n")
        .expect("worktree pointer");
    fs::create_dir(source.join("node_modules")).expect("ignored dependency directory");
    symlink("../README.md", source.join("node_modules/tool")).expect("ignored link");
    fs::create_dir_all(source.join("nested/.git")).expect("nested repository metadata");
    fs::write(source.join("nested/.git/config"), b"original-only\n").expect("nested git data");
    let managed = root.join("managed-project");

    let preview = preview_folder_import(&source).expect("excluded entries are not opened");
    assert!(preview
        .files()
        .iter()
        .any(|file| file.relative_path() == Path::new(".gitignore")));
    let prepared = PreparedFolderImport::prepare(&source, &managed).expect("prepare");
    assert_eq!(prepared.summary().digest(), preview.digest());
    let (confirmed, _) = prepared
        .confirm_into_workspace()
        .expect("confirm and ingest");

    assert_eq!(
        fs::read(source.join(".git")).expect("source git pointer retained"),
        b"gitdir: /private/original/worktree\n"
    );
    assert!(fs::symlink_metadata(source.join("node_modules/tool"))
        .expect("source link retained")
        .file_type()
        .is_symlink());
    assert!(!managed.join(".git").exists());
    assert!(!managed.join("nested/.git").exists());
    assert!(!managed.join("node_modules").exists());
    assert!(managed.join("README.md").is_file());
    assert!(managed.join(".gitignore").is_file());

    drop(confirmed);
    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn meshignore_reinclusion_is_load_bearing_and_included_links_still_refuse() {
    use std::os::unix::fs::symlink;

    let root = scratch("meshignore-reinclude");
    let source = fixture(&root);
    fs::write(source.join(".gitignore"), b"node_modules/\n").expect("repository rules");
    fs::write(source.join(".meshignore"), b"!node_modules/\n").expect("mesh rules");
    fs::create_dir(source.join("node_modules")).expect("dependency directory");
    symlink("../README.md", source.join("node_modules/tool")).expect("included link");
    let managed = root.join("managed-project");
    let canonical_source = source.canonicalize().expect("canonical source");

    let result = PreparedFolderImport::prepare(&source, &managed);
    assert!(
        matches!(
            &result,
            Err(FolderImportError::UnsupportedEntry { path })
            if path == &canonical_source.join("node_modules/tool")
        ),
        "unexpected result: {result:?}"
    );
    assert!(!managed.exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn descendant_reinclusion_below_an_excluded_parent_refuses_instead_of_omitting_work() {
    let root = scratch("descendant-reinclude");
    let source = root.join("source");
    let managed = root.join("managed-project");
    fs::create_dir_all(source.join("build/keep")).expect("re-included source tree");
    fs::write(source.join(".gitignore"), b"build/\n").expect("repository rules");
    fs::write(source.join(".meshignore"), b"!build/keep\n").expect("Mesh rules");
    fs::write(source.join("build/keep/note.md"), b"must not disappear\n")
        .expect("re-included work");

    let refusal = PreparedFolderImport::prepare(&source, &managed)
        .expect_err("a claimed re-inclusion cannot be silently omitted");
    let canonical_source = source.canonicalize().expect("canonical source");
    assert!(
        matches!(
            &refusal,
            FolderImportError::ReincludedPathBelowExcludedAncestor {
                path,
                excluded_ancestor,
            } if path == &canonical_source.join("build/keep")
                && excluded_ancestor == &canonical_source.join("build")
        ),
        "unexpected refusal: {refusal:?}"
    );
    assert!(
        !managed.exists(),
        "preview refusal must create no managed copy"
    );
    assert_eq!(
        fs::read(source.join("build/keep/note.md")).unwrap(),
        b"must not disappear\n"
    );
    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn exclusion_sources_must_be_confined_regular_files() {
    use std::os::unix::fs::symlink;

    let root = scratch("linked-exclusion-source");
    let source = fixture(&root);
    let outside = root.join("outside-ignore");
    fs::write(&outside, b"src/\n").expect("outside rules");
    symlink(&outside, source.join(".gitignore")).expect("linked rules");
    let managed = root.join("managed-project");
    let canonical_source = source.canonicalize().expect("canonical source");

    let result = PreparedFolderImport::prepare(&source, &managed);
    assert!(
        matches!(
            &result,
            Err(FolderImportError::ExclusionRulesUnavailable { path, .. })
            if path == &canonical_source.join(".gitignore")
        ),
        "unexpected result: {result:?}"
    );
    assert!(!managed.exists());
    assert_eq!(fs::read(&outside).expect("outside untouched"), b"src/\n");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn invalid_exclusion_rules_refuse_before_destination_creation() {
    let root = scratch("invalid-exclusion-rule");
    let source = fixture(&root);
    fs::write(source.join(".gitignore"), b"build/**/secret\n").expect("invalid rules");
    let managed = root.join("managed-project");

    assert!(matches!(
        preview_folder_import(&source),
        Err(FolderImportError::ExclusionRulesUnavailable { .. })
    ));
    assert!(matches!(
        PreparedFolderImport::prepare(&source, &managed),
        Err(FolderImportError::ExclusionRulesUnavailable { .. })
    ));
    assert!(!managed.exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn private_store_names_are_ordinary_content_only_in_a_presented_import() {
    let root = scratch("private-name-content");
    let source = fixture(&root);
    fs::create_dir(source.join("chunks")).expect("ordinary project directory");
    fs::write(source.join("chunks/content"), b"project bytes").expect("ordinary project file");
    fs::write(source.join("metadata.sqlite"), b"project database bytes")
        .expect("ordinary project database");
    let legacy = root.join("legacy-managed-project");
    let store = root.join("presented-private-store");

    let preview = preview_folder_import(&source).expect("presented import preview");
    assert!(preview
        .files()
        .iter()
        .any(|file| file.relative_path() == Path::new("chunks/content")));
    assert!(preview
        .files()
        .iter()
        .any(|file| file.relative_path() == Path::new("metadata.sqlite")));

    assert!(matches!(
        PreparedFolderImport::prepare(&source, &legacy),
        Err(FolderImportError::ReservedWorkspacePath { ref path })
            if path == Path::new("chunks")
    ));
    assert!(!legacy.exists(), "legacy private names remain protected");

    let (confirmed, _) = PreparedFolderImport::prepare_presented(&source, &store)
        .expect("prepare external-store import")
        .confirm_into_workspace()
        .expect("confirm external-store import");
    let working = confirmed.destination().to_path_buf();
    assert_eq!(
        fs::read(working.join("chunks/content")).unwrap(),
        b"project bytes"
    );
    assert_eq!(
        fs::read(working.join("metadata.sqlite")).unwrap(),
        b"project database bytes"
    );
    let opened = mesh_daemon::OpenWorkspace::open(&working).expect("reopen presented workspace");
    assert!(opened
        .entries()
        .iter()
        .any(|entry| entry.path() == "chunks/content"));
    assert!(opened
        .entries()
        .iter()
        .any(|entry| entry.path() == "metadata.sqlite"));
    assert_ne!(opened.database_file(), working.join("metadata.sqlite"));
    fs::write(working.join("chunks/new-agent-output"), b"later agent work")
        .expect("new file below ordinary chunks directory");
    fs::create_dir(working.join("logs")).expect("ordinary logs directory");
    assert_eq!(
        opened.native_untracked_files().expect("native discovery"),
        vec!["chunks/new-agent-output"],
        "private-store names in the external sibling must not hide native user work"
    );
    assert_eq!(
        opened
            .native_untracked_directories()
            .expect("native directory discovery"),
        vec!["logs"],
        "a new ordinary root directory must remain adoptable"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn rollback_sees_new_excluded_content_and_never_deletes_it() {
    let root = scratch("rollback-excluded-content");
    let source = fixture(&root);
    fs::write(source.join(".gitignore"), b"node_modules/\n").expect("ignore rules");
    let managed = root.join("managed-project");
    let confirmed = PreparedFolderImport::prepare(&source, &managed)
        .expect("prepare")
        .confirm()
        .expect("confirm");
    drop(confirmed);
    fs::create_dir(managed.join("node_modules")).expect("new excluded directory");
    fs::write(managed.join("node_modules/agent-output"), b"keep me\n").expect("new excluded work");

    let confirmed = mesh_daemon::ConfirmedFolderImport::open(&managed).expect("reopen receipt");
    let failure = confirmed
        .rollback()
        .expect_err("new excluded work blocks deletion");
    assert!(matches!(
        failure,
        FolderImportError::RollbackRefused { ref paths }
            if paths.contains(&PathBuf::from("node_modules"))
                && paths.contains(&PathBuf::from("node_modules/agent-output"))
    ));
    assert_eq!(
        fs::read(managed.join("node_modules/agent-output")).expect("retained"),
        b"keep me\n"
    );
    let _ = fs::remove_dir_all(root);
}
