//! The result row: the only shape a published Mesh number is allowed to have.
//!
//! Every field here answers a question a sceptical reader will ask — *which
//! commit, on what machine, built how, over which data, with which cache state,
//! how many samples, and how do you know the workload was even correct*. The
//! set is required in full precisely because the missing field is always the
//! one that would have explained the number away.

use super::error::SchemaError;
use crate::json::JsonObject;
use crate::stats::{mean_ns, percentile_ns, sorted_copy, P50, P95, P99};

/// The schema version written into, and demanded from, every row.
pub const SCHEMA_VERSION: &str = "mesh-bench/result/v1";

/// The commit the measured code was built from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repository {
    /// The remote a third party clones to obtain the commit.
    pub remote: String,
    /// The full 40-character lowercase hex commit the binary was built from.
    pub commit: String,
    /// Whether the worktree carried uncommitted changes at capture time.
    ///
    /// Kept rather than suppressed: a dirty run is measurable but not
    /// reproducible, and the sink policy — not the row — decides whether that
    /// is publishable.
    pub dirty: bool,
}

/// The machine the samples were taken on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hardware {
    /// CPU model string as reported by the host.
    pub cpu_model: String,
    /// Physical core count.
    pub physical_cores: u64,
    /// Logical (hardware-thread) count.
    pub logical_cores: u64,
    /// Installed memory in bytes.
    pub memory_bytes: u64,
}

/// The operating system and the filesystem the workload data lived on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostPlatform {
    /// Operating system name (`macos`, `linux`, …).
    pub os: String,
    /// Kernel or product version string.
    pub os_version: String,
    /// CPU architecture (`aarch64`, `x86_64`, …).
    pub arch: String,
    /// Filesystem type backing the workload directory (`apfs`, `ext4`, …).
    ///
    /// Mesh benchmarks are dominated by storage behaviour; a number without a
    /// filesystem is not a number.
    pub filesystem: String,
}

/// How the measured binary was compiled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildProfile {
    /// Cargo profile name (`release`, `debug`, …).
    pub profile: String,
    /// Optimisation level.
    pub opt_level: String,
    /// Debug-info setting reported by Cargo.
    pub debug_info: String,
    /// `rustc --version` of the compiler that produced the binary.
    pub rustc_version: String,
    /// Target triple the binary was compiled for.
    pub target_triple: String,
}

/// The data the workload ran over, described well enough to regenerate it.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkloadDescriptor {
    /// Identifier of the data generator, including its own version.
    pub generator: String,
    /// Version of the generator's output format or algorithm.
    pub generator_version: String,
    /// Seed the generator was driven with — the reason two runs see the same
    /// bytes.
    pub seed: u64,
    /// Generator parameters (sizes, counts, shapes).
    pub parameters: JsonObject,
}

/// Whether the caches were deliberately cold or deliberately warm.
///
/// There is no third option on purpose. "Whatever the machine happened to have
/// cached" is the single most common way a storage benchmark lies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheState {
    /// Caches dropped before every sample.
    Cold,
    /// Caches deliberately primed before timing begins.
    Warm,
}

impl CacheState {
    /// The wire word for this state.
    pub fn as_word(self) -> &'static str {
        match self {
            CacheState::Cold => "cold",
            CacheState::Warm => "warm",
        }
    }

    /// Parses the wire word, rejecting anything outside the vocabulary.
    pub fn from_word(word: &str) -> Option<Self> {
        match word {
            "cold" => Some(CacheState::Cold),
            "warm" => Some(CacheState::Warm),
            _ => None,
        }
    }
}

/// The distribution summary derived from the raw samples.
///
/// Every field is recomputable from `samples_ns`; the decoder recomputes them
/// and rejects the row if they disagree, so the summary can never drift from
/// the data it claims to summarise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LatencySummary {
    /// Fastest sample, nanoseconds.
    pub min_ns: u64,
    /// 50th percentile, nanoseconds.
    pub p50_ns: u64,
    /// 95th percentile, nanoseconds.
    pub p95_ns: u64,
    /// 99th percentile, nanoseconds.
    pub p99_ns: u64,
    /// Slowest sample, nanoseconds.
    pub max_ns: u64,
    /// Arithmetic mean, nanoseconds, truncated towards zero.
    pub mean_ns: u64,
}

impl LatencySummary {
    /// Derives the summary from raw samples, or `None` when there are none.
    pub fn from_samples(samples: &[u64]) -> Option<Self> {
        let sorted = sorted_copy(samples);
        Some(LatencySummary {
            min_ns: *sorted.first()?,
            p50_ns: percentile_ns(&sorted, P50)?,
            p95_ns: percentile_ns(&sorted, P95)?,
            p99_ns: percentile_ns(&sorted, P99)?,
            max_ns: *sorted.last()?,
            mean_ns: mean_ns(&sorted)?,
        })
    }
}

/// Where a storage figure was taken.
///
/// Two words, closed on purpose. `store` means the bytes were counted in a real
/// content-addressed store after a real promotion; `chunking` means the run
/// could not reach a store and counted the bytes a chunker would have handed
/// one. They are three orders of magnitude apart on some workloads, so a row
/// that did not say which would be unreadable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageBoundary {
    /// Counted in the content-addressed store itself.
    Store,
    /// Counted at the chunking boundary, because no store was reachable.
    Chunking,
}

impl StorageBoundary {
    /// The wire word for this boundary.
    pub fn as_word(self) -> &'static str {
        match self {
            StorageBoundary::Store => "store",
            StorageBoundary::Chunking => "chunking",
        }
    }

    /// Parses the wire word, rejecting anything outside the vocabulary.
    pub fn from_word(word: &str) -> Option<Self> {
        match word {
            "store" => Some(StorageBoundary::Store),
            "chunking" => Some(StorageBoundary::Chunking),
            _ => None,
        }
    }
}

/// What a run left on disk, and what it left it for.
///
/// Optional, and the only optional section in the row: most workloads measure a
/// latency and admit nothing, and a section of zeroes on those rows would be a
/// measurement nobody took. A workload that admits bytes carries this; one that
/// does not, omits it.
///
/// `amplification_per_mille` is **derived** from the two raw byte counts exactly
/// as `latency` is derived from `samples_ns`: the decoder recomputes it and
/// rejects the row when the declared value disagrees, so a hand-edited ratio is
/// refused the same way a hand-edited percentile is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorageFootprint {
    /// Whether the bytes were counted in the store or at the chunker.
    pub boundary: StorageBoundary,
    /// The unit the store admits bytes in — `chunk` for a content-addressed store.
    pub granularity: String,
    /// Bytes the store holds after the workload, including its own bookkeeping.
    pub admitted_bytes: u64,
    /// Bytes of distinct final content the workload asked it to hold.
    pub distinct_content_bytes: u64,
    /// `admitted_bytes / distinct_content_bytes`, in parts per thousand.
    ///
    /// Per mille rather than a float because every other derived number in this
    /// schema is an integer, and a ratio recomputed from two integers has to
    /// come out bit-identical on every host for the check above to mean
    /// anything.
    pub amplification_per_mille: u64,
}

impl StorageFootprint {
    /// Builds a footprint, deriving the ratio from the two raw counts.
    ///
    /// Returns `None` when there is no distinct content to divide by: a ratio
    /// over zero bytes is not a small number, it is not a number.
    pub fn measure(
        boundary: StorageBoundary,
        granularity: impl Into<String>,
        admitted_bytes: u64,
        distinct_content_bytes: u64,
    ) -> Option<Self> {
        Some(StorageFootprint {
            boundary,
            granularity: granularity.into(),
            admitted_bytes,
            distinct_content_bytes,
            amplification_per_mille: amplification_per_mille(
                admitted_bytes,
                distinct_content_bytes,
            )?,
        })
    }
}

/// `admitted * 1000 / distinct`, truncated, or `None` when `distinct` is zero.
///
/// `u128` throughout: a hundred-gigabyte workload times a thousand overflows a
/// `u64` at four exabytes, which is far enough away to be tempting and not far
/// enough away to be safe.
pub fn amplification_per_mille(admitted_bytes: u64, distinct_content_bytes: u64) -> Option<u64> {
    if distinct_content_bytes == 0 {
        return None;
    }
    let scaled = u128::from(admitted_bytes) * 1000 / u128::from(distinct_content_bytes);
    u64::try_from(scaled).ok()
}

/// The correctness check that ran *before* any timing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verification {
    /// How correctness was established (digest comparison, replay, …).
    pub method: String,
    /// The digest the workload is contracted to produce.
    pub expected_digest: String,
    /// The digest the workload actually produced.
    pub observed_digest: String,
}

impl Verification {
    /// Builds a verification record from its two digests.
    pub fn new(
        method: impl Into<String>,
        expected_digest: impl Into<String>,
        observed_digest: impl Into<String>,
    ) -> Self {
        Verification {
            method: method.into(),
            expected_digest: expected_digest.into(),
            observed_digest: observed_digest.into(),
        }
    }

    /// Whether the workload produced what it was contracted to produce.
    ///
    /// Derived, never asserted: the wire form carries the boolean too, and the
    /// decoder rejects a row whose boolean disagrees with its digests.
    pub fn passed(&self) -> bool {
        self.expected_digest == self.observed_digest
    }
}

/// One benchmark run, complete enough for a stranger to repeat it.
#[derive(Clone, Debug, PartialEq)]
pub struct BenchmarkResult {
    /// Schema version — always [`SCHEMA_VERSION`] for rows this build writes.
    pub schema_version: String,
    /// Stable identifier of the benchmark that produced the row.
    pub benchmark_id: String,
    /// The exact command line that produced the row.
    pub invocation: String,
    /// Wall-clock capture time, milliseconds since the Unix epoch.
    ///
    /// Descriptive only. Nothing in Mesh orders anything by this field.
    pub recorded_at_unix_ms: u64,
    /// The commit under measurement.
    pub repository: Repository,
    /// The machine.
    pub hardware: Hardware,
    /// The OS and filesystem.
    pub platform: HostPlatform,
    /// The build profile.
    pub build: BuildProfile,
    /// The workload and its data generator.
    pub workload: WorkloadDescriptor,
    /// Cold or warm caches.
    pub cache_state: CacheState,
    /// Number of timed samples — equal to `samples_ns.len()`.
    pub sample_count: u64,
    /// Iterations attempted, including the ones that failed.
    pub iterations_attempted: u64,
    /// Iterations that errored and were therefore never timed.
    pub failure_count: u64,
    /// Raw per-sample durations in nanoseconds, in observation order.
    pub samples_ns: Vec<u64>,
    /// The derived distribution summary.
    pub latency: LatencySummary,
    /// The correctness verification that gated the timing.
    pub verification: Verification,
    /// What the run left on disk, when it admitted anything.
    ///
    /// `None` for every workload that measures a latency and stores nothing,
    /// which is most of them.
    pub storage: Option<StorageFootprint>,
}

impl BenchmarkResult {
    /// Checks everything the row can be checked against on its own.
    ///
    /// Called by the decoder and again by the sink, so a row cannot enter the
    /// corpus through either door without passing.
    pub fn validate(&self) -> Result<(), SchemaError> {
        self.validate_identity()?;
        self.validate_environment()?;
        self.validate_counts()?;
        self.validate_latency()?;
        self.validate_verification()?;
        self.validate_storage()
    }

    fn validate_identity(&self) -> Result<(), SchemaError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(SchemaError::UnsupportedSchemaVersion {
                found: self.schema_version.clone(),
                expected: SCHEMA_VERSION,
            });
        }
        require_non_empty("benchmark_id", &self.benchmark_id)?;
        require_non_empty("invocation", &self.invocation)?;
        if self.recorded_at_unix_ms == 0 {
            return Err(SchemaError::out_of_range(
                "recorded_at_unix_ms",
                "must be a real capture time, not zero",
            ));
        }
        Ok(())
    }

    fn validate_environment(&self) -> Result<(), SchemaError> {
        require_non_empty("repository.remote", &self.repository.remote)?;
        require_commit("repository.commit", &self.repository.commit)?;

        require_non_empty("hardware.cpu_model", &self.hardware.cpu_model)?;
        require_positive("hardware.physical_cores", self.hardware.physical_cores)?;
        require_positive("hardware.logical_cores", self.hardware.logical_cores)?;
        require_positive("hardware.memory_bytes", self.hardware.memory_bytes)?;
        if self.hardware.logical_cores < self.hardware.physical_cores {
            return Err(SchemaError::inconsistent(
                "hardware.logical_cores is below hardware.physical_cores",
            ));
        }

        require_non_empty("platform.os", &self.platform.os)?;
        require_non_empty("platform.os_version", &self.platform.os_version)?;
        require_non_empty("platform.arch", &self.platform.arch)?;
        require_non_empty("platform.filesystem", &self.platform.filesystem)?;

        require_non_empty("build.profile", &self.build.profile)?;
        require_non_empty("build.opt_level", &self.build.opt_level)?;
        require_non_empty("build.debug_info", &self.build.debug_info)?;
        require_non_empty("build.rustc_version", &self.build.rustc_version)?;
        require_non_empty("build.target_triple", &self.build.target_triple)?;

        require_non_empty("workload.generator", &self.workload.generator)?;
        require_non_empty(
            "workload.generator_version",
            &self.workload.generator_version,
        )
    }

    fn validate_counts(&self) -> Result<(), SchemaError> {
        require_positive("sample_count", self.sample_count)?;
        if self.sample_count != self.samples_ns.len() as u64 {
            return Err(SchemaError::inconsistent(format!(
                "sample_count is {} but samples_ns holds {} values",
                self.sample_count,
                self.samples_ns.len()
            )));
        }
        let accounted = self
            .sample_count
            .checked_add(self.failure_count)
            .ok_or_else(|| {
                SchemaError::out_of_range("failure_count", "sample_count + failure_count overflows")
            })?;
        if self.iterations_attempted != accounted {
            return Err(SchemaError::inconsistent(format!(
                "iterations_attempted is {} but sample_count + failure_count is {accounted}",
                self.iterations_attempted
            )));
        }
        Ok(())
    }

    fn validate_latency(&self) -> Result<(), SchemaError> {
        let derived = LatencySummary::from_samples(&self.samples_ns)
            .ok_or_else(|| SchemaError::missing("samples_ns"))?;
        if derived != self.latency {
            return Err(SchemaError::inconsistent(format!(
                "latency does not match samples_ns (declared {:?}, derived {derived:?})",
                self.latency
            )));
        }
        Ok(())
    }

    fn validate_verification(&self) -> Result<(), SchemaError> {
        require_non_empty("verification.method", &self.verification.method)?;
        require_non_empty(
            "verification.expected_digest",
            &self.verification.expected_digest,
        )?;
        require_non_empty(
            "verification.observed_digest",
            &self.verification.observed_digest,
        )?;
        if !self.verification.passed() {
            return Err(SchemaError::UnverifiedRun {
                method: self.verification.method.clone(),
                expected_digest: self.verification.expected_digest.clone(),
                observed_digest: self.verification.observed_digest.clone(),
            });
        }
        Ok(())
    }

    fn validate_storage(&self) -> Result<(), SchemaError> {
        let Some(storage) = &self.storage else {
            return Ok(());
        };
        require_non_empty("storage.granularity", &storage.granularity)?;
        require_positive("storage.admitted_bytes", storage.admitted_bytes)?;
        require_positive(
            "storage.distinct_content_bytes",
            storage.distinct_content_bytes,
        )?;
        let derived =
            amplification_per_mille(storage.admitted_bytes, storage.distinct_content_bytes)
                .ok_or_else(|| SchemaError::missing("storage.distinct_content_bytes"))?;
        if derived != storage.amplification_per_mille {
            return Err(SchemaError::inconsistent(format!(
                "storage.amplification_per_mille is {} but {} admitted bytes over {} distinct \
                 content bytes is {derived}",
                storage.amplification_per_mille,
                storage.admitted_bytes,
                storage.distinct_content_bytes
            )));
        }
        Ok(())
    }
}

fn require_non_empty(field: &str, value: &str) -> Result<(), SchemaError> {
    if value.trim().is_empty() {
        return Err(SchemaError::out_of_range(field, "must not be empty"));
    }
    Ok(())
}

fn require_positive(field: &str, value: u64) -> Result<(), SchemaError> {
    if value == 0 {
        return Err(SchemaError::out_of_range(
            field,
            "must be greater than zero",
        ));
    }
    Ok(())
}

fn require_commit(field: &str, value: &str) -> Result<(), SchemaError> {
    let looks_like_a_commit = value.len() == 40
        && value
            .chars()
            .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase());
    if !looks_like_a_commit {
        return Err(SchemaError::out_of_range(
            field,
            "must be a full 40-character lowercase hex commit",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_state_vocabulary_is_closed() {
        assert_eq!(CacheState::from_word("cold"), Some(CacheState::Cold));
        assert_eq!(CacheState::from_word("warm"), Some(CacheState::Warm));
        assert_eq!(CacheState::from_word("whatever"), None);
        assert_eq!(CacheState::Warm.as_word(), "warm");
    }

    #[test]
    fn latency_summary_is_derived_from_samples() {
        let samples: Vec<u64> = (1..=100).collect();
        let summary = LatencySummary::from_samples(&samples).expect("non-empty");
        assert_eq!(summary.min_ns, 1);
        assert_eq!(summary.p50_ns, 50);
        assert_eq!(summary.p95_ns, 95);
        assert_eq!(summary.p99_ns, 99);
        assert_eq!(summary.max_ns, 100);
        assert_eq!(summary.mean_ns, 50);
    }

    #[test]
    fn no_samples_means_no_summary() {
        assert_eq!(LatencySummary::from_samples(&[]), None);
    }

    #[test]
    fn verification_passes_only_when_digests_agree() {
        assert!(Verification::new("digest", "a", "a").passed());
        assert!(!Verification::new("digest", "a", "b").passed());
    }

    #[test]
    fn a_ratio_is_derived_from_the_two_raw_counts() {
        let footprint = StorageFootprint::measure(StorageBoundary::Store, "chunk", 2_048, 1_024)
            .expect("a positive denominator");
        assert_eq!(footprint.amplification_per_mille, 2_000);
    }

    #[test]
    fn no_distinct_content_means_no_ratio_rather_than_a_large_one() {
        assert_eq!(amplification_per_mille(1_024, 0), None);
        assert!(StorageFootprint::measure(StorageBoundary::Store, "chunk", 1_024, 0).is_none());
    }

    #[test]
    fn the_ratio_truncates_rather_than_rounding_so_two_hosts_agree() {
        // 1499 / 1000 is 1.499; per mille that is 1499 exactly, and 1500/1001
        // must not round up into it.
        assert_eq!(amplification_per_mille(1_499, 1_000), Some(1_499));
        assert_eq!(amplification_per_mille(1_000, 3), Some(333_333));
    }

    #[test]
    fn a_hundred_gigabyte_workload_does_not_overflow_the_scale() {
        let hundred_gigabytes = 100_u64 * 1024 * 1024 * 1024;
        assert_eq!(
            amplification_per_mille(hundred_gigabytes, hundred_gigabytes),
            Some(1_000)
        );
    }

    #[test]
    fn the_storage_boundary_vocabulary_is_closed() {
        assert_eq!(
            StorageBoundary::from_word("store"),
            Some(StorageBoundary::Store)
        );
        assert_eq!(
            StorageBoundary::from_word("chunking"),
            Some(StorageBoundary::Chunking)
        );
        assert_eq!(StorageBoundary::from_word("roughly"), None);
        assert_eq!(StorageBoundary::Chunking.as_word(), "chunking");
    }

    #[test]
    fn commit_shape_is_enforced() {
        assert!(require_commit("repository.commit", &"a".repeat(40)).is_ok());
        assert!(require_commit("repository.commit", &"A".repeat(40)).is_err());
        assert!(require_commit("repository.commit", "abc").is_err());
        assert!(require_commit("repository.commit", &"g".repeat(40)).is_err());
    }
}
