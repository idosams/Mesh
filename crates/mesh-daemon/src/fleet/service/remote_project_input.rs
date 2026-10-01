//! Retained original-project ancestry for exporting an exact local saved result.
use super::*;
use crate::project_attachment::ProvisionedAttachment;
use mesh_cas::Digest32;

/// Native-only saved input and complete ancestry, with every allocation held until drop.
/// This is historical evidence, not dependency eligibility, dispatch or approval authority.
/// A consumer must revalidate this handle around any operation that can yield.
pub struct RemoteProjectInput<'a> {
    service: &'a FleetService,
    objective: String,
    project: &'a ProvisionedAttachment,
    selection: SavedReviewSelection,
    lineage: Vec<super::super::project_mapping::LineageStep>,
    histories: Vec<LaneHistory>,
    original: super::super::RemoteInputSource,
    input: super::super::RemoteInputSource,
    origins: BTreeMap<String, String>,
}
impl RemoteProjectInput<'_> {
    /// Recheck exact recorded selection, ancestry and all retained directory identities.
    /// Cancellation does not erase historical evidence; this method grants no new work authority.
    pub fn verify(&self) -> Result<(), Unavailable> {
        let mut inner = self.service.lock()?;
        inner.runtime.refresh().map_err(runtime_error)?;
        self.verify_runtime(&inner.runtime)
    }
    pub(in crate::fleet) fn verify_runtime(&self, runtime: &Runtime) -> Result<(), Unavailable> {
        if runtime.objective() != self.objective {
            return Err(refusal("fleet-project-objective-mismatch"));
        }
        let state = runtime.state();
        saved_review_binding_from_state(state, &self.selection)?;
        if super::super::project_mapping::lineage(
            state,
            &self.selection.lane,
            self.selection.version,
            self.project.id(),
        )
        .map_err(|_| refusal("fleet-project-lineage-unavailable"))?
            != self.lineage
        {
            return Err(refusal("fleet-project-lineage-changed"));
        }
        for history in &self.histories {
            history.verify()?;
        }
        self.original
            .verify_roots()
            .and_then(|_| self.input.verify_roots())
            .map_err(|_| refusal("fleet-project-input-roots-changed"))?;
        Ok(())
    }
    pub(in crate::fleet) fn verify_remote_child(
        &self,
        runtime: &Runtime,
        remote_lane: &str,
        project: &ProvisionedAttachment,
        input: &super::super::RemoteInputManifest,
        eligible: bool,
    ) -> Result<(), Unavailable> {
        self.verify_runtime(runtime)?;
        let state = runtime.state();
        let lane = state
            .lanes
            .get(remote_lane)
            .ok_or_else(|| refusal("fleet-remote-child-missing"))?;
        if project.id() != self.project.id()
            || lane.parent.as_deref() != Some(&self.selection.lane)
            || lane.source_project.as_deref() != Some(project.id())
            || lane.base != self.selection.version
            || input != self.input.manifest()
        {
            return Err(refusal("fleet-remote-child-input-mismatch"));
        }
        if eligible
            && (state.cancelled
                || self.lineage.iter().any(|step| {
                    state
                        .lanes
                        .get(&step.lane)
                        .and_then(|lane| lane.runs.last())
                        .is_none_or(|run| {
                            matches!(run.state, RunState::Stopping | RunState::Cancelled)
                        })
                }))
        {
            return Err(refusal("fleet-remote-ancestor-ineligible"));
        }
        Ok(())
    }
    pub(in crate::fleet) fn native_manifest(&self) -> &super::super::RemoteInputManifest {
        self.input.manifest()
    }
    pub(in crate::fleet) fn root_version(&self) -> RecordDigest {
        self.original.manifest().input()
    }
    pub(in crate::fleet) fn provenance(&self) -> Json {
        Json::object([
            ("schema", Json::text("mesh.remote-project-ancestry/v1")),
            ("selection", self.selection.to_json()),
            (
                "input_manifest",
                Json::text(self.input.manifest().bundle().to_string()),
            ),
            (
                "original_version",
                Json::text(self.root_version().to_string()),
            ),
            (
                "steps",
                Json::Array(
                    self.lineage
                        .iter()
                        .map(|step| {
                            Json::object([
                                ("lane", Json::text(&step.lane)),
                                (
                                    "source_version",
                                    Json::text(step.binding.source_version.to_string()),
                                ),
                                (
                                    "starting_version",
                                    step.binding
                                        .starting_version()
                                        .map_or(Json::Null, |v| Json::text(v.to_string())),
                                ),
                                ("result_version", Json::text(step.result.to_string())),
                            ])
                        })
                        .collect(),
                ),
            ),
        ])
    }
    pub(in crate::fleet) fn remote_origins(
        &self,
        correspondence: &super::super::RemoteResultCorrespondence,
        result: &super::super::RemoteInputManifest,
        local: &crate::workspace::HistoricalWorkspacePreview,
    ) -> std::io::Result<BTreeMap<String, String>> {
        let leaf = self
            .histories
            .last()
            .ok_or_else(|| std::io::Error::other("missing ancestry"))?;
        let parent = leaf
            .open
            .historical_workspace_preview(self.selection.version)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let immediate =
            correspondence.project_origins(self.input.manifest(), &parent, result, local)?;
        Ok(immediate
            .into_iter()
            .filter_map(|(local, upstream)| {
                self.origins
                    .get(&upstream)
                    .map(|original| (local, original.clone()))
            })
            .collect())
    }
    /// The exact exported input manifest, after native ancestry revalidation.
    pub fn manifest(&self) -> Result<&super::super::RemoteInputManifest, Unavailable> {
        self.verify()?;
        Ok(self.input.manifest())
    }
    /// Original-project predecessor, distinct from the delegated input operation.
    pub fn original_version(&self) -> Result<RecordDigest, Unavailable> {
        self.verify()?;
        Ok(self.original.manifest().input())
    }
    /// Complete saved-input object to original-project object correspondence. Added objects are
    /// absent; equal filenames never manufacture an original identity.
    pub fn original_objects(&self) -> Result<&BTreeMap<String, String>, Unavailable> {
        self.verify()?;
        Ok(&self.origins)
    }
    /// Read only authenticated saved bytes; later working edits never replace this input.
    pub fn read_chunk(&self, digest: Digest32) -> Result<Vec<u8>, Unavailable> {
        self.verify()?;
        let bytes = self
            .input
            .read_chunk(digest)
            .map_err(|_| refusal("fleet-project-input-content-unavailable"))?;
        self.verify()?;
        Ok(bytes)
    }
}
impl FleetService {
    /// Retain a complete native local ancestry together with exact saved export bytes.
    /// No allocation, worker adoption, signing or current dependency authorization occurs.
    pub fn prepare_remote_project_input<'a>(
        &'a self,
        selection: &SavedReviewSelection,
        project: &'a ProvisionedAttachment,
        trusted: &TrustedReviewers,
    ) -> Result<RemoteProjectInput<'a>, Unavailable> {
        let state = self.native_state()?;
        saved_review_binding_from_state(&state, selection)?;
        let lineage = super::super::project_mapping::lineage(
            &state,
            &selection.lane,
            selection.version,
            project.id(),
        )
        .map_err(|_| refusal("fleet-project-lineage-unavailable"))?;
        let histories = lineage
            .iter()
            .map(|step| self.allocator.reopen_history(&step.lane, &step.binding))
            .collect::<Result<Vec<_>, _>>()?;
        let root = lineage
            .first()
            .ok_or_else(|| refusal("fleet-project-lineage-unavailable"))?;
        let leaf = histories
            .last()
            .ok_or_else(|| refusal("fleet-project-lineage-unavailable"))?;
        leaf.open
            .review(&selection.bundle)
            .filter(|review| review.subject_operation == selection.version)
            .ok_or_else(|| refusal("fleet-review-not-recorded"))?;
        let original = project
            .prepare_remote_input(&root.binding.source_version.to_string())
            .map_err(|_| refusal("fleet-project-input-unavailable"))?;
        let origins = project
            .with_fleet_input(root.binding.source_version, trusted, |open, _| {
                let preview = |open: &crate::workspace::OpenWorkspace, version| {
                    open.historical_workspace_preview(version)
                        .map_err(|e| std::io::Error::other(e.to_string()))
                };
                let mut snapshots = Vec::new();
                for (step, history) in lineage.iter().zip(&histories) {
                    history
                        .verify()
                        .map_err(|_| std::io::Error::other("lineage history changed"))?;
                    snapshots.push((
                        preview(
                            &history.open,
                            step.binding
                                .starting_version()
                                .ok_or_else(|| std::io::Error::other("unbound input"))?,
                        )?,
                        preview(&history.open, step.result)?,
                    ));
                }
                super::super::project_mapping::import_correspondence(
                    preview(open, root.binding.source_version)?,
                    snapshots,
                )
            })
            .map_err(|_| refusal("fleet-project-mapping-unavailable"))?;
        let input = leaf
            .open
            .remote_input_source(selection.version)
            .and_then(|input| input.protecting_allocation(leaf.parents.clone(), leaf.allocation))
            .map_err(|_| refusal("fleet-project-input-unavailable"))?;
        let result = RemoteProjectInput {
            service: self,
            objective: self.objective()?,
            project,
            selection: selection.clone(),
            lineage,
            histories,
            original,
            input,
            origins,
        };
        result.verify()?;
        Ok(result)
    }
}
impl FleetHistory {
    /// Reopen exact original-project ancestry without acquiring execution ownership.
    pub fn prepare_remote_project_input<'a>(
        &'a self,
        selection: &SavedReviewSelection,
        project: &'a ProvisionedAttachment,
        trusted: &TrustedReviewers,
    ) -> Result<RemoteProjectInput<'a>, Unavailable> {
        self.0
            .prepare_remote_project_input(selection, project, trusted)
    }
}

#[cfg(target_os = "macos")]
impl FleetHistory {
    pub(super) fn with_retained_project_ancestry<T>(
        &self,
        request: &super::super::RetainedRemoteProjectRequest<'_>,
        action: impl FnOnce(
            &mut Runtime,
            &super::super::RetainedRemoteProjectRequest<'_>,
        ) -> Result<T, super::super::Error>,
    ) -> Result<T, Unavailable> {
        // Read selection while locked, but reopen ancestry outside the runtime mutex.
        let parent = {
            let mut inner = self.0.lock()?;
            inner.runtime.refresh().map_err(runtime_error)?;
            let receipt = inner
                .runtime
                .retained_remote_local_review(request.offer)
                .map_err(runtime_error)?
                .filter(|receipt| receipt.digest() == request.correlation)
                .ok_or_else(|| refusal("fleet-remote-correlation-mismatch"))?;
            let selected = receipt.selection();
            let lane_id = selected
                .get("lane")
                .and_then(Json::as_text)
                .ok_or_else(|| refusal("fleet-remote-lane-missing"))?;
            let state = inner.runtime.state();
            let lane = state
                .lanes
                .get(lane_id)
                .ok_or_else(|| refusal("fleet-remote-lane-missing"))?;
            match lane.parent.as_deref() {
                None => None,
                Some(parent) => {
                    let mut matches = state.checkpoints.iter().filter_map(|(id, cp)| {
                        (cp.lane == parent
                            && cp
                                .result
                                .as_ref()
                                .is_some_and(|r| r.complete && r.version == lane.base))
                        .then_some(cp.review)
                        .flatten()
                        .map(|bundle| SavedReviewSelection {
                            lane: parent.into(),
                            checkpoint: id.clone(),
                            version: lane.base,
                            bundle,
                        })
                    });
                    let selection = matches
                        .next()
                        .ok_or_else(|| refusal("fleet-remote-parent-review-missing"))?;
                    if matches.next().is_some() {
                        return Err(refusal("fleet-remote-parent-review-ambiguous"));
                    }
                    Some(selection)
                }
            }
        };
        let ancestry = parent
            .as_ref()
            .map(|selection| {
                self.0
                    .prepare_remote_project_input(selection, request.source, request.reviewers)
            })
            .transpose()?;
        let native = super::super::RetainedRemoteProjectRequest {
            offer: request.offer,
            correlation: request.correlation,
            source: request.source,
            reviewers: request.reviewers,
            request: request.request,
            expected_main: request.expected_main,
            ancestry: ancestry.as_ref(),
        };
        let mut inner = self.0.lock()?;
        inner.runtime.refresh().map_err(runtime_error)?;
        action(&mut inner.runtime, &native).map_err(runtime_error)
    }
}
