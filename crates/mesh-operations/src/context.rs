//! The causal and authority context a ChangeSet carries, mirrored from `mesh-types`.
//!
//! `src/ids.rs` states why this crate mirrors rather than imports. These four types are the ones a
//! ChangeSet binds beside its operations, and each exists to make one class of omission
//! unrepresentable rather than merely invalid.

use core::fmt;

use crate::ids::ChangeSetId;

/// The ChangeSets a ChangeSet causally follows.
///
/// The two cases are separate constructors on purpose. An empty parent list is a legitimate
/// state — the first ChangeSet in a workspace has no history — but it is also what a caller that
/// forgot to look up the parents produces. [`CausalParents::genesis`] makes the first case an
/// explicit statement and [`CausalParents::after`] makes the second unrepresentable, since it
/// takes the first parent by value.
#[derive(Clone, Debug, PartialEq, Eq)]
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

    /// The parents, in the order given.
    #[must_use]
    pub fn as_slice(&self) -> &[ChangeSetId] {
        &self.0
    }

    /// Whether this ChangeSet begins the history.
    #[must_use]
    pub fn is_genesis(&self) -> bool {
        self.0.is_empty()
    }
}

/// The policy epoch a ChangeSet was authored under.
///
/// Strictly increasing; authority granted under a prior epoch is not valid in a later one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PolicyEpoch(u64);

impl PolicyEpoch {
    /// An epoch.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// The epoch number.
    #[must_use]
    pub const fn value(&self) -> u64 {
        self.0
    }
}

impl fmt::Display for PolicyEpoch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// A hybrid logical clock reading — **for display and tiebreaking, never for causality**.
///
/// Order in this workspace is `lamport → event identifier → content hash`. Nothing here or
/// downstream may order two ChangeSets by this value, and the type carries no comparison that
/// would invite it: `PartialOrd` is deliberately not derived, because a derived one would make
/// `a < b` compile and read as causality.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Hlc {
    physical_millis: u64,
    logical: u32,
}

impl Hlc {
    /// A reading.
    #[must_use]
    pub const fn new(physical_millis: u64, logical: u32) -> Self {
        Self {
            physical_millis,
            logical,
        }
    }

    /// The physical component.
    #[must_use]
    pub const fn physical_millis(&self) -> u64 {
        self.physical_millis
    }

    /// The logical component.
    #[must_use]
    pub const fn logical(&self) -> u32 {
        self.logical
    }
}

/// A detached signature over a record.
///
/// Sixty-four bytes, carried opaquely. This crate neither produces nor checks one — signing is
/// `mesh-crypto`'s and the key material is a platform crate's — so there is no method here that
/// could be mistaken for verification.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Signature([u8; 64]);

impl Signature {
    /// The signature these bytes are.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }

    /// The bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 64] {
        &self.0
    }
}

impl fmt::Debug for Signature {
    /// Elided. A signature in a log line is noise, and a `Debug` that printed it would put it in
    /// every error message a ChangeSet appears in.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Signature(..)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genesis_and_after_are_different_statements() {
        assert!(CausalParents::genesis().is_genesis());
        let parents = CausalParents::after(ChangeSetId::from_bytes([1; 32]), Vec::new());
        assert!(!parents.is_genesis());
        assert_eq!(parents.as_slice().len(), 1);
    }

    #[test]
    fn a_merge_keeps_every_parent_in_the_order_given() {
        let parents = CausalParents::after(
            ChangeSetId::from_bytes([1; 32]),
            vec![
                ChangeSetId::from_bytes([2; 32]),
                ChangeSetId::from_bytes([3; 32]),
            ],
        );
        assert_eq!(
            parents.as_slice(),
            &[
                ChangeSetId::from_bytes([1; 32]),
                ChangeSetId::from_bytes([2; 32]),
                ChangeSetId::from_bytes([3; 32]),
            ]
        );
    }

    #[test]
    fn a_signature_does_not_print_itself() {
        let signature = Signature::from_bytes([0xab; 64]);
        assert_eq!(format!("{signature:?}"), "Signature(..)");
        assert_eq!(signature.as_bytes()[0], 0xab);
    }

    #[test]
    fn the_clock_reading_carries_both_components() {
        let hlc = Hlc::new(1_700_000_000_000, 3);
        assert_eq!(hlc.physical_millis(), 1_700_000_000_000);
        assert_eq!(hlc.logical(), 3);
    }
}
