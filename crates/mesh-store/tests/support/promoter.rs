#![allow(dead_code)]
//! A real content-addressed store for the crash harness, on a real filesystem.
//!
//! # Why this is here and not `mesh-cas`
//!
//! `mesh-cas` is the content-addressed store, it implements plan §6.3 steps 1 to 4, and its own
//! `crash-promotion.rs` kills a process at every one of them. This file does not replace any of
//! that and is not a second implementation of it for production use.
//!
//! It exists because **neither crate may declare a dependency** — not on each other and not on
//! `mesh-types` — which `docs/adr/0008-ship-the-sqlite-schema-without-a-sqlite-driver.md` records
//! with its alternatives. A `[dev-dependencies]` entry is a dependency: it rewrites `Cargo.lock`,
//! which is on this repository's governance list. So an integration test that drives both halves of
//! §6.3 in one process cannot exist in either crate, and the sequence's kill-point matrix would
//! otherwise stop at step 4 — precisely the boundary the acceptance criteria are about.
//!
//! What this supplies is the *ordering* steps 1 to 4 must have, on a real filesystem, so the
//! residue a `SIGKILL` leaves is a real residue:
//!
//! 1. bytes land in `scratch/` under a name no reader looks for;
//! 2. they are `fsync`ed before any name reveals them;
//! 3. they are read back **from disk** and hashed;
//! 4. the arrival is recorded and `fsync`ed *before* the rename that makes the chunk visible, then
//!    the chunk's directory is `fsync`ed.
//!
//! The arrival record is what makes step 4's residue *"a collection candidate, never a dangling
//! reference"* checkable: the parent reads a small file rather than walking the store, and a chunk
//! that became visible without being recorded would be invisible to the collector.
//!
//! # The digest is a test fixture and says so
//!
//! Chunk names here are a four-lane FNV-1a, not BLAKE3, because BLAKE3 lives in `mesh-cas` and
//! cannot be imported. Nothing in this file's purpose depends on which hash it is: what is under
//! test is that a name is a function of the bytes, that the bytes are read back and checked against
//! it, and what a kill leaves behind. A collision would be a bad hash and a fine test.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use mesh_store::{ChunkPromoter, RecordDigest};

/// Where scratch files live.
pub const SCRATCH_DIRECTORY: &str = "scratch";
/// Where promoted chunks live.
pub const CHUNKS_DIRECTORY: &str = "chunks";
/// The durable record of every chunk that is about to become visible.
pub const ARRIVALS_FILE: &str = "arrivals.log";

/// A four-lane FNV-1a over the bytes, giving a 32-byte content name.
#[must_use]
pub fn content_name(bytes: &[u8]) -> RecordDigest {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut out = [0u8; 32];
    for (lane, chunk) in out.chunks_exact_mut(8).enumerate() {
        let mut hash = OFFSET ^ (lane as u64).wrapping_mul(PRIME);
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(PRIME);
        }
        // Length is mixed in so that a truncation is a different name, which is the failure a
        // partly-written scratch file would produce.
        hash ^= bytes.len() as u64;
        hash = hash.wrapping_mul(PRIME);
        chunk.copy_from_slice(&hash.to_be_bytes());
    }
    RecordDigest::from_bytes(out)
}

/// A content-addressed store on a real filesystem, with the §6.3 step-1-to-4 ordering.
#[derive(Debug)]
pub struct FilePromoter {
    root: PathBuf,
    staged: Vec<(RecordDigest, PathBuf)>,
    /// A step number that is made to fail as though the disk were full.
    full_at: Option<u8>,
}

impl FilePromoter {
    /// Open — and create — the store under a workspace root.
    ///
    /// # Panics
    ///
    /// If the directories cannot be created, which means the test cannot run.
    pub fn open(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        for directory in [root.join(SCRATCH_DIRECTORY), root.join(CHUNKS_DIRECTORY)] {
            fs::create_dir_all(&directory).expect("the store's directories");
        }
        Self {
            root,
            staged: Vec::new(),
            full_at: None,
        }
    }

    /// Make the step with this plan §6.3 number fail as though the volume were full.
    #[must_use]
    pub const fn with_disk_full_at(mut self, plan_step: u8) -> Self {
        self.full_at = Some(plan_step);
        self
    }

    /// Where a chunk lives once promoted.
    #[must_use]
    pub fn chunk_path(&self, digest: &RecordDigest) -> PathBuf {
        let hex = digest.to_hex();
        self.root
            .join(CHUNKS_DIRECTORY)
            .join(&hex[0..2])
            .join(&hex[2..4])
            .join(&hex)
    }

    /// Every chunk name recorded as having arrived, in the order recorded.
    ///
    /// # Panics
    ///
    /// If the file exists but cannot be read.
    #[must_use]
    pub fn arrivals(&self) -> Vec<RecordDigest> {
        let path = self.root.join(ARRIVALS_FILE);
        let Ok(text) = fs::read_to_string(&path) else {
            return Vec::new();
        };
        text.lines()
            .filter(|line| !line.trim().is_empty())
            .filter_map(|line| RecordDigest::parse_hex(line.trim()).ok())
            .collect()
    }

    /// How many files are staged and not yet promoted.
    ///
    /// # Panics
    ///
    /// If the scratch directory cannot be listed.
    #[must_use]
    pub fn scratch_count(&self) -> usize {
        let scratch = self.root.join(SCRATCH_DIRECTORY);
        let Ok(entries) = fs::read_dir(&scratch) else {
            return 0;
        };
        entries.count()
    }

    /// Read a chunk back and check it against its own name.
    ///
    /// # Errors
    ///
    /// The name it actually hashes to, when that is not the name it is filed under.
    pub fn read_verified(&self, digest: &RecordDigest) -> Result<Vec<u8>, RecordDigest> {
        let bytes = fs::read(self.chunk_path(digest)).map_err(|_| *digest)?;
        let found = content_name(&bytes);
        if found == *digest {
            Ok(bytes)
        } else {
            Err(found)
        }
    }

    fn refuse_if_full(&self, plan_step: u8) -> Result<(), io::Error> {
        if self.full_at == Some(plan_step) {
            return Err(io::Error::other(format!(
                "No space left on device (injected at plan §6.3 step {plan_step})"
            )));
        }
        Ok(())
    }

    fn staging_path(&self, digest: &RecordDigest) -> PathBuf {
        self.root.join(SCRATCH_DIRECTORY).join(format!(
            "{}.{}.tmp",
            digest.to_hex(),
            std::process::id()
        ))
    }

    /// Record, durably, that a chunk is about to exist. Called before the rename, always.
    fn record_arrival(&self, digest: &RecordDigest) -> Result<(), io::Error> {
        let path = self.root.join(ARRIVALS_FILE);
        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
        writeln!(file, "{}", digest.to_hex())?;
        file.sync_all()
    }
}

impl ChunkPromoter for FilePromoter {
    type Error = io::Error;

    /// Step 1 — write to `scratch/`, under a name nothing reads.
    fn write_temporary(&mut self, chunks: &[Vec<u8>]) -> Result<Vec<RecordDigest>, Self::Error> {
        self.refuse_if_full(1)?;
        let mut names = Vec::with_capacity(chunks.len());
        for bytes in chunks {
            let digest = content_name(bytes);
            let path = self.staging_path(&digest);
            let mut file = File::create(&path)?;
            file.write_all(bytes)?;
            self.staged.push((digest, path));
            names.push(digest);
        }
        Ok(names)
    }

    /// Step 2 — force the staged bytes to durable storage, before any name reveals them.
    fn flush_temporary(&mut self) -> Result<(), Self::Error> {
        self.refuse_if_full(2)?;
        for (_, path) in &self.staged {
            File::open(path)?.sync_all()?;
        }
        Ok(())
    }

    /// Step 3 — read back **from disk** and check the bytes hash to the name they will take.
    fn verify_temporary(&mut self) -> Result<(), Self::Error> {
        self.refuse_if_full(3)?;
        for (digest, path) in &self.staged {
            let mut bytes = Vec::new();
            File::open(path)?.read_to_end(&mut bytes)?;
            let found = content_name(&bytes);
            if found != *digest {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{path:?} hashes to {found}, not to {digest}"),
                ));
            }
        }
        Ok(())
    }

    /// Step 4 — record the arrival durably, then rename, then sync the directory.
    fn promote(&mut self) -> Result<(), Self::Error> {
        self.refuse_if_full(4)?;
        for (digest, staged) in std::mem::take(&mut self.staged) {
            let target = self.chunk_path(&digest);
            if target.exists() {
                fs::remove_file(&staged)?;
                continue;
            }
            // Before the rename, never after: a chunk that becomes visible must already be
            // findable, or a crash here leaks it.
            self.record_arrival(&digest)?;
            let directory = target.parent().expect("a chunk has a directory");
            fs::create_dir_all(directory)?;
            fs::rename(&staged, &target)?;
            File::open(directory)?.sync_all()?;
        }
        Ok(())
    }

    fn discard_temporary(&mut self) -> Result<usize, Self::Error> {
        let scratch = self.root.join(SCRATCH_DIRECTORY);
        let mut removed = 0;
        for entry in fs::read_dir(&scratch)? {
            fs::remove_file(entry?.path())?;
            removed += 1;
        }
        self.staged.clear();
        Ok(removed)
    }

    fn is_durable(&self, digest: &RecordDigest) -> bool {
        self.chunk_path(digest).exists()
    }
}

/// Every chunk in the store, found by walking it. The backstop, used only to check the arrival
/// journal against reality.
///
/// # Panics
///
/// If a directory exists but cannot be listed.
#[must_use]
pub fn sweep_chunks(root: &Path) -> Vec<RecordDigest> {
    let mut found = Vec::new();
    let Ok(first_level) = fs::read_dir(root.join(CHUNKS_DIRECTORY)) else {
        return found;
    };
    for first in first_level.flatten() {
        let Ok(second_level) = fs::read_dir(first.path()) else {
            continue;
        };
        for second in second_level.flatten() {
            let Ok(entries) = fs::read_dir(second.path()) else {
                continue;
            };
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if let Ok(digest) = RecordDigest::parse_hex(name) {
                        found.push(digest);
                    }
                }
            }
        }
    }
    found.sort_unstable();
    found
}
