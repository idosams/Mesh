//! Percentile and summary statistics over raw sample durations.
//!
//! Integer arithmetic throughout, and the nearest-rank percentile definition
//! rather than an interpolating one. Both choices are about auditability: a
//! third party re-deriving p99 from the `samples_ns` array in a published row
//! must land on the same integer we published, on any machine, in any language.
//! Interpolation and float accumulation both break that property.

/// Nearest-rank percentile over an already-sorted slice.
///
/// `permille` is tenths of a percent (`500` = p50, `990` = p99), so the caller
/// never passes a float. The rank is `ceil(permille * n / 1000)`, clamped to
/// `1..=n`; `None` for an empty slice, because a percentile of nothing is not
/// zero — it is absent, and the schema refuses to publish an absent number.
pub fn percentile_ns(sorted: &[u64], permille: u32) -> Option<u64> {
    if sorted.is_empty() {
        return None;
    }
    let len = sorted.len() as u64;
    let rank = (u64::from(permille) * len).div_ceil(1000).clamp(1, len);
    // `rank` is in `1..=len`, so the index is in range.
    sorted.get((rank - 1) as usize).copied()
}

/// The arithmetic mean, truncated towards zero.
///
/// Accumulated in `u128` so a long run of large samples cannot wrap.
pub fn mean_ns(samples: &[u64]) -> Option<u64> {
    if samples.is_empty() {
        return None;
    }
    let total: u128 = samples.iter().map(|value| u128::from(*value)).sum();
    u64::try_from(total / samples.len() as u128).ok()
}

/// Returns a sorted copy of `samples` — the input is never mutated in place.
pub fn sorted_copy(samples: &[u64]) -> Vec<u64> {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    sorted
}

/// The permille constant for p50.
pub const P50: u32 = 500;
/// The permille constant for p95.
pub const P95: u32 = 950;
/// The permille constant for p99.
pub const P99: u32 = 990;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_use_nearest_rank() {
        let sorted: Vec<u64> = (1..=100).collect();
        assert_eq!(percentile_ns(&sorted, P50), Some(50));
        assert_eq!(percentile_ns(&sorted, P95), Some(95));
        assert_eq!(percentile_ns(&sorted, P99), Some(99));
        assert_eq!(percentile_ns(&sorted, 1000), Some(100));
    }

    #[test]
    fn a_single_sample_is_every_percentile() {
        assert_eq!(percentile_ns(&[7], P50), Some(7));
        assert_eq!(percentile_ns(&[7], P99), Some(7));
    }

    #[test]
    fn an_empty_slice_has_no_percentile() {
        assert_eq!(percentile_ns(&[], P50), None);
        assert_eq!(mean_ns(&[]), None);
    }

    #[test]
    fn percentiles_are_order_independent_after_sorting() {
        let unsorted = [9, 1, 8, 2, 7, 3, 6, 4, 5, 10];
        let sorted = sorted_copy(&unsorted);
        assert_eq!(percentile_ns(&sorted, P50), Some(5));
        assert_eq!(unsorted[0], 9, "sorting must not touch the caller's data");
    }

    #[test]
    fn mean_does_not_overflow_on_large_samples() {
        let samples = [u64::MAX, u64::MAX];
        assert_eq!(mean_ns(&samples), Some(u64::MAX));
    }

    #[test]
    fn mean_truncates_rather_than_rounds() {
        assert_eq!(mean_ns(&[1, 2]), Some(1));
    }
}
