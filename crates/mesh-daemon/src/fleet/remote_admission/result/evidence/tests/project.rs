use super::*;
use crate::fleet::NativeRemoteResultReceiver;
use crate::fleet::{
    Command, FleetStore, RemotePeerChallenge, RemoteProjectCandidateRequest, WorkspaceBinding,
};
use crate::project_attachment::{AttachmentStorage, ObservationLimits};
use std::fs;

fn project_import_journey(fault: u8) {
    let cancel_while_signing = fault == 1;
    let mut setup = Setup::new();
    let source_path = setup.f.path.join("original-project");
    let metadata = setup.f.path.join("original-metadata");
    fs::create_dir(&source_path).unwrap();
    fs::create_dir(&metadata).unwrap();
    fs::write(source_path.join("result.txt"), &setup.bytes).unwrap();
    let source = AttachmentStorage::open(&metadata)
        .unwrap()
        .provision(&source_path)
        .unwrap();
    let actor = public(&setup.f.coordinator);
    let saved = source
        .project()
        .save_capture(
            source.metadata_path(),
            &source
                .project()
                .capture_inputs(ObservationLimits::default())
                .unwrap(),
            actor,
            |p| sign(&setup.f.coordinator, p),
        )
        .unwrap();
    let input_version = saved.operation();
    let trusted = crate::TrustedReviewers::default();
    let original = source
        .with_fleet_input(input_version, &trusted, |open, _| {
            Ok(open.historical_workspace_preview(input_version).unwrap())
        })
        .unwrap();
    setup.manifest =
        RemoteInputManifest::new(input_version, setup.manifest.entries().to_vec()).unwrap();
    setup.f.work.assignment.input = input_version;
    setup.f.work.assignment.bundle = setup.manifest.bundle();
    let mut runtime = Runtime::open(
        FleetStore::open(setup.f.path.join("project-coordinator.sqlite")).unwrap(),
        "objective",
    )
    .unwrap();
    runtime
        .record(
            "start",
            Command::Start {
                goal: "Import a remote result".into(),
                limits: policy(&setup).maximum,
            },
        )
        .unwrap();
    runtime
        .record(
            "lane",
            Command::CreateAttachedLane {
                id: "lane".into(),
                project: source.id().into(),
                goal: setup.f.work.goal.clone(),
                provider: "codex".into(),
                base: input_version,
            },
        )
        .unwrap();
    runtime
        .record(
            "bind",
            Command::BindWorkspace {
                lane: "lane".into(),
                binding: WorkspaceBinding {
                    source_version: input_version,
                    starting_version: Some(input_version),
                    root: "native-root".into(),
                    digest: "native-digest".into(),
                    installation: "native-installation".into(),
                },
            },
        )
        .unwrap();
    runtime
        .record(
            "dispatch",
            Command::Dispatch {
                lane: "lane".into(),
                run: "run".into(),
            },
        )
        .unwrap();
    let proof = RemotePeerChallenge::issue(
        &mut runtime,
        "lane",
        "run",
        setup.f.work.assignment.clone(),
        public(&setup.f.worker),
    )
    .unwrap();
    let peer_signature = sign(&setup.f.worker, proof.signing_payload()).unwrap();
    proof
        .verify_and_claim(&mut runtime, "peer", &peer_signature)
        .unwrap();
    // Signed worker fixture: retain the old object under a new name and create a distinct object
    // at the old path. The coordinator materializes real independent native local history below.
    let mut initial = original.clone();
    initial.operation = RecordDigest::from_bytes([91; 32]);
    initial.files[0].object = mesh_materializer::ObjectId::from_bytes([41; 16]);
    let mut result = initial.clone();
    result.operation = RecordDigest::from_bytes([92; 32]);
    result.files[0].path = "renamed.txt".into();
    let mut replacement = initial.files[0].clone();
    replacement.object = mesh_materializer::ObjectId::from_bytes([42; 16]);
    result.files.push(replacement);
    let mut entries = setup.manifest.entries().to_vec();
    let mut renamed = entries[0].clone();
    if let crate::fleet::RemoteInputEntry::File { path, .. } = &mut renamed {
        *path = "renamed.txt".into();
    }
    entries.push(renamed);
    let result_manifest = RemoteInputManifest::new(result.operation, entries).unwrap();
    let correspondence =
        RemoteResultCorrespondence::derive(&setup.manifest, &initial, &result_manifest, &result)
            .unwrap();
    let context = RemoteWorkerStatusChallenge::issue(
        &mut runtime,
        "lane",
        "run",
        actor,
        public(&setup.f.worker),
    )
    .unwrap();
    let offer = RemoteSavedResultOffer::sign(
        Json::object([
            ("target", context.body.get("target").unwrap().clone()),
            ("owner", Json::text("01".repeat(32))),
            ("mapping", Json::text("02".repeat(32))),
            ("initial", Json::text(initial.operation.to_string())),
            ("installation", Json::text("fixture")),
            ("checkpoint", Json::text("checkpoint")),
            ("review", Json::text("03".repeat(32))),
            ("version", Json::text(result.operation.to_string())),
            ("manifest", Json::text(result_manifest.bundle().to_string())),
        ]),
        |p| sign(&setup.f.worker, p),
    )
    .unwrap()
    .encode();
    let body = Json::object([
        ("query", Json::text("01".repeat(32))),
        ("observed_ms", Json::Number(1)),
        ("offer", Json::text(&offer)),
        ("evidence", Json::text(correspondence.digest().to_string())),
        ("bytes", Json::Number(correspondence.encoded().len() as u64)),
    ]);
    let attestation = envelope(
        REPLY_SCHEMA,
        body.clone(),
        &sign(&setup.f.worker, &payload(REPLY_SIGNING, &body)).unwrap(),
    );
    let worker = public(&setup.f.worker);
    macro_rules! status {
        ($runtime:expr) => {
            RemoteWorkerStatusRequest {
                runtime: $runtime,
                lane: "lane",
                run: "run",
                coordinator: actor,
                worker,
            }
        };
    }
    let authenticated = AuthenticatedRemoteResultEvidence::verify_retained(
        RemoteResultEvidenceRequest {
            status: status!(&mut runtime),
            offer: &offer,
            input: &setup.manifest,
            result: &result_manifest,
        },
        &attestation,
        correspondence.encoded(),
    )
    .unwrap();
    let mut receiver = NativeRemoteResultReceiver::new(
        &setup.destination,
        result_manifest,
        &offer,
        status!(&mut runtime),
    )
    .unwrap();
    receiver
        .accept(&mut runtime, setup.digest, 0, &setup.bytes, true)
        .unwrap();
    let evidence = receiver
        .record_evidence_receipt(&mut runtime, &setup.manifest, &authenticated)
        .unwrap();
    let local = receiver
        .materialize_result(&mut runtime, &setup.manifest, &evidence, &"a".repeat(32))
        .unwrap()
        .into_result_workspace(
            trusted.clone(),
            crate::CheckpointRuntimeParameters::selected_defaults(),
        )
        .unwrap();
    let correlation = receiver
        .record_local_review(&mut runtime, &setup.manifest, &evidence, &local, actor)
        .unwrap();
    let request_id = "b".repeat(32);
    let request = RemoteProjectCandidateRequest {
        input: &setup.manifest,
        correlation: &correlation,
        source: &source,
        reviewers: &trusted,
        request: &request_id,
        expected_main: None,
    };
    let retained = crate::fleet::RetainedRemoteProjectRequest {
        offer: RecordDigest::from_bytes(*Blake3::digest_bytes(offer.as_bytes()).as_bytes()),
        correlation: correlation.digest(),
        source: &source,
        reviewers: &trusted,
        request: &request_id,
        expected_main: None,
    };
    assert!(
        receiver
            .prepare_project_candidate_import(&mut runtime, &request, actor)
            .is_err(),
        "preparation never creates a missing candidate"
    );
    drop(receiver);
    let wrong = crate::fleet::RetainedRemoteProjectRequest {
        correlation: RecordDigest::from_bytes([0xee; 32]),
        ..retained
    };
    assert!(runtime.stage_retained_remote_project(&wrong).is_err());
    assert_eq!(
        runtime
            .retained_remote_project_source(retained.offer, retained.correlation)
            .unwrap(),
        source.id()
    );
    let context = runtime
        .retained_remote_project_context(retained.offer, retained.correlation, &source, &trusted)
        .unwrap();
    assert_eq!(context.get("observed_main"), Some(&Json::Null));
    assert_eq!(
        context.get("input"),
        Some(&Json::text(input_version.to_string()))
    );
    assert_eq!(context.get("approval_authority"), Some(&Json::Bool(false)));
    let candidate = runtime.stage_retained_remote_project(&retained).unwrap();
    let receiver = NativeRemoteResultReceiver::reopen_content_receipt(
        &setup.destination,
        &offer,
        status!(&mut runtime),
    )
    .unwrap()
    .0;
    assert_eq!(
        candidate,
        receiver
            .stage_project_candidate(&mut runtime, &request)
            .unwrap()
    );
    let prepared = receiver
        .prepare_project_candidate_import(&mut runtime, &request, actor)
        .unwrap();
    let linked = |expected: &str| {
        prepared
            .historical_plan()
            .operations()
            .iter()
            .find_map(|operation| match operation {
                mesh_operations::Operation::LinkDirectoryEntry {
                    name, object_id, ..
                } if name == &mesh_operations::NormalizedName::new(expected).unwrap() => {
                    Some(object_id.to_string())
                }
                _ => None,
            })
            .expect("compiled link")
    };
    assert_eq!(linked("renamed.txt"), original.files[0].object.to_string());
    assert_ne!(linked("result.txt"), original.files[0].object.to_string());
    assert_eq!(
        prepared.context().get("approval_authority"),
        Some(&Json::Bool(false))
    );
    assert_eq!(
        fs::read(source_path.join("result.txt")).unwrap(),
        setup.bytes
    );
    assert!(!source_path.join("renamed.txt").exists());
    source
        .with_fleet_input(input_version, &trusted, |open, main| {
            assert_eq!(main, Json::Null);
            assert_eq!(open.workspace_versions().len(), 1);
            Ok(())
        })
        .unwrap();
    let changed_main = "11".repeat(32);
    let stale = RemoteProjectCandidateRequest {
        expected_main: Some(&changed_main),
        ..request
    };
    assert!(receiver
        .prepare_project_candidate_import(&mut runtime, &stale, actor)
        .is_err());

    let signing_key = || SigningKey::from_bytes(&[67; 32]);
    let refusing = ImportSigner {
        key: signing_key(),
        calls: Default::default(),
        refuse: true,
        cancel: None,
        replace: None,
    };
    assert!(receiver
        .import_project_candidate(&mut runtime, &request, &refusing)
        .is_err());
    assert_eq!(
        receiver
            .inspect_project_candidate_import(&mut runtime, &request, public(&refusing.key))
            .unwrap(),
        Json::Null
    );
    let signer = ImportSigner {
        key: signing_key(),
        calls: Default::default(),
        refuse: false,
        cancel: cancel_while_signing.then(|| setup.f.path.join("project-coordinator.sqlite")),
        replace: (fault == 2).then(|| {
            setup
                .f
                .path
                .join("allocations")
                .join(format!("result-{}", "a".repeat(32)))
        }),
    };
    drop(receiver);
    let mut outcome = runtime.import_retained_remote_project(&retained, &signer);
    let receiver = NativeRemoteResultReceiver::reopen_content_receipt(
        &setup.destination,
        &offer,
        status!(&mut runtime),
    )
    .unwrap()
    .0;
    if let Some(path) = &signer.replace {
        assert!(
            outcome.is_err(),
            "replaced remote allocation must refuse before append"
        );
        source
            .with_fleet_input(input_version, &trusted, |open, _| {
                assert_eq!(open.workspace_versions().len(), 1);
                Ok(())
            })
            .unwrap();
        assert!(correlation
            .reopen(&setup.destination, receiver.manifest(), &trusted)
            .is_err());
        // Restore only this test-created empty substitute, preserving the retained original.
        fs::remove_dir(path).unwrap();
        fs::rename(path.with_extension("preserved"), path).unwrap();
        let pending = receiver
            .inspect_project_candidate_import(&mut runtime, &request, public(&signer.key))
            .unwrap();
        assert_eq!(pending.get("state"), Some(&Json::text("pending")));
        // The exact durable intent supplies the signatures; invoking this signer again would
        // replace the allocation again and fail, so success also proves no signing replay.
        outcome = receiver.import_project_candidate(&mut runtime, &request, &signer);
    }
    assert_eq!(signer.calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    let expected_state = if cancel_while_signing {
        assert!(outcome.is_err());
        "pending"
    } else {
        let outcome = outcome.unwrap();
        assert_eq!(outcome.get("state"), Some(&Json::text("imported")));
        assert_eq!(outcome.get("approval_authority"), Some(&Json::Bool(false)));
        assert_eq!(
            receiver
                .import_project_candidate(&mut runtime, &request, &signer)
                .unwrap(),
            outcome
        );
        assert_eq!(
            signer.calls.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "retry must not sign twice"
        );
        let review = receiver
            .review_imported_project_candidate(&mut runtime, &request, true)
            .unwrap();
        assert_ne!(review.get("review"), Some(&Json::Null));
        assert_eq!(review.get("approval_authority"), Some(&Json::Bool(false)));
        assert_eq!(
            receiver
                .review_imported_project_candidate(&mut runtime, &request, false)
                .unwrap(),
            review
        );
        "imported"
    };
    drop(receiver);
    drop(runtime);
    let mut runtime = Runtime::open(
        FleetStore::open(setup.f.path.join("project-coordinator.sqlite")).unwrap(),
        "objective",
    )
    .unwrap();
    let recorded = runtime
        .recorded_retained_remote_project_import(&retained)
        .unwrap()
        .unwrap();
    assert_eq!(recorded.0, public(&signer.key));
    let recovered = runtime
        .inspect_retained_remote_project_import(&retained, public(&signer.key))
        .unwrap();
    if !cancel_while_signing {
        assert_ne!(
            runtime
                .review_retained_remote_project_import(&retained, false)
                .unwrap()
                .get("review"),
            Some(&Json::Null)
        );
    }
    let receiver = NativeRemoteResultReceiver::reopen_content_receipt(
        &setup.destination,
        &offer,
        status!(&mut runtime),
    )
    .unwrap()
    .0;
    assert_eq!(recovered.get("state"), Some(&Json::text(expected_state)));
    assert_eq!(recorded.1, recovered);
    source
        .with_fleet_input(input_version, &trusted, |open, main| {
            assert_eq!(main, Json::Null);
            assert_eq!(
                open.workspace_versions().len(),
                if cancel_while_signing { 1 } else { 2 }
            );
            if !cancel_while_signing {
                let target = RecordDigest::parse_hex(
                    recovered.get("target").and_then(Json::as_text).unwrap(),
                )
                .unwrap();
                let saved = open.historical_workspace_preview(target).unwrap();
                assert_eq!(
                    saved
                        .files
                        .iter()
                        .find(|f| f.path == "renamed.txt")
                        .unwrap()
                        .object,
                    original.files[0].object
                );
                assert_ne!(
                    saved
                        .files
                        .iter()
                        .find(|f| f.path == "result.txt")
                        .unwrap()
                        .object,
                    original.files[0].object
                );
            }
            Ok(())
        })
        .unwrap();
    assert_eq!(
        fs::read(source_path.join("result.txt")).unwrap(),
        setup.bytes
    );
    assert!(!source_path.join("renamed.txt").exists());
    if !cancel_while_signing {
        runtime.record("cancel", Command::Cancel).unwrap();
    }
    assert_eq!(
        receiver
            .inspect_project_candidate_import(&mut runtime, &request, public(&signer.key))
            .unwrap(),
        recovered
    );
    assert!(receiver
        .import_project_candidate(&mut runtime, &request, &signer)
        .is_err());

    assert!(receiver
        .prepare_project_candidate_import(&mut runtime, &request, actor)
        .is_err());
    assert!(receiver
        .stage_project_candidate(&mut runtime, &request)
        .is_err());
    assert!(
        correlation
            .reopen(&setup.destination, receiver.manifest(), &trusted)
            .is_ok(),
        "cancellation still allows immutable historical reads"
    );
    drop(receiver);
    assert_eq!(
        runtime
            .inspect_retained_remote_project_import(&retained, public(&signer.key))
            .unwrap(),
        recovered
    );
    assert!(runtime
        .import_retained_remote_project(&retained, &signer)
        .is_err());
    assert!(runtime.stage_retained_remote_project(&retained).is_err());
}

struct ImportSigner {
    key: SigningKey,
    calls: std::sync::atomic::AtomicUsize,
    refuse: bool,
    cancel: Option<std::path::PathBuf>,
    replace: Option<std::path::PathBuf>,
}
impl crate::CheckpointSigner for ImportSigner {
    fn public_key(&self) -> PublicKey {
        public(&self.key)
    }
    fn sign(&self, value: &SigningPayload) -> Result<Signature, String> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.refuse {
            return Err("fixture key unavailable".into());
        }
        sign(&self.key, value)
    }
}
impl crate::fleet::CandidateImportSigner for ImportSigner {
    fn sign_import_provenance(&self, value: &SigningPayload) -> Result<Signature, String> {
        if let Some(path) = &self.cancel {
            let mut other = Runtime::open(FleetStore::open(path).unwrap(), "objective").unwrap();
            other
                .record("cancel-during-signing", Command::Cancel)
                .unwrap();
        }
        if let Some(path) = &self.replace {
            fs::rename(path, path.with_extension("preserved")).unwrap();
            fs::create_dir(path).unwrap();
        }
        crate::CheckpointSigner::sign(self, value)
    }
}
#[test]
fn authenticated_remote_candidate_stages_and_compiles_original_objects_without_approval() {
    project_import_journey(0);
}
#[test]
fn remote_import_cancelled_while_signing_retains_pending_intent_without_appending() {
    project_import_journey(1);
}

#[test]
fn remote_import_replaced_allocation_during_signing_refuses_and_exact_pending_intent_recovers() {
    project_import_journey(2);
}
