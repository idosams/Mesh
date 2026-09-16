//! The generated corpus: operation sets built to be hostile, named by a seed.
//!
//! # The pools are small on purpose
//!
//! Eight object identifiers, five names, three manifests, two actors, four heads. A generator that
//! drew identifiers from the whole space would produce sets in which almost every operation names
//! something that does not exist, and would exercise one rejection arm a hundred thousand times
//! while never once binding a name that is already taken. Small pools make collisions the common
//! case: name contests, re-created objects, a directory moved under its own child, a version
//! written twice with different content.
//!
//! # Both halves of "valid operation set" are generated
//!
//! Plenty of what comes out is well-formed and materializes cleanly. Plenty is not: a ChangeSet
//! naming a causal parent nobody delivered, a ChangeSet naming itself, the same identifier twice.
//! Totality is a claim about *every* set a peer can hand this crate, not only about the ones a
//! well-behaved peer sends, so the generator is allowed to send the others.

use mesh_materializer::{
    ActorId, AppliedChangeSet, ApprovalId, AttributionConfidence, ChangeSetId, ContentHash,
    DerivationId, DerivationKind, HeadId, ManifestId, NormalizedName, ObjectId, Operation,
    PortableMetadata, PreservedEntry, ReadRegion, ReviewBundleId, ValidationOutcome, VersionId,
};

use super::rng::Rng;

/// One generated operation set and the workspace root it is materialized against.
pub struct GeneratedSet {
    /// The workspace root object.
    pub root: ObjectId,
    /// The ChangeSets, in the order the generator produced them.
    pub changesets: Vec<AppliedChangeSet>,
    /// The seed, so a failure can be replayed.
    pub seed: u64,
}

impl GeneratedSet {
    /// How many operations it holds.
    pub fn operation_count(&self) -> usize {
        self.changesets
            .iter()
            .map(|changeset| changeset.operations().len())
            .sum()
    }
}

/// The workspace root every generated set is materialized against.
pub fn root() -> ObjectId {
    ObjectId::from_bytes([0; 16])
}

fn object(index: u64) -> ObjectId {
    ObjectId::from_bytes([(index & 0xff) as u8; 16])
}

fn version(index: u64) -> VersionId {
    VersionId::from_bytes([(index & 0xff) as u8; 32])
}

fn name(index: u64) -> NormalizedName {
    const NAMES: [&str; 5] = ["a", "b", "notes.md", "src", "notes (2).md"];
    NormalizedName::new(NAMES[(index % NAMES.len() as u64) as usize]).expect("a legal name")
}

/// What the generator remembers, so that most of what it emits is coherent.
///
/// This is **not** a materializer and does not try to be one: it records what it has asked for, not
/// what the state accepted. An object it "created" may have been refused; a version it "wrote" may
/// name a deleted object. So the sets it produces are mostly well-formed with a steady minority
/// that is not, which is the mix the corpus needs — a generator with no memory refuses four
/// operations in five and never builds a tree deep enough to move a subtree through.
pub struct Shadow {
    files: Vec<ObjectId>,
    directories: Vec<ObjectId>,
    unminted: Vec<ObjectId>,
    versions: Vec<(ObjectId, VersionId)>,
    next_version: u64,
}

impl Shadow {
    fn new() -> Self {
        Self {
            files: Vec::new(),
            directories: vec![root()],
            unminted: (1..=8).map(object).collect(),
            versions: Vec::new(),
            next_version: 1,
        }
    }

    fn take_unminted(&mut self, rng: &mut Rng) -> Option<ObjectId> {
        if self.unminted.is_empty() {
            return None;
        }
        let at = rng.below(self.unminted.len() as u64) as usize;
        Some(self.unminted.remove(at))
    }

    fn any_object(&self, rng: &mut Rng) -> ObjectId {
        let known: Vec<ObjectId> = self
            .files
            .iter()
            .chain(self.directories.iter())
            .copied()
            .collect();
        match rng.below(24) {
            0 => root(),
            1 => object(200),
            _ => rng.pick(&known).copied().unwrap_or_else(|| object(1)),
        }
    }

    fn any_file(&self, rng: &mut Rng) -> ObjectId {
        if rng.chance(12) || self.files.is_empty() {
            self.any_object(rng)
        } else {
            rng.pick(&self.files).copied().unwrap_or_else(|| object(1))
        }
    }

    fn any_directory(&self, rng: &mut Rng) -> ObjectId {
        if rng.chance(10) {
            self.any_object(rng)
        } else if rng.chance(40) {
            root()
        } else {
            rng.pick(&self.directories).copied().unwrap_or_else(root)
        }
    }

    fn version_of(&self, rng: &mut Rng, object: ObjectId) -> VersionId {
        let mine: Vec<VersionId> = self
            .versions
            .iter()
            .filter(|(held, _)| *held == object)
            .map(|(_, version)| *version)
            .collect();
        if rng.chance(12) || mine.is_empty() {
            version(250)
        } else {
            rng.pick(&mine).copied().unwrap_or_else(|| version(250))
        }
    }

    fn fresh_version(&mut self) -> VersionId {
        let id = version(self.next_version % 200);
        self.next_version += 1;
        id
    }
}

fn some_version(rng: &mut Rng) -> VersionId {
    version(1 + rng.below(20))
}

fn some_versions(rng: &mut Rng, shadow: &Shadow, object: ObjectId) -> Vec<VersionId> {
    let count = rng.below(3);
    (0..count).map(|_| shadow.version_of(rng, object)).collect()
}

fn some_name(rng: &mut Rng) -> NormalizedName {
    name(rng.below(5))
}

fn some_head(rng: &mut Rng) -> HeadId {
    HeadId::from_bytes([(rng.below(4)) as u8; 32])
}

fn some_actor(rng: &mut Rng) -> ActorId {
    ActorId::from_bytes([(1 + rng.below(2)) as u8; 32])
}

/// One operation, drawn from the whole vocabulary with the weights below.
///
/// The weights bias towards the verbs that build state — a corpus of nothing but deletions would
/// spend its whole run on an empty workspace — while keeping every one of the eighteen reachable.
fn some_operation(rng: &mut Rng, shadow: &mut Shadow) -> Operation {
    match rng.below(100) {
        0..=9 => {
            let object_id = shadow
                .take_unminted(rng)
                .unwrap_or_else(|| shadow.any_object(rng));
            shadow.files.push(object_id);
            Operation::CreateFile { object_id }
        }
        10..=17 => {
            let object_id = shadow
                .take_unminted(rng)
                .unwrap_or_else(|| shadow.any_object(rng));
            shadow.directories.push(object_id);
            Operation::CreateDirectory { object_id }
        }
        18..=35 => {
            let object_id = shadow.any_file(rng);
            let parent_versions = some_versions(rng, shadow, object_id);
            let version_id = if rng.chance(15) {
                shadow.version_of(rng, object_id)
            } else {
                shadow.fresh_version()
            };
            shadow.versions.push((object_id, version_id));
            Operation::WriteFileVersion {
                object_id,
                version_id,
                parent_versions,
                manifest_id: ManifestId::from_bytes([(1 + rng.below(3)) as u8; 32]),
                portable_metadata: PortableMetadata::new(rng.chance(30)),
            }
        }
        36..=53 => {
            let object_id = shadow.any_object(rng);
            Operation::LinkDirectoryEntry {
                directory_id: shadow.any_directory(rng),
                name: some_name(rng),
                object_id,
                version_id: shadow.version_of(rng, object_id),
            }
        }
        54..=58 => Operation::UnlinkDirectoryEntry {
            directory_id: shadow.any_directory(rng),
            name: some_name(rng),
            object_id: shadow.any_object(rng),
        },
        59..=64 => Operation::RenameEntry {
            directory_id: shadow.any_directory(rng),
            from_name: some_name(rng),
            to_name: some_name(rng),
            object_id: shadow.any_object(rng),
        },
        65..=73 => Operation::MoveEntry {
            from_directory_id: shadow.any_directory(rng),
            from_name: some_name(rng),
            to_directory_id: shadow.any_directory(rng),
            to_name: some_name(rng),
            object_id: shadow.any_object(rng),
        },
        74..=76 => Operation::DeleteObject {
            object_id: shadow.any_object(rng),
        },
        77..=79 => {
            let object_id = shadow.any_object(rng);
            Operation::RestoreObject {
                object_id,
                restored_version_id: shadow.version_of(rng, object_id),
            }
        }
        80..=83 => {
            let object_id = shadow.any_file(rng);
            Operation::SetPortableMetadata {
                object_id,
                version_id: shadow.version_of(rng, object_id),
                portable_metadata: PortableMetadata::new(rng.chance(50)),
            }
        }
        84..=87 => {
            // Zero is reachable: a resolution that preserves nothing discards a contender, and the
            // refusal for it is a rule the corpus has to reach rather than assume.
            let count = rng.below(4);
            Operation::ResolveNameConflict {
                directory_id: shadow.any_directory(rng),
                contested_name: some_name(rng),
                preserved: (0..count)
                    .map(|_| PreservedEntry::new(shadow.any_object(rng), some_name(rng)))
                    .collect(),
            }
        }
        88..=90 => {
            let object_id = shadow.any_file(rng);
            Operation::ResolveContentConflict {
                object_id,
                resulting_version_id: shadow.version_of(rng, object_id),
                preserved_version_ids: some_versions(rng, shadow, object_id),
            }
        }
        91..=93 => Operation::AdvanceActorHead {
            actor_id: some_actor(rng),
            from_head: some_head(rng),
            to_head: some_head(rng),
        },
        94..=95 => {
            let object_id = shadow.any_object(rng);
            Operation::RecordReadObservation {
                actor_id: some_actor(rng),
                object_id,
                version_id: shadow.version_of(rng, object_id),
                region: ReadRegion::ALL[rng.below(7) as usize],
                confidence: AttributionConfidence::ALL[rng.below(6) as usize],
            }
        }
        96 => Operation::RecordDerivedNode {
            node_id: DerivationId::from_bytes([rng.byte(); 32]),
            node_kind: DerivationKind::ALL[rng.below(8) as usize],
            exact_inputs: vec![some_version(rng)],
            configuration_digest: ContentHash::from_bytes([rng.byte(); 32]),
            output_versions: vec![some_version(rng)],
            deterministic: rng.chance(50),
        },
        97 => Operation::CreateReviewBundle {
            bundle_id: ReviewBundleId::from_bytes([rng.byte(); 32]),
            actor_head: some_head(rng),
            base_head: some_head(rng),
        },
        98 => Operation::RecordValidation {
            subject_head: some_head(rng),
            validator_id: some_actor(rng),
            outcome: ValidationOutcome::ALL[rng.below(4) as usize],
            evidence: ContentHash::from_bytes([rng.byte(); 32]),
        },
        _ => Operation::AdvanceCanonicalHead {
            from_head: some_head(rng),
            to_head: some_head(rng),
            approval_id: ApprovalId::from_bytes([(1 + rng.below(3)) as u8; 32]),
        },
    }
}

/// The operation set for `seed`. The same seed always produces the same set, on every machine.
pub fn generate(seed: u64) -> GeneratedSet {
    let mut rng = Rng::new(seed);
    let mut shadow = Shadow::new();
    let count = 1 + rng.below(5);
    let mut changesets: Vec<AppliedChangeSet> = Vec::new();
    let mut minted: Vec<ChangeSetId> = Vec::new();

    for index in 0..count {
        let id = ChangeSetId::from_bytes([rng.byte(); 32]);
        let mut parents: Vec<ChangeSetId> =
            minted.iter().copied().filter(|_| rng.chance(45)).collect();
        // A causal parent nobody delivered, and — rarely — a ChangeSet that follows itself. Both
        // are malformed, and both are things a peer can send.
        if rng.chance(6) {
            parents.push(ChangeSetId::from_bytes([0xfe; 32]));
        }
        if rng.chance(3) {
            parents.push(id);
        }

        let operations = (0..2 + rng.below(11))
            .map(|_| some_operation(&mut rng, &mut shadow))
            .collect();
        changesets.push(AppliedChangeSet::new(id, parents, operations));
        minted.push(id);

        // Occasionally deliver the same identifier twice, with different operations.
        if rng.chance(4) && index + 1 < count {
            let repeat = (0..1 + rng.below(3))
                .map(|_| some_operation(&mut rng, &mut shadow))
                .collect();
            changesets.push(AppliedChangeSet::new(id, Vec::new(), repeat));
        }
    }

    GeneratedSet {
        root: root(),
        changesets,
        seed,
    }
}

/// How many operations a set really contributes: one record per identifier, chosen the way
/// `src/order.rs` chooses it, so a set that repeats an identifier is not counted twice.
pub fn deduplicated_operation_count(set: &GeneratedSet) -> usize {
    let mut ids: Vec<ChangeSetId> = set.changesets.iter().map(AppliedChangeSet::id).collect();
    ids.sort_unstable();
    ids.dedup();
    ids.iter()
        .filter_map(|id| {
            set.changesets
                .iter()
                .filter(|held| held.id() == *id)
                .min_by(|left, right| {
                    left.causal_parents()
                        .cmp(right.causal_parents())
                        .then_with(|| {
                            mesh_operations::encode_operations(left.operations())
                                .cmp(&mesh_operations::encode_operations(right.operations()))
                        })
                })
        })
        .map(|held| held.operations().len())
        .sum()
}

/// The same set with its ChangeSets in a seeded shuffle — a different *delivery* order over the
/// same *causal set*, which is what SG-1 requires to be indistinguishable.
pub fn shuffled(set: &GeneratedSet, seed: u64) -> Vec<AppliedChangeSet> {
    let mut shuffled = set.changesets.clone();
    Rng::new(seed).shuffle(&mut shuffled);
    shuffled
}

/// How many seeds a corpus test runs, and the sampling rate that number stands for.
///
/// The contract asks for at least 100,000 generated sets. Running 100,000 through both the
/// implementation and the oracle in an unoptimised test build is minutes, and `npm test` runs the
/// whole workspace suite on every push, so the default is a **deterministic sample by seed** —
/// seeds `0..sample`, which is a contiguous prefix rather than a random subset, so what ran is
/// exactly reproducible. The full corpus is one environment variable away and is run before a push:
///
/// ```text
/// MESH_MATERIALIZER_CORPUS=100000 cargo nextest run --release -p mesh-materializer
/// ```
///
/// The task's failure-and-recovery rule permits sampling *and requires the rate to be recorded*,
/// which is what [`corpus_size`] prints when it samples.
pub fn corpus_size(default: u64) -> u64 {
    match std::env::var("MESH_MATERIALIZER_CORPUS") {
        Ok(value) => value.trim().parse::<u64>().unwrap_or_else(|error| {
            panic!("MESH_MATERIALIZER_CORPUS={value:?} is not a number of seeds ({error})")
        }),
        Err(_) => {
            eprintln!(
                "corpus: {default} seeds of the 100000 the contract names ({:.1}% sample, seeds \
                 0..{default}); set MESH_MATERIALIZER_CORPUS=100000 for the full corpus",
                default as f64 / 1000.0
            );
            default
        }
    }
}
