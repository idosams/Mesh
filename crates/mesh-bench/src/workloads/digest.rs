//! The deterministic data generator and the digest used to verify it.
//!
//! Both are non-cryptographic and deliberately tiny. Their job is to make a
//! workload's *input* reproducible from a seed and its *output* checkable in a
//! few microseconds, so that correctness verification is cheap enough that no
//! one is ever tempted to skip it before timing.

/// A xorshift64\* pseudo-random generator.
///
/// Same seed, same bytes, on every machine and every build — that is the whole
/// requirement, and the reason the generator is written out here rather than
/// taken from a crate whose algorithm could change under a version bump.
#[derive(Clone, Debug)]
pub struct SeededGenerator {
    state: u64,
}

/// The generator's identity, recorded in every row it feeds.
pub const GENERATOR_NAME: &str = "mesh-bench/xorshift64star-blobs";
/// The generator's algorithm version. Bumping it invalidates comparisons.
pub const GENERATOR_VERSION: &str = "1";

impl SeededGenerator {
    /// Starts the generator. Seed 0 is remapped, since xorshift is stuck at 0.
    pub fn new(seed: u64) -> Self {
        SeededGenerator {
            state: if seed == 0 {
                0x9E37_79B9_7F4A_7C15
            } else {
                seed
            },
        }
    }

    /// The next 64 bits.
    pub fn next_u64(&mut self) -> u64 {
        let mut state = self.state;
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        self.state = state;
        state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Generates `blob_count` blobs of `blob_bytes` bytes each.
    pub fn blobs(&mut self, blob_count: usize, blob_bytes: usize) -> Vec<Vec<u8>> {
        (0..blob_count)
            .map(|_| {
                let mut blob = Vec::with_capacity(blob_bytes);
                while blob.len() < blob_bytes {
                    blob.extend_from_slice(&self.next_u64().to_le_bytes());
                }
                blob.truncate(blob_bytes);
                blob
            })
            .collect()
    }
}

/// FNV-1a, 64-bit.
///
/// Not a security primitive and never used as one — it is a cheap equality
/// witness for "did this run produce the same bytes as the reference run".
#[derive(Clone, Copy, Debug)]
pub struct Fnv1a {
    hash: u64,
}

impl Fnv1a {
    /// An empty digest.
    pub fn new() -> Self {
        Fnv1a {
            hash: 0xcbf2_9ce4_8422_2325,
        }
    }

    /// Folds `bytes` in.
    pub fn update(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.hash ^= u64::from(*byte);
            self.hash = self.hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    /// The digest so far, as lowercase hex.
    pub fn finish_hex(&self) -> String {
        format!("fnv1a64:{:016x}", self.hash)
    }

    /// The raw digest so far.
    pub fn value(&self) -> u64 {
        self.hash
    }
}

impl Default for Fnv1a {
    fn default() -> Self {
        Fnv1a::new()
    }
}

/// Digests a slice of blobs in order.
pub fn digest_blobs(blobs: &[Vec<u8>]) -> String {
    let mut digest = Fnv1a::new();
    for blob in blobs {
        digest.update(blob);
    }
    digest.finish_hex()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_generator_is_reproducible_from_its_seed() {
        let first = SeededGenerator::new(42).blobs(4, 64);
        let second = SeededGenerator::new(42).blobs(4, 64);
        assert_eq!(first, second);
    }

    #[test]
    fn different_seeds_produce_different_data() {
        let first = SeededGenerator::new(1).blobs(2, 32);
        let second = SeededGenerator::new(2).blobs(2, 32);
        assert_ne!(first, second);
    }

    #[test]
    fn blobs_have_the_requested_shape() {
        let blobs = SeededGenerator::new(7).blobs(3, 10);
        assert_eq!(blobs.len(), 3);
        assert!(blobs.iter().all(|blob| blob.len() == 10));
    }

    #[test]
    fn a_zero_seed_still_generates() {
        let blobs = SeededGenerator::new(0).blobs(1, 16);
        assert!(blobs[0].iter().any(|byte| *byte != 0));
    }

    #[test]
    fn the_digest_depends_on_content_and_order() {
        let a = vec![vec![1_u8, 2, 3], vec![4, 5, 6]];
        let b = vec![vec![4_u8, 5, 6], vec![1, 2, 3]];
        assert_ne!(digest_blobs(&a), digest_blobs(&b));
        assert_eq!(digest_blobs(&a), digest_blobs(&a.clone()));
    }

    #[test]
    fn the_digest_is_the_published_fnv_constant_for_the_empty_input() {
        assert_eq!(Fnv1a::new().value(), 0xcbf2_9ce4_8422_2325);
        assert!(digest_blobs(&[]).starts_with("fnv1a64:"));
    }
}
