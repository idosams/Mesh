//! The 256-entry gear table the rolling hash is built from.
//!
//! # Why it is computed rather than pasted
//!
//! Every published content-defined chunker carries a table of 256 "random" 64-bit words, and every
//! one of them is a wall of hex literals nobody can check. A pasted table has two problems this
//! crate cannot accept. It is **unverifiable** — a reviewer can confirm the digits were copied
//! correctly and nothing else — and it is **someone else's**, which is exactly the thing the task
//! contract warns against when it says the parameters must be measured rather than copied from
//! another project's workload.
//!
//! So the table is derived, at compile time, from a documented function of one seed by SplitMix64
//! (Steele, Lea and Flood, 2014), whose constants are published and whose output for a given seed
//! anyone can reproduce in three lines of any language. The whole table is then a function of
//! [`GEAR_SEED`], which is one number a reader can check rather than 256.
//!
//! # What the table has to be
//!
//! Only two properties matter for chunking, and neither needs cryptographic strength:
//!
//! * the 256 words are **distinct**, so distinct bytes push distinct values into the hash;
//! * the bits the cut mask looks at are **balanced across the table**, so a boundary is as likely
//!   after one byte value as after another and the chunk-size distribution is not skewed by the
//!   table.
//!
//! Both are asserted in this module's tests, over the actual table rather than over the generator.
//! A third test pins the table's BLAKE3 digest, so a change to the seed or the generator is a
//! deliberate act with a failing test attached rather than a silent reshuffle of every chunk
//! boundary in every existing store.
//!
//! # This is not a hash function for security
//!
//! Gear hashing is trivially invertible and an adversary who chooses content can place boundaries
//! wherever they like. That matters for a deduplicating service shared between distrusting
//! tenants, and it is **not** claimed against here: the chunker's contract is determinism and
//! locality, not unpredictability. Plan §2.10 forbids the stronger claim without evidence, and
//! there is none, because the property does not hold.

/// The seed the whole table is a function of.
///
/// The value is `0x0000_4D45_5348_0001` — the ASCII bytes `MESH` in the middle 32 bits with a
/// version counter in the low 16, so that a future deliberate reshuffle has an obvious next value
/// and an accidental one is visible on sight. Nothing about chunking depends on the seed's value;
/// everything depends on it never changing by accident.
pub const GEAR_SEED: u64 = 0x0000_4D45_5348_0001;

/// SplitMix64: one step of the published generator.
///
/// Returns `(next_state, output)`. The three constants are the published ones and are not tunable;
/// this crate does not invent a mixing function.
const fn splitmix64(state: u64) -> (u64, u64) {
    let state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (state, z ^ (z >> 31))
}

/// The table, built at compile time from [`GEAR_SEED`].
const fn build_table() -> [u64; 256] {
    let mut table = [0u64; 256];
    let mut state = GEAR_SEED;
    let mut index = 0;
    while index < 256 {
        let (next, value) = splitmix64(state);
        state = next;
        table[index] = value;
        index += 1;
    }
    table
}

/// One 64-bit word per byte value, mixed into the rolling hash as each byte is consumed.
pub(crate) const GEAR: [u64; 256] = build_table();

#[cfg(test)]
mod tests {
    use super::*;
    use crate::digest::{Blake3, ContentDigest};

    #[test]
    fn every_entry_is_distinct() {
        let mut sorted = GEAR;
        sorted.sort_unstable();
        for window in sorted.windows(2) {
            assert_ne!(
                window[0], window[1],
                "two byte values push the same word into the rolling hash, so the chunker cannot \
                 tell them apart"
            );
        }
    }

    #[test]
    fn no_entry_is_zero() {
        assert!(
            !GEAR.contains(&0),
            "a zero entry makes one byte value invisible to the rolling hash"
        );
    }

    #[test]
    fn the_bits_the_mask_reads_are_balanced() {
        // The cut masks read the low bits. Over 256 draws, each bit position should be set close
        // to 128 times; the bound is generous because this is a balance check, not a randomness
        // test — a table with a stuck bit would be hundreds away, not tens.
        for bit in 0..64u32 {
            let ones = GEAR.iter().filter(|word| (*word >> bit) & 1 == 1).count();
            assert!(
                (96..=160).contains(&ones),
                "bit {bit} is set in {ones} of 256 entries; a skewed bit skews every chunk \
                 boundary the mask reads it in"
            );
        }
    }

    #[test]
    fn the_table_is_pinned() {
        // The digest of the table's little-endian bytes. Changing the seed or the generator
        // changes every chunk boundary this crate has ever produced, which orphans every stored
        // chunk; that must be a deliberate act, so it is a failing test rather than a silent one.
        let mut bytes = Vec::with_capacity(256 * 8);
        for word in GEAR {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        assert_eq!(
            Blake3::digest_bytes(&bytes).to_hex(),
            "7c8b8f841ddc9d7b84428038d1137d600d4cced07323e2239b7733323953dc59",
            "the gear table changed. Every boundary in every existing store moves with it; if that \
             is intended, update this digest in the same commit and say so in the pull request"
        );
    }

    #[test]
    fn splitmix64_reproduces_its_published_output() {
        // Seed 0, the standard smoke vector for the published generator: a third party can check
        // this line against any other SplitMix64 implementation without reading the rest of the
        // crate.
        let (_, first) = splitmix64(0);
        assert_eq!(first, 0xE220_A839_7B1D_CDAF);
    }
}
