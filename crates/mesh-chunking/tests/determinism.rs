//! Acceptance criterion 2: *the same bytes always produce the same boundaries.*
//!
//! Determinism here means something stronger than "the function is pure". A chunk boundary must not
//! depend on:
//!
//! * **how the bytes arrived** — one push or a million, aligned or not;
//! * **what came after** — a cut is final before the rest of the file exists, or streaming is
//!   impossible;
//! * **the machine** — the arithmetic is `u64` with explicit wrapping over a table this crate
//!   computes from a seed, so there is no endianness, no floating point, no pointer width and no
//!   hash-map iteration order anywhere in the path;
//! * **the run** — no clock, no address, no environment.
//!
//! Each of those is a separate way the property can fail, so each gets its own test rather than
//! one test that happens to cover them all.
//!
//! The last one is the reason this file also pins a digest of a chunking of a fixed input. A
//! property test proves this build agrees with itself; the pinned digest proves this build agrees
//! with the build that produced the number, which is the only form of cross-version determinism a
//! single-machine test suite can actually assert.

use mesh_chunking::testing::{pseudorandom_bytes, source_like_bytes};
use mesh_chunking::{Blake3, ChunkStream, ChunkingConfig, ContentDigest, FileManifest};

fn config() -> ChunkingConfig {
    ChunkingConfig::default()
}

/// Chunk `data` by pushing it in `step`-byte slices.
fn chunk_in_steps(data: &[u8], step: usize, config: ChunkingConfig) -> FileManifest {
    let mut stream = ChunkStream::new(config);
    let mut offset = 0;
    while offset < data.len() {
        let end = (offset + step).min(data.len());
        stream.push(&data[offset..end], |_, _| {});
        offset = end;
    }
    stream.finish(|_, _| {})
}

/// A digest over a manifest's boundaries, so two chunkings can be compared in one value.
fn boundary_digest(manifest: &FileManifest) -> String {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&manifest.byte_length().to_le_bytes());
    bytes.extend_from_slice(manifest.content_hash().as_bytes());
    for chunk in manifest.chunks() {
        bytes.extend_from_slice(&chunk.offset().to_le_bytes());
        bytes.extend_from_slice(&chunk.length().to_le_bytes());
        bytes.extend_from_slice(chunk.content_hash().as_bytes());
    }
    Blake3::digest_bytes(&bytes).to_hex()
}

#[test]
fn the_same_bytes_chunk_the_same_way_every_time() {
    let data = pseudorandom_bytes(400_000, 101);
    let first = chunk_in_steps(&data, data.len(), config());
    for _ in 0..8 {
        assert_eq!(chunk_in_steps(&data, data.len(), config()), first);
    }
}

#[test]
fn the_arrival_pattern_does_not_move_a_boundary() {
    let data = source_like_bytes(400_000, 102);
    let reference = chunk_in_steps(&data, data.len(), config());

    for step in [1usize, 3, 7, 61, 511, 4096, 65_537, 400_000] {
        assert_eq!(
            chunk_in_steps(&data, step, config()),
            reference,
            "pushing in {step}-byte slices produced a different manifest"
        );
    }
}

#[test]
fn a_prefix_chunks_the_same_way_as_the_prefix_of_the_whole() {
    // Every chunk except the last is decided without knowing the rest of the file exists. If that
    // were false, a streaming chunker would have to buffer the file.
    let data = pseudorandom_bytes(400_000, 103);
    let whole = chunk_in_steps(&data, 8192, config());

    // Cut the input at the end of the fifth chunk: the first five chunks must be identical.
    let boundary = whole.chunks()[4].offset() + whole.chunks()[4].length();
    let prefix = chunk_in_steps(&data[..boundary as usize], 8192, config());
    assert_eq!(prefix.chunks(), &whole.chunks()[..5]);
}

#[test]
fn the_property_holds_over_many_seeds_and_many_parameter_sets() {
    // The property test the contract asks for: not one fixture, a family.
    let parameter_sets = [
        ChunkingConfig::always_chunked(256, 1024, 8192).expect("valid"),
        ChunkingConfig::always_chunked(512, 4096, 32768).expect("valid"),
        ChunkingConfig::default(),
        ChunkingConfig::plan_defaults(),
        ChunkingConfig::new(1024, 128, 1024, 8192, 1).expect("valid"),
        ChunkingConfig::new(0, 2048, 8192, 65536, 0).expect("valid"),
    ];

    for seed in 0..6u64 {
        let length = 200_000 + (seed as usize * 7919) % 100_000;
        let data = if seed % 2 == 0 {
            pseudorandom_bytes(length, seed)
        } else {
            source_like_bytes(length, seed)
        };

        for (index, config) in parameter_sets.iter().enumerate() {
            let a = chunk_in_steps(&data, data.len(), *config);
            let b = chunk_in_steps(&data, 997, *config);
            assert_eq!(
                a, b,
                "seed {seed} under parameter set {index} chunked differently when streamed"
            );
            assert!(
                a.chunks_are_contiguous(),
                "seed {seed} under parameter set {index} produced a manifest with a hole"
            );
            assert_eq!(a.content_hash(), &Blake3::digest_bytes(&data));
        }
    }
}

#[test]
fn every_chunk_respects_the_configured_bounds() {
    let config = ChunkingConfig::always_chunked(2048, 8192, 65536).expect("valid");
    let data = pseudorandom_bytes(1_000_000, 104);
    let manifest = chunk_in_steps(&data, 4096, config);
    let chunks = manifest.chunks();

    for (index, chunk) in chunks.iter().enumerate() {
        assert!(
            chunk.length() <= config.max_size() as u64,
            "chunk {index} is {} bytes, above the {} maximum",
            chunk.length(),
            config.max_size()
        );
        let last = index + 1 == chunks.len();
        if !last {
            assert!(
                chunk.length() >= config.min_size() as u64,
                "chunk {index} is {} bytes, below the {} minimum, and it is not the last",
                chunk.length(),
                config.min_size()
            );
        }
    }
}

#[test]
fn the_boundaries_of_a_fixed_input_are_pinned() {
    // Cross-version determinism, as far as a single-machine suite can assert it. The three digests
    // below were produced by this implementation; a change to the gear table, the masks, the
    // minimum-size skip or the normalized-chunking stages moves them. If one of these fails, the
    // question is not "is the new value fine" — it is "what happens to every chunk already in
    // every store", and the answer belongs in the pull request that changes it.
    let cases: [(ChunkingConfig, usize, u64, &str); 3] = [
        (
            ChunkingConfig::default(),
            600_000,
            105,
            "8506c91c11bf00c168ccd1a1c67c9c9b4081b477491fd230537508f99e1d6e86",
        ),
        (
            ChunkingConfig::plan_defaults(),
            2_500_000,
            105,
            "2823d4c1758730c1c036e15fb11d55ee7884d48f741abc3b1898fd31f56bc23a",
        ),
        (
            ChunkingConfig::always_chunked(64, 256, 2048).expect("valid"),
            200_000,
            106,
            "a262b0711b50c795586259e4a28fc056b74d7f4188ed9be8e4e392eb5cba73df",
        ),
    ];

    for (config, length, seed, expected) in cases {
        let data = pseudorandom_bytes(length, seed);
        let manifest = chunk_in_steps(&data, 65536, config);
        assert_eq!(
            boundary_digest(&manifest),
            expected,
            "the pinned boundaries moved for seed {seed} at average {} — every stored chunk cut \
             with these parameters is orphaned by this change",
            config.average_size()
        );
    }
}
