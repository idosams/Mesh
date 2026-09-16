//! The harness: verify, then measure, then emit — in that order, always.
//!
//! The ordering is the whole point. [`Harness::run`] calls
//! [`Workload::verify`](crate::workload::Workload::verify) before it reads the
//! clock even once. If verification does not pass, the run returns
//! [`RunOutcome::VerificationFailed`], which carries no duration, no percentile
//! and no sample array — there is no code path from a failed verification to a
//! timing number, and a test asserts the clock was never read.

use crate::clock::{Clock, WallClock};
use crate::env::{EnvironmentProbe, ProbeError};
use crate::schema::{
    BenchmarkResult, CacheState, LatencySummary, SchemaError, Verification, SCHEMA_VERSION,
};
use crate::workload::{Workload, WorkloadError};
use std::fmt;

/// How one run is configured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunConfig {
    /// Stable identifier of the benchmark.
    pub benchmark_id: String,
    /// The command line that produced the run, recorded verbatim.
    pub invocation: String,
    /// Cold or warm caches — there is no "as found" option.
    pub cache_state: CacheState,
    /// How many timed iterations to attempt.
    pub iterations: u64,
    /// Untimed iterations run first, to reach steady state.
    ///
    /// Under [`CacheState::Cold`] the caches are dropped again before each timed
    /// sample, so warmup buys stability of the *code path*, not of the cache.
    pub warmup_iterations: u64,
}

impl RunConfig {
    /// A configuration with the harness defaults: 100 timed iterations after 10
    /// untimed ones.
    pub fn new(benchmark_id: impl Into<String>, invocation: impl Into<String>) -> Self {
        RunConfig {
            benchmark_id: benchmark_id.into(),
            invocation: invocation.into(),
            cache_state: CacheState::Warm,
            iterations: 100,
            warmup_iterations: 10,
        }
    }

    /// Sets the cache state.
    #[must_use]
    pub fn with_cache_state(mut self, cache_state: CacheState) -> Self {
        self.cache_state = cache_state;
        self
    }

    /// Sets the number of timed iterations.
    #[must_use]
    pub fn with_iterations(mut self, iterations: u64) -> Self {
        self.iterations = iterations;
        self
    }

    /// Sets the number of untimed warmup iterations.
    #[must_use]
    pub fn with_warmup_iterations(mut self, warmup_iterations: u64) -> Self {
        self.warmup_iterations = warmup_iterations;
        self
    }
}

/// What a run produced.
#[derive(Clone, Debug, PartialEq)]
pub enum RunOutcome {
    /// The workload verified and was measured.
    Measured(Box<BenchmarkResult>),
    /// The workload did not compute the right answer; nothing was timed.
    VerificationFailed(VerificationFailure),
}

impl RunOutcome {
    /// The result row, if the run produced one.
    pub fn result(&self) -> Option<&BenchmarkResult> {
        match self {
            RunOutcome::Measured(result) => Some(result),
            RunOutcome::VerificationFailed(_) => None,
        }
    }

    /// Whether the run produced a publishable row.
    pub fn is_measured(&self) -> bool {
        matches!(self, RunOutcome::Measured(_))
    }
}

/// A run that stopped at the correctness gate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerificationFailure {
    /// The benchmark that failed to verify.
    pub benchmark_id: String,
    /// The digests that disagreed.
    pub verification: Verification,
}

impl fmt::Display for VerificationFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` failed correctness verification `{}` (expected `{}`, observed `{}`) — no timing was taken",
            self.benchmark_id,
            self.verification.method,
            self.verification.expected_digest,
            self.verification.observed_digest
        )
    }
}

/// A run could not be completed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HarnessError {
    /// The configuration asks for something unmeasurable.
    Config(String),
    /// The workload itself failed outside the timed section.
    Workload(WorkloadError),
    /// The environment could not be captured.
    Probe(ProbeError),
    /// Every attempted iteration failed, so there is nothing to summarise.
    AllIterationsFailed {
        /// How many iterations were attempted.
        attempted: u64,
        /// The last error the workload reported.
        last_error: WorkloadError,
    },
    /// The monotonic clock went backwards across a sample.
    ClockWentBackwards {
        /// The reading taken before the iteration.
        before_ns: u64,
        /// The reading taken after it.
        after_ns: u64,
    },
    /// The assembled row does not satisfy the schema — a harness bug, surfaced
    /// rather than written out.
    Schema(SchemaError),
}

impl fmt::Display for HarnessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HarnessError::Config(detail) => write!(f, "invalid run configuration: {detail}"),
            HarnessError::Workload(error) => write!(f, "{error}"),
            HarnessError::Probe(error) => write!(f, "{error}"),
            HarnessError::AllIterationsFailed {
                attempted,
                last_error,
            } => write!(
                f,
                "all {attempted} iterations failed, last: {last_error} — no percentile is reported for a run with no samples"
            ),
            HarnessError::ClockWentBackwards {
                before_ns,
                after_ns,
            } => write!(
                f,
                "monotonic clock went backwards during a sample ({before_ns} -> {after_ns})"
            ),
            HarnessError::Schema(error) => write!(f, "assembled row is invalid: {error}"),
        }
    }
}

impl std::error::Error for HarnessError {}

/// The measuring instrument.
///
/// Generic over its clock, its wall clock and its environment probe so a test
/// can drive it deterministically and a future CI or container probe can
/// replace the host probe without the harness noticing.
#[derive(Clone, Debug)]
pub struct Harness<C, W, P> {
    clock: C,
    wall_clock: W,
    probe: P,
}

impl<C: Clock, W: WallClock, P: EnvironmentProbe> Harness<C, W, P> {
    /// Assembles a harness from its three seams.
    pub fn new(clock: C, wall_clock: W, probe: P) -> Self {
        Harness {
            clock,
            wall_clock,
            probe,
        }
    }

    /// Runs one benchmark.
    ///
    /// Order of operations, which is also the order of the guarantees:
    /// 1. verify correctness — no clock reads yet;
    /// 2. bail out with [`RunOutcome::VerificationFailed`] if it did not pass;
    /// 3. capture the environment, failing the run if any field is unknowable;
    /// 4. warm up untimed;
    /// 5. take the samples;
    /// 6. assemble the row and validate it before returning it.
    pub fn run(
        &self,
        workload: &mut dyn Workload,
        config: &RunConfig,
    ) -> Result<RunOutcome, HarnessError> {
        if config.iterations == 0 {
            return Err(HarnessError::Config(
                "iterations must be at least 1".to_owned(),
            ));
        }

        let verification = workload.verify().map_err(HarnessError::Workload)?;
        if !verification.passed() {
            return Ok(RunOutcome::VerificationFailed(VerificationFailure {
                benchmark_id: config.benchmark_id.clone(),
                verification,
            }));
        }

        let environment = self.probe.probe().map_err(HarnessError::Probe)?;
        let descriptor = workload.descriptor();

        self.warm_up(workload, config)?;
        let samples = self.take_samples(workload, config)?;

        let latency = LatencySummary::from_samples(&samples.durations_ns).ok_or_else(|| {
            HarnessError::AllIterationsFailed {
                attempted: config.iterations,
                last_error: samples
                    .last_error
                    .clone()
                    .unwrap_or_else(|| WorkloadError::new("no samples were taken")),
            }
        })?;

        let result = BenchmarkResult {
            schema_version: SCHEMA_VERSION.to_owned(),
            benchmark_id: config.benchmark_id.clone(),
            invocation: config.invocation.clone(),
            recorded_at_unix_ms: self.wall_clock.unix_millis(),
            repository: environment.repository,
            hardware: environment.hardware,
            platform: environment.platform,
            build: environment.build,
            workload: descriptor,
            cache_state: config.cache_state,
            sample_count: samples.durations_ns.len() as u64,
            iterations_attempted: config.iterations,
            failure_count: samples.failure_count,
            samples_ns: samples.durations_ns,
            latency,
            verification,
            // After the last sample, so no directory walk is ever inside a
            // timed section.
            storage: workload.storage(),
        };
        result.validate().map_err(HarnessError::Schema)?;
        Ok(RunOutcome::Measured(Box::new(result)))
    }

    /// Untimed iterations. A failure here is a hard error: a workload that
    /// cannot complete a warmup iteration has nothing worth measuring.
    fn warm_up(&self, workload: &mut dyn Workload, config: &RunConfig) -> Result<(), HarnessError> {
        if config.cache_state == CacheState::Warm {
            workload
                .prepare(CacheState::Warm)
                .map_err(HarnessError::Workload)?;
        }
        for _ in 0..config.warmup_iterations {
            if config.cache_state == CacheState::Cold {
                workload
                    .prepare(CacheState::Cold)
                    .map_err(HarnessError::Workload)?;
            }
            workload.iterate().map_err(HarnessError::Workload)?;
        }
        Ok(())
    }

    fn take_samples(
        &self,
        workload: &mut dyn Workload,
        config: &RunConfig,
    ) -> Result<Samples, HarnessError> {
        let mut durations_ns = Vec::with_capacity(config.iterations as usize);
        let mut failure_count = 0_u64;
        let mut last_error = None;

        for _ in 0..config.iterations {
            if config.cache_state == CacheState::Cold {
                workload
                    .prepare(CacheState::Cold)
                    .map_err(HarnessError::Workload)?;
            }

            let before_ns = self.clock.now_nanos();
            let outcome = workload.iterate();
            let after_ns = self.clock.now_nanos();

            match outcome {
                Ok(()) => {
                    if after_ns < before_ns {
                        return Err(HarnessError::ClockWentBackwards {
                            before_ns,
                            after_ns,
                        });
                    }
                    durations_ns.push(after_ns - before_ns);
                }
                Err(error) => {
                    // A failed iteration is counted, never timed: its duration
                    // measures how long failing took, which is not the number
                    // anyone is asking about.
                    failure_count += 1;
                    last_error = Some(error);
                }
            }
        }

        Ok(Samples {
            durations_ns,
            failure_count,
            last_error,
        })
    }
}

struct Samples {
    durations_ns: Vec<u64>,
    failure_count: u64,
    last_error: Option<WorkloadError>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FixedWallClock, ScriptedClock, ScriptedWorkload, StaticProbe};

    fn harness(clock: ScriptedClock) -> Harness<ScriptedClock, FixedWallClock, StaticProbe> {
        Harness::new(
            clock,
            FixedWallClock::new(1_700_000_000_000),
            StaticProbe::new(),
        )
    }

    #[test]
    fn a_failed_verification_takes_no_timing_at_all() {
        let clock = ScriptedClock::new([0, 1_000]);
        let harness = harness(clock.clone());
        let mut workload = ScriptedWorkload::new([1_000; 4]).failing_verification();

        let outcome = harness
            .run(
                &mut workload,
                &RunConfig::new("b", "cmd").with_iterations(4),
            )
            .expect("a failed verification is an outcome, not an error");

        assert!(matches!(outcome, RunOutcome::VerificationFailed(_)));
        assert_eq!(outcome.result(), None);
        assert_eq!(clock.read_count(), 0, "the clock must never be read");
        assert_eq!(workload.iterate_calls(), 0, "nothing may be executed");
    }

    #[test]
    fn a_verified_run_is_measured() {
        let clock = ScriptedClock::from_durations([10, 20, 30, 40]);
        let harness = harness(clock);
        let mut workload = ScriptedWorkload::new([10, 20, 30, 40]);

        let outcome = harness
            .run(
                &mut workload,
                &RunConfig::new("b", "cmd")
                    .with_iterations(4)
                    .with_warmup_iterations(0),
            )
            .expect("run succeeds");

        let result = outcome.result().expect("measured").clone();
        assert_eq!(result.samples_ns, vec![10, 20, 30, 40]);
        assert_eq!(result.sample_count, 4);
        assert_eq!(result.failure_count, 0);
        assert_eq!(result.iterations_attempted, 4);
        assert_eq!(result.latency.p50_ns, 20);
        result.validate().expect("the harness emits valid rows");
    }

    #[test]
    fn failing_iterations_are_counted_and_never_timed() {
        let clock = ScriptedClock::from_durations([10, 20, 30, 40]);
        let harness = harness(clock);
        let mut workload = ScriptedWorkload::new([10, 20, 30, 40]).failing_iterations([1, 2]);

        let outcome = harness
            .run(
                &mut workload,
                &RunConfig::new("b", "cmd")
                    .with_iterations(4)
                    .with_warmup_iterations(0),
            )
            .expect("run succeeds");

        let result = outcome.result().expect("measured");
        assert_eq!(result.sample_count, 2);
        assert_eq!(result.failure_count, 2);
        assert_eq!(result.iterations_attempted, 4);
        assert_eq!(result.samples_ns.len(), 2);
    }

    #[test]
    fn a_run_where_everything_fails_has_no_percentiles() {
        let clock = ScriptedClock::from_durations([10, 20]);
        let harness = harness(clock);
        let mut workload = ScriptedWorkload::new([10, 20]).failing_iterations([0, 1]);

        let error = harness
            .run(
                &mut workload,
                &RunConfig::new("b", "cmd")
                    .with_iterations(2)
                    .with_warmup_iterations(0),
            )
            .expect_err("no samples, no summary");
        assert!(matches!(error, HarnessError::AllIterationsFailed { .. }));
    }

    #[test]
    fn zero_iterations_is_a_configuration_error() {
        let harness = harness(ScriptedClock::new([0]));
        let mut workload = ScriptedWorkload::new([10]);
        let error = harness
            .run(
                &mut workload,
                &RunConfig::new("b", "cmd").with_iterations(0),
            )
            .expect_err("zero samples is not a run");
        assert!(matches!(error, HarnessError::Config(_)));
    }

    #[test]
    fn a_backwards_clock_is_refused_rather_than_clamped() {
        let clock = ScriptedClock::new([100, 50]);
        let harness = harness(clock);
        let mut workload = ScriptedWorkload::new([0]);
        let error = harness
            .run(
                &mut workload,
                &RunConfig::new("b", "cmd")
                    .with_iterations(1)
                    .with_warmup_iterations(0),
            )
            .expect_err("negative durations are not measurements");
        assert!(matches!(error, HarnessError::ClockWentBackwards { .. }));
    }

    #[test]
    fn cold_runs_drop_caches_before_every_sample() {
        let clock = ScriptedClock::from_durations([10, 10, 10]);
        let harness = harness(clock);
        let mut workload = ScriptedWorkload::new([10, 10, 10]);

        harness
            .run(
                &mut workload,
                &RunConfig::new("b", "cmd")
                    .with_iterations(3)
                    .with_warmup_iterations(0)
                    .with_cache_state(CacheState::Cold),
            )
            .expect("run succeeds");

        assert_eq!(workload.cold_preparations(), 3);
        assert_eq!(workload.warm_preparations(), 0);
    }

    #[test]
    fn warm_runs_prime_once_and_then_leave_the_cache_alone() {
        let clock = ScriptedClock::from_durations([10, 10, 10, 10, 10]);
        let harness = harness(clock);
        let mut workload = ScriptedWorkload::new([10; 5]);

        harness
            .run(
                &mut workload,
                &RunConfig::new("b", "cmd")
                    .with_iterations(3)
                    .with_warmup_iterations(2)
                    .with_cache_state(CacheState::Warm),
            )
            .expect("run succeeds");

        assert_eq!(workload.warm_preparations(), 1);
        assert_eq!(workload.cold_preparations(), 0);
        assert_eq!(workload.iterate_calls(), 5, "2 warmup + 3 timed");
    }
}
