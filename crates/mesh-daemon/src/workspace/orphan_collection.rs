//! Private collection preparation. The caller must retain native writer custody through execute.
use super::*;
use mesh_store::{CollectionPlan, Reachability, RetainedRoots};

pub(crate) struct OrphanCollection {
    store: Cas<crate::root_authority::PinnedRootFs, mesh_cas::Blake3>,
    pins: Vec<crate::root_authority::PinnedWorkspaceRoot>,
    doomed: Vec<CasDigest>,
    named: BTreeSet<RecordDigest>,
}
pub(crate) struct OrphanCollectionSource {
    store: Cas<crate::root_authority::PinnedRootFs, mesh_cas::Blake3>,
    pins: Vec<crate::root_authority::PinnedWorkspaceRoot>,
    journal: RecordFile,
    ledger: Vec<mesh_store::Row>,
}

impl OpenWorkspace {
    /// Capture independent descriptor authority, without scanning the record stream or payloads.
    pub(crate) fn orphan_collection_source(&self) -> Result<OrphanCollectionSource, String> {
        self.ensure_physical_root().map_err(|e| e.to_string())?;
        if self.tail.is_fragment() || !self.names_answered || !self.conditions.is_empty() {
            return Err("workspace history or recovery is incomplete".into());
        }
        let journal = RecordFile::open_existing_pinned(
            &self.storage_pinned_root,
            Path::new(RECORD_FILE_NAME),
            self.record_file.clone(),
        )
        .map_err(|e| e.to_string())?;
        #[cfg(unix)]
        if journal.support_identity().map_err(|e| e.to_string())?
            != self.journal.support_identity().map_err(|e| e.to_string())?
        {
            return Err("workspace journal identity changed".into());
        }
        let store = Cas::with_filesystem(
            self.storage_root.as_path().to_path_buf(),
            self.storage_pinned_root.filesystem(),
        )
        .map_err(|e| e.to_string())?;
        Ok(OrphanCollectionSource {
            store,
            pins: [
                Some(self.pinned_root.clone()),
                Some(self.storage_pinned_root.clone()),
                self.storage_namespace.clone(),
            ]
            .into_iter()
            .flatten()
            .collect(),
            journal,
            ledger: self
                .store
                .index()
                .rows("schema_version")
                .unwrap_or_default(),
        })
    }
}

impl OrphanCollectionSource {
    /// Fresh history is folded outside the live view lock, while the caller retains custody.
    pub(crate) fn prepare(
        mut self,
        expected_digest: &str,
    ) -> Result<OrphanCollection, crate::ManagedTextFileError> {
        use crate::ManagedTextFileError;
        fn refused(error: impl std::fmt::Display) -> ManagedTextFileError {
            ManagedTextFileError::Recovery(error.to_string())
        }
        for pin in &self.pins {
            pin.ensure_namespace_identity().map_err(refused)?;
        }
        let bytes = self.journal.read_all().map_err(refused)?;
        let scan = scan_journal(&bytes).map_err(refused)?;
        if scan.tail().is_fragment() {
            return Err(refused("workspace history has an unfinished record"));
        }
        let (index, _) = mesh_store::rebuild(scan.into_records(), self.ledger).map_err(refused)?;
        if index.default_digest().to_string() != expected_digest {
            return Err(ManagedTextFileError::StaleWorkspace);
        }
        let names = materialize_names(&index, &self.store);
        if !names.complete || !names.conditions.is_empty() {
            return Err(refused("workspace history or payloads are incomplete"));
        }
        let roots = RetainedRoots::conservative(&index, mesh_store::RetentionPolicy::default());
        let reachable = Reachability::compute(&index, &roots).map_err(refused)?;
        if reachable.dangling_parents().next().is_some() {
            return Err(refused("workspace history has missing parents"));
        }
        let candidates = self.store.journal().candidates().map_err(refused)?;
        let plan = CollectionPlan::compute(
            &index,
            &roots,
            &reachable,
            candidates
                .iter()
                .map(|id| RecordDigest::from_bytes(*id.as_bytes())),
        )
        .map_err(refused)?;
        Ok(OrphanCollection {
            store: self.store,
            pins: self.pins,
            doomed: plan
                .doomed_digests()
                .into_iter()
                .take(256)
                .map(CasDigest::from_bytes)
                .collect(),
            // Independent veto from every recorded payload/manifest/chunk, not from the plan.
            named: index.named_content(),
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
