//! The order an operation set is applied in, and why it is not the order it arrived in.
//!
//! # The order is a function of the set
//!
//! SG-1: *materializing the same causal set on any peer, in any delivery order, produces the same
//! state hash*. That is only reachable if the applied order is derived from the set rather than
//! observed, so [`causal_order`] sorts by **causal depth, then ChangeSet identifier** — depth is
//! zero for a ChangeSet with no causal parent in the set and one more than its deepest parent
//! otherwise, and ties break on the thirty-two-byte identifier, which is a content digest.
//!
//! That is this protocol's spelling of `lamport → content-hash`, and it is the same rule
//! `crates/mesh-state/src/advance.rs` applies to head advancement — deliberately, because a head
//! and the state it names must be derived from the same sequence or the two would disagree about
//! what "current" means. `tests/vocabulary_drift.rs` reads that file and fails if the rule stops
//! being stated there.
//!
//! **Wall-clock time is not a tiebreaker and is not read.** No hybrid logical time reaches this
//! crate at all: [`AppliedChangeSet`] has no field for one, so a materialization that ordered by a
//! clock is not expressible rather than merely discouraged. `tests/no_ambient_io.rs` scans this
//! crate's own source for the filesystem, the network, the process table and the clock.
//!
//! # Depth is computed by a bounded traversal, so a malformed set still terminates
//!
//! Causal parents are a DAG — `mesh-state` refuses a ChangeSet that is its own parent before one
//! can be applied. This crate does not get to assume that, because it may be handed a set by a
//! caller that skipped that check. [`causal_order`] therefore uses a topological pass and gives
//! depth zero to any ChangeSet left in a cycle, which is deterministic, terminating, and stated
//! rather than discovered.
//!
//! A causal parent naming a ChangeSet outside the set is treated as depth zero, exactly as
//! `mesh-state` treats a parent it has not applied. A set that is not causally closed is still
//! materialized, because refusing to materialize is not an option a total function has.
//!
//! # Two records under one identifier
//!
//! A [`ChangeSetId`] is a content digest, so two records carrying one identifier and different
//! content cannot both be authentic — one of them is a lie, a corruption or a collision. This crate
//! has no bytes and no signature and cannot tell which, and `mesh-state` states the same ceiling:
//! *a caller that hands this crate an identifier it did not verify gets a state derived from
//! records it did not verify.* Verification belongs at the boundary that has the bytes.
//!
//! What materialization must not do is let **arrival order** decide, because then the two peers
//! that received the pair in different orders would hold different states and neither would know.
//! So [`canonical_records`] compares normalized causal parents and then the canonical bytes of the
//! ordered operation sequence. This is a protocol-derived rule every peer computes identically;
//! it does not create a public Rust ordering contract for [`Operation`].

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use crate::ids::ChangeSetId;
use crate::operation::Operation;
use mesh_operations::encode_operations;

/// One ChangeSet's contribution to a workspace state: its identity, its causal parents and its
/// operations, in the order its author sealed them.
///
/// This is the projection of `mesh_operations::ChangeSet` that materialization reads, and it is
/// deliberately narrower than that record. There is no actor, no session, no signature, no policy
/// epoch and **no hybrid logical time**: none of them may influence a state, and a field that is
/// present is a field something will eventually read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppliedChangeSet {
    id: ChangeSetId,
    causal_parents: Vec<ChangeSetId>,
    operations: Vec<Operation>,
}

impl AppliedChangeSet {
    /// A ChangeSet's contribution. Causal parents are sorted and deduplicated on the way in, so two
    /// spellings of one parent set are one value.
    #[must_use]
    pub fn new(
        id: ChangeSetId,
        mut causal_parents: Vec<ChangeSetId>,
        operations: Vec<Operation>,
    ) -> Self {
        causal_parents.sort_unstable();
        causal_parents.dedup();
        Self {
            id,
            causal_parents,
            operations,
        }
    }

    /// A ChangeSet with no causal parent.
    #[must_use]
    pub fn genesis(id: ChangeSetId, operations: Vec<Operation>) -> Self {
        Self::new(id, Vec::new(), operations)
    }

    /// Its identifier.
    #[must_use]
    pub const fn id(&self) -> ChangeSetId {
        self.id
    }

    /// Its causal parents, sorted.
    #[must_use]
    pub fn causal_parents(&self) -> &[ChangeSetId] {
        &self.causal_parents
    }

    /// Its operations, in the order its author sealed them.
    #[must_use]
    pub fn operations(&self) -> &[Operation] {
        &self.operations
    }
}

/// The order a set of ChangeSets is applied in: causal depth, then identifier.
///
/// A pure function of the set. Shuffling the input cannot change the output, and
/// `tests/order_insensitivity.rs` asserts that over the generated corpus rather than over an
/// example.
///
/// A ChangeSet appearing twice under one identifier contributes once. Which record wins is decided
/// by a rule, never by arrival order — see the module header, "two records under one identifier".
///
/// ```
/// use mesh_materializer::{causal_order, AppliedChangeSet, ChangeSetId};
///
/// let first = AppliedChangeSet::genesis(ChangeSetId::from_bytes([9; 32]), vec![]);
/// let second = AppliedChangeSet::new(
///     ChangeSetId::from_bytes([1; 32]),
///     vec![first.id()],
///     vec![],
/// );
///
/// // The child sorts after the parent even though its identifier is lower, and even though it
/// // was delivered first.
/// assert_eq!(
///     causal_order(&[second.clone(), first.clone()]),
///     vec![first.id(), second.id()]
/// );
/// ```
#[must_use]
pub fn causal_order(set: &[AppliedChangeSet]) -> Vec<ChangeSetId> {
    let mut ranked: Vec<(u64, ChangeSetId)> =
        depths(set).into_iter().map(|(id, d)| (d, id)).collect();
    ranked.sort_unstable();
    ranked.into_iter().map(|(_, id)| id).collect()
}

/// One record per identifier, chosen by a rule rather than by arrival order.
///
/// See the module header. Two records under one identifier contradict each other; the least one
/// wins, and the choice is a pure function of the set.
pub(crate) fn canonical_records(
    set: &[AppliedChangeSet],
) -> BTreeMap<ChangeSetId, &AppliedChangeSet> {
    let mut records: BTreeMap<ChangeSetId, &AppliedChangeSet> = BTreeMap::new();
    for changeset in set {
        records
            .entry(changeset.id())
            .and_modify(|held| {
                if canonical_record_cmp(changeset, held).is_lt() {
                    *held = changeset;
                }
            })
            .or_insert(changeset);
    }
    records
}

/// Compare contradictory records without imposing Rust ordering on the operation vocabulary.
fn canonical_record_cmp(left: &AppliedChangeSet, right: &AppliedChangeSet) -> Ordering {
    left.causal_parents
        .cmp(&right.causal_parents)
        .then_with(|| {
            encode_operations(&left.operations).cmp(&encode_operations(&right.operations))
        })
}

/// The causal depth of every ChangeSet in the set.
///
/// A topological pass over the parent edges that stay inside the set. A ChangeSet **settles** when
/// every one of its in-set causal parents has settled, and only then does it take a depth; anything
/// that never settles — a ChangeSet in a causal cycle, or one downstream of a cycle, both of which
/// are malformed input — keeps depth zero.
///
/// The "only on settling" part is what makes this reproducible by a different algorithm. A
/// traversal that wrote a partial depth into an unsettled ChangeSet as each parent was processed
/// would leave a value that depends on how far round the cycle the traversal got, and two
/// implementations would have to agree about that to agree about the order.
pub(crate) fn depths(set: &[AppliedChangeSet]) -> BTreeMap<ChangeSetId, u64> {
    let records = canonical_records(set);
    let present: BTreeSet<ChangeSetId> = records.keys().copied().collect();
    let mut depth: BTreeMap<ChangeSetId, u64> = present.iter().map(|id| (*id, 0)).collect();
    let mut accumulated: BTreeMap<ChangeSetId, u64> = present.iter().map(|id| (*id, 0)).collect();
    let mut waiting: BTreeMap<ChangeSetId, usize> = BTreeMap::new();
    let mut children: BTreeMap<ChangeSetId, Vec<ChangeSetId>> = BTreeMap::new();

    for changeset in records.values() {
        let parents: Vec<ChangeSetId> = changeset
            .causal_parents()
            .iter()
            .copied()
            .filter(|parent| present.contains(parent) && *parent != changeset.id())
            .collect();
        waiting.insert(changeset.id(), parents.len());
        for parent in parents {
            children.entry(parent).or_default().push(changeset.id());
        }
    }

    // A `BTreeSet` as the ready queue, so the traversal takes identifiers in a fixed order and the
    // result cannot depend on which ChangeSet happened to become ready first.
    let mut ready: BTreeSet<ChangeSetId> = waiting
        .iter()
        .filter(|(_, remaining)| **remaining == 0)
        .map(|(id, _)| *id)
        .collect();

    while let Some(id) = ready.iter().next().copied() {
        ready.remove(&id);
        let current = accumulated.get(&id).copied().unwrap_or(0);
        depth.insert(id, current);
        let Some(followers) = children.get(&id) else {
            continue;
        };
        for follower in followers.clone() {
            let existing = accumulated.get(&follower).copied().unwrap_or(0);
            accumulated.insert(follower, existing.max(current.saturating_add(1)));
            if let Some(remaining) = waiting.get_mut(&follower) {
                *remaining = remaining.saturating_sub(1);
                if *remaining == 0 {
                    ready.insert(follower);
                }
            }
        }
    }

    depth
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(byte: u8) -> ChangeSetId {
        ChangeSetId::from_bytes([byte; 32])
    }

    fn changeset(byte: u8, parents: &[u8]) -> AppliedChangeSet {
        AppliedChangeSet::new(
            id(byte),
            parents.iter().map(|b| id(*b)).collect(),
            Vec::new(),
        )
    }

    #[test]
    fn the_empty_set_has_an_empty_order() {
        assert!(causal_order(&[]).is_empty());
    }

    #[test]
    fn a_parent_is_never_ordered_after_a_child() {
        let set = [changeset(9, &[]), changeset(1, &[9]), changeset(2, &[1])];
        assert_eq!(causal_order(&set), vec![id(9), id(1), id(2)]);
    }

    #[test]
    fn concurrent_change_sets_break_the_tie_on_the_identifier() {
        let set = [changeset(9, &[]), changeset(3, &[9]), changeset(2, &[9])];
        assert_eq!(causal_order(&set), vec![id(9), id(2), id(3)]);
    }

    #[test]
    fn the_order_does_not_depend_on_the_order_of_the_slice() {
        let forwards = [changeset(9, &[]), changeset(1, &[9]), changeset(2, &[1])];
        let backwards = [changeset(2, &[1]), changeset(1, &[9]), changeset(9, &[])];
        assert_eq!(causal_order(&forwards), causal_order(&backwards));
    }

    #[test]
    fn a_parent_outside_the_set_is_treated_as_depth_zero() {
        // 5 names a parent nobody delivered. It still materializes, at depth zero.
        let set = [changeset(5, &[200]), changeset(3, &[])];
        assert_eq!(causal_order(&set), vec![id(3), id(5)]);
    }

    #[test]
    fn a_causal_cycle_terminates_rather_than_hanging() {
        let set = [changeset(1, &[2]), changeset(2, &[1])];
        assert_eq!(causal_order(&set), vec![id(1), id(2)]);
    }

    #[test]
    fn a_self_parent_terminates() {
        let set = [changeset(1, &[1])];
        assert_eq!(causal_order(&set), vec![id(1)]);
    }

    #[test]
    fn a_diamond_puts_the_merge_below_both_sides() {
        let set = [
            changeset(1, &[]),
            changeset(2, &[1]),
            changeset(3, &[1]),
            changeset(4, &[2, 3]),
        ];
        assert_eq!(causal_order(&set), vec![id(1), id(2), id(3), id(4)]);
        assert_eq!(depths(&set).get(&id(4)), Some(&2));
    }

    #[test]
    fn causal_parents_are_normalized_on_the_way_in() {
        let one = AppliedChangeSet::new(id(1), vec![id(3), id(2), id(3)], Vec::new());
        let other = AppliedChangeSet::new(id(1), vec![id(2), id(3)], Vec::new());
        assert_eq!(one, other);
        assert_eq!(one.causal_parents(), &[id(2), id(3)]);
    }

    #[test]
    fn duplicate_selection_compares_parents_before_canonical_operation_bytes() {
        let operation = Operation::CreateFile {
            object_id: crate::ObjectId::from_bytes([1; 16]),
        };
        let fewer_parents = AppliedChangeSet::new(id(1), vec![id(2)], vec![operation.clone()]);
        let more_parents = AppliedChangeSet::new(id(1), vec![id(2), id(3)], vec![operation]);

        let delivered = [more_parents, fewer_parents.clone()];
        let selected = canonical_records(&delivered);
        assert_eq!(selected.get(&id(1)).copied(), Some(&fewer_parents));
    }

    #[test]
    fn duplicate_selection_uses_variable_length_canonical_operation_bytes() {
        let short = AppliedChangeSet::genesis(
            id(1),
            vec![Operation::LinkDirectoryEntry {
                directory_id: crate::ObjectId::from_bytes([1; 16]),
                name: crate::NormalizedName::new("a").unwrap(),
                object_id: crate::ObjectId::from_bytes([2; 16]),
                version_id: crate::VersionId::from_bytes([3; 32]),
            }],
        );
        let long = AppliedChangeSet::genesis(
            id(1),
            vec![Operation::LinkDirectoryEntry {
                directory_id: crate::ObjectId::from_bytes([1; 16]),
                name: crate::NormalizedName::new("alphabetically-long").unwrap(),
                object_id: crate::ObjectId::from_bytes([2; 16]),
                version_id: crate::VersionId::from_bytes([3; 32]),
            }],
        );
        let short_bytes = encode_operations(short.operations());
        let long_bytes = encode_operations(long.operations());
        let expected = short_bytes.clone().min(long_bytes.clone());
        let forwards = [short.clone(), long.clone()];
        let backwards = [long, short];

        for delivered in [&forwards[..], &backwards[..]] {
            let selected = canonical_records(delivered);
            assert_eq!(
                encode_operations(selected.get(&id(1)).unwrap().operations()),
                expected
            );
        }
    }
}
