//! Owner-bound immutable saved reviews. No agent endpoint or publication admission.
use super::{
    dependency_decision::{NativeControlInput, Step},
    dependency_owner_context::OwnerHistoryContext,
    dependency_transaction::text,
    invalid, AttachmentStorage, NativeDependencyReviewSnapshot, ProvisionedAttachment,
    SavedAttachmentVersion,
};
use crate::ipc::Json;
use mesh_store::{DependencyKind, RecordDigest};
use std::{fs::File, io};

/// Exact native request. The opener is attribution, never approval authority.
pub struct NativeSavedReviewRequest<'a> {
    /// Registered work whose exact saved version is reviewed.
    pub source: &'a ProvisionedAttachment,
    /// Immutable saved output.
    pub version: SavedAttachmentVersion,
    /// Previously retained complete owner snapshot.
    pub snapshot: NativeDependencyReviewSnapshot,
    /// Stable idempotency identity for this binding.
    pub request: RecordDigest,
    /// Native opener attribution; no approval capability.
    pub opener: RecordDigest,
}
/// Retained owner-journal binding; not present eligibility or permission to publish.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeSavedDependencyReview {
    record: RecordDigest,
    bundle: RecordDigest,
}
impl NativeSavedDependencyReview {
    pub(super) fn from_verified_record(record: RecordDigest, bundle: RecordDigest) -> Self {
        Self { record, bundle }
    }

    /// Exact retained owner-journal binding payload.
    pub fn record(&self) -> RecordDigest {
        self.record
    }
    /// Immutable bundle identity including native snapshot evidence.
    pub fn bundle(&self) -> RecordDigest {
        self.bundle
    }
}
fn error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
fn current_review_base(
    history: &crate::workspace::OpenWorkspace,
) -> io::Result<mesh_approval::HeadId> {
    Ok(super::approval::main_head(history)?.unwrap_or(crate::publication::GENESIS_SHARED_HEAD))
}

impl AttachmentStorage {
    /// Bind a complete retained native snapshot to one exact immutable saved review.
    pub fn save_dependency_review(
        &self,
        owner: &ProvisionedAttachment,
        request: NativeSavedReviewRequest<'_>,
        available: &[&ProvisionedAttachment],
    ) -> io::Result<NativeSavedDependencyReview> {
        self.save_dependency_review_with_io(
            owner,
            request,
            available,
            |_, _, _| Ok(()),
            |f| f.sync_all(),
        )
    }
    pub(super) fn save_dependency_review_with_io(
        &self,
        owner: &ProvisionedAttachment,
        request: NativeSavedReviewRequest<'_>,
        available: &[&ProvisionedAttachment],
        hook: impl FnMut(Step, &mut File, &[u8]) -> io::Result<()>,
        sync: impl FnMut(&File) -> io::Result<()>,
    ) -> io::Result<NativeSavedDependencyReview> {
        let works = std::iter::once(request.source)
            .chain(available.iter().copied())
            .collect::<Vec<_>>();
        let prepared = self.prepare_dependency_graph(
            owner,
            request.source,
            request.version.operation(),
            &works,
        )?;
        let guard = crate::workspace_custody::lock_workspace_initialization_set(&prepared.roots)
            .map_err(error)?;
        let mut saved_bundle = None;
        let result = owner.control_with_io(
            None,
            request.request,
            |_, proof| {
                let context = self.resolve_consumed_histories(
                    owner,
                    &works,
                    &guard,
                    OwnerHistoryContext::for_control(owner, proof)?,
                )?;
                let graph =
                    self.inspect_dependency_graph_with_owner(&prepared, &guard, &context)?;
                let evidence = proof
                    .policy()
                    .review_evidence(request.snapshot.record())
                    .ok_or_else(|| invalid("complete review snapshot is absent from this owner"))?;
                if evidence.output() != graph.review_output()
                    || graph.digest() != request.snapshot.graph()
                    || evidence.validation() != request.snapshot.validation()
                {
                    return Err(invalid(
                        "saved review snapshot does not match the exact native output",
                    ));
                }
                let (_, _, history) = context.history(request.source)?;
                let prior = if proof.policy().native_request(request.request).is_some() {
                    Some(
                        proof
                            .policy()
                            .review_binding_body(request.request)
                            .ok_or_else(|| invalid("request belongs to another native control"))?,
                    )
                } else {
                    None
                };
                let canonical = if let Some(body) = &prior {
                    mesh_approval::HeadId::from_bytes(
                        *RecordDigest::parse_hex(text(body, "canonical")?)
                            .map_err(error)?
                            .as_bytes(),
                    )
                } else {
                    current_review_base(&history)?
                };
                let bundle = history
                    .native_saved_review_bundle(request.version.operation(), canonical, &evidence)
                    .map_err(error)?;
                let output = evidence.output();
                let body = Json::object([
                    ("request", Json::text(request.request.to_hex())),
                    ("revision", Json::Number(1)),
                    ("snapshot", Json::text(request.snapshot.record().to_hex())),
                    (
                        "output",
                        Json::Array(vec![
                            Json::Array(vec![
                                Json::text(output.0.to_hex()),
                                Json::text(output.1.to_hex()),
                            ]),
                            Json::text(output.2.to_hex()),
                        ]),
                    ),
                    (
                        "canonical",
                        Json::text(RecordDigest::from_bytes(*canonical.as_bytes()).to_hex()),
                    ),
                    ("bundle", Json::text(bundle.to_hex())),
                    ("opener", Json::text(request.opener.to_hex())),
                ]);
                if prior.as_ref().is_some_and(|prior| prior != &body) {
                    return Err(invalid(
                        "saved review request was reused with different evidence",
                    ));
                }
                saved_bundle = Some(bundle);
                Ok(NativeControlInput {
                    kind: DependencyKind::ReviewSnapshot,
                    revision_field: "revision",
                    body,
                    prior: None,
                })
            },
            hook,
            sync,
        )?;
        guard.ensure_current().map_err(error)?;
        Ok(NativeSavedDependencyReview {
            record: result.record(),
            bundle: saved_bundle.ok_or_else(|| invalid("missing saved review bundle"))?,
        })
    }
}

mod read;
mod receipt;

#[cfg(test)]
mod tests;
