//! Non-exclusive registration preserves existing project and Git state across process restarts.
#![cfg(unix)]

use mesh_daemon::ipc::Json;
use mesh_daemon::project_attachment::ObservationLimits;
use mesh_daemon::project_attachment::ProjectAttachment;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::os::unix::fs::{symlink, MetadataExt as _, PermissionsExt as _};
use std::path::PathBuf;
use std::process::Command;

struct Fixture {
    root: PathBuf,
    source: PathBuf,
    metadata: PathBuf,
}
impl Fixture {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("mesh-attachment-{name}-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let source = root.join("project");
        let metadata = root.join("metadata");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&metadata).unwrap();
        Self {
            root,
            source,
            metadata,
        }
    }
    fn git(&self, args: &[&str]) -> Vec<u8> {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.source)
            .args(args)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .output()
            .unwrap();
        assert!(output.status.success());
        output.stdout
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn attaches_to_dirty_git_project_without_changing_work_or_interrupting_open_file() {
    let f = Fixture::new("dirty");
    f.git(&["init", "--quiet"]);
    fs::write(f.source.join("work.txt"), "staged\n").unwrap();
    f.git(&["add", "work.txt"]);
    fs::write(f.source.join("work.txt"), "editing\n").unwrap();
    fs::write(f.source.join("untracked.txt"), "untracked\n").unwrap();
    let status = f.git(&["status", "--porcelain=v1"]);
    let index = fs::read(f.source.join(".git/index")).unwrap();
    let head = fs::read(f.source.join(".git/HEAD")).unwrap();
    let identity = fs::metadata(&f.source).unwrap().ino();
    let mut editor = OpenOptions::new()
        .append(true)
        .open(f.source.join("work.txt"))
        .unwrap();
    let attached = ProjectAttachment::register(&f.source, &f.metadata).unwrap();
    assert_eq!(attached.root(), f.source.canonicalize().unwrap());
    assert_eq!(f.git(&["status", "--porcelain=v1"]), status);
    assert_eq!(fs::read(f.source.join(".git/index")).unwrap(), index);
    assert_eq!(fs::read(f.source.join(".git/HEAD")).unwrap(), head);
    assert_eq!(fs::metadata(&f.source).unwrap().ino(), identity);
    assert!(!f.source.join(".mesh").exists());
    assert!(!f.source.join(".codex").exists());
    editor
        .write_all(b"continued in the same session\n")
        .unwrap();
    assert_eq!(
        fs::read_to_string(f.source.join("work.txt")).unwrap(),
        "editing\ncontinued in the same session\n"
    );
    let receipt = fs::read(f.metadata.join("attachment.json")).unwrap();
    drop(attached);
    ProjectAttachment::reopen(&f.metadata).unwrap();
    ProjectAttachment::register(&f.source, &f.metadata).unwrap();
    assert_eq!(
        fs::read(f.metadata.join("attachment.json")).unwrap(),
        receipt
    );
}

#[test]
fn project_replacement_and_conflicting_registration_preserve_both_projects() {
    let f = Fixture::new("replacement");
    let attached = ProjectAttachment::register(&f.source, &f.metadata).unwrap();
    let old = f.root.join("original");
    fs::rename(&f.source, &old).unwrap();
    fs::create_dir(&f.source).unwrap();
    fs::write(f.source.join("new.txt"), "replacement").unwrap();
    assert!(attached.ensure_current().is_err());
    assert!(ProjectAttachment::reopen(&f.metadata).is_err());
    let receipt = fs::read(f.metadata.join("attachment.json")).unwrap();
    assert!(ProjectAttachment::register(&f.source, &f.metadata).is_err());
    assert_eq!(
        fs::read(f.metadata.join("attachment.json")).unwrap(),
        receipt
    );
    assert_eq!(
        fs::read_to_string(f.source.join("new.txt")).unwrap(),
        "replacement"
    );
    assert!(old.is_dir());
}

#[test]
fn metadata_inside_project_and_symlink_receipts_are_refused() {
    let f = Fixture::new("confinement");
    let inside = f.source.join("metadata");
    fs::create_dir(&inside).unwrap();
    assert!(ProjectAttachment::register(&f.source, &inside).is_err());
    assert!(!inside.join("attachment.json").exists());
    let alias = f.root.join("alias");
    symlink(&f.source, &alias).unwrap();
    assert!(ProjectAttachment::register(&alias, &f.metadata).is_err());
    let elsewhere = f.root.join("unrelated");
    fs::write(&elsewhere, "unchanged").unwrap();
    symlink(&elsewhere, f.metadata.join("attachment.json")).unwrap();
    assert!(ProjectAttachment::register(&f.source, &f.metadata).is_err());
    assert!(ProjectAttachment::reopen(&f.metadata).is_err());
    assert_eq!(fs::read_to_string(elsewhere).unwrap(), "unchanged");
}

#[test]
fn incompatible_and_oversized_receipts_are_preserved_and_refused() {
    let f = Fixture::new("receipt");
    let path = f.metadata.join("attachment.json");
    for bytes in [
        b"{\"schema\":\"mesh.project-attachment/v99\"}".to_vec(),
        vec![b'x'; 65_537],
    ] {
        fs::write(&path, &bytes).unwrap();
        assert!(ProjectAttachment::reopen(&f.metadata).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn cli_registers_and_reopens_without_a_running_daemon() {
    let f = Fixture::new("cli");
    fs::write(f.source.join("note.txt"), "existing work").unwrap();
    let attached = Command::new(env!("CARGO_BIN_EXE_meshctl"))
        .arg("attach")
        .arg(&f.source)
        .arg(&f.metadata)
        .output()
        .unwrap();
    assert!(
        attached.status.success(),
        "{}",
        String::from_utf8_lossy(&attached.stderr)
    );
    let reopened = Command::new(env!("CARGO_BIN_EXE_meshctl"))
        .arg("attachment-status")
        .arg(&f.metadata)
        .output()
        .unwrap();
    assert!(reopened.status.success());
    assert_eq!(attached.stdout, reopened.stdout);
    let json = mesh_daemon::ipc::Json::parse(std::str::from_utf8(&attached.stdout).unwrap().trim())
        .unwrap();
    assert_eq!(
        json.get("observation")
            .and_then(mesh_daemon::ipc::Json::as_text),
        Some("not-started")
    );
    assert_eq!(
        fs::read_to_string(f.source.join("note.txt")).unwrap(),
        "existing work"
    );
}

#[test]
fn repeated_observations_preserve_git_and_apply_exclusions_without_claiming_a_version() {
    let f = Fixture::new("observe");
    f.git(&["init", "--quiet"]);
    fs::write(f.source.join("note.txt"), "first").unwrap();
    fs::write(f.source.join(".gitignore"), "ignored.txt\n").unwrap();
    fs::write(f.source.join("ignored.txt"), "excluded content").unwrap();
    let git = f.git(&["status", "--porcelain=v1"]);
    let attached = ProjectAttachment::register(&f.source, &f.metadata).unwrap();
    let first = attached.observe(ObservationLimits::default()).unwrap();
    assert_eq!(
        first,
        attached.observe(ObservationLimits::default()).unwrap(),
        "scans must not share directory cursor state"
    );
    assert_eq!(first.get("complete"), Some(&Json::Bool(true)));
    assert_eq!(first.get("atomic_snapshot"), Some(&Json::Bool(false)));
    assert_eq!(first.get("saved_version"), Some(&Json::Null));
    let files = first.get("files").unwrap().as_array().unwrap();
    assert_eq!(files.len(), 2);
    assert!(files
        .iter()
        .all(|file| file.get("attribution").and_then(Json::as_text) == Some("unknown")));
    assert_eq!(f.git(&["status", "--porcelain=v1"]), git);
    fs::write(f.source.join("note.txt"), "second").unwrap();
    let next = attached.observe(ObservationLimits::default()).unwrap();
    let digest = |report: &Json| {
        report
            .get("files")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f.get("path").and_then(Json::as_text) == Some("note.txt"))
            .unwrap()
            .get("digest")
            .unwrap()
            .clone()
    };
    assert_ne!(digest(&first), digest(&next));
    let output = Command::new(env!("CARGO_BIN_EXE_meshctl"))
        .arg("attachment-observe")
        .arg(&f.metadata)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        Json::parse(std::str::from_utf8(&output.stdout).unwrap().trim()).unwrap(),
        next
    );
}

#[test]
fn observation_reports_limits_and_unsupported_links_without_reading_outside() {
    let f = Fixture::new("observe-bounds");
    fs::create_dir(f.source.join(".GIT")).unwrap();
    fs::write(f.source.join(".GIT/internal"), "git metadata").unwrap();
    fs::write(f.source.join("large.bin"), [1_u8; 32]).unwrap();
    fs::write(f.root.join("outside.txt"), "outside source").unwrap();
    symlink(f.root.join("outside.txt"), f.source.join("escape")).unwrap();
    let attached = ProjectAttachment::register(&f.source, &f.metadata).unwrap();
    let observed = attached
        .observe(ObservationLimits {
            entries: 10,
            bytes: 8,
            file_bytes: 8,
        })
        .unwrap();
    assert_eq!(observed.get("complete"), Some(&Json::Bool(false)));
    assert!(observed
        .get("files")
        .unwrap()
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(observed.get("bytes_read").and_then(Json::as_u64), Some(0));
    assert_eq!(observed.get("issues").unwrap().as_array().unwrap().len(), 2);
    let limited = attached
        .observe(ObservationLimits {
            entries: 1,
            ..ObservationLimits::default()
        })
        .unwrap();
    assert_eq!(limited.get("complete"), Some(&Json::Bool(false)));
    assert_eq!(
        fs::read_to_string(f.root.join("outside.txt")).unwrap(),
        "outside source"
    );
    assert!(attached
        .observe(ObservationLimits {
            entries: 100_001,
            ..ObservationLimits::default()
        })
        .is_err());
}

#[test]
fn captured_inputs_keep_exact_bytes_and_modes_after_the_user_continues_editing() {
    use mesh_types::{Blake3, ContentDigest as _};
    let f = Fixture::new("capture-inputs");
    let path = f.source.join("script.sh");
    let original = b"private-capture-content\0binary\n";
    fs::write(&path, original).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    fs::create_dir(f.source.join("empty-directory")).unwrap();
    let attached = ProjectAttachment::register(&f.source, &f.metadata).unwrap();
    let captured = attached
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    assert_eq!(captured.root(), f.source.canonicalize().unwrap());
    assert_eq!(
        captured.identity().1,
        fs::metadata(&f.source).unwrap().ino()
    );
    assert_eq!(captured.directories(), [PathBuf::from("empty-directory")]);
    assert_eq!(captured.files().len(), 1);
    let file = &captured.files()[0];
    assert_eq!(file.path(), std::path::Path::new("script.sh"));
    assert_eq!(file.identity().1, fs::metadata(&path).unwrap().ino());
    assert_eq!(file.bytes(), original);
    assert_eq!(file.digest(), Blake3::digest_bytes(original));
    assert!(file.executable());
    assert!(!format!("{captured:?} {file:?}").contains("private-capture-content"));
    assert!(!attached
        .observe(ObservationLimits::default())
        .unwrap()
        .encode()
        .contains("private-capture-content"));
    fs::write(&path, "later user edit").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(file.bytes(), original);
    assert!(file.executable());
    let next = attached
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    assert_eq!(next.files()[0].bytes(), b"later user edit");
    assert!(!next.files()[0].executable());
    assert_eq!(fs::read_to_string(&path).unwrap(), "later user edit");
    assert_eq!(
        fs::read_dir(&f.metadata).unwrap().count(),
        1,
        "in-memory capture cannot claim a durable version"
    );
}

#[test]
fn incomplete_project_cannot_become_a_capture_input() {
    let f = Fixture::new("capture-refusal");
    fs::write(f.source.join("small.txt"), "small").unwrap();
    fs::write(f.source.join("large.txt"), [1_u8; 64]).unwrap();
    let attached = ProjectAttachment::register(&f.source, &f.metadata).unwrap();
    assert!(attached
        .capture_inputs(ObservationLimits {
            bytes: 32,
            file_bytes: 32,
            entries: 10
        })
        .is_err());
    fs::remove_file(f.source.join("large.txt")).unwrap();
    symlink(f.root.join("absent"), f.source.join("unsupported")).unwrap();
    assert!(attached
        .capture_inputs(ObservationLimits::default())
        .is_err());
    assert_eq!(
        fs::read_to_string(f.source.join("small.txt")).unwrap(),
        "small"
    );
}

#[test]
fn captured_exclusion_policy_survives_later_ignore_changes() {
    let f = Fixture::new("capture-policy");
    fs::write(f.source.join(".gitignore"), "ignored.txt\n").unwrap();
    fs::write(f.source.join("ignored.txt"), "still exists").unwrap();
    let attached = ProjectAttachment::register(&f.source, &f.metadata).unwrap();
    let first = attached
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    fs::write(f.source.join(".gitignore"), "different.txt\n").unwrap();
    let next = attached
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    assert_eq!(first.exclusion_rules(), (Some("ignored.txt\n"), None));
    assert_ne!(first.exclusion_digest(), next.exclusion_digest());
    assert!(!first
        .files()
        .iter()
        .any(|file| file.path() == std::path::Path::new("ignored.txt")));
    assert!(next
        .files()
        .iter()
        .any(|file| file.path() == std::path::Path::new("ignored.txt")));
}
