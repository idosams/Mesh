#![allow(dead_code)]
//! The durable record journal on a real filesystem: the [`RecordJournal`] the tests recover from.
//!
//! # Why the durability is the whole implementation
//!
//! `mesh-store` owns the frame layout, the checksums and the scan that decides where durability
//! stops — those are the parts that must not be a fixture, and they are unit-tested in
//! `src/recovery.rs`. What is left for a journal to do is one thing: **make bytes durable before
//! returning**. [`FileJournal::append`] therefore writes and `fsync`s, in that order, with no
//! buffering anywhere, because a buffered append moves the last durable boundary somewhere no scan
//! can see and every recovery assertion in this suite would be measuring a lie.
//!
//! The append is deliberately *not* atomic in any stronger sense. A kill mid-`write` leaves a
//! partial frame, which is precisely the state `TailResidue::Fragment` exists to name, and
//! `tests/recovery.rs` relies on being able to produce it.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use mesh_store::RecordJournal;

/// The journal file's name, a sibling of the database as plan §6.2 lays the workspace out.
pub const JOURNAL_FILE_NAME: &str = "records.journal";

/// An append-only record journal on a real filesystem.
#[derive(Debug)]
pub struct FileJournal {
    path: PathBuf,
}

impl FileJournal {
    /// Open — and create — the journal under a workspace root.
    pub fn open(root: impl AsRef<Path>) -> Self {
        Self {
            path: root.as_ref().join(JOURNAL_FILE_NAME),
        }
    }

    /// The file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// How many bytes the journal holds.
    pub fn byte_length(&self) -> u64 {
        std::fs::metadata(&self.path).map_or(0, |data| data.len())
    }

    /// Cut the journal down to `bytes`, the way an interrupted append leaves it.
    ///
    /// # Panics
    ///
    /// If the file cannot be truncated, which means the test cannot run.
    pub fn truncate_to(&self, bytes: u64) {
        OpenOptions::new()
            .write(true)
            .open(&self.path)
            .expect("the journal is writable")
            .set_len(bytes)
            .expect("the journal truncates");
    }

    /// Change one byte in place, the way a damaged sector presents.
    ///
    /// # Panics
    ///
    /// If the byte cannot be written, which means the test cannot run.
    pub fn flip_byte(&self, offset: u64) {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.path)
            .expect("the journal is writable");
        file.seek(SeekFrom::Start(offset))
            .expect("the offset is in the file");
        let mut byte = [0u8; 1];
        file.read_exact(&mut byte).expect("the byte reads");
        byte[0] ^= 0xff;
        file.seek(SeekFrom::Start(offset))
            .expect("the offset is in the file");
        file.write_all(&byte).expect("the byte writes");
        file.sync_all().expect("the change is durable");
    }
}

impl RecordJournal for FileJournal {
    type Error = io::Error;

    fn read_all(&mut self) -> Result<Vec<u8>, Self::Error> {
        match File::open(&self.path) {
            Ok(mut file) => {
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes)?;
                Ok(bytes)
            }
            // A workspace that has never saved anything has no journal, and an empty journal is
            // the correct answer rather than a failure.
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error),
        }
    }

    fn append(&mut self, framed: &[u8]) -> Result<(), Self::Error> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        file.write_all(framed)?;
        file.sync_all()
    }
}
