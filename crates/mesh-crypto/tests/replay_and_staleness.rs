//! Replay, staleness and substitution — the four ways a token that was once valid is presented
//! again when it should not be.
//!
//! A capability token is bearer evidence. Whoever holds the bytes can present them, so every bound
//! that limits when and where they are good has to be inside the signature and checked on every
//! path. A field that is checked in one caller and not another is a field that is not checked.

mod support;

use mesh_crypto::{
    CapabilityCodec, CapabilityError, CapabilityParts, CapabilityToken, Delegated,
    DelegationBudget, Expiry, KeyCustody, PolicyEpoch, TokenError, VerifyError,
};

use support::{workspace, PlumbingCodec, PlumbingScheme, TestCustody};

const EPOCH: PolicyEpoch = PolicyEpoch::new(4);
const EXPIRES_AT: Expiry = Expiry::at_unix_millis(5_000);

fn subject() -> mesh_crypto::ActorKey {
    mesh_crypto::ActorKey::from_public_bytes([0x77; 32])
}

fn issued(custody: &TestCustody) -> CapabilityToken {
    let parts = CapabilityParts::new(
        custody.public_key(),
        subject(),
        workspace(),
        "delegated".to_owned(),
        vec!["read-workspace".to_owned()],
        EPOCH,
        EXPIRES_AT,
        DelegationBudget::new(1),
    );
    let payload = PlumbingCodec::encode(&parts);
    let signature = custody
        .sign(&CapabilityToken::payload_to_sign(&payload))
        .expect("custody signs");
    CapabilityToken::new(signature, payload).expect("small payload")
}

#[test]
fn a_token_is_good_before_its_expiry_and_dead_after_it() {
    let custody = TestCustody::new(0x55);
    let token = issued(&custody);

    token
        .verify::<PlumbingScheme, PlumbingCodec, Delegated>(
            &custody.public_key(),
            &subject(),
            4_999,
            EPOCH,
        )
        .expect("valid one millisecond before expiry");

    assert_eq!(
        token.verify::<PlumbingScheme, PlumbingCodec, Delegated>(
            &custody.public_key(),
            &subject(),
            5_000,
            EPOCH
        ),
        Err(TokenError::NotCurrent(CapabilityError::Expired {
            not_after: EXPIRES_AT,
            now: 5_000
        })),
        "the expiry is inclusive of the moment it names"
    );
}

/// Revocation is an epoch rotation, so a replayed token from a past epoch is dead on every peer at
/// once, online or not. This is what makes revocation work without a global synchronous step.
#[test]
fn a_token_from_a_rotated_policy_epoch_is_refused() {
    let custody = TestCustody::new(0x55);
    let token = issued(&custody);
    assert_eq!(
        token.verify::<PlumbingScheme, PlumbingCodec, Delegated>(
            &custody.public_key(),
            &subject(),
            1_000,
            PolicyEpoch::new(5)
        ),
        Err(TokenError::NotCurrent(CapabilityError::WrongEpoch {
            issued_in: EPOCH,
            current: PolicyEpoch::new(5)
        }))
    );
}

/// Every single-byte change to a signed payload has to invalidate the token. This is TG-4's shape
/// one level down from the approval envelope: if any byte of the bound content can move without
/// the signature noticing, an attacker substitutes the bytes a signer never saw.
#[test]
fn no_single_byte_of_the_payload_can_be_changed_without_invalidating_the_token() {
    let custody = TestCustody::new(0x55);
    let wire = issued(&custody).to_wire();
    let payload_starts = 4 + 1 + 64 + 4;

    for index in payload_starts..wire.len() {
        for bit in 0..8u32 {
            let mut mutated = wire.clone();
            mutated[index] ^= 1 << bit;
            let Ok(token) = CapabilityToken::from_wire(&mutated) else {
                continue;
            };
            let outcome = token.verify::<PlumbingScheme, PlumbingCodec, Delegated>(
                &custody.public_key(),
                &subject(),
                1_000,
                EPOCH,
            );
            assert!(
                outcome.is_err(),
                "flipping bit {bit} of payload byte {index} left the token valid"
            );
        }
    }
}

/// And the signature itself.
#[test]
fn no_single_byte_of_the_signature_can_be_changed_without_invalidating_the_token() {
    let custody = TestCustody::new(0x55);
    let wire = issued(&custody).to_wire();
    for index in 5..5 + 64 {
        let mut mutated = wire.clone();
        mutated[index] ^= 0x80;
        let token = CapabilityToken::from_wire(&mutated).expect("still a well formed envelope");
        assert_eq!(
            token.verify::<PlumbingScheme, PlumbingCodec, Delegated>(
                &custody.public_key(),
                &subject(),
                1_000,
                EPOCH
            ),
            Err(TokenError::Signature(VerifyError::Mismatch)),
            "signature byte {index} was not covered"
        );
    }
}

/// A token issued by one key must not verify under another, even when the caller supplies the
/// wrong one by mistake. There is no "try every key I know" path in this API for exactly this
/// reason: the caller names the issuer it expects.
#[test]
fn a_token_does_not_verify_under_a_key_that_did_not_issue_it() {
    let issuer = TestCustody::new(0x55);
    let other = TestCustody::new(0x56);
    let token = issued(&issuer);
    assert_eq!(
        token.verify::<PlumbingScheme, PlumbingCodec, Delegated>(
            &other.public_key(),
            &subject(),
            1_000,
            EPOCH
        ),
        Err(TokenError::Signature(VerifyError::Mismatch))
    );
}

/// The domain separator is what stops a signature made over a capability token from being
/// presented as a signature over something else. Re-framing the same bytes in another domain must
/// produce a different message.
#[test]
fn the_signature_covers_the_domain_and_not_only_the_payload() {
    let custody = TestCustody::new(0x55);
    let payload = b"identical bytes";
    let in_token_domain = CapabilityToken::payload_to_sign(payload);
    let in_another_domain = mesh_crypto::SigningPayload::new(
        mesh_crypto::DomainSeparator::new("mesh.v0.approval-envelope"),
        payload,
    );

    assert_ne!(in_token_domain.as_bytes(), in_another_domain.as_bytes());

    let signature = custody.sign(&in_another_domain).expect("custody signs");
    let token = CapabilityToken::new(signature, payload.to_vec()).expect("small payload");
    assert_eq!(
        token.verify::<PlumbingScheme, PlumbingCodec, Delegated>(
            &custody.public_key(),
            &subject(),
            1_000,
            EPOCH
        ),
        Err(TokenError::Signature(VerifyError::Mismatch)),
        "a signature from another domain was accepted as a capability token"
    );
}
