//! The FUSE contract-1 evidence and producer matrix.

use std::path::Path;

use mesh_fuse::FuseAdapter;
use mesh_materializer::{
    ActorId, AdapterError, BoundaryObservationV1, BoundaryReasonV1, DestinationBefore,
    DestinationBindingOutcome, EventSequence, FsEvent, FsEventKind, MovedObjectIdentity,
    NormalizedName, ObjectId, OpenMode, PortableMetadata, RenameDisposition, RenameEvidence,
    WorkspaceAdapter, WORKSPACE_ADAPTER_CONTRACT_V1,
};

fn name(value: &str) -> NormalizedName {
    NormalizedName::new(value).expect("fixture name")
}

fn mounted(adapter: &FuseAdapter) -> mesh_materializer::ViewId {
    let fixture = adapter.prepare_fixture().expect("prepare");
    adapter
        .mount_actor_view(
            fixture.workspace(),
            ActorId::from_bytes([0x35; 32]),
            Path::new("mesh-v1/actor"),
        )
        .expect("mount")
        .id()
}

#[test]
fn fuse_claims_contract_one() {
    assert_eq!(
        FuseAdapter::new().describe().contract(),
        WORKSPACE_ADAPTER_CONTRACT_V1
    );
}

#[test]
fn atomic_replace_preserves_the_moved_object_and_replaces_the_binding() {
    let adapter = FuseAdapter::new();
    let view_id = mounted(&adapter);
    let view = adapter.view(view_id).expect("view");
    let temporary = name(".draft.tmp");
    let destination = name("draft.txt");
    let moved = view
        .create_file(view.root(), &temporary, PortableMetadata::default())
        .expect("temporary");
    let displaced = view
        .create_file(view.root(), &destination, PortableMetadata::default())
        .expect("destination");
    let held = view
        .open(displaced.object(), OpenMode::Read)
        .expect("hold displaced object");

    let evidence = view
        .rename_with_evidence(
            EventSequence::new(5),
            view.root(),
            &temporary,
            &destination,
            RenameDisposition::Replace,
        )
        .expect("replace");

    assert_eq!(evidence.view(), view_id);
    assert_eq!(evidence.sequence(), EventSequence::new(5));
    assert_eq!(evidence.source_before().object(), moved.object());
    assert_eq!(
        evidence.destination_before(),
        DestinationBefore::Bound(displaced.object())
    );
    assert_eq!(evidence.destination_after().object(), moved.object());
    assert_eq!(
        evidence.identity().moved_object(),
        MovedObjectIdentity::Preserved
    );
    assert_eq!(
        evidence.identity().destination_binding(),
        DestinationBindingOutcome::Replaced
    );
    assert_eq!(
        view.lookup(view.root(), &destination)
            .expect("new destination")
            .object(),
        moved.object()
    );
    assert_eq!(
        view.lookup(view.root(), &temporary),
        Err(AdapterError::NotFound)
    );

    // POSIX replacement removes the old binding but an outstanding handle still names its object.
    assert!(view.metadata(displaced.object()).is_ok());
    view.close(held).expect("close displaced handle");
    assert_eq!(
        view.metadata(displaced.object()),
        Err(AdapterError::NotFound)
    );
}

#[test]
fn fail_disposition_is_the_frozen_contract_zero_behavior() {
    let adapter = FuseAdapter::new();
    let view_id = mounted(&adapter);
    let view = adapter.view(view_id).expect("view");
    let source = name("source");
    let destination = name("destination");
    let source_entry = view
        .create_file(view.root(), &source, PortableMetadata::default())
        .expect("source");
    let destination_entry = view
        .create_file(view.root(), &destination, PortableMetadata::default())
        .expect("destination");

    assert_eq!(
        view.rename_with_evidence(
            EventSequence::new(1),
            view.root(),
            &source,
            &destination,
            RenameDisposition::Fail,
        ),
        Err(AdapterError::AlreadyExists)
    );
    assert_eq!(
        view.lookup(view.root(), &source).unwrap().object(),
        source_entry.object()
    );
    assert_eq!(
        view.lookup(view.root(), &destination).unwrap().object(),
        destination_entry.object()
    );
}

#[test]
fn move_case_only_and_directory_descendants_keep_exact_bindings() {
    let adapter = FuseAdapter::new();
    let view_id = mounted(&adapter);
    let view = adapter.view(view_id).expect("view");
    let left = view
        .create_directory(view.root(), &name("left"))
        .expect("left");
    let right = view
        .create_directory(view.root(), &name("right"))
        .expect("right");
    let source = view
        .create_file(
            left.object(),
            &name("Readme.md"),
            PortableMetadata::default(),
        )
        .expect("source");
    let displaced = view
        .create_file(
            right.object(),
            &name("README.md"),
            PortableMetadata::default(),
        )
        .expect("destination");
    let evidence = view
        .move_entry_with_evidence(
            EventSequence::new(8),
            left.object(),
            &name("Readme.md"),
            right.object(),
            &name("README.md"),
            RenameDisposition::Replace,
        )
        .expect("move replace");
    assert_eq!(evidence.source_before().name().as_str(), "Readme.md");
    assert_eq!(evidence.destination_after().name().as_str(), "README.md");
    assert_eq!(evidence.source_before().parent(), left.object());
    assert_eq!(evidence.destination_after().parent(), right.object());
    assert_eq!(
        evidence.destination_before(),
        DestinationBefore::Bound(displaced.object())
    );
    assert_eq!(
        view.lookup(right.object(), &name("README.md"))
            .unwrap()
            .object(),
        source.object()
    );

    let directory = view
        .create_directory(view.root(), &name("src"))
        .expect("directory");
    let child = view
        .create_file(
            directory.object(),
            &name("child"),
            PortableMetadata::default(),
        )
        .expect("child");
    let directory_evidence = view
        .rename_with_evidence(
            EventSequence::new(9),
            view.root(),
            &name("src"),
            &name("lib"),
            RenameDisposition::Replace,
        )
        .expect("directory rename");
    assert_eq!(
        directory_evidence.identity().destination_binding(),
        DestinationBindingOutcome::Created
    );
    let renamed_directory = view.lookup(view.root(), &name("lib")).unwrap();
    assert_eq!(renamed_directory.object(), directory.object());
    assert_eq!(
        view.lookup(renamed_directory.object(), &name("child"))
            .unwrap()
            .object(),
        child.object()
    );
}

#[test]
fn the_three_boundary_producers_are_fail_closed() {
    let adapter = FuseAdapter::new();
    let view_id = mounted(&adapter);
    let view = adapter.view(view_id).expect("view");
    let temporary = name(".next");
    let destination = name("current");
    let moved = view
        .create_file(view.root(), &temporary, PortableMetadata::default())
        .expect("temporary");
    view.create_file(view.root(), &destination, PortableMetadata::default())
        .expect("destination");
    let evidence = view
        .rename_with_evidence(
            EventSequence::new(21),
            view.root(),
            &temporary,
            &destination,
            RenameDisposition::Replace,
        )
        .expect("replacement evidence");
    let rename_event = FsEvent::new(
        view_id,
        EventSequence::new(21),
        FsEventKind::Renamed,
        moved.object(),
    );

    match adapter
        .observe_durable_boundary_v1(&rename_event, None)
        .expect("explicit refusal")
    {
        BoundaryObservationV1::Unsupported(unavailable) => {
            assert_eq!(unavailable.code(), "rename-binding-evidence-unavailable");
            assert_eq!(unavailable.missing().len(), 8);
        }
        other => panic!("missing evidence was guessed: {other:?}"),
    }
    assert_eq!(
        adapter
            .observe_durable_boundary_v1(
                &rename_event,
                Some(&RenameEvidence::Available(evidence.clone())),
            )
            .expect("rename producer"),
        BoundaryObservationV1::Candidate(mesh_materializer::CheckpointCandidateV1::new(
            view_id,
            EventSequence::new(21),
            BoundaryReasonV1::RenamedIntoPlace,
        ))
    );
    let wrong_event = FsEvent::new(
        view_id,
        EventSequence::new(22),
        FsEventKind::Renamed,
        moved.object(),
    );
    assert!(matches!(
        adapter
            .observe_durable_boundary_v1(&wrong_event, Some(&RenameEvidence::Available(evidence)),),
        Err(AdapterError::Backend(_))
    ));

    let second_view_id = mounted(&adapter);
    let second_view = adapter.view(second_view_id).expect("second view");
    let handle_one = view
        .open(moved.object(), OpenMode::ReadWrite)
        .expect("open one");
    let handle_two = second_view
        .open(moved.object(), OpenMode::ReadWrite)
        .expect("open two");
    view.write(&handle_one, 0, b"one").expect("modify one");
    second_view
        .write(&handle_two, 3, b"two")
        .expect("modify two");
    view.close(handle_one).expect("close one");
    let closed = FsEvent::new(
        view_id,
        EventSequence::new(23),
        FsEventKind::Closed,
        moved.object(),
    );
    assert_eq!(
        adapter
            .observe_durable_boundary_v1(&closed, None)
            .expect("one handle remains"),
        BoundaryObservationV1::None
    );
    second_view.close(handle_two).expect("close last");
    let last_closed = FsEvent::new(
        second_view_id,
        EventSequence::new(23),
        FsEventKind::Closed,
        moved.object(),
    );
    assert_eq!(
        adapter
            .observe_durable_boundary_v1(&last_closed, None)
            .expect("last handle closed"),
        BoundaryObservationV1::Candidate(mesh_materializer::CheckpointCandidateV1::new(
            second_view_id,
            EventSequence::new(23),
            BoundaryReasonV1::Closed,
        ))
    );

    let synced = FsEvent::new(
        view_id,
        EventSequence::new(24),
        FsEventKind::Synced,
        moved.object(),
    );
    assert_eq!(
        adapter
            .observe_durable_boundary_v1(&synced, None)
            .expect("successful fsync event"),
        BoundaryObservationV1::Candidate(mesh_materializer::CheckpointCandidateV1::new(
            view_id,
            EventSequence::new(24),
            BoundaryReasonV1::Synced,
        ))
    );
}

#[test]
fn replacement_refuses_directory_shape_changes_without_mutation() {
    let adapter = FuseAdapter::new();
    let view_id = mounted(&adapter);
    let view = adapter.view(view_id).expect("view");
    let file = view
        .create_file(view.root(), &name("file"), PortableMetadata::default())
        .expect("file");
    let directory = view
        .create_directory(view.root(), &name("directory"))
        .expect("directory");
    assert_eq!(
        view.rename_with_evidence(
            EventSequence::new(30),
            view.root(),
            &name("file"),
            &name("directory"),
            RenameDisposition::Replace,
        ),
        Err(AdapterError::IsADirectory)
    );
    assert_eq!(
        view.lookup(view.root(), &name("file")).unwrap().object(),
        file.object()
    );
    assert_eq!(
        view.lookup(view.root(), &name("directory"))
            .unwrap()
            .object(),
        directory.object()
    );
}

#[test]
fn absent_evidence_is_an_explicit_unsupported_value() {
    let unavailable = mesh_materializer::RenameEvidenceUnavailable::new([
        mesh_materializer::RenameEvidenceField::DestinationBeforeState,
        mesh_materializer::RenameEvidenceField::DestinationBeforeObject,
    ])
    .expect("missing destination evidence");
    assert_eq!(unavailable.missing().len(), 2);
    assert_eq!(unavailable.code(), "rename-binding-evidence-unavailable");

    // Keep the compiler honest about ObjectId being evidence, not a path surrogate.
    let _: ObjectId = ObjectId::from_bytes([0x55; 16]);
}
