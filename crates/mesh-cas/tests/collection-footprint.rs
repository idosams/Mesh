//! What collection actually frees, measured on disk before and after.
//!
//! `cargo nextest run -p mesh-cas --test collection-footprint`
//!
//! # Why this file is separate from `collection.rs`
//!
//! `collection.rs` proves the collector does the right thing. This file proves it does *anything*.
//! The acceptance criteria of task `01KZC2KZ29Y8Z6H9W93M40NPJ0` — "a dry run reports exactly what
//! would be deleted", "nothing reachable is collected" — are all satisfied by a collector that
//! deletes nothing and reports the empty set exactly. So the measurement here is the load-bearing
//! one: a store is built, filled, collected, and the bytes on disk are counted from the filesystem
//! both times.
//!
//! Every number below is a **count of bytes**. Nothing here reads a clock, nothing is timed, and
//! the same corpus produces the same figure on every machine — the same discipline
//! `storage-footprint.rs` states at length, for the same reason: `01KZD51YC12BP9AYVTX557RGAS`
//! recorded that the merge path already carries two wall-clock assertions and a third would make it
//! more fragile rather than better measured.
//!
//! The constants here deliberately do **not** use the `BUDGET_`, `GATE_CORPUS_` or
//! `ACTOR_SCALING_` prefixes: those are the register `tools/program/storage-budget/check.mjs`
//! holds against `benchmarks/budgets/storage.md`, and that page is outside this task's allowed
//! paths. These are the parameters of one measurement, not a budget the programme has adopted.

mod support;

use std::collections::BTreeSet;

use mesh_cas::{Cas, CollectionMode, Digest32};

use support::{Bytes, TempRoot};

/// How many chunks the measured store holds.
const CHUNKS: usize = 256;

/// How many bytes each chunk holds. Fixed, so the arithmetic below is checkable by hand.
const CHUNK_BYTES: usize = 4096;

/// Every fourth chunk is referenced; the rest is garbage. Chosen so the freed fraction is a
/// three-quarters that a reader can verify without running anything.
const KEEP_EVERY: usize = 4;

/// The store's chunk bytes, counted by walking the store rather than by trusting the collector.
fn on_disk(store: &Cas) -> u64 {
    store
        .chunk_bytes_on_disk()
        .expect("the store can be measured")
}

/// A store of [`CHUNKS`] distinct chunks, and the digests of the ones nothing references.
fn filled_store(root: &TempRoot) -> (Cas, Vec<Digest32>, Vec<Digest32>) {
    let store = Cas::open(root.path()).expect("the store opens");
    let mut generator = Bytes::seeded(0x0C011EC7);
    let mut kept = Vec::new();
    let mut doomed = Vec::new();
    for index in 0..CHUNKS {
        let digest = store
            .promote(generator.take(CHUNK_BYTES))
            .expect("promotion succeeds")
            .digest();
        if index % KEEP_EVERY == 0 {
            kept.push(digest);
        } else {
            doomed.push(digest);
        }
    }
    (store, kept, doomed)
}

/// The headline. Disk before, disk after, and the two agreeing with the collector's own arithmetic.
#[test]
fn collecting_unreferenced_chunks_frees_the_bytes_they_occupied() {
    let root = TempRoot::new("collection-footprint");
    let (store, kept, doomed) = filled_store(&root);

    let before = on_disk(&store);
    assert_eq!(
        before,
        (CHUNKS * CHUNK_BYTES) as u64,
        "the store should hold exactly the content promoted into it"
    );

    let referenced: BTreeSet<Digest32> = kept.iter().copied().collect();
    let report = store
        .collect(
            &doomed,
            &|digest: &Digest32| referenced.contains(digest),
            CollectionMode::Delete,
        )
        .expect("collection succeeds");

    let after = on_disk(&store);
    assert_eq!(
        report.collected().len(),
        doomed.len(),
        "every doomed chunk was collected"
    );
    assert_eq!(
        report.bytes(),
        (doomed.len() * CHUNK_BYTES) as u64,
        "the collector's own count of freed bytes"
    );
    assert_eq!(
        before - after,
        report.bytes(),
        "the disk measurement and the collector disagree about what was freed"
    );
    assert_eq!(
        after,
        (kept.len() * CHUNK_BYTES) as u64,
        "exactly the referenced chunks survive"
    );

    // Stated as a proportion because that is the form the concern was raised in: disk grows without
    // bound by construction, and this is the first thing in the repository that makes it fall.
    let freed_permille = (before - after) * 1000 / before;
    assert_eq!(
        freed_permille, 750,
        "expected three quarters of the store to be freed, got {freed_permille} per mille"
    );

    // And the survivors still verify: a collection that corrupted what it kept would be worse than
    // one that freed nothing.
    for digest in &kept {
        assert_eq!(
            store.read(digest).expect("a kept chunk still reads").len(),
            CHUNK_BYTES
        );
    }
}

/// A dry run over the same store frees nothing and predicts the same figure, so the number a
/// caller is shown before deciding is the number they get.
#[test]
fn a_dry_run_predicts_the_freed_bytes_without_changing_the_store() {
    let root = TempRoot::new("collection-footprint-dry");
    let (store, kept, doomed) = filled_store(&root);

    let before = on_disk(&store);
    let referenced: BTreeSet<Digest32> = kept.iter().copied().collect();
    let oracle = |digest: &Digest32| referenced.contains(digest);

    let dry = store
        .collect(&doomed, &oracle, CollectionMode::DryRun)
        .expect("a dry run succeeds");
    assert_eq!(
        on_disk(&store),
        before,
        "a dry run changed the bytes on disk"
    );

    let wet = store
        .collect(&doomed, &oracle, CollectionMode::Delete)
        .expect("the delete run succeeds");
    assert_eq!(
        dry.bytes(),
        wet.bytes(),
        "the dry run predicted {} bytes and the real run freed {}",
        dry.bytes(),
        wet.bytes()
    );
    assert_eq!(before - on_disk(&store), dry.bytes());
}

/// The trap, stated as a test: a collector that never deletes a byte satisfies every safety
/// property in this repository. This is the assertion it fails.
#[test]
fn the_collector_is_not_satisfiable_by_freeing_nothing() {
    let root = TempRoot::new("collection-footprint-nonvacuous");
    let (store, kept, doomed) = filled_store(&root);
    let referenced: BTreeSet<Digest32> = kept.iter().copied().collect();

    let report = store
        .collect(
            &doomed,
            &|digest: &Digest32| referenced.contains(digest),
            CollectionMode::Delete,
        )
        .expect("collection succeeds");

    assert!(!report.freed_nothing());
    assert!(
        report.bytes() >= 512 * 1024,
        "the collection freed only {} bytes, which is not a measurement of anything",
        report.bytes()
    );
}

/// Collection also shrinks the arrival journal, which is the other term that grows per chunk.
#[test]
fn collection_shrinks_the_candidate_journal_by_the_chunks_it_removed() {
    let root = TempRoot::new("collection-footprint-journal");
    let (store, kept, doomed) = filled_store(&root);
    assert_eq!(
        store
            .journal()
            .candidates()
            .expect("the journal reads")
            .len(),
        CHUNKS
    );

    let referenced: BTreeSet<Digest32> = kept.iter().copied().collect();
    store
        .collect(
            &doomed,
            &|digest: &Digest32| referenced.contains(digest),
            CollectionMode::Delete,
        )
        .expect("collection succeeds");

    assert_eq!(
        store
            .journal()
            .candidates()
            .expect("the journal reads")
            .len(),
        kept.len(),
        "the journal still offers chunks that are gone"
    );
}
