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
        Self::with_goal(name, "Coordinate", true)
    }
    fn with_goal(name: &str, goal: &str, mark_running: bool) -> Self {
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
            .create_root("coordinator", goal, "codex", &input)
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
        if mark_running {
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
        }
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

struct TestCheckpointSigner(ed25519_dalek::SigningKey);
impl mesh_daemon::fleet::service::CheckpointSigner for TestCheckpointSigner {
    fn public_key(&self) -> mesh_types::PublicKey {
        mesh_types::PublicKey::from_bytes(self.0.verifying_key().to_bytes())
    }
    fn sign(&self, payload: &mesh_crypto::SigningPayload) -> Result<mesh_types::Signature, String> {
        use ed25519_dalek::Signer as _;
        Ok(mesh_types::Signature::from_bytes(
            self.0.sign(payload.as_bytes()).to_bytes(),
        ))
    }
}

#[test]
fn authenticated_checkpoint_retry_pins_result_and_new_request_captures_later_work() {
    let mut f = Fixture::new("checkpoint-replay");
    let args = Json::object([("request", Json::text("checkpoint-a"))]);
    assert_eq!(
        f.call("checkpoint", &args).unwrap_err().code,
        "fleet-checkpoint-signer-unavailable"
    );
    f.credential = f
        .service
        .grant_with_signer(
            &f.lane,
            "root-run",
            "signed-session",
            Arc::new(TestCheckpointSigner(ed25519_dalek::SigningKey::from_bytes(
                &[0x61; 32],
            ))),
        )
        .unwrap();
    let context = f.context();
    let root = PathBuf::from(text(context.get("workspace").unwrap(), "root"));
    fs::write(root.join("note.txt"), "checkpoint one\n").unwrap();
    let first = f.call("checkpoint", &args).unwrap();
    assert_eq!(first.get("complete"), Some(&Json::Bool(true)));
    assert_eq!(first.get("saved_changes").and_then(Json::as_u64), Some(1));
    fs::write(root.join("note.txt"), "later working bytes\n").unwrap();
    assert_eq!(f.call("checkpoint", &args).unwrap(), first);
    let second = f
        .call(
            "checkpoint",
            &Json::object([("request", Json::text("checkpoint-b"))]),
        )
        .unwrap();
    assert_ne!(text(&first, "version"), text(&second, "version"));
    let child = f
        .call(
            "delegate",
            &Json::object([
                ("request", Json::text("from-checkpoint")),
                ("goal", Json::text("Continue saved work")),
                ("provider", Json::text("codex")),
                ("version", first.get("version").unwrap().clone()),
            ]),
        )
        .unwrap();
    assert_eq!(
        fs::read_to_string(
            PathBuf::from(text(child.get("workspace").unwrap(), "root")).join("note.txt")
        )
        .unwrap(),
        "checkpoint one\n"
    );
    let replay = Runtime::open(
        FleetStore::open(f.path.join("fleet.sqlite")).unwrap(),
        "objective",
    )
    .unwrap();
    assert_eq!(replay.state().checkpoints.len(), 2);
    let recorded = &replay.state().checkpoints[text(&first, "checkpoint")];
    assert_eq!(recorded.origin.actor, text(&context, "actor"));
    assert_eq!(recorded.origin.session, "signed-session");
    assert!(recorded.result.as_ref().unwrap().complete);
    assert_eq!(
        replay.state().lanes[&f.lane].saved.unwrap().to_string(),
        text(&second, "version")
    );
}

#[test]
fn checkpoint_identity_cannot_be_reused_by_a_rotated_session() {
    let mut f = Fixture::new("checkpoint-session-conflict");
    let signer = Arc::new(TestCheckpointSigner(ed25519_dalek::SigningKey::from_bytes(
        &[0x62; 32],
    )));
    f.credential = f
        .service
        .grant_with_signer(&f.lane, "root-run", "session-one", signer.clone())
        .unwrap();
    let args = Json::object([("request", Json::text("stable"))]);
    let first = f.call("checkpoint", &args).unwrap();
    assert_eq!(first.get("complete"), Some(&Json::Bool(true)));
    f.credential = f
        .service
        .grant_with_signer(&f.lane, "root-run", "session-two", signer)
        .unwrap();
    assert_eq!(
        f.call("checkpoint", &args).unwrap_err().code,
        "fleet-checkpoint-request-conflict"
    );
    assert_eq!(
        f.call(
            "submit_review",
            &Json::object([("checkpoint", first.get("checkpoint").unwrap().clone())])
        )
        .unwrap_err()
        .code,
        "fleet-checkpoint-not-in-session"
    );
}

#[test]
fn incomplete_checkpoint_cannot_be_submitted_for_review() {
    let mut f = Fixture::new("checkpoint-incomplete-review");
    f.credential = f
        .service
        .grant_with_signer(
            &f.lane,
            "root-run",
            "signed-session",
            Arc::new(TestCheckpointSigner(ed25519_dalek::SigningKey::from_bytes(
                &[0x67; 32],
            ))),
        )
        .unwrap();
    let context = f.context();
    let root = PathBuf::from(text(context.get("workspace").unwrap(), "root"));
    fs::rename(root.join("note.txt"), root.join("renamed.txt")).unwrap();
    let captured = f
        .call(
            "checkpoint",
            &Json::object([("request", Json::text("incomplete"))]),
        )
        .unwrap();
    assert_eq!(captured.get("complete"), Some(&Json::Bool(false)));
    assert_eq!(
        f.call(
            "submit_review",
            &Json::object([("checkpoint", captured.get("checkpoint").unwrap().clone())])
        )
        .unwrap_err()
        .code,
        "fleet-checkpoint-incomplete"
    );
    assert_eq!(
        f.context()
            .get("workspace")
            .unwrap()
            .get("reviews")
            .and_then(Json::as_u64),
        Some(0)
    );
}

#[test]
fn codex_adapter_uses_native_lane_and_never_relaunches_a_claimed_run() {
    use mesh_daemon::fleet::provider::CodexAdapter;
    let f = Fixture::new("provider-launch");
    let child = f.call("delegate", &f.delegate("provider-child")).unwrap();
    let lane = text(&child, "id");
    let root = PathBuf::from(text(child.get("workspace").unwrap(), "root"));
    f.service
        .native_command(
            "dispatch-provider",
            Command::Dispatch {
                lane: lane.into(),
                run: "provider-run".into(),
            },
        )
        .unwrap();
    let credential = f
        .service
        .grant(lane, "provider-run", "actor-worker", "session-worker")
        .unwrap();
    let executable = f.path.join("fake-codex");
    fs::write(
        &executable,
        r#"#!/bin/sh
case "$*" in *"$MESH_FLEET_CREDENTIAL"*) exit 17;; esac
pwd > provider-working-root.txt
cat > provider-prompt.txt
printf 'one\n' >> provider-launch-count.txt
printf '%s\n' '{"type":"thread.started","thread_id":"01234567-0123-0123-0123-0123456789ab"}'
printf '{"type":"item.completed","item":{"text":"%s"}}\n' "$MESH_FLEET_CREDENTIAL"
printf '%s\n' '{"type":"turn.completed"}'
"#,
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let adapter = CodexAdapter::new(&executable, &executable).unwrap();
    let mut process = f
        .service
        .start_codex(&credential, &adapter, &f.path.join("daemon.sock"))
        .unwrap();
    assert_eq!(
        f.service
            .start_codex(&credential, &adapter, &f.path.join("daemon.sock"))
            .unwrap_err()
            .code,
        "launch-needs-reconciliation"
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let (observed, outcome) = process.poll().unwrap();
        assert!(!format!("{observed:?}").contains(credential.transport_value()));
        if let Some(success) = outcome {
            assert!(success, "{observed:?}");
            assert_eq!(observed.events, 3);
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "provider did not exit"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        fs::read_to_string(root.join("provider-working-root.txt"))
            .unwrap()
            .trim(),
        root.canonicalize().unwrap().to_str().unwrap()
    );
    assert_eq!(
        fs::read_to_string(root.join("provider-launch-count.txt")).unwrap(),
        "one\n"
    );
    let prompt = fs::read_to_string(root.join("provider-prompt.txt")).unwrap();
    assert!(prompt.contains("Implement provider-child"));
    assert!(!prompt.contains(credential.transport_value()));
    let replay = Runtime::open(
        FleetStore::open(f.path.join("fleet.sqlite")).unwrap(),
        "objective",
    )
    .unwrap();
    assert!(replay.state().lanes[lane]
        .runs
        .last()
        .unwrap()
        .launch_owner
        .is_some());
}

/// Explicit opt-in: uses the installed provider's existing account and retains disposable evidence.
#[test]
#[ignore = "requires MESH_TEST_CODEX and MESH_TEST_MCP absolute executables and provider login"]
fn actual_codex_edits_checkpoints_and_submits_a_private_review() {
    use mesh_daemon::fleet::provider::CodexAdapter;
    use mesh_daemon::ipc::IpcServer;
    let adapter = CodexAdapter::new(
        &PathBuf::from(std::env::var_os("MESH_TEST_CODEX").expect("MESH_TEST_CODEX")),
        &PathBuf::from(std::env::var_os("MESH_TEST_MCP").expect("MESH_TEST_MCP")),
    )
    .unwrap();
    // Preserve the workspace even on timeout/panic: process-tree termination is not yet proven.
    let f = std::mem::ManuallyDrop::new(Fixture::new("actual-codex"));
    let selected_before = f.desktop.workspace_state().unwrap();
    let versions = f.context();
    let version = versions
        .get("workspace")
        .unwrap()
        .get("workspace_versions")
        .unwrap()
        .as_array()
        .unwrap()[0]
        .get("operation")
        .unwrap()
        .clone();
    let child = f.call("delegate", &Json::object([
        ("request", Json::text("actual-worker")),
        ("provider", Json::text("codex")),
        ("version", version),
        ("goal", Json::text("This is a disposable integration test. Replace note.txt with exactly 'actual Codex saved result' followed by a newline. Do not create other files or delegate work. Call mesh_fleet_checkpoint with request actual-result and then mesh_fleet_submit_review with the returned checkpoint identifier. Finish only after successful review submission. Do not approve or publish.")),
    ])).unwrap();
    let lane = text(&child, "id");
    let root = PathBuf::from(text(child.get("workspace").unwrap(), "root"));
    f.service
        .native_command(
            "dispatch-actual",
            Command::Dispatch {
                lane: lane.into(),
                run: "actual-run".into(),
            },
        )
        .unwrap();
    let credential = f
        .service
        .grant_with_signer(
            lane,
            "actual-run",
            "actual-session",
            Arc::new(TestCheckpointSigner(ed25519_dalek::SigningKey::from_bytes(
                &[0x74; 32],
            ))),
        )
        .unwrap();
    let socket_dir = PathBuf::from(format!(
        "/private/tmp/mesh-codex-ipc-{}",
        std::process::id()
    ));
    fs::create_dir(&socket_dir).unwrap();
    fs::set_permissions(&socket_dir, fs::Permissions::from_mode(0o700)).unwrap();
    let endpoint = socket_dir.join("daemon.sock");
    let server = IpcServer::bind(&endpoint)
        .unwrap()
        .spawn(f.desktop.clone())
        .unwrap();
    let mut process = f
        .service
        .start_codex(&credential, &adapter, &endpoint)
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(240);
    loop {
        let (observation, outcome) = process.poll().unwrap();
        if let Some(success) = outcome {
            assert!(
                success,
                "provider execution failed: {observation:?}; evidence: {}",
                f.path.display()
            );
            break;
        }
        if std::time::Instant::now() >= deadline {
            let _ = process.request_stop();
            panic!(
                "provider deadline exceeded; preserved evidence: {}",
                f.path.display()
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let runtime = Runtime::open(
        FleetStore::open(f.path.join("fleet.sqlite")).unwrap(),
        "objective",
    )
    .unwrap();
    let checkpoint = runtime
        .state()
        .checkpoints
        .values()
        .find(|c| c.lane == lane)
        .expect("agent saved checkpoint");
    assert!(checkpoint.result.as_ref().unwrap().complete);
    assert!(
        checkpoint.review.is_some(),
        "agent submitted immutable review"
    );
    assert_eq!(
        fs::read_to_string(root.join("note.txt")).unwrap(),
        "actual Codex saved result\n"
    );
    let selected_after = f.desktop.workspace_state().unwrap();
    assert_eq!(
        selected_before, selected_after,
        "provider must not change selected source or its shared state"
    );
    f.service.revoke(&credential).unwrap();
    server.shutdown();
    eprintln!("actual provider evidence retained at {}", f.path.display());
}

struct TestWorkerSigners;
impl mesh_daemon::fleet::host::WorkerSignerFactory for TestWorkerSigners {
    fn signer(
        &self,
        _lane: &str,
        _run: &str,
    ) -> Result<Arc<dyn mesh_daemon::fleet::service::CheckpointSigner>, mesh_daemon::ipc::Unavailable>
    {
        Ok(Arc::new(TestCheckpointSigner(
            ed25519_dalek::SigningKey::from_bytes(&[0x75; 32]),
        )))
    }
}

#[test]
fn native_host_schedules_children_within_limits_and_preserves_cancelled_slots() {
    use mesh_daemon::fleet::host::CodexFleetHost;
    use mesh_daemon::fleet::provider::CodexAdapter;
    let f = Fixture::new("host-scheduling");
    let children: Vec<_> = ["one", "two", "three"]
        .iter()
        .map(|id| f.call("delegate", &f.delegate(id)).unwrap())
        .collect();
    let executable = f.path.join("worker");
    fs::write(&executable, "#!/bin/sh\ncat >/dev/null\necho one >> launches\nwhile [ ! -f release ]; do sleep 0.01; done\nprintf '%s\\n' '{\"type\":\"turn.completed\"}'\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let adapter = CodexAdapter::new(&executable, &executable).unwrap();
    let mut host = CodexFleetHost::new(
        f.service.clone(),
        adapter.clone(),
        f.path.join("ipc.sock"),
        Arc::new(TestWorkerSigners),
    )
    .unwrap();
    host.tick().unwrap();
    let state = f.service.native_state().unwrap();
    assert_eq!(
        state.lanes.values().filter(|l| !l.runs.is_empty()).count(),
        3,
        "root and two child workers fill the budget"
    );
    let queued = state
        .lanes
        .values()
        .find(|l| l.runs.is_empty())
        .unwrap()
        .id
        .clone();
    let running = state
        .lanes
        .values()
        .find(|l| l.id != f.lane && !l.runs.is_empty())
        .unwrap()
        .id
        .clone();
    // A second host cannot adopt or relaunch the two already dispatched processes.
    let mut other = CodexFleetHost::new(
        f.service.clone(),
        adapter,
        f.path.join("ipc.sock"),
        Arc::new(TestWorkerSigners),
    )
    .unwrap();
    assert!(other.tick().unwrap().is_empty());
    let root_for = |id: &str| {
        PathBuf::from(text(
            children
                .iter()
                .find(|c| text(c, "id") == id)
                .unwrap()
                .get("workspace")
                .unwrap(),
            "root",
        ))
    };
    fs::write(root_for(&running).join("release"), "release").unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        host.tick().unwrap();
        if !f.service.native_state().unwrap().lanes[&queued]
            .runs
            .is_empty()
        {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        f.service.native_state().unwrap().lanes[&running].runs[0].state,
        RunState::Succeeded
    );
    assert_eq!(
        fs::read_to_string(root_for(&running).join("launches")).unwrap(),
        "one\n"
    );
    f.service
        .native_command("cancel-host", Command::Cancel)
        .unwrap();
    for _ in 0..20 {
        host.tick().unwrap();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        f.service.native_state().unwrap().lanes[&queued].runs[0].state,
        RunState::Stopping,
        "direct child exit cannot release a cancelled process-tree slot"
    );
    assert_eq!(
        f.service.native_state().unwrap().lanes[&queued].runs.len(),
        1
    );
}

#[test]
#[ignore = "requires installed provider login and MESH_TEST_CODEX/MESH_TEST_MCP"]
fn actual_coordinator_delegates_two_workers_and_host_saves_both_reviews() {
    use mesh_daemon::fleet::host::CodexFleetHost;
    use mesh_daemon::fleet::provider::CodexAdapter;
    use mesh_daemon::ipc::IpcServer;
    let adapter = CodexAdapter::new(
        &PathBuf::from(std::env::var_os("MESH_TEST_CODEX").expect("MESH_TEST_CODEX")),
        &PathBuf::from(std::env::var_os("MESH_TEST_MCP").expect("MESH_TEST_MCP")),
    )
    .unwrap();
    let f = std::mem::ManuallyDrop::new(Fixture::with_goal("actual-fleet", "Call mesh_fleet_context. Use mesh_fleet_delegate to create exactly two child lanes with provider codex, request worker-one and worker-two, and the saved operation version from your context. For each child, set its goal to: replace note.txt with the exact text 'worker-one' or 'worker-two' respectively followed by newline, save with mesh_fleet_checkpoint, submit the returned checkpoint with mesh_fleet_submit_review, and do not delegate further. Do not edit your own workspace. After both delegations succeed, finish your task. The native host will run the children.", false));
    let before = f.desktop.workspace_state().unwrap();
    let credential = f
        .service
        .grant_with_signer(
            &f.lane,
            "root-run",
            "coordinator-session",
            Arc::new(TestCheckpointSigner(ed25519_dalek::SigningKey::from_bytes(
                &[0x76; 32],
            ))),
        )
        .unwrap();
    let socket_dir = PathBuf::from(format!(
        "/private/tmp/mesh-fleet-ipc-{}",
        std::process::id()
    ));
    fs::create_dir(&socket_dir).unwrap();
    fs::set_permissions(&socket_dir, fs::Permissions::from_mode(0o700)).unwrap();
    let endpoint = socket_dir.join("daemon.sock");
    let server = IpcServer::bind(&endpoint)
        .unwrap()
        .spawn(f.desktop.clone())
        .unwrap();
    let mut coordinator = f
        .service
        .start_codex(&credential, &adapter, &endpoint)
        .unwrap();
    let mut host = CodexFleetHost::new(
        f.service.clone(),
        adapter,
        endpoint,
        Arc::new(TestWorkerSigners),
    )
    .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(240);
    let mut coordinator_done = false;
    loop {
        host.tick().unwrap();
        if !coordinator_done {
            if let Some(success) = coordinator.poll().unwrap().1 {
                assert!(success, "coordinator failed");
                f.service.revoke(&credential).unwrap();
                f.service
                    .native_command(
                        "coordinator-complete",
                        Command::Observe {
                            lane: f.lane.clone(),
                            run: "root-run".into(),
                            state: RunState::Succeeded,
                        },
                    )
                    .unwrap();
                coordinator_done = true;
            }
        }
        let state = f.service.native_state().unwrap();
        if coordinator_done
            && state.lanes.len() == 3
            && state.lanes.values().all(|l| {
                l.runs
                    .last()
                    .is_some_and(|r| r.state == RunState::Succeeded)
            })
        {
            break;
        }
        if std::time::Instant::now() >= deadline {
            f.service
                .native_command("proof-timeout", Command::Cancel)
                .unwrap();
            let _ = coordinator.request_stop();
            let _ = host.tick();
            panic!(
                "fleet timed out; evidence preserved at {}",
                f.path.display()
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let state = f.service.native_state().unwrap();
    let mut results = Vec::new();
    for lane in state.lanes.values().filter(|l| l.id != f.lane) {
        assert_eq!(lane.parent.as_deref(), Some(f.lane.as_str()));
        assert_eq!(lane.runs.len(), 1);
        let checkpoint = state
            .checkpoints
            .values()
            .find(|c| {
                c.lane == lane.id
                    && c.review.is_some()
                    && c.result.as_ref().is_some_and(|r| r.complete)
            })
            .unwrap();
        assert_saved_worker_review(lane, checkpoint);
        results.push(
            fs::read_to_string(
                PathBuf::from(lane.workspace.as_ref().unwrap().root()).join("note.txt"),
            )
            .unwrap(),
        );
    }
    results.sort();
    assert_eq!(results, ["worker-one\n", "worker-two\n"]);
    assert_eq!(f.desktop.workspace_state().unwrap(), before);
    server.shutdown();
    eprintln!("actual fleet evidence retained at {}", f.path.display());
}

fn assert_saved_worker_review(
    lane: &mesh_daemon::fleet::Lane,
    checkpoint: &mesh_daemon::fleet::Checkpoint,
) {
    let reader = LiveDaemon::new(StartupSummary::from(&nothing_to_recover()));
    let saved = reader
        .reopen_at_start(&PathBuf::from(lane.workspace.as_ref().unwrap().root()))
        .unwrap();
    let bundle = checkpoint.review.unwrap().to_string();
    let version = checkpoint.result.as_ref().unwrap().version.to_string();
    let review = saved
        .review_items
        .iter()
        .find(|item| item.get("bundle").and_then(Json::as_text) == Some(bundle.as_str()))
        .unwrap();
    let change = review
        .get("bundle_changes")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .find(|change| {
            change
                .get("path_after")
                .and_then(Json::as_text)
                .is_some_and(|path| path == "/note.txt")
        })
        .expect("exact /note.txt review entry");
    let artifact = reader
        .review_artifact_for_workspace(
            &saved.root,
            &saved.digest,
            &saved.installation,
            &bundle,
            &version,
            text(change, "object_id"),
            "after",
        )
        .unwrap();
    let working =
        fs::read(PathBuf::from(lane.workspace.as_ref().unwrap().root()).join("note.txt")).unwrap();
    assert_eq!(
        artifact.bytes(),
        working,
        "the immutable review must contain the worker's actual result"
    );
}

#[test]
#[ignore = "requires MESH_TEST_REVIEW_EVIDENCE pointing to a retained disposable fleet proof"]
fn retained_actual_fleet_reviews_reconstruct() {
    let path = PathBuf::from(
        std::env::var_os("MESH_TEST_REVIEW_EVIDENCE").expect("MESH_TEST_REVIEW_EVIDENCE"),
    );
    let runtime = Runtime::open(
        FleetStore::open(path.join("fleet.sqlite")).unwrap(),
        "objective",
    )
    .unwrap();
    let state = runtime.state();
    let children: Vec<_> = state
        .lanes
        .values()
        .filter(|lane| lane.parent.is_some())
        .collect();
    assert_eq!(children.len(), 2);
    let mut contents = Vec::new();
    for lane in children {
        let checkpoint = state
            .checkpoints
            .values()
            .find(|c| c.lane == lane.id && c.review.is_some())
            .unwrap();
        assert_saved_worker_review(lane, checkpoint);
        contents.push(
            fs::read_to_string(
                PathBuf::from(lane.workspace.as_ref().unwrap().root()).join("note.txt"),
            )
            .unwrap(),
        );
    }
    contents.sort();
    assert_eq!(contents, ["worker-one\n", "worker-two\n"]);
}
