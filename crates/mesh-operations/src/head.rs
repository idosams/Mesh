//! The resulting head is derived from the transition, and the derivation is a seam.
//!
//! # The acceptance criterion, and the circularity it walks into
//!
//! *"A ChangeSet cannot be constructed with a resulting head that does not follow from its
//! operations and base head."* The direct reading — bind the operations and the base head into the
//! ChangeSet identifier, derive the head from the identifier — does not close, and the reason is
//! worth stating because it is a real property of the tree this crate landed into:
//!
//! * `mesh-types` derives a ChangeSet's identifier over **ten** bound fields, and `resulting_head`
//!   is one of them (`crates/mesh-types/src/changeset.rs`, `impl Absorb for ChangeSet`).
//! * `mesh-state` derives `resulting_head` from the applied causal set, which contains that same
//!   identifier (`crates/mesh-state/src/advance.rs`, `head_with`).
//!
//! Identifier depends on head; head depends on identifier. Neither crate can see the other — they
//! declare no dependency — so nothing was there to notice.
//!
//! # The ruling, and what has not yet moved
//!
//! `docs/adr/0036-name-a-changeset-by-its-transition-and-let-the-head-it-produces-be-derived.md`
//! settles it in this crate's favour: a ChangeSet is named by the **nine** authored fields —
//! [`TransitionCommitment`], exactly — and `resulting_head` is derived from that identifier by
//! `mesh-state`'s fold over the causal set, never bound into it. Nine fields → identifier → head,
//! acyclic, one pass, no placeholder. The wire encoding keeps all ten fields.
//!
//! **The ruling is landed; the implementation is not.** Plan §14.3 rule 3 gives a protocol schema
//! and its implementation two runs, so `crates/mesh-types/src/changeset.rs` still binds ten fields
//! and `protocol/test-vectors/v0/changeset.json` still publishes the ten-field `record_id_hex`
//! today. Until that second run lands, the seam below is implementable under the ruling and no
//! implementation in this repository has been held against `mesh-types` — which is the gap
//! `01KZECDSPBM2V8KVPRNZNYPKRE` stays open for.
//!
//! # What this crate does instead
//!
//! [`TransitionCommitment`] is the **nine** bound fields — everything a ChangeSet binds except the
//! resulting head — as a canonical record under its own domain tag. It is acyclic by construction:
//! nothing in it depends on the head it commits to. The operations are in it, the base head is in
//! it, and the causal parents are in it, so a head derived from it *follows from the operations and
//! the base head* in the literal sense the criterion asks for.
//!
//! [`ChangeSetDraft::seal`](crate::ChangeSetDraft::seal) takes no resulting head. It takes a
//! [`HeadDerivation`] and asks it. **There is no parameter anywhere in this crate through which a
//! caller can supply a resulting head**, and that — not a runtime comparison — is how the criterion
//! is met on the authoring path.
//!
//! # Why no implementation of [`HeadDerivation`] ships here
//!
//! For the same reason `mesh-state` ships no `HeadDigest`: a default would be a value that looks
//! like a head identifier and is not the one any other implementation computes, and it would be
//! reached for exactly once by a lane in a hurry. The composition root supplies the real one — in
//! this workspace that is `mesh-state`'s fold over the applied causal set, which is the receiver
//! side of the same question. Making the caller supply it is the friction that keeps a second
//! definition of "head" from being born in this file.
//!
//! The consequence, stated plainly rather than implied: **a caller that supplies a
//! [`HeadDerivation`] disagreeing with its peers gets heads its peers refuse.** This crate makes
//! fabrication-by-the-author unrepresentable; it cannot make a wrong composition root correct, and
//! `ChangeSet::verify` is the receiver-side answer to that.

use crate::canonical::{
    encode_canonical, CanonicalEncode, CanonicalType, CanonicalValue, FieldSchema, RecordSchema,
};
use crate::context::{CausalParents, Hlc, PolicyEpoch};
use crate::encoding::encode_operations;
use crate::ids::{ActorId, HeadId, SessionId, WorkspaceId};
use crate::operation::Operation;
use crate::sequence::ActorSequence;

/// The domain tag every transition commitment is encoded under.
pub const TRANSITION_DOMAIN: &str = "mesh.v0.transition";

const HLC_FIELDS: &[FieldSchema] = &[
    FieldSchema::new("physical_millis", CanonicalType::Unsigned),
    FieldSchema::new("logical", CanonicalType::Unsigned),
];

/// The published schema of a transition commitment.
///
/// Deliberately the ChangeSet schema **minus `resulting_head`**, field for field and in the same
/// order, so the two can be read side by side and the one difference is the point.
pub const TRANSITION_SCHEMA: RecordSchema = RecordSchema::new(
    TRANSITION_DOMAIN,
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
        FieldSchema::new(
            "operations",
            CanonicalType::Sequence(&CanonicalType::Record),
        ),
        FieldSchema::new("policy_epoch", CanonicalType::Unsigned),
        FieldSchema::new("hybrid_logical_time", CanonicalType::Group(HLC_FIELDS)),
    ],
);

/// Everything a transition is, except what it produces.
///
/// Built by [`ChangeSetDraft::seal`](crate::ChangeSetDraft::seal) and handed to a
/// [`HeadDerivation`]. It has no public constructor for the same reason `ChangeSet` does not: a
/// commitment assembled from parts a caller chose freely would be a way to ask for a head over a
/// transition that never happened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransitionCommitment {
    workspace_id: WorkspaceId,
    actor_id: ActorId,
    session_id: SessionId,
    actor_sequence: ActorSequence,
    causal_parents: CausalParents,
    base_head: HeadId,
    operations: Vec<Operation>,
    policy_epoch: PolicyEpoch,
    hybrid_logical_time: Hlc,
}

impl TransitionCommitment {
    /// Assemble a commitment. Crate-private: see the type's own documentation.
    #[allow(clippy::too_many_arguments)]
    pub(crate) const fn new(
        workspace_id: WorkspaceId,
        actor_id: ActorId,
        session_id: SessionId,
        actor_sequence: ActorSequence,
        causal_parents: CausalParents,
        base_head: HeadId,
        operations: Vec<Operation>,
        policy_epoch: PolicyEpoch,
        hybrid_logical_time: Hlc,
    ) -> Self {
        Self {
            workspace_id,
            actor_id,
            session_id,
            actor_sequence,
            causal_parents,
            base_head,
            operations,
            policy_epoch,
            hybrid_logical_time,
        }
    }

    /// The workspace.
    #[must_use]
    pub const fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }

    /// The authoring actor.
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

    /// What it does.
    #[must_use]
    pub fn operations(&self) -> &[Operation] {
        &self.operations
    }

    /// The policy epoch it was authored under.
    #[must_use]
    pub const fn policy_epoch(&self) -> PolicyEpoch {
        self.policy_epoch
    }

    /// Its clock reading — for display and tiebreaking, never for causality.
    #[must_use]
    pub const fn hybrid_logical_time(&self) -> Hlc {
        self.hybrid_logical_time
    }

    /// The canonical bytes of this commitment: the pre-image a head derivation absorbs.
    ///
    /// A [`HeadDerivation`] that hashes anything is expected to hash these and nothing else. They
    /// are self-delimiting and carry their own domain tag, so a head derived from them cannot
    /// collide with a digest of the same bytes taken in another domain.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        encode_canonical(self)
    }
}

impl CanonicalEncode for TransitionCommitment {
    fn schema(&self) -> &'static RecordSchema {
        &TRANSITION_SCHEMA
    }

    fn canonical_fields(&self) -> Vec<CanonicalValue> {
        vec![
            CanonicalValue::from_array(*self.workspace_id.as_bytes()),
            CanonicalValue::from_array(*self.actor_id.as_bytes()),
            CanonicalValue::from_array(*self.session_id.as_bytes()),
            CanonicalValue::Unsigned(self.actor_sequence.value()),
            CanonicalValue::Sequence(
                self.causal_parents
                    .as_slice()
                    .iter()
                    .map(|parent| CanonicalValue::from_array(*parent.as_bytes()))
                    .collect(),
            ),
            CanonicalValue::from_array(*self.base_head.as_bytes()),
            CanonicalValue::Sequence(
                encode_operations(&self.operations)
                    .into_iter()
                    .map(CanonicalValue::Record)
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

/// The seam that says what head a transition produces.
///
/// One method, no default body, no blanket implementation, and no implementation in this crate —
/// see the module header for why. A composition root implements it over the workspace's real head
/// fold; a doctest or a simulator implements it over something cheap and says so.
pub trait HeadDerivation {
    /// The head that results from applying `commitment` to `commitment.base_head()`.
    ///
    /// Must be a pure function of the commitment: two actors that apply the same transition to the
    /// same base must reach the same head, and an implementation that consulted a clock, a random
    /// source or ambient state would break that without any test here being able to see it.
    fn resulting_head(&self, commitment: &TransitionCommitment) -> HeadId;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::{decode_canonical, schema_violations};
    use crate::ids::ChangeSetId;
    use crate::ids::ObjectId;
    use crate::name::NormalizedName;

    fn commitment(operations: Vec<Operation>) -> TransitionCommitment {
        TransitionCommitment::new(
            WorkspaceId::from_bytes([0x01; 16]),
            ActorId::from_bytes([0x02; 32]),
            SessionId::from_bytes([0x03; 16]),
            ActorSequence::new(1),
            CausalParents::after(ChangeSetId::from_bytes([0x04; 32]), Vec::new()),
            HeadId::from_bytes([0x05; 32]),
            operations,
            PolicyEpoch::new(7),
            Hlc::new(1_700_000_000_000, 3),
        )
    }

    #[test]
    fn the_commitment_agrees_with_its_published_schema() {
        let value = commitment(crate::corpus::one_of_every_operation());
        assert!(schema_violations(&TRANSITION_SCHEMA, &value.canonical_fields()).is_empty());
        assert!(decode_canonical(&TRANSITION_SCHEMA, &value.canonical_bytes()).is_ok());
    }

    #[test]
    fn the_transition_schema_is_the_changeset_schema_without_the_resulting_head() {
        let names: Vec<&str> = TRANSITION_SCHEMA
            .fields
            .iter()
            .map(|field| field.name)
            .collect();
        assert_eq!(
            names,
            vec![
                "workspace_id",
                "actor_id",
                "session_id",
                "actor_sequence",
                "causal_parents",
                "base_head",
                "operations",
                "policy_epoch",
                "hybrid_logical_time",
            ]
        );
        assert!(!names.contains(&"resulting_head"));
    }

    #[test]
    fn changing_one_operation_changes_the_pre_image() {
        let one = commitment(vec![Operation::CreateFile {
            object_id: ObjectId::from_bytes([9; 16]),
        }]);
        let other = commitment(vec![Operation::CreateFile {
            object_id: ObjectId::from_bytes([10; 16]),
        }]);
        assert_ne!(one.canonical_bytes(), other.canonical_bytes());
    }

    #[test]
    fn reordering_two_operations_changes_the_pre_image() {
        let first = Operation::CreateFile {
            object_id: ObjectId::from_bytes([9; 16]),
        };
        let second = Operation::CreateDirectory {
            object_id: ObjectId::from_bytes([10; 16]),
        };
        assert_ne!(
            commitment(vec![first.clone(), second.clone()]).canonical_bytes(),
            commitment(vec![second, first]).canonical_bytes()
        );
    }

    #[test]
    fn the_pre_image_is_stable_across_repeated_encodings() {
        let value = commitment(vec![Operation::RenameEntry {
            directory_id: ObjectId::from_bytes([1; 16]),
            from_name: NormalizedName::new("a").unwrap(),
            to_name: NormalizedName::new("b").unwrap(),
            object_id: ObjectId::from_bytes([2; 16]),
        }]);
        assert_eq!(value.canonical_bytes(), value.canonical_bytes());
    }
}
