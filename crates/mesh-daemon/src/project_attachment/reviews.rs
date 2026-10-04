//! Exact local review requests in the existing journal. No source write or approval capability.
use super::{history::verify_history_binding, invalid, read_receipt, ProjectAttachment};
use crate::ipc::Json;
use crate::root_authority::PinnedWorkspaceRoot;
use crate::workspace::OpenWorkspace;
use crate::TrustedReviewers;
use mesh_store::{RecordDigest, ReviewRecord, StoredRecord};
use mesh_types::{Blake3, PublicKey};
use std::io;
use std::path::Path;

fn error(problem: impl std::fmt::Display) -> io::Error {
    io::Error::other(problem.to_string())
}
fn digest(value: &str) -> io::Result<RecordDigest> {
    let digest = RecordDigest::parse_hex(value).map_err(error)?;
    if digest.to_string() != value {
        return Err(invalid("noncanonical review identity"));
    }
    Ok(digest)
}
fn relative_review_path(value: Option<&Json>) -> io::Result<Json> {
    let value = value.ok_or_else(|| invalid("missing review path"))?;
    if value == &Json::Null {
        return Ok(Json::Null);
    }
    // Review presentation paths begin at the logical workspace root, not the OS root. Attachment
    // projections use the same relative names as saved-file inspection and never expose that slash.
    let path = value
        .as_text()
        .and_then(|path| path.strip_prefix('/'))
        .ok_or_else(|| invalid("invalid logical review path"))?;
    if path.is_empty()
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(invalid("invalid relative review path"));
    }
    Ok(Json::text(path))
}
pub(super) fn summary(item: Json) -> io::Result<Json> {
    let field = |name| {
        item.get(name)
            .cloned()
            .ok_or_else(|| invalid("missing native review field"))
    };
    let Some(Json::Array(changes)) = item.get("bundle_changes") else {
        return Err(invalid("missing review changes"));
    };
    let changes = changes
        .iter()
        .map(|change| {
            Ok(Json::object([
                ("before", relative_review_path(change.get("path_before"))?),
                ("after", relative_review_path(change.get("path_after"))?),
                (
                    "effect",
                    change
                        .get("effect")
                        .cloned()
                        .ok_or_else(|| invalid("missing review effect"))?,
                ),
            ]))
        })
        .collect::<io::Result<Vec<_>>>()?;
    Ok(Json::object([
        ("bundle", field("bundle")?),
        ("target", field("subject_operation")?),
        ("reviewed_head", field("reviewed_head")?),
        ("presentation", field("presentation_digest")?),
        ("complete", field("content_complete")?),
        ("unavailable", field("unavailable_code")?),
        ("changes", Json::Array(changes)),
        ("changes_not_listed", field("bundle_changes_not_listed")?),
        (
            "operations_not_listed",
            field("subject_operations_not_listed")?,
        ),
        ("author_attribution", Json::text("unknown")),
        ("approval_authority", Json::Bool(false)),
    ]))
}
impl ProjectAttachment {
    pub(super) fn with_review_history<T>(
        &self,
        metadata: &Path,
        store: PinnedWorkspaceRoot,
        trusted: &TrustedReviewers,
        action: impl FnOnce(&mut OpenWorkspace, &PinnedWorkspaceRoot) -> io::Result<T>,
    ) -> io::Result<T> {
        self.ensure_current()?;
        store.ensure_namespace_identity()?;
        if read_receipt(&store)? != self.receipt()?.encode() {
            return Err(invalid("attachment receipt changed"));
        }
        let _guard =
            crate::workspace_custody::lock_workspace_initialization(&store).map_err(error)?;
        let (configuration, _) = self.history_configuration(&store, None)?;
        let mut workspace = OpenWorkspace::open_attachment_store_with_trusted_reviewers(
            metadata,
            store.clone(),
            false,
            trusted,
        )
        .map_err(error)?;
        verify_history_binding(&workspace, &configuration)?;
        let result = action(&mut workspace, &store)?;
        store.ensure_namespace_identity()?;
        self.ensure_current()?;
        Ok(result)
    }

    pub(super) fn request_saved_review(
        &self,
        metadata: &Path,
        store: PinnedWorkspaceRoot,
        target: &str,
        actor: PublicKey,
        trusted: &TrustedReviewers,
    ) -> io::Result<Json> {
        let target = digest(target)?;
        self.with_review_history(metadata, store, trusted, |workspace, store| {
            if !workspace
                .workspace_versions()
                .iter()
                .any(|version| version.operation() == target)
            {
                return Err(invalid("review target does not belong to this project"));
            }
            super::approval::main_head(workspace)?;
            let bundle = workspace
                .saved_publication_review_bundle(target)
                .map_err(error)?;
            if let Some(record) = workspace.review(&bundle) {
                if record.subject_operation != target {
                    return Err(invalid("review target mismatch"));
                }
            } else {
                workspace.append_record(&StoredRecord::Review(ReviewRecord {
                    bundle,
                    subject_operation: target,
                    opened_by: RecordDigest::from_bytes(
                        *actor.actor_id::<Blake3>().digest().as_bytes(),
                    ),
                }))?;
            }
            // Acknowledgement is based on reopened durable truth, including for idempotent retries.
            let reopened = OpenWorkspace::open_attachment_store_with_trusted_reviewers(
                metadata,
                store.clone(),
                false,
                trusted,
            )
            .map_err(error)?;
            let record = reopened
                .review(&bundle)
                .ok_or_else(|| invalid("review was not retained"))?;
            if record.subject_operation != target {
                return Err(invalid("retained review changed"));
            }
            summary(
                reopened
                    .recorded_review_item(bundle)
                    .ok_or_else(|| invalid("review unavailable"))?,
            )
        })
    }

    pub(super) fn saved_reviews(
        &self,
        metadata: &Path,
        store: PinnedWorkspaceRoot,
        trusted: &TrustedReviewers,
    ) -> io::Result<Json> {
        self.with_read_history(metadata, store, trusted, |workspace, _, _| {
            let (items, omitted) = workspace.review_items(None);
            Ok(Json::object([
                (
                    "reviews",
                    Json::Array(
                        items
                            .into_iter()
                            .map(summary)
                            .collect::<io::Result<Vec<_>>>()?,
                    ),
                ),
                ("not_listed", Json::Number(omitted)),
            ]))
        })
    }

    pub(super) fn saved_review(
        &self,
        metadata: &Path,
        store: PinnedWorkspaceRoot,
        bundle: &str,
        target: &str,
        trusted: &TrustedReviewers,
    ) -> io::Result<Json> {
        let bundle = digest(bundle)?;
        let target = digest(target)?;
        self.with_read_history(metadata, store, trusted, |workspace, _, _| {
            let record = workspace
                .review(&bundle)
                .ok_or_else(|| invalid("review unavailable"))?;
            if record.subject_operation != target {
                return Err(invalid("review target mismatch"));
            }
            summary(
                workspace
                    .recorded_review_item(bundle)
                    .ok_or_else(|| invalid("review unavailable"))?,
            )
        })
    }
}
