//! Native immutable review evidence. No renderer, agent, CLI or publication authority is exposed.
use super::{
    dependency_decision::{NativeControlInput, Step},
    dependency_owner_context::OwnerHistoryContext,
    dependency_transaction::{hash, read_payload},
    invalid, AttachmentStorage, ProvisionedAttachment, SavedAttachmentVersion,
};
use crate::{dependency_policy::QualifiedDependencyInput, ipc::Json, root_authority::PinnedRootFs};
use mesh_cas::{Blake3, Cas};
use mesh_store::{DependencyKind, RecordDigest};
use std::{fs::File, io};

/// Durable historical review evidence; not present eligibility or approval authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeDependencyReviewSnapshot {
    record: RecordDigest,
    graph: RecordDigest,
    validation: RecordDigest,
}
impl NativeDependencyReviewSnapshot {
    /// Exact owner-journal snapshot payload.
    pub fn record(&self) -> RecordDigest {
        self.record
    }
    /// Complete canonical immutable graph object retained in the owner's store.
    pub fn graph(&self) -> RecordDigest {
        self.graph
    }
    /// Native validation binding for this historical output and decision vector.
    pub fn validation(&self) -> RecordDigest {
        self.validation
    }
}
fn error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
fn input(i: QualifiedDependencyInput) -> Json {
    Json::Array(vec![
        Json::Array(vec![Json::text(i.0.to_hex()), Json::text(i.1.to_hex())]),
        Json::text(i.2.to_hex()),
    ])
}
impl AttachmentStorage {
    /// Freeze complete exact input evidence through native custody and durable owner replay.
    /// This does not create an ordinary review, authorize consumption, or advance protected main.
    pub fn save_dependency_review_snapshot(
        &self,
        owner: &ProvisionedAttachment,
        source: &ProvisionedAttachment,
        version: SavedAttachmentVersion,
        available: &[&ProvisionedAttachment],
        request: RecordDigest,
    ) -> io::Result<NativeDependencyReviewSnapshot> {
        self.save_dependency_review_snapshot_with_io(
            owner,
            source,
            version,
            available,
            request,
            |_, _, _| Ok(()),
            |file| file.sync_all(),
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn save_dependency_review_snapshot_with_io(
        &self,
        owner: &ProvisionedAttachment,
        source: &ProvisionedAttachment,
        version: SavedAttachmentVersion,
        available: &[&ProvisionedAttachment],
        request: RecordDigest,
        hook: impl FnMut(Step, &mut File, &[u8]) -> io::Result<()>,
        sync: impl FnMut(&File) -> io::Result<()>,
    ) -> io::Result<NativeDependencyReviewSnapshot> {
        let works = std::iter::once(source)
            .chain(available.iter().copied())
            .collect::<Vec<_>>();
        let prepared = self.prepare_dependency_graph(owner, source, version.operation(), &works)?;
        let guard = crate::workspace_custody::lock_workspace_initialization_set(&prepared.roots)
            .map_err(error)?;
        let mut evidence = None;
        let result = owner.control_with_io(
            None,
            request,
            |_, proof| {
                let context = self.resolve_consumed_histories(
                    owner,
                    &works,
                    &guard,
                    OwnerHistoryContext::for_control(owner, proof)?,
                )?;
                let graph =
                    self.inspect_dependency_graph_with_owner(&prepared, &guard, &context)?;
                let graph_bytes = graph.to_json().encode().into_bytes();
                let output = input(graph.review_output());
                let graph_id = graph.digest();
                let cas = Cas::<PinnedRootFs, Blake3>::with_filesystem(
                    owner.metadata_path(),
                    owner.store.filesystem(),
                )
                .map_err(error)?;
                let body = if let Some(record) = proof.policy().native_request(request) {
                    let body = proof
                        .policy()
                        .review_snapshot_body(request)
                        .ok_or_else(|| invalid("request belongs to a different native control"))?;
                    if body.get("output") != Some(&output)
                        || body.get("graph") != Some(&Json::text(graph_id.to_hex()))
                    {
                        return Err(invalid(
                            "snapshot request was reused with different output or inputs",
                        ));
                    }
                    let stored = read_payload(&cas, graph_id, 4 * 1024 * 1024)?;
                    if stored != graph_bytes {
                        return Err(invalid("historical snapshot graph differs"));
                    }
                    proof
                        .policy()
                        .verify_review_graph(record.payload, &stored)
                        .map_err(error)?;
                    body
                } else {
                    let decisions = graph
                        .review_inputs()
                        .map(|i| {
                            let (revision, record) =
                                proof.policy().eligible_decision(i).ok_or_else(|| {
                                    invalid("review input has no current eligible native decision")
                                })?;
                            Ok(Json::Array(vec![
                                input(i),
                                Json::Number(revision),
                                Json::text(record.to_hex()),
                            ]))
                        })
                        .collect::<io::Result<Vec<_>>>()?;
                    let decisions = Json::Array(decisions);
                    let validation = Json::object([
                        ("schema", Json::text("mesh.native-review-validation/v1")),
                        ("output", output.clone()),
                        ("graph", Json::text(graph_id.to_hex())),
                        ("decisions", decisions.clone()),
                    ])
                    .encode();
                    let body = Json::object([
                        ("request", Json::text(request.to_hex())),
                        ("revision", Json::Number(1)),
                        ("output", output),
                        ("graph", Json::text(graph_id.to_hex())),
                        ("decisions", decisions),
                        (
                            "validation",
                            Json::text(hash(validation.as_bytes()).to_hex()),
                        ),
                    ]);
                    cas.promote(graph_bytes.clone()).map_err(error)?;
                    if read_payload(&cas, graph_id, 4 * 1024 * 1024)? != graph_bytes {
                        return Err(invalid("staged review graph differs"));
                    }
                    body
                };
                let validation = super::dependency_transaction::digest(
                    super::dependency_transaction::text(&body, "validation")?,
                )?;
                evidence = Some((graph_id, validation));
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
        let (graph, validation) =
            evidence.ok_or_else(|| invalid("missing native review evidence"))?;
        Ok(NativeDependencyReviewSnapshot {
            record: result.record(),
            graph,
            validation,
        })
    }
}

#[cfg(test)]
mod tests;
