//! Read-only worker inventory outside the shared fleet lock. Snapshots never authorize a save.
use super::*;
use crate::AgentFinishPreflight;

/// Advisory classification of one exact assigned worker folder at inspection time.
/// A later save must independently revalidate its credential, custody and current contents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkerProgress {
    /// No supported edits or additions were found.
    Unchanged,
    /// Supported edits or additions were observed; no content has been saved by this read.
    Changed,
    /// Missing or unsupported entries need explicit resolution before a complete checkpoint.
    NeedsResolution,
}

impl FleetService {
    /// Inspect ordinary worker progress without appending fleet events or capturing content.
    /// This native-only read accepts an issued credential, never a caller-supplied lane or path.
    pub fn inspect_worker_progress(
        &self,
        credential: &AgentCredential,
    ) -> Result<WorkerProgress, Unavailable> {
        let inventory = self.inspect_agent_inventory(credential.transport_value())?;
        if !inventory.missing_files().is_empty() || !inventory.unsupported_entries().is_empty() {
            Ok(WorkerProgress::NeedsResolution)
        } else if inventory
            .managed_files()
            .iter()
            .any(|file| file.modified_from_current_version())
            || !inventory.native_files().is_empty()
            || !inventory.native_directories().is_empty()
        {
            Ok(WorkerProgress::Changed)
        } else {
            Ok(WorkerProgress::Unchanged)
        }
    }

    fn inspect_agent_inventory(
        &self,
        credential: &str,
    ) -> Result<AgentFinishPreflight, Unavailable> {
        self.inspect_agent_inventory_using(credential, inspect)
    }

    fn inspect_agent_inventory_using(
        &self,
        credential: &str,
        inspect: impl FnOnce(&LaneWorkspace, &Grant) -> Result<AgentFinishPreflight, Unavailable>,
    ) -> Result<AgentFinishPreflight, Unavailable> {
        if credential.len() != 64 || !credential.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(refusal("fleet-session-refused"));
        }
        let key = token_key(credential);
        let (grant, workspace) = {
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
            (grant, workspace)
        };
        // Native custody and full filesystem reads may wait. Neither holds the fleet mutex.
        let inventory = inspect(&workspace, &grant)?;
        let mut inner = self.lock()?;
        inner.runtime.refresh().map_err(runtime_error)?;
        // Tokens are never reused. Rotation or revocation removes this exact grant.
        let current = inner
            .grants
            .get(&key)
            .ok_or_else(|| refusal("fleet-session-refused"))?;
        ensure_run(&inner, &current.lane, &current.run)?;
        if current.lane != grant.lane
            || current.run != grant.run
            || current.generation != grant.generation
            || !inner
                .workspaces
                .get(&grant.lane)
                .is_some_and(|held| Arc::ptr_eq(held, &workspace))
        {
            return Err(refusal("fleet-session-custody-changed"));
        }
        Ok(inventory)
    }

    pub(super) fn agent_missing_files(&self, credential: &str) -> Result<Json, Unavailable> {
        let inventory = self.inspect_agent_inventory(credential)?;
        Ok(Json::object([
            ("schema", Json::text("mesh.fleet-missing-files/v1")),
            (
                "files",
                Json::Array(
                    inventory
                        .missing_files()
                        .iter()
                        .take(128)
                        .map(|file| {
                            Json::object([
                                ("path", Json::text(file.path())),
                                ("version", Json::text(file.current_version())),
                            ])
                        })
                        .collect(),
                ),
            ),
            (
                "not_listed",
                Json::Number(inventory.missing_files().len().saturating_sub(128) as u64),
            ),
            ("approval_authority", Json::Bool(false)),
        ]))
    }
}

fn inspect(workspace: &LaneWorkspace, grant: &Grant) -> Result<AgentFinishPreflight, Unavailable> {
    let state = exact_state(workspace)?;
    workspace
        .daemon()
        .inspect_agent_finish_preflight(
            &state.root,
            &state.digest,
            &state.installation,
            &grant.generation,
        )
        .map_err(|_| refusal("fleet-deletion-inspection-unavailable"))
}

#[cfg(all(test, target_os = "macos"))]
mod tests;
