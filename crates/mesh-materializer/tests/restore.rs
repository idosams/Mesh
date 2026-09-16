//! Append-only file restore, exact manifest identity, undo, restart and refusal coverage.

use mesh_materializer::{
    materialize, plan_file_restore, AppliedChangeSet, ChangeSetId, ManifestId, NormalizedName,
    ObjectId, Operation, PortableMetadata, RestoreRefusal, VersionId,
};

fn object(byte: u8) -> ObjectId {
    ObjectId::from_bytes([byte; 16])
}

fn version(byte: u8) -> VersionId {
    VersionId::from_bytes([byte; 32])
}

fn manifest(byte: u8) -> ManifestId {
    ManifestId::from_bytes([byte; 32])
}

fn changeset(byte: u8) -> ChangeSetId {
    ChangeSetId::from_bytes([byte; 32])
}

fn name() -> NormalizedName {
    NormalizedName::new("notes.txt").expect("portable name")
}

fn history() -> (ObjectId, ObjectId, Vec<AppliedChangeSet>) {
    let root = object(0);
    let file = object(1);
    let other_file = object(2);
    let first = AppliedChangeSet::genesis(
        changeset(1),
        vec![
            Operation::CreateFile { object_id: file },
            Operation::WriteFileVersion {
                object_id: file,
                version_id: version(1),
                parent_versions: vec![],
                manifest_id: manifest(1),
                portable_metadata: PortableMetadata::new(false),
            },
            Operation::CreateFile {
                object_id: other_file,
            },
            Operation::WriteFileVersion {
                object_id: other_file,
                version_id: version(7),
                parent_versions: vec![],
                manifest_id: manifest(7),
                portable_metadata: PortableMetadata::new(false),
            },
            Operation::LinkDirectoryEntry {
                directory_id: root,
                name: name(),
                object_id: file,
                version_id: version(1),
            },
        ],
    );
    let second = AppliedChangeSet::new(
        changeset(2),
        vec![first.id()],
        vec![
            Operation::UnlinkDirectoryEntry {
                directory_id: root,
                name: name(),
                object_id: file,
            },
            Operation::WriteFileVersion {
                object_id: file,
                version_id: version(2),
                parent_versions: vec![version(1)],
                manifest_id: manifest(2),
                portable_metadata: PortableMetadata::new(true),
            },
            Operation::LinkDirectoryEntry {
                directory_id: root,
                name: name(),
                object_id: file,
                version_id: version(2),
            },
        ],
    );
    (root, file, vec![first, second])
}

#[test]
fn restore_is_new_work_and_restarts_at_the_exact_earlier_manifest() {
    let (root, file, mut journal) = history();
    let original_records = journal.clone();
    let before = materialize(root, &journal);
    assert!(before.rejections().is_empty());

    let plan = plan_file_restore(before.state(), file, version(1)).expect("exact restore plan");
    assert_eq!(plan.from_version(), version(2));
    assert_eq!(plan.to_version(), version(1));
    assert_eq!(plan.operations().len(), 4);
    journal.push(AppliedChangeSet::new(
        changeset(3),
        vec![changeset(2)],
        plan.operations().to_vec(),
    ));

    // A fresh materialization is the restart path: truth comes only from the immutable records.
    let restarted = materialize(root, &journal);
    assert!(restarted.rejections().is_empty());
    assert_eq!(&journal[..2], original_records.as_slice());
    assert_eq!(
        restarted.order(),
        [changeset(1), changeset(2), changeset(3)]
    );
    assert_visible_version(restarted.state(), file, version(1), manifest(1), false);
    assert!(
        restarted.state().file_version(version(2)).is_some(),
        "the intervening version remains retained and undoable"
    );

    let second_restart = materialize(root, &journal);
    assert_eq!(restarted, second_restart);
}

#[test]
fn undo_is_another_append_and_round_trips_the_visible_file() {
    let (root, file, mut journal) = history();
    let source = materialize(root, &journal);
    let plan = plan_file_restore(source.state(), file, version(1)).expect("restore plan");
    journal.push(AppliedChangeSet::new(
        changeset(3),
        vec![changeset(2)],
        plan.operations().to_vec(),
    ));
    let restored = materialize(root, &journal);

    let undo = plan.undo(restored.state()).expect("checked inverse plan");
    assert_eq!(undo.to_version(), version(2));
    journal.push(AppliedChangeSet::new(
        changeset(4),
        vec![changeset(3)],
        undo.operations().to_vec(),
    ));
    let after_undo = materialize(root, &journal);
    assert!(after_undo.rejections().is_empty());
    assert_visible_version(after_undo.state(), file, version(2), manifest(2), true);
    assert_eq!(journal.len(), 4, "restore and undo each append one record");
}

#[test]
fn refusal_paths_leave_the_source_state_byte_identical() {
    let (root, file, journal) = history();
    let current = materialize(root, &journal);
    let bytes = current.state().canonical_bytes();

    let cases = [
        plan_file_restore(current.state(), object(9), version(1)),
        plan_file_restore(current.state(), root, version(1)),
        plan_file_restore(current.state(), file, version(9)),
        plan_file_restore(current.state(), file, version(7)),
        plan_file_restore(current.state(), file, version(2)),
    ];
    assert!(matches!(
        cases[0],
        Err(RestoreRefusal::UnknownObject { .. })
    ));
    assert!(matches!(cases[1], Err(RestoreRefusal::NotAFile { .. })));
    assert!(matches!(
        cases[2],
        Err(RestoreRefusal::UnknownTargetVersion { .. })
    ));
    assert!(matches!(
        cases[3],
        Err(RestoreRefusal::TargetBelongsToAnotherObject { .. })
    ));
    assert!(matches!(
        cases[4],
        Err(RestoreRefusal::AlreadyAtVersion { .. })
    ));
    assert_eq!(current.state().canonical_bytes(), bytes);
}

#[test]
fn deleted_sources_and_undo_against_the_wrong_state_fail_closed() {
    let (root, file, mut journal) = history();
    let current = materialize(root, &journal);
    let plan = plan_file_restore(current.state(), file, version(1)).expect("restore plan");
    assert!(matches!(
        plan.undo(current.state()),
        Err(RestoreRefusal::UndoStateMismatch { .. })
    ));

    journal.push(AppliedChangeSet::new(
        changeset(3),
        vec![changeset(2)],
        vec![
            Operation::UnlinkDirectoryEntry {
                directory_id: root,
                name: name(),
                object_id: file,
            },
            Operation::DeleteObject { object_id: file },
        ],
    ));
    let deleted = materialize(root, &journal);
    assert!(matches!(
        plan_file_restore(deleted.state(), file, version(1)),
        Err(RestoreRefusal::SourceDeleted { .. })
    ));
}

fn assert_visible_version(
    state: &mesh_materializer::WorkspaceState,
    file: ObjectId,
    expected_version: VersionId,
    expected_manifest: ManifestId,
    expected_executable: bool,
) {
    let record = state.object(file).expect("file object remains present");
    assert!(!record.is_deleted());
    assert_eq!(record.current_version(), Some(expected_version));
    let entry = state
        .root_directory()
        .entry(&name())
        .expect("same path remains visible");
    assert_eq!(entry.object_id(), file);
    assert_eq!(entry.version_id(), expected_version);
    let version_record = state
        .file_version(expected_version)
        .expect("immutable target version remains retained");
    assert_eq!(version_record.manifest_id(), expected_manifest);
    assert_eq!(
        version_record.portable_metadata(),
        PortableMetadata::new(expected_executable)
    );
}
