//! The Linux error-number table, held against the contract that refuses to carry one.
//!
//! `mesh_materializer::AdapterError` carries no errno and its header says why: *"`ENOTEMPTY` is 39
//! on Linux and 66 on Darwin. One integer in a portable crate is wrong on one of the two platforms
//! Mesh ships, invisibly, and a Linux-only run would never see it. Each backend projects its own."*
//! This file is the evidence that this backend projected one, that it is total over the contract's
//! twelve refusals, and that it is Linux's rather than this machine's.
//!
//! Verification is **warm** (`docs/adr/0004`).

use mesh_fuse::errno::{
    errno_of, EEXIST, EINVAL, EIO, EISDIR, ENOENT, ENOSYS, ENOTDIR, ENOTEMPTY, EROFS, ESTALE, EXDEV,
};
use mesh_materializer::{AdapterCapability, AdapterError, NameError};

/// Every refusal the contract publishes has a number, and it is the number Linux uses.
///
/// The list is written out rather than derived, because deriving it from the same `match` the
/// implementation uses would be a test that agrees with itself. A thirteenth `AdapterError`
/// variant fails to compile at `errno_of`, and a wrong number fails here.
#[test]
fn every_refusal_projects_the_number_linux_uses() {
    let table: [(AdapterError, _, &str); 12] = [
        (
            AdapterError::unsupported(AdapterCapability::Symlink),
            ENOSYS,
            "an operation the filesystem does not implement",
        ),
        (AdapterError::NotFound, ENOENT, "no such file or directory"),
        (AdapterError::AlreadyExists, EEXIST, "file exists"),
        (AdapterError::NotADirectory, ENOTDIR, "not a directory"),
        (AdapterError::IsADirectory, EISDIR, "is a directory"),
        (
            AdapterError::DirectoryNotEmpty,
            ENOTEMPTY,
            "directory not empty",
        ),
        (
            AdapterError::NameRejected(NameError::Relative),
            EINVAL,
            "a bad argument, not a missing file",
        ),
        (
            AdapterError::OutsideWorkspace,
            EXDEV,
            "the shape of a rename across a mount point",
        ),
        (AdapterError::ReadOnly, EROFS, "read-only file system"),
        (
            AdapterError::UnknownView,
            ESTALE,
            "the handle refers to something that is gone",
        ),
        (
            AdapterError::WouldCycle,
            EINVAL,
            "Linux's own rename(2) answer for a directory moved into itself",
        ),
        (
            AdapterError::Backend("a platform failure".to_owned()),
            EIO,
            "an input/output error",
        ),
    ];

    for (error, expected, why) in table {
        assert_eq!(
            errno_of(&error),
            expected,
            "{} should be {expected} — {why}",
            error.name()
        );
    }
}

/// The exact divergence the trait refuses to encode, pinned as a number.
///
/// `ENOTEMPTY` is 39 on Linux and 66 on Darwin. This crate is the Linux adapter, so it is 39 —
/// **on every machine this test runs on**, including the macOS one this run used. A backend that
/// had reached for the host's `errno.h` instead of writing the number down would answer 66 here
/// and 39 in CI, and no test could tell.
#[test]
fn enotempty_is_linuxs_thirty_nine_and_not_the_host_platforms_number() {
    assert_eq!(ENOTEMPTY.number(), 39);
    assert_eq!(ENOTEMPTY.name(), "ENOTEMPTY");
    assert_ne!(
        ENOTEMPTY.number(),
        66,
        "this is Darwin's number, not Linux's"
    );
    assert_eq!(format!("{ENOTEMPTY}"), "ENOTEMPTY (39)");
}

/// No two refusals that mean different things share a number by accident.
///
/// Two do share one on purpose — `NameRejected` and `WouldCycle` are both `EINVAL`, which is what
/// Linux itself does — and stating the count is how that stays deliberate rather than becoming the
/// place a third collision hides.
#[test]
fn the_table_collides_exactly_twice_and_on_purpose() {
    let numbers = [
        ENOSYS, ENOENT, EEXIST, ENOTDIR, EISDIR, ENOTEMPTY, EINVAL, EXDEV, EROFS, ESTALE, EIO,
    ];
    let mut unique: Vec<i32> = numbers.iter().map(|errno| errno.number()).collect();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        unique.len(),
        numbers.len(),
        "two named constants share a number: {numbers:?}"
    );
    assert_eq!(
        errno_of(&AdapterError::NameRejected(NameError::Nul)),
        errno_of(&AdapterError::WouldCycle),
        "the one deliberate collision stopped being one"
    );
}
