//! TG-2 and TG-3, as tests: a capability cannot be widened, and no agent-scoped capability can
//! carry publication authority.
//!
//! Several of these are written as assertions about behaviour that the compiler *already* refuses
//! to let anyone write. Where that is so, the commented line is the one that does not compile, and
//! the assertion beside it is the runtime shadow of the same fact. The compiler is the enforcement;
//! the test is the record of what would otherwise have to be remembered.

mod support;

use mesh_crypto::{
    ActorKey, Capability, CapabilityToken, DelegatedAction, Delegation, DelegationBudget,
    DelegationError, Expiry, HumanAction, HumanKeyCustody, KeyCustody, PolicyEpoch,
};

use support::{workspace, PlumbingCodec, PlumbingScheme, TestCustody};

const EPOCH: PolicyEpoch = PolicyEpoch::new(7);
const NOW: u64 = 1_000;
const HORIZON: Expiry = Expiry::at_unix_millis(10_000);

fn human_root() -> Capability<mesh_crypto::HumanHeld> {
    let custody = TestCustody::new(0x11);
    let attestation = custody.attest_human().expect("test custody attests");
    Capability::root(
        &attestation,
        workspace(),
        [
            HumanAction::AdvanceCanonicalHead,
            HumanAction::Delegated(DelegatedAction::AuthorChangeSet),
            HumanAction::Delegated(DelegatedAction::ReadWorkspace),
            HumanAction::Delegated(DelegatedAction::RequestReview),
        ],
        EPOCH,
        HORIZON,
        DelegationBudget::new(2),
    )
}

fn agent() -> ActorKey {
    ActorKey::from_public_bytes([0x22; 32])
}

#[test]
fn a_human_capability_carries_publication_authority_and_a_delegated_one_cannot() {
    let root = human_root();
    assert!(root.advances_canonical_head());

    let agent_capability = root
        .delegate(&Delegation::new(
            agent(),
            [
                DelegatedAction::AuthorChangeSet,
                DelegatedAction::ReadWorkspace,
            ],
            HORIZON,
        ))
        .expect("a narrower delegation");

    // `DelegatedAction::AdvanceCanonicalHead` does not exist, so this line cannot be written:
    //
    //     Delegation::new(agent(), [DelegatedAction::AdvanceCanonicalHead], HORIZON)
    //
    // error[E0599]: no variant named `AdvanceCanonicalHead` found for enum `DelegatedAction`
    //
    // The runtime shadow of that fact:
    assert!(!agent_capability.advances_canonical_head());
    assert_eq!(agent_capability.tier(), "delegated");
    for action in DelegatedAction::ALL {
        assert_ne!(action.as_str(), "advance-canonical-head");
    }
}

#[test]
fn every_delegation_of_a_delegation_stays_at_the_delegated_tier() {
    let agent_capability = human_root()
        .delegate(&Delegation::new(
            agent(),
            [
                DelegatedAction::AuthorChangeSet,
                DelegatedAction::ReadWorkspace,
            ],
            HORIZON,
        ))
        .expect("delegation");

    let run = agent_capability
        .delegate(&Delegation::new(
            ActorKey::from_public_bytes([0x33; 32]),
            [DelegatedAction::ReadWorkspace],
            HORIZON,
        ))
        .expect("an agent-run delegation");

    assert_eq!(run.tier(), "delegated");
    assert!(!run.advances_canonical_head());
    // The chain is finite: the budget went 2 → 1 → 0, and 0 ends it.
    assert_eq!(run.budget().remaining(), 0);
    assert_eq!(
        run.delegate(&Delegation::new(
            ActorKey::from_public_bytes([0x44; 32]),
            [DelegatedAction::ReadWorkspace],
            HORIZON,
        )),
        Err(DelegationError::BudgetExhausted)
    );
}

#[test]
fn a_delegation_cannot_grant_what_the_issuer_does_not_hold() {
    let agent_capability = human_root()
        .delegate(&Delegation::new(
            agent(),
            [DelegatedAction::ReadWorkspace],
            HORIZON,
        ))
        .expect("delegation");

    assert_eq!(
        agent_capability.delegate(&Delegation::new(
            ActorKey::from_public_bytes([0x33; 32]),
            [DelegatedAction::AuthorChangeSet],
            HORIZON,
        )),
        Err(DelegationError::ActionNotHeld {
            action: DelegatedAction::AuthorChangeSet
        })
    );
}

#[test]
fn a_delegation_cannot_outlive_its_parent() {
    let root = human_root();
    assert_eq!(
        root.delegate(&Delegation::new(
            agent(),
            [DelegatedAction::ReadWorkspace],
            Expiry::at_unix_millis(HORIZON.as_unix_millis() + 1),
        )),
        Err(DelegationError::ExpiryWidened {
            parent: HORIZON,
            requested: Expiry::at_unix_millis(HORIZON.as_unix_millis() + 1),
        })
    );
}

#[test]
fn a_delegation_grants_something_or_nothing_at_all() {
    assert_eq!(
        human_root().delegate(&Delegation::new(agent(), [], HORIZON)),
        Err(DelegationError::EmptyGrant)
    );
}

/// The attack the type system alone does not stop: a hostile peer writes the *bytes* of a delegated
/// capability that claims publication authority, signs them with its own key, and presents them.
///
/// It fails at decode, before any authorization check, because `advance-canonical-head` is not a
/// name the delegated vocabulary can resolve.
#[test]
fn a_forged_delegated_token_claiming_publication_authority_does_not_decode() {
    let custody = TestCustody::new(0x22);
    let forged = mesh_crypto::CapabilityParts::new(
        custody.public_key(),
        agent(),
        workspace(),
        "delegated".to_owned(),
        vec!["advance-canonical-head".to_owned()],
        EPOCH,
        HORIZON,
        DelegationBudget::new(1),
    );
    let payload = <PlumbingCodec as mesh_crypto::CapabilityCodec>::encode(&forged);
    let signature = custody
        .sign(&CapabilityToken::payload_to_sign(&payload))
        .expect("the forger can sign with its own key");
    let token = CapabilityToken::new(signature, payload).expect("small payload");

    let outcome = token.verify::<PlumbingScheme, PlumbingCodec, mesh_crypto::Delegated>(
        &custody.public_key(),
        &agent(),
        NOW,
        EPOCH,
    );
    assert!(
        matches!(outcome, Err(mesh_crypto::TokenError::Parts(_))),
        "a signature over a lie is still a lie: {outcome:?}"
    );
}

/// The same forgery at the human tier. The bytes decode — `advance-canonical-head` *is* in the
/// human vocabulary — and the token is still refused, because it was not signed by the key the
/// verifier trusts.
#[test]
fn a_forged_human_token_fails_against_the_key_the_verifier_trusts() {
    let forger = TestCustody::new(0x22);
    let human = TestCustody::new(0x11);

    let forged = mesh_crypto::CapabilityParts::new(
        human.public_key(),
        forger.public_key(),
        workspace(),
        "human-held".to_owned(),
        vec!["advance-canonical-head".to_owned()],
        EPOCH,
        HORIZON,
        DelegationBudget::new(1),
    );
    let payload = <PlumbingCodec as mesh_crypto::CapabilityCodec>::encode(&forged);
    let signature = forger
        .sign(&CapabilityToken::payload_to_sign(&payload))
        .expect("the forger signs with its own key");
    let token = CapabilityToken::new(signature, payload).expect("small payload");

    let outcome = token.verify::<PlumbingScheme, PlumbingCodec, mesh_crypto::HumanHeld>(
        &human.public_key(),
        &forger.public_key(),
        NOW,
        EPOCH,
    );
    assert_eq!(
        outcome,
        Err(mesh_crypto::TokenError::Signature(
            mesh_crypto::VerifyError::Mismatch
        ))
    );
}

/// A token signed by a key the caller trusts must still name that key as its issuer. Otherwise a
/// peer that legitimately holds one key can issue capabilities "from" another.
#[test]
fn a_token_cannot_claim_an_issuer_other_than_the_key_it_was_signed_with() {
    let signer = TestCustody::new(0x22);
    let parts = mesh_crypto::CapabilityParts::new(
        ActorKey::from_public_bytes([0x99; 32]),
        agent(),
        workspace(),
        "delegated".to_owned(),
        vec!["read-workspace".to_owned()],
        EPOCH,
        HORIZON,
        DelegationBudget::new(1),
    );
    let payload = <PlumbingCodec as mesh_crypto::CapabilityCodec>::encode(&parts);
    let signature = signer
        .sign(&CapabilityToken::payload_to_sign(&payload))
        .expect("signs");
    let token = CapabilityToken::new(signature, payload).expect("small payload");

    assert_eq!(
        token.verify::<PlumbingScheme, PlumbingCodec, mesh_crypto::Delegated>(
            &signer.public_key(),
            &agent(),
            NOW,
            EPOCH
        ),
        Err(mesh_crypto::TokenError::IssuerMismatch)
    );
}

/// A capability token is bearer evidence: the bytes are not secret, so a thief can present them.
/// The only bound that stops the replay is the caller stating who it authenticated, which is why
/// `verify` takes it rather than offering it as a getter.
#[test]
fn a_stolen_token_does_not_verify_for_the_thief() {
    let issuer = TestCustody::new(0x11);
    let parts = mesh_crypto::CapabilityParts::new(
        issuer.public_key(),
        agent(),
        workspace(),
        "delegated".to_owned(),
        vec!["read-workspace".to_owned()],
        EPOCH,
        HORIZON,
        DelegationBudget::new(1),
    );
    let payload = <PlumbingCodec as mesh_crypto::CapabilityCodec>::encode(&parts);
    let signature = issuer
        .sign(&CapabilityToken::payload_to_sign(&payload))
        .expect("the issuer signs a legitimate token");
    let token = CapabilityToken::new(signature, payload).expect("small payload");

    // The rightful holder presents it.
    token
        .verify::<PlumbingScheme, PlumbingCodec, mesh_crypto::Delegated>(
            &issuer.public_key(),
            &agent(),
            NOW,
            EPOCH,
        )
        .expect("the subject may present its own token");

    // A different actor replays the identical bytes.
    let thief = ActorKey::from_public_bytes([0x66; 32]);
    assert_eq!(
        token.verify::<PlumbingScheme, PlumbingCodec, mesh_crypto::Delegated>(
            &issuer.public_key(),
            &thief,
            NOW,
            EPOCH
        ),
        Err(mesh_crypto::TokenError::SubjectMismatch)
    );
}
