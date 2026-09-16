//! Policy epochs: how a revocation takes effect on a peer that was never contacted.
//!
//! # The problem an epoch solves
//!
//! Revocation by list requires reaching every peer. Mesh is local-first and a peer can be offline
//! for days, so a design that needs a live round trip to make a revocation bite is a design in which
//! a compromised key keeps working for as long as its holder can stay unreachable — and staying
//! unreachable is the one thing an attacker who has just stolen a key is well placed to do.
//!
//! The epoch inverts it. Every capability names the epoch it was issued in
//! ([`Capability::policy_epoch`](mesh_crypto::Capability::policy_epoch)) and every ChangeSet names
//! the epoch it was sealed under (`mesh_types::ChangeSet::policy_epoch`). Authority is checked by
//! **equality against the epoch in force**, so rotating the epoch kills every capability issued
//! before it, all of them, at once, with no list and no lookup.
//!
//! # Why that works offline
//!
//! An [`EpochRotation`] is self-contained evidence: it names the epoch it leaves, the epoch it
//! enters, why, whose keys it revokes, and the digest of the chain it extends. A peer applies it
//! with [`EpochChain::observe`] whenever it arrives, by whatever carrier — replication, a sync
//! session, a file on a memory stick — and verifies it against its own head without contacting
//! anybody. There is no issuer to be online and no directory to be reachable.
//!
//! # And why a peer that is behind fails closed
//!
//! A capability naming an epoch **later** than the one a peer knows is not accepted on the theory
//! that the peer must be behind. It is denied ([`EpochStanding::Unknown`]), because "accept
//! authority from an epoch I have never seen" is precisely the request a forged capability makes,
//! and the honest case is repaired by fetching one small record. Fail closed: the cost of the
//! false negative is a retry, the cost of the false positive is publication under a revoked key.
//!
//! # Ordering
//!
//! The chain is ordered by epoch number and linked by digest. No wall clock is read here and none
//! is stored: an epoch is a counter, and `RotationReason` is why it moved, not when.

use std::collections::BTreeSet;

use core::fmt;

use mesh_crypto::ActorKey;
use mesh_types::{Blake3, ContentDigest, Digest32, DigestWriter, DomainTag, PolicyEpoch};

use crate::revocation::{AuthorshipStanding, RevocationLedger, RevocationStanding};

/// The digest domain for a rotation record. Versioned: changing what the record covers changes
/// every digest in the chain, which is a protocol change and not an edit.
const ROTATION_DOMAIN: DomainTag = DomainTag::new("mesh.v0.policy-epoch-rotation");

/// The digest a genesis chain starts from, so the first rotation links to a stated value rather
/// than to an implicit zero that two implementations could spell differently.
const GENESIS_LINK: Digest32 = Digest32::from_bytes([0u8; 32]);

/// Why the policy epoch moved.
///
/// Recorded on the rotation itself rather than kept beside it, so a peer that receives the record
/// receives the reason. A rotation with no stated reason is an audit trail that cannot answer the
/// only question anybody asks of it afterwards.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum RotationReason {
    /// An actor key is believed compromised.
    ActorKeyCompromised,
    /// A human's approval key is believed compromised. The most severe case: until the rotation
    /// propagates, a holder of that key can approve.
    HumanKeyCompromised,
    /// A device holding a key is lost or decommissioned.
    DeviceRetired,
    /// An actor left the workspace.
    ActorRemoved,
    /// The policy itself changed — who may do what, independent of any key.
    PolicyChanged,
    /// A periodic rotation with no incident behind it.
    ScheduledRefresh,
}

impl RotationReason {
    /// Every reason. Exhaustive by construction: the match below stops compiling if a variant is
    /// added without being listed here.
    pub const ALL: [Self; 6] = [
        Self::ActorKeyCompromised,
        Self::HumanKeyCompromised,
        Self::DeviceRetired,
        Self::ActorRemoved,
        Self::PolicyChanged,
        Self::ScheduledRefresh,
    ];

    /// The reason's wire name.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::ActorKeyCompromised => "actor-key-compromised",
            Self::HumanKeyCompromised => "human-key-compromised",
            Self::DeviceRetired => "device-retired",
            Self::ActorRemoved => "actor-removed",
            Self::PolicyChanged => "policy-changed",
            Self::ScheduledRefresh => "scheduled-refresh",
        }
    }

    /// Whether this reason means a key is in hostile hands, as opposed to an orderly change.
    ///
    /// Not used to decide anything here — every rotation invalidates the prior epoch equally — and
    /// exposed so an operator surface can tell an incident from a housekeeping event.
    #[must_use]
    pub const fn is_compromise(&self) -> bool {
        matches!(self, Self::ActorKeyCompromised | Self::HumanKeyCompromised)
    }
}

impl fmt::Display for RotationReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One step of the epoch chain: the whole evidence a peer needs to move its own epoch forward.
///
/// Immutable and self-describing. It carries the predecessor digest so that a peer applying it can
/// tell "this extends the history I have" from "this extends some other history", which is the
/// difference between catching up and being handed a substituted policy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EpochRotation {
    from: PolicyEpoch,
    to: PolicyEpoch,
    reason: RotationReason,
    revoked: BTreeSet<ActorKey>,
    predecessor: Digest32,
}

impl EpochRotation {
    /// The epoch this rotation leaves.
    #[must_use]
    pub const fn from(&self) -> PolicyEpoch {
        self.from
    }

    /// The epoch this rotation enters.
    #[must_use]
    pub const fn to(&self) -> PolicyEpoch {
        self.to
    }

    /// Why it rotated.
    #[must_use]
    pub const fn reason(&self) -> RotationReason {
        self.reason
    }

    /// The subjects whose keys this rotation revokes.
    ///
    /// Belt to the epoch's braces. Rotating already kills every capability issued in the prior
    /// epoch; this names the subjects that must not be *re-issued* one in the new epoch, so the
    /// revocation survives the next round of issuance.
    #[must_use]
    pub const fn revoked(&self) -> &BTreeSet<ActorKey> {
        &self.revoked
    }

    /// The digest of the chain state this rotation extends.
    #[must_use]
    pub const fn predecessor(&self) -> Digest32 {
        self.predecessor
    }

    /// This rotation's digest, which becomes the next predecessor.
    ///
    /// Covers every field, length-framed by [`DigestWriter`], so no two distinct rotations share a
    /// digest and no field can be moved between neighbours without changing it.
    #[must_use]
    pub fn digest(&self) -> Digest32 {
        let mut writer = DigestWriter::new(ROTATION_DOMAIN, <Blake3 as ContentDigest>::hasher());
        writer
            .digest(&self.predecessor)
            .u64(self.from.value())
            .u64(self.to.value())
            .text(self.reason.as_str())
            .u64(self.revoked.len() as u64);
        for key in &self.revoked {
            writer.bytes(key.as_bytes());
        }
        writer.finish()
    }
}

/// Where an epoch stands relative to the one a peer has in force.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EpochStanding {
    /// The epoch in force. Authority issued in it is live.
    Current,
    /// An epoch that has been rotated past. Every capability issued in it is dead.
    Superseded {
        /// The epoch in force now.
        by: PolicyEpoch,
    },
    /// An epoch this peer has never seen. Denied, not deferred — see the module documentation.
    Unknown {
        /// The epoch this peer has in force.
        in_force: PolicyEpoch,
    },
}

impl EpochStanding {
    /// Whether authority issued in this epoch is live.
    ///
    /// Written as a match over every variant rather than as `matches!(self, Current)` so that
    /// adding a standing forces a decision here instead of silently joining the denied side —
    /// which is the safe side, but silently is not how a new authority state should arrive.
    #[must_use]
    pub const fn is_live(&self) -> bool {
        match self {
            Self::Current => true,
            Self::Superseded { .. } | Self::Unknown { .. } => false,
        }
    }
}

/// One peer's view of the policy epoch: the epoch in force, the chain digest that proves how it
/// got there, and every key revoked along the way with the epoch its revocation took effect in.
///
/// Immutable. [`EpochChain::rotate`] and [`EpochChain::observe`] consume the chain and return a new
/// one; nothing here takes `&mut self`, so no code path can move an epoch backwards by editing the
/// value somebody else is holding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EpochChain {
    current: PolicyEpoch,
    head: Digest32,
    revocations: RevocationLedger,
    rotations: u64,
}

impl EpochChain {
    /// A chain at `epoch` with no history.
    #[must_use]
    pub fn genesis(epoch: PolicyEpoch) -> Self {
        Self {
            current: epoch,
            head: GENESIS_LINK,
            revocations: RevocationLedger::empty(),
            rotations: 0,
        }
    }

    /// The epoch in force.
    #[must_use]
    pub const fn current(&self) -> PolicyEpoch {
        self.current
    }

    /// The chain head digest. Two peers agree on the policy history exactly when these are equal.
    #[must_use]
    pub const fn head(&self) -> Digest32 {
        self.head
    }

    /// How many rotations this peer has applied.
    #[must_use]
    pub const fn rotations(&self) -> u64 {
        self.rotations
    }

    /// Every key revoked by a rotation this peer has applied, with the epoch each revocation took
    /// effect in.
    #[must_use]
    pub const fn revocations(&self) -> &RevocationLedger {
        &self.revocations
    }

    /// Whether `key` has been revoked.
    ///
    /// The question the authority gate asks before issuing a *new* grant. It is deliberately not
    /// the question asked about an existing record: see [`EpochChain::authorship_standing`], which
    /// is the difference between revoking a key and rewriting what it signed.
    #[must_use]
    pub fn is_revoked(&self, key: &ActorKey) -> bool {
        self.revocations.contains(key)
    }

    /// Where `key` stands against the revocations this peer has applied.
    #[must_use]
    pub fn revocation_standing_of(&self, key: &ActorKey) -> RevocationStanding {
        self.revocations.standing_of(key)
    }

    /// Whether a record sealed by `author` under `sealed_in` was validly authored.
    ///
    /// This is the whole of "revocation is not a history rewrite", as a comparison of two epochs:
    ///
    /// * `sealed_in` **after** the epoch in force → [`AuthorshipStanding::Indeterminate`]. This peer
    ///   has not observed that epoch and therefore has no evidence about who was revoked in it.
    ///   Fails closed; the repair is to fetch the missing rotation records and ask again.
    /// * the author is not revoked, or is revoked from an epoch **later** than `sealed_in` → the
    ///   record [`AuthorshipStanding::Stands`], and stands permanently. A rotation issued on a chain
    ///   at epoch `n` is effective at `n + 1`, which is strictly greater than any epoch an
    ///   already-sealed record can name, so no future revocation reaches back past this answer.
    /// * otherwise the record was sealed at or after its author's revocation took effect and is
    ///   [`AuthorshipStanding::AuthoredAfterRevocation`].
    ///
    /// # The window this does not close
    ///
    /// The granularity is the epoch, not the instant. Records a stolen key sealed **in the epoch the
    /// theft happened in**, before the operator rotated, keep standing — deliberately, because the
    /// alternative is deleting reviewed work on the strength of one rotation record. Those records
    /// are bounded by the central claim rather than by this function: nothing they contain reaches
    /// canonical state without a human's signature over an approval envelope. The operator remedy is
    /// review, and [`RevocationEntry::reason`](crate::RevocationEntry::reason) is what tells a review
    /// surface which keys to look at.
    #[must_use]
    pub fn authorship_standing(
        &self,
        author: &ActorKey,
        sealed_in: PolicyEpoch,
    ) -> AuthorshipStanding {
        if sealed_in > self.current {
            return AuthorshipStanding::Indeterminate {
                sealed_in,
                in_force: self.current,
            };
        }
        match self.revocations.standing_of(author) {
            RevocationStanding::NotRevoked => AuthorshipStanding::Stands,
            RevocationStanding::Revoked { effective, .. } if sealed_in < effective => {
                AuthorshipStanding::Stands
            }
            RevocationStanding::Revoked { effective, .. } => {
                AuthorshipStanding::AuthoredAfterRevocation {
                    effective,
                    sealed_in,
                }
            }
        }
    }

    /// Where `epoch` stands against the epoch in force.
    #[must_use]
    pub fn standing_of(&self, epoch: PolicyEpoch) -> EpochStanding {
        if epoch == self.current {
            EpochStanding::Current
        } else if epoch < self.current {
            EpochStanding::Superseded { by: self.current }
        } else {
            EpochStanding::Unknown {
                in_force: self.current,
            }
        }
    }

    /// Rotate the epoch, producing the new chain and the record that carries it to every peer.
    ///
    /// The record is returned rather than merely retained because rotating without publishing the
    /// record leaves the rotating peer as the only one enforcing it, which is the failure mode the
    /// epoch design exists to avoid.
    ///
    /// # Errors
    ///
    /// [`EpochError::Exhausted`] if the epoch counter would overflow. Not reachable with a `u64`;
    /// stalling is the safe answer and a silent wrap to epoch zero would revive every capability
    /// ever issued.
    pub fn rotate(
        self,
        reason: RotationReason,
        revoke: impl IntoIterator<Item = ActorKey>,
    ) -> Result<(Self, EpochRotation), EpochError> {
        let next = self
            .current
            .value()
            .checked_add(1)
            .map(PolicyEpoch::new)
            .ok_or(EpochError::Exhausted)?;
        let rotation = EpochRotation {
            from: self.current,
            to: next,
            reason,
            revoked: revoke.into_iter().collect(),
            predecessor: self.head,
        };
        let applied = self.apply(&rotation);
        Ok((applied, rotation))
    }

    /// Apply a rotation received from anywhere, after checking it extends *this* chain.
    ///
    /// This is the whole offline story: the argument is bytes a peer received, the check is against
    /// state the peer already holds, and no network is involved.
    ///
    /// # Errors
    ///
    /// [`EpochError::NotContiguous`] when the record does not leave the epoch in force — a peer
    /// that is more than one rotation behind fetches the missing records and applies them in order.
    /// [`EpochError::NotASuccessor`] when the record's target is not the next epoch.
    /// [`EpochError::ForkedHistory`] when the record extends a chain state other than this one's,
    /// which is a substituted policy history and not a gap.
    pub fn observe(self, rotation: &EpochRotation) -> Result<Self, EpochError> {
        if rotation.from != self.current {
            return Err(EpochError::NotContiguous {
                in_force: self.current,
                offered: rotation.from,
            });
        }
        if rotation.to.value() != self.current.value().saturating_add(1) {
            return Err(EpochError::NotASuccessor {
                from: rotation.from,
                to: rotation.to,
            });
        }
        if rotation.predecessor != self.head {
            return Err(EpochError::ForkedHistory);
        }
        Ok(self.apply(rotation))
    }

    /// The state transition, shared by [`EpochChain::rotate`] and [`EpochChain::observe`] so the
    /// issuing peer and the receiving peer cannot end up in different states from the same record.
    fn apply(self, rotation: &EpochRotation) -> Self {
        // A revocation becomes effective in the epoch the rotation *enters*, never the one it
        // leaves. That single choice is what keeps every record sealed under the outgoing epoch
        // valid: `rotation.to` is strictly greater than any epoch already-sealed work can name.
        let revocations = rotation
            .revoked
            .iter()
            .fold(self.revocations, |ledger, subject| {
                ledger.record(*subject, rotation.to, rotation.reason)
            });
        Self {
            current: rotation.to,
            head: rotation.digest(),
            revocations,
            rotations: self.rotations.saturating_add(1),
        }
    }
}

/// Why a rotation was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EpochError {
    /// The record leaves an epoch other than the one in force.
    NotContiguous {
        /// The epoch in force on this peer.
        in_force: PolicyEpoch,
        /// The epoch the record leaves.
        offered: PolicyEpoch,
    },
    /// The record's target epoch is not its source plus one.
    NotASuccessor {
        /// The epoch the record leaves.
        from: PolicyEpoch,
        /// The epoch the record claims to enter.
        to: PolicyEpoch,
    },
    /// The record extends a different policy history.
    ForkedHistory,
    /// The epoch counter is exhausted.
    Exhausted,
}

impl fmt::Display for EpochError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotContiguous { in_force, offered } => write!(
                formatter,
                "policy epoch {} is in force and this rotation leaves epoch {}",
                in_force.value(),
                offered.value()
            ),
            Self::NotASuccessor { from, to } => write!(
                formatter,
                "a rotation from policy epoch {} enters epoch {}, not {}",
                from.value(),
                from.value() + 1,
                to.value()
            ),
            Self::ForkedHistory => {
                formatter.write_str("this rotation extends a different policy history")
            }
            Self::Exhausted => formatter.write_str("the policy epoch counter is exhausted"),
        }
    }
}

impl std::error::Error for EpochError {}
