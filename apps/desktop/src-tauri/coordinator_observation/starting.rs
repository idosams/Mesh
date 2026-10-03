//! Closed native creation and initial input transfer, plus read-only lost-output recovery.
use super::receiving::{closed, hexadecimal, path, text};
use super::*;
use mesh_daemon::{
    fleet::{
        catalog::{AttachedFleetRequest, FleetProviderPolicy},
        service::RemoteNativeStartRequest,
        Limits, RemoteAssignment, RemoteInputTransferOutcome,
    },
    project_attachment::{AttachmentStorage, RemoteStartRequest},
};
use std::time::{SystemTime, UNIX_EPOCH};
pub(super) struct StartConfiguration {
    pub(super) connection: ConnectionConfiguration,
    pub(super) storage: PathBuf,
    pub(super) project: String,
    pub(super) request: AttachedFleetRequest,
    pub(super) policy: FleetProviderPolicy,
    pub(super) lease_until_ms: u64,
}
pub(super) fn configuration(value: Json) -> Result<StartConfiguration, String> {
    closed(
        &value,
        &[
            "schema",
            "connection",
            "storage",
            "project",
            "request",
            "version",
            "goal",
            "provider",
            "limits",
            "lease_until_ms",
        ],
    )?;
    if text(&value, "schema")? != "mesh.coordinator-start-config/v1" {
        return Err(UNAVAILABLE.into());
    }
    let peer = value.get("connection").ok_or(UNAVAILABLE)?;
    closed(
        peer,
        &[
            "schema",
            "installation",
            "fleets",
            "host",
            "account",
            "port",
            "identity",
            "known_hosts",
            "worker",
        ],
    )?;
    if text(peer, "schema")? != "mesh.coordinator-peer-config/v1" {
        return Err(UNAVAILABLE.into());
    }
    let connection = connection_config(peer)?;
    let limit = value.get("limits").ok_or(UNAVAILABLE)?;
    closed(limit, &["lanes", "concurrency", "depth", "retries"])?;
    let number = |name| limit.get(name).and_then(Json::as_u64).ok_or(UNAVAILABLE);
    let limits = Limits {
        lanes: number("lanes")?,
        concurrency: number("concurrency")?,
        depth: number("depth")?,
        retries: number("retries")?,
    };
    let request = AttachedFleetRequest::new(
        text(&value, "request")?,
        text(&value, "goal")?,
        text(&value, "version")?,
        limits,
    )
    .map_err(|_| UNAVAILABLE)?;
    let provider = text(&value, "provider")?;
    let policy = FleetProviderPolicy::new(provider, &[provider.into()]).map_err(|_| UNAVAILABLE)?;
    let project = text(&value, "project")?;
    if !hexadecimal(project, 64) {
        return Err(UNAVAILABLE.into());
    }
    let lease_until_ms = value
        .get("lease_until_ms")
        .and_then(Json::as_u64)
        .filter(|n| *n != 0)
        .ok_or(UNAVAILABLE)?;
    Ok(StartConfiguration {
        connection,
        storage: path(&value, "storage")?,
        project: project.into(),
        request,
        policy,
        lease_until_ms,
    })
}
pub(super) fn validate_deadline(until: u64, now: u64) -> Result<(), String> {
    if until <= now || until.saturating_sub(now) > 3_600_000 {
        return Err(UNAVAILABLE.into());
    }
    Ok(())
}
pub(super) fn execute(path: &Path, inspect_only: bool) -> Result<Json, String> {
    let original = crate::worker_service::load_private_json(path)?;
    let selected = configuration(original.clone())?;
    // Recovery does not need the original project, a usable lease, SSH access or a signing key.
    if inspect_only {
        let directory = NativeFleetDirectory::open(
            &selected.connection.fleets,
            TrustedReviewers::default(),
            CheckpointRuntimeParameters::selected_defaults(),
        )
        .map_err(|_| UNAVAILABLE)?;
        return Ok(directory
            .attached_request_snapshot(&selected.request.request)
            .map_err(|_| UNAVAILABLE)?);
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .ok_or(UNAVAILABLE)?;
    validate_deadline(selected.lease_until_ms, now)?;
    // Persist the exact request before catalogue allocation, signing or transport. A lost output
    // cannot silently change peer, source, policy or deadline under the same idempotency key.
    let storage = AttachmentStorage::open(&selected.storage).map_err(|_| UNAVAILABLE)?;
    retain_configuration(&storage, &selected, &original)?;
    let NativeContext {
        installation,
        custody,
        peer,
        directory,
    } = open_context(&selected.connection)?;
    let attachment = storage.reopen(&selected.project).map_err(|_| UNAVAILABLE)?;
    let protected = [
        &selected.storage,
        &selected.connection.installation,
        attachment.project().root(),
        attachment.metadata_path(),
    ]
    .into_iter()
    .map(|path| ProtectedWorkspaceRoot::inspect(path).map_err(|_| UNAVAILABLE))
    .collect::<Result<Vec<_>, _>>()?;
    directory
        .verify_outside(&protected)
        .map_err(|_| UNAVAILABLE)?;
    let source = attachment
        .prepare_remote_input(&selected.request.version.to_string())
        .map_err(|_| UNAVAILABLE)?;
    // The catalogue admits a private allocation outside the original project. It owns destination
    // selection; no caller-authored manifest, lane path or renderer authority is accepted here.
    let service = directory
        .create_attached_with_providers(&attachment, &selected.request, &selected.policy)
        .map_err(|_| UNAVAILABLE)?;
    dispatch(
        &selected,
        &service,
        &source,
        NativeConnection {
            installation,
            custody,
            peer,
        },
    )
}
/// Reuse an admitted app-owned service; never open a second fleet catalogue for a UI command.
pub(super) fn dispatch(
    selected: &StartConfiguration,
    service: &mesh_daemon::fleet::service::FleetService,
    source: &mesh_daemon::fleet::RemoteInputSource,
    context: NativeConnection,
) -> Result<Json, String> {
    let NativeConnection {
        installation,
        custody,
        peer,
    } = context;
    let coordinator = installation.identity().map_err(|_| UNAVAILABLE)?.worker();
    let objective = service.objective().map_err(|_| UNAVAILABLE)?;
    let state = service.native_state().map_err(|_| UNAVAILABLE)?;
    let mut roots = state.lanes.values().filter(|lane| lane.parent.is_none());
    let lane = roots.next().ok_or(UNAVAILABLE)?.id.clone();
    if roots.next().is_some() {
        return Err(UNAVAILABLE.into());
    }
    let run = format!("start-{}", selected.request.request);
    let outcome = service.start_remote_input_over_ssh(&peer, RemoteNativeStartRequest {
        lane: &lane, run: &run,
        assignment: RemoteAssignment {
            id: format!("assignment-{}", selected.request.request),
            worker_key: hex(selected.connection.worker.as_bytes()),
            input: source.manifest().input(), bundle: source.manifest().bundle(),
            lease_sequence: 1, lease_until_ms: selected.lease_until_ms,
        }, source, coordinator, worker: selected.connection.worker,
    }, Duration::from_secs(25), |payload| {
        installation.identity().map_err(|_| UNAVAILABLE)?;
        custody.sign(payload).map_err(|_| UNAVAILABLE.into())
    }).map_err(|_| "Remote start outcome requires reconciliation; retain this configuration and use --coordinator created before any further action")?;
    installation.identity().map_err(|_| UNAVAILABLE)?;
    let (disposition, receipt) = match outcome {
        RemoteInputTransferOutcome::Materialized(receipt) => ("input-materialized", receipt),
        RemoteInputTransferOutcome::Retained(receipt) => ("input-retained", receipt),
    };
    Ok(Json::object([
        ("schema", Json::text("mesh.coordinator-start-result/v1")),
        ("request", Json::text(&selected.request.request)),
        ("objective", Json::text(objective)),
        ("lane", Json::text(lane)),
        ("run", Json::text(run)),
        ("disposition", Json::text(disposition)),
        ("correlation", receipt.correlation().clone()),
    ]))
}
fn retain_configuration(
    storage: &AttachmentStorage,
    selected: &StartConfiguration,
    original: &Json,
) -> Result<(), String> {
    storage.retain_remote_start_request(&RemoteStartRequest {
        request: selected.request.request.clone(),
        value: original.clone(),
    }).map_err(|_| "Original remote creation request could not be retained or differs from this retry; preserve it for reconciliation".into())
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
#[cfg(test)]
mod tests;
