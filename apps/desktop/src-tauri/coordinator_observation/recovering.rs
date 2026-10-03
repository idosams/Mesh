//! Explicit native recovery; configuration only selects an existing assignment and trusted peer.
use super::*;
use mesh_daemon::fleet::service::RemoteHistoryRecoveryRequest;
pub(super) fn execute(path: &Path) -> Result<Json, String> {
    let selected = config(crate::worker_service::load_private_json(path)?)?;
    let NativeContext {
        installation,
        custody,
        peer,
        directory,
    } = open_context(&selected.connection)?;
    let coordinator = installation.identity().map_err(|_| UNAVAILABLE)?.worker();
    let history = directory
        .history(&selected.objective)
        .map_err(|_| UNAVAILABLE)?;
    let receipt = history.recover_original_worker_over_ssh(&peer, RemoteHistoryRecoveryRequest {
        lane: &selected.lane, run: &selected.run, coordinator, worker: selected.connection.worker,
    }, Duration::from_secs(25), |payload| {
        installation.identity().map_err(|_| UNAVAILABLE)?;
        let signature = custody.sign(payload).map_err(|_| UNAVAILABLE)?;
        installation.identity().map_err(|_| UNAVAILABLE)?;
        Ok(signature)
    }).map_err(|_| "Original recovery requires reconciliation; preserve the original assignment and inspect retained status")?;
    installation.identity().map_err(|_| UNAVAILABLE)?;
    Ok(Json::object([
        (
            "schema",
            Json::text("mesh.coordinator-original-recovery/v1"),
        ),
        ("disposition", Json::text("initialization-recovered")),
        ("correlation", receipt.correlation().clone()),
    ]))
}
