//! Who is asking — and the one narrowing that only a human survives.
//!
//! `mesh_types::ActorKind::may_hold_approval_capability` is `true` for exactly one of the seven
//! actor kinds. That is a statement about the kind and it decides nothing on its own; this module
//! is where it becomes a type.
//!
//! [`Principal`] is any authenticated actor. [`HumanPrincipal`] is a principal that got past
//! [`HumanPrincipal::enrol`], which is the only constructor and which refuses every kind whose
//! `may_hold_approval_capability` is `false`. The publication guard takes a `&HumanPrincipal`, so
//! "an agent asked to publish" is not a request that gets denied — it is a value the caller cannot
//! assemble.
//!
//! # Two independent gates, deliberately
//!
//! The publication path requires **both** a `Capability<HumanHeld>` (mesh-crypto's tier type, which
//! no delegation can produce) and a [`HumanPrincipal`] (mesh-types' actor kind, checked here). They
//! are separate mechanisms over separate facts: the tier is about the authority chain, the kind is
//! about what the actor *is*. A single bug does not open both.

use core::fmt;

use mesh_crypto::ActorKey;
use mesh_types::ActorKind;

/// An authenticated actor: its key and what kind of actor it is.
///
/// Constructing one asserts nothing about authority. It is the pair a caller has after
/// authenticating a peer, and nothing more.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Principal {
    key: ActorKey,
    kind: ActorKind,
}

impl Principal {
    /// An actor of `kind` named by `key`.
    #[must_use]
    pub const fn new(key: ActorKey, kind: ActorKind) -> Self {
        Self { key, kind }
    }

    /// The actor's key.
    #[must_use]
    pub const fn key(&self) -> ActorKey {
        self.key
    }

    /// What kind of actor it is.
    #[must_use]
    pub const fn kind(&self) -> ActorKind {
        self.kind
    }
}

impl fmt::Display for Principal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}", self.kind, self.key)
    }
}

/// A principal that may hold the approval capability — which is to say, a person.
///
/// The inner principal is private and there is no `From`, no `Deref` and no second constructor.
/// [`HumanPrincipal::enrol`] is the only way in, and it is a `Result`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HumanPrincipal(Principal);

impl HumanPrincipal {
    /// Narrow a principal to a human one.
    ///
    /// # Errors
    ///
    /// [`PrincipalError::MayNotHoldApprovalCapability`] for every kind that may not, which is every
    /// kind except `human`. The condition is asked of `mesh-types` rather than restated here, so
    /// this crate cannot drift from the kind model it is enforcing.
    pub fn enrol(principal: Principal) -> Result<Self, PrincipalError> {
        if principal.kind.may_hold_approval_capability() {
            Ok(Self(principal))
        } else {
            Err(PrincipalError::MayNotHoldApprovalCapability {
                kind: principal.kind,
            })
        }
    }

    /// The human's key.
    #[must_use]
    pub const fn key(&self) -> ActorKey {
        self.0.key
    }

    /// The underlying principal.
    #[must_use]
    pub const fn principal(&self) -> Principal {
        self.0
    }
}

impl fmt::Display for HumanPrincipal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, formatter)
    }
}

/// Why a principal was not accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrincipalError {
    /// The actor kind may never hold the approval capability.
    MayNotHoldApprovalCapability {
        /// The kind that was offered.
        kind: ActorKind,
    },
}

impl fmt::Display for PrincipalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MayNotHoldApprovalCapability { kind } => write!(
                formatter,
                "a {kind} may never hold the approval capability; only a human may"
            ),
        }
    }
}

impl std::error::Error for PrincipalError {}
