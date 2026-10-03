//! Explicit continuation of a retained input transfer, never a new attempt or a lease renewal.
use super::*;
use mesh_daemon::fleet::{
    service::{RemoteHistoryInputRequest, RetainedRemoteInput},
    RemoteInputTransferOutcome,
};
impl RemotePanel {
    pub(crate) fn recover_original(&self, id: &str) -> Result<String, String> {
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
        let receipt = selected
            .history
            .recover_original_worker_over_ssh(
                &selected.peer,
                mesh_daemon::fleet::service::RemoteHistoryRecoveryRequest {
                    lane: &selected.config.lane,
                    run: &selected.config.run,
                    coordinator: selected.coordinator,
                    worker: selected.config.connection.worker,
                },
                Duration::from_secs(25),
                |payload| {
                    selected.verify()?;
                    context.installation.identity().map_err(|_| UNAVAILABLE)?;
                    let signature = context.custody.sign(payload).map_err(|_| UNAVAILABLE)?;
                    selected.verify()?;
                    context.installation.identity().map_err(|_| UNAVAILABLE)?;
                    Ok(signature)
                },
            )
            .map_err(|_| {
                "Original recovery requires reconciliation; inspect the original assignment"
            })?;
        selected.verify()?;
        context.installation.identity().map_err(|_| UNAVAILABLE)?;
        // Deliberately project only the authenticated outcome, not private proof or filesystem facts.
        let _ = receipt;
        Ok(Json::object([
            (
                "schema",
                Json::text("mesh.remote-panel-original-recovery/v1"),
            ),
            ("id", Json::text(id)),
            ("disposition", Json::text("initialization-recovered")),
            ("objective", Json::text(&selected.config.objective)),
            ("lane", Json::text(&selected.config.lane)),
            ("run", Json::text(&selected.config.run)),
        ])
        .encode())
    }
    pub(crate) fn reconnect_input(
        &self,
        id: &str,
        host: &crate::attachment_host::AttachmentHost,
    ) -> Result<String, String> {
        eligible()?;
        let held = self.0.try_lock().map_err(|_| BUSY)?;
        let selected = held.as_ref().filter(|s| s.id == id).ok_or(UNAVAILABLE)?;
        selected.verify()?;
        let project = match selected
            .history
            .retained_remote_input(&selected.config.lane, &selected.config.run)
            .map_err(|_| UNAVAILABLE)?
        {
            RetainedRemoteInput::Project { project, .. } => Some(host.review_history(&project)?),
            RetainedRemoteInput::Review(_) => None,
        };
        let source = selected
            .history
            .prepare_saved_remote_input(
                &selected.config.lane,
                &selected.config.run,
                project.as_ref(),
            )
            .map_err(|_| UNAVAILABLE)?;
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
    fn unsigned_original_recovery_refuses_before_selection_access() {
        assert_eq!(
            RemotePanel::default()
                .recover_original(&"a".repeat(64))
                .unwrap_err(),
            ELIGIBILITY
        );
    }
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
