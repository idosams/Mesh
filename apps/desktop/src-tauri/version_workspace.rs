//! Owner-only destinations for one-click historical workspace checkouts.
//!
//! The webview chooses a daemon-authenticated operation, never a filesystem path. The native
//! host derives an absent destination below its private application-data directory. A person may
//! still choose a custom destination explicitly, but the normal version-switch journey does not
//! expose private storage layout as a required product decision.

use std::ffi::{CString, OsStr, OsString};
use std::fmt;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::os::fd::{AsRawFd as _, FromRawFd as _};
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::{
    DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const DIRECTORY_NAME: &str = "workspace-versions";
const MAX_COLLISIONS: usize = 64;
const SOURCE_MARKER_NAME: &str = ".mesh-source-version";
const SOURCE_MARKER_PREFIX: &str = "mesh.workspace-version-source/1\n";
const RECOVERY_MARKER_PREFIX: &str = ".mesh-source-version.recovered-";
static MARKER_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[cfg(target_os = "macos")]
const OPEN_DIRECTORY_FLAGS: i32 = 0x0110_0100;
#[cfg(target_os = "linux")]
const OPEN_DIRECTORY_FLAGS: i32 = 0x000b_0000;

#[cfg(target_os = "macos")]
const OPEN_READ_FLAGS: i32 = 0x0100_0100;
#[cfg(target_os = "linux")]
const OPEN_READ_FLAGS: i32 = 0x000a_0000;

#[cfg(target_os = "macos")]
const CREATE_NEW_FLAGS: i32 = 0x0100_0b01;
#[cfg(target_os = "linux")]
const CREATE_NEW_FLAGS: i32 = 0x0008_00c1;

#[cfg(test)]
thread_local! {
    static BEFORE_SOURCE_MARKER_OPEN: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        std::cell::RefCell::new(None);
}

/// App-owned directory used for independently editable historical workspaces.
#[derive(Clone, Debug)]
pub struct VersionWorkspaceDirectory {
    directory: PathBuf,
}

impl VersionWorkspaceDirectory {
    /// Place app-managed versions below the native host's private application-data directory.
    #[must_use]
    pub fn new(application_data: &Path) -> Self {
        Self {
            directory: application_data.join(DIRECTORY_NAME),
        }
    }

    /// Allocate an absent, human-inspectable destination for one exact operation digest.
    pub fn allocate(&self, operation: &str) -> Result<PathBuf, VersionWorkspaceError> {
        self.allocate_named("point", operation)
    }

    /// Allocate an absent app-managed destination for a first folder import.
    ///
    /// The verified import summary is a content identity, not a caller-controlled path. Keeping
    /// first imports beside historical workspace copies means the ordinary alpha journey never
    /// asks a person to choose where Mesh's private database, journal, and CAS should live.
    pub fn allocate_import(&self, summary: &str) -> Result<PathBuf, VersionWorkspaceError> {
        self.allocate_named("workspace", summary)
    }

    /// Existing real app-owned destinations whose name could belong to this full import summary.
    ///
    /// A name is only a bounded lookup hint. The caller must still validate the canonical import
    /// receipt, complete managed workspace and exact source-identity receipts before adopting a
    /// candidate. This method deliberately excludes links and non-directories.
    pub fn import_recovery_candidates(
        &self,
        summary: &str,
    ) -> Result<Vec<PathBuf>, VersionWorkspaceError> {
        validate_operation(summary)?;
        self.ensure_private_directory()?;
        let short = &summary[..12];
        let mut candidates = Vec::new();
        for ordinal in 1..=MAX_COLLISIONS {
            let suffix = if ordinal == 1 {
                String::new()
            } else {
                format!("-{ordinal}")
            };
            let candidate = self
                .directory
                .join(format!("workspace-{short}{suffix}.mesh"));
            match fs::symlink_metadata(&candidate) {
                Ok(metadata)
                    if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() =>
                {
                    candidates.push(candidate);
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(VersionWorkspaceError::io(
                        "inspect existing imported workspace",
                        error,
                    ));
                }
            }
        }
        Ok(candidates)
    }

    fn allocate_named(
        &self,
        prefix: &str,
        identity: &str,
    ) -> Result<PathBuf, VersionWorkspaceError> {
        validate_operation(identity)?;
        self.ensure_private_directory()?;
        let short = &identity[..12];
        for ordinal in 1..=MAX_COLLISIONS {
            let suffix = if ordinal == 1 {
                String::new()
            } else {
                format!("-{ordinal}")
            };
            let candidate = self
                .directory
                .join(format!("{prefix}-{short}{suffix}.mesh"));
            match fs::symlink_metadata(&candidate) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(candidate);
                }
                Ok(_) => {}
                Err(error) => {
                    return Err(VersionWorkspaceError::io(
                        "inspect version destination",
                        error,
                    ));
                }
            }
        }
        Err(VersionWorkspaceError::Invalid(
            "too many app-managed copies exist for this saved point",
        ))
    }

    /// Existing app-owned checkouts that may contain this complete source operation.
    ///
    /// Exact full-digest markers are returned first. A real app-owned directory whose marker is
    /// absent or contains an interrupted, non-canonical owner-only write is returned only as a
    /// recovery candidate: the native host can stop after creating and opening a valid checkout
    /// but before the lookup hint is durable. The daemon must still prove the candidate's complete
    /// history, CAS content, native tree and recovery state before reuse. After that proof the
    /// caller may publish a separate create-only recovery hint; it never replaces the interrupted
    /// primary marker by pathname. A canonical primary marker for another full operation, or any
    /// shared or linked marker, remains excluded even if a recovery hint also exists.
    pub fn reuse_candidates_for(
        &self,
        operation: &str,
    ) -> Result<Vec<PathBuf>, VersionWorkspaceError> {
        validate_operation(operation)?;
        self.ensure_private_directory()?;
        let expected = source_marker_bytes(operation);
        let short = &operation[..12];
        let mut exact = Vec::new();
        let mut recoverable_marker = Vec::new();
        for ordinal in 1..=MAX_COLLISIONS {
            let suffix = if ordinal == 1 {
                String::new()
            } else {
                format!("-{ordinal}")
            };
            let candidate = self.directory.join(format!("point-{short}{suffix}.mesh"));
            let metadata = match fs::symlink_metadata(&candidate) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(VersionWorkspaceError::io(
                        "inspect existing version destination",
                        error,
                    ));
                }
            };
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                continue;
            }
            let marker = inspect_marker(&candidate.join(SOURCE_MARKER_NAME), &expected)?;
            match marker {
                MarkerInspection::Exact => exact.push(candidate),
                MarkerInspection::Missing | MarkerInspection::Interrupted => {
                    match inspect_marker(&recovery_marker(&candidate, operation), &expected)? {
                        MarkerInspection::Exact => exact.push(candidate),
                        MarkerInspection::Missing => recoverable_marker.push(candidate),
                        MarkerInspection::Interrupted
                        | MarkerInspection::CanonicalOther
                        | MarkerInspection::Unsafe => {}
                    }
                }
                MarkerInspection::CanonicalOther | MarkerInspection::Unsafe => {}
            }
        }
        exact.extend(recoverable_marker);
        Ok(exact)
    }

    /// Durably bind a newly created app-owned checkout to the complete source operation.
    pub fn record_source(
        &self,
        destination: &Path,
        operation: &str,
    ) -> Result<(), VersionWorkspaceError> {
        validate_operation(operation)?;
        self.ensure_private_directory()?;
        if destination.parent() != Some(self.directory.as_path()) {
            return Err(VersionWorkspaceError::Invalid(
                "version destination is outside app-owned storage",
            ));
        }
        let opened = OpenedVersionDirectory::open(&self.directory, destination)?;
        #[cfg(test)]
        BEFORE_SOURCE_MARKER_OPEN.with(|hook| {
            if let Some(hook) = hook.borrow_mut().take() {
                hook();
            }
        });
        let expected = source_marker_bytes(operation);
        let publication = match openat(
            &opened.directory,
            OsStr::new(SOURCE_MARKER_NAME),
            CREATE_NEW_FLAGS,
            0o600,
        ) {
            Ok(file) => write_new_marker(file, &opened.directory, &expected),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                match inspect_marker_at(
                    &opened.directory,
                    OsStr::new(SOURCE_MARKER_NAME),
                    &expected,
                )? {
                    MarkerInspection::Exact => Ok(()),
                    MarkerInspection::Interrupted => {
                        publish_recovery_marker(&opened.directory, operation, &expected)
                    }
                    MarkerInspection::CanonicalOther => Err(VersionWorkspaceError::Invalid(
                        "version source marker names another saved point",
                    )),
                    MarkerInspection::Missing => Err(VersionWorkspaceError::Invalid(
                        "version source marker disappeared during publication",
                    )),
                    MarkerInspection::Unsafe => Err(VersionWorkspaceError::Invalid(
                        "version source marker is not canonical",
                    )),
                }
            }
            Err(error) => Err(VersionWorkspaceError::io(
                "create version source marker",
                error,
            )),
        };
        publication?;
        opened.ensure_current()
    }

    fn ensure_private_directory(&self) -> Result<(), VersionWorkspaceError> {
        match fs::symlink_metadata(&self.directory) {
            Ok(metadata) => validate_private_directory(&metadata),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let parent = self
                    .directory
                    .parent()
                    .ok_or(VersionWorkspaceError::Invalid(
                        "version directory has no application-data parent",
                    ))?;
                let parent_metadata = fs::symlink_metadata(parent).map_err(|error| {
                    VersionWorkspaceError::io("inspect application-data directory", error)
                })?;
                if !parent_metadata.file_type().is_dir() || parent_metadata.file_type().is_symlink()
                {
                    return Err(VersionWorkspaceError::Invalid(
                        "application-data parent is not a real directory",
                    ));
                }
                DirBuilder::new()
                    .mode(0o700)
                    .create(&self.directory)
                    .map_err(|error| {
                        VersionWorkspaceError::io("create version directory", error)
                    })?;
                let metadata = fs::symlink_metadata(&self.directory).map_err(|error| {
                    VersionWorkspaceError::io("verify version directory", error)
                })?;
                validate_private_directory(&metadata)
            }
            Err(error) => Err(VersionWorkspaceError::io(
                "inspect version directory",
                error,
            )),
        }
    }
}

/// Retained authority to one exact direct-child version directory.
///
/// Paths remain useful display names, but every marker mutation is resolved from these retained
/// descriptors. A same-user process may rename either directory while publication is underway;
/// in that case writes remain confined to the originally admitted object and the final namespace
/// check refuses success.
struct OpenedVersionDirectory {
    parent_path: PathBuf,
    parent: File,
    parent_identity: (u64, u64),
    directory_path: PathBuf,
    directory: File,
    directory_identity: (u64, u64),
}

impl OpenedVersionDirectory {
    fn open(parent_path: &Path, directory_path: &Path) -> Result<Self, VersionWorkspaceError> {
        let leaf = directory_path
            .file_name()
            .filter(|name| !name.is_empty() && !name.as_bytes().contains(&b'/'))
            .ok_or(VersionWorkspaceError::Invalid(
                "version destination is not one direct child",
            ))?;
        let parent = OpenOptions::new()
            .read(true)
            .custom_flags(OPEN_DIRECTORY_FLAGS)
            .open(parent_path)
            .map_err(|error| VersionWorkspaceError::io("open version directory", error))?;
        let parent_metadata = parent
            .metadata()
            .map_err(|error| VersionWorkspaceError::io("inspect version directory", error))?;
        validate_private_directory(&parent_metadata)?;
        let parent_identity = (parent_metadata.dev(), parent_metadata.ino());

        let directory = openat(&parent, leaf, OPEN_DIRECTORY_FLAGS, 0)
            .map_err(|error| VersionWorkspaceError::io("open created version", error))?;
        let directory_metadata = directory
            .metadata()
            .map_err(|error| VersionWorkspaceError::io("inspect created version", error))?;
        if !directory_metadata.is_dir() {
            return Err(VersionWorkspaceError::Invalid(
                "created version is not a real directory",
            ));
        }
        let directory_identity = (directory_metadata.dev(), directory_metadata.ino());
        let opened = Self {
            parent_path: parent_path.to_path_buf(),
            parent,
            parent_identity,
            directory_path: directory_path.to_path_buf(),
            directory,
            directory_identity,
        };
        opened.ensure_current()?;
        Ok(opened)
    }

    fn ensure_current(&self) -> Result<(), VersionWorkspaceError> {
        ensure_directory_identity(&self.parent, &self.parent_path, self.parent_identity, true)?;
        ensure_directory_identity(
            &self.directory,
            &self.directory_path,
            self.directory_identity,
            false,
        )
    }
}

fn ensure_directory_identity(
    descriptor: &File,
    path: &Path,
    expected: (u64, u64),
    owner_only: bool,
) -> Result<(), VersionWorkspaceError> {
    let descriptor_metadata = descriptor
        .metadata()
        .map_err(|error| VersionWorkspaceError::io("inspect retained version directory", error))?;
    let path_metadata = fs::symlink_metadata(path)
        .map_err(|error| VersionWorkspaceError::io("recheck version directory path", error))?;
    let descriptor_identity = (descriptor_metadata.dev(), descriptor_metadata.ino());
    let path_identity = (path_metadata.dev(), path_metadata.ino());
    if !descriptor_metadata.is_dir()
        || !path_metadata.file_type().is_dir()
        || path_metadata.file_type().is_symlink()
        || descriptor_identity != expected
        || path_identity != expected
        || (owner_only && path_metadata.permissions().mode() & 0o077 != 0)
    {
        return Err(VersionWorkspaceError::Invalid(
            "version directory changed during marker publication",
        ));
    }
    Ok(())
}

fn source_marker_bytes(operation: &str) -> Vec<u8> {
    format!("{SOURCE_MARKER_PREFIX}{operation}\n").into_bytes()
}

fn canonical_marker_operation(bytes: &[u8]) -> Option<&str> {
    let operation = bytes
        .strip_prefix(SOURCE_MARKER_PREFIX.as_bytes())?
        .strip_suffix(b"\n")?;
    if operation.len() != 64
        || !operation
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return None;
    }
    std::str::from_utf8(operation).ok()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MarkerInspection {
    Missing,
    Exact,
    CanonicalOther,
    Interrupted,
    Unsafe,
}

fn inspect_marker(
    marker: &Path,
    expected: &[u8],
) -> Result<MarkerInspection, VersionWorkspaceError> {
    let metadata = match fs::symlink_metadata(marker) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(MarkerInspection::Missing);
        }
        Err(error) => {
            return Err(VersionWorkspaceError::io(
                "inspect version source marker",
                error,
            ));
        }
    };
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Ok(MarkerInspection::Unsafe);
    }
    let Ok(bytes) = fs::read(marker) else {
        return Ok(MarkerInspection::Unsafe);
    };
    if bytes == expected {
        Ok(MarkerInspection::Exact)
    } else if canonical_marker_operation(&bytes).is_some() {
        Ok(MarkerInspection::CanonicalOther)
    } else {
        Ok(MarkerInspection::Interrupted)
    }
}

fn inspect_marker_at(
    directory: &File,
    name: &OsStr,
    expected: &[u8],
) -> Result<MarkerInspection, VersionWorkspaceError> {
    let mut file = match openat(directory, name, OPEN_READ_FLAGS, 0) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(MarkerInspection::Missing);
        }
        Err(_) => return Ok(MarkerInspection::Unsafe),
    };
    let metadata = match file.metadata() {
        Ok(metadata) => metadata,
        Err(_) => return Ok(MarkerInspection::Unsafe),
    };
    if !metadata.is_file() || metadata.permissions().mode() & 0o077 != 0 {
        return Ok(MarkerInspection::Unsafe);
    }
    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() {
        return Ok(MarkerInspection::Unsafe);
    }
    if bytes == expected {
        Ok(MarkerInspection::Exact)
    } else if canonical_marker_operation(&bytes).is_some() {
        Ok(MarkerInspection::CanonicalOther)
    } else {
        Ok(MarkerInspection::Interrupted)
    }
}

fn recovery_marker(destination: &Path, operation: &str) -> PathBuf {
    destination.join(format!("{RECOVERY_MARKER_PREFIX}{operation}"))
}

fn write_new_marker(
    mut file: File,
    directory: &File,
    expected: &[u8],
) -> Result<(), VersionWorkspaceError> {
    file.write_all(expected)
        .and_then(|()| file.sync_all())
        .map_err(|error| VersionWorkspaceError::io("write version source marker", error))?;
    sync_marker_directory(directory)
}

fn publish_recovery_marker(
    directory: &File,
    operation: &str,
    expected: &[u8],
) -> Result<(), VersionWorkspaceError> {
    let marker = OsString::from(format!("{RECOVERY_MARKER_PREFIX}{operation}"));
    match inspect_marker_at(directory, &marker, expected)? {
        MarkerInspection::Exact => return Ok(()),
        MarkerInspection::Missing => {}
        MarkerInspection::Interrupted
        | MarkerInspection::CanonicalOther
        | MarkerInspection::Unsafe => {
            return Err(VersionWorkspaceError::Invalid(
                "version recovery marker is not canonical",
            ));
        }
    }
    let sequence = MARKER_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let pending = OsString::from(format!(
        ".{SOURCE_MARKER_NAME}.pending-{}-{sequence}",
        std::process::id()
    ));
    let mut file = openat(directory, &pending, CREATE_NEW_FLAGS, 0o600)
        .map_err(|error| VersionWorkspaceError::io("create repaired source marker", error))?;
    if let Err(error) = file.write_all(expected).and_then(|()| file.sync_all()) {
        let _ = unlinkat(directory, &pending);
        return Err(VersionWorkspaceError::io(
            "write repaired source marker",
            error,
        ));
    }
    drop(file);
    let publish = linkat(directory, &pending, &marker);
    let cleanup = unlinkat(directory, &pending);
    if let Err(error) = publish {
        if error.kind() == std::io::ErrorKind::AlreadyExists
            && inspect_marker_at(directory, &marker, expected)? == MarkerInspection::Exact
        {
            if cleanup
                .is_err_and(|cleanup_error| cleanup_error.kind() != std::io::ErrorKind::NotFound)
            {
                return Err(VersionWorkspaceError::Invalid(
                    "version recovery marker temporary file could not be removed",
                ));
            }
            return sync_marker_directory(directory);
        }
        return Err(VersionWorkspaceError::io(
            "publish recovered source marker",
            error,
        ));
    }
    cleanup.map_err(|error| {
        VersionWorkspaceError::io("remove recovered source marker temporary file", error)
    })?;
    sync_marker_directory(directory)
}

fn sync_marker_directory(directory: &File) -> Result<(), VersionWorkspaceError> {
    directory
        .sync_all()
        .map_err(|error| VersionWorkspaceError::io("sync version source marker", error))
}

#[allow(unsafe_code, clashing_extern_declarations)]
fn openat(directory: &File, name: &OsStr, flags: i32, mode: i32) -> std::io::Result<File> {
    unsafe extern "C" {
        #[link_name = "openat"]
        fn openat_with_mode(directory: i32, path: *const std::ffi::c_char, flags: i32, ...) -> i32;
    }
    let name = c_name(name)?;
    // SAFETY: `name` is a live C string, `directory` is an owned descriptor, and a successful
    // call returns one new descriptor transferred into `File` exactly once.
    let descriptor = unsafe { openat_with_mode(directory.as_raw_fd(), name.as_ptr(), flags, mode) };
    if descriptor < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: successful `openat` returned a fresh owned descriptor.
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

#[allow(unsafe_code)]
fn linkat(directory: &File, from: &OsStr, to: &OsStr) -> std::io::Result<()> {
    unsafe extern "C" {
        fn linkat(
            from_directory: i32,
            from: *const std::ffi::c_char,
            to_directory: i32,
            to: *const std::ffi::c_char,
            flags: i32,
        ) -> i32;
    }
    let from = c_name(from)?;
    let to = c_name(to)?;
    // SAFETY: both names are live C strings and both resolve in the retained directory.
    if unsafe {
        linkat(
            directory.as_raw_fd(),
            from.as_ptr(),
            directory.as_raw_fd(),
            to.as_ptr(),
            0,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[allow(unsafe_code)]
fn unlinkat(directory: &File, name: &OsStr) -> std::io::Result<()> {
    unsafe extern "C" {
        fn unlinkat(directory: i32, path: *const std::ffi::c_char, flags: i32) -> i32;
    }
    let name = c_name(name)?;
    // SAFETY: `name` is a live C string and flags=0 removes only that non-directory entry.
    if unsafe { unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

fn c_name(name: &OsStr) -> std::io::Result<CString> {
    if name.is_empty() || name.as_bytes().contains(&b'/') {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "name is not one ordinary component",
        ));
    }
    CString::new(name.as_bytes())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "name contains NUL"))
}

fn validate_operation(operation: &str) -> Result<(), VersionWorkspaceError> {
    if operation.len() != 64
        || !operation
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(VersionWorkspaceError::Invalid(
            "saved point identity is not a canonical digest",
        ));
    }
    Ok(())
}

fn validate_private_directory(metadata: &fs::Metadata) -> Result<(), VersionWorkspaceError> {
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(VersionWorkspaceError::Invalid(
            "version directory is not a real directory",
        ));
    }
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(VersionWorkspaceError::Invalid(
            "version directory is not owner-only",
        ));
    }
    Ok(())
}

/// Why an app-owned version destination could not be allocated safely.
#[derive(Debug)]
pub enum VersionWorkspaceError {
    Invalid(&'static str),
    Io {
        context: &'static str,
        source: std::io::Error,
    },
}

impl VersionWorkspaceError {
    fn io(context: &'static str, source: std::io::Error) -> Self {
        Self::Io { context, source }
    }
}

impl fmt::Display for VersionWorkspaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(reason) => write!(formatter, "version workspace refused: {reason}"),
            Self::Io { context, source } => {
                write!(formatter, "version workspace refused: {context}: {source}")
            }
        }
    }
}

impl std::error::Error for VersionWorkspaceError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "mesh-desktop-version-workspace-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn app_managed_destinations_are_private_stable_and_collision_free() {
        let application = scratch("allocate");
        fs::create_dir(&application).expect("application data");
        let versions = VersionWorkspaceDirectory::new(&application);
        let operation = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let first = versions.allocate(operation).expect("first destination");
        assert_eq!(first.file_name().unwrap(), "point-0123456789ab.mesh");
        let directory = first.parent().unwrap();
        assert_eq!(
            fs::metadata(directory).unwrap().permissions().mode() & 0o077,
            0
        );
        fs::create_dir(&first).expect("occupy first destination");
        versions
            .record_source(&first, operation)
            .expect("record complete source operation");
        assert_eq!(
            versions.reuse_candidates_for(operation).unwrap(),
            vec![first.clone()]
        );
        let second = versions.allocate(operation).expect("second destination");
        assert_eq!(second.file_name().unwrap(), "point-0123456789ab-2.mesh");

        let imported = versions
            .allocate_import("abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd")
            .expect("first-import destination");
        assert_eq!(imported.file_name().unwrap(), "workspace-abcdefabcdef.mesh");
        fs::create_dir(&imported).expect("occupy imported destination");
        assert_eq!(
            versions
                .import_recovery_candidates(
                    "abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd"
                )
                .unwrap(),
            vec![imported]
        );
        let _ = fs::remove_dir_all(application);
    }

    #[test]
    fn reuse_candidates_recover_only_missing_or_interrupted_markers() {
        let application = scratch("reuse-marker");
        fs::create_dir(&application).expect("application data");
        let versions = VersionWorkspaceDirectory::new(&application);
        let operation = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let first = versions.allocate(operation).expect("first destination");
        fs::create_dir(&first).expect("occupy first destination");
        assert_eq!(
            versions.reuse_candidates_for(operation).unwrap(),
            vec![first.clone()],
            "a stopped marker write must not strand an otherwise provable checkout"
        );
        versions
            .record_source(&first, operation)
            .expect("record source");
        assert_eq!(
            versions.reuse_candidates_for(operation).unwrap(),
            vec![first.clone()]
        );

        fs::write(
            first.join(SOURCE_MARKER_NAME),
            SOURCE_MARKER_PREFIX.as_bytes(),
        )
        .expect("leave interrupted marker bytes");
        assert_eq!(
            versions.reuse_candidates_for(operation).unwrap(),
            vec![first.clone()],
            "a partial owner-only marker remains a daemon-validated recovery candidate"
        );
        versions
            .record_source(&first, operation)
            .expect("publish create-only recovery marker");
        assert_eq!(
            fs::read(first.join(SOURCE_MARKER_NAME)).unwrap(),
            SOURCE_MARKER_PREFIX.as_bytes(),
            "recovery never replaces the interrupted primary marker by pathname"
        );
        let recovered = recovery_marker(&first, operation);
        assert_eq!(
            fs::read(&recovered).unwrap(),
            source_marker_bytes(operation)
        );
        let recovered_metadata = fs::symlink_metadata(&recovered).unwrap();
        assert!(recovered_metadata.is_file());
        assert!(!recovered_metadata.file_type().is_symlink());
        assert_eq!(recovered_metadata.permissions().mode() & 0o077, 0);
        assert_eq!(
            versions.reuse_candidates_for(operation).unwrap(),
            vec![first.clone()],
            "the operation-bound recovery marker is an exact lookup hint"
        );
        assert_eq!(
            fs::read_dir(&first)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().contains(".pending-"))
                .count(),
            0,
            "successful recovery publication leaves no temporary marker"
        );

        fs::write(
            first.join(SOURCE_MARKER_NAME),
            source_marker_bytes("abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd"),
        )
        .expect("insert a conflicting canonical primary marker");
        assert!(
            versions.reuse_candidates_for(operation).unwrap().is_empty(),
            "a canonical primary conflict wins over an earlier recovery hint"
        );
        assert!(versions.record_source(&first, operation).is_err());
        assert_eq!(
            fs::read(&recovered).unwrap(),
            source_marker_bytes(operation),
            "the operation-bound hint remains immutable"
        );

        fs::write(
            first.join(SOURCE_MARKER_NAME),
            SOURCE_MARKER_PREFIX.as_bytes(),
        )
        .expect("restore interrupted primary marker");
        fs::remove_file(&recovered).expect("remove exact recovery marker");
        fs::write(&recovered, b"interrupted competing recovery")
            .expect("occupy recovery marker non-canonically");
        let interrupted_recovery = fs::read(&recovered).unwrap();
        assert!(versions.reuse_candidates_for(operation).unwrap().is_empty());
        assert!(versions.record_source(&first, operation).is_err());
        assert_eq!(
            fs::read(&recovered).unwrap(),
            interrupted_recovery,
            "a preoccupied recovery marker is refused and never overwritten"
        );
        fs::remove_file(&recovered).expect("remove interrupted recovery marker");

        fs::set_permissions(
            first.join(SOURCE_MARKER_NAME),
            fs::Permissions::from_mode(0o644),
        )
        .expect("make marker shared");
        assert!(versions.reuse_candidates_for(operation).unwrap().is_empty());
        fs::set_permissions(
            first.join(SOURCE_MARKER_NAME),
            fs::Permissions::from_mode(0o600),
        )
        .expect("restore marker permissions");

        fs::write(
            first.join(SOURCE_MARKER_NAME),
            source_marker_bytes("abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd"),
        )
        .expect("replace marker bytes");
        assert!(versions.reuse_candidates_for(operation).unwrap().is_empty());
        let conflicting = fs::read(first.join(SOURCE_MARKER_NAME)).unwrap();
        assert!(versions.record_source(&first, operation).is_err());
        assert_eq!(
            fs::read(first.join(SOURCE_MARKER_NAME)).unwrap(),
            conflicting,
            "a canonical marker for another full operation is never repaired or overwritten"
        );

        let displaced = application.join("displaced");
        fs::rename(&first, &displaced).expect("displace marked checkout");
        symlink(&displaced, &first).expect("replace checkout with link");
        assert!(versions.reuse_candidates_for(operation).unwrap().is_empty());
        let _ = fs::remove_dir_all(application);
    }

    #[test]
    fn malformed_operations_and_unsafe_version_directories_are_refused() {
        let application = scratch("refuse");
        let outside = scratch("outside");
        fs::create_dir(&application).expect("application data");
        fs::create_dir(&outside).expect("outside");
        let versions = VersionWorkspaceDirectory::new(&application);
        assert!(versions.allocate("not-a-digest").is_err());
        symlink(&outside, application.join(DIRECTORY_NAME)).expect("linked versions");
        assert!(versions
            .allocate("abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd")
            .is_err());
        assert!(!outside.join("point-abcdefabcdef.mesh").exists());
        let _ = fs::remove_dir_all(application);
        let _ = fs::remove_dir_all(outside);
    }

    #[test]
    fn allocated_version_cannot_escape_when_its_private_parent_is_replaced() {
        let root = scratch("parent-replacement");
        let application = root.join("application");
        let source = root.join("source");
        let outside = root.join("outside");
        fs::create_dir_all(&application).expect("application data");
        fs::create_dir(&source).expect("source");
        fs::create_dir(&outside).expect("outside");
        fs::write(source.join("work.txt"), b"private work\n").expect("source bytes");

        let versions = VersionWorkspaceDirectory::new(&application);
        let operation = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let destination = versions.allocate(operation).expect("allocate destination");
        let private_parent = application.join(DIRECTORY_NAME);
        let displaced_parent = application.join("displaced-versions");
        fs::rename(&private_parent, &displaced_parent).expect("displace verified parent");
        symlink(&outside, &private_parent).expect("redirect allocated pathname");

        let result = mesh_daemon::PreparedFolderImport::prepare_presented(&source, &destination);
        assert!(
            result.is_err(),
            "a pathname allocated below a replaced private parent was accepted"
        );
        assert!(
            !outside.join(destination.file_name().unwrap()).exists(),
            "the saved workspace escaped into the replacement target"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn source_marker_cannot_escape_when_the_confirmed_version_is_replaced() {
        let root = scratch("source-marker-replacement");
        let application = root.join("application");
        let outside = root.join("outside");
        let displaced = root.join("displaced-version");
        fs::create_dir_all(&application).expect("application data");
        fs::create_dir(&outside).expect("outside directory");

        let versions = VersionWorkspaceDirectory::new(&application);
        let operation = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let destination = versions.allocate(operation).expect("allocate destination");
        fs::create_dir(&destination).expect("confirmed version directory");
        fs::write(
            destination.join(SOURCE_MARKER_NAME),
            SOURCE_MARKER_PREFIX.as_bytes(),
        )
        .expect("leave interrupted primary marker");
        fs::set_permissions(
            destination.join(SOURCE_MARKER_NAME),
            fs::Permissions::from_mode(0o600),
        )
        .expect("keep interrupted marker owner-only");
        let destination_for_hook = destination.clone();
        let displaced_for_hook = displaced.clone();
        let outside_for_hook = outside.clone();
        BEFORE_SOURCE_MARKER_OPEN.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                fs::rename(&destination_for_hook, &displaced_for_hook)
                    .expect("displace confirmed version");
                symlink(&outside_for_hook, &destination_for_hook)
                    .expect("redirect confirmed version path");
            }));
        });

        let result = versions.record_source(&destination, operation);
        assert!(
            result.is_err(),
            "a replacement version inherited source-marker publication"
        );
        assert!(
            !outside.join(SOURCE_MARKER_NAME).exists(),
            "the app-owned source marker escaped into the replacement directory"
        );
        assert_eq!(
            fs::read(recovery_marker(&displaced, operation))
                .expect("recovery marker remains confined to admitted directory"),
            source_marker_bytes(operation)
        );

        let _ = fs::remove_file(&destination);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn source_marker_cannot_escape_when_the_private_parent_is_replaced() {
        let root = scratch("source-marker-parent-replacement");
        let application = root.join("application");
        let outside = root.join("outside");
        let displaced_parent = root.join("displaced-versions");
        fs::create_dir_all(&application).expect("application data");
        fs::create_dir(&outside).expect("outside directory");

        let versions = VersionWorkspaceDirectory::new(&application);
        let operation = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let destination = versions.allocate(operation).expect("allocate destination");
        fs::create_dir(&destination).expect("confirmed version directory");
        let private_parent = application.join(DIRECTORY_NAME);
        let private_parent_for_hook = private_parent.clone();
        let displaced_for_hook = displaced_parent.clone();
        let outside_for_hook = outside.clone();
        BEFORE_SOURCE_MARKER_OPEN.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                fs::rename(&private_parent_for_hook, &displaced_for_hook)
                    .expect("displace private parent");
                symlink(&outside_for_hook, &private_parent_for_hook)
                    .expect("redirect private parent path");
            }));
        });

        let result = versions.record_source(&destination, operation);
        assert!(
            result.is_err(),
            "a replacement private parent inherited marker authority"
        );
        assert!(
            !outside.join(SOURCE_MARKER_NAME).exists()
                && !outside.join(destination.file_name().unwrap()).exists(),
            "marker publication escaped through the replacement parent"
        );
        assert_eq!(
            fs::read(
                displaced_parent
                    .join(destination.file_name().unwrap())
                    .join(SOURCE_MARKER_NAME)
            )
            .expect("marker remains confined to admitted directory"),
            source_marker_bytes(operation)
        );

        let _ = fs::remove_file(&private_parent);
        let _ = fs::remove_dir_all(root);
    }
}
