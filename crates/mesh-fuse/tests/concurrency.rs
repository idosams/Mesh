//! Concurrent actors, concurrent opens, and how long a mount takes.
//!
//! # The three claims here, and what each one is worth
//!
//! 1. **Two actors own two independent states** — epic exit criterion, and the reason
//!    `WorkspaceAdapter`'s every method takes `&self` rather than `&mut self`. The structural
//!    assertion is on distinct roots and isolation. Four-worker throughput is still printed as a
//!    diagnostic, but never used as a correctness oracle: hardware threads are not free cores.
//! 2. **Concurrent opens** — the second of the three failure modes design
//!    `01KZEZGDPMZ5RH7E60WDYDYYEE` named as out of reach for the conformance suite. The suite
//!    takes one handle at a time; nothing in it can see two handles on one object disagreeing.
//! 3. **A mount completes well inside 500 ms** — acceptance criterion 3, measured over 100 mounts
//!    at the worst case rather than the mean. **This times the semantic half only.** There is no
//!    `mount(2)` in this crate and this number does not stand in for one.
//!
//! Verification is **warm** (`docs/adr/0004`).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use mesh_fuse::FuseAdapter;
use mesh_materializer::{
    ActorId, AdapterError, NormalizedName, OpenMode, PortableMetadata, ViewId,
    WorkspaceAdapter as _,
};
use std::path::Path;

/// How many operations one worker performs. Large enough for the timing to mean something,
/// small enough that the whole file stays well under a second.
const OPERATIONS: u64 = 4_000;

fn name(text: &str) -> NormalizedName {
    NormalizedName::new(text).expect("a legal entry name")
}

fn prepared() -> Arc<FuseAdapter> {
    let adapter = Arc::new(FuseAdapter::new());
    adapter.prepare_fixture().expect("the backend prepares");
    adapter
}

fn mount(adapter: &FuseAdapter, actor: u8, at: &str) -> ViewId {
    adapter
        .mount_actor_view(
            adapter.workspace(),
            ActorId::from_bytes([actor; 32]),
            Path::new(at),
        )
        .expect("a mount")
        .id()
}

/// One worker's share of the work: create, write, read back, enumerate, unlink.
///
/// Deliberately a mix rather than only reads: a lock that excluded writers globally would be
/// invisible to a read-only workload, which is exactly the measurement that would look good and
/// mean nothing.
fn burst(adapter: &FuseAdapter, view: ViewId, tag: u64, operations: u64) -> u64 {
    let view = adapter.view(view).expect("the view resolves");
    let mut done = 0;
    for index in 0..operations {
        let entry = name(&format!("worker-{tag}-{index}.txt"));
        let created = view
            .create_file(view.root(), &entry, PortableMetadata::default())
            .expect("a created file");
        let handle = view
            .open(created.object(), OpenMode::ReadWrite)
            .expect("a handle");
        view.write(&handle, 0, b"payload").expect("a write");
        let mut buffer = [0u8; 8];
        view.read(&handle, 0, &mut buffer).expect("a read");
        view.close(handle).expect("the handle closes");
        view.unlink(view.root(), &entry).expect("an unlink");
        done += 1;
    }
    done
}

/// Four actors complete the mixed workload concurrently, with throughput retained as a diagnostic.
///
/// This deliberately makes no speedup assertion. `available_parallelism` reports hardware
/// threads, not spare execution capacity, so a saturated but correct host can produce any ratio.
/// The next test carries the deterministic correctness claim: actors have distinct roots and
/// mutations through one are absent from the other.
#[test]
fn four_actors_complete_the_mixed_workload() {
    let parallelism = thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);

    let adapter = prepared();
    let solo = mount(&adapter, 1, "/solo");
    let started = Instant::now();
    assert_eq!(burst(&adapter, solo, 0, OPERATIONS), OPERATIONS);
    let alone = started.elapsed();

    let adapter = prepared();
    let views: Vec<ViewId> = (0u8..4)
        .map(|worker| mount(&adapter, 10 + worker, &format!("/worker-{worker}")))
        .collect();
    let completed = Arc::new(AtomicU64::new(0));
    let started = Instant::now();
    thread::scope(|scope| {
        for (tag, view) in views.iter().copied().enumerate() {
            let adapter = Arc::clone(&adapter);
            let completed = Arc::clone(&completed);
            scope.spawn(move || {
                let done = burst(&adapter, view, tag as u64, OPERATIONS);
                completed.fetch_add(done, Ordering::SeqCst);
            });
        }
    });
    let together = started.elapsed();

    assert_eq!(completed.load(Ordering::SeqCst), OPERATIONS * 4);
    let alone_per_operation = alone.as_secs_f64() / OPERATIONS as f64;
    let together_per_operation = together.as_secs_f64() / (OPERATIONS * 4) as f64;
    let scaling = alone_per_operation / together_per_operation;
    println!(
        "one actor: {alone:?} for {OPERATIONS} operations; four actors: {together:?} for {} \
         operations; scaling {scaling:.2}x on {parallelism} hardware threads",
        OPERATIONS * 4
    );
}

/// Two actors' states are two objects behind two locks, which is the structural half of the claim
/// above.
///
/// A speed measurement can be dulled by a busy machine; this cannot. Two actors that shared one
/// tree would answer the same root object and mutations through one would appear in the other.
#[test]
fn two_actors_hold_two_states_and_share_no_root() {
    let adapter = prepared();
    let first = adapter.view(mount(&adapter, 41, "/a")).expect("a view");
    let second = adapter.view(mount(&adapter, 42, "/b")).expect("a view");
    assert_ne!(first.root(), second.root());

    let entry = name("mine.txt");
    first
        .create_file(first.root(), &entry, PortableMetadata::default())
        .expect("a created file");
    assert_eq!(
        second.lookup(second.root(), &entry).err(),
        Some(AdapterError::NotFound)
    );
}

/// Failure mode two: concurrent opens.
///
/// Eight threads take a handle on **one** object through one view at the same time and each writes
/// its own region. The suite cannot reach this — it takes one handle, uses it and closes it — and
/// the three things checked here are exactly what a handle table gets wrong under contention:
/// two handles sharing a number, one close taking another handle down with it, and a write through
/// one handle landing at another handle's offset.
#[test]
fn eight_concurrent_opens_of_one_object_are_eight_independent_handles() {
    const WORKERS: u64 = 8;
    const REGION: usize = 4;

    let adapter = prepared();
    let view_id = mount(&adapter, 51, "/opens");
    let view = adapter.view(view_id).expect("the view resolves");
    let shared = view
        .create_file(
            view.root(),
            &name("shared.bin"),
            PortableMetadata::default(),
        )
        .expect("a created file");
    let object = shared.object();

    let handles: Vec<u64> = thread::scope(|scope| {
        let workers: Vec<_> = (0..WORKERS)
            .map(|worker| {
                let adapter = Arc::clone(&adapter);
                scope.spawn(move || {
                    let view = adapter.view(view_id).expect("the view resolves");
                    let handle = view.open(object, OpenMode::ReadWrite).expect("a handle");
                    let payload = [b'a' + u8::try_from(worker).expect("eight workers"); REGION];
                    let offset = worker * REGION as u64;
                    assert_eq!(view.write(&handle, offset, &payload), Ok(REGION));
                    let number = handle.handle();
                    view.close(handle).expect("the handle closes");
                    number
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| worker.join().expect("a worker finished"))
            .collect()
    });

    let mut unique = handles.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        unique.len(),
        handles.len(),
        "two concurrent opens were given one handle number: {handles:?}"
    );

    let reader = view.open(object, OpenMode::Read).expect("a handle");
    let mut buffer = [0u8; (WORKERS as usize) * REGION];
    assert_eq!(view.read(&reader, 0, &mut buffer), Ok(buffer.len()));
    for worker in 0..WORKERS as usize {
        let region = &buffer[worker * REGION..(worker + 1) * REGION];
        assert_eq!(
            region,
            [b'a' + u8::try_from(worker).expect("eight workers"); REGION],
            "worker {worker}'s region holds {region:?}; a write landed at another handle's offset"
        );
    }
    view.close(reader).expect("the handle closes");
}

/// POSIX's unlink-while-open, which the conformance suite has no case for.
///
/// `OP-unlink/entry-is-gone` checks the name. Nothing checks what happens to a handle that was
/// open when the name went away, and on a real filesystem the answer is that the bytes stay
/// readable until the last handle closes — which is what an editor's save-over-an-open-file
/// depends on. This is evidence about the trait: `WorkspaceView` publishes no link count, so the
/// rule is unstated and every backend is free to disagree.
#[test]
fn a_handle_open_across_an_unlink_still_reads_the_bytes_it_was_opened_on() {
    let adapter = prepared();
    let view = adapter
        .view(mount(&adapter, 61, "/orphan"))
        .expect("a view");
    let entry = name("doomed.txt");
    let created = view
        .create_file(view.root(), &entry, PortableMetadata::default())
        .expect("a created file");
    let handle = view
        .open(created.object(), OpenMode::ReadWrite)
        .expect("a handle");
    assert_eq!(view.write(&handle, 0, b"still here"), Ok(10));

    view.unlink(view.root(), &entry).expect("an unlink");
    assert_eq!(
        view.lookup(view.root(), &entry).err(),
        Some(AdapterError::NotFound)
    );

    let mut buffer = [0u8; 16];
    assert_eq!(view.read(&handle, 0, &mut buffer), Ok(10));
    assert_eq!(&buffer[..10], b"still here");
    view.close(handle).expect("the handle closes");

    // And once the last handle is gone the object is gone with it: the identifier the caller still
    // holds names an inode that has been reclaimed, and the generation is what says so.
    assert_eq!(view.metadata(created.object()), Err(AdapterError::NotFound));
}

/// Acceptance criterion 3: a mount completes under 500 ms.
///
/// The **worst** of 100 mounts, not the mean: a criterion stated as a bound is a claim about the
/// slowest one. Two actors are alternated so that half the mounts create a tree and half attach to
/// one that exists, because those are two different costs and reporting only the cheap one would
/// be reporting the wrong number.
#[test]
fn a_mount_completes_far_inside_the_five_hundred_millisecond_budget() {
    const MOUNTS: u32 = 100;
    let adapter = prepared();
    let mut worst = std::time::Duration::ZERO;
    for round in 0..MOUNTS {
        let at = format!("/mount-{round}");
        let started = Instant::now();
        let mounted = adapter
            .mount_actor_view(
                adapter.workspace(),
                ActorId::from_bytes([70 + u8::try_from(round % 2).expect("two actors"); 32]),
                Path::new(&at),
            )
            .expect("a mount");
        worst = worst.max(started.elapsed());
        assert!(adapter.view(mounted.id()).is_ok());
    }
    println!("worst of {MOUNTS} mounts: {worst:?}");
    assert!(
        worst < std::time::Duration::from_millis(500),
        "the slowest of {MOUNTS} mounts took {worst:?} against a 500 ms budget"
    );
}
