//! What a resolution says happened, and the promise it carries.
//!
//! # `reachable_versions` is the contract
//!
//! Plan §2.5: no valid work is silently discarded. Plan §4.1: every durable version remains
//! reachable until an explicit retention policy permits deletion — and no retention policy exists
//! yet, so on this tree the qualifier is vacuous and the floor is *everything*.
//!
//! [`Resolution::reachable_versions`] is where that is made checkable. It is seeded with every
//! version the base held and every version any change wrote, **before** any rule runs, and no rule
//! can remove from it — nothing in this crate has a method that would.
//! `tests/preservation.rs` generates operation sets and interleavings and asserts the set equality
//! rather than trusting the sentence.
//!
//! A resolution that merges two versions into one still carries both inputs there. An automatic
//! merge is not permission to forget what it merged.

use std::collections::{BTreeMap, BTreeSet};

use crate::ids::{ActorId, ObjectId, VersionId};
use crate::name::NormalizedName;
use crate::rules::Rule;
use crate::tree::TreeResolution;

/// What happened to one object under one rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Disposition {
    /// The change landed as written, producing this version where it produced one.
    Applied {
        /// The version the object now holds.
        version: Option<VersionId>,
    },
    /// Concurrent text edits were combined. The merged lines, and the versions they came from.
    ///
    /// The merged content has no version identifier here: minting one means hashing it, and this
    /// crate declares no dependency and so has no hash. The caller that writes the merge is the
    /// one that names it.
    Merged {
        /// The combined text.
        lines: Vec<String>,
        /// Every version that went into it. All of them stay reachable.
        from: Vec<VersionId>,
    },
    /// Concurrent versions could not be combined and are all kept.
    PreservedVersions {
        /// Every kept version, in identifier order.
        versions: Vec<VersionId>,
    },
    /// A removal and one or more edits happened concurrently. Both the tombstone and the edits are
    /// kept.
    TombstonedAndPreserved {
        /// Every version written concurrently with the removal.
        versions: Vec<VersionId>,
    },
    /// The object was kept but hangs under a different name.
    Renamed {
        /// The name it hangs under now.
        name: NormalizedName,
    },
    /// A move did not land. What it aimed at, and what the object kept.
    PlacementRefused {
        /// The directory the move aimed at.
        attempted_directory: ObjectId,
        /// The directory the object hangs in instead.
        kept_directory: Option<ObjectId>,
    },
}

impl Disposition {
    /// Every version this disposition names.
    #[must_use]
    pub fn versions(&self) -> Vec<VersionId> {
        match self {
            Self::Applied { version } => version.iter().copied().collect(),
            Self::Merged { from, .. } => from.clone(),
            Self::PreservedVersions { versions } | Self::TombstonedAndPreserved { versions } => {
                versions.clone()
            }
            Self::Renamed { .. } | Self::PlacementRefused { .. } => Vec::new(),
        }
    }
}

/// One row of the table, fired on one object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    rule: Rule,
    object: ObjectId,
    actors: Vec<ActorId>,
    disposition: Disposition,
}

impl Outcome {
    /// The outcome with these parts.
    #[must_use]
    pub fn new(
        rule: Rule,
        object: ObjectId,
        actors: Vec<ActorId>,
        disposition: Disposition,
    ) -> Self {
        let mut actors = actors;
        actors.sort_unstable();
        actors.dedup();
        Self {
            rule,
            object,
            actors,
            disposition,
        }
    }

    /// Which row of plan §4.8 fired.
    #[must_use]
    pub const fn rule(&self) -> Rule {
        self.rule
    }

    /// The object it fired on.
    #[must_use]
    pub const fn object(&self) -> ObjectId {
        self.object
    }

    /// Whose work took part, in identifier order.
    ///
    /// Attribution only. Nothing in this crate resolves differently because of who authored a
    /// change, which is what stops a resolution depending on a peer's view of identity.
    #[must_use]
    pub fn actors(&self) -> &[ActorId] {
        &self.actors
    }

    /// What happened.
    #[must_use]
    pub const fn disposition(&self) -> &Disposition {
        &self.disposition
    }

    /// Whether a person has to look at this before it can be published.
    #[must_use]
    pub const fn needs_review(&self) -> bool {
        self.rule.needs_review()
    }
}

/// Everything one operation set resolves to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolution {
    tree: TreeResolution,
    outcomes: Vec<Outcome>,
    reachable: BTreeSet<VersionId>,
    tombstoned: BTreeSet<ObjectId>,
    holds: BTreeMap<ObjectId, VersionId>,
}

impl Resolution {
    /// Assemble a resolution. Used by [`resolve`](crate::resolve) and by nothing else.
    pub(crate) fn new(
        tree: TreeResolution,
        outcomes: Vec<Outcome>,
        reachable: BTreeSet<VersionId>,
        tombstoned: BTreeSet<ObjectId>,
        holds: BTreeMap<ObjectId, VersionId>,
    ) -> Self {
        Self {
            tree,
            outcomes,
            reachable,
            tombstoned,
            holds,
        }
    }

    /// Where every object ended up: acyclic, total, identical on every peer.
    #[must_use]
    pub const fn tree(&self) -> &TreeResolution {
        &self.tree
    }

    /// Every row that fired, in object then rule order.
    #[must_use]
    pub fn outcomes(&self) -> &[Outcome] {
        &self.outcomes
    }

    /// Every durable version this resolution can still reach.
    ///
    /// The promise: this is a superset of every version the base held and every version any change
    /// wrote, whatever the rules decided. A resolution that could not say this would be a
    /// resolution that lost somebody's work.
    #[must_use]
    pub const fn reachable_versions(&self) -> &BTreeSet<VersionId> {
        &self.reachable
    }

    /// Every object a removal applied to.
    ///
    /// A tombstone marks the object gone from the working tree. The versions it held are still in
    /// [`Resolution::reachable_versions`]; that is the difference between removing a file and
    /// destroying work.
    #[must_use]
    pub const fn tombstoned(&self) -> &BTreeSet<ObjectId> {
        &self.tombstoned
    }

    /// The single version an object settled on, where the rules produced one.
    ///
    /// Absent where the outcome preserved several: an object with an unresolved conflict does not
    /// have one current version, and reporting one would be the silent pick the table forbids.
    #[must_use]
    pub fn version_of(&self, object: ObjectId) -> Option<VersionId> {
        self.holds.get(&object).copied()
    }

    /// Whether anything here has to reach a person before it can be published.
    #[must_use]
    pub fn needs_review(&self) -> bool {
        self.outcomes.iter().any(Outcome::needs_review)
    }

    /// Only the outcomes a person has to look at.
    #[must_use]
    pub fn review_items(&self) -> Vec<&Outcome> {
        self.outcomes
            .iter()
            .filter(|outcome| outcome.needs_review())
            .collect()
    }

    /// Which rows of the table fired at all.
    #[must_use]
    pub fn rules_applied(&self) -> BTreeSet<Rule> {
        self.outcomes.iter().map(Outcome::rule).collect()
    }

    /// The path this object ends up at.
    #[must_use]
    pub fn path_of(&self, object: ObjectId) -> Option<String> {
        self.tree.path_of(object)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_disposition_reports_the_versions_it_names() {
        let one = VersionId::from_bytes([1; 32]);
        let two = VersionId::from_bytes([2; 32]);
        assert_eq!(
            Disposition::PreservedVersions {
                versions: vec![one, two]
            }
            .versions(),
            vec![one, two]
        );
        assert_eq!(
            Disposition::Merged {
                lines: vec![],
                from: vec![one]
            }
            .versions(),
            vec![one]
        );
        assert_eq!(Disposition::Applied { version: None }.versions(), vec![]);
        assert_eq!(
            Disposition::PlacementRefused {
                attempted_directory: ObjectId::from_bytes([1; 16]),
                kept_directory: None
            }
            .versions(),
            vec![]
        );
    }

    #[test]
    fn an_outcome_deduplicates_and_orders_its_actors() {
        let outcome = Outcome::new(
            Rule::IndependentFilesMerge,
            ObjectId::from_bytes([1; 16]),
            vec![
                ActorId::from_bytes([2; 32]),
                ActorId::from_bytes([1; 32]),
                ActorId::from_bytes([2; 32]),
            ],
            Disposition::Applied { version: None },
        );
        assert_eq!(
            outcome.actors(),
            &[ActorId::from_bytes([1; 32]), ActorId::from_bytes([2; 32])]
        );
    }

    #[test]
    fn review_items_are_the_outcomes_whose_row_needs_review() {
        let object = ObjectId::from_bytes([1; 16]);
        let automatic = Outcome::new(
            Rule::IndependentFilesMerge,
            object,
            vec![],
            Disposition::Applied { version: None },
        );
        let reviewed = Outcome::new(
            Rule::PreserveBinaryVersions,
            object,
            vec![],
            Disposition::PreservedVersions { versions: vec![] },
        );
        let resolution = Resolution::new(
            crate::tree::resolve_tree(&crate::snapshot::Snapshot::new(object), &[]),
            vec![automatic, reviewed.clone()],
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeMap::new(),
        );
        assert!(resolution.needs_review());
        assert_eq!(resolution.review_items(), vec![&reviewed]);
    }
}
