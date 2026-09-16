//! Deterministic fixtures, so that every measurement in this crate can be reproduced from a seed.
//!
//! Nothing here reads a clock, an environment variable or an operating-system entropy source. A
//! chunking test that used real randomness would be a test whose failures cannot be reproduced,
//! and a chunking benchmark that used real randomness would publish a number nobody else can get.
//!
//! The generator is SplitMix64, the same one [`crate::gear`] derives its table from, run over a
//! caller-supplied seed. It is not a cryptographic generator and is not used as one.

/// `length` bytes that are a pure function of `seed`.
///
/// The output has no structure a content-defined chunker can exploit and no long runs, which makes
/// it the *hardest* input for deduplication and therefore the honest one for a worst-case
/// measurement. [`source_like_bytes`] is the realistic counterpart.
#[must_use]
pub fn pseudorandom_bytes(length: usize, seed: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(length);
    let mut state = seed.wrapping_mul(0x2545_F491_4F6C_DD1D) ^ 0x5DEE_CE66_D1B2_45E7;
    while out.len() < length {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        let word = z ^ (z >> 31);
        let take = (length - out.len()).min(8);
        out.extend_from_slice(&word.to_le_bytes()[..take]);
    }
    out
}

/// `length` bytes that look like source code: short lines, a small alphabet, repeated identifiers.
///
/// The point is the *shape*, not the language. Real source has a low-entropy byte distribution and
/// heavy local repetition, and both change how many chunk boundaries a given mask finds. Measuring
/// a chunking policy only on white noise measures the case a source tree never presents, which is
/// how a parameter set that loses on the real corpus gets published as a win.
#[must_use]
pub fn source_like_bytes(length: usize, seed: u64) -> Vec<u8> {
    const WORDS: &[&str] = &[
        "let",
        "fn",
        "pub",
        "struct",
        "impl",
        "match",
        "return",
        "self",
        "config",
        "chunk",
        "manifest",
        "digest",
        "offset",
        "length",
        "bytes",
        "result",
        "value",
        "index",
        "stream",
        "assert_eq!",
        "Some",
        "None",
        "Ok",
        "Err",
        "u64",
        "usize",
        "&str",
        "Vec<u8>",
    ];
    let mut state = seed ^ 0x1234_5678_9ABC_DEF0;
    let mut next = move || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };

    let mut out = Vec::with_capacity(length);
    while out.len() < length {
        let indent = (next() % 4) as usize * 4;
        out.extend(std::iter::repeat_n(b' ', indent));
        let words = 2 + (next() % 7) as usize;
        for word in 0..words {
            if word > 0 {
                out.push(b' ');
            }
            out.extend_from_slice(WORDS[(next() % WORDS.len() as u64) as usize].as_bytes());
        }
        out.push(b';');
        out.push(b'\n');
    }
    out.truncate(length);
    out
}

/// `original` with `insert` spliced in at `at`, which is the W5 middle-insertion mutation.
///
/// # Panics
///
/// If `at` is beyond the end of `original`, because a test that meant to edit the middle and
/// silently appended instead would measure the easy case and report it as the hard one.
#[must_use]
pub fn insert_at(original: &[u8], at: usize, insert: &[u8]) -> Vec<u8> {
    assert!(
        at <= original.len(),
        "insertion point {at} is past the end of a {}-byte input",
        original.len()
    );
    let mut out = Vec::with_capacity(original.len() + insert.len());
    out.extend_from_slice(&original[..at]);
    out.extend_from_slice(insert);
    out.extend_from_slice(&original[at..]);
    out
}

/// `original` with `replacement` written over the bytes at `at`, which is the W5 in-place rewrite.
///
/// # Panics
///
/// If the rewrite would run past the end of `original`.
#[must_use]
pub fn overwrite_at(original: &[u8], at: usize, replacement: &[u8]) -> Vec<u8> {
    assert!(
        at + replacement.len() <= original.len(),
        "a {}-byte rewrite at {at} runs past the end of a {}-byte input",
        replacement.len(),
        original.len()
    );
    let mut out = original.to_vec();
    out[at..at + replacement.len()].copy_from_slice(replacement);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pseudorandom_bytes_are_a_function_of_the_seed() {
        assert_eq!(pseudorandom_bytes(1000, 7), pseudorandom_bytes(1000, 7));
        assert_ne!(pseudorandom_bytes(1000, 7), pseudorandom_bytes(1000, 8));
    }

    #[test]
    fn pseudorandom_bytes_are_the_requested_length() {
        for length in [0usize, 1, 7, 8, 9, 4096, 100_003] {
            assert_eq!(pseudorandom_bytes(length, 1).len(), length);
        }
    }

    #[test]
    fn a_prefix_is_stable_across_lengths() {
        // Chunking fixtures splice long and short generations together; a generator whose prefix
        // depended on the requested length would make those fixtures incomparable.
        let long = pseudorandom_bytes(10_000, 3);
        let short = pseudorandom_bytes(500, 3);
        assert_eq!(&long[..500], &short[..]);
    }

    #[test]
    fn source_like_bytes_are_a_function_of_the_seed_and_the_length() {
        assert_eq!(source_like_bytes(5000, 2), source_like_bytes(5000, 2));
        assert_ne!(source_like_bytes(5000, 2), source_like_bytes(5000, 3));
        assert_eq!(source_like_bytes(5000, 2).len(), 5000);
    }

    #[test]
    fn source_like_bytes_use_a_small_alphabet() {
        let data = source_like_bytes(50_000, 4);
        let mut seen = [false; 256];
        for byte in &data {
            seen[usize::from(*byte)] = true;
        }
        let distinct = seen.iter().filter(|flag| **flag).count();
        assert!(
            distinct < 80,
            "{distinct} distinct byte values is not a source-like distribution"
        );
    }

    #[test]
    fn insertion_and_rewrite_do_what_they_say() {
        assert_eq!(insert_at(b"abcdef", 3, b"XY"), b"abcXYdef".to_vec());
        assert_eq!(overwrite_at(b"abcdef", 2, b"XY"), b"abXYef".to_vec());
    }
}
