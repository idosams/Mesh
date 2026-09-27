//! Native Codex process adapter. Prompt and credentials never enter command-line arguments.
//!
//! This adapter reports process/protocol facts, not approval, publication or filesystem custody.
//! Restart reconciliation and process-tree cancellation remain host responsibilities.

use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use super::service::AgentCredential;
use crate::ipc::Json;

const MAX_EVENT_BYTES: usize = 1024 * 1024;

/// Installed native executables admitted by the host, never renderer or agent arguments.
#[derive(Clone, Debug)]
pub struct CodexAdapter {
    executable: PathBuf,
    bridge: PathBuf,
}

/// Bounded activity metadata. Provider message bodies and raw stderr are not retained here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodexObservation {
    /// Provider-assigned thread identity, when acknowledged.
    pub thread: Option<String>,
    /// Number of stdout events consumed, including ignored future event types.
    pub events: u64,
    /// Number of stderr lines consumed; these are not necessarily failures.
    pub stderr_lines: u64,
    /// Last recognized activity category, with no command text or file contents.
    pub activity: Option<String>,
    /// A completed turn was reported by the provider.
    pub turn_completed: bool,
    /// Explicit provider failure, malformed output or transport-reader failure.
    pub failed: bool,
    /// Both pipes closed. Process exit alone does not imply this.
    pub streams_closed: bool,
}
#[derive(Default)]
struct Output {
    observation: CodexObservation,
    closed: u8,
}

/// A currently owned OS process. Dropping it does not assert termination or release custody.
pub struct CodexProcess {
    child: Child,
    output: Arc<Mutex<Output>>,
    exit: Option<ExitStatus>,
}
impl std::fmt::Debug for CodexProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodexProcess")
            .field("pid", &self.child.id())
            .finish_non_exhaustive()
    }
}
impl CodexAdapter {
    /// Admit explicit absolute executable locations using native filesystem checks.
    pub fn new(executable: &Path, bridge: &Path) -> io::Result<Self> {
        Ok(Self {
            executable: executable_path(executable)?,
            bridge: executable_path(bridge)?,
        })
    }

    /// Start only after durable dispatch and native session/custody admission by the fleet host.
    pub(super) fn spawn(
        &self,
        root: &Path,
        endpoint: &Path,
        objective: &str,
        credential: &AgentCredential,
        goal: &str,
    ) -> io::Result<CodexProcess> {
        if !root.is_absolute() || !endpoint.is_absolute() || goal.is_empty() || goal.len() > 16_384
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid native provider launch",
            ));
        }
        let bridge = self
            .bridge
            .to_str()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid bridge path"))?;
        let endpoint = endpoint
            .to_str()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid endpoint path"))?;
        let mut command = Command::new(&self.executable);
        command
            .current_dir(root)
            .args([
                "exec",
                "--ignore-user-config",
                "--sandbox",
                "workspace-write",
                "--json",
                "--skip-git-repo-check",
                "--ephemeral",
                "--color",
                "never",
            ])
            .args(["-c", "approval_policy=\"never\""])
            .args([
                "-c",
                &format!("mcp_servers.mesh.command={}", Json::text(bridge).encode()),
            ])
            .args([
                "-c",
                &format!(
                    "mcp_servers.mesh.args={}",
                    Json::Array(vec![Json::text("--endpoint"), Json::text(endpoint)]).encode()
                ),
            ])
            .args([
                "-c",
                "mcp_servers.mesh.env_vars=[\"MESH_FLEET_OBJECTIVE\",\"MESH_FLEET_CREDENTIAL\"]",
            ])
            .args(["-c", "mcp_servers.mesh.required=true"])
            .args(["-c", "mcp_servers.mesh.tool_timeout_sec=600"])
            .args([
                "-c",
                "mcp_servers.mesh.default_tools_approval_mode=\"approve\"",
            ])
            .arg("-")
            .env("MESH_FLEET_OBJECTIVE", objective)
            .env("MESH_FLEET_CREDENTIAL", credential.transport_value())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn()?;
        let output = Arc::new(Mutex::new(Output::default()));
        // Drain both pipes before feeding input, so startup diagnostics cannot block the writer.
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let out = output.clone();
        thread::spawn(move || consume(stdout, out, false));
        let err = output.clone();
        thread::spawn(move || consume(stderr, err, true));
        let mut input = child.stdin.take().expect("piped stdin");
        let prompt = format!("Work only in this assigned Mesh lane. Call mesh_fleet_context before editing. Use mesh_fleet_checkpoint to save your result, then mesh_fleet_submit_review for a complete checkpoint. Never approve or publish shared state. Do not print environment credentials.\n\n{goal}\n");
        if input
            .write_all(prompt.as_bytes())
            .and_then(|()| input.flush())
            .is_err()
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "provider input was not accepted",
            ));
        }
        drop(input);
        Ok(CodexProcess {
            child,
            output,
            exit: None,
        })
    }
}
impl CodexProcess {
    /// Live process identity for native reconciliation metadata; not identity proof by itself.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }
    /// Poll actual process exit plus independently consumed protocol facts.
    pub fn poll(&mut self) -> io::Result<(CodexObservation, Option<bool>)> {
        if self.exit.is_none() {
            self.exit = self.child.try_wait()?;
        }
        let observation = self
            .output
            .lock()
            .map_err(|_| io::Error::other("provider observation unavailable"))?
            .observation
            .clone();
        let outcome = self
            .exit
            .filter(|_| observation.streams_closed)
            .map(|exit| exit.success() && observation.turn_completed && !observation.failed);
        Ok((observation, outcome))
    }
    /// Stop the directly owned process. This alone proves neither descendant exit nor custody release.
    pub fn request_stop(&mut self) -> io::Result<()> {
        self.child.kill()
    }
    pub(super) fn abort_direct(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn executable_path(path: &Path) -> io::Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt as _;
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "provider path must be absolute",
        ));
    }
    let path = path.canonicalize()?;
    let metadata = path.metadata()?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "provider path is not executable",
        ));
    }
    Ok(path)
}
fn consume(reader: impl io::Read, output: Arc<Mutex<Output>>, stderr: bool) {
    let mut reader = BufReader::new(reader);
    loop {
        // take() bounds allocation before read_until(), including malformed unterminated output.
        let mut line = Vec::new();
        let read = std::io::Read::take(&mut reader, (MAX_EVENT_BYTES + 1) as u64)
            .read_until(b'\n', &mut line);
        let mut state = output
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match read {
            Ok(0) => break,
            Err(_) => {
                state.observation.failed = true;
                break;
            }
            Ok(_) if line.len() > MAX_EVENT_BYTES => {
                state.observation.failed = true;
                break;
            }
            Ok(_) => {}
        }
        if stderr {
            state.observation.stderr_lines = state.observation.stderr_lines.saturating_add(1);
            continue;
        }
        state.observation.events = state.observation.events.saturating_add(1);
        let Some(event) = std::str::from_utf8(&line)
            .ok()
            .and_then(|line| Json::parse(line.trim()).ok())
        else {
            state.observation.failed = true;
            continue;
        };
        match event.get("type").and_then(Json::as_text) {
            Some("thread.started") => {
                if let Some(id) = event.get("thread_id").and_then(Json::as_text).filter(|id| {
                    id.len() == 36
                        && id.bytes().enumerate().all(|(index, b)| {
                            if [8, 13, 18, 23].contains(&index) {
                                b == b'-'
                            } else {
                                b.is_ascii_hexdigit()
                            }
                        })
                }) {
                    state.observation.thread = Some(id.into());
                } else {
                    state.observation.failed = true;
                }
            }
            Some("turn.completed") => {
                state.observation.turn_completed = true;
            }
            Some("turn.failed" | "error") => {
                state.observation.failed = true;
            }
            Some(kind @ ("turn.started" | "item.started" | "item.updated" | "item.completed")) => {
                state.observation.activity = Some(kind.into());
            }
            Some(_) => {} // Future event types do not grant success or change authority.
            None => {
                state.observation.failed = true;
            }
        }
    }
    let mut state = output
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    state.closed += 1;
    state.observation.streams_closed = state.closed == 2;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observe(stdout: &[u8], stderr: &[u8]) -> CodexObservation {
        let output = Arc::new(Mutex::new(Output::default()));
        consume(stdout, output.clone(), false);
        assert!(!output.lock().unwrap().observation.streams_closed);
        consume(stderr, output.clone(), true);
        let observation = output.lock().unwrap().observation.clone();
        assert!(observation.streams_closed);
        observation
    }

    #[test]
    fn provider_output_discards_bodies_and_rejects_failure_even_after_completion() {
        let output = observe(
            b"{\"type\":\"item.completed\",\"body\":\"sensitive-output\"}\n{\"type\":\"turn.completed\"}\n{\"type\":\"error\",\"message\":\"sensitive-output\"}\n",
            b"sensitive-output\n",
        );
        assert!(output.turn_completed);
        assert!(output.failed);
        assert_eq!(output.stderr_lines, 1);
        assert_eq!(output.events, 3);
        assert!(!format!("{output:?}").contains("sensitive-output"));
    }

    #[test]
    fn malformed_and_oversized_provider_events_fail_closed() {
        for input in [
            b"not-json\n".to_vec(),
            b"{\"type\":\"thread.started\",\"thread_id\":\"secret\"}\n".to_vec(),
            b"{\"body\":\"missing event type\"}\n".to_vec(),
            vec![b'x'; MAX_EVENT_BYTES + 1],
            vec![0xff, b'\n'],
        ] {
            let output = observe(&input, b"");
            assert!(output.failed);
            assert!(!output.turn_completed);
            assert!(output.thread.is_none());
        }
        assert!(observe(b"", &vec![b'x'; MAX_EVENT_BYTES + 1]).failed);
    }

    #[test]
    fn unknown_events_and_clean_eof_do_not_imply_completion() {
        let output = observe(b"{\"type\":\"future.event\",\"body\":\"discard\"}\n", b"");
        assert!(!output.failed);
        assert!(!output.turn_completed);
        assert!(output.activity.is_none());
        assert_eq!(output.events, 1);
        assert!(!observe(b"", b"").turn_completed);
    }
}
