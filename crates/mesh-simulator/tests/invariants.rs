//! Generated family coverage and executable core-model invariants.

use std::collections::BTreeSet;

use mesh_simulator::{
    minimize_failure, Invariant, OperationFamily, Schedule, Seed, SimulationConfig, StateMutant,
    MIN_SMOKE_STEPS,
};

fn schedule(seed: u64, steps: usize) -> Schedule {
    Schedule::generate(
        Seed::new(seed),
        SimulationConfig::new(4, steps, 128).expect("valid generated campaign"),
    )
}

#[test]
fn the_generated_tail_covers_every_identity_operation_family() {
    let schedule = schedule(7, MIN_SMOKE_STEPS + OperationFamily::ALL.len());
    let covered: BTreeSet<_> = schedule
        .changes()
        .iter()
        .skip(MIN_SMOKE_STEPS)
        .map(|change| change.family())
        .collect();
    assert_eq!(covered, BTreeSet::from(OperationFamily::ALL));
}

#[test]
fn generated_histories_hold_every_invariant() {
    for seed in 0..64 {
        let schedule = schedule(seed, 128);
        let report = schedule.audit();
        assert!(report.is_clean(), "seed {seed}: {:?}", report.violations());
    }
}

#[test]
fn every_planted_failure_is_detected_and_minimized_by_invariants() {
    let original = schedule(0x5eed, 64);
    for mutant in StateMutant::ALL {
        let record = minimize_failure(&original, |candidate| {
            !candidate.audit_mutated(mutant).is_clean()
        })
        .unwrap_or_else(|| panic!("{mutant:?} escaped every invariant"));
        let reproduction = record
            .reproduction(&original)
            .expect("the invariant failure replays");
        assert!(!reproduction.audit_mutated(mutant).is_clean());
        for removed in 0..record.minimal_reproduction().len() {
            let mut indexes = record.minimal_reproduction().to_vec();
            indexes.remove(removed);
            let smaller = original
                .reproduction(&indexes)
                .expect("a smaller canonical reproduction");
            assert!(
                smaller.audit_mutated(mutant).is_clean(),
                "{mutant:?} was not one-minimal: {:?}",
                record.minimal_reproduction()
            );
        }
    }
}

#[test]
fn each_mutant_routes_to_the_invariant_that_names_its_damage() {
    let schedule = schedule(11, MIN_SMOKE_STEPS);
    let cases = [
        (
            StateMutant::DropRename,
            Invariant::ResultingPlacementVisible,
        ),
        (
            StateMutant::DropWriteVersion,
            Invariant::WrittenVersionVisible,
        ),
        (
            StateMutant::TreatDuplicateAsApplied,
            Invariant::DuplicateIsIdempotent,
        ),
    ];
    for (mutant, expected) in cases {
        let report = schedule.audit_mutated(mutant);
        assert!(
            report
                .violations()
                .iter()
                .any(|violation| violation.invariant() == expected),
            "{mutant:?}: {:?}",
            report.violations()
        );
    }
}
