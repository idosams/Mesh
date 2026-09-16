//! The four acceptance criteria of actor head advancement, one test each, plus the refusals that
//! keep them true.
//!
//! Every test here is about a single named property. The order-and-duplication campaigns that
//! search for a counterexample live in `tests/delivery.rs`.

mod common;

use common::{actor, changeset_id, deliver_all, generate_history, Advancement, TestDigest};
use mesh_state::{
    CausalParents, ChangeSetId, DeliveredChangeSet, HeadDigest, HeadId, HeadState, Reception,
    Refusal, HEAD_DOMAIN,
};

/// Build a three-ChangeSet chain authored by one actor.
fn chain(length: u64) -> (Advancement, Vec<DeliveredChangeSet>) {
    let mut advancement = Advancement::new(actor(1));
    let mut authored = Vec::new();
    for sequence in 1..=length {
        let (advanced, changeset) = advancement
            .author(changeset_id(sequence))
            .expect("authored");
        advancement = advanced;
        authored.push(changeset);
    }
    (advancement, authored)
}

#[test]
fn an_actor_that_has_applied_nothing_still_has_a_head() {
    let empty = Advancement::new(actor(1));
    let also_empty = Advancement::new(actor(2));
    assert_eq!(
        empty.head(),
        also_empty.head(),
        "the head of the empty causal set is a property of the set, not of the actor"
    );
    assert!(empty.applied().is_empty());
    assert!(empty.tips().is_empty());
}

#[test]
fn a_genesis_changeset_is_authored_against_the_empty_head() {
    let empty = Advancement::new(actor(1));
    let (advanced, first) = empty.author(changeset_id(1)).expect("authored");

    assert!(
        first.parents().is_genesis(),
        "the first ChangeSet has no history to follow"
    );
    assert_eq!(first.base_head(), empty.head());
    assert_eq!(advanced.head(), first.resulting_head());
    assert_ne!(advanced.head(), empty.head(), "the head did not move");
}

// ---------------------------------------------------------------------------
// Acceptance criterion: reapplying a delivered ChangeSet has no additional effect.
// ---------------------------------------------------------------------------

#[test]
fn reapplying_a_delivered_changeset_changes_nothing() {
    let (_, authored) = chain(3);
    let full = deliver_all(Advancement::new(actor(9)), &authored);

    for changeset in &authored {
        let (again, reception) = full.deliver(changeset.clone());
        assert_eq!(reception, Reception::AlreadyApplied);
        assert!(!reception.changed_something());
        assert_eq!(again.head(), full.head(), "the head moved on a duplicate");
        assert_eq!(
            again, full,
            "some other part of the state moved on a duplicate"
        );
    }
}

#[test]
fn a_duplicate_of_a_buffered_changeset_is_answered_without_buffering_it_twice() {
    let (_, authored) = chain(2);
    let (waiting, first) = Advancement::new(actor(9)).deliver(authored[1].clone());
    assert!(matches!(first, Reception::Buffered { .. }));

    let (again, second) = waiting.deliver(authored[1].clone());
    assert_eq!(second, Reception::AlreadyBuffered);
    assert_eq!(again, waiting);
    assert_eq!(again.known_missing().len(), 1);
}

// ---------------------------------------------------------------------------
// Acceptance criterion: a child arriving before its parent is buffered and applied once the
// parent arrives, never dropped.
// ---------------------------------------------------------------------------

#[test]
fn a_child_that_arrives_before_its_parent_is_buffered_and_then_applied() {
    let (author, authored) = chain(3);
    let mut receiver = Advancement::new(actor(9));

    // Deliver the chain backwards: every ChangeSet but the last one to arrive is early.
    for changeset in authored.iter().rev() {
        let (advanced, reception) = receiver.deliver(changeset.clone());
        receiver = advanced;
        if changeset.id() == authored[0].id() {
            let Reception::Applied {
                applied, refused, ..
            } = &reception
            else {
                panic!("the parent's arrival should have applied the whole chain: {reception:?}");
            };
            assert!(refused.is_empty());
            assert_eq!(applied.len(), 3, "the buffered children were not drained");
        } else {
            assert!(
                matches!(reception, Reception::Buffered { .. }),
                "{reception:?}"
            );
        }
    }

    assert_eq!(receiver.head(), author.head());
    assert!(receiver.known_missing().is_empty());
    assert_eq!(receiver.applied().len(), 3);
}

#[test]
fn a_parent_that_never_arrives_leaves_its_child_visible_and_held() {
    let (_, authored) = chain(2);
    let (receiver, reception) = Advancement::new(actor(9)).deliver(authored[1].clone());

    let Reception::Buffered { missing } = reception else {
        panic!("a ChangeSet with an unknown causal parent must buffer: {reception:?}");
    };
    assert_eq!(missing, vec![authored[0].id()]);

    let known = receiver.known_missing();
    assert_eq!(known.len(), 1);
    assert_eq!(known[0].waiting(), authored[1].id());
    assert_eq!(known[0].missing(), &[authored[0].id()]);

    // Nothing this type can be asked to do collects it. Delivering unrelated work does not either.
    let unrelated = generate_history(7, 2, 6);
    let later = deliver_all(receiver, &unrelated.changesets);
    assert_eq!(
        later.known_missing().len(),
        1,
        "a held ChangeSet was collected"
    );
    assert!(!later.has_applied(&authored[1].id()));
}

#[test]
fn a_merge_waits_for_every_causal_parent_and_names_the_ones_it_lacks() {
    let (ido, ido_first) = Advancement::new(actor(1)).author(changeset_id(1)).unwrap();
    let (_, agent_first) = Advancement::new(actor(2)).author(changeset_id(2)).unwrap();

    // Ido learns of the agent's work and authors a merge over both.
    let (ido, _) = ido.deliver(agent_first.clone());
    let (_, merge) = ido.author(changeset_id(3)).unwrap();
    assert_eq!(
        merge.parents().len(),
        2,
        "a merge names both causal parents"
    );

    // A receiver holding only one of the two parents holds the merge and names the other.
    let receiver = deliver_all(Advancement::new(actor(9)), &[ido_first]);
    let (receiver, reception) = receiver.deliver(merge.clone());
    let Reception::Buffered { missing } = reception else {
        panic!("{reception:?}");
    };
    assert_eq!(missing, vec![agent_first.id()]);

    let (receiver, reception) = receiver.deliver(agent_first);
    let Reception::Applied { applied, .. } = &reception else {
        panic!("{reception:?}");
    };
    assert!(applied.contains(&merge.id()), "the merge was not drained");
    assert!(receiver.known_missing().is_empty());
}

// ---------------------------------------------------------------------------
// Acceptance criterion: head advancement never consults wall-clock time.
// ---------------------------------------------------------------------------

#[test]
fn a_wrong_clock_moves_no_head() {
    let (_, authored) = chain(4);
    let honest = deliver_all(Advancement::new(actor(9)), &authored);

    // What a machine with a badly-set clock sends: an epoch that never advances, one that runs
    // backwards, and one at the far end of the range.
    let clocks: [(u64, u32); 4] = [(0, 0), (u64::MAX, u32::MAX), (1, 7), (0, u32::MAX)];
    let skewed: Vec<DeliveredChangeSet> = authored
        .iter()
        .enumerate()
        .map(|(index, changeset)| {
            let (millis, counter) = clocks[index % clocks.len()];
            changeset.clone().with_hybrid_logical_time(millis, counter)
        })
        .collect();
    let confused = deliver_all(Advancement::new(actor(9)), &skewed);

    assert_eq!(
        confused.head(),
        honest.head(),
        "the hybrid logical time reached the head"
    );
    assert_eq!(confused.applied(), honest.applied());
}

#[test]
fn a_clock_running_backwards_across_a_whole_history_moves_no_head() {
    let history = generate_history(11, 3, 24);
    let forwards = deliver_all(Advancement::new(actor(9)), &history.changesets);

    let backwards: Vec<DeliveredChangeSet> = history
        .changesets
        .iter()
        .enumerate()
        .map(|(index, changeset)| {
            let millis = u64::MAX - u64::try_from(index).unwrap_or(0);
            changeset.clone().with_hybrid_logical_time(millis, 0)
        })
        .collect();
    let reversed_clock = deliver_all(Advancement::new(actor(9)), &backwards);

    assert_eq!(reversed_clock.head(), forwards.head());
    assert_eq!(forwards.head(), history.head);
}

// ---------------------------------------------------------------------------
// Acceptance criterion: two actors applying the same causal set reach the same head.
// ---------------------------------------------------------------------------

#[test]
fn two_actors_applying_the_same_causal_set_reach_the_same_head() {
    let history = generate_history(3, 3, 20);
    let ido = deliver_all(Advancement::new(actor(1)), &history.changesets);

    let mut reversed = history.changesets.clone();
    reversed.reverse();
    let agent = deliver_all(Advancement::new(actor(2)), &reversed);

    assert_eq!(ido.head(), agent.head(), "delivery order reached the head");
    assert_eq!(ido.applied(), agent.applied());
    assert_ne!(
        ido.actor(),
        agent.actor(),
        "the two actors are different actors"
    );
}

#[test]
fn concurrent_actors_converge_once_each_has_the_other_half() {
    let (ido, ido_first) = Advancement::new(actor(1)).author(changeset_id(1)).unwrap();
    let (agent, agent_first) = Advancement::new(actor(2)).author(changeset_id(2)).unwrap();

    assert_ne!(
        ido.head(),
        agent.head(),
        "two concurrent actors are not already agreed"
    );
    assert!(
        ido_first.parents().is_genesis() && agent_first.parents().is_genesis(),
        "both authored from the empty head, so neither follows the other"
    );

    let (ido, _) = ido.deliver(agent_first);
    let (agent, _) = agent.deliver(ido_first);

    assert_eq!(ido.head(), agent.head(), "concurrent work did not converge");
    assert_eq!(
        ido.tips().len(),
        2,
        "two concurrent tips, neither ordered after the other"
    );
}

// ---------------------------------------------------------------------------
// The refusals: a head never advances to a state the receiver did not derive.
// ---------------------------------------------------------------------------

#[test]
fn a_dropped_causal_parent_is_refused() {
    let (ido, ido_first) = Advancement::new(actor(1)).author(changeset_id(1)).unwrap();
    let (_, agent_first) = Advancement::new(actor(2)).author(changeset_id(2)).unwrap();
    let (ido, _) = ido.deliver(agent_first.clone());
    let (_, merge) = ido.author(changeset_id(3)).unwrap();
    assert_eq!(merge.parents().len(), 2);

    // The same ChangeSet, the same claimed heads, one causal parent removed. Without the base-head
    // derivation this would simply apply, and the two actors' histories would stop being relatable.
    let kept = merge.parents().as_slice()[0];
    let dropped = merge
        .clone()
        .following(CausalParents::after(kept, Vec::new()));

    let receiver = deliver_all(Advancement::new(actor(9)), &[ido_first, agent_first]);
    let (unchanged, reception) = receiver.deliver(dropped);

    let Reception::Refused(Refusal::BaseHeadNotDerived {
        claimed, derived, ..
    }) = reception
    else {
        panic!("a dropped causal parent must be refused: {reception:?}");
    };
    assert_ne!(claimed, derived);
    assert_eq!(unchanged, receiver, "a refusal changed the receiver");
}

/// A redundant causal parent is invisible to the head — it names a causal set the other parent
/// already covers — so it has to be refused structurally or not at all. It is refused, because the
/// parent list is bound into the ChangeSet's own identifier and one transition may have one name.
#[test]
fn a_causal_parent_another_parent_already_follows_is_refused() {
    let (_, authored) = chain(3);
    let receiver = deliver_all(Advancement::new(actor(9)), &authored[..2]);

    let extra = authored[2].clone().following(CausalParents::after(
        authored[1].id(),
        vec![authored[0].id()],
    ));
    let (unchanged, reception) = receiver.deliver(extra);

    assert_eq!(
        reception,
        Reception::Refused(Refusal::ParentImplied {
            changeset: authored[2].id(),
            parent: authored[0].id(),
            implied_by: authored[1].id(),
        }),
        "a redundant causal parent was accepted"
    );
    assert_eq!(unchanged.head(), receiver.head());
    assert_eq!(unchanged, receiver);
}

/// The redundant parent really is invisible to the head derivation, which is why the check above
/// cannot be folded into the base-head comparison. Asserted rather than argued.
#[test]
fn a_redundant_causal_parent_does_not_move_the_derived_base_head() {
    let (_, authored) = chain(3);
    let extra = authored[2].clone().following(CausalParents::after(
        authored[1].id(),
        vec![authored[0].id()],
    ));
    assert_eq!(
        extra.base_head(),
        authored[2].base_head(),
        "if these differed, BaseHeadNotDerived would already have caught it"
    );
}

#[test]
fn a_claimed_resulting_head_the_receiver_cannot_derive_is_refused() {
    let (_, authored) = chain(2);
    let receiver = deliver_all(Advancement::new(actor(9)), &authored[..1]);

    let invented = HeadId::from_bytes([0xee; 32]);
    let lying = authored[1]
        .clone()
        .claiming(authored[1].base_head(), invented);
    let (unchanged, reception) = receiver.deliver(lying);

    let Reception::Refused(Refusal::ResultingHeadNotDerived {
        claimed, derived, ..
    }) = reception
    else {
        panic!("an asserted head must not be believed: {reception:?}");
    };
    assert_eq!(claimed, invented);
    assert_eq!(derived, authored[1].resulting_head());
    assert_eq!(
        unchanged.head(),
        receiver.head(),
        "the head advanced anyway"
    );
}

#[test]
fn one_flipped_byte_in_a_claimed_head_is_refused() {
    let (_, authored) = chain(2);
    let receiver = deliver_all(Advancement::new(actor(9)), &authored[..1]);

    let mut bytes = *authored[1].resulting_head().as_bytes();
    bytes[31] ^= 0x01;
    let nearly = authored[1]
        .clone()
        .claiming(authored[1].base_head(), HeadId::from_bytes(bytes));

    let (_, reception) = receiver.deliver(nearly);
    assert!(
        matches!(
            reception,
            Reception::Refused(Refusal::ResultingHeadNotDerived { .. })
        ),
        "{reception:?}"
    );
}

#[test]
fn a_changeset_that_follows_itself_is_refused_before_it_is_ever_held() {
    let id = changeset_id(1);
    let impossible = DeliveredChangeSet::new(
        id,
        actor(1),
        CausalParents::after(id, Vec::new()),
        HeadId::from_bytes([0; 32]),
        HeadId::from_bytes([1; 32]),
    );
    let (unchanged, reception) = Advancement::new(actor(9)).deliver(impossible);

    assert_eq!(
        reception,
        Reception::Refused(Refusal::SelfParent { changeset: id })
    );
    assert!(
        unchanged.known_missing().is_empty(),
        "a ChangeSet that cannot be ordered must never be buffered"
    );
}

#[test]
fn a_repeated_causal_parent_is_refused() {
    let parent = changeset_id(1);
    let id = changeset_id(2);
    let doubled = DeliveredChangeSet::new(
        id,
        actor(1),
        CausalParents::after(parent, vec![parent]),
        HeadId::from_bytes([0; 32]),
        HeadId::from_bytes([1; 32]),
    );
    let (unchanged, reception) = Advancement::new(actor(9)).deliver(doubled);

    assert_eq!(
        reception,
        Reception::Refused(Refusal::ParentRepeated {
            changeset: id,
            parent
        })
    );
    assert!(unchanged.known_missing().is_empty());
}

#[test]
fn a_buffered_changeset_that_turns_out_to_be_underivable_is_reported_not_swallowed() {
    let (_, authored) = chain(2);
    let lying = authored[1]
        .clone()
        .claiming(authored[1].base_head(), HeadId::from_bytes([0xee; 32]));

    // It arrives before its causal parent, so nothing can be checked yet: it is held.
    let (receiver, reception) = Advancement::new(actor(9)).deliver(lying.clone());
    assert!(
        matches!(reception, Reception::Buffered { .. }),
        "{reception:?}"
    );

    // Its parent arrives, the check finally runs, and the refusal is reported rather than dropped.
    let (receiver, reception) = receiver.deliver(authored[0].clone());
    let Reception::Applied {
        applied, refused, ..
    } = reception
    else {
        panic!("{reception:?}");
    };
    assert_eq!(applied, vec![authored[0].id()]);
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0].0, lying.id());
    assert!(matches!(
        refused[0].1,
        Refusal::ResultingHeadNotDerived { .. }
    ));
    assert!(!receiver.has_applied(&lying.id()));
}

#[test]
fn authoring_refuses_an_identifier_the_actor_already_holds() {
    let (advanced, first) = Advancement::new(actor(1)).author(changeset_id(1)).unwrap();
    assert_eq!(
        advanced.author(first.id()),
        Err(Refusal::AlreadyKnown {
            changeset: first.id()
        })
    );
}

// ---------------------------------------------------------------------------
// The review axis: a head that was offered never advances underneath the offer.
// ---------------------------------------------------------------------------

#[test]
fn a_head_is_working_until_it_is_offered() {
    let (advanced, _) = Advancement::new(actor(1)).author(changeset_id(1)).unwrap();
    assert_eq!(advanced.head_state(), HeadState::Working);
    assert!(advanced.review_offer().is_none());

    let offered = advanced.offer_for_review();
    assert_eq!(offered.head_state(), HeadState::ReadyForReview);
    assert_eq!(offered.actor_head().state(), HeadState::ReadyForReview);
    assert_eq!(
        offered.review_offer().map(|offer| offer.head()),
        Some(offered.head())
    );
    assert_eq!(
        advanced.head_state(),
        HeadState::Working,
        "the input was mutated"
    );
}

#[test]
fn an_offer_does_not_follow_the_working_head() {
    let (advanced, _) = Advancement::new(actor(1)).author(changeset_id(1)).unwrap();
    let offered = advanced.offer_for_review();
    let under_review = offered.head();

    let (kept_working, _) = offered.author(changeset_id(2)).unwrap();

    assert_ne!(
        kept_working.head(),
        under_review,
        "the actor did not move on"
    );
    assert_eq!(
        kept_working.review_offer().map(|offer| offer.head()),
        Some(under_review),
        "the offered head followed the actor past the state that was offered"
    );
    assert_eq!(
        kept_working.head_state(),
        HeadState::Working,
        "the new head inherited the review state of a different head"
    );
    assert_eq!(
        kept_working.review_offer().map(|offer| offer.state()),
        Some(HeadState::ReadyForReview)
    );
}

#[test]
fn superseding_and_archiving_move_the_offer_and_nothing_else() {
    let (advanced, _) = Advancement::new(actor(1)).author(changeset_id(1)).unwrap();
    let offered = advanced.offer_for_review();

    let superseded = offered.supersede_review_offer();
    assert_eq!(
        superseded.review_offer().map(|offer| offer.state()),
        Some(HeadState::Superseded)
    );
    assert_eq!(superseded.head(), offered.head());
    assert_eq!(superseded.applied(), offered.applied());

    let archived = superseded.archive_review_offer();
    assert_eq!(
        archived.review_offer().map(|offer| offer.state()),
        Some(HeadState::Archived)
    );
    assert_eq!(
        archived.review_offer().map(|offer| offer.head()),
        Some(offered.head())
    );

    // With no offer there is nothing to move.
    let never_offered = Advancement::new(actor(1));
    assert!(never_offered
        .supersede_review_offer()
        .review_offer()
        .is_none());
}

// ---------------------------------------------------------------------------
// The ordering rule, re-derived independently of the crate.
// ---------------------------------------------------------------------------

/// A second implementation of the head framing, written from `src/digest.rs`'s prose rather than
/// from its code. A test that calls the same function it is checking proves only that the function
/// is deterministic.
fn independently_derived_head(ordered: &[ChangeSetId]) -> HeadId {
    let mut digest = TestDigest::start();
    digest.absorb(&(HEAD_DOMAIN.len() as u64).to_be_bytes());
    digest.absorb(HEAD_DOMAIN.as_bytes());
    digest.absorb(&(ordered.len() as u64).to_be_bytes());
    for id in ordered {
        digest.absorb(id.as_bytes());
    }
    digest.finish()
}

#[test]
fn the_head_is_the_framed_digest_of_the_applied_set_in_causal_order() {
    let history = generate_history(5, 3, 15);
    let receiver = deliver_all(Advancement::new(actor(9)), &history.changesets);

    assert_eq!(
        receiver.head(),
        independently_derived_head(&receiver.applied()),
        "the crate's head is not the framed digest of what it says it applied"
    );
}

#[test]
fn the_causal_order_never_places_a_parent_after_a_child() {
    let history = generate_history(13, 4, 30);
    let receiver = deliver_all(Advancement::new(actor(9)), &history.changesets);
    let order = receiver.applied();

    let position: std::collections::BTreeMap<ChangeSetId, usize> = order
        .iter()
        .enumerate()
        .map(|(index, id)| (*id, index))
        .collect();

    for changeset in &history.changesets {
        for parent in changeset.parents().as_slice() {
            assert!(
                position[parent] < position[&changeset.id()],
                "a causal parent was ordered after the ChangeSet that follows it"
            );
        }
    }
}

/// The test double's assumption, checked rather than assumed: every distinct causal set in these
/// campaigns gets its own head. If this ever fails, a convergence test could be passing on a
/// collision.
#[test]
fn distinct_causal_sets_have_distinct_heads() {
    let history = generate_history(17, 3, 40);
    let mut receiver = Advancement::new(actor(9));
    let mut seen = std::collections::BTreeSet::new();
    seen.insert(receiver.head());

    for changeset in &history.changesets {
        let (advanced, _) = receiver.deliver(changeset.clone());
        receiver = advanced;
        assert!(
            seen.insert(receiver.head()),
            "two different causal sets produced one head"
        );
    }
    assert_eq!(seen.len(), history.changesets.len() + 1);
}
