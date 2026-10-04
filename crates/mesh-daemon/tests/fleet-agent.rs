//! Agent authority and delegation through actual native lane allocation.
#![cfg(target_os = "macos")]
use mesh_daemon::fleet::service::{AgentCredential, FleetService, NativeLaneAllocator};
use mesh_daemon::fleet::workspace::VersionInput;
use mesh_daemon::fleet::{Command, Limits, RunState, Runtime};
use mesh_daemon::ipc::{nothing_to_recover, Json, Operations, StartupSummary};
use mesh_daemon::{CheckpointRuntimeParameters, LiveDaemon, TrustedReviewers};
use mesh_store::fleet::FleetStore;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
#[path = "support/fleet_measurements.rs"]
mod fleet_measurements;

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
        Self::with_providers(name, goal, mark_running, BTreeSet::from(["codex".into()]))
    }
    fn with_providers(
        name: &str,
        goal: &str,
        mark_running: bool,
        providers: BTreeSet<String>,
    ) -> Self {
        Self::with_limits(
            name,
            goal,
            mark_running,
            providers,
            Limits {
                lanes: 4,
                concurrency: 3,
                depth: 1,
                retries: 1,
            },
        )
    }
    fn with_limits(
        name: &str,
        goal: &str,
        mark_running: bool,
        providers: BTreeSet<String>,
        limits: Limits,
    ) -> Self {
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
                    goal: goal.into(),
                    limits,
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
        let service = Arc::new(FleetService::new(runtime, allocator, providers).unwrap());
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
    codex_adapter_journey(false);
}

#[test]
fn codex_adapter_selects_the_explicit_packaged_fleet_bridge_mode() {
    codex_adapter_journey(true);
}

#[test]
fn claude_adapter_uses_the_same_native_claim_and_custody_boundary() {
    provider_adapter_journey(false, true, false);
}
#[test]
fn claude_adapter_selects_the_packaged_mesh_bridge() {
    provider_adapter_journey(true, true, false);
}
#[test]
fn claude_error_result_cannot_succeed_even_when_the_process_exits_zero() {
    provider_adapter_journey(false, true, true);
}
fn codex_adapter_journey(packaged: bool) {
    provider_adapter_journey(packaged, false, false);
}
fn provider_adapter_journey(packaged: bool, claude: bool, failed: bool) {
    use mesh_daemon::fleet::provider::{ClaudeAdapter, CodexAdapter, NativeAdapter};
    let name = format!("provider-{packaged}-{claude}-{failed}");
    let f = Fixture::with_providers(
        &name,
        "Coordinate",
        true,
        BTreeSet::from(["codex".into(), "claude".into()]),
    );
    let mut delegation = f.delegate("provider-child");
    if claude {
        let Json::Object(fields) = &mut delegation else {
            unreachable!()
        };
        fields
            .iter_mut()
            .find(|(key, _)| key == "provider")
            .unwrap()
            .1 = Json::text("claude");
    }
    let child = f.call("delegate", &delegation).unwrap();
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
    let executable = f.path.join("fake-provider");
    let common = r#"#!/bin/sh
case "$*" in *"$MESH_FLEET_CREDENTIAL"*) exit 17;; esac
printf '%s\n' "$@" > provider-args.txt
pwd > provider-working-root.txt
cat > provider-prompt.txt
printf 'one\n' >> provider-launch-count.txt
"#;
    let output = if !claude {
        r#"printf '%s\n' '{"type":"thread.started","thread_id":"01234567-0123-0123-0123-0123456789ab"}'
printf '{"type":"item.completed","item":{"text":"%s"}}\n' "$MESH_FLEET_CREDENTIAL"
printf '%s\n' '{"type":"turn.completed"}'
"#
    } else if failed {
        r#"printf '%s\n' '{"type":"system","subtype":"init","session_id":"01234567-0123-0123-0123-0123456789ab"}'
printf '{"type":"assistant","error":"authentication_failed","message":"%s"}\n' "$MESH_FLEET_CREDENTIAL"
printf '%s\n' '{"type":"result","subtype":"success","is_error":true,"total_cost_usd":0.002}'
"#
    } else {
        r#"printf '%s\n' '{"type":"system","subtype":"init","session_id":"01234567-0123-0123-0123-0123456789ab"}'
printf '{"type":"assistant","message":"%s"}\n' "$MESH_FLEET_CREDENTIAL"
printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"total_cost_usd":0.002}'
"#
    };
    fs::write(&executable, format!("{common}{output}")).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let codex = if packaged {
        CodexAdapter::with_desktop_bridge(&executable, &executable)
    } else {
        CodexAdapter::new(&executable, &executable)
    }
    .unwrap();
    let adapter: NativeAdapter = if claude {
        if packaged {
            ClaudeAdapter::with_desktop_bridge(&executable, &executable)
        } else {
            ClaudeAdapter::new(&executable, &executable)
        }
        .unwrap()
        .into()
    } else {
        codex.clone().into()
    };
    let endpoint = f.path.join("daemon.sock");
    if claude {
        assert_eq!(
            f.service
                .start_codex(&credential, &codex, &endpoint)
                .unwrap_err()
                .code,
            "fleet-provider-mismatch"
        );
        assert!(f.service.native_state().unwrap().lanes[lane]
            .runs
            .last()
            .unwrap()
            .launch_owner
            .is_none());
    }
    let launch = || match &adapter {
        NativeAdapter::Claude(adapter) => f.service.start_claude(&credential, adapter, &endpoint),
        NativeAdapter::Codex(adapter) => f.service.start_codex(&credential, adapter, &endpoint),
    };
    let mut process = launch().unwrap();
    assert_eq!(launch().unwrap_err().code, "launch-needs-reconciliation");
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let (observed, outcome) = process.poll().unwrap();
        assert!(!format!("{observed:?}").contains(credential.transport_value()));
        if let Some(success) = outcome {
            assert_eq!(success, !failed, "{observed:?}");
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
    let arguments = fs::read_to_string(root.join("provider-args.txt")).unwrap();
    assert_eq!(arguments.contains("--mesh-fleet-mcp"), packaged);
    assert!(!arguments.contains(credential.transport_value()));
    if claude {
        let args: Vec<_> = arguments.lines().collect();
        let value = |flag| args[args.iter().position(|arg| *arg == flag).unwrap() + 1];
        assert_eq!(value("--setting-sources"), "");
        assert_eq!(value("--permission-mode"), "dontAsk");
        assert_eq!(value("--tools"), "Bash");
        assert_eq!(value("--allowedTools"), "mcp__mesh__*");
        for flag in [
            "--strict-mcp-config",
            "--disable-slash-commands",
            "--no-session-persistence",
        ] {
            assert!(args.contains(&flag));
        }
        for flag in [
            "--bare",
            "--safe-mode",
            "--dangerously-skip-permissions",
            "--allow-dangerously-skip-permissions",
        ] {
            assert!(!args.contains(&flag));
        }
        let settings = Json::parse(value("--settings")).unwrap();
        assert_eq!(
            settings.get("disableAllHooks").and_then(Json::as_bool),
            Some(true)
        );
        let sandbox = settings.get("sandbox").unwrap();
        for flag in ["enabled", "failIfUnavailable", "autoAllowBashIfSandboxed"] {
            assert_eq!(sandbox.get(flag).and_then(Json::as_bool), Some(true));
        }
        assert_eq!(
            sandbox
                .get("allowUnsandboxedCommands")
                .and_then(Json::as_bool),
            Some(false)
        );
        assert!(sandbox
            .get("excludedCommands")
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty());
        let config = Json::parse(value("--mcp-config")).unwrap();
        let server = config.get("mcpServers").unwrap().get("mesh").unwrap();
        assert_eq!(
            text(server, "command"),
            executable.canonicalize().unwrap().to_str().unwrap()
        );
        assert_eq!(
            text(server.get("env").unwrap(), "MESH_FLEET_CREDENTIAL"),
            "${MESH_FLEET_CREDENTIAL}"
        );
    }
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
    actual_provider_journey(false);
}

/// Explicit opt-in; authentication and sandbox prerequisites must be available on this host.
#[test]
#[ignore = "requires MESH_TEST_CLAUDE and MESH_TEST_MCP absolute executables and provider login"]
fn actual_claude_edits_checkpoints_and_submits_a_private_review() {
    actual_provider_journey(true);
}
fn actual_provider_journey(claude: bool) {
    use mesh_daemon::fleet::provider::{ClaudeAdapter, CodexAdapter, NativeAdapter};
    use mesh_daemon::ipc::IpcServer;
    let provider = if claude { "claude" } else { "codex" };
    let variable = if claude {
        "MESH_TEST_CLAUDE"
    } else {
        "MESH_TEST_CODEX"
    };
    let executable = PathBuf::from(std::env::var_os(variable).expect("provider executable"));
    let bridge = PathBuf::from(std::env::var_os("MESH_TEST_MCP").expect("MESH_TEST_MCP"));
    let adapter: NativeAdapter = if claude {
        ClaudeAdapter::new(&executable, &bridge).unwrap().into()
    } else {
        CodexAdapter::new(&executable, &bridge).unwrap().into()
    };
    // Preserve evidence even after failures: direct process stop does not prove descendant exit.
    let f = std::mem::ManuallyDrop::new(Fixture::with_providers(
        &format!("actual-{provider}"),
        "Coordinate",
        true,
        BTreeSet::from(["codex".into(), "claude".into()]),
    ));
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
        ("provider", Json::text(provider)),
        ("version", version),
        ("goal", Json::text("This is a disposable integration test. Replace note.txt with exactly 'actual provider saved result' followed by a newline. Do not create other files or delegate work. Call mesh_fleet_checkpoint with request actual-result and then mesh_fleet_submit_review with the returned checkpoint identifier. Finish only after successful review submission. Do not approve or publish.")),
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
        .start_provider(&credential, &adapter, &endpoint)
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
        "actual provider saved result\n"
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
fn native_hosts_select_their_provider_and_share_the_objective_budget() {
    use mesh_daemon::fleet::host::NativeFleetHost;
    use mesh_daemon::fleet::provider::{ClaudeAdapter, CodexAdapter};
    let f = Fixture::with_providers(
        "mixed-provider-hosts",
        "Coordinate",
        true,
        BTreeSet::from(["codex".into(), "claude".into()]),
    );
    let codex = f.call("delegate", &f.delegate("codex-child")).unwrap();
    let mut args = f.delegate("claude-child");
    let Json::Object(fields) = &mut args else {
        unreachable!()
    };
    fields
        .iter_mut()
        .find(|(key, _)| key == "provider")
        .unwrap()
        .1 = Json::text("claude");
    let claude = f.call("delegate", &args).unwrap();
    let executable = f.path.join("mixed-worker");
    fs::write(
        &executable,
        r#"#!/bin/sh
cat >/dev/null
case "$1" in
exec) printf '%s\n' '{"type":"turn.completed"}';;
--print) printf '%s\n' '{"type":"result","subtype":"success","is_error":false}';;
*) exit 17;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let mut claude_host = NativeFleetHost::new(
        f.service.clone(),
        ClaudeAdapter::new(&executable, &executable).unwrap(),
        f.path.join("ipc.sock"),
        Arc::new(TestWorkerSigners),
    )
    .unwrap();
    claude_host.tick().unwrap();
    let state = f.service.native_state().unwrap();
    assert!(state.lanes[text(&codex, "id")].runs.is_empty());
    assert_eq!(state.lanes[text(&claude, "id")].runs.len(), 1);
    let mut codex_host = NativeFleetHost::new(
        f.service.clone(),
        CodexAdapter::new(&executable, &executable).unwrap(),
        f.path.join("ipc.sock"),
        Arc::new(TestWorkerSigners),
    )
    .unwrap();
    codex_host.tick().unwrap();
    let state = f.service.native_state().unwrap();
    assert_eq!(
        state
            .lanes
            .values()
            .flat_map(|lane| &lane.runs)
            .filter(|run| run.state.occupies_slot())
            .count(),
        3
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        claude_host.poll_owned().unwrap();
        codex_host.poll_owned().unwrap();
        let state = f.service.native_state().unwrap();
        if [&claude, &codex]
            .iter()
            .all(|child| state.lanes[text(child, "id")].runs[0].state == RunState::Succeeded)
        {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
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
        host.poll_owned().unwrap();
        if f.service.native_state().unwrap().lanes[&running].runs[0].state == RunState::Succeeded {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        f.service.native_state().unwrap().lanes[&queued]
            .runs
            .is_empty(),
        "observation-only ticks cannot dispatch queued work even when a slot opens"
    );
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
    actual_fleet_measurement("actual-fleet", &["worker-one", "worker-two"], 3);
}

#[test]
#[ignore = "requires paid provider execution, installed login and MESH_TEST_CODEX/MESH_TEST_MCP"]
fn actual_four_workers_compare_serial_and_parallel_saved_reviews() {
    let workers = ["worker-one", "worker-two", "worker-three", "worker-four"];
    // Both modes perform the same immutable-input tasks. This measures native orchestration,
    // not human review, renderer event lag or an externally operated harness baseline.
    let serial = actual_fleet_measurement("four-serial", &workers, 1);
    let parallel = actual_fleet_measurement("four-parallel", &workers, 5);
    let report = Json::object([
        ("schema", Json::text("mesh.fleet-four-worker-comparison/v1")),
        ("serial", serial),
        ("parallel", parallel),
        (
            "acceptance",
            Json::text("private saved reviews only; no main approval"),
        ),
    ]);
    let path = std::env::temp_dir().join(format!(
        "mesh-four-worker-comparison-{}.json",
        std::process::id()
    ));
    fs::write(&path, report.encode()).unwrap();
    eprintln!("four-worker comparison retained at {}", path.display());
}

fn actual_fleet_measurement(name: &str, workers: &[&str], concurrency: u64) -> Json {
    use mesh_daemon::fleet::host::CodexFleetHost;
    use mesh_daemon::fleet::provider::CodexAdapter;
    use mesh_daemon::ipc::IpcServer;
    let adapter = CodexAdapter::new(
        &PathBuf::from(std::env::var_os("MESH_TEST_CODEX").expect("MESH_TEST_CODEX")),
        &PathBuf::from(std::env::var_os("MESH_TEST_MCP").expect("MESH_TEST_MCP")),
    )
    .unwrap();
    let provider_version = std::process::Command::new(std::env::var_os("MESH_TEST_CODEX").unwrap())
        .arg("--version")
        .output()
        .unwrap();
    assert!(provider_version.status.success());
    let provider_version = String::from_utf8(provider_version.stdout).unwrap();
    assert!(provider_version.len() <= 256 && !provider_version.trim().is_empty());
    let goal = format!("Call mesh_fleet_context. Use mesh_fleet_delegate to create exactly {} child lanes with provider codex, distinct request identifiers {}, and the saved operation version from your context. For each child, spell out its exact request identifier in the child goal and instruct it to replace note.txt with exactly that literal identifier followed by newline, save with mesh_fleet_checkpoint, submit the returned checkpoint with mesh_fleet_submit_review, and do not delegate further. Do not edit your own workspace. After all delegations succeed, finish your task. The native host will run the children.", workers.len(), workers.join(", "));
    let started = std::time::Instant::now();
    let f = std::mem::ManuallyDrop::new(Fixture::with_limits(
        name,
        &goal,
        false,
        BTreeSet::from(["codex".into()]),
        Limits {
            lanes: (workers.len() + 1) as u64,
            concurrency,
            depth: 1,
            retries: 0,
        },
    ));
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
        "/private/tmp/mesh-fleet-{name}-{}",
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
    let mut ticks = Vec::new();
    let mut reads = Vec::new();
    let mut first_events = BTreeMap::new();
    let mut peak_workers = 0;
    loop {
        let tick_started = std::time::Instant::now();
        let observations = host.tick().unwrap();
        ticks.push(tick_started.elapsed().as_micros() as u64);
        // Saved run state alone does not prove live provider overlap. Count only
        // acknowledged provider sessions with no observed completion or failure.
        let observed_active = observations
            .iter()
            .filter(|observation| {
                observation.outcome.is_none()
                    && observation.activity.thread.is_some()
                    && !observation.activity.turn_completed
                    && !observation.activity.failed
                    && !observation.activity.streams_closed
            })
            .count();
        peak_workers = peak_workers.max(observed_active);
        for observation in observations {
            if observation.activity.events > 0 {
                first_events
                    .entry(observation.lane)
                    .or_insert(started.elapsed().as_millis() as u64);
            }
        }
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
        let read_started = std::time::Instant::now();
        let state = f.service.native_state().unwrap();
        reads.push(read_started.elapsed().as_micros() as u64);
        let active = state
            .lanes
            .values()
            .filter(|lane| {
                lane.id != f.lane
                    && lane.runs.last().is_some_and(|run| {
                        matches!(
                            run.state,
                            RunState::Launching | RunState::Running | RunState::Waiting
                        )
                    })
            })
            .count();
        assert!(active <= concurrency as usize);
        if coordinator_done
            && state.lanes.len() == workers.len() + 1
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
    let execution_ms = started.elapsed().as_millis() as u64;
    let state = f.service.native_state().unwrap();
    let review_started = std::time::Instant::now();
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
    let mut expected: Vec<_> = workers.iter().map(|name| format!("{name}\n")).collect();
    expected.sort();
    assert_eq!(results, expected);
    assert_eq!(f.desktop.workspace_state().unwrap(), before);
    assert_eq!(
        fs::read_to_string(f.path.join("original/note.txt")).unwrap(),
        "immutable input\n"
    );
    let review_inspection_us = review_started.elapsed().as_micros() as u64;
    server.shutdown();
    let report = Json::object([
        ("schema", Json::text("mesh.fleet-native-measurement/v1")),
        ("provider_version", Json::text(provider_version.trim())),
        ("workers", Json::Number(workers.len() as u64)),
        ("concurrency_limit", Json::Number(concurrency)),
        ("peak_workers_observed", Json::Number(peak_workers as u64)),
        ("four_workers_observed", Json::Bool(peak_workers >= 4)),
        ("execution_ms", Json::Number(execution_ms)),
        ("tick_samples", Json::Number(ticks.len() as u64)),
        (
            "tick_p95_us",
            Json::Number(fleet_measurements::p95(&ticks).unwrap()),
        ),
        (
            "state_read_p95_us",
            Json::Number(fleet_measurements::p95(&reads).unwrap()),
        ),
        (
            "saved_review_inspection_us",
            Json::Number(review_inspection_us),
        ),
        (
            "first_worker_event_ms",
            Json::Array(
                first_events
                    .iter()
                    .map(|(lane, elapsed)| {
                        Json::object([
                            ("lane", Json::text(lane)),
                            ("elapsed_ms", Json::Number(*elapsed)),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("cost", Json::Null),
        ("renderer_event_lag_ms", Json::Null),
        ("human_coordination_ms", Json::Null),
        ("evidence_directory", Json::text(f.path.to_str().unwrap())),
    ]);
    fs::write(f.path.join("measurement.json"), report.encode()).unwrap();
    eprintln!("actual fleet evidence retained at {}", f.path.display());
    if workers.len() == 4 && concurrency == 5 {
        assert_eq!(peak_workers, 4, "four-worker overlap not observed; timing evidence is retained but acceptance is incomplete");
    }
    report
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

#[test]
fn native_fleet_registration_retries_only_the_exact_service_instance() {
    let f = Fixture::new("registration");
    f.desktop.register_fleet(f.service.clone()).unwrap();
    let allocation = f.path.join("other-allocations");
    fs::create_dir(&allocation).unwrap();
    fs::set_permissions(&allocation, fs::Permissions::from_mode(0o700)).unwrap();
    let allocator = Arc::new(
        NativeLaneAllocator::open(
            &allocation,
            TrustedReviewers::default(),
            CheckpointRuntimeParameters {
                idle_interval: Some(Duration::from_millis(10)),
                maximum_uncheckpointed_bytes: Some(65536),
                maximum_uncheckpointed_interval: Some(Duration::from_secs(60)),
            },
            vec![],
        )
        .unwrap(),
    );
    let mut runtime = Runtime::open(
        FleetStore::open(f.path.join("other.sqlite")).unwrap(),
        &f.service.objective().unwrap(),
    )
    .unwrap();
    runtime
        .record(
            "start",
            Command::Start {
                goal: "Replacement".into(),
                limits: Limits {
                    lanes: 1,
                    concurrency: 1,
                    depth: 0,
                    retries: 0,
                },
            },
        )
        .unwrap();
    let replacement =
        Arc::new(FleetService::new(runtime, allocator, BTreeSet::from(["codex".into()])).unwrap());
    assert!(f.desktop.register_fleet(replacement).is_err());
    assert!(f
        .call("context", &Json::object([] as [(&str, Json); 0]))
        .is_ok());
}

#[test]
fn native_saved_review_readers_pin_exact_results_across_new_edits_navigation_and_cancellation() {
    use mesh_daemon::fleet::service::SavedReviewSelection;
    let mut f = Fixture::new("pinned-native-reader");
    f.credential = f
        .service
        .grant_with_signer(
            &f.lane,
            "root-run",
            "review-reader-session",
            Arc::new(TestCheckpointSigner(ed25519_dalek::SigningKey::from_bytes(
                &[0x79; 32],
            ))),
        )
        .unwrap();
    let context = f.context();
    let root = PathBuf::from(text(context.get("workspace").unwrap(), "root"));
    let selected = f.desktop.workspace_state().unwrap();
    assert!(f
        .service
        .saved_reviews(&f.lane, None)
        .unwrap()
        .get("reviews")
        .unwrap()
        .as_array()
        .unwrap()
        .is_empty());
    fs::write(root.join("note.txt"), "pinned first result\n").unwrap();
    let first = f
        .call(
            "checkpoint",
            &Json::object([("request", Json::text("first"))]),
        )
        .unwrap();
    assert!(f
        .service
        .saved_reviews(&f.lane, None)
        .unwrap()
        .get("reviews")
        .unwrap()
        .as_array()
        .unwrap()
        .is_empty());
    let first_review = f
        .call(
            "submit_review",
            &Json::object([("checkpoint", first.get("checkpoint").unwrap().clone())]),
        )
        .unwrap();
    let selection = SavedReviewSelection::new(
        &f.lane,
        text(&first, "checkpoint"),
        text(&first, "version"),
        text(&first_review, "bundle"),
    )
    .unwrap();
    let input_comparison = f
        .service
        .saved_starting_comparison(&selection, None, None)
        .unwrap();
    let compared = input_comparison
        .get("input")
        .unwrap()
        .get("comparison")
        .unwrap();
    assert_eq!(compared.get("total").and_then(Json::as_u64), Some(1));
    let change = &compared.get("changes").unwrap().as_array().unwrap()[0];
    assert_eq!(
        change.get("effect").and_then(Json::as_text),
        Some("modified")
    );
    let input_object = text(change, "object").to_owned();
    let input_detail = f
        .service
        .saved_starting_comparison(&selection, None, Some(&input_object))
        .unwrap();
    let detail = &input_detail
        .get("input")
        .unwrap()
        .get("comparison")
        .unwrap()
        .get("changes")
        .unwrap()
        .as_array()
        .unwrap()[0];
    assert_eq!(
        detail
            .get("before")
            .unwrap()
            .get("text")
            .and_then(Json::as_text),
        Some("immutable input\n")
    );
    assert_eq!(
        detail
            .get("after")
            .unwrap()
            .get("text")
            .and_then(Json::as_text),
        Some("pinned first result\n")
    );
    assert_eq!(
        input_detail.get("approval_authority"),
        Some(&Json::Bool(false))
    );
    assert!(f
        .service
        .saved_starting_comparison(&selection, None, Some("../note.txt"))
        .is_err());
    assert!(f
        .service
        .saved_starting_comparison(&selection, Some(&input_object), Some(&input_object))
        .is_err());
    let frozen = f.service.saved_review(&selection).unwrap();
    assert_eq!(frozen.get("selection"), Some(&selection.to_json()));
    let item = frozen.get("review").unwrap();
    assert_eq!(item.get("content_complete"), Some(&Json::Bool(true)));
    let object = item
        .get("bundle_changes")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .find(|change| change.get("path_after").and_then(Json::as_text) == Some("/note.txt"))
        .unwrap()
        .get("object_id")
        .unwrap()
        .as_text()
        .unwrap();
    assert_eq!(
        f.service
            .saved_review_artifact(&selection, object, "after")
            .unwrap()
            .bytes(),
        b"pinned first result\n"
    );
    fs::write(root.join("note.txt"), "second saved result\n").unwrap();
    let second = f
        .call(
            "checkpoint",
            &Json::object([("request", Json::text("second"))]),
        )
        .unwrap();
    let second_review = f
        .call(
            "submit_review",
            &Json::object([("checkpoint", second.get("checkpoint").unwrap().clone())]),
        )
        .unwrap();
    let second_selection = SavedReviewSelection::new(
        &f.lane,
        text(&second, "checkpoint"),
        text(&second, "version"),
        text(&second_review, "bundle"),
    )
    .unwrap();
    fs::write(root.join("note.txt"), "unsaved work continues\n").unwrap();
    let page = f.service.saved_reviews(&f.lane, None).unwrap();
    assert_eq!(page.get("total").and_then(Json::as_u64), Some(2));
    let rows = page.get("reviews").unwrap().as_array().unwrap();
    let tail = f
        .service
        .saved_reviews(&f.lane, Some(text(&rows[0], "checkpoint")))
        .unwrap();
    assert_eq!(tail.get("reviews").unwrap().as_array().unwrap(), &rows[1..]);
    assert!(f
        .service
        .saved_reviews(&f.lane, Some("unknown-cursor"))
        .is_err());
    assert!(f.service.saved_reviews("other-lane", None).is_err());
    let wrong = SavedReviewSelection::new(
        &f.lane,
        text(&first, "checkpoint"),
        text(&second, "version"),
        text(&first_review, "bundle"),
    )
    .unwrap();
    assert!(f.service.saved_review(&wrong).is_err());
    // Each selection member is independently authoritative: another recorded bundle cannot
    // substitute for this checkpoint, and an unknown checkpoint cannot borrow a known review.
    assert_ne!(
        text(&first_review, "bundle"),
        text(&second_review, "bundle")
    );
    for (checkpoint, bundle) in [
        (text(&first, "checkpoint"), text(&second_review, "bundle")),
        ("missing-checkpoint", text(&first_review, "bundle")),
    ] {
        let wrong = SavedReviewSelection::new(&f.lane, checkpoint, text(&first, "version"), bundle)
            .unwrap();
        assert!(f.service.saved_review(&wrong).is_err());
        assert!(f
            .service
            .saved_starting_comparison(&wrong, None, None)
            .is_err());
        assert!(f
            .service
            .saved_review_artifact(&wrong, object, "after")
            .is_err());
    }
    let absent_object = mesh_materializer::ObjectId::from_bytes([0x9d; 16]).to_string();
    assert!(mesh_materializer::ObjectId::parse(&absent_object).is_ok());
    assert_ne!(absent_object, object);
    assert!(f
        .service
        .saved_starting_comparison(&selection, None, Some(&absent_object))
        .is_err());
    assert!(f
        .service
        .saved_review_artifact(&selection, &absent_object, "after")
        .is_err());
    assert!(f
        .service
        .saved_starting_comparison(&wrong, None, None)
        .is_err());
    let wrong = SavedReviewSelection::new(
        "different-lane",
        text(&first, "checkpoint"),
        text(&first, "version"),
        text(&first_review, "bundle"),
    )
    .unwrap();
    assert!(f.service.saved_review(&wrong).is_err());
    assert!(f
        .service
        .saved_starting_comparison(&wrong, None, None)
        .is_err());
    assert!(f
        .service
        .saved_review_artifact(&selection, "../note.txt", "after")
        .is_err());
    assert!(f
        .service
        .saved_review_artifact(&selection, object, "working")
        .is_err());
    assert!(SavedReviewSelection::new(
        "../lane",
        "checkpoint",
        text(&first, "version"),
        text(&first_review, "bundle")
    )
    .is_err());
    assert!(SavedReviewSelection::new(
        &f.lane,
        text(&first, "checkpoint"),
        &text(&first, "version").to_uppercase(),
        text(&first_review, "bundle")
    )
    .is_err());
    let navigation = f.path.join("navigation");
    fs::create_dir(&navigation).unwrap();
    fs::write(navigation.join("other.txt"), "another project").unwrap();
    let preview = f
        .desktop
        .preview_folder_import(navigation.to_str().unwrap())
        .unwrap();
    f.desktop
        .confirm_folder_import(
            navigation.to_str().unwrap(),
            f.path.join("navigation.mesh").to_str().unwrap(),
            text(&preview, "summary"),
        )
        .unwrap();
    let navigated = f.desktop.workspace_state().unwrap();
    assert_ne!(selected.root, navigated.root);
    f.service
        .native_command("cancel-reader", Command::Cancel)
        .unwrap();
    f.service.revoke(&f.credential).unwrap();
    let revision = f.service.native_state().unwrap().revision;
    assert_eq!(f.service.saved_review(&selection).unwrap(), frozen);
    assert_eq!(
        f.service
            .saved_starting_comparison(&selection, None, None)
            .unwrap(),
        input_comparison
    );
    assert_eq!(
        f.service
            .saved_starting_comparison(&selection, None, Some(&input_object))
            .unwrap(),
        input_detail
    );
    assert_eq!(
        f.service
            .saved_review_artifact(&selection, object, "after")
            .unwrap()
            .bytes(),
        b"pinned first result\n"
    );
    assert_eq!(
        f.service
            .saved_review_artifact(&second_selection, object, "after")
            .unwrap()
            .bytes(),
        b"second saved result\n"
    );
    assert_eq!(f.service.native_state().unwrap().revision, revision);
    assert_eq!(f.desktop.workspace_state().unwrap(), navigated);
    assert_eq!(
        fs::read(root.join("note.txt")).unwrap(),
        b"unsaved work continues\n"
    );
    assert_eq!(
        fs::read(f.path.join("original/note.txt")).unwrap(),
        b"immutable input\n"
    );
    fs::rename(&root, f.path.join("retained-reader-lane")).unwrap();
    fs::create_dir(&root).unwrap();
    fs::write(root.join("note.txt"), "foreign replacement").unwrap();
    assert!(f.service.saved_review(&selection).is_err());
    assert!(f
        .service
        .saved_starting_comparison(&selection, None, None)
        .is_err());
    assert!(f
        .service
        .saved_review_artifact(&selection, object, "after")
        .is_err());
    assert_eq!(
        fs::read(root.join("note.txt")).unwrap(),
        b"foreign replacement"
    );
}

#[test]
fn saved_review_pages_are_bounded_and_cover_every_checkpoint_without_duplicates() {
    let mut f = Fixture::new("review-pages");
    f.credential = f
        .service
        .grant_with_signer(
            &f.lane,
            "root-run",
            "review-page-session",
            Arc::new(TestCheckpointSigner(ed25519_dalek::SigningKey::from_bytes(
                &[0x7a; 32],
            ))),
        )
        .unwrap();
    let mut expected = Vec::new();
    for index in 0..53 {
        let checkpoint = f
            .call(
                "checkpoint",
                &Json::object([("request", Json::text(format!("page-{index:02}")))]),
            )
            .unwrap();
        f.call(
            "submit_review",
            &Json::object([("checkpoint", checkpoint.get("checkpoint").unwrap().clone())]),
        )
        .unwrap();
        expected.push(text(&checkpoint, "checkpoint").to_owned());
    }
    expected.sort();
    let first = f.service.saved_reviews(&f.lane, None).unwrap();
    assert_eq!(first.get("total").and_then(Json::as_u64), Some(53));
    let rows = first.get("reviews").unwrap().as_array().unwrap();
    assert_eq!(rows.len(), 50);
    assert_eq!(text(&first, "next_after"), expected[49]);
    let second = f
        .service
        .saved_reviews(&f.lane, Some(text(&first, "next_after")))
        .unwrap();
    let tail = second.get("reviews").unwrap().as_array().unwrap();
    assert_eq!(tail.len(), 3);
    assert_eq!(second.get("next_after"), Some(&Json::Null));
    let actual: Vec<_> = rows
        .iter()
        .chain(tail.iter())
        .map(|row| text(row, "checkpoint").to_owned())
        .collect();
    assert_eq!(actual, expected);
    let end = f
        .service
        .saved_reviews(&f.lane, expected.last().map(String::as_str))
        .unwrap();
    assert!(end.get("reviews").unwrap().as_array().unwrap().is_empty());
    assert_eq!(end.get("next_after"), Some(&Json::Null));
}

#[test]
fn starting_comparison_pages_changes_and_bounds_selected_content() {
    use mesh_daemon::fleet::service::SavedReviewSelection;
    let mut f = Fixture::new("starting-comparison-pages");
    f.credential = f
        .service
        .grant_with_signer(
            &f.lane,
            "root-run",
            "input-comparison",
            Arc::new(TestCheckpointSigner(ed25519_dalek::SigningKey::from_bytes(
                &[0x7b; 32],
            ))),
        )
        .unwrap();
    let context = f.context();
    let root = PathBuf::from(text(context.get("workspace").unwrap(), "root"));
    for index in 0..201 {
        fs::write(
            root.join(format!("added-{index:03}.txt")),
            "added content\n",
        )
        .unwrap();
    }
    fs::write(root.join("binary"), [0xff, 0, 1]).unwrap();
    fs::write(root.join("large"), vec![b'x'; 262_145]).unwrap();
    fs::write(root.join("unsafe"), "text\u{202e}hidden").unwrap();
    fs::set_permissions(root.join("note.txt"), fs::Permissions::from_mode(0o755)).unwrap();
    let saved = f
        .call(
            "checkpoint",
            &Json::object([("request", Json::text("input-page"))]),
        )
        .unwrap();
    let review = f
        .call(
            "submit_review",
            &Json::object([("checkpoint", saved.get("checkpoint").unwrap().clone())]),
        )
        .unwrap();
    let selection = SavedReviewSelection::new(
        &f.lane,
        text(&saved, "checkpoint"),
        text(&saved, "version"),
        text(&review, "bundle"),
    )
    .unwrap();
    let mut after = None;
    let mut seen = BTreeSet::new();
    let mut details = BTreeMap::new();
    loop {
        let response = f
            .service
            .saved_starting_comparison(&selection, after.as_deref(), None)
            .unwrap();
        let page = response.get("input").unwrap().get("comparison").unwrap();
        assert_eq!(page.get("total").and_then(Json::as_u64), Some(205));
        let changes = page.get("changes").unwrap().as_array().unwrap();
        assert!(changes.len() <= 200);
        for change in changes {
            let object = text(change, "object");
            assert!(seen.insert(object.to_owned()));
            let side = change.get("after").unwrap();
            assert_eq!(side.get("text"), Some(&Json::Null));
            if ["binary", "large", "unsafe", "note.txt"].contains(&text(side, "path")) {
                details.insert(text(side, "path").to_owned(), object.to_owned());
            }
        }
        after = page.get("next_after").unwrap().as_text().map(str::to_owned);
        if after.is_none() {
            break;
        }
    }
    assert_eq!(seen.len(), 205);
    for (path, expected) in [
        ("binary", "binary-or-unsafe-text"),
        ("large", "too-large"),
        ("unsafe", "binary-or-unsafe-text"),
        ("note.txt", "text"),
    ] {
        let detail = f
            .service
            .saved_starting_comparison(&selection, None, Some(&details[path]))
            .unwrap();
        let changes = detail
            .get("input")
            .unwrap()
            .get("comparison")
            .unwrap()
            .get("changes")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(
            text(changes[0].get("after").unwrap(), "content_state"),
            expected
        );
        if path == "note.txt" {
            assert_eq!(
                changes[0].get("before").unwrap().get("executable"),
                Some(&Json::Bool(false))
            );
            assert_eq!(
                changes[0].get("after").unwrap().get("executable"),
                Some(&Json::Bool(true))
            );
        }
    }
    assert!(f
        .service
        .saved_starting_comparison(&selection, Some(&"0".repeat(32)), None)
        .is_err());
    assert_eq!(
        fs::read(f.path.join("original/note.txt")).unwrap(),
        b"immutable input\n"
    );
}

#[test]
fn restarted_history_reads_preserve_work_and_never_restore_execution_authority() {
    use mesh_daemon::fleet::service::SavedReviewSelection;
    fn files(root: &std::path::Path) -> BTreeMap<PathBuf, Vec<u8>> {
        fn visit(
            root: &std::path::Path,
            path: &std::path::Path,
            result: &mut BTreeMap<PathBuf, Vec<u8>>,
        ) {
            for entry in fs::read_dir(path).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                let kind = entry.file_type().unwrap();
                if kind.is_dir() {
                    visit(root, &path, result);
                } else if kind.is_file() {
                    result.insert(
                        path.strip_prefix(root).unwrap().into(),
                        fs::read(&path).unwrap(),
                    );
                } else {
                    panic!("unexpected fixture entry");
                }
            }
        }
        let mut result = BTreeMap::new();
        visit(root, root, &mut result);
        result
    }
    let mut f = Fixture::new("history-restart");
    f.credential = f
        .service
        .grant_with_signer(
            &f.lane,
            "root-run",
            "history-session",
            Arc::new(TestCheckpointSigner(ed25519_dalek::SigningKey::from_bytes(
                &[0x51; 32],
            ))),
        )
        .unwrap();
    let context = f.context();
    let root = PathBuf::from(text(context.get("workspace").unwrap(), "root"));
    fs::write(root.join("note.txt"), "saved result\n").unwrap();
    let saved = f
        .call(
            "checkpoint",
            &Json::object([("request", Json::text("saved"))]),
        )
        .unwrap();
    let review = f
        .call(
            "submit_review",
            &Json::object([("checkpoint", saved.get("checkpoint").unwrap().clone())]),
        )
        .unwrap();
    let selection = SavedReviewSelection::new(
        &f.lane,
        text(&saved, "checkpoint"),
        text(&saved, "version"),
        text(&review, "bundle"),
    )
    .unwrap();
    let frozen = f.service.saved_review(&selection).unwrap();
    let comparison = f
        .service
        .saved_starting_comparison(&selection, None, None)
        .unwrap();
    let object = text(
        &comparison
            .get("input")
            .unwrap()
            .get("comparison")
            .unwrap()
            .get("changes")
            .unwrap()
            .as_array()
            .unwrap()[0],
        "object",
    )
    .to_owned();
    let detail = f
        .service
        .saved_starting_comparison(&selection, None, Some(&object))
        .unwrap();
    let artifact = f
        .service
        .saved_review_artifact(&selection, &object, "after")
        .unwrap();
    fs::write(root.join("note.txt"), "uncheckpointed editor work\n").unwrap();
    let state = f.service.native_state().unwrap();
    // Drop the original registry and lane service; the new host receives only durable history.
    f.desktop = Arc::new(LiveDaemon::new(StartupSummary::from(&nothing_to_recover())));
    let parameters = CheckpointRuntimeParameters {
        idle_interval: Some(Duration::from_millis(10)),
        maximum_uncheckpointed_bytes: Some(65_536),
        maximum_uncheckpointed_interval: Some(Duration::from_secs(60)),
    };
    f.service = Arc::new(
        FleetService::new(
            Runtime::open(
                FleetStore::open(f.path.join("fleet.sqlite")).unwrap(),
                "objective",
            )
            .unwrap(),
            Arc::new(
                NativeLaneAllocator::open(
                    &f.path.join("allocations"),
                    TrustedReviewers::default(),
                    parameters,
                    vec![],
                )
                .unwrap(),
            ),
            BTreeSet::from(["codex".into()]),
        )
        .unwrap(),
    );
    fs::rename(
        f.path.join("source.mesh"),
        f.path.join("offline-source.mesh"),
    )
    .unwrap();
    let before = files(&f.path.join("allocations"));
    // Restored history must preserve the exact recorded tuple, just like a current context.
    for (checkpoint, version, bundle) in [
        (
            text(&saved, "checkpoint"),
            text(&saved, "version"),
            "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
        ),
        (
            "unknown-checkpoint",
            text(&saved, "version"),
            text(&review, "bundle"),
        ),
        (
            text(&saved, "checkpoint"),
            "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
            text(&review, "bundle"),
        ),
    ] {
        let wrong = SavedReviewSelection::new(&f.lane, checkpoint, version, bundle).unwrap();
        assert!(f.service.saved_review(&wrong).is_err());
        assert!(f
            .service
            .saved_starting_comparison(&wrong, None, None)
            .is_err());
        assert!(f
            .service
            .saved_review_artifact(&wrong, &object, "after")
            .is_err());
    }
    assert_eq!(files(&f.path.join("allocations")), before);
    assert_eq!(f.service.saved_review(&selection).unwrap(), frozen);
    assert_eq!(
        f.service
            .saved_starting_comparison(&selection, None, None)
            .unwrap(),
        comparison
    );
    assert_eq!(
        f.service
            .saved_starting_comparison(&selection, None, Some(&object))
            .unwrap(),
        detail
    );
    assert_eq!(
        f.service
            .saved_review_artifact(&selection, &object, "after")
            .unwrap(),
        artifact
    );
    assert_eq!(f.service.native_state().unwrap(), state);
    assert_eq!(files(&f.path.join("allocations")), before);
    let allocation = f.path.join("allocations");
    let index = allocation.join(
        before
            .keys()
            .find(|path| {
                path.file_name() == Some(std::ffi::OsStr::new(mesh_store::DATABASE_FILE_NAME))
            })
            .unwrap(),
    );
    let retained_index = f.path.join("retained-index");
    fs::rename(&index, &retained_index).unwrap();
    assert_eq!(f.service.saved_review(&selection).unwrap(), frozen);
    assert!(
        !index.exists(),
        "history inspection must not rebuild the durable index"
    );
    fs::rename(&retained_index, &index).unwrap();
    let journal = allocation.join(
        before
            .keys()
            .find(|path| {
                path.file_name() == Some(std::ffi::OsStr::new(mesh_daemon::RECORD_FILE_NAME))
            })
            .unwrap(),
    );
    let retained_journal = f.path.join("retained-journal");
    fs::rename(&journal, &retained_journal).unwrap();
    assert!(f.service.saved_review(&selection).is_err());
    assert!(!journal.exists(), "missing history must not be recreated");
    std::os::unix::fs::symlink(&retained_journal, &journal).unwrap();
    assert!(f.service.saved_review(&selection).is_err());
    fs::remove_file(&journal).unwrap();
    fs::rename(&retained_journal, &journal).unwrap();
    assert_eq!(files(&allocation), before);
    let storage = journal.parent().unwrap();
    let scratch = storage.join("scratch");
    let retained_scratch = f.path.join("retained-scratch");
    fs::rename(&scratch, &retained_scratch).unwrap();
    assert!(f.service.saved_review(&selection).is_err());
    assert!(
        !scratch.exists(),
        "inspection cannot recreate CAS directories"
    );
    fs::rename(&retained_scratch, &scratch).unwrap();
    let chunk = before
        .iter()
        .find(|(path, bytes)| {
            path.components().any(|part| part.as_os_str() == "chunks")
                && bytes.as_slice() == b"saved result\n"
        })
        .map(|(path, _)| allocation.join(path))
        .unwrap();
    let saved_chunk = fs::read(&chunk).unwrap();
    fs::write(&chunk, b"corrupt retained bytes").unwrap();
    let corrupt = files(&allocation);
    assert!(f
        .service
        .saved_review_artifact(&selection, &object, "after")
        .is_err());
    assert_eq!(
        files(&allocation),
        corrupt,
        "read errors must not quarantine retained chunks"
    );
    fs::write(&chunk, saved_chunk).unwrap();
    assert_eq!(
        f.service
            .saved_review_artifact(&selection, &object, "after")
            .unwrap(),
        artifact
    );

    assert!(f
        .service
        .agent_call(
            f.credential.transport_value(),
            "context",
            &Json::empty_object()
        )
        .is_err());
    assert!(f
        .service
        .grant(&f.lane, "root-run", "new-actor", "new-session")
        .is_err());
    assert_eq!(
        fs::read(root.join("note.txt")).unwrap(),
        b"uncheckpointed editor work\n"
    );
    let retained = root.with_file_name("retained-history");
    fs::rename(&root, &retained).unwrap();
    fs::create_dir(&root).unwrap();
    fs::write(root.join("note.txt"), "replacement").unwrap();
    assert!(f.service.saved_review(&selection).is_err());
    assert!(f
        .service
        .saved_starting_comparison(&selection, None, None)
        .is_err());
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    fs::remove_dir_all(&root).unwrap();
    fs::rename(&retained, &root).unwrap();
    assert_eq!(f.service.saved_review(&selection).unwrap(), frozen);
}

#[test]
fn native_review_change_requests_are_exact_retryable_and_visible_only_to_the_originating_lane() {
    use mesh_daemon::fleet::service::SavedReviewSelection;
    let mut f = Fixture::new("review-change-requests");
    f.credential = f
        .service
        .grant_with_signer(
            &f.lane,
            "root-run",
            "review-feedback-session",
            Arc::new(TestCheckpointSigner(ed25519_dalek::SigningKey::from_bytes(
                &[0x68; 32],
            ))),
        )
        .unwrap();
    let context = f.context();
    let root = PathBuf::from(text(context.get("workspace").unwrap(), "root"));
    fs::write(root.join("note.txt"), "saved result\n").unwrap();
    let checkpoint = f
        .call(
            "checkpoint",
            &Json::object([("request", Json::text("result"))]),
        )
        .unwrap();
    let review = f
        .call(
            "submit_review",
            &Json::object([("checkpoint", checkpoint.get("checkpoint").unwrap().clone())]),
        )
        .unwrap();
    let selection = SavedReviewSelection::new(
        &f.lane,
        text(&checkpoint, "checkpoint"),
        text(&checkpoint, "version"),
        text(&review, "bundle"),
    )
    .unwrap();
    let before = f.service.native_state().unwrap();
    let message = "Preserve the previous introduction and add an example.";
    let receipt = f
        .service
        .request_review_changes("reviewer-request", &selection, message)
        .unwrap();
    assert_eq!(text(&receipt, "status"), "recorded");
    assert_eq!(receipt.get("approval_authority"), Some(&Json::Bool(false)));
    assert_eq!(
        f.service
            .request_review_changes("reviewer-request", &selection, message)
            .unwrap(),
        receipt
    );
    assert!(f
        .service
        .request_review_changes("reviewer-request", &selection, "Different feedback")
        .is_err());
    let after = f.service.native_state().unwrap();
    assert_eq!(before.lanes, after.lanes);
    assert_eq!(before.checkpoints, after.checkpoints);
    assert_eq!(after.revision, before.revision + 1);
    assert_eq!(
        f.service.saved_review_changes(&selection).unwrap(),
        Json::Array(vec![receipt.clone()])
    );
    assert_eq!(
        f.context().get("review_change_requests"),
        Some(&Json::Array(vec![receipt]))
    );
    assert!(f
        .call(
            "request_review_changes",
            &Json::object([("message", Json::text("Agent cannot impersonate review"))])
        )
        .is_err());
    let child = f.call("delegate", &f.delegate("feedback-child")).unwrap();
    let child_id = text(&child, "id");
    f.service
        .native_command(
            "dispatch-feedback-child",
            Command::Dispatch {
                lane: child_id.into(),
                run: "child-run".into(),
            },
        )
        .unwrap();
    let child_credential = f
        .service
        .grant(child_id, "child-run", "child-actor", "child-session")
        .unwrap();
    let child_context = f
        .service
        .agent_call(
            child_credential.transport_value(),
            "context",
            &Json::empty_object(),
        )
        .unwrap();
    assert_eq!(
        child_context.get("review_change_requests"),
        Some(&Json::Array(vec![]))
    );
    let feedback_id = f
        .service
        .native_state()
        .unwrap()
        .review_change_requests
        .keys()
        .next()
        .unwrap()
        .clone();
    let propose_original = Json::object([
        ("request", Json::text(&feedback_id)),
        ("checkpoint", checkpoint.get("checkpoint").unwrap().clone()),
    ]);
    assert!(f
        .call("propose_review_change_result", &propose_original)
        .is_err());
    assert!(f
        .service
        .agent_call(
            child_credential.transport_value(),
            "propose_review_change_result",
            &propose_original
        )
        .is_err());
    fs::write(root.join("note.txt"), "revised result with example\n").unwrap();
    let revised = f
        .call(
            "checkpoint",
            &Json::object([("request", Json::text("revision"))]),
        )
        .unwrap();
    let proposed = Json::object([
        ("request", Json::text(&feedback_id)),
        ("checkpoint", revised.get("checkpoint").unwrap().clone()),
    ]);
    assert!(f.call("propose_review_change_result", &proposed).is_err());
    let new_review = f
        .call(
            "submit_review",
            &Json::object([("checkpoint", revised.get("checkpoint").unwrap().clone())]),
        )
        .unwrap();
    let before_proposal = f.service.native_state().unwrap();
    let proposal = f.call("propose_review_change_result", &proposed).unwrap();
    assert_eq!(text(&proposal, "status"), "proposed");
    assert_eq!(proposal.get("version"), revised.get("version"));
    assert_eq!(proposal.get("bundle"), new_review.get("bundle"));
    assert_eq!(proposal.get("approval_authority"), Some(&Json::Bool(false)));
    assert_eq!(
        f.call("propose_review_change_result", &proposed).unwrap(),
        proposal
    );
    let after_proposal = f.service.native_state().unwrap();
    assert_eq!(before_proposal.lanes, after_proposal.lanes);
    assert_eq!(before_proposal.checkpoints, after_proposal.checkpoints);
    assert_eq!(
        before_proposal.review_change_requests,
        after_proposal.review_change_requests
    );
    let activity = f.service.saved_review_change_activity(&selection).unwrap();
    assert_eq!(
        activity.get("responses"),
        Some(&Json::Array(vec![proposal.clone()]))
    );
    assert_eq!(
        f.context().get("review_change_responses"),
        Some(&Json::Array(vec![proposal]))
    );
    assert_eq!(
        f.service
            .agent_call(
                child_credential.transport_value(),
                "context",
                &Json::empty_object()
            )
            .unwrap()
            .get("review_change_responses"),
        Some(&Json::Array(vec![]))
    );
    let target = text(&revised, "checkpoint");
    let before_decision = f.service.native_state().unwrap();
    for wrong in [
        SavedReviewSelection::new(
            &f.lane,
            "wrong-checkpoint",
            text(&checkpoint, "version"),
            text(&review, "bundle"),
        )
        .unwrap(),
        SavedReviewSelection::new(
            &f.lane,
            text(&checkpoint, "checkpoint"),
            &"0".repeat(64),
            text(&review, "bundle"),
        )
        .unwrap(),
        SavedReviewSelection::new(
            &f.lane,
            text(&checkpoint, "checkpoint"),
            text(&checkpoint, "version"),
            &"0".repeat(64),
        )
        .unwrap(),
    ] {
        assert!(f
            .service
            .decide_review_change(
                &wrong,
                &feedback_id,
                "wrong-selection",
                0,
                Some(target),
                |_| panic!("substituted selection must not reach confirmation")
            )
            .is_err());
        assert_eq!(f.service.native_state().unwrap(), before_decision);
    }
    let cancelled = f
        .service
        .decide_review_change(
            &selection,
            &feedback_id,
            "cancel-decision",
            0,
            Some(target),
            |prompt| {
                assert!(prompt.contains(message));
                assert!(prompt.contains(text(&revised, "version")));
                assert!(prompt.contains(text(&new_review, "bundle")));
                assert_eq!(f.service.native_state().unwrap(), before_decision); // Confirmation holds no fleet lock.
                false
            },
        )
        .unwrap();
    assert_eq!(cancelled.get("cancelled"), Some(&Json::Bool(true)));
    assert_eq!(f.service.native_state().unwrap(), before_decision);
    // A competing confirmed choice wins while the first native dialog is open.
    let stale = f.service.decide_review_change(
        &selection,
        &feedback_id,
        "stale-decision",
        0,
        Some(target),
        |_| {
            f.service
                .decide_review_change(
                    &selection,
                    &feedback_id,
                    "winning-decision",
                    0,
                    Some(target),
                    |_| true,
                )
                .unwrap();
            true
        },
    );
    assert!(stale.is_err());
    let receipt = f
        .service
        .decide_review_change(
            &selection,
            &feedback_id,
            "winning-decision",
            0,
            Some(target),
            |_| panic!("retry must not reconfirm"),
        )
        .unwrap();
    assert_eq!(text(receipt.get("current").unwrap(), "status"), "addressed");
    assert_eq!(
        receipt.get("current").unwrap().get("approval_authority"),
        Some(&Json::Bool(false))
    );
    assert!(f
        .call("decide_review_change", &Json::empty_object())
        .is_err());
    f.service
        .decide_review_change(&selection, &feedback_id, "reopen-decision", 1, None, |_| {
            true
        })
        .unwrap();
    let late = f
        .service
        .decide_review_change(
            &selection,
            &feedback_id,
            "winning-decision",
            0,
            Some(target),
            |_| panic!("receipt recovery must not reconfirm"),
        )
        .unwrap();
    assert_eq!(text(late.get("receipt").unwrap(), "status"), "addressed");
    assert_eq!(text(late.get("current").unwrap(), "status"), "open");
    assert_eq!(
        late.get("current")
            .unwrap()
            .get("revision")
            .and_then(Json::as_u64),
        Some(2)
    );
    assert!(f
        .service
        .decide_review_change(
            &selection,
            &feedback_id,
            "winning-decision",
            2,
            Some(target),
            |_| panic!("reused operation must refuse")
        )
        .is_err());
    let after_decision = f.service.native_state().unwrap();
    assert_eq!(before_decision.lanes, after_decision.lanes);
    assert_eq!(before_decision.checkpoints, after_decision.checkpoints);
    assert_eq!(
        before_decision.review_change_requests,
        after_decision.review_change_requests
    );
    assert_eq!(
        before_decision.review_change_responses,
        after_decision.review_change_responses
    );
    assert_eq!(
        f.context()
            .get("review_change_decisions")
            .unwrap()
            .as_array()
            .unwrap()[0]
            .get("revision")
            .and_then(Json::as_u64),
        Some(2)
    );
    let before_replacement = f.service.native_state().unwrap();
    let moved = f.path.join("decision-moved-workspace");
    let replaced = f.service.decide_review_change(
        &selection,
        &feedback_id,
        "replaced-during-confirmation",
        2,
        Some(target),
        |_| {
            fs::rename(&root, &moved).unwrap();
            true
        },
    );
    fs::rename(&moved, &root).unwrap();
    assert!(replaced.is_err());
    assert_eq!(f.service.native_state().unwrap(), before_replacement);
    fs::write(root.join("note.txt"), "newer unreviewed work\n").unwrap();
    assert_eq!(
        text(
            &f.service
                .saved_review_changes(&selection)
                .unwrap()
                .as_array()
                .unwrap()[0],
            "version"
        ),
        text(&checkpoint, "version")
    );
}

#[test]
fn explicit_file_deletion_is_scoped_replayable_and_allows_review_after_new_checkpoint() {
    let mut f = Fixture::new("explicit-deletion");
    let signer = Arc::new(TestCheckpointSigner(ed25519_dalek::SigningKey::from_bytes(
        &[0x75; 32],
    )));
    f.credential = f
        .service
        .grant_with_signer(&f.lane, "root-run", "delete-session", signer.clone())
        .unwrap();
    let context = f.context();
    let root = PathBuf::from(text(context.get("workspace").unwrap(), "root"));
    let initial_main = f.desktop.workspace_state().unwrap().shared_version;
    assert!(f
        .call("missing_files", &Json::empty_object())
        .unwrap()
        .get("files")
        .unwrap()
        .as_array()
        .unwrap()
        .is_empty());
    fs::remove_file(root.join("note.txt")).unwrap();
    let inventory = f.call("missing_files", &Json::empty_object()).unwrap();
    let files = inventory.get("files").unwrap().as_array().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(text(&files[0], "path"), "note.txt");
    let args = Json::object([
        ("request", Json::text("delete-note")),
        ("path", Json::text("note.txt")),
        ("version", files[0].get("version").unwrap().clone()),
    ]);
    let blocked = f
        .call(
            "checkpoint",
            &Json::object([("request", Json::text("before-resolution"))]),
        )
        .unwrap();
    assert_eq!(blocked.get("complete"), Some(&Json::Bool(false)));
    let saved = f.call("resolve_file_deletion", &args).unwrap();
    assert_eq!(saved.get("approval_authority"), Some(&Json::Bool(false)));
    assert_eq!(f.call("resolve_file_deletion", &args).unwrap(), saved);
    let state = f.service.native_state().unwrap();
    assert_eq!(state.file_deletions.len(), 1);
    let receipt = state.file_deletions.values().next().unwrap();
    assert!(receipt.operation.is_some());
    assert!(receipt.result.is_some());
    let complete = f
        .call(
            "checkpoint",
            &Json::object([("request", Json::text("after-resolution"))]),
        )
        .unwrap();
    assert_eq!(complete.get("complete"), Some(&Json::Bool(true)));
    assert_eq!(complete.get("saved_changes"), Some(&Json::Number(0)));
    // A canonical no-op is inspectable, including its real deletion relative to lane input.
    let empty_review = f
        .call(
            "submit_review",
            &Json::object([("checkpoint", complete.get("checkpoint").unwrap().clone())]),
        )
        .unwrap();
    let selection = mesh_daemon::fleet::service::SavedReviewSelection::new(
        &f.lane,
        text(&complete, "checkpoint"),
        text(&complete, "version"),
        text(&empty_review, "bundle"),
    )
    .unwrap();
    let inspected = f.service.saved_review(&selection).unwrap();
    assert_eq!(
        inspected.get("review").unwrap().get("content_complete"),
        Some(&Json::Bool(true))
    );
    assert!(inspected
        .get("review")
        .unwrap()
        .get("bundle_changes")
        .unwrap()
        .as_array()
        .unwrap()
        .is_empty());
    let input = f
        .service
        .saved_starting_comparison(&selection, None, None)
        .unwrap();
    assert!(input.encode().contains("note.txt"));
    fs::write(root.join("remaining.txt"), "retained result\n").unwrap();
    let reviewable = f
        .call(
            "checkpoint",
            &Json::object([("request", Json::text("remaining-result"))]),
        )
        .unwrap();
    assert_eq!(reviewable.get("complete"), Some(&Json::Bool(true)));
    f.call(
        "submit_review",
        &Json::object([("checkpoint", reviewable.get("checkpoint").unwrap().clone())]),
    )
    .unwrap();
    fs::write(root.join("note.txt"), "later recreated work\n").unwrap();
    assert_eq!(f.service.saved_review(&selection).unwrap(), inspected);
    assert_eq!(
        f.service
            .saved_starting_comparison(&selection, None, None)
            .unwrap(),
        input
    );
    assert_eq!(f.call("resolve_file_deletion", &args).unwrap(), saved);
    assert_eq!(
        fs::read(root.join("note.txt")).unwrap(),
        b"later recreated work\n"
    );
    let conflicting = Json::object([
        ("request", Json::text("delete-note")),
        ("path", Json::text("elsewhere")),
        ("version", files[0].get("version").unwrap().clone()),
    ]);
    assert_eq!(
        f.call("resolve_file_deletion", &conflicting)
            .unwrap_err()
            .code,
        "fleet-deletion-request-conflict"
    );
    assert_eq!(
        fs::read(f.path.join("original/note.txt")).unwrap(),
        b"immutable input\n"
    );
    assert_eq!(
        f.desktop.workspace_state().unwrap().shared_version,
        initial_main
    );
    f.credential = f
        .service
        .grant_with_signer(&f.lane, "root-run", "replacement-session", signer)
        .unwrap();
    assert_eq!(
        f.call("resolve_file_deletion", &args).unwrap_err().code,
        "fleet-deletion-request-conflict"
    );
}

#[test]
fn deletion_signer_failure_retains_one_intent_and_retries_without_duplicate_operations() {
    struct Signer {
        key: TestCheckpointSigner,
        fail: std::sync::atomic::AtomicBool,
    }
    impl mesh_daemon::fleet::service::CheckpointSigner for Signer {
        fn public_key(&self) -> mesh_types::PublicKey {
            self.key.public_key()
        }
        fn sign(
            &self,
            payload: &mesh_crypto::SigningPayload,
        ) -> Result<mesh_types::Signature, String> {
            if self.fail.swap(false, std::sync::atomic::Ordering::SeqCst) {
                Err("temporary signer failure".into())
            } else {
                self.key.sign(payload)
            }
        }
    }
    let mut f = Fixture::new("deletion-signer-retry");
    f.credential = f
        .service
        .grant_with_signer(
            &f.lane,
            "root-run",
            "delete-session",
            Arc::new(Signer {
                key: TestCheckpointSigner(ed25519_dalek::SigningKey::from_bytes(&[0x76; 32])),
                fail: std::sync::atomic::AtomicBool::new(true),
            }),
        )
        .unwrap();
    let context = f.context();
    let root = PathBuf::from(text(context.get("workspace").unwrap(), "root"));
    fs::remove_file(root.join("note.txt")).unwrap();
    let inventory = f.call("missing_files", &Json::empty_object()).unwrap();
    let version = inventory.get("files").unwrap().as_array().unwrap()[0]
        .get("version")
        .unwrap()
        .clone();
    let args = Json::object([
        ("request", Json::text("delete-note")),
        ("path", Json::text("note.txt")),
        ("version", version),
    ]);
    assert_eq!(
        f.call("resolve_file_deletion", &args).unwrap_err().code,
        "fleet-deletion-needs-reconciliation"
    );
    let state = f.service.native_state().unwrap();
    let pending = state.file_deletions.values().next().unwrap();
    assert!(pending.operation.is_none());
    assert!(pending.result.is_none());
    let saved = f.call("resolve_file_deletion", &args).unwrap();
    assert_eq!(f.call("resolve_file_deletion", &args).unwrap(), saved);
    assert_eq!(f.service.native_state().unwrap().file_deletions.len(), 1);
}

#[test]
fn automatic_progress_host_saves_a_live_worker_and_final_edits_without_checkpoint_calls() {
    use mesh_daemon::fleet::host::NativeFleetHost;
    use mesh_daemon::fleet::provider::CodexAdapter;
    let f = Fixture::new("automatic-worker-progress");
    let selected = f.desktop.workspace_state().unwrap();
    let child = f.call("delegate", &f.delegate("automatic-child")).unwrap();
    let lane = text(&child, "id").to_owned();
    let root = PathBuf::from(
        f.service.native_state().unwrap().lanes[&lane]
            .workspace
            .as_ref()
            .unwrap()
            .root(),
    );
    let ready = f.path.join("worker-ready");
    let release = f.path.join("worker-release");
    struct Release(PathBuf);
    impl Drop for Release {
        fn drop(&mut self) {
            let _ = fs::write(&self.0, b"release");
        }
    }
    let _release = Release(release.clone());
    let executable = f.path.join("automatic-worker");
    fs::write(
        &executable,
        format!(
            r#"#!/bin/sh
cat >/dev/null
printf 'live private progress\n' > note.txt
touch '{}'
while [ ! -f '{}' ]; do sleep 0.01; done
printf 'final private progress\n' > note.txt
printf '%s\n' '{{"type":"turn.completed"}}'
"#,
            ready.display(),
            release.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let mut host = NativeFleetHost::new(
        f.service.clone(),
        CodexAdapter::new(&executable, &executable).unwrap(),
        f.path.join("ipc.sock"),
        Arc::new(TestWorkerSigners),
    )
    .unwrap();
    host.tick().unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !ready.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    if !ready.exists() {
        fs::write(&release, b"release").unwrap();
        panic!("worker did not start");
    }
    let mut first = None;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        host.poll_owned().unwrap();
        let state = f.service.native_state().unwrap();
        if let Some(version) = state.lanes[&lane].saved {
            first = Some(version);
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if first.is_none() {
        fs::write(&release, b"release").unwrap();
        panic!("live worker progress was not saved automatically");
    }
    assert_eq!(
        f.service.native_state().unwrap().lanes[&lane].runs[0].state,
        RunState::Running
    );
    assert!(f.service.native_state().unwrap().checkpoints.is_empty());
    fs::write(&release, b"release").unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut completed = false;
    let mut observations = Vec::new();
    while std::time::Instant::now() < deadline {
        observations = host.poll_owned().unwrap();
        if f.service.native_state().unwrap().lanes[&lane].runs[0].state == RunState::Succeeded {
            completed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(completed, "final worker capture did not finish");
    let state = f.service.native_state().unwrap();
    assert_ne!(state.lanes[&lane].saved, first);
    assert!(state.checkpoints.is_empty());
    assert_eq!(
        fs::read(root.join("note.txt")).unwrap(),
        b"final private progress\n"
    );
    assert!(observations
        .iter()
        .find(|worker| worker.lane == lane)
        .unwrap()
        .progress
        .as_ref()
        .is_some_and(|save| matches!(save.state, "saved" | "unchanged")));
    assert_eq!(f.desktop.workspace_state().unwrap(), selected);
}
