//! Session-bound native selections for read-only graphical remote observations.
use super::*;
mod profiles;
mod recovery;
pub(crate) mod setup;
use ring::rand::{SecureRandom as _, SystemRandom};
use std::sync::Mutex;
const BUSY: &str = "Another remote observation is in progress";
const ELIGIBILITY: &str = "Remote observation requires an eligible signed Mesh application";
struct Selection {
    profile_source: Option<String>,
    identity_file: setup::BoundPath,
    hosts_file: setup::BoundPath,
    id: String,
    config: Configuration,
    peer: NativeSshDestination,
    installation: ProtectedWorkspaceRoot,
    fleets: ProtectedWorkspaceRoot,
    coordinator: PublicKey,
    history: mesh_daemon::fleet::service::FleetHistory,
}
#[derive(Default)]
pub(crate) struct RemotePanel(Mutex<Option<Selection>>, Mutex<setup::Draft>);
fn eligible() -> Result<(), String> {
    AppleActorCustody::availability().map_err(|_| ELIGIBILITY.into())
}
fn kind(action: &str, after: u64) -> Result<RemoteObservationKind, String> {
    match (action, after) {
        ("status", 0) => Ok(RemoteObservationKind::CurrentLease),
        ("results", 0..=4096) => Ok(RemoteObservationKind::Results { after }),
        _ => Err(UNAVAILABLE.into()),
    }
}
impl RemotePanel {
    pub(crate) fn available(&self) -> Result<(), String> {
        eligible()
    }
    // Called only with the native file chooser result, never a renderer-supplied path.
    pub(crate) fn select(
        &self,
        path: &Path,
        host: &crate::attachment_host::AttachmentHost,
    ) -> Result<String, String> {
        eligible()?;
        let mut held = self.0.try_lock().map_err(|_| BUSY)?;
        let config = config(crate::worker_service::load_private_json(path)?)?;
        let selection = admit_selection(config, host)?;
        let result = selection.render().encode();
        *held = Some(selection);
        Ok(result)
    }
    pub(crate) fn forget(&self, id: &str) -> Result<(), String> {
        let mut held = self.0.try_lock().map_err(|_| BUSY)?;
        if held.as_ref().is_none_or(|s| s.id != id) {
            return Err(UNAVAILABLE.into());
        }
        *held = None;
        Ok(())
    }
    pub(crate) fn read(&self, id: &str, action: &str, after: u64) -> Result<String, String> {
        let operation = kind(action, after)?;
        eligible()?;
        let held = self.0.try_lock().map_err(|_| BUSY)?;
        let selected = held.as_ref().filter(|s| s.id == id).ok_or(UNAVAILABLE)?;
        selected.verify()?;
        let context = open_connection(&selected.config.connection)?;
        if context
            .installation
            .identity()
            .map_err(|_| UNAVAILABLE)?
            .worker()
            != selected.coordinator
        {
            return Err(UNAVAILABLE.into());
        }
        let observation = selected
            .history
            .prepare_remote_observation(
                &selected.config.lane,
                &selected.config.run,
                selected.coordinator,
                selected.config.connection.worker,
                operation,
                |payload| {
                    context.installation.identity().map_err(|_| UNAVAILABLE)?;
                    context
                        .custody
                        .sign(payload)
                        .map_err(|_| UNAVAILABLE.into())
                },
            )
            .map_err(|_| UNAVAILABLE)?;
        // Retain the original file metadata admission across reads, not fresh replacement paths.
        let result = observation
            .read_over_ssh(&selected.peer, Duration::from_secs(25))
            .map_err(|_| UNAVAILABLE)?;
        selected.verify()?;
        context.installation.identity().map_err(|_| UNAVAILABLE)?;
        Ok(project(&selected.id, result).encode())
    }
}
impl Selection {
    fn verify(&self) -> Result<(), String> {
        self.identity_file.verify()?;
        self.hosts_file.verify()?;
        if ProtectedWorkspaceRoot::inspect(&self.config.connection.installation)
            .map_err(|_| UNAVAILABLE)?
            != self.installation
            || ProtectedWorkspaceRoot::inspect(&self.config.connection.fleets)
                .map_err(|_| UNAVAILABLE)?
                != self.fleets
        {
            return Err(UNAVAILABLE.into());
        }
        Ok(())
    }
    fn render(&self) -> Json {
        Json::object([
            ("schema", Json::text("mesh.remote-panel-selection/v1")),
            ("id", Json::text(&self.id)),
            ("host", Json::text(&self.config.connection.host)),
            (
                "worker",
                Json::text(
                    self.config
                        .connection
                        .worker
                        .as_bytes()
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>(),
                ),
            ),
            ("objective", Json::text(&self.config.objective)),
            ("lane", Json::text(&self.config.lane)),
            ("run", Json::text(&self.config.run)),
        ])
    }
}
fn project(id: &str, result: RemoteObservationOutcome) -> Json {
    let mut fields = vec![
        ("schema", Json::text("mesh.remote-panel-observation/v2")),
        ("id", Json::text(id)),
    ];
    match result {
        RemoteObservationOutcome::CurrentLease(receipt) => {
            let facts = receipt.facts();
            fields.extend([
                ("kind", Json::text("status")),
                ("observed_ms", Json::text(receipt.observed_ms().to_string())),
                (
                    "admitted",
                    Json::Bool(!matches!(facts.get("admission"), None | Some(Json::Null))),
                ),
                (
                    "launch_recorded",
                    Json::Bool(!matches!(facts.get("launch"), None | Some(Json::Null))),
                ),
                (
                    "lease_until_ms",
                    facts
                        .get("effective_lease")
                        .and_then(|x| x.get("until_ms"))
                        .and_then(Json::as_u64)
                        .map_or(Json::Null, |n| Json::text(n.to_string())),
                ),
            ]);
        }
        RemoteObservationOutcome::Results(page) => {
            fields.extend([
                ("kind", Json::text("results")),
                ("available", Json::Bool(page.is_some())),
                (
                    "revision",
                    page.as_ref()
                        .map_or(Json::Null, |p| Json::text(p.revision.to_string())),
                ),
                (
                    "after",
                    page.as_ref().map_or(Json::Null, |p| Json::Number(p.after)),
                ),
                (
                    "count",
                    Json::Number(page.as_ref().map_or(0, |p| p.offers.len() as u64)),
                ),
                (
                    "entries",
                    Json::Array(page.as_ref().map_or_else(Vec::new, |p| {
                        p.offers
                            .iter()
                            .map(|offer| offer.public_summary())
                            .collect()
                    })),
                ),
                (
                    "has_more",
                    Json::Bool(page.as_ref().is_some_and(|p| p.has_more)),
                ),
            ]);
        }
    }
    Json::object(fields)
}

fn admit_selection(
    config: Configuration,
    host: &crate::attachment_host::AttachmentHost,
) -> Result<Selection, String> {
    let installation = ProtectedWorkspaceRoot::inspect(&config.connection.installation)
        .map_err(|_| UNAVAILABLE)?;
    let fleets =
        ProtectedWorkspaceRoot::inspect(&config.connection.fleets).map_err(|_| UNAVAILABLE)?;
    let history = host.configured_remote_history(&config.connection.fleets, &config.objective)?;
    let identity_file = setup::BoundPath::capture(&config.connection.identity, false)?;
    let hosts_file = setup::BoundPath::capture(&config.connection.known_hosts, false)?;
    let context = open_connection(&config.connection)?;
    let coordinator = context
        .installation
        .identity()
        .map_err(|_| UNAVAILABLE)?
        .worker();
    let mut bytes = [0; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| UNAVAILABLE)?;
    let selection = Selection {
        profile_source: None,
        identity_file,
        hosts_file,
        id: bytes.iter().map(|b| format!("{b:02x}")).collect(),
        config,
        peer: context.peer,
        installation,
        fleets,
        coordinator,
        history,
    };
    selection.verify()?;
    Ok(selection)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_bounded_read_operations_are_accepted() {
        assert!(kind("status", 0).is_ok());
        assert!(kind("results", 4096).is_ok());
        for (action, after) in [
            ("status", 1),
            ("results", 4097),
            ("start", 0),
            ("receive", 0),
            ("reconnect-input", 0),
            ("/tmp/config", 0),
        ] {
            assert!(kind(action, after).is_err());
        }
    }
    #[test]
    fn unsigned_panel_refuses_before_configuration_or_custody_access() {
        let panel = RemotePanel::default();
        assert_eq!(
            panel
                .select(
                    Path::new("/unused-private-configuration"),
                    &crate::attachment_host::AttachmentHost::new(Path::new(
                        "/unused-application-data"
                    ))
                )
                .unwrap_err(),
            ELIGIBILITY
        );
        assert_eq!(
            panel.read(&"a".repeat(64), "status", 0).unwrap_err(),
            ELIGIBILITY
        );
        assert!(panel.forget(&"a".repeat(64)).is_err());
    }
    #[test]
    fn result_projection_keeps_the_returned_cursor_after_a_nonzero_page() {
        let result = project(
            "selection",
            RemoteObservationOutcome::Results(Some(mesh_daemon::fleet::RemoteSavedResultPage {
                revision: 16,
                after: 16,
                has_more: false,
                offers: vec![],
            })),
        );
        assert_eq!(result.get("after"), Some(&Json::Number(16)));
        assert_eq!(result.get("entries"), Some(&Json::Array(vec![])));
        assert_eq!(
            result.get("schema"),
            Some(&Json::text("mesh.remote-panel-observation/v2"))
        );
    }
    #[test]
    fn missing_result_history_is_distinct_from_an_empty_verified_page() {
        let result = project("selection", RemoteObservationOutcome::Results(None));
        assert!(matches!(result.get("available"), Some(Json::Bool(false))));
        assert!(matches!(result.get("revision"), Some(Json::Null)));
        assert_eq!(result.get("entries"), Some(&Json::Array(vec![])));
    }
}
