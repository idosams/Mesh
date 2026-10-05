use super::*;
use std::{fs, io::Write as _, path::PathBuf};
#[test]
fn consumed_review_snapshot_recovers_and_preserves_historical_decisions() {
    use crate::project_attachment::{
        NativeConsumedStartRequest, NativeGrantInspection, NativeInputGrantRequest,
        ObservationLimits,
    };
    use crate::workspace::OpenWorkspace;
    use ed25519_dalek::{Signer as _, SigningKey};
    use mesh_crypto::SigningPayload;
    use mesh_types::{PublicKey, Signature};
    struct Cleanup(PathBuf, bool);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            if !self.1 {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
    }
    let root = std::env::temp_dir().join(format!(
        "mesh-consumed-review-snapshot-{}",
        std::process::id()
    ));
    fs::create_dir(&root).unwrap();
    let mut cleanup = Cleanup(root.clone(), false);
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
    let missing = storage
        .save_dependency_review_snapshot(&owner, &destination, consumed, &[], id(10))
        .unwrap_err();
    assert!(missing.to_string().contains("eligible native decision"));
    let eligible = owner
        .decide_saved_input(
            version,
            super::super::SavedInputDecision::Eligible,
            None,
            id(11),
        )
        .unwrap();
    let owner_journal = owner.metadata_path().join(crate::RECORD_FILE_NAME);
    let child_journal = destination.metadata_path().join(crate::RECORD_FILE_NAME);
    let child_before = fs::read(&child_journal).unwrap();
    let before = fs::read(&owner_journal).unwrap();
    let failure = storage
        .save_dependency_review_snapshot_with_io(
            &owner,
            &destination,
            consumed,
            &[],
            id(10),
            |step, file, frame| {
                if matches!(step, Step::Staged) {
                    file.write_all(&frame[..1])?;
                    file.sync_all()?;
                    return Err(io::Error::other("interrupted review snapshot"));
                }
                Ok(())
            },
            |file| file.sync_all(),
        )
        .unwrap_err();
    assert_eq!(failure.to_string(), "interrupted review snapshot");
    let partial = fs::read(&owner_journal).unwrap();
    assert_eq!(partial.len(), before.len() + 1);
    assert!(storage
        .save_dependency_review_snapshot(&owner, &owner, version, &[], id(10))
        .is_err());
    assert_eq!(fs::read(&owner_journal).unwrap(), partial);
    let retention = owner
        .inspect_pending_dependency_control_retention(id(10))
        .unwrap();
    let recover = |request| {
        let reopened_storage = AttachmentStorage::open(&root.join("metadata")).unwrap();
        let reopened_owner = reopened_storage.reopen(owner.id()).unwrap();
        let reopened_destination = reopened_storage.reopen(destination.id()).unwrap();
        reopened_storage
            .save_dependency_review_snapshot(
                &reopened_owner,
                &reopened_destination,
                consumed,
                &[],
                request,
            )
            .unwrap()
    };
    let snapshot = recover(id(10));
    assert!(
        retention.encode().contains(&snapshot.graph().to_hex()),
        "staged graph must be retained"
    );
    let failure = storage
        .save_dependency_review_snapshot_with_io(
            &owner,
            &destination,
            consumed,
            &[],
            id(14),
            |step, _, _| {
                if matches!(step, Step::Appended) {
                    Err(io::Error::other("lost review snapshot reply"))
                } else {
                    Ok(())
                }
            },
            |file| file.sync_all(),
        )
        .unwrap_err();
    assert_eq!(failure.to_string(), "lost review snapshot reply");
    let durable = fs::read(&owner_journal).unwrap();
    let recovered = recover(id(14));
    assert_ne!(recovered.record(), snapshot.record());
    assert_eq!(recovered.graph(), snapshot.graph());
    assert_eq!(recovered.validation(), snapshot.validation());
    assert_eq!(fs::read(&owner_journal).unwrap(), durable);
    assert_eq!(recover(id(14)), recovered);
    assert_eq!(fs::read(&owner_journal).unwrap(), durable);
    assert_eq!(fs::read(&child_journal).unwrap(), child_before);
    struct RestoreRoot(PathBuf, PathBuf);
    impl Drop for RestoreRoot {
        fn drop(&mut self) {
            fs::rename(&self.1, &self.0).unwrap();
        }
    }
    let before_loss = fs::read(&owner_journal).unwrap();
    let mut restore = None;
    assert!(storage
        .save_dependency_review_snapshot_with_io(
            &owner,
            &destination,
            consumed,
            &[],
            id(15),
            |step, _, _| {
                if matches!(step, Step::Staged) {
                    let original = root.join("source");
                    let moved = root.join("source-moved");
                    fs::rename(&original, &moved)?;
                    restore = Some(RestoreRoot(original, moved));
                }
                Ok(())
            },
            |file| file.sync_all()
        )
        .is_err());
    assert_eq!(
        fs::read(&owner_journal).unwrap(),
        before_loss,
        "lost physical input refuses before append"
    );
    drop(restore);
    let restored = recover(id(15));
    assert_eq!(restored.graph(), snapshot.graph());
    assert_eq!(recover(id(15)), restored);
    storage
        .decide_work_input_with_inputs(
            &owner,
            super::super::NativeWorkDecisionRequest {
                source: &destination,
                version: consumed,
                decision: super::super::SavedInputDecision::Eligible,
                expected_previous: None,
                request: id(18),
            },
            &[],
        )
        .unwrap();
    let unrelated = storage
        .save_dependency_review_snapshot(&owner, &destination, consumed, &[], id(19))
        .unwrap();
    assert_eq!(
        unrelated.validation(),
        snapshot.validation(),
        "output-local decisions do not change input eligibility"
    );
    let journal = fs::read(owner.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap();
    assert_eq!(
        storage
            .save_dependency_review_snapshot(&owner, &destination, consumed, &[], id(10))
            .unwrap(),
        snapshot
    );
    assert_eq!(
        fs::read(owner.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap(),
        journal
    );
    owner
        .decide_saved_input(
            version,
            super::super::SavedInputDecision::Rejected,
            Some(eligible.record()),
            id(12),
        )
        .unwrap();
    let rejected = fs::read(owner.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap();
    assert_eq!(
        storage
            .save_dependency_review_snapshot(&owner, &destination, consumed, &[], id(10))
            .unwrap(),
        snapshot
    );
    assert!(storage
        .save_dependency_review_snapshot(&owner, &destination, consumed, &[], id(13))
        .is_err());
    assert_eq!(
        fs::read(owner.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap(),
        rejected
    );
    let cas = Cas::<PinnedRootFs, Blake3>::with_filesystem(
        owner.metadata_path(),
        owner.store.filesystem(),
    )
    .unwrap();
    let graph_path = cas.layout().chunk_path(&mesh_cas::Digest32::from_bytes(
        *snapshot.graph().as_bytes(),
    ));
    let graph_bytes = fs::read(&graph_path).unwrap();
    fs::write(&graph_path, b"substituted graph object").unwrap();
    assert!(storage
        .save_dependency_review_snapshot(&owner, &destination, consumed, &[], id(10))
        .is_err());
    assert_eq!(fs::read(&owner_journal).unwrap(), rejected);
    fs::write(&graph_path, graph_bytes).unwrap();
    assert_eq!(recover(id(10)), snapshot);
    // Retain only this fully asserted synthetic fixture when explicitly requested by a proof run.
    if let Some(export) = std::env::var_os("MESH_REVIEW_SNAPSHOT_FIXTURE") {
        let export = PathBuf::from(export);
        assert!(export.is_absolute());
        let path = |p: &std::path::Path| Json::text(p.to_str().unwrap());
        let manifest = Json::object([
            (
                "schema",
                Json::text("mesh.native-review-snapshot-fixture/v1"),
            ),
            ("root", path(&root)),
            ("storage", path(&root.join("metadata"))),
            ("owner_registration", Json::text(owner.id())),
            ("child_registration", Json::text(destination.id())),
            ("snapshot", Json::text(snapshot.record().to_hex())),
            ("graph", Json::text(snapshot.graph().to_hex())),
            (
                "preserved_files",
                Json::Array(vec![
                    path(&owner_journal),
                    path(&child_journal),
                    path(&graph_path),
                    path(&owner.project().root().join("note")),
                    path(&destination.project().root().join("note")),
                ]),
            ),
        ])
        .encode();
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(export)
            .unwrap();
        output.write_all(manifest.as_bytes()).unwrap();
        output.sync_all().unwrap();
        cleanup.1 = true;
    }
}
