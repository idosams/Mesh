use super::*;
use crate::ipc::Json;
use crate::project_attachment::{
    NativeInputGrant, NativeInputGrantRequest, NativeWorkDecisionRequest, ObservationLimits,
    SavedInputDecision,
};
use ed25519_dalek::{Signer as _, SigningKey};
use std::{fs, path::PathBuf};
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
        let root = std::env::temp_dir().join(format!(
            "mesh-native-grant-admission-{name}-{}",
            std::process::id()
        ));
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

impl Fixture {
    fn inspect<T>(
        &self,
        grant: RecordDigest,
        read: impl FnOnce(NativeGrantedInput<'_>) -> io::Result<T>,
    ) -> io::Result<T> {
        self.storage.with_current_input_grant(
            &self.owner,
            NativeGrantInspection {
                source: &self.source,
                version: self.version,
                destination: &self.destination,
                grant,
            },
            read,
        )
    }
}

#[test]
fn current_grant_reads_exact_immutable_bytes_and_eligibility_does_not_broaden_access() {
    let f = Fixture::new("immutable");
    let grant = f.grant(true, None, id(1)).unwrap();
    f.storage
        .decide_work_input(
            &f.owner,
            NativeWorkDecisionRequest {
                source: &f.source,
                version: f.version,
                decision: SavedInputDecision::Rejected,
                expected_previous: None,
                request: id(2),
            },
        )
        .unwrap();
    fs::write(f.source.project().root().join("note"), b"new editor bytes").unwrap();
    let before = fs::read(f.journal()).unwrap();
    let data = f
        .inspect(grant.record(), |view| {
            assert_eq!(
                view.files()
                    .map(|file| file.path.to_owned())
                    .collect::<Vec<_>>(),
                vec!["note"]
            );
            assert!(view.directories().next().is_none());
            let mut bytes = Vec::new();
            view.write_file("note", &mut bytes)?;
            assert!(view.write_file("../note", &mut Vec::new()).is_err());
            assert!(view
                .history
                .promote_approval_receipt(b"not a writer".to_vec())
                .is_err());
            Ok(bytes)
        })
        .unwrap();
    assert_eq!(data, b"private source version");
    assert_eq!(fs::read(f.journal()).unwrap(), before);
    assert_eq!(
        fs::read(f.source.project().root().join("note")).unwrap(),
        b"new editor bytes"
    );
}

#[test]
fn revoked_stale_foreign_and_wrong_destination_grants_never_enter_callback() {
    let f = Fixture::new("revoked");
    let first = f.grant(true, None, id(1)).unwrap();
    let revoked = f.grant(false, Some(first.record()), id(2)).unwrap();
    let denied = || -> io::Result<()> { panic!("denied grant entered callback") };
    assert!(f.inspect(first.record(), |_| denied()).is_err());
    assert!(f.inspect(revoked.record(), |_| denied()).is_err());
    let renewed = f.grant(true, Some(revoked.record()), id(3)).unwrap();
    assert!(f.inspect(first.record(), |_| denied()).is_err());
    assert!(f.inspect(id(99), |_| denied()).is_err());
    assert!(f
        .storage
        .with_current_input_grant(
            &f.owner,
            NativeGrantInspection {
                source: &f.source,
                version: f.version,
                destination: &f.source,
                grant: renewed.record(),
            },
            |_| denied()
        )
        .is_err());
    assert!(f.inspect(renewed.record(), |_| Ok(())).is_ok());
}

#[test]
fn unfinished_control_refuses_admission_without_repair_or_callback() {
    use std::os::unix::fs::PermissionsExt as _;
    let f = Fixture::new("pending");
    let grant = f.grant(true, None, id(1)).unwrap();
    let pending = f.owner.metadata_path().join(PENDING);
    fs::write(&pending, b"retained unfinished control").unwrap();
    fs::set_permissions(&pending, fs::Permissions::from_mode(0o600)).unwrap();
    let before = fs::read(f.journal()).unwrap();
    assert!(f
        .inspect(grant.record(), |_| -> io::Result<()> {
            panic!("unfinished control admitted")
        })
        .is_err());
    assert_eq!(fs::read(pending).unwrap(), b"retained unfinished control");
    assert_eq!(fs::read(f.journal()).unwrap(), before);
}

#[test]
fn callback_keeps_custody_and_changed_ancestry_refuses_acknowledgement() {
    let f = Fixture::new("custody");
    let grant = f.grant(true, None, id(1)).unwrap();
    let before = fs::read(f.journal()).unwrap();
    let ready = f
        .destination
        .project()
        .root()
        .parent()
        .unwrap()
        .join("ready.json");
    let original = fs::read(&ready).unwrap();
    let result = f.inspect(grant.record(), |_| {
        assert!(
            f.grant(false, Some(grant.record()), id(2)).is_err(),
            "callback cannot extend its held custody set"
        );
        fs::write(&ready, b"changed during inspection")?;
        Ok(())
    });
    assert!(result.is_err());
    assert_eq!(fs::read(&ready).unwrap(), b"changed during inspection");
    assert_eq!(fs::read(f.journal()).unwrap(), before);
    fs::write(&ready, original).unwrap();
    assert!(
        f.grant(false, Some(grant.record()), id(2)).is_ok(),
        "custody releases after refusal"
    );
}

#[test]
fn current_admission_refuses_replaced_container_until_new_native_grant() {
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
    assert!(f
        .inspect(grant.record(), |_| -> io::Result<()> {
            panic!("replaced binding admitted old grant")
        })
        .is_err());
    let new = f.grant(true, Some(grant.record()), id(2)).unwrap();
    assert_eq!(new.generation(), 2);
    assert!(f.inspect(new.record(), |_| Ok(())).is_ok());
}

#[test]
fn native_owner_input_uses_the_exact_read_only_owner_snapshot() {
    let f = Fixture::new("root");
    let version = f.owner.saved_versions().unwrap()[0];
    let grant = f
        .storage
        .grant_saved_input(
            &f.owner,
            NativeInputGrantRequest {
                source: &f.owner,
                version,
                destination: &f.destination,
                allowed: true,
                expected_previous: None,
                request: id(7),
            },
        )
        .unwrap();
    let bytes = f
        .storage
        .with_current_input_grant(
            &f.owner,
            NativeGrantInspection {
                source: &f.owner,
                version,
                destination: &f.destination,
                grant: grant.record(),
            },
            |view| {
                let mut bytes = Vec::new();
                view.write_file("note", &mut bytes)?;
                assert!(view
                    .history
                    .promote_approval_receipt(b"root stays read only".to_vec())
                    .is_err());
                Ok(bytes)
            },
        )
        .unwrap();
    assert_eq!(bytes, b"root source");
}

#[test]
fn prepared_grant_requires_complete_held_custody_and_refreshes_permission() {
    let f = Fixture::new("prepared-custody");
    let grant = f.grant(true, None, id(1)).unwrap();
    let prepared = f
        .storage
        .prepare_input_grant(
            &f.owner,
            NativeGrantInspection {
                source: &f.source,
                version: f.version,
                destination: &f.destination,
                grant: grant.record(),
            },
        )
        .unwrap();
    {
        let incomplete =
            crate::workspace_custody::lock_workspace_initialization(&f.owner.store).unwrap();
        assert!(f
            .storage
            .with_prepared_input_grant(&prepared, &incomplete, |_| -> io::Result<()> {
                panic!("incomplete custody entered callback")
            })
            .is_err());
    }
    {
        let guard =
            crate::workspace_custody::lock_workspace_initialization_set(&prepared.roots).unwrap();
        assert!(f
            .inspect(grant.record(), |_| -> io::Result<()> {
                panic!("nested public acquisition entered callback")
            })
            .is_err());
        let bytes = f
            .storage
            .with_prepared_input_grant(&prepared, &guard, |view| {
                let mut bytes = Vec::new();
                view.write_file("note", &mut bytes)?;
                Ok(bytes)
            })
            .unwrap();
        assert_eq!(bytes, b"private source version");
        guard.ensure_current().unwrap();
    }
    f.grant(false, Some(grant.record()), id(2)).unwrap();
    let guard =
        crate::workspace_custody::lock_workspace_initialization_set(&prepared.roots).unwrap();
    assert!(f
        .storage
        .with_prepared_input_grant(&prepared, &guard, |_| -> io::Result<()> {
            panic!("revoked prepared grant entered callback")
        })
        .is_err());
    guard.ensure_current().unwrap();
}

#[test]
fn granted_initial_snapshot_preparation_uses_saved_bytes_and_has_explicit_root() {
    use mesh_operations::{ActorId, ObjectId, Operation, WorkspaceId};
    let f = Fixture::new("starting-snapshot");
    let grant = f.grant(true, None, id(1)).unwrap();
    fs::write(
        f.source.project().root().join("note"),
        b"later unsaved bytes",
    )
    .unwrap();
    let prepare = |limits| {
        f.inspect(grant.record(), |view| {
            crate::project_attachment::consumption_start::prepare_initial_snapshot(
                &view,
                WorkspaceId::from_bytes([7; 16]),
                ActorId::from_bytes([8; 32]),
                limits,
            )
        })
    };
    let before = fs::read(f.journal()).unwrap();
    let snapshot = prepare(ObservationLimits::default()).unwrap();
    assert_eq!(
        snapshot.operations[0],
        Operation::InitializeWorkspace {
            root_id: ObjectId::from_bytes([0; 16])
        }
    );
    assert_eq!(snapshot.operations.len(), 4);
    assert_eq!(snapshot.files.len(), 1);
    let expected = crate::checkpoint_storage::PreparedCheckpointFile::from_bytes(
        b"private source version",
        &mesh_chunking::ChunkingConfig::default(),
        crate::ManifestPagingPolicy::flat(),
    )
    .unwrap();
    assert_eq!(snapshot.files[0].manifest(), expected.manifest());
    assert_eq!(
        prepare(ObservationLimits::default()).unwrap().operations,
        snapshot.operations
    );
    assert!(prepare(ObservationLimits {
        file_bytes: 1,
        ..ObservationLimits::default()
    })
    .is_err());
    assert!(prepare(ObservationLimits {
        bytes: 1,
        ..ObservationLimits::default()
    })
    .is_err());
    assert_eq!(fs::read(f.journal()).unwrap(), before);
    assert_eq!(
        fs::read(f.source.project().root().join("note")).unwrap(),
        b"later unsaved bytes"
    );
}
