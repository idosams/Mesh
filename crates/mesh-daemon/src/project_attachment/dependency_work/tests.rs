use super::*;
use crate::project_attachment::{ObservationLimits, SavedAttachmentVersion};
use ed25519_dalek::{Signer as _, SigningKey};
use std::{fs, path::PathBuf};
struct Fixture {
    root: PathBuf,
    storage: AttachmentStorage,
    owner: ProvisionedAttachment,
}
impl Fixture {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "mesh-dependency-work-{name}-{}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("source")).unwrap();
        fs::create_dir(root.join("metadata")).unwrap();
        fs::write(root.join("source/note"), b"manual source").unwrap();
        let storage = AttachmentStorage::open(&root.join("metadata")).unwrap();
        let owner = storage.provision(&root.join("source")).unwrap();
        Self {
            root,
            storage,
            owner,
        }
    }
    fn child(&self, parent: &ProvisionedAttachment, n: u32) -> ProvisionedAttachment {
        let version = save(parent);
        self.storage
            .open_version_lane(
                parent,
                &version.operation().to_string(),
                &format!("{n:032x}"),
                ObservationLimits::default(),
            )
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
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
fn journal(history: &ProvisionedAttachment) -> Vec<u8> {
    fs::read(history.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap()
}

#[test]
fn native_root_and_nested_manual_work_bindings_survive_restart_and_editor_writes() {
    let f = Fixture::new("nested");
    let child = f.child(&f.owner, 1);
    let nested = f.child(&child, 2);
    f.owner.enroll_dependency_history().unwrap();
    let before = journal(&f.owner);
    let child_before = journal(&child);
    let root = f
        .storage
        .dependency_work_binding(&f.owner, &f.owner)
        .unwrap();
    assert_eq!(root.work(), root.project());
    let selected = f
        .storage
        .dependency_work_binding(&f.owner, &nested)
        .unwrap();
    assert_eq!(selected.project(), root.project());
    assert_eq!(selected.authority(), root.authority());
    assert_ne!(selected.work(), root.work());
    assert_ne!(selected.installation(), root.installation());
    let child_binding = f.storage.dependency_work_binding(&f.owner, &child).unwrap();
    assert_ne!(selected.work(), child_binding.work());
    fs::write(
        nested.project().root().join("note"),
        b"ordinary harness keeps working",
    )
    .unwrap();
    let reopened_storage = AttachmentStorage::open(&f.root.join("metadata")).unwrap();
    let reopened_owner = reopened_storage.reopen(f.owner.id()).unwrap();
    let reopened_work = reopened_storage.reopen(nested.id()).unwrap();
    assert_eq!(
        reopened_storage
            .dependency_work_binding(&reopened_owner, &reopened_work)
            .unwrap(),
        selected
    );
    selected
        .revalidate(&reopened_storage, &reopened_owner, &reopened_work)
        .unwrap();
    assert_eq!(journal(&f.owner), before);
    assert_eq!(journal(&child), child_before);
    assert!(!nested
        .metadata_path()
        .join(crate::RECORD_FILE_NAME)
        .exists());
    assert_eq!(
        fs::read(nested.project().root().join("note")).unwrap(),
        b"ordinary harness keeps working"
    );
}

#[test]
fn native_work_refuses_unenrolled_foreign_or_descendant_root_authorities() {
    let f = Fixture::new("root-refusal");
    let child = f.child(&f.owner, 1);
    save(&child);
    assert!(f.storage.dependency_work_binding(&f.owner, &child).is_err());
    f.owner.enroll_dependency_history().unwrap();
    child.enroll_dependency_history().unwrap();
    assert!(f.storage.dependency_work_binding(&child, &child).is_err());
    fs::create_dir(f.root.join("unrelated")).unwrap();
    let unrelated = f.storage.provision(&f.root.join("unrelated")).unwrap();
    assert!(f
        .storage
        .dependency_work_binding(&f.owner, &unrelated)
        .is_err());
    let foreign = Fixture::new("foreign");
    save(&foreign.owner);
    foreign.owner.enroll_dependency_history().unwrap();
    assert!(f
        .storage
        .dependency_work_binding(&f.owner, &foreign.owner)
        .is_err());
    assert!(foreign
        .storage
        .dependency_work_binding(&f.owner, &child)
        .is_err());
}

#[test]
fn native_work_refuses_changed_receipts_source_and_store_without_adopting_replacements() {
    for mode in ["intent", "ready", "source", "store"] {
        let f = Fixture::new(mode);
        let child = f.child(&f.owner, 1);
        f.owner.enroll_dependency_history().unwrap();
        let selected = f.storage.dependency_work_binding(&f.owner, &child).unwrap();
        let before = journal(&f.owner);
        let allocation = child.project().root().parent().unwrap();
        let changed = match mode {
            "intent" | "ready" => {
                let path = allocation.join(format!("{mode}.json"));
                fs::write(&path, b"unknown changed ancestry").unwrap();
                Some(path)
            }
            "source" => {
                fs::rename(child.project().root(), allocation.join("retained-files")).unwrap();
                fs::create_dir(child.project().root()).unwrap();
                None
            }
            "store" => {
                fs::rename(child.metadata_path(), f.root.join("retained-store")).unwrap();
                fs::create_dir(child.metadata_path()).unwrap();
                None
            }
            _ => unreachable!(),
        };
        assert!(
            selected.revalidate(&f.storage, &f.owner, &child).is_err(),
            "{mode}"
        );
        assert_eq!(journal(&f.owner), before);
        if let Some(path) = changed {
            assert_eq!(fs::read(path).unwrap(), b"unknown changed ancestry");
        }
    }
}

#[test]
fn native_work_depth_bound_never_truncates_ancestry_to_a_successful_prefix() {
    let f = Fixture::new("depth");
    let mut works = vec![f.owner.clone()];
    for n in 1..=MAX_DEPTH + 1 {
        works.push(f.child(works.last().unwrap(), n as u32));
    }
    f.owner.enroll_dependency_history().unwrap();
    assert!(f
        .storage
        .dependency_work_binding(&f.owner, &works[MAX_DEPTH])
        .is_ok());
    let before = journal(&f.owner);
    assert!(f
        .storage
        .dependency_work_binding(&f.owner, works.last().unwrap())
        .is_err());
    assert_eq!(journal(&f.owner), before);
}

#[test]
fn replaced_allocation_container_cannot_reuse_a_binding_even_with_same_work_and_installation() {
    use mesh_types::{Blake3, ContentDigest as _};
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
    fn set(value: &mut Json, name: &str, replacement: Json) {
        let Json::Object(fields) = value else {
            panic!("object")
        };
        fields.iter_mut().find(|(key, _)| key == name).unwrap().1 = replacement;
    }
    let f = Fixture::new("container-replacement");
    let child = f.child(&f.owner, 1);
    f.owner.enroll_dependency_history().unwrap();
    let original = f.storage.dependency_work_binding(&f.owner, &child).unwrap();
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
    let refreshed = f.storage.dependency_work_binding(&f.owner, &child).unwrap();
    assert_eq!(refreshed.work(), original.work());
    assert_eq!(refreshed.installation(), original.installation());
    assert_ne!(
        refreshed, original,
        "same work/installation does not erase changed native allocation evidence"
    );
    assert!(original.revalidate(&f.storage, &f.owner, &child).is_err());
}

#[test]
fn two_prepared_work_selections_require_their_complete_still_held_custody_set() {
    use crate::workspace_custody::{
        lock_workspace_initialization, lock_workspace_initialization_set,
    };
    let f = Fixture::new("prepared-pair");
    let source = f.child(&f.owner, 1);
    let destination = f.child(&f.owner, 2);
    f.owner.enroll_dependency_history().unwrap();
    let source_selection = f
        .storage
        .prepare_dependency_work(&f.owner, &source)
        .unwrap();
    let destination_selection = f
        .storage
        .prepare_dependency_work(&f.owner, &destination)
        .unwrap();
    let owner_only = lock_workspace_initialization(&f.owner.store).unwrap();
    assert!(f
        .storage
        .validate_dependency_work(&source_selection, &owner_only)
        .is_err());
    assert!(f
        .storage
        .validate_dependency_work(&destination_selection, &owner_only)
        .is_err());
    drop(owner_only);
    let mut roots = source_selection.roots.clone();
    roots.extend(destination_selection.roots.iter().cloned());
    let complete = lock_workspace_initialization_set(&roots).unwrap();
    let selected_source = f
        .storage
        .validate_dependency_work(&source_selection, &complete)
        .unwrap();
    let selected_destination = f
        .storage
        .validate_dependency_work(&destination_selection, &complete)
        .unwrap();
    assert_ne!(selected_source.work(), selected_destination.work());
    assert_eq!(
        selected_source.authority(),
        selected_destination.authority()
    );
    // Preparation does not freeze receipts. They must be revalidated within this same transaction.
    let ready = destination
        .project()
        .root()
        .parent()
        .unwrap()
        .join("ready.json");
    let original = fs::read(&ready).unwrap();
    fs::write(&ready, b"changed after discovery").unwrap();
    assert!(f
        .storage
        .validate_dependency_work(&destination_selection, &complete)
        .is_err());
    assert!(f
        .storage
        .validate_dependency_work(&source_selection, &complete)
        .is_ok());
    fs::write(&ready, original).unwrap();
    let borrowed = lock_workspace_initialization(&f.owner.store).unwrap();
    drop(complete);
    assert!(f
        .storage
        .validate_dependency_work(&source_selection, &borrowed)
        .is_err());
    drop(borrowed);
    assert!(f
        .storage
        .dependency_work_binding(&f.owner, &destination)
        .is_ok());
}
