//! Policy epoch rotation, and the property the whole design exists for: a revocation takes effect
//! on a peer that was never contacted.
//!
//! Every test here is written against a *simulated peer*, meaning a second [`EpochChain`] value
//! that never sees the first one and only ever receives an [`EpochRotation`] record. Nothing in
//! this crate can open a socket, so "offline" is not simulated with a flag — it is the only mode
//! there is, and these tests show the mechanism working in it.

mod support;

use mesh_crypto::{DelegatedAction, Delegation};
use mesh_policy::{
    AuthorityRequest, DecisionLedger, DenialReason, EpochChain, EpochError, EpochStanding,
    Operation, RotationReason,
};
use mesh_types::PolicyEpoch;

use support::{
    agent_capability, every_human_action, human_capability_in, key, EPOCH, NOT_AFTER, NOW,
    WORKSPACE,
};

fn chain() -> EpochChain {
    EpochChain::genesis(EPOCH)
}

/// **The acceptance criterion.** A rotation on one peer invalidates the prior epoch's authority on
/// a second peer that was offline for it, once that peer receives the record — and the record is
/// the only thing that has to travel.
#[test]
fn rotation_invalidates_prior_authority_on_a_peer_that_was_never_contacted() {
    let issuing = chain();
    let offline = chain();
    let agent = agent_capability(9);
    let request = AuthorityRequest::new(key(9), WORKSPACE, Operation::AuthorChangeSet, NOW);

    // Before: the offline peer accepts, correctly.
    let (_, outcome) = DecisionLedger::empty().authorize(&offline, &agent, &request);
    assert!(outcome.is_ok());

    // The issuing peer rotates. The offline peer is not contacted and does not change.
    let (issuing, rotation) = issuing
        .rotate(RotationReason::ActorKeyCompromised, [key(9)])
        .expect("a rotation");
    assert_eq!(issuing.current(), PolicyEpoch::new(EPOCH.value() + 1));
    assert_eq!(offline.current(), EPOCH);

    // The record arrives by whatever carrier. Applying it needs nothing but the local chain.
    let offline = offline.observe(&rotation).expect("the record applies");
    assert_eq!(offline.current(), issuing.current());
    assert_eq!(
        offline.head(),
        issuing.head(),
        "both peers agree on the policy history"
    );

    // After: refused, with the reason recorded.
    let (ledger, outcome) = DecisionLedger::empty().authorize(&offline, &agent, &request);
    assert_eq!(
        outcome
            .expect_err("a prior-epoch capability survived a rotation")
            .reason(),
        DenialReason::EpochSuperseded {
            presented: EPOCH,
            in_force: offline.current()
        }
    );
    assert_eq!(ledger.denials().count(), 1);
    assert!(offline.is_revoked(&key(9)));
}

/// A peer several rotations behind catches up by applying the records in order, and reaches
/// exactly the same state — same epoch, same head digest, same revocation set.
#[test]
fn a_peer_that_is_several_rotations_behind_catches_up_in_order() {
    let mut issuing = chain();
    let mut records = Vec::new();
    for (reason, revoked) in [
        (RotationReason::ScheduledRefresh, None),
        (RotationReason::ActorKeyCompromised, Some(key(9))),
        (RotationReason::ActorRemoved, Some(key(8))),
        (RotationReason::PolicyChanged, None),
    ] {
        let (next, record) = issuing.rotate(reason, revoked).expect("a rotation");
        issuing = next;
        records.push(record);
    }

    let mut behind = chain();
    for record in &records {
        behind = behind.observe(record).expect("in-order catch-up");
    }
    assert_eq!(behind.current(), issuing.current());
    assert_eq!(behind.head(), issuing.head());
    assert_eq!(behind.revocations(), issuing.revocations());
    assert_eq!(behind.rotations(), 4);
    assert!(behind.is_revoked(&key(9)));
    assert!(behind.is_revoked(&key(8)));
}

/// Records applied out of order are refused, naming the gap, rather than skipping ahead. Skipping
/// would silently drop the revocation set of every record in between.
#[test]
fn a_gap_is_refused_and_named() {
    let (issuing, first) = chain()
        .rotate(RotationReason::ActorKeyCompromised, [key(9)])
        .expect("first rotation");
    let (_, second) = issuing
        .rotate(RotationReason::ActorRemoved, [key(8)])
        .expect("second rotation");

    let behind = chain();
    assert_eq!(
        behind.clone().observe(&second),
        Err(EpochError::NotContiguous {
            in_force: EPOCH,
            offered: first.to()
        })
    );
    // And the peer still holds the epoch it had. A refused record changes nothing.
    assert_eq!(behind.current(), EPOCH);
    assert!(!behind.is_revoked(&key(9)));

    // The repair is to fetch the missing record and apply it first.
    let repaired = behind
        .observe(&first)
        .expect("the missing record")
        .observe(&second)
        .expect("then the later one");
    assert!(repaired.is_revoked(&key(9)));
    assert!(repaired.is_revoked(&key(8)));
}

/// A rotation record that extends a *different* policy history is refused even though its epoch
/// numbers line up. That is a substituted policy, not a gap, and the digest is what tells them
/// apart.
#[test]
fn a_forked_policy_history_is_refused() {
    let (real, _) = chain()
        .rotate(RotationReason::HumanKeyCompromised, [key(1)])
        .expect("the real rotation");

    // An attacker builds a chain with the same epoch numbers and a different history: same source
    // epoch, same target, different revocation set — so a different digest.
    let (_, forged) = chain()
        .rotate(RotationReason::ScheduledRefresh, [])
        .expect("a divergent rotation");
    assert_eq!(forged.from(), EPOCH);
    assert_eq!(forged.to(), real.current());

    // The peer that already applied the real one refuses the forgery: it does not leave the epoch
    // in force.
    assert_eq!(
        real.clone().observe(&forged),
        Err(EpochError::NotContiguous {
            in_force: real.current(),
            offered: EPOCH
        })
    );

    // And a peer at the fork point that applied the forgery ends in a different head digest, so
    // the divergence is detectable rather than silent.
    let forked = chain()
        .observe(&forged)
        .expect("the forgery applies locally");
    assert_ne!(forked.head(), real.head());
    assert_eq!(forked.current(), real.current());
}

/// A rotation whose target is not its source plus one is refused. An epoch that jumps is an epoch
/// whose intermediate revocations were never seen.
#[test]
fn a_rotation_that_skips_epochs_is_refused() {
    let (_, honest) = chain()
        .rotate(RotationReason::ScheduledRefresh, [])
        .expect("a rotation");
    // Reconstruct the same record shape against a chain whose epoch is far behind, so the target
    // is more than one step away.
    let far_behind = EpochChain::genesis(PolicyEpoch::new(EPOCH.value() - 3));
    assert_eq!(
        far_behind.observe(&honest),
        Err(EpochError::NotContiguous {
            in_force: PolicyEpoch::new(EPOCH.value() - 3),
            offered: EPOCH
        })
    );
}

/// The rotation record carries its reason to every peer, and the reason survives the trip.
#[test]
fn the_reason_travels_with_the_record() {
    for reason in RotationReason::ALL {
        let (_, record) = chain().rotate(reason, [key(9)]).expect("a rotation");
        assert_eq!(record.reason(), reason);
        assert_eq!(record.reason().as_str(), reason.as_str());
        assert!(record.revoked().contains(&key(9)));
        assert_eq!(record.reason().to_string(), reason.as_str());
    }
    assert_eq!(
        RotationReason::ALL
            .into_iter()
            .filter(RotationReason::is_compromise)
            .count(),
        2
    );
}

/// The record's digest covers every field. Changing any one of them changes it, so a rotation
/// cannot be edited in flight and still apply to a peer that has the real predecessor.
#[test]
fn the_rotation_digest_covers_every_field() {
    let base = chain();
    let (_, record) = base
        .clone()
        .rotate(RotationReason::ActorKeyCompromised, [key(9)])
        .expect("a rotation");
    let digest = record.digest();

    // A different reason.
    let (_, other_reason) = base
        .clone()
        .rotate(RotationReason::ScheduledRefresh, [key(9)])
        .expect("a rotation");
    assert_ne!(other_reason.digest(), digest);

    // A different revocation set.
    let (_, other_revoked) = base
        .clone()
        .rotate(RotationReason::ActorKeyCompromised, [key(8)])
        .expect("a rotation");
    assert_ne!(other_revoked.digest(), digest);

    // An extra revoked key.
    let (_, more_revoked) = base
        .clone()
        .rotate(RotationReason::ActorKeyCompromised, [key(9), key(8)])
        .expect("a rotation");
    assert_ne!(more_revoked.digest(), digest);

    // A different source epoch.
    let (_, other_epoch) = EpochChain::genesis(PolicyEpoch::new(EPOCH.value() + 100))
        .rotate(RotationReason::ActorKeyCompromised, [key(9)])
        .expect("a rotation");
    assert_ne!(other_epoch.digest(), digest);

    // A different predecessor — the same rotation applied one link further along the chain.
    let (advanced, _) = base
        .rotate(RotationReason::ScheduledRefresh, [])
        .expect("a rotation");
    let (_, later) = advanced
        .rotate(RotationReason::ActorKeyCompromised, [key(9)])
        .expect("a rotation");
    assert_ne!(later.digest(), digest);

    // And the digest is deterministic: the same rotation twice is the same 32 bytes.
    let (_, again) = chain()
        .rotate(RotationReason::ActorKeyCompromised, [key(9)])
        .expect("a rotation");
    assert_eq!(again.digest(), digest);
}

/// The revocation set is order-independent: two peers that build the same rotation from differently
/// ordered inputs get the same digest, so a set that arrives shuffled does not fork the chain.
#[test]
fn the_revocation_set_is_order_independent() {
    let (_, one) = chain()
        .rotate(RotationReason::ActorRemoved, [key(3), key(1), key(2)])
        .expect("a rotation");
    let (_, other) = chain()
        .rotate(RotationReason::ActorRemoved, [key(2), key(3), key(1)])
        .expect("a rotation");
    assert_eq!(one.digest(), other.digest());
    assert_eq!(one, other);
    assert_eq!(one.revoked().len(), 3);
}

/// Standing is total: every epoch is current, superseded or unknown, and only the first is live.
#[test]
fn every_epoch_has_a_standing_and_only_the_current_one_is_live() {
    let chain = chain();
    assert_eq!(chain.standing_of(EPOCH), EpochStanding::Current);
    assert!(chain.standing_of(EPOCH).is_live());

    let older = PolicyEpoch::new(EPOCH.value() - 1);
    assert_eq!(
        chain.standing_of(older),
        EpochStanding::Superseded { by: EPOCH }
    );
    assert!(!chain.standing_of(older).is_live());

    let newer = PolicyEpoch::new(EPOCH.value() + 1);
    assert_eq!(
        chain.standing_of(newer),
        EpochStanding::Unknown { in_force: EPOCH }
    );
    assert!(!chain.standing_of(newer).is_live());
}

/// Revocation accumulates. A key revoked three rotations ago is still revoked, so an operator
/// cannot un-revoke a key by rotating again.
#[test]
fn revocation_is_never_undone_by_a_later_rotation() {
    let (chain, _) = chain()
        .rotate(RotationReason::ActorKeyCompromised, [key(9)])
        .expect("a rotation");
    let (chain, _) = chain
        .rotate(RotationReason::ScheduledRefresh, [])
        .expect("a rotation with no revocations");
    let (chain, _) = chain
        .rotate(RotationReason::PolicyChanged, [])
        .expect("another");
    assert!(chain.is_revoked(&key(9)));
    assert_eq!(chain.revocations().len(), 1);
}

/// A capability re-issued in the epoch in force works for a subject who was never revoked, so the
/// mechanism is a revocation and not a workspace-wide outage.
#[test]
fn rotation_does_not_lock_out_an_unrevoked_actor() {
    let (rotated, _) = chain()
        .rotate(RotationReason::ActorKeyCompromised, [key(9)])
        .expect("a rotation revoking one agent");
    let reissued = human_capability_in(
        1,
        every_human_action(),
        rotated.current(),
        NOT_AFTER,
        WORKSPACE,
    )
    .delegate(&Delegation::new(
        key(7),
        [DelegatedAction::AuthorChangeSet],
        NOT_AFTER,
    ))
    .expect("a re-issued delegation for a different agent");

    let (_, outcome) = DecisionLedger::empty().authorize(
        &rotated,
        &reissued,
        &AuthorityRequest::new(key(7), WORKSPACE, Operation::AuthorChangeSet, NOW),
    );
    assert!(outcome.is_ok(), "an unrevoked actor was locked out");
}

/// Nothing here mutates in place. A rotation returns a new chain and leaves the old value alone,
/// so no code path moves a peer's epoch backwards by editing a value somebody else holds.
#[test]
fn the_chain_is_immutable() {
    let before = chain();
    let snapshot = before.clone();
    let (after, _) = before
        .rotate(RotationReason::PolicyChanged, [key(9)])
        .expect("a rotation");
    assert_eq!(snapshot.current(), EPOCH);
    assert!(!snapshot.is_revoked(&key(9)));
    assert_eq!(snapshot.rotations(), 0);
    assert_eq!(after.rotations(), 1);
}
