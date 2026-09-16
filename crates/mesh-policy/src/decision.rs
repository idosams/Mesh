//! The authority-bearing operations, and the one gate every one of them goes through.
//!
//! # Every path, not most paths
//!
//! [`Operation`] enumerates every operation in Mesh that needs authority. `Operation::ALL` is
//! exhaustive by construction, and the tests iterate it rather than picking cases — so "an expired
//! capability is rejected on every path" is a loop over the vocabulary, not a claim about the three
//! paths somebody remembered.
//!
//! # The generic gate cannot grant publication, at any tier
//!
//! [`authorize`] is generic over [`AuthorityTier`]. To decide an operation it needs the action in
//! `T::Action`, and the only way it can build one is [`AuthorityTier::lift`], which takes a
//! [`DelegatedAction`]. There is no `DelegatedAction` for canonical-head advancement, so this
//! function **has no way to name the action it would have to check** — for `Delegated` and for
//! `HumanHeld` alike. It therefore denies [`Operation::AdvanceCanonicalHead`] unconditionally, and
//! not because a branch says so: because the value it would compare against cannot be constructed
//! here.
//!
//! Publication is decided in [`crate::publication`], monomorphic in
//! [`HumanHeld`](mesh_crypto::HumanHeld), which is the one place `HumanAction::AdvanceCanonicalHead`
//! is nameable.
//!
//! # Order of checks
//!
//! Subject, workspace, epoch, revocation, expiry, action. The order decides which reason a rejection
//! is recorded under when several apply, and it runs from "this is not your capability" outwards, so
//! the recorded reason is the most specific true statement about the request.

use core::fmt;
use core::marker::PhantomData;

use mesh_crypto::{AuthorityTier, Capability, DelegatedAction, Expiry, WorkspaceScope};
use mesh_types::PolicyEpoch;

use crate::epoch::{EpochChain, EpochStanding};
use mesh_crypto::ActorKey;

/// An operation that needs authority.
///
/// One variant per [`DelegatedAction`], plus the one that has no delegated form.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Operation {
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
    /// Advance the protected shared version. The one operation with no delegated form.
    AdvanceCanonicalHead,
}

impl Operation {
    /// Every authority-bearing operation. Exhaustive by construction: the matches below stop
    /// compiling if a variant is added without being listed here.
    pub const ALL: [Self; 8] = [
        Self::ReadWorkspace,
        Self::WriteOwnActorState,
        Self::AuthorChangeSet,
        Self::Replicate,
        Self::RequestReview,
        Self::RecordContextRead,
        Self::RunValidation,
        Self::AdvanceCanonicalHead,
    ];

    /// The operation's wire name.
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
            Self::AdvanceCanonicalHead => "advance-canonical-head",
        }
    }

    /// The delegable action this operation needs, or `None` when there is no such action.
    ///
    /// `None` for exactly one operation, and that is not a convention: [`DelegatedAction`] has no
    /// canonical-advance variant, so there is nothing to return.
    #[must_use]
    pub const fn delegable_action(&self) -> Option<DelegatedAction> {
        match self {
            Self::ReadWorkspace => Some(DelegatedAction::ReadWorkspace),
            Self::WriteOwnActorState => Some(DelegatedAction::WriteOwnActorState),
            Self::AuthorChangeSet => Some(DelegatedAction::AuthorChangeSet),
            Self::Replicate => Some(DelegatedAction::Replicate),
            Self::RequestReview => Some(DelegatedAction::RequestReview),
            Self::RecordContextRead => Some(DelegatedAction::RecordContextRead),
            Self::RunValidation => Some(DelegatedAction::RunValidation),
            Self::AdvanceCanonicalHead => None,
        }
    }

    /// Whether performing this operation advances the protected shared version.
    #[must_use]
    pub const fn advances_canonical_state(&self) -> bool {
        matches!(self, Self::AdvanceCanonicalHead)
    }
}

impl fmt::Display for Operation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// What is being asked, by whom, where and when.
///
/// `now` is an argument because this crate reads no clock. A gate that read the machine clock would
/// be a gate whose expiry check depends on a value an attacker on that machine controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuthorityRequest {
    subject: ActorKey,
    workspace: WorkspaceScope,
    operation: Operation,
    now: u64,
}

impl AuthorityRequest {
    /// A request by `subject` to perform `operation` in `workspace`, decided at `now`.
    ///
    /// `subject` is the peer the **caller authenticated**, never a field read off the capability:
    /// a capability is bearer evidence, and reading the subject out of it would accept a stolen one.
    #[must_use]
    pub const fn new(
        subject: ActorKey,
        workspace: WorkspaceScope,
        operation: Operation,
        now: u64,
    ) -> Self {
        Self {
            subject,
            workspace,
            operation,
            now,
        }
    }

    /// The authenticated peer.
    #[must_use]
    pub const fn subject(&self) -> ActorKey {
        self.subject
    }

    /// The workspace.
    #[must_use]
    pub const fn workspace(&self) -> WorkspaceScope {
        self.workspace
    }

    /// The operation.
    #[must_use]
    pub const fn operation(&self) -> Operation {
        self.operation
    }

    /// The moment the decision is made at, in Unix milliseconds.
    #[must_use]
    pub const fn now(&self) -> u64 {
        self.now
    }
}

/// Proof that one operation was authorized, for one subject, in one epoch.
///
/// There is no public constructor, no public field and no `Default`. The only value of this type
/// that exists is one [`crate::DecisionLedger`] produced, which is why a caller cannot hold a grant
/// that was never decided — and why every grant that exists has a ledger entry behind it.
///
/// A `Grant<HumanHeld>` is **not** publication authority. See [`crate::PublicationAuthority`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grant<T: AuthorityTier> {
    subject: ActorKey,
    workspace: WorkspaceScope,
    operation: Operation,
    epoch: PolicyEpoch,
    tier: PhantomData<T>,
}

impl<T: AuthorityTier> Grant<T> {
    pub(crate) const fn new(
        subject: ActorKey,
        workspace: WorkspaceScope,
        operation: Operation,
        epoch: PolicyEpoch,
    ) -> Self {
        Self {
            subject,
            workspace,
            operation,
            epoch,
            tier: PhantomData,
        }
    }

    /// Who the grant is for.
    #[must_use]
    pub const fn subject(&self) -> ActorKey {
        self.subject
    }

    /// The workspace it is good in.
    #[must_use]
    pub const fn workspace(&self) -> WorkspaceScope {
        self.workspace
    }

    /// What it authorizes.
    #[must_use]
    pub const fn operation(&self) -> Operation {
        self.operation
    }

    /// The epoch it was decided in. A grant does not outlive its epoch: a caller holding one across
    /// a rotation must ask again.
    #[must_use]
    pub const fn epoch(&self) -> PolicyEpoch {
        self.epoch
    }

    /// The tier's wire name.
    #[must_use]
    pub const fn tier(&self) -> &'static str {
        T::NAME
    }
}

/// Why a request was refused.
///
/// Every variant names one true, specific fact about the request. There is no `Other` and no
/// `Unknown`: a denial a reader cannot act on is a denial that gets ignored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DenialReason {
    /// The capability names a different subject than the peer that presented it.
    SubjectMismatch,
    /// The capability is scoped to a different workspace.
    WorkspaceMismatch,
    /// The capability was issued in an epoch that has been rotated past.
    EpochSuperseded {
        /// The epoch the capability names.
        presented: PolicyEpoch,
        /// The epoch in force.
        in_force: PolicyEpoch,
    },
    /// The capability names an epoch this peer has never observed. Denied rather than deferred.
    EpochUnknown {
        /// The epoch the capability names.
        presented: PolicyEpoch,
        /// The epoch in force on this peer.
        in_force: PolicyEpoch,
    },
    /// The subject's key was revoked by a rotation this peer has applied.
    SubjectRevoked {
        /// The epoch in force when the denial was recorded.
        in_force: PolicyEpoch,
    },
    /// The capability's expiry has passed.
    Expired {
        /// When it expired.
        not_after: Expiry,
        /// The moment the decision was made at.
        now: u64,
    },
    /// The capability does not grant the action this operation needs.
    ActionNotGranted {
        /// The operation that was refused.
        operation: Operation,
    },
    /// Canonical-head advancement is not decided by the general gate at any tier — see the module
    /// documentation. The publication guard is the only path, and it takes a human.
    CanonicalAdvanceIsNotDelegable,
    /// The actor kind may never hold the approval capability.
    NotAHumanPrincipal,
    /// The human presenting the capability is not the human the capability was issued to.
    ApproverMismatch,
}

impl fmt::Display for DenialReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SubjectMismatch => {
                formatter.write_str("this capability was issued to a different actor")
            }
            Self::WorkspaceMismatch => {
                formatter.write_str("this capability is scoped to a different workspace")
            }
            Self::EpochSuperseded {
                presented,
                in_force,
            } => write!(
                formatter,
                "issued in policy epoch {} and epoch {} is in force",
                presented.value(),
                in_force.value()
            ),
            Self::EpochUnknown {
                presented,
                in_force,
            } => write!(
                formatter,
                "names policy epoch {}, which this peer has not observed; epoch {} is in force",
                presented.value(),
                in_force.value()
            ),
            Self::SubjectRevoked { in_force } => write!(
                formatter,
                "this actor's key was revoked by a rotation applied at or before policy epoch {}",
                in_force.value()
            ),
            Self::Expired { not_after, now } => write!(
                formatter,
                "expired at {} and it is {now}",
                not_after.as_unix_millis()
            ),
            Self::ActionNotGranted { operation } => {
                write!(formatter, "this capability does not grant `{operation}`")
            }
            Self::CanonicalAdvanceIsNotDelegable => formatter
                .write_str("canonical-head advancement is not an action any capability delegates"),
            Self::NotAHumanPrincipal => {
                formatter.write_str("only a human may hold the approval capability")
            }
            Self::ApproverMismatch => {
                formatter.write_str("this approval capability belongs to a different person")
            }
        }
    }
}

/// A refusal, with everything a reader needs to act on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Denial {
    subject: ActorKey,
    operation: Operation,
    in_force: PolicyEpoch,
    reason: DenialReason,
}

impl Denial {
    pub(crate) const fn new(
        subject: ActorKey,
        operation: Operation,
        in_force: PolicyEpoch,
        reason: DenialReason,
    ) -> Self {
        Self {
            subject,
            operation,
            in_force,
            reason,
        }
    }

    /// Who was refused.
    #[must_use]
    pub const fn subject(&self) -> ActorKey {
        self.subject
    }

    /// What was refused.
    #[must_use]
    pub const fn operation(&self) -> Operation {
        self.operation
    }

    /// The epoch in force when the refusal was decided.
    #[must_use]
    pub const fn in_force(&self) -> PolicyEpoch {
        self.in_force
    }

    /// Why.
    #[must_use]
    pub const fn reason(&self) -> DenialReason {
        self.reason
    }
}

impl fmt::Display for Denial {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "`{}` refused for {}: {}",
            self.operation, self.subject, self.reason
        )
    }
}

impl std::error::Error for Denial {}

/// The checks that are the same whatever the operation is, in one place so the general gate and
/// the publication guard cannot drift apart.
///
/// Two copies of an epoch check is two places for a rotation to stop biting, and the copy that
/// stops biting will be the one on the publication path, because that is the one somebody wrote
/// second.
pub(crate) fn check_preconditions<T: AuthorityTier>(
    chain: &EpochChain,
    capability: &Capability<T>,
    request: &AuthorityRequest,
) -> Result<(), Denial> {
    let in_force = chain.current();
    let deny = |reason| {
        Err(Denial::new(
            request.subject,
            request.operation,
            in_force,
            reason,
        ))
    };

    if *capability.subject() != request.subject {
        return deny(DenialReason::SubjectMismatch);
    }
    if capability.workspace() != request.workspace {
        return deny(DenialReason::WorkspaceMismatch);
    }
    match chain.standing_of(capability.policy_epoch()) {
        EpochStanding::Current => {}
        EpochStanding::Superseded { by } => {
            return deny(DenialReason::EpochSuperseded {
                presented: capability.policy_epoch(),
                in_force: by,
            })
        }
        EpochStanding::Unknown { in_force } => {
            return deny(DenialReason::EpochUnknown {
                presented: capability.policy_epoch(),
                in_force,
            })
        }
    }
    if chain.is_revoked(&request.subject) {
        return deny(DenialReason::SubjectRevoked { in_force });
    }
    if capability.not_after().has_passed(request.now) {
        return deny(DenialReason::Expired {
            not_after: capability.not_after(),
            now: request.now,
        });
    }
    Ok(())
}

/// Decide one request against one capability.
///
/// Deliberately **not public**. The only caller is
/// [`DecisionLedger::authorize`](crate::DecisionLedger::authorize), so a decision cannot be reached
/// without a record of it being reached — which is how "every rejection is recorded with its reason"
/// is a property of the module structure and not of a caller remembering to log.
pub(crate) fn authorize<T: AuthorityTier>(
    chain: &EpochChain,
    capability: &Capability<T>,
    request: &AuthorityRequest,
) -> Result<Grant<T>, Denial> {
    check_preconditions(chain, capability, request)?;
    let in_force = chain.current();

    // The load-bearing line. `lift` is the only way this function can build a `T::Action`, and it
    // takes a `DelegatedAction` — of which there is none for canonical advancement. So the `None`
    // arm is not a policy choice about publication; it is the absence of a value to check.
    let Some(action) = request.operation.delegable_action() else {
        return Err(Denial::new(
            request.subject,
            request.operation,
            in_force,
            DenialReason::CanonicalAdvanceIsNotDelegable,
        ));
    };
    if !capability.grants(T::lift(action)) {
        return Err(Denial::new(
            request.subject,
            request.operation,
            in_force,
            DenialReason::ActionNotGranted {
                operation: request.operation,
            },
        ));
    }
    Ok(Grant::new(
        request.subject,
        request.workspace,
        request.operation,
        in_force,
    ))
}
