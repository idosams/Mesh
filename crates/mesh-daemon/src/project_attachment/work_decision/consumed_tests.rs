use super::*;
use std::{fs, io::Write as _, path::PathBuf};

struct RestoreInput(PathBuf, PathBuf);
impl Drop for RestoreInput {
    fn drop(&mut self) {
        fs::rename(&self.1, &self.0).expect("restore exact native input");
    }
}

impl AttachmentStorage {
    fn assert_consumed_work_decision(
        &self,
        owner: &ProvisionedAttachment,
        source: &ProvisionedAttachment,
        version: SavedAttachmentVersion,
        original: &ProvisionedAttachment,
    ) {
        let id = |n| {
            super::super::dependency_transaction::hash(
                format!("mesh-consumed-work-decision-proof-{n}").as_bytes(),
            )
        };
        let request = |decision, previous, number| NativeWorkDecisionRequest {
            source,
            version,
            decision,
            expected_previous: previous,
            request: id(number),
        };
        let owner_path = owner.metadata_path().join(crate::RECORD_FILE_NAME);
        let source_path = source.metadata_path().join(crate::RECORD_FILE_NAME);
        let source_before = fs::read(&source_path).unwrap();
        let before = fs::read(&owner_path).unwrap();
        let failure = self
            .decide_work_with_inputs_and_io(
                owner,
                request(SavedInputDecision::Rejected, None, 1),
                &[original],
                |step, file, frame| {
                    if matches!(step, Step::Staged) {
                        file.write_all(&frame[..1])?;
                        file.sync_all()?;
                        return Err(io::Error::other("partial consumed decision"));
                    }
                    Ok(())
                },
                |file| file.sync_all(),
            )
            .unwrap_err();
        assert_eq!(failure.to_string(), "partial consumed decision");
        let partial = fs::read(&owner_path).unwrap();
        assert_eq!(partial.len(), before.len() + 1);
        assert!(self
            .decide_work_input_with_inputs(
                owner,
                request(SavedInputDecision::Eligible, None, 1),
                &[original],
            )
            .is_err());
        assert_eq!(fs::read(&owner_path).unwrap(), partial);
        let recover = |decision, previous, number| {
            let storage = AttachmentStorage::open(&self.path).unwrap();
            let reopened_owner = storage.reopen(owner.id()).unwrap();
            let reopened_source = storage.reopen(source.id()).unwrap();
            let reopened_original = storage.reopen(original.id()).unwrap();
            storage
                .decide_work_input_with_inputs(
                    &reopened_owner,
                    NativeWorkDecisionRequest {
                        source: &reopened_source,
                        version,
                        decision,
                        expected_previous: previous,
                        request: id(number),
                    },
                    &[&reopened_original],
                )
                .unwrap()
        };
        let rejected = recover(SavedInputDecision::Rejected, None, 1);
        assert_eq!(rejected.revision(), 1);
        let acknowledged = fs::read(&owner_path).unwrap();
        assert_eq!(recover(SavedInputDecision::Rejected, None, 1), rejected);
        assert_eq!(fs::read(&owner_path).unwrap(), acknowledged);
        let failure = self
            .decide_work_with_inputs_and_io(
                owner,
                request(SavedInputDecision::Eligible, Some(rejected.record()), 2),
                &[original],
                |step, _, _| {
                    if matches!(step, Step::Appended) {
                        Err(io::Error::other("lost consumed decision reply"))
                    } else {
                        Ok(())
                    }
                },
                |file| file.sync_all(),
            )
            .unwrap_err();
        assert_eq!(failure.to_string(), "lost consumed decision reply");
        let appended = fs::read(&owner_path).unwrap();
        let eligible = recover(SavedInputDecision::Eligible, Some(rejected.record()), 2);
        assert_eq!(eligible.revision(), 2);
        assert_eq!(fs::read(&owner_path).unwrap(), appended);
        assert_eq!(recover(SavedInputDecision::Rejected, None, 1), rejected);
        assert_eq!(fs::read(&owner_path).unwrap(), appended);
        assert!(
            self.decide_work_input_with_inputs(
                owner,
                request(SavedInputDecision::Rejected, Some(rejected.record()), 3),
                &[original],
            )
            .is_err(),
            "stale prior decision must refuse after recovered revalidation"
        );
        assert_eq!(fs::read(&owner_path).unwrap(), appended);
        // Here the owner is also the consumed input, so it is already in the custody set.
        // Remove the actual required input after staging rather than omitting a redundant handle.
        assert_eq!(original.id(), owner.id());
        let original_path = original.project().root().to_path_buf();
        let offline = original_path.with_extension("decision-input-offline");
        assert!(!offline.exists());
        let mut restore = None;
        let failure = self.decide_work_with_inputs_and_io(
            owner,
            request(SavedInputDecision::Rejected, Some(eligible.record()), 4),
            &[original],
            |step, _, _| {
                if matches!(step, Step::Staged) {
                    fs::rename(&original_path, &offline)?;
                    restore = Some(RestoreInput(original_path.clone(), offline.clone()));
                }
                Ok(())
            },
            |file| file.sync_all(),
        );
        let staged = restore.is_some();
        drop(restore);
        assert!(
            staged,
            "input disappearance must occur after durable staging"
        );
        assert!(
            failure.is_err(),
            "lost required input cannot authorize append"
        );
        assert_eq!(fs::read(&owner_path).unwrap(), appended);
        let recovered = recover(SavedInputDecision::Rejected, Some(eligible.record()), 4);
        assert_eq!(recovered.revision(), 3);
        let final_journal = fs::read(&owner_path).unwrap();
        assert_eq!(
            recover(SavedInputDecision::Rejected, Some(eligible.record()), 4),
            recovered
        );
        assert_eq!(fs::read(&owner_path).unwrap(), final_journal);
        assert_eq!(fs::read(&source_path).unwrap(), source_before);
    }
}

#[test]
fn consumed_decisions_recover_torn_append_lost_reply_and_input_loss() {
    use crate::project_attachment::{
        NativeConsumedStartRequest, NativeGrantInspection, NativeInputGrantRequest,
        ObservationLimits,
    };
    use crate::workspace::OpenWorkspace;
    use ed25519_dalek::{Signer as _, SigningKey};
    use mesh_crypto::SigningPayload;
    use mesh_types::{PublicKey, Signature};
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let root = std::env::temp_dir().join(format!(
        "mesh-consumed-decision-recovery-{}",
        std::process::id()
    ));
    fs::create_dir(&root).unwrap();
    let _cleanup = Cleanup(root.clone());
    fs::create_dir(root.join("source")).unwrap();
    fs::create_dir(root.join("metadata")).unwrap();
    fs::write(root.join("source/note"), b"exact native input").unwrap();
    let storage = AttachmentStorage::open(&root.join("metadata")).unwrap();
    let owner = storage.provision(&root.join("source")).unwrap();
    let input = owner
        .project()
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    let (_, created) = owner
        .project()
        .history_configuration(&owner.store, Some(input.exclusion_digest()))
        .unwrap();
    drop(
        OpenWorkspace::open_attachment_store(owner.metadata_path(), owner.store.clone(), created)
            .unwrap(),
    );
    owner.enroll_dependency_history().unwrap();
    let key = SigningKey::from_bytes(&[179; 32]);
    let actor = PublicKey::from_bytes(key.verifying_key().to_bytes());
    let id = |n| RecordDigest::from_bytes([n; 32]);
    let sign = |payload: &SigningPayload| {
        Ok::<_, &'static str>(Signature::from_bytes(
            key.sign(payload.as_bytes()).to_bytes(),
        ))
    };
    let version = owner
        .prepare_dependency_capture(&input, actor, id(1), sign)
        .unwrap()
        .commit()
        .unwrap();
    let destination = storage
        .reserve_dependency_lane(&owner, &owner, version, id(2))
        .unwrap();
    let grant = storage
        .grant_saved_input(
            &owner,
            NativeInputGrantRequest {
                source: &owner,
                version,
                destination: &destination,
                allowed: true,
                expected_previous: None,
                request: id(3),
            },
        )
        .unwrap();
    let candidate = storage
        .prepare_consumed_start(
            &owner,
            NativeConsumedStartRequest {
                input: NativeGrantInspection {
                    source: &owner,
                    version,
                    destination: &destination,
                    grant: grant.record(),
                },
                available: &[],
                request: id(4),
                limits: ObservationLimits::default(),
            },
            actor,
            sign,
        )
        .unwrap();
    let staged = candidate.stage(&storage).unwrap();
    candidate
        .complete_fenced_consumption(&storage, &staged)
        .unwrap();
    let consumed = storage
        .registered_dependency_versions(destination.id())
        .unwrap()[0];
    storage.assert_consumed_work_decision(&owner, &destination, consumed, &owner);
}
