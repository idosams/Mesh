//! Reconnect admission: a peer applies the policy records it was offered **before** it admits one
//! operation, and the ordering is a type, not a convention.
//!
//! # The gap this closes
//!
//! An agent key leaks. The operator rotates the epoch and the rotation record starts propagating.
//! A peer that has been offline for a week reconnects and is handed, in the same session, both the
//! rotation record that revokes the key and a batch of operations signed by it. If that peer
//! processes operations first — or processes them concurrently, or applies policy lazily on the
//! first check that needs it — the revocation misses the very batch it exists to stop, and the
//! attacker's best move is simply to keep the victim peer offline until they have work to push.
//!
//! # Why this is a type and not a check
//!
//! [`PendingSession`] has no `admit` method. Not one that refuses: **none**. The only way to obtain
//! an [`AdmittingSession`] is [`PendingSession::catch_up`], which consumes the pending session, and
//! [`AdmittingSession`] has no public constructor. So "admitted an operation before applying the
//! policy records" is not a bug this module rejects; it is a program that does not compile.
//!
//! ```compile_fail,E0599
//! # use mesh_policy::{DecisionLedger, EpochChain, PendingSession};
//! # use mesh_types::PolicyEpoch;
//! # fn demo(capability: &mesh_crypto::Capability<mesh_crypto::Delegated>,
//! #         request: &mesh_policy::AuthorityRequest) {
//! let session = PendingSession::opening(EpochChain::genesis(PolicyEpoch::new(1)));
//! // no method named `admit` found for struct `PendingSession`
//! let _ = session.admit(DecisionLedger::empty(), capability, request);
//! # }
//! ```
//!
//! # Silence is not agreement
//!
//! Applying whatever records arrive is not enough on its own, because the cheapest attack on a
//! propagating revocation is to **withhold** it: a hostile relay offers zero rotations, the peer
//! applies zero rotations, and the peer concludes it is up to date. [`PendingSession::catch_up`]
//! therefore requires a [`PolicyHeadAssertion`] — what the partner says the chain is — and refuses
//! when the records it was given do not reach it ([`CatchUpError::RecordsWithheld`]) or reach a
//! different history at the same epoch ([`CatchUpError::ForkedPolicyHistory`]).
//!
//! A peer that is **ahead** of the assertion is not an error: it has already applied rotations the
//! partner has not seen, so it already enforces at least as much as the assertion asks for. Only
//! being behind is refused, and only the assertion's own authenticity is out of scope here — see
//! *What this does not claim*.
//!
//! # What this does not claim
//!
//! * **It is not a global interlock.** [`DecisionLedger::authorize`](crate::DecisionLedger::authorize)
//!   remains callable with any [`EpochChain`] a caller holds. This type makes the ordering
//!   unrepresentable *for a session that uses it*; it does not stop a second code path from deciding
//!   against a stale chain. Making that unrepresentable workspace-wide is the relay's construction,
//!   not this crate's.
//! * **It does not authenticate the assertion.** A [`PolicyHeadAssertion`] is the partner's claim and
//!   carries no signature. It must arrive on a channel that authenticated the partner; an
//!   unauthenticated assertion downgrades this check to "the partner and I agree", which detects a
//!   careless relay and not a hostile one. Signing the head is `services/relay` work.
//! * **Propagation is not instant.** A revocation binds on a peer when that peer applies the
//!   rotation record. The window is stated in `services/relay/README.md`; this module makes the
//!   window end at a defined moment rather than at "whenever the check happens to run".
//!
//! # Ordering
//!
//! No wall clock. A session's progress is measured in rotations applied and in the epoch reached.

use core::fmt;

use mesh_crypto::{AuthorityTier, Capability};
use mesh_types::{Digest32, PolicyEpoch};

use crate::decision::{AuthorityRequest, Denial, Grant};
use crate::epoch::{EpochChain, EpochError, EpochRotation};
use crate::ledger::DecisionLedger;

/// What a sync partner says the policy chain is: the epoch in force and the chain head digest.
///
/// Both fields, not just the digest. The epoch is what distinguishes "you are behind me" from "you
/// are ahead of me", and the digest is what distinguishes "we agree" from "we are on different
/// policy histories at the same epoch" — which is a substituted policy and not a gap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PolicyHeadAssertion {
    epoch: PolicyEpoch,
    head: Digest32,
}

impl PolicyHeadAssertion {
    /// The partner asserts this epoch and this chain head.
    #[must_use]
    pub const fn new(epoch: PolicyEpoch, head: Digest32) -> Self {
        Self { epoch, head }
    }

    /// The asserted epoch.
    #[must_use]
    pub const fn epoch(&self) -> PolicyEpoch {
        self.epoch
    }

    /// The asserted chain head.
    #[must_use]
    pub const fn head(&self) -> Digest32 {
        self.head
    }

    /// The assertion a peer makes about its own chain, so the two directions of a session are
    /// symmetric and neither side has to hand-assemble one.
    #[must_use]
    pub fn of(chain: &EpochChain) -> Self {
        Self::new(chain.current(), chain.head())
    }
}

/// Why a reconnect refused to reach the admitting state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatchUpError {
    /// One of the offered records did not extend this peer's chain.
    Rotation(EpochError),
    /// The offered records did not reach the epoch the partner asserted. Either the partner
    /// withheld records or the batch was truncated in transit; both are refused identically,
    /// because a peer cannot tell them apart and both leave a revocation unapplied.
    RecordsWithheld {
        /// The epoch this peer reached by applying what it was offered.
        reached: PolicyEpoch,
        /// The epoch the partner asserted.
        asserted: PolicyEpoch,
    },
    /// The offered records reached the asserted epoch by a different history. A substituted policy
    /// chain, not a gap.
    ForkedPolicyHistory {
        /// The epoch both sides claim.
        at: PolicyEpoch,
        /// The head this peer computed.
        reached: Digest32,
        /// The head the partner asserted.
        asserted: Digest32,
    },
}

impl fmt::Display for CatchUpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rotation(error) => write!(formatter, "{error}"),
            Self::RecordsWithheld { reached, asserted } => write!(
                formatter,
                "the offered policy records reach epoch {} and the partner asserts epoch {}",
                reached.value(),
                asserted.value()
            ),
            Self::ForkedPolicyHistory {
                at,
                reached,
                asserted,
            } => write!(
                formatter,
                "policy epoch {} is {reached} here and {asserted} at the partner",
                at.value()
            ),
        }
    }
}

impl std::error::Error for CatchUpError {}

impl From<EpochError> for CatchUpError {
    fn from(error: EpochError) -> Self {
        Self::Rotation(error)
    }
}

/// A reconnected session that has not yet applied the policy records it was offered.
///
/// It cannot admit anything. See the module documentation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingSession {
    chain: EpochChain,
}

impl PendingSession {
    /// Open a session against the policy chain this peer went offline with.
    #[must_use]
    pub const fn opening(chain: EpochChain) -> Self {
        Self { chain }
    }

    /// The chain as it stands before catching up.
    #[must_use]
    pub const fn chain(&self) -> &EpochChain {
        &self.chain
    }

    /// Apply every offered rotation, in order, and check the result against what the partner
    /// asserts. Consumes the session; this is the only way an [`AdmittingSession`] comes to exist.
    ///
    /// # Errors
    ///
    /// [`CatchUpError::Rotation`] when a record does not extend this peer's chain — a gap, a skip or
    /// a fork, named by [`EpochError`]. [`CatchUpError::RecordsWithheld`] when the applied records
    /// do not reach the asserted epoch. [`CatchUpError::ForkedPolicyHistory`] when they reach it by
    /// a different history.
    pub fn catch_up(
        self,
        offered: &[EpochRotation],
        asserted: PolicyHeadAssertion,
    ) -> Result<AdmittingSession, CatchUpError> {
        let mut chain = self.chain;
        let applied = offered.len();
        for rotation in offered {
            chain = chain.observe(rotation)?;
        }
        if chain.current() < asserted.epoch() {
            return Err(CatchUpError::RecordsWithheld {
                reached: chain.current(),
                asserted: asserted.epoch(),
            });
        }
        if chain.current() == asserted.epoch() && chain.head() != asserted.head() {
            return Err(CatchUpError::ForkedPolicyHistory {
                at: chain.current(),
                reached: chain.head(),
                asserted: asserted.head(),
            });
        }
        Ok(AdmittingSession { chain, applied })
    }
}

/// A session that has applied its policy records and may now decide operations.
///
/// No public constructor and no public field: every value of this type came out of
/// [`PendingSession::catch_up`], so holding one is itself the evidence that catch-up happened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdmittingSession {
    chain: EpochChain,
    applied: usize,
}

impl AdmittingSession {
    /// The chain in force for this session, after catch-up.
    #[must_use]
    pub const fn chain(&self) -> &EpochChain {
        &self.chain
    }

    /// How many rotation records this session applied before admitting anything.
    #[must_use]
    pub const fn applied_rotations(&self) -> usize {
        self.applied
    }

    /// Decide one operation against the caught-up chain, recording the decision.
    ///
    /// Delegates to [`DecisionLedger::authorize`] rather than re-implementing any check: a second
    /// copy of the revocation test is a second place for it to stop biting, and the copy that stops
    /// biting is the one somebody wrote second.
    #[must_use = "the returned ledger carries the record of this decision; dropping it loses the audit line"]
    pub fn admit<T: AuthorityTier>(
        &self,
        ledger: DecisionLedger,
        capability: &Capability<T>,
        request: &AuthorityRequest,
    ) -> (DecisionLedger, Result<Grant<T>, Denial>) {
        ledger.authorize(&self.chain, capability, request)
    }

    /// The chain to keep for the next session.
    #[must_use]
    pub fn into_chain(self) -> EpochChain {
        self.chain
    }
}
