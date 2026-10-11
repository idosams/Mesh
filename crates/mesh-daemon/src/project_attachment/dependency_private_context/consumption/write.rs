//! Sealed complete native source authority for uncommitted destination mutation only.
use super::*;
use crate::project_attachment::{
    consumption_prepare::source_verification::StartingSource, NativeTrustedConsumedStartRequest,
    ObservationLimits, SavedAttachmentVersion,
};
const ZERO: RecordDigest = RecordDigest::from_bytes([0; 32]);

// Private fields and constructor keep this proof inside one complete custody phase.
// It cannot admit ordinary history, owner completion, execution or protected main.
pub(in crate::project_attachment) struct NativeConsumptionWriteProof<'a> {
    storage: &'a AttachmentStorage,
    owner: &'a ProvisionedAttachment,
    source: &'a ProvisionedAttachment,
    destination: &'a ProvisionedAttachment,
    available: &'a [&'a ProvisionedAttachment],
    version: SavedAttachmentVersion,
    grant: RecordDigest,
    request: RecordDigest,
    limits: ObservationLimits,
    graph: &'a NativeDependencyGraph,
    source_binding: &'a NativeDependencyWorkBinding,
    destination_binding: &'a NativeDependencyWorkBinding,
    owner_binding: crate::dependency_policy::NativeDependencyBinding,
    guard: &'a WorkspaceInitializationGuard,
    verify: &'a dyn Fn() -> io::Result<()>,
    inspect_source: &'a dyn for<'s> Fn(StartingSource<'s>) -> io::Result<String>,
}
impl NativeConsumptionWriteProof<'_> {
    pub(in crate::project_attachment) fn verify(&self) -> io::Result<()> {
        (self.verify)()
    }
    pub(in crate::project_attachment) fn graph(&self) -> &NativeDependencyGraph {
        self.graph
    }
    pub(in crate::project_attachment) fn source(&self) -> &NativeDependencyWorkBinding {
        self.source_binding
    }
    pub(in crate::project_attachment) fn destination(&self) -> &NativeDependencyWorkBinding {
        self.destination_binding
    }
    pub(in crate::project_attachment) fn owner(
        &self,
    ) -> crate::dependency_policy::NativeDependencyBinding {
        self.owner_binding
    }
    pub(in crate::project_attachment) fn guard(&self) -> &WorkspaceInitializationGuard {
        self.guard
    }
    pub(in crate::project_attachment) fn version(&self) -> SavedAttachmentVersion {
        self.version
    }
    pub(in crate::project_attachment) fn recover<T>(
        &self,
        phase: RecoveryPhase,
        action: impl FnOnce(
            super::super::super::consumption_prepare::PreparedNativeConsumedStart,
            super::super::super::consumption_prepare::StagedNativeConsumedStart,
            &NativeDependencyGraph,
            &WorkspaceInitializationGuard,
        ) -> io::Result<T>,
    ) -> io::Result<T> {
        if phase == RecoveryPhase::CompletedRead {
            return Err(invalid(
                "native uncommitted recovery cannot admit completed history",
            ));
        }
        self.verify()?;
        let result = self.storage.with_recovered_consumed_material(
            RecoveryMaterial {
                owner: self.owner,
                request: NativeConsumedStartRequest {
                    input: NativeGrantInspection {
                        source: self.source,
                        version: self.version,
                        destination: self.destination,
                        grant: self.grant,
                    },
                    available: self.available,
                    request: self.request,
                    limits: self.limits,
                },
                phase,
                graph: self.graph,
                source_binding: self.source_binding,
                destination_binding: self.destination_binding,
                owner_binding: self.owner_binding,
            },
            self.guard,
            |_, expected| {
                self.verify()?;
                (self.inspect_source)(expected)
            },
            |_| self.verify(),
            action,
        )?;
        self.verify()?;
        Ok(result)
    }
}
impl AttachmentStorage {
    pub(in crate::project_attachment) fn with_native_consumption_write_proof<T>(
        &self,
        owner: &ProvisionedAttachment,
        request: &NativeTrustedConsumedStartRequest<'_>,
        trusted: &crate::TrustedReviewers,
        apply: impl FnOnce(NativeConsumptionWriteProof<'_>) -> io::Result<T>,
    ) -> io::Result<T> {
        if request.request == ZERO
            || request.operation == ZERO
            || request.grant == ZERO
            || request.available.len() > 256
            || request.source.id() == request.destination.id()
            || owner.id() == request.destination.id()
        {
            return Err(invalid("invalid native consumed-start selection"));
        }
        request.limits.validate()?;
        let verify_handles = || -> io::Result<()> {
            for work in [owner, request.source, request.destination]
                .into_iter()
                .chain(request.available.iter().copied())
            {
                self.exact_registered_work(work)?;
                if self.candidate_owning_root(work.id())? != owner.id() {
                    return Err(invalid(
                        "native consumed start crosses registered ownership",
                    ));
                }
                super::super::super::detachment::ensure_attached(&work.store)?;
            }
            Ok(())
        };
        verify_handles()?;
        let hints = self.catalog_discovery_hints(owner)?;
        let mut selected_ids = BTreeSet::from([request.source.id().to_owned()]);
        selected_ids.extend(request.available.iter().map(|work| work.id().to_owned()));
        let (_, parents) = hints
            .0
            .get(request.destination.id())
            .ok_or_else(|| invalid("native consumed destination registration missing"))?;
        selected_ids.extend(
            parents
                .iter()
                .filter(|id| id.as_str() != request.destination.id())
                .cloned(),
        );
        if selected_ids.len() > 256 || selected_ids.contains(request.destination.id()) {
            return Err(invalid(
                "empty destination cannot supply native source history",
            ));
        }
        let discover =
            |guard: &WorkspaceInitializationGuard,
             hints: super::super::super::dependency_catalog_discovery::CatalogHints| {
                selected_ids
                    .iter()
                    .map(|id| self.private_discovery(owner, id, guard, hints.clone()))
                    .collect::<io::Result<Vec<_>>>()
            };
        let prepared_owner = self.prepare_dependency_work(owner, owner)?;
        let discovery = {
            let guard =
                crate::workspace_custody::lock_workspace_initialization_set(&prepared_owner.roots)
                    .map_err(error)?;
            discover(&guard, hints)?
        };
        let ids = discovery
            .iter()
            .flat_map(|d| d.works.keys().cloned())
            .collect::<BTreeSet<_>>();
        if ids.len() > 256 || ids.contains(request.destination.id()) {
            return Err(invalid(
                "native consumed source discovery exceeds its bound or includes destination",
            ));
        }
        let works = ids
            .iter()
            .map(|id| self.reopen(id))
            .collect::<io::Result<Vec<_>>>()?;
        let selected = works.iter().collect::<Vec<_>>();
        let selected_graph =
            self.prepare_dependency_graph(owner, request.source, request.operation, &selected)?;
        let destination_selection = self.prepare_dependency_work(owner, request.destination)?;
        let origin = self
            .lane_origin_bound(request.destination)?
            .ok_or_else(|| invalid("native consumed start needs a reservation"))?;
        if !super::super::super::dependency_reservation::is_reservation(&origin.value) {
            return Err(invalid(
                "native consumption write requires its exact reservation",
            ));
        }
        let roots = selected_graph
            .roots
            .iter()
            .chain(&destination_selection.roots)
            .chain(std::iter::once(&origin.allocation))
            .map(|root| Ok((root.identity()?, root.clone())))
            .collect::<io::Result<BTreeMap<_, _>>>()?
            .into_values()
            .collect::<Vec<_>>();
        if roots.len() > 32 {
            return Err(invalid("native consumed-start custody exceeds its bound"));
        }
        let guard =
            crate::workspace_custody::lock_workspace_initialization_set(&roots).map_err(error)?;
        let verify_selection = || -> io::Result<()> {
            verify_handles()?;
            if discover(&guard, self.catalog_discovery_hints(owner)?)? != discovery {
                return Err(invalid("native consumption write discovery changed"));
            }
            ensure_consumption_retention_available(
                owner,
                request.destination,
                &works,
                RetentionPhase::Installed,
                request.request,
            )?;
            let current = self
                .lane_origin_bound(request.destination)?
                .ok_or_else(|| invalid("native reservation disappeared"))?;
            if current.value != origin.value
                || current.allocation.identity()? != origin.allocation.identity()?
            {
                return Err(invalid("native consumption reservation changed"));
            }
            guard.ensure_current().map_err(error)
        };
        verify_selection()?;
        let context = PrivateContext::resolve(self, owner, &works, &guard)?;
        let graph = context.graph(self, request.source, request.operation, &guard)?;
        if graph.operation_count() > 256 {
            return Err(invalid(
                "native consumption write closure exceeds its bound",
            ));
        }
        let source_binding = context.binding(self, request.source, &guard)?;
        let destination_binding = context.binding(self, request.destination, &guard)?;
        let owner_binding = context.read(owner, &guard)?.proof.binding();
        let verify_grant = || -> io::Result<()> {
            let owner = context.read(owner, &guard)?;
            if owner
                .proof
                .policy()
                .native_request(request.request)
                .is_some()
                || !owner.proof.policy().current_bound_grant(
                    (
                        source_binding.work(),
                        source_binding.installation(),
                        request.operation,
                    ),
                    (
                        destination_binding.work(),
                        destination_binding.installation(),
                    ),
                    request.grant,
                    (source_binding.correlation, destination_binding.correlation),
                )
            {
                return Err(invalid(
                    "native consumption write requires its exact current bound grant",
                ));
            }
            Ok(())
        };
        verify_grant()?;
        context.with_replayed_history(self, request.source, trusted, &guard, |history, _| {
            let version = SavedAttachmentVersion::from_verified_private_history(history, request.operation)?;
            let verify = || -> io::Result<()> {
                verify_selection()?;
                verify_grant()?;
                if context.graph(self, request.source, request.operation, &guard)? != graph
                    || context.binding(self, request.source, &guard)? != source_binding
                    || context.binding(self, request.destination, &guard)? != destination_binding {
                    return Err(invalid("native consumption write basis changed"));
                }
                // Trust can change independently from filesystem custody; replay it freshly.
                context.with_replayed_history(self, request.source, trusted, &guard, |current, _| {
                    if SavedAttachmentVersion::from_verified_private_history(current, request.operation)? != version {
                        return Err(invalid("native consumption source identity changed"));
                    }
                    current.with_saved_input(request.operation, |_| Ok(()))
                })?;
                guard.ensure_current().map_err(error)
            };
            let inspect_source = |expected: StartingSource<'_>| history.with_saved_input(request.operation, |input| {
                super::super::super::consumption_prepare::source_verification::verify_starting_source(request.destination, &input, &guard, expected)
            });
            verify()?;
            let result = apply(NativeConsumptionWriteProof {
                storage: self, owner, source: request.source, destination: request.destination,
                available: request.available, version, grant: request.grant, request: request.request,
                limits: request.limits, graph: &graph, source_binding: &source_binding,
                destination_binding: &destination_binding, owner_binding, guard: &guard,
                verify: &verify, inspect_source: &inspect_source,
            })?;
            verify()?;
            Ok(result)
        })
    }
}
