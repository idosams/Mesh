//! Explicit continuation of a retained input transfer, never a new attempt or a lease renewal.
use super::*;
use mesh_daemon::fleet::{
    service::{RemoteHistoryInputRequest, RetainedRemoteInput},
    RemoteInputTransferOutcome,
};
impl RemotePanel {
    pub(crate) fn reconnect_input(
        &self,
        id: &str,
        host: &crate::attachment_host::AttachmentHost,
    ) -> Result<String, String> {
        eligible()?;
        let held = self.0.try_lock().map_err(|_| BUSY)?;
        let selected = held.as_ref().filter(|s| s.id == id).ok_or(UNAVAILABLE)?;
        selected.verify()?;
        let source = match selected
            .history
            .retained_remote_input(&selected.config.lane, &selected.config.run)
            .map_err(|_| UNAVAILABLE)?
        {
            RetainedRemoteInput::Project { project, version } => host
                .review_history(&project)?
                .prepare_remote_input(&version)
                .map_err(|_| UNAVAILABLE)?,
            RetainedRemoteInput::Review(selection) => selected
                .history
                .prepare_remote_review_input(&selection)
                .map_err(|_| UNAVAILABLE)?,
        };
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
        selected.verify()?;
        let outcome = selected
            .history
            .reconnect_remote_input_over_ssh(
                &selected.peer,
                RemoteHistoryInputRequest {
                    lane: &selected.config.lane,
                    run: &selected.config.run,
                    source: &source,
                    coordinator: selected.coordinator,
                    worker: selected.config.connection.worker,
                },
                Duration::from_secs(25),
                |payload| {
                    selected.verify()?;
                    context.installation.identity().map_err(|_| UNAVAILABLE)?;
                    context
                        .custody
                        .sign(payload)
                        .map_err(|_| UNAVAILABLE.into())
                },
            )
            .map_err(|_| {
                "Input transfer outcome needs reconciliation; preserve the original assignment"
            })?;
        // A presentation or identity failure must never dispatch a second transfer.
        selected.verify()?;
        context.installation.identity().map_err(|_| UNAVAILABLE)?;
        let disposition = match outcome {
            RemoteInputTransferOutcome::Materialized(_) => "input-materialized",
            RemoteInputTransferOutcome::Retained(_) => "input-retained",
        };
        Ok(Json::object([
            ("schema", Json::text("mesh.remote-panel-input-recovery/v1")),
            ("id", Json::text(id)),
            ("disposition", Json::text(disposition)),
            ("objective", Json::text(&selected.config.objective)),
            ("lane", Json::text(&selected.config.lane)),
            ("run", Json::text(&selected.config.run)),
        ])
        .encode())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsigned_recovery_refuses_before_host_or_source_access() {
        let panel = RemotePanel::default();
        let host = crate::attachment_host::AttachmentHost::new(Path::new("/unused-recovery-store"));
        assert_eq!(
            panel.reconnect_input(&"a".repeat(64), &host).unwrap_err(),
            ELIGIBILITY
        );
    }
}
