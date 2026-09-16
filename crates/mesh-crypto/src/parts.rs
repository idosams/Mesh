//! The inert, tier-agnostic shape a capability travels as — and why the codec seam speaks it
//! instead of speaking [`Capability`](crate::Capability).
//!
//! # The hole this closes
//!
//! The obvious design gives the codec `decode<T>(bytes) -> Capability<T>`. It is also the design
//! that quietly hands every crate in the workspace a way to *mint* a `Capability<HumanHeld>`
//! carrying [`HumanAction::AdvanceCanonicalHead`](crate::HumanAction): a constructor reachable
//! from bytes is a constructor reachable from anywhere, and "an agent-issued capability cannot be
//! constructed with canonical-advance authority" stops being true the moment one exists.
//!
//! So the codec never sees a [`Capability`](crate::Capability). It encodes and decodes
//! [`CapabilityParts`], which grants nothing — it is field values, the same way a form is not a
//! passport. Turning parts into a capability is `pub(crate)` and happens in exactly one place:
//! after [`CapabilityToken::verify`](crate::CapabilityToken::verify) has checked the issuer's
//! signature over those exact bytes.
//!
//! # And the tier is checked by parsing, not by comparing
//!
//! `Capability::<Delegated>::from_parts` parses each action name through
//! [`AuthorityTier::parse_action`](crate::AuthorityTier::parse_action). For the delegated tier that
//! is [`DelegatedAction::parse`](crate::DelegatedAction::parse), which returns `None` for
//! `"advance-canonical-head"` — not because it rejects the string but because the enumeration has
//! no value to return. A hostile token claiming publication authority for an agent therefore fails
//! to *decode*. There is no later authorization check that could be forgotten.

use core::fmt;

use mesh_types::PolicyEpoch;

use crate::capability::{DelegationBudget, Expiry, WorkspaceScope};
use crate::keys::ActorKey;

/// One capability's field values, as they travel: names and numbers, no authority.
///
/// Constructing one is harmless by design. It is not a capability, it does not implement any trait
/// a policy check consumes, and the only thing that turns it into a capability is a verified
/// signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilityParts {
    issuer: ActorKey,
    subject: ActorKey,
    workspace: WorkspaceScope,
    tier: String,
    actions: Vec<String>,
    policy_epoch: PolicyEpoch,
    not_after: Expiry,
    budget: DelegationBudget,
}

impl CapabilityParts {
    /// Assemble the field values a codec reads and writes.
    ///
    /// Eight arguments, because a capability binds eight things and a decoder has to supply every
    /// one of them. A builder would let a codec forget one and get a default — and a defaulted
    /// expiry or policy epoch is exactly the field an attacker wants left off.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        issuer: ActorKey,
        subject: ActorKey,
        workspace: WorkspaceScope,
        tier: String,
        mut actions: Vec<String>,
        policy_epoch: PolicyEpoch,
        not_after: Expiry,
        budget: DelegationBudget,
    ) -> Self {
        // A capability carries a set of actions. Normalize it at the inert-parts boundary so the
        // canonical payload has one order and cannot spell the same grant twice with duplicates.
        actions.sort();
        actions.dedup();
        Self {
            issuer,
            subject,
            workspace,
            tier,
            actions,
            policy_epoch,
            not_after,
            budget,
        }
    }

    /// Who issued the capability.
    #[must_use]
    pub const fn issuer(&self) -> &ActorKey {
        &self.issuer
    }

    /// Whose authority it is.
    #[must_use]
    pub const fn subject(&self) -> &ActorKey {
        &self.subject
    }

    /// The workspace it is scoped to.
    #[must_use]
    pub const fn workspace(&self) -> WorkspaceScope {
        self.workspace
    }

    /// The authority tier's wire name.
    #[must_use]
    pub fn tier(&self) -> &str {
        &self.tier
    }

    /// The action names granted.
    #[must_use]
    pub fn actions(&self) -> &[String] {
        &self.actions
    }

    /// The policy epoch.
    #[must_use]
    pub const fn policy_epoch(&self) -> PolicyEpoch {
        self.policy_epoch
    }

    /// The expiry.
    #[must_use]
    pub const fn not_after(&self) -> Expiry {
        self.not_after
    }

    /// The delegation budget.
    #[must_use]
    pub const fn budget(&self) -> DelegationBudget {
        self.budget
    }
}

/// Why decoded field values are not a capability at the requested tier.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PartsError {
    /// The parts name a different authority tier.
    TierMismatch {
        /// The tier the parts name.
        found: String,
        /// The tier that was asked for.
        expected: &'static str,
    },
    /// An action name is not in the requested tier's vocabulary. This is what
    /// `"advance-canonical-head"` produces at the delegated tier.
    UnknownAction {
        /// The name that did not resolve.
        name: String,
        /// The tier it was resolved against.
        tier: &'static str,
    },
    /// The parts grant no action. A capability granting nothing is attack surface with no use.
    EmptyGrant,
}

impl fmt::Display for PartsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TierMismatch { found, expected } => write!(
                formatter,
                "these field values name the `{found}` tier and `{expected}` was asked for"
            ),
            Self::UnknownAction { name, tier } => write!(
                formatter,
                "`{name}` is not an action the `{tier}` tier can hold"
            ),
            Self::EmptyGrant => formatter.write_str("a capability grants at least one action"),
        }
    }
}

impl std::error::Error for PartsError {}
