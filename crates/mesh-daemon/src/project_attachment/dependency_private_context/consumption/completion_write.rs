//! Closed native destination completion authority. No ordinary history or runtime admission.
use super::*;
use crate::project_attachment::{
    consumption_prepare::{
        source_verification::StartingSource, NativeCompletionMaterial, PreparedNativeConsumedStart,
        StagedNativeConsumedStart,
    },
    NativeTrustedConsumedStartRequest, SavedAttachmentVersion,
};
struct CompletionBasis {
    candidate: PreparedNativeConsumedStart,
    staged: StagedNativeConsumedStart,
    graph: NativeDependencyGraph,
    version: SavedAttachmentVersion,
    source: NativeDependencyWorkBinding,
    destination: NativeDependencyWorkBinding,
    owner: (String, VerifiedPrivateHistory),
    material: NativeCompletionMaterial,
    owner_attempt: (u64, u64),
}
#[derive(PartialEq, Eq)]
struct CompletionIdentity {
    material: NativeCompletionMaterial,
    operation: RecordDigest,
    stage: RecordDigest,
    stage_root: (u64, u64),
    graph: RecordDigest,
    owner_attempt: (u64, u64),
}
impl CompletionBasis {
    fn matches(&self, other: &Self) -> io::Result<bool> {
        Ok(self.candidate.operation() == other.candidate.operation()
            && self.candidate.request() == other.candidate.request()
            && self.staged.receipt()? == other.staged.receipt()?
            && self.staged.retained_namespace_identity()?
                == other.staged.retained_namespace_identity()?
            && self.graph == other.graph
            && self.version == other.version
            && self.source == other.source
            && self.destination == other.destination
            && self.owner.0 == other.owner.0
            && self.owner.1.binding() == other.owner.1.binding()
            && self.owner_attempt == other.owner_attempt
            && self.material.same_origin(&other.material))
    }
    fn identity(&self) -> io::Result<CompletionIdentity> {
        Ok(CompletionIdentity {
            material: self.material.clone(),
            operation: self.candidate.operation(),
            stage: self.staged.receipt()?,
            stage_root: self.staged.retained_namespace_identity()?,
            graph: self.graph.digest(),
            owner_attempt: self.owner_attempt,
        })
    }
}
// Only this module mints the completion proof, after a complete exact committed-owner join.
pub(in crate::project_attachment) struct NativeCompletionConsumptionProof<'a> {
    basis: CompletionBasis,
    select: &'a dyn Fn() -> io::Result<CompletionBasis>,
}
impl NativeCompletionConsumptionProof<'_> {
    pub(in crate::project_attachment) fn verify_candidate(
        &self,
        candidate: &PreparedNativeConsumedStart,
    ) -> io::Result<()> {
        candidate.verify_native_owner_selection(&self.basis.candidate)
    }
    pub(in crate::project_attachment) fn material(&self) -> &NativeCompletionMaterial {
        &self.basis.material
    }
    pub(in crate::project_attachment) fn verify_before(&self) -> io::Result<()> {
        let current = (self.select)()?;
        if !self.basis.matches(&current)? || current.material.receipt != self.basis.material.receipt
        {
            return Err(invalid("native completion basis changed before append"));
        }
        Ok(())
    }
    pub(in crate::project_attachment) fn verify_after(
        &self,
        record: mesh_store::DependencyRecord,
    ) -> io::Result<()> {
        let current = (self.select)()?;
        if record.kind != mesh_store::DependencyKind::ConsumptionComplete
            || !self.basis.matches(&current)?
            || current.material.receipt != Some(record.payload)
        {
            return Err(invalid(
                "native completion did not commit its exact owner receipt",
            ));
        }
        Ok(())
    }
}
fn ensure_native_completion_available(
    owner: &ProvisionedAttachment,
    destination: &ProvisionedAttachment,
    works: &[ProvisionedAttachment],
) -> io::Result<()> {
    for work in works.iter().chain([owner, destination]) {
        super::super::super::detachment::ensure_attached(&work.store)?;
        for pending in [
            super::super::super::dependency_decision::PENDING,
            super::super::super::history::dependency_capture::PENDING,
            super::super::publication::PENDING,
        ] {
            super::super::super::history::dependency_capture::absent(&work.store, pending)?;
        }
    }
    Ok(())
}
impl AttachmentStorage {
    /// Complete or recover the exact local transaction after its committed native owner receipt.
    /// This historical receipt creates no new input permission, ordinary history or execution.
    pub fn complete_native_consumption_receipt(
        &self,
        owner: &ProvisionedAttachment,
        request: NativeTrustedConsumedStartRequest<'_>,
        trusted: &crate::TrustedReviewers,
    ) -> io::Result<RecordDigest> {
        self.complete_native_consumption_with_hooks(
            owner,
            &request,
            trusted,
            (|_, _, _| Ok(()), |file| file.sync_all()),
            || Ok(()),
        )
    }
    pub(in crate::project_attachment) fn complete_native_consumption_with_hooks(
        &self,
        owner: &ProvisionedAttachment,
        request: &NativeTrustedConsumedStartRequest<'_>,
        trusted: &crate::TrustedReviewers,
        io: (
            impl FnMut(&str, &mut std::fs::File, &[u8]) -> io::Result<()>,
            impl FnMut(&std::fs::File) -> io::Result<()>,
        ),
        after_custody: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<RecordDigest> {
        let (record, identity) =
            self.with_native_completion_consumption_proof(owner, request, trusted, |proof| {
                let record = proof
                    .basis
                    .candidate
                    .commit_native_completion_material_held(&proof, io)?;
                let committed = (proof.select)()?;
                if !proof.basis.matches(&committed)? || committed.material.receipt != Some(record) {
                    return Err(invalid("native completion changed after its held write"));
                }
                Ok((record, committed.identity()?))
            })?;
        after_custody()?;
        self.with_native_completion_consumption_proof(owner, request, trusted, |proof| {
            if proof.basis.material.receipt != Some(record) || proof.basis.identity()? != identity {
                return Err(invalid("native completion changed before acknowledgement"));
            }
            Ok(record)
        })
    }
    fn with_native_completion_consumption_proof<T>(
        &self,
        owner: &ProvisionedAttachment,
        request: &NativeTrustedConsumedStartRequest<'_>,
        trusted: &crate::TrustedReviewers,
        apply: impl FnOnce(NativeCompletionConsumptionProof<'_>) -> io::Result<T>,
    ) -> io::Result<T> {
        let zero = RecordDigest::from_bytes([0; 32]);
        if request.request == zero
            || request.operation == zero
            || request.grant == zero
            || request.available.len() > 256
            || request.source.id() == request.destination.id()
            || owner.id() == request.destination.id()
        {
            return Err(invalid("invalid native completion selection"));
        }
        request.limits.validate()?;
        let verify_handles = || -> io::Result<()> {
            for work in [owner, request.source, request.destination]
                .into_iter()
                .chain(request.available.iter().copied())
            {
                self.exact_registered_work(work)?;
                if self.candidate_owning_root(work.id())? != owner.id() {
                    return Err(invalid("native completion crosses registered ownership"));
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
            .ok_or_else(|| invalid("native completion destination registration missing"))?;
        selected_ids.extend(
            parents
                .iter()
                .filter(|id| id.as_str() != request.destination.id())
                .cloned(),
        );
        if selected_ids.len() > 256 || selected_ids.contains(request.destination.id()) {
            return Err(invalid(
                "unfinished destination cannot supply owner source history",
            ));
        }
        let owner_selection = self.prepare_dependency_work(owner, owner)?;
        let discover =
            |guard: &WorkspaceInitializationGuard,
             hints: super::super::super::dependency_catalog_discovery::CatalogHints| {
                selected_ids
                    .iter()
                    .map(|id| {
                        self.private_discovery_with_recovery(owner, id, guard, hints.clone(), None)
                    })
                    .collect::<io::Result<Vec<_>>>()
            };
        let discovery = {
            let guard =
                crate::workspace_custody::lock_workspace_initialization_set(&owner_selection.roots)
                    .map_err(error)?;
            committed_owner::VerifiedConsumptionOwnerReceipt::read(owner, request.request)?;
            discover(&guard, hints)?
        };
        let ids = discovery
            .iter()
            .flat_map(|d| d.works.keys().cloned())
            .collect::<BTreeSet<_>>();
        if ids.len() > 256 || ids.contains(request.destination.id()) {
            return Err(invalid(
                "native completion source discovery exceeds its bound or includes destination",
            ));
        }
        let works = ids
            .iter()
            .map(|id| self.reopen(id))
            .collect::<io::Result<Vec<_>>>()?;
        let selected = works.iter().collect::<Vec<_>>();
        let graph_selection =
            self.prepare_dependency_graph(owner, request.source, request.operation, &selected)?;
        let destination_selection = self.prepare_dependency_work(owner, request.destination)?;
        let origin = self
            .lane_origin_bound(request.destination)?
            .ok_or_else(|| invalid("native completion requires its original reservation"))?;
        if !super::super::super::dependency_reservation::is_reservation(&origin.value) {
            return Err(invalid("native completion requires its exact reservation"));
        }
        let roots = graph_selection
            .roots
            .iter()
            .chain(&destination_selection.roots)
            .chain(std::iter::once(&origin.allocation))
            .map(|root| Ok((root.identity()?, root.clone())))
            .collect::<io::Result<BTreeMap<_, _>>>()?
            .into_values()
            .collect::<Vec<_>>();
        if roots.len() > 32 {
            return Err(invalid("native completion custody exceeds its bound"));
        }
        let guard =
            crate::workspace_custody::lock_workspace_initialization_set(&roots).map_err(error)?;
        // Only the independently committed owner receipt can authorize this local completion.
        // Source registrations/closure and installed checkpoint remain fixed on every selection.
        let select = || -> io::Result<CompletionBasis> {
            verify_handles()?;
            for work in &works {
                self.exact_registered_work(work)?;
            }
            let current_origin = self
                .lane_origin_bound(request.destination)?
                .ok_or_else(|| invalid("native completion reservation disappeared"))?;
            if current_origin.value != origin.value
                || current_origin.allocation.identity()? != origin.allocation.identity()?
            {
                return Err(invalid("native completion reservation changed"));
            }
            ensure_native_completion_available(owner, request.destination, &works)?;
            let receipt =
                committed_owner::VerifiedConsumptionOwnerReceipt::read(owner, request.request)?;
            let current_discovery = discover(&guard, self.catalog_discovery_hints(owner)?)?;
            if current_discovery
                .iter()
                .flat_map(|d| d.works.keys().cloned())
                .collect::<BTreeSet<_>>()
                != ids
            {
                return Err(invalid("native completion source closure changed"));
            }
            let context = PrivateContext::resolve_with_recovery(self, owner, &works, &guard, None)?;
            let graph = context.graph(self, request.source, request.operation, &guard)?;
            if graph.operation_count() > 256 {
                return Err(invalid(
                    "native completion inherited operations exceed bound",
                ));
            }
            let source = context.binding(self, request.source, &guard)?;
            let destination = context.binding(self, request.destination, &guard)?;
            let snapshot = context.read(owner, &guard)?;
            let verify_permission = || -> io::Result<()> {
                receipt.verify_context(owner, &context.read(owner, &guard)?.proof)?;
                guard.ensure_current().map_err(error)
            };
            verify_permission()?;
            context.with_replayed_history(self, request.source, trusted, &guard, |history, _| {
                let version = SavedAttachmentVersion::from_verified_private_history(history, request.operation)?;
                self.with_recovered_consumed_material(
                    RecoveryMaterial {
                        owner, request: NativeConsumedStartRequest {
                            input: NativeGrantInspection { source: request.source, version, destination: request.destination, grant: request.grant },
                            available: request.available, request: request.request, limits: request.limits,
                        }, phase: RecoveryPhase::Installed, graph: &graph, source_binding: &source, destination_binding: &destination, owner_binding: snapshot.proof.binding(),
                    }, &guard,
                    |_, expected: StartingSource<'_>| {
                        verify_permission()?;
                        history.with_saved_input(request.operation, |input| super::super::super::consumption_prepare::source_verification::verify_starting_source(request.destination, &input, &guard, expected))
                    }, |_| verify_permission(),
                    |candidate, staged, _, held| {
                        let body = receipt.body_for(owner)?;
                        let material = candidate.native_completion_material_held(&graph, receipt.record().payload, &body, held)?;
                        verify_permission()?;
                        Ok(CompletionBasis { candidate, staged, graph: graph.clone(), version, source: source.clone(), destination: destination.clone(), owner: (snapshot.configuration.clone(), snapshot.proof.clone()), material, owner_attempt: receipt.sidecar_identity(owner)? })
                    },
                )
            })
        };
        let basis = select()
            .map_err(|e| io::Error::new(e.kind(), format!("native completion selection: {e}")))?;
        let result = apply(NativeCompletionConsumptionProof {
            basis,
            select: &select,
        })?;
        guard.ensure_current().map_err(error)?;
        Ok(result)
    }
}
