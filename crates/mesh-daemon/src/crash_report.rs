//! The last-durable-boundary report: what survived a crash, and what never existed.
//!
//! # The question this answers
//!
//! After a crash the first question is always *what survived*. `mesh-store` already decides that —
//! `mesh_store::scan_journal` walks the framed records and returns the last boundary every byte
//! before which is whole and verified — and [`crate::workspace::OpenWorkspace`] already calls it on
//! every open. What was missing was the sentence. This module turns the boundary into a report a
//! person can act on and a support bundle can carry, and it computes nothing: every number here is
//! read from the scan.
//!
//! # The two classes, and why the split is the whole point
//!
//! | Class | What it is | The promise |
//! |---|---|---|
//! | [`CrashReport::saved_records`] | whole records at or before the boundary, each forced to disk before its append returned | everything a person was told was **saved privately** is in here |
//! | [`CrashReport::unfinished_bytes`] | bytes after the boundary that never became a whole record | nobody was ever told about these, and setting them aside loses nothing anybody was promised |
//!
//! The first set is never smaller than the set of acknowledged work, and the direction matters. An
//! acknowledgement is only given after the append has returned, and the append forces the bytes to
//! disk before it returns — so a record that was acknowledged is on disk, and a record on disk may
//! additionally be one the process died before mentioning. `saved_records ⊇ acknowledged` is the
//! claim, it is one-directional on purpose, and `tests/crash-diagnostics.rs` kills a real process
//! at every step of plan §6.3's commit sequence to check it on the corpse.
//!
//! # What is deliberately not in the report
//!
//! No path, no record content, no actor name. Not because a bundle scanner will one day complain —
//! though `01KZC2XVD81A2183MWE5VDC9ER` builds exactly that scanner — but because a diagnostic whose
//! redaction is a later pass over its own output is a diagnostic somebody will ship unredacted.
//! [`CrashReport::to_bundle_section`] is counts, byte offsets, a severity word and one sentence,
//! and [`CrashReport::BUNDLE_SECTION`] is the name the bundle files it under.
//!
//! # Where the numbers stop
//!
//! The record boundary comes only from the immutable journal scan. When automatic checkpointing is
//! explicitly configured, [`crate::LiveDaemon`] supplies its already-restored
//! [`mesh_store::RecoverySnapshot`]; the read-only support path supplies the same value only after
//! immutable validation of the quiescent recovery owner. That adds three facts without re-deriving
//! them here: the last meaningful checkpoint whose existing durable-save acknowledgement was
//! admitted, the newest recovery-only prefix, and the activity window still open after it. A
//! report with no validated checkpoint snapshot says it is unavailable rather than turning
//! absence into an empty window.

use core::fmt;
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, Read as _};
use std::path::Path;
use std::time::{Duration, Instant};

use mesh_store::{rebuild, scan_journal, DurableBoundary, RecoverySnapshot, SqliteRecoveryState};

use crate::checkpoint_runtime::{recovery_database, LIVE_WORKSPACE_VIEW};
use crate::ipc::json::Json;
use crate::ipc::surface::severity_word;
use crate::recovery::{RecoveryDiagnostic, RecoveryOutcome, Severity, RECOVERY_BUDGET};
use crate::user_messages;
use crate::workspace::{OpenFailure, OpenWorkspace};

/// The most journal data a local support preview will hold in memory.
///
/// A support command is often run precisely because a workspace is damaged, so the file length is
/// not trusted input. The ordinary daemon recovery path remains responsible for journals beyond
/// this diagnostic ceiling; the preview fails closed instead of risking process-wide allocation.
const MAX_SUPPORT_JOURNAL_BYTES: u64 = 64 * 1024 * 1024;

/// What a start-up found, read as an answer to "what survived".
///
/// One value composed of two that already exist — the boundary the scan reported and the
/// diagnostic the start-up produced — rather than a second description of either. Nothing here
/// re-derives a count, a severity or a sentence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrashReport {
    boundary: DurableBoundary,
    unfinished_bytes: u64,
    diagnostic: RecoveryDiagnostic,
    checkpoint_state_available: bool,
    meaningful_checkpoint_through: Option<u64>,
    recovery_preserved_through: Option<u64>,
    open_activity: Option<(u64, u64)>,
}

/// One stable read-only report and the opened journal generation it describes.
///
/// The source identity is deliberately carried beside the report. Looking the workspace up again
/// after inspection would let a concurrent directory replacement or symlink retarget pair one
/// workspace's diagnostic facts with another workspace's public correlation.
pub(crate) struct ReadOnlyCrashInspection {
    pub(crate) report: CrashReport,
    pub(crate) source_identity: Option<SupportFileIdentity>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SupportFileIdentity {
    pub(crate) device: u64,
    pub(crate) inode: u64,
}

#[derive(Debug)]
struct ReadSupportJournal {
    bytes: Vec<u8>,
    source_identity: Option<SupportFileIdentity>,
}

impl CrashReport {
    /// The name a support bundle files this report under.
    ///
    /// A constant rather than a string at the call site, because the bundle is built by
    /// `01KZC2XVD81A2183MWE5VDC9ER` in another crate and a name spelt twice is a name that will be
    /// spelt two ways.
    pub const BUNDLE_SECTION: &'static str = "crash-diagnostics";

    /// Inspect the immutable journal without opening or changing the workspace.
    ///
    /// Support collection must remain safe when the supplied path is an import source rather than
    /// a managed workspace, and when the workspace itself is damaged. The normal workspace open
    /// path creates a journal and disposable index, creates content-store directories, and may
    /// quarantine corrupt content while materializing names. None of those mutations belong to a
    /// preview operation. This path therefore reads only `records.mesh`, verifies every frame,
    /// folds the records in memory, and inspects only the types of existing reserved SQLite and
    /// content-store entries. A valid journal cannot make `serving` true when one of those entries
    /// would prevent a real open. A quiescent canonical recovery snapshot is included in the same
    /// report; a live WAL family is refused rather than opened incompletely or mutated. No missing
    /// entry is created and no existing entry is repaired.
    #[must_use]
    pub(crate) fn inspect_read_only(workspace: &Path) -> ReadOnlyCrashInspection {
        let started = Instant::now();
        // Bind every later path lookup to one directory generation. The journal itself is opened
        // and checked through its file descriptor below, but the reserved runtime entries are
        // necessarily separate lookups. Without this outer snapshot, a concurrent rename or
        // symlink retarget can pair journal facts from workspace A with layout facts from B.
        let workspace_metadata = match fs::metadata(workspace) {
            Ok(metadata) if metadata.is_dir() => metadata,
            Ok(_) => {
                return ReadOnlyCrashInspection {
                    report: Self::of_failure(
                        &OpenFailure::Unreachable(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "the workspace root is not a directory",
                        )),
                        started.elapsed(),
                    ),
                    source_identity: None,
                };
            }
            Err(error) => {
                return ReadOnlyCrashInspection {
                    report: Self::of_failure(&OpenFailure::Unreachable(error), started.elapsed()),
                    source_identity: None,
                };
            }
        };
        let Ok(journal) = crate::workspace::workspace_record_file(workspace) else {
            return ReadOnlyCrashInspection {
                report: Self::of_read_only_layout_failure(
                    DurableBoundary::default(),
                    0,
                    started.elapsed(),
                ),
                source_identity: None,
            };
        };
        let metadata = match fs::symlink_metadata(&journal) {
            Ok(metadata) => metadata,
            Err(error) => {
                return ReadOnlyCrashInspection {
                    report: Self::of_failure(&OpenFailure::Unreachable(error), started.elapsed()),
                    source_identity: None,
                };
            }
        };
        if !metadata.file_type().is_file() {
            return ReadOnlyCrashInspection {
                report: Self::of_failure(
                    &OpenFailure::Unreachable(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "the workspace record entry is not a regular file",
                    )),
                    started.elapsed(),
                ),
                source_identity: None,
            };
        }
        let read = match read_support_journal(&journal, &metadata) {
            Ok(read) => read,
            Err(error) => {
                return ReadOnlyCrashInspection {
                    report: Self::of_failure(&OpenFailure::Unreachable(error), started.elapsed()),
                    source_identity: None,
                };
            }
        };
        let scan = match scan_journal(&read.bytes) {
            Ok(scan) => scan,
            Err(damage) => {
                return ReadOnlyCrashInspection {
                    report: Self::of_failure(&OpenFailure::Damaged(damage), started.elapsed()),
                    source_identity: read.source_identity,
                };
            }
        };
        let boundary = scan.boundary();
        let tail = scan.tail();
        if boundary.records == 0 && tail.is_fragment() {
            return ReadOnlyCrashInspection {
                report: Self::of_failure(
                    &OpenFailure::NothingReadable {
                        unfinished_bytes: tail.discarded_bytes(),
                    },
                    started.elapsed(),
                ),
                source_identity: read.source_identity,
            };
        }
        let (_, rebuilt) = match rebuild(scan.into_records(), Vec::new()) {
            Ok(rebuilt) => rebuilt,
            Err(error) => {
                return ReadOnlyCrashInspection {
                    report: Self::of_failure(
                        &OpenFailure::Contradictory {
                            detail: error.to_string(),
                            readable: boundary,
                        },
                        started.elapsed(),
                    ),
                    source_identity: read.source_identity,
                };
            }
        };
        let checkpoint_snapshot =
            match inspect_support_runtime_layout(workspace, &workspace_metadata) {
                Ok(snapshot) => snapshot,
                Err(_) => {
                    return ReadOnlyCrashInspection {
                        report: Self::of_read_only_layout_failure(
                            boundary,
                            tail.discarded_bytes(),
                            started.elapsed(),
                        ),
                        source_identity: read.source_identity,
                    };
                }
            };
        let unfinished_bytes = tail.discarded_bytes();
        let outcome = if tail.is_fragment() {
            RecoveryOutcome::RebuiltAfterAnInterruptedSave {
                records: boundary.records,
                rows: rebuilt.total_rows(),
                digest: rebuilt.digest,
                discarded_bytes: unfinished_bytes,
            }
        } else {
            RecoveryOutcome::Rebuilt {
                records: boundary.records,
                rows: rebuilt.total_rows(),
                digest: rebuilt.digest,
            }
        };
        let mut report = Self {
            boundary,
            unfinished_bytes,
            diagnostic: RecoveryDiagnostic::new(outcome, started.elapsed(), RECOVERY_BUDGET),
            checkpoint_state_available: false,
            meaningful_checkpoint_through: None,
            recovery_preserved_through: None,
            open_activity: None,
        };
        if let Some(snapshot) = checkpoint_snapshot.as_ref() {
            report.attach_checkpoint(snapshot);
        }
        ReadOnlyCrashInspection {
            report,
            source_identity: read.source_identity,
        }
    }

    /// The report for a workspace that opened.
    #[must_use]
    pub fn of(open: &OpenWorkspace) -> Self {
        Self {
            boundary: open.boundary(),
            unfinished_bytes: open.tail().discarded_bytes(),
            diagnostic: open.diagnostic().clone(),
            checkpoint_state_available: false,
            meaningful_checkpoint_through: None,
            recovery_preserved_through: None,
            open_activity: None,
        }
    }

    /// The report for an open workspace plus checkpoint truth already restored by the daemon.
    ///
    /// Crate-private because only the composition root owns both values under their required lock
    /// order. A report built directly from [`OpenWorkspace`] remains exact about journal durability
    /// and explicitly says checkpoint state was not supplied.
    #[must_use]
    pub(crate) fn of_with_checkpoint(open: &OpenWorkspace, snapshot: &RecoverySnapshot) -> Self {
        let mut report = Self::of(open);
        report.attach_checkpoint(snapshot);
        report
    }

    fn attach_checkpoint(&mut self, snapshot: &RecoverySnapshot) {
        self.checkpoint_state_available = true;
        self.meaningful_checkpoint_through = snapshot
            .last_meaningful()
            .map(|checkpoint| checkpoint.through().get());
        self.recovery_preserved_through = snapshot
            .latest_recovery()
            .map(|recovery| recovery.through().get());
        self.open_activity = snapshot
            .open_window()
            .map(|window| (window.from().get(), window.last().get()));
    }

    /// The report for a workspace that did not open, and how long the attempt took.
    ///
    /// A failed open still has an answer to "what survived": [`OpenFailure::readable_boundary`]
    /// carries how far the file read back as whole records, which is exactly the last durable
    /// boundary even though nothing was folded from it. Reporting zero here would be the same
    /// mistake as reporting a crash as a clean start, one variant further along.
    #[must_use]
    pub fn of_failure(failure: &OpenFailure, elapsed: Duration) -> Self {
        let outcome = match failure {
            OpenFailure::NothingReadable { unfinished_bytes } => {
                RecoveryOutcome::NothingDurableToRecover {
                    unfinished_bytes: *unfinished_bytes,
                }
            }
            other => RecoveryOutcome::Unrecoverable {
                detail: other.to_string(),
            },
        };
        Self {
            boundary: failure.readable_boundary(),
            unfinished_bytes: match failure {
                OpenFailure::NothingReadable { unfinished_bytes } => *unfinished_bytes,
                _ => 0,
            },
            diagnostic: RecoveryDiagnostic::new(outcome, elapsed, RECOVERY_BUDGET),
            checkpoint_state_available: false,
            meaningful_checkpoint_through: None,
            recovery_preserved_through: None,
            open_activity: None,
        }
    }

    fn of_read_only_layout_failure(
        boundary: DurableBoundary,
        unfinished_bytes: u64,
        elapsed: Duration,
    ) -> Self {
        Self {
            boundary,
            unfinished_bytes,
            diagnostic: RecoveryDiagnostic::new(
                RecoveryOutcome::Unrecoverable {
                    detail: "a reserved workspace runtime entry has an unsafe type".to_owned(),
                },
                elapsed,
                RECOVERY_BUDGET,
            ),
            checkpoint_state_available: false,
            meaningful_checkpoint_through: None,
            recovery_preserved_through: None,
            open_activity: None,
        }
    }

    /// The last durable boundary: how many whole records precede it, and where they end.
    #[must_use]
    pub const fn boundary(&self) -> DurableBoundary {
        self.boundary
    }

    /// Work that survived: whole records, each forced to disk before its append returned.
    ///
    /// Everything a person was told was saved privately is counted here. The set may be larger —
    /// a record whose append returned a moment before the crash is on disk whether or not anybody
    /// was told — and it is never smaller, which is the direction the durability claim needs.
    #[must_use]
    pub const fn saved_records(&self) -> u64 {
        self.boundary.records
    }

    /// Where the last surviving record ends, in bytes from the start of the file.
    #[must_use]
    pub const fn boundary_bytes(&self) -> u64 {
        self.boundary.byte_offset
    }

    /// Work that did not survive and that nobody was ever told about: bytes after the boundary
    /// that never became a whole record.
    #[must_use]
    pub const fn unfinished_bytes(&self) -> u64 {
        self.unfinished_bytes
    }

    /// Whether a save was in flight when the process died.
    #[must_use]
    pub const fn interrupted(&self) -> bool {
        self.unfinished_bytes > 0
    }

    /// Whether the workspace can be used after this.
    #[must_use]
    pub const fn is_serving(&self) -> bool {
        self.diagnostic.is_serving()
    }

    /// How serious it is.
    #[must_use]
    pub fn severity(&self) -> Severity {
        self.diagnostic.severity()
    }

    /// The diagnostic this report was read from.
    #[must_use]
    pub const fn diagnostic(&self) -> &RecoveryDiagnostic {
        &self.diagnostic
    }

    /// Whether this report includes a validated automatic-checkpoint snapshot.
    #[must_use]
    pub const fn checkpoint_state_available(&self) -> bool {
        self.checkpoint_state_available
    }

    /// The last complete meaningful checkpoint admitted with a real durable-save acknowledgement.
    #[must_use]
    pub const fn meaningful_checkpoint_through(&self) -> Option<u64> {
        self.meaningful_checkpoint_through
    }

    /// The newest prefix whose recovery-only bytes were verified and persisted.
    #[must_use]
    pub const fn recovery_preserved_through(&self) -> Option<u64> {
        self.recovery_preserved_through
    }

    /// The first and last observed sequence still waiting in the open activity window.
    #[must_use]
    pub const fn open_activity(&self) -> Option<(u64, u64)> {
        self.open_activity
    }

    /// The sentence a person reads, from [`crate::user_messages`] and from nowhere else.
    #[must_use]
    pub fn sentence(&self) -> String {
        user_messages::startup_sentence(self.diagnostic.outcome())
    }

    /// This report as the section a support bundle carries.
    ///
    /// Key order: `section`, `serving`, `severity`, `saved_records`, `boundary_bytes`,
    /// `unfinished_bytes`, `checkpoint_state_available`, `meaningful_checkpoint_through`,
    /// `recovery_preserved_through`, `open_activity_from`, `open_activity_through`, `elapsed_ms`,
    /// `sentence`. Fixed, because a bundle a person diffs against yesterday's is worth more than
    /// one whose keys move.
    ///
    /// No path, no record content, no actor: see this module's header.
    #[must_use]
    pub fn to_bundle_section(&self) -> Json {
        Json::object([
            ("section", Json::text(Self::BUNDLE_SECTION)),
            ("serving", Json::Bool(self.is_serving())),
            ("severity", Json::text(severity_word(self.severity()))),
            ("saved_records", Json::Number(self.saved_records())),
            ("boundary_bytes", Json::Number(self.boundary_bytes())),
            ("unfinished_bytes", Json::Number(self.unfinished_bytes())),
            (
                "checkpoint_state_available",
                Json::Bool(self.checkpoint_state_available()),
            ),
            (
                "meaningful_checkpoint_through",
                self.meaningful_checkpoint_through()
                    .map_or(Json::Null, sequence_json),
            ),
            (
                "recovery_preserved_through",
                self.recovery_preserved_through()
                    .map_or(Json::Null, sequence_json),
            ),
            (
                "open_activity_from",
                self.open_activity()
                    .map_or(Json::Null, |(from, _)| sequence_json(from)),
            ),
            (
                "open_activity_through",
                self.open_activity()
                    .map_or(Json::Null, |(_, through)| sequence_json(through)),
            ),
            (
                "elapsed_ms",
                Json::Number(
                    u64::try_from(self.diagnostic.elapsed().as_millis()).unwrap_or(u64::MAX),
                ),
            ),
            ("sentence", Json::text(self.sentence())),
        ])
    }
}

fn sequence_json(sequence: u64) -> Json {
    // JavaScript cannot represent every u64 exactly. Checkpoint sequence identities cross the
    // support boundary as canonical decimal strings so the scanner never rounds a valid durable
    // position before comparing it with the other checkpoint fields.
    Json::text(sequence.to_string())
}

fn inspect_support_runtime_layout(
    workspace: &Path,
    observed_workspace: &Metadata,
) -> Result<Option<RecoverySnapshot>, UnsafeSupportRuntimeLayout> {
    let matches_observed_generation = || {
        fs::metadata(workspace).is_ok_and(|metadata| {
            metadata.is_dir() && same_file_snapshot(observed_workspace, &metadata)
        })
    };

    let storage = crate::workspace::workspace_storage_root(workspace)
        .map_err(|_| UnsafeSupportRuntimeLayout)?;
    let workspace_root = mesh_store::WorkspaceRoot::new(storage.clone());
    let index_database = workspace_root.database();
    let recovery_database = recovery_database(index_database.as_path());

    if !matches_observed_generation()
        || !mesh_cas::StoreLayout::new(&storage)
            .directories()
            .iter()
            .all(|path| absent_or(path, |metadata| metadata.file_type().is_dir()))
        || !index_database
            .files()
            .iter()
            .all(|path| absent_or(path, |metadata| metadata.file_type().is_file()))
    {
        return Err(UnsafeSupportRuntimeLayout);
    }
    let snapshot = SqliteRecoveryState::inspect_isolated_read_only(
        &recovery_database,
        index_database.as_path(),
        LIVE_WORKSPACE_VIEW,
    )
    .map_err(|_| UnsafeSupportRuntimeLayout)?;
    if !matches_observed_generation() {
        return Err(UnsafeSupportRuntimeLayout);
    }
    Ok(snapshot)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct UnsafeSupportRuntimeLayout;

fn absent_or(path: &Path, predicate: impl FnOnce(&Metadata) -> bool) -> bool {
    match fs::symlink_metadata(path) {
        Ok(metadata) => predicate(&metadata),
        Err(error) => error.kind() == io::ErrorKind::NotFound,
    }
}

fn read_support_journal(journal: &Path, observed: &Metadata) -> io::Result<ReadSupportJournal> {
    if observed.len() > MAX_SUPPORT_JOURNAL_BYTES {
        return Err(support_journal_too_large());
    }

    let mut file = open_support_journal(journal)?;
    let opened = file.metadata()?;
    if !opened.is_file() || !same_file_snapshot(observed, &opened) {
        return Err(support_journal_changed());
    }
    let bytes = read_open_support_journal(&mut file, &opened)?;
    Ok(ReadSupportJournal {
        bytes,
        source_identity: support_file_identity(&opened),
    })
}

#[cfg(unix)]
fn support_file_identity(metadata: &Metadata) -> Option<SupportFileIdentity> {
    use std::os::unix::fs::MetadataExt as _;

    Some(SupportFileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(not(unix))]
fn support_file_identity(_metadata: &Metadata) -> Option<SupportFileIdentity> {
    None
}

/// Pin the exact regular journal generation named by `workspace` without reading its contents.
///
/// The live support client keeps the returned descriptor open until it accepts the daemon reply.
/// Device and inode therefore cannot be recycled underneath the correlation comparison, and a
/// pathname retarget after the command starts cannot relabel another workspace's report.
pub(crate) fn pin_support_journal(workspace: &Path) -> io::Result<(File, SupportFileIdentity)> {
    let journal = crate::workspace::workspace_record_file(workspace)?;
    let observed = fs::symlink_metadata(&journal)?;
    if !observed.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the workspace record entry is not a regular file",
        ));
    }
    let file = open_support_journal(&journal)?;
    let opened = file.metadata()?;
    if !opened.is_file() {
        return Err(support_journal_changed());
    }
    let observed_identity = support_file_identity(&observed).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "a stable support-journal identity is unavailable on this platform",
        )
    })?;
    let opened_identity = support_file_identity(&opened).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "a stable support-journal identity is unavailable on this platform",
        )
    })?;
    if observed_identity != opened_identity {
        return Err(support_journal_changed());
    }
    Ok((file, opened_identity))
}

#[cfg(any(
    target_os = "android",
    target_os = "freebsd",
    target_os = "ios",
    target_os = "linux",
    target_os = "macos",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn open_support_journal(journal: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt as _;

    // A regular-file check before open is not sufficient: a local process can replace the path
    // with a FIFO between those calls and make a diagnostic command wait forever for a peer.
    // O_NOFOLLOW closes the linked-target race and O_NONBLOCK makes special-file replacement
    // return to the descriptor check below instead of blocking. The values are stable ABI flags
    // on the two platform families Mesh ships and tests.
    #[cfg(any(target_os = "android", target_os = "linux"))]
    const SAFE_READ_FLAGS: i32 = 0o004_000 | 0o400_000;
    #[cfg(any(
        target_os = "freebsd",
        target_os = "ios",
        target_os = "macos",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    const SAFE_READ_FLAGS: i32 = 0x0004 | 0x0100;

    OpenOptions::new()
        .read(true)
        .custom_flags(SAFE_READ_FLAGS)
        .open(journal)
}

#[cfg(all(
    unix,
    not(any(
        target_os = "android",
        target_os = "freebsd",
        target_os = "ios",
        target_os = "linux",
        target_os = "macos",
        target_os = "netbsd",
        target_os = "openbsd"
    ))
))]
fn open_support_journal(_journal: &Path) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "a nonblocking no-follow support-journal open is not defined for this Unix platform",
    ))
}

#[cfg(not(unix))]
fn open_support_journal(_journal: &Path) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "read-only support diagnostics require a pinned safe file-open implementation",
    ))
}

fn read_open_support_journal(file: &mut File, opened: &Metadata) -> io::Result<Vec<u8>> {
    if opened.len() > MAX_SUPPORT_JOURNAL_BYTES {
        return Err(support_journal_too_large());
    }

    let mut bytes = Vec::with_capacity(opened.len() as usize);
    file.by_ref()
        .take(MAX_SUPPORT_JOURNAL_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_SUPPORT_JOURNAL_BYTES {
        return Err(support_journal_too_large());
    }

    // A live daemon appends to this file. Without a post-read check, collection can mistake an
    // append observed halfway through its read for crash damage and publish a false unfinished
    // tail. Refuse the unstable sample; a retry will inspect one complete journal generation.
    let completed = file.metadata()?;
    if !completed.is_file()
        || !same_file_snapshot(opened, &completed)
        || bytes.len() as u64 != opened.len()
    {
        return Err(support_journal_changed());
    }
    Ok(bytes)
}

fn support_journal_changed() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "the workspace record entry changed while it was being inspected",
    )
}

fn support_journal_too_large() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "the workspace journal exceeds the read-only support diagnostic limit",
    )
}

#[cfg(unix)]
fn same_file_snapshot(observed: &Metadata, opened: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;

    observed.dev() == opened.dev()
        && observed.ino() == opened.ino()
        && observed.len() == opened.len()
        && observed.mtime() == opened.mtime()
        && observed.mtime_nsec() == opened.mtime_nsec()
        && observed.ctime() == opened.ctime()
        && observed.ctime_nsec() == opened.ctime_nsec()
}

#[cfg(not(unix))]
fn same_file_snapshot(observed: &Metadata, opened: &Metadata) -> bool {
    observed.len() == opened.len()
        && observed.modified().ok() == opened.modified().ok()
        && observed.created().ok() == opened.created().ok()
}

impl fmt::Display for CrashReport {
    /// The operator's line: the boundary as numbers, then the diagnostic that produced it.
    ///
    /// Deliberately not [`Self::sentence`], which is the person's rendering of the same facts and
    /// carries no offsets. Two renderings, one source.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "last durable boundary: {} saved, {} unfinished bytes set aside — {}",
            self.boundary, self.unfinished_bytes, self.diagnostic
        )?;
        if self.checkpoint_state_available {
            write!(
                formatter,
                " — checkpoint state: meaningful through {}, recovery through {}, open activity {}",
                optional_sequence(self.meaningful_checkpoint_through),
                optional_sequence(self.recovery_preserved_through),
                self.open_activity.map_or_else(
                    || "none".to_owned(),
                    |(from, through)| format!("{from}..{through}"),
                ),
            )?;
        }
        Ok(())
    }
}

fn optional_sequence(sequence: Option<u64>) -> String {
    sequence.map_or_else(|| "none".to_owned(), |value| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;
    use std::io::Write as _;
    use std::path::PathBuf;

    use mesh_store::{frame_record, journal_records, no_session, OperationRecord, RecordDigest};

    use crate::workspace::RECORD_FILE_NAME;

    fn scratch(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "mesh-crash-report-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("mkdir");
        path
    }

    fn operation(id: u8, sequence: u64) -> mesh_store::StoredRecord {
        mesh_store::StoredRecord::Operation(OperationRecord {
            id: RecordDigest::from_bytes([id; 32]),
            actor: RecordDigest::from_bytes([9; 32]),
            actor_sequence: sequence,
            hlc_millis: 1_700_000_000_000,
            hlc_counter: 0,
            policy_epoch: 1,
            session: no_session(),
            payload_digest: RecordDigest::from_bytes([id; 32]),
            parents: Vec::new(),
        })
    }

    #[test]
    fn a_journal_append_during_read_is_refused_as_an_unstable_snapshot() {
        let root = scratch("append-during-support-read");
        let path = root.join(RECORD_FILE_NAME);
        fs::write(&path, frame_record(&operation(1, 1))).expect("first record");

        let mut reader = File::open(&path).expect("open support reader");
        let opened = reader.metadata().expect("opened metadata");
        let mut writer = fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open journal writer");
        writer
            .write_all(&frame_record(&operation(2, 2)))
            .expect("append while the support snapshot is open");
        writer.sync_all().expect("durable append");

        let error = read_open_support_journal(&mut reader, &opened)
            .expect_err("an unstable journal cannot become a crash report");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(
            error.to_string(),
            "the workspace record entry changed while it was being inspected"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_same_length_rewrite_during_read_is_refused_as_an_unstable_snapshot() {
        let root = scratch("rewrite-during-support-read");
        let path = root.join(RECORD_FILE_NAME);
        let original = frame_record(&operation(1, 1));
        let replacement = frame_record(&operation(2, 2));
        assert_eq!(
            original.len(),
            replacement.len(),
            "mutation must preserve length"
        );
        fs::write(&path, original).expect("original record");

        let mut reader = File::open(&path).expect("open support reader");
        let opened = reader.metadata().expect("opened metadata");
        std::thread::sleep(Duration::from_millis(2));
        let mut writer = fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&path)
            .expect("open journal rewriter");
        writer
            .write_all(&replacement)
            .expect("rewrite while the support snapshot is open");
        writer.sync_all().expect("durable rewrite");

        let error = read_open_support_journal(&mut reader, &opened)
            .expect_err("same-length rewritten bytes cannot become a support snapshot");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(
            error.to_string(),
            "the workspace record entry changed while it was being inspected"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn a_record_path_replaced_by_a_fifo_refuses_without_waiting_for_a_peer() {
        use std::process::{Command, Stdio};

        let root = scratch("fifo-before-support-open");
        let path = root.join(RECORD_FILE_NAME);
        fs::write(&path, frame_record(&operation(1, 1))).expect("original record");
        let observed = fs::symlink_metadata(&path).expect("observed regular journal");
        fs::remove_file(&path).expect("replace observed journal");
        assert!(Command::new("mkfifo")
            .arg(&path)
            .status()
            .expect("mkfifo starts")
            .success());

        // Unblock the vulnerable blocking open after a visible delay. A safe open returns before
        // this process reaches the FIFO and the child is killed below; the old File::open waits
        // for the peer, making the timing assertion an executable reproduction rather than a
        // source-shape claim.
        let mut delayed_writer = Command::new("sh")
            .arg("-c")
            .arg("sleep 0.25; printf x > \"$1\"")
            .arg("support-fifo-writer")
            .arg(&path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("delayed FIFO writer starts");
        let started = Instant::now();
        let error = read_support_journal(&path, &observed)
            .expect_err("a replacement FIFO cannot become a support snapshot");
        let elapsed = started.elapsed();
        if delayed_writer.try_wait().expect("writer status").is_none() {
            delayed_writer.kill().expect("stop delayed writer");
        }
        let _ = delayed_writer.wait();

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(
            elapsed < Duration::from_millis(100),
            "support collection blocked on a replacement FIFO for {elapsed:?}"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn a_replaced_workspace_root_cannot_validate_an_earlier_journal_generation() {
        let root = scratch("workspace-generation-first");
        let replacement = scratch("workspace-generation-second");
        let displaced = root.with_extension("displaced");
        fs::write(root.join(RECORD_FILE_NAME), frame_record(&operation(1, 1)))
            .expect("first journal");
        fs::write(
            replacement.join(RECORD_FILE_NAME),
            frame_record(&operation(2, 2)),
        )
        .expect("replacement journal");

        let journal = root.join(RECORD_FILE_NAME);
        let observed_workspace = fs::metadata(&root).expect("observe first workspace");
        let observed = fs::symlink_metadata(&journal).expect("observe first journal");
        let read = read_support_journal(&journal, &observed).expect("read first journal");
        assert_eq!(
            scan_journal(&read.bytes)
                .expect("scan first journal")
                .boundary()
                .records,
            1
        );

        fs::rename(&root, &displaced).expect("displace first workspace");
        fs::rename(&replacement, &root).expect("install replacement workspace");

        assert!(
            inspect_support_runtime_layout(&root, &observed_workspace).is_err(),
            "runtime-layout facts from a replacement root cannot validate the journal generation already read"
        );
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(displaced);
    }

    #[test]
    fn a_clean_open_reports_every_record_as_survived_and_nothing_unfinished() {
        let root = scratch("clean");
        let mut file =
            crate::workspace::RecordFile::open(&root.join(RECORD_FILE_NAME)).expect("open");
        let written =
            journal_records(&mut file, [&operation(1, 1), &operation(2, 2)]).expect("append");

        let open = OpenWorkspace::open(&root).expect("opens");
        let report = CrashReport::of(&open);

        assert_eq!(report.saved_records(), 2);
        assert_eq!(report.boundary_bytes(), written);
        assert_eq!(report.unfinished_bytes(), 0);
        assert!(!report.interrupted());
        assert!(report.is_serving());
        assert_eq!(report.severity(), Severity::Routine);
        let _ = fs::remove_dir_all(&root);
    }

    /// The split the criterion asks for, on the byte state a kill during an append leaves: one
    /// record acknowledged, one save in flight.
    #[test]
    fn the_report_separates_what_was_acknowledged_from_what_was_never_whole() {
        let root = scratch("split");
        let path = root.join(RECORD_FILE_NAME);
        let mut file = crate::workspace::RecordFile::open(&path).expect("open");
        let acknowledged = journal_records(&mut file, [&operation(1, 1)]).expect("append");
        let half = frame_record(&operation(2, 2));
        let torn = half.len() / 2;
        std::io::Write::write_all(
            &mut fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .expect("append handle"),
            &half[..torn],
        )
        .expect("a torn append");

        let open = OpenWorkspace::open(&root).expect("a fragment is not damage");
        let report = CrashReport::of(&open);

        assert_eq!(report.saved_records(), 1);
        assert_eq!(report.boundary_bytes(), acknowledged);
        assert_eq!(report.unfinished_bytes(), torn as u64);
        assert!(report.interrupted());
        assert!(report.is_serving(), "the survivor is still served");
        assert_eq!(report.severity(), Severity::Notable);
        assert!(report.sentence().contains("saved privately"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_workspace_with_no_boundary_reports_loudly_and_is_not_a_clean_start() {
        let root = scratch("no-boundary");
        let half = frame_record(&operation(1, 1));
        let torn = half.len() / 2;
        fs::write(root.join(RECORD_FILE_NAME), &half[..torn]).expect("write");

        let failure = OpenWorkspace::open(&root).expect_err("no boundary");
        let report = CrashReport::of_failure(&failure, Duration::from_millis(3));

        assert_eq!(report.saved_records(), 0);
        assert_eq!(report.unfinished_bytes(), torn as u64);
        assert!(!report.is_serving());
        assert_eq!(report.severity(), Severity::Blocking);

        let empty = scratch("empty");
        let opened = OpenWorkspace::open(&empty).expect("an empty folder opens");
        let clean = CrashReport::of(&opened);
        assert_eq!(clean.saved_records(), report.saved_records());
        assert_ne!(
            clean.sentence(),
            report.sentence(),
            "the two zero-record cases must not read the same"
        );
        assert!(clean.is_serving());
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&empty);
    }

    /// Damage stops the fold, and the report still answers where durability reached.
    #[test]
    fn a_damaged_workspace_still_reports_how_far_the_file_was_whole() {
        let root = scratch("damaged");
        let mut bytes = frame_record(&operation(1, 1));
        let whole = bytes.len() as u64;
        let mut second = frame_record(&operation(2, 2));
        let last = second.len() - 1;
        second[last] ^= 0xFF;
        bytes.extend_from_slice(&second);
        fs::write(root.join(RECORD_FILE_NAME), &bytes).expect("write");

        let failure = OpenWorkspace::open(&root).expect_err("damaged");
        let report = CrashReport::of_failure(&failure, Duration::from_millis(1));

        assert_eq!(report.saved_records(), 1);
        assert_eq!(report.boundary_bytes(), whole);
        assert!(!report.is_serving());
        assert_eq!(report.severity(), Severity::Blocking);
        let _ = fs::remove_dir_all(&root);
    }

    /// The bundle section is what a stranger may read. It carries counts and one sentence, and it
    /// carries no path, because the folder a person opened is theirs and not the diagnostic's.
    #[test]
    fn the_bundle_section_has_fixed_keys_and_no_path_in_it() {
        let root = scratch("bundle");
        let mut file =
            crate::workspace::RecordFile::open(&root.join(RECORD_FILE_NAME)).expect("open");
        journal_records(&mut file, [&operation(1, 1)]).expect("append");

        let open = OpenWorkspace::open(&root).expect("opens");
        let rendered = CrashReport::of(&open).to_bundle_section().to_string();

        for key in [
            "\"section\"",
            "\"serving\"",
            "\"severity\"",
            "\"saved_records\"",
            "\"boundary_bytes\"",
            "\"unfinished_bytes\"",
            "\"checkpoint_state_available\"",
            "\"meaningful_checkpoint_through\"",
            "\"recovery_preserved_through\"",
            "\"open_activity_from\"",
            "\"open_activity_through\"",
            "\"elapsed_ms\"",
            "\"sentence\"",
        ] {
            assert!(rendered.contains(key), "{key} is missing from {rendered}");
        }
        assert!(rendered.contains(CrashReport::BUNDLE_SECTION));
        assert!(rendered.contains("\"checkpoint_state_available\":false"));
        assert!(rendered.contains("\"meaningful_checkpoint_through\":null"));
        assert!(
            !rendered.contains(&root.display().to_string()),
            "the workspace path reached the bundle: {rendered}"
        );
        assert!(
            !rendered.contains("mesh-crash-report-bundle"),
            "a path fragment reached the bundle: {rendered}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn checkpoint_sequences_remain_exact_above_javascript_safe_integer_range() {
        let root = scratch("bundle-u64-sequence");
        let open = OpenWorkspace::open(&root).expect("opens");
        let mut report = CrashReport::of(&open);
        report.checkpoint_state_available = true;
        report.meaningful_checkpoint_through = Some(u64::MAX);
        report.recovery_preserved_through = Some(u64::MAX);

        let rendered = report.to_bundle_section().to_string();

        assert!(rendered.contains("\"meaningful_checkpoint_through\":\"18446744073709551615\""));
        assert!(rendered.contains("\"recovery_preserved_through\":\"18446744073709551615\""));
        assert!(!rendered.contains("\"meaningful_checkpoint_through\":18446744073709551615"));
        let _ = fs::remove_dir_all(root);
    }

    /// The order is fixed so two bundles taken a week apart can be read side by side.
    #[test]
    fn the_bundle_section_key_order_is_the_documented_one() {
        let root = scratch("order");
        let open = OpenWorkspace::open(&root).expect("opens");
        let rendered = CrashReport::of(&open).to_bundle_section().to_string();
        let mut at = 0usize;
        for key in [
            "section",
            "serving",
            "severity",
            "saved_records",
            "boundary_bytes",
            "unfinished_bytes",
            "checkpoint_state_available",
            "meaningful_checkpoint_through",
            "recovery_preserved_through",
            "open_activity_from",
            "open_activity_through",
            "elapsed_ms",
            "sentence",
        ] {
            let found = rendered
                .find(&format!("\"{key}\""))
                .unwrap_or_else(|| panic!("{key} is missing from {rendered}"));
            assert!(found >= at, "{key} is out of order in {rendered}");
            at = found;
        }
        let _ = fs::remove_dir_all(&root);
    }
}
