//! Native lane allocation preserves the selected workspace and isolates sibling edits.
#![cfg(unix)]

use mesh_daemon::fleet::workspace::{LaneWorkspace, VersionInput};
use mesh_daemon::fleet::{Command, Limits, Runtime};
use mesh_daemon::ipc::{nothing_to_recover, Json, Operations, StartupSummary};
use mesh_daemon::{CheckpointRuntimeParameters, LiveDaemon, TrustedReviewers};
use mesh_store::fleet::FleetStore;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

struct Fixture(PathBuf);
impl Fixture {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "mesh-fleet-allocation-{name}-{}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn source(&self) -> (LiveDaemon, VersionInput) {
        let source = self.0.join("original");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("note.txt"), "original\n").unwrap();
        let daemon = LiveDaemon::with_checkpoint_runtime(
            StartupSummary::from(&nothing_to_recover()),
            parameters(),
        )
        .unwrap();
        let preview = daemon
            .preview_folder_import(source.to_str().unwrap())
            .unwrap();
        daemon
            .confirm_folder_import(
                source.to_str().unwrap(),
                self.0.join("source.mesh").to_str().unwrap(),
                preview.get("summary").and_then(Json::as_text).unwrap(),
            )
            .unwrap();
        let state = daemon.workspace_state().unwrap();
        let input = VersionInput {
            root: state.root,
            digest: state.digest,
            installation: state.installation,
            version: state.workspace_versions[0].operation(),
        };
        (daemon, input)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn parameters() -> CheckpointRuntimeParameters {
    CheckpointRuntimeParameters {
        idle_interval: Some(Duration::from_millis(10)),
        maximum_uncheckpointed_bytes: Some(65_536),
        maximum_uncheckpointed_interval: Some(Duration::from_secs(60)),
    }
}
fn fork(
    input: &VersionInput,
    destination: &std::path::Path,
) -> Result<LaneWorkspace, mesh_daemon::ipc::Unavailable> {
    LaneWorkspace::fork(
        input,
        destination,
        &[],
        TrustedReviewers::default(),
        parameters(),
    )
}

#[test]
fn sibling_workspaces_are_independent_and_do_not_switch_desktop_selection() {
    let fixture = Fixture::new("siblings");
    let (desktop, input) = fixture.source();
    let before = desktop.workspace_state().unwrap();
    let a = fork(&input, &fixture.0.join("a.mesh")).unwrap();
    let b = fork(&input, &fixture.0.join("b.mesh")).unwrap();
    let mut runtime = Runtime::open(
        FleetStore::open(fixture.0.join("fleet.sqlite")).unwrap(),
        "objective",
    )
    .unwrap();
    runtime
        .submit(
            0,
            "start",
            Command::Start {
                goal: "Two independent workers".into(),
                limits: Limits {
                    lanes: 2,
                    concurrency: 2,
                    depth: 1,
                    retries: 1,
                },
            },
        )
        .unwrap();
    for (id, allocated) in [("a", &a), ("b", &b)] {
        runtime
            .submit(
                runtime.state().revision,
                &format!("create-{id}"),
                Command::CreateLane {
                    id: id.into(),
                    parent: None,
                    goal: format!("Work on {id}"),
                    provider: "fixture".into(),
                    base: input.version,
                },
            )
            .unwrap();
        runtime
            .submit(
                runtime.state().revision,
                &format!("bind-{id}"),
                Command::BindWorkspace {
                    lane: id.into(),
                    binding: allocated.binding().clone(),
                },
            )
            .unwrap();
    }
    let restored = Runtime::open(
        FleetStore::open(fixture.0.join("fleet.sqlite")).unwrap(),
        "objective",
    )
    .unwrap();
    assert_eq!(restored.state(), runtime.state());
    for (id, allocated) in [("a", &a), ("b", &b)] {
        let initial = allocated.state().unwrap().workspace_versions[0].operation();
        assert_eq!(
            restored.state().lanes[id]
                .workspace
                .as_ref()
                .unwrap()
                .starting_version(),
            Some(initial)
        );
    }

    assert_eq!(
        restored.state().lanes["a"]
            .workspace
            .as_ref()
            .unwrap()
            .root(),
        a.binding().root()
    );
    let a_state = a.state().unwrap();
    let b_state = b.state().unwrap();
    assert_ne!(a_state.root, b_state.root);
    assert_ne!(a_state.installation, b_state.installation);
    assert_eq!(desktop.workspace_state().unwrap().root, before.root);
    assert_eq!(desktop.workspace_state().unwrap().digest, before.digest);
    fs::write(
        PathBuf::from(&a_state.root).join("note.txt"),
        "worker A's unsaved edit\n",
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(PathBuf::from(&b_state.root).join("note.txt")).unwrap(),
        "original\n"
    );
    assert_eq!(
        fs::read_to_string(PathBuf::from(&input.root).join("note.txt")).unwrap(),
        "original\n"
    );
    // Navigate the desktop elsewhere; both lane services stay pinned to their own folder.
    desktop
        .reopen_at_start(std::path::Path::new(&b_state.root))
        .unwrap();
    assert_eq!(a.state().unwrap().root, a_state.root);
    assert_eq!(b.state().unwrap().root, b_state.root);
    assert_eq!(
        a.receipt().get("source_version").and_then(Json::as_text),
        Some(input.version.to_string().as_str())
    );
}

#[test]
fn stale_source_identity_and_occupied_destination_are_preserved() {
    let fixture = Fixture::new("refusals");
    let (_desktop, input) = fixture.source();
    let destination = fixture.0.join("candidate.mesh");
    let mut stale = input.clone();
    stale.installation = "replaced-installation".into();
    assert!(fork(&stale, &destination).is_err());
    assert!(!destination.exists());
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join("keep.txt"), "unrelated work").unwrap();
    assert!(fork(&input, &destination).is_err());
    assert_eq!(
        fs::read_to_string(destination.join("keep.txt")).unwrap(),
        "unrelated work"
    );
}

#[test]
fn missing_source_is_not_recreated_as_an_empty_workspace() {
    let fixture = Fixture::new("missing");
    let (_desktop, mut input) = fixture.source();
    let missing = fixture.0.join("missing");
    input.root = missing.to_string_lossy().into_owned();
    assert!(fork(&input, &fixture.0.join("child.mesh")).is_err());
    assert!(!missing.exists());
}

#[test]
fn replaced_allocation_parent_is_refused_before_writing_into_the_replacement() {
    let fixture = Fixture::new("parent-replaced");
    let (daemon, input) = fixture.source();
    let parent = fixture.0.join("allocation");
    fs::create_dir(&parent).unwrap();
    let identity = mesh_daemon::ProtectedWorkspaceRoot::inspect(&parent).unwrap();
    fs::rename(&parent, fixture.0.join("preserved-allocation")).unwrap();
    fs::create_dir(&parent).unwrap();
    let destination = parent.join("child.mesh");
    let version = input.version.to_string();
    let request = mesh_daemon::WorkspaceVersionForkRequest::new(
        &version,
        destination.to_str().unwrap(),
        &input.root,
        &input.digest,
        &input.installation,
        None,
    )
    .within_parent(identity);
    assert_eq!(
        daemon
            .fork_workspace_version_protected(request)
            .unwrap_err()
            .code,
        "workspace-version-parent-changed"
    );
    assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
    assert_eq!(
        fs::read_dir(fixture.0.join("preserved-allocation"))
            .unwrap()
            .count(),
        0
    );
}
