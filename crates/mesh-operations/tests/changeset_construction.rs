//! A ChangeSet's resulting head follows from its operations and its base head, or it is refused.
//!
//! The compile-time half — a draft missing its causal parents, base head or policy epoch has no
//! `seal` — is a `compile_fail` doctest in `src/changeset.rs`, because a compile error cannot be
//! observed from a test binary that has to compile. This file is the runtime half: what happens to
//! a head somebody made up.

mod common;

use mesh_operations::{
    encode_canonical, one_of_every_operation, ActorId, ActorSequence, CausalParents, ChangeSet,
    ChangeSetDraft, ChangeSetId, HeadDerivation, HeadId, Hlc, ObjectId, Operation, PolicyEpoch,
    ReceivedChangeSet, SessionId, Signature, TransitionCommitment, WorkspaceId,
};

/// A derivation for tests. Not a protocol digest and not claiming to be — what it must be is a
/// pure function of the commitment, which is the property every test here depends on.
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

fn draft(base: HeadId, sequence: u64) -> ChangeSetDraft<CausalParents, HeadId, PolicyEpoch> {
    ChangeSetDraft::new(
        WorkspaceId::from_bytes([1; 16]),
        ActorId::from_bytes([2; 32]),
        SessionId::from_bytes([3; 16]),
        ActorSequence::new(sequence),
        Hlc::new(1_700_000_000_000, 3),
    )
    .causal_parents(CausalParents::after(
        ChangeSetId::from_bytes([4; 32]),
        Vec::new(),
    ))
    .base_head(base)
    .policy_epoch(PolicyEpoch::new(7))
}

fn seal(base: HeadId, operations: Vec<Operation>) -> ChangeSet {
    draft(base, 1).seal(operations, &Fnv, Signature::from_bytes([0; 64]))
}

fn received_with(honest: &ChangeSet, claimed: HeadId) -> ReceivedChangeSet {
    ReceivedChangeSet::new(
        honest.workspace_id(),
        honest.actor_id(),
        honest.session_id(),
        honest.actor_sequence(),
        honest.causal_parents().clone(),
        honest.base_head(),
        claimed,
        honest.operations().to_vec(),
        honest.policy_epoch(),
        honest.hybrid_logical_time(),
        *honest.signature(),
    )
}

#[test]
fn a_fabricated_resulting_head_is_refused_and_the_refusal_names_both() {
    let honest = seal(
        HeadId::from_bytes([5; 32]),
        vec![Operation::CreateFile {
            object_id: ObjectId::from_bytes([9; 16]),
        }],
    );
    let fabricated = HeadId::from_bytes([0xff; 32]);
    let refusal = received_with(&honest, fabricated).verify(&Fnv).unwrap_err();
    assert_eq!(refusal.claimed, fabricated);
    assert_eq!(refusal.derived, honest.resulting_head());
    assert!(refusal.to_string().contains(&fabricated.to_string()));
}

/// The head that a *neighbouring* transition produces is the fabrication a naive check misses: it
/// is a real head, derived by the real derivation, from the wrong transition.
#[test]
fn a_head_borrowed_from_another_transition_is_refused() {
    let base = HeadId::from_bytes([5; 32]);
    let one = seal(
        base,
        vec![Operation::CreateFile {
            object_id: ObjectId::from_bytes([9; 16]),
        }],
    );
    let other = seal(
        base,
        vec![Operation::CreateFile {
            object_id: ObjectId::from_bytes([10; 16]),
        }],
    );
    assert_ne!(one.resulting_head(), other.resulting_head());
    assert!(received_with(&one, other.resulting_head())
        .verify(&Fnv)
        .is_err());
}

#[test]
fn changing_the_base_head_alone_moves_the_resulting_head() {
    let operations = vec![Operation::DeleteObject {
        object_id: ObjectId::from_bytes([9; 16]),
    }];
    let one = seal(HeadId::from_bytes([5; 32]), operations.clone());
    let other = seal(HeadId::from_bytes([6; 32]), operations);
    assert_ne!(one.resulting_head(), other.resulting_head());
}

#[test]
fn changing_one_operation_alone_moves_the_resulting_head() {
    let base = HeadId::from_bytes([5; 32]);
    let corpus = one_of_every_operation();
    let all = seal(base, corpus.clone());
    for index in 0..corpus.len() {
        let mut without = corpus.clone();
        without.remove(index);
        assert_ne!(
            seal(base, without).resulting_head(),
            all.resulting_head(),
            "dropping operation {index} left the head unchanged"
        );
    }
}

#[test]
fn reordering_the_operations_moves_the_resulting_head() {
    let base = HeadId::from_bytes([5; 32]);
    let mut corpus = one_of_every_operation();
    let forwards = seal(base, corpus.clone());
    corpus.reverse();
    assert_ne!(
        seal(base, corpus).resulting_head(),
        forwards.resulting_head()
    );
}

#[test]
fn an_honest_record_verifies_and_becomes_a_changeset() {
    let honest = seal(HeadId::from_bytes([5; 32]), one_of_every_operation());
    let accepted = received_with(&honest, honest.resulting_head())
        .verify(&Fnv)
        .unwrap();
    assert_eq!(accepted, honest);
    assert!(accepted.verify(&Fnv).is_ok());
}

/// Sealing is deterministic: the same draft and the same operations produce the same head, in this
/// process and in any other. An implementation that consulted a clock would fail this.
#[test]
fn sealing_the_same_transition_twice_produces_the_same_head() {
    let base = HeadId::from_bytes([5; 32]);
    let once = seal(base, one_of_every_operation());
    let twice = seal(base, one_of_every_operation());
    assert_eq!(once.resulting_head(), twice.resulting_head());
    assert_eq!(encode_canonical(&once), encode_canonical(&twice));
}

/// The actor sequence is one of the nine bound fields, so two ChangeSets that differ only in
/// their position in the author's sequence are different transitions.
#[test]
fn the_actor_sequence_is_bound_into_the_head() {
    let base = HeadId::from_bytes([5; 32]);
    let operations = vec![Operation::CreateDirectory {
        object_id: ObjectId::from_bytes([9; 16]),
    }];
    let first = draft(base, 1).seal(operations.clone(), &Fnv, Signature::from_bytes([0; 64]));
    let second = draft(base, 2).seal(operations, &Fnv, Signature::from_bytes([0; 64]));
    assert_ne!(first.resulting_head(), second.resulting_head());
}

/// The signature is not bound into the head or the encoding — a record that bound its own
/// signature could never be signed.
#[test]
fn the_signature_changes_neither_the_head_nor_the_encoding() {
    let base = HeadId::from_bytes([5; 32]);
    let operations = vec![Operation::CreateDirectory {
        object_id: ObjectId::from_bytes([9; 16]),
    }];
    let unsigned = draft(base, 1).seal(operations.clone(), &Fnv, Signature::from_bytes([0; 64]));
    let signed = draft(base, 1).seal(operations, &Fnv, Signature::from_bytes([0xab; 64]));
    assert_eq!(unsigned.resulting_head(), signed.resulting_head());
    assert_eq!(encode_canonical(&unsigned), encode_canonical(&signed));
    assert_ne!(
        unsigned.signature().as_bytes(),
        signed.signature().as_bytes()
    );
}

/// The genesis case: a ChangeSet with no causal parents is a statement, and it is a different
/// transition from one that follows something.
#[test]
fn genesis_and_a_followed_transition_produce_different_heads() {
    let base = HeadId::from_bytes([5; 32]);
    let operations = vec![Operation::CreateFile {
        object_id: ObjectId::from_bytes([9; 16]),
    }];
    let genesis = ChangeSetDraft::new(
        WorkspaceId::from_bytes([1; 16]),
        ActorId::from_bytes([2; 32]),
        SessionId::from_bytes([3; 16]),
        ActorSequence::new(1),
        Hlc::new(1_700_000_000_000, 3),
    )
    .causal_parents(CausalParents::genesis())
    .base_head(base)
    .policy_epoch(PolicyEpoch::new(7))
    .seal(operations.clone(), &Fnv, Signature::from_bytes([0; 64]));

    assert!(genesis.causal_parents().is_genesis());
    assert_ne!(
        genesis.resulting_head(),
        seal(base, operations).resulting_head()
    );
}

/// The clock reading is bound into the record — it is one of the ten published fields — but it is
/// never consulted for order. This pins the first half; the vocabulary carries no comparison that
/// could do the second.
#[test]
fn the_clock_reading_is_bound_but_never_ordered_on() {
    let base = HeadId::from_bytes([5; 32]);
    let operations = vec![Operation::CreateFile {
        object_id: ObjectId::from_bytes([9; 16]),
    }];
    let early = ChangeSetDraft::new(
        WorkspaceId::from_bytes([1; 16]),
        ActorId::from_bytes([2; 32]),
        SessionId::from_bytes([3; 16]),
        ActorSequence::new(1),
        Hlc::new(1, 0),
    )
    .causal_parents(CausalParents::genesis())
    .base_head(base)
    .policy_epoch(PolicyEpoch::new(7))
    .seal(operations.clone(), &Fnv, Signature::from_bytes([0; 64]));
    let late = ChangeSetDraft::new(
        WorkspaceId::from_bytes([1; 16]),
        ActorId::from_bytes([2; 32]),
        SessionId::from_bytes([3; 16]),
        ActorSequence::new(1),
        Hlc::new(u64::MAX, u32::MAX),
    )
    .causal_parents(CausalParents::genesis())
    .base_head(base)
    .policy_epoch(PolicyEpoch::new(7))
    .seal(operations, &Fnv, Signature::from_bytes([0; 64]));
    assert_ne!(early.resulting_head(), late.resulting_head());
    assert_eq!(early.causal_parents(), late.causal_parents());
}
