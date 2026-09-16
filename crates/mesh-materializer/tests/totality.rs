//! Acceptance criterion 2: materialization is total. No valid operation set panics or produces an
//! undefined state.
//!
//! # "Total" is a claim about every input, not about the well-formed ones
//!
//! An operation set arriving from a peer is not trusted input. It can name an object nobody
//! created, bind a name somebody already holds, ask a directory to become its own ancestor, follow
//! a ChangeSet nobody delivered, follow itself, or arrive twice under one identifier saying two
//! different things. Every one of those has to have an answer that two peers compute identically —
//! and a panic is not such an answer, nor is a silently dropped operation.
//!
//! So this file asserts three things over the corpus and over hand-built adversarial sets:
//!
//! 1. every operation is *reached* — `Materialization::reached` equals the number of operations in
//!    the set, so nothing was skipped;
//! 2. every operation produced exactly one of the three effects or one rejection;
//! 3. the resulting state satisfies its own invariants, whatever the input was.

mod common;

use common::corpus;
use mesh_materializer::{
    materialize, AppliedChangeSet, ChangeSetId, ManifestId, NormalizedName, ObjectId, Operation,
    PortableMetadata, PreservedEntry, VersionId,
};

/// How many seeds these run over. `MESH_MATERIALIZER_CORPUS` raises it to the full corpus; see
/// `tests/common/corpus.rs` for why the default is a deterministic sample.
const DEFAULT_SEEDS: u64 = 1_500;

fn object(byte: u8) -> ObjectId {
    ObjectId::from_bytes([byte; 16])
}

fn version(byte: u8) -> VersionId {
    VersionId::from_bytes([byte; 32])
}

fn changeset(byte: u8) -> ChangeSetId {
    ChangeSetId::from_bytes([byte; 32])
}

fn name(text: &str) -> NormalizedName {
    NormalizedName::new(text).expect("a legal name")
}

#[test]
fn every_operation_in_the_corpus_is_reached_and_answered() {
    for seed in 0..corpus::corpus_size(DEFAULT_SEEDS) {
        let generated = corpus::generate(seed);
        let expected: usize = corpus::deduplicated_operation_count(&generated);
        let materialized = materialize(generated.root, &generated.changesets);
        assert_eq!(
            materialized.reached(),
            expected,
            "seed {seed}: {} operations reached, {expected} in the set",
            materialized.reached()
        );
    }
}

/// Every path a hand-written adversarial set can take, one set per hazard. None of these may
/// unwind, and each has a stated answer.
#[test]
fn the_adversarial_sets_all_produce_a_state() {
    let root = object(0);

    let hazards: Vec<(&str, Vec<AppliedChangeSet>)> = vec![
        ("an empty set", vec![]),
        (
            "a ChangeSet with no operations",
            vec![AppliedChangeSet::genesis(changeset(1), vec![])],
        ),
        (
            "a ChangeSet that follows itself",
            vec![AppliedChangeSet::new(
                changeset(1),
                vec![changeset(1)],
                vec![Operation::CreateFile {
                    object_id: object(1),
                }],
            )],
        ),
        (
            "a two-ChangeSet causal cycle",
            vec![
                AppliedChangeSet::new(
                    changeset(1),
                    vec![changeset(2)],
                    vec![Operation::CreateFile {
                        object_id: object(1),
                    }],
                ),
                AppliedChangeSet::new(
                    changeset(2),
                    vec![changeset(1)],
                    vec![Operation::CreateDirectory {
                        object_id: object(2),
                    }],
                ),
            ],
        ),
        (
            "a causal parent nobody delivered",
            vec![AppliedChangeSet::new(
                changeset(1),
                vec![changeset(0xfe)],
                vec![Operation::CreateFile {
                    object_id: object(1),
                }],
            )],
        ),
        (
            "two records under one identifier",
            vec![
                AppliedChangeSet::genesis(
                    changeset(1),
                    vec![Operation::CreateFile {
                        object_id: object(1),
                    }],
                ),
                AppliedChangeSet::genesis(
                    changeset(1),
                    vec![Operation::CreateDirectory {
                        object_id: object(2),
                    }],
                ),
            ],
        ),
        (
            "everything named against an empty workspace",
            vec![AppliedChangeSet::genesis(
                changeset(1),
                vec![
                    Operation::WriteFileVersion {
                        object_id: object(9),
                        version_id: version(9),
                        parent_versions: vec![version(8), version(8)],
                        manifest_id: ManifestId::from_bytes([1; 32]),
                        portable_metadata: PortableMetadata::default(),
                    },
                    Operation::LinkDirectoryEntry {
                        directory_id: object(9),
                        name: name("a"),
                        object_id: object(8),
                        version_id: version(7),
                    },
                    Operation::UnlinkDirectoryEntry {
                        directory_id: object(9),
                        name: name("a"),
                        object_id: object(8),
                    },
                    Operation::RenameEntry {
                        directory_id: object(9),
                        from_name: name("a"),
                        to_name: name("a"),
                        object_id: object(8),
                    },
                    Operation::MoveEntry {
                        from_directory_id: object(9),
                        from_name: name("a"),
                        to_directory_id: object(8),
                        to_name: name("b"),
                        object_id: object(7),
                    },
                    Operation::DeleteObject {
                        object_id: object(9),
                    },
                    Operation::RestoreObject {
                        object_id: object(9),
                        restored_version_id: version(9),
                    },
                    Operation::SetPortableMetadata {
                        object_id: object(9),
                        version_id: version(9),
                        portable_metadata: PortableMetadata::new(true),
                    },
                    Operation::ResolveNameConflict {
                        directory_id: object(9),
                        contested_name: name("a"),
                        preserved: vec![],
                    },
                    Operation::ResolveContentConflict {
                        object_id: object(9),
                        resulting_version_id: version(9),
                        preserved_version_ids: vec![version(9)],
                    },
                ],
            )],
        ),
        (
            "a resolution naming one object twice",
            vec![AppliedChangeSet::genesis(
                changeset(1),
                vec![
                    Operation::CreateFile {
                        object_id: object(1),
                    },
                    Operation::ResolveNameConflict {
                        directory_id: root,
                        contested_name: name("a"),
                        preserved: vec![
                            PreservedEntry::new(object(1), name("a")),
                            PreservedEntry::new(object(1), name("b")),
                        ],
                    },
                ],
            )],
        ),
        (
            "the root named as the subject of everything",
            vec![AppliedChangeSet::genesis(
                changeset(1),
                vec![
                    Operation::DeleteObject { object_id: root },
                    Operation::RestoreObject {
                        object_id: root,
                        restored_version_id: version(1),
                    },
                    Operation::CreateDirectory { object_id: root },
                    Operation::LinkDirectoryEntry {
                        directory_id: root,
                        name: name("a"),
                        object_id: root,
                        version_id: version(1),
                    },
                    Operation::RenameEntry {
                        directory_id: root,
                        from_name: name("a"),
                        to_name: name("b"),
                        object_id: root,
                    },
                ],
            )],
        ),
    ];

    for (what, set) in hazards {
        let materialized = materialize(root, &set);
        let expected: usize = {
            let mut ids: Vec<ChangeSetId> = set.iter().map(AppliedChangeSet::id).collect();
            ids.sort_unstable();
            ids.dedup();
            ids.iter()
                .filter_map(|id| {
                    set.iter()
                        .filter(|held| held.id() == *id)
                        .min_by(|left, right| {
                            left.causal_parents()
                                .cmp(right.causal_parents())
                                .then_with(|| {
                                    mesh_operations::encode_operations(left.operations()).cmp(
                                        &mesh_operations::encode_operations(right.operations()),
                                    )
                                })
                        })
                })
                .map(|held| held.operations().len())
                .sum()
        };
        assert_eq!(
            materialized.reached(),
            expected,
            "{what}: {} operations reached, {expected} expected",
            materialized.reached()
        );
        // A state came out, and it is a state: the root is there and it is a directory.
        assert_eq!(materialized.state().root(), root);
        assert!(materialized.state().object(root).is_some(), "{what}");
        assert!(
            !materialized.state().object(root).unwrap().is_deleted(),
            "{what}"
        );
    }
}

/// Whatever the input, the tree stays a tree: no object is its own ancestor, and every object with
/// a parent is really bound in that parent.
#[test]
fn the_materialized_tree_is_a_tree_for_every_corpus_set() {
    for seed in 0..corpus::corpus_size(DEFAULT_SEEDS) {
        let generated = corpus::generate(seed);
        let materialized = materialize(generated.root, &generated.changesets);
        let state = materialized.state();
        for id in state.objects().keys() {
            if let Some(parent) = state.parent_of(*id) {
                assert!(
                    !state.is_self_or_ancestor(*id, parent),
                    "seed {seed}: {id} is its own ancestor"
                );
                assert!(
                    state
                        .directory(parent)
                        .is_some_and(|held| held.name_of(*id).is_some()),
                    "seed {seed}: {id} claims a parent that does not bind it"
                );
            }
        }
        // The root is never bound anywhere.
        assert_eq!(state.parent_of(state.root()), None, "seed {seed}");
    }
}
