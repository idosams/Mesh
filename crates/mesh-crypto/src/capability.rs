//! Capabilities, and the reason an agent cannot hold publication authority.
//!
//! # The claim
//!
//! `docs/protocol.md` TG-3: "No agent-scoped key can produce a valid approval envelope. The
//! capability is **unrepresentable**, not merely denied." TG-2: "A capability cannot be widened
//! after issuance. Widening is unrepresentable in the type, not merely rejected at runtime."
//!
//! Those two words — *unrepresentable*, twice — are what this module is. A runtime check that
//! rejects `AdvanceCanonicalHead` in an agent's action set is a line of code somebody can delete,
//! forget on a second code path, or bypass by deserializing straight into the struct. What follows
//! instead is an action vocabulary in which the agent's variant **does not exist**.
//!
//! # How
//!
//! Two action enumerations, one per authority tier:
//!
//! * [`DelegatedAction`] — everything an agent, an automation, a validator, a service or a device
//!   actor can ever be granted. **There is no canonical-advance variant in it.** Not commented out,
//!   not `#[doc(hidden)]`: absent. `DelegatedAction::ALL` is exhaustive by construction, so adding
//!   one fails to compile until a lane also adds it to that list, where the guard test sees it.
//! * [`HumanAction`] — every [`DelegatedAction`], plus [`HumanAction::AdvanceCanonicalHead`].
//!
//! A [`Capability<T>`] carries `T::Action`. `Capability<Delegated>` therefore carries
//! `DelegatedAction`s, and there is no value of that type that names canonical-head advancement. It
//! is not that the constructor refuses; it is that the argument cannot be written.
//!
//! # Delegation only ever moves down
//!
//! [`Capability::delegate`] is the **only** way to derive one capability from another. There is no
//! `widen`, no `with_action`, no `set_actions`, and no public constructor that takes an action set
//! and a parent. Every field of the result either equals the parent's or is strictly narrower:
//!
//! | Field | Rule |
//! |---|---|
//! | tier | `T::Delegated`, which is [`Delegated`] for **both** tiers — the lattice has no way up |
//! | actions | a subset of what the parent holds, lifted through `T::lift` |
//! | workspace | identical; a delegation cannot move to another workspace |
//! | policy epoch | identical; a capability is valid in one epoch, and revocation rotates the epoch |
//! | expiry | at or before the parent's |
//! | delegation budget | strictly less than the parent's, so a chain is finite |
//!
//! # What this does *not* claim
//!
//! A capability is not the cryptographic control. The head advances on a **signature** made by a
//! human's key over an approval envelope, and no capability substitutes for one. This module makes
//! an over-broad grant unwritable, which is defence in depth over key custody; it does not, and
//! cannot, stop a crate compiled into the same binary from implementing
//! [`HumanKeyCustody`](crate::HumanKeyCustody) and attesting to a key of its own choosing.
//!
//! **Nothing in this workspace bounds that set of implementations**, and
//! [`HumanKeyCustody`](crate::HumanKeyCustody) states that limit in its own docs rather than
//! naming a control that is not there. What an implementation cannot do without the person's
//! private half is produce a valid signature, and a signature is what the head advances on.

use core::fmt;
use std::collections::BTreeSet;

use mesh_types::PolicyEpoch;

use crate::keys::ActorKey;
use crate::parts::{CapabilityParts, PartsError};

mod sealed {
    /// Closes [`super::AuthorityTier`]. A third tier is a threat-model change and a decision doc.
    pub trait Sealed {}
    impl Sealed for super::Delegated {}
    impl Sealed for super::HumanHeld {}
}

/// An authority-bearing action that **any** actor kind can be granted.
///
/// Canonical-head advancement is absent from this enumeration and that absence is the enforcement
/// of TG-3. Adding a variant here grants it to every agent in the system; adding one that advances
/// canonical state is the one change this file exists to prevent, and
/// `no_canonical_advance_in_the_delegated_vocabulary` is the test that fires when somebody tries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum DelegatedAction {
    /// Read the workspace's materialized state.
    ReadWorkspace,
    /// Write to the holder's own private actor state.
    WriteOwnActorState,
    /// Author a ChangeSet against the holder's own actor head.
    AuthorChangeSet,
    /// Replicate private state to and from peers.
    Replicate,
    /// Assemble a review bundle and ask a human to review it.
    RequestReview,
    /// Record read observations in the context ledger.
    RecordContextRead,
    /// Execute a validation run and publish its result as evidence.
    RunValidation,
}

impl DelegatedAction {
    /// Every delegable action. Exhaustive by construction: the match below stops compiling if a
    /// variant is added without being listed here.
    pub const ALL: [Self; 7] = [
        Self::ReadWorkspace,
        Self::WriteOwnActorState,
        Self::AuthorChangeSet,
        Self::Replicate,
        Self::RequestReview,
        Self::RecordContextRead,
        Self::RunValidation,
    ];

    /// The wire name of this action.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::ReadWorkspace => "read-workspace",
            Self::WriteOwnActorState => "write-own-actor-state",
            Self::AuthorChangeSet => "author-change-set",
            Self::Replicate => "replicate",
            Self::RequestReview => "request-review",
            Self::RecordContextRead => "record-context-read",
            Self::RunValidation => "run-validation",
        }
    }

    /// Parse a wire name. Total, and `None` for every name this vocabulary does not contain —
    /// including `advance-canonical-head`, which is how a delegated token that claims publication
    /// authority fails to decode rather than decoding into something that has to be checked.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.as_str() == name)
    }
}

impl fmt::Display for DelegatedAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// An action a **human-held** capability can carry: everything delegable, plus the one thing that
/// is not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HumanAction {
    /// An action a human may also delegate onward.
    Delegated(DelegatedAction),
    /// Advance the protected shared version. Held only by a human, never delegable, and absent
    /// from [`DelegatedAction`] so that no delegation can produce it.
    AdvanceCanonicalHead,
}

impl HumanAction {
    /// The wire name of this action.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Delegated(action) => action.as_str(),
            Self::AdvanceCanonicalHead => "advance-canonical-head",
        }
    }

    /// Parse a wire name.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        if name == "advance-canonical-head" {
            return Some(Self::AdvanceCanonicalHead);
        }
        DelegatedAction::parse(name).map(Self::Delegated)
    }
}

impl fmt::Display for HumanAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One rung of the authority lattice. Sealed, and the lattice has exactly two rungs.
pub trait AuthorityTier: sealed::Sealed + Copy + PartialEq + Eq + fmt::Debug + 'static {
    /// The action vocabulary a capability at this tier carries.
    type Action: Copy + Ord + fmt::Debug + fmt::Display;

    /// The tier this tier delegates to. [`Delegated`] for both, which is the lattice having no way
    /// up: no chain of `delegate` calls can reach [`HumanHeld`] from anywhere.
    type Delegated: AuthorityTier<Action = DelegatedAction>;

    /// The tier's wire name.
    const NAME: &'static str;

    /// Whether a capability at this tier can carry canonical-head advancement at all.
    const MAY_ADVANCE_CANONICAL_HEAD: bool;

    /// Lift a delegable action into this tier's vocabulary, so a narrowing check can ask "does the
    /// parent hold this?" without knowing which tier the parent is.
    fn lift(action: DelegatedAction) -> Self::Action;

    /// Whether `action` is the canonical-advance action of this tier. Constant `false` for
    /// [`Delegated`], because there is no such value to compare against.
    fn is_canonical_advance(action: Self::Action) -> bool;

    /// Parse a wire action name into this tier's vocabulary.
    ///
    /// The load-bearing method on the decode path. `Delegated::parse_action`
    /// (`"advance-canonical-head"`) is `None`, not because it checks the name but because
    /// [`DelegatedAction`] has no value to return — so a delegated capability token claiming
    /// publication authority fails to decode, and there is no later check to forget.
    fn parse_action(name: &str) -> Option<Self::Action>;
}

/// The tier every non-human actor holds: agents, agent-runs, automations, validators, services and
/// device actors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Delegated;

/// The tier only a person's own key holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HumanHeld;

impl AuthorityTier for Delegated {
    type Action = DelegatedAction;
    type Delegated = Delegated;
    const NAME: &'static str = "delegated";
    const MAY_ADVANCE_CANONICAL_HEAD: bool = false;

    fn lift(action: DelegatedAction) -> Self::Action {
        action
    }

    fn is_canonical_advance(_action: Self::Action) -> bool {
        false
    }

    fn parse_action(name: &str) -> Option<Self::Action> {
        DelegatedAction::parse(name)
    }
}

impl AuthorityTier for HumanHeld {
    type Action = HumanAction;
    type Delegated = Delegated;
    const NAME: &'static str = "human-held";
    const MAY_ADVANCE_CANONICAL_HEAD: bool = true;

    fn lift(action: DelegatedAction) -> Self::Action {
        HumanAction::Delegated(action)
    }

    fn is_canonical_advance(action: Self::Action) -> bool {
        matches!(action, HumanAction::AdvanceCanonicalHead)
    }

    fn parse_action(name: &str) -> Option<Self::Action> {
        HumanAction::parse(name)
    }
}

/// The workspace a capability is scoped to. Sixteen opaque bytes — `mesh-types` owns `WorkspaceId`
/// and this crate does not define a second one.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkspaceScope([u8; 16]);

impl WorkspaceScope {
    /// Scope to the workspace named by these bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// The raw bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl fmt::Debug for WorkspaceScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "WorkspaceScope({:02x}{:02x}\u{2026})",
            self.0[0], self.0[1]
        )
    }
}

// The policy epoch a capability is valid in is `mesh_types::PolicyEpoch`, and this crate defines
// no second one. That is not tidiness: the epoch a capability is issued under and the epoch a
// ChangeSet is sealed under have to be *the same value*, and two structurally identical newtypes in
// two crates are two values that a `.get()`/`PolicyEpoch::new()` hop converts between. Every such
// hop is a place an epoch check can drift by one without failing to compile, which is exactly the
// check that makes a revocation take effect. One type, no conversion, no drift.

/// When a capability stops being valid, in Unix milliseconds.
///
/// There is no ambient clock in this crate: the current time is always an argument. A capability
/// that decided its own expiry by reading the machine clock would be a capability whose validity
/// depends on a value an attacker on that machine controls, and it would make every test
/// non-deterministic for the same reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Expiry(u64);

impl Expiry {
    /// An expiry at this Unix millisecond.
    #[must_use]
    pub const fn at_unix_millis(millis: u64) -> Self {
        Self(millis)
    }

    /// The Unix millisecond this expiry falls on.
    #[must_use]
    pub const fn as_unix_millis(&self) -> u64 {
        self.0
    }

    /// Whether this expiry has passed at `now`.
    #[must_use]
    pub const fn has_passed(&self, now: u64) -> bool {
        now >= self.0
    }
}

/// How many further delegations a capability permits. Strictly decreasing, so a chain is finite and
/// a lane cannot build an unbounded delegation graph by accident.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DelegationBudget(u8);

impl DelegationBudget {
    /// A budget permitting `depth` further delegations.
    #[must_use]
    pub const fn new(depth: u8) -> Self {
        Self(depth)
    }

    /// The remaining depth.
    #[must_use]
    pub const fn remaining(&self) -> u8 {
        self.0
    }
}

/// A scoped, expiring grant of authority to a principal, at one tier of the authority lattice.
///
/// Immutable: every method that produces a different capability returns a new value, and there is
/// no `&mut self` anywhere on this type. A capability that could be edited in place is a capability
/// whose scope a holder can widen after somebody checked it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capability<T: AuthorityTier> {
    issuer: ActorKey,
    subject: ActorKey,
    workspace: WorkspaceScope,
    actions: BTreeSet<T::Action>,
    policy_epoch: PolicyEpoch,
    not_after: Expiry,
    budget: DelegationBudget,
}

/// What a delegation asks for. Every field is checked against the parent before a capability
/// exists, so there is no moment at which an over-broad capability is a value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Delegation {
    subject: ActorKey,
    actions: BTreeSet<DelegatedAction>,
    not_after: Expiry,
}

impl Delegation {
    /// Ask for `actions` on behalf of `subject`, expiring at `not_after`.
    #[must_use]
    pub fn new(
        subject: ActorKey,
        actions: impl IntoIterator<Item = DelegatedAction>,
        not_after: Expiry,
    ) -> Self {
        Self {
            subject,
            actions: actions.into_iter().collect(),
            not_after,
        }
    }

    /// Who the delegation is for.
    #[must_use]
    pub const fn subject(&self) -> &ActorKey {
        &self.subject
    }

    /// The actions asked for.
    #[must_use]
    pub const fn actions(&self) -> &BTreeSet<DelegatedAction> {
        &self.actions
    }
}

impl<T: AuthorityTier> Capability<T> {
    /// Who issued this capability.
    #[must_use]
    pub const fn issuer(&self) -> &ActorKey {
        &self.issuer
    }

    /// Whose authority this capability is.
    #[must_use]
    pub const fn subject(&self) -> &ActorKey {
        &self.subject
    }

    /// The workspace this capability is scoped to.
    #[must_use]
    pub const fn workspace(&self) -> WorkspaceScope {
        self.workspace
    }

    /// The exact action set granted.
    #[must_use]
    pub const fn actions(&self) -> &BTreeSet<T::Action> {
        &self.actions
    }

    /// The policy epoch this capability is valid in.
    #[must_use]
    pub const fn policy_epoch(&self) -> PolicyEpoch {
        self.policy_epoch
    }

    /// When this capability stops being valid.
    #[must_use]
    pub const fn not_after(&self) -> Expiry {
        self.not_after
    }

    /// How many further delegations remain.
    #[must_use]
    pub const fn budget(&self) -> DelegationBudget {
        self.budget
    }

    /// The tier's wire name.
    #[must_use]
    pub const fn tier(&self) -> &'static str {
        T::NAME
    }

    /// Whether this capability grants `action`.
    #[must_use]
    pub fn grants(&self, action: T::Action) -> bool {
        self.actions.contains(&action)
    }

    /// Whether this capability actually carries canonical-head advancement.
    ///
    /// Constant `false` for every `Capability<Delegated>` — there is no [`DelegatedAction`] for it
    /// to hold — which is TG-3 restated as a total function rather than as a check.
    #[must_use]
    pub fn advances_canonical_head(&self) -> bool {
        T::MAY_ADVANCE_CANONICAL_HEAD && self.actions.iter().copied().any(T::is_canonical_advance)
    }

    /// Whether this capability is usable at `now` under `epoch`.
    ///
    /// # Errors
    ///
    /// [`CapabilityError::Expired`] past [`Capability::not_after`];
    /// [`CapabilityError::WrongEpoch`] when the policy epoch has rotated.
    pub fn check_current(&self, now: u64, epoch: PolicyEpoch) -> Result<(), CapabilityError> {
        if self.policy_epoch != epoch {
            return Err(CapabilityError::WrongEpoch {
                issued_in: self.policy_epoch,
                current: epoch,
            });
        }
        if self.not_after.has_passed(now) {
            return Err(CapabilityError::Expired {
                not_after: self.not_after,
                now,
            });
        }
        Ok(())
    }

    /// Derive a narrower capability for another principal.
    ///
    /// The only derivation path on this type. The result is at `T::Delegated`, which is
    /// [`Delegated`] for every `T`, so no chain of calls reaches [`HumanHeld`].
    ///
    /// # Errors
    ///
    /// [`DelegationError`] when the request is not strictly narrower than this capability.
    pub fn delegate(
        &self,
        request: &Delegation,
    ) -> Result<Capability<T::Delegated>, DelegationError> {
        if self.budget.remaining() == 0 {
            return Err(DelegationError::BudgetExhausted);
        }
        if request.not_after > self.not_after {
            return Err(DelegationError::ExpiryWidened {
                parent: self.not_after,
                requested: request.not_after,
            });
        }
        if request.actions.is_empty() {
            return Err(DelegationError::EmptyGrant);
        }
        for action in &request.actions {
            if !self.actions.contains(&T::lift(*action)) {
                return Err(DelegationError::ActionNotHeld { action: *action });
            }
        }
        Ok(Capability {
            issuer: self.subject,
            subject: request.subject,
            workspace: self.workspace,
            actions: request
                .actions
                .iter()
                .copied()
                .map(<T::Delegated as AuthorityTier>::lift)
                .collect(),
            policy_epoch: self.policy_epoch,
            not_after: request.not_after,
            budget: DelegationBudget::new(self.budget.remaining() - 1),
        })
    }

    /// The capability's field values, for a codec to encode.
    ///
    /// A capability's contents are not secret — a peer has to be able to read them to check them —
    /// so this direction is public. The other direction is not: see [`crate::CapabilityParts`].
    #[must_use]
    pub fn to_parts(&self) -> CapabilityParts {
        CapabilityParts::new(
            self.issuer,
            self.subject,
            self.workspace,
            T::NAME.to_owned(),
            self.actions.iter().map(|a| a.to_string()).collect(),
            self.policy_epoch,
            self.not_after,
            self.budget,
        )
    }

    /// Rebuild a capability from decoded field values.
    ///
    /// Deliberately **not public**. The only caller is
    /// [`CapabilityToken::verify`](crate::CapabilityToken::verify), after the issuer's signature
    /// over those exact bytes has been checked. A public version of this function is a public way
    /// to mint a `Capability<HumanHeld>` that grants publication authority.
    pub(crate) fn from_parts(parts: &CapabilityParts) -> Result<Self, PartsError> {
        if parts.tier() != T::NAME {
            return Err(PartsError::TierMismatch {
                found: parts.tier().to_owned(),
                expected: T::NAME,
            });
        }
        if parts.actions().is_empty() {
            return Err(PartsError::EmptyGrant);
        }
        let mut actions = BTreeSet::new();
        for name in parts.actions() {
            let action = T::parse_action(name).ok_or_else(|| PartsError::UnknownAction {
                name: name.clone(),
                tier: T::NAME,
            })?;
            actions.insert(action);
        }
        Ok(Self {
            issuer: *parts.issuer(),
            subject: *parts.subject(),
            workspace: parts.workspace(),
            actions,
            policy_epoch: parts.policy_epoch(),
            not_after: parts.not_after(),
            budget: parts.budget(),
        })
    }
}

impl Capability<HumanHeld> {
    /// Mint the root of a human authority chain.
    ///
    /// Requires a [`HumanKeyAttestation`](crate::HumanKeyAttestation), which only an implementation
    /// of [`HumanKeyCustody`](crate::HumanKeyCustody) can produce. This crate ships no such
    /// implementation, so nothing in the workspace can call this today, which is the correct state
    /// until the OS-keychain backend lands under the security epic.
    ///
    /// The attestation's key **is** the subject: a human capability cannot be minted for a key
    /// other than the one custody attested to, so there is no argument to get wrong.
    #[must_use]
    pub fn root(
        attestation: &crate::custody::HumanKeyAttestation,
        workspace: WorkspaceScope,
        actions: impl IntoIterator<Item = HumanAction>,
        policy_epoch: PolicyEpoch,
        not_after: Expiry,
        budget: DelegationBudget,
    ) -> Self {
        let key = *attestation.key();
        Self {
            issuer: key,
            subject: key,
            workspace,
            actions: actions.into_iter().collect(),
            policy_epoch,
            not_after,
            budget,
        }
    }
}

/// Why a capability is not usable right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapabilityError {
    /// The capability was issued under a policy epoch that has since rotated.
    WrongEpoch {
        /// The epoch the capability was issued in.
        issued_in: PolicyEpoch,
        /// The epoch in force now.
        current: PolicyEpoch,
    },
    /// The capability has expired.
    Expired {
        /// When it expired.
        not_after: Expiry,
        /// The time it was checked at.
        now: u64,
    },
}

impl fmt::Display for CapabilityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongEpoch { issued_in, current } => write!(
                formatter,
                "issued in policy epoch {} and epoch {} is in force",
                issued_in.value(),
                current.value()
            ),
            Self::Expired { not_after, now } => write!(
                formatter,
                "expired at {} and it is {now}",
                not_after.as_unix_millis()
            ),
        }
    }
}

impl std::error::Error for CapabilityError {}

/// Why a delegation was refused.
///
/// Every variant is a way the request was not strictly narrower than the parent. There is no
/// variant for "would have granted canonical-head advancement", because that request cannot be
/// written: [`Delegation`] carries [`DelegatedAction`], which has no such value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DelegationError {
    /// The parent's delegation budget is exhausted; the chain ends here.
    BudgetExhausted,
    /// The request asked to outlive its parent.
    ExpiryWidened {
        /// The parent's expiry.
        parent: Expiry,
        /// The expiry asked for.
        requested: Expiry,
    },
    /// The request asked for an action the parent does not hold.
    ActionNotHeld {
        /// The action that was not held.
        action: DelegatedAction,
    },
    /// The request asked for nothing. A capability granting no action is a token that only adds
    /// attack surface.
    EmptyGrant,
}

impl fmt::Display for DelegationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BudgetExhausted => {
                formatter.write_str("this capability may not be delegated further")
            }
            Self::ExpiryWidened { parent, requested } => write!(
                formatter,
                "a delegation expires at or before its parent: parent {}, requested {}",
                parent.as_unix_millis(),
                requested.as_unix_millis()
            ),
            Self::ActionNotHeld { action } => {
                write!(formatter, "the issuer does not hold `{action}`")
            }
            Self::EmptyGrant => formatter.write_str("a delegation grants at least one action"),
        }
    }
}

impl std::error::Error for DelegationError {}

#[cfg(test)]
mod tests {
    use super::*;

    /// The guard the module docs promise. If a lane adds a canonical-advance variant to the
    /// delegable vocabulary, this fires — and so does the source scan in
    /// `crate::no_secret_material`, which reads the enum body rather than its values.
    #[test]
    fn no_canonical_advance_in_the_delegated_vocabulary() {
        assert_eq!(DelegatedAction::ALL.len(), 7);
        for action in DelegatedAction::ALL {
            assert!(
                !Delegated::is_canonical_advance(action),
                "{action} is delegable and advances canonical state"
            );
            assert!(!action.as_str().contains("canonical"), "{action}");
        }
        const { assert!(!Delegated::MAY_ADVANCE_CANONICAL_HEAD) };
        const { assert!(HumanHeld::MAY_ADVANCE_CANONICAL_HEAD) };
    }

    const _: () = assert!(
        !Delegated::MAY_ADVANCE_CANONICAL_HEAD,
        "the delegated tier may never advance canonical state"
    );
}
