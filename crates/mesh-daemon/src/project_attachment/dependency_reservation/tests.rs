use super::*;
use crate::project_attachment::NativeInputGrantRequest;
use ed25519_dalek::{Signer as _, SigningKey};
use std::path::PathBuf;
struct Fixture {
    root: PathBuf,
    storage: AttachmentStorage,
    owner: ProvisionedAttachment,
    version: SavedAttachmentVersion,
}
impl Fixture {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("mesh-reservation-{name}-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("source")).unwrap();
        fs::create_dir(root.join("metadata")).unwrap();
        fs::write(root.join("source/note"), b"exact saved input").unwrap();
        let storage = AttachmentStorage::open(&root.join("metadata")).unwrap();
        let owner = storage.provision(&root.join("source")).unwrap();
        let input = owner
            .project()
            .capture_inputs(ObservationLimits::default())
            .unwrap();
        let key = SigningKey::from_bytes(&[91; 32]);
        let version = owner
            .project()
            .save_capture(
                owner.metadata_path(),
                &input,
                mesh_types::PublicKey::from_bytes(key.verifying_key().to_bytes()),
                |body| {
                    Ok::<_, &'static str>(mesh_types::Signature::from_bytes(
                        key.sign(body.as_bytes()).to_bytes(),
                    ))
                },
            )
            .unwrap();
        owner.enroll_dependency_history().unwrap();
        Self {
            root,
            storage,
            owner,
            version,
        }
    }
    fn reserve(&self, request: u8) -> io::Result<ProvisionedAttachment> {
        self.storage
            .reserve_dependency_lane(&self.owner, &self.owner, self.version, id(request))
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

#[test]
fn empty_reservation_is_grantable_without_copying_input_or_becoming_a_ready_lane() {
    let f = Fixture::new("grant");
    let before = fs::read(f.owner.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap();
    fs::write(f.root.join("source/note"), b"editor keeps working").unwrap();
    let work = f.reserve(1).unwrap();
    assert!(fs::read_dir(work.project().root())
        .unwrap()
        .next()
        .is_none());
    assert!(work.saved_versions().unwrap().is_empty());
    let binding = f.storage.dependency_work_binding(&f.owner, &work).unwrap();
    assert_ne!(binding.work(), binding.project());
    assert!(f.storage.dependency_work_binding(&work, &work).is_err());
    assert_eq!(
        fs::read(f.owner.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap(),
        before
    );
    let retry = f.reserve(1).unwrap();
    assert_eq!(retry.id(), work.id());
    assert_eq!(
        retry.store.identity().unwrap(),
        work.store.identity().unwrap()
    );
    assert_eq!(f.storage.registrations().unwrap().len(), 2);
    assert!(!work
        .project()
        .root()
        .parent()
        .unwrap()
        .join("ready.json")
        .exists());
    assert!(work
        .project()
        .root()
        .parent()
        .unwrap()
        .join(RESERVED)
        .exists());
    let input = work
        .project()
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    let refused = work.project().save_capture(
        work.metadata_path(),
        &input,
        mesh_types::PublicKey::from_bytes([1; 32]),
        |_| -> Result<mesh_types::Signature, &'static str> {
            panic!("legacy signer must never run")
        },
    );
    assert!(refused.is_err());
    let grant = f
        .storage
        .grant_saved_input(
            &f.owner,
            NativeInputGrantRequest {
                source: &f.owner,
                version: f.version,
                destination: &work,
                allowed: true,
                expected_previous: None,
                request: id(2),
            },
        )
        .unwrap();
    assert_eq!(grant.generation(), 1);
    let revoked = f
        .storage
        .grant_saved_input(
            &f.owner,
            NativeInputGrantRequest {
                source: &f.owner,
                version: f.version,
                destination: &work,
                allowed: false,
                expected_previous: Some(grant.record()),
                request: id(3),
            },
        )
        .unwrap();
    assert_eq!(revoked.generation(), 2);
    assert_eq!(
        f.storage.dependency_work_binding(&f.owner, &work).unwrap(),
        binding
    );
    assert_eq!(
        fs::read(f.root.join("source/note")).unwrap(),
        b"editor keeps working"
    );
}

#[test]
fn interrupted_reservation_recovers_same_physical_destination_without_duplicate_registration() {
    for stop in ["initialized", "fenced", "published", "recorded"] {
        let f = Fixture::new(stop);
        assert!(f
            .storage
            .reserve_with_hook(&f.owner, &f.owner, f.version, id(1), |phase| {
                if phase == stop {
                    Err(io::Error::other("injected lost reply"))
                } else {
                    Ok(())
                }
            })
            .is_err());
        let count = f.storage.registrations().unwrap().len();
        assert_eq!(
            count,
            if matches!(stop, "published" | "recorded") {
                2
            } else {
                1
            }
        );
        let work = f.reserve(1).unwrap();
        let retry = f.reserve(1).unwrap();
        assert_eq!(
            work.store.identity().unwrap(),
            retry.store.identity().unwrap()
        );
        assert_eq!(work.project().root(), retry.project().root());
        assert_eq!(f.storage.registrations().unwrap().len(), 2);
        assert!(fs::read_dir(work.project().root())
            .unwrap()
            .next()
            .is_none());
        assert_eq!(
            fs::read(work.metadata_path().join(crate::RECORD_FILE_NAME))
                .unwrap()
                .len(),
            145
        );
    }
}

#[test]
fn reserved_identity_substitution_and_conflicting_request_preserve_all_evidence() {
    let f = Fixture::new("substitution");
    let work = f.reserve(1).unwrap();
    let allocation = work.project().root().parent().unwrap();
    let before = fs::read(allocation.join(INTENT)).unwrap();
    fs::write(f.root.join("source/note"), b"new private progress").unwrap();
    let input = f
        .owner
        .project()
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    let key = SigningKey::from_bytes(&[91; 32]);
    let changed = f
        .owner
        .prepare_dependency_capture(
            &input,
            mesh_types::PublicKey::from_bytes(key.verifying_key().to_bytes()),
            id(9),
            |body| {
                Ok::<_, &'static str>(mesh_types::Signature::from_bytes(
                    key.sign(body.as_bytes()).to_bytes(),
                ))
            },
        )
        .unwrap()
        .commit()
        .unwrap();
    assert!(f
        .storage
        .reserve_dependency_lane(&f.owner, &f.owner, changed, id(1))
        .is_err());
    assert_eq!(fs::read(allocation.join(INTENT)).unwrap(), before);
    fs::rename(work.project().root(), allocation.join("retained-files")).unwrap();
    fs::create_dir(work.project().root()).unwrap();
    fs::write(work.project().root().join("unknown"), b"preserve me").unwrap();
    assert!(f.reserve(1).is_err());
    assert!(f.storage.dependency_work_binding(&f.owner, &work).is_err());
    assert_eq!(
        fs::read(work.project().root().join("unknown")).unwrap(),
        b"preserve me"
    );
}

#[test]
fn partial_reservation_never_overwrites_editor_work() {
    let f = Fixture::new("partial-editor");
    assert!(f
        .storage
        .reserve_with_hook(&f.owner, &f.owner, f.version, id(1), |phase| {
            if phase == "fenced" {
                Err(io::Error::other("pause before publication"))
            } else {
                Ok(())
            }
        })
        .is_err());
    let allocation = fs::read_dir(f.root.join("metadata/work-lanes"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::write(allocation.join("files/user-work"), b"keep").unwrap();
    assert!(f.reserve(1).is_err());
    assert_eq!(
        fs::read(allocation.join("files/user-work")).unwrap(),
        b"keep"
    );
    assert_eq!(f.storage.registrations().unwrap().len(), 1);
}
