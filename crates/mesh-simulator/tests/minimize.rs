//! One-minimal failure reduction and planted-defect coverage.

use mesh_simulator::{
    minimize_failure, FailureRecord, ReproductionError, Schedule, Seed, SimulationConfig,
    StateMutant, MIN_SMOKE_STEPS,
};

fn schedule(seed: u64) -> Schedule {
    Schedule::generate(
        Seed::new(seed),
        SimulationConfig::new(3, MIN_SMOKE_STEPS, 96).expect("valid smoke config"),
    )
}

fn detects(schedule: &Schedule, mutant: StateMutant) -> bool {
    schedule.run() != schedule.run_mutated(mutant)
}

#[test]
fn every_planted_defect_is_caught_and_reduced_to_one_minimal_input() {
    let original = schedule(0x5eed);

    for mutant in StateMutant::ALL {
        let record = minimize_failure(&original, |candidate| detects(candidate, mutant))
            .unwrap_or_else(|| panic!("{mutant:?} was not caught"));
        let reproduction = record
            .reproduction(&original)
            .expect("the recorded source and indexes replay");
        assert!(detects(&reproduction, mutant), "{mutant:?}");

        for removed in 0..record.minimal_reproduction().len() {
            let mut candidate = record.minimal_reproduction().to_vec();
            candidate.remove(removed);
            let smaller = original
                .reproduction(&candidate)
                .expect("a subset of canonical indexes is canonical");
            assert!(
                !detects(&smaller, mutant),
                "{mutant:?} was not one-minimal: {:?}",
                record.minimal_reproduction()
            );
        }
    }
}

#[test]
fn the_minimizer_is_byte_deterministic() {
    let original = schedule(42);
    let first = minimize_failure(&original, |candidate| {
        detects(candidate, StateMutant::TreatDuplicateAsApplied)
    })
    .expect("the duplicate mutant is caught");
    let second = minimize_failure(&original, |candidate| {
        detects(candidate, StateMutant::TreatDuplicateAsApplied)
    })
    .expect("the duplicate mutant is caught twice");

    assert_eq!(first, second);
    assert_eq!(first.minimal_reproduction(), [1, 6]);
    assert_eq!(first.schedule(), original.canonical_bytes());
}

#[test]
fn a_nonfailing_predicate_produces_no_failure_record() {
    let original = schedule(9);
    assert_eq!(minimize_failure(&original, |_| false), None);
}

#[test]
fn replay_refuses_the_wrong_source_and_noncanonical_indexes() {
    let original = schedule(1);
    let other = schedule(2);
    let record = minimize_failure(&original, |candidate| {
        detects(candidate, StateMutant::DropRename)
    })
    .expect("the rename mutant is caught");
    assert_eq!(
        record.reproduction(&other),
        Err(ReproductionError::SourceMismatch)
    );
    assert_eq!(
        original.reproduction(&[6, 1]),
        Err(ReproductionError::NonCanonicalIndexes)
    );
    assert_eq!(
        original.reproduction(&[MIN_SMOKE_STEPS]),
        Err(ReproductionError::DeliveryOutOfRange)
    );

    let noncanonical = FailureRecord::capture(&original, vec![6, 1]);
    assert_eq!(
        noncanonical.reproduction(&original),
        Err(ReproductionError::NonCanonicalIndexes)
    );
}
