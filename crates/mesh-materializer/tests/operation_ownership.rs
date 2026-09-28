//! The operation-owner boundary and the materializer's exhaustive outcome oracle.

use std::any::TypeId;

use mesh_materializer::{
    apply_operation, materialize, ActorId, AppliedChangeSet, ApprovalId, AttributionConfidence,
    ChangeSetId, ContentHash, DerivationId, DerivationKind, Effect, HeadId, ManifestId,
    NormalizedName, ObjectId, Operation, OperationKind, PortableMetadata, ReadRegion, Rejection,
    ReviewBundleId, StateHash, ValidationOutcome, VersionId, WorkspaceState,
};

fn object(byte: u8) -> ObjectId {
    ObjectId::from_bytes([byte; 16])
}

fn version(byte: u8) -> VersionId {
    VersionId::from_bytes([byte; 32])
}

fn name(text: &str) -> NormalizedName {
    NormalizedName::new(text).unwrap()
}

#[test]
fn materializer_reexports_the_owner_types_and_keeps_state_hash_local() {
    macro_rules! same_type {
        ($name:ident) => {
            assert_eq!(
                TypeId::of::<mesh_materializer::$name>(),
                TypeId::of::<mesh_operations::$name>(),
                stringify!($name)
            );
        };
    }

    same_type!(ActorId);
    same_type!(ApprovalId);
    same_type!(AttributionConfidence);
    same_type!(ChangeSetId);
    same_type!(ContentHash);
    same_type!(DerivationId);
    same_type!(DerivationKind);
    same_type!(HeadId);
    same_type!(IdError);
    same_type!(ManifestId);
    same_type!(NameError);
    same_type!(NormalizedName);
    same_type!(ObjectId);
    same_type!(Operation);
    same_type!(OperationKind);
    same_type!(PortableMetadata);
    same_type!(PreservedEntry);
    same_type!(ReadRegion);
    same_type!(ReviewBundleId);
    same_type!(ValidationOutcome);
    same_type!(VersionId);
    same_type!(WorkspaceId);
    same_type!(SessionId);

    assert_ne!(TypeId::of::<StateHash>(), TypeId::of::<HeadId>());
    assert_ne!(TypeId::of::<StateHash>(), TypeId::of::<ContentHash>());
}

#[test]
fn canonical_create_write_link_bytes_decode_directly_into_materialization() {
    let root = object(0);
    let file = object(1);
    let file_version = version(2);
    let expected_name = name("decoded.txt");
    let operations = vec![
        Operation::CreateFile { object_id: file },
        Operation::WriteFileVersion {
            object_id: file,
            version_id: file_version,
            parent_versions: vec![],
            manifest_id: ManifestId::from_bytes([3; 32]),
            portable_metadata: PortableMetadata::new(true),
        },
        Operation::LinkDirectoryEntry {
            directory_id: root,
            name: expected_name.clone(),
            object_id: file,
            version_id: file_version,
        },
    ];
    let decoded: Vec<mesh_materializer::Operation> =
        mesh_operations::encode_operations(&operations)
            .iter()
            .map(|bytes| mesh_operations::decode_operation(bytes).unwrap())
            .collect();

    let record = AppliedChangeSet::genesis(ChangeSetId::from_bytes([4; 32]), decoded);
    let result = materialize(root, &[record]);

    assert!(result.rejections().is_empty());
    assert_eq!(result.reached(), 3);
    let entry = result
        .state()
        .root_directory()
        .entry(&expected_name)
        .expect("decoded link must bind the decoded name");
    assert_eq!(entry.object_id(), file);
    assert_eq!(entry.version_id(), file_version);
}

#[test]
fn every_operation_kind_has_an_explicit_effect_or_named_rejection() {
    let root = object(0);
    let unknown = object(1);
    let unknown_version = version(2);
    let zero_head = HeadId::from_bytes([0; 32]);
    let one_head = HeadId::from_bytes([1; 32]);
    let operations = vec![
        Operation::CreateFile { object_id: unknown },
        Operation::CreateDirectory { object_id: unknown },
        Operation::WriteFileVersion {
            object_id: unknown,
            version_id: unknown_version,
            parent_versions: vec![],
            manifest_id: ManifestId::from_bytes([3; 32]),
            portable_metadata: PortableMetadata::default(),
        },
        Operation::LinkDirectoryEntry {
            directory_id: root,
            name: name("link"),
            object_id: unknown,
            version_id: unknown_version,
        },
        Operation::UnlinkDirectoryEntry {
            directory_id: root,
            name: name("unlink"),
            object_id: unknown,
        },
        Operation::RenameEntry {
            directory_id: root,
            from_name: name("before"),
            to_name: name("after"),
            object_id: unknown,
        },
        Operation::MoveEntry {
            from_directory_id: root,
            from_name: name("from"),
            to_directory_id: root,
            to_name: name("to"),
            object_id: unknown,
        },
        Operation::DeleteObject { object_id: unknown },
        Operation::RestoreObject {
            object_id: unknown,
            restored_version_id: unknown_version,
        },
        Operation::SetPortableMetadata {
            object_id: unknown,
            version_id: unknown_version,
            portable_metadata: PortableMetadata::new(true),
        },
        Operation::ResolveNameConflict {
            directory_id: root,
            contested_name: name("contested"),
            preserved: vec![],
        },
        Operation::ResolveContentConflict {
            object_id: unknown,
            resulting_version_id: unknown_version,
            preserved_version_ids: vec![],
        },
        Operation::AdvanceActorHead {
            actor_id: ActorId::from_bytes([4; 32]),
            from_head: zero_head,
            to_head: one_head,
        },
        Operation::RecordReadObservation {
            actor_id: ActorId::from_bytes([4; 32]),
            object_id: unknown,
            version_id: unknown_version,
            region: ReadRegion::WholeFile,
            confidence: AttributionConfidence::Unknown,
        },
        Operation::RecordDerivedNode {
            node_id: DerivationId::from_bytes([5; 32]),
            node_kind: DerivationKind::TestResult,
            exact_inputs: vec![unknown_version],
            configuration_digest: ContentHash::from_bytes([6; 32]),
            output_versions: vec![],
            deterministic: true,
        },
        Operation::CreateReviewBundle {
            bundle_id: ReviewBundleId::from_bytes([7; 32]),
            actor_head: one_head,
            base_head: zero_head,
        },
        Operation::RecordValidation {
            subject_head: one_head,
            validator_id: ActorId::from_bytes([8; 32]),
            outcome: ValidationOutcome::Passed,
            evidence: ContentHash::from_bytes([9; 32]),
        },
        Operation::AdvanceCanonicalHead {
            from_head: zero_head,
            to_head: one_head,
            approval_id: ApprovalId::from_bytes([10; 32]),
        },
        Operation::InitializeWorkspace { root_id: root },
    ];

    assert_eq!(
        operations.iter().map(Operation::kind).collect::<Vec<_>>(),
        OperationKind::ALL
    );

    for operation in operations {
        let mut state = WorkspaceState::empty(root);
        let outcome = apply_operation(&mut state, ChangeSetId::from_bytes([11; 32]), &operation);
        match operation.kind() {
            OperationKind::InitializeWorkspace => assert_eq!(outcome, Ok(Effect::AlreadyInEffect)),
            OperationKind::CreateFile => assert_eq!(outcome, Ok(Effect::Applied)),
            OperationKind::CreateDirectory => assert_eq!(outcome, Ok(Effect::Applied)),
            OperationKind::WriteFileVersion => {
                assert!(matches!(outcome, Err(Rejection::UnknownObject { .. })))
            }
            OperationKind::LinkDirectoryEntry => {
                assert!(matches!(outcome, Err(Rejection::UnknownObject { .. })))
            }
            OperationKind::UnlinkDirectoryEntry => {
                assert!(matches!(outcome, Err(Rejection::EntryNotBound { .. })))
            }
            OperationKind::RenameEntry => {
                assert!(matches!(outcome, Err(Rejection::EntryNotBound { .. })))
            }
            OperationKind::MoveEntry => {
                assert!(matches!(outcome, Err(Rejection::EntryNotBound { .. })))
            }
            OperationKind::DeleteObject => {
                assert!(matches!(outcome, Err(Rejection::UnknownObject { .. })))
            }
            OperationKind::RestoreObject => {
                assert!(matches!(outcome, Err(Rejection::UnknownObject { .. })))
            }
            OperationKind::SetPortableMetadata => {
                assert!(matches!(outcome, Err(Rejection::UnknownObject { .. })))
            }
            OperationKind::ResolveNameConflict => {
                assert!(matches!(outcome, Err(Rejection::EmptyResolution { .. })))
            }
            OperationKind::ResolveContentConflict => {
                assert!(matches!(outcome, Err(Rejection::UnknownObject { .. })))
            }
            OperationKind::AdvanceActorHead => assert_eq!(outcome, Ok(Effect::Applied)),
            OperationKind::RecordReadObservation => {
                assert_eq!(outcome, Ok(Effect::OutsideStateGraph))
            }
            OperationKind::RecordDerivedNode => {
                assert_eq!(outcome, Ok(Effect::OutsideStateGraph))
            }
            OperationKind::CreateReviewBundle => {
                assert_eq!(outcome, Ok(Effect::OutsideStateGraph))
            }
            OperationKind::RecordValidation => {
                assert_eq!(outcome, Ok(Effect::OutsideStateGraph))
            }
            OperationKind::AdvanceCanonicalHead => assert_eq!(outcome, Ok(Effect::Applied)),
        }
    }
}

#[test]
fn root_declaration_replay_preserves_existing_content_and_rejects_another_root() {
    let root = object(0);
    let file = object(1);
    let record = ChangeSetId::from_bytes([4; 32]);
    let mut state = WorkspaceState::empty(root);
    let declaration = Operation::InitializeWorkspace { root_id: root };
    assert_eq!(
        apply_operation(&mut state, record, &declaration),
        Ok(Effect::AlreadyInEffect)
    );
    for operation in [
        Operation::CreateFile { object_id: file },
        Operation::WriteFileVersion {
            object_id: file,
            version_id: version(2),
            parent_versions: vec![],
            manifest_id: ManifestId::from_bytes([3; 32]),
            portable_metadata: PortableMetadata::default(),
        },
        Operation::LinkDirectoryEntry {
            directory_id: root,
            name: name("retained"),
            object_id: file,
            version_id: version(2),
        },
    ] {
        assert_eq!(
            apply_operation(&mut state, record, &operation),
            Ok(Effect::Applied)
        );
    }
    let before = state.clone();
    for _ in 0..2 {
        assert_eq!(
            apply_operation(&mut state, record, &declaration),
            Ok(Effect::AlreadyInEffect)
        );
        assert_eq!(state, before);
    }
    assert_eq!(
        apply_operation(
            &mut state,
            record,
            &Operation::InitializeWorkspace { root_id: file }
        ),
        Err(Rejection::RootIdentityMismatch {
            expected: root,
            declared: file
        })
    );
    assert_eq!(state, before);
}
