#[path = "tests/restart.rs"]
mod restart;
use super::*;
use crate::project_attachment::{NativeSavedReviewRequest, ObservationLimits};
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
        if writer == 2 {
            let result = restart::interrupted_publication(
                &root,
                owner.id(),
                id(6),
                bound.record(),
                &receipt,
                key.public_key().as_ref(),
                &journal,
                "first",
            );
            assert_eq!(
                result.get("head").and_then(Json::as_text),
                Some(
                    RecordDigest::from_bytes(*preview.reviewed_actor_head().as_bytes())
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
        if writer == 2 {
            let result = restart::interrupted_publication(
                &root,
                owner.id(),
                id(9),
                next_review,
                &receipt,
                key.public_key().as_ref(),
                &journal,
                "last",
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
    let second_snapshot = storage
        .save_dependency_review_snapshot(&owner, &destination, second_version, &[], id(16))
        .unwrap();
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
        if writer != 0 {
            use super::super::dependency_private_context::publication::Step;
            let journal = owner.metadata_path().join(crate::RECORD_FILE_NAME);
            let before = fs::read(&journal).unwrap();
            if writer == 3 {
                owner
                    .decide_saved_input(
                        version,
                        super::super::SavedInputDecision::Rejected,
                        Some(initial_decision.record()),
                        id(99),
                    )
                    .unwrap();
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
    if writer >= 2 {
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
    let second_review = append_test_policy(
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
    );
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
        let (proof, first_receipt) = {
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
            let first_receipt =
                super::super::dependency_transaction::read_payload(&cas, claim.receipt, 65536)
                    .unwrap();
            (proof, first_receipt)
        };
        // Independent policy fixture: production control admission after publication is still
        // separate work. Replay must preserve this historically valid ordering now.
        append_test_policy(
            &owner,
            DependencyKind::Eligibility,
            "mesh.dependency-policy/v1",
            super::super::dependency_decision::body(
                proof.binding().project,
                proof.binding().installation,
                version,
                super::super::SavedInputDecision::Rejected,
                id(98),
                2,
                initial_decision.record(),
            ),
            None,
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
    }
}
