//! Exact root-lane input correspondence across independent workspace object identities.
//! This is review preparation evidence, never a patch, approval or write-back capability.
use crate::ipc::Json;
use crate::workspace::HistoricalWorkspacePreview;
use mesh_store::RecordDigest;
use std::collections::{BTreeMap, BTreeSet};
use std::io;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    object: String,
    path: String,
    file: Option<(RecordDigest, u64, bool)>,
}
impl Entry {
    fn same_content(&self, other: &Self) -> bool {
        self.path == other.path && self.file == other.file
    }
    fn json(&self) -> Json {
        Json::object([
            ("object", Json::text(&self.object)),
            ("path", Json::text(&self.path)),
            (
                "kind",
                Json::text(if self.file.is_some() {
                    "file"
                } else {
                    "folder"
                }),
            ),
            (
                "digest",
                self.file
                    .map_or(Json::Null, |(digest, _, _)| Json::text(digest.to_string())),
            ),
            (
                "bytes",
                self.file
                    .map_or(Json::Null, |(_, bytes, _)| Json::Number(bytes)),
            ),
            (
                "executable",
                self.file
                    .map_or(Json::Null, |(_, _, executable)| Json::Bool(executable)),
            ),
        ])
    }
}
fn refused() -> io::Error {
    io::Error::other("saved project input correspondence could not be verified")
}
fn entries(snapshot: HistoricalWorkspacePreview) -> io::Result<BTreeMap<String, Entry>> {
    let mut entries = BTreeMap::new();
    let mut paths = BTreeSet::new();
    for entry in snapshot
        .directories
        .into_iter()
        .map(|dir| Entry {
            object: dir.object.to_string(),
            path: dir.path,
            file: None,
        })
        .chain(snapshot.files.into_iter().map(|file| Entry {
            object: file.object.to_string(),
            path: file.path,
            file: Some((file.content_digest, file.byte_length, file.executable)),
        }))
    {
        if !paths.insert(entry.path.clone())
            || entries.insert(entry.object.clone(), entry).is_some()
        {
            return Err(refused());
        }
    }
    Ok(entries)
}

pub(super) fn mapping(
    source: HistoricalWorkspacePreview,
    input: HistoricalWorkspacePreview,
    result: HistoricalWorkspacePreview,
    after: Option<&str>,
) -> io::Result<Json> {
    let source = entries(source)?;
    let input = entries(input)?;
    let result = entries(result)?;
    let source_by_path: BTreeMap<_, _> = source.values().map(|e| (e.path.as_str(), e)).collect();
    // Only a complete equal immutable inventory proves the import boundary. Individual matching
    // filenames, current working files and a partial projection cannot establish correspondence.
    if source.len() != input.len()
        || input.values().any(|entry| {
            source_by_path
                .get(entry.path.as_str())
                .is_none_or(|original| !entry.same_content(original))
        })
    {
        return Err(refused());
    }
    let ids: BTreeSet<_> = input.keys().chain(result.keys()).collect();
    let changed: Vec<_> = ids
        .into_iter()
        .filter(|id| match (input.get(*id), result.get(*id)) {
            (Some(a), Some(b)) => !a.same_content(b),
            _ => true,
        })
        .collect();
    let start = match after {
        None => 0,
        Some(id) => {
            changed
                .iter()
                .position(|key| key.as_str() == id)
                .ok_or_else(refused)?
                + 1
        }
    };
    let end = (start + 200).min(changed.len());
    let changes = changed[start..end]
        .iter()
        .map(|id| {
            let original = input
                .get(*id)
                .and_then(|entry| source_by_path.get(entry.path.as_str()));
            Json::object([
                ("lane_object", Json::text(id.as_str())),
                ("source", original.map_or(Json::Null, |entry| entry.json())),
                ("result", result.get(*id).map_or(Json::Null, Entry::json)),
            ])
        })
        .collect();
    Ok(Json::object([
        ("order", Json::text("lane-object-id")),
        ("after", after.map_or(Json::Null, Json::text)),
        ("total", Json::Number(changed.len() as u64)),
        ("changes", Json::Array(changes)),
        (
            "next_after",
            if end < changed.len() {
                Json::text(changed[end - 1])
            } else {
                Json::Null
            },
        ),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::{HistoricalWorkspaceDirectory, HistoricalWorkspacePreviewFile};

    fn snapshot(object: u8) -> HistoricalWorkspacePreview {
        HistoricalWorkspacePreview {
            operation: RecordDigest::from_bytes([object; 32]),
            directories: vec![HistoricalWorkspaceDirectory {
                object: mesh_materializer::ObjectId::from_bytes([object; 16]),
                path: "folder".into(),
            }],
            files: vec![HistoricalWorkspacePreviewFile {
                object: mesh_materializer::ObjectId::from_bytes([object + 1; 16]),
                path: "folder/file".into(),
                manifest_id: RecordDigest::from_bytes([object; 32]),
                byte_length: 12,
                content_digest: RecordDigest::from_bytes([90; 32]),
                executable: false,
            }],
        }
    }

    #[test]
    fn maps_distinct_import_ids_and_tracks_moves_additions_and_deletions() {
        let source = snapshot(1);
        let input = snapshot(3);
        let mut result = input.clone();
        result.files[0].path = "renamed".into();
        result.directories.clear();
        let mut added = result.files[0].clone();
        added.object = mesh_materializer::ObjectId::from_bytes([5; 16]);
        added.path = "new-file".into();
        result.files.push(added);
        let mapped = mapping(source.clone(), input.clone(), result.clone(), None).unwrap();
        let rows = mapped.get("changes").unwrap().as_array().unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].get("result"), Some(&Json::Null));
        assert_eq!(
            rows[1].get("source").unwrap().get("object"),
            Some(&Json::text(source.files[0].object.to_string()))
        );
        assert_eq!(
            rows[1].get("result").unwrap().get("path"),
            Some(&Json::text("renamed"))
        );
        assert_eq!(rows[2].get("source"), Some(&Json::Null));
        assert!(mapping(source, input, result, Some("invented-cursor")).is_err());
    }

    #[test]
    fn complete_input_equality_is_required_including_types_and_metadata() {
        let source = snapshot(1);
        let input = snapshot(3);
        for kind in 0..6 {
            let mut changed = input.clone();
            match kind {
                0 => changed.files[0].content_digest = RecordDigest::from_bytes([0; 32]),
                1 => changed.files[0].byte_length += 1,
                2 => changed.files[0].executable = true,
                3 => {
                    changed.directories.clear();
                }
                4 => changed.files[0].path = "other".into(),
                _ => changed.files[0].path = "folder".into(),
            }
            assert!(mapping(source.clone(), changed, input.clone(), None).is_err());
        }
        assert_eq!(
            mapping(source, input.clone(), input, None)
                .unwrap()
                .get("total"),
            Some(&Json::Number(0))
        );
    }

    #[test]
    fn pages_every_changed_object_without_silently_truncating() {
        let source = snapshot(1);
        let input = snapshot(3);
        let mut result = input.clone();
        result.files.clear();
        for index in 1_u128..=205 {
            let mut file = input.files[0].clone();
            file.object = mesh_materializer::ObjectId::from_bytes(index.to_be_bytes());
            file.path = format!("added-{index}");
            result.files.push(file);
        }
        let page = mapping(source.clone(), input.clone(), result.clone(), None).unwrap();
        assert_eq!(page.get("total"), Some(&Json::Number(206)));
        assert_eq!(page.get("changes").unwrap().as_array().unwrap().len(), 200);
        let next = page.get("next_after").unwrap().as_text().unwrap();
        let tail = mapping(source, input, result, Some(next)).unwrap();
        assert_eq!(tail.get("changes").unwrap().as_array().unwrap().len(), 6);
        assert_eq!(tail.get("next_after"), Some(&Json::Null));
    }

    #[test]
    fn replacement_at_the_same_path_is_not_the_original_object() {
        let source = snapshot(1);
        let input = snapshot(3);
        let mut result = input.clone();
        result.files[0].object = mesh_materializer::ObjectId::from_bytes([5; 16]);
        let mapped = mapping(source.clone(), input.clone(), result.clone(), None).unwrap();
        let rows = mapped.get("changes").unwrap().as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0].get("lane_object"),
            Some(&Json::text(input.files[0].object.to_string()))
        );
        assert_eq!(
            rows[0].get("source").unwrap().get("object"),
            Some(&Json::text(source.files[0].object.to_string()))
        );
        assert_eq!(rows[0].get("result"), Some(&Json::Null));
        assert_eq!(
            rows[1].get("lane_object"),
            Some(&Json::text(result.files[0].object.to_string()))
        );
        assert_eq!(rows[1].get("source"), Some(&Json::Null));
        assert_eq!(
            rows[1].get("result").unwrap().get("path"),
            Some(&Json::text("folder/file"))
        );
    }

    #[test]
    fn ambiguous_identity_or_path_refuses_in_every_inventory() {
        let source = snapshot(1);
        let input = snapshot(3);
        for index in 0..3 {
            for duplicate_identity in [false, true] {
                let mut inventories = [source.clone(), input.clone(), input.clone()];
                let snapshot = &mut inventories[index];
                if duplicate_identity {
                    snapshot.files[0].object = snapshot.directories[0].object;
                } else {
                    snapshot.files[0].path = snapshot.directories[0].path.clone();
                }
                let [source, input, result] = inventories;
                assert!(mapping(source, input, result, None).is_err());
            }
        }
    }
}
