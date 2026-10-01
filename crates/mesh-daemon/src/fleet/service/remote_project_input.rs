//! Retained original-project ancestry for exporting an exact local saved result.
use super::*;
use crate::project_attachment::ProvisionedAttachment;
use mesh_cas::Digest32;

/// Native-only saved input and complete ancestry, with every allocation held until drop.
/// This is historical evidence, not dependency eligibility, dispatch or approval authority.
/// A consumer must revalidate this handle around any operation that can yield.
pub struct RemoteProjectInput<'a> {
    service: &'a FleetService,
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
        let state = self.service.native_state()?;
        saved_review_binding_from_state(&state, &self.selection)?;
        if super::super::project_mapping::lineage(
            &state,
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
