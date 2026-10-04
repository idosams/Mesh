//! Deterministic process-kill coverage of collection. This is not a power-loss or fleet-policy proof.
#![cfg(unix)]
mod support;

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use mesh_cas::{Blake3, Cas, CollectionMode, ContentDigest, Digest32, DurableFs, StdFs};
use support::TempRoot;

const ROOT: &str = "MESH_COLLECTION_CRASH_ROOT";
const POINT: &str = "MESH_COLLECTION_CRASH_POINT";
const READY: &str = "MESH-COLLECTION-READY";
const BODIES: [&[u8]; 3] = [
    b"pinned immutable review",
    b"unreferenced first",
    b"unreferenced second",
];
const POINTS: [(&str, usize, bool); 8] = [
    ("before-unlink", 0, false),
    ("after-first-unlink", 1, false),
    ("after-second-unlink", 2, false),
    ("after-chunk-sync", 2, false),
    ("partial-journal-write", 2, false),
    ("after-journal-sync", 2, false),
    ("after-journal-rename", 2, true),
    ("after-journal-directory-sync", 2, true),
];
fn digests() -> [Digest32; 3] {
    BODIES.map(Blake3::digest_bytes)
}

#[derive(Debug)]
struct PausingFs {
    point: String,
    chunks: [PathBuf; 2],
    rewrite: PathBuf,
    journal: PathBuf,
    logs: PathBuf,
    removed: AtomicUsize,
}
impl PausingFs {
    fn stop(&self, point: &str) {
        if self.point == point {
            println!("{READY}");
            io::stdout().flush().unwrap();
            // A missing parent kill is a failure, never a successful simulated crash.
            std::thread::sleep(Duration::from_secs(60));
            std::process::exit(96);
        }
    }
}
impl DurableFs for PausingFs {
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        StdFs.create_dir_all(path)
    }
    fn stage(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        if path == self.rewrite && self.point == "partial-journal-write" {
            assert!(bytes.len() > 1);
            StdFs.stage(path, &bytes[..bytes.len() / 2])?;
            self.stop("partial-journal-write");
            unreachable!();
        }
        StdFs.stage(path, bytes)
    }
    fn append(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        StdFs.append(path, bytes)
    }
    fn sync_file(&self, path: &Path) -> io::Result<()> {
        StdFs.sync_file(path)?;
        if path == self.rewrite {
            self.stop("after-journal-sync");
        }
        Ok(())
    }
    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        StdFs.sync_dir(path)?;
        if self.removed.load(Ordering::SeqCst) == 2 {
            if path == self.logs {
                self.stop("after-journal-directory-sync");
            } else if self.chunks.iter().any(|chunk| chunk.parent() == Some(path)) {
                self.stop("after-chunk-sync");
            }
        }
        Ok(())
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        StdFs.rename(from, to)?;
        if from == self.rewrite && to == self.journal {
            self.stop("after-journal-rename");
        }
        Ok(())
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
        let chunk = self.chunks.iter().any(|candidate| candidate == path);
        if chunk {
            self.stop("before-unlink");
        }
        StdFs.remove_file(path)?;
        if chunk {
            match self.removed.fetch_add(1, Ordering::SeqCst) {
                0 => self.stop("after-first-unlink"),
                1 => self.stop("after-second-unlink"),
                _ => panic!("unexpected extra unlink"),
            }
        }
        Ok(())
    }
}

#[test]
fn collection_crash_child() {
    let Some(root) = std::env::var_os(ROOT) else {
        assert!(std::env::var_os(POINT).is_none());
        return;
    };
    let point = std::env::var(POINT).unwrap();
    assert!(POINTS.iter().any(|(name, _, _)| *name == point));
    let store = Cas::open(&root).unwrap();
    let ids = digests();
    let fs = PausingFs {
        point,
        chunks: [
            store.layout().chunk_path(&ids[1]),
            store.layout().chunk_path(&ids[2]),
        ],
        rewrite: store.layout().arrival_journal_rewrite(),
        journal: store.layout().arrival_journal(),
        logs: store.layout().logs_directory(),
        removed: AtomicUsize::new(0),
    };
    let store = Cas::<_, Blake3>::with_filesystem(root, fs).unwrap();
    // Deliberately include the protected digest in the candidate list: the independent oracle
    // must veto it, including when collection is retried after a real process death.
    store
        .collect(&ids, &|id: &Digest32| *id == ids[0], CollectionMode::Delete)
        .unwrap();
    panic!("requested crash point was not reached");
}

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn kill_at(root: &Path, point: &str) {
    let mut child = OwnedChild(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "collection_crash_child",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(ROOT, root)
            .env(POINT, point)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let stdout = child.0.stdout.take().unwrap();
    let (send, receive) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let found = BufReader::new(stdout)
            .lines()
            .any(|line| line.is_ok_and(|line| line.contains(READY)));
        let _ = send.send(found);
    });
    assert_eq!(
        receive.recv_timeout(Duration::from_secs(30)),
        Ok(true),
        "no handshake at {point}"
    );
    reader.join().unwrap();
    child.0.kill().unwrap();
    let status = child.0.wait().unwrap();
    assert_eq!(status.signal(), Some(9), "not SIGKILL at {point}: {status}");
}

#[test]
fn killed_collection_preserves_retained_bytes_and_resumes_after_each_boundary() {
    for (point, removed, rewritten) in POINTS {
        let root = TempRoot::new(point);
        let store = Cas::open(root.path()).unwrap();
        let ids = digests();
        for (body, expected) in BODIES.into_iter().zip(ids) {
            assert_eq!(store.promote(body.to_vec()).unwrap().digest(), expected);
        }
        let before_journal = std::fs::read(store.layout().arrival_journal()).unwrap();
        drop(store);
        kill_at(root.path(), point);
        let store = Cas::open(root.path()).unwrap();
        assert_eq!(
            store.read(&ids[0]).unwrap(),
            BODIES[0],
            "retained bytes at {point}"
        );
        for (index, id) in ids[1..].iter().enumerate() {
            assert_eq!(
                store.contains(id),
                index >= removed,
                "wrong removed subset at {point}"
            );
            if index >= removed {
                assert_eq!(store.read(id).unwrap(), BODIES[index + 1]);
            }
        }
        if rewritten {
            assert_eq!(
                store.journal().candidates().unwrap(),
                vec![ids[0]],
                "new journal at {point}"
            );
        } else {
            assert_eq!(
                std::fs::read(store.layout().arrival_journal()).unwrap(),
                before_journal,
                "old journal changed before replacement at {point}"
            );
        }
        let result = store
            .collect(&ids, &|id: &Digest32| *id == ids[0], CollectionMode::Delete)
            .unwrap();
        assert_eq!(result.refused(), &[ids[0]]);
        assert!(ids[1..].iter().all(|id| !store.contains(id)));
        assert_eq!(store.read(&ids[0]).unwrap(), BODIES[0]);
        // Exercise recovery of any half-written journal staging file even if every orphan was
        // already absent and the retry therefore had no newly removed chunks to forget.
        store.journal().compact().unwrap();
        assert!(!store.layout().arrival_journal_rewrite().exists());
        drop(store);
        assert_eq!(
            Cas::open(root.path()).unwrap().read(&ids[0]).unwrap(),
            BODIES[0]
        );
    }
}
