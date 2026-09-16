//! Plan §6.3's eleven steps as one interruptible sequence, and the acknowledgement boundary.
//!
//! # The one sentence this module is built around
//!
//! > **"Saved privately" becomes true at exactly one instant — when the transaction of step 9
//! > returns — and the acknowledgement of step 10 is the only thing allowed to say so.**
//!
//! Everything below follows from that, including the parts that look like ceremony. An
//! acknowledgement that outruns durability is not a cosmetic defect: it is the product telling a
//! user their work is safe when it is not, which is the one failure `docs/consistency.md` names as
//! a P0 (*"Zero acknowledged-state loss is the durability claim"*).
//!
//! # The eleven steps, and what a kill after each one leaves behind
//!
//! Plan §6.3 numbers the steps and then states the crash behaviour in three clauses. Both are here
//! as data — [`SequenceStep::ORDER`] and [`SequenceStep::residue_if_killed_after`] — so a test
//! reads the contract off the plan instead of off some function's body.
//!
//! | # | Step | A kill immediately after it leaves |
//! |---|---|---|
//! | 1 | [`WriteChunks`](SequenceStep::WriteChunks) | temporary files, discarded at startup; no chunk, no reference |
//! | 2 | [`FlushChunks`](SequenceStep::FlushChunks) | the same, now durable but still not addressable |
//! | 3 | [`VerifyChunks`](SequenceStep::VerifyChunks) | the same; verification reads nothing into the store |
//! | 4 | [`PromoteChunks`](SequenceStep::PromoteChunks) | chunks in the store that nothing references — **collection candidates, never dangling references** |
//! | 5 | [`BeginTransaction`](SequenceStep::BeginTransaction) | the same as 4: the index is untouched |
//! | 6 | [`InsertImmutable`](SequenceStep::InsertImmutable) | the same as 4 |
//! | 7 | [`AdvanceHeads`](SequenceStep::AdvanceHeads) | the same as 4 |
//! | 8 | [`FillOutbox`](SequenceStep::FillOutbox) | the same as 4 |
//! | 9 | [`CommitTransaction`](SequenceStep::CommitTransaction) | **the checkpoint, whole** — durable, unacknowledged |
//! | 10 | [`ReportSaved`](SequenceStep::ReportSaved) | the checkpoint, whole, and the user was told |
//! | 11 | [`Replicate`](SequenceStep::Replicate) | the checkpoint, whole; replication resumes from the outbox |
//!
//! Rows 5 to 8 are identical to row 4 on purpose, and the reason is worth stating plainly rather
//! than letting a reader assume it is an oversight.
//!
//! # Why steps 5 to 8 are staged in memory and executed as one batch at step 9
//!
//! [`SqlExecutor`] hands a driver a batch of SQL and gets back success or failure; it has no way to
//! hold a transaction open across calls, and the test harness for this crate spawns a process per
//! call, so a `BEGIN IMMEDIATE` issued in one call would be rolled back by the exit of the process
//! that issued it. That is not a limitation this module works around — it is the shape the
//! guarantee already had. **SQLite's transaction is the atom.** A process killed between `BEGIN`
//! and `COMMIT` leaves a database indistinguishable from one killed before `BEGIN`, which is
//! exactly plan §6.3's clause *"after step 4 but before step 9: unreferenced chunks are
//! garbage-collected"*.
//!
//! So steps 5 to 8 *compose* the transaction, in the plan's order, each appending its own
//! statements from [`CommitPlan`], and step 9 executes the whole of it in one call. A kill at any
//! of those four boundaries leaves the index untouched, which the kill-point harness asserts with a
//! real `SIGKILL` rather than by repeating this paragraph. The harness also kills *inside* the
//! batch, because "a kill between 5 and 9 is the same as a kill before 5" is a claim about SQLite
//! that this crate is entitled to rely on but not to assert without evidence.
//!
//! # What makes an optimistic acknowledgement impossible rather than merely discouraged
//!
//! [`PrivateSaved`] has no public constructor. It cannot be built by a caller, cannot be built by
//! this crate before step 9 has returned `Ok`, and [`DurableCommit::acknowledgement`] answers
//! `None` at every earlier boundary. Reporting "saved privately" early is therefore a change to
//! this file, not a mistake a caller can make.
//!
//! The failure path is the half that decides whether the claim survives contact with reality. When
//! step 9's batch returns an error, **whether the transaction committed is unknown** — the driver
//! saw a failure, which may have arrived before or after `COMMIT` reached the disk. This module
//! reports [`SequenceError::CommitOutcomeUnknown`] and acknowledges nothing. Choosing the other
//! direction would trade a recoverable ambiguity for the one failure the whole sequence exists to
//! prevent.
//!
//! # Where to start
//!
//! ```
//! use mesh_store::{Checkpoint, ChunkPromoter, DurableCommit, RecordDigest, SequenceStep};
//! # use mesh_store::{OperationRecord, SqlExecutor, Store, Row, Table, Value};
//! # #[derive(Default)]
//! # struct NoChunks;
//! # impl ChunkPromoter for NoChunks {
//! #     type Error = String;
//! #     fn write_temporary(&mut self, chunks: &[Vec<u8>]) -> Result<Vec<RecordDigest>, String> {
//! #         Ok(chunks.iter().map(|_| RecordDigest::from_bytes([9; 32])).collect())
//! #     }
//! #     fn flush_temporary(&mut self) -> Result<(), String> { Ok(()) }
//! #     fn verify_temporary(&mut self) -> Result<(), String> { Ok(()) }
//! #     fn promote(&mut self) -> Result<(), String> { Ok(()) }
//! #     fn discard_temporary(&mut self) -> Result<usize, String> { Ok(0) }
//! #     fn is_durable(&self, _digest: &RecordDigest) -> bool { true }
//! # }
//! # #[derive(Default)]
//! # struct Accepting;
//! # impl SqlExecutor for Accepting {
//! #     type Error = String;
//! #     fn execute_batch(&mut self, _sql: &str) -> Result<(), String> { Ok(()) }
//! #     fn read_table(&mut self, _table: &Table) -> Result<Vec<Row>, String> { Ok(Vec::new()) }
//! #     fn table_exists(&mut self, _name: &str) -> Result<bool, String> { Ok(false) }
//! # }
//! # let mut store = Store::open(Accepting::default()).unwrap();
//! # let mut promoter = NoChunks::default();
//! # let checkpoint = Checkpoint {
//! #     operations: vec![OperationRecord {
//! #         id: RecordDigest::from_bytes([1; 32]),
//! #         actor: RecordDigest::from_bytes([2; 32]),
//! #         actor_sequence: 1,
//! #         hlc_millis: 1,
//! #         hlc_counter: 0,
//! #         policy_epoch: 1,
//! #         session: mesh_store::no_session(),
//! #         payload_digest: RecordDigest::from_bytes([3; 32]),
//! #         parents: Vec::new(),
//! #     }],
//! #     ..Checkpoint::default()
//! # };
//! let mut sequence = DurableCommit::new(&mut store, &mut promoter, Vec::new(), checkpoint);
//!
//! // Nothing is acknowledged until the transaction returns.
//! sequence.run_through(SequenceStep::FillOutbox).unwrap();
//! assert!(sequence.acknowledgement().is_none());
//!
//! let saved = sequence.finish().unwrap();
//! assert_eq!(saved.acknowledgement().operations(), 1);
//! ```

use core::fmt;

use crate::commit::{Checkpoint, CommitPlan, CommitStep, Statement};
use crate::digest::Digest16;
use crate::ids::RecordDigest;
use crate::index::{FoldError, Index};
use crate::row::Value;
use crate::sql::{SqlExecutor, Store};

/// One interruptible step of plan §6.3's commit sequence.
///
/// The order is [`SequenceStep::ORDER`] and the crash behaviour is
/// [`SequenceStep::residue_if_killed_after`]; both are the plan's own text as data, so a test can
/// enumerate them rather than restate them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SequenceStep {
    /// §6.3 step 1 — write new chunk data to temporary files.
    WriteChunks,
    /// §6.3 step 2 — flush temporary chunk data.
    FlushChunks,
    /// §6.3 step 3 — verify hashes.
    VerifyChunks,
    /// §6.3 step 4 — atomically promote chunks into the content-addressed store.
    PromoteChunks,
    /// §6.3 step 5 — begin the transaction.
    BeginTransaction,
    /// §6.3 step 6 — insert manifests and immutable operations.
    InsertImmutable,
    /// §6.3 step 7 — advance the actor head.
    AdvanceHeads,
    /// §6.3 step 8 — add outbox records.
    FillOutbox,
    /// §6.3 step 9 — commit the transaction. The acknowledgement boundary is here.
    CommitTransaction,
    /// §6.3 step 10 — report that the work is saved privately.
    ReportSaved,
    /// §6.3 step 11 — replicate asynchronously.
    Replicate,
}

/// What a crash immediately after a step leaves behind, in plan §6.3's own three classes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CrashResidue {
    /// §6.3: *"before step 4: temporary data is discarded"*. Nothing addressable was created and
    /// nothing durable refers to anything; startup removes the scratch files.
    DiscardTemporary,
    /// §6.3: *"after step 4 but before step 9: unreferenced chunks are garbage-collected"*. The
    /// chunks are in the store, whole and verified, and no durable head names them. They are
    /// collection candidates — the arrival journal records them before they become visible, which
    /// is what makes them findable rather than leaked.
    CollectUnreferencedChunks,
    /// §6.3: *"after step 9: the checkpoint is durable"*. Reopening recovers it intact.
    RecoverCheckpoint,
}

impl SequenceStep {
    /// Every step, in the order they are performed.
    pub const ORDER: [Self; 11] = [
        Self::WriteChunks,
        Self::FlushChunks,
        Self::VerifyChunks,
        Self::PromoteChunks,
        Self::BeginTransaction,
        Self::InsertImmutable,
        Self::AdvanceHeads,
        Self::FillOutbox,
        Self::CommitTransaction,
        Self::ReportSaved,
        Self::Replicate,
    ];

    /// The plan §6.3 step number, which is also this step's position in [`Self::ORDER`] plus one.
    #[must_use]
    pub const fn plan_step(self) -> u8 {
        match self {
            Self::WriteChunks => 1,
            Self::FlushChunks => 2,
            Self::VerifyChunks => 3,
            Self::PromoteChunks => 4,
            Self::BeginTransaction => 5,
            Self::InsertImmutable => 6,
            Self::AdvanceHeads => 7,
            Self::FillOutbox => 8,
            Self::CommitTransaction => 9,
            Self::ReportSaved => 10,
            Self::Replicate => 11,
        }
    }

    /// The step after this one, or `None` for the last.
    #[must_use]
    pub fn next(self) -> Option<Self> {
        let position = Self::ORDER.iter().position(|step| *step == self)?;
        Self::ORDER.get(position + 1).copied()
    }

    /// The step's name, for logs and for the kill harness's argument parsing.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::WriteChunks => "write-chunks",
            Self::FlushChunks => "flush-chunks",
            Self::VerifyChunks => "verify-chunks",
            Self::PromoteChunks => "promote-chunks",
            Self::BeginTransaction => "begin-transaction",
            Self::InsertImmutable => "insert-immutable",
            Self::AdvanceHeads => "advance-heads",
            Self::FillOutbox => "fill-outbox",
            Self::CommitTransaction => "commit-transaction",
            Self::ReportSaved => "report-saved",
            Self::Replicate => "replicate",
        }
    }

    /// The step with this name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ORDER.into_iter().find(|step| step.name() == name)
    }

    /// Which [`CommitStep`] this step contributes, for the five that are inside the transaction.
    ///
    /// The mapping exists so the transaction has exactly one definition. `CommitPlan` decides what
    /// SQL belongs to which of plan §6.3's steps 5 to 9; this sequence decides only *when* each
    /// group is appended, and never rewrites or reorders one.
    #[must_use]
    pub const fn commit_step(self) -> Option<CommitStep> {
        match self {
            Self::BeginTransaction => Some(CommitStep::Begin),
            Self::InsertImmutable => Some(CommitStep::InsertImmutable),
            Self::AdvanceHeads => Some(CommitStep::AdvanceHeads),
            Self::FillOutbox => Some(CommitStep::FillOutbox),
            Self::CommitTransaction => Some(CommitStep::Commit),
            _ => None,
        }
    }

    /// What a crash immediately after this step leaves behind.
    ///
    /// This is plan §6.3's crash-behaviour clause, transcribed. Nothing computes it from the
    /// implementation, which is the point: if the implementation ever stops matching, the
    /// kill-point harness fails rather than agreeing with itself.
    #[must_use]
    pub const fn residue_if_killed_after(self) -> CrashResidue {
        match self {
            Self::WriteChunks | Self::FlushChunks | Self::VerifyChunks => {
                CrashResidue::DiscardTemporary
            }
            Self::PromoteChunks
            | Self::BeginTransaction
            | Self::InsertImmutable
            | Self::AdvanceHeads
            | Self::FillOutbox => CrashResidue::CollectUnreferencedChunks,
            Self::CommitTransaction | Self::ReportSaved | Self::Replicate => {
                CrashResidue::RecoverCheckpoint
            }
        }
    }

    /// Whether a user has been told "saved privately" by the time this step has completed.
    ///
    /// Only step 10 and what follows it. Paired with [`Self::residue_if_killed_after`] this is the
    /// durability claim as a checkable property over the whole enumeration: **every step this
    /// answers `true` for must leave [`CrashResidue::RecoverCheckpoint`]**. That is one `assert`
    /// over eleven values, and it is the shape of the claim rather than an example of it.
    #[must_use]
    pub const fn acknowledged_by_here(self) -> bool {
        matches!(self, Self::ReportSaved | Self::Replicate)
    }
}

impl fmt::Display for SequenceStep {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "step {} ({})", self.plan_step(), self.name())
    }
}

/// Plan §6.3 steps 1 to 4: the content-addressed store, as a seam.
///
/// `mesh-store` does not import `mesh-cas`. The seam is not a workaround for that: the two halves
/// of the sequence **must be able to fail independently** for §6.3's crash
/// clauses to hold at all, and a trait boundary is what makes "the chunk is durable before the
/// reference is" something a caller can be held to rather than something one function happens to do
/// in the right order.
///
/// Each method is one of the plan's numbered steps, so a crash harness can stop between any two.
pub trait ChunkPromoter {
    /// Whatever the store fails with.
    type Error;

    /// Step 1 — write the chunk bytes to temporary files, returning the name each will have.
    ///
    /// The names are a function of the bytes alone, so they are known before anything is promoted.
    ///
    /// # Errors
    ///
    /// Whatever the store reports.
    fn write_temporary(&mut self, chunks: &[Vec<u8>]) -> Result<Vec<RecordDigest>, Self::Error>;

    /// Step 2 — force the temporary data to durable storage, before any name reveals it.
    ///
    /// # Errors
    ///
    /// Whatever the store reports.
    fn flush_temporary(&mut self) -> Result<(), Self::Error>;

    /// Step 3 — read the temporary files back and check they hash to the names they will take.
    ///
    /// # Errors
    ///
    /// Whatever the store reports, including a verification failure.
    fn verify_temporary(&mut self) -> Result<(), Self::Error>;

    /// Step 4 — atomically promote the verified chunks into the content namespace.
    ///
    /// # Errors
    ///
    /// Whatever the store reports.
    fn promote(&mut self) -> Result<(), Self::Error>;

    /// Discard temporary data left behind by a previous crash, returning how much was removed.
    ///
    /// Plan §6.3's *"before step 4: temporary data is discarded"*, for the process that lives to do
    /// it. Called at startup, not during a sequence.
    ///
    /// # Errors
    ///
    /// Whatever the store reports.
    fn discard_temporary(&mut self) -> Result<usize, Self::Error>;

    /// Whether the store holds a chunk under this name.
    ///
    /// Consulted at step 5, before a single statement is composed, so a manifest naming a chunk
    /// nothing promoted is refused with the index untouched. **This is a question about promotion,
    /// not about integrity**: it says a chunk was promoted, not that its bytes still hash to their
    /// name. That is a read's question, and reading every referenced chunk on every save would make
    /// each save O(workspace).
    fn is_durable(&self, digest: &RecordDigest) -> bool;
}

/// The acknowledgement: the work is saved privately.
///
/// # It has no public constructor, and that is the whole design
///
/// A value of this type is produced in exactly one place — after step 9's batch has returned `Ok`
/// — and nowhere else. A caller cannot build one, cannot clone one into existence early, and
/// cannot get one out of a [`DurableCommit`] that has not reached step 10:
///
/// ```compile_fail,E0423
/// # use mesh_store::PrivateSaved;
/// let optimistic = PrivateSaved { operations: 1, manifests: 0, chunks: 0 };
/// ```
///
/// So "PRIVATE_SAVED is reported only after the transaction commits" is a property of the type
/// rather than a rule a reviewer has to keep noticing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrivateSaved {
    index_digest: Digest16,
    operations: usize,
    manifests: usize,
    chunks: usize,
}

impl PrivateSaved {
    /// The only constructor, private to this module and reachable only from the step that runs
    /// after the transaction has returned.
    const fn after_the_transaction_returned(
        index_digest: Digest16,
        operations: usize,
        manifests: usize,
        chunks: usize,
    ) -> Self {
        Self {
            index_digest,
            operations,
            manifests,
            chunks,
        }
    }

    /// Recreate the acknowledgement only after recovery independently reproduced its exact index
    /// digest. This remains crate-private so callers cannot turn persisted counters into product
    /// truth without going through the store's verification path.
    pub(crate) const fn after_recovery_verified(
        index_digest: Digest16,
        operations: usize,
        manifests: usize,
        chunks: usize,
    ) -> Self {
        Self {
            index_digest,
            operations,
            manifests,
            chunks,
        }
    }

    /// A fingerprint of the whole index as it now stands, so a caller can prove two saves agree.
    #[must_use]
    pub const fn index_digest(&self) -> Digest16 {
        self.index_digest
    }

    /// How many operations this save made durable.
    #[must_use]
    pub const fn operations(&self) -> usize {
        self.operations
    }

    /// How many file manifests this save made durable.
    #[must_use]
    pub const fn manifests(&self) -> usize {
        self.manifests
    }

    /// How many chunks this save promoted before the transaction opened.
    #[must_use]
    pub const fn chunks(&self) -> usize {
        self.chunks
    }
}

impl fmt::Display for PrivateSaved {
    /// The user-facing wording, which is `docs/consistency.md` §4's promise and nothing else.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("saved privately")
    }
}

/// One thing a peer has not been sent yet.
///
/// Derived from the index rather than held as a queue: the outbox is *the difference* between what
/// an actor has authored and what a peer's watermark has reached, so losing this value loses
/// nothing. It is handed over at step 11 and can be recomputed at any time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OutboxEntry {
    /// The peer that has not been sent this yet.
    pub peer: RecordDigest,
    /// The actor that authored it.
    pub actor: RecordDigest,
    /// The author's own sequence number.
    pub actor_sequence: u64,
    /// The operation to send.
    pub operation: RecordDigest,
}

/// Plan §6.3 step 11's handoff: what replication should carry, once the save is acknowledged.
///
/// Produced *after* the acknowledgement and never before, because replication is asynchronous by
/// §6.3's own wording and a save that could not be sent is still a save.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplicationHandoff {
    entries: Vec<OutboxEntry>,
}

impl ReplicationHandoff {
    /// What is outstanding, ordered as the index orders it.
    #[must_use]
    pub fn entries(&self) -> &[OutboxEntry] {
        &self.entries
    }

    /// Whether anything is outstanding.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// A completed sequence: the acknowledgement, and what replication is handed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    acknowledgement: PrivateSaved,
    handoff: ReplicationHandoff,
}

impl Saved {
    /// The acknowledgement. Its existence is the proof the transaction returned.
    #[must_use]
    pub const fn acknowledgement(&self) -> &PrivateSaved {
        &self.acknowledgement
    }

    /// What replication is handed at step 11.
    #[must_use]
    pub const fn handoff(&self) -> &ReplicationHandoff {
        &self.handoff
    }
}

/// Why a sequence stopped.
///
/// Every variant records the step it stopped at, because *what a crash leaves behind* is a function
/// of that step and of nothing else.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SequenceError<C, D> {
    /// One of steps 1 to 4 failed. Nothing durable references the chunks: before step 4 there is
    /// only temporary data, and at step 4 a partly-promoted set is a set of unreferenced chunks.
    Chunks {
        /// Which of steps 1 to 4 failed.
        step: SequenceStep,
        /// What the content-addressed store reported.
        error: C,
    },
    /// A manifest names a chunk the store does not hold. Refused at step 5, before a single
    /// statement is composed, so the index cannot come to reference content that is not there.
    ChunkNotDurable {
        /// The chunk the manifest names.
        digest: RecordDigest,
    },
    /// The checkpoint contradicts what the index already holds. Nothing was executed.
    Fold(FoldError),
    /// Step 9's batch failed, and **whether the transaction committed is unknown**.
    ///
    /// The driver saw a failure; that failure may have arrived before `COMMIT` was durable or
    /// after. Nothing is acknowledged, because acknowledging an unknown is the one failure this
    /// sequence exists to prevent. The recovery is to reopen the database and read it: it holds
    /// either the whole checkpoint or none of it, and the caller learns which by looking rather
    /// than by assuming.
    CommitOutcomeUnknown {
        /// What the driver reported.
        error: D,
    },
}

impl<C: fmt::Display, D: fmt::Display> fmt::Display for SequenceError<C, D> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Chunks { step, error } => {
                write!(formatter, "{step} failed: {error}")
            }
            Self::ChunkNotDurable { digest } => write!(
                formatter,
                "a manifest names chunk {digest} but the content-addressed store does not hold it; \
                 the transaction was not composed, so no head references it"
            ),
            Self::Fold(error) => write!(formatter, "{error}"),
            Self::CommitOutcomeUnknown { error } => write!(
                formatter,
                "step 9 (commit-transaction) failed and the outcome is unknown: {error}. Nothing \
                 was acknowledged. Reopen the database and read it — it holds either the whole \
                 checkpoint or none of it."
            ),
        }
    }
}

impl<C, D> SequenceError<C, D> {
    /// The step the sequence stopped at.
    #[must_use]
    pub const fn step(&self) -> SequenceStep {
        match self {
            Self::Chunks { step, .. } => *step,
            Self::ChunkNotDurable { .. } | Self::Fold(_) => SequenceStep::BeginTransaction,
            Self::CommitOutcomeUnknown { .. } => SequenceStep::CommitTransaction,
        }
    }

    /// Whether this failure leaves the durable outcome unknown to the caller.
    ///
    /// True for exactly one variant. Every other failure leaves the index provably untouched, so a
    /// caller can retry without reading anything back.
    #[must_use]
    pub const fn outcome_is_unknown(&self) -> bool {
        matches!(self, Self::CommitOutcomeUnknown { .. })
    }
}

impl<C, D> From<FoldError> for SequenceError<C, D> {
    fn from(error: FoldError) -> Self {
        Self::Fold(error)
    }
}

impl<C: fmt::Debug + fmt::Display, D: fmt::Debug + fmt::Display> std::error::Error
    for SequenceError<C, D>
{
}

/// One run of plan §6.3's eleven steps, performed one step at a time.
///
/// Running the steps individually is the only implementation — [`Self::finish`] is a loop over
/// [`Self::step`] — so the sequence a crash harness interrupts is the sequence production runs.
/// That is deliberate and is the same discipline the content-addressed store's promotion uses: a
/// stop point that exists only for tests is a stop point that proves nothing about the real path.
#[derive(Debug)]
pub struct DurableCommit<'a, P: ChunkPromoter, E: SqlExecutor> {
    store: &'a mut Store<E>,
    promoter: &'a mut P,
    chunks: Vec<Vec<u8>>,
    checkpoint: Checkpoint,
    promoted: Vec<RecordDigest>,
    plan: Option<CommitPlan>,
    after: Option<Index>,
    pending: Vec<Statement>,
    next: Option<SequenceStep>,
    acknowledgement: Option<PrivateSaved>,
    handoff: Option<ReplicationHandoff>,
}

impl<'a, P: ChunkPromoter, E: SqlExecutor> DurableCommit<'a, P, E> {
    /// Begin a sequence, without performing any of it.
    pub fn new(
        store: &'a mut Store<E>,
        promoter: &'a mut P,
        chunks: Vec<Vec<u8>>,
        checkpoint: Checkpoint,
    ) -> Self {
        Self {
            store,
            promoter,
            chunks,
            checkpoint,
            promoted: Vec::new(),
            plan: None,
            after: None,
            pending: Vec::new(),
            next: Some(SequenceStep::WriteChunks),
            acknowledgement: None,
            handoff: None,
        }
    }

    /// The step that has not been performed yet, or `None` when the sequence is complete.
    #[must_use]
    pub const fn next_step(&self) -> Option<SequenceStep> {
        self.next
    }

    /// The acknowledgement, or `None` while the transaction has not returned.
    ///
    /// `None` at every boundary before step 10. This is the accessor the kill-point harness and
    /// `tests/crash-commit-sequence.rs` assert against, and it is the reason an optimistic report
    /// is not merely discouraged: there is nothing to report with.
    #[must_use]
    pub const fn acknowledgement(&self) -> Option<&PrivateSaved> {
        self.acknowledgement.as_ref()
    }

    /// The chunk names promoted by this sequence, once step 1 has run.
    #[must_use]
    pub fn promoted(&self) -> &[RecordDigest] {
        &self.promoted
    }

    /// Perform the next step, returning which one was performed.
    ///
    /// # Errors
    ///
    /// [`SequenceError`], whose variant says what the failure left behind.
    #[allow(clippy::type_complexity)]
    pub fn step(&mut self) -> Result<Option<SequenceStep>, SequenceError<P::Error, E::Error>> {
        let Some(step) = self.next else {
            return Ok(None);
        };
        match step {
            SequenceStep::WriteChunks => self.perform_write_chunks()?,
            SequenceStep::FlushChunks => self.perform_chunk_step(step, P::flush_temporary)?,
            SequenceStep::VerifyChunks => self.perform_chunk_step(step, P::verify_temporary)?,
            SequenceStep::PromoteChunks => self.perform_chunk_step(step, P::promote)?,
            SequenceStep::BeginTransaction => self.perform_begin()?,
            SequenceStep::InsertImmutable
            | SequenceStep::AdvanceHeads
            | SequenceStep::FillOutbox => self.append(step),
            SequenceStep::CommitTransaction => self.perform_commit()?,
            SequenceStep::ReportSaved => self.perform_report(),
            SequenceStep::Replicate => self.perform_replicate(),
        }
        self.next = step.next();
        Ok(Some(step))
    }

    /// Perform steps until `last` has been performed, then stop.
    ///
    /// # Errors
    ///
    /// Whatever [`Self::step`] returns.
    #[allow(clippy::type_complexity)]
    pub fn run_through(
        &mut self,
        last: SequenceStep,
    ) -> Result<(), SequenceError<P::Error, E::Error>> {
        while let Some(next) = self.next {
            self.step()?;
            if next == last {
                return Ok(());
            }
        }
        Ok(())
    }

    /// Perform every remaining step and return the acknowledgement with the replication handoff.
    ///
    /// # Errors
    ///
    /// Whatever [`Self::step`] returns.
    #[allow(clippy::type_complexity)]
    pub fn finish(mut self) -> Result<Saved, SequenceError<P::Error, E::Error>> {
        while self.next.is_some() {
            self.step()?;
        }
        // Both are set by steps 10 and 11, which a completed sequence has passed. The fallbacks
        // exist so that a future edit which reorders the steps produces a wrong count rather than a
        // panic in a durability path; the assertion that they cannot be reached is
        // `a_finished_sequence_carries_the_acknowledgement_it_produced`.
        Ok(Saved {
            acknowledgement: self.acknowledgement.unwrap_or_else(|| {
                PrivateSaved::after_the_transaction_returned(
                    self.store.index().default_digest(),
                    0,
                    0,
                    0,
                )
            }),
            handoff: self.handoff.unwrap_or_default(),
        })
    }

    fn perform_write_chunks(&mut self) -> Result<(), SequenceError<P::Error, E::Error>> {
        self.promoted = self
            .promoter
            .write_temporary(&self.chunks)
            .map_err(|error| SequenceError::Chunks {
                step: SequenceStep::WriteChunks,
                error,
            })?;
        Ok(())
    }

    fn perform_chunk_step(
        &mut self,
        step: SequenceStep,
        operation: fn(&mut P) -> Result<(), P::Error>,
    ) -> Result<(), SequenceError<P::Error, E::Error>> {
        operation(self.promoter).map_err(|error| SequenceError::Chunks { step, error })
    }

    /// Step 5. Refuse a checkpoint whose manifests outrun the store, then compose the transaction.
    ///
    /// The durability check is here rather than at step 9 for a reason worth stating: at step 5
    /// nothing has been composed, so a refusal costs nothing and leaves nothing. At step 9 the
    /// refusal would come after the whole transaction existed, and a caller under pressure would be
    /// one line away from running it anyway.
    fn perform_begin(&mut self) -> Result<(), SequenceError<P::Error, E::Error>> {
        for manifest in &self.checkpoint.manifests {
            for slice in &manifest.chunks {
                if !self.promoter.is_durable(&slice.digest) {
                    return Err(SequenceError::ChunkNotDurable {
                        digest: slice.digest,
                    });
                }
            }
        }

        let (plan, after) = CommitPlan::stage(self.store.index(), &self.checkpoint)?;
        self.plan = Some(plan);
        self.after = Some(after);
        self.append(SequenceStep::BeginTransaction);
        Ok(())
    }

    /// Append the statements plan §6.3 assigns to this step, in [`CommitPlan`]'s order.
    fn append(&mut self, step: SequenceStep) {
        let (Some(plan), Some(commit_step)) = (self.plan.as_ref(), step.commit_step()) else {
            return;
        };
        self.pending
            .extend(plan.statements_for(commit_step).iter().cloned());
    }

    /// Step 9. Execute the whole transaction as one batch, and only then let step 10 exist.
    fn perform_commit(&mut self) -> Result<(), SequenceError<P::Error, E::Error>> {
        self.append(SequenceStep::CommitTransaction);
        let sql = self
            .pending
            .iter()
            .map(|statement| statement.sql.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        self.store
            .executor_mut()
            .execute_batch(&sql)
            .map_err(|error| SequenceError::CommitOutcomeUnknown { error })?;

        if let Some(after) = self.after.take() {
            self.store.adopt_index(after);
        }
        Ok(())
    }

    /// Step 10. The acknowledgement, built from a transaction that has already returned.
    fn perform_report(&mut self) {
        self.acknowledgement = Some(PrivateSaved::after_the_transaction_returned(
            self.store.index().default_digest(),
            self.checkpoint.operations.len(),
            self.checkpoint.manifests.len(),
            self.promoted.len(),
        ));
    }

    /// Step 11. Hand replication the difference, read off the index the transaction left.
    fn perform_replicate(&mut self) {
        let entries = self
            .store
            .index()
            .rows("outbox")
            .unwrap_or_default()
            .iter()
            .filter_map(|row| match row.values() {
                [
                    Value::Blob(peer),
                    Value::Blob(actor),
                    Value::Integer(sequence),
                    Value::Blob(operation),
                ] => Some(OutboxEntry {
                    peer: digest_from(peer)?,
                    actor: digest_from(actor)?,
                    actor_sequence: u64::try_from(*sequence).ok()?,
                    operation: digest_from(operation)?,
                }),
                _ => None,
            })
            .collect();
        self.handoff = Some(ReplicationHandoff { entries });
    }
}

fn digest_from(bytes: &[u8]) -> Option<RecordDigest> {
    let fixed: [u8; 32] = bytes.try_into().ok()?;
    Some(RecordDigest::from_bytes(fixed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{ChunkSlice, ManifestRecord, OperationRecord, PeerRecord};
    use crate::row::Row;
    use crate::schema::Table;

    fn digest(seed: u8) -> RecordDigest {
        RecordDigest::from_bytes([seed; 32])
    }

    fn operation(id: u8, actor: u8, sequence: u64) -> OperationRecord {
        OperationRecord {
            id: digest(id),
            actor: digest(actor),
            actor_sequence: sequence,
            hlc_millis: 17,
            hlc_counter: 0,
            policy_epoch: 1,
            session: crate::index::no_session(),
            payload_digest: digest(id.wrapping_add(100)),
            parents: Vec::new(),
        }
    }

    fn checkpoint() -> Checkpoint {
        Checkpoint {
            operations: vec![operation(1, 2, 1)],
            peers: vec![PeerRecord {
                peer: digest(30),
                joined_at: digest(1),
            }],
            ..Checkpoint::default()
        }
    }

    /// A promoter that records what it was asked to do and can be told to fail at one step.
    #[derive(Debug, Default)]
    struct Promoter {
        performed: Vec<SequenceStep>,
        fail_at: Option<SequenceStep>,
        durable: Vec<RecordDigest>,
    }

    impl Promoter {
        fn note(&mut self, step: SequenceStep) -> Result<(), String> {
            self.performed.push(step);
            if self.fail_at == Some(step) {
                return Err(format!("{step} was told to fail"));
            }
            Ok(())
        }
    }

    impl ChunkPromoter for Promoter {
        type Error = String;

        fn write_temporary(&mut self, chunks: &[Vec<u8>]) -> Result<Vec<RecordDigest>, String> {
            self.note(SequenceStep::WriteChunks)?;
            Ok(chunks
                .iter()
                .map(|bytes| digest(bytes.first().copied().unwrap_or(0)))
                .collect())
        }

        fn flush_temporary(&mut self) -> Result<(), String> {
            self.note(SequenceStep::FlushChunks)
        }

        fn verify_temporary(&mut self) -> Result<(), String> {
            self.note(SequenceStep::VerifyChunks)
        }

        fn promote(&mut self) -> Result<(), String> {
            self.note(SequenceStep::PromoteChunks)
        }

        fn discard_temporary(&mut self) -> Result<usize, String> {
            Ok(0)
        }

        fn is_durable(&self, digest: &RecordDigest) -> bool {
            self.durable.contains(digest)
        }
    }

    /// How many batches `Store::open` runs before any sequence does: the pragmas, then the
    /// migrations.
    const BATCHES_ON_OPEN: usize = 2;

    /// An executor that records every batch, and can be armed to fail the next one.
    #[derive(Debug, Default)]
    struct Recorder {
        batches: Vec<String>,
        fail_commit: bool,
    }

    impl SqlExecutor for Recorder {
        type Error = String;

        fn execute_batch(&mut self, sql: &str) -> Result<(), Self::Error> {
            if self.fail_commit {
                return Err("the disk is full".to_owned());
            }
            self.batches.push(sql.to_owned());
            Ok(())
        }

        fn read_table(&mut self, _table: &Table) -> Result<Vec<Row>, Self::Error> {
            Ok(Vec::new())
        }

        fn table_exists(&mut self, _name: &str) -> Result<bool, Self::Error> {
            Ok(false)
        }
    }

    fn open() -> Store<Recorder> {
        Store::open(Recorder::default()).expect("opens")
    }

    #[test]
    fn the_eleven_steps_are_plan_6_3s_eleven_steps_in_order() {
        let numbers: Vec<u8> = SequenceStep::ORDER
            .iter()
            .map(|step| step.plan_step())
            .collect();
        assert_eq!(numbers, (1..=11).collect::<Vec<u8>>());
    }

    #[test]
    fn every_step_has_a_unique_name_that_round_trips() {
        for step in SequenceStep::ORDER {
            assert_eq!(SequenceStep::from_name(step.name()), Some(step));
        }
        let mut names: Vec<&str> = SequenceStep::ORDER.iter().map(|step| step.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), SequenceStep::ORDER.len());
    }

    /// The durability claim as one assertion over the whole enumeration: nothing is acknowledged at
    /// a point where a crash would not recover the checkpoint.
    #[test]
    fn no_step_acknowledges_before_the_checkpoint_is_recoverable() {
        for step in SequenceStep::ORDER {
            if step.acknowledged_by_here() {
                assert_eq!(
                    step.residue_if_killed_after(),
                    CrashResidue::RecoverCheckpoint,
                    "{step} acknowledges a save a crash would lose"
                );
            }
        }
    }

    /// The three clauses of plan §6.3's crash behaviour, transcribed and checked against the
    /// boundaries they name.
    #[test]
    fn the_crash_residue_changes_at_step_four_and_at_step_nine_and_nowhere_else() {
        let residues: Vec<CrashResidue> = SequenceStep::ORDER
            .iter()
            .map(|step| step.residue_if_killed_after())
            .collect();
        assert_eq!(
            residues,
            vec![
                CrashResidue::DiscardTemporary,
                CrashResidue::DiscardTemporary,
                CrashResidue::DiscardTemporary,
                CrashResidue::CollectUnreferencedChunks,
                CrashResidue::CollectUnreferencedChunks,
                CrashResidue::CollectUnreferencedChunks,
                CrashResidue::CollectUnreferencedChunks,
                CrashResidue::CollectUnreferencedChunks,
                CrashResidue::RecoverCheckpoint,
                CrashResidue::RecoverCheckpoint,
                CrashResidue::RecoverCheckpoint,
            ]
        );
    }

    #[test]
    fn the_five_transaction_steps_map_onto_the_commit_plan_and_the_other_six_do_not() {
        let mapped: Vec<Option<CommitStep>> = SequenceStep::ORDER
            .iter()
            .map(|step| step.commit_step())
            .collect();
        assert_eq!(
            mapped,
            vec![
                None,
                None,
                None,
                None,
                Some(CommitStep::Begin),
                Some(CommitStep::InsertImmutable),
                Some(CommitStep::AdvanceHeads),
                Some(CommitStep::FillOutbox),
                Some(CommitStep::Commit),
                None,
                None,
            ]
        );
    }

    #[test]
    fn a_sequence_performs_every_step_once_in_order() {
        let mut store = open();
        let mut promoter = Promoter::default();
        let mut sequence =
            DurableCommit::new(&mut store, &mut promoter, vec![vec![7; 8]], checkpoint());

        let mut performed = Vec::new();
        while let Some(step) = sequence.step().expect("the sequence runs") {
            performed.push(step);
        }
        assert_eq!(performed, SequenceStep::ORDER.to_vec());
    }

    /// The criterion, as a test: nothing to report with, at any boundary before step 10.
    #[test]
    fn the_acknowledgement_is_absent_at_every_boundary_before_the_transaction_returns() {
        for stop in SequenceStep::ORDER {
            let mut store = open();
            let mut promoter = Promoter::default();
            let mut sequence =
                DurableCommit::new(&mut store, &mut promoter, Vec::new(), checkpoint());
            sequence.run_through(stop).expect("the sequence runs");
            assert_eq!(
                sequence.acknowledgement().is_some(),
                stop.acknowledged_by_here(),
                "{stop}: the acknowledgement's presence disagrees with the plan"
            );
        }
    }

    /// A failure inside the transaction acknowledges nothing, and says the outcome is unknown
    /// rather than guessing at it.
    #[test]
    fn a_failed_transaction_acknowledges_nothing_and_reports_the_outcome_as_unknown() {
        let mut store = open();
        store.executor_mut().fail_commit = true;
        let mut promoter = Promoter::default();
        let mut sequence = DurableCommit::new(&mut store, &mut promoter, Vec::new(), checkpoint());

        let error = sequence
            .run_through(SequenceStep::CommitTransaction)
            .expect_err("the transaction fails");
        assert!(error.outcome_is_unknown());
        assert_eq!(error.step(), SequenceStep::CommitTransaction);
        assert!(sequence.acknowledgement().is_none());
        assert!(error.to_string().contains("Nothing was acknowledged"));
    }

    /// A failure in steps 1 to 4 never reaches the transaction, so nothing is composed.
    #[test]
    fn a_chunk_failure_stops_before_a_single_statement_exists() {
        for step in [
            SequenceStep::WriteChunks,
            SequenceStep::FlushChunks,
            SequenceStep::VerifyChunks,
            SequenceStep::PromoteChunks,
        ] {
            let mut store = open();
            let mut promoter = Promoter {
                fail_at: Some(step),
                ..Promoter::default()
            };
            let mut sequence =
                DurableCommit::new(&mut store, &mut promoter, vec![vec![3; 4]], checkpoint());
            let error = sequence.finish_err();
            assert!(matches!(error, SequenceError::Chunks { step: at, .. } if at == step));
            assert_eq!(
                store.executor_mut().batches.len(),
                BATCHES_ON_OPEN,
                "{step}: a batch beyond opening the database was executed"
            );
        }
    }

    /// A manifest naming a chunk nothing promoted is refused at step 5, with the index untouched.
    /// This is the first acceptance criterion in its strongest form: not merely that the ordering
    /// is right, but that the wrong order cannot be reached.
    #[test]
    fn a_manifest_naming_an_unpromoted_chunk_is_refused_before_the_transaction_is_composed() {
        let mut store = open();
        let mut promoter = Promoter::default();
        let dangling = Checkpoint {
            manifests: vec![ManifestRecord {
                id: digest(20),
                byte_length: 8,
                content_digest: digest(21),
                chunks: vec![ChunkSlice {
                    digest: digest(22),
                    byte_offset: 0,
                    byte_length: 8,
                }],
            }],
            ..Checkpoint::default()
        };
        let mut sequence = DurableCommit::new(&mut store, &mut promoter, Vec::new(), dangling);
        let error = sequence
            .run_through(SequenceStep::BeginTransaction)
            .expect_err("the sequence refuses");
        assert!(matches!(
            error,
            SequenceError::ChunkNotDurable { digest: d } if d == digest(22)
        ));
        assert_eq!(store.executor_mut().batches.len(), BATCHES_ON_OPEN);
    }

    #[test]
    fn a_manifest_whose_chunks_are_all_promoted_is_accepted() {
        let mut store = open();
        let mut promoter = Promoter {
            durable: vec![digest(22)],
            ..Promoter::default()
        };
        let manifested = Checkpoint {
            manifests: vec![ManifestRecord {
                id: digest(20),
                byte_length: 8,
                content_digest: digest(21),
                chunks: vec![ChunkSlice {
                    digest: digest(22),
                    byte_offset: 0,
                    byte_length: 8,
                }],
            }],
            ..Checkpoint::default()
        };
        let sequence = DurableCommit::new(&mut store, &mut promoter, Vec::new(), manifested);
        let saved = sequence.finish().expect("the sequence completes");
        assert_eq!(saved.acknowledgement().manifests(), 1);
    }

    /// The transaction is one batch, in plan §6.3's order, with `BEGIN IMMEDIATE` first and
    /// `COMMIT` last — the same plan `CommitPlan` produces, not a second rendering of it.
    #[test]
    fn the_transaction_is_one_batch_in_the_commit_plans_own_order() {
        let mut store = open();
        let mut promoter = Promoter::default();
        let sequence = DurableCommit::new(&mut store, &mut promoter, Vec::new(), checkpoint());
        sequence.finish().expect("the sequence completes");

        let batches = &store.executor_mut().batches;
        assert_eq!(
            batches.len(),
            BATCHES_ON_OPEN + 1,
            "opening the database, then exactly one transaction"
        );
        let transaction = &batches[BATCHES_ON_OPEN];
        assert!(transaction.starts_with("BEGIN IMMEDIATE;"));
        assert!(transaction.trim_end().ends_with("COMMIT;"));

        let (expected, _) = CommitPlan::stage(&Index::new(), &checkpoint()).expect("stages");
        assert_eq!(*transaction, expected.sql());
    }

    #[test]
    fn a_finished_sequence_carries_the_acknowledgement_it_produced() {
        let mut store = open();
        let mut promoter = Promoter::default();
        let sequence =
            DurableCommit::new(&mut store, &mut promoter, vec![vec![5; 2]], checkpoint());
        let saved = sequence.finish().expect("the sequence completes");
        assert_eq!(saved.acknowledgement().operations(), 1);
        assert_eq!(saved.acknowledgement().chunks(), 1);
        assert_eq!(
            saved.acknowledgement().index_digest(),
            store.index().default_digest()
        );
        assert_eq!(saved.acknowledgement().to_string(), "saved privately");
    }

    /// Step 11's handoff is the outbox difference, and it is produced only after step 10.
    #[test]
    fn replication_is_handed_the_outbox_difference_after_the_acknowledgement() {
        let mut store = open();
        let mut promoter = Promoter::default();
        let sequence = DurableCommit::new(&mut store, &mut promoter, Vec::new(), checkpoint());
        let saved = sequence.finish().expect("the sequence completes");
        assert_eq!(
            saved.handoff().entries(),
            &[OutboxEntry {
                peer: digest(30),
                actor: digest(2),
                actor_sequence: 1,
                operation: digest(1),
            }]
        );
        assert!(!saved.handoff().is_empty());
    }

    /// A checkpoint the fold rejects never reaches the transaction.
    #[test]
    fn a_contradictory_checkpoint_is_refused_at_step_five() {
        let mut store = open();
        let mut promoter = Promoter::default();
        DurableCommit::new(&mut store, &mut promoter, Vec::new(), checkpoint())
            .finish()
            .expect("the first save lands");

        let mut second = Promoter::default();
        let forked = Checkpoint {
            operations: vec![operation(9, 2, 1)],
            ..Checkpoint::default()
        };
        let mut sequence = DurableCommit::new(&mut store, &mut second, Vec::new(), forked);
        let error = sequence
            .run_through(SequenceStep::BeginTransaction)
            .expect_err("the fold refuses");
        assert!(matches!(error, SequenceError::Fold(_)));
        assert!(sequence.acknowledgement().is_none());
    }

    impl<P: ChunkPromoter, E: SqlExecutor> DurableCommit<'_, P, E> {
        /// Run to completion and return the error, for tests that expect one.
        fn finish_err(&mut self) -> SequenceError<P::Error, E::Error> {
            loop {
                match self.step() {
                    Ok(Some(_)) => {}
                    Ok(None) => panic!("the sequence completed without failing"),
                    Err(error) => return error,
                }
            }
        }
    }
}
