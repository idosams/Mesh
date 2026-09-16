//! The receipt-side identifier check: a ChangeSet identifier is recomputed from the record's bytes
//! before it may reach head advancement.
//!
//! # The obligation this discharges
//!
//! `mesh-state` derives an actor head from the applied **identifier** set. Convergence therefore
//! holds exactly when two peers holding the same identifier set hold the same causal set, which is
//! true exactly when a ChangeSet's identifier binds its causal parent set.
//! [`docs/adr/0015-actor-heads-converge-without-a-global-lock-if-an-identifier-binds-its-parent-set.md`](https://github.com/)
//! measured both halves and found the first enforced and the second enforced by nothing. Its
//! protocol commitment, quoted:
//!
//! > A peer MUST NOT admit a ChangeSet identifier into head advancement until it has recomputed
//! > that identifier from the record's canonical bytes under the protocol digest and found it
//! > equal. A record whose identifier does not match its bytes is refused, not buffered.
//!
//! [`admit`] is that sentence. ADR-0015 decision point 4 places the obligation here rather than in
//! `mesh-state`, which holds no bytes and declares no dependency; this crate holds the bytes,
//! because `CarriedChangeSet` carries them.
//!
//! # Why the check cannot be bypassed by forgetting it
//!
//! [`AdmittedChangeSet`] has no public constructor and no public field. The only way to obtain one
//! is [`admit`], which derives the identifier before it builds one. So "reached head advancement
//! without the check" is not a mistake a caller can make quietly; it is a value a caller cannot
//! construct. That is the same technique `mesh-operations` uses for `ReceivedChangeSet::verify`,
//! and it is what turns a rule into a type.
//!
//! # What the check is not
//!
//! It is not signature verification, which is `mesh-crypto`'s and is a separate check with a
//! separate failure mode: a record whose identifier matches its bytes is a record that says what
//! its author said, not a record whose author is who it claims to be. It is also not a head
//! derivation — the `resulting_head` a record carries is bound into the identifier, so this check
//! establishes that the author's claim is the author's claim, and nothing about whether the
//! transition produces it.

use mesh_sync_protocol::{
    ActorId, ActorSequence, CarriedChangeSet, ChangeSetId, HeadId, PolicyEpoch,
};
use mesh_types::{derive_id, Blake3};

use crate::decode::{rebuild, BodyRefused};
use crate::operation::OperationDecoder;

/// A ChangeSet whose identifier this peer recomputed from its bytes and found equal.
///
/// Every field is read **from the body**, never from the header fields a sender supplied beside
/// it: once the identifier is known to be the digest of the body, the body is the record and the
/// header is a hint that has already done its job.
///
/// There is no public constructor and no public field. Holding one means the check happened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdmittedChangeSet {
    id: ChangeSetId,
    author: ActorId,
    sequence: ActorSequence,
    parents: Vec<ChangeSetId>,
    base_head: HeadId,
    resulting_head: HeadId,
    policy_epoch: PolicyEpoch,
}

impl AdmittedChangeSet {
    /// The identifier, recomputed from the bytes rather than believed.
    #[must_use]
    pub const fn id(&self) -> ChangeSetId {
        self.id
    }

    /// The actor that authored it, read from the body.
    #[must_use]
    pub const fn author(&self) -> ActorId {
        self.author
    }

    /// Its position in that actor's sequence, read from the body.
    #[must_use]
    pub const fn sequence(&self) -> ActorSequence {
        self.sequence
    }

    /// Its causal parents, read from the body — the set the identifier binds.
    #[must_use]
    pub fn parents(&self) -> &[ChangeSetId] {
        &self.parents
    }

    /// The head its operations were computed against, read from the body.
    #[must_use]
    pub const fn base_head(&self) -> HeadId {
        self.base_head
    }

    /// The head its author claims it produces, read from the body. Bound into the identifier, and
    /// still a claim: deriving it is head advancement's job, not this one's.
    #[must_use]
    pub const fn resulting_head(&self) -> HeadId {
        self.resulting_head
    }

    /// The policy epoch it was authored under, read from the body.
    #[must_use]
    pub const fn policy_epoch(&self) -> PolicyEpoch {
        self.policy_epoch
    }
}

/// Why a received ChangeSet may not reach head advancement.
///
/// A refused record is not an [`AdmittedChangeSet`], so there is no way to go on using it as one,
/// and it is refused rather than buffered: a record held for later is a record that reaches head
/// advancement later.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// **The named error.** The identifier the sender claims is not the identifier its bytes
    /// derive.
    ///
    /// This is the refusal ADR-0015's condition rests on. Two records with one identifier and two
    /// causal parent sets is the n = 2 divergence in that ADR; at most one of them can survive this
    /// comparison, so two peers cannot reach two heads by applying them both.
    IdentifierMismatch {
        /// What the sender said the record is called.
        claimed: ChangeSetId,
        /// What its bytes say it is called.
        derived: ChangeSetId,
    },
    /// The body is not a canonical ChangeSet, so no identifier can be recomputed from it at all.
    UndecodableBody(BodyRefused),
    /// A header field disagrees with the body it is redundant with.
    ///
    /// `CarriedChangeSet`'s spelled-out fields exist so a receiver can plan its next request before
    /// decoding, and every one is redundant with the body beside it. A disagreement is therefore a
    /// defect in the sender rather than a difference of opinion, and it is refused so that a
    /// receiver's *plan* and a receiver's *record* can never be about two different ChangeSets.
    HeaderDisagreesWithBody {
        /// Which field, by the name `CarriedChangeSet` gives it.
        field: &'static str,
    },
}

impl core::fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::IdentifierMismatch { claimed, derived } => write!(
                formatter,
                "the record calls itself {} and its bytes derive {}",
                claimed.to_hex(),
                derived.to_hex()
            ),
            Self::UndecodableBody(why) => write!(formatter, "{why}"),
            Self::HeaderDisagreesWithBody { field } => write!(
                formatter,
                "the carried {field} disagrees with the body it is redundant with"
            ),
        }
    }
}

impl std::error::Error for Refusal {}

/// Recompute a carried ChangeSet's identifier from its bytes and admit it only if they agree.
///
/// The identifier is `derive_id::<Blake3, _>` over the ChangeSet the body encodes — the same
/// derivation `mesh-types` uses to name a record, reached through the same public function, so
/// there is no second implementation of identity here to disagree with the first.
///
/// # Errors
///
/// [`Refusal`] naming why the record may not reach head advancement. The record is refused, never
/// buffered.
pub fn admit<D: OperationDecoder>(
    carried: &CarriedChangeSet,
    decoder: &D,
) -> Result<AdmittedChangeSet, Refusal> {
    let record = rebuild(&carried.body, decoder).map_err(Refusal::UndecodableBody)?;
    let derived = derive_id::<Blake3, _>(&record);
    let derived = ChangeSetId::from_bytes(*derived.digest().as_bytes());

    if derived != carried.id {
        return Err(Refusal::IdentifierMismatch {
            claimed: carried.id,
            derived,
        });
    }

    let author = ActorId::from_bytes(*record.actor_id().digest().as_bytes());
    let sequence = ActorSequence::new(record.actor_sequence().value());
    let parents: Vec<ChangeSetId> = record
        .causal_parents()
        .as_slice()
        .iter()
        .map(|parent| ChangeSetId::from_bytes(*parent.digest().as_bytes()))
        .collect();
    let base_head = HeadId::from_bytes(*record.base_head().digest().as_bytes());
    let resulting_head = HeadId::from_bytes(*record.resulting_head().digest().as_bytes());
    let policy_epoch = PolicyEpoch::new(record.policy_epoch().value());

    // The redundant fields are checked after the identifier, not before: an identifier mismatch is
    // the refusal that matters, and reporting a header disagreement first would name the symptom.
    check_header(carried, "author", author == carried.author)?;
    check_header(carried, "sequence", sequence == carried.sequence)?;
    check_header(carried, "parents", parents == carried.parents)?;
    check_header(carried, "base_head", base_head == carried.base_head)?;
    check_header(
        carried,
        "resulting_head",
        resulting_head == carried.resulting_head,
    )?;
    check_header(
        carried,
        "policy_epoch",
        policy_epoch == carried.policy_epoch,
    )?;

    Ok(AdmittedChangeSet {
        id: derived,
        author,
        sequence,
        parents,
        base_head,
        resulting_head,
        policy_epoch,
    })
}

fn check_header(
    _carried: &CarriedChangeSet,
    field: &'static str,
    agrees: bool,
) -> Result<(), Refusal> {
    if agrees {
        Ok(())
    } else {
        Err(Refusal::HeaderDisagreesWithBody { field })
    }
}
