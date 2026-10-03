//! Finish a deterministic received-import marker without replacing or truncating retained bytes.
//! Callers retain original import ownership; this helper grants no restart or launch authority.
use super::*;
use std::io::{Seek as _, SeekFrom};
use std::os::unix::fs::MetadataExt as _;

#[cfg(test)]
thread_local! {
    static BEFORE_APPEND: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = std::cell::RefCell::new(None);
}

#[derive(PartialEq, Eq)]
struct Prefix {
    identity: (u64, u64),
    bytes: Vec<u8>,
}
fn changed() -> io::Error {
    io::Error::other("received import marker differs from its original initialization")
}
fn inspect_file(file: &mut File, expected: &[u8]) -> io::Result<Prefix> {
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.mode() & 0o7777 != 0o600
        || metadata.len() > expected.len() as u64
    {
        return Err(changed());
    }
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    file.take(expected.len() as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 != metadata.len() || !expected.starts_with(&bytes) {
        return Err(changed());
    }
    Ok(Prefix {
        identity: (metadata.dev(), metadata.ino()),
        bytes,
    })
}
fn inspect(root: &PinnedWorkspaceRoot, path: &Path, expected: &[u8]) -> io::Result<Prefix> {
    root.ensure_namespace_identity()?;
    let mut file = root.filesystem().read_only().read_file(path)?;
    let prefix = inspect_file(&mut file, expected)?;
    root.ensure_namespace_identity()?;
    Ok(prefix)
}

pub(super) fn finish(root: &PinnedWorkspaceRoot, path: &Path, expected: &[u8]) -> io::Result<()> {
    root.ensure_namespace_identity()?;
    let prefix = match inspect(root, path, expected) {
        Ok(prefix) => prefix,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            root.filesystem()
                .write_new_file(path, &[], fs::Permissions::from_mode(0o600))?;
            inspect(root, path, expected)?
        }
        Err(error) => return Err(error),
    };
    #[cfg(test)]
    BEFORE_APPEND.with(|hook| {
        if let Some(hook) = hook.borrow_mut().take() {
            hook();
        }
    });
    if inspect(root, path, expected)? != prefix {
        return Err(changed());
    }
    let mut file = root.filesystem().open_private_append_existing(path)?;
    // Re-read through the independently opened writable descriptor, not just the pathname.
    if inspect_file(&mut file, expected)? != prefix {
        return Err(changed());
    }
    root.ensure_namespace_identity()?;
    file.write_all(&expected[prefix.bytes.len()..])?;
    file.sync_all()?;
    let complete = inspect(root, path, expected)?;
    if complete.identity != prefix.identity || complete.bytes != expected {
        return Err(changed());
    }
    root.sync()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture {
        root: PathBuf,
        pin: PinnedWorkspaceRoot,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "mesh-received-record-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            let pin = PinnedWorkspaceRoot::open(root.clone()).unwrap();
            Self { root, pin }
        }
        fn path(&self) -> PathBuf {
            self.root.join("receipt")
        }
        fn write(&self, bytes: &[u8]) {
            fs::write(self.path(), bytes).unwrap();
            fs::set_permissions(self.path(), fs::Permissions::from_mode(0o600)).unwrap();
        }
        fn finish(&self, bytes: &[u8]) -> io::Result<()> {
            finish(&self.pin, Path::new("receipt"), bytes)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn missing_and_every_prefix_complete_in_place_and_replay() {
        let f = Fixture::new();
        let expected = b"mesh received receipt\0\x01\xff\n";
        f.finish(expected).unwrap();
        let inode = fs::metadata(f.path()).unwrap().ino();
        for cut in 0..=expected.len() {
            f.write(&expected[..cut]);
            f.finish(expected).unwrap();
            f.finish(expected).unwrap();
            assert_eq!(fs::read(f.path()).unwrap(), expected);
            assert_eq!(fs::metadata(f.path()).unwrap().ino(), inode);
        }
    }
    #[test]
    fn empty_index_is_idempotent_but_populated_or_conflicting_bytes_are_preserved() {
        let f = Fixture::new();
        f.finish(&[]).unwrap();
        let inode = fs::metadata(f.path()).unwrap().ino();
        f.finish(&[]).unwrap();
        assert_eq!(fs::metadata(f.path()).unwrap().ino(), inode);
        for bytes in [
            b"SQLite format 3\0".as_slice(),
            b"changed",
            b"expected and later",
        ] {
            f.write(bytes);
            assert!(f.finish(b"expected").is_err());
            assert!(f.finish(&[]).is_err());
            assert_eq!(fs::read(f.path()).unwrap(), bytes);
        }
    }
    #[test]
    fn links_and_noncanonical_modes_are_preserved() {
        for change in 0..5 {
            let f = Fixture::new();
            f.write(b"pre");
            let outside = f.root.join("retained");
            match change {
                0 => {
                    fs::rename(f.path(), &outside).unwrap();
                    symlink(&outside, f.path()).unwrap();
                }
                1 => fs::hard_link(f.path(), &outside).unwrap(),
                2 => fs::set_permissions(f.path(), fs::Permissions::from_mode(0o644)).unwrap(),
                3 => fs::set_permissions(f.path(), fs::Permissions::from_mode(0o700)).unwrap(),
                _ => fs::set_permissions(f.path(), fs::Permissions::from_mode(0o4600)).unwrap(),
            }
            assert!(f.finish(b"prefix").is_err());
            assert_eq!(fs::read(f.path()).unwrap(), b"pre");
        }
    }
    #[test]
    fn replacement_or_same_inode_change_during_inspection_cannot_be_completed() {
        for replace in [false, true] {
            let f = Fixture::new();
            f.write(b"pre");
            let path = f.path();
            let retained = f.root.join("retained");
            BEFORE_APPEND.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(move || {
                    if replace {
                        fs::rename(&path, retained).unwrap();
                    }
                    fs::write(&path, if replace { b"pre" } else { b"new" }).unwrap();
                    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
                }))
            });
            assert!(f.finish(b"prefix").is_err());
            assert_eq!(
                fs::read(f.path()).unwrap(),
                if replace { b"pre" } else { b"new" }
            );
        }
    }
    #[test]
    fn displaced_parent_refuses_before_appending_either_generation() {
        let f = Fixture::new();
        f.write(b"pre");
        let root = f.root.clone();
        let displaced = f.root.with_extension("retained");
        let saved = displaced.clone();
        BEFORE_APPEND.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                fs::rename(&root, saved).unwrap();
                fs::create_dir(&root).unwrap();
                fs::write(root.join("receipt"), b"replacement").unwrap();
            }))
        });
        assert!(f.finish(b"prefix").is_err());
        assert_eq!(fs::read(displaced.join("receipt")).unwrap(), b"pre");
        assert_eq!(fs::read(f.path()).unwrap(), b"replacement");
        fs::remove_dir_all(displaced).unwrap();
    }
}
