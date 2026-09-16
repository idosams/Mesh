//! The `mesh-types` values this crate needs, mirrored rather than imported.
//!
//! # Why a mirror and not an import
//!
//! `mesh-sync-protocol` declares no dependency, not even a path dependency on `mesh-types`,
//! because any dependency edge rewrites `Cargo.lock` and this repository's declaration gate
//! refuses a lane that write. `mesh-store` hit the fence first and recorded it in
//! `docs/adr/0008-ship-the-sqlite-schema-without-a-sqlite-driver.md`; `mesh-state` hit it second
//! and chose to mirror under the *same* names so the day the edge is allowed the whole module is
//! replaced by a `use mesh_types::{…}` line and no call site changes. This is the third copy and
//! it follows `mesh-state` exactly, for that reason.
//!
//! A declaration nothing checks is a comment, so `tests/mesh_types_drift.rs` reads `mesh-types`'
//! own source and fails if any name mirrored here stops being declared there.

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
            Self::Length { expected, found } => {
                write!(
                    formatter,
                    "expected {expected} hex characters, found {found}"
                )
            }
            Self::NotHex => formatter.write_str("an identifier holds only hex characters"),
        }
    }
}

impl std::error::Error for IdError {}

/// How many hex characters a thirty-two-byte identifier is written as.
const HEX_CHARS: usize = 64;

macro_rules! record_id {
    ($name:ident, $what:literal) => {
        #[doc = concat!("The identifier of ", $what, ".")]
        ///
        /// A thirty-two-byte content digest, exactly as `mesh-types` carries it. Byte equality is
        /// identity and the ordering is `memcmp` over the bytes, which is what lets a knowledge
        /// set hold them in a sorted set and produce the same wire bytes on any machine.
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name([u8; 32]);

        impl $name {
            /// How many bytes this identifier occupies.
            pub const BYTE_WIDTH: usize = 32;

            /// Wrap bytes some other layer already derived.
            ///
            /// There is no constructor that *mints* one: a record identifier is the digest of the
            /// record it names, and a minted one would name nothing.
            #[must_use]
            pub const fn from_bytes(bytes: [u8; 32]) -> Self {
                Self(bytes)
            }

            /// The raw bytes.
            #[must_use]
            pub const fn as_bytes(&self) -> &[u8; 32] {
                &self.0
            }

            /// The sixty-four-character lowercase hex form.
            #[must_use]
            pub fn to_hex(self) -> String {
                let mut text = String::with_capacity(HEX_CHARS);
                for byte in self.0 {
                    text.push(hex_digit(byte >> 4));
                    text.push(hex_digit(byte & 0x0f));
                }
                text
            }

            /// Parse the hex form, in either case.
            ///
            /// # Errors
            ///
            /// [`IdError`] when the text is not exactly sixty-four hex characters.
            pub fn parse_hex(text: &str) -> Result<Self, IdError> {
                let bytes = text.as_bytes();
                if bytes.len() != HEX_CHARS {
                    return Err(IdError::Length {
                        expected: HEX_CHARS,
                        found: bytes.len(),
                    });
                }
                let mut out = [0u8; 32];
                for (slot, pair) in out.iter_mut().zip(bytes.chunks_exact(2)) {
                    let high = hex_value(pair[0]).ok_or(IdError::NotHex)?;
                    let low = hex_value(pair[1]).ok_or(IdError::NotHex)?;
                    *slot = (high << 4) | low;
                }
                Ok(Self(out))
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.to_hex())
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, concat!(stringify!($name), "({})"), self.to_hex())
            }
        }
    };
}

record_id!(
    ActorId,
    "an actor, derived from that actor's public key and from nothing else"
);
record_id!(ChangeSetId, "a ChangeSet");
record_id!(
    HeadId,
    "a workspace state some party treats as current — an actor head or the canonical head"
);
record_id!(ManifestId, "a file manifest");
record_id!(ContentHash, "a chunk, under which its bytes are verified");
record_id!(ReviewBundleId, "a review bundle");
record_id!(ApprovalId, "an approval envelope");

/// One lowercase hex character.
fn hex_digit(nibble: u8) -> char {
    char::from_digit(u32::from(nibble), 16).unwrap_or('0')
}

/// The value of one hex character, in either case.
fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// One actor's monotone per-actor counter, exactly as `mesh-types` carries it.
///
/// It orders one actor's own ChangeSets and nothing else: two actors' sequence numbers are never
/// compared, which is why this is a per-actor counter and not a clock. A knowledge set records the
/// highest sequence for which *every* preceding sequence is also held — see
/// [`crate::ActorKnowledge`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActorSequence(u64);

impl ActorSequence {
    /// The sequence before an actor's first ChangeSet: nothing is held.
    pub const NONE: Self = Self(0);

    /// The sequence numbered `value`, where one is an actor's first ChangeSet.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// The number.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// The next sequence, saturating at the maximum rather than wrapping to zero.
    ///
    /// Wrapping would make a peer that has held a session open for 2^64 ChangeSets re-advertise
    /// sequence one, which reads as a peer that holds nothing. Saturating makes it stop advancing,
    /// which reads as a peer that is stuck — the failure that is diagnosable.
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

impl fmt::Display for ActorSequence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// The version of the capability and validation policy a ChangeSet was authored under, exactly as
/// `mesh-types` carries it.
///
/// Carried on the wire so a peer can tell that a message was produced under a policy it has not
/// yet learned, rather than silently evaluating it under the wrong one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PolicyEpoch(u64);

impl PolicyEpoch {
    /// The epoch a workspace starts in.
    pub const INITIAL: Self = Self(0);

    /// The epoch numbered `value`.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// The number.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for PolicyEpoch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        let id = ChangeSetId::from_bytes([0xab; 32]);
        assert_eq!(id.to_hex(), "ab".repeat(32));
        assert_eq!(ChangeSetId::parse_hex(&id.to_hex()), Ok(id));
    }

    #[test]
    fn hex_parses_in_either_case() {
        assert_eq!(
            HeadId::parse_hex(&"0f".repeat(32)),
            HeadId::parse_hex(&"0F".repeat(32))
        );
    }

    #[test]
    fn a_short_identifier_is_refused_with_both_counts() {
        assert_eq!(
            ActorId::parse_hex("00"),
            Err(IdError::Length {
                expected: 64,
                found: 2
            })
        );
    }

    #[test]
    fn a_non_hex_character_is_refused() {
        let text = format!("{}zz", "0".repeat(62));
        assert_eq!(ContentHash::parse_hex(&text), Err(IdError::NotHex));
    }

    /// A knowledge set holds identifiers in sorted sets, and the wire bytes it produces are the
    /// sorted order. `memcmp` order is therefore load-bearing rather than an incidental derive.
    #[test]
    fn ordering_is_memcmp_over_the_bytes() {
        let mut low = [0u8; 32];
        low[0] = 0x01;
        let mut high = [0u8; 32];
        high[0] = 0x02;
        assert!(ChangeSetId::from_bytes(low) < ChangeSetId::from_bytes(high));

        let mut late = [0u8; 32];
        late[31] = 0x01;
        assert!(ChangeSetId::from_bytes([0u8; 32]) < ChangeSetId::from_bytes(late));
    }

    #[test]
    fn debug_names_the_type_and_the_digest() {
        let text = format!("{:?}", ManifestId::from_bytes([0u8; 32]));
        assert!(text.starts_with("ManifestId("), "{text}");
    }

    #[test]
    fn a_sequence_saturates_rather_than_wrapping() {
        assert_eq!(ActorSequence::new(u64::MAX).next().get(), u64::MAX);
        assert_eq!(ActorSequence::NONE.next(), ActorSequence::new(1));
    }

    #[test]
    fn an_epoch_starts_at_zero() {
        assert_eq!(PolicyEpoch::INITIAL.get(), 0);
        assert_eq!(PolicyEpoch::default(), PolicyEpoch::INITIAL);
    }
}
