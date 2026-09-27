//! Native-only editing of one file in a verified managed copy.
//!
//! This module deliberately does not add a daemon IPC method. The desktop host owns the in-process
//! [`crate::LiveDaemon`] and uses this bounded path to keep ordinary text editing on the existing
//! CAS, SQLite, immutable-journal and checkpoint sequence.

use core::fmt;
#[cfg(test)]
use std::cell::RefCell;
#[cfg(unix)]
use std::ffi::{CString, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read as _, Write as _};
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt as _;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::os::unix::fs::OpenOptionsExt as _;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
#[cfg(unix)]
use std::os::unix::io::{AsRawFd as _, FromRawFd as _};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use mesh_operations::PortableMetadata;
use mesh_store::RecordDigest;
use mesh_types::ContentDigest as _;

/// Desktop text files are bounded so an accidental binary selection cannot freeze the webview.
pub const MAX_MANAGED_TEXT_BYTES: usize = 2 * 1024 * 1024;

/// The measured settling decision selected by TASK-347.
pub const LOCAL_EDIT_IDLE_MILLIS: u64 = 50;

static TEMPORARY_COUNTER: AtomicU64 = AtomicU64::new(1);

pub(crate) mod retained_replacement;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ManagedDirectoryIdentity {
    device: u64,
    inode: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ManagedFileIdentity {
    pub(crate) device: u64,
    pub(crate) inode: u64,
    incarnation: ManagedFileIncarnation,
}

/// Allocation identity for one file object beyond its recyclable `(device, inode)` pair.
///
/// The public workspace object identity deliberately remains device/inode because it survives a
/// rename. A Pull-back receipt has a narrower job: authorize later deletion of the exact file Mesh
/// installed. Linux can immediately recycle a deleted inode for an unrelated byte-identical file,
/// so that authority also binds the kernel's creation-time discriminator when the platform
/// exposes it. A filesystem without birth time falls back to inode change time. That can refuse a
/// later cleanup after harmless metadata activity, but it cannot silently fall back to the
/// recyclable pair and grant deletion authority to a same-byte replacement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ManagedFileIncarnation {
    kind: char,
    seconds: i64,
    nanoseconds: i64,
}

struct ConfinedParent {
    path: PathBuf,
    directory: File,
    name: OsString,
}

type ConfinedRegularFileRead = (
    PathBuf,
    File,
    OsString,
    fs::Metadata,
    ManagedFileIdentity,
    Vec<u8>,
);

/// Exact filesystem authority retained from a confined read until replacement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ManagedReplacementTarget {
    root: PathBuf,
    relative: String,
    reserve_private_top_level: bool,
    pub(crate) path: PathBuf,
    parent: ManagedDirectoryIdentity,
    file: ManagedFileIdentity,
    mode: u32,
}

/// One confined export destination observed either as an exact file or an exact empty name.
pub(crate) enum ManagedExportTarget {
    Existing(ManagedReplacementTarget, Vec<u8>),
    Missing {
        path: PathBuf,
        parent: ManagedDirectoryIdentity,
    },
}

/// One confined directory-export destination observed as an exact directory or absent name.
pub(crate) enum ManagedDirectoryExportTarget {
    Existing {
        parent: ManagedDirectoryIdentity,
        directory: ManagedDirectoryIdentity,
        empty: bool,
    },
    Missing {
        path: PathBuf,
        parent: ManagedDirectoryIdentity,
    },
}

impl ManagedReplacementTarget {
    pub(crate) const fn executable(&self) -> bool {
        self.mode & 0o111 != 0
    }

    pub(crate) const fn mode_with_executable(&self, executable: bool) -> u32 {
        if executable {
            self.mode | 0o111
        } else {
            self.mode & !0o111
        }
    }

    pub(crate) fn same_file_as(&self, other: &Self) -> bool {
        self.root == other.root
            && self.relative == other.relative
            && self.reserve_private_top_level == other.reserve_private_top_level
            && self.path == other.path
            && self.parent == other.parent
            && self.file == other.file
    }

    pub(crate) fn parent_installation(&self) -> String {
        self.parent.token()
    }

    pub(crate) fn file_installation(&self) -> String {
        self.file.token()
    }
}

impl ManagedDirectoryIdentity {
    pub(crate) fn token(self) -> String {
        format!("{:016x}:{:016x}", self.device, self.inode)
    }
}

impl ManagedFileIdentity {
    pub(crate) fn token(self) -> String {
        format!(
            "{:016x}:{:016x}:{}{:016x}.{:016x}",
            self.device,
            self.inode,
            self.incarnation.kind,
            u64::from_ne_bytes(self.incarnation.seconds.to_ne_bytes()),
            u64::from_ne_bytes(self.incarnation.nanoseconds.to_ne_bytes()),
        )
    }
}

impl ManagedExportTarget {
    pub(crate) fn parent_installation(&self) -> String {
        match self {
            Self::Existing(target, _) => target.parent_installation(),
            Self::Missing { parent, .. } => parent.token(),
        }
    }

    pub(crate) fn file_installation(&self) -> Option<String> {
        match self {
            Self::Existing(target, _) => Some(target.file_installation()),
            Self::Missing { .. } => None,
        }
    }
}

impl ManagedDirectoryExportTarget {
    pub(crate) fn parent_installation(&self) -> String {
        match self {
            Self::Existing { parent, .. } | Self::Missing { parent, .. } => parent.token(),
        }
    }

    pub(crate) fn directory_installation(&self) -> Option<String> {
        match self {
            Self::Existing { directory, .. } => Some(directory.token()),
            Self::Missing { .. } => None,
        }
    }

    pub(crate) const fn is_empty(&self) -> Option<bool> {
        match self {
            Self::Existing { empty, .. } => Some(*empty),
            Self::Missing { .. } => None,
        }
    }

    pub(crate) const fn parent_identity(&self) -> ManagedDirectoryIdentity {
        match self {
            Self::Existing { parent, .. } | Self::Missing { parent, .. } => *parent,
        }
    }
}

#[cfg(test)]
thread_local! {
    static BEFORE_RENAME_HOOK: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
    static BEFORE_READ_HOOK: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
}

#[cfg(test)]
fn run_before_rename_hook() {
    BEFORE_RENAME_HOOK.with(|hook| {
        if let Some(hook) = hook.borrow_mut().take() {
            hook();
        }
    });
}

#[cfg(not(test))]
fn run_before_rename_hook() {}

#[cfg(test)]
fn run_before_read_hook() {
    BEFORE_READ_HOOK.with(|hook| {
        if let Some(hook) = hook.borrow_mut().take() {
            hook();
        }
    });
}

#[cfg(not(test))]
fn run_before_read_hook() {}

/// One confined UTF-8 file read from the managed operating-system folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagedTextFile {
    path: String,
    text: String,
    current_version: String,
    modified_from_current_version: bool,
}

/// One confined tracked file inspected against its current durable manifest.
///
/// Unlike [`ManagedTextFile`], this answer remains available for binary and large files. The
/// optional text is only a convenience for the bounded desktop editor; exact byte count, digest,
/// and modification truth cover both exact bytes and portable executable metadata for every
/// regular file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagedFileInspection {
    path: String,
    current_version: String,
    byte_count: u64,
    content_digest: RecordDigest,
    executable: bool,
    text: Option<String>,
    bytes: Vec<u8>,
    modified_from_current_version: bool,
}

/// One confined regular file found in the native working folder but absent from durable history.
///
/// This is an inspection result, not proof that the fallback observed the file's creation. The
/// desktop may show the exact bytes and explicitly adopt them as a new authenticated private file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeFileInspection {
    path: String,
    byte_count: u64,
    content_digest: RecordDigest,
    executable: bool,
    text: Option<String>,
    bytes: Vec<u8>,
}

/// One native directory absent from durable history and ready for explicit adoption.
///
/// The opaque installation token binds the later operation to the same operating-system
/// directory object. Discovery alone is never durable authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeDirectoryInspection {
    path: String,
    installation: String,
}

impl NativeDirectoryInspection {
    pub(crate) fn new(path: String, installation: String) -> Self {
        Self { path, installation }
    }

    /// Root-relative native directory path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Opaque identity of the exact directory observed during discovery.
    #[must_use]
    pub fn installation(&self) -> &str {
        &self.installation
    }
}

/// One tracked regular file whose exact durable name is absent from the native working folder.
///
/// Absence is discovery, not deletion authority. The desktop can pair this with an exact
/// untracked-file inspection and explicitly adopt a rename, or explicitly record a deletion. Both
/// mutations revalidate the workspace, durable version, confined absence and destination state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeMissingFile {
    path: String,
    current_version: String,
    content_digest: RecordDigest,
    executable: bool,
}

impl NativeMissingFile {
    pub(crate) fn new(
        path: String,
        current_version: String,
        content_digest: RecordDigest,
        executable: bool,
    ) -> Self {
        Self {
            path,
            current_version,
            content_digest,
            executable,
        }
    }

    /// Durable root-relative name that is absent from the verified native parent.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Exact durable file version the structural change would move or delete.
    #[must_use]
    pub fn current_version(&self) -> &str {
        &self.current_version
    }

    /// Digest of that version's retained bytes, used only to suggest exact rename candidates.
    #[must_use]
    pub const fn content_digest(&self) -> RecordDigest {
        self.content_digest
    }

    /// Portable executable state of the durable version.
    #[must_use]
    pub const fn executable(&self) -> bool {
        self.executable
    }
}

/// Exact preview of copying one current saved file into a person-selected ordinary folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagedFileExportPreview {
    path: String,
    source_version: String,
    source_byte_count: u64,
    source_content_digest: RecordDigest,
    source_executable: bool,
    source_text: Option<String>,
    target_root: String,
    target_installation: String,
    target_parent_installation: String,
    target_file_installation: Option<String>,
    target_byte_count: Option<u64>,
    target_content_digest: Option<RecordDigest>,
    target_executable: Option<bool>,
    target_text: Option<String>,
    identical: bool,
    target_relation: &'static str,
    replace_allowed: bool,
}

impl ManagedFileExportPreview {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        path: String,
        source_version: String,
        source: &[u8],
        source_executable: bool,
        target_root: String,
        target_installation: String,
        target_parent_installation: String,
        target_file_installation: Option<String>,
        target: Option<(&[u8], bool)>,
        target_relation: &'static str,
        replace_allowed: bool,
    ) -> Self {
        let (target_byte_count, target_content_digest, target_executable, target_text, identical) =
            target.map_or(
                (None, None, None, None, false),
                |(target, target_executable)| {
                    (
                        Some(u64::try_from(target.len()).unwrap_or(u64::MAX)),
                        Some(RecordDigest::from_bytes(
                            *mesh_types::Blake3::digest_bytes(target).as_bytes(),
                        )),
                        Some(target_executable),
                        bounded_text(target),
                        source == target && source_executable == target_executable,
                    )
                },
            );
        Self {
            path,
            source_version,
            source_byte_count: u64::try_from(source.len()).unwrap_or(u64::MAX),
            source_content_digest: RecordDigest::from_bytes(
                *mesh_types::Blake3::digest_bytes(source).as_bytes(),
            ),
            source_executable,
            source_text: bounded_text(source),
            target_root,
            target_installation,
            target_parent_installation,
            target_file_installation,
            target_byte_count,
            target_content_digest,
            target_executable,
            target_text,
            identical,
            target_relation,
            replace_allowed,
        }
    }

    /// Drop bounded display text while retaining every field required to review or execute export.
    ///
    /// Whole-workspace preview uses this immediately after inspecting each file so the batch never
    /// retains all reconstructed file bodies at once. Digests, byte counts, executable state,
    /// destination identity, and provenance remain exact.
    pub(crate) fn without_text(mut self) -> Self {
        self.source_text = None;
        self.target_text = None;
        self
    }

    /// Root-relative file selected from durable history.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
    /// Current immutable version reconstructed for export.
    #[must_use]
    pub fn source_version(&self) -> &str {
        &self.source_version
    }
    /// Exact saved byte count.
    #[must_use]
    pub const fn source_byte_count(&self) -> u64 {
        self.source_byte_count
    }
    /// BLAKE3 digest of the exact saved bytes.
    #[must_use]
    pub const fn source_content_digest(&self) -> RecordDigest {
        self.source_content_digest
    }
    /// Portable executable state of the saved file.
    #[must_use]
    pub const fn source_executable(&self) -> bool {
        self.source_executable
    }
    /// Saved UTF-8 text when it fits the bounded preview.
    #[must_use]
    pub fn source_text(&self) -> Option<&str> {
        self.source_text.as_deref()
    }
    /// Person-selected ordinary destination folder.
    #[must_use]
    pub fn target_root(&self) -> &str {
        &self.target_root
    }
    /// Opaque directory identity that confirmation must reproduce.
    #[must_use]
    pub fn target_installation(&self) -> &str {
        &self.target_installation
    }
    /// Opaque identity of the exact destination parent observed during preview.
    #[must_use]
    pub fn target_parent_installation(&self) -> &str {
        &self.target_parent_installation
    }
    /// Opaque identity of the exact destination file, or `None` when its name was absent.
    #[must_use]
    pub fn target_file_installation(&self) -> Option<&str> {
        self.target_file_installation.as_deref()
    }
    /// Exact destination byte count observed during preview, or `None` when the name was absent.
    #[must_use]
    pub const fn target_byte_count(&self) -> Option<u64> {
        self.target_byte_count
    }
    /// BLAKE3 digest of the exact destination bytes, or `None` when the name was absent.
    #[must_use]
    pub const fn target_content_digest(&self) -> Option<RecordDigest> {
        self.target_content_digest
    }
    /// Destination executable state observed during preview, or `None` when the name was absent.
    #[must_use]
    pub const fn target_executable(&self) -> Option<bool> {
        self.target_executable
    }
    /// Destination UTF-8 text when it fits the bounded preview.
    #[must_use]
    pub fn target_text(&self) -> Option<&str> {
        self.target_text.as_deref()
    }
    /// Whether bytes and portable executable state already agree.
    #[must_use]
    pub fn identical(&self) -> bool {
        self.identical
    }
    /// Proven relationship between the current destination and this workspace's durable history.
    #[must_use]
    pub const fn target_relation(&self) -> &'static str {
        self.target_relation
    }
    /// Whether Mesh may replace this exact existing destination after confirmation.
    #[must_use]
    pub const fn replace_allowed(&self) -> bool {
        self.replace_allowed
    }

    /// Whether confirmation will replace an exact file rather than create an absent name.
    #[must_use]
    pub const fn target_exists(&self) -> bool {
        self.target_content_digest.is_some()
    }
}

/// Result of one exact atomic export into an ordinary folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagedFileExport {
    path: String,
    target_root: String,
    content_digest: RecordDigest,
    byte_count: u64,
    executable: bool,
    created: bool,
}

/// Exact preview of creating one saved directory in a person-selected ordinary folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagedDirectoryExportPreview {
    path: String,
    source_directory_installation: String,
    target_root: String,
    target_installation: String,
    target_parent_installation: String,
    target_directory_installation: Option<String>,
}

/// One whole-workspace directory Pull-back preview bound to an exact ordinary destination root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagedDirectoryExportBatchPreview {
    target_root: String,
    target_installation: String,
    missing_paths: Vec<String>,
}

impl ManagedDirectoryExportBatchPreview {
    pub(crate) fn new(
        target_root: String,
        target_installation: String,
        missing_paths: Vec<String>,
    ) -> Self {
        Self {
            target_root,
            target_installation,
            missing_paths,
        }
    }

    /// Person-selected ordinary destination folder.
    #[must_use]
    pub fn target_root(&self) -> &str {
        &self.target_root
    }

    /// Opaque identity of the exact destination root inspected for the batch.
    #[must_use]
    pub fn target_installation(&self) -> &str {
        &self.target_installation
    }

    /// Saved folders absent from the destination, in parent-before-child order.
    #[must_use]
    pub fn missing_paths(&self) -> &[String] {
        &self.missing_paths
    }
}

impl ManagedDirectoryExportPreview {
    pub(crate) fn new(
        path: String,
        source_directory_installation: String,
        target_root: String,
        target_installation: String,
        target_parent_installation: String,
        target_directory_installation: Option<String>,
    ) -> Self {
        Self {
            path,
            source_directory_installation,
            target_root,
            target_installation,
            target_parent_installation,
            target_directory_installation,
        }
    }

    /// Root-relative saved directory.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
    /// Opaque identity of the managed directory inspected during preview.
    #[must_use]
    pub fn source_directory_installation(&self) -> &str {
        &self.source_directory_installation
    }
    /// Person-selected ordinary destination folder.
    #[must_use]
    pub fn target_root(&self) -> &str {
        &self.target_root
    }
    /// Opaque identity of the exact destination root.
    #[must_use]
    pub fn target_installation(&self) -> &str {
        &self.target_installation
    }
    /// Opaque identity of the exact destination parent.
    #[must_use]
    pub fn target_parent_installation(&self) -> &str {
        &self.target_parent_installation
    }
    /// Opaque identity of an existing destination directory, if present.
    #[must_use]
    pub fn target_directory_installation(&self) -> Option<&str> {
        self.target_directory_installation.as_deref()
    }
    /// Whether the structural folder already exists at the destination.
    #[must_use]
    pub const fn target_exists(&self) -> bool {
        self.target_directory_installation.is_some()
    }
}

/// Result of one exact create-only directory export into an ordinary folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagedDirectoryExport {
    path: String,
    target_root: String,
    installation: String,
    created: bool,
}

/// Exact preview of one formerly saved path in an ordinary export folder.
///
/// `removable` is true only when the retired object came from the workspace's sole genesis import
/// and an existing file still equals its last durable bytes and executable state, or an existing
/// directory has the exact identity inspected here and is empty. Confirmation rechecks every
/// fact. Missing paths are harmless no-ops. Private-only paths remain preserved unless the exact
/// ordinary entry has a durable Pull-back installation receipt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagedRetiredExportPreview {
    path: String,
    entry_type: &'static str,
    source_version: Option<String>,
    source_content_digest: Option<RecordDigest>,
    source_executable: Option<bool>,
    target_root: String,
    target_installation: String,
    target_parent_installation: String,
    target_entry_installation: Option<String>,
    target_content_digest: Option<RecordDigest>,
    target_executable: Option<bool>,
    removable: bool,
    status: &'static str,
}

impl ManagedRetiredExportPreview {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        path: String,
        entry_type: &'static str,
        source_version: Option<String>,
        source_content_digest: Option<RecordDigest>,
        source_executable: Option<bool>,
        target_root: String,
        target_installation: String,
        target_parent_installation: String,
        target_entry_installation: Option<String>,
        target_content_digest: Option<RecordDigest>,
        target_executable: Option<bool>,
        removable: bool,
        status: &'static str,
    ) -> Self {
        Self {
            path,
            entry_type,
            source_version,
            source_content_digest,
            source_executable,
            target_root,
            target_installation,
            target_parent_installation,
            target_entry_installation,
            target_content_digest,
            target_executable,
            removable,
            status,
        }
    }

    /// Former root-relative path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
    /// `file` or `folder`.
    #[must_use]
    pub const fn entry_type(&self) -> &'static str {
        self.entry_type
    }
    /// Last durable file version, when this is a file.
    #[must_use]
    pub fn source_version(&self) -> Option<&str> {
        self.source_version.as_deref()
    }
    /// Digest of the last durable file bytes.
    #[must_use]
    pub const fn source_content_digest(&self) -> Option<RecordDigest> {
        self.source_content_digest
    }
    /// Portable executable state of the last durable file.
    #[must_use]
    pub const fn source_executable(&self) -> Option<bool> {
        self.source_executable
    }
    /// Ordinary folder selected for pull-back.
    #[must_use]
    pub fn target_root(&self) -> &str {
        &self.target_root
    }
    /// Exact identity of the selected ordinary root.
    #[must_use]
    pub fn target_installation(&self) -> &str {
        &self.target_installation
    }
    /// Exact identity of the destination parent.
    #[must_use]
    pub fn target_parent_installation(&self) -> &str {
        &self.target_parent_installation
    }
    /// Exact identity of the destination entry, when present.
    #[must_use]
    pub fn target_entry_installation(&self) -> Option<&str> {
        self.target_entry_installation.as_deref()
    }
    /// Exact destination file digest, when present and regular.
    #[must_use]
    pub const fn target_content_digest(&self) -> Option<RecordDigest> {
        self.target_content_digest
    }
    /// Destination executable state, when present and regular.
    #[must_use]
    pub const fn target_executable(&self) -> Option<bool> {
        self.target_executable
    }
    /// Whether this exact preview can be confirmed for removal.
    #[must_use]
    pub const fn removable(&self) -> bool {
        self.removable
    }
    /// Stable reason describing removal eligibility.
    #[must_use]
    pub const fn status(&self) -> &'static str {
        self.status
    }
}

/// Result of removing one exactly revalidated stale export path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagedRetiredExportRemoval {
    path: String,
    entry_type: &'static str,
    target_root: String,
}

impl ManagedRetiredExportRemoval {
    pub(crate) fn new(path: String, entry_type: &'static str, target_root: String) -> Self {
        Self {
            path,
            entry_type,
            target_root,
        }
    }
    /// Former root-relative path that was removed.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
    /// `file` or `folder`.
    #[must_use]
    pub const fn entry_type(&self) -> &'static str {
        self.entry_type
    }
    /// Ordinary folder from which the stale path was removed.
    #[must_use]
    pub fn target_root(&self) -> &str {
        &self.target_root
    }
}

impl ManagedDirectoryExport {
    pub(crate) fn new(
        path: String,
        target_root: String,
        installation: String,
        created: bool,
    ) -> Self {
        Self {
            path,
            target_root,
            installation,
            created,
        }
    }
    /// Root-relative folder confirmed at the destination.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
    /// Ordinary folder that received the saved folder.
    #[must_use]
    pub fn target_root(&self) -> &str {
        &self.target_root
    }
    /// Opaque identity verified after confirmation.
    #[must_use]
    pub fn installation(&self) -> &str {
        &self.installation
    }
    /// Whether this confirmation created the directory.
    #[must_use]
    pub const fn created(&self) -> bool {
        self.created
    }
}

impl ManagedFileExport {
    pub(crate) fn new(
        path: String,
        target_root: String,
        bytes: &[u8],
        executable: bool,
        created: bool,
    ) -> Self {
        Self {
            path,
            target_root,
            content_digest: RecordDigest::from_bytes(
                *mesh_types::Blake3::digest_bytes(bytes).as_bytes(),
            ),
            byte_count: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            executable,
            created,
        }
    }

    /// Root-relative file copied out.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
    /// Ordinary folder that received the file.
    #[must_use]
    pub fn target_root(&self) -> &str {
        &self.target_root
    }
    /// Digest of the bytes verified after creation or replacement.
    #[must_use]
    pub const fn content_digest(&self) -> RecordDigest {
        self.content_digest
    }
    /// Exact byte count verified after creation or replacement.
    #[must_use]
    pub const fn byte_count(&self) -> u64 {
        self.byte_count
    }
    /// Portable executable state verified after creation or replacement.
    #[must_use]
    pub const fn executable(&self) -> bool {
        self.executable
    }
    /// Whether export created an absent file rather than replacing an existing one.
    #[must_use]
    pub const fn created(&self) -> bool {
        self.created
    }
}

fn bounded_text(bytes: &[u8]) -> Option<String> {
    (bytes.len() <= MAX_MANAGED_TEXT_BYTES)
        .then(|| std::str::from_utf8(bytes).ok().map(str::to_owned))
        .flatten()
}

impl NativeFileInspection {
    pub(crate) fn from_owned_bytes(path: String, bytes: Vec<u8>, executable: bool) -> Self {
        let byte_count = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        let content_digest =
            RecordDigest::from_bytes(*mesh_types::Blake3::digest_bytes(&bytes).as_bytes());
        let text = bounded_text(&bytes);
        Self {
            path,
            byte_count,
            content_digest,
            executable,
            text,
            bytes,
        }
    }

    /// Root-relative native path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Exact byte count read through the confined native boundary.
    #[must_use]
    pub const fn byte_count(&self) -> u64 {
        self.byte_count
    }

    /// BLAKE3 digest of the exact inspected bytes.
    #[must_use]
    pub const fn content_digest(&self) -> RecordDigest {
        self.content_digest
    }

    /// Whether the native file carried any executable bit.
    #[must_use]
    pub const fn executable(&self) -> bool {
        self.executable
    }

    /// UTF-8 text when the file fits the bounded editor display.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    /// Exact confined bytes retained for a native-host-only inert preview.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(crate) fn without_text(mut self) -> Self {
        self.text = None;
        self.bytes = Vec::new();
        self
    }

    #[cfg(test)]
    pub(crate) fn retained_preview_capacity(&self) -> usize {
        self.bytes.capacity()
    }
}

impl ManagedFileInspection {
    /// Root-relative managed path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Current durable version named by private history.
    #[must_use]
    pub fn current_version(&self) -> &str {
        &self.current_version
    }

    /// Exact number of bytes read from the managed operating-system file.
    #[must_use]
    pub const fn byte_count(&self) -> u64 {
        self.byte_count
    }

    /// BLAKE3 digest of the exact bytes read from the operating-system file.
    #[must_use]
    pub const fn content_digest(&self) -> RecordDigest {
        self.content_digest
    }

    /// Whether the inspected operating-system file was executable.
    #[must_use]
    pub const fn executable(&self) -> bool {
        self.executable
    }

    /// UTF-8 text when the file fits the bounded editor, otherwise `None`.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    /// Exact confined bytes retained for a native-host-only inert preview.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Whether the operating-system bytes or portable executable metadata differ from the current
    /// durable version.
    #[must_use]
    pub const fn modified_from_current_version(&self) -> bool {
        self.modified_from_current_version
    }

    pub(crate) fn without_text(mut self) -> Self {
        self.text = None;
        self.bytes = Vec::new();
        self
    }

    #[cfg(test)]
    pub(crate) fn retained_preview_capacity(&self) -> usize {
        self.bytes.capacity()
    }
}

impl ManagedTextFile {
    /// Root-relative managed path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Exact UTF-8 contents read from the operating-system file.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Durable version that named the file when it was opened.
    #[must_use]
    pub fn current_version(&self) -> &str {
        &self.current_version
    }

    /// Whether the operating-system bytes differ from the current durable version.
    ///
    /// This is derived from the exact BLAKE3 content digest after the confined read, so the
    /// desktop can offer an authenticated private save for work made in an ordinary local editor
    /// without first rewriting those bytes through the webview.
    #[must_use]
    pub const fn modified_from_current_version(&self) -> bool {
        self.modified_from_current_version
    }
}

/// Durable identities returned after one native desktop save.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagedTextSave {
    path: String,
    recovery: RecordDigest,
    content_digest: RecordDigest,
    stable_after_idle: bool,
}

/// One authenticated file version durably committed to the local private workspace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagedPrivateSave {
    path: String,
    version: String,
    manifest: String,
    changeset: String,
    stable_after_idle: bool,
    meaningful_saved: bool,
    author_authenticated: bool,
}

/// One authenticated create, rename/move, or delete applied to the managed OS folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagedEntryChange {
    action: &'static str,
    from_path: Option<String>,
    to_path: Option<String>,
    changeset: String,
    meaningful_saved: bool,
    author_authenticated: bool,
}

impl ManagedEntryChange {
    pub(crate) fn new(
        action: &'static str,
        from_path: Option<String>,
        to_path: Option<String>,
        changeset: String,
        meaningful_saved: bool,
    ) -> Self {
        Self {
            action,
            from_path,
            to_path,
            changeset,
            meaningful_saved,
            author_authenticated: true,
        }
    }

    /// Stable machine action name.
    #[must_use]
    pub const fn action(&self) -> &'static str {
        self.action
    }

    /// Original path for move and delete operations.
    #[must_use]
    pub fn from_path(&self) -> Option<&str> {
        self.from_path.as_deref()
    }

    /// Resulting path for create and move operations.
    #[must_use]
    pub fn to_path(&self) -> Option<&str> {
        self.to_path.as_deref()
    }

    /// Canonical authenticated ChangeSet envelope identifier.
    #[must_use]
    pub fn changeset(&self) -> &str {
        &self.changeset
    }

    /// Whether the real durable acknowledgement advanced the meaningful checkpoint.
    #[must_use]
    pub const fn meaningful_saved(&self) -> bool {
        self.meaningful_saved
    }

    /// Whether the persisted actor signature was verified before append.
    #[must_use]
    pub const fn author_authenticated(&self) -> bool {
        self.author_authenticated
    }
}

impl ManagedPrivateSave {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        path: String,
        version: String,
        manifest: String,
        changeset: String,
        stable_after_idle: bool,
        meaningful_saved: bool,
        author_authenticated: bool,
    ) -> Self {
        Self {
            path,
            version,
            manifest,
            changeset,
            stable_after_idle,
            meaningful_saved,
            author_authenticated,
        }
    }

    /// Root-relative managed path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
    /// Newly retained immutable file version.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }
    /// Canonical file-manifest identifier.
    #[must_use]
    pub fn manifest(&self) -> &str {
        &self.manifest
    }
    /// Canonical authenticated ChangeSet envelope identifier.
    #[must_use]
    pub fn changeset(&self) -> &str {
        &self.changeset
    }
    /// Whether bytes remained unchanged during the measured settling interval.
    #[must_use]
    pub const fn stable_after_idle(&self) -> bool {
        self.stable_after_idle
    }
    /// Whether the real durable acknowledgement advanced the meaningful checkpoint.
    #[must_use]
    pub const fn meaningful_saved(&self) -> bool {
        self.meaningful_saved
    }
    /// Whether the persisted ChangeSet signature was verified before append.
    #[must_use]
    pub const fn author_authenticated(&self) -> bool {
        self.author_authenticated
    }
}

/// One retained immutable version restored into the managed operating-system working copy.
///
/// The durable history is not rewritten. The returned recovery identity names the new Working
/// state, while `target_version` says which retained bytes were hydrated from CAS.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagedVersionRestore {
    path: String,
    target_version: String,
    content_digest: RecordDigest,
    executable: bool,
    recovery: RecordDigest,
    stable_after_idle: bool,
}

impl ManagedVersionRestore {
    pub(crate) fn new(
        path: String,
        target_version: String,
        content_digest: RecordDigest,
        executable: bool,
        recovery: RecordDigest,
        stable_after_idle: bool,
    ) -> Self {
        Self {
            path,
            target_version,
            content_digest,
            executable,
            recovery,
            stable_after_idle,
        }
    }

    /// Root-relative managed path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Retained immutable version whose bytes were restored.
    #[must_use]
    pub fn target_version(&self) -> &str {
        &self.target_version
    }

    /// Digest of the exact retained bytes installed into the working copy.
    #[must_use]
    pub const fn content_digest(&self) -> RecordDigest {
        self.content_digest
    }

    /// Whether the restored retained version is executable.
    #[must_use]
    pub const fn executable(&self) -> bool {
        self.executable
    }

    /// Digest of the verified recovery envelope.
    #[must_use]
    pub const fn recovery(&self) -> RecordDigest {
        self.recovery
    }

    /// Whether the operating-system bytes were unchanged after the measured idle interval.
    #[must_use]
    pub const fn stable_after_idle(&self) -> bool {
        self.stable_after_idle
    }
}

impl ManagedTextSave {
    pub(crate) fn new(
        path: String,
        recovery: RecordDigest,
        content_digest: RecordDigest,
        stable_after_idle: bool,
    ) -> Self {
        Self {
            path,
            recovery,
            content_digest,
            stable_after_idle,
        }
    }

    /// Root-relative managed path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Digest of the verified recovery envelope.
    #[must_use]
    pub const fn recovery(&self) -> RecordDigest {
        self.recovery
    }

    /// Digest of the exact replacement bytes preserved for recovery.
    ///
    /// [`Self::stable_after_idle`] separately states whether those bytes still occupied the
    /// working copy after the settling interval.
    #[must_use]
    pub const fn content_digest(&self) -> RecordDigest {
        self.content_digest
    }

    /// Whether the operating-system bytes were unchanged after the measured idle interval.
    #[must_use]
    pub const fn stable_after_idle(&self) -> bool {
        self.stable_after_idle
    }
}

/// Why a desktop-managed text read or save was refused.
#[derive(Debug)]
pub enum ManagedTextFileError {
    /// No workspace is open in the desktop-owned daemon.
    NoWorkspace,
    /// The open workspace no longer matches the exact state the desktop displayed.
    StaleWorkspace,
    /// The path was absolute, empty, or contained a traversal component.
    InvalidPath,
    /// The path belongs to Mesh private workspace state.
    ReservedPath,
    /// Durable history or the filesystem does not identify a regular file here.
    NotRegularFile,
    /// The requested object or retained version is not in complete durable history.
    UnknownRetainedVersion,
    /// The retained manifest or one of its content-addressed chunks could not be reconstructed.
    RetainedContent(String),
    /// The text exceeded the bounded desktop editor size.
    TooLarge {
        /// Exact byte count that exceeded the bound.
        bytes: usize,
    },
    /// The file exceeded the native inert-preview byte ceiling before an unbounded allocation.
    PreviewTooLarge {
        /// Observed byte count, or the first byte count beyond the ceiling during a racing write.
        bytes: usize,
        /// Exact native-preview byte ceiling applied to the confined read.
        limit: usize,
    },
    /// The selected file was not UTF-8 text.
    NotUtf8,
    /// The replacement bytes equal the file already on disk.
    Unchanged,
    /// The operating-system bytes changed after the caller inspected them.
    StaleInspection,
    /// The native working copy differs from the saved version selected for export.
    UnsavedWorkingCopy,
    /// The person-selected export folder aliases or nests the managed workspace.
    UnsafeExportTarget,
    /// The workspace's original project may receive only the exact current shared version.
    OriginalExportRequiresSharedVersion,
    /// The export destination changed after the preview.
    StaleExportTarget,
    /// The existing destination is not proven to be the imported baseline or a prior Pull-back
    /// from this exact workspace.
    UnprovenExportReplacement,
    /// A create or move target already exists on disk or in durable history.
    TargetExists,
    /// A folder deletion was requested while the folder still contained entries.
    DirectoryNotEmpty,
    /// A filesystem operation failed.
    Io {
        /// Filesystem operation name.
        operation: &'static str,
        /// Exact path the operation targeted.
        path: PathBuf,
        /// Operating-system refusal.
        source: io::Error,
    },
    /// Recovery failed and restoring the prior operating-system bytes also failed.
    Rollback {
        /// Original checkpoint refusal.
        checkpoint: String,
        /// Failure restoring the prior bytes.
        rollback: io::Error,
    },
    /// The durable recovery runtime refused the edit.
    Checkpoint(String),
    /// Authenticated local authoring or durable version storage was refused.
    Authoring(String),
    /// An interrupted native mutation could not be safely created, cleared, or reconciled.
    Recovery(String),
}

impl ManagedTextFileError {
    pub(crate) fn io(operation: &'static str, path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            operation,
            path: path.into(),
            source,
        }
    }
}

impl fmt::Display for ManagedTextFileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoWorkspace => formatter.write_str("no managed workspace is open"),
            Self::StaleWorkspace => formatter.write_str(
                "the verified managed workspace changed before the operation began; refresh and review the current workspace",
            ),
            Self::InvalidPath => formatter.write_str("the managed file path is not a confined relative path"),
            Self::ReservedPath => formatter.write_str("the selected path belongs to Mesh private workspace state"),
            Self::NotRegularFile => formatter.write_str("the selected managed path is not a regular file"),
            Self::UnknownRetainedVersion => formatter.write_str("the selected retained file version is not in complete durable history"),
            Self::RetainedContent(error) => write!(formatter, "the retained file version could not be reconstructed exactly: {error}"),
            Self::TooLarge { bytes } => write!(formatter, "the selected text file is {bytes} bytes; the desktop limit is {MAX_MANAGED_TEXT_BYTES}"),
            Self::PreviewTooLarge { bytes, limit } => write!(
                formatter,
                "the selected live file is {bytes} bytes; the inert-preview limit is {limit} bytes"
            ),
            Self::NotUtf8 => formatter.write_str("the selected file is not UTF-8 text"),
            Self::Unchanged => formatter.write_str("the file already contains these exact bytes"),
            Self::StaleInspection => formatter.write_str(
                "the managed file changed after it was inspected; inspect it again before continuing",
            ),
            Self::UnsavedWorkingCopy => formatter.write_str(
                "the native working file differs from private history; save it privately before exporting",
            ),
            Self::UnsafeExportTarget => formatter.write_str(
                "the export folder must be an independent ordinary folder outside the managed workspace",
            ),
            Self::OriginalExportRequiresSharedVersion => formatter.write_str(
                "the original project can be updated only from the exact current version approved as shared",
            ),
            Self::StaleExportTarget => formatter.write_str(
                "the export destination or its parent changed after preview; preview it again before exporting",
            ),
            Self::UnprovenExportReplacement => formatter.write_str(
                "the ordinary-folder file changed outside this workspace; Mesh kept it for manual review",
            ),
            Self::TargetExists => formatter.write_str("the managed target path already exists"),
            Self::DirectoryNotEmpty => formatter.write_str("the managed folder is not empty"),
            Self::Io { operation, path, source } => write!(formatter, "{operation} {} failed: {source}", path.display()),
            Self::Rollback { checkpoint, rollback } => write!(formatter, "recovery preservation was refused ({checkpoint}) and restoring the prior file bytes failed: {rollback}"),
            Self::Checkpoint(error) => write!(formatter, "the managed edit checkpoint was refused: {error}"),
            Self::Authoring(error) => write!(formatter, "the private save was refused: {error}"),
            Self::Recovery(error) => write!(formatter, "local file recovery was refused: {error}"),
        }
    }
}

pub(crate) fn confined_free_path(
    root: &Path,
    relative: &str,
) -> Result<PathBuf, ManagedTextFileError> {
    let parent = open_confined_parent(root, relative, false)?;
    match open_read_at(&parent.directory, &parent.name) {
        Ok(_) => Err(ManagedTextFileError::TargetExists),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(parent.path),
        Err(error) if error.raw_os_error() == Some(CONFINED_LOOP_ERROR) => {
            Err(ManagedTextFileError::TargetExists)
        }
        Err(error) => Err(ManagedTextFileError::io("open", &parent.path, error)),
    }
}

pub(crate) fn confined_existing_entry(
    root: &Path,
    relative: &str,
    is_directory: bool,
) -> Result<PathBuf, ManagedTextFileError> {
    let parent = open_confined_parent(root, relative, false)?;
    let entry = open_entry_at(&parent.directory, &parent.name, is_directory)
        .map_err(|error| confined_open_error(&parent.path, error))?;
    let metadata = entry
        .metadata()
        .map_err(|error| ManagedTextFileError::io("metadata", &parent.path, error))?;
    if (is_directory && !metadata.is_dir()) || (!is_directory && !metadata.is_file()) {
        return Err(ManagedTextFileError::NotRegularFile);
    }
    Ok(parent.path)
}

/// Open one confined directory without following links and return its exact OS identity.
pub(crate) fn inspect_managed_directory(
    root: &Path,
    relative: &str,
) -> Result<(PathBuf, ManagedDirectoryIdentity), ManagedTextFileError> {
    let parent = open_confined_parent(root, relative, false)?;
    let directory = open_entry_at(&parent.directory, &parent.name, true)
        .map_err(|error| confined_open_error(&parent.path, error))?;
    let metadata = directory
        .metadata()
        .map_err(|error| ManagedTextFileError::io("metadata", &parent.path, error))?;
    if !metadata.is_dir() {
        return Err(ManagedTextFileError::NotRegularFile);
    }
    let identity = ManagedDirectoryIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    };
    Ok((parent.path, identity))
}

#[cfg(test)]
pub(crate) fn create_text_file(root: &Path, relative: &str, bytes: &[u8]) -> io::Result<()> {
    let parent = open_confined_parent(root, relative, false).map_err(io::Error::other)?;
    let temporary_name = format!(
        ".{}.mesh-create-{}-{}",
        parent.name.to_string_lossy(),
        std::process::id(),
        TEMPORARY_COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let result = (|| {
        let mut file = create_new_at(&parent.directory, &temporary_name)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        if !entry_absent_at(&parent.directory, &parent.name)? {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "target exists",
            ));
        }
        run_before_rename_hook();
        atomic_rename_noreplace_at(
            &parent.directory,
            std::ffi::OsStr::new(&temporary_name),
            &parent.directory,
            &parent.name,
        )?;
        parent.directory.sync_all()
    })();
    if result.is_err() {
        let _ = unlink_at(&parent.directory, &temporary_name);
    }
    result
}

#[cfg(test)]
pub(crate) fn create_managed_directory(root: &Path, relative: &str) -> io::Result<()> {
    let parent = open_confined_parent(root, relative, false).map_err(io::Error::other)?;
    let temporary_name = format!(
        ".{}.mesh-create-{}-{}",
        parent.name.to_string_lossy(),
        std::process::id(),
        TEMPORARY_COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let result = (|| {
        mkdir_at(&parent.directory, &temporary_name)?;
        if !entry_absent_at(&parent.directory, &parent.name)? {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "target exists",
            ));
        }
        run_before_rename_hook();
        atomic_rename_noreplace_at(
            &parent.directory,
            std::ffi::OsStr::new(&temporary_name),
            &parent.directory,
            &parent.name,
        )?;
        parent.directory.sync_all()
    })();
    if result.is_err() {
        let _ = unlink_directory_at(&parent.directory, &temporary_name);
    }
    result
}

/// Capture the exact non-symlink directory a managed operation is allowed to mutate.
#[cfg(unix)]
pub(crate) fn managed_directory_identity(path: &Path) -> io::Result<ManagedDirectoryIdentity> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(io::Error::other("managed parent is not a real directory"));
    }
    Ok(ManagedDirectoryIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(not(unix))]
pub(crate) fn managed_directory_identity(_path: &Path) -> io::Result<ManagedDirectoryIdentity> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "managed directory identity requires Unix file identity",
    ))
}

/// Create one file through the exact parent directory admitted by the mutation intent.
pub(crate) fn create_text_file_in_exact_parent(
    path: &Path,
    bytes: &[u8],
    expected_parent: ManagedDirectoryIdentity,
) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("managed file has no parent"))?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("managed file has no name"))?;
    let directory = open_exact_directory(parent, expected_parent)?;
    let temporary_name = format!(
        ".{}.mesh-create-{}-{}",
        name.to_string_lossy(),
        std::process::id(),
        TEMPORARY_COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let result = (|| {
        let mut file = create_new_at(&directory, &temporary_name)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        atomic_rename_noreplace_at(
            &directory,
            std::ffi::OsStr::new(&temporary_name),
            &directory,
            name,
        )?;
        directory.sync_all()
    })();
    if result.is_err() {
        let _ = unlink_at(&directory, &temporary_name);
    }
    result
}

/// Create one directory through the exact parent admitted by the mutation intent.
pub(crate) fn create_managed_directory_in_exact_parent(
    path: &Path,
    expected_parent: ManagedDirectoryIdentity,
) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("managed directory has no parent"))?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("managed directory has no name"))?;
    let directory = open_exact_directory(parent, expected_parent)?;
    let name = name
        .to_str()
        .ok_or_else(|| io::Error::other("managed directory name is not UTF-8"))?;
    mkdir_at(&directory, name)?;
    directory.sync_all()
}

/// Create one export directory only after its exact future identity has been durably prepared.
///
/// The callback runs after the temporary directory exists but before its create-only rename. A
/// caller can therefore persist crash-recoverable provenance for the exact inode without exposing
/// an unproven destination entry if that persistence fails.
pub(crate) fn create_export_directory_in_exact_parent_prepared<F>(
    path: &Path,
    expected_parent: ManagedDirectoryIdentity,
    prepare_installation: F,
) -> io::Result<ManagedDirectoryIdentity>
where
    F: FnOnce(ManagedDirectoryIdentity) -> io::Result<()>,
{
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("export directory has no parent"))?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("export directory has no name"))?;
    let directory = open_exact_directory(parent, expected_parent)?;
    let temporary_name = format!(
        ".{}.mesh-export-{}-{}",
        name.to_string_lossy(),
        std::process::id(),
        TEMPORARY_COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let result = (|| {
        mkdir_at(&directory, &temporary_name)?;
        let temporary = open_entry_at(&directory, std::ffi::OsStr::new(&temporary_name), true)?;
        let metadata = temporary.metadata()?;
        let identity = ManagedDirectoryIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        };
        prepare_installation(identity)?;
        atomic_rename_noreplace_at(
            &directory,
            std::ffi::OsStr::new(&temporary_name),
            &directory,
            name,
        )?;
        directory.sync_all()?;
        Ok(identity)
    })();
    if result.is_err() {
        let _ = unlink_directory_at(&directory, &temporary_name);
    }
    result
}

/// Finish a create-only directory export whose durable receipt preceded process loss.
///
/// Recovery publishes only one exact receipt-bound inode that is still empty. Receipt-owned
/// directories with unexpected contents refuse and remain untouched; unproven lookalikes are not
/// enumerated or removed.
pub(crate) fn recover_prepared_export_directory_in_exact_parent<F>(
    path: &Path,
    expected_parent: ManagedDirectoryIdentity,
    proves_installation: F,
) -> io::Result<Option<ManagedDirectoryIdentity>>
where
    F: Fn(ManagedDirectoryIdentity) -> bool,
{
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("export directory has no parent"))?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("export directory has no name"))?;
    let directory = open_exact_directory(parent, expected_parent)?;
    let prefix = format!(".{}.mesh-export-", name.to_string_lossy());
    let mut proven = Vec::new();

    for entry in fs::read_dir(parent)? {
        let candidate = entry?.file_name();
        if !candidate.to_string_lossy().starts_with(&prefix) {
            continue;
        }
        let Ok(opened) = open_entry_at(&directory, &candidate, true) else {
            continue;
        };
        let Ok(metadata) = opened.metadata() else {
            continue;
        };
        let identity = ManagedDirectoryIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        };
        if !metadata.is_dir() || !proves_installation(identity) {
            continue;
        }
        if fs::read_dir(parent.join(&candidate))?
            .next()
            .transpose()?
            .is_some()
        {
            return Err(io::Error::other(
                "prepared export directory contains unexpected entries",
            ));
        }
        proven.push((candidate, identity));
    }

    match proven.as_slice() {
        [] => Ok(None),
        [candidate] => {
            let _ = open_exact_directory(parent, expected_parent)?;
            let opened = open_entry_at(&directory, &candidate.0, true)?;
            let metadata = opened.metadata()?;
            let identity = ManagedDirectoryIdentity {
                device: metadata.dev(),
                inode: metadata.ino(),
            };
            if !metadata.is_dir()
                || identity != candidate.1
                || !proves_installation(candidate.1)
                || fs::read_dir(parent.join(&candidate.0))?
                    .next()
                    .transpose()?
                    .is_some()
            {
                return Err(io::Error::other(
                    "prepared export directory changed before recovery",
                ));
            }
            let rechecked = open_entry_at(&directory, &candidate.0, true)?;
            let rechecked_metadata = rechecked.metadata()?;
            if rechecked_metadata.dev() != candidate.1.device
                || rechecked_metadata.ino() != candidate.1.inode
            {
                return Err(io::Error::other(
                    "prepared export directory changed before recovery",
                ));
            }
            atomic_rename_noreplace_at(&directory, &candidate.0, &directory, name)?;
            directory.sync_all()?;
            Ok(Some(candidate.1))
        }
        _ => Err(io::Error::other(
            "multiple durable prepared exports match the same directory",
        )),
    }
}

fn restore_quarantined_entry(
    directory: &File,
    hidden_name: &str,
    original_name: &std::ffi::OsStr,
) -> io::Result<()> {
    atomic_rename_noreplace_at(
        directory,
        std::ffi::OsStr::new(hidden_name),
        directory,
        original_name,
    )?;
    directory.sync_all()
}

/// Remove one exact ordinary-folder file without ever unlinking an unverified replacement.
///
/// The name is first moved atomically to a private sibling. Only the moved inode is then verified
/// and unlinked. If another writer won the race, the moved entry is restored (or retained under
/// its explicit sibling name when the original name was concurrently reused).
pub(crate) fn remove_exact_export_file(
    target: &ManagedReplacementTarget,
    expected_bytes: &[u8],
) -> io::Result<()> {
    let (path, directory, name, metadata, identity, bytes) = read_confined_regular_file_in_layout(
        &target.root,
        &target.relative,
        target.reserve_private_top_level,
    )
    .map_err(|error| io::Error::other(format!("export cleanup confinement failed: {error}")))?;
    let parent_metadata = directory.metadata()?;
    if path != target.path
        || (ManagedDirectoryIdentity {
            device: parent_metadata.dev(),
            inode: parent_metadata.ino(),
        }) != target.parent
        || identity != target.file
        || metadata.mode() != target.mode
        || bytes != expected_bytes
    {
        return Err(io::Error::other(
            "export cleanup file changed after preview",
        ));
    }
    let hidden_name = format!(
        ".{}.mesh-remove-{}-{}",
        name.to_string_lossy(),
        std::process::id(),
        TEMPORARY_COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    run_before_rename_hook();
    atomic_rename_noreplace_at(
        &directory,
        &name,
        &directory,
        std::ffi::OsStr::new(&hidden_name),
    )?;
    let moved_matches = open_read_at(&directory, std::ffi::OsStr::new(&hidden_name))
        .ok()
        .is_some_and(|mut moved| {
            let Ok(moved_metadata) = moved.metadata() else {
                return false;
            };
            let mut moved_bytes = Vec::new();
            moved_metadata.is_file()
                && managed_file_identity(&moved, &moved_metadata).ok() == Some(target.file)
                && moved_metadata.mode() == target.mode
                && moved.read_to_end(&mut moved_bytes).is_ok()
                && moved_bytes == expected_bytes
        });
    if !moved_matches {
        restore_quarantined_entry(&directory, &hidden_name, &name)?;
        return Err(io::Error::other("export cleanup file changed at removal"));
    }
    if let Err(error) = directory.sync_all() {
        restore_quarantined_entry(&directory, &hidden_name, &name)?;
        return Err(error);
    }
    if let Err(error) = unlink_at(&directory, &hidden_name) {
        restore_quarantined_entry(&directory, &hidden_name, &name)?;
        return Err(error);
    }
    directory.sync_all()
}

/// Remove one exact empty ordinary-folder directory without recursive deletion.
pub(crate) fn remove_exact_export_directory(
    root: &Path,
    relative: &str,
    expected_parent: ManagedDirectoryIdentity,
    expected_directory: ManagedDirectoryIdentity,
) -> io::Result<()> {
    let confined =
        open_confined_parent_in_layout(root, relative, false, false).map_err(io::Error::other)?;
    let parent_metadata = confined.directory.metadata()?;
    let parent_identity = ManagedDirectoryIdentity {
        device: parent_metadata.dev(),
        inode: parent_metadata.ino(),
    };
    if parent_identity != expected_parent {
        return Err(io::Error::other(
            "export cleanup parent changed after preview",
        ));
    }
    let directory = open_entry_at(&confined.directory, &confined.name, true)?;
    let metadata = directory.metadata()?;
    let directory_identity = ManagedDirectoryIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    };
    if !metadata.is_dir() || directory_identity != expected_directory {
        return Err(io::Error::other(
            "export cleanup directory changed after preview",
        ));
    }
    let hidden_name = format!(
        ".{}.mesh-remove-{}-{}",
        confined.name.to_string_lossy(),
        std::process::id(),
        TEMPORARY_COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    run_before_rename_hook();
    atomic_rename_noreplace_at(
        &confined.directory,
        &confined.name,
        &confined.directory,
        std::ffi::OsStr::new(&hidden_name),
    )?;
    let moved_matches = open_entry_at(
        &confined.directory,
        std::ffi::OsStr::new(&hidden_name),
        true,
    )
    .ok()
    .and_then(|entry| entry.metadata().ok())
    .is_some_and(|moved| {
        moved.is_dir()
            && moved.dev() == expected_directory.device
            && moved.ino() == expected_directory.inode
    });
    if !moved_matches {
        restore_quarantined_entry(&confined.directory, &hidden_name, &confined.name)?;
        return Err(io::Error::other(
            "export cleanup directory changed at removal",
        ));
    }
    if let Err(error) = unlink_directory_at(&confined.directory, &hidden_name) {
        restore_quarantined_entry(&confined.directory, &hidden_name, &confined.name)?;
        return Err(error);
    }
    confined.directory.sync_all()
}

/// Rename one entry through the exact source and destination parents admitted by the caller.
pub(crate) fn atomic_rename_noreplace_in_exact_parents(
    from: &Path,
    to: &Path,
    expected_from_parent: ManagedDirectoryIdentity,
    expected_to_parent: ManagedDirectoryIdentity,
) -> io::Result<()> {
    let from_parent = from
        .parent()
        .ok_or_else(|| io::Error::other("managed source has no parent"))?;
    let to_parent = to
        .parent()
        .ok_or_else(|| io::Error::other("managed destination has no parent"))?;
    let from_name = from
        .file_name()
        .ok_or_else(|| io::Error::other("managed source has no name"))?;
    let to_name = to
        .file_name()
        .ok_or_else(|| io::Error::other("managed destination has no name"))?;
    let from_directory = open_exact_directory(from_parent, expected_from_parent)?;
    let to_directory = if expected_to_parent == expected_from_parent {
        from_directory.try_clone()?
    } else {
        open_exact_directory(to_parent, expected_to_parent)?
    };
    atomic_rename_noreplace_at(&from_directory, from_name, &to_directory, to_name)?;
    from_directory.sync_all()?;
    if expected_to_parent != expected_from_parent {
        to_directory.sync_all()?;
    }
    Ok(())
}

fn open_exact_directory(path: &Path, expected: ManagedDirectoryIdentity) -> io::Result<File> {
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(OPEN_DIRECTORY_FLAGS)
        .open(path)?;
    let metadata = directory.metadata()?;
    if !metadata.is_dir() || metadata.dev() != expected.device || metadata.ino() != expected.inode {
        return Err(io::Error::other(
            "managed parent directory identity changed",
        ));
    }
    Ok(directory)
}

#[cfg(test)]
pub(crate) fn move_managed_entry(
    root: &Path,
    from_relative: &str,
    to_relative: &str,
    expected: ManagedFileIdentity,
    is_directory: bool,
    allow_private_staging: bool,
) -> io::Result<()> {
    let from = open_confined_parent(root, from_relative, allow_private_staging)
        .map_err(io::Error::other)?;
    let to =
        open_confined_parent(root, to_relative, allow_private_staging).map_err(io::Error::other)?;
    let source = open_entry_at(&from.directory, &from.name, is_directory)?;
    let metadata = source.metadata()?;
    if managed_file_identity(&source, &metadata)? != expected {
        return Err(io::Error::other("managed source identity changed"));
    }
    if !entry_absent_at(&to.directory, &to.name)? {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "target exists",
        ));
    }
    run_before_rename_hook();
    atomic_rename_noreplace_at(&from.directory, &from.name, &to.directory, &to.name)?;
    from.directory.sync_all()?;
    if from.path.parent() != to.path.parent() {
        to.directory.sync_all()?;
    }
    Ok(())
}

fn open_confined_parent(
    root: &Path,
    relative: &str,
    allow_private_staging: bool,
) -> Result<ConfinedParent, ManagedTextFileError> {
    let reserve_private_top_level = crate::workspace::presented_workspace_storage_root(root)
        .map_or(true, |root| root.is_none());
    open_confined_parent_in_layout(
        root,
        relative,
        allow_private_staging,
        reserve_private_top_level,
    )
}

fn open_confined_parent_in_layout(
    root: &Path,
    relative: &str,
    allow_private_staging: bool,
    reserve_private_top_level: bool,
) -> Result<ConfinedParent, ManagedTextFileError> {
    let components = managed_components_with_staging(
        relative,
        allow_private_staging,
        reserve_private_top_level,
    )?;
    let mut path = root.to_path_buf();
    let mut directory = OpenOptions::new()
        .read(true)
        .custom_flags(OPEN_DIRECTORY_FLAGS)
        .open(root)
        .map_err(|error| confined_open_error(root, error))?;
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(name) = component else {
            return Err(ManagedTextFileError::InvalidPath);
        };
        path.push(name);
        if index + 1 == components.len() {
            return Ok(ConfinedParent {
                path,
                directory,
                name: name.to_os_string(),
            });
        }
        directory = open_directory_at(&directory, name)
            .map_err(|error| confined_open_error(&path, error))?;
    }
    Err(ManagedTextFileError::InvalidPath)
}

fn open_entry_at(directory: &File, name: &std::ffi::OsStr, is_directory: bool) -> io::Result<File> {
    if is_directory {
        open_directory_at(directory, name)
    } else {
        open_read_at(directory, name)
    }
}

#[cfg(test)]
fn entry_absent_at(directory: &File, name: &std::ffi::OsStr) -> io::Result<bool> {
    match open_read_at(directory, name) {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(true),
        Err(error) if error.raw_os_error() == Some(CONFINED_LOOP_ERROR) => Ok(false),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn confined_entry_identity(
    root: &Path,
    relative: &str,
    is_directory: bool,
    allow_private_staging: bool,
) -> io::Result<ManagedFileIdentity> {
    let entry =
        open_confined_parent(root, relative, allow_private_staging).map_err(io::Error::other)?;
    let opened = open_entry_at(&entry.directory, &entry.name, is_directory)?;
    managed_file_identity(&opened, &opened.metadata()?)
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn confined_entry_absent(
    root: &Path,
    relative: &str,
    allow_private_staging: bool,
) -> io::Result<bool> {
    let entry =
        open_confined_parent(root, relative, allow_private_staging).map_err(io::Error::other)?;
    entry_absent_at(&entry.directory, &entry.name)
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn read_confined_entry_bytes(
    root: &Path,
    relative: &str,
    allow_private_staging: bool,
) -> io::Result<(ManagedFileIdentity, Vec<u8>)> {
    let entry =
        open_confined_parent(root, relative, allow_private_staging).map_err(io::Error::other)?;
    let mut opened = open_entry_at(&entry.directory, &entry.name, false)?;
    let metadata = opened.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::other("managed entry is not a regular file"));
    }
    let identity = managed_file_identity(&opened, &metadata)?;
    let mut bytes = Vec::new();
    opened.read_to_end(&mut bytes)?;
    Ok((identity, bytes))
}

#[cfg(target_os = "macos")]
const OPEN_DIRECTORY_FLAGS: i32 = 0x0110_0100;
#[cfg(target_os = "linux")]
const OPEN_DIRECTORY_FLAGS: i32 = 0x000b_0000;

#[cfg(target_os = "macos")]
const OPEN_FILE_FLAGS: i32 = 0x0100_0104;
#[cfg(target_os = "linux")]
const OPEN_FILE_FLAGS: i32 = 0x000a_0800;

#[cfg(target_os = "macos")]
const CREATE_FILE_FLAGS: i32 = 0x0100_0a01;
#[cfg(target_os = "linux")]
const CREATE_FILE_FLAGS: i32 = 0x0008_00c1;

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[allow(unsafe_code)]
fn open_directory_at(directory: &File, name: &std::ffi::OsStr) -> io::Result<File> {
    unsafe extern "C" {
        fn openat(directory: i32, path: *const std::ffi::c_char, flags: i32, ...) -> i32;
    }
    let name = CString::new(name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name contains a NUL byte"))?;
    // SAFETY: `name` is a live C string and `directory` is an owned verified directory.
    let descriptor = unsafe { openat(directory.as_raw_fd(), name.as_ptr(), OPEN_DIRECTORY_FLAGS) };
    if descriptor < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful `openat` returned one newly owned descriptor.
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[allow(unsafe_code)]
fn open_read_at(directory: &File, name: &std::ffi::OsStr) -> io::Result<File> {
    unsafe extern "C" {
        fn openat(directory: i32, path: *const std::ffi::c_char, flags: i32, ...) -> i32;
    }
    let name = CString::new(name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name contains a NUL byte"))?;
    // SAFETY: `name` is a live C string and `directory` is an owned verified directory.
    let descriptor = unsafe { openat(directory.as_raw_fd(), name.as_ptr(), OPEN_FILE_FLAGS) };
    if descriptor < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful `openat` returned one newly owned descriptor.
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn open_read_at(_directory: &File, _name: &std::ffi::OsStr) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "descriptor-relative managed read requires Unix",
    ))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[allow(unsafe_code)]
fn create_new_at(directory: &File, name: &str) -> io::Result<File> {
    unsafe extern "C" {
        fn openat(directory: i32, path: *const std::ffi::c_char, flags: i32, ...) -> i32;
    }
    let name = CString::new(name)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name contains a NUL byte"))?;
    // SAFETY: `name` is live, `directory` is verified, and the descriptor is newly created.
    let descriptor = unsafe {
        openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            CREATE_FILE_FLAGS,
            0o600_i32,
        )
    };
    if descriptor < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful `openat` returned one newly owned descriptor.
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn mkdir_at(directory: &File, name: &str) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    type NativeMode = u16;
    #[cfg(not(target_os = "macos"))]
    type NativeMode = u32;
    unsafe extern "C" {
        fn mkdirat(directory: i32, path: *const std::ffi::c_char, mode: NativeMode) -> i32;
    }
    let name = CString::new(name)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name contains a NUL byte"))?;
    // SAFETY: `name` is live and `directory` is a verified open directory.
    if unsafe { mkdirat(directory.as_raw_fd(), name.as_ptr(), 0o700 as NativeMode) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(unix))]
fn mkdir_at(_directory: &File, _name: &str) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "descriptor-relative managed directory creation requires Unix",
    ))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn create_new_at(_directory: &File, _name: &str) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "descriptor-relative managed creation requires Unix",
    ))
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn unlink_at(directory: &File, name: &str) -> io::Result<()> {
    unsafe extern "C" {
        fn unlinkat(directory: i32, path: *const std::ffi::c_char, flags: i32) -> i32;
    }
    let name = CString::new(name)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name contains a NUL byte"))?;
    // SAFETY: `name` is live and `directory` is a verified open directory.
    if unsafe { unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn unlink_directory_at(directory: &File, name: &str) -> io::Result<()> {
    // `unlinkat(2)` uses an OS-specific flag value here.  macOS spells
    // `AT_REMOVEDIR` as 0x80 while Linux uses 0x200; sharing the macOS value
    // made Linux treat a failed prepared-directory cleanup like a plain
    // unlink and leave the private sibling behind.
    #[cfg(target_os = "macos")]
    const AT_REMOVEDIR: i32 = 0x80;
    #[cfg(target_os = "linux")]
    const AT_REMOVEDIR: i32 = 0x200;
    unsafe extern "C" {
        fn unlinkat(directory: i32, path: *const std::ffi::c_char, flags: i32) -> i32;
    }
    let name = CString::new(name)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name contains a NUL byte"))?;
    // SAFETY: `name` is live and `directory` is a verified open directory.
    if unsafe { unlinkat(directory.as_raw_fd(), name.as_ptr(), AT_REMOVEDIR) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(unix))]
fn unlink_directory_at(_directory: &File, _name: &str) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "descriptor-relative managed directory cleanup requires Unix",
    ))
}

#[cfg(not(unix))]
fn unlink_at(_directory: &File, _name: &str) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "descriptor-relative managed cleanup requires Unix",
    ))
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
pub(crate) fn atomic_rename_noreplace_at(
    from_directory: &File,
    from_name: &std::ffi::OsStr,
    to_directory: &File,
    to_name: &std::ffi::OsStr,
) -> io::Result<()> {
    const RENAME_EXCL: u32 = 0x0000_0004;
    unsafe extern "C" {
        fn renameatx_np(
            from_fd: i32,
            from: *const std::ffi::c_char,
            to_fd: i32,
            to: *const std::ffi::c_char,
            flags: u32,
        ) -> i32;
    }

    let from = CString::new(from_name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name contains a NUL byte"))?;
    let to = CString::new(to_name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name contains a NUL byte"))?;
    // SAFETY: both names are live C strings and both descriptors are open directories.
    if unsafe {
        renameatx_np(
            from_directory.as_raw_fd(),
            from.as_ptr(),
            to_directory.as_raw_fd(),
            to.as_ptr(),
            RENAME_EXCL,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn atomic_exchange_at(
    first_directory: &File,
    first_name: &std::ffi::OsStr,
    second_directory: &File,
    second_name: &std::ffi::OsStr,
) -> io::Result<()> {
    const RENAME_SWAP: u32 = 0x0000_0002;
    unsafe extern "C" {
        fn renameatx_np(
            first_fd: i32,
            first: *const std::ffi::c_char,
            second_fd: i32,
            second: *const std::ffi::c_char,
            flags: u32,
        ) -> i32;
    }
    let first = CString::new(first_name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name contains a NUL byte"))?;
    let second = CString::new(second_name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name contains a NUL byte"))?;
    // SAFETY: both names are live and both descriptors are verified directories.
    if unsafe {
        renameatx_np(
            first_directory.as_raw_fd(),
            first.as_ptr(),
            second_directory.as_raw_fd(),
            second.as_ptr(),
            RENAME_SWAP,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn atomic_exchange_at(
    first_directory: &File,
    first_name: &std::ffi::OsStr,
    second_directory: &File,
    second_name: &std::ffi::OsStr,
) -> io::Result<()> {
    const RENAME_EXCHANGE: u32 = 2;
    unsafe extern "C" {
        fn renameat2(
            first_fd: i32,
            first: *const std::ffi::c_char,
            second_fd: i32,
            second: *const std::ffi::c_char,
            flags: u32,
        ) -> i32;
    }
    let first = CString::new(first_name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name contains a NUL byte"))?;
    let second = CString::new(second_name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name contains a NUL byte"))?;
    // SAFETY: both names are live and both descriptors are verified directories.
    if unsafe {
        renameat2(
            first_directory.as_raw_fd(),
            first.as_ptr(),
            second_directory.as_raw_fd(),
            second.as_ptr(),
            RENAME_EXCHANGE,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn atomic_exchange_at(
    _first_directory: &File,
    _first_name: &std::ffi::OsStr,
    _second_directory: &File,
    _second_name: &std::ffi::OsStr,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "descriptor-relative atomic exchange requires Unix",
    ))
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
pub(crate) fn atomic_rename_noreplace_at(
    from_directory: &File,
    from_name: &std::ffi::OsStr,
    to_directory: &File,
    to_name: &std::ffi::OsStr,
) -> io::Result<()> {
    const RENAME_NOREPLACE: u32 = 1;
    unsafe extern "C" {
        fn renameat2(
            from_fd: i32,
            from: *const std::ffi::c_char,
            to_fd: i32,
            to: *const std::ffi::c_char,
            flags: u32,
        ) -> i32;
    }

    let from = CString::new(from_name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name contains a NUL byte"))?;
    let to = CString::new(to_name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name contains a NUL byte"))?;
    // SAFETY: both names are live C strings and both descriptors are open directories.
    if unsafe {
        renameat2(
            from_directory.as_raw_fd(),
            from.as_ptr(),
            to_directory.as_raw_fd(),
            to.as_ptr(),
            RENAME_NOREPLACE,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn remove_managed_entry(
    root: &Path,
    relative: &str,
    expected: ManagedFileIdentity,
    is_directory: bool,
    allow_private_staging: bool,
) -> io::Result<()> {
    let entry =
        open_confined_parent(root, relative, allow_private_staging).map_err(io::Error::other)?;
    let opened = open_entry_at(&entry.directory, &entry.name, is_directory)?;
    let metadata = opened.metadata()?;
    if managed_file_identity(&opened, &metadata)? != expected {
        return Err(io::Error::other("managed entry identity changed"));
    }
    let name = entry
        .name
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "name is not UTF-8"))?;
    if is_directory {
        unlink_directory_at(&entry.directory, name)?;
    } else {
        unlink_at(&entry.directory, name)?;
    }
    entry.directory.sync_all()?;
    Ok(())
}

impl std::error::Error for ManagedTextFileError {}

pub(crate) fn read_managed_text(
    root: &Path,
    relative: &str,
    current_version: String,
    current_content_digest: RecordDigest,
    current_metadata: PortableMetadata,
) -> Result<ManagedTextFile, ManagedTextFileError> {
    let (_path, _, _, metadata, _, bytes) = read_confined_regular_file(root, relative)?;
    if bytes.len() > MAX_MANAGED_TEXT_BYTES {
        return Err(ManagedTextFileError::TooLarge { bytes: bytes.len() });
    }
    let modified_from_current_version =
        RecordDigest::from_bytes(*mesh_types::Blake3::digest_bytes(&bytes).as_bytes())
            != current_content_digest
            || PortableMetadata::new(metadata.permissions().mode() & 0o111 != 0)
                != current_metadata;
    let text = String::from_utf8(bytes).map_err(|_| ManagedTextFileError::NotUtf8)?;
    Ok(ManagedTextFile {
        path: relative.to_owned(),
        text,
        current_version,
        modified_from_current_version,
    })
}

pub(crate) fn inspect_managed_file(
    root: &Path,
    relative: &str,
    current_version: String,
    current_content_digest: RecordDigest,
    current_metadata: PortableMetadata,
) -> Result<ManagedFileInspection, ManagedTextFileError> {
    let (_path, _, _, metadata, _, bytes) = read_confined_regular_file(root, relative)?;
    let byte_count = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let content_digest =
        RecordDigest::from_bytes(*mesh_types::Blake3::digest_bytes(&bytes).as_bytes());
    let text = (bytes.len() <= MAX_MANAGED_TEXT_BYTES)
        .then(|| String::from_utf8(bytes.clone()).ok())
        .flatten();
    let executable = metadata.permissions().mode() & 0o111 != 0;
    Ok(ManagedFileInspection {
        path: relative.to_owned(),
        current_version,
        byte_count,
        content_digest,
        executable,
        text,
        bytes,
        modified_from_current_version: content_digest != current_content_digest
            || PortableMetadata::new(executable) != current_metadata,
    })
}

pub(crate) fn inspect_managed_file_bounded(
    root: &Path,
    relative: &str,
    current_version: String,
    current_content_digest: RecordDigest,
    current_metadata: PortableMetadata,
    byte_limit: usize,
) -> Result<ManagedFileInspection, ManagedTextFileError> {
    let (_path, _, _, metadata, _, bytes) =
        read_confined_regular_file_bounded(root, relative, byte_limit)?;
    let byte_count = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let content_digest =
        RecordDigest::from_bytes(*mesh_types::Blake3::digest_bytes(&bytes).as_bytes());
    let text = bounded_text(&bytes);
    let executable = metadata.permissions().mode() & 0o111 != 0;
    Ok(ManagedFileInspection {
        path: relative.to_owned(),
        current_version,
        byte_count,
        content_digest,
        executable,
        text,
        bytes,
        modified_from_current_version: content_digest != current_content_digest
            || PortableMetadata::new(executable) != current_metadata,
    })
}

pub(crate) fn inspect_native_file_bounded(
    root: &Path,
    relative: &str,
    byte_limit: usize,
) -> Result<NativeFileInspection, ManagedTextFileError> {
    let reserve_private_top_level = crate::workspace::presented_workspace_storage_root(root)
        .map_or(true, |root| root.is_none());
    let (_path, _, _, metadata, _, bytes) = read_confined_regular_file_in_layout_bounded(
        root,
        relative,
        reserve_private_top_level,
        byte_limit,
    )?;
    Ok(NativeFileInspection::from_owned_bytes(
        relative.to_owned(),
        bytes,
        metadata.permissions().mode() & 0o111 != 0,
    ))
}

pub(crate) fn read_managed_bytes(
    root: &Path,
    relative: &str,
) -> Result<(PathBuf, Vec<u8>), ManagedTextFileError> {
    let (target, bytes) = read_managed_replacement(root, relative)?;
    Ok((target.path, bytes))
}

pub(crate) fn read_managed_replacement(
    root: &Path,
    relative: &str,
) -> Result<(ManagedReplacementTarget, Vec<u8>), ManagedTextFileError> {
    let reserve_private_top_level = crate::workspace::presented_workspace_storage_root(root)
        .map_or(true, |root| root.is_none());
    read_replacement_in_layout(root, relative, reserve_private_top_level)
}

pub(crate) fn read_export_replacement(
    root: &Path,
    relative: &str,
) -> Result<(ManagedReplacementTarget, Vec<u8>), ManagedTextFileError> {
    read_replacement_in_layout(root, relative, false)
}

fn read_replacement_in_layout(
    root: &Path,
    relative: &str,
    reserve_private_top_level: bool,
) -> Result<(ManagedReplacementTarget, Vec<u8>), ManagedTextFileError> {
    let (path, directory, _name, metadata, file_identity, bytes) =
        read_confined_regular_file_in_layout(root, relative, reserve_private_top_level)?;
    let parent_metadata = directory
        .metadata()
        .map_err(|error| ManagedTextFileError::io("metadata", &path, error))?;
    let parent = ManagedDirectoryIdentity {
        device: parent_metadata.dev(),
        inode: parent_metadata.ino(),
    };
    Ok((
        ManagedReplacementTarget {
            root: root.to_path_buf(),
            relative: relative.to_owned(),
            reserve_private_top_level,
            path,
            parent,
            file: file_identity,
            mode: metadata.mode(),
        },
        bytes,
    ))
}

pub(crate) fn inspect_managed_export_target(
    root: &Path,
    relative: &str,
) -> Result<ManagedExportTarget, ManagedTextFileError> {
    let parent = open_confined_parent_in_layout(root, relative, false, false)?;
    let parent_metadata = parent
        .directory
        .metadata()
        .map_err(|error| ManagedTextFileError::io("metadata", &parent.path, error))?;
    let parent_identity = ManagedDirectoryIdentity {
        device: parent_metadata.dev(),
        inode: parent_metadata.ino(),
    };
    let mut file = match open_read_at(&parent.directory, &parent.name) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(ManagedExportTarget::Missing {
                path: parent.path,
                parent: parent_identity,
            });
        }
        Err(error) => return Err(confined_open_error(&parent.path, error)),
    };
    let metadata = file
        .metadata()
        .map_err(|error| ManagedTextFileError::io("metadata", &parent.path, error))?;
    if !metadata.is_file() {
        return Err(ManagedTextFileError::NotRegularFile);
    }
    let file_identity = managed_file_identity(&file, &metadata)
        .map_err(|error| ManagedTextFileError::io("metadata", &parent.path, error))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| ManagedTextFileError::io("read", &parent.path, error))?;
    Ok(ManagedExportTarget::Existing(
        ManagedReplacementTarget {
            root: root.to_path_buf(),
            relative: relative.to_owned(),
            reserve_private_top_level: false,
            path: parent.path,
            parent: parent_identity,
            file: file_identity,
            mode: metadata.permissions().mode(),
        },
        bytes,
    ))
}

pub(crate) fn inspect_managed_directory_export_target(
    root: &Path,
    relative: &str,
) -> Result<ManagedDirectoryExportTarget, ManagedTextFileError> {
    let parent = open_confined_parent_in_layout(root, relative, false, false)?;
    let parent_metadata = parent
        .directory
        .metadata()
        .map_err(|error| ManagedTextFileError::io("metadata", &parent.path, error))?;
    let parent_identity = ManagedDirectoryIdentity {
        device: parent_metadata.dev(),
        inode: parent_metadata.ino(),
    };
    let directory = match open_entry_at(&parent.directory, &parent.name, true) {
        Ok(directory) => directory,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(ManagedDirectoryExportTarget::Missing {
                path: parent.path,
                parent: parent_identity,
            });
        }
        Err(error) => return Err(confined_open_error(&parent.path, error)),
    };
    let metadata = directory
        .metadata()
        .map_err(|error| ManagedTextFileError::io("metadata", &parent.path, error))?;
    if !metadata.is_dir() {
        return Err(ManagedTextFileError::NotRegularFile);
    }
    // This is preview classification rather than deletion authority. Revalidate the no-follow
    // directory identity immediately after enumeration; confirmation later reopens, rebinds, and
    // performs an empty-only descriptor-relative unlink.
    let empty = fs::read_dir(&parent.path)
        .map_err(|error| ManagedTextFileError::io("read export folder", &parent.path, error))?
        .next()
        .transpose()
        .map_err(|error| ManagedTextFileError::io("read export folder", &parent.path, error))?
        .is_none();
    let rechecked = open_entry_at(&parent.directory, &parent.name, true)
        .map_err(|error| confined_open_error(&parent.path, error))?;
    let rechecked_metadata = rechecked
        .metadata()
        .map_err(|error| ManagedTextFileError::io("metadata", &parent.path, error))?;
    if rechecked_metadata.dev() != metadata.dev() || rechecked_metadata.ino() != metadata.ino() {
        return Err(ManagedTextFileError::StaleExportTarget);
    }
    Ok(ManagedDirectoryExportTarget::Existing {
        parent: parent_identity,
        directory: ManagedDirectoryIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        },
        empty,
    })
}

#[cfg(test)]
pub(crate) fn create_export_file_in_exact_parent(
    path: &Path,
    bytes: &[u8],
    executable: bool,
    expected_parent: ManagedDirectoryIdentity,
) -> io::Result<()> {
    create_export_file_in_exact_parent_prepared(
        path,
        bytes,
        executable,
        expected_parent,
        |_| Ok(()),
    )
}

/// Create one export file only after its exact future identity has been durably prepared.
pub(crate) fn create_export_file_in_exact_parent_prepared<F>(
    path: &Path,
    bytes: &[u8],
    executable: bool,
    expected_parent: ManagedDirectoryIdentity,
    prepare_installation: F,
) -> io::Result<()>
where
    F: FnOnce(ManagedFileIdentity) -> io::Result<()>,
{
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("export file has no parent"))?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("export file has no name"))?;
    let directory = open_exact_directory(parent, expected_parent)?;
    let temporary_name = format!(
        ".{}.mesh-export-{}-{}",
        name.to_string_lossy(),
        std::process::id(),
        TEMPORARY_COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let result = (|| {
        let mut file = create_new_at(&directory, &temporary_name)?;
        file.set_permissions(fs::Permissions::from_mode(if executable {
            0o755
        } else {
            0o644
        }))?;
        file.write_all(bytes)?;
        file.sync_all()?;
        let installed_identity = managed_file_identity(&file, &file.metadata()?)?;
        prepare_installation(installed_identity)?;
        atomic_rename_noreplace_at(
            &directory,
            std::ffi::OsStr::new(&temporary_name),
            &directory,
            name,
        )?;
        directory.sync_all()
    })();
    if result.is_err() {
        let _ = unlink_at(&directory, &temporary_name);
    }
    result
}

/// Finish a create-only export whose durable receipt was published before the process stopped.
///
/// A prepared file is still private: its name contains the Mesh export marker and the reviewed
/// destination name remains absent. Recovery considers only a regular file whose exact inode,
/// bytes and executable state are accepted by `proves_installation`; lookalike user files are
/// ignored. More than one proven candidate refuses rather than choosing between two durable
/// histories.
pub(crate) fn recover_prepared_export_file_in_exact_parent<F>(
    path: &Path,
    bytes: &[u8],
    executable: bool,
    expected_parent: ManagedDirectoryIdentity,
    proves_installation: F,
) -> io::Result<bool>
where
    F: Fn(ManagedFileIdentity) -> bool,
{
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("export file has no parent"))?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("export file has no name"))?;
    let directory = open_exact_directory(parent, expected_parent)?;
    let prefix = format!(".{}.mesh-export-", name.to_string_lossy());
    let mut proven = Vec::new();

    for entry in fs::read_dir(parent)? {
        let candidate = entry?.file_name();
        if !candidate.to_string_lossy().starts_with(&prefix) {
            continue;
        }
        let Ok(mut file) = open_read_at(&directory, &candidate) else {
            continue;
        };
        let Ok(metadata) = file.metadata() else {
            continue;
        };
        let Ok(identity) = managed_file_identity(&file, &metadata) else {
            continue;
        };
        let mut candidate_bytes = Vec::new();
        if metadata.is_file()
            && (metadata.mode() & 0o111 != 0) == executable
            && proves_installation(identity)
            && file.read_to_end(&mut candidate_bytes).is_ok()
            && candidate_bytes == bytes
        {
            proven.push((candidate, identity));
        }
    }

    match proven.as_slice() {
        [] => Ok(false),
        [candidate] => {
            // Rebind the displayed parent path after enumeration and re-open the candidate through
            // the already pinned directory immediately before publishing its name.
            let _ = open_exact_directory(parent, expected_parent)?;
            let mut file = open_read_at(&directory, &candidate.0)?;
            let metadata = file.metadata()?;
            if !metadata.is_file()
                || managed_file_identity(&file, &metadata)? != candidate.1
                || (metadata.mode() & 0o111 != 0) != executable
                || !proves_installation(candidate.1)
            {
                return Err(io::Error::other("prepared export changed before recovery"));
            }
            let mut candidate_bytes = Vec::new();
            file.read_to_end(&mut candidate_bytes)?;
            if candidate_bytes != bytes {
                return Err(io::Error::other("prepared export changed before recovery"));
            }
            atomic_rename_noreplace_at(&directory, &candidate.0, &directory, name)?;
            directory.sync_all()?;
            Ok(true)
        }
        _ => Err(io::Error::other(
            "multiple durable prepared exports match the same destination",
        )),
    }
}

#[cfg(unix)]
fn managed_file_identity(file: &File, metadata: &fs::Metadata) -> io::Result<ManagedFileIdentity> {
    Ok(ManagedFileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
        incarnation: managed_file_incarnation(file, metadata),
    })
}

#[cfg(not(unix))]
fn managed_file_identity(
    _file: &File,
    _metadata: &fs::Metadata,
) -> io::Result<ManagedFileIdentity> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "managed file identity requires Unix",
    ))
}

#[cfg(target_os = "macos")]
fn managed_file_incarnation(_file: &File, metadata: &fs::Metadata) -> ManagedFileIncarnation {
    use std::os::darwin::fs::MetadataExt as _;

    ManagedFileIncarnation {
        kind: 'b',
        seconds: metadata.st_birthtime(),
        nanoseconds: metadata.st_birthtime_nsec(),
    }
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn managed_file_incarnation(file: &File, metadata: &fs::Metadata) -> ManagedFileIncarnation {
    use core::ffi::{c_char, c_int, c_uint, c_void};

    unsafe extern "C" {
        fn statx(
            directory: c_int,
            path: *const c_char,
            flags: c_int,
            mask: c_uint,
            result: *mut c_void,
        ) -> c_int;
    }
    const AT_EMPTY_PATH: c_int = 0x1000;
    const AT_SYMLINK_NOFOLLOW: c_int = 0x100;
    const STATX_BTIME: c_uint = 0x0000_0800;

    let empty = [0 as c_char];
    let mut storage = [0u64; 32];
    // SAFETY: `storage` is aligned, writable, and exactly 256 bytes, the kernel ABI size for
    // `struct statx`; the empty C string is live for this call and `AT_EMPTY_PATH` makes `file`'s
    // already-open descriptor—not a replaceable pathname—the object being inspected.
    let status = unsafe {
        statx(
            file.as_raw_fd(),
            empty.as_ptr(),
            AT_EMPTY_PATH | AT_SYMLINK_NOFOLLOW,
            STATX_BTIME,
            storage.as_mut_ptr().cast(),
        )
    };
    if status == 0 {
        let first = storage[0].to_ne_bytes();
        let mask = u32::from_ne_bytes(first[0..4].try_into().unwrap_or([0; 4]));
        if mask & STATX_BTIME != 0 {
            let seconds = i64::from_ne_bytes(storage[10].to_ne_bytes());
            let nanos = storage[11].to_ne_bytes();
            let nanoseconds = u32::from_ne_bytes(nanos[0..4].try_into().unwrap_or([0; 4]));
            if nanoseconds < 1_000_000_000 {
                return ManagedFileIncarnation {
                    kind: 'b',
                    seconds,
                    nanoseconds: i64::from(nanoseconds),
                };
            }
        }
    }
    change_time_incarnation(metadata)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn managed_file_incarnation(_file: &File, metadata: &fs::Metadata) -> ManagedFileIncarnation {
    change_time_incarnation(metadata)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn change_time_incarnation(metadata: &fs::Metadata) -> ManagedFileIncarnation {
    ManagedFileIncarnation {
        kind: 'c',
        seconds: metadata.ctime(),
        nanoseconds: metadata.ctime_nsec(),
    }
}

#[cfg(test)]
pub(crate) fn atomic_replace(
    target: &ManagedReplacementTarget,
    expected: &[u8],
    bytes: &[u8],
) -> Result<ManagedReplacementTarget, io::Error> {
    atomic_replace_with_mode(target, expected, bytes, target.mode)
}

pub(crate) fn atomic_replace_with_mode(
    target: &ManagedReplacementTarget,
    expected: &[u8],
    bytes: &[u8],
    installed_mode: u32,
) -> Result<ManagedReplacementTarget, io::Error> {
    atomic_replace_with_mode_prepared(target, expected, bytes, installed_mode, |_| Ok(()))
}

/// Replace one exact file only after its future inode has durable external provenance.
pub(crate) fn atomic_replace_with_mode_prepared<F>(
    target: &ManagedReplacementTarget,
    expected: &[u8],
    bytes: &[u8],
    installed_mode: u32,
    prepare_installation: F,
) -> Result<ManagedReplacementTarget, io::Error>
where
    F: FnOnce(ManagedFileIdentity) -> io::Result<()>,
{
    atomic_replace_with_durability_hooks(
        target,
        expected,
        bytes,
        installed_mode,
        prepare_installation,
        ReplacementDurabilityHooks {
            before_exchange: || {},
            after_exchange: || {},
            commit_directory: File::sync_all,
            cleanup_directory: File::sync_all,
        },
    )
}

#[cfg(test)]
fn atomic_replace_with<F>(
    target: &ManagedReplacementTarget,
    expected: &[u8],
    bytes: &[u8],
    before_exchange: F,
) -> Result<ManagedReplacementTarget, io::Error>
where
    F: FnOnce(),
{
    atomic_replace_with_hooks(target, expected, bytes, target.mode, before_exchange, || {})
}

#[cfg(test)]
fn atomic_replace_with_hooks<F, G>(
    target: &ManagedReplacementTarget,
    expected: &[u8],
    bytes: &[u8],
    installed_mode: u32,
    before_exchange: F,
    after_exchange: G,
) -> Result<ManagedReplacementTarget, io::Error>
where
    F: FnOnce(),
    G: FnOnce(),
{
    atomic_replace_with_durability_hooks(
        target,
        expected,
        bytes,
        installed_mode,
        |_| Ok(()),
        ReplacementDurabilityHooks {
            before_exchange,
            after_exchange,
            commit_directory: File::sync_all,
            cleanup_directory: File::sync_all,
        },
    )
}

struct ReplacementDurabilityHooks<F, G, S, C> {
    before_exchange: F,
    after_exchange: G,
    commit_directory: S,
    cleanup_directory: C,
}

fn atomic_replace_with_durability_hooks<F, G, S, C, P>(
    target: &ManagedReplacementTarget,
    expected: &[u8],
    bytes: &[u8],
    installed_mode: u32,
    prepare_installation: P,
    hooks: ReplacementDurabilityHooks<F, G, S, C>,
) -> Result<ManagedReplacementTarget, io::Error>
where
    F: FnOnce(),
    G: FnOnce(),
    S: FnOnce(&File) -> Result<(), io::Error>,
    C: FnOnce(&File) -> Result<(), io::Error>,
    P: FnOnce(ManagedFileIdentity) -> io::Result<()>,
{
    let ReplacementDurabilityHooks {
        before_exchange,
        after_exchange,
        commit_directory,
        cleanup_directory,
    } = hooks;
    let (path, directory, name, original_metadata, original_identity, original_bytes) =
        read_confined_regular_file_in_layout(
            &target.root,
            &target.relative,
            target.reserve_private_top_level,
        )
        .map_err(|error| io::Error::other(format!("managed file confinement failed: {error}")))?;
    let name = name.as_os_str();
    if path != target.path {
        return Err(io::Error::other(
            "managed file path changed before replacement",
        ));
    }
    let parent_metadata = directory.metadata()?;
    let parent_identity = ManagedDirectoryIdentity {
        device: parent_metadata.dev(),
        inode: parent_metadata.ino(),
    };
    if !original_metadata.is_file()
        || parent_identity != target.parent
        || original_identity != target.file
        || original_metadata.mode() != target.mode
        || original_bytes != expected
    {
        return Err(io::Error::other(
            "managed file changed identity before replacement",
        ));
    }
    let temporary_name = format!(
        ".{}.mesh-edit-{}-{}",
        name.to_string_lossy(),
        std::process::id(),
        TEMPORARY_COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let installed_permissions = fs::Permissions::from_mode(installed_mode);
    let mut exchange_live = false;
    let result = (|| {
        let mut file = create_new_at(&directory, &temporary_name)?;
        file.set_permissions(installed_permissions)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        let installed_metadata = file.metadata()?;
        let installed_identity = managed_file_identity(&file, &installed_metadata)?;
        let installed_mode = installed_metadata.mode();
        prepare_installation(installed_identity)?;
        let mut current = open_read_at(&directory, name)?;
        let current_metadata = current.metadata()?;
        let mut current_bytes = Vec::new();
        current.read_to_end(&mut current_bytes)?;
        if !current_metadata.is_file()
            || managed_file_identity(&current, &current_metadata)? != target.file
            || current_metadata.mode() != target.mode
            || current_bytes != expected
        {
            return Err(io::Error::other(
                "managed file changed identity before replacement",
            ));
        }
        before_exchange();
        atomic_exchange_at(
            &directory,
            std::ffi::OsStr::new(&temporary_name),
            &directory,
            name,
        )?;
        exchange_live = true;
        after_exchange();
        let displaced = open_read_at(&directory, std::ffi::OsStr::new(&temporary_name));
        let displaced_matches = displaced.ok().is_some_and(|mut file| {
            let metadata = match file.metadata() {
                Ok(metadata) => metadata,
                Err(_) => return false,
            };
            let mut displaced_bytes = Vec::new();
            metadata.is_file()
                && managed_file_identity(&file, &metadata).ok() == Some(target.file)
                && metadata.mode() == target.mode
                && file.read_to_end(&mut displaced_bytes).is_ok()
                && displaced_bytes == expected
        });
        if !displaced_matches {
            if !managed_file_matches(&directory, name, installed_identity, installed_mode, bytes) {
                return Err(io::Error::other(format!(
                    "managed file changed after replacement; prior file retained as {temporary_name}"
                )));
            }
            atomic_exchange_at(
                &directory,
                std::ffi::OsStr::new(&temporary_name),
                &directory,
                name,
            )?;
            exchange_live = false;
            return Err(io::Error::other(
                "managed file changed identity at replacement",
            ));
        }
        if !managed_file_identity_matches(&directory, name, installed_identity, installed_mode) {
            return Err(io::Error::other(format!(
                "managed file changed after replacement; prior file retained as {temporary_name}"
            )));
        }
        if let Err(commit) = commit_directory(&directory) {
            if !managed_file_matches(&directory, name, installed_identity, installed_mode, bytes) {
                return Err(io::Error::other(format!(
                    "managed replacement durability failed ({commit}); installed file changed and prior file was retained as {temporary_name}"
                )));
            }
            atomic_exchange_at(
                &directory,
                std::ffi::OsStr::new(&temporary_name),
                &directory,
                name,
            )
            .map_err(|rollback| {
                io::Error::other(format!(
                    "managed replacement durability failed ({commit}); rollback failed ({rollback})"
                ))
            })?;
            exchange_live = false;
            directory.sync_all().map_err(|rollback| {
                io::Error::other(format!(
                    "managed replacement durability failed ({commit}); rollback durability failed ({rollback})"
                ))
            })?;
            return Err(commit);
        }
        if let Err(cleanup) = unlink_at(&directory, &temporary_name) {
            if !managed_file_matches(&directory, name, installed_identity, installed_mode, bytes) {
                return Err(io::Error::other(format!(
                    "managed replacement cleanup failed ({cleanup}); installed file changed and prior file was retained as {temporary_name}"
                )));
            }
            atomic_exchange_at(
                &directory,
                std::ffi::OsStr::new(&temporary_name),
                &directory,
                name,
            )
            .map_err(|rollback| {
                io::Error::other(format!(
                    "managed replacement cleanup failed ({cleanup}); rollback failed ({rollback})"
                ))
            })?;
            exchange_live = false;
            directory.sync_all().map_err(|rollback| {
                io::Error::other(format!(
                    "managed replacement cleanup failed ({cleanup}); rollback durability failed ({rollback})"
                ))
            })?;
            return Err(cleanup);
        }
        exchange_live = false;
        // The first barrier made the exchange durable while the displaced inode was still named
        // and exactly reversible. Once it succeeds, failure to persist only removal of that hidden
        // displaced name cannot turn the installed durable replacement into a reported failure.
        if cleanup_directory(&directory).is_err() {
            let _ = directory.sync_all();
        }
        Ok(ManagedReplacementTarget {
            path: target.path.clone(),
            root: target.root.clone(),
            relative: target.relative.clone(),
            reserve_private_top_level: target.reserve_private_top_level,
            parent: target.parent,
            file: installed_identity,
            mode: installed_mode,
        })
    })();
    if result.is_err() && !exchange_live {
        let _ = unlink_at(&directory, &temporary_name);
    }
    result
}

fn managed_file_identity_matches(
    directory: &File,
    name: &std::ffi::OsStr,
    expected_identity: ManagedFileIdentity,
    expected_mode: u32,
) -> bool {
    open_read_at(directory, name).ok().is_some_and(|file| {
        file.metadata().is_ok_and(|metadata| {
            metadata.is_file()
                && managed_file_identity(&file, &metadata).ok() == Some(expected_identity)
                && metadata.mode() == expected_mode
        })
    })
}

fn managed_file_matches(
    directory: &File,
    name: &std::ffi::OsStr,
    expected_identity: ManagedFileIdentity,
    expected_mode: u32,
    expected_bytes: &[u8],
) -> bool {
    open_read_at(directory, name).ok().is_some_and(|mut file| {
        let metadata = match file.metadata() {
            Ok(metadata) => metadata,
            Err(_) => return false,
        };
        let mut bytes = Vec::new();
        metadata.is_file()
            && managed_file_identity(&file, &metadata).ok() == Some(expected_identity)
            && metadata.mode() == expected_mode
            && file.read_to_end(&mut bytes).is_ok()
            && bytes == expected_bytes
    })
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn read_confined_regular_file(
    root: &Path,
    relative: &str,
) -> Result<ConfinedRegularFileRead, ManagedTextFileError> {
    let reserve_private_top_level = crate::workspace::presented_workspace_storage_root(root)
        .map_or(true, |root| root.is_none());
    read_confined_regular_file_in_layout(root, relative, reserve_private_top_level)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn read_confined_regular_file_bounded(
    root: &Path,
    relative: &str,
    byte_limit: usize,
) -> Result<ConfinedRegularFileRead, ManagedTextFileError> {
    let reserve_private_top_level = crate::workspace::presented_workspace_storage_root(root)
        .map_or(true, |root| root.is_none());
    read_confined_regular_file_in_layout_bounded(
        root,
        relative,
        reserve_private_top_level,
        byte_limit,
    )
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn read_confined_regular_file_in_layout(
    root: &Path,
    relative: &str,
    reserve_private_top_level: bool,
) -> Result<ConfinedRegularFileRead, ManagedTextFileError> {
    let components = managed_components(relative, reserve_private_top_level)?;
    let mut path = root.to_path_buf();
    let mut directory = OpenOptions::new()
        .read(true)
        .custom_flags(OPEN_DIRECTORY_FLAGS)
        .open(root)
        .map_err(|error| confined_open_error(root, error))?;
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(name) = component else {
            return Err(ManagedTextFileError::InvalidPath);
        };
        path.push(name);
        if index + 1 == components.len() {
            let mut file = open_read_at(&directory, name)
                .map_err(|error| confined_open_error(&path, error))?;
            let metadata = file
                .metadata()
                .map_err(|error| ManagedTextFileError::io("metadata", &path, error))?;
            if !metadata.is_file() {
                return Err(ManagedTextFileError::NotRegularFile);
            }
            let identity = managed_file_identity(&file, &metadata)
                .map_err(|error| ManagedTextFileError::io("metadata", &path, error))?;
            run_before_read_hook();
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)
                .map_err(|error| ManagedTextFileError::io("read", &path, error))?;
            return Ok((
                path,
                directory,
                name.to_os_string(),
                metadata,
                identity,
                bytes,
            ));
        }
        directory = open_directory_at(&directory, name)
            .map_err(|error| confined_open_error(&path, error))?;
    }
    Err(ManagedTextFileError::InvalidPath)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn read_confined_regular_file_in_layout_bounded(
    root: &Path,
    relative: &str,
    reserve_private_top_level: bool,
    byte_limit: usize,
) -> Result<ConfinedRegularFileRead, ManagedTextFileError> {
    let components = managed_components(relative, reserve_private_top_level)?;
    let mut path = root.to_path_buf();
    let mut directory = OpenOptions::new()
        .read(true)
        .custom_flags(OPEN_DIRECTORY_FLAGS)
        .open(root)
        .map_err(|error| confined_open_error(root, error))?;
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(name) = component else {
            return Err(ManagedTextFileError::InvalidPath);
        };
        path.push(name);
        if index + 1 == components.len() {
            let mut file = open_read_at(&directory, name)
                .map_err(|error| confined_open_error(&path, error))?;
            let metadata = file
                .metadata()
                .map_err(|error| ManagedTextFileError::io("metadata", &path, error))?;
            if !metadata.is_file() {
                return Err(ManagedTextFileError::NotRegularFile);
            }
            let identity = managed_file_identity(&file, &metadata)
                .map_err(|error| ManagedTextFileError::io("metadata", &path, error))?;
            let metadata_bytes = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
            if metadata_bytes > byte_limit {
                return Err(ManagedTextFileError::PreviewTooLarge {
                    bytes: metadata_bytes,
                    limit: byte_limit,
                });
            }
            run_before_read_hook();
            let mut bytes = Vec::with_capacity(metadata_bytes);
            std::io::Read::by_ref(&mut file)
                .take(
                    u64::try_from(byte_limit)
                        .unwrap_or(u64::MAX)
                        .saturating_add(1),
                )
                .read_to_end(&mut bytes)
                .map_err(|error| ManagedTextFileError::io("read", &path, error))?;
            if bytes.len() > byte_limit {
                return Err(ManagedTextFileError::PreviewTooLarge {
                    bytes: bytes.len(),
                    limit: byte_limit,
                });
            }
            return Ok((
                path,
                directory,
                name.to_os_string(),
                metadata,
                identity,
                bytes,
            ));
        }
        directory = open_directory_at(&directory, name)
            .map_err(|error| confined_open_error(&path, error))?;
    }
    Err(ManagedTextFileError::InvalidPath)
}

#[cfg(target_os = "macos")]
const CONFINED_LOOP_ERROR: i32 = 62;
#[cfg(target_os = "linux")]
const CONFINED_LOOP_ERROR: i32 = 40;

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn confined_open_error(path: &Path, error: io::Error) -> ManagedTextFileError {
    // Descriptor-relative no-follow opens report a symlink as ELOOP and a non-directory parent as
    // ENOTDIR. Both are the same public confinement refusal that the prior lstat walk exposed.
    if matches!(error.raw_os_error(), Some(CONFINED_LOOP_ERROR | 20)) {
        ManagedTextFileError::NotRegularFile
    } else {
        ManagedTextFileError::io("open", path, error)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn read_confined_regular_file(
    root: &Path,
    relative: &str,
) -> Result<ConfinedRegularFileRead, ManagedTextFileError> {
    read_confined_regular_file_in_layout(root, relative, true)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn read_confined_regular_file_bounded(
    root: &Path,
    _relative: &str,
    _byte_limit: usize,
) -> Result<ConfinedRegularFileRead, ManagedTextFileError> {
    Err(ManagedTextFileError::io(
        "open",
        root,
        io::Error::new(
            io::ErrorKind::Unsupported,
            "descriptor-relative managed reads require macOS or Linux",
        ),
    ))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn read_confined_regular_file_in_layout_bounded(
    root: &Path,
    _relative: &str,
    _reserve_private_top_level: bool,
    _byte_limit: usize,
) -> Result<ConfinedRegularFileRead, ManagedTextFileError> {
    read_confined_regular_file_bounded(root, "", 0)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn read_confined_regular_file_in_layout(
    root: &Path,
    _relative: &str,
    _reserve_private_top_level: bool,
) -> Result<ConfinedRegularFileRead, ManagedTextFileError> {
    Err(ManagedTextFileError::io(
        "open",
        root,
        io::Error::new(
            io::ErrorKind::Unsupported,
            "descriptor-relative managed reads require macOS or Linux",
        ),
    ))
}

fn managed_components(
    relative: &str,
    reserve_private_top_level: bool,
) -> Result<Vec<Component<'_>>, ManagedTextFileError> {
    managed_components_with_staging(relative, false, reserve_private_top_level)
}

fn managed_components_with_staging(
    relative: &str,
    allow_private_staging: bool,
    reserve_private_top_level: bool,
) -> Result<Vec<Component<'_>>, ManagedTextFileError> {
    let relative_path = Path::new(relative);
    if relative.is_empty() || relative_path.is_absolute() {
        return Err(ManagedTextFileError::InvalidPath);
    }
    let components = relative_path.components().collect::<Vec<_>>();
    if components.is_empty()
        || components
            .iter()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(ManagedTextFileError::InvalidPath);
    }
    let private = [
        ".mesh",
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
        crate::pull_back_receipt::RECEIPT_DIRECTORY_NAME,
        "mount",
        ".mesh-managed-mutation",
        ".mesh-managed-mutation.next",
    ];
    let git_metadata = components.iter().any(|component| match component {
        Component::Normal(name) => *name == ".git",
        _ => true,
    });
    let private_top_level = reserve_private_top_level
        && components.first().is_some_and(|component| match component {
            Component::Normal(name) => private.iter().any(|reserved| name == reserved),
            _ => false,
        });
    if git_metadata
        || private_top_level
        || (!allow_private_staging
            && components.iter().any(|component| match component {
                Component::Normal(name) => name.to_string_lossy().starts_with(".mesh-delete-"),
                _ => true,
            }))
    {
        return Err(ManagedTextFileError::ReservedPath);
    }

    Ok(components)
}

#[cfg(test)]
mod tests {
    use super::{
        atomic_replace, atomic_replace_with, atomic_replace_with_durability_hooks,
        atomic_replace_with_hooks, atomic_replace_with_mode_prepared, confined_free_path,
        create_export_directory_in_exact_parent_prepared, create_export_file_in_exact_parent,
        create_export_file_in_exact_parent_prepared, create_managed_directory, create_text_file,
        inspect_managed_directory_export_target, inspect_managed_export_target,
        inspect_managed_file, managed_file_identity, move_managed_entry, read_managed_replacement,
        read_managed_text, recover_prepared_export_directory_in_exact_parent,
        recover_prepared_export_file_in_exact_parent, remove_exact_export_directory,
        remove_exact_export_file, remove_managed_entry, ManagedDirectoryExportTarget,
        ManagedExportTarget, PortableMetadata, RecordDigest, ReplacementDurabilityHooks,
        BEFORE_READ_HOOK, BEFORE_RENAME_HOOK,
    };
    use std::fs::{self, File};
    use std::io;
    use std::os::unix::fs::MetadataExt as _;

    fn fail_commit_barrier(_: &File) -> Result<(), io::Error> {
        Err(io::Error::other("injected commit barrier failure"))
    }

    fn fail_cleanup_barrier(_: &File) -> Result<(), io::Error> {
        Err(io::Error::other("injected cleanup barrier failure"))
    }

    #[cfg(unix)]
    #[test]
    fn confined_reads_never_follow_a_file_swapped_to_an_outside_symlink() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "mesh-confined-read-swap-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let managed = root.join("managed");
        let outside = root.join("outside-secret.txt");
        let relative = "note.txt";
        let path = managed.join(relative);
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&managed).expect("managed root");
        fs::write(&path, b"managed bytes").expect("managed file");
        fs::write(&outside, b"outside secret").expect("outside file");

        let swap_path = path.clone();
        let swap_outside = outside.clone();
        BEFORE_READ_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                fs::remove_file(&swap_path).expect("remove managed file");
                symlink(&swap_outside, &swap_path).expect("swap outside symlink");
            }));
        });
        let inspected = inspect_managed_file(
            &managed,
            relative,
            "version".to_owned(),
            RecordDigest::from_bytes([0; 32]),
            PortableMetadata::default(),
        )
        .expect("the already-confined managed file remains readable");
        assert_eq!(inspected.text.as_deref(), Some("managed bytes"));

        fs::remove_file(&path).expect("remove first link");
        fs::write(&path, b"managed bytes").expect("restore managed file");
        let swap_path = path.clone();
        let swap_outside = outside.clone();
        BEFORE_READ_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                fs::remove_file(&swap_path).expect("remove managed file");
                symlink(&swap_outside, &swap_path).expect("swap outside symlink");
            }));
        });
        let text = read_managed_text(
            &managed,
            relative,
            "version".to_owned(),
            RecordDigest::from_bytes([0; 32]),
            PortableMetadata::default(),
        )
        .expect("the already-confined managed text remains readable");
        assert_eq!(text.text(), "managed bytes");

        fs::remove_file(path).expect("remove final link");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn atomic_replacement_refuses_stale_expected_bytes() {
        let parent = std::env::temp_dir().join(format!(
            "mesh-atomic-replace-stale-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&parent);
        fs::create_dir_all(&parent).expect("scratch parent");
        let path = parent.join("file.txt");
        fs::write(&path, b"stale inspected bytes").expect("inspected bytes");
        let (target, _) = read_managed_replacement(&parent, "file.txt").expect("inspect target");
        fs::write(&path, b"newer external bytes").expect("newer bytes");

        let result = atomic_replace(&target, b"stale inspected bytes", b"desktop replacement");

        assert!(result.is_err());
        assert_eq!(
            fs::read(&path).expect("newer bytes remain"),
            b"newer external bytes"
        );
        fs::remove_dir_all(parent).expect("cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn atomic_replacement_never_follows_a_swapped_parent_outside_the_workspace() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "mesh-atomic-replace-parent-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let managed = root.join("managed");
        let admitted_parent = managed.join("docs");
        let displaced_parent = root.join("displaced-docs");
        let outside = root.join("outside");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&admitted_parent).expect("managed parent");
        fs::create_dir(&outside).expect("outside parent");
        fs::write(admitted_parent.join("note.txt"), b"same bytes").expect("managed file");
        fs::write(outside.join("note.txt"), b"same bytes").expect("outside file");

        let (target, _) =
            read_managed_replacement(&managed, "docs/note.txt").expect("inspect managed target");
        fs::rename(&admitted_parent, &displaced_parent).expect("displace managed parent");
        symlink(&outside, &admitted_parent).expect("redirect managed path outside");

        atomic_replace(&target, b"same bytes", b"mesh replacement")
            .expect_err("a replaced parent must never carry managed replacement authority");
        assert_eq!(
            fs::read(outside.join("note.txt")).expect("outside bytes remain"),
            b"same bytes"
        );
        assert_eq!(
            fs::read(displaced_parent.join("note.txt")).expect("managed bytes remain"),
            b"same bytes"
        );

        fs::remove_file(&admitted_parent).expect("remove parent link");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn managed_directory_creation_never_follows_a_replaced_ancestor_outside_the_workspace() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "mesh-create-directory-ancestor-swap-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let managed = root.join("managed");
        let admitted_ancestor = managed.join("team");
        let displaced_ancestor = root.join("displaced-team");
        let outside = root.join("outside");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(admitted_ancestor.join("docs")).expect("managed ancestor");
        fs::create_dir_all(outside.join("docs")).expect("outside ancestor");

        let _target = confined_free_path(&managed, "team/docs/new")
            .expect("target is initially confined and absent");
        fs::rename(&admitted_ancestor, &displaced_ancestor).expect("displace admitted ancestor");
        symlink(&outside, &admitted_ancestor).expect("redirect ancestor outside");

        create_managed_directory(&managed, "team/docs/new")
            .expect_err("creation must revalidate and remain confined to the managed root");
        assert!(
            !outside.join("docs/new").exists(),
            "managed creation escaped through the replaced ancestor"
        );
        assert!(
            !displaced_ancestor.join("docs/new").exists(),
            "the displaced managed directory must also remain unchanged"
        );

        fs::remove_file(&admitted_ancestor).expect("remove redirect");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn managed_file_mutations_never_follow_a_replaced_ancestor_outside_the_workspace() {
        use std::os::unix::fs::symlink;

        for operation in ["create", "move", "remove"] {
            let root = std::env::temp_dir().join(format!(
                "mesh-file-{operation}-ancestor-swap-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let managed = root.join("managed");
            let admitted_ancestor = managed.join("team");
            let displaced_ancestor = root.join("displaced-team");
            let outside = root.join("outside");
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(admitted_ancestor.join("docs")).expect("managed ancestor");
            fs::create_dir_all(outside.join("docs")).expect("outside ancestor");
            fs::write(admitted_ancestor.join("docs/source.txt"), b"managed source")
                .expect("managed source");
            fs::write(outside.join("docs/source.txt"), b"outside source").expect("outside source");
            let source_path = admitted_ancestor.join("docs/source.txt");
            let source_file = File::open(&source_path).expect("open managed source");
            let source_metadata = source_file.metadata().expect("managed metadata");
            let expected =
                managed_file_identity(&source_file, &source_metadata).expect("managed identity");

            fs::rename(&admitted_ancestor, &displaced_ancestor)
                .expect("displace admitted ancestor");
            symlink(&outside, &admitted_ancestor).expect("redirect ancestor outside");

            match operation {
                "create" => create_text_file(&managed, "team/docs/new.txt", b"managed create")
                    .expect_err("file creation must refuse a replaced ancestor"),
                "move" => move_managed_entry(
                    &managed,
                    "team/docs/source.txt",
                    "team/docs/moved.txt",
                    expected,
                    false,
                    false,
                )
                .expect_err("move must refuse a replaced ancestor"),
                "remove" => {
                    remove_managed_entry(&managed, "team/docs/source.txt", expected, false, false)
                        .expect_err("remove must refuse a replaced ancestor")
                }
                _ => unreachable!(),
            };
            assert_eq!(
                fs::read(outside.join("docs/source.txt")).expect("outside source remains"),
                b"outside source"
            );
            assert!(!outside.join("docs/new.txt").exists());
            assert!(!outside.join("docs/moved.txt").exists());
            assert_eq!(
                fs::read(displaced_ancestor.join("docs/source.txt"))
                    .expect("managed source remains"),
                b"managed source"
            );

            fs::remove_file(&admitted_ancestor).expect("remove redirect");
            fs::remove_dir_all(root).expect("cleanup");
        }
    }

    #[test]
    fn replacement_linearization_restores_a_file_installed_after_the_last_check() {
        let parent = std::env::temp_dir().join(format!(
            "mesh-atomic-replace-linearization-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&parent);
        fs::create_dir_all(&parent).expect("scratch parent");
        let path = parent.join("file.txt");
        let displaced = parent.join("displaced.txt");
        fs::write(&path, b"managed bytes").expect("managed bytes");
        let (target, _) =
            read_managed_replacement(&parent, "file.txt").expect("inspect managed target");

        atomic_replace_with(&target, b"managed bytes", b"mesh replacement", || {
            fs::rename(&path, &displaced).expect("displace after final check");
            fs::write(&path, b"concurrent bytes").expect("concurrent file");
        })
        .expect_err("the exchange must restore a concurrently installed file");

        assert_eq!(
            fs::read(&path).expect("concurrent file remains"),
            b"concurrent bytes"
        );
        assert_eq!(
            fs::read(&displaced).expect("managed file remains"),
            b"managed bytes"
        );
        fs::remove_dir_all(parent).expect("cleanup");
    }

    #[test]
    fn replacement_preserves_every_file_when_the_installed_file_is_swapped_again() {
        let parent = std::env::temp_dir().join(format!(
            "mesh-atomic-replace-post-exchange-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&parent);
        fs::create_dir_all(&parent).expect("scratch parent");
        let path = parent.join("file.txt");
        fs::write(&path, b"managed bytes").expect("managed bytes");
        let (target, _) =
            read_managed_replacement(&parent, "file.txt").expect("inspect managed target");

        let installed_elsewhere = parent.join("installed-elsewhere.txt");
        let result = atomic_replace_with_hooks(
            &target,
            b"managed bytes",
            b"mesh replacement",
            target.mode,
            || {},
            || {
                fs::rename(&path, &installed_elsewhere).expect("move installed replacement");
                fs::write(&path, b"third-party bytes").expect("install third-party file");
            },
        );
        let error = result.expect_err("a second swap makes the outcome ambiguous");
        assert!(error.to_string().contains("prior file retained"));

        assert_eq!(
            fs::read(&path).expect("third-party file remains"),
            b"third-party bytes"
        );
        assert_eq!(
            fs::read(&installed_elsewhere).expect("mesh replacement remains"),
            b"mesh replacement"
        );
        let retained = fs::read_dir(&parent)
            .expect("read parent")
            .filter_map(Result::ok)
            .find(|entry| entry.file_name().to_string_lossy().contains(".mesh-edit-"))
            .expect("prior file retained under the temporary name");
        assert_eq!(
            fs::read(retained.path()).expect("retained prior bytes"),
            b"managed bytes"
        );
        fs::remove_dir_all(parent).expect("cleanup");
    }

    #[test]
    fn replacement_commit_barrier_failure_restores_the_original_identity() {
        let parent = std::env::temp_dir().join(format!(
            "mesh-atomic-replace-commit-barrier-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&parent);
        fs::create_dir_all(&parent).expect("scratch parent");
        let path = parent.join("file.txt");
        fs::write(&path, b"managed bytes").expect("managed bytes");
        let (target, expected) =
            read_managed_replacement(&parent, "file.txt").expect("inspect managed target");

        atomic_replace_with_durability_hooks(
            &target,
            &expected,
            b"mesh replacement",
            target.mode,
            |_| Ok(()),
            ReplacementDurabilityHooks {
                before_exchange: || {},
                after_exchange: || {},
                commit_directory: fail_commit_barrier,
                cleanup_directory: File::sync_all,
            },
        )
        .expect_err("a failed commit barrier must report failure after rollback");

        assert_eq!(fs::read(&path).expect("original remains"), b"managed bytes");
        let restored_file = File::open(&path).expect("open restored file");
        let restored = restored_file.metadata().expect("restored identity");
        assert_eq!(
            managed_file_identity(&restored_file, &restored).expect("restored file identity"),
            target.file
        );
        assert_eq!(restored.mode(), target.mode);
        assert_eq!(fs::read_dir(&parent).expect("read parent").count(), 1);
        fs::remove_dir_all(parent).expect("cleanup");
    }

    #[test]
    fn replacement_cleanup_barrier_failure_does_not_report_a_false_failure() {
        let parent = std::env::temp_dir().join(format!(
            "mesh-atomic-replace-cleanup-barrier-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&parent);
        fs::create_dir_all(&parent).expect("scratch parent");
        let path = parent.join("file.txt");
        fs::write(&path, b"managed bytes").expect("managed bytes");
        let (target, expected) =
            read_managed_replacement(&parent, "file.txt").expect("inspect managed target");

        let replacement = atomic_replace_with_durability_hooks(
            &target,
            &expected,
            b"mesh replacement",
            target.mode,
            |_| Ok(()),
            ReplacementDurabilityHooks {
                before_exchange: || {},
                after_exchange: || {},
                commit_directory: File::sync_all,
                cleanup_directory: fail_cleanup_barrier,
            },
        )
        .expect("the durable replacement must not be reported as failed");

        assert_eq!(
            fs::read(&path).expect("replacement remains"),
            b"mesh replacement"
        );
        let installed_file = File::open(&path).expect("open installed file");
        let installed = installed_file.metadata().expect("installed identity");
        assert_eq!(
            managed_file_identity(&installed_file, &installed).expect("installed file identity"),
            replacement.file
        );
        assert_ne!(replacement.file, target.file);
        assert_eq!(fs::read_dir(&parent).expect("read parent").count(), 1);
        fs::remove_dir_all(parent).expect("cleanup");
    }

    #[test]
    fn move_does_not_overwrite_a_target_created_after_preflight() {
        let parent = std::env::temp_dir().join(format!(
            "mesh-move-noreplace-race-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&parent);
        fs::create_dir_all(&parent).expect("scratch parent");
        let from = parent.join("from.txt");
        let to = parent.join("to.txt");
        fs::write(&from, b"managed source").expect("source");

        let raced_target = to.clone();
        BEFORE_RENAME_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                fs::write(raced_target, b"external target").expect("racing target");
            }));
        });

        let source_file = File::open(&from).expect("open source");
        let source_metadata = source_file.metadata().expect("source metadata");
        let expected =
            managed_file_identity(&source_file, &source_metadata).expect("source identity");
        assert!(move_managed_entry(&parent, "from.txt", "to.txt", expected, false, false).is_err());
        assert_eq!(
            fs::read(&from).expect("source preserved"),
            b"managed source"
        );
        assert_eq!(fs::read(&to).expect("target preserved"), b"external target");
        fs::remove_dir_all(parent).expect("cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn move_keeps_opened_parent_authority_when_the_path_is_redirected_after_validation() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "mesh-move-parent-race-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let managed = root.join("managed");
        let admitted_ancestor = managed.join("team");
        let displaced_ancestor = root.join("displaced-team");
        let outside = root.join("outside");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(admitted_ancestor.join("docs")).expect("managed ancestor");
        fs::create_dir_all(outside.join("docs")).expect("outside ancestor");
        fs::write(admitted_ancestor.join("docs/source.txt"), b"managed source")
            .expect("managed source");
        fs::write(outside.join("docs/source.txt"), b"outside source").expect("outside source");
        let source_path = admitted_ancestor.join("docs/source.txt");
        let source_file = File::open(&source_path).expect("open managed source");
        let source_metadata = source_file.metadata().expect("managed metadata");
        let expected =
            managed_file_identity(&source_file, &source_metadata).expect("managed identity");

        let admitted_for_race = admitted_ancestor.clone();
        let admitted_link_for_race = admitted_ancestor.clone();
        let displaced_for_race = displaced_ancestor.clone();
        let outside_for_race = outside.clone();
        BEFORE_RENAME_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                fs::rename(admitted_for_race, displaced_for_race)
                    .expect("displace admitted ancestor after validation");
                symlink(outside_for_race, admitted_link_for_race)
                    .expect("redirect admitted path after validation");
            }));
        });

        move_managed_entry(
            &managed,
            "team/docs/source.txt",
            "team/docs/moved.txt",
            expected,
            false,
            false,
        )
        .expect("move remains bound to the opened managed directory");
        assert_eq!(
            fs::read(outside.join("docs/source.txt")).expect("outside source remains"),
            b"outside source"
        );
        assert!(!outside.join("docs/moved.txt").exists());
        assert!(!displaced_ancestor.join("docs/source.txt").exists());
        assert_eq!(
            fs::read(displaced_ancestor.join("docs/moved.txt")).expect("managed move completed"),
            b"managed source"
        );

        fs::remove_file(&admitted_ancestor).expect("remove redirect");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn create_does_not_overwrite_a_target_created_after_preflight() {
        let parent = std::env::temp_dir().join(format!(
            "mesh-create-noreplace-race-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&parent);
        fs::create_dir_all(&parent).expect("scratch parent");
        let target = parent.join("created.txt");

        let raced_target = target.clone();
        BEFORE_RENAME_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                fs::write(raced_target, b"external target").expect("racing target");
            }));
        });

        assert!(create_text_file(&parent, "created.txt", b"managed source").is_err());
        assert_eq!(
            fs::read(&target).expect("target preserved"),
            b"external target"
        );
        assert!(
            fs::read_dir(&parent).expect("parent").all(|entry| !entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .contains("mesh-create")),
            "failed creation left its private temporary file behind"
        );
        fs::remove_dir_all(parent).expect("cleanup");
    }

    #[test]
    fn export_create_stays_bound_to_the_exact_existing_parent() {
        let root = std::env::temp_dir().join(format!(
            "mesh-export-parent-race-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let target_parent = root.join("docs");
        let target = target_parent.join("agent-note.txt");
        let displaced = root.join("displaced-docs");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&target_parent).expect("target parent");
        let expected_parent = match inspect_managed_export_target(&root, "docs/agent-note.txt")
            .expect("absent export target")
        {
            ManagedExportTarget::Missing { parent, .. } => parent,
            ManagedExportTarget::Existing(_, _) => panic!("target unexpectedly existed"),
        };

        fs::rename(&target_parent, &displaced).expect("displace previewed parent");
        fs::create_dir(&target_parent).expect("replacement parent");
        fs::write(target_parent.join("ordinary.txt"), b"ordinary bytes")
            .expect("replacement contents");
        assert!(create_export_file_in_exact_parent(
            &target,
            b"agent bytes",
            false,
            expected_parent,
        )
        .is_err());
        assert!(!target.exists());
        assert_eq!(
            fs::read(target_parent.join("ordinary.txt")).expect("ordinary file preserved"),
            b"ordinary bytes"
        );
        assert!(
            fs::read_dir(&target_parent)
                .expect("replacement parent")
                .all(|entry| !entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .contains("mesh-export")),
            "refused export left a temporary file in the replacement parent"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn failed_export_provenance_preparation_changes_no_destination_name() {
        let root = std::env::temp_dir().join(format!(
            "mesh-export-provenance-preparation-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("root");
        fs::write(root.join("existing.txt"), b"ordinary bytes").expect("existing file");

        let (existing, expected) =
            read_managed_replacement(&root, "existing.txt").expect("existing target");
        atomic_replace_with_mode_prepared(
            &existing,
            &expected,
            b"saved Mesh bytes",
            existing.mode,
            |_| Err(io::Error::other("receipt unavailable")),
        )
        .expect_err("replacement must stop before exchange");
        assert_eq!(
            fs::read(root.join("existing.txt")).expect("original remains"),
            b"ordinary bytes"
        );

        let (new_file, file_parent) =
            match inspect_managed_export_target(&root, "new.txt").expect("missing file target") {
                ManagedExportTarget::Missing { path, parent } => (path, parent),
                ManagedExportTarget::Existing(_, _) => panic!("new file unexpectedly exists"),
            };
        create_export_file_in_exact_parent_prepared(
            &new_file,
            b"saved Mesh bytes",
            false,
            file_parent,
            |_| Err(io::Error::other("receipt unavailable")),
        )
        .expect_err("create must stop before publication");
        assert!(!new_file.exists());

        let (new_directory, directory_parent) =
            match inspect_managed_directory_export_target(&root, "new-folder")
                .expect("missing directory target")
            {
                ManagedDirectoryExportTarget::Missing { path, parent } => (path, parent),
                ManagedDirectoryExportTarget::Existing { .. } => {
                    panic!("new directory unexpectedly exists")
                }
            };
        create_export_directory_in_exact_parent_prepared(&new_directory, directory_parent, |_| {
            Err(io::Error::other("receipt unavailable"))
        })
        .expect_err("directory create must stop before publication");
        assert!(!new_directory.exists());
        assert_eq!(
            fs::read_dir(&root).expect("destination root").count(),
            1,
            "failed preparation left a temporary export entry"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn durable_prepared_export_recovers_after_process_loss_without_trusting_lookalikes() {
        use std::cell::Cell;
        use std::panic::{catch_unwind, AssertUnwindSafe};

        let root = std::env::temp_dir().join(format!(
            "mesh-export-prepared-recovery-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("root");
        let target = root.join("agent-result.txt");
        let parent = match inspect_managed_export_target(&root, "agent-result.txt")
            .expect("missing target")
        {
            ManagedExportTarget::Missing { parent, .. } => parent,
            ManagedExportTarget::Existing(_, _) => panic!("target unexpectedly exists"),
        };
        let prepared_identity = Cell::new(None);

        let stopped = catch_unwind(AssertUnwindSafe(|| {
            let _ = create_export_file_in_exact_parent_prepared(
                &target,
                b"durably reviewed agent result\n",
                false,
                parent,
                |identity| -> io::Result<()> {
                    prepared_identity.set(Some(identity));
                    panic!("simulated process loss after durable provenance");
                },
            );
        }));
        assert!(stopped.is_err());
        assert!(!target.exists(), "the reviewed name was not yet published");
        let prepared_identity = prepared_identity.get().expect("prepared identity");

        let lookalike = root.join(format!(
            ".agent-result.txt.mesh-export-lookalike-{}",
            std::process::id()
        ));
        fs::write(&lookalike, b"durably reviewed agent result\n").expect("lookalike");
        assert!(!recover_prepared_export_file_in_exact_parent(
            &target,
            b"durably reviewed agent result\n",
            false,
            parent,
            |_| false,
        )
        .expect("unproven recovery is a no-op"));
        assert!(lookalike.exists(), "an unproven lookalike is preserved");

        assert!(recover_prepared_export_file_in_exact_parent(
            &target,
            b"durably reviewed agent result\n",
            false,
            parent,
            |identity| identity == prepared_identity,
        )
        .expect("recover exact prepared export"));
        assert_eq!(
            fs::read(&target).expect("published target"),
            b"durably reviewed agent result\n"
        );
        assert!(
            lookalike.exists(),
            "recovery never removes unrelated entries"
        );
        assert!(
            fs::read_dir(&root)
                .expect("root")
                .filter_map(Result::ok)
                .all(|entry| {
                    let name = entry.file_name();
                    name == "agent-result.txt" || name == lookalike.file_name().unwrap()
                }),
            "the exact prepared name was consumed"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn durable_prepared_directory_recovers_only_while_exact_and_empty() {
        use std::cell::Cell;
        use std::panic::{catch_unwind, AssertUnwindSafe};

        let root = std::env::temp_dir().join(format!(
            "mesh-export-directory-recovery-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("root");
        let target = root.join("generated");
        let parent = match inspect_managed_directory_export_target(&root, "generated")
            .expect("missing target")
        {
            ManagedDirectoryExportTarget::Missing { parent, .. } => parent,
            ManagedDirectoryExportTarget::Existing { .. } => {
                panic!("target unexpectedly exists")
            }
        };
        let prepared_identity = Cell::new(None);

        let stopped = catch_unwind(AssertUnwindSafe(|| {
            let _ = create_export_directory_in_exact_parent_prepared(
                &target,
                parent,
                |identity| -> io::Result<()> {
                    prepared_identity.set(Some(identity));
                    panic!("simulated process loss after directory provenance");
                },
            );
        }));
        assert!(stopped.is_err());
        assert!(!target.exists(), "the reviewed name was not yet published");
        let prepared_identity = prepared_identity.get().expect("prepared identity");
        let prepared_path = fs::read_dir(&root)
            .expect("root")
            .filter_map(Result::ok)
            .find_map(|entry| {
                let metadata = entry.metadata().ok()?;
                (metadata.dev() == prepared_identity.device
                    && metadata.ino() == prepared_identity.inode)
                    .then_some(entry.path())
            })
            .expect("prepared directory");

        let lookalike = root.join(format!(
            ".generated.mesh-export-lookalike-{}",
            std::process::id()
        ));
        fs::create_dir(&lookalike).expect("lookalike");
        assert_eq!(
            recover_prepared_export_directory_in_exact_parent(&target, parent, |_| false)
                .expect("unproven recovery is a no-op"),
            None
        );
        assert!(lookalike.exists(), "an unproven lookalike is preserved");

        let injected = prepared_path.join("unreviewed.txt");
        fs::write(&injected, b"must not be published").expect("injected entry");
        assert!(
            recover_prepared_export_directory_in_exact_parent(&target, parent, |identity| identity
                == prepared_identity,)
            .is_err()
        );
        assert!(!target.exists(), "a nonempty prepared directory is refused");
        assert_eq!(
            fs::read(&injected).expect("injected entry retained"),
            b"must not be published"
        );

        fs::remove_file(&injected).expect("restore exact empty preparation");
        assert_eq!(
            recover_prepared_export_directory_in_exact_parent(&target, parent, |identity| identity
                == prepared_identity,)
            .expect("recover exact prepared directory"),
            Some(prepared_identity)
        );
        assert!(target.is_dir(), "the receipt-bound directory is published");
        assert!(
            lookalike.exists(),
            "recovery never removes unrelated entries"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn export_cleanup_never_unlinks_a_file_that_wins_the_final_race() {
        let root = std::env::temp_dir().join(format!(
            "mesh-export-remove-file-race-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let path = root.join("old.txt");
        let displaced = root.join("displaced-old.txt");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("root");
        fs::write(&path, b"saved old bytes").expect("old file");
        let target = match inspect_managed_export_target(&root, "old.txt").expect("inspect") {
            ManagedExportTarget::Existing(target, _) => target,
            ManagedExportTarget::Missing { .. } => panic!("old file unexpectedly absent"),
        };
        let raced_path = path.clone();
        let raced_displaced = displaced.clone();
        BEFORE_RENAME_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                fs::rename(&raced_path, &raced_displaced).expect("displace reviewed file");
                fs::write(&raced_path, b"new ordinary work").expect("racing replacement");
            }));
        });

        assert!(remove_exact_export_file(&target, b"saved old bytes").is_err());
        assert_eq!(fs::read(&path).unwrap(), b"new ordinary work");
        assert_eq!(fs::read(&displaced).unwrap(), b"saved old bytes");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn export_cleanup_never_recursively_removes_or_unlinks_a_replacement_directory() {
        let root = std::env::temp_dir().join(format!(
            "mesh-export-remove-directory-race-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let path = root.join("old");
        let displaced = root.join("displaced-old");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&path).expect("old directory");
        fs::write(path.join("keep.txt"), b"unrelated").expect("unrelated contents");
        let (parent, directory) = match inspect_managed_directory_export_target(&root, "old")
            .expect("inspect directory")
        {
            ManagedDirectoryExportTarget::Existing {
                parent, directory, ..
            } => (parent, directory),
            ManagedDirectoryExportTarget::Missing { .. } => panic!("old directory absent"),
        };

        // Even the exact directory is restored when it is non-empty; no recursive primitive exists.
        assert!(remove_exact_export_directory(&root, "old", parent, directory).is_err());
        assert_eq!(fs::read(path.join("keep.txt")).unwrap(), b"unrelated");

        let (parent, directory) = match inspect_managed_directory_export_target(&root, "old")
            .expect("reinspect directory")
        {
            ManagedDirectoryExportTarget::Existing {
                parent, directory, ..
            } => (parent, directory),
            ManagedDirectoryExportTarget::Missing { .. } => panic!("old directory absent"),
        };
        fs::remove_file(path.join("keep.txt")).expect("make reviewed directory empty");
        let raced_path = path.clone();
        let raced_displaced = displaced.clone();
        BEFORE_RENAME_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                fs::rename(&raced_path, &raced_displaced).expect("displace reviewed directory");
                fs::create_dir(&raced_path).expect("racing replacement directory");
                fs::write(raced_path.join("keep.txt"), b"replacement contents")
                    .expect("replacement contents");
            }));
        });
        assert!(remove_exact_export_directory(&root, "old", parent, directory).is_err());
        assert_eq!(
            fs::read(path.join("keep.txt")).unwrap(),
            b"replacement contents"
        );
        assert!(displaced.is_dir());
        fs::remove_dir_all(root).expect("cleanup");
    }
}
