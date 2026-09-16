//! The store itself: promote, read, quarantine, and find what nothing points at.
//!
//! # The one asymmetry worth knowing before using this type
//!
//! [`Cas::contains`] answers "is there a file at this chunk's name". [`Cas::read`] answers "are
//! those bytes this chunk". They are different questions and the second is the expensive one, so
//! nothing in this crate answers the second by asking the first. A caller that treats `contains`
//! as proof of integrity has reintroduced exactly the failure this crate exists to prevent, which
//! is why `contains` says so in its own documentation rather than only here.

use std::io;
use std::path::PathBuf;

use crate::digest::{Blake3, ContentDigest, Digest32};
use crate::error::CasError;
use crate::fs::{DurableFs, StdFs};
use crate::journal::ArrivalJournal;
use crate::layout::StoreLayout;
use crate::promotion::{staging_path_is_held, Promoted, Promotion};

/// How many quarantine names are tried before the sample is given up on.
const QUARANTINE_ATTEMPTS: u32 = 1024;

/// Who knows whether a chunk is referenced by durable state.
///
/// The store deliberately cannot answer this: references live in manifests and heads, which are
/// `mesh-store`'s tables, and a content-addressed store that believed it knew what pointed at its
/// content would be a second, disagreeing source of truth. The collector supplies the oracle; this
/// crate supplies the candidate set that keeps the oracle from being asked about every chunk that
/// has ever existed.
pub trait ReferenceOracle {
    /// Whether durable state references this chunk. A `true` answer must be conservative: when in
    /// doubt, say referenced, because the cost of a wrong `true` is a retained chunk and the cost
    /// of a wrong `false` is deleted content.
    fn is_referenced(&self, digest: &Digest32) -> bool;
}

impl<T: Fn(&Digest32) -> bool> ReferenceOracle for T {
    fn is_referenced(&self, digest: &Digest32) -> bool {
        self(digest)
    }
}

/// A workspace's content-addressed store.
///
/// Generic over its filesystem and its digest so both are replaceable at a seam: `StdFs` is the
/// only filesystem shipped and [`Blake3`] the only digest, but neither is named anywhere in the
/// promotion or read paths.
#[derive(Debug)]
pub struct Cas<F: DurableFs = StdFs, D: ContentDigest = Blake3> {
    layout: StoreLayout,
    filesystem: F,
    digest_marker: core::marker::PhantomData<D>,
}

impl Cas<StdFs, Blake3> {
    /// Open — and create if missing — the store under a workspace root.
    ///
    /// # Errors
    ///
    /// [`CasError::Io`] if any of the store's directories cannot be created.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, CasError> {
        Self::with_filesystem(root, StdFs)
    }
}

impl<F: DurableFs, D: ContentDigest> Cas<F, D> {
    /// Open the store on a supplied filesystem.
    ///
    /// # Errors
    ///
    /// [`CasError::Io`] if any of the store's directories cannot be created.
    pub fn with_filesystem(root: impl Into<PathBuf>, filesystem: F) -> Result<Self, CasError> {
        let layout = StoreLayout::new(root);
        for directory in layout.directories() {
            filesystem
                .create_dir_all(&directory)
                .map_err(|error| CasError::io("create_dir_all", &directory, error))?;
        }
        Ok(Self {
            layout,
            filesystem,
            digest_marker: core::marker::PhantomData,
        })
    }

    /// Where everything is.
    #[must_use]
    pub fn layout(&self) -> &StoreLayout {
        &self.layout
    }

    /// The filesystem this store runs on.
    #[must_use]
    pub fn filesystem(&self) -> &F {
        &self.filesystem
    }

    /// The arrival journal.
    #[must_use]
    pub fn journal(&self) -> ArrivalJournal<'_, F> {
        ArrivalJournal::new(&self.filesystem, &self.layout)
    }

    /// Begin a promotion, without performing any of it.
    ///
    /// The chunk's name is already known from the returned value's `digest()`, because it is a
    /// function of the bytes and not of anything on disk.
    pub fn begin_promotion(&self, bytes: Vec<u8>) -> Promotion<'_, F, D> {
        Promotion::new(&self.filesystem, &self.layout, bytes)
    }

    /// Begin a promotion of bytes the caller claims hash to `expected`.
    ///
    /// The claim is checked before anything is written, so a caller that has confused two buffers
    /// finds out with an empty `scratch/` rather than after four filesystem operations.
    ///
    /// # Errors
    ///
    /// [`CasError::DigestMismatch`] if the bytes do not hash to `expected`.
    pub fn begin_promotion_expecting(
        &self,
        bytes: Vec<u8>,
        expected: Digest32,
    ) -> Result<Promotion<'_, F, D>, CasError> {
        let found = D::digest_bytes(&bytes);
        if found != expected {
            return Err(CasError::DigestMismatch { expected, found });
        }
        Ok(self.begin_promotion(bytes))
    }

    /// Promote bytes into the store and return the name they now have, with what the promotion did
    /// to get them there.
    ///
    /// # This is not a repair
    ///
    /// A promotion whose digest is already in the store writes nothing and reports
    /// [`AlreadyPresent`](crate::PromotionOutcome::AlreadyPresent). The existing file is matched by
    /// *name*: it is never read, so its bytes are not checked against it and a chunk that has
    /// rotted since it was promoted is left exactly as it is. Replacing bytes takes
    /// [`Self::quarantine`] first — see `DURABILITY.md`, "Repairing a chunk", which is the one
    /// place that sequence is written down.
    ///
    /// The outcome is returned rather than reported by an error because a re-promotion of intact
    /// content is legitimate and common — it is what deduplication looks like — and because
    /// re-hashing the target on every promotion would make the deduplicated path O(chunk) for a
    /// check `Self::read` already performs on the path where it matters.
    ///
    /// # Errors
    ///
    /// Whatever [`Promotion::step`] returns.
    pub fn promote(&self, bytes: Vec<u8>) -> Result<Promoted, CasError> {
        self.begin_promotion(bytes).finish()
    }

    /// Whether a file exists at this chunk's name.
    ///
    /// **This is not an integrity check and must never be used as one.** It answers whether the
    /// chunk was promoted, which is a question about the past; whether the bytes are still the
    /// bytes is a question only [`Self::read`] answers, and only by hashing them.
    #[must_use]
    pub fn contains(&self, digest: &Digest32) -> bool {
        self.filesystem.exists(&self.layout.chunk_path(digest))
    }

    /// Read a chunk, verifying it before a single byte is returned.
    ///
    /// A chunk whose bytes do not hash to its name is quarantined — moved out of the content
    /// namespace by a rename — before this returns, so the same corrupt bytes cannot be served to
    /// a second caller that happens not to check.
    ///
    /// # Errors
    ///
    /// [`CasError::Absent`] if nothing is stored under that name, [`CasError::Corrupt`] if what is
    /// stored is not what the name says (the error carries where it was moved to), or
    /// [`CasError::Io`].
    pub fn read(&self, digest: &Digest32) -> Result<Vec<u8>, CasError> {
        let path = self.layout.chunk_path(digest);
        let bytes = match self.filesystem.read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(CasError::Absent { digest: *digest })
            }
            Err(error) => return Err(CasError::io("read", &path, error)),
        };

        let found = D::digest_bytes(&bytes);
        if found == *digest {
            return Ok(bytes);
        }

        let quarantined = self.quarantine(digest)?;
        Err(CasError::Corrupt {
            digest: *digest,
            found,
            quarantined,
        })
    }

    /// Move a chunk out of the content namespace, keeping the bytes for diagnosis.
    ///
    /// Returns where they went. Called automatically by [`Self::read`] on a verification failure;
    /// public because a peer that reports a chunk bad, or an operator acting on a scrub, has the
    /// same need. The move is a rename, so the chunk stops being readable at its content name at
    /// one instant rather than over an interval.
    ///
    /// **This is step one of the repair sequence**, and the step a caller acting on a scrub or a
    /// peer report has no other way to reach: without it, [`Self::promote`] of the good bytes finds
    /// a file at the name, writes nothing and reports
    /// [`AlreadyPresent`](crate::PromotionOutcome::AlreadyPresent).
    ///
    /// # Errors
    ///
    /// [`CasError::Io`] if the rename or the directory creation fails.
    pub fn quarantine(&self, digest: &Digest32) -> Result<PathBuf, CasError> {
        let directory = self.layout.quarantine_directory();
        self.filesystem
            .create_dir_all(&directory)
            .map_err(|error| CasError::io("create_dir_all", &directory, error))?;

        let source = self.layout.chunk_path(digest);
        // Each sample gets its own name so a chunk that arrives corrupt twice leaves two samples;
        // that is the difference between "a bit flipped once" and "this peer or this disk is
        // producing bad bytes". Past the thousandth sample the last slot is reused, because the
        // evidence has long since been made and unbounded growth would be its own fault.
        let destination = (0..QUARANTINE_ATTEMPTS)
            .map(|attempt| self.layout.quarantine_path(digest, attempt))
            .find(|candidate| !self.filesystem.exists(candidate))
            .unwrap_or_else(|| self.layout.quarantine_path(digest, QUARANTINE_ATTEMPTS - 1));
        self.filesystem
            .rename(&source, &destination)
            .map_err(|error| CasError::io("rename", &source, error))?;
        self.filesystem
            .sync_dir(&directory)
            .map_err(|error| CasError::io("sync_dir", &directory, error))?;
        Ok(destination)
    }

    /// The chunks that have arrived and that the oracle does not know a reference to.
    ///
    /// Reads the arrival journal and nothing else: the store's content directories are never
    /// listed, which is the acceptance criterion this method exists for and which
    /// `tests/unreferenced.rs` checks by counting directory listings rather than by trusting this
    /// sentence.
    ///
    /// # Errors
    ///
    /// Whatever [`ArrivalJournal::candidates`] returns.
    pub fn unreferenced_candidates(
        &self,
        oracle: &impl ReferenceOracle,
    ) -> Result<Vec<Digest32>, CasError> {
        Ok(self
            .journal()
            .candidates()?
            .into_iter()
            .filter(|digest| !oracle.is_referenced(digest))
            .collect())
    }

    /// Every chunk in the store, found by listing every fanout directory.
    ///
    /// **The backstop, not the routine path.** It is O(store) and exists because the arrival
    /// journal is an index and indexes can be lost: if the journal is deleted or truncated,
    /// unreferenced chunks stop being visible to [`Self::unreferenced_candidates`] and only a full
    /// enumeration finds them again. Naming it plainly is the point — a collector that calls this
    /// every run has silently reintroduced the scan the cheap path was built to avoid.
    ///
    /// A file whose name is not 64 hex characters is skipped rather than reported: the content
    /// namespace holds only content-named files, so anything else was not put there by this crate
    /// and this method has no standing to judge it.
    ///
    /// # Errors
    ///
    /// [`CasError::Io`] if a directory cannot be listed.
    pub fn sweep_all_chunks(&self) -> Result<Vec<Digest32>, CasError> {
        let mut found = Vec::new();
        let root = self.layout.chunks_directory();
        for first in self.list(&root)? {
            for second in self.list(&first)? {
                for entry in self.list(&second)? {
                    let Some(name) = entry.file_name().and_then(|name| name.to_str()) else {
                        continue;
                    };
                    if let Ok(digest) = Digest32::parse_hex(name) {
                        found.push(digest);
                    }
                }
            }
        }
        found.sort_unstable();
        Ok(found)
    }

    /// Discard everything staged — plan §6.3's "before step 4: temporary data is discarded".
    ///
    /// Returns how many files were removed. Call this at startup, before the first promotion.
    ///
    /// # What breaking the precondition costs
    ///
    /// **The precondition is that no promotion is in flight**, and it is the caller's to keep.
    /// Breaking it does not merely lose a writer's staging file and fail that writer's promotion.
    /// A staging name is a pure function of digest, process and attempt
    /// ([`StoreLayout::staging_path`](crate::StoreLayout::staging_path)), so a name freed here is
    /// the *first* name the next promotion of the same content picks. Five steps, no crash and no
    /// filesystem fault (`01KZDSHZAXQ223DB2GAV9GEVKM`):
    ///
    /// 1. Promotion A stages, flushes, verifies and records its arrival at
    ///    `scratch/<hex>.<pid>.0.chunk`. Everything this crate does to earn the rename is done.
    /// 2. The staging file is removed and its name is free.
    /// 3. A second writer stages the same digest, is granted attempt zero — A's name — and is
    ///    part-way through writing it.
    /// 4. A performs [`Link`](crate::PromotionStep::Link), renaming *the path it remembers*. That
    ///    path now holds the second writer's partially written file.
    /// 5. [`Self::contains`] is true and [`Self::read`] returns [`CasError::Corrupt`].
    ///
    /// So the cost of breaking the precondition is an unverified chunk standing at a verified
    /// name — the one outcome this crate exists to make impossible — reached through a promotion
    /// that reported success. It is not a failed promotion.
    ///
    /// # What this therefore does not remove
    ///
    /// A file that a live [`Promotion`] **in this process** holds is skipped, and is not counted in
    /// the return value. That makes the sequence above unreachable within one process however many
    /// `Cas` handles, threads or promotions are running: the name is not free until the promotion
    /// holding it has renamed the file away or removed it. The guard consults neither a clock nor a
    /// process identifier, so a reused pid cannot defeat it.
    ///
    /// # What is still the caller's
    ///
    /// A promotion in a **second process** holds nothing this one can see, and step 2 above is then
    /// reachable exactly as written. Plan §6.1 specifies a controlled single writer per workspace,
    /// which is what makes that keepable; a multi-writer store needs a cross-process lock here, and
    /// this paragraph is where that obligation is recorded. `DURABILITY.md` assumption 4 states the
    /// same cost, and `tests/scratch_discard.rs` reproduces both halves.
    ///
    /// # Errors
    ///
    /// [`CasError::Io`] if the scratch directory cannot be listed or an entry cannot be removed.
    pub fn discard_scratch(&self) -> Result<usize, CasError> {
        let scratch = self.layout.scratch_directory();
        let mut removed = 0;
        for entry in self.list(&scratch)? {
            // A held name is not temporary data. The file at it has been staged, flushed and
            // verified, and a live promotion is about to rename it; freeing the name is what
            // publishes the next writer's half-written bytes under it.
            if staging_path_is_held(&entry) {
                continue;
            }
            match self.filesystem.remove_file(&entry) {
                Ok(()) => removed += 1,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(CasError::io("remove_file", &entry, error)),
            }
        }
        Ok(removed)
    }

    fn list(&self, path: &std::path::Path) -> Result<Vec<PathBuf>, CasError> {
        self.filesystem
            .list_dir(path)
            .map_err(|error| CasError::io("list_dir", path, error))
    }
}
