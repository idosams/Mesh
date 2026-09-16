//! The invariants a materialized state must satisfy, checked against the corpus rather than
//! asserted in prose.
//!
//! `src/state.rs` says the parent index is *derived, never trusted*. This is what makes that
//! sentence checkable: after every generated set, the index the implementation maintains
//! incrementally is compared against one rebuilt from a full scan of the bindings. An index that
//! drifted from the entry maps would let `parent_of`, `binding_of` and the cycle check all agree
//! with each other and all be wrong, which is exactly the class of bug an accessor test cannot see.

mod common;

use common::{corpus, snapshot};
use mesh_materializer::{materialize, ObjectKind};

/// How many seeds these run over. `MESH_MATERIALIZER_CORPUS` raises it to the full corpus; see
/// `tests/common/corpus.rs` for why the default is a deterministic sample.
const DEFAULT_SEEDS: u64 = 1_500;

#[test]
fn the_parent_index_agrees_with_a_full_rescan() {
    for seed in 0..corpus::corpus_size(DEFAULT_SEEDS) {
        let generated = corpus::generate(seed);
        let materialized = materialize(generated.root, &generated.changesets);
        let state = materialized.state();

        let rebuilt = snapshot::rebuilt_parent_index(state);
        let held: std::collections::BTreeMap<String, String> = state
            .objects()
            .keys()
            .filter_map(|id| {
                state
                    .parent_of(*id)
                    .map(|parent| (id.to_string(), parent.to_string()))
            })
            .collect();
        assert_eq!(held, rebuilt, "seed {seed}: the parent index drifted");
    }
}

#[test]
fn every_directory_object_has_a_directory_version_and_no_file_does() {
    for seed in 0..corpus::corpus_size(DEFAULT_SEEDS) {
        let generated = corpus::generate(seed);
        let materialized = materialize(generated.root, &generated.changesets);
        let state = materialized.state();
        for (id, record) in state.objects() {
            match record.kind() {
                ObjectKind::Directory => assert!(
                    state.directory(*id).is_some(),
                    "seed {seed}: directory {id} has no directory version"
                ),
                ObjectKind::File => assert!(
                    state.directory(*id).is_none(),
                    "seed {seed}: file {id} has a directory version"
                ),
            }
        }
    }
}

#[test]
fn every_binding_names_an_object_that_exists() {
    for seed in 0..corpus::corpus_size(DEFAULT_SEEDS) {
        let generated = corpus::generate(seed);
        let materialized = materialize(generated.root, &generated.changesets);
        let state = materialized.state();
        for (directory, version) in state.directories() {
            for (name, entry) in version.entries() {
                assert!(
                    state.object(entry.object_id()).is_some(),
                    "seed {seed}: {directory} binds \"{name}\" to an object that does not exist"
                );
            }
        }
    }
}

#[test]
fn every_file_version_belongs_to_a_file_that_exists() {
    for seed in 0..corpus::corpus_size(DEFAULT_SEEDS) {
        let generated = corpus::generate(seed);
        let materialized = materialize(generated.root, &generated.changesets);
        let state = materialized.state();
        for (id, version) in state.file_versions() {
            let object = state
                .object(version.object_id())
                .unwrap_or_else(|| panic!("seed {seed}: version {id} names a missing object"));
            assert_eq!(
                object.kind(),
                ObjectKind::File,
                "seed {seed}: version {id} belongs to a directory"
            );
        }
    }
}

/// A version that materialization holds as current must be one it recorded — for files. A directory
/// takes a current version only through `RestoreObject`, which names a version this crate has no
/// file record for, and `src/apply.rs` says so.
#[test]
fn a_files_current_version_is_a_version_the_state_holds() {
    for seed in 0..corpus::corpus_size(DEFAULT_SEEDS) {
        let generated = corpus::generate(seed);
        let materialized = materialize(generated.root, &generated.changesets);
        let state = materialized.state();
        for (id, record) in state.objects() {
            if record.kind() != ObjectKind::File {
                continue;
            }
            if let Some(current) = record.current_version() {
                assert!(
                    state.file_version(current).is_some(),
                    "seed {seed}: file {id} is current at a version the state does not hold"
                );
            }
        }
    }
}

/// Deletion is a state, never an erasure: a deleted object keeps every version it ever had.
#[test]
fn deletion_never_removes_a_version() {
    for seed in 0..corpus::corpus_size(DEFAULT_SEEDS) {
        let generated = corpus::generate(seed);
        let materialized = materialize(generated.root, &generated.changesets);
        let state = materialized.state();
        let deleted: Vec<_> = state
            .objects()
            .iter()
            .filter(|(_, record)| record.is_deleted())
            .map(|(id, _)| *id)
            .collect();
        for id in deleted {
            for (version, record) in state.file_versions() {
                if record.object_id() == id {
                    assert!(
                        state.file_version(*version).is_some(),
                        "seed {seed}: a version of deleted {id} went missing"
                    );
                }
            }
        }
    }
}
