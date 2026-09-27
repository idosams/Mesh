//! Agent authority and delegation through actual native lane allocation.
#![cfg(target_os = "macos")]
use mesh_daemon::fleet::service::{AgentCredential, FleetService, NativeLaneAllocator};
use mesh_daemon::fleet::workspace::VersionInput;
use mesh_daemon::fleet::{Command, Limits, RunState, Runtime};
use mesh_daemon::ipc::{nothing_to_recover, Json, Operations, StartupSummary};
use mesh_daemon::{CheckpointRuntimeParameters, LiveDaemon, TrustedReviewers};
use mesh_store::fleet::FleetStore;
use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

struct Fixture {
    path: PathBuf,
    desktop: Arc<LiveDaemon>,
    service: Arc<FleetService>,
    lane: String,
    credential: AgentCredential,
}
impl Fixture {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("mesh-fleet-agent-{name}-{}", std::process::id()));
        fs::create_dir(&path).unwrap();
        let original = path.join("original");
        fs::create_dir(&original).unwrap();
        fs::write(original.join("note.txt"), "immutable input\n").unwrap();
        let parameters = CheckpointRuntimeParameters {
            idle_interval: Some(Duration::from_millis(10)),
            maximum_uncheckpointed_bytes: Some(65_536),
            maximum_uncheckpointed_interval: Some(Duration::from_secs(60)),
        };
        let desktop = Arc::new(
            LiveDaemon::with_checkpoint_runtime(
                StartupSummary::from(&nothing_to_recover()),
                parameters,
            )
            .unwrap(),
        );
        let preview = desktop
            .preview_folder_import(original.to_str().unwrap())
            .unwrap();
        desktop
            .confirm_folder_import(
                original.to_str().unwrap(),
                path.join("source.mesh").to_str().unwrap(),
                text(&preview, "summary"),
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
            FleetStore::open(path.join("fleet.sqlite")).unwrap(),
            "objective",
        )
        .unwrap();
        runtime
            .record(
                "start",
                Command::Start {
                    goal: "Coordinate two workers".into(),
                    limits: Limits {
                        lanes: 4,
                        concurrency: 3,
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
        let service = Arc::new(
            FleetService::new(runtime, allocator, BTreeSet::from(["codex".into()])).unwrap(),
        );
        let lane = service
            .create_root("coordinator", "Coordinate", "codex", &input)
            .unwrap();
        service
            .native_command(
                "dispatch-root",
                Command::Dispatch {
                    lane: lane.clone(),
                    run: "root-run".into(),
                },
            )
            .unwrap();
        let credential = service
            .grant(&lane, "root-run", "actor-root", "session-root")
            .unwrap();
        service
            .native_command(
                "running-root",
                Command::Observe {
                    lane: lane.clone(),
                    run: "root-run".into(),
                    state: RunState::Running,
                },
            )
            .unwrap();
        desktop.register_fleet(service.clone()).unwrap();
        Self {
            path,
            desktop,
            service,
            lane,
            credential,
        }
    }
    fn call(&self, action: &str, args: &Json) -> Result<Json, mesh_daemon::ipc::Unavailable> {
        self.desktop
            .fleet_agent_call("objective", self.credential.transport_value(), action, args)
    }
    fn context(&self) -> Json {
        self.call("context", &Json::empty_object()).unwrap()
    }
    fn delegate(&self, request: &str) -> Json {
        let context = self.context();
        let versions = context
            .get("workspace")
            .unwrap()
            .get("workspace_versions")
            .unwrap()
            .as_array()
            .unwrap();
        Json::object([
            ("request", Json::text(request)),
            ("goal", Json::text(format!("Implement {request}"))),
            ("provider", Json::text("codex")),
            ("version", versions[0].get("operation").unwrap().clone()),
        ])
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
fn text<'a>(value: &'a Json, key: &str) -> &'a str {
    value.get(key).and_then(Json::as_text).unwrap()
}

#[test]
fn agent_creates_two_independent_versions_and_retry_does_not_duplicate() {
    let f = Fixture::new("delegate");
    let desktop_root = f.desktop.workspace_state().unwrap().root;
    let args = f.delegate("worker-a");
    let first = f.call("delegate", &args).unwrap();
    let second = f.call("delegate", &f.delegate("worker-b")).unwrap();
    assert_eq!(f.call("delegate", &args).unwrap(), first);
    assert_ne!(text(&first, "id"), text(&second, "id"));
    let a = PathBuf::from(text(first.get("workspace").unwrap(), "root"));
    let b = PathBuf::from(text(second.get("workspace").unwrap(), "root"));
    fs::write(a.join("note.txt"), "worker a").unwrap();
    assert_eq!(
        fs::read_to_string(b.join("note.txt")).unwrap(),
        "immutable input\n"
    );
    assert_eq!(f.desktop.workspace_state().unwrap().root, desktop_root);
    assert_eq!(
        f.call("children", &Json::empty_object())
            .unwrap()
            .get("lanes")
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(text(&f.context(), "actor"), "actor-root");
    assert!(!format!("{:?}", f.credential).contains(f.credential.transport_value()));
    let replay = Runtime::open(
        FleetStore::open(f.path.join("fleet.sqlite")).unwrap(),
        "objective",
    )
    .unwrap();
    let origin = replay.state().lanes[text(&first, "id")]
        .created_by
        .as_ref()
        .unwrap();
    assert_eq!(origin.actor, "actor-root");
    assert_eq!(origin.session, "session-root");
    assert_eq!(origin.run, "root-run");
    let events = FleetStore::open(f.path.join("fleet.sqlite"))
        .unwrap()
        .events("objective", 0, 256)
        .unwrap();
    assert!(events
        .iter()
        .all(|event| !event.payload.contains(f.credential.transport_value())));
    let ledger = fs::read(f.path.join("fleet.sqlite")).unwrap();
    assert!(!ledger
        .windows(64)
        .any(|w| w == f.credential.transport_value().as_bytes()));
}

#[test]
fn request_conflicts_and_untrusted_paths_or_providers_never_allocate() {
    let f = Fixture::new("denials");
    let args = f.delegate("worker");
    f.call("delegate", &args).unwrap();
    let mut changed = args.clone();
    if let Json::Object(fields) = &mut changed {
        fields.iter_mut().find(|(k, _)| k == "goal").unwrap().1 = Json::text("different");
    }
    assert_eq!(
        f.call("delegate", &changed).unwrap_err().code,
        "fleet-request-conflict"
    );
    let mut path = f.delegate("escape");
    if let Json::Object(fields) = &mut path {
        fields.push(("destination".into(), Json::text("/tmp/arbitrary")));
    }
    assert_eq!(
        f.call("delegate", &path).unwrap_err().code,
        "fleet-arguments-invalid"
    );
    let mut provider = f.delegate("provider");
    if let Json::Object(fields) = &mut provider {
        fields.iter_mut().find(|(k, _)| k == "provider").unwrap().1 = Json::text("unconfigured");
    }
    assert_eq!(
        f.call("delegate", &provider).unwrap_err().code,
        "fleet-provider-not-authorized"
    );
    assert_eq!(
        f.call("approve", &Json::empty_object()).unwrap_err().code,
        "fleet-action-not-authorized"
    );
    assert_eq!(
        f.call("children", &Json::empty_object())
            .unwrap()
            .get("lanes")
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn rotated_revoked_cancelled_and_wrong_objective_sessions_are_refused() {
    let f = Fixture::new("sessions");
    assert!(f
        .desktop
        .fleet_agent_call(
            "other",
            f.credential.transport_value(),
            "context",
            &Json::empty_object()
        )
        .is_err());
    let rotated = f
        .service
        .grant(&f.lane, "root-run", "actor-root", "session-new")
        .unwrap();
    assert_eq!(
        f.call("context", &Json::empty_object()).unwrap_err().code,
        "fleet-session-refused"
    );
    assert!(f
        .service
        .agent_call(rotated.transport_value(), "context", &Json::empty_object())
        .is_ok());
    f.service.revoke(&rotated).unwrap();
    assert!(f
        .service
        .agent_call(rotated.transport_value(), "context", &Json::empty_object())
        .is_err());
    f.service.native_command("cancel", Command::Cancel).unwrap();
    assert!(f
        .service
        .grant(&f.lane, "root-run", "actor-root", "session-after-cancel")
        .is_err());
}

#[test]
fn child_cannot_see_siblings_or_expand_delegation_depth() {
    let f = Fixture::new("child-scope");
    let child = f.call("delegate", &f.delegate("child")).unwrap();
    let child_id = text(&child, "id");
    f.call("delegate", &f.delegate("sibling")).unwrap();
    f.service
        .native_command(
            "child-dispatch",
            Command::Dispatch {
                lane: child_id.into(),
                run: "child-run".into(),
            },
        )
        .unwrap();
    let credential = f
        .service
        .grant(child_id, "child-run", "child-actor", "child-session")
        .unwrap();
    let context = f
        .service
        .agent_call(
            credential.transport_value(),
            "context",
            &Json::empty_object(),
        )
        .unwrap();
    let version = context
        .get("workspace")
        .unwrap()
        .get("workspace_versions")
        .unwrap()
        .as_array()
        .unwrap()[0]
        .get("operation")
        .unwrap()
        .clone();
    let grandchildren = Json::object([
        ("request", Json::text("grandchild")),
        ("goal", Json::text("too deep")),
        ("provider", Json::text("codex")),
        ("version", version),
    ]);
    assert_eq!(
        f.service
            .agent_call(credential.transport_value(), "delegate", &grandchildren)
            .unwrap_err()
            .code,
        "delegation-limit"
    );
    assert!(f
        .service
        .agent_call(
            credential.transport_value(),
            "children",
            &Json::empty_object()
        )
        .unwrap()
        .get("lanes")
        .unwrap()
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn native_custody_rotation_invalidates_the_session_before_delegation() {
    let f = Fixture::new("custody");
    let args = f.delegate("forbidden-child");
    let context = f.context();
    let state = context.get("workspace").unwrap();
    f.desktop
        .reopen_at_start(std::path::Path::new(text(state, "root")))
        .unwrap();
    assert!(f
        .desktop
        .release_workspace_agent_custody(
            text(state, "root"),
            text(state, "digest"),
            text(state, "installation"),
            text(&context, "generation")
        )
        .unwrap());
    let changed = f
        .desktop
        .acquire_workspace_agent_custody(
            text(state, "root"),
            text(state, "digest"),
            text(state, "installation"),
            false,
            None,
        )
        .unwrap();
    assert_ne!(changed, text(&context, "generation"));
    assert_eq!(
        f.call("delegate", &args).unwrap_err().code,
        "fleet-session-custody-changed"
    );
    assert_eq!(
        f.service
            .snapshot()
            .unwrap()
            .get("lanes")
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
