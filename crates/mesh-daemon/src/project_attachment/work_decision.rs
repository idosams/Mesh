//! Owning-project decisions for native root or child work, independent of execution identity.
use super::{
    dependency_decision::{body, NativeControlInput, Step},
    invalid, AttachmentStorage, NativeInputDecision, ProvisionedAttachment, SavedAttachmentVersion,
    SavedInputDecision,
};
use mesh_store::{DependencyKind, RecordDigest};
use std::{fs::File, io};
const ZERO: RecordDigest = RecordDigest::from_bytes([0; 32]);

/// Explicit native-host decision for an exact saved input within an owning project.
pub struct NativeWorkDecisionRequest<'a> {
    /// Native registered source work, which may be the owner or one of its descendants.
    pub source: &'a ProvisionedAttachment,
    /// Exact saved operation whose publication eligibility changes.
    pub version: SavedAttachmentVersion,
    /// Rejection, replacement within this same work, or explicit revalidation.
    pub decision: SavedInputDecision,
    /// Exact prior decision for this input, or None for its first decision.
    pub expected_previous: Option<RecordDigest>,
    /// Nonzero stable request. A historical retry returns only its original record.
    pub request: RecordDigest,
}
fn error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
impl AttachmentStorage {
    /// Record input eligibility in the owning project, including for manual or delegated child
    /// work. No renderer, agent or CLI calls this native control. It grants no access or approval.
    pub fn decide_work_input(
        &self,
        owner: &ProvisionedAttachment,
        request: NativeWorkDecisionRequest<'_>,
    ) -> io::Result<NativeInputDecision> {
        self.decide_work_with_io(owner, request, |_, _, _| Ok(()), |file| file.sync_all())
    }

    fn decide_work_with_io(
        &self,
        owner: &ProvisionedAttachment,
        request: NativeWorkDecisionRequest<'_>,
        hook: impl FnMut(Step, &mut File, &[u8]) -> io::Result<()>,
        sync: impl FnMut(&File) -> io::Result<()>,
    ) -> io::Result<NativeInputDecision> {
        let prepared = self.prepare_dependency_work(owner, request.source)?;
        let guard = crate::workspace_custody::lock_workspace_initialization_set(&prepared.roots)
            .map_err(error)?;
        let result = owner.control_with_io(
            request.expected_previous,
            request.request,
            |owner_history, proof| {
                let work = self.validate_dependency_work_with_history(
                    &prepared,
                    &guard,
                    proof,
                    owner_history,
                )?;
                let validate = |version: SavedAttachmentVersion| -> io::Result<()> {
                    if request.source.id() == owner.id() {
                        if !owner_history
                            .workspace_versions()
                            .iter()
                            .any(|saved| saved.operation() == version.operation())
                        {
                            return Err(invalid("decision input is not saved in its native work"));
                        }
                        owner_history
                            .historical_workspace_preview(version.operation())
                            .map_err(error)?;
                    } else {
                        request.source.attachment.inspect_saved(
                            request.source.metadata_path(),
                            request.source.store.clone(),
                            &version.operation().to_string(),
                            |workspace, operation| {
                                workspace
                                    .historical_workspace_preview(operation)
                                    .map(|_| ())
                                    .map_err(error)
                            },
                        )?;
                    }
                    Ok(())
                };
                validate(request.version)?;
                if let SavedInputDecision::Replaced(replacement) = request.decision {
                    validate(replacement)?;
                }
                Ok(NativeControlInput {
                    kind: DependencyKind::Eligibility,
                    revision_field: "revision",
                    body: body(
                        work.work(),
                        work.installation(),
                        request.version,
                        request.decision,
                        request.request,
                        0,
                        request.expected_previous.unwrap_or(ZERO),
                    ),
                    prior: proof.policy().native_decision(
                        work.work(),
                        work.installation(),
                        request.version.operation(),
                    ),
                })
            },
            hook,
            sync,
        )?;
        guard.ensure_current().map_err(error)?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
