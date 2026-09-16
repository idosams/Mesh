//! Determinism and failure-record coverage for the seeded core-model schedule.

use mesh_simulator::{
    FailureRecord, Schedule, Seed, SimulationConfig, SimulationConfigError, MIN_SMOKE_STEPS,
    SIMULATOR_PROTOCOL_VERSION,
};

fn config(steps: usize) -> SimulationConfig {
    SimulationConfig::new(3, steps, 96).expect("valid test configuration")
}

#[test]
fn one_seed_reproduces_identical_schedule_and_outcome() {
    let first = Schedule::generate(Seed::new(0x5eed), config(64));
    let second = Schedule::generate(Seed::new(0x5eed), config(64));

    assert_eq!(first, second);
    assert_eq!(first.canonical_bytes(), second.canonical_bytes());
    assert_eq!(
        first.run().canonical_bytes(),
        second.run().canonical_bytes()
    );
}

#[test]
fn a_different_seed_changes_the_generated_tail() {
    let first = Schedule::generate(Seed::new(1), config(64));
    let second = Schedule::generate(Seed::new(2), config(64));
    assert_ne!(first.canonical_bytes(), second.canonical_bytes());
}

#[test]
fn the_smoke_prefix_contains_overlap_duplicate_and_reordered_delivery() {
    let schedule = Schedule::generate(Seed::new(7), config(MIN_SMOKE_STEPS));
    let changes = schedule.changes();

    assert_eq!(changes[0].stamp().lamport(), changes[1].stamp().lamport());
    assert_eq!(changes[2].stamp().lamport(), changes[3].stamp().lamport());
    assert_eq!(changes[4].stamp().lamport(), changes[5].stamp().lamport());
    assert_eq!(changes[1].stamp(), changes[6].stamp());
    assert_eq!(changes[1].change(), changes[6].change());
    assert!(changes[7].stamp() < changes[5].stamp());

    let outcome = String::from_utf8(schedule.run().canonical_bytes()).expect("text result");
    assert!(outcome.contains("6:already-applied"), "{outcome}");
}

#[test]
fn the_failure_record_carries_all_five_replay_fields() {
    let schedule = Schedule::generate(Seed::new(42), config(32));
    let record = FailureRecord::capture(&schedule, vec![0, 1, 6]);

    assert_eq!(record.seed(), Seed::new(42));
    assert_eq!(record.schedule(), schedule.canonical_bytes());
    assert_eq!(record.protocol_version(), SIMULATOR_PROTOCOL_VERSION);
    assert_eq!(record.configuration(), config(32));
    assert_eq!(record.minimal_reproduction(), [0, 1, 6]);
}

#[test]
fn invalid_configuration_is_refused_before_generation() {
    assert_eq!(
        SimulationConfig::new(1, MIN_SMOKE_STEPS, 0),
        Err(SimulationConfigError::TooFewActors)
    );
    assert_eq!(
        SimulationConfig::new(2, MIN_SMOKE_STEPS - 1, 0),
        Err(SimulationConfigError::TooFewSteps)
    );
    assert_eq!(
        SimulationConfig::new(2, MIN_SMOKE_STEPS, 257),
        Err(SimulationConfigError::InvalidOverlap)
    );
}
