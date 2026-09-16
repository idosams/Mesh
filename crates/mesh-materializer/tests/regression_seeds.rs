//! Every divergence the corpus has found, pinned by its seed.
//!
//! The task's failure-and-recovery rule: *a divergence from the oracle is always a bug in one of
//! them — never resolve it by changing the assertion*. This file is the other half of that rule.
//! A seed that once diverged runs here forever, at full strength, whatever the sampled corpus size
//! is set to, and with a sentence saying what was wrong and which side was wrong.
//!
//! # The seeds
//!
//! | Seed | What it found | Which side was wrong |
//! |---|---|---|
//! | 48 | Two records arriving under one ChangeSet identifier with different operations. `materialize` took the first occurrence in the delivered slice, so two peers that received the pair in different orders held different states. | The implementation. A ChangeSet identifier is a content digest, so the two records contradict each other and neither can be believed on arrival order; `src/order.rs` now keeps the least record under `AppliedChangeSet`'s own ordering, which every peer computes identically. |
//! | 9 | The same defect reached through the causal *order* rather than through the operations: the duplicate record's causal parents changed which ChangeSet settled first. | The implementation, same cause. `causal_order` and `materialize` now share one `canonical_records` pass. |

mod common;

use common::{corpus, oracle, snapshot};
use mesh_materializer::materialize;

/// Seeds that once diverged. Never shrinks.
const REGRESSION_SEEDS: [u64; 2] = [48, 9];

#[test]
fn every_regression_seed_still_agrees_with_the_oracle() {
    for seed in REGRESSION_SEEDS {
        let generated = corpus::generate(seed);
        let mine =
            snapshot::of_materialization(&materialize(generated.root, &generated.changesets));
        let theirs = oracle::materialize(generated.root, &generated.changesets);
        assert_eq!(
            mine.first_difference(&theirs),
            None,
            "seed {seed} diverged again"
        );
    }
}

#[test]
fn every_regression_seed_is_order_insensitive() {
    for seed in REGRESSION_SEEDS {
        let generated = corpus::generate(seed);
        let expected = materialize(generated.root, &generated.changesets)
            .state()
            .canonical_bytes();
        for shuffle in 0..16 {
            let delivered = corpus::shuffled(&generated, seed.wrapping_mul(7919) + shuffle);
            assert_eq!(
                materialize(generated.root, &delivered)
                    .state()
                    .canonical_bytes(),
                expected,
                "seed {seed}, shuffle {shuffle}"
            );
        }
    }
}

/// Seed 48's defect, isolated from the generator so it stays readable: one identifier, two
/// contradictory records, delivered both ways round.
#[test]
fn two_records_under_one_identifier_resolve_the_same_way_in_both_delivery_orders() {
    use mesh_materializer::{AppliedChangeSet, ChangeSetId, ObjectId, Operation};

    let root = ObjectId::from_bytes([0; 16]);
    let id = ChangeSetId::from_bytes([5; 32]);
    let one = AppliedChangeSet::genesis(
        id,
        vec![Operation::CreateFile {
            object_id: ObjectId::from_bytes([1; 16]),
        }],
    );
    let other = AppliedChangeSet::genesis(
        id,
        vec![Operation::CreateDirectory {
            object_id: ObjectId::from_bytes([2; 16]),
        }],
    );

    let forwards = materialize(root, &[one.clone(), other.clone()]);
    let backwards = materialize(root, &[other, one]);
    assert_eq!(
        forwards.state().canonical_bytes(),
        backwards.state().canonical_bytes()
    );
    // One record contributed, not both: the pair is one ChangeSet, not two.
    assert_eq!(forwards.state().objects().len(), 2);
    assert_eq!(forwards.reached(), 1);
}
