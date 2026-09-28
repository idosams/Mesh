//! Durable native discovery must not adopt unknown workers or manufacture missing fleet history.
#![cfg(target_os = "macos")]
use ed25519_dalek::{Signer as _, SigningKey};
use mesh_daemon::fleet::catalog::{AttachedFleetRequest, NativeFleetDirectory};
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
}
