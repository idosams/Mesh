//! Destination-based new-entry permissions, without creating a probe in the user's folder.
use super::metadata_digest;
use std::fs::{File, Permissions};
use std::io;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

/// Return the exact parent policy used to configure a fresh privately staged file. Re-reading
/// this policy at every apply boundary prevents a changed ACL/group from inheriting stale consent.
pub(crate) fn inherit_new_entry(
    parent: &File,
    staged: &File,
    mode: u32,
) -> io::Result<(String, u32)> {
    let before = parent.metadata()?;
    let entry = staged.metadata()?;
    let directory = entry.is_dir();
    if !before.is_dir()
        || (!directory && !entry.is_file())
        || mode & !(if directory { 0o777 } else { 0o755 }) != 0
    {
        return Err(io::Error::other("invalid new-entry permission input"));
    }
    let policy = metadata_digest(parent)?;
    configure(parent, staged, directory)?;
    // Linux propagates the parent setgid bit onto newly created directories. Preserve that
    // destination behavior even though allocation took place under a different private parent.
    #[cfg(target_os = "linux")]
    let mode = mode | if directory { before.mode() & 0o2000 } else { 0 };
    staged.set_permissions(Permissions::from_mode(mode))?;
    if metadata_digest(parent)? != policy || parent.metadata()?.mode() != before.mode() {
        return Err(io::Error::other(
            "parent permissions changed during preparation",
        ));
    }
    Ok((policy, before.mode()))
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn configure(parent: &File, staged: &File, directory: bool) -> io::Result<()> {
    use std::ffi::c_void;
    use std::os::fd::AsRawFd as _;
    type Acl = *mut c_void;
    const EXTENDED: i32 = 0x100;
    const INHERITED: u32 = 1 << 4;
    const FILE_INHERIT: u32 = 1 << 5;
    const DIR_INHERIT: u32 = 1 << 6;
    const LIMIT_INHERIT: u32 = 1 << 7;
    const ONLY_INHERIT: u32 = 1 << 8;
    unsafe extern "C" {
        fn acl_get_fd_np(fd: i32, kind: i32) -> Acl;
        fn acl_init(count: i32) -> Acl;
        fn acl_free(value: Acl) -> i32;
        fn acl_get_entry(acl: Acl, entry: i32, value: *mut Acl) -> i32;
        fn acl_create_entry(acl: *mut Acl, value: *mut Acl) -> i32;
        fn acl_copy_entry(destination: Acl, source: Acl) -> i32;
        fn acl_get_flagset_np(value: Acl, flags: *mut Acl) -> i32;
        fn acl_get_flag_np(flags: Acl, flag: u32) -> i32;
        fn acl_delete_flag_np(flags: Acl, flag: u32) -> i32;
        fn acl_add_flag_np(flags: Acl, flag: u32) -> i32;
        fn acl_set_fd_np(fd: i32, acl: Acl, kind: i32) -> i32;
        fn fchown(fd: i32, owner: u32, group: u32) -> i32;
    }
    struct OwnedAcl(Acl);
    impl Drop for OwnedAcl {
        fn drop(&mut self) {
            // SAFETY: this allocation came from acl_init/acl_get_fd_np and has one owner.
            unsafe {
                acl_free(self.0);
            }
        }
    }
    fn check(result: i32) -> io::Result<()> {
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
    // SAFETY: live borrowed file descriptors, valid native group id, and -1 sentinel preserves
    // the newly allocated file's owner. Failure refuses; no privilege escalation is attempted.
    if staged.metadata()?.gid() != parent.metadata()?.gid() {
        check(unsafe { fchown(staged.as_raw_fd(), u32::MAX, parent.metadata()?.gid()) })?;
    }
    // SAFETY: both handles are checked before use and freed once by OwnedAcl. The source ACL is
    // never modified; the product is a new empty ACL, discarding any staging-directory inheritance.
    let source = unsafe { acl_get_fd_np(parent.as_raw_fd(), EXTENDED) };
    let source = if source.is_null() {
        if io::Error::last_os_error().kind() != io::ErrorKind::NotFound {
            return Err(io::Error::last_os_error());
        }
        None
    } else {
        Some(OwnedAcl(source))
    };
    let result = unsafe { acl_init(0) };
    if result.is_null() {
        return Err(io::Error::last_os_error());
    }
    let mut result = OwnedAcl(result);
    if let Some(source) = source {
        for index in 0..=128 {
            let mut entry = std::ptr::null_mut();
            // SAFETY: source is a valid owned ACL. Darwin returns EINVAL at the end, unlike the
            // POSIX iterator convention. Every other failure refuses rather than truncating policy.
            let status =
                unsafe { acl_get_entry(source.0, if index == 0 { 0 } else { -1 }, &mut entry) };
            if status != 0 {
                if io::Error::last_os_error().raw_os_error() == Some(22) {
                    break;
                }
                return Err(io::Error::last_os_error());
            }
            if index == 128 || entry.is_null() {
                return Err(io::Error::other("ACL exceeds native entry bound"));
            }
            let mut flags = std::ptr::null_mut();
            // SAFETY: entries/flagsets borrow their owning ACL and remain valid until it is freed.
            check(unsafe { acl_get_flagset_np(entry, &mut flags) })?;
            if flags.is_null() {
                return Err(io::Error::other("missing ACL flags"));
            }
            match unsafe {
                acl_get_flag_np(flags, if directory { DIR_INHERIT } else { FILE_INHERIT })
            } {
                0 => continue,
                1 => {}
                _ => return Err(io::Error::last_os_error()),
            }
            let mut inherited = std::ptr::null_mut();
            // SAFETY: result is a valid mutable ACL handle; the API may update that owned handle.
            // Copy preserves principal, allow/deny, rights and order. Only inheritance controls change.
            check(unsafe { acl_create_entry(&mut result.0, &mut inherited) })?;
            if inherited.is_null() {
                return Err(io::Error::other("missing new ACL entry"));
            }
            check(unsafe { acl_copy_entry(inherited, entry) })?;
            check(unsafe { acl_get_flagset_np(inherited, &mut flags) })?;
            if flags.is_null() {
                return Err(io::Error::other("missing inherited ACL flags"));
            }
            let limited = match unsafe { acl_get_flag_np(flags, LIMIT_INHERIT) } {
                0 => false,
                1 => true,
                _ => return Err(io::Error::last_os_error()),
            };
            // A directory carries inheritable rules forward unless propagation was limited.
            // The inherited rule applies to the new entry itself, so only-inherit is cleared.
            check(unsafe { acl_delete_flag_np(flags, ONLY_INHERIT) })?;
            if !directory || limited {
                for flag in [FILE_INHERIT, DIR_INHERIT, LIMIT_INHERIT] {
                    check(unsafe { acl_delete_flag_np(flags, flag) })?;
                }
            }
            check(unsafe { acl_add_flag_np(flags, INHERITED) })?;
        }
    }
    // SAFETY: valid staged descriptor and owned ACL. An empty product also clears staging ACLs.
    check(unsafe { acl_set_fd_np(staged.as_raw_fd(), result.0, EXTENDED) })
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn configure(parent: &File, staged: &File, _directory: bool) -> io::Result<()> {
    use std::os::fd::AsRawFd as _;
    unsafe extern "C" {
        fn getegid() -> u32;
        fn fchown(fd: i32, owner: u32, group: u32) -> i32;
    }
    // metadata_digest rejects Linux attributes/default ACLs until their inheritance is supported.
    metadata_digest(staged)?;
    let metadata = parent.metadata()?;
    // SAFETY: getegid has no pointer arguments; fchown borrows a live file descriptor.
    let group = if metadata.mode() & 0o2000 != 0 {
        metadata.gid()
    } else {
        unsafe { getegid() }
    };
    if staged.metadata()?.gid() != group
        && unsafe { fchown(staged.as_raw_fd(), u32::MAX, group) } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn configure(_parent: &File, _staged: &File, _directory: bool) -> io::Result<()> {
    Err(io::Error::other(
        "new-entry permission inheritance is unavailable",
    ))
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use crate::managed_file::retained_replacement::tests::Fixture;
    use std::fs::{self, OpenOptions};
    use std::process::Command;

    fn acl(path: &std::path::Path, rule: &str) {
        let result = Command::new("/bin/chmod")
            .arg("+a")
            .arg(rule)
            .arg(path)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "ACL fixture setup failed: {:?}",
            result.status
        );
    }

    #[test]
    fn destination_acl_inheritance_matches_files_created_by_the_kernel() {
        let policies: &[&[&str]] = &[
            &[],
            &["everyone allow read,file_inherit"],
            &["everyone deny write,file_inherit,only_inherit,limit_inherit"],
            &["everyone allow append,directory_inherit"],
            &[
                "everyone deny write,file_inherit,only_inherit",
                "everyone allow readattr,file_inherit,directory_inherit,only_inherit",
                "everyone allow append,directory_inherit",
            ],
        ];
        for rules in policies {
            for executable in [false, true] {
                let f = Fixture::new();
                for rule in *rules {
                    acl(&f.source, rule);
                }
                acl(
                    &f.recovery,
                    "everyone allow execute,file_inherit,only_inherit",
                );
                let parent = File::open(&f.source).unwrap();
                let reference = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create_new(true)
                    .open(f.source.join("reference"))
                    .unwrap();
                let staged = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create_new(true)
                    .open(f.recovery.join("stage"))
                    .unwrap();
                let mode = if executable { 0o755 } else { 0o644 };
                reference
                    .set_permissions(Permissions::from_mode(mode))
                    .unwrap();
                let parent_before = metadata_digest(&parent).unwrap();
                let source_entries = fs::read_dir(&f.source).unwrap().count();
                let (policy, _) = inherit_new_entry(&parent, &staged, mode).unwrap();
                assert_eq!(policy, parent_before);
                assert_eq!(metadata_digest(&parent).unwrap(), parent_before);
                assert_eq!(fs::read_dir(&f.source).unwrap().count(), source_entries);
                assert_eq!(
                    staged.metadata().unwrap().mode(),
                    reference.metadata().unwrap().mode()
                );
                assert_eq!(
                    metadata_digest(&staged).unwrap(),
                    metadata_digest(&reference).unwrap(),
                    "native policy {rules:?}"
                );
            }
        }
    }

    #[test]
    fn directory_inheritance_and_descendant_propagation_match_kernel_creation() {
        use crate::root_authority::PinnedWorkspaceRoot;
        use std::ffi::OsStr;
        let policies: &[&[&str]] = &[
            &[],
            &["everyone allow readattr,file_inherit"],
            &["everyone allow readattr,directory_inherit"],
            &["everyone allow readattr,file_inherit,directory_inherit,only_inherit"],
            &["everyone allow readattr,file_inherit,directory_inherit,only_inherit,limit_inherit"],
            &[
                "everyone allow readattr,file_inherit,directory_inherit",
                "everyone allow readextattr,directory_inherit,limit_inherit",
                "everyone allow readsecurity,file_inherit,only_inherit",
            ],
        ];
        for rules in policies {
            let f = Fixture::new();
            for rule in *rules {
                acl(&f.source, rule);
            }
            // An unrelated staging ACL must not leak into the future destination tree.
            acl(
                &f.recovery,
                "everyone allow writeextattr,directory_inherit,file_inherit",
            );
            fs::create_dir(f.source.join("reference")).unwrap();
            let reference = File::open(f.source.join("reference")).unwrap();
            let parent = File::open(&f.source).unwrap();
            let staging = PinnedWorkspaceRoot::open(f.recovery.clone())
                .unwrap()
                .create_child_directory_with_mode(OsStr::new("stage"), 0o777)
                .unwrap();
            let staged = staging.try_clone_directory().unwrap();
            let mode = staged.metadata().unwrap().mode() & 0o777;
            let parent_policy = metadata_digest(&parent).unwrap();
            let entries = fs::read_dir(&f.source).unwrap().count();
            inherit_new_entry(&parent, &staged, mode).unwrap();
            assert_eq!(metadata_digest(&parent).unwrap(), parent_policy);
            assert_eq!(fs::read_dir(&f.source).unwrap().count(), entries);
            assert_eq!(
                staged.metadata().unwrap().mode(),
                reference.metadata().unwrap().mode()
            );
            assert_eq!(
                metadata_digest(&staged).unwrap(),
                metadata_digest(&reference).unwrap(),
                "{rules:?}"
            );
            // Let the kernel create children below both roots. This distinguishes propagated
            // directory ACLs from file-only inheritance and limited one-generation rules.
            for name in ["child", "child/grandchild"] {
                fs::create_dir(f.source.join("reference").join(name)).unwrap();
                fs::create_dir(f.recovery.join("stage").join(name)).unwrap();
                let expected = File::open(f.source.join("reference").join(name)).unwrap();
                let actual = File::open(f.recovery.join("stage").join(name)).unwrap();
                assert_eq!(
                    metadata_digest(&actual).unwrap(),
                    metadata_digest(&expected).unwrap(),
                    "{rules:?}: {name}"
                );
            }
            for name in ["file", "child/file", "child/grandchild/file"] {
                let create = |path| {
                    OpenOptions::new()
                        .read(true)
                        .write(true)
                        .create_new(true)
                        .open(path)
                        .unwrap()
                };
                let expected = create(f.source.join("reference").join(name));
                let actual = create(f.recovery.join("stage").join(name));
                assert_eq!(
                    metadata_digest(&actual).unwrap(),
                    metadata_digest(&expected).unwrap(),
                    "{rules:?}: {name}"
                );
            }
        }
    }

    #[test]
    fn directory_group_and_deny_entry_match_native_creation() {
        use crate::root_authority::PinnedWorkspaceRoot;
        use std::ffi::OsStr;
        let f = Fixture::new();
        let groups = Command::new("/usr/bin/id").arg("-G").output().unwrap();
        assert!(groups.status.success());
        let original = fs::metadata(&f.source).unwrap().gid();
        if let Some(group) = String::from_utf8(groups.stdout)
            .unwrap()
            .split_whitespace()
            .filter_map(|value| value.parse::<u32>().ok())
            .find(|group| *group != original)
        {
            assert!(Command::new("/usr/bin/chgrp")
                .arg(group.to_string())
                .arg(&f.source)
                .status()
                .unwrap()
                .success());
        }
        acl(
            &f.source,
            "everyone deny writeextattr,directory_inherit,only_inherit,limit_inherit",
        );
        fs::create_dir(f.source.join("reference")).unwrap();
        let reference = File::open(f.source.join("reference")).unwrap();
        let parent = File::open(&f.source).unwrap();
        let stage = PinnedWorkspaceRoot::open(f.recovery.clone())
            .unwrap()
            .create_child_directory_with_mode(OsStr::new("stage"), 0o777)
            .unwrap();
        let staged = stage.try_clone_directory().unwrap();
        let mode = staged.metadata().unwrap().mode() & 0o777;
        inherit_new_entry(&parent, &staged, mode).unwrap();
        assert_eq!(
            staged.metadata().unwrap().gid(),
            parent.metadata().unwrap().gid()
        );
        assert_eq!(
            metadata_digest(&staged).unwrap(),
            metadata_digest(&reference).unwrap()
        );
    }

    #[test]
    fn new_file_group_matches_native_destination_group() {
        let f = Fixture::new();
        let groups = Command::new("/usr/bin/id").arg("-G").output().unwrap();
        assert!(groups.status.success());
        let original = fs::metadata(&f.source).unwrap().gid();
        if let Some(group) = String::from_utf8(groups.stdout)
            .unwrap()
            .split_whitespace()
            .filter_map(|group| group.parse::<u32>().ok())
            .find(|group| *group != original)
        {
            assert!(Command::new("/usr/bin/chgrp")
                .arg(group.to_string())
                .arg(&f.source)
                .status()
                .unwrap()
                .success());
        }
        let parent = File::open(&f.source).unwrap();
        let reference = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(f.source.join("reference"))
            .unwrap();
        let staged = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(f.recovery.join("stage"))
            .unwrap();
        inherit_new_entry(&parent, &staged, 0o644).unwrap();
        assert_eq!(
            staged.metadata().unwrap().gid(),
            reference.metadata().unwrap().gid()
        );
        assert_eq!(
            staged.metadata().unwrap().gid(),
            parent.metadata().unwrap().gid()
        );
    }
}

#[cfg(all(test, target_os = "linux"))]
mod linux_directory_tests {
    use super::*;
    use crate::managed_file::retained_replacement::tests::Fixture;
    use std::fs;
    use std::os::unix::fs::DirBuilderExt as _;

    #[test]
    fn directory_group_and_setgid_match_kernel_creation_without_changing_parent() {
        for parent_mode in [0o750, 0o2750] {
            let f = Fixture::new();
            fs::set_permissions(&f.source, Permissions::from_mode(parent_mode)).unwrap();
            fs::DirBuilder::new()
                .mode(0o777)
                .create(f.source.join("reference"))
                .unwrap();
            fs::DirBuilder::new()
                .mode(0o777)
                .create(f.recovery.join("stage"))
                .unwrap();
            let parent = File::open(&f.source).unwrap();
            let reference = File::open(f.source.join("reference")).unwrap();
            let staged = File::open(f.recovery.join("stage")).unwrap();
            let mode = staged.metadata().unwrap().mode() & 0o777;
            let policy = metadata_digest(&parent).unwrap();
            let entries = fs::read_dir(&f.source).unwrap().count();
            inherit_new_entry(&parent, &staged, mode).unwrap();
            assert_eq!(
                staged.metadata().unwrap().mode(),
                reference.metadata().unwrap().mode()
            );
            assert_eq!(
                staged.metadata().unwrap().gid(),
                reference.metadata().unwrap().gid()
            );
            assert_eq!(metadata_digest(&parent).unwrap(), policy);
            assert_eq!(fs::read_dir(&f.source).unwrap().count(), entries);
            let before = staged.metadata().unwrap().mode();
            assert!(inherit_new_entry(&parent, &staged, 0o4777).is_err());
            assert_eq!(staged.metadata().unwrap().mode(), before);
        }
    }
}
