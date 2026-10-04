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
        let root = std::env::temp_dir().join(format!(
            "mesh-native-work-decision-{name}-{}",
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
        decision: SavedInputDecision,
        previous: Option<RecordDigest>,
        request: RecordDigest,
    ) -> NativeWorkDecisionRequest<'_> {
        NativeWorkDecisionRequest {
            source: &self.source,
            version: self.version,
            decision,
            expected_previous: previous,
            request,
        }
    }
    fn journal(&self) -> PathBuf {
        self.owner.metadata_path().join(crate::RECORD_FILE_NAME)
    }
    fn decide(
        &self,
        decision: SavedInputDecision,
        previous: Option<RecordDigest>,
        request: RecordDigest,
    ) -> io::Result<NativeInputDecision> {
        self.storage
            .decide_work_input(&self.owner, self.request(decision, previous, request))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn child_decisions_use_owning_authority_and_distinct_work_without_rewriting_child_history() {
    let f = Fixture::new("progression");
    let binding = f
        .storage
        .dependency_work_binding(&f.owner, &f.source)
        .unwrap();
    let rejected = f.decide(SavedInputDecision::Rejected, None, id(1)).unwrap();
    assert_eq!(rejected.revision(), 1);
    {
        let _guard =
            crate::workspace_custody::lock_workspace_initialization(&f.owner.store).unwrap();
        let (_, proof) = f
            .owner
            .project()
            .read_configuration(f.owner.metadata_path(), &f.owner.store)
            .unwrap();
        let proof = proof.unwrap();
        assert_eq!(
            proof.policy().native_decision(
                binding.work(),
                binding.installation(),
                f.version.operation()
            ),
            Some((1, rejected.record()))
        );
        assert_eq!(
            proof.policy().native_decision(
                binding.project(),
                binding.installation(),
                f.version.operation()
            ),
            None
        );
    }
    fs::write(
        f.source.project().root().join("note"),
        b"replacement saved by normal editor",
    )
    .unwrap();
    let replacement = save(&f.source);
    let child_before = fs::read(f.source.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap();
    let replaced = f
        .decide(
            SavedInputDecision::Replaced(replacement),
            Some(rejected.record()),
            id(2),
        )
        .unwrap();
    let eligible = f
        .decide(SavedInputDecision::Eligible, Some(replaced.record()), id(3))
        .unwrap();
    assert_eq!(eligible.revision(), 3);
    let before_retry = fs::read(f.journal()).unwrap();
    assert_eq!(
        f.decide(SavedInputDecision::Rejected, None, id(1)).unwrap(),
        rejected
    );
    assert_eq!(fs::read(f.journal()).unwrap(), before_retry);
    assert_eq!(
        fs::read(f.source.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap(),
        child_before
    );
    assert!(!f
        .destination
        .metadata_path()
        .join(crate::RECORD_FILE_NAME)
        .exists());
}

#[test]
fn work_decisions_refuse_foreign_versions_replacements_and_descendant_root_authority() {
    let f = Fixture::new("refusals");
    let foreign = Fixture::new("foreign");
    let before = fs::read(f.journal()).unwrap();
    assert!(f
        .storage
        .decide_work_input(
            &f.owner,
            NativeWorkDecisionRequest {
                source: &foreign.source,
                ..f.request(SavedInputDecision::Rejected, None, id(1))
            }
        )
        .is_err());
    assert!(f
        .storage
        .decide_work_input(
            &f.owner,
            NativeWorkDecisionRequest {
                version: foreign.version,
                ..f.request(SavedInputDecision::Rejected, None, id(1))
            }
        )
        .is_err());
    let other_version = save(&f.destination);
    assert!(f
        .decide(SavedInputDecision::Replaced(other_version), None, id(1))
        .is_err());
    assert!(f
        .decide(SavedInputDecision::Replaced(f.version), None, id(1))
        .is_err());
    assert!(f
        .decide(SavedInputDecision::Eligible, Some(id(99)), id(1))
        .is_err());
    f.source.enroll_dependency_history().unwrap();
    assert!(f
        .storage
        .decide_work_input(
            &f.source,
            f.request(SavedInputDecision::Eligible, None, id(1))
        )
        .is_err());
    assert_eq!(fs::read(f.journal()).unwrap(), before);
}

#[test]
fn interrupted_child_decisions_recover_exactly_after_catalog_reopen() {
    for length in [0, 1, 72, 144, 145] {
        let f = Fixture::new(&format!("prefix-{length}"));
        let before = fs::read(f.journal()).unwrap();
        let failed = f.storage.decide_work_with_io(
            &f.owner,
            f.request(SavedInputDecision::Rejected, None, id(1)),
            |step, file, frame| {
                if matches!(step, Step::Staged) {
                    file.write_all(&frame[..length])?;
                    file.sync_all()?;
                    return Err(io::Error::other("injected child-decision interruption"));
                }
                Ok(())
            },
            |file| file.sync_all(),
        );
        assert!(failed.is_err());
        let partial = fs::read(f.journal()).unwrap();
        assert_eq!(partial.len(), before.len() + length);
        assert!(f.decide(SavedInputDecision::Eligible, None, id(1)).is_err());
        assert_eq!(fs::read(f.journal()).unwrap(), partial);
        let storage = AttachmentStorage::open(&f.root.join("metadata")).unwrap();
        let owner = storage.reopen(f.owner.id()).unwrap();
        let source = storage.reopen(f.source.id()).unwrap();
        let recovered = storage
            .decide_work_input(
                &owner,
                NativeWorkDecisionRequest {
                    source: &source,
                    ..f.request(SavedInputDecision::Rejected, None, id(1))
                },
            )
            .unwrap();
        assert_eq!(recovered.revision(), 1);
        let complete = fs::read(f.journal()).unwrap();
        assert_eq!(complete.len(), before.len() + 145);
        assert_eq!(
            f.decide(SavedInputDecision::Rejected, None, id(1)).unwrap(),
            recovered
        );
        assert_eq!(fs::read(f.journal()).unwrap(), complete);
    }
}

#[test]
fn child_decision_rechecks_changed_ancestry_before_append_and_preserves_pending_evidence() {
    let f = Fixture::new("changed-ancestry");
    let before = fs::read(f.journal()).unwrap();
    let ready = f
        .source
        .project()
        .root()
        .parent()
        .unwrap()
        .join("ready.json");
    let original = fs::read(&ready).unwrap();
    let failed = f.storage.decide_work_with_io(
        &f.owner,
        f.request(SavedInputDecision::Rejected, None, id(1)),
        |step, _, _| {
            if matches!(step, Step::Staged) {
                fs::write(&ready, b"unknown changed ancestry")?;
            }
            Ok(())
        },
        |file| file.sync_all(),
    );
    assert!(failed.is_err());
    assert_eq!(fs::read(f.journal()).unwrap(), before);
    assert_eq!(fs::read(&ready).unwrap(), b"unknown changed ancestry");
    fs::write(&ready, original).unwrap();
    assert!(f.decide(SavedInputDecision::Rejected, None, id(1)).is_ok());
}

#[test]
fn native_root_selection_preserves_existing_decision_request_identity() {
    let f = Fixture::new("root-retry");
    let version = f.owner.saved_versions().unwrap()[0];
    let original = f
        .owner
        .decide_saved_input(version, SavedInputDecision::Rejected, None, id(1))
        .unwrap();
    let before = fs::read(f.journal()).unwrap();
    let retried = f
        .storage
        .decide_work_input(
            &f.owner,
            NativeWorkDecisionRequest {
                source: &f.owner,
                version,
                ..f.request(SavedInputDecision::Rejected, None, id(1))
            },
        )
        .unwrap();
    assert_eq!(retried, original);
    assert_eq!(fs::read(f.journal()).unwrap(), before);
}
