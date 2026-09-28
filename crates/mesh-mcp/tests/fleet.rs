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

struct NativeSigner(ed25519_dalek::SigningKey);
impl mesh_daemon::fleet::service::CheckpointSigner for NativeSigner {
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
    bridge_journey(std::path::Path::new(env!("CARGO_BIN_EXE_mesh-mcp")), None);
}

#[test]
#[ignore = "requires exact packaged MESH_TEST_DESKTOP and MESH_TEST_DESKTOP_REVISION"]
fn packaged_desktop_bridge_delegates_and_reviews_attached_versions() {
    let executable = std::env::var_os("MESH_TEST_DESKTOP").expect("MESH_TEST_DESKTOP");
    let revision = std::env::var("MESH_TEST_DESKTOP_REVISION").expect("MESH_TEST_DESKTOP_REVISION");
    assert_eq!(revision.len(), 40);
    assert!(revision.bytes().all(|b| b.is_ascii_hexdigit()));
    for arguments in [
        vec!["--mesh-fleet-mcp"],
        vec!["--mesh-fleet-mcp", "--endpoint", "relative"],
        vec![
            "--mesh-fleet-mcp",
            "--endpoint",
            "/private/tmp/unused-fleet.sock",
        ],
        vec!["--mesh-fleet-mcp", "--credential", "sensitive-test-value"],
    ] {
        let output = Process::new(&executable)
            .args(arguments)
            .env_remove("MESH_FLEET_OBJECTIVE")
            .env("MESH_FLEET_CREDENTIAL", "sensitive-test-value")
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("sensitive-test-value"));
    }
    bridge_journey(std::path::Path::new(&executable), Some(&revision));
}

fn bridge_journey(executable: &std::path::Path, packaged_revision: Option<&str>) {
    let root = std::path::PathBuf::from(format!(
        "/private/tmp/mf-mcp-{}-{}",
        std::process::id(),
        if packaged_revision.is_some() {
            "packaged"
        } else {
            "standalone"
        }
    ));
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
    let lane = if packaged_revision.is_some() {
        use mesh_daemon::project_attachment::{AttachmentStorage, ObservationLimits};
        use mesh_daemon::CheckpointSigner as _;
        let metadata = root.join("attachments");
        fs::create_dir(&metadata).unwrap();
        let attached = AttachmentStorage::open(&metadata)
            .unwrap()
            .provision(&root.join("source"))
            .unwrap();
        let captured = attached
            .project()
            .capture_inputs(ObservationLimits::default())
            .unwrap();
        let signer = NativeSigner(ed25519_dalek::SigningKey::from_bytes(&[0x72; 32]));
        let version = attached
            .project()
            .save_capture(
                attached.metadata_path(),
                &captured,
                signer.public_key(),
                |payload| signer.sign(payload),
            )
            .unwrap()
            .operation();
        fs::write(root.join("source/note.txt"), "original work continues\n").unwrap();
        let lane = service
            .create_root_from_attachment("root", "Coordinate", "codex", &attached, version)
            .unwrap();
        assert_eq!(
            service.native_state().unwrap().lanes[&lane]
                .source_project
                .as_deref(),
            Some(attached.id())
        );
        lane
    } else {
        service
            .create_root("root", "Coordinate", "codex", &input)
            .unwrap()
    };
    service
        .native_command(
            "dispatch",
            Command::Dispatch {
                lane: lane.clone(),
                run: "run".into(),
            },
        )
        .unwrap();
    let credential = service
        .grant_with_signer(
            &lane,
            "run",
            "session",
            Arc::new(NativeSigner(ed25519_dalek::SigningKey::from_bytes(
                &[0x71; 32],
            ))),
        )
        .unwrap();
    desktop.register_fleet(service.clone()).unwrap();
    let endpoint = root.join("ipc/daemon.sock");
    let server = IpcServer::bind(&endpoint)
        .unwrap()
        .spawn(desktop.clone())
        .unwrap();
    let mut command = Process::new(executable);
    if packaged_revision.is_some() {
        command.arg("--mesh-fleet-mcp");
    }
    let mut child = command
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
    let tools = listed
        .get("result")
        .unwrap()
        .get("tools")
        .unwrap()
        .as_array()
        .unwrap();
    let names: BTreeSet<_> = tools.iter().map(|tool| text(tool, "name")).collect();
    assert_eq!(tools.len(), names.len());
    assert_eq!(
        names,
        BTreeSet::from([
            "mesh_fleet_context",
            "mesh_fleet_children",
            "mesh_fleet_delegate",
            "mesh_fleet_checkpoint",
            "mesh_fleet_submit_review",
            "mesh_fleet_propose_review_change_result",
        ])
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
    if let Some(revision) = packaged_revision {
        assert_eq!(
            context.get("mesh_desktop_build_revision"),
            Some(&Json::text(revision))
        );
        assert_eq!(
            context.get("mesh_desktop_build_exact"),
            Some(&Json::Bool(true))
        );
    }

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
    let working_root = std::path::PathBuf::from(text(context.get("workspace").unwrap(), "root"));
    fs::write(working_root.join("note.txt"), "saved through real MCP\n").unwrap();
    let args = Json::object([("request", Json::text("capture"))]);
    let captured = call(
        &mut stdin,
        &mut stdout,
        10,
        "tools/call",
        tool("mesh_fleet_checkpoint", args.clone()),
        token,
    );
    let captured = captured
        .get("result")
        .unwrap()
        .get("structuredContent")
        .unwrap()
        .clone();
    assert_eq!(captured.get("complete"), Some(&Json::Bool(true)));
    assert_eq!(
        captured.get("saved_changes").and_then(Json::as_u64),
        Some(1)
    );
    fs::write(working_root.join("note.txt"), "newer unsaved work\n").unwrap();
    let retried = call(
        &mut stdin,
        &mut stdout,
        11,
        "tools/call",
        tool("mesh_fleet_checkpoint", args),
        token,
    );
    assert_eq!(
        retried.get("result").unwrap().get("structuredContent"),
        Some(&captured)
    );
    let review_args = Json::object([("checkpoint", captured.get("checkpoint").unwrap().clone())]);
    let submitted = call(
        &mut stdin,
        &mut stdout,
        12,
        "tools/call",
        tool("mesh_fleet_submit_review", review_args.clone()),
        token,
    );
    let submitted = submitted
        .get("result")
        .unwrap()
        .get("structuredContent")
        .unwrap()
        .clone();
    assert_eq!(submitted.get("version"), captured.get("version"));
    assert_eq!(submitted.get("recorded"), Some(&Json::Bool(true)));
    let repeated = call(
        &mut stdin,
        &mut stdout,
        13,
        "tools/call",
        tool("mesh_fleet_submit_review", review_args),
        token,
    );
    assert_eq!(
        repeated.get("result").unwrap().get("structuredContent"),
        Some(&submitted)
    );
    assert_eq!(
        fs::read_to_string(working_root.join("note.txt")).unwrap(),
        "newer unsaved work\n"
    );
    assert_eq!(desktop.workspace_state().unwrap().root, source.root);
    let selection = mesh_daemon::fleet::service::SavedReviewSelection::new(
        text(context, "lane"),
        text(&captured, "checkpoint"),
        text(&captured, "version"),
        text(&submitted, "bundle"),
    )
    .unwrap();
    let feedback = service
        .request_review_changes("native-feedback", &selection, "Add the missing example.")
        .unwrap();
    let feedback_id = feedback.get("id").unwrap().as_text().unwrap();
    let observed = call(
        &mut stdin,
        &mut stdout,
        14,
        "tools/call",
        tool("mesh_fleet_context", Json::empty_object()),
        token,
    );
    let observed = observed
        .get("result")
        .unwrap()
        .get("structuredContent")
        .unwrap();
    assert_eq!(
        text(
            &observed
                .get("review_change_requests")
                .unwrap()
                .as_array()
                .unwrap()[0],
            "id"
        ),
        feedback_id
    );
    let revision = call(
        &mut stdin,
        &mut stdout,
        15,
        "tools/call",
        tool(
            "mesh_fleet_checkpoint",
            Json::object([("request", Json::text("feedback-result"))]),
        ),
        token,
    );
    let revision = revision
        .get("result")
        .unwrap()
        .get("structuredContent")
        .unwrap();
    let revised_review = call(
        &mut stdin,
        &mut stdout,
        16,
        "tools/call",
        tool(
            "mesh_fleet_submit_review",
            Json::object([("checkpoint", revision.get("checkpoint").unwrap().clone())]),
        ),
        token,
    );
    let revised_review = revised_review
        .get("result")
        .unwrap()
        .get("structuredContent")
        .unwrap();
    let proposal_args = Json::object([
        ("request", Json::text(feedback_id)),
        ("checkpoint", revision.get("checkpoint").unwrap().clone()),
    ]);
    let proposed = call(
        &mut stdin,
        &mut stdout,
        17,
        "tools/call",
        tool(
            "mesh_fleet_propose_review_change_result",
            proposal_args.clone(),
        ),
        token,
    );
    let proposed = proposed
        .get("result")
        .unwrap()
        .get("structuredContent")
        .unwrap();
    assert_eq!(text(proposed, "status"), "proposed");
    assert_eq!(proposed.get("version"), revision.get("version"));
    assert_eq!(proposed.get("bundle"), revised_review.get("bundle"));
    assert_eq!(proposed.get("approval_authority"), Some(&Json::Bool(false)));
    let retried = call(
        &mut stdin,
        &mut stdout,
        18,
        "tools/call",
        tool("mesh_fleet_propose_review_change_result", proposal_args),
        token,
    );
    assert_eq!(
        retried.get("result").unwrap().get("structuredContent"),
        Some(proposed)
    );
    let pinned = service.saved_review(&selection).unwrap();
    assert_eq!(
        pinned
            .get("selection")
            .unwrap()
            .get("version")
            .unwrap()
            .as_text(),
        Some(text(&captured, "version"))
    );
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
    if packaged_revision.is_some() {
        assert_eq!(
            fs::read(root.join("source/note.txt")).unwrap(),
            b"original work continues\n"
        );
        eprintln!("Packaged fleet MCP passed: attached source preserved, two child lanes, signed checkpoint, pinned review, feedback, proposed revision, exact retry and revoked-session refusal; graphical=false");
    }
    server.shutdown();
}
