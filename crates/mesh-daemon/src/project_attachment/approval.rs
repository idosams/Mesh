//! Human receipts advance external Mesh history only. Source files and Git remain untouched.
use super::{invalid, ProjectAttachment};
use crate::root_authority::PinnedWorkspaceRoot;
use crate::workspace::OpenWorkspace;
use crate::{HumanApprovalPreview, TrustedReviewers};
use mesh_approval::{ExpectedHumanApproval, HeadId, HumanApprovalReceipt};
use mesh_store::{
    ApprovalRecord, RecordDigest, RecordKind, ReviewRecord, ReviewVerdict, StoredRecord,
};
use std::io;
use std::path::Path;

fn error(problem: impl std::fmt::Display) -> io::Error {
    io::Error::other(problem.to_string())
}

/// Absence of verified authority is genesis only when no approval record exists.
pub(super) fn main_head(workspace: &OpenWorkspace) -> io::Result<Option<HeadId>> {
    let head = workspace.shared_version();
    if head.is_none()
        && workspace
            .records_by_kind()
            .iter()
            .any(|(kind, count)| *kind == RecordKind::Approval && *count != 0)
    {
        return Err(invalid("existing Mesh main approval cannot be verified"));
    }
    Ok(head)
}

fn review(workspace: &OpenWorkspace, bundle: &str, target: &str) -> io::Result<ReviewRecord> {
    let parse = |value: &str| {
        let digest = RecordDigest::parse_hex(value).map_err(error)?;
        if digest.to_string() != value {
            return Err(invalid("noncanonical review identity"));
        }
        Ok(digest)
    };
    let record = workspace
        .review(&parse(bundle)?)
        .ok_or_else(|| invalid("review unavailable"))?;
    if record.subject_operation != parse(target)? {
        return Err(invalid("review target mismatch"));
    }
    Ok(record)
}

fn require_current_main(
    workspace: &OpenWorkspace,
    preview: &HumanApprovalPreview,
) -> io::Result<()> {
    let main = main_head(workspace)?;
    if main == Some(preview.context().reviewed_actor_head()) {
        return Err(invalid("review is already Mesh main"));
    }
    if main.unwrap_or(crate::publication::GENESIS_SHARED_HEAD)
        != preview.context().expected_canonical_head()
    {
        return Err(invalid("Mesh main changed since this review was opened"));
    }
    Ok(())
}

impl ProjectAttachment {
    pub(super) fn prepare_approval(
        &self,
        metadata: &Path,
        store: PinnedWorkspaceRoot,
        bundle: &str,
        target: &str,
        trusted: &TrustedReviewers,
    ) -> io::Result<HumanApprovalPreview> {
        if !trusted.has_human_credentials() {
            return Err(invalid(
                "native human approval credential is not configured",
            ));
        }
        self.with_review_history(metadata, store, trusted, |workspace, _| {
            let record = review(workspace, bundle, target)?;
            let preview = HumanApprovalPreview::from_record(workspace, &record).map_err(error)?;
            require_current_main(workspace, &preview)?;
            Ok(preview)
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn approve_saved_review(
        &self,
        metadata: &Path,
        store: PinnedWorkspaceRoot,
        bundle: &str,
        target: &str,
        bytes: &[u8],
        trusted: &TrustedReviewers,
    ) -> io::Result<HeadId> {
        let receipt = HumanApprovalReceipt::from_canonical_bytes(bytes).map_err(error)?;
        let carried = receipt.draft().expected();
        let credential = trusted
            .human_credential(carried.credential().id())
            .ok_or_else(|| invalid("human approval credential is not trusted"))?;
        if carried.challenge() == &[0; 32] {
            return Err(invalid("approval challenge is empty"));
        }
        self.with_review_history(metadata, store, trusted, |workspace, store| {
            let record = review(workspace, bundle, target)?;
            let preview = HumanApprovalPreview::from_record(workspace, &record).map_err(error)?;
            let expected = ExpectedHumanApproval::new(
                preview.context().clone(),
                credential,
                *carried.challenge(),
            );
            mesh_approval::verify_human_approval_receipt(bytes, &expected).map_err(error)?;
            let head = expected.context().reviewed_actor_head();
            if main_head(workspace)? == Some(head) {
                // An acknowledged append can lose its response. Only the same retained receipt
                // may be retried; a different ceremony or an older main is never silently accepted.
                let approval = workspace.approved_envelope(&record.bundle).map_err(error)?;
                if workspace
                    .approval_receipt(approval.approval)
                    .map_err(error)?
                    == bytes
                {
                    return Ok(head);
                }
                return Err(invalid(
                    "Mesh main already has a different approval receipt",
                ));
            }
            require_current_main(workspace, &preview)?;
            if workspace
                .approval_challenge_used(carried.challenge())
                .map_err(error)?
            {
                return Err(invalid("approval challenge was already used"));
            }
            // Capture may advance private history while the human considers this review. Unlike
            // managed-folder export, attachment approval accepts immutable saved content only;
            // neither current source bytes nor the newest capture can enter this receipt.
            self.ensure_current()?;
            store.ensure_namespace_identity()?;
            let digest = workspace.promote_approval_receipt(bytes.to_vec())?;
            workspace.append_record(&StoredRecord::Approval(ApprovalRecord {
                approval: digest,
                bundle: record.bundle,
                approver: RecordDigest::from_bytes(*carried.credential().id().as_bytes()),
                verdict: ReviewVerdict::Approved,
            }))?;
            let reopened = OpenWorkspace::open_attachment_store_with_trusted_reviewers(
                metadata,
                store.clone(),
                false,
                trusted,
            )
            .map_err(error)?;
            if main_head(&reopened)? != Some(head) {
                return Err(invalid("approved Mesh main was not retained"));
            }
            Ok(head)
        })
    }
}
