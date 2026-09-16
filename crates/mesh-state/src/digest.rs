//! How a head is named, and why this crate ships no implementation of it.
//!
//! # The head is derived, never asserted
//!
//! An actor head names the workspace state that actor currently treats as current. If an author
//! could *assert* the resulting head, a head could advance to a state nobody derived — and, at the
//! publication boundary, to a state nobody reviewed. So the head is a function of the applied
//! causal set and of nothing else: absorb the domain, absorb the count, absorb every applied
//! ChangeSet identifier in causal order, digest. A receiver that recomputes it and disagrees with
//! what an author claimed refuses the ChangeSet ([`crate::Refusal`]) rather than believing it.
//!
//! # The digest itself is a seam with no default, and that is on purpose
//!
//! The protocol digest is BLAKE3, and `mesh-types` already carries an implementation. This crate
//! cannot import it — `ids.rs` states why — so [`HeadDigest`] is the seam and the composition root
//! supplies it.
//!
//! No implementation ships here. `mesh-store` faced the same fence for its *index* digest and
//! shipped `Fnv1a128` as a default, which is sound there because that digest's only adversary is a
//! bug in a rebuild. A head identifier is a different thing: it is protocol identity, it is
//! carried between peers, and it is what an approval refers to. A default implementation here
//! would be a name that looks like a `HeadId` and is not the one any other implementation
//! computes, and it would be reached for exactly once by a lane in a hurry. Making the caller
//! supply one is the friction that keeps that from happening.
//!
//! The framing below is this crate's; the algorithm is the implementation's. Two implementations
//! agree on nothing but the framing, which is the point of a seam.

use crate::ids::{ChangeSetId, HeadId};

/// The versioned label every head digest is derived under.
///
/// Absorbed first, so that a head identifier can never collide with a digest of the same bytes
/// taken in another domain.
pub const HEAD_DOMAIN: &str = "mesh.v0.actor-head";

/// The digest seam: an algorithm that absorbs bytes and produces a [`HeadId`].
///
/// Implement this over BLAKE3 at the composition root. Nothing in this crate implements it, and
/// nothing in this crate names an algorithm.
pub trait HeadDigest {
    /// A fresh accumulator.
    fn start() -> Self;

    /// Absorb bytes.
    fn absorb(&mut self, bytes: &[u8]);

    /// Finish, producing the head identifier.
    fn finish(self) -> HeadId;
}

/// Unambiguous framing over any [`HeadDigest`].
///
/// Every variable-length value is preceded by its length and the sequence is preceded by its
/// count, so no two distinct causal sets can produce one byte stream. Without the count, the empty
/// causal set and a set holding the all-zero identifier would be hard to keep apart; without the
/// length prefix on the domain, a domain rename could be absorbed by a differently-split label.
pub(crate) struct HeadWriter<D: HeadDigest> {
    inner: D,
}

impl<D: HeadDigest> HeadWriter<D> {
    /// Begin, absorbing the domain first.
    pub(crate) fn new() -> Self {
        let mut inner = D::start();
        absorb_length_prefixed(&mut inner, HEAD_DOMAIN.as_bytes());
        Self { inner }
    }

    /// Absorb how many identifiers follow.
    pub(crate) fn count(&mut self, value: usize) -> &mut Self {
        self.inner.absorb(&(value as u64).to_be_bytes());
        self
    }

    /// Absorb one identifier, at its fixed width.
    pub(crate) fn changeset(&mut self, id: &ChangeSetId) -> &mut Self {
        self.inner.absorb(id.as_bytes());
        self
    }

    /// Finish.
    pub(crate) fn finish(self) -> HeadId {
        self.inner.finish()
    }
}

/// Absorb a byte string preceded by its length.
fn absorb_length_prefixed<D: HeadDigest>(inner: &mut D, bytes: &[u8]) {
    inner.absorb(&(bytes.len() as u64).to_be_bytes());
    inner.absorb(bytes);
}

/// The head over an already causally-ordered sequence of applied ChangeSets.
///
/// The caller owns the ordering, because the ordering is the protocol rule and belongs next to it
/// in `advance.rs`, not here. This function is the framing and nothing else.
pub(crate) fn head_over<'a, D, I>(ordered: I) -> HeadId
where
    D: HeadDigest,
    I: ExactSizeIterator<Item = &'a ChangeSetId>,
{
    let mut writer = HeadWriter::<D>::new();
    writer.count(ordered.len());
    for id in ordered {
        writer.changeset(id);
    }
    writer.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test double, and named as one. It exists so that the framing's own properties — a
    /// reordered sequence moves the head, a differently-counted sequence moves the head — can be
    /// asserted without an algorithm. It is not collision-resistant and is not exported.
    #[derive(Clone, Copy, Debug)]
    struct CountingDigest(u128);

    impl HeadDigest for CountingDigest {
        fn start() -> Self {
            Self(0x6c62_272e_07bb_0142_62b8_2175_6295_c58d)
        }

        fn absorb(&mut self, bytes: &[u8]) {
            let mut state = self.0;
            for byte in bytes {
                state ^= u128::from(*byte);
                state = state.wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_013b);
            }
            self.0 = state;
        }

        fn finish(self) -> HeadId {
            let mut bytes = [0u8; 32];
            bytes[..16].copy_from_slice(&self.0.to_be_bytes());
            bytes[16..].copy_from_slice(&self.0.rotate_left(37).to_be_bytes());
            HeadId::from_bytes(bytes)
        }
    }

    fn id(byte: u8) -> ChangeSetId {
        ChangeSetId::from_bytes([byte; 32])
    }

    fn head(ids: &[ChangeSetId]) -> HeadId {
        head_over::<CountingDigest, _>(ids.iter())
    }

    #[test]
    fn the_empty_causal_set_has_a_head() {
        assert_eq!(head(&[]), head(&[]));
    }

    #[test]
    fn the_empty_causal_set_is_not_a_set_holding_one_identifier() {
        assert_ne!(head(&[]), head(&[id(0)]));
    }

    #[test]
    fn reordering_moves_the_head() {
        assert_ne!(head(&[id(1), id(2)]), head(&[id(2), id(1)]));
    }

    #[test]
    fn one_flipped_byte_moves_the_head() {
        let mut other = [1u8; 32];
        other[17] = 2;
        assert_ne!(head(&[id(1)]), head(&[ChangeSetId::from_bytes(other)]));
    }

    #[test]
    fn the_head_is_a_pure_function_of_the_sequence() {
        assert_eq!(head(&[id(9), id(4)]), head(&[id(9), id(4)]));
    }

    /// The domain is absorbed length-prefixed, so a shorter domain followed by a first identifier
    /// cannot reproduce a longer one. Asserted by construction: the framing is fixed, so the test
    /// pins that the domain participates at all.
    #[test]
    fn the_domain_participates_in_the_head() {
        let framed = head(&[]);
        let mut bare = CountingDigest::start();
        bare.absorb(&0u64.to_be_bytes());
        assert_ne!(framed, bare.finish());
    }
}
