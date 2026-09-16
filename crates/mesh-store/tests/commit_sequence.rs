//! Plan §6.3 steps 5 to 9, executed against real SQLite.
//!
//! # The test that makes the order mean something
//!
//! Asserting that the plan emits statements in the order 6, 7, 8 proves only that the code does
//! what the code does. What makes the order *load-bearing* is that the wrong order fails — so this
//! file runs the plan with steps 6 and 7 swapped and asserts that SQLite rejects it, because
//! `actor_head.operation_id` is a foreign key into `operation` and `foreign_keys` is on.
//!
//! Two other things are checked here rather than assumed:
//!
//! * **Atomicity.** A transaction whose last statement fails leaves nothing behind. Plan §6.3's
//!   crash clause depends on this: "after step 9 the checkpoint is durable", and before it,
//!   nothing is.
//! * **Rows, not just statements.** After the plan runs, the database's own contents are compared
//!   against the index the plan was built from.

mod common;

use common::{Sqlite3, TempDir};
use mesh_store::{
    AckRecord, Checkpoint, ChunkSlice, CommitPlan, CommitStep, Index, ManifestRecord,
    OperationRecord, PeerRecord, RecordDigest, SqlExecutor, Store, StoredRecord,
};

fn digest(seed: u8) -> RecordDigest {
    RecordDigest::from_bytes([seed; 32])
}

fn operation(id: u8, actor: u8, sequence: u64, parents: &[u8]) -> OperationRecord {
    OperationRecord {
        id: digest(id),
        actor: digest(actor),
        actor_sequence: sequence,
        hlc_millis: 1_700_000_000_000,
        hlc_counter: 0,
        policy_epoch: 4,
        session: mesh_store::no_session(),
        payload_digest: digest(id.wrapping_add(100)),
        parents: parents.iter().map(|seed| digest(*seed)).collect(),
    }
}

fn checkpoint() -> Checkpoint {
    Checkpoint {
        manifests: vec![ManifestRecord {
            id: digest(20),
            byte_length: 40,
            content_digest: digest(21),
            chunks: vec![
                ChunkSlice {
                    digest: digest(22),
                    byte_offset: 0,
                    byte_length: 16,
                },
                ChunkSlice {
                    digest: digest(23),
                    byte_offset: 16,
                    byte_length: 24,
                },
            ],
        }],
        operations: vec![operation(1, 2, 1, &[]), operation(3, 2, 2, &[1])],
        peers: vec![PeerRecord {
            peer: digest(30),
            joined_at: digest(1),
        }],
        acknowledgements: vec![AckRecord {
            peer: digest(30),
            actor: digest(2),
            actor_sequence: 1,
        }],
        ..Checkpoint::default()
    }
}

/// The whole plan, executed. Every statement, against real SQLite, with foreign keys on.
#[test]
fn the_plan_runs_against_real_sqlite_and_leaves_the_index_it_describes() {
    let directory = TempDir::new("commit");
    let path = directory.join("metadata.sqlite");
    let mut store = Store::open(Sqlite3::at(&path)).expect("opens");
    store.commit(&checkpoint()).expect("the checkpoint commits");

    let mut reader = Sqlite3::at(&path);
    for (name, rows) in mesh_store::read_all_tables(&mut reader).expect("reads") {
        assert_eq!(
            store.index().rows(name).expect("rendered"),
            rows,
            "`{name}` in the database is not what the plan described"
        );
    }

    // The tables the commit sequence names by number are all non-empty, so the run exercised each
    // of steps 6, 7 and 8.
    for table in ["operation", "manifest", "actor_head", "outbox"] {
        assert!(
            reader
                .count(&format!("SELECT count(*) FROM {table};"))
                .expect("counts")
                > 0,
            "`{table}` is empty, so this run did not exercise the step that fills it"
        );
    }
}

#[test]
fn the_plan_is_ordered_by_plan_step() {
    let (plan, _) = CommitPlan::stage(&Index::new(), &checkpoint()).expect("stages");
    let steps: Vec<u8> = plan.steps().iter().map(|step| step.plan_step()).collect();
    assert!(
        steps.windows(2).all(|pair| pair[0] <= pair[1]),
        "the plan goes backwards through plan §6.3: {steps:?}"
    );
    assert_eq!(steps.first(), Some(&5));
    assert_eq!(steps.last(), Some(&9));
    assert!(steps.contains(&6) && steps.contains(&7) && steps.contains(&8));
}

/// The proof that the order is load-bearing: run step 7 before step 6 and SQLite refuses, because
/// the head would point at an operation that does not exist yet.
#[test]
fn advancing_the_head_before_inserting_the_operation_is_refused_by_the_database() {
    let directory = TempDir::new("wrong-order");
    let path = directory.join("metadata.sqlite");
    let _ = Store::open(Sqlite3::at(&path)).expect("opens");

    let (plan, _) = CommitPlan::stage(&Index::new(), &checkpoint()).expect("stages");
    let mut reordered: Vec<String> = Vec::new();
    let mut deferred: Vec<String> = Vec::new();
    for statement in plan.statements() {
        match statement.step {
            CommitStep::InsertImmutable => deferred.push(statement.sql.clone()),
            CommitStep::AdvanceHeads if !deferred.is_empty() => {
                // Emit the head advance first, then everything that should have preceded it.
                reordered.push(statement.sql.clone());
                reordered.append(&mut deferred);
            }
            _ => reordered.push(statement.sql.clone()),
        }
    }

    let mut executor = Sqlite3::at(&path);
    // No `PRAGMA foreign_keys = ON` here: the harness re-applies the crate's own connection
    // pragmas on every spawn, so this is testing what `PRAGMAS` does rather than what this test
    // does.
    let result = executor.execute_batch(&reordered.join("\n"));
    assert!(
        result.is_err(),
        "advancing the head before the operation existed was accepted, which means the §6.3 \
         ordering is a convention this schema does not enforce"
    );
    let message = result.unwrap_err().to_string().to_lowercase();
    assert!(
        message.contains("foreign key"),
        "refused for the wrong reason: {message}"
    );

    // And nothing was left behind, because the failure aborted the transaction.
    let reader = Sqlite3::at(&path);
    assert_eq!(
        reader
            .count("SELECT count(*) FROM operation;")
            .expect("counts"),
        0
    );
    assert_eq!(
        reader
            .count("SELECT count(*) FROM actor_head;")
            .expect("counts"),
        0
    );
}

/// Plan §6.3's crash clause: before step 9 nothing is durable. A transaction whose last statement
/// fails must leave the database exactly as it was.
#[test]
fn a_transaction_that_fails_at_its_last_statement_leaves_nothing() {
    let directory = TempDir::new("atomic");
    let path = directory.join("metadata.sqlite");
    let mut store = Store::open(Sqlite3::at(&path)).expect("opens");
    store.commit(&checkpoint()).expect("the first commit lands");

    let before = Sqlite3::at(&path)
        .count("SELECT count(*) FROM operation;")
        .expect("counts");

    let mut executor = Sqlite3::at(&path);
    let doomed = format!(
        "BEGIN IMMEDIATE;\nINSERT INTO operation VALUES (X'{}', X'{}', \
         9, 0, 0, 0, X'{}', X'{}');\nINSERT INTO actor_head VALUES (X'{}', X'{}', 1);\nCOMMIT;",
        digest(60).to_hex(),
        digest(61).to_hex(),
        "00".repeat(16),
        digest(62).to_hex(),
        digest(63).to_hex(),
        digest(64).to_hex(),
    );
    assert!(
        executor.execute_batch(&doomed).is_err(),
        "the doomed transaction was accepted"
    );

    let after = Sqlite3::at(&path)
        .count("SELECT count(*) FROM operation;")
        .expect("counts");
    assert_eq!(
        before, after,
        "a failed transaction left a row behind, so a crash between §6.3 steps 6 and 9 would not \
         be clean"
    );
}

/// Two saves in a row. The second must not try to re-insert the first's rows, or every save after
/// the first would fail on a primary-key conflict — a bug that a single-checkpoint test cannot see.
#[test]
fn a_second_checkpoint_commits_on_top_of_the_first() {
    let directory = TempDir::new("second");
    let path = directory.join("metadata.sqlite");
    let mut store = Store::open(Sqlite3::at(&path)).expect("opens");
    store.commit(&checkpoint()).expect("first commit");

    let second = Checkpoint {
        operations: vec![operation(5, 6, 1, &[3])],
        acknowledgements: vec![AckRecord {
            peer: digest(30),
            actor: digest(2),
            actor_sequence: 2,
        }],
        ..Checkpoint::default()
    };
    store.commit(&second).expect("second commit");

    let mut reader = Sqlite3::at(&path);
    assert_eq!(
        reader
            .count("SELECT count(*) FROM operation;")
            .expect("counts"),
        3
    );
    // The acknowledgement cleared actor 2's outbox rows; actor 6's operation is still queued.
    assert_eq!(
        reader
            .count("SELECT count(*) FROM outbox;")
            .expect("counts"),
        1
    );
    for (name, rows) in mesh_store::read_all_tables(&mut reader).expect("reads") {
        assert_eq!(
            store.index().rows(name).expect("rendered"),
            rows,
            "`{name}`"
        );
    }
}

/// A checkpoint the fold rejects must never reach the database, so a contradictory record cannot
/// half-write an index before being noticed.
#[test]
fn a_rejected_checkpoint_never_touches_the_database() {
    let directory = TempDir::new("rejected");
    let path = directory.join("metadata.sqlite");
    let mut store = Store::open(Sqlite3::at(&path)).expect("opens");
    store.commit(&checkpoint()).expect("first commit");
    let before = store.index().default_digest();

    let forked = Checkpoint {
        operations: vec![operation(70, 2, 1, &[])],
        ..Checkpoint::default()
    };
    assert!(store.commit(&forked).is_err());
    assert_eq!(store.index().default_digest(), before);

    let reader = Sqlite3::at(&path);
    assert_eq!(
        reader
            .count("SELECT count(*) FROM operation;")
            .expect("counts"),
        2
    );
}

/// `BEGIN IMMEDIATE`, not `BEGIN`. The difference matters under concurrency and is stated in the
/// module docs; this is what stops it being edited away silently.
#[test]
fn the_transaction_is_immediate() {
    let (plan, _) = CommitPlan::stage(&Index::new(), &checkpoint()).expect("stages");
    assert_eq!(plan.statements()[0].sql, "BEGIN IMMEDIATE;");
    assert_eq!(
        plan.statements()[plan.statements().len() - 1].sql,
        "COMMIT;"
    );
    assert!(!plan.sql().contains("BEGIN;"));
}

/// The rebuild plan is a transaction too, and it also runs against real SQLite.
#[test]
fn the_rebuild_plan_runs_against_real_sqlite() {
    let directory = TempDir::new("rebuild-plan");
    let path = directory.join("metadata.sqlite");
    let mut store = Store::open(Sqlite3::at(&path)).expect("opens");
    store.commit(&checkpoint()).expect("commits");

    let mut index = Index::new();
    for record in checkpoint().records() {
        index.apply(record).expect("folds");
    }
    index
        .apply(StoredRecord::Operation(operation(80, 90, 1, &[])))
        .expect("folds");

    let plan = CommitPlan::full_rebuild(&index);
    let mut executor = Sqlite3::at(&path);
    executor
        .execute_batch(&plan.sql())
        .expect("the rebuild plan runs");

    let mut reader = Sqlite3::at(&path);
    for (name, rows) in mesh_store::read_all_tables(&mut reader).expect("reads") {
        if name == "schema_version" {
            continue;
        }
        assert_eq!(index.rows(name).expect("rendered"), rows, "`{name}`");
    }
}
