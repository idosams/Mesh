//! Descriptor-pinned filesystem authority for one opened workspace.
//!
//! A path is a name, not durable authority. Once a workspace is opened, another process can
//! rename that directory away and put a byte-identical copy at the old name. Path-based opens
//! would then let the copy inherit an already-authenticated mutation. This module retains the
//! admitted directory descriptor and resolves every child through `*at` syscalls relative to it.

use std::ffi::{CStr, CString, OsStr, OsString};
use std::fs::{File, OpenOptions};
use std::io::{self, Read as _, Write as _};
use std::os::fd::{AsRawFd as _, FromRawFd as _, IntoRawFd as _};
use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use mesh_cas::DurableFs;

#[cfg(target_os = "macos")]
const OPEN_DIRECTORY_FLAGS: i32 = 0x0110_0100;
#[cfg(target_os = "linux")]
const OPEN_DIRECTORY_FLAGS: i32 = 0x000b_0000;

#[cfg(target_os = "macos")]
const OPEN_READ_FLAGS: i32 = 0x0100_0100;
#[cfg(target_os = "linux")]
const OPEN_READ_FLAGS: i32 = 0x000a_0000;

// Opening a FIFO for inspection must not wait for a writer. The descriptor is classified with
// `fstat` before any bytes are read, so the flag has no effect on admitted regular files.
#[cfg(target_os = "macos")]
const OPEN_INSPECT_FLAGS: i32 = OPEN_READ_FLAGS | 0x0000_0004;
#[cfg(target_os = "linux")]
const OPEN_INSPECT_FLAGS: i32 = OPEN_READ_FLAGS | 0x0000_0800;

#[cfg(target_os = "macos")]
const OPEN_WRITE_FLAGS: i32 = 0x0100_0101;
#[cfg(target_os = "linux")]
const OPEN_WRITE_FLAGS: i32 = 0x000a_0001;

#[cfg(target_os = "macos")]
const CREATE_NEW_FLAGS: i32 = 0x0100_0b01;
#[cfg(target_os = "linux")]
const CREATE_NEW_FLAGS: i32 = 0x0008_00c1;

#[cfg(target_os = "macos")]
const APPEND_CREATE_FLAGS: i32 = 0x0100_030a;
#[cfg(target_os = "linux")]
const APPEND_CREATE_FLAGS: i32 = 0x000a_0442;

#[cfg(target_os = "macos")]
const APPEND_EXISTING_FLAGS: i32 = APPEND_CREATE_FLAGS & !0x0200;
#[cfg(target_os = "linux")]
const APPEND_EXISTING_FLAGS: i32 = APPEND_CREATE_FLAGS & !0x0040;

#[cfg(target_os = "macos")]
type NativeMode = u16;
#[cfg(target_os = "linux")]
type NativeMode = u32;

/// The exact directory object admitted by an explicit workspace open.
#[derive(Clone, Debug)]
pub(crate) struct PinnedWorkspaceRoot {
    namespace: PathBuf,
    directory: Arc<File>,
}

/// Stable kernel identity of one real directory that a new workspace must not enter.
///
/// Paths can be renamed between a desktop preflight and the daemon's create-only transaction.
/// This value lets the transaction compare the retained destination descriptor with the exact
/// directory object the desktop inspected, independently of its current pathname.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProtectedWorkspaceRoot {
    device: u64,
    inode: u64,
}

impl ProtectedWorkspaceRoot {
    /// Inspect an existing real directory without accepting a final symlink.
    pub fn inspect(path: &Path) -> io::Result<Self> {
        let metadata = std::fs::symlink_metadata(path)?;
        if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
            return Err(io::Error::other(
                "the protected workspace root is not a real directory",
            ));
        }
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }

    /// Decode the daemon's exact directory installation token retained for an agent handoff.
    pub fn from_directory_token(token: &str) -> io::Result<Self> {
        let (device, inode) = token.split_once(':').ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "installation has no separator")
        })?;
        if device.len() != 16 || inode.len() != 16 || inode.contains(':') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "installation is not an exact directory identity",
            ));
        }
        let device = u64::from_str_radix(device, 16).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "installation device is invalid",
            )
        })?;
        let inode = u64::from_str_radix(inode, 16).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "installation inode is invalid")
        })?;
        let identity = Self { device, inode };
        if identity.directory_token() != token {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "installation directory identity is not canonical",
            ));
        }
        Ok(identity)
    }

    /// Encode this exact directory object for an owner-only navigation record.
    #[must_use]
    pub fn directory_token(self) -> String {
        format!("{:016x}:{:016x}", self.device, self.inode)
    }

    /// Stable macOS path to this exact directory object, independent of its mutable namespace.
    pub fn stable_reference(self) -> io::Result<PathBuf> {
        #[cfg(target_os = "macos")]
        let reference = PathBuf::from(format!("/.vol/{}/{}", self.device, self.inode));
        #[cfg(not(target_os = "macos"))]
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "this platform has no supported persistent directory reference",
        ));

        #[cfg(target_os = "macos")]
        {
            let current = Self::inspect(&reference)?;
            if current == self {
                Ok(reference)
            } else {
                Err(io::Error::other(
                    "the stable reference does not name the admitted directory",
                ))
            }
        }
    }
}

impl PinnedWorkspaceRoot {
    /// Open an existing real directory without following a final symlink.
    pub(crate) fn open(namespace: PathBuf) -> io::Result<Self> {
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(OPEN_DIRECTORY_FLAGS)
            .open(&namespace)?;
        if !directory.metadata()?.is_dir() {
            return Err(io::Error::other("the workspace root is not a directory"));
        }
        Ok(Self {
            namespace,
            directory: Arc::new(directory),
        })
    }

    pub(crate) fn filesystem(&self) -> PinnedRootFs {
        PinnedRootFs {
            namespace: self.namespace.clone(),
            directory: Arc::clone(&self.directory),
        }
    }

    /// Create one absent direct child and retain authority to that exact directory object.
    ///
    /// Both creation and the following open are relative to the already retained parent
    /// descriptor. Replacing the parent's pathname cannot redirect either operation. The child
    /// pathname is kept only for display and later namespace agreement checks; all writes can use
    /// the returned descriptor.
    pub(crate) fn create_child_directory(&self, name: &OsStr) -> io::Result<Self> {
        if name.is_empty() || name.as_bytes().contains(&b'/') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "child directory name is not one ordinary component",
            ));
        }
        self.ensure_namespace_identity()?;
        mkdirat(&self.directory, name)?;
        self.directory.sync_all()?;
        let directory = openat(&self.directory, name, OPEN_DIRECTORY_FLAGS, 0)?;
        if !directory.metadata()?.is_dir() {
            return Err(io::Error::other("created child is not a directory"));
        }
        Ok(Self {
            namespace: self.namespace.join(name),
            directory: Arc::new(directory),
        })
    }

    /// Reopen one direct child through the retained parent, refusing links and traversal.
    pub(crate) fn open_child_directory(&self, name: &OsStr) -> io::Result<Self> {
        if name.is_empty()
            || name == OsStr::new(".")
            || name == OsStr::new("..")
            || name.as_bytes().contains(&b'/')
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid child name",
            ));
        }
        self.ensure_namespace_identity()?;
        let directory = openat(&self.directory, name, OPEN_DIRECTORY_FLAGS, 0)?;
        let child = Self {
            namespace: self.namespace.join(name),
            directory: Arc::new(directory),
        };
        child.ensure_namespace_identity()?;
        Ok(child)
    }

    /// Stable device and inode identity of the retained directory descriptor.
    pub(crate) fn identity(&self) -> io::Result<(u64, u64)> {
        let metadata = self.directory.metadata()?;
        if !metadata.is_dir() {
            return Err(io::Error::other("the pinned root is not a directory"));
        }
        Ok((metadata.dev(), metadata.ino()))
    }

    /// Require the caller's admitted parent object before any descendant creation.
    pub(crate) fn ensure_protected_identity(
        &self,
        expected: ProtectedWorkspaceRoot,
    ) -> io::Result<()> {
        self.ensure_identity(expected.device, expected.inode)
    }

    /// Force the retained directory entry set to durable storage.
    pub(crate) fn sync(&self) -> io::Result<()> {
        self.directory.sync_all()
    }

    /// Duplicate the retained directory descriptor for one descriptor-relative syscall sequence.
    pub(crate) fn try_clone_directory(&self) -> io::Result<File> {
        self.directory.try_clone()
    }

    /// Open the retained directory with an independent lock description. A duplicated descriptor
    /// shares flock ownership, so concurrent users of cloned roots must not lock a `try_clone`.
    pub(crate) fn independent_lock_directory(&self) -> io::Result<File> {
        openat(&self.directory, OsStr::new("."), OPEN_DIRECTORY_FLAGS, 0)
    }

    /// Prove the retained descriptor still names the admitted directory object.
    pub(crate) fn ensure_identity(&self, device: u64, inode: u64) -> io::Result<()> {
        let metadata = self.directory.metadata()?;
        if metadata.is_dir() && metadata.dev() == device && metadata.ino() == inode {
            Ok(())
        } else {
            Err(io::Error::other(
                "the pinned workspace root does not match the admitted directory",
            ))
        }
    }

    /// Prove whether this retained directory is inside an exact protected directory object.
    ///
    /// Every parent is opened relative to the prior retained descriptor. Renaming or replacing
    /// any pathname during this walk therefore cannot redirect the result to another object.
    pub(crate) fn is_within(&self, protected: ProtectedWorkspaceRoot) -> io::Result<bool> {
        let mut directory = self.try_clone_directory()?;
        loop {
            let current = directory.metadata()?;
            if !current.is_dir() {
                return Err(io::Error::other("the retained ancestor is not a directory"));
            }
            if current.dev() == protected.device && current.ino() == protected.inode {
                return Ok(true);
            }
            let parent = openat(&directory, OsStr::new(".."), OPEN_DIRECTORY_FLAGS, 0)?;
            let parent_metadata = parent.metadata()?;
            if parent_metadata.dev() == current.dev() && parent_metadata.ino() == current.ino() {
                return Ok(false);
            }
            directory = parent;
        }
    }

    /// Prove the retained descriptor is still reachable through its displayed namespace.
    ///
    /// This check is never used as mutation authority—the descriptor remains the authority. It
    /// lets user-facing operations fail closed when another process has moved or replaced the
    /// displayed folder, even though descriptor-relative writes could still reach the displaced
    /// object safely.
    pub(crate) fn ensure_namespace_identity(&self) -> io::Result<()> {
        let retained = self.directory.metadata()?;
        let named = std::fs::symlink_metadata(&self.namespace)?;
        if named.file_type().is_dir()
            && !named.file_type().is_symlink()
            && named.dev() == retained.dev()
            && named.ino() == retained.ino()
        {
            Ok(())
        } else {
            Err(io::Error::other(
                "the pinned directory is no longer at its admitted namespace",
            ))
        }
    }

    pub(crate) fn open_record_file(&self, relative: &Path) -> io::Result<File> {
        self.filesystem().open_append_create(relative)
    }

    /// Open an existing record file without acquiring create authority for its name.
    pub(crate) fn open_existing_record_file(&self, relative: &Path) -> io::Result<File> {
        self.filesystem().open_append_existing(relative)
    }
}

/// A [`DurableFs`] whose namespace is resolved from a retained directory descriptor.
#[derive(Clone, Debug)]
pub(crate) struct PinnedRootFs {
    namespace: PathBuf,
    directory: Arc<File>,
}

impl PartialEq for PinnedRootFs {
    fn eq(&self, other: &Self) -> bool {
        self.namespace == other.namespace && Arc::ptr_eq(&self.directory, &other.directory)
    }
}

impl Eq for PinnedRootFs {}

impl PinnedRootFs {
    fn relative(&self, path: &Path) -> io::Result<Vec<OsString>> {
        let path = if path.is_absolute() {
            path.strip_prefix(&self.namespace).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "path is outside the pinned workspace root",
                )
            })?
        } else {
            path
        };
        path.components()
            .map(|component| match component {
                Component::Normal(name) => Ok(name.to_os_string()),
                _ => Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "path is not a confined workspace-relative path",
                )),
            })
            .collect()
    }

    fn open_directory_components(&self, components: &[OsString]) -> io::Result<File> {
        // `dup`/`try_clone` shares a directory stream offset with the retained descriptor. A
        // second enumeration could therefore begin at EOF and falsely report an empty tree.
        // Opening `.` relative to the retained descriptor gives each walk an independent open
        // file description without resolving the user-facing pathname again.
        let mut directory = openat(&self.directory, OsStr::new("."), OPEN_DIRECTORY_FLAGS, 0)?;
        for component in components {
            directory = openat(&directory, component, OPEN_DIRECTORY_FLAGS, 0)?;
            if !directory.metadata()?.is_dir() {
                return Err(io::Error::other(
                    "workspace path component is not a directory",
                ));
            }
        }
        Ok(directory)
    }

    fn parent_and_leaf(&self, path: &Path) -> io::Result<(File, OsString)> {
        let mut components = self.relative(path)?;
        let leaf = components.pop().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "workspace root has no leaf")
        })?;
        Ok((self.open_directory_components(&components)?, leaf))
    }

    fn open_append_create(&self, path: &Path) -> io::Result<File> {
        let (parent, leaf) = self.parent_and_leaf(path)?;
        openat(&parent, &leaf, APPEND_CREATE_FLAGS, 0o600)
    }

    fn open_append_existing(&self, path: &Path) -> io::Result<File> {
        let (parent, leaf) = self.parent_and_leaf(path)?;
        openat(&parent, &leaf, APPEND_EXISTING_FLAGS, 0)
    }

    pub(crate) fn read_file(&self, path: &Path) -> io::Result<File> {
        let (parent, leaf) = self.parent_and_leaf(path)?;
        openat(&parent, &leaf, OPEN_READ_FLAGS, 0)
    }

    /// Open one final entry without following any path component and without blocking on a FIFO.
    ///
    /// Callers must classify the returned descriptor before reading it. This is used by folder
    /// import so an entry that changes after directory enumeration cannot redirect a preview
    /// outside the retained source root or stall the process on a special file.
    pub(crate) fn inspect_entry(&self, path: &Path) -> io::Result<File> {
        let (parent, leaf) = self.parent_and_leaf(path)?;
        openat(&parent, &leaf, OPEN_INSPECT_FLAGS, 0)
    }

    /// Retain the exact parent alongside the nonblocking entry for recovery identity comparison.
    pub(crate) fn inspect_entry_with_parent(&self, path: &Path) -> io::Result<(File, File)> {
        let (parent, leaf) = self.parent_and_leaf(path)?;
        let file = openat(&parent, &leaf, OPEN_INSPECT_FLAGS, 0)?;
        Ok((parent, file))
    }

    /// Enumerate one exact directory below the retained root descriptor.
    ///
    /// The directory stream owns a duplicate of the already verified descriptor, so enumeration
    /// never resolves the caller's pathname again. Entry names are subsequently opened through
    /// [`Self::inspect_entry`], which re-confines every component.
    pub(crate) fn read_directory_names(&self, path: &Path) -> io::Result<Vec<OsString>> {
        self.read_directory_names_bounded(path, usize::MAX)
    }

    /// Bound directory-entry allocation before enumeration, including excluded names.
    pub(crate) fn read_directory_names_bounded(
        &self,
        path: &Path,
        limit: usize,
    ) -> io::Result<Vec<OsString>> {
        let directory = self.open_directory_components(&self.relative(path)?)?;
        let expected = directory.metadata()?;
        let mut names = read_directory_descriptor(&directory, limit)?;
        let after = directory.metadata()?;
        if after.dev() != expected.dev() || after.ino() != expected.ino() {
            return Err(io::Error::other(
                "descriptor directory changed during enumeration",
            ));
        }
        names.sort();
        Ok(names)
    }

    /// Read only a bounded prefix for a diagnostic overview. Unlike complete inventory, this
    /// explicitly returns whether another entry exists; callers must not claim an exact count.
    pub(crate) fn read_directory_prefix(
        &self,
        path: &Path,
        limit: usize,
    ) -> io::Result<(Vec<OsString>, bool)> {
        let directory = self.open_directory_components(&self.relative(path)?)?;
        let (mut names, more) = read_directory_descriptor_prefix(&directory, limit)?;
        names.sort();
        Ok((names, more))
    }

    /// Create and durably populate one file beneath the retained root descriptor.
    ///
    /// The returned pathname is never reopened: bytes, permissions, and durability are applied to
    /// the exact create-new descriptor, while its exact retained parent is synced afterward. A
    /// concurrent replacement therefore cannot redirect or inherit the write.
    pub(crate) fn write_new_file(
        &self,
        path: &Path,
        bytes: &[u8],
        permissions: std::fs::Permissions,
    ) -> io::Result<()> {
        self.write_new_file_with(path, permissions, |file| file.write_all(bytes))
    }

    /// Create and durably populate one file from a bounded writer callback.
    ///
    /// The callback receives the exact create-new descriptor. This lets large imports stream
    /// bytes without allocating the whole file while preserving the same descriptor-pinned
    /// publication, permission, file-sync and parent-sync order as [`Self::write_new_file`].
    pub(crate) fn write_new_file_with(
        &self,
        path: &Path,
        permissions: std::fs::Permissions,
        write: impl FnOnce(&mut File) -> io::Result<()>,
    ) -> io::Result<()> {
        let (parent, leaf) = self.parent_and_leaf(path)?;
        let mut file = openat(&parent, &leaf, CREATE_NEW_FLAGS, 0o600)?;
        write(&mut file)?;
        file.set_permissions(permissions)?;
        file.sync_all()?;
        parent.sync_all()
    }
}

impl DurableFs for PinnedRootFs {
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        let components = self.relative(path)?;
        let mut directory = self.directory.try_clone()?;
        for component in components {
            match mkdirat(&directory, &component) {
                Ok(()) => directory.sync_all()?,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
            directory = openat(&directory, &component, OPEN_DIRECTORY_FLAGS, 0)?;
        }
        Ok(())
    }

    fn stage(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        let (parent, leaf) = self.parent_and_leaf(path)?;
        let mut file = openat(&parent, &leaf, CREATE_NEW_FLAGS, 0o600)?;
        file.write_all(bytes)
    }

    fn append(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.open_append_create(path)?.write_all(bytes)
    }

    fn sync_file(&self, path: &Path) -> io::Result<()> {
        let (parent, leaf) = self.parent_and_leaf(path)?;
        openat(&parent, &leaf, OPEN_WRITE_FLAGS, 0)?.sync_all()
    }

    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        let components = self.relative(path)?;
        self.open_directory_components(&components)?.sync_all()
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        let (from_parent, from_leaf) = self.parent_and_leaf(from)?;
        let (to_parent, to_leaf) = self.parent_and_leaf(to)?;
        renameat(&from_parent, &from_leaf, &to_parent, &to_leaf)
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        self.read_file(path)?.read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    fn exists(&self, path: &Path) -> bool {
        self.read_file(path).is_ok()
    }

    fn file_len(&self, path: &Path) -> io::Result<u64> {
        Ok(self.read_file(path)?.metadata()?.len())
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        let (parent, leaf) = self.parent_and_leaf(path)?;
        unlinkat(&parent, &leaf)
    }

    fn list_dir(&self, _path: &Path) -> io::Result<Vec<PathBuf>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "descriptor-rooted directory enumeration is not exposed",
        ))
    }
}

#[cfg(target_os = "macos")]
#[repr(C)]
struct NativeDirent {
    inode: u64,
    seek_offset: u64,
    record_length: u16,
    name_length: u16,
    entry_type: u8,
    name: [std::os::raw::c_char; 1024],
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct NativeDirent {
    inode: u64,
    seek_offset: i64,
    record_length: u16,
    entry_type: u8,
    name: [std::os::raw::c_char; 256],
}

fn read_directory_descriptor(directory: &File, limit: usize) -> io::Result<Vec<OsString>> {
    let (names, more) = read_directory_descriptor_prefix(directory, limit)?;
    if more {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "directory entry limit exceeded",
        ))
    } else {
        Ok(names)
    }
}

#[allow(unsafe_code)]
fn read_directory_descriptor_prefix(
    directory: &File,
    limit: usize,
) -> io::Result<(Vec<OsString>, bool)> {
    unsafe extern "C" {
        fn fdopendir(descriptor: i32) -> *mut std::ffi::c_void;
        fn readdir(stream: *mut std::ffi::c_void) -> *mut NativeDirent;
        fn closedir(stream: *mut std::ffi::c_void) -> i32;
    }

    #[cfg(target_os = "macos")]
    unsafe fn errno_location() -> *mut i32 {
        unsafe extern "C" {
            fn __error() -> *mut i32;
        }
        // SAFETY: the C runtime returns the calling thread's live errno cell.
        unsafe { __error() }
    }

    #[cfg(target_os = "linux")]
    unsafe fn errno_location() -> *mut i32 {
        unsafe extern "C" {
            fn __errno_location() -> *mut i32;
        }
        // SAFETY: the C runtime returns the calling thread's live errno cell.
        unsafe { __errno_location() }
    }

    // open_directory_components already supplied an independent description for this walk.
    let duplicate = directory.try_clone()?;
    let descriptor = duplicate.into_raw_fd();
    // SAFETY: ownership of the duplicated descriptor transfers to the directory stream on
    // success. On failure it is reconstructed exactly once below.
    let stream = unsafe { fdopendir(descriptor) };
    if stream.is_null() {
        // SAFETY: fdopendir did not take ownership after returning null.
        drop(unsafe { File::from_raw_fd(descriptor) });
        return Err(io::Error::last_os_error());
    }

    let result = (|| {
        let mut names = Vec::new();
        loop {
            // SAFETY: errno_location is a live thread-local cell and `stream` remains owned until
            // the loop result has been collected.
            unsafe { *errno_location() = 0 };
            // SAFETY: `stream` is a live DIR pointer and this function is its sole reader.
            let entry = unsafe { readdir(stream) };
            if entry.is_null() {
                // SAFETY: same live thread-local cell set immediately before readdir.
                let errno = unsafe { *errno_location() };
                return if errno == 0 {
                    Ok((names, false))
                } else {
                    Err(io::Error::from_raw_os_error(errno))
                };
            }
            // SAFETY: readdir returned a live entry whose NUL-terminated name remains valid until
            // the next call on this stream. Copy it before advancing.
            let bytes = unsafe { CStr::from_ptr((*entry).name.as_ptr()) }.to_bytes();
            if bytes == b"." || bytes == b".." {
                continue;
            }
            if names.len() == limit {
                return Ok((names, true));
            }
            names.push(OsString::from_vec(bytes.to_vec()));
        }
    })();
    // SAFETY: fdopendir owns the duplicate and closedir consumes it exactly once.
    let close_result = unsafe { closedir(stream) };
    match (result, close_result) {
        (Err(error), _) => Err(error),
        (Ok(_), value) if value != 0 => Err(io::Error::last_os_error()),
        (Ok(names), _) => Ok(names),
    }
}

#[allow(unsafe_code)]
fn openat(directory: &File, name: &OsStr, flags: i32, mode: i32) -> io::Result<File> {
    unsafe extern "C" {
        fn openat(directory: i32, path: *const std::ffi::c_char, flags: i32, ...) -> i32;
    }
    let name = CString::new(name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name contains a NUL byte"))?;
    // SAFETY: `name` is a live C string, `directory` is an owned descriptor, and a successful
    // call returns one new descriptor that is transferred into `File` exactly once.
    let descriptor = unsafe { openat(directory.as_raw_fd(), name.as_ptr(), flags, mode) };
    if descriptor < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful `openat` returned one newly owned descriptor.
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

#[allow(unsafe_code)]
fn mkdirat(directory: &File, name: &OsStr) -> io::Result<()> {
    unsafe extern "C" {
        fn mkdirat(directory: i32, path: *const std::ffi::c_char, mode: NativeMode) -> i32;
    }
    let name = CString::new(name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name contains a NUL byte"))?;
    // SAFETY: `name` is a live C string and `directory` is an owned directory descriptor.
    if unsafe { mkdirat(directory.as_raw_fd(), name.as_ptr(), 0o700 as NativeMode) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[allow(unsafe_code)]
fn renameat(
    from_directory: &File,
    from: &OsStr,
    to_directory: &File,
    to: &OsStr,
) -> io::Result<()> {
    unsafe extern "C" {
        fn renameat(
            from_directory: i32,
            from: *const std::ffi::c_char,
            to_directory: i32,
            to: *const std::ffi::c_char,
        ) -> i32;
    }
    let from = CString::new(from.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name contains a NUL byte"))?;
    let to = CString::new(to.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name contains a NUL byte"))?;
    // SAFETY: both names are live C strings and both descriptors are owned directories.
    if unsafe {
        renameat(
            from_directory.as_raw_fd(),
            from.as_ptr(),
            to_directory.as_raw_fd(),
            to.as_ptr(),
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[allow(unsafe_code)]
fn unlinkat(directory: &File, name: &OsStr) -> io::Result<()> {
    unsafe extern "C" {
        fn unlinkat(directory: i32, path: *const std::ffi::c_char, flags: i32) -> i32;
    }
    let name = CString::new(name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name contains a NUL byte"))?;
    // SAFETY: `name` is a live C string and `directory` is an owned directory descriptor.
    if unsafe { unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mesh_cas::{Blake3, Cas};
    use mesh_store::{EntityUuid, OperationRecord, RecordDigest, StoredRecord};
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "mesh-pinned-root-{name}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("scratch root");
        path
    }

    fn copy_tree(from: &Path, to: &Path) {
        std::fs::create_dir(to).expect("copy root");
        for entry in std::fs::read_dir(from).expect("read source") {
            let entry = entry.expect("source entry");
            let target = to.join(entry.file_name());
            if entry.file_type().expect("entry type").is_dir() {
                copy_tree(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).expect("copy file");
            }
        }
    }

    #[test]
    fn partial_overview_does_not_relax_complete_inventory_or_share_its_cursor() {
        let selected = scratch("partial-directory-read");
        for name in ["one", "two", "three"] {
            std::fs::write(selected.join(name), b"").unwrap();
        }
        let root = PinnedWorkspaceRoot::open(selected.clone()).unwrap();
        let filesystem = root.filesystem();
        for _ in 0..2 {
            let (names, more) = filesystem.read_directory_prefix(Path::new(""), 2).unwrap();
            assert_eq!(names.len(), 2);
            assert!(more);
            assert!(filesystem
                .read_directory_names_bounded(Path::new(""), 2)
                .is_err());
            let (all, more) = filesystem.read_directory_prefix(Path::new(""), 3).unwrap();
            assert_eq!(all.len(), 3);
            assert!(!more);
            assert_eq!(
                filesystem
                    .read_directory_names_bounded(Path::new(""), 3)
                    .unwrap(),
                all
            );
        }
        std::fs::remove_dir_all(selected).unwrap();
    }

    #[test]
    fn copied_replacement_cannot_inherit_pinned_cas_authority() {
        let selected = scratch("copy-swap");
        let retained = selected.with_extension("retained");
        let root = PinnedWorkspaceRoot::open(selected.clone()).expect("pin root");
        let cas = Cas::<PinnedRootFs, Blake3>::with_filesystem(selected.clone(), root.filesystem())
            .expect("open pinned CAS");

        std::fs::rename(&selected, &retained).expect("move admitted directory");
        std::fs::create_dir_all(&selected).expect("replacement root");
        let promoted = cas
            .promote(b"retained authority".to_vec())
            .expect("promote");

        assert_eq!(
            cas.read(&promoted.digest()).expect("read"),
            b"retained authority"
        );
        assert!(retained.join("chunks").exists());
        assert!(!selected.join("chunks").exists());
        std::fs::remove_dir_all(&selected).expect("remove replacement");
        std::fs::remove_dir_all(&retained).expect("remove retained");
    }

    #[test]
    fn repeated_descriptor_enumeration_starts_from_the_directory_beginning() {
        let selected = scratch("repeat-directory-read");
        std::fs::write(selected.join("one.txt"), b"one").expect("first entry");
        std::fs::write(selected.join("two.txt"), b"two").expect("second entry");
        let root = PinnedWorkspaceRoot::open(selected.clone()).expect("pin root");

        let first = root
            .filesystem()
            .read_directory_names(Path::new(""))
            .expect("first read");
        let second = root
            .filesystem()
            .read_directory_names(Path::new(""))
            .expect("second read");

        assert_eq!(first, second);
        assert_eq!(
            first,
            vec![OsString::from("one.txt"), OsString::from("two.txt")]
        );
        std::fs::remove_dir_all(selected).expect("remove scratch");
    }

    #[test]
    fn copied_replacement_cannot_inherit_journal_or_refresh_authority() {
        let selected = scratch("journal-copy-swap");
        let retained = selected.with_extension("retained");
        let replacement = selected.with_extension("replacement");
        let mut workspace = crate::workspace::OpenWorkspace::open(&selected).expect("open");
        copy_tree(&selected, &replacement);
        std::fs::rename(&selected, &retained).expect("move admitted directory");
        std::fs::rename(&replacement, &selected).expect("install byte-identical copy");

        workspace
            .append_record(&StoredRecord::Operation(OperationRecord {
                id: RecordDigest::from_bytes([1; 32]),
                actor: RecordDigest::from_bytes([2; 32]),
                actor_sequence: 1,
                hlc_millis: 1,
                hlc_counter: 0,
                policy_epoch: 0,
                session: EntityUuid::from_bytes([3; 16]),
                payload_digest: RecordDigest::from_bytes([4; 32]),
                parents: Vec::new(),
            }))
            .expect("append through retained journal");
        workspace
            .refresh_with_trusted_reviewers(&crate::TrustedReviewers::default())
            .expect("refresh through retained handles");

        assert!(
            std::fs::metadata(
                retained
                    .join(crate::workspace::STORAGE_DIRECTORY_NAME)
                    .join(crate::workspace::RECORD_FILE_NAME),
            )
            .expect("retained journal")
            .len()
                > 0
        );
        assert_eq!(
            std::fs::metadata(
                selected
                    .join(crate::workspace::STORAGE_DIRECTORY_NAME)
                    .join(crate::workspace::RECORD_FILE_NAME),
            )
            .expect("replacement journal")
            .len(),
            0
        );
        assert_eq!(workspace.operations(), 1);
        std::fs::remove_dir_all(&selected).expect("remove replacement");
        std::fs::remove_dir_all(&retained).expect("remove retained");
    }

    #[test]
    fn remembered_reopen_cannot_recreate_a_journal_removed_after_root_pinning() {
        let selected = scratch("existing-journal-disappears");
        let journal = selected.join(crate::workspace::RECORD_FILE_NAME);
        std::fs::write(&journal, b"").expect("journal");
        let root = PinnedWorkspaceRoot::open(selected.clone()).expect("pin root");

        std::fs::remove_file(&journal).expect("remove journal after pinning");
        let error = root
            .open_existing_record_file(Path::new(crate::workspace::RECORD_FILE_NAME))
            .expect_err("existing-only open must refuse a missing journal");

        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(!journal.exists(), "refusal must not recreate the journal");
        let outside = selected.with_extension("outside-journal");
        std::fs::write(&outside, b"foreign").expect("outside journal");
        std::os::unix::fs::symlink(&outside, &journal).expect("replace journal with link");
        root.open_existing_record_file(Path::new(crate::workspace::RECORD_FILE_NAME))
            .expect_err("existing-only open must not follow a replacement link");
        assert_eq!(
            std::fs::read(&outside).expect("outside bytes"),
            b"foreign",
            "refusal must not append through the replacement link"
        );
        std::fs::remove_dir_all(&selected).expect("remove scratch");
        std::fs::remove_file(&outside).expect("remove outside journal");
    }
}
