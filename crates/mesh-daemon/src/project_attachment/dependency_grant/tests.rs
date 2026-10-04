use super::*;
use crate::project_attachment::ObservationLimits;
use ed25519_dalek::{Signer as _, SigningKey};
use std::{fs, io::Write as _, path::PathBuf};
struct Fixture {
    root: PathBuf,
    storage: AttachmentStorage,
    owner: ProvisionedAttachment,
    source: ProvisionedAttachment,
    destination: ProvisionedAttachment,
    version: SavedAttachmentVersion,
}
fn save(history: &ProvisionedAttachment) -> SavedAttachmentVersion {
    let key = SigningKey::from_bytes(&[67; 32]);
    let capture = history
        .project()
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    history
        .project()
        .save_capture(
            history.metadata_path(),
            &capture,
            mesh_types::PublicKey::from_bytes(key.verifying_key().to_bytes()),
            |payload| {
                Ok::<_, &'static str>(mesh_types::Signature::from_bytes(
                    key.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .unwrap()
}
fn id(n: u8) -> RecordDigest {
    RecordDigest::from_bytes([n; 32])
}
impl Fixture {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("mesh-native-grant-{name}-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("source")).unwrap();
        fs::create_dir(root.join("metadata")).unwrap();
        fs::write(root.join("source/note"), b"root source").unwrap();
        let storage = AttachmentStorage::open(&root.join("metadata")).unwrap();
        let owner = storage.provision(&root.join("source")).unwrap();
        let root_version = save(&owner);
        let source = storage
            .open_version_lane(
                &owner,
                &root_version.operation().to_string(),
                &"01".repeat(16),
                ObservationLimits::default(),
            )
            .unwrap();
        let destination = storage
            .open_version_lane(
                &owner,
                &root_version.operation().to_string(),
                &"02".repeat(16),
                ObservationLimits::default(),
            )
            .unwrap();
        fs::write(
            source.project().root().join("note"),
            b"private source version",
        )
        .unwrap();
        let version = save(&source);
        owner.enroll_dependency_history().unwrap();
        Self {
            root,
            storage,
            owner,
            source,
            destination,
            version,
        }
    }
    fn request(
        &self,
        allowed: bool,
        previous: Option<RecordDigest>,
        request: RecordDigest,
    ) -> NativeInputGrantRequest<'_> {
        NativeInputGrantRequest {
            source: &self.source,
            version: self.version,
            destination: &self.destination,
            allowed,
            expected_previous: previous,
            request,
        }
    }
    fn journal(&self) -> PathBuf {
        self.owner.metadata_path().join(crate::RECORD_FILE_NAME)
    }
    fn grant(
        &self,
        allowed: bool,
        previous: Option<RecordDigest>,
        request: RecordDigest,
    ) -> io::Result<NativeInputGrant> {
        self.storage
            .grant_saved_input(&self.owner, self.request(allowed, previous, request))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn grant_revoke_regrant_and_historical_retry_preserve_exact_input_and_child_histories() {
    let f = Fixture::new("progression");
    let source_before = fs::read(f.source.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap();
    let original = fs::read(f.journal()).unwrap();
    let grant = f.grant(true, None, id(1)).unwrap();
    assert_eq!(grant.generation(), 1);
    let revoked = f.grant(false, Some(grant.record()), id(2)).unwrap();
    assert_eq!(revoked.generation(), 2);
    let after_revoke = fs::read(f.journal()).unwrap();
    assert_eq!(f.grant(true, None, id(1)).unwrap(), grant);
    assert_eq!(fs::read(f.journal()).unwrap(), after_revoke);
    let renewed = f.grant(true, Some(revoked.record()), id(3)).unwrap();
    assert_eq!(renewed.generation(), 3);
    assert_eq!(
        fs::read(f.journal()).unwrap().len(),
        original.len() + 3 * 145
    );
    fs::write(
        f.source.project().root().join("note"),
        b"later ordinary editor work",
    )
    .unwrap();
    let storage = AttachmentStorage::open(&f.root.join("metadata")).unwrap();
    let owner = storage.reopen(f.owner.id()).unwrap();
    let source = storage.reopen(f.source.id()).unwrap();
    let destination = storage.reopen(f.destination.id()).unwrap();
    assert_eq!(
        storage
            .grant_saved_input(
                &owner,
                NativeInputGrantRequest {
                    source: &source,
                    destination: &destination,
                    ..f.request(true, Some(revoked.record()), id(3))
                }
            )
            .unwrap(),
        renewed
    );
    assert_eq!(
        fs::read(source.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap(),
        source_before
    );
    assert!(!destination
        .metadata_path()
        .join(crate::RECORD_FILE_NAME)
        .exists());
    assert!(owner
        .validate_lane_version(&owner.saved_versions().unwrap()[0].operation().to_string())
        .is_err());
}

#[test]
fn grant_refuses_foreign_self_stale_and_conflicting_requests_without_append() {
    let f = Fixture::new("refusal");
    let first = f.grant(true, None, id(1)).unwrap();
    let before = fs::read(f.journal()).unwrap();
    assert!(f.grant(false, None, id(1)).is_err());
    assert!(f.grant(true, None, id(2)).is_err());
    assert!(f.grant(true, Some(id(99)), id(2)).is_err());
    assert!(f.grant(true, Some(first.record()), ZERO).is_err());
    assert!(f
        .storage
        .grant_saved_input(
            &f.owner,
            NativeInputGrantRequest {
                destination: &f.source,
                ..f.request(true, None, id(3))
            }
        )
        .is_err());
    let foreign = Fixture::new("foreign");
    assert!(f
        .storage
        .grant_saved_input(
            &f.owner,
            NativeInputGrantRequest {
                destination: &foreign.destination,
                ..f.request(true, None, id(4))
            }
        )
        .is_err());
    assert!(f
        .storage
        .grant_saved_input(
            &f.owner,
            NativeInputGrantRequest {
                version: foreign.version,
                ..f.request(true, None, id(5))
            }
        )
        .is_err());
    assert_eq!(fs::read(f.journal()).unwrap(), before);
}

#[test]
fn every_interrupted_grant_frame_prefix_recovers_only_its_exact_request() {
    for length in 0..=145 {
        let f = Fixture::new(&format!("prefix-{length}"));
        let before = fs::read(f.journal()).unwrap();
        let failed = f.storage.grant_with_io(
            &f.owner,
            f.request(true, None, id(1)),
            |step, file, frame| {
                if matches!(step, Step::Staged) {
                    assert_eq!(frame.len(), 145);
                    file.write_all(&frame[..length])?;
                    file.sync_all()?;
                    return Err(io::Error::other("injected interrupted grant append"));
                }
                Ok(())
            },
            |file| file.sync_all(),
        );
        assert!(failed.is_err());
        let partial = fs::read(f.journal()).unwrap();
        assert_eq!(partial.len(), before.len() + length);
        let facts = f
            .owner
            .inspect_pending_dependency_control_retention(id(1))
            .unwrap();
        assert_eq!(
            facts.get("written_frame_bytes").and_then(Json::as_u64),
            Some(length as u64)
        );
        assert_eq!(fs::read(f.journal()).unwrap(), partial);
        assert!(f
            .owner
            .inspect_pending_dependency_control_retention(id(2))
            .is_err());

        assert!(f.grant(false, None, id(1)).is_err());
        assert!(f.grant(true, None, id(2)).is_err());
        assert_eq!(fs::read(f.journal()).unwrap(), partial);
        let recovered = f.grant(true, None, id(1)).unwrap();
        assert_eq!(recovered.generation(), 1);
        let complete = fs::read(f.journal()).unwrap();
        assert_eq!(complete.len(), before.len() + 145);
        assert_eq!(f.grant(true, None, id(1)).unwrap(), recovered);
        assert_eq!(fs::read(f.journal()).unwrap(), complete);
    }
}

#[test]
fn grant_sync_failure_and_lost_acknowledgement_require_exact_durable_recovery() {
    for mode in ["sync", "ack"] {
        let f = Fixture::new(mode);
        let before = fs::read(f.journal()).unwrap();
        let failed = f.storage.grant_with_io(
            &f.owner,
            f.request(true, None, id(1)),
            |step, _, _| {
                if mode == "ack" && matches!(step, Step::Appended) {
                    Err(io::Error::other("lost acknowledgement"))
                } else {
                    Ok(())
                }
            },
            |file| {
                if mode == "sync" {
                    Err(io::Error::other("injected sync failure"))
                } else {
                    file.sync_all()
                }
            },
        );
        assert!(failed.is_err(), "{mode} must not acknowledge");
        let appended = fs::read(f.journal()).unwrap();
        assert_eq!(appended.len(), before.len() + 145);
        let receipt = f.grant(true, None, id(1)).unwrap();
        assert_eq!(f.grant(true, None, id(1)).unwrap(), receipt);
        assert_eq!(fs::read(f.journal()).unwrap(), appended);
    }
}

#[test]
fn grant_rechecks_native_destination_after_staging_and_preserves_changed_receipt() {
    let f = Fixture::new("changed-ready");
    let before = fs::read(f.journal()).unwrap();
    let ready = f
        .destination
        .project()
        .root()
        .parent()
        .unwrap()
        .join("ready.json");
    let original = fs::read(&ready).unwrap();
    let failed = f.storage.grant_with_io(
        &f.owner,
        f.request(true, None, id(1)),
        |step, _, _| {
            if matches!(step, Step::Staged) {
                fs::write(&ready, b"unknown replacement evidence")?;
            }
            Ok(())
        },
        |file| file.sync_all(),
    );
    assert!(failed.is_err());
    assert_eq!(fs::read(f.journal()).unwrap(), before);
    assert_eq!(fs::read(&ready).unwrap(), b"unknown replacement evidence");
    fs::write(&ready, original).unwrap();
    assert!(f.grant(true, None, id(1)).is_ok());
}

#[test]
fn grant_retry_refuses_replaced_container_with_same_stable_work_and_installation() {
    use mesh_types::{Blake3, ContentDigest as _};
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
    fn set(value: &mut Json, name: &str, replacement: Json) {
        let Json::Object(fields) = value else {
            panic!("object")
        };
        fields.iter_mut().find(|(key, _)| key == name).unwrap().1 = replacement;
    }
    let f = Fixture::new("container-replacement");
    let child = &f.destination;
    let grant = f.grant(true, None, id(1)).unwrap();
    let before = fs::read(f.journal()).unwrap();
    let original = f.storage.dependency_work_binding(&f.owner, child).unwrap();
    let allocation = child.project().root().parent().unwrap().to_path_buf();
    let retained = f.root.join("retained-allocation");
    fs::rename(&allocation, &retained).unwrap();
    fs::create_dir(&allocation).unwrap();
    fs::rename(retained.join("files"), allocation.join("files")).unwrap();
    let mut intent =
        Json::parse(&fs::read_to_string(retained.join("intent.json")).unwrap()).unwrap();
    let metadata = fs::metadata(&allocation).unwrap();
    set(
        &mut intent,
        "allocation_device",
        Json::text(format!("{:016x}", metadata.dev())),
    );
    set(
        &mut intent,
        "allocation_inode",
        Json::text(format!("{:016x}", metadata.ino())),
    );
    let encoded = intent.encode();
    let mut ready = Json::parse(&fs::read_to_string(retained.join("ready.json")).unwrap()).unwrap();
    set(
        &mut ready,
        "intent_digest",
        Json::text(Blake3::digest_bytes(encoded.as_bytes()).to_string()),
    );
    for (name, bytes) in [("intent.json", encoded), ("ready.json", ready.encode())] {
        let path = allocation.join(name);
        fs::write(&path, bytes).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let refreshed = f.storage.dependency_work_binding(&f.owner, child).unwrap();
    assert_eq!(refreshed.work(), original.work());
    assert_eq!(refreshed.installation(), original.installation());
    assert_ne!(
        refreshed, original,
        "same work/installation does not erase changed native allocation evidence"
    );
    assert!(original.revalidate(&f.storage, &f.owner, child).is_err());
    assert!(f.grant(true, None, id(1)).is_err());
    assert_eq!(fs::read(f.journal()).unwrap(), before);
    let new = f.grant(true, Some(grant.record()), id(2)).unwrap();
    assert_eq!(new.generation(), 2);
}
