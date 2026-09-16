//! The arrival journal: how the collector finds unreferenced chunks without reading the store.
//!
//! # The problem it solves
//!
//! Plan §6.3 says that a crash after step 4 but before step 9 leaves a chunk in the store that no
//! durable manifest references, and that such chunks are garbage-collected. The obvious way to
//! find them is to list every chunk and ask the index about each. That is a full scan of the
//! largest thing in the workspace, and the acceptance criterion for this task is precisely that it
//! not be needed on every run.
//!
//! So promotion writes its intent down first. Before a chunk becomes visible it appends `+ <hex>`
//! here; when the transaction that references it commits, the caller appends `- <hex>`. The set
//! that is still `+` is the complete set of chunks that might be unreferenced, and it is bounded
//! by the promotions since the last commit rather than by the size of the store.
//!
//! # Why the ordering makes a torn write safe in both directions
//!
//! One record is one `write_all` followed by an `fsync`, so a crash can only damage the record
//! being appended — the last one. [`ArrivalJournal::candidates`] therefore ignores an unterminated
//! final line, and that is sound in both directions, which is the part worth checking rather than
//! asserting:
//!
//! * A torn `+` line means the append had not completed, and the append completes strictly before
//!   the rename that makes the chunk visible. So a torn `+` describes a chunk that is not in the
//!   store. Forgetting it loses nothing.
//! * A torn `-` line means a chunk stays in the candidate set although it is now referenced. The
//!   collector asks the reference oracle before deleting anything, the oracle says it is
//!   referenced, and nothing happens. Forgetting it costs one query.
//!
//! A malformed line anywhere *other* than the tail cannot be explained by a torn append. It is
//! real damage, and it is reported ([`CasError::JournalMalformed`]) rather than skipped, because
//! skipping it would silently turn "this chunk is referenced" into "this chunk is a candidate".
//!
//! # The case that argument does not cover
//!
//! "A torn record is an unterminated last line" assumes a crash truncates the file. A filesystem is
//! free to persist blocks out of order, so an interrupted append that straddled a block boundary
//! can instead leave the *end* of the record durable and its beginning as a hole — a line that is
//! newline-terminated and garbage. A record is 67 bytes, so roughly one record in sixty straddles a
//! four-kilobyte boundary and is exposed to this.
//!
//! That case is not silently mishandled, but it is not silently *tolerated* either: it fails
//! [`parse_record`] at a line that is not the tail and comes back as
//! [`CasError::JournalMalformed`], so the collector refuses to run rather than deleting something
//! on a misread. Recovery is [`crate::Cas::sweep_all_chunks`] — rebuild the candidate set from the
//! store itself, which is exactly the backstop the journal is an optimisation over. What is *not*
//! claimed anywhere is that a crash can never make this file unreadable; it can, and the design is
//! that unreadable costs a scan.
//!
//! # What losing the journal costs, and what it does not
//!
//! Losing this file does not lose data: every chunk stays exactly where it is, and every reference
//! to it stays valid. It loses the *index of candidates*, so unreferenced chunks become invisible
//! to the cheap path until [`crate::Cas::sweep_all_chunks`] — the full scan, named as the backstop
//! it is — runs. Cheap detection is an optimisation over a correct fallback, never a substitute
//! for one.
//!
//! # Losing all of it and losing some of it are different failures
//!
//! The paragraph above describes an all-or-nothing loss, and that is the easy half. A journal that
//! is gone, or that fails [`parse_record`] away from the tail, *announces itself*: the next
//! [`ArrivalJournal::candidates`] answers with an empty set or with
//! [`CasError::JournalMalformed`], and either answer tells an operator to sweep.
//!
//! A journal that is **silently incomplete** announces nothing. Well-formed, readable, every
//! record valid, and missing the `+` for a chunk that is sitting in the store: the fold returns a
//! healthy-looking candidate set that is a strict subset of the true one, every subsequent run
//! agrees with every other, and the chunks it dropped are invisible to the cheap path for the
//! lifetime of the store. No fold over this file can detect it, because the evidence that would
//! prove a record missing is the record that is missing. That is the acceptance criterion
//! "unreferenced chunks are detectable without scanning the whole store on every run" failing with
//! no symptom, and it is strictly worse than losing the file outright.
//!
//! Exactly two operations shorten this file — [`ArrivalJournal::compact`] and
//! [`ArrivalJournal::forget`] — and both replace it wholesale, so both are the only routes by
//! which a partial loss can be manufactured. What stops them is stated on
//! [`ArrivalJournal::compact`] and enforced by it: a rewrite is not a read, it excludes appends
//! rather than racing them, and it refuses rather than proceeding when it can see that it did not
//! fold every byte. Recovery, if one ever happens anyway, is still
//! [`crate::Cas::sweep_all_chunks`] — but it has to be run on suspicion rather than on a signal,
//! which is why the exclusion is a precondition of the operation rather than advice about it.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use crate::digest::Digest32;
use crate::error::CasError;
use crate::fs::DurableFs;
use crate::layout::StoreLayout;

/// The marker for a chunk that has become visible in the store.
const ARRIVED: u8 = b'+';

/// The marker for a chunk a durable reference now points at.
const RETAINED: u8 = b'-';

/// One line of the journal: a marker, a space, 64 hex characters, a newline.
const RECORD_LEN: usize = 67;

/// The exclusion protecting one journal file from being rewritten while it is being appended to.
///
/// A reader-writer lock and not a mutex, because the two kinds of writer here need different
/// answers. Appends are the *shared* side: two promotions appending concurrently is inside the
/// store's model — `layout.rs` says so where it explains why a staging name carries a thread's
/// content digest — and serialising them would make every promotion wait behind every other for
/// nothing, since an append is one `write_all` that the filesystem already orders. A rewrite is
/// the *exclusive* side, because it does not add to the file, it replaces it.
///
/// Keyed by path rather than held by [`ArrivalJournal`], because the journal is a borrowed view
/// constructed afresh on every `Cas::journal()` call: per-instance state would give each call its
/// own lock and exclude nothing. Keying by path also makes two `Cas` values opened on one root in
/// one process share the exclusion, which is the answer a caller would expect and would not get
/// from a lock owned by either.
///
/// The table only grows, by one entry per distinct journal path this process has touched. That is
/// one entry per store root — one for a daemon, one per fixture for a test binary — so it is
/// bounded by a number the caller chooses rather than by traffic. It is a leak, and it is named as
/// one rather than described as a cache.
fn exclusion_for(path: &Path) -> Arc<RwLock<()>> {
    static TABLE: OnceLock<Mutex<BTreeMap<PathBuf, Arc<RwLock<()>>>>> = OnceLock::new();
    let mut table = TABLE
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        // The guarded value is a map of locks and nothing reads a half-written invariant out of
        // it, so a panic elsewhere must not turn every later journal operation into a panic here.
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Arc::clone(table.entry(path.to_path_buf()).or_default())
}

/// The append-only record of which chunks have arrived and which are spoken for.
///
/// Borrowed rather than owned: the journal is a view over the store's filesystem and layout, so
/// there is no second copy of either and no way for the two to disagree about where the file is.
#[derive(Debug)]
pub struct ArrivalJournal<'a, F: DurableFs> {
    filesystem: &'a F,
    layout: &'a StoreLayout,
}

impl<'a, F: DurableFs> ArrivalJournal<'a, F> {
    /// A journal over this store's layout.
    pub(crate) fn new(filesystem: &'a F, layout: &'a StoreLayout) -> Self {
        Self { filesystem, layout }
    }

    /// Record that a chunk is about to become visible.
    ///
    /// Returns only once the record is durable, because the whole guarantee is that this record
    /// exists before the chunk does.
    ///
    /// # Errors
    ///
    /// [`CasError::Io`] if the append or either sync fails. The caller must treat that as a
    /// refusal to promote: a chunk made visible after a failed append is a chunk the collector
    /// cannot see.
    pub fn record_arrival(&self, digest: &Digest32) -> Result<(), CasError> {
        self.append_record(ARRIVED, digest)
    }

    /// Record that a durable reference now points at a chunk, so it is no longer a candidate.
    ///
    /// # Errors
    ///
    /// [`CasError::Io`] if the append or either sync fails. A failure here is not a correctness
    /// problem — the chunk merely stays in the candidate set and is protected by the oracle — but
    /// it is reported rather than swallowed so the caller can retry.
    pub fn record_retained(&self, digest: &Digest32) -> Result<(), CasError> {
        self.append_record(RETAINED, digest)
    }

    fn append_record(&self, marker: u8, digest: &Digest32) -> Result<(), CasError> {
        let path = self.layout.arrival_journal();
        let directory = self.layout.logs_directory();
        self.filesystem
            .create_dir_all(&directory)
            .map_err(|error| CasError::io("create_dir_all", &directory, error))?;

        // The shared side of the exclusion, held until this function returns — which is until the
        // record is durable, not merely written. A rewrite that started here would replace the
        // file under the append and destroy the record; see `compact`.
        //
        // Taken after the directory exists and not before, for two reasons. It covers exactly the
        // journal file, which is the only thing a rewrite replaces — creating the directory a
        // rewrite is also happy to create is not contention worth serialising. And it leaves the
        // `create_dir_all` observable to a caller's own `DurableFs`, which is the only point at
        // which a test can see this thread arrive at the lock: everything after it blocks, so a
        // guard taken first would make an append that is about to wait indistinguishable from one
        // that has not been called.
        let exclusion = exclusion_for(&path);
        let _appending = exclusion
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let mut record = Vec::with_capacity(RECORD_LEN);
        record.push(marker);
        record.push(b' ');
        record.extend_from_slice(digest.to_hex().as_bytes());
        record.push(b'\n');

        self.filesystem
            .append(&path, &record)
            .map_err(|error| CasError::io("append", &path, error))?;
        self.filesystem
            .sync_file(&path)
            .map_err(|error| CasError::io("sync_file", &path, error))?;
        // The directory sync matters only for the first record, which creates the file; syncing
        // unconditionally is one call against having to know whether this append was the first.
        self.filesystem
            .sync_dir(&directory)
            .map_err(|error| CasError::io("sync_dir", &directory, error))
    }

    /// Every chunk that has arrived and has not been recorded as referenced.
    ///
    /// Reads the journal and nothing else — in particular it never lists `chunks/`, which is the
    /// property `tests/unreferenced.rs` asserts by counting directory listings.
    ///
    /// # Errors
    ///
    /// [`CasError::Io`] if the journal cannot be read, or [`CasError::JournalMalformed`] if it
    /// holds a line that is not a record and is not the torn tail.
    pub fn candidates(&self) -> Result<Vec<Digest32>, CasError> {
        let exclusion = exclusion_for(&self.layout.arrival_journal());
        let _reading = exclusion
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.fold().map(|folded| folded.candidates)
    }

    /// The candidate set and the exact number of bytes it was folded from.
    ///
    /// Takes no guard, so that [`Self::compact`] and [`Self::forget`] can fold while holding the
    /// exclusive one — a re-entrant shared acquisition on the same thread would deadlock against
    /// their own write guard, and re-reading after releasing it would reopen the window they exist
    /// to close. The byte count is what makes the pre-rename check in [`Self::rewrite_to`]
    /// meaningful: it is not "how long was the file when I asked" but "how much of it did I
    /// actually account for".
    fn fold(&self) -> Result<FoldedJournal, CasError> {
        let path = self.layout.arrival_journal();
        let bytes = match self.filesystem.read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(FoldedJournal {
                    candidates: Vec::new(),
                    folded_bytes: 0,
                })
            }
            Err(error) => return Err(CasError::io("read", &path, error)),
        };

        let mut live: BTreeSet<Digest32> = BTreeSet::new();
        for (index, line) in complete_lines(&bytes).into_iter().enumerate() {
            let (marker, digest) = parse_record(line).ok_or(CasError::JournalMalformed {
                path: path.clone(),
                line: index + 1,
            })?;
            match marker {
                ARRIVED => {
                    live.insert(digest);
                }
                _ => {
                    live.remove(&digest);
                }
            }
        }
        Ok(FoldedJournal {
            candidates: live.into_iter().collect(),
            folded_bytes: bytes.len() as u64,
        })
    }

    /// Rewrite the journal so it holds one `+` record per live candidate and nothing else.
    ///
    /// Returns how many records the new journal has. Crash-safe by the same construction as a
    /// promotion — the replacement is written and synced under a second name, then renamed over
    /// the original, so an interruption leaves either the old journal or the new one, and an
    /// interruption before the rename leaves a stale rewrite file which the next rewrite removes —
    /// and concurrency-safe **only under exclusion**, which is said here rather than a paragraph
    /// away because a reader takes both properties from the same place and would otherwise take
    /// the first and assume the second. The rename replaces the very file
    /// [`Self::record_arrival`] appends to, so an arrival landing between the fold and the rename
    /// would be written to the inode the rename discards: the chunk stays visible in the store,
    /// leaves the candidate set permanently, and becomes reachable again only through
    /// [`crate::Cas::sweep_all_chunks`], with a journal that looks perfectly healthy afterwards.
    ///
    /// Within this process the precondition is met by this method rather than assumed of the
    /// caller. Every append holds a shared guard on this journal's path and every rewrite holds
    /// the exclusive one, so an arrival either completes in full before the fold reads or begins
    /// after the rename — and promotions are not serialised against each other, only against a
    /// rewrite.
    ///
    /// Across processes there is no exclusion and this method does not manufacture one; a second
    /// process rewriting or appending to the same journal is outside the store's model. What this
    /// method does about it is *notice*: immediately before the rename it re-reads the journal's
    /// length and refuses, leaving the journal exactly as it found it, if the file is no longer
    /// the bytes it folded. That check is a detector, not a proof — an append arriving between the
    /// check and the rename is still lost, and no arrangement of `append` and `rename` alone can
    /// close that gap. The guarantee is the exclusion; the check is what makes a violated
    /// precondition usually loud instead of always silent.
    ///
    /// # Errors
    ///
    /// [`CasError::Io`] on any filesystem failure, on whatever [`Self::candidates`] returns, and
    /// with operation `"compact"` if the journal changed underneath the rewrite.
    pub fn compact(&self) -> Result<usize, CasError> {
        let exclusion = exclusion_for(&self.layout.arrival_journal());
        let _rewriting = exclusion
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let folded = self.fold()?;
        self.rewrite_to(&folded.candidates, folded.folded_bytes)?;
        Ok(folded.candidates.len())
    }

    /// Drop these digests from the candidate set, keeping every other candidate.
    ///
    /// Returns how many records the new journal has. What the collector calls **after** it has
    /// removed the chunks: a deleted chunk that stayed `+` here would be offered as a candidate
    /// again on every future run, and each of those runs would `stat` a file that is not there.
    ///
    /// It is deliberately not spelled as a `-` record. That marker means "a durable reference now
    /// points at this", and writing it for a chunk that was just deleted would put a false
    /// statement in a durable file to save one rewrite.
    ///
    /// **Order matters and the safe order is delete-then-forget.** Forgetting first and crashing
    /// before the delete leaves a chunk in the store that the cheap candidate path can no longer
    /// see — recoverable only by [`crate::Cas::sweep_all_chunks`]. Deleting first and crashing
    /// before the forget leaves a stale `+` for a chunk that is gone, which costs one `stat` on the
    /// next run and nothing else.
    ///
    /// It replaces the journal rather than adding to it, so it carries exactly the exclusion and
    /// the pre-rename check described on [`Self::compact`], for exactly the same reason: a chunk
    /// whose arrival was appended while this ran would be dropped from the candidate set while
    /// remaining in the store.
    ///
    /// # Errors
    ///
    /// [`CasError::Io`] on any filesystem failure, on whatever [`Self::candidates`] returns, and
    /// with operation `"compact"` if the journal changed underneath the rewrite.
    pub fn forget(&self, collected: &BTreeSet<Digest32>) -> Result<usize, CasError> {
        let exclusion = exclusion_for(&self.layout.arrival_journal());
        let _rewriting = exclusion
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let folded = self.fold()?;
        let remaining: Vec<Digest32> = folded
            .candidates
            .into_iter()
            .filter(|digest| !collected.contains(digest))
            .collect();
        self.rewrite_to(&remaining, folded.folded_bytes)?;
        Ok(remaining.len())
    }

    /// Replace the journal with one `+` record per digest, crash-safely.
    ///
    /// The replacement is written and synced under a second name, then renamed over the original,
    /// so an interruption leaves either the old journal or the new one. An interruption before the
    /// rename leaves a stale rewrite file, which the next rewrite removes.
    ///
    /// Every caller holds the exclusive guard from [`exclusion_for`] — this function does not take
    /// it, because taking it here and there would deadlock, and taking it only here would leave
    /// the caller's fold outside it. `folded_bytes` is how much of the journal the caller
    /// accounted for; the file is checked against it immediately before the rename, so that a byte
    /// this rewrite never read is a refusal rather than a deletion. See [`Self::compact`] for why
    /// that check detects rather than prevents.
    fn rewrite_to(&self, digests: &[Digest32], folded_bytes: u64) -> Result<(), CasError> {
        let directory = self.layout.logs_directory();
        let rewrite = self.layout.arrival_journal_rewrite();
        let path = self.layout.arrival_journal();

        self.filesystem
            .create_dir_all(&directory)
            .map_err(|error| CasError::io("create_dir_all", &directory, error))?;
        if self.filesystem.exists(&rewrite) {
            self.filesystem
                .remove_file(&rewrite)
                .map_err(|error| CasError::io("remove_file", &rewrite, error))?;
        }

        let mut body = Vec::with_capacity(digests.len() * RECORD_LEN);
        for digest in digests {
            body.push(ARRIVED);
            body.push(b' ');
            body.extend_from_slice(digest.to_hex().as_bytes());
            body.push(b'\n');
        }

        self.filesystem
            .stage(&rewrite, &body)
            .map_err(|error| CasError::io("stage", &rewrite, error))?;
        self.filesystem
            .sync_file(&rewrite)
            .map_err(|error| CasError::io("sync_file", &rewrite, error))?;

        // The last moment at which refusing still costs nothing: the replacement is durable but
        // nothing points at it, so returning here leaves the journal byte-for-byte as it was.
        let present = self.journal_len()?;
        if present != folded_bytes {
            // The stale rewrite file is left for the next rewrite to remove, which is what the
            // crash path already leaves behind and already handles. A removal failure here would
            // replace the diagnosis with a symptom of it, so the diagnosis is what is returned.
            return Err(CasError::io(
                "compact",
                &path,
                io::Error::other(format!(
                    "the arrival journal is {present} bytes and this rewrite folded {folded_bytes} \
                     of them, so a record arrived while it ran; renaming the replacement over it \
                     would drop that chunk from the candidate set while leaving it in the store, \
                     so the journal was left unchanged"
                )),
            ));
        }

        self.filesystem
            .rename(&rewrite, &path)
            .map_err(|error| CasError::io("rename", &rewrite, error))?;
        self.filesystem
            .sync_dir(&directory)
            .map_err(|error| CasError::io("sync_dir", &directory, error))
    }

    /// How many bytes the journal holds, counting a journal that does not exist yet as zero — the
    /// same reading [`Self::fold`] takes of a missing file, so the two are comparable.
    fn journal_len(&self) -> Result<u64, CasError> {
        let path = self.layout.arrival_journal();
        match self.filesystem.file_len(&path) {
            Ok(length) => Ok(length),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(0),
            Err(error) => Err(CasError::io("file_len", &path, error)),
        }
    }
}

/// A fold over the journal, with the extent of the file it consumed.
///
/// The byte count travels with the candidate set rather than being fetched again later, because a
/// second `file_len` after the fold answers a different question — "how long is the file now",
/// which is already the wrong side of the race this pairing exists to detect.
#[derive(Debug)]
struct FoldedJournal {
    candidates: Vec<Digest32>,
    folded_bytes: u64,
}

/// The newline-terminated lines of `bytes`. An unterminated trailing line is a torn append and is
/// not yielded — see the module docs for why that is safe for both markers.
fn complete_lines(bytes: &[u8]) -> Vec<&[u8]> {
    let Some(last) = bytes.iter().rposition(|byte| *byte == b'\n') else {
        return Vec::new();
    };
    let mut lines: Vec<&[u8]> = bytes[..=last].split(|byte| *byte == b'\n').collect();
    // Splitting input that ends in the separator yields a final empty slice: that is the
    // terminator, not a line. Exactly one element is dropped, so an empty line *inside* the file
    // is kept and reaches `parse_record`, which rejects it as malformed rather than skipping it.
    lines.pop();
    lines
}

/// A record is a marker, a space and 64 hex characters. Anything else is not a record.
fn parse_record(line: &[u8]) -> Option<(u8, Digest32)> {
    if line.len() != RECORD_LEN - 1 || line[1] != b' ' {
        return None;
    }
    let marker = line[0];
    if marker != ARRIVED && marker != RETAINED {
        return None;
    }
    let hex = core::str::from_utf8(&line[2..]).ok()?;
    Digest32::parse_hex(hex).ok().map(|digest| (marker, digest))
}
