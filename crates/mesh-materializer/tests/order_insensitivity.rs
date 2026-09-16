//! Acceptance criterion 1: equivalent operation sets in any valid order materialize to identical
//! state, verified by hash — and, more strongly, by the bytes the hash is taken over.
//!
//! # What "any valid order" means here
//!
//! Causality is tracked at ChangeSet granularity (`docs/protocol.md` §2.2): the operations *inside*
//! a ChangeSet are an ordered set its author sealed, and reordering those would be authoring a
//! different ChangeSet rather than delivering the same one differently. So the free variable is the
//! **delivery order of the ChangeSets**, which is what a peer actually varies, and that is what is
//! shuffled below — four independent shuffles per set, plus the exact reversal, which is the
//! adversarial case for any implementation that quietly folds in slice order.

mod common;

use common::{corpus, digest};
use mesh_materializer::materialize;

/// How many seeds these run over. `MESH_MATERIALIZER_CORPUS` raises it to the full corpus; see
/// `tests/common/corpus.rs` for why the default is a deterministic sample.
const DEFAULT_SEEDS: u64 = 1_500;

#[test]
fn a_shuffled_delivery_order_materializes_to_the_same_bytes() {
    for seed in 0..corpus::corpus_size(DEFAULT_SEEDS) {
        let generated = corpus::generate(seed);
        let expected = materialize(generated.root, &generated.changesets)
            .state()
            .canonical_bytes();

        for shuffle in 0..4 {
            let delivered =
                corpus::shuffled(&generated, seed.wrapping_mul(31).wrapping_add(shuffle));
            let materialized = materialize(generated.root, &delivered);
            assert_eq!(
                materialized.state().canonical_bytes(),
                expected,
                "seed {seed}, shuffle {shuffle}: one causal set materialized to two states"
            );
        }
    }
}

#[test]
fn reversing_the_delivery_order_changes_nothing() {
    for seed in 0..corpus::corpus_size(DEFAULT_SEEDS) {
        let generated = corpus::generate(seed);
        let mut reversed = generated.changesets.clone();
        reversed.reverse();
        assert_eq!(
            materialize(generated.root, &reversed)
                .state()
                .canonical_bytes(),
            materialize(generated.root, &generated.changesets)
                .state()
                .canonical_bytes(),
            "seed {seed}"
        );
    }
}

/// The criterion says "verified by hash", so it is also verified by hash.
#[test]
fn a_shuffled_delivery_order_produces_the_same_state_hash() {
    for seed in 0..500 {
        let generated = corpus::generate(seed);
        let expected = materialize(generated.root, &generated.changesets)
            .state()
            .state_hash::<digest::TestDigest>();
        let delivered = corpus::shuffled(&generated, seed ^ 0x5f5f);
        assert_eq!(
            materialize(generated.root, &delivered)
                .state()
                .state_hash::<digest::TestDigest>(),
            expected,
            "seed {seed}"
        );
    }
}

/// The rejections are part of what a caller sees, so they are order-insensitive too — otherwise two
/// peers would agree about the state and disagree about what the set asked for.
#[test]
fn the_refusals_do_not_depend_on_delivery_order() {
    for seed in 0..corpus::corpus_size(DEFAULT_SEEDS) {
        let generated = corpus::generate(seed);
        let expected = materialize(generated.root, &generated.changesets);
        let delivered = corpus::shuffled(&generated, seed ^ 0xabcd);
        let materialized = materialize(generated.root, &delivered);
        assert_eq!(
            materialized.rejections(),
            expected.rejections(),
            "seed {seed}"
        );
        assert_eq!(materialized.order(), expected.order(), "seed {seed}");
    }
}

/// A delivery order that repeats a ChangeSet is still the same causal set. Re-delivery is normal —
/// `mesh-state` answers a known identifier `AlreadyApplied` — and it must not move a state.
#[test]
fn delivering_a_change_set_four_times_changes_nothing() {
    for seed in 0..400 {
        let generated = corpus::generate(seed);
        let expected = materialize(generated.root, &generated.changesets)
            .state()
            .canonical_bytes();

        let mut repeated = Vec::new();
        for changeset in &generated.changesets {
            for _ in 0..4 {
                repeated.push(changeset.clone());
            }
        }
        let delivered = {
            let mut held = repeated;
            common::rng::Rng::new(seed).shuffle(&mut held);
            held
        };
        assert_eq!(
            materialize(generated.root, &delivered)
                .state()
                .canonical_bytes(),
            expected,
            "seed {seed}"
        );
    }
}
