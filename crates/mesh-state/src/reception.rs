//! What happened to one delivered ChangeSet, and why a refusal is never a deletion.
//!
//! Delivery has four honest answers and this module names all four, because the difference between
//! them is the difference between the failures that matter. "Buffered" and "refused" in particular
//! are not the same thing and must never be collapsed: a buffered ChangeSet is *valid and
//! incomplete* and is held forever; a refused one is *structurally impossible* and is never
//! applied. Collapsing them either drops work that was only early, or admits a transition nobody
//! derived.

use core::fmt;

use crate::ids::{ChangeSetId, HeadId};

/// What one delivery did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reception {
    /// The ChangeSet was applied, possibly together with buffered ChangeSets it unblocked.
    Applied {
        /// The head after the delivery.
        head: HeadId,
        /// Everything applied by this delivery, in causal order — the delivered ChangeSet and any
        /// buffered ones whose last missing parent it supplied.
        applied: Vec<ChangeSetId>,
        /// Buffered ChangeSets this delivery unblocked and then refused, each with its reason.
        ///
        /// Empty in every honest run. Reported rather than swallowed, because a peer that sent a
        /// ChangeSet whose causal parents finally arrived and whose claimed head is still wrong has
        /// told the receiver something, and silence would be the wrong way to record it.
        refused: Vec<(ChangeSetId, Refusal)>,
    },
    /// The ChangeSet was already applied. The head did not move and nothing else changed.
    AlreadyApplied,
    /// The ChangeSet was already buffered, waiting for the same causal parents.
    AlreadyBuffered,
    /// A causal parent has not arrived, so the ChangeSet is held as a known-missing dependency.
    Buffered {
        /// The parents that have not arrived. Never empty.
        missing: Vec<ChangeSetId>,
    },
    /// The ChangeSet is not a transition this receiver can derive, so it was not applied.
    Refused(Refusal),
}

impl Reception {
    /// Whether the delivery changed anything at all.
    ///
    /// The idempotence question, asked in one place so that no caller has to enumerate the
    /// variants and get it subtly wrong.
    #[must_use]
    pub const fn changed_something(&self) -> bool {
        matches!(self, Self::Applied { .. } | Self::Buffered { .. })
    }

    /// The head, when the delivery produced one.
    #[must_use]
    pub const fn head(&self) -> Option<HeadId> {
        match self {
            Self::Applied { head, .. } => Some(*head),
            _ => None,
        }
    }
}

/// Why a ChangeSet is not a transition this receiver can derive.
///
/// Every variant is a statement about the delivered record, never about the receiver's state — a
/// refusal must mean the same thing on every peer holding the same causal set, or convergence is
/// gone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The ChangeSet names itself as a causal parent.
    SelfParent {
        /// The ChangeSet.
        changeset: ChangeSetId,
    },
    /// The ChangeSet names the same causal parent more than once.
    ParentRepeated {
        /// The ChangeSet.
        changeset: ChangeSetId,
        /// The repeated parent.
        parent: ChangeSetId,
    },
    /// The ChangeSet names a causal parent that another of its causal parents already follows.
    ///
    /// Harmless to the head — a redundant parent names a causal set that is already covered, so
    /// the head does not move — and refused anyway, because the causal parents are bound into the
    /// ChangeSet's own identifier. Two spellings of one transition would be two identifiers for
    /// one state, and the whole point of a content-derived name is that there is one.
    ParentImplied {
        /// The ChangeSet.
        changeset: ChangeSetId,
        /// The redundant parent.
        parent: ChangeSetId,
        /// The other parent that already follows it.
        implied_by: ChangeSetId,
    },
    /// The head the author says it was authored against is not the head its causal parents
    /// produce. A dropped causal parent lands here.
    BaseHeadNotDerived {
        /// The ChangeSet.
        changeset: ChangeSetId,
        /// What the author claimed.
        claimed: HeadId,
        /// What the receiver derived from the named causal parents.
        derived: HeadId,
    },
    /// The head the author says applying it produces is not the head it produces.
    ResultingHeadNotDerived {
        /// The ChangeSet.
        changeset: ChangeSetId,
        /// What the author claimed.
        claimed: HeadId,
        /// What the receiver derived.
        derived: HeadId,
    },
    /// Authoring was asked to reuse an identifier this actor already knows.
    AlreadyKnown {
        /// The identifier.
        changeset: ChangeSetId,
    },
}

impl Refusal {
    /// The ChangeSet the refusal is about.
    #[must_use]
    pub const fn changeset(&self) -> ChangeSetId {
        match self {
            Self::SelfParent { changeset }
            | Self::ParentRepeated { changeset, .. }
            | Self::ParentImplied { changeset, .. }
            | Self::BaseHeadNotDerived { changeset, .. }
            | Self::ResultingHeadNotDerived { changeset, .. }
            | Self::AlreadyKnown { changeset } => *changeset,
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SelfParent { changeset } => {
                write!(formatter, "{changeset} names itself as a causal parent")
            }
            Self::ParentRepeated { changeset, parent } => {
                write!(
                    formatter,
                    "{changeset} names the causal parent {parent} more than once"
                )
            }
            Self::ParentImplied {
                changeset,
                parent,
                implied_by,
            } => write!(
                formatter,
                "{changeset} names the causal parent {parent}, which {implied_by} already follows"
            ),
            Self::BaseHeadNotDerived {
                changeset,
                claimed,
                derived,
            } => write!(
                formatter,
                "{changeset} says it was authored against {claimed}, but its causal parents \
                 produce {derived}"
            ),
            Self::ResultingHeadNotDerived {
                changeset,
                claimed,
                derived,
            } => write!(
                formatter,
                "{changeset} says it produces {claimed}, but applying it produces {derived}"
            ),
            Self::AlreadyKnown { changeset } => {
                write!(formatter, "{changeset} is already known to this actor")
            }
        }
    }
}

impl std::error::Error for Refusal {}

/// A ChangeSet held because a causal parent has not arrived.
///
/// Held indefinitely and visibly. `docs/protocol.md` §3.3 defines a known-missing dependency as
/// exactly this: buffered, "held visibly and indefinitely rather than dropped or collected". There
/// is no expiry here and no eviction, and adding one would need a decision record, not a patch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnownMissing {
    waiting: ChangeSetId,
    missing: Vec<ChangeSetId>,
}

impl KnownMissing {
    /// A waiting ChangeSet and the parents it is waiting for.
    #[must_use]
    pub fn new(waiting: ChangeSetId, missing: Vec<ChangeSetId>) -> Self {
        Self { waiting, missing }
    }

    /// The ChangeSet that is waiting.
    #[must_use]
    pub const fn waiting(&self) -> ChangeSetId {
        self.waiting
    }

    /// The causal parents that have not arrived, in ascending identifier order.
    #[must_use]
    pub fn missing(&self) -> &[ChangeSetId] {
        &self.missing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(byte: u8) -> ChangeSetId {
        ChangeSetId::from_bytes([byte; 32])
    }

    #[test]
    fn only_applying_and_buffering_change_anything() {
        assert!(Reception::Applied {
            head: HeadId::from_bytes([0; 32]),
            applied: vec![id(1)],
            refused: Vec::new(),
        }
        .changed_something());
        assert!(Reception::Buffered {
            missing: vec![id(2)]
        }
        .changed_something());
        assert!(!Reception::AlreadyApplied.changed_something());
        assert!(!Reception::AlreadyBuffered.changed_something());
        assert!(!Reception::Refused(Refusal::SelfParent { changeset: id(1) }).changed_something());
    }

    #[test]
    fn every_refusal_names_its_changeset() {
        let refusals = [
            Refusal::SelfParent { changeset: id(1) },
            Refusal::ParentRepeated {
                changeset: id(1),
                parent: id(2),
            },
            Refusal::ParentImplied {
                changeset: id(1),
                parent: id(2),
                implied_by: id(3),
            },
            Refusal::BaseHeadNotDerived {
                changeset: id(1),
                claimed: HeadId::from_bytes([3; 32]),
                derived: HeadId::from_bytes([4; 32]),
            },
            Refusal::ResultingHeadNotDerived {
                changeset: id(1),
                claimed: HeadId::from_bytes([3; 32]),
                derived: HeadId::from_bytes([4; 32]),
            },
            Refusal::AlreadyKnown { changeset: id(1) },
        ];
        for refusal in refusals {
            assert_eq!(refusal.changeset(), id(1));
            assert!(!refusal.to_string().is_empty());
        }
    }

    /// A refusal is rendered for a person somewhere. The words the protocol forbids on a
    /// user-facing surface must not be how it explains itself.
    #[test]
    fn no_refusal_explains_itself_with_a_forbidden_word() {
        let forbidden = [
            "DAG",
            "frontier",
            "vector clock",
            "branch",
            "commit",
            "rebase",
            "staging",
            "operation log",
        ];
        let refusals = [
            Refusal::SelfParent { changeset: id(1) }.to_string(),
            Refusal::ParentRepeated {
                changeset: id(1),
                parent: id(2),
            }
            .to_string(),
            Refusal::ParentImplied {
                changeset: id(1),
                parent: id(2),
                implied_by: id(3),
            }
            .to_string(),
            Refusal::BaseHeadNotDerived {
                changeset: id(1),
                claimed: HeadId::from_bytes([3; 32]),
                derived: HeadId::from_bytes([4; 32]),
            }
            .to_string(),
            Refusal::ResultingHeadNotDerived {
                changeset: id(1),
                claimed: HeadId::from_bytes([3; 32]),
                derived: HeadId::from_bytes([4; 32]),
            }
            .to_string(),
            Refusal::AlreadyKnown { changeset: id(1) }.to_string(),
        ];
        for text in refusals {
            let lowered = text.to_lowercase();
            for word in forbidden {
                assert!(
                    !lowered.contains(&word.to_lowercase()),
                    "{text:?} uses the forbidden word {word:?}"
                );
            }
        }
    }

    #[test]
    fn a_known_missing_dependency_reports_both_halves() {
        let known = KnownMissing::new(id(9), vec![id(1), id(2)]);
        assert_eq!(known.waiting(), id(9));
        assert_eq!(known.missing(), &[id(1), id(2)]);
    }
}
