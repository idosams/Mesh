//! A deterministic pseudo-random source, so a corpus is named by a seed rather than by a file.
//!
//! SplitMix64, written out here because this crate may not declare a dev-dependency — the manifest
//! guard in `src/lib.rs` rejects one, and `Cargo.lock` is governance surface a lane escalates
//! rather than writes. That is a real cost and it is stated: this is not a statistically strong
//! generator and no test here depends on it being one. What the tests depend on is that
//! `Rng::new(seed)` produces exactly the same sequence on every machine and every run, so a
//! divergence found at seed 41,987 can be replayed by anybody who has the number.

/// A seeded, reproducible sequence.
pub struct Rng(u64);

impl Rng {
    /// The sequence for this seed.
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x5DEE_CE66_D1B1_4A5D)
    }

    /// The next value.
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value in `0..limit`. `limit` of zero answers zero.
    pub fn below(&mut self, limit: u64) -> u64 {
        if limit == 0 {
            0
        } else {
            self.next_u64() % limit
        }
    }

    /// A byte.
    pub fn byte(&mut self) -> u8 {
        (self.next_u64() & 0xff) as u8
    }

    /// Whether an event with probability `percent` happens.
    pub fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    /// One of `items`, or `None` when there are none.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            None
        } else {
            let at = self.below(items.len() as u64) as usize;
            items.get(at)
        }
    }

    /// Shuffle in place — a Fisher-Yates pass, so every permutation is reachable.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        if items.len() < 2 {
            return;
        }
        for index in (1..items.len()).rev() {
            let other = self.below(index as u64 + 1) as usize;
            items.swap(index, other);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_is_the_same_sequence() {
        let mut one = Rng::new(7);
        let mut other = Rng::new(7);
        for _ in 0..64 {
            assert_eq!(one.next_u64(), other.next_u64());
        }
    }

    #[test]
    fn two_seeds_are_two_sequences() {
        assert_ne!(Rng::new(7).next_u64(), Rng::new(8).next_u64());
    }

    #[test]
    fn a_shuffle_is_a_permutation() {
        let mut items: Vec<u32> = (0..32).collect();
        Rng::new(3).shuffle(&mut items);
        let mut sorted = items.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..32).collect::<Vec<u32>>());
    }

    #[test]
    fn a_shuffle_of_the_same_seed_is_the_same_permutation() {
        let mut one: Vec<u32> = (0..32).collect();
        let mut other: Vec<u32> = (0..32).collect();
        Rng::new(11).shuffle(&mut one);
        Rng::new(11).shuffle(&mut other);
        assert_eq!(one, other);
    }
}
