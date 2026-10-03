//! Complete a retained received-file prefix without truncation or replacement.
use super::*;
use std::io::{Seek as _, SeekFrom};
use std::os::unix::fs::MetadataExt as _;

#[cfg(test)]
thread_local! {
    static BEFORE_APPEND: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = std::cell::RefCell::new(None);
}

fn changed() -> io::Error {
    io::Error::other("received copy differs from its original input")
}
fn identity(file: &File) -> io::Result<(u64, u64, u64)> {
    let value = file.metadata()?;
    if !value.is_file() || value.nlink() != 1 || !matches!(value.mode() & 0o7777, 0o600 | 0o700) {
        return Err(changed());
    }
    Ok((value.dev(), value.ino(), value.len()))
}
fn verify_source(file: &mut File, expected: &ImportedFile) -> io::Result<()> {
    let metadata = file.metadata()?;
    if identity(file)?.2 != expected.bytes
        || metadata_is_executable(&metadata) != expected.executable
    {
        return Err(changed());
    }
    file.seek(SeekFrom::Start(0))?;
    let mut hasher = Blake3::hasher();
    let mut remaining = expected.bytes;
    let mut buffer = [0u8; FILE_IO_BUFFER_BYTES];
    while remaining > 0 {
        let count = usize::try_from(remaining.min(buffer.len() as u64)).map_err(|_| changed())?;
        file.read_exact(&mut buffer[..count])?;
        hasher.update(&buffer[..count]);
        remaining -= count as u64;
    }
    if hasher.finalize() != expected.digest || file.read(&mut buffer[..1])? != 0 {
        return Err(changed());
    }
    file.seek(SeekFrom::Start(0))?;
    Ok(())
}
fn compare_prefix(source: &mut File, copy: &mut File, bytes: u64) -> io::Result<()> {
    source.seek(SeekFrom::Start(0))?;
    copy.seek(SeekFrom::Start(0))?;
    let mut remaining = bytes;
    let mut input = [0u8; FILE_IO_BUFFER_BYTES];
    let mut retained = [0u8; FILE_IO_BUFFER_BYTES];
    while remaining > 0 {
        let count = usize::try_from(remaining.min(input.len() as u64)).map_err(|_| changed())?;
        source.read_exact(&mut input[..count])?;
        copy.read_exact(&mut retained[..count])?;
        if input[..count] != retained[..count] {
            return Err(changed());
        }
        remaining -= count as u64;
    }
    Ok(())
}

pub(super) fn finish(
    source: &PinnedWorkspaceRoot,
    destination: &PinnedWorkspaceRoot,
    expected: &ImportedFile,
) -> io::Result<()> {
    source.ensure_namespace_identity()?;
    destination.ensure_namespace_identity()?;
    let mut input = source
        .filesystem()
        .read_only()
        .read_file(&expected.relative_path)?;
    let input_identity = identity(&input)?;
    verify_source(&mut input, expected)?;
    let mut prefix = destination
        .filesystem()
        .read_only()
        .read_file(&expected.relative_path)?;
    let prefix_identity = identity(&prefix)?;
    if prefix_identity.2 > expected.bytes
        || (!expected.executable && metadata_is_executable(&prefix.metadata()?))
    {
        return Err(changed());
    }
    compare_prefix(&mut input, &mut prefix, prefix_identity.2)?;
    #[cfg(test)]
    BEFORE_APPEND.with(|hook| {
        if let Some(hook) = hook.borrow_mut().take() {
            hook();
        }
    });
    // Recheck the immutable source after prefix inspection before acquiring write access.
    verify_source(&mut input, expected)?;
    // The independent writable descriptor must still name the exact inspected file. It cannot
    // create a missing name, follow a link, truncate bytes or block on a substituted FIFO.
    let mut output = destination
        .filesystem()
        .open_private_append_existing(&expected.relative_path)?;
    if identity(&output)? != prefix_identity || identity(&input)? != input_identity {
        return Err(changed());
    }
    source.ensure_namespace_identity()?;
    destination.ensure_namespace_identity()?;
    compare_prefix(&mut input, &mut output, prefix_identity.2)?;
    let mut remaining = expected.bytes - prefix_identity.2;
    let mut buffer = [0u8; FILE_IO_BUFFER_BYTES];
    while remaining > 0 {
        let count = usize::try_from(remaining.min(buffer.len() as u64)).map_err(|_| changed())?;
        input.read_exact(&mut buffer[..count])?;
        output.write_all(&buffer[..count])?;
        remaining -= count as u64;
    }
    // A complete byte copy may have been interrupted before its final executable mode was set.
    // Apply that original input mode only to this retained inode, never a reopened pathname.
    output.set_permissions(fs::Permissions::from_mode(if expected.executable {
        0o700
    } else {
        0o600
    }))?;
    output.sync_all()?;
    verify_source(&mut input, expected)?;
    let mut complete = destination
        .filesystem()
        .read_only()
        .read_file(&expected.relative_path)?;
    if identity(&complete)? != (prefix_identity.0, prefix_identity.1, expected.bytes) {
        return Err(changed());
    }
    verify_source(&mut complete, expected)?;
    source.ensure_namespace_identity()?;
    destination.ensure_namespace_identity()?;
    destination.sync()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture {
        root: PathBuf,
        source: PathBuf,
        destination: PathBuf,
        input: PinnedWorkspaceRoot,
        output: PinnedWorkspaceRoot,
        snapshot: Snapshot,
        bytes: Vec<u8>,
    }
    impl Fixture {
        fn new(bytes: Vec<u8>, executable: bool) -> Self {
            let root = std::env::temp_dir().join(format!(
                "mesh-received-copy-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let source = root.join("source");
            let destination = root.join("destination");
            fs::create_dir_all(&source).unwrap();
            fs::create_dir(&destination).unwrap();
            fs::write(source.join("work"), &bytes).unwrap();
            fs::set_permissions(
                source.join("work"),
                fs::Permissions::from_mode(if executable { 0o700 } else { 0o600 }),
            )
            .unwrap();
            let snapshot = import_snapshot(&source).unwrap();
            let input = PinnedWorkspaceRoot::open(source.clone()).unwrap();
            let output = PinnedWorkspaceRoot::open(destination.clone()).unwrap();
            Self {
                root,
                source,
                destination,
                input,
                output,
                snapshot,
                bytes,
            }
        }
        fn file(&self) -> PathBuf {
            self.destination.join("work")
        }
        fn prefix(&self, cut: usize) {
            fs::write(self.file(), &self.bytes[..cut]).unwrap();
            fs::set_permissions(self.file(), fs::Permissions::from_mode(0o600)).unwrap();
        }
        fn complete(&self) -> Result<(), FolderImportError> {
            copy_snapshot_with_destination_root(
                &self.source,
                directory_identity(&self.source).unwrap(),
                &self.destination,
                directory_identity(&self.destination).unwrap(),
                &self.output,
                &self.snapshot,
                ImportPurpose::Received,
            )
        }
        fn finish(&self) -> io::Result<()> {
            finish(&self.input, &self.output, &self.snapshot.summary.files[0])
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    #[test]
    fn every_file_byte_cut_completes_original_inode_and_mode_once() {
        for executable in [false, true] {
            let f = Fixture::new(b"original\0received\xfffile".to_vec(), executable);
            for cut in 0..=f.bytes.len() {
                f.prefix(cut);
                let inode = fs::metadata(f.file()).unwrap().ino();
                f.complete().unwrap();
                f.complete().unwrap();
                assert_eq!(fs::read(f.file()).unwrap(), f.bytes);
                let metadata = fs::metadata(f.file()).unwrap();
                assert_eq!(metadata.ino(), inode);
                assert_eq!(metadata_is_executable(&metadata), executable);
                assert_eq!(fs::read(f.source.join("work")).unwrap(), f.bytes);
            }
        }
        let empty = Fixture::new(Vec::new(), true);
        empty.prefix(0);
        empty.complete().unwrap();
        assert!(metadata_is_executable(&fs::metadata(empty.file()).unwrap()));
    }
    #[test]
    fn prefix_comparison_and_completion_span_multiple_io_buffers() {
        let f = Fixture::new(
            (0..FILE_IO_BUFFER_BYTES * 2 + 13)
                .map(|i| (i % 251) as u8)
                .collect(),
            false,
        );
        for cut in [
            1,
            FILE_IO_BUFFER_BYTES - 1,
            FILE_IO_BUFFER_BYTES,
            FILE_IO_BUFFER_BYTES + 1,
            f.bytes.len() - 1,
        ] {
            f.prefix(cut);
            f.complete().unwrap();
            assert_eq!(fs::read(f.file()).unwrap(), f.bytes);
        }
    }
    #[test]
    fn conflicting_or_longer_work_is_never_overwritten() {
        let f = Fixture::new(b"original input".to_vec(), false);
        for bytes in [
            b"later edit".to_vec(),
            [f.bytes.as_slice(), b"extra"].concat(),
        ] {
            f.prefix(0);
            fs::write(f.file(), &bytes).unwrap();
            assert!(f.complete().is_err());
            assert_eq!(fs::read(f.file()).unwrap(), bytes);
        }
    }
    #[test]
    fn links_permissions_and_unexpected_executable_work_refuse() {
        for change in 0..5 {
            let f = Fixture::new(b"original input".to_vec(), false);
            f.prefix(3);
            let outside = f.root.join("outside");
            match change {
                0 => {
                    fs::rename(f.file(), &outside).unwrap();
                    symlink(&outside, f.file()).unwrap();
                }
                1 => fs::hard_link(f.file(), &outside).unwrap(),
                2 => fs::set_permissions(f.file(), fs::Permissions::from_mode(0o644)).unwrap(),
                3 => fs::set_permissions(f.file(), fs::Permissions::from_mode(0o700)).unwrap(),
                _ => fs::set_permissions(f.file(), fs::Permissions::from_mode(0o4600)).unwrap(),
            }
            assert!(f.complete().is_err());
            assert_eq!(fs::read(f.file()).unwrap(), f.bytes[..3]);
            if change < 2 {
                assert_eq!(fs::read(outside).unwrap(), f.bytes[..3]);
            }
        }
    }
    #[test]
    fn replacement_and_same_inode_edits_after_inspection_cannot_inherit_append() {
        for replacement in [false, true] {
            let f = Fixture::new(b"original input".to_vec(), false);
            f.prefix(3);
            let file = f.file();
            let displaced = f.root.join("displaced");
            BEFORE_APPEND.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(move || {
                    if replacement {
                        fs::rename(&file, displaced).unwrap();
                    }
                    fs::write(&file, if replacement { b"ori" } else { b"new" }).unwrap();
                    fs::set_permissions(file, fs::Permissions::from_mode(0o600)).unwrap();
                }))
            });
            assert!(f.finish().is_err());
            assert_eq!(
                fs::read(f.file()).unwrap(),
                if replacement { b"ori" } else { b"new" }
            );
        }
    }
    #[test]
    fn changed_source_and_displaced_parent_refuse_before_append() {
        for source_change in [false, true] {
            let f = Fixture::new(b"original input".to_vec(), false);
            f.prefix(3);
            let source = f.source.join("work");
            let destination = f.destination.clone();
            let displaced = f.root.join("displaced");
            let retained = displaced.clone();
            BEFORE_APPEND.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(move || {
                    if source_change {
                        fs::write(source, b"different data").unwrap();
                    } else {
                        fs::rename(destination, displaced).unwrap();
                    }
                }))
            });
            assert!(f.finish().is_err());
            let file = if source_change {
                f.file()
            } else {
                retained.join("work")
            };
            assert_eq!(fs::read(file).unwrap(), b"ori");
        }
    }
    #[test]
    fn ordinary_import_collision_does_not_acquire_completion_authority() {
        let f = Fixture::new(b"original input".to_vec(), false);
        f.prefix(3);
        assert!(copy_snapshot_with_destination_root(
            &f.source,
            directory_identity(&f.source).unwrap(),
            &f.destination,
            directory_identity(&f.destination).unwrap(),
            &f.output,
            &f.snapshot,
            ImportPurpose::User
        )
        .is_err());
        assert_eq!(fs::read(f.file()).unwrap(), b"ori");
    }
}
