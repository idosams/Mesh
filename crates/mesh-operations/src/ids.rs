//! The `mesh-types` identifiers this crate needs, mirrored rather than imported.
//!
//! # Why a mirror and not an import
//!
//! `mesh-operations` declares no dependency, not even a path dependency on `mesh-types`, because
//! any dependency edge rewrites `Cargo.lock` — measured, not assumed: adding
//! `mesh-types = { path = "../mesh-types" }` to this crate's manifest appends a three-line
//! `dependencies` block to `Cargo.lock`'s `mesh-operations` package, and
//! `tools/program/contract/declaration-gate.mjs` reports `forbidden-path-write` for that file
//! whatever the PR body says (decision `01KZCFACPMNDXEMZJR67S6KNT0` correction D).
//! `mesh-store` hit the fence first and `mesh-state` second;
//! `docs/adr/0008-ship-the-sqlite-schema-without-a-sqlite-driver.md` records the reasoning.
//!
//! The mirror is deliberately built to be **deleted**. Each type here carries the same name, the
//! same width and the same byte-equality semantics as its `mesh-types` counterpart, so the day the
//! edge is allowed the whole module becomes a `use mesh_types::{…};` line and no call site in this
//! crate changes.
//!
//! A declaration nothing checks is a comment, so `tests/mesh_types_drift.rs` reads `mesh-types`'
//! own source and fails if any name mirrored here stops being declared there, or if a *new*
//! identifier appears there that this crate has neither mirrored nor written down a reason for.
//!
//! # Two families, and the split is load-bearing
//!
//! Plan §4.2: minted UUIDv7 for mutable entity identity, BLAKE3-derived identifiers for immutable
//! canonical records. The two families are different types with no conversion between them here
//! either — [`entity_id!`] and [`record_id!`] generate disjoint APIs, and neither offers a
//! constructor taking the other's width.
//!
//! **This crate mints nothing.** It reads no clock and draws no entropy: every identifier arrives
//! from a caller or off the wire. `from_bytes` exists because a decoder needs it, not because an
//! operation may invent an identity.

use core::fmt;

/// Why a hex identifier failed to parse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdError {
    /// The text was not the expected number of hex characters.
    Length {
        /// How many characters were required.
        expected: usize,
        /// How many characters were supplied.
        found: usize,
    },
    /// The text held a character outside the hex alphabet.
    NotHex,
}

impl fmt::Display for IdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length { expected, found } => write!(
                formatter,
                "expected {expected} hex characters, found {found}"
            ),
            Self::NotHex => formatter.write_str("an identifier holds only hex characters"),
        }
    }
}

impl std::error::Error for IdError {}

/// Parse `text` as exactly `N` bytes of lowercase or uppercase hex.
fn parse_hex<const N: usize>(text: &str) -> Result<[u8; N], IdError> {
    let characters = text.as_bytes();
    if characters.len() != N * 2 {
        return Err(IdError::Length {
            expected: N * 2,
            found: characters.len(),
        });
    }
    let mut bytes = [0u8; N];
    for (index, byte) in bytes.iter_mut().enumerate() {
        let high = hex_value(characters[index * 2])?;
        let low = hex_value(characters[index * 2 + 1])?;
        *byte = (high << 4) | low;
    }
    Ok(bytes)
}

fn hex_value(character: u8) -> Result<u8, IdError> {
    match character {
        b'0'..=b'9' => Ok(character - b'0'),
        b'a'..=b'f' => Ok(character - b'a' + 10),
        b'A'..=b'F' => Ok(character - b'A' + 10),
        _ => Err(IdError::NotHex),
    }
}

fn write_hex(bytes: &[u8], formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    for byte in bytes {
        write!(formatter, "{byte:02x}")?;
    }
    Ok(())
}

macro_rules! fixed_width_id {
    ($name:ident, $width:literal, $what:literal, $family:literal) => {
        #[doc = concat!("The identifier of ", $what, ".")]
        ///
        #[doc = concat!("A ", stringify!($width), "-byte ", $family, ", exactly as `mesh-types`")]
        /// carries it. Byte equality is identity and the ordering is `memcmp` over the bytes, so
        /// the `lamport → event ULID → content hash` tiebreak this workspace uses can reach a
        /// total order without consulting a clock.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name([u8; $width]);

        impl $name {
            /// How many bytes this identifier occupies on the wire.
            pub const WIDTH: usize = $width;

            #[doc = concat!("The identifier these bytes are.")]
            ///
            /// This crate never mints an identifier; it receives one. A decoder needs this and an
            /// operation constructor needs it, and neither is a source of identity.
            #[must_use]
            pub const fn from_bytes(bytes: [u8; $width]) -> Self {
                Self(bytes)
            }

            /// The bytes.
            #[must_use]
            pub const fn as_bytes(&self) -> &[u8; $width] {
                &self.0
            }

            /// Parse the hex form.
            ///
            /// # Errors
            ///
            /// [`IdError`] when the text is the wrong length or holds a non-hex character.
            pub fn parse(text: &str) -> Result<Self, IdError> {
                parse_hex::<$width>(text).map(Self)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write_hex(&self.0, formatter)
            }
        }
    };
}

// The minted family: plan §4.2's UUIDv7 entity identifiers, sixteen bytes.
fixed_width_id!(WorkspaceId, 16, "a workspace", "minted entity identifier");
fixed_width_id!(
    SessionId,
    16,
    "an activity session",
    "minted entity identifier"
);
fixed_width_id!(ObjectId, 16, "a stable object", "minted entity identifier");

// The derived family: plan §4.2's content-derived record identifiers, thirty-two bytes.
fixed_width_id!(ActorId, 32, "an actor", "content digest");
fixed_width_id!(ChangeSetId, 32, "a ChangeSet", "content digest");
fixed_width_id!(HeadId, 32, "an actor head", "content digest");
fixed_width_id!(VersionId, 32, "an object version", "content digest");
fixed_width_id!(ManifestId, 32, "a file manifest", "content digest");
fixed_width_id!(ReviewBundleId, 32, "a review bundle", "content digest");
fixed_width_id!(ApprovalId, 32, "an approval envelope", "content digest");
fixed_width_id!(ContentHash, 32, "a chunk of content", "content digest");
fixed_width_id!(
    DerivationId,
    32,
    "a derived computation node",
    "content digest"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_families_carry_their_own_widths() {
        assert_eq!(ObjectId::WIDTH, 16);
        assert_eq!(WorkspaceId::WIDTH, 16);
        assert_eq!(SessionId::WIDTH, 16);
        assert_eq!(ChangeSetId::WIDTH, 32);
        assert_eq!(HeadId::WIDTH, 32);
    }

    #[test]
    fn hex_round_trips_in_both_directions() {
        let id = ChangeSetId::from_bytes([0xab; 32]);
        assert_eq!(ChangeSetId::parse(&id.to_string()).unwrap(), id);
        assert_eq!(id.to_string(), "ab".repeat(32));
        let object = ObjectId::from_bytes([0x01; 16]);
        assert_eq!(ObjectId::parse(&object.to_string()).unwrap(), object);
    }

    #[test]
    fn uppercase_hex_parses_and_lowercase_is_emitted() {
        let upper = "AB".repeat(32);
        assert_eq!(
            ChangeSetId::parse(&upper).unwrap().to_string(),
            "ab".repeat(32)
        );
    }

    #[test]
    fn a_wrong_length_or_a_non_hex_character_is_refused() {
        assert_eq!(
            ChangeSetId::parse("ab"),
            Err(IdError::Length {
                expected: 64,
                found: 2
            })
        );
        assert_eq!(ChangeSetId::parse(&"zz".repeat(32)), Err(IdError::NotHex));
        // A sixteen-byte identifier does not accept a thirty-two-byte hex string.
        assert!(ObjectId::parse(&"ab".repeat(32)).is_err());
    }

    #[test]
    fn ordering_is_memcmp_over_the_bytes() {
        let mut low = [0u8; 32];
        low[0] = 0x01;
        let mut high = [0u8; 32];
        high[0] = 0x02;
        assert!(ChangeSetId::from_bytes(low) < ChangeSetId::from_bytes(high));
        // The comparison is unsigned: 0x80 is above 0x7f, not below it.
        let mut top = [0u8; 32];
        top[0] = 0x80;
        assert!(ChangeSetId::from_bytes(high) < ChangeSetId::from_bytes(top));
    }
}
