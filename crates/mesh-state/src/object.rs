//! Object identity — the fact a rename and a move must not change.
//!
//! `src/ids.rs` states why this crate mirrors `mesh-types` rather than importing it. [`ObjectId`]
//! is mirrored from the other of plan §4.2's two identifier families: a **minted** UUIDv7 naming a
//! mutable entity, sixteen bytes wide, as opposed to the thirty-two-byte content digests
//! `record_id!` generates. The two families are separate types here for the same reason they are
//! separate there — an object outlives every version of itself, so an object cannot be named by a
//! digest of its content.
//!
//! The version validation `mesh-types` performs is deliberately *not* mirrored. Doing so would
//! mean mirroring its `Uuid` as well, and this crate never mints an identifier: every [`ObjectId`]
//! reaching the register arrives from a caller or off the wire. What matters here is the width,
//! byte equality and `memcmp` ordering, and `tests/mesh_types_drift.rs` holds the name against
//! `mesh-types`' own source.

use core::fmt;

use crate::ids::IdError;

/// How many hex characters a sixteen-byte identifier is written as.
const HEX_CHARS: usize = 32;

/// The identifier of a stable object — a file or directory identity independent of any path.
///
/// The whole point of this type is what it is *not* derived from. It is not derived from a path,
/// from a name, from a parent directory or from content, so no rename, move or edit can produce a
/// different one. `docs/protocol.md` SG-6 states the rule; [`crate::ObjectRegister`] is where it
/// is made true.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId([u8; 16]);

impl ObjectId {
    /// How many bytes this identifier occupies.
    pub const BYTE_WIDTH: usize = 16;

    /// Wrap bytes some other layer already minted.
    ///
    /// There is no constructor that mints one. Minting reads a clock and draws entropy, and
    /// `src/no_ambient_input.rs` refuses this crate both.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// The raw bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// The thirty-two-character lowercase hex form.
    #[must_use]
    pub fn to_hex(self) -> String {
        let mut text = String::with_capacity(HEX_CHARS);
        for byte in self.0 {
            text.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
            text.push(char::from_digit(u32::from(byte & 0x0f), 16).unwrap_or('0'));
        }
        text
    }

    /// Parse the hex form, in either case.
    ///
    /// # Errors
    ///
    /// [`IdError`] when the text is not exactly thirty-two hex characters.
    pub fn parse_hex(text: &str) -> Result<Self, IdError> {
        let characters = text.as_bytes();
        if characters.len() != HEX_CHARS {
            return Err(IdError::Length {
                expected: HEX_CHARS,
                found: characters.len(),
            });
        }
        let mut bytes = [0u8; 16];
        for (slot, pair) in bytes.iter_mut().zip(characters.chunks_exact(2)) {
            let high = hex_value(pair[0]).ok_or(IdError::NotHex)?;
            let low = hex_value(pair[1]).ok_or(IdError::NotHex)?;
            *slot = (high << 4) | low;
        }
        Ok(Self(bytes))
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

impl fmt::Debug for ObjectId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "ObjectId({})", self.to_hex())
    }
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

/// What an object is.
///
/// Only a directory holds directory entries, which is why the register can refuse to link a child
/// into a file rather than silently producing a shape no materializer can render.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ObjectKind {
    /// A file.
    File,
    /// A directory.
    Directory,
    /// A symbolic link.
    Symlink,
}

impl ObjectKind {
    /// Every kind, in `mesh-types`' order. The index is the wire value.
    pub const ALL: [Self; 3] = [Self::File, Self::Directory, Self::Symlink];

    /// The wire spelling `mesh-types` gives this kind.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Directory => "directory",
            Self::Symlink => "symlink",
        }
    }

    /// Whether this kind can hold directory entries.
    #[must_use]
    pub const fn holds_entries(&self) -> bool {
        matches!(self, Self::Directory)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        let id = ObjectId::from_bytes([0xab; 16]);
        assert_eq!(id.to_hex(), "ab".repeat(16));
        assert_eq!(ObjectId::parse_hex(&id.to_hex()), Ok(id));
    }

    #[test]
    fn hex_parses_in_either_case() {
        assert_eq!(
            ObjectId::parse_hex(&"0f".repeat(16)),
            ObjectId::parse_hex(&"0F".repeat(16))
        );
    }

    #[test]
    fn a_thirty_two_byte_identifier_is_refused_as_an_object() {
        // The two identifier families have different widths on purpose, and a record digest
        // handed in where an object identity belongs is caught rather than truncated.
        assert_eq!(
            ObjectId::parse_hex(&"00".repeat(32)),
            Err(IdError::Length {
                expected: 32,
                found: 64
            })
        );
    }

    #[test]
    fn a_non_hex_character_is_refused() {
        let text = format!("{}zz", "0".repeat(30));
        assert_eq!(ObjectId::parse_hex(&text), Err(IdError::NotHex));
    }

    #[test]
    fn ordering_is_memcmp_over_the_bytes() {
        let mut late = [0u8; 16];
        late[15] = 0x01;
        assert!(ObjectId::from_bytes([0u8; 16]) < ObjectId::from_bytes(late));
    }

    #[test]
    fn only_a_directory_holds_entries() {
        assert!(ObjectKind::Directory.holds_entries());
        assert!(!ObjectKind::File.holds_entries());
        assert!(!ObjectKind::Symlink.holds_entries());
        assert_eq!(ObjectKind::ALL.len(), 3);
    }

    #[test]
    fn debug_names_the_type_and_the_identifier() {
        let text = format!("{:?}", ObjectId::from_bytes([0u8; 16]));
        assert!(text.starts_with("ObjectId("), "{text}");
    }
}
