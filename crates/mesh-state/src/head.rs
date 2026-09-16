//! An actor head and where it sits on the review axis.
//!
//! # One axis here, not two
//!
//! Plan §4.4 lists six things an actor head "may be": local only, metadata replicated, fully
//! content available remotely, ready for review, superseded, archived. `docs/protocol.md` §2.1
//! splits that list into two independent axes and states why: `local only` describes what a peer
//! can retrieve, `ready for review` describes where the head sits in its lifecycle, and a surface
//! that reads one as the other reports a replication fact as a review fact. The register homes the
//! review axis (`head state`) here and the retrievability axis (`availability state`) in
//! `mesh-sync-engine`, whose business it is — this crate has no peer and no transfer, so it could
//! only ever hold that value, never compute it.
//!
//! So [`HeadState`] is the review axis, with the four members the register names, and the other
//! three members of plan §4.4's list are deliberately absent rather than forgotten.
//!
//! # The offered head is a value, not a pointer
//!
//! The failure this module exists to prevent is a head that advances past a state nobody reviewed.
//! It is prevented structurally: offering a head for review copies the head identifier into the
//! offer. Subsequent ChangeSets advance the actor's working head and cannot move the offer,
//! because the offer holds bytes rather than a reference to wherever the actor happens to be.
//! [`crate::HeadAdvancement::offer_for_review`] is where that happens and
//! `tests/heads.rs::an_offer_does_not_follow_the_working_head` is where it is proved.

use core::fmt;

use crate::ids::{ActorId, HeadId};

/// Where an actor head sits on the review axis.
///
/// Exactly the four members `docs/protocol.md` §3.2 names. Independent of `availability state`,
/// which is the retrievability axis and is never merged into this one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HeadState {
    /// The actor is working; this head has not been offered for review.
    Working,
    /// This exact head has been offered for review and has not yet been resolved.
    ReadyForReview,
    /// A later head replaced this one before it was resolved.
    Superseded,
    /// The head is kept for the record and is no longer a live offer.
    Archived,
}

impl HeadState {
    /// The user-facing word for this state.
    ///
    /// The six-state product vocabulary lives in `docs/product-prd.md` and this is not it: these
    /// are protocol register words. They are spelled out here so that a surface rendering a head
    /// never has to invent a spelling, and so that the spelling is a tested value rather than a
    /// format string in a caller.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::ReadyForReview => "ready for review",
            Self::Superseded => "superseded",
            Self::Archived => "archived",
        }
    }

    /// Every member, in register order, so that a caller enumerating them cannot miss one.
    #[must_use]
    pub const fn every() -> [Self; 4] {
        [
            Self::Working,
            Self::ReadyForReview,
            Self::Superseded,
            Self::Archived,
        ]
    }
}

impl fmt::Display for HeadState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One actor's head: whose it is, which state it names, and where that state sits on the review
/// axis.
///
/// Immutable. Every transition returns a new value, so a head that was offered for review cannot
/// be edited into naming a different state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ActorHead {
    actor: ActorId,
    head: HeadId,
    state: HeadState,
}

impl ActorHead {
    /// An actor head in a given review state.
    #[must_use]
    pub const fn new(actor: ActorId, head: HeadId, state: HeadState) -> Self {
        Self { actor, head, state }
    }

    /// Whose head this is.
    #[must_use]
    pub const fn actor(&self) -> ActorId {
        self.actor
    }

    /// The workspace state it names.
    #[must_use]
    pub const fn head(&self) -> HeadId {
        self.head
    }

    /// Where it sits on the review axis.
    #[must_use]
    pub const fn state(&self) -> HeadState {
        self.state
    }

    /// The same head in a different review state.
    ///
    /// Takes `self` by value and returns a new value: the head identifier is copied across
    /// unchanged, so no transition can be written that also moves which state is under review.
    #[must_use]
    pub const fn in_state(self, state: HeadState) -> Self {
        Self { state, ..self }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actor() -> ActorId {
        ActorId::from_bytes([7; 32])
    }

    fn head(byte: u8) -> HeadId {
        HeadId::from_bytes([byte; 32])
    }

    #[test]
    fn every_member_has_a_distinct_word() {
        let words: Vec<&str> = HeadState::every().iter().map(|s| s.as_str()).collect();
        let mut sorted = words.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), words.len(), "two members share a word");
        assert_eq!(words.len(), 4);
    }

    /// The register's member values, quoted exactly. A rename here is a register change, so it
    /// should have to break a test.
    #[test]
    fn the_words_are_the_register_words() {
        assert_eq!(HeadState::Working.as_str(), "working");
        assert_eq!(HeadState::ReadyForReview.as_str(), "ready for review");
        assert_eq!(HeadState::Superseded.as_str(), "superseded");
        assert_eq!(HeadState::Archived.as_str(), "archived");
    }

    /// The retrievability axis is `mesh-sync-engine`'s. None of its member values may appear here,
    /// or the two axes have been merged back into one enumeration.
    #[test]
    fn no_member_names_a_retrievability_fact() {
        for state in HeadState::every() {
            for retrievability in ["local only", "metadata replicated", "content available"] {
                assert_ne!(state.as_str(), retrievability);
            }
        }
    }

    #[test]
    fn a_transition_moves_the_state_and_nothing_else() {
        let offered = ActorHead::new(actor(), head(1), HeadState::ReadyForReview);
        let superseded = offered.in_state(HeadState::Superseded);
        assert_eq!(superseded.head(), head(1));
        assert_eq!(superseded.actor(), actor());
        assert_eq!(superseded.state(), HeadState::Superseded);
        assert_eq!(offered.state(), HeadState::ReadyForReview, "input mutated");
    }

    #[test]
    fn display_is_the_word() {
        assert_eq!(format!("{}", HeadState::ReadyForReview), "ready for review");
    }
}
