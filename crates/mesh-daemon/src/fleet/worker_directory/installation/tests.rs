use super::*;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "mesh-install-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, Permissions::from_mode(0o700)).unwrap();
        Self(root)
    }
    fn token(&self) -> ProtectedWorkspaceRoot {
        ProtectedWorkspaceRoot::inspect(&self.0).unwrap()
    }
    fn provision(&self) -> NativeWorkerInstallation {
        NativeWorkerInstallation::provision(&self.0, self.token(), &[], |_| {
            Ok((PublicKey::from_bytes([0xab; 32]), ()))
        })
        .unwrap()
        .0
    }
    fn reopen(&self) -> io::Result<NativeWorkerInstallation> {
        NativeWorkerInstallation::reopen(&self.0, self.token(), &[], |_, key| {
            if key != PublicKey::from_bytes([0xab; 32]) {
                return Err(unavailable());
            }
            Ok(())
        })
        .map(|x| x.0)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn registry(installation: &NativeWorkerInstallation) -> RemoteAdmissionRegistry {
    installation
        .registry(
            &"cd".repeat(32),
            "objective",
            Limits {
                lanes: 1,
                concurrency: 1,
                depth: 0,
                retries: 0,
            },
        )
        .unwrap()
}
#[test]
fn provisioning_binds_identity_and_preserves_locks_through_retained_registries() {
    let f = Fixture::new();
    let installation = f.provision();
    let identity = installation.identity().unwrap();
    assert!(f.reopen().is_err());
    let registry = registry(&installation);
    drop(installation);
    assert!(
        f.reopen().is_err(),
        "registry retains parent and ledger locks"
    );
    drop(registry);
    let reopened = f.reopen().unwrap();
    assert_eq!(reopened.identity().unwrap(), identity);
    drop(reopened);
    assert!(
        NativeWorkerInstallation::provision::<()>(&f.0, f.token(), &[], |_| panic!(
            "must not create another key"
        ))
        .is_err()
    );
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 3);
}
#[test]
fn failed_custody_retains_intent_and_never_retries_or_repairs_partial_state() {
    let f = Fixture::new();
    assert!(NativeWorkerInstallation::provision::<()>(
        &f.0,
        f.token(),
        &[],
        |_| Err(unavailable())
    )
    .is_err());
    let retained = fs::read(f.0.join(INTENT)).unwrap();
    assert!(!retained.is_empty());
    assert!(
        NativeWorkerInstallation::provision::<()>(&f.0, f.token(), &[], |_| panic!("no retry"))
            .is_err()
    );
    assert!(
        NativeWorkerInstallation::reopen::<()>(&f.0, f.token(), &[], |_, _| panic!(
            "no load from incomplete state"
        ))
        .is_err()
    );
    assert_eq!(fs::read(f.0.join(INTENT)).unwrap(), retained);
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 1);
}
#[test]
fn parent_receipt_replacement_revokes_retained_ledger_authority() {
    let f = Fixture::new();
    let installation = f.provision();
    let mut registry = registry(&installation);
    let retained = Fixture::new();
    let path = f.0.join(IDENTITY);
    let bytes = fs::read(&path).unwrap();
    fs::rename(&path, retained.0.join("retained-identity")).unwrap();
    fs::write(&path, bytes).unwrap();
    fs::set_permissions(&path, Permissions::from_mode(0o600)).unwrap();
    assert!(installation.verify().is_err());
    let work = crate::fleet::RemoteWork {
        lane: "lane".into(),
        run: "run".into(),
        provider: "codex".into(),
        goal: "Task".into(),
        assignment: crate::fleet::RemoteAssignment {
            id: "assignment".into(),
            worker_key: "ab".repeat(32),
            input: RecordDigest::from_bytes([1; 32]),
            bundle: RecordDigest::from_bytes([2; 32]),
            lease_sequence: 1,
            lease_until_ms: 1000,
        },
    };
    assert!(registry
        .reserve(work, "0123456789abcdef0123456789abcdef", 100)
        .is_err());
    assert!(retained.0.join("retained-identity").exists());
}
#[test]
fn changed_provision_intent_and_protected_root_refuse_before_ledger_creation() {
    let f = Fixture::new();
    assert!(
        NativeWorkerInstallation::provision::<()>(&f.0, f.token(), &[f.token()], |_| panic!(
            "protected root"
        ))
        .is_err()
    );
    assert!(fs::read_dir(&f.0).unwrap().next().is_none());
    assert!(
        NativeWorkerInstallation::provision(&f.0, f.token(), &[], |_| {
            fs::write(f.0.join(INTENT), b"substituted").unwrap();
            Ok((PublicKey::from_bytes([0xab; 32]), ()))
        })
        .is_err()
    );
    assert_eq!(fs::read(f.0.join(INTENT)).unwrap(), b"substituted");
    assert!(!f.0.join(LEDGER).exists());
}
#[test]
fn unavailable_custody_on_reopen_preserves_complete_installation() {
    let f = Fixture::new();
    drop(f.provision());
    let receipt = fs::read(f.0.join(IDENTITY)).unwrap();
    assert!(NativeWorkerInstallation::reopen::<()>(
        &f.0,
        f.token(),
        &[],
        |_, _| Err(unavailable())
    )
    .is_err());
    assert_eq!(fs::read(f.0.join(IDENTITY)).unwrap(), receipt);
    assert!(f.reopen().is_ok());
}

#[test]
fn replaced_namespace_and_noncanonical_receipt_refuse_without_repair() {
    let f = Fixture::new();
    let retained = Fixture::new();
    let installation = f.provision();
    fs::rename(&f.0, retained.0.join("old")).unwrap();
    fs::create_dir(&f.0).unwrap();
    fs::set_permissions(&f.0, Permissions::from_mode(0o700)).unwrap();
    assert!(installation.identity().is_err());
    assert!(fs::read_dir(&f.0).unwrap().next().is_none());
    drop(installation);
    let f = Fixture::new();
    drop(f.provision());
    let path = f.0.join(IDENTITY);
    let bytes = fs::read_to_string(&path).unwrap();
    fs::write(&path, format!("{bytes} ")).unwrap();
    assert!(
        NativeWorkerInstallation::reopen::<()>(&f.0, f.token(), &[], |_, _| panic!(
            "noncanonical receipt"
        ))
        .is_err()
    );
    assert_eq!(fs::read_to_string(path).unwrap(), format!("{bytes} "));
}

#[test]
fn identity_and_reopen_check_ledger_after_custody_callback() {
    let f = Fixture::new();
    let retained = Fixture::new();
    drop(f.provision());
    let database = f.0.join(LEDGER).join(DATABASE);
    assert!(
        NativeWorkerInstallation::reopen(&f.0, f.token(), &[], |_, _| {
            fs::rename(&database, retained.0.join("old-database")).unwrap();
            fs::write(&database, b"unrelated").unwrap();
            fs::set_permissions(&database, Permissions::from_mode(0o600)).unwrap();
            Ok(())
        })
        .is_err()
    );
    assert_eq!(fs::read(&database).unwrap(), b"unrelated");
    assert!(retained.0.join("old-database").exists());
}
