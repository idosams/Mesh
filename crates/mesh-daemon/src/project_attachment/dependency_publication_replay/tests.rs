#[path = "tests/consumption_committed_owner_retention.rs"]
mod consumption_committed_owner_retention;
#[path = "tests/consumption_install_retention.rs"]
mod consumption_install_retention;
#[path = "tests/consumption_owner_retention.rs"]
mod consumption_owner_retention;
#[path = "tests/consumption_retention.rs"]
mod consumption_retention;
#[path = "tests/native_completed_history.rs"]
mod native_completed_history;
#[path = "tests/native_consumption_checkpoint.rs"]
mod native_consumption_checkpoint;
#[path = "tests/native_consumption_completion.rs"]
mod native_consumption_completion;
#[path = "tests/native_consumption_owner.rs"]
mod native_consumption_owner;
#[path = "tests/native_consumption_start.rs"]
mod native_consumption_start;
#[path = "tests/native_grant.rs"]
mod native_grant;
#[path = "tests/native_reservation.rs"]
mod native_reservation;
#[path = "tests/restart.rs"]
mod restart;
use super::*;
use crate::project_attachment::{
    NativeSavedReviewRequest, ObservationLimits, ProvisionedAttachment,
};
use crate::workspace::OpenWorkspace;
use ed25519_dalek::{Signer as _, SigningKey};
use mesh_approval::{
    ApprovalDecision, ExpectedHumanApproval, HumanApprovalCredential, HumanApprovalReceiptDraft,
};
use mesh_crypto::SigningPayload;
use mesh_store::{DependencyKind, DependencyRecord, RecordDigest, StoredRecord};
use mesh_types::{PublicKey, Signature};
use ring::{
    rand::SystemRandom,
    signature::{EcdsaKeyPair, KeyPair as _, ECDSA_P256_SHA256_ASN1_SIGNING},
};
use std::{fs, io::Write as _, path::PathBuf};

fn assert_capture_history_inventory(
    storage: &AttachmentStorage,
    work: &ProvisionedAttachment,
    request: RecordDigest,
    trusted: &crate::TrustedReviewers,
    capture: &crate::project_attachment::NativeCaptureRetention,
    completed: &Json,
) {
    let inventory = storage
        .inspect_native_history_retention_during_capture(work, request, trusted)
        .unwrap();
    assert_eq!(
        inventory.get("schema"),
        Some(&Json::text("mesh.native-history-capture-retention/v1"))
    );
    assert_eq!(
        inventory.get("scope"),
        Some(&Json::text("selected-store-capture-recovery"))
    );
    assert_eq!(
        inventory.get("collection_authority"),
        Some(&Json::Bool(false))
    );
    let exact = capture.to_json();
    assert_eq!(inventory.get("capture"), Some(&exact));
    for name in ["device", "inode"] {
        assert_eq!(inventory.get(name), exact.get(name));
    }
    for name in ["operations", "manifests", "payloads", "sidecars"] {
        let Json::Array(retained) = inventory.get(name).unwrap() else {
            panic!("inventory roots missing")
        };
        let Json::Array(previous) = completed.get(name).unwrap() else {
            panic!("completed roots missing")
        };
        assert!(
            previous.iter().all(|object| retained.contains(object)),
            "completed {name} roots lost during capture"
        );
    }
    for name in ["manifests", "payloads"] {
        let Json::Array(retained) = inventory.get(name).unwrap() else {
            panic!("inventory roots missing")
        };
        let Json::Array(recovery) = exact.get(name).unwrap() else {
            panic!("capture roots missing")
        };
        assert!(
            recovery.iter().all(|object| retained.contains(object)),
            "staged {name} roots missing"
        );
    }
    let Json::Array(sidecars) = inventory.get("sidecars").unwrap() else {
        panic!("sidecars missing")
    };
    assert!(sidecars.contains(&Json::object([
        ("name", exact.get("sidecar").unwrap().clone()),
        ("digest", exact.get("sidecar_digest").unwrap().clone()),
    ])));
}

#[test]
fn root_publication_reopens_only_with_exact_trusted_receipt() {
    root_publication_fixture(0);
}
#[test]
fn native_writer_commits_root_publications_against_verified_main() {
    root_publication_fixture(1);
}
#[test]
fn root_publication_recovers_after_process_exit_at_both_frame_edges() {
    root_publication_fixture(2);
}
#[test]
fn staged_publication_fences_ordinary_native_decisions_without_losing_recovery() {
    root_publication_fixture(3);
}
#[test]
fn native_capture_after_publication_preserves_accepted_main() {
    root_publication_fixture(4);
}
#[test]
fn registered_scheduler_capture_after_publication_saves_and_recognizes_unchanged() {
    root_publication_fixture(5);
}
#[test]
fn native_consumption_start_retention_after_root_publication() {
    root_publication_fixture(6);
}
#[test]
fn native_consumption_install_retention_after_root_publication() {
    root_publication_fixture(7);
}
#[test]
fn native_consumption_owner_retention_after_root_publication() {
    root_publication_fixture(8);
}
#[test]
fn native_consumption_committed_owner_retention_after_root_publication() {
    root_publication_fixture(9);
}
#[test]
fn native_trusted_grants_after_root_publication() {
    root_publication_fixture(10);
}
#[test]
fn native_reservations_after_root_publication() {
    root_publication_fixture(11);
}
#[test]
fn native_consumed_start_after_root_publication() {
    root_publication_fixture(12);
}
#[test]
fn native_consumed_checkpoint_after_root_publication() {
    root_publication_fixture(13);
}
#[test]
fn native_consumption_owner_after_root_publication() {
    root_publication_fixture(14);
}
#[test]
fn native_consumption_completion_after_root_publication() {
    root_publication_fixture(15);
}
#[test]
fn native_completed_versions_after_root_publication() {
    root_publication_fixture(16);
}
fn root_publication_fixture(writer: u8) {
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let root = std::env::temp_dir().join(format!(
        "mesh-root-publication-replay-{writer}-{}",
        std::process::id()
    ));
    fs::create_dir(&root).unwrap();
    let _cleanup = Cleanup(root.clone());
    fs::create_dir(root.join("source")).unwrap();
    fs::create_dir(root.join("metadata")).unwrap();
    fs::write(root.join("source/note"), b"first private version").unwrap();
    if matches!(writer, 7..=9) {
        consumption_install_retention::add_source_entries(&root);
    }
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
    let key = SigningKey::from_bytes(&[191; 32]);
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
    let native_grant_destination =
        (writer == 10).then(|| native_grant::prepare(&storage, &owner, &owner, version));
    let pending_consumption =
        (writer == 6).then(|| consumption_retention::prepare(&storage, &owner, &owner, version));
    let installing = matches!(writer, 7..=9)
        .then(|| consumption_install_retention::prepare(&storage, &owner, &owner, version));
    let snapshot = storage
        .save_dependency_review_snapshot(&owner, &owner, version, &[], id(2))
        .unwrap();
    let bound = storage
        .save_dependency_review(
            &owner,
            NativeSavedReviewRequest {
                source: &owner,
                version,
                snapshot,
                request: id(3),
                opener: id(4),
            },
            &[],
        )
        .unwrap();
    let preview = storage
        .saved_dependency_review_preview(owner.id(), bound.record())
        .unwrap()
        .0;
    // A later private save must not substitute itself for the exact approved output.
    fs::write(root.join("source/note"), b"later private version").unwrap();
    let later = owner
        .project()
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    let later_version = owner
        .prepare_dependency_capture(&later, actor, id(5), sign)
        .unwrap()
        .commit()
        .unwrap();
    let later_snapshot = storage
        .save_dependency_review_snapshot(&owner, &owner, later_version, &[], id(7))
        .unwrap();
    assert_eq!(
        storage
            .inspect_root_publication_history(owner.id(), &TrustedReviewers::default())
            .unwrap()
            .get("publication"),
        Some(&Json::Null)
    );
    let rng = SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &rng).unwrap();
    let key =
        EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &rng).unwrap();
    let credential =
        HumanApprovalCredential::from_public_key(key.public_key().as_ref().try_into().unwrap())
            .unwrap();
    let trust = TrustedReviewers::with_human_credentials([credential.clone()]);
    let draft = HumanApprovalReceiptDraft::new(
        ExpectedHumanApproval::new(preview.clone(), credential.clone(), [61; 32]),
        ApprovalDecision::Approve,
    );
    let signature = key.sign(&rng, &draft.canonical_bytes()).unwrap();
    let receipt = draft
        .with_signature(signature.as_ref().to_vec())
        .unwrap()
        .canonical_bytes();
    let journal = owner
        .metadata_path()
        .join(crate::workspace::RECORD_FILE_NAME);
    if writer != 0 {
        if matches!(writer, 2 | 3) {
            let result = restart::interrupted_publication(
                &root,
                owner.id(),
                id(6),
                bound.record(),
                &receipt,
                key.public_key().as_ref(),
                &journal,
                "first",
                || {
                    if writer == 3 {
                        let error = storage
                            .decide_native_saved_input(
                                owner.id(),
                                version.operation(),
                                crate::project_attachment::NativeSavedInputDecision::Rejected,
                                None,
                                id(98),
                                &trust,
                            )
                            .unwrap_err();
                        assert!(
                            error.to_string().contains("pending")
                                && error.to_string().contains("publication"),
                            "{error}"
                        );
                        assert!(
                            owner
                                .decide_saved_input(
                                    version,
                                    crate::project_attachment::SavedInputDecision::Rejected,
                                    None,
                                    id(99)
                                )
                                .is_err(),
                            "an ordinary decision overtook a staged publication"
                        );
                        assert!(
                            owner.saved_versions().is_err(),
                            "ordinary native reads admitted unresolved publication"
                        );
                    }
                },
            );
            assert_eq!(
                result.get("head").and_then(Json::as_text),
                Some(
                    RecordDigest::from_bytes(*preview.reviewed_actor_head().as_bytes())
                        .to_hex()
                        .as_str()
                )
            );
        } else if writer == 1 {
            use super::super::dependency_private_context::publication::Step;
            use std::io::Write as _;
            let before = fs::read(&journal).unwrap();
            let failure = storage
                .commit_native_publication_with_io(
                    owner.id(),
                    id(6),
                    bound.record(),
                    &receipt,
                    &trust,
                    |step, file, frame| {
                        if matches!(step, Step::Staged) {
                            file.write_all(&frame[..1])?;
                            file.sync_all()?;
                            return Err(std::io::Error::other("root publication torn boundary"));
                        }
                        Ok(())
                    },
                    |file| file.sync_all(),
                )
                .unwrap_err();
            assert!(failure
                .to_string()
                .contains("root publication torn boundary"));
            let torn = fs::read(&journal).unwrap();
            assert!(torn.starts_with(&before) && torn.len() > before.len());
            assert!(storage
                .inspect_native_publication_history(owner.id(), &trust)
                .is_err());
            assert_eq!(fs::read(&journal).unwrap(), torn);
        }
        let committed = storage
            .commit_native_publication(owner.id(), id(6), bound.record(), &receipt, &trust)
            .unwrap();
        assert_eq!(committed.revision(), 1);
        assert_eq!(
            committed.head().as_bytes(),
            preview.reviewed_actor_head().as_bytes()
        );
    } else {
        // Test-only durable fixture. This does not exercise OS human presence or a publication writer.
        let _guard = crate::workspace_custody::lock_workspace_initialization(&owner.store).unwrap();
        let (_, evidence) = owner
            .project()
            .read_publication_private_history(owner.metadata_path(), &owner.store)
            .unwrap();
        let (ordinal, previous) = evidence.policy().native_head().unwrap();
        let cas =
            Cas::<_, Blake3>::with_filesystem(owner.metadata_path(), owner.store.filesystem())
                .unwrap();
        cas.promote(receipt.clone()).unwrap();
        let digest = super::super::dependency_transaction::hash;
        let body = Json::object([
            ("request", Json::text(id(6).to_hex())),
            ("revision", Json::Number(1)),
            ("previous", Json::text(id(0).to_hex())),
            ("review", Json::text(bound.record().to_hex())),
            ("receipt", Json::text(digest(&receipt).to_hex())),
            (
                "result",
                Json::text(
                    RecordDigest::from_bytes(*preview.reviewed_actor_head().as_bytes()).to_hex(),
                ),
            ),
            (
                "credential",
                Json::text(RecordDigest::from_bytes(*credential.id().as_bytes()).to_hex()),
            ),
            ("challenge", Json::text(id(61).to_hex())),
        ]);
        let bytes = Json::object([
            ("schema", Json::text("mesh.dependency-policy/v5")),
            (
                "authority",
                Json::text(evidence.binding().authority.to_hex()),
            ),
            ("revision", Json::Number(ordinal + 1)),
            ("previous", Json::text(previous.to_hex())),
            (
                "kind",
                Json::Number(DependencyKind::Publication.code().into()),
            ),
            ("body", body),
        ])
        .encode()
        .into_bytes();
        let record = DependencyRecord {
            authority: evidence.binding().authority,
            revision: ordinal + 1,
            previous,
            payload: digest(&bytes),
            kind: DependencyKind::Publication,
        };
        let mut policy = evidence.policy().clone();
        policy.apply(record, &bytes).unwrap();
        cas.promote(bytes).unwrap();
        let mut file = fs::OpenOptions::new().append(true).open(&journal).unwrap();
        file.write_all(&mesh_store::frame_record(&StoredRecord::Dependency(record)))
            .unwrap();
        file.sync_all().unwrap();
    }
    if writer == 16 {
        native_completed_history::inspect(
            &storage,
            &owner,
            &owner,
            version,
            &trust,
            key.public_key().as_ref(),
            b"first private version",
        );
        return;
    }
    if writer == 15 {
        native_consumption_completion::inspect(
            &storage,
            &owner,
            &owner,
            version,
            &trust,
            key.public_key().as_ref(),
            b"first private version",
        );
        return;
    }
    if writer == 14 {
        native_consumption_owner::inspect(
            &storage,
            &owner,
            &owner,
            version,
            &trust,
            key.public_key().as_ref(),
            b"first private version",
        );
        return;
    }
    if writer == 13 {
        native_consumption_checkpoint::inspect(
            &storage,
            &owner,
            &owner,
            version,
            &trust,
            key.public_key().as_ref(),
            b"first private version",
        );
        return;
    }
    if writer == 12 {
        native_consumption_start::inspect(
            &storage,
            &owner,
            &owner,
            version,
            &trust,
            key.public_key().as_ref(),
            b"first private version",
        );
        return;
    }
    if writer == 11 {
        native_reservation::inspect(
            &storage,
            &owner,
            &owner,
            version,
            &trust,
            key.public_key().as_ref(),
        );
        return;
    }
    if let Some(destination) = native_grant_destination {
        native_grant::inspect(
            &storage,
            &owner,
            &owner,
            version,
            &destination,
            &trust,
            key.public_key().as_ref(),
        );
        return;
    }
    if let Some(pending) = installing {
        if writer == 9 {
            consumption_committed_owner_retention::inspect(
                &storage, &owner, &owner, pending, &trust,
            );
        } else if writer == 8 {
            consumption_owner_retention::inspect(&storage, &owner, &owner, pending, &trust);
        } else {
            consumption_install_retention::inspect(&storage, &owner, &owner, pending, &trust);
        }
        return;
    }
    if let Some(pending) = pending_consumption {
        consumption_retention::inspect(&storage, &owner, &owner, pending, &trust);
        return;
    }
    if writer == 5 {
        let accepted = storage
            .inspect_native_publication_history(owner.id(), &trust)
            .unwrap();
        fs::write(
            root.join("source/note"),
            b"scheduled progress after publication",
        )
        .unwrap();
        let observed = owner
            .project()
            .capture_inputs(ObservationLimits::default())
            .unwrap();
        let saved = storage
            .save_verified_dependency_capture(&owner, &observed, actor, &trust, sign)
            .unwrap();
        let bytes = fs::read(&journal).unwrap();
        let unchanged = storage
            .save_verified_dependency_capture(
                &owner,
                &observed,
                actor,
                &trust,
                |_| -> Result<Signature, &'static str> {
                    panic!("unchanged observation reached signer")
                },
            )
            .unwrap();
        assert_eq!(saved, unchanged);
        assert_eq!(fs::read(&journal).unwrap(), bytes);
        assert!(storage
            .save_verified_dependency_capture(
                &owner,
                &observed,
                actor,
                &TrustedReviewers::default(),
                |_| -> Result<Signature, &'static str> { panic!("missing trust reached signer") }
            )
            .is_err());
        assert_eq!(fs::read(&journal).unwrap(), bytes);
        assert_eq!(
            storage
                .inspect_native_publication_history(owner.id(), &trust)
                .unwrap(),
            accepted
        );
        for interruption in 0..3 {
            fs::write(
                root.join("source/note"),
                format!("pending scheduled {interruption}"),
            )
            .unwrap();
            let observed = owner
                .project()
                .capture_inputs(ObservationLimits::default())
                .unwrap();
            let request = id(91 + interruption);
            let prepared = storage
                .prepare_verified_dependency_capture(
                    &owner, &observed, actor, request, &trust, sign,
                )
                .unwrap();
            let pending_operation = prepared.operation();
            assert!(prepared
                .commit_with_io(
                    |step, file, frames| {
                        if interruption < 2 && step == "staged" {
                            if interruption == 1 {
                                file.write_all(&frames[..frames.len() / 2])?;
                                file.sync_all()?;
                            }
                            return Err(std::io::Error::other("scheduled interruption"));
                        }
                        if interruption == 2 && step == "appended" {
                            return Err(std::io::Error::other("scheduled acknowledgement lost"));
                        }
                        Ok(())
                    },
                    |file| file.sync_all()
                )
                .is_err());
            fs::write(
                root.join("source/note"),
                format!("newer scheduled {interruption}"),
            )
            .unwrap();
            let newer = owner
                .project()
                .capture_inputs(ObservationLimits::default())
                .unwrap();
            let pending_path = owner.metadata_path().join("dependency-capture.pending");
            let pending = fs::read(&pending_path).unwrap();
            let before = fs::read(&journal).unwrap();
            assert!(storage
                .save_verified_dependency_capture(
                    &owner,
                    &newer,
                    actor,
                    &TrustedReviewers::default(),
                    sign
                )
                .is_err());
            assert_eq!(fs::read(&journal).unwrap(), before);
            assert_eq!(fs::read(&pending_path).unwrap(), pending);
            let newer_saved = storage
                .save_verified_dependency_capture(&owner, &newer, actor, &trust, sign)
                .unwrap();
            assert_ne!(newer_saved.operation(), pending_operation);
            assert_eq!(
                storage
                    .recover_verified_dependency_capture(&owner, request, &trust)
                    .unwrap()
                    .operation(),
                pending_operation
            );
            assert!(!pending_path.exists());
        }
        use crate::project_attachment::{
            AttachmentCaptureService, CaptureOutcome, CapturePhase, CaptureSchedule,
        };
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        use std::time::{Duration, Instant};
        struct TestSigner(AtomicUsize);
        impl crate::CheckpointSigner for TestSigner {
            fn public_key(&self) -> PublicKey {
                PublicKey::from_bytes(
                    SigningKey::from_bytes(&[191; 32])
                        .verifying_key()
                        .to_bytes(),
                )
            }
            fn sign(&self, payload: &SigningPayload) -> Result<Signature, String> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(Signature::from_bytes(
                    SigningKey::from_bytes(&[191; 32])
                        .sign(payload.as_bytes())
                        .to_bytes(),
                ))
            }
        }
        let signer = Arc::new(TestSigner(AtomicUsize::new(0)));
        let schedule = CaptureSchedule {
            native_signals: false,
            reconciliation_interval: Duration::from_secs(300),
            ..CaptureSchedule::default()
        };
        let service = AttachmentCaptureService::start_verified_dependency(
            &storage,
            &owner,
            signer.clone(),
            schedule,
            &trust,
        )
        .unwrap();
        let wait = |attempts| {
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                let status = service.status();
                if status.attempts >= attempts && status.phase == CapturePhase::Waiting {
                    break status;
                }
                assert!(
                    Instant::now() < deadline,
                    "scheduled capture did not finish: {status:?}"
                );
                service.wait_for_update(status.revision, Duration::from_millis(100));
            }
        };
        let first = wait(1);
        assert_eq!(first.last_outcome, CaptureOutcome::Unchanged);
        assert_eq!(first.versions_saved, 0);
        assert_eq!(signer.0.load(Ordering::SeqCst), 0);
        fs::write(root.join("source/note"), b"background worker progress").unwrap();
        assert!(service.request_capture());
        let second = wait(2);
        assert_eq!(second.last_outcome, CaptureOutcome::Saved);
        assert_ne!(second.saved_version, first.saved_version);
        assert_eq!(second.versions_saved, 1);
        assert_eq!(signer.0.load(Ordering::SeqCst), 1);
        let before = fs::read(&journal).unwrap();
        assert!(service.request_capture());
        let third = wait(3);
        assert_eq!(third.last_outcome, CaptureOutcome::Unchanged);
        assert_eq!(third.saved_version, second.saved_version);
        assert_eq!(third.versions_saved, 1);
        service.stop_and_join().unwrap();
        assert_eq!(fs::read(&journal).unwrap(), before);
        assert_eq!(
            fs::read(root.join("source/note")).unwrap(),
            b"background worker progress"
        );
        assert_eq!(
            storage
                .inspect_native_publication_history(owner.id(), &trust)
                .unwrap(),
            accepted
        );
        return;
    }
    if writer == 4 {
        let accepted = storage
            .inspect_native_publication_history(owner.id(), &trust)
            .unwrap();
        fs::write(
            root.join("source/note"),
            b"new private progress after publication",
        )
        .unwrap();
        let input = owner
            .project()
            .capture_inputs(ObservationLimits::default())
            .unwrap();
        let journal_before = fs::read(&journal).unwrap();
        assert!(storage
            .prepare_registered_dependency_capture(&owner, &input, actor, id(80), sign)
            .is_err());
        let mut called = false;
        assert!(storage
            .prepare_verified_dependency_capture(
                &owner,
                &input,
                actor,
                id(80),
                &crate::TrustedReviewers::default(),
                |payload| {
                    called = true;
                    sign(payload)
                }
            )
            .is_err());
        assert!(!called, "missing publication trust reached the signer");
        assert_eq!(fs::read(&journal).unwrap(), journal_before);
        let saved = storage
            .prepare_verified_dependency_capture(&owner, &input, actor, id(80), &trust, |payload| {
                let roots = [owner.store.clone(), owner.project().pinned.clone()];
                let (send, receive) = std::sync::mpsc::channel();
                let reader = std::thread::spawn(move || {
                    let guard = crate::workspace_custody::lock_workspace_initialization_set(&roots)
                        .unwrap();
                    send.send(()).unwrap();
                    drop(guard);
                });
                receive
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .expect("signer must run outside custody");
                reader.join().unwrap();
                sign(payload)
            })
            .expect("native capture must remain available after accepted publication")
            .commit()
            .unwrap();
        assert_ne!(saved.operation(), later_version.operation());
        assert_eq!(
            storage
                .inspect_native_publication_history(owner.id(), &trust)
                .unwrap(),
            accepted
        );
        assert_eq!(
            storage
                .inspect_native_saved_version(owner.id(), saved.operation(), &trust)
                .unwrap(),
            saved
        );
        assert!(
            owner.saved_versions().is_err(),
            "private capture must not remove ordinary admission guards"
        );
        fs::write(root.join("source/note"), b"later private progress").unwrap();
        let later_input = owner
            .project()
            .capture_inputs(ObservationLimits::default())
            .unwrap();
        let mut inner_saved = None;
        assert!(
            storage
                .prepare_verified_dependency_capture(
                    &owner,
                    &later_input,
                    actor,
                    id(81),
                    &trust,
                    |payload| {
                        inner_saved = Some(
                            storage
                                .prepare_verified_dependency_capture(
                                    &owner,
                                    &later_input,
                                    actor,
                                    id(82),
                                    &trust,
                                    sign,
                                )
                                .unwrap()
                                .commit()
                                .unwrap(),
                        );
                        sign(payload)
                    }
                )
                .is_err(),
            "a save during signing must stale the outer authoring basis"
        );
        assert_ne!(inner_saved.unwrap().operation(), saved.operation());
        assert_eq!(
            storage
                .inspect_native_publication_history(owner.id(), &trust)
                .unwrap(),
            accepted
        );
        let completed_before_capture = storage
            .inspect_native_history_retention(owner.id(), &trust)
            .unwrap();
        for interruption in 0..3 {
            fs::write(
                root.join("source/note"),
                format!("interrupted capture {interruption}"),
            )
            .unwrap();
            let observed = owner
                .project()
                .capture_inputs(ObservationLimits::default())
                .unwrap();
            let request = id(83 + interruption);
            let prepared = storage
                .prepare_verified_dependency_capture(
                    &owner, &observed, actor, request, &trust, sign,
                )
                .unwrap();
            let operation = prepared.operation();
            assert!(prepared
                .commit_with_io(
                    |step, file, frames| {
                        if interruption < 2 && step == "staged" {
                            if interruption == 1 {
                                file.write_all(&frames[..frames.len() / 2])?;
                                file.sync_all()?;
                            }
                            return Err(io::Error::other("interrupted capture"));
                        }
                        if interruption == 2 && step == "appended" {
                            return Err(io::Error::other("lost capture acknowledgement"));
                        }
                        Ok(())
                    },
                    |file| file.sync_all()
                )
                .is_err());
            let interrupted = fs::read(&journal).unwrap();
            let pending_path = owner.metadata_path().join("dependency-capture.pending");
            let pending = fs::read(&pending_path).unwrap();
            assert!(storage
                .inspect_native_history_retention(owner.id(), &trust)
                .is_err());
            assert_eq!(fs::read(&pending_path).unwrap(), pending);
            assert_eq!(fs::read(&journal).unwrap(), interrupted);
            let retention = storage
                .inspect_verified_dependency_capture_retention(&owner, request, &trust)
                .unwrap();
            assert!(retention.pending());
            assert_eq!(retention.operation(), operation);
            let source_before_inventory = fs::read(root.join("source/note")).unwrap();
            assert_capture_history_inventory(
                &storage,
                &owner,
                request,
                &trust,
                &retention,
                &completed_before_capture,
            );
            assert_eq!(
                fs::read(root.join("source/note")).unwrap(),
                source_before_inventory
            );
            assert_eq!(fs::read(&pending_path).unwrap(), pending);
            assert_eq!(fs::read(&journal).unwrap(), interrupted);
            if interruption == 0 {
                assert!(storage
                    .inspect_native_history_retention_during_capture(&owner, id(90), &trust)
                    .is_err());
                assert!(storage
                    .inspect_native_history_retention_during_capture(
                        &owner,
                        request,
                        &crate::TrustedReviewers::default()
                    )
                    .is_err());
            }
            let retained_json = retention.to_json();
            let Json::Array(payloads) = retained_json.get("payloads").unwrap() else {
                panic!("capture retention payloads missing");
            };
            for digest in [
                operation,
                super::super::dependency_transaction::hash(&receipt),
                super::super::dependency_transaction::hash(
                    format!("interrupted capture {interruption}").as_bytes(),
                ),
            ] {
                assert!(payloads.contains(&Json::text(digest.to_hex())));
            }
            if interruption == 0 {
                let foreign = String::from_utf8(pending.clone()).unwrap().replacen(
                    &request.to_hex(),
                    &id(90).to_hex(),
                    1,
                );
                assert_ne!(foreign.as_bytes(), pending.as_slice());
                fs::write(&pending_path, foreign.as_bytes()).unwrap();
                assert!(storage
                    .inspect_native_history_retention_during_capture(&owner, request, &trust)
                    .is_err());
                assert_eq!(fs::read(&pending_path).unwrap(), foreign.as_bytes());
                assert_eq!(fs::read(&journal).unwrap(), interrupted);
                fs::write(&pending_path, &pending).unwrap();
                let cas = Cas::<_, Blake3>::with_filesystem(
                    owner.metadata_path(),
                    owner.store.filesystem(),
                )
                .unwrap();
                let path = owner.metadata_path().join(
                    cas.layout()
                        .chunk_path(&mesh_cas::Digest32::from_bytes(*operation.as_bytes())),
                );
                let exact = fs::read(&path).unwrap();
                fs::write(&path, b"corrupt capture payload").unwrap();
                assert!(storage
                    .inspect_verified_dependency_capture_retention(&owner, request, &trust)
                    .is_err());
                assert!(storage
                    .inspect_native_history_retention_during_capture(&owner, request, &trust)
                    .is_err());
                assert_eq!(fs::read(&path).unwrap(), b"corrupt capture payload");
                fs::remove_file(&path).unwrap();
                assert!(storage
                    .inspect_verified_dependency_capture_retention(&owner, request, &trust)
                    .is_err());
                assert!(storage
                    .inspect_native_history_retention_during_capture(&owner, request, &trust)
                    .is_err());
                assert!(!path.exists());
                fs::write(&path, exact).unwrap();
                assert_eq!(
                    storage
                        .inspect_verified_dependency_capture_retention(&owner, request, &trust)
                        .unwrap(),
                    retention
                );
            }

            assert!(storage
                .inspect_verified_dependency_capture_retention(
                    &owner,
                    request,
                    &crate::TrustedReviewers::default()
                )
                .is_err());
            assert!(storage
                .inspect_verified_dependency_capture_retention(&owner, id(90), &trust)
                .is_err());
            assert_eq!(fs::read(&journal).unwrap(), interrupted);
            assert_eq!(fs::read(&pending_path).unwrap(), pending);
            let reopened = AttachmentStorage::open(&root.join("metadata")).unwrap();
            assert!(reopened
                .recover_verified_dependency_capture(&owner, id(90), &trust)
                .is_err());
            assert!(reopened
                .recover_verified_dependency_capture(
                    &owner,
                    request,
                    &crate::TrustedReviewers::default()
                )
                .is_err());
            assert_eq!(fs::read(&journal).unwrap(), interrupted);
            assert_eq!(fs::read(&pending_path).unwrap(), pending);
            if interruption == 0 {
                let cas = Cas::<_, Blake3>::with_filesystem(
                    owner.metadata_path(),
                    owner.store.filesystem(),
                )
                .unwrap();
                let mut intent =
                    crate::project_attachment::history::dependency_capture::CaptureIntent::parse(
                        std::str::from_utf8(&pending).unwrap(),
                    )
                    .unwrap();
                let frames =
                    crate::project_attachment::history::dependency_capture::validate_frames(
                        &cas, &intent,
                    )
                    .unwrap();
                let scan = mesh_store::scan_journal(&frames).unwrap();
                for mode in 0..8 {
                    let mut records = scan.records().to_vec();
                    let Some(StoredRecord::Operation(operation)) = records.last_mut() else {
                        panic!("capture operation missing");
                    };
                    match mode {
                        0 => operation.actor_sequence += 1,
                        1 => operation.session = mesh_store::EntityUuid::from_bytes([99; 16]),
                        2 => operation.policy_epoch += 1,
                        3 => operation.hlc_millis += 1,
                        4 => operation.hlc_counter += 1,
                        5 => operation.actor = id(99),
                        6 => operation.parents.clear(),
                        7 => operation.parents = vec![id(99)],
                        _ => unreachable!(),
                    }
                    let forged = records
                        .iter()
                        .flat_map(mesh_store::frame_record)
                        .collect::<Vec<_>>();
                    intent.frames = super::super::dependency_transaction::hash(&forged);
                    cas.promote(forged).unwrap();
                    fs::write(&pending_path, intent.encode()).unwrap();
                    let result =
                        reopened.recover_verified_dependency_capture(&owner, request, &trust);
                    assert_eq!(
                        fs::read(&journal).unwrap(),
                        interrupted,
                        "forged capture metadata must be refused before journal append"
                    );
                    assert!(result.is_err(), "forged capture metadata must refuse");
                    fs::write(&pending_path, &pending).unwrap();
                }
            }
            let recovered = reopened
                .recover_verified_dependency_capture(&owner, request, &trust)
                .unwrap();
            assert_eq!(recovered.operation(), operation);
            let completed = fs::read(&journal).unwrap();
            assert_eq!(
                reopened
                    .recover_verified_dependency_capture(&owner, request, &trust)
                    .unwrap(),
                recovered
            );
            assert_eq!(fs::read(&journal).unwrap(), completed);
            assert!(!pending_path.exists());
            assert_eq!(
                reopened
                    .inspect_native_publication_history(owner.id(), &trust)
                    .unwrap(),
                accepted
            );
        }
        let after_later_saves = fs::read(&journal).unwrap();
        assert_eq!(
            storage
                .recover_verified_dependency_capture(&owner, id(80), &trust)
                .unwrap(),
            saved
        );
        assert_eq!(fs::read(&journal).unwrap(), after_later_saves);
        assert!(owner.inspect_dependency_capture_retention(id(85)).is_err());
        let retained = storage
            .inspect_verified_dependency_capture_retention(&owner, id(85), &trust)
            .unwrap();
        assert!(!retained.pending());
        let history_retention = storage
            .inspect_native_history_retention(owner.id(), &trust)
            .unwrap();
        assert_capture_history_inventory(
            &storage,
            &owner,
            id(85),
            &trust,
            &retained,
            &history_retention,
        );
        assert_eq!(fs::read(&journal).unwrap(), after_later_saves);
        assert_eq!(
            history_retention.get("collection_authority"),
            Some(&Json::Bool(false))
        );
        assert!(storage
            .inspect_native_history_retention(owner.id(), &crate::TrustedReviewers::default())
            .is_err());
        let old_intent =
            crate::project_attachment::history::dependency_capture::CaptureIntent::parse(
                &fs::read_to_string(
                    owner
                        .metadata_path()
                        .join(format!("dependency-capture-{}.json", id(80).to_hex())),
                )
                .unwrap(),
            )
            .unwrap();
        let projection = retained.to_json();
        let Json::Array(payloads) = projection.get("payloads").unwrap() else {
            panic!("retention payloads missing");
        };
        assert!(
            payloads.contains(&Json::text(old_intent.frames.to_hex())),
            "capture retention must preserve historical capture retry frames"
        );
        let Json::Array(history_payloads) = history_retention.get("payloads").unwrap() else {
            panic!("completed history payloads missing");
        };
        assert!(history_payloads.contains(&Json::text(old_intent.frames.to_hex())));
        assert!(
            history_payloads.contains(&Json::text(old_intent.authority.to_hex())),
            "completed history must retain its local enrollment object"
        );

        let Json::Array(history_operations) = history_retention.get("operations").unwrap() else {
            panic!("completed history operations missing");
        };
        assert!(history_operations.contains(&Json::text(saved.operation().to_hex())));
        let receipt_path = owner
            .metadata_path()
            .join(format!("dependency-capture-{}.json", id(80).to_hex()));
        let original_receipt = fs::read(&receipt_path).unwrap();
        let Json::Array(sidecars) = projection.get("receipt_sidecars").unwrap() else {
            panic!("historical capture sidecars missing");
        };
        assert!(sidecars.contains(&Json::object([
            (
                "name",
                Json::text(format!("dependency-capture-{}.json", id(80).to_hex()))
            ),
            (
                "digest",
                Json::text(super::super::dependency_transaction::hash(&original_receipt).to_hex())
            ),
        ])));
        let cas =
            Cas::<_, Blake3>::with_filesystem(owner.metadata_path(), owner.store.filesystem())
                .unwrap();
        let frames_path =
            owner
                .metadata_path()
                .join(cas.layout().chunk_path(&mesh_cas::Digest32::from_bytes(
                    *old_intent.frames.as_bytes(),
                )));
        let original_frames = fs::read(&frames_path).unwrap();
        fs::write(&frames_path, b"corrupt historical retry frames").unwrap();
        assert!(storage
            .inspect_native_history_retention(owner.id(), &trust)
            .is_err());
        assert!(storage
            .inspect_verified_dependency_capture_retention(&owner, id(85), &trust)
            .is_err());
        assert_eq!(
            fs::read(&frames_path).unwrap(),
            b"corrupt historical retry frames"
        );
        fs::remove_file(&frames_path).unwrap();
        assert!(storage
            .inspect_native_history_retention(owner.id(), &trust)
            .is_err());
        assert!(storage
            .inspect_verified_dependency_capture_retention(&owner, id(85), &trust)
            .is_err());
        assert!(!frames_path.exists());
        fs::write(&frames_path, original_frames).unwrap();
        let mut foreign = old_intent;
        foreign.operation = id(99);
        fs::write(&receipt_path, foreign.encode()).unwrap();
        assert!(storage
            .inspect_native_history_retention(owner.id(), &trust)
            .is_err());
        assert!(storage
            .inspect_verified_dependency_capture_retention(&owner, id(85), &trust)
            .is_err());
        assert_eq!(fs::read_to_string(&receipt_path).unwrap(), foreign.encode());
        fs::write(&receipt_path, &original_receipt).unwrap();
        assert_eq!(
            storage
                .inspect_verified_dependency_capture_retention(&owner, id(85), &trust)
                .unwrap(),
            retained
        );
        assert_eq!(
            storage
                .inspect_native_history_retention(owner.id(), &trust)
                .unwrap(),
            history_retention
        );
        assert_eq!(fs::read(&receipt_path).unwrap(), original_receipt);
        assert_eq!(fs::read(&journal).unwrap(), after_later_saves);
        assert_eq!(
            storage
                .recover_verified_dependency_capture(&owner, id(80), &trust)
                .unwrap(),
            saved
        );
        assert_eq!(fs::read(&journal).unwrap(), after_later_saves);
        return;
    }
    let before = fs::read(&journal).unwrap();
    let reopened = AttachmentStorage::open(&root.join("metadata")).unwrap();
    let verified = reopened
        .inspect_root_publication_history(owner.id(), &trust)
        .unwrap();
    let publication = verified.get("publication").unwrap();
    assert_eq!(
        publication.get("operation"),
        Some(&Json::text(version.operation().to_hex()))
    );
    assert_eq!(publication.get("revision"), Some(&Json::Number(1)));
    assert!(reopened
        .inspect_root_publication_history(owner.id(), &TrustedReviewers::default())
        .is_err());
    assert!(
        owner.saved_versions().is_err(),
        "private replay must not grant ordinary admission"
    );
    assert_eq!(fs::read(&journal).unwrap(), before);
    let candidate = reopened
        .inspect_root_review_candidate(owner.id(), later_snapshot.record(), &trust)
        .unwrap();
    assert_eq!(candidate.get("canonical"), publication.get("head"));
    assert!(reopened
        .inspect_root_review_candidate(
            owner.id(),
            later_snapshot.record(),
            &TrustedReviewers::default()
        )
        .is_err());
    let (output, prior_record) = reopened
        .with_root_publication_history(owner.id(), &trust, |history, _, evidence, _| {
            assert_eq!(
                history
                    .approval_context(&evidence.policy().bound_review(bound.record()).unwrap())
                    .unwrap(),
                preview
            );
            Ok((
                evidence
                    .policy()
                    .review_evidence(later_snapshot.record())
                    .unwrap()
                    .output(),
                history.verified_publication().unwrap().record.payload,
            ))
        })
        .unwrap();
    let next_review = append_test_policy(
        &owner,
        DependencyKind::ReviewSnapshot,
        "mesh.dependency-policy/v4",
        Json::object([
            ("request", Json::text(id(8).to_hex())),
            ("revision", Json::Number(1)),
            ("snapshot", Json::text(later_snapshot.record().to_hex())),
            (
                "output",
                Json::Array(vec![
                    Json::Array(vec![
                        Json::text(output.0.to_hex()),
                        Json::text(output.1.to_hex()),
                    ]),
                    Json::text(output.2.to_hex()),
                ]),
            ),
            ("canonical", candidate.get("canonical").unwrap().clone()),
            ("bundle", candidate.get("bundle").unwrap().clone()),
            ("opener", Json::text(id(4).to_hex())),
        ]),
        None,
    );
    let second_context = reopened
        .with_root_publication_history(owner.id(), &trust, |history, _, evidence, _| {
            history
                .approval_context(&evidence.policy().bound_review(next_review).unwrap())
                .map_err(error)
        })
        .unwrap();
    assert_ne!(
        second_context.reviewed_actor_head(),
        preview.reviewed_actor_head()
    );
    let draft = HumanApprovalReceiptDraft::new(
        ExpectedHumanApproval::new(second_context.clone(), credential.clone(), [62; 32]),
        ApprovalDecision::Approve,
    );
    let signature = key.sign(&rng, &draft.canonical_bytes()).unwrap();
    let receipt = draft
        .with_signature(signature.as_ref().to_vec())
        .unwrap()
        .canonical_bytes();
    if writer != 0 {
        if writer >= 2 {
            let result = restart::interrupted_publication(
                &root,
                owner.id(),
                id(9),
                next_review,
                &receipt,
                key.public_key().as_ref(),
                &journal,
                "last",
                || {
                    if writer == 3 {
                        let error = storage
                            .decide_native_saved_input(
                                owner.id(),
                                version.operation(),
                                crate::project_attachment::NativeSavedInputDecision::Rejected,
                                None,
                                id(98),
                                &trust,
                            )
                            .unwrap_err();
                        assert!(
                            error.to_string().contains("pending")
                                && error.to_string().contains("publication"),
                            "{error}"
                        );
                        assert!(
                            owner
                                .decide_saved_input(
                                    version,
                                    crate::project_attachment::SavedInputDecision::Rejected,
                                    None,
                                    id(99)
                                )
                                .is_err(),
                            "an ordinary decision overtook a staged publication"
                        );
                        assert!(
                            owner.saved_versions().is_err(),
                            "ordinary native reads admitted unresolved publication"
                        );
                    }
                },
            );
            assert_eq!(
                result.get("head").and_then(Json::as_text),
                Some(
                    RecordDigest::from_bytes(*second_context.reviewed_actor_head().as_bytes())
                        .to_hex()
                        .as_str()
                )
            );
        } else {
            use super::super::dependency_private_context::publication::Step;
            use std::io::Write as _;
            let before = fs::read(&journal).unwrap();
            let failure = storage
                .commit_native_publication_with_io(
                    owner.id(),
                    id(9),
                    next_review,
                    &receipt,
                    &trust,
                    |step, file, frame| {
                        if matches!(step, Step::Staged) {
                            file.write_all(&frame[..frame.len() - 1])?;
                            file.sync_all()?;
                            return Err(std::io::Error::other("root publication torn boundary"));
                        }
                        Ok(())
                    },
                    |file| file.sync_all(),
                )
                .unwrap_err();
            assert!(failure
                .to_string()
                .contains("root publication torn boundary"));
            let torn = fs::read(&journal).unwrap();
            assert!(torn.starts_with(&before) && torn.len() > before.len());
            assert!(storage
                .inspect_native_publication_history(owner.id(), &trust)
                .is_err());
            assert_eq!(fs::read(&journal).unwrap(), torn);
        }
        let committed = storage
            .commit_native_publication(owner.id(), id(9), next_review, &receipt, &trust)
            .unwrap();
        assert_eq!(committed.revision(), 2);
        assert_eq!(
            committed.head().as_bytes(),
            second_context.reviewed_actor_head().as_bytes()
        );
    } else {
        append_test_policy(
            &owner,
            DependencyKind::Publication,
            "mesh.dependency-policy/v5",
            Json::object([
                ("request", Json::text(id(9).to_hex())),
                ("revision", Json::Number(2)),
                ("previous", Json::text(prior_record.to_hex())),
                ("review", Json::text(next_review.to_hex())),
                (
                    "receipt",
                    Json::text(super::super::dependency_transaction::hash(&receipt).to_hex()),
                ),
                (
                    "result",
                    Json::text(
                        RecordDigest::from_bytes(*second_context.reviewed_actor_head().as_bytes())
                            .to_hex(),
                    ),
                ),
                (
                    "credential",
                    Json::text(RecordDigest::from_bytes(*credential.id().as_bytes()).to_hex()),
                ),
                ("challenge", Json::text(id(62).to_hex())),
            ]),
            Some(receipt),
        );
    }
    let reopened_again = AttachmentStorage::open(&root.join("metadata")).unwrap();
    let second = reopened_again
        .inspect_root_publication_history(owner.id(), &trust)
        .unwrap();
    assert_eq!(
        second.get("publication").unwrap().get("operation"),
        Some(&Json::text(later_version.operation().to_hex()))
    );
    assert_eq!(
        second.get("publication").unwrap().get("revision"),
        Some(&Json::Number(2))
    );
    let after_second = fs::read(&journal).unwrap();
    reopened_again
        .with_root_publication_history(owner.id(), &trust, |history, _, evidence, _| {
            assert_eq!(
                history
                    .approval_context(&evidence.policy().bound_review(bound.record()).unwrap())
                    .unwrap(),
                preview
            );
            assert_eq!(
                history
                    .approval_context(&evidence.policy().bound_review(next_review).unwrap())
                    .unwrap(),
                second_context
            );
            Ok(())
        })
        .unwrap();
    assert!(
        reopened_again
            .inspect_root_review_candidate(owner.id(), snapshot.record(), &trust)
            .is_err(),
        "an old output cannot discard accepted main"
    );
    assert_eq!(fs::read(&journal).unwrap(), after_second);
    for (operation, expected) in [
        (version.operation(), b"first private version".as_slice()),
        (
            later_version.operation(),
            b"later private version".as_slice(),
        ),
    ] {
        let bytes = reopened_again
            .with_root_saved_input(owner.id(), operation, &trust, |input| {
                let files = input.files().collect::<Vec<_>>();
                assert_eq!(files.len(), 1);
                assert_eq!(files[0].path, "note");
                assert_eq!(files[0].byte_length, expected.len() as u64);
                let mut bytes = Vec::new();
                input.write_file("note", &mut bytes)?;
                assert!(input.write_file("../note", &mut Vec::new()).is_err());
                Ok(bytes)
            })
            .unwrap();
        assert_eq!(bytes, expected);
    }
    let mut called = false;
    assert!(reopened_again
        .with_root_saved_input(
            owner.id(),
            version.operation(),
            &TrustedReviewers::default(),
            |_| {
                called = true;
                Ok(())
            }
        )
        .is_err());
    assert!(
        !called,
        "missing trust must refuse before exposing saved input"
    );
    assert!(reopened_again
        .with_root_saved_input(owner.id(), id(250), &trust, |_| {
            called = true;
            Ok(())
        })
        .is_err());
    assert!(
        !called,
        "an unknown saved operation must not invoke the reader"
    );
    assert_eq!(fs::read(&journal).unwrap(), after_second);
    fs::OpenOptions::new()
        .append(true)
        .open(&journal)
        .unwrap()
        .write_all(&[1])
        .unwrap();
    let torn = fs::read(&journal).unwrap();
    assert!(reopened
        .inspect_root_publication_history(owner.id(), &trust)
        .is_err());
    assert_eq!(
        fs::read(&journal).unwrap(),
        torn,
        "read must not repair a torn publication history"
    );
}

// This durable fixture creates no production writer or human-presence claim.
fn append_test_policy(
    owner: &crate::project_attachment::ProvisionedAttachment,
    kind: DependencyKind,
    schema: &str,
    body: Json,
    receipt: Option<Vec<u8>>,
) -> RecordDigest {
    let _guard = crate::workspace_custody::lock_workspace_initialization(&owner.store).unwrap();
    let (_, evidence) = owner
        .project()
        .read_publication_private_history(owner.metadata_path(), &owner.store)
        .unwrap();
    let (ordinal, previous) = evidence.policy().native_head().unwrap();
    let cas =
        Cas::<_, Blake3>::with_filesystem(owner.metadata_path(), owner.store.filesystem()).unwrap();
    if let Some(receipt) = receipt {
        cas.promote(receipt).unwrap();
    }
    let bytes = Json::object([
        ("schema", Json::text(schema)),
        (
            "authority",
            Json::text(evidence.binding().authority.to_hex()),
        ),
        ("revision", Json::Number(ordinal + 1)),
        ("previous", Json::text(previous.to_hex())),
        ("kind", Json::Number(kind.code().into())),
        ("body", body),
    ])
    .encode()
    .into_bytes();
    let record = DependencyRecord {
        authority: evidence.binding().authority,
        revision: ordinal + 1,
        previous,
        payload: super::super::dependency_transaction::hash(&bytes),
        kind,
    };
    evidence.policy().clone().apply(record, &bytes).unwrap();
    cas.promote(bytes).unwrap();
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(
            owner
                .metadata_path()
                .join(crate::workspace::RECORD_FILE_NAME),
        )
        .unwrap();
    file.write_all(&mesh_store::frame_record(&StoredRecord::Dependency(record)))
        .unwrap();
    file.sync_all().unwrap();
    record.payload
}

#[test]
fn consumed_publications_reopen_with_exact_trust_and_prior_main() {
    consumed_publication_fixture(0);
}
#[test]
fn native_writer_commits_consumed_publications_and_recovers_completed_retries() {
    consumed_publication_fixture(1);
}
#[test]
fn native_writer_recovers_exact_torn_publication_and_refuses_foreign_retries() {
    consumed_publication_fixture(2);
}
#[test]
fn native_writer_refuses_new_publication_after_input_rejection() {
    consumed_publication_fixture(3);
}
#[test]
fn consumed_publication_recovers_after_process_exit_and_lost_acknowledgement() {
    consumed_publication_fixture(4);
}
#[test]
fn native_saved_review_after_publication_recovers_across_processes() {
    consumed_publication_fixture(5);
}
#[test]
fn native_snapshot_after_publication_preserves_graph_and_historical_decisions() {
    consumed_publication_fixture(6);
}
#[test]
fn consumed_capture_after_publication_recovers_without_changing_owner() {
    consumed_publication_fixture(7);
}
#[test]
fn native_consumption_start_retention_after_consumed_publication() {
    consumed_publication_fixture(8);
}
#[test]
fn native_consumption_install_retention_after_consumed_publication() {
    consumed_publication_fixture(9);
}
#[test]
fn native_consumption_owner_retention_after_consumed_publication() {
    consumed_publication_fixture(10);
}
#[test]
fn native_consumption_committed_owner_retention_after_consumed_publication() {
    consumed_publication_fixture(11);
}
#[test]
fn native_trusted_grants_after_consumed_publication() {
    consumed_publication_fixture(12);
}
#[test]
fn native_reservations_after_consumed_publication() {
    consumed_publication_fixture(13);
}
#[test]
fn native_consumed_start_after_consumed_publication() {
    consumed_publication_fixture(14);
}
#[test]
fn native_consumed_checkpoint_after_consumed_publication() {
    consumed_publication_fixture(15);
}
#[test]
fn native_consumption_owner_after_consumed_publication() {
    consumed_publication_fixture(16);
}
#[test]
fn native_consumption_completion_after_consumed_publication() {
    consumed_publication_fixture(17);
}
#[test]
fn native_completed_versions_after_consumed_publication() {
    consumed_publication_fixture(18);
}
fn consumed_publication_fixture(writer: u8) {
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
        "mesh-consumed-publication-replay-{writer}-{}",
        std::process::id()
    ));
    fs::create_dir(&root).unwrap();
    let _cleanup = Cleanup(root.clone(), false);
    fs::create_dir(root.join("source")).unwrap();
    fs::create_dir(root.join("metadata")).unwrap();
    fs::write(root.join("source/note"), b"exact native input").unwrap();
    if matches!(writer, 9..=11) {
        consumption_install_retention::add_source_entries(&root);
    }
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
    let native_grant_destination =
        (writer == 12).then(|| native_grant::prepare(&storage, &owner, &destination, consumed));
    let pending_consumption = (writer == 8)
        .then(|| consumption_retention::prepare(&storage, &owner, &destination, consumed));
    let installing = matches!(writer, 9..=11)
        .then(|| consumption_install_retention::prepare(&storage, &owner, &destination, consumed));
    let initial_decision = owner
        .decide_saved_input(
            version,
            super::super::SavedInputDecision::Eligible,
            None,
            id(11),
        )
        .unwrap();
    let first_snapshot = storage
        .save_dependency_review_snapshot(&owner, &destination, consumed, &[], id(12))
        .unwrap();
    let first_review = storage
        .save_dependency_review(
            &owner,
            NativeSavedReviewRequest {
                source: &destination,
                version: consumed,
                snapshot: first_snapshot,
                request: id(13),
                opener: id(14),
            },
            &[],
        )
        .unwrap();
    let first_context = storage
        .saved_dependency_review_preview(destination.id(), first_review.record())
        .unwrap()
        .0;
    fs::write(
        destination.project().root().join("note"),
        b"second saved child version",
    )
    .unwrap();
    let captured = destination
        .project()
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    let second_version = storage
        .prepare_registered_dependency_capture(&destination, &captured, actor, id(15), sign)
        .unwrap()
        .commit()
        .unwrap();
    let early_second_snapshot = if writer == 6 {
        None
    } else {
        Some(
            storage
                .save_dependency_review_snapshot(&owner, &destination, second_version, &[], id(16))
                .unwrap(),
        )
    };
    let rng = SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &rng).unwrap();
    let human_key =
        EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &rng).unwrap();
    let credential = HumanApprovalCredential::from_public_key(
        human_key.public_key().as_ref().try_into().unwrap(),
    )
    .unwrap();
    let trust = crate::TrustedReviewers::with_human_credentials([credential.clone()]);
    let append_publication = |review: RecordDigest,
                              revision: u64,
                              previous: RecordDigest,
                              context: mesh_approval::HumanApprovalContext,
                              challenge: u8| {
        let draft = HumanApprovalReceiptDraft::new(
            ExpectedHumanApproval::new(context.clone(), credential.clone(), [challenge; 32]),
            ApprovalDecision::Approve,
        );
        let signature = human_key.sign(&rng, &draft.canonical_bytes()).unwrap();
        let receipt = draft
            .with_signature(signature.as_ref().to_vec())
            .unwrap()
            .canonical_bytes();
        if matches!(writer, 6..=13) {
            // Snapshot/capture fixtures use real publication; earlier writer fixtures retain
            // the complete publication fault/refusal campaign without repeating it here.
            let committed = storage
                .commit_native_publication(
                    destination.id(),
                    id(challenge),
                    review,
                    &receipt,
                    &trust,
                )
                .unwrap();
            assert_eq!(committed.revision(), revision);
            return committed.record();
        }
        if writer != 0 {
            use super::super::dependency_private_context::publication::Step;
            let journal = owner.metadata_path().join(crate::RECORD_FILE_NAME);
            let before = fs::read(&journal).unwrap();
            if writer == 3 {
                let result = restart::interrupted_native_decision(
                    &root,
                    owner.id(),
                    version.operation(),
                    initial_decision.record(),
                    id(99),
                    human_key.public_key().as_ref(),
                    &journal,
                    false,
                    "first",
                );
                assert_eq!(result.get("revision").and_then(Json::as_u64), Some(2));
                let rejected = fs::read(&journal).unwrap();
                assert!(storage
                    .commit_native_publication(
                        destination.id(),
                        id(challenge),
                        review,
                        &receipt,
                        &trust
                    )
                    .is_err());
                assert_eq!(
                    fs::read(&journal).unwrap(),
                    rejected,
                    "rejected input must refuse a new publication without altering history"
                );
                assert!(!owner
                    .metadata_path()
                    .join("native-publication.pending")
                    .exists());
                return id(0);
            }

            assert!(storage
                .commit_native_publication(
                    destination.id(),
                    id(challenge),
                    review,
                    &receipt,
                    &crate::TrustedReviewers::default()
                )
                .is_err());
            assert!(storage
                .commit_native_publication(owner.id(), id(challenge), review, &receipt, &trust)
                .is_err());
            assert_eq!(fs::read(&journal).unwrap(), before);
            if writer == 4 {
                let result = restart::interrupted_publication(
                    &root,
                    destination.id(),
                    id(challenge),
                    review,
                    &receipt,
                    human_key.public_key().as_ref(),
                    &journal,
                    "middle",
                    || {},
                );
                let completed = fs::read(&journal).unwrap();
                let recovered = storage
                    .commit_native_publication(
                        destination.id(),
                        id(challenge),
                        review,
                        &receipt,
                        &trust,
                    )
                    .unwrap();
                assert_eq!(
                    result.get("record").and_then(Json::as_text),
                    Some(recovered.record().to_hex().as_str())
                );
                assert_eq!(
                    result.get("head").and_then(Json::as_text),
                    Some(recovered.head().to_hex().as_str())
                );
                assert_eq!(
                    result.get("revision").and_then(Json::as_u64),
                    Some(recovered.revision())
                );
                assert_eq!(fs::read(&journal).unwrap(), completed);
                storage
                    .inspect_native_publication_history(destination.id(), &trust)
                    .unwrap();
                assert!(!owner
                    .metadata_path()
                    .join("native-publication.pending")
                    .exists());
                return id(0);
            }
            if writer == 2 {
                let failure = storage
                    .commit_native_publication_with_io(
                        destination.id(),
                        id(challenge),
                        review,
                        &receipt,
                        &trust,
                        |step, file, frame| {
                            if matches!(step, Step::Staged) {
                                file.write_all(&frame[..frame.len() / 2])?;
                                file.sync_all()?;
                                return Err(std::io::Error::other("interrupted publication frame"));
                            }
                            Ok(())
                        },
                        |file| file.sync_all(),
                    )
                    .unwrap_err();
                assert!(failure
                    .to_string()
                    .contains("interrupted publication frame"));
                let torn = fs::read(&journal).unwrap();
                assert!(torn.starts_with(&before) && torn.len() > before.len());
                assert!(storage
                    .inspect_native_publication_history(destination.id(), &trust)
                    .is_err());
                for (request, receipt_bytes, reviewers) in [
                    (
                        id(challenge),
                        receipt.as_slice(),
                        crate::TrustedReviewers::default(),
                    ),
                    (id(99), receipt.as_slice(), trust.clone()),
                    (id(challenge), b"foreign receipt".as_slice(), trust.clone()),
                ] {
                    assert!(storage
                        .commit_native_publication(
                            destination.id(),
                            request,
                            review,
                            receipt_bytes,
                            &reviewers
                        )
                        .is_err());
                    assert_eq!(
                        fs::read(&journal).unwrap(),
                        torn,
                        "refused recovery changed the journal"
                    );
                }
                let recovered = storage
                    .commit_native_publication(
                        destination.id(),
                        id(challenge),
                        review,
                        &receipt,
                        &trust,
                    )
                    .unwrap();
                let completed = fs::read(&journal).unwrap();
                assert!(completed.starts_with(&torn) && completed.len() > torn.len());
                assert_eq!(
                    storage
                        .commit_native_publication(
                            destination.id(),
                            id(challenge),
                            review,
                            &receipt,
                            &trust
                        )
                        .unwrap(),
                    recovered
                );
                assert_eq!(
                    fs::read(&journal).unwrap(),
                    completed,
                    "exact retry appended twice"
                );
                storage
                    .inspect_native_publication_history(destination.id(), &trust)
                    .unwrap();
                assert!(!owner
                    .metadata_path()
                    .join("native-publication.pending")
                    .exists());
                return id(0);
            }
            let failure = storage
                .commit_native_publication_with_io(
                    destination.id(),
                    id(challenge),
                    review,
                    &receipt,
                    &trust,
                    |step, _, _| {
                        if matches!(step, Step::Staged) {
                            Err(std::io::Error::other("staged publication interruption"))
                        } else {
                            Ok(())
                        }
                    },
                    |file| file.sync_all(),
                )
                .unwrap_err();
            assert!(failure
                .to_string()
                .contains("staged publication interruption"));
            assert_eq!(
                fs::read(&journal).unwrap(),
                before,
                "staging alone cannot commit approval"
            );
            let failure = storage
                .commit_native_publication_with_io(
                    destination.id(),
                    id(challenge),
                    review,
                    &receipt,
                    &trust,
                    |step, _, _| {
                        if matches!(step, Step::Appended) {
                            Err(std::io::Error::other("lost publication acknowledgement"))
                        } else {
                            Ok(())
                        }
                    },
                    |file| file.sync_all(),
                )
                .unwrap_err();
            assert!(failure
                .to_string()
                .contains("lost publication acknowledgement"));
            let committed = fs::read(&journal).unwrap();
            assert!(committed.len() > before.len());
            let result = storage
                .commit_native_publication(
                    destination.id(),
                    id(challenge),
                    review,
                    &receipt,
                    &trust,
                )
                .unwrap();
            assert_eq!(result.revision(), revision);
            assert_eq!(
                result.head().as_bytes(),
                context.reviewed_actor_head().as_bytes()
            );
            assert_eq!(
                storage
                    .commit_native_publication(
                        destination.id(),
                        id(challenge),
                        review,
                        &receipt,
                        &trust
                    )
                    .unwrap(),
                result
            );
            assert_eq!(
                fs::read(&journal).unwrap(),
                committed,
                "retry must not append twice"
            );
            assert!(!owner
                .metadata_path()
                .join("native-publication.pending")
                .exists());

            let other_draft = HumanApprovalReceiptDraft::new(
                ExpectedHumanApproval::new(
                    context.clone(),
                    credential.clone(),
                    [challenge + 1; 32],
                ),
                ApprovalDecision::Approve,
            );
            let other_signature = human_key
                .sign(&rng, &other_draft.canonical_bytes())
                .unwrap();
            let other_receipt = other_draft
                .with_signature(other_signature.as_ref().to_vec())
                .unwrap()
                .canonical_bytes();
            assert!(
                storage
                    .commit_native_publication(
                        destination.id(),
                        id(challenge),
                        review,
                        &other_receipt,
                        &trust
                    )
                    .is_err(),
                "a different valid ceremony must not reuse an accepted request"
            );
            assert_eq!(fs::read(&journal).unwrap(), committed);
            let mut wrong = receipt.clone();
            let last = wrong.len() - 1;
            wrong[last] ^= 1;
            assert!(storage
                .commit_native_publication(destination.id(), id(challenge), review, &wrong, &trust)
                .is_err());
            assert_eq!(fs::read(&journal).unwrap(), committed);
            return result.record();
        }
        append_test_policy(
            &owner,
            DependencyKind::Publication,
            "mesh.dependency-policy/v5",
            Json::object([
                ("request", Json::text(id(challenge).to_hex())),
                ("revision", Json::Number(revision)),
                ("previous", Json::text(previous.to_hex())),
                ("review", Json::text(review.to_hex())),
                (
                    "receipt",
                    Json::text(super::super::dependency_transaction::hash(&receipt).to_hex()),
                ),
                (
                    "result",
                    Json::text(
                        RecordDigest::from_bytes(*context.reviewed_actor_head().as_bytes())
                            .to_hex(),
                    ),
                ),
                (
                    "credential",
                    Json::text(RecordDigest::from_bytes(*credential.id().as_bytes()).to_hex()),
                ),
                ("challenge", Json::text(id(challenge).to_hex())),
            ]),
            Some(receipt),
        )
    };
    let first_record =
        append_publication(first_review.record(), 1, id(0), first_context.clone(), 71);
    if writer == 18 {
        native_completed_history::inspect(
            &storage,
            &owner,
            &destination,
            consumed,
            &trust,
            human_key.public_key().as_ref(),
            b"exact native input",
        );
        return;
    }
    if writer == 17 {
        native_consumption_completion::inspect(
            &storage,
            &owner,
            &destination,
            consumed,
            &trust,
            human_key.public_key().as_ref(),
            b"exact native input",
        );
        return;
    }
    if writer == 16 {
        native_consumption_owner::inspect(
            &storage,
            &owner,
            &destination,
            consumed,
            &trust,
            human_key.public_key().as_ref(),
            b"exact native input",
        );
        return;
    }
    if writer == 15 {
        native_consumption_checkpoint::inspect(
            &storage,
            &owner,
            &destination,
            consumed,
            &trust,
            human_key.public_key().as_ref(),
            b"exact native input",
        );
        return;
    }
    if writer == 14 {
        native_consumption_start::inspect(
            &storage,
            &owner,
            &destination,
            consumed,
            &trust,
            human_key.public_key().as_ref(),
            b"exact native input",
        );
        return;
    }
    if writer == 13 {
        native_reservation::inspect(
            &storage,
            &owner,
            &destination,
            consumed,
            &trust,
            human_key.public_key().as_ref(),
        );
        return;
    }
    if let Some(grant_destination) = native_grant_destination {
        native_grant::inspect(
            &storage,
            &owner,
            &destination,
            consumed,
            &grant_destination,
            &trust,
            human_key.public_key().as_ref(),
        );
        return;
    }
    if let Some(pending) = installing {
        if writer == 11 {
            consumption_committed_owner_retention::inspect(
                &storage,
                &owner,
                &destination,
                pending,
                &trust,
            );
        } else if writer == 10 {
            consumption_owner_retention::inspect(&storage, &owner, &destination, pending, &trust);
        } else {
            consumption_install_retention::inspect(&storage, &owner, &destination, pending, &trust);
        }
        return;
    }
    if let Some(pending) = pending_consumption {
        consumption_retention::inspect(&storage, &owner, &destination, pending, &trust);
        return;
    }
    if writer == 7 {
        let owner_journal = owner.metadata_path().join(crate::RECORD_FILE_NAME);
        let child_journal = destination.metadata_path().join(crate::RECORD_FILE_NAME);
        let owner_before = fs::read(&owner_journal).unwrap();
        let accepted = storage
            .inspect_native_publication_history(destination.id(), &trust)
            .unwrap();
        let mut first_capture = None;
        let completed_before_capture = storage
            .inspect_native_history_retention(destination.id(), &trust)
            .unwrap();
        for interruption in 0..4 {
            fs::write(
                destination.project().root().join("note"),
                format!("child capture after publication {interruption}"),
            )
            .unwrap();
            let observed = destination
                .project()
                .capture_inputs(ObservationLimits::default())
                .unwrap();
            let request = id(80 + interruption);
            assert!(storage
                .prepare_registered_dependency_capture(
                    &destination,
                    &observed,
                    actor,
                    request,
                    sign
                )
                .is_err());
            let prepared = storage
                .prepare_verified_dependency_capture(
                    &destination,
                    &observed,
                    actor,
                    request,
                    &trust,
                    sign,
                )
                .unwrap();
            let operation = prepared.operation();
            let saved = if interruption == 0 {
                prepared.commit().unwrap()
            } else {
                assert!(prepared
                    .commit_with_io(
                        |step, file, frames| {
                            if interruption < 3 && step == "staged" {
                                if interruption == 2 {
                                    file.write_all(&frames[..frames.len() / 2])?;
                                    file.sync_all()?;
                                }
                                return Err(io::Error::other("interrupted child capture"));
                            }
                            if interruption == 3 && step == "appended" {
                                return Err(io::Error::other("lost child capture acknowledgement"));
                            }
                            Ok(())
                        },
                        |file| file.sync_all()
                    )
                    .is_err());
                let before = fs::read(&child_journal).unwrap();
                let pending_path = destination
                    .metadata_path()
                    .join("dependency-capture.pending");
                let pending = fs::read(&pending_path).unwrap();
                assert!(storage
                    .inspect_native_history_retention(destination.id(), &trust)
                    .is_err());
                assert_eq!(fs::read(&pending_path).unwrap(), pending);
                assert_eq!(fs::read(&child_journal).unwrap(), before);
                let retained = storage
                    .inspect_verified_dependency_capture_retention(&destination, request, &trust)
                    .unwrap();
                assert!(retained.pending());
                assert_eq!(retained.operation(), operation);
                let source_before_inventory =
                    fs::read(destination.project().root().join("note")).unwrap();
                assert_capture_history_inventory(
                    &storage,
                    &destination,
                    request,
                    &trust,
                    &retained,
                    &completed_before_capture,
                );
                assert_eq!(
                    fs::read(destination.project().root().join("note")).unwrap(),
                    source_before_inventory
                );
                if interruption == 1 {
                    assert!(storage
                        .inspect_native_history_retention_during_capture(
                            &destination,
                            id(90),
                            &trust
                        )
                        .is_err());
                    assert!(storage
                        .inspect_native_history_retention_during_capture(
                            &destination,
                            request,
                            &crate::TrustedReviewers::default()
                        )
                        .is_err());
                }
                assert_eq!(fs::read(&child_journal).unwrap(), before);
                assert_eq!(fs::read(&pending_path).unwrap(), pending);
                let reopened = AttachmentStorage::open(&root.join("metadata")).unwrap();
                assert!(reopened
                    .recover_verified_dependency_capture(&destination, id(90), &trust)
                    .is_err());
                assert!(reopened
                    .recover_verified_dependency_capture(
                        &destination,
                        request,
                        &crate::TrustedReviewers::default()
                    )
                    .is_err());
                assert_eq!(fs::read(&child_journal).unwrap(), before);
                assert_eq!(fs::read(&pending_path).unwrap(), pending);
                reopened
                    .recover_verified_dependency_capture(&destination, request, &trust)
                    .unwrap()
            };
            assert_eq!(saved.operation(), operation);
            let retained = storage
                .inspect_verified_dependency_capture_retention(&destination, request, &trust)
                .unwrap();
            assert!(!retained.pending());
            assert_eq!(retained.operation(), operation);
            let history_retention = storage
                .inspect_native_history_retention(destination.id(), &trust)
                .unwrap();
            assert_eq!(
                history_retention.get("collection_authority"),
                Some(&Json::Bool(false))
            );
            let Json::Array(objects) = history_retention.get("payloads").unwrap() else {
                panic!("consumed history objects missing");
            };
            assert!(objects.contains(&Json::text(operation.to_hex())));
            let Json::Array(sidecars) = history_retention.get("sidecars").unwrap() else {
                panic!("consumed history receipts missing");
            };
            use crate::project_attachment::dependency_transaction::{digest, hash, text};
            for name in [
                "consumption-start.pending",
                "consumption-history.pending",
                "consumption-complete.pending",
                "consumption-owner.pending",
            ] {
                let raw = fs::read(destination.metadata_path().join(name)).unwrap();
                assert!(
                    sidecars.contains(&Json::object([
                        ("name", Json::text(name)),
                        ("digest", Json::text(hash(&raw).to_hex())),
                    ])),
                    "missing exact consumed receipt {name}"
                );
            }
            let local_cas = Cas::<_, Blake3>::with_filesystem(
                destination.metadata_path(),
                destination.store.filesystem().read_only(),
            )
            .unwrap();
            let read_json = |id: RecordDigest| {
                assert!(
                    objects.contains(&Json::text(id.to_hex())),
                    "missing consumed object {id:?}"
                );
                let raw = fs::read(
                    destination.metadata_path().join(
                        local_cas
                            .layout()
                            .chunk_path(&mesh_cas::Digest32::from_bytes(*id.as_bytes())),
                    ),
                )
                .unwrap();
                assert_eq!(hash(&raw), id);
                Json::parse(std::str::from_utf8(&raw).unwrap()).unwrap()
            };
            let start_intent = Json::parse(
                &fs::read_to_string(
                    destination
                        .metadata_path()
                        .join("consumption-start.pending"),
                )
                .unwrap(),
            )
            .unwrap();
            let start = read_json(digest(text(&start_intent, "payload").unwrap()).unwrap());
            assert!(
                objects.contains(&Json::text(text(&start, "authority").unwrap())),
                "consumed history must retain its own local enrollment object"
            );
            let descriptor =
                read_json(digest(text(start.get("body").unwrap(), "staged").unwrap()).unwrap());
            let stage = read_json(digest(text(&descriptor, "stage").unwrap()).unwrap());
            let frames = digest(text(&stage, "frames").unwrap()).unwrap();
            assert!(objects.contains(&Json::text(frames.to_hex())));
            assert!(!objects.contains(&Json::text(grant.record().to_hex())));
            let identity = destination.store.identity().unwrap();
            assert_eq!(
                history_retention.get("device"),
                Some(&Json::text(format!("{:x}", identity.0)))
            );
            assert_eq!(
                history_retention.get("inode"),
                Some(&Json::text(format!("{:x}", identity.1)))
            );
            let complete = fs::read(&child_journal).unwrap();
            assert_eq!(
                storage
                    .recover_verified_dependency_capture(&destination, request, &trust)
                    .unwrap(),
                saved
            );
            assert_eq!(fs::read(&child_journal).unwrap(), complete);
            assert_eq!(
                storage
                    .inspect_native_saved_version(destination.id(), operation, &trust)
                    .unwrap(),
                saved
            );
            assert_eq!(
                storage
                    .inspect_native_publication_history(destination.id(), &trust)
                    .unwrap(),
                accepted
            );
            assert_eq!(fs::read(&owner_journal).unwrap(), owner_before);
            first_capture.get_or_insert(saved);
        }
        let complete = fs::read(&child_journal).unwrap();
        assert_eq!(
            storage
                .recover_verified_dependency_capture(&destination, id(80), &trust)
                .unwrap(),
            first_capture.unwrap()
        );
        assert_eq!(fs::read(&child_journal).unwrap(), complete);
        fs::write(
            destination.project().root().join("note"),
            b"scheduled consumed progress",
        )
        .unwrap();
        let observed = destination
            .project()
            .capture_inputs(ObservationLimits::default())
            .unwrap();
        let saved = storage
            .save_verified_dependency_capture(&destination, &observed, actor, &trust, sign)
            .unwrap();
        let after = fs::read(&child_journal).unwrap();
        assert_eq!(
            storage
                .save_verified_dependency_capture(
                    &destination,
                    &observed,
                    actor,
                    &trust,
                    |_| -> Result<Signature, &'static str> {
                        panic!("unchanged consumed capture signed")
                    }
                )
                .unwrap(),
            saved
        );
        assert_eq!(fs::read(&child_journal).unwrap(), after);
        assert_eq!(fs::read(&owner_journal).unwrap(), owner_before);
        assert_eq!(
            storage
                .inspect_native_publication_history(destination.id(), &trust)
                .unwrap(),
            accepted
        );
        return;
    }
    if matches!(writer, 2..=4) {
        return;
    }
    let owner_journal = owner.metadata_path().join(crate::RECORD_FILE_NAME);
    let child_journal = destination.metadata_path().join(crate::RECORD_FILE_NAME);
    let child_before = fs::read(&child_journal).unwrap();
    let reopened = AttachmentStorage::open(&root.join("metadata")).unwrap();
    let first = reopened
        .inspect_native_publication_history(destination.id(), &trust)
        .unwrap();
    assert_eq!(
        first.get("publication").unwrap().get("operation"),
        Some(&Json::text(consumed.operation().to_hex()))
    );
    assert!(reopened
        .inspect_native_publication_history(destination.id(), &crate::TrustedReviewers::default())
        .is_err());
    assert!(reopened
        .inspect_root_publication_history(owner.id(), &trust)
        .is_err());
    fs::write(
        destination.project().root().join("note"),
        b"uncaptured editor work must not become approval",
    )
    .unwrap();
    let mut snapshot_predecessor = None;
    let second_snapshot = if writer == 6 {
        use crate::project_attachment::NativeSavedInputDecision as Decision;
        let rejected = reopened
            .decide_native_saved_input(
                owner.id(),
                version.operation(),
                Decision::Rejected,
                Some(initial_decision.record()),
                id(80),
                &trust,
            )
            .unwrap();
        let before = fs::read(&owner_journal).unwrap();
        assert!(reopened
            .save_native_review_snapshot(
                destination.id(),
                second_version.operation(),
                id(16),
                &trust
            )
            .is_err());
        assert_eq!(fs::read(&owner_journal).unwrap(), before);
        let eligible = reopened
            .decide_native_saved_input(
                owner.id(),
                version.operation(),
                Decision::Eligible,
                Some(rejected.record()),
                id(81),
                &trust,
            )
            .unwrap();
        assert_eq!(eligible.revision(), 3);
        snapshot_predecessor = Some(eligible.record());
        let before = fs::read(&owner_journal).unwrap();
        assert!(reopened
            .save_native_review_snapshot(
                destination.id(),
                second_version.operation(),
                id(16),
                &crate::TrustedReviewers::default()
            )
            .is_err());
        assert!(reopened
            .save_native_review_snapshot(owner.id(), second_version.operation(), id(16), &trust)
            .is_err());
        assert!(reopened
            .save_native_review_snapshot(destination.id(), id(99), id(16), &trust)
            .is_err());
        assert_eq!(fs::read(&owner_journal).unwrap(), before);
        let result = restart::interrupted_native_snapshot(
            &root,
            destination.id(),
            second_version.operation(),
            id(16),
            human_key.public_key().as_ref(),
            &owner_journal,
            |_phase| {
                use mesh_cas::{Blake3, Cas};
                let cas = Cas::<_, Blake3>::with_filesystem(
                    owner.metadata_path(),
                    owner.store.filesystem().read_only(),
                )
                .unwrap();
                let raw =
                    fs::read_to_string(owner.metadata_path().join("dependency-decision.pending"))
                        .unwrap();
                let intent = Json::parse(&raw).unwrap();
                let payload =
                    RecordDigest::parse_hex(intent.get("payload").and_then(Json::as_text).unwrap())
                        .unwrap();
                let bytes =
                    super::super::dependency_transaction::read_payload(&cas, payload, 65536)
                        .unwrap();
                let value = Json::parse(std::str::from_utf8(&bytes).unwrap()).unwrap();
                let graph = RecordDigest::parse_hex(
                    value
                        .get("body")
                        .unwrap()
                        .get("graph")
                        .and_then(Json::as_text)
                        .unwrap(),
                )
                .unwrap();
                let path = cas
                    .layout()
                    .chunk_path(&mesh_cas::Digest32::from_bytes(*graph.as_bytes()));
                let original = fs::read(&path).unwrap();
                let journal_before = fs::read(&owner_journal).unwrap();
                fs::remove_file(&path).unwrap();
                assert!(
                    reopened
                        .save_native_review_snapshot(
                            destination.id(),
                            second_version.operation(),
                            id(16),
                            &trust
                        )
                        .is_err(),
                    "missing staged graph must refuse, not rebuild"
                );
                assert!(!path.exists(), "retry reconstructed lost retained graph");
                assert_eq!(fs::read(&owner_journal).unwrap(), journal_before);
                assert_eq!(
                    fs::read_to_string(owner.metadata_path().join("dependency-decision.pending"))
                        .unwrap(),
                    raw
                );
                fs::write(&path, b"substituted retained snapshot graph").unwrap();
                assert!(reopened
                    .save_native_review_snapshot(
                        destination.id(),
                        second_version.operation(),
                        id(16),
                        &trust
                    )
                    .is_err());
                assert_eq!(
                    fs::read(&path).unwrap(),
                    b"substituted retained snapshot graph"
                );
                assert_eq!(fs::read(&owner_journal).unwrap(), journal_before);
                fs::write(&path, original).unwrap();
            },
        );
        let saved = reopened
            .save_native_review_snapshot(
                destination.id(),
                second_version.operation(),
                id(16),
                &trust,
            )
            .unwrap();
        assert_eq!(
            result.get("record"),
            Some(&Json::text(saved.record().to_hex()))
        );
        assert_eq!(
            result.get("graph"),
            Some(&Json::text(saved.graph().to_hex()))
        );
        assert_eq!(
            result.get("validation"),
            Some(&Json::text(saved.validation().to_hex()))
        );
        reopened
            .with_native_publication_history(destination.id(), &trust, |_, proof| {
                let body = proof.policy().review_snapshot_body(id(16)).unwrap();
                let Json::Array(decisions) = body.get("decisions").unwrap() else {
                    panic!("decisions missing")
                };
                assert_eq!(decisions.len(), 1);
                let Json::Array(exact) = &decisions[0] else {
                    panic!("decision missing")
                };
                assert_eq!(exact[1], Json::Number(3));
                assert_eq!(exact[2], Json::text(eligible.record().to_hex()));
                Ok(())
            })
            .unwrap();
        saved
    } else {
        early_second_snapshot.unwrap()
    };
    let candidate = reopened
        .inspect_native_review_candidate(destination.id(), second_snapshot.record(), &trust)
        .unwrap();
    assert_eq!(
        candidate.get("canonical"),
        first.get("publication").unwrap().get("head")
    );
    let output = reopened
        .with_native_publication_history(destination.id(), &trust, |_, proof| {
            Ok(proof
                .policy()
                .review_evidence(second_snapshot.record())
                .unwrap()
                .output())
        })
        .unwrap();
    let second_review = if writer == 6 {
        reopened
            .save_native_review(
                destination.id(),
                second_snapshot.record(),
                id(14),
                id(17),
                &trust,
            )
            .unwrap()
            .record()
    } else if writer == 5 {
        let before = fs::read(&owner_journal).unwrap();
        assert!(reopened
            .save_native_review(
                destination.id(),
                second_snapshot.record(),
                id(14),
                id(17),
                &crate::TrustedReviewers::default()
            )
            .is_err());
        assert!(reopened
            .save_native_review(owner.id(), second_snapshot.record(), id(14), id(17), &trust)
            .is_err());
        assert!(reopened
            .save_native_review(destination.id(), id(99), id(14), id(17), &trust)
            .is_err());
        assert_eq!(fs::read(&owner_journal).unwrap(), before);
        let result = restart::interrupted_native_review(
            &root,
            destination.id(),
            second_snapshot.record(),
            id(14),
            id(17),
            human_key.public_key().as_ref(),
            &owner_journal,
        );
        assert_eq!(result.get("bundle"), candidate.get("bundle"));
        let saved = reopened
            .save_native_review(
                destination.id(),
                second_snapshot.record(),
                id(14),
                id(17),
                &trust,
            )
            .unwrap();
        assert_eq!(
            result.get("record"),
            Some(&Json::text(saved.record().to_hex()))
        );
        saved.record()
    } else {
        append_test_policy(
            &owner,
            DependencyKind::ReviewSnapshot,
            "mesh.dependency-policy/v4",
            Json::object([
                ("request", Json::text(id(17).to_hex())),
                ("revision", Json::Number(1)),
                ("snapshot", Json::text(second_snapshot.record().to_hex())),
                (
                    "output",
                    Json::Array(vec![
                        Json::Array(vec![
                            Json::text(output.0.to_hex()),
                            Json::text(output.1.to_hex()),
                        ]),
                        Json::text(output.2.to_hex()),
                    ]),
                ),
                ("canonical", candidate.get("canonical").unwrap().clone()),
                ("bundle", candidate.get("bundle").unwrap().clone()),
                ("opener", Json::text(id(14).to_hex())),
            ]),
            None,
        )
    };
    let second_context = reopened
        .with_native_publication_history(destination.id(), &trust, |history, proof| {
            history
                .approval_context(&proof.policy().bound_review(second_review).unwrap())
                .map_err(error)
        })
        .unwrap();
    assert_ne!(
        first_context.reviewed_actor_head(),
        second_context.reviewed_actor_head()
    );
    append_publication(second_review, 2, first_record, second_context.clone(), 72);
    let reopened_again = AttachmentStorage::open(&root.join("metadata")).unwrap();
    let accepted = reopened_again
        .inspect_native_publication_history(destination.id(), &trust)
        .unwrap();
    assert_eq!(
        accepted.get("publication").unwrap().get("operation"),
        Some(&Json::text(second_version.operation().to_hex()))
    );
    assert_eq!(
        accepted.get("publication").unwrap().get("revision"),
        Some(&Json::Number(2))
    );
    if writer == 6 {
        reopened_again
            .decide_native_saved_input(
                owner.id(),
                version.operation(),
                crate::project_attachment::NativeSavedInputDecision::Rejected,
                snapshot_predecessor,
                id(82),
                &trust,
            )
            .unwrap();
        let before = fs::read(&owner_journal).unwrap();
        assert_eq!(
            reopened_again
                .save_native_review_snapshot(
                    destination.id(),
                    second_version.operation(),
                    id(16),
                    &trust
                )
                .unwrap(),
            second_snapshot
        );
        assert!(reopened_again
            .save_native_review_snapshot(
                destination.id(),
                second_version.operation(),
                id(18),
                &trust
            )
            .is_err());
        assert!(
            reopened_again
                .save_native_review_snapshot(
                    destination.id(),
                    second_version.operation(),
                    id(17),
                    &trust
                )
                .is_err(),
            "binding request must not become a snapshot"
        );
        assert_eq!(fs::read(&owner_journal).unwrap(), before);
        assert_eq!(
            reopened_again
                .inspect_native_publication_history(destination.id(), &trust)
                .unwrap(),
            accepted
        );
        assert_eq!(fs::read(&child_journal).unwrap(), child_before);
        assert!(
            owner.saved_versions().is_err(),
            "snapshot must not grant ordinary admission"
        );
        return;
    }
    if writer == 5 {
        let before = fs::read(&owner_journal).unwrap();
        let retry = reopened_again
            .save_native_review(
                destination.id(),
                second_snapshot.record(),
                id(14),
                id(17),
                &trust,
            )
            .unwrap();
        assert_eq!(
            retry.record(),
            second_review,
            "historical review retry must retain original main"
        );
        assert_eq!(
            Some(&Json::text(retry.bundle().to_hex())),
            candidate.get("bundle")
        );
        assert!(reopened_again
            .save_native_review(
                destination.id(),
                second_snapshot.record(),
                id(15),
                id(17),
                &trust
            )
            .is_err());
        assert_eq!(fs::read(&owner_journal).unwrap(), before);
        assert_eq!(fs::read(&child_journal).unwrap(), child_before);
        assert!(
            owner.saved_versions().is_err(),
            "saved review must not grant ordinary admission"
        );
        return;
    }
    let inspection_journals = [&owner, &destination].map(|work| {
        let path = work.metadata_path().join(crate::RECORD_FILE_NAME);
        let bytes = fs::read(&path).unwrap();
        (path, bytes)
    });
    assert_eq!(
        reopened_again
            .inspect_native_saved_version(destination.id(), second_version.operation(), &trust)
            .unwrap(),
        second_version
    );
    assert!(reopened_again
        .inspect_native_saved_version(destination.id(), id(99), &trust)
        .is_err());
    assert!(reopened_again
        .inspect_native_saved_version(
            destination.id(),
            second_version.operation(),
            &crate::TrustedReviewers::default()
        )
        .is_err());
    assert!(reopened_again
        .inspect_native_saved_version(owner.id(), second_version.operation(), &trust)
        .is_err());
    for (path, bytes) in inspection_journals {
        assert_eq!(
            fs::read(path).unwrap(),
            bytes,
            "saved version inspection changed native history"
        );
    }
    let owner_before = fs::read(&owner_journal).unwrap();
    reopened_again
        .with_native_publication_history(destination.id(), &trust, |history, proof| {
            assert_eq!(
                history
                    .approval_context(&proof.policy().bound_review(first_review.record()).unwrap())
                    .unwrap(),
                first_context
            );
            assert_eq!(
                history
                    .approval_context(&proof.policy().bound_review(second_review).unwrap())
                    .unwrap(),
                second_context
            );
            Ok(())
        })
        .unwrap();
    assert!(reopened_again
        .inspect_native_publication_history(destination.id(), &crate::TrustedReviewers::default())
        .is_err());
    fs::rename(root.join("source"), root.join("preserved-source")).unwrap();
    fs::create_dir(root.join("source")).unwrap();
    assert!(
        reopened_again
            .inspect_native_publication_history(destination.id(), &trust)
            .is_err(),
        "replacement source must not reuse accepted input context"
    );
    fs::remove_dir(root.join("source")).unwrap();
    fs::rename(root.join("preserved-source"), root.join("source")).unwrap();
    assert_eq!(fs::read(&owner_journal).unwrap(), owner_before);
    assert_eq!(fs::read(&child_journal).unwrap(), child_before);
    assert!(
        owner.saved_versions().is_err(),
        "verified replay must not grant ordinary admission"
    );

    if writer == 1 {
        let first_receipt = {
            let _guard =
                crate::workspace_custody::lock_workspace_initialization(&owner.store).unwrap();
            let (_, proof) = owner
                .project()
                .read_publication_private_history(owner.metadata_path(), &owner.store)
                .unwrap();
            let claim = proof.policy().publication_claim(first_record).unwrap();
            let cas = Cas::<_, Blake3>::with_filesystem(
                owner.metadata_path(),
                owner.store.filesystem().read_only(),
            )
            .unwrap();
            super::super::dependency_transaction::read_payload(&cas, claim.receipt, 65536).unwrap()
        };
        let decision_result = restart::interrupted_native_decision(
            &root,
            owner.id(),
            version.operation(),
            initial_decision.record(),
            id(98),
            human_key.public_key().as_ref(),
            &owner_journal,
            true,
            "last",
        );
        let rejected_decision = storage
            .decide_native_saved_input(
                owner.id(),
                version.operation(),
                super::super::NativeSavedInputDecision::Rejected,
                Some(initial_decision.record()),
                id(98),
                &trust,
            )
            .unwrap();
        assert_eq!(rejected_decision.revision(), 2);
        assert_eq!(
            decision_result.get("record").and_then(Json::as_text),
            Some(rejected_decision.record().to_hex().as_str())
        );
        assert!(!owner
            .metadata_path()
            .join(super::super::dependency_decision::PENDING)
            .exists());

        assert_eq!(
            storage
                .decide_native_saved_input(
                    owner.id(),
                    version.operation(),
                    super::super::NativeSavedInputDecision::Rejected,
                    Some(initial_decision.record()),
                    id(98),
                    &trust
                )
                .unwrap(),
            rejected_decision
        );
        let rejected = fs::read(&owner_journal).unwrap();
        let recovered = storage
            .commit_native_publication(
                destination.id(),
                id(71),
                first_review.record(),
                &first_receipt,
                &trust,
            )
            .unwrap();
        assert_eq!(recovered.record(), first_record);
        assert_eq!(recovered.revision(), 1);
        assert_eq!(
            recovered.head().as_bytes(),
            first_context.reviewed_actor_head().as_bytes()
        );
        assert_eq!(
            fs::read(&owner_journal).unwrap(),
            rejected,
            "later input rejection must not append or invalidate an exact completed retry"
        );

        use super::super::NativeSavedInputDecision as Decision;
        for (operation, decision, previous, request) in [
            (
                version.operation(),
                Decision::Eligible,
                Some(initial_decision.record()),
                id(98),
            ),
            (version.operation(), Decision::Eligible, None, id(95)),
            (
                version.operation(),
                Decision::Replaced(second_version.operation()),
                Some(rejected_decision.record()),
                id(95),
            ),
            (
                version.operation(),
                Decision::Replaced(version.operation()),
                Some(rejected_decision.record()),
                id(95),
            ),
        ] {
            assert!(storage
                .decide_native_saved_input(
                    owner.id(),
                    operation,
                    decision,
                    previous,
                    request,
                    &trust
                )
                .is_err());
            assert_eq!(fs::read(&owner_journal).unwrap(), rejected);
        }
        let revalidated = storage
            .decide_native_saved_input(
                owner.id(),
                version.operation(),
                Decision::Eligible,
                Some(rejected_decision.record()),
                id(97),
                &trust,
            )
            .unwrap();
        assert_eq!(revalidated.revision(), 3);
        let after_revalidation = fs::read(&owner_journal).unwrap();
        assert_eq!(
            storage
                .decide_native_saved_input(
                    owner.id(),
                    version.operation(),
                    Decision::Rejected,
                    Some(initial_decision.record()),
                    id(98),
                    &trust
                )
                .unwrap(),
            rejected_decision
        );
        assert_eq!(fs::read(&owner_journal).unwrap(), after_revalidation);
        assert_eq!(
            storage
                .inspect_native_publication_history(destination.id(), &trust)
                .unwrap(),
            accepted
        );
        let stale = storage
            .commit_native_publication(
                destination.id(),
                id(95),
                first_review.record(),
                &first_receipt,
                &trust,
            )
            .unwrap_err();
        assert!(
            stale.to_string().contains("exact decisions changed"),
            "{stale}"
        );
        assert_eq!(fs::read(&owner_journal).unwrap(), after_revalidation);
        assert!(
            owner.saved_versions().is_err(),
            "native controls granted ordinary admission"
        );
    }
}
