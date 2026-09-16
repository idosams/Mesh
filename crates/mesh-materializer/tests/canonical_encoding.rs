//! The framing properties of the canonical encoding, on the cases most likely to break them.
//!
//! Length prefixes and counts are only worth having if a boundary is never inferred from the data.
//! Each case below is a pair of states that a naive encoder would give one byte stream: a name that
//! is a prefix of another, an empty collection beside an absent one, two fields whose values could
//! be split differently across the boundary between them.

mod common;

use common::digest;
use mesh_materializer::{
    materialize, AppliedChangeSet, ChangeSetId, ManifestId, NormalizedName, ObjectId, Operation,
    PortableMetadata, StateDigest, VersionId, WorkspaceState, STATE_DOMAIN,
};

fn object(byte: u8) -> ObjectId {
    ObjectId::from_bytes([byte; 16])
}

fn version(byte: u8) -> VersionId {
    VersionId::from_bytes([byte; 32])
}

fn name(text: &str) -> NormalizedName {
    NormalizedName::new(text).expect("a legal name")
}

/// Materialize one ChangeSet's worth of operations against a fresh workspace.
fn state_of(operations: Vec<Operation>) -> WorkspaceState {
    materialize(
        object(0),
        &[AppliedChangeSet::genesis(
            ChangeSetId::from_bytes([1; 32]),
            operations,
        )],
    )
    .state()
    .clone()
}

fn file_at(id: u8, version_byte: u8, entry: &str, executable: bool) -> Vec<Operation> {
    vec![
        Operation::CreateFile {
            object_id: object(id),
        },
        Operation::WriteFileVersion {
            object_id: object(id),
            version_id: version(version_byte),
            parent_versions: vec![],
            manifest_id: ManifestId::from_bytes([9; 32]),
            portable_metadata: PortableMetadata::new(executable),
        },
        Operation::LinkDirectoryEntry {
            directory_id: object(0),
            name: name(entry),
            object_id: object(id),
            version_id: version(version_byte),
        },
    ]
}

#[test]
fn the_domain_opens_the_encoding_length_prefixed() {
    let bytes = WorkspaceState::empty(object(0)).canonical_bytes();
    assert_eq!(&bytes[..8], &(STATE_DOMAIN.len() as u64).to_be_bytes());
    assert_eq!(&bytes[8..8 + STATE_DOMAIN.len()], STATE_DOMAIN.as_bytes());
}

#[test]
fn a_name_that_is_a_prefix_of_another_does_not_collide() {
    assert_ne!(
        state_of(file_at(1, 1, "a", false)).canonical_bytes(),
        state_of(file_at(1, 1, "ab", false)).canonical_bytes()
    );
}

#[test]
fn one_flipped_metadata_bit_moves_the_encoding() {
    assert_ne!(
        state_of(file_at(1, 1, "a", false)).canonical_bytes(),
        state_of(file_at(1, 1, "a", true)).canonical_bytes()
    );
}

#[test]
fn an_empty_directory_is_not_an_absent_one() {
    let with = state_of(vec![Operation::CreateDirectory {
        object_id: object(1),
    }]);
    let without = state_of(vec![]);
    assert_ne!(with.canonical_bytes(), without.canonical_bytes());
    assert!(with.directory(object(1)).expect("a directory").is_empty());
    assert!(without.directory(object(1)).is_none());
}

#[test]
fn a_file_and_a_directory_under_one_name_do_not_collide() {
    let file = state_of(file_at(1, 1, "a", false));
    let directory = state_of(vec![
        Operation::CreateDirectory {
            object_id: object(1),
        },
        Operation::LinkDirectoryEntry {
            directory_id: object(0),
            name: name("a"),
            object_id: object(1),
            version_id: version(1),
        },
    ]);
    assert_ne!(file.canonical_bytes(), directory.canonical_bytes());
}

#[test]
fn the_root_identifier_participates() {
    assert_ne!(
        WorkspaceState::empty(object(0)).canonical_bytes(),
        WorkspaceState::empty(object(1)).canonical_bytes()
    );
}

#[test]
fn the_encoding_is_a_pure_function_of_the_state() {
    let state = state_of(file_at(1, 1, "notes.md", true));
    assert_eq!(state.canonical_bytes(), state.canonical_bytes());
    assert_eq!(
        state.state_hash::<digest::TestDigest>(),
        state.state_hash::<digest::TestDigest>()
    );
}

/// Streaming the encoding into the digest and digesting the finished buffer must agree, because the
/// state hash is *defined* as the digest of the canonical bytes and the streaming form is an
/// optimisation.
#[test]
fn streaming_the_encoding_matches_digesting_the_buffer() {
    let state = state_of(file_at(1, 1, "notes.md", true));
    assert_eq!(
        state.state_hash::<digest::TestDigest>(),
        digest::digest_of(&state.canonical_bytes())
    );
}

/// The domain is absorbed, so a state hash cannot collide with a digest of the same bytes taken in
/// another domain. Asserted by construction: digesting the encoding *without* the domain gives a
/// different answer.
#[test]
fn the_domain_participates_in_the_state_hash() {
    let state = WorkspaceState::empty(object(0));
    let bytes = state.canonical_bytes();
    let without_domain = &bytes[8 + STATE_DOMAIN.len()..];
    let mut bare = digest::TestDigest::start();
    bare.absorb(without_domain);
    assert_ne!(state.state_hash::<digest::TestDigest>(), bare.finish());
}
