//! Correctness verification runs before timing, and a failed verification
//! produces no timing number.
//!
//! Three independent proofs, because one is easy to defeat by accident:
//!
//! 1. the clock is never read when verification fails (counted, not inferred);
//! 2. the workload is never executed when verification fails;
//! 3. even a hand-assembled row claiming an unverified run is refused, so the
//!    guarantee survives someone bypassing the harness entirely.

use mesh_bench::harness::{Harness, RunConfig, RunOutcome};
use mesh_bench::json::Json;
use mesh_bench::schema::{BenchmarkResult, SchemaError};
use mesh_bench::sink::{MemoryWriter, SinkError, SinkPolicy, ValidatingSink};
use mesh_bench::testing::{
    sample_result, FixedWallClock, ScriptedClock, ScriptedWorkload, StaticProbe,
};
use mesh_bench::workload::Workload;
use mesh_bench::workloads::blob_scan::{BlobScan, BlobScanParameters};

fn harness(clock: ScriptedClock) -> Harness<ScriptedClock, FixedWallClock, StaticProbe> {
    Harness::new(
        clock,
        FixedWallClock::new(1_775_000_000_000),
        StaticProbe::new(),
    )
}

#[test]
fn a_failed_verification_reads_no_clock_and_runs_no_iteration() {
    let clock = ScriptedClock::from_durations([10, 20, 30, 40]);
    let mut workload = ScriptedWorkload::new([10, 20, 30, 40]).failing_verification();

    let outcome = harness(clock.clone())
        .run(
            &mut workload,
            &RunConfig::new("mesh-bench/reference", "cmd").with_iterations(4),
        )
        .expect("a failed verification is a reported outcome, not a crash");

    assert!(!outcome.is_measured());
    assert_eq!(outcome.result(), None, "there is no row to publish");
    assert_eq!(clock.read_count(), 0, "the clock was read");
    assert_eq!(workload.iterate_calls(), 0, "the workload was executed");
    assert_eq!(workload.warm_preparations(), 0);
    assert_eq!(workload.cold_preparations(), 0);
}

#[test]
fn the_failure_says_which_digests_disagreed() {
    let mut workload = ScriptedWorkload::new([10]).failing_verification();
    let outcome = harness(ScriptedClock::new([0, 10]))
        .run(
            &mut workload,
            &RunConfig::new("mesh-bench/reference", "cmd").with_iterations(1),
        )
        .expect("outcome");

    let RunOutcome::VerificationFailed(failure) = outcome else {
        panic!("a corrupt workload must not be measured");
    };
    assert_eq!(failure.benchmark_id, "mesh-bench/reference");
    assert_ne!(
        failure.verification.expected_digest,
        failure.verification.observed_digest
    );
    assert!(failure.to_string().contains("no timing was taken"));
}

#[test]
fn a_verified_run_carries_the_digests_it_was_verified_with() {
    let mut workload = ScriptedWorkload::new([10, 20]);
    let outcome = harness(ScriptedClock::from_durations([10, 20]))
        .run(
            &mut workload,
            &RunConfig::new("mesh-bench/reference", "cmd")
                .with_iterations(2)
                .with_warmup_iterations(0),
        )
        .expect("outcome");

    let result = outcome.result().expect("measured");
    assert!(result.verification.passed());
    assert_eq!(
        result.to_json().get_path("verification.verified"),
        Some(&Json::Bool(true))
    );
}

#[test]
fn the_reference_workload_gates_on_its_own_digest() {
    let parameters = BlobScanParameters {
        blob_count: 4,
        blob_bytes: 256,
    };
    assert!(BlobScan::new(9, parameters)
        .verify()
        .expect("verification runs")
        .passed());
    assert!(!BlobScan::new(9, parameters)
        .corrupted()
        .verify()
        .expect("verification runs")
        .passed());
}

#[test]
fn a_corrupt_workload_never_reaches_the_sink() {
    let parameters = BlobScanParameters {
        blob_count: 2,
        blob_bytes: 64,
    };
    let mut workload = BlobScan::new(3, parameters).corrupted();
    let outcome = harness(ScriptedClock::from_durations([10; 8]))
        .run(
            &mut workload,
            &RunConfig::new("mesh-bench/reference/blob-scan", "cmd")
                .with_iterations(8)
                .with_warmup_iterations(0),
        )
        .expect("outcome");

    let mut sink = ValidatingSink::new(MemoryWriter::new(), SinkPolicy::EXPLORATORY);
    if let Some(result) = outcome.result() {
        let _ = sink.accept(result);
    }
    assert!(
        sink.writer().lines().is_empty(),
        "a corrupt workload produced a publishable row"
    );
}

#[test]
fn a_row_claiming_an_unverified_run_is_refused_even_without_the_harness() {
    let row = sample_result();
    let verification = row
        .to_json()
        .get_path("verification")
        .and_then(Json::as_object)
        .cloned()
        .expect("section")
        .with("observed_digest", Json::string("fnv1a64:ffffffffffffffff"))
        .with("verified", Json::Bool(false));
    let forged = Json::Object(
        row.to_json()
            .as_object()
            .cloned()
            .expect("object")
            .with("verification", Json::Object(verification)),
    );

    let error = BenchmarkResult::from_json(&forged).expect_err("no timing without verification");
    assert!(matches!(error, SchemaError::UnverifiedRun { .. }));

    let mut sink = ValidatingSink::new(MemoryWriter::new(), SinkPolicy::EXPLORATORY);
    assert!(matches!(
        sink.accept_json(&forged),
        Err(SinkError::Schema(SchemaError::UnverifiedRun { .. }))
    ));
    assert!(sink.writer().lines().is_empty());
}
