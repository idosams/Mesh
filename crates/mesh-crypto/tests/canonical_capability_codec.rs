//! The product capability payload has one strict `mesh-cbor/0` spelling.

mod support;

use mesh_crypto::{
    ActorKey, CapabilityParts, CapabilityToken, CodecError, Delegated, DelegatedAction,
    DelegationBudget, Ed25519, Expiry, PolicyEpoch, TokenError, WorkspaceScope,
};
use mesh_types::CborWriter;
use support::RealSigner;

const FIXTURE: &str = include_str!("fixtures/capability-payload-v0.json");

fn fixture_string(field: &str) -> &str {
    let needle = format!("\"{field}\": \"");
    FIXTURE
        .split_once(&needle)
        .and_then(|(_, rest)| rest.split_once('"'))
        .map(|(value, _)| value)
        .unwrap_or_else(|| panic!("fixture field {field}"))
}

fn unhex(text: &str) -> Vec<u8> {
    assert_eq!(text.len() % 2, 0);
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char).to_digit(16).expect("hex") as u8;
            let low = (pair[1] as char).to_digit(16).expect("hex") as u8;
            (high << 4) | low
        })
        .collect()
}

fn parts(issuer: &RealSigner, subject: &RealSigner) -> CapabilityParts {
    CapabilityParts::new(
        issuer.actor_key(),
        subject.actor_key(),
        WorkspaceScope::from_bytes([3; 16]),
        "delegated".to_owned(),
        vec![
            "write-own-actor-state".to_owned(),
            "author-change-set".to_owned(),
            "author-change-set".to_owned(),
        ],
        PolicyEpoch::new(7),
        Expiry::at_unix_millis(200),
        DelegationBudget::new(0),
    )
}

fn fixture_parts() -> CapabilityParts {
    CapabilityParts::new(
        ActorKey::from_public_bytes([1; 32]),
        ActorKey::from_public_bytes([2; 32]),
        WorkspaceScope::from_bytes([3; 16]),
        "delegated".to_owned(),
        vec![
            "write-own-actor-state".to_owned(),
            "author-change-set".to_owned(),
            "author-change-set".to_owned(),
        ],
        PolicyEpoch::new(7),
        Expiry::at_unix_millis(200),
        DelegationBudget::new(0),
    )
}

fn token_for(issuer: &RealSigner, payload: Vec<u8>) -> CapabilityToken {
    let signature = issuer.sign_payload(&CapabilityToken::payload_to_sign(&payload));
    CapabilityToken::new(signature, payload).expect("bounded payload")
}

fn verify(payload: Vec<u8>) -> Result<mesh_crypto::Capability<Delegated>, TokenError> {
    let issuer = RealSigner::from_filler(1);
    let subject = RealSigner::from_filler(2);
    token_for(&issuer, payload).verify_canonical::<Ed25519, Delegated>(
        &issuer.actor_key(),
        &subject.actor_key(),
        100,
        PolicyEpoch::new(7),
    )
}

fn payload_with(actions: &[&str], workspace: &[u8], budget: u64) -> Vec<u8> {
    let mut writer = CborWriter::new();
    writer
        .array(9)
        .unsigned(0)
        .bytes(&[1; 32])
        .bytes(&[2; 32])
        .bytes(workspace)
        .text("delegated")
        .array(actions.len() as u64);
    for action in actions {
        writer.text(action);
    }
    writer.unsigned(7).unsigned(200).unsigned(budget);
    writer.finish()
}

#[test]
fn fixture_pins_the_exact_bytes_and_real_token_round_trip() {
    assert_eq!(fixture_string("schema"), "mesh-cbor/0;capability/0");
    let issuer = RealSigner::from_filler(1);
    let subject = RealSigner::from_filler(2);
    let fixed = fixture_parts();
    assert_eq!(
        fixed.actions(),
        &["author-change-set", "write-own-actor-state"]
    );
    assert_eq!(
        CapabilityToken::canonical_payload(&fixed),
        unhex(fixture_string("payload_hex"))
    );

    let payload = CapabilityToken::canonical_payload(&parts(&issuer, &subject));
    let capability = token_for(&issuer, payload)
        .verify_canonical::<Ed25519, Delegated>(
            &issuer.actor_key(),
            &subject.actor_key(),
            100,
            PolicyEpoch::new(7),
        )
        .expect("canonical token verifies");
    assert!(capability.grants(DelegatedAction::AuthorChangeSet));
    assert!(capability.grants(DelegatedAction::WriteOwnActorState));
}

#[test]
fn strict_decoder_refuses_every_noncanonical_or_ambiguous_shape() {
    let canonical = unhex(fixture_string("payload_hex"));
    let mut cases = Vec::new();

    let mut wrong_array_length = canonical.clone();
    wrong_array_length[0] = 0x88;
    cases.push((
        "wrong array length",
        wrong_array_length,
        CodecError::Malformed,
    ));

    let mut unsupported_version = canonical.clone();
    unsupported_version[1] = 1;
    cases.push((
        "unsupported version",
        unsupported_version,
        CodecError::UnsupportedVersion,
    ));

    let mut noncanonical_zero = vec![0x89, 0x18, 0x00];
    noncanonical_zero.extend_from_slice(&canonical[2..]);
    cases.push((
        "noncanonical integer",
        noncanonical_zero,
        CodecError::Malformed,
    ));

    cases.push((
        "wrong workspace length",
        payload_with(&["author-change-set", "write-own-actor-state"], &[3; 15], 0),
        CodecError::Malformed,
    ));
    cases.push((
        "unsorted actions",
        payload_with(&["write-own-actor-state", "author-change-set"], &[3; 16], 0),
        CodecError::Malformed,
    ));
    cases.push((
        "duplicate actions",
        payload_with(&["author-change-set", "author-change-set"], &[3; 16], 0),
        CodecError::Malformed,
    ));
    cases.push((
        "budget overflow",
        payload_with(
            &["author-change-set", "write-own-actor-state"],
            &[3; 16],
            256,
        ),
        CodecError::Malformed,
    ));
    let mut trailing = canonical.clone();
    trailing.push(0);
    cases.push(("trailing byte", trailing, CodecError::Malformed));

    for (name, payload, expected) in cases {
        assert_eq!(verify(payload), Err(TokenError::Codec(expected)), "{name}");
    }
}

#[test]
fn every_truncation_is_total_and_refused() {
    let canonical = unhex(fixture_string("payload_hex"));
    for cut in 0..canonical.len() {
        assert!(verify(canonical[..cut].to_vec()).is_err(), "cut {cut}");
    }
}
