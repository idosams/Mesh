//! Re-reading a folder, and what that can and cannot tell you.
//!
//! # One sentence this file is built around
//!
//! **A change worked out by comparing two readings of a folder is never recorded as a change Mesh
//! saw.** Plan §7.4: watchers coalesce and omit, so the fallback *must not be considered the
//! authoritative final mechanism*. Plan §4.9 already publishes the six-word confidence vocabulary
//! that says so — `ExactFilesystemRange` is an observation, `RecoveryDetected` is something found
//! while putting the pieces back together — and this file spends the existing words rather than
//! inventing a seventh. `mesh_materializer::AttributionConfidence` is that enumeration; three of
//! its six answers are inferences, and `is_observed()` is what separates them.
//!
//! # The one shape that makes acceptance criterion 2 unbreakable
//!
//! [`AttributedChange::confidence`] matches on the **change** before it matches on the source, and
//! [`WatchedChange::MovedInferred`] answers [`AttributionConfidence::RecoveryDetected`] in every
//! arm. There is no value of [`AttributedChange`] whose change is a worked-out move and whose
//! confidence is observed. A caller cannot construct one, an edit that wanted one would have to
//! delete that arm, and `a_worked_out_move_can_never_be_recorded_as_an_exact_operation` in
//! `crates/mesh-daemon/tests/folder-watch.rs` walks the whole cross product rather than one example.
//!
//! # Nothing here orders by a clock
//!
//! A reading of a folder is a name, an object identity, a kind, the portable metadata and a digest
//! of the bytes. It never uses modification time: two readings ordered or compared by time would
//! make the change set a function of when the reading happened, and a file written twice inside
//! one timestamp granularity would compare equal. On macOS and Linux, the kernel's creation-time
//! fields are retained only as an equality discriminator for one inode allocation, preventing an
//! immediately recycled inode from impersonating a move. They never order events or substitute
//! for content. `lamport → event_ulid → content-hash`, never wall-clock ordering.
//!
//! # Two things this file deliberately does not do
//!
//! 1. **It does not guess a move from a name.** A disappearance and an appearance are paired only
//!    when they carry the *same object identity*, and only when the pairing is unambiguous. A
//!    rename that also rewrote the file is reported as a disappearance and an appearance, which
//!    is less informative and is not a claim that would be wrong.
//! 2. **It does not pretend to see what happened between two readings.** A file created and
//!    removed again in the gap leaves no trace in either reading, and [`reconcile`] reports
//!    nothing about it. That is the `short-lived-work-is-missed` restriction, and
//!    `a_file_that_lived_and_died_between_two_readings_is_never_seen` is the test that holds this
//!    file to it rather than the doc comment.

use std::collections::BTreeMap;
#[cfg(target_os = "linux")]
use std::ffi::CString;
use std::fs;
use std::io;
#[cfg(target_os = "linux")]
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

use mesh_materializer::{
    AttributionConfidence, DestinationBefore, EventSequence, NormalizedName, ObjectId, ObjectKind,
    PortableMetadata, RenameBinding, RenameBindingEvidence, RenameEvidence,
    RenameEvidenceUnavailable, ViewId,
};
use mesh_types::{Blake3, ContentDigest as _, Digest32};

use super::{object_of, portable_of};

/// One entry as a reading of the folder found it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotEntry {
    object: ObjectId,
    incarnation: Option<ObjectIncarnation>,
    kind: ObjectKind,
    metadata: PortableMetadata,
    bytes: u64,
    digest: u64,
    verified_digest: Option<Digest32>,
}

/// A creation-time discriminator for one `(device, inode)` allocation.
///
/// Unix may recycle an inode immediately after unlink. The device/inode pair remains the public
/// adapter identity because it survives rename, while this private rescan discriminator prevents
/// a later allocation from being mistaken for that rename. Unsupported filesystems return None
/// and retain the fallback's documented conservative limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ObjectIncarnation {
    seconds: i64,
    nanoseconds: u32,
}

impl SnapshotEntry {
    /// The object identity, which on a real filesystem survives a rename.
    #[must_use]
    pub const fn object(&self) -> ObjectId {
        self.object
    }

    /// Whether it is a file or a directory.
    #[must_use]
    pub const fn kind(&self) -> ObjectKind {
        self.kind
    }

    /// The portable metadata.
    #[must_use]
    pub const fn metadata(&self) -> PortableMetadata {
        self.metadata
    }

    /// The file length found by this reading, or zero for a directory.
    #[must_use]
    pub const fn bytes(&self) -> u64 {
        self.bytes
    }

    /// A digest of the bytes, or `0` for a directory.
    ///
    /// A digest and not a length: two edits that keep a file the same length are the ordinary case
    /// — a character replaced, a flag flipped — and a comparison by length would report a folder
    /// as unchanged while its content had moved underneath it.
    #[must_use]
    pub const fn digest(&self) -> u64 {
        self.digest
    }
}

/// One recovery-detected regular-file state verified against a real post-rescan read.
///
/// This is deliberately one file, not a complete-folder checkpoint. A folder rescan can omit
/// short-lived or unreadable work, so constructing this value proves only that these exact bytes
/// matched one entry in the later reading. It provides no durable-boundary evidence and cannot
/// close a meaningful checkpoint on its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetectedFolderFile {
    relative_path: String,
    sequence: EventSequence,
    bytes: Vec<u8>,
}

impl DetectedFolderFile {
    /// Verify one appeared or content-changed regular file from two readings.
    ///
    /// # Errors
    ///
    /// Refuses an unconfined path, changed folder identity, a path not reported as new content,
    /// links/non-files, an I/O failure, or bytes that no longer match the later reading.
    pub fn from_rescan(
        folder: &Path,
        before: &Snapshot,
        after: &Snapshot,
        sequence: EventSequence,
        relative_path: &str,
    ) -> Result<Self, DetectedFolderFileError> {
        let relative = confined_relative_path(relative_path)?;
        let root_metadata = fs::symlink_metadata(folder)
            .map_err(|error| DetectedFolderFileError::io("metadata", folder, error))?;
        let current_root = root_metadata.is_dir().then(|| object_of(&root_metadata));
        if before.root.is_none() || before.root != after.root || after.root != current_root {
            return Err(DetectedFolderFileError::FolderIdentityChanged);
        }

        let changes = reconcile(before, after);
        let content_detected = changes.iter().any(|attributed| {
            matches!(
                attributed.change(),
                WatchedChange::Appeared { path, .. }
                    | WatchedChange::ContentChanged { path, .. }
                    if path == relative_path
            )
        });
        if !content_detected {
            return Err(DetectedFolderFileError::ContentChangeNotDetected {
                relative_path: relative_path.to_owned(),
            });
        }
        let expected = after.get(relative_path).ok_or_else(|| {
            DetectedFolderFileError::ContentChangeNotDetected {
                relative_path: relative_path.to_owned(),
            }
        })?;
        if expected.kind != ObjectKind::File {
            return Err(DetectedFolderFileError::NotRegularFile {
                relative_path: relative_path.to_owned(),
            });
        }

        let path = folder.join(relative);
        let before_read = regular_file_metadata(&path)?;
        let bytes =
            fs::read(&path).map_err(|error| DetectedFolderFileError::io("read", &path, error))?;
        let after_read = regular_file_metadata(&path)?;
        let length = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        let verified_digest = expected.verified_digest.ok_or_else(|| {
            DetectedFolderFileError::ChangedAfterRescan {
                relative_path: relative_path.to_owned(),
            }
        })?;
        if object_of(&before_read) != expected.object
            || object_of(&after_read) != expected.object
            || length != expected.bytes
            || Blake3::digest_bytes(&bytes) != verified_digest
        {
            return Err(DetectedFolderFileError::ChangedAfterRescan {
                relative_path: relative_path.to_owned(),
            });
        }
        Ok(Self {
            relative_path: relative_path.to_owned(),
            sequence,
            bytes,
        })
    }

    /// Path relative to the folder that was re-read.
    #[must_use]
    pub fn relative_path(&self) -> &str {
        &self.relative_path
    }

    /// The adapter-local event position associated with the later reading.
    #[must_use]
    pub const fn sequence(&self) -> EventSequence {
        self.sequence
    }

    /// Exact bytes re-read and verified against the later snapshot.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// This candidate was reconstructed by a rescan and is never an exact observation.
    #[must_use]
    pub const fn confidence(&self) -> AttributionConfidence {
        AttributionConfidence::RecoveryDetected
    }
}

/// Why a folder rescan could not produce one verified file candidate.
#[derive(Debug)]
pub enum DetectedFolderFileError {
    /// The requested path was empty, absolute, non-Unicode, or escaped the folder.
    UnconfinedPath {
        /// The refused path.
        path: PathBuf,
    },
    /// The two readings or current folder do not name one directory instance.
    FolderIdentityChanged,
    /// The rescan did not report appeared or changed content at this path.
    ContentChangeNotDetected {
        /// The requested relative path.
        relative_path: String,
    },
    /// The requested entry is not a real regular file.
    NotRegularFile {
        /// The requested relative path.
        relative_path: String,
    },
    /// The file no longer matches the post-change reading.
    ChangedAfterRescan {
        /// The requested relative path.
        relative_path: String,
    },
    /// A real filesystem operation failed.
    Io {
        /// Operation that failed.
        operation: &'static str,
        /// Target of the operation.
        path: PathBuf,
        /// Filesystem error.
        source: io::Error,
    },
}

impl DetectedFolderFileError {
    fn io(operation: &'static str, path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            operation,
            path: path.into(),
            source,
        }
    }
}

impl std::fmt::Display for DetectedFolderFileError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnconfinedPath { path } => {
                write!(
                    formatter,
                    "{} is not confined to the watched folder",
                    path.display()
                )
            }
            Self::FolderIdentityChanged => {
                formatter.write_str("the folder identity changed between readings")
            }
            Self::ContentChangeNotDetected { relative_path } => write!(
                formatter,
                "the folder rescan found no new content at {relative_path}"
            ),
            Self::NotRegularFile { relative_path } => {
                write!(formatter, "{relative_path} is not a regular file")
            }
            Self::ChangedAfterRescan { relative_path } => write!(
                formatter,
                "{relative_path} changed after the folder was re-read"
            ),
            Self::Io {
                operation,
                path,
                source,
            } => write!(formatter, "{operation} {} failed: {source}", path.display()),
        }
    }
}

impl std::error::Error for DetectedFolderFileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn confined_relative_path(path: &str) -> Result<PathBuf, DetectedFolderFileError> {
    let path = Path::new(path);
    let mut confined = PathBuf::new();
    for component in path.components() {
        let std::path::Component::Normal(name) = component else {
            return Err(DetectedFolderFileError::UnconfinedPath {
                path: path.to_path_buf(),
            });
        };
        if name.to_str().is_none() {
            return Err(DetectedFolderFileError::UnconfinedPath {
                path: path.to_path_buf(),
            });
        }
        confined.push(name);
    }
    if confined.as_os_str().is_empty() {
        return Err(DetectedFolderFileError::UnconfinedPath {
            path: path.to_path_buf(),
        });
    }
    Ok(confined)
}

fn regular_file_metadata(path: &Path) -> Result<fs::Metadata, DetectedFolderFileError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| DetectedFolderFileError::io("metadata", path, error))?;
    if metadata.is_file() && !metadata.file_type().is_symlink() {
        Ok(metadata)
    } else {
        Err(DetectedFolderFileError::NotRegularFile {
            relative_path: path.to_string_lossy().into_owned(),
        })
    }
}

/// One reading of a whole folder.
///
/// Keyed by the path relative to the folder root, written with `/` on every platform, so two
/// readings are comparable and iteration is in byte order of the path.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    root: Option<ObjectId>,
    root_incarnation: Option<ObjectIncarnation>,
    entries: BTreeMap<String, SnapshotEntry>,
}

impl Snapshot {
    /// Read `root` from end to end.
    ///
    /// An unreadable entry is left out rather than reported: this is the fallback, and a folder it
    /// cannot fully read is exactly the situation it exists to survive.
    #[must_use]
    pub fn of(root: &Path) -> Self {
        let mut entries = BTreeMap::new();
        walk(root, &mut entries);
        let root_metadata = fs::symlink_metadata(root).ok().filter(fs::Metadata::is_dir);
        let root_incarnation = root_metadata
            .as_ref()
            .and_then(|metadata| object_incarnation(root, metadata));
        let root = root_metadata.map(|metadata| object_of(&metadata));
        Self {
            root,
            root_incarnation,
            entries,
        }
    }

    /// What the reading found at `path`, if anything.
    #[must_use]
    pub fn get(&self, path: &str) -> Option<&SnapshotEntry> {
        self.entries.get(path)
    }

    /// Every path found, in byte order.
    #[must_use]
    pub fn paths(&self) -> Vec<&str> {
        self.entries.keys().map(String::as_str).collect()
    }

    /// How many entries the reading found.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the reading found nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn object_at_or_root(&self, path: &str) -> Option<ObjectId> {
        if path.is_empty() {
            self.root
        } else {
            self.get(path).map(SnapshotEntry::object)
        }
    }

    fn occurrences(&self, object: ObjectId) -> usize {
        self.entries
            .values()
            .filter(|entry| entry.object == object)
            .count()
    }
}

fn binding(snapshot: &Snapshot, path: &str, object: ObjectId) -> Option<RenameBinding> {
    let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
    let parent = snapshot.object_at_or_root(parent)?;
    let name = NormalizedName::new(name).ok()?;
    Some(RenameBinding::new(parent, name, object))
}

/// Derive rename evidence from two complete readings only when identity makes the pairing unique.
///
/// Paths select entries to inspect; they are never treated as identity. Missing roots, missing
/// entries, a source that remains bound, duplicate identities, or a changed moved object all
/// return the contract's explicit unsupported value rather than a guessed binding.
#[must_use]
pub fn rename_evidence(
    before: &Snapshot,
    after: &Snapshot,
    view: ViewId,
    sequence: EventSequence,
    from: &str,
    to: &str,
) -> RenameEvidence {
    let unavailable = || RenameEvidence::Unsupported(RenameEvidenceUnavailable::all());
    if before.root.is_none()
        || before.root != after.root
        || !same_optional_incarnation(before.root_incarnation, after.root_incarnation)
    {
        return unavailable();
    }
    let Some(source) = before.get(from) else {
        return unavailable();
    };
    let Some(destination_after) = after.get(to) else {
        return unavailable();
    };
    if source.object != destination_after.object
        || after.get(from).is_some()
        || before.occurrences(source.object) != 1
        || after.occurrences(source.object) != 1
    {
        return unavailable();
    }
    let Some(source_before) = binding(before, from, source.object) else {
        return unavailable();
    };
    let Some(destination_after_binding) = binding(after, to, destination_after.object) else {
        return unavailable();
    };
    let destination_before = before.get(to).map_or(DestinationBefore::Unbound, |entry| {
        DestinationBefore::Bound(entry.object)
    });
    RenameEvidence::Available(RenameBindingEvidence::new(
        view,
        sequence,
        source_before,
        destination_before,
        destination_after_binding,
    ))
}

fn walk(root: &Path, into: &mut BTreeMap<String, SnapshotEntry>) {
    // Use heap-backed work rather than recursive calls: the filesystem supplies the depth, and a
    // valid file does not disappear merely because it is the sixty-fifth directory down. Links
    // are inspected with `symlink_metadata` and never added to `pending`, so a link cycle cannot
    // make this traversal unbounded.
    let mut pending: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(at) = pending.pop() {
        let Ok(listing) = fs::read_dir(at) else {
            continue;
        };
        for entry in listing.flatten() {
            // A managed native workspace keeps its journal, index and CAS beneath this reserved
            // root namespace. Those bytes are daemon state, not user work: observing them would
            // make every private save look like another filesystem edit and could recursively
            // checkpoint Mesh's own checkpoint. The reservation is root-scoped, matching the
            // managed-path and import boundaries; an ordinary nested directory with the same
            // basename remains workspace content.
            if entry.path().parent() == Some(root)
                && entry.file_name() == crate::workspace::STORAGE_DIRECTORY_NAME
            {
                continue;
            }
            let path = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            let kind = if metadata.is_dir() {
                ObjectKind::Directory
            } else if metadata.is_file() {
                ObjectKind::File
            } else {
                // Neither a file nor a directory. `Symlink` is reserved at
                // `mesh-workspace-adapter/1` and this backend does not declare it, so a link is not
                // something this reading has any way to describe.
                continue;
            };
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            let Some(key) = relative.to_str().map(|text| text.replace('\\', "/")) else {
                continue;
            };
            let (digest, verified_digest) = match kind {
                ObjectKind::Directory => (0, None),
                ObjectKind::File => fs::read(&path).map_or((0, None), |bytes| {
                    (fnv1a(&bytes), Some(Blake3::digest_bytes(&bytes)))
                }),
            };
            into.insert(
                key,
                SnapshotEntry {
                    object: object_of(&metadata),
                    incarnation: object_incarnation(&path, &metadata),
                    kind,
                    metadata: portable_of(&metadata),
                    bytes: if kind == ObjectKind::File {
                        metadata.len()
                    } else {
                        0
                    },
                    digest,
                    verified_digest,
                },
            );
            if kind == ObjectKind::Directory {
                pending.push(path);
            }
        }
    }
}

/// FNV-1a over the bytes, so a change of the same length is still a change.
///
/// Not the protocol digest: `docs/protocol.md` names BLAKE3, and nothing in this crate may mint an
/// identifier. This compares two readings of one folder on one machine, it never leaves this
/// process, it is never written down, and nothing derives an identifier from it. A collision costs
/// one missed `content-changed` between two readings, which is a restriction this backend already
/// publishes as `changes-are-found-late` rather than a new one.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// One difference between two readings of a folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WatchedChange {
    /// A path is in the later reading and not in the earlier one.
    Appeared {
        /// Where it was found, relative to the folder root.
        path: String,
        /// The object identity found there.
        object: ObjectId,
    },
    /// A path is in the earlier reading and not in the later one.
    Disappeared {
        /// Where it used to be, relative to the folder root.
        path: String,
        /// The object identity that used to be there.
        object: ObjectId,
    },
    /// A path is in both and the bytes differ.
    ContentChanged {
        /// Where it is, relative to the folder root.
        path: String,
        /// The object identity, which did not change.
        object: ObjectId,
    },
    /// A path is in both and the portable metadata differs.
    MetadataChanged {
        /// Where it is, relative to the folder root.
        path: String,
        /// The object identity, which did not change.
        object: ObjectId,
    },
    /// Mesh performed this move itself, through the view.
    Moved {
        /// Where it was, relative to the folder root.
        from: String,
        /// Where it is now, relative to the folder root.
        to: String,
        /// The object identity, which a move does not change.
        object: ObjectId,
    },
    /// One object left one path and arrived at another, and nothing watched it happen.
    MovedInferred {
        /// Where it was, relative to the folder root.
        from: String,
        /// Where it is now, relative to the folder root.
        to: String,
        /// The object identity that pairs the two, and the only reason to believe the pairing.
        object: ObjectId,
    },
}

impl WatchedChange {
    /// The published name of the difference.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Appeared { .. } => "appeared",
            Self::Disappeared { .. } => "disappeared",
            Self::ContentChanged { .. } => "content-changed",
            Self::MetadataChanged { .. } => "metadata-changed",
            Self::Moved { .. } => "moved",
            Self::MovedInferred { .. } => "moved-inferred",
        }
    }

    /// The path the change is sorted and reported under.
    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            Self::Appeared { path, .. }
            | Self::Disappeared { path, .. }
            | Self::ContentChanged { path, .. }
            | Self::MetadataChanged { path, .. } => path,
            Self::Moved { from, .. } | Self::MovedInferred { from, .. } => from,
        }
    }

    /// The object the change is about.
    #[must_use]
    pub const fn object(&self) -> ObjectId {
        match self {
            Self::Appeared { object, .. }
            | Self::Disappeared { object, .. }
            | Self::ContentChanged { object, .. }
            | Self::MetadataChanged { object, .. }
            | Self::Moved { object, .. }
            | Self::MovedInferred { object, .. } => *object,
        }
    }

    /// The slot this variant sorts into when two changes share a path.
    const fn order(&self) -> u8 {
        match self {
            Self::MovedInferred { .. } => 0,
            Self::Moved { .. } => 1,
            Self::Disappeared { .. } => 2,
            Self::Appeared { .. } => 3,
            Self::ContentChanged { .. } => 4,
            Self::MetadataChanged { .. } => 5,
        }
    }
}

/// Where a change came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChangeSource {
    /// Mesh did it, through a [`mesh_materializer::WorkspaceView`] method.
    PerformedByMesh,
    /// Mesh found it by comparing two readings of the folder.
    FoundByRescan,
}

/// One change, and how much Mesh actually knows about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttributedChange {
    change: WatchedChange,
    source: ChangeSource,
}

impl AttributedChange {
    /// A change Mesh performed itself.
    #[must_use]
    pub const fn performed_by_mesh(change: WatchedChange) -> Self {
        Self {
            change,
            source: ChangeSource::PerformedByMesh,
        }
    }

    /// A change Mesh found by re-reading the folder.
    #[must_use]
    pub const fn found_by_rescan(change: WatchedChange) -> Self {
        Self {
            change,
            source: ChangeSource::FoundByRescan,
        }
    }

    /// The change.
    #[must_use]
    pub const fn change(&self) -> &WatchedChange {
        &self.change
    }

    /// Where it came from.
    #[must_use]
    pub const fn source(&self) -> ChangeSource {
        self.source
    }

    /// How confident Mesh is, in plan §4.9's vocabulary.
    ///
    /// The move-that-was-worked-out arm comes **first**, and it is the whole of acceptance
    /// criterion 2: whatever a caller claims about where the change came from, a
    /// [`WatchedChange::MovedInferred`] answers an inference. There is no path through this
    /// function that pairs it with an observed confidence.
    #[must_use]
    pub const fn confidence(&self) -> AttributionConfidence {
        match self.change {
            WatchedChange::MovedInferred { .. } => AttributionConfidence::RecoveryDetected,
            _ => match self.source {
                ChangeSource::PerformedByMesh => AttributionConfidence::ExactFilesystemRange,
                ChangeSource::FoundByRescan => AttributionConfidence::RecoveryDetected,
            },
        }
    }

    /// Whether this may be presented to a person as something Mesh saw.
    #[must_use]
    pub const fn is_exact(&self) -> bool {
        is_observed(self.confidence())
    }
}

/// Whether a confidence may be presented to a human as an observation.
///
/// Plan §4.8: *"This distinction prevents overstating what the system knows."* Two of the six
/// answers are observations and four are not.
///
/// Written here rather than called, and the reason is worth stating: `mesh-operations` publishes
/// exactly this predicate as `AttributionConfidence::is_observed`, but `mesh-materializer` may
/// declare no dependency at all (`src/lib.rs`), so `src/operation.rs` is a *mirror* of that
/// enumeration built to be deleted the day the edge is allowed — and the mirror carries the six
/// variants without the method. This is the second writing-down of one rule, which is a cost;
/// `the_two_writings_of_observed_confidence_agree` in
/// `crates/mesh-daemon/tests/folder-watch.rs` reads
/// `crates/mesh-operations/src/operation.rs` as source text and fails if they stop matching.
#[must_use]
pub const fn is_observed(confidence: AttributionConfidence) -> bool {
    matches!(
        confidence,
        AttributionConfidence::ExactIntegratedRead | AttributionConfidence::ExactFilesystemRange
    )
}

/// Every difference between two readings of one folder, in a deterministic order.
///
/// The whole of the fallback's reconciliation path: it is a pure function of the two readings, so
/// it does not matter how many change notifications were dropped between them — none, some, or all
/// of them. That is what makes a missed change recoverable, and
/// `a_missed_change_is_recovered_by_re_reading_the_folder` is where it is checked rather than
/// claimed.
#[must_use]
pub fn reconcile(before: &Snapshot, after: &Snapshot) -> Vec<AttributedChange> {
    let mut gone: Vec<(&str, &SnapshotEntry)> = Vec::new();
    let mut fresh: Vec<(&str, &SnapshotEntry)> = Vec::new();
    let mut changes: Vec<WatchedChange> = Vec::new();

    for (path, entry) in &before.entries {
        match after.entries.get(path) {
            None => gone.push((path, entry)),
            Some(now) => {
                if now.object != entry.object
                    || !same_optional_incarnation(entry.incarnation, now.incarnation)
                {
                    // The name survived and the object under it did not. Two separate facts, and
                    // collapsing them into "changed" would hide that the old object left.
                    gone.push((path, entry));
                    fresh.push((path, now));
                    continue;
                }
                if now.digest != entry.digest || now.bytes != entry.bytes {
                    changes.push(WatchedChange::ContentChanged {
                        path: path.clone(),
                        object: now.object,
                    });
                }
                if now.metadata != entry.metadata {
                    changes.push(WatchedChange::MetadataChanged {
                        path: path.clone(),
                        object: now.object,
                    });
                }
            }
        }
    }
    for (path, entry) in &after.entries {
        if !before.entries.contains_key(path) {
            fresh.push((path, entry));
        }
    }

    let mut paired: Vec<usize> = Vec::new();
    for departure in &gone {
        let (from, left) = *departure;
        // Unambiguous only. Two arrivals carrying one identity is inode reuse or a hard link, and
        // a guess between them would be exactly the overstatement plan §4.9 exists to prevent.
        let mut matches: Vec<usize> = Vec::new();
        for (index, arrival) in fresh.iter().enumerate() {
            if arrival.1.object == left.object
                && same_optional_incarnation(left.incarnation, arrival.1.incarnation)
                && arrival.1.kind == left.kind
                && arrival.1.bytes == left.bytes
                && arrival.1.digest == left.digest
                && arrival.1.metadata == left.metadata
                && !paired.contains(&index)
            {
                matches.push(index);
            }
        }
        if matches.len() == 1 {
            let index = matches[0];
            paired.push(index);
            changes.push(WatchedChange::MovedInferred {
                from: from.to_owned(),
                to: fresh[index].0.to_owned(),
                object: left.object,
            });
        } else {
            changes.push(WatchedChange::Disappeared {
                path: from.to_owned(),
                object: left.object,
            });
        }
    }
    for (index, arrival) in fresh.iter().enumerate() {
        if !paired.contains(&index) {
            changes.push(WatchedChange::Appeared {
                path: arrival.0.to_owned(),
                object: arrival.1.object,
            });
        }
    }

    changes.sort_by(|left, right| {
        left.path()
            .as_bytes()
            .cmp(right.path().as_bytes())
            .then(left.order().cmp(&right.order()))
    });
    changes
        .into_iter()
        .map(AttributedChange::found_by_rescan)
        .collect()
}

fn same_optional_incarnation(
    left: Option<ObjectIncarnation>,
    right: Option<ObjectIncarnation>,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left == right,
        // Some filesystems do not expose birth time. Preserve their existing inode-based fallback
        // rather than claiming a discriminator the kernel did not provide.
        (None, None) => true,
        _ => false,
    }
}

#[cfg(target_os = "macos")]
fn object_incarnation(_path: &Path, metadata: &fs::Metadata) -> Option<ObjectIncarnation> {
    use std::os::darwin::fs::MetadataExt as _;

    Some(ObjectIncarnation {
        seconds: metadata.st_birthtime(),
        nanoseconds: u32::try_from(metadata.st_birthtime_nsec()).ok()?,
    })
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn object_incarnation(path: &Path, _metadata: &fs::Metadata) -> Option<ObjectIncarnation> {
    use core::ffi::{c_char, c_int, c_uint, c_void};

    // Linux keeps the first 96 bytes of `struct statx` ABI-stable. Birth time starts at byte 80
    // as `i64 seconds, u32 nanoseconds`; a 256-byte u64-aligned buffer leaves room for the full
    // current structure without binding this crate to a general-purpose raw-C dependency.
    unsafe extern "C" {
        fn statx(
            directory: c_int,
            path: *const c_char,
            flags: c_int,
            mask: c_uint,
            result: *mut c_void,
        ) -> c_int;
    }
    const AT_FDCWD: c_int = -100;
    const AT_SYMLINK_NOFOLLOW: c_int = 0x100;
    const STATX_BTIME: c_uint = 0x0000_0800;

    let path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut storage = [0u64; 32];
    // SAFETY: `storage` is aligned, writable, and exactly 256 bytes, the kernel ABI size for
    // `struct statx`; `path` is a live NUL-terminated byte string for this call.
    let status = unsafe {
        statx(
            AT_FDCWD,
            path.as_ptr(),
            AT_SYMLINK_NOFOLLOW,
            STATX_BTIME,
            storage.as_mut_ptr().cast(),
        )
    };
    if status != 0 {
        return None;
    }
    let first = storage[0].to_ne_bytes();
    let mask = u32::from_ne_bytes(first[0..4].try_into().ok()?);
    if mask & STATX_BTIME == 0 {
        return None;
    }
    let seconds = i64::from_ne_bytes(storage[10].to_ne_bytes());
    let nanos = storage[11].to_ne_bytes();
    let nanoseconds = u32::from_ne_bytes(nanos[0..4].try_into().ok()?);
    (nanoseconds < 1_000_000_000).then_some(ObjectIncarnation {
        seconds,
        nanoseconds,
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn object_incarnation(_path: &Path, _metadata: &fs::Metadata) -> Option<ObjectIncarnation> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(object: ObjectId, incarnation: ObjectIncarnation) -> SnapshotEntry {
        SnapshotEntry {
            object,
            incarnation: Some(incarnation),
            kind: ObjectKind::File,
            metadata: PortableMetadata::new(false),
            bytes: 4,
            digest: 7,
            verified_digest: Some(Blake3::digest_bytes(b"same")),
        }
    }

    #[test]
    fn a_recycled_inode_cannot_impersonate_a_move_even_with_identical_bytes() {
        let root = ObjectId::from_bytes([1; 16]);
        let object = ObjectId::from_bytes([2; 16]);
        let root_incarnation = ObjectIncarnation {
            seconds: 1,
            nanoseconds: 1,
        };
        let before = Snapshot {
            root: Some(root),
            root_incarnation: Some(root_incarnation),
            entries: BTreeMap::from([(
                "removed.txt".to_owned(),
                entry(
                    object,
                    ObjectIncarnation {
                        seconds: 2,
                        nanoseconds: 2,
                    },
                ),
            )]),
        };
        let after = Snapshot {
            root: Some(root),
            root_incarnation: Some(root_incarnation),
            entries: BTreeMap::from([(
                "appeared.txt".to_owned(),
                entry(
                    object,
                    ObjectIncarnation {
                        seconds: 3,
                        nanoseconds: 3,
                    },
                ),
            )]),
        };

        let changes = reconcile(&before, &after);
        assert!(matches!(
            changes[0].change(),
            WatchedChange::Appeared { path, .. } if path == "appeared.txt"
        ));
        assert!(matches!(
            changes[1].change(),
            WatchedChange::Disappeared { path, .. } if path == "removed.txt"
        ));
        assert!(changes
            .iter()
            .all(|change| !matches!(change.change(), WatchedChange::MovedInferred { .. })));
    }
}
