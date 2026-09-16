//! The chunking parameters, as configuration rather than as constants.
//!
//! Plan §6.2 names four numbers and then says, in the same breath, *"These values are experiment
//! defaults, not a fixed protocol commitment."* This module is what makes that sentence true in
//! code: every number a boundary depends on arrives through a [`ChunkingConfig`] value, no call
//! site in this crate reads a constant, and the two named presets below are ordinary values a
//! caller can ignore entirely.
//!
//! # Which default, and why there are two
//!
//! [`ChunkingConfig::plan_defaults`] is plan §6.2 exactly — 1 MiB whole-file threshold, 64 KiB
//! minimum, 256 KiB average, 1 MiB maximum. It is kept so the plan's numbers stay runnable and so
//! the benchmark has something to beat.
//!
//! [`ChunkingConfig::default`] is **not** those numbers, and the difference is measured rather
//! than argued. `cargo bench -p mesh-chunking --bench chunking` runs ten policies over a
//! source-tree corpus, a large-binary corpus and the exact comparison
//! `benchmarks/budgets/storage.md` §6 lost to Git on, and reports both what each costs to store and
//! what a one-byte edit costs to replicate. The plan's parameters put every file under 1 MiB into
//! the whole-file policy, which is nearly every file in a source tree, so under them a one-byte
//! edit re-transfers the whole file. The task contract's own failure clause demands exactly this:
//! *"If the chosen parameters lose to whole-file on the real corpus, change the parameters and
//! republish the benchmark."*
//!
//! The chunk-policy ADR that is supposed to cite these measurements is **not written here**:
//! `docs/adr/**` is outside this task's allowed paths. The benchmark is the citable artifact until
//! it is.
//!
//! # What the numbers mean
//!
//! * `whole_file_threshold` — a file of this many bytes **or fewer** is one object, named by the
//!   digest of the whole file. Content-defined chunking above it.
//! * `min_size` — no chunk is shorter than this, except the last chunk of a file.
//! * `average_size` — the target mean. It sets the cut mask, so it must be a power of two.
//! * `max_size` — a hard cut, taken whether or not the content agreed. It bounds both the memory a
//!   streaming chunker holds and the worst-case cost of a single-chunk retransfer.
//! * `normalization` — FastCDC's normalized chunking level. The first `average_size` bytes of a
//!   chunk are judged by a mask `normalization` bits *harder* than the target, the remainder by one
//!   `normalization` bits *easier*, which pulls the chunk-size distribution towards the average
//!   instead of the exponential distribution a single mask produces.

use core::fmt;

/// One kibibyte, spelled once.
const KIB: usize = 1024;

/// One mebibyte, spelled once.
const MIB: usize = 1024 * KIB;

/// The parameters a chunk boundary is a function of.
///
/// Construct with [`ChunkingConfig::new`], which rejects every combination the chunker cannot
/// honour, or take one of the two presets. The fields are private precisely so that no value of
/// this type can exist without having passed that check — a chunker holding a validated config
/// needs no defensive branches, and there is no partially-valid state to test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkingConfig {
    whole_file_threshold: u64,
    min_size: usize,
    average_size: usize,
    max_size: usize,
    normalization: u32,
}

impl ChunkingConfig {
    /// The parameters exactly as plan §6.2 states them.
    ///
    /// Files of 1 MiB or fewer are one object; above that, content-defined chunking with a 64 KiB
    /// minimum, a 256 KiB average and a 1 MiB maximum.
    ///
    /// These are the plan's *experiment* defaults. They are not [`ChunkingConfig::default`], and
    /// the benchmark that separates the two is `cargo bench -p mesh-chunking --bench chunking`.
    #[must_use]
    pub const fn plan_defaults() -> Self {
        Self {
            whole_file_threshold: MIB as u64,
            min_size: 64 * KIB,
            average_size: 256 * KIB,
            max_size: MIB,
            normalization: DEFAULT_NORMALIZATION,
        }
    }

    /// Chunking with no whole-file policy at all: every file is cut by content.
    ///
    /// Not a recommended production setting — it is the control arm the benchmark needs in order
    /// to separate "the threshold helped" from "the chunk sizes helped", and it is public because
    /// a benchmark that has to reach into private state is a benchmark of the wrong thing.
    ///
    /// # Errors
    ///
    /// [`ConfigError`] under the same rules as [`ChunkingConfig::new`].
    pub const fn always_chunked(
        min_size: usize,
        average_size: usize,
        max_size: usize,
    ) -> Result<Self, ConfigError> {
        Self::new(0, min_size, average_size, max_size, DEFAULT_NORMALIZATION)
    }

    /// Parameters that are checked rather than trusted.
    ///
    /// # Errors
    ///
    /// [`ConfigError`], naming the one thing that is wrong:
    ///
    /// * `average_size` is not a power of two, or is outside `[64, 1 GiB]` — the cut mask is
    ///   derived from its base-2 logarithm, so a non-power-of-two has no mask to derive;
    /// * `min_size` is zero, or exceeds `average_size`;
    /// * `max_size` is below `average_size`;
    /// * `normalization` would push either mask outside 1..=63 bits, which would make one of the
    ///   two stages either never cut or always cut.
    pub const fn new(
        whole_file_threshold: u64,
        min_size: usize,
        average_size: usize,
        max_size: usize,
        normalization: u32,
    ) -> Result<Self, ConfigError> {
        if average_size < MIN_AVERAGE_SIZE || average_size > MAX_AVERAGE_SIZE {
            return Err(ConfigError::AverageOutOfRange {
                found: average_size,
            });
        }
        if !average_size.is_power_of_two() {
            return Err(ConfigError::AverageNotPowerOfTwo {
                found: average_size,
            });
        }
        if min_size == 0 {
            return Err(ConfigError::MinimumIsZero);
        }
        if min_size > average_size {
            return Err(ConfigError::MinimumAboveAverage {
                min_size,
                average_size,
            });
        }
        if max_size < average_size {
            return Err(ConfigError::MaximumBelowAverage {
                max_size,
                average_size,
            });
        }
        let bits = average_size.trailing_zeros();
        if bits + normalization > MAX_MASK_BITS || normalization >= bits {
            return Err(ConfigError::NormalizationOutOfRange {
                normalization,
                bits,
            });
        }
        Ok(Self {
            whole_file_threshold,
            min_size,
            average_size,
            max_size,
            normalization,
        })
    }

    /// A file of this many bytes or fewer is stored as one object.
    #[must_use]
    pub const fn whole_file_threshold(&self) -> u64 {
        self.whole_file_threshold
    }

    /// The shortest chunk the content-defined cut will produce, except at end of file.
    #[must_use]
    pub const fn min_size(&self) -> usize {
        self.min_size
    }

    /// The target mean chunk size.
    #[must_use]
    pub const fn average_size(&self) -> usize {
        self.average_size
    }

    /// The hard cut, taken whether or not the content agreed.
    #[must_use]
    pub const fn max_size(&self) -> usize {
        self.max_size
    }

    /// FastCDC's normalization level.
    #[must_use]
    pub const fn normalization(&self) -> u32 {
        self.normalization
    }

    /// The strict mask, applied while the chunk is still shorter than the average.
    #[must_use]
    pub(crate) const fn strict_mask(&self) -> u64 {
        mask(self.average_size.trailing_zeros() + self.normalization)
    }

    /// The permissive mask, applied once the chunk has passed the average.
    #[must_use]
    pub(crate) const fn permissive_mask(&self) -> u64 {
        mask(self.average_size.trailing_zeros() - self.normalization)
    }

    /// The most bytes a streaming chunker can be holding at once, ignoring the caller's own push
    /// size: it must buffer a whole file up to the threshold before it knows the file is small,
    /// and it must buffer a maximum-length chunk before it can force a cut.
    #[must_use]
    pub fn peak_buffered_bytes(&self) -> u64 {
        let threshold_headroom = self.whole_file_threshold.saturating_add(1);
        threshold_headroom.max(self.max_size as u64)
    }
}

impl Default for ChunkingConfig {
    /// The measured default: a 1 KiB whole-file threshold, a 4 KiB average, cut between 1 KiB and
    /// 32 KiB.
    ///
    /// **Derived from a measurement, and the first candidate was rejected by it.** An 8 KiB
    /// average is the obvious choice and it is wrong for this workload: at 8 KiB the benchmark's
    /// reproduction of `benchmarks/budgets/storage.md` §6 row 3 — one byte edited in an 8,895-byte
    /// file — comes back at 8,962 bytes, *identical to storing the whole file again*, because a
    /// file of that size usually offers no cut point at all before end of file. At 4 KiB the same
    /// row is 1,949 bytes. The number that motivated this crate does not move until the average
    /// drops below the median source file, and no amount of reasoning about "typical" chunk sizes
    /// would have said so.
    ///
    /// The cost is stated with the win: 4 KiB chunks mean roughly twice as many chunks as 8 KiB,
    /// and every chunk costs 67 bytes of `mesh-cas` arrival journal. On the benchmark's source-tree
    /// corpus that is 1.46 % of content, against `mesh-cas`'
    /// `BUDGET_CAS_AMPLIFICATION_PER_MILLE` of 1010 — which was set when a file was one chunk.
    /// **Nothing wires this crate into the store yet, so no budget moves today**, but the task that
    /// does must re-derive that number with its own measurement rather than discover it.
    ///
    /// Reproduce the whole sweep with `cargo bench -p mesh-chunking --bench chunking`.
    fn default() -> Self {
        match Self::new(
            DEFAULT_WHOLE_FILE_THRESHOLD,
            DEFAULT_MIN_SIZE,
            DEFAULT_AVERAGE_SIZE,
            DEFAULT_MAX_SIZE,
            DEFAULT_NORMALIZATION,
        ) {
            Ok(config) => config,
            // Unreachable: the five constants are checked by `default_is_constructible` in this
            // module's tests, so a change that breaks them fails the suite rather than reaching
            // here. Panicking rather than substituting a silent fallback is the point — a caller
            // that asked for the measured default must never be handed different parameters.
            Err(error) => panic!("the measured defaults are not a valid configuration: {error:?}"),
        }
    }
}

/// The whole-file threshold of [`ChunkingConfig::default`].
const DEFAULT_WHOLE_FILE_THRESHOLD: u64 = KIB as u64;

/// The minimum chunk size of [`ChunkingConfig::default`].
const DEFAULT_MIN_SIZE: usize = KIB;

/// The average chunk size of [`ChunkingConfig::default`].
const DEFAULT_AVERAGE_SIZE: usize = 4 * KIB;

/// The maximum chunk size of [`ChunkingConfig::default`].
const DEFAULT_MAX_SIZE: usize = 32 * KIB;

/// The normalization level both presets use: FastCDC's own recommendation, and the one the
/// benchmark measured against levels 0 and 1.
const DEFAULT_NORMALIZATION: u32 = 2;

/// The smallest average a mask can be derived for without the strict stage cutting on nearly every
/// byte.
const MIN_AVERAGE_SIZE: usize = 64;

/// The largest average this crate accepts. Above a gibibyte the mask arithmetic still works and
/// the parameter has stopped meaning anything.
const MAX_AVERAGE_SIZE: usize = 1024 * MIB;

/// The widest mask, in bits. The rolling hash is 64 bits and a 64-bit mask never matches.
const MAX_MASK_BITS: u32 = 63;

/// `bits` low bits set.
const fn mask(bits: u32) -> u64 {
    if bits >= 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    }
}

/// Why a set of parameters was rejected.
///
/// Every variant carries the offending value, because "invalid chunking configuration" tells a
/// caller nothing it can act on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigError {
    /// `average_size` was outside the accepted range.
    AverageOutOfRange {
        /// The rejected average.
        found: usize,
    },
    /// `average_size` was not a power of two, so no cut mask can be derived from it.
    AverageNotPowerOfTwo {
        /// The rejected average.
        found: usize,
    },
    /// `min_size` was zero, which would admit empty chunks and never terminate.
    MinimumIsZero,
    /// `min_size` exceeded `average_size`.
    MinimumAboveAverage {
        /// The rejected minimum.
        min_size: usize,
        /// The average it exceeded.
        average_size: usize,
    },
    /// `max_size` was below `average_size`, so the hard cut would pre-empt every content cut.
    MaximumBelowAverage {
        /// The rejected maximum.
        max_size: usize,
        /// The average it fell below.
        average_size: usize,
    },
    /// `normalization` would push a mask outside the usable width.
    NormalizationOutOfRange {
        /// The rejected level.
        normalization: u32,
        /// The base-2 logarithm of the average, which the two masks are derived from.
        bits: u32,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AverageOutOfRange { found } => write!(
                formatter,
                "the average chunk size is {found} bytes; it must be between {MIN_AVERAGE_SIZE} \
                 and {MAX_AVERAGE_SIZE}"
            ),
            Self::AverageNotPowerOfTwo { found } => write!(
                formatter,
                "the average chunk size is {found} bytes; it must be a power of two, because the \
                 cut mask is its base-2 logarithm"
            ),
            Self::MinimumIsZero => formatter.write_str(
                "the minimum chunk size is zero, which would admit empty chunks and never advance",
            ),
            Self::MinimumAboveAverage {
                min_size,
                average_size,
            } => write!(
                formatter,
                "the minimum chunk size {min_size} exceeds the average {average_size}, so no chunk \
                 could ever reach the average"
            ),
            Self::MaximumBelowAverage {
                max_size,
                average_size,
            } => write!(
                formatter,
                "the maximum chunk size {max_size} is below the average {average_size}, so the \
                 hard cut would pre-empt every content-defined cut"
            ),
            Self::NormalizationOutOfRange {
                normalization,
                bits,
            } => write!(
                formatter,
                "a normalization level of {normalization} against a {bits}-bit average puts one of \
                 the two masks outside 1..={MAX_MASK_BITS} bits, where it would either never cut \
                 or always cut"
            ),
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_constructible() {
        let config = ChunkingConfig::default();
        assert_eq!(config.whole_file_threshold(), DEFAULT_WHOLE_FILE_THRESHOLD);
        assert_eq!(config.min_size(), DEFAULT_MIN_SIZE);
        assert_eq!(config.average_size(), DEFAULT_AVERAGE_SIZE);
        assert_eq!(config.max_size(), DEFAULT_MAX_SIZE);
    }

    #[test]
    fn the_plan_preset_is_the_plan_numbers() {
        let config = ChunkingConfig::plan_defaults();
        assert_eq!(config.whole_file_threshold(), 1024 * 1024);
        assert_eq!(config.min_size(), 64 * 1024);
        assert_eq!(config.average_size(), 256 * 1024);
        assert_eq!(config.max_size(), 1024 * 1024);
    }

    #[test]
    fn the_plan_preset_is_valid_under_the_same_checks() {
        let plan = ChunkingConfig::plan_defaults();
        let rebuilt = ChunkingConfig::new(
            plan.whole_file_threshold(),
            plan.min_size(),
            plan.average_size(),
            plan.max_size(),
            plan.normalization(),
        );
        assert_eq!(rebuilt, Ok(plan));
    }

    #[test]
    fn a_non_power_of_two_average_is_rejected() {
        assert_eq!(
            ChunkingConfig::new(0, 1024, 3000, 8192, 2),
            Err(ConfigError::AverageNotPowerOfTwo { found: 3000 })
        );
    }

    #[test]
    fn a_minimum_above_the_average_is_rejected() {
        assert_eq!(
            ChunkingConfig::new(0, 16384, 8192, 65536, 2),
            Err(ConfigError::MinimumAboveAverage {
                min_size: 16384,
                average_size: 8192,
            })
        );
    }

    #[test]
    fn a_maximum_below_the_average_is_rejected() {
        assert_eq!(
            ChunkingConfig::new(0, 1024, 8192, 4096, 2),
            Err(ConfigError::MaximumBelowAverage {
                max_size: 4096,
                average_size: 8192,
            })
        );
    }

    #[test]
    fn a_zero_minimum_is_rejected() {
        assert_eq!(
            ChunkingConfig::new(0, 0, 8192, 65536, 2),
            Err(ConfigError::MinimumIsZero)
        );
    }

    #[test]
    fn a_normalization_that_would_empty_the_strict_mask_is_rejected() {
        // 8 KiB is 13 bits; normalization 13 would make the permissive mask zero bits wide, which
        // cuts on every byte. Stated against an explicit 8 KiB rather than the default so the
        // arithmetic in this comment does not go stale when the default moves.
        assert!(matches!(
            ChunkingConfig::new(0, 1024, 8192, 65536, 13),
            Err(ConfigError::NormalizationOutOfRange { .. })
        ));
    }

    #[test]
    fn an_average_below_the_floor_is_rejected() {
        assert_eq!(
            ChunkingConfig::new(0, 1, 32, 64, 1),
            Err(ConfigError::AverageOutOfRange { found: 32 })
        );
    }

    #[test]
    fn the_two_masks_straddle_the_average() {
        let config = ChunkingConfig::default();
        assert!(
            config.strict_mask() > config.permissive_mask(),
            "the strict mask must be the wider of the two, or normalized chunking is inverted"
        );
        assert_eq!(config.strict_mask().count_ones(), 12 + 2);
        assert_eq!(config.permissive_mask().count_ones(), 12 - 2);
    }

    #[test]
    fn the_error_text_names_the_offending_value() {
        let error = ChunkingConfig::new(0, 1024, 3000, 8192, 2).unwrap_err();
        assert!(error.to_string().contains("3000"));
    }

    #[test]
    fn peak_buffered_bytes_covers_both_reasons_to_buffer() {
        let small_threshold = ChunkingConfig::default();
        assert_eq!(
            small_threshold.peak_buffered_bytes(),
            small_threshold.max_size() as u64
        );

        let large_threshold = ChunkingConfig::new(4 * MIB as u64, 2048, 8192, 65536, 2).unwrap();
        assert_eq!(large_threshold.peak_buffered_bytes(), 4 * MIB as u64 + 1);
    }
}
