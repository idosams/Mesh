//! Revocation, and the only question that matters: **what happens to what the key already signed?**
//!
//! # Marking a key revoked is the easy half
//!
//! A revocation scheme that only answers "is this key revoked" has two ways to be wrong and picks
//! one of them by accident. If a revoked author's records are refused retroactively, revocation is a
//! **history rewrite**: publishing one rotation record deletes work that was reviewed, approved and
//! built on, and an attacker who can get a key revoked can erase that key's history. If they are
//! accepted unconditionally, revocation does not bite the case it exists for.
//!
//! Mesh answers it by comparing two epochs rather than by consulting a flag. Every record names the
//! policy epoch it was sealed under; every revocation names the epoch it becomes **effective** in.
//! A record stands exactly when it was sealed **strictly before** its author's revocation took
//! effect ([`crate::EpochChain::authorship_standing`]).
//!
//! # Why a later revocation can never reach back
//!
//! A rotation issued on a chain at epoch `n` becomes effective at `n + 1`
//! ([`EpochRotation::to`](crate::EpochRotation::to)), and `n + 1` is strictly greater than every
//! epoch any already-sealed record can name. So "sealed before the revocation" is a fact fixed at
//! sealing time that no future rotation can change. That is the structural reason revocation is not
//! a history rewrite — not a rule the check remembers to apply.
//!
//! The mirror-image attack is a *second* rotation naming a key that is already revoked, with a later
//! effective epoch, which would re-validate the records the first revocation refused.
//! [`RevocationLedger::record`] keeps the **earliest** effective epoch per key, so a revocation only
//! ever moves earlier, never later, and re-issuing one is a no-op rather than a widening.
//!
//! # Rotating an actor key mints a different actor
//!
//! ADR-0003: an actor is named by its key and by nothing else. Succeeding a compromised key
//! therefore does not rename an actor, it introduces a new one, and the retired key keeps naming
//! itself on every record it signed — correctly and permanently. Three consequences, all of which
//! this module takes as given rather than as a choice:
//!
//! * The revoked key's history stays attributed to the revoked key. There is no re-attribution pass
//!   and there is nothing for one to rewrite.
//! * The successor key inherits **nothing**: not the predecessor's authority, which it obtains only
//!   by a fresh human-issued capability in the epoch now in force, and not the predecessor's
//!   revocation, which names one key.
//! * A revoked holder cannot re-enter by minting itself a successor.
//!   [`Capability::delegate`](mesh_crypto::Capability::delegate) copies the parent's policy epoch and
//!   takes no epoch argument, so a capability derived from a dead one is dead in the same epoch. The
//!   re-entry is not rejected; it is unwritable.
//!
//! # Fail closed
//!
//! [`AuthorshipStanding::Indeterminate`] is the answer whenever the peer has not observed the epoch
//! a record names. A peer that is behind cannot know which rotations happened in the epochs it has
//! not seen, so "the author was not revoked" is a statement it has no evidence for, and the honest
//! answer is not "valid". Repairing it costs one small record; guessing wrong the other way accepts
//! work from a key the operator has already declared stolen.
//!
//! # Ordering
//!
//! Epochs are counters. No wall clock is read or stored here — a revocation's position is the epoch
//! it is effective in, and `RotationReason` says why it happened, never when.

use core::fmt;
use std::collections::BTreeMap;

use mesh_crypto::ActorKey;
use mesh_types::PolicyEpoch;

use crate::epoch::RotationReason;

/// One key's revocation, as the rotation that carried it recorded it.
///
/// Immutable, and there is no public constructor: every entry in existence was produced by applying
/// a rotation record, so an entry cannot name a revocation that no rotation carried.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RevocationEntry {
    subject: ActorKey,
    effective: PolicyEpoch,
    reason: RotationReason,
}

impl RevocationEntry {
    /// The key that was revoked.
    #[must_use]
    pub const fn subject(&self) -> ActorKey {
        self.subject
    }

    /// The epoch this revocation is effective **in**, and from then on.
    ///
    /// A record sealed in an earlier epoch was sealed before the revocation and stands. A record
    /// sealed in this epoch or a later one was sealed by a key already declared revoked.
    #[must_use]
    pub const fn effective(&self) -> PolicyEpoch {
        self.effective
    }

    /// Why the rotation that carried this revocation happened.
    ///
    /// Not used to decide validity — every revocation bites identically — and carried so an operator
    /// surface can tell "this key was stolen, review everything it authored before the rotation"
    /// from "this device was decommissioned".
    #[must_use]
    pub const fn reason(&self) -> RotationReason {
        self.reason
    }
}

impl fmt::Display for RevocationEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} revoked from policy epoch {} ({})",
            self.subject,
            self.effective.value(),
            self.reason
        )
    }
}

/// Where a key stands against the revocations one peer has applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RevocationStanding {
    /// No rotation this peer has applied revokes this key.
    NotRevoked,
    /// Revoked, from `effective` onwards.
    Revoked {
        /// The epoch the revocation is effective in.
        effective: PolicyEpoch,
        /// Why the key was revoked.
        reason: RotationReason,
    },
}

impl RevocationStanding {
    /// Whether this key is revoked as far as this peer knows.
    ///
    /// A match over every variant rather than `matches!`, so that adding a standing forces a
    /// decision here instead of silently joining one side.
    #[must_use]
    pub const fn is_revoked(&self) -> bool {
        match self {
            Self::NotRevoked => false,
            Self::Revoked { .. } => true,
        }
    }
}

/// Whether one record's author had authority to seal it, judged from one peer's knowledge.
///
/// The three answers are deliberately not two. "This peer cannot tell" is a distinct outcome from
/// "this is invalid" because they have different repairs — fetch the missing policy records, versus
/// refuse the record permanently — and collapsing them either loses the repair or, far worse,
/// collapses "cannot tell" into "valid".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthorshipStanding {
    /// Sealed in an epoch this peer has observed, before any revocation of its author took effect.
    /// The record stands, permanently: no future rotation can reach back past it.
    Stands,
    /// Sealed in or after the epoch the author's revocation took effect.
    AuthoredAfterRevocation {
        /// The epoch the author's revocation took effect in.
        effective: PolicyEpoch,
        /// The epoch the record was sealed under.
        sealed_in: PolicyEpoch,
    },
    /// The record names an epoch this peer has never observed, so this peer has no evidence about
    /// which keys were revoked in it. Fails closed — see the module documentation.
    Indeterminate {
        /// The epoch the record was sealed under.
        sealed_in: PolicyEpoch,
        /// The epoch in force on this peer.
        in_force: PolicyEpoch,
    },
}

impl AuthorshipStanding {
    /// Whether the record may be treated as validly authored.
    ///
    /// True for exactly one variant. Written as an exhaustive match so that a new standing has to
    /// be placed on a side by a person rather than by a `matches!` pattern that never saw it.
    #[must_use]
    pub const fn stands(&self) -> bool {
        match self {
            Self::Stands => true,
            Self::AuthoredAfterRevocation { .. } | Self::Indeterminate { .. } => false,
        }
    }

    /// Whether this peer refused for want of evidence rather than on the merits — the case a caller
    /// repairs by fetching policy records and asking again.
    #[must_use]
    pub const fn is_indeterminate(&self) -> bool {
        matches!(self, Self::Indeterminate { .. })
    }
}

impl fmt::Display for AuthorshipStanding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stands => formatter.write_str("sealed before its author was revoked"),
            Self::AuthoredAfterRevocation {
                effective,
                sealed_in,
            } => write!(
                formatter,
                "sealed in policy epoch {} and its author was revoked from epoch {}",
                sealed_in.value(),
                effective.value()
            ),
            Self::Indeterminate {
                sealed_in,
                in_force,
            } => write!(
                formatter,
                "sealed in policy epoch {}, which this peer has not observed; epoch {} is in force",
                sealed_in.value(),
                in_force.value()
            ),
        }
    }
}

/// Every revocation one peer has applied, with the epoch each became effective in.
///
/// Immutable and monotone: [`RevocationLedger::record`] consumes the ledger and returns a new one,
/// nothing takes `&mut self`, and there is no removal. A ledger an attacker can edit is a ledger the
/// attacker edits immediately after the rotation that named their key.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RevocationLedger {
    entries: BTreeMap<ActorKey, RevocationEntry>,
}

impl RevocationLedger {
    /// A ledger with no revocations in it.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    /// How many distinct keys are revoked.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no key is revoked.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Whether `key` is revoked.
    #[must_use]
    pub fn contains(&self, key: &ActorKey) -> bool {
        self.entries.contains_key(key)
    }

    /// The revocation of `key`, if there is one.
    #[must_use]
    pub fn entry_for(&self, key: &ActorKey) -> Option<&RevocationEntry> {
        self.entries.get(key)
    }

    /// Where `key` stands.
    #[must_use]
    pub fn standing_of(&self, key: &ActorKey) -> RevocationStanding {
        self.entries
            .get(key)
            .map_or(RevocationStanding::NotRevoked, |entry| {
                RevocationStanding::Revoked {
                    effective: entry.effective,
                    reason: entry.reason,
                }
            })
    }

    /// Every revocation, ordered by key.
    pub fn entries(&self) -> impl Iterator<Item = &RevocationEntry> {
        self.entries.values()
    }

    /// Record one revocation, keeping the **earliest** effective epoch for a key already revoked.
    ///
    /// Deliberately **not public**: the only caller is `EpochChain::apply`, so every entry that
    /// exists came from a rotation record that a peer verified against its own chain head.
    ///
    /// The earliest-wins rule is the whole reason this is not a plain insert. A second rotation
    /// naming an already-revoked key carries a later effective epoch, and letting it overwrite the
    /// first would re-validate exactly the records the first revocation refused — a history rewrite
    /// in the other direction, available to anyone who can get one more rotation issued.
    #[must_use]
    pub(crate) fn record(
        self,
        subject: ActorKey,
        effective: PolicyEpoch,
        reason: RotationReason,
    ) -> Self {
        let mut entries = self.entries;
        entries
            .entry(subject)
            .and_modify(|existing| {
                if effective < existing.effective {
                    *existing = RevocationEntry {
                        subject,
                        effective,
                        reason,
                    };
                }
            })
            .or_insert(RevocationEntry {
                subject,
                effective,
                reason,
            });
        Self { entries }
    }
}

impl fmt::Display for RevocationLedger {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} revoked key(s)", self.entries.len())
    }
}
