//! ChangeSet construction: the causal context is a typestate, and the resulting head is derived.
//!
//! # Two guarantees, made two different ways, because they are two different kinds of claim
//!
//! **A ChangeSet cannot exist without its causal parents, base head and policy epoch.** That is a
//! *presence* claim, and presence is decidable at compile time: [`ChangeSetDraft`] carries the
//! three as type parameters, each starting as the zero-sized [`Unset`] and being replaced by the
//! field's own type, and [`ChangeSetDraft::seal`] exists only on the draft that has all three.
//! There is no unwrap in this module and no runtime check to forget. The negative case is a
//! compile error and is tested as one:
//!
//! ```compile_fail,E0599
//! use mesh_operations::{CausalParents, ChangeSetDraft, HeadId};
//! # use mesh_operations::{ActorId, ActorSequence, Hlc, SessionId, Signature, WorkspaceId};
//! # struct Zero;
//! # impl mesh_operations::HeadDerivation for Zero {
//! #     fn resulting_head(&self, _: &mesh_operations::TransitionCommitment) -> HeadId {
//! #         HeadId::from_bytes([0; 32])
//! #     }
//! # }
//! let draft = ChangeSetDraft::new(
//!     WorkspaceId::from_bytes([1; 16]),
//!     ActorId::from_bytes([2; 32]),
//!     SessionId::from_bytes([3; 16]),
//!     ActorSequence::new(1),
//!     Hlc::new(0, 0),
//! )
//! .causal_parents(CausalParents::genesis())
//! .base_head(HeadId::from_bytes([4; 32]));
//! // No `.policy_epoch(…)`: `seal` does not exist on this type.
//! let changeset = draft.seal(Vec::new(), &Zero, Signature::from_bytes([0; 64]));
//! ```
//!
//! The same program with `.policy_epoch(PolicyEpoch::new(0))` inserted compiles — that is
//! [`ChangeSetDraft::seal`]'s own documentation example, and a compile-fail test whose positive
//! twin is missing proves only that the snippet is broken.
//!
//! **A ChangeSet's resulting head follows from its operations and its base head.** That is a
//! *derivation* claim, and it cannot be a typestate: no type discipline can tell a correct thirty-two
//! bytes from an incorrect thirty-two bytes. So it is made structurally instead — by removing the
//! parameter. [`ChangeSetDraft::seal`] takes operations, a [`HeadDerivation`] and a signature, and
//! **no resulting head**. There is nowhere to put a fabricated one.
//!
//! A record arriving from a peer *does* carry a head its author claims, so that path is
//! [`ReceivedChangeSet`], which is not a [`ChangeSet`] and cannot become one except through
//! [`ReceivedChangeSet::verify`]. `tests/changeset_construction.rs` fabricates a head and requires
//! the refusal.

use core::fmt;
use core::marker::PhantomData;

use crate::canonical::{CanonicalEncode, CanonicalType, CanonicalValue, FieldSchema, RecordSchema};
use crate::context::{CausalParents, Hlc, PolicyEpoch, Signature};
use crate::encoding::encode_operations;
use crate::head::{HeadDerivation, TransitionCommitment};
use crate::ids::{ActorId, HeadId, SessionId, WorkspaceId};
use crate::operation::Operation;
use crate::sequence::ActorSequence;

/// The domain tag every ChangeSet is encoded under.
///
/// The same tag `mesh-types` uses, and the schema below is that crate's field for field: the
/// published vectors in `protocol/test-vectors/v0/changeset.json` are reproduced byte for byte by
/// `tests/published_vectors.rs`, which is what makes "the same encoding" a measurement rather than
/// an intention.
pub const CHANGESET_DOMAIN: &str = "mesh.v0.changeset";

const HLC_FIELDS: &[FieldSchema] = &[
    FieldSchema::new("physical_millis", CanonicalType::Unsigned),
    FieldSchema::new("logical", CanonicalType::Unsigned),
];

/// The published schema of a ChangeSet: ten bound fields, and never the signature.
///
/// The signature is made over the record, so a record that bound its own signature could never be
/// signed.
pub const CHANGESET_SCHEMA: RecordSchema = RecordSchema::new(
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

/// The typestate marker for a draft field that has not been supplied yet.
///
/// A zero-sized placeholder. Once the field is supplied, the field's own type replaces this one in
/// the draft's parameter list, so the draft carries the value rather than an option over it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Unset;

/// A ChangeSet under construction.
///
/// The three type parameters are the causal parents, the base head and the policy epoch. Each
/// starts as [`Unset`] and becomes the field's own type when supplied. Each setter exists only
/// while its field is [`Unset`], so a field cannot be supplied twice either.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeSetDraft<Parents = Unset, Base = Unset, Epoch = Unset> {
    workspace_id: WorkspaceId,
    actor_id: ActorId,
    session_id: SessionId,
    actor_sequence: ActorSequence,
    hybrid_logical_time: Hlc,
    causal_parents: Parents,
    base_head: Base,
    policy_epoch: Epoch,
    operations: PhantomData<fn() -> Operation>,
}

impl ChangeSetDraft<Unset, Unset, Unset> {
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

impl<Base, Epoch> ChangeSetDraft<Unset, Base, Epoch> {
    /// State what this ChangeSet causally follows.
    #[must_use]
    pub fn causal_parents(
        self,
        causal_parents: CausalParents,
    ) -> ChangeSetDraft<CausalParents, Base, Epoch> {
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

impl<Parents, Epoch> ChangeSetDraft<Parents, Unset, Epoch> {
    /// State the head this ChangeSet was authored against.
    #[must_use]
    pub fn base_head(self, base_head: HeadId) -> ChangeSetDraft<Parents, HeadId, Epoch> {
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

impl<Parents, Base> ChangeSetDraft<Parents, Base, Unset> {
    /// State the policy epoch this ChangeSet was authored under.
    #[must_use]
    pub fn policy_epoch(
        self,
        policy_epoch: PolicyEpoch,
    ) -> ChangeSetDraft<Parents, Base, PolicyEpoch> {
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

impl ChangeSetDraft<CausalParents, HeadId, PolicyEpoch> {
    /// Seal the draft into a [`ChangeSet`], deriving the resulting head.
    ///
    /// This method exists only on a draft whose causal parents, base head and policy epoch have
    /// all been supplied — the impl block's type parameters are the enforcement, not anything this
    /// body does. And it takes **no resulting head**: the head comes from `derivation`, applied to
    /// a [`TransitionCommitment`] over the operations and the base head.
    ///
    /// ```
    /// use mesh_operations::{
    ///     ActorId, ActorSequence, CausalParents, ChangeSetDraft, HeadDerivation, HeadId, Hlc,
    ///     ObjectId, Operation, PolicyEpoch, SessionId, Signature, TransitionCommitment,
    ///     WorkspaceId,
    /// };
    ///
    /// // The seam. A composition root supplies the workspace's real fold; an example supplies
    /// // this, and says out loud that it is not a protocol digest.
    /// struct ExampleDerivation;
    /// impl HeadDerivation for ExampleDerivation {
    ///     fn resulting_head(&self, commitment: &TransitionCommitment) -> HeadId {
    ///         let mut state: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    ///         for byte in commitment.canonical_bytes() {
    ///             state = (state ^ u128::from(byte))
    ///                 .wrapping_mul(0x0100_0000_0000_0000_0000_013b);
    ///         }
    ///         let mut out = [0u8; 32];
    ///         out[..16].copy_from_slice(&state.to_be_bytes());
    ///         out[16..].copy_from_slice(&state.rotate_left(37).to_be_bytes());
    ///         HeadId::from_bytes(out)
    ///     }
    /// }
    ///
    /// let base = HeadId::from_bytes([2; 32]);
    /// let changeset = ChangeSetDraft::new(
    ///     WorkspaceId::from_bytes([1; 16]),
    ///     ActorId::from_bytes([1; 32]),
    ///     SessionId::from_bytes([2; 16]),
    ///     ActorSequence::new(1),
    ///     Hlc::new(0, 0),
    /// )
    /// .causal_parents(CausalParents::genesis())
    /// .base_head(base)
    /// .policy_epoch(PolicyEpoch::new(0))
    /// .seal(
    ///     vec![Operation::CreateFile { object_id: ObjectId::from_bytes([9; 16]) }],
    ///     &ExampleDerivation,
    ///     Signature::from_bytes([0; 64]),
    /// );
    ///
    /// assert!(changeset.causal_parents().is_genesis());
    /// assert_eq!(changeset.base_head(), base);
    /// // The head was derived, and re-deriving it agrees.
    /// assert!(changeset.verify(&ExampleDerivation).is_ok());
    /// ```
    #[must_use]
    pub fn seal<D: HeadDerivation + ?Sized>(
        self,
        operations: Vec<Operation>,
        derivation: &D,
        signature: Signature,
    ) -> ChangeSet {
        let commitment = TransitionCommitment::new(
            self.workspace_id,
            self.actor_id,
            self.session_id,
            self.actor_sequence,
            self.causal_parents,
            self.base_head,
            operations,
            self.policy_epoch,
            self.hybrid_logical_time,
        );
        let resulting_head = derivation.resulting_head(&commitment);
        ChangeSet {
            commitment,
            resulting_head,
            signature,
        }
    }
}

/// One authored transition: who caused it, what it followed, what it did and what it produced.
///
/// There is no public constructor and no public field. The only two ways to obtain one are
/// [`ChangeSetDraft::seal`] and [`ReceivedChangeSet::verify`], and **both derive the resulting
/// head**. Holding a `ChangeSet` therefore means the head has been derived from the transition by
/// somebody, not that an author asserted it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeSet {
    commitment: TransitionCommitment,
    resulting_head: HeadId,
    signature: Signature,
}

impl ChangeSet {
    /// The workspace this transition belongs to.
    #[must_use]
    pub const fn workspace_id(&self) -> WorkspaceId {
        self.commitment.workspace_id()
    }

    /// The actor that authored it.
    #[must_use]
    pub const fn actor_id(&self) -> ActorId {
        self.commitment.actor_id()
    }

    /// The session it was authored in.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.commitment.session_id()
    }

    /// Its position in the authoring actor's sequence.
    #[must_use]
    pub const fn actor_sequence(&self) -> ActorSequence {
        self.commitment.actor_sequence()
    }

    /// What it causally follows.
    #[must_use]
    pub const fn causal_parents(&self) -> &CausalParents {
        self.commitment.causal_parents()
    }

    /// The head it was authored against.
    #[must_use]
    pub const fn base_head(&self) -> HeadId {
        self.commitment.base_head()
    }

    /// The head applying it produces. Derived, never supplied.
    #[must_use]
    pub const fn resulting_head(&self) -> HeadId {
        self.resulting_head
    }

    /// What it does.
    #[must_use]
    pub fn operations(&self) -> &[Operation] {
        self.commitment.operations()
    }

    /// The policy epoch it was authored under.
    #[must_use]
    pub const fn policy_epoch(&self) -> PolicyEpoch {
        self.commitment.policy_epoch()
    }

    /// Its clock reading — for display and tiebreaking, never for causality.
    #[must_use]
    pub const fn hybrid_logical_time(&self) -> Hlc {
        self.commitment.hybrid_logical_time()
    }

    /// The authoring actor's signature over the ten bound fields.
    #[must_use]
    pub const fn signature(&self) -> &Signature {
        &self.signature
    }

    /// The nine-field commitment this transition's head was derived from.
    #[must_use]
    pub const fn commitment(&self) -> &TransitionCommitment {
        &self.commitment
    }

    /// Re-derive the head and compare.
    ///
    /// Cheap and worth calling even on a locally sealed ChangeSet: it is the assertion that this
    /// receiver's derivation agrees with whoever produced the record.
    ///
    /// # Errors
    ///
    /// [`HeadRefused`] carrying both heads when they differ.
    pub fn verify<D: HeadDerivation + ?Sized>(&self, derivation: &D) -> Result<(), HeadRefused> {
        let derived = derivation.resulting_head(&self.commitment);
        if derived == self.resulting_head {
            Ok(())
        } else {
            Err(HeadRefused {
                claimed: self.resulting_head,
                derived,
            })
        }
    }
}

/// A ChangeSet as it arrived, before its head was checked.
///
/// This type exists because a record off the wire genuinely does carry a head somebody else
/// derived, and pretending otherwise would push the claimed value into a `ChangeSet` unchecked.
/// It has every field a [`ChangeSet`] has and none of its meaning: the only thing to do with one
/// is [`ReceivedChangeSet::verify`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceivedChangeSet {
    commitment: TransitionCommitment,
    claimed_resulting_head: HeadId,
    signature: Signature,
}

impl ReceivedChangeSet {
    /// A record as it arrived.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        workspace_id: WorkspaceId,
        actor_id: ActorId,
        session_id: SessionId,
        actor_sequence: ActorSequence,
        causal_parents: CausalParents,
        base_head: HeadId,
        claimed_resulting_head: HeadId,
        operations: Vec<Operation>,
        policy_epoch: PolicyEpoch,
        hybrid_logical_time: Hlc,
        signature: Signature,
    ) -> Self {
        Self {
            commitment: TransitionCommitment::new(
                workspace_id,
                actor_id,
                session_id,
                actor_sequence,
                causal_parents,
                base_head,
                operations,
                policy_epoch,
                hybrid_logical_time,
            ),
            claimed_resulting_head,
            signature,
        }
    }

    /// The head its author claims. Not to be trusted; that is what
    /// [`ReceivedChangeSet::verify`] is for.
    #[must_use]
    pub const fn claimed_resulting_head(&self) -> HeadId {
        self.claimed_resulting_head
    }

    /// Its position in the authoring actor's sequence, readable before verification so a
    /// [`SequenceWitness`](crate::SequenceWitness) can see a gap in records it goes on to refuse.
    #[must_use]
    pub const fn actor_sequence(&self) -> ActorSequence {
        self.commitment.actor_sequence()
    }

    /// The actor that authored it, readable before verification for the same reason.
    #[must_use]
    pub const fn actor_id(&self) -> ActorId {
        self.commitment.actor_id()
    }

    /// Derive the head from the transition and accept the record only if it agrees.
    ///
    /// # Errors
    ///
    /// [`HeadRefused`] carrying both heads when they differ. A refused record is not a
    /// [`ChangeSet`], so there is no way to go on using it as one.
    pub fn verify<D: HeadDerivation + ?Sized>(
        self,
        derivation: &D,
    ) -> Result<ChangeSet, HeadRefused> {
        let derived = derivation.resulting_head(&self.commitment);
        if derived != self.claimed_resulting_head {
            return Err(HeadRefused {
                claimed: self.claimed_resulting_head,
                derived,
            });
        }
        Ok(ChangeSet {
            commitment: self.commitment,
            resulting_head: derived,
            signature: self.signature,
        })
    }
}

/// A claimed resulting head that the transition does not produce.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeadRefused {
    /// What the author said.
    pub claimed: HeadId,
    /// What the transition actually produces.
    pub derived: HeadId,
}

impl fmt::Display for HeadRefused {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "the record claims the resulting head {} and the transition produces {}",
            self.claimed, self.derived
        )
    }
}

impl std::error::Error for HeadRefused {}

impl CanonicalEncode for ChangeSet {
    fn schema(&self) -> &'static RecordSchema {
        &CHANGESET_SCHEMA
    }

    fn canonical_fields(&self) -> Vec<CanonicalValue> {
        vec![
            CanonicalValue::from_array(*self.workspace_id().as_bytes()),
            CanonicalValue::from_array(*self.actor_id().as_bytes()),
            CanonicalValue::from_array(*self.session_id().as_bytes()),
            CanonicalValue::Unsigned(self.actor_sequence().value()),
            CanonicalValue::Sequence(
                self.causal_parents()
                    .as_slice()
                    .iter()
                    .map(|parent| CanonicalValue::from_array(*parent.as_bytes()))
                    .collect(),
            ),
            CanonicalValue::from_array(*self.base_head().as_bytes()),
            CanonicalValue::from_array(*self.resulting_head.as_bytes()),
            CanonicalValue::Sequence(
                encode_operations(self.operations())
                    .into_iter()
                    .map(CanonicalValue::Record)
                    .collect(),
            ),
            CanonicalValue::Unsigned(self.policy_epoch().value()),
            CanonicalValue::Group(vec![
                CanonicalValue::Unsigned(self.hybrid_logical_time().physical_millis()),
                CanonicalValue::Unsigned(u64::from(self.hybrid_logical_time().logical())),
            ]),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::schema_violations;
    use crate::ids::{ChangeSetId, ObjectId};

    /// A derivation for tests only. It is not a protocol digest and does not claim to be; what it
    /// has to be is a pure function of the commitment, which is all these tests exercise.
    struct Fnv;

    impl HeadDerivation for Fnv {
        fn resulting_head(&self, commitment: &TransitionCommitment) -> HeadId {
            let mut state: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
            for byte in commitment.canonical_bytes() {
                state = (state ^ u128::from(byte)).wrapping_mul(0x0100_0000_0000_0000_0000_013b);
            }
            let mut out = [0u8; 32];
            out[..16].copy_from_slice(&state.to_be_bytes());
            out[16..].copy_from_slice(&state.rotate_left(37).to_be_bytes());
            HeadId::from_bytes(out)
        }
    }

    /// A derivation that always answers the same thing, to show `verify` is comparing rather than
    /// agreeing with itself by construction.
    struct Constant([u8; 32]);

    impl HeadDerivation for Constant {
        fn resulting_head(&self, _: &TransitionCommitment) -> HeadId {
            HeadId::from_bytes(self.0)
        }
    }

    fn sealed(operations: Vec<Operation>) -> ChangeSet {
        ChangeSetDraft::new(
            WorkspaceId::from_bytes([1; 16]),
            ActorId::from_bytes([2; 32]),
            SessionId::from_bytes([3; 16]),
            ActorSequence::new(1),
            Hlc::new(1_700_000_000_000, 3),
        )
        .causal_parents(CausalParents::after(
            ChangeSetId::from_bytes([4; 32]),
            Vec::new(),
        ))
        .base_head(HeadId::from_bytes([5; 32]))
        .policy_epoch(PolicyEpoch::new(7))
        .seal(operations, &Fnv, Signature::from_bytes([0; 64]))
    }

    #[test]
    fn a_sealed_changeset_verifies_against_the_derivation_that_sealed_it() {
        let changeset = sealed(crate::corpus::one_of_every_operation());
        assert!(changeset.verify(&Fnv).is_ok());
    }

    #[test]
    fn the_resulting_head_moves_when_the_operations_move() {
        let one = sealed(vec![Operation::CreateFile {
            object_id: ObjectId::from_bytes([9; 16]),
        }]);
        let other = sealed(vec![Operation::CreateFile {
            object_id: ObjectId::from_bytes([10; 16]),
        }]);
        assert_ne!(one.resulting_head(), other.resulting_head());
    }

    #[test]
    fn a_fabricated_resulting_head_is_refused() {
        let honest = sealed(vec![Operation::DeleteObject {
            object_id: ObjectId::from_bytes([9; 16]),
        }]);
        let fabricated = HeadId::from_bytes([0xff; 32]);
        let received = ReceivedChangeSet::new(
            honest.workspace_id(),
            honest.actor_id(),
            honest.session_id(),
            honest.actor_sequence(),
            honest.causal_parents().clone(),
            honest.base_head(),
            fabricated,
            honest.operations().to_vec(),
            honest.policy_epoch(),
            honest.hybrid_logical_time(),
            *honest.signature(),
        );
        assert_eq!(received.claimed_resulting_head(), fabricated);
        let refusal = received.verify(&Fnv).unwrap_err();
        assert_eq!(refusal.claimed, fabricated);
        assert_eq!(refusal.derived, honest.resulting_head());
    }

    #[test]
    fn an_honest_record_survives_the_round_trip_through_the_received_form() {
        let honest = sealed(vec![Operation::CreateDirectory {
            object_id: ObjectId::from_bytes([9; 16]),
        }]);
        let received = ReceivedChangeSet::new(
            honest.workspace_id(),
            honest.actor_id(),
            honest.session_id(),
            honest.actor_sequence(),
            honest.causal_parents().clone(),
            honest.base_head(),
            honest.resulting_head(),
            honest.operations().to_vec(),
            honest.policy_epoch(),
            honest.hybrid_logical_time(),
            *honest.signature(),
        );
        assert_eq!(received.verify(&Fnv).unwrap(), honest);
    }

    #[test]
    fn verify_compares_rather_than_agreeing_with_itself() {
        let changeset = sealed(Vec::new());
        assert!(changeset.verify(&Constant([0xaa; 32])).is_err());
    }

    #[test]
    fn a_changeset_agrees_with_its_published_schema() {
        let changeset = sealed(crate::corpus::one_of_every_operation());
        assert!(schema_violations(&CHANGESET_SCHEMA, &changeset.canonical_fields()).is_empty());
    }

    #[test]
    fn the_changeset_schema_carries_the_resulting_head_and_never_the_signature() {
        let names: Vec<&str> = CHANGESET_SCHEMA
            .fields
            .iter()
            .map(|field| field.name)
            .collect();
        assert!(names.contains(&"resulting_head"));
        assert!(!names.contains(&"signature"));
        assert_eq!(names.len(), 10);
    }
}
