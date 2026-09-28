//! Ordinary manual/harness lanes fork frozen attached content without redirecting existing work.
#![cfg(unix)]
use ed25519_dalek::{Signer as _, SigningKey};
use mesh_daemon::ipc::Json;
use mesh_daemon::project_attachment::{
    AttachmentStorage, ObservationLimits, ProvisionedAttachment,
};
use std::fs;
use std::io::Write as _;
use std::os::unix::fs::{symlink, PermissionsExt as _};
use std::path::PathBuf;

struct Fixture {
    root: PathBuf,
    source: PathBuf,
    storage: AttachmentStorage,
    history: ProvisionedAttachment,
}
impl Fixture {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "mesh-attachment-lanes-{name}-{}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let source = root.join("source");
        fs::create_dir(&source).unwrap();
        let metadata = root.join("metadata");
        fs::create_dir(&metadata).unwrap();
        let storage = AttachmentStorage::open(&metadata).unwrap();
        let history = storage.provision(&source).unwrap();
        Self {
            root,
            source,
            storage,
            history,
        }
    }
    fn save(&self) -> String {
        let key = SigningKey::from_bytes(&[63; 32]);
        let capture = self
            .history
            .project()
            .capture_inputs(ObservationLimits::default())
            .unwrap();
        self.history
            .project()
            .save_capture(
                self.history.metadata_path(),
                &capture,
                mesh_types::PublicKey::from_bytes(key.verifying_key().to_bytes()),
                |body| {
                    Ok::<_, String>(mesh_types::Signature::from_bytes(
                        key.sign(body.as_bytes()).to_bytes(),
                    ))
                },
            )
            .unwrap()
            .operation()
            .to_string()
    }
    fn open(&self, version: &str, request: &str) -> std::io::Result<ProvisionedAttachment> {
        self.storage.open_version_lane(
            &self.history,
            version,
            request,
            ObservationLimits::default(),
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn independent_lanes_use_saved_bytes_preserve_editor_streams_and_retry_without_overwriting() {
    let f = Fixture::new("independent");
    fs::create_dir_all(f.source.join("folder/empty")).unwrap();
    fs::write(f.source.join("folder/binary"), [0, 255, 1, 2]).unwrap();
    fs::write(f.source.join("run"), "saved\n").unwrap();
    fs::set_permissions(f.source.join("run"), fs::Permissions::from_mode(0o755)).unwrap();
    let mut editor = fs::OpenOptions::new()
        .append(true)
        .open(f.source.join("run"))
        .unwrap();
    let version = f.save();
    let mut git = std::process::Command::new("git");
    assert!(git
        .arg("-C")
        .arg(&f.source)
        .args(["init", "--quiet"])
        .status()
        .unwrap()
        .success());
    assert!(std::process::Command::new("git")
        .arg("-C")
        .arg(&f.source)
        .args(["add", "."])
        .status()
        .unwrap()
        .success());
    let index = fs::read(f.source.join(".git/index")).unwrap();
    let head = fs::read(f.source.join(".git/HEAD")).unwrap();
    editor.write_all(b"original continues\n").unwrap();
    let a = f.open(&version, &"a".repeat(32)).unwrap();
    let b = f.open(&version, &"b".repeat(32)).unwrap();
    assert_ne!(a.id(), b.id());
    assert_ne!(a.project().root(), f.source);
    assert_eq!(
        fs::read(a.project().root().join("run")).unwrap(),
        b"saved\n"
    );
    assert_eq!(
        fs::read(b.project().root().join("folder/binary")).unwrap(),
        [0, 255, 1, 2]
    );
    assert!(a.project().root().join("folder/empty").is_dir());
    assert_eq!(
        fs::metadata(a.project().root().join("run"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert!(!a.project().root().join(".git").exists());
    assert!(!a.metadata_path().starts_with(a.project().root()));
    fs::write(a.project().root().join("run"), "lane A continues\n").unwrap();
    let retry = f.open(&version, &"a".repeat(32)).unwrap();
    assert_eq!(retry.id(), a.id());
    assert_eq!(
        fs::read(retry.project().root().join("run")).unwrap(),
        b"lane A continues\n"
    );
    assert_eq!(
        fs::read(b.project().root().join("run")).unwrap(),
        b"saved\n"
    );
    editor.write_all(b"still open\n").unwrap();
    assert_eq!(
        fs::read(f.source.join("run")).unwrap(),
        b"saved\noriginal continues\nstill open\n"
    );
    assert_eq!(fs::read(f.source.join(".git/index")).unwrap(), index);
    assert_eq!(fs::read(f.source.join(".git/HEAD")).unwrap(), head);
    let origin = f.storage.lane_origin(&a).unwrap().unwrap();
    assert_eq!(
        origin.get("source_project"),
        Some(&Json::text(f.history.id()))
    );
    assert_eq!(origin.get("source_version"), Some(&Json::text(&version)));
    assert_eq!(origin.get("attribution"), Some(&Json::text("unknown")));
    assert_eq!(origin.get("provider"), Some(&Json::Null));
    let reopened = AttachmentStorage::open(&f.root.join("metadata")).unwrap();
    assert_eq!(
        reopened
            .lane_origin(&reopened.reopen(a.id()).unwrap())
            .unwrap(),
        Some(origin)
    );
    let new_version = f.save();
    assert!(f.open(&new_version, &"a".repeat(32)).is_err());
    assert_eq!(
        fs::read(a.project().root().join("run")).unwrap(),
        b"lane A continues\n"
    );
}

#[test]
fn lane_allocation_refuses_unknown_versions_limits_partial_records_and_replaced_folders() {
    let f = Fixture::new("refusals");
    fs::write(f.source.join("work"), "saved").unwrap();
    let version = f.save();
    assert!(f.open(&"f".repeat(64), &"a".repeat(32)).is_err());
    assert!(f.open(&version, "../outside").is_err());
    assert!(f
        .storage
        .open_version_lane(
            &f.history,
            &version,
            &"a".repeat(32),
            ObservationLimits {
                bytes: 1,
                ..ObservationLimits::default()
            }
        )
        .is_err());
    assert!(!f.root.join("metadata/work-lanes").exists());
    let a = f.open(&version, &"a".repeat(32)).unwrap();
    let allocation = a.project().root().parent().unwrap();
    let ready = fs::read(allocation.join("ready.json")).unwrap();
    fs::remove_file(allocation.join("ready.json")).unwrap();
    fs::write(a.project().root().join("work"), "retained partial work").unwrap();
    assert!(f.open(&version, &"a".repeat(32)).is_err());
    assert!(f.storage.lane_origin(&a).is_err());
    assert_eq!(
        fs::read_dir(f.root.join("metadata/work-lanes"))
            .unwrap()
            .count(),
        1
    );
    assert_eq!(
        fs::read(a.project().root().join("work")).unwrap(),
        b"retained partial work"
    );
    fs::write(allocation.join("ready.json"), ready).unwrap();
    let kept = f.root.join("kept-lane");
    fs::rename(a.project().root(), &kept).unwrap();
    symlink(&kept, a.project().root()).unwrap();
    assert!(f.open(&version, &"a".repeat(32)).is_err());
    assert!(f.storage.lane_origin(&a).is_err());
    assert!(a.project().native_folder_reference().is_err());
    assert_eq!(
        fs::read(kept.join("work")).unwrap(),
        b"retained partial work"
    );
}

#[test]
fn concurrent_request_retries_share_one_lane_and_do_not_change_saved_history() {
    let f = Fixture::new("concurrent");
    fs::write(f.source.join("work"), "saved").unwrap();
    let version = f.save();
    let before = f.history.saved_versions().unwrap();
    let (a, b) = std::thread::scope(|scope| {
        let left = scope.spawn(|| f.open(&version, &"d".repeat(32)).unwrap());
        let right = scope.spawn(|| f.open(&version, &"d".repeat(32)).unwrap());
        (left.join().unwrap(), right.join().unwrap())
    });
    assert_eq!(a.id(), b.id());
    assert_eq!(f.history.saved_versions().unwrap(), before);
    assert_eq!(
        fs::read_dir(f.root.join("metadata/work-lanes"))
            .unwrap()
            .count(),
        1
    );
}

fn fleet_allocator(
    path: &std::path::Path,
) -> std::sync::Arc<mesh_daemon::fleet::service::NativeLaneAllocator> {
    fs::create_dir(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    std::sync::Arc::new(
        mesh_daemon::fleet::service::NativeLaneAllocator::open(
            path,
            mesh_daemon::TrustedReviewers::default(),
            mesh_daemon::CheckpointRuntimeParameters {
                idle_interval: Some(std::time::Duration::from_millis(10)),
                maximum_uncheckpointed_bytes: Some(65_536),
                maximum_uncheckpointed_interval: Some(std::time::Duration::from_secs(60)),
            },
            vec![],
        )
        .unwrap(),
    )
}

#[test]
fn attached_fleet_roots_delegate_from_saved_bytes_and_replay_project_correlation() {
    use mesh_daemon::fleet::service::FleetService;
    use mesh_daemon::fleet::{Command, Limits, Runtime};
    use mesh_store::{fleet::FleetStore, RecordDigest};
    let f = Fixture::new("managed-fleet");
    fs::write(f.source.join("work.txt"), "saved input\n").unwrap();
    fs::create_dir_all(f.source.join("empty/subdir")).unwrap();
    fs::write(f.source.join("binary"), [0, 255, 1]).unwrap();
    fs::set_permissions(f.source.join("work.txt"), fs::Permissions::from_mode(0o755)).unwrap();
    let version = RecordDigest::parse_hex(&f.save()).unwrap();
    let mut editor = fs::OpenOptions::new()
        .append(true)
        .open(f.source.join("work.txt"))
        .unwrap();
    editor.write_all(b"ongoing source edits\n").unwrap();
    let allocation = fleet_allocator(&f.root.join("managed"));
    let db = f.root.join("fleet.sqlite");
    let mut runtime = Runtime::open(FleetStore::open(&db).unwrap(), "attached-objective").unwrap();
    runtime
        .record(
            "start",
            Command::Start {
                goal: "Coordinate attached work".into(),
                limits: Limits {
                    lanes: 4,
                    concurrency: 3,
                    depth: 1,
                    retries: 1,
                },
            },
        )
        .unwrap();
    let service = FleetService::new(
        runtime,
        allocation.clone(),
        std::collections::BTreeSet::from(["codex".into(), "authorized-other".into()]),
    )
    .unwrap();
    let lane = service
        .create_root_from_attachment("root", "Coordinate", "codex", &f.history, version)
        .unwrap();
    let state = service.native_state().unwrap();
    assert_eq!(
        state.lanes[&lane].source_project.as_deref(),
        Some(f.history.id())
    );
    let root = PathBuf::from(state.lanes[&lane].workspace.as_ref().unwrap().root());
    assert_eq!(fs::read(root.join("work.txt")).unwrap(), b"saved input\n");
    assert_eq!(fs::read(root.join("binary")).unwrap(), [0, 255, 1]);
    assert!(root.join("empty/subdir").is_dir());
    assert_ne!(
        fs::metadata(root.join("work.txt"))
            .unwrap()
            .permissions()
            .mode()
            & 0o111,
        0
    );
    fs::write(root.join("work.txt"), "lane continues\n").unwrap();
    assert_eq!(
        service
            .create_root_from_attachment("root", "Coordinate", "codex", &f.history, version)
            .unwrap(),
        lane
    );
    assert_eq!(
        fs::read(root.join("work.txt")).unwrap(),
        b"lane continues\n"
    );
    let before_refusals = service.native_state().unwrap();
    let allocated_before = fs::read_dir(f.root.join("managed")).unwrap().count();
    for (goal, provider) in [
        ("Different task", "codex"),
        ("Coordinate", "authorized-other"),
        ("Coordinate", "unauthorized"),
    ] {
        assert!(service
            .create_root_from_attachment("root", goal, provider, &f.history, version)
            .is_err());
        assert_eq!(service.native_state().unwrap(), before_refusals);
    }
    let other = Fixture::new("managed-other-project");
    fs::write(other.source.join("work.txt"), "other project").unwrap();
    let other_version = RecordDigest::parse_hex(&other.save()).unwrap();
    assert!(service
        .create_root_from_attachment("root", "Coordinate", "codex", &other.history, other_version)
        .is_err());
    assert_eq!(service.native_state().unwrap(), before_refusals);
    assert_eq!(
        fs::read_dir(f.root.join("managed")).unwrap().count(),
        allocated_before
    );
    assert_eq!(
        fs::read(root.join("work.txt")).unwrap(),
        b"lane continues\n"
    );
    let latest = RecordDigest::parse_hex(&f.save()).unwrap();
    assert!(service
        .create_root_from_attachment("root", "Coordinate", "codex", &f.history, latest)
        .is_err());
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
    let context = service
        .agent_call(
            credential.transport_value(),
            "context",
            &Json::empty_object(),
        )
        .unwrap();
    let input = context
        .get("workspace")
        .unwrap()
        .get("workspace_versions")
        .unwrap()
        .as_array()
        .unwrap()[0]
        .get("operation")
        .unwrap()
        .clone();
    let child = service
        .agent_call(
            credential.transport_value(),
            "delegate",
            &Json::object([
                ("request", Json::text("child")),
                ("goal", Json::text("Independent worker")),
                ("provider", Json::text("codex")),
                ("version", input),
            ]),
        )
        .unwrap();
    assert_eq!(
        child.get("source_project"),
        Some(&Json::text(f.history.id()))
    );
    let child_id = child.get("id").unwrap().as_text().unwrap();
    let state = service.native_state().unwrap();
    let child_root = state.lanes[child_id].workspace.as_ref().unwrap().root();
    assert_eq!(
        fs::read(std::path::Path::new(child_root).join("work.txt")).unwrap(),
        b"saved input\n"
    );
    editor.write_all(b"still open\n").unwrap();
    assert_eq!(
        fs::read(f.source.join("work.txt")).unwrap(),
        b"saved input\nongoing source edits\nstill open\n"
    );
    let replay = Runtime::open(FleetStore::open(&db).unwrap(), "attached-objective").unwrap();
    assert_eq!(replay.state(), &state);
    let restarted = FleetService::new(
        replay,
        allocation,
        std::collections::BTreeSet::from(["codex".into()]),
    )
    .unwrap();
    assert!(restarted
        .create_root_from_attachment("root", "Coordinate", "codex", &f.history, version)
        .is_err());
    assert_eq!(
        fs::read(root.join("work.txt")).unwrap(),
        b"lane continues\n"
    );
    struct LaneSigner(SigningKey);
    impl mesh_daemon::fleet::service::CheckpointSigner for LaneSigner {
        fn public_key(&self) -> mesh_types::PublicKey {
            mesh_types::PublicKey::from_bytes(self.0.verifying_key().to_bytes())
        }
        fn sign(
            &self,
            body: &mesh_crypto::SigningPayload,
        ) -> Result<mesh_types::Signature, String> {
            Ok(mesh_types::Signature::from_bytes(
                self.0.sign(body.as_bytes()).to_bytes(),
            ))
        }
    }
    service
        .native_command(
            "running",
            Command::Observe {
                lane: lane.clone(),
                run: "run".into(),
                state: mesh_daemon::fleet::RunState::Running,
            },
        )
        .unwrap();
    let signed = service
        .grant_with_signer(
            &lane,
            "run",
            "signed",
            std::sync::Arc::new(LaneSigner(SigningKey::from_bytes(&[64; 32]))),
        )
        .unwrap();
    let saved = service
        .agent_call(
            signed.transport_value(),
            "checkpoint",
            &Json::object([("request", Json::text("input-comparison"))]),
        )
        .unwrap();
    let reviewed = service
        .agent_call(
            signed.transport_value(),
            "submit_review",
            &Json::object([("checkpoint", saved.get("checkpoint").unwrap().clone())]),
        )
        .unwrap();
    let selection = mesh_daemon::fleet::service::SavedReviewSelection::new(
        &lane,
        saved.get("checkpoint").unwrap().as_text().unwrap(),
        saved.get("version").unwrap().as_text().unwrap(),
        reviewed.get("bundle").unwrap().as_text().unwrap(),
    )
    .unwrap();
    let comparison = service
        .saved_starting_comparison(&selection, None, None)
        .unwrap();
    let input = comparison.get("input").unwrap();
    assert_eq!(
        input.get("source_version"),
        Some(&Json::text(version.to_string()))
    );
    let changes = input
        .get("comparison")
        .unwrap()
        .get("changes")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(
        changes.len(),
        1,
        "unchanged folders, binary files and imported contents are not lane changes"
    );
    let object = changes[0].get("object").unwrap().as_text().unwrap();
    let detail = service
        .saved_starting_comparison(&selection, None, Some(object))
        .unwrap();
    let changed = &detail
        .get("input")
        .unwrap()
        .get("comparison")
        .unwrap()
        .get("changes")
        .unwrap()
        .as_array()
        .unwrap()[0];
    assert_eq!(
        changed.get("before").unwrap().get("text"),
        Some(&Json::text("saved input\n"))
    );
    assert_eq!(
        changed.get("after").unwrap().get("text"),
        Some(&Json::text("lane continues\n"))
    );
    assert_eq!(
        fs::read(f.source.join("work.txt")).unwrap(),
        b"saved input\nongoing source edits\nstill open\n"
    );
    let retained_state = restarted.native_state().unwrap();
    assert_eq!(
        restarted
            .saved_starting_comparison(&selection, None, None)
            .unwrap(),
        comparison
    );
    assert_eq!(
        restarted
            .saved_starting_comparison(&selection, None, Some(object))
            .unwrap(),
        detail
    );
    assert_eq!(restarted.native_state().unwrap(), retained_state);
    assert!(restarted
        .agent_call(signed.transport_value(), "context", &Json::empty_object())
        .is_err());
    assert!(restarted
        .grant(&lane, "run", "restarted-actor", "restarted-session")
        .is_err());
    assert_eq!(
        fs::read(f.source.join("work.txt")).unwrap(),
        b"saved input\nongoing source edits\nstill open\n"
    );
}

#[test]
fn attached_fleet_allocator_refuses_source_overlap_unknown_versions_and_partial_retry() {
    use mesh_daemon::fleet::service::LaneAllocator as _;
    use mesh_store::RecordDigest;
    let f = Fixture::new("managed-refuse");
    fs::write(f.source.join("work"), "saved").unwrap();
    let version = RecordDigest::parse_hex(&f.save()).unwrap();
    let inside = fleet_allocator(&f.source.join("allocation"));
    let lane = format!("lane-{}", "a".repeat(64));
    assert!(inside
        .allocate_attached(&lane, &f.history, version)
        .is_err());
    assert_eq!(
        fs::read_dir(f.source.join("allocation")).unwrap().count(),
        0
    );
    let outside = fleet_allocator(&f.root.join("managed"));
    assert!(outside
        .allocate_attached(&lane, &f.history, RecordDigest::from_bytes([0; 32]))
        .is_err());
    assert_eq!(fs::read_dir(f.root.join("managed")).unwrap().count(), 0);
    let partial = f.root.join("managed").join(&lane);
    fs::create_dir(&partial).unwrap();
    fs::write(partial.join("keep"), "uncertain work").unwrap();
    assert!(outside
        .allocate_attached(&lane, &f.history, version)
        .is_err());
    assert_eq!(fs::read(partial.join("keep")).unwrap(), b"uncertain work");
    assert!(!partial.join("source").exists());
}
