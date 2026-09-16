//! Acceptance criterion 3: materializing the same operation set twice produces byte-identical
//! state.
//!
//! Asserted on the **bytes**, not on a digest. A hash comparison answers this question with a
//! probability that depends on an algorithm this crate deliberately does not ship; the canonical
//! encoding answers it with the bytes. The digest is checked separately, and only for the property
//! it is responsible for — being a function of those bytes.

mod common;

use common::{corpus, digest};
use mesh_materializer::{materialize, WorkspaceState};

/// How many seeds these run over. `MESH_MATERIALIZER_CORPUS` raises it to the full corpus; see
/// `tests/common/corpus.rs` for why the default is a deterministic sample.
const DEFAULT_SEEDS: u64 = 1_500;

#[test]
fn materializing_the_same_set_twice_is_byte_identical() {
    for seed in 0..corpus::corpus_size(DEFAULT_SEEDS) {
        let generated = corpus::generate(seed);
        let once = materialize(generated.root, &generated.changesets);
        let twice = materialize(generated.root, &generated.changesets);
        assert_eq!(
            once.state().canonical_bytes(),
            twice.state().canonical_bytes(),
            "seed {seed} materialized to two different states"
        );
        assert_eq!(once, twice, "seed {seed} produced two different results");
    }
}

#[test]
fn a_second_materialization_reaches_the_same_rejections_in_the_same_order() {
    for seed in 0..corpus::corpus_size(DEFAULT_SEEDS) {
        let generated = corpus::generate(seed);
        let once = materialize(generated.root, &generated.changesets);
        let twice = materialize(generated.root, &generated.changesets);
        assert_eq!(once.rejections(), twice.rejections(), "seed {seed}");
        assert_eq!(once.order(), twice.order(), "seed {seed}");
    }
}

#[test]
fn the_state_hash_is_a_function_of_the_canonical_bytes() {
    for seed in 0..corpus::corpus_size(DEFAULT_SEEDS) {
        let generated = corpus::generate(seed);
        let state = materialize(generated.root, &generated.changesets);
        let bytes = state.state().canonical_bytes();
        assert_eq!(
            state.state().state_hash::<digest::TestDigest>(),
            digest::digest_of(&bytes),
            "seed {seed}: streaming the encoding into the digest and digesting the buffer disagree"
        );
    }
}

/// Two different states must not share an encoding. Over the corpus this is checked the only way it
/// can be checked without a cryptographic argument: every distinct state seen is kept, and a
/// repeated encoding must come with an equal state.
#[test]
fn one_encoding_never_stands_for_two_states() {
    let mut seen: Vec<(Vec<u8>, WorkspaceState)> = Vec::new();
    for seed in 0..400 {
        let generated = corpus::generate(seed);
        let state = materialize(generated.root, &generated.changesets)
            .state()
            .clone();
        let bytes = state.canonical_bytes();
        if let Some((_, held)) = seen.iter().find(|(encoding, _)| *encoding == bytes) {
            assert_eq!(
                held, &state,
                "seed {seed} encodes to bytes another state already claims"
            );
        } else {
            seen.push((bytes, state));
        }
    }
    assert!(
        seen.len() > 100,
        "only {} distinct states over 400 seeds, so this proves very little",
        seen.len()
    );
}
