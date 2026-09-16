//! The harness that will judge the Ed25519 backend, judged first.
//!
//! A conformance suite nobody has watched fail is a suite that might be asserting nothing. These
//! tests run it against implementations known to be wrong and require it to say so, so that when it
//! passes for a real backend the pass carries information.

mod support;

use mesh_crypto::conformance::{
    check_codec_conformance, check_ed25519_conformance, ConformanceFailure,
    ED25519_REJECTION_VECTORS, ED25519_VECTORS,
};
use mesh_crypto::{
    ActorKey, CapabilityCodec, CapabilityParts, CodecError, DelegationBudget, Expiry, PolicyEpoch,
};

use support::{workspace, AlwaysAcceptScheme, PlumbingCodec, PlumbingScheme};

/// The single most dangerous thing a lane could merge into this crate.
#[test]
fn the_harness_rejects_a_backend_that_verifies_everything() {
    let outcome = check_ed25519_conformance::<AlwaysAcceptScheme>();
    assert_eq!(
        outcome,
        Err(ConformanceFailure::ShouldReject {
            vector: "non-canonical-scalar"
        }),
        "a scheme that accepts unconditionally passed conformance"
    );
}

/// And the second most dangerous: the test double in this directory quietly becoming the shipped
/// implementation.
#[test]
fn the_harness_rejects_the_test_double_as_not_being_ed25519() {
    let outcome = check_ed25519_conformance::<PlumbingScheme>();
    assert!(
        matches!(outcome, Err(ConformanceFailure::ShouldVerify { .. })),
        "the plumbing double passed an Ed25519 conformance run: {outcome:?}"
    );
}

/// The malleability vector is the one a hand-written implementation is most likely to get wrong,
/// so it is the one whose presence is asserted rather than assumed.
#[test]
fn the_corpus_covers_the_failures_a_hand_written_implementation_actually_has() {
    let names: Vec<&str> = ED25519_REJECTION_VECTORS
        .iter()
        .map(|vector| vector.name)
        .collect();
    for required in [
        "non-canonical-scalar",
        "low-order-public-key",
        "tampered-r",
        "tampered-s",
        "tampered-message",
        "wrong-public-key",
    ] {
        assert!(
            names.contains(&required),
            "no `{required}` rejection vector"
        );
    }
    assert_eq!(ED25519_VECTORS.len(), 4, "the four RFC 8032 §7.1 vectors");
}

fn corpus() -> Vec<CapabilityParts> {
    vec![
        CapabilityParts::new(
            ActorKey::from_public_bytes([1; 32]),
            ActorKey::from_public_bytes([2; 32]),
            workspace(),
            "delegated".to_owned(),
            vec!["read-workspace".to_owned(), "author-change-set".to_owned()],
            PolicyEpoch::new(3),
            Expiry::at_unix_millis(9_000),
            DelegationBudget::new(1),
        ),
        CapabilityParts::new(
            ActorKey::from_public_bytes([4; 32]),
            ActorKey::from_public_bytes([4; 32]),
            workspace(),
            "human-held".to_owned(),
            vec!["advance-canonical-head".to_owned()],
            PolicyEpoch::new(0),
            Expiry::at_unix_millis(u64::MAX),
            DelegationBudget::new(255),
        ),
    ]
}

#[test]
fn the_codec_harness_accepts_a_codec_that_round_trips() {
    check_codec_conformance::<PlumbingCodec>(&corpus()).expect("the plumbing codec round trips");
}

#[test]
fn the_codec_harness_refuses_an_empty_corpus() {
    assert_eq!(
        check_codec_conformance::<PlumbingCodec>(&[]),
        Err(ConformanceFailure::EmptyCorpus)
    );
}

/// A codec that encodes the same value two different ways breaks every signature made over it, a
/// week later, on another machine. The harness has to catch it.
struct NonDeterministicCodec;

impl CapabilityCodec for NonDeterministicCodec {
    const CODEC: &'static str = "non-deterministic/0";

    fn encode(parts: &CapabilityParts) -> Vec<u8> {
        use std::sync::atomic::{AtomicU8, Ordering};
        static COUNTER: AtomicU8 = AtomicU8::new(0);
        let mut out = PlumbingCodec::encode(parts);
        out.push(COUNTER.fetch_add(1, Ordering::Relaxed));
        out
    }

    fn decode(bytes: &[u8]) -> Result<CapabilityParts, CodecError> {
        let split = bytes.len().checked_sub(1).ok_or(CodecError::Malformed)?;
        PlumbingCodec::decode(&bytes[..split])
    }
}

#[test]
fn the_codec_harness_catches_a_non_deterministic_encoder() {
    assert_eq!(
        check_codec_conformance::<NonDeterministicCodec>(&corpus()),
        Err(ConformanceFailure::NotDeterministic)
    );
}

/// A codec whose decoder does not reconstruct what its encoder wrote is the same failure one step
/// later: the peer checks a signature over bytes that mean something else to it.
struct LossyCodec;

impl CapabilityCodec for LossyCodec {
    const CODEC: &'static str = "lossy/0";

    fn encode(parts: &CapabilityParts) -> Vec<u8> {
        PlumbingCodec::encode(parts)
    }

    fn decode(bytes: &[u8]) -> Result<CapabilityParts, CodecError> {
        let parts = PlumbingCodec::decode(bytes)?;
        Ok(CapabilityParts::new(
            *parts.issuer(),
            *parts.subject(),
            parts.workspace(),
            parts.tier().to_owned(),
            parts.actions().to_vec(),
            parts.policy_epoch(),
            Expiry::at_unix_millis(parts.not_after().as_unix_millis().saturating_add(1)),
            parts.budget(),
        ))
    }
}

#[test]
fn the_codec_harness_catches_a_decoder_that_changes_a_field() {
    assert_eq!(
        check_codec_conformance::<LossyCodec>(&corpus()),
        Err(ConformanceFailure::RoundTripChangedTheCapability)
    );
}
