//! Test doubles and fixtures, shipped as part of the crate.
//!
//! Public on purpose: every benchmark that will eventually emit rows through
//! this harness needs to test its own workload without shelling out to `git`,
//! reading `/proc` or depending on how fast the machine happens to be. Making
//! the doubles part of the published API is what stops each benchmark inventing
//! its own — and inventing one that is more permissive than this one.

use crate::clock::{Clock, WallClock};
use crate::env::{Environment, EnvironmentProbe, ProbeError};
use crate::json::{Json, JsonObject};
use crate::schema::{
    BenchmarkResult, BuildProfile, CacheState, Hardware, HostPlatform, LatencySummary, Repository,
    StorageBoundary, StorageFootprint, Verification, WorkloadDescriptor, SCHEMA_VERSION,
};
use crate::workload::{Workload, WorkloadError};
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

/// A scratch directory that removes itself.
///
/// Corpus generation writes to a filesystem, so testing it needs somewhere to
/// write. It lives here rather than in each test for the reason in the module
/// comment: a scratch directory that forgets to be unique passes alone and
/// fails under `nextest`'s parallelism, and one that forgets to clean up leaves
/// a materialised corpus behind after every run.
#[derive(Debug)]
pub struct TempDir {
    path: PathBuf,
}

/// Distinguishes two scratch directories made in the same process.
static SCRATCH_ORDINAL: AtomicU64 = AtomicU64::new(0);

impl TempDir {
    /// Creates a scratch directory whose name carries `label`.
    ///
    /// # Panics
    ///
    /// If the directory cannot be created, which leaves a test with nowhere to
    /// write and nothing useful to do next.
    #[must_use]
    pub fn new(label: &str) -> Self {
        let ordinal = SCRATCH_ORDINAL.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "mesh-bench-{label}-{}-{ordinal}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path)
            .unwrap_or_else(|error| panic!("cannot create {}: {error}", path.display()));
        TempDir { path }
    }

    /// The directory.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // Best effort: a failed cleanup must not turn a passing test red, and a
        // panic while unwinding aborts the process.
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// A clock that returns a fixed script of readings and counts its reads.
///
/// Panics when the script runs out: a harness that reads the clock more often
/// than the test expected has changed its measurement behaviour, and silently
/// repeating the last reading would hide exactly that.
#[derive(Clone, Debug)]
pub struct ScriptedClock {
    readings: Rc<RefCell<Vec<u64>>>,
    reads: Rc<RefCell<usize>>,
}

impl ScriptedClock {
    /// A clock that hands out `readings` in order.
    pub fn new(readings: impl IntoIterator<Item = u64>) -> Self {
        ScriptedClock {
            readings: Rc::new(RefCell::new(readings.into_iter().collect())),
            reads: Rc::new(RefCell::new(0)),
        }
    }

    /// A clock whose reading pairs produce exactly `durations`.
    pub fn from_durations(durations: impl IntoIterator<Item = u64>) -> Self {
        let mut readings = Vec::new();
        let mut now = 0_u64;
        for duration in durations {
            readings.push(now);
            now += duration;
            readings.push(now);
        }
        ScriptedClock::new(readings)
    }

    /// How many times the clock has been read.
    pub fn read_count(&self) -> usize {
        *self.reads.borrow()
    }
}

impl Clock for ScriptedClock {
    fn now_nanos(&self) -> u64 {
        let index = *self.reads.borrow();
        *self.reads.borrow_mut() += 1;
        *self.readings.borrow().get(index).unwrap_or_else(|| {
            panic!(
                "the scripted clock was read {} times, more than it was scripted for",
                index + 1
            )
        })
    }
}

/// A wall clock frozen at one instant.
#[derive(Clone, Copy, Debug)]
pub struct FixedWallClock {
    unix_millis: u64,
}

impl FixedWallClock {
    /// Freezes the wall clock at `unix_millis`.
    pub fn new(unix_millis: u64) -> Self {
        FixedWallClock { unix_millis }
    }
}

impl WallClock for FixedWallClock {
    fn unix_millis(&self) -> u64 {
        self.unix_millis
    }
}

/// An environment probe that returns fixed, valid facts.
#[derive(Clone, Debug)]
pub struct StaticProbe {
    environment: Environment,
    failure: Option<ProbeError>,
}

impl StaticProbe {
    /// A probe reporting a plausible machine and a clean checkout.
    pub fn new() -> Self {
        StaticProbe {
            environment: sample_environment(),
            failure: None,
        }
    }

    /// A probe that reports a dirty worktree.
    #[must_use]
    pub fn dirty(mut self) -> Self {
        self.environment.repository.dirty = true;
        self
    }

    /// A probe that cannot establish the environment.
    #[must_use]
    pub fn failing(mut self, error: ProbeError) -> Self {
        self.failure = Some(error);
        self
    }
}

impl Default for StaticProbe {
    fn default() -> Self {
        StaticProbe::new()
    }
}

impl EnvironmentProbe for StaticProbe {
    fn probe(&self) -> Result<Environment, ProbeError> {
        match &self.failure {
            Some(error) => Err(error.clone()),
            None => Ok(self.environment.clone()),
        }
    }
}

/// A workload whose verification, failures and call counts are scripted.
#[derive(Clone, Debug)]
pub struct ScriptedWorkload {
    durations_ns: Vec<u64>,
    verification_passes: bool,
    failing_iterations: BTreeSet<u64>,
    iterate_calls: u64,
    cold_preparations: u64,
    warm_preparations: u64,
}

impl ScriptedWorkload {
    /// A workload that verifies and never fails.
    pub fn new(durations_ns: impl IntoIterator<Item = u64>) -> Self {
        ScriptedWorkload {
            durations_ns: durations_ns.into_iter().collect(),
            verification_passes: true,
            failing_iterations: BTreeSet::new(),
            iterate_calls: 0,
            cold_preparations: 0,
            warm_preparations: 0,
        }
    }

    /// Returns a copy whose correctness verification fails.
    #[must_use]
    pub fn failing_verification(mut self) -> Self {
        self.verification_passes = false;
        self
    }

    /// Returns a copy where the given zero-based iteration indices fail.
    #[must_use]
    pub fn failing_iterations(mut self, indices: impl IntoIterator<Item = u64>) -> Self {
        self.failing_iterations = indices.into_iter().collect();
        self
    }

    /// How many times `iterate` was called.
    pub fn iterate_calls(&self) -> u64 {
        self.iterate_calls
    }

    /// How many times the caches were dropped.
    pub fn cold_preparations(&self) -> u64 {
        self.cold_preparations
    }

    /// How many times the caches were primed.
    pub fn warm_preparations(&self) -> u64 {
        self.warm_preparations
    }
}

impl Workload for ScriptedWorkload {
    fn descriptor(&self) -> WorkloadDescriptor {
        WorkloadDescriptor {
            generator: "mesh-bench/scripted".to_owned(),
            generator_version: "1".to_owned(),
            seed: 1,
            parameters: JsonObject::new().with(
                "scripted_durations_ns",
                Json::array(self.durations_ns.iter().copied().map(Json::Uint)),
            ),
        }
    }

    fn verify(&mut self) -> Result<Verification, WorkloadError> {
        let observed = if self.verification_passes {
            "digest:expected"
        } else {
            "digest:wrong"
        };
        Ok(Verification::new("scripted", "digest:expected", observed))
    }

    fn prepare(&mut self, cache_state: CacheState) -> Result<(), WorkloadError> {
        match cache_state {
            CacheState::Cold => self.cold_preparations += 1,
            CacheState::Warm => self.warm_preparations += 1,
        }
        Ok(())
    }

    fn iterate(&mut self) -> Result<(), WorkloadError> {
        let index = self.iterate_calls;
        self.iterate_calls += 1;
        if self.failing_iterations.contains(&index) {
            return Err(WorkloadError::new(format!("scripted failure at {index}")));
        }
        Ok(())
    }
}

/// A plausible, internally consistent environment.
pub fn sample_environment() -> Environment {
    Environment {
        repository: Repository {
            remote: "https://github.com/idosams/Mesh.git".to_owned(),
            commit: "a".repeat(40),
            dirty: false,
        },
        hardware: Hardware {
            cpu_model: "Apple M3 Max".to_owned(),
            physical_cores: 14,
            logical_cores: 14,
            memory_bytes: 36 * 1024 * 1024 * 1024,
        },
        platform: HostPlatform {
            os: "macos".to_owned(),
            os_version: "15.5 (24F74)".to_owned(),
            arch: "aarch64".to_owned(),
            filesystem: "apfs".to_owned(),
        },
        build: BuildProfile {
            profile: "release".to_owned(),
            opt_level: "3".to_owned(),
            debug_info: "true".to_owned(),
            rustc_version: "rustc 1.97.1 (8bab26f4f 2026-07-14)".to_owned(),
            target_triple: "aarch64-apple-darwin".to_owned(),
        },
    }
}

/// A complete, valid result row — the fixture the schema tests mutilate.
pub fn sample_result() -> BenchmarkResult {
    let samples: Vec<u64> = (1..=100).map(|index: u64| 1_000 + index * 7).collect();
    sample_result_with_samples(&samples)
}

/// A complete, valid result row over the given raw samples.
pub fn sample_result_with_samples(samples_ns: &[u64]) -> BenchmarkResult {
    let environment = sample_environment();
    let latency =
        LatencySummary::from_samples(samples_ns).expect("a fixture must carry at least one sample");
    BenchmarkResult {
        schema_version: SCHEMA_VERSION.to_owned(),
        benchmark_id: "mesh-bench/reference/blob-scan".to_owned(),
        invocation: "mesh-bench run --workload blob-scan --iterations 100".to_owned(),
        recorded_at_unix_ms: 1_775_000_000_000,
        repository: environment.repository,
        hardware: environment.hardware,
        platform: environment.platform,
        build: environment.build,
        workload: WorkloadDescriptor {
            generator: "mesh-bench/xorshift64star-blobs".to_owned(),
            generator_version: "1".to_owned(),
            seed: 42,
            parameters: JsonObject::new()
                .with("blob_count", Json::Uint(64))
                .with("blob_bytes", Json::Uint(4096)),
        },
        cache_state: CacheState::Warm,
        sample_count: samples_ns.len() as u64,
        iterations_attempted: samples_ns.len() as u64,
        failure_count: 0,
        samples_ns: samples_ns.to_vec(),
        latency,
        verification: Verification::new(
            "blob-digest-fnv1a64",
            "fnv1a64:0123456789abcdef",
            "fnv1a64:0123456789abcdef",
        ),
        storage: None,
    }
}

/// A complete, valid result row that also carries the optional storage section.
///
/// Separate from [`sample_result`] on purpose: the required contract's tests
/// start from a row with no storage at all, because that is what almost every
/// row is, and a fixture that always carried the optional section would let the
/// section drift into being required without anybody noticing.
pub fn sample_storage_result() -> BenchmarkResult {
    BenchmarkResult {
        storage: StorageFootprint::measure(StorageBoundary::Store, "chunk", 3_072, 1_024),
        ..sample_result()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fixture_is_a_valid_row() {
        sample_result()
            .validate()
            .expect("the fixture every schema test starts from must itself be valid");
    }

    #[test]
    fn the_storage_fixture_is_a_valid_row() {
        let row = sample_storage_result();
        row.validate().expect("the storage fixture must be valid");
        assert_eq!(row.storage.expect("storage").amplification_per_mille, 3_000);
    }

    #[test]
    fn scripted_clocks_produce_the_requested_durations() {
        let clock = ScriptedClock::from_durations([10, 25]);
        assert_eq!(clock.now_nanos(), 0);
        assert_eq!(clock.now_nanos(), 10);
        assert_eq!(clock.now_nanos(), 10);
        assert_eq!(clock.now_nanos(), 35);
        assert_eq!(clock.read_count(), 4);
    }

    #[test]
    #[should_panic(expected = "more than it was scripted for")]
    fn an_exhausted_scripted_clock_panics_rather_than_inventing_time() {
        let clock = ScriptedClock::new([1]);
        let _ = clock.now_nanos();
        let _ = clock.now_nanos();
    }

    #[test]
    fn a_failing_probe_reports_its_error() {
        let probe = StaticProbe::new().failing(ProbeError::missing("platform.filesystem", "test"));
        assert!(probe.probe().is_err());
    }

    #[test]
    fn the_dirty_probe_reports_a_dirty_worktree() {
        assert!(
            StaticProbe::new()
                .dirty()
                .probe()
                .expect("probes")
                .repository
                .dirty
        );
    }
}
