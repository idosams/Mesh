//! Pure planning for restoring one file to an immutable earlier version.
//!
//! The planner never edits a [`WorkspaceState`]. It returns ordinary operations
//! for the caller to seal into a new ChangeSet and append through the existing
//! durable checkpoint path. The old version and every intervening version stay
//! in the causal set and content-addressed store.

use core::fmt;

use crate::{
    apply_operation, ChangeSetId, ObjectId, ObjectKind, Operation, Rejection, VersionId,
    WorkspaceState,
};

const PLAN_CHECK_CHANGESET: ChangeSetId = ChangeSetId::from_bytes([0xff; 32]);

/// A checked sequence of ordinary operations that restores one file version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileRestorePlan {
    object: ObjectId,
    from_version: VersionId,
    to_version: VersionId,
    operations: Vec<Operation>,
}

impl FileRestorePlan {
    /// File whose visible version changes.
    #[must_use]
    pub const fn object(&self) -> ObjectId {
        self.object
    }

    /// Version visible before this plan.
    #[must_use]
    pub const fn from_version(&self) -> VersionId {
        self.from_version
    }

    /// Immutable earlier version this plan makes visible.
    #[must_use]
    pub const fn to_version(&self) -> VersionId {
        self.to_version
    }

    /// Operations to seal, in their required order, into one new ChangeSet.
    #[must_use]
    pub fn operations(&self) -> &[Operation] {
        &self.operations
    }

    /// Plan the inverse transition after this plan has materialized.
    ///
    /// # Errors
    ///
    /// Returns [`RestoreRefusal`] unless `restored` truthfully holds this
    /// plan's target as the file's current version.
    pub fn undo(&self, restored: &WorkspaceState) -> Result<Self, RestoreRefusal> {
        let current = restored
            .object(self.object)
            .and_then(|record| record.current_version());
        if current != Some(self.to_version) {
            return Err(RestoreRefusal::UndoStateMismatch {
                expected: self.to_version,
                found: current,
            });
        }
        plan_file_restore(restored, self.object, self.from_version)
    }
}

/// Refusal to construct an exact append-only file restore.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RestoreRefusal {
    /// The object is not in the materialized causal set.
    UnknownObject {
        /// Requested stable object identity.
        object: ObjectId,
    },
    /// Restore currently supports files, not directory topology snapshots.
    NotAFile {
        /// Requested stable object identity.
        object: ObjectId,
    },
    /// The source file is deleted, so this slice cannot promise an exact undo to its prior state.
    SourceDeleted {
        /// Deleted stable object identity.
        object: ObjectId,
    },
    /// The source file has no current immutable version to return to on undo.
    SourceHasNoVersion {
        /// Stable object identity without a current version.
        object: ObjectId,
    },
    /// The requested content identifier is absent from retained history.
    UnknownTargetVersion {
        /// Requested immutable version identity.
        version: VersionId,
    },
    /// The requested version belongs to another stable object.
    TargetBelongsToAnotherObject {
        /// Requested immutable version identity.
        version: VersionId,
        /// Object the caller asked to restore.
        expected: ObjectId,
        /// Object that actually owns the requested version.
        found: ObjectId,
    },
    /// Restoring the already-visible version would create misleading no-op work.
    AlreadyAtVersion {
        /// Already-visible immutable version identity.
        version: VersionId,
    },
    /// The containing directory is deleted, so unlink/relink cannot apply exactly.
    BindingDirectoryDeleted {
        /// Deleted containing directory identity.
        directory: ObjectId,
    },
    /// One staged ordinary operation was refused by the real materializer.
    OperationRejected {
        /// Zero-based index in the generated operation sequence.
        index: usize,
        /// Materializer refusal returned while staging the operation.
        rejection: Rejection,
    },
    /// The staged operations applied but did not produce the exact requested file/binding version.
    ExactnessCheckFailed,
    /// `undo` was asked to invert a state other than this plan's result.
    UndoStateMismatch {
        /// Version that this plan would have made current.
        expected: VersionId,
        /// Version actually current when undo was requested.
        found: Option<VersionId>,
    },
}

impl fmt::Display for RestoreRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownObject { object } => write!(formatter, "restore refused: {object} is unknown"),
            Self::NotAFile { object } => write!(formatter, "restore refused: {object} is not a file"),
            Self::SourceDeleted { object } => write!(
                formatter,
                "restore refused: {object} is deleted, so this file-version restore could not be undone exactly"
            ),
            Self::SourceHasNoVersion { object } => write!(
                formatter,
                "restore refused: {object} has no current version to preserve for undo"
            ),
            Self::UnknownTargetVersion { version } => write!(
                formatter,
                "restore refused: version {version} is not present in retained history"
            ),
            Self::TargetBelongsToAnotherObject {
                version,
                expected,
                found,
            } => write!(
                formatter,
                "restore refused: version {version} belongs to {found}, not {expected}"
            ),
            Self::AlreadyAtVersion { version } => {
                write!(formatter, "restore refused: version {version} is already visible")
            }
            Self::BindingDirectoryDeleted { directory } => write!(
                formatter,
                "restore refused: containing directory {directory} is deleted"
            ),
            Self::OperationRejected { index, rejection } => write!(
                formatter,
                "restore refused: staged operation {index} would not apply: {rejection}"
            ),
            Self::ExactnessCheckFailed => formatter.write_str(
                "restore refused: staged operations did not reproduce the exact requested file version",
            ),
            Self::UndoStateMismatch { expected, found } => write!(
                formatter,
                "restore undo refused: expected current version {expected}, found {found:?}"
            ),
        }
    }
}

impl std::error::Error for RestoreRefusal {}

/// Plan an exact file-version restore as new, ordinary work.
///
/// The current state is never mutated. When the file is named, the plan unlinks
/// it first and relinks the same stable object at the requested version after
/// the delete/restore pair. This keeps the directory entry and object record in
/// agreement. The target `VersionId` continues to name its original immutable
/// manifest; no content is copied or reconstructed.
///
/// # Errors
///
/// Returns [`RestoreRefusal`] when the source cannot be undone, the target is
/// absent or belongs to another object, or the real materializer rejects the
/// staged sequence.
pub fn plan_file_restore(
    current: &WorkspaceState,
    object: ObjectId,
    target: VersionId,
) -> Result<FileRestorePlan, RestoreRefusal> {
    let record = current
        .object(object)
        .ok_or(RestoreRefusal::UnknownObject { object })?;
    if record.kind() != ObjectKind::File {
        return Err(RestoreRefusal::NotAFile { object });
    }
    if record.is_deleted() {
        return Err(RestoreRefusal::SourceDeleted { object });
    }
    let from_version = record
        .current_version()
        .ok_or(RestoreRefusal::SourceHasNoVersion { object })?;
    let target_record = current
        .file_version(target)
        .ok_or(RestoreRefusal::UnknownTargetVersion { version: target })?;
    if target_record.object_id() != object {
        return Err(RestoreRefusal::TargetBelongsToAnotherObject {
            version: target,
            expected: object,
            found: target_record.object_id(),
        });
    }
    if target == from_version {
        return Err(RestoreRefusal::AlreadyAtVersion { version: target });
    }

    let binding = current.binding_of(object);
    if let Some((directory, _)) = &binding {
        if current
            .object(*directory)
            .is_some_and(|parent| parent.is_deleted())
        {
            return Err(RestoreRefusal::BindingDirectoryDeleted {
                directory: *directory,
            });
        }
    }

    let mut operations = Vec::with_capacity(if binding.is_some() { 4 } else { 2 });
    if let Some((directory, name)) = &binding {
        operations.push(Operation::UnlinkDirectoryEntry {
            directory_id: *directory,
            name: name.clone(),
            object_id: object,
        });
    }
    operations.push(Operation::DeleteObject { object_id: object });
    operations.push(Operation::RestoreObject {
        object_id: object,
        restored_version_id: target,
    });
    if let Some((directory, name)) = &binding {
        operations.push(Operation::LinkDirectoryEntry {
            directory_id: *directory,
            name: name.clone(),
            object_id: object,
            version_id: target,
        });
    }

    let mut staged = current.clone();
    for (index, operation) in operations.iter().enumerate() {
        apply_operation(&mut staged, PLAN_CHECK_CHANGESET, operation)
            .map_err(|rejection| RestoreRefusal::OperationRejected { index, rejection })?;
    }
    if !is_exact(&staged, object, target, binding.as_ref()) {
        return Err(RestoreRefusal::ExactnessCheckFailed);
    }

    Ok(FileRestorePlan {
        object,
        from_version,
        to_version: target,
        operations,
    })
}

fn is_exact(
    staged: &WorkspaceState,
    object: ObjectId,
    target: VersionId,
    binding: Option<&(ObjectId, crate::NormalizedName)>,
) -> bool {
    let object_exact = staged
        .object(object)
        .is_some_and(|record| !record.is_deleted() && record.current_version() == Some(target));
    let binding_exact = match binding {
        Some((directory, name)) => staged
            .directory(*directory)
            .and_then(|held| held.entry(name))
            .is_some_and(|entry| entry.object_id() == object && entry.version_id() == target),
        None => staged.binding_of(object).is_none(),
    };
    object_exact && binding_exact
}
