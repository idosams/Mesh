//! Native metadata copy and bounded comparison. No paths or raw attributes leave this module.
use mesh_types::{Blake3, ContentDigest as _};
use std::fs::File;
use std::io;
use std::os::unix::fs::MetadataExt as _;
use std::os::unix::io::AsRawFd as _;

mod creation;
pub(super) use creation::inherit_new_entry;

const LIMIT: usize = 1024 * 1024;
fn problem() -> io::Error {
    io::Error::other("native metadata unavailable, changed or exceeds budget")
}
fn field(output: &mut Vec<u8>, bytes: &[u8]) -> io::Result<()> {
    if output.len().saturating_add(bytes.len()).saturating_add(8) > LIMIT {
        return Err(problem());
    }
    output.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    output.extend_from_slice(bytes);
    Ok(())
}

pub(super) fn metadata_digest(file: &File) -> io::Result<String> {
    let before = file.metadata()?;
    let mut bytes = Vec::new();
    field(&mut bytes, &before.uid().to_le_bytes())?;
    field(&mut bytes, &before.gid().to_le_bytes())?;
    extra_metadata(file, &mut bytes)?;
    let after = file.metadata()?;
    if (
        before.dev(),
        before.ino(),
        before.ctime(),
        before.ctime_nsec(),
    ) != (after.dev(), after.ino(), after.ctime(), after.ctime_nsec())
    {
        return Err(problem());
    }
    Ok(Blake3::digest_bytes(&bytes).to_string())
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
pub(super) fn copy_metadata(from: &File, to: &File) -> io::Result<()> {
    use crate::managed_file::managed_file_identity;
    use std::os::macos::fs::MetadataExt as _;
    unsafe extern "C" {
        fn fcopyfile(from: i32, to: i32, state: *mut std::ffi::c_void, flags: u32) -> i32;
        fn fchown(file: i32, owner: u32, group: u32) -> i32;
        fn fchflags(file: i32, flags: u32) -> i32;
    }
    let source = from.metadata()?;
    let destination = to.metadata()?;
    let allocation = managed_file_identity(to, &destination)?;
    if (source.uid(), source.gid()) != (destination.uid(), destination.gid()) {
        // SAFETY: live borrowed descriptor and uid_t/gid_t values from native metadata. Failure
        // refuses preparation; ownership is never silently changed to the current process owner.
        if unsafe { fchown(to.as_raw_fd(), source.uid(), source.gid()) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    // SAFETY: live borrowed descriptors, null default copy state. COPYFILE_ACL | COPYFILE_XATTR
    // deliberately excludes COPYFILE_STAT, which transplants the source's creation timestamp
    // and would weaken the allocation discriminator used by native recovery receipts.
    if unsafe { fcopyfile(from.as_raw_fd(), to.as_raw_fd(), std::ptr::null_mut(), 5) } < 0 {
        return Err(io::Error::last_os_error());
    }
    if source.st_flags() != to.metadata()?.st_flags() {
        // SAFETY: live owned destination descriptor, flags read from the selected native source.
        if unsafe { fchflags(to.as_raw_fd(), source.st_flags()) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    if managed_file_identity(to, &to.metadata()?)? != allocation {
        return Err(io::Error::other(
            "metadata copy changed destination allocation identity",
        ));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn extra_metadata(file: &File, output: &mut Vec<u8>) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::macos::fs::MetadataExt as _;
    unsafe extern "C" {
        fn flistxattr(fd: i32, names: *mut std::ffi::c_char, size: usize, options: i32) -> isize;
        fn fgetxattr(
            fd: i32,
            name: *const std::ffi::c_char,
            value: *mut std::ffi::c_void,
            size: usize,
            position: u32,
            options: i32,
        ) -> isize;
        fn acl_get_fd_np(fd: i32, kind: i32) -> *mut std::ffi::c_void;
        fn acl_to_text(acl: *mut std::ffi::c_void, length: *mut isize) -> *mut std::ffi::c_char;
        fn acl_free(acl: *mut std::ffi::c_void) -> i32;
    }
    field(output, &file.metadata()?.st_flags().to_le_bytes())?;
    // SAFETY: size-only query has null buffer; subsequent buffers have exactly their declared
    // lengths. SHOWCOMPRESSION includes metadata that the default xattr listing can hide.
    let count = unsafe { flistxattr(file.as_raw_fd(), std::ptr::null_mut(), 0, 0x0020) };
    if count < 0 || count as usize > LIMIT {
        return Err(problem());
    }
    let mut names = vec![0u8; count as usize];
    let read = unsafe {
        flistxattr(
            file.as_raw_fd(),
            names.as_mut_ptr().cast(),
            names.len(),
            0x0020,
        )
    };
    if read != count {
        return Err(problem());
    }
    let mut names: Vec<_> = names
        .split(|b| *b == 0)
        .filter(|name| !name.is_empty())
        .collect();
    if names.len() > 256 {
        return Err(problem());
    }
    names.sort_unstable();
    for name in names {
        field(output, name)?;
        let name = CString::new(name).map_err(|_| problem())?;
        let size = unsafe {
            fgetxattr(
                file.as_raw_fd(),
                name.as_ptr(),
                std::ptr::null_mut(),
                0,
                0,
                0x0020,
            )
        };
        if size < 0 || size as usize > LIMIT.saturating_sub(output.len()) {
            return Err(problem());
        }
        let mut value = vec![0u8; size as usize];
        let read = unsafe {
            fgetxattr(
                file.as_raw_fd(),
                name.as_ptr(),
                value.as_mut_ptr().cast(),
                value.len(),
                0,
                0x0020,
            )
        };
        if read != size {
            return Err(problem());
        }
        field(output, &value)?;
    }
    // SAFETY: returned owned allocations are checked for null and freed exactly once. ENOENT is
    // the native representation of no extended ACL; every other retrieval failure refuses.
    let acl = unsafe { acl_get_fd_np(file.as_raw_fd(), 0x00000100) };
    if acl.is_null() {
        if io::Error::last_os_error().kind() != io::ErrorKind::NotFound {
            return Err(problem());
        }
        field(output, b"")?;
    } else {
        let mut size = 0isize;
        let text = unsafe { acl_to_text(acl, &mut size) };
        let result = if text.is_null() || size < 0 || size as usize > LIMIT {
            Err(problem())
        } else {
            // SAFETY: acl_to_text returned a live allocation and its explicit nonnegative length.
            field(output, unsafe {
                std::slice::from_raw_parts(text.cast::<u8>(), size as usize)
            })
        };
        if !text.is_null() {
            unsafe {
                acl_free(text.cast());
            }
        }
        unsafe {
            acl_free(acl);
        }
        result?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn extra_metadata(file: &File, _output: &mut Vec<u8>) -> io::Result<()> {
    unsafe extern "C" {
        fn flistxattr(fd: i32, names: *mut std::ffi::c_char, size: usize) -> isize;
    }
    // SAFETY: live descriptor and null/zero size-only query. Linux ACLs are extended attributes.
    if unsafe { flistxattr(file.as_raw_fd(), std::ptr::null_mut(), 0) } != 0 {
        return Err(problem());
    }
    Ok(())
}
#[cfg(target_os = "linux")]
pub(super) fn copy_metadata(from: &File, to: &File) -> io::Result<()> {
    if metadata_digest(from)? != metadata_digest(to)? {
        return Err(problem());
    }
    Ok(())
}
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn extra_metadata(_file: &File, _output: &mut Vec<u8>) -> io::Result<()> {
    Err(problem())
}
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(super) fn copy_metadata(_from: &File, _to: &File) -> io::Result<()> {
    Err(problem())
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use crate::managed_file::{managed_file_identity, TEMPORARY_COUNTER};
    use std::sync::atomic::Ordering;

    #[test]
    fn metadata_copy_preserves_the_new_files_allocation_identity() {
        let root = std::env::temp_dir().join(format!(
            "mesh-metadata-allocation-{}-{}",
            std::process::id(),
            TEMPORARY_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let open = |name| {
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(root.join(name))
                .unwrap()
        };
        let source = open("source");
        std::thread::sleep(std::time::Duration::from_millis(25));
        let staged = open("staged");
        let source_id = managed_file_identity(&source, &source.metadata().unwrap()).unwrap();
        let before = managed_file_identity(&staged, &staged.metadata().unwrap()).unwrap();
        assert_ne!(
            source_id.incarnation, before.incarnation,
            "fixture needs distinct native allocation timestamps"
        );
        copy_metadata(&source, &staged).unwrap();
        let after = managed_file_identity(&staged, &staged.metadata().unwrap()).unwrap();
        let _ = std::fs::remove_dir_all(root);
        assert_eq!(
            after, before,
            "metadata preservation must not transplant the old allocation timestamp"
        );
    }
}
