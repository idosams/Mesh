//! Private collection preparation. The caller must retain native writer custody through execute.
use super::*;
use mesh_store::{CollectionPlan, Reachability, RetainedRoots};

pub(crate) struct OrphanCollection {
    store: Cas<crate::root_authority::PinnedRootFs, mesh_cas::Blake3>,
    pins: Vec<crate::root_authority::PinnedWorkspaceRoot>,
    doomed: Vec<CasDigest>,
    named: BTreeSet<RecordDigest>,
}
impl OpenWorkspace {
    pub(crate) fn prepare_orphan_collection(&self) -> Result<OrphanCollection, String> {
        self.ensure_physical_root().map_err(|e| e.to_string())?;
        if self.tail.is_fragment() || !self.names_answered || !self.conditions.is_empty() {
            return Err("workspace history or recovery is incomplete".into());
        }
        let roots =
            RetainedRoots::conservative(&self.record_index, mesh_store::RetentionPolicy::default());
        let reachable =
            Reachability::compute(&self.record_index, &roots).map_err(|e| e.to_string())?;
        if reachable.dangling_parents().next().is_some() {
            return Err("workspace history has missing parents".into());
        }
        let candidates = self
            .payload_store
            .journal()
            .candidates()
            .map_err(|e| e.to_string())?;
        let plan = CollectionPlan::compute(
            &self.record_index,
            &roots,
            &reachable,
            candidates
                .iter()
                .map(|id| RecordDigest::from_bytes(*id.as_bytes())),
        )
        .map_err(|e| e.to_string())?;
        // Independent veto from every recorded payload/manifest/chunk, not from the plan.
        let named = self.record_index.named_content();
        let store = Cas::with_filesystem(
            self.storage_root.as_path().to_path_buf(),
            self.storage_pinned_root.filesystem(),
        )
        .map_err(|e| e.to_string())?;
        Ok(OrphanCollection {
            store,
            pins: [
                Some(self.pinned_root.clone()),
                Some(self.storage_pinned_root.clone()),
                self.storage_namespace.clone(),
            ]
            .into_iter()
            .flatten()
            .collect(),
            doomed: plan
                .doomed_digests()
                .into_iter()
                .take(256)
                .map(CasDigest::from_bytes)
                .collect(),
            named,
        })
    }
}
impl OrphanCollection {
    pub(crate) fn execute(
        self,
        mode: mesh_cas::CollectionMode,
    ) -> Result<mesh_cas::Collected, String> {
        for pin in &self.pins {
            pin.ensure_namespace_identity().map_err(|e| e.to_string())?;
        }
        let report = self
            .store
            .collect(
                &self.doomed,
                &|id: &CasDigest| {
                    self.named
                        .contains(&RecordDigest::from_bytes(*id.as_bytes()))
                },
                mode,
            )
            .map_err(|e| e.to_string())?;
        // Interrupted deletion can leave absent candidates forever. Native writer custody keeps
        // a re-promotion from crossing this cleanup of those stale arrival records.
        if mode.deletes() && !report.absent().is_empty() {
            self.store
                .journal()
                .forget(&report.absent().iter().copied().collect())
                .map_err(|e| e.to_string())?;
        }
        for pin in &self.pins {
            pin.ensure_namespace_identity().map_err(|e| e.to_string())?;
        }
        Ok(report)
    }
}
