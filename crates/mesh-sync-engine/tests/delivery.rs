//! Sender-side delivery state is durable record truth, not an in-memory retry queue.

use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use mesh_store::{
    journal_records, no_session, scan_journal, Checkpoint, OperationRecord, PeerRecord,
    RecordDigest, RecordJournal, Row, SqlExecutor, Sqlite, SqliteError, Store, Table,
};
use mesh_sync_engine::{
    persist_delivery_acknowledgement, persist_inbound_operation, DeliveryPersistenceError,
    DeliveryReceipt, InboundOperationReceipt, InboundPersistenceError,
};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

fn digest(seed: u8) -> RecordDigest {
    RecordDigest::from_bytes([seed; 32])
}

fn operation(id: u8, sequence: u64) -> OperationRecord {
    OperationRecord {
        id: digest(id),
        actor: digest(20),
        actor_sequence: sequence,
        hlc_millis: sequence,
        hlc_counter: 0,
        policy_epoch: 1,
        session: no_session(),
        payload_digest: digest(id.wrapping_add(100)),
        parents: (sequence > 1).then(|| digest(id - 1)).into_iter().collect(),
    }
}

fn seed() -> Checkpoint {
    Checkpoint {
        operations: vec![operation(1, 1), operation(2, 2), operation(3, 3)],
        peers: vec![PeerRecord {
            peer: digest(30),
            joined_at: digest(1),
        }],
        ..Checkpoint::default()
    }
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let ordinal = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "mesh-sync-{label}-{}-{ordinal}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create scratch directory");
        Self(path)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct FileJournal(PathBuf);

impl FileJournal {
    fn at(path: impl AsRef<Path>) -> Self {
        Self(path.as_ref().to_path_buf())
    }

    fn length(&self) -> u64 {
        fs::metadata(&self.0).map_or(0, |metadata| metadata.len())
    }
}

impl RecordJournal for FileJournal {
    type Error = io::Error;

    fn read_all(&mut self) -> Result<Vec<u8>, Self::Error> {
        match fs::read(&self.0) {
            Ok(bytes) => Ok(bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error),
        }
    }

    fn append(&mut self, framed: &[u8]) -> Result<(), Self::Error> {
        let mut file = OpenOptions::new().create(true).append(true).open(&self.0)?;
        file.write_all(framed)?;
        file.sync_data()
    }
}

struct RejectingJournal;

impl RecordJournal for RejectingJournal {
    type Error = io::Error;

    fn read_all(&mut self) -> Result<Vec<u8>, Self::Error> {
        Ok(Vec::new())
    }

    fn append(&mut self, _framed: &[u8]) -> Result<(), Self::Error> {
        Err(io::Error::other("injected durable append failure"))
    }
}

#[derive(Debug)]
enum InjectedSqlError {
    Driver(SqliteError),
    Commit,
}

impl std::fmt::Display for InjectedSqlError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Driver(error) => error.fmt(formatter),
            Self::Commit => formatter.write_str("injected SQLite commit failure"),
        }
    }
}

struct FailNextCommit {
    inner: Sqlite,
    armed: bool,
}

impl FailNextCommit {
    fn new(path: impl AsRef<Path>) -> Self {
        Self {
            inner: Sqlite::open(path).expect("open SQLite driver"),
            armed: false,
        }
    }

    fn arm(&mut self) {
        self.armed = true;
    }
}

impl SqlExecutor for FailNextCommit {
    type Error = InjectedSqlError;

    fn execute_batch(&mut self, sql: &str) -> Result<(), Self::Error> {
        if self.armed {
            self.armed = false;
            return Err(InjectedSqlError::Commit);
        }
        self.inner
            .execute_batch(sql)
            .map_err(InjectedSqlError::Driver)
    }

    fn read_table(&mut self, table: &Table) -> Result<Vec<Row>, Self::Error> {
        self.inner
            .read_table(table)
            .map_err(InjectedSqlError::Driver)
    }

    fn table_exists(&mut self, name: &str) -> Result<bool, Self::Error> {
        self.inner
            .table_exists(name)
            .map_err(InjectedSqlError::Driver)
    }
}

fn open(path: impl AsRef<Path>) -> Store<Sqlite> {
    Store::open(Sqlite::open(path).expect("open SQLite driver")).expect("open durable store")
}

fn persist_seed(store: &mut Store<Sqlite>, journal: &mut FileJournal) {
    let checkpoint = seed();
    journal_records(journal, &checkpoint.records()).expect("durably journal seed");
    store.commit(&checkpoint).expect("index seed");
}

#[test]
fn duplicate_and_reordered_receipts_drain_once_in_effect_across_restart() {
    let scratch = Scratch::new("delivery-restart");
    let mut journal = FileJournal::at(scratch.join("records.mesh"));
    let mut store = open(scratch.join("live.sqlite"));
    persist_seed(&mut store, &mut journal);

    let peer = digest(30);
    let actor = digest(20);
    assert_eq!(store.index().owed_to_peer(&peer).len(), 3);

    let first = persist_delivery_acknowledgement(&mut store, &mut journal, peer, actor, 2)
        .expect("persist first receipt");
    assert!(matches!(
        first,
        DeliveryReceipt::Advanced {
            from: 0,
            through: 2,
            remaining_operations: 1,
            ..
        }
    ));
    let durable_length = journal.length();
    let digest_after_first = store.index().default_digest();

    for through in [2, 1, 2] {
        assert_eq!(
            persist_delivery_acknowledgement(&mut store, &mut journal, peer, actor, through)
                .expect("retry is observable"),
            DeliveryReceipt::AlreadyAcknowledged {
                through: 2,
                remaining_operations: 1,
            }
        );
        assert_eq!(journal.length(), durable_length, "retry appended a record");
        assert_eq!(store.index().default_digest(), digest_after_first);
    }
    drop(store);

    let scan = scan_journal(&journal.read_all().expect("read journal")).expect("scan journal");
    let mut restarted = open(scratch.join("restart.sqlite"));
    restarted
        .rebuild_from(scan.into_records())
        .expect("rebuild delivery state");
    assert_eq!(
        restarted.index().acknowledged_through(&peer, &actor),
        Some(2)
    );
    assert_eq!(
        restarted.index().owed_to_peer(&peer),
        [digest(3)].into_iter().collect()
    );

    assert!(matches!(
        persist_delivery_acknowledgement(&mut restarted, &mut journal, peer, actor, 3)
            .expect("persist final receipt"),
        DeliveryReceipt::Advanced {
            from: 2,
            through: 3,
            remaining_operations: 0,
            ..
        }
    ));
    let drained_length = journal.length();
    assert!(matches!(
        persist_delivery_acknowledgement(&mut restarted, &mut journal, peer, actor, 3)
            .expect("retry final receipt"),
        DeliveryReceipt::AlreadyAcknowledged {
            through: 3,
            remaining_operations: 0
        }
    ));
    assert_eq!(journal.length(), drained_length);
    drop(restarted);

    let scan =
        scan_journal(&journal.read_all().expect("read journal again")).expect("scan journal again");
    let mut second_restart = open(scratch.join("second-restart.sqlite"));
    second_restart
        .rebuild_from(scan.into_records())
        .expect("rebuild fully drained outbox");
    assert_eq!(
        second_restart.index().acknowledged_through(&peer, &actor),
        Some(3)
    );
    assert!(second_restart.index().owed_to_peer(&peer).is_empty());
}

#[test]
fn a_failed_durable_append_cannot_optimistically_drain_the_outbox() {
    let scratch = Scratch::new("delivery-failure");
    let mut seed_journal = FileJournal::at(scratch.join("seed.mesh"));
    let mut store = open(scratch.join("live.sqlite"));
    persist_seed(&mut store, &mut seed_journal);

    let peer = digest(30);
    let actor = digest(20);
    let before = store.index().default_digest();
    let error = persist_delivery_acknowledgement(&mut store, &mut RejectingJournal, peer, actor, 2)
        .expect_err("failed journal append must be reported");

    assert!(matches!(error, DeliveryPersistenceError::Journal(_)));
    assert_eq!(store.index().default_digest(), before);
    assert_eq!(store.index().acknowledged_through(&peer, &actor), Some(0));
    assert_eq!(store.index().owed_to_peer(&peer).len(), 3);
}

#[test]
fn restart_finishes_an_acknowledgement_durable_before_the_index_failed() {
    let scratch = Scratch::new("delivery-index-gap");
    let mut journal = FileJournal::at(scratch.join("records.mesh"));
    let mut store =
        Store::open(FailNextCommit::new(scratch.join("live.sqlite"))).expect("open store");
    let checkpoint = seed();
    journal_records(&mut journal, &checkpoint.records()).expect("durably journal seed");
    store.commit(&checkpoint).expect("index seed");
    let before_append = journal.length();
    store.executor_mut().arm();

    let error =
        persist_delivery_acknowledgement(&mut store, &mut journal, digest(30), digest(20), 2)
            .expect_err("derived index failure must be explicit");
    assert!(matches!(
        error,
        DeliveryPersistenceError::IndexAfterJournal(_)
    ));
    assert!(journal.length() > before_append, "receipt was not durable");
    assert_eq!(
        store.index().acknowledged_through(&digest(30), &digest(20)),
        Some(0),
        "failed derived write changed the in-memory index"
    );
    drop(store);

    let scan = scan_journal(&journal.read_all().expect("read journal")).expect("scan journal");
    let mut restarted = open(scratch.join("restart.sqlite"));
    restarted
        .rebuild_from(scan.into_records())
        .expect("rebuild catches up from durable truth");
    assert_eq!(
        restarted
            .index()
            .acknowledged_through(&digest(30), &digest(20)),
        Some(2)
    );
    assert_eq!(restarted.index().owed_to_peer(&digest(30)).len(), 1);
}

#[test]
fn an_unknown_peer_never_creates_delivery_truth() {
    let scratch = Scratch::new("delivery-unknown-peer");
    let mut journal = FileJournal::at(scratch.join("records.mesh"));
    let mut store = open(scratch.join("live.sqlite"));
    let checkpoint = Checkpoint {
        operations: vec![operation(1, 1)],
        ..Checkpoint::default()
    };
    journal_records(&mut journal, &checkpoint.records()).expect("journal operation");
    store.commit(&checkpoint).expect("index operation");
    let before = journal.length();

    let error =
        persist_delivery_acknowledgement(&mut store, &mut journal, digest(99), digest(20), 1)
            .expect_err("unknown peer must be refused");

    assert!(matches!(error, DeliveryPersistenceError::UnknownPeer(peer) if peer == digest(99)));
    assert_eq!(journal.length(), before);
}

#[test]
fn a_receipt_beyond_the_local_actor_head_cannot_drop_future_outbox_entries() {
    let scratch = Scratch::new("delivery-impossible-receipt");
    let mut journal = FileJournal::at(scratch.join("records.mesh"));
    let mut store = open(scratch.join("live.sqlite"));
    persist_seed(&mut store, &mut journal);
    let before = journal.length();

    let error =
        persist_delivery_acknowledgement(&mut store, &mut journal, digest(30), digest(20), 4)
            .expect_err("sender cannot acknowledge a sequence it does not hold");

    assert!(matches!(
        error,
        DeliveryPersistenceError::UnsendableSequence {
            actor,
            requested: 4,
            available: Some(3)
        } if actor == digest(20)
    ));
    assert_eq!(journal.length(), before);
    assert_eq!(store.index().owed_to_peer(&digest(30)).len(), 3);
}

#[test]
fn child_before_parent_is_durable_but_not_ready_and_duplicates_have_zero_effect() {
    let scratch = Scratch::new("inbound-reorder");
    let mut journal = FileJournal::at(scratch.join("records.mesh"));
    let mut store = open(scratch.join("live.sqlite"));
    let child = operation(2, 2);
    let parent = operation(1, 1);

    assert_eq!(
        persist_inbound_operation(&mut store, &mut journal, child.clone()).expect("buffer child"),
        InboundOperationReceipt::Buffered {
            operation: digest(2),
            missing: [digest(1)].into_iter().collect(),
            journal_bytes: journal.length(),
        }
    );
    assert_eq!(store.index().operation_count(), 1);
    assert!(store.index().actor_head(&digest(20)).is_none());
    assert!(!store.index().is_causally_ready(&digest(2)));

    let buffered_length = journal.length();
    let buffered_digest = store.index().default_digest();
    assert_eq!(
        persist_inbound_operation(&mut store, &mut journal, child.clone())
            .expect("duplicate child"),
        InboundOperationReceipt::Duplicate {
            operation: digest(2),
            causally_ready: false,
        }
    );
    assert_eq!(journal.length(), buffered_length);
    assert_eq!(store.index().default_digest(), buffered_digest);

    assert!(matches!(
        persist_inbound_operation(&mut store, &mut journal, parent.clone())
            .expect("parent releases closure"),
        InboundOperationReceipt::Ready {
            operation,
            newly_ready,
            ..
        } if operation == digest(1) && newly_ready == vec![digest(1), digest(2)]
    ));
    assert_eq!(
        store.index().actor_head(&digest(20)).map(|head| head.id),
        Some(digest(2))
    );
    let ready_length = journal.length();
    let ready_digest = store.index().default_digest();
    assert_eq!(
        persist_inbound_operation(&mut store, &mut journal, child).expect("ready duplicate"),
        InboundOperationReceipt::Duplicate {
            operation: digest(2),
            causally_ready: true,
        }
    );
    assert_eq!(journal.length(), ready_length);
    assert_eq!(store.index().default_digest(), ready_digest);

    drop(store);
    let scan = scan_journal(&journal.read_all().expect("read journal")).expect("scan journal");
    let mut restarted = open(scratch.join("restart.sqlite"));
    restarted
        .rebuild_from(scan.into_records())
        .expect("rebuild reordered causal closure");
    assert_eq!(restarted.index().default_digest(), ready_digest);
    assert_eq!(
        restarted
            .index()
            .actor_head(&digest(20))
            .map(|head| head.id),
        Some(digest(2))
    );

    let mut clean_journal = FileJournal::at(scratch.join("clean.mesh"));
    let mut clean = open(scratch.join("clean.sqlite"));
    persist_inbound_operation(&mut clean, &mut clean_journal, parent).expect("parent first");
    persist_inbound_operation(&mut clean, &mut clean_journal, operation(2, 2))
        .expect("child second");
    assert_eq!(clean.index().default_digest(), ready_digest);
}

#[test]
fn restart_recovers_a_buffered_child_when_sqlite_failed_after_the_journal() {
    let scratch = Scratch::new("inbound-crash");
    let mut journal = FileJournal::at(scratch.join("records.mesh"));
    let mut store =
        Store::open(FailNextCommit::new(scratch.join("live.sqlite"))).expect("open store");
    store.executor_mut().arm();

    let error = persist_inbound_operation(&mut store, &mut journal, operation(2, 2))
        .expect_err("injected derived-index failure");
    assert!(matches!(
        error,
        InboundPersistenceError::IndexAfterJournal(_)
    ));
    assert!(journal.length() > 0);
    assert_eq!(store.index().operation_count(), 0);
    drop(store);

    let scan = scan_journal(&journal.read_all().expect("read journal")).expect("scan journal");
    let mut restarted = open(scratch.join("restart.sqlite"));
    restarted
        .rebuild_from(scan.into_records())
        .expect("rebuild buffered child");
    assert_eq!(store_buffer_state(&restarted), (1, false));

    assert!(matches!(
        persist_inbound_operation(&mut restarted, &mut journal, operation(1, 1))
            .expect("parent releases recovered child"),
        InboundOperationReceipt::Ready { newly_ready, .. }
            if newly_ready == vec![digest(1), digest(2)]
    ));
    assert_eq!(store_buffer_state(&restarted), (2, true));
}

fn store_buffer_state<E: SqlExecutor>(store: &Store<E>) -> (usize, bool) {
    (
        store.index().operation_count(),
        store.index().is_causally_ready(&digest(2)),
    )
}

#[test]
fn invalid_causal_parents_fail_before_journal_or_index_mutation() {
    let scratch = Scratch::new("inbound-invalid-causality");
    let mut journal = FileJournal::at(scratch.join("records.mesh"));
    let mut store = open(scratch.join("live.sqlite"));

    let mut self_parent = operation(1, 1);
    self_parent.parents = vec![self_parent.id];
    let error = persist_inbound_operation(&mut store, &mut journal, self_parent)
        .expect_err("self-parent must be refused");
    assert!(matches!(
        error,
        InboundPersistenceError::Preflight(mesh_store::FoldError::SelfCausalParent { .. })
    ));

    let mut repeated_parent = operation(2, 2);
    repeated_parent.parents = vec![digest(1), digest(1)];
    let error = persist_inbound_operation(&mut store, &mut journal, repeated_parent)
        .expect_err("duplicate parent must be refused");
    assert!(matches!(
        error,
        InboundPersistenceError::Preflight(mesh_store::FoldError::DuplicateCausalParent { .. })
    ));
    assert_eq!(journal.length(), 0);
    assert_eq!(store.index().operation_count(), 0);

    let mut first = operation(1, 1);
    first.parents = vec![digest(2)];
    persist_inbound_operation(&mut store, &mut journal, first.clone())
        .expect("buffer first cycle leg");
    let before_length = journal.length();
    let before_digest = store.index().default_digest();

    let mut conflicting_duplicate = first;
    conflicting_duplicate.policy_epoch = 99;
    let error = persist_inbound_operation(&mut store, &mut journal, conflicting_duplicate)
        .expect_err("same identifier with different content must be refused");
    assert!(matches!(
        error,
        InboundPersistenceError::Preflight(mesh_store::FoldError::ConflictingOperation { .. })
    ));
    assert_eq!(journal.length(), before_length);
    assert_eq!(store.index().default_digest(), before_digest);

    let mut second = operation(2, 2);
    second.parents = vec![digest(1)];
    let error = persist_inbound_operation(&mut store, &mut journal, second)
        .expect_err("completed causal cycle must be refused");
    assert!(matches!(
        error,
        InboundPersistenceError::Preflight(mesh_store::FoldError::CausalCycle { .. })
    ));
    assert_eq!(journal.length(), before_length);
    assert_eq!(store.index().default_digest(), before_digest);
    assert_eq!(store.index().operation_count(), 1);
    assert!(!store.index().is_causally_ready(&digest(1)));
}
