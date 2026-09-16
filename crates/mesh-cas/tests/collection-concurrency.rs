//! "Collection never blocks a local write", made structural rather than timed.
//!
//! `cargo nextest run -p mesh-cas --test collection-concurrency`
//!
//! # Why this is not a latency assertion
//!
//! The obvious reading of the criterion is "measure write latency with the collector idle and with
//! it running, and gate the ratio". That would put a third wall-clock assertion on the merge path,
//! which `01KZD51YC12BP9AYVTX557RGAS` recorded as a thing this repository has decided not to do:
//! a timing gate on a shared machine is a flake generator, and a flaky gate is a gate that gets
//! switched off.
//!
//! The stronger statement is available anyway, because "does not block" is a *structural* property
//! and can be proved rather than sampled. A collection is **held open in the middle of a deletion**
//! by a filesystem that refuses to return from `remove_file` until a writer says so. If collection
//! took any exclusive hold on the store, the writer could not finish, and it does. The failure mode
//! is a timeout inside the writer's own wait, reported as a failed assertion rather than a hang.
//!
//! A number is reported too, because plan §2.10 asks for one — but it is printed with
//! `--nocapture`, never asserted.

mod support;

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use mesh_cas::{Blake3, Cas, CollectionMode, Digest32, DurableFs, StdFs};

use support::{Bytes, TempRoot};

/// How long a thread waits for the other before calling it a deadlock.
const PATIENCE: Duration = Duration::from_secs(20);

/// The real filesystem, with a gate across `remove_file`.
///
/// The gate closes for the *first* unlink only: the point is to hold one collection open across a
/// whole promotion, not to serialise the run.
#[derive(Debug)]
struct GatedFs {
    inner: StdFs,
    /// `true` once the writer has released the collector.
    released: Mutex<bool>,
    signal: Condvar,
    unlinked: AtomicUsize,
    /// Set when a wait timed out, so the failure surfaces as an assertion.
    timed_out: AtomicUsize,
}

impl GatedFs {
    fn new() -> Self {
        Self {
            inner: StdFs,
            released: Mutex::new(false),
            signal: Condvar::new(),
            unlinked: AtomicUsize::new(0),
            timed_out: AtomicUsize::new(0),
        }
    }

    /// Let the collector past its gate.
    fn release(&self) {
        let mut released = self
            .released
            .lock()
            .expect("the gate mutex is not poisoned");
        *released = true;
        self.signal.notify_all();
    }

    /// How many chunks have been unlinked so far.
    fn unlinked(&self) -> usize {
        self.unlinked.load(Ordering::SeqCst)
    }

    fn timed_out(&self) -> bool {
        self.timed_out.load(Ordering::SeqCst) > 0
    }

    fn wait_for_release(&self) {
        let released = self
            .released
            .lock()
            .expect("the gate mutex is not poisoned");
        let (guard, outcome) = self
            .signal
            .wait_timeout_while(released, PATIENCE, |released| !*released)
            .expect("the gate mutex is not poisoned");
        drop(guard);
        if outcome.timed_out() {
            self.timed_out.fetch_add(1, Ordering::SeqCst);
        }
    }
}

impl DurableFs for GatedFs {
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.inner.create_dir_all(path)
    }

    fn stage(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.inner.stage(path, bytes)
    }

    fn append(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.inner.append(path, bytes)
    }

    fn sync_file(&self, path: &Path) -> io::Result<()> {
        self.inner.sync_file(path)
    }

    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        self.inner.sync_dir(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.inner.rename(from, to)
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.inner.read(path)
    }

    fn exists(&self, path: &Path) -> bool {
        self.inner.exists(path)
    }

    fn file_len(&self, path: &Path) -> io::Result<u64> {
        self.inner.file_len(path)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        if self.unlinked.fetch_add(1, Ordering::SeqCst) == 0 && is_chunk(path) {
            self.wait_for_release();
        }
        self.inner.remove_file(path)
    }

    fn list_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        self.inner.list_dir(path)
    }
}

/// Whether this path is a chunk rather than a staging file or a journal.
fn is_chunk(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| Digest32::parse_hex(name).is_ok())
}

/// A write completes while a collection is suspended mid-deletion.
#[test]
fn a_write_completes_while_a_collection_is_held_open_mid_deletion() {
    let root = TempRoot::new("collection-concurrency");
    let store: Cas<GatedFs, Blake3> =
        Cas::with_filesystem(root.path(), GatedFs::new()).expect("the store opens");

    let mut generator = Bytes::seeded(0xC0FFEE);
    let mut doomed = Vec::new();
    for _ in 0..32 {
        doomed.push(
            store
                .promote(generator.take(1024))
                .expect("promotion succeeds")
                .digest(),
        );
    }
    let referenced: BTreeSet<Digest32> = BTreeSet::new();

    let written = std::thread::scope(|scope| {
        let collector = scope.spawn(|| {
            store
                .collect(
                    &doomed,
                    &|digest: &Digest32| referenced.contains(digest),
                    CollectionMode::Delete,
                )
                .expect("collection succeeds")
        });

        // The writer runs while the collector is parked inside its first `remove_file`.
        let writer = scope.spawn(|| {
            let promoted = store
                .promote(b"a write that must not wait for the collector".to_vec())
                .expect("the promotion succeeds while a collection is in flight");
            // Observed *before* releasing the collector: the collection was demonstrably still in
            // progress when this promotion returned.
            let unlinked_at_completion = store.filesystem().unlinked();
            store.filesystem().release();
            (promoted.digest(), unlinked_at_completion)
        });

        let (digest, unlinked_at_completion) = writer.join().expect("the writer thread finished");
        let report = collector.join().expect("the collector thread finished");
        assert!(
            !store.filesystem().timed_out(),
            "the collector waited {PATIENCE:?} for a writer that never finished, which is the \
             signature of collection blocking a write"
        );
        assert!(
            unlinked_at_completion < doomed.len(),
            "the promotion only returned after the collection had finished all {} deletions, so \
             nothing here proves it was not blocked",
            doomed.len()
        );
        assert_eq!(report.collected().len(), doomed.len());
        digest
    });

    // The chunk written during the collection survived it and still verifies.
    assert!(
        store.contains(&written),
        "the chunk promoted during the collection was removed by it"
    );
    assert_eq!(
        store.read(&written).expect("the new chunk reads"),
        b"a write that must not wait for the collector"
    );
    for digest in &doomed {
        assert!(!store.contains(digest), "a doomed chunk survived");
    }
}

/// A chunk promoted *after* the doomed list was computed is not in that list and is never at risk,
/// which is the property that makes the previous test's race benign rather than lucky.
#[test]
fn a_chunk_promoted_after_the_plan_was_computed_is_not_in_the_plan() {
    let root = TempRoot::new("collection-concurrency-plan");
    let store = Cas::open(root.path()).expect("the store opens");
    let doomed = vec![store
        .promote(b"garbage".to_vec())
        .expect("promotion succeeds")
        .digest()];

    let later = store
        .promote(b"written after the plan".to_vec())
        .expect("promotion succeeds")
        .digest();
    assert!(!doomed.contains(&later));

    store
        .collect(&doomed, &|_: &Digest32| false, CollectionMode::Delete)
        .expect("collection succeeds");
    assert!(store.contains(&later));
}

/// The reported number plan §2.10 asks for: promotion latency with the collector idle and with it
/// deleting flat out, in the same process. Printed under `--nocapture`, asserted nowhere — a timing
/// gate on a shared machine is a flake, and this repository has decided against a third one.
#[test]
fn promotion_latency_beside_a_running_collection_is_measured_and_reported() {
    let root = TempRoot::new("collection-concurrency-latency");
    let store = Cas::open(root.path()).expect("the store opens");
    let mut generator = Bytes::seeded(0xBEEF);

    let mut doomed = Vec::new();
    for _ in 0..128 {
        doomed.push(
            store
                .promote(generator.take(4096))
                .expect("promotion succeeds")
                .digest(),
        );
    }

    let quiet = median_promotion_micros(&store, &mut generator);

    let busy = std::thread::scope(|scope| {
        let collector = scope.spawn(|| {
            store
                .collect(&doomed, &|_: &Digest32| false, CollectionMode::Delete)
                .expect("collection succeeds")
        });
        let sampled = median_promotion_micros(&store, &mut generator);
        collector.join().expect("the collector finished");
        sampled
    });

    println!(
        "promotion latency: {quiet} us idle, {busy} us beside a collection of {} chunks",
        doomed.len()
    );
    assert!(quiet > 0 && busy > 0, "the samples measured nothing");
}

/// The median of fifteen promotions, in microseconds.
fn median_promotion_micros<F: DurableFs>(store: &Cas<F, Blake3>, generator: &mut Bytes) -> u128 {
    let mut samples = Vec::new();
    for _ in 0..15 {
        let bytes = generator.take(4096);
        let started = Instant::now();
        store.promote(bytes).expect("promotion succeeds");
        samples.push(started.elapsed().as_micros());
    }
    samples.sort_unstable();
    samples[samples.len() / 2]
}
