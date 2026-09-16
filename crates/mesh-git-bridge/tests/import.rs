//! Exact Git provenance and independent native-workspace tests.

use mesh_git_bridge::{
    install_independent_git_context, install_independent_git_history, GitContextError,
    GitImportError, GitObjectFormat, GitProvenanceAnchor,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "mesh-git-import-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("scratch");
        Self(root)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn git(root: &Path, arguments: &[&str]) -> Vec<u8> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .output()
        .expect("run git fixture command");
    assert!(
        output.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn repository(label: &str) -> Scratch {
    let scratch = Scratch::new(label);
    git(&scratch.0, &["init", "-b", "main"]);
    git(&scratch.0, &["config", "user.name", "Mesh test"]);
    git(
        &scratch.0,
        &["config", "user.email", "mesh@example.invalid"],
    );
    fs::write(scratch.0.join("tracked.txt"), b"baseline\n").expect("tracked file");
    git(&scratch.0, &["add", "tracked.txt"]);
    git(&scratch.0, &["commit", "-m", "baseline"]);
    scratch
}

fn direct_status(root: &Path) -> Vec<u8> {
    git(
        root,
        &[
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.untrackedCache=false",
            "-c",
            "core.hooksPath=/dev/null",
            "status",
            "--porcelain=v2",
            "--branch",
            "--show-stash",
            "--untracked-files=all",
            "--ignored=matching",
            "-z",
        ],
    )
}

fn non_ignored_status_records(status: &[u8]) -> Vec<&[u8]> {
    status
        .split(|byte| *byte == 0)
        .filter(|record| {
            !record.is_empty() && !record.starts_with(b"# ") && !record.starts_with(b"! ")
        })
        .collect()
}

#[test]
fn captures_clean_dirty_ignored_and_detached_state_exactly() {
    let scratch = repository("states");
    let clean = GitProvenanceAnchor::inspect(&scratch.0).expect("clean anchor");
    assert_eq!(clean.repository().object_format(), GitObjectFormat::Sha1);
    assert_eq!(clean.status_porcelain_v2_z(), direct_status(&scratch.0));
    assert_eq!(clean.head_ref(), Some(b"refs/heads/main".as_slice()));
    assert!(clean.ignored_paths().is_empty());

    fs::write(scratch.0.join(".gitignore"), b"ignored.log\n").expect("ignore file");
    fs::write(scratch.0.join("tracked.txt"), b"changed\n").expect("dirty tracked file");
    fs::write(scratch.0.join("untracked.txt"), b"new\n").expect("untracked file");
    fs::write(scratch.0.join("ignored.log"), b"private\n").expect("ignored file");

    let dirty = GitProvenanceAnchor::inspect(&scratch.0).expect("dirty anchor");
    assert_eq!(dirty.status_porcelain_v2_z(), direct_status(&scratch.0));
    assert_eq!(dirty.ignored_paths().len(), 1);
    assert_eq!(dirty.ignored_paths()[0].as_bytes(), b"ignored.log");
    assert_eq!(dirty.repository(), clean.repository());
    assert_ne!(dirty.canonical_bytes(), clean.canonical_bytes());

    git(&scratch.0, &["checkout", "--detach"]);
    let detached = GitProvenanceAnchor::inspect(&scratch.0).expect("detached anchor");
    assert_eq!(detached.head(), clean.head());
    assert_eq!(detached.head_ref(), None);
    assert_ne!(detached.canonical_bytes(), dirty.canonical_bytes());
}

#[test]
fn identity_is_path_independent_but_working_state_remains_exact() {
    let source = repository("identity-source");
    let clone_parent = Scratch::new("identity-clone");
    let clone = clone_parent.0.join("copy");
    git(
        &clone_parent.0,
        &[
            "clone",
            source.0.to_str().expect("UTF-8 fixture path"),
            clone.to_str().expect("UTF-8 fixture path"),
        ],
    );

    let source_anchor = GitProvenanceAnchor::inspect(&source.0).expect("source anchor");
    let clone_anchor = GitProvenanceAnchor::inspect(&clone).expect("clone anchor");
    assert_eq!(source_anchor.repository(), clone_anchor.repository());
    assert_eq!(source_anchor.head(), clone_anchor.head());
    assert_ne!(
        source_anchor.status_porcelain_v2_z(),
        clone_anchor.status_porcelain_v2_z(),
        "upstream/tracking headers are exact working-copy state"
    );
    assert_ne!(source_anchor.source_root(), clone_anchor.source_root());
}

#[test]
fn records_submodule_gitlinks_without_following_them() {
    let child = repository("submodule-child");
    let parent = repository("submodule-parent");
    git(
        &parent.0,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            child.0.to_str().expect("UTF-8 fixture path"),
            "deps/child",
        ],
    );
    git(&parent.0, &["commit", "-am", "add submodule"]);

    let anchor = GitProvenanceAnchor::inspect(&parent.0).expect("submodule anchor");
    assert_eq!(anchor.gitlinks().len(), 1);
    let gitlink = &anchor.gitlinks()[0];
    assert_eq!(gitlink.stage(), 0);
    assert_eq!(gitlink.path().as_bytes(), b"deps/child");
    let child_head = String::from_utf8(git(&child.0, &["rev-parse", "HEAD"]))
        .expect("object name")
        .trim()
        .to_owned();
    assert_eq!(gitlink.object().as_str(), child_head);
}

#[test]
fn refuses_subdirectories_shallow_history_and_configured_fsmonitor_execution() {
    let source = repository("refusals");
    let nested = source.0.join("nested");
    fs::create_dir(&nested).expect("nested directory");
    assert!(matches!(
        GitProvenanceAnchor::inspect(&nested),
        Err(GitImportError::NotRepositoryRoot)
    ));

    let sentinel = source.0.join("fsmonitor-was-executed");
    let monitor = source.0.join("hostile-fsmonitor");
    fs::write(
        &monitor,
        format!("#!/bin/sh\ntouch '{}'\nexit 1\n", sentinel.display()),
    )
    .expect("monitor script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&monitor, fs::Permissions::from_mode(0o700)).expect("monitor mode");
    }
    git(
        &source.0,
        &[
            "config",
            "core.fsmonitor",
            monitor.to_str().expect("UTF-8 path"),
        ],
    );
    GitProvenanceAnchor::inspect(&source.0).expect("fsmonitor disabled by inspector");
    assert!(!sentinel.exists(), "configured fsmonitor must not execute");

    fs::write(source.0.join("tracked.txt"), b"second\n").expect("second revision");
    git(&source.0, &["commit", "-am", "second"]);
    let shallow_parent = Scratch::new("shallow");
    let shallow = shallow_parent.0.join("copy");
    let source_url = format!("file://{}", source.0.display());
    git(
        &shallow_parent.0,
        &[
            "clone",
            "--depth",
            "1",
            &source_url,
            shallow.to_str().expect("UTF-8 fixture path"),
        ],
    );
    assert!(matches!(
        GitProvenanceAnchor::inspect(&shallow),
        Err(GitImportError::ShallowRepository)
    ));
}

#[test]
fn installs_independent_metadata_while_preserving_exact_dirty_files() {
    let source = repository("install-source");
    let external_diff = Scratch::new("external-diff");
    let external_diff_program = external_diff.0.join("configured-diff");
    let external_diff_sentinel = external_diff.0.join("executed");
    fs::write(
        &external_diff_program,
        format!(
            "#!/bin/sh\ntouch '{}'\nexit 1\n",
            external_diff_sentinel.display()
        ),
    )
    .expect("external diff fixture");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&external_diff_program, fs::Permissions::from_mode(0o700))
            .expect("external diff mode");
    }
    git(
        &source.0,
        &[
            "config",
            "diff.external",
            external_diff_program.to_str().expect("UTF-8 helper path"),
        ],
    );
    fs::write(source.0.join(".gitignore"), b"ignored.log\n").expect("ignore file");
    fs::write(source.0.join("tracked.txt"), b"staged change\n").expect("staged file");
    git(&source.0, &["add", "tracked.txt"]);
    fs::write(source.0.join("tracked.txt"), b"locally changed\n").expect("dirty file");
    fs::write(source.0.join("new.txt"), b"untracked\n").expect("new file");
    fs::write(source.0.join("ignored.log"), b"ignored\n").expect("ignored file");
    let expected = GitProvenanceAnchor::inspect(&source.0).expect("source anchor");

    let destination_parent = Scratch::new("install-destination");
    let destination = destination_parent.0.join("workspace");
    fs::create_dir(&destination).expect("destination");
    let destination = fs::canonicalize(destination).expect("canonical destination");
    for name in [".gitignore", "tracked.txt", "new.txt"] {
        fs::copy(source.0.join(name), destination.join(name)).expect("copy working file");
    }

    let installed = install_independent_git_context(&source.0, &destination, &expected)
        .expect("independent context");
    assert!(
        !external_diff_sentinel.exists(),
        "repository-configured external diff executed during import"
    );
    assert_eq!(installed.git_directory(), destination.join(".git"));
    assert_eq!(installed.anchor().repository(), expected.repository());
    assert_eq!(installed.anchor().head(), expected.head());
    assert_eq!(installed.anchor().head_ref(), expected.head_ref());
    assert_eq!(
        non_ignored_status_records(installed.anchor().status_porcelain_v2_z()),
        non_ignored_status_records(expected.status_porcelain_v2_z()),
        "staged and unstaged state must remain distinguishable after import"
    );
    assert_eq!(
        fs::read(destination.join("tracked.txt")).expect("installed working file"),
        b"locally changed\n"
    );
    assert!(destination.join("new.txt").is_file());
    assert!(
        !destination.join("ignored.log").exists(),
        "Git-ignored working bytes stay outside the managed import"
    );
    assert_eq!(expected.ignored_paths().len(), 1);
    assert!(installed.anchor().ignored_paths().is_empty());

    let source_git = fs::canonicalize(source.0.join(".git")).expect("source git directory");
    let installed_git = fs::canonicalize(destination.join(".git")).expect("installed git");
    assert_ne!(source_git, installed_git);
    assert!(!installed_git.join("objects/info/alternates").exists());
    let source_head = fs::read(source_git.join("HEAD")).expect("source head bytes");
    fs::write(installed_git.join("HEAD"), b"ref: refs/heads/agent\n")
        .expect("mutate independent HEAD");
    assert_eq!(
        fs::read(source_git.join("HEAD")).expect("source remains unchanged"),
        source_head
    );
}

#[test]
fn install_refuses_existing_metadata_changed_source_and_submodules_without_replacement() {
    let source = repository("install-refusals");
    let expected = GitProvenanceAnchor::inspect(&source.0).expect("source anchor");
    let destination_parent = Scratch::new("install-refusals-destination");
    let destination = destination_parent.0.join("workspace");
    fs::create_dir(&destination).expect("destination");
    let destination = fs::canonicalize(destination).expect("canonical destination");
    fs::write(destination.join("tracked.txt"), b"baseline\n").expect("working file");
    fs::write(destination.join(".git"), b"do not replace\n").expect("existing metadata");
    assert!(matches!(
        install_independent_git_context(&source.0, &destination, &expected),
        Err(GitContextError::GitEntryExists)
    ));
    assert_eq!(
        fs::read(destination.join(".git")).expect("retained metadata"),
        b"do not replace\n"
    );
    fs::remove_file(destination.join(".git")).expect("remove fixture metadata");

    fs::write(source.0.join("untracked.txt"), b"arrived later\n").expect("change source");
    assert!(matches!(
        install_independent_git_context(&source.0, &destination, &expected),
        Err(GitContextError::SourceChanged)
    ));
    assert!(!destination.join(".git").exists());

    let child = repository("install-submodule-child");
    let parent = repository("install-submodule-parent");
    git(
        &parent.0,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            child.0.to_str().expect("UTF-8 fixture path"),
            "nested",
        ],
    );
    git(&parent.0, &["commit", "-am", "submodule"]);
    let submodule_anchor = GitProvenanceAnchor::inspect(&parent.0).expect("submodule anchor");
    assert!(matches!(
        install_independent_git_context(&parent.0, &destination, &submodule_anchor),
        Err(GitContextError::SubmodulesUnsupported)
    ));
    assert!(!destination.join(".git").exists());
}

#[test]
fn another_saved_version_gets_history_without_rewriting_its_files() {
    let source = repository("history-source");
    let expected = GitProvenanceAnchor::inspect(&source.0).expect("source anchor");
    let destination_parent = Scratch::new("history-destination");
    let destination = destination_parent.0.join("workspace");
    fs::create_dir(&destination).expect("destination");
    let destination = fs::canonicalize(destination).expect("canonical destination");
    fs::write(destination.join("tracked.txt"), b"earlier Mesh bytes\n").expect("historical file");
    fs::write(destination.join("version-only.txt"), b"retained\n").expect("version file");

    assert!(matches!(
        install_independent_git_context(&source.0, &destination, &expected),
        Err(GitContextError::DestinationMismatch)
    ));
    assert!(!destination.join(".git").exists());

    let installed = install_independent_git_history(&source.0, &destination, &expected)
        .expect("history-only context");
    assert_eq!(installed.anchor().head(), expected.head());
    assert_eq!(
        fs::read(destination.join("tracked.txt")).expect("historical bytes retained"),
        b"earlier Mesh bytes\n"
    );
    assert!(installed
        .anchor()
        .status_porcelain_v2_z()
        .windows(b"version-only.txt".len())
        .any(|window| window == b"version-only.txt"));
}
