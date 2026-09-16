//! Conflict row ten: the canonical head moved after a review was produced.
//!
//! # Why this row is not about the object graph
//!
//! The other rows resolve two changes against each other. This one resolves a *review* against the
//! state it was a review of. A person read a bundle, decided it was correct and approved it; by the
//! time the approval arrives, the head it was computed against may no longer be the head it would
//! advance.
//!
//! The plan's answer is "replay safely or require re-review", and the whole of the safety is in
//! which of the two you pick. [`head_movement`] picks by one rule: **the approval replays only
//! when nothing it touched was touched by what moved the head.** Any overlap and a person looks
//! again.
//!
//! That is deliberately blunt. A cleverer rule — replay when the overlapping changes merge
//! cleanly — would mean a person approved one thing and something else was published. The
//! reviewed state has to be the published state, or the human review gate is decoration. Plan §4.7
//! makes the same point from the other side: an agent key is never issued the capability to
//! advance the canonical head.
//!
//! # Nothing here discards anything
//!
//! [`HeadMovement::RequiresReReview`] is not a rejection. The approval stands, the work stands, and
//! what is required is another look — which is the difference between "we cannot publish this
//! yet" and "we threw this away".

use std::collections::BTreeSet;

use crate::ids::{HeadId, ObjectId};
use crate::rules::Rule;

/// What should happen to an approval whose head moved underneath it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HeadMovement {
    /// The head is the one the review was computed against. Publish.
    Unchanged,
    /// The head moved, but not over anything the review touched. The approval replays as written.
    ReplaySafe,
    /// The head moved over something the review touched. A person looks again.
    RequiresReReview {
        /// Every object both the review and the head movement touched, in identifier order.
        contested: Vec<ObjectId>,
    },
}

impl HeadMovement {
    /// The row of plan §4.8 this verdict comes from.
    #[must_use]
    pub const fn rule(&self) -> Rule {
        Rule::HeadMovedAfterReview
    }

    /// Whether a person has to look again before anything can be published.
    #[must_use]
    pub const fn needs_review(&self) -> bool {
        matches!(self, Self::RequiresReReview { .. })
    }

    /// Whether the approval may be published without another human decision.
    #[must_use]
    pub const fn may_publish(&self) -> bool {
        matches!(self, Self::Unchanged | Self::ReplaySafe)
    }
}

/// What to do with an approval computed against `reviewed_head` when the canonical head is
/// `current_head`.
///
/// `reviewed_objects` is what the approval changes; `landed_objects` is what moved the head since.
/// Both are sets, so the verdict does not depend on the order anything arrived in.
#[must_use]
pub fn head_movement(
    reviewed_head: HeadId,
    current_head: HeadId,
    reviewed_objects: &BTreeSet<ObjectId>,
    landed_objects: &BTreeSet<ObjectId>,
) -> HeadMovement {
    if reviewed_head == current_head {
        return HeadMovement::Unchanged;
    }
    let contested: Vec<ObjectId> = reviewed_objects
        .intersection(landed_objects)
        .copied()
        .collect();
    if contested.is_empty() {
        HeadMovement::ReplaySafe
    } else {
        HeadMovement::RequiresReReview { contested }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn objects(ids: &[u8]) -> BTreeSet<ObjectId> {
        ids.iter()
            .map(|id| ObjectId::from_bytes([*id; 16]))
            .collect()
    }

    fn head(byte: u8) -> HeadId {
        HeadId::from_bytes([byte; 32])
    }

    #[test]
    fn an_unmoved_head_publishes() {
        let verdict = head_movement(head(1), head(1), &objects(&[1]), &objects(&[1]));
        assert_eq!(verdict, HeadMovement::Unchanged);
        assert!(verdict.may_publish());
    }

    #[test]
    fn a_head_that_moved_elsewhere_replays() {
        let verdict = head_movement(head(1), head(2), &objects(&[1, 2]), &objects(&[3, 4]));
        assert_eq!(verdict, HeadMovement::ReplaySafe);
        assert!(verdict.may_publish());
        assert!(!verdict.needs_review());
    }

    #[test]
    fn a_head_that_moved_over_the_review_requires_another_look() {
        let verdict = head_movement(head(1), head(2), &objects(&[1, 2]), &objects(&[2, 3]));
        assert_eq!(
            verdict,
            HeadMovement::RequiresReReview {
                contested: vec![ObjectId::from_bytes([2; 16])]
            }
        );
        assert!(!verdict.may_publish());
        assert!(verdict.needs_review());
    }

    #[test]
    fn the_verdict_names_its_row() {
        assert_eq!(
            head_movement(head(1), head(1), &objects(&[]), &objects(&[])).rule(),
            Rule::HeadMovedAfterReview
        );
    }
}
