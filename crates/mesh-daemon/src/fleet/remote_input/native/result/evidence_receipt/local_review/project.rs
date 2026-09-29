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
    ) -> Result<RemoteResultEvidenceReceipt, Error> {
        self.verify_complete(runtime)?;
        let state = runtime.state();
        let lane = state.lanes.get(&self.lane).ok_or_else(refused)?;
        if state.cancelled
            || lane.parent.is_some()
            || lane.source_project.as_deref() != Some(request.source.id())
            || lane.base != request.input.input()
            || lane.runs.last().is_none_or(|r| {
                matches!(
                    r.state,
                    crate::fleet::RunState::Stopping | crate::fleet::RunState::Cancelled
                )
            })
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
        let evidence = self.project_context(runtime, request)?;
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
                            if main.get("head").and_then(Json::as_text) != request.expected_main {
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
                            self.project_context(runtime, request).map_err(|_| {
                                io::Error::other("remote candidate context changed")
                            })?;
                            if runtime.state().revision != revision {
                                return Err(io::Error::other("remote candidate revision changed"));
                            }
                            source.with_fleet_input(
                                request.input.input(),
                                request.reviewers,
                                |_, main| {
                                    if main.get("head").and_then(Json::as_text)
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
                            if main.get("head").and_then(Json::as_text) != request.expected_main {
                                return Err(io::Error::other("remote candidate main changed"));
                            }
                            read(project, target, &snapshot, &origins, &candidate)
                        },
                    )
                },
            )
            .map_err(store_error)?;
        if self.project_context(runtime, request)?.digest() != evidence.digest()
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
        self.with_project_candidate(runtime, request, true, |_, _, _, _, candidate| {
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
}
