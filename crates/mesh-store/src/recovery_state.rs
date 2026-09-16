//! Recovery preservation without confusing recoverable bytes with a meaningful save.
//!
//! This module is the production state machine for `mesh-recovery-preservation/0`. It owns no
//! scheduler and chooses no time or byte threshold. A caller supplies one of the nine named
//! triggers, and this module enforces the result class, ordering, durable-pointer update, restart
//! restoration, and acknowledgement boundary.

use core::fmt;

use crate::{Digest16, PrivateSaved, RecordDigest};

/// The exact compatibility contract implemented by this module.
pub const RECOVERY_PRESERVATION_CONTRACT: &str = "mesh-recovery-preservation/0";

/// A sequence point in one view's open activity window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecoverySequence(u64);

impl RecoverySequence {
    /// Construct a non-zero sequence point.
    #[must_use]
    pub const fn new(value: u64) -> Option<Self> {
        if value == 0 {
            None
        } else {
            Some(Self(value))
        }
    }

    /// The underlying sequence value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A ULID in its 16-byte binary form.
///
/// ULID text sorts by these bytes, so deriving `Ord` preserves the contract's lexical event-ULID
/// ordering without admitting a second textual spelling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecoveryEventUlid([u8; 16]);

impl RecoveryEventUlid {
    /// Wrap bytes produced by the repository's ULID generator.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// The canonical binary ordering bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

/// The only ordering key for meaningful and recovery records.
///
/// Field declaration order is load-bearing: derived ordering is Lamport value, event ULID, then
/// content hash. Wall-clock time has no field and therefore cannot become a tiebreaker.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecoveryStamp {
    lamport: u64,
    event_ulid: RecoveryEventUlid,
    content_hash: RecordDigest,
}

impl RecoveryStamp {
    /// Build an ordering stamp.
    #[must_use]
    pub const fn new(
        lamport: u64,
        event_ulid: RecoveryEventUlid,
        content_hash: RecordDigest,
    ) -> Self {
        Self {
            lamport,
            event_ulid,
            content_hash,
        }
    }

    /// The Lamport component.
    #[must_use]
    pub const fn lamport(self) -> u64 {
        self.lamport
    }

    /// The event-ULID component.
    #[must_use]
    pub const fn event_ulid(self) -> RecoveryEventUlid {
        self.event_ulid
    }

    /// The content-hash component.
    #[must_use]
    pub const fn content_hash(self) -> RecordDigest {
        self.content_hash
    }
}

/// One of plan §4.5's nine conditions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RecoveryTrigger {
    /// The activity window is proven settled after its final event.
    ActorBecameIdleAfterSettling,
    /// An integrated agent requested a flush.
    IntegratedAgentRequestsFlush,
    /// The actor process exited.
    ActorProcessExited,
    /// The configured uncheckpointed byte or time bound was reached.
    MaximumUncheckpointedBytesOrTime,
    /// A user opened review.
    UserOpenedReview,
    /// The actor disconnected.
    ActorDisconnected,
    /// A modified file handle closed.
    ModifiedFileHandleClosed,
    /// A successful fsync completed.
    FsyncCompleted,
    /// An atomic replacement completed.
    AtomicReplacementCompleted,
}

impl RecoveryTrigger {
    /// Every trigger, in contract order.
    pub const ALL: [Self; 9] = [
        Self::ActorBecameIdleAfterSettling,
        Self::IntegratedAgentRequestsFlush,
        Self::ActorProcessExited,
        Self::MaximumUncheckpointedBytesOrTime,
        Self::UserOpenedReview,
        Self::ActorDisconnected,
        Self::ModifiedFileHandleClosed,
        Self::FsyncCompleted,
        Self::AtomicReplacementCompleted,
    ];

    /// The closed result class for this trigger.
    #[must_use]
    pub const fn effect(self) -> TriggerEffect {
        match self {
            Self::ActorBecameIdleAfterSettling => TriggerEffect::MeaningfulAfterSettling,
            Self::IntegratedAgentRequestsFlush
            | Self::ActorProcessExited
            | Self::MaximumUncheckpointedBytesOrTime
            | Self::UserOpenedReview
            | Self::ActorDisconnected => TriggerEffect::RecoveryOnly,
            Self::ModifiedFileHandleClosed
            | Self::FsyncCompleted
            | Self::AtomicReplacementCompleted => TriggerEffect::EvidenceOnly,
        }
    }
}

/// The three result classes selected by the nine triggers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TriggerEffect {
    /// The only class allowed to close the window and carry a prior `PrivateSaved` value.
    MeaningfulAfterSettling,
    /// Durable recovery bytes only; the meaningful window remains open.
    RecoveryOnly,
    /// Candidate evidence only; no durable pointer moves.
    EvidenceOnly,
}

/// The three boundary-evidence kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BoundaryEvidenceKind {
    /// A modified handle closed.
    Closed,
    /// Fsync returned success.
    Synced,
    /// Atomic replacement completed.
    RenamedIntoPlace,
}

/// Boundary evidence recorded inside an open window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RecoveryBoundaryEvidence {
    through: RecoverySequence,
    kind: BoundaryEvidenceKind,
}

impl RecoveryBoundaryEvidence {
    /// Construct evidence at one observed prefix.
    #[must_use]
    pub const fn new(through: RecoverySequence, kind: BoundaryEvidenceKind) -> Self {
        Self { through, kind }
    }

    /// The last event this evidence can see.
    #[must_use]
    pub const fn through(self) -> RecoverySequence {
        self.through
    }

    /// What boundary was observed.
    #[must_use]
    pub const fn kind(self) -> BoundaryEvidenceKind {
        self.kind
    }
}

/// Verified bytes preserved for recovery, never a meaningful checkpoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryPreserved {
    stamp: RecoveryStamp,
    through: RecoverySequence,
    bytes: Vec<u8>,
    verified_content_hash: RecordDigest,
}

impl RecoveryPreserved {
    /// Construct a record after a content verifier returned `verified_content_hash`.
    ///
    /// # Errors
    ///
    /// [`RecoveryStateError::ContentHashMismatch`] when the verifier's result is not the hash in
    /// the ordering stamp, or [`RecoveryStateError::EmptyRecoveryBytes`] for an empty payload.
    pub fn from_verified_bytes(
        stamp: RecoveryStamp,
        through: RecoverySequence,
        bytes: Vec<u8>,
        verified_content_hash: RecordDigest,
    ) -> Result<Self, RecoveryStateError> {
        if bytes.is_empty() {
            return Err(RecoveryStateError::EmptyRecoveryBytes);
        }
        if stamp.content_hash != verified_content_hash {
            return Err(RecoveryStateError::ContentHashMismatch);
        }
        Ok(Self {
            stamp,
            through,
            bytes,
            verified_content_hash,
        })
    }

    /// The ordering stamp.
    #[must_use]
    pub const fn stamp(&self) -> RecoveryStamp {
        self.stamp
    }

    /// The exact view prefix represented by these bytes.
    #[must_use]
    pub const fn through(&self) -> RecoverySequence {
        self.through
    }

    /// The immutable recovery bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The verifier result admitted alongside the bytes.
    #[must_use]
    pub const fn verified_content_hash(&self) -> RecordDigest {
        self.verified_content_hash
    }
}

/// A meaningful checkpoint that was acknowledged by the existing durable-save sequence.
///
/// Restart restores this value but cannot reconstruct or replay a `PrivateSaved` value from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeaningfulCheckpoint {
    stamp: RecoveryStamp,
    through: RecoverySequence,
    index_digest: Digest16,
    operations: usize,
    manifests: usize,
    chunks: usize,
}

/// A journal-backed private save waiting for the configured idle interval.
///
/// The value is durable so a process restart cannot lose the only path from an already committed
/// save to the meaningful checkpoint boundary. It is not itself a `PrivateSaved` acknowledgement:
/// recovery must still reproduce `index_digest` from immutable records before that type can be
/// reconstructed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingMeaningfulSave {
    through: RecoverySequence,
    stamp: RecoveryStamp,
    index_digest: Digest16,
    operations: usize,
    manifests: usize,
    chunks: usize,
}

impl PendingMeaningfulSave {
    fn from_acknowledgement(
        through: RecoverySequence,
        stamp: RecoveryStamp,
        saved: &PrivateSaved,
    ) -> Self {
        Self {
            through,
            stamp,
            index_digest: saved.index_digest(),
            operations: saved.operations(),
            manifests: saved.manifests(),
            chunks: saved.chunks(),
        }
    }

    /// The exact final event this save can settle.
    #[must_use]
    pub const fn through(self) -> RecoverySequence {
        self.through
    }

    /// The durable ChangeSet-derived ordering stamp.
    #[must_use]
    pub const fn stamp(self) -> RecoveryStamp {
        self.stamp
    }

    /// The index digest that restart must reproduce before acknowledging this save.
    #[must_use]
    pub const fn index_digest(self) -> Digest16 {
        self.index_digest
    }

    /// Counts carried by the original post-transaction acknowledgement.
    #[must_use]
    pub const fn counts(self) -> (usize, usize, usize) {
        (self.operations, self.manifests, self.chunks)
    }
}

impl MeaningfulCheckpoint {
    fn from_acknowledgement(
        stamp: RecoveryStamp,
        through: RecoverySequence,
        saved: &PrivateSaved,
    ) -> Self {
        Self {
            stamp,
            through,
            index_digest: saved.index_digest(),
            operations: saved.operations(),
            manifests: saved.manifests(),
            chunks: saved.chunks(),
        }
    }

    /// The ordering stamp.
    #[must_use]
    pub const fn stamp(self) -> RecoveryStamp {
        self.stamp
    }

    /// The complete settled prefix.
    #[must_use]
    pub const fn through(self) -> RecoverySequence {
        self.through
    }

    /// The index digest carried by the meaningful acknowledgement.
    #[must_use]
    pub const fn index_digest(self) -> Digest16 {
        self.index_digest
    }

    /// Counts carried by the meaningful acknowledgement.
    #[must_use]
    pub const fn counts(self) -> (usize, usize, usize) {
        (self.operations, self.manifests, self.chunks)
    }
}

/// The open activity window, including only evidence that has actually occurred.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecoveryWindow {
    from: RecoverySequence,
    last: RecoverySequence,
    latest_evidence: Option<RecoveryBoundaryEvidence>,
}

impl RecoveryWindow {
    /// The first observed sequence in the window.
    #[must_use]
    pub const fn from(self) -> RecoverySequence {
        self.from
    }

    /// The last observed sequence in the window.
    #[must_use]
    pub const fn last(self) -> RecoverySequence {
        self.last
    }

    /// The latest candidate evidence, if one was supplied.
    #[must_use]
    pub const fn latest_evidence(self) -> Option<RecoveryBoundaryEvidence> {
        self.latest_evidence
    }
}

/// Durable state needed to restore the machine after a restart.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecoverySnapshot {
    last_meaningful: Option<MeaningfulCheckpoint>,
    latest_recovery: Option<RecoveryPreserved>,
    open_window: Option<RecoveryWindow>,
    pending_meaningful: Option<PendingMeaningfulSave>,
}

impl RecoverySnapshot {
    /// The last complete meaningful checkpoint.
    #[must_use]
    pub const fn last_meaningful(&self) -> Option<&MeaningfulCheckpoint> {
        self.last_meaningful.as_ref()
    }

    /// The newest verified recovery record.
    #[must_use]
    pub const fn latest_recovery(&self) -> Option<&RecoveryPreserved> {
        self.latest_recovery.as_ref()
    }

    /// The window that remains open, including after recovery-only restart.
    #[must_use]
    pub const fn open_window(&self) -> Option<RecoveryWindow> {
        self.open_window
    }

    /// The durable save that can close the current window after restart and verification.
    #[must_use]
    pub const fn pending_meaningful(&self) -> Option<PendingMeaningfulSave> {
        self.pending_meaningful
    }

    /// Whether this is the next journal-backed observation after `prior`.
    ///
    /// The recovery slot may advance only within the same open window. Meaningful and recovery
    /// pointers remain byte-identical, while boundary evidence may stay unchanged or advance.
    pub(crate) fn continues_observation_recovery_from(&self, prior: &Self) -> bool {
        let (Some(next_window), Some(prior_window)) = (self.open_window, prior.open_window) else {
            return false;
        };
        let (Some(next_pending), Some(prior_pending)) =
            (self.pending_meaningful, prior.pending_meaningful)
        else {
            return false;
        };
        let evidence_continues = match (prior_window.latest_evidence, next_window.latest_evidence) {
            (None, _) => true,
            (Some(_), None) => false,
            (Some(prior), Some(next)) => next.through >= prior.through,
        };
        self.last_meaningful == prior.last_meaningful
            && self.latest_recovery == prior.latest_recovery
            && next_window.from == prior_window.from
            && evidence_continues
            && next_window.last == next_pending.through
            && prior_window.last == prior_pending.through
            && next_pending.through > prior_pending.through
    }

    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"mesh-recovery-state\0");
        out.push(2);
        let mut flags = 0u8;
        if self.last_meaningful.is_some() {
            flags |= 1;
        }
        if self.latest_recovery.is_some() {
            flags |= 2;
        }
        if self.open_window.is_some() {
            flags |= 4;
        }
        if self.pending_meaningful.is_some() {
            flags |= 8;
        }
        out.push(flags);
        if let Some(checkpoint) = self.last_meaningful {
            encode_stamp(&mut out, checkpoint.stamp);
            encode_u64(&mut out, checkpoint.through.get());
            out.extend_from_slice(checkpoint.index_digest.as_bytes());
            encode_u64(&mut out, checkpoint.operations as u64);
            encode_u64(&mut out, checkpoint.manifests as u64);
            encode_u64(&mut out, checkpoint.chunks as u64);
        }
        if let Some(recovery) = self.latest_recovery.as_ref() {
            encode_stamp(&mut out, recovery.stamp);
            encode_u64(&mut out, recovery.through.get());
            encode_u64(&mut out, recovery.bytes.len() as u64);
            out.extend_from_slice(&recovery.bytes);
            out.extend_from_slice(recovery.verified_content_hash.as_bytes());
        }
        if let Some(window) = self.open_window {
            encode_u64(&mut out, window.from.get());
            encode_u64(&mut out, window.last.get());
            match window.latest_evidence {
                Some(evidence) => {
                    out.push(1);
                    encode_u64(&mut out, evidence.through.get());
                    out.push(match evidence.kind {
                        BoundaryEvidenceKind::Closed => 1,
                        BoundaryEvidenceKind::Synced => 2,
                        BoundaryEvidenceKind::RenamedIntoPlace => 3,
                    });
                }
                None => out.push(0),
            }
        }
        if let Some(pending) = self.pending_meaningful {
            encode_u64(&mut out, pending.through.get());
            encode_stamp(&mut out, pending.stamp);
            out.extend_from_slice(pending.index_digest.as_bytes());
            encode_u64(&mut out, pending.operations as u64);
            encode_u64(&mut out, pending.manifests as u64);
            encode_u64(&mut out, pending.chunks as u64);
        }
        out
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, RecoveryStateError> {
        let mut reader = SnapshotReader::new(bytes);
        if reader.take(b"mesh-recovery-state\0".len())? != b"mesh-recovery-state\0" {
            return Err(RecoveryStateError::InvalidSnapshot);
        }
        let version = reader.byte()?;
        if !matches!(version, 1 | 2) {
            return Err(RecoveryStateError::InvalidSnapshot);
        }
        let flags = reader.byte()?;
        let allowed_flags = if version == 1 { 7 } else { 15 };
        if flags & !allowed_flags != 0 {
            return Err(RecoveryStateError::InvalidSnapshot);
        }
        let last_meaningful = if flags & 1 != 0 {
            let stamp = reader.stamp()?;
            let through = reader.sequence()?;
            let index_digest = Digest16::from_bytes(reader.array()?);
            let operations =
                usize::try_from(reader.u64()?).map_err(|_| RecoveryStateError::InvalidSnapshot)?;
            let manifests =
                usize::try_from(reader.u64()?).map_err(|_| RecoveryStateError::InvalidSnapshot)?;
            let chunks =
                usize::try_from(reader.u64()?).map_err(|_| RecoveryStateError::InvalidSnapshot)?;
            Some(MeaningfulCheckpoint {
                stamp,
                through,
                index_digest,
                operations,
                manifests,
                chunks,
            })
        } else {
            None
        };
        let latest_recovery = if flags & 2 != 0 {
            let stamp = reader.stamp()?;
            let through = reader.sequence()?;
            let byte_len =
                usize::try_from(reader.u64()?).map_err(|_| RecoveryStateError::InvalidSnapshot)?;
            let recovery_bytes = reader.take(byte_len)?.to_vec();
            let verified_content_hash = RecordDigest::from_bytes(reader.array()?);
            Some(RecoveryPreserved::from_verified_bytes(
                stamp,
                through,
                recovery_bytes,
                verified_content_hash,
            )?)
        } else {
            None
        };
        let open_window = if flags & 4 != 0 {
            let from = reader.sequence()?;
            let last = reader.sequence()?;
            let latest_evidence = match reader.byte()? {
                0 => None,
                1 => {
                    let through = reader.sequence()?;
                    let kind = match reader.byte()? {
                        1 => BoundaryEvidenceKind::Closed,
                        2 => BoundaryEvidenceKind::Synced,
                        3 => BoundaryEvidenceKind::RenamedIntoPlace,
                        _ => return Err(RecoveryStateError::InvalidSnapshot),
                    };
                    Some(RecoveryBoundaryEvidence { through, kind })
                }
                _ => return Err(RecoveryStateError::InvalidSnapshot),
            };
            Some(RecoveryWindow {
                from,
                last,
                latest_evidence,
            })
        } else {
            None
        };
        let pending_meaningful = if flags & 8 != 0 {
            let through = reader.sequence()?;
            let stamp = reader.stamp()?;
            let index_digest = Digest16::from_bytes(reader.array()?);
            let operations =
                usize::try_from(reader.u64()?).map_err(|_| RecoveryStateError::InvalidSnapshot)?;
            let manifests =
                usize::try_from(reader.u64()?).map_err(|_| RecoveryStateError::InvalidSnapshot)?;
            let chunks =
                usize::try_from(reader.u64()?).map_err(|_| RecoveryStateError::InvalidSnapshot)?;
            Some(PendingMeaningfulSave {
                through,
                stamp,
                index_digest,
                operations,
                manifests,
                chunks,
            })
        } else {
            None
        };
        if !reader.is_empty() {
            return Err(RecoveryStateError::InvalidSnapshot);
        }
        let snapshot = Self {
            last_meaningful,
            latest_recovery,
            open_window,
            pending_meaningful,
        };
        validate_snapshot(&snapshot)?;
        Ok(snapshot)
    }
}

fn encode_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn encode_stamp(out: &mut Vec<u8>, stamp: RecoveryStamp) {
    encode_u64(out, stamp.lamport);
    out.extend_from_slice(stamp.event_ulid.as_bytes());
    out.extend_from_slice(stamp.content_hash.as_bytes());
}

struct SnapshotReader<'a> {
    remaining: &'a [u8],
}

impl<'a> SnapshotReader<'a> {
    const fn new(remaining: &'a [u8]) -> Self {
        Self { remaining }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], RecoveryStateError> {
        if self.remaining.len() < length {
            return Err(RecoveryStateError::InvalidSnapshot);
        }
        let (value, remaining) = self.remaining.split_at(length);
        self.remaining = remaining;
        Ok(value)
    }

    fn byte(&mut self) -> Result<u8, RecoveryStateError> {
        Ok(self.take(1)?[0])
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], RecoveryStateError> {
        self.take(N)?
            .try_into()
            .map_err(|_| RecoveryStateError::InvalidSnapshot)
    }

    fn u64(&mut self) -> Result<u64, RecoveryStateError> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    fn sequence(&mut self) -> Result<RecoverySequence, RecoveryStateError> {
        RecoverySequence::new(self.u64()?).ok_or(RecoveryStateError::InvalidSnapshot)
    }

    fn stamp(&mut self) -> Result<RecoveryStamp, RecoveryStateError> {
        Ok(RecoveryStamp::new(
            self.u64()?,
            RecoveryEventUlid::from_bytes(self.array()?),
            RecordDigest::from_bytes(self.array()?),
        ))
    }

    const fn is_empty(&self) -> bool {
        self.remaining.is_empty()
    }
}

/// Atomic durable ownership for the recovery snapshot.
///
/// Implementations must make the whole snapshot visible at once. Bytes written before the pointer
/// update are unadmitted and `load` must return the prior snapshot after that crash stage.
pub trait RecoveryStatePersistence {
    /// The persistence failure.
    type Error;

    /// Load the last atomically admitted snapshot.
    ///
    /// # Errors
    ///
    /// Whatever the durable store reports.
    fn load(&mut self) -> Result<Option<RecoverySnapshot>, Self::Error>;

    /// Atomically replace the admitted snapshot.
    ///
    /// # Errors
    ///
    /// Whatever the durable store reports. On error the prior admitted snapshot must remain the
    /// value returned by [`Self::load`].
    fn persist(&mut self, snapshot: &RecoverySnapshot) -> Result<(), Self::Error>;

    /// Retain a journal-backed prospective snapshot when replacing the primary snapshot failed.
    ///
    /// The distinct recovery slot must win on load until a successful primary persist clears it.
    /// On error neither the primary snapshot nor an earlier recovery slot may change.
    fn persist_observation_recovery(
        &mut self,
        snapshot: &RecoverySnapshot,
    ) -> Result<(), Self::Error>;
}

/// Input whose shape must match the trigger's fixed result class.
#[derive(Debug)]
pub enum RecoveryTriggerInput {
    /// Candidate boundary evidence.
    Evidence(RecoveryBoundaryEvidence),
    /// Verified recovery bytes.
    Recovery(RecoveryPreserved),
    /// A settled checkpoint plus the acknowledgement produced by its durable transaction.
    Meaningful {
        /// Ordering and extent of the checkpoint.
        stamp: RecoveryStamp,
        /// The final event in the settled window.
        through: RecoverySequence,
        /// Existing proof that the durable checkpoint transaction returned.
        acknowledgement: PrivateSaved,
    },
}

/// One successful or idempotent transition.
#[derive(Debug, PartialEq, Eq)]
pub enum RecoveryTransition {
    /// Evidence was recorded; no durable pointer moved.
    BoundaryEvidenceRecorded(RecoveryBoundaryEvidence),
    /// `latest_recovery` advanced; there is deliberately no `PrivateSaved` field.
    RecoveryPreserved(RecoveryPreserved),
    /// `last_meaningful` advanced and carries the pre-existing acknowledgement.
    MeaningfulSaved {
        /// The durable meaningful point.
        checkpoint: MeaningfulCheckpoint,
        /// The acknowledgement supplied by the durable save sequence.
        acknowledgement: PrivateSaved,
    },
    /// An exact stamp duplicate changed nothing.
    Duplicate,
    /// A lower stamp changed nothing.
    OlderIgnored,
}

impl RecoveryTransition {
    /// The meaningful acknowledgement, present for one transition variant only.
    #[must_use]
    pub const fn private_saved(&self) -> Option<&PrivateSaved> {
        match self {
            Self::MeaningfulSaved {
                acknowledgement, ..
            } => Some(acknowledgement),
            Self::BoundaryEvidenceRecorded(_)
            | Self::RecoveryPreserved(_)
            | Self::Duplicate
            | Self::OlderIgnored => None,
        }
    }
}

/// The six product states, closed in product order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryProductStatus {
    /// Active work, including recovery-preserved work.
    Working,
    /// A meaningful checkpoint was acknowledged locally.
    SavedPrivately,
    /// A peer can open that checkpoint.
    AvailableToTeam,
    /// A frozen review bundle exists.
    ReadyForReview,
    /// A person must decide what happens next.
    NeedsAttention,
    /// Signed publication succeeded.
    Approved,
}

/// A transition or restored snapshot violated the contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryStateError {
    /// No activity window is open.
    NoOpenWindow,
    /// A sequence moved backwards or lay outside the open window.
    SequenceOutsideWindow,
    /// A trigger received an input from another result class.
    WrongInputForTrigger,
    /// Boundary evidence kind disagreed with its trigger.
    WrongEvidenceForTrigger,
    /// The verifier's digest disagreed with the ordering stamp.
    ContentHashMismatch,
    /// Recovery preservation cannot write an empty byte object.
    EmptyRecoveryBytes,
    /// One ordering stamp was reused for different content or extent.
    ConflictingDuplicate,
    /// Restored state violated an extent or ordering invariant.
    InvalidSnapshot,
}

impl fmt::Display for RecoveryStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NoOpenWindow => "no recovery window is open",
            Self::SequenceOutsideWindow => "the sequence is outside the open recovery window",
            Self::WrongInputForTrigger => "the trigger input belongs to a different result class",
            Self::WrongEvidenceForTrigger => "the evidence kind does not match its trigger",
            Self::ContentHashMismatch => "verified recovery content does not match its stamp",
            Self::EmptyRecoveryBytes => "recovery preservation needs at least one byte",
            Self::ConflictingDuplicate => "one recovery stamp names conflicting state",
            Self::InvalidSnapshot => "the restored recovery snapshot is internally inconsistent",
        })
    }
}

impl std::error::Error for RecoveryStateError {}

/// Opening or applying the state machine failed.
#[derive(Debug, PartialEq, Eq)]
pub enum RecoveryMachineError<E> {
    /// The state itself violated the contract.
    State(RecoveryStateError),
    /// Persistence failed; the prior in-memory and durable snapshot remains authoritative.
    Persistence {
        /// The trigger whose prospective state was refused.
        trigger: RecoveryTrigger,
        /// What the durable owner reported.
        error: E,
    },
    /// Persisting a journal-backed activity observation failed.
    ObservationPersistence {
        /// Why the primary snapshot was not replaced.
        error: E,
        /// Why the distinct post-journal recovery slot was also unavailable.
        recovery_error: E,
    },
}

impl<E: fmt::Display> fmt::Display for RecoveryMachineError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::State(error) => error.fmt(formatter),
            Self::Persistence { trigger, error } => {
                write!(formatter, "{trigger:?} did not become durable: {error}")
            }
            Self::ObservationPersistence {
                error,
                recovery_error,
            } => {
                write!(
                    formatter,
                    "the activity observation did not become durable ({error}), and its journal-backed recovery slot also failed: {recovery_error}"
                )
            }
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for RecoveryMachineError<E> {}

impl<E> RecoveryMachineError<E> {
    /// A failed state transition projects to the closed six-state vocabulary.
    #[must_use]
    pub const fn status(&self) -> RecoveryProductStatus {
        RecoveryProductStatus::NeedsAttention
    }
}

/// The per-view recovery-preservation state machine.
#[derive(Debug)]
pub struct RecoveryMachine<P: RecoveryStatePersistence> {
    persistence: P,
    snapshot: RecoverySnapshot,
}

impl<P: RecoveryStatePersistence> RecoveryMachine<P> {
    /// Open from the last admitted durable snapshot.
    ///
    /// # Errors
    ///
    /// A persistence error, or [`RecoveryStateError::InvalidSnapshot`].
    pub fn open(mut persistence: P) -> Result<Self, RecoveryMachineError<P::Error>> {
        let snapshot = persistence
            .load()
            .map_err(|error| RecoveryMachineError::Persistence {
                trigger: RecoveryTrigger::ActorBecameIdleAfterSettling,
                error,
            })?
            .unwrap_or_default();
        validate_snapshot(&snapshot).map_err(RecoveryMachineError::State)?;
        Ok(Self {
            persistence,
            snapshot,
        })
    }

    /// Observe one event, opening or extending the activity window.
    ///
    /// # Errors
    ///
    /// [`RecoveryStateError::SequenceOutsideWindow`] if the sequence moves backwards.
    pub fn observe(&mut self, sequence: RecoverySequence) -> Result<(), RecoveryStateError> {
        observe_snapshot(&mut self.snapshot, sequence)
    }

    /// Observe one journal-backed event and atomically persist the resulting open window.
    ///
    /// This is separate from [`Self::observe`] because volatile adapter evidence must not become
    /// durable merely by being seen. Composition roots use it only after their canonical journal
    /// append returned successfully.
    pub fn observe_durable(
        &mut self,
        sequence: RecoverySequence,
        stamp: RecoveryStamp,
        acknowledgement: PrivateSaved,
    ) -> Result<(), RecoveryMachineError<P::Error>> {
        let mut next = self.snapshot.clone();
        observe_snapshot(&mut next, sequence).map_err(RecoveryMachineError::State)?;
        next.pending_meaningful = Some(PendingMeaningfulSave::from_acknowledgement(
            sequence,
            stamp,
            &acknowledgement,
        ));
        if let Err(error) = self.persistence.persist(&next) {
            if let Err(recovery_error) = self.persistence.persist_observation_recovery(&next) {
                return Err(RecoveryMachineError::ObservationPersistence {
                    error,
                    recovery_error,
                });
            }
        }
        self.snapshot = next;
        Ok(())
    }

    /// Atomically observe one event, record its boundary evidence, and preserve its recovery
    /// bytes.
    ///
    /// Native filesystem mutations use this after their operating-system write succeeds. None of
    /// the three in-memory facts becomes visible unless the combined recovery snapshot commits;
    /// callers can therefore roll the filesystem write back without leaving phantom activity.
    pub fn observe_evidence_and_recover(
        &mut self,
        sequence: RecoverySequence,
        evidence_trigger: RecoveryTrigger,
        evidence: RecoveryBoundaryEvidence,
        recovery_trigger: RecoveryTrigger,
        recovery: RecoveryPreserved,
    ) -> Result<RecoveryTransition, RecoveryMachineError<P::Error>> {
        if evidence_trigger.effect() != TriggerEffect::EvidenceOnly
            || recovery_trigger.effect() != TriggerEffect::RecoveryOnly
            || recovery_trigger == RecoveryTrigger::MaximumUncheckpointedBytesOrTime
        {
            return Err(RecoveryMachineError::State(
                RecoveryStateError::WrongInputForTrigger,
            ));
        }
        let mut next = self.snapshot.clone();
        observe_snapshot(&mut next, sequence).map_err(RecoveryMachineError::State)?;
        apply_evidence_to_snapshot(&mut next, evidence_trigger, evidence)
            .map_err(RecoveryMachineError::State)?;
        if recovery.through != sequence {
            return Err(RecoveryMachineError::State(
                RecoveryStateError::SequenceOutsideWindow,
            ));
        }
        if let Some(current) = next.latest_recovery.as_ref() {
            match recovery.stamp.cmp(&current.stamp) {
                core::cmp::Ordering::Less => {
                    return Err(RecoveryMachineError::State(
                        RecoveryStateError::SequenceOutsideWindow,
                    ));
                }
                core::cmp::Ordering::Equal => {
                    return Err(RecoveryMachineError::State(
                        RecoveryStateError::ConflictingDuplicate,
                    ));
                }
                core::cmp::Ordering::Greater => {}
            }
        }
        let window = next.open_window.ok_or(RecoveryMachineError::State(
            RecoveryStateError::NoOpenWindow,
        ))?;
        ensure_inside(window, recovery.through).map_err(RecoveryMachineError::State)?;
        next.latest_recovery = Some(recovery.clone());
        self.persist(recovery_trigger, next)?;
        Ok(RecoveryTransition::RecoveryPreserved(recovery))
    }

    /// Apply one of the nine trigger transitions.
    ///
    /// Recovery and meaningful pointer changes are made visible in memory only after the atomic
    /// persistence call succeeds. Evidence-only triggers never call persistence.
    ///
    /// # Errors
    ///
    /// A contract-shape error or persistence failure. Either leaves the prior snapshot intact.
    pub fn apply(
        &mut self,
        trigger: RecoveryTrigger,
        input: RecoveryTriggerInput,
    ) -> Result<RecoveryTransition, RecoveryMachineError<P::Error>> {
        match (trigger.effect(), input) {
            (TriggerEffect::EvidenceOnly, RecoveryTriggerInput::Evidence(evidence)) => self
                .apply_evidence(trigger, evidence)
                .map_err(RecoveryMachineError::State),
            (TriggerEffect::RecoveryOnly, RecoveryTriggerInput::Recovery(recovery)) => {
                self.apply_recovery(trigger, recovery)
            }
            (
                TriggerEffect::MeaningfulAfterSettling,
                RecoveryTriggerInput::Meaningful {
                    stamp,
                    through,
                    acknowledgement,
                },
            ) => self.apply_meaningful(trigger, stamp, through, acknowledgement),
            _ => Err(RecoveryMachineError::State(
                RecoveryStateError::WrongInputForTrigger,
            )),
        }
    }

    /// The currently admitted snapshot.
    #[must_use]
    pub const fn snapshot(&self) -> &RecoverySnapshot {
        &self.snapshot
    }

    /// The base six-state projection controlled by this machine.
    #[must_use]
    pub const fn status(&self) -> RecoveryProductStatus {
        if self.snapshot.open_window.is_some() || self.snapshot.last_meaningful.is_none() {
            RecoveryProductStatus::Working
        } else {
            RecoveryProductStatus::SavedPrivately
        }
    }

    /// Return the persistence owner, useful when transferring it to another process in tests or
    /// during an orderly restart.
    #[must_use]
    pub fn into_persistence(self) -> P {
        self.persistence
    }

    fn apply_evidence(
        &mut self,
        trigger: RecoveryTrigger,
        evidence: RecoveryBoundaryEvidence,
    ) -> Result<RecoveryTransition, RecoveryStateError> {
        apply_evidence_to_snapshot(&mut self.snapshot, trigger, evidence)?;
        Ok(RecoveryTransition::BoundaryEvidenceRecorded(evidence))
    }

    fn apply_recovery(
        &mut self,
        trigger: RecoveryTrigger,
        recovery: RecoveryPreserved,
    ) -> Result<RecoveryTransition, RecoveryMachineError<P::Error>> {
        if let Some(current) = self.snapshot.latest_recovery.as_ref() {
            match recovery.stamp.cmp(&current.stamp) {
                core::cmp::Ordering::Less => return Ok(RecoveryTransition::OlderIgnored),
                core::cmp::Ordering::Equal if recovery == *current => {
                    return Ok(RecoveryTransition::Duplicate);
                }
                core::cmp::Ordering::Equal => {
                    return Err(RecoveryMachineError::State(
                        RecoveryStateError::ConflictingDuplicate,
                    ));
                }
                core::cmp::Ordering::Greater => {}
            }
        }
        let window = self
            .snapshot
            .open_window
            .ok_or(RecoveryMachineError::State(
                RecoveryStateError::NoOpenWindow,
            ))?;
        ensure_inside(window, recovery.through).map_err(RecoveryMachineError::State)?;
        let mut next = self.snapshot.clone();
        next.latest_recovery = Some(recovery.clone());
        self.persist(trigger, next)?;
        Ok(RecoveryTransition::RecoveryPreserved(recovery))
    }

    fn apply_meaningful(
        &mut self,
        trigger: RecoveryTrigger,
        stamp: RecoveryStamp,
        through: RecoverySequence,
        acknowledgement: PrivateSaved,
    ) -> Result<RecoveryTransition, RecoveryMachineError<P::Error>> {
        let checkpoint =
            MeaningfulCheckpoint::from_acknowledgement(stamp, through, &acknowledgement);
        if let Some(current) = self.snapshot.last_meaningful {
            match stamp.cmp(&current.stamp) {
                core::cmp::Ordering::Less => return Ok(RecoveryTransition::OlderIgnored),
                core::cmp::Ordering::Equal if checkpoint == current => {
                    return Ok(RecoveryTransition::Duplicate);
                }
                core::cmp::Ordering::Equal => {
                    return Err(RecoveryMachineError::State(
                        RecoveryStateError::ConflictingDuplicate,
                    ));
                }
                core::cmp::Ordering::Greater => {}
            }
        }
        let window = self
            .snapshot
            .open_window
            .ok_or(RecoveryMachineError::State(
                RecoveryStateError::NoOpenWindow,
            ))?;
        ensure_inside(window, through).map_err(RecoveryMachineError::State)?;
        if through != window.last {
            return Err(RecoveryMachineError::State(
                RecoveryStateError::SequenceOutsideWindow,
            ));
        }
        let mut next = self.snapshot.clone();
        next.last_meaningful = Some(checkpoint);
        next.open_window = None;
        next.pending_meaningful = None;
        self.persist(trigger, next)?;
        Ok(RecoveryTransition::MeaningfulSaved {
            checkpoint,
            acknowledgement,
        })
    }

    fn persist(
        &mut self,
        trigger: RecoveryTrigger,
        next: RecoverySnapshot,
    ) -> Result<(), RecoveryMachineError<P::Error>> {
        self.persistence
            .persist(&next)
            .map_err(|error| RecoveryMachineError::Persistence { trigger, error })?;
        self.snapshot = next;
        Ok(())
    }
}

fn apply_evidence_to_snapshot(
    snapshot: &mut RecoverySnapshot,
    trigger: RecoveryTrigger,
    evidence: RecoveryBoundaryEvidence,
) -> Result<(), RecoveryStateError> {
    let expected = match trigger {
        RecoveryTrigger::ModifiedFileHandleClosed => BoundaryEvidenceKind::Closed,
        RecoveryTrigger::FsyncCompleted => BoundaryEvidenceKind::Synced,
        RecoveryTrigger::AtomicReplacementCompleted => BoundaryEvidenceKind::RenamedIntoPlace,
        _ => return Err(RecoveryStateError::WrongInputForTrigger),
    };
    if evidence.kind != expected {
        return Err(RecoveryStateError::WrongEvidenceForTrigger);
    }
    let window = snapshot
        .open_window
        .as_mut()
        .ok_or(RecoveryStateError::NoOpenWindow)?;
    ensure_inside(*window, evidence.through)?;
    if window
        .latest_evidence
        .is_none_or(|current| evidence.through >= current.through)
    {
        window.latest_evidence = Some(evidence);
    }
    Ok(())
}

fn observe_snapshot(
    snapshot: &mut RecoverySnapshot,
    sequence: RecoverySequence,
) -> Result<(), RecoveryStateError> {
    match snapshot.open_window.as_mut() {
        Some(window) if sequence < window.last => Err(RecoveryStateError::SequenceOutsideWindow),
        Some(window) => {
            window.last = sequence;
            Ok(())
        }
        None => {
            if snapshot
                .last_meaningful
                .is_some_and(|checkpoint| sequence <= checkpoint.through)
            {
                return Err(RecoveryStateError::SequenceOutsideWindow);
            }
            snapshot.open_window = Some(RecoveryWindow {
                from: sequence,
                last: sequence,
                latest_evidence: None,
            });
            Ok(())
        }
    }
}

fn ensure_inside(
    window: RecoveryWindow,
    through: RecoverySequence,
) -> Result<(), RecoveryStateError> {
    if through < window.from || through > window.last {
        Err(RecoveryStateError::SequenceOutsideWindow)
    } else {
        Ok(())
    }
}

fn validate_snapshot(snapshot: &RecoverySnapshot) -> Result<(), RecoveryStateError> {
    if let Some(window) = snapshot.open_window {
        if window.from > window.last {
            return Err(RecoveryStateError::InvalidSnapshot);
        }
        if snapshot
            .last_meaningful
            .is_some_and(|meaningful| meaningful.through >= window.from)
        {
            return Err(RecoveryStateError::InvalidSnapshot);
        }
        if let Some(evidence) = window.latest_evidence {
            ensure_inside(window, evidence.through)
                .map_err(|_| RecoveryStateError::InvalidSnapshot)?;
        }
        if let Some(recovery) = snapshot.latest_recovery.as_ref() {
            if recovery.bytes.is_empty()
                || recovery.stamp.content_hash != recovery.verified_content_hash
            {
                return Err(RecoveryStateError::InvalidSnapshot);
            }
            let belongs_to_current_window = ensure_inside(window, recovery.through).is_ok();
            let belongs_to_prior_meaningful = snapshot
                .last_meaningful
                .is_some_and(|checkpoint| recovery.through <= checkpoint.through);
            if !belongs_to_current_window && !belongs_to_prior_meaningful {
                return Err(RecoveryStateError::InvalidSnapshot);
            }
        }
        if let Some(pending) = snapshot.pending_meaningful {
            ensure_inside(window, pending.through)
                .map_err(|_| RecoveryStateError::InvalidSnapshot)?;
        }
    } else {
        if snapshot.pending_meaningful.is_some() {
            return Err(RecoveryStateError::InvalidSnapshot);
        }
        if let Some(recovery) = snapshot.latest_recovery.as_ref() {
            let Some(meaningful) = snapshot.last_meaningful else {
                return Err(RecoveryStateError::InvalidSnapshot);
            };
            if recovery.through > meaningful.through {
                return Err(RecoveryStateError::InvalidSnapshot);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    fn sequence(value: u64) -> RecoverySequence {
        RecoverySequence::new(value).expect("non-zero test sequence")
    }

    fn stamp(value: u8) -> RecoveryStamp {
        RecoveryStamp::new(
            u64::from(value),
            RecoveryEventUlid::from_bytes([value; 16]),
            RecordDigest::from_bytes([value; 32]),
        )
    }

    #[test]
    fn pending_acknowledgement_round_trips_in_v2_and_v1_remains_readable() {
        let acknowledgement =
            PrivateSaved::after_recovery_verified(Digest16::from_bytes([7; 16]), 1, 2, 3);
        let mut v2 = RecoverySnapshot {
            open_window: Some(RecoveryWindow {
                from: sequence(4),
                last: sequence(4),
                latest_evidence: None,
            }),
            pending_meaningful: Some(PendingMeaningfulSave::from_acknowledgement(
                sequence(4),
                stamp(4),
                &acknowledgement,
            )),
            ..RecoverySnapshot::default()
        };
        let encoded = v2.encode();
        assert_eq!(RecoverySnapshot::decode(&encoded), Ok(v2.clone()));

        v2.pending_meaningful = None;
        let mut legacy = v2.encode();
        legacy[b"mesh-recovery-state\0".len()] = 1;
        assert_eq!(RecoverySnapshot::decode(&legacy), Ok(v2));
    }

    #[test]
    fn pending_acknowledgement_outside_the_open_window_is_refused() {
        let acknowledgement =
            PrivateSaved::after_recovery_verified(Digest16::from_bytes([7; 16]), 1, 1, 1);
        let invalid = RecoverySnapshot {
            open_window: Some(RecoveryWindow {
                from: sequence(1),
                last: sequence(1),
                latest_evidence: None,
            }),
            pending_meaningful: Some(PendingMeaningfulSave::from_acknowledgement(
                sequence(2),
                stamp(2),
                &acknowledgement,
            )),
            ..RecoverySnapshot::default()
        };
        assert_eq!(
            RecoverySnapshot::decode(&invalid.encode()),
            Err(RecoveryStateError::InvalidSnapshot)
        );
    }

    #[test]
    fn checkpoint_snapshot_refuses_overlapping_meaningful_and_open_ranges() {
        let acknowledgement =
            PrivateSaved::after_recovery_verified(Digest16::from_bytes([7; 16]), 1, 1, 1);
        let invalid = RecoverySnapshot {
            last_meaningful: Some(MeaningfulCheckpoint::from_acknowledgement(
                stamp(4),
                sequence(4),
                &acknowledgement,
            )),
            open_window: Some(RecoveryWindow {
                from: sequence(4),
                last: sequence(5),
                latest_evidence: None,
            }),
            ..RecoverySnapshot::default()
        };
        assert_eq!(
            RecoverySnapshot::decode(&invalid.encode()),
            Err(RecoveryStateError::InvalidSnapshot)
        );
    }

    #[test]
    fn checkpoint_snapshot_refuses_closed_recovery_beyond_meaningful_state() {
        let acknowledgement =
            PrivateSaved::after_recovery_verified(Digest16::from_bytes([7; 16]), 1, 1, 1);
        let invalid = RecoverySnapshot {
            last_meaningful: Some(MeaningfulCheckpoint::from_acknowledgement(
                stamp(4),
                sequence(4),
                &acknowledgement,
            )),
            latest_recovery: Some(
                RecoveryPreserved::from_verified_bytes(
                    stamp(5),
                    sequence(5),
                    vec![5],
                    RecordDigest::from_bytes([5; 32]),
                )
                .expect("valid recovery payload"),
            ),
            ..RecoverySnapshot::default()
        };
        assert_eq!(
            RecoverySnapshot::decode(&invalid.encode()),
            Err(RecoveryStateError::InvalidSnapshot)
        );
    }
}
