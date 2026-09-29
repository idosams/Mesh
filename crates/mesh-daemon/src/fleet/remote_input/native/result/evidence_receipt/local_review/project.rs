//! Stage and compile authenticated remote results against their exact attached-project input.
use super::*;
use crate::project_attachment::{CandidateAdmission, ProvisionedAttachment};
use crate::workspace::{HistoricalWorkspacePreview, OpenWorkspace};
use crate::TrustedReviewers;
use std::collections::BTreeMap;

/// Native inputs for exact remote candidate preparation; no filesystem path or approval grant.
/// This direct-input phase refuses delegated inputs until their complete lineage can be verified.
pub struct RemoteProjectCandidateRequest<'a> {
    /// Exact original-project manifest admitted for the remote assignment.
    pub input: &'a RemoteInputManifest,
    /// Durably recorded remote-to-local review correlation.
    pub correlation: &'a RemoteLocalReviewReceipt,
    /// Native admitted original project, never a peer-selected path.
    pub source: &'a ProvisionedAttachment,
    /// Native trust configuration for retained history verification.
    pub reviewers: &'a TrustedReviewers,
    /// Stable 32-character lowercase hexadecimal candidate retry identity.
    pub request: &'a str,
    /// Exact observed project main head, or no main yet.
    pub expected_main: Option<&'a str>,
}
impl NativeRemoteResultReceiver<'_> {
    fn project_context(
        &self,
        runtime: &mut Runtime,
        request: &RemoteProjectCandidateRequest<'_>,
        eligible: bool,
    ) -> Result<RemoteResultEvidenceReceipt, Error> {
        self.verify_complete(runtime)?;
        let state = runtime.state();
        let lane = state.lanes.get(&self.lane).ok_or_else(refused)?;
        if lane.parent.is_some()
            || lane.source_project.as_deref() != Some(request.source.id())
            || lane.base != request.input.input()
            || (eligible
                && (state.cancelled
                    || lane.runs.last().is_none_or(|r| {
                        matches!(
                            r.state,
                            crate::fleet::RunState::Stopping | crate::fleet::RunState::Cancelled
                        )
                    })))
        {
            return Err(refused());
        }
        let content = self.verify_content_receipt(runtime)?.digest();
        let evidence = self
            .load_evidence(runtime, request.input, content)?
            .ok_or_else(refused)?;
        let receipt = request.correlation;
        if runtime
            .retained_remote_local_review(hash(&self.offer))?
            .as_ref()
            != Some(receipt)
            || receipt.evidence != evidence.digest()
            || receipt.content != content
            || receipt.lane != self.lane
            || receipt.run != self.run
        {
            return Err(refused());
        }
        Ok(evidence)
    }

    fn with_project_candidate<T>(
        &self,
        runtime: &mut Runtime,
        request: &RemoteProjectCandidateRequest<'_>,
        create: bool,
        eligible: bool,
        read: impl FnOnce(
            &OpenWorkspace,
            &OpenWorkspace,
            &HistoricalWorkspacePreview,
            &BTreeMap<String, String>,
            &Json,
        ) -> io::Result<T>,
    ) -> Result<T, Error> {
        if request.expected_main.is_some_and(|v| {
            RecordDigest::parse_hex(v)
                .ok()
                .is_none_or(|d| d.to_string() != v)
        }) {
            return Err(refused());
        }
        let evidence = self.project_context(runtime, request, eligible)?;
        let revision = runtime.state().revision;
        let receipt = request.correlation;
        let source = request.source;
        let provenance = Json::object([
            ("schema", Json::text("mesh.remote-project-candidate/v1")),
            ("objective", Json::text(runtime.objective())),
            ("selection", receipt.selection()),
            ("evidence", Json::text(evidence.digest().to_string())),
            (
                "correspondence",
                Json::text(evidence.evidence().correspondence().digest().to_string()),
            ),
            ("source_project", Json::text(source.id())),
            (
                "source_version",
                Json::text(request.input.input().to_string()),
            ),
            (
                "expected_main",
                request.expected_main.map_or(Json::Null, Json::text),
            ),
            ("attribution", Json::text("authenticated-remote-result")),
            ("approval_authority", Json::Bool(false)),
        ]);
        let result = receipt
            .with_review(
                self.destination,
                &self.manifest,
                request.reviewers,
                |target| {
                    let snapshot = target
                        .historical_workspace_preview(receipt.version)
                        .map_err(|error| io::Error::other(error.to_string()))?;
                    // Validate the original history before any candidate allocation is made.
                    let origins = source.with_fleet_input(
                        request.input.input(),
                        request.reviewers,
                        |project, main| {
                            if eligible
                                && main.get("head").and_then(Json::as_text) != request.expected_main
                            {
                                return Err(io::Error::other("remote candidate main changed"));
                            }
                            let original = project
                                .historical_workspace_preview(request.input.input())
                                .map_err(|error| io::Error::other(error.to_string()))?;
                            evidence.evidence().correspondence().project_origins(
                                request.input,
                                &original,
                                &self.manifest,
                                &snapshot,
                            )
                        },
                    )?;
                    let candidate = source.stage_fleet_candidate(
                        request.request,
                        &provenance,
                        target,
                        &snapshot,
                        if create {
                            CandidateAdmission::Stage { main_matches: true }
                        } else {
                            CandidateAdmission::Inspect
                        },
                        || {
                            self.project_context(runtime, request, eligible)
                                .map_err(|_| {
                                    io::Error::other("remote candidate context changed")
                                })?;
                            if runtime.state().revision != revision {
                                return Err(io::Error::other("remote candidate revision changed"));
                            }
                            source.with_fleet_input(
                                request.input.input(),
                                request.reviewers,
                                |_, main| {
                                    if eligible
                                        && main.get("head").and_then(Json::as_text)
                                            != request.expected_main
                                    {
                                        return Err(io::Error::other(
                                            "remote candidate main changed",
                                        ));
                                    }
                                    Ok(())
                                },
                            )
                        },
                    )?;
                    source.with_fleet_input(
                        request.input.input(),
                        request.reviewers,
                        |project, main| {
                            if eligible
                                && main.get("head").and_then(Json::as_text) != request.expected_main
                            {
                                return Err(io::Error::other("remote candidate main changed"));
                            }
                            read(project, target, &snapshot, &origins, &candidate)
                        },
                    )
                },
            )
            .map_err(store_error)?;
        if self.project_context(runtime, request, eligible)?.digest() != evidence.digest()
            || runtime.state().revision != revision
        {
            return Err(refused());
        }
        Ok(result)
    }

    /// Copy verified content to a private original-project candidate. Original files, capture
    /// history and protected main remain unchanged. Replays require identical provenance/content.
    pub fn stage_project_candidate(
        &self,
        runtime: &mut Runtime,
        request: &RemoteProjectCandidateRequest<'_>,
    ) -> Result<Json, Error> {
        self.with_project_candidate(runtime, request, true, true, |_, _, _, _, candidate| {
            Ok(candidate.clone())
        })
    }

    /// Compile an existing exact candidate to original-project operations. This does not sign,
    /// commit, approve or write back; those remain separate native operations.
    pub fn prepare_project_candidate_import(
        &self,
        runtime: &mut Runtime,
        request: &RemoteProjectCandidateRequest<'_>,
        actor: PublicKey,
    ) -> Result<crate::fleet::PreparedProjectCandidateImport, Error> {
        self.with_project_candidate(
            runtime,
            request,
            false,
            true,
            |project, target, snapshot, origins, candidate| {
                crate::fleet::project_import::compile(
                    project,
                    request.input.input(),
                    target,
                    snapshot,
                    origins,
                    candidate,
                    actor,
                )
            },
        )
    }
    fn project_candidate_snapshot(
        &self,
        runtime: &mut Runtime,
        request: &RemoteProjectCandidateRequest<'_>,
        eligible: bool,
    ) -> Result<(Json, HistoricalWorkspacePreview), Error> {
        self.with_project_candidate(
            runtime,
            request,
            false,
            eligible,
            |_, _, snapshot, _, candidate| Ok((candidate.clone(), snapshot.clone())),
        )
    }

    /// Inspect a durable import outcome without signing, appending or repairing it. Cancellation
    /// and later main advancement do not erase an already retained outcome.
    pub fn inspect_project_candidate_import(
        &self,
        runtime: &mut Runtime,
        request: &RemoteProjectCandidateRequest<'_>,
        actor: PublicKey,
    ) -> Result<Json, Error> {
        let (candidate, snapshot) = self.project_candidate_snapshot(runtime, request, false)?;
        let outcome = request
            .source
            .inspect_fleet_import(
                request.request,
                &candidate,
                &snapshot,
                actor,
                request.reviewers,
            )
            .map_err(store_error)?
            .unwrap_or(Json::Null);
        if self.project_candidate_snapshot(runtime, request, false)?.0 != candidate {
            return Err(refused());
        }
        Ok(outcome)
    }

    /// Append a signed, provenance-bound private project version. Exact retries recover native
    /// journal truth without signing twice. This never advances main or writes original files.
    pub fn import_project_candidate(
        &self,
        runtime: &mut Runtime,
        request: &RemoteProjectCandidateRequest<'_>,
        signer: &dyn crate::fleet::CandidateImportSigner,
    ) -> Result<Json, Error> {
        self.project_context(runtime, request, true)?;
        let existing =
            self.inspect_project_candidate_import(runtime, request, signer.public_key())?;
        if existing.get("state") == Some(&Json::text("imported")) {
            return Ok(existing);
        }
        let plan = self.prepare_project_candidate_import(runtime, request, signer.public_key())?;
        let (candidate, _) = self.project_candidate_snapshot(runtime, request, true)?;
        let revision = runtime.state().revision;
        // Pin verified remote storage before entering the original project write custody.
        let retained_source = request
            .correlation
            .reopen(self.destination, &self.manifest, request.reviewers)
            .map_err(store_error)?;
        let outcome = request
            .source
            .commit_fleet_import_with(
                request.request,
                &candidate,
                plan,
                signer,
                request.reviewers,
                || {
                    // Native signing may yield. Refuse cancellation/context changes before journal
                    // append; an already durable intent is retained for explicit inspection/recovery.
                    self.project_context(runtime, request, true)
                        .map_err(|_| io::Error::other("remote import context changed"))?;
                    if runtime.state().revision != revision {
                        return Err(io::Error::other("remote import revision changed"));
                    }
                    retained_source.verify_roots()?;
                    Ok(())
                },
            )
            .map_err(store_error)?;
        if self.project_candidate_snapshot(runtime, request, true)?.0 != candidate {
            return Err(refused());
        }
        Ok(outcome)
    }

    /// Record or inspect the actual imported project review against its fixed historical main.
    /// The review grants no human approval or original-folder write-back authority.
    pub fn review_imported_project_candidate(
        &self,
        runtime: &mut Runtime,
        request: &RemoteProjectCandidateRequest<'_>,
        create: bool,
    ) -> Result<Json, Error> {
        let (candidate, snapshot) = self.project_candidate_snapshot(runtime, request, create)?;
        let review = request
            .source
            .review_fleet_import(
                request.request,
                &candidate,
                &snapshot,
                request.reviewers,
                create,
            )
            .map_err(store_error)?;
        if self.project_candidate_snapshot(runtime, request, create)?.0 != candidate {
            return Err(refused());
        }
        Ok(review)
    }
}

mod retained;
pub use retained::RetainedRemoteProjectRequest;
