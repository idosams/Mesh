//! The filesystem seam: the ten operations promotion is built out of.
//!
//! # Why the seam is this shape
//!
//! Atomicity here is not a property of any one call, it is a property of the *order* of several.
//! Data must be on the platter before the name that reveals it exists; the name must be on the
//! platter before anyone is told the chunk is there. A trait whose method were
//! `write_and_sync_and_rename` would hide exactly the thing that has to be right, and no test
//! could observe the order.
//!
//! So [`stage`](DurableFs::stage) and [`sync_file`](DurableFs::sync_file) are separate calls even
//! though the standard implementation could fuse them, and [`rename`](DurableFs::rename) and
//! [`sync_dir`](DurableFs::sync_dir) are separate from both. A recording implementation in
//! `tests/support/` then turns "data is synced before the rename that reveals it" from a comment
//! into an assertion — see `tests/promotion_ordering.rs`.
//!
//! # What this seam does not buy
//!
//! Substituting a filesystem lets a test observe order and inject failure. It does **not** let a
//! test prove that a real kernel honours `fsync`, or that a real filesystem's `rename` is atomic.
//! Those are platform guarantees this crate consumes; where they do not hold, the platform is
//! unsupported and says so — see [`sync_dir`](DurableFs::sync_dir) and `DURABILITY.md`.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// The filesystem operations the store needs, each one separately observable.
///
/// Implementations must not fuse operations: in particular `stage` must not sync, because the
/// whole point of `sync_file` being a distinct call is that a caller can be interrupted between
/// the two and a test can prove what that leaves behind.
pub trait DurableFs {
    /// Create a directory and every missing parent.
    ///
    /// # Errors
    ///
    /// Whatever the filesystem says.
    fn create_dir_all(&self, path: &Path) -> io::Result<()>;

    /// Create a file that must not already exist and write `bytes` into it, **without syncing**.
    ///
    /// `create_new` semantics are load-bearing: staging names are chosen to be unique, and a
    /// collision means a leftover from a crashed process rather than a name to overwrite.
    ///
    /// # Errors
    ///
    /// [`io::ErrorKind::AlreadyExists`] when the path is taken, or whatever the filesystem says.
    fn stage(&self, path: &Path, bytes: &[u8]) -> io::Result<()>;

    /// Append `bytes` to a file, creating it if absent, **without syncing**.
    ///
    /// # Errors
    ///
    /// Whatever the filesystem says.
    fn append(&self, path: &Path, bytes: &[u8]) -> io::Result<()>;

    /// Force this file's contents to durable storage.
    ///
    /// # Errors
    ///
    /// Whatever the filesystem says.
    fn sync_file(&self, path: &Path) -> io::Result<()>;

    /// Force this directory's entries to durable storage, so a rename into it survives a crash.
    ///
    /// # Errors
    ///
    /// [`io::ErrorKind::Unsupported`] on platforms where a directory cannot be synced, or whatever
    /// the filesystem says.
    fn sync_dir(&self, path: &Path) -> io::Result<()>;

    /// Move a file to a new name, atomically, replacing any existing entry.
    ///
    /// # Errors
    ///
    /// Whatever the filesystem says.
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;

    /// Read a whole file.
    ///
    /// # Errors
    ///
    /// Whatever the filesystem says.
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;

    /// Whether a path resolves to something.
    fn exists(&self, path: &Path) -> bool;

    /// How many bytes a file holds, without reading it.
    ///
    /// A default is provided so that adding this to the seam did not break every implementation of
    /// it, but the default is the *wrong* one for anything large: it reads the whole file. Any
    /// implementation over a real filesystem should override it with a `stat`, as [`StdFs`] does.
    /// The collector calls this once per candidate chunk, so a store being freed of a gigabyte
    /// would otherwise read that gigabyte in order to delete it.
    ///
    /// # Errors
    ///
    /// Whatever the filesystem says, including [`io::ErrorKind::NotFound`].
    fn file_len(&self, path: &Path) -> io::Result<u64> {
        let bytes = self.read(path)?;
        Ok(bytes.len() as u64)
    }

    /// Remove a file.
    ///
    /// # Errors
    ///
    /// Whatever the filesystem says.
    fn remove_file(&self, path: &Path) -> io::Result<()>;

    /// The entries directly inside a directory, in no guaranteed order.
    ///
    /// A path that is missing, or that is not a directory, is an empty list rather than an error:
    /// the store creates directories lazily, and a stray file where a fanout directory would be is
    /// not this operation's business to complain about.
    ///
    /// # Errors
    ///
    /// Whatever the filesystem says, other than a missing or non-directory path.
    fn list_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>>;
}

/// The standard-library filesystem: what the store runs on outside tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StdFs;

impl DurableFs for StdFs {
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        fs::create_dir_all(path)
    }

    fn stage(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        file.write_all(bytes)
    }

    fn append(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        let mut file = OpenOptions::new().append(true).create(true).open(path)?;
        file.write_all(bytes)
    }

    fn sync_file(&self, path: &Path) -> io::Result<()> {
        // Opened for writing rather than reading: `fsync` on a read-only descriptor is accepted by
        // the kernels this runs on, but the portable spelling is the writable one.
        OpenOptions::new().write(true).open(path)?.sync_all()
    }

    #[cfg(unix)]
    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        // A directory can only be opened read-only, and `fsync` on that descriptor is what commits
        // the directory entry a `rename` created.
        fs::File::open(path)?.sync_all()
    }

    #[cfg(not(unix))]
    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        // Not a workaround, and deliberately not a silent success. Plan §6.3 promotes a chunk by
        // renaming it into place; if the directory entry that rename creates cannot be forced to
        // durable storage, a crash can leave a chunk that is present but whose name is not, and
        // the store's whole guarantee is gone. The task's failure-and-recovery clause says to
        // document such a platform as unsupported rather than work around it, so this refuses.
        let _ = path;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "this platform cannot sync a directory, so chunk promotion cannot be made durable; \
             see crates/mesh-cas/DURABILITY.md",
        ))
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        fs::read(path)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn file_len(&self, path: &Path) -> io::Result<u64> {
        fs::metadata(path).map(|metadata| metadata.len())
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }

    fn list_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        if !path.is_dir() {
            return Ok(Vec::new());
        }
        match fs::read_dir(path) {
            Ok(entries) => entries.map(|entry| entry.map(|e| e.path())).collect(),
            // Still handled despite the `is_dir` check above: the two are separate syscalls and
            // another process may remove the directory between them.
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error),
        }
    }
}
