//! Explicit original-assignment input reconnect. Configuration cannot replace a lease or run.
use super::receiving::{closed, input_selection, input_source, text, Input};
use super::*;
use mesh_daemon::fleet::{service::RemoteHistoryInputRequest, RemoteInputTransferOutcome};
struct ReconnectConfiguration {
    connection: Configuration,
    input: Input,
}
fn configuration(value: Json) -> Result<ReconnectConfiguration, String> {
    closed(&value, &["schema", "connection", "input"])?;
    if text(&value, "schema")? != "mesh.coordinator-input-reconnect-config/v1" {
        return Err(UNAVAILABLE.into());
    }
    Ok(ReconnectConfiguration {
        connection: config(value.get("connection").ok_or(UNAVAILABLE)?.clone())?,
        input: input_selection(value.get("input").ok_or(UNAVAILABLE)?)?,
    })
}
pub(super) fn execute(path: &Path) -> Result<Json, String> {
    let selected = configuration(crate::worker_service::load_private_json(path)?)?;
    let NativeContext {
        installation,
        custody,
        peer,
        directory,
    } = open_context(&selected.connection.connection)?;
    let coordinator = installation.identity().map_err(|_| UNAVAILABLE)?.worker();
    let history = directory
        .history(&selected.connection.objective)
        .map_err(|_| UNAVAILABLE)?;
    let source = input_source(&selected.input, &history, &mut Vec::new())?;
    let outcome = history.reconnect_remote_input_over_ssh(&peer, RemoteHistoryInputRequest {
        lane: &selected.connection.lane, run: &selected.connection.run, source: &source,
        coordinator, worker: selected.connection.connection.worker,
    }, Duration::from_secs(25), |payload| {
        installation.identity().map_err(|_| UNAVAILABLE)?;
        custody.sign(payload).map_err(|_| UNAVAILABLE.into())
    }).map_err(|_| "Input reconnect requires reconciliation; preserve the original assignment and configuration")?;
    installation.identity().map_err(|_| UNAVAILABLE)?;
    let (disposition, receipt) = match outcome {
        RemoteInputTransferOutcome::Materialized(receipt) => ("input-materialized", receipt),
        RemoteInputTransferOutcome::Retained(receipt) => ("input-retained", receipt),
    };
    Ok(Json::object([
        ("schema", Json::text("mesh.coordinator-input-reconnect/v1")),
        ("disposition", Json::text(disposition)),
        ("correlation", receipt.correlation().clone()),
    ]))
}
#[cfg(test)]
mod tests;
