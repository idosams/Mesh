//! The identifier parsers are total: no string of any length or content makes one panic.
//!
//! A parser sits at a system boundary, so its input is written by somebody else. A panic there is
//! a denial of service on input the sender controls, and `mesh-types` is the crate every other
//! crate builds against — the reach is every surface that reads an identifier from a wire message,
//! a file or a command line.

use mesh_types::{Digest32, ObjectId, SessionId, Uuid, UuidParseError, WorkspaceId};

/// SplitMix64: the same generator the identity property tests use, so "random" means the same
/// corpus on every machine and in every run.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// The six inputs that panicked before the group-structured walk landed.
///
/// Each is exactly 36 characters with hyphens at 8, 13, 18 and 23 — so it passes the layout
/// check — plus a fifth hyphen at an even offset inside a hex group. The old walk skipped
/// separators wherever it met them, so the stray hyphen shifted every later read by one and the
/// final two-character read started at index 35, indexing 36 into a 36-byte slice.
const ONCE_PANICKED: [&str; 6] = [
    "01234567-89ab-cdef-0123--56789abcdef",
    "01234567-89ab-cdef-0123-45-789abcdef",
    "01234567-89ab-cdef-0123-4567-9abcdef",
    "01234567-89ab-cdef-0123-456789-bcdef",
    "01234567-89ab-cdef-0123-456789ab-def",
    "01234567-89ab-cdef-0123-456789abcd-f",
];

#[test]
fn the_inputs_that_once_panicked_are_now_refused() {
    for input in ONCE_PANICKED {
        assert_eq!(input.chars().count(), 36, "{input} is not 36 characters");
        assert_eq!(
            Uuid::parse(input),
            Err(UuidParseError::NotHex),
            "{input} must be refused, not parsed and not panicked on"
        );
        assert!(WorkspaceId::parse(input).is_err());
    }
}

/// A stray hyphen that does not reach the end used to make the parse *succeed*, writing fewer
/// than sixteen bytes and returning an identifier padded with zeroes. Every 36-character string
/// with a hyphen outside the four separator positions must be refused.
#[test]
fn a_stray_hyphen_is_refused_rather_than_parsed_short() {
    let hex: Vec<char> = "0123456789abcdef0123456789abcdef".chars().collect();
    for stray in 0..32usize {
        let mut mutated = hex.clone();
        mutated[stray] = '-';
        let text = format!(
            "{}-{}-{}-{}-{}",
            mutated[0..8].iter().collect::<String>(),
            mutated[8..12].iter().collect::<String>(),
            mutated[12..16].iter().collect::<String>(),
            mutated[16..20].iter().collect::<String>(),
            mutated[20..32].iter().collect::<String>(),
        );
        assert_eq!(text.chars().count(), 36);
        assert_eq!(
            Uuid::parse(&text),
            Err(UuidParseError::NotHex),
            "a hyphen at hex offset {stray} must be refused"
        );
    }
}

/// The well-formed case still works, and still round-trips, so the fix narrowed nothing it should
/// not have. A regression test with no positive twin proves only that the input is broken.
#[test]
fn a_well_formed_uuid_still_parses_in_either_case() {
    let lower = "0123456f-89ab-7def-8123-456789abcdef";
    let parsed = Uuid::parse(lower).expect("a well-formed version-7 UUID parses");
    assert_eq!(parsed.version(), 7);
    assert!(parsed.is_rfc_variant());
    assert_eq!(parsed.to_string(), lower);
    assert_eq!(
        Uuid::parse(&lower.to_uppercase()).expect("uppercase parses"),
        parsed
    );
    assert!(WorkspaceId::parse(lower).is_ok());
}

/// No string panics. The alphabet deliberately includes the hyphen, non-hex ASCII, a multi-byte
/// character and a brace, because a byte-indexing parser fed multi-byte text is the other way this
/// class of bug shows up.
#[test]
fn no_generated_string_makes_any_identifier_parser_panic() {
    let alphabet: Vec<char> = "0123456789abcdefABCDEF--gz {}\u{00e9}\u{4e2d}"
        .chars()
        .collect();
    let mut rng = SplitMix64(0x1234_5678_9ABC_DEF0);
    let mut refused = 0usize;

    for _ in 0..200_000 {
        let len = (rng.next() % 42) as usize;
        let text: String = (0..len)
            .map(|_| alphabet[(rng.next() as usize) % alphabet.len()])
            .collect();

        if Uuid::parse(&text).is_err() {
            refused += 1;
        }
        let _ = WorkspaceId::parse(&text);
        let _ = SessionId::parse(&text);
        let _ = ObjectId::parse(&text);
        let _ = Digest32::parse_hex(&text);
    }

    // Nothing here should have been accepted: a random draw over this alphabet producing a
    // well-formed UUID would mean the parser accepts far too much.
    assert_eq!(refused, 200_000, "a generated string parsed as a UUID");
}

/// Every 36-character string built only from hex characters and hyphens, with the four separators
/// in place, exercised exhaustively over where a fifth separator can sit. This is the shape that
/// slips past the layout check, which is why it gets its own pass rather than relying on the
/// random draw to find it.
#[test]
fn every_placement_of_a_fifth_separator_is_handled() {
    let hex: Vec<char> = "0123456789abcdef0123456789abcdef".chars().collect();
    for first in 0..32usize {
        for second in first..32usize {
            let mut mutated = hex.clone();
            mutated[first] = '-';
            mutated[second] = '-';
            let text = format!(
                "{}-{}-{}-{}-{}",
                mutated[0..8].iter().collect::<String>(),
                mutated[8..12].iter().collect::<String>(),
                mutated[12..16].iter().collect::<String>(),
                mutated[16..20].iter().collect::<String>(),
                mutated[20..32].iter().collect::<String>(),
            );
            assert!(
                Uuid::parse(&text).is_err(),
                "separators at hex offsets {first} and {second} must be refused"
            );
        }
    }
}
