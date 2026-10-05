use super::*;
use crate::project_attachment::ObservationLimits;
use crate::workspace::OpenWorkspace;
use ed25519_dalek::{Signer as _, SigningKey};
use mesh_crypto::SigningPayload;
use mesh_store::{ApprovalRecord, ReviewRecord, ReviewVerdict, StoredRecord};
use mesh_types::{PublicKey, Signature};
use std::{fs, path::PathBuf};

#[test]
fn native_saved_review_refuses_unverified_pre_enrollment_main() {
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let root = std::env::temp_dir().join(format!(
        "mesh-bound-review-unverified-main-{}",
        std::process::id()
    ));
    fs::create_dir(&root).unwrap();
    let _cleanup = Cleanup(root.clone());
    fs::create_dir(root.join("source")).unwrap();
    fs::create_dir(root.join("metadata")).unwrap();
    fs::write(root.join("source/note"), b"preserve this project").unwrap();
    let storage = AttachmentStorage::open(&root.join("metadata")).unwrap();
    let owner = storage.provision(&root.join("source")).unwrap();
    let input = owner
        .project()
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    let id = |n| RecordDigest::from_bytes([n; 32]);
    let key = SigningKey::from_bytes(&[193; 32]);
    let actor = PublicKey::from_bytes(key.verifying_key().to_bytes());
    let legacy = owner
        .project()
        .save_capture(
            owner.metadata_path(),
            &input,
            actor,
            |payload: &SigningPayload| {
                Ok::<_, &'static str>(Signature::from_bytes(
                    key.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .unwrap();
    {
        let mut history =
            OpenWorkspace::open_attachment_store(owner.metadata_path(), owner.store.clone(), false)
                .unwrap();
        let bundle = history
            .saved_publication_review_bundle(legacy.operation())
            .unwrap();
        let actor_id =
            RecordDigest::from_bytes(*actor.actor_id::<mesh_types::Blake3>().digest().as_bytes());
        history
            .append_record(&StoredRecord::Review(ReviewRecord {
                bundle,
                subject_operation: legacy.operation(),
                opened_by: actor_id,
            }))
            .unwrap();
        let receipt = history
            .promote_approval_receipt(b"unverified retained receipt".to_vec())
            .unwrap();
        // A structurally retained historical approval is not proof of genesis.
        history
            .append_record(&StoredRecord::Approval(ApprovalRecord {
                approval: receipt,
                bundle,
                approver: actor_id,
                verdict: ReviewVerdict::Approved,
            }))
            .unwrap();
    }
    let reopened =
        OpenWorkspace::open_attachment_store(owner.metadata_path(), owner.store.clone(), false)
            .unwrap();
    let refused = current_review_base(&reopened)
        .expect_err("unverified existing approval must not become a genesis review");
    assert!(refused
        .to_string()
        .contains("existing Mesh main approval cannot be verified"));
    drop(reopened);
    owner.enroll_dependency_history().unwrap();
    fs::write(root.join("source/note"), b"new native private progress").unwrap();
    let input = owner
        .project()
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    let version = owner
        .prepare_dependency_capture(&input, actor, id(1), |payload: &SigningPayload| {
            Ok::<_, &'static str>(Signature::from_bytes(
                key.sign(payload.as_bytes()).to_bytes(),
            ))
        })
        .unwrap()
        .commit()
        .unwrap();
    let journal = owner.metadata_path().join(crate::RECORD_FILE_NAME);
    let before = fs::read(&journal).unwrap();
    let refused = storage
        .save_dependency_review_snapshot(&owner, &owner, version, &[], id(2))
        .unwrap_err();
    assert!(refused
        .to_string()
        .contains("legacy input ancestry needs explicit migration evidence"));
    assert_eq!(fs::read(journal).unwrap(), before);
    assert_eq!(
        fs::read(root.join("source/note")).unwrap(),
        b"new native private progress"
    );
}
