//! Read-only saved-version inspection through retained native attachment authority.

use super::{history::verify_history_binding, invalid, read_receipt, ProjectAttachment};
use crate::ipc::Json;
use crate::root_authority::PinnedWorkspaceRoot;
use crate::workspace::OpenWorkspace;
use mesh_store::RecordDigest;
use std::io;
use std::path::Path;

const PAGE_SIZE: usize = 200;
const TEXT_LIMIT: u64 = 262_144;
fn error(problem: impl std::fmt::Display) -> io::Error {
    io::Error::other(problem.to_string())
}
impl ProjectAttachment {
    fn inspect_saved<T>(
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
        self.ensure_current()?;
        store.ensure_namespace_identity()?;
        if read_receipt(&store)? != self.receipt()?.encode() {
            return Err(invalid("attachment receipt changed"));
        }
        let _guard =
            crate::workspace_custody::lock_workspace_initialization(&store).map_err(error)?;
        let (configuration, _) = self.history_configuration(&store, None)?;
        let workspace =
            OpenWorkspace::open_attachment_store(metadata, store.clone(), false).map_err(error)?;
        verify_history_binding(&workspace, &configuration)?;
        if !workspace
            .workspace_versions()
            .iter()
            .any(|version| version.operation() == digest)
        {
            return Err(invalid("version does not belong to attachment history"));
        }
        let result = read(&workspace, digest)?;
        store.ensure_namespace_identity()?;
        self.ensure_current()?;
        Ok(result)
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
