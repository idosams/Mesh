//! Immutable input-relative lane comparisons. No live-file reads or publication authority.
use crate::ipc::{Json, Unavailable};
use crate::workspace::{HistoricalWorkspacePreview, HistoricalWorkspacePreviewFile, OpenWorkspace};
use mesh_store::RecordDigest;
use mesh_types::{Blake3, ContentDigest};
use std::collections::{BTreeMap, BTreeSet};

const PAGE_SIZE: usize = 200;
const TEXT_LIMIT: u64 = 262_144;
fn refused() -> Unavailable {
    Unavailable::new(
        "fleet-starting-comparison-unavailable",
        "The exact lane input comparison could not be verified.",
    )
}
#[derive(PartialEq, Eq)]
struct Entry {
    path: String,
    file: Option<HistoricalWorkspacePreviewFile>,
}
impl Entry {
    fn json(
        &self,
        open: &OpenWorkspace,
        operation: RecordDigest,
        content: bool,
    ) -> Result<Json, Unavailable> {
        let (state, text) = match &self.file {
            None => ("folder", Json::Null),
            Some(_) if !content => ("not-requested", Json::Null),
            Some(file) if file.byte_length > TEXT_LIMIT => ("too-large", Json::Null),
            Some(file) => {
                let body = open
                    .historical_workspace_file(operation, &self.path)
                    .map_err(|_| refused())?
                    .ok_or_else(refused)?;
                if RecordDigest::from_bytes(*Blake3::digest_bytes(&body.bytes).as_bytes())
                    != file.content_digest
                    || body.object != file.object
                    || body.executable != file.executable
                    || body.bytes.len() as u64 != file.byte_length
                {
                    return Err(refused());
                }
                match String::from_utf8(body.bytes) {
                    Ok(text) if !text.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t') || matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')) => ("text", Json::text(text)),
                    _ => ("binary-or-unsafe-text", Json::Null),
                }
            }
        };
        Ok(Json::object([
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
                    .as_ref()
                    .map_or(Json::Null, |f| Json::text(f.content_digest.to_string())),
            ),
            (
                "bytes",
                self.file
                    .as_ref()
                    .map_or(Json::Null, |f| Json::Number(f.byte_length)),
            ),
            (
                "executable",
                self.file
                    .as_ref()
                    .map_or(Json::Null, |f| Json::Bool(f.executable)),
            ),
            ("content_state", Json::text(state)),
            ("text", text),
        ]))
    }
    fn same_content(&self, other: &Self) -> bool {
        self.path == other.path
            && match (&self.file, &other.file) {
                (None, None) => true,
                (Some(a), Some(b)) => {
                    a.content_digest == b.content_digest
                        && a.byte_length == b.byte_length
                        && a.executable == b.executable
                }
                _ => false,
            }
    }
}
fn entries(preview: HistoricalWorkspacePreview) -> BTreeMap<String, Entry> {
    preview
        .directories
        .into_iter()
        .map(|dir| {
            (
                dir.object.to_string(),
                Entry {
                    path: dir.path,
                    file: None,
                },
            )
        })
        .chain(preview.files.into_iter().map(|file| {
            (
                file.object.to_string(),
                Entry {
                    path: file.path.clone(),
                    file: Some(file),
                },
            )
        }))
        .collect()
}

pub(crate) fn compare(
    open: &OpenWorkspace,
    base: RecordDigest,
    target: RecordDigest,
    after: Option<&str>,
    selected: Option<&str>,
) -> Result<Json, Unavailable> {
    if after.is_some() && selected.is_some() {
        return Err(refused());
    }
    for id in [after, selected].into_iter().flatten() {
        if id.len() != 32
            || !id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(refused());
        }
    }
    let before = entries(
        open.historical_workspace_preview(base)
            .map_err(|_| refused())?,
    );
    let later = entries(
        open.historical_workspace_preview(target)
            .map_err(|_| refused())?,
    );
    let ids: BTreeSet<_> = before.keys().chain(later.keys()).collect();
    let changed: Vec<_> = ids
        .into_iter()
        .filter(|id| match (before.get(*id), later.get(*id)) {
            (Some(a), Some(b)) => !a.same_content(b),
            _ => true,
        })
        .collect();
    let start = match after.or(selected) {
        Some(id) => {
            changed
                .iter()
                .position(|value| value.as_str() == id)
                .ok_or_else(refused)?
                + usize::from(after.is_some())
        }
        None => 0,
    };
    let end = (start + if selected.is_some() { 1 } else { PAGE_SIZE }).min(changed.len());
    let changes = changed[start..end]
        .iter()
        .map(|id| {
            let a = before.get(*id);
            let b = later.get(*id);
            let effect = match (a, b) {
                (None, Some(_)) => "added",
                (Some(_), None) => "removed",
                (Some(a), Some(b)) if a.path != b.path => "moved-or-modified",
                _ => "modified",
            };
            Ok(Json::object([
                ("object", Json::text(id.as_str())),
                ("effect", Json::text(effect)),
                (
                    "before",
                    a.map(|entry| entry.json(open, base, selected.is_some()))
                        .transpose()?
                        .unwrap_or(Json::Null),
                ),
                (
                    "after",
                    b.map(|entry| entry.json(open, target, selected.is_some()))
                        .transpose()?
                        .unwrap_or(Json::Null),
                ),
            ]))
        })
        .collect::<Result<Vec<_>, Unavailable>>()?;
    Ok(Json::object([
        ("base", Json::text(base.to_string())),
        ("target", Json::text(target.to_string())),
        ("order", Json::text("object-id")),
        ("after", after.map_or(Json::Null, Json::text)),
        ("selected", selected.map_or(Json::Null, Json::text)),
        ("total", Json::Number(changed.len() as u64)),
        ("changes", Json::Array(changes)),
        (
            "next_after",
            if selected.is_none() && end < changed.len() {
                Json::text(changed[end - 1])
            } else {
                Json::Null
            },
        ),
        ("approval_authority", Json::Bool(false)),
    ]))
}
