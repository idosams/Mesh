//! UUIDs, and the version-7 discipline entity identifiers are held to.
//!
//! The crate mints UUIDs but never reads a clock and never draws entropy: [`Uuid::new_v7`] takes
//! the millisecond and the random bytes as arguments. That is not an inconvenience, it is the
//! point — a types crate with ambient authority over the clock and the entropy pool is a types
//! crate whose output cannot be reproduced in a test or a simulator, and plan §12.3 requires the
//! simulator to replace exactly those two things.

use core::fmt;

/// A 128-bit UUID in RFC 9562 layout.
///
/// This type is a container, not a promise: it will hold any 16 bytes, including a version the
/// protocol does not use. The promise lives one level up, in the entity identifiers, which only
/// accept a version 7 value.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Uuid([u8; 16]);

impl Uuid {
    /// Wrap 16 bytes exactly as given.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// The raw bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// Build a version-7 UUID: 48 bits of Unix milliseconds, then 74 bits of caller-supplied
    /// randomness with the version and variant fields overwritten.
    ///
    /// `unix_millis` is truncated to its low 48 bits, which is what the layout has room for and
    /// what every version-7 implementation does; the field wraps in the year 10889.
    #[must_use]
    pub const fn new_v7(unix_millis: u64, random: [u8; 10]) -> Self {
        let millis = unix_millis & 0x0000_FFFF_FFFF_FFFF;
        let mut bytes = [0u8; 16];
        bytes[0] = (millis >> 40) as u8;
        bytes[1] = (millis >> 32) as u8;
        bytes[2] = (millis >> 24) as u8;
        bytes[3] = (millis >> 16) as u8;
        bytes[4] = (millis >> 8) as u8;
        bytes[5] = millis as u8;
        bytes[6] = 0x70 | (random[0] & 0x0f);
        bytes[7] = random[1];
        bytes[8] = 0x80 | (random[2] & 0x3f);
        bytes[9] = random[3];
        bytes[10] = random[4];
        bytes[11] = random[5];
        bytes[12] = random[6];
        bytes[13] = random[7];
        bytes[14] = random[8];
        bytes[15] = random[9];
        Self(bytes)
    }

    /// The version nibble.
    #[must_use]
    pub const fn version(&self) -> u8 {
        self.0[6] >> 4
    }

    /// Whether the two top variant bits are the RFC 9562 variant.
    #[must_use]
    pub const fn is_rfc_variant(&self) -> bool {
        self.0[8] & 0xc0 == 0x80
    }

    /// The 48-bit millisecond field, meaningful only for a version-7 value.
    #[must_use]
    pub const fn unix_millis(&self) -> u64 {
        ((self.0[0] as u64) << 40)
            | ((self.0[1] as u64) << 32)
            | ((self.0[2] as u64) << 24)
            | ((self.0[3] as u64) << 16)
            | ((self.0[4] as u64) << 8)
            | (self.0[5] as u64)
    }

    /// The five hex groups of the 8-4-4-4-12 form, as half-open byte ranges into the
    /// 36-character text. Every group has an even length, which is what lets a two-character read
    /// inside one never cross a separator or run off the end.
    const GROUPS: [(usize, usize); 5] = [(0, 8), (9, 13), (14, 18), (19, 23), (24, 36)];

    /// Parse the hyphenated 36-character form, in either case.
    ///
    /// The walk follows the group structure rather than skipping separators wherever they turn
    /// up. Skipping was how this function used to work and it had two faults, both reachable from
    /// any surface that parses an identifier out of input somebody else wrote: a fifth hyphen at
    /// an even offset inside a group left a two-character read starting at index 35, which
    /// **panicked**; and a fifth hyphen anywhere else made the parse succeed with fewer than
    /// sixteen bytes written, returning an identifier padded with zeroes the caller never saw.
    /// Walking fixed ranges removes both by construction — see `tests/uuid_parse_is_total.rs`,
    /// which pins the six inputs that used to panic.
    ///
    /// # Errors
    ///
    /// [`UuidParseError`] when the length, the hyphen positions or the digits are wrong.
    pub fn parse(text: &str) -> Result<Self, UuidParseError> {
        let bytes = text.as_bytes();
        if bytes.len() != 36 {
            return Err(UuidParseError::Length { found: bytes.len() });
        }
        for position in [8, 13, 18, 23] {
            if bytes[position] != b'-' {
                return Err(UuidParseError::Layout);
            }
        }

        let mut out = [0u8; 16];
        let mut written = 0usize;
        for (start, end) in Self::GROUPS {
            let mut index = start;
            while index < end {
                let high = hex_value(bytes[index]).ok_or(UuidParseError::NotHex)?;
                let low = hex_value(bytes[index + 1]).ok_or(UuidParseError::NotHex)?;
                out[written] = (high << 4) | low;
                written += 1;
                index += 2;
            }
        }
        debug_assert_eq!(written, 16, "the group ranges cover exactly sixteen bytes");
        Ok(Self(out))
    }
}

const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

impl fmt::Display for Uuid {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, byte) in self.0.iter().enumerate() {
            if matches!(index, 4 | 6 | 8 | 10) {
                formatter.write_str("-")?;
            }
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Uuid {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Uuid({self})")
    }
}

/// Why a hyphenated UUID failed to parse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UuidParseError {
    /// The input was not 36 characters long.
    Length {
        /// How many characters were supplied.
        found: usize,
    },
    /// A hyphen was missing from one of the four fixed positions.
    Layout,
    /// A character outside the hex alphabet appeared.
    NotHex,
}

impl fmt::Display for UuidParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length { found } => {
                write!(formatter, "a UUID is 36 characters, found {found}")
            }
            Self::Layout => formatter.write_str("a UUID has hyphens at positions 8, 13, 18 and 23"),
            Self::NotHex => formatter.write_str("a UUID contains only hex characters and hyphens"),
        }
    }
}

impl std::error::Error for UuidParseError {}
