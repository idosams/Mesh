#![allow(dead_code)]
//! A real SQLite driver for the tests, built on the system `sqlite3` binary.
//!
//! # Why this exists, and what it is not
//!
//! Production uses the bundled in-process driver. The tests retain this independent process
//! driver: it spawns `sqlite3` and feeds it statements, so everything the schema asserts is also
//! checked against the system SQLite build.
//!
//! # It fails loudly rather than skipping
//!
//! If `sqlite3` is not on `PATH`, every test using this harness fails with a message naming the
//! binary and the `MESH_STORE_SQLITE3` override. There is deliberately no skip: a suite that goes
//! green because it silently did not run the concurrency proof is worse than a red one, because
//! the criterion then reads as met.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mesh_store::{Row, SqlExecutor, Table, Value};

/// Whatever went wrong talking to `sqlite3`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SqliteError {
    /// What was being attempted.
    pub context: String,
    /// What `sqlite3` said, or why it could not be run.
    pub detail: String,
}

impl fmt::Display for SqliteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.context, self.detail)
    }
}

impl std::error::Error for SqliteError {}

/// The `sqlite3` binary to use.
pub fn sqlite3_binary() -> OsString {
    std::env::var_os("MESH_STORE_SQLITE3").unwrap_or_else(|| OsString::from("sqlite3"))
}

/// The version `sqlite3` reports, or an error explaining that it could not be run.
///
/// # Errors
///
/// [`SqliteError`] when the binary is absent or does not answer.
pub fn sqlite3_version() -> Result<String, SqliteError> {
    let output = Command::new(sqlite3_binary())
        .arg("--version")
        .output()
        .map_err(|error| SqliteError {
            context: "running sqlite3".to_owned(),
            detail: format!(
                "{error}. These integration tests drive the system sqlite3 binary independently; \
                 install it or point MESH_STORE_SQLITE3 at one. The tests do not \
                 skip, because a skipped concurrency proof reads as a met criterion."
            ),
        })?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// A temporary directory that removes itself.
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// Make one.
    ///
    /// # Panics
    ///
    /// If the directory cannot be created, which means the test cannot run at all.
    pub fn new(label: &str) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let unique = format!(
            "mesh-store-{label}-{}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
            nanos
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("a temporary directory");
        Self { path }
    }

    /// The directory.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A path inside it.
    pub fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// A [`SqlExecutor`] over the system `sqlite3` binary.
#[derive(Debug)]
pub struct Sqlite3 {
    database: PathBuf,
}

impl Sqlite3 {
    /// Talk to the database at this path, creating it on first write.
    pub fn at(database: impl Into<PathBuf>) -> Self {
        Self {
            database: database.into(),
        }
    }

    /// The database file.
    pub fn path(&self) -> &Path {
        &self.database
    }

    /// Run SQL and return stdout.
    ///
    /// # Every call is a new connection, so the per-connection pragmas are re-applied
    ///
    /// This harness spawns a `sqlite3` process per call, and a process is a connection. Anything
    /// `PRAGMAS` sets that is not persistent — `foreign_keys` above all — would otherwise be lost
    /// the moment `Store::open`'s process exited, and every `REFERENCES` clause in the schema would
    /// go unenforced for the rest of the run.
    ///
    /// That is not hypothetical. It is what this harness did until a mutation test turned
    /// `foreign_keys` off in `PRAGMAS` and nothing failed: the two tests that claimed to prove
    /// foreign keys were enforced were prefixing `PRAGMA foreign_keys = ON` themselves, so they
    /// proved that SQLite enforces foreign keys and nothing whatever about this crate.
    ///
    /// `journal_mode` is deliberately *not* re-applied — it is persistent, and re-applying it would
    /// convert the rollback-journal control in `tests/wal_concurrency.rs` back to WAL and destroy
    /// the experiment.
    ///
    /// A `PRAGMA name = value;` statement prints the value it settled on, so the batch is wrapped
    /// in `.output` redirection; without that every scalar read comes back with `5000` and `1000`
    /// in front of it.
    ///
    /// # Errors
    ///
    /// [`SqliteError`] when `sqlite3` cannot be run, or exits non-zero.
    pub fn run(&self, sql: &str) -> Result<String, SqliteError> {
        self.run_raw(&format!(
            ".output /dev/null\n{}\n.output stdout\n{sql}",
            mesh_store::connection_pragmas_sql()
        ))
    }

    /// Run SQL with nothing prepended, for the few checks that are about connection state itself.
    ///
    /// # Errors
    ///
    /// [`SqliteError`] when `sqlite3` cannot be run, or exits non-zero.
    pub fn run_raw(&self, sql: &str) -> Result<String, SqliteError> {
        let mut child = Command::new(sqlite3_binary())
            .arg("-batch")
            .arg("-bail")
            .arg("-noheader")
            .arg(&self.database)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| SqliteError {
                context: format!("spawning sqlite3 for {}", self.database.display()),
                detail: format!(
                    "{error}. Install sqlite3 or set MESH_STORE_SQLITE3; these tests do not skip."
                ),
            })?;

        child
            .stdin
            .take()
            .expect("stdin is piped")
            .write_all(sql.as_bytes())
            .map_err(|error| SqliteError {
                context: "writing SQL to sqlite3".to_owned(),
                detail: error.to_string(),
            })?;

        let output = child.wait_with_output().map_err(|error| SqliteError {
            context: "waiting for sqlite3".to_owned(),
            detail: error.to_string(),
        })?;

        if !output.status.success() {
            return Err(SqliteError {
                context: format!("sqlite3 exited {}", output.status),
                detail: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// The journal mode the database is in.
    ///
    /// # Errors
    ///
    /// [`SqliteError`] when the query fails.
    pub fn journal_mode(&self) -> Result<String, SqliteError> {
        Ok(self.run("PRAGMA journal_mode;")?.trim().to_lowercase())
    }

    /// One scalar as text.
    ///
    /// # Errors
    ///
    /// [`SqliteError`] when the query fails.
    pub fn scalar(&self, sql: &str) -> Result<String, SqliteError> {
        Ok(self.run(sql)?.trim().to_owned())
    }

    /// One scalar as an integer.
    ///
    /// # Errors
    ///
    /// [`SqliteError`] when the query fails or the answer is not an integer.
    pub fn count(&self, sql: &str) -> Result<i64, SqliteError> {
        let text = self.scalar(sql)?;
        text.parse().map_err(|_| SqliteError {
            context: format!("reading a count from {sql:?}"),
            detail: format!("{text:?} is not an integer"),
        })
    }
}

/// The `SELECT` that reads a table back in the order [`Row`]'s own ordering produces.
fn select_for(table: &Table) -> String {
    let projection: Vec<String> = table
        .columns
        .iter()
        .map(|column| match column.column_type {
            mesh_store::ColumnType::Blob(_) => format!("hex({})", column.name),
            mesh_store::ColumnType::Integer => column.name.to_owned(),
        })
        .collect();
    let ordering: Vec<String> = (1..=table.columns.len()).map(|n| n.to_string()).collect();
    format!(
        "SELECT {} FROM {} ORDER BY {};",
        projection.join(", "),
        table.name,
        ordering.join(", ")
    )
}

fn parse_row(table: &Table, line: &str) -> Result<Row, SqliteError> {
    let cells: Vec<&str> = line.split('|').collect();
    if cells.len() != table.columns.len() {
        return Err(SqliteError {
            context: format!("reading {}", table.name),
            detail: format!(
                "expected {} columns, found {} in {line:?}",
                table.columns.len(),
                cells.len()
            ),
        });
    }
    let mut values = Vec::with_capacity(cells.len());
    for (column, cell) in table.columns.iter().zip(cells) {
        let value = match column.column_type {
            mesh_store::ColumnType::Integer => {
                Value::Integer(cell.parse().map_err(|_| SqliteError {
                    context: format!("reading {}.{}", table.name, column.name),
                    detail: format!("{cell:?} is not an integer"),
                })?)
            }
            mesh_store::ColumnType::Blob(_) => {
                Value::Blob(decode_hex(cell).ok_or_else(|| SqliteError {
                    context: format!("reading {}.{}", table.name, column.name),
                    detail: format!("{cell:?} is not hex"),
                })?)
            }
        };
        values.push(value);
    }
    Ok(Row::new(values))
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(text.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let high = (pair[0] as char).to_digit(16)?;
        let low = (pair[1] as char).to_digit(16)?;
        out.push(u8::try_from(high * 16 + low).ok()?);
    }
    Some(out)
}

impl SqlExecutor for Sqlite3 {
    type Error = SqliteError;

    fn execute_batch(&mut self, sql: &str) -> Result<(), Self::Error> {
        self.run(sql).map(|_| ())
    }

    fn read_table(&mut self, table: &Table) -> Result<Vec<Row>, Self::Error> {
        let output = self.run(&select_for(table))?;
        output
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| parse_row(table, line))
            .collect()
    }

    fn table_exists(&mut self, name: &str) -> Result<bool, Self::Error> {
        let sql =
            format!("SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND name = '{name}';");
        Ok(self.count(&sql)? > 0)
    }
}

/// A `sqlite3` process holding an open, uncommitted write transaction.
///
/// This is the whole point of the concurrency test. A writer that has already committed proves
/// nothing about readers; what has to be held open is a transaction whose changes are *not yet*
/// visible, so that a reader either sees the old snapshot (WAL) or is refused (rollback journal).
pub struct HeldWriter {
    child: Child,
    stdin: std::process::ChildStdin,
    output: Arc<Mutex<Vec<String>>>,
}

/// The marker the writer prints once its transaction is open.
pub const HOLDING_MARKER: &str = "WRITER-HOLDING";

impl HeldWriter {
    /// Open a write transaction and hold it until [`HeldWriter::commit`] is called.
    ///
    /// `statements` runs inside the transaction, after `BEGIN EXCLUSIVE` and before the marker.
    ///
    /// # Errors
    ///
    /// [`SqliteError`] when `sqlite3` cannot be started, or does not reach the marker in time.
    pub fn begin(database: &Path, statements: &str) -> Result<Self, SqliteError> {
        let mut child = Command::new(sqlite3_binary())
            .arg("-batch")
            .arg("-bail")
            .arg("-noheader")
            .arg(database)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| SqliteError {
                context: "spawning the holding writer".to_owned(),
                detail: error.to_string(),
            })?;

        let stdout = child.stdout.take().expect("stdout is piped");
        let output = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&output);
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                sink.lock().expect("the output lock").push(line);
            }
        });

        let mut stdin = child.stdin.take().expect("stdin is piped");
        let script = format!(
            "PRAGMA busy_timeout = 0;\nBEGIN EXCLUSIVE;\n{statements}\nSELECT '{HOLDING_MARKER}';\n"
        );
        stdin
            .write_all(script.as_bytes())
            .and_then(|()| stdin.flush())
            .map_err(|error| SqliteError {
                context: "starting the held transaction".to_owned(),
                detail: error.to_string(),
            })?;

        let writer = Self {
            child,
            stdin,
            output,
        };
        writer.wait_for_marker()?;
        Ok(writer)
    }

    fn wait_for_marker(&self) -> Result<(), SqliteError> {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if self
                .output
                .lock()
                .expect("the output lock")
                .iter()
                .any(|line| line.contains(HOLDING_MARKER))
            {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Err(SqliteError {
            context: "waiting for the writer to hold its transaction".to_owned(),
            detail: format!("{HOLDING_MARKER} never appeared within ten seconds"),
        })
    }

    /// Commit and wait for the process to exit.
    ///
    /// # Errors
    ///
    /// [`SqliteError`] when the commit cannot be written or the process fails.
    pub fn commit(mut self) -> Result<(), SqliteError> {
        self.stdin
            .write_all(b"COMMIT;\n.quit\n")
            .and_then(|()| self.stdin.flush())
            .map_err(|error| SqliteError {
                context: "committing the held transaction".to_owned(),
                detail: error.to_string(),
            })?;
        drop(self.stdin);
        let status = self.child.wait().map_err(|error| SqliteError {
            context: "waiting for the holding writer".to_owned(),
            detail: error.to_string(),
        })?;
        if !status.success() {
            return Err(SqliteError {
                context: format!("the holding writer exited {status}"),
                detail: String::new(),
            });
        }
        Ok(())
    }
}

/// What one reader observed while the writer held its transaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReaderOutcome {
    /// Whether the read succeeded at all.
    pub succeeded: bool,
    /// What it read, when it read anything.
    pub observed: String,
    /// What `sqlite3` said, when it failed.
    pub failure: String,
    /// How long it took, in milliseconds.
    pub elapsed_millis: u128,
}

/// Run one reader with no busy timeout at all, so a lock is an immediate refusal rather than a
/// wait. Without a zero timeout a blocked reader would eventually succeed and the difference
/// between WAL and a rollback journal would be invisible — which is why the rollback-journal
/// control in `tests/wal_concurrency.rs` is what proves the timeout really is zero: if it were
/// not, that control would pass and the test would fail.
///
/// `.timeout 0` is used rather than `PRAGMA busy_timeout = 0` because the dot command prints
/// nothing, leaving stdout to carry the query's own result and nothing else.
pub fn read_once(database: &Path, query: &str) -> ReaderOutcome {
    let started = Instant::now();
    let result = Command::new(sqlite3_binary())
        .arg("-batch")
        .arg("-bail")
        .arg("-noheader")
        .arg("-cmd")
        .arg(".timeout 0")
        .arg(database)
        .arg(query)
        .output();
    let elapsed_millis = started.elapsed().as_millis();

    match result {
        Ok(output) => ReaderOutcome {
            succeeded: output.status.success(),
            observed: String::from_utf8_lossy(&output.stdout).trim().to_owned(),
            failure: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            elapsed_millis,
        },
        Err(error) => ReaderOutcome {
            succeeded: false,
            observed: String::new(),
            failure: error.to_string(),
            elapsed_millis,
        },
    }
}
