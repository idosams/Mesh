//! Durable fleet control events, separate from the reconstructable workspace index.
//!
//! The caller authorizes the private database location and command before calling this module.
//! A committed append is a scheduling decision, not proof that an external worker was launched.

use std::path::Path;
use std::sync::Arc;

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

const APPLICATION_ID: i64 = 0x4d_46_4c_54;
const SCHEMA_VERSION: i64 = 1;
/// Maximum metadata payload; file contents and credentials do not belong in this ledger.
pub const MAX_FLEET_EVENT_BYTES: usize = 65_536;
/// Maximum number of records returned by one replay page.
pub const MAX_FLEET_EVENT_PAGE: usize = 256;

/// An immutable command/event accepted for an objective stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FleetEvent {
    /// Objective identity, scoped by the service before storage access.
    pub stream: String,
    /// One-based contiguous revision within the stream.
    pub revision: u64,
    /// Client request identity, unique within this stream.
    pub request: String,
    /// Versioned control metadata, interpreted by the daemon.
    pub payload: String,
}

/// Result of this writer transaction, distinct from the durable event being recovered.
/// This is a storage fact, not authentication, policy approval or an execution permit.
#[derive(Debug, PartialEq, Eq)]
pub enum FleetAppendOutcome {
    /// This call inserted and committed the event, then passed the final authority check.
    Inserted(FleetEvent),
    /// An identical earlier event was recovered; no new external effect is authorized.
    Replayed(FleetEvent),
}
impl FleetAppendOutcome {
    /// Recover the ordinary durable receipt when the caller does not need insertion provenance.
    pub fn into_event(self) -> FleetEvent {
        match self {
            Self::Inserted(event) | Self::Replayed(event) => event,
        }
    }
}

/// Failure to read or atomically advance the control ledger.
#[derive(Debug)]
pub enum FleetStoreError {
    /// A bounded identity, payload, revision or page argument was invalid.
    InvalidInput,
    /// Another writer advanced the stream; reload before deciding again.
    StaleRevision {
        /// Actual committed revision.
        actual: u64,
    },
    /// A request identity was reused for a different command or expected revision.
    RequestConflict,
    /// The file belongs to another application or uses an unsupported schema.
    UnsupportedSchema,
    /// The native owner no longer recognizes this exact ledger location.
    AuthorityChanged,
    /// SQLite refused an operation. The transaction was not acknowledged.
    Database(rusqlite::Error),
}

impl std::fmt::Display for FleetStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput => f.write_str("invalid fleet ledger input"),
            Self::StaleRevision { actual } => write!(f, "fleet revision changed to {actual}"),
            Self::RequestConflict => f.write_str("fleet request identity was reused"),
            Self::UnsupportedSchema => f.write_str("unsupported fleet ledger schema"),
            Self::AuthorityChanged => f.write_str("fleet ledger authority changed"),
            Self::Database(error) => write!(f, "fleet ledger database operation failed: {error}"),
        }
    }
}

impl std::error::Error for FleetStoreError {}
impl From<rusqlite::Error> for FleetStoreError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Database(value)
    }
}

/// Native authority retained for the lifetime of a ledger. A check grants no actor or run rights.
/// Implementations must reject changed directory/file identities and aliases without repairing them.
pub trait FleetStoreAuthority: Send + Sync + std::fmt::Debug {
    /// Verify the location before and after each database operation.
    fn check(&self) -> Result<(), FleetStoreError>;
}

/// SQLite-backed ordered event streams with transactional idempotency and revision checks.
///
/// Open only at a service-authorized private location. This type does not authenticate callers
/// or defend an arbitrary user-supplied path against substitution.
#[derive(Debug)]
pub struct FleetStore {
    connection: Connection,
    authority: Option<Arc<dyn FleetStoreAuthority>>,
}

impl FleetStore {
    /// Open or initialize a fleet ledger. Unknown formats are never migrated implicitly.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, FleetStoreError> {
        let path = path.as_ref();
        if !path.is_absolute() {
            return Err(FleetStoreError::InvalidInput);
        }
        // Existing callers may use OS parent aliases such as macOS /var. Resolve only the parent;
        // the final ledger entry must still pass SQLite's no-follow check.
        let parent = path
            .parent()
            .ok_or(FleetStoreError::InvalidInput)?
            .canonicalize()
            .map_err(|_| FleetStoreError::InvalidInput)?;
        let name = path.file_name().ok_or(FleetStoreError::InvalidInput)?;
        Self::open_inner(&parent.join(name), true, None)
    }

    /// Open a native-bound ledger. Only initial allocation may initialize an empty file;
    /// reopening refuses missing files and empty/unknown schemas instead of fabricating history.
    /// `path` must be a native-resolved name or stable directory reference with no symbolic aliases.
    pub fn open_guarded(
        path: &Path,
        initialize: bool,
        authority: Arc<dyn FleetStoreAuthority>,
    ) -> Result<Self, FleetStoreError> {
        Self::open_inner(path, initialize, Some(authority))
    }

    /// Open an independent connection to this exact native-guarded ledger. The retained native
    /// authority is shared, including its lifetime pins; no caller-selected path or initialization
    /// is accepted. This does not copy a transaction, authenticate an actor or adopt execution.
    /// Callers must replay current events and retain ordinary expected-revision checks.
    pub fn reopen_guarded_connection(&self) -> Result<Self, FleetStoreError> {
        let authority = self
            .authority
            .as_ref()
            .ok_or(FleetStoreError::AuthorityChanged)?;
        authority.check()?;
        let path = self
            .connection
            .path()
            .filter(|path| !path.is_empty())
            .ok_or(FleetStoreError::InvalidInput)?;
        Self::open_inner(Path::new(path), false, Some(Arc::clone(authority)))
    }

    fn check_authority(&self) -> Result<(), FleetStoreError> {
        check_authority(self.authority.as_deref())
    }

    fn open_inner(
        path: &Path,
        initialize: bool,
        authority: Option<Arc<dyn FleetStoreAuthority>>,
    ) -> Result<Self, FleetStoreError> {
        check_authority(authority.as_deref())?;
        // Relative SQLite names can select temporary or URI-backed in-memory databases. A
        // durable control ledger requires an absolute native filename selected by the service.
        if !path.is_absolute() {
            return Err(FleetStoreError::InvalidInput);
        }
        let mut flags = rusqlite::OpenFlags::default() | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW;
        if !initialize {
            flags.remove(rusqlite::OpenFlags::SQLITE_OPEN_CREATE);
        }
        let mut connection = Connection::open_with_flags(path, flags)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        // Check identity before changing the file's persistent journal mode.
        {
            let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let version: i64 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
            let app: i64 = tx.pragma_query_value(None, "application_id", |r| r.get(0))?;
            let tables: i64 = tx.query_row(
                "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
                [],
                |r| r.get(0),
            )?;
            if initialize && version == 0 && app == 0 && tables == 0 {
                tx.execute_batch(
                    "CREATE TABLE fleet_events (
                        stream TEXT NOT NULL,
                        revision INTEGER NOT NULL CHECK(revision > 0),
                        request TEXT NOT NULL,
                        payload TEXT NOT NULL,
                        PRIMARY KEY(stream, revision),
                        UNIQUE(stream, request)
                    ) STRICT;",
                )?;
                tx.pragma_update(None, "application_id", APPLICATION_ID)?;
                tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            } else if version != SCHEMA_VERSION || app != APPLICATION_ID {
                return Err(FleetStoreError::UnsupportedSchema);
            }
            check_authority(authority.as_deref())?;
            tx.commit()?;
        }
        let mode: String = connection.pragma_query_value(None, "journal_mode", |r| r.get(0))?;
        if mode != "wal" {
            connection.pragma_update(None, "journal_mode", "WAL")?;
        }
        connection.pragma_update(None, "synchronous", "FULL")?;
        check_authority(authority.as_deref())?;
        Ok(Self {
            connection,
            authority,
        })
    }

    /// Current committed revision, or zero for an objective with no events.
    pub fn revision(&self, stream: &str) -> Result<u64, FleetStoreError> {
        self.check_authority()?;
        valid_id(stream)?;
        let revision: u64 = self.connection.query_row(
            "SELECT coalesce(max(revision), 0) FROM fleet_events WHERE stream = ?1",
            [stream],
            |r| r.get(0),
        )?;
        self.check_authority()?;
        Ok(revision)
    }

    /// Find an already committed request before validating a retry against newer state.
    pub fn request(
        &self,
        stream: &str,
        request: &str,
    ) -> Result<Option<FleetEvent>, FleetStoreError> {
        self.check_authority()?;
        valid_id(stream)?;
        valid_id(request)?;
        let result = self
            .connection
            .query_row(
                "SELECT revision, payload FROM fleet_events WHERE stream = ?1 AND request = ?2",
                params![stream, request],
                |r| {
                    Ok(FleetEvent {
                        stream: stream.to_owned(),
                        revision: r.get(0)?,
                        request: request.to_owned(),
                        payload: r.get(1)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into);
        self.check_authority()?;
        result
    }

    /// Append exactly once at `expected_revision`, or return the identical earlier append.
    ///
    /// Request lookup, revision comparison and insert share one writer transaction. A response
    /// lost after commit can be recovered with the original arguments, even after later appends.
    pub fn append(
        &mut self,
        stream: &str,
        expected_revision: u64,
        request: &str,
        payload: &str,
    ) -> Result<FleetEvent, FleetStoreError> {
        self.append_with_outcome(stream, expected_revision, request, payload)
            .map(FleetAppendOutcome::into_event)
    }

    /// Append with an atomic distinction between a new commit and recovery of an old receipt.
    /// The distinction is made under the same writer transaction as lookup and insertion; a
    /// separate caller-side lookup cannot safely replace it. A post-commit authority failure
    /// returns an error, not Inserted, and a later identical retry is Replayed. Callers must
    /// retain that uncertainty rather than interpreting replay as a second grant to execute.
    pub fn append_with_outcome(
        &mut self,
        stream: &str,
        expected_revision: u64,
        request: &str,
        payload: &str,
    ) -> Result<FleetAppendOutcome, FleetStoreError> {
        self.check_authority()?;
        valid_id(stream)?;
        valid_id(request)?;
        if payload.is_empty()
            || payload.len() > MAX_FLEET_EVENT_BYTES
            || expected_revision >= i64::MAX as u64
        {
            return Err(FleetStoreError::InvalidInput);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<(u64, String)> = tx
            .query_row(
                "SELECT revision, payload FROM fleet_events WHERE stream = ?1 AND request = ?2",
                params![stream, request],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((revision, saved)) = existing {
            if revision != expected_revision + 1 || saved != payload {
                return Err(FleetStoreError::RequestConflict);
            }
            check_authority(self.authority.as_deref())?;
            return Ok(FleetAppendOutcome::Replayed(FleetEvent {
                stream: stream.to_owned(),
                revision,
                request: request.to_owned(),
                payload: saved,
            }));
        }
        let actual: u64 = tx.query_row(
            "SELECT coalesce(max(revision), 0) FROM fleet_events WHERE stream = ?1",
            [stream],
            |r| r.get(0),
        )?;
        if actual != expected_revision {
            return Err(FleetStoreError::StaleRevision { actual });
        }
        let revision = actual + 1;
        tx.execute(
            "INSERT INTO fleet_events(stream, revision, request, payload) VALUES (?1, ?2, ?3, ?4)",
            params![stream, revision, request, payload],
        )?;
        check_authority(self.authority.as_deref())?;
        tx.commit()?;
        self.check_authority()?;
        Ok(FleetAppendOutcome::Inserted(FleetEvent {
            stream: stream.to_owned(),
            revision,
            request: request.to_owned(),
            payload: payload.to_owned(),
        }))
    }

    /// Read a bounded ordered page after the supplied cursor. Events are never consumed by reads.
    pub fn events(
        &self,
        stream: &str,
        after: u64,
        limit: usize,
    ) -> Result<Vec<FleetEvent>, FleetStoreError> {
        self.check_authority()?;
        valid_id(stream)?;
        if after > i64::MAX as u64 || limit == 0 || limit > MAX_FLEET_EVENT_PAGE {
            return Err(FleetStoreError::InvalidInput);
        }
        let mut statement = self.connection.prepare(
            "SELECT revision, request, payload FROM fleet_events
             WHERE stream = ?1 AND revision > ?2 ORDER BY revision LIMIT ?3",
        )?;
        let records = statement.query_map(params![stream, after, limit as u64], |r| {
            Ok(FleetEvent {
                stream: stream.to_owned(),
                revision: r.get(0)?,
                request: r.get(1)?,
                payload: r.get(2)?,
            })
        })?;
        let result = records.collect::<Result<Vec<_>, _>>().map_err(Into::into);
        self.check_authority()?;
        result
    }
}

fn check_authority(authority: Option<&dyn FleetStoreAuthority>) -> Result<(), FleetStoreError> {
    authority.map_or(Ok(()), FleetStoreAuthority::check)
}

fn valid_id(id: &str) -> Result<(), FleetStoreError> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
    {
        Err(FleetStoreError::InvalidInput)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Barrier,
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Directory(std::path::PathBuf);
    impl Directory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "mesh-fleet-store-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn db(&self) -> std::path::PathBuf {
            self.0.join("fleet.sqlite")
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn lost_reply_replays_original_after_restart_and_later_commands() {
        let dir = Directory::new();
        let first = {
            let mut store = FleetStore::open(dir.db()).unwrap();
            let first = store
                .append("objective", 0, "request-1", "create-lane")
                .unwrap();
            store
                .append("objective", 1, "request-2", "queue-run")
                .unwrap();
            first
        };
        let mut reopened = FleetStore::open(dir.db()).unwrap();
        assert_eq!(
            reopened
                .append("objective", 0, "request-1", "create-lane")
                .unwrap(),
            first
        );
        assert_eq!(reopened.revision("objective").unwrap(), 2);
        assert_eq!(reopened.events("objective", 0, 1).unwrap(), vec![first]);
        assert_eq!(
            reopened.events("objective", 1, 1).unwrap()[0].payload,
            "queue-run"
        );
    }

    #[test]
    fn changed_retry_and_stale_writer_never_append() {
        let dir = Directory::new();
        let mut store = FleetStore::open(dir.db()).unwrap();
        store.append("o", 0, "r", "one").unwrap();
        assert!(matches!(
            store.append("o", 0, "r", "two"),
            Err(FleetStoreError::RequestConflict)
        ));
        assert!(matches!(
            store.append("o", 1, "r", "one"),
            Err(FleetStoreError::RequestConflict)
        ));
        assert!(matches!(
            store.append("o", 0, "another", "one"),
            Err(FleetStoreError::StaleRevision { actual: 1 })
        ));
        assert_eq!(store.revision("o").unwrap(), 1);
        store.append("other-objective", 0, "r", "one").unwrap();
    }

    #[test]
    fn concurrent_schedulers_cannot_both_claim_the_same_revision() {
        let dir = Directory::new();
        let a = FleetStore::open(dir.db()).unwrap();
        let b = FleetStore::open(dir.db()).unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let handles: Vec<_> = [a, b]
            .into_iter()
            .enumerate()
            .map(|(i, mut store)| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    store.append("o", 0, &format!("request-{i}"), "dispatch")
                })
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|r| matches!(r, Err(FleetStoreError::StaleRevision { actual: 1 })))
                .count(),
            1
        );
        assert_eq!(
            FleetStore::open(dir.db()).unwrap().revision("o").unwrap(),
            1
        );
    }

    #[test]
    fn identical_concurrent_requests_report_one_insert_and_one_replay() {
        let dir = Directory::new();
        let stores = [
            FleetStore::open(dir.db()).unwrap(),
            FleetStore::open(dir.db()).unwrap(),
        ];
        let barrier = Arc::new(Barrier::new(2));
        let handles: Vec<_> = stores
            .into_iter()
            .map(|mut store| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    store.append_with_outcome("o", 0, "same-request", "launch-intent")
                })
            })
            .collect();
        let results: Vec<_> = handles
            .into_iter()
            .map(|h| h.join().unwrap().unwrap())
            .collect();
        assert_eq!(
            results
                .iter()
                .filter(|r| matches!(r, FleetAppendOutcome::Inserted(_)))
                .count(),
            1
        );
        assert_eq!(
            results
                .iter()
                .filter(|r| matches!(r, FleetAppendOutcome::Replayed(_)))
                .count(),
            1
        );
        let events: Vec<_> = results
            .into_iter()
            .map(FleetAppendOutcome::into_event)
            .collect();
        assert_eq!(events[0], events[1]);
        assert_eq!(
            FleetStore::open(dir.db()).unwrap().revision("o").unwrap(),
            1
        );
    }

    #[test]
    fn lost_insert_reply_is_only_replayed_after_restart_and_later_writes() {
        let dir = Directory::new();
        {
            let mut store = FleetStore::open(dir.db()).unwrap();
            assert!(matches!(
                store.append_with_outcome("o", 0, "intent", "launch"),
                Ok(FleetAppendOutcome::Inserted(_))
            ));
            store.append("o", 1, "later", "observation").unwrap();
        }
        let mut reopened = FleetStore::open(dir.db()).unwrap();
        let outcome = reopened
            .append_with_outcome("o", 0, "intent", "launch")
            .unwrap();
        assert!(matches!(outcome, FleetAppendOutcome::Replayed(_)));
        assert_eq!(outcome.into_event().revision, 1);
        assert!(matches!(
            reopened.append_with_outcome("o", 0, "intent", "changed"),
            Err(FleetStoreError::RequestConflict)
        ));
        assert_eq!(reopened.revision("o").unwrap(), 2);
    }

    #[derive(Debug)]
    struct RevokeAfterDurableCommit {
        path: std::path::PathBuf,
        armed: std::sync::atomic::AtomicBool,
    }
    impl FleetStoreAuthority for RevokeAfterDurableCommit {
        fn check(&self) -> Result<(), FleetStoreError> {
            if !self.armed.load(Ordering::SeqCst) {
                return Ok(());
            }
            // Another SQLite connection observes only durable committed rows, not this writer's
            // uncommitted insert. Revoke based on independent evidence, not a check-call counter.
            let connection = Connection::open_with_flags(
                &self.path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?;
            let count: u64 =
                connection.query_row("SELECT count(*) FROM fleet_events", [], |row| row.get(0))?;
            if count > 0 {
                Err(FleetStoreError::AuthorityChanged)
            } else {
                Ok(())
            }
        }
    }
    #[test]
    fn post_commit_authority_loss_returns_no_insert_outcome_and_retry_only_replays() {
        let dir = Directory::new();
        let path = dir.0.canonicalize().unwrap().join("fleet.sqlite");
        let authority = Arc::new(RevokeAfterDurableCommit {
            path: path.clone(),
            armed: std::sync::atomic::AtomicBool::new(false),
        });
        let mut store = FleetStore::open_guarded(&path, true, authority.clone()).unwrap();
        authority.armed.store(true, Ordering::SeqCst);
        assert!(matches!(
            store.append_with_outcome("o", 0, "intent", "launch"),
            Err(FleetStoreError::AuthorityChanged)
        ));
        drop(store);
        // The test's independently admitted fixture can inspect the committed event. Production
        // must reestablish native authority first; this does not authorize bypassing a failed guard.
        let mut inspected = FleetStore::open(path).unwrap();
        assert_eq!(inspected.revision("o").unwrap(), 1);
        assert!(matches!(
            inspected.append_with_outcome("o", 0, "intent", "launch"),
            Ok(FleetAppendOutcome::Replayed(_))
        ));
    }

    #[test]
    fn unknown_schema_and_unrelated_database_are_preserved() {
        let dir = Directory::new();
        let connection = Connection::open(dir.db()).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE unrelated(value TEXT); INSERT INTO unrelated VALUES ('keep');",
            )
            .unwrap();
        assert!(matches!(
            FleetStore::open(dir.db()),
            Err(FleetStoreError::UnsupportedSchema)
        ));
        assert_eq!(
            connection
                .query_row("SELECT value FROM unrelated", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "keep"
        );
        let other = dir.0.join("future.sqlite");
        drop(FleetStore::open(&other).unwrap());
        let future = Connection::open(&other).unwrap();
        future.pragma_update(None, "user_version", 99).unwrap();
        assert!(matches!(
            FleetStore::open(&other),
            Err(FleetStoreError::UnsupportedSchema)
        ));
    }

    #[test]
    fn bounds_reject_before_any_state_change() {
        let dir = Directory::new();
        for path in [
            "",
            ":memory:",
            "file::memory:?cache=shared",
            "relative.sqlite",
        ] {
            assert!(matches!(
                FleetStore::open(path),
                Err(FleetStoreError::InvalidInput)
            ));
        }
        let mut store = FleetStore::open(dir.db()).unwrap();
        for id in ["", "bad/identity", "contains space"] {
            assert!(matches!(
                store.append(id, 0, "r", "p"),
                Err(FleetStoreError::InvalidInput)
            ));
        }
        assert!(matches!(
            store.append("o", 0, "r", &"x".repeat(MAX_FLEET_EVENT_BYTES + 1)),
            Err(FleetStoreError::InvalidInput)
        ));
        assert!(matches!(
            store.append("o", u64::MAX, "r", "p"),
            Err(FleetStoreError::InvalidInput)
        ));
        assert!(matches!(
            store.events("o", 0, MAX_FLEET_EVENT_PAGE + 1),
            Err(FleetStoreError::InvalidInput)
        ));
        assert_eq!(store.revision("o").unwrap(), 0);
    }
    #[test]
    fn abrupt_process_exit_preserves_acknowledged_events_and_rolls_back_partial_append() {
        let dir = Directory::new();
        for mode in ["committed", "uncommitted"] {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "fleet::tests::crash_writer_helper",
                    "--nocapture",
                ])
                .env("MESH_FLEET_TEST_DB", dir.db())
                .env("MESH_FLEET_TEST_EXIT", mode)
                .status()
                .unwrap();
            assert_eq!(status.code(), Some(86));
            let store = FleetStore::open(dir.db()).unwrap();
            assert_eq!(store.revision("o").unwrap(), 1);
            assert_eq!(store.events("o", 0, 2).unwrap()[0].payload, "acknowledged");
        }
    }

    #[test]
    fn crash_writer_helper() {
        let Some(path) = std::env::var_os("MESH_FLEET_TEST_DB") else {
            return;
        };
        let mut store = FleetStore::open(std::path::PathBuf::from(path)).unwrap();
        if std::env::var("MESH_FLEET_TEST_EXIT").unwrap() == "committed" {
            store.append("o", 0, "committed", "acknowledged").unwrap();
        } else {
            store.connection.execute_batch(
                "BEGIN IMMEDIATE; INSERT INTO fleet_events VALUES ('o', 2, 'partial', 'never acknowledged');"
            ).unwrap();
        }
        // No unwinding, transaction drop, or Connection drop: exercise SQLite crash recovery.
        std::process::exit(86);
    }

    #[derive(Debug)]
    struct BudgetAuthority(AtomicU64);
    impl FleetStoreAuthority for BudgetAuthority {
        fn check(&self) -> Result<(), FleetStoreError> {
            self.0
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                    remaining.checked_sub(1)
                })
                .map(|_| ())
                .map_err(|_| FleetStoreError::AuthorityChanged)
        }
    }

    #[test]
    fn independent_guarded_connections_share_commits_revision_checks_and_authority() {
        let dir = Directory::new();
        let db = dir.0.canonicalize().unwrap().join("fleet.sqlite");
        let authority = Arc::new(BudgetAuthority(AtomicU64::new(1000)));
        let mut first = FleetStore::open_guarded(&db, true, authority.clone()).unwrap();
        let mut second = first.reopen_guarded_connection().unwrap();
        first.append("objective", 0, "first", "saved").unwrap();
        assert_eq!(second.revision("objective").unwrap(), 1);
        assert!(matches!(
            second.append("objective", 0, "competing", "refused"),
            Err(FleetStoreError::StaleRevision { actual: 1 })
        ));
        assert!(matches!(
            second
                .append_with_outcome("objective", 0, "first", "saved")
                .unwrap(),
            FleetAppendOutcome::Replayed(_)
        ));
        second.append("objective", 1, "second", "received").unwrap();
        assert_eq!(first.revision("objective").unwrap(), 2);
        drop(first);
        assert_eq!(second.events("objective", 0, 4).unwrap().len(), 2);
        // The second connection owns the same authority even after its source is dropped.
        assert_eq!(Arc::strong_count(&authority), 2);
        authority.0.store(0, Ordering::SeqCst);
        assert!(matches!(
            second.revision("objective"),
            Err(FleetStoreError::AuthorityChanged)
        ));
        assert!(matches!(
            second.reopen_guarded_connection(),
            Err(FleetStoreError::AuthorityChanged)
        ));
        assert!(matches!(
            second.append("objective", 2, "third", "refused"),
            Err(FleetStoreError::AuthorityChanged)
        ));
        authority.0.store(100, Ordering::SeqCst);
        assert_eq!(second.revision("objective").unwrap(), 2);
    }

    #[test]
    fn independent_connection_refuses_unguarded_or_missing_history() {
        let dir = Directory::new();
        let db = dir.0.canonicalize().unwrap().join("fleet.sqlite");
        let unguarded = FleetStore::open(&db).unwrap();
        assert!(matches!(
            unguarded.reopen_guarded_connection(),
            Err(FleetStoreError::AuthorityChanged)
        ));
        drop(unguarded);
        let authority = Arc::new(BudgetAuthority(AtomicU64::new(100)));
        let store = FleetStore::open_guarded(&db, false, authority).unwrap();
        std::fs::rename(&db, db.with_extension("preserved")).unwrap();
        assert!(store.reopen_guarded_connection().is_err());
        assert!(
            !db.exists(),
            "a new connection cannot recreate missing history"
        );
    }

    #[cfg(unix)]
    #[test]
    fn independent_connection_retains_native_file_identity_refusal() {
        use std::os::unix::fs::MetadataExt as _;
        #[derive(Debug)]
        struct Identity {
            path: std::path::PathBuf,
            device: u64,
            inode: u64,
        }
        impl FleetStoreAuthority for Identity {
            fn check(&self) -> Result<(), FleetStoreError> {
                let m = std::fs::symlink_metadata(&self.path)
                    .map_err(|_| FleetStoreError::AuthorityChanged)?;
                if m.is_file() && m.dev() == self.device && m.ino() == self.inode {
                    Ok(())
                } else {
                    Err(FleetStoreError::AuthorityChanged)
                }
            }
        }
        let dir = Directory::new();
        let db = dir.0.canonicalize().unwrap().join("fleet.sqlite");
        drop(FleetStore::open(&db).unwrap());
        let metadata = std::fs::metadata(&db).unwrap();
        let authority = Arc::new(Identity {
            path: db.clone(),
            device: metadata.dev(),
            inode: metadata.ino(),
        });
        let first = FleetStore::open_guarded(&db, false, authority).unwrap();
        let second = first.reopen_guarded_connection().unwrap();
        std::fs::rename(&db, db.with_extension("preserved")).unwrap();
        std::fs::write(&db, b"replacement must remain untouched").unwrap();
        for connection in [&first, &second] {
            assert!(matches!(
                connection.reopen_guarded_connection(),
                Err(FleetStoreError::AuthorityChanged)
            ));
            assert!(matches!(
                connection.revision("objective"),
                Err(FleetStoreError::AuthorityChanged)
            ));
        }
        assert_eq!(
            std::fs::read(&db).unwrap(),
            b"replacement must remain untouched"
        );
    }

    #[test]
    fn guarded_reopen_never_initializes_missing_or_empty_history() {
        let dir = Directory::new();
        let db = dir.0.canonicalize().unwrap().join("fleet.sqlite");
        let authority = Arc::new(BudgetAuthority(AtomicU64::new(100)));
        assert!(FleetStore::open_guarded(&db, false, authority.clone()).is_err());
        assert!(!db.exists());
        std::fs::write(&db, []).unwrap();
        assert!(matches!(
            FleetStore::open_guarded(&db, false, authority.clone()),
            Err(FleetStoreError::UnsupportedSchema)
        ));
        assert_eq!(std::fs::metadata(&db).unwrap().len(), 0);
        let mut store = FleetStore::open_guarded(&db, true, authority.clone()).unwrap();
        store.append("objective", 0, "first", "saved").unwrap();
        drop(store);
        let store = FleetStore::open_guarded(&db, false, authority).unwrap();
        assert_eq!(store.revision("objective").unwrap(), 1);
    }

    #[test]
    fn authority_loss_after_commit_retains_the_event_for_exact_retry_after_reopen() {
        let dir = Directory::new();
        let db = dir.0.canonicalize().unwrap().join("fleet.sqlite");
        let authority = Arc::new(BudgetAuthority(AtomicU64::new(100)));
        let mut store = FleetStore::open_guarded(&db, true, authority.clone()).unwrap();
        store.append("objective", 0, "first", "saved").unwrap();
        // Permit admission and the precommit check, then refuse acknowledgment after commit.
        authority.0.store(2, Ordering::SeqCst);
        assert!(matches!(
            store.append("objective", 1, "second", "durable-but-unacknowledged"),
            Err(FleetStoreError::AuthorityChanged)
        ));
        drop(store);
        authority.0.store(100, Ordering::SeqCst);
        let mut reopened = FleetStore::open_guarded(&db, false, authority).unwrap();
        assert_eq!(reopened.revision("objective").unwrap(), 2);
        let retry = reopened
            .append("objective", 1, "second", "durable-but-unacknowledged")
            .unwrap();
        assert_eq!(retry.revision, 2);
        assert_eq!(retry.payload, "durable-but-unacknowledged");
        assert_eq!(reopened.events("objective", 0, 4).unwrap().len(), 2);
        assert!(matches!(
            reopened.append("objective", 1, "second", "different"),
            Err(FleetStoreError::RequestConflict)
        ));
        assert_eq!(reopened.revision("objective").unwrap(), 2);
    }

    #[test]
    fn lost_native_authority_refuses_reads_and_rolls_back_before_append_commit() {
        let dir = Directory::new();
        let db = dir.0.canonicalize().unwrap().join("fleet.sqlite");
        let authority = Arc::new(BudgetAuthority(AtomicU64::new(100)));
        let mut store = FleetStore::open_guarded(&db, true, authority.clone()).unwrap();
        store.append("objective", 0, "first", "saved").unwrap();
        authority.0.store(1, Ordering::SeqCst);
        assert!(matches!(
            store.append("objective", 1, "second", "refused"),
            Err(FleetStoreError::AuthorityChanged)
        ));
        authority.0.store(100, Ordering::SeqCst);
        assert_eq!(store.revision("objective").unwrap(), 1);
        for action in 0..3 {
            authority.0.store(1, Ordering::SeqCst);
            let refused = match action {
                0 => store.revision("objective").is_err(),
                1 => store.request("objective", "first").is_err(),
                _ => store.events("objective", 0, 2).is_err(),
            };
            assert!(refused, "authority must be rechecked after reading");
        }
    }
}
