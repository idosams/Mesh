//! A dormant, nonce-bound bridge for proving the packaged webview rendered and handled controls.
//!
//! This is deliberately not a workspace API. Without the exact proof environment supplied by the
//! archive verifier every command refuses. Exact, secret-free progress diagnostics and the final
//! accepted result are emitted only to stderr for the enclosing local verifier.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use mesh_daemon::ipc::Json;

const NONCE_ENV: &str = "MESH_RENDERER_PROOF_NONCE";
const SURFACE_ENV: &str = "MESH_RENDERER_PROOF_SURFACE";
const SOURCE_ENV: &str = "MESH_RENDERER_PROOF_SOURCE";
const DESTINATION_ENV: &str = "MESH_RENDERER_PROOF_DESTINATION";
const SCREENSHOT_ENV: &str = "MESH_RENDERER_PROOF_SCREENSHOT";
const AGENT_RESULT_NAME: &str = "agent-proof-result.txt";
const AGENT_RESULT_BYTES: &[u8] = b"packaged agent handoff result\n";
const MAX_REPORT_BYTES: usize = 4_096;
const FAILURE_CODES: [&str; 33] = [
    "configuration",
    "onboarding-mount",
    "onboarding-path",
    "onboarding-preview",
    "onboarding-confirm",
    "files-mount",
    "files-navigation",
    "files-native-open",
    "files-native-reveal",
    "files-folder-open",
    "files-screenshot",
    "review-mount",
    "review-content",
    "review-inline",
    "review-image-visual",
    "review-image-preview",
    "review-image-preview-native",
    "review-image-preview-envelope",
    "review-image-preview-pending",
    "review-image-selection-lost",
    "review-image-preview-evidence",
    "review-image-preview-refused",
    "review-image-preview-idle",
    "review-native-open",
    "review-native-reveal",
    "versions-mount",
    "versions-preview",
    "private-export-mount",
    "private-export-original",
    "private-export-complete",
    "agent-handoff-mount",
    "agent-handoff-start",
    "agent-handoff-finish",
];
const CHECKPOINT_CODES: [&str; 56] = [
    "files-mounted",
    "files-folder-expanded",
    "files-file-selected",
    "files-file-opened",
    "files-file-revealed",
    "files-folder-opened",
    "files-workspace-opened",
    "review-mounted",
    "review-text-selected",
    "review-visual",
    "review-content",
    "review-inline",
    "review-image-selected",
    "review-image-visual",
    "review-image-preview",
    "review-saved-open",
    "review-saved-reveal",
    "review-bounded-unavailable",
    "versions-start",
    "versions-mounted",
    "versions-clicked",
    "private-export-start",
    "private-export-mounted",
    "private-export-clicked",
    "private-export-destination-mounted",
    "private-export-destination-visible",
    "private-export-original-refused",
    "private-export-target-empty",
    "private-export-target-source",
    "private-export-target-other",
    "private-export-target-ready",
    "private-export-target-accepted",
    "private-export-preview-enabled",
    "private-export-preview-ready",
    "private-export-bounded-blocked",
    "agent-handoff-start-clicked",
    "agent-handoff-assigned",
    "agent-handoff-finish-clicked",
    "agent-handoff-confirmed",
    "agent-handoff-preflight-command-entered",
    "agent-handoff-preflight-command-returned",
    "agent-handoff-complete",
    "agent-handoff-native-preflight",
    "agent-handoff-native-release",
    "agent-handoff-native-rescan",
    "private-export-picker-superseded",
    "private-export-input-superseded",
    "private-export-workspace-superseded",
    "private-export-coordinator-accepted",
    "private-export-legacy-target-empty",
    "private-export-legacy-target-ready",
    "private-export-projection-target-empty",
    "private-export-projection-target-ready",
    "private-export-react-generation-stale",
    "private-export-react-generation-advanced",
    "private-export-react-rejected",
];

#[derive(Clone, Debug, Eq, PartialEq)]
struct RendererProofConfiguration {
    nonce: String,
    surface: &'static str,
    source: Option<String>,
    destination: Option<String>,
    capture_screenshot: bool,
}

impl RendererProofConfiguration {
    fn from_values(
        nonce: Option<String>,
        surface: Option<String>,
        source: Option<String>,
        destination: Option<String>,
    ) -> Option<Self> {
        let nonce = nonce?;
        let surface = surface?;
        if nonce.len() != 64
            || !nonce
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return None;
        }
        let surface = match surface.as_str() {
            "onboarding" => "onboarding",
            "files" => "files",
            "review" => "review",
            "versions" => "versions",
            "private-export" => "private-export",
            "agent-handoff" => "agent-handoff",
            _ => return None,
        };
        let (source, destination) = match (surface, source, destination) {
            ("onboarding", Some(source), None) if !source.is_empty() && source.len() <= 4_096 => {
                (Some(source), None)
            }
            ("files", None, None) => (None, None),
            ("review", None, None) => (None, None),
            ("versions", None, None) => (None, None),
            ("agent-handoff", Some(source), None)
                if !source.is_empty() && source.len() <= 4_096 =>
            {
                (Some(source), None)
            }
            ("private-export", Some(source), Some(destination))
                if !source.is_empty()
                    && source.len() <= 4_096
                    && !destination.is_empty()
                    && destination.len() <= 4_096
                    && source != destination =>
            {
                (Some(source), Some(destination))
            }
            _ => return None,
        };
        Some(Self {
            nonce,
            surface,
            source,
            destination,
            capture_screenshot: false,
        })
    }

    fn from_environment() -> Option<Self> {
        let mut configuration = Self::from_values(
            std::env::var(NONCE_ENV).ok(),
            std::env::var(SURFACE_ENV).ok(),
            std::env::var(SOURCE_ENV).ok(),
            std::env::var(DESTINATION_ENV).ok(),
        )?;
        match std::env::var(SCREENSHOT_ENV) {
            Err(std::env::VarError::NotPresent) => {}
            Ok(value) if value == "1" && configuration.surface == "files" => {
                configuration.capture_screenshot = true;
            }
            _ => {
                eprintln!("mesh-renderer-proof-configuration-refused:screenshot");
                return None;
            }
        }
        if configuration.surface == "private-export"
            && !private_export_paths_are_confined(&configuration)
        {
            eprintln!("mesh-renderer-proof-configuration-refused:private-export-confinement");
            return None;
        }
        if configuration.surface == "agent-handoff"
            && !agent_handoff_path_is_confined(&configuration)
        {
            eprintln!("mesh-renderer-proof-configuration-refused:agent-handoff-confinement");
            return None;
        }
        Some(configuration)
    }

    fn json(&self) -> String {
        Json::object([
            ("schema", Json::text("mesh-renderer-proof-config/v2")),
            ("nonce", Json::text(&self.nonce)),
            ("surface", Json::text(self.surface)),
            (
                "source",
                self.source.as_ref().map_or(Json::Null, Json::text),
            ),
            (
                "destination",
                self.destination.as_ref().map_or(Json::Null, Json::text),
            ),
        ])
        .encode()
    }
}

fn canonical_directory(value: impl AsRef<Path>) -> Option<PathBuf> {
    let path = std::fs::canonicalize(value).ok()?;
    path.is_dir().then_some(path)
}

fn private_export_paths_are_confined(configuration: &RendererProofConfiguration) -> bool {
    let Some(proof_root) = std::env::var_os("TMPDIR") else {
        return false;
    };
    let Some(home) = std::env::var_os("HOME") else {
        return false;
    };
    let Some(fixed_home) = std::env::var_os("CFFIXED_USER_HOME") else {
        return false;
    };
    private_export_paths_are_confined_to(configuration, proof_root, home, fixed_home)
}

fn private_export_paths_are_confined_to(
    configuration: &RendererProofConfiguration,
    proof_root: impl AsRef<Path>,
    home: impl AsRef<Path>,
    fixed_home: impl AsRef<Path>,
) -> bool {
    let Some(source) = configuration
        .source
        .as_deref()
        .and_then(canonical_directory)
    else {
        return false;
    };
    let Some(destination) = configuration
        .destination
        .as_deref()
        .and_then(canonical_directory)
    else {
        return false;
    };
    let Some(proof_root) = canonical_directory(proof_root) else {
        return false;
    };
    let Some(home) = canonical_directory(home) else {
        return false;
    };
    let Some(fixed_home) = canonical_directory(fixed_home) else {
        return false;
    };
    let private_root = proof_root
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("mesh-app-"));
    #[cfg(unix)]
    let private_permissions = std::fs::metadata(&proof_root)
        .ok()
        .is_some_and(|metadata| metadata.permissions().mode() & 0o077 == 0);
    #[cfg(not(unix))]
    let private_permissions = false;
    private_root
        && private_permissions
        && home == fixed_home
        && home == proof_root.join("home")
        && source.starts_with(&proof_root)
        && !source.starts_with(&home)
        && destination.starts_with(&home)
        && destination != home
        && destination != source
}

fn agent_handoff_path_is_confined(configuration: &RendererProofConfiguration) -> bool {
    let Some(proof_root) = std::env::var_os("TMPDIR").and_then(canonical_directory) else {
        return false;
    };
    let Some(home) = std::env::var_os("HOME").and_then(canonical_directory) else {
        return false;
    };
    let Some(fixed_home) = std::env::var_os("CFFIXED_USER_HOME").and_then(canonical_directory)
    else {
        return false;
    };
    agent_handoff_path_is_confined_to(configuration, &proof_root, &home, &fixed_home)
}

fn agent_handoff_path_is_confined_to(
    configuration: &RendererProofConfiguration,
    proof_root: &Path,
    home: &Path,
    fixed_home: &Path,
) -> bool {
    let private_root = proof_root
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("mesh-app-"));
    #[cfg(unix)]
    let private_permissions = std::fs::metadata(proof_root)
        .ok()
        .is_some_and(|metadata| metadata.permissions().mode() & 0o077 == 0);
    #[cfg(not(unix))]
    let private_permissions = false;
    let Some(source) = configuration
        .source
        .as_deref()
        .and_then(canonical_directory)
    else {
        return false;
    };
    let versions = home.join("Library/Application Support/dev.mesh.desktop/workspace-versions");
    private_root
        && private_permissions
        && home == fixed_home
        && home == proof_root.join("home")
        && source
            .file_name()
            .is_some_and(|name| name == mesh_daemon::workspace::PRESENTED_DIRECTORY_NAME)
        && source
            .parent()
            .is_some_and(|parent| parent.starts_with(versions))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AgentHandoffProofPhase {
    Launched,
    Preflight,
    Released,
    Rescanned,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AgentHandoffProofState {
    root: String,
    installation: String,
    generation: String,
    phase: AgentHandoffProofPhase,
}

pub(crate) struct RendererProofRuntime {
    configuration: Option<RendererProofConfiguration>,
    completed: Mutex<bool>,
    private_export_picker_uses: Mutex<u8>,
    private_export_confirmation_used: Mutex<bool>,
    agent_handoff: Mutex<Option<AgentHandoffProofState>>,
}

impl RendererProofRuntime {
    #[cfg(test)]
    pub(crate) fn disabled() -> Self {
        Self {
            configuration: None,
            completed: Mutex::new(false),
            private_export_picker_uses: Mutex::new(0),
            private_export_confirmation_used: Mutex::new(false),
            agent_handoff: Mutex::new(None),
        }
    }

    pub(crate) fn from_environment() -> Self {
        Self {
            configuration: RendererProofConfiguration::from_environment(),
            completed: Mutex::new(false),
            private_export_picker_uses: Mutex::new(0),
            private_export_confirmation_used: Mutex::new(false),
            agent_handoff: Mutex::new(None),
        }
    }

    pub(crate) fn configuration(&self) -> Result<String, String> {
        let configuration = self
            .configuration
            .as_ref()
            .ok_or_else(|| "packaged renderer proof is not enabled".to_owned())?;
        eprintln!(
            "mesh-renderer-proof-configuration-served:{}",
            configuration.surface
        );
        Ok(configuration.json())
    }

    pub(crate) fn files_screenshot_name(&self) -> Result<Option<String>, String> {
        let configuration = self
            .configuration
            .as_ref()
            .ok_or_else(|| "packaged renderer proof is not enabled".to_owned())?;
        if configuration.surface != "files" {
            return Err(
                "packaged renderer screenshot is available only for Files proof".to_owned(),
            );
        }
        Ok(configuration
            .capture_screenshot
            .then(|| format!("files-{}.png", configuration.nonce)))
    }

    pub(crate) fn accept(&self, report: &str) -> Result<String, String> {
        let configuration = self
            .configuration
            .as_ref()
            .ok_or_else(|| "packaged renderer proof is not enabled".to_owned())?;
        if report.len() > MAX_REPORT_BYTES {
            return Err("packaged renderer proof exceeded its bounded size".to_owned());
        }
        let parsed = Json::parse(report)
            .map_err(|_| "packaged renderer proof was not exact JSON".to_owned())?;
        if parsed.encode() != report {
            return Err("packaged renderer proof was not canonical JSON".to_owned());
        }
        let Json::Object(fields) = &parsed else {
            return Err("packaged renderer proof was not one object".to_owned());
        };
        let expected_keys = [
            "schema",
            "nonce",
            "surface",
            "mounted",
            "visible",
            "interaction",
            "outcome",
        ];
        if fields.len() != expected_keys.len()
            || fields
                .iter()
                .zip(expected_keys)
                .any(|((actual, _), expected)| actual != expected)
        {
            return Err("packaged renderer proof had unrecognized or missing fields".to_owned());
        }
        let expected_claim = match configuration.surface {
            "onboarding" => ("preview-path-confirm-import", "import-completed-after-busy"),
            "files" => (
                "expand-select-open-reveal-folders",
                "native-file-and-folder-actions-completed",
            ),
            "review" => ("", ""),
            "versions" => ("select-saved-point", "verified-preview-ready"),
            "private-export" => ("", ""),
            "agent-handoff" => ("start-finish-rescan", "agent-handoff-completed"),
            _ => unreachable!("configuration construction closes the surface vocabulary"),
        };
        let interaction = parsed.get("interaction").and_then(Json::as_text);
        let outcome = parsed.get("outcome").and_then(Json::as_text);
        let claim_matches = match configuration.surface {
            "review" => matches!(
                (interaction, outcome),
                (
                    Some("content-inline-native-open-reveal"),
                    Some("saved-side-native-launches-completed")
                ) | (
                    Some("bounded-incomplete-review-inspection"),
                    Some("incomplete-review-disclosed-without-authority")
                )
            ),
            "private-export" => matches!(
                (interaction, outcome),
                (
                    Some("refuse-original-then-confirm-private"),
                    Some("private-export-completed")
                ) | (
                    Some("bounded-review-private-export-refusal"),
                    Some("private-export-blocked-without-complete-review")
                )
            ),
            _ => interaction == Some(expected_claim.0) && outcome == Some(expected_claim.1),
        };
        if parsed.get("schema").and_then(Json::as_text) != Some("mesh-renderer-proof/v1")
            || parsed.get("nonce").and_then(Json::as_text) != Some(configuration.nonce.as_str())
            || parsed.get("surface").and_then(Json::as_text) != Some(configuration.surface)
            || parsed.get("mounted").and_then(Json::as_bool) != Some(true)
            || parsed.get("visible").and_then(Json::as_bool) != Some(true)
            || !claim_matches
        {
            return Err(
                "packaged renderer proof did not match the configured interaction".to_owned(),
            );
        }
        if configuration.surface == "agent-handoff"
            && self
                .agent_handoff
                .lock()
                .map_err(|_| "packaged agent proof state is unavailable".to_owned())?
                .as_ref()
                .map(|state| state.phase)
                != Some(AgentHandoffProofPhase::Rescanned)
        {
            return Err("packaged agent proof did not complete its exact lifecycle".to_owned());
        }
        let mut completed = self
            .completed
            .lock()
            .map_err(|_| "packaged renderer proof state is unavailable".to_owned())?;
        if *completed {
            return Err("packaged renderer proof was already completed".to_owned());
        }
        *completed = true;
        Ok(parsed.encode())
    }

    pub(crate) fn accept_agent_handoff_launch(
        &self,
        root: &Path,
        installation: &str,
        generation: &str,
    ) -> Result<Option<bool>, String> {
        let Some(configuration) = self.configuration.as_ref() else {
            return Ok(None);
        };
        if configuration.surface != "agent-handoff" {
            return Ok(None);
        }
        let configured = configuration
            .source
            .as_deref()
            .and_then(canonical_directory)
            .ok_or_else(|| "packaged agent proof source was unavailable".to_owned())?;
        let actual = canonical_directory(root)
            .ok_or_else(|| "packaged agent proof workspace was unavailable".to_owned())?;
        if configured != actual
            || installation.is_empty()
            || generation.len() != 32
            || !generation
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("packaged agent proof launch binding was invalid".to_owned());
        }
        let result = actual.join(AGENT_RESULT_NAME);
        if std::fs::symlink_metadata(&result).is_ok() {
            return Err("packaged agent proof result already existed".to_owned());
        }
        {
            let mut state = self
                .agent_handoff
                .lock()
                .map_err(|_| "packaged agent proof state is unavailable".to_owned())?;
            if state.is_some() {
                return Err("packaged agent proof launcher was already used".to_owned());
            }
            *state = Some(AgentHandoffProofState {
                root: actual.display().to_string(),
                installation: installation.to_owned(),
                generation: generation.to_owned(),
                phase: AgentHandoffProofPhase::Launched,
            });
        }
        eprintln!("mesh-renderer-proof-checkpoint:agent-handoff-launched");
        for _ in 0..100 {
            match std::fs::symlink_metadata(&result) {
                Ok(metadata) if metadata.file_type().is_file() => {
                    let bytes = std::fs::read(&result)
                        .map_err(|_| "packaged agent proof result was unreadable".to_owned())?;
                    if bytes == AGENT_RESULT_BYTES {
                        return Ok(Some(true));
                    }
                    return Err("packaged agent proof result bytes were invalid".to_owned());
                }
                Ok(_) => {
                    return Err("packaged agent proof result was not a regular file".to_owned())
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(_) => {
                    return Err("packaged agent proof result could not be inspected".to_owned())
                }
            }
        }
        Err("packaged agent proof result did not arrive".to_owned())
    }

    fn advance_agent_handoff(
        &self,
        root: &str,
        installation: &str,
        generation: &str,
        expected: AgentHandoffProofPhase,
        next: AgentHandoffProofPhase,
    ) -> Result<(), String> {
        let Some(configuration) = self.configuration.as_ref() else {
            return Ok(());
        };
        if configuration.surface != "agent-handoff" {
            return Ok(());
        }
        let canonical = canonical_directory(root)
            .ok_or_else(|| "packaged agent proof workspace was unavailable".to_owned())?;
        let mut state = self
            .agent_handoff
            .lock()
            .map_err(|_| "packaged agent proof state is unavailable".to_owned())?;
        let current = state
            .as_mut()
            .ok_or_else(|| "packaged agent proof launcher was not accepted".to_owned())?;
        let root_matches = current.root == canonical.display().to_string();
        let installation_matches = current.installation == installation;
        let generation_matches = current.generation == generation;
        let phase_matches = current.phase == expected;
        if !root_matches || !installation_matches || !generation_matches || !phase_matches {
            eprintln!(
                "mesh-renderer-proof-agent-lifecycle-mismatch:root={root_matches}:installation={installation_matches}:generation={generation_matches}:phase={phase_matches}:actual_phase={:?}:expected_phase={expected:?}",
                current.phase,
            );
            return Err("packaged agent proof lifecycle changed identity or order".to_owned());
        }
        current.phase = next;
        let checkpoint = match next {
            AgentHandoffProofPhase::Preflight => "agent-handoff-native-preflight",
            AgentHandoffProofPhase::Released => "agent-handoff-native-release",
            AgentHandoffProofPhase::Rescanned => "agent-handoff-native-rescan",
            AgentHandoffProofPhase::Launched => unreachable!("launch is recorded separately"),
        };
        eprintln!("mesh-renderer-proof-checkpoint:{checkpoint}");
        Ok(())
    }

    pub(crate) fn record_agent_handoff_preflight(
        &self,
        root: &str,
        installation: &str,
        generation: &str,
    ) -> Result<(), String> {
        self.advance_agent_handoff(
            root,
            installation,
            generation,
            AgentHandoffProofPhase::Launched,
            AgentHandoffProofPhase::Preflight,
        )
    }

    pub(crate) fn record_agent_handoff_release(
        &self,
        root: &str,
        installation: &str,
        generation: &str,
    ) -> Result<(), String> {
        self.advance_agent_handoff(
            root,
            installation,
            generation,
            AgentHandoffProofPhase::Preflight,
            AgentHandoffProofPhase::Released,
        )
    }

    pub(crate) fn record_agent_handoff_rescan(
        &self,
        root: &str,
        installation: &str,
        generation: &str,
    ) -> Result<(), String> {
        self.advance_agent_handoff(
            root,
            installation,
            generation,
            AgentHandoffProofPhase::Released,
            AgentHandoffProofPhase::Rescanned,
        )
    }

    pub(crate) fn accept_private_export_confirmation(
        &self,
        destination: &str,
    ) -> Result<bool, String> {
        let configuration = self
            .configuration
            .as_ref()
            .ok_or_else(|| "packaged renderer proof is not enabled".to_owned())?;
        if configuration.surface != "private-export"
            || configuration.destination.as_deref() != Some(destination)
        {
            eprintln!("mesh-renderer-proof-confirmation-refused:surface-or-destination");
            return Err(
                "packaged renderer proof did not authorize this private export destination"
                    .to_owned(),
            );
        }
        let mut used = self
            .private_export_confirmation_used
            .lock()
            .map_err(|_| "packaged renderer proof confirmation is unavailable".to_owned())?;
        if *used {
            eprintln!("mesh-renderer-proof-confirmation-refused:already-used");
            return Err("packaged renderer proof confirmation was already used".to_owned());
        }
        *used = true;
        eprintln!("mesh-renderer-proof-confirmation-accepted:private-export");
        Ok(true)
    }

    pub(crate) fn take_private_export_picker_destination(&self) -> Result<Option<String>, String> {
        let Some(configuration) = self.configuration.as_ref() else {
            return Ok(None);
        };
        eprintln!(
            "mesh-renderer-proof-picker-requested:{}",
            configuration.surface
        );
        if configuration.surface != "private-export" {
            return Ok(None);
        }
        let source = configuration
            .source
            .as_ref()
            .ok_or_else(|| "packaged renderer proof has no private export source".to_owned())?;
        let destination = configuration.destination.as_ref().ok_or_else(|| {
            "packaged renderer proof has no private export destination".to_owned()
        })?;
        let mut uses = self
            .private_export_picker_uses
            .lock()
            .map_err(|_| "packaged renderer proof picker is unavailable".to_owned())?;
        let selected = match *uses {
            0 => source,
            1 => destination,
            _ => {
                eprintln!("mesh-renderer-proof-picker-refused:already-complete");
                return Err(
                    "packaged renderer proof picker sequence was already complete".to_owned(),
                );
            }
        };
        *uses += 1;
        eprintln!("mesh-renderer-proof-picker-accepted:private-export:{uses}");
        Ok(Some(selected.clone()))
    }

    pub(crate) fn report_failure(&self, code: &str) -> Result<&'static str, String> {
        let configuration = self
            .configuration
            .as_ref()
            .ok_or_else(|| "packaged renderer proof is not enabled".to_owned())?;
        let code = FAILURE_CODES
            .into_iter()
            .find(|candidate| *candidate == code)
            .ok_or_else(|| "packaged renderer proof failure code was not recognized".to_owned())?;
        eprintln!(
            "mesh-renderer-proof-failure:{}:{code}",
            configuration.surface
        );
        Ok(code)
    }

    pub(crate) fn report_checkpoint(&self, code: &str) -> Result<&'static str, String> {
        let configuration = self
            .configuration
            .as_ref()
            .ok_or_else(|| "packaged renderer proof is not enabled".to_owned())?;
        let surface_matches = match configuration.surface {
            "files" => code.starts_with("files-"),
            "review" => code.starts_with("review-"),
            "versions" => code.starts_with("versions-"),
            "private-export" => code.starts_with("private-export-"),
            "agent-handoff" => code.starts_with("agent-handoff-"),
            _ => false,
        };
        if !surface_matches {
            return Err("packaged renderer proof checkpoint was not available".to_owned());
        }
        let code = CHECKPOINT_CODES
            .into_iter()
            .find(|candidate| *candidate == code)
            .ok_or_else(|| "packaged renderer proof checkpoint was not recognized".to_owned())?;
        eprintln!("mesh-renderer-proof-checkpoint:{code}");
        Ok(code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime(surface: &str) -> RendererProofRuntime {
        RendererProofRuntime {
            configuration: RendererProofConfiguration::from_values(
                Some("ab".repeat(32)),
                Some(surface.to_owned()),
                matches!(surface, "onboarding" | "private-export" | "agent-handoff")
                    .then(|| "/tmp/source".to_owned()),
                (surface == "private-export").then(|| "/tmp/destination".to_owned()),
            ),
            completed: Mutex::new(false),
            private_export_picker_uses: Mutex::new(0),
            private_export_confirmation_used: Mutex::new(false),
            agent_handoff: Mutex::new(None),
        }
    }

    fn report(surface: &str, interaction: &str, outcome: &str) -> String {
        Json::object([
            ("schema", Json::text("mesh-renderer-proof/v1")),
            ("nonce", Json::text("ab".repeat(32))),
            ("surface", Json::text(surface)),
            ("mounted", Json::Bool(true)),
            ("visible", Json::Bool(true)),
            ("interaction", Json::text(interaction)),
            ("outcome", Json::text(outcome)),
        ])
        .encode()
    }

    #[test]
    fn configuration_requires_one_exact_complete_session() {
        assert!(RendererProofConfiguration::from_values(None, None, None, None).is_none());
        assert!(RendererProofConfiguration::from_values(
            Some("ab".repeat(32)),
            Some("onboarding".to_owned()),
            None,
            None,
        )
        .is_none());
        assert!(RendererProofConfiguration::from_values(
            Some("AB".repeat(32)),
            Some("review".to_owned()),
            None,
            None,
        )
        .is_none());
        assert!(RendererProofConfiguration::from_values(
            Some("ab".repeat(32)),
            Some("invented".to_owned()),
            None,
            None,
        )
        .is_none());
        let accepted = RendererProofConfiguration::from_values(
            Some("ab".repeat(32)),
            Some("onboarding".to_owned()),
            Some("/tmp/source".to_owned()),
            None,
        )
        .expect("exact proof configuration");
        assert_eq!(accepted.surface, "onboarding");
        let versions = RendererProofConfiguration::from_values(
            Some("ab".repeat(32)),
            Some("versions".to_owned()),
            None,
            None,
        )
        .expect("exact versions proof configuration");
        assert_eq!(versions.surface, "versions");
        let files = RendererProofConfiguration::from_values(
            Some("ab".repeat(32)),
            Some("files".to_owned()),
            None,
            None,
        )
        .expect("exact Files proof configuration");
        assert_eq!(files.surface, "files");
        assert!(RendererProofConfiguration::from_values(
            Some("ab".repeat(32)),
            Some("files".to_owned()),
            Some("/tmp/source".to_owned()),
            None,
        )
        .is_none());
        assert!(RendererProofConfiguration::from_values(
            Some("ab".repeat(32)),
            Some("versions".to_owned()),
            Some("/tmp/source".to_owned()),
            None,
        )
        .is_none());
        assert!(RendererProofConfiguration::from_values(
            Some("ab".repeat(32)),
            Some("private-export".to_owned()),
            Some("/tmp/source".to_owned()),
            None,
        )
        .is_none());
        let private_export = RendererProofConfiguration::from_values(
            Some("ab".repeat(32)),
            Some("private-export".to_owned()),
            Some("/tmp/source".to_owned()),
            Some("/tmp/destination".to_owned()),
        )
        .expect("complete private export proof");
        assert_eq!(private_export.surface, "private-export");
        let agent_handoff = RendererProofConfiguration::from_values(
            Some("ab".repeat(32)),
            Some("agent-handoff".to_owned()),
            Some("/tmp/source".to_owned()),
            None,
        )
        .expect("complete agent handoff proof");
        assert_eq!(agent_handoff.surface, "agent-handoff");
    }

    #[test]
    fn files_screenshot_name_is_nonce_bound_and_dormant_by_default() {
        let mut files_runtime = runtime("files");
        assert_eq!(files_runtime.files_screenshot_name().unwrap(), None);
        files_runtime
            .configuration
            .as_mut()
            .expect("Files proof configuration")
            .capture_screenshot = true;
        assert_eq!(
            files_runtime.files_screenshot_name().unwrap().as_deref(),
            Some("files-abababababababababababababababababababababababababababababababab.png")
        );
        assert!(runtime("review").files_screenshot_name().is_err());
        assert!(RendererProofRuntime::disabled()
            .files_screenshot_name()
            .is_err());
    }

    #[test]
    fn mutating_private_export_proof_is_confined_to_the_private_verifier_home() {
        let proof_root = std::env::temp_dir().join(format!(
            "mesh-app-renderer-proof-confinement-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&proof_root);
        let home = proof_root.join("home");
        let source = proof_root.join("source");
        let destination = home.join("private-export");
        let outside = std::env::temp_dir().join(format!(
            "mesh-renderer-proof-outside-{}",
            std::process::id()
        ));
        for directory in [&home, &source, &destination, &outside] {
            std::fs::create_dir_all(directory).expect("proof directory");
        }
        #[cfg(unix)]
        std::fs::set_permissions(&proof_root, std::fs::Permissions::from_mode(0o700))
            .expect("private proof root");
        let proof_root = std::fs::canonicalize(&proof_root).expect("canonical proof root");
        let home = std::fs::canonicalize(&home).expect("canonical proof home");
        let configuration = RendererProofConfiguration::from_values(
            Some("ab".repeat(32)),
            Some("private-export".to_owned()),
            Some(source.to_string_lossy().into_owned()),
            Some(destination.to_string_lossy().into_owned()),
        )
        .expect("private export configuration");
        assert!(private_export_paths_are_confined_to(
            &configuration,
            &proof_root,
            &home,
            &home,
        ));

        let outside_configuration = RendererProofConfiguration::from_values(
            Some("ab".repeat(32)),
            Some("private-export".to_owned()),
            Some(source.to_string_lossy().into_owned()),
            Some(outside.to_string_lossy().into_owned()),
        )
        .expect("outside private export configuration");
        assert!(!private_export_paths_are_confined_to(
            &outside_configuration,
            &proof_root,
            &home,
            &home,
        ));
        assert!(!private_export_paths_are_confined_to(
            &configuration,
            &proof_root,
            &home,
            &source,
        ));
        let _ = std::fs::remove_dir_all(proof_root);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[test]
    fn mutating_agent_handoff_proof_is_confined_to_a_private_managed_mount() {
        let proof_root = std::env::temp_dir().join(format!(
            "mesh-app-agent-proof-confinement-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&proof_root);
        let home = proof_root.join("home");
        let source = home.join(
            "Library/Application Support/dev.mesh.desktop/workspace-versions/proof.mesh/Mesh Version - Working Folder",
        );
        let outside = proof_root.join("outside/mounts");
        for directory in [&source, &outside] {
            std::fs::create_dir_all(directory).expect("proof directory");
        }
        #[cfg(unix)]
        std::fs::set_permissions(&proof_root, std::fs::Permissions::from_mode(0o700))
            .expect("private proof root");
        let proof_root = std::fs::canonicalize(&proof_root).expect("canonical proof root");
        let home = std::fs::canonicalize(&home).expect("canonical proof home");
        let configuration = RendererProofConfiguration::from_values(
            Some("ab".repeat(32)),
            Some("agent-handoff".to_owned()),
            Some(source.to_string_lossy().into_owned()),
            None,
        )
        .expect("agent handoff configuration");
        assert!(agent_handoff_path_is_confined_to(
            &configuration,
            &proof_root,
            &home,
            &home,
        ));
        let outside_configuration = RendererProofConfiguration::from_values(
            Some("ab".repeat(32)),
            Some("agent-handoff".to_owned()),
            Some(outside.to_string_lossy().into_owned()),
            None,
        )
        .expect("outside agent handoff configuration");
        assert!(!agent_handoff_path_is_confined_to(
            &outside_configuration,
            &proof_root,
            &home,
            &home,
        ));
        assert!(!agent_handoff_path_is_confined_to(
            &configuration,
            &proof_root,
            &home,
            &outside,
        ));
        let _ = std::fs::remove_dir_all(proof_root);
    }

    #[test]
    fn report_is_nonce_bound_closed_and_single_use() {
        let proof_runtime = runtime("review");
        let accepted = report(
            "review",
            "content-inline-native-open-reveal",
            "saved-side-native-launches-completed",
        );
        assert_eq!(
            proof_runtime.accept(&accepted).expect("accepted report"),
            accepted
        );
        assert_eq!(
            proof_runtime
                .accept(&accepted)
                .expect_err("duplicate report"),
            "packaged renderer proof was already completed",
        );

        let wrong_nonce = accepted.replace(&"ab".repeat(32), &"cd".repeat(32));
        assert!(runtime("review").accept(&wrong_nonce).is_err());
        let invented =
            accepted.replacen("\"outcome\":", "\"native_authority\":true,\"outcome\":", 1);
        assert!(runtime("review").accept(&invented).is_err());
        let bounded = report(
            "review",
            "bounded-incomplete-review-inspection",
            "incomplete-review-disclosed-without-authority",
        );
        assert_eq!(
            runtime("review").accept(&bounded).expect("bounded review"),
            bounded,
        );
        let crossed_claim = report(
            "review",
            "bounded-incomplete-review-inspection",
            "saved-side-native-launches-completed",
        );
        assert!(runtime("review").accept(&crossed_claim).is_err());
    }

    #[test]
    fn private_export_confirmation_is_destination_bound_and_single_use() {
        assert!(RendererProofRuntime::disabled()
            .accept_private_export_confirmation("/tmp/destination")
            .is_err());
        assert!(runtime("review")
            .accept_private_export_confirmation("/tmp/destination")
            .is_err());
        assert!(runtime("private-export")
            .accept_private_export_confirmation("/tmp/wrong")
            .is_err());
        let runtime = runtime("private-export");
        assert!(runtime
            .accept_private_export_confirmation("/tmp/destination")
            .expect("first exact confirmation"));
        assert!(runtime
            .accept_private_export_confirmation("/tmp/destination")
            .is_err());
    }

    #[test]
    fn incomplete_review_private_export_refusal_is_one_closed_claim() {
        let bounded = report(
            "private-export",
            "bounded-review-private-export-refusal",
            "private-export-blocked-without-complete-review",
        );
        assert_eq!(
            runtime("private-export")
                .accept(&bounded)
                .expect("bounded refusal"),
            bounded,
        );
        let crossed_claim = report(
            "private-export",
            "bounded-review-private-export-refusal",
            "private-export-completed",
        );
        assert!(runtime("private-export").accept(&crossed_claim).is_err());
    }

    #[test]
    fn private_export_picker_is_confined_and_exactly_two_stage() {
        assert_eq!(
            RendererProofRuntime::disabled()
                .take_private_export_picker_destination()
                .expect("normal picker fallback"),
            None,
        );
        assert_eq!(
            runtime("review")
                .take_private_export_picker_destination()
                .expect("review picker fallback"),
            None,
        );
        let runtime = runtime("private-export");
        assert_eq!(
            runtime
                .take_private_export_picker_destination()
                .expect("proof original"),
            Some("/tmp/source".to_owned()),
        );
        assert_eq!(
            runtime
                .take_private_export_picker_destination()
                .expect("proof destination"),
            Some("/tmp/destination".to_owned()),
        );
        assert!(runtime.take_private_export_picker_destination().is_err());
    }

    #[test]
    fn renderer_checkpoints_are_surface_bound_and_closed() {
        assert!(RendererProofRuntime::disabled()
            .report_checkpoint("private-export-start")
            .is_err());
        assert!(runtime("review")
            .report_checkpoint("private-export-start")
            .is_err());
        assert!(runtime("private-export")
            .report_checkpoint("invented")
            .is_err());
        assert_eq!(
            runtime("private-export")
                .report_checkpoint("private-export-preview-ready")
                .expect("closed checkpoint"),
            "private-export-preview-ready",
        );
        assert_eq!(
            runtime("files")
                .report_checkpoint("files-file-selected")
                .expect("closed Files checkpoint"),
            "files-file-selected",
        );
        assert_eq!(
            runtime("versions")
                .report_checkpoint("versions-clicked")
                .expect("closed versions checkpoint"),
            "versions-clicked",
        );
        assert!(runtime("versions")
            .report_checkpoint("private-export-clicked")
            .is_err());
        assert!(runtime("files")
            .report_checkpoint("review-mounted")
            .is_err());
    }

    #[test]
    fn report_accepts_only_the_configured_surface_outcome() {
        let onboarding = report(
            "onboarding",
            "preview-path-confirm-import",
            "import-completed-after-busy",
        );
        assert_eq!(
            runtime("onboarding")
                .accept(&onboarding)
                .expect("onboarding"),
            onboarding
        );
        assert!(runtime("review").accept(&onboarding).is_err());
        let files = report(
            "files",
            "expand-select-open-reveal-folders",
            "native-file-and-folder-actions-completed",
        );
        assert_eq!(runtime("files").accept(&files).expect("Files"), files);
        assert!(runtime("files")
            .accept(&report(
                "files",
                "expand-select-open-reveal-folders",
                "native-file-opened"
            ))
            .is_err());
        assert!(runtime("review")
            .accept(&report("review", "content-inline", "verified-preview"))
            .is_err());
        let versions = report("versions", "select-saved-point", "verified-preview-ready");
        assert_eq!(
            runtime("versions").accept(&versions).expect("versions"),
            versions
        );
        assert!(runtime("versions")
            .accept(&report(
                "versions",
                "select-saved-point",
                "verified-preview"
            ))
            .is_err());
        let private_export = report(
            "private-export",
            "refuse-original-then-confirm-private",
            "private-export-completed",
        );
        assert_eq!(
            runtime("private-export")
                .accept(&private_export)
                .expect("private export"),
            private_export,
        );
    }

    #[test]
    fn agent_handoff_report_requires_the_exact_ordered_native_lifecycle() {
        let proof_runtime = runtime("agent-handoff");
        let canonical = canonical_directory("/tmp").expect("canonical temp directory");
        let root = canonical.display().to_string();
        let installation = "workspace-installation";
        let generation = "ab".repeat(16);
        *proof_runtime
            .agent_handoff
            .lock()
            .expect("agent proof state") = Some(AgentHandoffProofState {
            root: root.clone(),
            installation: installation.to_owned(),
            generation: generation.clone(),
            phase: AgentHandoffProofPhase::Launched,
        });
        let accepted = report(
            "agent-handoff",
            "start-finish-rescan",
            "agent-handoff-completed",
        );
        assert!(proof_runtime.accept(&accepted).is_err());
        assert!(proof_runtime
            .record_agent_handoff_release(&root, installation, &generation)
            .is_err());
        proof_runtime
            .record_agent_handoff_preflight(&root, installation, &generation)
            .expect("exact preflight");
        proof_runtime
            .record_agent_handoff_release(&root, installation, &generation)
            .expect("exact release");
        assert!(proof_runtime
            .record_agent_handoff_rescan(&root, installation, &"cd".repeat(16))
            .is_err());
        proof_runtime
            .record_agent_handoff_rescan(&root, installation, &generation)
            .expect("exact rescan");
        assert_eq!(
            proof_runtime.accept(&accepted).expect("complete lifecycle"),
            accepted
        );
    }

    #[test]
    fn failure_diagnostics_are_closed_and_disabled_without_a_proof_session() {
        assert_eq!(
            runtime("onboarding")
                .report_failure("onboarding-preview")
                .expect("known diagnostic"),
            "onboarding-preview",
        );
        assert!(runtime("onboarding").report_failure("path-secret").is_err());
        assert!(RendererProofRuntime::disabled()
            .report_failure("configuration")
            .is_err());
        assert_eq!(
            runtime("files")
                .report_failure("files-navigation")
                .expect("known Files diagnostic"),
            "files-navigation",
        );
        assert_eq!(
            runtime("review")
                .report_checkpoint("review-content")
                .expect("known review checkpoint"),
            "review-content",
        );
        assert!(runtime("review")
            .report_checkpoint("private-export-target-ready")
            .is_err());
    }
}
