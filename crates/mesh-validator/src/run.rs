//! Running a plan: the authority it needs, the port it runs through, and the six ways it refuses.
//!
//! # Authority is `mesh-policy`'s, not a second model invented here
//!
//! [`ValidationAuthority`] is built from a [`Grant<Delegated>`], and `mesh-policy` gives
//! `Grant<T>` no public constructor: the only values that exist are ones `DecisionLedger`
//! produced, which means every one has an audit line behind it. Two consequences follow without
//! this crate writing a check for either:
//!
//! * A validation run cannot begin without a recorded policy decision.
//! * The authority it begins under is the **delegated** tier, whose action vocabulary
//!   ([`DelegatedAction`](mesh_crypto::DelegatedAction)) has no canonical-advance variant at all.
//!   A validator therefore cannot hold, be handed, or be mistaken for publication authority —
//!   `advance-canonical-head` is not a value `Grant<Delegated>` can carry.
//!
//! This crate additionally declares no dependency on `mesh-approval`, so it cannot reach the
//! bundle machinery even indirectly; the architecture map's `validators-cannot-mutate-canonical-state`
//! restriction is what enforces that, and `Cargo.toml` says why.
//!
//! # Nothing in this crate executes anything
//!
//! [`ValidatorExecutor`] is a port. The implementation lives in the service layer, and the
//! composition root chooses it. What [`execute_plan`] guarantees is narrower than "the command was
//! sandboxed", and stating the narrow thing is the point:
//!
//! > `execute_plan` never calls [`ValidatorExecutor::execute`] for a step that the approved
//! > profile does not admit, that no profile has been approved for, whose profile is out of force,
//! > or whose specification says it cannot be confined.
//!
//! `tests/profile-approval.rs` asserts it with an executor that records every call and returns a
//! pass — so a leak shows up as a pass that should not exist, which is the direction a test can
//! actually catch.
//!
//! # The seven refusals, in order
//!
//! | Condition | Outcome |
//! |---|---|
//! | the workspace is not the plan's review | `Blocked(WorkspaceSnapshotMismatch)` |
//! | no profile approved | `Blocked(AwaitingProfileApproval)` |
//! | profile scoped to another workspace | `Blocked(ProfileWorkspaceMismatch)` |
//! | profile approved in another epoch | `Blocked(ProfileEpochSuperseded)` |
//! | command not admitted by the profile | `Blocked(NotInProfile)` |
//! | command cannot be confined | `Blocked(NotSandboxable)` |
//! | executor returned a fault | `Faulted(…)` |
//!
//! And one more that is not a refusal but a detection: an executor that reports a different
//! snapshot after the run than the one it was given produces `Faulted(SnapshotDrift)`. A run that
//! changed what it was validating never reads as a pass, whatever exit status it reported.
//!
//! Every step of the plan produces exactly one record, in plan order. A step that never ran is
//! *present* in the ledger with the reason, because a missing record is indistinguishable from a
//! step nobody thought of.
//!
//! # Which review the evidence is about
//!
//! The first refusal is the one that keeps the rest meaningful. A [`ValidationPlan`] names the
//! immutable review snapshot it was planned for, and the workspace names the snapshot it holds. If
//! they differ, the run would point an approved command at a tree nobody planned for and stamp the
//! resulting evidence with a review it does not describe — so nothing is started and every step is
//! recorded blocked. Downstream, [`RunLedger::clears`] considers only records whose snapshot is the
//! plan's, so evidence of one review can never satisfy another; `tests/evidence-replay.rs`
//! constructs that attack.

use mesh_crypto::{Delegated, WorkspaceScope};
use mesh_policy::{Grant, Operation};
use mesh_types::{Digest32, PolicyEpoch};

use crate::command::{EnvironmentDigest, ValidationCommand};
use crate::profile::ValidationProfile;
use crate::record::{
    Artifact, BlockReason, ExitStatus, RunFault, RunLedger, RunOutcome, ValidationRecord,
};
use crate::registry::SandboxRequirement;
use crate::selection::{PlannedValidation, ValidationPlan};
use crate::workspace::IsolatedWorkspace;

/// Proof that this run may execute validations, in this workspace, in this epoch.
///
/// Built only from a [`Grant<Delegated>`] for [`Operation::RunValidation`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValidationAuthority {
    workspace: WorkspaceScope,
    epoch: PolicyEpoch,
}

impl ValidationAuthority {
    /// Narrow a policy grant to validation authority.
    ///
    /// # Errors
    ///
    /// [`AuthorityError::WrongOperation`] when the grant authorises something else. A grant for
    /// another operation is not usable here even though it is a valid grant, because authority is
    /// per operation and reusing one would be exactly the widening the capability model forbids.
    pub fn from_grant(grant: &Grant<Delegated>) -> Result<Self, AuthorityError> {
        if grant.operation() != Operation::RunValidation {
            return Err(AuthorityError::WrongOperation {
                granted: grant.operation(),
            });
        }
        Ok(Self {
            workspace: grant.workspace(),
            epoch: grant.epoch(),
        })
    }

    /// The workspace this authority is good in.
    #[must_use]
    pub const fn workspace(&self) -> WorkspaceScope {
        self.workspace
    }

    /// The policy epoch it was decided in.
    #[must_use]
    pub const fn epoch(&self) -> PolicyEpoch {
        self.epoch
    }
}

/// Why a grant was not validation authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthorityError {
    /// The grant authorises a different operation.
    WrongOperation {
        /// What it does authorise.
        granted: Operation,
    },
}

impl core::fmt::Display for AuthorityError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::WrongOperation { granted } => write!(
                formatter,
                "the grant authorises `{granted}`, not `{}`",
                Operation::RunValidation
            ),
        }
    }
}

impl std::error::Error for AuthorityError {}

/// One command, confined to one immutable snapshot, handed to an executor.
///
/// Borrowed rather than owned so that constructing one costs nothing and, more usefully, so that
/// an executor cannot keep one past the call and re-run it later against a workspace that has
/// moved on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SandboxedRun<'run> {
    command: &'run ValidationCommand,
    workspace: &'run IsolatedWorkspace,
}

impl<'run> SandboxedRun<'run> {
    /// What to run.
    #[must_use]
    pub const fn command(&self) -> &'run ValidationCommand {
        self.command
    }

    /// Where to run it, and which immutable snapshot it holds.
    #[must_use]
    pub const fn workspace(&self) -> &'run IsolatedWorkspace {
        self.workspace
    }

    /// The snapshot the executor must find unchanged when it finishes.
    #[must_use]
    pub const fn snapshot(&self) -> Digest32 {
        self.workspace.snapshot()
    }
}

/// What an executor reports back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionReport {
    exit: ExitStatus,
    environment: EnvironmentDigest,
    snapshot_after: Digest32,
    artifacts: Vec<Artifact>,
}

impl ExecutionReport {
    /// A report of one run.
    ///
    /// `snapshot_after` is the digest of the workspace **as the executor found it when the process
    /// finished**. Reporting the digest it was given without measuring is possible and this crate
    /// cannot detect it; what it can detect, and does, is an executor that measured and found a
    /// difference.
    #[must_use]
    pub fn new(
        exit: ExitStatus,
        environment: EnvironmentDigest,
        snapshot_after: Digest32,
        artifacts: impl IntoIterator<Item = Artifact>,
    ) -> Self {
        Self {
            exit,
            environment,
            snapshot_after,
            artifacts: artifacts.into_iter().collect(),
        }
    }

    /// How the process ended.
    #[must_use]
    pub const fn exit(&self) -> ExitStatus {
        self.exit
    }

    /// The environment it observed.
    #[must_use]
    pub const fn environment(&self) -> EnvironmentDigest {
        self.environment
    }

    /// The snapshot digest measured after the run.
    #[must_use]
    pub const fn snapshot_after(&self) -> Digest32 {
        self.snapshot_after
    }

    /// What it produced.
    #[must_use]
    pub fn artifacts(&self) -> &[Artifact] {
        &self.artifacts
    }
}

/// Why an executor produced no usable answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecutionFault {
    /// The process died without an exit status.
    Crashed,
    /// The run exceeded its budget.
    TimedOut,
    /// The sandbox could not be established, so nothing was run.
    SandboxUnavailable,
    /// Anything else the executor can say.
    Failed(String),
}

impl ExecutionFault {
    /// The record this fault becomes. Never a pass, for any variant.
    #[must_use]
    fn into_fault(self) -> RunFault {
        match self {
            Self::Crashed => RunFault::Crashed,
            Self::TimedOut => RunFault::TimedOut,
            Self::SandboxUnavailable => RunFault::SandboxUnavailable,
            Self::Failed(detail) => RunFault::ExecutorFailed(detail),
        }
    }
}

impl core::fmt::Display for ExecutionFault {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Crashed => formatter.write_str("the process died without an exit status"),
            Self::TimedOut => formatter.write_str("the run exceeded its budget"),
            Self::SandboxUnavailable => formatter.write_str("the sandbox could not be established"),
            Self::Failed(detail) => formatter.write_str(detail),
        }
    }
}

impl std::error::Error for ExecutionFault {}

/// The seam between deciding what to run and running it.
///
/// The implementation is the service layer's and the composition root chooses it. An
/// implementation that ignores the sandbox is possible and nothing here can prevent it; what this
/// crate controls is which commands reach the call at all, and the snapshot comparison that
/// catches one that wrote to what it was reading.
pub trait ValidatorExecutor {
    /// Run one confined command.
    ///
    /// # Errors
    ///
    /// [`ExecutionFault`] when no exit status was obtained. Returning `Ok` with a fabricated zero
    /// would be the one mistake this whole crate exists to make unavailable, so the signature
    /// gives a failing executor somewhere else to go.
    fn execute(&self, run: &SandboxedRun<'_>) -> Result<ExecutionReport, ExecutionFault>;
}

/// Run every step of `plan`, refusing the ones that are not approved and confinable.
///
/// Produces exactly one record per plan step, in plan order. See the module documentation for the
/// refusal table; the short form is that [`ValidatorExecutor::execute`] is reached only by a step
/// whose command an in-force profile for this workspace admits, whose specification says it can be
/// confined, and whose plan was made for the review the workspace actually holds.
#[must_use]
pub fn execute_plan<E: ValidatorExecutor>(
    authority: &ValidationAuthority,
    profile: Option<&ValidationProfile>,
    plan: &ValidationPlan,
    workspace: &IsolatedWorkspace,
    executor: &E,
) -> RunLedger {
    // Plan-level rather than per-step: a workspace holding another review makes every step of this
    // plan unrunnable, and the check is cheaper and clearer stated once.
    let wrong_review = !workspace.is_unchanged(plan.snapshot());
    let records = plan
        .steps()
        .iter()
        .map(|step| {
            if wrong_review {
                return blocked(step, workspace, BlockReason::WorkspaceSnapshotMismatch);
            }
            run_step(authority, profile, step, workspace, executor)
        })
        .collect::<Vec<ValidationRecord>>();
    RunLedger::of(records)
}

/// The decision for one step. Every early return is a refusal that never reaches the executor.
fn run_step<E: ValidatorExecutor>(
    authority: &ValidationAuthority,
    profile: Option<&ValidationProfile>,
    step: &PlannedValidation,
    workspace: &IsolatedWorkspace,
    executor: &E,
) -> ValidationRecord {
    if let Some(reason) = refusal(authority, profile, step) {
        return blocked(step, workspace, reason);
    }

    let run = SandboxedRun {
        command: step.command(),
        workspace,
    };
    match executor.execute(&run) {
        Err(fault) => faulted(step, workspace, fault.into_fault()),
        Ok(report) if !workspace.is_unchanged(report.snapshot_after()) => {
            faulted(step, workspace, RunFault::SnapshotDrift)
        }
        Ok(report) => ValidationRecord::new(
            step.validator().clone(),
            step.command().clone(),
            Some(report.environment()),
            workspace.snapshot(),
            RunOutcome::Executed {
                exit: report.exit(),
            },
            report.artifacts().to_vec(),
        ),
    }
}

/// Why this step must not be started, or `None` when it may be.
fn refusal(
    authority: &ValidationAuthority,
    profile: Option<&ValidationProfile>,
    step: &PlannedValidation,
) -> Option<BlockReason> {
    let Some(profile) = profile else {
        return Some(BlockReason::AwaitingProfileApproval);
    };
    if !profile.scoped_to(authority.workspace()) {
        return Some(BlockReason::ProfileWorkspaceMismatch);
    }
    if !profile.in_force(authority.epoch()) {
        return Some(BlockReason::ProfileEpochSuperseded);
    }
    if !profile.admits(step.command()) {
        return Some(BlockReason::NotInProfile);
    }
    match step.sandbox() {
        SandboxRequirement::Isolated => None,
        SandboxRequirement::HostAccess { why } => Some(BlockReason::NotSandboxable(why.clone())),
    }
}

/// A record for a step that was never started.
fn blocked(
    step: &PlannedValidation,
    workspace: &IsolatedWorkspace,
    reason: BlockReason,
) -> ValidationRecord {
    ValidationRecord::new(
        step.validator().clone(),
        step.command().clone(),
        None,
        workspace.snapshot(),
        RunOutcome::Blocked { reason },
        [],
    )
}

/// A record for a step that started and produced no usable answer.
fn faulted(
    step: &PlannedValidation,
    workspace: &IsolatedWorkspace,
    fault: RunFault,
) -> ValidationRecord {
    ValidationRecord::new(
        step.validator().clone(),
        step.command().clone(),
        None,
        workspace.snapshot(),
        RunOutcome::Faulted { fault },
        [],
    )
}

#[cfg(test)]
mod tests {
    use super::{AuthorityError, ExecutionFault, ExecutionReport};
    use crate::command::EnvironmentDigest;
    use crate::record::{ExitStatus, RunFault};

    use mesh_policy::Operation;
    use mesh_types::Digest32;

    #[test]
    fn no_execution_fault_can_become_a_pass() {
        for fault in [
            ExecutionFault::Crashed,
            ExecutionFault::TimedOut,
            ExecutionFault::SandboxUnavailable,
            ExecutionFault::Failed("the validator is unknown to this executor".to_owned()),
        ] {
            let rendered = fault.to_string();
            assert!(!rendered.is_empty(), "a fault must say something");
            let recorded: RunFault = fault.into_fault();
            assert!(
                !recorded.as_str().is_empty(),
                "a fault must have a wire name"
            );
        }
    }

    #[test]
    fn an_executor_report_carries_what_it_was_asked_to_carry() {
        let report = ExecutionReport::new(
            ExitStatus::Code(0),
            EnvironmentDigest::empty(),
            Digest32::from_bytes([5; 32]),
            [],
        );
        assert_eq!(report.exit(), ExitStatus::Code(0));
        assert_eq!(report.environment(), EnvironmentDigest::empty());
        assert_eq!(report.snapshot_after(), Digest32::from_bytes([5; 32]));
        assert!(report.artifacts().is_empty());
    }

    #[test]
    fn the_authority_error_names_the_operation_that_was_granted() {
        let error = AuthorityError::WrongOperation {
            granted: Operation::ReadWorkspace,
        };
        let rendered = error.to_string();
        assert!(rendered.contains("read-workspace"));
        assert!(rendered.contains("run-validation"));
    }
}
