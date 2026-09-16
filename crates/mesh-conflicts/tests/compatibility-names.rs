//! Cross-platform name compatibility, decided by the conflict engine rather than by a filesystem.
//!
//! # What this suite is for
//!
//! `resolve_tree` already separates two objects that ask for the same *bytes* in one directory —
//! plan §4.8 row eight — by giving the later one a derived suffix. This suite is about the harder
//! pair: two objects whose names are visibly different to a person and identical to a filesystem.
//! `README.md` and `readme.md`. `café.md` typed two ways. `ı.txt` and `i.txt` on Windows.
//!
//! **Neither of those objects is renamed, and that is the assertion.** A rename that no operation
//! records is data loss: the author's name is gone, the next synchronization sees a change nobody
//! made, and no conflict is raised because the system believes it was helping. So both names
//! stand, the pair is reported as a [`PortabilityCollision`], and the layer that knows which
//! volume it is writing to refuses before the first byte lands.
//!
//! # The corpus is the same one `mesh-state` uses, and that is deliberate
//!
//! `crates/mesh-state/tests/name-corpus.txt` is generated from the Unicode Character Database and
//! carries the UCD's answer for every pair. Both crates carry a byte-identical `src/fold.rs`, so
//! reading one corpus from both is what proves the two copies really do agree: a fold that drifted
//! in one crate would answer this file differently from `mesh-state`'s `tests/names.rs`, and the
//! two would not both be green.
//!
//! # What is NOT proved here
//!
//! **No filesystem is touched, on any platform.** Everything below is a pure function of an
//! operation set. Whether a real NTFS volume folds a given pair the way `crates/mesh-state`'s
//! profile says it does is a claim about that filesystem, and settling it needs a filesystem
//! adapter this tree does not have.

use std::fs;
use std::path::PathBuf;

use mesh_conflicts::{
    caseless_key, relate, resolve_tree, upcase_key, ActorId, Change, Effect, EventId, Lamport,
    NameFold, NameRelation, NormalizedName, ObjectId, ObjectKind, Snapshot, Stamp,
};

/// A stamp at this counter with this event byte.
fn at(lamport: u64, event: u8) -> Stamp {
    Stamp::new(
        Lamport::new(lamport),
        EventId::from_bytes([event; 16]),
        [0; 32],
    )
}

/// A name, which must be one.
fn name(text: &str) -> NormalizedName {
    NormalizedName::new(text).expect("the test corpus holds only structurally valid names")
}

/// One actor's change.
fn change(stamp: Stamp, effect: Effect) -> Change {
    Change::new(stamp, ActorId::from_bytes([1; 32]), effect)
}

/// An empty root, and its identifier.
fn empty_root() -> (Snapshot, ObjectId) {
    let root = ObjectId::from_bytes([0; 16]);
    (Snapshot::new(root), root)
}

/// The tree that results from creating these names in the root, one per object.
fn tree_of(names: &[&str]) -> mesh_conflicts::TreeResolution {
    let (base, root) = empty_root();
    let changes: Vec<Change> = names
        .iter()
        .enumerate()
        .map(|(index, text)| {
            let object = ObjectId::from_bytes([(index + 1) as u8; 16]);
            change(
                at(1 + index as u64, (index + 1) as u8),
                Effect::Create {
                    object,
                    kind: ObjectKind::File,
                    directory: root,
                    name: name(text),
                },
            )
        })
        .collect();
    resolve_tree(&base, &changes)
}

#[test]
fn a_case_only_pair_is_reported_and_neither_object_is_renamed() {
    let tree = tree_of(&["README.md", "readme.md"]);
    assert!(
        tree.name_collisions().is_empty(),
        "two different names are not a same-name create"
    );
    assert_eq!(tree.portability_collisions().len(), 1);

    let collision = &tree.portability_collisions()[0];
    assert!(collision.relation().joined_by(NameFold::Upcase));
    assert!(collision.relation().joined_by(NameFold::Caseless));
    assert!(!collision.relation().joined_by(NameFold::Canonical));

    let names: Vec<&str> = collision
        .entries()
        .iter()
        .map(|(_, held)| held.as_str())
        .collect();
    assert_eq!(names, ["README.md", "readme.md"]);

    // The tree still holds both, under the names they were created with.
    assert_eq!(
        tree.path_of(ObjectId::from_bytes([1; 16])).unwrap(),
        "/README.md"
    );
    assert_eq!(
        tree.path_of(ObjectId::from_bytes([2; 16])).unwrap(),
        "/readme.md"
    );
}

#[test]
fn a_normalization_only_pair_is_reported_and_neither_object_is_renamed() {
    let tree = tree_of(&["caf\u{e9}.md", "cafe\u{301}.md"]);
    assert_eq!(tree.portability_collisions().len(), 1);
    let collision = &tree.portability_collisions()[0];
    assert!(collision.relation().joined_by(NameFold::Canonical));
    assert!(!collision.relation().joined_by(NameFold::Upcase));
    assert_eq!(
        tree.path_of(ObjectId::from_bytes([1; 16])).unwrap(),
        "/caf\u{e9}.md"
    );
    assert_eq!(
        tree.path_of(ObjectId::from_bytes([2; 16])).unwrap(),
        "/cafe\u{301}.md"
    );
}

#[test]
fn the_dotless_i_is_found_even_though_the_caseless_fold_walks_past_it() {
    // The pair that makes the union of two fold passes load-bearing. Grouping by the caseless key
    // alone finds nothing here, and NTFS holds the two as one entry.
    assert_ne!(caseless_key("\u{131}.txt"), caseless_key("i.txt"));
    assert_eq!(upcase_key("\u{131}.txt"), upcase_key("i.txt"));

    let tree = tree_of(&["\u{131}.txt", "i.txt"]);
    assert_eq!(
        tree.portability_collisions().len(),
        1,
        "a caseless-only pass would report nothing here"
    );
    let collision = &tree.portability_collisions()[0];
    assert!(collision.relation().joined_by(NameFold::Upcase));
    assert!(!collision.relation().joined_by(NameFold::Caseless));
}

#[test]
fn an_identical_pair_stays_a_same_name_create_and_is_not_reported_twice() {
    // Row eight resolves this one by deriving a name, so it must not also appear as a portability
    // collision — the same problem in two lists is the shape a person resolves twice.
    let tree = tree_of(&["notes.md", "notes.md"]);
    assert_eq!(tree.name_collisions().len(), 1);
    assert!(tree.portability_collisions().is_empty());
}

#[test]
fn a_derived_disambiguation_never_creates_a_new_portability_collision() {
    // Three creates of one name: two get derived suffixes. The suffixes are hexadecimal, which
    // folds to itself, so the derivation cannot manufacture the very problem it resolves.
    let tree = tree_of(&["notes.md", "notes.md", "notes.md"]);
    assert_eq!(tree.name_collisions().len(), 1);
    assert_eq!(tree.name_collisions()[0].renamed().len(), 2);
    assert!(tree.portability_collisions().is_empty());
}

#[test]
fn names_that_merely_look_alike_are_not_a_collision() {
    for pair in [
        ["README.md", "READ_ME.md"],
        ["\u{FB01}le.txt", "file.txt"],
        ["stra\u{df}e.txt", "strasse.txt"],
        ["\u{41A}.txt", "K.txt"],
        ["report.", "report"],
    ] {
        let tree = tree_of(&pair);
        assert!(
            tree.portability_collisions().is_empty(),
            "{pair:?} was reported as colliding"
        );
    }
}

#[test]
fn a_collision_in_one_directory_is_not_a_collision_across_two() {
    let (base, root) = empty_root();
    let first = ObjectId::from_bytes([1; 16]);
    let second = ObjectId::from_bytes([2; 16]);
    let left = ObjectId::from_bytes([3; 16]);
    let right = ObjectId::from_bytes([4; 16]);
    let changes = [
        change(
            at(1, 1),
            Effect::Create {
                object: left,
                kind: ObjectKind::Directory,
                directory: root,
                name: name("left"),
            },
        ),
        change(
            at(2, 2),
            Effect::Create {
                object: right,
                kind: ObjectKind::Directory,
                directory: root,
                name: name("right"),
            },
        ),
        change(
            at(3, 3),
            Effect::Create {
                object: first,
                kind: ObjectKind::File,
                directory: left,
                name: name("README.md"),
            },
        ),
        change(
            at(4, 4),
            Effect::Create {
                object: second,
                kind: ObjectKind::File,
                directory: right,
                name: name("readme.md"),
            },
        ),
    ];
    let tree = resolve_tree(&base, &changes);
    assert!(tree.portability_collisions().is_empty());
}

#[test]
fn a_group_of_three_reports_only_what_every_pair_shares() {
    let tree = tree_of(&["caf\u{e9}.md", "cafe\u{301}.md", "CAF\u{c9}.md"]);
    assert_eq!(tree.portability_collisions().len(), 1);
    let collision = &tree.portability_collisions()[0];
    assert_eq!(collision.entries().len(), 3);
    assert!(collision.relation().joined_by(NameFold::Caseless));
    assert!(!collision.relation().joined_by(NameFold::Canonical));
    assert!(!collision.relation().joined_by(NameFold::Upcase));
}

#[test]
fn detection_does_not_depend_on_the_order_the_operations_arrived_in() {
    // The whole crate's determinism promise, applied to this detector: one operation set, one
    // answer, whatever order the changes were handed over in.
    let names = ["README.md", "readme.md", "caf\u{e9}.md", "cafe\u{301}.md"];
    let forward = tree_of(&names);
    let mut reversed = names;
    reversed.reverse();
    let backward = tree_of(&reversed);
    assert_eq!(
        forward.portability_collisions().len(),
        backward.portability_collisions().len()
    );
    assert_eq!(forward.portability_collisions().len(), 2);
}

/// Every row of the shared corpus, decoded from its hexadecimal code points.
fn corpus() -> Vec<(NameRelation, String, String, String)> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../mesh-state/tests/name-corpus.txt");
    let text = fs::read_to_string(&path).expect("mesh-state's corpus is readable from here");
    let mut rows = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split(';');
        let mut relation = NameRelation::DISTINCT;
        for family in fields.next().expect("a fold list").split(',') {
            relation = match family {
                "none" => relation,
                "exact" => relation.with(NameFold::Exact),
                "canonical" => relation.with(NameFold::Canonical),
                "upcase" => relation.with(NameFold::Upcase),
                "caseless" => relation.with(NameFold::Caseless),
                other => panic!("the corpus names an unknown fold family {other:?}"),
            };
        }
        let left = decode(fields.next().expect("a left name"));
        let right = decode(fields.next().expect("a right name"));
        let note = fields.next().unwrap_or("").to_owned();
        rows.push((relation, left, right, note));
    }
    assert!(rows.len() > 600, "the corpus shrank to {} rows", rows.len());
    rows
}

/// A name written as space-separated hexadecimal code points.
fn decode(field: &str) -> String {
    field
        .split(' ')
        .filter(|part| !part.is_empty())
        .map(|part| {
            u32::from_str_radix(part, 16)
                .ok()
                .and_then(char::from_u32)
                .unwrap_or_else(|| panic!("the corpus holds {part:?}, which is not a code point"))
        })
        .collect()
}

#[test]
fn this_crate_folds_exactly_the_way_mesh_state_does() {
    // The two copies of `src/fold.rs` are byte-identical and `tests/mesh_state_drift.rs` proves
    // that. This proves the thing that actually matters: the same corpus gets the same answers
    // through this crate's copy.
    let mut wrong = Vec::new();
    for (expected, left, right, note) in corpus() {
        let got = relate(&left, &right);
        if got != expected {
            wrong.push(format!("{note}: expected {expected:?}, got {got:?}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} rows disagree:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

#[test]
fn every_corpus_pair_that_a_volume_folds_is_reported_by_the_tree() {
    // The detector's contract, over the whole corpus rather than over the pairs somebody chose:
    // a pair joined by ANY fold family is reported, a pair joined by none is not, and no pair is
    // renamed either way.
    for (expected, left, right, note) in corpus() {
        if left == right
            || NormalizedName::new(&left).is_err()
            || NormalizedName::new(&right).is_err()
        {
            continue;
        }
        let tree = tree_of(&[left.as_str(), right.as_str()]);
        let reported = !tree.portability_collisions().is_empty();
        let should = !expected.is_distinct();
        assert_eq!(reported, should, "{note}");
        if reported {
            let collision = &tree.portability_collisions()[0];
            assert_eq!(collision.relation(), expected, "{note}");
            let held: Vec<&str> = collision
                .entries()
                .iter()
                .map(|(_, name)| name.as_str())
                .collect();
            assert_eq!(held, [left.as_str(), right.as_str()], "{note}");
        }
        // Whatever the verdict, both names survive exactly as written.
        assert_eq!(
            tree.path_of(ObjectId::from_bytes([1; 16])).unwrap(),
            format!("/{left}")
        );
        assert_eq!(
            tree.path_of(ObjectId::from_bytes([2; 16])).unwrap(),
            format!("/{right}")
        );
    }
}
