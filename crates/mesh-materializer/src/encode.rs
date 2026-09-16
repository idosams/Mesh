//! The canonical encoding of a workspace state — the bytes a state hash is taken over.
//!
//! # Why the bytes are the authority and the hash is a convenience
//!
//! The acceptance criterion this crate exists to satisfy is *materializing the same operation set
//! twice produces byte-identical state*. Comparing two states through a digest answers that
//! question with a probability; comparing the canonical encodings answers it with the bytes. So the
//! encoding is the primary artifact, [`crate::WorkspaceState::state_hash`] is defined over it, and
//! the tests compare encodings rather than digests wherever they can.
//!
//! # Unambiguous framing
//!
//! Every variable-length value is preceded by its length as eight big-endian bytes, every sequence
//! by its count, and every optional by a one-byte tag. Two states therefore cannot share an
//! encoding unless every field of both is equal: there is no place where a boundary is inferred
//! from the data. `tests/canonical_encoding.rs` holds that against the cases most likely to break
//! it — an empty directory beside an absent one, a name that is a prefix of another, a version set
//! that differs only in order before normalization.
//!
//! # What is not encoded
//!
//! The parent index. It is a function of the directory versions
//! ([`crate::WorkspaceState::parent_of`]), and a derived cache that fed into a state hash would let
//! a state be named by something that is not its content. `tests/state_invariants.rs` rebuilds it
//! from a full scan instead.

use crate::ids::StateHash;
use crate::state::WorkspaceState;
use crate::version::ObjectKind;

/// The versioned label the canonical encoding opens with.
///
/// Absorbed first and length-prefixed, so a state hash can never collide with a digest of the same
/// bytes taken in another domain, and so a domain rename cannot be reproduced by a differently
/// split label.
pub const STATE_DOMAIN: &str = "mesh.v0.workspace-state";

/// The digest seam: an algorithm that absorbs bytes and produces a [`StateHash`].
///
/// The protocol digest is BLAKE3 and `mesh-types` already carries an implementation. This crate
/// cannot import it — `src/ids.rs` states why — so the composition root supplies it.
///
/// **No implementation ships here**, for the reason `mesh-state`'s `HeadDigest` states in more
/// detail: a state hash is protocol identity, it is what an approval refers to, and a convenient
/// default would be a name that looks like a [`StateHash`] and is not the one any other
/// implementation computes. The framing below is this crate's; the algorithm is the
/// implementation's.
pub trait StateDigest {
    /// A fresh accumulator.
    fn start() -> Self;

    /// Absorb bytes.
    fn absorb(&mut self, bytes: &[u8]);

    /// Finish, producing the state hash.
    fn finish(self) -> StateHash;
}

/// A byte sink the encoder writes through, so one encoding routine serves both the bytes and the
/// digest without materializing the bytes twice.
trait Sink {
    fn put(&mut self, bytes: &[u8]);
}

impl Sink for Vec<u8> {
    fn put(&mut self, bytes: &[u8]) {
        self.extend_from_slice(bytes);
    }
}

struct DigestSink<D: StateDigest>(D);

impl<D: StateDigest> Sink for DigestSink<D> {
    fn put(&mut self, bytes: &[u8]) {
        self.0.absorb(bytes);
    }
}

/// Write `bytes` preceded by its length.
fn framed(sink: &mut impl Sink, bytes: &[u8]) {
    sink.put(&(bytes.len() as u64).to_be_bytes());
    sink.put(bytes);
}

/// Write a count.
fn count(sink: &mut impl Sink, value: usize) {
    sink.put(&(value as u64).to_be_bytes());
}

/// Write a one-byte tag.
fn tag(sink: &mut impl Sink, value: u8) {
    sink.put(&[value]);
}

/// The whole encoding, in one place, over any sink.
fn encode_state(state: &WorkspaceState, sink: &mut impl Sink) {
    framed(sink, STATE_DOMAIN.as_bytes());
    sink.put(state.root().as_bytes());

    count(sink, state.objects().len());
    for (id, record) in state.objects() {
        sink.put(id.as_bytes());
        tag(
            sink,
            match record.kind() {
                ObjectKind::File => 0,
                ObjectKind::Directory => 1,
            },
        );
        match record.created_by() {
            None => tag(sink, 0),
            Some(changeset) => {
                tag(sink, 1);
                sink.put(changeset.as_bytes());
            }
        }
        tag(sink, u8::from(record.is_deleted()));
        match record.current_version() {
            None => tag(sink, 0),
            Some(version) => {
                tag(sink, 1);
                sink.put(version.as_bytes());
            }
        }
    }

    count(sink, state.directories().len());
    for (id, directory) in state.directories() {
        sink.put(id.as_bytes());
        count(sink, directory.entries().len());
        for (name, entry) in directory.entries() {
            framed(sink, name.as_str().as_bytes());
            sink.put(entry.object_id().as_bytes());
            sink.put(entry.version_id().as_bytes());
        }
    }

    count(sink, state.file_versions().len());
    for (id, version) in state.file_versions() {
        sink.put(id.as_bytes());
        sink.put(version.object_id().as_bytes());
        count(sink, version.parent_versions().len());
        for parent in version.parent_versions() {
            sink.put(parent.as_bytes());
        }
        sink.put(version.manifest_id().as_bytes());
        tag(sink, u8::from(version.portable_metadata().is_executable()));
        sink.put(version.created_by().as_bytes());
    }

    count(sink, state.actor_heads().len());
    for (actor, head) in state.actor_heads() {
        sink.put(actor.as_bytes());
        sink.put(head.as_bytes());
    }

    match state.canonical_head() {
        None => tag(sink, 0),
        Some(advance) => {
            tag(sink, 1);
            sink.put(advance.head().as_bytes());
            sink.put(advance.approval().as_bytes());
        }
    }
}

impl WorkspaceState {
    /// The canonical encoding of this state.
    ///
    /// Two states are the same state exactly when these bytes are equal. See the module header for
    /// what is framed and what is left out.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        encode_state(self, &mut bytes);
        bytes
    }

    /// The state hash: `D` taken over [`WorkspaceState::canonical_bytes`].
    ///
    /// Streamed rather than taken over a buffer, so hashing a large state does not require holding
    /// its encoding in memory. Equal to digesting `canonical_bytes()` directly, which
    /// `tests/canonical_encoding.rs` asserts rather than assumes.
    #[must_use]
    pub fn state_hash<D: StateDigest>(&self) -> StateHash {
        let mut sink = DigestSink(D::start());
        encode_state(self, &mut sink);
        sink.0.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{ChangeSetId, ManifestId, ObjectId, VersionId};
    use crate::name::{NormalizedName, PortableMetadata};
    use crate::version::{DirectoryEntry, FileVersion, ObjectRecord};

    fn object(byte: u8) -> ObjectId {
        ObjectId::from_bytes([byte; 16])
    }

    fn name(text: &str) -> NormalizedName {
        NormalizedName::new(text).unwrap()
    }

    #[test]
    fn the_domain_opens_the_encoding() {
        let bytes = WorkspaceState::empty(object(0)).canonical_bytes();
        let length = STATE_DOMAIN.len();
        assert_eq!(&bytes[..8], &(length as u64).to_be_bytes());
        assert_eq!(&bytes[8..8 + length], STATE_DOMAIN.as_bytes());
    }

    #[test]
    fn two_empty_states_with_different_roots_encode_differently() {
        assert_ne!(
            WorkspaceState::empty(object(0)).canonical_bytes(),
            WorkspaceState::empty(object(1)).canonical_bytes()
        );
    }

    #[test]
    fn a_name_that_is_a_prefix_of_another_does_not_collide() {
        let mut one = WorkspaceState::empty(object(0));
        one.insert_object(
            object(1),
            ObjectRecord::minted(
                crate::version::ObjectKind::File,
                ChangeSetId::from_bytes([1; 32]),
            ),
        );
        one.bind(
            object(0),
            name("ab"),
            DirectoryEntry::new(object(1), VersionId::from_bytes([1; 32])),
        );

        let mut other = WorkspaceState::empty(object(0));
        other.insert_object(
            object(1),
            ObjectRecord::minted(
                crate::version::ObjectKind::File,
                ChangeSetId::from_bytes([1; 32]),
            ),
        );
        other.bind(
            object(0),
            name("a"),
            DirectoryEntry::new(object(1), VersionId::from_bytes([1; 32])),
        );

        assert_ne!(one.canonical_bytes(), other.canonical_bytes());
    }

    #[test]
    fn an_absent_version_is_not_a_present_one() {
        let mut with = WorkspaceState::empty(object(0));
        with.insert_object(
            object(1),
            ObjectRecord::minted(
                crate::version::ObjectKind::File,
                ChangeSetId::from_bytes([0; 32]),
            )
            .with_current_version(Some(VersionId::from_bytes([0; 32]))),
        );
        let mut without = WorkspaceState::empty(object(0));
        without.insert_object(
            object(1),
            ObjectRecord::minted(
                crate::version::ObjectKind::File,
                ChangeSetId::from_bytes([0; 32]),
            ),
        );
        assert_ne!(with.canonical_bytes(), without.canonical_bytes());
    }

    #[test]
    fn a_file_version_participates_in_the_encoding() {
        let mut state = WorkspaceState::empty(object(0));
        let before = state.canonical_bytes();
        state.insert_file_version(
            VersionId::from_bytes([5; 32]),
            FileVersion::new(
                object(1),
                vec![],
                ManifestId::from_bytes([6; 32]),
                PortableMetadata::new(true),
                ChangeSetId::from_bytes([7; 32]),
            ),
        );
        assert_ne!(before, state.canonical_bytes());
    }

    #[test]
    fn the_encoding_is_a_pure_function_of_the_state() {
        let state = WorkspaceState::empty(object(3));
        assert_eq!(state.canonical_bytes(), state.canonical_bytes());
    }
}
