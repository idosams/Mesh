use super::*;
use crate::project_attachment::{AttachmentStorage, ObservationLimits, SavedInputDecision};
use ed25519_dalek::{Signer as _, SigningKey};
struct Fixture {
    root: PathBuf,
    a: ProvisionedAttachment,
    key: SigningKey,
    initial: SavedAttachmentVersion,
}
impl Fixture {
    fn new(name: &str, enrolled: bool) -> Self {
        let root =
            std::env::temp_dir().join(format!("mesh-native-capture-{name}-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("source")).unwrap();
        fs::create_dir(root.join("metadata")).unwrap();
        fs::write(root.join("source/note"), b"initial").unwrap();
        let storage = AttachmentStorage::open(&root.join("metadata")).unwrap();
        let a = storage.provision(&root.join("source")).unwrap();
        let key = SigningKey::from_bytes(&[69; 32]);
        let input = a
            .project()
            .capture_inputs(ObservationLimits::default())
            .unwrap();
        let initial = a
            .project()
            .save_capture(a.metadata_path(), &input, public(&key), |p| sign(&key, p))
            .unwrap();
        if enrolled {
            a.enroll_dependency_history().unwrap();
        }
        Self {
            root,
            a,
            key,
            initial,
        }
    }
    fn input(&self, bytes: &[u8]) -> CapturedProjectInput {
        fs::write(self.a.project().root().join("note"), bytes).unwrap();
        self.a
            .project()
            .capture_inputs(ObservationLimits::default())
            .unwrap()
    }
    fn journal(&self) -> Vec<u8> {
        fs::read(self.a.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap()
    }
    fn prepare(&self, input: &CapturedProjectInput, n: u8) -> io::Result<PreparedNativeCapture> {
        self.a
            .prepare_dependency_capture(input, public(&self.key), id(n), |p| sign(&self.key, p))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn id(n: u8) -> RecordDigest {
    RecordDigest::from_bytes([n; 32])
}
fn public(key: &SigningKey) -> PublicKey {
    PublicKey::from_bytes(key.verifying_key().to_bytes())
}
fn sign(key: &SigningKey, p: &SigningPayload) -> io::Result<Signature> {
    Ok(Signature::from_bytes(key.sign(p.as_bytes()).to_bytes()))
}
#[test]
fn enrolled_capture_saves_exact_snapshot_and_keeps_legacy_writers_fenced() {
    let f = Fixture::new("roundtrip", true);
    let input = f.input(b"saved while enrolled");
    let before = f.journal();
    let prepared = f.prepare(&input, 1).unwrap();
    let exact = prepared.operation();
    assert_eq!(f.journal(), before);
    fs::write(f.a.project().root().join("note"), b"later editor bytes").unwrap();
    let saved = prepared.commit().unwrap();
    assert_eq!(saved.operation(), exact);
    assert_eq!(
        fs::read(f.a.project().root().join("note")).unwrap(),
        b"later editor bytes"
    );
    assert_eq!(
        f.a.project()
            .saved_file(f.a.metadata_path(), saved, "note")
            .unwrap()
            .unwrap(),
        b"saved while enrolled"
    );
    assert!(
        OpenWorkspace::open_attachment_store(f.a.metadata_path(), f.a.store.clone(), false)
            .is_err()
    );
    assert!(f
        .a
        .project()
        .save_capture(f.a.metadata_path(), &input, public(&f.key), |p| sign(
            &f.key, p
        ))
        .is_err());
    let second = f
        .prepare(&f.input(b"next private progress"), 2)
        .unwrap()
        .commit()
        .unwrap();
    assert_ne!(saved, second);
    assert_eq!(
        f.a.project()
            .saved_versions(f.a.metadata_path())
            .unwrap()
            .len(),
        3
    );
    assert!(!f.a.metadata_path().join(PENDING).exists());
}
#[test]
fn signer_has_no_custody_and_a_changed_policy_refuses_before_any_capture_write() {
    let f = Fixture::new("signer-race", true);
    let input = f.input(b"prepared version");
    let prepared =
        f.a.prepare_dependency_capture(&input, public(&f.key), id(1), |p| {
            let root = f.a.store.clone();
            assert!(std::thread::spawn(move || {
                crate::workspace_custody::lock_workspace_initialization(&root).is_ok()
            })
            .join()
            .unwrap());
            f.a.decide_saved_input(f.initial, SavedInputDecision::Rejected, None, id(2))
                .unwrap();
            sign(&f.key, p)
        })
        .unwrap();
    let after_policy = f.journal();
    assert!(prepared.commit().is_err());
    assert_eq!(f.journal(), after_policy);
    assert!(!f.a.metadata_path().join(PENDING).exists());
}
#[test]
fn competing_preparations_cannot_overwrite_the_committed_capture_basis() {
    let f = Fixture::new("competing", true);
    let first = f.prepare(&f.input(b"first candidate"), 1).unwrap();
    let second = f.prepare(&f.input(b"second candidate"), 2).unwrap();
    first.commit().unwrap();
    let before = f.journal();
    assert!(second.commit().is_err());
    assert_eq!(f.journal(), before);
    assert_eq!(
        fs::read(f.a.project().root().join("note")).unwrap(),
        b"second candidate"
    );
}
#[test]
fn unenrolled_and_pending_native_state_refuse_without_signing_or_journal_changes() {
    let legacy = Fixture::new("legacy", false);
    let input = legacy.input(b"new");
    assert!(legacy
        .a
        .prepare_dependency_capture(
            &input,
            public(&legacy.key),
            id(1),
            |_| -> io::Result<Signature> { panic!("signer must not run") }
        )
        .is_err());
    let f = Fixture::new("pending", true);
    let input = f.input(b"new");
    let before = f.journal();
    f.a.store
        .filesystem()
        .write_new_file(
            Path::new(PENDING),
            b"unknown retained evidence",
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
    assert!(f
        .a
        .prepare_dependency_capture(
            &input,
            public(&f.key),
            id(1),
            |_| -> io::Result<Signature> { panic!("signer must not run") }
        )
        .is_err());
    assert_eq!(f.journal(), before);
    assert_eq!(
        fs::read(f.a.metadata_path().join(PENDING)).unwrap(),
        b"unknown retained evidence"
    );
}
