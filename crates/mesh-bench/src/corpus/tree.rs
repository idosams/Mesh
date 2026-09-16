//! The pieces every file-tree workload shares: paths, size distributions and
//! exact-fraction selection.
//!
//! Three problems come up in W1, W2 and W4 alike, and each has one wrong answer
//! that is tempting:
//!
//! 1. **Paths.** Flat names make directory enumeration meaningless and a random
//!    depth makes the tree different every seed. [`path_for`] builds a fanned
//!    tree from the index alone, so the directory structure is a property of the
//!    scale rather than of the seed.
//! 2. **Sizes.** Drawing sizes from a distribution gives a plausible shape and a
//!    total that misses the workload's stated size. [`size_ladder`] draws the
//!    shape, then rescales so the total is *exactly* the stated figure — which
//!    is what makes "1.5 GB" a fact in the shape report rather than an
//!    aspiration.
//! 3. **Sparse access.** "5% of files" drawn per-file with probability 0.05 is
//!    5% on average and something else on the day. [`select_exactly`] is Knuth's
//!    Algorithm S: streaming, O(1) memory, and exactly k of n every time.

use super::plan::ContentKind;
use super::rng::SplitMix64;

/// How many entries a generated directory holds before the tree fans out.
///
/// 64: deep enough that a million files is four levels rather than one, shallow
/// enough that no directory listing is pathological on any filesystem.
pub const FANOUT: usize = 64;

/// A band in a size distribution: what share of the files, and how large.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SizeBand {
    /// Share of the file count, in parts per thousand.
    pub share_permille: u32,
    /// Smallest size drawn in this band, in bytes.
    pub min_bytes: u64,
    /// Largest size drawn in this band, in bytes.
    pub max_bytes: u64,
    /// The byte profile files in this band carry.
    pub kind: ContentKind,
    /// The extension files in this band carry, without the dot.
    pub extension: &'static str,
}

/// Which band an index falls in.
///
/// Assignment is by position, not by draw, so the band counts are exact and a
/// reader can predict them from the parameters. `bands` must not be empty.
#[must_use]
pub fn band_of(index: usize, count: usize, bands: &[SizeBand]) -> usize {
    debug_assert!(!bands.is_empty(), "a size ladder needs at least one band");
    if count == 0 {
        return 0;
    }
    let position = (index as u128 * 1000) / count as u128;
    let mut cumulative = 0_u128;
    for (ordinal, band) in bands.iter().enumerate() {
        cumulative += u128::from(band.share_permille);
        if position < cumulative {
            return ordinal;
        }
    }
    bands.len() - 1
}

/// Draws `count` sizes from `bands`, then rescales them to total exactly
/// `target_bytes`.
///
/// The rescale is integer arithmetic with the remainder handed out one byte at
/// a time to the lowest indices, so the total is exact and the result does not
/// depend on floating point. Every file keeps at least one byte: a zero-byte
/// file in a storage benchmark is a directory entry pretending to be data.
///
/// With `target_bytes` of zero the sizes are returned as drawn, which is what a
/// generator wants when it has no stated total.
#[must_use]
pub fn size_ladder(
    seed: u64,
    label: &str,
    count: usize,
    bands: &[SizeBand],
    target_bytes: u64,
) -> Vec<u64> {
    let mut raw: Vec<u64> = Vec::with_capacity(count);
    for index in 0..count {
        let band = &bands[band_of(index, count, bands)];
        let mut source = SplitMix64::derived(seed, label, index as u64);
        raw.push(source.in_range(band.min_bytes, band.max_bytes));
    }
    if target_bytes == 0 || raw.is_empty() {
        return raw;
    }
    rescale(raw, target_bytes)
}

/// Scales `sizes` so they sum to exactly `total`, keeping every entry positive.
fn rescale(sizes: Vec<u64>, total: u64) -> Vec<u64> {
    let count = sizes.len() as u64;
    let sum: u128 = sizes.iter().map(|size| u128::from(*size)).sum();
    if sum == 0 {
        return spread_evenly(sizes.len(), total);
    }
    // A target smaller than one byte per file cannot be met without empty
    // files; give every file its byte and report the overshoot through the
    // shape fact rather than silently producing a corpus of holes.
    if total < count {
        return vec![1; sizes.len()];
    }
    let mut scaled: Vec<u64> = sizes
        .iter()
        .map(|size| {
            let value = (u128::from(*size) * u128::from(total)) / sum;
            u64::try_from(value).unwrap_or(u64::MAX).max(1)
        })
        .collect();
    balance(&mut scaled, total);
    scaled
}

/// Splits `total` across `count` files as evenly as integers allow.
fn spread_evenly(count: usize, total: u64) -> Vec<u64> {
    if count == 0 {
        return Vec::new();
    }
    let share = (total / count as u64).max(1);
    let mut sizes = vec![share; count];
    balance(&mut sizes, total);
    sizes
}

/// Moves bytes between entries, from the front, until `sizes` sums to exactly
/// `total` — or until nothing more can be moved.
///
/// Two things this has to get right, because a corpus generator that spins or
/// miscounts is worse than one that is approximate:
///
/// * **Every step is accounted for exactly.** The amount added is bounded by the
///   entry's remaining headroom and the amount removed by what it can spare
///   above one byte, so `sum` always tracks the array rather than what the step
///   intended. A `saturating_add` whose saturation went unrecorded would leave
///   the loop believing it had hit the target.
/// * **The shrink loop terminates unconditionally.** `untouched` counts
///   consecutive entries with nothing to spare; when it reaches the length, one
///   whole pass has moved nothing and no further pass ever will. That is the
///   guard, not a scan of the array on every iteration.
fn balance(sizes: &mut [u64], total: u64) {
    if sizes.is_empty() {
        return;
    }
    let target = u128::from(total);
    let mut sum: u128 = sizes.iter().map(|size| u128::from(*size)).sum();

    let mut index = 0;
    let mut untouched = 0;
    while sum < target && untouched < sizes.len() {
        let headroom = u128::from(u64::MAX - sizes[index]);
        let step = (target - sum).min(headroom);
        if step == 0 {
            untouched += 1;
        } else {
            untouched = 0;
            sizes[index] += u64::try_from(step).unwrap_or(0);
            sum += step;
        }
        index = (index + 1) % sizes.len();
    }

    index = 0;
    untouched = 0;
    while sum > target && untouched < sizes.len() {
        // `saturating_sub`, not `- 1`: every caller keeps entries positive, but
        // a zero reaching here would wrap to `u64::MAX` in a release build and
        // hand the loop an entry with limitless spare capacity.
        let spare = u128::from(sizes[index].saturating_sub(1));
        let step = (sum - target).min(spare);
        if step == 0 {
            untouched += 1;
        } else {
            untouched = 0;
            sizes[index] -= u64::try_from(step).unwrap_or(0);
            sum -= step;
        }
        index = (index + 1) % sizes.len();
    }
}

/// Builds the path of file `index`, in a tree that fans out at [`FANOUT`].
///
/// The name carries the band's extension so that a corpus is browsable and a
/// tool that keys off extensions sees what it expects.
#[must_use]
pub fn path_for(root: &str, index: usize, extension: &str) -> String {
    let mut segments = Vec::new();
    let mut level = index / FANOUT;
    while level > 0 {
        segments.push(level % FANOUT);
        level /= FANOUT;
    }
    segments.reverse();
    let mut path = String::from(root);
    for segment in segments {
        path.push_str(&format!("/d{segment:02}"));
    }
    if extension.is_empty() {
        path.push_str(&format!("/f{index:07}"));
    } else {
        path.push_str(&format!("/f{index:07}.{extension}"));
    }
    path
}

/// Knuth's Algorithm S: selects exactly `wanted` of `total` indices.
///
/// Streaming and stateless in memory — it decides each index as it passes it,
/// using the exact conditional probability `remaining_wanted /
/// remaining_total`. That is what makes W2's "only 5% accessed" an exact
/// fraction rather than an expected one, without materialising a permutation of
/// a million entries.
#[derive(Clone, Debug)]
pub struct ExactSelector {
    source: SplitMix64,
    remaining_total: u64,
    remaining_wanted: u64,
}

impl ExactSelector {
    /// Selects `wanted` of the next `total` calls to [`ExactSelector::take`].
    #[must_use]
    pub fn new(seed: u64, label: &str, total: u64, wanted: u64) -> Self {
        ExactSelector {
            source: SplitMix64::derived(seed, label, 0),
            remaining_total: total,
            remaining_wanted: wanted.min(total),
        }
    }

    /// Whether the next index is selected.
    pub fn take(&mut self) -> bool {
        if self.remaining_total == 0 {
            return false;
        }
        // `draw < wanted * SCALE / total` with integer arithmetic, so the
        // decision is identical on every target and every optimisation level.
        let draw = self.source.next_u64() % self.remaining_total;
        let selected = draw < self.remaining_wanted;
        self.remaining_total -= 1;
        if selected {
            self.remaining_wanted -= 1;
        }
        selected
    }

    /// How many selections are still owed.
    #[must_use]
    pub const fn remaining_wanted(&self) -> u64 {
        self.remaining_wanted
    }
}

/// Selects exactly `wanted` of `total` indices, as a vector of booleans.
#[must_use]
pub fn select_exactly(seed: u64, label: &str, total: u64, wanted: u64) -> Vec<bool> {
    let mut selector = ExactSelector::new(seed, label, total, wanted);
    (0..total).map(|_| selector.take()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BANDS: [SizeBand; 2] = [
        SizeBand {
            share_permille: 900,
            min_bytes: 100,
            max_bytes: 200,
            kind: ContentKind::Text,
            extension: "rs",
        },
        SizeBand {
            share_permille: 100,
            min_bytes: 1_000,
            max_bytes: 2_000,
            kind: ContentKind::Binary,
            extension: "bin",
        },
    ];

    #[test]
    fn bands_are_assigned_by_position_so_counts_are_exact() {
        let count = 1_000;
        let in_second = (0..count)
            .filter(|index| band_of(*index, count, &BANDS) == 1)
            .count();
        assert_eq!(in_second, 100, "a 100-permille band of 1000 files is 100");
    }

    #[test]
    fn a_single_band_takes_everything() {
        let bands = [BANDS[0]];
        assert!((0..50).all(|index| band_of(index, 50, &bands) == 0));
    }

    #[test]
    fn an_empty_count_does_not_divide_by_zero() {
        assert_eq!(band_of(0, 0, &BANDS), 0);
    }

    #[test]
    fn balancing_reaches_the_target_from_either_side() {
        for (start, total) in [
            (vec![1_u64, 1, 1], 30_u64),
            (vec![10_u64, 10, 10], 3),
            (vec![5_u64], 5),
            (vec![7_u64, 3], 10),
        ] {
            let mut sizes = start.clone();
            balance(&mut sizes, total);
            assert_eq!(
                sizes.iter().sum::<u64>(),
                total,
                "{start:?} did not balance to {total}"
            );
            assert!(sizes.iter().all(|size| *size >= 1), "{sizes:?} has a hole");
        }
    }

    #[test]
    fn balancing_stops_when_no_entry_can_spare_another_byte() {
        // The termination case: three one-byte entries cannot shrink to one
        // byte in total, so `balance` must stop rather than spin. `rescale`
        // never asks it to, but a loop that only terminates because its caller
        // is careful is a loop that terminates by luck.
        let mut sizes = vec![1_u64, 1, 1];
        balance(&mut sizes, 1);
        assert_eq!(sizes, vec![1, 1, 1]);
    }

    #[test]
    fn balancing_a_zero_entry_does_not_wrap_it_into_the_largest_file_ever_made() {
        let mut sizes = vec![0_u64, 10];
        balance(&mut sizes, 4);
        assert_eq!(sizes.iter().sum::<u64>(), 4);
        assert!(sizes.iter().all(|size| *size <= 10), "{sizes:?} grew");
    }

    #[test]
    fn balancing_an_empty_slice_is_a_no_op() {
        let mut sizes: Vec<u64> = Vec::new();
        balance(&mut sizes, 100);
        assert!(sizes.is_empty());
    }

    #[test]
    fn balancing_never_overflows_a_saturated_entry() {
        // Headroom is what bounds a step, so an entry already at the ceiling
        // is skipped rather than silently counted as having absorbed bytes.
        let mut sizes = vec![u64::MAX, 1];
        balance(&mut sizes, u64::MAX);
        assert_eq!(
            u128::from(sizes[0]) + u128::from(sizes[1]),
            u128::from(u64::MAX)
        );
    }

    #[test]
    fn the_ladder_totals_exactly_its_target() {
        for target in [1_000_u64, 1_000_000, 12_345_678] {
            let sizes = size_ladder(7, "test/ladder", 500, &BANDS, target);
            assert_eq!(sizes.iter().sum::<u64>(), target, "target {target}");
            assert!(sizes.iter().all(|size| *size >= 1), "a file lost its bytes");
        }
    }

    #[test]
    fn the_ladder_is_reproducible() {
        let first = size_ladder(7, "test/ladder", 200, &BANDS, 500_000);
        let second = size_ladder(7, "test/ladder", 200, &BANDS, 500_000);
        assert_eq!(first, second);
    }

    #[test]
    fn a_different_seed_gives_a_different_ladder() {
        let first = size_ladder(7, "test/ladder", 200, &BANDS, 500_000);
        let second = size_ladder(8, "test/ladder", 200, &BANDS, 500_000);
        assert_ne!(first, second);
        assert_eq!(first.iter().sum::<u64>(), second.iter().sum::<u64>());
    }

    #[test]
    fn the_ladder_keeps_the_distribution_it_drew() {
        // The rescale must not flatten the bands: the large band's files should
        // still be an order of magnitude bigger than the small band's.
        let sizes = size_ladder(7, "test/ladder", 1_000, &BANDS, 10_000_000);
        let small: u64 = sizes[..900].iter().sum::<u64>() / 900;
        let large: u64 = sizes[900..].iter().sum::<u64>() / 100;
        assert!(large > small * 4, "small {small}, large {large}");
    }

    #[test]
    fn an_impossible_target_gives_every_file_one_byte() {
        let sizes = size_ladder(7, "test/ladder", 100, &BANDS, 10);
        assert_eq!(sizes.len(), 100);
        assert!(sizes.iter().all(|size| *size == 1));
    }

    #[test]
    fn no_target_leaves_the_drawn_sizes_alone() {
        let sizes = size_ladder(7, "test/ladder", 10, &BANDS, 0);
        assert!(sizes.iter().take(9).all(|size| (100..=200).contains(size)));
    }

    #[test]
    fn paths_are_unique_and_fan_out() {
        let paths: Vec<String> = (0..5_000)
            .map(|index| path_for("root", index, "rs"))
            .collect();
        let unique: std::collections::BTreeSet<&String> = paths.iter().collect();
        assert_eq!(unique.len(), paths.len(), "two files share a path");
        let deepest = paths
            .iter()
            .map(|path| path.matches('/').count())
            .max()
            .expect("paths exist");
        assert!(deepest >= 3, "5000 files should not be one flat directory");
    }

    #[test]
    fn paths_are_relative_and_carry_their_extension() {
        let path = path_for("workspace", 100, "docx");
        assert!(!path.starts_with('/'), "{path} is absolute");
        assert!(path.starts_with("workspace/"), "{path}");
        assert!(path.ends_with(".docx"), "{path}");
    }

    #[test]
    fn a_missing_extension_produces_a_bare_name() {
        assert!(path_for("root", 3, "").ends_with("/f0000003"));
    }

    #[test]
    fn exact_selection_selects_exactly_the_requested_number() {
        for (total, wanted) in [(1_000_u64, 50_u64), (37, 1), (100, 100), (100, 0)] {
            let chosen = select_exactly(3, "test/select", total, wanted);
            assert_eq!(
                chosen.iter().filter(|taken| **taken).count() as u64,
                wanted,
                "total {total}, wanted {wanted}"
            );
        }
    }

    #[test]
    fn exact_selection_is_reproducible() {
        assert_eq!(
            select_exactly(3, "test/select", 200, 10),
            select_exactly(3, "test/select", 200, 10)
        );
    }

    #[test]
    fn exact_selection_is_spread_rather_than_clustered() {
        // Algorithm S must not degenerate into "the first k" or "the last k".
        let chosen = select_exactly(3, "test/select", 1_000, 50);
        let first_half = chosen[..500].iter().filter(|taken| **taken).count();
        assert!(
            (10..=40).contains(&first_half),
            "{first_half} of 50 in the first half"
        );
    }

    #[test]
    fn wanting_more_than_exists_selects_everything() {
        let chosen = select_exactly(3, "test/select", 10, 99);
        assert_eq!(chosen.iter().filter(|taken| **taken).count(), 10);
    }

    #[test]
    fn a_selector_reports_what_it_still_owes() {
        let mut selector = ExactSelector::new(3, "test/select", 10, 4);
        assert_eq!(selector.remaining_wanted(), 4);
        for _ in 0..10 {
            selector.take();
        }
        assert_eq!(selector.remaining_wanted(), 0);
        assert!(!selector.take(), "an exhausted selector selects nothing");
    }
}
