//! The change under review, as an immutable and canonically ordered value.
//!
//! Selection reads this and nothing else about the change. That is the whole reason the type
//! exists: a selector that reached for the working tree would produce a different plan on two
//! machines holding the same change, and "selection is deterministic for a given change set" would
//! be untestable. Everything a validator is chosen by — the paths, the operation kinds, the
//! volume, whether conflicts are present, whether an input is stale — is a field here.
//!
//! Two invariants make the determinism real rather than incidental:
//!
//! * **Paths are sorted and a duplicate is rejected.** A caller that collected changes by walking a
//!   hash map hands them over in an order that varies between processes; [`ReviewChange`] imposes
//!   its own order. A repeated path is an error and not a last-wins overwrite, because last-wins
//!   depends on arrival order and would put the nondeterminism back.
//! * **A path is validated at the boundary.** An absolute path, a `..` component or an embedded NUL
//!   is refused on the way in, so no later stage has to decide what one means.

use std::collections::BTreeSet;

use mesh_types::{Absorb, Digest32, DigestHasher, DigestWriter};

/// The longest path this crate will carry, in bytes.
///
/// Not a filesystem limit — a bound on the memory one change description can force this crate to
/// hold. Chosen well above any real repository path.
pub const MAX_PATH_BYTES: usize = 1024;

/// How one path changed.
///
/// A rename is recorded against its destination path: the source disappearing is a
/// [`PathEdit::Removed`] entry of its own, so a rename is two entries and both are visible to a
/// selector that keys on either side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PathEdit {
    /// The path did not exist before this change.
    Added,
    /// The path existed and its content differs.
    Modified,
    /// The path existed and does not now.
    Removed,
    /// The path is the destination of a rename.
    Renamed,
}

impl PathEdit {
    /// Every edit kind. Exhaustive by construction: the match below stops compiling if a variant is
    /// added without being listed here.
    pub const ALL: [Self; 4] = [Self::Added, Self::Modified, Self::Removed, Self::Renamed];

    /// The edit's stable wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Modified => "modified",
            Self::Removed => "removed",
            Self::Renamed => "renamed",
        }
    }
}

/// The coarse operation axis a validator can be selected by.
///
/// Deliberately coarser than `mesh-operations`' operation vocabulary. A selector wants to know
/// "did anything get deleted" and not which operation identifier did it, and keeping the axis
/// local means a new operation variant in the core does not silently change which validators run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChangeOperation {
    /// Something came into existence.
    CreateObject,
    /// Existing content was edited.
    EditContent,
    /// Something moved or was renamed.
    MoveObject,
    /// Something was deleted.
    DeleteObject,
    /// Metadata changed without the content changing.
    EditMetadata,
}

impl ChangeOperation {
    /// Every operation kind.
    pub const ALL: [Self; 5] = [
        Self::CreateObject,
        Self::EditContent,
        Self::MoveObject,
        Self::DeleteObject,
        Self::EditMetadata,
    ];

    /// The operation's stable wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CreateObject => "create-object",
            Self::EditContent => "edit-content",
            Self::MoveObject => "move-object",
            Self::DeleteObject => "delete-object",
            Self::EditMetadata => "edit-metadata",
        }
    }
}

/// One path, how it changed, and how many bytes it now holds.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChangedPath {
    path: String,
    edit: PathEdit,
    bytes: u64,
}

impl ChangedPath {
    /// One changed path.
    ///
    /// # Errors
    ///
    /// [`ChangeError`] when `path` is empty, longer than [`MAX_PATH_BYTES`], absolute, holds a
    /// `..` or `.` component, or holds a NUL or a backslash.
    pub fn new(path: &str, edit: PathEdit, bytes: u64) -> Result<Self, ChangeError> {
        validate_path(path)?;
        Ok(Self {
            path: path.to_owned(),
            edit,
            bytes,
        })
    }

    /// The path, relative to the workspace root, with `/` separators.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The final path component, which is what tooling detection keys on.
    #[must_use]
    pub fn file_name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    /// How it changed.
    #[must_use]
    pub const fn edit(&self) -> PathEdit {
        self.edit
    }

    /// How many bytes it holds after the change. Zero for a removal.
    #[must_use]
    pub const fn bytes(&self) -> u64 {
        self.bytes
    }
}

impl Absorb for ChangedPath {
    fn absorb<H: DigestHasher>(&self, writer: &mut DigestWriter<H>) {
        writer.text(&self.path);
        writer.text(self.edit.as_str());
        writer.u64(self.bytes);
    }
}

/// How much changed, for a selector that keys on size rather than on shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChangeVolume {
    paths: u32,
    bytes: u64,
}

impl ChangeVolume {
    /// How many paths changed.
    #[must_use]
    pub const fn paths(self) -> u32 {
        self.paths
    }

    /// How many bytes those paths hold after the change.
    #[must_use]
    pub const fn bytes(self) -> u64 {
        self.bytes
    }
}

/// Everything selection is allowed to know about a change.
///
/// Built by consuming `with_*` calls that each return a new value; there is no method that mutates
/// one in place, so a plan computed from a change cannot be invalidated by a later edit to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewChange {
    snapshot: Digest32,
    paths: Vec<ChangedPath>,
    operations: BTreeSet<ChangeOperation>,
    conflicts: u32,
    stale_inputs: u32,
}

impl ReviewChange {
    /// An empty change against the immutable review snapshot named by `snapshot`.
    #[must_use]
    pub fn against(snapshot: Digest32) -> Self {
        Self {
            snapshot,
            paths: Vec::new(),
            operations: BTreeSet::new(),
            conflicts: 0,
            stale_inputs: 0,
        }
    }

    /// The same change with one more path, kept in sorted order.
    ///
    /// # Errors
    ///
    /// [`ChangeError::DuplicatePath`] when the path is already present. Rejected rather than
    /// overwritten: an overwrite would make the result depend on arrival order.
    pub fn with_path(mut self, changed: ChangedPath) -> Result<Self, ChangeError> {
        match self
            .paths
            .binary_search_by(|existing| existing.path().cmp(changed.path()))
        {
            Ok(_) => Err(ChangeError::DuplicatePath(changed.path().to_owned())),
            Err(at) => {
                self.paths.insert(at, changed);
                Ok(self)
            }
        }
    }

    /// The same change with one more operation kind recorded.
    #[must_use]
    pub fn with_operation(mut self, operation: ChangeOperation) -> Self {
        self.operations.insert(operation);
        self
    }

    /// The same change with the conflict count set.
    #[must_use]
    pub const fn with_conflicts(mut self, conflicts: u32) -> Self {
        self.conflicts = conflicts;
        self
    }

    /// The same change with the stale-input count set.
    #[must_use]
    pub const fn with_stale_inputs(mut self, stale_inputs: u32) -> Self {
        self.stale_inputs = stale_inputs;
        self
    }

    /// The immutable review snapshot this change is described against.
    #[must_use]
    pub const fn snapshot(&self) -> Digest32 {
        self.snapshot
    }

    /// The changed paths, in path order.
    #[must_use]
    pub fn paths(&self) -> &[ChangedPath] {
        &self.paths
    }

    /// The operation kinds present, in enumeration order.
    pub fn operations(&self) -> impl Iterator<Item = ChangeOperation> + '_ {
        self.operations.iter().copied()
    }

    /// Whether the change carries `operation`.
    #[must_use]
    pub fn carries(&self, operation: ChangeOperation) -> bool {
        self.operations.contains(&operation)
    }

    /// How many conflicts the change is carrying into review.
    #[must_use]
    pub const fn conflicts(&self) -> u32 {
        self.conflicts
    }

    /// How many outputs are stale with respect to their inputs.
    #[must_use]
    pub const fn stale_inputs(&self) -> u32 {
        self.stale_inputs
    }

    /// How much changed.
    #[must_use]
    pub fn volume(&self) -> ChangeVolume {
        ChangeVolume {
            paths: u32::try_from(self.paths.len()).unwrap_or(u32::MAX),
            bytes: self.paths.iter().map(ChangedPath::bytes).sum(),
        }
    }

    /// Whether nothing changed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty() && self.operations.is_empty()
    }
}

impl Absorb for ReviewChange {
    fn absorb<H: DigestHasher>(&self, writer: &mut DigestWriter<H>) {
        writer.digest(&self.snapshot);
        writer.sequence(&self.paths, |writer, path| path.absorb(writer));
        let operations: Vec<ChangeOperation> = self.operations.iter().copied().collect();
        writer.sequence(&operations, |writer, operation| {
            writer.text(operation.as_str());
        });
        writer.u64(u64::from(self.conflicts));
        writer.u64(u64::from(self.stale_inputs));
    }
}

/// Why a change description was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChangeError {
    /// The path was empty.
    EmptyPath,
    /// The path was longer than [`MAX_PATH_BYTES`].
    PathTooLong {
        /// How many bytes it held.
        bytes: usize,
    },
    /// The path was absolute, so it names something outside the workspace.
    AbsolutePath(String),
    /// The path held a `.` or `..` component, which no canonical path does.
    TraversalComponent(String),
    /// The path held a byte no path in this workspace holds: a NUL or a backslash.
    IllegalByte(String),
    /// The path was already present in the change.
    DuplicatePath(String),
}

impl core::fmt::Display for ChangeError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyPath => formatter.write_str("a changed path is empty"),
            Self::PathTooLong { bytes } => {
                write!(formatter, "a changed path is {bytes} bytes, over the limit")
            }
            Self::AbsolutePath(path) => write!(formatter, "`{path}` is absolute"),
            Self::TraversalComponent(path) => {
                write!(formatter, "`{path}` holds a `.` or `..` component")
            }
            Self::IllegalByte(path) => write!(formatter, "`{path}` holds a NUL or a backslash"),
            Self::DuplicatePath(path) => write!(formatter, "`{path}` is already in the change"),
        }
    }
}

impl std::error::Error for ChangeError {}

/// The boundary check every path in this crate passes through.
fn validate_path(path: &str) -> Result<(), ChangeError> {
    if path.is_empty() {
        return Err(ChangeError::EmptyPath);
    }
    if path.len() > MAX_PATH_BYTES {
        return Err(ChangeError::PathTooLong { bytes: path.len() });
    }
    if path.starts_with('/') {
        return Err(ChangeError::AbsolutePath(path.to_owned()));
    }
    if path.contains('\0') || path.contains('\\') {
        return Err(ChangeError::IllegalByte(path.to_owned()));
    }
    if path
        .split('/')
        .any(|component| component == "." || component == ".." || component.is_empty())
    {
        return Err(ChangeError::TraversalComponent(path.to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        ChangeError, ChangeOperation, ChangedPath, PathEdit, ReviewChange, MAX_PATH_BYTES,
    };
    use mesh_types::Digest32;

    fn snapshot() -> Digest32 {
        Digest32::from_bytes([3; 32])
    }

    #[test]
    fn a_path_is_checked_at_the_boundary() {
        for bad in ["", "/etc/passwd", "a/../b", "./a", "a//b", "a\\b", "a\0b"] {
            assert!(
                ChangedPath::new(bad, PathEdit::Modified, 0).is_err(),
                "`{bad}` was accepted"
            );
        }
        assert!(
            ChangedPath::new("crates/mesh-validator/src/lib.rs", PathEdit::Modified, 9).is_ok()
        );
    }

    #[test]
    fn an_over_long_path_is_refused() {
        let long = "a".repeat(MAX_PATH_BYTES + 1);
        assert_eq!(
            ChangedPath::new(&long, PathEdit::Added, 0),
            Err(ChangeError::PathTooLong {
                bytes: MAX_PATH_BYTES + 1
            })
        );
    }

    #[test]
    fn paths_are_held_in_path_order_however_they_arrive() {
        let forwards = ReviewChange::against(snapshot())
            .with_path(ChangedPath::new("a.rs", PathEdit::Added, 1).expect("a legal path"))
            .expect("no duplicate")
            .with_path(ChangedPath::new("b.rs", PathEdit::Added, 2).expect("a legal path"))
            .expect("no duplicate");
        let backwards = ReviewChange::against(snapshot())
            .with_path(ChangedPath::new("b.rs", PathEdit::Added, 2).expect("a legal path"))
            .expect("no duplicate")
            .with_path(ChangedPath::new("a.rs", PathEdit::Added, 1).expect("a legal path"))
            .expect("no duplicate");
        assert_eq!(forwards, backwards);
        assert_eq!(forwards.paths()[0].path(), "a.rs");
    }

    #[test]
    fn a_repeated_path_is_an_error_not_an_overwrite() {
        let change = ReviewChange::against(snapshot())
            .with_path(ChangedPath::new("a.rs", PathEdit::Added, 1).expect("a legal path"))
            .expect("no duplicate");
        assert_eq!(
            change.with_path(ChangedPath::new("a.rs", PathEdit::Removed, 0).expect("legal")),
            Err(ChangeError::DuplicatePath("a.rs".to_owned()))
        );
    }

    #[test]
    fn volume_counts_paths_and_bytes() {
        let change = ReviewChange::against(snapshot())
            .with_path(ChangedPath::new("a.rs", PathEdit::Added, 10).expect("legal"))
            .expect("no duplicate")
            .with_path(ChangedPath::new("b.rs", PathEdit::Modified, 32).expect("legal"))
            .expect("no duplicate");
        assert_eq!(change.volume().paths(), 2);
        assert_eq!(change.volume().bytes(), 42);
    }

    #[test]
    fn the_file_name_is_the_last_component() {
        let changed = ChangedPath::new("a/b/Cargo.toml", PathEdit::Modified, 1).expect("legal");
        assert_eq!(changed.file_name(), "Cargo.toml");
        let root = ChangedPath::new("Cargo.toml", PathEdit::Modified, 1).expect("legal");
        assert_eq!(root.file_name(), "Cargo.toml");
    }

    #[test]
    fn operations_are_a_set_and_recording_one_twice_changes_nothing() {
        let once = ReviewChange::against(snapshot()).with_operation(ChangeOperation::EditContent);
        let twice = once
            .clone()
            .with_operation(ChangeOperation::EditContent)
            .with_operation(ChangeOperation::EditContent);
        assert_eq!(once, twice);
        assert!(twice.carries(ChangeOperation::EditContent));
        assert!(!twice.carries(ChangeOperation::DeleteObject));
    }

    #[test]
    fn the_wire_names_are_distinct() {
        let edits: Vec<&str> = PathEdit::ALL.iter().map(|edit| edit.as_str()).collect();
        assert_eq!(edits.len(), 4);
        for (at, name) in edits.iter().enumerate() {
            assert!(!edits[at + 1..].contains(name), "`{name}` is repeated");
        }
        let operations: Vec<&str> = ChangeOperation::ALL
            .iter()
            .map(|operation| operation.as_str())
            .collect();
        for (at, name) in operations.iter().enumerate() {
            assert!(!operations[at + 1..].contains(name), "`{name}` is repeated");
        }
    }

    #[test]
    fn an_empty_change_reports_itself_empty() {
        assert!(ReviewChange::against(snapshot()).is_empty());
        assert!(!ReviewChange::against(snapshot())
            .with_operation(ChangeOperation::EditContent)
            .is_empty());
    }
}
