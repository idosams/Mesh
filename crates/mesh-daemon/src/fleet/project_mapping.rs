//! Exact transitive input correspondence across independent workspace object identities.
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct LineageStep {
    pub lane: String,
    pub binding: super::WorkspaceBinding,
    pub result: RecordDigest,
}

pub(super) fn lineage(
    state: &super::State,
    selected: &str,
    version: RecordDigest,
    project: &str,
) -> io::Result<Vec<LineageStep>> {
    let limit = state.limits.as_ref().ok_or_else(refused)?.depth.min(32) + 1;
    let mut steps = Vec::new();
    let mut seen = BTreeSet::new();
    let mut id = selected;
    let mut result = version;
    loop {
        if !seen.insert(id) || steps.len() as u64 >= limit {
            return Err(refused());
        }
        let lane = state.lanes.get(id).ok_or_else(refused)?;
        let binding = lane.workspace.as_ref().ok_or_else(refused)?;
        if lane.id != id
            || lane.source_project.as_deref() != Some(project)
            || lane.base != binding.source_version
            || binding.starting_version().is_none()
        {
            return Err(refused());
        }
        steps.push(LineageStep {
            lane: id.into(),
            binding: binding.clone(),
            result,
        });
        match lane.parent.as_deref() {
            Some(parent) => {
                id = parent;
                result = binding.source_version;
            }
            None => break,
        }
    }
    steps.reverse();
    if steps
        .iter()
        .enumerate()
        .any(|(depth, step)| state.lanes[&step.lane].depth != depth as u64)
    {
        return Err(refused());
    }
    Ok(steps)
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

/// Walk each immutable import boundary and retain original identity through local edits.
/// Earlier deletions and additions remain part of the final source-relative result.
pub(super) fn mapping_chain(
    source: HistoricalWorkspacePreview,
    steps: Vec<(HistoricalWorkspacePreview, HistoricalWorkspacePreview)>,
    after: Option<&str>,
) -> io::Result<Json> {
    if steps.is_empty() || steps.len() > 33 {
        return Err(refused());
    }
    let source = entries(source)?;
    let mut current = source.clone();
    let mut origins: BTreeMap<String, String> =
        source.keys().map(|id| (id.clone(), id.clone())).collect();
    for (input, result) in steps {
        let input = entries(input)?;
        let result = entries(result)?;
        let by_path: BTreeMap<_, _> = current.values().map(|e| (e.path.as_str(), e)).collect();
        // Every import must match the complete upstream saved version, including empty folders.
        if current.len() != input.len()
            || input.values().any(|entry| {
                by_path
                    .get(entry.path.as_str())
                    .is_none_or(|upstream| !entry.same_content(upstream))
            })
        {
            return Err(refused());
        }
        origins = input
            .values()
            .filter_map(|entry| {
                let upstream = by_path.get(entry.path.as_str())?;
                let original = origins.get(&upstream.object)?;
                result
                    .contains_key(&entry.object)
                    .then(|| (entry.object.clone(), original.clone()))
            })
            .collect();
        current = result;
    }
    let reverse: BTreeMap<_, _> = origins
        .iter()
        .map(|(local, original)| (original, local))
        .collect();
    if reverse.len() != origins.len() {
        return Err(refused());
    }
    let mut changed = BTreeMap::new();
    for (id, original) in &source {
        let result = reverse.get(id).and_then(|local| current.get(*local));
        if result.is_some_and(|result| original.same_content(result)) {
            continue;
        }
        changed.insert(format!("source:{id}"), (Some(original), result));
    }
    for (id, result) in &current {
        if !origins.contains_key(id) {
            changed.insert(format!("result:{id}"), (None, Some(result)));
        }
    }
    let changes: Vec<_> = changed.into_iter().collect();
    let start = match after {
        None => 0,
        Some(id) => {
            changes
                .iter()
                .position(|(key, _)| key == id)
                .ok_or_else(refused)?
                + 1
        }
    };
    let end = (start + 200).min(changes.len());
    let rows = changes[start..end]
        .iter()
        .map(|(id, (original, result))| {
            Json::object([
                ("id", Json::text(id)),
                ("source", original.map_or(Json::Null, Entry::json)),
                ("result", result.map_or(Json::Null, Entry::json)),
            ])
        })
        .collect();
    Ok(Json::object([
        ("order", Json::text("correspondence-id")),
        ("after", after.map_or(Json::Null, Json::text)),
        ("total", Json::Number(changes.len() as u64)),
        ("changes", Json::Array(rows)),
        (
            "next_after",
            if end < changes.len() {
                Json::text(&changes[end - 1].0)
            } else {
                Json::Null
            },
        ),
    ]))
}

#[cfg(test)]
fn mapping(
    source: HistoricalWorkspacePreview,
    input: HistoricalWorkspacePreview,
    result: HistoricalWorkspacePreview,
    after: Option<&str>,
) -> io::Result<Json> {
    mapping_chain(source, vec![(input, result)], after)
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
        assert_eq!(rows[1].get("result"), Some(&Json::Null));
        assert_eq!(
            rows[2].get("source").unwrap().get("object"),
            Some(&Json::text(source.files[0].object.to_string()))
        );
        assert_eq!(
            rows[2].get("result").unwrap().get("path"),
            Some(&Json::text("renamed"))
        );
        assert_eq!(rows[0].get("source"), Some(&Json::Null));
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
        let mapped = mapping(source.clone(), input, result.clone(), None).unwrap();
        let rows = mapped.get("changes").unwrap().as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0].get("id"),
            Some(&Json::text(format!("result:{}", result.files[0].object)))
        );
        assert_eq!(rows[0].get("source"), Some(&Json::Null));
        assert_eq!(
            rows[0].get("result").unwrap().get("object"),
            Some(&Json::text(result.files[0].object.to_string()))
        );
        assert_eq!(
            rows[0].get("result").unwrap().get("path"),
            Some(&Json::text("folder/file"))
        );
        assert_eq!(
            rows[1].get("id"),
            Some(&Json::text(format!("source:{}", source.files[0].object)))
        );
        assert_eq!(
            rows[1].get("source").unwrap().get("object"),
            Some(&Json::text(source.files[0].object.to_string()))
        );
        assert_eq!(
            rows[1].get("source").unwrap().get("path"),
            Some(&Json::text("folder/file"))
        );
        assert_eq!(rows[1].get("result"), Some(&Json::Null));
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

    fn reidentify(
        mut snapshot: HistoricalWorkspacePreview,
        offset: u8,
    ) -> HistoricalWorkspacePreview {
        for (index, directory) in snapshot.directories.iter_mut().enumerate() {
            directory.object = mesh_materializer::ObjectId::from_bytes([offset + index as u8; 16]);
        }
        for (index, file) in snapshot.files.iter_mut().enumerate() {
            file.object = mesh_materializer::ObjectId::from_bytes([offset + 10 + index as u8; 16]);
        }
        snapshot
    }

    #[test]
    fn transitive_mapping_includes_upstream_moves_additions_and_deletions() {
        let source = snapshot(1);
        let root = reidentify(source.clone(), 20);
        let mut upstream = root.clone();
        upstream.directories.clear();
        upstream.files[0].path = "moved".into();
        let mut added = upstream.files[0].clone();
        added.object = mesh_materializer::ObjectId::from_bytes([90; 16]);
        added.path = "upstream-addition".into();
        upstream.files.push(added);
        let child = reidentify(upstream.clone(), 40);
        let grandchild = reidentify(child.clone(), 60);
        let mut final_result = grandchild.clone();
        final_result.files[0].content_digest = RecordDigest::from_bytes([77; 32]);
        let steps = vec![
            (root.clone(), upstream.clone()),
            (child.clone(), child.clone()),
            (grandchild.clone(), final_result.clone()),
        ];
        let mapped = mapping_chain(source.clone(), steps.clone(), None).unwrap();
        let rows = mapped.get("changes").unwrap().as_array().unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].get("source"), Some(&Json::Null));
        assert_eq!(
            rows[0].get("result").unwrap().get("path"),
            Some(&Json::text("upstream-addition"))
        );
        assert_eq!(
            rows[1].get("result"),
            Some(&Json::Null),
            "an ancestor deletion cannot disappear at a child import"
        );
        assert_eq!(
            rows[2].get("source").unwrap().get("object"),
            Some(&Json::text(source.files[0].object.to_string()))
        );
        assert_eq!(
            rows[2].get("result").unwrap().get("object"),
            Some(&Json::text(final_result.files[0].object.to_string()))
        );
        let mut bad = steps;
        bad[1].0.files[0].executable = true;
        assert!(mapping_chain(source.clone(), bad, None).is_err());
        assert!(mapping_chain(source.clone(), Vec::new(), None).is_err());
        assert!(mapping_chain(source, vec![(child.clone(), child); 34], None).is_err());
    }

    #[test]
    fn deletion_and_recreation_at_the_same_path_do_not_invent_shared_identity() {
        let source = snapshot(1);
        let input = reidentify(source.clone(), 20);
        let mut upstream = input.clone();
        upstream.files.clear();
        let child = reidentify(upstream.clone(), 40);
        let mut result = child.clone();
        result.files = reidentify(source.clone(), 60).files;
        let mapped = mapping_chain(source, vec![(input, upstream), (child, result)], None).unwrap();
        let rows = mapped.get("changes").unwrap().as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get("source"), Some(&Json::Null));
        assert_eq!(rows[1].get("result"), Some(&Json::Null));
    }

    #[test]
    fn lineage_refuses_cycles_missing_or_foreign_inputs_and_unbound_legacy_history() {
        use super::super::{Lane, Limits, State, WorkspaceBinding};
        let mut state = State {
            limits: Some(Limits {
                lanes: 4,
                concurrency: 2,
                depth: 2,
                retries: 0,
            }),
            ..State::default()
        };
        for (id, parent, depth, byte) in [("root", None, 0, 1), ("child", Some("root"), 1, 3)] {
            let base = RecordDigest::from_bytes([byte; 32]);
            state.lanes.insert(
                id.into(),
                Lane {
                    id: id.into(),
                    parent: parent.map(str::to_owned),
                    created_by: None,
                    source_project: Some("project".into()),
                    goal: "work".into(),
                    provider: "provider".into(),
                    base,
                    depth,
                    workspace: Some(WorkspaceBinding {
                        source_version: base,
                        starting_version: Some(RecordDigest::from_bytes([byte + 1; 32])),
                        root: id.into(),
                        digest: "digest".into(),
                        installation: "installation".into(),
                    }),
                    runs: Vec::new(),
                    saved: None,
                },
            );
        }
        let version = RecordDigest::from_bytes([5; 32]);
        let steps = lineage(&state, "child", version, "project").unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].lane, "root");
        assert_eq!(steps[0].result, state.lanes["child"].base);
        assert_eq!(steps[1].result, version);
        for mutation in 0..7 {
            let mut changed = state.clone();
            let root = changed.lanes.get_mut("root").unwrap();
            match mutation {
                0 => root.parent = Some("child".into()),
                1 => root.source_project = Some("foreign".into()),
                2 => root.workspace = None,
                3 => root.workspace.as_mut().unwrap().starting_version = None,
                4 => root.base = version,
                5 => root.depth = 1,
                _ => {
                    changed.lanes.remove("root");
                }
            }
            assert!(lineage(&changed, "child", version, "project").is_err());
        }
        state.limits.as_mut().unwrap().depth = 0;
        assert!(lineage(&state, "child", version, "project").is_err());
    }

    #[test]
    fn transitive_reversions_and_removed_private_additions_do_not_invent_net_changes() {
        let source = snapshot(1);
        let root = reidentify(source.clone(), 20);
        let mut upstream = root.clone();
        upstream.files[0].path = "renamed".into();
        upstream.files[0].content_digest = RecordDigest::from_bytes([77; 32]);
        let mut added = upstream.files[0].clone();
        added.object = mesh_materializer::ObjectId::from_bytes([90; 16]);
        added.path = "temporary-addition".into();
        upstream.files.push(added);
        let child = reidentify(upstream.clone(), 40);
        let mut result = child.clone();
        result.files.truncate(1);
        result.files[0].path = source.files[0].path.clone();
        result.files[0].content_digest = source.files[0].content_digest;
        let restored = mapping_chain(
            source.clone(),
            vec![
                (root.clone(), upstream.clone()),
                (child.clone(), result.clone()),
            ],
            None,
        )
        .unwrap();
        assert_eq!(restored.get("total"), Some(&Json::Number(0)));
        assert_eq!(restored.get("changes"), Some(&Json::Array(vec![])));
        result.files[0].executable = true;
        let changed = mapping_chain(
            source.clone(),
            vec![(root, upstream), (child, result.clone())],
            None,
        )
        .unwrap();
        let rows = changed.get("changes").unwrap().as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].get("source").unwrap().get("object"),
            Some(&Json::text(source.files[0].object.to_string()))
        );
        assert_eq!(
            rows[0].get("result").unwrap().get("object"),
            Some(&Json::text(result.files[0].object.to_string()))
        );
        assert_eq!(
            rows[0].get("source").unwrap().get("executable"),
            Some(&Json::Bool(false))
        );
        assert_eq!(
            rows[0].get("result").unwrap().get("executable"),
            Some(&Json::Bool(true))
        );
    }
}
