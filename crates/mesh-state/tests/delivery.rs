//! The delivery campaign: reorder it, duplicate it, drop one, and require one head.
//!
//! `tests/heads.rs` asserts each acceptance criterion once, on a fixture written to show what the
//! criterion means. This file tries to break the same criteria by search: several generated
//! concurrent histories, each delivered in many independently shuffled orders, with duplicates
//! injected. Plan §13.3's duplicate and reorder rows are what it is standing in for.
//!
//! Every campaign names its seeds as literals. A failure here is reproducible by rerunning the
//! same test, and a seed that ever fails belongs in the fixture list in `heads.rs` as a named
//! regression rather than staying a number in a loop.
//!
//! **Scope note.** The task's automated validation names
//! `cargo nextest run -p mesh-simulator --test delivery`. `mesh-simulator` is outside this task's
//! allowed paths and, having no dependency edge to `mesh-state`, could not exercise this fold even
//! if it were written. That mismatch is escalated in the pull request rather than papered over;
//! this file is the delivery campaign, run against the crate that owns the behaviour.

mod common;

use std::collections::BTreeSet;

use common::{actor, changeset_id, deliver_all, generate_history, Advancement, History, Shuffler};
use mesh_state::{ChangeSetId, DeliveredChangeSet, Reception};

/// The seeds every campaign runs. Small, named, and reproducible.
const SEEDS: [u64; 6] = [1, 2, 3, 5, 8, 13];

/// How many shuffled orders each campaign tries per history.
const ORDERS: usize = 8;

fn history(seed: u64) -> History {
    generate_history(seed, 3, 25)
}

#[test]
fn every_delivery_order_reaches_one_head() {
    for seed in SEEDS {
        let history = history(seed);
        for order in 0..ORDERS {
            let mut stream = history.changesets.clone();
            Shuffler::new(seed * 1_000 + order as u64).shuffle(&mut stream);

            let receiver = deliver_all(Advancement::new(actor(9)), &stream);
            assert_eq!(
                receiver.head(),
                history.head,
                "seed {seed}, order {order}: delivery order reached the head"
            );
            assert!(
                receiver.known_missing().is_empty(),
                "seed {seed}, order {order}: work was still held after the whole set arrived"
            );
            assert_eq!(receiver.applied().len(), history.changesets.len());
        }
    }
}

#[test]
fn duplicated_delivery_reaches_the_same_head() {
    for seed in SEEDS {
        let history = history(seed);
        let mut shuffler = Shuffler::new(seed * 7 + 1);

        // Every ChangeSet between one and four times, then the whole stream shuffled.
        let mut stream: Vec<DeliveredChangeSet> = Vec::new();
        for changeset in &history.changesets {
            for _ in 0..=shuffler.below(4) {
                stream.push(changeset.clone());
            }
        }
        shuffler.shuffle(&mut stream);
        assert!(stream.len() >= history.changesets.len());

        let receiver = deliver_all(Advancement::new(actor(9)), &stream);
        assert_eq!(
            receiver.head(),
            history.head,
            "seed {seed}: duplication moved the head"
        );
        assert_eq!(receiver.applied().len(), history.changesets.len());
    }
}

/// Idempotence stated as a count rather than as a head comparison: a duplicate must be *answered*
/// without being applied, not applied a second time to the same effect.
#[test]
fn a_duplicate_is_answered_rather_than_reapplied() {
    let history = history(3);
    let full = deliver_all(Advancement::new(actor(9)), &history.changesets);

    let mut receiver = full.clone();
    let mut duplicates = 0;
    for changeset in &history.changesets {
        let (advanced, reception) = receiver.deliver(changeset.clone());
        assert_eq!(reception, Reception::AlreadyApplied);
        assert!(!reception.changed_something());
        duplicates += 1;
        receiver = advanced;
    }
    assert_eq!(duplicates, history.changesets.len());
    assert_eq!(receiver, full, "a duplicate changed some part of the state");
}

#[test]
fn reversed_delivery_reaches_the_same_head() {
    for seed in SEEDS {
        let history = history(seed);
        let mut reversed = history.changesets.clone();
        reversed.reverse();

        let receiver = deliver_all(Advancement::new(actor(9)), &reversed);
        assert_eq!(receiver.head(), history.head, "seed {seed}");
        assert!(receiver.known_missing().is_empty());
    }
}

/// The property with the actors put back in: several actors, each receiving the same causal set in
/// its own shuffled order, must end at one head. Serial delivery to one actor is a weaker claim.
#[test]
fn actors_receiving_independently_shuffled_streams_agree() {
    for seed in SEEDS {
        let history = history(seed);
        let mut heads = BTreeSet::new();

        for tag in 1..=5u8 {
            let mut stream = history.changesets.clone();
            Shuffler::new(seed * 31 + u64::from(tag)).shuffle(&mut stream);
            let receiver = deliver_all(Advancement::new(actor(tag)), &stream);
            heads.insert(receiver.head());
            assert_eq!(receiver.applied(), {
                let reference = deliver_all(Advancement::new(actor(0xfe)), &history.changesets);
                reference.applied()
            });
        }

        assert_eq!(
            heads.len(),
            1,
            "seed {seed}: five actors, {} heads",
            heads.len()
        );
        assert!(heads.contains(&history.head));
    }
}

/// Nothing is dropped, in the strong sense: every identifier that was delivered is either applied
/// or held, and the held set is empty once the whole causal set has arrived.
#[test]
fn nothing_delivered_is_ever_dropped() {
    for seed in SEEDS {
        let history = history(seed);
        let mut stream = history.changesets.clone();
        Shuffler::new(seed).shuffle(&mut stream);

        let mut receiver = Advancement::new(actor(9));
        let mut delivered: BTreeSet<ChangeSetId> = BTreeSet::new();
        for changeset in &stream {
            let (advanced, _) = receiver.deliver(changeset.clone());
            receiver = advanced;
            delivered.insert(changeset.id());

            let held: BTreeSet<ChangeSetId> = receiver
                .known_missing()
                .iter()
                .map(|known| known.waiting())
                .collect();
            let applied: BTreeSet<ChangeSetId> = receiver.applied().into_iter().collect();
            let accounted: BTreeSet<ChangeSetId> = applied.union(&held).copied().collect();
            assert_eq!(
                accounted, delivered,
                "seed {seed}: something delivered was neither applied nor held"
            );
        }
    }
}

/// The failure and recovery contract: a causal parent that never arrives holds its children
/// forever, visibly, and the head stops exactly where the arrived work ends.
#[test]
fn one_withheld_changeset_holds_its_children_and_moves_no_head() {
    for seed in SEEDS {
        let history = history(seed);
        let withheld = history.changesets[history.changesets.len() / 2].id();
        let mut stream: Vec<DeliveredChangeSet> = history
            .changesets
            .iter()
            .filter(|changeset| changeset.id() != withheld)
            .cloned()
            .collect();
        Shuffler::new(seed * 97).shuffle(&mut stream);

        let receiver = deliver_all(Advancement::new(actor(9)), &stream);

        assert!(!receiver.has_applied(&withheld));
        assert_ne!(
            receiver.head(),
            history.head,
            "seed {seed}: the head reached a state whose work never arrived"
        );

        // Every held ChangeSet waits either on the withheld one or on another held one: the held
        // set is exactly the withheld ChangeSet's descendants, with nothing held for any other
        // reason and nothing that should have been held missing from it.
        let held = receiver.known_missing();
        let waiting: BTreeSet<ChangeSetId> = held.iter().map(|known| known.waiting()).collect();
        for known in &held {
            assert!(!known.missing().is_empty(), "seed {seed}: held for nothing");
            for missing in known.missing() {
                assert!(
                    *missing == withheld || waiting.contains(missing),
                    "seed {seed}: {} waits on {missing}, which is neither withheld nor held",
                    known.waiting()
                );
            }
        }

        // And the moment it arrives, everything held drains and the head lands where it should.
        let (recovered, reception) = receiver.deliver(
            history
                .changesets
                .iter()
                .find(|changeset| changeset.id() == withheld)
                .expect("the withheld ChangeSet")
                .clone(),
        );
        let Reception::Applied {
            applied, refused, ..
        } = &reception
        else {
            panic!("seed {seed}: {reception:?}");
        };
        assert!(refused.is_empty());
        assert_eq!(applied.len(), held.len() + 1);
        assert_eq!(recovered.head(), history.head, "seed {seed}");
        assert!(recovered.known_missing().is_empty());
    }
}

/// A head is only reached by applying the work behind it. Without this, a head that were constant,
/// or that ignored most of its input, would pass every convergence test above.
#[test]
fn no_proper_prefix_of_the_stream_reaches_the_final_head() {
    let history = history(5);
    let mut receiver = Advancement::new(actor(9));

    for (index, changeset) in history.changesets.iter().enumerate() {
        assert_ne!(
            receiver.head(),
            history.head,
            "the final head was reached after {index} of {} ChangeSets",
            history.changesets.len()
        );
        let (advanced, _) = receiver.deliver(changeset.clone());
        receiver = advanced;
    }
    assert_eq!(receiver.head(), history.head);
}

#[test]
fn a_wrong_clock_moves_no_head_under_any_delivery_order() {
    for seed in SEEDS {
        let history = history(seed);
        let mut shuffler = Shuffler::new(seed * 1_009);

        let mut stream: Vec<DeliveredChangeSet> = history
            .changesets
            .iter()
            .map(|changeset| {
                let millis = shuffler.next_u64();
                let counter = u32::try_from(shuffler.next_u64() % u64::from(u32::MAX)).unwrap_or(0);
                changeset.clone().with_hybrid_logical_time(millis, counter)
            })
            .collect();
        shuffler.shuffle(&mut stream);

        let receiver = deliver_all(Advancement::new(actor(9)), &stream);
        assert_eq!(
            receiver.head(),
            history.head,
            "seed {seed}: a randomised hybrid logical time reached the head"
        );
    }
}

/// A history built from one actor is a chain; a history built from several is not. If the
/// generator ever stopped producing merges, every convergence claim above would quietly narrow to
/// the serial case, so the generator's own output is checked.
#[test]
fn the_generated_histories_are_actually_concurrent() {
    for seed in SEEDS {
        let history = history(seed);
        let merges = history
            .changesets
            .iter()
            .filter(|changeset| changeset.parents().len() > 1)
            .count();
        let authors: BTreeSet<_> = history
            .changesets
            .iter()
            .map(DeliveredChangeSet::actor)
            .collect();
        let genesis = history
            .changesets
            .iter()
            .filter(|changeset| changeset.parents().is_genesis())
            .count();
        assert_eq!(
            genesis, 3,
            "seed {seed}: the concurrent opening did not happen"
        );
        assert!(
            merges > 0,
            "seed {seed}: no ChangeSet names two causal parents. A merge exists only when two \
             ChangeSets were concurrent — neither an ancestor of the other — so without one, \
             every claim above narrows to the serial case"
        );
        assert!(
            authors.len() > 1,
            "seed {seed}: one author, so nothing was ever concurrent"
        );
    }
}

/// The identifier space each seed occupies is disjoint from every other seed's and from the
/// hand-written fixtures, which is what stops one campaign's ChangeSet from silently answering
/// another's missing parent.
#[test]
fn seeds_do_not_share_identifiers() {
    let mut seen: BTreeSet<ChangeSetId> = (1..=100).map(changeset_id).collect();
    for seed in SEEDS {
        for changeset in &history(seed).changesets {
            assert!(
                seen.insert(changeset.id()),
                "seed {seed} reused an identifier another campaign already used"
            );
        }
    }
}
