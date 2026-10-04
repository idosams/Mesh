//! Bounded ENOSPC injection against real files; this does not fill the host filesystem.
#![cfg(unix)]
mod support;

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use mesh_cas::{Blake3, Cas, CasError, CollectionMode, Digest32, DurableFs, StdFs};
use support::TempRoot;

#[derive(Debug)]
struct FullFs {
    operation: &'static str,
    path: PathBuf,
    partial: bool,
    hits: AtomicUsize,
}
impl FullFs {
    fn refuse(&self, operation: &str, path: &Path) -> io::Result<()> {
        if operation == self.operation && path == self.path {
            self.hits.fetch_add(1, Ordering::SeqCst);
            // ENOSPC on the supported Unix platforms. Do not consume real disk capacity.
            return Err(io::Error::from_raw_os_error(28));
        }
        Ok(())
    }
}
impl DurableFs for FullFs {
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.refuse("create_dir_all", path)?;
        StdFs.create_dir_all(path)
    }
    fn stage(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        if self.partial && path == self.path {
            assert!(bytes.len() > 1);
            StdFs.stage(path, &bytes[..bytes.len() / 2])?;
        }
        self.refuse("stage", path)?;
        StdFs.stage(path, bytes)
    }
    fn append(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.refuse("append", path)?;
        StdFs.append(path, bytes)
    }
    fn sync_file(&self, path: &Path) -> io::Result<()> {
        self.refuse("sync_file", path)?;
        StdFs.sync_file(path)
    }
    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        self.refuse("sync_dir", path)?;
        StdFs.sync_dir(path)
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.refuse("rename", from)?;
        StdFs.rename(from, to)
    }
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        StdFs.read(path)
    }
    fn exists(&self, path: &Path) -> bool {
        StdFs.exists(path)
    }
    fn file_len(&self, path: &Path) -> io::Result<u64> {
        StdFs.file_len(path)
    }
    fn list_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        StdFs.list_dir(path)
    }
    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.refuse("remove_file", path)?;
        StdFs.remove_file(path)
    }
}

#[test]
fn collection_reports_storage_full_and_retains_exact_bytes_across_reopen_and_retry() {
    for point in 0..8 {
        let root = TempRoot::new("collection-storage-full");
        let store = Cas::open(root.path()).unwrap();
        let retained_body = b"immutable pinned review remains available";
        let retained = store.promote(retained_body.to_vec()).unwrap().digest();
        let first = store.promote(b"first orphan".to_vec()).unwrap().digest();
        let second = store.promote(b"second orphan".to_vec()).unwrap().digest();
        let old_journal = std::fs::read(store.layout().arrival_journal()).unwrap();
        let rewrite = store.layout().arrival_journal_rewrite();
        let (operation, path, removed) = match point {
            0 => ("remove_file", store.layout().chunk_path(&first), 0),
            1 => ("remove_file", store.layout().chunk_path(&second), 1),
            2 => ("sync_dir", store.layout().chunk_directory(&first), 2),
            3 | 4 => ("stage", rewrite.clone(), 2),
            5 => ("sync_file", rewrite.clone(), 2),
            6 => ("rename", rewrite.clone(), 2),
            7 => ("sync_dir", store.layout().logs_directory(), 2),
            _ => unreachable!(),
        };
        drop(store);
        let store = Cas::<_, Blake3>::with_filesystem(
            root.path(),
            FullFs {
                operation,
                path: path.clone(),
                partial: point == 4,
                hits: AtomicUsize::new(0),
            },
        )
        .unwrap();
        let error = store
            .collect(
                &[retained, first, second],
                &|id: &Digest32| *id == retained,
                CollectionMode::Delete,
            )
            .expect_err("storage exhaustion must not report a successful collection");
        match error {
            CasError::Io {
                operation: actual,
                path: actual_path,
                source,
            } => {
                assert_eq!(actual, operation, "point {point}");
                assert_eq!(actual_path, path, "point {point}");
                assert_eq!(source.raw_os_error(), Some(28), "point {point}");
            }
            other => panic!("wrong diagnosis at {point}: {other}"),
        }
        assert_eq!(store.filesystem().hits.load(Ordering::SeqCst), 1);
        drop(store);

        // Reopen without the injected capacity limit, as after space becomes available.
        let store = Cas::open(root.path()).unwrap();
        assert_eq!(
            store.read(&retained).unwrap(),
            retained_body,
            "point {point}"
        );
        for (index, id) in [first, second].iter().enumerate() {
            assert_eq!(store.contains(id), index >= removed, "point {point}");
        }
        if point == 7 {
            // Rename already happened; failing its directory sync must still return an error.
            assert_eq!(store.journal().candidates().unwrap(), vec![retained]);
        } else {
            assert_eq!(
                std::fs::read(store.layout().arrival_journal()).unwrap(),
                old_journal
            );
        }
        if point == 4 {
            let expected = format!("+ {}\n", retained.to_hex());
            assert_eq!(
                std::fs::read(&rewrite).unwrap(),
                expected.as_bytes()[..expected.len() / 2]
            );
        }
        let result = store
            .collect(
                &[retained, first, second],
                &|id: &Digest32| *id == retained,
                CollectionMode::Delete,
            )
            .unwrap();
        assert_eq!(result.refused(), &[retained]);
        assert_eq!(result.collected().len(), 2 - removed);
        assert_eq!(result.absent().len(), removed);
        assert!(!store.contains(&first) && !store.contains(&second));
        // All orphans may already be absent, so the retry need not rewrite the journal.
        // Explicit compaction is the supported recovery of a partial replacement file.
        store.journal().compact().unwrap();
        assert!(!rewrite.exists());
        assert!(store.journal().candidates().unwrap().contains(&retained));
        let fresh_body = b"new checkpoint after capacity returns";
        let fresh = store.promote(fresh_body.to_vec()).unwrap().digest();
        drop(store);
        let reopened = Cas::open(root.path()).unwrap();
        assert_eq!(reopened.read(&retained).unwrap(), retained_body);
        assert_eq!(reopened.read(&fresh).unwrap(), fresh_body);
        assert!(reopened.journal().candidates().unwrap().contains(&fresh));
    }
}
