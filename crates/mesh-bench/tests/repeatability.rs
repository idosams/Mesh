//! Two runs on the same commit and machine land inside the stated band.
//!
//! Two layers, because "repeatable" means two different things:
//!
//! * **Deterministic** — identical inputs through the harness produce a
//!   byte-identical row. Checked with a scripted clock, so a busy machine
//!   cannot make this test lie either way.
//! * **Inside the band** — a real, jittery machine still lands within
//!   [`VarianceBand::STATED`]. Checked against modelled jitter here and against
//!   the real clock in `cargo bench --bench smoke`, which is where a genuinely
//!   noisy host is allowed to fail the build.

use mesh_bench::harness::{Harness, RunConfig};
use mesh_bench::schema::{BenchmarkResult, CacheState};
use mesh_bench::testing::{FixedWallClock, ScriptedClock, ScriptedWorkload, StaticProbe};
use mesh_bench::variance::{compare, VarianceBand};

const CAPTURE_MILLIS: u64 = 1_775_000_000_000;

fn run_with(durations_ns: &[u64], cache_state: CacheState) -> BenchmarkResult {
    let harness = Harness::new(
        ScriptedClock::from_durations(durations_ns.iter().copied()),
        FixedWallClock::new(CAPTURE_MILLIS),
        StaticProbe::new(),
    );
    let mut workload = ScriptedWorkload::new(durations_ns.iter().copied());
    let config = RunConfig::new(
        "mesh-bench/reference/blob-scan",
        "cargo nextest run -p mesh-bench",
    )
    .with_cache_state(cache_state)
    .with_iterations(durations_ns.len() as u64)
    .with_warmup_iterations(0);

    harness
        .run(&mut workload, &config)
        .expect("the run completes")
        .result()
        .expect("the workload verified")
        .clone()
}

/// A deterministic model of a slightly noisy machine: a fixed base with a
/// repeating jitter pattern and one slow tail sample.
fn modelled_durations(base_ns: u64, jitter_permille: u64, count: u64) -> Vec<u64> {
    (0..count)
        .map(|index| {
            let jitter = base_ns * jitter_permille * (index % 7) / 7_000;
            let tail = if index % 50 == 49 { base_ns / 4 } else { 0 };
            base_ns + jitter + tail
        })
        .collect()
}

#[test]
fn identical_inputs_produce_a_byte_identical_row() {
    let durations = modelled_durations(1_000, 20, 100);
    let first = run_with(&durations, CacheState::Warm);
    let second = run_with(&durations, CacheState::Warm);

    assert_eq!(first, second);
    assert_eq!(first.to_json_line(), second.to_json_line());
    assert_eq!(first.samples_ns, durations);
}

#[test]
fn two_runs_on_the_same_commit_stay_inside_the_stated_band() {
    // The second run is the same machine a few minutes later: same base cost,
    // a different phase of the same jitter, one extra tail event.
    let first = run_with(&modelled_durations(1_000, 20, 200), CacheState::Warm);
    let second = run_with(&modelled_durations(1_010, 25, 200), CacheState::Warm);

    let report = compare(&first, &second, VarianceBand::STATED).expect("comparable runs");
    assert!(report.within_band(), "{report}");
}

#[test]
fn the_band_is_stated_up_front_and_not_derived_from_the_numbers() {
    assert_eq!(VarianceBand::STATED.p50_permille, 50);
    assert_eq!(VarianceBand::STATED.p95_permille, 100);
    assert_eq!(VarianceBand::STATED.p99_permille, 150);
    assert_eq!(VarianceBand::default(), VarianceBand::STATED);
}

#[test]
fn a_real_regression_still_breaches_the_band() {
    // Repeatability must not be so loose that it hides a 30 % regression.
    let first = run_with(&modelled_durations(1_000, 20, 100), CacheState::Warm);
    let second = run_with(&modelled_durations(1_300, 20, 100), CacheState::Warm);

    let report = compare(&first, &second, VarianceBand::STATED).expect("comparable runs");
    assert!(!report.within_band(), "{report}");
    assert_eq!(report.breaches().len(), 3);
}

#[test]
fn cold_and_warm_runs_are_never_compared_to_each_other() {
    let warm = run_with(&modelled_durations(1_000, 20, 50), CacheState::Warm);
    let cold = run_with(&modelled_durations(1_000, 20, 50), CacheState::Cold);

    let error = compare(&warm, &cold, VarianceBand::STATED)
        .expect_err("a cold run and a warm run measure different things");
    assert_eq!(error.field, "cache_state");
}

#[test]
fn the_percentiles_in_the_row_are_the_ones_a_reader_recomputes() {
    let durations = modelled_durations(1_000, 20, 100);
    let row = run_with(&durations, CacheState::Warm);

    let mut sorted = row.samples_ns.clone();
    sorted.sort_unstable();
    assert_eq!(row.latency.p50_ns, sorted[49]);
    assert_eq!(row.latency.p95_ns, sorted[94]);
    assert_eq!(row.latency.p99_ns, sorted[98]);
    assert_eq!(row.latency.min_ns, sorted[0]);
    assert_eq!(row.latency.max_ns, sorted[99]);
}

#[test]
fn the_harness_reads_the_clock_exactly_twice_per_iteration() {
    let clock = ScriptedClock::from_durations([10, 20, 30]);
    let harness = Harness::new(
        clock.clone(),
        FixedWallClock::new(CAPTURE_MILLIS),
        StaticProbe::new(),
    );
    let mut workload = ScriptedWorkload::new([10, 20, 30]);
    harness
        .run(
            &mut workload,
            &RunConfig::new("b", "cmd")
                .with_iterations(3)
                .with_warmup_iterations(2),
        )
        .expect("the run completes");

    assert_eq!(clock.read_count(), 6, "warmup iterations must not be timed");
}
