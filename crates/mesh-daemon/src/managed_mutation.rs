//! Crash reconciliation for native managed-folder lifecycle operations.
//!
//! The operating-system rename/create/delete must happen before its immutable ChangeSet append:
//! doing the journal first would let durable history name a path that was never changed on disk.
//! A single create-only, fsynced marker bridges that interval. On restart the immutable journal is
//! the authority: absence rolls the exact identified OS change back; presence finishes it. Any
//! malformed marker, replacement identity, or ambiguous shape is preserved and reported rather
//! than guessed or deleted.

use core::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read as _, Write as _};
use std::path::{Component, Path, PathBuf};

use mesh_cas::DurableFs as _;
use mesh_store::RecordDigest;
use mesh_types::{Blake3, ContentDigest as _};

use crate::managed_file::{
    atomic_rename_noreplace_in_exact_parents, create_managed_directory_in_exact_parent,
    create_text_file_in_exact_parent, managed_directory_identity, ManagedDirectoryIdentity,
};
use crate::root_authority::PinnedRootFs;

pub(crate) const MARKER_NAME: &str = ".mesh-managed-mutation";
const NEXT_MARKER_NAME: &str = ".mesh-managed-mutation.next";
const MAGIC: &[u8] = b"mesh-managed-mutation/1\0";
const MAX_MARKER_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IntentKind {
    CreateFile = 1,
    CreateDirectory = 2,
    MoveFile = 3,
    MoveDirectory = 4,
    DeleteFile = 5,
    DeleteDirectory = 6,
}

impl IntentKind {
    const fn from_byte(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::CreateFile),
            2 => Some(Self::CreateDirectory),
            3 => Some(Self::MoveFile),
            4 => Some(Self::MoveDirectory),
            5 => Some(Self::DeleteFile),
            6 => Some(Self::DeleteDirectory),
            _ => None,
        }
    }

    const fn is_directory(self) -> bool {
        matches!(
            self,
            Self::CreateDirectory | Self::MoveDirectory | Self::DeleteDirectory
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileIdentity {
    device: u64,
    inode: u64,
}

/// Exact operating-system source selected before an authenticated move or delete is signed.
///
/// A signer is caller code and may take arbitrarily long. The path can name a different inode by
/// the time it returns, while the signed operation still names the durable object selected before
/// the callback. Carry both identity and file content across that boundary so a replacement cannot
/// inherit the earlier operation's authority.
#[derive(Debug)]
pub(crate) struct ManagedMutationSource {
    identity: FileIdentity,
    content_digest: [u8; 32],
    parent: ManagedDirectoryIdentity,
    // Keeping the inspected object open until the operation applies prevents Unix from recycling
    // its inode for a same-byte replacement during an arbitrarily long signing callback.
    handle: File,
}

impl PartialEq for ManagedMutationSource {
    fn eq(&self, other: &Self) -> bool {
        self.identity == other.identity
            && self.content_digest == other.content_digest
            && self.parent == other.parent
    }
}

impl Eq for ManagedMutationSource {}

impl ManagedMutationSource {
    pub(crate) fn capture(
        root: &Path,
        relative: &str,
        is_directory: bool,
    ) -> Result<Self, ManagedMutationRecoveryError> {
        let path = confined_path(root, relative)?;
        capture_source(&path, is_directory)
    }
}

/// One exact pending OS mutation and the authenticated ChangeSet that decides its outcome.
#[derive(Debug)]
pub(crate) struct ManagedMutationIntent {
    root: PathBuf,
    filesystem: Option<PinnedRootFs>,
    kind: IntentKind,
    changeset: RecordDigest,
    from: String,
    to: String,
    identity: FileIdentity,
    content_digest: [u8; 32],
    source_parent: Option<ManagedDirectoryIdentity>,
    target_parent: Option<ManagedDirectoryIdentity>,
    source_handle: Option<File>,
    bytes: Vec<u8>,
}

/// What restart did with a valid interrupted marker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ManagedMutationRecovery {
    /// Journal absence made the pre-mutation OS shape authoritative.
    RolledBack,
    /// Journal presence made the post-mutation OS shape authoritative.
    Completed,
}

impl ManagedMutationRecovery {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::RolledBack => "managed-mutation-rolled-back",
            Self::Completed => "managed-mutation-completed",
        }
    }

    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::RolledBack => {
                "Mesh restored an interrupted local file change before opening this workspace."
            }
            Self::Completed => {
                "Mesh finished an interrupted saved file change before opening this workspace."
            }
        }
    }
}

/// A marker or OS shape could not be reconciled without guessing.
#[derive(Debug)]
pub(crate) enum ManagedMutationRecoveryError {
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    AppliedButNotDurable(Box<Self>),
    UnrecognizedMarker,
    AmbiguousShape,
}

impl ManagedMutationRecoveryError {
    fn io(operation: &'static str, path: &Path, source: io::Error) -> Self {
        Self::Io {
            operation,
            path: path.to_path_buf(),
            source,
        }
    }

    fn after_mutation(error: Self) -> Self {
        Self::AppliedButNotDurable(Box::new(error))
    }

    /// Whether the filesystem mutation crossed its linearization point before this error.
    ///
    /// Callers must retain the fsynced intent marker in this case. Restart reconciliation can
    /// safely roll the exact identified move back when the journal is absent; deleting the marker
    /// would strand an unjournaled filesystem change with no remaining recovery authority.
    pub(crate) const fn mutation_may_have_applied(&self) -> bool {
        matches!(self, Self::AppliedButNotDurable(_))
    }

    pub(crate) const fn code(&self) -> &'static str {
        "managed-mutation-recovery-needed"
    }

    pub(crate) const fn message(&self) -> &'static str {
        "An interrupted local file change needs attention before this workspace can be changed again."
    }
}

impl fmt::Display for ManagedMutationRecoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                operation,
                path,
                source,
            } => write!(formatter, "{operation} {} failed: {source}", path.display()),
            Self::AppliedButNotDurable(source) => write!(
                formatter,
                "the filesystem mutation may have applied before durability failed: {source}"
            ),
            Self::UnrecognizedMarker => {
                formatter.write_str("the managed-mutation marker is not canonical")
            }
            Self::AmbiguousShape => formatter.write_str(
                "the interrupted managed mutation no longer matches its recorded OS identity",
            ),
        }
    }
}

impl std::error::Error for ManagedMutationRecoveryError {}

impl ManagedMutationIntent {
    #[cfg(test)]
    pub(crate) fn begin_create_file(
        root: &Path,
        to: &str,
        changeset: RecordDigest,
        contents: &[u8],
    ) -> Result<Self, ManagedMutationRecoveryError> {
        let target = confined_path(root, to)?;
        let target_parent = target
            .parent()
            .ok_or(ManagedMutationRecoveryError::AmbiguousShape)?;
        let target_parent = managed_directory_identity(target_parent).map_err(|error| {
            ManagedMutationRecoveryError::io("capture parent identity", target_parent, error)
        })?;
        Self::begin_create_file_guarded(root, to, changeset, contents, target_parent, None)
    }

    pub(crate) fn begin_create_file_guarded(
        root: &Path,
        to: &str,
        changeset: RecordDigest,
        contents: &[u8],
        expected_parent: ManagedDirectoryIdentity,
        filesystem: Option<PinnedRootFs>,
    ) -> Result<Self, ManagedMutationRecoveryError> {
        let target = confined_path(root, to)?;
        let target_parent_path = target
            .parent()
            .ok_or(ManagedMutationRecoveryError::AmbiguousShape)?;
        let target_parent = managed_directory_identity(target_parent_path).map_err(|error| {
            ManagedMutationRecoveryError::io("capture parent identity", target_parent_path, error)
        })?;
        if target_parent != expected_parent {
            return Err(ManagedMutationRecoveryError::AmbiguousShape);
        }
        let mut intent = Self::create(
            root,
            IntentKind::CreateFile,
            changeset,
            "",
            to,
            FileIdentity {
                device: 0,
                inode: 0,
            },
            *Blake3::digest_bytes(contents).as_bytes(),
            filesystem,
        )?;
        intent.target_parent = Some(expected_parent);
        Ok(intent)
    }

    #[cfg(test)]
    pub(crate) fn begin_create_directory(
        root: &Path,
        to: &str,
        changeset: RecordDigest,
    ) -> Result<Self, ManagedMutationRecoveryError> {
        let target = confined_path(root, to)?;
        let target_parent = target
            .parent()
            .ok_or(ManagedMutationRecoveryError::AmbiguousShape)?;
        let target_parent = managed_directory_identity(target_parent).map_err(|error| {
            ManagedMutationRecoveryError::io("capture parent identity", target_parent, error)
        })?;
        Self::begin_create_directory_guarded(root, to, changeset, target_parent, None)
    }

    pub(crate) fn begin_create_directory_guarded(
        root: &Path,
        to: &str,
        changeset: RecordDigest,
        expected_parent: ManagedDirectoryIdentity,
        filesystem: Option<PinnedRootFs>,
    ) -> Result<Self, ManagedMutationRecoveryError> {
        let target = confined_path(root, to)?;
        let target_parent_path = target
            .parent()
            .ok_or(ManagedMutationRecoveryError::AmbiguousShape)?;
        let target_parent = managed_directory_identity(target_parent_path).map_err(|error| {
            ManagedMutationRecoveryError::io("capture parent identity", target_parent_path, error)
        })?;
        if target_parent != expected_parent {
            return Err(ManagedMutationRecoveryError::AmbiguousShape);
        }
        let mut intent = Self::create(
            root,
            IntentKind::CreateDirectory,
            changeset,
            "",
            to,
            FileIdentity {
                device: 0,
                inode: 0,
            },
            [0; 32],
            filesystem,
        )?;
        intent.target_parent = Some(expected_parent);
        Ok(intent)
    }

    pub(crate) fn apply_create_file(
        &self,
        contents: &[u8],
    ) -> Result<PathBuf, ManagedMutationRecoveryError> {
        if self.kind != IntentKind::CreateFile
            || Blake3::digest_bytes(contents).as_bytes() != &self.content_digest
        {
            return Err(ManagedMutationRecoveryError::AmbiguousShape);
        }
        let target = confined_path(&self.root, &self.to)?;
        let parent = self
            .target_parent
            .ok_or(ManagedMutationRecoveryError::AmbiguousShape)?;
        match create_text_file_in_exact_parent(&target, contents, parent) {
            Ok(()) => Ok(target),
            Err(error) => {
                let error = ManagedMutationRecoveryError::io("create", &target, error);
                if exact_regular_file(&target, self.content_digest)? {
                    Err(ManagedMutationRecoveryError::after_mutation(error))
                } else {
                    Err(error)
                }
            }
        }
    }

    pub(crate) fn apply_create_directory(&self) -> Result<PathBuf, ManagedMutationRecoveryError> {
        if self.kind != IntentKind::CreateDirectory {
            return Err(ManagedMutationRecoveryError::AmbiguousShape);
        }
        let target = confined_path(&self.root, &self.to)?;
        let parent = self
            .target_parent
            .ok_or(ManagedMutationRecoveryError::AmbiguousShape)?;
        match create_managed_directory_in_exact_parent(&target, parent) {
            Ok(()) => Ok(target),
            Err(error) => {
                let error = ManagedMutationRecoveryError::io("create directory", &target, error);
                if correct_directory(&target)? {
                    Err(ManagedMutationRecoveryError::after_mutation(error))
                } else {
                    Err(error)
                }
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn begin_move(
        root: &Path,
        from: &str,
        to: &str,
        changeset: RecordDigest,
        is_directory: bool,
    ) -> Result<Self, ManagedMutationRecoveryError> {
        let source = ManagedMutationSource::capture(root, from, is_directory)?;
        let target = confined_path(root, to)?;
        let target_parent_path = target
            .parent()
            .ok_or(ManagedMutationRecoveryError::AmbiguousShape)?;
        let target_parent = managed_directory_identity(target_parent_path).map_err(|error| {
            ManagedMutationRecoveryError::io(
                "capture destination parent identity",
                target_parent_path,
                error,
            )
        })?;
        Self::begin_move_guarded(
            root,
            from,
            to,
            changeset,
            is_directory,
            source,
            target_parent,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn begin_move_guarded(
        root: &Path,
        from: &str,
        to: &str,
        changeset: RecordDigest,
        is_directory: bool,
        expected: ManagedMutationSource,
        expected_target_parent: ManagedDirectoryIdentity,
        filesystem: Option<PinnedRootFs>,
    ) -> Result<Self, ManagedMutationRecoveryError> {
        let from_path = confined_path(root, from)?;
        let to_path = confined_path(root, to)?;
        if capture_source(&from_path, is_directory)? != expected {
            return Err(ManagedMutationRecoveryError::AmbiguousShape);
        }
        let target_parent_path = to_path
            .parent()
            .ok_or(ManagedMutationRecoveryError::AmbiguousShape)?;
        let target_parent = managed_directory_identity(target_parent_path).map_err(|error| {
            ManagedMutationRecoveryError::io(
                "capture destination parent identity",
                target_parent_path,
                error,
            )
        })?;
        if target_parent != expected_target_parent {
            return Err(ManagedMutationRecoveryError::AmbiguousShape);
        }
        let mut intent = Self::create(
            root,
            if is_directory {
                IntentKind::MoveDirectory
            } else {
                IntentKind::MoveFile
            },
            changeset,
            from,
            to,
            expected.identity,
            expected.content_digest,
            filesystem,
        )?;
        intent.source_parent = Some(expected.parent);
        intent.target_parent = Some(expected_target_parent);
        intent.source_handle = Some(expected.handle);
        Ok(intent)
    }

    #[cfg(test)]
    pub(crate) fn begin_delete(
        root: &Path,
        from: &str,
        changeset: RecordDigest,
        is_directory: bool,
    ) -> Result<Self, ManagedMutationRecoveryError> {
        let source = ManagedMutationSource::capture(root, from, is_directory)?;
        Self::begin_delete_guarded(root, from, changeset, is_directory, source, None)
    }

    pub(crate) fn begin_delete_guarded(
        root: &Path,
        from: &str,
        changeset: RecordDigest,
        is_directory: bool,
        expected: ManagedMutationSource,
        filesystem: Option<PinnedRootFs>,
    ) -> Result<Self, ManagedMutationRecoveryError> {
        let from_path = confined_path(root, from)?;
        if capture_source(&from_path, is_directory)? != expected {
            return Err(ManagedMutationRecoveryError::AmbiguousShape);
        }
        let mut intent = Self::create(
            root,
            if is_directory {
                IntentKind::DeleteDirectory
            } else {
                IntentKind::DeleteFile
            },
            changeset,
            from,
            "",
            expected.identity,
            expected.content_digest,
            filesystem,
        )?;
        intent.source_parent = Some(expected.parent);
        intent.target_parent = Some(expected.parent);
        intent.source_handle = Some(expected.handle);
        if !absent(&intent.staged_delete_path()?)? {
            intent.clear()?;
            return Err(ManagedMutationRecoveryError::AmbiguousShape);
        }
        Ok(intent)
    }

    pub(crate) fn apply_move(&self) -> Result<(), ManagedMutationRecoveryError> {
        if !matches!(self.kind, IntentKind::MoveFile | IntentKind::MoveDirectory) {
            return Err(ManagedMutationRecoveryError::AmbiguousShape);
        }
        let from = confined_path(&self.root, &self.from)?;
        let to = confined_path(&self.root, &self.to)?;
        if !move_content_matches(self, &from)? {
            return Err(ManagedMutationRecoveryError::AmbiguousShape);
        }
        self.rename_in_admitted_parents(&from, &to)
    }

    pub(crate) fn moved_entry_matches(&self) -> Result<bool, ManagedMutationRecoveryError> {
        if !matches!(self.kind, IntentKind::MoveFile | IntentKind::MoveDirectory) {
            return Err(ManagedMutationRecoveryError::AmbiguousShape);
        }
        let to = confined_path(&self.root, &self.to)?;
        Ok(same_identity(&to, self.identity, self.kind.is_directory())?
            && move_content_matches(self, &to)?)
    }

    /// Whether a newly created directory is still the exact empty directory this intent made.
    ///
    /// A directory's inode is not its complete save boundary: an ordinary editor may add a child
    /// during the idle interval without replacing the directory itself. That newer child is not in
    /// the authenticated create operation, so it must keep the checkpoint window Working.
    pub(crate) fn created_directory_matches(&self) -> Result<bool, ManagedMutationRecoveryError> {
        if self.kind != IntentKind::CreateDirectory || self.identity.device == 0 {
            return Err(ManagedMutationRecoveryError::AmbiguousShape);
        }
        let target = confined_path(&self.root, &self.to)?;
        if !correct_directory(&target)? || !same_identity(&target, self.identity, true)? {
            return Ok(false);
        }
        let mut entries = fs::read_dir(&target)
            .map_err(|error| ManagedMutationRecoveryError::io("read directory", &target, error))?;
        match entries.next() {
            None => Ok(true),
            Some(Ok(_)) => Ok(false),
            Some(Err(error)) => Err(ManagedMutationRecoveryError::io(
                "read directory entry",
                &target,
                error,
            )),
        }
    }

    /// Whether a newly created file still has the exact identity and bytes bound into this intent.
    pub(crate) fn created_file_matches(&self) -> Result<bool, ManagedMutationRecoveryError> {
        if self.kind != IntentKind::CreateFile || self.identity.device == 0 {
            return Err(ManagedMutationRecoveryError::AmbiguousShape);
        }
        let target = confined_path(&self.root, &self.to)?;
        Ok(exact_regular_file(&target, self.content_digest)?
            && same_identity(&target, self.identity, false)?)
    }

    #[cfg(test)]
    fn apply_move_with_sync<S>(&self, sync: S) -> Result<(), ManagedMutationRecoveryError>
    where
        S: FnOnce(&Path, &Path) -> Result<(), ManagedMutationRecoveryError>,
    {
        if !matches!(self.kind, IntentKind::MoveFile | IntentKind::MoveDirectory) {
            return Err(ManagedMutationRecoveryError::AmbiguousShape);
        }
        let from = confined_path(&self.root, &self.from)?;
        let to = confined_path(&self.root, &self.to)?;
        if !move_content_matches(self, &from)? {
            return Err(ManagedMutationRecoveryError::AmbiguousShape);
        }
        move_exact_with_sync(&from, &to, self.identity, self.kind.is_directory(), sync)
    }

    pub(crate) fn stage_delete(&self) -> Result<PathBuf, ManagedMutationRecoveryError> {
        if !matches!(
            self.kind,
            IntentKind::DeleteFile | IntentKind::DeleteDirectory
        ) {
            return Err(ManagedMutationRecoveryError::AmbiguousShape);
        }
        let from = confined_path(&self.root, &self.from)?;
        let staged = self.staged_delete_path()?;
        if !delete_content_matches(self, &from)? {
            return Err(ManagedMutationRecoveryError::AmbiguousShape);
        }
        self.rename_in_admitted_parents(&from, &staged)?;
        Ok(staged)
    }

    fn rename_in_admitted_parents(
        &self,
        from: &Path,
        to: &Path,
    ) -> Result<(), ManagedMutationRecoveryError> {
        if !same_identity(from, self.identity, self.kind.is_directory())? {
            return Err(ManagedMutationRecoveryError::AmbiguousShape);
        }
        let source_parent = self
            .source_parent
            .ok_or(ManagedMutationRecoveryError::AmbiguousShape)?;
        let target_parent = self
            .target_parent
            .ok_or(ManagedMutationRecoveryError::AmbiguousShape)?;
        match atomic_rename_noreplace_in_exact_parents(from, to, source_parent, target_parent) {
            Ok(()) => {
                if same_identity(to, self.identity, self.kind.is_directory())? {
                    Ok(())
                } else {
                    Err(ManagedMutationRecoveryError::after_mutation(
                        ManagedMutationRecoveryError::AmbiguousShape,
                    ))
                }
            }
            Err(error) => {
                let error =
                    ManagedMutationRecoveryError::io("move without replacement", from, error);
                if same_identity(to, self.identity, self.kind.is_directory())? {
                    Err(ManagedMutationRecoveryError::after_mutation(error))
                } else {
                    Err(error)
                }
            }
        }
    }

    #[cfg(test)]
    fn stage_delete_with_sync<S>(&self, sync: S) -> Result<PathBuf, ManagedMutationRecoveryError>
    where
        S: FnOnce(&Path, &Path) -> Result<(), ManagedMutationRecoveryError>,
    {
        if !matches!(
            self.kind,
            IntentKind::DeleteFile | IntentKind::DeleteDirectory
        ) {
            return Err(ManagedMutationRecoveryError::AmbiguousShape);
        }
        let from = confined_path(&self.root, &self.from)?;
        let staged = self.staged_delete_path()?;
        if !delete_content_matches(self, &from)? {
            return Err(ManagedMutationRecoveryError::AmbiguousShape);
        }
        move_exact_with_sync(
            &from,
            &staged,
            self.identity,
            self.kind.is_directory(),
            sync,
        )?;
        Ok(staged)
    }

    /// Recheck the only directory shape that an empty-directory deletion may commit.
    ///
    /// The caller's signer is arbitrary code and may create a child after the initial preflight.
    /// Rechecking the exact staged inode keeps that newer child out of a deletion journal record.
    pub(crate) fn staged_delete_directory_is_empty(
        &self,
        staged: &Path,
    ) -> Result<bool, ManagedMutationRecoveryError> {
        if self.kind != IntentKind::DeleteDirectory
            || staged != self.staged_delete_path()?
            || !same_identity(staged, self.identity, true)?
        {
            return Err(ManagedMutationRecoveryError::AmbiguousShape);
        }
        let mut entries = fs::read_dir(staged)
            .map_err(|error| ManagedMutationRecoveryError::io("read directory", staged, error))?;
        match entries.next() {
            None => Ok(true),
            Some(Ok(_)) => Ok(false),
            Some(Err(error)) => Err(ManagedMutationRecoveryError::io(
                "read directory entry",
                staged,
                error,
            )),
        }
    }

    /// Restore a staged mutation that has not reached the immutable journal.
    pub(crate) fn roll_back_nondurable(&self) -> Result<(), ManagedMutationRecoveryError> {
        roll_back(self)?;
        self.clear()
    }

    #[allow(clippy::too_many_arguments)]
    fn create(
        root: &Path,
        kind: IntentKind,
        changeset: RecordDigest,
        from: &str,
        to: &str,
        identity: FileIdentity,
        content_digest: [u8; 32],
        filesystem: Option<PinnedRootFs>,
    ) -> Result<Self, ManagedMutationRecoveryError> {
        let mut intent = Self {
            root: root.to_path_buf(),
            filesystem,
            kind,
            changeset,
            from: from.to_owned(),
            to: to.to_owned(),
            identity,
            content_digest,
            source_parent: None,
            target_parent: None,
            source_handle: None,
            bytes: Vec::new(),
        };
        intent.bytes = intent.encode();
        let marker = intent.marker();
        intent
            .write_new_marker(MARKER_NAME, &intent.bytes, true)
            .map_err(|error| Self::map_marker_create(&marker, error))?;
        Ok(intent)
    }

    fn map_marker_create(path: &Path, error: io::Error) -> ManagedMutationRecoveryError {
        if error.kind() == io::ErrorKind::AlreadyExists {
            ManagedMutationRecoveryError::AmbiguousShape
        } else {
            ManagedMutationRecoveryError::io("create", path, error)
        }
    }

    pub(crate) fn staged_delete_path(&self) -> Result<PathBuf, ManagedMutationRecoveryError> {
        if !matches!(
            self.kind,
            IntentKind::DeleteFile | IntentKind::DeleteDirectory
        ) {
            return Err(ManagedMutationRecoveryError::AmbiguousShape);
        }
        let from = Path::new(&self.from);
        let parent = from.parent().unwrap_or_else(|| Path::new(""));
        let suffix = hex_prefix(self.changeset.as_bytes(), 12);
        let relative = parent.join(format!(".mesh-delete-{suffix}"));
        let relative = relative
            .to_str()
            .ok_or(ManagedMutationRecoveryError::UnrecognizedMarker)?;
        confined_internal_path(&self.root, relative)
    }

    pub(crate) fn bind_created(&mut self, path: &Path) -> Result<(), ManagedMutationRecoveryError> {
        if !matches!(
            self.kind,
            IntentKind::CreateFile | IntentKind::CreateDirectory
        ) || self.identity.device != 0
            || self.identity.inode != 0
        {
            return Err(ManagedMutationRecoveryError::AmbiguousShape);
        }
        if self.kind == IntentKind::CreateFile && !exact_regular_file(path, self.content_digest)? {
            return Err(ManagedMutationRecoveryError::AmbiguousShape);
        }
        self.identity = file_identity(path, self.kind.is_directory())?;
        let next_bytes = self.encode();
        let marker = self.marker();
        if self.read_marker(MARKER_NAME)? != self.bytes {
            return Err(ManagedMutationRecoveryError::UnrecognizedMarker);
        }
        self.write_new_marker(NEXT_MARKER_NAME, &next_bytes, false)
            .map_err(|error| {
                ManagedMutationRecoveryError::io("create", &self.root.join(NEXT_MARKER_NAME), error)
            })?;
        if let Some(filesystem) = &self.filesystem {
            filesystem
                .rename(Path::new(NEXT_MARKER_NAME), Path::new(MARKER_NAME))
                .and_then(|()| filesystem.sync_dir(Path::new("")))
                .map_err(|error| ManagedMutationRecoveryError::io("replace", &marker, error))?;
        } else {
            fs::rename(self.root.join(NEXT_MARKER_NAME), &marker)
                .map_err(|error| ManagedMutationRecoveryError::io("replace", &marker, error))?;
            sync_directory(&self.root)?;
        }
        self.bytes = next_bytes;
        Ok(())
    }

    pub(crate) fn clear(&self) -> Result<(), ManagedMutationRecoveryError> {
        let marker = self.marker();
        let current = match self.read_marker(MARKER_NAME) {
            Ok(current) => current,
            Err(ManagedMutationRecoveryError::Io { source, .. })
                if source.kind() == io::ErrorKind::NotFound =>
            {
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        if current != self.bytes {
            return Err(ManagedMutationRecoveryError::UnrecognizedMarker);
        }
        if let Some(filesystem) = &self.filesystem {
            filesystem
                .remove_file(Path::new(MARKER_NAME))
                .and_then(|()| filesystem.sync_dir(Path::new("")))
                .map_err(|error| ManagedMutationRecoveryError::io("remove", &marker, error))
        } else {
            fs::remove_file(&marker)
                .map_err(|error| ManagedMutationRecoveryError::io("remove", &marker, error))?;
            sync_directory(&self.root)
        }
    }

    /// Clear a failed intent only when the filesystem mutation provably did not linearize.
    pub(crate) fn clear_after_failed_apply(
        &self,
        error: &ManagedMutationRecoveryError,
    ) -> Result<(), ManagedMutationRecoveryError> {
        if error.mutation_may_have_applied() {
            Ok(())
        } else {
            self.clear()
        }
    }

    pub(crate) fn complete_durable(&self) -> Result<(), ManagedMutationRecoveryError> {
        complete(self)?;
        self.clear()
    }

    fn marker(&self) -> PathBuf {
        self.root.join(MARKER_NAME)
    }

    fn read_marker(&self, name: &str) -> Result<Vec<u8>, ManagedMutationRecoveryError> {
        let path = self.root.join(name);
        let bytes = if let Some(filesystem) = &self.filesystem {
            filesystem
                .read(Path::new(name))
                .map_err(|error| ManagedMutationRecoveryError::io("read", &path, error))?
        } else {
            fs::read(&path)
                .map_err(|error| ManagedMutationRecoveryError::io("read", &path, error))?
        };
        if bytes.len() > MAX_MARKER_BYTES {
            return Err(ManagedMutationRecoveryError::UnrecognizedMarker);
        }
        Ok(bytes)
    }

    fn write_new_marker(&self, name: &str, bytes: &[u8], sync_root: bool) -> io::Result<()> {
        if let Some(filesystem) = &self.filesystem {
            let path = Path::new(name);
            filesystem.stage(path, bytes)?;
            filesystem.sync_file(path)?;
            if sync_root {
                filesystem.sync_dir(Path::new(""))?;
            }
            Ok(())
        } else {
            let path = self.root.join(name);
            let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            if sync_root {
                File::open(&self.root)?.sync_all()?;
            }
            Ok(())
        }
    }

    fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(MAGIC.len() + self.from.len() + self.to.len() + 128);
        bytes.extend_from_slice(MAGIC);
        bytes.push(self.kind as u8);
        bytes.extend_from_slice(self.changeset.as_bytes());
        bytes.extend_from_slice(&self.identity.device.to_be_bytes());
        bytes.extend_from_slice(&self.identity.inode.to_be_bytes());
        bytes.extend_from_slice(&self.content_digest);
        write_text(&mut bytes, &self.from);
        write_text(&mut bytes, &self.to);
        bytes
    }

    fn decode(root: &Path, bytes: Vec<u8>) -> Result<Self, ManagedMutationRecoveryError> {
        let mut reader = MarkerReader::new(&bytes);
        if reader.take(MAGIC.len())? != MAGIC {
            return Err(ManagedMutationRecoveryError::UnrecognizedMarker);
        }
        let kind = IntentKind::from_byte(reader.byte()?)
            .ok_or(ManagedMutationRecoveryError::UnrecognizedMarker)?;
        let changeset = RecordDigest::from_bytes(reader.array()?);
        let identity = FileIdentity {
            device: reader.u64()?,
            inode: reader.u64()?,
        };
        let content_digest = reader.array()?;
        let from = reader.text()?;
        let to = reader.text()?;
        if !reader.done() {
            return Err(ManagedMutationRecoveryError::UnrecognizedMarker);
        }
        let valid = match kind {
            IntentKind::CreateFile => {
                from.is_empty()
                    && !to.is_empty()
                    && ((identity.device == 0 && identity.inode == 0)
                        || (identity.device != 0 && identity.inode != 0))
                    && content_digest != [0; 32]
            }
            IntentKind::CreateDirectory => {
                from.is_empty()
                    && !to.is_empty()
                    && ((identity.device == 0 && identity.inode == 0)
                        || (identity.device != 0 && identity.inode != 0))
                    && content_digest == [0; 32]
            }
            // Legacy file-move markers used a zero digest; new markers bind exact file content.
            IntentKind::MoveFile => !from.is_empty() && !to.is_empty(),
            IntentKind::MoveDirectory => {
                !from.is_empty() && !to.is_empty() && content_digest == [0; 32]
            }
            IntentKind::DeleteFile => {
                !from.is_empty() && to.is_empty() && content_digest != [0; 32]
            }
            IntentKind::DeleteDirectory => {
                !from.is_empty() && to.is_empty() && content_digest == [0; 32]
            }
        };
        if !valid {
            return Err(ManagedMutationRecoveryError::UnrecognizedMarker);
        }
        let intent = Self {
            root: root.to_path_buf(),
            filesystem: None,
            kind,
            changeset,
            from,
            to,
            identity,
            content_digest,
            source_parent: None,
            target_parent: None,
            source_handle: None,
            bytes,
        };
        if intent.encode() != intent.bytes {
            return Err(ManagedMutationRecoveryError::UnrecognizedMarker);
        }
        Ok(intent)
    }
}

/// Reconcile the one pending marker, if any, against exact immutable journal truth.
pub(crate) fn reconcile_pending_mutation<F>(
    root: &Path,
    has_operation: F,
) -> Result<Option<ManagedMutationRecovery>, ManagedMutationRecoveryError>
where
    F: FnOnce(&RecordDigest) -> bool,
{
    let next_marker = root.join(NEXT_MARKER_NAME);
    match fs::symlink_metadata(&next_marker) {
        Ok(_) => return Err(ManagedMutationRecoveryError::UnrecognizedMarker),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(ManagedMutationRecoveryError::io(
                "metadata",
                &next_marker,
                error,
            ));
        }
    }
    let marker = root.join(MARKER_NAME);
    let bytes = match fs::symlink_metadata(&marker) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            read_marker_bytes(&marker)?
        }
        Ok(_) => return Err(ManagedMutationRecoveryError::UnrecognizedMarker),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(ManagedMutationRecoveryError::io("metadata", &marker, error)),
    };
    let intent = ManagedMutationIntent::decode(root, bytes)?;
    let durable = has_operation(&intent.changeset);
    if durable {
        complete(&intent)?;
        intent.clear()?;
        Ok(Some(ManagedMutationRecovery::Completed))
    } else {
        roll_back(&intent)?;
        intent.clear()?;
        Ok(Some(ManagedMutationRecovery::RolledBack))
    }
}

fn complete(intent: &ManagedMutationIntent) -> Result<(), ManagedMutationRecoveryError> {
    match intent.kind {
        IntentKind::CreateFile => {
            let target = confined_path(&intent.root, &intent.to)?;
            if intent.identity.device != 0
                && exact_regular_file(&target, intent.content_digest)?
                && same_identity(&target, intent.identity, false)?
            {
                Ok(())
            } else {
                Err(ManagedMutationRecoveryError::AmbiguousShape)
            }
        }
        IntentKind::CreateDirectory => {
            let target = confined_path(&intent.root, &intent.to)?;
            if intent.identity.device != 0
                && correct_directory(&target)?
                && same_identity(&target, intent.identity, true)?
            {
                Ok(())
            } else {
                Err(ManagedMutationRecoveryError::AmbiguousShape)
            }
        }
        IntentKind::MoveFile | IntentKind::MoveDirectory => {
            let from = confined_path(&intent.root, &intent.from)?;
            let to = confined_path(&intent.root, &intent.to)?;
            match (
                same_identity(&from, intent.identity, intent.kind.is_directory())?,
                same_identity(&to, intent.identity, intent.kind.is_directory())?,
                absent(&from)?,
                absent(&to)?,
                move_content_matches(intent, &from)?,
                move_content_matches(intent, &to)?,
            ) {
                (false, true, true, false, _, true) => Ok(()),
                (true, false, false, true, true, _) => {
                    move_exact(&from, &to, intent.identity, intent.kind.is_directory())
                }
                _ => Err(ManagedMutationRecoveryError::AmbiguousShape),
            }
        }
        IntentKind::DeleteFile | IntentKind::DeleteDirectory => {
            let from = confined_path(&intent.root, &intent.from)?;
            let staged = intent.staged_delete_path()?;
            let from_absent = absent(&from)?;
            let staged_absent = absent(&staged)?;
            let from_same = same_identity(&from, intent.identity, intent.kind.is_directory())?;
            let staged_same = same_identity(&staged, intent.identity, intent.kind.is_directory())?;
            match (from_same, staged_same, from_absent, staged_absent) {
                (false, false, true, true) => Ok(()),
                (false, true, true, false) if delete_content_matches(intent, &staged)? => {
                    remove_exact(&staged, intent.identity, intent.kind.is_directory())
                }
                (true, false, false, true) if delete_content_matches(intent, &from)? => {
                    move_exact(&from, &staged, intent.identity, intent.kind.is_directory())?;
                    remove_exact(&staged, intent.identity, intent.kind.is_directory())
                }
                _ => Err(ManagedMutationRecoveryError::AmbiguousShape),
            }
        }
    }
}

fn roll_back(intent: &ManagedMutationIntent) -> Result<(), ManagedMutationRecoveryError> {
    match intent.kind {
        IntentKind::CreateFile => {
            let target = confined_path(&intent.root, &intent.to)?;
            if absent(&target)? {
                return Ok(());
            }
            if intent.identity.device == 0
                || !exact_regular_file(&target, intent.content_digest)?
                || !same_identity(&target, intent.identity, false)?
            {
                return Err(ManagedMutationRecoveryError::AmbiguousShape);
            }
            remove_exact(&target, intent.identity, false)
        }
        IntentKind::CreateDirectory => {
            let target = confined_path(&intent.root, &intent.to)?;
            if absent(&target)? {
                return Ok(());
            }
            if intent.identity.device == 0
                || !correct_directory(&target)?
                || !same_identity(&target, intent.identity, true)?
            {
                return Err(ManagedMutationRecoveryError::AmbiguousShape);
            }
            fs::remove_dir(&target).map_err(|error| {
                ManagedMutationRecoveryError::io("remove directory", &target, error)
            })?;
            sync_parent(&target)
        }
        IntentKind::MoveFile | IntentKind::MoveDirectory => {
            let from = confined_path(&intent.root, &intent.from)?;
            let to = confined_path(&intent.root, &intent.to)?;
            match (
                same_identity(&from, intent.identity, intent.kind.is_directory())?,
                same_identity(&to, intent.identity, intent.kind.is_directory())?,
                absent(&from)?,
                absent(&to)?,
                move_content_matches(intent, &from)?,
                move_content_matches(intent, &to)?,
            ) {
                (true, false, false, true, true, _) => Ok(()),
                (false, true, true, false, _, true) => {
                    move_exact(&to, &from, intent.identity, intent.kind.is_directory())
                }
                _ => Err(ManagedMutationRecoveryError::AmbiguousShape),
            }
        }
        IntentKind::DeleteFile | IntentKind::DeleteDirectory => {
            let from = confined_path(&intent.root, &intent.from)?;
            let staged = intent.staged_delete_path()?;
            match (
                same_identity(&from, intent.identity, intent.kind.is_directory())?,
                same_identity(&staged, intent.identity, intent.kind.is_directory())?,
                absent(&from)?,
                absent(&staged)?,
            ) {
                (true, false, false, true) => Ok(()),
                (false, true, true, false) => {
                    move_exact(&staged, &from, intent.identity, intent.kind.is_directory())
                }
                _ => Err(ManagedMutationRecoveryError::AmbiguousShape),
            }
        }
    }
}

fn delete_content_matches(
    intent: &ManagedMutationIntent,
    path: &Path,
) -> Result<bool, ManagedMutationRecoveryError> {
    if intent.kind == IntentKind::DeleteDirectory {
        Ok(true)
    } else {
        exact_regular_file(path, intent.content_digest)
    }
}

fn move_content_matches(
    intent: &ManagedMutationIntent,
    path: &Path,
) -> Result<bool, ManagedMutationRecoveryError> {
    // Pre-repair move markers carried a zero digest. Keep those append-only recovery records
    // readable while every newly written file move binds exact content across signing and crash
    // reconciliation.
    if intent.kind == IntentKind::MoveDirectory || intent.content_digest == [0; 32] {
        Ok(true)
    } else {
        exact_regular_file(path, intent.content_digest)
    }
}

fn move_exact(
    from: &Path,
    to: &Path,
    identity: FileIdentity,
    is_directory: bool,
) -> Result<(), ManagedMutationRecoveryError> {
    if !same_identity(from, identity, is_directory)? {
        return Err(ManagedMutationRecoveryError::AmbiguousShape);
    }
    let from_parent = from
        .parent()
        .ok_or(ManagedMutationRecoveryError::AmbiguousShape)?;
    let to_parent = to
        .parent()
        .ok_or(ManagedMutationRecoveryError::AmbiguousShape)?;
    let from_parent_identity = managed_directory_identity(from_parent).map_err(|error| {
        ManagedMutationRecoveryError::io("capture source parent identity", from_parent, error)
    })?;
    let to_parent_identity = managed_directory_identity(to_parent).map_err(|error| {
        ManagedMutationRecoveryError::io("capture destination parent identity", to_parent, error)
    })?;
    match atomic_rename_noreplace_in_exact_parents(
        from,
        to,
        from_parent_identity,
        to_parent_identity,
    ) {
        Ok(()) => {
            if same_identity(to, identity, is_directory)? {
                Ok(())
            } else {
                Err(ManagedMutationRecoveryError::after_mutation(
                    ManagedMutationRecoveryError::AmbiguousShape,
                ))
            }
        }
        Err(error) => {
            let error = ManagedMutationRecoveryError::io("move without replacement", from, error);
            if same_identity(to, identity, is_directory)? {
                Err(ManagedMutationRecoveryError::after_mutation(error))
            } else {
                Err(error)
            }
        }
    }
}

#[cfg(test)]
fn move_exact_with_sync<S>(
    from: &Path,
    to: &Path,
    identity: FileIdentity,
    is_directory: bool,
    sync: S,
) -> Result<(), ManagedMutationRecoveryError>
where
    S: FnOnce(&Path, &Path) -> Result<(), ManagedMutationRecoveryError>,
{
    if !same_identity(from, identity, is_directory)? {
        return Err(ManagedMutationRecoveryError::AmbiguousShape);
    }
    let from_parent = from
        .parent()
        .ok_or(ManagedMutationRecoveryError::AmbiguousShape)?;
    let to_parent = to
        .parent()
        .ok_or(ManagedMutationRecoveryError::AmbiguousShape)?;
    let from_parent_identity = managed_directory_identity(from_parent).map_err(|error| {
        ManagedMutationRecoveryError::io("capture source parent identity", from_parent, error)
    })?;
    let to_parent_identity = managed_directory_identity(to_parent).map_err(|error| {
        ManagedMutationRecoveryError::io("capture destination parent identity", to_parent, error)
    })?;
    atomic_rename_noreplace_in_exact_parents(from, to, from_parent_identity, to_parent_identity)
        .map_err(|error| {
            ManagedMutationRecoveryError::io("move without replacement", from, error)
        })?;
    sync(from, to).map_err(ManagedMutationRecoveryError::after_mutation)
}

fn remove_exact(
    path: &Path,
    identity: FileIdentity,
    is_directory: bool,
) -> Result<(), ManagedMutationRecoveryError> {
    remove_exact_with_sync(path, identity, is_directory, sync_parent)
}

fn remove_exact_with_sync<S>(
    path: &Path,
    identity: FileIdentity,
    is_directory: bool,
    sync: S,
) -> Result<(), ManagedMutationRecoveryError>
where
    S: FnOnce(&Path) -> Result<(), ManagedMutationRecoveryError>,
{
    if !same_identity(path, identity, is_directory)? {
        return Err(ManagedMutationRecoveryError::AmbiguousShape);
    }
    let result = if is_directory {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    };
    result.map_err(|error| ManagedMutationRecoveryError::io("remove", path, error))?;
    // The immutable journal or the pre-mutation shape decides whether this entry must stay
    // absent. Do not clear the recovery marker until the directory entry removal itself is
    // durable. In particular, syncing only the workspace root does not cover a staged delete or
    // created file inside a nested directory.
    sync(path).map_err(ManagedMutationRecoveryError::after_mutation)
}

fn confined_path(root: &Path, relative: &str) -> Result<PathBuf, ManagedMutationRecoveryError> {
    confined_path_with_private_staging(root, relative, false)
}

fn confined_internal_path(
    root: &Path,
    relative: &str,
) -> Result<PathBuf, ManagedMutationRecoveryError> {
    confined_path_with_private_staging(root, relative, true)
}

fn confined_path_with_private_staging(
    root: &Path,
    relative: &str,
    allow_staging_name: bool,
) -> Result<PathBuf, ManagedMutationRecoveryError> {
    let relative_path = Path::new(relative);
    if relative.is_empty() || relative_path.is_absolute() {
        return Err(ManagedMutationRecoveryError::UnrecognizedMarker);
    }
    let components = relative_path.components().collect::<Vec<_>>();
    if components.is_empty()
        || components
            .iter()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(ManagedMutationRecoveryError::UnrecognizedMarker);
    }
    let private = [
        "records.mesh",
        "metadata.sqlite",
        "metadata.sqlite-wal",
        "metadata.sqlite-shm",
        ".mesh-recovery.sqlite",
        ".mesh-recovery.sqlite-wal",
        ".mesh-recovery.sqlite-shm",
        "chunks",
        "scratch",
        "incoming",
        "quarantine",
        "logs",
        "mount",
        MARKER_NAME,
        NEXT_MARKER_NAME,
    ];
    if components.first().is_some_and(|component| match component {
        Component::Normal(name) => private.iter().any(|reserved| name == reserved),
        _ => true,
    }) {
        return Err(ManagedMutationRecoveryError::UnrecognizedMarker);
    }
    if !allow_staging_name
        && components.iter().any(|component| match component {
            Component::Normal(name) => name.to_string_lossy().starts_with(".mesh-delete-"),
            _ => true,
        })
    {
        return Err(ManagedMutationRecoveryError::UnrecognizedMarker);
    }
    let mut path = root.to_path_buf();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(name) = component else {
            return Err(ManagedMutationRecoveryError::UnrecognizedMarker);
        };
        path.push(name);
        if index + 1 != components.len() {
            match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
                Ok(_) => return Err(ManagedMutationRecoveryError::AmbiguousShape),
                Err(error) => {
                    return Err(ManagedMutationRecoveryError::io("metadata", &path, error));
                }
            }
        }
    }
    Ok(path)
}

fn exact_regular_file(
    path: &Path,
    expected: [u8; 32],
) -> Result<bool, ManagedMutationRecoveryError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(ManagedMutationRecoveryError::io("metadata", path, error)),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Ok(false);
    }
    let bytes =
        fs::read(path).map_err(|error| ManagedMutationRecoveryError::io("read", path, error))?;
    Ok(Blake3::digest_bytes(&bytes).as_bytes() == &expected)
}

fn correct_directory(path: &Path) -> Result<bool, ManagedMutationRecoveryError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(metadata.is_dir() && !metadata.file_type().is_symlink()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(ManagedMutationRecoveryError::io("metadata", path, error)),
    }
}

fn absent(path: &Path) -> Result<bool, ManagedMutationRecoveryError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(ManagedMutationRecoveryError::io("metadata", path, error)),
    }
}

fn same_identity(
    path: &Path,
    expected: FileIdentity,
    is_directory: bool,
) -> Result<bool, ManagedMutationRecoveryError> {
    match file_identity(path, is_directory) {
        Ok(actual) => Ok(actual == expected),
        Err(ManagedMutationRecoveryError::Io { source, .. })
            if source.kind() == io::ErrorKind::NotFound =>
        {
            Ok(false)
        }
        Err(ManagedMutationRecoveryError::AmbiguousShape) => Ok(false),
        Err(error) => Err(error),
    }
}

fn capture_source(
    path: &Path,
    is_directory: bool,
) -> Result<ManagedMutationSource, ManagedMutationRecoveryError> {
    let mut handle =
        File::open(path).map_err(|error| ManagedMutationRecoveryError::io("open", path, error))?;
    let metadata = handle
        .metadata()
        .map_err(|error| ManagedMutationRecoveryError::io("metadata", path, error))?;
    let identity = identity_from_metadata(path, &metadata, is_directory)?;
    if file_identity(path, is_directory)? != identity {
        return Err(ManagedMutationRecoveryError::AmbiguousShape);
    }
    let content_digest = if is_directory {
        [0; 32]
    } else {
        let mut bytes = Vec::new();
        handle
            .read_to_end(&mut bytes)
            .map_err(|error| ManagedMutationRecoveryError::io("read", path, error))?;
        *Blake3::digest_bytes(&bytes).as_bytes()
    };
    let parent_path = path
        .parent()
        .ok_or(ManagedMutationRecoveryError::AmbiguousShape)?;
    let parent = managed_directory_identity(parent_path).map_err(|error| {
        ManagedMutationRecoveryError::io("capture source parent identity", parent_path, error)
    })?;
    if file_identity(path, is_directory)? != identity {
        return Err(ManagedMutationRecoveryError::AmbiguousShape);
    }
    Ok(ManagedMutationSource {
        identity,
        content_digest,
        parent,
        handle,
    })
}

#[cfg(unix)]
fn file_identity(
    path: &Path,
    is_directory: bool,
) -> Result<FileIdentity, ManagedMutationRecoveryError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| ManagedMutationRecoveryError::io("metadata", path, error))?;
    identity_from_metadata(path, &metadata, is_directory)
}

#[cfg(unix)]
fn identity_from_metadata(
    _path: &Path,
    metadata: &fs::Metadata,
    is_directory: bool,
) -> Result<FileIdentity, ManagedMutationRecoveryError> {
    use std::os::unix::fs::MetadataExt as _;

    if metadata.file_type().is_symlink()
        || (is_directory && !metadata.is_dir())
        || (!is_directory && !metadata.is_file())
    {
        return Err(ManagedMutationRecoveryError::AmbiguousShape);
    }
    Ok(FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(not(unix))]
fn file_identity(
    path: &Path,
    _is_directory: bool,
) -> Result<FileIdentity, ManagedMutationRecoveryError> {
    Err(ManagedMutationRecoveryError::io(
        "identity",
        path,
        io::Error::new(
            io::ErrorKind::Unsupported,
            "managed mutation recovery requires stable file identity",
        ),
    ))
}

fn read_marker_bytes(path: &Path) -> Result<Vec<u8>, ManagedMutationRecoveryError> {
    let bytes =
        fs::read(path).map_err(|error| ManagedMutationRecoveryError::io("read", path, error))?;
    if bytes.len() > MAX_MARKER_BYTES {
        return Err(ManagedMutationRecoveryError::UnrecognizedMarker);
    }
    Ok(bytes)
}

fn sync_parent(path: &Path) -> Result<(), ManagedMutationRecoveryError> {
    sync_directory(path.parent().unwrap_or_else(|| Path::new(".")))
}

fn sync_directory(path: &Path) -> Result<(), ManagedMutationRecoveryError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| ManagedMutationRecoveryError::io("sync directory", path, error))
}

fn write_text(bytes: &mut Vec<u8>, text: &str) {
    let length = u32::try_from(text.len()).unwrap_or(u32::MAX);
    bytes.extend_from_slice(&length.to_be_bytes());
    bytes.extend_from_slice(text.as_bytes());
}

fn hex_prefix(bytes: &[u8], count: usize) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(count * 2);
    for byte in bytes.iter().take(count) {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

struct MarkerReader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> MarkerReader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], ManagedMutationRecoveryError> {
        let end = self
            .at
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len())
            .ok_or(ManagedMutationRecoveryError::UnrecognizedMarker)?;
        let value = &self.bytes[self.at..end];
        self.at = end;
        Ok(value)
    }

    fn byte(&mut self) -> Result<u8, ManagedMutationRecoveryError> {
        self.take(1)?
            .first()
            .copied()
            .ok_or(ManagedMutationRecoveryError::UnrecognizedMarker)
    }

    fn array(&mut self) -> Result<[u8; 32], ManagedMutationRecoveryError> {
        self.take(32)?
            .try_into()
            .map_err(|_| ManagedMutationRecoveryError::UnrecognizedMarker)
    }

    fn u64(&mut self) -> Result<u64, ManagedMutationRecoveryError> {
        self.take(8)?
            .try_into()
            .map(u64::from_be_bytes)
            .map_err(|_| ManagedMutationRecoveryError::UnrecognizedMarker)
    }

    fn text(&mut self) -> Result<String, ManagedMutationRecoveryError> {
        let count = self
            .take(4)?
            .try_into()
            .map(u32::from_be_bytes)
            .map_err(|_| ManagedMutationRecoveryError::UnrecognizedMarker)?;
        let count =
            usize::try_from(count).map_err(|_| ManagedMutationRecoveryError::UnrecognizedMarker)?;
        std::str::from_utf8(self.take(count)?)
            .map(str::to_owned)
            .map_err(|_| ManagedMutationRecoveryError::UnrecognizedMarker)
    }

    const fn done(&self) -> bool {
        self.at == self.bytes.len()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static SCRATCH: AtomicU64 = AtomicU64::new(1);

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "mesh-managed-mutation-{name}-{}-{}",
            std::process::id(),
            SCRATCH.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create scratch root");
        path
    }

    fn digest(value: u8) -> RecordDigest {
        RecordDigest::from_bytes([value; 32])
    }

    fn copy_tree(source: &Path, destination: &Path) {
        fs::create_dir(destination).expect("create copied root");
        for entry in fs::read_dir(source).expect("read source") {
            let entry = entry.expect("source entry");
            let target = destination.join(entry.file_name());
            if entry.file_type().expect("entry type").is_dir() {
                copy_tree(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), target).expect("copy file");
            }
        }
    }

    #[test]
    fn copied_root_cannot_inherit_marker_completion_authority() {
        let root = scratch("pinned-marker-copy");
        let retained = root.with_extension("retained");
        let replacement = root.with_extension("replacement");
        let pinned = crate::root_authority::PinnedWorkspaceRoot::open(root.clone())
            .expect("pin workspace root");
        let parent = managed_directory_identity(&root).expect("root identity");
        let intent = ManagedMutationIntent::begin_create_file_guarded(
            &root,
            "draft.txt",
            digest(99),
            b"draft",
            parent,
            Some(pinned.filesystem()),
        )
        .expect("create pinned marker");
        copy_tree(&root, &replacement);
        fs::rename(&root, &retained).expect("move admitted root");
        fs::rename(&replacement, &root).expect("install byte-identical replacement");

        intent.clear().expect("clear retained marker");

        assert!(!retained.join(MARKER_NAME).exists());
        assert!(
            root.join(MARKER_NAME).exists(),
            "the copied marker is not the retained intent and must not be mutated"
        );
        fs::remove_dir_all(&root).expect("remove replacement");
        fs::remove_dir_all(&retained).expect("remove retained");
    }

    #[test]
    fn create_file_rolls_back_without_journal_and_completes_with_journal() {
        let root = scratch("create-file");
        let mut intent =
            ManagedMutationIntent::begin_create_file(&root, "notes.txt", digest(1), b"draft")
                .expect("intent");
        fs::write(root.join("notes.txt"), b"draft").expect("materialize file");
        intent
            .bind_created(&root.join("notes.txt"))
            .expect("bind identity");

        assert_eq!(
            reconcile_pending_mutation(&root, |_| false).expect("rollback"),
            Some(ManagedMutationRecovery::RolledBack)
        );
        assert!(!root.join("notes.txt").exists());
        assert!(!intent.marker().exists());

        let mut intent =
            ManagedMutationIntent::begin_create_file(&root, "notes.txt", digest(2), b"saved")
                .expect("intent");
        fs::write(root.join("notes.txt"), b"saved").expect("materialize file");
        intent
            .bind_created(&root.join("notes.txt"))
            .expect("bind identity");
        assert_eq!(
            reconcile_pending_mutation(&root, |id| id == &digest(2)).expect("complete"),
            Some(ManagedMutationRecovery::Completed)
        );
        assert_eq!(
            fs::read(root.join("notes.txt")).expect("saved file"),
            b"saved"
        );
        assert!(!intent.marker().exists());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn create_file_cannot_escape_through_a_swapped_parent_directory() {
        use std::os::unix::fs::symlink;

        let root = scratch("create-parent-swap");
        let outside = scratch("create-parent-swap-outside");
        let parent = root.join("folder");
        let displaced = root.join("displaced-folder");
        fs::create_dir(&parent).expect("managed parent");

        let intent = ManagedMutationIntent::begin_create_file(
            &root,
            "folder/draft.txt",
            digest(45),
            b"managed draft",
        )
        .expect("intent checked the original parent");
        fs::rename(&parent, &displaced).expect("displace checked parent");
        symlink(&outside, &parent).expect("replace it with an outside link");

        let result = intent.apply_create_file(b"managed draft");
        let escaped = fs::read(outside.join("draft.txt")).ok();

        fs::remove_dir_all(&root).expect("cleanup root");
        fs::remove_dir_all(&outside).expect("cleanup outside");

        assert!(result.is_err(), "the swapped parent must refuse creation");
        assert_eq!(escaped, None, "managed bytes escaped the workspace root");
    }

    #[cfg(unix)]
    #[test]
    fn move_cannot_escape_through_a_swapped_destination_parent() {
        use std::os::unix::fs::symlink;

        let root = scratch("move-parent-swap");
        let outside = scratch("move-parent-swap-outside");
        let parent = root.join("folder");
        let displaced = root.join("displaced-folder");
        fs::create_dir(&parent).expect("managed destination parent");
        fs::write(root.join("draft.txt"), b"managed draft").expect("managed source");

        let intent = ManagedMutationIntent::begin_move(
            &root,
            "draft.txt",
            "folder/draft.txt",
            digest(46),
            false,
        )
        .expect("intent checked the original destination parent");
        fs::rename(&parent, &displaced).expect("displace checked destination parent");
        symlink(&outside, &parent).expect("replace it with an outside link");

        let result = intent.apply_move();
        let escaped = fs::read(outside.join("draft.txt")).ok();

        fs::remove_dir_all(&root).expect("cleanup root");
        fs::remove_dir_all(&outside).expect("cleanup outside");

        assert!(result.is_err(), "the swapped parent must refuse the move");
        assert_eq!(escaped, None, "managed bytes escaped the workspace root");
    }

    #[test]
    fn admitted_parent_identity_survives_real_directory_replacement() {
        let root = scratch("real-parent-replacement");
        let parent = root.join("folder");
        let displaced = root.join("displaced-folder");
        fs::create_dir(&parent).expect("managed parent");

        let create = ManagedMutationIntent::begin_create_file(
            &root,
            "folder/new.txt",
            digest(47),
            b"new body",
        )
        .expect("create intent");
        fs::rename(&parent, &displaced).expect("displace admitted create parent");
        fs::create_dir(&parent).expect("install lookalike create parent");
        let create_error = create
            .apply_create_file(b"new body")
            .expect_err("lookalike parent must not inherit create authority");
        create
            .clear_after_failed_apply(&create_error)
            .expect("pre-apply create refusal clears marker");
        assert!(!parent.join("new.txt").exists());
        assert!(!displaced.join("new.txt").exists());

        fs::write(displaced.join("source.txt"), b"move body").expect("original move source");
        let move_intent = ManagedMutationIntent::begin_move(
            &root,
            "displaced-folder/source.txt",
            "displaced-folder/moved.txt",
            digest(48),
            false,
        )
        .expect("move intent");
        let second_displaced = root.join("second-displaced-folder");
        fs::rename(&displaced, &second_displaced).expect("displace admitted move parent");
        fs::create_dir(&displaced).expect("install lookalike move parent");
        fs::write(displaced.join("source.txt"), b"move body").expect("lookalike source bytes");
        let move_error = move_intent
            .apply_move()
            .expect_err("lookalike parent must not inherit move authority");
        move_intent
            .clear_after_failed_apply(&move_error)
            .expect("pre-apply move refusal clears marker");
        assert_eq!(
            fs::read(displaced.join("source.txt")).expect("lookalike source retained"),
            b"move body"
        );
        assert_eq!(
            fs::read(second_displaced.join("source.txt")).expect("original source retained"),
            b"move body"
        );
        assert!(!displaced.join("moved.txt").exists());
        assert!(!second_displaced.join("moved.txt").exists());

        let delete_intent = ManagedMutationIntent::begin_delete(
            &root,
            "displaced-folder/source.txt",
            digest(49),
            false,
        )
        .expect("delete intent");
        fs::remove_file(displaced.join("source.txt")).expect("replace admitted delete object");
        fs::write(displaced.join("source.txt"), b"move body").expect("lookalike delete source");
        let delete_error = delete_intent
            .stage_delete()
            .expect_err("lookalike object must not inherit delete authority");
        delete_intent
            .clear_after_failed_apply(&delete_error)
            .expect("pre-apply delete refusal clears marker");
        assert_eq!(
            fs::read(displaced.join("source.txt")).expect("replacement source retained"),
            b"move body"
        );

        fs::remove_dir_all(&root).expect("cleanup root");
    }

    #[test]
    fn create_directory_refuses_a_replaced_parent() {
        let root = scratch("create-directory-parent-replacement");
        let parent = root.join("folder");
        let displaced = root.join("displaced-folder");
        fs::create_dir(&parent).expect("managed parent");
        let intent =
            ManagedMutationIntent::begin_create_directory(&root, "folder/nested", digest(50))
                .expect("directory intent");

        fs::rename(&parent, &displaced).expect("displace admitted parent");
        fs::create_dir(&parent).expect("install lookalike parent");
        let error = intent
            .apply_create_directory()
            .expect_err("lookalike parent must not inherit directory creation authority");
        intent
            .clear_after_failed_apply(&error)
            .expect("pre-apply refusal clears marker");

        assert!(!parent.join("nested").exists());
        assert!(!displaced.join("nested").exists());
        fs::remove_dir_all(&root).expect("cleanup root");
    }

    #[test]
    fn create_directory_marker_only_is_a_safe_noop_rollback() {
        let root = scratch("create-dir");
        ManagedMutationIntent::begin_create_directory(&root, "folder", digest(3)).expect("intent");
        assert_eq!(
            reconcile_pending_mutation(&root, |_| false).expect("rollback"),
            Some(ManagedMutationRecovery::RolledBack)
        );
        assert!(!root.join("folder").exists());
        assert!(!root.join(MARKER_NAME).exists());

        ManagedMutationIntent::begin_create_directory(&root, "external", digest(31))
            .expect("unbound intent");
        fs::create_dir(root.join("external")).expect("external directory");
        assert!(matches!(
            reconcile_pending_mutation(&root, |_| false),
            Err(ManagedMutationRecoveryError::AmbiguousShape)
        ));
        assert!(root.join("external").is_dir());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn move_rolls_back_or_completes_from_exact_identity() {
        let root = scratch("move");
        fs::write(root.join("from.txt"), b"body").expect("source");
        ManagedMutationIntent::begin_move(&root, "from.txt", "to.txt", digest(4), false)
            .expect("intent");
        fs::rename(root.join("from.txt"), root.join("to.txt")).expect("move");
        assert_eq!(
            reconcile_pending_mutation(&root, |_| false).expect("rollback"),
            Some(ManagedMutationRecovery::RolledBack)
        );
        assert!(root.join("from.txt").exists());
        assert!(!root.join("to.txt").exists());

        ManagedMutationIntent::begin_move(&root, "from.txt", "to.txt", digest(5), false)
            .expect("intent");
        fs::rename(root.join("from.txt"), root.join("to.txt")).expect("move");
        assert_eq!(
            reconcile_pending_mutation(&root, |_| true).expect("complete"),
            Some(ManagedMutationRecovery::Completed)
        );
        assert!(!root.join("from.txt").exists());
        assert_eq!(fs::read(root.join("to.txt")).expect("target"), b"body");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn an_atomic_move_never_replaces_a_destination_entry() {
        let root = scratch("move-no-replace");
        let source = root.join("source.txt");
        let destination = root.join("destination.txt");
        fs::write(&source, b"signed source").expect("source");
        fs::write(&destination, b"newer destination").expect("destination");
        let source_identity = file_identity(&source, false).expect("source identity");

        let occupied = move_exact(&source, &destination, source_identity, false)
            .expect_err("occupied destination refuses");
        assert!(!occupied.mutation_may_have_applied());
        assert_eq!(
            fs::read(&source).expect("source retained"),
            b"signed source"
        );
        assert_eq!(
            fs::read(&destination).expect("destination retained"),
            b"newer destination"
        );

        // The replacing primitive used before this guard proves why an earlier absence check is
        // not a safety boundary: if a destination appears after that check, ordinary rename
        // silently destroys it.
        let replacing_source = root.join("replacing-source.txt");
        let replacing_destination = root.join("replacing-destination.txt");
        fs::write(&replacing_source, b"source").expect("mutant source");
        fs::write(&replacing_destination, b"newer").expect("mutant destination");
        fs::rename(&replacing_source, &replacing_destination)
            .expect("the planted replacing primitive accepts an occupied target");
        assert_eq!(
            fs::read(&replacing_destination).expect("mutant result"),
            b"source"
        );

        fs::remove_file(&destination).expect("free destination");
        move_exact(&source, &destination, source_identity, false).expect("move into absent target");
        assert!(!source.exists());
        assert_eq!(
            fs::read(&destination).expect("moved source"),
            b"signed source"
        );

        let source_directory = root.join("source-directory");
        let destination_directory = root.join("destination-directory");
        fs::create_dir(&source_directory).expect("source directory");
        fs::write(source_directory.join("child.txt"), b"child").expect("source child");
        fs::create_dir(&destination_directory).expect("destination directory");
        let directory_identity =
            file_identity(&source_directory, true).expect("directory identity");
        assert!(move_exact(
            &source_directory,
            &destination_directory,
            directory_identity,
            true,
        )
        .is_err());
        assert_eq!(
            fs::read(source_directory.join("child.txt")).expect("source directory retained"),
            b"child"
        );
        assert!(destination_directory.is_dir());

        fs::remove_dir(&destination_directory).expect("free directory destination");
        move_exact(
            &source_directory,
            &destination_directory,
            directory_identity,
            true,
        )
        .expect("directory move into absent target");
        assert!(!source_directory.exists());
        assert_eq!(
            fs::read(destination_directory.join("child.txt")).expect("moved directory child"),
            b"child"
        );

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn a_post_rename_sync_failure_keeps_the_marker_for_restart_reconciliation() {
        fn fail_sync(_from: &Path, to: &Path) -> Result<(), ManagedMutationRecoveryError> {
            Err(ManagedMutationRecoveryError::io(
                "injected parent sync",
                to,
                io::Error::other("injected post-rename failure"),
            ))
        }

        let root = scratch("post-rename-sync-failure");
        fs::write(root.join("from.txt"), b"move body").expect("move source");
        let move_intent =
            ManagedMutationIntent::begin_move(&root, "from.txt", "to.txt", digest(41), false)
                .expect("move intent");
        let move_error = move_intent
            .apply_move_with_sync(fail_sync)
            .expect_err("sync fails after the atomic rename");
        assert!(move_error.mutation_may_have_applied());
        move_intent
            .clear_after_failed_apply(&move_error)
            .expect("post-rename error retains intent");
        assert!(!root.join("from.txt").exists());
        assert_eq!(
            fs::read(root.join("to.txt")).expect("moved bytes"),
            b"move body"
        );
        assert!(move_intent.marker().is_file(), "recovery authority remains");
        assert_eq!(
            reconcile_pending_mutation(&root, |_| false).expect("restart rolls move back"),
            Some(ManagedMutationRecovery::RolledBack)
        );
        assert_eq!(
            fs::read(root.join("from.txt")).expect("move restored"),
            b"move body"
        );
        assert!(!root.join("to.txt").exists());

        fs::write(root.join("delete.txt"), b"delete body").expect("delete source");
        let delete_intent =
            ManagedMutationIntent::begin_delete(&root, "delete.txt", digest(42), false)
                .expect("delete intent");
        let delete_error = delete_intent
            .stage_delete_with_sync(fail_sync)
            .expect_err("sync fails after staging rename");
        assert!(delete_error.mutation_may_have_applied());
        delete_intent
            .clear_after_failed_apply(&delete_error)
            .expect("post-stage error retains intent");
        let staged = delete_intent.staged_delete_path().expect("staged path");
        assert!(!root.join("delete.txt").exists());
        assert_eq!(fs::read(&staged).expect("staged bytes"), b"delete body");
        assert!(delete_intent.marker().is_file(), "delete recovery remains");
        assert_eq!(
            reconcile_pending_mutation(&root, |_| false).expect("restart restores delete"),
            Some(ManagedMutationRecovery::RolledBack)
        );
        assert_eq!(
            fs::read(root.join("delete.txt")).expect("delete restored"),
            b"delete body"
        );
        assert!(!staged.exists());

        fs::write(root.join("occupied-from.txt"), b"source").expect("occupied source");
        fs::write(root.join("occupied-to.txt"), b"destination").expect("occupied target");
        let refused_intent = ManagedMutationIntent::begin_move(
            &root,
            "occupied-from.txt",
            "occupied-to.txt",
            digest(43),
            false,
        )
        .expect("refused move intent");
        let refused = refused_intent
            .apply_move()
            .expect_err("occupied target refuses before rename");
        assert!(!refused.mutation_may_have_applied());
        refused_intent
            .clear_after_failed_apply(&refused)
            .expect("pre-rename refusal clears intent");
        assert!(!refused_intent.marker().exists());
        assert_eq!(
            fs::read(root.join("occupied-from.txt")).expect("source retained"),
            b"source"
        );
        assert_eq!(
            fs::read(root.join("occupied-to.txt")).expect("target retained"),
            b"destination"
        );

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn a_post_remove_sync_failure_is_treated_as_an_applied_mutation() {
        let root = scratch("post-remove-sync-failure");
        let nested = root.join("nested");
        fs::create_dir(&nested).expect("nested parent");
        let removed = nested.join("removed.txt");
        fs::write(&removed, b"removed body").expect("remove target");
        let identity = file_identity(&removed, false).expect("target identity");
        let mut sync_called = false;

        let error = remove_exact_with_sync(&removed, identity, false, |path| {
            sync_called = true;
            assert_eq!(path, removed);
            Err(ManagedMutationRecoveryError::io(
                "injected parent sync",
                path,
                io::Error::other("injected post-remove failure"),
            ))
        })
        .expect_err("directory sync fails after the entry is removed");

        assert!(
            sync_called,
            "removal must cross a directory durability barrier"
        );
        assert!(
            !removed.exists(),
            "the unlink linearized before sync failed"
        );
        assert!(
            error.mutation_may_have_applied(),
            "the caller must retain its recovery marker after an uncertain unlink"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn delete_rolls_back_or_finishes_from_staged_identity() {
        let root = scratch("delete");
        fs::write(root.join("gone.txt"), b"body").expect("source");
        let intent = ManagedMutationIntent::begin_delete(&root, "gone.txt", digest(6), false)
            .expect("intent");
        let staged = intent.staged_delete_path().expect("stage path");
        fs::rename(root.join("gone.txt"), &staged).expect("stage");
        assert_eq!(
            reconcile_pending_mutation(&root, |_| false).expect("rollback"),
            Some(ManagedMutationRecovery::RolledBack)
        );
        assert_eq!(fs::read(root.join("gone.txt")).expect("restored"), b"body");

        let intent = ManagedMutationIntent::begin_delete(&root, "gone.txt", digest(7), false)
            .expect("intent");
        let staged = intent.staged_delete_path().expect("stage path");
        fs::rename(root.join("gone.txt"), &staged).expect("stage");
        assert_eq!(
            reconcile_pending_mutation(&root, |_| true).expect("complete"),
            Some(ManagedMutationRecovery::Completed)
        );
        assert!(!root.join("gone.txt").exists());
        assert!(!staged.exists());

        fs::write(root.join("changed.txt"), b"before").expect("changed source");
        let intent = ManagedMutationIntent::begin_delete(&root, "changed.txt", digest(32), false)
            .expect("intent");
        let staged = intent.staged_delete_path().expect("stage path");
        fs::rename(root.join("changed.txt"), &staged).expect("stage");
        fs::write(&staged, b"newer bytes").expect("newer write");
        assert!(matches!(
            reconcile_pending_mutation(&root, |_| true),
            Err(ManagedMutationRecoveryError::AmbiguousShape)
        ));
        assert_eq!(
            fs::read(&staged).expect("preserved newer bytes"),
            b"newer bytes"
        );
        assert!(intent.marker().exists());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn malformed_marker_and_replaced_identity_are_preserved_for_attention() {
        let root = scratch("ambiguous");
        fs::write(root.join(MARKER_NAME), b"not canonical").expect("marker");
        assert!(matches!(
            reconcile_pending_mutation(&root, |_| false),
            Err(ManagedMutationRecoveryError::UnrecognizedMarker)
        ));
        assert_eq!(
            fs::read(root.join(MARKER_NAME)).expect("preserved marker"),
            b"not canonical"
        );

        fs::remove_file(root.join(MARKER_NAME)).expect("reset marker");
        fs::write(root.join("from.txt"), b"original").expect("source");
        let intent =
            ManagedMutationIntent::begin_move(&root, "from.txt", "to.txt", digest(8), false)
                .expect("intent");
        fs::rename(root.join("from.txt"), root.join("to.txt")).expect("move");
        fs::write(root.join("from.txt"), b"replacement").expect("replacement");
        assert!(matches!(
            reconcile_pending_mutation(&root, |_| false),
            Err(ManagedMutationRecoveryError::AmbiguousShape)
        ));
        assert_eq!(
            fs::read(root.join("from.txt")).expect("replacement"),
            b"replacement"
        );
        assert_eq!(fs::read(root.join("to.txt")).expect("moved"), b"original");
        assert!(intent.marker().exists());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn nondurable_rollback_cannot_delete_a_replacement_at_a_created_path() {
        let root = scratch("created-replacement-rollback");
        let target = root.join("draft.txt");
        let mut intent =
            ManagedMutationIntent::begin_create_file(&root, "draft.txt", digest(44), b"draft")
                .expect("intent");
        fs::write(&target, b"draft").expect("created file");
        intent.bind_created(&target).expect("captured identity");

        fs::remove_file(&target).expect("replace created identity");
        fs::write(&target, b"newer external bytes").expect("external replacement");

        assert!(matches!(
            intent.roll_back_nondurable(),
            Err(ManagedMutationRecoveryError::AmbiguousShape)
        ));
        assert_eq!(
            fs::read(&target).expect("replacement retained"),
            b"newer external bytes"
        );
        assert!(intent.marker().exists(), "ambiguous intent remains visible");

        // The pre-repair live rollback used this path-only primitive. It has no way to distinguish
        // the created inode from the replacement and therefore deletes the newer bytes.
        fs::remove_file(&target).expect("planted path-only rollback");
        assert!(
            !target.exists(),
            "the planted direct rollback did not reproduce the deletion"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn workspace_open_reports_automatic_rollback() {
        let root = scratch("open-rollback");
        drop(crate::OpenWorkspace::open(&root).expect("initialize workspace"));
        let mut intent =
            ManagedMutationIntent::begin_create_file(&root, "draft.txt", digest(9), b"draft")
                .expect("intent");
        fs::write(root.join("draft.txt"), b"draft").expect("materialize file");
        intent
            .bind_created(&root.join("draft.txt"))
            .expect("bind identity");

        let opened = crate::OpenWorkspace::open(&root).expect("reopen");
        assert!(!root.join("draft.txt").exists());
        assert!(opened
            .conditions()
            .iter()
            .any(|condition| condition.code() == "managed-mutation-rolled-back"));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn workspace_open_preserves_ambiguous_marker_and_reports_attention() {
        let root = scratch("open-attention");
        drop(crate::OpenWorkspace::open(&root).expect("initialize workspace"));
        fs::write(root.join(MARKER_NAME), b"not canonical").expect("marker");

        let opened = crate::OpenWorkspace::open(&root).expect("reopen");
        assert!(opened
            .conditions()
            .iter()
            .any(|condition| condition.code() == "managed-mutation-recovery-needed"));
        assert_eq!(
            fs::read(root.join(MARKER_NAME)).expect("preserved"),
            b"not canonical"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }
}
