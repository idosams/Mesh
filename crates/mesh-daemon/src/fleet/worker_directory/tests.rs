use super::*;
use crate::fleet::{RemoteAdmissionOutcome, RemoteAssignment, RemoteWork};
use std::fs;
use std::os::unix::fs::symlink;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
const ALLOCATION: &str = "0123456789abcdef0123456789abcdef";
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "mesh-native-worker-ledger-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        for name in ["wrapper", "wrapper/worker", "project"] {
            fs::create_dir(root.join(name)).unwrap();
            fs::set_permissions(root.join(name), Permissions::from_mode(0o700)).unwrap();
        }
        Self(root)
    }
    fn path(&self) -> PathBuf {
        self.0.join("wrapper/worker")
    }
    fn token(&self) -> ProtectedWorkspaceRoot {
        ProtectedWorkspaceRoot::inspect(&self.path()).unwrap()
    }
    fn protected(&self) -> Vec<ProtectedWorkspaceRoot> {
        vec![ProtectedWorkspaceRoot::inspect(&self.0.join("project")).unwrap()]
    }
    fn create(&self) -> NativeRemoteWorkerDirectory {
        NativeRemoteWorkerDirectory::create(
            &self.path(),
            self.token(),
            &"ab".repeat(32),
            &self.protected(),
        )
        .unwrap()
    }
    fn reopen(&self) -> io::Result<NativeRemoteWorkerDirectory> {
        NativeRemoteWorkerDirectory::reopen(
            &self.path(),
            self.token(),
            &"ab".repeat(32),
            &self.protected(),
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn limits() -> Limits {
    Limits {
        lanes: 2,
        concurrency: 2,
        depth: 0,
        retries: 0,
    }
}
fn registry(directory: &NativeRemoteWorkerDirectory) -> RemoteAdmissionRegistry {
    directory
        .registry(&"cd".repeat(32), "objective", limits())
        .unwrap()
}
fn work() -> RemoteWork {
    RemoteWork {
        lane: "lane".into(),
        run: "run".into(),
        provider: "codex".into(),
        goal: "Task".into(),
        assignment: RemoteAssignment {
            id: "assignment".into(),
            worker_key: "ab".repeat(32),
            input: RecordDigest::from_bytes([1; 32]),
            bundle: RecordDigest::from_bytes([2; 32]),
            lease_sequence: 1,
            lease_until_ms: 1000,
        },
    }
}

#[test]
fn shared_guarded_registry_replays_after_restart_and_retains_exclusive_owner() {
    let fixture = Fixture::new();
    let directory = fixture.create();
    let mut first = registry(&directory);
    let mut second = registry(&directory);
    assert!(matches!(
        first.reserve(work(), ALLOCATION, 100),
        Ok(RemoteAdmissionOutcome::Reserved(_))
    ));
    assert!(matches!(
        second.reserve(work(), ALLOCATION, 100),
        Ok(RemoteAdmissionOutcome::Retained(_))
    ));
    drop(directory);
    assert!(
        fixture.reopen().is_err(),
        "live registries must keep directory ownership"
    );
    drop(first);
    assert!(fixture.reopen().is_err());
    drop(second);
    let reopened = fixture.reopen().unwrap();
    assert!(matches!(
        registry(&reopened).reserve(work(), ALLOCATION, 2000),
        Ok(RemoteAdmissionOutcome::Retained(_))
    ));
    assert!(NativeRemoteWorkerDirectory::create(
        &fixture.path(),
        fixture.token(),
        &"ab".repeat(32),
        &fixture.protected()
    )
    .is_err());
}

#[test]
fn namespace_replacement_and_database_substitution_refuse_without_writing_replacement() {
    let fixture = Fixture::new();
    let directory = fixture.create();
    let mut current = registry(&directory);
    fs::rename(fixture.path(), fixture.0.join("old-worker")).unwrap();
    fs::create_dir(fixture.path()).unwrap();
    fs::set_permissions(fixture.path(), Permissions::from_mode(0o700)).unwrap();
    assert!(current.reserve(work(), ALLOCATION, 100).is_err());
    assert!(fs::read_dir(fixture.path()).unwrap().next().is_none());
    assert!(fixture.0.join("old-worker/worker.sqlite").exists());

    let fixture = Fixture::new();
    let directory = fixture.create();
    let mut current = registry(&directory);
    let database = fixture.path().join(DATABASE);
    fs::rename(&database, fixture.path().join("retained-database")).unwrap();
    fs::write(&database, b"unrelated content").unwrap();
    fs::set_permissions(&database, Permissions::from_mode(0o600)).unwrap();
    assert!(current.reserve(work(), ALLOCATION, 100).is_err());
    assert_eq!(fs::read(database).unwrap(), b"unrelated content");
}

#[test]
fn missing_and_partial_provisioning_never_recreates_history() {
    let fixture = Fixture::new();
    assert!(fixture.reopen().is_err());
    assert!(fs::read_dir(fixture.path()).unwrap().next().is_none());
    // Retained state before the physical receipt was written: neither reopen nor create repairs it.
    fs::write(fixture.path().join(DATABASE), []).unwrap();
    fs::set_permissions(fixture.path().join(DATABASE), Permissions::from_mode(0o600)).unwrap();
    assert!(fixture.reopen().is_err());
    assert!(NativeRemoteWorkerDirectory::create(
        &fixture.path(),
        fixture.token(),
        &"ab".repeat(32),
        &fixture.protected()
    )
    .is_err());
    assert_eq!(
        fs::metadata(fixture.path().join(DATABASE)).unwrap().len(),
        0
    );

    let fixture = Fixture::new();
    drop(fixture.create());
    let receipt = fs::read(fixture.path().join(RECEIPT)).unwrap();
    fs::remove_file(fixture.path().join(DATABASE)).unwrap();
    assert!(fixture.reopen().is_err());
    assert!(!fixture.path().join(DATABASE).exists());
    assert_eq!(fs::read(fixture.path().join(RECEIPT)).unwrap(), receipt);
}

#[test]
fn abrupt_owner_helper() {
    let Some(root) = std::env::var_os("MESH_WORKER_LEDGER_CRASH_FIXTURE") else {
        return;
    };
    let fixture = Fixture(PathBuf::from(root));
    let directory = fixture.create();
    let mut registry = registry(&directory);
    assert!(matches!(
        registry.reserve(work(), ALLOCATION, 100),
        Ok(RemoteAdmissionOutcome::Reserved(_))
    ));
    // Deliberately skip every Rust destructor, including SQLite close/checkpoint and Owner drop.
    std::process::exit(73);
}

#[test]
fn abrupt_owner_exit_releases_os_lock_but_never_regrants_committed_admission() {
    let fixture = Fixture::new();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "fleet::worker_directory::tests::abrupt_owner_helper",
            "--nocapture",
        ])
        .env("MESH_WORKER_LEDGER_CRASH_FIXTURE", &fixture.0)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(73),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let directory = fixture.reopen().unwrap();
    let mut registry = registry(&directory);
    assert_eq!(registry.receipts().unwrap().len(), 1);
    assert!(matches!(
        registry.reserve(work(), ALLOCATION, 100),
        Ok(RemoteAdmissionOutcome::Retained(_))
    ));
}

#[test]
fn wrong_key_format_permissions_links_and_extra_entries_refuse() {
    let fixture = Fixture::new();
    drop(fixture.create());
    let receipt = fs::read(fixture.path().join(RECEIPT)).unwrap();
    assert!(NativeRemoteWorkerDirectory::reopen(
        &fixture.path(),
        fixture.token(),
        &"ef".repeat(32),
        &fixture.protected()
    )
    .is_err());
    assert_eq!(fs::read(fixture.path().join(RECEIPT)).unwrap(), receipt);
    fs::set_permissions(fixture.path(), Permissions::from_mode(0o755)).unwrap();
    assert!(fixture.reopen().is_err());
    fs::set_permissions(fixture.path(), Permissions::from_mode(0o700)).unwrap();
    fs::write(
        fixture.path().join(RECEIPT),
        String::from_utf8(receipt.clone())
            .unwrap()
            .replace("directory/v1", "directory/v2"),
    )
    .unwrap();
    assert!(fixture.reopen().is_err());
    fs::write(fixture.path().join(RECEIPT), receipt).unwrap();
    drop(fixture.reopen().unwrap());
    let external = fixture.0.join("external");
    fs::write(&external, b"external content").unwrap();
    fs::set_permissions(&external, Permissions::from_mode(0o600)).unwrap();
    for hard in [false, true] {
        let path = fixture.path().join("worker.sqlite-journal");
        if hard {
            fs::hard_link(&external, &path).unwrap();
        } else {
            symlink(&external, &path).unwrap();
        }
        assert!(fixture.reopen().is_err());
        assert_eq!(fs::read(&external).unwrap(), b"external content");
        fs::remove_file(path).unwrap();
    }
    fs::write(fixture.path().join("unknown"), b"preserve").unwrap();
    assert!(fixture.reopen().is_err());
    assert_eq!(
        fs::read(fixture.path().join("unknown")).unwrap(),
        b"preserve"
    );
}

#[test]
fn protected_project_admission_and_ancestor_alias_movement_refuse() {
    let fixture = Fixture::new();
    let project = fixture.0.join("project");
    assert!(NativeRemoteWorkerDirectory::create(
        &project,
        ProtectedWorkspaceRoot::inspect(&project).unwrap(),
        &"ab".repeat(32),
        &fixture.protected()
    )
    .is_err());
    assert!(fs::read_dir(&project).unwrap().next().is_none());
    let directory = fixture.create();
    let mut registry = registry(&directory);
    fs::rename(fixture.0.join("wrapper"), project.join("moved")).unwrap();
    symlink(project.join("moved"), fixture.0.join("wrapper")).unwrap();
    assert_eq!(fixture.token(), directory.authority.owner.token);
    assert!(registry.reserve(work(), ALLOCATION, 100).is_err());
    assert!(project.join("moved/worker/worker.sqlite").exists());
}
