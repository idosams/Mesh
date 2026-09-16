//! The revocation attack suite: **a revoked key authorizes nothing new, and loses nothing old.**
//!
//! `crates/mesh-policy/tests/revocation.rs`, so `cargo nextest run --test revocation` runs it.
//!
//! Revocation is where signature schemes usually fail, and they fail in one of two directions.
//! Every test here is written as an attack from one of them:
//!
//! * *the revocation does not bite* — the key keeps working somewhere, on some surface, on some
//!   peer that has not heard, or through a successor it minted for itself;
//! * *the revocation bites too hard* — publishing a rotation record deletes reviewed history, which
//!   makes revocation a history-rewrite primitive available to whoever can get a key revoked.
//!
//! The threat model is the standard one for this crate: the agent process can read the workspace,
//! run code the user allowed, hold whatever capability it was legitimately issued, replay anything
//! it has ever seen, and lie about everything it says about itself. It may also be the relay.

mod support;

use mesh_crypto::{Delegated, DelegatedAction, Delegation, HumanAction};
use mesh_policy::{
    AuthorityRequest, AuthorshipStanding, CatchUpError, DecisionLedger, DenialReason, EpochChain,
    HumanPrincipal, Operation, PendingSession, PolicyHeadAssertion, RevocationStanding,
    RotationReason,
};
use mesh_types::PolicyEpoch;

use support::{
    agent_capability, delegated_capability_in, every_human_action, human, human_capability_in, key,
    EPOCH, NOT_AFTER, NOW, WORKSPACE,
};

/// The key that gets stolen in every scenario below.
const LEAKED: u8 = 9;

/// The epoch the rotation that revokes it enters.
const AFTER: PolicyEpoch = PolicyEpoch::new(EPOCH.value() + 1);

fn request(subject: u8, operation: Operation) -> AuthorityRequest {
    AuthorityRequest::new(key(subject), WORKSPACE, operation, NOW)
}

/// A chain at [`EPOCH`], and the chain after the operator revoked [`LEAKED`] for compromise.
fn revoked_chain() -> (EpochChain, EpochChain, mesh_policy::EpochRotation) {
    let before = EpochChain::genesis(EPOCH);
    let (after, rotation) = before
        .clone()
        .rotate(RotationReason::ActorKeyCompromised, [key(LEAKED)])
        .expect("the epoch counter is nowhere near exhausted");
    (before, after, rotation)
}

// ---------------------------------------------------------------------------------------------
// Direction one: the revocation must bite, on every surface.
// ---------------------------------------------------------------------------------------------

/// **The gate for this task.** A capability issued to the revoked key *in the epoch now in force* —
/// the one thing rotating the epoch does not already kill — is refused for every operation, at both
/// tiers, on both gates.
///
/// A stale capability is denied earlier, for [`DenialReason::EpochSuperseded`]; this test uses a
/// freshly re-issued one so that the denial that fires is the revocation itself and not the epoch
/// comparison standing in front of it. Both are asserted, because "the revocation is redundant
/// today" is not the same claim as "the revocation works".
#[test]
fn a_revoked_key_is_refused_for_every_operation_on_every_surface() {
    let (_, after, _) = revoked_chain();

    for operation in Operation::ALL {
        // The delegated surface, re-issued in the epoch in force.
        let reissued =
            delegated_capability_in(LEAKED, DelegatedAction::ALL, after.current(), NOT_AFTER);
        let (_, outcome) =
            DecisionLedger::empty().authorize(&after, &reissued, &request(LEAKED, operation));
        let denial = outcome.expect_err("a revoked key was granted authority");
        // One reason for all eight, including canonical advancement: the shared precondition check
        // runs before the action check, so revocation answers first. Canonical advancement is
        // refused a second time behind it, by a vocabulary that has no word for it — see
        // `revocation_is_not_what_stops_an_agent_from_publishing`.
        let expected = DenialReason::SubjectRevoked {
            in_force: after.current(),
        };
        assert_eq!(denial.reason(), expected, "delegated tier, {operation}");

        // The human surface, including the publication guard.
        let human_capability = human_capability_in(
            LEAKED,
            every_human_action(),
            after.current(),
            NOT_AFTER,
            WORKSPACE,
        );
        let (_, outcome) = DecisionLedger::empty().authorize(
            &after,
            &human_capability,
            &request(LEAKED, operation),
        );
        let denial = outcome.expect_err("a revoked human key was granted authority");
        assert_eq!(denial.reason(), expected, "human tier, {operation}");
    }

    // The publication guard is its own door and is asked directly.
    let human_capability = human_capability_in(
        LEAKED,
        every_human_action(),
        after.current(),
        NOT_AFTER,
        WORKSPACE,
    );
    let (ledger, outcome) = DecisionLedger::empty().authorize_publication(
        &after,
        &human(LEAKED),
        &human_capability,
        &request(LEAKED, Operation::AdvanceCanonicalHead),
    );
    assert_eq!(
        outcome.expect_err("a revoked human published").reason(),
        DenialReason::SubjectRevoked {
            in_force: after.current()
        }
    );
    assert_eq!(ledger.denials().count(), 1, "the refusal is recorded");
}

/// The stale capability the leaked key is actually holding — issued before the rotation — is dead
/// too, and dead for the epoch reason, which is what makes revocation work on a peer nobody
/// contacted.
#[test]
fn the_capability_the_revoked_key_already_holds_is_dead_in_the_new_epoch() {
    let (_, after, _) = revoked_chain();
    let held = agent_capability(LEAKED);
    assert_eq!(held.policy_epoch(), EPOCH);

    let (_, outcome) = DecisionLedger::empty().authorize(
        &after,
        &held,
        &request(LEAKED, Operation::AuthorChangeSet),
    );
    assert_eq!(
        outcome
            .expect_err("a capability from a rotated epoch was honoured")
            .reason(),
        DenialReason::EpochSuperseded {
            presented: EPOCH,
            in_force: AFTER
        }
    );
}

/// The re-entry attack: the holder of a revoked key mints itself a fresh key and delegates to it,
/// on the theory that `ActorId` is derived from the public key, so the successor is a different
/// actor that no revocation names.
///
/// It fails, and it fails structurally.
/// [`Capability::delegate`](mesh_crypto::Capability::delegate) copies the parent's policy epoch and
/// **takes no epoch argument**, so a child of a dead capability is dead in the same epoch. There is
/// no widening call to reject because there is none to write.
#[test]
fn a_revoked_actor_cannot_re_enter_by_minting_a_successor_key() {
    let (_, after, _) = revoked_chain();
    let held = agent_capability(LEAKED);
    let successor = key(42);

    let minted = held
        .delegate(&Delegation::new(
            successor,
            [DelegatedAction::AuthorChangeSet],
            NOT_AFTER,
        ))
        .expect("delegating an action the parent holds is structurally fine");

    // The one field that would have made this work is the one `delegate` will not take.
    assert_eq!(
        minted.policy_epoch(),
        EPOCH,
        "a delegation carries its parent's epoch, and no argument changes it"
    );

    let (_, outcome) = DecisionLedger::empty().authorize(
        &after,
        &minted,
        &AuthorityRequest::new(successor, WORKSPACE, Operation::AuthorChangeSet, NOW),
    );
    assert_eq!(
        outcome
            .expect_err("a revoked actor re-entered through a successor key")
            .reason(),
        DenialReason::EpochSuperseded {
            presented: EPOCH,
            in_force: AFTER
        }
    );

    // And the successor is genuinely a different actor: it is not itself revoked. Authority for it
    // comes from a fresh human issuance in the epoch in force, and from nowhere else.
    assert_eq!(
        after.revocation_standing_of(&successor),
        RevocationStanding::NotRevoked
    );
}

/// Every rotation reason revokes identically. A revocation is not softer because the device was
/// merely retired: an operator who picks the wrong word does not get a weaker revocation.
#[test]
fn every_rotation_reason_revokes_with_an_effective_epoch() {
    for reason in RotationReason::ALL {
        let (chain, _) = EpochChain::genesis(EPOCH)
            .rotate(reason, [key(LEAKED)])
            .expect("rotation");
        let entry = chain
            .revocations()
            .entry_for(&key(LEAKED))
            .expect("the revocation is recorded");
        assert_eq!(entry.effective(), AFTER, "{reason}");
        assert_eq!(entry.reason(), reason);
        assert_eq!(entry.subject(), key(LEAKED));
        assert!(chain.revocation_standing_of(&key(LEAKED)).is_revoked());
    }
}

// ---------------------------------------------------------------------------------------------
// Direction two: the revocation must not rewrite history.
// ---------------------------------------------------------------------------------------------

/// Work the leaked key authored before the rotation stays valid and stays reachable. Anything else
/// makes one rotation record a delete button for a reviewed history.
#[test]
fn history_sealed_before_the_revocation_still_stands() {
    let (before, after, _) = revoked_chain();

    assert_eq!(
        before.authorship_standing(&key(LEAKED), EPOCH),
        AuthorshipStanding::Stands
    );
    assert_eq!(
        after.authorship_standing(&key(LEAKED), EPOCH),
        AuthorshipStanding::Stands,
        "revoking a key deleted what it had already sealed"
    );
    assert!(after.authorship_standing(&key(LEAKED), EPOCH).stands());
    assert!(after.is_revoked(&key(LEAKED)), "and it is still revoked");
}

/// A record sealed in or after the epoch the revocation took effect in is refused. The boundary is
/// asserted on both sides, because an off-by-one here is either a live compromised key or a deleted
/// history.
#[test]
fn a_record_sealed_from_the_effective_epoch_onwards_is_refused() {
    let (_, after, _) = revoked_chain();

    assert_eq!(
        after.authorship_standing(&key(LEAKED), EPOCH),
        AuthorshipStanding::Stands,
        "the last epoch before the revocation"
    );
    assert_eq!(
        after.authorship_standing(&key(LEAKED), AFTER),
        AuthorshipStanding::AuthoredAfterRevocation {
            effective: AFTER,
            sealed_in: AFTER
        },
        "the epoch the revocation is effective in"
    );
}

/// No rotation issued later can reach back past a record that already exists. A rotation on a chain
/// at epoch `n` is effective at `n + 1`, which is strictly greater than every epoch an existing
/// record can name, so this is a property of the construction and not of a guard.
#[test]
fn no_later_rotation_reaches_back_past_a_sealed_record() {
    let mut chain = EpochChain::genesis(EPOCH);
    for _ in 0..5 {
        let (next, _) = chain
            .rotate(RotationReason::ActorKeyCompromised, [key(LEAKED)])
            .expect("rotation");
        chain = next;
        assert_eq!(
            chain.authorship_standing(&key(LEAKED), EPOCH),
            AuthorshipStanding::Stands,
            "a rotation at epoch {} invalidated a record sealed in epoch {}",
            chain.current().value(),
            EPOCH.value()
        );
    }
}

/// The mirror-image rewrite: a second rotation naming an already-revoked key carries a later
/// effective epoch, and letting it win would re-validate exactly the records the first revocation
/// refused. The earliest effective epoch is kept.
#[test]
fn a_second_revocation_cannot_move_the_effective_epoch_later() {
    let (_, after, _) = revoked_chain();
    let (later, _) = after
        .rotate(RotationReason::ScheduledRefresh, [key(LEAKED)])
        .expect("rotation");

    assert_eq!(
        later
            .revocations()
            .entry_for(&key(LEAKED))
            .expect("still revoked")
            .effective(),
        AFTER,
        "a later rotation moved the revocation forward"
    );
    assert_eq!(
        later.revocations().len(),
        1,
        "re-revoking a key is one revocation, not two"
    );
    assert_eq!(
        later.authorship_standing(&key(LEAKED), AFTER),
        AuthorshipStanding::AuthoredAfterRevocation {
            effective: AFTER,
            sealed_in: AFTER
        },
        "a record refused by the first revocation was re-validated by the second"
    );
    assert_eq!(
        later
            .revocations()
            .entry_for(&key(LEAKED))
            .expect("still revoked")
            .reason(),
        RotationReason::ActorKeyCompromised,
        "the incident reason was overwritten by a housekeeping one"
    );
}

/// A key nobody revoked keeps every record it ever sealed, at every epoch this peer has observed.
#[test]
fn an_unrevoked_author_stands_at_every_observed_epoch() {
    let (_, after, _) = revoked_chain();
    for value in 0..=after.current().value() {
        assert_eq!(
            after.authorship_standing(&key(3), PolicyEpoch::new(value)),
            AuthorshipStanding::Stands,
            "epoch {value}"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Fail closed: an unknown revocation state never reads as valid.
// ---------------------------------------------------------------------------------------------

/// A record naming an epoch this peer has never observed is indeterminate, for every author,
/// revoked or not. A peer that is behind has no evidence about which keys were revoked in the
/// epochs it has not seen, and "no evidence" is not "valid".
#[test]
fn an_unobserved_epoch_is_indeterminate_and_never_valid() {
    let (_, after, _) = revoked_chain();
    let in_force = after.current();

    for ahead in 1..=5u64 {
        let sealed_in = PolicyEpoch::new(in_force.value() + ahead);
        for author in [key(LEAKED), key(3)] {
            let standing = after.authorship_standing(&author, sealed_in);
            assert_eq!(
                standing,
                AuthorshipStanding::Indeterminate {
                    sealed_in,
                    in_force
                }
            );
            assert!(!standing.stands(), "an unobserved epoch read as valid");
            assert!(standing.is_indeterminate());
        }
    }
}

/// Catching up turns indeterminate into a decision, in whichever direction the evidence points.
/// The repair for a fail-closed answer is one record, which is the reason failing closed is
/// affordable.
#[test]
fn catching_up_resolves_an_indeterminate_record() {
    let (before, _, rotation) = revoked_chain();

    assert!(before
        .authorship_standing(&key(LEAKED), AFTER)
        .is_indeterminate());

    let repaired = before
        .observe(&rotation)
        .expect("the record extends this chain");
    assert_eq!(
        repaired.authorship_standing(&key(LEAKED), AFTER),
        AuthorshipStanding::AuthoredAfterRevocation {
            effective: AFTER,
            sealed_in: AFTER
        }
    );
    assert_eq!(
        repaired.authorship_standing(&key(3), AFTER),
        AuthorshipStanding::Stands
    );
}

// ---------------------------------------------------------------------------------------------
// Propagation: the offline peer applies the revocation before it admits anything.
// ---------------------------------------------------------------------------------------------

/// The scenario the task exists for. A peer goes offline holding epoch 7. While it is away the key
/// leaks and the operator rotates. On reconnect it is handed the rotation record and a request from
/// the leaked key in the same session — and it applies the record first, because there is no method
/// on the pending session that admits anything.
#[test]
fn an_offline_peer_applies_the_revocation_before_it_admits_one_operation() {
    let (offline, issuing, rotation) = revoked_chain();
    let session = PendingSession::opening(offline);
    assert_eq!(session.chain().current(), EPOCH);

    let session = session
        .catch_up(&[rotation], PolicyHeadAssertion::of(&issuing))
        .expect("the offered record reaches the asserted head");
    assert_eq!(session.applied_rotations(), 1);
    assert_eq!(session.chain().current(), AFTER);

    // The very first operation of the session, from a capability re-issued in the epoch in force.
    let reissued = delegated_capability_in(LEAKED, DelegatedAction::ALL, AFTER, NOT_AFTER);
    let (ledger, outcome) = session.admit(
        DecisionLedger::empty(),
        &reissued,
        &request(LEAKED, Operation::AuthorChangeSet),
    );
    assert_eq!(
        outcome
            .expect_err("the first operation after reconnect was admitted")
            .reason(),
        DenialReason::SubjectRevoked { in_force: AFTER }
    );
    assert_eq!(
        ledger.len(),
        1,
        "the first decision of the session is this one"
    );

    // And the work it authored before it went bad is still there.
    assert_eq!(
        session.chain().authorship_standing(&key(LEAKED), EPOCH),
        AuthorshipStanding::Stands
    );
}

/// The cheapest attack on a propagating revocation is to withhold the record: a hostile relay
/// offers nothing and lets the peer conclude it is current. The peer never reaches a session that
/// can admit anything.
#[test]
fn a_partner_that_withholds_the_rotation_record_never_reaches_an_admitting_session() {
    let (offline, issuing, _) = revoked_chain();

    let outcome = PendingSession::opening(offline).catch_up(&[], PolicyHeadAssertion::of(&issuing));
    assert_eq!(
        outcome.expect_err("a peer caught up on zero records"),
        CatchUpError::RecordsWithheld {
            reached: EPOCH,
            asserted: AFTER
        }
    );
}

/// A substituted policy history reaching the same epoch by a different route is refused, and named
/// as a fork rather than as a gap: the repairs are different and only one of them is "fetch more".
#[test]
fn a_substituted_policy_history_is_refused_at_catch_up() {
    let (offline, issuing, _) = revoked_chain();
    let (substituted, substituted_rotation) = EpochChain::genesis(EPOCH)
        .rotate(RotationReason::ScheduledRefresh, [])
        .expect("rotation");
    assert_eq!(substituted.current(), issuing.current());
    assert_ne!(substituted.head(), issuing.head());

    let outcome = PendingSession::opening(offline)
        .catch_up(&[substituted_rotation], PolicyHeadAssertion::of(&issuing));
    assert_eq!(
        outcome.expect_err("a substituted policy history was accepted"),
        CatchUpError::ForkedPolicyHistory {
            at: AFTER,
            reached: substituted.head(),
            asserted: issuing.head()
        }
    );
}

/// A peer that is *ahead* of its partner is not behind, and refusing it would stop a peer that
/// already enforces the revocation from doing any work at all. It admits, and it still refuses the
/// revoked key.
#[test]
fn a_peer_ahead_of_its_partner_admits_and_still_enforces_the_revocation() {
    let (_, after, _) = revoked_chain();
    let (ahead, _) = after
        .clone()
        .rotate(RotationReason::ScheduledRefresh, [])
        .expect("rotation");
    let stale_partner = PolicyHeadAssertion::of(&after);

    let session = PendingSession::opening(ahead)
        .catch_up(&[], stale_partner)
        .expect("being ahead of the partner is not being behind");
    assert_eq!(session.applied_rotations(), 0);

    let reissued = delegated_capability_in(
        LEAKED,
        DelegatedAction::ALL,
        session.chain().current(),
        NOT_AFTER,
    );
    let (_, outcome) = session.admit(
        DecisionLedger::empty(),
        &reissued,
        &request(LEAKED, Operation::Replicate),
    );
    assert_eq!(
        outcome
            .expect_err("a peer ahead of its partner stopped enforcing")
            .reason(),
        DenialReason::SubjectRevoked {
            in_force: session.chain().current()
        }
    );
}

/// A peer several rotations behind applies the whole backlog in order before admitting, and a
/// backlog offered out of order is refused rather than partially applied.
#[test]
fn a_backlog_is_applied_in_order_before_anything_is_admitted() {
    let start = EpochChain::genesis(EPOCH);
    let (one, first) = start
        .clone()
        .rotate(RotationReason::DeviceRetired, [key(4)])
        .expect("rotation");
    let (two, second) = one
        .rotate(RotationReason::ActorKeyCompromised, [key(LEAKED)])
        .expect("rotation");

    let out_of_order = PendingSession::opening(start.clone()).catch_up(
        &[second.clone(), first.clone()],
        PolicyHeadAssertion::of(&two),
    );
    assert!(
        matches!(out_of_order, Err(CatchUpError::Rotation(_))),
        "an out-of-order backlog was applied"
    );

    let session = PendingSession::opening(start)
        .catch_up(&[first, second], PolicyHeadAssertion::of(&two))
        .expect("the backlog in order reaches the asserted head");
    assert_eq!(session.applied_rotations(), 2);
    assert_eq!(session.chain().head(), two.head());
    assert!(session.chain().is_revoked(&key(LEAKED)));
    assert!(session.chain().is_revoked(&key(4)));
    assert_eq!(
        session
            .chain()
            .revocations()
            .entry_for(&key(4))
            .expect("revoked")
            .effective(),
        AFTER,
        "the first rotation's revocation is effective in the epoch it entered, not the last one"
    );
}

// ---------------------------------------------------------------------------------------------
// The recorded evidence.
// ---------------------------------------------------------------------------------------------

/// The revocation, its effective epoch and its reason are all recorded, and the record travels with
/// the rotation rather than being kept beside it.
#[test]
fn the_revocation_and_its_effective_epoch_are_recorded() {
    let (_, after, rotation) = revoked_chain();

    assert!(rotation.revoked().contains(&key(LEAKED)));
    assert_eq!(rotation.to(), AFTER);
    assert_eq!(rotation.reason(), RotationReason::ActorKeyCompromised);
    assert!(rotation.reason().is_compromise());

    let entry = after
        .revocations()
        .entry_for(&key(LEAKED))
        .expect("the revocation is on the chain");
    assert_eq!(entry.effective(), AFTER);
    assert_eq!(entry.reason(), RotationReason::ActorKeyCompromised);
    assert_eq!(
        after.revocations().entries().count(),
        1,
        "one rotation revoking one key records one revocation"
    );
    assert!(after.revocations().contains(&key(LEAKED)));
    assert!(!after.revocations().is_empty());
    assert!(!entry.to_string().is_empty());
}

/// A human whose approval key was revoked is refused at the publication guard, and the refusal is
/// recorded with the revocation reason available to the operator surface.
#[test]
fn a_revoked_human_key_reaches_no_publication_surface() {
    let (chain, rotation) = EpochChain::genesis(EPOCH)
        .rotate(RotationReason::HumanKeyCompromised, [key(2)])
        .expect("rotation");
    assert_eq!(rotation.reason(), RotationReason::HumanKeyCompromised);

    let capability = human_capability_in(2, every_human_action(), AFTER, NOT_AFTER, WORKSPACE);
    let approver: HumanPrincipal = human(2);
    let (ledger, outcome) = DecisionLedger::empty().authorize_publication(
        &chain,
        &approver,
        &capability,
        &request(2, Operation::AdvanceCanonicalHead),
    );
    assert_eq!(
        outcome.expect_err("a revoked human published").reason(),
        DenialReason::SubjectRevoked { in_force: AFTER }
    );
    assert_eq!(
        ledger
            .last()
            .expect("the decision is recorded")
            .outcome()
            .reason(),
        Some(DenialReason::SubjectRevoked { in_force: AFTER })
    );
    assert_eq!(
        chain.revocation_standing_of(&key(2)),
        RevocationStanding::Revoked {
            effective: AFTER,
            reason: RotationReason::HumanKeyCompromised
        }
    );
}

/// Revocation is defence in depth on the publication path and never the thing that carries it. An
/// agent whose key **nobody revoked**, holding the widest capability that can be delegated to it,
/// in the epoch in force, is refused for canonical advancement — because `DelegatedAction` has no
/// variant naming it, not because a list says no.
///
/// This matters because it is what keeps the central claim independent of revocation working. A
/// revocation bug is a serious bug; it is not a path to publication.
#[test]
fn revocation_is_not_what_stops_an_agent_from_publishing() {
    let (_, after, _) = revoked_chain();
    const UNREVOKED: u8 = 3;
    assert_eq!(
        after.revocation_standing_of(&key(UNREVOKED)),
        RevocationStanding::NotRevoked
    );
    let widest: mesh_crypto::Capability<Delegated> =
        delegated_capability_in(UNREVOKED, DelegatedAction::ALL, AFTER, NOT_AFTER);

    assert!(!widest.advances_canonical_head());
    assert!(!widest
        .actions()
        .iter()
        .any(|action| action.as_str() == HumanAction::AdvanceCanonicalHead.as_str()));

    let (_, outcome) = DecisionLedger::empty().authorize(
        &after,
        &widest,
        &request(UNREVOKED, Operation::AdvanceCanonicalHead),
    );
    assert_eq!(
        outcome.expect_err("an agent published").reason(),
        DenialReason::CanonicalAdvanceIsNotDelegable
    );
}
