//! Read-only registered projections. Present eligibility never rewrites retained review evidence.
use super::super::dependency_catalog_read::VerifiedCatalogDependencyRead;
use super::*;
use crate::dependency_policy::NativeReviewBinding;
use crate::root_authority::PinnedRootFs;
use mesh_cas::{Blake3, Cas};

fn exact_binding(
    context: &VerifiedCatalogDependencyRead<'_>,
    record: RecordDigest,
) -> io::Result<NativeReviewBinding> {
    let binding = context
        .owner_proof
        .policy()
        .bound_review(record)
        .ok_or_else(|| invalid("native saved review is missing"))?;
    let selected = context.work_binding;
    let output = binding.evidence().output();
    if (output.0, output.1) != (selected.work(), selected.installation()) {
        return Err(invalid("native saved review belongs to another work"));
    }
    Ok(binding)
}
fn projection(
    context: &VerifiedCatalogDependencyRead<'_>,
    binding: &NativeReviewBinding,
) -> io::Result<Json> {
    let policy = context.owner_proof.policy();
    let snapshot = binding.evidence().snapshot();
    let body = policy
        .review_snapshot_record_body(snapshot)
        .ok_or_else(|| invalid("native review snapshot is missing"))?;
    let graph = policy
        .review_graph(snapshot)
        .ok_or_else(|| invalid("native review graph is missing"))?;
    let cas = Cas::<PinnedRootFs, Blake3>::with_filesystem(
        context.owner.metadata_path(),
        context.owner.store.filesystem(),
    )
    .map_err(error)?;
    let bytes = super::super::dependency_transaction::read_payload(&cas, graph, 4 * 1024 * 1024)?;
    policy
        .verify_review_graph(snapshot, &bytes)
        .map_err(error)?;
    let graph_json = Json::parse(std::str::from_utf8(&bytes).map_err(error)?).map_err(error)?;
    Ok(Json::object([
        ("record", Json::text(binding.record().to_hex())),
        ("snapshot", Json::text(snapshot.to_hex())),
        (
            "canonical",
            Json::text(RecordDigest::from_bytes(*binding.canonical().as_bytes()).to_hex()),
        ),
        (
            "historical_validation",
            Json::text(binding.evidence().validation().to_hex()),
        ),
        (
            "historical_decisions",
            body.get("decisions")
                .cloned()
                .ok_or_else(|| invalid("native review decisions are missing"))?,
        ),
        ("historical_graph", graph_json),
        (
            "inputs_currently_eligible",
            Json::Bool(policy.review_inputs_eligible(snapshot)),
        ),
        (
            "historical_decisions_current",
            Json::Bool(policy.review_decisions_current(snapshot)),
        ),
        ("approval_authority", Json::Bool(false)),
        (
            "review",
            context
                .history
                .native_recorded_review_item(binding)
                .map_err(error)?,
        ),
    ]))
}
impl AttachmentStorage {
    /// Reopen an exact native saved review under complete owner/input custody.
    pub fn saved_dependency_review(&self, work_id: &str, record: RecordDigest) -> io::Result<Json> {
        self.with_registered_dependency_context(work_id, |context| {
            projection(context, &exact_binding(context, record)?)
        })
    }
    /// List a bounded page of retained review bindings for this exact registered work.
    pub fn saved_dependency_reviews(
        &self,
        work_id: &str,
        after: Option<RecordDigest>,
    ) -> io::Result<Json> {
        self.with_registered_dependency_context(work_id, |context| {
            let selected = context.work_binding;
            let bindings = context
                .owner_proof
                .policy()
                .bound_reviews()
                .filter(|binding| {
                    let output = binding.evidence().output();
                    (output.0, output.1) == (selected.work(), selected.installation())
                        && after.is_none_or(|after| binding.record() > after)
                })
                .collect::<Vec<_>>();
            let not_listed = bindings.len().saturating_sub(64);
            let items = bindings
                .iter()
                .take(64)
                .map(|binding| {
                    Ok(Json::object([
                        ("record", Json::text(binding.record().to_hex())),
                        (
                            "snapshot",
                            Json::text(binding.evidence().snapshot().to_hex()),
                        ),
                        (
                            "review",
                            context
                                .history
                                .native_recorded_review_item(binding)
                                .map_err(error)?,
                        ),
                        (
                            "inputs_currently_eligible",
                            Json::Bool(
                                context
                                    .owner_proof
                                    .policy()
                                    .review_inputs_eligible(binding.evidence().snapshot()),
                            ),
                        ),
                        (
                            "historical_decisions_current",
                            Json::Bool(
                                context
                                    .owner_proof
                                    .policy()
                                    .review_decisions_current(binding.evidence().snapshot()),
                            ),
                        ),
                    ]))
                })
                .collect::<io::Result<Vec<_>>>()?;
            Ok(Json::object([
                ("reviews", Json::Array(items)),
                ("not_listed", Json::Number(not_listed as u64)),
            ]))
        })
    }
    /// Read one exact reviewed file side. The selector is an object identity, never an OS path.
    pub fn saved_dependency_review_file(
        &self,
        work_id: &str,
        record: RecordDigest,
        object: mesh_materializer::ObjectId,
        after: bool,
    ) -> io::Result<Vec<u8>> {
        self.with_registered_dependency_context(work_id, |context| {
            let binding = exact_binding(context, record)?;
            let side = if after {
                crate::workspace::ReviewArtifactSide::After
            } else {
                crate::workspace::ReviewArtifactSide::Before
            };
            context
                .history
                .native_review_artifact(&binding, object, side)
                .map(|file| file.bytes)
                .map_err(error)
        })
    }
    /// Construct the exact human-review context and native summary, without signing or publication.
    pub fn saved_dependency_review_preview(
        &self,
        work_id: &str,
        record: RecordDigest,
    ) -> io::Result<(mesh_approval::HumanApprovalContext, String)> {
        self.with_registered_dependency_context(work_id, |context| {
            let binding = exact_binding(context, record)?;
            context
                .history
                .native_human_approval_preview(&binding)
                .map(|(context, summary, _, _, _)| (context, summary))
                .map_err(error)
        })
    }
}
