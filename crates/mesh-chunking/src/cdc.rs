//! Where one chunk stops and the next begins.
//!
//! The cut is FastCDC with normalized chunking (Xia et al., ATC 2016; Xia et al., TPDS 2020): a
//! gear-hash rolling window, a minimum length that is skipped without hashing, a strict mask while
//! the chunk is short and a permissive mask once it has passed the average, and a hard cut at the
//! maximum.
//!
//! # The one property everything else rests on
//!
//! > **A cut point is a function of the bytes immediately before it, and of nothing else.**
//!
//! Not of the file's length, not of the chunk's index, not of where the file sits on disk, and —
//! after the first `min_size` bytes of a chunk — not of where the chunk started. That is what makes
//! an insertion local: the chunks before the insertion are cut by bytes the insertion did not
//! touch, and the chunks after it re-synchronise as soon as one full window of unchanged bytes has
//! passed under the mask. `tests/locality.rs` measures how quickly, rather than asserting that it
//! does.
//!
//! The same property is what makes the cut deterministic across runs, machines, endiannesses and
//! Rust versions: it is integer arithmetic on `u64` with explicit wrapping and a table this crate
//! computes itself.
//!
//! # The minimum-size skip, and what it costs
//!
//! FastCDC does not hash the first `min_size` bytes of a chunk. That is most of its speed, and it
//! is also a real weakening of locality: an edit inside the skipped region cannot move the chunk's
//! own cut point, so the whole chunk changes rather than splitting. That is the correct trade — a
//! chunk shorter than the minimum is not wanted anyway — but it is stated here rather than left for
//! someone to discover, because it is the reason `min_size` is a locality parameter and not just a
//! performance one.

use crate::config::ChunkingConfig;
use crate::gear::GEAR;

/// The number of bytes from `data` that belong to the next chunk.
///
/// The returned length is always at least `1` for non-empty input and never exceeds
/// `config.max_size()`. It equals `data.len()` when the content offered no cut point, which the
/// streaming caller must distinguish from a genuine cut: a length equal to the buffer is only
/// final at end of input.
///
/// # Panics
///
/// Never. Every branch is bounds-checked by construction and `config` is validated on the way in.
#[must_use]
pub(crate) fn next_cut(data: &[u8], config: &ChunkingConfig) -> usize {
    let available = data.len();
    if available == 0 {
        return 0;
    }
    if available <= config.min_size() {
        return available;
    }

    // Never look past the hard maximum: a cut found beyond it would be discarded anyway, and
    // hashing those bytes is pure cost.
    let ceiling = available.min(config.max_size());

    // The strict mask governs up to the average, the permissive mask from there to the ceiling.
    let strict_until = ceiling.min(config.average_size());

    let strict = config.strict_mask();
    let permissive = config.permissive_mask();

    let mut fingerprint: u64 = 0;
    let mut index = config.min_size();

    while index < strict_until {
        fingerprint = (fingerprint << 1).wrapping_add(GEAR[usize::from(data[index])]);
        index += 1;
        if fingerprint & strict == 0 {
            return index;
        }
    }

    while index < ceiling {
        fingerprint = (fingerprint << 1).wrapping_add(GEAR[usize::from(data[index])]);
        index += 1;
        if fingerprint & permissive == 0 {
            return index;
        }
    }

    ceiling
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::pseudorandom_bytes;

    fn small_config() -> ChunkingConfig {
        ChunkingConfig::always_chunked(64, 256, 2048).expect("a valid small configuration")
    }

    #[test]
    fn an_empty_buffer_has_no_cut() {
        assert_eq!(next_cut(&[], &small_config()), 0);
    }

    #[test]
    fn a_buffer_at_or_below_the_minimum_is_taken_whole() {
        let config = small_config();
        let data = pseudorandom_bytes(config.min_size(), 1);
        assert_eq!(next_cut(&data, &config), config.min_size());
    }

    #[test]
    fn a_cut_never_exceeds_the_maximum() {
        let config = small_config();
        // A run of one repeated byte offers the rolling hash nothing to vary on, so this is the
        // worst case for the content-defined stage and the hard cut has to carry it.
        let data = vec![0x5Au8; config.max_size() * 4];
        assert_eq!(next_cut(&data, &config), config.max_size());
    }

    #[test]
    fn a_cut_is_never_shorter_than_the_minimum_when_more_data_exists() {
        let config = small_config();
        let data = pseudorandom_bytes(config.max_size() * 4, 2);
        let mut offset = 0;
        while offset < data.len() {
            let cut = next_cut(&data[offset..], &config);
            let remaining = data.len() - offset;
            if remaining > config.min_size() {
                assert!(
                    cut >= config.min_size(),
                    "a cut of {cut} at offset {offset} is below the {} minimum",
                    config.min_size()
                );
            }
            assert!(
                cut > 0,
                "the cut must advance or the chunker cannot terminate"
            );
            offset += cut;
        }
    }

    #[test]
    fn the_same_bytes_cut_at_the_same_place_every_time() {
        let config = small_config();
        let data = pseudorandom_bytes(64 * 1024, 3);
        let first = next_cut(&data, &config);
        for _ in 0..16 {
            assert_eq!(next_cut(&data, &config), first);
        }
    }

    #[test]
    fn a_longer_buffer_with_the_same_prefix_cuts_at_the_same_place() {
        // The cut may not depend on how much data happens to be available past it — that is the
        // whole basis of streaming and of the locality argument.
        let config = small_config();
        let data = pseudorandom_bytes(64 * 1024, 4);
        let cut = next_cut(&data, &config);
        assert!(cut < data.len(), "this fixture is meant to contain a cut");
        for extra in [0usize, 1, 7, 64, 4096] {
            let end = (cut + extra).min(data.len());
            assert_eq!(
                next_cut(&data[..end], &config),
                cut,
                "the cut moved when {extra} trailing bytes were made visible"
            );
        }
    }

    #[test]
    fn the_average_chunk_lands_near_the_configured_average() {
        // Normalized chunking exists to make this true. Without it a single mask gives an
        // exponential distribution whose mean drifts far from the target once the minimum and
        // maximum clamp the tails.
        let config = ChunkingConfig::always_chunked(2048, 8192, 65536).expect("valid");
        let data = pseudorandom_bytes(4 * 1024 * 1024, 5);
        let mut offset = 0;
        let mut chunks = 0usize;
        while offset < data.len() {
            offset += next_cut(&data[offset..], &config);
            chunks += 1;
        }
        let mean = data.len() / chunks;
        assert!(
            (4096..=16384).contains(&mean),
            "the mean chunk was {mean} bytes against an 8192-byte target over {chunks} chunks"
        );
    }
}
