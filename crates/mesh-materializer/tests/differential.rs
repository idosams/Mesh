//! Acceptance criterion 4: the reference oracle and the real implementation agree on the full
//! generated corpus.
//!
//! # What the two sides share, and what they do not
//!
//! They share the rules and the vocabulary. They share no data structure, no lookup strategy, no
//! ordering algorithm and no encoder — `tests/common/oracle.rs` has the table. So an agreement here
//! is two implementations reaching the same answer, which is the evidence a differential test
//! exists to produce, and a disagreement is a bug in exactly one of them.
//!
//! # Corpus size, and the sampling rate
//!
//! The contract asks for at least 100,000 generated sets. `cargo nextest run --workspace` runs in
//! an unoptimised build on every push, and 100,000 sets through both sides there is minutes rather
//! than seconds, so the default is a **deterministic sample by seed**: seeds `0..N`, a contiguous
//! prefix, printed by the test so the rate is recorded rather than implied. The full corpus is one
//! variable away and was run before this landed:
//!
//! ```text
//! MESH_MATERIALIZER_CORPUS=100000 cargo nextest run --release -p mesh-materializer
//! ```
//!
//! That is exactly the escape the task's failure-and-recovery section permits — *sample
//! deterministically by seed and record the sampling rate; never silently shrink coverage*.

mod common;

use std::collections::BTreeSet;

use common::{corpus, oracle, snapshot};
use mesh_materializer::{materialize, OperationKind, Rejection};

/// How many seeds run by default. Chosen so this test stays under a second in a debug build.
const DEFAULT_SEEDS: u64 = 4_000;

/// The shape of a rejection, so the run can say which arms the corpus reached.
///
/// A `match` rather than a `Debug` prefix, so a new variant is a compile error here.
fn shape_of(rejection: &Rejection) -> &'static str {
    match rejection {
        Rejection::UnknownObject { .. } => "UnknownObject",
        Rejection::ObjectAlreadyExists { .. } => "ObjectAlreadyExists",
        Rejection::NotADirectory { .. } => "NotADirectory",
        Rejection::NotAFile { .. } => "NotAFile",
        Rejection::ObjectDeleted { .. } => "ObjectDeleted",
        Rejection::RootObject { .. } => "RootObject",
        Rejection::NameTaken { .. } => "NameTaken",
        Rejection::EntryNotBound { .. } => "EntryNotBound",
        Rejection::EntryBoundElsewhere { .. } => "EntryBoundElsewhere",
        Rejection::AlreadyLinked { .. } => "AlreadyLinked",
        Rejection::WouldCycle { .. } => "WouldCycle",
        Rejection::UnknownVersion { .. } => "UnknownVersion",
        Rejection::NoKnownVersion { .. } => "NoKnownVersion",
        Rejection::VersionAlreadyRecorded { .. } => "VersionAlreadyRecorded",
        Rejection::VersionObjectMismatch { .. } => "VersionObjectMismatch",
        Rejection::EmptyResolution { .. } => "EmptyResolution",
        Rejection::DuplicateResolution { .. } => "DuplicateResolution",
        Rejection::HeadNotCurrent { .. } => "HeadNotCurrent",
    }
}

#[test]
fn the_oracle_and_the_implementation_agree_over_the_corpus() {
    let seeds = corpus::corpus_size(DEFAULT_SEEDS);
    let mut operations = 0usize;
    let mut rejections = 0usize;
    let mut verbs: BTreeSet<&'static str> = BTreeSet::new();
    let mut shapes: BTreeSet<&'static str> = BTreeSet::new();

    for seed in 0..seeds {
        let generated = corpus::generate(seed);
        operations += generated.operation_count();
        for changeset in &generated.changesets {
            for operation in changeset.operations() {
                verbs.insert(operation.kind().as_str());
            }
        }

        let materialized = materialize(generated.root, &generated.changesets);
        let mine = snapshot::of_materialization(&materialized);
        let theirs = oracle::materialize(generated.root, &generated.changesets);
        rejections += materialized.rejections().len();
        for rejected in materialized.rejections() {
            shapes.insert(shape_of(rejected.rejection()));
        }

        if let Some(difference) = mine.first_difference(&theirs) {
            panic!(
                "seed {seed} diverges. Add it to tests/regression_seeds.rs before fixing \
                 anything.\n{difference}"
            );
        }
    }

    // A corpus that generated almost nothing would agree with anything, so the run says what it
    // covered. These floors are about the generator, not about the implementation.
    assert!(
        operations > seeds as usize * 4,
        "the corpus produced only {operations} operations over {seeds} seeds"
    );
    assert!(
        rejections > seeds as usize / 2,
        "only {rejections} operations were refused over {seeds} seeds, so the refusal arms are \
         barely covered"
    );
    assert_eq!(
        verbs.len(),
        OperationKind::ALL.len(),
        "the corpus never emitted every verb: {verbs:?}"
    );
    assert_eq!(
        shapes.len(),
        18,
        "the corpus reached only {} of the eighteen refusal shapes: {shapes:?}",
        shapes.len()
    );
    eprintln!(
        "corpus: {seeds} seeds, {operations} operations, {rejections} refused, {} refusal shapes \
         reached: {shapes:?}",
        shapes.len()
    );
}

/// The oracle has to be able to *fail*. A differential test whose oracle agrees with everything
/// proves nothing, so this deliberately materializes two different sets and requires the two
/// snapshots to differ — which is only possible if the oracle really reads its input.
#[test]
fn the_oracle_distinguishes_two_different_operation_sets() {
    let one = corpus::generate(1);
    let other = corpus::generate(2);
    assert_ne!(
        oracle::materialize(one.root, &one.changesets),
        oracle::materialize(other.root, &other.changesets)
    );
}

/// And it has to be able to disagree with the implementation. An operation set the implementation
/// never sees cannot be compared, so this compares the oracle against a *hand-computed* answer: two
/// files created, one linked, one refused for the taken name.
#[test]
fn the_oracle_computes_the_hand_worked_example() {
    use mesh_materializer::{
        AppliedChangeSet, ChangeSetId, ManifestId, NormalizedName, ObjectId, Operation,
        PortableMetadata, VersionId,
    };

    let root = ObjectId::from_bytes([0; 16]);
    let first = ObjectId::from_bytes([1; 16]);
    let second = ObjectId::from_bytes([2; 16]);
    let one = VersionId::from_bytes([1; 32]);
    let two = VersionId::from_bytes([2; 32]);
    let manifest = ManifestId::from_bytes([9; 32]);
    let name = NormalizedName::new("notes.md").unwrap();

    let set = [AppliedChangeSet::genesis(
        ChangeSetId::from_bytes([7; 32]),
        vec![
            Operation::CreateFile { object_id: first },
            Operation::CreateFile { object_id: second },
            Operation::WriteFileVersion {
                object_id: first,
                version_id: one,
                parent_versions: vec![],
                manifest_id: manifest,
                portable_metadata: PortableMetadata::default(),
            },
            Operation::WriteFileVersion {
                object_id: second,
                version_id: two,
                parent_versions: vec![],
                manifest_id: manifest,
                portable_metadata: PortableMetadata::default(),
            },
            Operation::LinkDirectoryEntry {
                directory_id: root,
                name: name.clone(),
                object_id: first,
                version_id: one,
            },
            Operation::LinkDirectoryEntry {
                directory_id: root,
                name,
                object_id: second,
                version_id: two,
            },
        ],
    )];

    let theirs = oracle::materialize(root, &set);
    assert_eq!(theirs.objects.len(), 3, "the root and the two files");
    assert_eq!(theirs.entries.len(), 1, "one of the two links was refused");
    assert_eq!(theirs.rejections.len(), 1);
    assert!(theirs.rejections[0].contains("already binds"));
    assert_eq!(theirs.applied, 5);

    let mine = snapshot::of_materialization(&materialize(root, &set));
    assert_eq!(mine.first_difference(&theirs), None);
}
