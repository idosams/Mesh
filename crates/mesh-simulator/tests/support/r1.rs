#![allow(dead_code)]

use std::collections::BTreeSet;
use std::thread;

use mesh_state::{
    ActorId, ChangeSetId, DeliveredChangeSet, HeadAdvancement, HeadDigest, HeadId, Reception,
};
use mesh_types::{Blake3, Blake3Hasher, ContentDigest, DigestHasher};

pub(crate) const EXHAUSTIVE_HEAD: &str =
    "02336fe97b8329c4a6d4bb167f44db3b354c8f9abc2e5eeb07b137b7d935dd20";
pub(crate) const DIVERGENCE_CHILD_HEAD: &str =
    "31d959a782f300b16b726b1280182718aaaedfefea17707be1fd2e261c6d6997";
pub(crate) const DIVERGENCE_TWIN_HEAD: &str =
    "a5023003fc4ef127b2a04f091c20e78d068ebbec95d823631f11be8231ccb39f";

pub(crate) struct Blake3Head(Blake3Hasher);

impl HeadDigest for Blake3Head {
    fn start() -> Self {
        Self(Blake3::hasher())
    }

    fn absorb(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }

    fn finish(self) -> HeadId {
        HeadId::from_bytes(*self.0.finalize().as_bytes())
    }
}

pub(crate) type Adv = HeadAdvancement<Blake3Head>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Outcome {
    pub(crate) head: HeadId,
    pub(crate) applied: Vec<ChangeSetId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ExhaustiveCounts {
    pub(crate) permutations: usize,
    pub(crate) duplicate_streams: usize,
}

pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed.wrapping_add(0x9E37_79B9_7F4A_7C15))
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }

    fn shuffle<T>(&mut self, values: &mut [T]) {
        for index in (1..values.len()).rev() {
            let other = self.below(index + 1);
            values.swap(index, other);
        }
    }
}

pub(crate) fn cs_id(ordinal: u64) -> ChangeSetId {
    ChangeSetId::from_bytes(*Blake3::digest_bytes(&ordinal.to_be_bytes()).as_bytes())
}

pub(crate) fn history7() -> Vec<DeliveredChangeSet> {
    let mut actor_a = Adv::new(ActorId::from_bytes([1; 32]));
    let mut actor_b = Adv::new(ActorId::from_bytes([2; 32]));
    let mut actor_c = Adv::new(ActorId::from_bytes([3; 32]));
    let mut records = Vec::new();

    let (next_a, root) = actor_a.author(cs_id(0)).expect("author root");
    actor_a = next_a;
    records.push(root.clone());
    actor_b = deliver_honestly(&actor_b, &root, "history7 root to B");
    actor_c = deliver_honestly(&actor_c, &root, "history7 root to C");

    let (next_a, fork_a) = actor_a.author(cs_id(1)).expect("author fork A");
    actor_a = next_a;
    records.push(fork_a.clone());
    let (next_b, fork_b) = actor_b.author(cs_id(2)).expect("author fork B");
    actor_b = next_b;
    records.push(fork_b.clone());
    let (next_c, fork_c) = actor_c.author(cs_id(3)).expect("author fork C");
    actor_c = next_c;
    records.push(fork_c.clone());

    let (_next_a, extension_a) = actor_a.author(cs_id(4)).expect("author extension A");
    records.push(extension_a.clone());
    actor_b = deliver_honestly(&actor_b, &fork_a, "history7 fork A to B");
    actor_b = deliver_honestly(&actor_b, &fork_c, "history7 fork C to B");
    let (_next_b, merge_b) = actor_b.author(cs_id(5)).expect("author merge B");
    records.push(merge_b.clone());

    actor_c = deliver_honestly(&actor_c, &fork_b, "history7 fork B to C");
    actor_c = deliver_honestly(&actor_c, &extension_a, "history7 extension A to C");
    actor_c = deliver_honestly(&actor_c, &merge_b, "history7 merge B to C");
    let (_next_c, merge_c) = actor_c.author(cs_id(6)).expect("author merge C");
    records.push(merge_c);
    records
}

pub(crate) fn history(
    seed: u64,
    count: usize,
    actor_count: usize,
) -> Result<Vec<DeliveredChangeSet>, String> {
    if actor_count == 0 {
        return Err("actor count must be positive".to_owned());
    }
    if actor_count > 255 {
        return Err("actor count must fit the deterministic actor identifier".to_owned());
    }

    let mut rng = Rng::new(seed);
    let mut actors: Vec<Adv> = (0..actor_count)
        .map(|index| Adv::new(ActorId::from_bytes([index as u8 + 1; 32])))
        .collect();
    let mut authored: Vec<DeliveredChangeSet> = Vec::new();

    for step in 0..count {
        let who = rng.below(actor_count);
        let mut pool: Vec<usize> = (0..authored.len()).collect();
        rng.shuffle(&mut pool);
        let take = rng.below(pool.len() + 1);
        for index in pool.into_iter().take(take) {
            let (next, reception) = actors[who].deliver(authored[index].clone());
            refuse_if_dishonest(&reception, "random history cross-delivery")?;
            actors[who] = next;
        }
        let (next, record) = actors[who]
            .author(cs_id(step as u64))
            .map_err(|refusal| format!("random history author refused step {step}: {refusal}"))?;
        actors[who] = next;
        authored.push(record);
    }
    Ok(authored)
}

pub(crate) fn divergence_heads() -> (HeadId, HeadId) {
    let mut child_bytes = [0_u8; 32];
    child_bytes[0] = 0x01;
    let mut parent_bytes = [0_u8; 32];
    parent_bytes[0] = 0x0a;
    let child = ChangeSetId::from_bytes(child_bytes);
    let parent = ChangeSetId::from_bytes(parent_bytes);
    assert!(
        child < parent,
        "the divergence needs the child to sort before its parent"
    );

    let parent_view = Adv::new(ActorId::from_bytes([1; 32]));
    let (parent_view, parent_record) = parent_view.author(parent).expect("author parent");
    let (child_view, _child_record) = parent_view.author(child).expect("author child");

    let twin_view = Adv::new(ActorId::from_bytes([1; 32]));
    let (_twin_view, twin_record) = twin_view.author(child).expect("author twin");

    let receiver = Adv::new(ActorId::from_bytes([2; 32]));
    let receiver = deliver_honestly(&receiver, &parent_record, "divergence parent");
    let receiver = deliver_honestly(&receiver, &twin_record, "divergence twin");

    assert_eq!(child_view.applied().len(), receiver.applied().len());
    assert!(child_view.known_missing().is_empty());
    assert!(receiver.known_missing().is_empty());
    (child_view.head(), receiver.head())
}

pub(crate) fn exhaustive_history7() -> Result<ExhaustiveCounts, String> {
    let history = history7();
    let reference = deliver_stream(&history, 0xf0)?;
    if reference.head.to_hex() != EXHAUSTIVE_HEAD {
        return Err(format!(
            "ADR head mismatch: expected {EXHAUSTIVE_HEAD}, got {}",
            reference.head
        ));
    }

    let mut permutation = history.clone();
    let mut permutations = Vec::with_capacity(5_040);
    heap_permutations(&mut permutation, &mut |ordered| {
        permutations.push(ordered.to_vec());
    });

    // Each permutation is independent. A fixed worker count makes the merge-path cost
    // predictable without changing the exhaustive corpus or deriving authority from host load.
    let worker_count = 8.min(permutations.len());
    let chunk_size = permutations.len().div_ceil(worker_count);
    let duplicate_streams = thread::scope(|scope| {
        let mut workers = Vec::with_capacity(worker_count);
        for (chunk_index, chunk) in permutations.chunks(chunk_size).enumerate() {
            let history = &history;
            let reference = &reference;
            workers.push(scope.spawn(move || {
                let mut duplicate_streams = 0;
                for (offset, ordered) in chunk.iter().enumerate() {
                    let permutation_number = chunk_index * chunk_size + offset + 1;
                    duplicate_streams +=
                        verify_permutation(history, reference, ordered, permutation_number)?;
                }
                Ok::<usize, String>(duplicate_streams)
            }));
        }

        let mut duplicate_streams = 0;
        for worker in workers {
            duplicate_streams += worker
                .join()
                .expect("R1 permutation worker must not panic")?;
        }
        Ok::<usize, String>(duplicate_streams)
    })?;

    Ok(ExhaustiveCounts {
        permutations: permutations.len(),
        duplicate_streams,
    })
}

fn verify_permutation(
    history: &[DeliveredChangeSet],
    reference: &Outcome,
    ordered: &[DeliveredChangeSet],
    permutation_number: usize,
) -> Result<usize, String> {
    let prefixes = delivery_prefixes(ordered, 0xf1)
        .map_err(|error| format!("permutation {permutation_number}: {error}"))?;
    match outcome(prefixes.last().expect("every prefix list starts empty")) {
        Ok(outcome) if &outcome == reference => {}
        Ok(outcome) => {
            return Err(format!(
                "permutation {permutation_number} diverged: head {}, {} applied records",
                outcome.head,
                outcome.applied.len()
            ));
        }
        Err(error) => return Err(format!("permutation {permutation_number}: {error}")),
    }

    let streams_per_permutation = history.len() * (ordered.len() + 1);
    let mut duplicate_streams = 0;
    for duplicate in history {
        for position in 0..=ordered.len() {
            duplicate_streams += 1;
            verify_duplicate_insertion(&prefixes, ordered, duplicate, position).map_err(
                |error| {
                    let global_stream =
                        (permutation_number - 1) * streams_per_permutation + duplicate_streams;
                    format!("duplicate stream {global_stream} at position {position}: {error}")
                },
            )?;
        }
    }
    Ok(duplicate_streams)
}

pub(crate) fn randomized_case(
    seed: u64,
    count: usize,
    actor_count: usize,
    peer_count: usize,
) -> Result<Outcome, String> {
    if peer_count == 0 {
        return Err("peer count must be positive".to_owned());
    }
    if peer_count > 255 {
        return Err("peer count must fit the deterministic peer identifier".to_owned());
    }
    let records = history(seed, count, actor_count)?;
    let reference = deliver_stream(&records, 0xe0)?;

    for peer_index in 0..peer_count {
        let peer_seed =
            seed.wrapping_add(0xA076_1D64_78BD_642F_u64.wrapping_mul(peer_index as u64 + 1));
        let mut rng = Rng::new(peer_seed);
        let mut stream = Vec::new();
        for record in &records {
            let copies = rng.below(4) + 1;
            stream.extend(std::iter::repeat_n(record.clone(), copies));
        }
        rng.shuffle(&mut stream);
        let boundary = rng.below(stream.len() + 1);
        let actor_byte = u8::try_from(peer_index + 1).map_err(|_| "peer index overflow")?;
        let mut peer = Adv::new(ActorId::from_bytes([actor_byte; 32]));
        let mut delivered = BTreeSet::new();

        if boundary == 0 {
            check_partition(&peer, &delivered)?;
        }
        for (index, record) in stream.iter().enumerate() {
            delivered.insert(record.id());
            let (next, reception) = peer.deliver(record.clone());
            refuse_if_dishonest(&reception, "random campaign delivery")?;
            peer = next;
            if index + 1 == boundary {
                check_partition(&peer, &delivered)?;
            }
        }

        if !peer.known_missing().is_empty() {
            return Err(format!(
                "peer {peer_index} ended with {} known-missing records",
                peer.known_missing().len()
            ));
        }
        let outcome = Outcome {
            head: peer.head(),
            applied: peer.applied(),
        };
        if outcome != reference {
            return Err(format!(
                "peer {peer_index} diverged: expected head {}, got {}",
                reference.head, outcome.head
            ));
        }
    }
    Ok(reference)
}

fn deliver_stream(stream: &[DeliveredChangeSet], actor_byte: u8) -> Result<Outcome, String> {
    let prefixes = delivery_prefixes(stream, actor_byte)?;
    outcome(prefixes.last().expect("every prefix list starts empty"))
}

fn delivery_prefixes(stream: &[DeliveredChangeSet], actor_byte: u8) -> Result<Vec<Adv>, String> {
    let mut peer = Adv::new(ActorId::from_bytes([actor_byte; 32]));
    let mut prefixes = Vec::with_capacity(stream.len() + 1);
    prefixes.push(peer.clone());
    for record in stream {
        let (next, reception) = peer.deliver(record.clone());
        refuse_if_dishonest(&reception, "stream delivery")?;
        peer = next;
        prefixes.push(peer.clone());
    }
    Ok(prefixes)
}

fn verify_duplicate_insertion(
    prefixes: &[Adv],
    ordered: &[DeliveredChangeSet],
    duplicate: &DeliveredChangeSet,
    position: usize,
) -> Result<(), String> {
    let original = ordered
        .iter()
        .position(|record| record.id() == duplicate.id())
        .expect("the duplicate comes from the seven-record history");
    let mut peer = prefixes[position].clone();
    let (next, reception) = peer.deliver(duplicate.clone());
    refuse_if_dishonest(&reception, "duplicate insertion")?;
    peer = next;

    if position > original {
        if !matches!(
            reception,
            Reception::AlreadyApplied | Reception::AlreadyBuffered
        ) {
            return Err(format!(
                "late duplicate changed the prefix at position {position}: {reception:?}"
            ));
        }
        return Ok(());
    }

    let mut original_reception = None;
    for record in &ordered[position..=original] {
        let is_original = record.id() == duplicate.id();
        let (next, reception) = peer.deliver(record.clone());
        refuse_if_dishonest(&reception, "duplicate stream through original")?;
        if is_original {
            original_reception = Some(reception.clone());
        }
        peer = next;
    }
    if !matches!(
        original_reception,
        Some(Reception::AlreadyApplied | Reception::AlreadyBuffered)
    ) {
        return Err(format!(
            "the original was not idempotent after its early duplicate: {original_reception:?}"
        ));
    }

    // Both streams now have the same observable causal knowledge and receive an identical suffix.
    // HeadAdvancement is an immutable fold over exactly these sets, so equality at this join point
    // proves the rest of the duplicate-insertion stream without replaying the common suffix 282,240
    // times. This is still one real-fold execution per enumerated stream, not a model of the fold.
    let actual = snapshot(&peer);
    let baseline = snapshot(&prefixes[original + 1]);
    if actual != baseline {
        return Err(format!(
            "state differed from the unduplicated prefix at join {}: actual {actual:?}, baseline {baseline:?}",
            original + 1
        ));
    }
    Ok(())
}

fn outcome(peer: &Adv) -> Result<Outcome, String> {
    if !peer.known_missing().is_empty() {
        return Err(format!(
            "stream ended with {} known-missing records",
            peer.known_missing().len()
        ));
    }
    Ok(Outcome {
        head: peer.head(),
        applied: peer.applied(),
    })
}

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    head: HeadId,
    applied: Vec<ChangeSetId>,
    waiting: Vec<ChangeSetId>,
}

fn snapshot(peer: &Adv) -> Snapshot {
    Snapshot {
        head: peer.head(),
        applied: peer.applied(),
        waiting: peer
            .known_missing()
            .into_iter()
            .map(|missing| missing.waiting())
            .collect(),
    }
}

fn deliver_honestly(peer: &Adv, record: &DeliveredChangeSet, context: &str) -> Adv {
    let (next, reception) = peer.deliver(record.clone());
    if let Reception::Refused(refusal) = reception {
        panic!("{context}: honest record refused: {refusal}");
    }
    next
}

fn refuse_if_dishonest(reception: &Reception, context: &str) -> Result<(), String> {
    match reception {
        Reception::Refused(refusal) => Err(format!("{context}: record refused: {refusal}")),
        Reception::Applied { refused, .. } if !refused.is_empty() => Err(format!(
            "{context}: delivery unblocked {} refused records",
            refused.len()
        )),
        _ => Ok(()),
    }
}

fn check_partition(peer: &Adv, delivered: &BTreeSet<ChangeSetId>) -> Result<(), String> {
    let waiting: BTreeSet<ChangeSetId> = peer
        .known_missing()
        .iter()
        .map(mesh_state::KnownMissing::waiting)
        .collect();
    for id in delivered {
        if !peer.has_applied(id) && !waiting.contains(id) {
            return Err(format!(
                "partition lost delivered record {id}: neither applied nor known-missing"
            ));
        }
    }
    Ok(())
}

fn heap_permutations<T, F>(values: &mut [T], visit: &mut F)
where
    F: FnMut(&[T]),
{
    if values.is_empty() {
        visit(values);
        return;
    }
    heap_permutations_inner(values, values.len(), visit);
}

fn heap_permutations_inner<T, F>(values: &mut [T], size: usize, visit: &mut F)
where
    F: FnMut(&[T]),
{
    if size == 1 {
        visit(values);
        return;
    }
    heap_permutations_inner(values, size - 1, visit);
    for index in 0..size - 1 {
        if size % 2 == 0 {
            values.swap(index, size - 1);
        } else {
            values.swap(0, size - 1);
        }
        heap_permutations_inner(values, size - 1, visit);
    }
}
