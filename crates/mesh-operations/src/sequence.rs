//! Actor sequence numbers: monotonic where they are issued, and gap-detectable where they arrive.
//!
//! # The counter exists to make an omission visible
//!
//! Every ChangeSet carries its author's position in that author's own sequence. Causal parents
//! already say what a ChangeSet followed; the sequence says something parents cannot — **that
//! nothing of this author's was dropped between them**. A peer that withholds one ChangeSet and
//! forwards the next leaves a hole an ancestry check cannot see, because the later ChangeSet's
//! parents may not name the withheld one at all.
//!
//! So the two halves live in two types, and neither can do the other's job:
//!
//! * [`SequenceLedger`] issues. It is the only way to obtain a number for a ChangeSet this actor
//!   authors, it never issues the same number twice, and it never goes backwards.
//! * [`SequenceWitness`] observes. It records what has arrived from each author and reports what is
//!   missing.
//!
//! # Monotonic is a property of the type, not of the caller
//!
//! [`SequenceLedger::issue`] takes `self` by value and returns the next ledger with the number, so
//! an issued number cannot be re-issued by holding the old ledger — the old ledger is gone. That is
//! the same move `ChangeSetDraft` makes for causal context, and it is why there is no `Mutex` here
//! and no interior mutability: concurrency is the caller's, ownership is the guarantee.
//! `tests/sequence.rs` drives eight threads through one shared ledger and requires the issued set
//! to be exactly `1..=n` with no repeat.
//!
//! # Saturating, not wrapping
//!
//! A wrapped actor sequence makes an omission undetectable, which is the one thing this counter
//! exists to prevent. `u64::MAX` is unreachable in practice and stalling there is the safe
//! failure. [`SequenceLedger::issue`] refuses at the ceiling rather than repeating a number.

use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

use crate::ids::ActorId;

/// A strictly increasing per-actor counter carried by every ChangeSet.
///
/// Mirrors `mesh_types::ActorSequence`; `src/ids.rs` states why this crate mirrors rather than
/// imports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActorSequence(u64);

impl ActorSequence {
    /// The first number an actor ever issues.
    pub const FIRST: Self = Self(1);

    /// A sequence number.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// The number.
    #[must_use]
    pub const fn value(&self) -> u64 {
        self.0
    }

    /// The next number in the sequence.
    ///
    /// Saturating rather than wrapping, for the reason in the module header.
    #[must_use]
    pub const fn next(&self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

impl fmt::Display for ActorSequence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// Why a number could not be issued.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SequenceExhausted {
    /// The counter reached `u64::MAX`. Stalling is the safe failure; wrapping would make an
    /// omission undetectable for the rest of this actor's life.
    Ceiling {
        /// The actor whose counter is at the ceiling.
        actor: ActorId,
    },
}

impl fmt::Display for SequenceExhausted {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ceiling { actor } => write!(
                formatter,
                "actor {actor} has issued every sequence number a u64 can hold"
            ),
        }
    }
}

impl std::error::Error for SequenceExhausted {}

/// The issuing half: one actor's own counter.
///
/// Consumed by [`SequenceLedger::issue`] and returned advanced, so a number cannot be issued twice
/// by keeping the earlier ledger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SequenceLedger {
    actor: ActorId,
    issued: u64,
}

impl SequenceLedger {
    /// A ledger that has issued nothing. The first [`SequenceLedger::issue`] returns
    /// [`ActorSequence::FIRST`].
    ///
    /// Zero is never issued, so "this actor has authored nothing" and "this actor authored its
    /// first ChangeSet" are different values rather than the same one.
    #[must_use]
    pub const fn new(actor: ActorId) -> Self {
        Self { actor, issued: 0 }
    }

    /// A ledger that has already issued up to `issued`, for resuming after a restart.
    ///
    /// The durable store is the source of this number. A ledger resumed below what was really
    /// issued would repeat numbers, which is why this is a named constructor rather than a setter.
    #[must_use]
    pub const fn resuming(actor: ActorId, issued: ActorSequence) -> Self {
        Self {
            actor,
            issued: issued.value(),
        }
    }

    /// The actor this ledger issues for.
    #[must_use]
    pub const fn actor(&self) -> ActorId {
        self.actor
    }

    /// The highest number issued so far, or `None` when nothing has been.
    #[must_use]
    pub const fn issued(&self) -> Option<ActorSequence> {
        if self.issued == 0 {
            None
        } else {
            Some(ActorSequence::new(self.issued))
        }
    }

    /// Issue the next number.
    ///
    /// # Errors
    ///
    /// [`SequenceExhausted`] at `u64::MAX`.
    ///
    /// ```
    /// use mesh_operations::{ActorId, ActorSequence, SequenceLedger};
    ///
    /// let ledger = SequenceLedger::new(ActorId::from_bytes([1; 32]));
    /// let (ledger, first) = ledger.issue().unwrap();
    /// let (_, second) = ledger.issue().unwrap();
    /// assert_eq!(first, ActorSequence::FIRST);
    /// assert_eq!(second, ActorSequence::new(2));
    /// ```
    pub fn issue(self) -> Result<(Self, ActorSequence), SequenceExhausted> {
        if self.issued == u64::MAX {
            return Err(SequenceExhausted::Ceiling { actor: self.actor });
        }
        let next = self.issued + 1;
        Ok((
            Self {
                actor: self.actor,
                issued: next,
            },
            ActorSequence::new(next),
        ))
    }
}

/// What a witness made of one arriving sequence number.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SequenceObservation {
    /// The next number in this author's sequence, with no hole before it.
    InOrder,
    /// A number already seen. Re-delivery is normal and changes nothing.
    Duplicate,
    /// A number beyond the next expected one. The numbers in between have not arrived.
    ///
    /// This is a statement about *this receiver's* knowledge, not an accusation: the missing
    /// ChangeSets may be in flight. It becomes a fault when it persists.
    Gap {
        /// The numbers not yet seen, in ascending order.
        missing: Vec<ActorSequence>,
    },
}

/// The observing half: what has arrived from each author, and what has not.
///
/// Keyed by author, because a sequence is per actor. Two actors both at 7 are unrelated facts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SequenceWitness {
    seen: BTreeMap<ActorId, BTreeSet<u64>>,
}

impl SequenceWitness {
    /// A witness that has seen nothing.
    #[must_use]
    pub fn new() -> Self {
        Self {
            seen: BTreeMap::new(),
        }
    }

    /// Record `sequence` from `actor`, and say what it means.
    ///
    /// ```
    /// use mesh_operations::{ActorId, ActorSequence, SequenceObservation, SequenceWitness};
    ///
    /// let actor = ActorId::from_bytes([1; 32]);
    /// let mut witness = SequenceWitness::new();
    /// assert_eq!(witness.observe(actor, ActorSequence::new(1)), SequenceObservation::InOrder);
    /// assert_eq!(
    ///     witness.observe(actor, ActorSequence::new(4)),
    ///     SequenceObservation::Gap { missing: vec![ActorSequence::new(2), ActorSequence::new(3)] }
    /// );
    /// // The hole closes as the missing numbers arrive, and closing it is not itself a gap.
    /// assert_eq!(witness.observe(actor, ActorSequence::new(2)), SequenceObservation::InOrder);
    /// assert_eq!(witness.missing(actor), vec![ActorSequence::new(3)]);
    /// ```
    pub fn observe(&mut self, actor: ActorId, sequence: ActorSequence) -> SequenceObservation {
        let seen = self.seen.entry(actor).or_default();
        if !seen.insert(sequence.value()) {
            return SequenceObservation::Duplicate;
        }
        let missing = missing_below(seen);
        if missing.iter().any(|value| *value < sequence.value()) {
            SequenceObservation::Gap {
                missing: missing
                    .into_iter()
                    .filter(|value| *value < sequence.value())
                    .map(ActorSequence::new)
                    .collect(),
            }
        } else {
            SequenceObservation::InOrder
        }
    }

    /// Every number this witness expects from `actor` and has not seen, in ascending order.
    ///
    /// Bounded by the highest number seen: a witness cannot know how many ChangeSets an actor has
    /// authored that it has never heard of, and reporting an unbounded tail as missing would make
    /// every actor permanently incomplete.
    #[must_use]
    pub fn missing(&self, actor: ActorId) -> Vec<ActorSequence> {
        self.seen
            .get(&actor)
            .map(|seen| {
                missing_below(seen)
                    .into_iter()
                    .map(ActorSequence::new)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The highest number seen from `actor`, if any.
    #[must_use]
    pub fn highest(&self, actor: ActorId) -> Option<ActorSequence> {
        self.seen
            .get(&actor)
            .and_then(|seen| seen.iter().next_back())
            .map(|value| ActorSequence::new(*value))
    }

    /// Whether every number from one up to the highest seen has arrived, for `actor`.
    #[must_use]
    pub fn is_contiguous(&self, actor: ActorId) -> bool {
        self.missing(actor).is_empty()
    }

    /// Every actor this witness has heard from, in identifier order.
    #[must_use]
    pub fn actors(&self) -> Vec<ActorId> {
        self.seen.keys().copied().collect()
    }
}

/// The numbers from one up to the highest seen that are not in `seen`.
fn missing_below(seen: &BTreeSet<u64>) -> Vec<u64> {
    let Some(highest) = seen.iter().next_back().copied() else {
        return Vec::new();
    };
    (1..highest).filter(|value| !seen.contains(value)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actor(byte: u8) -> ActorId {
        ActorId::from_bytes([byte; 32])
    }

    #[test]
    fn a_ledger_issues_one_two_three_and_never_zero() {
        let mut ledger = SequenceLedger::new(actor(1));
        assert_eq!(ledger.issued(), None);
        let mut issued = Vec::new();
        for _ in 0..5 {
            let (next, value) = ledger.issue().unwrap();
            ledger = next;
            issued.push(value.value());
        }
        assert_eq!(issued, vec![1, 2, 3, 4, 5]);
        assert_eq!(ledger.issued(), Some(ActorSequence::new(5)));
    }

    #[test]
    fn a_resumed_ledger_continues_rather_than_restarting() {
        let ledger = SequenceLedger::resuming(actor(1), ActorSequence::new(41));
        let (_, next) = ledger.issue().unwrap();
        assert_eq!(next, ActorSequence::new(42));
    }

    #[test]
    fn the_ceiling_stalls_rather_than_wrapping() {
        let ledger = SequenceLedger::resuming(actor(1), ActorSequence::new(u64::MAX));
        assert_eq!(
            ledger.issue(),
            Err(SequenceExhausted::Ceiling { actor: actor(1) })
        );
        // And the mirrored counter saturates rather than wrapping to zero.
        assert_eq!(
            ActorSequence::new(u64::MAX).next(),
            ActorSequence::new(u64::MAX)
        );
    }

    #[test]
    fn a_gap_is_reported_with_every_number_it_hides() {
        let mut witness = SequenceWitness::new();
        assert_eq!(
            witness.observe(actor(1), ActorSequence::new(1)),
            SequenceObservation::InOrder
        );
        assert_eq!(
            witness.observe(actor(1), ActorSequence::new(5)),
            SequenceObservation::Gap {
                missing: vec![
                    ActorSequence::new(2),
                    ActorSequence::new(3),
                    ActorSequence::new(4)
                ]
            }
        );
        assert!(!witness.is_contiguous(actor(1)));
        assert_eq!(witness.highest(actor(1)), Some(ActorSequence::new(5)));
    }

    #[test]
    fn a_gap_that_starts_at_one_is_still_a_gap() {
        // The first thing this witness ever hears from the actor is number 3. Numbers 1 and 2 were
        // withheld, and a witness that only compared against "the last one I saw" would call this
        // in-order.
        let mut witness = SequenceWitness::new();
        assert_eq!(
            witness.observe(actor(2), ActorSequence::new(3)),
            SequenceObservation::Gap {
                missing: vec![ActorSequence::new(1), ActorSequence::new(2)]
            }
        );
    }

    #[test]
    fn re_delivery_is_a_duplicate_and_changes_nothing() {
        let mut witness = SequenceWitness::new();
        witness.observe(actor(1), ActorSequence::new(1));
        let before = witness.clone();
        assert_eq!(
            witness.observe(actor(1), ActorSequence::new(1)),
            SequenceObservation::Duplicate
        );
        assert_eq!(witness, before);
    }

    #[test]
    fn arrival_order_does_not_change_what_is_missing() {
        let mut forwards = SequenceWitness::new();
        for value in [1u64, 2, 4, 6] {
            forwards.observe(actor(1), ActorSequence::new(value));
        }
        let mut backwards = SequenceWitness::new();
        for value in [6u64, 4, 2, 1] {
            backwards.observe(actor(1), ActorSequence::new(value));
        }
        assert_eq!(forwards.missing(actor(1)), backwards.missing(actor(1)));
        assert_eq!(
            forwards.missing(actor(1)),
            vec![ActorSequence::new(3), ActorSequence::new(5)]
        );
    }

    #[test]
    fn two_actors_at_the_same_number_are_unrelated() {
        let mut witness = SequenceWitness::new();
        witness.observe(actor(1), ActorSequence::new(7));
        witness.observe(actor(2), ActorSequence::new(1));
        assert!(witness.is_contiguous(actor(2)));
        assert!(!witness.is_contiguous(actor(1)));
        assert_eq!(witness.actors(), vec![actor(1), actor(2)]);
    }

    #[test]
    fn an_actor_never_heard_from_is_missing_nothing() {
        let witness = SequenceWitness::new();
        assert!(witness.missing(actor(9)).is_empty());
        assert_eq!(witness.highest(actor(9)), None);
        assert!(witness.is_contiguous(actor(9)));
    }
}
