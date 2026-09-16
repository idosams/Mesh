//! Acceptance criterion 1 and plan §12.2's W5 workload: *a middle insertion into a large file
//! changes only the chunks around the insertion point.*
//!
//! W5 names three mutations, and they fail in different ways, so all three are here:
//!
//! | Mutation | What breaks if the chunker is wrong |
//! |---|---|
//! | **Middle insertion** | every byte after the insertion shifts, so a fixed-size chunker re-cuts the entire tail and transfers the whole file |
//! | **Append** | nothing shifts, so this is the easy case — but a chunker that re-derives boundaries from the file's total length would still re-cut everything |
//! | **Random 4 KiB rewrites** | the changes are scattered, so the cost must be proportional to the number of edits and not to the file |
//!
//! # What "only the chunks around the insertion point" is measured as
//!
//! `FileManifest::transfer_bytes_given` — the total length of the chunks in the new version whose
//! digests the peer does not already hold. That is the quantity plan §12.4's budget row is written
//! in, and it is content bytes only: manifest bytes and per-chunk store overhead are the caller's
//! and this crate does not know them.
//!
//! The assertions are stated as multiples of the *configured average chunk size*, not as absolute
//! byte counts, because the property being tested is "a constant number of chunks", and a bound in
//! bytes would silently pass or fail as the parameters move.

use mesh_chunking::testing::{insert_at, overwrite_at, pseudorandom_bytes, source_like_bytes};
use mesh_chunking::{ChunkStream, ChunkingConfig, Digest32, FileManifest};

/// One megabyte of test file. Large enough that whole-file retransfer is obvious against a
/// per-chunk cost, small enough that a debug build finishes in well under a second.
const FILE_LENGTH: usize = 1_024 * 1_024;

fn chunk(data: &[u8], config: ChunkingConfig) -> FileManifest {
    let mut stream = ChunkStream::new(config);
    let mut offset = 0;
    while offset < data.len() {
        let end = (offset + 64 * 1024).min(data.len());
        stream.push(&data[offset..end], |_, _| {});
        offset = end;
    }
    stream.finish(|_, _| {})
}

/// The bytes a peer holding `before` must fetch to reconstruct `after`.
fn transfer(before: &FileManifest, after: &FileManifest) -> u64 {
    let held: Vec<Digest32> = before.distinct_chunks();
    after.transfer_bytes_given(&held)
}

/// How many of `after`'s chunks are new.
fn new_chunk_count(before: &FileManifest, after: &FileManifest) -> usize {
    let held = before.distinct_chunks();
    after
        .distinct_chunks()
        .into_iter()
        .filter(|digest| !held.contains(digest))
        .count()
}

fn config() -> ChunkingConfig {
    ChunkingConfig::default()
}

#[test]
fn a_middle_insertion_moves_only_the_chunks_around_it() {
    let config = config();
    let original = pseudorandom_bytes(FILE_LENGTH, 201);
    let edited = insert_at(&original, FILE_LENGTH / 2, b"one kibibyte of new text");

    let before = chunk(&original, config);
    let after = chunk(&edited, config);

    let moved = transfer(&before, &after);
    let count = new_chunk_count(&before, &after);

    assert!(
        moved <= 4 * config.average_size() as u64,
        "a 24-byte insertion into a {FILE_LENGTH}-byte file moved {moved} bytes across {count} \
         chunks; the chunker is not re-synchronising after the insertion point"
    );
    assert!(
        count <= 4,
        "{count} chunks changed for one insertion; at most the chunk containing it and its \
         immediate neighbours should"
    );
    assert!(
        moved > 0,
        "an insertion changed nothing, which means the two manifests are not of different content"
    );
}

#[test]
fn the_insertion_point_does_not_matter() {
    // A single lucky offset proves nothing: the cut points are content-derived, so an insertion
    // that happens to land next to one is the easy case. Sweep the file.
    let config = config();
    let original = source_like_bytes(FILE_LENGTH, 202);
    let before = chunk(&original, config);
    let ceiling = 4 * config.average_size() as u64;

    let mut worst = 0u64;
    for step in 1..16usize {
        let at = FILE_LENGTH * step / 16;
        let edited = insert_at(&original, at, b"// an inserted line\n");
        let moved = transfer(&before, &chunk(&edited, config));
        worst = worst.max(moved);
        assert!(
            moved <= ceiling,
            "an insertion at byte {at} moved {moved} bytes, above the {ceiling}-byte ceiling"
        );
    }
    assert!(
        worst > 0,
        "no insertion changed anything, so nothing was measured"
    );
}

#[test]
fn an_append_leaves_every_earlier_chunk_alone() {
    let config = config();
    let original = pseudorandom_bytes(FILE_LENGTH, 203);
    let mut edited = original.clone();
    edited.extend_from_slice(&pseudorandom_bytes(4096, 204));

    let before = chunk(&original, config);
    let after = chunk(&edited, config);

    // Every chunk of the original except the last must survive verbatim: the last one is the only
    // chunk whose bytes the append changed, because it was cut short by end of file.
    let held = before.distinct_chunks();
    let survivors = after
        .chunks()
        .iter()
        .filter(|chunk| held.contains(chunk.content_hash()))
        .count();
    assert!(
        survivors >= before.chunks().len() - 1,
        "only {survivors} of {} chunks survived an append",
        before.chunks().len()
    );

    let moved = transfer(&before, &after);
    assert!(
        moved <= 4 * config.average_size() as u64 + 4096,
        "an append of 4096 bytes moved {moved} bytes"
    );
}

#[test]
fn scattered_small_rewrites_cost_per_edit_and_not_per_file() {
    // W5's third mutation. The point is the *shape* of the cost: doubling the edits should roughly
    // double the transfer, and neither should approach the file size.
    let config = config();
    let original = pseudorandom_bytes(FILE_LENGTH, 205);
    let before = chunk(&original, config);
    let replacement = pseudorandom_bytes(4096, 206);

    let mut measured = Vec::new();
    for edits in [1usize, 2, 4, 8] {
        let mut edited = original.clone();
        for edit in 0..edits {
            let at = (FILE_LENGTH / (edits + 1)) * (edit + 1);
            edited = overwrite_at(&edited, at, &replacement);
        }
        let moved = transfer(&before, &chunk(&edited, config));
        measured.push((edits, moved));
        assert!(
            moved <= edits as u64 * 6 * config.average_size() as u64,
            "{edits} scattered 4 KiB rewrites moved {moved} bytes, which is more than a constant \
             number of chunks per edit"
        );
    }

    let (_, one) = measured[0];
    let (_, eight) = measured[3];
    assert!(
        eight < FILE_LENGTH as u64 / 2,
        "eight 4 KiB rewrites moved {eight} bytes of a {FILE_LENGTH}-byte file"
    );
    assert!(
        eight > one,
        "eight edits cost no more than one, so the measurement is not sensitive to the edits at all"
    );
}

#[test]
fn the_plan_parameters_lose_this_workload_to_the_measured_ones() {
    // The task contract's failure clause in reverse: the reason `ChunkingConfig::default` is not
    // `ChunkingConfig::plan_defaults` is measured here, in the suite, and not only in a benchmark
    // nobody runs. If a future change makes the plan's parameters competitive on this workload,
    // this test fails and the default should be revisited.
    let original = source_like_bytes(FILE_LENGTH, 207);
    let edited = insert_at(&original, FILE_LENGTH / 2, b"// one inserted line\n");

    let measured = transfer(
        &chunk(&original, ChunkingConfig::default()),
        &chunk(&edited, ChunkingConfig::default()),
    );
    let plan = transfer(
        &chunk(&original, ChunkingConfig::plan_defaults()),
        &chunk(&edited, ChunkingConfig::plan_defaults()),
    );

    assert!(
        measured * 4 < plan,
        "the measured default moved {measured} bytes and the plan's parameters moved {plan}; the \
         gap that justifies diverging from plan §6.2 has closed, so the default needs re-deriving"
    );
}

#[test]
fn a_one_byte_edit_to_a_quarter_megabyte_file_does_not_cost_the_whole_file() {
    // The measurement that opened this task: a one-byte edit to a 262,144-byte file previously
    // cost 262,144 bytes, because the file was one object. Under the measured default it costs one
    // chunk. Recorded as a test so the regression is caught rather than remembered.
    let config = config();
    let original = source_like_bytes(262_144, 208);
    let edited = overwrite_at(&original, 131_072, b"X");

    let before = chunk(&original, config);
    let after = chunk(&edited, config);
    let moved = transfer(&before, &after);

    assert!(
        moved <= 2 * config.average_size() as u64,
        "a one-byte edit to a 262,144-byte file moved {moved} bytes"
    );

    // And the same edit under the plan's parameters, which put this file below the whole-file
    // threshold, costs all of it. This is the number the default exists to fix.
    let plan = ChunkingConfig::plan_defaults();
    let plan_moved = transfer(&chunk(&original, plan), &chunk(&edited, plan));
    assert_eq!(
        plan_moved, 262_144,
        "under plan §6.2's parameters this file is a single object, so a one-byte edit should cost \
         all 262,144 bytes; if it no longer does, the comparison in this test is stale"
    );
}
