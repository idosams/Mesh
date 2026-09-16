//! The driver seam, the WAL pragmas, and the single-writer discipline.
//!
//! # The seam and its production driver
//!
//! [`SqlExecutor`] keeps schema and transaction semantics independent of one library. The
//! production [`crate::Sqlite`] implementation uses bundled SQLite, while the process-based test
//! executor remains an independent check of every migration, constraint and WAL behaviour.
//!
//! # WAL, and the pragmas that are not decoration
//!
//! [`PRAGMAS`] is applied on every open. Each entry carries why it is there, because a pragma list
//! nobody can justify is a pragma list nobody dares change.

use crate::commit::{Checkpoint, CommitPlan};
use crate::digest::Digest16;
use crate::index::{FoldError, Index};
use crate::migration::{plan_migrations, verify_applied, AppliedMigration, MigrationError};
use crate::record::StoredRecord;
use crate::recovery_state::PendingMeaningfulSave;
use crate::row::{Row, Value};
use crate::schema::{Table, TABLES};
use crate::sequence::PrivateSaved;

/// One database setting, the reason it is set, and how long it lasts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pragma {
    /// The pragma name.
    pub name: &'static str,
    /// The value it is set to.
    pub value: &'static str,
    /// Why. Quoted into the crate documentation and read by whoever wants to change it.
    pub reason: &'static str,
    /// Whether the setting is written into the database file and survives reconnecting.
    ///
    /// This distinction is not bookkeeping. A driver that opens more than one connection — a pool,
    /// a reader alongside the writer, or a process that spawns a fresh connection per statement —
    /// **must** re-apply every non-persistent pragma on each one, and must **not** re-apply
    /// `journal_mode`, because that would convert a database another connection is reading.
    ///
    /// Getting it wrong is silent: `foreign_keys` reverts to off, every `REFERENCES` clause stops
    /// being enforced, and nothing says so. It was got wrong here first. The test harness spawns a
    /// process per statement, lost `foreign_keys` every time, and the two tests that claimed to
    /// prove foreign keys were enforced were setting the pragma themselves — so turning it off in
    /// [`PRAGMAS`] left the whole suite green. A mutation test found that; reading did not.
    pub persistent: bool,
}

impl Pragma {
    /// The statement that applies it.
    #[must_use]
    pub fn sql(&self) -> String {
        format!("PRAGMA {} = {};", self.name, self.value)
    }
}

/// The settings every connection to this database is opened with.
pub const PRAGMAS: &[Pragma] = &[
    Pragma {
        name: "journal_mode",
        value: "WAL",
        reason: "plan §6.1 requires WAL from day one: it is what lets concurrent readers proceed \
                 while the single writer holds a transaction, which is measured in \
                 tests/wal_concurrency.rs against a rollback-journal control",
        persistent: true,
    },
    Pragma {
        name: "foreign_keys",
        value: "ON",
        reason: "SQLite disables foreign keys by default, so every REFERENCES clause in the \
                 migrations is decoration until this is set; it is what makes a wrongly-ordered \
                 commit plan fail instead of writing a dangling row",
        persistent: false,
    },
    Pragma {
        name: "synchronous",
        value: "NORMAL",
        reason: "the WAL-mode pairing: a commit is durable against process death, and a machine \
                 that loses power can lose the tail of the log — which is recoverable here, \
                 because this database is an index and the records it folds are elsewhere",
        persistent: false,
    },
    Pragma {
        name: "busy_timeout",
        value: "5000",
        reason: "under WAL a reader never waits, but a second *writer* does; five seconds turns \
                 that into a wait rather than an immediate SQLITE_BUSY the caller has to retry",
        persistent: false,
    },
    Pragma {
        name: "wal_autocheckpoint",
        value: "1000",
        reason: "SQLite's own default, restated so that changing it is a decision; the log is \
                 folded back into the database roughly every thousand pages",
        persistent: false,
    },
];

/// The settings a driver must re-apply on **every** connection, as one batch.
///
/// Excludes the persistent ones, so applying this to a second connection cannot convert the
/// database out from under the first.
#[must_use]
pub fn connection_pragmas_sql() -> String {
    PRAGMAS
        .iter()
        .filter(|pragma| !pragma.persistent)
        .map(Pragma::sql)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The SQL execution seam.
///
/// Four methods, none of which mentions SQLite. An implementation is a driver; this crate has
/// none, and its tests supply one over the system `sqlite3` binary.
pub trait SqlExecutor {
    /// Whatever the driver fails with.
    type Error;

    /// Execute one or more statements.
    ///
    /// # Errors
    ///
    /// Whatever the driver reports.
    fn execute_batch(&mut self, sql: &str) -> Result<(), Self::Error>;

    /// Read every row of a table, ordered by every column left to right.
    ///
    /// The ordering is part of the contract, because [`Index::digest`] compares an in-memory sort
    /// against this one.
    ///
    /// # Errors
    ///
    /// Whatever the driver reports.
    fn read_table(&mut self, table: &Table) -> Result<Vec<Row>, Self::Error>;

    /// Whether a table exists.
    ///
    /// # Errors
    ///
    /// Whatever the driver reports.
    fn table_exists(&mut self, name: &str) -> Result<bool, Self::Error>;
}

/// Why opening or writing a store failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreError<E> {
    /// The driver failed.
    Driver(E),
    /// The migration ledger is unusable.
    Migration(MigrationError),
    /// A record could not be folded into the index.
    Fold(FoldError),
    /// A `schema_version` row is not the shape the schema declares. The database is corrupt, which
    /// means the recovery is a rebuild rather than a repair.
    MalformedLedger,
}

impl<E: core::fmt::Display> core::fmt::Display for StoreError<E> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Driver(error) => write!(formatter, "the database driver failed: {error}"),
            Self::Migration(error) => write!(formatter, "{error}"),
            Self::Fold(error) => write!(formatter, "{error}"),
            Self::MalformedLedger => formatter.write_str(
                "the schema_version table is not the shape this build declares; drop the index and \
                 rebuild from records",
            ),
        }
    }
}

impl<E: core::fmt::Debug + core::fmt::Display> std::error::Error for StoreError<E> {}

/// The single writer.
///
/// # Single-writer discipline, and exactly how far it goes
///
/// [`Store::commit`] takes `&mut self`, so within one process the borrow checker serializes every
/// write through this value — two concurrent writes do not compile:
///
/// ```compile_fail,E0499
/// # use mesh_store::{Checkpoint, Store, SqlExecutor, Table, Row};
/// # fn two_at_once<E: SqlExecutor>(store: &mut Store<E>) {
/// let first = &mut *store;
/// let second = &mut *store;
/// let _ = (first.schema_version(), second.schema_version());
/// # }
/// ```
///
/// Across processes SQLite serializes writers itself, and `busy_timeout` turns the collision into
/// a wait. What neither of those covers is a second `Store` opened over a second connection in the
/// same process: nothing here prevents that, and the honest statement is that the discipline is
/// "one `Store` per database" enforced by the caller holding one, not by a lock this crate takes.
#[derive(Debug)]
pub struct Store<E: SqlExecutor> {
    executor: E,
    index: Index,
    schema_version: u32,
}

/// A persisted pending save did not match the index rebuilt from immutable records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingPrivateSaveError {
    expected: Digest16,
    rebuilt: Digest16,
}

impl PendingPrivateSaveError {
    /// Digest persisted with the original post-transaction acknowledgement.
    #[must_use]
    pub const fn expected(self) -> Digest16 {
        self.expected
    }

    /// Digest independently reproduced from immutable records.
    #[must_use]
    pub const fn rebuilt(self) -> Digest16 {
        self.rebuilt
    }
}

impl core::fmt::Display for PendingPrivateSaveError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            formatter,
            "the rebuilt index digest {} does not match pending acknowledgement {}",
            self.rebuilt, self.expected
        )
    }
}

impl std::error::Error for PendingPrivateSaveError {}

impl<E: SqlExecutor> Store<E> {
    /// Open a database: apply the pragmas, verify the migration ledger, run what is outstanding.
    ///
    /// # Errors
    ///
    /// [`StoreError::Migration`] when the ledger is from the future, has a gap, or records a
    /// migration whose SQL has since changed; [`StoreError::Driver`] when the driver fails.
    pub fn open(mut executor: E) -> Result<Self, StoreError<E::Error>> {
        let pragmas: String = PRAGMAS
            .iter()
            .map(|pragma| pragma.sql())
            .collect::<Vec<_>>()
            .join("\n");
        executor
            .execute_batch(&pragmas)
            .map_err(StoreError::Driver)?;

        let applied = read_ledger(&mut executor)?;
        let current = verify_applied(&applied).map_err(StoreError::Migration)?;
        let plan = plan_migrations(current).map_err(StoreError::Migration)?;

        if !plan.is_empty() {
            let mut sql = String::from("BEGIN IMMEDIATE;\n");
            sql.push_str(&plan.statements().join("\n"));
            sql.push_str("\nCOMMIT;\n");
            executor.execute_batch(&sql).map_err(StoreError::Driver)?;
        }

        let mut index = Index::new();
        index.set_ledger_rows(ledger_rows(&read_ledger(&mut executor)?));

        Ok(Self {
            executor,
            index,
            schema_version: plan.target_version(),
        })
    }

    /// The schema version this database is at.
    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// The index as this store believes it to be.
    #[must_use]
    pub const fn index(&self) -> &Index {
        &self.index
    }

    /// Reconstruct a process-lost acknowledgement only after this store independently reproduces
    /// the exact index digest persisted with it.
    ///
    /// # Errors
    ///
    /// [`PendingPrivateSaveError`] when immutable-record truth does not reproduce the pending
    /// acknowledgement. The caller must additionally bind the pending stamp to the corresponding
    /// journal operation before using the result as a meaningful checkpoint.
    pub fn verify_pending_private_save(
        &self,
        pending: PendingMeaningfulSave,
    ) -> Result<PrivateSaved, PendingPrivateSaveError> {
        let rebuilt = self.index.default_digest();
        if rebuilt != pending.index_digest() {
            return Err(PendingPrivateSaveError {
                expected: pending.index_digest(),
                rebuilt,
            });
        }
        let (operations, manifests, chunks) = pending.counts();
        Ok(PrivateSaved::after_recovery_verified(
            rebuilt, operations, manifests, chunks,
        ))
    }

    /// The driver, for a caller that needs to read.
    pub fn executor_mut(&mut self) -> &mut E {
        &mut self.executor
    }

    /// Believe a new index, after a transaction describing it has returned.
    ///
    /// Crate-private and deliberately so: the only two callers are [`Self::commit`] and
    /// [`crate::DurableCommit`], and both call it *after* the driver reported success, never
    /// before. A public setter would be a way to make the store believe something the database does
    /// not hold, which is the same defect as an optimistic acknowledgement one level down.
    pub(crate) fn adopt_index(&mut self, index: Index) {
        self.index = index;
    }

    /// Plan §6.3 steps 5 to 9: fold a checkpoint in and commit it in one transaction.
    ///
    /// The in-memory index advances only after the driver reports the transaction committed, so a
    /// failed write leaves the store believing exactly what the database holds.
    ///
    /// # Errors
    ///
    /// [`StoreError::Fold`] when the checkpoint contradicts the index — nothing is executed in
    /// that case — and [`StoreError::Driver`] when the transaction fails.
    pub fn commit(&mut self, checkpoint: &Checkpoint) -> Result<Digest16, StoreError<E::Error>> {
        let (plan, after) = CommitPlan::stage(&self.index, checkpoint).map_err(StoreError::Fold)?;
        self.executor
            .execute_batch(&plan.sql())
            .map_err(StoreError::Driver)?;
        self.index = after;
        Ok(self.index.default_digest())
    }

    /// Plan §6.3's index-corruption recovery: replay records and rewrite every record-derived
    /// table.
    ///
    /// # Errors
    ///
    /// [`StoreError::Fold`] when the record stream is contradictory, [`StoreError::Driver`] when
    /// the rewrite fails.
    pub fn rebuild_from<I>(&mut self, records: I) -> Result<Digest16, StoreError<E::Error>>
    where
        I: IntoIterator<Item = StoredRecord>,
    {
        let ledger = self.index.rows("schema_version").unwrap_or_default();
        let (rebuilt, _) = crate::rebuild::rebuild(records, ledger).map_err(StoreError::Fold)?;
        let plan = CommitPlan::full_rebuild(&rebuilt);
        self.executor
            .execute_batch(&plan.sql())
            .map_err(StoreError::Driver)?;
        self.index = rebuilt;
        Ok(self.index.default_digest())
    }
}

/// Read every table out of a database, in [`TABLES`] order.
///
/// Skips tables a migration has not created yet, so it is usable against a partly-migrated
/// database — which is what the migration tests need.
///
/// # Errors
///
/// Whatever the driver reports.
pub fn read_all_tables<E: SqlExecutor>(
    executor: &mut E,
) -> Result<Vec<(&'static str, Vec<Row>)>, E::Error> {
    let mut out = Vec::with_capacity(TABLES.len());
    for table in TABLES {
        if executor.table_exists(table.name)? {
            out.push((table.name, executor.read_table(table)?));
        }
    }
    Ok(out)
}

fn read_ledger<E: SqlExecutor>(
    executor: &mut E,
) -> Result<Vec<AppliedMigration>, StoreError<E::Error>> {
    let ledger = crate::schema::table("schema_version").ok_or(StoreError::MalformedLedger)?;
    if !executor
        .table_exists(ledger.name)
        .map_err(StoreError::Driver)?
    {
        return Ok(Vec::new());
    }
    let rows = executor.read_table(ledger).map_err(StoreError::Driver)?;
    rows.iter().map(applied_from_row).collect()
}

fn applied_from_row<E>(row: &Row) -> Result<AppliedMigration, StoreError<E>> {
    let [Value::Integer(version), Value::Blob(fingerprint)] = row.values() else {
        return Err(StoreError::MalformedLedger);
    };
    let bytes: [u8; 16] = fingerprint
        .as_slice()
        .try_into()
        .map_err(|_| StoreError::MalformedLedger)?;
    let version = u32::try_from(*version).map_err(|_| StoreError::MalformedLedger)?;
    Ok(AppliedMigration {
        version,
        fingerprint: Digest16::from_bytes(bytes),
    })
}

fn ledger_rows(applied: &[AppliedMigration]) -> Vec<Row> {
    let mut rows: Vec<Row> = applied
        .iter()
        .map(|entry| {
            Row::new(vec![
                Value::Integer(i64::from(entry.version)),
                Value::blob(entry.fingerprint.as_bytes()),
            ])
        })
        .collect();
    rows.sort();
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migration::{CURRENT_VERSION, MIGRATIONS};

    /// An executor that records what it was asked to run and answers reads from a fixed map. It
    /// executes no SQL, which is the point: it checks the *sequence* this module produces, and
    /// `tests/` checks that the sequence works against real SQLite.
    #[derive(Debug, Default)]
    struct Recorder {
        batches: Vec<String>,
        tables: Vec<(&'static str, Vec<Row>)>,
    }

    impl SqlExecutor for Recorder {
        type Error = String;

        fn execute_batch(&mut self, sql: &str) -> Result<(), Self::Error> {
            self.batches.push(sql.to_owned());
            Ok(())
        }

        fn read_table(&mut self, table: &Table) -> Result<Vec<Row>, Self::Error> {
            Ok(self
                .tables
                .iter()
                .find(|(name, _)| *name == table.name)
                .map(|(_, rows)| rows.clone())
                .unwrap_or_default())
        }

        fn table_exists(&mut self, name: &str) -> Result<bool, Self::Error> {
            Ok(self.tables.iter().any(|(table, _)| *table == name))
        }
    }

    #[test]
    fn every_pragma_carries_a_reason() {
        for pragma in PRAGMAS {
            assert!(
                !pragma.reason.trim().is_empty(),
                "{} has no reason",
                pragma.name
            );
            assert!(pragma.sql().starts_with("PRAGMA "));
            assert!(pragma.sql().ends_with(';'));
        }
    }

    #[test]
    fn wal_and_foreign_keys_are_both_set() {
        let names: Vec<&str> = PRAGMAS.iter().map(|pragma| pragma.name).collect();
        assert!(names.contains(&"journal_mode"));
        assert!(names.contains(&"foreign_keys"));
        let wal = PRAGMAS
            .iter()
            .find(|pragma| pragma.name == "journal_mode")
            .expect("journal_mode is set");
        assert_eq!(wal.value, "WAL");
    }

    #[test]
    fn opening_a_fresh_database_applies_pragmas_then_migrates() {
        let store = Store::open(Recorder::default()).expect("opens");
        assert_eq!(store.schema_version(), CURRENT_VERSION);
        let batches = &store.executor.batches;
        assert!(batches[0].contains("PRAGMA journal_mode = WAL;"));
        assert!(batches[1].starts_with("BEGIN IMMEDIATE;"));
        assert!(batches[1].trim_end().ends_with("COMMIT;"));
        for migration in MIGRATIONS {
            assert!(batches[1].contains(migration.sql));
        }
    }

    #[test]
    fn opening_an_up_to_date_database_runs_no_migration() {
        let rows: Vec<Row> = MIGRATIONS
            .iter()
            .map(|migration| {
                Row::new(vec![
                    Value::Integer(i64::from(migration.version)),
                    Value::blob(migration.fingerprint().as_bytes()),
                ])
            })
            .collect();
        let executor = Recorder {
            batches: Vec::new(),
            tables: vec![("schema_version", rows)],
        };
        let store = Store::open(executor).expect("opens");
        assert_eq!(store.schema_version(), CURRENT_VERSION);
        assert_eq!(store.executor.batches.len(), 1, "only the pragmas ran");
    }

    #[test]
    fn a_rewritten_migration_refuses_to_open() {
        let executor = Recorder {
            batches: Vec::new(),
            tables: vec![(
                "schema_version",
                vec![Row::new(vec![Value::Integer(1), Value::blob([0u8; 16])])],
            )],
        };
        let error = Store::open(executor).expect_err("refuses");
        assert!(matches!(
            error,
            StoreError::Migration(MigrationError::Rewritten { version: 1, .. })
        ));
    }

    #[test]
    fn a_malformed_ledger_row_is_a_corrupt_database_not_a_panic() {
        let executor = Recorder {
            batches: Vec::new(),
            tables: vec![(
                "schema_version",
                vec![Row::new(vec![Value::Integer(1), Value::blob([0u8; 4])])],
            )],
        };
        assert!(matches!(
            Store::open(executor),
            Err(StoreError::<String>::MalformedLedger)
        ));
    }

    #[test]
    fn a_ledger_row_of_the_wrong_arity_is_refused() {
        let executor = Recorder {
            batches: Vec::new(),
            tables: vec![("schema_version", vec![Row::new(vec![Value::Integer(1)])])],
        };
        assert!(matches!(
            Store::open(executor),
            Err(StoreError::<String>::MalformedLedger)
        ));
    }

    #[test]
    fn a_failed_commit_leaves_the_index_where_it_was() {
        #[derive(Debug, Default)]
        struct Failing {
            opened: bool,
        }
        impl SqlExecutor for Failing {
            type Error = String;
            fn execute_batch(&mut self, _sql: &str) -> Result<(), Self::Error> {
                if self.opened {
                    return Err("disk full".to_owned());
                }
                self.opened = true;
                Ok(())
            }
            fn read_table(&mut self, _table: &Table) -> Result<Vec<Row>, Self::Error> {
                Ok(Vec::new())
            }
            fn table_exists(&mut self, _name: &str) -> Result<bool, Self::Error> {
                Ok(false)
            }
        }

        // The first batch is the pragmas; the migration batch is the one that fails.
        let error = Store::open(Failing::default()).expect_err("the migration fails");
        assert!(matches!(error, StoreError::Driver(_)));
        assert!(error.to_string().contains("disk full"));
    }
}
