//! Whole-project candidate comparison against a verified historical main, without approval power.
use super::inspection::{comparison_entries, ComparisonEntry};
use super::{invalid, ProvisionedAttachment};
use crate::ipc::Json;
use crate::workspace::{HistoricalWorkspacePreview, OpenWorkspace};
use crate::TrustedReviewers;
use mesh_store::RecordDigest;
use mesh_types::{Blake3, ContentDigest as _};
use std::collections::BTreeSet;
use std::io;

const PAGE: usize = 200;
const TEXT_LIMIT: u64 = 262_144;
fn error(value: impl std::fmt::Display) -> io::Error {
    io::Error::other(value.to_string())
}
fn field<'a>(value: &'a Json, key: &str) -> io::Result<&'a Json> {
    value
        .get(key)
        .ok_or_else(|| invalid("missing candidate review identity"))
}
fn text<'a>(value: &'a Json, key: &str) -> io::Result<&'a str> {
    field(value, key)?
        .as_text()
        .ok_or_else(|| invalid("invalid candidate review identity"))
}
fn digest(value: &str) -> io::Result<RecordDigest> {
    let parsed = RecordDigest::parse_hex(value).map_err(error)?;
    if parsed.to_string() != value {
        return Err(invalid("noncanonical candidate review identity"));
    }
    Ok(parsed)
}
fn side(
    open: &OpenWorkspace,
    version: Option<RecordDigest>,
    entry: Option<&ComparisonEntry>,
    path: &str,
    selected: bool,
) -> io::Result<Json> {
    let Some(entry) = entry else {
        return Ok(Json::Null);
    };
    let (state, content) = if entry.kind == "folder" {
        ("folder", Json::Null)
    } else if !selected {
        ("not-requested", Json::Null)
    } else if entry.bytes.is_none_or(|bytes| bytes > TEXT_LIMIT) {
        ("too-large", Json::Null)
    } else {
        let file = open
            .historical_workspace_file(
                version.ok_or_else(|| invalid("missing review side version"))?,
                path,
            )
            .map_err(error)?
            .ok_or_else(|| invalid("candidate review file unavailable"))?;
        if entry.bytes != Some(file.bytes.len() as u64)
            || entry.executable != Some(file.executable)
            || entry.digest
                != Some(RecordDigest::from_bytes(
                    *Blake3::digest_bytes(&file.bytes).as_bytes(),
                ))
        {
            return Err(invalid("candidate review content changed"));
        }
        match String::from_utf8(file.bytes) {
            Ok(value) if !value.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t') || matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')) => ("text", Json::text(value)),
            _ => ("binary-or-unsafe-text", Json::Null),
        }
    };
    Ok(Json::object([
        ("entry", entry.json()),
        ("content_state", Json::text(state)),
        ("text", content),
    ]))
}

impl ProvisionedAttachment {
    pub(crate) fn review_fleet_candidate(
        &self,
        candidate: &Json,
        target: &OpenWorkspace,
        snapshot: HistoricalWorkspacePreview,
        trusted: &TrustedReviewers,
        page: (Option<&str>, Option<&str>),
    ) -> io::Result<Json> {
        let (after, selected) = page;
        if after.is_some() && selected.is_some() {
            return Err(invalid("choose a page or one changed path"));
        }
        let provenance = field(candidate, "provenance")?;
        let source_version = digest(text(provenance, "source_version")?)?;
        if text(candidate, "project")? != self.id()
            || text(provenance, "source_project")? != self.id()
            || text(candidate, "content_digest")? != super::candidates::content_digest(&snapshot)
            || text(field(provenance, "selection")?, "version")? != snapshot.operation.to_string()
        {
            return Err(invalid("candidate review selection changed"));
        }
        let expected_main = field(provenance, "expected_main")?;
        let target_version = snapshot.operation;
        self.with_fleet_input(source_version, trusted, |project, current_main| {
            let (base_version, before) = match expected_main {
                Json::Null => (None, Default::default()),
                Json::Text(head) => {
                    let head = mesh_approval::HeadId::from_bytes(*digest(head)?.as_bytes());
                    if !project.has_verified_shared_head(head) {
                        return Err(invalid(
                            "candidate base is not verified project main history",
                        ));
                    }
                    let version = project.review_target_for_head(head).map_err(error)?;
                    let snapshot = project
                        .historical_workspace_preview(version)
                        .map_err(error)?;
                    (Some(version), comparison_entries(snapshot))
                }
                _ => return Err(invalid("invalid candidate base")),
            };
            let later = comparison_entries(snapshot);
            let paths: BTreeSet<_> = before.keys().chain(later.keys()).collect();
            let changes: Vec<_> = paths
                .into_iter()
                .filter(|path| before.get(*path) != later.get(*path))
                .collect();
            let start = match after.or(selected) {
                None => 0,
                Some(path) => {
                    changes
                        .iter()
                        .position(|value| value.as_str() == path)
                        .ok_or_else(|| invalid("candidate review cursor is not a changed path"))?
                        + usize::from(after.is_some())
                }
            };
            let end = (start + if selected.is_some() { 1 } else { PAGE }).min(changes.len());
            let rows = changes[start..end]
                .iter()
                .map(|path| {
                    let old = before.get(*path);
                    let new = later.get(*path);
                    let change = match (old, new) {
                        (None, Some(_)) => "added",
                        (Some(_), None) => "removed",
                        (Some(a), Some(b)) if a.kind != b.kind => "type-changed",
                        (Some(a), Some(b)) if a.digest == b.digest && a.bytes == b.bytes => {
                            "mode-changed"
                        }
                        _ => "modified",
                    };
                    Ok(Json::object([
                        ("path", Json::text(path.as_str())),
                        ("change", Json::text(change)),
                        (
                            "before",
                            side(project, base_version, old, path, selected.is_some())?,
                        ),
                        (
                            "after",
                            side(target, Some(target_version), new, path, selected.is_some())?,
                        ),
                    ]))
                })
                .collect::<io::Result<Vec<_>>>()?;
            let context = Json::object([
                ("schema", Json::text("mesh.fleet-project-review-context/v1")),
                ("scope", Json::text("whole-project-snapshot")),
                ("project", Json::text(self.id())),
                ("candidate", field(candidate, "candidate")?.clone()),
                (
                    "candidate_receipt_digest",
                    Json::text(Blake3::digest_bytes(candidate.encode().as_bytes()).to_string()),
                ),
                ("base_head", expected_main.clone()),
                (
                    "base_version",
                    base_version.map_or(Json::Null, |id| Json::text(id.to_string())),
                ),
                ("target_version", Json::text(target_version.to_string())),
                (
                    "content_digest",
                    field(candidate, "content_digest")?.clone(),
                ),
            ]);
            let current_head = current_main.get("head").cloned().unwrap_or(Json::Null);
            Ok(Json::object([
                (
                    "schema",
                    Json::text("mesh.fleet-project-candidate-review/v1"),
                ),
                (
                    "review",
                    Json::text(Blake3::digest_bytes(context.encode().as_bytes()).to_string()),
                ),
                ("context", context),
                ("observed_main", current_main),
                ("provenance", provenance.clone()),
                (
                    "base_is_current",
                    Json::Bool(current_head == *expected_main),
                ),
                (
                    "comparison",
                    Json::object([
                        ("order", Json::text("path")),
                        ("after", after.map_or(Json::Null, Json::text)),
                        ("selected", selected.map_or(Json::Null, Json::text)),
                        ("total", Json::Number(changes.len() as u64)),
                        ("changes", Json::Array(rows)),
                        (
                            "next_after",
                            if selected.is_none() && end < changes.len() {
                                Json::text(changes[end - 1])
                            } else {
                                Json::Null
                            },
                        ),
                    ]),
                ),
                ("approval_authority", Json::Bool(false)),
            ]))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_attachment::{AttachmentStorage, ObservationLimits};
    use ed25519_dalek::{Signer as _, SigningKey};
    use std::fs;

    #[test]
    fn review_pages_all_content_and_bounds_selected_text_without_using_live_bytes() {
        let root = std::env::temp_dir().join(format!(
            "mesh-candidate-review-pages-{}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let files = root.join("files");
        let metadata = root.join("metadata");
        fs::create_dir(&files).unwrap();
        fs::create_dir(&metadata).unwrap();
        for index in 0..205 {
            fs::write(
                files.join(format!("file-{index:03}")),
                format!("saved-{index}"),
            )
            .unwrap();
        }
        fs::write(files.join("exact-limit"), vec![b'x'; TEXT_LIMIT as usize]).unwrap();
        fs::write(files.join("invalid-utf8"), [0xff, 0xfe]).unwrap();
        fs::write(files.join("large"), vec![b'x'; TEXT_LIMIT as usize + 1]).unwrap();
        fs::write(files.join("unsafe"), "hidden\u{202e}text").unwrap();
        let history = AttachmentStorage::open(&metadata)
            .unwrap()
            .provision(&files)
            .unwrap();
        let captured = history
            .project()
            .capture_inputs(ObservationLimits::default())
            .unwrap();
        let key = SigningKey::from_bytes(&[72; 32]);
        let version = history
            .project()
            .save_capture(
                history.metadata_path(),
                &captured,
                mesh_types::PublicKey::from_bytes(key.verifying_key().to_bytes()),
                |payload| {
                    Ok::<_, String>(mesh_types::Signature::from_bytes(
                        key.sign(payload.as_bytes()).to_bytes(),
                    ))
                },
            )
            .unwrap()
            .operation();
        let open = OpenWorkspace::open_attachment_store(
            history.metadata_path(),
            history.store.clone(),
            false,
        )
        .unwrap();
        let snapshot = open.historical_workspace_preview(version).unwrap();
        // Test the internal projection with verified native snapshots. Public service tests separately
        // require a complete retained candidate before this projection can be reached.
        let candidate = |head: Json| {
            Json::object([
                (
                    "candidate",
                    Json::text(format!("candidate-{}", "a".repeat(64))),
                ),
                ("project", Json::text(history.id())),
                (
                    "content_digest",
                    Json::text(super::super::candidates::content_digest(&snapshot)),
                ),
                (
                    "provenance",
                    Json::object([
                        ("source_project", Json::text(history.id())),
                        ("source_version", Json::text(version.to_string())),
                        (
                            "selection",
                            Json::object([("version", Json::text(version.to_string()))]),
                        ),
                        ("expected_main", head),
                    ]),
                ),
            ])
        };
        let receipt = candidate(Json::Null);
        let review = |after, selected| {
            history.review_fleet_candidate(
                &receipt,
                &open,
                snapshot.clone(),
                &TrustedReviewers::default(),
                (after, selected),
            )
        };
        let first = review(None, None).unwrap();
        let comparison = first.get("comparison").unwrap();
        assert_eq!(comparison.get("total"), Some(&Json::Number(209)));
        assert_eq!(
            comparison.get("changes").unwrap().as_array().unwrap().len(),
            200
        );
        let next = comparison.get("next_after").unwrap().as_text().unwrap();
        let last = review(Some(next), None).unwrap();
        assert_eq!(
            last.get("comparison")
                .unwrap()
                .get("changes")
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            9
        );
        assert_eq!(
            last.get("comparison").unwrap().get("next_after"),
            Some(&Json::Null)
        );
        assert_eq!(last.get("review"), first.get("review"));
        fs::write(files.join("file-000"), "newer live bytes").unwrap();
        for (path, state, expected) in [
            ("file-000", "text", Json::text("saved-0")),
            (
                "exact-limit",
                "text",
                Json::text("x".repeat(TEXT_LIMIT as usize)),
            ),
            ("invalid-utf8", "binary-or-unsafe-text", Json::Null),
            ("large", "too-large", Json::Null),
            ("unsafe", "binary-or-unsafe-text", Json::Null),
        ] {
            let selected = review(None, Some(path)).unwrap();
            let row = &selected
                .get("comparison")
                .unwrap()
                .get("changes")
                .unwrap()
                .as_array()
                .unwrap()[0];
            assert_eq!(
                row.get("after").unwrap().get("content_state"),
                Some(&Json::text(state))
            );
            assert_eq!(row.get("after").unwrap().get("text"), Some(&expected));
            assert_eq!(selected.get("review"), first.get("review"));
        }
        assert!(review(Some("unknown"), None).is_err());
        assert!(review(None, Some("../files/file-000")).is_err());
        assert!(review(Some(next), Some("file-000")).is_err());
        let unapproved = candidate(Json::text(open.private_version().version().unwrap()));
        assert!(history
            .review_fleet_candidate(
                &unapproved,
                &open,
                snapshot.clone(),
                &TrustedReviewers::default(),
                (None, None)
            )
            .is_err());
        let mut substituted = snapshot.clone();
        substituted.files[0].content_digest = RecordDigest::from_bytes([0; 32]);
        assert!(history
            .review_fleet_candidate(
                &receipt,
                &open,
                substituted,
                &TrustedReviewers::default(),
                (None, None)
            )
            .is_err());
        assert_eq!(
            history
                .project()
                .saved_versions(history.metadata_path())
                .unwrap()
                .len(),
            1
        );
        fs::remove_dir_all(root).unwrap();
    }
}
