//! Where bytes live under the workspace root, and why each directory is where it is.
//!
//! Plan §6.2 gives the tree:
//!
//! ```text
//! ~/.mesh/workspaces/<workspace-id>/
//!   metadata.sqlite
//!   objects/
//!   chunks/      <- promoted content, named by digest. Nothing else ever writes here.
//!   scratch/     <- staging. Discarded wholesale at startup; never read as content.
//!   incoming/    <- durable partial transfers, named by the chunk they are building.
//!   mounts/
//!   segments/
//!   logs/        <- the arrival journal lives here.
//! ```
//!
//! # The two directories the plan does not list
//!
//! `quarantine/` is not in plan §6.2's tree. It is here because the task's failure-and-recovery
//! clause requires that a chunk found corrupt be quarantined, re-requested and recorded, and
//! quarantine needs somewhere the bytes survive for diagnosis. The three candidates were all
//! wrong: inside `chunks/` it would still be in the content namespace a collector sweeps; inside
//! `scratch/` it would be deleted by the next startup, which is the one thing evidence must not
//! be; inside `logs/` it would call a megabyte of bad bytes a log. So it is a sibling, and this
//! paragraph is the record that §6.2's tree was extended rather than followed.
//!
//! `incoming/` is the other extension. It cannot be `scratch/`: scratch is deliberately discarded
//! at startup, while a partial network transfer has to survive startup so the requester can resume
//! at its durable length. It cannot be `chunks/` either, because incomplete and unverified bytes
//! are not content. The final digest check and atomic promotion are the only route between them.
//!
//! # Fanout
//!
//! A chunk is at `chunks/<hex[0..2]>/<hex[2..4]>/<hex>`. Two levels of 256 keep any one directory
//! to roughly a millionth of the store, which matters on filesystems whose directory lookup
//! degrades with entry count. The leaf keeps the *full* 64-character digest rather than the
//! remaining 60, so a file found on its own still names itself and a mistaken move between fanout
//! directories cannot silently rename a chunk.

use std::path::{Path, PathBuf};

use crate::digest::Digest32;

/// Promoted, immutable, content-named files. Plan §6.2.
pub const CHUNKS_DIRECTORY_NAME: &str = "chunks";

/// In-flight staging. Plan §6.2. Everything here is discardable by definition.
pub const SCRATCH_DIRECTORY_NAME: &str = "scratch";

/// Durable partial transfers. Unlike promotion scratch, these survive a restart for resumption.
pub const INCOMING_DIRECTORY_NAME: &str = "incoming";

/// Bytes that failed verification, kept for diagnosis. Not in plan §6.2 — see the module docs.
pub const QUARANTINE_DIRECTORY_NAME: &str = "quarantine";

/// Append-only operational records. Plan §6.2.
pub const LOGS_DIRECTORY_NAME: &str = "logs";

/// The arrival journal's file name inside [`LOGS_DIRECTORY_NAME`].
pub const ARRIVAL_JOURNAL_FILE_NAME: &str = "cas-arrivals.log";

/// The arrival journal's rewrite target during compaction, alongside it so the rename is atomic.
pub const ARRIVAL_JOURNAL_REWRITE_FILE_NAME: &str = "cas-arrivals.log.rewrite";

/// The paths of one workspace's content-addressed store.
///
/// Purely a naming calculation: constructing a layout touches no filesystem, so a caller can ask
/// where a chunk *would* live without creating anything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreLayout {
    root: PathBuf,
}

impl StoreLayout {
    /// The layout under a workspace root.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The workspace root this layout is relative to.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The promoted-content directory.
    #[must_use]
    pub fn chunks_directory(&self) -> PathBuf {
        self.root.join(CHUNKS_DIRECTORY_NAME)
    }

    /// The staging directory.
    #[must_use]
    pub fn scratch_directory(&self) -> PathBuf {
        self.root.join(SCRATCH_DIRECTORY_NAME)
    }

    /// Durable partial chunk transfers.
    #[must_use]
    pub fn incoming_directory(&self) -> PathBuf {
        self.root.join(INCOMING_DIRECTORY_NAME)
    }

    /// The durable partial transfer building `digest`.
    #[must_use]
    pub fn incoming_path(&self, digest: &Digest32) -> PathBuf {
        self.incoming_directory()
            .join(format!("{}.part", digest.to_hex()))
    }

    /// The quarantine directory.
    #[must_use]
    pub fn quarantine_directory(&self) -> PathBuf {
        self.root.join(QUARANTINE_DIRECTORY_NAME)
    }

    /// The log directory.
    #[must_use]
    pub fn logs_directory(&self) -> PathBuf {
        self.root.join(LOGS_DIRECTORY_NAME)
    }

    /// The arrival journal.
    #[must_use]
    pub fn arrival_journal(&self) -> PathBuf {
        self.logs_directory().join(ARRIVAL_JOURNAL_FILE_NAME)
    }

    /// The arrival journal's compaction target.
    #[must_use]
    pub fn arrival_journal_rewrite(&self) -> PathBuf {
        self.logs_directory()
            .join(ARRIVAL_JOURNAL_REWRITE_FILE_NAME)
    }

    /// Every directory on the way to a chunk, outermost first: `chunks/`, `chunks/<aa>`,
    /// `chunks/<aa>/<bb>`.
    ///
    /// The whole chain is named here, rather than assembled by whoever needs it, because a
    /// promotion's durability depends on committing the directory entry at *every* level: the entry
    /// naming `<bb>` lives inside `<aa>`, and the entry naming `<aa>` lives inside `chunks/`. A
    /// caller that rebuilt the chain from [`Self::chunk_directory`] would go stale the day the
    /// fanout changes depth, and the failure would be silent — see `DURABILITY.md`.
    #[must_use]
    pub fn chunk_directory_chain(&self, digest: &Digest32) -> Vec<PathBuf> {
        let hex = digest.to_hex();
        let chunks = self.chunks_directory();
        let first = chunks.join(&hex[0..2]);
        let second = first.join(&hex[2..4]);
        vec![chunks, first, second]
    }

    /// The directory a chunk with this digest lives in.
    #[must_use]
    pub fn chunk_directory(&self, digest: &Digest32) -> PathBuf {
        let mut chain = self.chunk_directory_chain(digest);
        // The chain is never empty: it always starts at `chunks/`. The fallback is that directory
        // rather than a panic, for the same reason the rest of this crate prefers an error on a
        // wrong path to a panic in a durability path.
        chain.pop().unwrap_or_else(|| self.chunks_directory())
    }

    /// The file a chunk with this digest lives at.
    #[must_use]
    pub fn chunk_path(&self, digest: &Digest32) -> PathBuf {
        self.chunk_directory(digest).join(digest.to_hex())
    }

    /// The staging path for the `attempt`-th try at promoting this digest from this process.
    ///
    /// The process identifier keeps two live processes apart. The attempt counter resolves the two
    /// collisions it cannot: a *dead* process's leftovers, since a dead process cannot be asked to
    /// give its name back, and two threads of this process staging byte-for-byte identical content
    /// at the same instant, since those two agree on the digest and on the process. The digest is
    /// in the name so an orphan in `scratch/` can be read by a human without hashing it.
    #[must_use]
    pub fn staging_path(&self, digest: &Digest32, process: u32, attempt: u32) -> PathBuf {
        self.scratch_directory()
            .join(format!("{}.{process}.{attempt}.chunk", digest.to_hex()))
    }

    /// The quarantine path for bytes that were stored under `digest` but do not hash to it.
    ///
    /// The attempt counter is there because a chunk can be re-requested, arrive corrupt again, and
    /// be quarantined again; overwriting the first sample would destroy the evidence that the
    /// corruption is reproducible rather than a one-off.
    #[must_use]
    pub fn quarantine_path(&self, digest: &Digest32, attempt: u32) -> PathBuf {
        self.quarantine_directory()
            .join(format!("{}.{attempt}", digest.to_hex()))
    }

    /// The directories the store needs to exist before anything is written.
    #[must_use]
    pub fn directories(&self) -> Vec<PathBuf> {
        vec![
            self.chunks_directory(),
            self.scratch_directory(),
            self.incoming_directory(),
            self.quarantine_directory(),
            self.logs_directory(),
        ]
    }
}
