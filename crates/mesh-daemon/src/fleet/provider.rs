//! Native provider process adapters. Prompt and credentials never enter command-line arguments.
//!
//! This adapter reports process/protocol facts, not approval, publication or filesystem custody.
//! Restart reconciliation and process-tree cancellation remain host responsibilities.

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::process::CommandExt as _;
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
    desktop_bridge: bool,
}

/// Redacted stream protocols shared by native provider adapters.
pub mod protocol;
/// Compatibility name for existing Codex host consumers.
pub use protocol::ProviderObservation as CodexObservation;
use protocol::ProviderProtocol;
mod claude;
pub use claude::ClaudeAdapter;

/// Explicitly admitted native executable configuration. Never deserialized from IPC.
#[derive(Clone, Debug)]
pub enum NativeAdapter {
    /// Codex exec integration.
    Codex(CodexAdapter),
    /// Claude Code print integration.
    Claude(ClaudeAdapter),
}
impl From<CodexAdapter> for NativeAdapter {
    fn from(adapter: CodexAdapter) -> Self {
        Self::Codex(adapter)
    }
}
impl From<ClaudeAdapter> for NativeAdapter {
    fn from(adapter: ClaudeAdapter) -> Self {
        Self::Claude(adapter)
    }
}
impl NativeAdapter {
    /// Stable provider identity used to match a durably allocated lane before claiming launch.
    pub fn provider(&self) -> &'static str {
        match self {
            Self::Codex(_) => "codex",
            Self::Claude(_) => "claude",
        }
    }
    pub(super) fn spawn(
        &self,
        root: &Path,
        endpoint: &Path,
        objective: &str,
        credential: &AgentCredential,
        goal: &str,
    ) -> io::Result<NativeProcess> {
        match self {
            Self::Codex(adapter) => adapter.spawn(root, endpoint, objective, credential, goal),
            Self::Claude(adapter) => adapter.spawn(root, endpoint, objective, credential, goal),
        }
    }
}
/// Compatibility name for callers of the original Codex adapter.
pub type CodexProcess = NativeProcess;

#[derive(Default)]
struct Output {
    observation: CodexObservation,
    closed: u8,
}

/// A currently owned OS process. Dropping it does not assert termination or release custody.
pub struct NativeProcess {
    child: Child,
    output: Arc<Mutex<Output>>,
    exit: Option<ExitStatus>,
    authority: Option<Arc<dyn Send + Sync>>,
}
impl std::fmt::Debug for NativeProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeProcess")
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
            desktop_bridge: false,
        })
    }

    /// Use the packaged desktop executable's explicit fleet bridge mode. The mode prefix is
    /// fixed native configuration, never a renderer-supplied argument or shell command.
    pub fn with_desktop_bridge(executable: &Path, desktop: &Path) -> io::Result<Self> {
        Ok(Self {
            executable: executable_path(executable)?,
            bridge: executable_path(desktop)?,
            desktop_bridge: true,
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
        let mut bridge_args = Vec::new();
        if self.desktop_bridge {
            bridge_args.push(Json::text("--mesh-fleet-mcp"));
        }
        bridge_args.extend([Json::text("--endpoint"), Json::text(endpoint)]);
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
                    Json::Array(bridge_args).encode()
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
        spawn_configured(command, goal, ProviderProtocol::Codex)
    }
}
fn spawn_configured(
    mut command: Command,
    goal: &str,
    protocol: ProviderProtocol,
) -> io::Result<NativeProcess> {
    // The live, unreaped direct child pins this process-group number against PID reuse.
    // Descendants may deliberately leave the group, so this is cancellation, not containment.
    command.process_group(0);
    let mut child = command.spawn()?;
    let output = Arc::new(Mutex::new(Output::default()));
    // Drain both pipes before feeding input, so startup diagnostics cannot block the writer.
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let out = output.clone();
    thread::spawn(move || consume_protocol(stdout, out, false, protocol));
    let err = output.clone();
    thread::spawn(move || consume_protocol(stderr, err, true, protocol));
    let mut input = child.stdin.take().expect("piped stdin");
    let prompt = format!("Work only in this assigned Mesh lane. Call mesh_fleet_context before editing. Use mesh_fleet_checkpoint to save your result, then mesh_fleet_submit_review for a complete checkpoint. Never approve or publish shared state. Do not print environment credentials.\n\n{goal}\n");
    if input
        .write_all(prompt.as_bytes())
        .and_then(|()| input.flush())
        .is_err()
    {
        let _ = stop_owned_group(&mut child);
        let _ = child.wait();
        return Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "provider input was not accepted",
        ));
    }
    drop(input);
    Ok(NativeProcess {
        child,
        output,
        exit: None,
        authority: None,
    })
}

impl NativeProcess {
    pub(super) fn retain_authority(&mut self, authority: Arc<dyn Send + Sync>) {
        debug_assert!(self.authority.is_none());
        self.authority = Some(authority);
    }

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
    /// Stop the owned provider group before reaping its leader. Escaped descendants may remain;
    /// neither successful signaling nor pipe closure authorizes capacity or custody release.
    /// After the direct child is reaped, its numeric identity is never used to signal a group.
    pub fn request_stop(&mut self) -> io::Result<()> {
        if self.exit.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "provider leader already reaped",
            ));
        }
        stop_owned_group(&mut self.child)
    }
    pub(super) fn abort_owned_group(&mut self) {
        let _ = self.request_stop();
        if self.exit.is_none() {
            self.exit = self.child.wait().ok();
        }
    }
}

#[allow(unsafe_code)]
fn stop_owned_group(child: &mut Child) -> io::Result<()> {
    unsafe extern "C" {
        fn kill(pid: std::os::raw::c_int, signal: std::os::raw::c_int) -> std::os::raw::c_int;
    }
    let leader = i32::try_from(child.id())
        .ok()
        .filter(|id| *id > 1)
        .ok_or_else(|| io::Error::other("invalid owned provider group"))?;
    // SAFETY: spawn_configured created this group, and callers have not reaped the owned child.
    // Its PID cannot be reused while it is alive or an unreaped zombie. Never signal after poll
    // recorded exit, or reconstruct this authority from persisted process numbers.
    let group = if unsafe { kill(-leader, 9) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    };
    // Also stop the owned child if it moved to another group; this handle is still unreaped.
    let direct = child.kill();
    group.and(direct)
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
#[cfg(test)]
fn consume(reader: impl io::Read, output: Arc<Mutex<Output>>, stderr: bool) {
    consume_protocol(reader, output, stderr, ProviderProtocol::Codex);
}
fn consume_protocol(
    reader: impl io::Read,
    output: Arc<Mutex<Output>>,
    stderr: bool,
    protocol: ProviderProtocol,
) {
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
        protocol.observe(&line, &mut state.observation);
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

    #[test]
    fn cancellation_stops_inherited_provider_group_without_signaling_an_independent_process() {
        use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
        let root = std::env::temp_dir().join(format!(
            "mesh-provider-group-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let ready = root.join("ready");
        let mut command = Command::new("/bin/sh");
        // The background child inherits both output pipes. Killing only the shell cannot close
        // those pipes promptly. It also has a short natural lifetime, so a failing regression
        // can drain and reap safely without signaling a numeric group after its leader is reaped.
        command
            .args([
                "-c",
                "cat >/dev/null; sleep 4 & echo ready > \"$MESH_TEST_READY\"; wait",
            ])
            .env("MESH_TEST_READY", &ready)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut process = spawn_configured(command, "fixture", ProviderProtocol::Codex).unwrap();
        let mut independent = Command::new("/bin/sleep").arg("10").spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !ready.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        let started = ready.exists();
        let before_stop = Instant::now();
        let stop = process.request_stop();
        let deadline = Instant::now() + Duration::from_secs(6);
        let mut observed = None;
        while Instant::now() < deadline {
            match process.poll() {
                Ok((activity, Some(outcome))) => {
                    observed = Some((activity.streams_closed, outcome, before_stop.elapsed()));
                    break;
                }
                Ok(_) => thread::sleep(Duration::from_millis(5)),
                Err(_) => break,
            }
        }
        let independent_live = independent.try_wait().unwrap().is_none();
        let _ = independent.kill();
        independent.wait().unwrap();
        process.abort_owned_group();
        let after_reap = process.request_stop().unwrap_err().kind();
        std::fs::remove_dir_all(root).unwrap();
        assert!(
            started,
            "provider descendant must be started before cancellation"
        );
        stop.unwrap();
        let (closed, outcome, elapsed) = observed.expect("owned provider must drain and exit");
        assert!(closed);
        assert!(!outcome);
        assert!(
            elapsed < Duration::from_secs(2),
            "inherited provider group must stop before natural child exit"
        );
        assert!(independent_live, "an unrelated process must remain alive");
        assert_eq!(after_reap, io::ErrorKind::InvalidInput);
    }

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
