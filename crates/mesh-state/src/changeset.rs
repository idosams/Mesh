//! A ChangeSet as it arrives from a peer — the input to head advancement.
//!
//! This is not `mesh-types`' `ChangeSet`. That type is the authored record with its operations and
//! its signature; this is the projection head advancement actually reads: an identifier, an
//! author, the causal parents, and the two heads the author claims the transition runs between.
//! The operations are the materializer's business and the signature is `mesh-crypto`'s, and a head
//! that read either would be coupled to both.
//!
//! # The clock is carried and never consulted
//!
//! [`DeliveredChangeSet::hybrid_logical_time_millis`] and
//! [`DeliveredChangeSet::hybrid_logical_time_counter`] mirror the two fields `mesh-types`' ChangeSet
//! carries. They are here because a peer sends them and dropping a received field would be lossy —
//! and they are read by exactly nothing in this crate. `docs/protocol.md` §3.3 states the rule as
//! OG-6: hybrid logical time never decides causality and never affects head advancement.
//!
//! Two things hold that rule up rather than restating it. `src/no_ambient_input.rs` is a compile-time
//! scan proving no source file here reaches the system clock at all, and
//! `tests/heads.rs::a_wrong_clock_moves_no_head` delivers the same causal set twice with the two
//! fields set to values a badly-set machine would produce, and requires byte-identical heads.

use crate::ids::{ActorId, ChangeSetId, HeadId};
use crate::parents::CausalParents;

/// One authored transition, as a peer delivers it.
///
/// Immutable: every field is set at construction, and the two builders that exist return new
/// values. A delivered record that could be edited after arrival would make "the head is a
/// function of what was delivered" unfalsifiable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeliveredChangeSet {
    id: ChangeSetId,
    actor: ActorId,
    parents: CausalParents,
    base_head: HeadId,
    resulting_head: HeadId,
    hybrid_logical_time_millis: u64,
    hybrid_logical_time_counter: u32,
}

impl DeliveredChangeSet {
    /// A delivered ChangeSet, with its hybrid logical time at the origin.
    ///
    /// The two heads are what the *author* claims. Nothing here checks them: they are checked
    /// against the receiver's own derivation in [`crate::HeadAdvancement::deliver`], which is the
    /// only place that can, because it is the only place that holds the causal set.
    #[must_use]
    pub const fn new(
        id: ChangeSetId,
        actor: ActorId,
        parents: CausalParents,
        base_head: HeadId,
        resulting_head: HeadId,
    ) -> Self {
        Self {
            id,
            actor,
            parents,
            base_head,
            resulting_head,
            hybrid_logical_time_millis: 0,
            hybrid_logical_time_counter: 0,
        }
    }

    /// The same ChangeSet carrying a hybrid logical time.
    ///
    /// Carried for display and for tiebreaking somewhere else. It is not an input to any head.
    #[must_use]
    pub fn with_hybrid_logical_time(self, millis: u64, counter: u32) -> Self {
        Self {
            hybrid_logical_time_millis: millis,
            hybrid_logical_time_counter: counter,
            ..self
        }
    }

    /// The same ChangeSet claiming different heads.
    ///
    /// Exists so that a test can produce the record a corrupted or dishonest peer would send
    /// without hand-building every field, and so that the refusal path has an adversary that is
    /// constructed rather than imagined.
    #[must_use]
    pub fn claiming(self, base_head: HeadId, resulting_head: HeadId) -> Self {
        Self {
            base_head,
            resulting_head,
            ..self
        }
    }

    /// The same ChangeSet claiming different causal parents.
    ///
    /// The dropped-parent adversary: same identifier, same claimed heads, one parent removed.
    #[must_use]
    pub fn following(self, parents: CausalParents) -> Self {
        Self { parents, ..self }
    }

    /// Its identifier.
    #[must_use]
    pub const fn id(&self) -> ChangeSetId {
        self.id
    }

    /// The actor that authored it.
    #[must_use]
    pub const fn actor(&self) -> ActorId {
        self.actor
    }

    /// What it causally follows.
    #[must_use]
    pub const fn parents(&self) -> &CausalParents {
        &self.parents
    }

    /// The head its author says it was authored against.
    #[must_use]
    pub const fn base_head(&self) -> HeadId {
        self.base_head
    }

    /// The head its author says applying it produces.
    #[must_use]
    pub const fn resulting_head(&self) -> HeadId {
        self.resulting_head
    }

    /// The physical millisecond of its hybrid logical time. Carried, never consulted.
    #[must_use]
    pub const fn hybrid_logical_time_millis(&self) -> u64 {
        self.hybrid_logical_time_millis
    }

    /// The logical counter of its hybrid logical time. Carried, never consulted.
    #[must_use]
    pub const fn hybrid_logical_time_counter(&self) -> u32 {
        self.hybrid_logical_time_counter
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn changeset() -> DeliveredChangeSet {
        DeliveredChangeSet::new(
            ChangeSetId::from_bytes([1; 32]),
            ActorId::from_bytes([2; 32]),
            CausalParents::genesis(),
            HeadId::from_bytes([3; 32]),
            HeadId::from_bytes([4; 32]),
        )
    }

    #[test]
    fn a_new_changeset_carries_the_origin_clock() {
        let delivered = changeset();
        assert_eq!(delivered.hybrid_logical_time_millis(), 0);
        assert_eq!(delivered.hybrid_logical_time_counter(), 0);
    }

    #[test]
    fn every_builder_returns_a_new_value() {
        let original = changeset();
        let clocked = original.clone().with_hybrid_logical_time(u64::MAX, 9);
        assert_eq!(original.hybrid_logical_time_millis(), 0, "input mutated");
        assert_eq!(clocked.hybrid_logical_time_millis(), u64::MAX);
        assert_eq!(clocked.id(), original.id());
        assert_eq!(clocked.parents(), original.parents());
    }

    #[test]
    fn claiming_moves_only_the_two_heads() {
        let original = changeset();
        let lying = original
            .clone()
            .claiming(HeadId::from_bytes([9; 32]), HeadId::from_bytes([8; 32]));
        assert_eq!(lying.id(), original.id());
        assert_eq!(lying.base_head(), HeadId::from_bytes([9; 32]));
        assert_eq!(lying.resulting_head(), HeadId::from_bytes([8; 32]));
        assert_eq!(original.base_head(), HeadId::from_bytes([3; 32]));
    }

    #[test]
    fn following_moves_only_the_parents() {
        let original = changeset();
        let reparented = original.clone().following(CausalParents::after(
            ChangeSetId::from_bytes([5; 32]),
            vec![],
        ));
        assert!(original.parents().is_genesis(), "input mutated");
        assert_eq!(reparented.parents().len(), 1);
        assert_eq!(reparented.base_head(), original.base_head());
    }
}
