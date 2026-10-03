//! Fresh remote requests use the app's existing native owners. No renderer paths or auto retries.
use super::super::starting;
use super::*;
use mesh_daemon::project_attachment::RemoteStartRequest;
const SCHEMA: &str = "mesh.desktop-remote-creation/v1";
fn now() -> Result<u64, String> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .ok_or_else(|| UNAVAILABLE.into())
}
fn decode(record: &RemoteStartRequest) -> Result<(starting::StartConfiguration, Json), String> {
    receiving::closed(&record.value, &["schema", "configuration", "bindings"])?;
    if receiving::text(&record.value, "schema")? != SCHEMA {
        return Err(UNAVAILABLE.into());
    }
    let selected = starting::configuration(
        record
            .value
            .get("configuration")
            .ok_or(UNAVAILABLE)?
            .clone(),
    )?;
    if selected.request.request != record.request {
        return Err(UNAVAILABLE.into());
    }
    Ok((
        selected,
        record.value.get("bindings").ok_or(UNAVAILABLE)?.clone(),
    ))
}
fn bindings(config: &ConnectionConfiguration, coordinator: PublicKey) -> Result<Json, String> {
    Ok(Json::object([
        (
            "installation",
            Json::text(
                ProtectedWorkspaceRoot::inspect(&config.installation)
                    .map_err(|_| UNAVAILABLE)?
                    .directory_token(),
            ),
        ),
        (
            "fleets",
            Json::text(
                ProtectedWorkspaceRoot::inspect(&config.fleets)
                    .map_err(|_| UNAVAILABLE)?
                    .directory_token(),
            ),
        ),
        (
            "identity",
            setup::BoundPath::capture(&config.identity, false)?.fingerprint(),
        ),
        (
            "hosts",
            setup::BoundPath::capture(&config.known_hosts, false)?.fingerprint(),
        ),
        (
            "coordinator",
            Json::text(
                coordinator
                    .as_bytes()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>(),
            ),
        ),
    ]))
}
fn public(record: &RemoteStartRequest) -> Result<Json, String> {
    let (s, _) = decode(record)?;
    Ok(Json::object([
        ("request", Json::text(&record.request)),
        (
            "limits",
            record
                .value
                .get("configuration")
                .and_then(|c| c.get("limits"))
                .ok_or(UNAVAILABLE)?
                .clone(),
        ),
        ("project", Json::text(s.project)),
        ("version", Json::text(s.request.version.to_string())),
        ("goal", Json::text(s.request.goal)),
        ("provider", Json::text(s.policy.coordinator())),
        ("host", Json::text(s.connection.host)),
        (
            "worker",
            Json::text(
                s.connection
                    .worker
                    .as_bytes()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>(),
            ),
        ),
        ("lease_until_ms", Json::text(s.lease_until_ms.to_string())),
    ]))
}
fn entries(
    host: &crate::attachment_host::AttachmentHost,
) -> Result<Vec<RemoteStartRequest>, String> {
    host.remote_start_requests()?
        .into_iter()
        .filter(|r| {
            r.value.get("schema").and_then(Json::as_text)
                != Some("mesh.coordinator-start-config/v1")
        })
        .map(|r| {
            decode(&r)?;
            Ok(r)
        })
        .collect()
}
fn configured_request(
    peer: Json,
    input: &Json,
    request: &str,
    storage: &Path,
    deadline: u64,
) -> Result<Json, String> {
    receiving::closed(
        input,
        &[
            "connection",
            "project",
            "version",
            "goal",
            "provider",
            "limits",
        ],
    )?;
    let mut fields = vec![
        ("schema", Json::text("mesh.coordinator-start-config/v1")),
        ("connection", peer),
        ("storage", Json::text(storage.to_str().ok_or(UNAVAILABLE)?)),
        ("request", Json::text(request)),
        ("lease_until_ms", Json::Number(deadline)),
    ];
    for key in ["project", "version", "goal", "provider", "limits"] {
        fields.push((key, input.get(key).ok_or(UNAVAILABLE)?.clone()));
    }
    let value = Json::object(fields);
    if starting::configuration(value.clone())?
        .request
        .limits
        .retries
        != 0
    {
        return Err(UNAVAILABLE.into());
    }
    Ok(value)
}
impl RemotePanel {
    pub(crate) fn creation(
        &self,
        action: &str,
        id: &str,
        input: &str,
        host: &crate::attachment_host::AttachmentHost,
    ) -> Result<String, String> {
        eligible()?;
        let draft = self.1.try_lock().map_err(|_| BUSY)?;
        if action == "list" && id.is_empty() && input.is_empty() {
            return Ok(Json::object([
                ("schema", Json::text("mesh.remote-creation-list/v1")),
                (
                    "entries",
                    Json::Array(
                        entries(host)?
                            .iter()
                            .map(public)
                            .collect::<Result<Vec<_>, _>>()?,
                    ),
                ),
            ])
            .encode());
        }
        if action == "prepare" {
            if input.len() > 16_384 {
                return Err(UNAVAILABLE.into());
            }
            let value = Json::parse(input).map_err(|_| UNAVAILABLE)?;
            let peer = draft.peer_configuration(
                id,
                value.get("connection").ok_or(UNAVAILABLE)?,
                host.remote_fleet_storage_path(),
            )?;
            let request = &id[..32]; // peer_configuration verified the native 64-character draft.
            let old = entries(host)?.into_iter().find(|r| r.request == request);
            let deadline = match &old {
                Some(r) => decode(r)?.0.lease_until_ms,
                None => now()?.checked_add(900_000).ok_or(UNAVAILABLE)?,
            };
            let configuration = configured_request(
                peer,
                &value,
                request,
                host.remote_start_storage_path(),
                deadline,
            )?;
            let selected = starting::configuration(configuration.clone())?;
            let source = host.review_history(&selected.project)?;
            source
                .prepare_remote_input(&selected.request.version.to_string())
                .map_err(|_| UNAVAILABLE)?;
            let context = open_connection(&selected.connection)?;
            let protected = [
                &selected.storage,
                &selected.connection.installation,
                source.project().root(),
                source.metadata_path(),
            ]
            .into_iter()
            .map(|p| ProtectedWorkspaceRoot::inspect(p).map_err(|_| UNAVAILABLE))
            .collect::<Result<Vec<_>, _>>()?;
            host.with_fleets(true, |d| {
                d.ok_or(UNAVAILABLE)?
                    .verify_outside(&protected)
                    .map_err(|_| UNAVAILABLE.into())
            })?;
            let bound = bindings(
                &selected.connection,
                context
                    .installation
                    .identity()
                    .map_err(|_| UNAVAILABLE)?
                    .worker(),
            )?;
            let record = RemoteStartRequest {
                request: request.into(),
                value: Json::object([
                    ("schema", Json::text(SCHEMA)),
                    ("configuration", configuration),
                    ("bindings", bound),
                ]),
            };
            // Original bindings and deadline must survive repeated preparation without widening.
            if old.as_ref().is_some_and(|old| old != &record) {
                return Err(UNAVAILABLE.into());
            }
            draft.peer_configuration(
                id,
                value.get("connection").ok_or(UNAVAILABLE)?,
                host.remote_fleet_storage_path(),
            )?;
            host.retain_remote_start_request(&record)?;
            return Ok(Json::object([
                ("schema", Json::text("mesh.remote-creation-prepared/v1")),
                ("draft", Json::text(id)),
                ("entry", public(&record)?),
            ])
            .encode());
        }
        if !matches!(action, "inspect" | "send")
            || !input.is_empty()
            || !receiving::hexadecimal(id, 32)
        {
            return Err(UNAVAILABLE.into());
        }
        let record = entries(host)?
            .into_iter()
            .find(|r| r.request == id)
            .ok_or(UNAVAILABLE)?;
        let (selected, original) = decode(&record)?;
        if selected.storage != host.remote_start_storage_path()
            || selected.connection.fleets != host.remote_fleet_storage_path()
        {
            return Err(UNAVAILABLE.into());
        }
        let context = open_connection(&selected.connection)?;
        let coordinator = context
            .installation
            .identity()
            .map_err(|_| UNAVAILABLE)?
            .worker();
        if bindings(&selected.connection, coordinator)? != original {
            return Err(UNAVAILABLE.into());
        }
        if action == "send" {
            starting::validate_deadline(selected.lease_until_ms, now()?)?;
            let source = host.review_history(&selected.project)?;
            let input = source
                .prepare_remote_input(&selected.request.version.to_string())
                .map_err(|_| UNAVAILABLE)?;
            let protected = [
                &selected.storage,
                &selected.connection.installation,
                source.project().root(),
                source.metadata_path(),
            ]
            .into_iter()
            .map(|p| ProtectedWorkspaceRoot::inspect(p).map_err(|_| UNAVAILABLE))
            .collect::<Result<Vec<_>, _>>()?;
            let service = host.with_fleets(false, |d| {
                let d = d.ok_or(UNAVAILABLE)?;
                d.verify_outside(&protected).map_err(|_| UNAVAILABLE)?;
                d.create_attached_with_providers(&source, &selected.request, &selected.policy)
                    .map_err(|_| UNAVAILABLE.into())
            })?;
            if bindings(&selected.connection, coordinator)? != original {
                return Err(UNAVAILABLE.into());
            }
            // The existing service rejects every already-dispatched attempt, even identical retries.
            let outcome = starting::dispatch(&selected, &service, &input, context)?;
            if bindings(&selected.connection, coordinator)? != original {
                return Err(UNAVAILABLE.into());
            }
            return Ok(Json::object([
                ("schema", Json::text("mesh.remote-creation-sent/v1")),
                ("request", Json::text(id)),
                (
                    "disposition",
                    outcome.get("disposition").ok_or(UNAVAILABLE)?.clone(),
                ),
            ])
            .encode());
        }
        let snapshot = host.with_fleets(false, |d| {
            d.ok_or(UNAVAILABLE)?
                .attached_request_snapshot(id)
                .map_err(|_| UNAVAILABLE.into())
        })?;
        let fleet = snapshot.get("fleet").ok_or(UNAVAILABLE)?;
        let mut selection = Json::Null;
        if let Some(state) = fleet.get("state") {
            if let Some(lanes) = state.get("lanes").and_then(Json::as_array) {
                let mut roots = lanes
                    .iter()
                    .filter(|l| l.get("parent") == Some(&Json::Null));
                if let Some(root) = roots.next() {
                    if roots.next().is_some() {
                        return Err(UNAVAILABLE.into());
                    }
                    let run = format!("start-{id}");
                    if root
                        .get("run")
                        .and_then(|r| r.get("id"))
                        .and_then(Json::as_text)
                        == Some(&run)
                    {
                        let configured = Configuration {
                            connection: selected.connection,
                            objective: receiving::text(fleet, "objective")?.into(),
                            lane: receiving::text(root, "id")?.into(),
                            run,
                        };
                        let next = admit_selection(configured, host)?;
                        if bindings(&next.config.connection, next.coordinator)? != original {
                            return Err(UNAVAILABLE.into());
                        }
                        selection = next.render();
                        *self.0.try_lock().map_err(|_| BUSY)? = Some(next);
                    }
                }
            }
        }
        Ok(Json::object([
            ("schema", Json::text("mesh.remote-creation-inspected/v1")),
            ("request", Json::text(id)),
            ("allocated", Json::Bool(fleet != &Json::Null)),
            ("selection", selection),
        ])
        .encode())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> Json {
        Json::object([
            ("connection", Json::Null),
            ("project", Json::text("a".repeat(64))),
            ("version", Json::text("b".repeat(64))),
            ("goal", Json::text("Keep ordinary project work unchanged")),
            ("provider", Json::text("codex")),
            (
                "limits",
                Json::object([
                    ("lanes", Json::Number(4)),
                    ("concurrency", Json::Number(2)),
                    ("depth", Json::Number(1)),
                    ("retries", Json::Number(0)),
                ]),
            ),
        ])
    }
    fn peer() -> Json {
        Json::object([
            ("schema", Json::text("mesh.coordinator-peer-config/v1")),
            ("installation", Json::text("/private/installation")),
            ("fleets", Json::text("/private/fleets")),
            ("identity", Json::text("/private/key")),
            ("known_hosts", Json::text("/private/trust")),
            ("host", Json::text("worker.example")),
            ("account", Json::text("mesh")),
            ("port", Json::Number(22)),
            ("worker", Json::text("c".repeat(64))),
        ])
    }
    #[test]
    fn fresh_configuration_needs_no_existing_attempt_and_rejects_renderer_authority() {
        let v = configured_request(
            peer(),
            &input(),
            &"d".repeat(32),
            Path::new("/private/storage"),
            900001,
        )
        .unwrap();
        let s = starting::configuration(v.clone()).unwrap();
        assert_eq!(s.request.request, "d".repeat(32));
        assert_eq!(s.lease_until_ms, 900001);
        assert!(s.connection.fleets.is_absolute());
        for key in ["request", "storage", "manifest", "lease_until_ms"] {
            let Json::Object(mut fields) = input() else {
                unreachable!()
            };
            fields.push((key.into(), Json::text("forged")));
            assert!(configured_request(
                peer(),
                &Json::Object(fields),
                &"d".repeat(32),
                Path::new("/private/storage"),
                900001
            )
            .is_err());
        }
        let record = RemoteStartRequest {
            request: "d".repeat(32),
            value: Json::object([
                ("schema", Json::text(SCHEMA)),
                ("configuration", v),
                ("bindings", Json::object([] as [(&str, Json); 0])),
            ]),
        };
        let projected = public(&record).unwrap().encode();
        assert!(projected.contains("worker.example"));
        assert!(projected.contains("limits"));
        assert!(!projected.contains("/private"));
        assert!(!projected.contains("known_hosts"));
        let changed = RemoteStartRequest {
            request: "e".repeat(32),
            ..record
        };
        assert!(decode(&changed).is_err());
    }
    #[test]
    fn unsigned_creation_commands_refuse_before_storage_and_network() {
        let panel = RemotePanel::default();
        let host =
            crate::attachment_host::AttachmentHost::new(Path::new("/unused-creation-storage"));
        for action in ["prepare", "list", "inspect", "send"] {
            assert_eq!(
                panel.creation(action, "", "", &host).unwrap_err(),
                ELIGIBILITY
            );
        }
    }
}
