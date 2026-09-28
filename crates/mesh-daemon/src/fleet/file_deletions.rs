//! Explicit native deletion intent and outcome. No filesystem effect occurs during replay.
use super::{
    current_run, fold_digest_valid, id_valid, refuse, AgentOrigin, Command, Error, RunState, State,
};
use mesh_store::RecordDigest;

/// Exact pending deletion request, independent of worker connection or later working edits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileDeletion {
    /// Bound lane.
    pub lane: String,
    /// Authenticated native session at admission.
    pub origin: AgentOrigin,
    /// Workspace fold observed before resolution.
    pub input_digest: String,
    /// Explicit relative path, not a request to remove a live file.
    pub path: String,
    /// Exact last saved file version.
    pub version: RecordDigest,
    /// Prepared authenticated operation, recorded before native append.
    pub operation: Option<RecordDigest>,
    /// Absent means reconcile; never infer success or replay the filesystem operation.
    pub result: Option<FileDeletionResult>,
}
/// Durable acknowledgment of one authenticated deletion, not a whole-folder checkpoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileDeletionResult {
    /// Must equal the previously prepared operation.
    pub operation: RecordDigest,
    /// Workspace fold after append or reconciliation.
    pub workspace_digest: String,
    /// Whether native observation settled at the time of this acknowledgment.
    pub settled: bool,
}
impl State {
    pub(super) fn apply_file_deletion(&mut self, command: &Command) -> Result<(), Error> {
        match command {
            Command::BeginFileDeletion {
                id,
                lane,
                origin,
                input_digest,
                path,
                version,
            } => {
                id_valid(id)?;
                id_valid(&origin.actor)?;
                id_valid(&origin.session)?;
                fold_digest_valid(input_digest)?;
                if self.cancelled {
                    return refuse("objective-cancelled");
                }
                if origin.generation.len() != 32
                    || !origin.generation.bytes().all(|b| b.is_ascii_hexdigit())
                {
                    return refuse("invalid-generation");
                }
                if path.is_empty() || path.len() > 4096 || path.starts_with('/') || path.split('/').any(|part| part.is_empty() || part == "." || part == "..")
                    || path.chars().any(|ch| ch.is_control() || matches!(ch, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')) {
                    return refuse("invalid-deletion-path");
                }
                if self.file_deletions.contains_key(id) {
                    return refuse("file-deletion-exists");
                }
                if self.file_deletions.len() >= 4096 {
                    return refuse("file-deletion-limit");
                }
                let run = current_run(&mut self.lanes, lane, &origin.run)?;
                if !matches!(
                    run.state,
                    RunState::Launching | RunState::Running | RunState::Waiting
                ) {
                    return refuse("run-not-saveable");
                }
                self.file_deletions.insert(
                    id.clone(),
                    FileDeletion {
                        lane: lane.clone(),
                        origin: origin.clone(),
                        input_digest: input_digest.clone(),
                        path: path.clone(),
                        version: *version,
                        operation: None,
                        result: None,
                    },
                );
            }
            Command::PrepareFileDeletion { id, operation } => {
                if self.cancelled {
                    return refuse("objective-cancelled");
                }
                let pending = self
                    .file_deletions
                    .get(id)
                    .ok_or(Error::Refused("file-deletion-missing"))?;
                if pending.operation.is_some() || pending.result.is_some() {
                    return refuse("file-deletion-already-prepared");
                }
                let run = current_run(&mut self.lanes, &pending.lane, &pending.origin.run)?;
                if !matches!(
                    run.state,
                    RunState::Launching | RunState::Running | RunState::Waiting
                ) {
                    return refuse("run-not-saveable");
                }
                self.file_deletions
                    .get_mut(id)
                    .ok_or(Error::Refused("file-deletion-missing"))?
                    .operation = Some(*operation);
            }
            Command::FinishFileDeletion { id, result } => {
                fold_digest_valid(&result.workspace_digest)?;
                let pending = self
                    .file_deletions
                    .get_mut(id)
                    .ok_or(Error::Refused("file-deletion-missing"))?;
                if pending.result.is_some() {
                    return refuse("file-deletion-already-finished");
                }
                if pending.operation != Some(result.operation) {
                    return refuse("file-deletion-operation-mismatch");
                }
                pending.result = Some(result.clone());
            }
            _ => return Err(Error::InvalidHistory),
        }
        Ok(())
    }
}
