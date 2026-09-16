//! Crash recovery: destroy the index, rebuild it from records, and prove it came back.
//!
//! # The claim under test
//!
//! > **Every table is reconstructable from immutable operations and manifests alone**, and the
//! > proof is a rebuild that reaches the digest the acknowledgement carried — not an argument that
//! > one could.
//!
//! `tests/reconstruction.rs` already checks that a rebuild over a record stream held in memory
//! matches the live path. That is the arithmetic. This file is the *recovery*: the database is
//! deleted, a process is killed at each of plan §6.3's eleven steps, and the only input to what
//! comes back is the durable record journal on disk.
//!
//! # The four campaigns, and which acceptance criterion each one is
//!
//! | Campaign | Criterion |
//! |---|---|
//! | The database file is deleted outright and rebuilt | *"A dropped index rebuilds to a digest-identical state."* |
//! | A real `SIGKILL` at each of the eleven steps, then a recovery in the parent | *"The last durable boundary is identifiable after any kill point."* |
//! | A populated workspace, counted end to end through real SQLite | *"Recovery after a daemon crash completes within the plan's budget."* |
//! | One byte changed in a whole frame, and an append cut off mid-frame | *"Recovery never invents state: an unrecoverable record is reported, not skipped silently."* |
//!
//! The third campaign is *counted* rather than timed on the merge path, and
//! `benchmarks/budgets/recovery.md` is where that decision, the command that still times it and the
//! number it produced are recorded together.
//!
//! # Where the journal is written, and why it is there rather than anywhere else
//!
//! The child appends every record of a checkpoint to the journal, durably, **after plan §6.3's
//! step 4 and before step 5** — after the chunks a manifest names are promoted, and before a single
//! statement of the transaction is composed. That is the only ordering that holds the rule up:
//!
//! * journalling *after* the transaction would leave a window in which the index holds a checkpoint
//!   no rebuild could reproduce, which is the index being the source of truth;
//! * journalling *before* the chunks are promoted would let a rebuilt index reference content the
//!   content-addressed store does not hold.
//!
//! A kill between the journal append and the transaction therefore leaves a checkpoint that is
//! durable and unacknowledged, and recovery makes it visible. That is not a loss and not a
//! resurrection: the records were on disk, nobody was told anything, and plan §6.1's rule that the
//! records and not the database are the truth is exactly what says the recovery is right.
//!
//! # What is *not* claimed
//!
//! The five-second number below is measured on the machine running the test, against the process-
//! per-batch `sqlite3` driver this crate's tests supply — a driver that pays a process spawn where
//! a production one would not. It is therefore an **upper bound on this hardware with this driver**
//! and is reported with both, never as "recovery takes N milliseconds".

mod common;
mod support;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::{Sqlite3, TempDir};
use mesh_store::{
    scan_journal, AckRecord, ApprovalRecord, Checkpoint, ChunkPromoter, ChunkSlice, ContextAccess,
    ContextRecord, Digest16, DurableCommit, EntityUuid, ManifestRecord, OperationRecord,
    PeerRecord, RecordDigest, RecordJournal, RecoveryError, ReviewRecord, ReviewVerdict,
    SequenceStep, Store, TailResidue, TABLES,
};
use support::crash::{
    announce_ready, assert_killed, kill_child_at_ready, wait_to_be_killed, ChildSpec,
};
use support::journal::FileJournal;
use support::promoter::{content_name, FilePromoter};

/// The workspace root the child saves into.
const ROOT_VARIABLE: &str = "MESH_STORE_RECOVERY_ROOT";
/// The last step the child performs before it stops and waits to be killed.
const STOP_VARIABLE: &str = "MESH_STORE_RECOVERY_STOP";

/// The name of the child entry point, as libtest knows it.
const CHILD_TEST: &str = "recovery_child";

/// The file the child writes the acknowledged index digest into, the instant it exists.
const ACKNOWLEDGED_DIGEST: &str = "acknowledged.digest";

/// The database file, a sibling of the content store as plan §6.2 lays it out.
const DATABASE: &str = "metadata.sqlite";

/// The database's sidecars, which a dropped index has to lose too.
const SIDECARS: [&str; 2] = ["metadata.sqlite-wal", "metadata.sqlite-shm"];

/// Plan §6.3's recovery budget: *"recovery after daemon crash: under five seconds"*.
const RECOVERY_BUDGET: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------------------------------------
// The workspace both sides build
// ---------------------------------------------------------------------------------------------

fn digest(seed: u8) -> RecordDigest {
    RecordDigest::from_bytes([seed; 32])
}

fn session(seed: u8) -> EntityUuid {
    EntityUuid::from_bytes([seed; 16])
}

fn chunk_bytes(seed: u8) -> Vec<u8> {
    (0..64 * 1024u32)
        .map(|index| (index as u8) ^ seed)
        .collect()
}

fn operation(index: u64, actor: u8, parents: &[u64]) -> OperationRecord {
    OperationRecord {
        id: operation_id(index),
        actor: digest(actor),
        actor_sequence: index + 1,
        hlc_millis: 1_700_000_000_000 + index,
        hlc_counter: index,
        policy_epoch: 3,
        session: session(1),
        payload_digest: digest(9),
        parents: parents.iter().map(|parent| operation_id(*parent)).collect(),
    }
}

fn operation_id(index: u64) -> RecordDigest {
    let mut id = [0u8; 32];
    id[0..8].copy_from_slice(&index.to_be_bytes());
    id[31] = 0xA1;
    RecordDigest::from_bytes(id)
}

/// A checkpoint that touches every table, with `weight` operations and the one promoted chunk.
///
/// Every table matters: a recovery proved over a workspace that only wrote operations would say
/// nothing about the nine tables that are not the operation table.
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
        operations: (0..weight)
            .map(|index| {
                let parents: Vec<u64> = if index == 0 {
                    Vec::new()
                } else {
                    vec![index - 1]
                };
                operation(index, 2, &parents)
            })
            .collect(),
        peers: vec![PeerRecord {
            peer: digest(30),
            joined_at: operation_id(0),
        }],
        acknowledgements: vec![AckRecord {
            peer: digest(30),
            actor: digest(2),
            actor_sequence: 1,
        }],
        reviews: vec![ReviewRecord {
            bundle: digest(40),
            subject_operation: operation_id(0),
            opened_by: digest(2),
        }],
        approvals: vec![ApprovalRecord {
            approval: digest(41),
            bundle: digest(40),
            approver: digest(2),
            verdict: ReviewVerdict::Approved,
        }],
        context_entries: vec![ContextRecord {
            entry: digest(50),
            session: session(1),
            operation: operation_id(0),
            access: ContextAccess::Wrote,
            byte_length: 60,
        }],
    }
}

/// How many records a checkpoint of this weight puts in the journal.
fn record_count(weight: u64) -> u64 {
    weight + 6
}

// ---------------------------------------------------------------------------------------------
// The child
// ---------------------------------------------------------------------------------------------

/// One operation is enough for the kill-point campaign: what varies is *where* the kill lands.
const LIGHT: u64 = 1;

/// The child process's body. Inert unless the parent set [`ROOT_VARIABLE`].
///
/// Running normally — as one of the suite's tests — it asserts its own inertness, so this is not a
/// test that passes by doing nothing without saying so.
#[test]
fn recovery_child() {
    let Ok(root) = std::env::var(ROOT_VARIABLE) else {
        assert!(
            std::env::var(STOP_VARIABLE).is_err(),
            "the recovery child was told where to stop but not where to work; the parent sets both \
             or neither"
        );
        return;
    };

    let root = PathBuf::from(root);
    let stop = SequenceStep::from_name(&std::env::var(STOP_VARIABLE).expect("a stop point"))
        .expect("the stop point names a step");

    let mut promoter = FilePromoter::open(&root);
    promoter
        .discard_temporary()
        .expect("scratch from a previous crash is discardable");
    let mut journal = FileJournal::open(&root);
    let mut store = Store::open(Sqlite3::at(root.join(DATABASE))).expect("the database opens");

    let bytes = chunk_bytes(7);
    let chunk = content_name(&bytes);
    let saving = checkpoint(LIGHT, chunk);
    let records = saving.records();

    let mut sequence = DurableCommit::new(&mut store, &mut promoter, vec![bytes], saving);
    let mut acknowledged = false;
    while let Some(performed) = sequence.step().expect("the sequence runs") {
        // The journal append sits between step 4 and step 5, and the module docs say why.
        if performed == SequenceStep::PromoteChunks {
            mesh_store::journal_records(&mut journal, &records).expect("the journal accepts them");
        }
        if !acknowledged {
            if let Some(saved) = sequence.acknowledgement() {
                record_acknowledged_digest(&root, saved.index_digest());
                acknowledged = true;
            }
        }
        if performed == stop {
            break;
        }
    }

    announce_ready();
    wait_to_be_killed();
}

/// Write and `fsync` the digest the acknowledgement carried, so the parent can hold a rebuild
/// against the exact index the user was told about.
fn record_acknowledged_digest(root: &Path, value: Digest16) {
    use std::io::Write;
    let mut file =
        std::fs::File::create(root.join(ACKNOWLEDGED_DIGEST)).expect("the marker is writable");
    file.write_all(value.to_hex().as_bytes())
        .expect("the marker is written");
    file.sync_all().expect("the marker is durable");
}

fn acknowledged_digest(root: &Path) -> Option<Digest16> {
    let text = std::fs::read_to_string(root.join(ACKNOWLEDGED_DIGEST)).ok()?;
    let mut bytes = [0u8; 16];
    for (slot, pair) in bytes.iter_mut().zip(text.trim().as_bytes().chunks_exact(2)) {
        let high = (pair[0] as char).to_digit(16)?;
        let low = (pair[1] as char).to_digit(16)?;
        *slot = u8::try_from(high * 16 + low).ok()?;
    }
    Some(Digest16::from_bytes(bytes))
}

// ---------------------------------------------------------------------------------------------
// The parent's tools
// ---------------------------------------------------------------------------------------------

/// Open a store over a database a killed process left behind.
///
/// A killed engine leaves a write-ahead log the next reader recovers, and that recovery is fast but
/// not instantaneous. A refusal is retried a few times before it is believed, because reading "the
/// database is busy" as "the database is empty" is the same sentence as an acknowledged-state loss.
fn open_after_a_crash(path: &Path) -> Store<Sqlite3> {
    let mut last = String::new();
    for attempt in 0..20 {
        match Store::open(Sqlite3::at(path)) {
            Ok(store) => return store,
            Err(error) => {
                last = error.to_string();
                std::thread::sleep(Duration::from_millis(25 * (attempt + 1)));
            }
        }
    }
    panic!("the database could not be opened after the crash: {last}");
}

/// Every table's rows, read out of the database rather than out of an index that believes in them.
fn tables_on_disk(path: &Path) -> Vec<(&'static str, Vec<mesh_store::Row>)> {
    let mut reader = Sqlite3::at(path);
    mesh_store::read_all_tables(&mut reader).expect("every table reads back")
}

/// Delete the index and both its sidecars. This is the "drop" in "drop and rebuild".
fn destroy_the_index(root: &Path) {
    std::fs::remove_file(root.join(DATABASE)).expect("the database file is removable");
    for sidecar in SIDECARS {
        let _ = std::fs::remove_file(root.join(sidecar));
    }
    assert!(!root.join(DATABASE).exists(), "the index is still there");
}

/// Save a checkpoint the ordinary way, journalling its records first, and return the live digest.
fn save(
    store: &mut Store<Sqlite3>,
    journal: &mut FileJournal,
    checkpoint: &Checkpoint,
) -> Digest16 {
    mesh_store::journal_records(journal, &checkpoint.records()).expect("the journal accepts them");
    store.commit(checkpoint).expect("the checkpoint commits")
}

// ---------------------------------------------------------------------------------------------
// Campaign one: the index is destroyed and rebuilt
// ---------------------------------------------------------------------------------------------

/// The first acceptance criterion, executed rather than argued: the file is *deleted*, a fresh
/// database is migrated from nothing, and the records alone put every table back.
#[test]
fn a_deleted_index_rebuilds_to_a_digest_identical_state() {
    let directory = TempDir::new("recovery-drop");
    let root = directory.path();
    let path = root.join(DATABASE);
    let mut journal = FileJournal::open(root);

    let mut store = Store::open(Sqlite3::at(&path)).expect("opens");
    let healthy = save(&mut store, &mut journal, &checkpoint(4, digest(22)));
    let before = tables_on_disk(&path);
    drop(store);

    destroy_the_index(root);

    let mut recovered = Store::open(Sqlite3::at(&path)).expect("a fresh database migrates");
    let report = recovered
        .recover_verified(&mut journal, healthy)
        .expect("the rebuild reproduces the acknowledged digest");

    assert_eq!(report.digest(), healthy);
    assert_eq!(report.boundary().records, record_count(4));
    assert_eq!(report.tail(), TailResidue::Whole);
    assert!(!report.interrupted());

    let after = tables_on_disk(&path);
    assert_eq!(after.len(), TABLES.len());
    assert_eq!(before, after, "a table did not come back byte for byte");
    for (name, rows) in &after {
        if *name == "schema_version" {
            continue;
        }
        assert!(
            !rows.is_empty(),
            "`{name}` came back empty, so its equality above says nothing"
        );
    }
    println!(
        "rebuilt {} records into {} rows, digest {}",
        report.rebuild().records_replayed,
        report.rebuild().total_rows(),
        report.digest()
    );
}

/// A rebuild that lands somewhere else is plan §6.3's P0, and it is reported as a divergence rather
/// than accepted as close enough.
#[test]
fn a_rebuild_that_misses_the_acknowledged_digest_is_reported_as_a_divergence() {
    let directory = TempDir::new("recovery-diverge");
    let root = directory.path();
    let path = root.join(DATABASE);
    let mut journal = FileJournal::open(root);

    let mut store = Store::open(Sqlite3::at(&path)).expect("opens");
    let healthy = save(&mut store, &mut journal, &checkpoint(2, digest(22)));
    destroy_the_index(root);

    let mut recovered = Store::open(Sqlite3::at(&path)).expect("a fresh database migrates");
    let error = recovered
        .recover_verified(&mut journal, Digest16::from_bytes([0xcd; 16]))
        .expect_err("the digests disagree");
    assert!(
        error.to_string().contains("correctness defect"),
        "a divergence must not read as a recoverable hiccup: {error}"
    );
    let RecoveryError::Diverged { expected, rebuilt } = error else {
        panic!("expected a divergence");
    };
    assert_eq!(expected, Digest16::from_bytes([0xcd; 16]));
    assert_eq!(rebuilt, healthy);
}

// ---------------------------------------------------------------------------------------------
// Campaign two: a real kill at every one of plan §6.3's eleven steps
// ---------------------------------------------------------------------------------------------

/// The second acceptance criterion. After a `SIGKILL` at each step, a fresh process opens the
/// database and recovers, and the boundary it finds is the one the step implies.
///
/// One test rather than eleven because the assertion is the same one eleven times, and a per-step
/// test would restate the ordering rule by hand — which is the drift this file exists to avoid.
#[test]
fn the_last_durable_boundary_is_identifiable_after_a_kill_at_any_step() {
    for step in SequenceStep::ORDER {
        let directory = TempDir::new(&format!("recovery-{}", step.name()));
        let root = directory.path();
        let context = format!("after {step}");

        let spec = ChildSpec::new(CHILD_TEST)
            .with(ROOT_VARIABLE, root.display().to_string())
            .with(STOP_VARIABLE, step.name());
        assert_killed(kill_child_at_ready(&spec), &context);

        // The journal is appended between steps 4 and 5, so a kill before step 5 finds nothing.
        let journalled = step.plan_step() >= SequenceStep::PromoteChunks.plan_step();
        let expected_records = if journalled { record_count(LIGHT) } else { 0 };

        let mut journal = FileJournal::open(root);
        let scan = scan_journal(&journal.read_all().expect("the journal reads"))
            .unwrap_or_else(|damage| panic!("{context}: {damage}"));
        assert_eq!(
            scan.boundary().records,
            expected_records,
            "{context}: the boundary disagrees with where the append sits"
        );
        assert_eq!(
            scan.tail(),
            TailResidue::Whole,
            "{context}: an append was interrupted, which a kill between steps cannot do"
        );

        let mut store = open_after_a_crash(&root.join(DATABASE));
        let report = match acknowledged_digest(root) {
            Some(told) => store
                .recover_verified(&mut journal, told)
                .unwrap_or_else(|error| {
                    panic!(
                        "{context}: the user was told the work was saved privately and the rebuild \
                         did not reproduce it: {error}"
                    )
                }),
            None => store
                .recover(&mut journal)
                .unwrap_or_else(|error| panic!("{context}: {error}")),
        };

        assert_eq!(report.boundary(), scan.boundary(), "{context}");
        assert_eq!(report.rebuild().records_replayed as u64, expected_records);

        let operations = tables_on_disk(&root.join(DATABASE))
            .into_iter()
            .find(|(name, _)| *name == "operation")
            .map(|(_, rows)| rows.len() as u64)
            .expect("the operation table exists");
        assert_eq!(
            operations,
            if journalled { LIGHT } else { 0 },
            "{context}: the recovered index holds the wrong number of operations"
        );
    }
}

/// The coverage claim cannot go stale silently: a twelfth step fails this.
#[test]
fn every_sequence_step_is_a_kill_point_this_file_can_name() {
    assert_eq!(SequenceStep::ORDER.len(), 11);
    for step in SequenceStep::ORDER {
        assert_eq!(SequenceStep::from_name(step.name()), Some(step));
    }
}

// ---------------------------------------------------------------------------------------------
// Campaign three: the recovery cost model, and the wall-clock budget it stands for
// ---------------------------------------------------------------------------------------------

/// Every call a rebuild makes into the SQL driver, counted.
///
/// This is the instrument that replaced an `Instant::now()` comparison on the merge path, and the
/// reason for the replacement is `01KZGHMQ7NED1AH1T9X6S63N4K`: under a sixteen-writer disk load ramp
/// the elapsed-time assertion failed three whole-workspace runs in seven on an unchanged tree, so
/// its verdict was a function of what else the machine was doing. A clock measures the machine.
/// These counters measure the code, and they are identical on a busy laptop and an idle one.
///
/// They are not a proxy for the budget by hope. Under this crate's process-per-batch driver
/// (`docs/adr/0008-ship-the-sqlite-schema-without-a-sqlite-driver.md`) a `sqlite3` process spawn
/// costs milliseconds and dominates everything else, so the five-second budget is spent or saved in
/// exactly one place: how many processes the rebuild asks for, and how much SQL it hands each one.
/// A rebuild that stays at a fixed process count and a fixed number of statements per record cannot
/// leave the budget without the hardware changing; a rebuild that spawns per record, or composes a
/// second statement per record, breaks both counters long before it breaks the clock.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct DriverWork {
    /// `execute_batch` calls — one `sqlite3` process each.
    batches: u64,
    /// `read_table` calls — one process each.
    reads: u64,
    /// `table_exists` calls — one process each.
    probes: u64,
    /// Statements handed to the driver, counted by terminator, so this is an upper bound.
    statements: u64,
    /// SQL bytes handed to the driver.
    sql_bytes: u64,
}

impl DriverWork {
    /// Every process the driver spawns is one of the three call kinds above.
    fn processes(self) -> u64 {
        self.batches + self.reads + self.probes
    }

    /// This work minus the work already done when `earlier` was taken.
    fn since(self, earlier: Self) -> Self {
        Self {
            batches: self.batches - earlier.batches,
            reads: self.reads - earlier.reads,
            probes: self.probes - earlier.probes,
            statements: self.statements - earlier.statements,
            sql_bytes: self.sql_bytes - earlier.sql_bytes,
        }
    }
}

/// A shared tally, so the counts survive `Store` taking ownership of the executor.
#[derive(Clone, Debug, Default)]
struct Meter(Arc<Mutex<DriverWork>>);

impl Meter {
    fn read(&self) -> DriverWork {
        *self.0.lock().expect("the meter is not poisoned")
    }

    fn add(&self, change: impl FnOnce(&mut DriverWork)) {
        change(&mut self.0.lock().expect("the meter is not poisoned"));
    }
}

/// A [`mesh_store::SqlExecutor`] that counts what passes through it and forwards it unchanged.
///
/// It wraps the real driver rather than replacing it: the rebuild under measurement is the same
/// rebuild the rest of this file exercises, against real SQLite, and the counters are a side
/// observation rather than a simulation of one.
#[derive(Debug)]
struct Counted<E> {
    inner: E,
    meter: Meter,
}

impl<E> Counted<E> {
    fn new(inner: E, meter: Meter) -> Self {
        Self { inner, meter }
    }
}

impl<E: mesh_store::SqlExecutor> mesh_store::SqlExecutor for Counted<E> {
    type Error = E::Error;

    fn execute_batch(&mut self, sql: &str) -> Result<(), Self::Error> {
        let statements = sql.matches(';').count() as u64;
        let bytes = sql.len() as u64;
        self.meter.add(|work| {
            work.batches += 1;
            work.statements += statements;
            work.sql_bytes += bytes;
        });
        self.inner.execute_batch(sql)
    }

    fn read_table(
        &mut self,
        table: &mesh_store::Table,
    ) -> Result<Vec<mesh_store::Row>, Self::Error> {
        self.meter.add(|work| {
            work.reads += 1;
            work.statements += 1;
        });
        self.inner.read_table(table)
    }

    fn table_exists(&mut self, name: &str) -> Result<bool, Self::Error> {
        self.meter.add(|work| {
            work.probes += 1;
            work.statements += 1;
        });
        self.inner.table_exists(name)
    }
}

/// What one measured rebuild of a populated workspace cost.
struct Rebuild {
    /// The driver work of the rebuild alone — the migration that precedes it is subtracted.
    work: DriverWork,
    /// How long the rebuild took, which is reported and, on the merge path, never asserted.
    elapsed: Duration,
    /// Records replayed out of the journal.
    records: u64,
    /// Rows written across every table.
    rows: u64,
    /// Journal bytes read.
    journal_bytes: u64,
}

/// Enough operations that the rebuild is thousands of statements rather than a handful.
const POPULATED: u64 = 4_000;

/// Save a populated workspace, destroy the index, and rebuild it, counting everything.
fn rebuild_a_populated_workspace(weight: u64) -> Rebuild {
    let directory = TempDir::new("recovery-budget");
    let root = directory.path();
    let path = root.join(DATABASE);
    let mut journal = FileJournal::open(root);

    let mut store = Store::open(Sqlite3::at(&path)).expect("opens");
    let healthy = save(&mut store, &mut journal, &checkpoint(weight, digest(22)));
    drop(store);
    destroy_the_index(root);

    let meter = Meter::default();
    let mut recovered = Store::open(Counted::new(Sqlite3::at(&path), meter.clone()))
        .expect("a fresh database migrates");
    // The migration is not the recovery, so what it spent is subtracted rather than budgeted.
    let migrated = meter.read();

    let started = Instant::now();
    let report = recovered
        .recover_verified(&mut journal, healthy)
        .expect("the rebuild reproduces the acknowledged digest");
    let elapsed = started.elapsed();

    assert_eq!(report.boundary().records, record_count(weight));
    Rebuild {
        work: meter.read().since(migrated),
        elapsed,
        records: report.rebuild().records_replayed as u64,
        rows: report.rebuild().total_rows() as u64,
        journal_bytes: journal.byte_length(),
    }
}

/// One `sqlite3` process per record — or per table per record — is the regression that would spend
/// plan §6.3's budget, and this is the count that refuses it. Fifty is far above the twenty-odd a
/// correct rebuild needs and far below the four thousand a per-record spawn would need, so it fires
/// on the defect and not on a table being added.
const MAX_DRIVER_PROCESSES: u64 = 50;

/// Statements per replayed record, plus [`FIXED_STATEMENT_ALLOWANCE`] for the preamble.
///
/// Measured on this tree: 12,019 statements for 4,006 records, which is 3.0002 per record — the
/// fixed part is a rounding error at this weight. The bound is therefore set *at* the measured rate
/// rather than above it, so that a regression adding one statement per record (16,025) fails rather
/// than fitting in slack. Raising it is allowed; raising it without the measurement that justifies
/// the new rate, in the same pull request, is what `benchmarks/budgets/storage.md` §8 forbids.
const MAX_STATEMENTS_PER_RECORD: u64 = 3;

/// Room for the rebuild's fixed preamble — the transaction, the deletes, the pragmas — so the rate
/// above is a rate and not a rate plus a constant nobody wrote down.
const FIXED_STATEMENT_ALLOWANCE: u64 = 256;

/// SQL bytes per replayed record, measured at 916 and bounded at twice that.
///
/// The statement count alone would miss a regression that keeps the count and grows each statement,
/// which is what re-emitting a whole table per record looks like from the outside.
const MAX_SQL_BYTES_PER_RECORD: u64 = 2_048;

/// Plan §6.3's *"recovery after daemon crash: under five seconds"*, on the merge path, expressed as
/// the work the rebuild asks of the driver rather than as a reading of the wall clock.
///
/// # Why this is not the budget deleted
///
/// The wall-clock assertion still exists, still uses the same five-second constant, and still runs —
/// as `recovery_over_a_populated_workspace_is_inside_the_five_second_budget`, which is `#[ignore]`d
/// so that it is run deliberately on a quiet machine rather than incidentally on whatever machine
/// happens to be running the merge gate. `benchmarks/budgets/recovery.md` names the command, records
/// the measured number, and states plainly that `npm test` no longer enforces it. What `npm test`
/// enforces instead is the cost model above, which is what a real recovery regression breaks first.
#[test]
fn recovery_over_a_populated_workspace_stays_inside_its_cost_model() {
    let run = rebuild_a_populated_workspace(POPULATED);

    println!(
        "recovered {} records into {} rows with {} driver processes ({} batches, {} reads, {} \
         probes), {} statements, {} SQL bytes, journal {} bytes; observed {} ms, not asserted here",
        run.records,
        run.rows,
        run.work.processes(),
        run.work.batches,
        run.work.reads,
        run.work.probes,
        run.work.statements,
        run.work.sql_bytes,
        run.journal_bytes,
        run.elapsed.as_millis(),
    );

    assert_eq!(run.records, record_count(POPULATED));
    assert!(
        run.rows >= POPULATED,
        "{} rows for {POPULATED} operations, so the rebuild did not do the work the counts below \
         are meant to bound",
        run.rows
    );
    assert!(
        run.work.processes() <= MAX_DRIVER_PROCESSES,
        "the rebuild spawned {} `sqlite3` processes for {} records, over the {MAX_DRIVER_PROCESSES} \
         this cost model allows; at a process spawn per record plan §6.3's {RECOVERY_BUDGET:?} \
         cannot be met on any hardware",
        run.work.processes(),
        run.records
    );
    assert!(
        run.work.statements <= MAX_STATEMENTS_PER_RECORD * run.records + FIXED_STATEMENT_ALLOWANCE,
        "the rebuild issued {} statements for {} records — {:.4} per record, over the \
         {MAX_STATEMENTS_PER_RECORD} per record plus {FIXED_STATEMENT_ALLOWANCE} this cost model \
         allows. Recovery is doing more work per record than plan §6.3's {RECOVERY_BUDGET:?} was \
         measured against",
        run.work.statements,
        run.records,
        run.work.statements as f64 / run.records as f64
    );
    assert!(
        run.work.sql_bytes <= MAX_SQL_BYTES_PER_RECORD * run.records,
        "the rebuild handed the driver {} SQL bytes for {} records — {:.0} per record, over the \
         {MAX_SQL_BYTES_PER_RECORD} this cost model allows",
        run.work.sql_bytes,
        run.records,
        run.work.sql_bytes as f64 / run.records as f64
    );
}

/// Plan §6.3's *"recovery after daemon crash: under five seconds"*, measured end to end over a
/// populated workspace: journal read, scan, fold, and every table rewritten through real SQLite.
///
/// `#[ignore]`d, and that is the whole repair for `01KZGHMQ7NED1AH1T9X6S63N4K`. This assertion reads
/// a clock, so it answers a question about the machine as much as about the tree, and a merge gate
/// that a busy laptop can turn red teaches every lane that a red `npm test` may mean nothing. The
/// command that runs it, the number it produced and the fact that `npm test` does not enforce it are
/// all in `benchmarks/budgets/recovery.md`.
///
/// The number is printed as well as asserted, because a budget test that only says "under five
/// seconds" hides the day it becomes 4.9.
#[test]
#[ignore = "reads a wall clock; run deliberately on a quiet machine — benchmarks/budgets/recovery.md"]
fn recovery_over_a_populated_workspace_is_inside_the_five_second_budget() {
    let run = rebuild_a_populated_workspace(POPULATED);

    println!(
        "recovered {} records into {} rows in {} ms (budget {} ms), journal {} bytes",
        run.records,
        run.rows,
        run.elapsed.as_millis(),
        RECOVERY_BUDGET.as_millis(),
        run.journal_bytes
    );
    assert!(
        run.elapsed < RECOVERY_BUDGET,
        "recovery took {:?}, over plan §6.3's budget of {RECOVERY_BUDGET:?}",
        run.elapsed
    );
}

// ---------------------------------------------------------------------------------------------
// Campaign four: recovery never invents state
// ---------------------------------------------------------------------------------------------

/// The fourth acceptance criterion. One byte of a whole frame is changed, and recovery stops and
/// says which record — it does not skip the record, and it does not write a smaller index.
#[test]
fn a_damaged_record_stops_the_recovery_and_names_it_rather_than_skipping_it() {
    let directory = TempDir::new("recovery-damage");
    let root = directory.path();
    let path = root.join(DATABASE);
    let mut journal = FileJournal::open(root);

    let mut store = Store::open(Sqlite3::at(&path)).expect("opens");
    save(&mut store, &mut journal, &checkpoint(3, digest(22)));
    let healthy_tables = tables_on_disk(&path);
    drop(store);

    // A byte inside the second frame's body: the frame is whole, and its bytes are not the bytes
    // that were written.
    let scan = scan_journal(&journal.read_all().expect("reads")).expect("the journal is whole");
    let first_frame = scan.boundary().byte_offset / scan.boundary().records;
    journal.flip_byte(first_frame + 40);

    let mut recovering = Store::open(Sqlite3::at(&path)).expect("opens");
    let error = recovering
        .recover(&mut journal)
        .expect_err("a damaged record is never a successful recovery");
    let RecoveryError::Damaged(damage) = error else {
        panic!("expected damage, found {error}");
    };
    assert_eq!(damage.ordinal(), 1, "the wrong record was named");
    assert_eq!(damage.intact_prefix().records, 1);
    assert!(damage.to_string().contains("Nothing was skipped"));

    // And nothing was written: the database still holds what it held before.
    assert_eq!(
        tables_on_disk(&path),
        healthy_tables,
        "a refused recovery rewrote the index anyway"
    );
}

/// An append cut off mid-frame is the ordinary residue of a crash, and it is not damage. Recovery
/// resumes at the boundary before it and says how many bytes it discarded.
#[test]
fn an_interrupted_append_recovers_to_the_boundary_before_it() {
    let directory = TempDir::new("recovery-torn");
    let root = directory.path();
    let path = root.join(DATABASE);
    let mut journal = FileJournal::open(root);

    let mut store = Store::open(Sqlite3::at(&path)).expect("opens");
    save(&mut store, &mut journal, &checkpoint(3, digest(22)));
    drop(store);
    destroy_the_index(root);

    let whole = journal.byte_length();
    let torn = whole - 17;
    journal.truncate_to(torn);

    let mut recovered = Store::open(Sqlite3::at(&path)).expect("a fresh database migrates");
    let report = recovered.recover(&mut journal).expect("recovers");

    assert!(report.interrupted(), "the fragment was not noticed");
    assert!(report.boundary().byte_offset < torn);
    assert_eq!(
        report.tail(),
        TailResidue::Fragment {
            bytes: torn - report.boundary().byte_offset
        }
    );
    assert_eq!(
        report.rebuild().records_replayed as u64,
        report.boundary().records,
        "a record after the boundary was replayed"
    );
    assert!(
        report.boundary().records < record_count(3),
        "the truncation removed nothing, so the test proves nothing"
    );
}

/// A journal that is not a journal is refused, rather than read as an empty workspace. Reading
/// noise as "nothing was ever saved" is the silent version of losing everything.
#[test]
fn a_journal_that_is_not_a_journal_is_refused_rather_than_read_as_empty() {
    let directory = TempDir::new("recovery-noise");
    let root = directory.path();
    let path = root.join(DATABASE);
    let mut journal = FileJournal::open(root);
    journal
        .append(b"this file is not a mesh record journal at all, not even slightly")
        .expect("the bytes land");

    let mut store = Store::open(Sqlite3::at(&path)).expect("opens");
    let error = store
        .recover(&mut journal)
        .expect_err("noise is never a successful recovery");
    assert!(matches!(error, RecoveryError::Damaged(_)), "{error}");
}
