//! The attack: making a validator's own output decide what it is judging.
//!
//! `tests/workspace-isolation.rs` establishes that a run which *changed* its subject cannot read as
//! a pass, and that a plan step with no record at all cannot read as cleared. Both are about a
//! single pass over a single review. This file attacks the layer above: the ledger is the artefact
//! that leaves this crate and travels into a review bundle, so the question a reviewer actually
//! relies on is not "did this run pass" but **"is this evidence evidence of *this* review"**.
//!
//! Two ways to answer no, both constructed here as an attacker would:
//!
//! 1. **Replay.** Evidence produced against one immutable review snapshot is offered for another.
//!    Neither the validator identifier nor the command digest mentions the snapshot — `cargo
//!    nextest run` is `cargo nextest run` whatever it was pointed at — so an oracle keyed on those
//!    two clears a review nothing ever ran against. The actor chooses which ledger to submit, which
//!    makes this the transferable form of a validator deciding its own verdict.
//! 2. **Drowning a failure.** A failing record for a planned step sits next to a passing one for
//!    the same step. An oracle asking "does a passing record exist" says yes; there is no ordering
//!    in a ledger that could justify letting the pass win.
//!
//! Both must fail closed. [`mesh_validator::RunLedger::clears`] is the only oracle in the crate, so
//! both are asserted against it.

mod support;

use mesh_policy::Operation;
use mesh_types::Digest32;
use mesh_validator::{
    execute_plan, plan_validation, ChangedPath, EnvironmentDigest, ExecutionFault, ExecutionReport,
    ExitStatus, IsolatedWorkspace, PathEdit, ReviewChange, RunLedger, RunOutcome, SandboxedRun,
    ToolingInventory, ValidationAuthority, ValidationPlan, ValidationRecord, ValidatorExecutor,
};

use support::{approved, grant_for, registry_of, snapshot};

/// A second review snapshot: the one nothing was ever run against.
fn other_snapshot() -> Digest32 {
    Digest32::from_bytes([0x22; 32])
}

/// The same one-path change, described against whichever snapshot is named.
fn change_against(snapshot: Digest32) -> ReviewChange {
    ReviewChange::against(snapshot)
        .with_path(ChangedPath::new("src/lib.rs", PathEdit::Modified, 128).expect("a legal path"))
        .expect("no duplicate")
}

fn authority() -> ValidationAuthority {
    ValidationAuthority::from_grant(&grant_for(Operation::RunValidation))
        .expect("a grant for validation")
}

fn plan_for(snapshot: Digest32) -> ValidationPlan {
    plan_validation(
        &registry_of(&["alpha"]),
        &change_against(snapshot),
        &ToolingInventory::empty(),
    )
}

/// An executor that passes, honestly: it reports back the snapshot it was handed.
struct Passing;

impl ValidatorExecutor for Passing {
    fn execute(&self, run: &SandboxedRun<'_>) -> Result<ExecutionReport, ExecutionFault> {
        Ok(ExecutionReport::new(
            ExitStatus::Code(0),
            EnvironmentDigest::empty(),
            run.snapshot(),
            [],
        ))
    }
}

#[test]
fn evidence_from_one_review_cannot_clear_another_review() {
    // The attacker's position: a review that genuinely passed, and a second review of a different
    // subject whose plan is — necessarily — identical, because a plan says what to run and not what
    // to run it against.
    let passed = plan_for(snapshot());
    let unrun = plan_for(other_snapshot());
    assert_eq!(
        passed.steps().len(),
        unrun.steps().len(),
        "the fixtures must select the same validators for the attack to be the interesting one"
    );

    let ledger = execute_plan(
        &authority(),
        Some(&approved(&passed)),
        &passed,
        &IsolatedWorkspace::over(snapshot(), "").expect("a legal root"),
        &Passing,
    );
    assert!(
        ledger.clears(&passed),
        "the honest run must clear the review it was run against"
    );
    for record in ledger.records() {
        assert_eq!(record.snapshot(), snapshot());
    }

    assert!(
        !ledger.clears(&unrun),
        "evidence produced against one review snapshot cleared a plan for another: a validator's \
         output decided the verdict of a change it never saw"
    );
}

#[test]
fn a_plan_carries_the_review_it_was_planned_against() {
    // Without this, `clears` cannot express the question the previous test asks: the oracle would
    // have no value to compare a record's snapshot against.
    assert_eq!(plan_for(snapshot()).snapshot(), snapshot());
    assert_eq!(plan_for(other_snapshot()).snapshot(), other_snapshot());
    assert_ne!(
        plan_for(snapshot()).digest(),
        plan_for(other_snapshot()).digest(),
        "two plans for two different reviews shared one identity"
    );
}

#[test]
fn a_recorded_failure_is_not_cancelled_by_a_pass_for_the_same_step() {
    // A ledger has no ordering that could justify "the later one wins", so a step with a failure
    // recorded against it is not cleared by a pass sitting beside it. Assembled directly, because
    // the reachable route is a caller merging what two passes produced.
    let plan = plan_for(snapshot());
    let step = &plan.steps()[0];

    let outcome_of = |exit: ExitStatus| {
        ValidationRecord::new(
            step.validator().clone(),
            step.command().clone(),
            Some(EnvironmentDigest::empty()),
            snapshot(),
            RunOutcome::Executed { exit },
            [],
        )
    };
    let failed = outcome_of(ExitStatus::Code(1));
    let passed = outcome_of(ExitStatus::Code(0));

    assert!(
        RunLedger::of([passed.clone()]).clears(&plan),
        "a lone pass must clear"
    );
    for ledger in [
        RunLedger::of([failed.clone(), passed.clone()]),
        RunLedger::of([passed, failed]),
    ] {
        assert!(
            !ledger.clears(&plan),
            "a recorded failure for a planned step was cancelled by a pass beside it"
        );
    }
}

#[test]
fn a_run_against_the_wrong_snapshot_cannot_be_smuggled_in_beside_an_honest_one() {
    // The mixed case: the ledger holds a genuine record for one step and a replayed one for
    // another. Whole-plan clearance must still be refused.
    let plan = plan_validation(
        &registry_of(&["alpha", "beta"]),
        &change_against(snapshot()),
        &ToolingInventory::empty(),
    );
    assert_eq!(plan.steps().len(), 2);

    let honest = ValidationRecord::new(
        plan.steps()[0].validator().clone(),
        plan.steps()[0].command().clone(),
        Some(EnvironmentDigest::empty()),
        snapshot(),
        RunOutcome::Executed {
            exit: ExitStatus::Code(0),
        },
        [],
    );
    let replayed = ValidationRecord::new(
        plan.steps()[1].validator().clone(),
        plan.steps()[1].command().clone(),
        Some(EnvironmentDigest::empty()),
        other_snapshot(),
        RunOutcome::Executed {
            exit: ExitStatus::Code(0),
        },
        [],
    );

    let ledger = RunLedger::of([honest, replayed]);
    assert_eq!(ledger.len(), 2, "both records must be present to be judged");
    assert!(
        !ledger.clears(&plan),
        "a passing record from another review satisfied a step of this one"
    );
}
