use super::*;
use crate::ipc::{nothing_to_recover, Operations, StartupSummary};
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::time::Duration;

pub(in crate::fleet::service) struct Fixture {
    pub(in crate::fleet::service) service: Arc<FleetService>,
    pub(in crate::fleet::service) credential: AgentCredential,
    pub(in crate::fleet::service) lane: String,
    pub(in crate::fleet::service) root: std::path::PathBuf,
}
pub(in crate::fleet::service) fn fixture() -> Fixture {
    let path = std::env::temp_dir().join(format!(
        "mesh-worker-progress-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&path).unwrap();
    let original = path.join("original");
    fs::create_dir(&original).unwrap();
    fs::write(original.join("note.txt"), b"original\n").unwrap();
    let parameters = CheckpointRuntimeParameters {
        idle_interval: Some(std::time::Duration::from_millis(10)),
        maximum_uncheckpointed_bytes: Some(65_536),
        maximum_uncheckpointed_interval: Some(std::time::Duration::from_secs(60)),
    };
    let desktop = crate::LiveDaemon::with_checkpoint_runtime(
        StartupSummary::from(&nothing_to_recover()),
        parameters,
    )
    .unwrap();
    let preview = desktop
        .preview_folder_import(original.to_str().unwrap())
        .unwrap();
    desktop
        .confirm_folder_import(
            original.to_str().unwrap(),
            path.join("source.mesh").to_str().unwrap(),
            preview.get("summary").unwrap().as_text().unwrap(),
        )
        .unwrap();
    let source = desktop.workspace_state().unwrap();
    let input = VersionInput {
        root: source.root,
        digest: source.digest,
        installation: source.installation,
        version: source.workspace_versions[0].operation(),
    };
    let mut runtime = Runtime::open(
        mesh_store::fleet::FleetStore::open(path.join("fleet.sqlite")).unwrap(),
        "objective",
    )
    .unwrap();
    runtime
        .record(
            "start",
            Command::Start {
                goal: "Recover deletion".into(),
                limits: crate::fleet::Limits {
                    lanes: 2,
                    concurrency: 1,
                    depth: 1,
                    retries: 1,
                },
            },
        )
        .unwrap();
    let allocation = path.join("allocations");
    fs::create_dir(&allocation).unwrap();
    fs::set_permissions(&allocation, fs::Permissions::from_mode(0o700)).unwrap();
    let allocator = Arc::new(
        NativeLaneAllocator::open(&allocation, TrustedReviewers::default(), parameters, vec![])
            .unwrap(),
    );
    let service = FleetService::new(runtime, allocator, BTreeSet::from(["codex".into()])).unwrap();
    let lane = service
        .create_root("root", "Resolve deletion", "codex", &input)
        .unwrap();
    service
        .native_command(
            "dispatch",
            Command::Dispatch {
                lane: lane.clone(),
                run: "run".into(),
            },
        )
        .unwrap();

    let credential = service.grant(&lane, "run", "actor", "session").unwrap();
    let root = {
        let inner = service.lock().unwrap();
        std::path::PathBuf::from(exact_state(&inner.workspaces[&lane]).unwrap().root)
    };
    Fixture {
        service: Arc::new(service),
        credential,
        lane,
        root,
    }
}

#[test]
fn worker_progress_classifies_real_files_without_saving_or_ledger_events() {
    let f = fixture();
    let before = f.service.native_state().unwrap();
    let state_before = exact_state(&f.service.lock().unwrap().workspaces[&f.lane]).unwrap();
    for _ in 0..10 {
        assert_eq!(
            f.service.inspect_worker_progress(&f.credential).unwrap(),
            WorkerProgress::Unchanged
        );
    }
    fs::write(f.root.join("note.txt"), b"modified\n").unwrap();
    assert_eq!(
        f.service.inspect_worker_progress(&f.credential).unwrap(),
        WorkerProgress::Changed
    );
    assert_eq!(fs::read(f.root.join("note.txt")).unwrap(), b"modified\n");
    fs::write(f.root.join("note.txt"), b"original\n").unwrap();
    fs::write(f.root.join("new.txt"), b"new\n").unwrap();
    assert_eq!(
        f.service.inspect_worker_progress(&f.credential).unwrap(),
        WorkerProgress::Changed
    );
    fs::remove_file(f.root.join("new.txt")).unwrap();
    fs::create_dir(f.root.join("new-directory")).unwrap();
    assert_eq!(
        f.service.inspect_worker_progress(&f.credential).unwrap(),
        WorkerProgress::Changed
    );
    fs::remove_dir(f.root.join("new-directory")).unwrap();
    fs::remove_file(f.root.join("note.txt")).unwrap();
    assert_eq!(
        f.service.inspect_worker_progress(&f.credential).unwrap(),
        WorkerProgress::NeedsResolution
    );
    let missing = f
        .service
        .agent_call(
            f.credential.transport_value(),
            "missing_files",
            &Json::empty_object(),
        )
        .unwrap();
    assert!(missing.encode().contains("note.txt"));
    fs::write(f.root.join("note.txt"), b"original\n").unwrap();
    std::os::unix::fs::symlink("note.txt", f.root.join("link")).unwrap();
    assert_eq!(
        f.service.inspect_worker_progress(&f.credential).unwrap(),
        WorkerProgress::NeedsResolution
    );
    fs::remove_file(f.root.join("link")).unwrap();
    assert_eq!(
        f.service.inspect_worker_progress(&f.credential).unwrap(),
        WorkerProgress::Unchanged
    );
    assert_eq!(f.service.native_state().unwrap(), before);
    assert_eq!(
        exact_state(&f.service.lock().unwrap().workspaces[&f.lane]).unwrap(),
        state_before
    );
}

#[test]
fn worker_progress_refuses_revocation_rotation_and_cancellation_during_inspection() {
    for action in 0..3 {
        let f = fixture();
        let result = f.service.inspect_agent_inventory_using(
            f.credential.transport_value(),
            |workspace, grant| {
                let inventory = inspect(workspace, grant)?;
                assert!(
                    f.service.inner.try_lock().is_ok(),
                    "inspection must release the fleet lock"
                );
                match action {
                    0 => f.service.revoke(&f.credential).unwrap(),
                    1 => {
                        let _rotated = f.service.grant(&f.lane, "run", "actor", "rotated").unwrap();
                    }
                    _ => f
                        .service
                        .native_command("cancel-inspection", Command::Cancel)
                        .unwrap(),
                }
                Ok(inventory)
            },
        );
        assert!(result.is_err(), "stale inspection must not escape");
    }
}

#[test]
fn worker_progress_keeps_fleet_reads_available_while_inventory_is_paused() {
    let f = fixture();
    let before = f.service.native_state().unwrap();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (resume_tx, resume_rx) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        let service = &f.service;
        let credential = &f.credential;
        let scan = scope.spawn(move || {
            service.inspect_agent_inventory_using(
                credential.transport_value(),
                |workspace, grant| {
                    entered_tx.send(()).unwrap();
                    resume_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                    inspect(workspace, grant)
                },
            )
        });
        entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let (read_tx, read_rx) = std::sync::mpsc::channel();
        scope.spawn(move || {
            read_tx.send(service.native_state()).unwrap();
        });
        let observed = read_rx.recv_timeout(Duration::from_secs(2));
        resume_tx.send(()).unwrap();
        scan.join().unwrap().unwrap();
        assert_eq!(
            observed
                .expect("fleet read blocked behind inventory")
                .unwrap(),
            before
        );
    });
}

#[test]
fn worker_progress_refuses_replaced_root_and_previously_revoked_session() {
    let f = fixture();
    let moved = f.root.with_extension("preserved-original");
    fs::rename(&f.root, &moved).unwrap();
    fs::create_dir(&f.root).unwrap();
    fs::write(f.root.join("note.txt"), b"replacement\n").unwrap();
    assert!(f.service.inspect_worker_progress(&f.credential).is_err());
    assert_eq!(fs::read(f.root.join("note.txt")).unwrap(), b"replacement\n");
    f.service.revoke(&f.credential).unwrap();
    assert!(f.service.inspect_worker_progress(&f.credential).is_err());
    assert_eq!(fs::read(moved.join("note.txt")).unwrap(), b"original\n");
}
