//! ADR-0015's condition, enforced by a test rather than by prose.
//!
//! ADR-0015 answered plan §11 research item R1 — *can actor heads converge without a global lock?*
//! — with "conditionally yes", and named the condition:
//!
//! > A peer MUST NOT admit a ChangeSet identifier into head advancement until it has recomputed
//! > that identifier from the record's canonical bytes under the protocol digest and found it
//! > equal. A record whose identifier does not match its bytes is refused, not buffered.
//!
//! Its evidence table's last row read *"A receiver verifies the binding | none — no implementation
//! exists | **unenforced**"*. These tests are what that row is about.
//!
//! # The divergence, driven through the receipt path
//!
//! ADR-0015's reduction to n = 2: two records carry one identifier, by one author, differing in
//! exactly one field — one names a causal parent, the other names none. Two peers that each apply
//! one of them hold the same identifier set, report nothing outstanding, reach two different heads,
//! refuse nothing, and afterwards permanently reject every honest ChangeSet the other authors.
//!
//! [`the_two_record_divergence_cannot_reach_head_advancement`] builds exactly that pair from real
//! canonical bytes and drives both through [`mesh_sync_engine::admit`]. At most one of the two can
//! survive the comparison, because at most one identifier is the digest of the bytes beside it. The
//! second is refused, so the two peers cannot both apply, and cannot reach two heads.

use mesh_sync_engine::{admit, EmptyOperations, Refusal};
use mesh_sync_protocol::{
    ActorId as WireActorId, ActorSequence as WireSequence, CarriedChangeSet,
    ChangeSetId as WireChangeSetId, HeadId as WireHeadId, PolicyEpoch as WirePolicyEpoch,
};
use mesh_types::{
    derive_id, encode_canonical, ActorId, ActorSequence, Blake3, CausalParents, ChangeSet,
    ChangeSetDraft, ChangeSetId, Digest32, HeadId, Hlc, PolicyEpoch, SessionId, Signature,
    WorkspaceId,
};

// -------------------------------------------------------------------------------------------------
// Building real records
// -------------------------------------------------------------------------------------------------

/// One authored transition with no operations, so `EmptyOperations` is a complete decoder for it.
///
/// `sequence` and `parents` are the two things the tests vary; everything else is fixed so a
/// difference in the derived identifier can only have come from one of them.
fn seal(sequence: u64, parents: CausalParents) -> ChangeSet<()> {
    ChangeSetDraft::<()>::new(
        WorkspaceId::mint(1_700_000_000_000, [1; 10]),
        ActorId::from_digest(Digest32::from_bytes([2; 32])),
        SessionId::mint(1_700_000_000_000, [3; 10]),
        ActorSequence::new(sequence),
        Hlc::new(1_700_000_000_000, 4),
    )
    .causal_parents(parents)
    .base_head(HeadId::from_digest(Digest32::from_bytes([5; 32])))
    .policy_epoch(PolicyEpoch::new(7))
    .seal(
        Vec::new(),
        HeadId::from_digest(Digest32::from_bytes([9; 32])),
        Signature::from_bytes([0; 64]),
    )
}

/// The identifier a record's own bytes derive — the value the receipt path recomputes.
fn identifier(record: &ChangeSet<()>) -> ChangeSetId {
    derive_id::<Blake3, _>(record)
}

fn wire_id(id: ChangeSetId) -> WireChangeSetId {
    WireChangeSetId::from_bytes(*id.digest().as_bytes())
}

/// An honest `CarriedChangeSet`: every header field taken from the record it carries.
fn carry(record: &ChangeSet<()>) -> CarriedChangeSet {
    CarriedChangeSet {
        id: wire_id(identifier(record)),
        author: WireActorId::from_bytes(*record.actor_id().digest().as_bytes()),
        sequence: WireSequence::new(record.actor_sequence().value()),
        parents: record
            .causal_parents()
            .as_slice()
            .iter()
            .map(|parent| wire_id(*parent))
            .collect(),
        base_head: WireHeadId::from_bytes(*record.base_head().digest().as_bytes()),
        resulting_head: WireHeadId::from_bytes(*record.resulting_head().digest().as_bytes()),
        policy_epoch: WirePolicyEpoch::new(record.policy_epoch().value()),
        body: encode_canonical(record),
    }
}

// -------------------------------------------------------------------------------------------------
// The criteria
// -------------------------------------------------------------------------------------------------

/// **The check is not vacuous.** An honest record round-trips through the receipt path and is
/// admitted, so the refusals below are refusals and not a blanket.
#[test]
fn an_honest_carried_changeset_is_admitted() {
    let record = seal(1, CausalParents::genesis());
    let carried = carry(&record);

    let admitted = admit(&carried, &EmptyOperations).expect("an honest record is admitted");

    assert_eq!(admitted.id(), carried.id);
    assert_eq!(admitted.author(), carried.author);
    assert_eq!(admitted.sequence(), carried.sequence);
    assert_eq!(admitted.parents(), carried.parents.as_slice());
    assert_eq!(admitted.base_head(), carried.base_head);
    assert_eq!(admitted.resulting_head(), carried.resulting_head);
    assert_eq!(admitted.policy_epoch(), carried.policy_epoch);
}

/// **The criterion.** A record whose identifier disagrees with its bytes is refused by name.
#[test]
fn a_changeset_whose_identifier_disagrees_with_its_bytes_is_refused() {
    let record = seal(1, CausalParents::genesis());
    let mut carried = carry(&record);
    let honest = carried.id;
    carried.id = WireChangeSetId::from_bytes([0xff; 32]);

    let refusal =
        admit(&carried, &EmptyOperations).expect_err("a fabricated identifier is refused");

    match refusal {
        Refusal::IdentifierMismatch { claimed, derived } => {
            assert_eq!(claimed, WireChangeSetId::from_bytes([0xff; 32]));
            assert_eq!(
                derived, honest,
                "the derived value is the record's own name"
            );
        }
        other => panic!("expected an identifier mismatch, got {other:?}"),
    }
}

/// **ADR-0015's two-record divergence, driven through the receipt path.**
///
/// One identifier, two causal parent sets. Peer A applies the record that follows `a`; peer B is
/// offered the twin that follows nothing under the same name. Before this check existed both were
/// applied and the two peers reached two heads with no refusal on either side. Here the twin never
/// becomes an `AdmittedChangeSet`, so it cannot reach head advancement at all.
#[test]
fn the_two_record_divergence_cannot_reach_head_advancement() {
    let first = seal(1, CausalParents::genesis());
    let a = identifier(&first);

    // b: sequence 2, following a. b': sequence 2, following nothing. They differ in exactly one
    // bound field, which is what makes this the ADR's minimal case.
    let b = seal(2, CausalParents::after(a, Vec::new()));
    let b_twin = seal(2, CausalParents::genesis());
    assert_ne!(
        identifier(&b),
        identifier(&b_twin),
        "the identifier binds the causal parent set; if this ever fails, the divergence is back \
         and the repair is in mesh-types, not here"
    );

    // Peer A's view of b is honest and is admitted.
    let honest = carry(&b);
    let admitted = admit(&honest, &EmptyOperations).expect("b is honest");
    assert_eq!(admitted.parents(), &[wire_id(a)]);

    // The divergent record: the twin's bytes under b's name, exactly the pair ADR-0015 constructed.
    let divergent = CarriedChangeSet {
        id: wire_id(identifier(&b)),
        parents: Vec::new(),
        body: encode_canonical(&b_twin),
        ..carry(&b_twin)
    };

    let refusal = admit(&divergent, &EmptyOperations)
        .expect_err("one identifier cannot name two causal parent sets");
    assert!(
        matches!(refusal, Refusal::IdentifierMismatch { .. }),
        "expected an identifier mismatch, got {refusal:?}"
    );

    // And the other direction: b's bytes under the twin's name are refused too, so the pair cannot
    // be admitted by choosing which peer is asked first.
    let mirrored = CarriedChangeSet {
        id: wire_id(identifier(&b_twin)),
        ..carry(&b)
    };
    assert!(matches!(
        admit(&mirrored, &EmptyOperations),
        Err(Refusal::IdentifierMismatch { .. })
    ));
}

/// Every bound field is bound: moving any one of them moves the identifier, so a record edited in
/// any of them is refused rather than admitted with a stale name.
#[test]
fn a_record_edited_in_any_bound_field_is_refused() {
    let original = seal(
        3,
        CausalParents::after(
            ChangeSetId::from_digest(Digest32::from_bytes([1; 32])),
            Vec::new(),
        ),
    );
    let name = wire_id(identifier(&original));

    let edited = [
        ("actor_sequence", seal(4, original.causal_parents().clone())),
        ("causal_parents", seal(3, CausalParents::genesis())),
    ];

    for (field, variant) in edited {
        let carried = CarriedChangeSet {
            id: name,
            body: encode_canonical(&variant),
            ..carry(&variant)
        };
        assert!(
            matches!(
                admit(&carried, &EmptyOperations),
                Err(Refusal::IdentifierMismatch { .. })
            ),
            "editing {field} left the record admissible under its old name"
        );
    }
}

/// A body that is not a canonical ChangeSet is refused before any identifier is compared, because
/// there is no identifier to compare.
#[test]
fn a_body_that_is_not_a_changeset_is_refused_rather_than_admitted() {
    let record = seal(1, CausalParents::genesis());
    let carried = CarriedChangeSet {
        body: vec![0x80],
        ..carry(&record)
    };
    assert!(matches!(
        admit(&carried, &EmptyOperations),
        Err(Refusal::UndecodableBody(_))
    ));
}

/// A body truncated by one byte is refused. The canonical reader rejects trailing and missing bytes
/// alike, so a partial record cannot be admitted as a shorter one.
#[test]
fn a_truncated_body_is_refused() {
    let record = seal(1, CausalParents::genesis());
    let mut body = encode_canonical(&record);
    body.pop();
    let carried = CarriedChangeSet {
        body,
        ..carry(&record)
    };
    assert!(matches!(
        admit(&carried, &EmptyOperations),
        Err(Refusal::UndecodableBody(_))
    ));
}

/// A header field that disagrees with the body it is redundant with is refused, so a receiver's
/// *plan* and a receiver's *record* are never about two different ChangeSets.
#[test]
fn a_header_field_that_disagrees_with_the_body_is_refused() {
    let record = seal(1, CausalParents::genesis());
    let carried = CarriedChangeSet {
        base_head: WireHeadId::from_bytes([0xab; 32]),
        ..carry(&record)
    };
    match admit(&carried, &EmptyOperations).expect_err("a lying header is refused") {
        Refusal::HeaderDisagreesWithBody { field } => assert_eq!(field, "base_head"),
        other => panic!("expected a header disagreement, got {other:?}"),
    }
}

/// An operation the composition cannot decode is refused, not skipped. Skipping it would leave the
/// identifier re-derivable and wrong — the operations are bound, so a dropped operation is a
/// different record.
#[test]
fn an_undecodable_operation_is_refused_rather_than_skipped() {
    // The body of a ChangeSet whose one operation is a record this composition does not know. Built
    // by hand at the canonical layer, because `ChangeSet<()>` cannot carry one.
    let record = seal(1, CausalParents::genesis());
    let mut body = encode_canonical(&record);
    let operations_marker = encode_canonical::<()>(&());
    // The empty operations sequence is a zero-length array (`0x80`); replace it with a one-element
    // array holding an encoding this decoder refuses.
    let foreign = encode_canonical::<mesh_types::FileManifest>(&mesh_types::FileManifest::new(
        0,
        Digest32::from_bytes([0; 32]),
        Vec::new(),
    ));
    let at = body
        .windows(1)
        .enumerate()
        .rev()
        .find_map(|(index, window)| (window == [0x80]).then_some(index))
        .expect("the empty operations array is in the encoding");
    let mut replacement = vec![0x81];
    replacement.extend_from_slice(&foreign);
    body.splice(at..=at, replacement);
    assert_ne!(operations_marker, foreign);

    let carried = CarriedChangeSet {
        body,
        ..carry(&record)
    };
    match admit(&carried, &EmptyOperations).expect_err("an unknown operation is refused") {
        Refusal::UndecodableBody(_) => {}
        other => panic!("expected the body to be refused, got {other:?}"),
    }
}
