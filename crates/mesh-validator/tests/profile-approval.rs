//! No detected command executes before the user approves the project's validation profile.
//!
//! The first acceptance criterion of this task, asserted the way it can fail. The executor here is
//! a **recording** one that returns a clean pass, not a panicking one: an executor that panicked
//! would turn a leak into a test failure only if the leaked command were also run, whereas this
//! one makes every reached command visible in `calls` whether or not anything else notices. Every
//! assertion below is therefore of the form "the executor saw exactly this list".

mod support;

use std::cell::RefCell;

use mesh_policy::Operation;
use mesh_validator::{
    execute_plan, plan_validation, BlockReason, EnvironmentDigest, ExecutionFault, ExecutionReport,
    ExitStatus, IsolatedWorkspace, ProfileProposal, RunLedger, RunOutcome, RunVerdict,
    SandboxRequirement, SandboxedRun, ToolingInventory, ValidationAuthority, ValidationPlan,
    ValidatorExecutor, ValidatorRegistry,
};

use support::{
    approved, change, grant_for, human, registry_of, snapshot, validator, EPOCH, OTHER_WORKSPACE,
    ROTATED_EPOCH, WORKSPACE,
};

/// An executor that records every command it is handed and reports a clean pass.
///
/// The clean pass is deliberate. A leak past the profile gate shows up twice — as an unexpected
/// entry in `calls` and as a `Passed` record that the profile never authorised — and either one
/// fails the assertion.
#[derive(Default)]
struct Recording {
    calls: RefCell<Vec<String>>,
}

impl Recording {
    fn seen(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }
}

impl ValidatorExecutor for Recording {
    fn execute(&self, run: &SandboxedRun<'_>) -> Result<ExecutionReport, ExecutionFault> {
        self.calls.borrow_mut().push(run.command().to_line());
        Ok(ExecutionReport::new(
            ExitStatus::Code(0),
            EnvironmentDigest::of([("PATH".to_owned(), "/usr/bin".to_owned())]),
            run.snapshot(),
            [],
        ))
    }
}

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

/// Every step must appear in the ledger, blocked for `reason`, with nothing reaching the executor.
fn assert_all_blocked(ledger: &RunLedger, plan: &ValidationPlan, reason: &BlockReason) {
    assert_eq!(ledger.len(), plan.len());
    for record in ledger.records() {
        assert_eq!(record.verdict(), RunVerdict::Blocked);
        assert_eq!(
            record.outcome(),
            &RunOutcome::Blocked {
                reason: reason.clone()
            }
        );
        assert_eq!(
            record.exit(),
            None,
            "a step that never started has no exit status to report"
        );
        assert_eq!(record.environment(), None);
    }
    assert!(!ledger.clears(plan));
}

#[test]
fn with_no_approved_profile_nothing_reaches_the_executor() {
    let plan = plan_of(&["alpha", "beta", "gamma"]);
    let executor = Recording::default();
    let ledger = execute_plan(&authority(), None, &plan, &workspace(), &executor);

    assert_eq!(
        executor.seen(),
        Vec::<String>::new(),
        "a command executed before the user approved anything"
    );
    assert_all_blocked(&ledger, &plan, &BlockReason::AwaitingProfileApproval);
}

#[test]
fn a_command_outside_the_approved_profile_never_reaches_the_executor() {
    let approved_plan = plan_of(&["alpha"]);
    let profile = approved(&approved_plan);

    // The change later selects a second validator the user never saw.
    let wider = plan_of(&["alpha", "beta"]);
    let executor = Recording::default();
    let ledger = execute_plan(
        &authority(),
        Some(&profile),
        &wider,
        &workspace(),
        &executor,
    );

    assert_eq!(
        executor.seen(),
        vec!["alpha".to_owned()],
        "only the approved command may run"
    );

    let beta = ledger
        .records()
        .iter()
        .find(|record| record.validator().as_str() == "beta")
        .expect("every step has a record");
    assert_eq!(
        beta.outcome(),
        &RunOutcome::Blocked {
            reason: BlockReason::NotInProfile
        }
    );
    assert!(!ledger.clears(&wider), "an unapproved step cannot clear");
    assert!(ledger.clears(&approved_plan));
}

#[test]
fn a_profile_from_a_superseded_epoch_authorises_nothing() {
    let plan = plan_of(&["alpha"]);
    let stale = ProfileProposal::from_plan(&plan).approve(&human(9), WORKSPACE, ROTATED_EPOCH);
    assert!(!stale.in_force(EPOCH));

    let executor = Recording::default();
    let ledger = execute_plan(&authority(), Some(&stale), &plan, &workspace(), &executor);
    assert_eq!(executor.seen(), Vec::<String>::new());
    assert_all_blocked(&ledger, &plan, &BlockReason::ProfileEpochSuperseded);
}

#[test]
fn a_profile_approved_for_another_workspace_authorises_nothing() {
    let plan = plan_of(&["alpha"]);
    let elsewhere = ProfileProposal::from_plan(&plan).approve(&human(9), OTHER_WORKSPACE, EPOCH);

    let executor = Recording::default();
    let ledger = execute_plan(
        &authority(),
        Some(&elsewhere),
        &plan,
        &workspace(),
        &executor,
    );
    assert_eq!(executor.seen(), Vec::<String>::new());
    assert_all_blocked(&ledger, &plan, &BlockReason::ProfileWorkspaceMismatch);
}

#[test]
fn a_command_that_cannot_be_sandboxed_is_never_started_automatically() {
    let registry = ValidatorRegistry::empty()
        .with(validator("safe", "safe", SandboxRequirement::Isolated))
        .expect("registered")
        .with(validator(
            "risky",
            "risky",
            SandboxRequirement::HostAccess {
                why: "it needs the network".to_owned(),
            },
        ))
        .expect("registered");
    let plan = plan_validation(&registry, &change(), &ToolingInventory::empty());

    // The user approved BOTH commands. Approval is not enough for the unconfinable one.
    let profile = approved(&plan);
    assert!(profile.covers(&plan));

    let executor = Recording::default();
    let ledger = execute_plan(&authority(), Some(&profile), &plan, &workspace(), &executor);

    assert_eq!(executor.seen(), vec!["safe".to_owned()]);
    let risky = ledger
        .records()
        .iter()
        .find(|record| record.validator().as_str() == "risky")
        .expect("every step has a record");
    assert_eq!(
        risky.outcome(),
        &RunOutcome::Blocked {
            reason: BlockReason::NotSandboxable("it needs the network".to_owned())
        }
    );
    assert!(
        !ledger.clears(&plan),
        "a deferred step must not read as cleared"
    );

    // The decision is surfaced rather than swallowed: the plan names it, and so does the record.
    let surfaced: Vec<&str> = plan
        .needing_host_access()
        .map(|step| step.validator().as_str())
        .collect();
    assert_eq!(surfaced, vec!["risky"]);
}

#[test]
fn an_approved_profile_lets_exactly_the_approved_commands_run() {
    let plan = plan_of(&["alpha", "beta"]);
    let profile = approved(&plan);
    let executor = Recording::default();
    let ledger = execute_plan(&authority(), Some(&profile), &plan, &workspace(), &executor);

    assert_eq!(executor.seen(), vec!["alpha".to_owned(), "beta".to_owned()]);
    assert_eq!(ledger.len(), 2);
    assert!(ledger.clears(&plan));
    for record in ledger.records() {
        assert_eq!(record.verdict(), RunVerdict::Passed);
        assert_eq!(record.exit(), Some(ExitStatus::Code(0)));
        assert!(
            record.environment().is_some(),
            "an executed run records the environment it observed"
        );
        assert_eq!(record.snapshot(), snapshot());
    }
    assert!(!ledger.has_inconclusive());
}

#[test]
fn a_grant_for_another_operation_is_not_validation_authority() {
    for operation in Operation::ALL {
        if operation == Operation::AdvanceCanonicalHead {
            // Not askable at this tier: `authorize` denies it unconditionally because
            // `DelegatedAction` has no value to compare against. Nothing to narrow.
            continue;
        }
        let grant = grant_for(operation);
        let narrowed = ValidationAuthority::from_grant(&grant);
        assert_eq!(
            narrowed.is_ok(),
            operation == Operation::RunValidation,
            "`{operation}` narrowed to validation authority incorrectly"
        );
    }
}

#[test]
fn the_proposal_shown_to_the_user_is_exactly_what_the_profile_admits() {
    let plan = plan_of(&["alpha", "beta"]);
    let proposal = ProfileProposal::from_plan(&plan);
    let profile = proposal.approve(&human(9), WORKSPACE, EPOCH);

    assert_eq!(proposal.len(), profile.len());
    for command in proposal.commands() {
        assert!(profile.admits(command));
    }
    assert_eq!(profile.proposal_digest(), proposal.digest());
    assert_eq!(profile.approver(), human(9).key());
}
