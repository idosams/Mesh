//! Keep held Linux directory descriptors in SQLite filenames, including WAL sidecars.
//! SQLite's unix VFS normally resolves every symlink in xFullPathname. Only the exact kernel
//! /proc/self/fd/<fd>/<leaf> spelling is exempted here; the caller retains that directory fd.
//! All file, locking, sync and sidecar operations remain the bundled unix implementation.
#![allow(unsafe_code)]

use rusqlite::ffi;
use std::ffi::{c_char, c_int, CStr};
use std::path::PathBuf;
use std::sync::OnceLock;

pub(super) const NAME: &str = "mesh-held-directory";

pub(super) fn register() -> rusqlite::Result<()> {
    static REGISTERED: OnceLock<c_int> = OnceLock::new();
    let code = *REGISTERED.get_or_init(|| {
        // SAFETY: SQLite initializes its built-in VFS before returning the static unix pointer.
        // Copy its complete ABI record so pAppData, locking and file allocation remain native.
        // The registered copy and its static name live for the process lifetime. OnceLock
        // serializes publication; the copy is never freed or unregistered while connections exist.
        unsafe {
            let base = ffi::sqlite3_vfs_find(c"unix".as_ptr());
            if base.is_null() {
                return ffi::SQLITE_NOTFOUND;
            }
            let mut wrapper = Box::new(*base);
            wrapper.pNext = std::ptr::null_mut();
            wrapper.zName = c"mesh-held-directory".as_ptr();
            wrapper.xFullPathname = Some(full_pathname);
            let wrapper = Box::into_raw(wrapper);
            let result = ffi::sqlite3_vfs_register(wrapper, 0);
            if result != ffi::SQLITE_OK {
                drop(Box::from_raw(wrapper));
            }
            result
        }
    });
    if code == ffi::SQLITE_OK {
        Ok(())
    } else {
        Err(rusqlite::Error::SqliteFailure(ffi::Error::new(code), None))
    }
}

fn descriptor_filename(name: &str) -> Option<PathBuf> {
    let (descriptor, leaf) = name.strip_prefix("/proc/self/fd/")?.split_once('/')?;
    let number = descriptor.parse::<i32>().ok()?;
    if number < 0 || number.to_string() != descriptor {
        return None;
    }
    let leaf = leaf.strip_prefix("./").unwrap_or(leaf);
    if leaf.is_empty() || leaf == "." || leaf == ".." || leaf.contains('/') {
        return None;
    }
    let parent = PathBuf::from(format!("/proc/self/fd/{number}"));
    if !std::fs::metadata(&parent).ok()?.is_dir() {
        return None;
    }
    let path = parent.join(leaf);
    // The kernel directory reference is the only permitted link. Never bypass NOFOLLOW for
    // a caller-controlled database leaf. Unix xOpen also uses O_NOFOLLOW after this inspection.
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) => {
            use std::os::unix::fs::MetadataExt as _;
            if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.nlink() != 1 {
                return None;
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return None,
    }
    Some(path)
}

unsafe extern "C" fn full_pathname(
    _vfs: *mut ffi::sqlite3_vfs,
    input: *const c_char,
    capacity: c_int,
    output: *mut c_char,
) -> c_int {
    if input.is_null() || output.is_null() || capacity <= 0 {
        return ffi::SQLITE_CANTOPEN;
    }
    // SAFETY: SQLite provides a terminated input and a writable buffer of capacity bytes.
    let input = unsafe { CStr::from_ptr(input) };
    if input.to_bytes().starts_with(b"/proc/self/fd/") {
        let Some(path) = input.to_str().ok().and_then(descriptor_filename) else {
            return ffi::SQLITE_CANTOPEN;
        };
        use std::os::unix::ffi::OsStrExt as _;
        let bytes = path.as_os_str().as_bytes();
        if bytes.len() >= capacity as usize {
            return ffi::SQLITE_CANTOPEN;
        }
        // SAFETY: input-derived bytes cannot contain NUL, and the capacity check reserves its
        // terminator. The allocation is independent of SQLite's destination buffer.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr().cast(), output, bytes.len());
            *output.add(bytes.len()) = 0;
        }
        return ffi::SQLITE_OK;
    }
    // SAFETY: the built-in unix VFS has process lifetime. Its callback receives its own record
    // and the original SQLite-provided buffers, preserving ordinary symlink refusal semantics.
    unsafe {
        let base = ffi::sqlite3_vfs_find(c"unix".as_ptr());
        if base.is_null() {
            return ffi::SQLITE_CANTOPEN;
        }
        match (*base).xFullPathname {
            Some(callback) => callback(base, input.as_ptr(), capacity, output),
            None => ffi::SQLITE_CANTOPEN,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{Connection, OpenFlags};

    #[test]
    fn delegates_ordinary_paths_and_keeps_nofollow() {
        register().unwrap();
        let root = std::env::temp_dir().join(format!("mesh-native-vfs-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let path = root.join("index.sqlite");
        let flags = OpenFlags::default() | OpenFlags::SQLITE_OPEN_NOFOLLOW;
        let connection = Connection::open_with_flags_and_vfs(&path, flags, NAME).unwrap();
        connection
            .execute_batch("CREATE TABLE retained(value INTEGER)")
            .unwrap();
        drop(connection);
        let link = root.join("link.sqlite");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(Connection::open_with_flags_and_vfs(&link, flags, NAME).is_err());
        for invalid in [
            "/proc/self/fd/01/a",
            "/proc/self/fd/-1/a",
            "/proc/self/fd/0/../a",
            "/proc/self/fd/0/a/b",
        ] {
            assert!(descriptor_filename(invalid).is_none());
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
