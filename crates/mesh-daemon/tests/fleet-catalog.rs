//! Durable native discovery must not adopt unknown workers or manufacture missing fleet history.
#![cfg(target_os = "macos")]
use ed25519_dalek::{Signer as _, SigningKey};
use mesh_daemon::fleet::catalog::{
    AttachedFleetRequest, FleetProviderPolicy, NativeFleetDirectory,
};
use mesh_daemon::fleet::{Command, Limits};
use mesh_daemon::ipc::Json;
use mesh_daemon::project_attachment::{
    AttachmentStorage, ObservationLimits, ProvisionedAttachment,
};
use mesh_daemon::{CheckpointRuntimeParameters, TrustedReviewers};
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt as _};
use std::path::PathBuf;
use std::time::Duration;

struct ImportSigner {
    key: SigningKey,
    calls: std::sync::atomic::AtomicUsize,
    refuse: bool,
}
impl mesh_daemon::CheckpointSigner for ImportSigner {
    fn public_key(&self) -> mesh_types::PublicKey {
        mesh_types::PublicKey::from_bytes(self.key.verifying_key().to_bytes())
    }
    fn sign(&self, payload: &mesh_crypto::SigningPayload) -> Result<mesh_types::Signature, String> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.refuse {
            return Err("signing disabled".into());
        }
        Ok(mesh_types::Signature::from_bytes(
            self.key.sign(payload.as_bytes()).to_bytes(),
        ))
    }
}
impl mesh_daemon::fleet::CandidateImportSigner for ImportSigner {
    fn sign_import_provenance(
        &self,
        payload: &mesh_crypto::SigningPayload,
    ) -> Result<mesh_types::Signature, String> {
        mesh_daemon::CheckpointSigner::sign(self, payload)
    }
}

struct Fixture {
    root: PathBuf,
    source: PathBuf,
    catalog: PathBuf,
    history: ProvisionedAttachment,
    request: AttachedFleetRequest,
}
fn parameters() -> CheckpointRuntimeParameters {
    CheckpointRuntimeParameters {
        idle_interval: Some(Duration::from_millis(10)),
        maximum_uncheckpointed_bytes: Some(65_536),
        maximum_uncheckpointed_interval: Some(Duration::from_secs(60)),
    }
}
impl Fixture {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("mesh-fleet-catalog-{name}-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let source = root.join("source");
        let metadata = root.join("metadata");
        let catalog = root.join("fleets");
        for path in [&source, &metadata, &catalog] {
            fs::create_dir(path).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        fs::write(source.join("work.txt"), "saved input\n").unwrap();
        let history = AttachmentStorage::open(&metadata)
            .unwrap()
            .provision(&source)
            .unwrap();
        let captured = history
            .project()
            .capture_inputs(ObservationLimits::default())
            .unwrap();
        let key = SigningKey::from_bytes(&[61; 32]);
        let version = history
            .project()
            .save_capture(
                history.metadata_path(),
                &captured,
                mesh_types::PublicKey::from_bytes(key.verifying_key().to_bytes()),
                |payload| {
                    Ok::<_, String>(mesh_types::Signature::from_bytes(
                        key.sign(payload.as_bytes()).to_bytes(),
                    ))
                },
            )
            .unwrap()
            .operation();
        Self {
            root,
            source,
            catalog,
            history,
            request: AttachedFleetRequest {
                request: "a".repeat(32),
                goal: "Coordinate original work".into(),
                version,
                limits: Limits {
                    lanes: 4,
                    concurrency: 2,
                    depth: 1,
                    retries: 1,
                },
            },
        }
    }
    fn open(&self) -> std::io::Result<NativeFleetDirectory> {
        NativeFleetDirectory::open(&self.catalog, TrustedReviewers::default(), parameters())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn rows(value: &Json) -> &[Json] {
    value.get("fleets").unwrap().as_array().unwrap()
}
fn text<'a>(value: &'a Json, name: &str) -> &'a str {
    value.get(name).unwrap().as_text().unwrap()
}

#[test]
fn provider_policy_binds_retries_receipts_and_restart_without_adoption() {
    for (name, coordinator, providers) in [
        ("policy-legacy", "codex", vec!["codex".into()]),
        ("policy-claude", "claude", vec!["claude".into()]),
        (
            "policy-mixed",
            "claude",
            vec!["codex".into(), "claude".into()],
        ),
    ] {
        let f = Fixture::new(name);
        let catalog = f.open().unwrap();
        let policy = FleetProviderPolicy::new(coordinator, &providers).unwrap();
        let service = catalog
            .create_attached_with_providers(&f.history, &f.request, &policy)
            .unwrap();
        let id = service.objective().unwrap();
        let state = service.native_state().unwrap();
        assert_eq!(state.lanes.len(), 1);
        assert_eq!(state.lanes.values().next().unwrap().provider, coordinator);
        assert_eq!(
            service.admitted_providers().collect::<Vec<_>>(),
            policy.providers().collect::<Vec<_>>()
        );
        let receipt_path = f.catalog.join(&id).join("allocation.json");
        let receipt_bytes = fs::read(&receipt_path).unwrap();
        let receipt = Json::parse(std::str::from_utf8(&receipt_bytes).unwrap()).unwrap();
        let legacy = coordinator == "codex" && providers.len() == 1;
        assert_eq!(
            text(&receipt, "schema"),
            if legacy {
                "mesh.native-fleet-allocation/v1"
            } else {
                "mesh.native-fleet-allocation/v2"
            }
        );
        if legacy {
            assert!(receipt.get("providers").is_none());
            assert!(std::sync::Arc::ptr_eq(
                &service,
                &catalog.create_attached(&f.history, &f.request).unwrap()
            ));
        } else {
            assert_eq!(text(&receipt, "coordinator_provider"), coordinator);
            assert!(catalog.create_attached(&f.history, &f.request).is_err());
        }
        assert!(std::sync::Arc::ptr_eq(
            &service,
            &catalog
                .create_attached_with_providers(&f.history, &f.request, &policy)
                .unwrap()
        ));
        for conflicting in [
            FleetProviderPolicy::default(),
            FleetProviderPolicy::new("claude", &["claude".into()]).unwrap(),
            FleetProviderPolicy::new("claude", &["claude".into(), "codex".into()]).unwrap(),
            FleetProviderPolicy::new("codex", &["claude".into(), "codex".into()]).unwrap(),
        ]
        .into_iter()
        .filter(|candidate| candidate != &policy)
        {
            assert!(catalog
                .create_attached_with_providers(&f.history, &f.request, &conflicting)
                .is_err());
        }
        assert_eq!(fs::read(&receipt_path).unwrap(), receipt_bytes);
        assert_eq!(service.native_state().unwrap(), state);
        drop(service);
        drop(catalog);
        let reopened = f.open().unwrap();
        let restored = reopened.snapshot().unwrap();
        assert_eq!(rows(&restored)[0].get("policy"), Some(&policy.to_json()));
        assert_eq!(
            text(&rows(&restored)[0], "ownership"),
            "restored-unattached"
        );
        assert!(reopened.current_service(&id).is_err());
        let lanes = rows(&restored)[0]
            .get("state")
            .unwrap()
            .get("lanes")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(text(&lanes[0], "provider"), coordinator);
        assert_eq!(fs::read(&receipt_path).unwrap(), receipt_bytes);
    }
}

#[test]
fn persisted_provider_policy_must_agree_with_the_coordinator_ledger() {
    let f = Fixture::new("policy-ledger-mismatch");
    let catalog = f.open().unwrap();
    let policy = FleetProviderPolicy::new("claude", &["claude".into(), "codex".into()]).unwrap();
    let service = catalog
        .create_attached_with_providers(&f.history, &f.request, &policy)
        .unwrap();
    let id = service.objective().unwrap();
    let receipt_path = f.catalog.join(&id).join("allocation.json");
    let original = fs::read(&receipt_path).unwrap();
    drop(service);
    drop(catalog);
    let original_text = std::str::from_utf8(&original).unwrap();
    assert_eq!(
        original_text
            .matches("\"coordinator_provider\":\"claude\"")
            .count(),
        1
    );
    let altered = original_text.replace(
        "\"coordinator_provider\":\"claude\"",
        "\"coordinator_provider\":\"codex\"",
    );
    fs::write(&receipt_path, &altered).unwrap();
    let reopened = f.open().unwrap();
    assert_eq!(
        text(&rows(&reopened.snapshot().unwrap())[0], "ownership"),
        "unavailable"
    );
    assert!(reopened.current_service(&id).is_err());
    assert_eq!(fs::read(&receipt_path).unwrap(), altered.as_bytes());
    drop(reopened);
    fs::write(&receipt_path, original).unwrap();
    let reopened = f.open().unwrap();
    assert_eq!(
        text(&rows(&reopened.snapshot().unwrap())[0], "ownership"),
        "restored-unattached"
    );
}

#[test]
fn catalogue_restarts_with_uncertain_runs_visible_and_no_automatic_reattachment() {
    let f = Fixture::new("restart");
    let catalog = f.open().unwrap();
    let service = catalog.create_attached(&f.history, &f.request).unwrap();
    let id = service.objective().unwrap();
    assert!(std::sync::Arc::ptr_eq(
        &service,
        &catalog.current_service(&id).unwrap()
    ));
    assert!(catalog.current_service("unknown").is_err());
    let state = service.native_state().unwrap();
    let lane = state.lanes.keys().next().unwrap().clone();
    let working = PathBuf::from(state.lanes[&lane].workspace.as_ref().unwrap().root());
    fs::write(working.join("work.txt"), "later lane work\n").unwrap();
    fs::write(f.source.join("work.txt"), "original continues\n").unwrap();
    assert!(std::sync::Arc::ptr_eq(
        &service,
        &catalog.create_attached(&f.history, &f.request).unwrap()
    ));
    let mut conflicting = f.request.clone();
    conflicting.goal = "Different task".into();
    assert!(catalog.create_attached(&f.history, &conflicting).is_err());
    service
        .native_command(
            "dispatch",
            Command::Dispatch {
                lane: lane.clone(),
                run: "uncertain-run".into(),
            },
        )
        .unwrap();
    let before = catalog.snapshot().unwrap();
    assert_eq!(text(&rows(&before)[0], "ownership"), "current-host");
    drop(catalog);
    assert!(
        f.open().is_err(),
        "a retained service still owns the directory lease"
    );
    drop(service);
    fs::rename(&f.source, f.root.join("offline-source")).unwrap();
    let reopened = f.open().unwrap();
    assert!(reopened.current_service(&id).is_err());
    let after = reopened.snapshot().unwrap();
    assert!(reopened.current_service(&id).is_err());
    assert_eq!(rows(&after)[0].get("state"), rows(&before)[0].get("state"));
    assert_eq!(text(&rows(&after)[0], "ownership"), "restored-unattached");
    assert_eq!(text(&rows(&after)[0], "objective"), id);
    assert_eq!(
        fs::read(working.join("work.txt")).unwrap(),
        b"later lane work\n"
    );
    fs::rename(f.root.join("offline-source"), &f.source).unwrap();
    assert!(reopened.create_attached(&f.history, &f.request).is_err());
    assert_eq!(
        fs::read(f.source.join("work.txt")).unwrap(),
        b"original continues\n"
    );
    assert_eq!(fs::read_dir(&f.catalog).unwrap().count(), 1);
}

#[test]
fn missing_and_empty_ledgers_are_reported_without_recreating_history() {
    let f = Fixture::new("missing");
    let catalog = f.open().unwrap();
    let service = catalog.create_attached(&f.history, &f.request).unwrap();
    let id = service.objective().unwrap();
    drop(service);
    drop(catalog);
    let ledger = f.catalog.join(&id).join("fleet.sqlite");
    fs::rename(&ledger, f.root.join("retained.sqlite")).unwrap();
    let reopened = f.open().unwrap();
    let projection = reopened.snapshot().unwrap();
    assert_eq!(text(&rows(&projection)[0], "ownership"), "unavailable");
    assert_eq!(rows(&projection)[0].get("state"), Some(&Json::Null));
    assert!(!ledger.exists());
    assert!(reopened.create_attached(&f.history, &f.request).is_err());
    assert!(!ledger.exists());
    drop(reopened);
    fs::rename(f.root.join("retained.sqlite"), &ledger).unwrap();
    fs::write(&ledger, []).unwrap();
    let reopened = f.open().unwrap();
    let projection = reopened.snapshot().unwrap();
    assert_eq!(text(&rows(&projection)[0], "ownership"), "unavailable");
    assert_eq!(fs::metadata(&ledger).unwrap().len(), 0);
}

#[test]
fn replaced_directory_and_linked_database_refuse_without_mutating_foreign_content() {
    let f = Fixture::new("replacement");
    let catalog = f.open().unwrap();
    let service = catalog.create_attached(&f.history, &f.request).unwrap();
    let id = service.objective().unwrap();
    let folder = f.catalog.join(&id);
    let moved = f.root.join("retained-fleet");
    fs::rename(&folder, &moved).unwrap();
    fs::create_dir(&folder).unwrap();
    assert!(service.native_state().is_err());
    assert_eq!(
        rows(&catalog.snapshot().unwrap())[0].get("state"),
        Some(&Json::Null)
    );
    assert_eq!(fs::read_dir(&folder).unwrap().count(), 0);
    drop(service);
    drop(catalog);
    fs::remove_dir(&folder).unwrap();
    fs::rename(&moved, &folder).unwrap();
    let ledger = folder.join("fleet.sqlite");
    fs::rename(&ledger, f.root.join("kept.sqlite")).unwrap();
    let foreign = f.root.join("foreign");
    fs::write(&foreign, "never alter this").unwrap();
    symlink(&foreign, &ledger).unwrap();
    let reopened = f.open().unwrap();
    assert_eq!(
        text(&rows(&reopened.snapshot().unwrap())[0], "ownership"),
        "unavailable"
    );
    assert!(reopened.create_attached(&f.history, &f.request).is_err());
    assert_eq!(fs::read(&foreign).unwrap(), b"never alter this");
}

#[test]
fn native_catalogue_refuses_competing_owners_invalid_limits_and_source_overlap() {
    let f = Fixture::new("admission");
    let catalog = f.open().unwrap();
    assert!(f.open().is_err());
    let mut invalid = f.request.clone();
    invalid.limits.concurrency = 0;
    assert!(catalog.create_attached(&f.history, &invalid).is_err());
    assert_eq!(fs::read_dir(&f.catalog).unwrap().count(), 0);
    let inside = f.source.join("catalog");
    fs::create_dir(&inside).unwrap();
    fs::set_permissions(&inside, fs::Permissions::from_mode(0o700)).unwrap();
    let bad =
        NativeFleetDirectory::open(&inside, TrustedReviewers::default(), parameters()).unwrap();
    assert!(bad.create_attached(&f.history, &f.request).is_err());
    assert_eq!(fs::read_dir(&inside).unwrap().count(), 0);
    let service = catalog.create_attached(&f.history, &f.request).unwrap();
    let id = service.objective().unwrap();
    drop(service);
    drop(catalog);
    let record = f.catalog.join(id).join("allocation.json");
    let mut bytes = fs::read(&record).unwrap();
    bytes.push(b' ');
    fs::write(&record, bytes).unwrap();
    let reopened = f.open().unwrap();
    assert_eq!(
        text(&rows(&reopened.snapshot().unwrap())[0], "ownership"),
        "unavailable"
    );
}

#[test]
fn active_sidecar_aliases_and_replaced_lane_directory_never_grant_new_authority() {
    let f = Fixture::new("sidecars");
    let catalog = f.open().unwrap();
    let service = catalog.create_attached(&f.history, &f.request).unwrap();
    let id = service.objective().unwrap();
    let folder = f.catalog.join(&id);
    let journal = folder.join("fleet.sqlite-journal");
    let foreign = f.root.join("foreign");
    fs::write(&foreign, "preserved").unwrap();
    fs::set_permissions(&foreign, fs::Permissions::from_mode(0o600)).unwrap();
    for hard in [false, true] {
        if hard {
            fs::hard_link(&foreign, &journal).unwrap();
        } else {
            symlink(&foreign, &journal).unwrap();
        }
        assert!(service.native_command("cancel", Command::Cancel).is_err());
        fs::remove_file(&journal).unwrap();
        assert!(!service.native_state().unwrap().cancelled);
        assert_eq!(fs::read(&foreign).unwrap(), b"preserved");
    }
    fs::rename(folder.join("lanes"), folder.join("retained-lanes")).unwrap();
    fs::create_dir(folder.join("lanes")).unwrap();
    fs::set_permissions(folder.join("lanes"), fs::Permissions::from_mode(0o700)).unwrap();
    assert!(service.native_state().is_err());
    drop(service);
    drop(catalog);
    let reopened = f.open().unwrap();
    assert_eq!(
        text(&rows(&reopened.snapshot().unwrap())[0], "ownership"),
        "unavailable"
    );
    assert!(reopened.create_attached(&f.history, &f.request).is_err());
    assert_eq!(fs::read_dir(folder.join("lanes")).unwrap().count(), 0);
    assert_eq!(
        fs::read_dir(folder.join("retained-lanes")).unwrap().count(),
        1
    );
}

#[test]
fn maximum_encoded_goal_remains_readable_after_restart() {
    let mut f = Fixture::new("escaped-goal");
    f.request.goal = "\u{0001}".repeat(8192);
    let catalog = f.open().unwrap();
    let service = catalog.create_attached(&f.history, &f.request).unwrap();
    assert_eq!(
        service.native_state().unwrap().goal.as_deref(),
        Some(f.request.goal.as_str())
    );
    drop(service);
    drop(catalog);
    let reopened = f.open().unwrap();
    assert_eq!(
        text(&rows(&reopened.snapshot().unwrap())[0], "ownership"),
        "restored-unattached"
    );
}

struct HistorySigner(SigningKey);
impl mesh_daemon::fleet::service::CheckpointSigner for HistorySigner {
    fn public_key(&self) -> mesh_types::PublicKey {
        mesh_types::PublicKey::from_bytes(self.0.verifying_key().to_bytes())
    }
    fn sign(&self, payload: &mesh_crypto::SigningPayload) -> Result<mesh_types::Signature, String> {
        Ok(mesh_types::Signature::from_bytes(
            self.0.sign(payload.as_bytes()).to_bytes(),
        ))
    }
}
#[test]
fn history_discovery_reopens_attached_results_offline_without_execution_or_reallocation() {
    use mesh_daemon::fleet::service::SavedReviewSelection;
    let f = Fixture::new("history-offline");
    let catalog = f.open().unwrap();
    let service = catalog.create_attached(&f.history, &f.request).unwrap();
    let objective = service.objective().unwrap();
    let state = service.native_state().unwrap();
    let lane = state.lanes.keys().next().unwrap().clone();
    let working = PathBuf::from(state.lanes[&lane].workspace.as_ref().unwrap().root());
    service
        .native_command(
            "dispatch",
            Command::Dispatch {
                lane: lane.clone(),
                run: "saved-run".into(),
            },
        )
        .unwrap();
    let credential = service
        .grant_with_signer(
            &lane,
            "saved-run",
            "saved-session",
            std::sync::Arc::new(HistorySigner(SigningKey::from_bytes(&[63; 32]))),
        )
        .unwrap();
    fs::write(working.join("work.txt"), "saved agent result\n").unwrap();
    let saved = service
        .agent_call(
            credential.transport_value(),
            "checkpoint",
            &Json::object([("request", Json::text("save"))]),
        )
        .unwrap();
    let reviewed = service
        .agent_call(
            credential.transport_value(),
            "submit_review",
            &Json::object([("checkpoint", saved.get("checkpoint").unwrap().clone())]),
        )
        .unwrap();
    let selection = SavedReviewSelection::new(
        &lane,
        text(&saved, "checkpoint"),
        text(&saved, "version"),
        text(&reviewed, "bundle"),
    )
    .unwrap();
    let feedback = service
        .request_review_changes(
            "native-review-feedback",
            &selection,
            "Add the missing example without replacing the opening.",
        )
        .unwrap();
    fs::write(working.join("work.txt"), "proposed revised result\n").unwrap();
    let revised = service
        .agent_call(
            credential.transport_value(),
            "checkpoint",
            &Json::object([("request", Json::text("revised"))]),
        )
        .unwrap();
    service
        .agent_call(
            credential.transport_value(),
            "submit_review",
            &Json::object([("checkpoint", revised.get("checkpoint").unwrap().clone())]),
        )
        .unwrap();
    service
        .agent_call(
            credential.transport_value(),
            "propose_review_change_result",
            &Json::object([
                ("request", feedback.get("id").unwrap().clone()),
                ("checkpoint", revised.get("checkpoint").unwrap().clone()),
            ]),
        )
        .unwrap();
    service
        .decide_review_change(
            &selection,
            text(&feedback, "id"),
            "history-decision",
            0,
            Some(text(&revised, "checkpoint")),
            |_| true,
        )
        .unwrap();
    let activity = service.saved_review_change_activity(&selection).unwrap();
    let mapping = service
        .saved_project_mapping(&selection, &f.history, &TrustedReviewers::default(), None)
        .unwrap();
    let correspondence = mapping.get("mapping").unwrap();
    assert_eq!(
        correspondence.get("source_project"),
        Some(&Json::text(f.history.id()))
    );
    assert_eq!(
        correspondence.get("source_version"),
        Some(&Json::text(f.request.version.to_string()))
    );
    assert_eq!(correspondence.get("observed_main"), Some(&Json::Null));
    assert_eq!(
        correspondence.get("correspondence").unwrap().get("total"),
        Some(&Json::Number(1))
    );
    let foreign = Fixture::new("mapping-foreign");
    assert!(service
        .saved_project_mapping(
            &selection,
            &foreign.history,
            &TrustedReviewers::default(),
            None
        )
        .is_err());
    fs::write(
        f.source.join("work.txt"),
        "ordinary source work continues\n",
    )
    .unwrap();
    let capture = f
        .history
        .project()
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    let key = SigningKey::from_bytes(&[61; 32]);
    let later_source = f
        .history
        .project()
        .save_capture(
            f.history.metadata_path(),
            &capture,
            mesh_types::PublicKey::from_bytes(key.verifying_key().to_bytes()),
            |payload| {
                Ok::<_, String>(mesh_types::Signature::from_bytes(
                    key.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .unwrap();
    assert_ne!(later_source.operation(), f.request.version);
    assert_eq!(
        service
            .saved_project_mapping(&selection, &f.history, &TrustedReviewers::default(), None)
            .unwrap(),
        mapping
    );
    let expected = service.saved_review(&selection).unwrap();
    let input = service
        .saved_starting_comparison(&selection, None, None)
        .unwrap();
    let page = service.saved_reviews(&lane, None).unwrap();
    fs::write(working.join("work.txt"), "unsaved work survives\n").unwrap();
    drop(service);
    drop(catalog);
    fs::rename(&f.source, f.root.join("offline-original")).unwrap();
    let restored = f.open().unwrap();
    // A pinned review may load before the overview performs discovery.
    let history = restored.history(&objective).unwrap();
    let before = restored.snapshot().unwrap();
    assert_eq!(history.saved_reviews(&lane, None).unwrap(), page);
    assert_eq!(history.saved_review(&selection).unwrap(), expected);
    assert_eq!(
        history.saved_review_change_activity(&selection).unwrap(),
        activity
    );
    assert_eq!(
        history.saved_review_changes(&selection).unwrap(),
        Json::Array(vec![feedback])
    );
    assert_eq!(
        history
            .saved_starting_comparison(&selection, None, None)
            .unwrap(),
        input
    );
    assert_eq!(restored.snapshot().unwrap(), before);
    assert!(restored.current_service(&objective).is_err());
    assert_eq!(text(&rows(&before)[0], "ownership"), "restored-unattached");
    assert_eq!(
        fs::read(working.join("work.txt")).unwrap(),
        b"unsaved work survives\n"
    );
    assert_eq!(fs::read_dir(&f.catalog).unwrap().count(), 1);
    assert!(restored.history("unknown").is_err());
    assert!(history
        .saved_project_mapping(&selection, &f.history, &TrustedReviewers::default(), None)
        .is_err());
    fs::rename(f.root.join("offline-original"), &f.source).unwrap();
    assert_eq!(
        history
            .saved_project_mapping(&selection, &f.history, &TrustedReviewers::default(), None)
            .unwrap(),
        mapping
    );
    assert_eq!(
        fs::read(f.source.join("work.txt")).unwrap(),
        b"ordinary source work continues\n"
    );
    assert_eq!(restored.snapshot().unwrap(), before);
}

#[test]
fn delegated_project_mapping_includes_exact_ancestry_and_survives_restart() {
    use mesh_daemon::fleet::service::{AgentCredential, SavedReviewSelection};
    use std::sync::Arc;
    let mut f = Fixture::new("project-lineage");
    fs::write(f.source.join("obsolete.txt"), "original content\n").unwrap();
    let captured = f
        .history
        .project()
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    let key = SigningKey::from_bytes(&[61; 32]);
    f.request.version = f
        .history
        .project()
        .save_capture(
            f.history.metadata_path(),
            &captured,
            mesh_types::PublicKey::from_bytes(key.verifying_key().to_bytes()),
            |payload| {
                Ok::<_, String>(mesh_types::Signature::from_bytes(
                    key.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .unwrap()
        .operation();
    let catalog = f.open().unwrap();
    let service = catalog.create_attached(&f.history, &f.request).unwrap();
    let objective = service.objective().unwrap();
    let state = service.native_state().unwrap();
    let root_lane = state.lanes.keys().next().unwrap().clone();
    let root = PathBuf::from(state.lanes[&root_lane].workspace.as_ref().unwrap().root());
    let start = |lane: &str, run: &str| {
        service
            .native_command(
                &format!("dispatch-{run}"),
                Command::Dispatch {
                    lane: lane.into(),
                    run: run.into(),
                },
            )
            .unwrap();
        service
            .grant_with_signer(
                lane,
                run,
                &format!("session-{run}"),
                Arc::new(HistorySigner(SigningKey::from_bytes(&[63; 32]))),
            )
            .unwrap()
    };
    let save = |lane: &str, credential: &AgentCredential, request: &str| {
        let saved = service
            .agent_call(
                credential.transport_value(),
                "checkpoint",
                &Json::object([("request", Json::text(request))]),
            )
            .unwrap();
        assert_eq!(
            saved.get("complete"),
            Some(&Json::Bool(true)),
            "{request}: {}",
            saved.encode()
        );
        let review = service
            .agent_call(
                credential.transport_value(),
                "submit_review",
                &Json::object([("checkpoint", saved.get("checkpoint").unwrap().clone())]),
            )
            .unwrap();
        (
            SavedReviewSelection::new(
                lane,
                text(&saved, "checkpoint"),
                text(&saved, "version"),
                text(&review, "bundle"),
            )
            .unwrap(),
            saved,
        )
    };
    let parent_credential = start(&root_lane, "parent-run");
    fs::write(root.join("work.txt"), "parent result\n").unwrap();
    fs::write(root.join("upstream.txt"), "inherited addition\n").unwrap();
    fs::remove_file(root.join("obsolete.txt")).unwrap();
    // Missing managed entries are currently an explicit resolution boundary, not inferred deletion.
    // Preserve that refusal; this native journey proves supported edits/additions. The pure mapping
    // tests separately exercise already-recorded ancestor deletions and recreation identities.
    let incomplete = service
        .agent_call(
            parent_credential.transport_value(),
            "checkpoint",
            &Json::object([("request", Json::text("missing-entry"))]),
        )
        .unwrap();
    assert_eq!(incomplete.get("complete"), Some(&Json::Bool(false)));
    assert_eq!(
        incomplete.get("issue"),
        Some(&Json::text("checkpoint-entry-resolution-required"))
    );
    assert!(service
        .agent_call(
            parent_credential.transport_value(),
            "submit_review",
            &Json::object([("checkpoint", incomplete.get("checkpoint").unwrap().clone())])
        )
        .is_err());
    fs::write(root.join("obsolete.txt"), "original content\n").unwrap();
    let (_, parent_result) = save(&root_lane, &parent_credential, "parent-result");
    let child = service
        .agent_call(
            parent_credential.transport_value(),
            "delegate",
            &Json::object([
                ("request", Json::text("child")),
                ("goal", Json::text("Revise inherited work")),
                ("provider", Json::text("codex")),
                ("version", parent_result.get("version").unwrap().clone()),
            ]),
        )
        .unwrap();
    let child_lane = text(&child, "id");
    let child_root = PathBuf::from(text(child.get("workspace").unwrap(), "root"));
    let child_credential = start(child_lane, "child-run");
    fs::write(child_root.join("work.txt"), "child result\n").unwrap();
    let (selection, _) = save(child_lane, &child_credential, "child-result");
    // Later parent work must not replace the exact parent version used by the delegation.
    fs::write(root.join("unrelated-later.txt"), "not inherited\n").unwrap();
    save(&root_lane, &parent_credential, "later-parent-result");
    fs::write(f.source.join("work.txt"), "ongoing ordinary work\n").unwrap();
    let before = service.native_state().unwrap();
    let mapping = service
        .saved_project_mapping(&selection, &f.history, &TrustedReviewers::default(), None)
        .unwrap();
    assert_eq!(service.native_state().unwrap(), before);
    assert_eq!(text(&mapping, "schema"), "mesh.fleet-project-mapping/v2");
    let result = mapping.get("mapping").unwrap();
    let lineage = result.get("lineage").unwrap().as_array().unwrap();
    assert_eq!(lineage.len(), 2);
    assert_eq!(
        lineage[0].get("result_version"),
        parent_result.get("version")
    );
    assert_eq!(
        lineage[1].get("source_version"),
        parent_result.get("version")
    );
    let changes = result
        .get("correspondence")
        .unwrap()
        .get("changes")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(changes.len(), 2);
    assert!(changes
        .iter()
        .any(|row| row.get("result").and_then(|v| v.get("path"))
            == Some(&Json::text("upstream.txt"))
            && row.get("source") == Some(&Json::Null)));
    assert!(!mapping.encode().contains("unrelated-later.txt"));
    let moved = root.with_extension("temporarily-moved");
    fs::rename(&root, &moved).unwrap();
    fs::create_dir(&root).unwrap();
    let refused = service
        .saved_project_mapping(&selection, &f.history, &TrustedReviewers::default(), None)
        .is_err();
    fs::remove_dir(&root).unwrap();
    fs::rename(&moved, &root).unwrap();
    assert!(refused, "a replaced ancestor must refuse the whole mapping");
    let candidate_request = "b".repeat(32);
    let candidate_root = f.history.metadata_path().join("fleet-candidates");
    let versions_before = f
        .history
        .project()
        .saved_versions(f.history.metadata_path())
        .unwrap();
    assert!(service
        .inspect_project_candidate(
            &selection,
            &f.history,
            &TrustedReviewers::default(),
            &candidate_request,
            None
        )
        .is_err());
    assert!(
        !candidate_root.exists(),
        "inspection cannot create candidate storage"
    );
    assert!(service
        .stage_project_candidate(
            &selection,
            &f.history,
            &TrustedReviewers::default(),
            &candidate_request,
            Some(&"0".repeat(64))
        )
        .is_err());
    assert!(
        !candidate_root.exists(),
        "stale main cannot start a candidate"
    );
    fs::write(
        child_root.join("work.txt"),
        "later uncheckpointed child work\n",
    )
    .unwrap();
    let candidate = service
        .stage_project_candidate(
            &selection,
            &f.history,
            &TrustedReviewers::default(),
            &candidate_request,
            None,
        )
        .unwrap();
    let import_actor = mesh_types::PublicKey::from_bytes(
        ed25519_dalek::SigningKey::from_bytes(&[123; 32])
            .verifying_key()
            .to_bytes(),
    );
    let import_plan = service
        .prepare_project_candidate_import(
            &selection,
            &f.history,
            &TrustedReviewers::default(),
            &candidate_request,
            None,
            import_actor,
        )
        .unwrap();
    let retry_plan = service
        .prepare_project_candidate_import(
            &selection,
            &f.history,
            &TrustedReviewers::default(),
            &candidate_request,
            None,
            import_actor,
        )
        .unwrap();
    assert_eq!(import_plan.context(), retry_plan.context());
    assert_eq!(
        import_plan.context().get("prepared_files"),
        Some(&Json::Number(2))
    );
    assert_eq!(
        import_plan.context().get("approval_authority"),
        Some(&Json::Bool(false))
    );
    assert!(service
        .prepare_project_candidate_import(
            &selection,
            &f.history,
            &TrustedReviewers::default(),
            &candidate_request,
            Some(&"0".repeat(64)),
            import_actor,
        )
        .is_err());
    let retained = candidate_root.join(text(&candidate, "candidate"));
    assert_eq!(
        fs::read(retained.join("files/work.txt")).unwrap(),
        b"child result\n"
    );
    assert_eq!(
        fs::read(retained.join("files/upstream.txt")).unwrap(),
        b"inherited addition\n"
    );
    assert!(!retained.join("files/unrelated-later.txt").exists());
    assert_eq!(text(&candidate, "state"), "staged");
    let project_review = service
        .review_project_candidate(
            &selection,
            &f.history,
            &TrustedReviewers::default(),
            &candidate_request,
            None,
            (None, None),
        )
        .unwrap();
    assert_eq!(
        project_review.get("comparison").unwrap().get("total"),
        Some(&Json::Number(3)),
        "genesis review must include all proposed files, not only the two lane changes"
    );
    let project_detail = service
        .review_project_candidate(
            &selection,
            &f.history,
            &TrustedReviewers::default(),
            &candidate_request,
            None,
            (None, Some("work.txt")),
        )
        .unwrap();
    assert_eq!(project_detail.get("review"), project_review.get("review"));
    let row = &project_detail
        .get("comparison")
        .unwrap()
        .get("changes")
        .unwrap()
        .as_array()
        .unwrap()[0];
    assert_eq!(row.get("before"), Some(&Json::Null));
    assert_eq!(
        row.get("after").unwrap().get("text"),
        Some(&Json::text("child result\n"))
    );
    assert!(service
        .review_project_candidate(
            &selection,
            &f.history,
            &TrustedReviewers::default(),
            &candidate_request,
            None,
            (None, Some("../work.txt"))
        )
        .is_err());
    assert!(service
        .review_project_candidate(
            &selection,
            &f.history,
            &TrustedReviewers::default(),
            &candidate_request,
            None,
            (Some("work.txt"), Some("work.txt"))
        )
        .is_err());
    assert_eq!(
        candidate.get("approval_authority"),
        Some(&Json::Bool(false))
    );
    assert_eq!(
        candidate.get("provenance").unwrap().get("lineage"),
        result.get("lineage")
    );
    assert_eq!(
        service
            .stage_project_candidate(
                &selection,
                &f.history,
                &TrustedReviewers::default(),
                &candidate_request,
                None
            )
            .unwrap(),
        candidate
    );
    assert!(service
        .stage_project_candidate(
            &selection,
            &f.history,
            &TrustedReviewers::default(),
            &candidate_request,
            Some(&"0".repeat(64))
        )
        .is_err());
    assert_eq!(fs::read_dir(&candidate_root).unwrap().count(), 1);
    assert_eq!(
        f.history
            .project()
            .saved_versions(f.history.metadata_path())
            .unwrap(),
        versions_before
    );
    assert_eq!(
        f.history
            .accepted_main(&TrustedReviewers::default())
            .unwrap(),
        Json::Null
    );
    let signer = ImportSigner {
        key: SigningKey::from_bytes(&[123; 32]),
        calls: std::sync::atomic::AtomicUsize::new(0),
        refuse: false,
    };
    assert_eq!(
        service
            .inspect_project_candidate_import(
                &selection,
                &f.history,
                &TrustedReviewers::default(),
                &candidate_request,
                None,
                import_actor
            )
            .unwrap(),
        Json::Null
    );
    let imported = service
        .import_project_candidate(
            &selection,
            &f.history,
            &TrustedReviewers::default(),
            &candidate_request,
            None,
            &signer,
        )
        .unwrap();
    assert_eq!(text(&imported, "state"), "imported");
    assert_eq!(signer.calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert_eq!(imported.get("approval_authority"), Some(&Json::Bool(false)));
    assert_eq!(
        f.history
            .project()
            .saved_versions(f.history.metadata_path())
            .unwrap(),
        versions_before
    );
    assert_eq!(
        f.history
            .accepted_main(&TrustedReviewers::default())
            .unwrap(),
        Json::Null
    );
    let unsigned_retry = ImportSigner {
        key: SigningKey::from_bytes(&[123; 32]),
        calls: std::sync::atomic::AtomicUsize::new(0),
        refuse: true,
    };
    assert_eq!(
        service
            .import_project_candidate(
                &selection,
                &f.history,
                &TrustedReviewers::default(),
                &candidate_request,
                None,
                &unsigned_retry
            )
            .unwrap(),
        imported
    );
    assert_eq!(
        unsigned_retry
            .calls
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
    let import_receipt = fs::read(retained.join("import.json")).unwrap();
    let mut altered = Json::parse(std::str::from_utf8(&import_receipt).unwrap()).unwrap();
    if let Json::Object(fields) = &mut altered {
        *fields
            .iter_mut()
            .find(|(key, _)| key == "signature")
            .unwrap() = ("signature".into(), Json::text("00".repeat(64)));
    }
    fs::write(retained.join("import.json"), altered.encode()).unwrap();
    assert!(service
        .inspect_project_candidate_import(
            &selection,
            &f.history,
            &TrustedReviewers::default(),
            &candidate_request,
            None,
            import_actor
        )
        .is_err());
    fs::write(retained.join("import.json"), &import_receipt).unwrap();
    let review = f
        .history
        .request_review_with_trusted_reviewers(
            text(&imported, "target"),
            import_actor,
            &TrustedReviewers::default(),
        )
        .unwrap();
    assert!(review.get("bundle").is_some());
    drop(service);
    drop(catalog);
    let reopened = f.open().unwrap();
    let history = reopened.history(&objective).unwrap();
    assert_eq!(
        history
            .inspect_project_candidate_import(
                &selection,
                &f.history,
                &TrustedReviewers::default(),
                &candidate_request,
                None,
                import_actor
            )
            .unwrap(),
        imported
    );
    assert_eq!(
        fs::read(retained.join("import.json")).unwrap(),
        import_receipt
    );
    let before = reopened.snapshot().unwrap();
    assert_eq!(
        history
            .saved_project_mapping(&selection, &f.history, &TrustedReviewers::default(), None)
            .unwrap(),
        mapping
    );
    assert_eq!(reopened.snapshot().unwrap(), before);
    assert!(reopened.current_service(&objective).is_err());
    assert_eq!(
        history
            .review_project_candidate(
                &selection,
                &f.history,
                &TrustedReviewers::default(),
                &candidate_request,
                None,
                (None, None)
            )
            .unwrap(),
        project_review
    );
    assert_eq!(
        history
            .inspect_project_candidate(
                &selection,
                &f.history,
                &TrustedReviewers::default(),
                &candidate_request,
                None
            )
            .unwrap(),
        candidate
    );
    assert!(history
        .inspect_project_candidate(
            &selection,
            &f.history,
            &TrustedReviewers::default(),
            &"c".repeat(32),
            None
        )
        .is_err());
    assert_eq!(fs::read_dir(&candidate_root).unwrap().count(), 1);
    fs::write(
        retained.join("files/work.txt"),
        "changed retained candidate\n",
    )
    .unwrap();
    assert!(history
        .review_project_candidate(
            &selection,
            &f.history,
            &TrustedReviewers::default(),
            &candidate_request,
            None,
            (None, None)
        )
        .is_err());
    assert!(history
        .inspect_project_candidate(
            &selection,
            &f.history,
            &TrustedReviewers::default(),
            &candidate_request,
            None
        )
        .is_err());
    assert_eq!(
        fs::read(retained.join("files/work.txt")).unwrap(),
        b"changed retained candidate\n"
    );
    assert_eq!(
        f.history
            .project()
            .saved_versions(f.history.metadata_path())
            .unwrap(),
        versions_before
    );
    assert_eq!(reopened.snapshot().unwrap(), before);
    assert_eq!(
        fs::read(f.source.join("work.txt")).unwrap(),
        b"ongoing ordinary work\n"
    );
    assert_eq!(
        fs::read(f.source.join("obsolete.txt")).unwrap(),
        b"original content\n"
    );
}

#[test]
fn candidate_review_keeps_its_verified_main_base_after_main_advances() {
    use mesh_approval::{
        ApprovalDecision, ExpectedHumanApproval, HumanApprovalCredential, HumanApprovalReceiptDraft,
    };
    use mesh_daemon::fleet::service::SavedReviewSelection;
    use ring::rand::SystemRandom;
    use ring::signature::{EcdsaKeyPair, KeyPair as _, ECDSA_P256_SHA256_ASN1_SIGNING};
    use std::sync::Arc;
    let f = Fixture::new("candidate-review-main");
    let random = SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &random).unwrap();
    let key =
        EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &random).unwrap();
    let credential =
        HumanApprovalCredential::from_public_key(key.public_key().as_ref().try_into().unwrap())
            .unwrap();
    let trust = TrustedReviewers::with_human_credentials([credential.clone()]);
    // Cryptographic fixture receipts exercise the native fold; this is not OS user-presence proof.
    let approve = |version: &str, challenge: u8| {
        let review = f
            .history
            .request_review_with_trusted_reviewers(
                version,
                mesh_types::PublicKey::from_bytes([3; 32]),
                &trust,
            )
            .unwrap();
        let preview = f
            .history
            .approval_preview(text(&review, "bundle"), version, &trust)
            .unwrap();
        let draft = HumanApprovalReceiptDraft::new(
            ExpectedHumanApproval::new(
                preview.context().clone(),
                credential.clone(),
                [challenge; 32],
            ),
            ApprovalDecision::Approve,
        );
        let signature = key.sign(&random, &draft.canonical_bytes()).unwrap();
        let receipt = draft
            .with_signature(signature.as_ref().to_vec())
            .unwrap()
            .canonical_bytes();
        f.history
            .approve_review(text(&review, "bundle"), version, &receipt, &trust)
            .unwrap()
    };
    let first_main = approve(&f.request.version.to_string(), 1).to_string();
    let catalog = f.open().unwrap();
    let service = catalog.create_attached(&f.history, &f.request).unwrap();
    let state = service.native_state().unwrap();
    let lane = state.lanes.keys().next().unwrap();
    let root = PathBuf::from(state.lanes[lane].workspace.as_ref().unwrap().root());
    service
        .native_command(
            "dispatch",
            Command::Dispatch {
                lane: lane.clone(),
                run: "run".into(),
            },
        )
        .unwrap();
    let agent = service
        .grant_with_signer(
            lane,
            "run",
            "session",
            Arc::new(HistorySigner(SigningKey::from_bytes(&[63; 32]))),
        )
        .unwrap();
    fs::write(root.join("work.txt"), "candidate result\n").unwrap();
    let saved = service
        .agent_call(
            agent.transport_value(),
            "checkpoint",
            &Json::object([("request", Json::text("save"))]),
        )
        .unwrap();
    let reviewed = service
        .agent_call(
            agent.transport_value(),
            "submit_review",
            &Json::object([("checkpoint", saved.get("checkpoint").unwrap().clone())]),
        )
        .unwrap();
    let selection = SavedReviewSelection::new(
        lane,
        text(&saved, "checkpoint"),
        text(&saved, "version"),
        text(&reviewed, "bundle"),
    )
    .unwrap();
    let request = "e".repeat(32);
    let candidate = service
        .stage_project_candidate(&selection, &f.history, &trust, &request, Some(&first_main))
        .unwrap();
    let signer = ImportSigner {
        key: SigningKey::from_bytes(&[124; 32]),
        calls: std::sync::atomic::AtomicUsize::new(0),
        refuse: false,
    };
    let imported = service
        .import_project_candidate(
            &selection,
            &f.history,
            &trust,
            &request,
            Some(&first_main),
            &signer,
        )
        .unwrap();
    let imported_review = service
        .review_imported_project_candidate(
            &selection,
            &f.history,
            &trust,
            &request,
            Some(&first_main),
            true,
        )
        .unwrap();
    let unimported_request = "d".repeat(32);
    service
        .stage_project_candidate(
            &selection,
            &f.history,
            &trust,
            &unimported_request,
            Some(&first_main),
        )
        .unwrap();
    let original = service
        .review_project_candidate(
            &selection,
            &f.history,
            &trust,
            &request,
            Some(&first_main),
            (None, Some("work.txt")),
        )
        .unwrap();
    assert_eq!(original.get("base_is_current"), Some(&Json::Bool(true)));
    assert!(
        f.history
            .approval_preview(
                text(&original, "review"),
                &f.request.version.to_string(),
                &trust
            )
            .is_err(),
        "a candidate content-review digest is not a signable source-history review bundle"
    );
    let before_text = |review: &Json| {
        review
            .get("comparison")
            .unwrap()
            .get("changes")
            .unwrap()
            .as_array()
            .unwrap()[0]
            .get("before")
            .unwrap()
            .get("text")
            .unwrap()
            .clone()
    };
    assert_eq!(before_text(&original), Json::text("saved input\n"));
    fs::write(f.source.join("work.txt"), "new accepted main\n").unwrap();
    let capture = f
        .history
        .project()
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    let capture_key = SigningKey::from_bytes(&[61; 32]);
    let next = f
        .history
        .project()
        .save_capture(
            f.history.metadata_path(),
            &capture,
            mesh_types::PublicKey::from_bytes(capture_key.verifying_key().to_bytes()),
            |payload| {
                Ok::<_, String>(mesh_types::Signature::from_bytes(
                    capture_key.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .unwrap()
        .operation();
    let next_main = approve(&next.to_string(), 2).to_string();
    let old_review = service
        .review_imported_project_candidate(
            &selection,
            &f.history,
            &trust,
            &request,
            Some(&first_main),
            false,
        )
        .unwrap();
    assert_eq!(old_review.get("review"), imported_review.get("review"));
    assert_eq!(old_review.get("base_is_current"), Some(&Json::Bool(false)));
    assert_eq!(
        service
            .review_imported_project_candidate(
                &selection,
                &f.history,
                &trust,
                &request,
                Some(&first_main),
                true
            )
            .unwrap(),
        old_review
    );
    assert!(service
        .review_imported_project_candidate(
            &selection,
            &f.history,
            &TrustedReviewers::default(),
            &request,
            Some(&first_main),
            true
        )
        .is_err());

    let retry = ImportSigner {
        key: SigningKey::from_bytes(&[124; 32]),
        calls: std::sync::atomic::AtomicUsize::new(0),
        refuse: true,
    };
    assert_eq!(
        service
            .import_project_candidate(
                &selection,
                &f.history,
                &trust,
                &request,
                Some(&first_main),
                &retry
            )
            .unwrap(),
        imported
    );
    assert!(service
        .import_project_candidate(
            &selection,
            &f.history,
            &trust,
            &unimported_request,
            Some(&first_main),
            &retry
        )
        .is_err());
    assert_eq!(retry.calls.load(std::sync::atomic::Ordering::SeqCst), 0);

    assert_eq!(
        service
            .stage_project_candidate(&selection, &f.history, &trust, &request, Some(&first_main))
            .unwrap(),
        candidate
    );
    let stale = service
        .review_project_candidate(
            &selection,
            &f.history,
            &trust,
            &request,
            Some(&first_main),
            (None, Some("work.txt")),
        )
        .unwrap();
    assert_eq!(stale.get("review"), original.get("review"));
    assert_eq!(stale.get("context"), original.get("context"));
    assert_eq!(stale.get("comparison"), original.get("comparison"));
    assert_eq!(stale.get("base_is_current"), Some(&Json::Bool(false)));
    assert_eq!(
        stale.get("observed_main").unwrap().get("head"),
        Some(&Json::text(&next_main))
    );
    assert!(service
        .review_project_candidate(
            &selection,
            &f.history,
            &TrustedReviewers::default(),
            &request,
            Some(&first_main),
            (None, None)
        )
        .is_err());
    let new_request = "f".repeat(32);
    assert!(service
        .stage_project_candidate(
            &selection,
            &f.history,
            &trust,
            &new_request,
            Some(&first_main)
        )
        .is_err());
    service
        .stage_project_candidate(
            &selection,
            &f.history,
            &trust,
            &new_request,
            Some(&next_main),
        )
        .unwrap();
    let refreshed = service
        .review_project_candidate(
            &selection,
            &f.history,
            &trust,
            &new_request,
            Some(&next_main),
            (None, Some("work.txt")),
        )
        .unwrap();
    assert_ne!(refreshed.get("review"), original.get("review"));
    assert_eq!(before_text(&refreshed), Json::text("new accepted main\n"));
    assert_eq!(
        refreshed.get("approval_authority"),
        Some(&Json::Bool(false))
    );
    assert_eq!(
        fs::read(f.source.join("work.txt")).unwrap(),
        b"new accepted main\n"
    );
    assert_eq!(
        f.history.accepted_main(&trust).unwrap().get("head"),
        Some(&Json::text(next_main))
    );
}

#[test]
fn deletion_only_lane_imports_as_exact_project_deletion_and_reopens_without_execution() {
    use mesh_approval::{
        ApprovalDecision, ExpectedHumanApproval, HumanApprovalCredential, HumanApprovalReceiptDraft,
    };
    use mesh_daemon::fleet::service::SavedReviewSelection;
    use ring::rand::SystemRandom;
    use ring::signature::{EcdsaKeyPair, KeyPair as _, ECDSA_P256_SHA256_ASN1_SIGNING};
    use std::sync::Arc;
    let f = Fixture::new("empty-lane-project-review");
    let random = SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &random).unwrap();
    let key =
        EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &random).unwrap();
    let credential =
        HumanApprovalCredential::from_public_key(key.public_key().as_ref().try_into().unwrap())
            .unwrap();
    let trust = TrustedReviewers::with_human_credentials([credential.clone()]);
    // Cryptographic fixture receipts exercise the native fold; this is not OS user-presence proof.
    let approve = |version: &str, challenge: u8| {
        let review = f
            .history
            .request_review_with_trusted_reviewers(
                version,
                mesh_types::PublicKey::from_bytes([3; 32]),
                &trust,
            )
            .unwrap();
        let preview = f
            .history
            .approval_preview(text(&review, "bundle"), version, &trust)
            .unwrap();
        let draft = HumanApprovalReceiptDraft::new(
            ExpectedHumanApproval::new(
                preview.context().clone(),
                credential.clone(),
                [challenge; 32],
            ),
            ApprovalDecision::Approve,
        );
        let signature = key.sign(&random, &draft.canonical_bytes()).unwrap();
        let receipt = draft
            .with_signature(signature.as_ref().to_vec())
            .unwrap()
            .canonical_bytes();
        f.history
            .approve_review(text(&review, "bundle"), version, &receipt, &trust)
            .unwrap()
    };
    let first_main = approve(&f.request.version.to_string(), 1).to_string();
    let catalog = f.open().unwrap();
    let service = catalog.create_attached(&f.history, &f.request).unwrap();
    let state = service.native_state().unwrap();
    let lane = state.lanes.keys().next().unwrap();
    let root = PathBuf::from(state.lanes[lane].workspace.as_ref().unwrap().root());
    service
        .native_command(
            "dispatch",
            Command::Dispatch {
                lane: lane.clone(),
                run: "run".into(),
            },
        )
        .unwrap();
    let agent = service
        .grant_with_signer(
            lane,
            "run",
            "session",
            Arc::new(HistorySigner(SigningKey::from_bytes(&[63; 32]))),
        )
        .unwrap();

    fs::remove_file(root.join("work.txt")).unwrap();
    let missing = service
        .agent_call(
            agent.transport_value(),
            "missing_files",
            &Json::empty_object(),
        )
        .unwrap();
    let file = &missing.get("files").unwrap().as_array().unwrap()[0];
    service
        .agent_call(
            agent.transport_value(),
            "resolve_file_deletion",
            &Json::object([
                ("request", Json::text("remove-work")),
                ("path", Json::text("work.txt")),
                ("version", file.get("version").unwrap().clone()),
            ]),
        )
        .unwrap();
    let saved = service
        .agent_call(
            agent.transport_value(),
            "checkpoint",
            &Json::object([("request", Json::text("empty"))]),
        )
        .unwrap();
    assert_eq!(saved.get("complete"), Some(&Json::Bool(true)));
    let reviewed = service
        .agent_call(
            agent.transport_value(),
            "submit_review",
            &Json::object([("checkpoint", saved.get("checkpoint").unwrap().clone())]),
        )
        .unwrap();
    let selection = SavedReviewSelection::new(
        lane,
        text(&saved, "checkpoint"),
        text(&saved, "version"),
        text(&reviewed, "bundle"),
    )
    .unwrap();
    let inspected = service.saved_review(&selection).unwrap();
    assert_eq!(
        inspected.get("review").unwrap().get("content_complete"),
        Some(&Json::Bool(true))
    );
    let input = service
        .saved_starting_comparison(&selection, None, None)
        .unwrap();
    let changes = input
        .get("input")
        .unwrap()
        .get("comparison")
        .unwrap()
        .get("changes")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(text(&changes[0], "effect"), "removed");
    assert_eq!(text(changes[0].get("before").unwrap(), "path"), "work.txt");
    assert_eq!(changes[0].get("after"), Some(&Json::Null));
    let request = "9".repeat(32);
    service
        .stage_project_candidate(&selection, &f.history, &trust, &request, Some(&first_main))
        .unwrap();
    let signer = ImportSigner {
        key: SigningKey::from_bytes(&[125; 32]),
        calls: std::sync::atomic::AtomicUsize::new(0),
        refuse: false,
    };
    service
        .import_project_candidate(
            &selection,
            &f.history,
            &trust,
            &request,
            Some(&first_main),
            &signer,
        )
        .unwrap();
    let imported = service
        .review_imported_project_candidate(
            &selection,
            &f.history,
            &trust,
            &request,
            Some(&first_main),
            true,
        )
        .unwrap();
    let review = imported.get("review").unwrap();
    let preview = f
        .history
        .approval_preview(text(review, "bundle"), text(review, "target"), &trust)
        .unwrap();
    assert_eq!(preview.review_bundle().presentation().len(), 1);
    assert_eq!(
        fs::read(f.source.join("work.txt")).unwrap(),
        b"saved input\n"
    );
    let objective = service.objective().unwrap();
    let main_before = f.history.accepted_main(&trust).unwrap();
    let lane = lane.clone();
    fs::write(root.join("later.txt"), b"later private work\n").unwrap();
    drop(service);
    drop(catalog);
    let restored = f.open().unwrap();
    let history = restored.history(&objective).unwrap();
    assert_eq!(history.saved_review(&selection).unwrap(), inspected);
    assert_eq!(
        history
            .saved_starting_comparison(&selection, None, None)
            .unwrap(),
        input
    );
    assert_eq!(
        history.saved_reviews(&lane, None).unwrap().get("total"),
        Some(&Json::Number(1))
    );
    assert!(restored.current_service(&objective).is_err());
    assert_eq!(f.history.accepted_main(&trust).unwrap(), main_before);
    assert_eq!(
        fs::read(root.join("later.txt")).unwrap(),
        b"later private work\n"
    );
}
