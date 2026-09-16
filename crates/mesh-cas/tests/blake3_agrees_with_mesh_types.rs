//! The vendored hasher against the one it was copied from, and against the published vectors.
//!
//! # What is at stake
//!
//! A chunk's name is the BLAKE3 digest of its bytes. `mesh-types` computes the digests that go into
//! manifests. If the two implementations ever disagree by a bit, a manifest names a chunk that is
//! not in the store and the store holds a chunk no manifest names — silently, and for every chunk
//! written after the divergence. The compiler cannot prevent that here, because
//! `docs/adr/0008-ship-the-sqlite-schema-without-a-sqlite-driver.md` puts a dependency edge out of
//! reach: it would rewrite `Cargo.lock`, which this lane may not write.
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

use mesh_cas::{Blake3, Blake3Hasher, ContentDigest, DigestHasher};

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
            "cannot read {} ({error}). mesh-cas holds its copy of BLAKE3 against mesh-types by \
             reading its source, because no dependency edge is permitted; if that file has moved, \
             the copy is unchecked until this path is corrected.",
            path.display()
        )
    })
}

/// Source text with documentation, comments and blank lines removed.
///
/// The two copies differ in their documentation on purpose — one explains why it is the original,
/// the other why it is a copy, and the doctest in each names its own crate. Neither difference can
/// change a digest, so the comparison is over code and the tests below are what cover the rest.
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
    assert!(
        vectors.len() >= MINIMUM_VECTORS,
        "only {} reference vectors were parsed out of mesh-types' table; the table's shape has \
         changed and this test is no longer comparing anything",
        vectors.len()
    );
    vectors
}

/// The published test-vector input pattern: byte *i* is *i* mod 251.
fn pattern(length: usize) -> Vec<u8> {
    (0..length).map(|index| (index % 251) as u8).collect()
}

#[test]
fn this_crate_reproduces_the_reference_vectors() {
    for (length, expected) in reference_vectors() {
        assert_eq!(
            Blake3::digest_bytes(&pattern(length)).to_hex(),
            expected,
            "the {length}-byte reference vector"
        );
    }
}

/// The table is only evidence if a broken hasher would fail it: a hasher that ignored its input
/// would still have to produce a different digest for a different input.
#[test]
fn one_flipped_byte_changes_every_reference_vector() {
    for (length, expected) in reference_vectors() {
        if length == 0 {
            continue;
        }
        let mut input = pattern(length);
        input[length - 1] ^= 0x01;
        assert_ne!(
            Blake3::digest_bytes(&input).to_hex(),
            expected,
            "flipping the last byte of the {length}-byte vector did not change its digest"
        );
    }
}

/// Streaming must give the same answer as one shot, at every split. A chunk is hashed twice on the
/// promotion path — once from the caller's buffer, once from the file read back — and if those two
/// could differ, verification would be checking the wrong thing.
#[test]
fn streaming_in_any_split_matches_the_one_shot_digest() {
    let splits = [1usize, 63, 64, 65, 512, 1023, 1024, 1025, 2048, 4096];
    for (length, expected) in reference_vectors() {
        let input = pattern(length);
        for split in splits {
            if split > length {
                continue;
            }
            let mut hasher = Blake3Hasher::new();
            for piece in input.chunks(split) {
                hasher.update(piece);
            }
            assert_eq!(
                hasher.finalize().to_hex(),
                expected,
                "the {length}-byte vector streamed in {split}-byte pieces"
            );
        }
    }
}
