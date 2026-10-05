//! Read-only reconstruction of owning-project native publication authority.
use super::{dependency_transaction::read_payload, invalid, AttachmentStorage};
use crate::{ipc::Json, workspace::NativePrivateReviewHistory, TrustedReviewers};
use mesh_cas::{Blake3, Cas};
use std::io;
fn error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

impl AttachmentStorage {
    /// Verify every owning-root publication in order using configured human trust. This
    /// read projection neither opens ordinary workspace authority nor permits a mutation.
    /// Histories involving consumed works require the complete-input replay path.
    pub fn inspect_root_publication_history(
        &self,
        registration: &str,
        trusted: &TrustedReviewers,
    ) -> io::Result<Json> {
        self.with_root_publication_history(registration, trusted, |history, binding, _, _| {
            Ok(Json::object([
                (
                    "schema",
                    Json::text("mesh.native-root-publication-history/v1"),
                ),
                ("work", Json::text(binding.work().to_hex())),
                (
                    "publication",
                    history.verified_publication().map_or(Json::Null, |claim| {
                        Json::object([
                            ("head", Json::text(claim.result.to_hex())),
                            (
                                "operation",
                                Json::text(claim.review.evidence().output().2.to_hex()),
                            ),
                            ("revision", Json::Number(claim.revision)),
                            ("receipt", Json::text(claim.receipt.to_hex())),
                        ])
                    }),
                ),
            ]))
        })
    }

    /// Reconstruct a prospective root review against verified main, without saving a review
    /// or requesting a signature. The snapshot must already exist in this owning history.
    pub fn inspect_root_review_candidate(
        &self,
        registration: &str,
        snapshot: mesh_store::RecordDigest,
        trusted: &TrustedReviewers,
    ) -> io::Result<Json> {
        self.with_root_publication_history(
            registration,
            trusted,
            |history, _, evidence, workspace| {
                let review = evidence
                    .policy()
                    .review_evidence(snapshot)
                    .ok_or_else(|| invalid("root review snapshot is unavailable"))?;
                let graph = super::dependency_closure::root_publication_graph(
                    review.output(),
                    workspace,
                    history,
                )?;
                if evidence.policy().review_graph(snapshot) != Some(graph.digest()) {
                    return Err(invalid(
                        "root review snapshot differs from verified content",
                    ));
                }
                let (canonical, bundle) = history.current_review_bundle(&review).map_err(error)?;
                Ok(Json::object([
                    ("schema", Json::text("mesh.native-root-review-candidate/v1")),
                    (
                        "canonical",
                        Json::text(
                            mesh_store::RecordDigest::from_bytes(*canonical.as_bytes()).to_hex(),
                        ),
                    ),
                    ("bundle", Json::text(bundle.to_hex())),
                    ("snapshot", Json::text(snapshot.to_hex())),
                ]))
            },
        )
    }

    /// Inspect an exact private saved input after independently replaying owning-root main.
    /// The synchronous bounded callback must not wait for a provider, network or person.
    /// This is a historical read, not permission to consume, launch work, or publish. Extracted
    /// bytes carry no continuing authority, and callback side effects cannot be rolled back.
    pub fn with_root_saved_input<T>(
        &self,
        registration: &str,
        operation: mesh_store::RecordDigest,
        trusted: &TrustedReviewers,
        read: impl FnOnce(super::NativeGrantedInput<'_>) -> io::Result<T>,
    ) -> io::Result<T> {
        self.with_root_publication_history(registration, trusted, |history, _, _, _| {
            history.with_saved_input(operation, read)
        })
    }

    fn with_root_publication_history<T>(
        &self,
        registration: &str,
        trusted: &TrustedReviewers,
        inspect: impl FnOnce(
            &NativePrivateReviewHistory<'_>,
            &super::NativeDependencyWorkBinding,
            &super::VerifiedPrivateHistory,
            mesh_operations::WorkspaceId,
        ) -> io::Result<T>,
    ) -> io::Result<T> {
        let owner = self.reopen(registration)?;
        let prepared = self.prepare_dependency_work(&owner, &owner)?;
        let guard = crate::workspace_custody::lock_workspace_initialization_set(&prepared.roots)
            .map_err(error)?;
        let (configuration, evidence) = owner
            .project()
            .read_publication_private_history(owner.metadata_path(), &owner.store)?;
        let binding = self.validate_publication_root(&prepared, &guard, &evidence)?;
        let workspace = mesh_operations::WorkspaceId::from_bytes(super::history::short_id(
            configuration.as_bytes(),
        ));
        let mut history = NativePrivateReviewHistory::open(
            owner.metadata_path(),
            owner.store.clone(),
            &evidence,
            (&owner.store, &evidence),
            workspace,
            &binding,
            &guard,
        )
        .map_err(error)?;
        let cas = Cas::<_, Blake3>::with_filesystem(
            owner.metadata_path(),
            owner.store.filesystem().read_only(),
        )
        .map_err(error)?;
        for claim in evidence.policy().publication_claims_in_order() {
            let output = claim.review.evidence().output();
            if (output.0, output.1) != (binding.work(), binding.installation()) {
                return Err(invalid(
                    "publication requires complete consumed work context",
                ));
            }
            let graph =
                super::dependency_closure::root_publication_graph(output, workspace, &history)?;
            let bytes = read_payload(&cas, claim.receipt, 65_536)?;
            history
                .replay_publication(claim, &graph, &bytes, trusted)
                .map_err(error)?;
        }
        let result = inspect(&history, &binding, &evidence, workspace)?;
        let refreshed = owner
            .project()
            .read_publication_private_history(owner.metadata_path(), &owner.store)?;
        if refreshed != (configuration, evidence.clone())
            || self.validate_publication_root(&prepared, &guard, &evidence)? != binding
        {
            return Err(invalid("publication replay native context changed"));
        }
        guard.ensure_current().map_err(error)?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
