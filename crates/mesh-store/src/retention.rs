//! The retained-root set and the reachability computation the collector is checked against.
//!
//! Plan §6.4 says a version stays until an explicit retention policy permits deletion, and plan
//! §2.5 says no valid work is silently discarded. Both are statements about a *set*, so this module
//! makes the set a type: [`RetainedRoots`] is the complete, enumerable statement of what this
//! workspace promises to keep, and [`Reachability`] is what that promise closes over.
//!
//! # The asymmetry the whole module is built on
//!
//! Over-retention costs disk. Under-retention loses work. So every doubt resolves toward keeping:
//!
//! * A root naming something the index does not hold is a [`RetentionError`], not a skipped root.
//!   A root that resolved to nothing would silently shrink the retained set.
//! * A parent edge that leaves the index is *recorded* ([`Reachability::dangling_parents`]) and
//!   never treated as the end of the walk's obligations.
//! * [`RetainedRoots::conservative`] names every available actor head and complete actor-history
//!   window, every review bundle, every peer and every manifest, so the default collector frees
//!   only what no record mentions
//!   at all. Narrowing that set is a deliberate act with a name attached.
//!
//! # Nothing here reads a clock
//!
//! The one time-like question this module answers — "has an offline peer's claim on unsent content
//! expired?" — is answered in **policy epochs**, which are carried by the operations themselves
//! ([`crate::OperationRecord::policy_epoch`]). A peer expires when the workspace has moved
//! `offline_peer_expiry_epochs` epochs past the last epoch that peer acknowledged. That is derived
//! from immutable records, so it survives a rebuild and produces the same answer on every machine;
//! a wall-clock deadline would do neither and is banned besides.
//!
//! # What this module deliberately cannot do
//!
//! It cannot delete anything. It computes a set of digests and hands it on; `mesh-cas` owns the
//! bytes. The two crates share no types at all — neither declares a dependency — so the handoff is
//! `[u8; 32]`, and `crates/mesh-store/RETENTION.md` is where the two halves are written down
//! together.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::ids::RecordDigest;
use crate::index::Index;

/// One reason a piece of content is kept, named precisely enough to print in a dry run.
///
/// The variants are plan §6.4's list, in the task contract's order. A collector that wants to free
/// more disk removes a root from the set; it never gains a new way to ignore one, because there is
/// no "weak" root and no priority ordering between them.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RetainedRoot {
    /// The reviewed shared version everyone agrees on: an operation identifier.
    CanonicalHead(RecordDigest),
    /// An actor's own latest work, named by the *actor*, so the root follows the head forward
    /// rather than pinning the operation the root was written against.
    ActorHead(RecordDigest),
    /// A review bundle. Its subject operation and that operation's ancestry stay readable for as
    /// long as the review can be looked at.
    ReviewBundle(RecordDigest),
    /// An operation carrying a conflict nobody has resolved yet. Plan §4.8 preserves both sides;
    /// this is what keeps the bytes of both sides on disk.
    UnresolvedConflict(RecordDigest),
    /// A restore point somebody named, and the operation it names.
    RestorePoint {
        /// The name a human gave it.
        name: String,
        /// The operation it restores to.
        operation: RecordDigest,
    },
    /// A retention policy stated as a window over one actor's own chain: keep everything at or
    /// after this sequence number, whatever else happens to it.
    RetentionWindow {
        /// Whose chain.
        actor: RecordDigest,
        /// The lowest sequence number the window keeps.
        from_sequence: u64,
    },
    /// A peer that has not acknowledged everything it is owed. Everything it is still owed stays
    /// until it returns and acknowledges, or until the policy expires its claim.
    OfflinePeer(RecordDigest),
    /// A manifest named directly, which is how a caller that knows more than this crate does about
    /// which files are live states it.
    Manifest(RecordDigest),
}

impl core::fmt::Display for RetainedRoot {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::CanonicalHead(id) => write!(formatter, "canonical head {id}"),
            Self::ActorHead(actor) => write!(formatter, "head of actor {actor}"),
            Self::ReviewBundle(bundle) => write!(formatter, "review bundle {bundle}"),
            Self::UnresolvedConflict(id) => write!(formatter, "unresolved conflict at {id}"),
            Self::RestorePoint { name, operation } => {
                write!(formatter, "restore point {name:?} at {operation}")
            }
            Self::RetentionWindow {
                actor,
                from_sequence,
            } => write!(
                formatter,
                "retention window over actor {actor} from sequence {from_sequence}"
            ),
            Self::OfflinePeer(peer) => write!(formatter, "offline peer {peer}"),
            Self::Manifest(id) => write!(formatter, "manifest {id}"),
        }
    }
}

/// The parts of retention that are a policy choice rather than a fact about the workspace.
///
/// One field, on purpose. Every other question this module answers is decided by the record stream,
/// and a knob per question would be a second, disagreeing description of what the workspace keeps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RetentionPolicy {
    /// How many policy epochs past the last epoch a peer acknowledged its unsent content survives.
    ///
    /// `None` — the default — never expires a peer, which is the conservative reading of plan
    /// §2.5: an absent peer is presumed to be coming back. `Some(0)` expires a peer as soon as the
    /// workspace advances past the epoch it last acknowledged.
    pub offline_peer_expiry_epochs: Option<u64>,
}

/// The complete statement of what this workspace promises to keep.
///
/// Immutable: [`Self::with`] returns a new set rather than mutating, so a caller can narrow a
/// conservative set for one collection without the narrowing leaking into the next.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RetainedRoots {
    roots: BTreeSet<RetainedRoot>,
    policy: RetentionPolicy,
}

impl RetainedRoots {
    /// An empty set under a policy.
    ///
    /// Empty is legal to *construct* and refused by [`crate::CollectionPlan::compute`]: "retain
    /// nothing" is never what a caller meant, and the refusal is where that is said.
    #[must_use]
    pub fn new(policy: RetentionPolicy) -> Self {
        Self {
            roots: BTreeSet::new(),
            policy,
        }
    }

    /// This set plus one more root.
    #[must_use]
    pub fn with(&self, root: RetainedRoot) -> Self {
        let mut roots = self.roots.clone();
        roots.insert(root);
        Self {
            roots,
            policy: self.policy,
        }
    }

    /// This set minus one root, for a caller narrowing a conservative set deliberately.
    #[must_use]
    pub fn without(&self, root: &RetainedRoot) -> Self {
        let mut roots = self.roots.clone();
        roots.remove(root);
        Self {
            roots,
            policy: self.policy,
        }
    }

    /// Every available actor head, a complete recorded-history window for every known actor,
    /// each review bundle, each peer and each manifest. Buffered operations remain retained even
    /// when their missing causal parents prevent them from advancing an actor head.
    ///
    /// **This is the safe default and it is deliberately unhelpful about disk.** A collector run
    /// against it frees exactly the content no record in the index mentions — crash residue from
    /// plan §6.3's window between step 4 and step 9 — and nothing else. Freeing more means naming
    /// which of these roots is genuinely gone, which is a decision this crate cannot make and does
    /// not pretend to.
    #[must_use]
    pub fn conservative(index: &Index, policy: RetentionPolicy) -> Self {
        let mut roots = BTreeSet::new();
        for actor in index.actors() {
            if index.actor_head(&actor).is_some() {
                roots.insert(RetainedRoot::ActorHead(actor));
            }
            // A ready head cannot cover buffered or disconnected records. Default retention
            // promises to preserve every recorded operation, not only materializable history.
            roots.insert(RetainedRoot::RetentionWindow {
                actor,
                from_sequence: 0,
            });
        }
        for bundle in index.review_bundles() {
            roots.insert(RetainedRoot::ReviewBundle(bundle));
        }
        for peer in index.peer_ids() {
            roots.insert(RetainedRoot::OfflinePeer(peer));
        }
        for manifest in index.manifest_ids() {
            roots.insert(RetainedRoot::Manifest(manifest));
        }
        Self { roots, policy }
    }

    /// The roots, in their own total order.
    pub fn roots(&self) -> impl Iterator<Item = &RetainedRoot> {
        self.roots.iter()
    }

    /// How many roots the set holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.roots.len()
    }

    /// Whether the set names nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    /// The policy in force.
    #[must_use]
    pub fn policy(&self) -> RetentionPolicy {
        self.policy
    }
}

/// Why a retained-root set could not be resolved against an index.
///
/// Every variant means the same thing operationally: **do not collect**. A root that does not
/// resolve is a root whose content would silently stop being retained, so the computation fails
/// rather than returning a smaller set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RetentionError {
    /// A root names an operation the index does not hold.
    UnknownOperation {
        /// The operation named.
        id: RecordDigest,
        /// The root that named it.
        root: RetainedRoot,
    },
    /// A root names an actor the index has seen no operation from.
    UnknownActor {
        /// The actor named.
        actor: RecordDigest,
        /// The root that named it.
        root: RetainedRoot,
    },
    /// A root names a review bundle the index does not hold.
    UnknownReviewBundle {
        /// The bundle named.
        bundle: RecordDigest,
    },
    /// A root names a peer that never joined.
    UnknownPeer {
        /// The peer named.
        peer: RecordDigest,
    },
    /// A root names a manifest the index does not hold.
    UnknownManifest {
        /// The manifest named.
        id: RecordDigest,
    },
    /// A collection was asked for with nothing retained. Refused: see
    /// [`RetainedRoots::conservative`].
    NoRetainedRoots,
}

impl core::fmt::Display for RetentionError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnknownOperation { id, root } => {
                write!(
                    formatter,
                    "{root} names operation {id}, which is not indexed"
                )
            }
            Self::UnknownActor { actor, root } => write!(
                formatter,
                "{root} names actor {actor}, which has no indexed operation"
            ),
            Self::UnknownReviewBundle { bundle } => {
                write!(formatter, "review bundle {bundle} is not indexed")
            }
            Self::UnknownPeer { peer } => write!(formatter, "peer {peer} never joined"),
            Self::UnknownManifest { id } => write!(formatter, "manifest {id} is not indexed"),
            Self::NoRetainedRoots => formatter.write_str(
                "collection refused: the retained-root set is empty, which would retain nothing; \
                 build one with RetainedRoots::conservative and narrow it deliberately",
            ),
        }
    }
}

impl std::error::Error for RetentionError {}

/// Everything a retained-root set reaches, and which root reaches it.
///
/// The map is root-attributed rather than a bare set because the dry run has to say *why* a chunk
/// is kept, and a reason reconstructed after the fact is a reason that can be wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reachability {
    operations: BTreeMap<RecordDigest, RetainedRoot>,
    manifests: BTreeMap<RecordDigest, RetainedRoot>,
    content: BTreeMap<RecordDigest, RetainedRoot>,
    dangling_parents: BTreeSet<RecordDigest>,
    expired_peers: BTreeSet<RecordDigest>,
}

impl Reachability {
    /// Close a retained-root set over the index.
    ///
    /// The walk is: every root resolves to seed operations and manifests; every operation drags in
    /// its transitive parents; every reachable operation contributes its payload digest; every
    /// reachable manifest contributes its own content digest and every chunk it names. An operation
    /// whose payload digest *is* a manifest identifier also drags that manifest in, which is the
    /// only derived edge here and is stated as such in `RETENTION.md`.
    ///
    /// # Errors
    ///
    /// [`RetentionError`] when a root names something the index does not hold. Nothing partial is
    /// returned: an unresolvable root means the caller's picture of the workspace disagrees with
    /// the index, and collecting on the smaller of two disagreeing pictures is how content is lost.
    pub fn compute(index: &Index, roots: &RetainedRoots) -> Result<Self, RetentionError> {
        let expired_peers = expired_peers(index, roots.policy());

        let mut operations: BTreeMap<RecordDigest, RetainedRoot> = BTreeMap::new();
        let mut manifests: BTreeMap<RecordDigest, RetainedRoot> = BTreeMap::new();
        let mut queue: VecDeque<(RecordDigest, RetainedRoot)> = VecDeque::new();

        for root in roots.roots() {
            for seed in seeds_of(index, root, &expired_peers)? {
                match seed {
                    Seed::Operation(id) => queue.push_back((id, root.clone())),
                    Seed::Manifest(id) => {
                        manifests.entry(id).or_insert_with(|| root.clone());
                    }
                }
            }
        }

        let mut dangling_parents = BTreeSet::new();
        while let Some((id, root)) = queue.pop_front() {
            if operations.contains_key(&id) {
                continue;
            }
            let Some(operation) = index.operation(&id) else {
                dangling_parents.insert(id);
                continue;
            };
            operations.insert(id, root.clone());
            for parent in &operation.parents {
                if !operations.contains_key(parent) {
                    queue.push_back((*parent, root.clone()));
                }
            }
        }

        // The derived edge: an operation whose payload is itself a manifest keeps that manifest.
        // It only ever *adds* reachability, so a workspace where the relation does not hold loses
        // nothing by it.
        for (id, root) in &operations {
            let Some(operation) = index.operation(id) else {
                continue;
            };
            if index.manifest(&operation.payload_digest).is_some() {
                manifests
                    .entry(operation.payload_digest)
                    .or_insert_with(|| root.clone());
            }
        }

        let mut content: BTreeMap<RecordDigest, RetainedRoot> = BTreeMap::new();
        for (id, root) in &operations {
            if let Some(operation) = index.operation(id) {
                content
                    .entry(operation.payload_digest)
                    .or_insert_with(|| root.clone());
            }
        }
        for (id, root) in &manifests {
            let Some(manifest) = index.manifest(id) else {
                continue;
            };
            content
                .entry(manifest.content_digest)
                .or_insert_with(|| root.clone());
            for chunk in &manifest.chunks {
                content.entry(chunk.digest).or_insert_with(|| root.clone());
            }
        }

        Ok(Self {
            operations,
            manifests,
            content,
            dangling_parents,
            expired_peers,
        })
    }

    /// Whether any retained root reaches this content digest. The collector's whole question.
    #[must_use]
    pub fn retains(&self, digest: &RecordDigest) -> bool {
        self.content.contains_key(digest)
    }

    /// Which root keeps this content digest alive, if any.
    #[must_use]
    pub fn why_retained(&self, digest: &RecordDigest) -> Option<&RetainedRoot> {
        self.content.get(digest)
    }

    /// Every retained content digest, with the root that keeps it.
    pub fn content(&self) -> impl Iterator<Item = (&RecordDigest, &RetainedRoot)> {
        self.content.iter()
    }

    /// Every retained operation, with the root that keeps it.
    pub fn operations(&self) -> impl Iterator<Item = (&RecordDigest, &RetainedRoot)> {
        self.operations.iter()
    }

    /// Every retained manifest, with the root that keeps it.
    pub fn manifests(&self) -> impl Iterator<Item = (&RecordDigest, &RetainedRoot)> {
        self.manifests.iter()
    }

    /// Parent edges that left the index.
    ///
    /// Not an error: an operation whose parent this node never received is normal in a partially
    /// replicated workspace, and its content is not here to collect. It is *reported* because a
    /// growing set here means the local index is losing history, which is a different problem that
    /// a silent skip would hide.
    pub fn dangling_parents(&self) -> impl Iterator<Item = &RecordDigest> {
        self.dangling_parents.iter()
    }

    /// Peers whose claim on unsent content the policy has expired.
    pub fn expired_peers(&self) -> impl Iterator<Item = &RecordDigest> {
        self.expired_peers.iter()
    }

    /// How many content digests are retained.
    #[must_use]
    pub fn retained_content_count(&self) -> usize {
        self.content.len()
    }
}

/// What a root resolves to before the walk starts.
enum Seed {
    Operation(RecordDigest),
    Manifest(RecordDigest),
}

fn seeds_of(
    index: &Index,
    root: &RetainedRoot,
    expired_peers: &BTreeSet<RecordDigest>,
) -> Result<Vec<Seed>, RetentionError> {
    match root {
        RetainedRoot::CanonicalHead(id)
        | RetainedRoot::UnresolvedConflict(id)
        | RetainedRoot::RestorePoint { operation: id, .. } => {
            if index.operation(id).is_none() {
                return Err(RetentionError::UnknownOperation {
                    id: *id,
                    root: root.clone(),
                });
            }
            Ok(vec![Seed::Operation(*id)])
        }
        RetainedRoot::ActorHead(actor) => {
            let head = index
                .actor_head(actor)
                .ok_or_else(|| RetentionError::UnknownActor {
                    actor: *actor,
                    root: root.clone(),
                })?;
            Ok(vec![Seed::Operation(head.id)])
        }
        RetainedRoot::ReviewBundle(bundle) => {
            let review = index
                .review(bundle)
                .ok_or(RetentionError::UnknownReviewBundle { bundle: *bundle })?;
            Ok(vec![Seed::Operation(review.subject_operation)])
        }
        RetainedRoot::RetentionWindow {
            actor,
            from_sequence,
        } => {
            if index.operations_of(actor).next().is_none() {
                return Err(RetentionError::UnknownActor {
                    actor: *actor,
                    root: root.clone(),
                });
            }
            Ok(index
                .operations_of(actor)
                .filter(|operation| operation.actor_sequence >= *from_sequence)
                .map(|operation| Seed::Operation(operation.id))
                .collect())
        }
        RetainedRoot::OfflinePeer(peer) => {
            if !index.peer_ids().contains(peer) {
                return Err(RetentionError::UnknownPeer { peer: *peer });
            }
            if expired_peers.contains(peer) {
                return Ok(Vec::new());
            }
            Ok(index
                .owed_to_peer(peer)
                .into_iter()
                .map(Seed::Operation)
                .collect())
        }
        RetainedRoot::Manifest(id) => {
            if index.manifest(id).is_none() {
                return Err(RetentionError::UnknownManifest { id: *id });
            }
            Ok(vec![Seed::Manifest(*id)])
        }
    }
}

/// Peers whose claim the policy has expired, in policy epochs and never in seconds.
///
/// A peer's "last epoch" is the highest policy epoch among the operations its watermarks name; a
/// peer that has acknowledged nothing falls back to the epoch of the operation it joined at. The
/// workspace's epoch is the highest any indexed operation carries. The peer expires when the
/// difference exceeds the policy's allowance.
fn expired_peers(index: &Index, policy: RetentionPolicy) -> BTreeSet<RecordDigest> {
    let Some(allowance) = policy.offline_peer_expiry_epochs else {
        return BTreeSet::new();
    };
    let Some(current) = index.highest_policy_epoch() else {
        return BTreeSet::new();
    };
    index
        .peer_ids()
        .into_iter()
        .filter(|peer| match index.peer_last_acknowledged_epoch(peer) {
            Some(last) => current.saturating_sub(last) > allowance,
            None => false,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{ChunkSlice, ManifestRecord, OperationRecord, StoredRecord};
    use crate::EntityUuid;

    fn digest(seed: u8) -> RecordDigest {
        RecordDigest::from_bytes([seed; 32])
    }

    fn operation(id: u8, actor: u8, sequence: u64, payload: u8) -> OperationRecord {
        OperationRecord {
            id: digest(id),
            actor: digest(actor),
            actor_sequence: sequence,
            hlc_millis: 0,
            hlc_counter: 0,
            policy_epoch: 1,
            session: EntityUuid::from_bytes([0; 16]),
            payload_digest: digest(payload),
            parents: Vec::new(),
        }
    }

    #[test]
    fn an_actor_head_root_retains_its_whole_ancestry() {
        let mut index = Index::new();
        let first = operation(1, 100, 1, 10);
        let mut second = operation(2, 100, 2, 11);
        second.parents = vec![digest(1)];
        index.apply(StoredRecord::Operation(first)).unwrap();
        index.apply(StoredRecord::Operation(second)).unwrap();

        let roots = RetainedRoots::new(RetentionPolicy::default())
            .with(RetainedRoot::ActorHead(digest(100)));
        let reach = Reachability::compute(&index, &roots).unwrap();
        assert!(reach.retains(&digest(10)));
        assert!(reach.retains(&digest(11)));
    }

    #[test]
    fn a_root_that_names_nothing_is_an_error_rather_than_a_smaller_set() {
        let index = Index::new();
        let roots = RetainedRoots::new(RetentionPolicy::default())
            .with(RetainedRoot::CanonicalHead(digest(7)));
        assert_eq!(
            Reachability::compute(&index, &roots),
            Err(RetentionError::UnknownOperation {
                id: digest(7),
                root: RetainedRoot::CanonicalHead(digest(7)),
            })
        );
    }

    #[test]
    fn a_manifest_root_retains_every_chunk_it_names() {
        let mut index = Index::new();
        index
            .apply(StoredRecord::Manifest(ManifestRecord {
                id: digest(50),
                byte_length: 6,
                content_digest: digest(51),
                chunks: vec![
                    ChunkSlice {
                        digest: digest(52),
                        byte_offset: 0,
                        byte_length: 3,
                    },
                    ChunkSlice {
                        digest: digest(53),
                        byte_offset: 3,
                        byte_length: 3,
                    },
                ],
            }))
            .unwrap();
        let roots =
            RetainedRoots::new(RetentionPolicy::default()).with(RetainedRoot::Manifest(digest(50)));
        let reach = Reachability::compute(&index, &roots).unwrap();
        assert!(reach.retains(&digest(52)));
        assert!(reach.retains(&digest(53)));
        assert!(reach.retains(&digest(51)));
    }

    #[test]
    fn a_root_set_is_immutable_under_with_and_without() {
        let base = RetainedRoots::new(RetentionPolicy::default());
        let one = base.with(RetainedRoot::CanonicalHead(digest(1)));
        assert!(base.is_empty());
        assert_eq!(one.len(), 1);
        assert!(one
            .without(&RetainedRoot::CanonicalHead(digest(1)))
            .is_empty());
        assert_eq!(one.len(), 1);
    }

    #[test]
    fn every_root_prints_something_a_dry_run_can_show() {
        let roots = [
            RetainedRoot::CanonicalHead(digest(1)),
            RetainedRoot::ActorHead(digest(2)),
            RetainedRoot::ReviewBundle(digest(3)),
            RetainedRoot::UnresolvedConflict(digest(4)),
            RetainedRoot::RestorePoint {
                name: "before the refactor".to_owned(),
                operation: digest(5),
            },
            RetainedRoot::RetentionWindow {
                actor: digest(6),
                from_sequence: 3,
            },
            RetainedRoot::OfflinePeer(digest(7)),
            RetainedRoot::Manifest(digest(8)),
        ];
        for root in roots {
            let rendered = root.to_string();
            assert!(!rendered.is_empty());
            assert!(rendered.chars().next().is_some_and(char::is_alphabetic));
        }
    }
}
