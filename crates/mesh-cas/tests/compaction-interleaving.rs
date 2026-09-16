//! What a compaction does about an arrival that lands while it is running.
//!
//! Compaction folds the journal, writes the replacement under a second name, and renames it over
//! the original. Every byte appended between the fold and the rename goes to the inode the rename
//! discards. The record is destroyed while the chunk it describes stays visible in the store, so
//! `unreferenced_candidates` never offers that chunk again and only `sweep_all_chunks` — the full
//! scan the journal exists to avoid — can still find it. The journal looks healthy afterwards,
//! which is what makes this worth a test file of its own rather than a line in `unreferenced.rs`.
//!
//! Two things hold it, and they are tested separately because they fail separately.
//!
//! * **Exclusion**, for an arrival inside this process: an append takes the shared side of the
//!   journal's lock and a rewrite takes the exclusive side, so the two cannot overlap.
//!   `a_promotion_racing_a_compaction_is_excluded_rather_than_lost` drives a real promotion on a
//!   second thread and pins the two together with a handshake in each direction, so the rewrite is
//!   provably still holding the lock at the moment the promotion is provably about to want it.
//! * **Refusal**, for an arrival this process cannot exclude — a second process, which the store's
//!   model does not admit but a filesystem cannot prevent.
//!   `an_arrival_recorded_during_compaction_is_not_destroyed_by_it` forces exactly that byte in at
//!   exactly the moment that destroys it, by appending to the journal underneath the crate's own
//!   lock. It is the test the acceptance criterion asks for.
//!
//! Neither test waits on a clock, and no assertion here is about elapsed time. Both handshakes are
//! channel sends, so a scheduler that runs one thread slowly makes the other one wait rather than
//! making this file fail.

mod support;

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use mesh_cas::{Blake3, Cas, CasError, Digest32, DurableFs, StdFs};

use support::{Bytes, TempRoot};

fn payload(seed: u64, length: usize) -> Vec<u8> {
    Bytes::seeded(seed).take(length)
}

/// The 67 bytes `record_arrival` writes, built here rather than borrowed from the crate so that a
/// change to the record format shows up as a failing test instead of a silently agreeing one.
fn arrival_record(digest: &Digest32) -> Vec<u8> {
    let mut record = Vec::with_capacity(67);
    record.push(b'+');
    record.push(b' ');
    record.extend_from_slice(digest.to_hex().as_bytes());
    record.push(b'\n');
    record
}

/// A digest that no chunk in these fixtures has, so its presence in a fold is unambiguous.
fn outsider() -> Digest32 {
    Digest32::parse_hex(&"ab".repeat(32)).expect("64 hex characters parse")
}

/// A filesystem that appends one arrival record straight to the journal the first time a rewrite
/// is staged.
///
/// It appends through [`StdFs`] rather than through [`mesh_cas::ArrivalJournal`], which is the
/// whole point: going through the journal would take the shared guard and block, which is the
/// behaviour the *other* test measures. Bypassing it reproduces the only writer the crate cannot
/// exclude — a second process — at the one instant where the bytes are lost.
struct AppendsDuringRewrite {
    inner: StdFs,
    journal: PathBuf,
    rewrite: PathBuf,
    racing: Digest32,
    fired: AtomicBool,
}

impl AppendsDuringRewrite {
    fn new(journal: PathBuf, rewrite: PathBuf, racing: Digest32) -> Self {
        Self {
            inner: StdFs,
            journal,
            rewrite,
            racing,
            fired: AtomicBool::new(false),
        }
    }

    fn has_fired(&self) -> bool {
        self.fired.load(Ordering::SeqCst)
    }
}

impl DurableFs for AppendsDuringRewrite {
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.inner.create_dir_all(path)
    }

    fn stage(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        let staged = self.inner.stage(path, bytes);
        // After the replacement exists and before anything points at it: the fold has happened,
        // the rename has not, and this is the byte the rename would throw away.
        if path == self.rewrite && !self.fired.swap(true, Ordering::SeqCst) {
            self.inner
                .append(&self.journal, &arrival_record(&self.racing))?;
            self.inner.sync_file(&self.journal)?;
        }
        staged
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
        self.inner.remove_file(path)
    }

    fn list_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        self.inner.list_dir(path)
    }
}

/// A filesystem that releases a promoting thread the first time a rewrite is staged, and then
/// waits for that thread to reach the journal's lock before letting the rewrite continue.
///
/// The wait is what makes the interleaving forced rather than hoped for. Without it the rewrite
/// runs on to its rename in the time the released thread needs to stage and sync a chunk, and the
/// test passes whether or not any exclusion exists — measured, not assumed: with the shared guard
/// removed from `append_record`, the un-waiting version passed six runs out of six.
struct ReleasesDuringRewrite {
    inner: StdFs,
    rewrite: PathBuf,
    release: Sender<()>,
    arrived: Receiver<()>,
    fired: AtomicBool,
}

impl DurableFs for ReleasesDuringRewrite {
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.inner.create_dir_all(path)
    }

    fn stage(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        let staged = self.inner.stage(path, bytes);
        if path == self.rewrite && !self.fired.swap(true, Ordering::SeqCst) {
            // A failed send or receive means the racer is gone, which the join reports with a
            // better message than a panic from inside a filesystem call would — and, importantly,
            // without hanging: `recv` on a dropped sender returns rather than blocking.
            let _ = self.release.send(());
            let _ = self.arrived.recv();
        }
        staged
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
        self.inner.remove_file(path)
    }

    fn list_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        self.inner.list_dir(path)
    }
}

/// A filesystem that announces this thread's arrival at the journal's lock.
///
/// `record_arrival` creates the log directory and *then* takes the shared guard, so the last thing
/// a promoting thread does before it can block is `create_dir_all` on that directory. Announcing
/// there puts the announcement as close to the lock as an observer outside the crate can get: what
/// follows it is the guard acquisition and nothing else.
///
/// Armed after the store is opened, because opening one creates every store directory — including
/// this one — and an unarmed signal would fire on that instead.
struct AnnouncesAtTheJournalLock {
    inner: StdFs,
    logs: PathBuf,
    arrived: Sender<()>,
    armed: AtomicBool,
    fired: AtomicBool,
}

impl AnnouncesAtTheJournalLock {
    fn arm(&self) {
        self.armed.store(true, Ordering::SeqCst);
    }
}

impl DurableFs for AnnouncesAtTheJournalLock {
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        let created = self.inner.create_dir_all(path);
        if path == self.logs
            && self.armed.load(Ordering::SeqCst)
            && !self.fired.swap(true, Ordering::SeqCst)
        {
            let _ = self.arrived.send(());
        }
        created
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
        self.inner.remove_file(path)
    }

    fn list_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        self.inner.list_dir(path)
    }
}

/// The reproduction, forced rather than raced: a record appended between the fold and the rename.
///
/// The assertion is the acceptance criterion's disjunction, and both halves are checked rather
/// than either. The compaction **refuses**, and because it refuses before the rename the record is
/// also **preserved** — the journal is left byte-for-byte as the interleaved append left it and
/// the racing digest is still a candidate. The refusal is asserted as a refusal, not as the
/// absence of a crash: a compaction that returned `Ok` here would have passed a "did not panic"
/// test while destroying the record.
#[test]
fn an_arrival_recorded_during_compaction_is_not_destroyed_by_it() {
    let root = TempRoot::new("compact-interleaved");
    let racing = outsider();

    let layout = Cas::open(root.path()).expect("the store opens");
    let journal_path = layout.layout().arrival_journal();
    let rewrite_path = layout.layout().arrival_journal_rewrite();
    drop(layout);

    let store: Cas<AppendsDuringRewrite, Blake3> = Cas::with_filesystem(
        root.path(),
        AppendsDuringRewrite::new(journal_path.clone(), rewrite_path, racing),
    )
    .expect("the store opens");

    let mut promoted = Vec::new();
    for seed in 200..210 {
        promoted.push(
            store
                .promote(payload(seed, 256))
                .expect("the promotion completes")
                .digest(),
        );
    }
    // Some retentions, so the compaction has real work and its output would differ from its input.
    for digest in promoted.iter().take(4) {
        store
            .journal()
            .record_retained(digest)
            .expect("the retention records");
    }

    let before = std::fs::read(&journal_path).expect("the journal exists");

    let outcome = store.journal().compact();

    assert!(
        store.filesystem().has_fired(),
        "the interleaved append never ran, so this test proved nothing about compaction"
    );
    match outcome {
        Err(CasError::Io { operation, .. }) => assert_eq!(
            operation, "compact",
            "the refusal came from somewhere other than the pre-rename check"
        ),
        Err(other) => panic!("compaction failed for the wrong reason: {other}"),
        Ok(written) => panic!(
            "compaction wrote a {written}-record journal over a file it had not fully read, \
             which destroys the arrival that landed while it ran"
        ),
    }

    let after = std::fs::read(&journal_path).expect("the journal exists");
    let mut expected = before.clone();
    expected.extend_from_slice(&arrival_record(&racing));
    assert_eq!(
        after, expected,
        "the refused compaction did not leave the journal alone; it must be the bytes it folded \
         plus the record that arrived, and nothing else"
    );

    let candidates = store
        .journal()
        .candidates()
        .expect("the journal is readable");
    assert!(
        candidates.contains(&racing),
        "the chunk that arrived during compaction is no longer a candidate, so nothing but a full \
         store scan can ever find it again"
    );
    for digest in promoted.iter().skip(4) {
        assert!(
            candidates.contains(digest),
            "a chunk that was a candidate before the refused compaction is not one after it"
        );
    }
}

/// The refusal is recoverable, and recovering does not need the operator to know anything.
///
/// The hook fires once, so the second compaction runs unraced. It must succeed, shrink the file,
/// and keep the record the first one refused to destroy — otherwise "refuse" would mean "this
/// journal can never be compacted again", which is a worse answer than the defect.
#[test]
fn a_refused_compaction_leaves_the_journal_compactable() {
    let root = TempRoot::new("compact-retry");
    let racing = outsider();

    let layout = Cas::open(root.path()).expect("the store opens");
    let journal_path = layout.layout().arrival_journal();
    let rewrite_path = layout.layout().arrival_journal_rewrite();
    drop(layout);

    let store: Cas<AppendsDuringRewrite, Blake3> = Cas::with_filesystem(
        root.path(),
        AppendsDuringRewrite::new(journal_path.clone(), rewrite_path, racing),
    )
    .expect("the store opens");

    let mut promoted = Vec::new();
    for seed in 220..232 {
        promoted.push(
            store
                .promote(payload(seed, 256))
                .expect("the promotion completes")
                .digest(),
        );
    }
    for digest in promoted.iter().take(6) {
        store
            .journal()
            .record_retained(digest)
            .expect("the retention records");
    }

    assert!(
        store.journal().compact().is_err(),
        "the raced compaction was expected to refuse"
    );

    let before = std::fs::metadata(&journal_path)
        .expect("the journal exists")
        .len();
    let expected = store
        .journal()
        .candidates()
        .expect("the journal is readable");

    let written = store
        .journal()
        .compact()
        .expect("an unraced compaction succeeds after a refused one");

    assert_eq!(written, expected.len());
    let after = std::fs::metadata(&journal_path)
        .expect("the journal exists")
        .len();
    assert!(
        after < before,
        "compaction left the journal at {after} bytes, no smaller than the {before} it started at"
    );
    assert_eq!(
        store
            .journal()
            .candidates()
            .expect("the journal is readable"),
        expected,
        "the retried compaction changed which chunks are candidates"
    );
    assert!(
        expected.contains(&racing),
        "the record the first compaction refused to destroy was destroyed by the second"
    );
}

/// A real promotion on a second thread, held at the journal's lock while the rewrite runs.
///
/// This is the in-process half, and its oracle is that **nothing has to refuse**: the promotion
/// takes the shared side of the journal's lock, the rewrite holds the exclusive side, so the
/// append happens strictly before the fold or strictly after the rename and never in between. If
/// the exclusion were not there, the append would land in the discarded inode, the pre-rename
/// check would see a file it had not folded, and `compact` would return the error the test above
/// asserts — so `Ok` here is a claim about the lock and not merely about the absence of a crash.
///
/// The overlap is forced in both directions rather than hoped for. The rewrite does not stage
/// until it has released the racer, and does not proceed past its `stage` until the racer has
/// announced that it has finished the last filesystem call before the guard. So at the moment the
/// rewrite goes on to sync and rename, the racer is inside `record_arrival` with only the lock
/// left to take.
#[test]
fn a_promotion_racing_a_compaction_is_excluded_rather_than_lost() {
    let root = TempRoot::new("compact-exclusion");

    let layout = Cas::open(root.path()).expect("the store opens");
    let rewrite_path = layout.layout().arrival_journal_rewrite();
    let logs_path = layout.layout().logs_directory();
    drop(layout);

    let (release, released) = mpsc::channel::<()>();
    let (arrived_tx, arrived_rx) = mpsc::channel::<()>();
    let store: Cas<ReleasesDuringRewrite, Blake3> = Cas::with_filesystem(
        root.path(),
        ReleasesDuringRewrite {
            inner: StdFs,
            rewrite: rewrite_path,
            release,
            arrived: arrived_rx,
            fired: AtomicBool::new(false),
        },
    )
    .expect("the store opens");

    let mut promoted = Vec::new();
    for seed in 240..252 {
        promoted.push(
            store
                .promote(payload(seed, 256))
                .expect("the promotion completes")
                .digest(),
        );
    }
    for digest in promoted.iter().take(5) {
        store
            .journal()
            .record_retained(digest)
            .expect("the retention records");
    }

    // A second `Cas` over the same root, so this also exercises the exclusion being keyed by the
    // journal's path rather than owned by one store value.
    let racer_root = root.path().to_path_buf();
    let racer = thread::spawn(move || {
        let racing_store: Cas<AnnouncesAtTheJournalLock, Blake3> = Cas::with_filesystem(
            &racer_root,
            AnnouncesAtTheJournalLock {
                inner: StdFs,
                logs: logs_path,
                arrived: arrived_tx,
                armed: AtomicBool::new(false),
                fired: AtomicBool::new(false),
            },
        )
        .expect("the second store opens");
        racing_store.filesystem().arm();
        released.recv().expect("the rewrite releases the racer");
        racing_store
            .promote(payload(253, 256))
            .expect("the racing promotion completes")
            .digest()
    });

    let written = store
        .journal()
        .compact()
        .expect("a compaction that excludes appends has nothing to refuse");
    let racing = racer.join().expect("the racing thread does not panic");

    let candidates = store
        .journal()
        .candidates()
        .expect("the journal is readable");
    assert!(
        candidates.contains(&racing),
        "the chunk promoted while the journal was being rewritten is not a candidate, so the \
         rewrite replaced the file its arrival record was appended to"
    );
    assert!(
        store.read(&racing).is_ok(),
        "the racing chunk is not in the store, so this test raced nothing"
    );
    for digest in promoted.iter().skip(5) {
        assert!(
            candidates.contains(digest),
            "a chunk that was a candidate before the compaction is not one after it"
        );
    }
    assert!(
        written <= candidates.len(),
        "the compaction reported {written} records while {} chunks are candidates",
        candidates.len()
    );
}
