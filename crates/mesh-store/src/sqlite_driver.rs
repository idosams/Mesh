//! The production [`crate::SqlExecutor`] backed by bundled SQLite.
//!
//! One connection lives for the executor's lifetime. [`crate::Store::open`] applies every
//! persistent and per-connection pragma before migrations or reads, so later calls cannot
//! accidentally lose `foreign_keys`, `synchronous` or the writer timeout by hopping between
//! connections.

use core::fmt;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension as _, TransactionBehavior};

use crate::{
    ColumnType, RecoverySnapshot, RecoveryStatePersistence, Row, SqlExecutor, Table, Value,
};

const RECOVERY_STATE_TABLE: &str = "mesh_recovery_state";
const RECOVERY_OBSERVATION_TABLE: &str = "mesh_recovery_observation";
/// The namespaced database whose contents are not disposable with the record-derived index.
pub const RECOVERY_DATABASE_FILE_NAME: &str = ".mesh-recovery.sqlite";
const RECOVERY_STATE_SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS mesh_recovery_state (
  view_key BLOB PRIMARY KEY NOT NULL CHECK(length(view_key) > 0),
  snapshot BLOB NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS mesh_recovery_observation (
  view_key BLOB PRIMARY KEY NOT NULL CHECK(length(view_key) > 0),
  snapshot BLOB NOT NULL
) STRICT;
";

/// A live connection to one workspace's `metadata.sqlite`.
#[derive(Debug)]
pub struct Sqlite {
    path: PathBuf,
    connection: Connection,
}

impl Sqlite {
    /// Open or create the database at `path`.
    ///
    /// Pragmas and migrations are deliberately not applied here; [`crate::Store::open`] owns that
    /// ordering for every executor implementation.
    ///
    /// # Errors
    ///
    /// [`SqliteError`] when SQLite cannot open the supplied path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, SqliteError> {
        let path = path.as_ref().to_path_buf();
        let connection = Connection::open(&path).map_err(SqliteError::driver)?;
        Ok(Self { path, connection })
    }

    /// Open a process-local disposable database.
    ///
    /// This is used while a newly copied workspace is still awaiting confirmation. Durable truth
    /// is written only to the pinned CAS and journal in that interval; the derived index is
    /// rebuilt at the admitted pathname after confirmation, so SQLite never needs path authority
    /// during the rename-sensitive transaction.
    pub fn open_in_memory() -> Result<Self, SqliteError> {
        let connection = Connection::open_in_memory().map_err(SqliteError::driver)?;
        Ok(Self {
            path: PathBuf::from(":memory:"),
            connection,
        })
    }

    /// The database file this connection owns.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The journal mode SQLite reports for this connection.
    ///
    /// # Errors
    ///
    /// [`SqliteError`] when the pragma cannot be read.
    pub fn journal_mode(&self) -> Result<String, SqliteError> {
        self.connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .map_err(SqliteError::driver)
    }
}

/// A production SQLite operation failed.
#[derive(Debug)]
pub struct SqliteError {
    detail: String,
}

impl SqliteError {
    fn driver(error: rusqlite::Error) -> Self {
        Self {
            detail: error.to_string(),
        }
    }
}

impl fmt::Display for SqliteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for SqliteError {}

/// Durable recovery-state ownership inside a workspace's isolated recovery database.
///
/// The file is deliberately outside the record-derived `metadata.sqlite`: index replacement must
/// not delete atomically admitted runtime state. Unlike the rebuildable index, this connection
/// must use WAL with `synchronous=FULL`; losing its WAL tail after an acknowledged recovery
/// transition would lose the only durable copy of that runtime truth.
#[derive(Debug)]
pub struct SqliteRecoveryState {
    connection: Connection,
    view_key: Vec<u8>,
    // Outer `None` means this handle has not loaded yet; inner `None` means it observed no row.
    // Runtime writes compare against these exact bytes so a second process cannot regress truth.
    observed_snapshot: Option<Option<Vec<u8>>>,
    observed_recovery: Option<Option<Vec<u8>>>,
}

impl SqliteRecoveryState {
    /// Inspect one logical view without creating, migrating, repairing, chmodding, or otherwise
    /// mutating the recovery database family.
    ///
    /// An absent database, an empty database that the writable opener can initialize, or a valid
    /// database with no row for `view_key` returns `Ok(None)`. Existing snapshot bytes are decoded
    /// through [`RecoverySnapshot`]'s canonical validator before they are returned. Inspection is
    /// immutable and accepts only a quiescent, fully checkpointed database: a WAL family is
    /// refused for retry instead of letting SQLite create or update sidecars in the workspace.
    /// Linked, special, corrupt, changing, or undecodable inputs fail closed.
    ///
    /// # Errors
    ///
    /// [`SqliteRecoveryStateError`] when the key is empty or an existing database family cannot
    /// be safely opened and decoded read-only.
    pub fn inspect_read_only(
        path: impl AsRef<Path>,
        view_key: impl AsRef<[u8]>,
    ) -> Result<Option<RecoverySnapshot>, SqliteRecoveryStateError> {
        let view_key = view_key.as_ref();
        if view_key.is_empty() {
            return Err(SqliteRecoveryStateError::new(
                "a recovery-state view key cannot be empty",
            ));
        }
        load_immutable_snapshot(path.as_ref(), view_key)
    }

    /// Inspect the isolated owner and its one-time legacy migration source without changing
    /// either database.
    ///
    /// Once the isolated row exists it is authoritative and the legacy database is deliberately
    /// ignored, matching [`Self::open_isolated`]. Otherwise the legacy row must also decode before
    /// a caller may report that the writable runtime can open and migrate this view.
    ///
    /// # Errors
    ///
    /// [`SqliteRecoveryStateError`] under the same alias, corruption, and snapshot-validation
    /// conditions as [`Self::inspect_read_only`], or when the two database paths alias.
    pub fn inspect_isolated_read_only(
        recovery_path: impl AsRef<Path>,
        legacy_index_path: impl AsRef<Path>,
        view_key: impl AsRef<[u8]>,
    ) -> Result<Option<RecoverySnapshot>, SqliteRecoveryStateError> {
        let recovery_path = recovery_path.as_ref();
        let legacy_index_path = legacy_index_path.as_ref();
        refuse_database_path_alias(recovery_path, legacy_index_path)?;
        let view_key = view_key.as_ref();
        if let Some(snapshot) = Self::inspect_read_only(recovery_path, view_key)? {
            return Ok(Some(snapshot));
        }
        Self::inspect_read_only(legacy_index_path, view_key)
    }

    /// Open one logical view in an existing workspace database.
    ///
    /// # Errors
    ///
    /// [`SqliteRecoveryStateError`] when the key is empty or SQLite cannot open and prepare the
    /// durable table.
    pub fn open(
        path: impl AsRef<Path>,
        view_key: impl AsRef<[u8]>,
    ) -> Result<Self, SqliteRecoveryStateError> {
        let view_key = view_key.as_ref().to_vec();
        if view_key.is_empty() {
            return Err(SqliteRecoveryStateError::new(
                "a recovery-state view key cannot be empty",
            ));
        }
        let path = path.as_ref();
        let opened_path = canonical_database_path(path)?;
        let path = opened_path.as_path();
        refuse_database_family_aliases(path)?;
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::default() | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(SqliteRecoveryStateError::driver)?;
        restrict_database_family_to_owner(path)?;
        connection
            .execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA foreign_keys = ON;
                 PRAGMA synchronous = FULL;
                 PRAGMA fullfsync = ON;
                 PRAGMA checkpoint_fullfsync = ON;
                 PRAGMA busy_timeout = 5000;
                 PRAGMA wal_autocheckpoint = 1000;",
            )
            .map_err(SqliteRecoveryStateError::driver)?;
        connection
            .execute_batch(RECOVERY_STATE_SCHEMA)
            .map_err(SqliteRecoveryStateError::driver)?;
        restrict_database_family_to_owner(path)?;
        verify_recovery_pragmas(&connection)?;
        Ok(Self {
            connection,
            view_key,
            observed_snapshot: None,
            observed_recovery: None,
        })
    }

    /// Open the isolated owner, copying an older row from the disposable index exactly once.
    ///
    /// The destination row is the migration marker. If a process stops after creating the new
    /// database but before its transaction commits, the absent row makes the next open retry the
    /// read-only copy. Once present, the isolated row always wins and the legacy database is not
    /// consulted. The legacy database is never modified, so every migration failure leaves its
    /// only pre-existing copy intact.
    ///
    /// # Errors
    ///
    /// [`SqliteRecoveryStateError`] when the paths alias, either database cannot be read, a legacy
    /// snapshot is invalid, or the isolated FULL transaction cannot commit.
    pub fn open_isolated(
        recovery_path: impl AsRef<Path>,
        legacy_index_path: impl AsRef<Path>,
        view_key: impl AsRef<[u8]>,
    ) -> Result<Self, SqliteRecoveryStateError> {
        let recovery_path = recovery_path.as_ref();
        let legacy_index_path = legacy_index_path.as_ref();
        refuse_database_path_alias(recovery_path, legacy_index_path)?;
        let view_key = view_key.as_ref();
        let mut isolated = Self::open(recovery_path, view_key)?;
        if isolated.load()?.is_some() {
            return Ok(isolated);
        }

        let snapshot = load_legacy_snapshot(legacy_index_path, view_key)?.unwrap_or_default();
        if !isolated.persist_migration_if_absent(&snapshot)? && isolated.load()?.is_none() {
            return Err(SqliteRecoveryStateError::new(
                "the recovery migration lost its destination row",
            ));
        }
        Ok(isolated)
    }

    /// Create or migrate an exact default isolated recovery owner, then leave its SQLite family
    /// quiescent for immutable validation.
    ///
    /// The caller must serialize the workspace directory against every other Mesh opener for the
    /// duration of this call. A non-default recovery snapshot is never rewritten or normalized:
    /// it returns `Ok(false)`. SQLite must successfully checkpoint the family and leave DELETE
    /// journal mode before this returns `Ok(true)`, so a later immutable inspector cannot mistake
    /// uncheckpointed WAL state for a clean candidate.
    ///
    /// # Errors
    ///
    /// [`SqliteRecoveryStateError`] when either database family is unsafe, malformed, aliased, or
    /// cannot be made quiescent while the caller holds its external workspace serial.
    pub fn quiesce_default_isolated(
        recovery_path: impl AsRef<Path>,
        legacy_index_path: impl AsRef<Path>,
        view_key: impl AsRef<[u8]>,
    ) -> Result<bool, SqliteRecoveryStateError> {
        let recovery_path = recovery_path.as_ref();
        let legacy_index_path = legacy_index_path.as_ref();
        let view_key = view_key.as_ref();
        {
            let mut isolated = Self::open_isolated(recovery_path, legacy_index_path, view_key)?;
            if isolated.load()?.as_ref() != Some(&RecoverySnapshot::default()) {
                return Ok(false);
            }
            let mode = isolated
                .connection
                .query_row("PRAGMA journal_mode = DELETE", [], |row| {
                    row.get::<_, String>(0)
                })
                .map_err(SqliteRecoveryStateError::driver)?;
            if mode != "delete" {
                return Err(SqliteRecoveryStateError::new(
                    "the default recovery database did not become quiescent",
                ));
            }
        }
        for suffix in ["-wal", "-shm"] {
            let sidecar = database_sidecar(recovery_path, suffix);
            match std::fs::symlink_metadata(&sidecar) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Ok(_) => {
                    return Err(SqliteRecoveryStateError::new(
                        "the default recovery database retained a WAL sidecar",
                    ));
                }
                Err(error) => return Err(SqliteRecoveryStateError::new(error.to_string())),
            }
        }
        Ok(true)
    }

    /// Preserve an existing isolated owner or copy a legacy row without creating state for a
    /// workspace that has never used recovery persistence.
    ///
    /// Runtime checkpoint configuration is intentionally not an input. A process that omits
    /// thresholds still lacks authority to delete recovery truth left by an earlier configured
    /// process when it repairs the record-derived index. An existing isolated path is validated
    /// through [`Self::open_isolated`]; when that path is absent, the destination is created only
    /// if the legacy index actually contains this view.
    ///
    /// # Errors
    ///
    /// [`SqliteRecoveryStateError`] when an existing owner or legacy source cannot be validated
    /// and preserved exactly.
    pub fn preserve_legacy_if_present(
        recovery_path: impl AsRef<Path>,
        legacy_index_path: impl AsRef<Path>,
        view_key: impl AsRef<[u8]>,
    ) -> Result<bool, SqliteRecoveryStateError> {
        let recovery_path = recovery_path.as_ref();
        let legacy_index_path = legacy_index_path.as_ref();
        let view_key = view_key.as_ref();
        if view_key.is_empty() {
            return Err(SqliteRecoveryStateError::new(
                "a recovery-state view key cannot be empty",
            ));
        }
        refuse_database_path_alias(recovery_path, legacy_index_path)?;

        match std::fs::symlink_metadata(recovery_path) {
            Ok(_) => {
                drop(Self::open_isolated(
                    recovery_path,
                    legacy_index_path,
                    view_key,
                )?);
                Ok(true)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let Some(snapshot) = load_legacy_snapshot(legacy_index_path, view_key)? else {
                    return Ok(false);
                };
                // Carry the exact bytes this probe admitted into the destination transaction.
                // Re-reading the disposable source after this point would let a concurrent index
                // replacement turn an observed non-default snapshot into a default migration.
                let mut isolated = Self::open(recovery_path, view_key)?;
                if isolated.load()?.is_none()
                    && !isolated.persist_migration_if_absent(&snapshot)?
                    && isolated.load()?.is_none()
                {
                    return Err(SqliteRecoveryStateError::new(
                        "the recovery migration lost its destination row",
                    ));
                }
                Ok(true)
            }
            Err(error) => Err(SqliteRecoveryStateError::new(error.to_string())),
        }
    }

    /// Admit a migration snapshot only while this view still has no isolated authority.
    ///
    /// Unlike ordinary runtime persistence, this must never update a conflicting row: another
    /// opener can win the migration and advance the runtime while this opener is reading legacy
    /// state. Returning `false` tells the caller to retain and validate that winning row.
    fn persist_migration_if_absent(
        &mut self,
        snapshot: &RecoverySnapshot,
    ) -> Result<bool, SqliteRecoveryStateError> {
        let encoded = snapshot.encode();
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(SqliteRecoveryStateError::driver)?;
        let inserted = transaction
            .execute(
                "INSERT INTO mesh_recovery_state(view_key, snapshot) VALUES (?1, ?2)
                 ON CONFLICT(view_key) DO NOTHING",
                rusqlite::params![&self.view_key, encoded],
            )
            .map_err(SqliteRecoveryStateError::driver)?;
        transaction
            .commit()
            .map_err(SqliteRecoveryStateError::driver)?;
        if inserted == 1 {
            self.observed_snapshot = Some(Some(encoded));
            self.observed_recovery.get_or_insert(None);
        }
        Ok(inserted == 1)
    }

    /// The private table name, exposed only so reconstruction tests can prove it is not folded.
    #[must_use]
    pub const fn table_name() -> &'static str {
        RECOVERY_STATE_TABLE
    }

    /// The post-journal recovery table name, exposed for failure-injection and reconstruction
    /// tests. It is private runtime state and must never be folded into the record-derived index.
    #[must_use]
    pub const fn observation_table_name() -> &'static str {
        RECOVERY_OBSERVATION_TABLE
    }
}

/// A workspace SQLite recovery-state operation failed.
#[derive(Debug)]
pub struct SqliteRecoveryStateError {
    detail: String,
}

impl SqliteRecoveryStateError {
    fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }

    fn driver(error: rusqlite::Error) -> Self {
        Self::new(error.to_string())
    }
}

impl fmt::Display for SqliteRecoveryStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for SqliteRecoveryStateError {}

impl RecoveryStatePersistence for SqliteRecoveryState {
    type Error = SqliteRecoveryStateError;

    fn load(&mut self) -> Result<Option<RecoverySnapshot>, Self::Error> {
        let primary: Option<Vec<u8>> = self
            .connection
            .query_row(
                "SELECT snapshot FROM mesh_recovery_state WHERE view_key = ?1",
                [&self.view_key],
                |row| row.get(0),
            )
            .optional()
            .map_err(SqliteRecoveryStateError::driver)?;
        let recovery: Option<Vec<u8>> = self
            .connection
            .query_row(
                "SELECT snapshot FROM mesh_recovery_observation WHERE view_key = ?1",
                [&self.view_key],
                |row| row.get(0),
            )
            .optional()
            .map_err(SqliteRecoveryStateError::driver)?;
        let authoritative = recovery.as_ref().or(primary.as_ref());
        let snapshot = authoritative
            .map(|bytes| {
                RecoverySnapshot::decode(bytes.as_slice())
                    .map_err(|error| SqliteRecoveryStateError::new(error.to_string()))
            })
            .transpose()?;
        self.observed_snapshot = Some(primary);
        self.observed_recovery = Some(recovery);
        Ok(snapshot)
    }

    fn persist(&mut self, snapshot: &RecoverySnapshot) -> Result<(), Self::Error> {
        let encoded = snapshot.encode();
        let observed_snapshot = self.observed_snapshot.clone();
        let observed_recovery = self.observed_recovery.clone();
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(SqliteRecoveryStateError::driver)?;
        let expected = match observed_snapshot {
            Some(expected) => expected,
            None => transaction
                .query_row(
                    "SELECT snapshot FROM mesh_recovery_state WHERE view_key = ?1",
                    [&self.view_key],
                    |row| row.get(0),
                )
                .optional()
                .map_err(SqliteRecoveryStateError::driver)?,
        };
        let expected_recovery = match observed_recovery {
            Some(expected) => expected,
            None => transaction
                .query_row(
                    "SELECT snapshot FROM mesh_recovery_observation WHERE view_key = ?1",
                    [&self.view_key],
                    |row| row.get(0),
                )
                .optional()
                .map_err(SqliteRecoveryStateError::driver)?,
        };
        let current_recovery: Option<Vec<u8>> = transaction
            .query_row(
                "SELECT snapshot FROM mesh_recovery_observation WHERE view_key = ?1",
                [&self.view_key],
                |row| row.get(0),
            )
            .optional()
            .map_err(SqliteRecoveryStateError::driver)?;
        if current_recovery != expected_recovery {
            return Err(SqliteRecoveryStateError::new(
                "the journal-backed recovery snapshot changed by another writer",
            ));
        }
        let admitted = match expected.as_deref() {
            Some(expected) => transaction
                .execute(
                    "INSERT INTO mesh_recovery_state(view_key, snapshot) VALUES (?1, ?2)
                     ON CONFLICT(view_key) DO UPDATE SET snapshot = excluded.snapshot
                     WHERE mesh_recovery_state.snapshot = ?3",
                    rusqlite::params![&self.view_key, &encoded, expected],
                )
                .map_err(SqliteRecoveryStateError::driver)?,
            None => transaction
                .execute(
                    "INSERT INTO mesh_recovery_state(view_key, snapshot) VALUES (?1, ?2)
                     ON CONFLICT(view_key) DO NOTHING",
                    rusqlite::params![&self.view_key, &encoded],
                )
                .map_err(SqliteRecoveryStateError::driver)?,
        };
        if admitted != 1 {
            let winner: Option<Vec<u8>> = transaction
                .query_row(
                    "SELECT snapshot FROM mesh_recovery_state WHERE view_key = ?1",
                    [&self.view_key],
                    |row| row.get(0),
                )
                .optional()
                .map_err(SqliteRecoveryStateError::driver)?;
            if winner.as_deref() != Some(encoded.as_slice()) {
                return Err(SqliteRecoveryStateError::new(
                    "the recovery snapshot changed by another writer",
                ));
            }
        }
        transaction
            .execute(
                "DELETE FROM mesh_recovery_observation WHERE view_key = ?1",
                [&self.view_key],
            )
            .map_err(SqliteRecoveryStateError::driver)?;
        transaction
            .commit()
            .map_err(SqliteRecoveryStateError::driver)?;
        self.observed_snapshot = Some(Some(encoded));
        self.observed_recovery = Some(None);
        Ok(())
    }

    fn persist_observation_recovery(
        &mut self,
        snapshot: &RecoverySnapshot,
    ) -> Result<(), Self::Error> {
        let encoded = snapshot.encode();
        let observed_snapshot = self.observed_snapshot.clone();
        let observed_recovery = self.observed_recovery.clone();
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(SqliteRecoveryStateError::driver)?;
        let expected_primary = match observed_snapshot {
            Some(expected) => expected,
            None => transaction
                .query_row(
                    "SELECT snapshot FROM mesh_recovery_state WHERE view_key = ?1",
                    [&self.view_key],
                    |row| row.get(0),
                )
                .optional()
                .map_err(SqliteRecoveryStateError::driver)?,
        };
        let current_primary: Option<Vec<u8>> = transaction
            .query_row(
                "SELECT snapshot FROM mesh_recovery_state WHERE view_key = ?1",
                [&self.view_key],
                |row| row.get(0),
            )
            .optional()
            .map_err(SqliteRecoveryStateError::driver)?;
        if current_primary != expected_primary {
            return Err(SqliteRecoveryStateError::new(
                "the recovery snapshot changed by another writer",
            ));
        }
        let expected_recovery = match observed_recovery {
            Some(expected) => expected,
            None => transaction
                .query_row(
                    "SELECT snapshot FROM mesh_recovery_observation WHERE view_key = ?1",
                    [&self.view_key],
                    |row| row.get(0),
                )
                .optional()
                .map_err(SqliteRecoveryStateError::driver)?,
        };
        let existing: Option<Vec<u8>> = transaction
            .query_row(
                "SELECT snapshot FROM mesh_recovery_observation WHERE view_key = ?1",
                [&self.view_key],
                |row| row.get(0),
            )
            .optional()
            .map_err(SqliteRecoveryStateError::driver)?;
        if existing != expected_recovery {
            return Err(SqliteRecoveryStateError::new(
                "the journal-backed recovery snapshot changed by another writer",
            ));
        }
        match existing {
            Some(existing) if existing != encoded => {
                let prior = RecoverySnapshot::decode(&existing)
                    .map_err(|error| SqliteRecoveryStateError::new(error.to_string()))?;
                if !snapshot.continues_observation_recovery_from(&prior) {
                    return Err(SqliteRecoveryStateError::new(
                        "the journal-backed recovery snapshot conflicts with an earlier pending observation",
                    ));
                }
                let advanced = transaction
                    .execute(
                        "UPDATE mesh_recovery_observation SET snapshot = ?2
                         WHERE view_key = ?1 AND snapshot = ?3",
                        rusqlite::params![&self.view_key, &encoded, &existing],
                    )
                    .map_err(SqliteRecoveryStateError::driver)?;
                if advanced != 1 {
                    return Err(SqliteRecoveryStateError::new(
                        "the journal-backed recovery snapshot changed by another writer",
                    ));
                }
            }
            Some(_) => {}
            None => {
                transaction
                    .execute(
                        "INSERT INTO mesh_recovery_observation(view_key, snapshot) VALUES (?1, ?2)",
                        rusqlite::params![&self.view_key, &encoded],
                    )
                    .map_err(SqliteRecoveryStateError::driver)?;
            }
        }
        transaction
            .commit()
            .map_err(SqliteRecoveryStateError::driver)?;
        self.observed_recovery = Some(Some(encoded));
        Ok(())
    }
}

fn verify_recovery_pragmas(connection: &Connection) -> Result<(), SqliteRecoveryStateError> {
    let journal_mode: String = connection
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .map_err(SqliteRecoveryStateError::driver)?;
    if !journal_mode.eq_ignore_ascii_case("wal") {
        return Err(SqliteRecoveryStateError::new(format!(
            "the recovery database requires WAL mode, but SQLite selected {journal_mode}"
        )));
    }
    let synchronous: i64 = connection
        .pragma_query_value(None, "synchronous", |row| row.get(0))
        .map_err(SqliteRecoveryStateError::driver)?;
    if synchronous != 2 {
        return Err(SqliteRecoveryStateError::new(format!(
            "the recovery database requires synchronous=FULL (2), but SQLite selected {synchronous}"
        )));
    }
    for pragma in ["fullfsync", "checkpoint_fullfsync"] {
        let enabled: i64 = connection
            .pragma_query_value(None, pragma, |row| row.get(0))
            .map_err(SqliteRecoveryStateError::driver)?;
        if enabled != 1 {
            return Err(SqliteRecoveryStateError::new(format!(
                "the recovery database requires {pragma}=ON, but SQLite selected {enabled}"
            )));
        }
    }
    Ok(())
}

fn canonical_database_path(path: &Path) -> Result<PathBuf, SqliteRecoveryStateError> {
    if path == Path::new(":memory:") {
        return Ok(path.to_path_buf());
    }
    let file_name = path.file_name().ok_or_else(|| {
        SqliteRecoveryStateError::new("a recovery database path must name a file")
    })?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = std::fs::canonicalize(parent)
        .map_err(|error| SqliteRecoveryStateError::new(error.to_string()))?;
    Ok(parent.join(file_name))
}

fn refuse_database_path_alias(
    recovery_path: &Path,
    legacy_index_path: &Path,
) -> Result<(), SqliteRecoveryStateError> {
    let recovery_path = canonical_database_path(recovery_path)?;
    let legacy_index_path = canonical_database_path(legacy_index_path)?;
    if recovery_path == legacy_index_path {
        return Err(SqliteRecoveryStateError::new(
            "the recovery database must be separate from the disposable index",
        ));
    }
    Ok(())
}

fn refuse_database_family_aliases(path: &Path) -> Result<(), SqliteRecoveryStateError> {
    refuse_database_alias(path)?;
    for suffix in ["-wal", "-shm"] {
        let mut sidecar = path.as_os_str().to_os_string();
        sidecar.push(suffix);
        refuse_database_alias(Path::new(&sidecar))?;
    }
    Ok(())
}

#[cfg(unix)]
fn restrict_database_family_to_owner(path: &Path) -> Result<(), SqliteRecoveryStateError> {
    if path == Path::new(":memory:") {
        return Ok(());
    }
    restrict_database_file_to_owner(path)?;
    for suffix in ["-wal", "-shm"] {
        let mut sidecar = path.as_os_str().to_os_string();
        sidecar.push(suffix);
        let sidecar = Path::new(&sidecar);
        match std::fs::symlink_metadata(sidecar) {
            Ok(_) => restrict_database_file_to_owner(sidecar)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(SqliteRecoveryStateError::new(error.to_string())),
        }
    }
    Ok(())
}

#[cfg(unix)]
fn restrict_database_file_to_owner(path: &Path) -> Result<(), SqliteRecoveryStateError> {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

    let before = std::fs::symlink_metadata(path)
        .map_err(|error| SqliteRecoveryStateError::new(error.to_string()))?;
    if !before.is_file() || before.file_type().is_symlink() || before.nlink() != 1 {
        return Err(SqliteRecoveryStateError::new(
            "a recovery database or sidecar must be a physically separate single-link regular file",
        ));
    }
    let opened = std::fs::File::open(path)
        .map_err(|error| SqliteRecoveryStateError::new(error.to_string()))?;
    let opened_metadata = opened
        .metadata()
        .map_err(|error| SqliteRecoveryStateError::new(error.to_string()))?;
    if !opened_metadata.is_file()
        || opened_metadata.dev() != before.dev()
        || opened_metadata.ino() != before.ino()
        || opened_metadata.nlink() != 1
    {
        return Err(SqliteRecoveryStateError::new(
            "a recovery database or sidecar changed while it was secured",
        ));
    }
    let mut permissions = opened_metadata.permissions();
    permissions.set_mode(0o600);
    opened
        .set_permissions(permissions)
        .map_err(|error| SqliteRecoveryStateError::new(error.to_string()))?;

    let after = std::fs::symlink_metadata(path)
        .map_err(|error| SqliteRecoveryStateError::new(error.to_string()))?;
    if !after.is_file()
        || after.file_type().is_symlink()
        || after.dev() != before.dev()
        || after.ino() != before.ino()
        || after.nlink() != 1
        || after.permissions().mode() & 0o077 != 0
    {
        return Err(SqliteRecoveryStateError::new(
            "a recovery database or sidecar changed while it was secured",
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn restrict_database_family_to_owner(path: &Path) -> Result<(), SqliteRecoveryStateError> {
    if path == Path::new(":memory:") {
        return Ok(());
    }
    refuse_database_family_aliases(path)
}

fn refuse_database_alias(path: &Path) -> Result<(), SqliteRecoveryStateError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(SqliteRecoveryStateError::new(
            "a recovery database or sidecar must be a physically separate regular file, not a symbolic link",
        )),
        Ok(metadata) if !metadata.is_file() => Err(SqliteRecoveryStateError::new(
            "a recovery database or sidecar must be a physically separate regular file",
        )),
        Ok(metadata) => refuse_multiple_links(&metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(SqliteRecoveryStateError::new(error.to_string())),
    }
}

#[cfg(unix)]
fn refuse_multiple_links(metadata: &std::fs::Metadata) -> Result<(), SqliteRecoveryStateError> {
    use std::os::unix::fs::MetadataExt as _;

    if metadata.nlink() != 1 {
        return Err(SqliteRecoveryStateError::new(
            "a recovery database or sidecar must be a physically separate single-link file",
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn refuse_multiple_links(metadata: &std::fs::Metadata) -> Result<(), SqliteRecoveryStateError> {
    use std::os::windows::fs::MetadataExt as _;

    if metadata.number_of_links().is_some_and(|links| links != 1) {
        return Err(SqliteRecoveryStateError::new(
            "a recovery database or sidecar must be a physically separate single-link file",
        ));
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn refuse_multiple_links(_: &std::fs::Metadata) -> Result<(), SqliteRecoveryStateError> {
    Ok(())
}

fn load_legacy_snapshot(
    path: &Path,
    view_key: &[u8],
) -> Result<Option<RecoverySnapshot>, SqliteRecoveryStateError> {
    let opened_path = canonical_database_path(path)?;
    let path = opened_path.as_path();
    refuse_database_family_aliases(path)?;
    if !path.exists() {
        return Ok(None);
    }
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(SqliteRecoveryStateError::driver)?;
    let table_exists = connection
        .query_row(
            "SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1",
            [RECOVERY_STATE_TABLE],
            |_| Ok(()),
        )
        .optional()
        .map_err(SqliteRecoveryStateError::driver)?
        .is_some();
    if !table_exists {
        return Ok(None);
    }
    let bytes: Option<Vec<u8>> = connection
        .query_row(
            "SELECT snapshot FROM mesh_recovery_state WHERE view_key = ?1",
            [view_key],
            |row| row.get(0),
        )
        .optional()
        .map_err(SqliteRecoveryStateError::driver)?;
    bytes
        .map(|bytes| {
            RecoverySnapshot::decode(&bytes)
                .map_err(|error| SqliteRecoveryStateError::new(error.to_string()))
        })
        .transpose()
}

/// Read one quiescent database generation without letting SQLite create WAL or shared-memory
/// sidecars beside workspace-owned state.
///
/// SQLite's ordinary read-only mode may still create `-wal` and `-shm` files while opening a WAL
/// database. Diagnostic inspection cannot do that: its caller promises that previewing state is
/// not a workspace mutation. `immutable=1` suppresses those writes, so this path accepts only a
/// fully checkpointed database with no sidecars and verifies that the main file stayed the same
/// generation across the query. A live or crashed WAL family is refused for retry rather than
/// sampled incompletely.
fn load_immutable_snapshot(
    path: &Path,
    view_key: &[u8],
) -> Result<Option<RecoverySnapshot>, SqliteRecoveryStateError> {
    let opened_path = canonical_database_path(path)?;
    let path = opened_path.as_path();
    refuse_database_family_aliases(path)?;
    if !path.exists() {
        return Ok(None);
    }
    if database_sidecars(path)
        .iter()
        .any(|sidecar| sidecar.exists())
    {
        return Err(SqliteRecoveryStateError::new(
            "read-only recovery inspection requires a quiescent database without WAL sidecars",
        ));
    }
    let observed = std::fs::symlink_metadata(path)
        .map_err(|error| SqliteRecoveryStateError::new(error.to_string()))?;
    if !observed.is_file() || observed.file_type().is_symlink() {
        return Err(SqliteRecoveryStateError::new(
            "a recovery database must be a regular file",
        ));
    }
    let uri = immutable_database_uri(path)?;
    let connection = Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW
            | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(SqliteRecoveryStateError::driver)?;
    let primary_table_exists = connection
        .query_row(
            "SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1",
            [RECOVERY_STATE_TABLE],
            |_| Ok(()),
        )
        .optional()
        .map_err(SqliteRecoveryStateError::driver)?
        .is_some();
    let recovery_table_exists = connection
        .query_row(
            "SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1",
            [RECOVERY_OBSERVATION_TABLE],
            |_| Ok(()),
        )
        .optional()
        .map_err(SqliteRecoveryStateError::driver)?
        .is_some();
    let primary = if primary_table_exists {
        connection
            .query_row(
                "SELECT snapshot FROM mesh_recovery_state WHERE view_key = ?1",
                [view_key],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()
            .map_err(SqliteRecoveryStateError::driver)?
    } else {
        None
    };
    let recovery = if recovery_table_exists {
        connection
            .query_row(
                "SELECT snapshot FROM mesh_recovery_observation WHERE view_key = ?1",
                [view_key],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()
            .map_err(SqliteRecoveryStateError::driver)?
    } else {
        None
    };
    let completed = std::fs::symlink_metadata(path)
        .map_err(|error| SqliteRecoveryStateError::new(error.to_string()))?;
    if !same_database_snapshot(&observed, &completed)
        || database_sidecars(path)
            .iter()
            .any(|sidecar| sidecar.exists())
    {
        return Err(SqliteRecoveryStateError::new(
            "the recovery database changed while it was inspected read-only",
        ));
    }
    recovery
        .or(primary)
        .map(|bytes| {
            RecoverySnapshot::decode(&bytes)
                .map_err(|error| SqliteRecoveryStateError::new(error.to_string()))
        })
        .transpose()
}

fn database_sidecars(path: &Path) -> [PathBuf; 2] {
    [
        database_sidecar(path, "-wal"),
        database_sidecar(path, "-shm"),
    ]
}

fn database_sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut sidecar = path.as_os_str().to_os_string();
    sidecar.push(suffix);
    PathBuf::from(sidecar)
}

fn immutable_database_uri(path: &Path) -> Result<String, SqliteRecoveryStateError> {
    let path = path.to_str().ok_or_else(|| {
        SqliteRecoveryStateError::new(
            "read-only recovery inspection requires a UTF-8 database path",
        )
    })?;
    let mut uri = String::from("file:");
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in path.replace('\\', "/").bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b':' | b'.' | b'-' | b'_' | b'~') {
            uri.push(char::from(byte));
        } else {
            uri.push('%');
            uri.push(char::from(HEX[(byte >> 4) as usize]));
            uri.push(char::from(HEX[(byte & 0x0f) as usize]));
        }
    }
    uri.push_str("?immutable=1&mode=ro");
    Ok(uri)
}

#[cfg(unix)]
fn same_database_snapshot(observed: &std::fs::Metadata, completed: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;

    observed.is_file()
        && completed.is_file()
        && observed.dev() == completed.dev()
        && observed.ino() == completed.ino()
        && observed.nlink() == 1
        && completed.nlink() == 1
        && observed.len() == completed.len()
        && observed.mtime() == completed.mtime()
        && observed.mtime_nsec() == completed.mtime_nsec()
        && observed.ctime() == completed.ctime()
        && observed.ctime_nsec() == completed.ctime_nsec()
}

#[cfg(not(unix))]
fn same_database_snapshot(observed: &std::fs::Metadata, completed: &std::fs::Metadata) -> bool {
    observed.is_file()
        && completed.is_file()
        && observed.len() == completed.len()
        && observed.modified().ok() == completed.modified().ok()
        && observed.created().ok() == completed.created().ok()
}

impl SqlExecutor for Sqlite {
    type Error = SqliteError;

    fn execute_batch(&mut self, sql: &str) -> Result<(), Self::Error> {
        self.connection
            .execute_batch(sql)
            .map_err(SqliteError::driver)
    }

    fn read_table(&mut self, table: &Table) -> Result<Vec<Row>, Self::Error> {
        let projection = table
            .columns
            .iter()
            .map(|column| column.name)
            .collect::<Vec<_>>()
            .join(", ");
        let ordering = (1..=table.columns.len())
            .map(|column| column.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT {projection} FROM {} ORDER BY {ordering}",
            table.name
        );
        let mut statement = self.connection.prepare(&sql).map_err(SqliteError::driver)?;
        let rows = statement
            .query_map([], |source| {
                let values = table
                    .columns
                    .iter()
                    .enumerate()
                    .map(|(index, column)| match column.column_type {
                        ColumnType::Integer => source.get(index).map(Value::Integer),
                        ColumnType::Blob(_) => source.get(index).map(Value::Blob),
                    })
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(Row::new(values))
            })
            .map_err(SqliteError::driver)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(SqliteError::driver)
    }

    fn table_exists(&mut self, name: &str) -> Result<bool, Self::Error> {
        self.connection
            .query_row(
                "SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1",
                [name],
                |_| Ok(()),
            )
            .optional()
            .map(|row| row.is_some())
            .map_err(SqliteError::driver)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{read_all_tables, Store, CURRENT_VERSION};
    use std::fs;

    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "mesh-sqlite-driver-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    #[test]
    fn bundled_driver_creates_reopens_and_reads_the_real_schema() {
        let root = scratch("schema");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("scratch");
        let path = root.join("metadata.sqlite");

        {
            let mut store = Store::open(Sqlite::open(&path).expect("driver")).expect("store");
            assert_eq!(store.schema_version(), CURRENT_VERSION);
            assert_eq!(store.executor_mut().journal_mode().expect("mode"), "wal");
            let tables = read_all_tables(store.executor_mut()).expect("tables");
            assert!(!tables.is_empty());
        }

        let mut reopened =
            Store::open(Sqlite::open(&path).expect("reopen driver")).expect("reopen");
        assert_eq!(reopened.schema_version(), CURRENT_VERSION);
        assert!(
            path.exists(),
            "the production driver created a durable file"
        );
        assert_eq!(reopened.executor_mut().journal_mode().expect("mode"), "wal");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn recovery_state_uses_full_synchronous_durability() {
        let root = scratch("recovery-full-sync");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("scratch");
        let path = root.join("metadata.sqlite");

        let state = SqliteRecoveryState::open(&path, b"live-view").expect("recovery state");
        let journal_mode: String = state
            .connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .expect("journal mode");
        let synchronous: i64 = state
            .connection
            .pragma_query_value(None, "synchronous", |row| row.get(0))
            .expect("synchronous mode");
        let fullfsync: i64 = state
            .connection
            .pragma_query_value(None, "fullfsync", |row| row.get(0))
            .expect("fullfsync mode");
        let checkpoint_fullfsync: i64 = state
            .connection
            .pragma_query_value(None, "checkpoint_fullfsync", |row| row.get(0))
            .expect("checkpoint fullfsync mode");
        assert_eq!(journal_mode, "wal");
        assert_eq!(
            synchronous, 2,
            "recovery state is not record-derived and must use SQLite FULL durability"
        );
        assert_eq!(
            (fullfsync, checkpoint_fullfsync),
            (1, 1),
            "macOS recovery commits and WAL checkpoints require F_FULLFSYNC"
        );

        let index_connection = Connection::open(&path).expect("second connection");
        index_connection
            .pragma_update(None, "synchronous", "NORMAL")
            .expect("normal index connection");
        let still_full: i64 = state
            .connection
            .pragma_query_value(None, "synchronous", |row| row.get(0))
            .expect("recovery synchronous mode");
        assert_eq!(
            still_full, 2,
            "another connection's NORMAL setting must not weaken recovery commits"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn recovery_state_refuses_a_backend_that_did_not_enter_wal() {
        let problem = SqliteRecoveryState::open(":memory:", b"live-view")
            .expect_err("an in-memory database cannot satisfy durable WAL ownership");
        assert!(
            problem.to_string().contains("requires WAL mode"),
            "{problem}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn recovery_state_read_only_inspection_validates_without_repairing() {
        use std::os::unix::fs::PermissionsExt as _;

        let root = scratch("recovery-read-only-inspection");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("scratch");
        let database = root.join(RECOVERY_DATABASE_FILE_NAME);
        let expected = RecoverySnapshot::default();
        let mut state =
            SqliteRecoveryState::open(&database, b"live-view").expect("create recovery owner");
        state.persist(&expected).expect("persist recovery state");
        drop(state);

        fs::set_permissions(&database, fs::Permissions::from_mode(0o644))
            .expect("make a repair observable");
        let directory_entries = || {
            let mut entries = fs::read_dir(&root)
                .expect("read recovery directory")
                .map(|entry| entry.expect("directory entry").file_name())
                .collect::<Vec<_>>();
            entries.sort();
            entries
        };
        let before = directory_entries();
        let inspected = SqliteRecoveryState::inspect_read_only(&database, b"live-view")
            .expect("inspect read-only");
        assert_eq!(inspected, Some(expected));
        assert_eq!(
            directory_entries(),
            before,
            "read-only inspection created SQLite sidecars"
        );
        assert_eq!(
            fs::metadata(&database)
                .expect("database metadata")
                .permissions()
                .mode()
                & 0o777,
            0o644,
            "read-only inspection repaired permissions"
        );

        let wal = database_sidecar(&database, "-wal");
        fs::write(&wal, b"uncheckpointed bytes").expect("plant a WAL sidecar");
        let with_wal = directory_entries();
        assert!(
            SqliteRecoveryState::inspect_read_only(&database, b"live-view").is_err(),
            "immutable inspection ignored a WAL family it cannot safely fold"
        );
        assert_eq!(
            directory_entries(),
            with_wal,
            "refusing a WAL family changed workspace files"
        );
        fs::remove_file(wal).expect("remove planted WAL");

        fs::write(&database, b"not a sqlite database").expect("replace with corrupt bytes");
        assert!(
            SqliteRecoveryState::inspect_read_only(&database, b"live-view").is_err(),
            "corrupt recovery authority was reported as openable"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn isolated_read_only_inspection_matches_migration_precedence() {
        let root = scratch("recovery-read-only-migration");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("scratch");
        let recovery = root.join(RECOVERY_DATABASE_FILE_NAME);
        let legacy = root.join("metadata.sqlite");
        let expected = RecoverySnapshot::default();

        let mut legacy_state =
            SqliteRecoveryState::open(&legacy, b"live-view").expect("legacy owner");
        legacy_state
            .persist(&expected)
            .expect("persist legacy snapshot");
        drop(legacy_state);
        assert_eq!(
            SqliteRecoveryState::inspect_isolated_read_only(&recovery, &legacy, b"live-view")
                .expect("inspect migration source"),
            Some(expected.clone())
        );

        let mut isolated =
            SqliteRecoveryState::open(&recovery, b"live-view").expect("isolated owner");
        isolated
            .persist(&expected)
            .expect("persist isolated snapshot");
        drop(isolated);
        fs::write(&legacy, b"corrupt legacy bytes").expect("corrupt superseded legacy owner");
        assert_eq!(
            SqliteRecoveryState::inspect_isolated_read_only(&recovery, &legacy, b"live-view")
                .expect("isolated owner must win"),
            Some(expected)
        );

        fs::remove_file(&recovery).expect("remove isolated owner");
        assert!(
            SqliteRecoveryState::inspect_isolated_read_only(&recovery, &legacy, b"live-view")
                .is_err(),
            "corrupt migration source was reported as openable"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn recovery_state_never_follows_a_database_symlink() {
        use std::os::unix::fs::symlink;

        let root = scratch("recovery-no-follow");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("scratch");
        let target = root.join("outside.sqlite");
        let linked = root.join(RECOVERY_DATABASE_FILE_NAME);
        Connection::open(&target).expect("target database");
        symlink(&target, &linked).expect("database symlink");

        assert!(
            SqliteRecoveryState::open(&linked, b"live-view").is_err(),
            "a workspace-controlled symlink must never redirect recovery writes"
        );
        assert!(
            SqliteRecoveryState::inspect_read_only(&linked, b"live-view").is_err(),
            "read-only inspection followed a workspace-controlled symlink"
        );
        let connection = Connection::open(&target).expect("reopen target");
        let table_exists = connection
            .query_row(
                "SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1",
                [RECOVERY_STATE_TABLE],
                |_| Ok(()),
            )
            .optional()
            .expect("schema query");
        assert!(table_exists.is_none(), "the target was not modified");
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn recovery_state_database_family_is_private() {
        use std::os::unix::fs::PermissionsExt as _;

        let root = scratch("recovery-private-mode");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("scratch");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).expect("shared parent");
        let database = root.join(RECOVERY_DATABASE_FILE_NAME);

        let mut state = SqliteRecoveryState::open(&database, b"live-view").expect("recovery state");
        state
            .persist(&RecoverySnapshot::default())
            .expect("persist private recovery bytes");

        for path in [
            database.clone(),
            root.join(format!("{RECOVERY_DATABASE_FILE_NAME}-wal")),
            root.join(format!("{RECOVERY_DATABASE_FILE_NAME}-shm")),
        ] {
            if path.exists() {
                assert_eq!(
                    fs::metadata(&path)
                        .expect("database-family metadata")
                        .permissions()
                        .mode()
                        & 0o777,
                    0o600,
                    "recovery bytes must not be readable by another local user: {}",
                    path.display()
                );
            }
        }

        drop(state);
        fs::set_permissions(&database, fs::Permissions::from_mode(0o644))
            .expect("simulate an older broad database mode");
        drop(SqliteRecoveryState::open(&database, b"live-view").expect("secure older database"));
        assert_eq!(
            fs::metadata(&database)
                .expect("secured database")
                .permissions()
                .mode()
                & 0o777,
            0o600,
            "an older broad recovery owner is restricted before it is read"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn recovery_state_refuses_a_linked_wal_sidecar_before_open() {
        use std::os::unix::fs::symlink;

        let root = scratch("recovery-no-follow-wal");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("scratch");
        let target = root.join("outside-wal");
        let database = root.join(RECOVERY_DATABASE_FILE_NAME);
        let wal = root.join(format!("{RECOVERY_DATABASE_FILE_NAME}-wal"));
        fs::write(&target, b"outside bytes").expect("outside target");
        symlink(&target, &wal).expect("linked WAL path");

        let refused = SqliteRecoveryState::open(&database, b"live-view")
            .expect_err("a linked WAL path must fail before SQLite opens");
        assert!(refused.to_string().contains("symbolic link"), "{refused}");
        assert_eq!(
            fs::read(&target).expect("outside target after refusal"),
            b"outside bytes"
        );
        assert!(!database.exists(), "the database was not created");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn migration_admission_never_overwrites_a_winning_row() {
        let root = scratch("recovery-migration-insert-only");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("scratch");
        let path = root.join(RECOVERY_DATABASE_FILE_NAME);
        let mut state = SqliteRecoveryState::open(&path, b"live-view").expect("recovery state");
        state
            .connection
            .execute(
                "INSERT INTO mesh_recovery_state(view_key, snapshot) VALUES (?1, ?2)",
                rusqlite::params![&state.view_key, b"winning-row"],
            )
            .expect("concurrent winner");

        assert!(
            !state
                .persist_migration_if_absent(&RecoverySnapshot::default())
                .expect("insert-only migration"),
            "the already-admitted row wins the migration race"
        );
        let bytes: Vec<u8> = state
            .connection
            .query_row(
                "SELECT snapshot FROM mesh_recovery_state WHERE view_key = ?1",
                [&state.view_key],
                |row| row.get(0),
            )
            .expect("winning bytes");
        assert_eq!(bytes, b"winning-row");
        let _ = fs::remove_dir_all(&root);
    }
}
