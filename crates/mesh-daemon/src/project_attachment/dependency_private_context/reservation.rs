//! Empty correlated destinations selected through complete configured-trust native history.
use super::super::SavedAttachmentVersion;
use super::*;
const ZERO: RecordDigest = RecordDigest::from_bytes([0; 32]);
struct Selection<'a> {
    owner: &'a ProvisionedAttachment,
    parent: &'a ProvisionedAttachment,
    operation: RecordDigest,
    available: &'a [&'a ProvisionedAttachment],
}
impl Selection<'_> {
    fn verify_handles(&self, storage: &AttachmentStorage) -> io::Result<()> {
        for work in [self.owner, self.parent]
            .into_iter()
            .chain(self.available.iter().copied())
        {
            storage.exact_registered_work(work)?;
            if storage.candidate_owning_root(work.id())? != self.owner.id() {
                return Err(invalid("native reservation crosses registered ownership"));
            }
            super::super::detachment::ensure_attached(&work.store)?;
        }
        Ok(())
    }
    fn discover(
        &self,
        storage: &AttachmentStorage,
        guard: &WorkspaceInitializationGuard,
        hints: super::super::dependency_catalog_discovery::CatalogHints,
    ) -> io::Result<Vec<Discovery>> {
        let ids = std::iter::once(self.parent)
            .chain(self.available.iter().copied())
            .map(|work| work.id())
            .collect::<BTreeSet<_>>();
        ids.into_iter()
            .map(|id| storage.private_discovery(self.owner, id, guard, hints.clone()))
            .collect()
    }
}
impl AttachmentStorage {
    /// Reserve an empty fenced destination through complete configured publication trust.
    /// The operation is only a selector: every custody phase reconstructs its exact saved content.
    /// This creates correlation, not an input grant, installed content, runnable lane or main authority.
    #[allow(clippy::too_many_arguments)]
    pub fn reserve_native_dependency_lane(
        &self,
        owner: &ProvisionedAttachment,
        parent: &ProvisionedAttachment,
        operation: RecordDigest,
        request: RecordDigest,
        available: &[&ProvisionedAttachment],
        trusted: &crate::TrustedReviewers,
    ) -> io::Result<ProvisionedAttachment> {
        self.reserve_native_dependency_lane_with_hook(
            owner,
            parent,
            operation,
            request,
            available,
            trusted,
            |_| Ok(()),
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub(in crate::project_attachment) fn reserve_native_dependency_lane_with_hook(
        &self,
        owner: &ProvisionedAttachment,
        parent: &ProvisionedAttachment,
        operation: RecordDigest,
        request: RecordDigest,
        available: &[&ProvisionedAttachment],
        trusted: &crate::TrustedReviewers,
        hook: impl FnMut(&str) -> io::Result<()>,
    ) -> io::Result<ProvisionedAttachment> {
        if operation == ZERO || request == ZERO || available.len() > 256 {
            return Err(invalid("invalid native reservation request"));
        }
        let selection = Selection {
            owner,
            parent,
            operation,
            available,
        };
        selection.verify_handles(self)?;
        let prepared_owner = self.prepare_dependency_work(owner, owner)?;
        self.prepare_dependency_work(owner, parent)?
            .require_child_capacity()?;
        // Pin catalog candidates before taking the owner-only discovery lock.
        let hints = self.catalog_discovery_hints(owner)?;
        let discovery = {
            let guard =
                crate::workspace_custody::lock_workspace_initialization_set(&prepared_owner.roots)
                    .map_err(error)?;
            selection.discover(self, &guard, hints)?
        };
        let ids = discovery
            .iter()
            .flat_map(|d| d.works.keys().cloned())
            .collect::<BTreeSet<_>>();
        if ids.len() > 256 {
            return Err(invalid("native reservation source bound exceeded"));
        }
        let works = ids
            .iter()
            .map(|id| self.reopen(id))
            .collect::<io::Result<Vec<_>>>()?;
        let selected = works.iter().collect::<Vec<_>>();
        let graph = self.prepare_dependency_graph(owner, parent, operation, &selected)?;
        let resolve = |guard: &WorkspaceInitializationGuard| -> io::Result<PrivateContext<'_>> {
            selection.verify_handles(self)?;
            if selection.discover(self, guard, self.catalog_discovery_hints(owner)?)? != discovery {
                return Err(invalid("native reservation source history changed"));
            }
            for work in &works {
                self.exact_registered_work(work)?;
                super::super::detachment::ensure_attached(&work.store)?;
                // Completed consumption receipts remain historical evidence. Active capture,
                // control and publication intents must recover before a new allocation.
                for pending in [
                    super::super::history::dependency_capture::PENDING,
                    super::super::dependency_decision::PENDING,
                    super::publication::PENDING,
                ] {
                    super::super::history::dependency_capture::absent(&work.store, pending)?;
                }
            }
            PrivateContext::resolve(self, owner, &works, guard)
        };
        let verify =
            |guard: &WorkspaceInitializationGuard| -> io::Result<NativeDependencyWorkBinding> {
                let context = resolve(guard)?;
                context.graph(self, parent, selection.operation, guard)?;
                context.with_replayed_history(self, parent, trusted, guard, |history, _| {
                    SavedAttachmentVersion::from_verified_private_history(
                        history,
                        selection.operation,
                    )?;
                    history.with_saved_input(selection.operation, |_| Ok(()))
                })?;
                context.binding(self, parent, guard)
            };
        self.reserve_verified_with_hook(
            owner,
            parent,
            operation,
            request,
            graph.roots,
            verify,
            |work, guard| {
                self.exact_registered_work(work)?;
                if self.candidate_owning_root(work.id())? != owner.id() {
                    return Err(invalid("native reserved destination ownership changed"));
                }
                resolve(guard)?.binding(self, work, guard).map(|_| ())
            },
            hook,
        )
    }
}
