//! Ordinary manual/harness lanes fork frozen attached content without redirecting existing work.
#![cfg(unix)]
use ed25519_dalek::{Signer as _, SigningKey};
use mesh_daemon::ipc::Json;
use mesh_daemon::project_attachment::{
    AttachmentStorage, ObservationLimits, ProvisionedAttachment,
};
use std::fs;
use std::io::Write as _;
use std::os::unix::fs::{symlink, PermissionsExt as _};
use std::path::PathBuf;

struct Fixture {
    root: PathBuf,
    source: PathBuf,
    storage: AttachmentStorage,
    history: ProvisionedAttachment,
}
impl Fixture {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "mesh-attachment-lanes-{name}-{}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let source = root.join("source");
        fs::create_dir(&source).unwrap();
        let metadata = root.join("metadata");
        fs::create_dir(&metadata).unwrap();
        let storage = AttachmentStorage::open(&metadata).unwrap();
        let history = storage.provision(&source).unwrap();
        Self {
            root,
            source,
            storage,
            history,
        }
    }
    fn save(&self) -> String {
        let key = SigningKey::from_bytes(&[63; 32]);
        let capture = self
            .history
            .project()
            .capture_inputs(ObservationLimits::default())
            .unwrap();
        self.history
            .project()
            .save_capture(
                self.history.metadata_path(),
                &capture,
                mesh_types::PublicKey::from_bytes(key.verifying_key().to_bytes()),
                |body| {
                    Ok::<_, String>(mesh_types::Signature::from_bytes(
                        key.sign(body.as_bytes()).to_bytes(),
                    ))
                },
            )
            .unwrap()
            .operation()
            .to_string()
    }
    fn open(&self, version: &str, request: &str) -> std::io::Result<ProvisionedAttachment> {
        self.storage.open_version_lane(
            &self.history,
            version,
            request,
            ObservationLimits::default(),
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn independent_lanes_use_saved_bytes_preserve_editor_streams_and_retry_without_overwriting() {
    let f = Fixture::new("independent");
    fs::create_dir_all(f.source.join("folder/empty")).unwrap();
    fs::write(f.source.join("folder/binary"), [0, 255, 1, 2]).unwrap();
    fs::write(f.source.join("run"), "saved\n").unwrap();
    fs::set_permissions(f.source.join("run"), fs::Permissions::from_mode(0o755)).unwrap();
    let mut editor = fs::OpenOptions::new()
        .append(true)
        .open(f.source.join("run"))
        .unwrap();
    let version = f.save();
    let mut git = std::process::Command::new("git");
    assert!(git
        .arg("-C")
        .arg(&f.source)
        .args(["init", "--quiet"])
        .status()
        .unwrap()
        .success());
    assert!(std::process::Command::new("git")
        .arg("-C")
        .arg(&f.source)
        .args(["add", "."])
        .status()
        .unwrap()
        .success());
    let index = fs::read(f.source.join(".git/index")).unwrap();
    let head = fs::read(f.source.join(".git/HEAD")).unwrap();
    editor.write_all(b"original continues\n").unwrap();
    let a = f.open(&version, &"a".repeat(32)).unwrap();
    let b = f.open(&version, &"b".repeat(32)).unwrap();
    assert_ne!(a.id(), b.id());
    assert_ne!(a.project().root(), f.source);
    assert_eq!(
        fs::read(a.project().root().join("run")).unwrap(),
        b"saved\n"
    );
    assert_eq!(
        fs::read(b.project().root().join("folder/binary")).unwrap(),
        [0, 255, 1, 2]
    );
    assert!(a.project().root().join("folder/empty").is_dir());
    assert_eq!(
        fs::metadata(a.project().root().join("run"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert!(!a.project().root().join(".git").exists());
    assert!(!a.metadata_path().starts_with(a.project().root()));
    fs::write(a.project().root().join("run"), "lane A continues\n").unwrap();
    let retry = f.open(&version, &"a".repeat(32)).unwrap();
    assert_eq!(retry.id(), a.id());
    assert_eq!(
        fs::read(retry.project().root().join("run")).unwrap(),
        b"lane A continues\n"
    );
    assert_eq!(
        fs::read(b.project().root().join("run")).unwrap(),
        b"saved\n"
    );
    editor.write_all(b"still open\n").unwrap();
    assert_eq!(
        fs::read(f.source.join("run")).unwrap(),
        b"saved\noriginal continues\nstill open\n"
    );
    assert_eq!(fs::read(f.source.join(".git/index")).unwrap(), index);
    assert_eq!(fs::read(f.source.join(".git/HEAD")).unwrap(), head);
    let origin = f.storage.lane_origin(&a).unwrap().unwrap();
    assert_eq!(
        origin.get("source_project"),
        Some(&Json::text(f.history.id()))
    );
    assert_eq!(origin.get("source_version"), Some(&Json::text(&version)));
    assert_eq!(origin.get("attribution"), Some(&Json::text("unknown")));
    assert_eq!(origin.get("provider"), Some(&Json::Null));
    let reopened = AttachmentStorage::open(&f.root.join("metadata")).unwrap();
    assert_eq!(
        reopened
            .lane_origin(&reopened.reopen(a.id()).unwrap())
            .unwrap(),
        Some(origin)
    );
    let new_version = f.save();
    assert!(f.open(&new_version, &"a".repeat(32)).is_err());
    assert_eq!(
        fs::read(a.project().root().join("run")).unwrap(),
        b"lane A continues\n"
    );
}

#[test]
fn lane_allocation_refuses_unknown_versions_limits_partial_records_and_replaced_folders() {
    let f = Fixture::new("refusals");
    fs::write(f.source.join("work"), "saved").unwrap();
    let version = f.save();
    assert!(f.open(&"f".repeat(64), &"a".repeat(32)).is_err());
    assert!(f.open(&version, "../outside").is_err());
    assert!(f
        .storage
        .open_version_lane(
            &f.history,
            &version,
            &"a".repeat(32),
            ObservationLimits {
                bytes: 1,
                ..ObservationLimits::default()
            }
        )
        .is_err());
    assert!(!f.root.join("metadata/work-lanes").exists());
    let a = f.open(&version, &"a".repeat(32)).unwrap();
    let allocation = a.project().root().parent().unwrap();
    let ready = fs::read(allocation.join("ready.json")).unwrap();
    fs::remove_file(allocation.join("ready.json")).unwrap();
    fs::write(a.project().root().join("work"), "retained partial work").unwrap();
    assert!(f.open(&version, &"a".repeat(32)).is_err());
    assert!(f.storage.lane_origin(&a).is_err());
    assert_eq!(
        fs::read_dir(f.root.join("metadata/work-lanes"))
            .unwrap()
            .count(),
        1
    );
    assert_eq!(
        fs::read(a.project().root().join("work")).unwrap(),
        b"retained partial work"
    );
    fs::write(allocation.join("ready.json"), ready).unwrap();
    let kept = f.root.join("kept-lane");
    fs::rename(a.project().root(), &kept).unwrap();
    symlink(&kept, a.project().root()).unwrap();
    assert!(f.open(&version, &"a".repeat(32)).is_err());
    assert!(f.storage.lane_origin(&a).is_err());
    assert!(a.project().native_folder_reference().is_err());
    assert_eq!(
        fs::read(kept.join("work")).unwrap(),
        b"retained partial work"
    );
}

#[test]
fn concurrent_request_retries_share_one_lane_and_do_not_change_saved_history() {
    let f = Fixture::new("concurrent");
    fs::write(f.source.join("work"), "saved").unwrap();
    let version = f.save();
    let before = f.history.saved_versions().unwrap();
    let (a, b) = std::thread::scope(|scope| {
        let left = scope.spawn(|| f.open(&version, &"d".repeat(32)).unwrap());
        let right = scope.spawn(|| f.open(&version, &"d".repeat(32)).unwrap());
        (left.join().unwrap(), right.join().unwrap())
    });
    assert_eq!(a.id(), b.id());
    assert_eq!(f.history.saved_versions().unwrap(), before);
    assert_eq!(
        fs::read_dir(f.root.join("metadata/work-lanes"))
            .unwrap()
            .count(),
        1
    );
}
