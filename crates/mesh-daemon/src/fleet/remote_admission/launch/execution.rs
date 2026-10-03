//! Read-only correlated session facts. No process adoption or capacity-release authority.
use super::*;
use crate::fleet::{wire, Command, RunState};
use mesh_store::fleet::MAX_FLEET_EVENT_PAGE;

/// Persisted session state, never a statement that a process is currently alive or dead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteExecutionState {
    /// Launch intent exists but no session command was recorded. Execution remains uncertain.
    Unrecorded,
    /// Only an exact prefix of native session setup was committed. Never resume it implicitly.
    SetupIncomplete,
    /// Last committed run observation, including historical running or completion facts.
    Recorded(RunState),
}

/// Historical execution observation correlated to the exact original launch intent.
/// Even a terminal observation cannot release capacity, restart work or approve a version.
#[derive(Clone, PartialEq, Eq)]
pub struct RemoteExecutionObservation {
    launch: RemoteLaunchReceipt,
    revision: u64,
    state: RemoteExecutionState,
}
impl RemoteExecutionObservation {
    /// Exact admission, workspace mapping and original launch owner.
    pub fn launch(&self) -> &RemoteLaunchReceipt {
        &self.launch
    }
    /// Last committed session revision observed, not a timestamp or lease.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Recorded state only; lack of a live owner cannot strengthen this fact.
    pub fn state(&self) -> RemoteExecutionState {
        self.state
    }
}

impl RemoteAdmissionRegistry {
    /// Inspect the original session in this native-owned ledger without opening a workspace,
    /// constructing a service, granting credentials, writing records or dispatching a provider.
    /// None means no launch intent for an admitted assignment, never permission to retry.
    /// Concurrent advancement or malformed/mismatched session history refuses the observation.
    pub fn execution_observation(
        &self,
        assignment: &str,
    ) -> Result<Option<RemoteExecutionObservation>, Error> {
        let Some(launch) = self.launch_receipt(assignment)? else {
            return Ok(None);
        };
        let scope = session_scope(&self.launch_stream(assignment));
        let revision = self.store.revision(&scope)?;
        let mut state = State::default();
        // Pin the upper revision so an active writer cannot extend this read indefinitely.
        while state.revision < revision {
            let count = (revision - state.revision).min(MAX_FLEET_EVENT_PAGE as u64) as usize;
            let page = self.store.events(&scope, state.revision, count)?;
            if page.is_empty() {
                return Err(Error::InvalidHistory);
            }
            for event in page {
                if event.revision != state.revision + 1 || event.revision > revision {
                    return Err(Error::InvalidHistory);
                }
                let command = wire::decode(&event.payload)?;
                if wire::encode(&command) != event.payload {
                    return Err(Error::InvalidHistory);
                }
                check_setup(&launch, &event, &command)?;
                state.apply(&command).map_err(|_| Error::InvalidHistory)?;
                state.revision = event.revision;
            }
        }
        let observed = if revision == 0 {
            RemoteExecutionState::Unrecorded
        } else if revision < 4 {
            RemoteExecutionState::SetupIncomplete
        } else {
            check_session(&launch, &state)?;
            RemoteExecutionState::Recorded(state.lanes[&launch.admission.work.lane].runs[0].state)
        };
        if self.store.revision(&scope)? != revision
            || self.launch_receipt(assignment)?.as_ref() != Some(&launch)
        {
            return refuse("remote-execution-observation-changed");
        }
        Ok(Some(RemoteExecutionObservation {
            launch,
            revision,
            state: observed,
        }))
    }
}
fn session_limits() -> Limits {
    Limits {
        lanes: 1,
        concurrency: 1,
        depth: 0,
        retries: 0,
    }
}
fn check_setup(
    launch: &RemoteLaunchReceipt,
    event: &FleetEvent,
    command: &Command,
) -> Result<(), Error> {
    let work = launch.admission.work();
    let expected = match event.revision {
        1 => (
            "start",
            Command::Start {
                goal: work.goal.clone(),
                limits: session_limits(),
            },
        ),
        2 => (
            "lane",
            Command::CreateLane {
                id: work.lane.clone(),
                parent: None,
                goal: work.goal.clone(),
                provider: work.provider.clone(),
                base: work.assignment.input,
            },
        ),
        3 => {
            let Command::BindWorkspace { lane, binding } = command else {
                return Err(Error::InvalidHistory);
            };
            if lane != &work.lane
                || binding.source_version != work.assignment.input
                || binding.starting_version() != Some(launch.initial_operation())
                || binding.installation() != launch.installation()
            {
                return Err(Error::InvalidHistory);
            }
            ("workspace", command.clone())
        }
        4 => (
            "dispatch",
            Command::Dispatch {
                lane: work.lane.clone(),
                run: work.run.clone(),
            },
        ),
        _ => return Ok(()),
    };
    if event.request != expected.0 || command != &expected.1 {
        return Err(Error::InvalidHistory);
    }
    Ok(())
}
fn check_session(launch: &RemoteLaunchReceipt, state: &State) -> Result<(), Error> {
    let work = launch.admission.work();
    let lane = state.lanes.get(&work.lane).ok_or(Error::InvalidHistory)?;
    if state.goal.as_ref() != Some(&work.goal)
        || state.limits.as_ref() != Some(&session_limits())
        || state.lanes.len() != 1
        || lane.id != work.lane
        || lane.goal != work.goal
        || lane.provider != work.provider
        || lane.base != work.assignment.input
        || lane.parent.is_some()
        || lane.created_by.is_some()
        || lane.source_project.is_some()
        || lane.depth != 0
        || lane.runs.len() != 1
        || lane.runs[0].id != work.run
        || lane.runs[0].remote.is_some()
    {
        return Err(Error::InvalidHistory);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
