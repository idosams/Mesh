//! Native immutable graph inspection. This is neither consumption nor a collection/publication permit.
use super::dependency_transaction::hash;
use super::{invalid, AttachmentStorage, ProvisionedAttachment, SavedAttachmentVersion};
use crate::{
    dependency_policy::{NativeConsumptionFact, QualifiedDependencyInput as Input},
    ipc::Json,
};
use mesh_store::RecordDigest;
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
};
const MAX_NODES: usize = 4096;
const MAX_EDGES: usize = 16384;
const MAX_BYTES: usize = 4 * 1024 * 1024;
fn error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
fn input_json(i: Input) -> Json {
    Json::Array(vec![
        Json::text(i.0.to_hex()),
        Json::text(i.1.to_hex()),
        Json::text(i.2.to_hex()),
    ])
}

/// Read-only historical graph. It grants no current access, eligibility or collection authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeDependencyGraph {
    root: Input,
    nodes: BTreeMap<Input, Node>,
    digest: RecordDigest,
    retained: BTreeMap<(RecordDigest, RecordDigest), StoreFacts>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct StoreFacts {
    physical: (u64, u64),
    sidecars: BTreeMap<String, RecordDigest>,
    correlation: RecordDigest,
    payloads: BTreeSet<RecordDigest>,
    manifests: BTreeSet<RecordDigest>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Node {
    parents: BTreeSet<Input>,
    manifests: BTreeSet<RecordDigest>,
    chunks: BTreeSet<RecordDigest>,
    consumption: Option<NativeConsumptionFact>,
}
impl NativeDependencyGraph {
    /// Canonical complete graph digest, independent of work-list and traversal ordering.
    pub fn digest(&self) -> RecordDigest {
        self.digest
    }
    pub(super) fn consumption_inputs_json(&self) -> Json {
        Json::Array(
            self.nodes
                .keys()
                .map(|i| {
                    Json::Array(vec![
                        Json::Array(vec![Json::text(i.0.to_hex()), Json::text(i.1.to_hex())]),
                        Json::text(i.2.to_hex()),
                    ])
                })
                .collect(),
        )
    }
    /// Number of exact operation identities, including the selected root.
    pub fn operation_count(&self) -> usize {
        self.nodes.len()
    }
    /// Verified content and policy objects for this selected graph, qualified by native store.
    /// Includes selected capture receipts and fully journaled pending capture/control evidence.
    /// Torn journals still require the separate exact recovery inspectors. Unselected versions
    /// are excluded. This is not a complete-store collection oracle or a durable pin.
    pub fn retained_content_json(&self) -> Json {
        Json::object([
            ("schema", Json::text("mesh.native-dependency-content/v1")),
            ("graph", Json::text(self.digest.to_hex())),
            (
                "stores",
                Json::Array(
                    self.retained
                        .iter()
                        .map(|(work, facts)| {
                            Json::object([
                                ("work", Json::text(work.0.to_hex())),
                                ("installation", Json::text(work.1.to_hex())),
                                ("device", Json::text(format!("{:x}", facts.physical.0))),
                                ("inode", Json::text(format!("{:x}", facts.physical.1))),
                                ("correlation", Json::text(facts.correlation.to_hex())),
                                (
                                    "sidecars",
                                    Json::Array(
                                        facts
                                            .sidecars
                                            .iter()
                                            .map(|(name, digest)| {
                                                Json::object([
                                                    ("name", Json::text(name)),
                                                    ("digest", Json::text(digest.to_hex())),
                                                ])
                                            })
                                            .collect(),
                                    ),
                                ),
                                (
                                    "payloads",
                                    Json::Array(
                                        facts
                                            .payloads
                                            .iter()
                                            .map(|p| Json::text(p.to_hex()))
                                            .collect(),
                                    ),
                                ),
                                (
                                    "manifests",
                                    Json::Array(
                                        facts
                                            .manifests
                                            .iter()
                                            .map(|p| Json::text(p.to_hex()))
                                            .collect(),
                                    ),
                                ),
                            ])
                        })
                        .collect(),
                ),
            ),
        ])
    }
    /// Immutable graph facts only. No live working-directory content is included.
    pub fn to_json(&self) -> Json {
        Json::object([
            ("schema", Json::text("mesh.native-dependency-graph/v1")),
            ("root", input_json(self.root)),
            (
                "nodes",
                Json::Array(
                    self.nodes
                        .iter()
                        .map(|(id, n)| {
                            Json::object([
                                ("input", input_json(*id)),
                                (
                                    "parents",
                                    Json::Array(
                                        n.parents.iter().copied().map(input_json).collect(),
                                    ),
                                ),
                                (
                                    "manifests",
                                    Json::Array(
                                        n.manifests
                                            .iter()
                                            .map(|m| Json::text(m.to_hex()))
                                            .collect(),
                                    ),
                                ),
                                (
                                    "chunks",
                                    Json::Array(
                                        n.chunks.iter().map(|c| Json::text(c.to_hex())).collect(),
                                    ),
                                ),
                                (
                                    "consumption",
                                    n.consumption
                                        .as_ref()
                                        .map_or(Json::Null, |c| Json::text(c.record.to_hex())),
                                ),
                            ])
                        })
                        .collect(),
                ),
            ),
        ])
    }
}
fn walk(
    root: Input,
    mut load: impl FnMut(Input) -> io::Result<Node>,
    max_nodes: usize,
    max_edges: usize,
    max_depth: usize,
    max_bytes: usize,
) -> io::Result<NativeDependencyGraph> {
    if max_nodes == 0 || max_depth == 0 {
        return Err(invalid("missing graph budget"));
    }
    let mut nodes = BTreeMap::<Input, Node>::new();
    let mut active = BTreeSet::new();
    let mut heights = BTreeMap::<Input, usize>::new();
    let mut stack = vec![(root, false)];
    let mut edges = 0usize;
    let mut references = 0usize;
    while let Some((id, finish)) = stack.pop() {
        if finish {
            let node = nodes
                .get(&id)
                .ok_or_else(|| invalid("incomplete dependency traversal"))?;
            let height = node.parents.iter().try_fold(1usize, |h, p| {
                heights
                    .get(p)
                    .and_then(|n| n.checked_add(1))
                    .map(|n| h.max(n))
                    .ok_or_else(|| invalid("dependency cycle or missing parent"))
            })?;
            if height > max_depth {
                return Err(invalid("dependency depth exceeds its bound"));
            }
            heights.insert(id, height);
            active.remove(&id);
            continue;
        }
        if heights.contains_key(&id) {
            continue;
        }
        if !active.insert(id) {
            return Err(invalid("dependency cycle"));
        }
        if nodes.len() >= max_nodes {
            return Err(invalid("dependency node count exceeds its bound"));
        }
        let node = load(id)?;
        references = references
            .checked_add(node.manifests.len())
            .and_then(|n| n.checked_add(node.chunks.len()))
            .filter(|n| *n <= max_bytes / 64)
            .ok_or_else(|| invalid("dependency content references exceed their bound"))?;
        edges = edges
            .checked_add(node.parents.len())
            .filter(|n| *n <= max_edges)
            .ok_or_else(|| invalid("dependency edge count exceeds its bound"))?;
        stack.push((id, true));
        for parent in node.parents.iter().rev() {
            stack.push((*parent, false));
        }
        nodes.insert(id, node);
    }
    // A receipt's flat declaration is checked against independently traversed source ancestry.
    for node in nodes.values() {
        if let Some(receipt) = &node.consumption {
            let mut seen = BTreeSet::new();
            let mut pending = vec![receipt.source];
            while let Some(id) = pending.pop() {
                if !seen.insert(id) {
                    continue;
                }
                if seen.len() > receipt.inputs.len() {
                    return Err(invalid("consumption omits inherited inputs"));
                }
                let n = nodes
                    .get(&id)
                    .ok_or_else(|| invalid("consumption input is missing"))?;
                pending.extend(n.parents.iter().copied());
            }
            if seen.into_iter().collect::<Vec<_>>() != receipt.inputs {
                return Err(invalid("consumption has conflicting inherited inputs"));
            }
        }
    }
    let mut graph = NativeDependencyGraph {
        root,
        nodes,
        digest: RecordDigest::from_bytes([0; 32]),
        retained: BTreeMap::new(),
    };
    let bytes = graph.to_json().encode();
    if bytes.len() > max_bytes {
        return Err(invalid("dependency graph encoding exceeds its bound"));
    }
    graph.digest = hash(bytes.as_bytes());
    Ok(graph)
}
// Native selection and the entire required lock set. Preparation grants no authority; validation
// must run after acquisition and repeat all physical/history checks under that same guard.
pub(super) struct PreparedDependencyGraph<'a> {
    owner: &'a ProvisionedAttachment,
    source: &'a ProvisionedAttachment,
    operation: RecordDigest,
    selections: Vec<(
        &'a ProvisionedAttachment,
        super::dependency_work::PreparedDependencyWork,
    )>,
    pub(super) roots: Vec<crate::root_authority::PinnedWorkspaceRoot>,
}

impl AttachmentStorage {
    /// Inspect all signed operation and owner-recorded consumed-input edges. `available` supplies
    /// native handles, never an asserted input list. Omitted referenced work refuses the whole read.
    /// Legacy histories without established input provenance require explicit migration evidence.
    /// This development API does not authorize consumption, publication, or collection.
    pub fn inspect_dependency_graph(
        &self,
        owner: &ProvisionedAttachment,
        source: &ProvisionedAttachment,
        version: SavedAttachmentVersion,
        available: &[&ProvisionedAttachment],
    ) -> io::Result<NativeDependencyGraph> {
        self.inspect_dependency_operation_graph(owner, source, version.operation(), available)
    }

    // General native operation roots need not belong to the capture-only linear history API.
    pub(super) fn inspect_dependency_operation_graph(
        &self,
        owner: &ProvisionedAttachment,
        source: &ProvisionedAttachment,
        operation: RecordDigest,
        available: &[&ProvisionedAttachment],
    ) -> io::Result<NativeDependencyGraph> {
        let prepared = self.prepare_dependency_graph(owner, source, operation, available)?;
        let guard = crate::workspace_custody::lock_workspace_initialization_set(&prepared.roots)
            .map_err(error)?;
        self.inspect_prepared_dependency_graph(&prepared, &guard)
    }

    pub(super) fn prepare_dependency_graph<'a>(
        &self,
        owner: &'a ProvisionedAttachment,
        source: &'a ProvisionedAttachment,
        operation: RecordDigest,
        available: &[&'a ProvisionedAttachment],
    ) -> io::Result<PreparedDependencyGraph<'a>> {
        if available.len() > 256 {
            return Err(invalid("too many candidate work handles"));
        }
        let mut works = BTreeMap::new();
        for work in std::iter::once(owner)
            .chain(std::iter::once(source))
            .chain(available.iter().copied())
        {
            if let Some(previous) = works.insert(work.id(), work) {
                if previous.store.identity()? != work.store.identity()? {
                    return Err(invalid("conflicting native work handles"));
                }
            }
        }
        let mut selections = Vec::new();
        let mut roots = BTreeMap::new();
        for work in works.values() {
            let selected = self.prepare_dependency_work(owner, work)?;
            for root in &selected.roots {
                root.ensure_namespace_identity()?;
                roots
                    .entry(root.identity()?)
                    .or_insert_with(|| root.clone());
            }
            if roots.len() > 32 {
                return Err(invalid("complete graph custody exceeds its bound"));
            }
            selections.push((*work, selected));
        }
        Ok(PreparedDependencyGraph {
            owner,
            source,
            operation,
            selections,
            roots: roots.into_values().collect(),
        })
    }

    // A caller may compose this with current-grant validation and a native transaction without
    // releasing custody. Never acquire additional locks or accept a partial set here.
    pub(super) fn inspect_prepared_dependency_graph(
        &self,
        prepared: &PreparedDependencyGraph<'_>,
        guard: &crate::workspace_custody::WorkspaceInitializationGuard,
    ) -> io::Result<NativeDependencyGraph> {
        let works = prepared
            .selections
            .iter()
            .map(|(work, _)| *work)
            .collect::<Vec<_>>();
        let context = self.resolve_consumed_histories(
            prepared.owner,
            &works,
            guard,
            super::dependency_owner_context::OwnerHistoryContext::current(prepared.owner),
        )?;
        self.inspect_dependency_graph_with_owner(prepared, guard, &context)
    }
    pub(super) fn inspect_dependency_graph_with_owner(
        &self,
        prepared: &PreparedDependencyGraph<'_>,
        guard: &crate::workspace_custody::WorkspaceInitializationGuard,
        context: &super::dependency_owner_context::OwnerHistoryContext<'_>,
    ) -> io::Result<NativeDependencyGraph> {
        guard.require_roots(&prepared.roots).map_err(error)?;
        let owner = prepared.owner;
        let source = prepared.source;
        let operation = prepared.operation;
        let selections = &prepared.selections;
        let mut bindings = BTreeMap::new();
        let mut legacy_copied_work = BTreeSet::new();
        for (work, selected) in selections {
            let binding = context.validate(self, selected, guard)?;
            if selected.has_legacy_copied_origin() {
                legacy_copied_work.insert((binding.work(), binding.installation()));
            }
            if bindings
                .insert(
                    (binding.work(), binding.installation()),
                    (work, binding.clone()),
                )
                .is_some()
            {
                return Err(invalid("duplicate native work binding"));
            }
        }
        let (_, owner_proof) = context.read(owner)?;
        let owner_proof =
            owner_proof.ok_or_else(|| invalid("owning dependency authority is unavailable"))?;
        let mut consumptions = BTreeMap::new();
        for fact in owner_proof.policy().consumption_facts() {
            if consumptions.insert(fact.start, fact).is_some() {
                return Err(invalid(
                    "conflicting consumption for one starting operation",
                ));
            }
        }
        let mut histories = BTreeMap::new();
        for (key, (work, _)) in &bindings {
            let (configuration, proof) = context.read(work)?;
            let proof = proof.ok_or_else(|| invalid("work has no native dependency enrollment"))?;
            let history = crate::workspace::OpenWorkspace::open_attachment_read_history(
                work.metadata_path(),
                work.store.clone(),
                &crate::TrustedReviewers::default(),
                Some(&proof),
            )
            .map_err(error)?;
            super::history::verify_history_binding(&history, &configuration)?;
            let workspace = super::history::short_id(configuration.as_bytes());
            histories.insert(
                *key,
                (
                    history,
                    proof,
                    mesh_operations::WorkspaceId::from_bytes(workspace),
                ),
            );
        }
        let source_binding = bindings
            .values()
            .find(|(work, _)| work.id() == source.id())
            .ok_or_else(|| invalid("source work is unavailable"))?
            .1
            .clone();
        let root = (
            source_binding.work(),
            source_binding.installation(),
            operation,
        );
        let mut content_budget = 1024 * 1024 * 1024u64;
        let mut verified_manifests = BTreeMap::new();
        let mut graph = walk(
            root,
            |id| {
                let (history, proof, workspace) = histories
                    .get(&(id.0, id.1))
                    .ok_or_else(|| invalid("referenced work is unavailable"))?;
                if legacy_copied_work.contains(&(id.0, id.1)) {
                    return Err(invalid(
                        "legacy copied ancestry needs explicit migration evidence",
                    ));
                }
                if proof.is_legacy_operation(id.2) {
                    return Err(invalid(
                        "legacy input ancestry needs explicit migration evidence",
                    ));
                }
                let fact = history.dependency_operation_fact(id.2).map_err(error)?;
                if fact.workspace != *workspace || fact.operation != id.2 {
                    return Err(invalid(
                        "signed operation belongs to another native history",
                    ));
                }
                let mut parents = fact
                    .parents
                    .iter()
                    .map(|p| (id.0, id.1, *p))
                    .collect::<BTreeSet<_>>();
                let consumption = consumptions.get(&id).cloned();
                if let Some(receipt) = &consumption {
                    let source = bindings
                        .get(&(receipt.source.0, receipt.source.1))
                        .ok_or_else(|| invalid("consumed source unavailable"))?;
                    let destination = bindings
                        .get(&(id.0, id.1))
                        .ok_or_else(|| invalid("consumed destination unavailable"))?;
                    if receipt.bindings != Some((source.1.correlation, destination.1.correlation)) {
                        return Err(invalid("consumption has no exact native binding"));
                    }
                    parents.insert(receipt.source);
                }
                if fact.manifests.len() > 4096 {
                    return Err(invalid("operation manifest count exceeds its bound"));
                }
                let mut chunks = BTreeSet::new();
                for manifest in &fact.manifests {
                    let key = (id.0, id.1, *manifest);
                    if let std::collections::btree_map::Entry::Vacant(entry) =
                        verified_manifests.entry(key)
                    {
                        let retained = history
                            .dependency_manifest_chunks(*manifest, &mut content_budget)
                            .map_err(error)?;
                        entry.insert(retained);
                    }
                    chunks.extend(verified_manifests[&key].iter().copied());
                    if chunks.len() > 65536 {
                        return Err(invalid("dependency chunk roots exceed their bound"));
                    }
                }
                Ok(Node {
                    chunks,
                    parents,
                    manifests: fact.manifests,
                    consumption,
                })
            },
            MAX_NODES,
            MAX_EDGES,
            MAX_NODES,
            MAX_BYTES,
        )?;
        // Policy payloads are local objects; cross-work reference IDs must never be
        // misclassified as local CAS roots. Keep control history separate from graph identity.
        let owner_binding = bindings
            .values()
            .find(|(work, _)| work.id() == owner.id())
            .ok_or_else(|| invalid("owner work is unavailable"))?
            .1
            .clone();
        let mut included = graph
            .nodes
            .keys()
            .map(|id| (id.0, id.1))
            .collect::<BTreeSet<_>>();
        included.insert((owner_binding.work(), owner_binding.installation()));
        let mut retained_count = 0usize;
        let mut receipt_budget = 1024 * 1024 * 1024usize;
        for key in included {
            let (work, binding) = &bindings[&key];
            let proof = &histories[&key].1;
            let mut payloads = proof.policy().policy_payloads().collect::<BTreeSet<_>>();
            payloads.insert(proof.binding().authority);
            let mut manifests = BTreeSet::new();
            for (id, node) in &graph.nodes {
                if (id.0, id.1) == key {
                    payloads.insert(id.2);
                    payloads.extend(node.chunks.iter().copied());
                    manifests.extend(node.manifests.iter().copied());
                }
            }
            let operations = graph
                .nodes
                .keys()
                .filter(|id| (id.0, id.1) == key)
                .map(|id| id.2)
                .collect::<BTreeSet<_>>();
            let mut receipts = work.completed_capture_receipt_roots_with_owner(
                &operations,
                &mut receipt_budget,
                context,
            )?;
            let pending_name = super::dependency_decision::PENDING;
            match super::dependency_enrollment::read_private_in_store(&work.store, pending_name) {
                Ok(raw) => {
                    let parsed = Json::parse(&raw).map_err(error)?;
                    let request = super::dependency_transaction::digest(
                        super::dependency_transaction::text(&parsed, "request")?,
                    )?;
                    let pending = work.inspect_pending_dependency_control_retention(request)?;
                    let expected = hash(raw.as_bytes());
                    if pending.get("sidecar_digest").and_then(Json::as_text)
                        != Some(expected.to_hex().as_str())
                        || super::dependency_enrollment::read_private_in_store(
                            &work.store,
                            pending_name,
                        )? != raw
                    {
                        return Err(invalid("pending graph control evidence changed"));
                    }
                    receipts.sidecars.insert(pending_name.to_owned(), expected);
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
            if let Some((name, digest, payload)) = context.pending_roots(work)? {
                receipts.sidecars.insert(name, digest);
                payloads.insert(payload);
            }
            if let Some(roots) = context.verified_history_roots(work)? {
                receipts.sidecars.extend(roots.sidecars);
                payloads.extend(roots.payloads);
            }
            payloads.extend(receipts.payloads);
            manifests.extend(receipts.manifests);
            retained_count = retained_count
                .checked_add(receipts.sidecars.len())
                .and_then(|n| n.checked_add(payloads.len()))
                .and_then(|n| n.checked_add(manifests.len()))
                .filter(|n| *n <= MAX_BYTES / 64)
                .ok_or_else(|| invalid("retained content exceeds its bound"))?;
            graph.retained.insert(
                key,
                StoreFacts {
                    physical: work.store.identity()?,
                    sidecars: receipts.sidecars,
                    correlation: binding.correlation,
                    payloads,
                    manifests,
                },
            );
        }
        if graph.retained_content_json().encode().len() > MAX_BYTES {
            return Err(invalid("retained content encoding exceeds its bound"));
        }
        for (work, selected) in selections {
            let refreshed = context.validate(self, selected, guard)?;
            if bindings
                .get(&(refreshed.work(), refreshed.installation()))
                .map(|(_, b)| b)
                != Some(&refreshed)
            {
                return Err(invalid("native graph work changed"));
            }
            let (_, proof) = context.read(work)?;
            if proof.as_ref()
                != histories
                    .get(&(refreshed.work(), refreshed.installation()))
                    .map(|(_, p, _)| p)
            {
                return Err(invalid("native graph history changed"));
            }
        }
        guard.ensure_current().map_err(error)?;
        Ok(graph)
    }
}
#[cfg(test)]
mod tests;
