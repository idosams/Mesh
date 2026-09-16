//! The engine: one operation set in, one [`Resolution`] out.
//!
//! # Reproducible from the operation set alone
//!
//! [`resolve`] is a pure function of `(base, changes)` and reads nothing else — no clock, no
//! arrival order, no actor preference, no configuration. Every ordering decision inside it comes
//! from [`Stamp`](crate::Stamp), which is `lamport → event ULID → content hash`. That is what the
//! task contract's fifth acceptance criterion asks for and what `tests/determinism.rs` measures by
//! shuffling.
//!
//! # The order the rows are decided in
//!
//! 1. **Placement** — `src/tree.rs` resolves where everything hangs, including rows eight and
//!    nine. It runs first because rows one and two are statements about placement and content
//!    being independent, and you cannot state that until you have both.
//! 2. **Content** — per object, from the base version and the concurrent writes. Rows three
//!    through seven.
//! 3. **Identity observations** — rows one and two, which report that an edit survived a rename
//!    and that a child survived its directory moving.
//!
//! # Which case each content row takes
//!
//! | The object received | The row |
//! |---|---|
//! | one write, no removal | three — merge automatically |
//! | several text writes that do not overlap | four — attempt three-way merge |
//! | several text writes that do overlap | five — preserve multiple versions |
//! | several writes where any side is bytes | six — preserve both versions |
//! | a removal and at least one write | seven — preserve tombstone and edited version |
//!
//! Two things about that table are deliberate and both err the same way. **Any binary side makes
//! the whole object row six**, including a text write concurrent with a binary write and a text
//! write onto a binary base: there is no defensible three-way merge across a representation
//! change, and preserving is always correct where merging is sometimes catastrophic. And **a
//! removal concurrent with a write is never a plain removal**, even when the removal is later in
//! the total order — row seven has no clause about which came first, because the actor who wrote
//! the version did valid work either way.
//!
//! # The case with no rule
//!
//! The contract is explicit that a conflict case with no rule is a specification gap rather than
//! an implementation choice. Two shapes reach that here, and neither is silently resolved: a move
//! onto a non-existent or non-directory target is refused and reported as a
//! [`RefusedMove`](crate::RefusedMove) with a reason that is *not* `WouldFormCycle`, and it fires
//! no rule outcome, because plan §4.8 has no row for it. It is an invalid operation, not a
//! conflict between two valid ones.

use std::collections::{BTreeMap, BTreeSet};

use crate::change::{Change, Effect};
use crate::ids::{ActorId, ObjectId, VersionId};
use crate::object::Content;
use crate::outcome::{Disposition, Outcome, Resolution};
use crate::rules::Rule;
use crate::snapshot::Snapshot;
use crate::text::{three_way, TextMerge};
use crate::tree::{resolve_tree, RefusalReason, TreeResolution};

/// Resolve one set of concurrent changes against the state they departed from.
///
/// The order of `changes` is not read. Two peers holding the same set in any order produce equal
/// resolutions.
#[must_use]
pub fn resolve(base: &Snapshot, changes: &[Change]) -> Resolution {
    let tree = resolve_tree(base, changes);

    let mut reachable: BTreeSet<VersionId> = base.versions().into_iter().collect();
    for change in changes {
        if let Some(version) = change.effect().version() {
            reachable.insert(version);
        }
    }

    let mut by_object: BTreeMap<ObjectId, Vec<&Change>> = BTreeMap::new();
    for change in changes {
        by_object.entry(change.object()).or_default().push(change);
    }
    for grouped in by_object.values_mut() {
        grouped.sort_by_key(|change| (*change.stamp(), change.effect().order_rank()));
    }

    let mut outcomes = Vec::new();
    let mut tombstoned = BTreeSet::new();
    let mut holds = BTreeMap::new();
    for (object, grouped) in &by_object {
        let decided = decide_content(base, *object, grouped);
        if decided.tombstoned {
            tombstoned.insert(*object);
        }
        if let Some(version) = decided.settled {
            holds.insert(*object, version);
        }
        outcomes.extend(decided.outcomes);
    }
    for (object, held) in base.objects() {
        if by_object.contains_key(object) {
            continue;
        }
        if let Some(content) = held.content() {
            holds.insert(*object, content.version());
        }
    }

    outcomes.extend(identity_observations(&tree, &by_object));
    outcomes.extend(placement_outcomes(&tree));
    outcomes.sort_by_key(|outcome| (outcome.object(), outcome.rule()));

    Resolution::new(tree, outcomes, reachable, tombstoned, holds)
}

/// What the content rows decided for one object.
struct ContentDecision {
    outcomes: Vec<Outcome>,
    tombstoned: bool,
    settled: Option<VersionId>,
}

/// Rows three through seven, for one object.
fn decide_content(base: &Snapshot, object: ObjectId, grouped: &[&Change]) -> ContentDecision {
    let writes: Vec<&Change> = grouped
        .iter()
        .copied()
        .filter(|change| change.effect().version().is_some())
        .collect();
    let removed = grouped
        .iter()
        .any(|change| matches!(change.effect(), Effect::Delete { .. }));

    let base_content = base.object(object).and_then(|held| held.content());
    let actors: Vec<ActorId> = grouped.iter().map(|change| change.actor()).collect();

    if writes.is_empty() {
        return ContentDecision {
            outcomes: Vec::new(),
            tombstoned: removed,
            settled: base_content.map(Content::version),
        };
    }

    let mut written: Vec<VersionId> = writes
        .iter()
        .filter_map(|change| change.effect().version())
        .collect();
    written.sort_unstable();
    written.dedup();

    if removed {
        return ContentDecision {
            outcomes: vec![Outcome::new(
                Rule::PreserveTombstoneAndEdit,
                object,
                actors,
                Disposition::TombstonedAndPreserved { versions: written },
            )],
            tombstoned: true,
            settled: None,
        };
    }

    if written.len() == 1 {
        let version = written[0];
        return ContentDecision {
            outcomes: vec![Outcome::new(
                Rule::IndependentFilesMerge,
                object,
                actors,
                Disposition::Applied {
                    version: Some(version),
                },
            )],
            tombstoned: false,
            settled: Some(version),
        };
    }

    if binary_is_involved(base_content, &writes) {
        return ContentDecision {
            outcomes: vec![Outcome::new(
                Rule::PreserveBinaryVersions,
                object,
                actors,
                Disposition::PreservedVersions { versions: written },
            )],
            tombstoned: false,
            settled: None,
        };
    }

    let base_lines: Vec<String> = base_content
        .and_then(Content::lines)
        .map(<[String]>::to_vec)
        .unwrap_or_default();
    let sides: Vec<&[String]> = writes
        .iter()
        .filter_map(|change| match change.effect() {
            Effect::WriteText { lines, .. } => Some(lines.as_slice()),
            _ => None,
        })
        .collect();

    match three_way(&base_lines, &sides) {
        TextMerge::Clean(lines) => ContentDecision {
            outcomes: vec![Outcome::new(
                Rule::ThreeWayTextMerge,
                object,
                actors,
                Disposition::Merged {
                    lines,
                    from: written,
                },
            )],
            tombstoned: false,
            settled: None,
        },
        TextMerge::Overlapping(_) => ContentDecision {
            outcomes: vec![Outcome::new(
                Rule::PreserveOverlappingText,
                object,
                actors,
                Disposition::PreservedVersions { versions: written },
            )],
            tombstoned: false,
            settled: None,
        },
    }
}

/// Whether any side of this object's content is opaque bytes.
///
/// The base counts. A text write onto a binary base is a representation change, and a three-way
/// merge across one produces bytes neither actor wrote.
fn binary_is_involved(base_content: Option<&Content>, writes: &[&Change]) -> bool {
    base_content.is_some_and(Content::is_binary)
        || writes
            .iter()
            .any(|change| matches!(change.effect(), Effect::WriteBinary { .. }))
}

/// Rows one and two: what survived a rename, and what survived its directory moving.
fn identity_observations(
    tree: &TreeResolution,
    by_object: &BTreeMap<ObjectId, Vec<&Change>>,
) -> Vec<Outcome> {
    let mut found = Vec::new();

    for (object, grouped) in by_object {
        let renamed = grouped
            .iter()
            .any(|change| matches!(change.effect(), Effect::Rename { .. }));
        if renamed && grouped.iter().any(|c| c.effect().version().is_some()) {
            found.push(Outcome::new(
                Rule::EditFollowsIdentity,
                *object,
                grouped.iter().map(|change| change.actor()).collect(),
                survived(grouped),
            ));
        }
    }

    let moved: Vec<ObjectId> = by_object
        .iter()
        .filter(|(object, grouped)| {
            grouped
                .iter()
                .any(|change| matches!(change.effect(), Effect::Reparent { .. }))
                && !tree
                    .refused_moves()
                    .iter()
                    .any(|refusal| refusal.object() == **object)
        })
        .map(|(object, _)| *object)
        .collect();

    for (object, grouped) in by_object {
        if !grouped.iter().any(|c| c.effect().version().is_some()) {
            continue;
        }
        if moved
            .iter()
            .any(|directory| is_descendant(tree, *object, *directory))
        {
            found.push(Outcome::new(
                Rule::ChildStaysAttached,
                *object,
                grouped.iter().map(|change| change.actor()).collect(),
                survived(grouped),
            ));
        }
    }

    found
}

/// What rows one and two report about the versions written to one object.
///
/// [`Disposition::Applied`] with a version means *this is what the object holds now*, everywhere in
/// this crate. So an identity observation may only use it when there is one version to hold; where
/// several concurrent writes exist, the content row already decided to preserve them and this row
/// says the same thing rather than naming one of them. Reporting the first would be a silent pick
/// wearing an observation's clothes — `tests/preservation.rs` is what found that, on seed 0.
fn survived(grouped: &[&Change]) -> Disposition {
    let mut written: Vec<VersionId> = grouped
        .iter()
        .filter_map(|change| change.effect().version())
        .collect();
    written.sort_unstable();
    written.dedup();
    match written.len() {
        1 => Disposition::Applied {
            version: Some(written[0]),
        },
        _ => Disposition::PreservedVersions { versions: written },
    }
}

/// Whether `object` hangs somewhere beneath `ancestor` in the resolved tree.
fn is_descendant(tree: &TreeResolution, object: ObjectId, ancestor: ObjectId) -> bool {
    if object == ancestor {
        return false;
    }
    let mut at = object;
    for _ in 0..crate::snapshot::MAX_PATH_DEPTH {
        let Some(placed) = tree.placement(at) else {
            return false;
        };
        if placed.directory() == ancestor {
            return true;
        }
        if placed.directory() == tree.root() {
            return false;
        }
        at = placed.directory();
    }
    false
}

/// Rows eight and nine, read off the resolved tree.
fn placement_outcomes(tree: &TreeResolution) -> Vec<Outcome> {
    let mut found = Vec::new();
    for collision in tree.name_collisions() {
        for (object, name) in collision.renamed() {
            found.push(Outcome::new(
                Rule::RetainBothIdentities,
                *object,
                Vec::new(),
                Disposition::Renamed { name: name.clone() },
            ));
        }
    }
    for refusal in tree.refused_moves() {
        if refusal.reason() != RefusalReason::WouldFormCycle {
            continue;
        }
        found.push(Outcome::new(
            Rule::DeterministicCycleBreak,
            refusal.object(),
            Vec::new(),
            Disposition::PlacementRefused {
                attempted_directory: refusal.attempted_directory(),
                kept_directory: refusal.kept_directory(),
            },
        ));
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::name::NormalizedName;
    use crate::object::ObjectKind;
    use crate::stamp::{EventId, Lamport, Stamp};

    fn name(text: &str) -> NormalizedName {
        NormalizedName::new(text).unwrap()
    }

    fn at(lamport: u64, event: u8) -> Stamp {
        Stamp::new(
            Lamport::new(lamport),
            EventId::from_bytes([event; 16]),
            [0; 32],
        )
    }

    fn by(actor: u8, stamp: Stamp, effect: Effect) -> Change {
        Change::new(stamp, ActorId::from_bytes([actor; 32]), effect)
    }

    fn lines(text: &[&str]) -> Vec<String> {
        text.iter().map(|line| (*line).to_owned()).collect()
    }

    fn one_text_file() -> (Snapshot, ObjectId, ObjectId) {
        let root = ObjectId::from_bytes([0; 16]);
        let notes = ObjectId::from_bytes([1; 16]);
        let base = Snapshot::new(root).with_file(
            notes,
            root,
            name("notes.md"),
            Content::Text {
                version: VersionId::from_bytes([1; 32]),
                lines: lines(&["a", "b", "c"]),
            },
        );
        (base, root, notes)
    }

    #[test]
    fn an_empty_operation_set_changes_nothing_and_loses_nothing() {
        let (base, _, notes) = one_text_file();
        let resolved = resolve(&base, &[]);
        assert!(!resolved.needs_review());
        assert_eq!(resolved.path_of(notes).unwrap(), "/notes.md");
        assert!(resolved
            .reachable_versions()
            .contains(&VersionId::from_bytes([1; 32])));
    }

    #[test]
    fn a_lone_write_lands_automatically() {
        let (base, _, notes) = one_text_file();
        let changes = [by(
            1,
            at(2, 1),
            Effect::WriteText {
                object: notes,
                version: VersionId::from_bytes([2; 32]),
                lines: lines(&["a", "B", "c"]),
            },
        )];
        let resolved = resolve(&base, &changes);
        assert!(!resolved.needs_review());
        assert_eq!(
            resolved.version_of(notes),
            Some(VersionId::from_bytes([2; 32]))
        );
        assert!(resolved
            .rules_applied()
            .contains(&Rule::IndependentFilesMerge));
    }

    #[test]
    fn an_untouched_file_keeps_reporting_its_base_version() {
        let (base, _, notes) = one_text_file();
        let resolved = resolve(&base, &[]);
        assert_eq!(
            resolved.version_of(notes),
            Some(VersionId::from_bytes([1; 32]))
        );
    }

    #[test]
    fn a_write_concurrent_with_a_removal_keeps_both() {
        let (base, _, notes) = one_text_file();
        let changes = [
            by(1, at(2, 1), Effect::Delete { object: notes }),
            by(
                2,
                at(2, 2),
                Effect::WriteText {
                    object: notes,
                    version: VersionId::from_bytes([2; 32]),
                    lines: lines(&["a", "B", "c"]),
                },
            ),
        ];
        let resolved = resolve(&base, &changes);
        assert!(resolved.tombstoned().contains(&notes));
        assert!(resolved
            .reachable_versions()
            .contains(&VersionId::from_bytes([2; 32])));
        assert!(resolved
            .reachable_versions()
            .contains(&VersionId::from_bytes([1; 32])));
        assert!(resolved.needs_review());
    }

    #[test]
    fn an_uncontested_removal_still_keeps_the_version_reachable() {
        let (base, _, notes) = one_text_file();
        let changes = [by(1, at(2, 1), Effect::Delete { object: notes })];
        let resolved = resolve(&base, &changes);
        assert!(resolved.tombstoned().contains(&notes));
        assert!(resolved
            .reachable_versions()
            .contains(&VersionId::from_bytes([1; 32])));
        assert!(!resolved.needs_review());
    }

    #[test]
    fn a_text_write_concurrent_with_a_binary_write_preserves_both() {
        let (base, _, notes) = one_text_file();
        let changes = [
            by(
                1,
                at(2, 1),
                Effect::WriteText {
                    object: notes,
                    version: VersionId::from_bytes([2; 32]),
                    lines: lines(&["a", "B", "c"]),
                },
            ),
            by(
                2,
                at(2, 2),
                Effect::WriteBinary {
                    object: notes,
                    version: VersionId::from_bytes([3; 32]),
                    digest: [9; 32],
                    byte_length: 12,
                },
            ),
        ];
        let resolved = resolve(&base, &changes);
        assert!(resolved
            .rules_applied()
            .contains(&Rule::PreserveBinaryVersions));
        assert_eq!(resolved.version_of(notes), None);
    }

    #[test]
    fn creating_a_second_file_does_not_disturb_the_first() {
        let (base, root, notes) = one_text_file();
        let extra = ObjectId::from_bytes([2; 16]);
        let changes = [
            by(
                1,
                at(2, 1),
                Effect::Create {
                    object: extra,
                    kind: ObjectKind::File,
                    directory: root,
                    name: name("other.md"),
                },
            ),
            by(
                1,
                at(2, 2),
                Effect::WriteText {
                    object: extra,
                    version: VersionId::from_bytes([5; 32]),
                    lines: lines(&["x"]),
                },
            ),
        ];
        let resolved = resolve(&base, &changes);
        assert_eq!(resolved.path_of(notes).unwrap(), "/notes.md");
        assert_eq!(resolved.path_of(extra).unwrap(), "/other.md");
        assert!(!resolved.needs_review());
    }
}
