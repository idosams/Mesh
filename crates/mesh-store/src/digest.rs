//! The index digest: how "rebuild every table and match a digest" is checked.
//!
//! # This is a drift detector, not a security primitive, and the difference is the point
//!
//! The digest here answers one question: did two independently-produced copies of this index come
//! out identical? Its adversary is a bug — a fold that drops a row, a `ORDER BY` that disagrees
//! with an in-memory sort, a migration that loses a column. Its adversary is *not* somebody
//! choosing bytes to collide with, because nothing here is signed, replicated or trusted: plan
//! §6.1's rule is that this database is an index and never the source of truth, so a forged index
//! is repaired by throwing it away and rebuilding from records that *are* signed.
//!
//! [`Fnv1a128`] is therefore what the default implementation is, and it is named rather than
//! disguised. `mesh-types` already carries a BLAKE3 the moment this crate is allowed a dependency
//! edge to it — see `docs/adr/0008-ship-the-sqlite-schema-without-a-sqlite-driver.md` for why it
//! is not allowed one today — and swapping is one type, because everything below is written
//! against [`IndexDigest`] and no call site names FNV.

use core::fmt;

/// A 128-bit index digest value.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Digest16([u8; 16]);

impl Digest16 {
    /// Wrap sixteen bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// The raw bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// The 32-character lowercase hex form.
    #[must_use]
    pub fn to_hex(self) -> String {
        let mut text = String::with_capacity(32);
        for byte in self.0 {
            text.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
            text.push(char::from_digit(u32::from(byte & 0x0f), 16).unwrap_or('0'));
        }
        text
    }
}

impl fmt::Display for Digest16 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

impl fmt::Debug for Digest16 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Digest16({})", self.to_hex())
    }
}

/// The digest seam: an algorithm that absorbs bytes and produces a [`Digest16`].
///
/// Two implementations would agree on nothing but the framing, which is deliberate — the framing
/// is this crate's, the algorithm is the implementation's.
pub trait IndexDigest {
    /// A fresh accumulator.
    fn start() -> Self;
    /// Absorb bytes.
    fn absorb(&mut self, bytes: &[u8]);
    /// Finish.
    fn finish(self) -> Digest16;
}

/// FNV-1a over 128 bits: the default [`IndexDigest`].
///
/// Chosen because it is a few lines, has no dependency, and is entirely sufficient for detecting
/// an accidental difference between two copies of an index. It is not collision-resistant against
/// a chosen input and is not used where that would matter.
#[derive(Clone, Copy, Debug)]
pub struct Fnv1a128(u128);

impl Fnv1a128 {
    const OFFSET_BASIS: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;
}

impl IndexDigest for Fnv1a128 {
    fn start() -> Self {
        Self(Self::OFFSET_BASIS)
    }

    fn absorb(&mut self, bytes: &[u8]) {
        let mut state = self.0;
        for byte in bytes {
            state ^= u128::from(*byte);
            state = state.wrapping_mul(Self::PRIME);
        }
        self.0 = state;
    }

    fn finish(self) -> Digest16 {
        Digest16::from_bytes(self.0.to_be_bytes())
    }
}

/// Unambiguous framing over any [`IndexDigest`].
///
/// Every variable-length value is preceded by its length, so no two distinct sequences of values
/// can produce one byte stream. Without that, a row of `[b"ab", b"c"]` and a row of `[b"a",
/// b"bc"]` would digest identically and the reconstruction check would pass over a real
/// difference.
pub(crate) struct DigestWriter<D: IndexDigest> {
    inner: D,
}

impl<D: IndexDigest> DigestWriter<D> {
    pub(crate) fn new() -> Self {
        Self { inner: D::start() }
    }

    pub(crate) fn bytes(&mut self, value: &[u8]) -> &mut Self {
        self.inner.absorb(&(value.len() as u64).to_be_bytes());
        self.inner.absorb(value);
        self
    }

    pub(crate) fn text(&mut self, value: &str) -> &mut Self {
        self.bytes(value.as_bytes())
    }

    pub(crate) fn integer(&mut self, value: i64) -> &mut Self {
        self.inner.absorb(&value.to_be_bytes());
        self
    }

    pub(crate) fn count(&mut self, value: usize) -> &mut Self {
        self.inner.absorb(&(value as u64).to_be_bytes());
        self
    }

    pub(crate) fn tag(&mut self, value: u8) -> &mut Self {
        self.inner.absorb(&[value]);
        self
    }

    pub(crate) fn finish(self) -> Digest16 {
        self.inner.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest_of(values: &[&[u8]]) -> Digest16 {
        let mut writer = DigestWriter::<Fnv1a128>::new();
        for value in values {
            writer.bytes(value);
        }
        writer.finish()
    }

    #[test]
    fn the_empty_digest_is_the_offset_basis() {
        let writer = DigestWriter::<Fnv1a128>::new();
        assert_eq!(
            writer.finish(),
            Digest16::from_bytes(Fnv1a128::OFFSET_BASIS.to_be_bytes())
        );
    }

    /// The property the whole reconstruction check depends on. Without the length prefix these two
    /// digest identically, and a fold that split a value differently would go unnoticed.
    #[test]
    fn framing_separates_a_differently_split_sequence() {
        assert_ne!(digest_of(&[b"ab", b"c"]), digest_of(&[b"a", b"bc"]));
    }

    #[test]
    fn one_flipped_byte_moves_the_digest() {
        let base = digest_of(&[b"the quick brown fox"]);
        assert_ne!(base, digest_of(&[b"the quick brown fpx"]));
    }

    #[test]
    fn reordering_two_values_moves_the_digest() {
        assert_ne!(digest_of(&[b"one", b"two"]), digest_of(&[b"two", b"one"]));
    }

    /// An empty value and an absent value must not look the same.
    #[test]
    fn an_empty_value_is_not_no_value() {
        assert_ne!(digest_of(&[b""]), digest_of(&[]));
    }

    #[test]
    fn an_integer_is_absorbed_in_a_fixed_width() {
        let mut one = DigestWriter::<Fnv1a128>::new();
        one.integer(1);
        let mut also_one = DigestWriter::<Fnv1a128>::new();
        also_one.integer(1);
        let mut two = DigestWriter::<Fnv1a128>::new();
        two.integer(2);
        let one = one.finish();
        assert_eq!(one, also_one.finish());
        assert_ne!(one, two.finish());
    }

    /// A tag byte and a one-byte value must not collide, or a `NULL`-shaped variant could
    /// impersonate a value.
    #[test]
    fn a_tag_is_not_a_one_byte_value() {
        let mut tagged = DigestWriter::<Fnv1a128>::new();
        tagged.tag(7);
        let mut valued = DigestWriter::<Fnv1a128>::new();
        valued.bytes(&[7]);
        assert_ne!(tagged.finish(), valued.finish());
    }

    #[test]
    fn hex_is_thirty_two_lowercase_characters() {
        let hex = Digest16::from_bytes([0xab; 16]).to_hex();
        assert_eq!(hex, "ab".repeat(16));
        assert_eq!(hex.len(), 32);
    }

    /// The digest is a pure function of the bytes, so two runs in the same process agree. The
    /// cross-process claim is checked in `tests/reconstruction.rs`, where a second process
    /// recomputes it.
    #[test]
    fn the_digest_is_deterministic_within_a_process() {
        assert_eq!(digest_of(&[b"stable"]), digest_of(&[b"stable"]));
    }
}
