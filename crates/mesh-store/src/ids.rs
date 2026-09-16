//! The two identifier shapes a stored row can hold.
//!
//! `mesh-types` splits identity in two — minted UUIDv7 for mutable entities, BLAKE3 digests for
//! immutable records — and keeps the two families as distinct Rust types with no conversion
//! between them. At the storage boundary that split survives, but it changes form: a SQLite column
//! has exactly one storage class, so the *type* narrows to two byte widths and the *role* moves
//! into schema metadata ([`crate::ColumnDomain`]).
//!
//! That narrowing is deliberate and it has a cost, stated rather than hidden: an
//! [`OperationRecord::actor`](crate::OperationRecord::actor) and an
//! [`OperationRecord::id`](crate::OperationRecord::id) are the same Rust type here, so nothing but
//! the field name stops them being swapped at a call site. What catches a swap is the schema —
//! every column declares its domain, and `tests/mesh_types_drift.rs` holds those declarations
//! against the identifiers `mesh-types` actually declares.

use core::fmt;

/// A 32-byte content digest, as one is stored: the storage form of every `mesh-types` record
/// identifier.
///
/// Byte equality is identity. The ordering is `memcmp` over the bytes, which is exactly how SQLite
/// orders a `BLOB` column, so a table sorted in memory and the same table sorted by `ORDER BY`
/// come out in the same order — the property the reconstruction digest rests on.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecordDigest([u8; 32]);

/// A 16-byte minted identifier, as one is stored: the storage form of every `mesh-types` entity
/// identifier.
///
/// Deliberately not the same type as [`RecordDigest`], and with no conversion between them, so
/// that the `mesh-types` identity split does not decay into "some blob" once it reaches a column.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityUuid([u8; 16]);

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

macro_rules! stored_id {
    ($name:ident, $width:literal, $chars:literal) => {
        impl $name {
            /// How many bytes this identifier occupies in a column.
            pub const BYTE_WIDTH: usize = $width;

            /// Wrap bytes some other layer already produced.
            #[must_use]
            pub const fn from_bytes(bytes: [u8; $width]) -> Self {
                Self(bytes)
            }

            /// The raw bytes, as they are written to a `BLOB` column.
            #[must_use]
            pub const fn as_bytes(&self) -> &[u8; $width] {
                &self.0
            }

            /// The lowercase hex form, which is what SQLite's `hex()` produces once lowercased.
            #[must_use]
            pub fn to_hex(self) -> String {
                let mut text = String::with_capacity($chars);
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
            /// [`IdError`] when the text is not exactly the right number of hex characters.
            pub fn parse_hex(text: &str) -> Result<Self, IdError> {
                let bytes = text.as_bytes();
                if bytes.len() != $chars {
                    return Err(IdError::Length {
                        expected: $chars,
                        found: bytes.len(),
                    });
                }
                let mut out = [0u8; $width];
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

stored_id!(RecordDigest, 32, 64);
stored_id!(EntityUuid, 16, 32);

const fn hex_digit(nibble: u8) -> char {
    (if nibble < 10 {
        b'0' + nibble
    } else {
        b'a' + nibble - 10
    }) as char
}

const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_digest_round_trips_through_hex() {
        let digest = RecordDigest::from_bytes([0xab; 32]);
        assert_eq!(digest.to_hex().len(), 64);
        assert_eq!(RecordDigest::parse_hex(&digest.to_hex()), Ok(digest));
    }

    #[test]
    fn an_entity_uuid_round_trips_through_hex() {
        let uuid = EntityUuid::from_bytes([0x01; 16]);
        assert_eq!(uuid.to_hex().len(), 32);
        assert_eq!(EntityUuid::parse_hex(&uuid.to_hex()), Ok(uuid));
    }

    #[test]
    fn uppercase_hex_parses_because_sqlite_emits_it() {
        let digest = RecordDigest::from_bytes([0xde; 32]);
        let upper = digest.to_hex().to_uppercase();
        assert_eq!(RecordDigest::parse_hex(&upper), Ok(digest));
    }

    /// The width check runs before any indexing, so a short or long input is an error and never a
    /// panic. `mesh-types` shipped a parser that panicked on a wrong-shaped input of the right
    /// length; the shape here is fixed-width with no separators, and this is what pins that.
    #[test]
    fn a_wrong_length_is_an_error_and_never_a_panic() {
        for text in ["", "ab", &"a".repeat(63), &"a".repeat(65), &"a".repeat(128)] {
            assert!(matches!(
                RecordDigest::parse_hex(text),
                Err(IdError::Length { expected: 64, .. })
            ));
        }
        for text in ["", &"a".repeat(31), &"a".repeat(33)] {
            assert!(matches!(
                EntityUuid::parse_hex(text),
                Err(IdError::Length { expected: 32, .. })
            ));
        }
    }

    #[test]
    fn a_non_hex_character_is_an_error() {
        let mut text = "a".repeat(64);
        text.replace_range(7..8, "z");
        assert_eq!(RecordDigest::parse_hex(&text), Err(IdError::NotHex));
    }

    /// Every byte value survives the round trip, so no digest is unrepresentable.
    #[test]
    fn every_byte_value_round_trips() {
        for value in 0u8..=255 {
            let digest = RecordDigest::from_bytes([value; 32]);
            assert_eq!(RecordDigest::parse_hex(&digest.to_hex()), Ok(digest));
        }
    }

    /// Ordering is `memcmp`, which is what SQLite does to a `BLOB`. The reconstruction digest
    /// compares an in-memory sort against a SQL `ORDER BY`, so this is load-bearing.
    #[test]
    fn ordering_is_memcmp_over_the_bytes() {
        let mut low = [0u8; 32];
        let mut high = [0u8; 32];
        low[0] = 0x00;
        high[0] = 0x01;
        assert!(RecordDigest::from_bytes(low) < RecordDigest::from_bytes(high));

        let mut a = [0u8; 32];
        let mut b = [0u8; 32];
        a[31] = 1;
        b[31] = 2;
        assert!(RecordDigest::from_bytes(a) < RecordDigest::from_bytes(b));
    }
}
