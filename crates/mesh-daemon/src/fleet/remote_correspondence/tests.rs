use super::*;
use crate::fleet::{RemoteInputChunk, RemoteInputEntry};
use crate::workspace::{HistoricalWorkspaceDirectory, HistoricalWorkspacePreviewFile};
use mesh_materializer::ObjectId;
fn snapshot() -> HistoricalWorkspacePreview {
    HistoricalWorkspacePreview {
        operation: RecordDigest::from_bytes([3; 32]),
        directories: vec![HistoricalWorkspaceDirectory {
            object: ObjectId::from_bytes([1; 16]),
            path: "folder".into(),
        }],
        files: vec![HistoricalWorkspacePreviewFile {
            object: ObjectId::from_bytes([2; 16]),
            path: "folder/file".into(),
            manifest_id: RecordDigest::from_bytes([5; 32]),
            byte_length: 7,
            content_digest: RecordDigest::from_bytes([6; 32]),
            executable: false,
        }],
    }
}
fn manifest(snapshot: &HistoricalWorkspacePreview, operation: RecordDigest) -> RemoteInputManifest {
    let entries = snapshot
        .directories
        .iter()
        .map(|v| RemoteInputEntry::Directory {
            path: v.path.clone(),
        })
        .chain(snapshot.files.iter().map(|v| {
            let digest = mesh_cas::Digest32::from_bytes(*v.content_digest.as_bytes());
            RemoteInputEntry::File {
                path: v.path.clone(),
                executable: v.executable,
                digest,
                chunks: vec![RemoteInputChunk {
                    digest,
                    bytes: v.byte_length,
                }],
            }
        }))
        .collect();
    RemoteInputManifest::new(operation, entries).unwrap()
}
#[test]
fn exact_history_keeps_renamed_identity_and_does_not_adopt_a_replacement_at_the_old_path() {
    let initial = snapshot();
    let input = manifest(&initial, RecordDigest::from_bytes([8; 32]));
    let mut saved = initial.clone();
    saved.operation = RecordDigest::from_bytes([9; 32]);
    saved.files[0].path = "renamed".into();
    let mut new = initial.files[0].clone();
    new.object = ObjectId::from_bytes([4; 16]);
    saved.files.push(new);
    let result = manifest(&saved, saved.operation);
    let proof = RemoteResultCorrespondence::derive(&input, &initial, &result, &saved).unwrap();
    let value = Json::parse(proof.encoded()).unwrap();
    let rows = value.get("entries").unwrap().as_array().unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[1].get("path"), Some(&Json::text("folder/file")));
    assert_eq!(rows[1].get("input_path"), Some(&Json::Null));
    assert_eq!(rows[2].get("path"), Some(&Json::text("renamed")));
    assert_eq!(rows[2].get("input_path"), Some(&Json::text("folder/file")));
    saved.files.reverse();
    assert_eq!(
        proof.digest(),
        RemoteResultCorrespondence::derive(&input, &initial, &result, &saved)
            .unwrap()
            .digest()
    );
    saved.files.retain(|v| v.path == "renamed");
    saved.directories.clear();
    let removed = RemoteResultCorrespondence::derive(
        &input,
        &initial,
        &manifest(&saved, saved.operation),
        &saved,
    )
    .unwrap();
    assert_eq!(
        Json::parse(removed.encoded())
            .unwrap()
            .get("entries")
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn wrong_content_duplicate_identity_kind_changes_and_wrong_version_refuse() {
    let initial = snapshot();
    let input = manifest(&initial, RecordDigest::from_bytes([8; 32]));
    let mut saved = initial.clone();
    saved.operation = RecordDigest::from_bytes([9; 32]);
    let result = manifest(&saved, saved.operation);
    let mut changed = saved.clone();
    changed.files[0].byte_length += 1;
    assert!(RemoteResultCorrespondence::derive(&input, &initial, &result, &changed).is_err());
    let mut changed = saved.clone();
    changed.files[0].object = changed.directories[0].object;
    assert!(RemoteResultCorrespondence::derive(&input, &initial, &result, &changed).is_err());
    let mut changed = saved.clone();
    changed.directories.clear();
    changed.files[0].path = "folder".into();
    changed.files[0].object = initial.directories[0].object;
    assert!(RemoteResultCorrespondence::derive(
        &input,
        &initial,
        &manifest(&changed, changed.operation),
        &changed
    )
    .is_err());
    let wrong = manifest(&saved, RecordDigest::from_bytes([10; 32]));
    assert!(RemoteResultCorrespondence::derive(&input, &initial, &wrong, &saved).is_err());
    let mut wrong_initial = initial.clone();
    wrong_initial.files[0].executable = true;
    assert!(RemoteResultCorrespondence::derive(&input, &wrong_initial, &result, &saved).is_err());
}

#[test]
fn decoder_requires_complete_canonical_manifest_bound_correspondence() {
    let initial = snapshot();
    let input = manifest(&initial, RecordDigest::from_bytes([8; 32]));
    let mut saved = initial.clone();
    saved.operation = RecordDigest::from_bytes([9; 32]);
    let result = manifest(&saved, saved.operation);
    let proof = RemoteResultCorrespondence::derive(&input, &initial, &result, &saved).unwrap();
    assert_eq!(
        RemoteResultCorrespondence::decode_bound(
            proof.encoded(),
            &input,
            &result,
            initial.operation
        )
        .unwrap()
        .digest(),
        proof.digest()
    );
    let Json::Object(fields) = Json::parse(proof.encoded()).unwrap() else {
        panic!("object");
    };
    for mutation in 0..8 {
        let mut fields = fields.clone();
        match mutation {
            0 => fields.push(("extra".into(), Json::Null)),
            1 => {
                fields
                    .iter_mut()
                    .find(|(k, _)| k == "worker_initial")
                    .unwrap()
                    .1 = Json::text("00".repeat(32))
            }
            2 => fields.reverse(),
            _ => {
                let Json::Array(rows) =
                    &mut fields.iter_mut().find(|(k, _)| k == "entries").unwrap().1
                else {
                    panic!("rows");
                };
                match mutation {
                    3 => {
                        rows.pop();
                    }
                    4 => rows.reverse(),
                    _ => {
                        let Json::Object(row) = &mut rows[1] else {
                            panic!("row");
                        };
                        match mutation {
                            5 => {
                                row.iter_mut().find(|(k, _)| k == "object").unwrap().1 =
                                    Json::text(initial.directories[0].object.to_string())
                            }
                            6 => {
                                row.iter_mut().find(|(k, _)| k == "input_path").unwrap().1 =
                                    Json::text("folder")
                            }
                            _ => {
                                row.iter_mut().find(|(k, _)| k == "path").unwrap().1 =
                                    Json::text("../escape")
                            }
                        }
                    }
                }
            }
        }
        assert!(
            RemoteResultCorrespondence::decode_bound(
                &Json::Object(fields).encode(),
                &input,
                &result,
                initial.operation
            )
            .is_err(),
            "mutation {mutation}"
        );
    }
    assert!(RemoteResultCorrespondence::decode_bound(
        &(proof.encoded().to_owned() + " "),
        &input,
        &result,
        initial.operation
    )
    .is_err());
    assert!(RemoteResultCorrespondence::decode_bound(
        &"x".repeat(4_194_305),
        &input,
        &result,
        initial.operation
    )
    .is_err());
}

#[test]
fn decoder_refuses_two_results_claiming_the_same_original_identity() {
    let initial = snapshot();
    let input = manifest(&initial, RecordDigest::from_bytes([8; 32]));
    let mut saved = initial.clone();
    saved.operation = RecordDigest::from_bytes([9; 32]);
    let mut new = initial.files[0].clone();
    new.path = "new".into();
    new.object = mesh_materializer::ObjectId::from_bytes([4; 16]);
    saved.files.push(new);
    let result = manifest(&saved, saved.operation);
    let proof = RemoteResultCorrespondence::derive(&input, &initial, &result, &saved).unwrap();
    let Json::Object(mut fields) = Json::parse(proof.encoded()).unwrap() else {
        panic!("body");
    };
    let Json::Array(rows) = &mut fields.iter_mut().find(|(k, _)| k == "entries").unwrap().1 else {
        panic!("rows");
    };
    let Json::Object(row) = &mut rows[2] else {
        panic!("row");
    };
    row.iter_mut().find(|(k, _)| k == "input_path").unwrap().1 = Json::text("folder/file");
    assert!(RemoteResultCorrespondence::decode_bound(
        &Json::Object(fields).encode(),
        &input,
        &result,
        initial.operation
    )
    .is_err());
}

#[test]
fn original_mapping_requires_complete_exact_copies_and_keeps_replacement_distinct() {
    let worker = snapshot();
    let input = manifest(&worker, RecordDigest::from_bytes([8; 32]));
    let mut original = worker.clone();
    original.operation = input.input();
    original.directories[0].object = ObjectId::from_bytes([20; 16]);
    original.files[0].object = ObjectId::from_bytes([21; 16]);
    let mut saved = worker.clone();
    saved.operation = RecordDigest::from_bytes([9; 32]);
    saved.files[0].path = "renamed".into();
    let mut replacement = worker.files[0].clone();
    replacement.object = ObjectId::from_bytes([4; 16]);
    saved.files.push(replacement);
    let result = manifest(&saved, saved.operation);
    let proof = RemoteResultCorrespondence::derive(&input, &worker, &result, &saved).unwrap();
    let mut local = saved.clone();
    local.operation = RecordDigest::from_bytes([10; 32]);
    local.directories[0].object = ObjectId::from_bytes([30; 16]);
    local.files[0].object = ObjectId::from_bytes([31; 16]);
    local.files[1].object = ObjectId::from_bytes([32; 16]);
    let mapped = proof
        .project_origins(&input, &original, &result, &local)
        .unwrap();
    assert_eq!(mapped.len(), 2);
    assert_eq!(
        mapped.get(&local.files[0].object.to_string()),
        Some(&original.files[0].object.to_string())
    );
    assert!(!mapped.contains_key(&local.files[1].object.to_string()));
    for mutation in 0..6 {
        let mut wrong = local.clone();
        match mutation {
            0 => wrong.files[0].executable = !wrong.files[0].executable,
            1 => wrong.files[0].byte_length += 1,
            2 => wrong.files[0].content_digest = RecordDigest::from_bytes([99; 32]),
            3 => wrong.files[0].object = wrong.directories[0].object,
            4 => {
                wrong.directories.clear();
            }
            _ => {
                wrong.files.pop();
            }
        }
        assert!(proof
            .project_origins(&input, &original, &result, &wrong)
            .is_err());
    }
    original.operation = RecordDigest::from_bytes([11; 32]);
    assert!(proof
        .project_origins(&input, &original, &result, &local)
        .is_err());
}
