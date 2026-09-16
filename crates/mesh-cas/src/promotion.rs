//! Promotion: plan §6.3 steps 1–4, expressed as six steps a crash can land between any two of.
//!
//! # Why this is a state machine and not a function
//!
//! "Atomic" is a claim about what every interruption leaves behind, so the interruption points
//! have to be *nameable* for a test to enumerate them. [`PromotionStep`] is that enumeration, and
//! [`Promotion::step`] performs exactly one, which is what lets `tests/crash-promotion.rs` drive a
//! real child process to a chosen point and have it killed there.
//!
//! It is not a test hook bolted onto a function. Running the steps one at a time is the only
//! implementation; [`Promotion::finish`] is a loop over [`Promotion::step`], so the sequence the
//! tests interrupt is the sequence production runs.
//!
//! # The order, and what each step buys
//!
//! | Step | Plan §6.3 | Why it is where it is |
//! |---|---|---|
//! | [`Stage`](PromotionStep::Stage) | 1 | Bytes land in `scratch/`, never at their final name, so nothing incomplete is ever addressable. |
//! | [`Flush`](PromotionStep::Flush) | 2 | Data reaches durable storage **before** any name reveals it. Reversing this and the rename is the bug that produces a present-but-empty chunk after a power loss. |
//! | [`Verify`](PromotionStep::Verify) | 3 | The staged file is read back **from disk** and hashed. Hashing the in-memory buffer would only prove the caller is consistent with itself; reading back is what makes a write that landed wrong fail here rather than at some later read. |
//! | [`RecordArrival`](PromotionStep::RecordArrival) | — | Durably notes the chunk before it exists, so a crash after the rename cannot produce a chunk the collector never hears about. Skipped, and only skipped, when the chunk is already in the store and therefore already recorded. See [`crate::journal`]. |
//! | [`Link`](PromotionStep::Link) | 4 | Commits every entry on the path to the chunk — creating each missing fanout directory and `fsync`ing its parent, present or not — then renames. The rename is the instant the chunk becomes visible, and it is atomic or the platform is unsupported. |
//! | [`SyncDirectory`](PromotionStep::SyncDirectory) | 4 | `fsync` of the chunk's own directory, so the entry the rename created survives a power loss rather than only a process death. Runs on every path, including the deduplicated one. |
//!
//! # Committing a *path*, not a directory entry
//!
//! A chunk is reachable only if every entry on `chunks/` → `<aa>` → `<bb>` → `<digest>` is durable.
//! Syncing the leaf commits the last of those four and nothing else, so a promotion that created
//! `<aa>` and `<bb>` used to return `Ok` over two entries that no `fsync` had ever touched
//! (`01KZE8JDBVPQ97MVMD9NFKVB9T`). [`Promotion::perform_link`] therefore walks the levels one at a
//! time, creating each one that is missing and syncing each one's *parent*, before the rename — so
//! by the time the chunk is visible, the path to it is already committed.
//!
//! It commits a level **it did not create**, too. Skipping those was cheaper and rested on an
//! assumption about a predecessor: that whoever created the directory got as far as syncing its
//! parent. `sync_dir` can fail, so this crate reaches the state where that is false without any
//! crash — and the later promotions that skip the existing directory are exactly the ones that
//! would have repaired it (`01KZEBAEK05P3XB9A788TYV4QM`). What replaces minimality is a bound: the
//! syncs are the three directories on this chunk's own path, once each, whatever else is in the
//! store. What it costs is in `DURABILITY.md`, in microseconds.
//!
//! # What "either the old state or the new one" means at each point
//!
//! Before `Link`, the store does not contain the chunk; the only residue is a file in `scratch/`,
//! which is not content, is not addressable, and is discarded by [`crate::Cas::discard_scratch`].
//! From `Link` onward the store contains the chunk, whole and verified. There is no interval in
//! which the chunk is partly present, because the only operation that creates the chunk's name is
//! a rename of a file that was already complete, already durable and already verified.
//!
//! # The staged file has to still be the staged file at `Link`
//!
//! That last sentence is only true while the path `Link` renames still names the bytes `Verify`
//! read. A staging name is a pure function of digest, process and attempt
//! ([`StoreLayout::staging_path`]), so a name that is *freed* is the first name the next promotion
//! of the same content picks — and a promotion that renames a name rather than a file it still
//! holds will happily publish somebody else's half-written bytes under a verified name
//! (`01KZDSHZAXQ223DB2GAV9GEVKM`). [`crate::Cas::discard_scratch`] is the one thing in this crate
//! that can free such a name, so [`HELD_STAGING_PATHS`] records every staging path a live
//! `Promotion` holds and `discard_scratch` skips those. The name is never given back until the
//! file has been renamed away or removed, which makes the sequence unreachable in this process
//! rather than merely unlikely.
//!
//! Re-reading and re-hashing the staged file immediately before the rename was considered and
//! **rejected**: it costs an O(chunk) read on the promotion hot path — up to plan §6.2's 1 MiB —
//! and it does not close the hole, because the substitution can happen between the re-hash and the
//! `rename`. A check that narrows a window is not the same as a name that is never free, and only
//! the second one is a guarantee. The residue the registry cannot cover is a *second process*,
//! which is plan §6.1's controlled-single-writer precondition and is stated as such in
//! `DURABILITY.md` assumption 4.
//!
//! The one thing this crate cannot do for the caller is decide when the *reference* becomes
//! durable; that is plan §6.3 steps 5–9 and belongs to `mesh-store`. Between `Link` and that
//! commit the chunk is present and unreferenced, which is exactly the state the arrival journal
//! exists to make findable.
//!
//! # A promotion that finds the chunk already there is not a repair
//!
//! [`Link`](PromotionStep::Link) treats a file already standing at the chunk's name as the same
//! content and writes nothing. That is right under content addressing and wrong under bit rot,
//! where a file's name and its bytes have stopped agreeing — so a caller re-promoting known-good
//! bytes over a rotted chunk would be told `Ok` for a repair that did not happen
//! (`01KZDSJVMT1H82594HTDM63VEJ`). The answer is not to re-hash the target on every promotion, which
//! would make the deduplicated path O(chunk); it is that the outcome says which happened.
//! [`PromotionOutcome::AlreadyPresent`] means *this promotion wrote no content*, and
//! `DURABILITY.md` states the repair contract it implies: quarantine first, then promote.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::digest::{ContentDigest, Digest32};
use crate::error::CasError;
use crate::fs::DurableFs;
use crate::journal::ArrivalJournal;
use crate::layout::StoreLayout;

/// How many staging names are tried before giving up. Only ever more than one when a crashed
/// process with this process's identifier left files behind.
const STAGING_ATTEMPTS: u32 = 64;

/// The staging paths that live [`Promotion`]s in this process hold.
///
/// # Why this is process-wide and not a field of the store
///
/// The thing being protected is a *path*, and a path is process-wide: two [`crate::Cas`] handles
/// opened on one root are two handles onto one `scratch/` directory, so a set hanging off one of
/// them would let the other free a name the first is still using. Keying on the path makes the
/// guard as wide as the resource.
///
/// # What it is not
///
/// It is not a lock, it is not durable, and it says nothing about any other process. A promotion
/// running in a second process holds nothing here and is invisible to this one — which is plan
/// §6.1's controlled-single-writer precondition, recorded in `DURABILITY.md` assumption 4 rather
/// than papered over here. Nor is it a liveness heuristic: membership is written by a live
/// `Promotion` and erased by that same promotion's last use of the name, so a reused process
/// identifier cannot defeat it and no clock is consulted.
///
/// The one way to leak an entry is to `mem::forget` a `Promotion`, which leaks the staged file
/// too; the cost is one undeletable file in `scratch/` until the process exits, never a published
/// chunk.
static HELD_STAGING_PATHS: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

/// The held-path set, recovering rather than propagating a poisoned lock.
///
/// A `BTreeSet<PathBuf>` has no invariant a panic elsewhere could have broken, and the cost of
/// refusing to answer is that [`crate::Cas::discard_scratch`] deletes a file a live promotion
/// holds — the exact failure this set exists to prevent. So the poison is stepped over.
fn held_staging_paths() -> MutexGuard<'static, BTreeSet<PathBuf>> {
    HELD_STAGING_PATHS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// Claim a staging path for a live promotion.
fn hold_staging_path(path: &Path) {
    held_staging_paths().insert(path.to_path_buf());
}

/// Give a staging path back, after the file at it has been renamed away or removed.
fn release_staging_path(path: &Path) {
    held_staging_paths().remove(path);
}

/// Whether a live promotion in this process holds this staging path.
///
/// [`crate::Cas::discard_scratch`] asks before it removes anything: a held path is a file that has
/// been staged, flushed and verified and that a promotion is about to rename, not the temporary
/// data plan §6.3 says to discard.
pub(crate) fn staging_path_is_held(path: &Path) -> bool {
    held_staging_paths().contains(path)
}

/// What a promotion did to the store.
///
/// The distinction exists because "the store holds this digest" and "this promotion put it there"
/// are different facts, and only the second one means the bytes on disk are the bytes the caller
/// handed over. A caller repairing a chunk it has been told is bad needs the second; a caller
/// filling a cache needs neither and can ignore this.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PromotionOutcome {
    /// This promotion staged, flushed, verified and renamed the bytes into the content namespace.
    /// The file at the chunk's name is the file this promotion wrote.
    Linked,
    /// A file already stood at the chunk's name, so this promotion wrote no content and removed its
    /// staged copy instead.
    ///
    /// **This is not a statement about the bytes on disk.** The existing file was matched by
    /// *name*, never re-read, so a chunk whose bytes have rotted since it was promoted produces
    /// this outcome exactly as an intact one does. Re-promoting good bytes over a rotted chunk
    /// therefore repairs nothing — see `DURABILITY.md`, "Repairing a chunk", for the sequence that
    /// does.
    AlreadyPresent,
}

impl PromotionOutcome {
    /// Whether this promotion is the reason the chunk is in the store.
    ///
    /// `false` means the store's copy predates this call and has not been read, let alone verified,
    /// by it.
    #[must_use]
    pub fn wrote_content(self) -> bool {
        matches!(self, Self::Linked)
    }
}

/// A completed promotion: the chunk's name, and what the promotion did to get it there.
///
/// Returned by [`crate::Cas::promote`] instead of a bare digest so that "the store holds these
/// bytes now" and "the store already held something under this name" cannot be reported by the same
/// value. That collapse is what let a failed repair read as a successful one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Promoted {
    digest: Digest32,
    outcome: PromotionOutcome,
}

impl Promoted {
    /// The chunk's content-addressed name.
    #[must_use]
    pub fn digest(&self) -> Digest32 {
        self.digest
    }

    /// What the promotion did to the store.
    #[must_use]
    pub fn outcome(&self) -> PromotionOutcome {
        self.outcome
    }

    /// Whether this promotion is the reason the chunk is in the store.
    #[must_use]
    pub fn wrote_content(&self) -> bool {
        self.outcome.wrote_content()
    }
}

/// One interruptible step of a promotion.
///
/// The order is [`PromotionStep::ORDER`]; each variant's rationale is in the module docs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PromotionStep {
    /// Write the bytes into `scratch/` under a name no reader looks for.
    Stage,
    /// Force the staged bytes to durable storage.
    Flush,
    /// Read the staged file back and check it hashes to the name it will be promoted under.
    Verify,
    /// Durably record, before the chunk exists, that it is about to.
    RecordArrival,
    /// Rename the staged file to its content-addressed name. The chunk becomes visible here.
    Link,
    /// Force the new directory entry to durable storage.
    SyncDirectory,
}

impl PromotionStep {
    /// Every step, in the order they are performed.
    pub const ORDER: [Self; 6] = [
        Self::Stage,
        Self::Flush,
        Self::Verify,
        Self::RecordArrival,
        Self::Link,
        Self::SyncDirectory,
    ];

    /// The step after this one, or `None` for the last.
    #[must_use]
    pub fn next(self) -> Option<Self> {
        let position = Self::ORDER.iter().position(|step| *step == self)?;
        Self::ORDER.get(position + 1).copied()
    }

    /// The plan §6.3 step this belongs to, or `None` for a step the plan does not number.
    ///
    /// Plan §6.3's numbering is coarser than this state machine: step 4 — *"atomically promote
    /// chunks into CAS"* — is two operations here, because the rename and the `fsync` of the
    /// directory it wrote into are separately interruptible and leave different residues.
    /// [`RecordArrival`](Self::RecordArrival) has no number at all: it is this crate's addition,
    /// and the reason it is not a renumbering of the plan is that it changes nothing about what a
    /// crash leaves behind — it only makes what a crash leaves behind *findable*.
    ///
    /// The mapping exists so that the sequence which owns steps 5 to 11 can be held against this
    /// one. `mesh-store` may not import this crate — no dependency edge is permitted, see
    /// `docs/adr/0008-ship-the-sqlite-schema-without-a-sqlite-driver.md` — so the two are bound by
    /// `tests/sequence_steps_agree_with_mesh_store.rs` reading both sources, exactly as
    /// `tests/blake3_agrees_with_mesh_types.rs` binds the hasher.
    #[must_use]
    pub const fn plan_step(self) -> Option<u8> {
        match self {
            Self::Stage => Some(1),
            Self::Flush => Some(2),
            Self::Verify => Some(3),
            Self::RecordArrival => None,
            Self::Link | Self::SyncDirectory => Some(4),
        }
    }

    /// The step's name, for logs and for the crash harness's argument parsing.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Stage => "stage",
            Self::Flush => "flush",
            Self::Verify => "verify",
            Self::RecordArrival => "record-arrival",
            Self::Link => "link",
            Self::SyncDirectory => "sync-directory",
        }
    }

    /// The step with this name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ORDER.into_iter().find(|step| step.name() == name)
    }
}

/// A promotion in progress.
///
/// Dropping one that has not reached [`PromotionStep::Link`] removes the staged file, so an
/// ordinary error path leaves no litter. A *crash* runs no destructor, which is why
/// [`crate::Cas::discard_scratch`] exists and is not optional at startup.
#[derive(Debug)]
pub struct Promotion<'a, F: DurableFs, D: ContentDigest> {
    filesystem: &'a F,
    layout: &'a StoreLayout,
    bytes: Vec<u8>,
    digest: Digest32,
    staged: Option<PathBuf>,
    next: Option<PromotionStep>,
    outcome: Option<PromotionOutcome>,
    arrival_recorded: bool,
    digest_marker: core::marker::PhantomData<D>,
}

impl<'a, F: DurableFs, D: ContentDigest> Promotion<'a, F, D> {
    pub(crate) fn new(filesystem: &'a F, layout: &'a StoreLayout, bytes: Vec<u8>) -> Self {
        let digest = D::digest_bytes(&bytes);
        Self {
            filesystem,
            layout,
            bytes,
            digest,
            staged: None,
            next: Some(PromotionStep::Stage),
            outcome: None,
            arrival_recorded: false,
            digest_marker: core::marker::PhantomData,
        }
    }

    /// The name this chunk will have. Known before anything is written, because it is a function of
    /// the bytes alone.
    #[must_use]
    pub fn digest(&self) -> Digest32 {
        self.digest
    }

    /// The step that has not been performed yet, or `None` when the promotion is complete.
    #[must_use]
    pub fn next_step(&self) -> Option<PromotionStep> {
        self.next
    }

    /// Where the bytes are staged, once [`PromotionStep::Stage`] has run.
    #[must_use]
    pub fn staged_path(&self) -> Option<&Path> {
        self.staged.as_deref()
    }

    /// Whether the store already held this chunk, making [`PromotionStep::Link`] a no-op.
    ///
    /// Only meaningful after `Link` has run.
    #[must_use]
    pub fn was_already_present(&self) -> bool {
        self.outcome == Some(PromotionOutcome::AlreadyPresent)
    }

    /// Whether this promotion appended an arrival record.
    ///
    /// `false` after [`PromotionStep::RecordArrival`] means the chunk was already in the store, so
    /// the promotion that made it visible recorded it and a second record would cost a journal line
    /// per attempt rather than per chunk.
    #[must_use]
    pub fn recorded_arrival(&self) -> bool {
        self.arrival_recorded
    }

    /// What the promotion did to the store, or `None` while [`PromotionStep::Link`] has yet to
    /// decide.
    ///
    /// Recorded by `Link` rather than inferred from how far the promotion got, so a promotion that
    /// ended early — verification failed, the filesystem refused — reports `None` instead of
    /// claiming an outcome it never reached.
    #[must_use]
    pub fn outcome(&self) -> Option<PromotionOutcome> {
        self.outcome
    }

    /// Perform the next step, returning which one was performed.
    ///
    /// # Errors
    ///
    /// [`CasError::StagedVerificationFailed`] if the bytes on disk do not hash to the chunk's name
    /// — in which case the staged file has been removed and the promotion is over — or
    /// [`CasError::Io`] for any filesystem failure.
    pub fn step(&mut self) -> Result<Option<PromotionStep>, CasError> {
        let Some(step) = self.next else {
            return Ok(None);
        };
        match step {
            PromotionStep::Stage => self.perform_stage()?,
            PromotionStep::Flush => self.perform_flush()?,
            PromotionStep::Verify => self.perform_verify()?,
            PromotionStep::RecordArrival => self.perform_record_arrival()?,
            PromotionStep::Link => self.perform_link()?,
            PromotionStep::SyncDirectory => self.perform_sync_directory()?,
        }
        self.next = step.next();
        Ok(Some(step))
    }

    /// Perform steps until `last` has been performed, then stop.
    ///
    /// This is what the crash harness drives; nothing else in the crate uses it, and nothing about
    /// it is test-only — it is [`Self::finish`] with a different stopping condition.
    ///
    /// # Errors
    ///
    /// Whatever [`Self::step`] returns.
    pub fn run_through(&mut self, last: PromotionStep) -> Result<(), CasError> {
        while let Some(next) = self.next {
            self.step()?;
            if next == last {
                return Ok(());
            }
        }
        Ok(())
    }

    /// Perform every remaining step and return the chunk's name with what the promotion did to
    /// get it there.
    ///
    /// # Errors
    ///
    /// Whatever [`Self::step`] returns.
    pub fn finish(mut self) -> Result<Promoted, CasError> {
        while self.next.is_some() {
            self.step()?;
        }
        Ok(Promoted {
            digest: self.digest,
            // A promotion that ran to completion has been past `Link`, which is the only writer of
            // this field. `Linked` is the fallback rather than a panic because a completed
            // promotion whose steps all returned `Ok` did put the chunk there.
            outcome: self.outcome.unwrap_or(PromotionOutcome::Linked),
        })
    }

    fn perform_stage(&mut self) -> Result<(), CasError> {
        let scratch = self.layout.scratch_directory();
        self.filesystem
            .create_dir_all(&scratch)
            .map_err(|error| CasError::io("create_dir_all", &scratch, error))?;

        let process = std::process::id();
        let mut last = self.layout.staging_path(&self.digest, process, 0);
        for attempt in 0..STAGING_ATTEMPTS {
            let path = self.layout.staging_path(&self.digest, process, attempt);
            match self.filesystem.stage(&path, &self.bytes) {
                Ok(()) => {
                    // Claimed before it is recorded on `self`, so there is no instant at which
                    // this promotion owns a file whose name `discard_scratch` believes is free.
                    hold_staging_path(&path);
                    self.staged = Some(path);
                    return Ok(());
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    last = path;
                }
                Err(error) => return Err(CasError::io("stage", &path, error)),
            }
        }
        Err(CasError::StagingNamesExhausted {
            path: last,
            attempts: STAGING_ATTEMPTS,
        })
    }

    fn perform_flush(&mut self) -> Result<(), CasError> {
        let path = self.staged_or_unreachable();
        self.filesystem
            .sync_file(&path)
            .map_err(|error| CasError::io("sync_file", &path, error))
    }

    fn perform_verify(&mut self) -> Result<(), CasError> {
        let path = self.staged_or_unreachable();
        let written = self
            .filesystem
            .read(&path)
            .map_err(|error| CasError::io("read", &path, error))?;
        let found = D::digest_bytes(&written);
        if found == self.digest {
            return Ok(());
        }
        // The staged file is removed here rather than left for `discard_scratch`, because bytes
        // that failed verification must not sit in the store under any name for any length of
        // time. A failure to remove them is not allowed to mask the verification failure, so the
        // removal is best-effort and the verification error is what the caller sees.
        let _ = self.filesystem.remove_file(&path);
        self.release_staged();
        self.next = None;
        Err(CasError::StagedVerificationFailed {
            expected: self.digest,
            found,
            staged: path,
        })
    }

    /// Step 4. Durably note the chunk before it exists — unless it already does.
    ///
    /// The record exists so that a chunk which becomes visible is always findable by the collector.
    /// A chunk that is *already* in the store became visible under some earlier promotion, and that
    /// promotion recorded it; a second record adds nothing and costs 67 bytes per promotion
    /// *attempt* rather than per chunk, which is affine in the actor count for content that
    /// deduplicates perfectly (`01KZE8KYZGGCD732XZHZSFT9B6`). A `+` appended after the caller's `-`
    /// would also put an already-referenced chunk back into the candidate set.
    ///
    /// The existence check is a `stat`, not a read, and it is not load-bearing: [`Self::perform_link`]
    /// asks the same question again for itself and records the arrival there if this step declined
    /// to. So the invariant is not "step 4 records" but "no rename is ever performed before the
    /// record is durable", which holds on every path through this type.
    fn perform_record_arrival(&mut self) -> Result<(), CasError> {
        if self
            .filesystem
            .exists(&self.layout.chunk_path(&self.digest))
        {
            return Ok(());
        }
        self.record_arrival()
    }

    fn record_arrival(&mut self) -> Result<(), CasError> {
        ArrivalJournal::new(self.filesystem, self.layout).record_arrival(&self.digest)?;
        self.arrival_recorded = true;
        Ok(())
    }

    fn perform_link(&mut self) -> Result<(), CasError> {
        let staged = self.staged_or_unreachable();
        let target = self.layout.chunk_path(&self.digest);

        // Before the chunk's own name is looked at, and therefore on the deduplicated path too.
        // A chunk that is already present is standing in a `<bb>` whose entry in `<aa>` may never
        // have been committed by anybody — the same reasoning that made
        // [`Self::perform_sync_directory`] unconditional, one level up the path
        // (`01KZEBAEK05P3XB9A788TYV4QM`). Directory creation is idempotent and reveals no content,
        // so running it first costs the linked path nothing and moves no visibility earlier.
        self.create_chunk_directory()?;

        if self.filesystem.exists(&target) {
            // Content addressing makes this a no-op rather than a conflict: the existing file is
            // named by the same digest as the bytes just verified. The staged copy is removed and
            // the existing chunk is left alone, so a re-promotion never disturbs a chunk something
            // may already be reading.
            //
            // The existing file is matched by name and never re-read, so this says nothing about
            // whether its bytes still hash to it. That is why the promotion reports
            // `AlreadyPresent` rather than an indistinguishable success: re-promoting good bytes
            // over a rotted chunk repairs nothing, and the caller has to be told
            // (`01KZDSJVMT1H82594HTDM63VEJ`).
            self.outcome = Some(PromotionOutcome::AlreadyPresent);
            self.filesystem
                .remove_file(&staged)
                .map_err(|error| CasError::io("remove_file", &staged, error))?;
            self.release_staged();
            return Ok(());
        }

        // Step 4 declines to record an arrival for a chunk that is already in the store. If the
        // chunk has stopped being in the store between that check and this one, the record has to
        // happen now: what must never happen is a rename that no durable record precedes.
        if !self.arrival_recorded {
            self.record_arrival()?;
        }

        self.filesystem
            .rename(&staged, &target)
            .map_err(|error| CasError::io("rename", &staged, error))?;
        // Released only once the rename has succeeded. Giving the name back any earlier would open
        // the window this registry exists to close, between the last use of the name and the last
        // moment it must not be reused.
        self.release_staged();
        self.outcome = Some(PromotionOutcome::Linked);
        Ok(())
    }

    /// Create every missing directory on the way to the chunk, and commit every entry on that path
    /// — whether or not this promotion is the one that created it.
    ///
    /// `create_dir_all(chunks/aa/bb)` followed by one `fsync` of the leaf commits the chunk's entry
    /// and nothing else: the entry naming `bb` lives in `aa` and the entry naming `aa` lives in
    /// `chunks/`, and POSIX makes neither durable until that directory is synced. A power loss after
    /// `promote` returned could therefore leave the chunk's inode unreachable — the exact failure
    /// the atomicity guarantee exists to prevent (`01KZE8JDBVPQ97MVMD9NFKVB9T`).
    ///
    /// Two properties are deliberate:
    ///
    /// * **Each parent is synced before the rename, not after.** By the instant the chunk becomes
    ///   visible, the whole path to it is already committed — the same reason the data is flushed
    ///   before the name that reveals it exists.
    /// * **A directory that already exists is committed anyway.** This used to skip it, on the
    ///   reasoning that whoever created it got as far as syncing its parent — `DURABILITY.md`
    ///   assumption 8. That is an assumption about a *predecessor's* progress, and this crate can
    ///   reach the state where it is false without any crash at all, because `sync_dir` can fail: a
    ///   promotion that creates `chunks/aa/bb` and then takes an error from the `fsync` of
    ///   `chunks/aa` returns `Err` over a directory that exists and whose name is not durable, and
    ///   every later promotion into that 1-in-65 536 fanout slot skipped it forever
    ///   (`01KZEBAEK05P3XB9A788TYV4QM`). The crash variant is the same window, one syscall wide.
    ///   `tests/promotion_ordering.rs::a_fanout_directory_left_uncommitted_by_a_failed_promotion_is_committed_by_the_next_one`
    ///   is the reproduction and it is red on the skip.
    ///
    /// The price is two `fsync` calls of directories that are clean on 65 535 promotions out of
    /// 65 536 — **14.6–15.0 µs** per promotion on the host in `benchmarks/budgets/storage.md` §1,
    /// measured rather than assumed and recorded in `DURABILITY.md`'s "What it costs". It is
    /// bounded by the depth of the fanout, which is two, and never by the size of the store:
    /// exactly the three directories on this chunk's own path are synced, once each, whatever else
    /// is in `chunks/`.
    ///
    /// `chunks/` is the one level whose own entry is not committed here: it lives in the workspace
    /// root, which this crate does not own. `DURABILITY.md` records that as the caller's.
    fn create_chunk_directory(&mut self) -> Result<(), CasError> {
        let chain = self.layout.chunk_directory_chain(&self.digest);
        for depth in 0..chain.len() {
            let directory = &chain[depth];
            if !self.filesystem.exists(directory) {
                self.filesystem
                    .create_dir_all(directory)
                    .map_err(|error| CasError::io("create_dir_all", directory, error))?;
            }
            let Some(parent) = depth.checked_sub(1).map(|above| &chain[above]) else {
                continue;
            };
            self.filesystem
                .sync_dir(parent)
                .map_err(|error| CasError::io("sync_dir", parent, error))?;
        }
        Ok(())
    }

    /// Step 6. Commit the entry the rename created, on **every** path.
    ///
    /// This used to return early when the chunk was already present, on the reasoning that a no-op
    /// rename has nothing to commit. That is true only if some earlier promotion ran this step, and
    /// there is a reachable sequence in which none did: a promotion is killed between `Link` and
    /// here — the state `crash-promotion.rs::killing_after_linking_leaves_a_whole_readable_chunk`
    /// produces with a real `SIGKILL` — and every later promotion of the same content took the same
    /// early return, so the rename that made the chunk visible was never followed by a directory
    /// sync by anybody (`01KZE8KDMMQQDV62C83AZWFVSS`). The cost of being wrong is a lost chunk; the
    /// cost of being right is one `fsync` of a directory that is usually already clean.
    fn perform_sync_directory(&mut self) -> Result<(), CasError> {
        let directory = self.layout.chunk_directory(&self.digest);
        self.filesystem
            .sync_dir(&directory)
            .map_err(|error| CasError::io("sync_dir", &directory, error))
    }

    /// Stop holding the staging name, so `discard_scratch` may remove anything left at it.
    ///
    /// Every exit from a promotion that has staged runs through here — verification failure, the
    /// deduplicated `Link`, the linking `Link`, and the destructor — and each one calls it *after*
    /// the file has been renamed away or removed. A single writer of `self.staged` keeps the
    /// registry and the field from disagreeing, which is the one way this guard could quietly stop
    /// guarding.
    fn release_staged(&mut self) {
        if let Some(path) = self.staged.take() {
            release_staging_path(&path);
        }
    }

    /// The staged path, which every step after `Stage` has by construction.
    ///
    /// `Stage` is the first step and sets it; the state machine performs steps only in
    /// [`PromotionStep::ORDER`], and the two steps that clear it (`Verify` on failure, `Link` on
    /// success) both end the promotion's use of it. The fallback is the staging path for attempt
    /// zero rather than a panic, because a panic in a durability path is a worse failure than an
    /// error from an operation on a path that does not exist.
    fn staged_or_unreachable(&self) -> PathBuf {
        self.staged.clone().unwrap_or_else(|| {
            self.layout
                .staging_path(&self.digest, std::process::id(), 0)
        })
    }
}

impl<F: DurableFs, D: ContentDigest> Drop for Promotion<'_, F, D> {
    /// Discard an abandoned staging file — plan §6.3's "before step 4: temporary data is
    /// discarded", for the case where the process lives to do it.
    ///
    /// Best-effort by necessity: a destructor that returned an error would have nowhere to put it
    /// and a destructor that panicked during unwinding would abort the process. The case this
    /// cannot cover — the process dying mid-promotion — is covered by
    /// [`crate::Cas::discard_scratch`] instead, and that is the one the crash tests exercise.
    fn drop(&mut self) {
        let Some(path) = self.staged.clone() else {
            return;
        };
        let _ = self.filesystem.remove_file(&path);
        self.release_staged();
    }
}
