//! Killing a real process at every point of a promotion, and checking what is left.
//!
//! # Why this is a child process and not a mocked interruption
//!
//! The claim under test is that no interruption leaves a partial chunk. An interruption modelled
//! by returning `Err` from a fake filesystem tests the error path — worth testing, and
//! `promotion_ordering.rs` does — but it is not an interruption: destructors run, buffers flush,
//! and the very cleanup a crash skips is exactly what happens. So these tests spawn this same test
//! binary as a child, drive it to a chosen step, and send it `SIGKILL`. No destructor runs, no
//! buffer is flushed, and [`std::process::ExitStatus::code`] returning `None` is asserted in every
//! case so that a child which quietly exited on its own can never be mistaken for one that was
//! killed.
//!
//! # The two campaigns
//!
//! * **Every step boundary.** Six steps, plus the point before any of them, each with an exact
//!   expected residue. These are deterministic: they assert not just that the store is consistent
//!   but that it is in the *specific* state plan §6.3 says it should be in.
//! * **Sixteen points inside the steps.** Sixteen children killed *inside* a two-megabyte
//!   promotion — mid-write, mid-`fsync`, mid-`rename`, mid-journal-record — at points named by the
//!   work the child has done and never by how long it has been running. Each asserts the invariant
//!   *and* the exact side of the rename its point falls on, because with the child stopped at a
//!   named point that side is a fact about the code rather than a guess about the machine.
//!
//! # Why the interior campaign is no longer scheduled by a clock (`01KZEBZW17SGQ2JR9GHGBND5R7`)
//!
//! It used to be. The parent measured how long one whole promotion took, then killed sixteen
//! children at delays spread from a third of that span to one and a half times it, and failed if
//! the sixteen corpses did not straddle the rename. The delays were measured rather than chosen
//! because the first version chose them and all sixteen kills landed before the first step
//! finished: process startup dwarfed every constant that looked reasonable.
//!
//! Measuring fixed the constant and left the defect. The schedule was valid only while the
//! machine's load after the calibration matched its load during it, and nothing made that true —
//! under `cargo nextest run --workspace` the load is decided by nextest's scheduler, which starts
//! long disk-heavy binaries at arbitrary points relative to this one. Two whole-suite runs in
//! eleven failed here, one of them on a branch whose only change was a JSON file, with a message
//! that already knew the answer: *the calibration is wrong on this machine*. A merge gate that
//! calls a change broken because another test was doing disk I/O teaches every lane to re-run
//! rather than read, which disables it for the case that counts.
//!
//! So the clock is gone. The child runs on a [`PausingFs`] — the same [`DurableFs`] seam
//! `promotion_ordering.rs` uses to observe order — which performs the real operation up to a named
//! point, announces, and blocks. The parent kills at the handshake, exactly as the boundary
//! campaign does. Load changes how long the campaign takes and cannot change what it asserts.
//!
//! This is not a weaker interruption than a timed one. The child is `SIGKILL`ed while stopped
//! *inside* `stage`'s write, holding a partial file that was never flushed, or between the `rename`
//! and the directory sync: no destructor runs, no buffer is flushed, and `discard_scratch` still
//! proves the promotion's own cleanup never ran. What it is not is *random* — a timed kill sampled
//! the interior and this enumerates it, which is the stronger of the two so long as the enumeration
//! is the one below and a reader can check it against the six steps.
//!
//! # What a `SIGKILL` proves, and what it does not
//!
//! It proves the process died without cleaning up: it covers every failure in which the machine
//! keeps running, which is process crashes, `OOM` kills and forced termination. It does **not**
//! prove behaviour under power loss, because the page cache survives a killed process and does not
//! survive a power cut. Power-loss durability rests on the platform honouring `fsync` and on
//! `rename` being atomic; this crate issues those calls in the order that makes the guarantee hold
//! — asserted in `promotion_ordering.rs` — and states in `DURABILITY.md` that the platform's half
//! of that bargain is assumed, not measured here. Anyone reading these tests as proof of
//! power-loss safety is reading more than they say.

mod support;

use std::fs::OpenOptions;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use mesh_cas::{
    Blake3, Cas, ContentDigest, Digest32, DurableFs, PromotionStep, StdFs,
    ARRIVAL_JOURNAL_FILE_NAME, CHUNKS_DIRECTORY_NAME, SCRATCH_DIRECTORY_NAME,
};

use support::{Bytes, TempRoot};

/// The workspace root the child promotes into.
const ROOT_VARIABLE: &str = "MESH_CAS_CRASH_ROOT";
/// The last step the child performs before it stops and waits to be killed.
const STOP_VARIABLE: &str = "MESH_CAS_CRASH_STOP";
/// The seed for the child's chunk bytes.
const SEED_VARIABLE: &str = "MESH_CAS_CRASH_SEED";
/// The length of the child's chunk in bytes.
const LENGTH_VARIABLE: &str = "MESH_CAS_CRASH_LENGTH";
/// The interior point the child's filesystem stops at, when it stops at one.
const PAUSE_VARIABLE: &str = "MESH_CAS_CRASH_PAUSE";

/// The value of [`STOP_VARIABLE`] meaning "stop before performing any step".
const STOP_BEFORE_ANY: &str = "none";
/// The value of [`STOP_VARIABLE`] meaning "run the whole promotion without stopping".
const STOP_NOWHERE: &str = "all";

/// The name of the child entry point, as libtest knows it.
const CHILD_TEST: &str = "crash_child";

/// What the child prints when it has reached its stop point.
///
/// The parent looks for this as a *substring*, not as a line prefix: libtest writes
/// `test crash_child ... ` with no trailing newline before the test body runs, so the child's first
/// line of output is glued to it. A prefix match here silently never fires, the parent blocks until
/// the child gives up, and the whole suite fails with a misleading message — which is exactly what
/// it did before this was written down.
const READY_TOKEN: &str = "MESH-CAS-READY";

/// The bytes a child promotes, from a seed and a length.
fn chunk(seed: u64, length: usize) -> Vec<u8> {
    Bytes::seeded(seed).take(length)
}

// ---------------------------------------------------------------------------------------------
// The child
// ---------------------------------------------------------------------------------------------

/// The child process's body. Inert unless the parent set [`ROOT_VARIABLE`].
///
/// Running normally — as one of the suite's tests — it asserts its own inertness, so this is not a
/// test that passes by doing nothing without saying so.
#[test]
fn crash_child() {
    let Ok(root) = std::env::var(ROOT_VARIABLE) else {
        assert!(
            std::env::var(STOP_VARIABLE).is_err(),
            "the crash child was told where to stop but not where to work; the parent sets both \
             or neither"
        );
        return;
    };

    let stop = std::env::var(STOP_VARIABLE).expect("the parent sets the stop point");
    let seed: u64 = std::env::var(SEED_VARIABLE)
        .expect("the parent sets the seed")
        .parse()
        .expect("the seed is a number");
    let length: usize = std::env::var(LENGTH_VARIABLE)
        .expect("the parent sets the length")
        .parse()
        .expect("the length is a number");

    let bytes = chunk(seed, length);

    // The interior campaign: the store runs on a filesystem that stops inside one named operation,
    // announces from there, and never returns. Everything the promotion did before that point is on
    // disk exactly as the real code left it, and nothing after it has happened.
    if let Ok(pause) = std::env::var(PAUSE_VARIABLE) {
        let point = PausePoint::from_name(&pause).expect("the pause point names an interior point");
        let store = Cas::<PausingFs, Blake3>::with_filesystem(&root, PausingFs::new(point))
            .expect("the store opens");
        // `promote` is expected never to return: the filesystem beneath it stops. If it does
        // return, the point was not reached, and the parent's `assert_killed` turns the child's
        // self-exit into a named failure rather than a silent pass.
        let _ = store.promote(bytes);
        std::process::exit(96);
    }

    let store = Cas::open(&root).expect("the store opens");

    if stop == STOP_BEFORE_ANY {
        announce_ready();
        wait_to_be_killed();
    }

    if stop == STOP_NOWHERE {
        let _ = store.promote(bytes);
        announce_ready();
        wait_to_be_killed();
    }

    let step = PromotionStep::from_name(&stop).expect("the stop point names a step");
    let mut promotion = store.begin_promotion(bytes);
    promotion
        .run_through(step)
        .expect("the promotion reaches its stop point");
    announce_ready();
    wait_to_be_killed();
}

// ---------------------------------------------------------------------------------------------
// The interior points, and the filesystem that stops at one
// ---------------------------------------------------------------------------------------------

/// Which side of the `rename` a point falls on. The rename is the instant a chunk becomes visible,
/// so this is the only classification the invariant cares about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    /// The chunk cannot be in the store, because the rename that reveals it has not run.
    BeforeTheRename,
    /// The chunk is in the store, whole, because the rename that reveals it has returned.
    AfterTheRename,
}

/// A point *inside* one of the six steps, named by the work done rather than by elapsed time.
///
/// The enumeration is the campaign's schedule. Adding a point adds a child; the campaign asserts
/// its own length, so the count in `ROUNDS` cannot drift away from this list silently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PausePoint {
    /// `stage` entered and the scratch file not yet created — step 1, before its first byte.
    StageBeforeCreate,
    /// `stage` with `eighths`/8 of the bytes written into an open, unflushed file — mid-write.
    StagePartial(u32),
    /// `stage` with every byte written and the call not yet returned — step 1's last instant.
    StageWritten,
    /// `sync_file` on the scratch file entered — inside step 2, the flush in flight.
    FlushEntered,
    /// `sync_file` on the scratch file returned — the bytes are durable and nothing names them.
    FlushReturned,
    /// The read-back in step 3 returned and the digest has not been compared — mid-verify.
    VerifyRead,
    /// Half of the arrival record appended — a torn journal line, which step 4 can produce.
    ArrivalTorn,
    /// The whole arrival record appended and not yet synced — inside step 4.
    ArrivalAppended,
    /// `rename` entered — the last instant at which the chunk is not there.
    RenameEntered,
    /// `rename` returned and the directory not yet synced — the first instant at which it is.
    RenameReturned,
    /// The directory sync that follows the rename, in flight — inside step 6.
    SyncDirectoryEntered,
}

impl PausePoint {
    /// The campaign's schedule, in the order a promotion reaches the points.
    ///
    /// Sixteen children, the same count the timed campaign ran, and every one of them named.
    /// Fourteen fall before the rename and two after it, so both sides are covered by construction
    /// rather than by a tally that a busy machine can empty. The six `StagePartial` fractions are
    /// what a timed kill was aiming at when it landed mid-write: a scratch file holding some of the
    /// bytes, open, unflushed, never closed.
    const ORDER: [Self; 16] = [
        Self::StageBeforeCreate,
        Self::StagePartial(1),
        Self::StagePartial(2),
        Self::StagePartial(3),
        Self::StagePartial(4),
        Self::StagePartial(5),
        Self::StagePartial(6),
        Self::StageWritten,
        Self::FlushEntered,
        Self::FlushReturned,
        Self::VerifyRead,
        Self::ArrivalTorn,
        Self::ArrivalAppended,
        Self::RenameEntered,
        Self::RenameReturned,
        Self::SyncDirectoryEntered,
    ];

    fn name(self) -> String {
        match self {
            Self::StageBeforeCreate => "stage-before-create".to_owned(),
            Self::StagePartial(eighths) => format!("stage-{eighths}-eighths"),
            Self::StageWritten => "stage-written".to_owned(),
            Self::FlushEntered => "flush-entered".to_owned(),
            Self::FlushReturned => "flush-returned".to_owned(),
            Self::VerifyRead => "verify-read".to_owned(),
            Self::ArrivalTorn => "arrival-torn".to_owned(),
            Self::ArrivalAppended => "arrival-appended".to_owned(),
            Self::RenameEntered => "rename-entered".to_owned(),
            Self::RenameReturned => "rename-returned".to_owned(),
            Self::SyncDirectoryEntered => "sync-directory-entered".to_owned(),
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Self::ORDER.into_iter().find(|point| point.name() == name)
    }

    /// Which side of the rename the child is stopped on. This is what makes the campaign evidence
    /// rather than a tally: every round asserts its own side, so a chunk that became visible before
    /// the rename ran fails the round that found it and names the point.
    fn side(self) -> Side {
        match self {
            Self::RenameReturned | Self::SyncDirectoryEntered => Side::AfterTheRename,
            _ => Side::BeforeTheRename,
        }
    }

    /// How many staged files a kill here leaves behind, and why.
    ///
    /// Not a detail: it is what proves the promotion's own cleanup did not run. A destructor that
    /// tidied up would leave zero here at every point before the rename, and the campaign would be
    /// testing an orderly shutdown instead of a crash.
    fn expected_scratch(self) -> usize {
        match self {
            // The file does not exist yet.
            Self::StageBeforeCreate => 0,
            // The rename consumed the staged file.
            Self::RenameReturned | Self::SyncDirectoryEntered => 0,
            _ => 1,
        }
    }
}

/// The real filesystem, stopped at one named interior point.
///
/// Every operation is performed by [`StdFs`] except the one the point names, and that one is
/// performed *up to* the point and then abandoned mid-call: the process announces and blocks until
/// the parent kills it. Nothing unwinds, so the state on disk is the state a crash leaves.
#[derive(Debug)]
struct PausingFs {
    inner: StdFs,
    point: PausePoint,
    /// Whether the rename has returned, which is what tells the directory sync that follows the
    /// rename apart from the fanout syncs `create_chunk_directory` issues before it.
    renamed: AtomicBool,
}

impl PausingFs {
    fn new(point: PausePoint) -> Self {
        Self {
            inner: StdFs,
            point,
            renamed: AtomicBool::new(false),
        }
    }

    fn at(&self, point: PausePoint) -> bool {
        self.point == point
    }

    fn is_scratch(path: &Path) -> bool {
        path.components()
            .any(|part| part.as_os_str() == SCRATCH_DIRECTORY_NAME)
    }

    fn is_chunk(path: &Path) -> bool {
        path.components()
            .any(|part| part.as_os_str() == CHUNKS_DIRECTORY_NAME)
    }

    fn is_journal(path: &Path) -> bool {
        path.file_name()
            .is_some_and(|name| name == ARRIVAL_JOURNAL_FILE_NAME)
    }
}

impl DurableFs for PausingFs {
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.inner.create_dir_all(path)
    }

    fn stage(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        if !Self::is_scratch(path) {
            return self.inner.stage(path, bytes);
        }
        if self.at(PausePoint::StageBeforeCreate) {
            announce_and_block();
        }
        if let PausePoint::StagePartial(eighths) = self.point {
            // The real `create_new` open, then a partial write into it, then the process stops with
            // the file still open and nothing flushed. This is the state a timed kill was trying to
            // sample: some of the bytes on disk under a scratch name, no sync, no destructor.
            let cut = bytes.len() / 8 * eighths as usize;
            let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
            file.write_all(&bytes[..cut])?;
            announce_and_block();
        }
        self.inner.stage(path, bytes)?;
        if self.at(PausePoint::StageWritten) {
            announce_and_block();
        }
        Ok(())
    }

    fn append(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        if !Self::is_journal(path) {
            return self.inner.append(path, bytes);
        }
        if self.at(PausePoint::ArrivalTorn) {
            // Half a record. `append` is one `write_all` on an `O_APPEND` descriptor, and a write
            // can be partial, so a torn last line is a state a real crash can leave. The journal
            // reader parses complete lines only, which is what this point checks it still does.
            self.inner.append(path, &bytes[..bytes.len() / 2])?;
            announce_and_block();
        }
        self.inner.append(path, bytes)?;
        if self.at(PausePoint::ArrivalAppended) {
            announce_and_block();
        }
        Ok(())
    }

    fn sync_file(&self, path: &Path) -> io::Result<()> {
        if !Self::is_scratch(path) {
            return self.inner.sync_file(path);
        }
        if self.at(PausePoint::FlushEntered) {
            announce_and_block();
        }
        self.inner.sync_file(path)?;
        if self.at(PausePoint::FlushReturned) {
            announce_and_block();
        }
        Ok(())
    }

    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        if self.at(PausePoint::SyncDirectoryEntered)
            && Self::is_chunk(path)
            && self.renamed.load(Ordering::SeqCst)
        {
            announce_and_block();
        }
        self.inner.sync_dir(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        if self.at(PausePoint::RenameEntered) && Self::is_chunk(to) {
            announce_and_block();
        }
        self.inner.rename(from, to)?;
        if Self::is_chunk(to) {
            self.renamed.store(true, Ordering::SeqCst);
            if self.at(PausePoint::RenameReturned) {
                announce_and_block();
            }
        }
        Ok(())
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        let bytes = self.inner.read(path)?;
        if self.at(PausePoint::VerifyRead) && Self::is_scratch(path) {
            announce_and_block();
        }
        Ok(bytes)
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

/// Announce that the child is stopped at its point, and stay there until it is killed.
fn announce_and_block() -> ! {
    announce_ready();
    wait_to_be_killed();
}

/// Tell the parent the child has reached its stop point.
fn announce_ready() {
    println!("{READY_TOKEN}");
    // `println!` writes into a line-buffered stream that a `SIGKILL` would discard, so the flush
    // is what makes the handshake a handshake.
    use std::io::Write;
    std::io::stdout().flush().expect("stdout flushes");
}

/// Block until the parent kills this process, or give up so an orphan cannot outlive the suite.
fn wait_to_be_killed() -> ! {
    for _ in 0..600 {
        std::thread::sleep(Duration::from_millis(100));
    }
    // Reached only if the parent died first. Exiting non-zero rather than looping forever keeps a
    // stray child from outliving the test run.
    std::process::exit(97);
}

// ---------------------------------------------------------------------------------------------
// The parent
// ---------------------------------------------------------------------------------------------

/// Spawn a child, wait for it to reach `stop`, kill it, and return how it died.
fn kill_child_at(root: &Path, stop: &str, seed: u64, length: usize) -> ExitStatus {
    spawn_and_kill(root, stop, seed, length, None)
}

/// Spawn a child whose filesystem stops at `point`, kill it there, and return how it died.
///
/// The only scheduling this campaign does. There is no delay and no calibration: the parent waits
/// for the child's handshake, which the child sends from inside the operation the point names.
fn kill_child_inside(root: &Path, point: PausePoint, seed: u64, length: usize) -> ExitStatus {
    spawn_and_kill(root, STOP_NOWHERE, seed, length, Some(point))
}

/// Spawn the child, wait for its handshake, kill it, and return how it died.
fn spawn_and_kill(
    root: &Path,
    stop: &str,
    seed: u64,
    length: usize,
    pause: Option<PausePoint>,
) -> ExitStatus {
    let binary = std::env::current_exe().expect("the test binary knows its own path");
    let mut command = Command::new(binary);
    command
        .args([CHILD_TEST, "--exact", "--nocapture", "--test-threads=1"])
        .env(ROOT_VARIABLE, root)
        .env(STOP_VARIABLE, stop)
        .env(SEED_VARIABLE, seed.to_string())
        .env(LENGTH_VARIABLE, length.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    let described = match pause {
        Some(point) => {
            command.env(PAUSE_VARIABLE, point.name());
            point.name()
        }
        None => stop.to_owned(),
    };
    let mut child = command.spawn().expect("the child test binary starts");

    let stdout = child.stdout.take().expect("the child's stdout is piped");
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .expect("the child's output is readable");
        if read == 0 {
            let _ = child.kill();
            let status = child.wait().expect("the child is reaped");
            panic!(
                "the child exited ({status:?}) before reaching {described:?}; its stderr is above"
            );
        }
        if line.contains(READY_TOKEN) {
            break;
        }
    }

    sigkill(child.id());
    child.wait().expect("the child is reaped")
}

/// Send `SIGKILL` through `/bin/kill`, which is how this is done without an `unsafe` block or a
/// dependency on `libc`. A failure to send is a failure of the test, never a silent skip.
fn sigkill(process: u32) {
    let output = Command::new("/bin/kill")
        .args(["-9", &process.to_string()])
        .output()
        .expect("/bin/kill runs; this suite cannot test a crash without it");
    assert!(
        output.status.success(),
        "/bin/kill -9 {process} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Assert the child died by signal rather than exiting on its own.
///
/// Without this, a child that panicked on startup would leave an empty store and every "the chunk
/// is absent" assertion below would pass for the wrong reason.
fn assert_killed(status: ExitStatus, context: &str) {
    assert!(
        status.code().is_none(),
        "the child at {context} exited with {status:?} instead of being killed by a signal, so \
         nothing about a crash was tested"
    );
}

/// The invariant, checked against whatever a killed child left behind: the chunk is either absent
/// or complete, and never anything else.
///
/// Returns the verdict rather than asserting it, so that
/// [`a_planted_partial_chunk_is_still_rejected`] can prove this check can fail. A property nobody
/// has watched fail is a property nobody has tested.
fn all_or_nothing_verdict(
    root: &Path,
    digest: &Digest32,
    expected: &[u8],
    context: &str,
) -> Result<Side, String> {
    let store = Cas::open(root)
        .map_err(|error| format!("{context}: the store did not reopen after the crash: {error}"))?;
    if store.contains(digest) {
        let bytes = store
            .read(digest)
            .map_err(|error| format!("{context}: the chunk is present but unreadable: {error}"))?;
        if bytes != expected {
            return Err(format!(
                "{context}: the chunk is present with the wrong bytes, which is the failure this \
                 crate exists to prevent"
            ));
        }
        // Ordering guarantee: the arrival record is durable before the rename that reveals the
        // chunk, so a chunk that became visible was recorded first. This is a candidate rather than
        // merely recorded because nothing in these tests retains anything — a chunk a transaction
        // has committed a reference to is deliberately *not* a candidate, and asserting otherwise
        // in general would be wrong.
        let candidates = store
            .journal()
            .candidates()
            .map_err(|error| format!("{context}: the arrival journal is unreadable: {error}"))?;
        if !candidates.contains(digest) {
            return Err(format!(
                "{context}: the chunk is visible but the arrival journal does not list it, so it \
                 would leak"
            ));
        }
        return Ok(Side::AfterTheRename);
    }
    match store.read(digest) {
        Err(mesh_cas::CasError::Absent { .. }) => {}
        other => {
            return Err(format!(
                "{context}: the chunk is not present but reading it gave {other:?}"
            ))
        }
    }
    // The journal is read on this side too. A kill inside the arrival append can leave a torn last
    // record, and a reader that choked on one would turn a survivable crash into an unopenable
    // store — which is what `PausePoint::ArrivalTorn` exists to find.
    store
        .journal()
        .candidates()
        .map_err(|error| format!("{context}: the arrival journal is unreadable: {error}"))?;
    Ok(Side::BeforeTheRename)
}

/// The invariant as an assertion, which is how every campaign in this file uses it.
fn assert_all_or_nothing(root: &Path, digest: &Digest32, expected: &[u8], context: &str) -> Side {
    match all_or_nothing_verdict(root, digest, expected, context) {
        Ok(side) => side,
        Err(problem) => panic!("{problem}"),
    }
}

// ---------------------------------------------------------------------------------------------
// Every step boundary
// ---------------------------------------------------------------------------------------------

const SMALL: usize = 64 * 1024;

#[test]
fn killing_before_any_step_leaves_the_store_untouched() {
    let root = TempRoot::new("before-any");
    let bytes = chunk(1, SMALL);
    let digest = Blake3::digest_bytes(&bytes);

    let status = kill_child_at(root.path(), STOP_BEFORE_ANY, 1, SMALL);
    assert_killed(status, "before any step");

    let store = Cas::open(root.path()).expect("the store reopens");
    assert!(!store.contains(&digest), "nothing was promoted");
    assert_eq!(
        store.discard_scratch().expect("scratch is readable"),
        0,
        "nothing was staged"
    );
    assert!(
        store
            .journal()
            .candidates()
            .expect("the journal is readable")
            .is_empty(),
        "nothing was recorded"
    );
    assert_all_or_nothing(root.path(), &digest, &bytes, "before any step");
}

#[test]
fn killing_after_staging_leaves_a_discardable_scratch_file_and_no_chunk() {
    let root = TempRoot::new("after-stage");
    let bytes = chunk(2, SMALL);
    let digest = Blake3::digest_bytes(&bytes);

    let status = kill_child_at(root.path(), PromotionStep::Stage.name(), 2, SMALL);
    assert_killed(status, "after staging");

    let store = Cas::open(root.path()).expect("the store reopens");
    assert!(
        !store.contains(&digest),
        "staged bytes are not addressable; only the rename makes a chunk"
    );
    assert!(
        store
            .journal()
            .candidates()
            .expect("the journal is readable")
            .is_empty(),
        "nothing arrived, so nothing was recorded as having arrived"
    );
    assert_all_or_nothing(root.path(), &digest, &bytes, "after staging");
    assert_eq!(
        store.discard_scratch().expect("scratch is readable"),
        1,
        "exactly one staged file is left, and it is discardable — which also proves the \
         promotion's destructor did not run, so this really was a crash"
    );
    assert_eq!(
        store.discard_scratch().expect("scratch is readable"),
        0,
        "discarding scratch is idempotent"
    );
}

#[test]
fn killing_after_flushing_leaves_a_discardable_scratch_file_and_no_chunk() {
    let root = TempRoot::new("after-flush");
    let bytes = chunk(3, SMALL);
    let digest = Blake3::digest_bytes(&bytes);

    let status = kill_child_at(root.path(), PromotionStep::Flush.name(), 3, SMALL);
    assert_killed(status, "after flushing");

    let store = Cas::open(root.path()).expect("the store reopens");
    assert!(
        !store.contains(&digest),
        "durable bytes are still not a chunk"
    );
    assert_all_or_nothing(root.path(), &digest, &bytes, "after flushing");
    assert_eq!(store.discard_scratch().expect("scratch is readable"), 1);
}

#[test]
fn killing_after_verifying_leaves_a_discardable_scratch_file_and_no_chunk() {
    let root = TempRoot::new("after-verify");
    let bytes = chunk(4, SMALL);
    let digest = Blake3::digest_bytes(&bytes);

    let status = kill_child_at(root.path(), PromotionStep::Verify.name(), 4, SMALL);
    assert_killed(status, "after verifying");

    let store = Cas::open(root.path()).expect("the store reopens");
    assert!(
        !store.contains(&digest),
        "verified bytes are still not a chunk until they are renamed"
    );
    assert!(store
        .journal()
        .candidates()
        .expect("the journal is readable")
        .is_empty());
    assert_all_or_nothing(root.path(), &digest, &bytes, "after verifying");
    assert_eq!(store.discard_scratch().expect("scratch is readable"), 1);
}

#[test]
fn killing_after_recording_arrival_leaves_a_candidate_for_a_chunk_that_never_arrived() {
    let root = TempRoot::new("after-record");
    let bytes = chunk(5, SMALL);
    let digest = Blake3::digest_bytes(&bytes);

    let status = kill_child_at(root.path(), PromotionStep::RecordArrival.name(), 5, SMALL);
    assert_killed(status, "after recording arrival");

    let store = Cas::open(root.path()).expect("the store reopens");
    assert!(!store.contains(&digest), "the rename had not happened");
    assert_eq!(
        store
            .journal()
            .candidates()
            .expect("the journal is readable"),
        vec![digest],
        "the arrival was recorded before the chunk existed, which is the ordering that makes a \
         crashed promotion findable rather than leaked"
    );
    // The record is ahead of reality here, and that direction is the safe one: the collector will
    // ask about a chunk that is not there, find nothing to delete, and move on.
    assert_all_or_nothing(root.path(), &digest, &bytes, "after recording arrival");
    assert_eq!(store.discard_scratch().expect("scratch is readable"), 1);
}

#[test]
fn killing_after_linking_leaves_a_whole_readable_chunk() {
    let root = TempRoot::new("after-link");
    let bytes = chunk(6, SMALL);
    let digest = Blake3::digest_bytes(&bytes);

    let status = kill_child_at(root.path(), PromotionStep::Link.name(), 6, SMALL);
    assert_killed(status, "after linking");

    let store = Cas::open(root.path()).expect("the store reopens");
    assert!(store.contains(&digest), "the rename made the chunk visible");
    assert_eq!(
        store.read(&digest).expect("the chunk verifies"),
        bytes,
        "a chunk that is visible is whole, even though the directory sync never ran"
    );
    assert_eq!(
        store
            .journal()
            .candidates()
            .expect("the journal is readable"),
        vec![digest]
    );
    assert_eq!(
        store.discard_scratch().expect("scratch is readable"),
        0,
        "the rename consumed the staged file"
    );
    assert_all_or_nothing(root.path(), &digest, &bytes, "after linking");
}

#[test]
fn killing_after_the_directory_sync_leaves_a_whole_readable_chunk() {
    let root = TempRoot::new("after-sync");
    let bytes = chunk(7, SMALL);
    let digest = Blake3::digest_bytes(&bytes);

    let status = kill_child_at(root.path(), PromotionStep::SyncDirectory.name(), 7, SMALL);
    assert_killed(status, "after the directory sync");

    let store = Cas::open(root.path()).expect("the store reopens");
    assert!(store.contains(&digest));
    assert_eq!(store.read(&digest).expect("the chunk verifies"), bytes);
    assert_eq!(store.discard_scratch().expect("scratch is readable"), 0);
    assert_all_or_nothing(root.path(), &digest, &bytes, "after the directory sync");
}

/// Every step is covered above. This fails the day a seventh is added, so the coverage claim
/// cannot go stale silently.
#[test]
fn every_promotion_step_has_a_kill_test() {
    let covered = [
        PromotionStep::Stage,
        PromotionStep::Flush,
        PromotionStep::Verify,
        PromotionStep::RecordArrival,
        PromotionStep::Link,
        PromotionStep::SyncDirectory,
    ];
    assert_eq!(
        PromotionStep::ORDER.to_vec(),
        covered.to_vec(),
        "a promotion step exists that no test in this file kills at"
    );
}

// ---------------------------------------------------------------------------------------------
// Kills inside the steps rather than between them
// ---------------------------------------------------------------------------------------------

/// A campaign has to straddle the rename to be evidence. The old check counted corpses and could
/// therefore be emptied by a busy machine; this one is a statement about the *schedule*, which is a
/// constant in this file, so it fails only if somebody edits the schedule down to one side.
///
/// Separated from the campaign so it can be watched failing —
/// [`the_straddle_check_still_fails_when_the_schedule_covers_one_side`] does exactly that. That is
/// the guarantee the report asked for: the repair must not be "stop asserting the straddle".
fn straddle_verdict(schedule: &[PausePoint]) -> Result<(usize, usize), String> {
    let after = schedule
        .iter()
        .filter(|point| point.side() == Side::AfterTheRename)
        .count();
    let before = schedule.len() - after;
    if before == 0 || after == 0 {
        return Err(format!(
            "the schedule places all {} of its kills on one side of the rename ({before} before, \
             {after} after), so the campaign would pass without exercising the boundary",
            schedule.len()
        ));
    }
    Ok((before, after))
}

/// Sixteen children killed inside the promotion, each at a named point, each asserting the
/// all-or-nothing invariant and the exact residue its point implies.
///
/// Nothing here is timed. The name kept its shape so a lane bisecting an old failure still finds
/// the test, but the kills are no longer randomised over a measured span: they are enumerated over
/// [`PausePoint::ORDER`], and the verdict is a function of the tree rather than of the machine.
#[test]
fn randomised_kills_never_leave_a_partial_chunk() {
    const LARGE: usize = 2 * 1024 * 1024;

    let (before, after) =
        straddle_verdict(&PausePoint::ORDER).unwrap_or_else(|problem| panic!("{problem}"));
    assert_eq!(
        PausePoint::ORDER.len(),
        16,
        "the interior campaign is sixteen children; deleting one is not a way to make the suite \
         green"
    );

    let mut seen_present = 0;
    let mut seen_absent = 0;

    for (round, point) in PausePoint::ORDER.into_iter().enumerate() {
        let root = TempRoot::new(&format!("inside-{}", point.name()));
        let seed = 1_000 + round as u64;
        let bytes = chunk(seed, LARGE);
        let digest = Blake3::digest_bytes(&bytes);

        let status = kill_child_inside(root.path(), point, seed, LARGE);
        let context = format!("round {round} killed at {}", point.name());
        assert_killed(status, &context);

        let observed = assert_all_or_nothing(root.path(), &digest, &bytes, &context);
        assert_eq!(
            observed,
            point.side(),
            "{context}: the child was stopped {:?} and the store disagrees. A chunk visible before \
             the rename returned, or absent after it did, is an ordering failure and not a \
             scheduling one — the child was blocked at this point when it was killed, so it cannot \
             have run ahead",
            point.side()
        );
        match observed {
            Side::AfterTheRename => seen_present += 1,
            Side::BeforeTheRename => seen_absent += 1,
        }

        let store = Cas::open(root.path()).expect("the store reopens");
        assert_eq!(
            store.discard_scratch().expect("scratch is readable"),
            point.expected_scratch(),
            "{context}: the staged files left behind are not what a crash at this point leaves. \
             Fewer than expected means the promotion's destructor ran, so this was a shutdown \
             rather than a crash"
        );
        assert_eq!(
            store.discard_scratch().expect("scratch is readable"),
            0,
            "{context}: scratch is empty after being discarded"
        );
    }

    // Counts, not timings. Both of these are decided by `PausePoint::side`, which is decided by
    // where the child was blocked, so a loaded machine changes how long this takes and nothing else.
    assert_eq!(
        (seen_absent, seen_present),
        (before, after),
        "the corpses did not fall where the schedule says they must: {seen_absent} before the \
         rename and {seen_present} after it, against a schedule of {before} and {after}"
    );
}

/// The straddle check can still fail. Run over a schedule that covers one side, it says so.
///
/// Without this the repair would be indistinguishable from deleting the assertion, which is what
/// `01KZEBZW17SGQ2JR9GHGBND5R7` put out of scope in as many words.
#[test]
fn the_straddle_check_still_fails_when_the_schedule_covers_one_side() {
    let one_sided: Vec<PausePoint> = PausePoint::ORDER
        .into_iter()
        .filter(|point| point.side() == Side::BeforeTheRename)
        .collect();
    let problem = straddle_verdict(&one_sided).expect_err("a one-sided schedule is not evidence");
    assert!(problem.contains("one side of the rename"), "{problem}");

    let other_side: Vec<PausePoint> = PausePoint::ORDER
        .into_iter()
        .filter(|point| point.side() == Side::AfterTheRename)
        .collect();
    straddle_verdict(&other_side).expect_err("the other one-sided schedule is not evidence either");

    straddle_verdict(&PausePoint::ORDER).expect("the campaign's own schedule straddles the rename");
}

/// The invariant can still fail. A chunk file holding the wrong bytes is rejected.
///
/// This is the negative control for the campaign above: every round of it calls
/// [`all_or_nothing_verdict`], and a check that has only ever been observed to pass is not evidence
/// that a partial chunk would be caught. The partial chunk is planted rather than produced, because
/// the whole claim of this crate is that a crash cannot produce one.
#[test]
fn a_planted_partial_chunk_is_still_rejected() {
    let root = TempRoot::new("planted-partial");
    let bytes = chunk(4_242, SMALL);
    let digest = Blake3::digest_bytes(&bytes);

    let store = Cas::open(root.path()).expect("the store opens");
    store.promote(bytes.clone()).expect("the chunk promotes");
    all_or_nothing_verdict(root.path(), &digest, &bytes, "before planting")
        .expect("a whole chunk satisfies the invariant");

    // Truncate the chunk in place: the name still says these are the bytes, and they are not.
    let path = store.layout().chunk_path(&digest);
    std::fs::write(&path, &bytes[..bytes.len() / 2]).expect("the chunk file is writable");

    let problem = all_or_nothing_verdict(root.path(), &digest, &bytes, "planted partial")
        .expect_err("a partial chunk is exactly what this invariant forbids");
    assert!(
        problem.contains("planted partial"),
        "the verdict names the round it came from: {problem}"
    );
}

/// Every interior point is reachable, distinctly named, and round-trips through the child's
/// environment variable. A point the child cannot be told about is a point that never runs.
#[test]
fn every_interior_point_is_nameable_and_distinct() {
    let mut names: Vec<String> = PausePoint::ORDER.iter().map(|point| point.name()).collect();
    let total = names.len();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), total, "two interior points share a name");

    for point in PausePoint::ORDER {
        assert_eq!(
            PausePoint::from_name(&point.name()),
            Some(point),
            "{} cannot be named on the child's command line, so it cannot be killed at",
            point.name()
        );
    }
    assert_eq!(PausePoint::from_name("no-such-point"), None);
}
