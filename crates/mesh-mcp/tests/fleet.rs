//! Real stdio MCP -> Unix IPC -> authorized native allocation, without model/provider fixtures.
#![cfg(target_os = "macos")]
use mesh_daemon::fleet::service::{FleetService, NativeLaneAllocator};
use mesh_daemon::fleet::workspace::VersionInput;
use mesh_daemon::fleet::{Command, Limits, Runtime};
use mesh_daemon::ipc::{nothing_to_recover, IpcServer, Operations, StartupSummary};
use mesh_daemon::{CheckpointRuntimeParameters, LiveDaemon, TrustedReviewers};
use mesh_mcp::json::Json;
use mesh_store::fleet::FleetStore;
use std::collections::BTreeSet;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt as _;
use std::process::{Command as Process, Stdio};
use std::sync::Arc;
use std::time::Duration;

fn text<'a>(value: &'a Json, key: &str) -> &'a str {
    value.get(key).and_then(Json::as_text).unwrap()
}
fn call(
    stdin: &mut impl Write,
    stdout: &mut impl BufRead,
    id: u64,
    method: &str,
    params: Json,
    credential: &str,
) -> Json {
    let request = Json::object([
        ("jsonrpc", Json::text("2.0")),
        ("id", Json::Number(id)),
        ("method", Json::text(method)),
        ("params", params),
    ]);
    writeln!(stdin, "{}", request.encode()).unwrap();
    stdin.flush().unwrap();
    let mut response = String::new();
    stdout.read_line(&mut response).unwrap();
    assert!(
        !response.contains(credential),
        "MCP must never expose the bearer credential"
    );
    let response = Json::parse(response.trim()).unwrap();
    assert_eq!(response.get("id").and_then(Json::as_u64), Some(id));
    response
}
fn tool(name: &str, arguments: Json) -> Json {
    Json::object([("name", Json::text(name)), ("arguments", arguments)])
}

#[test]
fn real_agent_bridge_creates_two_child_versions_with_scoped_credentials() {
    let root = std::path::PathBuf::from(format!("/private/tmp/mf-mcp-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    let source = root.join("source");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("note.txt"), "saved input\n").unwrap();
    let checkpoint = CheckpointRuntimeParameters {
        idle_interval: Some(Duration::from_millis(10)),
        maximum_uncheckpointed_bytes: Some(65_536),
        maximum_uncheckpointed_interval: Some(Duration::from_secs(60)),
    };
    let desktop = Arc::new(
        LiveDaemon::with_checkpoint_runtime(
            StartupSummary::from(&nothing_to_recover()),
            checkpoint,
        )
        .unwrap(),
    );
    let preview = desktop
        .preview_folder_import(source.to_str().unwrap())
        .unwrap();
    desktop
        .confirm_folder_import(
            source.to_str().unwrap(),
            root.join("source.mesh").to_str().unwrap(),
            preview
                .get("summary")
                .and_then(mesh_daemon::ipc::Json::as_text)
                .unwrap(),
        )
        .unwrap();
    let source = desktop.workspace_state().unwrap();
    let input = VersionInput {
        root: source.root.clone(),
        digest: source.digest,
        installation: source.installation,
        version: source.workspace_versions[0].operation(),
    };
    let mut runtime = Runtime::open(
        FleetStore::open(root.join("fleet.sqlite")).unwrap(),
        "objective",
    )
    .unwrap();
    runtime
        .record(
            "start",
            Command::Start {
                goal: "Delegate two workers".into(),
                limits: Limits {
                    lanes: 3,
                    concurrency: 3,
                    depth: 1,
                    retries: 1,
                },
            },
        )
        .unwrap();
    let allocation = root.join("allocations");
    fs::create_dir(&allocation).unwrap();
    fs::set_permissions(&allocation, fs::Permissions::from_mode(0o700)).unwrap();
    let service = Arc::new(
        FleetService::new(
            runtime,
            Arc::new(
                NativeLaneAllocator::open(
                    &allocation,
                    TrustedReviewers::default(),
                    checkpoint,
                    vec![],
                )
                .unwrap(),
            ),
            BTreeSet::from(["codex".into()]),
        )
        .unwrap(),
    );
    let lane = service
        .create_root("root", "Coordinate", "codex", &input)
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
    desktop.register_fleet(service.clone()).unwrap();
    let endpoint = root.join("ipc/daemon.sock");
    let server = IpcServer::bind(&endpoint)
        .unwrap()
        .spawn(desktop.clone())
        .unwrap();
    let mut child = Process::new(env!("CARGO_BIN_EXE_mesh-mcp"))
        .args(["--endpoint", endpoint.to_str().unwrap()])
        .env("MESH_FLEET_OBJECTIVE", "objective")
        .env("MESH_FLEET_CREDENTIAL", credential.transport_value())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let token = credential.transport_value();
    let initialized = call(
        &mut stdin,
        &mut stdout,
        1,
        "initialize",
        Json::object([("protocolVersion", Json::text("2025-06-18"))]),
        token,
    );
    assert!(initialized.get("result").is_some());
    let listed = call(
        &mut stdin,
        &mut stdout,
        2,
        "tools/list",
        Json::empty_object(),
        token,
    );
    assert_eq!(
        listed
            .get("result")
            .unwrap()
            .get("tools")
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let context = call(
        &mut stdin,
        &mut stdout,
        3,
        "tools/call",
        tool("mesh_fleet_context", Json::empty_object()),
        token,
    );
    let context = context
        .get("result")
        .unwrap()
        .get("structuredContent")
        .unwrap();
    assert_eq!(text(context, "lane"), lane);
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
    let mut first = None;
    for (id, request) in [(4, "a"), (5, "b"), (6, "a")] {
        let args = Json::object([
            ("request", Json::text(request)),
            ("goal", Json::text(format!("Work on {request}"))),
            ("provider", Json::text("codex")),
            ("version", version.clone()),
        ]);
        let response = call(
            &mut stdin,
            &mut stdout,
            id,
            "tools/call",
            tool("mesh_fleet_delegate", args),
            token,
        );
        let result = response.get("result").unwrap();
        assert_eq!(result.get("isError"), Some(&Json::Bool(false)));
        let result = result.get("structuredContent").unwrap();
        let working = std::path::PathBuf::from(text(result.get("workspace").unwrap(), "root"));
        assert_eq!(
            fs::read_to_string(working.join("note.txt")).unwrap(),
            "saved input\n"
        );
        if id == 4 {
            first = Some(result.clone());
        }
        if id == 6 {
            assert_eq!(Some(result), first.as_ref());
        }
    }
    let response = call(
        &mut stdin,
        &mut stdout,
        7,
        "tools/call",
        tool("mesh_fleet_children", Json::empty_object()),
        token,
    );
    assert_eq!(
        response
            .get("result")
            .unwrap()
            .get("structuredContent")
            .unwrap()
            .get("lanes")
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let denied = call(
        &mut stdin,
        &mut stdout,
        8,
        "tools/call",
        tool("review.approve", Json::empty_object()),
        token,
    );
    assert!(denied.get("error").is_some());
    assert_eq!(desktop.workspace_state().unwrap().root, source.root);
    service.revoke(&credential).unwrap();
    let revoked = call(
        &mut stdin,
        &mut stdout,
        9,
        "tools/call",
        tool("mesh_fleet_context", Json::empty_object()),
        token,
    );
    assert_eq!(
        revoked.get("result").unwrap().get("isError"),
        Some(&Json::Bool(true))
    );
    drop(stdin);
    assert!(child.wait().unwrap().success());
    server.shutdown();
}
