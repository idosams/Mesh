#![cfg(unix)]
//! Real-process and Unix-socket coverage for the bounded MCP bridge.

use mesh_mcp::json::Json;
use mesh_mcp::{DaemonWorkspaceState, WorkspaceStateProvider};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{symlink, PermissionsExt};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        // Unix socket paths have a small platform-specific limit. `/tmp` exists on every target
        // exercised here (and canonicalizes to `/private/tmp` on macOS), while `/private/tmp`
        // itself is not portable to Linux and the process TMPDIR can be too long for SUN_LEN.
        let path =
            PathBuf::from("/tmp").join(format!("mcp-{label}-{}-{nonce}", std::process::id()));
        fs::create_dir(&path).expect("scratch directory");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).expect("owner-only scratch");
        Self(fs::canonicalize(path).expect("canonical scratch path"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fake_daemon(socket: &Path) -> thread::JoinHandle<()> {
    let listener = UnixListener::bind(socket).expect("bind fake daemon");
    thread::spawn(move || {
        let (stream, _) = listener.accept().expect("accept bridge");
        let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
        let mut writer = stream;
        let mut line = String::new();
        reader.read_line(&mut line).expect("read hello");
        assert_eq!(
            line,
            "{\"t\":\"hello\",\"id\":1,\"protocol\":\"mesh-ipc\",\"versions\":[1,2,3,4,5,6,7,8],\"session\":\"mesh-mcp\"}\n"
        );
        writer
            .write_all(b"{\"t\":\"welcome\",\"id\":1,\"version\":7,\"session\":\"mesh-mcp\",\"resumed\":false,\"surface_version\":7}\n")
            .expect("write welcome");
        line.clear();
        reader.read_line(&mut line).expect("read call");
        assert_eq!(
            line,
            "{\"t\":\"call\",\"id\":2,\"method\":\"workspace.state\",\"version\":7,\"params\":{}}\n"
        );
        let complete = format!(
            "{{\"t\":\"result\",\"id\":2,\"value\":{{\"root\":\"/native/current\",\"records\":9,\"native_untracked_files\":[\"agent.md\"],\"padding\":\"{}\"}}}}",
            "x".repeat(70_000)
        );
        let parts = complete.len().div_ceil(30_000);
        for (index, chunk) in complete.as_bytes().chunks(30_000).enumerate() {
            let hex: String = chunk.iter().map(|byte| format!("{byte:02x}")).collect();
            writeln!(
                writer,
                "{{\"t\":\"chunk\",\"id\":2,\"index\":{index},\"parts\":{parts},\"total_bytes\":{},\"hex\":\"{hex}\"}}",
                complete.len()
            )
            .expect("write state chunk");
        }
    })
}

#[test]
fn real_stdio_binary_calls_the_daemon_and_returns_structured_state() {
    let scratch = Scratch::new("stdio");
    let socket = scratch.0.join("daemon.sock");
    let daemon = fake_daemon(&socket);
    let mut child = Command::new(env!("CARGO_BIN_EXE_mesh-mcp"))
        .args(["--endpoint", socket.to_str().expect("socket path")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("launch mesh-mcp");
    let input = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-06-18\"}}\n",
        "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"mesh_workspace_state\",\"arguments\":{}}}\n"
    );
    child
        .stdin
        .take()
        .expect("child stdin")
        .write_all(input.as_bytes())
        .expect("write MCP exchange");
    let output = child.wait_with_output().expect("mesh-mcp output");
    daemon.join().expect("fake daemon");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let replies: Vec<Json> = String::from_utf8(output.stdout)
        .expect("utf8")
        .lines()
        .map(|line| Json::parse(line).expect("JSON-RPC response"))
        .collect();
    assert_eq!(replies.len(), 2);
    let result = replies[1].get("result").expect("result");
    let structured = result.get("structuredContent").expect("structured content");
    assert_eq!(
        structured.get("root").and_then(Json::as_text),
        Some("/native/current")
    );
    let native = structured
        .get("native_untracked_files")
        .and_then(Json::as_array)
        .expect("native files");
    assert_eq!(native[0].as_text(), Some("agent.md"));
    assert_eq!(
        structured
            .get("padding")
            .and_then(Json::as_text)
            .map(str::len),
        Some(70_000)
    );
}

#[test]
fn endpoint_symlink_is_refused_before_connect() {
    let scratch = Scratch::new("symlink");
    let socket = scratch.0.join("daemon.sock");
    let alias = scratch.0.join("alias.sock");
    let _listener = UnixListener::bind(&socket).expect("socket");
    symlink(&socket, &alias).expect("symlink");
    let problem = DaemonWorkspaceState::new(alias)
        .workspace_state()
        .expect_err("symlink must be rejected");
    assert!(problem.contains("real Unix socket"), "{problem}");
}

#[test]
fn fleet_session_refuses_an_older_daemon_before_sending_its_credential() {
    let scratch = Scratch::new("fleet-old");
    let socket = scratch.0.join("daemon.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let daemon = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut writer = stream;
        let mut hello = String::new();
        reader.read_line(&mut hello).unwrap();
        assert!(!hello.contains("credential"));
        writer.write_all(b"{\"t\":\"welcome\",\"id\":1,\"version\":7,\"session\":\"mesh-mcp\",\"resumed\":false,\"surface_version\":7}\n").unwrap();
        let mut next = String::new();
        assert_eq!(
            reader.read_line(&mut next).unwrap(),
            0,
            "no fleet command may be sent after a downgrade"
        );
    });
    let token = "ab".repeat(32);
    let provider = DaemonWorkspaceState::fleet(socket, "objective".into(), token.clone()).unwrap();
    assert!(!format!("{provider:?}").contains(&token));
    assert!(provider
        .fleet_call("context", &Json::empty_object())
        .is_err());
    daemon.join().unwrap();
}
