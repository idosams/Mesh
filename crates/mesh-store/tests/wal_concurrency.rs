//! "Concurrent readers never block on the writer under WAL" — demonstrated, not asserted.
//!
//! # What would make this test worthless
//!
//! Three things, and each one is closed below rather than avoided by care:
//!
//! 1. **The writer might have already committed.** A test that starts a writer, sleeps, and then
//!    reads proves nothing: the reader may simply have arrived after the write finished. So the
//!    writer here is a live `sqlite3` process holding an *uncommitted* `BEGIN EXCLUSIVE`
//!    transaction, and every reader asserts it sees the **pre-transaction** value. Seeing the old
//!    value is the proof that the new one was in flight.
//!
//! 2. **The readers might not have been blocked by anything.** A reader that succeeds tells you
//!    nothing unless you know the same reader would have failed without WAL. So the identical
//!    experiment runs against a `journal_mode = DELETE` database as a control, and that control is
//!    asserted to **fail**. If a future SQLite stopped blocking rollback-journal readers, or if
//!    the zero busy-timeout stopped taking effect, the control would pass and this test would go
//!    red — which is the behaviour you want from a control.
//!
//! 3. **"Never blocks" might mean "blocks for slightly less than the timeout".** So the readers
//!    run with no busy timeout at all: a blocked reader is refused immediately rather than
//!    waiting. There is nothing for a latency bound to hide inside.
//!
//! The schema under test is the real one — `mesh-store`'s own migrations, applied through
//! [`Store::open`], with `operation` rows the fold produced.

mod common;

use std::path::Path;

use common::{read_once, HeldWriter, ReaderOutcome, Sqlite3, TempDir};
use mesh_store::{
    Checkpoint, Index, OperationRecord, RecordDigest, Row, SqlExecutor, Store, Value,
};

const READERS: usize = 16;

fn digest(seed: u8) -> RecordDigest {
    RecordDigest::from_bytes([seed; 32])
}

fn operation(id: u8, actor: u8, sequence: u64) -> OperationRecord {
    OperationRecord {
        id: digest(id),
        actor: digest(actor),
        actor_sequence: sequence,
        hlc_millis: 1_700_000_000_000,
        hlc_counter: 0,
        policy_epoch: 1,
        session: mesh_store::no_session(),
        payload_digest: digest(id.wrapping_add(100)),
        parents: Vec::new(),
    }
}

/// A database carrying the real schema and one operation, at the given journal mode.
fn populated(directory: &TempDir, name: &str, journal_mode: &str) -> Sqlite3 {
    let path = directory.join(name);
    let mut executor = Sqlite3::at(&path);
    executor
        .execute_batch(&format!("PRAGMA journal_mode = {journal_mode};"))
        .expect("the journal mode is set");

    let mut store = Store::open(Sqlite3::at(&path)).expect("the store opens");
    store
        .commit(&Checkpoint {
            operations: vec![operation(1, 2, 1)],
            ..Checkpoint::default()
        })
        .expect("the first checkpoint commits");

    // `Store::open` applies `journal_mode = WAL` unconditionally, so the control has to be put
    // back into rollback-journal mode after the store has finished with it. Doing it in this order
    // — rather than skipping the store for the control — keeps both arms of the experiment running
    // against the identical schema and the identical rows.
    if !journal_mode.eq_ignore_ascii_case("wal") {
        executor
            .execute_batch(&format!("PRAGMA journal_mode = {journal_mode};"))
            .expect("the control returns to its journal mode");
    }
    executor
}

/// Hold an uncommitted write on `operation`'s `policy_epoch`, run the readers, then commit.
fn readers_against_a_held_write(database: &Path) -> Vec<ReaderOutcome> {
    let writer = HeldWriter::begin(
        database,
        "UPDATE operation SET policy_epoch = 99 WHERE policy_epoch = 1;",
    )
    .expect("the writer takes and holds its transaction");

    let outcomes: Vec<ReaderOutcome> = (0..READERS)
        .map(|_| read_once(database, "SELECT 'EPOCH:' || policy_epoch FROM operation;"))
        .collect();

    writer.commit().expect("the held transaction commits");
    outcomes
}

#[test]
fn the_sqlite_binary_these_tests_need_is_present() {
    let version = common::sqlite3_version().expect("sqlite3 answers --version");
    assert!(
        !version.is_empty(),
        "sqlite3 reported no version; the independent system-driver tests do not skip"
    );
    println!("sqlite3 {version}");
}

/// The criterion. Sixteen readers, one writer holding an uncommitted `BEGIN EXCLUSIVE`, and every
/// reader both succeeds and sees the pre-transaction snapshot.
#[test]
fn under_wal_no_reader_blocks_on_a_held_write_transaction() {
    let directory = TempDir::new("wal");
    let database = populated(&directory, "metadata.sqlite", "WAL");
    assert_eq!(
        database.journal_mode().expect("the mode is readable"),
        "wal",
        "the experiment is only about WAL if the database is in WAL"
    );

    let outcomes = readers_against_a_held_write(database.path());
    assert_eq!(outcomes.len(), READERS);

    for (index, outcome) in outcomes.iter().enumerate() {
        assert!(
            outcome.succeeded,
            "reader {index} was refused while the writer held its transaction: {}",
            outcome.failure
        );
        // The proof that the writer was genuinely mid-transaction: the reader saw the OLD value.
        assert_eq!(
            outcome.observed, "EPOCH:1",
            "reader {index} did not see the pre-transaction snapshot, so the writer had already \
             committed and this run proved nothing"
        );
    }

    // After the commit the new value is visible, which shows the write really was pending.
    assert_eq!(
        database
            .scalar("SELECT 'EPOCH:' || policy_epoch FROM operation;")
            .expect("the row is readable"),
        "EPOCH:99"
    );
}

/// The control. The identical experiment on a rollback-journal database, where the readers are
/// expected to be **refused**. If this ever passes, the WAL test above has stopped proving
/// anything and both go red together.
#[test]
fn without_wal_a_reader_is_refused_by_the_same_held_write() {
    let directory = TempDir::new("delete");
    let database = populated(&directory, "metadata.sqlite", "DELETE");
    assert_eq!(
        database.journal_mode().expect("the mode is readable"),
        "delete"
    );

    let outcomes = readers_against_a_held_write(database.path());
    let refused = outcomes.iter().filter(|o| !o.succeeded).count();
    assert_eq!(
        refused, READERS,
        "every reader should have been refused without WAL; the control proving nothing means the \
         WAL result proves nothing either. Outcomes: {outcomes:?}"
    );
    assert!(
        outcomes
            .iter()
            .all(|outcome| outcome.failure.to_lowercase().contains("locked")),
        "the refusals should be lock refusals: {outcomes:?}"
    );
}

/// "Never blocks" has to mean *never*, not "waits less than the timeout". With no busy timeout at
/// all there is nothing to wait inside, so this bound is about process startup rather than about
/// lock contention — and the assertion is deliberately loose, because a tight one would be a
/// flakiness generator on a loaded machine while proving nothing extra.
#[test]
fn a_reader_under_wal_finishes_in_the_time_a_process_takes_to_start() {
    let directory = TempDir::new("wal-latency");
    let database = populated(&directory, "metadata.sqlite", "WAL");

    let baseline = read_once(
        database.path(),
        "SELECT 'EPOCH:' || policy_epoch FROM operation;",
    );
    assert!(baseline.succeeded, "the uncontended read failed");

    let contended = readers_against_a_held_write(database.path());
    let slowest = contended
        .iter()
        .map(|outcome| outcome.elapsed_millis)
        .max()
        .expect("there is at least one reader");

    println!(
        "uncontended {}ms, slowest contended {}ms over {} readers",
        baseline.elapsed_millis, slowest, READERS
    );
    assert!(
        slowest < 5_000,
        "a reader took {slowest}ms against a held write transaction, which is long enough to be a \
         wait rather than a snapshot read"
    );
}

/// A second *writer* is the case WAL does not make free, and pretending otherwise would be the
/// dishonest version of this file. It waits, and with `busy_timeout = 0` it is refused outright.
#[test]
fn a_second_writer_is_still_serialized_under_wal() {
    let directory = TempDir::new("wal-writers");
    let database = populated(&directory, "metadata.sqlite", "WAL");

    let writer = HeldWriter::begin(
        database.path(),
        "UPDATE operation SET policy_epoch = 99 WHERE policy_epoch = 1;",
    )
    .expect("the first writer holds");

    let second = read_once(
        database.path(),
        "BEGIN IMMEDIATE; UPDATE operation SET hlc_counter = 1; COMMIT;",
    );
    writer.commit().expect("the first writer commits");

    assert!(
        !second.succeeded,
        "a second writer was not serialized, which would mean WAL had removed a guarantee it does \
         not remove"
    );
    assert!(
        second.failure.to_lowercase().contains("locked")
            || second.failure.to_lowercase().contains("busy"),
        "the second writer failed for the wrong reason: {second:?}"
    );
}

/// WAL leaves two sidecar files beside the database. They are not an implementation detail to be
/// ignored: they hold the most recently written pages, which makes them the freshest data in the
/// store and the worst thing to leak into a materialized workspace.
#[test]
fn wal_creates_the_sidecar_files_the_exclusion_test_accounts_for() {
    let directory = TempDir::new("wal-sidecars");
    let database = populated(&directory, "metadata.sqlite", "WAL");

    let writer = HeldWriter::begin(database.path(), "UPDATE operation SET policy_epoch = 7;")
        .expect("the writer holds");
    let wal = directory.join("metadata.sqlite-wal");
    let shm = directory.join("metadata.sqlite-shm");
    assert!(wal.exists(), "no -wal file while a transaction is open");
    assert!(shm.exists(), "no -shm file while a transaction is open");
    writer.commit().expect("the writer commits");
}

/// The schema really is the one under test: the readers above were querying rows the fold wrote,
/// through the migrations this crate ships.
#[test]
fn the_database_under_test_carries_the_real_schema_and_the_real_rows() {
    let directory = TempDir::new("wal-schema");
    let mut database = populated(&directory, "metadata.sqlite", "WAL");

    let operation_table = mesh_store::table("operation").expect("the schema declares operation");
    let rows = database
        .read_table(operation_table)
        .expect("rows read back");
    assert_eq!(rows.len(), 1);

    let mut expected = Index::new();
    expected
        .apply(mesh_store::StoredRecord::Operation(operation(1, 2, 1)))
        .expect("the fold accepts it");
    assert_eq!(rows, expected.rows("operation").expect("rendered"));

    // And the row is what the schema says it is, not merely the right shape.
    let first: &Row = &rows[0];
    assert_eq!(first.values()[0], Value::blob(digest(1).as_bytes()));
    assert_eq!(first.values()[2], Value::Integer(1));
}
