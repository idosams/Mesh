//! Conflicts, computed into the bundle rather than at review time.
//!
//! # Why they are in the bundle
//!
//! A conflict discovered while a person is reading is a conflict discovered against a state that
//! may already have moved. The approval has to name exact bytes, and "these are the conflicts" is
//! part of what was approved — so it is computed once, absorbed into the bundle's identity, and
//! recomputable by anyone holding the same three states.
//!
//! # The rule, and what it deliberately is not
//!
//! This module answers one question: **did the canonical head move over anything the actor
//! touched?** It is `mesh-conflicts`' row ten (`head_movement`) applied per object, and it is
//! deliberately blunt. It does not merge, does not decide a winner, and does not judge whether two
//! changes would compose — a cleverer rule would mean a person approved one thing and something
//! else was published. Merging is `mesh-conflicts`' work; reporting is this crate's.
//!
//! # Nothing here loses a version
//!
//! Every [`Conflict`] carries [`Conflict::preserved_versions`], seeded from the fork and from both
//! sides before any classification runs. A conflict records that two versions exist; it never
//! decides that one of them stops existing.

use std::collections::{BTreeMap, BTreeSet};

use crate::diff::{Effect, ObjectChange};
use crate::digest::{Absorb, DigestHasher, DigestWriter};
use crate::ids::{ObjectId, VersionId};

/// What a conflict is, in the only two shapes this crate is willing to claim.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Disposition {
    /// One side removed the object and the other changed it. Plan §4.8: preserve the tombstone
    /// *and* the edited version — a removal concurrent with an edit is not a removal.
    TombstonedAndPreserved,
    /// Both sides changed the object and neither removed it. A person decides; both versions stay
    /// reachable until they do.
    Contested,
}

impl Disposition {
    /// The value this disposition is absorbed as.
    const fn tag(self) -> u64 {
        match self {
            Self::TombstonedAndPreserved => 0,
            Self::Contested => 1,
        }
    }

    /// A short, stable word for a surface that renders one.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::TombstonedAndPreserved => "tombstoned-and-preserved",
            Self::Contested => "contested",
        }
    }
}

/// One object both the actor and the canonical head changed since they parted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    object: ObjectId,
    path: Option<String>,
    canonical_effects: Vec<String>,
    actor_effects: Vec<String>,
    preserved_versions: Vec<VersionId>,
    disposition: Disposition,
}

impl Conflict {
    /// The contested object.
    #[must_use]
    pub const fn object(&self) -> ObjectId {
        self.object
    }

    /// Where it is, derived from the actor's state when it exists there and from the canonical
    /// state otherwise. Absent when neither holds it.
    #[must_use]
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// What the canonical head did to it, as effect labels in the order the diff emitted them.
    #[must_use]
    pub fn canonical_effects(&self) -> &[String] {
        &self.canonical_effects
    }

    /// What the actor did to it.
    #[must_use]
    pub fn actor_effects(&self) -> &[String] {
        &self.actor_effects
    }

    /// Every version that must stay reachable whatever the reviewer decides, in identifier order.
    #[must_use]
    pub fn preserved_versions(&self) -> &[VersionId] {
        &self.preserved_versions
    }

    /// Which of the two shapes this conflict has.
    #[must_use]
    pub const fn disposition(&self) -> Disposition {
        self.disposition
    }
}

impl Absorb for Conflict {
    fn absorb<H: DigestHasher>(&self, writer: &mut DigestWriter<H>) {
        writer.bytes(self.object.as_bytes());
        writer.option(self.path.as_ref(), |writer, path| {
            writer.text(path);
        });
        writer.sequence(&self.canonical_effects, |writer, label| {
            writer.text(label);
        });
        writer.sequence(&self.actor_effects, |writer, label| {
            writer.text(label);
        });
        writer.sequence(&self.preserved_versions, |writer, version| {
            writer.bytes(version.as_bytes());
        });
        writer.u64(self.disposition.tag());
    }
}

/// Every object both change lists touched, in identifier order.
///
/// `canonical` is what landed on the canonical head since the actor forked; `actor` is what the
/// actor did. `path_of` supplies the location a reviewer reads, and is passed in rather than
/// derived here so that the caller decides which state a path is read from.
pub(crate) fn conflicts(
    canonical: &[ObjectChange],
    actor: &[ObjectChange],
    path_of: impl Fn(ObjectId) -> Option<String>,
) -> Vec<Conflict> {
    let canonical_by_object = group(canonical);
    let actor_by_object = group(actor);

    let contested: BTreeSet<ObjectId> = canonical_by_object
        .keys()
        .filter(|object| actor_by_object.contains_key(*object))
        .copied()
        .collect();

    contested
        .into_iter()
        .map(|object| {
            let canonical_side = canonical_by_object
                .get(&object)
                .map_or(&[][..], Vec::as_slice);
            let actor_side = actor_by_object.get(&object).map_or(&[][..], Vec::as_slice);
            let removed = removes(canonical_side) || removes(actor_side);
            let disposition = if removed {
                Disposition::TombstonedAndPreserved
            } else {
                Disposition::Contested
            };
            let mut preserved: BTreeSet<VersionId> = BTreeSet::new();
            for change in canonical_side.iter().chain(actor_side.iter()) {
                collect_versions(change.effect(), &mut preserved);
            }
            Conflict {
                object,
                path: path_of(object),
                canonical_effects: labels(canonical_side),
                actor_effects: labels(actor_side),
                preserved_versions: preserved.into_iter().collect(),
                disposition,
            }
        })
        .collect()
}

/// The changes on each object, in the order the diff emitted them.
fn group(changes: &[ObjectChange]) -> BTreeMap<ObjectId, Vec<ObjectChange>> {
    let mut grouped: BTreeMap<ObjectId, Vec<ObjectChange>> = BTreeMap::new();
    for change in changes {
        grouped
            .entry(change.object())
            .or_default()
            .push(change.clone());
    }
    grouped
}

/// Whether one side removed the object.
fn removes(changes: &[ObjectChange]) -> bool {
    changes
        .iter()
        .any(|change| matches!(change.effect(), Effect::Removed { .. }))
}

/// The effect labels of one side, in order.
fn labels(changes: &[ObjectChange]) -> Vec<String> {
    changes
        .iter()
        .map(|change| change.effect().label().to_owned())
        .collect()
}

/// Every version an effect mentions, on either side of it.
fn collect_versions(effect: &Effect, into: &mut BTreeSet<VersionId>) {
    match effect {
        Effect::Created { content, .. } | Effect::Removed { content, .. } => {
            if let Some(content) = content {
                into.insert(content.version());
            }
        }
        Effect::ContentWritten { from, to } => {
            if let Some(from) = from {
                into.insert(from.version());
            }
            into.insert(to.version());
        }
        Effect::ContentCleared { from } => {
            into.insert(from.version());
        }
        Effect::Moved { .. } | Effect::Renamed { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::diff;
    use crate::name::NormalizedName;
    use crate::state::{Content, WorkspaceState};

    fn name(text: &str) -> NormalizedName {
        NormalizedName::new(text).unwrap()
    }

    fn text(byte: u8, line: &str) -> Content {
        Content::Text {
            version: VersionId::from_bytes([byte; 32]),
            lines: vec![line.to_owned()],
        }
    }

    fn root() -> ObjectId {
        ObjectId::from_bytes([0; 16])
    }

    fn notes() -> ObjectId {
        ObjectId::from_bytes([1; 16])
    }

    fn fork() -> WorkspaceState {
        WorkspaceState::new(root()).with_file(notes(), root(), name("notes.md"), text(1, "one"))
    }

    #[test]
    fn two_sides_that_touched_nothing_in_common_conflict_in_nothing() {
        let other = ObjectId::from_bytes([2; 16]);
        let canonical = fork().with_file(other, root(), name("other.md"), text(2, "two"));
        let actor = fork().with_file(notes(), root(), name("notes.md"), text(3, "three"));
        let found = conflicts(
            &diff(&fork(), &canonical).unwrap(),
            &diff(&fork(), &actor).unwrap(),
            |_| None,
        );
        assert!(found.is_empty());
    }

    #[test]
    fn two_writes_to_one_object_are_contested_and_keep_every_version() {
        let canonical = fork().with_file(notes(), root(), name("notes.md"), text(2, "two"));
        let actor = fork().with_file(notes(), root(), name("notes.md"), text(3, "three"));
        let found = conflicts(
            &diff(&fork(), &canonical).unwrap(),
            &diff(&fork(), &actor).unwrap(),
            |_| Some("/notes.md".to_owned()),
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].disposition(), Disposition::Contested);
        assert_eq!(found[0].path(), Some("/notes.md"));
        assert_eq!(
            found[0].preserved_versions(),
            &[
                VersionId::from_bytes([1; 32]),
                VersionId::from_bytes([2; 32]),
                VersionId::from_bytes([3; 32])
            ]
        );
    }

    #[test]
    fn a_removal_against_an_edit_keeps_the_tombstone_and_the_edit() {
        let canonical = fork().without(notes());
        let actor = fork().with_file(notes(), root(), name("notes.md"), text(3, "three"));
        let found = conflicts(
            &diff(&fork(), &canonical).unwrap(),
            &diff(&fork(), &actor).unwrap(),
            |_| None,
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].disposition(), Disposition::TombstonedAndPreserved);
        assert!(found[0]
            .preserved_versions()
            .contains(&VersionId::from_bytes([1; 32])));
        assert!(found[0]
            .preserved_versions()
            .contains(&VersionId::from_bytes([3; 32])));
    }

    #[test]
    fn both_sides_effects_are_reported_and_the_dispositions_are_labelled() {
        let canonical = fork().with_file(notes(), root(), name("moved.md"), text(2, "two"));
        let actor = fork().with_file(notes(), root(), name("notes.md"), text(3, "three"));
        let found = conflicts(
            &diff(&fork(), &canonical).unwrap(),
            &diff(&fork(), &actor).unwrap(),
            |_| None,
        );
        assert_eq!(
            found[0].canonical_effects(),
            &["renamed".to_owned(), "content-written".to_owned()]
        );
        assert_eq!(found[0].actor_effects(), &["content-written".to_owned()]);
        assert_eq!(found[0].disposition().label(), "contested");
        assert_eq!(
            Disposition::TombstonedAndPreserved.label(),
            "tombstoned-and-preserved"
        );
    }
}
