//! The daemon's start-up path, run end to end against a store and a journal.
//!
//! The unit tests in `src/recovery.rs` check what a diagnostic *says*. This file checks that the
//! daemon actually calls the recovery — that `recover_on_start` reaches `mesh_store::Store`, that a
//! damaged journal comes back as a blocking diagnostic instead of a panic, and that the digest
//! comparison is really wired to the acknowledged digest rather than to whatever was rebuilt.
//!
//! The SQL executor here is an in-memory recorder, not a database. That is deliberate and it is not
//! a gap: `mesh-store`'s own `tests/recovery.rs` runs the identical path against real SQLite with a
//! real `SIGKILL` in front of it, and what is under test *here* is the daemon's composition. A
//! second `sqlite3` harness in this crate would be a second thing to keep true.

use std::time::Duration;

use mesh_daemon::{
    recover_on_start, BudgetVerdict, DiagnosticsFeed, RecoveryOutcome, Severity, RECOVERY_BUDGET,
};
use mesh_store::{
    frame_record, no_session, OperationRecord, PeerRecord, RecordDigest, RecordJournal, Row,
    SqlExecutor, Store, StoredRecord, Table,
};

/// An executor that accepts every batch and answers every read empty.
#[derive(Debug, Default)]
struct Accepting {
    batches: Vec<String>,
}

impl SqlExecutor for Accepting {
    type Error = String;

    fn execute_batch(&mut self, sql: &str) -> Result<(), Self::Error> {
        self.batches.push(sql.to_owned());
        Ok(())
    }

    fn read_table(&mut self, _table: &Table) -> Result<Vec<Row>, Self::Error> {
        Ok(Vec::new())
    }

    fn table_exists(&mut self, _name: &str) -> Result<bool, Self::Error> {
        Ok(false)
    }
}

/// A journal held in memory, so this test measures composition and not a filesystem.
#[derive(Debug, Default)]
struct Bytes {
    held: Vec<u8>,
}

impl RecordJournal for Bytes {
    type Error = String;

    fn read_all(&mut self) -> Result<Vec<u8>, Self::Error> {
        Ok(self.held.clone())
    }

    fn append(&mut self, framed: &[u8]) -> Result<(), Self::Error> {
        self.held.extend_from_slice(framed);
        Ok(())
    }
}

fn digest(seed: u8) -> RecordDigest {
    RecordDigest::from_bytes([seed; 32])
}

fn records() -> Vec<StoredRecord> {
    vec![
        StoredRecord::Operation(OperationRecord {
            id: digest(1),
            actor: digest(2),
            actor_sequence: 1,
            hlc_millis: 1_700_000_000_000,
            hlc_counter: 0,
            policy_epoch: 1,
            session: no_session(),
            payload_digest: digest(3),
            parents: Vec::new(),
        }),
        StoredRecord::Peer(PeerRecord {
            peer: digest(30),
            joined_at: digest(1),
        }),
    ]
}

fn journal_of(records: &[StoredRecord]) -> Bytes {
    Bytes {
        held: records.iter().flat_map(frame_record).collect(),
    }
}

#[test]
fn the_daemon_rebuilds_the_index_before_it_serves_anything() {
    let mut store = Store::open(Accepting::default()).expect("opens");
    let mut journal = journal_of(&records());

    let diagnostic = recover_on_start(&mut store, &mut journal, None);

    assert!(diagnostic.is_serving());
    assert_eq!(diagnostic.severity(), Severity::Routine);
    assert_eq!(diagnostic.verdict(), BudgetVerdict::Inside);
    let RecoveryOutcome::Rebuilt {
        records, digest, ..
    } = diagnostic.outcome()
    else {
        panic!("expected a rebuild, found {:?}", diagnostic.outcome());
    };
    assert_eq!(*records, 2);
    assert_eq!(*digest, store.index().default_digest());
}

#[test]
fn the_acknowledged_digest_is_what_the_rebuild_is_held_against() {
    let mut store = Store::open(Accepting::default()).expect("opens");
    let mut journal = journal_of(&records());

    // The digest the rebuild will land on, taken from a recovery that is allowed to succeed.
    let told = {
        let mut rehearsal = Store::open(Accepting::default()).expect("opens");
        let mut copy = journal_of(&records());
        recover_on_start(&mut rehearsal, &mut copy, None);
        rehearsal.index().default_digest()
    };

    assert!(recover_on_start(&mut store, &mut journal, Some(told)).is_serving());

    let mut elsewhere = Store::open(Accepting::default()).expect("opens");
    let mut again = journal_of(&records());
    let diagnostic = recover_on_start(
        &mut elsewhere,
        &mut again,
        Some(mesh_store::Digest16::from_bytes([0xab; 16])),
    );
    assert!(!diagnostic.is_serving());
    assert_eq!(diagnostic.severity(), Severity::Blocking);
    assert!(diagnostic.to_string().contains("correctness defect"));
}

#[test]
fn a_damaged_journal_is_a_blocking_diagnostic_and_never_a_panic() {
    let mut store = Store::open(Accepting::default()).expect("opens");
    let mut journal = journal_of(&records());
    journal.held[40] ^= 0xff;

    let diagnostic = recover_on_start(&mut store, &mut journal, None);

    assert!(!diagnostic.is_serving());
    assert_eq!(diagnostic.severity(), Severity::Blocking);
    let RecoveryOutcome::Unrecoverable { detail } = diagnostic.outcome() else {
        panic!(
            "expected an unrecoverable journal, found {:?}",
            diagnostic.outcome()
        );
    };
    assert!(detail.contains("unrecoverable"), "{detail}");
    assert!(detail.contains("Nothing was skipped"), "{detail}");
}

#[test]
fn an_interrupted_save_is_reported_and_the_daemon_still_serves() {
    let mut store = Store::open(Accepting::default()).expect("opens");
    let mut journal = journal_of(&records());
    journal.held.truncate(journal.held.len() - 9);

    let diagnostic = recover_on_start(&mut store, &mut journal, None);

    assert!(diagnostic.is_serving());
    assert_eq!(diagnostic.severity(), Severity::Notable);
    let RecoveryOutcome::RebuiltAfterAnInterruptedSave {
        records,
        discarded_bytes,
        ..
    } = diagnostic.outcome()
    else {
        panic!(
            "expected an interrupted save, found {:?}",
            diagnostic.outcome()
        );
    };
    assert_eq!(*records, 1);
    assert!(*discarded_bytes > 0);
}

#[test]
fn every_start_up_leaves_one_entry_in_the_feed() {
    let mut feed = DiagnosticsFeed::new();
    for _ in 0..3 {
        let mut store = Store::open(Accepting::default()).expect("opens");
        let mut journal = journal_of(&records());
        feed.record(recover_on_start(&mut store, &mut journal, None));
    }
    assert_eq!(feed.entries().len(), 3);
    assert_eq!(feed.dropped(), 0);
    assert!(feed
        .latest()
        .expect("an entry")
        .elapsed()
        .lt(&Duration::from_secs(1)));
    assert_eq!(RECOVERY_BUDGET.limit, Duration::from_secs(5));
}
