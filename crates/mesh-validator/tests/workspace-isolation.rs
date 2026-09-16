//! The validation workspace cannot mutate the actor's state, and a validator that fails to answer
//! never reads as a pass.
//!
//! Two of the task's three test obligations live here. What each one can and cannot establish is
//! stated next to it, because the honest form of the isolation claim is narrower than "the
//! validator is sandboxed":
//!
//! * This crate cannot write anything — it names no ambient module and holds no process — so the
//!   *library* half of the isolation claim is a compile-time assertion in `no_ambient_io`, not a
//!   runtime test.
//! * The *executor* half cannot be proven from here. What is checked instead is the detection:
//!   an executor that changed the snapshot under itself produces `SnapshotDrift`, and a validator
//!   that came back with no usable answer produces an inconclusive record. Neither can produce a
//!   pass.

mod support;

use std::cell::RefCell;

use mesh_policy::Operation;
use mesh_types::Digest32;
use mesh_validator::{
    execute_plan, plan_validation, Artifact, BlockReason, EnvironmentDigest, ExecutionFault,
    ExecutionReport, ExitStatus, IsolatedWorkspace, RunFault, RunOutcome, RunVerdict, SandboxedRun,
    ToolingInventory, ValidationAuthority, ValidationPlan, ValidatorExecutor,
};

use support::{approved, change, grant_for, registry_of, snapshot};

fn workspace() -> IsolatedWorkspace {
    IsolatedWorkspace::over(snapshot(), "").expect("a legal root")
}

fn authority() -> ValidationAuthority {
    ValidationAuthority::from_grant(&grant_for(Operation::RunValidation))
        .expect("a grant for validation")
}

fn plan_of(names: &[&str]) -> ValidationPlan {
    plan_validation(&registry_of(names), &change(), &ToolingInventory::empty())
}

/// An executor that reports success and a DIFFERENT snapshot: it wrote to what it was validating.
struct Meddling;

impl ValidatorExecutor for Meddling {
    fn execute(&self, _run: &SandboxedRun<'_>) -> Result<ExecutionReport, ExecutionFault> {
        Ok(ExecutionReport::new(
            ExitStatus::Code(0),
            EnvironmentDigest::empty(),
            Digest32::from_bytes([0xee; 32]),
            [],
        ))
    }
}

/// An executor that always faults with whatever it was told to.
struct Faulting(ExecutionFault);

impl ValidatorExecutor for Faulting {
    fn execute(&self, _run: &SandboxedRun<'_>) -> Result<ExecutionReport, ExecutionFault> {
        Err(self.0.clone())
    }
}

/// An executor that records the snapshot every run was handed.
#[derive(Default)]
struct Observing {
    snapshots: RefCell<Vec<Digest32>>,
}

impl ValidatorExecutor for Observing {
    fn execute(&self, run: &SandboxedRun<'_>) -> Result<ExecutionReport, ExecutionFault> {
        self.snapshots.borrow_mut().push(run.snapshot());
        Ok(ExecutionReport::new(
            ExitStatus::Code(0),
            EnvironmentDigest::empty(),
            run.snapshot(),
            [
                Artifact::new("stdout.txt", Digest32::from_bytes([7; 32]), 12)
                    .expect("a legal artifact name"),
            ],
        ))
    }
}

#[test]
fn an_isolated_workspace_cannot_name_a_path_outside_itself() {
    // The structural half of "cannot mutate the actor's state": every root that could reach out of
    // the sandbox is refused at construction, so no handle naming one exists to be handed on.
    for escaping in [
        "/",
        "/Users/somebody/.mesh/actor-state",
        "..",
        "../../.mesh",
        "state/../../..",
        "./state",
    ] {
        assert!(
            IsolatedWorkspace::over(snapshot(), escaping).is_err(),
            "`{escaping}` was accepted as an isolated workspace root"
        );
    }
}

#[test]
fn every_run_is_handed_the_immutable_review_snapshot_and_no_other_value() {
    let plan = plan_of(&["alpha", "beta"]);
    let executor = Observing::default();
    let ledger = execute_plan(
        &authority(),
        Some(&approved(&plan)),
        &plan,
        &workspace(),
        &executor,
    );

    assert_eq!(
        executor.snapshots.borrow().as_slice(),
        [snapshot(), snapshot()],
        "a run was handed something other than the review snapshot"
    );
    for record in ledger.records() {
        assert_eq!(record.snapshot(), snapshot());
        assert_eq!(record.artifacts().len(), 1);
        assert_eq!(record.artifacts()[0].name(), "stdout.txt");
    }
}

#[test]
fn a_run_that_changed_what_it_was_validating_is_a_fault_and_not_a_pass() {
    let plan = plan_of(&["alpha"]);
    let ledger = execute_plan(
        &authority(),
        Some(&approved(&plan)),
        &plan,
        &workspace(),
        &Meddling,
    );

    assert_eq!(ledger.len(), 1);
    let record = &ledger.records()[0];
    assert_eq!(
        record.outcome(),
        &RunOutcome::Faulted {
            fault: RunFault::SnapshotDrift
        },
        "an executor reporting a changed snapshot AND a zero exit was read as a pass"
    );
    assert_eq!(record.verdict(), RunVerdict::Errored);
    assert!(!record.verdict().is_pass());
    assert!(ledger.has_inconclusive());
    assert!(!ledger.clears(&plan));
}

#[test]
fn no_executor_fault_produces_a_pass() {
    let plan = plan_of(&["alpha"]);
    let profile = approved(&plan);
    let cases = [
        (ExecutionFault::Crashed, RunFault::Crashed),
        (ExecutionFault::TimedOut, RunFault::TimedOut),
        (
            ExecutionFault::SandboxUnavailable,
            RunFault::SandboxUnavailable,
        ),
        (
            ExecutionFault::Failed("no such validator on this host".to_owned()),
            RunFault::ExecutorFailed("no such validator on this host".to_owned()),
        ),
    ];

    for (fault, expected) in cases {
        let ledger = execute_plan(
            &authority(),
            Some(&profile),
            &plan,
            &workspace(),
            &Faulting(fault.clone()),
        );
        assert_eq!(ledger.len(), 1, "a faulted step must still be recorded");
        let record = &ledger.records()[0];
        assert_eq!(
            record.outcome(),
            &RunOutcome::Faulted {
                fault: expected.clone()
            }
        );
        assert!(!record.verdict().is_pass(), "`{fault}` was read as a pass");
        assert_eq!(record.exit(), None);
        assert_eq!(record.environment(), None);
        assert!(!ledger.clears(&plan));
    }
}

#[test]
fn a_ledger_missing_a_step_does_not_clear_the_plan() {
    // The absent-evidence case: a ledger with NO record for a planned step is not a clean run,
    // which is what a "no failures recorded" check would have called clean.
    let narrow = plan_of(&["alpha"]);
    let wider = plan_of(&["alpha", "beta"]);
    let ledger = execute_plan(
        &authority(),
        Some(&approved(&narrow)),
        &narrow,
        &workspace(),
        &Observing::default(),
    );

    assert!(ledger.clears(&narrow));
    assert!(
        !ledger.clears(&wider),
        "a plan step with no record at all was treated as cleared"
    );
}

#[test]
fn an_empty_plan_hands_the_executor_nothing() {
    let executor = Observing::default();
    let nothing = ValidationPlan::nothing_for(snapshot());
    let ledger = execute_plan(&authority(), None, &nothing, &workspace(), &executor);
    assert!(executor.snapshots.borrow().is_empty());
    assert!(ledger.is_empty());
    assert!(ledger.clears(&nothing));
}

#[test]
fn a_workspace_holding_another_review_starts_nothing() {
    // The plan says which review it is for; the workspace says which one it holds. When they
    // disagree the run would point an approved command at a tree nobody planned for, so no step is
    // started and none of them clears.
    let plan = plan_of(&["alpha", "beta"]);
    let executor = Observing::default();
    let elsewhere =
        IsolatedWorkspace::over(Digest32::from_bytes([0x33; 32]), "").expect("a legal root");

    let ledger = execute_plan(
        &authority(),
        Some(&approved(&plan)),
        &plan,
        &elsewhere,
        &executor,
    );

    assert!(
        executor.snapshots.borrow().is_empty(),
        "a command was started against a review the plan was not made for"
    );
    assert_eq!(ledger.len(), plan.steps().len());
    for record in ledger.records() {
        assert_eq!(
            record.outcome(),
            &RunOutcome::Blocked {
                reason: BlockReason::WorkspaceSnapshotMismatch
            }
        );
        assert!(!record.verdict().is_pass());
        assert!(
            !record.outcome().exit().is_some(),
            "a step that never started has no exit status"
        );
    }
    assert!(!ledger.clears(&plan));
    assert!(
        !BlockReason::WorkspaceSnapshotMismatch.clears_with_approval(),
        "approving a profile must not make this go away"
    );
}

#[test]
fn the_ledger_digest_changes_when_the_evidence_changes() {
    let plan = plan_of(&["alpha"]);
    let profile = approved(&plan);
    let passing = execute_plan(
        &authority(),
        Some(&profile),
        &plan,
        &workspace(),
        &Observing::default(),
    );
    let faulted = execute_plan(
        &authority(),
        Some(&profile),
        &plan,
        &workspace(),
        &Faulting(ExecutionFault::Crashed),
    );
    assert_ne!(
        passing.digest(),
        faulted.digest(),
        "two runs with different evidence share a ledger digest"
    );
}
