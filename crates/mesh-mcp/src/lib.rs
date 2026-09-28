//! A deliberately small MCP bridge to the local Mesh daemon.
//!
//! Default mode exposes the read-only `workspace.state` projection. Native-issued fleet mode
//! exposes bounded context, child delegation and observation. Neither mode grants approval or
//! publication authority; fleet credentials never appear in model tool arguments.

use json::Json;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

/// The bounded, dependency-free JSON representation shared by the two local wire protocols.
pub mod json;

/// Maximum accepted MCP input line, in bytes.
pub const MAX_LINE_BYTES: usize = 65_536;

/// Maximum reconstructed daemon message and emitted MCP response.
pub const MAX_RESPONSE_BYTES: usize = 17 * 1024 * 1024;

const MAX_DAEMON_MESSAGE_BYTES: usize = 16 * 1024 * 1024;
const DAEMON_CHUNK_DATA_BYTES: usize = 30_000;
#[cfg(unix)]
const DAEMON_WORKSPACE_STATE_REPLY_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(5 * 60);

/// The read-only tool published in default, unscoped mode.
pub const WORKSPACE_STATE_TOOL: &str = "mesh_workspace_state";

const SESSION_INSTRUCTIONS: &str = "Call mesh_workspace_state before editing. Work only in the \
native folder returned as structuredContent.root; this agent session stays pinned to that folder. \
If Mesh switches versions, keep working in this pinned folder and never follow Mesh's moving \
working-folder shortcut. Its read-only Mesh context will remain unavailable until the person \
reselects this exact agent folder. Start a separate agent from Mesh only to work on the newly \
selected version. When the work is complete and every related agent, terminal, and editor has \
stopped, ask the person to select this agent folder in Mesh, choose Finish agent handoff, inspect \
the changes, and choose Save all privately. This server cannot finish the handoff, save, switch versions, update the \
original folder, review, publish, or approve.";

const TOOL_DESCRIPTION: &str = "Verify the exact Mesh workspace pinned to this agent and return \
its native root, unsaved native changes, retained versions, review state, and unavailable \
capabilities. Call this before editing. Read-only: it cannot save, switch versions, update the \
original folder, review, publish, or approve.";

const RESULT_GUIDANCE: &str = "Mesh verified this agent's workspace in structuredContent. Work \
only in structuredContent.root. A later Mesh version switch does not move this folder. After \
editing and stopping every related process, ask the person to select this exact agent folder in \
Mesh, choose Finish agent handoff, inspect the changes, and choose Save all privately.";

const SUPPORTED_PROTOCOLS: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18"];

/// Supplies the read-only Mesh workspace projection.
pub trait WorkspaceStateProvider {
    /// Return the exact JSON object produced by `workspace.state`.
    fn workspace_state(&self) -> Result<Json, String>;
    /// Whether the native launcher supplied a scoped fleet session.
    fn fleet_enabled(&self) -> bool {
        false
    }
    /// Execute a bounded fleet action. Credentials are held by the bridge, never tool arguments.
    fn fleet_call(&self, _action: &str, _arguments: &Json) -> Result<Json, String> {
        Err("This Mesh bridge has no fleet session".into())
    }
}

/// The exact workspace identity an agent session was opened against.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpectedWorkspace {
    root: PathBuf,
    installation: String,
}

impl ExpectedWorkspace {
    /// Create a fail-closed binding for one version-pinned native workspace.
    #[must_use]
    pub fn new(root: PathBuf, installation: String) -> Self {
        Self { root, installation }
    }

    fn verify(&self, state: &Json) -> Result<(), String> {
        let root = state
            .get("root")
            .and_then(Json::as_text)
            .ok_or("Mesh workspace state omitted its root")?;
        let digest = state
            .get("digest")
            .and_then(Json::as_text)
            .ok_or("Mesh workspace state omitted its digest")?;
        if digest.is_empty() {
            return Err("Mesh workspace state returned an empty digest".to_owned());
        }
        let installation = state
            .get("installation")
            .and_then(Json::as_text)
            .ok_or("Mesh workspace state omitted its installation")?;
        if Path::new(root) != self.root {
            return Err(format!(
                "Mesh is now serving a different workspace. This agent remains safely pinned to {} and may keep working there; do not follow Mesh's moving working-folder shortcut. Ask the person to select this exact agent folder in Mesh before inspecting, saving, or using its context.",
                self.root.display()
            ));
        }
        if installation != self.installation {
            return Err(
                "The pinned Mesh workspace installation changed. Refresh Mesh and start a new agent session before using its context."
                    .to_owned(),
            );
        }
        Ok(())
    }
}

/// Serve newline-delimited JSON-RPC until EOF.
pub fn serve<R: BufRead, W: Write>(
    mut input: R,
    mut output: W,
    provider: &dyn WorkspaceStateProvider,
) -> Result<(), String> {
    let mut initialized = false;
    let mut line = Vec::new();
    loop {
        line.clear();
        let Some(overlong) = read_bounded_line(&mut input, &mut line)? else {
            return Ok(());
        };
        if overlong {
            write_response(
                &mut output,
                &rpc_error(Json::Null, -32600, "request exceeds the 65536-byte bound"),
            )?;
            continue;
        }
        if line.last() == Some(&b'\n') {
            line.pop();
        }
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        let request = match parse_line(&line) {
            Ok(value) => value,
            Err(problem) => {
                write_response(&mut output, &rpc_error(Json::Null, -32700, &problem))?;
                continue;
            }
        };
        let id = request.get("id").cloned();
        let response = dispatch(&request, &mut initialized, provider);
        if let Some(id) = id {
            write_response(
                &mut output,
                &response.unwrap_or_else(|| rpc_error(id, -32600, "invalid request")),
            )?;
        }
    }
}

fn read_bounded_line<R: BufRead>(
    input: &mut R,
    line: &mut Vec<u8>,
) -> Result<Option<bool>, String> {
    let mut overlong = false;
    let mut saw_bytes = false;
    loop {
        let available = input
            .fill_buf()
            .map_err(|error| format!("could not read MCP input: {error}"))?;
        if available.is_empty() {
            return Ok(saw_bytes.then_some(overlong));
        }
        saw_bytes = true;
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |position| position + 1);
        if !overlong {
            if line.len() + take > MAX_LINE_BYTES {
                overlong = true;
                line.clear();
            } else {
                line.extend_from_slice(&available[..take]);
            }
        }
        input.consume(take);
        if newline.is_some() {
            return Ok(Some(overlong));
        }
    }
}

fn parse_line(line: &[u8]) -> Result<Json, String> {
    let text = std::str::from_utf8(line).map_err(|_| "invalid UTF-8 in JSON request".to_owned())?;
    Json::parse(text).map_err(|error| format!("invalid JSON: {error}"))
}

fn dispatch(
    request: &Json,
    initialized: &mut bool,
    provider: &dyn WorkspaceStateProvider,
) -> Option<Json> {
    if !request.is_object() {
        return Some(rpc_error(Json::Null, -32600, "request must be an object"));
    }
    let id = request.get("id").cloned().unwrap_or(Json::Null);
    if !matches!(
        id,
        Json::Null | Json::Text(_) | Json::Number(_) | Json::Signed(_)
    ) {
        return Some(rpc_error(
            Json::Null,
            -32600,
            "id must be a string, number, or null",
        ));
    }
    if request.get("jsonrpc").and_then(Json::as_text) != Some("2.0") {
        return Some(rpc_error(id, -32600, "jsonrpc must be exactly 2.0"));
    }
    let Some(method) = request.get("method").and_then(Json::as_text) else {
        return Some(rpc_error(id, -32600, "method must be a string"));
    };
    if request.get("id").is_none() && method != "notifications/initialized" {
        return None;
    }
    match method {
        "initialize" => initialize(request, id, initialized, provider.fleet_enabled()),
        "notifications/initialized" => None,
        "ping" if *initialized => Some(rpc_result(id, Json::empty_object())),
        "tools/list" if *initialized => Some(rpc_result(
            id,
            Json::object([(
                "tools",
                Json::Array(if provider.fleet_enabled() {
                    fleet_tools()
                } else {
                    vec![tool_description()]
                }),
            )]),
        )),
        "tools/call" if *initialized => call_tool(request, id, provider),
        _ if !*initialized => Some(rpc_error(id, -32002, "server is not initialized")),
        _ => Some(rpc_error(id, -32601, "method not found")),
    }
}

fn initialize(request: &Json, id: Json, initialized: &mut bool, fleet: bool) -> Option<Json> {
    if *initialized {
        return Some(rpc_error(id, -32600, "server is already initialized"));
    }
    let requested = request
        .get("params")
        .and_then(|params| params.get("protocolVersion"))
        .and_then(Json::as_text);
    let Some(protocol) = requested.filter(|value| SUPPORTED_PROTOCOLS.contains(value)) else {
        return Some(rpc_error(id, -32602, "unsupported MCP protocol version"));
    };
    *initialized = true;
    Some(rpc_result(
        id,
        Json::object([
            ("protocolVersion", Json::text(protocol)),
            (
                "capabilities",
                Json::object([("tools", Json::object([("listChanged", Json::Bool(false))]))]),
            ),
            (
                "serverInfo",
                Json::object([
                    ("name", Json::text("mesh-mcp")),
                    ("version", Json::text(env!("CARGO_PKG_VERSION"))),
                ]),
            ),
            (
                "instructions",
                Json::text(if fleet {
                    FLEET_INSTRUCTIONS
                } else {
                    SESSION_INSTRUCTIONS
                }),
            ),
        ]),
    ))
}

fn call_tool(request: &Json, id: Json, provider: &dyn WorkspaceStateProvider) -> Option<Json> {
    let params = request.get("params");
    let name = params
        .and_then(|params| params.get("name"))
        .and_then(Json::as_text);
    if provider.fleet_enabled() {
        return Some(fleet_tool_call(
            id,
            name,
            params.and_then(|p| p.get("arguments")),
            provider,
        ));
    }
    if name != Some(WORKSPACE_STATE_TOOL) {
        return Some(rpc_error(id, -32602, "unknown Mesh tool"));
    }
    let arguments = params.and_then(|params| params.get("arguments"));
    if arguments.is_some_and(|value| !matches!(value, Json::Object(fields) if fields.is_empty())) {
        return Some(rpc_error(
            id,
            -32602,
            "mesh_workspace_state takes no arguments",
        ));
    }
    let result = match provider.workspace_state() {
        Ok(state) => Json::object([
            (
                "content",
                Json::Array(vec![Json::object([
                    ("type", Json::text("text")),
                    ("text", Json::text(RESULT_GUIDANCE)),
                ])]),
            ),
            ("structuredContent", state),
            ("isError", Json::Bool(false)),
        ]),
        Err(problem) => Json::object([
            (
                "content",
                Json::Array(vec![Json::object([
                    ("type", Json::text("text")),
                    ("text", Json::text(problem)),
                ])]),
            ),
            ("isError", Json::Bool(true)),
        ]),
    };
    Some(rpc_result(id, result))
}

const FLEET_INSTRUCTIONS: &str = "Call mesh_fleet_context first. It identifies your exact lane, run and native working folder. Its review_change_requests are recorded feedback for exact earlier results in your lane; check context again before submitting work. A recorded request does not prove it has been addressed. Work only there. Use mesh_fleet_checkpoint to save supported private edits and additions; incomplete results require attention and do not mean the whole folder was saved. Use a stable request identity for retries: it returns the same result even after later edits. A new capture needs a new request. Submit a complete checkpoint for immutable review with mesh_fleet_submit_review using its returned checkpoint identity. Submission does not approve or publish it. You can delegate private child lanes from one of your saved workspace_versions with mesh_fleet_delegate, then observe them with mesh_fleet_children. Use a stable request identity when retrying delegation. This session cannot approve shared state, choose output paths, or grant authority to another lane.";

fn fleet_tools() -> Vec<Json> {
    [
        (
            "mesh_fleet_context",
            "Inspect this agent's bound lane and saved versions",
            &[][..],
        ),
        (
            "mesh_fleet_children",
            "Observe this lane's direct children",
            &[][..],
        ),
        (
            "mesh_fleet_delegate",
            "Create an isolated child lane from an exact saved version",
            &["request", "goal", "provider", "version"][..],
        ),
        (
            "mesh_fleet_checkpoint",
            "Save private workspace edits and additions with a stable request identity",
            &["request"][..],
        ),
        (
            "mesh_fleet_submit_review",
            "Submit a completed checkpoint as an immutable review without approving it",
            &["checkpoint"][..],
        ),
    ]
    .into_iter()
    .map(|(name, description, fields)| {
        Json::object([
            ("name", Json::text(name)),
            ("description", Json::text(description)),
            (
                "inputSchema",
                Json::object([
                    ("type", Json::text("object")),
                    (
                        "properties",
                        Json::object(
                            fields
                                .iter()
                                .map(|key| (*key, Json::object([("type", Json::text("string"))]))),
                        ),
                    ),
                    (
                        "required",
                        Json::Array(fields.iter().map(|key| Json::text(*key)).collect()),
                    ),
                    ("additionalProperties", Json::Bool(false)),
                ]),
            ),
            (
                "outputSchema",
                Json::object([("type", Json::text("object"))]),
            ),
            (
                "annotations",
                Json::object([
                    ("readOnlyHint", Json::Bool(fields.is_empty())),
                    ("destructiveHint", Json::Bool(false)),
                    ("idempotentHint", Json::Bool(true)),
                    ("openWorldHint", Json::Bool(false)),
                ]),
            ),
        ])
    })
    .collect()
}
fn fleet_tool_call(
    id: Json,
    name: Option<&str>,
    arguments: Option<&Json>,
    provider: &dyn WorkspaceStateProvider,
) -> Json {
    let (action, expected): (&str, &[&str]) = match name {
        Some("mesh_fleet_context") => ("context", &[]),
        Some("mesh_fleet_children") => ("children", &[]),
        Some("mesh_fleet_delegate") => ("delegate", &["request", "goal", "provider", "version"]),
        Some("mesh_fleet_checkpoint") => ("checkpoint", &["request"]),
        Some("mesh_fleet_submit_review") => ("submit_review", &["checkpoint"]),
        _ => return rpc_error(id, -32602, "unknown Mesh fleet tool"),
    };
    let empty = Json::empty_object();
    let arguments = arguments.unwrap_or(&empty);
    let Json::Object(fields) = arguments else {
        return rpc_error(id, -32602, "invalid fleet arguments");
    };
    if fields.len() != expected.len()
        || fields
            .iter()
            .any(|(key, value)| !expected.contains(&key.as_str()) || value.as_text().is_none())
    {
        return rpc_error(id, -32602, "invalid fleet arguments");
    }
    let result = match provider.fleet_call(action, arguments) {
        Ok(value) => Json::object([
            (
                "content",
                Json::Array(vec![Json::object([
                    ("type", Json::text("text")),
                    (
                        "text",
                        Json::text("Mesh verified this fleet action within your lane session."),
                    ),
                ])]),
            ),
            ("structuredContent", value),
            ("isError", Json::Bool(false)),
        ]),
        Err(problem) => Json::object([
            (
                "content",
                Json::Array(vec![Json::object([
                    ("type", Json::text("text")),
                    ("text", Json::text(problem)),
                ])]),
            ),
            ("isError", Json::Bool(true)),
        ]),
    };
    rpc_result(id, result)
}

fn tool_description() -> Json {
    Json::object([
        ("name", Json::text(WORKSPACE_STATE_TOOL)),
        ("title", Json::text("Inspect the open Mesh workspace")),
        ("description", Json::text(TOOL_DESCRIPTION)),
        (
            "inputSchema",
            Json::object([
                ("type", Json::text("object")),
                ("properties", Json::empty_object()),
                ("additionalProperties", Json::Bool(false)),
            ]),
        ),
        (
            "outputSchema",
            Json::object([("type", Json::text("object"))]),
        ),
        (
            "annotations",
            Json::object([
                ("readOnlyHint", Json::Bool(true)),
                ("destructiveHint", Json::Bool(false)),
                ("idempotentHint", Json::Bool(true)),
                ("openWorldHint", Json::Bool(false)),
            ]),
        ),
    ])
}

fn rpc_result(id: Json, result: Json) -> Json {
    Json::object([
        ("jsonrpc", Json::text("2.0")),
        ("id", id),
        ("result", result),
    ])
}

fn rpc_error(id: Json, code: i64, message: &str) -> Json {
    Json::object([
        ("jsonrpc", Json::text("2.0")),
        ("id", id),
        (
            "error",
            Json::object([
                ("code", Json::Signed(code)),
                ("message", Json::text(message)),
            ]),
        ),
    ])
}

fn write_response(output: &mut dyn Write, response: &Json) -> Result<(), String> {
    let encoded = response.encode();
    if encoded.len() + 1 > MAX_RESPONSE_BYTES {
        return Err(response_bound_error());
    }
    output
        .write_all(encoded.as_bytes())
        .and_then(|()| output.write_all(b"\n"))
        .and_then(|()| output.flush())
        .map_err(|error| format!("could not write MCP response: {error}"))
}

fn response_bound_error() -> String {
    format!("MCP response exceeds the {MAX_RESPONSE_BYTES}-byte bound")
}

/// A provider that reconnects to the local Mesh daemon for every tool call.
#[derive(Clone)]
pub struct DaemonWorkspaceState {
    endpoint: PathBuf,
    expected: Option<ExpectedWorkspace>,
    fleet: Option<FleetSession>,
}

#[derive(Clone)]
struct FleetSession {
    objective: String,
    credential: String,
}
impl std::fmt::Debug for DaemonWorkspaceState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonWorkspaceState")
            .field("endpoint", &self.endpoint)
            .field("expected", &self.expected)
            .field("fleet_session", &self.fleet.is_some())
            .finish()
    }
}

impl DaemonWorkspaceState {
    /// Construct a scoped bridge using native launcher configuration, never model tool arguments.
    pub fn fleet(endpoint: PathBuf, objective: String, credential: String) -> Result<Self, String> {
        if objective.is_empty()
            || objective.len() > 128
            || !objective
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
            || credential.len() != 64
            || !credential.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err("Invalid native fleet session configuration".into());
        }
        Ok(Self {
            endpoint,
            expected: None,
            fleet: Some(FleetSession {
                objective,
                credential,
            }),
        })
    }

    /// Bind the provider to one local Unix socket path.
    #[must_use]
    pub fn new(endpoint: PathBuf) -> Self {
        Self {
            endpoint,
            expected: None,
            fleet: None,
        }
    }

    /// Bind every response to the version identity used to start this agent.
    #[must_use]
    pub fn bound(endpoint: PathBuf, expected: ExpectedWorkspace) -> Self {
        Self {
            endpoint,
            expected: Some(expected),
            fleet: None,
        }
    }
}

#[cfg(unix)]
impl DaemonWorkspaceState {
    fn daemon_call(
        &self,
        method: &str,
        params: &Json,
        minimum_version: u64,
    ) -> Result<Json, String> {
        use std::io::{BufReader, Read};
        use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
        use std::os::unix::net::UnixStream;
        use std::time::Duration;

        fn validate_endpoint(path: &std::path::Path) -> Result<(), String> {
            if !path.is_absolute() {
                return Err("the Mesh daemon endpoint must be absolute".to_owned());
            }
            let parent = path
                .parent()
                .ok_or("the Mesh daemon endpoint has no parent")?;
            let canonical = std::fs::canonicalize(parent)
                .map_err(|error| format!("could not verify the Mesh endpoint parent: {error}"))?;
            if canonical != parent {
                return Err(
                    "the Mesh endpoint parent must be a real directory, not a path alias"
                        .to_owned(),
                );
            }
            let parent_metadata = std::fs::symlink_metadata(parent)
                .map_err(|error| format!("could not inspect the Mesh endpoint parent: {error}"))?;
            if !parent_metadata.is_dir() || parent_metadata.permissions().mode() & 0o077 != 0 {
                return Err(
                    "the Mesh endpoint parent must be an owner-only real directory".to_owned(),
                );
            }
            let endpoint_metadata = std::fs::symlink_metadata(path)
                .map_err(|error| format!("could not inspect the Mesh endpoint: {error}"))?;
            if !endpoint_metadata.file_type().is_socket() {
                return Err("the Mesh endpoint must be a real Unix socket".to_owned());
            }
            if endpoint_metadata.uid() != parent_metadata.uid() {
                return Err("the Mesh endpoint and its parent have different owners".to_owned());
            }
            Ok(())
        }

        fn read_daemon_frame(reader: &mut BufReader<UnixStream>) -> Result<Json, String> {
            let mut bytes = Vec::new();
            reader
                .take(MAX_LINE_BYTES as u64)
                .read_until(b'\n', &mut bytes)
                .map_err(|error| format!("could not read the Mesh daemon response: {error}"))?;
            if bytes.is_empty() || bytes.len() >= MAX_LINE_BYTES || bytes.last() != Some(&b'\n') {
                return Err("Mesh returned an empty, overlong, or unterminated response".to_owned());
            }
            bytes.pop();
            parse_line(&bytes)
        }

        fn read_daemon_message(reader: &mut BufReader<UnixStream>) -> Result<Json, String> {
            let first = read_daemon_frame(reader)?;
            if first.get("t").and_then(Json::as_text) != Some("chunk") {
                return Ok(first);
            }
            let id = first
                .get("id")
                .and_then(Json::as_u64)
                .ok_or("Mesh chunk omitted id")?;
            let parts = first
                .get("parts")
                .and_then(Json::as_u64)
                .ok_or("Mesh chunk omitted parts")?;
            let total = first
                .get("total_bytes")
                .and_then(Json::as_u64)
                .ok_or("Mesh chunk omitted total_bytes")?;
            let total = usize::try_from(total).map_err(|_| "Mesh chunk total is too large")?;
            if parts == 0 || !(MAX_LINE_BYTES..=MAX_DAEMON_MESSAGE_BYTES).contains(&total) {
                return Err("Mesh returned invalid chunk bounds".to_owned());
            }
            let mut output = Vec::with_capacity(total);
            let mut frame = first;
            for expected in 0..parts {
                if expected != 0 {
                    frame = read_daemon_frame(reader)?;
                }
                if frame.get("t").and_then(Json::as_text) != Some("chunk")
                    || frame.get("id").and_then(Json::as_u64) != Some(id)
                    || frame.get("index").and_then(Json::as_u64) != Some(expected)
                    || frame.get("parts").and_then(Json::as_u64) != Some(parts)
                    || frame.get("total_bytes").and_then(Json::as_u64) != u64::try_from(total).ok()
                {
                    return Err(
                        "Mesh returned inconsistent or reordered response chunks".to_owned()
                    );
                }
                let hex = frame
                    .get("hex")
                    .and_then(Json::as_text)
                    .ok_or("Mesh chunk omitted hex")?;
                if hex.is_empty() || hex.len() % 2 != 0 || hex.len() > DAEMON_CHUNK_DATA_BYTES * 2 {
                    return Err("Mesh returned an invalid chunk payload length".to_owned());
                }
                for pair in hex.as_bytes().chunks_exact(2) {
                    let high = daemon_hex(pair[0]).ok_or("Mesh chunk hex is not lower-case")?;
                    let low = daemon_hex(pair[1]).ok_or("Mesh chunk hex is not lower-case")?;
                    output.push((high << 4) | low);
                }
                if output.len() > total {
                    return Err("Mesh chunks exceeded their declared size".to_owned());
                }
            }
            if output.len() != total {
                return Err("Mesh chunks did not reach their declared size".to_owned());
            }
            let complete = parse_line(&output)?;
            if complete.get("t").and_then(Json::as_text) == Some("chunk")
                || complete.get("id").and_then(Json::as_u64) != Some(id)
            {
                return Err("Mesh reconstructed a response with the wrong identity".to_owned());
            }
            Ok(complete)
        }

        fn daemon_hex(byte: u8) -> Option<u8> {
            match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                _ => None,
            }
        }

        validate_endpoint(&self.endpoint)?;
        let stream = UnixStream::connect(&self.endpoint).map_err(|error| {
            format!(
                "Mesh is not reachable at {}: {error}",
                self.endpoint.display()
            )
        })?;
        stream
            .set_read_timeout(Some(DAEMON_WORKSPACE_STATE_REPLY_TIMEOUT))
            .map_err(|error| format!("could not bound the Mesh daemon reply: {error}"))?;
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .map_err(|error| format!("could not bound the Mesh daemon request: {error}"))?;
        let mut writer = stream
            .try_clone()
            .map_err(|error| format!("could not use the Mesh daemon connection: {error}"))?;
        let mut reader = BufReader::new(stream);
        writeln!(writer, "{{\"t\":\"hello\",\"id\":1,\"protocol\":\"mesh-ipc\",\"versions\":[1,2,3,4,5,6,7,8],\"session\":\"mesh-mcp\"}}")
            .and_then(|()| writer.flush())
            .map_err(|error| format!("could not greet the Mesh daemon: {error}"))?;
        let welcome = read_daemon_message(&mut reader)?;
        if welcome.get("t").and_then(Json::as_text) != Some("welcome")
            || welcome.get("id").and_then(Json::as_u64) != Some(1)
            || welcome.get("session").and_then(Json::as_text) != Some("mesh-mcp")
        {
            return Err("Mesh refused the local MCP session".to_owned());
        }
        let version = welcome
            .get("version")
            .and_then(Json::as_u64)
            .filter(|version| (minimum_version..=8).contains(version))
            .ok_or("Mesh negotiated no IPC version supporting the requested tool")?;
        if welcome
            .get("surface_version")
            .and_then(Json::as_u64)
            .is_none_or(|surface| surface < version)
        {
            return Err("Mesh returned an inconsistent IPC surface version".to_owned());
        }
        let call = Json::object([
            ("t", Json::text("call")),
            ("id", Json::Number(2)),
            ("method", Json::text(method)),
            ("version", Json::Number(version)),
            ("params", params.clone()),
        ]);
        writeln!(writer, "{}", call.encode())
            .and_then(|()| writer.flush())
            .map_err(|error| format!("could not request Mesh workspace state: {error}"))?;
        let answer = read_daemon_message(&mut reader)?;
        if answer.get("id").and_then(Json::as_u64) != Some(2) {
            return Err("Mesh returned a response with the wrong correlation id".to_owned());
        }
        match answer.get("t").and_then(Json::as_text) {
            Some("result") => {
                let state = answer
                    .get("value")
                    .filter(|value| value.is_object())
                    .cloned()
                    .ok_or_else(|| {
                        "Mesh returned workspace state in an invalid shape".to_owned()
                    })?;
                if method == "workspace.state" {
                    if let Some(expected) = &self.expected {
                        expected.verify(&state)?;
                    }
                }
                Ok(state)
            }
            Some("failed") => Err(format!(
                "Mesh could not inspect the workspace: {}",
                answer
                    .get("message")
                    .and_then(Json::as_text)
                    .unwrap_or("unknown refusal")
            )),
            _ => Err("Mesh returned an unexpected workspace.state response".to_owned()),
        }
    }
}

#[cfg(not(unix))]
impl DaemonWorkspaceState {
    fn daemon_call(
        &self,
        _method: &str,
        _params: &Json,
        _minimum_version: u64,
    ) -> Result<Json, String> {
        Err("the alpha Mesh MCP bridge currently requires a Unix local socket".to_owned())
    }
}

impl WorkspaceStateProvider for DaemonWorkspaceState {
    fn workspace_state(&self) -> Result<Json, String> {
        if self.fleet.is_some() {
            self.fleet_call("context", &Json::empty_object())?
                .get("workspace")
                .cloned()
                .ok_or_else(|| "Fleet context omitted its workspace".into())
        } else {
            self.daemon_call("workspace.state", &Json::empty_object(), 2)
        }
    }
    fn fleet_enabled(&self) -> bool {
        self.fleet.is_some()
    }
    fn fleet_call(&self, action: &str, arguments: &Json) -> Result<Json, String> {
        let session = self
            .fleet
            .as_ref()
            .ok_or("This Mesh bridge has no fleet session")?;
        self.daemon_call(
            "fleet.agent.call",
            &Json::object([
                ("objective", Json::text(&session.objective)),
                ("credential", Json::text(&session.credential)),
                ("action", Json::text(action)),
                ("arguments", arguments.clone()),
            ]),
            8,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    struct State(Result<Json, String>);

    impl WorkspaceStateProvider for State {
        fn workspace_state(&self) -> Result<Json, String> {
            self.0.clone()
        }
    }

    fn exchange(lines: &[&str], provider: &dyn WorkspaceStateProvider) -> Vec<Json> {
        let input = lines.join("\n") + "\n";
        let mut output = Vec::new();
        serve(Cursor::new(input), &mut output, provider).expect("serve");
        String::from_utf8(output)
            .expect("utf8")
            .lines()
            .map(|line| Json::parse(line).expect("response"))
            .collect()
    }

    fn field<'a>(value: &'a Json, path: &[&str]) -> &'a Json {
        path.iter()
            .fold(value, |current, name| current.get(name).expect("field"))
    }

    #[test]
    fn expected_workspace_binding_follows_saves_but_refuses_root_or_installation_drift() {
        let expected = ExpectedWorkspace::new(
            PathBuf::from("/native/version-one"),
            "installation-one".to_owned(),
        );
        let exact = Json::object([
            ("root", Json::text("/native/version-one")),
            ("digest", Json::text("digest-one")),
            ("installation", Json::text("installation-one")),
        ]);
        expected.verify(&exact).expect("exact binding");

        let advanced = Json::object([
            ("root", Json::text("/native/version-one")),
            ("digest", Json::text("digest-two")),
            ("installation", Json::text("installation-one")),
        ]);
        expected
            .verify(&advanced)
            .expect("a durable save in the same physical workspace remains current");

        for (name, value, expected_text) in [
            (
                "root",
                "/native/version-two",
                "remains safely pinned to /native/version-one and may keep working there",
            ),
            ("installation", "installation-two", "installation changed"),
        ] {
            let mut fields = [
                ("root", "/native/version-one"),
                ("digest", "digest-one"),
                ("installation", "installation-one"),
            ];
            fields
                .iter_mut()
                .find(|(field, _)| *field == name)
                .expect("field")
                .1 = value;
            let state = Json::object(fields.map(|(field, value)| (field, Json::text(value))));
            let problem = expected.verify(&state).expect_err("drift must refuse");
            assert!(problem.contains(expected_text), "{problem}");
        }
    }

    #[test]
    fn initializes_lists_and_returns_structured_read_only_state() {
        let replies = exchange(
            &[
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
                r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"mesh_workspace_state","arguments":{}}}"#,
            ],
            &State(Ok(Json::object([
                ("root", Json::text("/workspace")),
                (
                    "native_untracked_files",
                    Json::Array(vec![Json::text("note.md")]),
                ),
            ]))),
        );
        assert_eq!(replies.len(), 3);
        assert_eq!(
            field(&replies[0], &["result", "protocolVersion"]).as_text(),
            Some("2025-06-18")
        );
        let instructions = field(&replies[0], &["result", "instructions"])
            .as_text()
            .expect("agent instructions");
        for required in [
            "Call mesh_workspace_state before editing",
            "structuredContent.root",
            "keep working in this pinned folder",
            "never follow Mesh's moving working-folder shortcut",
            "Start a separate agent from Mesh only",
            "select this agent folder in Mesh",
            "choose Finish agent handoff",
            "Save all privately",
            "cannot finish the handoff, save, switch versions, update the original folder, review, publish, or approve",
        ] {
            assert!(
                instructions.contains(required),
                "missing {required}: {instructions}"
            );
        }
        assert!(!instructions.contains("adopt"), "obsolete internal wording");
        assert!(
            !instructions.contains("checkpoint"),
            "obsolete internal wording"
        );
        let tools = field(&replies[1], &["result", "tools"])
            .as_array()
            .expect("tools");
        assert_eq!(
            tools[0].get("name").and_then(Json::as_text),
            Some(WORKSPACE_STATE_TOOL)
        );
        assert_eq!(
            tools[0]
                .get("annotations")
                .and_then(|value| value.get("readOnlyHint"))
                .and_then(Json::as_bool),
            Some(true)
        );
        let description = tools[0]
            .get("description")
            .and_then(Json::as_text)
            .expect("tool description");
        assert!(description.contains("exact Mesh workspace pinned to this agent"));
        assert!(description.contains("Call this before editing"));
        assert!(description.contains("cannot save, switch versions"));
        assert_eq!(
            field(&replies[2], &["result", "structuredContent", "root"]).as_text(),
            Some("/workspace")
        );
        assert_eq!(
            field(&replies[2], &["result", "isError"]).as_bool(),
            Some(false)
        );
        let guidance = field(&replies[2], &["result", "content"])
            .as_array()
            .expect("content")[0]
            .get("text")
            .and_then(Json::as_text)
            .expect("result guidance");
        assert!(guidance.contains("Work only in structuredContent.root"));
        assert!(guidance.contains("does not move this folder"));
        assert!(guidance.contains("select this exact agent folder in Mesh"));
        assert!(guidance.contains("choose Finish agent handoff"));
        assert!(guidance.contains("Save all privately"));
    }

    #[test]
    fn refuses_duplicate_keys_and_preinitialize_calls() {
        let replies = exchange(
            &[
                r#"{"jsonrpc":"2.0","jsonrpc":"2.0","id":1,"method":"initialize"}"#,
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
            ],
            &State(Ok(Json::empty_object())),
        );
        assert_eq!(
            field(&replies[0], &["error", "code"]),
            &Json::Signed(-32700)
        );
        assert_eq!(
            field(&replies[1], &["error", "code"]),
            &Json::Signed(-32002)
        );
    }

    #[test]
    fn notification_cannot_initialize_or_execute_a_tool() {
        let replies = exchange(
            &[
                r#"{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
                r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"mesh_workspace_state","arguments":{}}}"#,
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#,
            ],
            &State(Ok(Json::object([("root", Json::text("/workspace"))]))),
        );
        assert_eq!(replies.len(), 1);
        assert_eq!(
            field(&replies[0], &["error", "code"]),
            &Json::Signed(-32002)
        );
    }

    #[test]
    fn overlong_input_is_drained_without_losing_the_next_request() {
        let mut input = vec![b'x'; MAX_LINE_BYTES + 1_024];
        input.push(b'\n');
        input.extend_from_slice(
            br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
        );
        input.push(b'\n');
        let mut output = Vec::new();
        serve(
            Cursor::new(input),
            &mut output,
            &State(Ok(Json::empty_object())),
        )
        .expect("serve");
        let replies: Vec<Json> = String::from_utf8(output)
            .expect("utf8")
            .lines()
            .map(|line| Json::parse(line).expect("response"))
            .collect();
        assert_eq!(replies.len(), 2);
        assert_eq!(
            field(&replies[0], &["error", "code"]),
            &Json::Signed(-32600)
        );
        assert_eq!(
            field(&replies[1], &["result", "protocolVersion"]).as_text(),
            Some("2025-06-18")
        );
    }

    #[test]
    fn daemon_failure_is_a_visible_tool_error() {
        let replies = exchange(
            &[
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#,
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"mesh_workspace_state","arguments":{}}}"#,
            ],
            &State(Err("Mesh is not running".to_owned())),
        );
        assert_eq!(
            field(&replies[1], &["result", "isError"]).as_bool(),
            Some(true)
        );
        let content = field(&replies[1], &["result", "content"])
            .as_array()
            .expect("content");
        assert_eq!(
            content[0].get("text").and_then(Json::as_text),
            Some("Mesh is not running")
        );
    }

    #[test]
    fn response_refusal_names_the_actual_enforced_bound() {
        assert_eq!(
            response_bound_error(),
            format!("MCP response exceeds the {MAX_RESPONSE_BYTES}-byte bound")
        );
        assert_ne!(MAX_RESPONSE_BYTES, 131_072);
    }
}
