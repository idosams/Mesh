//! Acceptance criterion 4: *a one-kibibyte edit in a one-gibibyte file transfers under the plan's
//! budget.*
//!
//! Plan §12.4 states the budget as a table row:
//!
//! ```text
//! | 1 KiB edit in 1 GiB file | <4 MiB transfer | target <1 MiB |
//! ```
//!
//! Both numbers are asserted, not just the ceiling. A change that stays under 4 MiB while blowing
//! past 1 MiB has lost the property this crate exists for, and finding that out from a benchmark
//! nobody ran is finding it out too late.
//!
//! # The scale this runs at, stated plainly
//!
//! **By default this test runs at 32 MiB, not at 1 GiB.** The suite is built with `opt-level = 0`,
//! where this crate chunks at roughly 8 MiB/s, so a gibibyte in each of two versions is about four
//! and a half minutes of a shared test suite for a property that is already scale-independent by
//! construction — the transfer is a constant number of chunks, and
//! `the_transfer_does_not_grow_with_the_file` measures exactly that constancy across four scales.
//!
//! The full gibibyte is a supported run and is not hypothetical:
//!
//! ```text
//! MESH_CHUNKING_W5_BYTES=1073741824 cargo nextest run -p mesh-chunking --test transfer_budget
//! ```
//!
//! The pull request that introduced this file pastes that run's output. Anyone who does not trust
//! the extrapolation can reproduce it in five minutes, which is the standard plan §2.10 asks for —
//! a number with a command attached — rather than a claim.
//!
//! # Why the content is generated block by block
//!
//! A gibibyte in a `Vec` would make the test's own memory the limiting factor and would prove
//! nothing about the chunker's. Both versions are streamed through [`ChunkStream`] a block at a
//! time and the peak buffer is asserted, so a passing run is also evidence that chunking a
//! gibibyte never needs a gibibyte of memory.

use std::collections::HashSet;

use mesh_chunking::testing::pseudorandom_bytes;
use mesh_chunking::{ChunkStream, ChunkingConfig, Digest32};

/// The block the synthetic file is generated and pushed in.
const BLOCK: usize = 1024 * 1024;

/// The plan §12.4 ceiling.
const BUDGET: u64 = 4 * 1024 * 1024;

/// The plan §12.4 target.
const TARGET: u64 = 1024 * 1024;

/// The edit W5 specifies.
const EDIT_LENGTH: usize = 1024;

/// The scale to run at: `MESH_CHUNKING_W5_BYTES` if set, otherwise 32 MiB.
fn file_length() -> usize {
    match std::env::var("MESH_CHUNKING_W5_BYTES") {
        Ok(text) => text
            .trim()
            .parse()
            .expect("MESH_CHUNKING_W5_BYTES must be a byte count"),
        Err(_) => 32 * 1024 * 1024,
    }
}

/// One block of the synthetic file, deterministic in its index.
fn block_at(index: usize, length: usize) -> Vec<u8> {
    pseudorandom_bytes(length, 0x5715_0000 ^ index as u64)
}

/// The result of streaming one version of the file.
struct Version {
    digests: Vec<Digest32>,
    lengths: Vec<u64>,
    peak_buffer: usize,
    chunk_count: usize,
}

/// Stream `total` bytes of the synthetic file, optionally overwriting `EDIT_LENGTH` bytes at
/// `edit_at`, and record what came out.
fn stream_version(total: usize, config: ChunkingConfig, edit_at: Option<usize>) -> Version {
    let mut stream = ChunkStream::new(config);
    let mut digests = Vec::new();
    let mut lengths = Vec::new();
    let mut peak_buffer = 0usize;

    let mut offset = 0;
    while offset < total {
        let length = BLOCK.min(total - offset);
        let mut block = block_at(offset / BLOCK, length);

        if let Some(at) = edit_at {
            let end = at + EDIT_LENGTH;
            if at < offset + length && end > offset {
                let from = at.max(offset) - offset;
                let to = end.min(offset + length) - offset;
                for byte in &mut block[from..to] {
                    *byte = !*byte;
                }
            }
        }

        stream.push(&block, |chunk_ref, _| {
            digests.push(*chunk_ref.content_hash());
            lengths.push(chunk_ref.length());
        });
        peak_buffer = peak_buffer.max(stream.buffered_bytes());
        offset += length;
    }

    let _ = stream.finish(|chunk_ref, _| {
        digests.push(*chunk_ref.content_hash());
        lengths.push(chunk_ref.length());
    });

    let chunk_count = digests.len();
    Version {
        digests,
        lengths,
        peak_buffer,
        chunk_count,
    }
}

/// The bytes of `after` that a peer holding every chunk of `before` still needs.
fn transfer(before: &Version, after: &Version) -> u64 {
    let held: HashSet<Digest32> = before.digests.iter().copied().collect();
    let mut counted: HashSet<Digest32> = HashSet::new();
    let mut total = 0u64;
    for (digest, length) in after.digests.iter().zip(after.lengths.iter()) {
        if held.contains(digest) || !counted.insert(*digest) {
            continue;
        }
        total += *length;
    }
    total
}

#[test]
fn a_one_kibibyte_edit_transfers_under_the_plan_budget() {
    let total = file_length();
    let config = ChunkingConfig::default();

    let before = stream_version(total, config, None);
    let after = stream_version(total, config, Some(total / 2));

    let moved = transfer(&before, &after);
    let percent = moved as f64 * 100.0 / total as f64;

    eprintln!(
        "W5 transfer budget: file {total} B, {} chunks, edit {EDIT_LENGTH} B at {} -> {moved} B \
         transferred ({percent:.6}% of the file); peak chunker buffer {} B",
        before.chunk_count,
        total / 2,
        before.peak_buffer
    );

    assert!(
        moved < BUDGET,
        "a {EDIT_LENGTH}-byte edit in a {total}-byte file moved {moved} bytes, above plan §12.4's \
         {BUDGET}-byte ceiling"
    );
    assert!(
        moved < TARGET,
        "a {EDIT_LENGTH}-byte edit in a {total}-byte file moved {moved} bytes, above plan §12.4's \
         {TARGET}-byte target. The ceiling still holds, so this is a warning rather than a \
         correctness failure — but the target is what content-defined chunking is for."
    );
    assert!(
        moved > 0,
        "the edited version transferred nothing, so the two versions are identical and this test \
         measured nothing"
    );
}

#[test]
fn chunking_a_large_file_does_not_need_a_large_buffer() {
    let total = file_length();
    let config = ChunkingConfig::default();
    let version = stream_version(total, config, None);

    let bound = config.peak_buffered_bytes() as usize + config.max_size() + BLOCK;
    assert!(
        version.peak_buffer <= bound,
        "chunking {total} bytes peaked at {} bytes of buffer, above the {bound}-byte bound; the \
         streaming property this crate claims does not hold",
        version.peak_buffer
    );
    assert_eq!(
        version.lengths.iter().sum::<u64>(),
        total as u64,
        "the chunks do not account for every byte of the file"
    );
}

#[test]
fn the_transfer_does_not_grow_with_the_file() {
    // The reason the default scale above is honest. If the cost of one edit were a function of the
    // file's size, this would show it: the same edit is applied to files sixteen times apart and
    // the transfers are compared to each other, not to a constant.
    let config = ChunkingConfig::default();
    let mut measurements = Vec::new();

    for megabytes in [2usize, 8, 16, 32] {
        let total = megabytes * 1024 * 1024;
        let before = stream_version(total, config, None);
        let after = stream_version(total, config, Some(total / 2));
        let moved = transfer(&before, &after);
        measurements.push((total, moved));
    }

    let (smallest_file, smallest_moved) = measurements[0];
    let (largest_file, largest_moved) = measurements[measurements.len() - 1];

    eprintln!("W5 scale sweep: {measurements:?}");

    assert!(
        largest_moved <= smallest_moved * 4,
        "a file {}x larger moved {largest_moved} bytes against {smallest_moved} for the small one; \
         the cost of an edit is tracking the file size, so extrapolating to a gibibyte would not \
         be valid",
        largest_file / smallest_file
    );

    for (total, moved) in measurements {
        assert!(
            moved < TARGET,
            "at {total} bytes the edit moved {moved}, above the {TARGET}-byte target"
        );
    }
}
