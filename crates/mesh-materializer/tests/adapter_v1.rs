//! Contract-1 value semantics, independent of any backend.

use mesh_materializer::{
    BoundaryReasonV1, DestinationBefore, DestinationBindingOutcome, EventSequence, FsEvent,
    FsEventKind, MovedObjectIdentity, NormalizedName, ObjectId, RenameBinding,
    RenameBindingEvidence, RenameEvidenceField, RenameEvidenceUnavailable, ViewId,
    WORKSPACE_ADAPTER_CONTRACT, WORKSPACE_ADAPTER_CONTRACT_V1,
};

fn object(byte: u8) -> ObjectId {
    ObjectId::from_bytes([byte; 16])
}

fn name(value: &str) -> NormalizedName {
    NormalizedName::new(value).expect("fixture name")
}

fn evidence(destination_before: DestinationBefore, after: ObjectId) -> RenameBindingEvidence {
    RenameBindingEvidence::new(
        ViewId::new(7),
        EventSequence::new(11),
        RenameBinding::new(object(1), name("draft"), object(3)),
        destination_before,
        RenameBinding::new(object(1), name("final"), after),
    )
}

#[test]
fn contract_zero_is_frozen_and_contract_one_is_explicit() {
    assert_eq!(WORKSPACE_ADAPTER_CONTRACT, "mesh-workspace-adapter/0");
    assert_eq!(WORKSPACE_ADAPTER_CONTRACT_V1, "mesh-workspace-adapter/1");
}

#[test]
fn moved_object_and_destination_binding_are_independent() {
    let replacement = evidence(DestinationBefore::Bound(object(2)), object(3)).identity();
    assert_eq!(replacement.moved_object(), MovedObjectIdentity::Preserved);
    assert_eq!(
        replacement.destination_binding(),
        DestinationBindingOutcome::Replaced
    );

    let substitution = evidence(DestinationBefore::Unbound, object(4)).identity();
    assert_eq!(substitution.moved_object(), MovedObjectIdentity::Changed);
    assert_eq!(
        substitution.destination_binding(),
        DestinationBindingOutcome::Created
    );

    let preserved = evidence(DestinationBefore::Bound(object(3)), object(3)).identity();
    assert_eq!(
        preserved.destination_binding(),
        DestinationBindingOutcome::Preserved
    );
}

#[test]
fn event_association_is_exact_and_carries_no_clock() {
    let evidence = evidence(DestinationBefore::Unbound, object(3));
    assert!(evidence.matches(&FsEvent::new(
        ViewId::new(7),
        EventSequence::new(11),
        FsEventKind::Renamed,
        object(3),
    )));
    assert!(!evidence.matches(&FsEvent::new(
        ViewId::new(7),
        EventSequence::new(12),
        FsEventKind::Renamed,
        object(3),
    )));
    assert_eq!(evidence.source_before().name().as_str(), "draft");
    assert_eq!(evidence.destination_after().name().as_str(), "final");
}

#[test]
fn unsupported_is_nonempty_closed_sorted_and_deduplicated() {
    assert!(RenameEvidenceUnavailable::new([]).is_none());
    let unavailable = RenameEvidenceUnavailable::new([
        RenameEvidenceField::DestinationAfterObject,
        RenameEvidenceField::SourceName,
        RenameEvidenceField::SourceName,
    ])
    .expect("non-empty refusal");
    assert_eq!(
        unavailable.missing(),
        &[
            RenameEvidenceField::SourceName,
            RenameEvidenceField::DestinationAfterObject,
        ]
    );
    assert_eq!(unavailable.code(), "rename-binding-evidence-unavailable");
    assert_eq!(RenameEvidenceUnavailable::all().missing().len(), 8);
}

#[test]
fn boundary_reason_v1_has_exactly_three_published_members() {
    assert_eq!(
        [
            BoundaryReasonV1::Closed,
            BoundaryReasonV1::Synced,
            BoundaryReasonV1::RenamedIntoPlace,
        ]
        .map(BoundaryReasonV1::as_str),
        ["Closed", "Synced", "RenamedIntoPlace"]
    );
}
