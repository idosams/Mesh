//! Exact retained graph and historical decision vector for native review snapshots.
use super::*;
use crate::dependency_policy::QualifiedDependencyInput;
use crate::project_attachment::dependency_transaction::read_payload;

#[derive(PartialEq, Eq)]
pub(super) struct RetainedGraph {
    digest: RecordDigest,
    bytes: Vec<u8>,
}
impl RetainedGraph {
    pub(super) fn verify(&self, owner: &ProvisionedAttachment) -> io::Result<()> {
        let cas = Cas::<_, Blake3>::with_filesystem(
            owner.metadata_path(),
            owner.store.filesystem().read_only(),
        )
        .map_err(error)?;
        if read_payload(&cas, self.digest, 4 * 1024 * 1024)? != self.bytes {
            return Err(invalid("retained native snapshot graph differs"));
        }
        Ok(())
    }
    pub(super) fn promote(&self, owner: &ProvisionedAttachment) -> io::Result<()> {
        let cas =
            Cas::<_, Blake3>::with_filesystem(owner.metadata_path(), owner.store.filesystem())
                .map_err(error)?;
        cas.promote(self.bytes.clone()).map_err(error)?;
        self.verify(owner)
    }
}
fn input(value: QualifiedDependencyInput) -> Json {
    Json::Array(vec![
        Json::Array(vec![
            Json::text(value.0.to_hex()),
            Json::text(value.1.to_hex()),
        ]),
        Json::text(value.2.to_hex()),
    ])
}
#[allow(clippy::too_many_arguments)]
pub(super) fn select(
    context: &PrivateContext<'_>,
    storage: &AttachmentStorage,
    work: &ProvisionedAttachment,
    guard: &WorkspaceInitializationGuard,
    owner: &VerifiedPrivateHistory,
    operation: RecordDigest,
    request: RecordDigest,
) -> io::Result<(NativeControlInput, RetainedGraph)> {
    let graph = context.graph(storage, work, operation, guard)?;
    let binding = context.binding(storage, work, guard)?;
    if graph.review_output() != (binding.work(), binding.installation(), operation) {
        return Err(invalid("native snapshot output belongs to another work"));
    }
    let material = RetainedGraph {
        digest: graph.digest(),
        bytes: graph.to_json().encode().into_bytes(),
    };
    let output = input(graph.review_output());
    let body = if let Some(record) = owner.policy().native_request(request) {
        let body = owner
            .policy()
            .review_snapshot_body(request)
            .ok_or_else(|| invalid("request is not a native review snapshot"))?;
        if body.get("output") != Some(&output)
            || body.get("graph") != Some(&Json::text(material.digest.to_hex()))
        {
            return Err(invalid(
                "native snapshot retry differs from exact output or graph",
            ));
        }
        material.verify(context.owner)?;
        owner
            .policy()
            .verify_review_graph(record.payload, &material.bytes)
            .map_err(error)?;
        body
    } else {
        let decisions = Json::Array(
            graph
                .review_inputs()
                .map(|i| {
                    let (revision, record) =
                        owner.policy().eligible_decision(i).ok_or_else(|| {
                            invalid("review input has no current eligible native decision")
                        })?;
                    Ok(Json::Array(vec![
                        input(i),
                        Json::Number(revision),
                        Json::text(record.to_hex()),
                    ]))
                })
                .collect::<io::Result<Vec<_>>>()?,
        );
        let validation = Json::object([
            ("schema", Json::text("mesh.native-review-validation/v1")),
            ("output", output.clone()),
            ("graph", Json::text(material.digest.to_hex())),
            ("decisions", decisions.clone()),
        ])
        .encode();
        Json::object([
            ("request", Json::text(request.to_hex())),
            ("revision", Json::Number(1)),
            ("output", output),
            ("graph", Json::text(material.digest.to_hex())),
            ("decisions", decisions),
            (
                "validation",
                Json::text(hash(validation.as_bytes()).to_hex()),
            ),
        ])
    };
    Ok((
        NativeControlInput {
            kind: DependencyKind::ReviewSnapshot,
            revision_field: "revision",
            body,
            prior: None,
        },
        material,
    ))
}
