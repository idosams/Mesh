//! Write-time rejection: the only door a result row can enter the corpus by.
//!
//! Validation lives at the sink rather than at the reporting step because a row
//! that reaches a file is a row someone will eventually quote. Everything the
//! schema can check is checked here again, plus the publishing policy the
//! schema deliberately leaves out — minimum sample count, whether a dirty
//! worktree is acceptable, how many failed iterations a run may carry.

use crate::json::Json;
use crate::schema::{BenchmarkResult, SchemaError};
use std::fmt;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// What a sink refuses beyond the schema itself.
///
/// These are publishing decisions, not truths about the row, so they live in
/// one small config object rather than being scattered as constants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SinkPolicy {
    /// Whether a run from a dirty worktree may be filed.
    ///
    /// `false` by default: a stranger cannot reproduce a number taken from
    /// uncommitted code.
    pub allow_dirty_worktree: bool,
    /// The fewest timed samples a publishable run may carry.
    pub min_sample_count: u64,
    /// The largest tolerated share of failed iterations, in permille.
    pub max_failure_permille: u32,
}

impl SinkPolicy {
    /// The policy published Mesh numbers are held to.
    pub const PUBLISHABLE: SinkPolicy = SinkPolicy {
        allow_dirty_worktree: false,
        min_sample_count: 20,
        max_failure_permille: 0,
    };

    /// A deliberately looser policy for local exploration.
    ///
    /// Exists so the honest answer to "I just want a quick number" is a
    /// different sink, not a disabled check.
    pub const EXPLORATORY: SinkPolicy = SinkPolicy {
        allow_dirty_worktree: true,
        min_sample_count: 1,
        max_failure_permille: 1000,
    };
}

impl Default for SinkPolicy {
    fn default() -> Self {
        SinkPolicy::PUBLISHABLE
    }
}

/// A row was refused at the door.
#[derive(Debug)]
pub enum SinkError {
    /// The row is not a valid result row.
    Schema(SchemaError),
    /// The row is valid but not publishable under this policy.
    Policy {
        /// Why it was refused.
        reason: String,
    },
    /// The row was accepted but could not be written.
    Io(io::Error),
}

impl fmt::Display for SinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SinkError::Schema(error) => write!(f, "{error}"),
            SinkError::Policy { reason } => write!(f, "row refused by sink policy: {reason}"),
            SinkError::Io(error) => write!(f, "cannot write result row: {error}"),
        }
    }
}

impl std::error::Error for SinkError {}

impl From<SchemaError> for SinkError {
    fn from(error: SchemaError) -> Self {
        SinkError::Schema(error)
    }
}

/// Where accepted rows go. Replaceable: a file today, a service later.
pub trait ResultWriter {
    /// Appends one already-validated JSON line.
    fn write_line(&mut self, line: &str) -> io::Result<()>;
}

/// Appends rows to a JSON-lines file.
#[derive(Clone, Debug)]
pub struct FileWriter {
    path: PathBuf,
}

impl FileWriter {
    /// Writes to `path`, creating it if needed.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        FileWriter { path: path.into() }
    }

    /// The file rows are appended to.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl ResultWriter for FileWriter {
    fn write_line(&mut self, line: &str) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{line}")
    }
}

/// Collects rows in memory — used by tests and by dry runs.
#[derive(Clone, Debug, Default)]
pub struct MemoryWriter {
    lines: Vec<String>,
}

impl MemoryWriter {
    /// An empty writer.
    pub fn new() -> Self {
        MemoryWriter::default()
    }

    /// The lines written so far.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }
}

impl ResultWriter for MemoryWriter {
    fn write_line(&mut self, line: &str) -> io::Result<()> {
        self.lines.push(line.to_owned());
        Ok(())
    }
}

/// The validating door in front of a [`ResultWriter`].
#[derive(Clone, Debug)]
pub struct ValidatingSink<W> {
    writer: W,
    policy: SinkPolicy,
}

impl<W: ResultWriter> ValidatingSink<W> {
    /// Wraps `writer` with `policy`.
    pub fn new(writer: W, policy: SinkPolicy) -> Self {
        ValidatingSink { writer, policy }
    }

    /// The policy this sink applies.
    pub fn policy(&self) -> SinkPolicy {
        self.policy
    }

    /// The wrapped writer.
    pub fn writer(&self) -> &W {
        &self.writer
    }

    /// Validates and writes a typed row.
    pub fn accept(&mut self, result: &BenchmarkResult) -> Result<(), SinkError> {
        result.validate()?;
        self.check_policy(result)?;
        self.writer
            .write_line(&result.to_json_line())
            .map_err(SinkError::Io)
    }

    /// Decodes, validates and writes an untyped candidate row.
    ///
    /// This is the path that matters for the contract: a row assembled by
    /// anything other than the harness — a script, a hand edit, an older
    /// build — is refused by field name before it can be filed.
    pub fn accept_json(&mut self, candidate: &Json) -> Result<BenchmarkResult, SinkError> {
        let result = BenchmarkResult::from_json(candidate)?;
        self.check_policy(&result)?;
        self.writer
            .write_line(&result.to_json_line())
            .map_err(SinkError::Io)?;
        Ok(result)
    }

    fn check_policy(&self, result: &BenchmarkResult) -> Result<(), SinkError> {
        if result.repository.dirty && !self.policy.allow_dirty_worktree {
            return Err(SinkError::Policy {
                reason: format!(
                    "commit {} was measured from a dirty worktree, which nobody else can reproduce",
                    result.repository.commit
                ),
            });
        }
        if result.sample_count < self.policy.min_sample_count {
            return Err(SinkError::Policy {
                reason: format!(
                    "{} samples is below the minimum of {}",
                    result.sample_count, self.policy.min_sample_count
                ),
            });
        }
        let failure_permille = failure_permille(result);
        if failure_permille > self.policy.max_failure_permille {
            return Err(SinkError::Policy {
                reason: format!(
                    "{} of {} iterations failed ({failure_permille} permille), above the limit of {}",
                    result.failure_count, result.iterations_attempted, self.policy.max_failure_permille
                ),
            });
        }
        Ok(())
    }
}

fn failure_permille(result: &BenchmarkResult) -> u32 {
    if result.iterations_attempted == 0 {
        return 0;
    }
    u32::try_from(u128::from(result.failure_count) * 1000 / u128::from(result.iterations_attempted))
        .unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::REQUIRED_FIELDS;
    use crate::testing::sample_result;

    fn sink() -> ValidatingSink<MemoryWriter> {
        ValidatingSink::new(MemoryWriter::new(), SinkPolicy::PUBLISHABLE)
    }

    #[test]
    fn a_complete_row_is_written_once() {
        let mut sink = sink();
        sink.accept(&sample_result()).expect("complete row");
        assert_eq!(sink.writer().lines().len(), 1);
        assert_eq!(sink.writer().lines()[0], sample_result().to_json_line());
    }

    #[test]
    fn an_incomplete_row_never_reaches_the_writer() {
        let complete = sample_result().to_json();
        for field in REQUIRED_FIELDS {
            let mut sink = sink();
            let error = sink
                .accept_json(&complete.without_path(field))
                .expect_err("incomplete rows are refused");
            assert!(
                matches!(&error, SinkError::Schema(SchemaError::MissingField { field: named }) if named.as_str() == *field),
                "removing `{field}` was not refused by name: {error}"
            );
            assert!(
                sink.writer().lines().is_empty(),
                "a refused row must not be written"
            );
        }
    }

    #[test]
    fn a_dirty_worktree_is_not_publishable() {
        let mut row = sample_result();
        row.repository.dirty = true;
        let mut sink = sink();
        let error = sink
            .accept(&row)
            .expect_err("dirty runs are not reproducible");
        assert!(matches!(error, SinkError::Policy { .. }));
        assert!(sink.writer().lines().is_empty());
    }

    #[test]
    fn a_dirty_worktree_is_allowed_by_the_exploratory_policy() {
        let mut row = sample_result();
        row.repository.dirty = true;
        let mut sink = ValidatingSink::new(MemoryWriter::new(), SinkPolicy::EXPLORATORY);
        sink.accept(&row)
            .expect("exploration is allowed, publishing is not");
        assert_eq!(sink.writer().lines().len(), 1);
    }

    #[test]
    fn too_few_samples_are_not_publishable() {
        let row = crate::testing::sample_result_with_samples(&[10, 20, 30]);
        let mut sink = sink();
        let error = sink.accept(&row).expect_err("three samples is not a p99");
        assert!(matches!(error, SinkError::Policy { .. }));
    }

    #[test]
    fn failed_iterations_are_not_publishable_by_default() {
        let mut row = sample_result();
        row.failure_count = 1;
        row.iterations_attempted = row.sample_count + 1;
        let mut sink = sink();
        let error = sink
            .accept(&row)
            .expect_err("silent failures are not publishable");
        assert!(matches!(error, SinkError::Policy { .. }));
    }

    #[test]
    fn accepted_rows_round_trip_from_the_written_line() {
        let mut sink = sink();
        sink.accept(&sample_result()).expect("complete row");
        let line = &sink.writer().lines()[0];
        let decoded = BenchmarkResult::from_json_text(line).expect("written rows decode");
        assert_eq!(decoded, sample_result());
    }

    #[test]
    fn file_writers_append_rather_than_truncate() {
        let path = std::env::temp_dir().join(format!(
            "mesh-bench-sink-{}-{}.jsonl",
            std::process::id(),
            line!()
        ));
        let _ = std::fs::remove_file(&path);
        let mut sink = ValidatingSink::new(FileWriter::new(&path), SinkPolicy::PUBLISHABLE);
        sink.accept(&sample_result()).expect("first row");
        sink.accept(&sample_result()).expect("second row");
        let contents = std::fs::read_to_string(&path).expect("file exists");
        assert_eq!(contents.lines().count(), 2);
        std::fs::remove_file(&path).expect("cleanup");
    }
}
