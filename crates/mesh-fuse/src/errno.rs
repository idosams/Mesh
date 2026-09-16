//! The Linux errno projection this backend owns.
//!
//! # Why the table is here and not on the trait
//!
//! [`mesh_materializer::AdapterError`] carries no errno on purpose, and its own header says why:
//! `ENOTEMPTY` is **39 on Linux and 66 on Darwin**, so one integer in a portable crate is wrong on
//! one of the two platforms Mesh ships, invisibly, and a Linux-only run would never see it.
//! `tests/compatibility/adapter/v0/README.md` §5 carries the recommended table as guidance each
//! backend owns. This file is this backend's discharge of that obligation.
//!
//! Every number below is **Linux's**, written as a literal. There is no `libc` here — see this
//! crate's manifest for why the dependency fence keeps it out — which turns out to be the point
//! rather than a workaround: a constant a human can read against `errno.h` is auditable, and a
//! `libc::ENOTEMPTY` that resolved to 66 on the developer's Mac and 39 on the runner would be the
//! exact divergence the trait refused to encode.
//!
//! # `tests/errno.rs` holds this against the trait
//!
//! It matches exhaustively over every [`AdapterError`] variant, so a thirteenth variant is a
//! compile error here rather than a silent `EIO`.

use mesh_materializer::AdapterError;

/// One Linux error number, and the name a person reads it by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Errno {
    number: i32,
    name: &'static str,
}

impl Errno {
    const fn new(number: i32, name: &'static str) -> Self {
        Self { number, name }
    }

    /// The number the kernel is handed, negated as FUSE requires it in a reply header.
    #[must_use]
    pub const fn number(self) -> i32 {
        self.number
    }

    /// The name in `errno.h`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        self.name
    }
}

impl core::fmt::Display for Errno {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{} ({})", self.name, self.number)
    }
}

/// No such file or directory.
pub const ENOENT: Errno = Errno::new(2, "ENOENT");
/// Input/output error.
pub const EIO: Errno = Errno::new(5, "EIO");
/// File exists.
pub const EEXIST: Errno = Errno::new(17, "EEXIST");
/// Invalid cross-device link — Linux's answer when a rename crosses a mount.
pub const EXDEV: Errno = Errno::new(18, "EXDEV");
/// Not a directory.
pub const ENOTDIR: Errno = Errno::new(20, "ENOTDIR");
/// Is a directory.
pub const EISDIR: Errno = Errno::new(21, "EISDIR");
/// Invalid argument.
pub const EINVAL: Errno = Errno::new(22, "EINVAL");
/// Read-only file system.
pub const EROFS: Errno = Errno::new(30, "EROFS");
/// Function not implemented.
pub const ENOSYS: Errno = Errno::new(38, "ENOSYS");
/// Directory not empty. **39 on Linux, 66 on Darwin** — the divergence the trait refuses to carry.
pub const ENOTEMPTY: Errno = Errno::new(39, "ENOTEMPTY");
/// Stale file handle.
pub const ESTALE: Errno = Errno::new(116, "ESTALE");

/// The errno a FUSE reply carries for one refusal.
///
/// Exhaustive over [`AdapterError`], so a variant added to the contract stops this build rather
/// than falling into a default arm and reaching a person as `EIO`.
#[must_use]
pub const fn errno_of(error: &AdapterError) -> Errno {
    match error {
        // The capability was never built. `ENOSYS` is what a kernel tells an application about an
        // operation the filesystem does not implement, and it is the one refusal a caller can act
        // on by not asking again.
        AdapterError::Unsupported { .. } => ENOSYS,
        AdapterError::NotFound => ENOENT,
        AdapterError::AlreadyExists => EEXIST,
        AdapterError::NotADirectory => ENOTDIR,
        AdapterError::IsADirectory => EISDIR,
        AdapterError::DirectoryNotEmpty => ENOTEMPTY,
        // A name the portable rules refuse is a bad argument, not a missing file. Answering
        // `ENOENT` here would tell an application the path is free when it is illegal.
        AdapterError::NameRejected(_) => EINVAL,
        // A path that resolves out of the workspace is the same shape as a rename across a mount
        // point, which is what `EXDEV` means and what every tool already knows how to handle:
        // fall back to copy-then-delete rather than reporting corruption.
        AdapterError::OutsideWorkspace => EXDEV,
        AdapterError::ReadOnly => EROFS,
        // The view was released, so every handle the kernel still holds on it is stale. `ESTALE`
        // is the answer that makes an application re-resolve instead of retrying forever.
        AdapterError::UnknownView => ESTALE,
        // Linux's own `rename(2)` answers `EINVAL` for "new directory is a subdirectory of old".
        AdapterError::WouldCycle => EINVAL,
        AdapterError::Backend(_) => EIO,
    }
}
