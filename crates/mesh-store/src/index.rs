//! The fold: the in-memory reference implementation of every table.
//!
//! This is what "the index is reconstructable" *means* operationally. [`Index`] holds nothing but
//! the result of applying [`StoredRecord`]s in order, and [`Index::rows`] renders any table from
//! that. The live commit path and a from-scratch rebuild run the identical code, so the two cannot
//! disagree about what a table should contain — and `tests/reconstruction.rs` writes the result
//! into a real SQLite database and reads it back to check that SQLite does not disagree either.
//!
//! # Ordering is causal, never wall-clock
//!
//! Nothing here reads a clock, and `hlc_millis` participates in no key and no comparison. An
//! actor's own operations order by `actor_sequence`; operations across actors order by their
//! causal parents. [`Index::apply`] can therefore receive operations in network order: an absent
//! parent keeps its child out of the causally-ready projection, and the projection advances only
//! when the complete closure is present. Actor-sequence slots still make a fork a refusal.

use std::collections::{BTreeMap, BTreeSet};

use crate::digest::{Digest16, DigestWriter, Fnv1a128, IndexDigest};
use crate::ids::{EntityUuid, RecordDigest};
use crate::record::{
    AckRecord, ApprovalRecord, ContextRecord, ManifestRecord, OperationRecord, PeerRecord,
    ReviewRecord, StoredRecord,
};
use crate::row::{absorb_table, Row, Value};
use crate::schema::TABLES;

/// Why a record could not be folded in.
///
/// Every variant is a statement about the *record stream*, not about the index: the index has no
/// state that a well-formed stream can put it into a bad place from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FoldError {
    /// Two different operations claim the same identifier. A digest names its content, so this is
    /// either a collision or a forged record, and neither is repaired locally.
    ConflictingOperation {
        /// The identifier claimed twice.
        id: RecordDigest,
    },
    /// One actor issued two different operations at the same sequence number — a fork in a chain
    /// that must not fork.
    ForkedActorChain {
        /// The actor whose chain forked.
        actor: RecordDigest,
        /// The sequence number claimed twice.
        actor_sequence: u64,
    },
    /// An operation names itself as a causal parent.
    SelfCausalParent {
        /// The invalid operation.
        operation: RecordDigest,
    },
    /// One causal parent appears more than once in the same operation.
    DuplicateCausalParent {
        /// The invalid operation.
        operation: RecordDigest,
        /// The repeated parent.
        parent: RecordDigest,
    },
    /// Adding an operation would complete a causal cycle.
    CausalCycle {
        /// The operation whose arrival completed the cycle.
        operation: RecordDigest,
        /// One parent edge from that operation into the cycle.
        parent: RecordDigest,
    },
    /// Two different manifests claim the same identifier.
    ConflictingManifest {
        /// The identifier claimed twice.
        id: RecordDigest,
    },
    /// A manifest's chunks do not tile its byte length without gap or overlap.
    DiscontiguousManifest {
        /// The manifest.
        id: RecordDigest,
    },
    /// A record refers to something the stream has not supplied.
    DanglingReference {
        /// Which table the row would have gone into.
        table: &'static str,
        /// The identifier that resolved to nothing.
        missing: RecordDigest,
    },
}

impl core::fmt::Display for FoldError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ConflictingOperation { id } => {
                write!(formatter, "two different operations claim {id}")
            }
            Self::ForkedActorChain {
                actor,
                actor_sequence,
            } => write!(
                formatter,
                "actor {actor} issued two operations at sequence {actor_sequence}"
            ),
            Self::SelfCausalParent { operation } => {
                write!(
                    formatter,
                    "operation {operation} names itself as a causal parent"
                )
            }
            Self::DuplicateCausalParent { operation, parent } => write!(
                formatter,
                "operation {operation} names causal parent {parent} more than once"
            ),
            Self::CausalCycle { operation, parent } => write!(
                formatter,
                "operation {operation} through parent {parent} completes a causal cycle"
            ),
            Self::ConflictingManifest { id } => {
                write!(formatter, "two different manifests claim {id}")
            }
            Self::DiscontiguousManifest { id } => write!(
                formatter,
                "manifest {id} has chunks that do not tile its byte length"
            ),
            Self::DanglingReference { table, missing } => {
                write!(
                    formatter,
                    "{table} refers to {missing}, which is not in the stream"
                )
            }
        }
    }
}

impl std::error::Error for FoldError {}

/// The whole local index, in memory.
///
/// Every collection is a `BTree`, so iteration order is the key order — which is the same order
/// SQLite produces for the same key, which is why the digest over one can be compared against the
/// digest over the other.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Index {
    operations: BTreeMap<RecordDigest, OperationRecord>,
    /// Which operation occupies each `(actor, actor_sequence)` slot.
    ///
    /// Derived from `operations` and never independent of it: the two are written together and a
    /// fold that produced one without the other would be a bug the derived `PartialEq` catches.
    /// It exists so a fork is detected by one lookup instead of a scan over every operation the
    /// index holds — the difference between a recovery that is linear in the journal and one that
    /// is quadratic, which is what plan §6.3's five-second budget is spent on.
    chains: BTreeMap<(RecordDigest, u64), RecordDigest>,
    /// Parent identifiers named by accepted operations, including parents not yet received.
    /// A new operation can close a cycle only if an existing edge already points to its id.
    /// This derived lookup avoids scanning the whole ancestry of each ordinary append.
    referenced_parents: BTreeSet<RecordDigest>,
    manifests: BTreeMap<RecordDigest, ManifestRecord>,
    peers: BTreeMap<RecordDigest, PeerRecord>,
    watermarks: BTreeMap<(RecordDigest, RecordDigest), u64>,
    reviews: BTreeMap<RecordDigest, ReviewRecord>,
    approvals: BTreeMap<RecordDigest, ApprovalRecord>,
    context: BTreeMap<RecordDigest, ContextRecord>,
    ledger: Vec<Row>,
}

impl Index {
    /// An index holding nothing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record the migration ledger rows this database carries.
    ///
    /// The one part of the index that is not a fold over records, and the reason
    /// [`crate::Provenance`] has a second shape. It is supplied rather than derived because it is
    /// a fact about *this file*, not about the workspace's history.
    pub fn set_ledger_rows(&mut self, rows: Vec<Row>) {
        self.ledger = rows;
        self.ledger.sort();
    }

    /// Fold one record in.
    ///
    /// # Errors
    ///
    /// [`FoldError`] when the record contradicts one already folded, or refers to something the
    /// stream has not supplied.
    pub fn apply(&mut self, record: StoredRecord) -> Result<(), FoldError> {
        match record {
            StoredRecord::Operation(operation) => self.apply_operation(operation),
            StoredRecord::Manifest(manifest) => self.apply_manifest(manifest),
            StoredRecord::Peer(peer) => self.apply_peer(peer),
            StoredRecord::Acknowledgement(ack) => self.apply_ack(&ack),
            StoredRecord::Review(review) => self.apply_review(review),
            StoredRecord::Approval(approval) => self.apply_approval(approval),
            StoredRecord::ContextEntry(entry) => self.apply_context(entry),
        }
    }

    fn apply_operation(&mut self, operation: OperationRecord) -> Result<(), FoldError> {
        if let Some(existing) = self.operations.get(&operation.id) {
            if existing == &operation {
                return Ok(());
            }
            return Err(FoldError::ConflictingOperation { id: operation.id });
        }
        let slot = (operation.actor, operation.actor_sequence);
        if self.chains.contains_key(&slot) {
            return Err(FoldError::ForkedActorChain {
                actor: operation.actor,
                actor_sequence: operation.actor_sequence,
            });
        }
        let mut parents = BTreeSet::new();
        for parent in &operation.parents {
            if parent == &operation.id {
                return Err(FoldError::SelfCausalParent {
                    operation: operation.id,
                });
            }
            if !parents.insert(*parent) {
                return Err(FoldError::DuplicateCausalParent {
                    operation: operation.id,
                    parent: *parent,
                });
            }
            if self.referenced_parents.contains(&operation.id)
                && self.parent_path_reaches(parent, &operation.id)
            {
                return Err(FoldError::CausalCycle {
                    operation: operation.id,
                    parent: *parent,
                });
            }
        }
        self.referenced_parents.extend(parents);
        self.chains.insert(slot, operation.id);
        self.operations.insert(operation.id, operation);
        Ok(())
    }

    fn parent_path_reaches(&self, from: &RecordDigest, target: &RecordDigest) -> bool {
        let mut visited = BTreeSet::new();
        let mut pending = vec![*from];
        while let Some(id) = pending.pop() {
            if &id == target {
                return true;
            }
            if !visited.insert(id) {
                continue;
            }
            if let Some(operation) = self.operations.get(&id) {
                pending.extend(operation.parents.iter().copied());
            }
        }
        false
    }

    fn apply_manifest(&mut self, manifest: ManifestRecord) -> Result<(), FoldError> {
        if let Some(existing) = self.manifests.get(&manifest.id) {
            if existing == &manifest {
                return Ok(());
            }
            return Err(FoldError::ConflictingManifest { id: manifest.id });
        }
        if !chunks_tile(&manifest) {
            return Err(FoldError::DiscontiguousManifest { id: manifest.id });
        }
        self.manifests.insert(manifest.id, manifest);
        Ok(())
    }

    fn apply_peer(&mut self, peer: PeerRecord) -> Result<(), FoldError> {
        if !self.operations.contains_key(&peer.joined_at) {
            return Err(FoldError::DanglingReference {
                table: "peer",
                missing: peer.joined_at,
            });
        }
        self.peers.insert(peer.peer, peer);
        Ok(())
    }

    /// A watermark only ever moves forward. An acknowledgement that arrives out of order therefore
    /// changes nothing, which is what makes the fold insensitive to the order acknowledgements are
    /// replayed in — the property a rebuild depends on.
    fn apply_ack(&mut self, ack: &AckRecord) -> Result<(), FoldError> {
        if !self.peers.contains_key(&ack.peer) {
            return Err(FoldError::DanglingReference {
                table: "peer_watermark",
                missing: ack.peer,
            });
        }
        let slot = self.watermarks.entry((ack.peer, ack.actor)).or_insert(0);
        *slot = (*slot).max(ack.actor_sequence);
        Ok(())
    }

    fn apply_review(&mut self, review: ReviewRecord) -> Result<(), FoldError> {
        if !self.operations.contains_key(&review.subject_operation) {
            return Err(FoldError::DanglingReference {
                table: "review_bundle",
                missing: review.subject_operation,
            });
        }
        self.reviews.insert(review.bundle, review);
        Ok(())
    }

    fn apply_approval(&mut self, approval: ApprovalRecord) -> Result<(), FoldError> {
        if !self.reviews.contains_key(&approval.bundle) {
            return Err(FoldError::DanglingReference {
                table: "review_approval",
                missing: approval.bundle,
            });
        }
        self.approvals.insert(approval.approval, approval);
        Ok(())
    }

    fn apply_context(&mut self, entry: ContextRecord) -> Result<(), FoldError> {
        if !self.operations.contains_key(&entry.operation) {
            return Err(FoldError::DanglingReference {
                table: "context_ledger",
                missing: entry.operation,
            });
        }
        self.context.insert(entry.entry, entry);
        Ok(())
    }

    /// How many operations the index holds.
    #[must_use]
    pub fn operation_count(&self) -> usize {
        self.operations.len()
    }

    /// The head operation of one actor, if the index has seen any.
    ///
    /// A durably held operation whose causal closure is incomplete is deliberately absent: it is
    /// buffered record truth, not yet a transition that may advance application state.
    #[must_use]
    pub fn actor_head(&self, actor: &RecordDigest) -> Option<&OperationRecord> {
        let ready = self.causally_ready_ids();
        self.operations
            .values()
            .filter(|operation| &operation.actor == actor)
            .filter(|operation| ready.contains(&operation.id))
            .max_by_key(|operation| operation.actor_sequence)
    }

    /// Operation identifiers whose complete transitive causal closure is present.
    ///
    /// The returned order is deterministic and topological: every parent precedes its children.
    /// Missing-parent operations stay in the durable record/index planes but never appear here.
    #[must_use]
    pub fn causally_ready_operations(&self) -> Vec<RecordDigest> {
        let mut remaining = BTreeMap::<RecordDigest, usize>::new();
        let mut children = BTreeMap::<RecordDigest, BTreeSet<RecordDigest>>::new();
        let mut ready = BTreeSet::new();
        for operation in self.operations.values() {
            remaining.insert(operation.id, operation.parents.len());
            if operation.parents.is_empty() {
                ready.insert(operation.id);
            }
            for parent in &operation.parents {
                children.entry(*parent).or_default().insert(operation.id);
            }
        }
        let mut order = Vec::new();
        while let Some(id) = ready.first().copied() {
            ready.remove(&id);
            order.push(id);
            if let Some(dependants) = children.get(&id) {
                for dependant in dependants {
                    let count = remaining
                        .get_mut(dependant)
                        .expect("every child is a held operation");
                    *count -= 1;
                    if *count == 0 {
                        ready.insert(*dependant);
                    }
                }
            }
        }
        order
    }

    /// Whether one durably held operation is causally ready for application.
    #[must_use]
    pub fn is_causally_ready(&self, id: &RecordDigest) -> bool {
        self.causally_ready_ids().contains(id)
    }

    /// Missing leaves that keep an operation's transitive causal closure buffered.
    ///
    /// `None` means the operation itself is not held. An empty set means it is ready.
    #[must_use]
    pub fn unresolved_causal_dependencies(
        &self,
        id: &RecordDigest,
    ) -> Option<BTreeSet<RecordDigest>> {
        self.operations.get(id)?;
        let mut missing = BTreeSet::new();
        self.collect_missing(id, &mut missing);
        Some(missing)
    }

    fn causally_ready_ids(&self) -> BTreeSet<RecordDigest> {
        self.causally_ready_operations().into_iter().collect()
    }

    fn collect_missing(&self, id: &RecordDigest, missing: &mut BTreeSet<RecordDigest>) {
        let mut visited = BTreeSet::new();
        let mut pending = vec![*id];
        while let Some(current) = pending.pop() {
            if !visited.insert(current) {
                continue;
            }
            if let Some(operation) = self.operations.get(&current) {
                pending.extend(operation.parents.iter().copied());
            } else {
                missing.insert(current);
            }
        }
    }

    /// Every actor the index has seen an operation from.
    #[must_use]
    pub fn actors(&self) -> BTreeSet<RecordDigest> {
        self.operations
            .values()
            .map(|operation| operation.actor)
            .collect()
    }

    // -----------------------------------------------------------------------
    // Readers the retained-root computation needs.
    //
    // Every one of these is a *lookup over what the fold already holds*: none
    // adds state, none is a second source of truth, and none can be written to.
    // `crate::Reachability` is a pure function of them, which is what lets
    // `tests/gc.rs` recompute the same closure a second, independent way and
    // compare — a collector checked only against its own helpers checks nothing.
    // -----------------------------------------------------------------------

    /// One operation by identifier.
    #[must_use]
    pub fn operation(&self, id: &RecordDigest) -> Option<&OperationRecord> {
        self.operations.get(id)
    }

    /// One manifest by identifier.
    #[must_use]
    pub fn manifest(&self, id: &RecordDigest) -> Option<&ManifestRecord> {
        self.manifests.get(id)
    }

    /// One review bundle by identifier.
    #[must_use]
    pub fn review(&self, bundle: &RecordDigest) -> Option<&ReviewRecord> {
        self.reviews.get(bundle)
    }

    /// Every immutable approval envelope that names one review bundle.
    ///
    /// Callers must still interpret verdict order and validate the referenced receipt. This is a
    /// read-only projection of the record fold, not publication authority.
    pub fn approvals_for_bundle<'a>(
        &'a self,
        bundle: &'a RecordDigest,
    ) -> impl Iterator<Item = &'a ApprovalRecord> {
        self.approvals
            .values()
            .filter(move |approval| &approval.bundle == bundle)
    }

    /// Every manifest identifier the index holds.
    #[must_use]
    pub fn manifest_ids(&self) -> BTreeSet<RecordDigest> {
        self.manifests.keys().copied().collect()
    }

    /// Every review bundle identifier the index holds.
    #[must_use]
    pub fn review_bundles(&self) -> BTreeSet<RecordDigest> {
        self.reviews.keys().copied().collect()
    }

    /// Every peer in the replication set.
    #[must_use]
    pub fn peer_ids(&self) -> BTreeSet<RecordDigest> {
        self.peers.keys().copied().collect()
    }

    /// One actor's own operations, in identifier order.
    pub fn operations_of<'a>(
        &'a self,
        actor: &'a RecordDigest,
    ) -> impl Iterator<Item = &'a OperationRecord> {
        self.operations
            .values()
            .filter(move |operation| &operation.actor == actor)
    }

    /// The operations a peer has not acknowledged — the outbox for that peer, as identifiers.
    ///
    /// The same difference [`Self::rows`] renders into the `outbox` table, exposed as the set the
    /// retained-root computation needs. Sharing the definition is the point: an offline peer's
    /// claim on content is exactly "what is still in its outbox", and two spellings of that would
    /// eventually disagree about which chunk is safe to delete.
    #[must_use]
    pub fn owed_to_peer(&self, peer: &RecordDigest) -> BTreeSet<RecordDigest> {
        self.operations
            .values()
            .filter(|operation| &operation.actor != peer)
            .filter(|operation| {
                let watermark = self
                    .watermarks
                    .get(&(*peer, operation.actor))
                    .copied()
                    .unwrap_or(0);
                operation.actor_sequence > watermark
            })
            .map(|operation| operation.id)
            .collect()
    }

    /// Highest contiguous sequence `peer` has acknowledged for `actor`.
    ///
    /// `None` means the peer is not in the replication set. A known peer with no receipt has
    /// acknowledged through zero. Delivery uses this distinction to reject an unknown peer before
    /// writing a receipt to the immutable journal, while treating a duplicate or reordered retry
    /// as a no-op.
    #[must_use]
    pub fn acknowledged_through(&self, peer: &RecordDigest, actor: &RecordDigest) -> Option<u64> {
        self.peers
            .contains_key(peer)
            .then(|| self.watermarks.get(&(*peer, *actor)).copied().unwrap_or(0))
    }

    /// The highest policy epoch any indexed operation carries, which is how far the workspace has
    /// moved. `None` when the index holds no operation.
    ///
    /// Policy epochs rather than time: plan ordering is `lamport → event id → content hash`, and
    /// nothing in this crate reads a clock.
    #[must_use]
    pub fn highest_policy_epoch(&self) -> Option<u64> {
        self.operations
            .values()
            .map(|operation| operation.policy_epoch)
            .max()
    }

    /// The highest policy epoch a peer's watermarks reach, or the epoch it joined at when it has
    /// acknowledged nothing.
    ///
    /// `None` when the peer never joined, or when it joined at an operation this index does not
    /// hold — and `None` means "do not expire it", because an unknown position is never evidence
    /// that a peer is gone.
    #[must_use]
    pub fn peer_last_acknowledged_epoch(&self, peer: &RecordDigest) -> Option<u64> {
        let joined = self.peers.get(peer)?;
        let acknowledged = self
            .watermarks
            .iter()
            .filter(|((holder, _), _)| holder == peer)
            .filter_map(|((_, actor), sequence)| self.chains.get(&(*actor, *sequence)))
            .filter_map(|id| self.operations.get(id))
            .map(|operation| operation.policy_epoch)
            .max();
        match acknowledged {
            Some(epoch) => Some(epoch),
            None => self
                .operations
                .get(&joined.joined_at)
                .map(|operation| operation.policy_epoch),
        }
    }

    /// Every content digest any record in this index names.
    ///
    /// The set a collection candidate is checked against to tell "nothing has ever referenced
    /// this" from "something referenced it and no retained root reaches that something". The two
    /// are different reports and the difference matters to whoever reads a dry run.
    #[must_use]
    pub fn named_content(&self) -> BTreeSet<RecordDigest> {
        let mut named = BTreeSet::new();
        for operation in self.operations.values() {
            named.insert(operation.payload_digest);
        }
        for manifest in self.manifests.values() {
            named.insert(manifest.content_digest);
            for chunk in &manifest.chunks {
                named.insert(chunk.digest);
            }
        }
        named
    }

    /// The rows of one table, in the order SQLite would return them under `ORDER BY 1, 2, …`.
    ///
    /// Returns `None` for a table [`TABLES`] does not declare, which is the only way to ask for a
    /// table that does not exist.
    #[must_use]
    pub fn rows(&self, table: &str) -> Option<Vec<Row>> {
        let mut rows = match table {
            "schema_version" => self.ledger.clone(),
            "operation" => self.operation_rows(),
            "operation_parent" => self.operation_parent_rows(),
            "manifest" => self.manifest_rows(),
            "manifest_chunk" => self.manifest_chunk_rows(),
            "actor_head" => self.actor_head_rows(),
            "peer" => self.peer_rows(),
            "peer_watermark" => self.watermark_rows(),
            "outbox" => self.outbox_rows(),
            "review_bundle" => self.review_rows(),
            "review_approval" => self.approval_rows(),
            "context_ledger" => self.context_rows(),
            _ => return None,
        };
        rows.sort();
        Some(rows)
    }

    fn operation_rows(&self) -> Vec<Row> {
        self.operations
            .values()
            .map(|operation| {
                Row::new(vec![
                    Value::blob(operation.id.as_bytes()),
                    Value::blob(operation.actor.as_bytes()),
                    Value::count(operation.actor_sequence),
                    Value::count(operation.hlc_millis),
                    Value::count(operation.hlc_counter),
                    Value::count(operation.policy_epoch),
                    Value::blob(operation.session.as_bytes()),
                    Value::blob(operation.payload_digest.as_bytes()),
                ])
            })
            .collect()
    }

    fn operation_parent_rows(&self) -> Vec<Row> {
        self.operations
            .values()
            .flat_map(|operation| {
                operation
                    .parents
                    .iter()
                    .enumerate()
                    .map(|(ordinal, parent)| {
                        Row::new(vec![
                            Value::blob(operation.id.as_bytes()),
                            Value::count(ordinal as u64),
                            Value::blob(parent.as_bytes()),
                        ])
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn manifest_rows(&self) -> Vec<Row> {
        self.manifests
            .values()
            .map(|manifest| {
                Row::new(vec![
                    Value::blob(manifest.id.as_bytes()),
                    Value::count(manifest.byte_length),
                    Value::blob(manifest.content_digest.as_bytes()),
                ])
            })
            .collect()
    }

    fn manifest_chunk_rows(&self) -> Vec<Row> {
        self.manifests
            .values()
            .flat_map(|manifest| {
                manifest
                    .chunks
                    .iter()
                    .enumerate()
                    .map(|(ordinal, chunk)| {
                        Row::new(vec![
                            Value::blob(manifest.id.as_bytes()),
                            Value::count(ordinal as u64),
                            Value::blob(chunk.digest.as_bytes()),
                            Value::count(chunk.byte_offset),
                            Value::count(chunk.byte_length),
                        ])
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn actor_head_rows(&self) -> Vec<Row> {
        let ready = self.causally_ready_ids();
        let mut heads = BTreeMap::<RecordDigest, &OperationRecord>::new();
        for operation in self
            .operations
            .values()
            .filter(|operation| ready.contains(&operation.id))
        {
            let head = heads.entry(operation.actor).or_insert(operation);
            if operation.actor_sequence > head.actor_sequence {
                *head = operation;
            }
        }
        heads
            .into_iter()
            .map(|(actor, head)| {
                Row::new(vec![
                    Value::blob(actor.as_bytes()),
                    Value::blob(head.id.as_bytes()),
                    Value::count(head.actor_sequence),
                ])
            })
            .collect()
    }

    fn peer_rows(&self) -> Vec<Row> {
        self.peers
            .values()
            .map(|peer| {
                Row::new(vec![
                    Value::blob(peer.peer.as_bytes()),
                    Value::blob(peer.joined_at.as_bytes()),
                ])
            })
            .collect()
    }

    fn watermark_rows(&self) -> Vec<Row> {
        self.watermarks
            .iter()
            .map(|((peer, actor), sequence)| {
                Row::new(vec![
                    Value::blob(peer.as_bytes()),
                    Value::blob(actor.as_bytes()),
                    Value::count(*sequence),
                ])
            })
            .collect()
    }

    /// The outbox, computed rather than stored: for every peer, every operation whose sequence is
    /// above that peer's watermark for that operation's actor.
    ///
    /// A peer never appears in its own outbox — sending an actor its own operations back is not
    /// replication, and a self-row would also mean the local node queues work for itself forever,
    /// because no acknowledgement of one's own operations is ever produced.
    fn outbox_rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for peer in self.peers.keys() {
            for operation in self.operations.values() {
                if &operation.actor == peer {
                    continue;
                }
                let watermark = self
                    .watermarks
                    .get(&(*peer, operation.actor))
                    .copied()
                    .unwrap_or(0);
                if operation.actor_sequence > watermark {
                    rows.push(Row::new(vec![
                        Value::blob(peer.as_bytes()),
                        Value::blob(operation.actor.as_bytes()),
                        Value::count(operation.actor_sequence),
                        Value::blob(operation.id.as_bytes()),
                    ]));
                }
            }
        }
        rows
    }

    fn review_rows(&self) -> Vec<Row> {
        self.reviews
            .values()
            .map(|review| {
                Row::new(vec![
                    Value::blob(review.bundle.as_bytes()),
                    Value::blob(review.subject_operation.as_bytes()),
                    Value::blob(review.opened_by.as_bytes()),
                ])
            })
            .collect()
    }

    fn approval_rows(&self) -> Vec<Row> {
        self.approvals
            .values()
            .map(|approval| {
                Row::new(vec![
                    Value::blob(approval.approval.as_bytes()),
                    Value::blob(approval.bundle.as_bytes()),
                    Value::blob(approval.approver.as_bytes()),
                    Value::Integer(approval.verdict.code()),
                ])
            })
            .collect()
    }

    fn context_rows(&self) -> Vec<Row> {
        self.context
            .values()
            .map(|entry| {
                Row::new(vec![
                    Value::blob(entry.entry.as_bytes()),
                    Value::blob(entry.session.as_bytes()),
                    Value::blob(entry.operation.as_bytes()),
                    Value::Integer(entry.access.code()),
                    Value::count(entry.byte_length),
                ])
            })
            .collect()
    }

    /// The digest of every table, absorbed in [`TABLES`] order.
    ///
    /// Two indexes with this digest hold the same rows in the same order. It is the check that
    /// "rebuild every table from immutable records" produced the same thing the live path did.
    #[must_use]
    pub fn digest<D: IndexDigest>(&self) -> Digest16 {
        let mut writer = DigestWriter::<D>::new();
        writer.count(TABLES.len());
        for table in TABLES {
            let rows = self.rows(table.name).unwrap_or_default();
            absorb_table(&mut writer, table.name, &rows);
        }
        writer.finish()
    }

    /// The digest under the default algorithm.
    #[must_use]
    pub fn default_digest(&self) -> Digest16 {
        self.digest::<Fnv1a128>()
    }
}

/// Whether a manifest's chunks tile `[0, byte_length)` with no gap and no overlap.
fn chunks_tile(manifest: &ManifestRecord) -> bool {
    let mut cursor = 0u64;
    for chunk in &manifest.chunks {
        if chunk.byte_offset != cursor {
            return false;
        }
        cursor = match cursor.checked_add(chunk.byte_length) {
            Some(next) => next,
            None => return false,
        };
    }
    cursor == manifest.byte_length
}

/// A session identifier that names no session, for records that predate sessions.
#[must_use]
pub const fn no_session() -> EntityUuid {
    EntityUuid::from_bytes([0; 16])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{ChunkSlice, ContextAccess, ReviewVerdict};

    fn digest(seed: u8) -> RecordDigest {
        RecordDigest::from_bytes([seed; 32])
    }

    fn operation(id: u8, actor: u8, sequence: u64) -> OperationRecord {
        OperationRecord {
            id: digest(id),
            actor: digest(actor),
            actor_sequence: sequence,
            hlc_millis: 1_700_000_000_000,
            hlc_counter: 0,
            policy_epoch: 1,
            session: no_session(),
            payload_digest: digest(id.wrapping_add(100)),
            parents: Vec::new(),
        }
    }

    fn manifest(id: u8, lengths: &[u64]) -> ManifestRecord {
        let mut chunks = Vec::new();
        let mut offset = 0;
        for (index, length) in lengths.iter().enumerate() {
            chunks.push(ChunkSlice {
                digest: digest(200u8.wrapping_add(index as u8)),
                byte_offset: offset,
                byte_length: *length,
            });
            offset += length;
        }
        ManifestRecord {
            id: digest(id),
            byte_length: offset,
            content_digest: digest(id.wrapping_add(50)),
            chunks,
        }
    }

    #[test]
    fn out_of_order_cycles_are_refused_without_changing_the_index() {
        let mut index = Index::new();
        let mut first = operation(1, 9, 1);
        first.parents = vec![digest(2)];
        let mut second = operation(2, 9, 2);
        second.parents = vec![digest(3)];
        index.apply(StoredRecord::Operation(first)).unwrap();
        index.apply(StoredRecord::Operation(second)).unwrap();
        let before = index.clone();
        let mut third = operation(3, 9, 3);
        third.parents = vec![digest(4), digest(1)];
        assert_eq!(
            index.apply(StoredRecord::Operation(third)),
            Err(FoldError::CausalCycle {
                operation: digest(3),
                parent: digest(1)
            })
        );
        assert_eq!(
            index, before,
            "rejected edges must not enter the derived lookup"
        );
        index
            .apply(StoredRecord::Operation(operation(3, 9, 3)))
            .unwrap();
        assert_eq!(
            index.causally_ready_operations(),
            vec![digest(3), digest(2), digest(1)]
        );
    }

    #[test]
    fn parent_lookup_is_independent_of_delivery_order() {
        let first = operation(1, 9, 1);
        let mut second = operation(2, 9, 2);
        second.parents = vec![digest(1)];
        let mut third = operation(3, 9, 3);
        third.parents = vec![digest(1), digest(2)];
        let records = [first, second, third];
        let mut forward = Index::new();
        let mut reverse = Index::new();
        for record in &records {
            forward
                .apply(StoredRecord::Operation(record.clone()))
                .unwrap();
        }
        for record in records.iter().rev() {
            reverse
                .apply(StoredRecord::Operation(record.clone()))
                .unwrap();
        }
        assert_eq!(forward, reverse);
    }

    #[test]
    fn a_fresh_index_renders_every_table_empty() {
        let index = Index::new();
        for table in TABLES {
            assert_eq!(
                index.rows(table.name),
                Some(Vec::new()),
                "{} is not renderable",
                table.name
            );
        }
    }

    #[test]
    fn a_table_the_schema_does_not_declare_has_no_rows() {
        assert_eq!(Index::new().rows("not_a_table"), None);
    }

    /// Every table in the schema must be renderable, or the reconstruction digest would silently
    /// treat a missing renderer as an empty table.
    #[test]
    fn every_declared_table_has_a_renderer() {
        let mut index = Index::new();
        index
            .apply(StoredRecord::Operation(operation(1, 2, 1)))
            .unwrap();
        for table in TABLES {
            assert!(
                index.rows(table.name).is_some(),
                "{} has no renderer",
                table.name
            );
        }
    }

    #[test]
    fn applying_the_same_operation_twice_is_idempotent() {
        let mut index = Index::new();
        let record = StoredRecord::Operation(operation(1, 2, 1));
        index.apply(record.clone()).unwrap();
        index.apply(record).unwrap();
        assert_eq!(index.operation_count(), 1);
    }

    #[test]
    fn two_operations_claiming_one_identifier_are_refused() {
        let mut index = Index::new();
        index
            .apply(StoredRecord::Operation(operation(1, 2, 1)))
            .unwrap();
        let mut other = operation(1, 2, 1);
        other.policy_epoch = 9;
        assert_eq!(
            index.apply(StoredRecord::Operation(other)),
            Err(FoldError::ConflictingOperation { id: digest(1) })
        );
    }

    #[test]
    fn a_forked_actor_chain_is_refused() {
        let mut index = Index::new();
        index
            .apply(StoredRecord::Operation(operation(1, 2, 1)))
            .unwrap();
        assert_eq!(
            index.apply(StoredRecord::Operation(operation(3, 2, 1))),
            Err(FoldError::ForkedActorChain {
                actor: digest(2),
                actor_sequence: 1
            })
        );
    }

    /// The chain map is an index over the operations and never a second source of truth. Two
    /// indexes holding the same operations must compare equal whichever order they arrived in, and
    /// a refused fork must leave nothing behind — a slot reserved by a rejected operation would
    /// make the next legitimate one impossible.
    #[test]
    fn the_chain_lookup_stays_in_step_with_the_operations_it_indexes() {
        let mut forwards = Index::new();
        let mut backwards = Index::new();
        for record in [operation(1, 2, 1), operation(3, 2, 2), operation(5, 4, 1)] {
            forwards
                .apply(StoredRecord::Operation(record.clone()))
                .expect("folds");
        }
        for record in [operation(5, 4, 1), operation(3, 2, 2), operation(1, 2, 1)] {
            backwards
                .apply(StoredRecord::Operation(record))
                .expect("folds");
        }
        assert_eq!(forwards, backwards);

        let mut rejecting = Index::new();
        assert!(rejecting
            .apply(StoredRecord::Operation(operation(1, 2, 1)))
            .is_ok());
        let mut conflicting = operation(1, 2, 1);
        conflicting.policy_epoch = 99;
        assert!(rejecting
            .apply(StoredRecord::Operation(conflicting))
            .is_err());
        // The rejected record reserved nothing: a different actor at the same sequence still folds.
        assert!(rejecting
            .apply(StoredRecord::Operation(operation(9, 4, 1)))
            .is_ok());
        assert_eq!(rejecting.operation_count(), 2);
    }

    #[test]
    fn the_actor_head_is_the_highest_sequence_not_the_last_applied() {
        let mut index = Index::new();
        index
            .apply(StoredRecord::Operation(operation(5, 2, 5)))
            .unwrap();
        index
            .apply(StoredRecord::Operation(operation(1, 2, 1)))
            .unwrap();
        assert_eq!(index.actor_head(&digest(2)).unwrap().actor_sequence, 5);
        assert_eq!(
            index.rows("actor_head").unwrap(),
            vec![Row::new(vec![
                Value::blob(digest(2).as_bytes()),
                Value::blob(digest(5).as_bytes()),
                Value::Integer(5),
            ])]
        );
    }

    #[test]
    fn a_manifest_whose_chunks_leave_a_gap_is_refused() {
        let mut index = Index::new();
        let mut broken = manifest(1, &[10, 10]);
        broken.chunks[1].byte_offset = 11;
        assert_eq!(
            index.apply(StoredRecord::Manifest(broken)),
            Err(FoldError::DiscontiguousManifest { id: digest(1) })
        );
    }

    #[test]
    fn a_manifest_whose_chunks_overrun_its_length_is_refused() {
        let mut index = Index::new();
        let mut broken = manifest(1, &[10, 10]);
        broken.byte_length = 19;
        assert_eq!(
            index.apply(StoredRecord::Manifest(broken)),
            Err(FoldError::DiscontiguousManifest { id: digest(1) })
        );
    }

    #[test]
    fn an_empty_manifest_tiles_zero_bytes() {
        let mut index = Index::new();
        index
            .apply(StoredRecord::Manifest(manifest(1, &[])))
            .unwrap();
        assert_eq!(index.rows("manifest").unwrap().len(), 1);
        assert!(index.rows("manifest_chunk").unwrap().is_empty());
    }

    #[test]
    fn a_peer_that_joined_at_an_unknown_operation_is_refused() {
        let mut index = Index::new();
        assert_eq!(
            index.apply(StoredRecord::Peer(PeerRecord {
                peer: digest(7),
                joined_at: digest(99),
            })),
            Err(FoldError::DanglingReference {
                table: "peer",
                missing: digest(99)
            })
        );
    }

    /// The property that makes a rebuild order-insensitive: an out-of-order acknowledgement cannot
    /// move a watermark backwards.
    #[test]
    fn a_watermark_only_moves_forward() {
        let mut index = Index::new();
        index
            .apply(StoredRecord::Operation(operation(1, 2, 1)))
            .unwrap();
        index
            .apply(StoredRecord::Peer(PeerRecord {
                peer: digest(7),
                joined_at: digest(1),
            }))
            .unwrap();
        index
            .apply(StoredRecord::Acknowledgement(AckRecord {
                peer: digest(7),
                actor: digest(2),
                actor_sequence: 5,
            }))
            .unwrap();
        index
            .apply(StoredRecord::Acknowledgement(AckRecord {
                peer: digest(7),
                actor: digest(2),
                actor_sequence: 2,
            }))
            .unwrap();
        assert_eq!(
            index.rows("peer_watermark").unwrap(),
            vec![Row::new(vec![
                Value::blob(digest(7).as_bytes()),
                Value::blob(digest(2).as_bytes()),
                Value::Integer(5),
            ])]
        );
    }

    /// The outbox is a difference, so acknowledging empties it without anything being deleted.
    #[test]
    fn the_outbox_empties_as_the_watermark_catches_up() {
        let mut index = Index::new();
        index
            .apply(StoredRecord::Operation(operation(1, 2, 1)))
            .unwrap();
        index
            .apply(StoredRecord::Operation(operation(3, 2, 2)))
            .unwrap();
        index
            .apply(StoredRecord::Peer(PeerRecord {
                peer: digest(7),
                joined_at: digest(1),
            }))
            .unwrap();
        assert_eq!(index.rows("outbox").unwrap().len(), 2);

        index
            .apply(StoredRecord::Acknowledgement(AckRecord {
                peer: digest(7),
                actor: digest(2),
                actor_sequence: 1,
            }))
            .unwrap();
        assert_eq!(index.rows("outbox").unwrap().len(), 1);

        index
            .apply(StoredRecord::Acknowledgement(AckRecord {
                peer: digest(7),
                actor: digest(2),
                actor_sequence: 2,
            }))
            .unwrap();
        assert!(index.rows("outbox").unwrap().is_empty());
    }

    #[test]
    fn a_peer_is_not_queued_its_own_operations() {
        let mut index = Index::new();
        index
            .apply(StoredRecord::Operation(operation(1, 7, 1)))
            .unwrap();
        index
            .apply(StoredRecord::Peer(PeerRecord {
                peer: digest(7),
                joined_at: digest(1),
            }))
            .unwrap();
        assert!(index.rows("outbox").unwrap().is_empty());
    }

    #[test]
    fn an_approval_without_its_bundle_is_refused() {
        let mut index = Index::new();
        assert_eq!(
            index.apply(StoredRecord::Approval(ApprovalRecord {
                approval: digest(9),
                bundle: digest(8),
                approver: digest(2),
                verdict: ReviewVerdict::Approved,
            })),
            Err(FoldError::DanglingReference {
                table: "review_approval",
                missing: digest(8)
            })
        );
    }

    #[test]
    fn a_context_entry_for_an_unknown_operation_is_refused() {
        let mut index = Index::new();
        assert_eq!(
            index.apply(StoredRecord::ContextEntry(ContextRecord {
                entry: digest(10),
                session: no_session(),
                operation: digest(1),
                access: ContextAccess::Read,
                byte_length: 4,
            })),
            Err(FoldError::DanglingReference {
                table: "context_ledger",
                missing: digest(1)
            })
        );
    }

    #[test]
    fn the_digest_of_two_empty_indexes_agrees() {
        assert_eq!(Index::new().default_digest(), Index::new().default_digest());
    }

    #[test]
    fn one_extra_operation_moves_the_digest() {
        let mut one = Index::new();
        one.apply(StoredRecord::Operation(operation(1, 2, 1)))
            .unwrap();
        let before = one.default_digest();
        one.apply(StoredRecord::Operation(operation(3, 2, 2)))
            .unwrap();
        assert_ne!(before, one.default_digest());
    }

    /// Applying the same records in a different order must produce the same index, or a rebuild
    /// from a differently-ordered record stream would not match.
    #[test]
    fn the_fold_is_insensitive_to_the_order_of_independent_records() {
        let ops = [operation(1, 2, 1), operation(3, 4, 1), operation(5, 6, 1)];
        let mut forwards = Index::new();
        for op in &ops {
            forwards.apply(StoredRecord::Operation(op.clone())).unwrap();
        }
        let mut backwards = Index::new();
        for op in ops.iter().rev() {
            backwards
                .apply(StoredRecord::Operation(op.clone()))
                .unwrap();
        }
        assert_eq!(forwards.default_digest(), backwards.default_digest());
    }

    #[test]
    fn the_ledger_rows_are_part_of_the_digest() {
        let mut with_ledger = Index::new();
        with_ledger.set_ledger_rows(vec![Row::new(vec![
            Value::Integer(1),
            Value::blob([0; 16]),
        ])]);
        assert_ne!(with_ledger.default_digest(), Index::new().default_digest());
    }
}
