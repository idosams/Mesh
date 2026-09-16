//! Killing a real process at every one of plan §6.3's eleven steps, and checking what is left.
//!
//! # The claim under test
//!
//! > **Zero acknowledged-checkpoint loss.** If a user was told "saved privately", the checkpoint is
//! > there after the crash. Always. And the other direction: nothing durable ever references
//! > content that is not durable.
//!
//! Both halves are asserted after every kill in this file, not only at the boundaries where they
//! are interesting, because a durability claim that holds at the points somebody thought to check
//! is not a durability claim.
//!
//! # Why a child process and not a mocked interruption
//!
//! An interruption modelled by returning `Err` tests the error path — worth testing, and the
//! disk-full campaign at the bottom of this file does exactly that — but it is not an
//! interruption: destructors run, buffers flush, and the cleanup a crash skips is the cleanup that
//! happens. So these tests spawn this same test binary as a child, drive it to a chosen step, and
//! `SIGKILL` it. The machinery for that lives in `tests/support/crash.rs`, shared with
//! `tests/recovery.rs` and documented there — including why the `sqlite3` grandchild has to die
//! too. `ExitStatus::code` returning `None` is asserted every time, so a child that exited on its
//! own can never be mistaken for one that was killed.
//!
//! # How "the user was told" is observed across a process death
//!
//! The child writes an acknowledgement marker file the instant
//! [`mesh_store::DurableCommit::acknowledgement`] stops being `None`, and `fsync`s it. The parent's
//! central invariant is then a single implication it can check on a corpse:
//!
//! ```text
//! marker present  ⟹  the database holds the whole checkpoint
//! ```
//!
//! The marker is deliberately written *before* the parent could ever kill the child at step 11, so
//! it errs toward existing. A bug that acknowledged early would make the marker appear at a step
//! where the database is still empty, and every kill point from 1 to 8 would fail.
//!
//! # The three campaigns
//!
//! * **Every step boundary** — eleven steps plus the point before any of them, each with the exact
//!   residue plan §6.3 names, cross-checked against
//!   [`mesh_store::SequenceStep::residue_if_killed_after`] so the harness and the code cannot drift
//!   apart quietly.
//! * **Kills inside the transaction** — the boundary campaign can only kill *between* steps, and
//!   steps 5 to 8 compose the transaction in memory. So a second campaign kills twelve children
//!   *inside* the batch `sqlite3` is executing, each at a named statement offset, and asserts
//!   all-or-nothing. That is what turns "SQLite's transaction is the atom" from a citation into a
//!   measurement on this machine.
//! * **Disk full at every step** — an injected `ENOSPC` at each of plan §6.3's failure-capable
//!   steps, in-process against a real database, asserting nothing is acknowledged and nothing
//!   durable moved.
//!
//! # Why the interior campaign is no longer scheduled by a clock (`01KZERXN1BC2FEDNNXNBKTNY7E`)
//!
//! It used to place its twelve kills at fractions of a span *measured once, on the machine,
//! immediately before the campaign ran*. That schedule was valid only while the machine's load
//! after the calibration matched its load during it, and under `cargo nextest run --workspace`
//! nothing makes that true. Two failures in eleven whole-suite runs came from it, and the worse of
//! the two did not even print the designed assertion: the kill, aimed at the interior of the
//! transaction, landed during `Store::open`'s migrations instead, and the parent reported
//! `no such table: operation` — one word away from the sentence this whole file exists to make
//! impossible.
//!
//! The clock is gone. The child now runs the transaction through [`PausingExecutor`], which feeds
//! `sqlite3` a *prefix* of the batch on a live connection, waits for the engine to answer that it
//! has executed exactly that prefix, and then announces and blocks. The parent kills at the
//! handshake, and the grandchild engine dies with it while its transaction is open and uncommitted.
//! The schedule is a list of statement offsets — a count, not a duration — so a busy machine
//! changes how long the campaign takes and nothing about what it asserts.
//!
//! The interruption is not weaker for being scheduled. The engine is killed with the transaction
//! genuinely open: the prefix has been executed, `COMMIT` has not been sent, and no destructor and
//! no rollback ran in either process. A timed kill had to *hope* it landed there.
//!
//! # What a `SIGKILL` proves, and what it does not
//!
//! It covers every failure in which the machine keeps running: process crashes, `OOM` kills, forced
//! termination. It does **not** prove behaviour under power loss, because the page cache survives a
//! killed process and does not survive a power cut. Power-loss durability rests on the platform
//! honouring `fsync`, on `rename` being atomic, and on SQLite's own `synchronous = NORMAL` under
//! WAL. Those are stated as assumptions, and reading this file as proof of them is reading more
//! than it says.

mod common;
mod support;

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use common::{sqlite3_binary, Sqlite3, SqliteError, TempDir};
use mesh_store::{
    Checkpoint, ChunkPromoter, ChunkSlice, CrashResidue, DurableCommit, ManifestRecord,
    OperationRecord, PeerRecord, RecordDigest, Row, SequenceStep, SqlExecutor, Store, Table,
};
use support::crash::{
    announce_ready, assert_killed, kill_child_at_ready, wait_to_be_killed, ChildSpec,
};
use support::promoter::{content_name, FilePromoter};

/// The workspace root the child saves into.
const ROOT_VARIABLE: &str = "MESH_STORE_CRASH_ROOT";
/// The last step the child performs before it stops and waits to be killed.
const STOP_VARIABLE: &str = "MESH_STORE_CRASH_STOP";
/// How many operations the child's checkpoint carries, which is how the transaction is made long
/// enough to be killed inside.
const WEIGHT_VARIABLE: &str = "MESH_STORE_CRASH_WEIGHT";
/// How many of the transaction's statements the child executes before it stops, as a percentage of
/// the batch. Absent unless the interior campaign is driving.
const PREFIX_VARIABLE: &str = "MESH_STORE_CRASH_PREFIX";

/// The value of [`STOP_VARIABLE`] meaning "stop before performing any step".
const STOP_BEFORE_ANY: &str = "none";
/// The value of [`STOP_VARIABLE`] meaning "run the whole sequence without stopping".
const STOP_NOWHERE: &str = "all";

/// The name of the child entry point, as libtest knows it.
const CHILD_TEST: &str = "crash_child";

/// The file whose existence means "the user was told the work is saved privately".
const ACKNOWLEDGED_MARKER: &str = "acknowledged";

/// The database file, a sibling of the content store as plan §6.2 lays it out.
const DATABASE: &str = "metadata.sqlite";

/// The bytes of the child's one chunk, from a seed.
fn chunk_bytes(seed: u8) -> Vec<u8> {
    (0..64 * 1024u32)
        .map(|index| (index as u8) ^ seed)
        .collect()
}

fn digest(seed: u8) -> RecordDigest {
    RecordDigest::from_bytes([seed; 32])
}

fn operation(index: u64) -> OperationRecord {
    let mut id = [0u8; 32];
    id[0..8].copy_from_slice(&index.to_be_bytes());
    id[31] = 0xA1;
    OperationRecord {
        id: RecordDigest::from_bytes(id),
        actor: digest(2),
        actor_sequence: index + 1,
        hlc_millis: 1_700_000_000_000,
        hlc_counter: index,
        policy_epoch: 1,
        session: mesh_store::no_session(),
        payload_digest: digest(9),
        parents: Vec::new(),
    }
}

/// The checkpoint the child saves: `weight` operations, a peer, and one manifest naming the chunk
/// that steps 1 to 4 promote.
///
/// The manifest is the load-bearing part. It is what makes "a durable head referencing a chunk"
/// something this test can look for, and it is why the sequence refuses to compose a transaction
/// whose chunks are not already in the store.
fn checkpoint(weight: u64, chunk: RecordDigest) -> Checkpoint {
    Checkpoint {
        manifests: vec![ManifestRecord {
            id: digest(20),
            byte_length: 64 * 1024,
            content_digest: digest(21),
            chunks: vec![ChunkSlice {
                digest: chunk,
                byte_offset: 0,
                byte_length: 64 * 1024,
            }],
        }],
        operations: (0..weight).map(operation).collect(),
        peers: vec![PeerRecord {
            peer: digest(30),
            joined_at: RecordDigest::from_bytes({
                let mut id = [0u8; 32];
                id[31] = 0xA1;
                id
            }),
        }],
        ..Checkpoint::default()
    }
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

    let root = PathBuf::from(root);
    let stop = std::env::var(STOP_VARIABLE).expect("the parent sets the stop point");
    let weight: u64 = std::env::var(WEIGHT_VARIABLE)
        .expect("the parent sets the weight")
        .parse()
        .expect("the weight is a number");

    let mut promoter = FilePromoter::open(&root);
    // Plan §6.3's "before step 4: temporary data is discarded", at startup, before the first save.
    promoter
        .discard_temporary()
        .expect("scratch from a previous crash is discardable");

    let database = Sqlite3::at(root.join(DATABASE));
    match std::env::var(PREFIX_VARIABLE) {
        // The interior campaign. Everything before step 9 runs on the real driver; step 9 runs on
        // an executor that stops part way through the batch and never returns.
        Ok(name) => {
            let prefix = Prefix::from_name(&name).expect("the prefix names a schedule point");
            let armed = Arc::new(AtomicBool::new(false));
            save(
                &root,
                &mut promoter,
                PausingExecutor::new(database, prefix, Arc::clone(&armed)),
                &stop,
                weight,
                Some(armed),
            );
        }
        Err(_) => save(&root, &mut promoter, database, &stop, weight, None),
    }

    announce_ready();
    wait_to_be_killed();
}

/// Everything the child does with the store, over whichever executor it was given.
///
/// Generic so the interior campaign can substitute [`PausingExecutor`] without a second copy of the
/// child; the boundary campaign and the interior campaign must save the *same* checkpoint through
/// the *same* sequence, or they stop being two views of one thing.
///
/// `armed`, when present, is set the instant step 8 returns — so the executor stops inside plan
/// §6.3's step 9 and nowhere else. Arming is not a nicety: `Store::open`'s migrations are also a
/// batch opening with `BEGIN IMMEDIATE;`, and an executor that matched on the text alone stopped in
/// the migrations instead, which is the accident this whole repair exists to stop happening by
/// chance.
fn save<E>(
    root: &Path,
    promoter: &mut FilePromoter,
    executor: E,
    stop: &str,
    weight: u64,
    armed: Option<Arc<AtomicBool>>,
) where
    E: SqlExecutor,
    E::Error: std::fmt::Debug,
{
    let mut store = Store::open(executor).expect("the database opens");
    let bytes = chunk_bytes(7);
    let chunk = content_name(&bytes);

    if stop == STOP_BEFORE_ANY {
        announce_ready();
        wait_to_be_killed();
    }

    let mut sequence =
        DurableCommit::new(&mut store, promoter, vec![bytes], checkpoint(weight, chunk));

    let stop_after = if stop == STOP_NOWHERE {
        None
    } else {
        Some(SequenceStep::from_name(stop).expect("the stop point names a step"))
    };

    let mut acknowledged = false;
    while let Some(performed) = sequence.step().expect("the sequence runs") {
        // The instant the acknowledgement exists, it is durably recorded — before anything else
        // happens, so a kill cannot land between "the user was told" and "the test can see it".
        if !acknowledged && sequence.acknowledgement().is_some() {
            record_acknowledgement(root);
            acknowledged = true;
        }
        if performed == STEP_BEFORE_THE_TRANSACTION {
            if let Some(armed) = &armed {
                armed.store(true, Ordering::SeqCst);
            }
        }
        if Some(performed) == stop_after {
            break;
        }
    }
}

/// The step after which the next `execute_batch` is the transaction and nothing else.
const STEP_BEFORE_THE_TRANSACTION: SequenceStep = SequenceStep::FillOutbox;

// ---------------------------------------------------------------------------------------------
// The schedule of the interior campaign, and the executor that keeps to it
// ---------------------------------------------------------------------------------------------

/// What the engine prints once it has executed exactly the prefix it was sent.
const ENGINE_MARKER: &str = "MESH-STORE-BATCH-EXECUTED";

/// The first statement of the transaction batch, and the discriminator that tells the transaction
/// apart from the migrations `Store::open` runs through the same executor.
const BATCH_OPENS_WITH: &str = "BEGIN IMMEDIATE;";

/// How much of the transaction the child executes before it stops.
///
/// A count of statements, never a duration. That is the whole repair: under load the engine takes
/// longer to reach the same statement, and the same statement is where the kill lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Prefix {
    /// `BEGIN IMMEDIATE;` plus this percentage of the statements between it and `COMMIT;`.
    Percent(u32),
    /// Every statement of the transaction except `COMMIT;` — the widest uncommitted state there is.
    AllButCommit,
    /// The whole batch, `COMMIT;` included. The one point at which the checkpoint survives.
    WholeBatch,
}

impl Prefix {
    /// The campaign's schedule: twelve children, the same count the timed campaign ran.
    ///
    /// Eleven of them stop with the transaction open and one stops just after it committed, so the
    /// campaign straddles the commit by construction rather than by luck.
    const ORDER: [Self; 12] = [
        Self::Percent(0),
        Self::Percent(10),
        Self::Percent(20),
        Self::Percent(30),
        Self::Percent(40),
        Self::Percent(50),
        Self::Percent(60),
        Self::Percent(70),
        Self::Percent(80),
        Self::Percent(90),
        Self::AllButCommit,
        Self::WholeBatch,
    ];

    fn name(self) -> String {
        match self {
            Self::Percent(percent) => format!("p{percent}"),
            Self::AllButCommit => "all-but-commit".to_owned(),
            Self::WholeBatch => "whole-batch".to_owned(),
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Self::ORDER.into_iter().find(|prefix| prefix.name() == name)
    }

    /// Whether the checkpoint survives a kill here. `COMMIT;` is the only thing that makes it so.
    fn commits(self) -> bool {
        self == Self::WholeBatch
    }

    /// How many of `statements` to execute. The first is `BEGIN IMMEDIATE;` and the last `COMMIT;`.
    fn cut(self, statements: &[String]) -> usize {
        let interior = statements.len().saturating_sub(2);
        match self {
            Self::Percent(percent) => 1 + interior * percent as usize / 100,
            Self::AllButCommit => statements.len() - 1,
            Self::WholeBatch => statements.len(),
        }
    }
}

/// The batch split into statements, in order.
///
/// [`mesh_store::CommitPlan`] joins statements with a newline and each one ends in `;`, so lines
/// are accumulated until one does. Splitting on `;` alone would be wrong the day a statement holds
/// one inside a literal; splitting on lines alone would be wrong the day one spans two.
fn split_statements(sql: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    for line in sql.lines() {
        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line);
        if line.trim_end().ends_with(';') {
            statements.push(std::mem::take(&mut current));
        }
    }
    if !current.trim().is_empty() {
        statements.push(current);
    }
    statements
}

/// The real driver, except that the transaction batch is executed only up to a named statement and
/// the process then stops there, inside it, waiting to be killed.
#[derive(Debug)]
struct PausingExecutor {
    inner: Sqlite3,
    prefix: Prefix,
    /// Set by the child once step 8 has returned. Until then every batch is passed straight
    /// through, which is what keeps this out of `Store::open`'s migrations.
    armed: Arc<AtomicBool>,
}

impl PausingExecutor {
    fn new(inner: Sqlite3, prefix: Prefix, armed: Arc<AtomicBool>) -> Self {
        Self {
            inner,
            prefix,
            armed,
        }
    }
}

impl SqlExecutor for PausingExecutor {
    type Error = SqliteError;

    fn execute_batch(&mut self, sql: &str) -> Result<(), Self::Error> {
        if !self.armed.load(Ordering::SeqCst) {
            // Migrations, the connection pragmas, the ledger. Nothing here is the transaction, and
            // stopping in one of them is the accident that produced `no such table: operation`.
            return self.inner.execute_batch(sql);
        }
        assert!(
            sql.starts_with(BATCH_OPENS_WITH),
            "the first batch after step 8 does not open with {BATCH_OPENS_WITH:?}, so this is not \
             the transaction and the schedule would stop in the wrong place"
        );
        let statements = split_statements(sql);
        assert!(
            statements
                .last()
                .is_some_and(|last| last.trim() == "COMMIT;"),
            "the transaction batch does not end in COMMIT; the schedule assumes it does"
        );
        let cut = self.prefix.cut(&statements);
        execute_prefix_then_block(self.inner.path(), &statements[..cut]);
    }

    fn read_table(&mut self, table: &Table) -> Result<Vec<Row>, Self::Error> {
        self.inner.read_table(table)
    }

    fn table_exists(&mut self, name: &str) -> Result<bool, Self::Error> {
        self.inner.table_exists(name)
    }
}

/// Feed `prefix` to a live `sqlite3`, wait for it to answer that it has executed all of it, then
/// announce and block with the transaction still open.
///
/// The engine is left mid-batch on purpose: its connection is alive, its transaction is open, and
/// its `stdin` is still held by this process, so nothing has told it to commit or to roll back.
/// When the parent kills this process it kills the engine too — before this one, because an orphan
/// is reparented and can no longer be found — and neither gets to clean up.
///
/// `clippy::zombie_processes` is exactly right about the shape and exactly wrong about this case:
/// waiting on the engine is the one thing that must not happen here, because a `wait` would mean
/// the batch had ended. The engine is reaped by the parent's kill, which
/// `support::crash::spawn_and_kill` sends to every child of this process before it sends one here.
#[allow(clippy::zombie_processes)]
fn execute_prefix_then_block(database: &Path, prefix: &[String]) -> ! {
    let mut engine = Command::new(sqlite3_binary())
        .arg("-batch")
        .arg("-bail")
        .arg("-noheader")
        .arg(database)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("sqlite3 starts; this suite cannot test a crash without it");

    let mut stdin = engine.stdin.take().expect("stdin is piped");
    // The same connection setup `Sqlite3::run` performs. Without it this would be a different
    // connection from the one the driver uses, and `foreign_keys` above all would be off.
    write!(
        stdin,
        ".output /dev/null\n{}\n.output stdout\n",
        mesh_store::connection_pragmas_sql()
    )
    .expect("the connection pragmas are written");
    for statement in prefix {
        writeln!(stdin, "{statement}").expect("a statement is written");
    }
    writeln!(stdin, "SELECT '{ENGINE_MARKER}';").expect("the marker is written");
    stdin.flush().expect("the batch reaches the engine");

    let stdout = engine.stdout.take().expect("stdout is piped");
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .expect("the engine's output is readable");
        assert!(
            read > 0,
            "sqlite3 exited before executing the {} statements it was sent, so the kill would land \
             nowhere near the transaction",
            prefix.len()
        );
        if line.contains(ENGINE_MARKER) {
            break;
        }
    }

    announce_ready();
    // `stdin` and `engine` stay alive because this never returns, which is what keeps the engine's
    // transaction open until the kill arrives.
    wait_to_be_killed();
}

/// Write and `fsync` the marker meaning "the user was told".
fn record_acknowledgement(root: &Path) {
    let path = root.join(ACKNOWLEDGED_MARKER);
    let mut file = std::fs::File::create(&path).expect("the marker is writable");
    file.write_all(b"saved privately\n")
        .expect("the marker is written");
    file.sync_all().expect("the marker is durable");
}

// ---------------------------------------------------------------------------------------------
// The parent
// ---------------------------------------------------------------------------------------------

/// The child that saves into `root`, stops after `stop`, and carries `weight` operations.
fn child(root: &Path, stop: &str, weight: u64) -> ChildSpec {
    ChildSpec::new(CHILD_TEST)
        .with(ROOT_VARIABLE, root.display().to_string())
        .with(STOP_VARIABLE, stop)
        .with(WEIGHT_VARIABLE, weight.to_string())
}

/// Spawn a child, wait for it to reach `stop`, kill it, and return how it died.
fn kill_child_at(root: &Path, stop: &str, weight: u64) -> ExitStatus {
    kill_child_at_ready(&child(root, stop, weight))
}

// ---------------------------------------------------------------------------------------------
// What the parent finds
// ---------------------------------------------------------------------------------------------

/// What state the database was found in, told apart from what it holds.
///
/// The distinction is the whole point. "There is no schema" and "the checkpoint is not there" are
/// different findings, and the harness reported the first as the second — as a panic reading
/// `no such table: operation`, which is one word away from the sentence this file exists to make
/// impossible (`01KZERXN1BC2FEDNNXNBKTNY7E`). A reader who cannot tell a workspace killed before
/// its migrations from a workspace that lost acknowledged work has no evidence at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DatabaseState {
    /// No file. The child was killed before `Store::open` created one.
    NoFile,
    /// A file with no `operation` table. The child was killed while `Store::open` was migrating,
    /// after SQLite created the file and before the schema existed. Empty, and *not* a loss:
    /// nothing had been acknowledged, because nothing had been saved.
    Migrating,
    /// A file with the schema, which is the only state in which a count means anything.
    Ready,
}

impl DatabaseState {
    fn describe(self) -> &'static str {
        match self {
            Self::NoFile => "no database file — the child died before one was created",
            Self::Migrating => {
                "a database file with no schema — the child died inside `Store::open`'s \
                 migrations, so there was nothing to lose and nothing was acknowledged"
            }
            Self::Ready => "a migrated database",
        }
    }
}

/// What a killed child left behind.
#[derive(Debug)]
struct Residue {
    /// Whether the user had been told the work was saved privately.
    acknowledged: bool,
    /// Whether the database was readable, migrating, or absent when the counts were taken.
    database: DatabaseState,
    /// How many operation rows the database holds.
    operations: i64,
    /// How many actor-head rows the database holds.
    heads: i64,
    /// How many manifest-chunk references the database holds.
    references: i64,
    /// The chunk names recorded as having arrived.
    arrivals: Vec<RecordDigest>,
    /// How many files are staged and unpromoted.
    scratch: usize,
    /// Whether the chunk is in the content store, whole and hashing to its own name.
    chunk_present: bool,
}

/// How many times a refused read is retried before it is believed, and the pause before each.
///
/// A killed engine leaves a write-ahead log the next reader recovers; that recovery is fast but not
/// instantaneous, so a refusal is retried before it is believed. Nothing about the *schedule* of
/// this campaign depends on these numbers — they bound how long a genuinely damaged database takes
/// to be reported, not where any kill lands.
const READ_ATTEMPTS: u32 = 20;

/// Whether the database exists and has been migrated, retried the same way a count is.
///
/// Asked before any count, because the answer changes what a count *means*. `sqlite3` creates the
/// file on first write, so the file existing says only that `Store::open` got as far as its first
/// statement.
fn database_state(root: &Path) -> DatabaseState {
    let path = root.join(DATABASE);
    if !path.is_file() {
        return DatabaseState::NoFile;
    }
    let reader = Sqlite3::at(&path);
    let sql = "SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND name = 'operation';";
    match retrying(|| reader.count(sql)) {
        Ok(1..) => DatabaseState::Ready,
        Ok(_) => DatabaseState::Migrating,
        // `sqlite_schema` is readable in every state a real SQLite database can be in, so a refusal
        // here is the harness failing to read, which is reported rather than guessed at.
        Err(problem) => panic!(
            "the database at {} could not be interrogated after the crash, so nothing below is \
             evidence either way: {problem}",
            path.display()
        ),
    }
}

/// Count a table, failing loudly rather than reading a locked or damaged database as empty.
///
/// A read that quietly returned zero would turn "the parent could not open the database" into "the
/// checkpoint is not there", which is the same sentence as an acknowledged-state loss and would
/// have made this harness capable of both false alarms and false silence. It was capable of exactly
/// that until a leftover engine process produced a database that answered one query and refused the
/// next.
///
/// Only ever called on a [`DatabaseState::Ready`] database. A missing table here therefore means the
/// schema lost a table it had, which is a real finding, and the message says so instead of reading
/// like acknowledged-state loss.
fn count_or_fail(reader: &Sqlite3, table: &str) -> i64 {
    let sql = format!("SELECT count(*) FROM {table};");
    match retrying(|| reader.count(&sql)) {
        Ok(count) => count,
        Err(problem) => panic!(
            "`{table}` is missing from a database that has been migrated, or could not be counted \
             in {READ_ATTEMPTS} attempts: {problem}. This is the harness failing to read the \
             database, not the database failing to hold the checkpoint — the acknowledgement \
             marker, not this, is what says whether anything was promised."
        ),
    }
}

/// Retry a read that a recovering write-ahead log may refuse, and return the last failure.
fn retrying<T>(read: impl Fn() -> Result<T, SqliteError>) -> Result<T, String> {
    let mut last = String::new();
    for attempt in 0..READ_ATTEMPTS {
        match read() {
            Ok(value) => return Ok(value),
            Err(error) => {
                last = error.to_string();
                std::thread::sleep(Duration::from_millis(25 * u64::from(attempt + 1)));
            }
        }
    }
    Err(last)
}

fn inspect(root: &Path) -> Residue {
    let promoter = FilePromoter::open(root);
    let bytes = chunk_bytes(7);
    let chunk = content_name(&bytes);

    let database = database_state(root);
    let (operations, heads, references) = match database {
        DatabaseState::Ready => {
            let reader = Sqlite3::at(root.join(DATABASE));
            (
                count_or_fail(&reader, "operation"),
                count_or_fail(&reader, "actor_head"),
                count_or_fail(&reader, "manifest_chunk"),
            )
        }
        // Zero rows, and the state that says why, so no assertion below can read this as a loss.
        DatabaseState::NoFile | DatabaseState::Migrating => (0, 0, 0),
    };

    let chunk_present = promoter.is_durable(&chunk)
        && promoter
            .read_verified(&chunk)
            .is_ok_and(|found| found == bytes);

    Residue {
        acknowledged: root.join(ACKNOWLEDGED_MARKER).exists(),
        database,
        operations,
        heads,
        references,
        arrivals: promoter.arrivals(),
        scratch: promoter.scratch_count(),
        chunk_present,
    }
}

/// The two invariants, checked on every corpse this file produces.
///
/// 1. **An acknowledgement is never ahead of durability.** If the marker is there, the checkpoint
///    is there.
/// 2. **A durable reference is never ahead of content.** If the database references a chunk, the
///    chunk is in the store, whole, and hashing to its own name.
///
/// Also checked: a chunk that became visible is recorded as having arrived, so it is a collection
/// candidate rather than a leak.
fn assert_durability_invariants(root: &Path, residue: &Residue, context: &str) {
    if residue.acknowledged {
        assert!(
            residue.operations > 0 && residue.heads > 0,
            "{context}: the user was told the work was saved privately, and the parent found {} \
             holding {} operations and {} heads. This is acknowledged-state loss, which is a P0.",
            residue.database.describe(),
            residue.operations,
            residue.heads
        );
    }

    if residue.references > 0 {
        assert!(
            residue.chunk_present,
            "{context}: the database references a chunk the content store does not hold, whole \
             and verified — a dangling reference, which plan §6.3 forbids at every step boundary"
        );
    }

    if residue.chunk_present {
        let bytes = chunk_bytes(7);
        assert!(
            residue.arrivals.contains(&content_name(&bytes)),
            "{context}: the chunk is visible but no arrival was recorded, so an unreferenced copy \
             would leak instead of becoming a collection candidate"
        );
        assert_eq!(
            support::promoter::sweep_chunks(root).len(),
            1,
            "{context}: the store holds a chunk this sequence did not promote"
        );
    }
}

/// The residue class plan §6.3 names for this step, checked against what is on disk.
fn assert_matches_declared_residue(residue: &Residue, step: SequenceStep) {
    let context = format!("after {step}");
    match step.residue_if_killed_after() {
        CrashResidue::DiscardTemporary => {
            assert!(
                !residue.chunk_present,
                "{context}: a chunk is addressable before step 4 promoted it"
            );
            assert!(
                residue.arrivals.is_empty(),
                "{context}: an arrival was recorded for a chunk that never arrived"
            );
            assert_eq!(
                residue.scratch, 1,
                "{context}: temporary data should be present and discardable"
            );
            assert_eq!(residue.operations, 0, "{context}: the index is not empty");
            assert_eq!(residue.references, 0, "{context}: a reference exists");
        }
        CrashResidue::CollectUnreferencedChunks => {
            assert!(
                residue.chunk_present,
                "{context}: the chunk was promoted and is not there"
            );
            assert_eq!(
                residue.scratch, 0,
                "{context}: the promotion left its temporary file behind"
            );
            assert_eq!(
                residue.operations, 0,
                "{context}: the transaction had not committed, so nothing is indexed"
            );
            assert_eq!(
                residue.references, 0,
                "{context}: a durable reference to the chunk exists before the transaction \
                 committed"
            );
        }
        CrashResidue::RecoverCheckpoint => {
            assert!(residue.chunk_present, "{context}: the chunk is gone");
            assert!(
                residue.operations > 0,
                "{context}: the transaction returned and the checkpoint is not there"
            );
            assert!(residue.heads > 0, "{context}: no head survived");
            assert!(residue.references > 0, "{context}: no manifest survived");
        }
    }
    assert_eq!(
        residue.acknowledged,
        step.acknowledged_by_here(),
        "{context}: whether the user was told disagrees with the plan"
    );
}

// ---------------------------------------------------------------------------------------------
// Campaign one: every step boundary
// ---------------------------------------------------------------------------------------------

/// One operation is enough for the boundary campaign; the weight exists for the timing campaign.
const LIGHT: u64 = 1;

#[test]
fn killing_before_any_step_leaves_the_workspace_untouched() {
    let directory = TempDir::new("crash-before-any");
    let root = directory.path();
    assert_killed(
        kill_child_at(root, STOP_BEFORE_ANY, LIGHT),
        "before any step",
    );

    let residue = inspect(root);
    assert!(!residue.acknowledged);
    assert!(!residue.chunk_present);
    assert_eq!(residue.scratch, 0);
    assert_eq!(residue.operations, 0);
    assert!(residue.arrivals.is_empty());
    assert_durability_invariants(root, &residue, "before any step");
}

/// Every one of the eleven steps, killed immediately after it, with the residue plan §6.3 names.
///
/// One test rather than eleven because the assertion is the same one eleven times, and because a
/// per-step test would have to repeat the plan's table by hand — which is the drift this file is
/// built to avoid. The step is named in every failure message.
#[test]
fn killing_after_each_of_the_eleven_steps_leaves_exactly_what_the_plan_says() {
    for step in SequenceStep::ORDER {
        let directory = TempDir::new(&format!("crash-{}", step.name()));
        let root = directory.path();
        let context = format!("after {step}");

        assert_killed(kill_child_at(root, step.name(), LIGHT), &context);

        let residue = inspect(root);
        assert_durability_invariants(root, &residue, &context);
        assert_matches_declared_residue(&residue, step);
    }
}

/// The acknowledgement boundary, stated as the one comparison that matters: the first step after
/// which the checkpoint survives, and the first step after which the user has been told.
///
/// They are step 9 and step 10, in that order, and never the other way round.
#[test]
fn durability_is_reached_before_the_acknowledgement_is_given() {
    let first_durable = SequenceStep::ORDER
        .into_iter()
        .find(|step| step.residue_if_killed_after() == CrashResidue::RecoverCheckpoint)
        .expect("some step makes the checkpoint durable");
    let first_acknowledged = SequenceStep::ORDER
        .into_iter()
        .find(|step| step.acknowledged_by_here())
        .expect("some step acknowledges");

    assert_eq!(first_durable, SequenceStep::CommitTransaction);
    assert_eq!(first_acknowledged, SequenceStep::ReportSaved);
    assert!(first_durable.plan_step() < first_acknowledged.plan_step());
}

/// The coverage claim cannot go stale silently: a twelfth step fails this.
#[test]
fn every_sequence_step_has_a_kill_point() {
    assert_eq!(SequenceStep::ORDER.len(), 11);
    for step in SequenceStep::ORDER {
        assert_eq!(
            SequenceStep::from_name(step.name()),
            Some(step),
            "{step} cannot be named on the child's command line, so it cannot be killed at"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Campaign two: kills inside the transaction
// ---------------------------------------------------------------------------------------------

/// Enough operations that the transaction is thousands of statements, so a percentage of the batch
/// is a meaningful place to stop.
const HEAVY: u64 = 2_000;

/// A campaign has to straddle the commit to be evidence. The old check counted corpses, which is a
/// quantity a busy machine could empty; this one is a statement about the *schedule*, which is a
/// constant in this file, so it fails only if somebody edits the schedule down to one side.
///
/// Separated from the campaign so it can be watched failing —
/// [`the_straddle_check_still_fails_when_the_schedule_covers_one_side`] does exactly that.
fn straddle_verdict(schedule: &[Prefix]) -> Result<(usize, usize), String> {
    let committed = schedule.iter().filter(|prefix| prefix.commits()).count();
    let empty = schedule.len() - committed;
    if committed == 0 || empty == 0 {
        return Err(format!(
            "the schedule places all {} of its kills on one side of the transaction ({empty} \
             before COMMIT, {committed} after it), so the campaign would pass without exercising \
             the boundary",
            schedule.len()
        ));
    }
    Ok((empty, committed))
}

/// The boundary campaign kills *between* steps. This one kills *inside* the batch `sqlite3` is
/// executing, which is the only interval in which a partly-written index could exist.
///
/// Twelve children, each stopped at a named statement offset with the engine's transaction open,
/// and each asserting both the all-or-nothing invariant and the exact row count its offset implies.
/// Nothing is timed; see the module documentation for what was here before and why it went.
#[test]
fn kills_inside_the_transaction_leave_all_of_it_or_none_of_it() {
    let (expected_empty, expected_committed) =
        straddle_verdict(&Prefix::ORDER).unwrap_or_else(|problem| panic!("{problem}"));
    assert_eq!(
        Prefix::ORDER.len(),
        12,
        "the interior campaign is twelve children; deleting one is not a way to make the suite green"
    );

    let mut committed = 0;
    let mut empty = 0;

    for (round, prefix) in Prefix::ORDER.into_iter().enumerate() {
        let directory = TempDir::new(&format!("crash-inside-{}", prefix.name()));
        let root = directory.path();

        let context = format!("round {round} killed at {}", prefix.name());
        assert_killed(
            kill_child_at_ready(
                &child(root, STOP_NOWHERE, HEAVY).with(PREFIX_VARIABLE, prefix.name()),
            ),
            &context,
        );

        let residue = inspect(root);
        assert_durability_invariants(root, &residue, &context);
        assert_eq!(
            residue.database,
            DatabaseState::Ready,
            "{context}: the parent found {}. This campaign's kills land inside the transaction, so \
             a database that never got a schema means the schedule stopped somewhere it does not \
             claim to",
            residue.database.describe()
        );

        // All or nothing: the transaction is the atom, so a partial index is the failure.
        assert!(
            residue.operations == 0 || residue.operations == HEAVY as i64,
            "{context}: the database holds {} of {HEAVY} operations, which is a partly-committed \
             transaction",
            residue.operations
        );
        // And the stronger statement the schedule now supports: which of the two it is, is decided
        // by whether `COMMIT;` was sent, not by how busy the machine was.
        assert_eq!(
            residue.operations > 0,
            prefix.commits(),
            "{context}: the engine was killed with {} statements executed and the database holds \
             {} operations. The transaction's fate disagrees with whether COMMIT was sent",
            prefix.name(),
            residue.operations
        );
        if residue.operations == 0 {
            empty += 1;
            assert_eq!(
                residue.references, 0,
                "{context}: a manifest reference survived a transaction that did not commit"
            );
        } else {
            committed += 1;
            assert!(
                residue.heads > 0 && residue.references > 0,
                "{context}: the transaction committed and left {} heads and {} references",
                residue.heads,
                residue.references
            );
        }
    }

    assert_eq!(
        (empty, committed),
        (expected_empty, expected_committed),
        "the corpses did not fall where the schedule says they must: {empty} empty and {committed} \
         committed, against a schedule of {expected_empty} and {expected_committed}"
    );
}

/// The straddle check can still fail. Run over a schedule that covers one side, it says so.
///
/// Without this the repair would be indistinguishable from deleting the assertion, which
/// `01KZERXN1BC2FEDNNXNBKTNY7E` put out of scope in as many words.
#[test]
fn the_straddle_check_still_fails_when_the_schedule_covers_one_side() {
    let uncommitted: Vec<Prefix> = Prefix::ORDER
        .into_iter()
        .filter(|prefix| !prefix.commits())
        .collect();
    let problem =
        straddle_verdict(&uncommitted).expect_err("a schedule that never commits is not evidence");
    assert!(problem.contains("one side of the transaction"), "{problem}");

    straddle_verdict(&[Prefix::WholeBatch])
        .expect_err("a schedule that only commits is not evidence either");
    straddle_verdict(&Prefix::ORDER).expect("the campaign's own schedule straddles the commit");
}

/// The schedule is twelve distinct, nameable points, and every one of them names a real cut of the
/// batch. A point the child cannot be told about is a point that never runs.
#[test]
fn every_interior_prefix_is_nameable_and_cuts_the_batch_where_it_says() {
    let mut names: Vec<String> = Prefix::ORDER.iter().map(|prefix| prefix.name()).collect();
    let total = names.len();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), total, "two schedule points share a name");

    for prefix in Prefix::ORDER {
        assert_eq!(Prefix::from_name(&prefix.name()), Some(prefix));
    }
    assert_eq!(Prefix::from_name("no-such-prefix"), None);

    // A batch of `BEGIN`, ten interior statements and `COMMIT`.
    let batch: Vec<String> = std::iter::once("BEGIN IMMEDIATE;".to_owned())
        .chain((0..10).map(|n| format!("INSERT INTO operation VALUES ({n});")))
        .chain(std::iter::once("COMMIT;".to_owned()))
        .collect();
    assert_eq!(Prefix::Percent(0).cut(&batch), 1, "nothing but BEGIN");
    assert_eq!(
        Prefix::Percent(50).cut(&batch),
        6,
        "BEGIN and half the inserts"
    );
    assert_eq!(Prefix::Percent(90).cut(&batch), 10);
    assert_eq!(
        Prefix::AllButCommit.cut(&batch),
        11,
        "everything but COMMIT"
    );
    assert_eq!(Prefix::WholeBatch.cut(&batch), 12, "COMMIT included");
    for prefix in Prefix::ORDER {
        assert!(
            prefix.cut(&batch) >= 1,
            "{} would stop before BEGIN, which is not inside the transaction",
            prefix.name()
        );
    }
}

/// The batch splitter keeps statements whole, including one that spans two lines.
///
/// The schedule is a count of statements, so a splitter that miscounted would move every kill point
/// silently — the exact class of defect this campaign was rewritten to remove.
#[test]
fn the_batch_splits_into_whole_statements() {
    let sql = "BEGIN IMMEDIATE;\nINSERT INTO a VALUES (1);\nINSERT INTO b\n  VALUES (2);\nCOMMIT;";
    assert_eq!(
        split_statements(sql),
        vec![
            "BEGIN IMMEDIATE;".to_owned(),
            "INSERT INTO a VALUES (1);".to_owned(),
            "INSERT INTO b\n  VALUES (2);".to_owned(),
            "COMMIT;".to_owned(),
        ]
    );
}

// ---------------------------------------------------------------------------------------------
// Campaign three: disk full at every step
// ---------------------------------------------------------------------------------------------

/// `ENOSPC` injected at each of plan §6.3's steps 1 to 4, in-process, against a real database.
///
/// This is the error path rather than the crash path, and the two are different: an error unwinds,
/// so what it must leave behind is *at most* what a crash at the same point leaves. The assertion
/// is therefore the same durability invariant plus one more — nothing was acknowledged.
#[test]
fn a_full_disk_at_any_chunk_step_acknowledges_nothing_and_indexes_nothing() {
    for plan_step in 1..=4u8 {
        let directory = TempDir::new(&format!("full-{plan_step}"));
        let root = directory.path();
        let mut promoter = FilePromoter::open(root).with_disk_full_at(plan_step);
        let mut store = Store::open(Sqlite3::at(root.join(DATABASE))).expect("the database opens");

        let bytes = chunk_bytes(7);
        let chunk = content_name(&bytes);
        let mut sequence = DurableCommit::new(
            &mut store,
            &mut promoter,
            vec![bytes],
            checkpoint(LIGHT, chunk),
        );

        let error = loop {
            match sequence.step() {
                Ok(Some(_)) => {}
                Ok(None) => panic!("step {plan_step} was made to fail and the sequence completed"),
                Err(error) => break error,
            }
        };
        assert_eq!(error.step().plan_step(), plan_step);
        assert!(!error.outcome_is_unknown());
        assert!(
            sequence.acknowledgement().is_none(),
            "step {plan_step} failed and the sequence acknowledged anyway"
        );

        let residue = inspect(root);
        assert!(!residue.acknowledged);
        assert_eq!(
            residue.operations, 0,
            "step {plan_step} failed and the index was written"
        );
        assert_durability_invariants(root, &residue, &format!("ENOSPC at step {plan_step}"));
    }
}

/// A full disk at step 9 leaves the outcome unknown by construction, acknowledges nothing, and
/// leaves the database holding either the whole checkpoint or none of it.
///
/// The executor is made to fail on the transaction batch by pointing it at a database path that has
/// been replaced with a directory — a real failure from the real driver, not a mocked one.
#[test]
fn a_failing_transaction_acknowledges_nothing_and_says_the_outcome_is_unknown() {
    let directory = TempDir::new("full-commit");
    let root = directory.path();
    let mut promoter = FilePromoter::open(root);
    let database = root.join(DATABASE);
    let mut store = Store::open(Sqlite3::at(&database)).expect("the database opens");

    let bytes = chunk_bytes(7);
    let chunk = content_name(&bytes);
    let mut sequence = DurableCommit::new(
        &mut store,
        &mut promoter,
        vec![bytes],
        checkpoint(LIGHT, chunk),
    );
    sequence
        .run_through(SequenceStep::FillOutbox)
        .expect("steps 1 to 8 run");
    assert!(sequence.acknowledgement().is_none());

    // Make the transaction fail for real: the database file is replaced by a directory, so the
    // driver's next connection cannot open it.
    std::fs::remove_file(&database).expect("the database file is removable");
    for sidecar in ["metadata.sqlite-wal", "metadata.sqlite-shm"] {
        let _ = std::fs::remove_file(root.join(sidecar));
    }
    std::fs::create_dir(&database).expect("a directory stands in the database's place");

    let error = sequence.step().expect_err("the transaction fails");
    assert!(
        error.outcome_is_unknown(),
        "a failure inside the transaction must be reported as an unknown outcome, not as a clean \
         rollback: {error}"
    );
    assert!(sequence.acknowledgement().is_none());
    assert!(error.to_string().contains("Nothing was acknowledged"));
}
