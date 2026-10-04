//! The one thing in this repository that can delete a chunk.
//!
//! Before this module there was none: every `remove_file` in `mesh-cas` removed a *staging* file,
//! so `chunks/` grew for the life of a workspace by construction. Plan §6.4 asks for collection and
//! plan §2.5 asks that nothing valid be discarded, and those two are in tension exactly here.
//!
//! # The three checks a byte passes before it stops existing
//!
//! 1. **The plan.** `mesh-store`'s `CollectionPlan` computes the doomed set from the retained-root
//!    closure. That is the reachability argument, and it lives next to the index because that is
//!    where references are.
//! 2. **The oracle, at the instant of deletion.** [`Cas::collect`] *requires* a
//!    [`ReferenceOracle`](crate::ReferenceOracle) and asks it about every digest again, immediately
//!    before removing the file. The signature is the point: there is no way to reach the deleter
//!    without supplying something that can veto it, so a caller who computed the wrong list, or who
//!    computed the right list and then let references change before the check, can still veto
//!    referenced content. Writer exclusion must keep that answer valid through unlink. A veto is [`Collected::refused`] — reported, never an error, because a
//!    reference appearing between the plan and the delete is normal.
//! 3. **The name.** Only `chunks/<aa>/<bb>/<64 hex>` is ever unlinked. Nothing else in the store is
//!    reachable from this module: quarantine keeps evidence, `scratch/` is
//!    [`Cas::discard_scratch`]'s, and `logs/` is rewritten but never removed.
//!
//! # The dry run is the same computation
//!
//! [`CollectionMode::DryRun`] takes the identical path and stops one call short of `remove_file`.
//! It measures the same bytes with the same `stat`, consults the same oracle, and produces a
//! [`Collected`] of the same shape. There is no second code path that could describe a program
//! nobody runs — and, equally, a dry run that reports nothing is a report that nothing is
//! collectable, which `tests/collection-footprint.rs` is there to contradict with a number.
//!
//! # Writer coordination is the caller's responsibility
//!
//! `collect` takes no writer lock. Its concurrency test demonstrates progress for a promotion of
//! a different digest; it does not prove an atomic reference-check/unlink for the same digest.
//! The caller must exclude reference commits and same-digest promotion through deletion and
//! journal rewriting. An oracle answer can become stale between the check and unlink otherwise.
//! The native daemon composes workspace custody for its explicit orphan-cleanup operation.

use std::collections::BTreeSet;
use std::io;

use crate::digest::{Blake3, ContentDigest, Digest32};
use crate::error::CasError;
use crate::fs::{DurableFs, StdFs};
use crate::store::{Cas, ReferenceOracle};

/// Whether a collection reports or acts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CollectionMode {
    /// Measure and report; unlink nothing. The default, so a caller that forgets to say which it
    /// wants gets the harmless one.
    #[default]
    DryRun,
    /// Unlink the chunks.
    Delete,
}

impl CollectionMode {
    /// Whether this mode removes anything.
    #[must_use]
    pub const fn deletes(self) -> bool {
        matches!(self, Self::Delete)
    }
}

/// What a collection did, or — in [`CollectionMode::DryRun`] — would have done.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Collected {
    mode: CollectionMode,
    collected: Vec<Digest32>,
    absent: Vec<Digest32>,
    refused: Vec<Digest32>,
    bytes: u64,
}

impl Collected {
    /// Which mode produced this.
    #[must_use]
    pub const fn mode(&self) -> CollectionMode {
        self.mode
    }

    /// The chunks removed — or, in a dry run, exactly the chunks that would have been.
    #[must_use]
    pub fn collected(&self) -> &[Digest32] {
        &self.collected
    }

    /// Doomed chunks the store did not hold. Normal: a candidate list can name a chunk a previous
    /// collection already removed, or one that never finished arriving.
    #[must_use]
    pub fn absent(&self) -> &[Digest32] {
        &self.absent
    }

    /// Doomed chunks the oracle vetoed at the instant of deletion.
    ///
    /// **A non-empty list here is worth looking at.** It means the caller's plan and the oracle
    /// disagreed, which is either a workspace that moved during the collection — benign — or a
    /// plan computed against the wrong retained-root set, which is the shape of the bug that loses
    /// data. Nothing was deleted either way.
    #[must_use]
    pub fn refused(&self) -> &[Digest32] {
        &self.refused
    }

    /// Bytes freed, or in a dry run the bytes that would be freed.
    #[must_use]
    pub const fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Whether this collection freed nothing.
    #[must_use]
    pub fn freed_nothing(&self) -> bool {
        self.bytes == 0
    }
}

impl<F: DurableFs, D: ContentDigest> Cas<F, D> {
    /// Remove the doomed chunks, having asked the oracle about each one first.
    ///
    /// The oracle is not optional and not a courtesy: it is the second of the two independent
    /// reachability checks that stand between a plan and a missing file, and it runs *here*, at the
    /// last moment, rather than wherever the plan was computed.
    ///
    /// Deletion order is the caller's list order; the directory of each removed chunk is synced
    /// once at the end of the run rather than once per file, because an interruption in the middle
    /// leaves some chunks removed and some not, which is a state the store already tolerates — a
    /// collected chunk is one nothing references, so a half-finished collection is a smaller
    /// collection and never a broken store.
    ///
    /// # Errors
    ///
    /// [`CasError::Io`] if a chunk cannot be removed, a directory cannot be synced, or the arrival
    /// journal cannot be rewritten. A `NotFound` on the chunk itself is not an error — it is
    /// [`Collected::absent`].
    pub fn collect(
        &self,
        doomed: &[Digest32],
        oracle: &impl ReferenceOracle,
        mode: CollectionMode,
    ) -> Result<Collected, CasError> {
        let mut collected = Vec::new();
        let mut absent = Vec::new();
        let mut refused = Vec::new();
        let mut bytes = 0u64;
        let mut directories: BTreeSet<std::path::PathBuf> = BTreeSet::new();
        let mut seen: BTreeSet<Digest32> = BTreeSet::new();

        for digest in doomed {
            if !seen.insert(*digest) {
                continue;
            }
            if oracle.is_referenced(digest) {
                refused.push(*digest);
                continue;
            }
            let path = self.layout().chunk_path(digest);
            let length = match self.filesystem().file_len(&path) {
                Ok(length) => length,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    absent.push(*digest);
                    continue;
                }
                Err(error) => return Err(CasError::io("file_len", &path, error)),
            };

            if mode.deletes() {
                match self.filesystem().remove_file(&path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        absent.push(*digest);
                        continue;
                    }
                    Err(error) => return Err(CasError::io("remove_file", &path, error)),
                }
                directories.insert(self.layout().chunk_directory(digest));
            }
            collected.push(*digest);
            bytes = bytes.saturating_add(length);
        }

        if mode.deletes() {
            for directory in &directories {
                self.filesystem()
                    .sync_dir(directory)
                    .map_err(|error| CasError::io("sync_dir", directory, error))?;
            }
            if !collected.is_empty() {
                // Delete first, forget second — see `ArrivalJournal::forget` for why that order and
                // not the other one.
                self.journal()
                    .forget(&collected.iter().copied().collect())?;
            }
        }

        Ok(Collected {
            mode,
            collected,
            absent,
            refused,
            bytes,
        })
    }
}

impl Cas<StdFs, Blake3> {
    /// The bytes every chunk in the store occupies, found by sweeping.
    ///
    /// Present so that "collection freed disk" is a *measurement* rather than an inference from the
    /// collector's own arithmetic. `tests/collection-footprint.rs` reads this before and after a
    /// real collection and checks the difference against [`Collected::bytes`]; two numbers from two
    /// sources agreeing is evidence, one number agreeing with itself is not.
    ///
    /// O(store), like [`Cas::sweep_all_chunks`] it is built on, and named for a measurement rather
    /// than for a routine path for the same reason.
    ///
    /// # Errors
    ///
    /// Whatever [`Cas::sweep_all_chunks`] returns, or [`CasError::Io`] if a chunk cannot be
    /// measured.
    pub fn chunk_bytes_on_disk(&self) -> Result<u64, CasError> {
        let mut total = 0u64;
        for digest in self.sweep_all_chunks()? {
            let path = self.layout().chunk_path(&digest);
            match self.filesystem().file_len(&path) {
                Ok(length) => total = total.saturating_add(length),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(CasError::io("file_len", &path, error)),
            }
        }
        Ok(total)
    }
}
