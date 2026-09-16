//! The digest a chunk is named by, and the seam that names the algorithm once.
//!
//! [`Digest32`], [`DigestHasher`], [`ContentDigest`] and [`Blake3`] mirror `mesh-types`' and
//! `mesh-cas`' types of the same names, for the reason given in [`crate::blake3`]: the value has to
//! be identical across the three crates, and a dependency edge that would make that structural is
//! not available. The *framing* half of `mesh-types`' digest module — `DigestWriter`,
//! `CanonicalRecord`, `DomainTag` — is deliberately not copied. Nothing here derives a record
//! identifier from fields; a chunk is named by the digest of its bytes, flat and unframed, which is
//! exactly what [`ContentDigest::digest_bytes`] is for.

use core::fmt;

use crate::blake3::{hash as blake3_hash, Blake3Hasher};

/// A 32-byte content-derived value: the name a chunk has in the store.
///
/// Displayed and parsed as 64 lowercase hex characters. Equality is byte equality.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Digest32([u8; 32]);

impl Digest32 {
    /// Wrap 32 bytes that a digest function already produced.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The raw bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Parse 64 lowercase or uppercase hex characters.
    ///
    /// # Errors
    ///
    /// [`DigestParseError`] when the input is not exactly 64 hex characters.
    pub fn parse_hex(text: &str) -> Result<Self, DigestParseError> {
        let bytes = text.as_bytes();
        if bytes.len() != 64 {
            return Err(DigestParseError::Length { found: bytes.len() });
        }
        let mut out = [0u8; 32];
        for (slot, pair) in out.iter_mut().zip(bytes.chunks_exact(2)) {
            let high = hex_value(pair[0]).ok_or(DigestParseError::NotHex)?;
            let low = hex_value(pair[1]).ok_or(DigestParseError::NotHex)?;
            *slot = (high << 4) | low;
        }
        Ok(Self(out))
    }

    /// The 64-character lowercase hex form.
    #[must_use]
    pub fn to_hex(self) -> String {
        let mut text = String::with_capacity(64);
        for byte in self.0 {
            text.push(hex_digit(byte >> 4));
            text.push(hex_digit(byte & 0x0f));
        }
        text
    }
}

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

impl fmt::Display for Digest32 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

impl fmt::Debug for Digest32 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Digest32({})", self.to_hex())
    }
}

/// Why a digest could not be parsed from text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DigestParseError {
    /// The text was not 64 characters long.
    Length {
        /// How many characters were found.
        found: usize,
    },
    /// The text held a character that is not a hex digit.
    NotHex,
}

impl fmt::Display for DigestParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length { found } => {
                write!(formatter, "a digest is 64 hex characters, found {found}")
            }
            Self::NotHex => formatter.write_str("a digest contains only hex characters"),
        }
    }
}

impl std::error::Error for DigestParseError {}

/// Incremental digest state: the half of the seam that absorbs bytes.
pub trait DigestHasher {
    /// Absorb more bytes.
    fn update(&mut self, bytes: &[u8]);

    /// Finish and produce the 32-byte value.
    fn finalize(self) -> Digest32;
}

/// A 32-byte content digest algorithm: the half of the seam that names one.
///
/// Every chunk name and every content hash in this crate is produced through `D: ContentDigest`, so
/// replacing BLAKE3 is a new type implementing this trait rather than an edit to a call site.
pub trait ContentDigest {
    /// The incremental state this algorithm uses.
    type Hasher: DigestHasher;

    /// A fresh hasher.
    fn hasher() -> Self::Hasher;

    /// The digest of a flat byte string, with no framing — which is what a chunk's name is.
    fn digest_bytes(input: &[u8]) -> Digest32 {
        let mut hasher = Self::hasher();
        hasher.update(input);
        hasher.finalize()
    }
}

/// BLAKE3, the default content digest, and the one `mesh-types` uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Blake3;

impl DigestHasher for Blake3Hasher {
    fn update(&mut self, bytes: &[u8]) {
        Blake3Hasher::update(self, bytes);
    }

    fn finalize(self) -> Digest32 {
        Digest32::from_bytes(Blake3Hasher::finalize(&self))
    }
}

impl ContentDigest for Blake3 {
    type Hasher = Blake3Hasher;

    fn hasher() -> Self::Hasher {
        Blake3Hasher::new()
    }

    fn digest_bytes(input: &[u8]) -> Digest32 {
        Digest32::from_bytes(blake3_hash(input))
    }
}
