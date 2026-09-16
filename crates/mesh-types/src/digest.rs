//! Content digests and the framing that turns a record into the bytes it is named by.
//!
//! Three things live here and they are deliberately separate:
//!
//! * [`Digest32`] — an opaque 32-byte content-derived value.
//! * [`DigestHasher`] / [`ContentDigest`] — the seam. `ContentDigest` names an algorithm,
//!   `DigestHasher` is its incremental state. [`Blake3`] is the only implementation today; a
//!   second one is a new type, not an edit to any call site.
//! * [`DigestWriter`] and [`CanonicalRecord`] — the *framing*, which is not a serialization
//!   format. A record absorbs its fields in a fixed order, each length-prefixed and preceded by a
//!   domain tag, so no two records can produce the same byte stream by accident.
//!
//! # The framing is retired, and this module is what is waiting to change
//!
//! That last bullet used to end by saying the interchange encoding was a different thing owned by a
//! different task, and that re-specifying record-ID derivation on top of it when it landed would be
//! a compatibility event rather than a refactor. Both halves have now happened. [`crate::canonical`]
//! is the encoding, and
//! `docs/adr/0033-name-an-immutable-record-by-the-digest-of-its-canonical-encoding.md` ruled that a
//! record's name is [`crate::canonical_digest`] — BLAKE3 of that encoding — retiring this framing
//! rather than publishing it, because no published document ever described it and an external
//! implementer therefore could not name a record at all.
//!
//! [`derive_id`] below still computes the retired value. Moving it onto the encoding, deleting
//! [`DigestWriter`] and [`Absorb`], and bringing an actor key under the same rule is
//! `01KZFMZC4MTHTT3BW4Y0BW6NYA` — kept separate because plan §14.3 rule 3 forbids one unsupervised
//! run from moving a signed record's definition and its implementation together. Nothing new should
//! be built on this framing.

use core::fmt;

use crate::blake3::{hash as blake3_hash, Blake3Hasher};

/// A 32-byte content-derived value: the output of a [`ContentDigest`] over framed record bytes.
///
/// Displayed and parsed as 64 lowercase hex characters. Equality is byte equality — this type
/// carries no notion of what was hashed, which is what lets the same 32 bytes name a version in
/// one context and a manifest in another without either type being convertible into the other.
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

/// Why a hex digest failed to parse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DigestParseError {
    /// The input was not 64 characters long.
    Length {
        /// How many characters were supplied.
        found: usize,
    },
    /// The input contained a character that is not a hex digit.
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
/// Replacing the implementation is a new type implementing this trait — no call site in this crate
/// or any downstream crate mentions BLAKE3 by name, they mention `D: ContentDigest`.
pub trait ContentDigest {
    /// The incremental state this algorithm uses.
    type Hasher: DigestHasher;

    /// A fresh hasher.
    fn hasher() -> Self::Hasher;

    /// The digest of a flat byte string, with no framing. Used for content bytes that are already
    /// unambiguous — file content, a public key — never for a multi-field record.
    fn digest_bytes(input: &[u8]) -> Digest32 {
        let mut hasher = Self::hasher();
        hasher.update(input);
        hasher.finalize()
    }
}

/// BLAKE3, the default content digest.
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

/// The domain a record's identifier is derived in.
///
/// Two records with identical field bytes but different domains get different identifiers, so a
/// file version can never accidentally share an identifier with a directory version. The tag is
/// absorbed first and length-prefixed like any other field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DomainTag(&'static str);

impl DomainTag {
    /// Declare a domain tag. Tags are versioned (`mesh.v0.…`) because changing what a domain
    /// covers changes every identifier in it, which is a protocol change and not an edit.
    #[must_use]
    pub const fn new(tag: &'static str) -> Self {
        Self(tag)
    }

    /// The tag text.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        self.0
    }
}

impl fmt::Display for DomainTag {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

/// Unambiguous framing over any [`DigestHasher`].
///
/// Every variable-length field is preceded by its length as eight big-endian bytes, so no two
/// distinct field sequences can produce the same stream. Fixed-width fields are written bare
/// because their width is part of the record's shape, not of its data.
pub struct DigestWriter<H: DigestHasher> {
    hasher: H,
}

impl<H: DigestHasher> DigestWriter<H> {
    /// Start a writer in `domain`.
    pub fn new(domain: DomainTag, hasher: H) -> Self {
        let mut writer = Self { hasher };
        writer.bytes(domain.as_str().as_bytes());
        writer
    }

    /// A variable-length field: its length, then its bytes.
    pub fn bytes(&mut self, value: &[u8]) -> &mut Self {
        self.hasher.update(&(value.len() as u64).to_be_bytes());
        self.hasher.update(value);
        self
    }

    /// A string field, framed exactly like a byte field.
    pub fn text(&mut self, value: &str) -> &mut Self {
        self.bytes(value.as_bytes())
    }

    /// A fixed-width unsigned field, eight big-endian bytes.
    pub fn u64(&mut self, value: u64) -> &mut Self {
        self.hasher.update(&value.to_be_bytes());
        self
    }

    /// A boolean field, one byte.
    pub fn bool(&mut self, value: bool) -> &mut Self {
        self.hasher.update(&[u8::from(value)]);
        self
    }

    /// A fixed-width digest field, 32 bytes.
    pub fn digest(&mut self, value: &Digest32) -> &mut Self {
        self.hasher.update(value.as_bytes());
        self
    }

    /// A sequence: its element count, then each element in order.
    pub fn sequence<T>(&mut self, items: &[T], mut each: impl FnMut(&mut Self, &T)) -> &mut Self {
        self.u64(items.len() as u64);
        for item in items {
            each(self, item);
        }
        self
    }

    /// An optional field: a discriminant byte, then the value when present.
    pub fn option<T>(&mut self, value: Option<&T>, each: impl FnOnce(&mut Self, &T)) -> &mut Self {
        match value {
            None => self.bool(false),
            Some(inner) => {
                self.bool(true);
                each(self, inner);
                self
            }
        }
    }

    /// Finish and produce the record's digest.
    #[must_use]
    pub fn finish(self) -> Digest32 {
        self.hasher.finalize()
    }
}

impl<H: DigestHasher> fmt::Debug for DigestWriter<H> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DigestWriter(..)")
    }
}

/// Anything that contributes its fields to a digest in a fixed order.
///
/// Split out from [`CanonicalRecord`] because a ChangeSet's operations are absorbed into the
/// ChangeSet's own identifier without being separately identified records themselves — the
/// operation vocabulary is `mesh-operations`' to define, and this is the only thing this crate
/// needs to know about it.
pub trait Absorb {
    /// Absorb every field, in a fixed order.
    fn absorb<H: DigestHasher>(&self, writer: &mut DigestWriter<H>);
}

/// An immutable record whose identity is a digest of its own content.
///
/// Implementing this is what makes a record self-verifying: anyone holding the record can
/// recompute its identifier and compare. Mutable entities do not implement it — their identifiers
/// are minted, not derived, and this trait is the only derivation path in the crate.
pub trait CanonicalRecord: Absorb {
    /// The identifier type this record is named by.
    type Id: From<Digest32>;

    /// The domain this record's identifiers are derived in. Unique per record type.
    const DOMAIN: DomainTag;
}

/// Derive a canonical record's identifier under `D`.
///
/// ```
/// use mesh_types::{derive_id, Blake3, FileManifest, ChunkRef, Digest32};
///
/// let manifest = FileManifest::new(
///     11,
///     Digest32::from_bytes([7; 32]),
///     vec![ChunkRef::new(Digest32::from_bytes([9; 32]), 0, 11)],
/// );
///
/// // Same content in a different process, on a different machine: same identifier.
/// assert_eq!(derive_id::<Blake3, _>(&manifest), derive_id::<Blake3, _>(&manifest));
/// ```
#[must_use]
pub fn derive_id<D: ContentDigest, R: CanonicalRecord>(record: &R) -> R::Id {
    let mut writer = DigestWriter::new(R::DOMAIN, D::hasher());
    record.absorb(&mut writer);
    R::Id::from(writer.finish())
}
