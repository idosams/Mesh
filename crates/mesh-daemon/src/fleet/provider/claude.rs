//! Claude's fixed native launch configuration. Existing account authentication is provider-owned.
use super::*;

/// An installed Claude executable and Mesh bridge explicitly admitted by the native host.
/// Managed provider policy still applies; this does not claim to isolate the provider itself.
#[derive(Clone, Debug)]
pub struct ClaudeAdapter {
    executable: PathBuf,
    bridge: PathBuf,
    desktop_bridge: bool,
}
impl ClaudeAdapter {
    /// Admit explicit absolute executable locations, preserving the user's existing account.
    pub fn new(executable: &Path, bridge: &Path) -> io::Result<Self> {
        Ok(Self {
            executable: executable_path(executable)?,
            bridge: executable_path(bridge)?,
            desktop_bridge: false,
        })
    }
    /// Use the packaged desktop's fixed Mesh MCP mode.
    pub fn with_desktop_bridge(executable: &Path, desktop: &Path) -> io::Result<Self> {
        Ok(Self {
            executable: executable_path(executable)?,
            bridge: executable_path(desktop)?,
            desktop_bridge: true,
        })
    }
    pub(super) fn spawn(
        &self,
        root: &Path,
        endpoint: &Path,
        objective: &str,
        credential: &AgentCredential,
        goal: &str,
    ) -> io::Result<NativeProcess> {
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
        let mut arguments = Vec::new();
        if self.desktop_bridge {
            arguments.push(Json::text("--mesh-fleet-mcp"));
        }
        arguments.extend([Json::text("--endpoint"), Json::text(endpoint)]);
        let mcp = Json::object([(
            "mcpServers",
            Json::object([(
                "mesh",
                Json::object([
                    ("type", Json::text("stdio")),
                    ("command", Json::text(bridge)),
                    ("args", Json::Array(arguments)),
                    // Literal expansion references keep credential bytes out of arguments and config files.
                    (
                        "env",
                        Json::object([
                            (
                                "MESH_FLEET_OBJECTIVE",
                                Json::text("${MESH_FLEET_OBJECTIVE}"),
                            ),
                            (
                                "MESH_FLEET_CREDENTIAL",
                                Json::text("${MESH_FLEET_CREDENTIAL}"),
                            ),
                        ]),
                    ),
                ]),
            )]),
        )])
        .encode();
        let mut command = Command::new(&self.executable);
        command
            .current_dir(root)
            .args([
                "--print",
                "--verbose",
                "--output-format",
                "stream-json",
                "--no-session-persistence",
                "--permission-mode",
                "dontAsk",
                "--setting-sources",
                "",
                "--disable-slash-commands",
                "--strict-mcp-config",
                "--mcp-config",
                &mcp,
                "--settings",
                SETTINGS,
                // All file work goes through sandboxed Bash; no unsandboxed built-in write tools.
                "--tools",
                "Bash",
                "--allowedTools",
                "mcp__mesh__*",
            ])
            .env("MESH_FLEET_OBJECTIVE", objective)
            .env("MESH_FLEET_CREDENTIAL", credential.transport_value())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        spawn_configured(command, goal, ProviderProtocol::Claude)
    }
}
// No bare/safe-mode: bare disables existing OAuth; safe-mode disables the required Mesh MCP.
// dontAsk refuses interactive permission requests. The provider must support these settings;
// managed administration remains authoritative and is not overridden by Mesh.
const SETTINGS: &str = r#"{"disableAllHooks":true,"sandbox":{"enabled":true,"failIfUnavailable":true,"allowUnsandboxedCommands":false,"autoAllowBashIfSandboxed":true,"excludedCommands":[]}}"#;
