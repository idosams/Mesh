//! Standing reproduction of ADR-0015's actor-head convergence evidence.

mod support;

use std::time::{Duration, Instant};

use support::r1::{
    divergence_heads, exhaustive_history7, randomized_case, DIVERGENCE_CHILD_HEAD,
    DIVERGENCE_TWIN_HEAD, EXHAUSTIVE_HEAD,
};

#[test]
fn identifier_parent_binding_divergence_seed_keeps_both_recorded_heads() {
    let (child_head, twin_head) = divergence_heads();
    assert_eq!(child_head.to_hex(), DIVERGENCE_CHILD_HEAD);
    assert_eq!(twin_head.to_hex(), DIVERGENCE_TWIN_HEAD);
    assert_ne!(child_head, twin_head);
}

#[test]
fn exhaustive_and_gate_sized_randomized_campaign_stay_under_thirty_seconds() {
    let started = Instant::now();
    let counts = exhaustive_history7().expect("ADR-0015 exhaustive n=7 reproduction");
    assert_eq!(counts.permutations, 5_040);
    assert_eq!(counts.duplicate_streams, 282_240);

    for seed in 0..64 {
        let outcome = randomized_case(seed, 30, 5, 8)
            .unwrap_or_else(|error| panic!("randomized R1 seed {seed} failed: {error}"));
        assert_eq!(outcome.applied.len(), 30, "seed {seed}");
    }

    let elapsed = started.elapsed();
    eprintln!(
        "R1: {} permutations + {} duplicate streams + 64 randomized seeds; head {EXHAUSTIVE_HEAD}; {elapsed:?}",
        counts.permutations, counts.duplicate_streams
    );
    assert!(
        elapsed < Duration::from_secs(30),
        "R1 merge-path campaign exceeded its 30 s budget: {elapsed:?}"
    );
}
