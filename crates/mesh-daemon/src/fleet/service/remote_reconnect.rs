//! Explicit continuation of one retained initial transfer; never dispatch or worker adoption.
use super::*;
use crate::fleet::{
    deliver_remote_input_over_ssh, NativeSshDestination, RemoteInputDeliveryIntent,
    RemoteInputSource, RemoteInputTransferOutcome, RemoteInputTransferRequest,
};
use mesh_crypto::SigningPayload;
use mesh_types::{PublicKey, Signature};
use std::{io, time::Duration};
/// Exact native saved input and independently admitted identities. No new assignment or lease.
pub struct RemoteHistoryInputRequest<'a> {
    /// Retained lane identity.
    pub lane: &'a str,
    /// Exact retained attempt.
    pub run: &'a str,
    /// Native saved-history export, never mutable project contents.
    pub source: &'a RemoteInputSource,
    /// Independently admitted coordinator identity.
    pub coordinator: PublicKey,
    /// Independently trusted worker identity.
    pub worker: PublicKey,
}
/// Native history-derived source selector. Resolving it grants no execution or transfer authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RetainedRemoteInput {
    /// The exact original attached project version of a root lane.
    Project {
        /// Registered project identity, never a path.
        project: String,
        /// Exact immutable original input version.
        version: String,
    },
    /// An exact completed and reviewed parent checkpoint.
    Review(SavedReviewSelection),
}
fn retained_input(
    state: &super::super::State,
    lane: &str,
    run: &str,
) -> io::Result<RetainedRemoteInput> {
    let lane = state.lanes.get(lane).ok_or_else(unavailable)?;
    let attempt = lane
        .runs
        .last()
        .filter(|r| r.id == run && r.state == RunState::Launching && r.launch_owner.is_some())
        .ok_or_else(unavailable)?;
    let assignment = attempt.remote.as_ref().ok_or_else(unavailable)?;
    if state.cancelled || assignment.input != lane.base || assignment.lease_sequence != 1 {
        return Err(unavailable());
    }
    if let Some(parent) = &lane.parent {
        let (id, checkpoint) = state
            .checkpoints
            .iter()
            .find(|(_, c)| {
                c.lane == *parent
                    && c.review.is_some()
                    && c.result
                        .as_ref()
                        .is_some_and(|r| r.complete && r.version == lane.base)
            })
            .ok_or_else(unavailable)?;
        return Ok(RetainedRemoteInput::Review(
            SavedReviewSelection::new(
                parent,
                id,
                &lane.base.to_string(),
                &checkpoint.review.ok_or_else(unavailable)?.to_string(),
            )
            .map_err(|_| unavailable())?,
        ));
    }
    let project = lane.source_project.as_ref().ok_or_else(unavailable)?;
    if lane
        .workspace
        .as_ref()
        .is_none_or(|binding| binding.source_version != lane.base)
    {
        return Err(unavailable());
    }
    Ok(RetainedRemoteInput::Project {
        project: project.clone(),
        version: lane.base.to_string(),
    })
}
impl FleetHistory {
    /// Resolve retained immutable input for an already claimed initial transfer. This does not
    /// adopt execution, renew a lease, read mutable work or contact the worker. The actual transfer
    /// must independently recheck the assignment, source manifest, peer and cancellation.
    pub fn retained_remote_input(&self, lane: &str, run: &str) -> io::Result<RetainedRemoteInput> {
        retained_input(
            &self.0.native_state().map_err(|_| unavailable())?,
            lane,
            run,
        )
    }

    /// Reconnect only a claimed, still-launching initial-lease input transfer. The existing peer
    /// challenge rechecks ownership/input/key/lease and cancellation before signing and after reply.
    /// No coordinator command, lease renewal, workspace adoption or new allocation is authorized.
    /// The resident worker must retain the original reservation. An error preserves uncertainty;
    /// neither an error nor input acceptance is evidence of worker completion or safe replacement.
    pub fn reconnect_remote_input_over_ssh(
        &self,
        peer: &NativeSshDestination,
        request: RemoteHistoryInputRequest<'_>,
        budget: Duration,
        sign: impl FnMut(&SigningPayload) -> Result<Signature, String>,
    ) -> io::Result<RemoteInputTransferOutcome> {
        self.reconnect_remote_input(request, |request| {
            deliver_remote_input_over_ssh(
                peer,
                request,
                RemoteInputDeliveryIntent::Reconnect,
                budget,
                sign,
            )
        })
    }
    pub(in crate::fleet) fn reconnect_remote_input<T>(
        &self,
        request: RemoteHistoryInputRequest<'_>,
        transfer: impl FnOnce(RemoteInputTransferRequest<'_>) -> io::Result<T>,
    ) -> io::Result<T> {
        let mut runtime = {
            let inner = self.0.lock().map_err(|_| unavailable())?;
            let store = inner
                .runtime
                .store
                .reopen_guarded_connection()
                .map_err(|_| unavailable())?;
            Runtime::open(store, inner.runtime.objective()).map_err(|_| unavailable())?
        };
        // Only the closed operation can use this guarded runtime. No service lock survives into
        // transport, and retained history never becomes a generic execution or dispatch surface.
        transfer(RemoteInputTransferRequest {
            runtime: &mut runtime,
            lane: request.lane,
            run: request.run,
            source: request.source,
            coordinator: request.coordinator,
            worker: request.worker,
        })
    }
}
fn unavailable() -> io::Error {
    io::Error::other("Retained input transfer unavailable; reconcile the original assignment")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::{
        AgentOrigin, Checkpoint, CheckpointResult, RemoteAssignment, Run, State, WorkspaceBinding,
    };
    fn state() -> State {
        let base = RecordDigest::from_bytes([1; 32]);
        let mut state = State::default();
        state.lanes.insert(
            "lane".into(),
            Lane {
                id: "lane".into(),
                parent: None,
                created_by: None,
                source_project: Some("a".repeat(64)),
                goal: "test".into(),
                provider: "codex".into(),
                base,
                depth: 0,
                saved: None,
                workspace: Some(WorkspaceBinding {
                    source_version: base,
                    starting_version: None,
                    root: "/unused".into(),
                    digest: "unused".into(),
                    installation: "unused".into(),
                }),
                runs: vec![Run {
                    id: "run".into(),
                    state: RunState::Launching,
                    launch_owner: Some("native".into()),
                    remote: Some(RemoteAssignment {
                        id: "assignment".into(),
                        worker_key: "b".repeat(64),
                        input: base,
                        bundle: RecordDigest::from_bytes([2; 32]),
                        lease_sequence: 1,
                        lease_until_ms: 1,
                    }),
                }],
            },
        );
        state
    }
    #[test]
    fn retained_source_is_exact_and_refuses_replacement_attempt_cancellation_or_changed_input() {
        let original = state();
        assert_eq!(
            retained_input(&original, "lane", "run").unwrap(),
            RetainedRemoteInput::Project {
                project: "a".repeat(64),
                version: RecordDigest::from_bytes([1; 32]).to_string()
            }
        );
        assert!(retained_input(&original, "lane", "other").is_err());
        let mut cancelled = original.clone();
        cancelled.cancelled = true;
        assert!(retained_input(&cancelled, "lane", "run").is_err());
        for change in 0..5 {
            let mut state = original.clone();
            let lane = state.lanes.get_mut("lane").unwrap();
            let run = lane.runs.last_mut().unwrap();
            match change {
                0 => run.state = RunState::Succeeded,
                1 => run.launch_owner = None,
                2 => run.remote = None,
                3 => run.remote.as_mut().unwrap().lease_sequence = 2,
                _ => lane.base = RecordDigest::from_bytes([3; 32]),
            }
            assert!(retained_input(&state, "lane", "run").is_err());
        }
    }
    #[test]
    fn child_input_requires_exact_complete_reviewed_parent_checkpoint() {
        let mut state = state();
        state.lanes.get_mut("lane").unwrap().parent = Some("parent".into());
        assert!(retained_input(&state, "lane", "run").is_err());
        state.checkpoints.insert(
            "checkpoint".into(),
            Checkpoint {
                lane: "parent".into(),
                origin: AgentOrigin {
                    actor: "actor".into(),
                    generation: "generation".into(),
                    session: "session".into(),
                    run: "parent-run".into(),
                },
                input_digest: "input".into(),
                result: Some(CheckpointResult {
                    complete: true,
                    version: RecordDigest::from_bytes([1; 32]),
                    workspace_digest: "state".into(),
                    saved_changes: 1,
                    issue: None,
                }),
                review: Some(RecordDigest::from_bytes([4; 32])),
            },
        );
        assert_eq!(
            retained_input(&state, "lane", "run").unwrap(),
            RetainedRemoteInput::Review(
                SavedReviewSelection::new(
                    "parent",
                    "checkpoint",
                    &RecordDigest::from_bytes([1; 32]).to_string(),
                    &RecordDigest::from_bytes([4; 32]).to_string()
                )
                .unwrap()
            )
        );
        state
            .checkpoints
            .get_mut("checkpoint")
            .unwrap()
            .result
            .as_mut()
            .unwrap()
            .complete = false;
        assert!(retained_input(&state, "lane", "run").is_err());
        state
            .checkpoints
            .get_mut("checkpoint")
            .unwrap()
            .result
            .as_mut()
            .unwrap()
            .complete = true;
        state.checkpoints.get_mut("checkpoint").unwrap().review = None;
        assert!(retained_input(&state, "lane", "run").is_err());
    }
}
