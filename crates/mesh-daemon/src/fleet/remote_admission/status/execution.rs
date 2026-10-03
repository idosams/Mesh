//! Fresh authenticated historical session observations, never execution or capacity authority.
use super::*;
use crate::fleet::{RemoteExecutionState, RunState};

/// Exact recorded session revision/state. Even terminal state does not prove process-tree death.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RemoteRecordedExecution {
    /// Revision of the original received session, not the coordinator objective or admission.
    pub revision: u64,
    /// Historical state only. Unrecorded/incomplete setup stays uncertain.
    pub state: RemoteExecutionState,
}
impl RemoteWorkerStatusChallenge {
    /// Request original recorded execution facts using independent v4 signing domains.
    /// Older reply versions cannot satisfy this query; no fallback grants process authority.
    pub fn issue_with_execution_observation(
        runtime: &mut Runtime,
        lane: &str,
        run: &str,
        coordinator: PublicKey,
        worker: PublicKey,
    ) -> Result<Self, Error> {
        let mut challenge = Self::issue(runtime, lane, run, coordinator, worker)?;
        challenge.version = StatusVersion::Execution;
        Ok(challenge)
    }
}
impl RemoteWorkerStatusReceipt {
    /// Whether this response includes the v4 historical execution field. Older protocols do not.
    pub fn reports_execution(&self) -> bool {
        self.facts.get("execution").is_some()
    }
    /// None means older protocol or no retained launch; use reports_execution to distinguish them.
    /// Neither absence nor terminal state grants restart, capacity release or main approval.
    pub fn recorded_execution(&self) -> Option<RemoteRecordedExecution> {
        let value = self.facts.get("execution")?;
        parse_execution(value).ok()
    }
}
impl VerifiedRemoteWorkerStatusQuery {
    pub(super) fn execution_facts(
        &self,
        registry: &RemoteAdmissionRegistry,
        base: Json,
    ) -> Result<Json, Error> {
        let execution = if matches!(base.get("admission"), Some(Json::Null)) {
            Json::Null
        } else {
            let assignment = text(
                self.query.body.get("target").ok_or_else(invalid)?,
                "assignment",
            )?;
            match registry.execution_observation(assignment)? {
                None => Json::Null,
                Some(observed) => Json::object([
                    ("revision", Json::Number(observed.revision())),
                    ("state", Json::text(state_word(observed.state()))),
                ]),
            }
        };
        canonical_execution_facts(&Json::object([
            (
                "admission",
                base.get("admission").ok_or_else(invalid)?.clone(),
            ),
            ("launch", base.get("launch").ok_or_else(invalid)?.clone()),
            (
                "effective_lease",
                base.get("effective_lease").ok_or_else(invalid)?.clone(),
            ),
            ("execution", execution),
        ]))
    }
}
fn state_word(state: RemoteExecutionState) -> &'static str {
    match state {
        RemoteExecutionState::Unrecorded => "unrecorded",
        RemoteExecutionState::SetupIncomplete => "setup-incomplete",
        RemoteExecutionState::Recorded(state) => crate::fleet::wire::state_word(state),
    }
}
fn parse_execution(value: &Json) -> Result<RemoteRecordedExecution, Error> {
    closed(value, &["revision", "state"])?;
    let revision = number(value, "revision")?;
    let state = match (revision, text(value, "state")?) {
        (0, "unrecorded") => RemoteExecutionState::Unrecorded,
        (1..=3, "setup-incomplete") => RemoteExecutionState::SetupIncomplete,
        (4.., word) => RemoteExecutionState::Recorded(match word {
            "launching" => RunState::Launching,
            "running" => RunState::Running,
            "waiting" => RunState::Waiting,
            "reconciling" => RunState::Reconciling,
            "stopping" => RunState::Stopping,
            "succeeded" => RunState::Succeeded,
            "failed" => RunState::Failed,
            "cancelled" => RunState::Cancelled,
            _ => return Err(invalid()),
        }),
        _ => return Err(invalid()),
    };
    if revision == 4 && state != RemoteExecutionState::Recorded(RunState::Launching) {
        return Err(invalid());
    }
    if revision > i64::MAX as u64 {
        return Err(invalid());
    }
    Ok(RemoteRecordedExecution { revision, state })
}
pub(super) fn canonical_execution_facts(value: &Json) -> Result<Json, Error> {
    closed(
        value,
        &["admission", "launch", "effective_lease", "execution"],
    )?;
    let base = canonical_facts(
        &Json::object([
            (
                "admission",
                value.get("admission").ok_or_else(invalid)?.clone(),
            ),
            ("launch", value.get("launch").ok_or_else(invalid)?.clone()),
            (
                "effective_lease",
                value.get("effective_lease").ok_or_else(invalid)?.clone(),
            ),
        ]),
        StatusVersion::Effective,
    )?;
    let execution = value.get("execution").ok_or_else(invalid)?;
    let execution = if matches!(base.get("launch"), Some(Json::Null)) {
        if !matches!(execution, Json::Null) {
            return Err(invalid());
        }
        Json::Null
    } else {
        let recorded = parse_execution(execution)?;
        Json::object([
            ("revision", Json::Number(recorded.revision)),
            ("state", Json::text(state_word(recorded.state))),
        ])
    };
    Ok(Json::object([
        ("admission", base.get("admission").unwrap().clone()),
        ("launch", base.get("launch").unwrap().clone()),
        (
            "effective_lease",
            base.get("effective_lease").unwrap().clone(),
        ),
        ("execution", execution),
    ]))
}
/// Query recorded original worker state over bounded authenticated SSH without adopting execution.
pub fn inspect_remote_worker_execution_over_ssh(
    destination: &NativeSshDestination,
    request: RemoteWorkerStatusRequest<'_>,
    budget: Duration,
    sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
) -> io::Result<RemoteWorkerStatusReceipt> {
    inspect_version(destination, request, budget, sign, StatusVersion::Execution)
}
#[cfg(test)]
mod tests;
