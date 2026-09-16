//! The decision ledger — and why it is the only door to the gates.
//!
//! # "Every rejection is recorded with its reason" as a structure, not a habit
//!
//! [`crate::decision::authorize`] and [`crate::publication::authorize_publication`] are
//! `pub(crate)`. The only public callers are [`DecisionLedger::authorize`] and
//! [`DecisionLedger::authorize_publication`], and both return a new ledger alongside the outcome.
//! A caller therefore cannot reach a decision without also receiving the record of it: there is no
//! "log it if you remember" path, because there is no path that skips the ledger.
//!
//! Both methods take `self` by value and return a new [`DecisionLedger`]. Nothing here takes
//! `&mut self` and nothing removes an entry — a ledger somebody can rewrite is a ledger an attacker
//! rewrites after the denial that named them.
//!
//! # Ordering
//!
//! Entries are ordered by [`DecisionRecord::sequence`], which counts decisions on this ledger. No
//! wall clock is read and none is stored. The `now` a request carries is the value the expiry check
//! compared against — evidence about the decision, not a position in the order.

use core::fmt;

use mesh_crypto::{AuthorityTier, Capability, HumanHeld};
use mesh_types::PolicyEpoch;

use crate::decision::{authorize, AuthorityRequest, Denial, DenialReason, Grant, Operation};
use crate::epoch::EpochChain;
use crate::principal::HumanPrincipal;
use crate::publication::{authorize_publication, PublicationAuthority};
use mesh_crypto::ActorKey;

/// What a gate decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The request was authorized.
    Granted,
    /// The request was refused, for this reason.
    Denied(DenialReason),
}

impl Outcome {
    /// Whether this outcome authorized anything.
    #[must_use]
    pub const fn is_granted(&self) -> bool {
        matches!(self, Self::Granted)
    }

    /// The reason, when there was one.
    #[must_use]
    pub const fn reason(&self) -> Option<DenialReason> {
        match self {
            Self::Granted => None,
            Self::Denied(reason) => Some(*reason),
        }
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Granted => formatter.write_str("granted"),
            Self::Denied(reason) => write!(formatter, "denied: {reason}"),
        }
    }
}

/// One decision, as it was made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecisionRecord {
    sequence: u64,
    subject: ActorKey,
    operation: Operation,
    in_force: PolicyEpoch,
    tier: &'static str,
    outcome: Outcome,
}

impl DecisionRecord {
    /// This decision's position in the ledger. Ordering is by this number and by nothing else.
    #[must_use]
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Who asked.
    #[must_use]
    pub const fn subject(&self) -> ActorKey {
        self.subject
    }

    /// What they asked for.
    #[must_use]
    pub const fn operation(&self) -> Operation {
        self.operation
    }

    /// The policy epoch in force when it was decided.
    #[must_use]
    pub const fn in_force(&self) -> PolicyEpoch {
        self.in_force
    }

    /// The authority tier the request was decided at.
    #[must_use]
    pub const fn tier(&self) -> &'static str {
        self.tier
    }

    /// What was decided, and why if it was a refusal.
    #[must_use]
    pub const fn outcome(&self) -> Outcome {
        self.outcome
    }
}

impl fmt::Display for DecisionRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "#{} {} {} ({} tier, policy epoch {}): {}",
            self.sequence,
            self.subject,
            self.operation,
            self.tier,
            self.in_force.value(),
            self.outcome
        )
    }
}

/// Every authority decision this peer has made, in the order it made them.
///
/// Append-only and immutable. The gates are reached only through it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DecisionLedger {
    entries: Vec<DecisionRecord>,
}

impl DecisionLedger {
    /// A ledger with no decisions in it.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Every decision, oldest first.
    #[must_use]
    pub fn entries(&self) -> &[DecisionRecord] {
        &self.entries
    }

    /// How many decisions have been made.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no decision has been made.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every refusal, oldest first.
    pub fn denials(&self) -> impl Iterator<Item = &DecisionRecord> {
        self.entries
            .iter()
            .filter(|record| !record.outcome.is_granted())
    }

    /// The most recent decision.
    #[must_use]
    pub fn last(&self) -> Option<&DecisionRecord> {
        self.entries.last()
    }

    /// Decide one request, and record the decision.
    ///
    /// Returns the extended ledger and the outcome. Both, always: there is no variant of this call
    /// that decides without recording.
    ///
    /// This gate never grants [`Operation::AdvanceCanonicalHead`] at any tier — see
    /// [`crate::decision`]. Publication goes through [`DecisionLedger::authorize_publication`].
    #[must_use = "the returned ledger carries the record of this decision; dropping it loses the audit line"]
    pub fn authorize<T: AuthorityTier>(
        self,
        chain: &EpochChain,
        capability: &Capability<T>,
        request: &AuthorityRequest,
    ) -> (Self, Result<Grant<T>, Denial>) {
        let outcome = authorize(chain, capability, request);
        let recorded = self.record(
            request,
            chain.current(),
            T::NAME,
            match &outcome {
                Ok(_) => Outcome::Granted,
                Err(denial) => Outcome::Denied(denial.reason()),
            },
        );
        (recorded, outcome)
    }

    /// Decide whether one human may advance canonical state, and record the decision.
    ///
    /// The signature is the enforcement: `capability` is a `Capability<HumanHeld>` and `approver` is
    /// a [`HumanPrincipal`]. An agent holds neither, and neither can be produced from what an agent
    /// holds.
    #[must_use = "the returned ledger carries the record of this decision; dropping it loses the audit line"]
    pub fn authorize_publication(
        self,
        chain: &EpochChain,
        approver: &HumanPrincipal,
        capability: &Capability<HumanHeld>,
        request: &AuthorityRequest,
    ) -> (Self, Result<PublicationAuthority, Denial>) {
        let outcome = authorize_publication(chain, approver, capability, request);
        let recorded = self.record(
            request,
            chain.current(),
            HumanHeld::NAME,
            match &outcome {
                Ok(_) => Outcome::Granted,
                Err(denial) => Outcome::Denied(denial.reason()),
            },
        );
        (recorded, outcome)
    }

    /// Append one record. Private: an entry that did not come from a gate is a fabricated audit
    /// line, and this type would be the thing that made fabricating one easy.
    fn record(
        self,
        request: &AuthorityRequest,
        in_force: PolicyEpoch,
        tier: &'static str,
        outcome: Outcome,
    ) -> Self {
        let sequence = self.entries.len() as u64;
        let mut entries = self.entries;
        entries.push(DecisionRecord {
            sequence,
            subject: request.subject(),
            operation: request.operation(),
            in_force,
            tier,
            outcome,
        });
        Self { entries }
    }
}
