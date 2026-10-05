//! Complete private content context. Structural publication records never become main authority.
use super::{
    consumption_prepare::{ConsumedHistorySelection, RecoveryMaterial, RecoveryPhase},
    dependency_closure::{private_publication_graph, PrivateGraphHistory},
    dependency_enrollment::read_private_in_store,
    dependency_owner_context::VerifiedHistoryRoots,
    dependency_transaction::{digest, hash},
    invalid, AttachmentStorage, NativeConsumedStartRequest, NativeDependencyGraph,
    NativeDependencyWorkBinding, NativeGrantInspection, ProvisionedAttachment,
    VerifiedPrivateHistory,
};
use crate::{
    ipc::Json, workspace::NativePrivateReviewHistory,
    workspace_custody::WorkspaceInitializationGuard,
};
use mesh_store::RecordDigest;
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
};
type Key = (RecordDigest, RecordDigest);
fn error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

struct Snapshot {
    work: ProvisionedAttachment,
    configuration: String,
    proof: VerifiedPrivateHistory,
    retained: VerifiedHistoryRoots,
}
struct PrivateContext<'a> {
    owner: &'a ProvisionedAttachment,
    histories: BTreeMap<String, Snapshot>,
}
// This proof can only be constructed by complete native private-context verification.
// It borrows the same custody set and re-verifies graph content at each use. It does not
// verify a human receipt itself and cannot authorize a write or ordinary workspace opening.
pub(crate) struct VerifiedConsumedPublicationGraph<'read, 'owner> {
    context: &'read PrivateContext<'owner>,
    storage: &'read AttachmentStorage,
    work: &'read ProvisionedAttachment,
    guard: &'read WorkspaceInitializationGuard,
    claim: crate::dependency_policy::NativePublicationClaim,
    graph: NativeDependencyGraph,
}
impl VerifiedConsumedPublicationGraph<'_, '_> {
    pub(crate) fn verify_for(
        &self,
        owner: &VerifiedPrivateHistory,
        claim: crate::dependency_policy::NativePublicationClaim,
    ) -> io::Result<&NativeDependencyGraph> {
        let current = self.context.read(self.context.owner, self.guard)?;
        if current.proof != *owner
            || self.claim != claim
            || owner.policy().publication_claim(claim.record.payload) != Some(claim)
            || self.graph.review_output() != claim.review.evidence().output()
            || self.graph.review_inputs().next().is_none()
            || owner
                .policy()
                .review_graph(claim.review.evidence().snapshot())
                != Some(self.graph.digest())
        {
            return Err(invalid(
                "consumed publication proof differs from its exact owner, inputs or snapshot",
            ));
        }
        if self.context.graph(
            self.storage,
            self.work,
            claim.review.evidence().output().2,
            self.guard,
        )? != self.graph
        {
            return Err(invalid("consumed publication input content changed"));
        }
        self.guard.ensure_current().map_err(error)?;
        Ok(&self.graph)
    }
}

impl<'a> PrivateContext<'a> {
    fn read(
        &self,
        work: &ProvisionedAttachment,
        guard: &WorkspaceInitializationGuard,
    ) -> io::Result<&Snapshot> {
        let selected = self
            .histories
            .get(work.id())
            .ok_or_else(|| invalid("private history is unresolved"))?;
        guard
            .require_roots(&[work.store.clone(), work.project().pinned.clone()])
            .map_err(error)?;
        if selected.work.store.identity()? != work.store.identity()?
            || selected.work.project().receipt()? != work.project().receipt()?
        {
            return Err(invalid("private history registration was replaced"));
        }
        selected.proof.verify_current(&work.store)?;
        if work.id() == self.owner.id() {
            let (configuration, proof) = work
                .project()
                .read_publication_private_history(work.metadata_path(), &work.store)?;
            if configuration != selected.configuration || proof != selected.proof {
                return Err(invalid("private owner history changed"));
            }
        } else {
            let (_, facts) =
                work.project()
                    .read_native_facts(work.metadata_path(), &work.store, None, None)?;
            if !facts
                .as_ref()
                .is_some_and(|f| selected.proof.matches_facts(f))
            {
                return Err(invalid("private consumed history changed"));
            }
        }
        work.project().history_configuration_with_previous(
            &work.store,
            None,
            Some(selected.configuration.clone()),
        )?;
        for (name, expected) in &selected.retained.sidecars {
            if hash(read_private_in_store(&work.store, name)?.as_bytes()) != *expected {
                return Err(invalid("private retained transaction changed"));
            }
        }
        guard.ensure_current().map_err(error)?;
        Ok(selected)
    }
    fn binding(
        &self,
        storage: &AttachmentStorage,
        work: &ProvisionedAttachment,
        guard: &WorkspaceInitializationGuard,
    ) -> io::Result<NativeDependencyWorkBinding> {
        let selected = storage.prepare_dependency_work(self.owner, work)?;
        if selected.has_legacy_copied_origin() {
            return Err(invalid(
                "private copied ancestry requires migration evidence",
            ));
        }
        let owner = self.read(self.owner, guard)?;
        if work.id() == self.owner.id() {
            return storage.validate_publication_root(&selected, guard, &owner.proof);
        }
        storage.validate_private_work_with_parents(
            &selected,
            guard,
            &owner.proof,
            |operation| {
                self.with_history(storage, self.owner, guard, |history| {
                    history.with_saved_input(operation, |_| Ok(()))
                })
            },
            |parent, version| {
                let operation = digest(version.strip_prefix("blake3:").unwrap_or(version))?;
                self.with_history(storage, parent, guard, |history| {
                    history.with_saved_input(operation, |_| Ok(()))
                })
            },
        )
    }
    fn with_history<T>(
        &self,
        storage: &AttachmentStorage,
        work: &ProvisionedAttachment,
        guard: &WorkspaceInitializationGuard,
        action: impl FnOnce(&NativePrivateReviewHistory<'_>) -> io::Result<T>,
    ) -> io::Result<T> {
        let selected = self.read(work, guard)?;
        let owner = self.read(self.owner, guard)?;
        let binding = self.binding(storage, work, guard)?;
        let workspace = mesh_operations::WorkspaceId::from_bytes(super::history::short_id(
            selected.configuration.as_bytes(),
        ));
        let history = NativePrivateReviewHistory::open(
            work.metadata_path(),
            work.store.clone(),
            &selected.proof,
            (&self.owner.store, &owner.proof),
            workspace,
            &binding,
            guard,
        )
        .map_err(error)?;
        let result = action(&history)?;
        self.read(work, guard)?;
        self.read(self.owner, guard)?;
        if self.binding(storage, work, guard)? != binding {
            return Err(invalid("private native correlation changed"));
        }
        Ok(result)
    }
    fn graph(
        &self,
        storage: &AttachmentStorage,
        work: &ProvisionedAttachment,
        operation: RecordDigest,
        guard: &WorkspaceInitializationGuard,
    ) -> io::Result<NativeDependencyGraph> {
        let owner = self.read(self.owner, guard)?;
        let mut bindings = BTreeMap::new();
        for snapshot in self.histories.values() {
            self.read(&snapshot.work, guard)?;
            let Ok(binding) = self.binding(storage, &snapshot.work, guard) else {
                // An independently enrolled descendant may require a consumed parent that
                // this pass has not resolved yet. Required graph edges still refuse missing
                // entries, and the completed context validates every selected binding.
                continue;
            };
            let key = (binding.work(), binding.installation());
            if bindings.insert(key, (snapshot, binding)).is_some() {
                return Err(invalid("ambiguous private native history"));
            }
        }
        let mut histories = BTreeMap::new();
        for (key, (snapshot, binding)) in &bindings {
            let workspace = mesh_operations::WorkspaceId::from_bytes(super::history::short_id(
                snapshot.configuration.as_bytes(),
            ));
            let history = NativePrivateReviewHistory::open(
                snapshot.work.metadata_path(),
                snapshot.work.store.clone(),
                &snapshot.proof,
                (&self.owner.store, &owner.proof),
                workspace,
                binding,
                guard,
            )
            .map_err(error)?;
            histories.insert(*key, (history, workspace));
        }
        let views = histories
            .iter()
            .map(|(key, (history, workspace))| {
                (
                    *key,
                    PrivateGraphHistory {
                        history,
                        binding: &bindings[key].1,
                        workspace: *workspace,
                    },
                )
            })
            .collect();
        let root = self.binding(storage, work, guard)?;
        let result = private_publication_graph(
            (root.work(), root.installation(), operation),
            &views,
            &owner.proof,
        )?;
        for snapshot in self.histories.values() {
            self.read(&snapshot.work, guard)?;
        }
        Ok(result)
    }
    fn consumed_publication_graph<'read>(
        &'read self,
        storage: &'read AttachmentStorage,
        claim: crate::dependency_policy::NativePublicationClaim,
        guard: &'read WorkspaceInitializationGuard,
    ) -> io::Result<VerifiedConsumedPublicationGraph<'read, 'a>> {
        let output = claim.review.evidence().output();
        let mut selected = None;
        for snapshot in self.histories.values() {
            let binding = self.binding(storage, &snapshot.work, guard)?;
            if (binding.work(), binding.installation()) == (output.0, output.1)
                && selected.replace(&snapshot.work).is_some()
            {
                return Err(invalid("consumed publication work is ambiguous"));
            }
        }
        let work = selected.ok_or_else(|| invalid("consumed publication work is unavailable"))?;
        let graph = self.graph(storage, work, output.2, guard)?;
        let proof = VerifiedConsumedPublicationGraph {
            context: self,
            storage,
            work,
            guard,
            claim,
            graph,
        };
        proof.verify_for(&self.read(self.owner, guard)?.proof, claim)?;
        Ok(proof)
    }

    fn with_replayed_history<T>(
        &self,
        storage: &AttachmentStorage,
        work: &ProvisionedAttachment,
        trusted: &crate::TrustedReviewers,
        guard: &WorkspaceInitializationGuard,
        inspect: impl FnOnce(&NativePrivateReviewHistory<'_>, &VerifiedPrivateHistory) -> io::Result<T>,
    ) -> io::Result<T> {
        let owner = self.read(self.owner, guard)?;
        let mut bindings = BTreeMap::new();
        for snapshot in self.histories.values() {
            self.read(&snapshot.work, guard)?;
            let binding = self.binding(storage, &snapshot.work, guard)?;
            if bindings
                .insert(
                    (binding.work(), binding.installation()),
                    (snapshot, binding),
                )
                .is_some()
            {
                return Err(invalid("ambiguous publication history"));
            }
        }
        let mut histories = BTreeMap::new();
        for (key, (snapshot, binding)) in &bindings {
            let workspace = mesh_operations::WorkspaceId::from_bytes(super::history::short_id(
                snapshot.configuration.as_bytes(),
            ));
            histories.insert(
                *key,
                NativePrivateReviewHistory::open(
                    snapshot.work.metadata_path(),
                    snapshot.work.store.clone(),
                    &snapshot.proof,
                    (&self.owner.store, &owner.proof),
                    workspace,
                    binding,
                    guard,
                )
                .map_err(error)?,
            );
        }
        let cas = mesh_cas::Cas::<_, mesh_cas::Blake3>::with_filesystem(
            self.owner.metadata_path(),
            self.owner.store.filesystem().read_only(),
        )
        .map_err(error)?;
        let claims = owner.proof.policy().publication_claims_in_order();
        if claims.len() > 4096 {
            return Err(invalid("publication replay exceeds its bound"));
        }
        for claim in claims {
            let output = claim.review.evidence().output();
            let key = (output.0, output.1);
            let (snapshot, _) = bindings
                .get(&key)
                .ok_or_else(|| invalid("publication output history is unavailable"))?;
            let graph = self.graph(storage, &snapshot.work, output.2, guard)?;
            let receipt = super::dependency_transaction::read_payload(&cas, claim.receipt, 65536)?;
            let history = histories
                .get_mut(&key)
                .ok_or_else(|| invalid("publication history disappeared"))?;
            if graph.review_inputs().next().is_some() {
                let complete = self.consumed_publication_graph(storage, claim, guard)?;
                history
                    .replay_consumed_publication(claim, &complete, &receipt, trusted)
                    .map_err(error)?;
            } else {
                history
                    .replay_publication(claim, &graph, &receipt, trusted)
                    .map_err(error)?;
            }
        }
        let selected = self.binding(storage, work, guard)?;
        let history = histories
            .get(&(selected.work(), selected.installation()))
            .ok_or_else(|| invalid("selected publication history is unavailable"))?;
        let result = inspect(history, &owner.proof)?;
        for snapshot in self.histories.values() {
            self.read(&snapshot.work, guard)?;
        }
        guard.ensure_current().map_err(error)?;
        Ok(result)
    }

    fn resolve(
        storage: &AttachmentStorage,
        owner: &'a ProvisionedAttachment,
        works: &[ProvisionedAttachment],
        guard: &WorkspaceInitializationGuard,
    ) -> io::Result<Self> {
        let (configuration, proof) = owner
            .project()
            .read_publication_private_history(owner.metadata_path(), &owner.store)?;
        let mut context = Self {
            owner,
            histories: BTreeMap::from([(
                owner.id().to_owned(),
                Snapshot {
                    work: owner.clone(),
                    configuration,
                    proof,
                    retained: VerifiedHistoryRoots::default(),
                },
            )]),
        };
        let mut pending = Vec::new();
        for work in works {
            if work.id() == owner.id() {
                continue;
            }
            match work
                .project()
                .read_publication_private_history(work.metadata_path(), &work.store)
            {
                Ok((configuration, proof)) => {
                    context.histories.insert(
                        work.id().to_owned(),
                        Snapshot {
                            work: work.clone(),
                            configuration,
                            proof,
                            retained: VerifiedHistoryRoots::default(),
                        },
                    );
                }
                Err(_) => pending.push(ConsumedHistorySelection::read(work)?),
            }
        }
        while !pending.is_empty() {
            let before = pending.len();
            let mut remaining = Vec::new();
            for selection in pending {
                let attempt = (|| {
                    let source = context
                        .histories
                        .values()
                        .filter_map(|snapshot| {
                            context
                                .binding(storage, &snapshot.work, guard)
                                .ok()
                                .filter(|b| (b.work(), b.installation()) == selection.source)
                                .map(|_| &snapshot.work)
                        })
                        .collect::<Vec<_>>();
                    let [source] = source.as_slice() else {
                        return Err(invalid("private source unavailable or ambiguous"));
                    };
                    let source_binding = context.binding(storage, source, guard)?;
                    let destination_binding = context.binding(storage, selection.work, guard)?;
                    let version = context.with_history(storage, source, guard, |h| {
                        h.saved_version(selection.operation)
                    })?;
                    let graph = context.graph(storage, source, selection.operation, guard)?;
                    let available = context
                        .histories
                        .values()
                        .map(|s| &s.work)
                        .collect::<Vec<_>>();
                    let owner_snapshot = context.read(owner, guard)?;
                    let verify_source = |operation| -> io::Result<()> {
                        let proof = &context.read(owner, guard)?.proof;
                        let record = proof
                            .policy()
                            .native_request(selection.request)
                            .ok_or_else(|| invalid("private consumption receipt missing"))?;
                        if record.kind != mesh_store::DependencyKind::Consumption
                            || !proof.policy().consumption_facts().iter().any(|f| {
                                f.record == record.payload
                                    && f.start
                                        == (
                                            destination_binding.work(),
                                            destination_binding.installation(),
                                            operation,
                                        )
                                    && f.source
                                        == (
                                            source_binding.work(),
                                            source_binding.installation(),
                                            selection.operation,
                                        )
                                    && f.grant == selection.grant
                                    && f.bindings
                                        == Some((
                                            source_binding.correlation,
                                            destination_binding.correlation,
                                        ))
                            })
                        {
                            return Err(invalid("private historical source receipt differs"));
                        }
                        context.with_history(storage, source, guard, |h| {
                            h.with_saved_input(selection.operation, |_| Ok(()))
                        })
                    };
                    storage.with_recovered_consumed_material(RecoveryMaterial {
                        owner, request: NativeConsumedStartRequest { input: NativeGrantInspection {
                            source, version, destination: selection.work, grant: selection.grant,
                        }, available: &available, request: selection.request, limits: selection.limits },
                        phase: RecoveryPhase::CompletedRead, graph: &graph, source_binding: &source_binding,
                        destination_binding: &destination_binding, owner_binding: owner_snapshot.proof.binding(),
                    }, guard, |operation, expected| {
                        verify_source(operation)?;
                        context.with_history(storage, source, guard, |h| h.with_saved_input(selection.operation, |input| {
                            super::consumption_prepare::source_verification::verify_starting_source(selection.work, &input, guard, expected)
                        }))
                    }, verify_source, |candidate, staged, graph, held| {
                        let (configuration, proof) = candidate.verify_completed_private_history(graph, held, None, || {
                            let snapshot = context.read(owner, held)?;
                            Ok((snapshot.configuration.clone(), snapshot.proof.clone()))
                        })?;
                        let retained = candidate.completed_graph_roots(&staged, graph)?;
                        Ok(Snapshot { work: selection.work.clone(), configuration, proof, retained })
                    })
                })();
                match attempt {
                    Ok(snapshot) => {
                        context
                            .histories
                            .insert(selection.work.id().to_owned(), snapshot);
                    }
                    Err(_) => remaining.push(selection),
                }
            }
            if remaining.len() == before {
                return Err(invalid(
                    "private consumed histories cannot be resolved completely",
                ));
            }
            pending = remaining;
        }
        for snapshot in context.histories.values() {
            context.read(&snapshot.work, guard)?;
            context.binding(storage, &snapshot.work, guard)?;
        }
        Ok(context)
    }
}

#[derive(PartialEq, Eq)]
struct Discovery {
    configuration: String,
    owner: VerifiedPrivateHistory,
    works: BTreeMap<String, Key>,
}
impl AttachmentStorage {
    fn private_discovery(
        &self,
        owner: &ProvisionedAttachment,
        work_id: &str,
        guard: &WorkspaceInitializationGuard,
        catalog: super::dependency_catalog_discovery::CatalogHints,
    ) -> io::Result<Discovery> {
        let selected = self.prepare_dependency_work(owner, owner)?;
        guard.require_roots(&selected.roots).map_err(error)?;
        let (configuration, proof) = owner
            .project()
            .read_publication_private_history(owner.metadata_path(), &owner.store)?;
        let (hints, keys) = catalog;
        let exact = |key: Key| -> io::Result<String> {
            let matches = keys
                .get(&key)
                .ok_or_else(|| invalid("required private input is unavailable"))?;
            let [id] = matches.as_slice() else {
                return Err(invalid("required private input is ambiguous"));
            };
            Ok(id.clone())
        };
        let mut pending = BTreeSet::from([owner.id().to_owned(), work_id.to_owned()]);
        // Include every historical publication output and its source closure before custody.
        // A structurally valid claim selects work only; it is not accepted-main evidence.
        for claim in proof.policy().publication_claims_in_order() {
            let output = claim.review.evidence().output();
            pending.insert(exact((output.0, output.1))?);
        }
        let facts = proof.policy().consumption_facts();
        let mut works = BTreeMap::new();
        while let Some(id) = pending.pop_first() {
            if works.contains_key(&id) {
                continue;
            }
            if works.len() >= 256 {
                return Err(invalid("private discovery exceeds native work bound"));
            }
            let (key, parents) = hints
                .get(&id)
                .ok_or_else(|| invalid("required private registration unavailable"))?;
            if exact(*key)? != id {
                return Err(invalid("private registration binding differs"));
            }
            works.insert(id, *key);
            pending.extend(parents.iter().cloned());
            for fact in facts.iter().filter(|f| (f.start.0, f.start.1) == *key) {
                if fact.bindings.is_none() {
                    return Err(invalid("private consumption has no native correlation"));
                }
                for input in std::iter::once(&fact.source).chain(&fact.inputs) {
                    pending.insert(exact((input.0, input.1))?);
                }
            }
        }
        Ok(Discovery {
            configuration,
            owner: proof,
            works,
        })
    }
    /// Inspect immutable private content and consumed-input provenance from native registration.
    /// Publication claims select required history but do not assert accepted main or grant writes.
    /// The complete bounded root set is acquired before reconstruction and checked again afterward.
    pub fn inspect_private_dependency_graph(
        &self,
        work_id: &str,
        operation: RecordDigest,
    ) -> io::Result<Json> {
        self.with_private_inspection(work_id, |context, work, guard| {
            context
                .graph(self, work, operation, guard)
                .map(|graph| graph.to_json())
        })
    }

    // Repeat the complete read after reconstructing retained material. A failed second read
    // never acknowledges the first result, and no workspace escapes the custody boundary.
    fn with_private_inspection<T: PartialEq>(
        &self,
        work_id: &str,
        inspect: impl Fn(
            &PrivateContext<'_>,
            &ProvisionedAttachment,
            &WorkspaceInitializationGuard,
        ) -> io::Result<T>,
    ) -> io::Result<T> {
        let owner = self.reopen(&self.candidate_owning_root(work_id)?)?;
        let work = self.reopen(work_id)?;
        let owner_selection = self.prepare_dependency_work(&owner, &owner)?;
        // Finish candidate catalog pinning before the initial owner-only lock, matching ordinary discovery.
        let hints = self.catalog_discovery_hints(&owner)?;
        let discovery = {
            let guard =
                crate::workspace_custody::lock_workspace_initialization_set(&owner_selection.roots)
                    .map_err(error)?;
            self.private_discovery(&owner, work_id, &guard, hints)?
        };
        let works = discovery
            .works
            .keys()
            .map(|id| self.reopen(id))
            .collect::<io::Result<Vec<_>>>()?;
        let available = works.iter().collect::<Vec<_>>();
        let prepared = self.prepare_dependency_graph(
            &owner,
            &work,
            RecordDigest::from_bytes([0; 32]),
            &available,
        )?;
        let guard = crate::workspace_custody::lock_workspace_initialization_set(&prepared.roots)
            .map_err(error)?;
        if self.private_discovery(
            &owner,
            work_id,
            &guard,
            self.catalog_discovery_hints(&owner)?,
        )? != discovery
        {
            return Err(invalid("private input discovery changed"));
        }
        let context = PrivateContext::resolve(self, &owner, &works, &guard)?;
        let result = inspect(&context, &work, &guard)?;
        if self.private_discovery(
            &owner,
            work_id,
            &guard,
            self.catalog_discovery_hints(&owner)?,
        )? != discovery
        {
            return Err(invalid("private input discovery changed during inspection"));
        }
        // Reconstruct again to verify retained material as well as journal/sidecar identity.
        let after = PrivateContext::resolve(self, &owner, &works, &guard)?;
        if inspect(&after, &work, &guard)? != result {
            return Err(invalid("private content changed during inspection"));
        }
        guard.ensure_current().map_err(error)?;
        Ok(result)
    }

    pub(super) fn with_native_publication_history<T: PartialEq>(
        &self,
        work_id: &str,
        trusted: &crate::TrustedReviewers,
        inspect: impl Fn(&NativePrivateReviewHistory<'_>, &VerifiedPrivateHistory) -> io::Result<T>,
    ) -> io::Result<T> {
        self.with_private_inspection(work_id, |context, work, guard| {
            context.with_replayed_history(self, work, trusted, guard, &inspect)
        })
    }

    /// Replay owning-project publications with configured human receipt trust and complete
    /// consumed-input proofs. This is read-only accepted-main evidence, never permission
    /// to publish, mutate, launch work or bypass human presence for a new decision.
    pub fn inspect_native_publication_history(
        &self,
        work_id: &str,
        trusted: &crate::TrustedReviewers,
    ) -> io::Result<Json> {
        self.with_native_publication_history(work_id, trusted, |history, _| {
            Ok(Json::object([
                ("schema", Json::text("mesh.native-publication-history/v1")),
                (
                    "publication",
                    history.verified_publication().map_or(Json::Null, |claim| {
                        Json::object([
                            ("head", Json::text(claim.result.to_hex())),
                            (
                                "operation",
                                Json::text(claim.review.evidence().output().2.to_hex()),
                            ),
                            ("revision", Json::Number(claim.revision)),
                            ("receipt", Json::text(claim.receipt.to_hex())),
                        ])
                    }),
                ),
            ]))
        })
    }

    /// Reconstruct a prospective review against independently verified native main. This
    /// saves no review and requests no signature; its snapshot must match the complete graph.
    pub fn inspect_native_review_candidate(
        &self,
        work_id: &str,
        snapshot: RecordDigest,
        trusted: &crate::TrustedReviewers,
    ) -> io::Result<Json> {
        self.with_private_inspection(work_id, |context, work, guard| {
            context.with_replayed_history(self, work, trusted, guard, |history, owner| {
                let review = owner
                    .policy()
                    .review_evidence(snapshot)
                    .ok_or_else(|| invalid("native review snapshot is unavailable"))?;
                let binding = context.binding(self, work, guard)?;
                let output = review.output();
                if (output.0, output.1) != (binding.work(), binding.installation()) {
                    return Err(invalid("native review snapshot belongs to another work"));
                }
                let graph = context.graph(self, work, output.2, guard)?;
                if owner.policy().review_graph(snapshot) != Some(graph.digest()) {
                    return Err(invalid(
                        "native review snapshot differs from complete content",
                    ));
                }
                let (canonical, bundle) = history.current_review_bundle(&review).map_err(error)?;
                Ok(Json::object([
                    ("schema", Json::text("mesh.native-review-candidate/v1")),
                    (
                        "canonical",
                        Json::text(RecordDigest::from_bytes(*canonical.as_bytes()).to_hex()),
                    ),
                    ("bundle", Json::text(bundle.to_hex())),
                    ("snapshot", Json::text(snapshot.to_hex())),
                ]))
            })
        })
    }
}
