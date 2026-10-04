//! Read-only saved-version inspection through retained native attachment authority.

use super::{invalid, ProjectAttachment};
use crate::ipc::Json;
use crate::root_authority::PinnedWorkspaceRoot;
use crate::workspace::OpenWorkspace;
use mesh_store::RecordDigest;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

const PAGE_SIZE: usize = 200;
const TEXT_LIMIT: u64 = 262_144;
fn error(problem: impl std::fmt::Display) -> io::Error {
    io::Error::other(problem.to_string())
}
impl ProjectAttachment {
    /// Existing native consumption paths keep their legacy fence until exact dependency grants
    /// and materialization bindings are integrated. Read-only visibility never authorizes a fork.
    pub(super) fn inspect_consumption_input<T>(
        &self,
        metadata: &Path,
        store: PinnedWorkspaceRoot,
        operation: &str,
        consume: impl FnOnce(&OpenWorkspace, RecordDigest) -> io::Result<T>,
    ) -> io::Result<T> {
        let digest = RecordDigest::parse_hex(operation).map_err(error)?;
        if digest.to_string() != operation {
            return Err(invalid("noncanonical version identity"));
        }
        self.with_review_history(
            metadata,
            store,
            &crate::TrustedReviewers::default(),
            |workspace, _| {
                if !workspace
                    .workspace_versions()
                    .iter()
                    .any(|version| version.operation() == digest)
                {
                    return Err(invalid("version does not belong to attachment history"));
                }
                consume(workspace, digest)
            },
        )
    }

    pub(super) fn inspect_saved<T>(
        &self,
        metadata: &Path,
        store: PinnedWorkspaceRoot,
        operation: &str,
        read: impl FnOnce(&OpenWorkspace, RecordDigest) -> io::Result<T>,
    ) -> io::Result<T> {
        let digest = RecordDigest::parse_hex(operation).map_err(error)?;
        if digest.to_string() != operation {
            return Err(invalid("noncanonical version identity"));
        }
        self.with_read_history(
            metadata,
            store,
            &crate::TrustedReviewers::default(),
            |workspace, _, _| {
                if !workspace
                    .workspace_versions()
                    .iter()
                    .any(|version| version.operation() == digest)
                {
                    return Err(invalid("version does not belong to attachment history"));
                }
                read(workspace, digest)
            },
        )
    }

    pub(super) fn compare_saved(
        &self,
        metadata: &Path,
        store: PinnedWorkspaceRoot,
        base: &str,
        target: &str,
        selection: (Option<&str>, Option<&str>),
    ) -> io::Result<Json> {
        let (after, selected_path) = selection;
        self.inspect_saved(metadata, store, base, |workspace, base_digest| {
            let target_digest = RecordDigest::parse_hex(target).map_err(error)?;
            if target_digest.to_string() != target
                || !workspace
                    .workspace_versions()
                    .iter()
                    .any(|version| version.operation() == target_digest)
            {
                return Err(invalid(
                    "comparison target does not belong to attachment history",
                ));
            }
            let before = comparison_entries(
                workspace
                    .historical_workspace_preview(base_digest)
                    .map_err(error)?,
            );
            let after_entries = comparison_entries(
                workspace
                    .historical_workspace_preview(target_digest)
                    .map_err(error)?,
            );
            let paths: BTreeSet<_> = before.keys().chain(after_entries.keys()).collect();
            let changes: Vec<_> = paths
                .into_iter()
                .filter_map(|path| {
                    let old = before.get(path);
                    let new = after_entries.get(path);
                    if old == new {
                        return None;
                    }
                    let kind = match (old, new) {
                        (None, Some(_)) => "added",
                        (Some(_), None) => "removed",
                        (Some(old), Some(new)) if old.kind != new.kind => "type-changed",
                        (Some(old), Some(new))
                            if old.digest == new.digest && old.bytes == new.bytes =>
                        {
                            "mode-changed"
                        }
                        _ => "modified",
                    };
                    Some((
                        path,
                        Json::object([
                            ("path", Json::text(path)),
                            ("change", Json::text(kind)),
                            ("before", old.map_or(Json::Null, ComparisonEntry::json)),
                            ("after", new.map_or(Json::Null, ComparisonEntry::json)),
                        ]),
                    ))
                })
                .collect();
            if let Some(path) = selected_path {
                let change = changes
                    .iter()
                    .find(|change| change.0 == path)
                    .ok_or_else(|| {
                        invalid("selected path does not change between these versions")
                    })?;
                return Ok(Json::object([
                    ("schema", Json::text("mesh.attachment-comparison/v1")),
                    ("base", Json::text(base)),
                    ("target", Json::text(target)),
                    ("after", Json::Null),
                    ("total", Json::Number(1)),
                    ("changes", Json::Array(vec![change.1.clone()])),
                    ("next_after", Json::Null),
                ]));
            }
            let start = match after {
                None => 0,
                Some(cursor) => {
                    changes
                        .iter()
                        .position(|entry| entry.0 == cursor)
                        .ok_or_else(|| {
                            invalid("comparison cursor does not identify a changed path")
                        })?
                        + 1
                }
            };
            let end = (start + PAGE_SIZE).min(changes.len());
            Ok(Json::object([
                ("schema", Json::text("mesh.attachment-comparison/v1")),
                ("base", Json::text(base)),
                ("target", Json::text(target)),
                ("after", after.map_or(Json::Null, Json::text)),
                ("total", Json::Number(changes.len() as u64)),
                (
                    "changes",
                    Json::Array(
                        changes[start..end]
                            .iter()
                            .map(|entry| entry.1.clone())
                            .collect(),
                    ),
                ),
                (
                    "next_after",
                    if end < changes.len() {
                        Json::text(changes[end - 1].0)
                    } else {
                        Json::Null
                    },
                ),
            ]))
        })
    }

    pub(super) fn inspect_entries(
        &self,
        metadata: &Path,
        store: PinnedWorkspaceRoot,
        operation: &str,
        after: Option<&str>,
    ) -> io::Result<Json> {
        self.inspect_saved(metadata, store, operation, |workspace, digest| {
            let preview = workspace
                .historical_workspace_preview(digest)
                .map_err(error)?;
            let mut entries: Vec<_> = preview
                .directories
                .iter()
                .map(|entry| (entry.path.clone(), "folder", None))
                .chain(
                    preview
                        .files
                        .iter()
                        .map(|entry| (entry.path.clone(), "file", Some(entry))),
                )
                .collect();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            let start = match after {
                None => 0,
                Some(cursor) => {
                    entries
                        .iter()
                        .position(|entry| entry.0 == cursor)
                        .ok_or_else(|| invalid("entry cursor does not belong to this version"))?
                        + 1
                }
            };
            let end = (start + PAGE_SIZE).min(entries.len());
            let page = entries[start..end]
                .iter()
                .map(|(path, kind, file)| {
                    Json::object([
                        ("path", Json::text(path)),
                        ("kind", Json::text(*kind)),
                        (
                            "bytes",
                            file.map_or(Json::Null, |file| Json::Number(file.byte_length)),
                        ),
                        (
                            "digest",
                            file.map_or(Json::Null, |file| {
                                Json::text(file.content_digest.to_string())
                            }),
                        ),
                        (
                            "executable",
                            file.map_or(Json::Null, |file| Json::Bool(file.executable)),
                        ),
                    ])
                })
                .collect();
            Ok(Json::object([
                ("schema", Json::text("mesh.attachment-entries/v1")),
                ("operation", Json::text(operation)),
                ("after", after.map_or(Json::Null, Json::text)),
                ("entries", Json::Array(page)),
                (
                    "next_after",
                    if end < entries.len() {
                        Json::text(&entries[end - 1].0)
                    } else {
                        Json::Null
                    },
                ),
            ]))
        })
    }

    pub(super) fn inspect_text(
        &self,
        metadata: &Path,
        store: PinnedWorkspaceRoot,
        operation: &str,
        path: &str,
    ) -> io::Result<Json> {
        self.inspect_saved(metadata, store, operation, |workspace, digest| {
            let preview = workspace
                .historical_workspace_preview(digest)
                .map_err(error)?;
            let entry = preview
                .files
                .iter()
                .find(|entry| entry.path == path)
                .ok_or_else(|| invalid("file does not belong to this saved version"))?;
            let (state, text) = if entry.byte_length > TEXT_LIMIT {
                ("too-large", Json::Null)
            } else {
                let file = workspace
                    .historical_workspace_file(digest, path)
                    .map_err(error)?
                    .ok_or_else(|| invalid("saved file unavailable"))?;
                match String::from_utf8(file.bytes) {
                    Ok(text) if !text.contains('\0') => ("text", Json::text(text)),
                    _ => ("binary", Json::Null),
                }
            };
            Ok(Json::object([
                ("schema", Json::text("mesh.attachment-text/v1")),
                ("operation", Json::text(operation)),
                ("path", Json::text(path)),
                ("digest", Json::text(entry.content_digest.to_string())),
                ("bytes", Json::Number(entry.byte_length)),
                ("executable", Json::Bool(entry.executable)),
                ("state", Json::text(state)),
                ("text", text),
            ]))
        })
    }
}

#[derive(PartialEq, Eq)]
pub(super) struct ComparisonEntry {
    pub(super) kind: &'static str,
    pub(super) bytes: Option<u64>,
    pub(super) digest: Option<RecordDigest>,
    pub(super) executable: Option<bool>,
}
impl ComparisonEntry {
    pub(super) fn json(&self) -> Json {
        Json::object([
            ("kind", Json::text(self.kind)),
            ("bytes", self.bytes.map_or(Json::Null, Json::Number)),
            (
                "digest",
                self.digest
                    .map_or(Json::Null, |digest| Json::text(digest.to_string())),
            ),
            ("executable", self.executable.map_or(Json::Null, Json::Bool)),
        ])
    }
}
pub(super) fn comparison_entries(
    preview: crate::workspace::HistoricalWorkspacePreview,
) -> BTreeMap<String, ComparisonEntry> {
    preview
        .directories
        .into_iter()
        .map(|entry| {
            (
                entry.path,
                ComparisonEntry {
                    kind: "folder",
                    bytes: None,
                    digest: None,
                    executable: None,
                },
            )
        })
        .chain(preview.files.into_iter().map(|entry| {
            (
                entry.path,
                ComparisonEntry {
                    kind: "file",
                    bytes: Some(entry.byte_length),
                    digest: Some(entry.content_digest),
                    executable: Some(entry.executable),
                },
            )
        }))
        .collect()
}
