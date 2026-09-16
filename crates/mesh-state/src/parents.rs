//! What a ChangeSet causally follows, mirrored from `mesh-types` for the reason `ids.rs` gives.
//!
//! The two constructors are separate on purpose, and that split is the reason a dropped causal
//! parent is a detectable event rather than a silent one. An empty parent list is a legitimate
//! state — the first ChangeSet in a workspace has no history — but it is also what a caller that
//! forgot to look up the parents produces. [`CausalParents::genesis`] makes the first case an
//! explicit statement and [`CausalParents::after`] makes the second unrepresentable, since it
//! takes the first parent by value.

use crate::ids::ChangeSetId;

/// The ChangeSets a ChangeSet causally follows.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CausalParents(Vec<ChangeSetId>);

impl CausalParents {
    /// No parents, said deliberately: this ChangeSet begins the history.
    #[must_use]
    pub const fn genesis() -> Self {
        Self(Vec::new())
    }

    /// At least one parent. `rest` carries the additional parents of a merge.
    #[must_use]
    pub fn after(first: ChangeSetId, rest: Vec<ChangeSetId>) -> Self {
        let mut parents = Vec::with_capacity(rest.len() + 1);
        parents.push(first);
        parents.extend(rest);
        Self(parents)
    }

    /// Build from however many parents a caller happens to hold.
    ///
    /// The one constructor that can produce an accidental genesis, which is why it is spelled
    /// `from_slice` rather than `new`: a caller reaching for it is stating that the count is data.
    /// Every path inside this crate uses [`CausalParents::genesis`] or [`CausalParents::after`].
    #[must_use]
    pub fn from_slice(parents: &[ChangeSetId]) -> Self {
        match parents.split_first() {
            None => Self::genesis(),
            Some((first, rest)) => Self::after(*first, rest.to_vec()),
        }
    }

    /// The parents, in the order given.
    #[must_use]
    pub fn as_slice(&self) -> &[ChangeSetId] {
        &self.0
    }

    /// How many parents there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether this ChangeSet begins the history.
    ///
    /// The same question as [`CausalParents::is_empty`], asked in the vocabulary of the protocol
    /// rather than of the container.
    #[must_use]
    pub fn is_genesis(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether there are no parents.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(byte: u8) -> ChangeSetId {
        ChangeSetId::from_bytes([byte; 32])
    }

    #[test]
    fn genesis_is_empty_and_says_so() {
        let parents = CausalParents::genesis();
        assert!(parents.is_genesis());
        assert_eq!(parents.as_slice(), &[]);
    }

    #[test]
    fn after_keeps_the_order_it_was_given() {
        let parents = CausalParents::after(id(3), vec![id(1), id(2)]);
        assert_eq!(parents.as_slice(), &[id(3), id(1), id(2)]);
        assert!(!parents.is_genesis());
        assert_eq!(parents.len(), 3);
    }

    /// `after` cannot express "no parents": the first one is taken by value. This is the whole
    /// reason the two constructors exist, so it is asserted rather than assumed.
    #[test]
    fn from_slice_agrees_with_the_two_deliberate_constructors() {
        assert_eq!(CausalParents::from_slice(&[]), CausalParents::genesis());
        assert_eq!(
            CausalParents::from_slice(&[id(1), id(2)]),
            CausalParents::after(id(1), vec![id(2)])
        );
    }
}
