//! Raw writes are accumulated locally and coalesced. They are never distributed.
//!
//! # The rule, and where it is actually enforced
//!
//! Plan §4.3: *"Raw `write()` calls are not distributed as permanent user-visible operations. They
//! are accumulated locally and coalesced into durable file versions."*
//!
//! A rule stated in a doc comment is a convention. This one is enforced by three facts that hold
//! whether or not anyone remembers it:
//!
//! 1. **[`RawWrite`] carries no content.** An object, an offset and a length — an accounting
//!    record. There is no field, constructor or method in this crate that accepts file bytes, so
//!    "the bytes leaked into an operation" is not a bug that can be introduced here; it is a
//!    signature that does not exist.
//! 2. **[`RawWrite`] is not an [`Operation`] and cannot become one.** It does not implement
//!    [`CanonicalEncode`](crate::CanonicalEncode), so it cannot be encoded, cannot be nested in a
//!    [`TransitionCommitment`](crate::TransitionCommitment), and cannot be sealed into a
//!    [`ChangeSet`](crate::ChangeSet).
//! 3. **The one exit from the accumulator demands a [`ManifestId`].** A manifest identifier names
//!    content that is already in the content plane, so producing a distributable operation
//!    *requires* the bytes to have been content-addressed first, by a crate that is not this one.
//!
//! `tests/no_raw_write_is_distributable.rs` reads this crate's own source and fails if a public
//! item ever takes a byte slice, which is the mechanical half.
//!
//! # What is deliberately not here
//!
//! Plan §4.5 asks a save coalescer to recognise the temp-file dance —
//! `write temp · flush · rename over destination · remove backup` — and emit one meaningful
//! change. **That recognition needs paths, file handles and rename events, which are platform
//! facts.** This task's own failure-and-recovery clause settles where it goes: *"If an operation
//! cannot be expressed without platform knowledge, it belongs in an adapter, not the vocabulary."*
//! So [`WriteCoalescer`] accumulates and gates; the adapter that can see a rename decides *when*
//! to call [`WriteCoalescer::checkpoint`] and with which manifest. [`CheckpointTrigger`] is the
//! vocabulary the two share, enumerated from plan §4.5 so an adapter cannot invent a reason that
//! nothing downstream can read.

use crate::ids::{ManifestId, ObjectId, VersionId};
use crate::name::PortableMetadata;
use crate::operation::Operation;

/// One local `write()`, as accounting and nothing else.
///
/// Deliberately contentless. See the module header: this type existing without a byte field is
/// half the reason raw content cannot reach the wire.
///
/// The other half is that it is not encodable, which is a compile error rather than a rule:
///
/// ```compile_fail,E0277
/// use mesh_operations::{encode_canonical, ObjectId, RawWrite};
/// // `RawWrite` does not implement `CanonicalEncode`, so it cannot be encoded, cannot be nested
/// // in a transition commitment, and cannot be sealed into a ChangeSet.
/// let bytes = encode_canonical(&RawWrite::new(ObjectId::from_bytes([1; 16]), 0, 16));
/// ```
///
/// The positive twin — the operation a coalesced write *does* produce — is
/// [`WriteCoalescer::checkpoint`]'s own example, because a compile-fail test whose positive twin
/// is missing proves only that the snippet is broken.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RawWrite {
    object_id: ObjectId,
    offset: u64,
    length: u64,
}

impl RawWrite {
    /// A write of `length` bytes at `offset` into `object_id`.
    #[must_use]
    pub const fn new(object_id: ObjectId, offset: u64, length: u64) -> Self {
        Self {
            object_id,
            offset,
            length,
        }
    }

    /// The object written to.
    #[must_use]
    pub const fn object_id(&self) -> ObjectId {
        self.object_id
    }

    /// Where the write landed.
    #[must_use]
    pub const fn offset(&self) -> u64 {
        self.offset
    }

    /// How many bytes it covered.
    #[must_use]
    pub const fn length(&self) -> u64 {
        self.length
    }

    /// The byte just past this write.
    ///
    /// Saturating: an offset plus a length that overflows `u64` cannot describe a real file, and
    /// wrapping would report a high write as covering the start of the file.
    #[must_use]
    pub const fn end(&self) -> u64 {
        self.offset.saturating_add(self.length)
    }
}

/// Why a checkpoint is being taken, from plan §4.5's list.
///
/// Enumerated rather than free text so that an adapter cannot invent a reason nothing downstream
/// can read, and so that "why did this version exist" is answerable for every version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CheckpointTrigger {
    /// An integrated agent asked for a flush.
    AgentFlush,
    /// A modified file handle closed.
    HandleClosed,
    /// An `fsync` completed.
    FsyncCompleted,
    /// An atomic replacement completed — the end of the temp-file save dance.
    AtomicReplacement,
    /// The actor's process exited.
    ProcessExit,
    /// The actor went idle for the configured interval.
    Idle,
    /// The uncheckpointed byte or time ceiling was reached.
    CeilingReached,
    /// The user opened review.
    ReviewOpened,
    /// The actor disconnected.
    Disconnected,
}

impl CheckpointTrigger {
    /// Every trigger, in the order plan §4.5 lists them.
    pub const ALL: [Self; 9] = [
        Self::AgentFlush,
        Self::HandleClosed,
        Self::FsyncCompleted,
        Self::AtomicReplacement,
        Self::ProcessExit,
        Self::Idle,
        Self::CeilingReached,
        Self::ReviewOpened,
        Self::Disconnected,
    ];

    /// The published name.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::AgentFlush => "AgentFlush",
            Self::HandleClosed => "HandleClosed",
            Self::FsyncCompleted => "FsyncCompleted",
            Self::AtomicReplacement => "AtomicReplacement",
            Self::ProcessExit => "ProcessExit",
            Self::Idle => "Idle",
            Self::CeilingReached => "CeilingReached",
            Self::ReviewOpened => "ReviewOpened",
            Self::Disconnected => "Disconnected",
        }
    }
}

/// Why a checkpoint produced no operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotCheckpointed {
    /// Nothing was written since the last checkpoint, so there is no durable change to record.
    ///
    /// A version per `fsync` on an untouched file would fill an actor's history with transitions
    /// that say nothing, and every one of them would have to be reviewed.
    NothingWritten,
    /// A write named a different object than the one this coalescer accumulates for.
    ///
    /// One coalescer, one object. Returned rather than silently ignored, because a misrouted write
    /// means the caller's bookkeeping is wrong and losing it quietly loses a real change.
    ForeignObject {
        /// The object this coalescer accumulates for.
        expected: ObjectId,
        /// The object the write named.
        found: ObjectId,
    },
}

/// The local accumulator between one durable version and the next.
///
/// One instance per object being written. Consumed and returned by every method, so an accumulator
/// cannot be checkpointed twice from the same value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteCoalescer {
    object_id: ObjectId,
    writes: Vec<RawWrite>,
}

impl WriteCoalescer {
    /// An accumulator for one object, holding nothing.
    #[must_use]
    pub const fn new(object_id: ObjectId) -> Self {
        Self {
            object_id,
            writes: Vec::new(),
        }
    }

    /// The object this accumulates for.
    #[must_use]
    pub const fn object_id(&self) -> ObjectId {
        self.object_id
    }

    /// How many raw writes have accumulated.
    #[must_use]
    pub fn raw_write_count(&self) -> usize {
        self.writes.len()
    }

    /// The highest byte any accumulated write reached, or zero when none has.
    #[must_use]
    pub fn high_water_mark(&self) -> u64 {
        self.writes.iter().map(RawWrite::end).max().unwrap_or(0)
    }

    /// Whether nothing has accumulated.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.writes.is_empty()
    }

    /// Accumulate one write.
    ///
    /// # Errors
    ///
    /// [`NotCheckpointed::ForeignObject`] when the write names another object.
    pub fn record(mut self, write: RawWrite) -> Result<Self, NotCheckpointed> {
        if write.object_id() != self.object_id {
            return Err(NotCheckpointed::ForeignObject {
                expected: self.object_id,
                found: write.object_id(),
            });
        }
        self.writes.push(write);
        Ok(self)
    }

    /// Coalesce everything accumulated into one durable file version, and start over.
    ///
    /// The accumulated writes do not appear in the result and are not carried anywhere: they are
    /// **replaced** by one [`Operation::WriteFileVersion`] naming the content-addressed manifest
    /// the caller supplies. That substitution is the whole of "coalesced into durable file
    /// versions", and the `manifest_id` parameter is what makes it impossible to reach this
    /// operation without having gone through the content plane.
    ///
    /// Returns the emptied accumulator alongside the outcome, so a caller that checkpoints on an
    /// untouched file keeps a usable coalescer rather than having to rebuild one.
    ///
    /// ```
    /// use mesh_operations::{
    ///     CheckpointTrigger, ManifestId, NotCheckpointed, ObjectId, Operation, PortableMetadata,
    ///     RawWrite, VersionId, WriteCoalescer,
    /// };
    ///
    /// let object = ObjectId::from_bytes([1; 16]);
    /// let coalescer = WriteCoalescer::new(object)
    ///     .record(RawWrite::new(object, 0, 4096)).unwrap()
    ///     .record(RawWrite::new(object, 4096, 512)).unwrap();
    /// assert_eq!(coalescer.raw_write_count(), 2);
    ///
    /// let (coalescer, outcome) = coalescer.checkpoint(
    ///     CheckpointTrigger::FsyncCompleted,
    ///     VersionId::from_bytes([2; 32]),
    ///     Vec::new(),
    ///     ManifestId::from_bytes([3; 32]),
    ///     PortableMetadata::new(false),
    /// );
    ///
    /// // Two raw writes became one distributable operation, and the accumulator is empty.
    /// assert!(matches!(outcome, Ok(Operation::WriteFileVersion { .. })));
    /// assert!(coalescer.is_empty());
    ///
    /// // A second checkpoint over nothing produces nothing.
    /// let (_, outcome) = coalescer.checkpoint(
    ///     CheckpointTrigger::Idle,
    ///     VersionId::from_bytes([4; 32]),
    ///     Vec::new(),
    ///     ManifestId::from_bytes([5; 32]),
    ///     PortableMetadata::new(false),
    /// );
    /// assert_eq!(outcome, Err(NotCheckpointed::NothingWritten));
    /// ```
    #[must_use = "the emptied accumulator must be kept: dropping it loses every write recorded \
                  after this checkpoint"]
    pub fn checkpoint(
        mut self,
        trigger: CheckpointTrigger,
        version_id: VersionId,
        parent_versions: Vec<VersionId>,
        manifest_id: ManifestId,
        portable_metadata: PortableMetadata,
    ) -> (Self, Result<Operation, NotCheckpointed>) {
        let _ = trigger;
        if self.writes.is_empty() {
            return (self, Err(NotCheckpointed::NothingWritten));
        }
        self.writes.clear();
        let operation = Operation::WriteFileVersion {
            object_id: self.object_id,
            version_id,
            parent_versions,
            manifest_id,
            portable_metadata,
        };
        (self, Ok(operation))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object() -> ObjectId {
        ObjectId::from_bytes([1; 16])
    }

    #[test]
    fn a_thousand_raw_writes_become_one_operation() {
        let mut coalescer = WriteCoalescer::new(object());
        for index in 0..1000u64 {
            coalescer = coalescer
                .record(RawWrite::new(object(), index * 4096, 4096))
                .unwrap();
        }
        assert_eq!(coalescer.raw_write_count(), 1000);
        assert_eq!(coalescer.high_water_mark(), 1000 * 4096);

        let (coalescer, outcome) = coalescer.checkpoint(
            CheckpointTrigger::HandleClosed,
            VersionId::from_bytes([2; 32]),
            vec![VersionId::from_bytes([1; 32])],
            ManifestId::from_bytes([3; 32]),
            PortableMetadata::new(false),
        );
        let operation = outcome.unwrap();
        assert!(matches!(operation, Operation::WriteFileVersion { .. }));
        assert!(coalescer.is_empty());
        assert_eq!(coalescer.high_water_mark(), 0);
    }

    #[test]
    fn a_checkpoint_over_nothing_produces_nothing() {
        let (coalescer, outcome) = WriteCoalescer::new(object()).checkpoint(
            CheckpointTrigger::Idle,
            VersionId::from_bytes([2; 32]),
            Vec::new(),
            ManifestId::from_bytes([3; 32]),
            PortableMetadata::new(false),
        );
        assert_eq!(outcome, Err(NotCheckpointed::NothingWritten));
        assert!(coalescer.is_empty());
    }

    #[test]
    fn a_write_for_another_object_is_refused_rather_than_dropped() {
        let other = ObjectId::from_bytes([9; 16]);
        assert_eq!(
            WriteCoalescer::new(object()).record(RawWrite::new(other, 0, 1)),
            Err(NotCheckpointed::ForeignObject {
                expected: object(),
                found: other
            })
        );
    }

    #[test]
    fn every_trigger_produces_the_same_operation_shape() {
        // The trigger is why a checkpoint happened, which is local. It is deliberately not a field
        // of the distributed operation: a peer cannot check it and does not act on it.
        let mut shapes = Vec::new();
        for trigger in CheckpointTrigger::ALL {
            let coalescer = WriteCoalescer::new(object())
                .record(RawWrite::new(object(), 0, 1))
                .unwrap();
            let (_, outcome) = coalescer.checkpoint(
                trigger,
                VersionId::from_bytes([2; 32]),
                Vec::new(),
                ManifestId::from_bytes([3; 32]),
                PortableMetadata::new(false),
            );
            shapes.push(outcome.unwrap());
        }
        assert_eq!(shapes.len(), CheckpointTrigger::ALL.len());
        assert!(shapes.windows(2).all(|pair| pair[0] == pair[1]));
    }

    #[test]
    fn an_offset_that_would_overflow_saturates_rather_than_wrapping() {
        let write = RawWrite::new(object(), u64::MAX - 1, 16);
        assert_eq!(write.end(), u64::MAX);
    }

    #[test]
    fn the_trigger_vocabulary_has_no_duplicate_name() {
        let mut names: Vec<&str> = CheckpointTrigger::ALL
            .iter()
            .map(CheckpointTrigger::as_str)
            .collect();
        assert_eq!(names.len(), 9);
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 9);
    }
}
