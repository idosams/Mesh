//! Complete only the canonical original received-import journal, without truncating any bytes.
//! This is private ingestion machinery, not a recovery/launch capability or a generic journal repair.
use super::*;
use std::os::unix::fs::MetadataExt as _;

struct JournalPrefix {
    bytes: Vec<u8>,
    identity: (u64, u64),
}
fn unavailable() -> io::Error {
    io::Error::other("received initial history is unavailable or differs from its original input")
}
fn inspect(root: &PinnedWorkspaceRoot, expected: &[u8]) -> io::Result<JournalPrefix> {
    root.ensure_namespace_identity()?;
    let file = root
        .filesystem()
        .read_only()
        .read_file(Path::new(crate::workspace::RECORD_FILE_NAME))?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.mode() & 0o077 != 0
        || metadata.len() > expected.len() as u64
    {
        return Err(unavailable());
    }
    let mut bytes = Vec::new();
    file.take(expected.len() as u64 + 1)
        .read_to_end(&mut bytes)?;
    if !expected.starts_with(&bytes) {
        return Err(unavailable());
    }
    root.ensure_namespace_identity()?;
    Ok(JournalPrefix {
        bytes,
        identity: (metadata.dev(), metadata.ino()),
    })
}
fn append_suffix(
    root: &PinnedWorkspaceRoot,
    expected: &[u8],
    prefix: &JournalPrefix,
) -> io::Result<()> {
    let current = inspect(root, expected)?;
    if current.identity != prefix.identity || current.bytes != prefix.bytes {
        return Err(unavailable());
    }
    let mut file = root
        .filesystem()
        .open_private_append_existing(Path::new(crate::workspace::RECORD_FILE_NAME))?;
    let metadata = file.metadata()?;
    if (metadata.dev(), metadata.ino()) != prefix.identity
        || metadata.len() != prefix.bytes.len() as u64
    {
        return Err(unavailable());
    }
    root.ensure_namespace_identity()?;
    file.write_all(&expected[prefix.bytes.len()..])?;
    file.sync_all()?;
    let complete = inspect(root, expected)?;
    if complete.identity != prefix.identity || complete.bytes != expected {
        return Err(unavailable());
    }
    root.sync()
}

pub(super) fn finish(
    root: &PinnedWorkspaceRoot,
    path: &Path,
    checkpoint: Checkpoint,
    objects: Vec<Vec<u8>>,
) -> io::Result<u64> {
    let expected = checkpoint
        .records()
        .iter()
        .flat_map(mesh_store::frame_record)
        .collect::<Vec<_>>();
    if expected.is_empty() {
        return Err(unavailable());
    }
    root.ensure_namespace_identity()?;
    let prefix = match inspect(root, &expected) {
        Ok(prefix) => prefix,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            root.filesystem().write_new_file(
                Path::new(crate::workspace::RECORD_FILE_NAME),
                &[],
                fs::Permissions::from_mode(0o600),
            )?;
            inspect(root, &expected)?
        }
        Err(error) => return Err(error),
    };
    // Validate/promote all referenced payloads before extending the journal. The SQLite commit
    // happens in a fresh transient index: a torn journal must not be opened as an empty workspace.
    let cas = Cas::<_, mesh_cas::Blake3>::with_filesystem(path.to_path_buf(), root.filesystem())
        .map_err(io::Error::other)?;
    let mut promoter = CasChunkPromoter::new(&cas);
    let driver = mesh_store::Sqlite::open_in_memory().map_err(io::Error::other)?;
    let mut store = mesh_store::Store::open(driver).map_err(io::Error::other)?;
    DurableCommit::new(&mut store, &mut promoter, objects, checkpoint)
        .finish()
        .map_err(io::Error::other)?;
    append_suffix(root, &expected, &prefix)?;
    Ok(promoter.linked_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mesh_store::StoredRecord;
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture {
        root: PathBuf,
        store: PathBuf,
        pin: PinnedWorkspaceRoot,
        expected: Vec<u8>,
        checkpoint: Checkpoint,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "mesh-received-genesis-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let source = root.join("source");
            fs::create_dir_all(&source).unwrap();
            fs::write(source.join("work.txt"), b"original received work\n").unwrap();
            let store = root.join("worker.mesh");
            PreparedFolderImport::prepare_received_with_parent(
                &source,
                &store,
                &[],
                ProtectedWorkspaceRoot::inspect(&root).unwrap(),
            )
            .unwrap()
            .confirm_into_workspace_without_origin()
            .unwrap();
            let expected = fs::read(store.join(crate::workspace::RECORD_FILE_NAME)).unwrap();
            let mut checkpoint = Checkpoint::default();
            for record in mesh_store::scan_journal(&expected).unwrap().into_records() {
                match record {
                    StoredRecord::Manifest(value) => checkpoint.manifests.push(value),
                    StoredRecord::Operation(value) => checkpoint.operations.push(value),
                    _ => panic!("unexpected genesis record"),
                }
            }
            let pin = PinnedWorkspaceRoot::open(store.clone()).unwrap();
            Self {
                root,
                store,
                pin,
                expected,
                checkpoint,
            }
        }
        fn journal(&self) -> PathBuf {
            self.store.join(crate::workspace::RECORD_FILE_NAME)
        }
        fn finish(&self) -> io::Result<u64> {
            finish(&self.pin, &self.store, self.checkpoint.clone(), Vec::new())
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    #[test]
    fn every_initial_journal_byte_cut_completes_once_without_truncation() {
        let fixture = Fixture::new();
        let identity = fs::metadata(fixture.journal()).unwrap().ino();
        for cut in 0..=fixture.expected.len() {
            fs::write(fixture.journal(), &fixture.expected[..cut]).unwrap();
            fixture.finish().unwrap();
            assert_eq!(
                fs::read(fixture.journal()).unwrap(),
                fixture.expected,
                "cut {cut}"
            );
            assert_eq!(fs::metadata(fixture.journal()).unwrap().ino(), identity);
        }
        fixture.finish().unwrap();
        fixture.finish().unwrap();
        assert_eq!(fs::read(fixture.journal()).unwrap(), fixture.expected);
    }
    #[test]
    fn foreign_tail_and_changed_prefix_are_preserved() {
        let fixture = Fixture::new();
        for bytes in [
            b"foreign".to_vec(),
            [fixture.expected.as_slice(), b"later work"].concat(),
        ] {
            fs::write(fixture.journal(), &bytes).unwrap();
            assert!(fixture.finish().is_err());
            assert_eq!(fs::read(fixture.journal()).unwrap(), bytes);
        }
    }
    #[test]
    fn journal_links_and_shared_permissions_refuse_without_writing() {
        for change in 0..3 {
            let fixture = Fixture::new();
            let outside = fixture.root.join("outside");
            match change {
                0 => {
                    fs::rename(fixture.journal(), &outside).unwrap();
                    symlink(&outside, fixture.journal()).unwrap();
                }
                1 => {
                    fs::hard_link(fixture.journal(), &outside).unwrap();
                }
                _ => fs::set_permissions(fixture.journal(), fs::Permissions::from_mode(0o644))
                    .unwrap(),
            }
            assert!(fixture.finish().is_err());
            assert_eq!(fs::read(fixture.journal()).unwrap(), fixture.expected);
            if change != 2 {
                assert_eq!(fs::read(outside).unwrap(), fixture.expected);
            }
        }
    }
    #[test]
    fn replaced_or_changed_journal_cannot_inherit_the_inspected_prefix() {
        for replacement in [false, true] {
            let fixture = Fixture::new();
            fs::write(fixture.journal(), &fixture.expected[..7]).unwrap();
            let prefix = inspect(&fixture.pin, &fixture.expected).unwrap();
            if replacement {
                fs::rename(fixture.journal(), fixture.root.join("retained-journal")).unwrap();
                fs::write(fixture.journal(), &fixture.expected[..7]).unwrap();
                fs::set_permissions(fixture.journal(), fs::Permissions::from_mode(0o600)).unwrap();
            } else {
                fs::write(fixture.journal(), &fixture.expected[..8]).unwrap();
            }
            let before = fs::read(fixture.journal()).unwrap();
            assert!(append_suffix(&fixture.pin, &fixture.expected, &prefix).is_err());
            assert_eq!(fs::read(fixture.journal()).unwrap(), before);
        }
    }
    #[test]
    fn missing_payload_refuses_before_extending_an_existing_prefix() {
        let fixture = Fixture::new();
        let empty = fixture.root.join("empty-private-store");
        fs::create_dir(&empty).unwrap();
        let pin = PinnedWorkspaceRoot::open(empty.clone()).unwrap();
        pin.filesystem()
            .write_new_file(
                Path::new(crate::workspace::RECORD_FILE_NAME),
                &fixture.expected[..7],
                fs::Permissions::from_mode(0o600),
            )
            .unwrap();
        assert!(finish(&pin, &empty, fixture.checkpoint.clone(), Vec::new()).is_err());
        assert_eq!(
            fs::read(empty.join(crate::workspace::RECORD_FILE_NAME)).unwrap(),
            fixture.expected[..7]
        );
    }
}
