//! Actors, actor kinds and the key material an actor is identified by.

use core::fmt;

use crate::digest::{ContentDigest, DomainTag};
use crate::record_id::ActorId;

/// The domain an actor identifier is derived in.
const ACTOR_KEY_DOMAIN: DomainTag = DomainTag::new("mesh.v0.actor-key");

/// An Ed25519 public key — plan §8.1 pins the scheme.
///
/// The key is opaque here. Signing, verification and key handling belong to `mesh-crypto`; this
/// crate carries the bytes because an actor's identity is derived from them.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PublicKey([u8; 32]);

impl PublicKey {
    /// Wrap 32 key bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The raw key bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// The actor identifier this key names.
    ///
    /// Derived from the key alone, never from the mutable parts of an [`Actor`]. A display name
    /// change or a disablement must not rename the actor, and the only way to guarantee that is
    /// for neither to be an input.
    ///
    /// ```
    /// use mesh_types::{Actor, ActorKind, Blake3, PublicKey, Timestamp};
    ///
    /// let key = PublicKey::from_bytes([3; 32]);
    /// let actor = Actor::new(
    ///     ActorKind::Agent,
    ///     key,
    ///     "reviewer".to_owned(),
    ///     Timestamp::from_unix_millis(1),
    /// );
    /// let renamed = actor.clone().with_display_name("reviewer-2".to_owned());
    ///
    /// assert_eq!(actor.id::<Blake3>(), renamed.id::<Blake3>());
    /// assert_eq!(actor.id::<Blake3>(), key.actor_id::<Blake3>());
    /// ```
    #[must_use]
    pub fn actor_id<D: ContentDigest>(&self) -> ActorId {
        let mut hasher = D::hasher();
        crate::digest::DigestHasher::update(&mut hasher, ACTOR_KEY_DOMAIN.as_str().as_bytes());
        crate::digest::DigestHasher::update(&mut hasher, &self.0);
        ActorId::from_digest(crate::digest::DigestHasher::finalize(hasher))
    }
}

impl fmt::Debug for PublicKey {
    /// Public keys are public, but a full key in every log line is noise; the first four bytes
    /// identify one in a test failure without pretending to be the key.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "PublicKey({:02x}{:02x}{:02x}{:02x}…)",
            self.0[0], self.0[1], self.0[2], self.0[3]
        )
    }
}

/// An Ed25519 signature over a canonical record.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Signature([u8; 64]);

impl Signature {
    /// Wrap 64 signature bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }

    /// The raw signature bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 64] {
        &self.0
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Signature(..)")
    }
}

/// A Unix-epoch millisecond timestamp.
///
/// Display and ordering only. Causality is the operation graph's job; a timestamp never decides
/// what happened first — see the `hybrid logical time` register row and OG-6.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(u64);

impl Timestamp {
    /// A timestamp from Unix milliseconds.
    #[must_use]
    pub const fn from_unix_millis(millis: u64) -> Self {
        Self(millis)
    }

    /// The Unix milliseconds.
    #[must_use]
    pub const fn as_unix_millis(&self) -> u64 {
        self.0
    }
}

/// Hybrid logical time: a monotonic ChangeSet timestamp for display and total-order tiebreaking.
///
/// It never decides causality and never affects head advancement. The physical half is
/// milliseconds; the logical half breaks ties within one millisecond.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hlc {
    physical: u64,
    logical: u32,
}

impl Hlc {
    /// A hybrid logical time from its two halves.
    #[must_use]
    pub const fn new(physical_millis: u64, logical: u32) -> Self {
        Self {
            physical: physical_millis,
            logical,
        }
    }

    /// The physical half, in Unix milliseconds.
    #[must_use]
    pub const fn physical_millis(&self) -> u64 {
        self.physical
    }

    /// The logical half.
    #[must_use]
    pub const fn logical(&self) -> u32 {
        self.logical
    }
}

/// What an actor is. Exactly one of the seven kinds in plan §4.2.
///
/// The spelling follows the terminology register rather than the plan for one kind: the plan's
/// `Device` is [`ActorKind::DeviceActor`], because `device` already names the machine an actor
/// runs on and one word may not carry two definitions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ActorKind {
    /// A person, holding their key under OS key isolation. The only kind that can hold the
    /// approval capability.
    Human,
    /// A machine acting in its own right, under a device key.
    DeviceActor,
    /// A long-running model-driven participant with a sponsor human.
    Agent,
    /// One bounded execution of an agent, so work is attributable to a single run.
    AgentRun,
    /// A rule-driven, non-model participant: a scheduled job, a hook, a trigger.
    Automation,
    /// An actor that executes validation runs and can never author a canonical transition.
    Validator,
    /// A first-party Mesh component participating as an actor in its own right.
    Service,
}

impl ActorKind {
    /// Every kind, in plan §4.2 order. Exhaustive by construction: adding a variant without
    /// adding it here fails the match below to compile.
    pub const ALL: [Self; 7] = [
        Self::Human,
        Self::DeviceActor,
        Self::Agent,
        Self::AgentRun,
        Self::Automation,
        Self::Validator,
        Self::Service,
    ];

    /// The register's spelling of this kind.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::DeviceActor => "device actor",
            Self::Agent => "agent",
            Self::AgentRun => "agent-run",
            Self::Automation => "automation",
            Self::Validator => "validator",
            Self::Service => "service",
        }
    }

    /// Whether this kind may ever hold the approval capability.
    ///
    /// Only a human may. This is a statement about the kind, not an authorization decision —
    /// `mesh-policy` owns capabilities — but stating it here is what lets a downstream crate make
    /// TG-3 unrepresentable rather than merely denied.
    #[must_use]
    pub const fn may_hold_approval_capability(&self) -> bool {
        matches!(self, Self::Human)
    }
}

impl fmt::Display for ActorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// An authenticated participant that can hold a private state and author ChangeSets.
///
/// The identity is the key; everything else is mutable state about the identity. [`Actor::id`]
/// therefore derives from the key alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Actor {
    kind: ActorKind,
    public_key: PublicKey,
    sponsor_human: Option<ActorId>,
    display_name: String,
    created_at: Timestamp,
    disabled_at: Option<Timestamp>,
}

impl Actor {
    /// An enabled actor with no sponsor.
    #[must_use]
    pub fn new(
        kind: ActorKind,
        public_key: PublicKey,
        display_name: String,
        created_at: Timestamp,
    ) -> Self {
        Self {
            kind,
            public_key,
            sponsor_human: None,
            display_name,
            created_at,
            disabled_at: None,
        }
    }

    /// The same actor with a sponsor human recorded.
    #[must_use]
    pub fn with_sponsor_human(self, sponsor: ActorId) -> Self {
        Self {
            sponsor_human: Some(sponsor),
            ..self
        }
    }

    /// The same actor under a different display name.
    #[must_use]
    pub fn with_display_name(self, display_name: String) -> Self {
        Self {
            display_name,
            ..self
        }
    }

    /// The same actor, disabled at `at`.
    #[must_use]
    pub fn disabled_at(self, at: Timestamp) -> Self {
        Self {
            disabled_at: Some(at),
            ..self
        }
    }

    /// This actor's identifier, derived from its public key.
    #[must_use]
    pub fn id<D: ContentDigest>(&self) -> ActorId {
        self.public_key.actor_id::<D>()
    }

    /// What kind of actor this is.
    #[must_use]
    pub const fn kind(&self) -> ActorKind {
        self.kind
    }

    /// The actor's public key.
    #[must_use]
    pub const fn public_key(&self) -> &PublicKey {
        &self.public_key
    }

    /// The human accountable for this actor's authority, when it has one.
    #[must_use]
    pub const fn sponsor_human(&self) -> Option<&ActorId> {
        self.sponsor_human.as_ref()
    }

    /// The actor's display name.
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// When the actor was created.
    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }

    /// When the actor was disabled, if it has been.
    #[must_use]
    pub const fn disabled_at_timestamp(&self) -> Option<Timestamp> {
        self.disabled_at
    }
}
