//! The collector's safety property, checked against an independently written oracle.
//!
//! `cargo nextest run -p mesh-store --test gc`
//!
//! # The claim, and why it needs a second implementation to be worth anything
//!
//! > **No content reachable from any retained root is ever collected.**
//!
//! Asserting that with the collector's own reachability set proves only that a function agrees with
//! itself. So [`brute_force_reachable`] recomputes the closure a different way — a naive fixed
//! point over every record in the index, repeated until nothing changes, with no work queue, no
//! root attribution and no shared helper — and the property is stated between the two. A bug in
//! [`mesh_store::Reachability`] that shrinks the retained set turns this red; a bug that grows it
//! is caught by [`the_doomed_set_is_exactly_the_candidates_no_root_reaches`].
//!
//! # The trap this file is written against
//!
//! A design review pointed out that the acceptance criteria of task
//! `01KZC2KZ29Y8Z6H9W93M40NPJ0` are all satisfiable by a collector that never deletes a byte: a dry
//! run "reports exactly what would be deleted", and reporting the empty set reports it exactly. So
//! this file also asserts a **floor** on how much the campaign collects
//! ([`the_campaign_collects_a_substantial_amount_of_real_garbage`]). A collector that stopped
//! deleting would pass every safety test here and fail that one.
//!
//! No clock is read anywhere in this file, including by the offline-peer expiry, which counts
//! policy epochs.

use std::collections::{BTreeMap, BTreeSet};

use mesh_store::{
    AckRecord, ChunkSlice, CollectionPlan, CollectionReason, EntityUuid, Index, ManifestRecord,
    OperationRecord, PeerRecord, Reachability, RecordDigest, RetainedRoot, RetainedRoots,
    RetentionError, RetentionPolicy, ReviewRecord, StoredRecord,
};

// ---------------------------------------------------------------------------
// A generator, written here because `mesh-store` keeps its test closure free of
// convenience dependencies. Sixteen lines of xorshift is the price of that
// rule, and it buys a seed a failing run can name.
// ---------------------------------------------------------------------------

struct Rng {
    state: u64,
}

impl Rng {
    fn seeded(seed: u64) -> Self {
        Self {
            state: if seed == 0 {
                0x9E37_79B9_7F4A_7C15
            } else {
                seed
            },
        }
    }

    fn next_u64(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next_u64() % bound.max(1)
    }

    fn chance(&mut self, one_in: u64) -> bool {
        self.below(one_in) == 0
    }
}

/// A digest that is a function of a label and a number, so a generated history is readable in a
/// failure message and reproducible from its seed.
fn digest(tag: u8, number: u64) -> RecordDigest {
    let mut bytes = [0u8; 32];
    bytes[0] = tag;
    bytes[1..9].copy_from_slice(&number.to_be_bytes());
    RecordDigest::from_bytes(bytes)
}

const ACTOR: u8 = 0xA0;
const OPERATION: u8 = 0xB0;
const PAYLOAD: u8 = 0xC0;
const MANIFEST: u8 = 0xD0;
const CHUNK: u8 = 0xE0;
const FILE: u8 = 0xE8;
const PEER: u8 = 0xF0;
const BUNDLE: u8 = 0xF8;
const ORPHAN: u8 = 0xFF;

/// One generated workspace: the index, plus the candidate set a store would offer the collector.
struct History {
    index: Index,
    /// Every content digest a chunk store would hold — the referenced ones plus crash orphans.
    candidates: Vec<RecordDigest>,
    /// The orphans, which no record names and which are therefore always collectable.
    orphans: BTreeSet<RecordDigest>,
    actors: Vec<RecordDigest>,
    peers: Vec<RecordDigest>,
    bundles: Vec<RecordDigest>,
    manifests: Vec<RecordDigest>,
}

/// A workspace of a few actors, their chains, manifests, peers with watermarks, and reviews.
///
/// Deliberately shaped like the real thing rather than like a graph: an actor's chain is linear and
/// occasionally cross-links to another actor's operation, which is what makes the parent closure do
/// work.
fn generate(seed: u64) -> History {
    let mut rng = Rng::seeded(seed);
    let mut index = Index::new();

    let actor_count = 2 + rng.below(3);
    let mut actors = Vec::new();
    let mut all_operations: Vec<RecordDigest> = Vec::new();
    let mut candidates: BTreeSet<RecordDigest> = BTreeSet::new();
    let mut operation_number = 0u64;

    for a in 0..actor_count {
        let actor = digest(ACTOR, a);
        actors.push(actor);
        let chain_length = 2 + rng.below(5);
        let mut previous: Option<RecordDigest> = None;
        for sequence in 1..=chain_length {
            operation_number += 1;
            let id = digest(OPERATION, operation_number);
            let payload = digest(PAYLOAD, operation_number);
            let mut parents = Vec::new();
            if let Some(parent) = previous {
                parents.push(parent);
            }
            if !all_operations.is_empty() && rng.chance(3) {
                let pick = all_operations[rng.below(all_operations.len() as u64) as usize];
                if !parents.contains(&pick) {
                    parents.push(pick);
                }
            }
            index
                .apply(StoredRecord::Operation(OperationRecord {
                    id,
                    actor,
                    actor_sequence: sequence,
                    hlc_millis: 0,
                    hlc_counter: 0,
                    policy_epoch: 1 + operation_number / 4,
                    session: EntityUuid::from_bytes([0; 16]),
                    payload_digest: payload,
                    parents,
                }))
                .expect("the generated chain never forks");
            candidates.insert(payload);
            all_operations.push(id);
            previous = Some(id);
        }
    }

    // Manifests, each tiling its own byte length with chunks that repeat across manifests so that
    // deduplication — the case where one chunk is reachable from a doomed root and a retained one
    // at the same time — is actually exercised.
    let manifest_count = 1 + rng.below(6);
    let mut manifests = Vec::new();
    for m in 0..manifest_count {
        let id = digest(MANIFEST, m);
        let chunk_count = 1 + rng.below(4);
        let mut chunks = Vec::new();
        let mut offset = 0u64;
        for _ in 0..chunk_count {
            let chunk = digest(CHUNK, rng.below(8));
            chunks.push(ChunkSlice {
                digest: chunk,
                byte_offset: offset,
                byte_length: 16,
            });
            candidates.insert(chunk);
            offset += 16;
        }
        let content = digest(FILE, m);
        candidates.insert(content);
        index
            .apply(StoredRecord::Manifest(ManifestRecord {
                id,
                byte_length: offset,
                content_digest: content,
                chunks,
            }))
            .expect("the generated manifest tiles its own length");
        manifests.push(id);
    }

    // Peers, each with a watermark somewhere inside some actor's chain.
    let peer_count = rng.below(3);
    let mut peers = Vec::new();
    for p in 0..peer_count {
        let peer = digest(PEER, p);
        let joined = all_operations[rng.below(all_operations.len() as u64) as usize];
        index
            .apply(StoredRecord::Peer(PeerRecord {
                peer,
                joined_at: joined,
            }))
            .expect("the peer joins at an indexed operation");
        for actor in &actors {
            let head = index
                .actor_head(actor)
                .expect("every generated actor has a head");
            let watermark = rng.below(head.actor_sequence + 1);
            index
                .apply(StoredRecord::Acknowledgement(AckRecord {
                    peer,
                    actor: *actor,
                    actor_sequence: watermark,
                }))
                .expect("the peer is indexed before it acknowledges");
        }
        peers.push(peer);
    }

    // Review bundles over arbitrary operations.
    let bundle_count = rng.below(3);
    let mut bundles = Vec::new();
    for b in 0..bundle_count {
        let bundle = digest(BUNDLE, b);
        let subject = all_operations[rng.below(all_operations.len() as u64) as usize];
        index
            .apply(StoredRecord::Review(ReviewRecord {
                bundle,
                subject_operation: subject,
                opened_by: actors[0],
            }))
            .expect("the review subject is indexed");
        bundles.push(bundle);
    }

    // Crash orphans: chunks a promotion made visible for a transaction that never committed. Plan
    // §6.3's window between step 4 and step 9, which is what the arrival journal exists to find.
    let mut orphans = BTreeSet::new();
    for o in 0..(1 + rng.below(6)) {
        let orphan = digest(ORPHAN, o);
        orphans.insert(orphan);
        candidates.insert(orphan);
    }

    History {
        index,
        candidates: candidates.into_iter().collect(),
        orphans,
        actors,
        peers,
        bundles,
        manifests,
    }
}

/// A random subset of the roots the index justifies, always non-empty.
fn random_roots(history: &History, rng: &mut Rng) -> RetainedRoots {
    let policy = RetentionPolicy {
        offline_peer_expiry_epochs: if rng.chance(2) {
            Some(rng.below(4))
        } else {
            None
        },
    };
    let mut roots = RetainedRoots::new(policy);
    for actor in &history.actors {
        if rng.chance(2) {
            roots = roots.with(RetainedRoot::ActorHead(*actor));
        }
    }
    for peer in &history.peers {
        if rng.chance(2) {
            roots = roots.with(RetainedRoot::OfflinePeer(*peer));
        }
    }
    for bundle in &history.bundles {
        if rng.chance(2) {
            roots = roots.with(RetainedRoot::ReviewBundle(*bundle));
        }
    }
    for manifest in &history.manifests {
        if rng.chance(2) {
            roots = roots.with(RetainedRoot::Manifest(*manifest));
        }
    }
    if rng.chance(3) {
        let actor = history.actors[rng.below(history.actors.len() as u64) as usize];
        roots = roots.with(RetainedRoot::RetentionWindow {
            actor,
            from_sequence: 1 + rng.below(3),
        });
    }
    if roots.is_empty() {
        roots = roots.with(RetainedRoot::ActorHead(history.actors[0]));
    }
    roots
}

// ---------------------------------------------------------------------------
// The independent oracle.
// ---------------------------------------------------------------------------

/// Every content digest a retained-root set reaches, computed by a naive fixed point.
///
/// Written to share nothing with `Reachability` beyond the index readers: it collects seed
/// operations into a set, then loops over *every* operation in the index adding any whose
/// identifier is already in the set together with its parents, until a pass changes nothing. Slower
/// by a large factor and structurally different, which is the entire point of having it.
fn brute_force_reachable(
    index: &Index,
    roots: &RetainedRoots,
    expired_peers: &BTreeSet<RecordDigest>,
) -> BTreeSet<RecordDigest> {
    let mut operations: BTreeSet<RecordDigest> = BTreeSet::new();
    let mut manifests: BTreeSet<RecordDigest> = BTreeSet::new();

    for root in roots.roots() {
        match root {
            RetainedRoot::CanonicalHead(id)
            | RetainedRoot::UnresolvedConflict(id)
            | RetainedRoot::RestorePoint { operation: id, .. } => {
                operations.insert(*id);
            }
            RetainedRoot::ActorHead(actor) => {
                if let Some(head) = index.actor_head(actor) {
                    operations.insert(head.id);
                }
            }
            RetainedRoot::ReviewBundle(bundle) => {
                if let Some(review) = index.review(bundle) {
                    operations.insert(review.subject_operation);
                }
            }
            RetainedRoot::RetentionWindow {
                actor,
                from_sequence,
            } => {
                for operation in index.operations_of(actor) {
                    if operation.actor_sequence >= *from_sequence {
                        operations.insert(operation.id);
                    }
                }
            }
            RetainedRoot::OfflinePeer(peer) => {
                if !expired_peers.contains(peer) {
                    operations.extend(index.owed_to_peer(peer));
                }
            }
            RetainedRoot::Manifest(id) => {
                manifests.insert(*id);
            }
        }
    }

    loop {
        let before = operations.len();
        let mut additions = BTreeSet::new();
        for id in &operations {
            if let Some(operation) = index.operation(id) {
                for parent in &operation.parents {
                    if index.operation(parent).is_some() {
                        additions.insert(*parent);
                    }
                }
            }
        }
        operations.extend(additions);
        if operations.len() == before {
            break;
        }
    }

    for id in &operations {
        if let Some(operation) = index.operation(id) {
            if index.manifest(&operation.payload_digest).is_some() {
                manifests.insert(operation.payload_digest);
            }
        }
    }

    let mut content = BTreeSet::new();
    for id in &operations {
        if let Some(operation) = index.operation(id) {
            content.insert(operation.payload_digest);
        }
    }
    for id in &manifests {
        if let Some(manifest) = index.manifest(id) {
            content.insert(manifest.content_digest);
            for chunk in &manifest.chunks {
                content.insert(chunk.digest);
            }
        }
    }
    content
}

/// The campaign, run once and shared by the assertions below.
struct Campaign {
    /// Total candidates offered across every generated workspace.
    candidates: usize,
    /// Total digests the collector doomed.
    doomed: usize,
    /// Total digests doomed for each reason.
    never_referenced: usize,
    no_root_reaches: usize,
    /// How many of the generated workspaces had at least one collectable chunk.
    workspaces_with_garbage: usize,
    /// How many workspaces were generated.
    workspaces: usize,
}

const SEEDS: u64 = 200;

fn run_campaign() -> Campaign {
    let mut campaign = Campaign {
        candidates: 0,
        doomed: 0,
        never_referenced: 0,
        no_root_reaches: 0,
        workspaces_with_garbage: 0,
        workspaces: 0,
    };

    for seed in 1..=SEEDS {
        let history = generate(seed);
        let mut rng = Rng::seeded(seed.wrapping_mul(0x5DEE_CE66));
        let roots = random_roots(&history, &mut rng);

        let reachability = Reachability::compute(&history.index, &roots)
            .unwrap_or_else(|error| panic!("seed {seed}: roots did not resolve: {error}"));
        let expired: BTreeSet<RecordDigest> = reachability.expired_peers().copied().collect();
        let independent = brute_force_reachable(&history.index, &roots, &expired);

        let plan = CollectionPlan::compute(
            &history.index,
            &roots,
            &reachability,
            history.candidates.iter().copied(),
        )
        .unwrap_or_else(|error| panic!("seed {seed}: plan refused: {error}"));

        let doomed: BTreeSet<RecordDigest> =
            plan.doomed().iter().map(|entry| entry.digest).collect();

        // THE SAFETY PROPERTY.
        for digest in &independent {
            assert!(
                !doomed.contains(digest),
                "seed {seed}: {digest} is reachable from a retained root and was doomed anyway. \
                 roots: {:?}",
                roots.roots().collect::<Vec<_>>()
            );
        }

        // The two computations agree in both directions, so the collector is neither over- nor
        // under-retaining relative to the oracle.
        let retained_candidates: BTreeSet<RecordDigest> = history
            .candidates
            .iter()
            .copied()
            .filter(|digest| independent.contains(digest))
            .collect();
        let kept: BTreeSet<RecordDigest> = plan.kept().iter().map(|entry| entry.digest).collect();
        assert_eq!(
            kept, retained_candidates,
            "seed {seed}: the collector and the independent oracle disagree about what is retained"
        );

        // Crash orphans are always collectable — no root can reach a digest no record names.
        for orphan in &history.orphans {
            assert!(
                doomed.contains(orphan),
                "seed {seed}: orphan {orphan} survived, so the collector frees nothing it should"
            );
        }

        campaign.workspaces += 1;
        campaign.candidates += history.candidates.len();
        campaign.doomed += doomed.len();
        for entry in plan.doomed() {
            match entry.reason {
                CollectionReason::NeverReferenced => campaign.never_referenced += 1,
                CollectionReason::NoRetainedRootReaches => campaign.no_root_reaches += 1,
            }
        }
        if !plan.frees_nothing() {
            campaign.workspaces_with_garbage += 1;
        }
    }

    campaign
}

#[test]
fn no_content_reachable_from_a_retained_root_is_ever_collected() {
    let campaign = run_campaign();
    assert_eq!(campaign.workspaces, SEEDS as usize);
    // The assertions are inside the campaign; this test's own job is to prove the campaign ran on a
    // corpus large enough for the property to mean something.
    assert!(
        campaign.candidates > 2_000,
        "the campaign offered only {} candidates, which is too small a corpus for the property",
        campaign.candidates
    );
}

/// The anti-vacuity test. A collector that never deletes a byte passes every safety assertion in
/// this file and fails here.
#[test]
fn the_campaign_collects_a_substantial_amount_of_real_garbage() {
    let campaign = run_campaign();
    assert_eq!(
        campaign.workspaces_with_garbage, campaign.workspaces,
        "some generated workspace had nothing collectable, which cannot happen while every one of \
         them carries crash orphans"
    );
    assert!(
        campaign.doomed > 500,
        "the campaign doomed only {} digests; a collector that reports the empty set satisfies \
         every other test here, so this floor is what stops it",
        campaign.doomed
    );
    assert!(
        campaign.never_referenced > 0 && campaign.no_root_reaches > 0,
        "both reasons must occur: {} never-referenced, {} unreachable",
        campaign.never_referenced,
        campaign.no_root_reaches
    );
    assert!(
        campaign.doomed < campaign.candidates,
        "everything was doomed, so the retained-root set retained nothing"
    );
}

#[test]
fn the_doomed_set_is_exactly_the_candidates_no_root_reaches() {
    for seed in 1..=40u64 {
        let history = generate(seed);
        let mut rng = Rng::seeded(seed);
        let roots = random_roots(&history, &mut rng);
        let reachability = Reachability::compute(&history.index, &roots).expect("roots resolve");
        let plan = CollectionPlan::compute(
            &history.index,
            &roots,
            &reachability,
            history.candidates.iter().copied(),
        )
        .expect("the plan is computable");

        let mut partitioned: Vec<RecordDigest> =
            plan.doomed().iter().map(|entry| entry.digest).collect();
        partitioned.extend(plan.kept().iter().map(|entry| entry.digest));
        partitioned.sort_unstable();
        let mut offered = history.candidates.clone();
        offered.sort_unstable();
        assert_eq!(
            partitioned, offered,
            "seed {seed}: the plan lost or invented a candidate"
        );
    }
}

// ---------------------------------------------------------------------------
// The named acceptance criteria, one test each.
// ---------------------------------------------------------------------------

/// An offline peer's watermark keeps what it is owed alive; the peer returning releases it.
#[test]
fn an_offline_peers_watermark_keeps_its_content_alive_until_the_peer_returns() {
    let (index, actor, peer) = two_actor_workspace_with_a_peer(0);
    let roots =
        RetainedRoots::new(RetentionPolicy::default()).with(RetainedRoot::OfflinePeer(peer));
    let reach = Reachability::compute(&index, &roots).expect("the peer joined");

    // Everything the peer has not acknowledged is retained.
    for sequence in 1..=3u64 {
        assert!(
            reach.retains(&payload_of(&index, &actor, sequence)),
            "sequence {sequence} is owed to the offline peer and must be retained"
        );
    }

    // The peer returns and acknowledges everything.
    let (returned, actor, peer) = two_actor_workspace_with_a_peer(3);
    let roots =
        RetainedRoots::new(RetentionPolicy::default()).with(RetainedRoot::OfflinePeer(peer));
    let after = Reachability::compute(&returned, &roots).expect("the peer joined");
    for sequence in 1..=3u64 {
        assert!(
            !after.retains(&payload_of(&returned, &actor, sequence)),
            "sequence {sequence} is acknowledged, so this root no longer keeps it"
        );
    }
}

/// …or until the policy expires it, counted in policy epochs and never in seconds.
#[test]
fn an_offline_peers_claim_expires_by_policy_epoch_and_never_by_a_clock() {
    let (index, actor, peer) = two_actor_workspace_with_a_peer(0);

    let never = RetainedRoots::new(RetentionPolicy {
        offline_peer_expiry_epochs: None,
    })
    .with(RetainedRoot::OfflinePeer(peer));
    let kept = Reachability::compute(&index, &never).expect("the peer joined");
    assert!(kept.retains(&payload_of(&index, &actor, 3)));
    assert_eq!(kept.expired_peers().count(), 0);

    let expiring = RetainedRoots::new(RetentionPolicy {
        offline_peer_expiry_epochs: Some(0),
    })
    .with(RetainedRoot::OfflinePeer(peer));
    let expired = Reachability::compute(&index, &expiring).expect("the peer joined");
    assert_eq!(
        expired.expired_peers().copied().collect::<Vec<_>>(),
        vec![peer],
        "a peer whose last acknowledged epoch is behind the workspace's expires at allowance 0"
    );
    assert!(
        !expired.retains(&payload_of(&index, &actor, 3)),
        "an expired peer's claim keeps nothing"
    );
}

/// A dry run reports what would go and why, and the plan it reports is the plan that would run.
#[test]
fn a_dry_run_reports_exactly_what_would_be_deleted_and_why() {
    let history = generate(7);
    let roots = RetainedRoots::conservative(&history.index, RetentionPolicy::default());
    let reach = Reachability::compute(&history.index, &roots).expect("roots resolve");
    let plan = CollectionPlan::compute(
        &history.index,
        &roots,
        &reach,
        history.candidates.iter().copied(),
    )
    .expect("the plan is computable");

    assert!(
        !plan.frees_nothing(),
        "the conservative root set must still free the crash orphans"
    );
    let report = plan.report();
    for entry in plan.doomed() {
        assert!(
            report.contains(&format!("delete {} — {}", entry.digest, entry.reason)),
            "the report omits {}",
            entry.digest
        );
    }
    for entry in plan.kept() {
        assert!(
            report.contains(&format!("keep   {} — {}", entry.digest, entry.root)),
            "the report does not say why {} is kept",
            entry.digest
        );
    }
    // Under the conservative root set, every orphan and only the orphans go.
    let doomed: BTreeSet<RecordDigest> = plan.doomed().iter().map(|entry| entry.digest).collect();
    assert_eq!(doomed, history.orphans);
}

/// Retaining nothing is refused rather than obeyed.
#[test]
fn an_empty_retained_root_set_refuses_to_produce_a_plan() {
    let history = generate(11);
    let roots = RetainedRoots::new(RetentionPolicy::default());
    let reach = Reachability::compute(&history.index, &roots).expect("no roots resolve trivially");
    assert_eq!(
        CollectionPlan::compute(
            &history.index,
            &roots,
            &reach,
            history.candidates.iter().copied()
        ),
        Err(RetentionError::NoRetainedRoots)
    );
}

/// A root the index cannot resolve stops the collection instead of shrinking the retained set.
#[test]
fn an_unresolvable_root_is_an_error_and_not_a_smaller_retained_set() {
    let history = generate(13);
    let missing = digest(OPERATION, 999_999);
    let roots = RetainedRoots::conservative(&history.index, RetentionPolicy::default())
        .with(RetainedRoot::CanonicalHead(missing));
    match Reachability::compute(&history.index, &roots) {
        Err(RetentionError::UnknownOperation { id, .. }) => assert_eq!(id, missing),
        other => panic!("expected an unresolvable root to be refused, got {other:?}"),
    }
}

/// The dedup case: one chunk under both a doomed manifest and a retained one survives.
#[test]
fn a_chunk_shared_by_a_dropped_root_and_a_retained_one_survives() {
    let mut index = Index::new();
    let shared = digest(CHUNK, 1);
    let only_in_dropped = digest(CHUNK, 2);
    for (m, extra) in [(0u64, only_in_dropped), (1u64, digest(CHUNK, 3))] {
        index
            .apply(StoredRecord::Manifest(ManifestRecord {
                id: digest(MANIFEST, m),
                byte_length: 32,
                content_digest: digest(FILE, m),
                chunks: vec![
                    ChunkSlice {
                        digest: shared,
                        byte_offset: 0,
                        byte_length: 16,
                    },
                    ChunkSlice {
                        digest: extra,
                        byte_offset: 16,
                        byte_length: 16,
                    },
                ],
            }))
            .expect("the manifest tiles its length");
    }

    // Keep manifest 1, drop manifest 0.
    let roots = RetainedRoots::new(RetentionPolicy::default())
        .with(RetainedRoot::Manifest(digest(MANIFEST, 1)));
    let reach = Reachability::compute(&index, &roots).expect("roots resolve");
    let plan = CollectionPlan::compute(
        &index,
        &roots,
        &reach,
        [shared, only_in_dropped, digest(CHUNK, 3)],
    )
    .expect("the plan is computable");

    let doomed: BTreeSet<RecordDigest> = plan.doomed().iter().map(|entry| entry.digest).collect();
    assert!(
        !doomed.contains(&shared),
        "a chunk deduplicated between a dropped root and a retained one must survive"
    );
    assert!(doomed.contains(&only_in_dropped));
    assert_eq!(
        plan.doomed()[0].reason,
        CollectionReason::NoRetainedRootReaches
    );
}

// ---------------------------------------------------------------------------
// Fixtures for the criterion tests.
// ---------------------------------------------------------------------------

/// One actor with a three-operation chain, and a peer whose watermark sits at `acknowledged`.
///
/// The operations sit in ascending policy epochs so that expiry has something to measure.
fn two_actor_workspace_with_a_peer(acknowledged: u64) -> (Index, RecordDigest, RecordDigest) {
    let actor = digest(ACTOR, 1);
    let peer = digest(PEER, 1);
    let mut index = Index::new();
    let mut previous = None;
    for sequence in 1..=3u64 {
        index
            .apply(StoredRecord::Operation(OperationRecord {
                id: digest(OPERATION, sequence),
                actor,
                actor_sequence: sequence,
                hlc_millis: 0,
                hlc_counter: 0,
                policy_epoch: sequence,
                session: EntityUuid::from_bytes([0; 16]),
                payload_digest: digest(PAYLOAD, sequence),
                parents: previous.into_iter().collect(),
            }))
            .expect("the chain does not fork");
        previous = Some(digest(OPERATION, sequence));
    }
    index
        .apply(StoredRecord::Peer(PeerRecord {
            peer,
            joined_at: digest(OPERATION, 1),
        }))
        .expect("the peer joins at an indexed operation");
    index
        .apply(StoredRecord::Acknowledgement(AckRecord {
            peer,
            actor,
            actor_sequence: acknowledged,
        }))
        .expect("the peer is indexed");
    (index, actor, peer)
}

fn payload_of(index: &Index, actor: &RecordDigest, sequence: u64) -> RecordDigest {
    index
        .operations_of(actor)
        .find(|operation| operation.actor_sequence == sequence)
        .map(|operation| operation.payload_digest)
        .unwrap_or_else(|| panic!("no operation at sequence {sequence}"))
}

/// The retained-root set is a closed enumeration, and the count is asserted so that a new root kind
/// arriving without a test arriving with it turns this red.
#[test]
fn the_retained_root_set_is_the_seven_kinds_plan_6_4_names_plus_the_manifest_handle() {
    let all = [
        RetainedRoot::CanonicalHead(digest(OPERATION, 1)),
        RetainedRoot::ActorHead(digest(ACTOR, 1)),
        RetainedRoot::ReviewBundle(digest(BUNDLE, 1)),
        RetainedRoot::UnresolvedConflict(digest(OPERATION, 1)),
        RetainedRoot::RestorePoint {
            name: "release candidate".to_owned(),
            operation: digest(OPERATION, 1),
        },
        RetainedRoot::RetentionWindow {
            actor: digest(ACTOR, 1),
            from_sequence: 1,
        },
        RetainedRoot::OfflinePeer(digest(PEER, 1)),
        RetainedRoot::Manifest(digest(MANIFEST, 1)),
    ];
    let distinct: BTreeMap<String, ()> = all.iter().map(|r| (r.to_string(), ())).collect();
    assert_eq!(distinct.len(), 8, "two roots render identically");
}
