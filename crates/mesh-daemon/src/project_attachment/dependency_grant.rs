//! Explicit native-host grants. No actor, renderer or CLI route can issue these records.
use super::{
    dependency_decision::{NativeControlInput, Step},
    invalid, AttachmentStorage, ProvisionedAttachment, SavedAttachmentVersion,
};
use crate::ipc::Json;
use mesh_store::{DependencyKind, RecordDigest};
use std::{fs::File, io};
const ZERO: RecordDigest = RecordDigest::from_bytes([0; 32]);

/// Native-host-selected exact input and destination, independent of provider or run identity.
/// This request does not materialize content or establish a consumed-input receipt.
pub struct NativeInputGrantRequest<'a> {
    /// Existing native source work within the owning project.
    pub source: &'a ProvisionedAttachment,
    /// Exact immutable saved operation in that source work.
    pub version: SavedAttachmentVersion,
    /// Existing native destination work within the same project.
    pub destination: &'a ProvisionedAttachment,
    /// True permits the specified consumption; false revokes future consumption.
    pub allowed: bool,
    /// Exact prior grant/revocation for this input and destination, or None for generation one.
    pub expected_previous: Option<RecordDigest>,
    /// Stable nonzero request identifier. Retries must repeat the exact original intent.
    pub request: RecordDigest,
}

/// Durable historical grant facts. Current admission must revalidate policy and native bindings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeInputGrant {
    record: RecordDigest,
    generation: u64,
}
impl NativeInputGrant {
    /// Exact immutable grant payload retained by the owning authority.
    pub fn record(&self) -> RecordDigest {
        self.record
    }
    /// Per-input/destination authorization generation, not an eligibility decision revision.
    pub fn generation(&self) -> u64 {
        self.generation
    }
}
fn error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
fn work(id: RecordDigest, installation: RecordDigest) -> Json {
    Json::Array(vec![
        Json::text(id.to_hex()),
        Json::text(installation.to_hex()),
    ])
}
impl AttachmentStorage {
    /// Grant or revoke one exact input for one exact destination. This development entry point is
    /// trusted-native-host only. Agents cannot call it, and no consumption path is enabled by it.
    pub fn grant_saved_input(
        &self,
        owner: &ProvisionedAttachment,
        request: NativeInputGrantRequest<'_>,
    ) -> io::Result<NativeInputGrant> {
        self.grant_with_io(owner, request, |_, _, _| Ok(()), |file| file.sync_all())
    }

    fn grant_with_io(
        &self,
        owner: &ProvisionedAttachment,
        request: NativeInputGrantRequest<'_>,
        hook: impl FnMut(Step, &mut File, &[u8]) -> io::Result<()>,
        sync: impl FnMut(&File) -> io::Result<()>,
    ) -> io::Result<NativeInputGrant> {
        self.grant_with_inputs_and_io(owner, request, &[], hook, sync)
    }
    /// Grant or revoke an exact saved input with its complete native history context. This remains
    /// a trusted-native-host operation; allocation and historical readability do not grant access.
    pub fn grant_saved_input_with_inputs(
        &self,
        owner: &ProvisionedAttachment,
        request: NativeInputGrantRequest<'_>,
        available: &[&ProvisionedAttachment],
    ) -> io::Result<NativeInputGrant> {
        self.grant_with_inputs_and_io(
            owner,
            request,
            available,
            |_, _, _| Ok(()),
            |file| file.sync_all(),
        )
    }
    fn grant_with_inputs_and_io(
        &self,
        owner: &ProvisionedAttachment,
        request: NativeInputGrantRequest<'_>,
        available: &[&ProvisionedAttachment],
        hook: impl FnMut(Step, &mut File, &[u8]) -> io::Result<()>,
        sync: impl FnMut(&File) -> io::Result<()>,
    ) -> io::Result<NativeInputGrant> {
        let source = self.prepare_dependency_work(owner, request.source)?;
        let destination = self.prepare_dependency_work(owner, request.destination)?;
        // A destination may be empty or mid-transaction. Verify its native correlation and
        // custody without demanding a readable source history or admitting its pending work.
        let works = std::iter::once(request.source)
            .chain(available.iter().copied())
            .collect::<Vec<_>>();
        let selected_works = works
            .iter()
            .copied()
            .chain(std::iter::once(request.destination))
            .collect::<Vec<_>>();
        let graph = self.prepare_dependency_graph(
            owner,
            request.source,
            request.version.operation(),
            &selected_works,
        )?;
        let guard = crate::workspace_custody::lock_workspace_initialization_set(&graph.roots)
            .map_err(error)?;
        let receipt = owner.control_with_io(
            request.expected_previous,
            request.request,
            |owner_history, proof| {
                let context = self.resolve_consumed_histories(
                    owner,
                    &works,
                    &guard,
                    super::dependency_owner_context::OwnerHistoryContext::for_control(
                        owner, proof,
                    )?,
                )?;
                let source = context.validate(self, &source, &guard)?;
                let destination = context.validate(self, &destination, &guard)?;
                if source.authority() != destination.authority()
                    || source.project() != destination.project()
                {
                    return Err(invalid("grant crosses native project authority"));
                }
                if request.source.id() == owner.id() {
                    if !owner_history
                        .workspace_versions()
                        .iter()
                        .any(|v| v.operation() == request.version.operation())
                    {
                        return Err(invalid(
                            "grant input is not a saved operation of its source work",
                        ));
                    }
                    owner_history
                        .historical_workspace_preview(request.version.operation())
                        .map_err(error)?;
                } else if context.has_verified_history(request.source) {
                    let (_, _, history) = context.history(request.source)?;
                    SavedAttachmentVersion::from_verified_history(
                        &history,
                        request.version.operation(),
                    )?;
                    history
                        .historical_workspace_preview(request.version.operation())
                        .map_err(error)?;
                } else {
                    request.source.attachment.inspect_saved(
                        request.source.metadata_path(),
                        request.source.store.clone(),
                        &request.version.operation().to_string(),
                        |workspace, version| {
                            workspace
                                .historical_workspace_preview(version)
                                .map(|_| ())
                                .map_err(error)
                        },
                    )?;
                }
                Ok(NativeControlInput {
                    kind: DependencyKind::Grant,
                    revision_field: "generation",
                    body: Json::object([
                        ("request", Json::text(request.request.to_hex())),
                        (
                            "source",
                            Json::Array(vec![
                                work(source.work(), source.installation()),
                                Json::text(request.version.operation().to_hex()),
                            ]),
                        ),
                        (
                            "destination",
                            work(destination.work(), destination.installation()),
                        ),
                        ("generation", Json::Number(0)),
                        (
                            "previous",
                            Json::text(request.expected_previous.unwrap_or(ZERO).to_hex()),
                        ),
                        ("allowed", Json::Bool(request.allowed)),
                        (
                            "bindings",
                            Json::Array(vec![
                                Json::text(source.correlation.to_hex()),
                                Json::text(destination.correlation.to_hex()),
                            ]),
                        ),
                    ]),
                    prior: proof.policy().native_grant(
                        (
                            source.work(),
                            source.installation(),
                            request.version.operation(),
                        ),
                        (destination.work(), destination.installation()),
                    ),
                })
            },
            hook,
            sync,
        )?;
        guard.ensure_current().map_err(error)?;
        Ok(NativeInputGrant {
            record: receipt.record(),
            generation: receipt.revision(),
        })
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod consumed_tests;
