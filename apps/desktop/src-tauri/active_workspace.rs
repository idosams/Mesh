//! One stable native path for the workspace currently selected in the desktop app.
//!
//! The link is navigation, never workspace authority. The daemon opens and pins the real
//! presented directory first; only then may the desktop point this app-owned link at that exact
//! directory. Keeping the link below an owner-only application directory also means switching
//! versions never replaces an arbitrary user file or follows a caller-provided link.

use std::ffi::{CString, OsStr, OsString};
use std::fmt;
use std::fs::{self, File};
use std::os::fd::{AsRawFd as _, FromRawFd as _};
use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const DIRECTORY_NAME: &str = "native-workspace";
const LINK_NAME: &str = "current";
static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(1);

/// App-owned stable path that follows only a daemon-verified native workspace folder.
#[derive(Clone, Debug)]
pub struct ActiveWorkspaceLink {
    directory: PathBuf,
    link: PathBuf,
}

/// Exact navigation entry published by one successful activation attempt.
///
/// Keeping its identity lets a later verification failure withdraw only that entry. A same-user
/// process that replaces `current` after publication is never mistaken for this activation.
#[derive(Debug)]
pub struct ActiveWorkspaceActivation {
    path: PathBuf,
    directory_identity: (u64, u64),
    _link_handle: File,
    link_identity: (u64, u64),
    target: PathBuf,
}

#[derive(Debug)]
struct InspectedWorkspaceLink {
    directory_identity: (u64, u64),
    _handle: File,
    link_identity: (u64, u64),
    target: PathBuf,
}

impl ActiveWorkspaceActivation {
    /// Stable native navigation path for Finder and editors that should follow the current
    /// selection. Long-running agents receive the verified real workspace directory instead.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Withdraw this exact activation if it still owns the stable path.
    pub fn deactivate_if_unchanged(
        &self,
        active: &ActiveWorkspaceLink,
    ) -> Result<(), ActiveWorkspaceLinkError> {
        remove_current_if_owned(
            &active.directory,
            &active.link,
            self.directory_identity,
            Some((self.link_identity, self.target.as_path())),
            None,
        )
    }
}

impl ActiveWorkspaceLink {
    /// Place the stable path below the desktop's private application-data directory.
    #[must_use]
    pub fn new(application_data: &Path) -> Self {
        let directory = application_data.join(DIRECTORY_NAME);
        let link = directory.join(LINK_NAME);
        Self { directory, link }
    }

    /// Atomically point the stable path at one already-verified real directory.
    #[cfg(test)]
    pub fn activate(&self, target: &Path) -> Result<PathBuf, ActiveWorkspaceLinkError> {
        self.activate_with_policy(target, || {}, false)
            .map(|activation| activation.path)
    }

    /// Atomically point the stable path at a verified directory, removing only navigation links
    /// this exact attempt inspected or created when publication fails.
    ///
    /// The desktop uses this form because a failed workspace transition must not leave its prior
    /// link advertised as current. A same-user replacement that wins the publication race is not
    /// owned by this attempt and is preserved.
    pub fn activate_or_deactivate(
        &self,
        target: &Path,
    ) -> Result<ActiveWorkspaceActivation, ActiveWorkspaceLinkError> {
        let prior = self.inspected_current_link()?;
        match self.activate_with_policy(target, || {}, true) {
            Ok(activation) => Ok(activation),
            Err(error) => {
                if let Some(prior) = prior {
                    remove_current_if_owned(
                        &self.directory,
                        &self.link,
                        prior.directory_identity,
                        None,
                        Some((prior.link_identity, prior.target.as_path())),
                    )?;
                }
                Err(error)
            }
        }
    }

    #[cfg(test)]
    fn activate_with(
        &self,
        target: &Path,
        before_publish: impl FnOnce(),
    ) -> Result<ActiveWorkspaceActivation, ActiveWorkspaceLinkError> {
        self.activate_with_policy(target, before_publish, false)
    }

    fn activate_with_policy(
        &self,
        target: &Path,
        before_publish: impl FnOnce(),
        deactivate_owned_on_failure: bool,
    ) -> Result<ActiveWorkspaceActivation, ActiveWorkspaceLinkError> {
        self.activate_with_hooks(target, || {}, before_publish, deactivate_owned_on_failure)
    }

    fn activate_with_hooks(
        &self,
        target: &Path,
        before_directory_use: impl FnOnce(),
        before_publish: impl FnOnce(),
        deactivate_owned_on_failure: bool,
    ) -> Result<ActiveWorkspaceActivation, ActiveWorkspaceLinkError> {
        let requested_metadata = fs::symlink_metadata(target)
            .map_err(|error| ActiveWorkspaceLinkError::io("inspect workspace", error))?;
        if !requested_metadata.file_type().is_dir() || requested_metadata.file_type().is_symlink() {
            return Err(ActiveWorkspaceLinkError::Invalid(
                "workspace is not a real directory",
            ));
        }
        let canonical = fs::canonicalize(target)
            .map_err(|error| ActiveWorkspaceLinkError::io("resolve workspace", error))?;
        if canonical != target {
            return Err(ActiveWorkspaceLinkError::Invalid(
                "workspace path is not canonical",
            ));
        }
        let target_metadata = fs::symlink_metadata(&canonical)
            .map_err(|error| ActiveWorkspaceLinkError::io("inspect workspace", error))?;
        if !target_metadata.file_type().is_dir() || target_metadata.file_type().is_symlink() {
            return Err(ActiveWorkspaceLinkError::Invalid(
                "workspace is not a real directory",
            ));
        }

        let directory_identity = self.ensure_private_directory()?;
        before_directory_use();
        let opened_directory =
            ensure_private_directory_identity(&self.directory, directory_identity)?;
        // Keep the inspected link itself open until publication and cleanup finish. On Unix an
        // open descriptor pins the unlinked inode, so a same-user replacement cannot recycle the
        // exact (device, inode) pair and impersonate the navigation entry we inspected.
        let replaced_link =
            match open_inspected_symlink_at(&opened_directory, OsStr::new(LINK_NAME)) {
                Ok(link) => Some(link),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
                    return Err(ActiveWorkspaceLinkError::Invalid(
                        "stable path was replaced by a non-link entry",
                    ))
                }
                Err(error) => {
                    return Err(ActiveWorkspaceLinkError::io("inspect stable path", error))
                }
            };

        let temporary_name = OsString::from(format!(
            ".current-{}-{}.tmp",
            std::process::id(),
            NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
        ));
        let mut created_temporary = None;
        let result = (|| {
            symlink_at(&opened_directory, &canonical, &temporary_name)
                .map_err(|error| ActiveWorkspaceLinkError::io("create stable path", error))?;
            let (temporary_handle, temporary_identity, temporary_target) =
                open_inspected_symlink_at(&opened_directory, &temporary_name)
                    .map_err(|error| ActiveWorkspaceLinkError::io("inspect stable path", error))?;
            if temporary_target != canonical {
                return Err(ActiveWorkspaceLinkError::Invalid(
                    "temporary stable path did not name the verified workspace",
                ));
            }
            created_temporary = Some((temporary_handle, temporary_identity));
            before_publish();
            let publish_directory =
                ensure_private_directory_identity(&self.directory, directory_identity)?;
            publish_link_without_replacing_entry(
                &publish_directory,
                &temporary_name,
                OsStr::new(LINK_NAME),
                replaced_link.as_ref().map(|(_, identity, _)| *identity),
            )?;
            publish_directory.sync_all().map_err(|error| {
                ActiveWorkspaceLinkError::io("sync stable-path directory", error)
            })?;
            if !symlink_identity_at(&publish_directory, OsStr::new(LINK_NAME))
                .is_ok_and(|identity| identity == temporary_identity)
                || !read_link_at(&publish_directory, OsStr::new(LINK_NAME))
                    .is_ok_and(|target| target == canonical.as_os_str())
            {
                return Err(ActiveWorkspaceLinkError::Invalid(
                    "published stable path did not name the verified workspace",
                ));
            }
            ensure_private_directory_identity(&self.directory, directory_identity)?;
            let (link_handle, link_identity) =
                created_temporary
                    .take()
                    .ok_or(ActiveWorkspaceLinkError::Invalid(
                        "stable path identity was not retained",
                    ))?;
            Ok(ActiveWorkspaceActivation {
                path: self.link.clone(),
                directory_identity,
                _link_handle: link_handle,
                link_identity,
                target: canonical.clone(),
            })
        })();
        if let Err(error) = result {
            // A failed exchange rollback can leave a foreign replacement at the temporary name.
            // Clean up only the exact symlink this activation created; never unlink by pathname
            // alone after the publish boundary has admitted same-user contention.
            if let Some((_, identity)) = created_temporary.as_ref() {
                let _ = remove_symlink_if_unchanged_at(
                    &opened_directory,
                    &temporary_name,
                    *identity,
                    &canonical,
                );
            }
            if deactivate_owned_on_failure {
                remove_current_if_owned(
                    &self.directory,
                    &self.link,
                    directory_identity,
                    created_temporary
                        .as_ref()
                        .map(|(_, identity)| (*identity, canonical.as_path())),
                    replaced_link
                        .as_ref()
                        .map(|(_, identity, target)| (*identity, target.as_path())),
                )?;
            }
            return Err(error);
        }
        result
    }

    fn inspected_current_link(
        &self,
    ) -> Result<Option<InspectedWorkspaceLink>, ActiveWorkspaceLinkError> {
        let directory = match fs::symlink_metadata(&self.directory) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(ActiveWorkspaceLinkError::io(
                    "inspect stable-path directory",
                    error,
                ))
            }
            Ok(metadata) if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() => {
                return Err(ActiveWorkspaceLinkError::Invalid(
                    "stable-path directory is not a real directory",
                ))
            }
            Ok(metadata) if metadata.permissions().mode() & 0o077 != 0 => {
                return Err(ActiveWorkspaceLinkError::Invalid(
                    "stable-path directory is not owner-only",
                ))
            }
            Ok(metadata) => (metadata.dev(), metadata.ino()),
        };
        let opened_directory = ensure_private_directory_identity(&self.directory, directory)?;
        match open_inspected_symlink_at(&opened_directory, OsStr::new(LINK_NAME)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) if error.kind() == std::io::ErrorKind::InvalidData => Err(
                ActiveWorkspaceLinkError::Invalid("stable path was replaced by a non-link entry"),
            ),
            Err(error) => Err(ActiveWorkspaceLinkError::io("inspect stable path", error)),
            Ok((handle, link_identity, target)) => Ok(Some(InspectedWorkspaceLink {
                directory_identity: directory,
                _handle: handle,
                link_identity,
                target,
            })),
        }
    }

    /// Remove only the app-owned navigation link; never remove its target or a replacement entry.
    pub fn deactivate(&self) -> Result<(), ActiveWorkspaceLinkError> {
        self.deactivate_with(|| {})
    }

    /// Withdraw the current navigation only when it still names the rejected workspace.
    ///
    /// Another desktop process may already have moved the stable path to a different verified
    /// workspace. That later selection wins and must not be removed by stale error handling.
    pub fn deactivate_matching_target(
        &self,
        expected_target: &Path,
    ) -> Result<(), ActiveWorkspaceLinkError> {
        let Some(current) = self.inspected_current_link()? else {
            return Ok(());
        };
        if current.target != expected_target {
            return Ok(());
        }
        remove_current_if_owned(
            &self.directory,
            &self.link,
            current.directory_identity,
            Some((current.link_identity, current.target.as_path())),
            None,
        )
    }

    fn deactivate_with(
        &self,
        before_remove: impl FnOnce(),
    ) -> Result<(), ActiveWorkspaceLinkError> {
        let directory_identity = match fs::symlink_metadata(&self.directory) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(ActiveWorkspaceLinkError::io(
                    "inspect stable-path directory",
                    error,
                ))
            }
            Ok(metadata) if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() => {
                return Err(ActiveWorkspaceLinkError::Invalid(
                    "stable-path directory is not a real directory",
                ))
            }
            Ok(metadata) if metadata.permissions().mode() & 0o077 != 0 => {
                return Err(ActiveWorkspaceLinkError::Invalid(
                    "stable-path directory is not owner-only",
                ))
            }
            Ok(metadata) => (metadata.dev(), metadata.ino()),
        };
        let opened_directory =
            ensure_private_directory_identity(&self.directory, directory_identity)?;
        match open_inspected_symlink_at(&opened_directory, OsStr::new(LINK_NAME)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::InvalidData => Err(
                ActiveWorkspaceLinkError::Invalid("stable path was replaced by a non-link entry"),
            ),
            Err(error) => Err(ActiveWorkspaceLinkError::io("inspect stable path", error)),
            Ok((_handle, identity, target)) => {
                before_remove();
                remove_link_if_unchanged(
                    &self.directory,
                    &self.link,
                    directory_identity,
                    identity,
                    &target,
                )?;
                ensure_private_directory_identity(&self.directory, directory_identity)?
                    .sync_all()
                    .map_err(|error| {
                        ActiveWorkspaceLinkError::io("sync stable-path directory", error)
                    })
            }
        }
    }

    fn ensure_private_directory(&self) -> Result<(u64, u64), ActiveWorkspaceLinkError> {
        match fs::symlink_metadata(&self.directory) {
            Ok(metadata) => {
                if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
                    return Err(ActiveWorkspaceLinkError::Invalid(
                        "stable-path directory is not a real directory",
                    ));
                }
                if metadata.permissions().mode() & 0o077 != 0 {
                    return Err(ActiveWorkspaceLinkError::Invalid(
                        "stable-path directory is not owner-only",
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::DirBuilder::new()
                    .mode(0o700)
                    .create(&self.directory)
                    .map_err(|error| {
                        ActiveWorkspaceLinkError::io("create stable-path directory", error)
                    })?;
            }
            Err(error) => {
                return Err(ActiveWorkspaceLinkError::io(
                    "inspect stable-path directory",
                    error,
                ))
            }
        }
        let metadata = fs::symlink_metadata(&self.directory)
            .map_err(|error| ActiveWorkspaceLinkError::io("verify stable-path directory", error))?;
        if !metadata.file_type().is_dir()
            || metadata.file_type().is_symlink()
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(ActiveWorkspaceLinkError::Invalid(
                "stable-path directory is not owner-only",
            ));
        }
        Ok((metadata.dev(), metadata.ino()))
    }
}

fn ensure_private_directory_identity(
    directory: &Path,
    expected_identity: (u64, u64),
) -> Result<File, ActiveWorkspaceLinkError> {
    let path_metadata = fs::symlink_metadata(directory)
        .map_err(|error| ActiveWorkspaceLinkError::io("inspect stable-path directory", error))?;
    if !path_metadata.file_type().is_dir()
        || path_metadata.file_type().is_symlink()
        || path_metadata.permissions().mode() & 0o077 != 0
        || (path_metadata.dev(), path_metadata.ino()) != expected_identity
    {
        return Err(ActiveWorkspaceLinkError::Invalid(
            "stable-path directory changed after verification",
        ));
    }
    let opened = File::open(directory)
        .map_err(|error| ActiveWorkspaceLinkError::io("open stable-path directory", error))?;
    let opened_metadata = opened
        .metadata()
        .map_err(|error| ActiveWorkspaceLinkError::io("inspect stable-path directory", error))?;
    if !opened_metadata.file_type().is_dir()
        || (opened_metadata.dev(), opened_metadata.ino()) != expected_identity
    {
        return Err(ActiveWorkspaceLinkError::Invalid(
            "stable-path directory changed after verification",
        ));
    }
    Ok(opened)
}

fn remove_link_if_unchanged(
    directory: &Path,
    link: &Path,
    expected_directory_identity: (u64, u64),
    expected_identity: (u64, u64),
    expected_target: &Path,
) -> Result<(), ActiveWorkspaceLinkError> {
    let opened = ensure_private_directory_identity(directory, expected_directory_identity)?;
    let link_name = link
        .file_name()
        .ok_or(ActiveWorkspaceLinkError::Invalid("stable path has no name"))?;
    let retired_name = (0..64)
        .find_map(|_| {
            let candidate_name = OsString::from(format!(
                ".current-{}-{}.retired",
                std::process::id(),
                NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
            ));
            match atomic_rename_noreplace_at(&opened, link_name, &candidate_name) {
                Ok(()) => Some(Ok(candidate_name)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(Err(
                    ActiveWorkspaceLinkError::Invalid("stable path changed during deactivation"),
                )),
                Err(error) => Some(Err(ActiveWorkspaceLinkError::io(
                    "quarantine stable path",
                    error,
                ))),
            }
        })
        .transpose()?
        .ok_or(ActiveWorkspaceLinkError::Invalid(
            "stable path cleanup name unavailable",
        ))?;
    if symlink_identity_at(&opened, &retired_name)
        .is_ok_and(|identity| identity == expected_identity)
        && read_link_at(&opened, &retired_name)
            .is_ok_and(|target| target == expected_target.as_os_str())
    {
        return unlink_at(&opened, &retired_name)
            .map_err(|error| ActiveWorkspaceLinkError::io("remove stable path", error));
    }

    atomic_rename_noreplace_at(&opened, &retired_name, link_name).map_err(|error| {
        ActiveWorkspaceLinkError::io("restore stable path after a deactivation race", error)
    })?;
    Err(ActiveWorkspaceLinkError::Invalid(
        "stable path changed during deactivation",
    ))
}

fn remove_current_if_owned(
    directory: &Path,
    link: &Path,
    expected_directory_identity: (u64, u64),
    created: Option<((u64, u64), &Path)>,
    replaced: Option<((u64, u64), &Path)>,
) -> Result<(), ActiveWorkspaceLinkError> {
    let metadata = match fs::symlink_metadata(link) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(ActiveWorkspaceLinkError::io("inspect stable path", error)),
        Ok(metadata) if !metadata.file_type().is_symlink() => return Ok(()),
        Ok(metadata) => metadata,
    };
    let identity = (metadata.dev(), metadata.ino());
    let target = fs::read_link(link)
        .map_err(|error| ActiveWorkspaceLinkError::io("read stable path", error))?;
    let owned =
        [created, replaced]
            .into_iter()
            .flatten()
            .any(|(expected_identity, expected_target)| {
                identity == expected_identity && target == expected_target
            });
    if !owned {
        return Ok(());
    }
    remove_link_if_unchanged(
        directory,
        link,
        expected_directory_identity,
        identity,
        &target,
    )?;
    ensure_private_directory_identity(directory, expected_directory_identity)?
        .sync_all()
        .map_err(|error| ActiveWorkspaceLinkError::io("sync stable-path directory", error))
}

fn publish_link_without_replacing_entry(
    directory: &File,
    temporary_name: &OsStr,
    link_name: &OsStr,
    replaced_link_identity: Option<(u64, u64)>,
) -> Result<(), ActiveWorkspaceLinkError> {
    let Some(replaced_link_identity) = replaced_link_identity else {
        return atomic_rename_noreplace_at(directory, temporary_name, link_name)
            .map_err(|error| ActiveWorkspaceLinkError::io("publish stable path", error));
    };

    atomic_exchange_at(directory, temporary_name, link_name)
        .map_err(|error| ActiveWorkspaceLinkError::io("publish stable path", error))?;
    if symlink_identity_at(directory, temporary_name)
        .is_ok_and(|identity| identity == replaced_link_identity)
    {
        unlink_at(directory, temporary_name)
            .map_err(|error| ActiveWorkspaceLinkError::io("remove prior stable path", error))?;
        return Ok(());
    }

    // A same-user process replaced `current` after the preflight. This includes a replacement
    // symlink: being a link is not proof that it is the exact navigation entry we inspected.
    // Exchange the entry back before refusing so the app never deletes or strands that foreign
    // path. The newly created symlink returns to `temporary`, where the caller's ordinary error
    // cleanup may safely remove it.
    atomic_exchange_at(directory, temporary_name, link_name).map_err(|error| {
        ActiveWorkspaceLinkError::io("restore stable path after a replacement race", error)
    })?;
    Err(ActiveWorkspaceLinkError::Invalid(
        "stable path changed during activation",
    ))
}

#[allow(unsafe_code)]
fn symlink_at(directory: &File, target: &Path, name: &OsStr) -> std::io::Result<()> {
    unsafe extern "C" {
        fn symlinkat(
            target: *const std::ffi::c_char,
            directory_fd: i32,
            name: *const std::ffi::c_char,
        ) -> i32;
    }
    let target = CString::new(target.as_os_str().as_bytes()).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "target contains NUL")
    })?;
    let name = c_name(name)?;
    // SAFETY: both C strings are live and the descriptor is the verified app-owned directory.
    if unsafe { symlinkat(target.as_ptr(), directory.as_raw_fd(), name.as_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn open_symlink_at(directory: &File, name: &OsStr) -> std::io::Result<File> {
    const O_SYMLINK: i32 = 0x0020_0000;
    unsafe extern "C" {
        fn openat(directory_fd: i32, name: *const std::ffi::c_char, flags: i32) -> i32;
    }
    let name = c_name(name)?;
    // SAFETY: the name is a live C string. O_SYMLINK opens the link itself, never its target.
    let fd = unsafe { openat(directory.as_raw_fd(), name.as_ptr(), O_SYMLINK) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: openat returned a fresh owned descriptor.
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn open_symlink_at(directory: &File, name: &OsStr) -> std::io::Result<File> {
    const O_NOFOLLOW: i32 = 0o0040_0000;
    const O_PATH: i32 = 0o10_000_000;
    unsafe extern "C" {
        fn openat(directory_fd: i32, name: *const std::ffi::c_char, flags: i32) -> i32;
    }
    let name = c_name(name)?;
    // SAFETY: the name is a live C string. O_PATH|O_NOFOLLOW opens the link, not its target.
    let fd = unsafe { openat(directory.as_raw_fd(), name.as_ptr(), O_PATH | O_NOFOLLOW) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: openat returned a fresh owned descriptor.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn symlink_identity_at(directory: &File, name: &OsStr) -> std::io::Result<(u64, u64)> {
    let entry = open_symlink_at(directory, name)?;
    let metadata = entry.metadata()?;
    if !metadata.file_type().is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "entry is not a symbolic link",
        ));
    }
    Ok((metadata.dev(), metadata.ino()))
}

fn open_inspected_symlink_at(
    directory: &File,
    name: &OsStr,
) -> std::io::Result<(File, (u64, u64), PathBuf)> {
    let entry = open_symlink_at(directory, name)?;
    let metadata = entry.metadata()?;
    if !metadata.file_type().is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "entry is not a symbolic link",
        ));
    }
    let identity = (metadata.dev(), metadata.ino());
    let target = PathBuf::from(read_link_at(directory, name)?);
    if symlink_identity_at(directory, name)? != identity {
        return Err(std::io::Error::other(
            "symbolic link changed while it was inspected",
        ));
    }
    Ok((entry, identity, target))
}

#[allow(unsafe_code)]
fn read_link_at(directory: &File, name: &OsStr) -> std::io::Result<OsString> {
    unsafe extern "C" {
        fn readlinkat(
            directory_fd: i32,
            name: *const std::ffi::c_char,
            buffer: *mut std::ffi::c_char,
            size: usize,
        ) -> isize;
    }
    let name = c_name(name)?;
    let mut capacity = 256_usize;
    loop {
        let mut bytes = vec![0_u8; capacity];
        // SAFETY: the buffer is writable for capacity bytes and the name is a live C string.
        let read = unsafe {
            readlinkat(
                directory.as_raw_fd(),
                name.as_ptr(),
                bytes.as_mut_ptr().cast(),
                bytes.len(),
            )
        };
        if read < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let read = usize::try_from(read).map_err(|_| std::io::Error::other("invalid link size"))?;
        if read < bytes.len() {
            bytes.truncate(read);
            return Ok(OsString::from_vec(bytes));
        }
        capacity = capacity
            .checked_mul(2)
            .filter(|next| *next <= 1024 * 1024)
            .ok_or_else(|| std::io::Error::other("stable path target is too long"))?;
    }
}

#[allow(unsafe_code)]
fn unlink_at(directory: &File, name: &OsStr) -> std::io::Result<()> {
    unsafe extern "C" {
        fn unlinkat(directory_fd: i32, name: *const std::ffi::c_char, flags: i32) -> i32;
    }
    let name = c_name(name)?;
    // SAFETY: the name is a live C string and flags=0 removes only the named non-directory entry.
    if unsafe { unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

fn remove_symlink_if_unchanged_at(
    directory: &File,
    name: &OsStr,
    expected_identity: (u64, u64),
    expected_target: &Path,
) -> std::io::Result<()> {
    if symlink_identity_at(directory, name).is_ok_and(|identity| identity == expected_identity)
        && read_link_at(directory, name).is_ok_and(|target| target == expected_target.as_os_str())
    {
        unlink_at(directory, name)?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn atomic_rename_noreplace_at(
    directory: &File,
    from: &std::ffi::OsStr,
    to: &std::ffi::OsStr,
) -> std::io::Result<()> {
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
    let from = c_name(from)?;
    let to = c_name(to)?;
    // SAFETY: both names are live C strings and the descriptor is an open app-owned directory.
    if unsafe {
        renameatx_np(
            directory.as_raw_fd(),
            from.as_ptr(),
            directory.as_raw_fd(),
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
fn atomic_rename_noreplace_at(
    directory: &File,
    from: &std::ffi::OsStr,
    to: &std::ffi::OsStr,
) -> std::io::Result<()> {
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
    let from = c_name(from)?;
    let to = c_name(to)?;
    // SAFETY: both names are live C strings and the descriptor is an open app-owned directory.
    if unsafe {
        renameat2(
            directory.as_raw_fd(),
            from.as_ptr(),
            directory.as_raw_fd(),
            to.as_ptr(),
            RENAME_NOREPLACE,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn atomic_exchange_at(
    directory: &File,
    first: &std::ffi::OsStr,
    second: &std::ffi::OsStr,
) -> std::io::Result<()> {
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
    let first = c_name(first)?;
    let second = c_name(second)?;
    // SAFETY: both names are live C strings and the descriptor is an open app-owned directory.
    if unsafe {
        renameatx_np(
            directory.as_raw_fd(),
            first.as_ptr(),
            directory.as_raw_fd(),
            second.as_ptr(),
            RENAME_SWAP,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn atomic_exchange_at(
    directory: &File,
    first: &std::ffi::OsStr,
    second: &std::ffi::OsStr,
) -> std::io::Result<()> {
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
    let first = c_name(first)?;
    let second = c_name(second)?;
    // SAFETY: both names are live C strings and the descriptor is an open app-owned directory.
    if unsafe {
        renameat2(
            directory.as_raw_fd(),
            first.as_ptr(),
            directory.as_raw_fd(),
            second.as_ptr(),
            RENAME_EXCHANGE,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

fn c_name(name: &std::ffi::OsStr) -> std::io::Result<CString> {
    CString::new(name.as_bytes())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "name contains NUL"))
}

/// Why the native navigation link could not be activated.
#[derive(Debug)]
pub enum ActiveWorkspaceLinkError {
    Invalid(&'static str),
    Io {
        action: &'static str,
        source: std::io::Error,
    },
}

impl ActiveWorkspaceLinkError {
    fn io(action: &'static str, source: std::io::Error) -> Self {
        Self::Io { action, source }
    }
}

impl fmt::Display for ActiveWorkspaceLinkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(reason) => write!(formatter, "stable native workspace refused: {reason}"),
            Self::Io { action, source } => write!(formatter, "could not {action}: {source}"),
        }
    }
}

impl std::error::Error for ActiveWorkspaceLinkError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn scratch(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("mesh-desktop-active-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("scratch");
        path
    }

    #[test]
    fn stable_path_atomically_follows_the_selected_real_workspace() {
        let root = scratch("switch");
        let application = root.join("application");
        let first = root.join("version-one");
        let second = root.join("version-two");
        fs::create_dir(&application).expect("application");
        fs::create_dir(&first).expect("first workspace");
        fs::create_dir(&second).expect("second workspace");
        fs::write(first.join("version.txt"), "one").expect("first file");
        fs::write(second.join("version.txt"), "two").expect("second file");
        let first = fs::canonicalize(first).expect("canonical first workspace");
        let second = fs::canonicalize(second).expect("canonical second workspace");
        let active = ActiveWorkspaceLink::new(&application);

        let path = active.activate(&first).expect("first activation");
        assert_eq!(fs::read_to_string(path.join("version.txt")).unwrap(), "one");
        let same_path = active.activate(&second).expect("second activation");
        assert_eq!(same_path, path);
        assert_eq!(fs::read_to_string(path.join("version.txt")).unwrap(), "two");
        assert_eq!(fs::read_link(&path).unwrap(), second);
        assert_eq!(
            fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn linked_targets_and_replacement_entries_fail_closed() {
        let root = scratch("refusals");
        let application = root.join("application");
        let real = root.join("real");
        let linked = root.join("linked");
        fs::create_dir(&application).expect("application");
        fs::create_dir(&real).expect("real workspace");
        symlink(&real, &linked).expect("linked workspace");
        let active = ActiveWorkspaceLink::new(&application);
        assert!(active.activate(&linked).is_err());

        fs::create_dir(&active.directory).expect("stable directory");
        fs::write(&active.link, "do not replace").expect("replacement file");
        assert!(active.activate(&real).is_err());
        assert_eq!(fs::read_to_string(&active.link).unwrap(), "do not replace");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_linked_stable_directory_is_never_followed() {
        let root = scratch("linked-directory");
        let application = root.join("application");
        let outside = root.join("outside");
        let target = root.join("workspace");
        fs::create_dir(&application).expect("application");
        fs::create_dir(&outside).expect("outside");
        fs::create_dir(&target).expect("workspace");
        symlink(&outside, application.join(DIRECTORY_NAME)).expect("redirect stable directory");
        let active = ActiveWorkspaceLink::new(&application);
        assert!(active.activate(&target).is_err());
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_shared_stable_directory_is_not_repaired_or_written() {
        let root = scratch("shared-directory");
        let application = root.join("application");
        let target = fs::canonicalize(&root)
            .expect("canonical root")
            .join("workspace");
        fs::create_dir(&application).expect("application");
        fs::create_dir(&target).expect("workspace");
        let active = ActiveWorkspaceLink::new(&application);
        fs::create_dir(&active.directory).expect("stable directory");
        fs::set_permissions(&active.directory, fs::Permissions::from_mode(0o755))
            .expect("share stable directory");

        assert!(active.activate(&target).is_err());
        assert_eq!(
            fs::metadata(&active.directory)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
        assert_eq!(fs::read_dir(&active.directory).unwrap().count(), 0);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn deactivation_removes_only_the_link_and_preserves_replacements() {
        let root = scratch("deactivate");
        let application = root.join("application");
        let target = fs::canonicalize(&root)
            .expect("canonical root")
            .join("workspace");
        fs::create_dir(&application).expect("application");
        fs::create_dir(&target).expect("workspace");
        let active = ActiveWorkspaceLink::new(&application);
        let link = active.activate(&target).expect("activate");
        active.deactivate().expect("deactivate");
        assert!(!link.exists());
        assert!(target.is_dir());

        fs::write(&link, "replacement").expect("replacement");
        assert!(active.deactivate().is_err());
        assert_eq!(fs::read_to_string(link).unwrap(), "replacement");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn activation_never_overwrites_a_non_link_inserted_after_preflight() {
        let root = scratch("publish-race");
        let application = root.join("application");
        let first = root.join("version-one");
        let second = root.join("version-two");
        fs::create_dir(&application).expect("application");
        fs::create_dir(&first).expect("first workspace");
        fs::create_dir(&second).expect("second workspace");
        let first = fs::canonicalize(first).expect("canonical first workspace");
        let second = fs::canonicalize(second).expect("canonical second workspace");
        let active = ActiveWorkspaceLink::new(&application);
        active.activate(&first).expect("initial activation");

        let link = active.link.clone();
        let refusal = active
            .activate_with(&second, || {
                fs::remove_file(&link).expect("remove checked link");
                fs::write(&link, "foreign data").expect("insert replacement file");
            })
            .expect_err("a replacement at the publish boundary must win");

        assert!(refusal.to_string().contains("changed during activation"));
        assert_eq!(fs::read_to_string(&link).unwrap(), "foreign data");
        assert_eq!(fs::read_dir(link.parent().unwrap()).unwrap().count(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn activation_never_deletes_a_symlink_inserted_after_preflight() {
        let root = scratch("publish-symlink-race");
        let application = root.join("application");
        let first = root.join("version-one");
        let second = root.join("version-two");
        let foreign = root.join("foreign-version");
        fs::create_dir(&application).expect("application");
        fs::create_dir(&first).expect("first workspace");
        fs::create_dir(&second).expect("second workspace");
        fs::create_dir(&foreign).expect("foreign workspace");
        let first = fs::canonicalize(first).expect("canonical first workspace");
        let second = fs::canonicalize(second).expect("canonical second workspace");
        let foreign = fs::canonicalize(foreign).expect("canonical foreign workspace");
        let active = ActiveWorkspaceLink::new(&application);
        active.activate(&first).expect("initial activation");

        let link = active.link.clone();
        let refusal = active
            .activate_with(&second, || {
                fs::remove_file(&link).expect("remove checked link");
                symlink(&foreign, &link).expect("insert replacement link");
            })
            .expect_err("a replacement link at the publish boundary must win");

        assert!(refusal.to_string().contains("changed during activation"));
        assert_eq!(fs::read_link(&link).unwrap(), foreign);
        assert_eq!(fs::read_dir(link.parent().unwrap()).unwrap().count(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn failed_desktop_transition_preserves_a_symlink_that_wins_publication_race() {
        let root = scratch("desktop-publish-symlink-race");
        let application = root.join("application");
        let first = root.join("version-one");
        let second = root.join("version-two");
        let foreign = root.join("foreign-version");
        fs::create_dir(&application).expect("application");
        fs::create_dir(&first).expect("first workspace");
        fs::create_dir(&second).expect("second workspace");
        fs::create_dir(&foreign).expect("foreign workspace");
        let first = fs::canonicalize(first).expect("canonical first workspace");
        let second = fs::canonicalize(second).expect("canonical second workspace");
        let foreign = fs::canonicalize(foreign).expect("canonical foreign workspace");
        let active = ActiveWorkspaceLink::new(&application);
        active.activate(&first).expect("initial activation");

        let link = active.link.clone();
        let refusal = active
            .activate_with_policy(
                &second,
                || {
                    fs::remove_file(&link).expect("remove checked link");
                    symlink(&foreign, &link).expect("insert replacement link");
                },
                true,
            )
            .expect_err("a competing replacement must win the desktop transition");

        assert!(refusal.to_string().contains("changed during activation"));
        assert_eq!(fs::read_link(&link).unwrap(), foreign);
        assert_eq!(fs::read_dir(link.parent().unwrap()).unwrap().count(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn post_activation_refusal_withdraws_only_the_exact_published_link() {
        let root = scratch("tracked-deactivation-race");
        let application = root.join("application");
        let target = root.join("version-one");
        let foreign = root.join("foreign-version");
        fs::create_dir(&application).expect("application");
        fs::create_dir(&target).expect("workspace");
        fs::create_dir(&foreign).expect("foreign workspace");
        let target = fs::canonicalize(target).expect("canonical workspace");
        let foreign = fs::canonicalize(foreign).expect("canonical foreign workspace");
        let active = ActiveWorkspaceLink::new(&application);
        let activation = active
            .activate_or_deactivate(&target)
            .expect("tracked activation");

        fs::remove_file(activation.path()).expect("remove published link");
        symlink(&foreign, activation.path()).expect("insert replacement link");
        activation
            .deactivate_if_unchanged(&active)
            .expect("foreign replacement is not owned");

        assert_eq!(fs::read_link(activation.path()).unwrap(), foreign);
        assert_eq!(
            fs::read_dir(activation.path().parent().unwrap())
                .unwrap()
                .count(),
            1
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn invalid_transition_withdraws_the_exact_prior_link() {
        let root = scratch("invalid-transition-cleanup");
        let application = root.join("application");
        let first = root.join("version-one");
        let missing = root.join("missing-version");
        fs::create_dir(&application).expect("application");
        fs::create_dir(&first).expect("workspace");
        let first = fs::canonicalize(first).expect("canonical workspace");
        let active = ActiveWorkspaceLink::new(&application);
        let link = active.activate(&first).expect("initial activation");

        active
            .activate_or_deactivate(&missing)
            .expect_err("missing target must refuse");

        assert!(fs::symlink_metadata(link).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejected_workspace_cleanup_preserves_a_later_selection() {
        let root = scratch("rejected-workspace-later-selection");
        let application = root.join("application");
        let rejected = root.join("rejected-version");
        let later = root.join("later-version");
        fs::create_dir(&application).expect("application");
        fs::create_dir(&rejected).expect("rejected workspace");
        fs::create_dir(&later).expect("later workspace");
        let rejected = fs::canonicalize(rejected).expect("canonical rejected workspace");
        let later = fs::canonicalize(later).expect("canonical later workspace");
        let active = ActiveWorkspaceLink::new(&application);
        let stable = active.activate(&later).expect("later activation");

        active
            .deactivate_matching_target(&rejected)
            .expect("stale cleanup must be a no-op");

        assert_eq!(fs::read_link(stable).unwrap(), later);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn deactivation_never_deletes_a_symlink_inserted_after_preflight() {
        let root = scratch("deactivate-symlink-race");
        let application = root.join("application");
        let target = root.join("version-one");
        let foreign = root.join("foreign-version");
        fs::create_dir(&application).expect("application");
        fs::create_dir(&target).expect("workspace");
        fs::create_dir(&foreign).expect("foreign workspace");
        let target = fs::canonicalize(target).expect("canonical workspace");
        let foreign = fs::canonicalize(foreign).expect("canonical foreign workspace");
        let active = ActiveWorkspaceLink::new(&application);
        active.activate(&target).expect("initial activation");

        let link = active.link.clone();
        let refusal = active
            .deactivate_with(|| {
                fs::remove_file(&link).expect("remove checked link");
                symlink(&foreign, &link).expect("insert replacement link");
            })
            .expect_err("a replacement link at the removal boundary must win");

        assert!(refusal.to_string().contains("changed during deactivation"));
        assert_eq!(fs::read_link(&link).unwrap(), foreign);
        assert_eq!(fs::read_dir(link.parent().unwrap()).unwrap().count(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn activation_never_writes_through_a_directory_replaced_after_preflight() {
        let root = scratch("directory-replacement-race");
        let application = root.join("application");
        let first = root.join("version-one");
        let second = root.join("version-two");
        let outside = root.join("outside");
        let displaced = root.join("displaced-native-workspace");
        fs::create_dir(&application).expect("application");
        fs::create_dir(&first).expect("first workspace");
        fs::create_dir(&second).expect("second workspace");
        fs::create_dir(&outside).expect("outside");
        let first = fs::canonicalize(first).expect("canonical first workspace");
        let second = fs::canonicalize(second).expect("canonical second workspace");
        let active = ActiveWorkspaceLink::new(&application);
        active.activate(&first).expect("initial activation");

        let directory = active.directory.clone();
        let refusal = active
            .activate_with_hooks(
                &second,
                || {
                    fs::rename(&directory, &displaced).expect("displace checked directory");
                    symlink(&outside, &directory).expect("replace directory with link");
                },
                || {},
                false,
            )
            .expect_err("a replacement stable-path directory must never receive navigation data");

        assert!(refusal
            .to_string()
            .contains("stable-path directory changed"));
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        assert_eq!(fs::read_link(displaced.join(LINK_NAME)).unwrap(), first);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn deactivation_never_follows_a_directory_replaced_after_preflight() {
        let root = scratch("deactivate-directory-race");
        let application = root.join("application");
        let target = root.join("version-one");
        let outside = root.join("outside");
        let displaced = root.join("displaced-native-workspace");
        fs::create_dir(&application).expect("application");
        fs::create_dir(&target).expect("workspace");
        fs::create_dir(&outside).expect("outside");
        let target = fs::canonicalize(target).expect("canonical workspace");
        let active = ActiveWorkspaceLink::new(&application);
        active.activate(&target).expect("initial activation");

        let directory = active.directory.clone();
        let refusal = active
            .deactivate_with(|| {
                fs::rename(&directory, &displaced).expect("displace checked directory");
                symlink(&outside, &directory).expect("replace directory with link");
            })
            .expect_err("a replacement stable-path directory must never receive cleanup writes");

        assert!(refusal
            .to_string()
            .contains("stable-path directory changed"));
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        assert_eq!(fs::read_link(displaced.join(LINK_NAME)).unwrap(), target);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn activation_failure_cleanup_never_unlinks_through_a_replaced_directory() {
        let root = scratch("cleanup-directory-replacement-race");
        let application = root.join("application");
        let target = root.join("version-one");
        let outside = root.join("outside");
        let displaced = root.join("displaced-native-workspace");
        fs::create_dir(&application).expect("application");
        fs::create_dir(&target).expect("workspace");
        fs::create_dir(&outside).expect("outside");
        let target = fs::canonicalize(target).expect("canonical workspace");
        let active = ActiveWorkspaceLink::new(&application);

        let directory = active.directory.clone();
        let outside_for_hook = outside.clone();
        let target_for_hook = target.clone();
        let refusal = active
            .activate_with_hooks(
                &target,
                || {},
                || {
                    fs::rename(&directory, &displaced).expect("displace checked directory");
                    let temporary_name = fs::read_dir(&displaced)
                        .expect("read displaced directory")
                        .map(|entry| entry.expect("temporary entry").file_name())
                        .find(|name| name.to_string_lossy().ends_with(".tmp"))
                        .expect("activation temporary name");
                    symlink(&target_for_hook, outside_for_hook.join(temporary_name))
                        .expect("plant matching foreign symlink");
                    symlink(&outside_for_hook, &directory).expect("replace directory with link");
                },
                false,
            )
            .expect_err("replacement stable-path directory must refuse publication");

        assert!(refusal
            .to_string()
            .contains("stable-path directory changed"));
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
        let _ = fs::remove_dir_all(root);
    }
}
