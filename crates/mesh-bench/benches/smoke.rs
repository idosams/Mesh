//! The smoke bench: proves the instrument works before anyone measures with it.
//!
//! Two modes, both run from `cargo bench`:
//!
//! ```text
//! cargo bench -p mesh-bench --bench smoke -- --verify-schema
//! cargo bench -p mesh-bench --bench smoke
//! ```
//!
//! `--verify-schema` runs the required-field contract against this build and
//! exits non-zero if any field stopped being required — it needs no repository,
//! no host probe and no timing, so it is safe anywhere. Without the flag the
//! bench takes two short measured runs of the reference workload through the
//! real harness and holds them to the stated variance band, which needs a
//! checkout to read the commit from.
//!
//! `harness = false`: the bench owns its `main`, so the flag reaches this code
//! rather than libtest, and no benchmark framework sits between the clock and
//! the workload.

use mesh_bench::clock::{Clock, MonotonicClock, SystemWallClock, WallClock};
use mesh_bench::env::{EnvironmentProbe, SystemProbe};
use mesh_bench::harness::{Harness, RunConfig, RunOutcome};
use mesh_bench::json::JsonObject;
use mesh_bench::schema::{BenchmarkResult, CacheState};
use mesh_bench::selfcheck;
use mesh_bench::variance::{compare, VarianceBand};
use mesh_bench::workloads::WorkloadRegistry;

fn main() {
    // Cargo passes `--bench` to benchmark targets; anything else is ours.
    let arguments: Vec<String> = std::env::args()
        .skip(1)
        .filter(|argument| argument != "--bench")
        .collect();
    let verify_only = arguments
        .iter()
        .any(|argument| argument == "--verify-schema");

    let report = selfcheck::run();
    println!("{report}");
    if !report.passed() {
        eprintln!("smoke: this build no longer refuses incomplete rows");
        std::process::exit(1);
    }

    if verify_only {
        println!("smoke: schema contract verified, timing skipped (--verify-schema)");
        return;
    }

    if let Err(error) = measured_run() {
        eprintln!("smoke: {error}");
        std::process::exit(1);
    }
}

/// Two short warm runs of the reference workload, checked for repeatability.
fn measured_run() -> Result<(), String> {
    let registry = WorkloadRegistry::builtin();
    let probe = SystemProbe::here().map_err(|error| error.to_string())?;
    let harness = Harness::new(MonotonicClock::new(), SystemWallClock, probe);
    let config = RunConfig::new(
        "mesh-bench/reference/blob-scan",
        "cargo bench -p mesh-bench --bench smoke",
    )
    .with_cache_state(CacheState::Warm)
    .with_iterations(200)
    .with_warmup_iterations(20);

    let first = measure(&registry, &harness, &config)?;
    let second = measure(&registry, &harness, &config)?;

    println!("{}", first.to_json_pretty());
    let repeatability =
        compare(&first, &second, VarianceBand::STATED).map_err(|error| error.to_string())?;
    print!("{repeatability}");
    if !repeatability.within_band() {
        return Err(format!(
            "two runs of the same commit fell outside the stated band ({} breaches) — \
             fix the variance source before publishing anything from this harness",
            repeatability.breaches().len()
        ));
    }
    Ok(())
}

fn measure<C: Clock, W: WallClock, P: EnvironmentProbe>(
    registry: &WorkloadRegistry,
    harness: &Harness<C, W, P>,
    config: &RunConfig,
) -> Result<BenchmarkResult, String> {
    let mut workload = registry
        .build("blob-scan", 42, &JsonObject::new())
        .map_err(|error| error.to_string())?;
    match harness
        .run(workload.as_mut(), config)
        .map_err(|error| error.to_string())?
    {
        RunOutcome::Measured(result) => Ok(*result),
        RunOutcome::VerificationFailed(failure) => Err(failure.to_string()),
    }
}
