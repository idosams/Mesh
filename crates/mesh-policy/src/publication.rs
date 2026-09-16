//! The publication guard: the only path in Mesh that ends in canonical state advancing.
//!
//! # The claim, and where it is enforced
//!
//! `docs/protocol.md` TG-3: no agent-scoped key can produce a valid approval envelope, and the
//! capability is **unrepresentable, not merely denied**. This module is where that sentence is a
//! function signature.
//!
//! ```text
//! fn authorize_publication(
//!     chain:      &EpochChain,
//!     approver:   &HumanPrincipal,        // mesh-types: only `human` gets past `enrol`
//!     capability: &Capability<HumanHeld>, // mesh-crypto: no delegation produces this type
//!     request:    &AuthorityRequest,
//! ) -> Result<PublicationAuthority, Denial>
//! ```
//!
//! An agent holds a `Capability<Delegated>`. Passing one here is a **type error at the call site** —
//! `expected Capability<HumanHeld>, found Capability<Delegated>` — not a denial at run time. There is
//! no conversion between the two: `AuthorityTier::Delegated` is `Delegated` for both tiers, so no
//! chain of `delegate` calls climbs to `HumanHeld`, and the only constructor of a `HumanHeld`
//! capability needs a `HumanKeyAttestation` that only an implementation of `HumanKeyCustody` can
//! produce.
//!
//! Even holding a `Capability<HumanHeld>`, an agent process gets no further: the action set inside
//! it must contain `HumanAction::AdvanceCanonicalHead`, the approver must be an
//! [`ActorKind::Human`](mesh_types::ActorKind::Human), the approver must **be** the capability's
//! subject, and the epoch must be the one in force.
//!
//! # What a `PublicationAuthority` is and is not
//!
//! It is a witness that the policy layer said yes. It is **not** a signature, and it is not
//! sufficient to advance canonical state on its own: the head advances on a human key's signature
//! over an approval envelope, and `mesh-crypto` holds no secret with which to make one. This type
//! is the gate the approval path must pass through; the cryptography is the lock behind it.
//!
//! There is no public constructor, no public field, no `Default` and no `Clone`-from-nothing. The
//! only value of this type in existence is one the ledger produced.

use core::fmt;

use mesh_crypto::{Capability, HumanAction, HumanHeld, WorkspaceScope};
use mesh_types::PolicyEpoch;

use crate::decision::{check_preconditions, AuthorityRequest, Denial, DenialReason, Operation};
use crate::epoch::EpochChain;
use crate::principal::HumanPrincipal;

/// Proof that one named person is authorized to advance the protected shared version, once, in one
/// workspace, under one policy epoch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PublicationAuthority {
    approver: mesh_crypto::ActorKey,
    workspace: WorkspaceScope,
    epoch: PolicyEpoch,
}

impl PublicationAuthority {
    /// The person who is authorized. Never an agent: the argument that produced this value is a
    /// [`HumanPrincipal`], which only `ActorKind::Human` gets past.
    #[must_use]
    pub const fn approver(&self) -> mesh_crypto::ActorKey {
        self.approver
    }

    /// The workspace it is good in.
    #[must_use]
    pub const fn workspace(&self) -> WorkspaceScope {
        self.workspace
    }

    /// The policy epoch it was decided under.
    ///
    /// An approval path must carry this into the approval envelope's policy-epoch binding: an
    /// authority decided in epoch `n` and applied after a rotation to `n+1` is a stale approval,
    /// and the epoch is what lets the applying side notice.
    #[must_use]
    pub const fn epoch(&self) -> PolicyEpoch {
        self.epoch
    }
}

impl fmt::Display for PublicationAuthority {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "publication authority for {} in policy epoch {}",
            self.approver,
            self.epoch.value()
        )
    }
}

/// Decide whether one human may advance canonical state.
///
/// Deliberately **not public**. The only caller is
/// [`DecisionLedger::authorize_publication`](crate::DecisionLedger::authorize_publication), so a
/// publication decision — granted or refused — cannot happen without a record of it.
pub(crate) fn authorize_publication(
    chain: &EpochChain,
    approver: &HumanPrincipal,
    capability: &Capability<HumanHeld>,
    request: &AuthorityRequest,
) -> Result<PublicationAuthority, Denial> {
    let in_force = chain.current();
    let deny = |reason| {
        Err(Denial::new(
            request.subject(),
            request.operation(),
            in_force,
            reason,
        ))
    };

    // This guard decides one operation. A request naming another arrived at the wrong gate, and
    // answering it here would let a caller obtain publication authority while asking for a read.
    if request.operation() != Operation::AdvanceCanonicalHead {
        return deny(DenialReason::ActionNotGranted {
            operation: request.operation(),
        });
    }
    // The authenticated peer, the person, and the capability's subject must be one actor. Two of
    // the three agreeing is how a stolen human capability gets replayed by its thief.
    if approver.key() != request.subject() {
        return deny(DenialReason::ApproverMismatch);
    }
    if *capability.subject() != approver.key() {
        return deny(DenialReason::ApproverMismatch);
    }
    check_preconditions(chain, capability, request)?;
    if !capability.grants(HumanAction::AdvanceCanonicalHead) {
        return deny(DenialReason::ActionNotGranted {
            operation: Operation::AdvanceCanonicalHead,
        });
    }
    Ok(PublicationAuthority {
        approver: approver.key(),
        workspace: capability.workspace(),
        epoch: in_force,
    })
}
