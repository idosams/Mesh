//! Closed native owner receipt authority. Destination completion remains separate.
use super::*;
use crate::project_attachment::{
    consumption_prepare::{
        source_verification::StartingSource, PreparedNativeConsumedStart, StagedNativeConsumedStart,
    },
    NativeTrustedConsumedStartRequest, SavedAttachmentVersion,
};
use std::{fs::File, os::unix::ffi::OsStrExt as _, path::Path};

// Classification selects exact recovery evidence; it is never input permission by itself.
enum OwnerState {
    Uncommitted,
    Prefix(Box<VerifiedConsumptionOwnerPrefix>),
    Committed(Box<committed_owner::VerifiedConsumptionOwnerReceipt>),
}
impl OwnerState {
    fn read(owner: &ProvisionedAttachment, request: RecordDigest) -> io::Result<Self> {
        let prefix = format!("consumption-owner-{}-", request.to_hex());
        let names = owner
            .store
            .filesystem()
            .read_directory_names_bounded(Path::new(""), 16384)?;
        let count = names
            .iter()
            .filter(|name| name.as_bytes().starts_with(prefix.as_bytes()))
            .count();
        if count > 1 {
            return Err(invalid("native owner receipt attempts are ambiguous"));
        }
        if let Ok((_, proof)) = owner
            .project()
            .read_publication_private_history(owner.metadata_path(), &owner.store)
        {
            if proof.policy().native_request(request).is_some() {
                return Ok(Self::Committed(Box::new(
                    committed_owner::VerifiedConsumptionOwnerReceipt::read(owner, request)?,
                )));
            }
            if count == 0 {
                return Ok(Self::Uncommitted);
            }
        }
        Ok(Self::Prefix(Box::new(
            VerifiedConsumptionOwnerPrefix::read(owner, request)?,
        )))
    }
    fn recovery(&self) -> Option<OwnerRecovery<'_>> {
        match self {
            Self::Prefix(prefix) => Some(OwnerRecovery::Consumption(prefix)),
            _ => None,
        }
    }
    fn sidecar_identity(&self, owner: &ProvisionedAttachment) -> io::Result<Option<(u64, u64)>> {
        match self {
            Self::Uncommitted => Ok(None),
            Self::Prefix(prefix) => prefix.sidecar_identity(owner).map(Some),
            Self::Committed(receipt) => receipt.sidecar_identity(owner).map(Some),
        }
    }
    fn receipt(&self) -> Option<RecordDigest> {
        match self {
            Self::Committed(receipt) => Some(receipt.record().payload),
            _ => None,
        }
    }
    fn verify_body(
        &self,
        owner: &ProvisionedAttachment,
        context: &PrivateContext<'_>,
        guard: &WorkspaceInitializationGuard,
        expected: &Json,
    ) -> io::Result<()> {
        let body = match self {
            Self::Uncommitted => return Ok(()),
            Self::Prefix(prefix) => prefix.body_for(owner)?,
            Self::Committed(receipt) => {
                receipt.verify_context(owner, &context.read(owner, guard)?.proof)?;
                receipt.body_for(owner)?
            }
        };
        if &body != expected {
            return Err(invalid(
                "native owner receipt differs from its exact checkpoint",
            ));
        }
        Ok(())
    }
}
struct OwnerBasis {
    candidate: PreparedNativeConsumedStart,
    staged: StagedNativeConsumedStart,
    graph: NativeDependencyGraph,
    version: SavedAttachmentVersion,
    source: NativeDependencyWorkBinding,
    destination: NativeDependencyWorkBinding,
    owner: (String, VerifiedPrivateHistory),
    intent: String,
    receipt: Option<RecordDigest>,
    attempt: Option<(u64, u64)>,
}
impl OwnerBasis {
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
            && self.intent == other.intent
            && self
                .attempt
                .is_none_or(|identity| other.attempt == Some(identity)))
    }
    fn identity(&self) -> io::Result<(RecordDigest, RecordDigest, (u64, u64), RecordDigest)> {
        Ok((
            self.candidate.operation(),
            self.staged.receipt()?,
            self.staged.retained_namespace_identity()?,
            hash(self.intent.as_bytes()),
        ))
    }
}
/// Minted only after complete native source and exact installed-checkpoint verification.
/// It does not convert private history into ordinary admission or expose a writable candidate.
pub(in crate::project_attachment) struct NativeOwnerConsumptionProof<'a> {
    basis: OwnerBasis,
    select: &'a dyn Fn() -> io::Result<OwnerBasis>,
}
impl NativeOwnerConsumptionProof<'_> {
    pub(in crate::project_attachment) fn verify_candidate(
        &self,
        candidate: &PreparedNativeConsumedStart,
    ) -> io::Result<()> {
        candidate.verify_native_owner_selection(&self.basis.candidate)
    }

    pub(in crate::project_attachment) fn intent(&self) -> &str {
        &self.basis.intent
    }
    pub(in crate::project_attachment) fn owner_snapshot(&self) -> (String, VerifiedPrivateHistory) {
        self.basis.owner.clone()
    }
    pub(in crate::project_attachment) fn verify_before(&self) -> io::Result<()> {
        let current = (self.select)()
            .map_err(|e| io::Error::new(e.kind(), format!("native owner before append: {e}")))?;
        if !self.basis.matches(&current)? || current.receipt != self.basis.receipt {
            return Err(invalid("native owner receipt basis changed before append"));
        }
        Ok(())
    }
    pub(in crate::project_attachment) fn verify_after(
        &self,
        record: mesh_store::DependencyRecord,
    ) -> io::Result<(String, VerifiedPrivateHistory)> {
        let current = (self.select)()
            .map_err(|e| io::Error::new(e.kind(), format!("native owner after append: {e}")))?;
        if !self.basis.matches(&current)?
            || current.receipt != Some(record.payload)
            || current
                .owner
                .1
                .policy()
                .native_request(self.basis.candidate.request())
                != Some(record)
        {
            return Err(invalid(
                "native owner receipt did not commit against its original checkpoint",
            ));
        }
        Ok(current.owner)
    }
}
impl AttachmentStorage {
    /// Commit or recover the exact historical owner receipt for a retained native checkpoint.
    /// An uncommitted append needs current input permission. A committed receipt is historical
    /// evidence, including after later revocation; it never authorizes new input or completion.
    pub fn commit_native_consumed_owner_receipt(
        &self,
        owner: &ProvisionedAttachment,
        request: NativeTrustedConsumedStartRequest<'_>,
        trusted: &crate::TrustedReviewers,
    ) -> io::Result<RecordDigest> {
        self.commit_native_consumed_owner_with_hooks(
            owner,
            &request,
            trusted,
            (|_, _, _| Ok(()), |file| file.sync_all()),
            || Ok(()),
        )
    }
    pub(in crate::project_attachment) fn commit_native_consumed_owner_with_hooks(
        &self,
        owner: &ProvisionedAttachment,
        request: &NativeTrustedConsumedStartRequest<'_>,
        trusted: &crate::TrustedReviewers,
        io: (
            impl FnMut(&str, &mut File, &[u8]) -> io::Result<()>,
            impl FnMut(&File) -> io::Result<()>,
        ),
        after_custody: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<RecordDigest> {
        let (record, identity, attempt) =
            self.with_native_owner_consumption_proof(owner, request, trusted, |proof| {
                let identity = proof.basis.identity()?;
                let record = proof
                    .basis
                    .candidate
                    .commit_native_owner_material_held(&proof, io)?;
                let committed = (proof.select)()?;
                if !proof.basis.matches(&committed)? || committed.receipt != Some(record) {
                    return Err(invalid("native owner receipt changed after its held write"));
                }
                let attempt = committed
                    .attempt
                    .ok_or_else(|| invalid("native owner committed attempt missing"))?;
                Ok((record, identity, attempt))
            })?;
        after_custody()?;
        self.with_native_owner_consumption_proof(owner, request, trusted, |proof| {
            if proof.basis.receipt != Some(record)
                || proof.basis.identity()? != identity
                || proof.basis.attempt != Some(attempt)
            {
                return Err(invalid(
                    "native owner receipt changed before acknowledgement",
                ));
            }
            Ok(record)
        })
    }
    fn with_native_owner_consumption_proof<T>(
        &self,
        owner: &ProvisionedAttachment,
        request: &NativeTrustedConsumedStartRequest<'_>,
        trusted: &crate::TrustedReviewers,
        apply: impl FnOnce(NativeOwnerConsumptionProof<'_>) -> io::Result<T>,
    ) -> io::Result<T> {
        let zero = RecordDigest::from_bytes([0; 32]);
        if request.request == zero
            || request.operation == zero
            || request.grant == zero
            || request.available.len() > 256
            || request.source.id() == request.destination.id()
            || owner.id() == request.destination.id()
        {
            return Err(invalid("invalid native owner receipt selection"));
        }
        request.limits.validate()?;
        let verify_handles = || -> io::Result<()> {
            for work in [owner, request.source, request.destination]
                .into_iter()
                .chain(request.available.iter().copied())
            {
                self.exact_registered_work(work)?;
                if self.candidate_owning_root(work.id())? != owner.id() {
                    return Err(invalid("native owner receipt crosses registered ownership"));
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
            .ok_or_else(|| invalid("native owner destination registration missing"))?;
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
             state: &OwnerState,
             hints: super::super::super::dependency_catalog_discovery::CatalogHints| {
                selected_ids
                    .iter()
                    .map(|id| {
                        self.private_discovery_with_recovery(
                            owner,
                            id,
                            guard,
                            hints.clone(),
                            state.recovery(),
                        )
                    })
                    .collect::<io::Result<Vec<_>>>()
            };
        let discovery = {
            let guard =
                crate::workspace_custody::lock_workspace_initialization_set(&owner_selection.roots)
                    .map_err(error)?;
            let state = OwnerState::read(owner, request.request)?;
            discover(&guard, &state, hints)?
        };
        let ids = discovery
            .iter()
            .flat_map(|d| d.works.keys().cloned())
            .collect::<BTreeSet<_>>();
        if ids.len() > 256 || ids.contains(request.destination.id()) {
            return Err(invalid(
                "native owner source discovery exceeds its bound or includes destination",
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
            .ok_or_else(|| invalid("native owner receipt requires its original reservation"))?;
        if !super::super::super::dependency_reservation::is_reservation(&origin.value) {
            return Err(invalid(
                "native owner receipt requires its exact reservation",
            ));
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
            return Err(invalid("native owner receipt custody exceeds its bound"));
        }
        let guard =
            crate::workspace_custody::lock_workspace_initialization_set(&roots).map_err(error)?;
        // A new owner prefix is allowed only through the canonical writer's exact append proof.
        // Source registrations/closure and installed checkpoint remain fixed on every selection.
        let select = || -> io::Result<OwnerBasis> {
            verify_handles()?;
            for work in &works {
                self.exact_registered_work(work)?;
            }
            let current_origin = self
                .lane_origin_bound(request.destination)?
                .ok_or_else(|| invalid("native owner reservation disappeared"))?;
            if current_origin.value != origin.value
                || current_origin.allocation.identity()? != origin.allocation.identity()?
            {
                return Err(invalid("native owner receipt reservation changed"));
            }
            ensure_consumption_retention_available(
                owner,
                request.destination,
                &works,
                RetentionPhase::Owner,
                request.request,
            )?;
            let state = OwnerState::read(owner, request.request)?;
            let current_discovery = discover(&guard, &state, self.catalog_discovery_hints(owner)?)?;
            if current_discovery
                .iter()
                .flat_map(|d| d.works.keys().cloned())
                .collect::<BTreeSet<_>>()
                != ids
            {
                return Err(invalid("native owner receipt source closure changed"));
            }
            let context = PrivateContext::resolve_with_recovery(
                self,
                owner,
                &works,
                &guard,
                state.recovery(),
            )?;
            let graph = context.graph(self, request.source, request.operation, &guard)?;
            if graph.operation_count() > 256 {
                return Err(invalid("native owner inherited operations exceed bound"));
            }
            let source = context.binding(self, request.source, &guard)?;
            let destination = context.binding(self, request.destination, &guard)?;
            let snapshot = context.read(owner, &guard)?;
            let verify_permission = || -> io::Result<()> {
                let current = context.read(owner, &guard)?;
                if let OwnerState::Committed(receipt) = &state {
                    receipt.verify_context(owner, &current.proof)?;
                } else if current
                    .proof
                    .policy()
                    .native_request(request.request)
                    .is_some()
                    || !current.proof.policy().current_bound_grant(
                        (source.work(), source.installation(), request.operation),
                        (destination.work(), destination.installation()),
                        request.grant,
                        (source.correlation, destination.correlation),
                    )
                {
                    return Err(invalid(
                        "uncommitted native owner receipt needs its exact current grant",
                    ));
                }
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
                        let intent = candidate.native_owner_intent_held(self, &staged, &graph, held)?;
                        let value = Json::parse(&intent).map_err(error)?;
                        let body = value.get("body").ok_or_else(|| invalid("native owner intent body missing"))?;
                        state.verify_body(owner, &context, held, body)?;
                        verify_permission()?;
                        Ok(OwnerBasis { candidate, staged, graph: graph.clone(), version, source: source.clone(), destination: destination.clone(), owner: (snapshot.configuration.clone(), snapshot.proof.clone()), intent, receipt: state.receipt(), attempt: state.sidecar_identity(owner)? })
                    },
                )
            })
        };
        let basis = select()
            .map_err(|e| io::Error::new(e.kind(), format!("native owner selection: {e}")))?;
        let result = apply(NativeOwnerConsumptionProof {
            basis,
            select: &select,
        })?;
        guard.ensure_current().map_err(error)?;
        Ok(result)
    }
}
