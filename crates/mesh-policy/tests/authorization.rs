//! The publication attack suite. **Zero unauthorized publications is the bar.**
//!
//! `crates/mesh-policy/tests/authorization.rs`, so `cargo nextest run --test authorization` runs it.
//!
//! Every test here is written as an *attack*, not as a feature check. The question each one answers
//! is "what does a hostile agent process try next", where hostile means: it can read the workspace,
//! run code the user allowed, hold whatever capability it was legitimately issued, replay anything
//! it has ever seen, and lie about everything it says about itself.
//!
//! The one line a release cannot ship without is
//! [`no_agent_reachable_path_advances_canonical_state`].

mod support;

use mesh_crypto::{
    AuthorityTier, Capability, Delegated, DelegatedAction, Delegation, DelegationBudget,
    DelegationError, Expiry, HumanAction, HumanHeld, HumanKeyCustody,
};
use mesh_policy::{
    AuthorityRequest, DecisionLedger, DenialReason, EpochChain, HumanPrincipal, Operation,
    Principal, PrincipalError, RotationReason,
};
use mesh_types::PolicyEpoch;

use support::{
    agent_capability, delegated_capability, every_human_action, human, human_capability,
    human_capability_in, key, non_human_kinds, EPOCH, NOT_AFTER, NOW, OTHER_WORKSPACE, WORKSPACE,
};

fn chain() -> EpochChain {
    EpochChain::genesis(EPOCH)
}

fn request(subject: u8, operation: Operation) -> AuthorityRequest {
    AuthorityRequest::new(key(subject), WORKSPACE, operation, NOW)
}

// ---------------------------------------------------------------------------------------------
// The claim.
// ---------------------------------------------------------------------------------------------

/// **The release gate.** Enumerate every way an agent can reach the decision layer and show each
/// one ending in a denial rather than in publication authority.
///
/// The four routes:
///
/// 1. present its own capability for `advance-canonical-head` through the general gate;
/// 2. present the widest capability that can be delegated to it, for the same operation;
/// 3. present a capability delegated from a *human root that itself holds* canonical-head
///    advancement — the strongest starting point that exists;
/// 4. hold a human-held capability and try to enrol as the approver.
///
/// Route 4 is the only one that needs a run-time answer, and it needs one because the actor kind is
/// data. Routes 1 to 3 cannot even name the action: `DelegatedAction` has no variant for it, so the
/// capability they present is the widest one representable and it still does not contain it. The
/// publication guard itself is unreachable from any of them — passing a `Capability<Delegated>` to
/// `authorize_publication` does not compile, which `lib.rs` proves with a `compile_fail` doctest.
#[test]
fn no_agent_reachable_path_advances_canonical_state() {
    let chain = chain();
    let ledger = DecisionLedger::empty();

    // Route 1 and 2: the widest capability an agent can hold, asking for publication.
    let widest = agent_capability(9);
    assert_eq!(
        widest.actions().len(),
        DelegatedAction::ALL.len(),
        "the fixture is meant to be the widest delegable grant"
    );
    assert!(
        !widest.advances_canonical_head(),
        "a delegated capability reported canonical-advance authority"
    );
    let (ledger, outcome) = ledger.authorize(
        &chain,
        &widest,
        &request(9, Operation::AdvanceCanonicalHead),
    );
    assert_eq!(
        outcome
            .expect_err("an agent was granted publication")
            .reason(),
        DenialReason::CanonicalAdvanceIsNotDelegable
    );

    // Route 3: delegate from a root that *does* hold canonical-head advancement. The delegation
    // request cannot mention it, so the child cannot carry it.
    let root = human_capability(1, every_human_action());
    assert!(root.advances_canonical_head());
    let child = root
        .delegate(&Delegation::new(key(9), DelegatedAction::ALL, NOT_AFTER))
        .expect("a delegation of the delegable actions");
    assert!(!child.advances_canonical_head());
    for action in child.actions() {
        assert!(
            !Delegated::is_canonical_advance(*action),
            "a delegated capability carries {action}"
        );
    }
    let (ledger, outcome) =
        ledger.authorize(&chain, &child, &request(9, Operation::AdvanceCanonicalHead));
    assert_eq!(
        outcome
            .expect_err("a delegated child was granted publication")
            .reason(),
        DenialReason::CanonicalAdvanceIsNotDelegable
    );

    // Route 4: an agent holding a human-held capability still cannot become the approver.
    for kind in non_human_kinds() {
        assert_eq!(
            HumanPrincipal::enrol(Principal::new(key(9), kind)),
            Err(PrincipalError::MayNotHoldApprovalCapability { kind }),
            "{kind} enrolled as a human"
        );
    }

    // And every denial is on the record, with its reason.
    assert_eq!(ledger.denials().count(), 2);
    assert!(ledger.entries().iter().all(|record| {
        record.outcome().reason() == Some(DenialReason::CanonicalAdvanceIsNotDelegable)
    }));
}

/// The general gate never grants canonical-head advancement **at either tier**, including to a
/// human whose capability genuinely holds it. Publication has exactly one door.
#[test]
fn the_general_gate_grants_publication_to_nobody_at_all() {
    let chain = chain();
    let human_cap = human_capability(1, every_human_action());
    let (ledger, outcome) = DecisionLedger::empty().authorize(
        &chain,
        &human_cap,
        &request(1, Operation::AdvanceCanonicalHead),
    );
    assert_eq!(
        outcome
            .expect_err("the general gate granted publication to a human")
            .reason(),
        DenialReason::CanonicalAdvanceIsNotDelegable
    );
    assert_eq!(ledger.denials().count(), 1);
}

/// The one path that does grant it: a human, with a human-held capability that names the action,
/// in the epoch in force.
#[test]
fn the_publication_guard_grants_a_human_and_records_it() {
    let chain = chain();
    let (ledger, outcome) = DecisionLedger::empty().authorize_publication(
        &chain,
        &human(1),
        &human_capability(1, every_human_action()),
        &request(1, Operation::AdvanceCanonicalHead),
    );
    let authority = outcome.expect("a human with the capability publishes");
    assert_eq!(authority.approver(), key(1));
    assert_eq!(authority.epoch(), EPOCH);
    assert_eq!(authority.workspace(), WORKSPACE);
    assert_eq!(ledger.len(), 1);
    assert!(ledger.last().expect("one record").outcome().is_granted());
}

/// A human-held capability that does *not* name the action does not publish. Holding the tier is
/// not holding the authority.
#[test]
fn a_human_without_the_action_does_not_publish() {
    let chain = chain();
    let without = human_capability(1, [HumanAction::Delegated(DelegatedAction::RequestReview)]);
    let (_, outcome) = DecisionLedger::empty().authorize_publication(
        &chain,
        &human(1),
        &without,
        &request(1, Operation::AdvanceCanonicalHead),
    );
    assert_eq!(
        outcome
            .expect_err("a human without the action published")
            .reason(),
        DenialReason::ActionNotGranted {
            operation: Operation::AdvanceCanonicalHead
        }
    );
}

// ---------------------------------------------------------------------------------------------
// Widening.
// ---------------------------------------------------------------------------------------------

/// A delegation is narrower on every field, or it is refused. Enumerated per field rather than
/// argued.
#[test]
fn a_capability_cannot_be_widened_on_any_field() {
    let root = human_capability(1, every_human_action());
    let child = root
        .delegate(&Delegation::new(
            key(2),
            [DelegatedAction::AuthorChangeSet],
            NOT_AFTER,
        ))
        .expect("a narrowing delegation");

    // Actions: a child cannot hand on what it does not hold.
    assert_eq!(
        child.delegate(&Delegation::new(
            key(3),
            [DelegatedAction::RunValidation],
            NOT_AFTER
        )),
        Err(DelegationError::ActionNotHeld {
            action: DelegatedAction::RunValidation
        })
    );
    // Expiry: never past the parent's.
    assert_eq!(
        child.delegate(&Delegation::new(
            key(3),
            [DelegatedAction::AuthorChangeSet],
            Expiry::at_unix_millis(NOT_AFTER.as_unix_millis() + 1)
        )),
        Err(DelegationError::ExpiryWidened {
            parent: NOT_AFTER,
            requested: Expiry::at_unix_millis(NOT_AFTER.as_unix_millis() + 1)
        })
    );
    // Workspace and epoch: not arguments at all, so they are copied and cannot move.
    let grandchild = child
        .delegate(&Delegation::new(
            key(3),
            [DelegatedAction::AuthorChangeSet],
            NOT_AFTER,
        ))
        .expect("a same-width delegation");
    assert_eq!(grandchild.workspace(), root.workspace());
    assert_eq!(grandchild.policy_epoch(), root.policy_epoch());
    // Budget: strictly decreasing, so the chain is finite.
    assert!(grandchild.budget().remaining() < child.budget().remaining());
    assert!(child.budget().remaining() < root.budget().remaining());
    // Tier: every delegation lands on `Delegated`, from both tiers.
    assert_eq!(child.tier(), Delegated::NAME);
    assert_eq!(grandchild.tier(), Delegated::NAME);
}

/// The delegation chain terminates. A budget that did not decrease would let a lane build an
/// unbounded chain, and every link is a key that can be stolen.
#[test]
fn the_delegation_chain_is_finite() {
    let mut capability: Capability<Delegated> = human_capability(1, every_human_action())
        .delegate(&Delegation::new(
            key(2),
            [DelegatedAction::ReadWorkspace],
            NOT_AFTER,
        ))
        .expect("first delegation");
    let mut links = 1;
    loop {
        match capability.delegate(&Delegation::new(
            key(3),
            [DelegatedAction::ReadWorkspace],
            NOT_AFTER,
        )) {
            Ok(next) => {
                capability = next;
                links += 1;
                assert!(links < 64, "the delegation chain did not terminate");
            }
            Err(error) => {
                assert_eq!(error, DelegationError::BudgetExhausted);
                break;
            }
        }
    }
    assert_eq!(links, 3, "budget 3 permits three delegations and no more");
}

/// A capability granting nothing is refused rather than minted. An empty grant is a bearer token
/// with attack surface and no use.
#[test]
fn an_empty_grant_is_refused() {
    let root = human_capability(1, every_human_action());
    assert_eq!(
        root.delegate(&Delegation::new(key(2), [], NOT_AFTER)),
        Err(DelegationError::EmptyGrant)
    );
}

// ---------------------------------------------------------------------------------------------
// Expiry, epoch and revocation — on *every* path.
// ---------------------------------------------------------------------------------------------

/// An expired capability is refused for every operation in the vocabulary, at both tiers.
///
/// A loop over `Operation::ALL`, so adding an operation without an expiry check fails this test
/// rather than shipping.
#[test]
fn an_expired_capability_is_refused_on_every_path() {
    let chain = chain();
    let expired = Expiry::at_unix_millis(NOW);
    let agent = delegated_capability(9, DelegatedAction::ALL, expired);
    let person = human_capability_in(1, every_human_action(), EPOCH, expired, WORKSPACE);

    for operation in Operation::ALL {
        let (ledger, outcome) =
            DecisionLedger::empty().authorize(&chain, &agent, &request(9, operation));
        // Expiry is a precondition, checked before the action is even looked up — so the recorded
        // reason is `Expired` for the publication operation too, which is the more specific true
        // statement about the request.
        assert_eq!(
            outcome
                .expect_err("an expired capability was accepted")
                .reason(),
            DenialReason::Expired {
                not_after: expired,
                now: NOW
            },
            "{operation}"
        );
        assert_eq!(ledger.denials().count(), 1);
    }

    let (_, outcome) = DecisionLedger::empty().authorize_publication(
        &chain,
        &human(1),
        &person,
        &request(1, Operation::AdvanceCanonicalHead),
    );
    assert_eq!(
        outcome
            .expect_err("an expired human capability published")
            .reason(),
        DenialReason::Expired {
            not_after: expired,
            now: NOW
        }
    );
}

/// Expiry is inclusive at the boundary. `now == not_after` is expired: a capability valid "until"
/// a moment is not valid *at* it, and an off-by-one here is a millisecond of authority after
/// revocation.
#[test]
fn expiry_is_inclusive_at_the_boundary() {
    let chain = chain();
    let at = delegated_capability(
        9,
        [DelegatedAction::ReadWorkspace],
        Expiry::at_unix_millis(NOW),
    );
    let (_, outcome) =
        DecisionLedger::empty().authorize(&chain, &at, &request(9, Operation::ReadWorkspace));
    assert!(matches!(
        outcome.expect_err("valid at its own expiry").reason(),
        DenialReason::Expired { .. }
    ));

    let just_after = delegated_capability(
        9,
        [DelegatedAction::ReadWorkspace],
        Expiry::at_unix_millis(NOW + 1),
    );
    let (_, outcome) = DecisionLedger::empty().authorize(
        &chain,
        &just_after,
        &request(9, Operation::ReadWorkspace),
    );
    assert!(outcome.is_ok());
}

/// A capability from a rotated epoch is refused for every operation, at both tiers.
#[test]
fn a_superseded_epoch_is_refused_on_every_path() {
    let (rotated, _) = chain()
        .rotate(RotationReason::ActorKeyCompromised, [])
        .expect("a rotation");
    let agent = agent_capability(9);
    let person = human_capability(1, every_human_action());

    for operation in Operation::ALL {
        let (ledger, outcome) =
            DecisionLedger::empty().authorize(&rotated, &agent, &request(9, operation));
        assert_eq!(
            outcome
                .expect_err("a prior-epoch capability was accepted")
                .reason(),
            DenialReason::EpochSuperseded {
                presented: EPOCH,
                in_force: rotated.current()
            },
            "{operation}"
        );
        assert_eq!(ledger.denials().count(), 1);
    }

    let (_, outcome) = DecisionLedger::empty().authorize_publication(
        &rotated,
        &human(1),
        &person,
        &request(1, Operation::AdvanceCanonicalHead),
    );
    assert_eq!(
        outcome
            .expect_err("a prior-epoch human capability published")
            .reason(),
        DenialReason::EpochSuperseded {
            presented: EPOCH,
            in_force: rotated.current()
        }
    );
}

/// A capability naming an epoch this peer has never observed is refused, not deferred to. Fail
/// closed: forging a *later* epoch is the obvious move once rotation is the revocation mechanism.
#[test]
fn an_unobserved_future_epoch_is_refused_on_every_path() {
    let chain = chain();
    let ahead = PolicyEpoch::new(EPOCH.value() + 5);
    let forged = human_capability_in(1, every_human_action(), ahead, NOT_AFTER, WORKSPACE);

    let (_, outcome) = DecisionLedger::empty().authorize_publication(
        &chain,
        &human(1),
        &forged,
        &request(1, Operation::AdvanceCanonicalHead),
    );
    assert_eq!(
        outcome
            .expect_err("a capability from an unseen epoch published")
            .reason(),
        DenialReason::EpochUnknown {
            presented: ahead,
            in_force: EPOCH
        }
    );

    let agent = human_capability_in(1, every_human_action(), ahead, NOT_AFTER, WORKSPACE)
        .delegate(&Delegation::new(key(9), DelegatedAction::ALL, NOT_AFTER))
        .expect("a delegation carries its parent's epoch");
    for operation in Operation::ALL
        .into_iter()
        .filter(|o| !o.advances_canonical_state())
    {
        let (_, outcome) =
            DecisionLedger::empty().authorize(&chain, &agent, &request(9, operation));
        assert_eq!(
            outcome.expect_err("an unseen epoch was accepted").reason(),
            DenialReason::EpochUnknown {
                presented: ahead,
                in_force: EPOCH
            },
            "{operation}"
        );
    }
}

/// A revoked subject is refused for every operation even while the epoch it names is in force,
/// because the rotation that revoked it also moved the epoch — and once a peer has applied that
/// rotation, a *re-issued* capability in the new epoch is refused too.
#[test]
fn a_revoked_subject_is_refused_on_every_path() {
    let (rotated, _) = chain()
        .rotate(RotationReason::ActorKeyCompromised, [key(9)])
        .expect("a rotation revoking the agent");
    assert!(rotated.is_revoked(&key(9)));

    // Re-issued in the epoch now in force: the epoch check passes and revocation still bites.
    let reissued = human_capability_in(
        1,
        every_human_action(),
        rotated.current(),
        NOT_AFTER,
        WORKSPACE,
    )
    .delegate(&Delegation::new(key(9), DelegatedAction::ALL, NOT_AFTER))
    .expect("a re-issued delegation");

    for operation in Operation::ALL
        .into_iter()
        .filter(|o| !o.advances_canonical_state())
    {
        let (ledger, outcome) =
            DecisionLedger::empty().authorize(&rotated, &reissued, &request(9, operation));
        assert_eq!(
            outcome
                .expect_err("a revoked subject was accepted")
                .reason(),
            DenialReason::SubjectRevoked {
                in_force: rotated.current()
            },
            "{operation}"
        );
        assert_eq!(ledger.denials().count(), 1);
    }
}

/// A revoked **human** cannot publish, which is the case the whole mechanism exists for: the
/// approval key is the one key whose theft advances canonical state.
#[test]
fn a_revoked_human_cannot_publish() {
    let (rotated, _) = chain()
        .rotate(RotationReason::HumanKeyCompromised, [key(1)])
        .expect("a rotation revoking the human");
    let reissued = human_capability_in(
        1,
        every_human_action(),
        rotated.current(),
        NOT_AFTER,
        WORKSPACE,
    );
    let (ledger, outcome) = DecisionLedger::empty().authorize_publication(
        &rotated,
        &human(1),
        &reissued,
        &request(1, Operation::AdvanceCanonicalHead),
    );
    assert_eq!(
        outcome.expect_err("a revoked human published").reason(),
        DenialReason::SubjectRevoked {
            in_force: rotated.current()
        }
    );
    assert_eq!(ledger.denials().count(), 1);
}

// ---------------------------------------------------------------------------------------------
// Replay, substitution and scope escape.
// ---------------------------------------------------------------------------------------------

/// A capability replayed by a peer other than its subject is refused. The subject is the peer the
/// caller authenticated, never a field read off the capability — a capability is bearer evidence.
#[test]
fn a_replayed_capability_is_refused() {
    let chain = chain();
    let stolen = agent_capability(9);
    for operation in Operation::ALL
        .into_iter()
        .filter(|o| !o.advances_canonical_state())
    {
        let (_, outcome) =
            DecisionLedger::empty().authorize(&chain, &stolen, &request(8, operation));
        assert_eq!(
            outcome
                .expect_err("a stolen capability was accepted")
                .reason(),
            DenialReason::SubjectMismatch,
            "{operation}"
        );
    }

    // The same replay against the publication guard, by a human who is not the approver.
    let (_, outcome) = DecisionLedger::empty().authorize_publication(
        &chain,
        &human(2),
        &human_capability(1, every_human_action()),
        &request(2, Operation::AdvanceCanonicalHead),
    );
    assert_eq!(
        outcome
            .expect_err("a stolen human capability published")
            .reason(),
        DenialReason::ApproverMismatch
    );
}

/// A capability from another workspace does not work here. Scope is a binding, not a label.
#[test]
fn a_capability_does_not_escape_its_workspace() {
    let chain = chain();
    let elsewhere = human_capability_in(1, every_human_action(), EPOCH, NOT_AFTER, OTHER_WORKSPACE);
    let (_, outcome) = DecisionLedger::empty().authorize_publication(
        &chain,
        &human(1),
        &elsewhere,
        &request(1, Operation::AdvanceCanonicalHead),
    );
    assert_eq!(
        outcome
            .expect_err("a capability from another workspace published")
            .reason(),
        DenialReason::WorkspaceMismatch
    );
}

/// The publication guard answers one operation. A request naming another arrived at the wrong gate
/// and is refused, so publication authority cannot be obtained while asking for a read.
#[test]
fn the_publication_guard_answers_only_its_own_operation() {
    let chain = chain();
    for operation in Operation::ALL
        .into_iter()
        .filter(|o| !o.advances_canonical_state())
    {
        let (_, outcome) = DecisionLedger::empty().authorize_publication(
            &chain,
            &human(1),
            &human_capability(1, every_human_action()),
            &request(1, operation),
        );
        assert_eq!(
            outcome
                .expect_err("the publication guard answered another operation")
                .reason(),
            DenialReason::ActionNotGranted { operation },
            "{operation}"
        );
    }
}

/// An agent's capability grants exactly what it was issued and nothing adjacent. A grant of one
/// action does not confer another.
#[test]
fn a_grant_of_one_action_confers_no_other() {
    let chain = chain();
    for granted in DelegatedAction::ALL {
        let capability = delegated_capability(9, [granted], NOT_AFTER);
        for operation in Operation::ALL {
            let (_, outcome) =
                DecisionLedger::empty().authorize(&chain, &capability, &request(9, operation));
            let expected_ok = operation.delegable_action() == Some(granted);
            assert_eq!(
                outcome.is_ok(),
                expected_ok,
                "`{granted}` decided `{operation}` as {}",
                outcome.is_ok()
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The record.
// ---------------------------------------------------------------------------------------------

/// Every decision — granted and refused — carries its reason, its subject, its operation, its tier
/// and the epoch in force, in the order it was made.
#[test]
fn every_decision_is_recorded_with_its_reason() {
    let chain = chain();
    let ledger = DecisionLedger::empty();
    assert!(ledger.is_empty());

    let (ledger, _) = ledger.authorize(
        &chain,
        &agent_capability(9),
        &request(9, Operation::AuthorChangeSet),
    );
    let (ledger, _) = ledger.authorize(
        &chain,
        &agent_capability(9),
        &request(8, Operation::Replicate),
    );
    let (ledger, _) = ledger.authorize_publication(
        &chain,
        &human(1),
        &human_capability(1, every_human_action()),
        &request(1, Operation::AdvanceCanonicalHead),
    );

    let entries = ledger.entries();
    assert_eq!(entries.len(), 3);
    assert_eq!(
        entries.iter().map(|e| e.sequence()).collect::<Vec<_>>(),
        vec![0, 1, 2],
        "the ledger is ordered by its own sequence, never by a clock"
    );

    assert!(entries[0].outcome().is_granted());
    assert_eq!(entries[0].operation(), Operation::AuthorChangeSet);
    assert_eq!(entries[0].tier(), Delegated::NAME);
    assert_eq!(entries[0].in_force(), EPOCH);

    assert_eq!(
        entries[1].outcome().reason(),
        Some(DenialReason::SubjectMismatch)
    );
    assert_eq!(entries[1].subject(), key(8));

    assert!(entries[2].outcome().is_granted());
    assert_eq!(entries[2].tier(), HumanHeld::NAME);

    assert_eq!(ledger.denials().count(), 1);
    // Every reason renders. A denial nobody can read is a denial nobody acts on.
    for record in ledger.entries() {
        assert!(!record.to_string().is_empty());
        assert!(record.to_string().contains(record.operation().as_str()));
    }
}

/// The reason on a denial and the reason in its ledger entry are the same value. Two spellings of
/// "why" is how an audit trail and an error message diverge.
#[test]
fn the_recorded_reason_is_the_returned_reason() {
    let chain = chain();
    let cases: Vec<(u8, Operation, Capability<Delegated>)> = vec![
        (8, Operation::ReadWorkspace, agent_capability(9)),
        (9, Operation::AdvanceCanonicalHead, agent_capability(9)),
        (
            9,
            Operation::RunValidation,
            delegated_capability(9, [DelegatedAction::ReadWorkspace], NOT_AFTER),
        ),
    ];
    for (subject, operation, capability) in cases {
        let (ledger, outcome) =
            DecisionLedger::empty().authorize(&chain, &capability, &request(subject, operation));
        let denial = outcome.expect_err("these cases all deny");
        assert_eq!(
            ledger.last().expect("one record").outcome().reason(),
            Some(denial.reason())
        );
        assert_eq!(
            ledger.last().expect("one record").subject(),
            denial.subject()
        );
        assert_eq!(
            ledger.last().expect("one record").operation(),
            denial.operation()
        );
    }
}

// ---------------------------------------------------------------------------------------------
// The vocabulary itself.
// ---------------------------------------------------------------------------------------------

/// Exactly one operation has no delegated form, and it is the one that advances canonical state.
///
/// The guard that fires if a lane adds a canonical-advance action to the delegable vocabulary: the
/// new variant would have to be listed in `Operation::delegable_action`, and this assertion counts.
#[test]
fn exactly_one_operation_has_no_delegated_form() {
    let undelegable: Vec<_> = Operation::ALL
        .into_iter()
        .filter(|operation| operation.delegable_action().is_none())
        .collect();
    assert_eq!(undelegable, vec![Operation::AdvanceCanonicalHead]);

    let advancing: Vec<_> = Operation::ALL
        .into_iter()
        .filter(Operation::advances_canonical_state)
        .collect();
    assert_eq!(advancing, vec![Operation::AdvanceCanonicalHead]);

    // Every delegable action is reachable as an operation, so no action is granted with no gate.
    for action in DelegatedAction::ALL {
        assert!(
            Operation::ALL
                .into_iter()
                .any(|operation| operation.delegable_action() == Some(action)),
            "`{action}` is grantable and no operation checks it"
        );
    }
}

/// Only `human` gets past `HumanPrincipal::enrol`, and the crate asks `mesh-types` rather than
/// restating the rule.
#[test]
fn only_a_human_enrols_as_an_approver() {
    let mut enrolled = 0;
    for kind in mesh_types::ActorKind::ALL {
        match HumanPrincipal::enrol(Principal::new(key(1), kind)) {
            Ok(_) => {
                assert_eq!(kind, mesh_types::ActorKind::Human);
                enrolled += 1;
            }
            Err(PrincipalError::MayNotHoldApprovalCapability { kind: refused }) => {
                assert_eq!(refused, kind);
                assert!(!kind.may_hold_approval_capability());
            }
        }
    }
    assert_eq!(enrolled, 1, "exactly one actor kind may approve");
}

/// A `Grant` is not publication authority, whatever its tier. The types do not convert.
#[test]
fn a_grant_is_not_publication_authority() {
    let chain = chain();
    let (_, outcome) = DecisionLedger::empty().authorize(
        &chain,
        &human_capability(1, every_human_action()),
        &request(1, Operation::ReadWorkspace),
    );
    let grant = outcome.expect("a human may read");
    assert_eq!(grant.tier(), HumanHeld::NAME);
    assert_eq!(grant.operation(), Operation::ReadWorkspace);
    assert!(!grant.operation().advances_canonical_state());
    // There is no method on `Grant` that yields a `PublicationAuthority`, and no constructor for
    // one outside this crate's ledger. The `compile_fail` proof is in `lib.rs`.
}

/// A budget of zero is a leaf: it holds authority and hands none on.
#[test]
fn a_leaf_capability_delegates_nothing() {
    let custody = support::AttestingCustody::holding(1);
    let attestation = custody.attest_human().expect("attestation");
    let leaf = Capability::<HumanHeld>::root(
        &attestation,
        WORKSPACE,
        every_human_action(),
        EPOCH,
        NOT_AFTER,
        DelegationBudget::new(0),
    );
    assert_eq!(
        leaf.delegate(&Delegation::new(
            key(2),
            [DelegatedAction::ReadWorkspace],
            NOT_AFTER
        )),
        Err(DelegationError::BudgetExhausted)
    );
}
