//! Contract-1 evidence and negative producer coverage for the folder-watching fallback.

#![cfg(unix)]

use std::fs;
use std::path::Path;

use mesh_daemon::folder_watch::{watch, DirectoryAdapter, DECLARED};
use mesh_materializer::{
    ActorId, AdapterCapability, AdapterError, BoundaryObservationV1, DestinationBefore,
    DestinationBindingOutcome, EventSequence, FsEvent, FsEventKind, MovedObjectIdentity,
    NormalizedName, PortableMetadata, RenameDisposition, RenameEvidence, ViewId,
    WorkspaceAdapter as _, WORKSPACE_ADAPTER_CONTRACT_V1,
};

fn name(value: &str) -> NormalizedName {
    NormalizedName::new(value).expect("fixture name")
}

fn mounted(label: &str) -> (DirectoryAdapter, ViewId) {
    let adapter = DirectoryAdapter::in_scratch(label);
    let fixture = adapter.prepare_fixture().expect("prepare folder backend");
    let mounted = adapter
        .mount_actor_view(
            fixture.workspace(),
            ActorId::from_bytes([9; 32]),
            Path::new("/contract-one"),
        )
        .expect("mount folder view");
    (adapter, mounted.id())
}

#[test]
fn folder_watch_opts_into_contract_one_without_claiming_boundaries() {
    let (adapter, view_id) = mounted("v1-declaration");
    assert_eq!(adapter.describe().contract(), WORKSPACE_ADAPTER_CONTRACT_V1);
    assert!(!DECLARED.contains(AdapterCapability::ObserveDurableBoundary));

    let closed = FsEvent::new(
        view_id,
        EventSequence::new(1),
        FsEventKind::Closed,
        adapter.view(view_id).unwrap().root(),
    );
    assert_eq!(
        adapter.observe_durable_boundary(&closed),
        Err(AdapterError::unsupported(
            AdapterCapability::ObserveDurableBoundary
        ))
    );
    assert_eq!(
        adapter.observe_durable_boundary_v1(&closed, None),
        Err(AdapterError::unsupported(
            AdapterCapability::ObserveDurableBoundary
        ))
    );
    let synced = FsEvent::new(
        view_id,
        EventSequence::new(2),
        FsEventKind::Synced,
        adapter.view(view_id).unwrap().root(),
    );
    assert_eq!(
        adapter.observe_durable_boundary_v1(&synced, None),
        Err(AdapterError::unsupported(
            AdapterCapability::ObserveDurableBoundary
        ))
    );

    let view = adapter.view(view_id).unwrap();
    let source = view
        .create_file(view.root(), &name("next"), PortableMetadata::default())
        .unwrap();
    view.create_file(view.root(), &name("current"), PortableMetadata::default())
        .unwrap();
    let evidence = view
        .rename_with_evidence(
            EventSequence::new(3),
            view.root(),
            &name("next"),
            &name("current"),
            RenameDisposition::Replace,
        )
        .unwrap();
    let renamed = FsEvent::new(
        view_id,
        EventSequence::new(3),
        FsEventKind::Renamed,
        source.object(),
    );
    assert!(matches!(
        adapter.observe_durable_boundary_v1(&renamed, Some(&RenameEvidence::Available(evidence))),
        Ok(BoundaryObservationV1::Unsupported(_))
    ));
}

#[test]
fn performed_rename_replace_and_move_return_exact_identity_evidence() {
    let (adapter, view_id) = mounted("v1-performed");
    let view = adapter.view(view_id).expect("view");
    let draft = view
        .create_file(view.root(), &name("draft.txt"), PortableMetadata::default())
        .expect("draft");
    let renamed = view
        .rename_with_evidence(
            EventSequence::new(10),
            view.root(),
            &name("draft.txt"),
            &name("final.txt"),
            RenameDisposition::Fail,
        )
        .expect("exact rename");
    assert_eq!(renamed.source_before().object(), draft.object());
    assert_eq!(renamed.destination_before(), DestinationBefore::Unbound);
    assert_eq!(renamed.destination_after().object(), draft.object());

    let temporary = view
        .create_file(view.root(), &name(".next"), PortableMetadata::default())
        .expect("replacement source");
    let displaced = view
        .create_file(view.root(), &name("current"), PortableMetadata::default())
        .expect("replacement destination");
    let replaced = view
        .rename_with_evidence(
            EventSequence::new(20),
            view.root(),
            &name(".next"),
            &name("current"),
            RenameDisposition::Replace,
        )
        .expect("exact replacement");
    assert_eq!(
        replaced.destination_before(),
        DestinationBefore::Bound(displaced.object())
    );
    assert_eq!(replaced.destination_after().object(), temporary.object());
    assert_eq!(
        replaced.identity().destination_binding(),
        DestinationBindingOutcome::Replaced
    );

    let directory = view
        .create_directory(view.root(), &name("src"))
        .expect("source directory");
    let child = view
        .create_file(
            directory.object(),
            &name("child"),
            PortableMetadata::default(),
        )
        .expect("child");
    let moved = view
        .rename_with_evidence(
            EventSequence::new(30),
            view.root(),
            &name("src"),
            &name("lib"),
            RenameDisposition::Fail,
        )
        .expect("directory rename");
    assert_eq!(
        moved.identity().moved_object(),
        MovedObjectIdentity::Preserved
    );
    let lib = view
        .lookup(view.root(), &name("lib"))
        .expect("renamed directory");
    assert_eq!(lib.object(), directory.object());
    assert_eq!(
        view.lookup(lib.object(), &name("child")).unwrap().object(),
        child.object()
    );
}

#[test]
fn case_only_rename_reports_the_filesystems_actual_binding() {
    let (adapter, view_id) = mounted("v1-case");
    let folder = adapter.workspace_root().join("contract-one");
    let view = adapter.view(view_id).expect("view");
    let created = view
        .create_file(view.root(), &name("Readme.md"), PortableMetadata::default())
        .expect("mixed-case file");
    let destination_was_alias = fs::symlink_metadata(folder.join("README.md")).is_ok();
    let evidence = view
        .rename_with_evidence(
            EventSequence::new(40),
            view.root(),
            &name("Readme.md"),
            &name("README.md"),
            RenameDisposition::Replace,
        )
        .expect("case-only rename");
    assert_eq!(evidence.destination_after().object(), created.object());
    assert_eq!(
        evidence.identity().destination_binding(),
        if destination_was_alias {
            DestinationBindingOutcome::Preserved
        } else {
            DestinationBindingOutcome::Created
        }
    );
}

#[test]
fn snapshots_emit_evidence_only_for_one_unambiguous_object_move() {
    let (adapter, view_id) = mounted("v1-snapshot");
    let folder = adapter.workspace_root().join("contract-one");
    fs::write(folder.join("draft.txt"), b"same object").expect("draft");
    let before = watch::Snapshot::of(&folder);
    fs::rename(folder.join("draft.txt"), folder.join("final.txt")).expect("rename");
    let after = watch::Snapshot::of(&folder);
    let evidence = watch::rename_evidence(
        &before,
        &after,
        view_id,
        EventSequence::new(50),
        "draft.txt",
        "final.txt",
    );
    let RenameEvidence::Available(evidence) = evidence else {
        panic!("an unambiguous identity-preserving move was refused");
    };
    assert_eq!(
        evidence.identity().moved_object(),
        MovedObjectIdentity::Preserved
    );
    assert_eq!(
        evidence.identity().destination_binding(),
        DestinationBindingOutcome::Created
    );

    let absent = watch::rename_evidence(
        &before,
        &after,
        view_id,
        EventSequence::new(51),
        "missing.txt",
        "final.txt",
    );
    let RenameEvidence::Unsupported(unavailable) = absent else {
        panic!("missing source evidence was guessed");
    };
    assert_eq!(unavailable.code(), "rename-binding-evidence-unavailable");
    assert_eq!(unavailable.missing().len(), 8);

    fs::hard_link(folder.join("final.txt"), folder.join("alias.txt")).expect("ambiguous alias");
    let ambiguous = watch::Snapshot::of(&folder);
    assert!(matches!(
        watch::rename_evidence(
            &before,
            &ambiguous,
            view_id,
            EventSequence::new(52),
            "draft.txt",
            "final.txt",
        ),
        RenameEvidence::Unsupported(_)
    ));
}

#[test]
fn snapshot_replacement_and_directory_descendants_preserve_separate_facts() {
    let (adapter, view_id) = mounted("v1-snapshot-replace");
    let folder = adapter.workspace_root().join("contract-one");
    fs::write(folder.join("next"), b"new").expect("source");
    fs::write(folder.join("current"), b"old").expect("destination");
    fs::create_dir(folder.join("src")).expect("directory");
    fs::write(folder.join("src/child"), b"child").expect("child");
    let before = watch::Snapshot::of(&folder);
    let child_before = before.get("src/child").unwrap().object();
    fs::rename(folder.join("next"), folder.join("current")).expect("replace");
    fs::rename(folder.join("src"), folder.join("lib")).expect("directory rename");
    let after = watch::Snapshot::of(&folder);

    let RenameEvidence::Available(replaced) = watch::rename_evidence(
        &before,
        &after,
        view_id,
        EventSequence::new(60),
        "next",
        "current",
    ) else {
        panic!("replacement evidence unavailable");
    };
    assert_eq!(
        replaced.identity().destination_binding(),
        DestinationBindingOutcome::Replaced
    );

    let RenameEvidence::Available(directory) = watch::rename_evidence(
        &before,
        &after,
        view_id,
        EventSequence::new(61),
        "src",
        "lib",
    ) else {
        panic!("directory evidence unavailable");
    };
    assert_eq!(
        directory.identity().moved_object(),
        MovedObjectIdentity::Preserved
    );
    assert_eq!(after.get("lib/child").unwrap().object(), child_before);
}
