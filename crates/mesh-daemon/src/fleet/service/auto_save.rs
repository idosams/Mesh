//! Ordinary private progress reuses native journal recovery and the existing Saved event.
use super::*;

/// One completed native observation. Partial native appends remain durable even when incomplete.
#[derive(Clone, Debug)]
pub struct WorkerProgressSave {
    /// Exact private version observed after capture; never a review or publication authority.
    pub version: RecordDigest,
    /// Number of authenticated changes captured in this call.
    pub saved_changes: u64,
    /// Whether final inventory and native recovery checks found a complete private version.
    pub complete: bool,
    /// The native version is retained but its fleet observation must be retried.
    pub observation_pending: bool,
    /// Redacted native reason for incomplete progress.
    pub issue: Option<&'static str>,
}

impl FleetService {
    pub(in crate::fleet) fn supports_progress_saving(&self) -> Result<bool, Unavailable> {
        Ok(self.lock()?.received.is_none())
    }

    /// Spawn only for an already authorized local worker. This does not launch or adopt a provider.
    pub(in crate::fleet) fn spawn_worker_progress_save(
        self: &Arc<Self>,
        credential: &AgentCredential,
    ) -> std::io::Result<std::thread::JoinHandle<Result<WorkerProgressSave, Unavailable>>> {
        let service = self.clone();
        let credential = AgentCredential(credential.transport_value().to_owned());
        std::thread::Builder::new()
            .name("mesh-worker-save".into())
            .spawn(move || service.save_worker_progress(&credential))
    }

    /// Capture observed local worker changes without creating an explicit handoff checkpoint.
    /// Working files are never rewritten. Unchanged polling does not append empty fleet events.
    pub fn save_worker_progress(
        &self,
        credential: &AgentCredential,
    ) -> Result<WorkerProgressSave, Unavailable> {
        self.save_worker_progress_then(credential, || {})
    }

    fn save_worker_progress_then(
        &self,
        credential: &AgentCredential,
        after_capture: impl FnOnce(),
    ) -> Result<WorkerProgressSave, Unavailable> {
        let key = token_key(credential.transport_value());
        let (grant, workspace) = {
            let mut inner = self.lock()?;
            if inner.received.is_some() {
                return Err(refusal("fleet-local-progress-required"));
            }
            inner.runtime.refresh().map_err(runtime_error)?;
            let grant = inner
                .grants
                .get(&key)
                .cloned()
                .ok_or_else(|| refusal("fleet-session-refused"))?;
            ensure_run(&inner, &grant.lane, &grant.run)?;
            let run = inner.runtime.state().lanes[&grant.lane]
                .runs
                .last()
                .unwrap();
            if !matches!(run.state, RunState::Running | RunState::Waiting) {
                return Err(refusal("fleet-progress-run-not-running"));
            }
            let workspace = inner
                .workspaces
                .get(&grant.lane)
                .cloned()
                .ok_or_else(|| refusal("fleet-lane-needs-reattachment"))?;
            (grant, workspace)
        };
        let initial = workspace
            .binding()
            .starting_version()
            .ok_or_else(|| refusal("fleet-progress-initial-version-unavailable"))?;
        let state = exact_state(&workspace)?;
        let signer = grant
            .signer
            .as_ref()
            .ok_or_else(|| refusal("fleet-checkpoint-signer-unavailable"))?;
        let public = signer.public_key();
        if RecordDigest::from_bytes(*public.as_bytes()).to_string() != grant.actor {
            return Err(refusal("fleet-checkpoint-signer-changed"));
        }
        let report = workspace
            .daemon()
            .checkpoint_agent_workspace(
                crate::AgentWorkspaceCheckpointRequest {
                    root: &state.root,
                    digest: &state.digest,
                    installation: &state.installation,
                    generation: &grant.generation,
                },
                public,
                |payload| {
                    drop(
                        self.try_checkpoint_authority(&key, &grant, &workspace)
                            .map_err(|_| "fleet-progress-authority-unavailable".to_owned())?,
                    );
                    let signature = signer.sign(payload)?;
                    drop(
                        self.try_checkpoint_authority(&key, &grant, &workspace)
                            .map_err(|_| "fleet-progress-authority-unavailable".to_owned())?,
                    );
                    Ok::<_, String>(signature)
                },
            )
            .map_err(|_| refusal("fleet-progress-needs-recovery"))?;
        let version = report
            .workspace
            .workspace_versions
            .last()
            .ok_or_else(|| refusal("fleet-progress-version-unavailable"))?
            .operation();
        let mut result = WorkerProgressSave {
            version,
            saved_changes: report.saved_changes.len() as u64,
            complete: report.complete,
            observation_pending: false,
            issue: report.issue,
        };
        after_capture();
        if !report.complete || initial == version {
            return Ok(result);
        }
        // A missed Saved acknowledgment is recoverable from the native journal on the next poll.
        // Pin the observed fold until the fleet event is recorded, preventing an older completion
        // from replacing a newer explicit checkpoint or background save in the projection.
        let recorded = (|| {
            let _authority = workspace
                .daemon()
                .lock_workspace_agent_setup(
                    &report.workspace.root,
                    &report.workspace.digest,
                    &report.workspace.installation,
                    &grant.generation,
                )
                .map_err(|_| refusal("fleet-progress-observation-stale"))?;
            let mut inner = self.try_checkpoint_authority(&key, &grant, &workspace)?;
            if inner.runtime.state().lanes[&grant.lane].saved != Some(version) {
                inner
                    .runtime
                    .record(
                        &progress_request(&grant, version),
                        Command::Saved {
                            lane: grant.lane.clone(),
                            run: grant.run.clone(),
                            version,
                        },
                    )
                    .map_err(runtime_error)?;
            }
            Ok::<_, Unavailable>(())
        })();
        if recorded.is_err() {
            result.observation_pending = true;
            result.issue = Some("fleet-progress-observation-pending");
        }
        Ok(result)
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests;

fn progress_request(grant: &Grant, version: RecordDigest) -> String {
    let identity = Json::object([
        ("domain", Json::text("mesh.worker-progress/v1")),
        ("lane", Json::text(&grant.lane)),
        ("run", Json::text(&grant.run)),
        ("version", Json::text(version.to_string())),
    ])
    .encode();
    format!(
        "progress-{}",
        Blake3::digest_bytes(identity.as_bytes()).to_hex()
    )
}
