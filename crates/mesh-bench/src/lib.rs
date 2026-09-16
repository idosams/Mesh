//! The reproducible benchmark harness and its result schema.
//!
//! Every performance number Mesh publishes comes out of this crate, and the
//! crate is built around one rule: **a measurement that cannot be reproduced
//! from its own result row does not get to exist**. That rule shows up three
//! times, deliberately:
//!
//! 1. [`schema`] — a row must carry the commit, the machine, the OS and
//!    filesystem, the build profile, the data generator, the cache state, the
//!    sample count, the raw samples, p50/p95/p99, the failure count and the
//!    correctness verification. A row missing any of them is rejected by field
//!    name, at write time, by [`sink::ValidatingSink`].
//! 2. [`harness`] — correctness verification runs *before* the clock is read
//!    even once, and a failed verification returns
//!    [`harness::RunOutcome::VerificationFailed`], which carries no timing.
//! 3. [`variance`] — two runs of the same commit on the same machine are held
//!    to a stated band ([`variance::VarianceBand::STATED`]) rather than to
//!    whatever the numbers happened to do.
//!
//! # Shape
//!
//! ```text
//!   workload (trait)  --verify--> harness --samples--> schema --> sink --> .jsonl
//!        ^                          ^   ^                          ^
//!   workloads::registry     clock --'   '-- env::EnvironmentProbe  '-- SinkPolicy
//! ```
//!
//! Each arrow crosses a trait, so a benchmark can replace its workload, a test
//! can replace the clock and the probe, and a future results service can replace
//! the writer, without any of them reaching into the harness.
//!
//! # Running one
//!
//! ```no_run
//! use mesh_bench::clock::{MonotonicClock, SystemWallClock};
//! use mesh_bench::env::SystemProbe;
//! use mesh_bench::harness::{Harness, RunConfig};
//! use mesh_bench::json::JsonObject;
//! use mesh_bench::schema::CacheState;
//! use mesh_bench::sink::{FileWriter, SinkPolicy, ValidatingSink};
//! use mesh_bench::workloads::WorkloadRegistry;
//!
//! let mut workload = WorkloadRegistry::builtin().build("blob-scan", 42, &JsonObject::new())?;
//! let harness = Harness::new(MonotonicClock::new(), SystemWallClock, SystemProbe::here()?);
//! let config = RunConfig::new("mesh-bench/reference/blob-scan", "mesh-bench run …")
//!     .with_cache_state(CacheState::Cold)
//!     .with_iterations(100);
//!
//! if let Some(result) = harness.run(workload.as_mut(), &config)?.result() {
//!     // Outside the measured checkout on purpose: a results file written inside
//!     // it makes `git status --porcelain` non-empty, which sets
//!     // `repository.dirty`, which makes PUBLISHABLE refuse the *next* run.
//!     let mut sink = ValidatingSink::new(
//!         FileWriter::new(std::env::temp_dir().join("mesh-bench/blob-scan.jsonl")),
//!         SinkPolicy::PUBLISHABLE,
//!     );
//!     sink.accept(result)?;
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![forbid(unsafe_code)]

pub mod clock;
pub mod corpus;
pub mod env;
pub mod harness;
pub mod json;
pub mod schema;
pub mod selfcheck;
pub mod sink;
pub mod stats;
pub mod testing;
pub mod variance;
pub mod workload;
pub mod workloads;

/// The crate's name, as it appears in generator and benchmark identifiers.
pub const CRATE_NAME: &str = "mesh-bench";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_stable() {
        assert_eq!(CRATE_NAME, "mesh-bench");
    }
}
