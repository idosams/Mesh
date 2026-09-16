//! ChangeSets, and the type-level guarantee that one cannot exist without its causal context.
//!
//! A ChangeSet with no causal parents, no base head or no policy epoch is not a ChangeSet that is
//! *invalid* — it is a ChangeSet whose causality and authority cannot be checked at all. The
//! operation graph's whole claim is that every transition names what it followed and what
//! authority it ran under, and a runtime check placed after construction is a check somebody can
//! forget to call. So the guarantee lives in the types: [`ChangeSetDraft`] carries the three fields
//! as its own type parameters, and [`ChangeSetDraft::seal`] exists only on the draft that has all
//! three.
//!
//! The negative case is a compile error, and it is tested as one:
//!
//! ```compile_fail,E0599
//! use mesh_types::{ChangeSetDraft, CausalParents, HeadId, Digest32};
//! # use mesh_types::{ActorId, ActorSequence, Hlc, SessionId, Signature, WorkspaceId};
//! # let workspace = WorkspaceId::mint(1, [0; 10]);
//! # let actor = ActorId::from_digest(Digest32::from_bytes([1; 32]));
//! # let session = SessionId::mint(2, [0; 10]);
//! # let head = HeadId::from_digest(Digest32::from_bytes([2; 32]));
//! let draft = ChangeSetDraft::<()>::new(
//!     workspace, actor, session, ActorSequence::new(1), Hlc::new(0, 0),
//! )
//! .causal_parents(CausalParents::genesis())
//! .base_head(head);
//! // No `.policy_epoch(…)`: `seal` does not exist on this type.
//! let changeset = draft.seal(Vec::new(), head, Signature::from_bytes([0; 64]));
//! ```
//!
//! The same program with `.policy_epoch(PolicyEpoch::new(0))` inserted compiles, which is
//! [`ChangeSetDraft::seal`]'s own documentation example — a compile-fail test whose positive twin
//! is missing proves only that the snippet is broken.

use core::fmt;
use core::marker::PhantomData;

use crate::actor::{Hlc, Signature};
use crate::canonical::{
    encode_canonical, CanonicalEncode, CanonicalType, CanonicalValue, FieldSchema, RecordSchema,
};
use crate::digest::{Absorb, CanonicalRecord, DigestHasher, DigestWriter, DomainTag};
use crate::entity_id::{SessionId, WorkspaceId};
use crate::record_id::{ActorId, ChangeSetId, HeadId};

/// The domain every ChangeSet identifier and encoding is derived under.
///
/// A free constant rather than `<ChangeSet<Op> as CanonicalRecord>::DOMAIN`, because reading it
/// through that path would make [`CanonicalEncode`] for a ChangeSet require `Op: Absorb` as well
/// as `Op: CanonicalEncode` — coupling the interchange encoding to the identity framing through
/// a bound, which is the one place the two are meant not to touch.
const CHANGESET_DOMAIN: DomainTag = DomainTag::new("mesh.v0.changeset");

/// The hybrid-logical-time group a ChangeSet carries.
const HLC_FIELDS: &[FieldSchema] = &[
    FieldSchema::new("physical_millis", CanonicalType::Unsigned),
    FieldSchema::new("logical", CanonicalType::Unsigned),
];

/// A strictly increasing per-actor counter carried by every ChangeSet, making omission detectable
/// by any peer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActorSequence(u64);

impl ActorSequence {
    /// A sequence number.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// The number.
    #[must_use]
    pub const fn value(&self) -> u64 {
        self.0
    }

    /// The next number in the sequence.
    ///
    /// Saturating rather than wrapping: a wrapped actor sequence would make an omission
    /// undetectable, which is the one thing this counter exists to prevent. Reaching `u64::MAX` is
    /// not reachable in practice and stalling there is the safe failure.
    #[must_use]
    pub const fn next(&self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

/// The policy epoch a ChangeSet was authored under.
///
/// Strictly increasing; authority granted under a prior epoch is not valid in a later one (TG-7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PolicyEpoch(u64);

impl PolicyEpoch {
    /// An epoch.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// The epoch number.
    #[must_use]
    pub const fn value(&self) -> u64 {
        self.0
    }
}

/// The ChangeSets a ChangeSet causally follows.
///
/// The two cases are separate constructors on purpose. An empty parent list is a legitimate
/// state — the first ChangeSet in a workspace has no history — but it is also what a caller that
/// forgot to look up the parents produces. [`CausalParents::genesis`] makes the first case an
/// explicit statement and [`CausalParents::after`] makes the second unrepresentable, since it
/// takes the first parent by value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CausalParents(Vec<ChangeSetId>);

impl CausalParents {
    /// No parents, said deliberately: this ChangeSet begins the history.
    #[must_use]
    pub const fn genesis() -> Self {
        Self(Vec::new())
    }

    /// At least one parent. `rest` carries the additional parents of a merge.
    #[must_use]
    pub fn after(first: ChangeSetId, rest: Vec<ChangeSetId>) -> Self {
        let mut parents = Vec::with_capacity(rest.len() + 1);
        parents.push(first);
        parents.extend(rest);
        Self(parents)
    }

    /// The parents, in the order given.
    #[must_use]
    pub fn as_slice(&self) -> &[ChangeSetId] {
        &self.0
    }

    /// Whether this ChangeSet begins the history.
    #[must_use]
    pub fn is_genesis(&self) -> bool {
        self.0.is_empty()
    }
}

/// The typestate marker for a draft field that has not been supplied yet.
///
/// A zero-sized placeholder. Once the field is supplied, the field's own type replaces this one in
/// the draft's parameter list, so the draft carries the value rather than an option over it — there
/// is no unwrap anywhere in this module.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Unset;

/// A ChangeSet under construction.
///
/// The three type parameters after `Op` are the causal parents, the base head and the policy
/// epoch. Each starts as [`Unset`] and becomes the field's own type when supplied. Each setter
/// exists only while its field is [`Unset`], so a field cannot be supplied twice either.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeSetDraft<Op, Parents = Unset, Base = Unset, Epoch = Unset> {
    workspace_id: WorkspaceId,
    actor_id: ActorId,
    session_id: SessionId,
    actor_sequence: ActorSequence,
    hybrid_logical_time: Hlc,
    causal_parents: Parents,
    base_head: Base,
    policy_epoch: Epoch,
    operations: PhantomData<fn() -> Op>,
}

impl<Op> ChangeSetDraft<Op, Unset, Unset, Unset> {
    /// Begin a draft with the fields that identify who authored it and when.
    #[must_use]
    pub const fn new(
        workspace_id: WorkspaceId,
        actor_id: ActorId,
        session_id: SessionId,
        actor_sequence: ActorSequence,
        hybrid_logical_time: Hlc,
    ) -> Self {
        Self {
            workspace_id,
            actor_id,
            session_id,
            actor_sequence,
            hybrid_logical_time,
            causal_parents: Unset,
            base_head: Unset,
            policy_epoch: Unset,
            operations: PhantomData,
        }
    }
}

impl<Op, Base, Epoch> ChangeSetDraft<Op, Unset, Base, Epoch> {
    /// State what this ChangeSet causally follows.
    #[must_use]
    pub fn causal_parents(
        self,
        causal_parents: CausalParents,
    ) -> ChangeSetDraft<Op, CausalParents, Base, Epoch> {
        ChangeSetDraft {
            workspace_id: self.workspace_id,
            actor_id: self.actor_id,
            session_id: self.session_id,
            actor_sequence: self.actor_sequence,
            hybrid_logical_time: self.hybrid_logical_time,
            causal_parents,
            base_head: self.base_head,
            policy_epoch: self.policy_epoch,
            operations: PhantomData,
        }
    }
}

impl<Op, Parents, Epoch> ChangeSetDraft<Op, Parents, Unset, Epoch> {
    /// State the head this ChangeSet was authored against.
    #[must_use]
    pub fn base_head(self, base_head: HeadId) -> ChangeSetDraft<Op, Parents, HeadId, Epoch> {
        ChangeSetDraft {
            workspace_id: self.workspace_id,
            actor_id: self.actor_id,
            session_id: self.session_id,
            actor_sequence: self.actor_sequence,
            hybrid_logical_time: self.hybrid_logical_time,
            causal_parents: self.causal_parents,
            base_head,
            policy_epoch: self.policy_epoch,
            operations: PhantomData,
        }
    }
}

impl<Op, Parents, Base> ChangeSetDraft<Op, Parents, Base, Unset> {
    /// State the policy epoch this ChangeSet was authored under.
    #[must_use]
    pub fn policy_epoch(
        self,
        policy_epoch: PolicyEpoch,
    ) -> ChangeSetDraft<Op, Parents, Base, PolicyEpoch> {
        ChangeSetDraft {
            workspace_id: self.workspace_id,
            actor_id: self.actor_id,
            session_id: self.session_id,
            actor_sequence: self.actor_sequence,
            hybrid_logical_time: self.hybrid_logical_time,
            causal_parents: self.causal_parents,
            base_head: self.base_head,
            policy_epoch,
            operations: PhantomData,
        }
    }
}

impl<Op> ChangeSetDraft<Op, CausalParents, HeadId, PolicyEpoch> {
    /// Seal the draft into a [`ChangeSet`].
    ///
    /// This method exists only on a draft whose causal parents, base head and policy epoch have
    /// all been supplied — that is the guarantee, and it is enforced by the impl block's type
    /// parameters rather than by anything this function body does.
    ///
    /// ```
    /// use mesh_types::{
    ///     ActorId, ActorSequence, CausalParents, ChangeSetDraft, Digest32, HeadId, Hlc,
    ///     PolicyEpoch, SessionId, Signature, WorkspaceId,
    /// };
    ///
    /// let head = HeadId::from_digest(Digest32::from_bytes([2; 32]));
    /// let changeset = ChangeSetDraft::<()>::new(
    ///     WorkspaceId::mint(1, [0; 10]),
    ///     ActorId::from_digest(Digest32::from_bytes([1; 32])),
    ///     SessionId::mint(2, [0; 10]),
    ///     ActorSequence::new(1),
    ///     Hlc::new(0, 0),
    /// )
    /// .causal_parents(CausalParents::genesis())
    /// .base_head(head)
    /// .policy_epoch(PolicyEpoch::new(0))
    /// .seal(Vec::new(), head, Signature::from_bytes([0; 64]));
    ///
    /// assert!(changeset.causal_parents().is_genesis());
    /// assert_eq!(changeset.policy_epoch(), PolicyEpoch::new(0));
    /// ```
    #[must_use]
    pub fn seal(
        self,
        operations: Vec<Op>,
        resulting_head: HeadId,
        signature: Signature,
    ) -> ChangeSet<Op> {
        ChangeSet {
            workspace_id: self.workspace_id,
            actor_id: self.actor_id,
            session_id: self.session_id,
            actor_sequence: self.actor_sequence,
            causal_parents: self.causal_parents,
            base_head: self.base_head,
            resulting_head,
            operations,
            policy_epoch: self.policy_epoch,
            hybrid_logical_time: self.hybrid_logical_time,
            signature,
        }
    }
}

/// One authored transition: who caused it, what it followed, what it did and what it produced.
///
/// There is no public constructor and no public field. The only way to obtain one is
/// [`ChangeSetDraft::seal`], which the type system only offers once the causal context is complete.
///
/// `Op` is the operation type, supplied by `mesh-operations`. This crate never names an operation:
/// the operation vocabulary is that crate's to define, and a types crate that knew the vocabulary
/// would have to change every time the vocabulary did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeSet<Op> {
    workspace_id: WorkspaceId,
    actor_id: ActorId,
    session_id: SessionId,
    actor_sequence: ActorSequence,
    causal_parents: CausalParents,
    base_head: HeadId,
    resulting_head: HeadId,
    operations: Vec<Op>,
    policy_epoch: PolicyEpoch,
    hybrid_logical_time: Hlc,
    signature: Signature,
}

impl<Op> ChangeSet<Op> {
    /// The workspace this transition belongs to.
    #[must_use]
    pub const fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }

    /// The actor that authored it.
    #[must_use]
    pub const fn actor_id(&self) -> ActorId {
        self.actor_id
    }

    /// The session it was authored in.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Its position in the authoring actor's sequence.
    #[must_use]
    pub const fn actor_sequence(&self) -> ActorSequence {
        self.actor_sequence
    }

    /// What it causally follows.
    #[must_use]
    pub const fn causal_parents(&self) -> &CausalParents {
        &self.causal_parents
    }

    /// The head it was authored against.
    #[must_use]
    pub const fn base_head(&self) -> HeadId {
        self.base_head
    }

    /// The head applying it produces.
    #[must_use]
    pub const fn resulting_head(&self) -> HeadId {
        self.resulting_head
    }

    /// What it does.
    #[must_use]
    pub fn operations(&self) -> &[Op] {
        &self.operations
    }

    /// The policy epoch it was authored under.
    #[must_use]
    pub const fn policy_epoch(&self) -> PolicyEpoch {
        self.policy_epoch
    }

    /// Its hybrid logical time — for display and tiebreaking, never for causality.
    #[must_use]
    pub const fn hybrid_logical_time(&self) -> Hlc {
        self.hybrid_logical_time
    }

    /// The authoring actor's signature over the ten bound fields.
    #[must_use]
    pub const fn signature(&self) -> &Signature {
        &self.signature
    }
}

impl<Op: Absorb> CanonicalRecord for ChangeSet<Op> {
    type Id = ChangeSetId;

    const DOMAIN: DomainTag = CHANGESET_DOMAIN;
}

impl<Op: Absorb> Absorb for ChangeSet<Op> {
    /// Absorbs ten fields and **not** the signature.
    ///
    /// The signature is over the identifier, so binding it into the identifier would make the
    /// identifier unknowable until after signing and unverifiable afterwards. This is the same
    /// distinction `docs/protocol.md` §2.4 draws for the approval envelope, where nine fields are
    /// bound and the tenth member is the signature over them.
    fn absorb<H: DigestHasher>(&self, writer: &mut DigestWriter<H>) {
        writer.bytes(self.workspace_id.uuid().as_bytes());
        writer.digest(self.actor_id.digest());
        writer.bytes(self.session_id.uuid().as_bytes());
        writer.u64(self.actor_sequence.value());
        writer.sequence(self.causal_parents.as_slice(), |writer, parent| {
            writer.digest(parent.digest());
        });
        writer.digest(self.base_head.digest());
        writer.digest(self.resulting_head.digest());
        writer.sequence(&self.operations, |writer, operation| {
            operation.absorb(writer);
        });
        writer.u64(self.policy_epoch.value());
        writer.u64(self.hybrid_logical_time.physical_millis());
        writer.u64(u64::from(self.hybrid_logical_time.logical()));
    }
}

impl Absorb for () {
    /// The empty operation, so a ChangeSet carrying no operation vocabulary is still a canonical
    /// record. Absorbing nothing is safe because the sequence that contains it is length-prefixed.
    fn absorb<H: DigestHasher>(&self, _writer: &mut DigestWriter<H>) {}
}

/// The empty operation's own domain, so that a nested operation encoding is self-identifying even
/// when it carries no field.
const EMPTY_OPERATION_DOMAIN: DomainTag = DomainTag::new("mesh.v0.empty-operation");

impl CanonicalEncode for () {
    /// The empty operation. `mesh-operations` supplies the real vocabulary and its schemas; this
    /// exists so that a ChangeSet is encodable — and has published vectors — before that crate
    /// does, and so that the `operations` sequence has a tested element type.
    const SCHEMA: RecordSchema = RecordSchema::new(EMPTY_OPERATION_DOMAIN, &[]);

    fn canonical_fields(&self) -> Vec<CanonicalValue> {
        Vec::new()
    }
}

impl<Op: CanonicalEncode> CanonicalEncode for ChangeSet<Op> {
    /// The ten bound fields, and **not** the signature — the same ten
    /// [`Absorb`](Absorb::absorb) binds, and for the same reason: the signature is made over the
    /// record, so a record that bound its own signature could never be signed.
    ///
    /// `operations` is a sequence of complete nested encodings rather than a sequence of a shape
    /// this schema names, because the operation vocabulary belongs to `mesh-operations`. Each
    /// element carries its own domain tag, so a decoder that knows the operation schemas can read
    /// it and one that does not can still skip it.
    const SCHEMA: RecordSchema = RecordSchema::new(
        CHANGESET_DOMAIN,
        &[
            FieldSchema::new("workspace_id", CanonicalType::Bytes(Some(16))),
            FieldSchema::new("actor_id", CanonicalType::Bytes(Some(32))),
            FieldSchema::new("session_id", CanonicalType::Bytes(Some(16))),
            FieldSchema::new("actor_sequence", CanonicalType::Unsigned),
            FieldSchema::new(
                "causal_parents",
                CanonicalType::Sequence(&CanonicalType::Bytes(Some(32))),
            ),
            FieldSchema::new("base_head", CanonicalType::Bytes(Some(32))),
            FieldSchema::new("resulting_head", CanonicalType::Bytes(Some(32))),
            FieldSchema::new(
                "operations",
                CanonicalType::Sequence(&CanonicalType::Record),
            ),
            FieldSchema::new("policy_epoch", CanonicalType::Unsigned),
            FieldSchema::new("hybrid_logical_time", CanonicalType::Group(HLC_FIELDS)),
        ],
    );

    fn canonical_fields(&self) -> Vec<CanonicalValue> {
        vec![
            CanonicalValue::from_array(*self.workspace_id.uuid().as_bytes()),
            CanonicalValue::from_digest(self.actor_id.digest()),
            CanonicalValue::from_array(*self.session_id.uuid().as_bytes()),
            CanonicalValue::Unsigned(self.actor_sequence.value()),
            CanonicalValue::Sequence(
                self.causal_parents
                    .as_slice()
                    .iter()
                    .map(|parent| CanonicalValue::from_digest(parent.digest()))
                    .collect(),
            ),
            CanonicalValue::from_digest(self.base_head.digest()),
            CanonicalValue::from_digest(self.resulting_head.digest()),
            CanonicalValue::Sequence(
                self.operations
                    .iter()
                    .map(|operation| CanonicalValue::Record(encode_canonical(operation)))
                    .collect(),
            ),
            CanonicalValue::Unsigned(self.policy_epoch.value()),
            CanonicalValue::Group(vec![
                CanonicalValue::Unsigned(self.hybrid_logical_time.physical_millis()),
                CanonicalValue::Unsigned(u64::from(self.hybrid_logical_time.logical())),
            ]),
        ]
    }
}

impl fmt::Display for ActorSequence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

impl fmt::Display for PolicyEpoch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}
