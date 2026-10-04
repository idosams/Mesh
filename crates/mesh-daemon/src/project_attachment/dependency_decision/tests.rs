use super::*;
use crate::project_attachment::{AttachmentStorage, ObservationLimits};
use ed25519_dalek::{Signer as _, SigningKey};
use std::{fs, path::PathBuf};
struct Fixture {
    root: PathBuf,
    history: ProvisionedAttachment,
    versions: Vec<SavedAttachmentVersion>,
}
impl Fixture {
    fn new(name: &str, enroll: bool) -> Self {
        let root = std::env::temp_dir().join(format!(
            "mesh-native-input-decision-{name}-{}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("source")).unwrap();
        fs::create_dir(root.join("metadata")).unwrap();
        let storage = AttachmentStorage::open(&root.join("metadata")).unwrap();
        let history = storage.provision(&root.join("source")).unwrap();
        let key = SigningKey::from_bytes(&[67; 32]);
        let mut versions = Vec::new();
        for bytes in ["first", "second"] {
            fs::write(root.join("source/note"), bytes).unwrap();
            let input = history
                .project()
                .capture_inputs(ObservationLimits::default())
                .unwrap();
            versions.push(
                history
                    .project()
                    .save_capture(
                        history.metadata_path(),
                        &input,
                        mesh_types::PublicKey::from_bytes(key.verifying_key().to_bytes()),
                        |payload| {
                            Ok::<_, &'static str>(mesh_types::Signature::from_bytes(
                                key.sign(payload.as_bytes()).to_bytes(),
                            ))
                        },
                    )
                    .unwrap(),
            );
        }
        if enroll {
            history.enroll_dependency_history().unwrap();
        }
        Self {
            root,
            history,
            versions,
        }
    }
    fn journal(&self) -> Vec<u8> {
        fs::read(self.history.metadata_path().join(RECORD_FILE_NAME)).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn request(n: u8) -> RecordDigest {
    RecordDigest::from_bytes([n; 32])
}

#[test]
fn native_decisions_reject_replace_and_revalidate_exact_saved_input_without_rewriting_history() {
    let f = Fixture::new("progression", true);
    let before = f.journal();
    let version = f.versions[0];
    let rejected = f
        .history
        .decide_saved_input(version, SavedInputDecision::Rejected, None, request(1))
        .unwrap();
    assert_eq!(rejected.revision(), 1);
    assert!(f
        .history
        .decide_saved_input(version, SavedInputDecision::Eligible, None, request(2))
        .is_err());
    let replacement = f
        .history
        .decide_saved_input(
            version,
            SavedInputDecision::Replaced(f.versions[1]),
            Some(rejected.record()),
            request(2),
        )
        .unwrap();
    assert_eq!(replacement.revision(), 2);
    let revalidated = f
        .history
        .decide_saved_input(
            version,
            SavedInputDecision::Eligible,
            Some(replacement.record()),
            request(3),
        )
        .unwrap();
    assert_eq!(revalidated.revision(), 3);
    let after = f.journal();
    assert_eq!(&after[..before.len()], before);
    assert_eq!(after.len() - before.len(), 3 * 145);
    assert_eq!(
        f.history
            .decide_saved_input(version, SavedInputDecision::Rejected, None, request(1))
            .unwrap(),
        rejected
    );
    assert_eq!(
        f.journal(),
        after,
        "historical retry must not rewind or duplicate decisions"
    );
    assert_eq!(
        f.history
            .project()
            .saved_file(f.history.metadata_path(), version, "note")
            .unwrap()
            .unwrap(),
        b"first"
    );
    assert_eq!(fs::read(f.root.join("source/note")).unwrap(), b"second");
    let _guard = crate::workspace_custody::lock_workspace_initialization(&f.history.store).unwrap();
    let (_, proof) = f
        .history
        .project()
        .read_configuration(f.history.metadata_path(), &f.history.store)
        .unwrap();
    let proof = proof.unwrap();
    let binding = proof.binding();
    assert_eq!(
        proof
            .policy()
            .native_decision(binding.project, binding.installation, version.operation()),
        Some((3, revalidated.record()))
    );
    assert!(!f.history.metadata_path().join(PENDING).exists());
}

#[test]
fn native_decisions_refuse_foreign_self_replacement_conflicting_requests_and_legacy_input() {
    let f = Fixture::new("refusal", true);
    let foreign = Fixture::new("foreign", false);
    let before = f.journal();
    let version = f.versions[0];
    assert!(f
        .history
        .decide_saved_input(
            foreign.versions[0],
            SavedInputDecision::Rejected,
            None,
            request(1)
        )
        .is_err());
    assert!(f
        .history
        .decide_saved_input(
            version,
            SavedInputDecision::Replaced(foreign.versions[0]),
            None,
            request(1)
        )
        .is_err());
    assert!(f
        .history
        .decide_saved_input(
            version,
            SavedInputDecision::Replaced(version),
            None,
            request(1)
        )
        .is_err());
    assert!(f
        .history
        .decide_saved_input(version, SavedInputDecision::Rejected, None, ZERO)
        .is_err());
    assert!(foreign
        .history
        .decide_saved_input(
            foreign.versions[0],
            SavedInputDecision::Rejected,
            None,
            request(1)
        )
        .is_err());
    assert_eq!(f.journal(), before);
    let rejected = f
        .history
        .decide_saved_input(version, SavedInputDecision::Rejected, None, request(1))
        .unwrap();
    let after = f.journal();
    assert!(f
        .history
        .decide_saved_input(version, SavedInputDecision::Eligible, None, request(1))
        .is_err());
    assert!(f
        .history
        .decide_saved_input(
            version,
            SavedInputDecision::Eligible,
            Some(request(99)),
            request(2)
        )
        .is_err());
    assert_eq!(f.journal(), after);
    assert_eq!(
        f.history
            .decide_saved_input(version, SavedInputDecision::Rejected, None, request(1))
            .unwrap(),
        rejected
    );
}
