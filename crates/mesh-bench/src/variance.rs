//! The stated variance band, and the comparison that enforces it.
//!
//! "Reproducible" is not a feeling. Two runs of the same commit, on the same
//! machine, over the same generated data, in the same cache state must land
//! inside [`VarianceBand::STATED`]; if they do not, the instrument is not
//! trustworthy yet and nothing measured with it should be published. The band
//! widens with the percentile because tail latency is genuinely noisier than
//! the median — but it is stated up front, in one place, rather than decided
//! after seeing the numbers.

use crate::schema::BenchmarkResult;
use std::fmt;

/// The maximum relative difference tolerated between two runs, in permille.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VarianceBand {
    /// Tolerance on p50.
    pub p50_permille: u32,
    /// Tolerance on p95.
    pub p95_permille: u32,
    /// Tolerance on p99.
    pub p99_permille: u32,
}

impl VarianceBand {
    /// The band Mesh publishes against: 5 % on p50, 10 % on p95, 15 % on p99.
    pub const STATED: VarianceBand = VarianceBand {
        p50_permille: 50,
        p95_permille: 100,
        p99_permille: 150,
    };
}

impl Default for VarianceBand {
    fn default() -> Self {
        VarianceBand::STATED
    }
}

/// One percentile compared across two runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PercentileDelta {
    /// Which percentile this is.
    pub percentile: &'static str,
    /// The baseline run's value.
    pub baseline_ns: u64,
    /// The candidate run's value.
    pub candidate_ns: u64,
    /// Observed relative difference, in permille of the baseline.
    pub observed_permille: u32,
    /// Tolerated relative difference, in permille.
    pub allowed_permille: u32,
}

impl PercentileDelta {
    /// Whether this percentile stayed inside the band.
    pub fn within_band(&self) -> bool {
        self.observed_permille <= self.allowed_permille
    }
}

/// The verdict on a pair of runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepeatabilityReport {
    /// Per-percentile comparisons, in p50/p95/p99 order.
    pub deltas: Vec<PercentileDelta>,
    /// The band that was applied.
    pub band: VarianceBand,
}

impl RepeatabilityReport {
    /// Whether every percentile stayed inside the band.
    pub fn within_band(&self) -> bool {
        self.deltas.iter().all(PercentileDelta::within_band)
    }

    /// The percentiles that fell outside the band.
    pub fn breaches(&self) -> Vec<&PercentileDelta> {
        self.deltas
            .iter()
            .filter(|delta| !delta.within_band())
            .collect()
    }
}

impl fmt::Display for RepeatabilityReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for delta in &self.deltas {
            writeln!(
                f,
                "{}: {} ns -> {} ns ({}.{}% of baseline, band {}.{}%) {}",
                delta.percentile,
                delta.baseline_ns,
                delta.candidate_ns,
                delta.observed_permille / 10,
                delta.observed_permille % 10,
                delta.allowed_permille / 10,
                delta.allowed_permille % 10,
                if delta.within_band() { "ok" } else { "BREACH" }
            )?;
        }
        Ok(())
    }
}

/// Two runs cannot be compared: they did not measure the same thing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComparabilityError {
    /// The field that differs.
    pub field: &'static str,
    /// The baseline run's value.
    pub baseline: String,
    /// The candidate run's value.
    pub candidate: String,
}

impl fmt::Display for ComparabilityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "runs are not comparable: `{}` is `{}` in the baseline and `{}` in the candidate",
            self.field, self.baseline, self.candidate
        )
    }
}

impl std::error::Error for ComparabilityError {}

/// Compares two runs against a band, refusing pairs that are not comparable.
///
/// Comparability is checked before variance, because a "5 % regression" between
/// two different commits, machines or cache states is not a regression at all —
/// it is a different benchmark.
pub fn compare(
    baseline: &BenchmarkResult,
    candidate: &BenchmarkResult,
    band: VarianceBand,
) -> Result<RepeatabilityReport, ComparabilityError> {
    require_same(
        "benchmark_id",
        &baseline.benchmark_id,
        &candidate.benchmark_id,
    )?;
    require_same(
        "repository.commit",
        &baseline.repository.commit,
        &candidate.repository.commit,
    )?;
    require_same(
        "hardware.cpu_model",
        &baseline.hardware.cpu_model,
        &candidate.hardware.cpu_model,
    )?;
    require_same("platform.os", &baseline.platform.os, &candidate.platform.os)?;
    require_same(
        "platform.filesystem",
        &baseline.platform.filesystem,
        &candidate.platform.filesystem,
    )?;
    require_same(
        "build.profile",
        &baseline.build.profile,
        &candidate.build.profile,
    )?;
    require_same(
        "build.rustc_version",
        &baseline.build.rustc_version,
        &candidate.build.rustc_version,
    )?;
    require_same(
        "build.target_triple",
        &baseline.build.target_triple,
        &candidate.build.target_triple,
    )?;
    require_same(
        "workload.generator",
        &baseline.workload.generator,
        &candidate.workload.generator,
    )?;
    require_same(
        "workload.generator_version",
        &baseline.workload.generator_version,
        &candidate.workload.generator_version,
    )?;
    require_same(
        "workload.seed",
        &baseline.workload.seed.to_string(),
        &candidate.workload.seed.to_string(),
    )?;
    require_same(
        "cache_state",
        baseline.cache_state.as_word(),
        candidate.cache_state.as_word(),
    )?;

    Ok(RepeatabilityReport {
        deltas: vec![
            delta(
                "p50",
                baseline.latency.p50_ns,
                candidate.latency.p50_ns,
                band.p50_permille,
            ),
            delta(
                "p95",
                baseline.latency.p95_ns,
                candidate.latency.p95_ns,
                band.p95_permille,
            ),
            delta(
                "p99",
                baseline.latency.p99_ns,
                candidate.latency.p99_ns,
                band.p99_permille,
            ),
        ],
        band,
    })
}

fn require_same(
    field: &'static str,
    baseline: &str,
    candidate: &str,
) -> Result<(), ComparabilityError> {
    if baseline == candidate {
        return Ok(());
    }
    Err(ComparabilityError {
        field,
        baseline: baseline.to_owned(),
        candidate: candidate.to_owned(),
    })
}

/// Relative difference in permille of the baseline, saturating rather than
/// wrapping so a pathological pair reports "wildly out of band" instead of a
/// small number.
fn delta(
    percentile: &'static str,
    baseline_ns: u64,
    candidate_ns: u64,
    allowed_permille: u32,
) -> PercentileDelta {
    let difference = baseline_ns.abs_diff(candidate_ns);
    let observed_permille = if baseline_ns == 0 {
        if difference == 0 {
            0
        } else {
            u32::MAX
        }
    } else {
        u32::try_from(u128::from(difference) * 1000 / u128::from(baseline_ns)).unwrap_or(u32::MAX)
    };
    PercentileDelta {
        percentile,
        baseline_ns,
        candidate_ns,
        observed_permille,
        allowed_permille,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::CacheState;
    use crate::testing::sample_result;

    fn with_latency(p50: u64, p95: u64, p99: u64) -> BenchmarkResult {
        let mut result = sample_result();
        result.latency.p50_ns = p50;
        result.latency.p95_ns = p95;
        result.latency.p99_ns = p99;
        result
    }

    #[test]
    fn identical_runs_are_inside_the_band() {
        let report =
            compare(&sample_result(), &sample_result(), VarianceBand::STATED).expect("comparable");
        assert!(report.within_band());
        assert!(report.breaches().is_empty());
    }

    #[test]
    fn a_four_percent_median_shift_is_inside_the_band() {
        let baseline = with_latency(1_000, 2_000, 3_000);
        let candidate = with_latency(1_040, 2_000, 3_000);
        let report = compare(&baseline, &candidate, VarianceBand::STATED).expect("comparable");
        assert!(report.within_band());
    }

    #[test]
    fn a_six_percent_median_shift_breaches_the_band() {
        let baseline = with_latency(1_000, 2_000, 3_000);
        let candidate = with_latency(1_060, 2_000, 3_000);
        let report = compare(&baseline, &candidate, VarianceBand::STATED).expect("comparable");
        assert!(!report.within_band());
        assert_eq!(report.breaches().len(), 1);
        assert_eq!(report.breaches()[0].percentile, "p50");
    }

    #[test]
    fn the_tail_gets_a_wider_band_than_the_median() {
        // The same ~13 % shift: tolerated on p99, refused on p50.
        let baseline = with_latency(1_000, 2_000, 3_000);

        let tail_shift = with_latency(1_000, 2_000, 3_400);
        let tail = compare(&baseline, &tail_shift, VarianceBand::STATED).expect("comparable");
        assert!(tail.within_band(), "{tail}");

        let median_shift = with_latency(1_133, 2_000, 3_000);
        let median = compare(&baseline, &median_shift, VarianceBand::STATED).expect("comparable");
        assert!(!median.within_band(), "{median}");
        assert_eq!(median.breaches()[0].percentile, "p50");
    }

    #[test]
    fn runs_from_different_commits_are_not_compared_at_all() {
        let baseline = sample_result();
        let mut candidate = sample_result();
        candidate.repository.commit = "b".repeat(40);
        let error = compare(&baseline, &candidate, VarianceBand::STATED)
            .expect_err("different commits are different benchmarks");
        assert_eq!(error.field, "repository.commit");
    }

    #[test]
    fn runs_from_different_cache_states_are_not_compared_at_all() {
        let baseline = sample_result();
        let mut candidate = sample_result();
        candidate.cache_state = CacheState::Cold;
        let error = compare(&baseline, &candidate, VarianceBand::STATED)
            .expect_err("cold and warm are different benchmarks");
        assert_eq!(error.field, "cache_state");
    }

    #[test]
    fn a_zero_baseline_never_looks_like_agreement() {
        let baseline = with_latency(0, 0, 0);
        let candidate = with_latency(0, 1, 0);
        let report = compare(&baseline, &candidate, VarianceBand::STATED).expect("comparable");
        assert!(!report.within_band());
    }
}
