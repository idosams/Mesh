//! Independent Git metadata for a Mesh-native working folder.

use crate::{isolated_git_command, GitImportError, GitObjectFormat, GitProvenanceAnchor};
use std::ffi::{CStr, CString, OsStr, OsString};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read as _, Seek as _, Write as _};
use std::os::fd::{AsRawFd as _, FromRawFd as _, IntoRawFd as _};
use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

#[cfg(target_os = "macos")]
const OPEN_DIRECTORY_FLAGS: i32 = 0x0110_0100;
#[cfg(target_os = "linux")]
const OPEN_DIRECTORY_FLAGS: i32 = 0x000b_0000;

#[cfg(target_os = "macos")]
const CREATE_NEW_FLAGS: i32 = 0x0100_0b02;
#[cfg(target_os = "linux")]
const CREATE_NEW_FLAGS: i32 = 0x0008_00c2;

#[cfg(target_os = "macos")]
type NativeMode = u16;
#[cfg(target_os = "linux")]
type NativeMode = u32;

#[cfg(target_os = "macos")]
const AT_REMOVEDIR: i32 = 0x80;
#[cfg(target_os = "linux")]
const AT_REMOVEDIR: i32 = 0x200;

#[cfg(test)]
thread_local! {
    static AFTER_GIT_INIT: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        std::cell::RefCell::new(None);
    static BEFORE_GIT_PUBLICATION: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        std::cell::RefCell::new(None);
    static AFTER_GIT_PUBLICATION: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        std::cell::RefCell::new(None);
}

/// Installed Git context for one independently writable Mesh-native folder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndependentGitContext {
    git_directory: PathBuf,
    anchor: GitProvenanceAnchor,
}

/// Stable kernel identity of one admitted Git destination directory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GitDestinationIdentity {
    device: u64,
    inode: u64,
}

impl GitDestinationIdentity {
    /// Inspect one existing real directory without following a final symlink.
    pub fn inspect(path: &Path) -> io::Result<Self> {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
            return Err(io::Error::other("Git destination is not a real directory"));
        }
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
}

impl IndependentGitContext {
    /// Private Git directory installed inside the native working folder.
    #[must_use]
    pub fn git_directory(&self) -> &Path {
        &self.git_directory
    }

    /// Exact state observed after installation.
    #[must_use]
    pub const fn anchor(&self) -> &GitProvenanceAnchor {
        &self.anchor
    }
}

/// Install a standalone `.git` directory without sharing mutable metadata with the source.
///
/// The destination must already contain the exact imported working files and must not contain a
/// `.git` entry. The source is checked against `expected` before and after its history is bundled;
/// Git's non-header, non-ignored porcelain records must then match in the destination. Ignored
/// working files are deliberately absent from a Mesh import and remain recorded only in the
/// provenance anchor. The resulting checkout contains copied objects and its own index, refs and
/// locks. No remote, hook, credential helper, alternates file, or source path is retained.
///
/// # Errors
///
/// Returns a typed refusal without replacing any existing `.git` entry. Temporary files created
/// by a failed attempt are removed only when their exact private names are still ordinary entries.
pub fn install_independent_git_context(
    source: &Path,
    destination: &Path,
    expected: &GitProvenanceAnchor,
) -> Result<IndependentGitContext, GitContextError> {
    install(source, destination, None, expected, true)
}

/// Install independent Git context only into the exact previously inspected directory object.
///
/// A pathname may be renamed and replaced between an outer workspace authority check and this
/// adapter. The expected identity is compared with the descriptor opened by this function before
/// any staging or publication begins, so a replacement cannot inherit Git metadata.
pub fn install_independent_git_context_for_destination(
    source: &Path,
    destination: &Path,
    destination_identity: GitDestinationIdentity,
    expected: &GitProvenanceAnchor,
) -> Result<IndependentGitContext, GitContextError> {
    install(
        source,
        destination,
        Some(destination_identity),
        expected,
        true,
    )
}

/// Install only the independently copied repository history and index into another Mesh version.
///
/// Unlike [`install_independent_git_context`], the destination working files may intentionally
/// differ from the source because they represent another saved Mesh point. Git will show those
/// differences normally against the imported `HEAD`; repository identity, branch and object
/// storage remain exact and independent.
///
/// # Errors
///
/// Returns the same fail-closed errors as [`install_independent_git_context`].
pub fn install_independent_git_history(
    source: &Path,
    destination: &Path,
    expected: &GitProvenanceAnchor,
) -> Result<IndependentGitContext, GitContextError> {
    install(source, destination, None, expected, false)
}

/// Install independent Git history only into the exact previously inspected directory object.
pub fn install_independent_git_history_for_destination(
    source: &Path,
    destination: &Path,
    destination_identity: GitDestinationIdentity,
    expected: &GitProvenanceAnchor,
) -> Result<IndependentGitContext, GitContextError> {
    install(
        source,
        destination,
        Some(destination_identity),
        expected,
        false,
    )
}

fn install(
    source: &Path,
    destination: &Path,
    destination_identity: Option<GitDestinationIdentity>,
    expected: &GitProvenanceAnchor,
    require_exact_worktree: bool,
) -> Result<IndependentGitContext, GitContextError> {
    let destination = canonical_real_directory(destination)?;
    let publication = PinnedGitPublication::open(&destination, destination_identity)?;
    if destination == expected.source_root() || destination == fs::canonicalize(source)? {
        return Err(GitContextError::SameWorkspace);
    }
    if !expected.gitlinks().is_empty() {
        return Err(GitContextError::SubmodulesUnsupported);
    }
    let before = GitProvenanceAnchor::inspect(source)?;
    if &before != expected {
        return Err(GitContextError::SourceChanged);
    }
    let final_git = destination.join(".git");
    match fs::symlink_metadata(&final_git) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Ok(_) => return Err(GitContextError::GitEntryExists),
        Err(error) => return Err(GitContextError::Io(error)),
    }

    let suffix = random_staging_suffix()?;
    let temporary_git = OsString::from(format!(".mesh-git-{suffix}.tmp"));
    let bundle = OsString::from(format!(".mesh-git-{suffix}.bundle"));
    let git_directory = publication.create_staging_directory(&temporary_git)?;
    let mut bundle_file = publication.create_staging_file(&bundle)?;
    let mut cleanup = Cleanup::new(
        publication.destination.try_clone()?,
        temporary_git.clone(),
        git_directory.try_clone()?,
    );
    // The create-only file descriptor is the bundle capability. Drop its discoverable name before
    // writing any bytes so a namespace replacement cannot redirect input or become a cleanup
    // target. The retained descriptor remains readable by the target Git process.
    publication.remove_staging_file(&bundle)?;
    source_git_to_file(
        source,
        [
            OsStr::new("bundle"),
            OsStr::new("create"),
            OsStr::new("-"),
            OsStr::new("HEAD"),
        ],
        &bundle_file,
    )?;
    bundle_file.sync_all()?;
    bundle_file.rewind()?;
    let after_bundle = GitProvenanceAnchor::inspect(source)?;
    if &after_bundle != expected {
        return Err(GitContextError::SourceChanged);
    }
    target_git(
        &git_directory,
        [
            OsStr::new("init"),
            OsStr::new("--bare"),
            OsStr::new(object_format_argument(
                expected.repository().object_format(),
            )),
        ],
    )?;
    #[cfg(test)]
    AFTER_GIT_INIT.with(|hook| {
        if let Some(hook) = hook.borrow_mut().take() {
            hook();
        }
    });
    target_git_with_file_input(
        &git_directory,
        [
            OsStr::new("bundle"),
            OsStr::new("unbundle"),
            OsStr::new("-"),
        ],
        &bundle_file,
    )?;
    target_git(
        &git_directory,
        [
            OsStr::new("update-ref"),
            OsStr::new("refs/mesh/import"),
            OsStr::new(expected.head().as_str()),
        ],
    )?;
    install_head(&git_directory, expected)?;
    target_git(
        &git_directory,
        [
            OsStr::new("read-tree"),
            OsStr::new(expected.head().as_str()),
        ],
    )?;
    let staged_patch = source_git(
        source,
        [
            OsStr::new("diff"),
            OsStr::new("--no-ext-diff"),
            OsStr::new("--cached"),
            OsStr::new("--binary"),
            OsStr::new("--full-index"),
            OsStr::new("--no-renames"),
            OsStr::new("HEAD"),
            OsStr::new("--"),
        ],
    )?
    .stdout;
    let after_index_read = GitProvenanceAnchor::inspect(source)?;
    if &after_index_read != expected {
        return Err(GitContextError::SourceChanged);
    }
    if !staged_patch.is_empty() {
        target_git_with_input(
            &git_directory,
            [
                OsStr::new("apply"),
                OsStr::new("--cached"),
                OsStr::new("--binary"),
                OsStr::new("--whitespace=nowarn"),
                OsStr::new("-"),
            ],
            staged_patch,
        )?;
    }
    target_git(
        &git_directory,
        [
            OsStr::new("update-ref"),
            OsStr::new("-d"),
            OsStr::new("refs/mesh/import"),
        ],
    )?;

    #[cfg(test)]
    BEFORE_GIT_PUBLICATION.with(|hook| {
        if let Some(hook) = hook.borrow_mut().take() {
            hook();
        }
    });
    publication.ensure_current()?;
    ensure_named_directory_identity(&publication.destination, &temporary_git, &git_directory)?;
    publication.publish(&temporary_git, OsStr::new(".git"))?;
    cleanup.git_name = OsString::from(".git");
    publication.sync()?;
    publication.ensure_current()?;
    ensure_named_directory_identity(&publication.destination, OsStr::new(".git"), &git_directory)?;
    #[cfg(test)]
    AFTER_GIT_PUBLICATION.with(|hook| {
        if let Some(hook) = hook.borrow_mut().take() {
            hook();
        }
    });

    let temporary_status = target_worktree_status(&git_directory)?;
    if require_exact_worktree
        && worktree_records(&temporary_status) != worktree_records(expected.status_porcelain_v2_z())
    {
        return Err(GitContextError::DestinationMismatch);
    }
    target_git(
        &git_directory,
        [
            OsStr::new("config"),
            OsStr::new("core.bare"),
            OsStr::new("false"),
        ],
    )?;

    publication.ensure_current()?;
    publication.sync()?;
    publication.ensure_current()?;
    ensure_named_directory_identity(&publication.destination, OsStr::new(".git"), &git_directory)?;
    let installed_head = target_git(
        &git_directory,
        [
            OsStr::new("rev-parse"),
            OsStr::new("--verify"),
            OsStr::new("HEAD^{commit}"),
        ],
    )?;
    if installed_head.stdout != format!("{}\n", expected.head().as_str()).as_bytes() {
        return Err(GitContextError::DestinationMismatch);
    }
    let installed_head_ref = target_symbolic_head(&git_directory)?;
    if installed_head_ref.as_deref() != expected.head_ref() {
        return Err(GitContextError::DestinationMismatch);
    }
    let installed = GitProvenanceAnchor::from_verified_install(
        destination.clone(),
        expected,
        installed_head_ref,
        temporary_status,
    )?;
    publication.ensure_current()?;
    if installed.repository() != expected.repository()
        || installed.head() != expected.head()
        || installed.head_ref() != expected.head_ref()
    {
        return Err(GitContextError::DestinationMismatch);
    }
    cleanup.git_present = false;
    Ok(IndependentGitContext {
        git_directory: final_git,
        anchor: installed,
    })
}

/// Exact destination authority retained across the potentially long Git staging operation.
struct PinnedGitPublication {
    parent_path: PathBuf,
    parent: File,
    parent_identity: (u64, u64),
    destination_path: PathBuf,
    destination: File,
    destination_identity: (u64, u64),
}

impl PinnedGitPublication {
    fn open(
        destination_path: &Path,
        expected_identity: Option<GitDestinationIdentity>,
    ) -> Result<Self, GitContextError> {
        let parent_path = destination_path
            .parent()
            .ok_or(GitContextError::DestinationNotDirectory)?
            .to_path_buf();
        let destination_name = destination_path
            .file_name()
            .ok_or(GitContextError::DestinationNotDirectory)?;
        let parent = open_directory(&parent_path)?;
        let parent_metadata = parent.metadata()?;
        let destination = openat_directory(&parent, destination_name)?;
        let destination_metadata = destination.metadata()?;
        if expected_identity.is_some_and(|expected| {
            expected
                != (GitDestinationIdentity {
                    device: destination_metadata.dev(),
                    inode: destination_metadata.ino(),
                })
        }) {
            return Err(GitContextError::DestinationMismatch);
        }
        let pinned = Self {
            parent_path,
            parent,
            parent_identity: (parent_metadata.dev(), parent_metadata.ino()),
            destination_path: destination_path.to_path_buf(),
            destination,
            destination_identity: (destination_metadata.dev(), destination_metadata.ino()),
        };
        pinned.ensure_current()?;
        Ok(pinned)
    }

    fn create_staging_directory(&self, name: &OsStr) -> Result<File, GitContextError> {
        mkdirat(&self.destination, name).map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                GitContextError::TemporaryEntryExists
            } else {
                GitContextError::Io(error)
            }
        })?;
        self.destination.sync_all()?;
        Ok(openat_directory(&self.destination, name)?)
    }

    fn create_staging_file(&self, name: &OsStr) -> Result<File, GitContextError> {
        let file = openat_create_new(&self.parent, name).map_err(map_temporary_entry_error)?;
        self.parent.sync_all()?;
        Ok(file)
    }

    fn remove_staging_file(&self, name: &OsStr) -> Result<(), GitContextError> {
        unlinkat(&self.parent, name, 0)?;
        self.parent.sync_all()?;
        Ok(())
    }

    fn publish(&self, staged_name: &OsStr, name: &OsStr) -> Result<(), GitContextError> {
        renameat_create_new(&self.destination, staged_name, &self.destination, name).map_err(
            |error| {
                if error.kind() == io::ErrorKind::AlreadyExists {
                    GitContextError::GitEntryExists
                } else {
                    GitContextError::Io(error)
                }
            },
        )
    }

    fn sync(&self) -> Result<(), GitContextError> {
        self.destination.sync_all()?;
        self.parent.sync_all()?;
        Ok(())
    }

    fn ensure_current(&self) -> Result<(), GitContextError> {
        ensure_directory_identity(&self.parent, &self.parent_path, self.parent_identity)?;
        ensure_directory_identity(
            &self.destination,
            &self.destination_path,
            self.destination_identity,
        )
    }
}

fn ensure_named_directory_identity(
    parent: &File,
    name: &OsStr,
    retained: &File,
) -> Result<(), GitContextError> {
    let named = openat_directory(parent, name).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            GitContextError::DestinationMismatch
        } else {
            GitContextError::Io(error)
        }
    })?;
    let expected = retained.metadata()?;
    let current = named.metadata()?;
    if !expected.is_dir()
        || !current.is_dir()
        || (expected.dev(), expected.ino()) != (current.dev(), current.ino())
    {
        return Err(GitContextError::DestinationMismatch);
    }
    Ok(())
}

fn ensure_directory_identity(
    descriptor: &File,
    path: &Path,
    expected: (u64, u64),
) -> Result<(), GitContextError> {
    let descriptor_metadata = descriptor.metadata()?;
    let path_metadata = fs::symlink_metadata(path)?;
    if !descriptor_metadata.is_dir()
        || !path_metadata.file_type().is_dir()
        || path_metadata.file_type().is_symlink()
        || (descriptor_metadata.dev(), descriptor_metadata.ino()) != expected
        || (path_metadata.dev(), path_metadata.ino()) != expected
    {
        return Err(GitContextError::DestinationMismatch);
    }
    Ok(())
}

pub(crate) fn open_directory(path: &Path) -> std::io::Result<File> {
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(OPEN_DIRECTORY_FLAGS)
        .open(path)?;
    if !directory.metadata()?.is_dir() {
        return Err(std::io::Error::other("path is not a directory"));
    }
    Ok(directory)
}

#[allow(unsafe_code, clashing_extern_declarations)]
fn openat_directory(directory: &File, name: &OsStr) -> std::io::Result<File> {
    unsafe extern "C" {
        #[link_name = "openat"]
        fn openat_without_mode(directory: i32, path: *const std::ffi::c_char, flags: i32) -> i32;
    }
    let name = c_name(name)?;
    // SAFETY: the name is a live C string and a successful call returns one fresh descriptor.
    let descriptor =
        unsafe { openat_without_mode(directory.as_raw_fd(), name.as_ptr(), OPEN_DIRECTORY_FLAGS) };
    if descriptor < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: successful `openat` returned one newly owned descriptor.
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

#[allow(unsafe_code, clashing_extern_declarations)]
fn openat_create_new(directory: &File, name: &OsStr) -> std::io::Result<File> {
    unsafe extern "C" {
        fn openat(directory: i32, path: *const std::ffi::c_char, flags: i32, ...) -> i32;
    }
    let name = c_name(name)?;
    // SAFETY: the name is a live C string and a successful call returns one fresh descriptor.
    let descriptor = unsafe {
        openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            CREATE_NEW_FLAGS,
            0o600_i32,
        )
    };
    if descriptor < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: successful `openat` returned one newly owned descriptor.
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

#[allow(unsafe_code)]
fn mkdirat(directory: &File, name: &OsStr) -> std::io::Result<()> {
    unsafe extern "C" {
        fn mkdirat(directory: i32, path: *const std::ffi::c_char, mode: NativeMode) -> i32;
    }
    let name = c_name(name)?;
    // SAFETY: the name is a live C string and the descriptor is a retained directory.
    if unsafe { mkdirat(directory.as_raw_fd(), name.as_ptr(), 0o700 as NativeMode) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn renameat_create_new(
    from_directory: &File,
    from: &OsStr,
    to_directory: &File,
    to: &OsStr,
) -> std::io::Result<()> {
    unsafe extern "C" {
        fn renameatx_np(
            from_directory: i32,
            from: *const std::ffi::c_char,
            to_directory: i32,
            to: *const std::ffi::c_char,
            flags: u32,
        ) -> i32;
    }
    const RENAME_EXCL: u32 = 0x4;
    let from = c_name(from)?;
    let to = c_name(to)?;
    // SAFETY: both names are live C strings, both descriptors are retained directories, and
    // `RENAME_EXCL` makes the publication create-only.
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
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn renameat_create_new(
    from_directory: &File,
    from: &OsStr,
    to_directory: &File,
    to: &OsStr,
) -> std::io::Result<()> {
    unsafe extern "C" {
        fn renameat2(
            from_directory: i32,
            from: *const std::ffi::c_char,
            to_directory: i32,
            to: *const std::ffi::c_char,
            flags: u32,
        ) -> i32;
    }
    const RENAME_NOREPLACE: u32 = 1;
    let from = c_name(from)?;
    let to = c_name(to)?;
    // SAFETY: both names are live C strings, both descriptors are retained directories, and
    // `RENAME_NOREPLACE` makes the publication create-only.
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
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[allow(unsafe_code)]
fn unlinkat(directory: &File, name: &OsStr, flags: i32) -> std::io::Result<()> {
    unsafe extern "C" {
        fn unlinkat(directory: i32, path: *const std::ffi::c_char, flags: i32) -> i32;
    }
    let name = c_name(name)?;
    // SAFETY: the name is a live C string and removal remains rooted at the retained directory.
    if unsafe { unlinkat(directory.as_raw_fd(), name.as_ptr(), flags) } != 0 {
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

fn random_staging_suffix() -> Result<String, GitContextError> {
    let mut random = [0_u8; 16];
    File::open("/dev/urandom")?.read_exact(&mut random)?;
    let mut suffix = String::with_capacity(random.len() * 2);
    for byte in random {
        use std::fmt::Write as _;
        write!(&mut suffix, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok(suffix)
}

fn target_worktree_status(git: &File) -> Result<Vec<u8>, GitContextError> {
    let mut command = safe_git();
    command
        .arg("--git-dir")
        .arg(".")
        .arg("--work-tree")
        .arg("..")
        .args([
            "status",
            "--porcelain=v2",
            "--branch",
            "--show-stash",
            "--untracked-files=all",
            "--ignored=matching",
            "-z",
        ]);
    Ok(run_in_directory(command, git)?.stdout)
}

fn install_head(git: &File, expected: &GitProvenanceAnchor) -> Result<(), GitContextError> {
    match expected.head_ref() {
        Some(reference) => {
            let reference = std::str::from_utf8(reference)
                .map_err(|_| GitContextError::UnsupportedHeadReference)?;
            if !reference.starts_with("refs/heads/") {
                return Err(GitContextError::UnsupportedHeadReference);
            }
            target_git(
                git,
                [
                    OsStr::new("update-ref"),
                    OsStr::new(reference),
                    OsStr::new(expected.head().as_str()),
                ],
            )?;
            target_git(
                git,
                [
                    OsStr::new("symbolic-ref"),
                    OsStr::new("HEAD"),
                    OsStr::new(reference),
                ],
            )?;
        }
        None => {
            target_git(
                git,
                [
                    OsStr::new("update-ref"),
                    OsStr::new("HEAD"),
                    OsStr::new(expected.head().as_str()),
                ],
            )?;
        }
    }
    Ok(())
}

fn canonical_real_directory(path: &Path) -> Result<PathBuf, GitContextError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(GitContextError::DestinationNotDirectory);
    }
    let canonical = fs::canonicalize(path)?;
    Ok(canonical)
}

fn map_temporary_entry_error(error: std::io::Error) -> GitContextError {
    if error.kind() == std::io::ErrorKind::AlreadyExists {
        GitContextError::TemporaryEntryExists
    } else {
        GitContextError::Io(error)
    }
}

fn object_format_argument(format: GitObjectFormat) -> &'static str {
    match format {
        GitObjectFormat::Sha1 => "--object-format=sha1",
        GitObjectFormat::Sha256 => "--object-format=sha256",
    }
}

fn source_git<'a>(
    root: &Path,
    arguments: impl IntoIterator<Item = &'a OsStr>,
) -> Result<Output, GitContextError> {
    let mut command = safe_git();
    command.arg("-C").arg(root).args(arguments);
    run(command)
}

fn source_git_to_file<'a>(
    root: &Path,
    arguments: impl IntoIterator<Item = &'a OsStr>,
    output: &File,
) -> Result<Output, GitContextError> {
    let mut command = safe_git();
    command
        .arg("-C")
        .arg(root)
        .args(arguments)
        .stdout(Stdio::from(output.try_clone()?));
    run(command)
}

fn target_git<'a>(
    git: &File,
    arguments: impl IntoIterator<Item = &'a OsStr>,
) -> Result<Output, GitContextError> {
    let mut command = safe_git();
    command.arg("--git-dir").arg(".").args(arguments);
    run_in_directory(command, git)
}

fn target_git_with_file_input<'a>(
    git: &File,
    arguments: impl IntoIterator<Item = &'a OsStr>,
    input: &File,
) -> Result<Output, GitContextError> {
    let mut command = safe_git();
    command
        .arg("--git-dir")
        .arg(".")
        .args(arguments)
        .stdin(Stdio::from(input.try_clone()?));
    run_in_directory(command, git)
}

fn target_symbolic_head(git: &File) -> Result<Option<Vec<u8>>, GitContextError> {
    let mut command = safe_git();
    command
        .arg("--git-dir")
        .arg(".")
        .args(["symbolic-ref", "-q", "HEAD"]);
    pin_command_directory(&mut command, git);
    let output = command.output()?;
    if output.status.success() {
        let Some(reference) = output.stdout.strip_suffix(b"\n") else {
            return Err(GitContextError::DestinationMismatch);
        };
        if reference.is_empty() || reference.contains(&b'\n') || !output.stderr.is_empty() {
            return Err(GitContextError::DestinationMismatch);
        }
        return Ok(Some(reference.to_vec()));
    }
    if output.status.code() == Some(1) && output.stdout.is_empty() && output.stderr.is_empty() {
        return Ok(None);
    }
    Err(GitContextError::Git {
        status: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr[..output.stderr.len().min(4_096)])
            .trim()
            .to_owned(),
    })
}

fn target_git_with_input<'a>(
    git: &File,
    arguments: impl IntoIterator<Item = &'a OsStr>,
    input: Vec<u8>,
) -> Result<Output, GitContextError> {
    let mut command = safe_git();
    command
        .arg("--git-dir")
        .arg(".")
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    pin_command_directory(&mut command, git);
    let mut child = command.spawn()?;
    let mut stdin = child.stdin.take().ok_or_else(|| {
        GitContextError::Io(std::io::Error::other(
            "Git context command supplied no stdin pipe",
        ))
    })?;
    // `wait_with_output` drains stdout and stderr while this writer feeds a potentially large
    // binary patch, so neither side can fill a pipe and deadlock the import.
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let output = child.wait_with_output()?;
    let write_result = writer.join().map_err(|_| {
        GitContextError::Io(std::io::Error::other(
            "Git context stdin writer terminated unexpectedly",
        ))
    })?;
    if !output.status.success() {
        return Err(GitContextError::Git {
            status: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr[..output.stderr.len().min(4_096)])
                .trim()
                .to_owned(),
        });
    }
    write_result?;
    Ok(output)
}

fn safe_git() -> Command {
    let mut command = isolated_git_command();
    command
        .arg("-c")
        .arg("core.fsmonitor=false")
        .arg("-c")
        .arg("core.untrackedCache=false")
        .arg("-c")
        .arg("core.hooksPath=/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_PAGER", "cat")
        .env("LC_ALL", "C")
        .env("LANG", "C");
    command
}

fn run(mut command: Command) -> Result<Output, GitContextError> {
    let output = command.output()?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(GitContextError::Git {
            status: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr[..output.stderr.len().min(4_096)])
                .trim()
                .to_owned(),
        })
    }
}

fn run_in_directory(mut command: Command, directory: &File) -> Result<Output, GitContextError> {
    pin_command_directory(&mut command, directory);
    run(command)
}

#[allow(unsafe_code)]
fn pin_command_directory(command: &mut Command, directory: &File) {
    let descriptor = directory.as_raw_fd();
    // SAFETY: `pre_exec` runs after fork and before exec while the retained directory remains
    // live; `fchdir` is async-signal-safe and binds Git to that exact object, not its mutable name.
    unsafe {
        command.pre_exec(move || {
            unsafe extern "C" {
                fn fchdir(descriptor: i32) -> i32;
            }
            if fchdir(descriptor) == 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        });
    }
}

fn worktree_records(status: &[u8]) -> Vec<&[u8]> {
    status
        .split(|byte| *byte == 0)
        .filter(|record| {
            !record.is_empty() && !record.starts_with(b"# ") && !record.starts_with(b"! ")
        })
        .collect()
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

#[allow(unsafe_code)]
fn read_directory_descriptor(directory: &File) -> io::Result<Vec<OsString>> {
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
        // SAFETY: the C runtime returns this thread's live errno cell.
        unsafe { __error() }
    }

    #[cfg(target_os = "linux")]
    unsafe fn errno_location() -> *mut i32 {
        unsafe extern "C" {
            fn __errno_location() -> *mut i32;
        }
        // SAFETY: the C runtime returns this thread's live errno cell.
        unsafe { __errno_location() }
    }

    let duplicate = directory.try_clone()?;
    let descriptor = duplicate.into_raw_fd();
    // SAFETY: ownership of the duplicate transfers to the stream on success.
    let stream = unsafe { fdopendir(descriptor) };
    if stream.is_null() {
        // SAFETY: `fdopendir` did not take ownership after returning null.
        drop(unsafe { File::from_raw_fd(descriptor) });
        return Err(io::Error::last_os_error());
    }
    let result = (|| {
        let mut names = Vec::new();
        loop {
            // SAFETY: this is the live thread-local errno cell.
            unsafe { *errno_location() = 0 };
            // SAFETY: the stream remains live and has one reader.
            let entry = unsafe { readdir(stream) };
            if entry.is_null() {
                // SAFETY: this is the cell reset immediately before `readdir`.
                let errno = unsafe { *errno_location() };
                return if errno == 0 {
                    Ok(names)
                } else {
                    Err(io::Error::from_raw_os_error(errno))
                };
            }
            // SAFETY: a successful `readdir` supplies one NUL-terminated live name.
            let bytes = unsafe { CStr::from_ptr((*entry).name.as_ptr()) }.to_bytes();
            if bytes != b"." && bytes != b".." {
                names.push(OsString::from_vec(bytes.to_vec()));
            }
        }
    })();
    // SAFETY: `fdopendir` owns the duplicate and `closedir` consumes it exactly once.
    let close_result = unsafe { closedir(stream) };
    match (result, close_result) {
        (Err(error), _) => Err(error),
        (Ok(_), value) if value != 0 => Err(io::Error::last_os_error()),
        (Ok(names), _) => Ok(names),
    }
}

pub(crate) fn remove_directory_contents(directory: &File) -> io::Result<()> {
    for name in read_directory_descriptor(directory)? {
        match openat_directory(directory, &name) {
            Ok(child) => {
                let identity = child.metadata()?;
                remove_directory_contents(&child)?;
                let named = openat_directory(directory, &name)?;
                let named_identity = named.metadata()?;
                if (identity.dev(), identity.ino()) != (named_identity.dev(), named_identity.ino())
                {
                    return Err(io::Error::other(
                        "temporary Git directory entry changed during cleanup",
                    ));
                }
                unlinkat(directory, &name, AT_REMOVEDIR)?;
            }
            Err(error)
                if matches!(
                    error.raw_os_error(),
                    Some(20) // ENOTDIR
                        | Some(40) // ELOOP on Linux
                        | Some(62) // ELOOP on macOS
                ) =>
            {
                unlinkat(directory, &name, 0)?;
            }
            Err(error) => return Err(error),
        }
    }
    directory.sync_all()
}

struct Cleanup {
    git_parent: File,
    git_name: OsString,
    git: File,
    git_present: bool,
}

impl Cleanup {
    fn new(git_parent: File, git_name: OsString, git: File) -> Self {
        Self {
            git_parent,
            git_name,
            git,
            git_present: true,
        }
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        if self.git_present {
            let retained = self.git.metadata();
            let named = openat_directory(&self.git_parent, &self.git_name)
                .and_then(|entry| entry.metadata());
            if let (Ok(retained), Ok(named)) = (retained, named) {
                if (retained.dev(), retained.ino()) == (named.dev(), named.ino())
                    && remove_directory_contents(&self.git).is_ok()
                {
                    let _ = unlinkat(&self.git_parent, &self.git_name, AT_REMOVEDIR);
                }
            }
        }
        let _ = self.git_parent.sync_all();
    }
}

/// Why an independent native Git context could not be installed.
#[derive(Debug)]
pub enum GitContextError {
    /// Exact source inspection failed.
    Import(GitImportError),
    /// Filesystem operation failed.
    Io(std::io::Error),
    /// Source and destination resolve to the same working directory.
    SameWorkspace,
    /// Destination is not a real directory.
    DestinationNotDirectory,
    /// Destination already has Git metadata, which is never replaced.
    GitEntryExists,
    /// A private temporary name unexpectedly existed.
    TemporaryEntryExists,
    /// Source changed after the exact preview.
    SourceChanged,
    /// Nested repositories require independent recursive custody not supplied by this version.
    SubmodulesUnsupported,
    /// Symbolic HEAD was not a supported UTF-8 local branch ref.
    UnsupportedHeadReference,
    /// Git command failed.
    Git {
        /// Exit code if the process returned one.
        status: Option<i32>,
        /// Bounded diagnostic.
        stderr: String,
    },
    /// Installed Git view did not reproduce the expected repository, head, branch and worktree.
    DestinationMismatch,
}

impl From<GitImportError> for GitContextError {
    fn from(error: GitImportError) -> Self {
        Self::Import(error)
    }
}

impl From<std::io::Error> for GitContextError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl fmt::Display for GitContextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Import(error) => write!(formatter, "{error}"),
            Self::Io(error) => write!(
                formatter,
                "Git context filesystem operation failed: {error}"
            ),
            Self::SameWorkspace => {
                formatter.write_str("Git context destination is the source workspace")
            }
            Self::DestinationNotDirectory => {
                formatter.write_str("Git context destination is not a real directory")
            }
            Self::GitEntryExists => {
                formatter.write_str("Git context destination already contains .git")
            }
            Self::TemporaryEntryExists => {
                formatter.write_str("Git context private temporary entry already exists")
            }
            Self::SourceChanged => {
                formatter.write_str("Git source changed after its exact preview")
            }
            Self::SubmodulesUnsupported => {
                formatter.write_str("Git submodules require a separately managed nested checkout")
            }
            Self::UnsupportedHeadReference => {
                formatter.write_str("Git symbolic HEAD is not a supported local branch reference")
            }
            Self::Git { status, stderr } => write!(
                formatter,
                "Git context command failed with status {status:?}: {stderr}"
            ),
            Self::DestinationMismatch => formatter
                .write_str("installed Git context does not match the imported working state"),
        }
    }
}

impl std::error::Error for GitContextError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Import(error) => Some(error),
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod replacement_tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(label: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "mesh-git-publication-{label}-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir(&root).expect("scratch");
            Self(root)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn git(root: &Path, arguments: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(arguments)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("LC_ALL", "C")
            .output()
            .expect("run Git fixture command");
        assert!(
            output.status.success(),
            "git {arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn admitted_destination_identity_refuses_an_early_replacement() {
        let fixture = Scratch::new("early-destination-replacement");
        let source = fixture.0.join("source");
        let destination = fixture.0.join("destination");
        let displaced = fixture.0.join("displaced");
        fs::create_dir(&source).expect("source");
        fs::create_dir(&destination).expect("destination");
        git(&source, &["init", "-b", "main"]);
        git(&source, &["config", "user.name", "Mesh test"]);
        git(&source, &["config", "user.email", "mesh@example.invalid"]);
        fs::write(source.join("tracked.txt"), b"baseline\n").expect("source file");
        git(&source, &["add", "tracked.txt"]);
        git(&source, &["commit", "-m", "baseline"]);
        fs::write(destination.join("tracked.txt"), b"baseline\n").expect("destination file");
        let expected = GitProvenanceAnchor::inspect(&source).expect("source anchor");
        let identity = GitDestinationIdentity::inspect(&destination).expect("admit destination");

        fs::rename(&destination, &displaced).expect("displace admitted destination");
        fs::create_dir(&destination).expect("replacement destination");
        fs::write(destination.join("tracked.txt"), b"baseline\n").expect("replacement file");

        let result = install_independent_git_context_for_destination(
            &source,
            &destination,
            identity,
            &expected,
        );
        assert!(
            matches!(result, Err(GitContextError::DestinationMismatch)),
            "replacement directory inherited admitted Git setup: {result:?}"
        );
        assert!(
            !destination.join(".git").exists(),
            "replacement directory received Git metadata"
        );
        assert!(
            !displaced.join(".git").exists(),
            "refusal unexpectedly mutated the displaced admitted directory"
        );
    }

    #[test]
    fn publication_cannot_follow_a_replaced_destination() {
        let fixture = Scratch::new("destination-replacement");
        let source = fixture.0.join("source");
        let destination = fixture.0.join("destination");
        let displaced = fixture.0.join("displaced");
        let outside = fixture.0.join("outside");
        fs::create_dir(&source).expect("source");
        fs::create_dir(&destination).expect("destination");
        fs::create_dir(&outside).expect("outside");
        git(&source, &["init", "-b", "main"]);
        git(&source, &["config", "user.name", "Mesh test"]);
        git(&source, &["config", "user.email", "mesh@example.invalid"]);
        fs::write(source.join("tracked.txt"), b"baseline\n").expect("source file");
        git(&source, &["add", "tracked.txt"]);
        git(&source, &["commit", "-m", "baseline"]);
        fs::write(destination.join("tracked.txt"), b"baseline\n").expect("destination file");
        let expected = GitProvenanceAnchor::inspect(&source).expect("source anchor");

        let destination_for_hook = destination.clone();
        let displaced_for_hook = displaced.clone();
        let outside_for_hook = outside.clone();
        BEFORE_GIT_PUBLICATION.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                fs::rename(&destination_for_hook, &displaced_for_hook)
                    .expect("displace admitted destination");
                symlink(&outside_for_hook, &destination_for_hook)
                    .expect("replace destination with outside link");
            }));
        });

        let result = install_independent_git_context(&source, &destination, &expected);
        assert!(
            result.is_err(),
            "a replaced destination inherited Git context"
        );
        assert!(
            !outside.join(".git").exists(),
            "Git metadata escaped into the replacement directory"
        );
    }

    #[test]
    fn git_commands_never_follow_a_substituted_git_directory() {
        let fixture = Scratch::new("git-entry-race");
        let source = fixture.0.join("source");
        let destination = fixture.0.join("destination");
        fs::create_dir(&source).expect("source");
        fs::create_dir(&destination).expect("destination");
        git(&source, &["init", "-b", "main"]);
        git(&source, &["config", "user.name", "Mesh test"]);
        git(&source, &["config", "user.email", "mesh@example.invalid"]);
        fs::write(source.join("tracked.txt"), b"baseline\n").expect("source file");
        git(&source, &["add", "tracked.txt"]);
        git(&source, &["commit", "-m", "baseline"]);
        fs::write(destination.join("tracked.txt"), b"baseline\n").expect("destination file");
        let expected = GitProvenanceAnchor::inspect(&source).expect("source anchor");

        let displaced_git = fixture.0.join("displaced-git");
        let destination_for_hook = destination.clone();
        let displaced_for_hook = displaced_git.clone();
        AFTER_GIT_INIT.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                let staging_name = fs::read_dir(&destination_for_hook)
                    .expect("destination entries")
                    .filter_map(Result::ok)
                    .map(|entry| entry.file_name())
                    .find(|name| {
                        let name = name.to_string_lossy();
                        name.starts_with(".mesh-git-") && name.ends_with(".tmp")
                    })
                    .expect("private Git staging directory");
                fs::rename(
                    destination_for_hook.join(&staging_name),
                    &displaced_for_hook,
                )
                .expect("displace admitted Git directory");
                fs::create_dir(destination_for_hook.join(&staging_name))
                    .expect("replacement Git directory");
                fs::write(
                    destination_for_hook.join(&staging_name).join("sentinel"),
                    b"replacement\n",
                )
                .expect("replacement sentinel");
            }));
        });

        let result = install_independent_git_context(&source, &destination, &expected);
        assert!(
            matches!(result, Err(GitContextError::DestinationMismatch)),
            "substituted Git directory was accepted: {result:?}"
        );
        let replacement_git = fs::read_dir(&destination)
            .expect("destination entries")
            .filter_map(Result::ok)
            .find(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                name.starts_with(".mesh-git-") && name.ends_with(".tmp")
            })
            .expect("retained replacement Git directory")
            .path();
        assert_eq!(
            fs::read(replacement_git.join("sentinel")).expect("replacement sentinel"),
            b"replacement\n",
            "a Git subprocess wrote through the substituted name"
        );
        assert_eq!(
            fs::read_dir(&replacement_git)
                .expect("replacement Git directory")
                .count(),
            1,
            "the replacement Git directory received Mesh metadata"
        );
        assert!(
            !destination.join(".git").exists(),
            "a substituted staging directory was published"
        );
        assert!(
            displaced_git.join("objects").is_dir(),
            "Git work remained bound to the exact retained directory"
        );
    }

    #[test]
    fn failed_create_only_git_setup_cleans_exact_entry_and_allows_retry() {
        let fixture = Scratch::new("git-create-failure-retry");
        let source = fixture.0.join("source");
        let destination = fixture.0.join("destination");
        fs::create_dir(&source).expect("source");
        fs::create_dir(&destination).expect("destination");
        git(&source, &["init", "-b", "main"]);
        git(&source, &["config", "user.name", "Mesh test"]);
        git(&source, &["config", "user.email", "mesh@example.invalid"]);
        fs::write(source.join("tracked.txt"), b"baseline\n").expect("source file");
        git(&source, &["add", "tracked.txt"]);
        git(&source, &["commit", "-m", "baseline"]);
        fs::write(destination.join("tracked.txt"), b"baseline\n").expect("destination file");
        let expected = GitProvenanceAnchor::inspect(&source).expect("source anchor");

        let destination_for_hook = destination.clone();
        AFTER_GIT_PUBLICATION.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                fs::write(
                    destination_for_hook.join("late.txt"),
                    b"changed after publication\n",
                )
                .expect("change worktree after create-only publication");
            }));
        });
        let result = install_independent_git_context(&source, &destination, &expected);
        assert!(
            matches!(result, Err(GitContextError::DestinationMismatch)),
            "forced post-publication failure had the wrong result: {result:?}"
        );
        assert!(
            !destination.join(".git").exists(),
            "an exact failed attempt left incomplete Git metadata"
        );
        fs::remove_file(destination.join("late.txt")).expect("restore exact worktree");
        install_independent_git_context(&source, &destination, &expected)
            .expect("clean retry succeeds");
        assert!(destination.join(".git").is_dir());
    }

    #[test]
    fn published_git_directory_cannot_be_renamed_and_substituted() {
        let fixture = Scratch::new("published-git-substitution");
        let source = fixture.0.join("source");
        let destination = fixture.0.join("destination");
        let displaced_git = fixture.0.join("displaced-published-git");
        fs::create_dir(&source).expect("source");
        fs::create_dir(&destination).expect("destination");
        git(&source, &["init", "-b", "main"]);
        git(&source, &["config", "user.name", "Mesh test"]);
        git(&source, &["config", "user.email", "mesh@example.invalid"]);
        fs::write(source.join("tracked.txt"), b"baseline\n").expect("source file");
        git(&source, &["add", "tracked.txt"]);
        git(&source, &["commit", "-m", "baseline"]);
        fs::write(destination.join("tracked.txt"), b"baseline\n").expect("destination file");
        let expected = GitProvenanceAnchor::inspect(&source).expect("source anchor");

        let destination_for_hook = destination.clone();
        let displaced_for_hook = displaced_git.clone();
        AFTER_GIT_PUBLICATION.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                fs::rename(destination_for_hook.join(".git"), &displaced_for_hook)
                    .expect("displace published Git directory");
                fs::create_dir(destination_for_hook.join(".git"))
                    .expect("replacement Git directory");
                fs::write(destination_for_hook.join(".git/sentinel"), b"replacement\n")
                    .expect("replacement sentinel");
            }));
        });

        let result = install_independent_git_context(&source, &destination, &expected);
        assert!(
            matches!(result, Err(GitContextError::DestinationMismatch)),
            "substituted published Git directory was accepted: {result:?}"
        );
        assert_eq!(
            fs::read(destination.join(".git/sentinel")).expect("replacement sentinel"),
            b"replacement\n",
            "a Git command or cleanup touched the replacement"
        );
        assert_eq!(
            fs::read_dir(destination.join(".git"))
                .expect("replacement Git directory")
                .count(),
            1,
            "the replacement Git directory received Mesh metadata"
        );
        assert!(
            displaced_git.join("objects").is_dir(),
            "Git operations remained bound to the displaced admitted directory"
        );
    }

    #[test]
    fn same_commit_symbolic_head_substitution_is_refused() {
        let fixture = Scratch::new("symbolic-head-substitution");
        let source = fixture.0.join("source");
        let destination = fixture.0.join("destination");
        fs::create_dir(&source).expect("source");
        fs::create_dir(&destination).expect("destination");
        git(&source, &["init", "-b", "main"]);
        git(&source, &["config", "user.name", "Mesh test"]);
        git(&source, &["config", "user.email", "mesh@example.invalid"]);
        fs::write(source.join("tracked.txt"), b"baseline\n").expect("source file");
        git(&source, &["add", "tracked.txt"]);
        git(&source, &["commit", "-m", "baseline"]);
        fs::write(destination.join("tracked.txt"), b"baseline\n").expect("destination file");
        let expected = GitProvenanceAnchor::inspect(&source).expect("source anchor");

        let destination_for_hook = destination.clone();
        let expected_head = expected.head().as_str().to_owned();
        AFTER_GIT_PUBLICATION.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                let git_directory = destination_for_hook.join(".git");
                let update = Command::new("git")
                    .arg("--git-dir")
                    .arg(&git_directory)
                    .args(["update-ref", "refs/heads/other", &expected_head])
                    .output()
                    .expect("create same-commit alternate branch");
                assert!(update.status.success());
                let switch = Command::new("git")
                    .arg("--git-dir")
                    .arg(&git_directory)
                    .args(["symbolic-ref", "HEAD", "refs/heads/other"])
                    .output()
                    .expect("switch symbolic HEAD");
                assert!(switch.status.success());
            }));
        });

        let result = install_independent_git_context(&source, &destination, &expected);
        assert!(
            matches!(result, Err(GitContextError::DestinationMismatch)),
            "same-commit branch substitution was accepted: {result:?}"
        );
        assert!(
            !destination.join(".git").exists(),
            "a failed branch verification left the incomplete Git context installed"
        );
    }

    #[test]
    fn staging_cannot_follow_a_replaced_parent() {
        let fixture = Scratch::new("parent-replacement");
        let source = fixture.0.join("source");
        let parent = fixture.0.join("versions");
        let destination = parent.join("destination");
        let displaced = fixture.0.join("displaced-versions");
        let outside = fixture.0.join("outside");
        fs::create_dir(&source).expect("source");
        fs::create_dir(&parent).expect("parent");
        fs::create_dir(&destination).expect("destination");
        fs::create_dir(&outside).expect("outside");
        git(&source, &["init", "-b", "main"]);
        git(&source, &["config", "user.name", "Mesh test"]);
        git(&source, &["config", "user.email", "mesh@example.invalid"]);
        fs::write(source.join("tracked.txt"), b"baseline\n").expect("source file");
        git(&source, &["add", "tracked.txt"]);
        git(&source, &["commit", "-m", "baseline"]);
        fs::write(destination.join("tracked.txt"), b"baseline\n").expect("destination file");
        let expected = GitProvenanceAnchor::inspect(&source).expect("source anchor");

        let parent_for_hook = parent.clone();
        let displaced_for_hook = displaced.clone();
        let outside_for_hook = outside.clone();
        AFTER_GIT_INIT.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                fs::rename(&parent_for_hook, &displaced_for_hook)
                    .expect("displace admitted parent");
                fs::create_dir(outside_for_hook.join("destination")).expect("outside destination");
                fs::write(
                    outside_for_hook.join("destination/tracked.txt"),
                    b"outside\n",
                )
                .expect("outside working file");
                for entry in fs::read_dir(&displaced_for_hook).expect("staging names") {
                    let entry = entry.expect("staging entry");
                    let name = entry.file_name();
                    if name.to_string_lossy().ends_with(".tmp") {
                        let replacement = outside_for_hook.join(&name);
                        fs::create_dir(&replacement).expect("outside staging directory");
                        fs::write(replacement.join("sentinel"), b"outside staging\n")
                            .expect("outside staging sentinel");
                    } else if name.to_string_lossy().ends_with(".bundle") {
                        fs::write(outside_for_hook.join(&name), b"outside bundle\n")
                            .expect("outside bundle sentinel");
                    }
                }
                symlink(&outside_for_hook, &parent_for_hook)
                    .expect("replace parent with outside link");
            }));
        });

        let result = install_independent_git_context(&source, &destination, &expected);
        assert!(result.is_err(), "a replaced parent inherited Git staging");
        assert_eq!(
            fs::read(outside.join("destination/tracked.txt")).expect("outside sentinel"),
            b"outside\n"
        );
        let protected_entries = fs::read_dir(&outside)
            .expect("outside entries")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".mesh-git-")
            })
            .collect::<Vec<_>>();
        assert_eq!(
            protected_entries.len(),
            0,
            "predictable Git or bundle staging names escaped into the replacement parent"
        );
    }

    #[test]
    fn inherited_git_environment_cannot_redirect_native_history_writes() {
        let fixture = Scratch::new("inherited-environment");
        let source = fixture.0.join("source");
        let destination = fixture.0.join("destination");
        let outside_index = fixture.0.join("outside.index");
        fs::create_dir(&source).expect("source");
        fs::create_dir(&destination).expect("destination");
        git(&source, &["init", "-b", "main"]);
        git(&source, &["config", "user.name", "Mesh test"]);
        git(&source, &["config", "user.email", "mesh@example.invalid"]);
        fs::write(source.join("tracked.txt"), b"baseline\n").expect("source file");
        git(&source, &["add", "tracked.txt"]);
        git(&source, &["commit", "-m", "baseline"]);
        fs::write(destination.join("tracked.txt"), b"baseline\n").expect("destination file");

        let output = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "workspace::replacement_tests::git_environment_helper",
                "--ignored",
                "--nocapture",
            ])
            .env("MESH_GIT_ENV_SOURCE", &source)
            .env("MESH_GIT_ENV_DESTINATION", &destination)
            .env("GIT_INDEX_FILE", &outside_index)
            .output()
            .expect("run isolated hostile-environment helper");
        assert!(
            output.status.success(),
            "hostile-environment helper failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !outside_index.exists(),
            "an inherited Git index override received native-history writes"
        );
        assert!(destination.join(".git").is_dir());
    }

    #[test]
    #[ignore = "child-process helper for hostile inherited Git environment"]
    fn git_environment_helper() {
        let Some(source) = std::env::var_os("MESH_GIT_ENV_SOURCE").map(PathBuf::from) else {
            return;
        };
        let destination = PathBuf::from(
            std::env::var_os("MESH_GIT_ENV_DESTINATION").expect("helper destination"),
        );
        let expected = GitProvenanceAnchor::inspect(&source).expect("source anchor");
        install_independent_git_context(&source, &destination, &expected)
            .expect("isolated Git context");
    }
}
