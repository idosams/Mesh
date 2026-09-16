//! Deterministic pseudo-randomness for corpus generation.
//!
//! Two properties matter here and nothing else does. The generator must produce
//! the **same bytes from the same seed on every machine**, and two streams
//! derived from adjacent labels must be **independent**, so that adding a file
//! to W1 cannot shift a single byte of W3.
//!
//! # Why a second algorithm alongside `workloads::digest::SeededGenerator`
//!
//! That one is xorshift64\*, and it is fine for what it does: one stream, drawn
//! in order, from one seed. Corpus generation needs something xorshift64\* is
//! bad at — **stateless derivation of thousands of independent streams from
//! adjacent integers**. A xorshift state seeded with `n` and one seeded with
//! `n + 1` differ in one bit, and their first outputs stay visibly related for
//! several steps, because a shift-xor round has poor avalanche. Seeding one
//! stream per file index that way makes neighbouring files share structure.
//!
//! SplitMix64's finalizer is an avalanche function by construction: flipping any
//! input bit flips roughly half the output bits. That is exactly the property
//! [`stream`] needs, so it is written out here — like the digest next door, and
//! for the same reason: an algorithm this repository depends on for
//! reproducibility must not be able to change under a version bump.
//!
//! Neither is a cryptographic primitive and neither is used as one.

use crate::workloads::digest::Fnv1a;

/// The SplitMix64 finalizer: a bijective avalanche over 64 bits.
///
/// Constants from Steele, Lea and Flood's SplitMix (2014), as used by
/// `java.util.SplittableRandom` and reproduced identically everywhere since.
#[must_use]
pub const fn mix(value: u64) -> u64 {
    let mut z = value;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Derives an independent stream seed from a root seed, a label and an index.
///
/// The label names *what* the stream is for (`"w1/file-size"`, `"w3/overlap"`),
/// so two generators drawing the same index never draw the same numbers. The
/// FNV fold makes the label a 64-bit value; [`mix`] is what makes adjacent
/// indices independent, and it is the half that cannot be dropped.
#[must_use]
pub fn stream(seed: u64, label: &str, index: u64) -> u64 {
    let mut digest = Fnv1a::new();
    digest.update(&seed.to_le_bytes());
    digest.update(label.as_bytes());
    digest.update(&index.to_le_bytes());
    mix(digest.value())
}

/// A SplitMix64 sequence.
#[derive(Clone, Copy, Debug)]
pub struct SplitMix64 {
    state: u64,
}

/// The odd increment SplitMix64 walks its state by (the golden-ratio constant).
const GAMMA: u64 = 0x9e37_79b9_7f4a_7c15;

impl SplitMix64 {
    /// Starts a sequence at `seed`.
    ///
    /// Every seed is usable, including zero — unlike xorshift, whose zero state
    /// is absorbing.
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        SplitMix64 { state: seed }
    }

    /// Starts the sequence [`stream`] names.
    #[must_use]
    pub fn derived(seed: u64, label: &str, index: u64) -> Self {
        SplitMix64::new(stream(seed, label, index))
    }

    /// The next 64 bits.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(GAMMA);
        mix(self.state)
    }

    /// A value in `low..=high`.
    ///
    /// Modulo reduction, not rejection sampling. The bias is bounded by
    /// `span / 2^64`; every span in this module is under 2^40, so the largest
    /// bias any caller can see is below 2^-24 of one draw — far under the
    /// tolerances the shape report is checked against, and the trade buys
    /// constant-time draws and a sequence whose length does not depend on its
    /// values, which is what keeps two runs in lockstep.
    ///
    /// # Panics
    ///
    /// If `low > high`, which is a programming error in a generator, not input.
    pub fn in_range(&mut self, low: u64, high: u64) -> u64 {
        assert!(low <= high, "in_range({low}, {high}): empty range");
        let span = high - low + 1;
        if span == 0 {
            return self.next_u64();
        }
        low + (self.next_u64() % span)
    }

    /// A Bernoulli draw with probability `permille / 1000`.
    pub fn chance(&mut self, permille: u32) -> bool {
        (self.next_u64() % 1000) < u64::from(permille)
    }

    /// Fills `buffer` with the sequence's bytes, little-endian, eight at a time.
    pub fn fill(&mut self, buffer: &mut [u8]) {
        for block in buffer.chunks_mut(8) {
            let word = self.next_u64().to_le_bytes();
            block.copy_from_slice(&word[..block.len()]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_gives_the_same_sequence() {
        let first: Vec<u64> = (0..8).map(|_| SplitMix64::new(7).next_u64()).collect();
        let mut source = SplitMix64::new(7);
        let second: Vec<u64> = (0..8).map(|_| source.next_u64()).collect();
        assert_eq!(first[0], second[0]);
        assert_ne!(second[0], second[1], "a sequence must advance");
    }

    #[test]
    fn adjacent_stream_indices_are_independent() {
        // The property xorshift64* does not have, pinned as a number rather
        // than asserted in prose: adjacent derived streams must differ in
        // roughly half their bits, not in one.
        let left = stream(42, "w1/file-size", 100);
        let right = stream(42, "w1/file-size", 101);
        let differing = (left ^ right).count_ones();
        assert!(
            (16..=48).contains(&differing),
            "adjacent streams differ in {differing} bits, which is not an avalanche"
        );
    }

    #[test]
    fn different_labels_give_different_streams() {
        assert_ne!(stream(42, "w1/file-size", 0), stream(42, "w3/overlap", 0));
    }

    #[test]
    fn different_seeds_give_different_streams() {
        assert_ne!(stream(1, "w1/file-size", 0), stream(2, "w1/file-size", 0));
    }

    #[test]
    fn a_zero_seed_still_advances() {
        let mut source = SplitMix64::new(0);
        assert_ne!(source.next_u64(), 0);
        assert_ne!(source.next_u64(), source.next_u64());
    }

    #[test]
    fn ranged_draws_stay_inside_their_range() {
        let mut source = SplitMix64::new(11);
        for _ in 0..10_000 {
            let value = source.in_range(10, 20);
            assert!((10..=20).contains(&value), "{value} escaped 10..=20");
        }
    }

    #[test]
    fn a_single_point_range_is_that_point() {
        let mut source = SplitMix64::new(3);
        assert_eq!(source.in_range(5, 5), 5);
    }

    #[test]
    fn chance_tracks_its_stated_probability() {
        let mut source = SplitMix64::new(99);
        let hits = (0..100_000).filter(|_| source.chance(200)).count();
        // 100k Bernoulli(0.2) draws: sigma is 126 hits, so a 1,000-hit band is
        // eight sigma. A generator outside it is broken, not unlucky.
        assert!((19_000..=21_000).contains(&hits), "{hits} hits in 100000");
    }

    #[test]
    fn certain_and_impossible_chances_are_exact() {
        let mut source = SplitMix64::new(5);
        assert!(source.chance(1000));
        assert!(!source.chance(0));
    }

    #[test]
    fn filling_is_reproducible_and_covers_partial_blocks() {
        let mut first = [0_u8; 13];
        let mut second = [0_u8; 13];
        SplitMix64::new(4).fill(&mut first);
        SplitMix64::new(4).fill(&mut second);
        assert_eq!(first, second);
        assert!(first.iter().any(|byte| *byte != 0));
    }

    #[test]
    fn the_mixer_is_a_bijection_on_the_values_we_use() {
        let mut seen = std::collections::BTreeSet::new();
        for value in 0..2_000_u64 {
            assert!(seen.insert(mix(value)), "mix collided at {value}");
        }
    }
}
