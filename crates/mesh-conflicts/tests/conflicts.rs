//! One targeted test per row of plan §4.8, each written to fail if that row is removed.
//!
//! The task contract asks for exactly this: *"One targeted test per conflict-table row, each
//! failing if the rule is removed."* Each test below names, in its own doc comment, the change to
//! `src/` that would make it pass without the rule — so the claim is checkable rather than
//! asserted. `row_coverage` at the end fails if a row of [`Rule::TABLE`] has no test here.
//!
//! Two of the eleven rows are not about the object graph and are exercised through their own entry
//! points: row ten through [`head_movement`] and row eleven through [`ContextLedger`]. They are
//! rows of the same table and get the same treatment.

mod support;

use std::collections::{BTreeMap, BTreeSet};

use mesh_conflicts::{
    head_movement, resolve, ActorId, Change, Content, ContextLedger, Disposition, Effect, HeadId,
    HeadMovement, ObjectId, ObjectKind, Rule, Snapshot, VersionId,
};

use support::{at, lines, name, object, version};

/// Which rows this file covers. `row_coverage` requires it to be all of them.
const COVERED: [Rule; 11] = [
    Rule::EditFollowsIdentity,
    Rule::ChildStaysAttached,
    Rule::IndependentFilesMerge,
    Rule::ThreeWayTextMerge,
    Rule::PreserveOverlappingText,
    Rule::PreserveBinaryVersions,
    Rule::PreserveTombstoneAndEdit,
    Rule::RetainBothIdentities,
    Rule::DeterministicCycleBreak,
    Rule::HeadMovedAfterReview,
    Rule::ContextInputChanged,
];

fn by(actor: u8, stamp: mesh_conflicts::Stamp, effect: Effect) -> Change {
    Change::new(stamp, ActorId::from_bytes([actor; 32]), effect)
}

/// Root, one directory, one text file in it.
fn base_with_text() -> (Snapshot, ObjectId, ObjectId, ObjectId) {
    let root = object(0);
    let folder = object(1);
    let notes = object(2);
    let base = Snapshot::new(root)
        .with_directory(folder, root, name("folder"))
        .with_file(
            notes,
            folder,
            name("notes.md"),
            Content::Text {
                version: version(1),
                lines: lines(&["one", "two", "three", "four", "five"]),
            },
        );
    (base, root, folder, notes)
}

// ---------------------------------------------------------------------------------------------
// Row 1 — Rename + edit same object → Edit follows stable object ID
// ---------------------------------------------------------------------------------------------

/// Fails if the edit is keyed by anything but the object identifier.
///
/// The mutation this kills: make `Effect::WriteText` carry a path, or make `resolve` drop a write
/// whose object was renamed in the same set. Either way the rename lands and the edit does not,
/// and `version_of` comes back as the base version.
#[test]
fn row_1_a_rename_and_an_edit_of_one_object_both_land() {
    let (base, _, _, notes) = base_with_text();
    let changes = [
        by(
            1,
            at(5, 1),
            Effect::Rename {
                object: notes,
                name: name("journal.md"),
            },
        ),
        by(
            2,
            at(5, 2),
            Effect::WriteText {
                object: notes,
                version: version(2),
                lines: lines(&["one", "TWO", "three", "four", "five"]),
            },
        ),
    ];
    let resolved = resolve(&base, &changes);

    assert_eq!(resolved.path_of(notes).unwrap(), "/folder/journal.md");
    assert_eq!(resolved.version_of(notes), Some(version(2)));
    assert!(resolved
        .rules_applied()
        .contains(&Rule::EditFollowsIdentity));
    assert!(!resolved.needs_review());
}

// ---------------------------------------------------------------------------------------------
// Row 2 — Move directory + child edit → Child remains attached to object graph
// ---------------------------------------------------------------------------------------------

/// Fails if a moved directory detaches its children.
///
/// The mutation this kills: store a path per object instead of a directory entry, so moving the
/// parent leaves the child's recorded path pointing at a directory that no longer holds it. The
/// child's derived path is asserted through the *new* parent path, which a stored-path model gets
/// wrong without any rule having to be deleted.
#[test]
fn row_2_a_child_edited_while_its_directory_moved_stays_attached() {
    let (base, root, folder, notes) = base_with_text();
    let archive = object(3);
    let base = base.with_directory(archive, root, name("archive"));

    let changes = [
        by(
            1,
            at(5, 1),
            Effect::Reparent {
                object: folder,
                directory: archive,
            },
        ),
        by(
            2,
            at(5, 2),
            Effect::WriteText {
                object: notes,
                version: version(2),
                lines: lines(&["one", "two", "three", "four", "FIVE"]),
            },
        ),
    ];
    let resolved = resolve(&base, &changes);

    assert_eq!(resolved.path_of(notes).unwrap(), "/archive/folder/notes.md");
    assert_eq!(resolved.version_of(notes), Some(version(2)));
    assert!(resolved.rules_applied().contains(&Rule::ChildStaysAttached));
    assert!(!resolved.needs_review());
}

// ---------------------------------------------------------------------------------------------
// Row 3 — Independent files → Merge automatically
// ---------------------------------------------------------------------------------------------

/// Fails if two edits to two different files need a human.
///
/// The mutation this kills: widen any preserve-both rule to fire on concurrency alone rather than
/// on concurrency *over one object*. `needs_review` turns true and the assertion catches it.
#[test]
fn row_3_edits_to_different_files_merge_with_no_human_involvement() {
    let (base, root, _, notes) = base_with_text();
    let other = object(4);
    let base = base.with_file(
        other,
        root,
        name("other.md"),
        Content::Text {
            version: version(9),
            lines: lines(&["x"]),
        },
    );

    let changes = [
        by(
            1,
            at(5, 1),
            Effect::WriteText {
                object: notes,
                version: version(2),
                lines: lines(&["ONE", "two", "three", "four", "five"]),
            },
        ),
        by(
            2,
            at(5, 2),
            Effect::WriteText {
                object: other,
                version: version(3),
                lines: lines(&["X"]),
            },
        ),
    ];
    let resolved = resolve(&base, &changes);

    assert!(!resolved.needs_review());
    assert_eq!(resolved.version_of(notes), Some(version(2)));
    assert_eq!(resolved.version_of(other), Some(version(3)));
    assert!(resolved
        .rules_applied()
        .contains(&Rule::IndependentFilesMerge));
}

// ---------------------------------------------------------------------------------------------
// Row 4 — Non-overlapping text changes → Attempt three-way merge
// ---------------------------------------------------------------------------------------------

/// Fails if two edits to different lines of one file do not combine.
///
/// The mutation this kills: send every multi-write object to preserve-both. The merged text is
/// asserted line by line, so a rule that preserved instead of merging leaves `Disposition::Merged`
/// absent and the assertion fails on the disposition, not only on the review flag.
#[test]
fn row_4_edits_to_different_lines_of_one_file_combine() {
    let (base, _, _, notes) = base_with_text();
    let changes = [
        by(
            1,
            at(5, 1),
            Effect::WriteText {
                object: notes,
                version: version(2),
                lines: lines(&["ONE", "two", "three", "four", "five"]),
            },
        ),
        by(
            2,
            at(5, 2),
            Effect::WriteText {
                object: notes,
                version: version(3),
                lines: lines(&["one", "two", "three", "four", "FIVE"]),
            },
        ),
    ];
    let resolved = resolve(&base, &changes);

    assert!(!resolved.needs_review());
    let merged = resolved
        .outcomes()
        .iter()
        .find(|outcome| outcome.rule() == Rule::ThreeWayTextMerge)
        .expect("a non-overlapping pair of text edits is row four");
    assert_eq!(
        merged.disposition(),
        &Disposition::Merged {
            lines: lines(&["ONE", "two", "three", "four", "FIVE"]),
            from: vec![version(2), version(3)],
        }
    );
    // Both inputs stay reachable even though the merge succeeded.
    assert!(resolved.reachable_versions().contains(&version(2)));
    assert!(resolved.reachable_versions().contains(&version(3)));
}

// ---------------------------------------------------------------------------------------------
// Row 5 — Overlapping text changes → Preserve multiple versions
// ---------------------------------------------------------------------------------------------

/// Fails if two edits to *the same line* are combined by picking one.
///
/// This is the failure that matters most in the whole crate: a resolution that loses an actor's
/// work silently. The mutation this kills: make `three_way` fall through to last-writer-wins, or
/// make `Hunk::overlaps` return false for equal base ranges. Either leaves a clean merge whose
/// text is one actor's and whose review flag is false.
#[test]
fn row_5_edits_to_one_line_preserve_every_version_and_surface_for_review() {
    let (base, _, _, notes) = base_with_text();
    let changes = [
        by(
            1,
            at(5, 1),
            Effect::WriteText {
                object: notes,
                version: version(2),
                lines: lines(&["one", "MINE", "three", "four", "five"]),
            },
        ),
        by(
            2,
            at(5, 2),
            Effect::WriteText {
                object: notes,
                version: version(3),
                lines: lines(&["one", "THEIRS", "three", "four", "five"]),
            },
        ),
    ];
    let resolved = resolve(&base, &changes);

    assert!(resolved.needs_review());
    assert_eq!(resolved.version_of(notes), None, "no version was picked");
    let preserved = resolved
        .outcomes()
        .iter()
        .find(|outcome| outcome.rule() == Rule::PreserveOverlappingText)
        .expect("two edits to one line are row five");
    assert_eq!(
        preserved.disposition(),
        &Disposition::PreservedVersions {
            versions: vec![version(2), version(3)]
        }
    );
    assert_eq!(
        resolved.reachable_versions(),
        &BTreeSet::from([version(1), version(2), version(3)])
    );
}

// ---------------------------------------------------------------------------------------------
// Row 6 — Binary changes → Preserve both versions
// ---------------------------------------------------------------------------------------------

/// Fails if two concurrent byte writes resolve to one.
///
/// The mutation this kills: order the two writes by stamp and keep the later. The test asserts
/// that neither version is reported as current and that both are reachable, so a last-writer-wins
/// resolution fails on the first assertion rather than only on the review flag.
#[test]
fn row_6_two_binary_writes_preserve_both_versions() {
    let root = object(0);
    let image = object(1);
    let base = Snapshot::new(root).with_file(
        image,
        root,
        name("diagram.png"),
        Content::Binary {
            version: version(1),
            digest: [1; 32],
            byte_length: 100,
        },
    );

    let changes = [
        by(
            1,
            at(5, 1),
            Effect::WriteBinary {
                object: image,
                version: version(2),
                digest: [2; 32],
                byte_length: 120,
            },
        ),
        by(
            2,
            at(5, 2),
            Effect::WriteBinary {
                object: image,
                version: version(3),
                digest: [3; 32],
                byte_length: 140,
            },
        ),
    ];
    let resolved = resolve(&base, &changes);

    assert!(resolved.needs_review());
    assert_eq!(resolved.version_of(image), None);
    let preserved = resolved
        .outcomes()
        .iter()
        .find(|outcome| outcome.rule() == Rule::PreserveBinaryVersions)
        .expect("two byte writes are row six");
    assert_eq!(
        preserved.disposition(),
        &Disposition::PreservedVersions {
            versions: vec![version(2), version(3)]
        }
    );
    assert_eq!(
        resolved.reachable_versions(),
        &BTreeSet::from([version(1), version(2), version(3)])
    );
}

// ---------------------------------------------------------------------------------------------
// Row 7 — Delete + edit → Preserve tombstone and edited version
// ---------------------------------------------------------------------------------------------

/// Fails if a removal that is later in the total order swallows a concurrent edit.
///
/// The removal here carries the *higher* stamp, so any implementation that resolves by order
/// rather than by rule drops the edit. The mutation this kills: apply `Delete` as a removal of the
/// object's versions, or skip row seven when the removal wins the tiebreak.
#[test]
fn row_7_a_removal_keeps_both_the_tombstone_and_the_concurrent_edit() {
    let (base, _, _, notes) = base_with_text();
    let changes = [
        by(
            1,
            at(5, 1),
            Effect::WriteText {
                object: notes,
                version: version(2),
                lines: lines(&["one", "EDITED", "three", "four", "five"]),
            },
        ),
        by(2, at(5, 9), Effect::Delete { object: notes }),
    ];
    let resolved = resolve(&base, &changes);

    assert!(resolved.tombstoned().contains(&notes));
    assert!(resolved.needs_review());
    let outcome = resolved
        .outcomes()
        .iter()
        .find(|outcome| outcome.rule() == Rule::PreserveTombstoneAndEdit)
        .expect("a removal concurrent with an edit is row seven");
    assert_eq!(
        outcome.disposition(),
        &Disposition::TombstonedAndPreserved {
            versions: vec![version(2)]
        }
    );
    assert_eq!(
        resolved.reachable_versions(),
        &BTreeSet::from([version(1), version(2)])
    );
}

// ---------------------------------------------------------------------------------------------
// Row 8 — Same-name create → Retain both object IDs; expose naming conflict
// ---------------------------------------------------------------------------------------------

/// Fails if one of two same-name creates is dropped.
///
/// The mutation this kills: `or_insert` on the directory entry, keeping the first create and
/// silently discarding the second — which reads like idempotence and is data loss. Both objects
/// are asserted to have a path, which no single-entry model can satisfy.
#[test]
fn row_8_two_creates_of_one_name_keep_two_objects_and_expose_the_collision() {
    let root = object(0);
    let base = Snapshot::new(root);
    let mine = ObjectId::from_bytes([0xa1; 16]);
    let theirs = ObjectId::from_bytes([0xb2; 16]);

    let changes = [
        by(
            1,
            at(5, 1),
            Effect::Create {
                object: mine,
                kind: ObjectKind::File,
                directory: root,
                name: name("plan.md"),
            },
        ),
        by(
            2,
            at(5, 2),
            Effect::Create {
                object: theirs,
                kind: ObjectKind::File,
                directory: root,
                name: name("plan.md"),
            },
        ),
    ];
    let resolved = resolve(&base, &changes);

    assert_eq!(resolved.path_of(mine).unwrap(), "/plan.md");
    assert_eq!(resolved.path_of(theirs).unwrap(), "/plan~b2b2b2b2.md");
    assert!(resolved.needs_review());

    let collisions = resolved.tree().name_collisions();
    assert_eq!(collisions.len(), 1);
    assert_eq!(collisions[0].kept(), mine);
    assert_eq!(collisions[0].name(), &name("plan.md"));

    let outcome = resolved
        .outcomes()
        .iter()
        .find(|outcome| outcome.rule() == Rule::RetainBothIdentities)
        .expect("two creates of one name are row eight");
    assert_eq!(outcome.object(), theirs);
}

// ---------------------------------------------------------------------------------------------
// Row 9 — Concurrent cyclic directory moves → Deterministic cycle-free resolution
// ---------------------------------------------------------------------------------------------

/// Fails if a cyclic pair of moves produces a cycle, or produces different trees on two peers.
///
/// The mutation this kills: apply moves in arrival order, or drop the ancestor walk in
/// `reparent_refusal`. The first makes the two orders disagree; the second makes `has_cycle` true
/// and every path in the cycle underivable.
#[test]
fn row_9_cyclic_directory_moves_resolve_to_one_acyclic_tree_on_every_peer() {
    let root = object(0);
    let alpha = object(1);
    let beta = object(2);
    let base = Snapshot::new(root)
        .with_directory(alpha, root, name("alpha"))
        .with_directory(beta, root, name("beta"));

    let ido = by(
        1,
        at(5, 1),
        Effect::Reparent {
            object: alpha,
            directory: beta,
        },
    );
    let agent = by(
        2,
        at(5, 2),
        Effect::Reparent {
            object: beta,
            directory: alpha,
        },
    );

    let one_peer = resolve(&base, &[ido.clone(), agent.clone()]);
    let other_peer = resolve(&base, &[agent, ido]);

    assert_eq!(
        one_peer, other_peer,
        "two peers, two orders, one resolution"
    );
    assert!(!one_peer.tree().has_cycle());
    assert_eq!(one_peer.path_of(alpha).unwrap(), "/beta/alpha");
    assert_eq!(one_peer.path_of(beta).unwrap(), "/beta");

    let refused = one_peer.tree().refused_moves();
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0].object(), beta);
    assert_eq!(refused[0].kept_directory(), Some(root));

    let outcome = one_peer
        .outcomes()
        .iter()
        .find(|outcome| outcome.rule() == Rule::DeterministicCycleBreak)
        .expect("a cyclic pair of moves is row nine");
    assert_eq!(
        outcome.disposition(),
        &Disposition::PlacementRefused {
            attempted_directory: alpha,
            kept_directory: Some(root),
        }
    );
}

/// A refused move drops no content. Row nine is the only row exempt from the preservation
/// promise, and this is what that exemption is allowed to cost.
#[test]
fn row_9_a_refused_move_loses_no_version() {
    let root = object(0);
    let alpha = object(1);
    let beta = object(2);
    let inside = object(3);
    let base = Snapshot::new(root)
        .with_directory(alpha, root, name("alpha"))
        .with_directory(beta, root, name("beta"))
        .with_file(
            inside,
            beta,
            name("held.md"),
            Content::Text {
                version: version(7),
                lines: lines(&["kept"]),
            },
        );

    let changes = [
        by(
            1,
            at(5, 1),
            Effect::Reparent {
                object: alpha,
                directory: beta,
            },
        ),
        by(
            2,
            at(5, 2),
            Effect::Reparent {
                object: beta,
                directory: alpha,
            },
        ),
    ];
    let resolved = resolve(&base, &changes);

    assert!(resolved.reachable_versions().contains(&version(7)));
    assert_eq!(resolved.path_of(inside).unwrap(), "/beta/held.md");
    assert!(resolved.tombstoned().is_empty());
}

// ---------------------------------------------------------------------------------------------
// Row 10 — Canonical head changed after review → replay safely or require re-review
// ---------------------------------------------------------------------------------------------

/// Fails if an approval is published over a head that moved across what it touched.
///
/// The mutation this kills: return `ReplaySafe` whenever the overlapping changes would merge. That
/// publishes something no person read, which is what plan §4.7's approval envelope exists to
/// prevent.
#[test]
fn row_10_a_head_that_moved_over_the_review_requires_another_look() {
    let reviewed = HeadId::from_bytes([1; 32]);
    let current = HeadId::from_bytes([2; 32]);
    let touched = BTreeSet::from([object(1), object(2)]);

    let elsewhere = head_movement(reviewed, current, &touched, &BTreeSet::from([object(9)]));
    assert_eq!(elsewhere, HeadMovement::ReplaySafe);
    assert!(elsewhere.may_publish());

    let across = head_movement(
        reviewed,
        current,
        &touched,
        &BTreeSet::from([object(2), object(9)]),
    );
    assert_eq!(
        across,
        HeadMovement::RequiresReReview {
            contested: vec![object(2)]
        }
    );
    assert!(!across.may_publish());
    assert_eq!(across.rule(), Rule::HeadMovedAfterReview);
}

// ---------------------------------------------------------------------------------------------
// Row 11 — Context input changed → Mark affected outputs stale
// ---------------------------------------------------------------------------------------------

/// Fails if a changed input leaves its outputs unmarked, or if staleness stops at the first hop.
///
/// The mutation this kills: drop the transitive closure. A chain of three outputs then reports one
/// stale file and two current ones, which is a worse answer than reporting nothing — a person
/// reading it concludes the other two were checked.
#[test]
fn row_11_a_changed_input_marks_every_output_downstream_of_it_stale() {
    let source = object(1);
    let first = object(2);
    let second = object(3);
    let unrelated = object(4);

    let ledger = ContextLedger::new()
        .with_read(first, source, version(1))
        .with_read(second, first, version(2))
        .with_read(unrelated, object(9), version(9));

    let unchanged: BTreeMap<ObjectId, VersionId> = BTreeMap::from([
        (source, version(1)),
        (first, version(2)),
        (object(9), version(9)),
    ]);
    assert!(ledger.stale_outputs(&unchanged).is_empty());

    let moved_on: BTreeMap<ObjectId, VersionId> = BTreeMap::from([
        (source, version(5)),
        (first, version(2)),
        (object(9), version(9)),
    ]);
    assert_eq!(
        ledger.stale_outputs(&moved_on),
        BTreeSet::from([first, second]),
        "staleness travels the chain and stops where the chain stops"
    );
    assert_eq!(ledger.rule(), Rule::ContextInputChanged);
}

// ---------------------------------------------------------------------------------------------
// The table itself
// ---------------------------------------------------------------------------------------------

/// Every row of plan §4.8 has a test in this file.
///
/// A twelfth row added to [`Rule::TABLE`] without a test here fails this. The contract calls a
/// conflict case with no rule a specification gap; a rule with no test is the same gap one step
/// later.
#[test]
fn row_coverage_is_the_whole_table() {
    assert_eq!(COVERED.len(), Rule::TABLE.len());
    for rule in Rule::TABLE {
        assert!(
            COVERED.contains(&rule),
            "plan §4.8 row {rule} has no targeted test in tests/conflicts.rs"
        );
    }
}

/// The table's text is the plan's text.
///
/// Transcription, not paraphrase. If somebody edits a row's wording here it should be because plan
/// §4.8 changed, and this test is where that gets noticed.
#[test]
fn the_table_reads_as_the_plan_prints_it() {
    let printed: Vec<String> = Rule::TABLE.iter().map(ToString::to_string).collect();
    assert_eq!(
        printed,
        vec![
            "Rename + edit same object → Edit follows stable object ID",
            "Move directory + child edit → Child remains attached to object graph",
            "Independent files → Merge automatically",
            "Non-overlapping text changes → Attempt three-way merge",
            "Overlapping text changes → Preserve multiple versions",
            "Binary changes → Preserve both versions",
            "Delete + edit → Preserve tombstone and edited version",
            "Same-name create → Retain both object IDs; expose naming conflict",
            "Concurrent cyclic directory moves → Deterministic cycle-free resolution",
            "Canonical head changed after review → Replay safely or require re-review",
            "Context input changed → Mark affected outputs stale",
        ]
    );
}
