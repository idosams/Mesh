//! Complete native preparation barrier. Facts only live inside one synchronous custody phase.
use super::*;
use crate::{
    project_attachment::{
        consumption_prepare::{starting_snapshot, Basis},
        consumption_start::InitialSnapshot,
        NativeTrustedConsumedStartRequest, SavedAttachmentVersion,
    },
    root_authority::PinnedWorkspaceRoot,
    workspace::OpenWorkspace,
};
use mesh_types::PublicKey;
use std::path::Path;
const ZERO: RecordDigest = RecordDigest::from_bytes([0; 32]);
pub(in crate::project_attachment) struct NativeConsumedStartBasis<'a> {
    pub(in crate::project_attachment) basis: Basis,
    pub(in crate::project_attachment) snapshot: InitialSnapshot,
    pub(in crate::project_attachment) history: OpenWorkspace,
    pub(in crate::project_attachment) version: SavedAttachmentVersion,
    pub(in crate::project_attachment) graph: &'a NativeDependencyGraph,
    pub(in crate::project_attachment) allocation: &'a PinnedWorkspaceRoot,
    pub(in crate::project_attachment) guard: &'a WorkspaceInitializationGuard,
}
impl AttachmentStorage {
    pub(in crate::project_attachment) fn with_native_consumed_start_basis<T>(
        &self,
        owner: &ProvisionedAttachment,
        request: &NativeTrustedConsumedStartRequest<'_>,
        actor: PublicKey,
        trusted: &crate::TrustedReviewers,
        apply: impl FnOnce(NativeConsumedStartBasis<'_>) -> io::Result<T>,
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
            return Err(invalid("native consumed start needs an empty reservation"));
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
                return Err(invalid("native consumed source discovery changed"));
            }
            for work in works.iter().chain([owner, request.destination]) {
                self.exact_registered_work(work)?;
                super::super::super::detachment::ensure_attached(&work.store)?;
                for pending in [
                    super::super::super::dependency_decision::PENDING,
                    super::super::super::history::dependency_capture::PENDING,
                    super::super::publication::PENDING,
                ] {
                    super::super::super::history::dependency_capture::absent(&work.store, pending)?;
                }
            }
            for pending in [
                super::super::super::consumption_prepare::START_PENDING,
                super::super::super::consumption_history::PENDING,
                "consumption-owner.pending",
                super::super::super::consumption_complete::PENDING,
            ] {
                super::super::super::history::dependency_capture::absent(
                    &request.destination.store,
                    pending,
                )?;
            }
            let current = self
                .lane_origin_bound(request.destination)?
                .ok_or_else(|| invalid("native reservation disappeared"))?;
            if current.value != origin.value
                || current.allocation.identity()? != origin.allocation.identity()?
            {
                return Err(invalid("native consumed reservation changed"));
            }
            if !request
                .destination
                .project()
                .pinned
                .filesystem()
                .read_directory_names_bounded(Path::new(""), 1)?
                .is_empty()
            {
                return Err(invalid(
                    "native consumed destination contains unexpected work",
                ));
            }
            guard.ensure_current().map_err(error)
        };
        verify_selection()?;
        let context = PrivateContext::resolve(self, owner, &works, &guard)?;
        let graph = context.graph(self, request.source, request.operation, &guard)?;
        if graph.operation_count() > 256 {
            return Err(invalid(
                "native consumption closure exceeds policy format bound",
            ));
        }
        let source_binding = context.binding(self, request.source, &guard)?;
        let destination_binding = context.binding(self, request.destination, &guard)?;
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
                    "native consumed start requires its exact current bound grant",
                ));
            }
            Ok(())
        };
        verify_grant()?;
        let destination = request.destination;
        let (configuration, proof) = destination
            .project()
            .read_configuration(destination.metadata_path(), &destination.store)?;
        let proof =
            proof.ok_or_else(|| invalid("native consumed destination enrollment missing"))?;
        let history = OpenWorkspace::open_attachment_read_history(
            destination.metadata_path(),
            destination.store.clone(),
            &crate::TrustedReviewers::default(),
            Some(&proof),
        )
        .map_err(error)?;
        super::super::super::history::verify_history_binding(&history, &configuration)?;
        if history.operations() != 0 {
            return Err(invalid(
                "native consumed destination already has saved work",
            ));
        }
        let (version, prospective, snapshot) = context.with_replayed_history(
            self,
            request.source,
            trusted,
            &guard,
            |history, _| {
                let version = SavedAttachmentVersion::from_verified_private_history(
                    history,
                    request.operation,
                )?;
                let (prospective, snapshot) =
                    history.with_saved_input(request.operation, |input| {
                        starting_snapshot(
                            destination,
                            &configuration,
                            &input,
                            actor,
                            request.limits,
                        )
                    })?;
                Ok((version, prospective, snapshot))
            },
        )?;
        let verify_current = || -> io::Result<()> {
            verify_selection()?;
            verify_grant()?;
            let (current_configuration, current_proof) = destination
                .project()
                .read_configuration(destination.metadata_path(), &destination.store)?;
            if current_configuration != configuration
                || current_proof.as_ref() != Some(&proof)
                || context.graph(self, request.source, request.operation, &guard)? != graph
                || context.binding(self, destination, &guard)? != destination_binding
                || context.binding(self, request.source, &guard)? != source_binding
            {
                return Err(invalid("native consumed preparation basis changed"));
            }
            // Trust configuration may change independently from workspace custody.
            context.with_replayed_history(
                self,
                request.source,
                trusted,
                &guard,
                |history, _| {
                    let current = SavedAttachmentVersion::from_verified_private_history(
                        history,
                        request.operation,
                    )?;
                    if current != version {
                        return Err(invalid("native saved input identity changed"));
                    }
                    history.with_saved_input(request.operation, |_| Ok(()))
                },
            )?;
            guard.ensure_current().map_err(error)
        };
        verify_current()?;
        let result = apply(NativeConsumedStartBasis {
            basis: Basis {
                configuration: configuration.clone(),
                prospective,
                graph: graph.digest(),
                destination: destination_binding.clone(),
                enrollment: proof.binding(),
            },
            snapshot,
            history,
            version,
            graph: &graph,
            allocation: &origin.allocation,
            guard: &guard,
        })?;
        verify_current()?;
        Ok(result)
    }
}
