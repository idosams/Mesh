//! Local signed capture without holding the shared fleet mutex across native work.
//! Received sessions retain their existing authority path until separately validated.
use super::*;

impl FleetService {
    pub(super) fn local_checkpoint(
        &self,
        credential: &str,
        arguments: &Json,
    ) -> Result<Json, Unavailable> {
        if credential.len() != 64 || !credential.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(refusal("fleet-session-refused"));
        }
        exact_fields(arguments, &["request"])?;
        let request = field(arguments, "request")?;
        let key = token_key(credential);
        let (grant, workspace, id) = {
            let mut inner = self.lock()?;
            inner.runtime.refresh().map_err(runtime_error)?;
            let grant = inner
                .grants
                .get(&key)
                .cloned()
                .ok_or_else(|| refusal("fleet-session-refused"))?;
            ensure_run(&inner, &grant.lane, &grant.run)?;
            let workspace = inner
                .workspaces
                .get(&grant.lane)
                .cloned()
                .ok_or_else(|| refusal("fleet-lane-needs-reattachment"))?;
            let id = format!(
                "checkpoint-{}",
                &lane_identity(inner.runtime.objective(), &grant.lane, request)?[5..]
            );
            (grant, workspace, id)
        };
        let state = exact_state(&workspace)?;
        verify_custody(&workspace, &state, &grant.generation)?;
        {
            let mut inner = self.lock()?;
            validate(&mut inner, &key, &grant, &workspace)?;
            if let Some(previous) = previous(&inner, &id, &grant)? {
                return Ok(previous);
            }
        }
        let signer = grant
            .signer
            .as_ref()
            .ok_or_else(|| refusal("fleet-checkpoint-signer-unavailable"))?;
        let public = signer.public_key();
        if RecordDigest::from_bytes(*public.as_bytes()).to_string() != grant.actor {
            return Err(refusal("fleet-checkpoint-signer-changed"));
        }
        {
            let _authority = workspace
                .daemon()
                .lock_workspace_agent_setup(
                    &state.root,
                    &state.digest,
                    &state.installation,
                    &grant.generation,
                )
                .map_err(|_| refusal("fleet-session-custody-changed"))?;
            // Other routes may hold the fleet mutex while waiting for this native custody.
            // Never wait for that mutex while native custody is held.
            let mut inner = self.try_checkpoint_authority(&key, &grant, &workspace)?;
            if let Some(previous) = previous(&inner, &id, &grant)? {
                return Ok(previous);
            }
            inner
                .runtime
                .record(
                    &format!("begin-{id}"),
                    Command::BeginCheckpoint {
                        id: id.clone(),
                        lane: grant.lane.clone(),
                        origin: origin(&grant),
                        input_digest: state.digest.clone(),
                    },
                )
                .map_err(runtime_error)?;
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
                            .map_err(|_| "fleet-checkpoint-authority-unavailable".to_owned())?,
                    );
                    let signature = signer.sign(payload)?;
                    // Cancellation or rotation while the signer waited cannot authorize this payload.
                    drop(
                        self.try_checkpoint_authority(&key, &grant, &workspace)
                            .map_err(|_| "fleet-checkpoint-authority-unavailable".to_owned())?,
                    );
                    Ok::<_, String>(signature)
                },
            )
            .map_err(|_| refusal("fleet-checkpoint-needs-recovery"))?;
        let result = super::super::CheckpointResult {
            complete: report.complete,
            version: report
                .workspace
                .workspace_versions
                .last()
                .ok_or_else(|| refusal("fleet-checkpoint-version-unavailable"))?
                .operation(),
            workspace_digest: report.workspace.digest.clone(),
            saved_changes: report.saved_changes.len() as u64,
            issue: report.issue.map(str::to_owned),
        };
        // Native capture has released its locks. Record truthful durable partial progress even
        // after cancellation/revocation; this acknowledges an existing begin, not fresh authority.
        self.lock()?
            .runtime
            .record(
                &format!("finish-{id}"),
                Command::FinishCheckpoint {
                    id: id.clone(),
                    result: result.clone(),
                },
            )
            .map_err(runtime_error)?;
        Ok(checkpoint_summary(&id, &result))
    }

    pub(super) fn try_checkpoint_authority(
        &self,
        key: &str,
        grant: &Grant,
        workspace: &Arc<LaneWorkspace>,
    ) -> Result<MutexGuard<'_, Inner>, Unavailable> {
        let mut inner = self
            .inner
            .try_lock()
            .map_err(|_| refusal("fleet-checkpoint-authority-busy"))?;
        validate(&mut inner, key, grant, workspace)?;
        Ok(inner)
    }
}

fn validate(
    inner: &mut Inner,
    key: &str,
    grant: &Grant,
    workspace: &Arc<LaneWorkspace>,
) -> Result<(), Unavailable> {
    if inner.received.is_some() {
        return Err(refusal("fleet-local-checkpoint-required"));
    }
    inner.runtime.refresh().map_err(runtime_error)?;
    let current = inner
        .grants
        .get(key)
        .ok_or_else(|| refusal("fleet-session-refused"))?;
    if current.lane != grant.lane
        || current.run != grant.run
        || current.actor != grant.actor
        || current.session != grant.session
        || current.generation != grant.generation
        || !inner
            .workspaces
            .get(&grant.lane)
            .is_some_and(|held| Arc::ptr_eq(held, workspace))
    {
        return Err(refusal("fleet-session-custody-changed"));
    }
    ensure_run(inner, &grant.lane, &grant.run)
}

fn origin(grant: &Grant) -> super::super::AgentOrigin {
    super::super::AgentOrigin {
        actor: grant.actor.clone(),
        session: grant.session.clone(),
        run: grant.run.clone(),
        generation: grant.generation.clone(),
    }
}
fn previous(inner: &Inner, id: &str, grant: &Grant) -> Result<Option<Json>, Unavailable> {
    let Some(previous) = inner.runtime.state().checkpoints.get(id) else {
        return Ok(None);
    };
    if previous.lane != grant.lane || previous.origin != origin(grant) {
        return Err(refusal("fleet-checkpoint-request-conflict"));
    }
    previous
        .result
        .as_ref()
        .map(|result| Some(checkpoint_summary(id, result)))
        .ok_or_else(|| refusal("fleet-checkpoint-needs-recovery"))
}

#[cfg(all(test, target_os = "macos"))]
mod tests;
