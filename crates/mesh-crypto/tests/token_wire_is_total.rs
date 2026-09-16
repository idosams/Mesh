//! Token parsing is total: no byte string of any length or content makes it panic, and nothing it
//! accepts re-serializes to different bytes.
//!
//! This is the fuzz target the task contract asks for, written as a deterministic campaign rather
//! than as a `cargo-fuzz` harness. `cargo-fuzz` needs `libfuzzer-sys` in `[dev-dependencies]`,
//! which rewrites `Cargo.lock` — the same fence as the Ed25519 primitive itself, escalated in
//! `docs/adr/0009-escalate-the-lockfile-fence-for-an-audited-ed25519-rather-than-hand-roll-one.md`. The
//! campaign below is seeded, so a failure names an exact input a lane can replay, and `mesh-types`
//! set the precedent in `tests/uuid_parse_is_total.rs`.
//!
//! Panicking here is not a cosmetic defect. Token bytes arrive from a peer, so a panic on a
//! malformed one is a denial of service any peer can trigger, and an out-of-bounds index on a
//! truncated length prefix is the classic way to get one.

use mesh_types::Signature;

use mesh_crypto::{
    CapabilityToken, MAX_PAYLOAD_BYTES, SIGNATURE_BYTES, TOKEN_MAGIC, TOKEN_VERSION,
};

/// xorshift64*. Seeded, so every failure is replayable and no run depends on a clock.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn byte(&mut self) -> u8 {
        (self.next() >> 33) as u8
    }

    fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            0
        } else {
            (self.next() >> 1) as usize % bound
        }
    }
}

fn valid_token(payload: Vec<u8>) -> Vec<u8> {
    CapabilityToken::new(Signature::from_bytes([0xa5; SIGNATURE_BYTES]), payload)
        .expect("payload within bounds")
        .to_wire()
}

/// Anything parsed must re-serialize to exactly the bytes it was parsed from. A parser that
/// normalizes silently lets two peers disagree about what they received.
fn assert_round_trip(input: &[u8]) {
    if let Ok(token) = CapabilityToken::from_wire(input) {
        assert_eq!(
            token.to_wire(),
            input,
            "a token parsed and re-serialized to different bytes"
        );
    }
}

#[test]
fn no_random_byte_string_makes_the_parser_panic() {
    let mut rng = Rng(0x5eed_1234_abcd_0001);
    for _ in 0..20_000 {
        let length = rng.below(200);
        let input: Vec<u8> = (0..length).map(|_| rng.byte()).collect();
        assert_round_trip(&input);
    }
}

/// Random bytes almost never look like a token. Mutating a valid one is where the interesting
/// inputs are: a length field that overruns, a magic byte flipped, a payload cut in half.
#[test]
fn no_mutation_of_a_valid_token_makes_the_parser_panic() {
    let mut rng = Rng(0x5eed_1234_abcd_0002);
    for _ in 0..20_000 {
        let payload_length = rng.below(48);
        let payload: Vec<u8> = (0..payload_length).map(|_| rng.byte()).collect();
        let mut wire = valid_token(payload);

        match rng.below(4) {
            0 => {
                let at = rng.below(wire.len());
                wire[at] ^= 1 << rng.below(8);
            }
            1 => {
                let at = rng.below(wire.len().max(1));
                wire.truncate(at);
            }
            2 => wire.extend((0..rng.below(8)).map(|_| rng.byte())),
            _ => {
                let at = rng.below(wire.len());
                wire[at] = rng.byte();
            }
        }
        assert_round_trip(&wire);
    }
}

/// The length prefix is the field an attacker controls most directly. Every value of it, including
/// the ones far past the buffer, must be a refusal rather than an allocation or an index.
#[test]
fn every_declared_length_is_refused_or_matched_exactly() {
    let base = valid_token(b"payload".to_vec());
    let length_at = TOKEN_MAGIC.len() + 1 + SIGNATURE_BYTES;
    for declared in [
        0u32,
        1,
        6,
        7,
        8,
        MAX_PAYLOAD_BYTES as u32,
        MAX_PAYLOAD_BYTES as u32 + 1,
        u32::MAX / 2,
        u32::MAX,
    ] {
        let mut wire = base.clone();
        wire[length_at..length_at + 4].copy_from_slice(&declared.to_be_bytes());
        match CapabilityToken::from_wire(&wire) {
            Ok(token) => {
                assert_eq!(declared as usize, 7, "only the true length may parse");
                assert_eq!(token.to_wire(), wire);
            }
            Err(_) => assert_ne!(declared as usize, 7),
        }
    }
}

#[test]
fn a_payload_at_the_bound_is_accepted_and_one_past_it_is_not() {
    let at_bound = vec![0u8; MAX_PAYLOAD_BYTES];
    let token = CapabilityToken::new(Signature::from_bytes([0; SIGNATURE_BYTES]), at_bound)
        .expect("exactly at the bound");
    assert_round_trip(&token.to_wire());

    let past_bound = vec![0u8; MAX_PAYLOAD_BYTES + 1];
    assert!(CapabilityToken::new(Signature::from_bytes([0; SIGNATURE_BYTES]), past_bound).is_err());
}

#[test]
fn the_version_byte_is_checked_rather_than_ignored() {
    let base = valid_token(b"x".to_vec());
    for version in 0u8..=255 {
        let mut wire = base.clone();
        wire[TOKEN_MAGIC.len()] = version;
        let parsed = CapabilityToken::from_wire(&wire);
        assert_eq!(
            parsed.is_ok(),
            version == TOKEN_VERSION,
            "version {version} parsed as {parsed:?}"
        );
    }
}
