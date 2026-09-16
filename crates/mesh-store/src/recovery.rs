//! Recovery after a crash: find the last durable boundary, then rebuild the index from records.
//!
//! # The sentence this module exists to make executable
//!
//! > **Every table is reconstructable from immutable operations and manifests alone.**
//!
//! [`crate::rebuild`] already proves that over a record stream a caller holds in memory. That is
//! the *arithmetic* of reconstruction and it is not the recovery: after a crash nobody is holding
//! anything, and the question is where the records come from and how far they can be trusted. This
//! module answers both, and it answers them without a repair path — plan §6.3's clause is *"index
//! corruption: rebuild from immutable operations and manifests"*, and a repair would have to decide
//! which of two disagreeing surfaces is right when plan §6.1 already says the database is never the
//! right one.
//!
//! # The three states a crash can leave the record journal in, and why only three
//!
//! The journal is append-only and every record is written as one length-prefixed, checksummed
//! frame. That framing is what makes the classification decidable rather than a judgement call:
//!
//! | What the scan finds | What it means | What recovery does |
//! |---|---|---|
//! | Frames that all verify, ending exactly at the file's end | a clean stop | rebuilds to the last frame |
//! | A trailing **fragment** shorter than its own declared length | a kill mid-append; those bytes were never a whole record and were never acknowledged | rebuilds to the boundary *before* the fragment, and reports the discarded bytes |
//! | A frame that is whole but does not match its checksum | the bytes on disk are not the bytes that were written | **stops and reports** — [`RecoveryError::Damaged`] |
//!
//! The third row is the fourth acceptance criterion in its only defensible form. A recovery that
//! skipped a damaged frame and carried on would produce an index missing a record, digest-equal to
//! nothing, and *silent*. There is deliberately no flag to make it do that: the loud failure is the
//! whole behaviour, and [`JournalDamage::intact_prefix`] exists so an operator can see how far the
//! journal was good, not so a caller can recover past the damage.
//!
//! # Why a torn tail is not damage
//!
//! A frame declares its own body length before its body. An interrupted append therefore leaves
//! *fewer* bytes than the header promises, which is arithmetic rather than inference. The record
//! was never whole, so no acknowledgement can ever have been given for it, so discarding it cannot
//! lose acknowledged state. `tests/recovery.rs` kills a real process at every one of plan §6.3's
//! eleven steps and checks the boundary that comes back against exactly this rule.
//!
//! # What is measured, and where
//!
//! The recovery-time budget is plan §6.3's *"recovery after daemon crash: under five seconds"*.
//! Nothing in this module reads a clock — `mesh-store` orders by `lamport → event id →
//! content-hash` and never by time, and a duration measured inside the crate would be a number
//! nobody could reproduce. The measurement lives with the caller: `tests/recovery.rs` asserts it
//! end to end through a real database, and `crates/mesh-bench/benches/recovery.rs` reports it as a
//! benchmark row.

use core::fmt;

use crate::commit::CommitPlan;
use crate::digest::{Digest16, Fnv1a128, IndexDigest};
use crate::ids::{EntityUuid, RecordDigest};
use crate::index::FoldError;
use crate::rebuild::{rebuild, RebuildReport};
use crate::record::{
    AckRecord, ApprovalRecord, ChunkSlice, ContextAccess, ContextRecord, ManifestRecord,
    OperationRecord, PeerRecord, RecordKind, ReviewRecord, ReviewVerdict, StoredRecord,
};
use crate::recovery_state::PendingMeaningfulSave;
use crate::sequence::PrivateSaved;
use crate::sql::{SqlExecutor, Store};

/// The two bytes every journal frame starts with.
const FRAME_MAGIC: [u8; 2] = *b"MJ";

/// The frame layout this build writes and reads.
const FRAME_VERSION: u8 = 1;

/// Magic, version, record tag and the body length.
const FRAME_PREFIX_BYTES: usize = 2 + 1 + 1 + 4;

/// A checksum, over the prefix and again over the body.
const CHECKSUM_BYTES: usize = 16;

/// The prefix and the checksum that makes the declared body length trustworthy.
///
/// **The header carries its own checksum, and that is the load-bearing part of the layout.** The
/// body length is the number the scan uses to decide whether the rest of a frame is present, so a
/// flipped bit in it would otherwise enlarge the frame past the end of the file and read as an
/// interrupted append — silently discarding every record after it. Verifying the prefix before
/// trusting its length turns that from a quiet truncation into [`DamageKind::HeaderDamaged`].
const HEADER_BYTES: usize = FRAME_PREFIX_BYTES + CHECKSUM_BYTES;

/// The durable, append-only log of immutable records: the one input a rebuild is allowed.
///
/// Two methods, neither of which mentions a filesystem. The journal owner decides how bytes reach
/// stable storage; this crate keeps the part that must not be a fixture: the frame layout, the
/// checksum and the scan that decides where durability stops.
pub trait RecordJournal {
    /// Whatever the journal fails with.
    type Error;

    /// Every byte the journal holds, in the order it was appended.
    ///
    /// # Errors
    ///
    /// Whatever the implementation reports.
    fn read_all(&mut self) -> Result<Vec<u8>, Self::Error>;

    /// Append these bytes and force them to durable storage before returning.
    ///
    /// The durability is the contract. An implementation that buffers has moved the last durable
    /// boundary somewhere [`scan_journal`] cannot see, which is the one way this module can be made
    /// to lie.
    ///
    /// # Errors
    ///
    /// Whatever the implementation reports.
    fn append(&mut self, framed: &[u8]) -> Result<(), Self::Error>;
}

/// How far the journal is durable: how many whole records, and where they end.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DurableBoundary {
    /// How many whole, verified records precede the boundary.
    pub records: u64,
    /// The byte offset the last whole record ends at.
    pub byte_offset: u64,
}

impl fmt::Display for DurableBoundary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} records, {} bytes",
            self.records, self.byte_offset
        )
    }
}

/// What lies after the last durable boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TailResidue {
    /// Nothing: the journal ends exactly where the last whole record ends.
    Whole,
    /// A fragment of a record whose append was interrupted. Never whole, so never acknowledged.
    Fragment {
        /// How many bytes are discarded by resuming at the boundary.
        bytes: u64,
    },
}

impl TailResidue {
    /// Whether the journal ends mid-record, which is what a kill during an append leaves.
    #[must_use]
    pub const fn is_fragment(self) -> bool {
        matches!(self, Self::Fragment { .. })
    }

    /// How many bytes lie after the boundary.
    #[must_use]
    pub const fn discarded_bytes(self) -> u64 {
        match self {
            Self::Whole => 0,
            Self::Fragment { bytes } => bytes,
        }
    }
}

/// What a scan of the journal found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalScan {
    records: Vec<StoredRecord>,
    boundary: DurableBoundary,
    tail: TailResidue,
}

impl JournalScan {
    /// Every whole record, in the order it was appended.
    #[must_use]
    pub fn records(&self) -> &[StoredRecord] {
        &self.records
    }

    /// Take the records, for a rebuild that consumes them.
    #[must_use]
    pub fn into_records(self) -> Vec<StoredRecord> {
        self.records
    }

    /// The last durable boundary.
    #[must_use]
    pub const fn boundary(&self) -> DurableBoundary {
        self.boundary
    }

    /// What lies after it.
    #[must_use]
    pub const fn tail(&self) -> TailResidue {
        self.tail
    }
}

/// Why a frame could not be read, in the classes a scan can tell apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DamageKind {
    /// The frame does not begin where a frame must begin.
    NotAFrame,
    /// The header is whole and its bytes are not the bytes that were written, so the body length
    /// it declares cannot be believed and the frame cannot be measured.
    HeaderDamaged {
        /// The checksum the header carries.
        declared: Digest16,
        /// The checksum its bytes actually produce.
        computed: Digest16,
    },
    /// The frame was written by a layout this build does not read.
    UnknownFrameVersion {
        /// The version the frame declares.
        found: u8,
    },
    /// The record tag names no [`RecordKind`] this build knows.
    UnknownRecordKind {
        /// The tag the frame declares.
        found: u8,
    },
    /// The frame is whole and its bytes are not the bytes that were written.
    ChecksumMismatch {
        /// The checksum the frame carries.
        declared: Digest16,
        /// The checksum its bytes actually produce.
        computed: Digest16,
    },
    /// The checksum matched and the body is still not a record of the declared kind.
    MalformedBody,
}

impl fmt::Display for DamageKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAFrame => formatter.write_str("the bytes do not begin a record frame"),
            Self::HeaderDamaged { declared, computed } => write!(
                formatter,
                "the frame header carries checksum {declared} and its bytes produce {computed}, so \
                 the length it declares cannot be believed"
            ),
            Self::UnknownFrameVersion { found } => {
                write!(formatter, "the frame declares layout version {found}")
            }
            Self::UnknownRecordKind { found } => {
                write!(formatter, "the frame declares record tag {found}")
            }
            Self::ChecksumMismatch { declared, computed } => write!(
                formatter,
                "the frame carries checksum {declared} and its bytes produce {computed}"
            ),
            Self::MalformedBody => {
                formatter.write_str("the body is not a record of the kind the frame declares")
            }
        }
    }
}

/// One unrecoverable record, named so it is reported rather than skipped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JournalDamage {
    ordinal: u64,
    byte_offset: u64,
    kind: DamageKind,
    intact_prefix: DurableBoundary,
}

impl JournalDamage {
    /// Which record, counting from zero.
    #[must_use]
    pub const fn ordinal(self) -> u64 {
        self.ordinal
    }

    /// Where in the journal it starts.
    #[must_use]
    pub const fn byte_offset(self) -> u64 {
        self.byte_offset
    }

    /// What is wrong with it.
    #[must_use]
    pub const fn kind(self) -> DamageKind {
        self.kind
    }

    /// How far the journal was good, **for a report and not for a shortcut**.
    ///
    /// Nothing in this crate rebuilds from the prefix. Doing so would drop every record after the
    /// damage without saying which, and a record that was acknowledged is one the user was told was
    /// saved privately. An operator who decides to truncate is making that trade knowingly; a
    /// library that decides it silently is the failure this whole module is against.
    #[must_use]
    pub const fn intact_prefix(self) -> DurableBoundary {
        self.intact_prefix
    }
}

impl fmt::Display for JournalDamage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "record {} at byte {} is unrecoverable: {}. The journal is whole for {} before it. \
             Nothing was skipped and nothing was rebuilt.",
            self.ordinal, self.byte_offset, self.kind, self.intact_prefix
        )
    }
}

impl std::error::Error for JournalDamage {}

/// Frame one record: header, body, checksum.
///
/// The name says *frame* rather than *encode* because the layout, not the field order, is what this
/// module owns. It is a local durability envelope and deliberately not a second spelling of the
/// wire format — that is `mesh-types`' canonical encoding, which this crate cannot import.
#[must_use]
pub fn frame_record(record: &StoredRecord) -> Vec<u8> {
    let mut body = Vec::new();
    write_body(record, &mut body);

    let mut framed = Vec::with_capacity(HEADER_BYTES + body.len() + CHECKSUM_BYTES);
    framed.extend_from_slice(&FRAME_MAGIC);
    framed.push(FRAME_VERSION);
    framed.push(kind_tag(record.kind()));
    framed.extend_from_slice(&(body.len() as u32).to_be_bytes());
    framed.extend_from_slice(checksum(&framed).as_bytes());
    framed.extend_from_slice(&body);
    framed.extend_from_slice(checksum(&framed).as_bytes());
    framed
}

/// Frame these records and append each one durably, returning how many bytes the journal grew by.
///
/// # Errors
///
/// Whatever the journal reports. A failure part-way through leaves the journal ending in whole
/// frames — every append is a whole frame — so the boundary a later scan finds is still exact.
pub fn journal_records<'a, J, I>(journal: &mut J, records: I) -> Result<u64, J::Error>
where
    J: RecordJournal,
    I: IntoIterator<Item = &'a StoredRecord>,
{
    let mut written = 0;
    for record in records {
        let framed = frame_record(record);
        journal.append(&framed)?;
        written += framed.len() as u64;
    }
    Ok(written)
}

/// Walk the journal and answer where durability stops.
///
/// # Errors
///
/// [`JournalDamage`] for a frame that is whole and wrong. A frame that is merely *incomplete* is
/// not an error: it is [`TailResidue::Fragment`], because an interrupted append never produced a
/// record anybody could have been told about.
pub fn scan_journal(bytes: &[u8]) -> Result<JournalScan, JournalDamage> {
    let mut records = Vec::new();
    let mut boundary = DurableBoundary::default();
    let mut at = 0usize;

    loop {
        let remaining = bytes.len() - at;
        if remaining == 0 {
            return Ok(JournalScan {
                records,
                boundary,
                tail: TailResidue::Whole,
            });
        }
        if remaining < HEADER_BYTES {
            return Ok(fragment(records, boundary, remaining));
        }

        let header = &bytes[at..at + HEADER_BYTES];
        let damage = |kind: DamageKind| JournalDamage {
            ordinal: boundary.records,
            byte_offset: at as u64,
            kind,
            intact_prefix: boundary,
        };

        if header[0..2] != FRAME_MAGIC {
            return Err(damage(DamageKind::NotAFrame));
        }
        // Before the declared length is used for anything at all.
        let computed = checksum(&header[..FRAME_PREFIX_BYTES]);
        let declared = declared_checksum(&header[FRAME_PREFIX_BYTES..]);
        if computed != declared {
            return Err(damage(DamageKind::HeaderDamaged { declared, computed }));
        }
        if header[2] != FRAME_VERSION {
            return Err(damage(DamageKind::UnknownFrameVersion { found: header[2] }));
        }
        let Some(kind) = kind_from_tag(header[3]) else {
            return Err(damage(DamageKind::UnknownRecordKind { found: header[3] }));
        };
        let body_length = u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize;
        let frame_length = HEADER_BYTES + body_length + CHECKSUM_BYTES;
        if remaining < frame_length {
            return Ok(fragment(records, boundary, remaining));
        }

        let frame = &bytes[at..at + frame_length];
        let computed = checksum(&frame[..HEADER_BYTES + body_length]);
        let declared = declared_checksum(&frame[HEADER_BYTES + body_length..]);
        if computed != declared {
            return Err(damage(DamageKind::ChecksumMismatch { declared, computed }));
        }

        let Some(record) = read_body(kind, &frame[HEADER_BYTES..HEADER_BYTES + body_length]) else {
            return Err(damage(DamageKind::MalformedBody));
        };
        records.push(record);
        at += frame_length;
        boundary = DurableBoundary {
            records: boundary.records + 1,
            byte_offset: at as u64,
        };
    }
}

fn fragment(
    records: Vec<StoredRecord>,
    boundary: DurableBoundary,
    remaining: usize,
) -> JournalScan {
    JournalScan {
        records,
        boundary,
        tail: TailResidue::Fragment {
            bytes: remaining as u64,
        },
    }
}

/// What a recovery did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryReport {
    boundary: DurableBoundary,
    tail: TailResidue,
    rebuild: RebuildReport,
}

impl RecoveryReport {
    /// The last durable boundary the scan found.
    #[must_use]
    pub const fn boundary(&self) -> DurableBoundary {
        self.boundary
    }

    /// What lay after it.
    #[must_use]
    pub const fn tail(&self) -> TailResidue {
        self.tail
    }

    /// The rebuild, row counts and all.
    #[must_use]
    pub const fn rebuild(&self) -> &RebuildReport {
        &self.rebuild
    }

    /// The digest of the index that came back.
    #[must_use]
    pub const fn digest(&self) -> Digest16 {
        self.rebuild.digest
    }

    /// Whether the journal ended mid-record, which is the evidence a crash interrupted an append.
    #[must_use]
    pub const fn interrupted(&self) -> bool {
        self.tail.is_fragment()
    }
}

/// Why a recovery stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecoveryError<J, D> {
    /// The journal could not be read.
    Journal(J),
    /// A record is unrecoverable. Reported, never skipped.
    Damaged(JournalDamage),
    /// The records contradict each other, so no index satisfies all of them.
    Fold(FoldError),
    /// The rebuilt tables could not be written back.
    Rewrite(D),
    /// The rebuild produced an index the acknowledged digest does not match.
    ///
    /// Plan §6.3's *"if reconstruction diverges from the pre-drop digest, the divergence is a P0
    /// correctness bug"*. The index is left holding the rebuild — there is nothing better to hold —
    /// and the caller is told which two digests disagree.
    Diverged {
        /// The digest the last acknowledgement carried.
        expected: Digest16,
        /// The digest the rebuild produced.
        rebuilt: Digest16,
    },
}

impl<J: fmt::Display, D: fmt::Display> fmt::Display for RecoveryError<J, D> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Journal(error) => write!(formatter, "the record journal is unreadable: {error}"),
            Self::Damaged(damage) => write!(formatter, "{damage}"),
            Self::Fold(error) => write!(formatter, "{error}"),
            Self::Rewrite(error) => {
                write!(formatter, "the rebuilt index could not be written: {error}")
            }
            Self::Diverged { expected, rebuilt } => write!(
                formatter,
                "the rebuild produced {rebuilt} where the last acknowledgement carried {expected}. \
                 A rebuild that does not reproduce the acknowledged digest is a correctness defect, \
                 never a close-enough recovery."
            ),
        }
    }
}

impl<J: fmt::Debug + fmt::Display, D: fmt::Debug + fmt::Display> std::error::Error
    for RecoveryError<J, D>
{
}

/// Why an earlier acknowledged journal boundary could not reproduce a pending private save.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PendingPrivateSaveBoundaryError<J> {
    /// The append-only journal could not be read.
    Journal(J),
    /// The requested historical boundary is not an exact whole-record prefix of this journal.
    BoundaryUnavailable {
        /// The boundary carried by the recovery pointer.
        expected: DurableBoundary,
        /// The boundary the available prefix actually proved, when one could be scanned.
        actual: Option<DurableBoundary>,
    },
    /// A whole record inside the historical prefix is damaged.
    Damaged(JournalDamage),
    /// The historical records contradict each other.
    Fold(FoldError),
    /// The exact historical fold disagrees with the acknowledgement captured at that boundary.
    Diverged {
        /// The digest carried by the pending acknowledgement.
        expected: Digest16,
        /// The digest reproduced from the immutable historical prefix.
        rebuilt: Digest16,
    },
}

impl<J: fmt::Display> fmt::Display for PendingPrivateSaveBoundaryError<J> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Journal(error) => write!(formatter, "the record journal is unreadable: {error}"),
            Self::BoundaryUnavailable { expected, actual } => write!(
                formatter,
                "the acknowledged journal boundary ({}, {}) is unavailable; the prefix proved {actual:?}",
                expected.records, expected.byte_offset
            ),
            Self::Damaged(error) => error.fmt(formatter),
            Self::Fold(error) => error.fmt(formatter),
            Self::Diverged { expected, rebuilt } => write!(
                formatter,
                "the historical rebuild produced {rebuilt} where the pending acknowledgement carried {expected}"
            ),
        }
    }
}

impl<J: fmt::Debug + fmt::Display> std::error::Error for PendingPrivateSaveBoundaryError<J> {}

impl<E: SqlExecutor> Store<E> {
    /// Reproduce a pending acknowledgement from its exact earlier append-only journal boundary.
    ///
    /// Orthogonal durable records such as opening a review legitimately change the current full
    /// index digest. They do not invalidate an acknowledgement that was true at an earlier exact
    /// boundary. This method keeps the full-index check load-bearing by rebuilding that historical
    /// prefix instead of accepting the current, different digest or falling back to row counts.
    pub fn verify_pending_private_save_at_boundary<J: RecordJournal>(
        &self,
        journal: &mut J,
        pending: PendingMeaningfulSave,
        expected_boundary: DurableBoundary,
    ) -> Result<PrivateSaved, PendingPrivateSaveBoundaryError<J::Error>> {
        let bytes = journal
            .read_all()
            .map_err(PendingPrivateSaveBoundaryError::Journal)?;
        let end = usize::try_from(expected_boundary.byte_offset).ok();
        let Some(prefix) = end.and_then(|end| bytes.get(..end)) else {
            return Err(PendingPrivateSaveBoundaryError::BoundaryUnavailable {
                expected: expected_boundary,
                actual: None,
            });
        };
        let scan = scan_journal(prefix).map_err(PendingPrivateSaveBoundaryError::Damaged)?;
        if scan.boundary() != expected_boundary || scan.tail() != TailResidue::Whole {
            return Err(PendingPrivateSaveBoundaryError::BoundaryUnavailable {
                expected: expected_boundary,
                actual: Some(scan.boundary()),
            });
        }
        let ledger = self.index().rows("schema_version").unwrap_or_default();
        let (rebuilt, _report) =
            rebuild(scan.into_records(), ledger).map_err(PendingPrivateSaveBoundaryError::Fold)?;
        let digest = rebuilt.default_digest();
        if digest != pending.index_digest() {
            return Err(PendingPrivateSaveBoundaryError::Diverged {
                expected: pending.index_digest(),
                rebuilt: digest,
            });
        }
        let (operations, manifests, chunks) = pending.counts();
        Ok(PrivateSaved::after_recovery_verified(
            digest, operations, manifests, chunks,
        ))
    }

    /// Scan the journal, rebuild every record-derived table, and report what was found.
    ///
    /// This is the start-up path, and it is the same code the `rebuild --verify` equivalent runs:
    /// [`Self::recover_verified`] is this plus one comparison, so a verified recovery cannot take a
    /// different route from an unverified one.
    ///
    /// # Errors
    ///
    /// [`RecoveryError`], whose variant says whether the journal, the records or the database was
    /// the problem.
    pub fn recover<J: RecordJournal>(
        &mut self,
        journal: &mut J,
    ) -> Result<RecoveryReport, RecoveryError<J::Error, E::Error>> {
        let bytes = journal.read_all().map_err(RecoveryError::Journal)?;
        let scan = scan_journal(&bytes).map_err(RecoveryError::Damaged)?;
        let boundary = scan.boundary();
        let tail = scan.tail();

        let ledger = self.index().rows("schema_version").unwrap_or_default();
        let (rebuilt, report) =
            rebuild(scan.into_records(), ledger).map_err(RecoveryError::Fold)?;
        let plan = CommitPlan::full_rebuild(&rebuilt);
        self.executor_mut()
            .execute_batch(&plan.sql())
            .map_err(RecoveryError::Rewrite)?;
        self.adopt_index(rebuilt);

        Ok(RecoveryReport {
            boundary,
            tail,
            rebuild: report,
        })
    }

    /// Recover, then hold the result against the digest the last acknowledgement carried.
    ///
    /// `expected` is [`crate::PrivateSaved::index_digest`] as the acknowledging process recorded
    /// it. Comparing against it is what turns "the index was rebuilt" into "the index came back",
    /// and it is the only check that can catch a fold which is self-consistently wrong.
    ///
    /// # Errors
    ///
    /// [`RecoveryError::Diverged`] when the rebuilt digest is not `expected`, and everything
    /// [`Self::recover`] returns.
    pub fn recover_verified<J: RecordJournal>(
        &mut self,
        journal: &mut J,
        expected: Digest16,
    ) -> Result<RecoveryReport, RecoveryError<J::Error, E::Error>> {
        let report = self.recover(journal)?;
        if report.digest() == expected {
            return Ok(report);
        }
        Err(RecoveryError::Diverged {
            expected,
            rebuilt: report.digest(),
        })
    }
}

// -- the frame's own arithmetic ------------------------------------------------------------------

fn checksum(bytes: &[u8]) -> Digest16 {
    let mut algorithm = Fnv1a128::start();
    algorithm.absorb(bytes);
    algorithm.finish()
}

fn declared_checksum(bytes: &[u8]) -> Digest16 {
    let mut fixed = [0u8; CHECKSUM_BYTES];
    fixed.copy_from_slice(&bytes[..CHECKSUM_BYTES]);
    Digest16::from_bytes(fixed)
}

/// The byte a frame carries for each record kind.
///
/// A separate mapping from [`RecordKind::ALL`]'s order on purpose: the enumeration may be reordered
/// or extended, and a journal written last week must still read. `the_tags_are_stable_and_distinct`
/// is what fails when a kind is added without one.
const fn kind_tag(kind: RecordKind) -> u8 {
    match kind {
        RecordKind::Operation => 1,
        RecordKind::Manifest => 2,
        RecordKind::Peer => 3,
        RecordKind::Acknowledgement => 4,
        RecordKind::Review => 5,
        RecordKind::Approval => 6,
        RecordKind::ContextEntry => 7,
    }
}

const fn kind_from_tag(tag: u8) -> Option<RecordKind> {
    match tag {
        1 => Some(RecordKind::Operation),
        2 => Some(RecordKind::Manifest),
        3 => Some(RecordKind::Peer),
        4 => Some(RecordKind::Acknowledgement),
        5 => Some(RecordKind::Review),
        6 => Some(RecordKind::Approval),
        7 => Some(RecordKind::ContextEntry),
        _ => None,
    }
}

fn write_body(record: &StoredRecord, out: &mut Vec<u8>) {
    match record {
        StoredRecord::Operation(operation) => {
            out.extend_from_slice(operation.id.as_bytes());
            out.extend_from_slice(operation.actor.as_bytes());
            out.extend_from_slice(&operation.actor_sequence.to_be_bytes());
            out.extend_from_slice(&operation.hlc_millis.to_be_bytes());
            out.extend_from_slice(&operation.hlc_counter.to_be_bytes());
            out.extend_from_slice(&operation.policy_epoch.to_be_bytes());
            out.extend_from_slice(operation.session.as_bytes());
            out.extend_from_slice(operation.payload_digest.as_bytes());
            out.extend_from_slice(&(operation.parents.len() as u32).to_be_bytes());
            for parent in &operation.parents {
                out.extend_from_slice(parent.as_bytes());
            }
        }
        StoredRecord::Manifest(manifest) => {
            out.extend_from_slice(manifest.id.as_bytes());
            out.extend_from_slice(&manifest.byte_length.to_be_bytes());
            out.extend_from_slice(manifest.content_digest.as_bytes());
            out.extend_from_slice(&(manifest.chunks.len() as u32).to_be_bytes());
            for chunk in &manifest.chunks {
                out.extend_from_slice(chunk.digest.as_bytes());
                out.extend_from_slice(&chunk.byte_offset.to_be_bytes());
                out.extend_from_slice(&chunk.byte_length.to_be_bytes());
            }
        }
        StoredRecord::Peer(peer) => {
            out.extend_from_slice(peer.peer.as_bytes());
            out.extend_from_slice(peer.joined_at.as_bytes());
        }
        StoredRecord::Acknowledgement(ack) => {
            out.extend_from_slice(ack.peer.as_bytes());
            out.extend_from_slice(ack.actor.as_bytes());
            out.extend_from_slice(&ack.actor_sequence.to_be_bytes());
        }
        StoredRecord::Review(review) => {
            out.extend_from_slice(review.bundle.as_bytes());
            out.extend_from_slice(review.subject_operation.as_bytes());
            out.extend_from_slice(review.opened_by.as_bytes());
        }
        StoredRecord::Approval(approval) => {
            out.extend_from_slice(approval.approval.as_bytes());
            out.extend_from_slice(approval.bundle.as_bytes());
            out.extend_from_slice(approval.approver.as_bytes());
            out.push(verdict_tag(approval.verdict));
        }
        StoredRecord::ContextEntry(entry) => {
            out.extend_from_slice(entry.entry.as_bytes());
            out.extend_from_slice(entry.session.as_bytes());
            out.extend_from_slice(entry.operation.as_bytes());
            out.push(access_tag(entry.access));
            out.extend_from_slice(&entry.byte_length.to_be_bytes());
        }
    }
}

const fn verdict_tag(verdict: ReviewVerdict) -> u8 {
    match verdict {
        ReviewVerdict::Approved => 0,
        ReviewVerdict::ChangesRequested => 1,
        ReviewVerdict::Withdrawn => 2,
    }
}

const fn verdict_from_tag(tag: u8) -> Option<ReviewVerdict> {
    match tag {
        0 => Some(ReviewVerdict::Approved),
        1 => Some(ReviewVerdict::ChangesRequested),
        2 => Some(ReviewVerdict::Withdrawn),
        _ => None,
    }
}

const fn access_tag(access: ContextAccess) -> u8 {
    match access {
        ContextAccess::Read => 0,
        ContextAccess::Wrote => 1,
        ContextAccess::Referenced => 2,
    }
}

const fn access_from_tag(tag: u8) -> Option<ContextAccess> {
    match tag {
        0 => Some(ContextAccess::Read),
        1 => Some(ContextAccess::Wrote),
        2 => Some(ContextAccess::Referenced),
        _ => None,
    }
}

/// A reader that answers `None` the moment it is asked for more than it has, so a malformed body
/// is a `None` rather than a panic. Every failure path in this module is a report.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(count)?;
        let slice = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(slice)
    }

    fn digest(&mut self) -> Option<RecordDigest> {
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(self.take(32)?);
        Some(RecordDigest::from_bytes(bytes))
    }

    fn uuid(&mut self) -> Option<EntityUuid> {
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(self.take(16)?);
        Some(EntityUuid::from_bytes(bytes))
    }

    fn integer(&mut self) -> Option<u64> {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(self.take(8)?);
        Some(u64::from_be_bytes(bytes))
    }

    fn length(&mut self) -> Option<usize> {
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(self.take(4)?);
        Some(u32::from_be_bytes(bytes) as usize)
    }

    fn tag(&mut self) -> Option<u8> {
        self.take(1).map(|slice| slice[0])
    }

    /// Whether every byte was consumed. Trailing bytes inside a frame mean the writer and the
    /// reader disagree about the record, which is a malformed body and not a tolerable extension.
    const fn is_exhausted(&self) -> bool {
        self.at == self.bytes.len()
    }
}

fn read_body(kind: RecordKind, body: &[u8]) -> Option<StoredRecord> {
    let mut reader = Reader::new(body);
    let record = match kind {
        RecordKind::Operation => {
            let id = reader.digest()?;
            let actor = reader.digest()?;
            let actor_sequence = reader.integer()?;
            let hlc_millis = reader.integer()?;
            let hlc_counter = reader.integer()?;
            let policy_epoch = reader.integer()?;
            let session = reader.uuid()?;
            let payload_digest = reader.digest()?;
            let count = reader.length()?;
            let mut parents = Vec::with_capacity(count.min(1024));
            for _ in 0..count {
                parents.push(reader.digest()?);
            }
            StoredRecord::Operation(OperationRecord {
                id,
                actor,
                actor_sequence,
                hlc_millis,
                hlc_counter,
                policy_epoch,
                session,
                payload_digest,
                parents,
            })
        }
        RecordKind::Manifest => {
            let id = reader.digest()?;
            let byte_length = reader.integer()?;
            let content_digest = reader.digest()?;
            let count = reader.length()?;
            let mut chunks = Vec::with_capacity(count.min(1024));
            for _ in 0..count {
                chunks.push(ChunkSlice {
                    digest: reader.digest()?,
                    byte_offset: reader.integer()?,
                    byte_length: reader.integer()?,
                });
            }
            StoredRecord::Manifest(ManifestRecord {
                id,
                byte_length,
                content_digest,
                chunks,
            })
        }
        RecordKind::Peer => StoredRecord::Peer(PeerRecord {
            peer: reader.digest()?,
            joined_at: reader.digest()?,
        }),
        RecordKind::Acknowledgement => StoredRecord::Acknowledgement(AckRecord {
            peer: reader.digest()?,
            actor: reader.digest()?,
            actor_sequence: reader.integer()?,
        }),
        RecordKind::Review => StoredRecord::Review(ReviewRecord {
            bundle: reader.digest()?,
            subject_operation: reader.digest()?,
            opened_by: reader.digest()?,
        }),
        RecordKind::Approval => StoredRecord::Approval(ApprovalRecord {
            approval: reader.digest()?,
            bundle: reader.digest()?,
            approver: reader.digest()?,
            verdict: verdict_from_tag(reader.tag()?)?,
        }),
        RecordKind::ContextEntry => StoredRecord::ContextEntry(ContextRecord {
            entry: reader.digest()?,
            session: reader.uuid()?,
            operation: reader.digest()?,
            access: access_from_tag(reader.tag()?)?,
            byte_length: reader.integer()?,
        }),
    };
    reader.is_exhausted().then_some(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::no_session;

    fn digest(seed: u8) -> RecordDigest {
        RecordDigest::from_bytes([seed; 32])
    }

    fn operation(id: u8, actor: u8, sequence: u64, parents: &[u8]) -> StoredRecord {
        StoredRecord::Operation(OperationRecord {
            id: digest(id),
            actor: digest(actor),
            actor_sequence: sequence,
            hlc_millis: 1_700_000_000_000,
            hlc_counter: sequence,
            policy_epoch: 3,
            session: no_session(),
            payload_digest: digest(id.wrapping_add(100)),
            parents: parents.iter().map(|seed| digest(*seed)).collect(),
        })
    }

    /// One of every kind, so the codec is never exercised over a single shape.
    fn every_kind() -> Vec<StoredRecord> {
        vec![
            operation(1, 2, 1, &[]),
            operation(3, 2, 2, &[1]),
            StoredRecord::Manifest(ManifestRecord {
                id: digest(20),
                byte_length: 30,
                content_digest: digest(21),
                chunks: vec![
                    ChunkSlice {
                        digest: digest(22),
                        byte_offset: 0,
                        byte_length: 10,
                    },
                    ChunkSlice {
                        digest: digest(23),
                        byte_offset: 10,
                        byte_length: 20,
                    },
                ],
            }),
            StoredRecord::Peer(PeerRecord {
                peer: digest(30),
                joined_at: digest(1),
            }),
            StoredRecord::Acknowledgement(AckRecord {
                peer: digest(30),
                actor: digest(2),
                actor_sequence: 1,
            }),
            StoredRecord::Review(ReviewRecord {
                bundle: digest(40),
                subject_operation: digest(3),
                opened_by: digest(2),
            }),
            StoredRecord::Approval(ApprovalRecord {
                approval: digest(41),
                bundle: digest(40),
                approver: digest(2),
                verdict: ReviewVerdict::Withdrawn,
            }),
            StoredRecord::ContextEntry(ContextRecord {
                entry: digest(50),
                session: no_session(),
                operation: digest(1),
                access: ContextAccess::Referenced,
                byte_length: 60,
            }),
        ]
    }

    fn journal_bytes(records: &[StoredRecord]) -> Vec<u8> {
        records.iter().flat_map(frame_record).collect()
    }

    #[test]
    fn every_record_kind_survives_a_frame() {
        let records = every_kind();
        let scan = scan_journal(&journal_bytes(&records)).expect("scans");
        assert_eq!(scan.records(), records.as_slice());
        assert_eq!(scan.tail(), TailResidue::Whole);
    }

    #[test]
    fn the_tags_are_stable_and_distinct() {
        let mut tags: Vec<u8> = RecordKind::ALL.iter().map(|kind| kind_tag(*kind)).collect();
        for kind in RecordKind::ALL {
            assert_eq!(kind_from_tag(kind_tag(*kind)), Some(*kind));
        }
        tags.sort_unstable();
        tags.dedup();
        assert_eq!(tags.len(), RecordKind::ALL.len());
        assert!(kind_from_tag(0).is_none());
    }

    #[test]
    fn an_empty_journal_has_a_boundary_of_nothing() {
        let scan = scan_journal(&[]).expect("scans");
        assert_eq!(scan.boundary(), DurableBoundary::default());
        assert_eq!(scan.tail(), TailResidue::Whole);
        assert!(scan.records().is_empty());
    }

    /// The boundary is the byte the last whole record ends at, for every prefix of the journal.
    #[test]
    fn the_boundary_advances_one_whole_record_at_a_time() {
        let records = every_kind();
        let bytes = journal_bytes(&records);
        let mut expected = 0u64;
        for (index, record) in records.iter().enumerate() {
            expected += frame_record(record).len() as u64;
            let scan = scan_journal(&bytes[..expected as usize]).expect("scans");
            assert_eq!(
                scan.boundary(),
                DurableBoundary {
                    records: index as u64 + 1,
                    byte_offset: expected,
                }
            );
        }
    }

    /// A kill during an append: every truncation that is not a frame boundary is a fragment, and
    /// the boundary is the last whole record before it.
    #[test]
    fn every_truncation_inside_a_frame_is_a_fragment_and_never_damage() {
        let records = every_kind();
        let bytes = journal_bytes(&records);
        let mut boundaries: Vec<u64> = Vec::new();
        let mut cursor = 0u64;
        for record in &records {
            cursor += frame_record(record).len() as u64;
            boundaries.push(cursor);
        }

        for length in 0..bytes.len() {
            let scan = scan_journal(&bytes[..length]).unwrap_or_else(|damage| {
                panic!("truncating to {length} bytes was reported as damage: {damage}")
            });
            let boundary = scan.boundary().byte_offset;
            let expected = boundaries
                .iter()
                .copied()
                .rfind(|end| *end <= length as u64)
                .unwrap_or(0);
            assert_eq!(boundary, expected, "at {length} bytes");
            assert_eq!(
                scan.tail(),
                if boundary == length as u64 {
                    TailResidue::Whole
                } else {
                    TailResidue::Fragment {
                        bytes: length as u64 - boundary,
                    }
                }
            );
        }
    }

    /// The fourth criterion. A whole frame whose bytes changed is reported, at its own offset,
    /// with the intact prefix named — and no record after it is produced.
    #[test]
    fn a_flipped_byte_anywhere_in_a_frame_is_reported_and_never_skipped() {
        let records = every_kind();
        let clean = journal_bytes(&records);
        let first = frame_record(&records[0]).len();

        for position in 0..clean.len() {
            let mut damaged = clean.clone();
            damaged[position] ^= 0b0100_0000;
            let error = scan_journal(&damaged)
                .expect_err("a changed byte is never a successful scan")
                .kind();
            assert!(
                matches!(
                    error,
                    DamageKind::ChecksumMismatch { .. }
                        | DamageKind::HeaderDamaged { .. }
                        | DamageKind::NotAFrame
                ),
                "byte {position} produced {error}"
            );
        }

        // And the report locates it rather than merely refusing.
        let mut damaged = clean.clone();
        damaged[first + HEADER_BYTES + 4] ^= 0xff;
        let damage = scan_journal(&damaged).expect_err("damaged");
        assert_eq!(damage.ordinal(), 1);
        assert_eq!(damage.byte_offset(), first as u64);
        assert_eq!(damage.intact_prefix().records, 1);
        assert!(damage.to_string().contains("Nothing was skipped"));
    }

    /// A frame whose length field is enlarged turns the rest of the journal into its own body. It
    /// must not read as a fragment that silently drops every later record, and it is the reason the
    /// header carries its own checksum.
    #[test]
    fn an_enlarged_length_field_is_damage_rather_than_a_quiet_truncation() {
        let records = every_kind();
        let mut bytes = journal_bytes(&records);
        let inflated =
            (u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) + 8).to_be_bytes();
        bytes[4..8].copy_from_slice(&inflated);
        let damage = scan_journal(&bytes).expect_err("damaged");
        assert_eq!(damage.ordinal(), 0);
        assert!(matches!(damage.kind(), DamageKind::HeaderDamaged { .. }));
    }

    #[test]
    fn a_body_the_checksum_agrees_with_and_the_reader_does_not_is_malformed() {
        // Frame an operation, then rewrite the frame as though it were a peer record: the checksum
        // is recomputed, so only the body's shape is wrong.
        let record = operation(1, 2, 1, &[]);
        let framed = frame_record(&record);
        let body = &framed[HEADER_BYTES..framed.len() - CHECKSUM_BYTES];

        let damage = scan_journal(&reframe(kind_tag(RecordKind::Peer), FRAME_VERSION, body))
            .expect_err("damaged");
        assert_eq!(damage.kind(), DamageKind::MalformedBody);
    }

    /// A well-formed frame this build cannot read is named, never treated as a torn tail: the two
    /// call for opposite responses and only one of them is safe to take silently.
    #[test]
    fn a_frame_from_a_layout_or_a_kind_this_build_does_not_know_is_named_as_such() {
        let framed = frame_record(&operation(1, 2, 1, &[]));
        let body = &framed[HEADER_BYTES..framed.len() - CHECKSUM_BYTES];

        assert_eq!(
            scan_journal(&reframe(
                kind_tag(RecordKind::Operation),
                FRAME_VERSION + 1,
                body
            ))
            .expect_err("damaged")
            .kind(),
            DamageKind::UnknownFrameVersion {
                found: FRAME_VERSION + 1
            }
        );
        assert_eq!(
            scan_journal(&reframe(200, FRAME_VERSION, body))
                .expect_err("damaged")
                .kind(),
            DamageKind::UnknownRecordKind { found: 200 }
        );
    }

    /// Build a frame around a body with a chosen tag and layout version, checksums recomputed, so a
    /// test can change one declared fact without changing anything else.
    fn reframe(tag: u8, version: u8, body: &[u8]) -> Vec<u8> {
        let mut framed = Vec::new();
        framed.extend_from_slice(&FRAME_MAGIC);
        framed.push(version);
        framed.push(tag);
        framed.extend_from_slice(&(body.len() as u32).to_be_bytes());
        framed.extend_from_slice(checksum(&framed).as_bytes());
        framed.extend_from_slice(body);
        framed.extend_from_slice(checksum(&framed).as_bytes());
        framed
    }

    #[test]
    fn bytes_that_are_not_a_frame_at_all_are_named_as_such() {
        let noise = b"this file is not a mesh record journal at all";
        assert!(noise.len() > HEADER_BYTES);
        let damage = scan_journal(noise).expect_err("damaged");
        assert_eq!(damage.kind(), DamageKind::NotAFrame);
        assert_eq!(damage.byte_offset(), 0);
    }

    /// The one place the classification is unavoidably generous, stated rather than left to be
    /// discovered: fewer bytes than a header cannot be checked against anything, so they read as an
    /// interrupted append. That is the safe direction — the boundary is still exact and nothing
    /// after it is invented — and it only ever applies to the final bytes of a file.
    #[test]
    fn fewer_bytes_than_a_header_are_a_fragment_because_nothing_can_verify_them() {
        let scan = scan_journal(b"too short").expect("scans");
        assert_eq!(scan.boundary(), DurableBoundary::default());
        assert_eq!(scan.tail(), TailResidue::Fragment { bytes: 9 });
    }

    /// The scan hands the rebuild its records, and the rebuild is the same one the live path uses.
    #[test]
    fn a_scanned_journal_rebuilds_the_index_the_records_describe() {
        let records = every_kind();
        let scan = scan_journal(&journal_bytes(&records)).expect("scans");
        let (_, report) = rebuild(scan.into_records(), Vec::new()).expect("rebuilds");
        let (_, direct) = rebuild(records, Vec::new()).expect("rebuilds");
        assert_eq!(report.digest, direct.digest);
    }

    /// A journal that fails on write leaves whole frames behind it, never half of one.
    #[test]
    fn journalling_reports_the_bytes_it_made_durable() {
        struct Sink {
            appended: Vec<u8>,
            fail_after: usize,
        }
        impl RecordJournal for Sink {
            type Error = String;
            fn read_all(&mut self) -> Result<Vec<u8>, String> {
                Ok(self.appended.clone())
            }
            fn append(&mut self, framed: &[u8]) -> Result<(), String> {
                if self.fail_after == 0 {
                    return Err("no space left on device".to_owned());
                }
                self.fail_after -= 1;
                self.appended.extend_from_slice(framed);
                Ok(())
            }
        }

        let records = every_kind();
        let mut sink = Sink {
            appended: Vec::new(),
            fail_after: 3,
        };
        let error = journal_records(&mut sink, &records).expect_err("the journal fills up");
        assert_eq!(error, "no space left on device");

        let scan = scan_journal(&sink.appended).expect("the prefix is whole");
        assert_eq!(scan.boundary().records, 3);
        assert_eq!(scan.tail(), TailResidue::Whole);
    }
}
