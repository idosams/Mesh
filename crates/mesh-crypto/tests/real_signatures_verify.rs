//! A real Ed25519 signature, made by a real secret, checked by the shipped verifier.
//!
//! Every other test in this crate either checks the plumbing with a deliberate non-signature
//! ([`support::PlumbingScheme`]) or holds the verifier to a corpus of published bytes. Neither
//! closes the loop, and the loop is the claim: **something signed with a private half verifies, and
//! nothing else does.** These tests close it.
//!
//! The signing half is a `tests/`-only dev-dependency (see [`support::RealSigner`]). It ships in no
//! binary, no production path can reach it, and the library it is testing still holds no secret.

mod support;

use mesh_crypto::conformance::{ED25519_REJECTION_VECTORS, ED25519_VECTORS};
use mesh_crypto::{
    CapabilityCodec, CapabilityParts, CapabilityToken, Delegated, DelegationBudget, Ed25519,
    Expiry, PolicyEpoch, SignatureScheme, TokenError, VerifyError,
};
use mesh_types::{PublicKey, Signature};

use support::{workspace, PlumbingCodec, RealSigner};

/// RFC 8032 §7.1: the seed each published vector was generated from.
///
/// The vectors in `mesh_crypto::conformance` carry the public key and the signature. These are the
/// other half — the secret seeds the RFC publishes beside them — and they are what makes the
/// signing direction checkable at all. They are test data from a public standards document, not key
/// material: every one of them has been printed in RFC 8032 since 2017.
const RFC_8032_SEEDS: [&str; 4] = [
    "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
    "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb",
    "c5aa8df43f9f837bedb7442f31dcb7b166d38535076f094b85ce3a2e0b4458f7",
    "833fe62409237b9d62ec77587520911e9a759cec1d19755b7da901b96dca3d42",
];

fn unhex(text: &str) -> Vec<u8> {
    assert!(text.len() % 2 == 0, "hex is even-length: {text}");
    (0..text.len() / 2)
        .map(|index| {
            u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).expect("a hex byte pair")
        })
        .collect()
}

fn seed(index: usize) -> [u8; 32] {
    unhex(RFC_8032_SEEDS[index])
        .try_into()
        .expect("a 32-byte seed")
}

/// The signing direction, against the standard.
///
/// Ed25519 is deterministic, so a seed and a message fix the signature exactly. Three claims are
/// separable here and all three are asserted: the public half is derived from the seed the way RFC
/// 8032 says, the signature over the message is byte-identical to the published one, and the
/// shipped verifier accepts it. A backend could pass the third and fail the first two — that is
/// what "compatible with itself" looks like — and this is the test that tells them apart.
#[test]
fn signing_reproduces_every_published_rfc_8032_signature_byte_for_byte() {
    for (index, vector) in ED25519_VECTORS.iter().enumerate() {
        let signer = RealSigner::from_seed(&seed(index));

        assert_eq!(
            signer.public_key().as_bytes().to_vec(),
            unhex(vector.public_key_hex),
            "vector `{}`: the public key derived from the seed is not the published one",
            vector.name
        );

        let message = unhex(vector.message_hex);
        let signature = signer.sign(&message);
        assert_eq!(
            signature.as_bytes().to_vec(),
            unhex(vector.signature_hex),
            "vector `{}`: the signature is not the published one",
            vector.name
        );

        Ed25519::verify(&signer.public_key(), &message, &signature).unwrap_or_else(|error| {
            panic!(
                "vector `{}`: our own signature did not verify: {error}",
                vector.name
            )
        });
    }
}

/// A freshly made signature verifies, and every single-bit change to it does not.
#[test]
fn a_signature_verifies_and_no_mutation_of_it_does() {
    let signer = RealSigner::from_filler(0x11);
    let message = b"an approval envelope would go here";
    let signature = signer.sign(message);

    Ed25519::verify(&signer.public_key(), message, &signature).expect("a real signature verifies");

    for byte in 0..64 {
        for bit in 0..8 {
            let mut mutated = *signature.as_bytes();
            mutated[byte] ^= 1 << bit;
            assert!(
                Ed25519::verify(
                    &signer.public_key(),
                    message,
                    &Signature::from_bytes(mutated)
                )
                .is_err(),
                "flipping bit {bit} of byte {byte} produced a second valid signature"
            );
        }
    }
}

/// The message is bound, not just the key.
#[test]
fn a_signature_over_one_message_does_not_verify_over_another() {
    let signer = RealSigner::from_filler(0x22);
    let signature = signer.sign(b"advance to head A");

    assert_eq!(
        Ed25519::verify(&signer.public_key(), b"advance to head B", &signature),
        Err(VerifyError::Mismatch)
    );
}

/// The key is bound, not just the message.
#[test]
fn a_signature_does_not_verify_under_another_key() {
    let signer = RealSigner::from_filler(0x33);
    let other = RealSigner::from_filler(0x44);
    let message = b"the same bytes under two keys";

    assert_eq!(
        Ed25519::verify(&other.public_key(), message, &signer.sign(message)),
        Err(VerifyError::Mismatch)
    );
}

/// Malleability, end to end on a signature this test produced rather than on a canned vector.
///
/// `S + L` is congruent to `S` modulo the group order, so an implementation that reduces before
/// checking accepts it and the system has two valid encodings of one signature. The verifier must
/// refuse the *encoding*, and it must refuse it as a malformed signature rather than as a mismatch,
/// because the two say different things to an operator.
#[test]
fn adding_the_group_order_to_a_real_signature_is_refused_as_malformed() {
    let signer = RealSigner::from_filler(0x55);
    let message = b"one signature, one encoding";
    let signature = signer.sign(message);
    Ed25519::verify(&signer.public_key(), message, &signature).expect("the original verifies");

    // L, little-endian.
    const GROUP_ORDER: [u8; 32] = [
        0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde,
        0x14, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x10,
    ];

    let mut malleated = *signature.as_bytes();
    let mut carry: u16 = 0;
    for index in 0..32 {
        let sum = u16::from(malleated[32 + index]) + u16::from(GROUP_ORDER[index]) + carry;
        malleated[32 + index] = sum as u8;
        carry = sum >> 8;
    }
    assert_eq!(carry, 0, "S + L overflowed 32 bytes; pick another key");
    assert_ne!(
        malleated,
        *signature.as_bytes(),
        "S + L is a different encoding"
    );

    assert_eq!(
        Ed25519::verify(
            &signer.public_key(),
            message,
            &Signature::from_bytes(malleated)
        ),
        Err(VerifyError::MalformedSignature),
        "S + L verified: every signature in the system is malleable"
    );
}

/// The corpus is not a set of arbitrary bytes: each rejection vector is one edit away from a
/// signature that does verify. This runs the positive vector beside its rejection to say so.
#[test]
fn every_rejection_vector_is_refused_by_the_shipped_verifier() {
    for vector in ED25519_REJECTION_VECTORS {
        let key: [u8; 32] = unhex(vector.public_key_hex).try_into().expect("32 bytes");
        let signature: [u8; 64] = unhex(vector.signature_hex).try_into().expect("64 bytes");
        assert!(
            Ed25519::verify(
                &PublicKey::from_bytes(key),
                &unhex(vector.message_hex),
                &Signature::from_bytes(signature)
            )
            .is_err(),
            "rejection vector `{}` was accepted",
            vector.name
        );
    }
}

const EPOCH: PolicyEpoch = PolicyEpoch::new(7);
const EXPIRES_AT: Expiry = Expiry::at_unix_millis(9_000);

/// A capability token, signed with a real Ed25519 secret, checked by the shipped verifier.
///
/// This is the call that could not be written at all before a backend existed: `verify` needs a
/// `SignatureScheme`, and with none in the workspace the failure mode was a compile error at every
/// call site. It compiles now, and it runs a real check.
#[test]
fn a_capability_token_signed_with_a_real_key_verifies_end_to_end() {
    let issuer = RealSigner::from_filler(0x66);
    let subject = RealSigner::from_filler(0x77);

    let parts = CapabilityParts::new(
        issuer.actor_key(),
        subject.actor_key(),
        workspace(),
        "delegated".to_owned(),
        vec!["read-workspace".to_owned(), "author-change-set".to_owned()],
        EPOCH,
        EXPIRES_AT,
        DelegationBudget::new(1),
    );
    let payload = PlumbingCodec::encode(&parts);
    let signature = issuer.sign_payload(&CapabilityToken::payload_to_sign(&payload));
    let token = CapabilityToken::new(signature, payload.clone()).expect("small payload");

    let capability = token
        .verify::<Ed25519, PlumbingCodec, Delegated>(
            &issuer.actor_key(),
            &subject.actor_key(),
            1_000,
            EPOCH,
        )
        .expect("a real signature over a well-formed capability");
    assert_eq!(capability.issuer(), &issuer.actor_key());
    assert_eq!(capability.subject(), &subject.actor_key());

    // One byte of the signed payload, changed. The signature is over the framed payload, so this
    // has to fail at the signature and not later at a field comparison.
    let mut tampered = payload;
    tampered[8] ^= 0x01;
    let forged = CapabilityToken::new(signature, tampered).expect("small payload");
    assert_eq!(
        forged.verify::<Ed25519, PlumbingCodec, Delegated>(
            &issuer.actor_key(),
            &subject.actor_key(),
            1_000,
            EPOCH
        ),
        Err(TokenError::Signature(VerifyError::Mismatch)),
        "a token whose payload was edited after signing was accepted"
    );
}

/// A token an agent signed for itself is not a token from the issuer the caller trusts.
///
/// The signature check is the first gate and it is the one that has to hold: with a real backend,
/// "self-issued" is not a policy failure the later checks catch, it is a signature that was never
/// made by the expected issuer.
#[test]
fn a_token_signed_by_the_wrong_key_never_reaches_the_field_checks() {
    let issuer = RealSigner::from_filler(0x88);
    let impostor = RealSigner::from_filler(0x99);
    let subject = RealSigner::from_filler(0xaa);

    let parts = CapabilityParts::new(
        issuer.actor_key(),
        subject.actor_key(),
        workspace(),
        "delegated".to_owned(),
        vec!["read-workspace".to_owned()],
        EPOCH,
        EXPIRES_AT,
        DelegationBudget::new(1),
    );
    let payload = PlumbingCodec::encode(&parts);
    let signature = impostor.sign_payload(&CapabilityToken::payload_to_sign(&payload));
    let token = CapabilityToken::new(signature, payload).expect("small payload");

    assert_eq!(
        token.verify::<Ed25519, PlumbingCodec, Delegated>(
            &issuer.actor_key(),
            &subject.actor_key(),
            1_000,
            EPOCH
        ),
        Err(TokenError::Signature(VerifyError::Mismatch)),
        "a token signed by a key the caller does not trust was accepted"
    );
}

/// The central claim, with the signature no longer stubbed.
///
/// An agent holding a real key, able to produce real signatures over anything it likes, still
/// cannot mint itself publication authority: `advance-canonical-head` is not in `DelegatedAction`'s
/// vocabulary, so a delegated token naming it is refused *after* its signature has been checked and
/// found perfectly valid. The signature was never what stopped it.
#[test]
fn a_perfectly_signed_delegated_token_still_cannot_name_canonical_advancement() {
    let agent = RealSigner::from_filler(0xbb);

    let parts = CapabilityParts::new(
        agent.actor_key(),
        agent.actor_key(),
        workspace(),
        "delegated".to_owned(),
        vec!["advance-canonical-head".to_owned()],
        EPOCH,
        EXPIRES_AT,
        DelegationBudget::new(255),
    );
    let payload = PlumbingCodec::encode(&parts);
    let signature = agent.sign_payload(&CapabilityToken::payload_to_sign(&payload));
    let token = CapabilityToken::new(signature, payload).expect("small payload");

    let outcome = token.verify::<Ed25519, PlumbingCodec, Delegated>(
        &agent.actor_key(),
        &agent.actor_key(),
        1_000,
        EPOCH,
    );
    assert!(
        matches!(outcome, Err(TokenError::Parts(_))),
        "an agent minted itself publication authority with a valid signature: {outcome:?}"
    );
}
