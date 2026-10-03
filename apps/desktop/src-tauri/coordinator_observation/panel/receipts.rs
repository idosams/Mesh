//! Explicit exact-offer download and retained-intent recovery through the app's native owners.
use super::*;
use mesh_daemon::fleet::{
    service::{RemoteHistoryIngestionRequest, RetainedRemoteInput},
    RemoteReceiptIntent,
};
const UNCERTAIN: &str =
    "Download outcome needs reconciliation; retain the original receipt selection";
fn action_valid(action: &str, offer: &str) -> bool {
    (action == "list" && offer.is_empty())
        || (matches!(action, "receive" | "recover")
            && super::super::receiving::hexadecimal(offer, 64))
}
fn entry(intent: &RemoteReceiptIntent) -> Result<Json, String> {
    let offer = Json::parse(intent.offer()).map_err(|_| UNAVAILABLE)?;
    let body = offer.get("body").ok_or(UNAVAILABLE)?;
    Ok(Json::object([
        ("offer", Json::text(intent.id())),
        (
            "checkpoint",
            body.get("checkpoint").ok_or(UNAVAILABLE)?.clone(),
        ),
        ("version", body.get("version").ok_or(UNAVAILABLE)?.clone()),
    ]))
}
impl RemotePanel {
    pub(crate) fn receipt(
        &self,
        id: &str,
        action: &str,
        offer: &str,
        host: &crate::attachment_host::AttachmentHost,
    ) -> Result<String, String> {
        if !action_valid(action, offer) {
            return Err(UNAVAILABLE.into());
        }
        eligible()?;
        let held = self.0.try_lock().map_err(|_| BUSY)?;
        let selected = held.as_ref().filter(|s| s.id == id).ok_or(UNAVAILABLE)?;
        selected.verify()?;
        let original = profiles::value(selected)?;
        let protected = [selected.installation, selected.fleets];
        if action == "list" {
            let inbox = host.remote_result_inbox(&protected, false)?;
            let intents = inbox
                .as_ref()
                .map(|i| i.receipt_intents())
                .transpose()
                .map_err(|_| UNCERTAIN)?
                .unwrap_or_default();
            let entries = intents
                .iter()
                .filter(|i| i.context() == &original)
                .map(entry)
                .collect::<Result<Vec<_>, _>>()?;
            selected.verify()?;
            return Ok(Json::object([
                (
                    "schema",
                    Json::text("mesh.remote-panel-receipt-attempts/v1"),
                ),
                ("id", Json::text(id)),
                ("entries", Json::Array(entries)),
            ])
            .encode());
        }
        // Only a signed offer retained from this selection's last authenticated page can start
        // a new receipt. Recovery uses the original native intent, never a renderer-authored offer.
        let cached = if action == "receive" {
            Some(selected.offers.get(offer).ok_or(UNAVAILABLE)?)
        } else {
            None
        };
        let saved_inbox = if action == "recover" {
            Some(
                host.remote_result_inbox(&protected, false)?
                    .ok_or(UNCERTAIN)?,
            )
        } else {
            None
        };
        let saved = saved_inbox
            .as_ref()
            .map(|inbox| {
                inbox
                    .receipt_intents()
                    .map_err(|_| UNCERTAIN)?
                    .into_iter()
                    .find(|i| i.id() == offer && i.context() == &original)
                    .ok_or(UNCERTAIN)
            })
            .transpose()?;
        let project = match selected
            .history
            .saved_remote_input(&selected.config.lane, &selected.config.run)
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
        let inbox = match saved_inbox {
            Some(inbox) => inbox,
            None => host
                .remote_result_inbox(&protected, true)?
                .ok_or(UNCERTAIN)?,
        };
        let intent = match saved {
            Some(intent) => intent,
            None => inbox
                .retain_receipt_intent(cached.ok_or(UNAVAILABLE)?, &original)
                .map_err(|_| UNCERTAIN)?,
        };
        if intent.id() != offer {
            return Err(UNAVAILABLE.into());
        }
        let trusted = TrustedReviewers::default();
        let receipt = selected
            .history
            .ingest_remote_result_over_ssh(
                &selected.peer,
                RemoteHistoryIngestionRequest {
                    lane: &selected.config.lane,
                    run: &selected.config.run,
                    coordinator: selected.coordinator,
                    worker: selected.config.connection.worker,
                    input: source.manifest(),
                    offer: intent.offer(),
                    destination: inbox.destination().map_err(|_| UNCERTAIN)?,
                    allocation: intent.allocation(),
                    reviewers: &trusted,
                    checkpoint: CheckpointRuntimeParameters::selected_defaults(),
                    actor: selected.coordinator,
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
            .map_err(|_| UNCERTAIN)?;
        selected.verify()?;
        context.installation.identity().map_err(|_| UNAVAILABLE)?;
        inbox.destination().map_err(|_| UNCERTAIN)?;
        Ok(Json::object([
            ("schema", Json::text("mesh.remote-panel-received-result/v1")),
            ("id", Json::text(id)),
            ("offer", Json::text(offer)),
            ("objective", Json::text(&selected.config.objective)),
            ("lane", Json::text(&selected.config.lane)),
            ("run", Json::text(&selected.config.run)),
            ("correlation", Json::text(receipt.digest().to_string())),
            ("version", Json::text(receipt.version().to_string())),
            ("review", Json::text(receipt.review().to_string())),
        ])
        .encode())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn receipt_actions_refuse_paths_unknown_actions_and_unsigned_app_before_storage_access() {
        for (action, offer) in [
            ("list", "/tmp/path"),
            ("receive", "../result"),
            ("start", ""),
            ("recover", ""),
        ] {
            assert!(!action_valid(action, offer));
        }
        let panel = RemotePanel::default();
        let host = crate::attachment_host::AttachmentHost::new(Path::new("/unused-receipt-host"));
        for (action, offer) in [
            ("list", String::new()),
            ("receive", "a".repeat(64)),
            ("recover", "a".repeat(64)),
        ] {
            assert_eq!(
                panel
                    .receipt("selection", action, &offer, &host)
                    .unwrap_err(),
                ELIGIBILITY
            );
        }
    }
    #[test]
    fn desktop_host_reopens_the_same_native_inbox_without_creating_on_read() {
        use std::os::unix::fs::PermissionsExt as _;
        let root =
            std::env::temp_dir().join(format!("mesh-desktop-receipt-host-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        let host = crate::attachment_host::AttachmentHost::new(&root);
        assert!(host.remote_result_inbox(&[], false).unwrap().is_none());
        assert!(!root.join("attached-projects").exists());
        let inbox = host.remote_result_inbox(&[], true).unwrap().unwrap();
        let identity = inbox.identity().unwrap();
        drop(inbox);
        drop(host);
        let host = crate::attachment_host::AttachmentHost::new(&root);
        assert_eq!(
            host.remote_result_inbox(&[], false)
                .unwrap()
                .unwrap()
                .identity()
                .unwrap(),
            identity
        );
        let parent = ProtectedWorkspaceRoot::inspect(&root).unwrap();
        assert!(host.remote_result_inbox(&[parent], false).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
