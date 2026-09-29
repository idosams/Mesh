//! Durable objective and run lifecycle, independent of desktop workspace selection.
//!
//! This is the scheduler's control model, not an agent authorization boundary. Native callers
//! must authenticate/authorize commands and verify versions before admitting them here. External
//! process effects happen only after dispatch intent commits, and uncertain runs retain their slot.

#[cfg(target_os = "macos")]
pub mod catalog;
#[cfg(unix)]
pub(crate) mod comparison;
#[cfg(unix)]
pub mod host;
#[cfg(unix)]
mod input_transfer;
#[cfg(unix)]
mod received_host;
#[cfg(unix)]
mod receiving_broker;
#[cfg(unix)]
mod receiving_session;
#[cfg(unix)]
pub use input_transfer::{
    transfer_remote_input, RemoteInputTransferOutcome, RemoteInputTransferReceipt,
    RemoteInputTransferRequest,
};
#[cfg(unix)]
pub use received_host::{
    ReceivedWorkerHost, ReceivedWorkerLaunch, ReceivedWorkerMailbox, ReceivedWorkerObservation,
    ReceivedWorkerRequest, ReceivedWorkerSupervisor,
};
#[cfg(unix)]
pub use receiving_broker::{
    serve_remote_receiving, RemoteReceivedHandoff, RemoteReceivingBrokerOutcome,
    RemoteReceivingCommand,
};
#[cfg(unix)]
pub use receiving_session::{
    RemoteReceivingAccess, RemoteReceivingConnection, RemoteReceivingProgress,
    RemoteReceivingSession,
};
#[cfg(unix)]
pub(crate) mod project_import;
#[cfg(unix)]
mod project_mapping;
#[cfg(unix)]
pub use project_import::{CandidateImportSigner, PreparedProjectCandidateImport};
mod file_deletions;
#[cfg(unix)]
pub mod provider;
mod remote;
mod remote_admission;
#[cfg(target_os = "macos")]
pub use remote_admission::status::{
    inspect_remote_worker_current_lease_over_ssh, inspect_remote_worker_over_ssh,
    RemoteWorkerStatusChallenge, RemoteWorkerStatusQuery, RemoteWorkerStatusReceipt,
    RemoteWorkerStatusRequest, VerifiedRemoteWorkerStatusQuery,
};
#[cfg(unix)]
pub use remote_admission::{RemoteAdmissionChallenge, RemoteAdmissionProof};
pub use remote_admission::{
    RemoteAdmissionOutcome, RemoteAdmissionReceipt, RemoteAdmissionRegistry,
    RemoteInputReservation, RemoteWork, RemoteWorkerLease,
};
#[cfg(unix)]
pub use remote_admission::{RemoteLaunchOutcome, RemoteLaunchReceipt, RemoteLaunchReservation};
#[cfg(target_os = "macos")]
mod worker_connections;
#[cfg(target_os = "macos")]
mod worker_directory;
#[cfg(target_os = "macos")]
pub use worker_connections::{NativeWorkerConnections, WorkerConnectionOutcome};
#[cfg(target_os = "macos")]
pub use worker_directory::{
    NativeRemoteWorkerDirectory, NativeWorkerEndpoint, NativeWorkerInstallation,
    NativeWorkerStream, WorkerInstallationIdentity,
};
#[cfg(target_os = "macos")]
mod remote_delivery;
mod remote_input;
mod remote_transport;
#[cfg(target_os = "macos")]
pub use remote_delivery::{deliver_remote_input_over_ssh, RemoteInputDeliveryIntent};
#[cfg(target_os = "macos")]
mod ssh_transport;
#[cfg(unix)]
pub use remote_input::NativeRemoteInputReceiver;
pub use remote_transport::{RemoteFrame, RemoteFrameReader, RemoteFrameWriter};
#[cfg(target_os = "macos")]
pub use ssh_transport::{NativeSshConnection, NativeSshDestination, SshPipe};
#[cfg(unix)]
mod remote_materialization;
#[cfg(unix)]
pub use crate::workspace::RemoteInputSource;
pub use remote_input::{
    RemoteInputChunk, RemoteInputEntry, RemoteInputManifest, RemoteInputReceiver,
};
#[cfg(unix)]
pub use remote_materialization::{
    ReceivedWorkerWorkspace, RemoteInputAllocation, RemoteInputDestination,
};
#[cfg(unix)]
mod remote_peer;
#[cfg(unix)]
pub use remote_peer::{
    RemoteDispatch, RemoteDispatchPolicy, RemoteInputReconnectChallenge, RemotePeerChallenge,
    VerifiedRemoteDispatch,
};
#[cfg(unix)]
pub mod service;
mod wire;
pub use file_deletions::{FileDeletion, FileDeletionResult};
pub use remote::RemoteAssignment;
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
    /// Native host that durably claimed the one launch attempt; not a credential or liveness proof.
    pub launch_owner: Option<String>,
    /// Durable remote assignment, absent for existing local run histories.
    pub remote: Option<RemoteAssignment>,
}

/// Native allocation identity, constructed from a verified lane workspace receipt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceBinding {
    source_version: RecordDigest,
    starting_version: Option<RecordDigest>,
    root: String,
    digest: String,
    installation: String,
}
impl WorkspaceBinding {
    /// Exact local initial import, bound during allocation. Legacy records have no such proof.
    pub fn starting_version(&self) -> Option<RecordDigest> {
        self.starting_version
    }

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

/// Durable attribution for one accepted agent delegation; contains no credential.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentOrigin {
    /// Native-issued actor identity.
    pub actor: String,
    /// Native-issued session identity.
    pub session: String,
    /// Exact parent run that accepted the work.
    pub run: String,
    /// Native workspace custody generation at acceptance.
    pub generation: String,
}

/// Durable request identity for a native agent capture, without secrets or file contents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checkpoint {
    /// Bound lane.
    pub lane: String,
    /// Accepted native session.
    pub origin: AgentOrigin,
    /// Exact fold before capture began.
    pub input_digest: String,
    /// Absent means capture must be reconciled, never implicitly repeated.
    pub result: Option<CheckpointResult>,
    /// Immutable native review bundle for the completed checkpoint, if submitted.
    pub review: Option<RecordDigest>,
}

/// Bounded immutable capture acknowledgment, safe to replay after later working edits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckpointResult {
    /// Whether native final inventory and recovery checks completed.
    pub complete: bool,
    /// Latest retained workspace operation, not publication authority.
    pub version: RecordDigest,
    /// Fold after capture, including partial progress.
    pub workspace_digest: String,
    /// Number of authenticated appends completed by this invocation.
    pub saved_changes: u64,
    /// Stable bounded reason when incomplete.
    pub issue: Option<String>,
}

/// Persistent work stream, surviving replacement of an agent process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lane {
    /// Objective-unique identity.
    pub id: String,
    /// Optional parent lane.
    pub parent: Option<String>,
    /// Agent identity responsible for delegation, absent for native-created root lanes.
    pub created_by: Option<AgentOrigin>,
    /// Original attached project shared by this lane and its descendants, if any.
    pub source_project: Option<String>,
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

/// A native-requested revision of one immutable review. Recording is not delivery or approval.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewChangeRequest {
    /// Objective-unique retry identity.
    pub id: String,
    /// Only this originating lane may retrieve the request through agent context.
    pub lane: String,
    /// Exact completed checkpoint.
    pub checkpoint: String,
    /// Saved content reviewed by the caller.
    pub version: RecordDigest,
    /// Exact recorded review bundle.
    pub bundle: RecordDigest,
    /// Bounded user feedback, stored privately and never included in diagnostic summaries.
    pub message: String,
}

/// Authenticated proposal of a saved result in response to one review change request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewChangeResponse {
    /// Exact request in this objective.
    pub request: String,
    /// Complete recorded checkpoint from the originating lane.
    pub checkpoint: String,
    /// Saved operation derived from that checkpoint, never supplied by the agent.
    pub version: RecordDigest,
    /// Recorded review derived from that checkpoint.
    pub bundle: RecordDigest,
    /// Native-authenticated session that proposed the result.
    pub origin: AgentOrigin,
}

/// Native-confirmed work decision, distinct from shared-state approval.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReviewChangeDecision {
    /// Per-request revision prevents stale decisions, including address/reopen cycles.
    pub revision: u64,
    /// Proposed checkpoint marked as addressing the request, or None for an open request.
    pub checkpoint: Option<String>,
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
    /// Acknowledged capture intents and immutable outcomes, retained across restart.
    pub checkpoints: BTreeMap<String, Checkpoint>,
    /// Explicit deletion intents and exact prepared operations, never inferred from missing files.
    pub file_deletions: BTreeMap<String, FileDeletion>,
    /// Durable requests remain readable independently of worker attempts.
    pub review_change_requests: BTreeMap<String, ReviewChangeRequest>,
    /// Append-ordered proposals, bounded to eight per request. No resolution is inferred.
    pub review_change_responses: BTreeMap<String, Vec<ReviewChangeResponse>>,
    /// Explicit native-confirmed work decisions; absent entries are open at revision zero.
    pub review_change_decisions: BTreeMap<String, ReviewChangeDecision>,
}

/// Authorized scheduling decisions and adapter observations admitted to the ledger.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Native-only feedback bound to a completed, recorded review. No scheduling side effects.
    RequestReviewChanges(ReviewChangeRequest),
    /// Native-only decision after exact confirmation; this never approves shared state.
    SetReviewChangeDecision {
        /// Original request identity.
        request: String,
        /// Exact per-request revision shown before confirmation.
        expected_revision: u64,
        /// A recorded proposal, or None to reopen the request.
        checkpoint: Option<String>,
    },
    /// Propose an exact reviewed checkpoint; the native caller authenticates this session.
    ProposeReviewChangeResult {
        /// Original native-requested feedback identity.
        request: String,
        /// Complete recorded checkpoint produced by this session.
        checkpoint: String,
        /// Native-authenticated caller, not agent-supplied metadata.
        origin: AgentOrigin,
    },
    /// Reserve remote execution before transfer. Native code must authenticate the worker
    /// and verify the immutable bundle before submitting this control-plane record.
    ClaimRemoteLaunch {
        /// Exact lane.
        lane: String,
        /// Exact current attempt.
        run: String,
        /// Native-verified assignment; contains no credentials or filesystem paths.
        assignment: RemoteAssignment,
    },
    /// Record a verified continuation of the same remote assignment. This never grants
    /// another launch, releases a slot or proves result integrity or process completion.
    AdvanceRemoteLease {
        /// Exact lane.
        lane: String,
        /// Exact current attempt.
        run: String,
        /// Exact retained assignment identity.
        assignment: String,
        /// Authenticated worker public-key identity, supplied by native code.
        worker_key: String,
        /// Previously acknowledged sequence, preventing stale renewal races.
        expected_sequence: u64,
        /// New native-authorized expiry, strictly later than the retained expiry.
        lease_until_ms: u64,
    },
    /// Claim a dispatch exactly once before performing its external process launch.
    ClaimLaunch {
        /// Exact lane.
        lane: String,
        /// Exact current run.
        run: String,
        /// Fresh native host identity, distinct across service restarts.
        owner: String,
    },
    /// A native review record was durably created for one completed checkpoint.
    SubmitReview {
        /// Original checkpoint identity.
        checkpoint: String,
        /// Exact native bundle identity, not approval authority.
        bundle: RecordDigest,
    },
    /// Commit capture intent before calling the native filesystem/journal operation.
    BeginCheckpoint {
        /// Objective-unique request identity.
        id: String,
        /// Exact lane.
        lane: String,
        /// Native-authorized caller.
        origin: AgentOrigin,
        /// Record fold admitted at acceptance.
        input_digest: String,
    },
    /// Record the outcome of an accepted capture, including incomplete results.
    FinishCheckpoint {
        /// Original request identity.
        id: String,
        /// Native-verified outcome.
        result: CheckpointResult,
    },
    /// Persist an explicit missing-file resolution before any native append.
    BeginFileDeletion {
        /// Objective-unique request identity.
        id: String,
        /// Bound lane.
        lane: String,
        /// Native-authenticated caller.
        origin: AgentOrigin,
        /// Exact fold at admission.
        input_digest: String,
        /// Explicit confined relative path; no filesystem mutation is authorized by this alone.
        path: String,
        /// Last saved file version expected at this path.
        version: RecordDigest,
    },
    /// Bind the exact signed native operation before appending it to workspace history.
    PrepareFileDeletion {
        /// Previously accepted intent.
        id: String,
        /// Exact authenticated deletion operation identity.
        operation: RecordDigest,
    },
    /// Record a native-verified outcome, including reconciliation after cancellation.
    FinishFileDeletion {
        /// Previously accepted intent.
        id: String,
        /// Immutable native outcome; no approval authority.
        result: FileDeletionResult,
    },
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
    /// Native-created root from an attached project; correlation is not authorship or approval.
    CreateAttachedLane {
        /// Objective-unique lane identity.
        id: String,
        /// Exact native registration identity.
        project: String,
        /// Work requested of the lane.
        goal: String,
        /// Native-authorized provider.
        provider: String,
        /// Exact saved input operation.
        base: RecordDigest,
    },
    /// Delegate from a current parent run, retaining exact actor/session attribution.
    Delegate {
        /// New lane identity.
        id: String,
        /// Parent lane whose authority was checked.
        parent: String,
        /// Work requested from the child.
        goal: String,
        /// Native-authorized provider.
        provider: String,
        /// Exact saved input version.
        base: RecordDigest,
        /// Native-issued session provenance.
        origin: AgentOrigin,
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

    /// Objective identity for native session scoping.
    pub fn objective(&self) -> &str {
        &self.objective
    }

    /// Recover an existing exact command receipt without performing a mutation.
    pub(super) fn recorded(&mut self, request: &str) -> Result<Option<FleetEvent>, Error> {
        self.refresh()?;
        self.store
            .request(&self.objective, request)
            .map_err(Into::into)
    }

    /// Native orchestration helper: recover an identical command or submit at the latest revision.
    /// A competing writer still causes a typed stale-revision refusal.
    pub fn record(&mut self, request: &str, command: Command) -> Result<FleetEvent, Error> {
        self.refresh()?;
        let expected = self
            .store
            .request(&self.objective, request)?
            .map(|event| event.revision - 1)
            .unwrap_or(self.state.revision);
        self.submit(expected, request, command)
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
            limits_valid(limits)?;
            self.goal = Some(goal.clone());
            self.limits = Some(limits.clone());
            return Ok(());
        }
        let limits = self.limits.as_ref().ok_or(Error::Refused("not-started"))?;
        match command {
            Command::BeginFileDeletion { .. }
            | Command::PrepareFileDeletion { .. }
            | Command::FinishFileDeletion { .. } => self.apply_file_deletion(command)?,
            Command::Start { .. } => unreachable!("handled above"),
            Command::SetReviewChangeDecision {
                request,
                expected_revision,
                checkpoint,
            } => {
                if !self.review_change_requests.contains_key(request) {
                    return refuse("review-change-request-missing");
                }
                let before = self
                    .review_change_decisions
                    .get(request)
                    .cloned()
                    .unwrap_or_default();
                if before.revision != *expected_revision {
                    return refuse("review-change-decision-stale");
                }
                if before.revision >= 64 {
                    return refuse("review-change-decision-limit");
                }
                if &before.checkpoint == checkpoint {
                    return refuse("review-change-decision-unchanged");
                }
                if checkpoint.as_ref().is_some_and(|checkpoint| {
                    !self
                        .review_change_responses
                        .get(request)
                        .is_some_and(|responses| {
                            responses
                                .iter()
                                .any(|response| &response.checkpoint == checkpoint)
                        })
                }) {
                    return refuse("review-change-proposal-missing");
                }
                self.review_change_decisions.insert(
                    request.clone(),
                    ReviewChangeDecision {
                        revision: before.revision + 1,
                        checkpoint: checkpoint.clone(),
                    },
                );
            }
            Command::ProposeReviewChangeResult {
                request,
                checkpoint,
                origin,
            } => {
                if self.cancelled {
                    return refuse("objective-cancelled");
                }
                if self
                    .review_change_decisions
                    .get(request)
                    .is_some_and(|decision| decision.checkpoint.is_some())
                {
                    return refuse("review-change-request-addressed");
                }
                let feedback = self
                    .review_change_requests
                    .get(request)
                    .ok_or(Error::Refused("review-change-request-missing"))?;
                let saved = self
                    .checkpoints
                    .get(checkpoint)
                    .ok_or(Error::Refused("checkpoint-missing"))?;
                let result = saved
                    .result
                    .as_ref()
                    .filter(|result| result.complete)
                    .ok_or(Error::Refused("checkpoint-incomplete"))?;
                let bundle = saved
                    .review
                    .ok_or(Error::Refused("checkpoint-review-missing"))?;
                if saved.lane != feedback.lane || &saved.origin != origin {
                    return refuse("review-change-response-not-in-session");
                }
                if result.version == feedback.version || checkpoint == &feedback.checkpoint {
                    return refuse("review-change-response-unchanged");
                }
                let run = self
                    .lanes
                    .get(&feedback.lane)
                    .and_then(|lane| lane.runs.last())
                    .ok_or(Error::Refused("run-missing"))?;
                if run.id != origin.run
                    || !matches!(
                        run.state,
                        RunState::Launching | RunState::Running | RunState::Waiting
                    )
                {
                    return refuse("run-not-active");
                }
                let responses = self
                    .review_change_responses
                    .entry(request.clone())
                    .or_default();
                if responses
                    .iter()
                    .any(|response| response.checkpoint == *checkpoint)
                {
                    return refuse("review-change-response-exists");
                }
                if responses.len() >= 8 {
                    return refuse("review-change-response-limit");
                }
                responses.push(ReviewChangeResponse {
                    request: request.clone(),
                    checkpoint: checkpoint.clone(),
                    version: result.version,
                    bundle,
                    origin: origin.clone(),
                });
            }
            Command::RequestReviewChanges(request) => {
                id_valid(&request.id)?;
                id_valid(&request.lane)?;
                id_valid(&request.checkpoint)?;
                review_change_message_valid(&request.message)?;
                if self.review_change_requests.contains_key(&request.id) {
                    return refuse("review-change-request-exists");
                }
                if self.review_change_requests.len() >= 256
                    || self
                        .review_change_requests
                        .values()
                        .filter(|previous| previous.lane == request.lane)
                        .count()
                        >= 32
                {
                    return refuse("review-change-request-limit");
                }
                let checkpoint = self
                    .checkpoints
                    .get(&request.checkpoint)
                    .ok_or(Error::Refused("checkpoint-missing"))?;
                if checkpoint.lane != request.lane
                    || checkpoint.review != Some(request.bundle)
                    || !checkpoint
                        .result
                        .as_ref()
                        .is_some_and(|result| result.complete && result.version == request.version)
                {
                    return refuse("review-change-selection-mismatch");
                }
                self.review_change_requests
                    .insert(request.id.clone(), request.clone());
            }
            Command::ClaimRemoteLaunch {
                lane,
                run,
                assignment,
            } => {
                assignment.validate()?;
                if self.cancelled {
                    return refuse("objective-cancelled");
                }
                if self.lanes.values().flat_map(|lane| &lane.runs).any(|run| {
                    run.remote
                        .as_ref()
                        .is_some_and(|old| old.id == assignment.id)
                }) {
                    return refuse("remote-assignment-exists");
                }
                let target = self.lanes.get(lane).ok_or(Error::Refused("lane-missing"))?;
                if target.base != assignment.input {
                    return refuse("remote-input-mismatch");
                }
                let current = current_run(&mut self.lanes, lane, run)?;
                if current.state != RunState::Launching || current.launch_owner.is_some() {
                    return refuse("launch-needs-reconciliation");
                }
                current.launch_owner = Some(format!("remote:{}", assignment.id));
                current.remote = Some(assignment.clone());
            }
            Command::AdvanceRemoteLease {
                lane,
                run,
                assignment,
                worker_key,
                expected_sequence,
                lease_until_ms,
            } => {
                if self.cancelled {
                    return refuse("objective-cancelled");
                }
                let current = current_run(&mut self.lanes, lane, run)?;
                if !current.state.occupies_slot() || current.state == RunState::Stopping {
                    return refuse("remote-run-not-active");
                }
                current
                    .remote
                    .as_mut()
                    .ok_or(Error::Refused("remote-assignment-missing"))?
                    .advance(assignment, worker_key, *expected_sequence, *lease_until_ms)?;
            }
            Command::ClaimLaunch { lane, run, owner } => {
                id_valid(owner)?;
                if self.cancelled {
                    return refuse("objective-cancelled");
                }
                let current = current_run(&mut self.lanes, lane, run)?;
                if current.state != RunState::Launching || current.launch_owner.is_some() {
                    return refuse("launch-needs-reconciliation");
                }
                current.launch_owner = Some(owner.clone());
            }
            Command::SubmitReview { checkpoint, bundle } => {
                let checkpoint = self
                    .checkpoints
                    .get_mut(checkpoint)
                    .ok_or(Error::Refused("checkpoint-missing"))?;
                if !checkpoint
                    .result
                    .as_ref()
                    .is_some_and(|result| result.complete)
                {
                    return refuse("checkpoint-incomplete");
                }
                if checkpoint.review.is_some() {
                    return refuse("checkpoint-review-exists");
                }
                checkpoint.review = Some(*bundle);
            }
            Command::BeginCheckpoint {
                id,
                lane,
                origin,
                input_digest,
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
                if self.checkpoints.contains_key(id) {
                    return refuse("checkpoint-exists");
                }
                if self.checkpoints.len() >= 4096 {
                    return refuse("checkpoint-limit");
                }
                let run = current_run(&mut self.lanes, lane, &origin.run)?;
                if !matches!(
                    run.state,
                    RunState::Launching | RunState::Running | RunState::Waiting
                ) {
                    return refuse("run-not-saveable");
                }
                self.checkpoints.insert(
                    id.clone(),
                    Checkpoint {
                        lane: lane.clone(),
                        origin: origin.clone(),
                        input_digest: input_digest.clone(),
                        result: None,
                        review: None,
                    },
                );
            }
            Command::FinishCheckpoint { id, result } => {
                fold_digest_valid(&result.workspace_digest)?;
                if result.saved_changes > 1024 || result.complete != result.issue.is_none() {
                    return refuse("invalid-checkpoint-result");
                }
                if let Some(issue) = &result.issue {
                    id_valid(issue)?;
                }
                let checkpoint = self
                    .checkpoints
                    .get_mut(id)
                    .ok_or(Error::Refused("checkpoint-missing"))?;
                if checkpoint.result.is_some() {
                    return refuse("checkpoint-already-finished");
                }
                checkpoint.result = Some(result.clone());
                let lane = self
                    .lanes
                    .get_mut(&checkpoint.lane)
                    .ok_or(Error::Refused("lane-missing"))?;
                if result.complete
                    && lane
                        .runs
                        .last()
                        .is_some_and(|run| run.id == checkpoint.origin.run)
                {
                    lane.saved = Some(result.version);
                }
            }
            Command::CreateAttachedLane {
                id,
                project,
                goal,
                provider,
                base,
            } => {
                if RecordDigest::parse_hex(project).is_err()
                    || project.len() != 64
                    || project.bytes().any(|b| b.is_ascii_uppercase())
                {
                    return refuse("attachment-project-invalid");
                }
                self.apply(&Command::CreateLane {
                    id: id.clone(),
                    parent: None,
                    goal: goal.clone(),
                    provider: provider.clone(),
                    base: *base,
                })?;
                self.lanes
                    .get_mut(id)
                    .ok_or(Error::Refused("lane-missing"))?
                    .source_project = Some(project.clone());
            }
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
                        source_project: parent
                            .as_ref()
                            .and_then(|id| self.lanes.get(id))
                            .and_then(|lane| lane.source_project.clone()),
                        created_by: None,
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
            Command::Delegate {
                id,
                parent,
                goal,
                provider,
                base,
                origin,
            } => {
                id_valid(&origin.actor)?;
                id_valid(&origin.session)?;
                if origin.generation.len() != 32
                    || !origin.generation.bytes().all(|b| b.is_ascii_hexdigit())
                {
                    return refuse("invalid-agent-generation");
                }
                let run = self
                    .lanes
                    .get(parent)
                    .and_then(|lane| lane.runs.last())
                    .ok_or(Error::Refused("parent-run-missing"))?;
                if run.id != origin.run
                    || !matches!(
                        run.state,
                        RunState::Launching | RunState::Running | RunState::Waiting
                    )
                {
                    return refuse("parent-run-not-active");
                }
                self.apply(&Command::CreateLane {
                    id: id.clone(),
                    parent: Some(parent.clone()),
                    goal: goal.clone(),
                    provider: provider.clone(),
                    base: *base,
                })?;
                self.lanes
                    .get_mut(id)
                    .ok_or(Error::InvalidHistory)?
                    .created_by = Some(origin.clone());
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
                    launch_owner: None,
                    remote: None,
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
fn fold_digest_valid(value: &str) -> Result<(), Error> {
    if value.len() != 32
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return refuse("invalid-workspace-digest");
    }
    Ok(())
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
fn review_change_message_valid(message: &str) -> Result<(), Error> {
    if message.trim().is_empty() || message.len() > 8192
        || message.chars().any(|ch| (ch.is_control() && ch != '\n' && ch != '\t')
            || matches!(ch, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')) {
        return refuse("invalid-review-change-message");
    }
    Ok(())
}
fn limits_valid(limits: &Limits) -> Result<(), Error> {
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

#[cfg(target_os = "macos")]
pub use remote_admission::status::renewal::{
    RemoteLeaseRenewal, RemoteLeaseRenewalPlan, RemoteLeaseRenewalRequest,
    VerifiedRemoteLeaseRenewal,
};

#[cfg(target_os = "macos")]
pub use remote_admission::status::result::{RemoteSavedResultExport, RemoteSavedResultOffer};

#[cfg(target_os = "macos")]
pub use remote_admission::status::result::query::{
    inspect_remote_saved_result_over_ssh, RemoteSavedResultChallenge, RemoteSavedResultQuery,
    VerifiedRemoteSavedResultQuery,
};

#[cfg(target_os = "macos")]
pub use remote_admission::status::result::catalog::RemoteSavedResultPage;

#[cfg(target_os = "macos")]
pub use remote_admission::status::result::discovery::{
    discover_remote_saved_results_over_ssh, RemoteResultDiscoveryChallenge,
    RemoteResultDiscoveryQuery, VerifiedRemoteResultDiscoveryQuery,
};

#[cfg(target_os = "macos")]
pub use remote_input::{
    NativeRemoteResultReceiver, RemoteResultContentReceipt, RemoteResultEvidenceReceipt,
};

#[cfg(target_os = "macos")]
pub use remote_admission::status::result::transfer::{
    receive_remote_saved_result, receive_remote_saved_result_over_ssh,
};

#[cfg(target_os = "macos")]
mod remote_correspondence;
#[cfg(target_os = "macos")]
pub use remote_correspondence::RemoteResultCorrespondence;

#[cfg(target_os = "macos")]
pub use remote_admission::status::result::evidence::{
    receive_remote_result_evidence, receive_remote_result_evidence_over_ssh,
    AuthenticatedRemoteResultEvidence, RemoteResultEvidenceRequest,
};
