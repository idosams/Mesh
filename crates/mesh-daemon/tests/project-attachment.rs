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

fn save_attached(
    attached: &ProjectAttachment,
    metadata: &std::path::Path,
    input: &mesh_daemon::project_attachment::CapturedProjectInput,
) -> std::io::Result<mesh_daemon::project_attachment::SavedAttachmentVersion> {
    use ed25519_dalek::{Signer as _, SigningKey};
    let key = SigningKey::from_bytes(&[67; 32]);
    attached.save_capture(
        metadata,
        input,
        mesh_types::PublicKey::from_bytes(key.verifying_key().to_bytes()),
        |payload| {
            Ok::<_, &'static str>(mesh_types::Signature::from_bytes(
                key.sign(payload.as_bytes()).to_bytes(),
            ))
        },
    )
}

#[test]
fn attached_history_saves_changes_and_reopens_old_bytes_without_changing_git_or_tools() {
    use ed25519_dalek::{Signer as _, SigningKey};
    let f = Fixture::new("history");
    f.git(&["init", "--quiet"]);
    fs::write(f.source.join("work.txt"), b"staged").unwrap();
    f.git(&["add", "work.txt"]);
    fs::write(f.source.join("work.txt"), b"captured").unwrap();
    fs::create_dir(f.source.join("empty")).unwrap();
    fs::create_dir(f.source.join("nested")).unwrap();
    fs::write(f.source.join("nested/binary"), b"\0\xff").unwrap();
    let index = fs::read(f.source.join(".git/index")).unwrap();
    let head = fs::read(f.source.join(".git/HEAD")).unwrap();
    let attached = ProjectAttachment::register(&f.source, &f.metadata).unwrap();
    let first_input = attached
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    let mut editor = OpenOptions::new()
        .append(true)
        .open(f.source.join("work.txt"))
        .unwrap();
    let key = SigningKey::from_bytes(&[67; 32]);
    let first = attached
        .save_capture(
            &f.metadata,
            &first_input,
            mesh_types::PublicKey::from_bytes(key.verifying_key().to_bytes()),
            |payload| {
                // The source remains writable while the external history commit holds its own serial lock.
                editor.write_all(b" while saving").unwrap();
                Ok::<_, &'static str>(mesh_types::Signature::from_bytes(
                    key.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .unwrap();
    assert_eq!(
        attached
            .saved_file(&f.metadata, first, "work.txt")
            .unwrap()
            .unwrap(),
        b"captured"
    );
    assert_eq!(
        fs::read(f.source.join("work.txt")).unwrap(),
        b"captured while saving"
    );
    fs::remove_file(f.source.join("nested/binary")).unwrap();
    fs::remove_dir(f.source.join("nested")).unwrap();
    fs::write(f.source.join("nested"), b"directory became a file").unwrap();
    fs::create_dir(f.source.join("new-folder")).unwrap();
    fs::write(f.source.join("new-folder/item"), b"new").unwrap();
    let status = f.git(&["status", "--porcelain=v1"]);
    let second = save_attached(
        &attached,
        &f.metadata,
        &attached
            .capture_inputs(ObservationLimits::default())
            .unwrap(),
    )
    .unwrap();
    assert_ne!(first, second);
    drop(attached);
    let reopened = ProjectAttachment::reopen(&f.metadata).unwrap();
    assert_eq!(
        reopened.saved_versions(&f.metadata).unwrap(),
        vec![first, second]
    );
    assert_eq!(
        reopened
            .saved_file(&f.metadata, first, "nested/binary")
            .unwrap()
            .unwrap(),
        b"\0\xff"
    );
    assert!(reopened
        .saved_file(&f.metadata, second, "nested/binary")
        .unwrap()
        .is_none());
    assert_eq!(
        reopened
            .saved_file(&f.metadata, second, "work.txt")
            .unwrap()
            .unwrap(),
        b"captured while saving"
    );
    assert_eq!(
        reopened
            .saved_file(&f.metadata, second, "nested")
            .unwrap()
            .unwrap(),
        b"directory became a file"
    );
    let same = reopened
        .save_capture(
            &f.metadata,
            &reopened
                .capture_inputs(ObservationLimits::default())
                .unwrap(),
            mesh_types::PublicKey::from_bytes(key.verifying_key().to_bytes()),
            |_| -> Result<mesh_types::Signature, &'static str> {
                panic!("unchanged capture must not create another signed version")
            },
        )
        .unwrap();
    assert_eq!(same, second);
    assert_eq!(f.git(&["status", "--porcelain=v1"]), status);
    assert_eq!(fs::read(f.source.join(".git/index")).unwrap(), index);
    assert_eq!(fs::read(f.source.join(".git/HEAD")).unwrap(), head);
    assert!(!f.source.join(".mesh").exists());
    let workspace = mesh_daemon::OpenWorkspace::open(&f.metadata).unwrap();
    assert!(workspace.shared_version().is_none());
    let history = workspace
        .file_histories()
        .iter()
        .find(|h| h.path() == "work.txt")
        .unwrap();
    assert_eq!(history.retained().len(), 2);
}

#[test]
fn attachment_history_refuses_policy_changes_wrong_inputs_and_missing_journal() {
    let f = Fixture::new("history-refusals");
    fs::write(f.source.join("work"), b"first").unwrap();
    let attached = ProjectAttachment::register(&f.source, &f.metadata).unwrap();
    let first = save_attached(
        &attached,
        &f.metadata,
        &attached
            .capture_inputs(ObservationLimits::default())
            .unwrap(),
    )
    .unwrap();
    let journal = f.metadata.join(mesh_daemon::RECORD_FILE_NAME);
    let before = fs::read(&journal).unwrap();
    fs::write(f.source.join(".meshignore"), "work\n").unwrap();
    assert!(save_attached(
        &attached,
        &f.metadata,
        &attached
            .capture_inputs(ObservationLimits::default())
            .unwrap()
    )
    .is_err());
    assert_eq!(fs::read(&journal).unwrap(), before);
    assert_eq!(
        attached
            .saved_file(&f.metadata, first, "work")
            .unwrap()
            .unwrap(),
        b"first"
    );
    let other = Fixture::new("history-other");
    fs::write(other.source.join("work"), b"other").unwrap();
    let other_attached = ProjectAttachment::register(&other.source, &other.metadata).unwrap();
    let other_input = other_attached
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    assert!(save_attached(&attached, &f.metadata, &other_input).is_err());
    assert_eq!(fs::read(&journal).unwrap(), before);
    fs::remove_file(&journal).unwrap();
    assert!(attached.saved_versions(&f.metadata).is_err());
    fs::remove_file(f.source.join(".meshignore")).unwrap();
    assert!(save_attached(
        &attached,
        &f.metadata,
        &attached
            .capture_inputs(ObservationLimits::default())
            .unwrap()
    )
    .is_err());
    assert!(
        !journal.exists(),
        "missing history must not be silently recreated"
    );
}

#[test]
fn history_rechecks_the_source_identity_after_the_signer_returns() {
    use ed25519_dalek::{Signer as _, SigningKey};
    let f = Fixture::new("history-source-replaced");
    fs::write(f.source.join("work"), b"captured").unwrap();
    let attached = ProjectAttachment::register(&f.source, &f.metadata).unwrap();
    let input = attached
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    let key = SigningKey::from_bytes(&[67; 32]);
    let original = f.root.join("original");
    let result = attached.save_capture(
        &f.metadata,
        &input,
        mesh_types::PublicKey::from_bytes(key.verifying_key().to_bytes()),
        |payload| {
            fs::rename(&f.source, &original).unwrap();
            fs::create_dir(&f.source).unwrap();
            fs::write(f.source.join("work"), b"replacement").unwrap();
            Ok::<_, &'static str>(mesh_types::Signature::from_bytes(
                key.sign(payload.as_bytes()).to_bytes(),
            ))
        },
    );
    assert!(result.is_err());
    assert_eq!(fs::read(original.join("work")).unwrap(), b"captured");
    assert_eq!(fs::read(f.source.join("work")).unwrap(), b"replacement");
    assert!(fs::read(f.metadata.join(mesh_daemon::RECORD_FILE_NAME))
        .unwrap()
        .is_empty());
}

#[test]
fn attached_history_retains_empty_versions_and_never_reinitializes_a_copied_binding() {
    let f = Fixture::new("history-empty");
    let attached = ProjectAttachment::register(&f.source, &f.metadata).unwrap();
    assert!(save_attached(
        &attached,
        &f.metadata,
        &attached
            .capture_inputs(ObservationLimits::default())
            .unwrap()
    )
    .is_err());
    fs::write(f.source.join("only"), b"kept in history").unwrap();
    let first = save_attached(
        &attached,
        &f.metadata,
        &attached
            .capture_inputs(ObservationLimits::default())
            .unwrap(),
    )
    .unwrap();
    fs::remove_file(f.source.join("only")).unwrap();
    let empty = save_attached(
        &attached,
        &f.metadata,
        &attached
            .capture_inputs(ObservationLimits::default())
            .unwrap(),
    )
    .unwrap();
    assert_ne!(first, empty);
    assert!(attached
        .saved_file(&f.metadata, empty, "only")
        .unwrap()
        .is_none());
    assert_eq!(
        attached
            .saved_file(&f.metadata, first, "only")
            .unwrap()
            .unwrap(),
        b"kept in history"
    );
    let copied = f.root.join("copied");
    fs::create_dir(&copied).unwrap();
    for name in ["attachment.json", "attachment-history.json"] {
        fs::copy(f.metadata.join(name), copied.join(name)).unwrap();
    }
    assert!(attached.saved_versions(&copied).is_err());
    assert!(!copied.join(mesh_daemon::RECORD_FILE_NAME).exists());
}

#[test]
fn changing_a_history_binding_cannot_relabel_existing_signed_versions() {
    let f = Fixture::new("history-binding");
    fs::write(f.source.join("work"), b"original").unwrap();
    let attached = ProjectAttachment::register(&f.source, &f.metadata).unwrap();
    let first = save_attached(
        &attached,
        &f.metadata,
        &attached
            .capture_inputs(ObservationLimits::default())
            .unwrap(),
    )
    .unwrap();
    let binding_path = f.metadata.join("attachment-history.json");
    let binding = fs::read_to_string(&binding_path).unwrap();
    let parsed = Json::parse(&binding).unwrap();
    let digest = parsed.get("exclusions").unwrap().as_text().unwrap();
    fs::write(&binding_path, binding.replace(digest, &"a".repeat(64))).unwrap();
    assert!(attached.saved_versions(&f.metadata).is_err());
    assert!(attached.saved_file(&f.metadata, first, "work").is_err());
    fs::write(&binding_path, &binding).unwrap();
    assert_eq!(attached.saved_versions(&f.metadata).unwrap(), vec![first]);
}

#[test]
fn concurrent_identical_captures_commit_once_under_external_store_serialization() {
    let f = Fixture::new("history-concurrent");
    fs::write(f.source.join("work"), b"same capture").unwrap();
    let attached = ProjectAttachment::register(&f.source, &f.metadata).unwrap();
    let first = attached
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    let second = attached
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    let barrier = std::sync::Barrier::new(2);
    let versions = std::thread::scope(|scope| {
        let run = |input: mesh_daemon::project_attachment::CapturedProjectInput| {
            let attached = ProjectAttachment::reopen(&f.metadata).unwrap();
            barrier.wait();
            save_attached(&attached, &f.metadata, &input).unwrap()
        };
        let one = scope.spawn(move || run(first));
        let two = scope.spawn(move || run(second));
        (one.join().unwrap(), two.join().unwrap())
    });
    assert_eq!(versions.0, versions.1);
    assert_eq!(
        attached.saved_versions(&f.metadata).unwrap(),
        vec![versions.0]
    );
}

#[test]
fn native_provisioning_is_idempotent_and_preserves_source() {
    use mesh_daemon::project_attachment::AttachmentStorage;
    let f = Fixture::new("provision");
    fs::write(f.source.join("work.txt"), "ordinary work").unwrap();
    let storage = AttachmentStorage::open(&f.metadata).unwrap();
    let first = storage.provision(&f.source).unwrap();
    let again = storage.provision(&f.source).unwrap();
    assert_eq!(first.id(), again.id());
    assert_eq!(first.metadata_path(), again.metadata_path());
    assert_eq!(fs::read_dir(&f.metadata).unwrap().count(), 1);
    assert_eq!(fs::read_dir(&f.source).unwrap().count(), 1);
    assert_eq!(
        fs::metadata(first.metadata_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    ProjectAttachment::reopen(first.metadata_path()).unwrap();
    let inside = f.source.join("internal");
    fs::create_dir(&inside).unwrap();
    assert!(AttachmentStorage::open(&inside)
        .unwrap()
        .provision(&f.source)
        .is_err());
    assert_eq!(fs::read_dir(inside).unwrap().count(), 0);
}

#[test]
fn provisioning_refuses_replaced_roots_links_and_partial_receipts() {
    use mesh_daemon::project_attachment::AttachmentStorage;
    let f = Fixture::new("provision-replacement");
    let storage = AttachmentStorage::open(&f.metadata).unwrap();
    let first = storage.provision(&f.source).unwrap();
    let receipt = first.metadata_path().join("attachment.json");
    fs::remove_file(&receipt).unwrap();
    assert!(storage.provision(&f.source).is_err());
    assert!(!receipt.exists());
    fs::remove_dir(first.metadata_path()).unwrap();
    symlink(&f.source, first.metadata_path()).unwrap();
    assert!(storage.provision(&f.source).is_err());
    assert!(!f.source.join("attachment.json").exists());
    fs::rename(&f.metadata, f.root.join("old-metadata")).unwrap();
    fs::create_dir(&f.metadata).unwrap();
    assert!(storage.provision(&f.source).is_err());
    assert_eq!(fs::read_dir(&f.metadata).unwrap().count(), 0);
}

#[test]
fn saved_inspection_is_paged_exact_and_never_reads_current_source_content() {
    use mesh_daemon::project_attachment::AttachmentStorage;
    let f = Fixture::new("inspection");
    for number in 0..201 {
        fs::write(
            f.source.join(format!("file-{number:03}")),
            format!("saved {number}"),
        )
        .unwrap();
    }
    fs::create_dir(f.source.join("empty")).unwrap();
    fs::write(f.source.join("binary"), b"\0\xff").unwrap();
    fs::write(f.source.join("large"), vec![b'x'; 262_145]).unwrap();
    let history = AttachmentStorage::open(&f.metadata)
        .unwrap()
        .provision(&f.source)
        .unwrap();
    let input = history
        .project()
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    let saved = save_attached(history.project(), history.metadata_path(), &input)
        .unwrap()
        .operation()
        .to_string();
    let first = history.inspect_entries(&saved, None).unwrap();
    let cursor = first.get("next_after").unwrap().as_text().unwrap();
    assert!(
        history
            .inspect_entries(&saved, Some(cursor))
            .unwrap()
            .get("next_after")
            .unwrap()
            == &Json::Null
    );
    fs::write(f.source.join("file-000"), "new live work").unwrap();
    fs::remove_file(f.source.join("binary")).unwrap();
    let text = history.inspect_text(&saved, "file-000").unwrap();
    assert_eq!(text.get("text").and_then(Json::as_text), Some("saved 0"));
    assert_eq!(history.inspect_entries(&saved, None).unwrap(), first);
    assert_eq!(
        history
            .inspect_text(&saved, "binary")
            .unwrap()
            .get("state")
            .and_then(Json::as_text),
        Some("binary")
    );
    assert_eq!(
        history
            .inspect_text(&saved, "large")
            .unwrap()
            .get("state")
            .and_then(Json::as_text),
        Some("too-large")
    );
    assert!(history.inspect_text(&saved, "../outside").is_err());
    assert!(history.inspect_entries(&"f".repeat(64), None).is_err());
    assert!(history.inspect_entries(&saved, Some("missing")).is_err());
    fs::rename(history.metadata_path(), f.root.join("old-history")).unwrap();
    fs::create_dir(history.metadata_path()).unwrap();
    assert!(history.inspect_text(&saved, "file-000").is_err());
}

#[test]
fn saved_comparison_tracks_content_modes_types_and_pinned_pages() {
    use mesh_daemon::project_attachment::AttachmentStorage;
    let f = Fixture::new("comparison");
    for path in ["stable", "content", "mode", "removed", "type"] {
        fs::write(f.source.join(path), "before").unwrap();
        fs::set_permissions(f.source.join(path), fs::Permissions::from_mode(0o600)).unwrap();
    }
    let history = AttachmentStorage::open(&f.metadata)
        .unwrap()
        .provision(&f.source)
        .unwrap();
    let save = || {
        let input = history
            .project()
            .capture_inputs(ObservationLimits::default())
            .unwrap();
        save_attached(history.project(), history.metadata_path(), &input)
            .unwrap()
            .operation()
            .to_string()
    };
    let base = save();
    fs::write(f.source.join("content"), "after").unwrap();
    fs::set_permissions(f.source.join("mode"), fs::Permissions::from_mode(0o700)).unwrap();
    fs::rename(f.source.join("removed"), f.source.join("added")).unwrap();
    fs::remove_file(f.source.join("type")).unwrap();
    fs::create_dir(f.source.join("type")).unwrap();
    for number in 0..201 {
        fs::write(f.source.join(format!("new-{number:03}")), b"\0\xff").unwrap();
    }
    let target = save();
    history.inspect_entries(&target, None).unwrap();
    assert_eq!(
        history
            .inspect_text(&target, "content")
            .unwrap()
            .get("text")
            .and_then(Json::as_text),
        Some("after")
    );
    let first = history.compare_versions(&base, &target, None).unwrap();
    let cursor = first.get("next_after").unwrap().as_text().unwrap();
    let second = history
        .compare_versions(&base, &target, Some(cursor))
        .unwrap();
    let Json::Array(first_changes) = first.get("changes").unwrap() else {
        panic!("changes");
    };
    let Json::Array(second_changes) = second.get("changes").unwrap() else {
        panic!("changes");
    };
    let exact = history.comparison_path(&base, &target, "removed").unwrap();
    assert_eq!(exact.get("total"), Some(&Json::Number(1)));
    assert_eq!(exact.get("after"), Some(&Json::Null));
    assert_eq!(exact.get("next_after"), Some(&Json::Null));
    assert!(history.comparison_path(&base, &target, "stable").is_err());
    assert!(history
        .comparison_path(&base, &target, "../outside")
        .is_err());
    assert!(history
        .comparison_path(&base, &"f".repeat(64), "removed")
        .is_err());
    assert_eq!(first_changes.len(), 200);
    let changes: Vec<_> = first_changes.iter().chain(second_changes.iter()).collect();
    for (path, kind) in [
        ("content", "modified"),
        ("mode", "mode-changed"),
        ("removed", "removed"),
        ("added", "added"),
        ("type", "type-changed"),
    ] {
        let change = changes
            .iter()
            .find(|change| change.get("path").and_then(Json::as_text) == Some(path))
            .unwrap();
        assert_eq!(change.get("change").and_then(Json::as_text), Some(kind));
    }
    assert!(!changes
        .iter()
        .any(|change| change.get("path").and_then(Json::as_text) == Some("stable")));
    assert_eq!(
        history
            .compare_versions(&base, &base, None)
            .unwrap()
            .get("total"),
        Some(&Json::Number(0))
    );
    fs::write(f.source.join("content"), "still newer work").unwrap();
    save();
    assert_eq!(
        history.compare_versions(&base, &target, None).unwrap(),
        first
    );
    assert_eq!(
        history
            .compare_versions(&base, &target, Some(cursor))
            .unwrap(),
        second
    );
    assert!(history
        .compare_versions(&base, &target, Some("stable"))
        .is_err());
    assert!(history
        .compare_versions(&base, &"f".repeat(64), None)
        .is_err());
}

#[test]
fn catalog_discovers_offline_registrations_without_adopting_replacements() {
    use mesh_daemon::project_attachment::AttachmentStorage;
    let f = Fixture::new("catalog-offline");
    let storage = AttachmentStorage::open(&f.metadata).unwrap();
    let project = storage.provision(&f.source).unwrap();
    let id = project.id().to_owned();
    fs::rename(&f.source, f.root.join("original")).unwrap();
    let found = storage.registrations().unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id(), id);
    assert_eq!(found[0].root(), project.project().root());
    assert!(storage.reopen(&id).is_err());
    fs::create_dir(&f.source).unwrap();
    assert!(storage.reopen(&id).is_err());
    assert!(fs::read_dir(&f.source).unwrap().next().is_none());
    fs::remove_dir(&f.source).unwrap();
    fs::rename(f.root.join("original"), &f.source).unwrap();
    assert_eq!(storage.reopen(&id).unwrap().id(), id);
    assert!(storage.reopen("../project").is_err());
    fs::rename(project.metadata_path(), f.root.join("old-store")).unwrap();
    symlink(&f.source, project.metadata_path()).unwrap();
    assert!(storage.registrations().is_err());
    assert!(storage.reopen(&id).is_err());
}

#[test]
fn catalog_preserves_partial_and_modified_registration_evidence() {
    use mesh_daemon::project_attachment::AttachmentStorage;
    let f = Fixture::new("catalog-damage");
    let storage = AttachmentStorage::open(&f.metadata).unwrap();
    let project = storage.provision(&f.source).unwrap();
    let receipt = project.metadata_path().join("attachment.json");
    let original = fs::read(&receipt).unwrap();
    fs::write(&receipt, b"{}").unwrap();
    assert!(storage.registrations().is_err());
    assert_eq!(fs::read(&receipt).unwrap(), b"{}");
    fs::write(&receipt, original).unwrap();
    assert_eq!(storage.registrations().unwrap().len(), 1);
    fs::remove_file(&receipt).unwrap();
    assert!(storage.registrations().is_err());
    assert!(!receipt.exists());
}

fn pin_selector(key: &str) -> mesh_daemon::project_attachment::AttachmentPin {
    mesh_daemon::project_attachment::AttachmentPin {
        key: key.into(),
        project: "a".repeat(64),
        base: "b".repeat(64),
        target: "c".repeat(64),
        after: Some("docs/readme.md".into()),
        path: Some("src/main.rs".into()),
    }
}

#[test]
fn pin_snapshot_roundtrips_with_revision_and_persists_last_pin_removal() {
    use mesh_daemon::project_attachment::AttachmentStorage;
    let f = Fixture::new("pins-roundtrip");
    let storage = AttachmentStorage::open(&f.metadata).unwrap();
    assert_eq!(storage.load_comparison_pins().unwrap().revision, 0);
    let first = storage
        .save_comparison_pins(0, vec![pin_selector("1"), pin_selector("2")])
        .unwrap();
    assert_eq!(first.revision, 1);
    let restarted = AttachmentStorage::open(&f.metadata).unwrap();
    assert_eq!(restarted.load_comparison_pins().unwrap(), first);
    assert_eq!(
        restarted
            .save_comparison_pins(1, first.pins.clone())
            .unwrap(),
        first
    );
    assert!(restarted.save_comparison_pins(0, vec![]).is_err());
    let removed = restarted.save_comparison_pins(1, vec![]).unwrap();
    assert_eq!(removed.revision, 2);
    assert!(AttachmentStorage::open(&f.metadata)
        .unwrap()
        .load_comparison_pins()
        .unwrap()
        .pins
        .is_empty());
    assert_eq!(fs::read_dir(&f.source).unwrap().count(), 0);
    assert_eq!(
        fs::metadata(f.metadata.join("desktop-comparison-pins.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(storage.registrations().unwrap().is_empty());
}

#[test]
fn pin_snapshots_refuse_corruption_links_copies_and_incomplete_first_write() {
    use mesh_daemon::project_attachment::AttachmentStorage;
    let f = Fixture::new("pins-refusal");
    let storage = AttachmentStorage::open(&f.metadata).unwrap();
    storage
        .save_comparison_pins(0, vec![pin_selector("1")])
        .unwrap();
    let record = f.metadata.join("desktop-comparison-pins.json");
    let original = fs::read(&record).unwrap();
    let copy = f.root.join("copy");
    fs::create_dir(&copy).unwrap();
    fs::write(copy.join("desktop-comparison-pins.json"), &original).unwrap();
    fs::set_permissions(
        copy.join("desktop-comparison-pins.json"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    assert!(AttachmentStorage::open(&copy)
        .unwrap()
        .load_comparison_pins()
        .is_err());
    let staged = f.metadata.join("desktop-comparison-pins.pending");
    fs::write(&staged, b"interrupted next snapshot").unwrap();
    assert!(storage.save_comparison_pins(1, vec![]).is_err());
    assert_eq!(
        storage.load_comparison_pins().unwrap().pins,
        vec![pin_selector("1")]
    );
    assert_eq!(fs::read(&record).unwrap(), original);
    assert_eq!(fs::read(&staged).unwrap(), b"interrupted next snapshot");
    fs::remove_file(staged).unwrap();
    fs::write(&record, b"{}").unwrap();
    assert!(storage.load_comparison_pins().is_err());
    assert!(storage.save_comparison_pins(1, vec![]).is_err());
    assert_eq!(fs::read(&record).unwrap(), b"{}");
    fs::remove_file(&record).unwrap();
    let outside = f.root.join("outside");
    fs::write(&outside, &original).unwrap();
    symlink(&outside, &record).unwrap();
    assert!(storage.load_comparison_pins().is_err());
    assert!(storage.save_comparison_pins(1, vec![]).is_err());
    assert_eq!(fs::read(&outside).unwrap(), original);
    fs::remove_file(&record).unwrap();
    let pending = f.metadata.join("desktop-comparison-pins.pending");
    fs::write(&pending, b"interrupted snapshot").unwrap();
    assert!(storage.load_comparison_pins().is_err());
    assert!(storage
        .save_comparison_pins(0, vec![pin_selector("1")])
        .is_err());
    assert_eq!(fs::read(pending).unwrap(), b"interrupted snapshot");
}

#[test]
fn concurrent_pin_updates_have_one_winner_and_invalid_selectors_never_publish() {
    use mesh_daemon::project_attachment::AttachmentStorage;
    use std::sync::{Arc, Barrier};
    let f = Fixture::new("pins-concurrent");
    let barrier = Arc::new(Barrier::new(2));
    let workers: Vec<_> = ["1", "2"]
        .into_iter()
        .map(|key| {
            let barrier = barrier.clone();
            let metadata = f.metadata.clone();
            std::thread::spawn(move || {
                let storage = AttachmentStorage::open(&metadata).unwrap();
                barrier.wait();
                storage.save_comparison_pins(0, vec![pin_selector(key)])
            })
        })
        .collect();
    let results: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let storage = AttachmentStorage::open(&f.metadata).unwrap();
    let before = storage.load_comparison_pins().unwrap();
    assert_eq!(before.revision, 1);
    let mut invalid = pin_selector("3");
    invalid.path = Some("../outside".into());
    assert!(storage.save_comparison_pins(1, vec![invalid]).is_err());
    assert!(storage
        .save_comparison_pins(1, vec![pin_selector("3"); 9])
        .is_err());
    assert!(storage
        .save_comparison_pins(1, vec![pin_selector("3"); 2])
        .is_err());
    assert_eq!(storage.load_comparison_pins().unwrap(), before);
    fs::rename(&f.metadata, f.root.join("old-catalog")).unwrap();
    fs::create_dir(&f.metadata).unwrap();
    assert!(storage.save_comparison_pins(1, vec![]).is_err());
    assert!(fs::read_dir(&f.metadata).unwrap().next().is_none());
}

#[test]
fn pin_ui_projection_roundtrips_without_content_and_refuses_ambiguous_fields() {
    use mesh_daemon::project_attachment::AttachmentPinState;
    let state = AttachmentPinState {
        revision: u64::MAX,
        pins: vec![pin_selector("1")],
    };
    let encoded = state.to_json().encode();
    assert_eq!(
        AttachmentPinState::parse_projection(&encoded).unwrap(),
        state
    );
    assert!(AttachmentPinState::parse_projection(
        &encoded.replace("18446744073709551615", "18446744073709551616")
    )
    .is_err());
    assert!(
        AttachmentPinState::parse_projection(&encoded.replace("18446744073709551615", "01"))
            .is_err()
    );
    assert!(
        AttachmentPinState::parse_projection(&encoded.replacen("{", "{\"extra\":null,", 1))
            .is_err()
    );
    let mut invalid = state;
    invalid.pins[0].path = Some("../outside".into());
    assert!(AttachmentPinState::parse_projection(&invalid.to_json().encode()).is_err());
}
