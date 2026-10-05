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
    {
        let source_selection = storage
            .prepare_dependency_graph(&owner, &owner, version.operation(), &[])
            .unwrap();
        let destination_selection = storage
            .prepare_dependency_work(&owner, &destination)
            .unwrap();
        let roots = source_selection
            .roots
            .iter()
            .chain(&destination_selection.roots)
            .cloned()
            .collect::<Vec<_>>();
        let guard = crate::workspace_custody::lock_workspace_initialization_set(&roots).unwrap();
        let graph = storage
            .inspect_prepared_dependency_graph(&source_selection, &guard)
            .unwrap();
        let (configuration, owner_private) = owner
            .project()
            .read_publication_private_history(owner.metadata_path(), &owner.store)
            .unwrap();
        let read_owner = || Ok((configuration.clone(), owner_private.clone()));
        let (_, child_private) = candidate
            .verify_completed_private_history(&graph, &guard, None, read_owner)
            .unwrap();
        assert_ne!(
            child_private.binding().installation,
            owner_private.binding().installation
        );
        let foreign = candidate
            .verify_completed_private_history(&graph, &guard, None, || {
                Ok((configuration.clone(), child_private.clone()))
            })
            .err()
            .expect("foreign owner proof must refuse");
        assert!(
            foreign
                .to_string()
                .contains("validated dependency history changed"),
            "{foreign}"
        );
        let wrong_configuration = candidate
            .verify_completed_private_history(&graph, &guard, None, || {
                Ok(("another configuration".into(), owner_private.clone()))
            })
            .err()
            .expect("substituted configuration must refuse");
        assert!(
            wrong_configuration
                .to_string()
                .contains("private history configuration differs"),
            "{wrong_configuration}"
        );
        let journal = owner.metadata_path().join(crate::RECORD_FILE_NAME);
        struct RestorePrivateOwner(PathBuf, Vec<u8>);
        impl Drop for RestorePrivateOwner {
            fn drop(&mut self) {
                fs::write(&self.0, &self.1).unwrap();
            }
        }
        let restore = RestorePrivateOwner(journal.clone(), fs::read(&journal).unwrap());
        let mut changed = restore.1.clone();
        changed.push(1);
        let reads = std::cell::Cell::new(0);
        let stale = candidate
            .verify_completed_private_history(&graph, &guard, None, || {
                reads.set(reads.get() + 1);
                if reads.get() == 2 {
                    fs::write(&journal, &changed)?;
                }
                read_owner()
            })
            .err()
            .expect("cached owner proof must not survive a journal change");
        assert_eq!(reads.get(), 2);
        assert!(
            stale
                .to_string()
                .contains("validated dependency history changed"),
            "{stale}"
        );
        assert_eq!(fs::read(&journal).unwrap(), changed);
        drop(restore);
        candidate
            .verify_completed_private_history(&graph, &guard, None, read_owner)
            .unwrap();
    }
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
    // A saved review binds the exact snapshot, not just the output or latest decision.
    let bind = |selected_snapshot, request| {
        let reopened = AttachmentStorage::open(&root.join("metadata")).unwrap();
        let current_owner = reopened.reopen(owner.id()).unwrap();
        let current_destination = reopened.reopen(destination.id()).unwrap();
        reopened.save_dependency_review(
            &current_owner,
            super::super::NativeSavedReviewRequest {
                source: &current_destination,
                version: consumed,
                snapshot: selected_snapshot,
                request,
                opener: id(22),
            },
            &[],
        )
    };
    let before_binding = fs::read(&owner_journal).unwrap();
    let failed = storage.save_dependency_review_with_io(
        &owner,
        super::super::NativeSavedReviewRequest {
            source: &destination,
            version: consumed,
            snapshot,
            request: id(20),
            opener: id(22),
        },
        &[],
        |step, file, frame| {
            if matches!(step, Step::Staged) {
                file.write_all(&frame[..1])?;
                file.sync_all()?;
                return Err(io::Error::other("interrupted bound review"));
            }
            Ok(())
        },
        |f| f.sync_all(),
    );
    assert!(failed.is_err());
    let partial_binding = fs::read(&owner_journal).unwrap();
    assert_eq!(&partial_binding[..before_binding.len()], before_binding);
    assert_eq!(partial_binding.len(), before_binding.len() + 1);
    assert!(
        bind(recovered, id(20)).is_err(),
        "pending review cannot change its snapshot"
    );
    assert_eq!(fs::read(&owner_journal).unwrap(), partial_binding);
    let bound = bind(snapshot, id(20)).unwrap();
    let after_binding = fs::read(&owner_journal).unwrap();
    assert_eq!(bind(snapshot, id(20)).unwrap(), bound);
    assert_eq!(fs::read(&owner_journal).unwrap(), after_binding);
    let other_bound = bind(recovered, id(21)).unwrap();
    assert_ne!(
        bound.bundle(),
        other_bound.bundle(),
        "distinct snapshots remain independently addressable"
    );
    assert!(storage
        .save_dependency_review_with_io(
            &owner,
            super::super::NativeSavedReviewRequest {
                source: &destination,
                version: consumed,
                snapshot,
                request: id(23),
                opener: id(22),
            },
            &[],
            |step, _, _| {
                if matches!(step, Step::Appended) {
                    return Err(io::Error::other("lost bound review reply"));
                }
                Ok(())
            },
            |f| f.sync_all()
        )
        .is_err());
    let lost_reply = fs::read(&owner_journal).unwrap();
    let recovered_bound = bind(snapshot, id(23)).unwrap();
    assert_eq!(recovered_bound.bundle(), bound.bundle());
    assert_ne!(recovered_bound.record(), bound.record());
    assert_eq!(fs::read(&owner_journal).unwrap(), lost_reply);
    assert_eq!(fs::read(&child_journal).unwrap(), child_before);
    let inspect = || {
        AttachmentStorage::open(&root.join("metadata"))
            .unwrap()
            .saved_dependency_review(destination.id(), bound.record())
            .unwrap()
    };
    let displayed = inspect();
    assert_eq!(
        displayed.get("historical_decisions_current"),
        Some(&Json::Bool(true))
    );
    assert_eq!(
        displayed.get("inputs_currently_eligible"),
        Some(&Json::Bool(true))
    );
    assert_eq!(
        displayed.get("approval_authority"),
        Some(&Json::Bool(false))
    );
    assert_eq!(
        displayed.get("review").unwrap().get("bundle"),
        Some(&Json::text(bound.bundle().to_hex()))
    );
    let Some(Json::Array(changes)) = displayed.get("review").unwrap().get("bundle_changes") else {
        panic!("missing exact changes")
    };
    let note = changes
        .iter()
        .find(|change| change.get("path_after").and_then(Json::as_text) == Some("/note"))
        .unwrap();
    let object =
        mesh_materializer::ObjectId::parse(note.get("object_id").and_then(Json::as_text).unwrap())
            .unwrap();
    let inspect_file = || {
        AttachmentStorage::open(&root.join("metadata"))
            .unwrap()
            .saved_dependency_review_file(destination.id(), bound.record(), object, true)
            .unwrap()
    };
    assert_eq!(inspect_file(), b"exact native input");
    assert!(storage
        .saved_dependency_review_file(destination.id(), bound.record(), object, false)
        .is_err());
    assert!(storage
        .saved_dependency_review_file(
            destination.id(),
            bound.record(),
            mesh_materializer::ObjectId::from_bytes([255; 16]),
            true
        )
        .is_err());
    assert!(storage
        .saved_dependency_review(owner.id(), bound.record())
        .is_err());
    assert!(storage
        .saved_dependency_review(destination.id(), id(99))
        .is_err());
    let listed = storage
        .saved_dependency_reviews(destination.id(), None)
        .unwrap();
    let Some(Json::Array(rows)) = listed.get("reviews") else {
        panic!("missing review list")
    };
    assert_eq!(rows.len(), 3);
    assert!(rows
        .iter()
        .any(|row| row.get("record") == Some(&Json::text(other_bound.record().to_hex()))));
    let preview = storage
        .saved_dependency_review_preview(destination.id(), bound.record())
        .unwrap();
    assert_eq!(
        preview.0.review_bundle().digest().as_bytes(),
        bound.bundle().as_bytes()
    );
    let native_bound = storage
        .with_registered_dependency_context(destination.id(), |context| {
            let native = context
                .owner_proof
                .policy()
                .bound_review(bound.record())
                .unwrap();
            let private = context.private_review_history()?;
            assert_eq!(private.approval_context(&native).unwrap(), preview.0);
            let mut claims = context.owner_proof.policy().clone();
            let (ordinal, previous) = claims.native_head().unwrap();
            let output = native.evidence().output();
            let digest_json = |digest: RecordDigest| Json::text(digest.to_hex());
            let value = Json::object([
                ("schema", Json::text("mesh.dependency-policy/v4")),
                (
                    "authority",
                    digest_json(context.owner_proof.binding().authority),
                ),
                ("revision", Json::Number(ordinal + 1)),
                ("previous", digest_json(previous)),
                (
                    "kind",
                    Json::Number(mesh_store::DependencyKind::ReviewSnapshot.code().into()),
                ),
                (
                    "body",
                    Json::object([
                        ("request", digest_json(id(240))),
                        ("revision", Json::Number(1)),
                        ("snapshot", digest_json(native.evidence().snapshot())),
                        (
                            "output",
                            Json::Array(vec![
                                Json::Array(vec![digest_json(output.0), digest_json(output.1)]),
                                digest_json(output.2),
                            ]),
                        ),
                        ("canonical", digest_json(id(241))),
                        ("bundle", digest_json(native.review().bundle)),
                        ("opener", digest_json(native.review().opened_by)),
                    ]),
                ),
            ])
            .encode();
            let payload = super::super::dependency_transaction::hash(value.as_bytes());
            claims
                .apply(
                    mesh_store::DependencyRecord {
                        authority: context.owner_proof.binding().authority,
                        revision: ordinal + 1,
                        previous,
                        payload,
                        kind: mesh_store::DependencyKind::ReviewSnapshot,
                    },
                    value.as_bytes(),
                )
                .unwrap();
            let unsupported = claims.bound_review(payload).unwrap();
            assert_eq!(
                private.approval_context(&unsupported).unwrap_err(),
                "verified native canonical ancestry is unavailable"
            );
            struct RestoreJournal(PathBuf, Vec<u8>);
            impl Drop for RestoreJournal {
                fn drop(&mut self) {
                    fs::write(&self.0, &self.1).unwrap();
                }
            }
            for path in [&owner_journal, &child_journal] {
                let restore = RestoreJournal(path.clone(), fs::read(path).unwrap());
                let mut changed = restore.1.clone();
                changed.push(1);
                fs::write(path, &changed).unwrap();
                let refused = private.approval_context(&native).unwrap_err();
                assert!(
                    refused.contains("validated dependency history changed"),
                    "{refused}"
                );
                assert_eq!(
                    fs::read(path).unwrap(),
                    changed,
                    "private read must not repair history"
                );
                drop(restore);
                assert_eq!(private.approval_context(&native).unwrap(), preview.0);
            }
            let read_input = || {
                private.with_saved_input(native.evidence().output().2, |input| {
                    let mut bytes = Vec::new();
                    input.write_file("note", &mut bytes)?;
                    Ok(bytes)
                })
            };
            assert_eq!(read_input().unwrap(), b"exact native input");
            for path in [&owner_journal, &child_journal] {
                let restore = RestoreJournal(path.clone(), fs::read(path).unwrap());
                let mut changed = restore.1.clone();
                changed.push(1);
                let refused = private
                    .with_saved_input(native.evidence().output().2, |input| {
                        let mut bytes = Vec::new();
                        input.write_file("note", &mut bytes)?;
                        fs::write(path, &changed)?;
                        Ok(bytes)
                    })
                    .unwrap_err();
                assert!(
                    refused
                        .to_string()
                        .contains("validated dependency history changed"),
                    "{refused}"
                );
                assert_eq!(
                    fs::read(path).unwrap(),
                    changed,
                    "private input must not repair a changed journal"
                );
                drop(restore);
                assert_eq!(read_input().unwrap(), b"exact native input");
            }
            Ok(native)
        })
        .unwrap();
    storage
        .with_registered_dependency_context(owner.id(), |context| {
            let private = context.private_review_history()?;
            assert!(private
                .approval_context(&native_bound)
                .unwrap_err()
                .contains("another native work"));
            Ok(())
        })
        .unwrap();
    let other_preview = storage
        .saved_dependency_review_preview(destination.id(), other_bound.record())
        .unwrap();
    assert_ne!(
        preview.0.validation_digest(),
        other_preview.0.validation_digest()
    );
    use mesh_approval::{
        ApprovalDecision, ExpectedHumanApproval, HumanApprovalCredential, HumanApprovalReceiptDraft,
    };
    use ring::rand::SystemRandom;
    use ring::signature::{EcdsaKeyPair, KeyPair as _, ECDSA_P256_SHA256_ASN1_SIGNING};
    let rng = SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &rng).unwrap();
    let human_key =
        EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &rng).unwrap();
    let human_credential = HumanApprovalCredential::from_public_key(
        human_key.public_key().as_ref().try_into().unwrap(),
    )
    .unwrap();
    let trust = crate::TrustedReviewers::with_human_credentials([human_credential.clone()]);
    let receipt_for = |context: mesh_approval::HumanApprovalContext, challenge: u8, decision| {
        let draft = HumanApprovalReceiptDraft::new(
            ExpectedHumanApproval::new(context, human_credential.clone(), [challenge; 32]),
            decision,
        );
        let signature = human_key.sign(&rng, &draft.canonical_bytes()).unwrap();
        draft
            .with_signature(signature.as_ref().to_vec())
            .unwrap()
            .canonical_bytes()
    };
    let receipt = receipt_for(preview.0.clone(), 61, ApprovalDecision::Approve);
    {
        // A publication-bearing owner can prove completed private content without granting
        // ordinary admission. This test-only frame is removed before the remaining scenarios.
        let expected_private_graph = storage
            .inspect_dependency_graph(&owner, &destination, consumed, &[&owner])
            .unwrap()
            .to_json();
        let source_selection = storage
            .prepare_dependency_graph(&owner, &owner, version.operation(), &[])
            .unwrap();
        let destination_selection = storage
            .prepare_dependency_work(&owner, &destination)
            .unwrap();
        let roots = source_selection
            .roots
            .iter()
            .chain(&destination_selection.roots)
            .cloned()
            .collect::<Vec<_>>();
        let guard = crate::workspace_custody::lock_workspace_initialization_set(&roots).unwrap();
        let graph = storage
            .inspect_prepared_dependency_graph(&source_selection, &guard)
            .unwrap();
        let destination_binding = storage
            .validate_dependency_work(&destination_selection, &guard)
            .unwrap();
        let (_, private) = owner
            .project()
            .read_publication_private_history(owner.metadata_path(), &owner.store)
            .unwrap();
        let (ordinal, previous) = private.policy().native_head().unwrap();
        let hash = super::super::dependency_transaction::hash;
        let j = |digest: RecordDigest| Json::text(digest.to_hex());
        let bytes = Json::object([
            ("schema", Json::text("mesh.dependency-policy/v5")),
            ("authority", j(private.binding().authority)),
            ("revision", Json::Number(ordinal + 1)),
            ("previous", j(previous)),
            (
                "kind",
                Json::Number(mesh_store::DependencyKind::Publication.code().into()),
            ),
            (
                "body",
                Json::object([
                    ("request", j(id(244))),
                    ("revision", Json::Number(1)),
                    ("previous", j(id(0))),
                    ("review", j(bound.record())),
                    ("receipt", j(hash(&receipt))),
                    (
                        "result",
                        j(RecordDigest::from_bytes(
                            *preview.0.reviewed_actor_head().as_bytes(),
                        )),
                    ),
                    (
                        "credential",
                        j(RecordDigest::from_bytes(*human_credential.id().as_bytes())),
                    ),
                    ("challenge", j(id(61))),
                ]),
            ),
        ])
        .encode()
        .into_bytes();
        let record = mesh_store::DependencyRecord {
            authority: private.binding().authority,
            revision: ordinal + 1,
            previous,
            payload: hash(&bytes),
            kind: mesh_store::DependencyKind::Publication,
        };
        private.policy().clone().apply(record, &bytes).unwrap();
        let cas = mesh_cas::Cas::<_, mesh_cas::Blake3>::with_filesystem(
            owner.metadata_path(),
            owner.store.filesystem(),
        )
        .unwrap();
        cas.promote(receipt.clone()).unwrap();
        cas.promote(bytes).unwrap();
        struct RestorePublication(PathBuf, Vec<u8>);
        impl Drop for RestorePublication {
            fn drop(&mut self) {
                fs::write(&self.0, &self.1).unwrap();
            }
        }
        let restore = RestorePublication(owner_journal.clone(), fs::read(&owner_journal).unwrap());
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&owner_journal)
            .unwrap();
        file.write_all(&mesh_store::frame_record(
            &mesh_store::StoredRecord::Dependency(record),
        ))
        .unwrap();
        file.sync_all().unwrap();
        let published = fs::read(&owner_journal).unwrap();
        let (configuration, owner_private) = owner
            .project()
            .read_publication_private_history(owner.metadata_path(), &owner.store)
            .unwrap();
        assert!(owner_private.policy().has_publication_claims());
        let (prospective, child_private) = candidate
            .verify_completed_private_history(&graph, &guard, None, || {
                Ok((configuration.clone(), owner_private.clone()))
            })
            .unwrap();
        assert_ne!(
            child_private.binding().installation,
            owner_private.binding().installation
        );
        let root_selection = storage.prepare_dependency_work(&owner, &owner).unwrap();
        let owner_binding = storage
            .validate_publication_root(&root_selection, &guard, &owner_private)
            .unwrap();
        let source_history = crate::workspace::NativePrivateReviewHistory::open(
            owner.metadata_path(),
            owner.store.clone(),
            &owner_private,
            (&owner.store, &owner_private),
            mesh_operations::WorkspaceId::from_bytes(super::super::history::short_id(
                configuration.as_bytes(),
            )),
            &owner_binding,
            &guard,
        )
        .unwrap();
        // Rebuild from durable transaction material; do not reuse the original in-memory
        // candidate. Complete private-context discovery still supplies graph/bindings separately.
        let rebuild = |selected_owner| {
            storage.with_recovered_consumed_material(
                super::super::consumption_prepare::RecoveryMaterial {
                    owner: &owner,
                    request: NativeConsumedStartRequest {
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
                    phase: super::super::consumption_prepare::RecoveryPhase::CompletedRead,
                    graph: &graph,
                    source_binding: &owner_binding,
                    destination_binding: &destination_binding,
                    owner_binding: selected_owner,
                },
                &guard,
                |operation, expected| {
                    assert_eq!(operation, candidate.operation());
                    owner_private.verify_current(&owner.store)?;
                    source_history.with_saved_input(version.operation(), |input| {
                        super::super::consumption_prepare::source_verification::verify_starting_source(
                            &destination, &input, &guard, expected,
                        )
                    })
                },
                |operation| {
                    assert_eq!(operation, candidate.operation());
                    owner_private.verify_current(&owner.store)?;
                    source_history.with_saved_input(version.operation(), |_| Ok(()))
                },
                |recovered, _, reconstructed_graph, held| {
                    recovered.verify_completed_private_history(reconstructed_graph, held, None, || {
                        Ok((configuration.clone(), owner_private.clone()))
                    })
                },
            )
        };
        let reconstructed = rebuild(owner_private.binding()).unwrap();
        assert_eq!(reconstructed.0, prospective);
        assert!(reconstructed.1 == child_private);
        let mut different_owner = owner_private.binding();
        different_owner.installation = id(248);
        assert!(
            rebuild(different_owner).is_err(),
            "foreign owner must not reconstruct a completed lane"
        );
        let (initial_configuration, _) = destination
            .project()
            .read_completed_start_facts(destination.metadata_path(), &destination.store)
            .unwrap();
        let child_cas = mesh_cas::Cas::<_, mesh_cas::Blake3>::with_filesystem(
            destination.metadata_path(),
            destination.store.filesystem().read_only(),
        )
        .unwrap();
        let signed = super::super::dependency_transaction::read_payload(
            &child_cas,
            candidate.operation(),
            16 * 1024 * 1024,
        )
        .unwrap();
        let envelope =
            crate::authenticated_changeset::AuthenticatedChangeSet::from_canonical_bytes(&signed)
                .unwrap();
        let reconstruct = |actor, expected: &str| {
            source_history.with_saved_input(version.operation(), |input| {
                super::super::consumption_prepare::source_verification::verify_starting_source(
                    &destination,
                    &input,
                    &guard,
                    super::super::consumption_prepare::source_verification::StartingSource {
                        configuration: &initial_configuration,
                        prospective: expected,
                        actor,
                        limits: ObservationLimits::default(),
                        envelope: &envelope,
                    },
                )
            })
        };
        assert_eq!(reconstruct(actor, &prospective).unwrap(), prospective);
        let wrong_actor = reconstruct(PublicKey::from_bytes([249; 32]), &prospective).unwrap_err();
        assert!(
            wrong_actor.to_string().contains("starting actor changed"),
            "{wrong_actor}"
        );
        let wrong_destination =
            reconstruct(actor, "another prospective configuration").unwrap_err();
        assert!(
            wrong_destination
                .to_string()
                .contains("retained signed start differs from exact saved source"),
            "{wrong_destination}"
        );
        assert!(owner
            .project()
            .read_configuration(owner.metadata_path(), &owner.store)
            .is_err());
        assert_eq!(fs::read(&owner_journal).unwrap(), published);
        assert_eq!(fs::read(&child_journal).unwrap(), child_before);
        drop(source_history);
        drop(guard);
        let reopened = AttachmentStorage::open(&root.join("metadata")).unwrap();
        let accepted = reopened
            .inspect_native_publication_history(destination.id(), &trust)
            .unwrap();
        assert_eq!(
            accepted.get("publication").unwrap().get("operation"),
            Some(&Json::text(consumed.operation().to_hex()))
        );
        assert!(reopened
            .inspect_native_publication_history(
                destination.id(),
                &crate::TrustedReviewers::default()
            )
            .is_err());
        assert!(
            reopened
                .inspect_root_publication_history(owner.id(), &trust)
                .is_err(),
            "root-only replay must still refuse consumed publications"
        );
        let unchanged = reopened
            .inspect_native_review_candidate(destination.id(), snapshot.record(), &trust)
            .unwrap_err();
        assert!(
            unchanged.to_string().contains("nothing to review"),
            "{unchanged}"
        );
        assert_eq!(
            reopened
                .inspect_private_dependency_graph(destination.id(), consumed.operation())
                .unwrap(),
            expected_private_graph,
            "private discovery must reconstruct the complete consumed graph after publication"
        );
        assert!(reopened
            .inspect_private_dependency_graph(destination.id(), id(249))
            .is_err());
        let child_restore =
            RestorePublication(child_journal.clone(), fs::read(&child_journal).unwrap());
        fs::OpenOptions::new()
            .append(true)
            .open(&child_journal)
            .unwrap()
            .write_all(&[1])
            .unwrap();
        let partial_child = fs::read(&child_journal).unwrap();
        assert!(reopened
            .inspect_private_dependency_graph(destination.id(), consumed.operation())
            .is_err());
        assert_eq!(
            fs::read(&child_journal).unwrap(),
            partial_child,
            "private discovery must not repair a required history"
        );
        drop(child_restore);
        assert_eq!(fs::read(&owner_journal).unwrap(), published);
        assert_eq!(fs::read(&child_journal).unwrap(), child_before);
        drop(restore);
    }
    let inspect_receipt = || {
        storage
            .inspect_saved_dependency_review_receipt(
                destination.id(),
                bound.record(),
                &receipt,
                &trust,
            )
            .unwrap()
    };
    let before_receipt_owner = fs::read(&owner_journal).unwrap();
    let before_receipt_child = fs::read(&child_journal).unwrap();
    let checked_receipt = inspect_receipt();
    assert_eq!(
        checked_receipt.get("receipt_verified"),
        Some(&Json::Bool(true))
    );
    assert_eq!(
        checked_receipt.get("publication_committed"),
        Some(&Json::Bool(false))
    );
    assert_eq!(
        checked_receipt.get("approval_authority"),
        Some(&Json::Bool(false))
    );
    assert_eq!(
        checked_receipt.get("inputs_currently_eligible"),
        Some(&Json::Bool(true))
    );
    assert_eq!(
        checked_receipt.get("historical_decisions_current"),
        Some(&Json::Bool(true))
    );
    assert_eq!(
        checked_receipt.get("bundle"),
        Some(&Json::text(bound.bundle().to_hex()))
    );
    assert_eq!(
        checked_receipt.get("result"),
        Some(&Json::text(
            RecordDigest::from_bytes(*preview.0.reviewed_actor_head().as_bytes()).to_hex()
        ))
    );
    let mut corrupted = receipt.clone();
    *corrupted.last_mut().unwrap() ^= 1;
    for bad in [
        receipt_for(other_preview.0.clone(), 62, ApprovalDecision::Approve),
        receipt_for(preview.0.clone(), 0, ApprovalDecision::Approve),
        receipt_for(preview.0.clone(), 63, ApprovalDecision::Reject),
        corrupted,
        vec![],
        vec![0; 65_537],
        b"malformed".to_vec(),
    ] {
        assert!(
            storage
                .inspect_saved_dependency_review_receipt(
                    destination.id(),
                    bound.record(),
                    &bad,
                    &trust
                )
                .is_err(),
            "receipt must match native context and trusted approval"
        );
    }
    assert!(storage
        .inspect_saved_dependency_review_receipt(
            destination.id(),
            other_bound.record(),
            &receipt,
            &trust
        )
        .is_err());
    assert!(storage
        .inspect_saved_dependency_review_receipt(owner.id(), bound.record(), &receipt, &trust)
        .is_err());
    assert!(storage
        .inspect_saved_dependency_review_receipt(destination.id(), id(199), &receipt, &trust)
        .is_err());
    assert!(storage
        .inspect_saved_dependency_review_receipt(
            destination.id(),
            bound.record(),
            &receipt,
            &crate::TrustedReviewers::default()
        )
        .is_err());
    assert_eq!(fs::read(&owner_journal).unwrap(), before_receipt_owner);
    assert_eq!(fs::read(&child_journal).unwrap(), before_receipt_child);
    assert_eq!(
        inspect_receipt(),
        checked_receipt,
        "historical inspection is repeatable without committing a challenge"
    );

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
    assert!(storage
        .inspect_saved_dependency_review_receipt(destination.id(), bound.record(), &receipt, &trust)
        .is_err());
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
    let rejection = owner
        .decide_saved_input(
            version,
            super::super::SavedInputDecision::Rejected,
            Some(eligible.record()),
            id(12),
        )
        .unwrap();
    let rejected = fs::read(owner.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap();
    let historical = inspect();
    let rejected_receipt = inspect_receipt();
    assert_eq!(
        rejected_receipt.get("receipt_verified"),
        Some(&Json::Bool(true))
    );
    assert_eq!(
        rejected_receipt.get("inputs_currently_eligible"),
        Some(&Json::Bool(false))
    );
    assert_eq!(
        rejected_receipt.get("historical_decisions_current"),
        Some(&Json::Bool(false))
    );
    for field in [
        "review",
        "receipt",
        "bundle",
        "canonical",
        "result",
        "credential",
        "challenge",
    ] {
        assert_eq!(rejected_receipt.get(field), checked_receipt.get(field));
    }

    assert_eq!(
        historical.get("historical_decisions_current"),
        Some(&Json::Bool(false))
    );
    assert_eq!(
        historical.get("inputs_currently_eligible"),
        Some(&Json::Bool(false))
    );
    for field in [
        "record",
        "snapshot",
        "canonical",
        "historical_validation",
        "historical_decisions",
        "historical_graph",
        "review",
    ] {
        assert_eq!(
            historical.get(field),
            displayed.get(field),
            "historical {field} changed after rejection"
        );
    }
    assert_eq!(inspect_file(), b"exact native input");
    assert_eq!(
        storage
            .saved_dependency_review_preview(destination.id(), bound.record())
            .unwrap(),
        preview
    );

    assert_eq!(bind(snapshot, id(20)).unwrap(), bound);
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
        .inspect_saved_dependency_review_receipt(destination.id(), bound.record(), &receipt, &trust)
        .is_err());

    assert!(storage
        .saved_dependency_review(destination.id(), bound.record())
        .is_err());
    assert!(storage
        .saved_dependency_review_preview(destination.id(), bound.record())
        .is_err());
    assert!(storage
        .saved_dependency_review_file(destination.id(), bound.record(), object, true)
        .is_err());

    assert!(bind(snapshot, id(20)).is_err());
    assert!(storage
        .save_dependency_review_snapshot(&owner, &destination, consumed, &[], id(10))
        .is_err());
    assert_eq!(fs::read(&owner_journal).unwrap(), rejected);
    fs::write(&graph_path, graph_bytes).unwrap();
    assert_eq!(recover(id(10)), snapshot);
    fs::write(
        destination.project().root().join("note"),
        b"later private progress",
    )
    .unwrap();
    let later_input = destination
        .project()
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    let later = storage
        .prepare_registered_dependency_capture(&destination, &later_input, actor, id(30), sign)
        .unwrap()
        .commit()
        .unwrap();
    assert_ne!(later.operation(), consumed.operation());
    assert_eq!(
        inspect(),
        historical,
        "later saved output replaced historical review evidence"
    );
    assert_eq!(inspect_file(), b"exact native input");
    assert_eq!(
        storage
            .saved_dependency_review_preview(destination.id(), bound.record())
            .unwrap(),
        preview
    );
    assert_eq!(bind(snapshot, id(20)).unwrap(), bound);
    owner
        .decide_saved_input(
            version,
            super::super::SavedInputDecision::Eligible,
            Some(rejection.record()),
            id(31),
        )
        .unwrap();
    let revalidated = inspect();
    let revalidated_receipt = inspect_receipt();
    assert_eq!(
        revalidated_receipt.get("inputs_currently_eligible"),
        Some(&Json::Bool(true))
    );
    assert_eq!(
        revalidated_receipt.get("historical_decisions_current"),
        Some(&Json::Bool(false))
    );
    assert_eq!(
        revalidated_receipt.get("approval_authority"),
        Some(&Json::Bool(false))
    );
    for field in [
        "review",
        "receipt",
        "bundle",
        "canonical",
        "result",
        "credential",
        "challenge",
    ] {
        assert_eq!(revalidated_receipt.get(field), checked_receipt.get(field));
    }

    assert_eq!(
        revalidated.get("historical_decisions_current"),
        Some(&Json::Bool(false))
    );
    let revalidated_list = storage
        .saved_dependency_reviews(destination.id(), None)
        .unwrap();
    let Some(Json::Array(items)) = revalidated_list.get("reviews") else {
        panic!("missing reviews")
    };
    for item in items {
        assert_eq!(
            item.get("inputs_currently_eligible"),
            Some(&Json::Bool(true))
        );
        assert_eq!(
            item.get("historical_decisions_current"),
            Some(&Json::Bool(false))
        );
    }

    assert_eq!(
        revalidated.get("inputs_currently_eligible"),
        Some(&Json::Bool(true)),
        "revalidated input is eligible even when the historical decision revision is stale"
    );
    assert_eq!(revalidated.get("review"), displayed.get("review"));
    assert_eq!(inspect_file(), b"exact native input");
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
