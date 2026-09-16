//! Invariants checked after every delivery in a generated schedule.

use mesh_state::{IdentityChange, IdentityOutcome, ObjectId};

use crate::schedule::{initial_register, Schedule, StateMutant};

/// A property every generated core-model history must preserve.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Invariant {
    /// Once minted, an object identity remains known after every operation about it.
    IdentityRetained,
    /// A content-version write becomes the object's visible current version.
    WrittenVersionVisible,
    /// Link, unlink, rename and move produce the absolute placement they carry.
    ResultingPlacementVisible,
    /// Binding a name already claimed keeps every previous claimant and the new claimant.
    SameNameContendersPreserved,
    /// Reapplying the exact same delivery is reported as already applied.
    DuplicateIsIdempotent,
}

impl Invariant {
    /// Every standing invariant, in report order.
    pub const ALL: [Self; 5] = [
        Self::IdentityRetained,
        Self::WrittenVersionVisible,
        Self::ResultingPlacementVisible,
        Self::SameNameContendersPreserved,
        Self::DuplicateIsIdempotent,
    ];

    /// Stable word used in machine-readable campaign reports.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::IdentityRetained => "identity-retained",
            Self::WrittenVersionVisible => "written-version-visible",
            Self::ResultingPlacementVisible => "resulting-placement-visible",
            Self::SameNameContendersPreserved => "same-name-contenders-preserved",
            Self::DuplicateIsIdempotent => "duplicate-is-idempotent",
        }
    }
}

/// One invariant broken by one delivered change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InvariantViolation {
    delivery: usize,
    invariant: Invariant,
}

impl InvariantViolation {
    /// Delivery position that first exposed the violation.
    #[must_use]
    pub const fn delivery(self) -> usize {
        self.delivery
    }

    /// Property that was false after that delivery.
    #[must_use]
    pub const fn invariant(self) -> Invariant {
        self.invariant
    }
}

/// Deterministically ordered invariant findings for one schedule execution.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InvariantReport {
    violations: Vec<InvariantViolation>,
}

impl InvariantReport {
    /// Whether every checked property held.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.violations.is_empty()
    }

    /// Findings in delivery order, then invariant declaration order.
    #[must_use]
    pub fn violations(&self) -> &[InvariantViolation] {
        &self.violations
    }
}

pub(crate) fn audit(schedule: &Schedule, mutant: Option<StateMutant>) -> InvariantReport {
    let mut register = initial_register();
    let mut violations = Vec::new();
    for scheduled in schedule.changes() {
        let before = register.clone();
        let (next, outcome) = if mutant.is_some_and(|mutant| mutant.drops(scheduled.change())) {
            (register, None)
        } else {
            let (next, outcome) = register.apply(scheduled.change(), scheduled.stamp());
            (next, Some(outcome))
        };
        register = next;

        let delivery = scheduled.delivery();
        let object = scheduled.change().object();
        if !register.contains(object) {
            violations.push(violation(delivery, Invariant::IdentityRetained));
        }

        match scheduled.change() {
            IdentityChange::WriteVersion { version, .. } => {
                if register.version_of(object) != Some(*version) {
                    violations.push(violation(delivery, Invariant::WrittenVersionVisible));
                }
            }
            IdentityChange::Link {
                directory, name, ..
            } => {
                check_placement(&register, scheduled.change(), delivery, &mut violations);
                let previous = before.contenders_for(*directory, name);
                if !previous.is_empty() {
                    let current = register.contenders_for(*directory, name);
                    if !contains_every(&current, &previous) || !current.contains(&object) {
                        violations
                            .push(violation(delivery, Invariant::SameNameContendersPreserved));
                    }
                }
            }
            IdentityChange::Unlink { .. }
            | IdentityChange::Rename { .. }
            | IdentityChange::Move { .. } => {
                check_placement(&register, scheduled.change(), delivery, &mut violations);
            }
            IdentityChange::Create { .. } => {}
        }

        if outcome == Some(IdentityOutcome::AlreadyApplied)
            && mutant == Some(StateMutant::TreatDuplicateAsApplied)
        {
            violations.push(violation(delivery, Invariant::DuplicateIsIdempotent));
        }
    }
    violations.sort_unstable();
    violations.dedup();
    InvariantReport { violations }
}

fn check_placement(
    register: &mesh_state::ObjectRegister,
    change: &IdentityChange,
    delivery: usize,
    violations: &mut Vec<InvariantViolation>,
) {
    let expected = change
        .resulting_placement()
        .expect("only placement operations call this helper");
    if register.placement_of(change.object()) != Some(&expected) {
        violations.push(violation(delivery, Invariant::ResultingPlacementVisible));
    }
}

fn contains_every(current: &[ObjectId], expected: &[ObjectId]) -> bool {
    expected.iter().all(|object| current.contains(object))
}

const fn violation(delivery: usize, invariant: Invariant) -> InvariantViolation {
    InvariantViolation {
        delivery,
        invariant,
    }
}
