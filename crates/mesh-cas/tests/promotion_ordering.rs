//! The order of the syscalls, and what a failure at each one leaves behind.
//!
//! # Why order is the thing being tested
//!
//! Every operation a promotion performs is individually harmless. What makes the sequence atomic is
//! that they happen in one particular order, and two reorderings that look innocent are the whole
//! bug:
//!
//! * **`rename` before `sync_file`** — the chunk becomes addressable while its bytes are only in
//!   the page cache. Kill the process and nothing is wrong; cut the power and the chunk is present
//!   and empty. That is the present-but-partial state everything downstream is entitled to assume
//!   cannot happen.
//! * **`rename` before the arrival record** — the chunk becomes visible before anything durable
//!   says it exists. Crash in between and it is referenced by nothing and known to nothing: a leak
//!   that only a full scan of the store can ever find.
//!
//! Neither reordering is caught by a test that promotes a chunk and reads it back, because both
//! produce a perfectly good chunk when nothing goes wrong. They are caught here, by recording what
//! the store asked the filesystem to do and asserting on the sequence.
//!
//! # Order is not enough: the set has to be complete
//!
//! Three pairwise orderings were asserted here for a long time, and all three passed over a
//! promotion that synced the chunk's leaf directory and neither of the two directories it had just
//! created on the way to it (`01KZE8JDBVPQ97MVMD9NFKVB9T`). An assertion that operation A precedes
//! operation B says nothing about operation C that never happened, so the tests below assert the
//! **set** of directory syncs as well as their order — completeness on a fresh store, and on a warm
//! one the *bound*: exactly the three directories on the chunk's own path, once each, whatever else
//! is in the store.
//!
//! That warm assertion used to be minimality — sync nothing this promotion did not create — and it
//! was replaced rather than deleted. `01KZEBAEK05P3XB9A788TYV4QM`: minimality assumes a directory
//! that exists had its own entry committed by whoever created it, and a promotion whose `sync_dir`
//! *fails* leaves one that did not, in the same process, with no crash. The promotions that skipped
//! it were the only ones that could have repaired it. The argument, and the microseconds it costs,
//! are on `a_promotion_into_an_existing_fanout_commits_the_whole_path_it_depends_on`.
//!
//! # And what it cannot test
//!
//! That `fsync` reaches the platter and that `rename` is atomic are the platform's promises, not
//! this crate's. Recording that the calls were made in the right order is exactly as strong as the
//! platform beneath them — see `DURABILITY.md`, which names the platforms where that promise is
//! not made and says the store refuses to run there rather than pretending.

mod support;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mesh_cas::{Blake3, Cas, CasError, ContentDigest, PromotionOutcome, PromotionStep};

use support::{Bytes, FailingFs, RecordingFs, TempRoot};

fn payload(seed: u64, length: usize) -> Vec<u8> {
    Bytes::seeded(seed).take(length)
}

/// The directories under `chunks/` that this recording says were synced, without repeats.
///
/// `logs/` is synced by the arrival journal on every promotion and is not what these tests are
/// about, so the filter is on the content tree rather than on `sync_dir` alone.
fn chunk_tree_syncs(filesystem: &RecordingFs, chunks: &Path) -> BTreeSet<PathBuf> {
    filesystem
        .paths("sync_dir")
        .into_iter()
        .filter(|path| path.starts_with(chunks))
        .collect()
}

#[test]
fn the_staged_bytes_are_synced_before_the_rename_that_reveals_them() {
    let root = TempRoot::new("order-sync");
    let store: Cas<RecordingFs, Blake3> =
        Cas::with_filesystem(root.path(), RecordingFs::new()).expect("the store opens");
    let bytes = payload(11, 4096);
    let digest = Blake3::digest_bytes(&bytes);

    store.filesystem().clear();
    let mut promotion = store.begin_promotion(bytes);
    promotion
        .run_through(PromotionStep::SyncDirectory)
        .expect("the promotion completes");
    let staged = store.layout().staging_path(&digest, std::process::id(), 0);

    let filesystem = store.filesystem();
    let sync = filesystem
        .position("sync_file", &staged)
        .expect("the staged file was synced");
    let rename = filesystem
        .position("rename", &staged)
        .expect("the staged file was renamed");
    assert!(
        sync < rename,
        "the staged bytes must be durable before the name that reveals them exists; the recorded \
         order was {:?}",
        filesystem.names()
    );
}

#[test]
fn the_arrival_is_recorded_before_the_rename_that_reveals_the_chunk() {
    let root = TempRoot::new("order-arrival");
    let store: Cas<RecordingFs, Blake3> =
        Cas::with_filesystem(root.path(), RecordingFs::new()).expect("the store opens");
    let bytes = payload(12, 4096);
    let digest = Blake3::digest_bytes(&bytes);

    store.filesystem().clear();
    store.promote(bytes).expect("the promotion completes");

    let filesystem = store.filesystem();
    let staged = store.layout().staging_path(&digest, std::process::id(), 0);
    let journal = store.layout().arrival_journal();
    let journal_sync = filesystem
        .position("sync_file", &journal)
        .expect("the arrival record was synced");
    let rename = filesystem
        .position("rename", &staged)
        .expect("the staged file was renamed");
    assert!(
        journal_sync < rename,
        "the arrival record must be durable before the chunk is visible, or a crash between them \
         leaks a chunk nothing can find; the recorded order was {:?}",
        filesystem.names()
    );
}

#[test]
fn the_chunk_directory_is_synced_after_the_rename() {
    let root = TempRoot::new("order-dirsync");
    let store: Cas<RecordingFs, Blake3> =
        Cas::with_filesystem(root.path(), RecordingFs::new()).expect("the store opens");
    let bytes = payload(13, 4096);
    let digest = Blake3::digest_bytes(&bytes);

    store.filesystem().clear();
    store.promote(bytes).expect("the promotion completes");

    let filesystem = store.filesystem();
    let staged = store.layout().staging_path(&digest, std::process::id(), 0);
    let directory = store.layout().chunk_directory(&digest);
    let rename = filesystem
        .position("rename", &staged)
        .expect("the rename happened");
    let sync = filesystem
        .position("sync_dir", &directory)
        .expect("the chunk's directory was synced");
    assert!(
        rename < sync,
        "syncing the directory before the rename would commit an entry that does not exist yet; \
         the recorded order was {:?}",
        filesystem.names()
    );
}

/// Every directory entry a promotion creates on the way to the chunk is committed before the call
/// returns — not only the entry naming the chunk.
///
/// This is the assertion whose absence let `01KZE8JDBVPQ97MVMD9NFKVB9T` live behind three green
/// ordering tests. It is phrased as a set so that removing any one of the three syncs fails it, and
/// the positions are read out of the recording rather than out of the source, so a sync issued
/// before the operation it is supposed to commit does not count.
#[test]
fn a_promotion_into_a_fresh_store_commits_every_directory_entry_it_creates() {
    let root = TempRoot::new("order-ancestors");
    let store: Cas<RecordingFs, Blake3> =
        Cas::with_filesystem(root.path(), RecordingFs::new()).expect("the store opens");
    let bytes = payload(18, 4096);
    let digest = Blake3::digest_bytes(&bytes);

    store.filesystem().clear();
    store.promote(bytes).expect("the promotion completes");

    let filesystem = store.filesystem();
    let chain = store.layout().chunk_directory_chain(&digest);
    assert_eq!(chain.len(), 3, "the fanout is `chunks/`, `<aa>`, `<bb>`");
    let staged = store.layout().staging_path(&digest, std::process::id(), 0);

    assert_eq!(
        chunk_tree_syncs(filesystem, &store.layout().chunks_directory()),
        chain.iter().cloned().collect::<BTreeSet<_>>(),
        "a promotion into a fresh store must commit the entry naming the chunk, the entry naming \
         its fanout directory and the entry naming that directory's parent. Syncing only the leaf \
         leaves the chunk's inode reachable through entries no `fsync` ever touched, so a power \
         loss after `promote` returned can unmake a chunk it just reported. The recorded \
         operations were {:?}",
        filesystem.names()
    );

    let created = |path: &PathBuf| {
        filesystem
            .position("create_dir_all", path)
            .unwrap_or_else(|| panic!("{path:?} was created by this promotion"))
    };
    let synced = |path: &PathBuf| {
        filesystem
            .position("sync_dir", path)
            .unwrap_or_else(|| panic!("{path:?} was synced by this promotion"))
    };
    let rename = filesystem
        .position("rename", &staged)
        .expect("the staged file was renamed");

    assert!(
        created(&chain[1]) < synced(&chain[0]),
        "`chunks/` is synced to commit the entry naming `<aa>`, so it has to be synced after that \
         entry exists; the recorded order was {:?}",
        filesystem.names()
    );
    assert!(
        created(&chain[2]) < synced(&chain[1]),
        "`<aa>` is synced to commit the entry naming `<bb>`, so it has to be synced after that \
         entry exists; the recorded order was {:?}",
        filesystem.names()
    );
    assert!(
        rename < synced(&chain[2]),
        "`<bb>` is synced to commit the entry naming the chunk, so it has to be synced after the \
         rename; the recorded order was {:?}",
        filesystem.names()
    );
    assert!(
        synced(&chain[0]) < rename && synced(&chain[1]) < rename,
        "the path to the chunk is committed before the rename that makes the chunk visible, for \
         the same reason the data is flushed before the name that reveals it; the recorded order \
         was {:?}",
        filesystem.names()
    );
}

/// A promotion into fanout directories that already exist commits the whole path anyway — and
/// commits nothing else.
///
/// # What this replaces, and the argument for replacing it
///
/// This assertion used to be the opposite one,
/// `a_promotion_into_an_existing_fanout_syncs_no_ancestor_it_did_not_create`, which failed if a
/// promotion committed a directory it had not created. That was minimality, and it was bought with
/// an assumption — `DURABILITY.md` assumption 8, *whoever created that directory got as far as
/// syncing its parent*. The assumption is false in a state this crate can reach with no crash at
/// all, and `a_fanout_directory_left_uncommitted_by_a_failed_promotion_is_committed_by_the_next_one`
/// below reaches it: `sync_dir` returns an error, the promotion returns `Err` over a directory that
/// exists and whose entry in its parent is not durable, and every later promotion into that fanout
/// slot was the one that skipped it. The skip and the repair were the same promotions.
///
/// The two assertions cannot both hold, so the choice is a price against a loss. The price,
/// measured on the host in `benchmarks/budgets/storage.md` §1 and recorded in `DURABILITY.md`:
/// **14.6–15.0 µs** per steady-state promotion, two `fsync` calls of directories nothing has
/// written to. The loss: every chunk under that fanout directory, on a power cut, with nothing a
/// reader could have checked beforehand.
///
/// **Minimality is not abandoned; it is restated as a bound.** The old test also ruled out the
/// correct-and-ruinous repair — sync every ancestor of every chunk in the store — and that is still
/// ruled out, by asserting the syncs are *exactly* the three directories on this chunk's own path,
/// once each. A promotion's cost stays constant in the size of the store; the constant is three
/// rather than one.
#[test]
fn a_promotion_into_an_existing_fanout_commits_the_whole_path_it_depends_on() {
    let root = TempRoot::new("order-ancestors-warm");
    let store: Cas<RecordingFs, Blake3> =
        Cas::with_filesystem(root.path(), RecordingFs::new()).expect("the store opens");
    let bytes = payload(19, 4096);
    let digest = Blake3::digest_bytes(&bytes);

    // Created outside the store, so the promotion below creates nothing: this is the steady state
    // of a workspace whose 65 536 fanout directories already exist.
    let chain = store.layout().chunk_directory_chain(&digest);
    std::fs::create_dir_all(&chain[2]).expect("the fanout directory is creatable");

    store.filesystem().clear();
    store.promote(bytes).expect("the promotion completes");

    let filesystem = store.filesystem();
    let chunks = store.layout().chunks_directory();
    assert_eq!(
        filesystem
            .paths("create_dir_all")
            .into_iter()
            .filter(|path| path.starts_with(&chunks))
            .count(),
        0,
        "this promotion creates no directory at all; if it did, the steady state is not what is \
         being measured. The recorded operations were {:?}",
        filesystem.names()
    );

    let synced: Vec<PathBuf> = filesystem
        .paths("sync_dir")
        .into_iter()
        .filter(|path| path.starts_with(&chunks))
        .collect();
    assert_eq!(
        synced,
        chain,
        "a promotion that creates nothing still depends on all three entries on the path to the \
         chunk being durable, and it cannot see whether a predecessor committed them — a \
         predecessor that took an error from its own `sync_dir` left the directory standing and \
         the entry naming it uncommitted. The recorded operations were {:?}",
        filesystem.names()
    );
    assert_eq!(
        synced.len(),
        3,
        "the price of committing the path is bounded by the depth of the fanout and never by the \
         size of the store: three directories, once each, whatever else is in `chunks/`. A repair \
         that walked further would be correct and would stop being constant-time"
    );
}

/// The window assumption 8 bought, reproduced — and then closed.
///
/// # The window
///
/// A promotion creates `chunks/<aa>/<bb>` and then commits the entry naming it by syncing
/// `chunks/<aa>`. Between those two operations the directory exists and its name is not durable.
/// The skip that used to follow — *a directory that already exists has had its entry committed* —
/// meant every later promotion into that fanout slot walked past it, so the entry was committed by
/// nobody, ever, and a power cut could unmake the whole subtree under it.
///
/// # Reaching it without a crash
///
/// `01KZEBAEK05P3XB9A788TYV4QM` described a process killed inside the window, which needs a crash,
/// a later promotion into that specific 1-in-65 536 slot, and a power loss. The variant below needs
/// none of the first: **`sync_dir` can fail.** A promotion that takes an error from the `fsync` of
/// `chunks/<aa>` returns `Err` and leaves exactly the same residue, in the same process, with its
/// destructor running normally. `a_filesystem_failure_at_any_operation_leaves_no_partial_chunk`
/// already injects that failure and passes, because the *chunk* is consistent either way; what was
/// inconsistent is the path to it, which no assertion looked at.
///
/// `RecordingFs::refusing` names the operation rather than counting to it, so this test does not go
/// stale the next time the promotion sequence gains a step.
///
/// # What is asserted
///
/// That the second promotion commits `chunks/<aa>`. Under the skip it did not, and neither did the
/// first — that is the window — so this test is red on the skip and is the pin for the choice
/// `a_promotion_into_an_existing_fanout_commits_the_whole_path_it_depends_on` argues for.
#[test]
fn a_fanout_directory_left_uncommitted_by_a_failed_promotion_is_committed_by_the_next_one() {
    let root = TempRoot::new("order-uncommitted-fanout");
    let bytes = payload(21, 4096);
    let digest = Blake3::digest_bytes(&bytes);

    // `chunks/` → `<aa>` → `<aa>/<bb>`. The entry naming `<bb>` lives in `chain[1]`, so `chain[1]`
    // is the directory whose `fsync` commits it, and the one this test refuses.
    let chain = Cas::open(root.path())
        .expect("the store opens")
        .layout()
        .chunk_directory_chain(&digest);
    let fanout_parent = chain[1].clone();
    let leaf = chain[2].clone();

    let interrupted: Cas<RecordingFs, Blake3> = Cas::with_filesystem(
        root.path(),
        RecordingFs::refusing("sync_dir", fanout_parent.clone()),
    )
    .expect("the store reopens");
    interrupted.filesystem().clear();
    let refused = interrupted.promote(bytes.clone());

    assert!(
        refused.is_err(),
        "the promotion was to be stopped by the refused `sync_dir`; it returned {refused:?}"
    );
    assert!(
        leaf.exists(),
        "the reproduction needs the fanout directory created before the promotion stopped, which \
         is the whole of the window: {}",
        leaf.display()
    );
    assert!(
        !interrupted.contains(&digest),
        "the promotion stopped before the rename, so the chunk is not in the store — the residue \
         is the directory, not the content"
    );
    assert!(
        interrupted
            .filesystem()
            .position("sync_dir", &fanout_parent)
            .is_none(),
        "the entry naming the fanout directory was committed after all, so there is no window here \
         and this test is measuring something else. The recorded operations were {:?}",
        interrupted.filesystem().operations()
    );
    drop(interrupted);

    // A second promotion into the same fanout slot, on a filesystem that refuses nothing. Under
    // the skip this took the `exists` branch and committed nothing above the leaf.
    let store: Cas<RecordingFs, Blake3> =
        Cas::with_filesystem(root.path(), RecordingFs::new()).expect("the store reopens");
    store.filesystem().clear();
    let promoted = store
        .promote(bytes)
        .expect("the second promotion completes");

    assert_eq!(
        promoted.outcome(),
        PromotionOutcome::Linked,
        "the first promotion never renamed, so this one is the promotion that writes the content"
    );
    assert!(
        store
            .filesystem()
            .position("sync_dir", &fanout_parent)
            .is_some(),
        "the fanout directory already existed, so this promotion skipped it — and the promotion \
         that created it stopped before committing its name. No promotion into this slot will ever \
         commit it, so the caller is told a chunk is durable while the entry naming the directory \
         it lives in is not. The recorded operations were {:?}",
        store.filesystem().names()
    );
    assert_eq!(
        chunk_tree_syncs(store.filesystem(), &store.layout().chunks_directory()),
        chain.iter().cloned().collect::<BTreeSet<_>>(),
        "the whole path to the chunk is committed by the promotion that makes it visible, whoever \
         created the directories on it. The recorded operations were {:?}",
        store.filesystem().names()
    );
}

/// A promotion that finds the chunk already there still commits its directory entry.
///
/// The regression seed for `01KZE8KDMMQQDV62C83AZWFVSS`. The predecessor is driven through
/// [`PromotionStep::Link`] and stopped, which is the state a `SIGKILL` between `Link` and
/// `SyncDirectory` leaves behind — `crash-promotion.rs::killing_after_linking_leaves_a_whole_readable_chunk`
/// is what proves a real signal reaches it. The chunk is then visible and its directory entry has
/// been committed by nobody, and the deduplicated path used to return `Ok` without ever committing
/// it, for this promotion or any later one.
#[test]
fn a_promotion_that_finds_the_chunk_already_there_still_syncs_its_directory() {
    let root = TempRoot::new("order-dedup-sync");
    let bytes = payload(20, 4096);
    let digest = Blake3::digest_bytes(&bytes);

    {
        let store = Cas::open(root.path()).expect("the store opens");
        let mut promotion = store.begin_promotion(bytes.clone());
        promotion
            .run_through(PromotionStep::Link)
            .expect("the promotion reaches the rename");
        assert!(
            store.contains(&digest),
            "the rename made the chunk visible, which is what makes the next promotion take the \
             deduplicated path"
        );
        // Dropping here removes nothing: `Link` consumed the staged file, so the destructor has
        // nothing to clean up and the store is left exactly as a killed process would leave it.
    }

    let store: Cas<RecordingFs, Blake3> =
        Cas::with_filesystem(root.path(), RecordingFs::new()).expect("the store reopens");
    store.filesystem().clear();
    let promoted = store.promote(bytes).expect("the re-promotion completes");

    assert_eq!(
        promoted.outcome(),
        PromotionOutcome::AlreadyPresent,
        "the chunk was already there, so this promotion wrote no content"
    );
    assert!(
        store
            .filesystem()
            .position("sync_dir", &store.layout().chunk_directory(&digest))
            .is_some(),
        "the deduplicated path returned `Ok` without committing the directory entry that makes the \
         chunk visible. No later promotion of the same content will do it either — every one of \
         them takes this same path — so the caller is entitled to commit a durable reference to a \
         chunk a power loss can still remove. The recorded operations were {:?}",
        store.filesystem().names()
    );
    assert_eq!(
        chunk_tree_syncs(store.filesystem(), &store.layout().chunks_directory()),
        store
            .layout()
            .chunk_directory_chain(&digest)
            .into_iter()
            .collect::<BTreeSet<_>>(),
        "the deduplicated path commits the whole path, not just the leaf: a chunk that is already \
         present is standing in a `<bb>` whose own entry in `<aa>` may have been committed by \
         nobody, for exactly the reason this promotion cannot see \
         (`01KZEBAEK05P3XB9A788TYV4QM`). The recorded operations were {:?}",
        store.filesystem().names()
    );
}

#[test]
fn verification_reads_the_staged_file_back_from_disk_rather_than_trusting_the_buffer() {
    let root = TempRoot::new("order-verify");
    let store: Cas<RecordingFs, Blake3> =
        Cas::with_filesystem(root.path(), RecordingFs::new()).expect("the store opens");
    let bytes = payload(14, 4096);
    let digest = Blake3::digest_bytes(&bytes);

    store.filesystem().clear();
    let mut promotion = store.begin_promotion(bytes);
    promotion
        .run_through(PromotionStep::Verify)
        .expect("verification succeeds");

    let staged = store.layout().staging_path(&digest, std::process::id(), 0);
    let filesystem = store.filesystem();
    let read = filesystem.position("read", &staged).expect(
        "verification read the staged file back; hashing the in-memory buffer would prove \
                 only that the caller agrees with itself",
    );
    let sync = filesystem
        .position("sync_file", &staged)
        .expect("the file was synced");
    assert!(
        sync < read,
        "the read-back must follow the sync, or it can be served entirely from a cache that a \
         power loss would discard; the recorded order was {:?}",
        filesystem.names()
    );
}

#[test]
fn nothing_lists_the_chunk_tree_during_a_promotion() {
    let root = TempRoot::new("order-nolist");
    let store: Cas<RecordingFs, Blake3> =
        Cas::with_filesystem(root.path(), RecordingFs::new()).expect("the store opens");

    store.filesystem().clear();
    store
        .promote(payload(15, 4096))
        .expect("the promotion completes");

    assert_eq!(
        store.filesystem().count("list_dir"),
        0,
        "a promotion that scans the store does not stay constant-time as the store grows; the \
         recorded operations were {:?}",
        store.filesystem().names()
    );
}

/// A failure at each mutating operation leaves the store consistent — no chunk, or a whole one.
///
/// This is the complement to `crash-promotion.rs`: that file kills the process, this one makes the
/// filesystem refuse. The two failure modes are different — a refusal unwinds and runs destructors,
/// a kill does not — and a store has to survive both.
#[test]
fn a_filesystem_failure_at_any_operation_leaves_no_partial_chunk() {
    let bytes = payload(16, 8192);
    let digest = Blake3::digest_bytes(&bytes);

    // How many mutating operations opening a store and promoting one chunk into it actually
    // performs, measured rather than guessed. The previous version of this test used a constant
    // ten, which was "comfortably past" the count of the day and stopped being so the moment the
    // promotion learned to sync the directories it creates — silently, because the assertion below
    // was a lower bound. Counting first makes the coverage claim exact and self-maintaining.
    let probe_root = TempRoot::new("fail-probe");
    let probe: Cas<FailingFs, Blake3> =
        Cas::with_filesystem(probe_root.path(), FailingFs::new(usize::MAX))
            .expect("the probe store opens");
    probe
        .promote(bytes.clone())
        .expect("the probe promotion completes");
    let mutating = probe.filesystem().performed();
    assert!(
        mutating >= 10,
        "a promotion into a fresh store performs {mutating} mutating operations, fewer than this \
         test has ever seen; the sequence has lost a step rather than gained one"
    );

    let mut failures = 0;
    for fail_at in 1..=mutating {
        let root = TempRoot::new(&format!("fail-{fail_at}"));
        let store: Cas<FailingFs, Blake3> =
            match Cas::with_filesystem(root.path(), FailingFs::new(fail_at)) {
                Ok(store) => store,
                // The injected failure hit directory creation during `open`. Nothing was promoted,
                // which is the consistent outcome, and there is nothing further to check.
                Err(_) => {
                    failures += 1;
                    continue;
                }
            };

        let outcome = store.promote(bytes.clone());
        if outcome.is_err() {
            failures += 1;
        }

        // Whether the promotion failed or squeaked through, the store is in one of the two allowed
        // states and never in a third.
        let inspector = Cas::open(root.path()).expect("the store reopens on a real filesystem");
        if inspector.contains(&digest) {
            assert_eq!(
                inspector.read(&digest).expect("a visible chunk verifies"),
                bytes,
                "failure at operation {fail_at} left a visible chunk with the wrong bytes"
            );
        } else {
            assert!(
                matches!(
                    inspector.read(&digest),
                    Err(CasError::Absent { .. })
                ),
                "failure at operation {fail_at} left something at the chunk's name that is not the \
                 chunk"
            );
        }
    }
    assert_eq!(
        failures, mutating,
        "only {failures} of the {mutating} injected failures produced an error; an operation on \
         the success path that can fail without the promotion noticing is an operation whose \
         failure is being swallowed"
    );
}

/// A promotion abandoned by an ordinary error path leaves no litter — the destructor removes the
/// staged file. This is the case a crash cannot cover, and `discard_scratch` covers that one.
#[test]
fn dropping_a_promotion_before_the_rename_removes_the_staged_file() {
    let root = TempRoot::new("drop-cleanup");
    let store = Cas::open(root.path()).expect("the store opens");

    {
        let mut promotion = store.begin_promotion(payload(17, 2048));
        promotion
            .run_through(PromotionStep::Verify)
            .expect("verification succeeds");
        let staged = promotion
            .staged_path()
            .expect("the file is staged")
            .to_path_buf();
        assert!(
            staged.exists(),
            "the staged file is there while the promotion is alive"
        );
    }

    assert_eq!(
        store.discard_scratch().expect("scratch is readable"),
        0,
        "the dropped promotion removed its own staged file"
    );
}
