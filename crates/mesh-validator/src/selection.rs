//! Automatic validator planning: which validators a change selects, and in what order.
//!
//! # The determinism claim, and what backs it
//!
//! Selection is a pure function of three values — the registry, the change and the tooling
//! inventory — and all three impose their own order. The registry is a `BTreeMap`, the change's
//! paths are sorted on the way in and its operation set is a `BTreeSet`, and every trigger is a
//! total predicate over those. So the same three values produce the same [`ValidationPlan`] with
//! the same steps in the same order, and [`ValidationPlan::digest`] turns that into one value a
//! test can compare. `tests/validator-selection.rs` asserts it across the detection matrix and
//! across shuffled inputs.
//!
//! # Nothing here runs anything
//!
//! A plan is a **proposal**. Producing one starts no process and touches nothing: it is the input
//! to [`ProfileProposal`](crate::ProfileProposal), which the user approves once, and only then
//! does [`execute_plan`](crate::execute_plan) hand anything to an executor. Planning and executing
//! are separate functions in separate modules precisely so that "we planned it" can never be
//! mistaken for "we ran it".

use mesh_types::{Absorb, Blake3, ContentDigest, Digest32, DigestHasher, DigestWriter, DomainTag};

use crate::change::ReviewChange;
use crate::command::ValidationCommand;
use crate::registry::{SandboxRequirement, ValidatorId, ValidatorRegistry};
use crate::tooling::ToolingInventory;
use crate::trigger::ValidationTrigger;

/// The domain a plan's digest is derived in.
const PLAN_DOMAIN: DomainTag = DomainTag::new("mesh.v0.validator.plan");

/// One validator the change selected, with the reasons it was selected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedValidation {
    validator: ValidatorId,
    command: ValidationCommand,
    sandbox: SandboxRequirement,
    reasons: Vec<ValidationTrigger>,
}

impl PlannedValidation {
    /// Which validator.
    #[must_use]
    pub const fn validator(&self) -> &ValidatorId {
        &self.validator
    }

    /// What it would run.
    #[must_use]
    pub const fn command(&self) -> &ValidationCommand {
        &self.command
    }

    /// How it must be confined.
    #[must_use]
    pub const fn sandbox(&self) -> &SandboxRequirement {
        &self.sandbox
    }

    /// Every trigger that fired, in the validator's declared order.
    ///
    /// Never empty: a step exists because at least one trigger fired.
    #[must_use]
    pub fn reasons(&self) -> &[ValidationTrigger] {
        &self.reasons
    }
}

impl Absorb for PlannedValidation {
    fn absorb<H: DigestHasher>(&self, writer: &mut DigestWriter<H>) {
        writer.text(self.validator.as_str());
        self.command.absorb(writer);
        writer.text(self.sandbox.as_str());
        writer.sequence(&self.reasons, |writer, reason| reason.absorb(writer));
    }
}

/// What validation a change proposes, in a fixed order, **for one immutable review snapshot**.
///
/// The snapshot is part of the plan rather than an aside. A plan says what to run, and the steps
/// alone cannot say what it was to be run *against*: `cargo nextest run` is the same command and
/// the same digest whatever tree it is pointed at. Without the snapshot here,
/// [`RunLedger::clears`](crate::RunLedger::clears) has no value to check a record's snapshot
/// against, and evidence from a review that passed satisfies a review nothing ever ran.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationPlan {
    snapshot: Digest32,
    steps: Vec<PlannedValidation>,
}

impl ValidationPlan {
    /// A plan that proposes nothing, for the review named by `snapshot`.
    ///
    /// There is deliberately no snapshot-less constructor and no `Default`: a plan that does not
    /// know which review it belongs to is the value the replay attack needs.
    #[must_use]
    pub fn nothing_for(snapshot: Digest32) -> Self {
        Self {
            snapshot,
            steps: Vec::new(),
        }
    }

    /// The immutable review snapshot this plan was made for.
    #[must_use]
    pub const fn snapshot(&self) -> Digest32 {
        self.snapshot
    }

    /// The steps, in validator-identifier order.
    #[must_use]
    pub fn steps(&self) -> &[PlannedValidation] {
        &self.steps
    }

    /// The step for `validator`, when the change selected it.
    #[must_use]
    pub fn step(&self, validator: &ValidatorId) -> Option<&PlannedValidation> {
        self.steps.iter().find(|step| step.validator() == validator)
    }

    /// How many validators the change selected.
    #[must_use]
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Whether the change selected nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// The steps that cannot be confined, and so are never started automatically.
    pub fn needing_host_access(&self) -> impl Iterator<Item = &PlannedValidation> {
        self.steps.iter().filter(|step| !step.sandbox.is_isolated())
    }

    /// The plan's identity. Equal plans have equal digests, and unequal plans do not.
    ///
    /// The snapshot is absorbed first, so two reviews that select the same validators still have
    /// two identities. A digest that ignored it would let a plan for one review stand in for a plan
    /// for another wherever plans are compared.
    #[must_use]
    pub fn digest(&self) -> Digest32 {
        let mut writer = DigestWriter::new(PLAN_DOMAIN, Blake3::hasher());
        writer.digest(&self.snapshot);
        writer.sequence(&self.steps, |writer, step| step.absorb(writer));
        writer.finish()
    }
}

/// Choose the validators a change selects.
///
/// A validator is selected when **any** of its triggers fires. The reasons recorded are every
/// trigger that fired, in the order the validator declared them, so the explanation shown to the
/// user is stable rather than dependent on which one was tested first.
#[must_use]
pub fn plan_validation(
    registry: &ValidatorRegistry,
    change: &ReviewChange,
    tooling: &ToolingInventory,
) -> ValidationPlan {
    let steps = registry
        .iter()
        .filter_map(|spec| {
            let reasons: Vec<ValidationTrigger> = spec
                .triggers()
                .iter()
                .filter(|trigger| trigger.fires(change, tooling))
                .cloned()
                .collect();
            if reasons.is_empty() {
                return None;
            }
            Some(PlannedValidation {
                validator: spec.id().clone(),
                command: spec.command().clone(),
                sandbox: spec.sandbox().clone(),
                reasons,
            })
        })
        .collect();
    ValidationPlan {
        snapshot: change.snapshot(),
        steps,
    }
}

#[cfg(test)]
mod tests {
    use super::{plan_validation, ValidationPlan};
    use crate::change::{ChangeOperation, ChangedPath, PathEdit, ReviewChange};
    use crate::command::ValidationCommand;
    use crate::registry::{SandboxRequirement, ValidatorId, ValidatorRegistry, ValidatorSpec};
    use crate::tooling::{DetectedTool, ToolingInventory};
    use crate::trigger::ValidationTrigger;
    use mesh_types::Digest32;

    fn spec(name: &str, triggers: Vec<ValidationTrigger>) -> ValidatorSpec {
        ValidatorSpec::new(
            ValidatorId::parse(name).expect("legal"),
            ValidationCommand::at_root(name, []).expect("legal"),
            triggers,
            SandboxRequirement::Isolated,
        )
        .expect("at least one trigger")
    }

    fn rust_change() -> ReviewChange {
        ReviewChange::against(Digest32::from_bytes([2; 32]))
            .with_path(ChangedPath::new("src/lib.rs", PathEdit::Modified, 40).expect("legal"))
            .expect("no duplicate")
            .with_operation(ChangeOperation::EditContent)
    }

    #[test]
    fn a_validator_is_selected_when_any_trigger_fires_and_carries_every_one_that_did() {
        let registry = ValidatorRegistry::empty()
            .with(spec(
                "rust",
                vec![
                    ValidationTrigger::PathSuffix(".rs".to_owned()),
                    ValidationTrigger::Tool(DetectedTool::Cargo),
                    ValidationTrigger::Tool(DetectedTool::Go),
                ],
            ))
            .expect("registered");
        let plan = plan_validation(
            &registry,
            &rust_change(),
            &ToolingInventory::detect(["Cargo.toml"]),
        );
        assert_eq!(plan.len(), 1);
        assert_eq!(plan.steps()[0].reasons().len(), 2);
        assert_eq!(
            plan.steps()[0].reasons()[0],
            ValidationTrigger::PathSuffix(".rs".to_owned()),
            "reasons follow the validator's declared order"
        );
    }

    #[test]
    fn a_validator_no_trigger_fires_is_absent_from_the_plan() {
        let registry = ValidatorRegistry::empty()
            .with(spec(
                "python",
                vec![ValidationTrigger::PathSuffix(".py".to_owned())],
            ))
            .expect("registered");
        let plan = plan_validation(&registry, &rust_change(), &ToolingInventory::empty());
        assert!(plan.is_empty());
        assert_eq!(
            plan.digest(),
            ValidationPlan::nothing_for(rust_change().snapshot()).digest()
        );
    }

    #[test]
    fn two_reviews_that_select_the_same_validators_are_still_two_plans() {
        let registry = ValidatorRegistry::empty()
            .with(spec("rust", vec![ValidationTrigger::Always]))
            .expect("registered");
        let here = plan_validation(&registry, &rust_change(), &ToolingInventory::empty());
        let elsewhere = plan_validation(
            &registry,
            &ReviewChange::against(Digest32::from_bytes([3; 32]))
                .with_path(ChangedPath::new("src/lib.rs", PathEdit::Modified, 40).expect("legal"))
                .expect("no duplicate")
                .with_operation(ChangeOperation::EditContent),
            &ToolingInventory::empty(),
        );

        assert_eq!(here.steps(), elsewhere.steps(), "the same step is selected");
        assert_ne!(here.snapshot(), elsewhere.snapshot());
        assert_ne!(
            here.digest(),
            elsewhere.digest(),
            "two reviews shared one plan identity"
        );
        assert_eq!(
            ValidationPlan::nothing_for(here.snapshot()).snapshot(),
            here.snapshot()
        );
    }

    #[test]
    fn the_plan_is_in_identifier_order_however_the_registry_was_built() {
        let triggers = vec![ValidationTrigger::Always];
        let forwards = ValidatorRegistry::empty()
            .with(spec("a", triggers.clone()))
            .expect("first")
            .with(spec("z", triggers.clone()))
            .expect("second");
        let backwards = ValidatorRegistry::empty()
            .with(spec("z", triggers.clone()))
            .expect("first")
            .with(spec("a", triggers))
            .expect("second");
        let one = plan_validation(&forwards, &rust_change(), &ToolingInventory::empty());
        let two = plan_validation(&backwards, &rust_change(), &ToolingInventory::empty());
        assert_eq!(one, two);
        assert_eq!(one.digest(), two.digest());
        assert_eq!(one.steps()[0].validator().as_str(), "a");
    }

    #[test]
    fn a_different_change_produces_a_different_digest() {
        let registry = ValidatorRegistry::empty()
            .with(spec(
                "rust",
                vec![ValidationTrigger::PathSuffix(".rs".to_owned())],
            ))
            .expect("registered")
            .with(spec("everything", vec![ValidationTrigger::Always]))
            .expect("registered");
        let with_rust = plan_validation(&registry, &rust_change(), &ToolingInventory::empty());
        let without_rust = plan_validation(
            &registry,
            &ReviewChange::against(Digest32::from_bytes([2; 32])),
            &ToolingInventory::empty(),
        );
        assert_ne!(with_rust.digest(), without_rust.digest());
        assert_eq!(with_rust.len(), 2);
        assert_eq!(without_rust.len(), 1);
    }

    #[test]
    fn a_step_can_be_looked_up_by_validator() {
        let registry = ValidatorRegistry::standard();
        let plan = plan_validation(
            &registry,
            &rust_change(),
            &ToolingInventory::detect(["Cargo.toml"]),
        );
        let id = ValidatorId::parse("cargo-test").expect("legal");
        assert!(plan.step(&id).is_some());
        assert!(plan
            .step(&ValidatorId::parse("go-test").expect("legal"))
            .is_none());
    }

    #[test]
    fn the_steps_needing_host_access_are_reported_separately() {
        let plan = plan_validation(
            &ValidatorRegistry::standard(),
            &rust_change(),
            &ToolingInventory::detect(["Cargo.toml", "Makefile"]),
        );
        let deferred: Vec<&str> = plan
            .needing_host_access()
            .map(|step| step.validator().as_str())
            .collect();
        assert_eq!(deferred, vec!["make-test"]);
        assert_eq!(plan.len(), 2);
    }
}
