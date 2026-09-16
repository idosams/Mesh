//! The vendored hasher against the one it was copied from, and against the published vectors.
//!
//! # What is at stake
//!
//! A chunk's name is the BLAKE3 digest of its bytes. `mesh-types` computes the digests that go into
//! manifests and `mesh-cas` stores chunks under them. If any of the three implementations ever
//! disagree by a bit, a manifest written here names a chunk that is not in the store and the store
//! holds a chunk no manifest names — silently, and for every chunk written after the divergence.
//! The compiler cannot prevent that, because a dependency edge would rewrite `Cargo.lock`, which
//! this lane may not write; `crates/mesh-cas/tests/blake3_agrees_with_mesh_types.rs` is the same
//! test for the same reason, and this file is deliberately its twin.
//!
//! So the guarantee is a test instead of a type, and it is two tests rather than one, because
//! either alone has a hole:
//!
//! * **Code equality** catches an edit to either copy — including an edit that is *correct*, since
//!   a correct change on one side is still a divergence until it is on both. It cannot catch the
//!   case where both files are identical and both are wrong.
//! * **The published vectors** catch both being wrong. They are read out of `mesh-types`' own test
//!   file rather than copied here, so the two crates cannot even disagree about what the right
//!   answers are.
//!
//! # How it fails
//!
//! Both tests read `mesh-types` by path. Moving or renaming those files makes these tests fail
//! loudly — the arity assertions below turn "found nothing to compare" into a failure — rather
//! than silently passing over a comparison that no longer happens.

use std::fs;
use std::path::PathBuf;

use mesh_chunking::{Blake3, Blake3Hasher, ContentDigest, DigestHasher};

/// The fewest reference vectors a successful parse can plausibly yield.
const MINIMUM_VECTORS: usize = 30;

fn mesh_types_file(relative: &[&str]) -> String {
    let mut path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "..", "mesh-types"]
        .iter()
        .collect();
    for part in relative {
        path.push(part);
    }
    fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read {} ({error}). mesh-chunking holds its copy of BLAKE3 against mesh-types \
             by reading its source, because no dependency edge is permitted; if that file has \
             moved, the copy is unchecked until this path is corrected.",
            path.display()
        )
    })
}

/// Source text with documentation, comments and blank lines removed.
///
/// The two copies differ in their documentation on purpose — one explains why it is the original,
/// the other why it is a copy, and the doctest in each names its own crate. Neither difference can
/// change a digest, so the comparison is over code and the vectors below cover the rest.
fn code_only(source: &str) -> Vec<String> {
    source
        .lines()
        .map(str::trim_end)
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.is_empty() && !trimmed.starts_with("//")
        })
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_vendored_hasher_is_code_identical_to_the_one_in_mesh_types() {
    let theirs = code_only(&mesh_types_file(&["src", "blake3.rs"]));
    let ours = code_only(
        &fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/blake3.rs"))
            .expect("this crate's own copy is readable"),
    );

    assert!(
        theirs.len() > 100,
        "only {} lines of code were found in mesh-types' hasher; the comment filter is wrong or \
         the file is not what this test thinks it is",
        theirs.len()
    );

    for (number, (theirs, ours)) in theirs.iter().zip(ours.iter()).enumerate() {
        assert_eq!(
            theirs,
            ours,
            "the two copies of BLAKE3 diverge at code line {}. A digest computed here would stop \
             matching the digest mesh-types puts in a manifest, so the copies must be brought back \
             together — in both files — before this can go green.",
            number + 1
        );
    }
    assert_eq!(
        theirs.len(),
        ours.len(),
        "the two copies of BLAKE3 have different amounts of code, so one has gained or lost \
         something the other has not"
    );
}

/// `(input length, expected digest)`, parsed out of `mesh-types`' own vector table.
fn reference_vectors() -> Vec<(usize, String)> {
    let source = mesh_types_file(&["tests", "blake3_reference_vectors.rs"]);
    let start = source
        .find("REFERENCE_VECTORS")
        .expect("mesh-types declares a REFERENCE_VECTORS table");
    let body = &source[start..];
    let end = body.find("\n];").expect("the vector table is terminated");

    let mut vectors = Vec::new();
    let mut pending: Option<usize> = None;
    for line in body[..end].lines() {
        let field = line.trim().trim_end_matches(',');
        if let Ok(length) = field.parse::<usize>() {
            pending = Some(length);
            continue;
        }
        let Some(hex) = field
            .strip_prefix('"')
            .and_then(|rest| rest.strip_suffix('"'))
        else {
            continue;
        };
        if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            continue;
        }
        if let Some(length) = pending.take() {
            vectors.push((length, hex.to_owned()));
        }
    }
    vectors
}

/// The published BLAKE3 test-vector input: the repeating byte pattern `0, 1, .., 250, 0, 1, ..`.
fn published_input(length: usize) -> Vec<u8> {
    (0..length).map(|index| (index % 251) as u8).collect()
}

#[test]
fn the_vendored_hasher_reproduces_the_published_vectors() {
    let vectors = reference_vectors();
    assert!(
        vectors.len() >= MINIMUM_VECTORS,
        "only {} reference vectors were parsed out of mesh-types' table; the parser is broken and \
         a passing run would prove nothing",
        vectors.len()
    );

    for (length, expected) in vectors {
        let input = published_input(length);
        assert_eq!(
            Blake3::digest_bytes(&input).to_hex(),
            expected,
            "the vendored hasher disagrees with the published vector for a {length}-byte input"
        );

        // The same input through the incremental interface, split at an awkward place, so that a
        // divergence in the chunk-stack logic cannot hide behind the one-shot path.
        let mut hasher = Blake3Hasher::new();
        let split = length / 3;
        DigestHasher::update(&mut hasher, &input[..split]);
        DigestHasher::update(&mut hasher, &input[split..]);
        assert_eq!(
            DigestHasher::finalize(hasher).to_hex(),
            expected,
            "the incremental path disagrees with the one-shot path for a {length}-byte input"
        );
    }
}
