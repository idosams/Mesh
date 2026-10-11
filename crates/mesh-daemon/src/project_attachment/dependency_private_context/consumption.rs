//! Exact uncommitted consumption inventories. No recovery or admission escapes.
use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum RetentionPhase {
    Start,
    Installed,
}

fn ensure_consumption_retention_available(
    owner: &ProvisionedAttachment,
    destination: &ProvisionedAttachment,
    works: &[ProvisionedAttachment],
    phase: RetentionPhase,
    request: RecordDigest,
) -> io::Result<()> {
    for work in works.iter().chain([owner, destination]) {
        super::super::detachment::ensure_attached(&work.store)?;
        for pending in [
            super::super::dependency_decision::PENDING,
            super::super::history::dependency_capture::PENDING,
            super::publication::PENDING,
        ] {
            super::super::history::dependency_capture::absent(&work.store, pending)?;
        }
    }
    let pending = if phase == RetentionPhase::Start {
        vec![
            super::super::consumption_history::PENDING,
            "consumption-owner.pending",
        ]
    } else {
        let prefix = format!("consumption-owner-{}-", request.to_hex());
        use std::os::unix::ffi::OsStrExt as _;
        if owner
            .store
            .filesystem()
            .read_directory_names_bounded(std::path::Path::new(""), 16384)?
            .iter()
            .any(|name| name.as_bytes().starts_with(prefix.as_bytes()))
        {
            return Err(invalid(
                "installed retention precedes any owner receipt attempt",
            ));
        }
        vec![]
    };
    for pending in pending
        .into_iter()
        .chain([super::super::consumption_complete::PENDING])
    {
        super::super::history::dependency_capture::absent(&destination.store, pending)?;
    }
    Ok(())
}
impl AttachmentStorage {
    /// Inspect one exact retained start while its reserved destination is still empty.
    /// Historical publication is verified with configured trust. This grants no install,
    /// recovery, saved-history admission, run, deletion or protected-main authority.
    pub fn inspect_native_consumption_start_retention(
        &self,
        owner: &ProvisionedAttachment,
        request: NativeConsumedStartRequest<'_>,
        trusted: &crate::TrustedReviewers,
    ) -> io::Result<Json> {
        self.inspect_native_consumption_retention(owner, request, trusted, RetentionPhase::Start)
    }

    /// Inspect retained installation and checkpoint bytes before any owner receipt attempt.
    /// Explicit publication trust and the current exact grant remain required. No candidate,
    /// history/runtime admission, recovery, protected-main or deletion authority is returned.
    pub fn inspect_native_consumption_install_retention(
        &self,
        owner: &ProvisionedAttachment,
        request: NativeConsumedStartRequest<'_>,
        trusted: &crate::TrustedReviewers,
    ) -> io::Result<Json> {
        self.inspect_native_consumption_retention(
            owner,
            request,
            trusted,
            RetentionPhase::Installed,
        )
    }

    fn inspect_native_consumption_retention(
        &self,
        owner: &ProvisionedAttachment,
        request: NativeConsumedStartRequest<'_>,
        trusted: &crate::TrustedReviewers,
        phase: RetentionPhase,
    ) -> io::Result<Json> {
        request.limits.validate()?;
        let source = request.input.source;
        let destination = request.input.destination;
        if request.request == RecordDigest::from_bytes([0; 32])
            || request.available.len() > 256
            || source.id() == destination.id()
            || owner.id() == destination.id()
        {
            return Err(invalid("invalid native consumption retention selection"));
        }
        for work in [owner, source, destination]
            .into_iter()
            .chain(request.available.iter().copied())
        {
            self.exact_registered_work(work)?;
            if self.candidate_owning_root(work.id())? != owner.id() {
                return Err(invalid(
                    "consumption retention crosses registered ownership",
                ));
            }
        }
        let selected_ids = std::iter::once(source.id())
            .chain(request.available.iter().map(|work| work.id()))
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        if selected_ids.contains(destination.id()) {
            return Err(invalid("pending destination cannot supply source history"));
        }
        // Catalog pinning precedes owner custody; the complete discovered set is acquired below.
        let hints = self.catalog_discovery_hints(owner)?;
        let owner_selection = self.prepare_dependency_work(owner, owner)?;
        let discovery = {
            let guard =
                crate::workspace_custody::lock_workspace_initialization_set(&owner_selection.roots)
                    .map_err(error)?;
            selected_ids
                .iter()
                .map(|id| self.private_discovery(owner, id, &guard, hints.clone()))
                .collect::<io::Result<Vec<_>>>()?
        };
        let ids = discovery
            .iter()
            .flat_map(|entry| entry.works.keys().cloned())
            .collect::<BTreeSet<_>>();
        if ids.len() > 256 || ids.contains(destination.id()) {
            return Err(invalid(
                "consumption retention source set is incomplete or exceeds bound",
            ));
        }
        let works = ids
            .iter()
            .map(|id| self.reopen(id))
            .collect::<io::Result<Vec<_>>>()?;
        let available = works
            .iter()
            .chain(std::iter::once(destination))
            .collect::<Vec<_>>();
        let prepared = self.prepare_dependency_graph(
            owner,
            source,
            request.input.version.operation(),
            &available,
        )?;
        let destination_selection = self.prepare_dependency_work(owner, destination)?;
        let guard = crate::workspace_custody::lock_workspace_initialization_set(&prepared.roots)
            .map_err(error)?;
        guard
            .require_roots(&destination_selection.roots)
            .map_err(error)?;
        let rediscover = || {
            let hints = self.catalog_discovery_hints(owner)?;
            selected_ids
                .iter()
                .map(|id| self.private_discovery(owner, id, &guard, hints.clone()))
                .collect::<io::Result<Vec<_>>>()
        };
        if rediscover()? != discovery {
            return Err(invalid("consumption retention discovery changed"));
        }
        ensure_consumption_retention_available(owner, destination, &works, phase, request.request)?;
        let context = PrivateContext::resolve(self, owner, &works, &guard)?;
        let inspect = || -> io::Result<Json> {
            ensure_consumption_retention_available(
                owner,
                destination,
                &works,
                phase,
                request.request,
            )?;
            for work in works.iter().chain(std::iter::once(destination)) {
                self.exact_registered_work(work)?;
            }
            // Each physical source/owner inventory independently verifies all historical receipts
            // and authored content with configured trust. A selected intent is not accepted main.
            let stores = works
                .iter()
                .map(|work| context.native_history_retention(self, work, trusted, &guard, None))
                .collect::<io::Result<Vec<_>>>()?;
            let graph = context.graph(self, source, request.input.version.operation(), &guard)?;
            let source_binding = context.binding(self, source, &guard)?;
            let owner_snapshot = context.read(owner, &guard)?;
            let destination_binding = self.validate_private_work_with_parents(
                &destination_selection,
                &guard,
                &owner_snapshot.proof,
                |operation| {
                    context.with_history(self, owner, &guard, |history| {
                        history.with_saved_input(operation, |_| Ok(()))
                    })
                },
                |parent, version| {
                    let operation = digest(version.strip_prefix("blake3:").unwrap_or(version))?;
                    context.with_history(self, parent, &guard, |history| {
                        history.with_saved_input(operation, |_| Ok(()))
                    })
                },
            )?;
            let verify_source = |_operation| -> io::Result<()> {
                let current = context.read(owner, &guard)?;
                if current
                    .proof
                    .policy()
                    .native_request(request.request)
                    .is_some()
                    || !current.proof.policy().current_bound_grant(
                        (
                            source_binding.work(),
                            source_binding.installation(),
                            request.input.version.operation(),
                        ),
                        (
                            destination_binding.work(),
                            destination_binding.installation(),
                        ),
                        request.input.grant,
                        (source_binding.correlation, destination_binding.correlation),
                    )
                {
                    return Err(invalid(
                        "consumption start requires its exact current bound grant",
                    ));
                }
                context.with_history(self, source, &guard, |history| {
                    history.with_saved_input(request.input.version.operation(), |_| Ok(()))
                })
            };
            let destination_inventory = self.with_recovered_consumed_material(
                RecoveryMaterial {
                    owner, request: NativeConsumedStartRequest {
                        input: NativeGrantInspection {source, version: request.input.version, destination, grant: request.input.grant},
                        available: request.available, request: request.request, limits: request.limits,
                    }, phase: if phase == RetentionPhase::Start { RecoveryPhase::Start } else { RecoveryPhase::Installed }, graph: &graph, source_binding: &source_binding,
                    destination_binding: &destination_binding, owner_binding: owner_snapshot.proof.binding(),
                }, &guard,
                |operation, expected| {
                    verify_source(operation)?;
                    context.with_history(self, source, &guard, |history| history.with_saved_input(request.input.version.operation(), |input| {
                        super::super::consumption_prepare::source_verification::verify_starting_source(destination, &input, &guard, expected)
                    }))
                }, verify_source,
                |candidate, staged, graph, held| match phase { RetentionPhase::Start => candidate.retained_start_roots(self, &staged, graph, held), RetentionPhase::Installed => candidate.retained_installed_roots(self, &staged, graph, held) },
            )?;
            if context.graph(self, source, request.input.version.operation(), &guard)? != graph
                || context.binding(self, source, &guard)? != source_binding
            {
                return Err(invalid("consumption retention source changed"));
            }
            let result = Json::object([
                (
                    "schema",
                    Json::text(match phase {
                        RetentionPhase::Start => "mesh.native-consumption-start-retention/v1",
                        RetentionPhase::Installed => "mesh.native-consumption-install-retention/v1",
                    }),
                ),
                (
                    "scope",
                    Json::text(match phase {
                        RetentionPhase::Start => "empty-destination-start-recovery",
                        RetentionPhase::Installed => "installation-checkpoint-before-owner",
                    }),
                ),
                ("collection_authority", Json::Bool(false)),
                ("request", Json::text(request.request.to_hex())),
                (
                    "source",
                    Json::object([
                        ("work", Json::text(source_binding.work().to_hex())),
                        (
                            "installation",
                            Json::text(source_binding.installation().to_hex()),
                        ),
                        (
                            "operation",
                            Json::text(request.input.version.operation().to_hex()),
                        ),
                        ("grant", Json::text(request.input.grant.to_hex())),
                        ("closure", Json::text(graph.digest().to_hex())),
                    ]),
                ),
                ("stores", Json::Array(stores)),
                ("destination", destination_inventory),
            ]);
            if result.encode().len() > 4 * 1024 * 1024 {
                return Err(invalid("consumption retention encoding exceeds bound"));
            }
            ensure_consumption_retention_available(
                owner,
                destination,
                &works,
                phase,
                request.request,
            )?;
            Ok(result)
        };
        let result = inspect()?;
        if inspect()? != result || rediscover()? != discovery {
            return Err(invalid("consumption retention changed during inspection"));
        }
        ensure_consumption_retention_available(owner, destination, &works, phase, request.request)?;
        self.exact_registered_work(owner)?;
        self.exact_registered_work(destination)?;
        guard.ensure_current().map_err(error)?;
        Ok(result)
    }
}
