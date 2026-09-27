//! Durable objective and run lifecycle, independent of desktop workspace selection.
//!
//! This is the scheduler's control model, not an agent authorization boundary. Native callers
//! must authenticate/authorize commands and verify versions before admitting them here. External
//! process effects happen only after dispatch intent commits, and uncertain runs retain their slot.

mod wire;
#[cfg(unix)]
pub mod workspace;

use mesh_store::fleet::{FleetEvent, FleetStore, FleetStoreError, MAX_FLEET_EVENT_PAGE};
use mesh_store::RecordDigest;
use std::collections::BTreeMap;

/// Human-authorized upper bounds inherited by every child lane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Maximum retained lanes in this objective.
    pub lanes: u64,
    /// Maximum simultaneous launching, live or uncertain attempts.
    pub concurrency: u64,
    /// Maximum parent-child depth; root lanes have depth zero.
    pub depth: u64,
    /// Maximum additional attempts after the first attempt.
    pub retries: u64,
}

/// A worker attempt's observed execution state; independent of review/work state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunState {
    /// Dispatch is durable, but process creation has not yet been acknowledged.
    Launching,
    /// The adapter confirmed an attached worker.
    Running,
    /// The worker still exists but needs input or a dependency.
    Waiting,
    /// Process ownership or liveness must be reconciled; do not launch a duplicate.
    Reconciling,
    /// Cancellation was requested; the slot stays occupied until termination is confirmed.
    Stopping,
    /// The adapter confirmed successful termination; this does not approve any content.
    Succeeded,
    /// The adapter confirmed failed termination.
    Failed,
    /// The adapter confirmed cancelled termination or absence before launch.
    Cancelled,
}
impl RunState {
    /// Whether this attempt still consumes a concurrency slot.
    pub fn occupies_slot(self) -> bool {
        !matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

/// A single execution attempt; identity remains stable across observation reconnects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    /// Objective-unique run identity.
    pub id: String,
    /// Current execution observation.
    pub state: RunState,
}

/// Native allocation identity, constructed from a verified lane workspace receipt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceBinding {
    source_version: RecordDigest,
    root: String,
    digest: String,
    installation: String,
}
impl WorkspaceBinding {
    /// Pinned native working directory. It is a locator, not authorization by itself.
    pub fn root(&self) -> &str {
        &self.root
    }
    /// Workspace content identity observed during allocation.
    pub fn digest(&self) -> &str {
        &self.digest
    }
    /// Exact installation identity to revalidate before subsequent native actions.
    pub fn installation(&self) -> &str {
        &self.installation
    }
}

/// Persistent work stream, surviving replacement of an agent process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lane {
    /// Objective-unique identity.
    pub id: String,
    /// Optional parent lane.
    pub parent: Option<String>,
    /// Work requested of this lane.
    pub goal: String,
    /// Configured adapter identity.
    pub provider: String,
    /// Exact immutable input; the native allocator must verify it before dispatch.
    pub base: RecordDigest,
    /// Delegation depth from a root lane.
    pub depth: u64,
    /// Native allocation committed before any run can be dispatched.
    pub workspace: Option<WorkspaceBinding>,
    /// Every attempt, including failed and cancelled attempts.
    pub runs: Vec<Run>,
    /// Latest acknowledged immutable work; recording it grants no publication authority.
    pub saved: Option<RecordDigest>,
}

/// Reconstructable objective control state.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct State {
    /// Last committed event revision.
    pub revision: u64,
    /// Requested outcome, absent before initialization.
    pub goal: Option<String>,
    /// Configured limits, absent before initialization.
    pub limits: Option<Limits>,
    /// Cancellation blocks creation and future dispatch permanently for this objective.
    pub cancelled: bool,
    /// Lanes in stable identity order.
    pub lanes: BTreeMap<String, Lane>,
}

/// Authorized scheduling decisions and adapter observations admitted to the ledger.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Initialize an objective once.
    Start {
        /// Human-requested outcome.
        goal: String,
        /// Inherited execution bounds.
        limits: Limits,
    },
    /// Register a lane before native allocation. IDs and paths are not interchangeable.
    CreateLane {
        /// New lane identity.
        id: String,
        /// Parent lane, or a root lane when absent.
        parent: Option<String>,
        /// Requested work for this lane.
        goal: String,
        /// Configured provider adapter.
        provider: String,
        /// Exact immutable input version.
        base: RecordDigest,
    },
    /// Bind native allocation after verifying its receipt; no worker may start before this commits.
    BindWorkspace {
        /// Lane whose private folder was allocated.
        lane: String,
        /// Native-verified allocation identity.
        binding: WorkspaceBinding,
    },
    /// Claim a concurrency slot and record intent before spawning a worker.
    Dispatch {
        /// Lane to execute.
        lane: String,
        /// New objective-unique attempt identity.
        run: String,
    },
    /// Record a checked observation about the current attempt only.
    Observe {
        /// Lane whose current attempt is addressed.
        lane: String,
        /// Exact current attempt identity.
        run: String,
        /// Adapter-confirmed execution observation.
        state: RunState,
    },
    /// Record a native-verified saved version while preserving attempt history.
    Saved {
        /// Lane whose current attempt is addressed.
        lane: String,
        /// Exact current attempt identity.
        run: String,
        /// Native-verified saved content identity.
        version: RecordDigest,
    },
    /// Prevent future delegation/dispatch and request termination of every active attempt.
    Cancel,
}

/// A command was refused or persisted state could not be reconstructed.
#[derive(Debug)]
pub enum Error {
    /// Storage refused an operation.
    Store(FleetStoreError),
    /// A lifecycle invariant was violated; code is safe to expose without command contents.
    Refused(&'static str),
    /// The durable stream is unsupported or inconsistent. No repair is attempted implicitly.
    InvalidHistory,
}
impl From<FleetStoreError> for Error {
    fn from(value: FleetStoreError) -> Self {
        Self::Store(value)
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(e) => write!(f, "{e}"),
            Self::Refused(code) => write!(f, "fleet command refused: {code}"),
            Self::InvalidHistory => f.write_str("fleet history cannot be reconstructed"),
        }
    }
}
impl std::error::Error for Error {}

/// One objective's durable control stream. It does not own any provider processes yet.
pub struct Runtime {
    store: FleetStore,
    objective: String,
    state: State,
}
impl Runtime {
    /// Reconstruct a stream without restarting workers or interpreting disconnect as failure.
    pub fn open(store: FleetStore, objective: &str) -> Result<Self, Error> {
        let mut runtime = Self {
            store,
            objective: objective.to_owned(),
            state: State::default(),
        };
        runtime.refresh()?;
        Ok(runtime)
    }

    /// Current projection. Refresh before making a decision if another service can write.
    pub fn state(&self) -> &State {
        &self.state
    }

    /// Replay committed events. Corrupt/unknown commands fail closed and keep the old projection.
    pub fn refresh(&mut self) -> Result<(), Error> {
        let mut next = self.state.clone();
        loop {
            let page = self
                .store
                .events(&self.objective, next.revision, MAX_FLEET_EVENT_PAGE)?;
            if page.is_empty() {
                break;
            }
            for event in page {
                if event.revision != next.revision + 1 {
                    return Err(Error::InvalidHistory);
                }
                let command = wire::decode(&event.payload)?;
                next.apply(&command).map_err(|_| Error::InvalidHistory)?;
                next.revision = event.revision;
            }
        }
        self.state = next;
        Ok(())
    }

    /// Validate and commit one decision. Identical retries recover the original acknowledgment.
    pub fn submit(
        &mut self,
        expected: u64,
        request: &str,
        command: Command,
    ) -> Result<FleetEvent, Error> {
        let payload = wire::encode(&command);
        if self.store.request(&self.objective, request)?.is_some() {
            let event = self
                .store
                .append(&self.objective, expected, request, &payload)?;
            self.refresh()?;
            return Ok(event);
        }
        self.refresh()?;
        if self.state.revision != expected {
            return Err(FleetStoreError::StaleRevision {
                actual: self.state.revision,
            }
            .into());
        }
        let mut next = self.state.clone();
        next.apply(&command)?;
        let event = self
            .store
            .append(&self.objective, expected, request, &payload)?;
        next.revision = event.revision;
        self.state = next;
        Ok(event)
    }
}

impl State {
    fn apply(&mut self, command: &Command) -> Result<(), Error> {
        if let Command::Start { goal, limits } = command {
            if self.goal.is_some() {
                return refuse("already-started");
            }
            goal_valid(goal)?;
            if limits.lanes == 0
                || limits.lanes > 1024
                || limits.concurrency == 0
                || limits.concurrency > 64
                || limits.concurrency > limits.lanes
                || limits.depth > 32
                || limits.retries > 10
            {
                return refuse("invalid-limits");
            }
            self.goal = Some(goal.clone());
            self.limits = Some(limits.clone());
            return Ok(());
        }
        let limits = self.limits.as_ref().ok_or(Error::Refused("not-started"))?;
        match command {
            Command::Start { .. } => unreachable!("handled above"),
            Command::CreateLane {
                id,
                parent,
                goal,
                provider,
                base,
            } => {
                if self.cancelled {
                    return refuse("objective-cancelled");
                }
                id_valid(id)?;
                id_valid(provider)?;
                goal_valid(goal)?;
                if self.lanes.contains_key(id) {
                    return refuse("lane-exists");
                }
                if self.lanes.len() as u64 >= limits.lanes {
                    return refuse("lane-limit");
                }
                let depth = match parent {
                    Some(parent) => {
                        self.lanes
                            .get(parent)
                            .ok_or(Error::Refused("parent-missing"))?
                            .depth
                            + 1
                    }
                    None => 0,
                };
                if depth > limits.depth {
                    return refuse("delegation-limit");
                }
                self.lanes.insert(
                    id.clone(),
                    Lane {
                        id: id.clone(),
                        parent: parent.clone(),
                        goal: goal.clone(),
                        provider: provider.clone(),
                        base: *base,
                        depth,
                        workspace: None,
                        runs: Vec::new(),
                        saved: None,
                    },
                );
            }
            Command::BindWorkspace { lane, binding } => {
                for value in [&binding.root, &binding.digest, &binding.installation] {
                    if value.is_empty() || value.len() > 4096 || value.contains('\0') {
                        return refuse("invalid-workspace-binding");
                    }
                }
                if self
                    .lanes
                    .values()
                    .filter_map(|lane| lane.workspace.as_ref())
                    .any(|other| {
                        other.installation == binding.installation || other.root == binding.root
                    })
                {
                    return refuse("workspace-already-bound");
                }
                let target = self
                    .lanes
                    .get_mut(lane)
                    .ok_or(Error::Refused("lane-missing"))?;
                if binding.source_version != target.base {
                    return refuse("allocation-version-mismatch");
                }
                if target.workspace.is_some() {
                    return refuse("lane-already-allocated");
                }
                // A completed allocation after cancellation must still be recorded for recovery.
                target.workspace = Some(binding.clone());
            }
            Command::Dispatch { lane, run } => {
                if self.cancelled {
                    return refuse("objective-cancelled");
                }
                id_valid(run)?;
                if self
                    .lanes
                    .values()
                    .flat_map(|lane| &lane.runs)
                    .any(|old| old.id == *run)
                {
                    return refuse("run-exists");
                }
                let active = self
                    .lanes
                    .values()
                    .flat_map(|lane| &lane.runs)
                    .filter(|run| run.state.occupies_slot())
                    .count();
                if active as u64 >= limits.concurrency {
                    return refuse("concurrency-limit");
                }
                let lane = self
                    .lanes
                    .get_mut(lane)
                    .ok_or(Error::Refused("lane-missing"))?;
                if lane.workspace.is_none() {
                    return refuse("lane-not-allocated");
                }
                if let Some(previous) = lane.runs.last() {
                    if previous.state.occupies_slot() {
                        return refuse("run-already-active");
                    }
                    if previous.state == RunState::Succeeded {
                        return refuse("lane-completed");
                    }
                }
                if lane.runs.len() as u64 > limits.retries {
                    return refuse("retry-limit");
                }
                lane.runs.push(Run {
                    id: run.clone(),
                    state: RunState::Launching,
                });
            }
            Command::Observe { lane, run, state } => {
                let current = current_run(&mut self.lanes, lane, run)?;
                if !transition(current.state, *state) {
                    return refuse("invalid-run-transition");
                }
                current.state = *state;
            }
            Command::Saved { lane, run, version } => {
                let current = current_run(&mut self.lanes, lane, run)?;
                if !matches!(
                    current.state,
                    RunState::Running
                        | RunState::Waiting
                        | RunState::Stopping
                        | RunState::Succeeded
                ) {
                    return refuse("run-not-saveable");
                }
                self.lanes
                    .get_mut(lane)
                    .ok_or(Error::Refused("lane-missing"))?
                    .saved = Some(*version);
            }
            Command::Cancel => {
                self.cancelled = true;
                for lane in self.lanes.values_mut() {
                    if let Some(run) = lane.runs.last_mut() {
                        if run.state.occupies_slot() {
                            run.state = RunState::Stopping;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

fn current_run<'a>(
    lanes: &'a mut BTreeMap<String, Lane>,
    lane: &str,
    run: &str,
) -> Result<&'a mut Run, Error> {
    let current = lanes
        .get_mut(lane)
        .and_then(|lane| lane.runs.last_mut())
        .ok_or(Error::Refused("run-missing"))?;
    if current.id != run {
        return refuse("stale-run");
    }
    Ok(current)
}
fn transition(from: RunState, to: RunState) -> bool {
    use RunState::*;
    if !from.occupies_slot() {
        return false;
    }
    match from {
        Launching => matches!(to, Running | Reconciling | Stopping | Failed | Cancelled),
        Running | Waiting => matches!(
            to,
            Running | Waiting | Reconciling | Stopping | Succeeded | Failed | Cancelled
        ),
        Reconciling => matches!(
            to,
            Running | Waiting | Stopping | Succeeded | Failed | Cancelled
        ),
        Stopping => matches!(to, Succeeded | Failed | Cancelled),
        Succeeded | Failed | Cancelled => false,
    }
}
fn id_valid(value: &str) -> Result<(), Error> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
    {
        return refuse("invalid-identity");
    }
    Ok(())
}
fn goal_valid(value: &str) -> Result<(), Error> {
    if value.trim().is_empty() || value.len() > 8192 {
        return refuse("invalid-goal");
    }
    Ok(())
}
fn refuse<T>(code: &'static str) -> Result<T, Error> {
    Err(Error::Refused(code))
}

#[cfg(test)]
mod tests;
