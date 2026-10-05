//! The immutable records every table is a fold over.
//!
//! This module is the whole content of the load-bearing rule. Plan §6.3's recovery clause says
//! "index corruption: rebuild from immutable operations and manifests", and the task states the
//! rule in its sharper form: **if a fact cannot be rebuilt, it does not belong in this database.**
//!
//! A rule stated in prose is a rule nothing enforces. So the rule is expressed as a type: every
//! table in [`crate::TABLES`] declares the [`RecordKind`]s that feed it, a table declaring none is
//! rejected by `tests/reconstruction.rs`, and [`crate::rebuild`] replays a stream of these records
//! into an index that must digest identically to the one the live path built.
//!
//! Two entries in plan §6.1's storage list are the ones that make the rule bite:
//!
//! * **The peer watermark.** A watermark is not a counter the sync engine increments — a counter
//!   is unrecoverable state. It is the fold of [`AckRecord`]s, each an immutable receipt naming
//!   what a peer acknowledged. Written that way it is reconstructable; written as a counter it is
//!   not, and it would not belong here.
//! * **The outbox.** Not a queue with rows deleted on send. It is a *difference*: every operation
//!   whose actor sequence a peer's watermark has not yet reached. Sending nothing and losing the
//!   whole database still yields the same outbox on rebuild.

use crate::ids::{EntityUuid, RecordDigest};

/// Which immutable record a [`StoredRecord`] is.
///
/// A table's [`crate::Provenance`] names these, which is how "reconstructable" stops being a claim
/// in a comment and becomes something a test reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RecordKind {
    /// An operation — plan §6.1's "canonical immutable operations", a `mesh-types` ChangeSet as it
    /// is indexed locally.
    Operation,
    /// A file manifest.
    Manifest,
    /// A peer joining this workspace's replication set.
    Peer,
    /// A peer's acknowledgement receipt.
    Acknowledgement,
    /// A review bundle being opened over an operation.
    Review,
    /// An approval envelope over a review bundle.
    Approval,
    /// One context-ledger entry.
    ContextEntry,
    /// Native dependency-policy envelope; storage alone grants no authority.
    Dependency,
}

impl RecordKind {
    /// Every kind, in a fixed order, so a test can enumerate them without a hand-written list
    /// going stale.
    pub const ALL: &'static [Self] = &[
        Self::Operation,
        Self::Manifest,
        Self::Peer,
        Self::Acknowledgement,
        Self::Review,
        Self::Approval,
        Self::ContextEntry,
        Self::Dependency,
    ];
}

/// One immutable record, as the local index receives it.
///
/// The stream of these is the *only* input [`crate::rebuild`] is given. Anything the index holds
/// that this stream cannot produce is, by the rule above, a fact in the wrong place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoredRecord {
    /// An operation.
    Operation(OperationRecord),
    /// A file manifest.
    Manifest(ManifestRecord),
    /// A peer joining.
    Peer(PeerRecord),
    /// A peer acknowledgement.
    Acknowledgement(AckRecord),
    /// A review bundle.
    Review(ReviewRecord),
    /// An approval.
    Approval(ApprovalRecord),
    /// A context-ledger entry.
    ContextEntry(ContextRecord),
    /// One ordered native dependency-policy envelope.
    Dependency(DependencyRecord),
}

impl StoredRecord {
    /// Which kind this record is.
    #[must_use]
    pub const fn kind(&self) -> RecordKind {
        match self {
            Self::Operation(_) => RecordKind::Operation,
            Self::Manifest(_) => RecordKind::Manifest,
            Self::Peer(_) => RecordKind::Peer,
            Self::Acknowledgement(_) => RecordKind::Acknowledgement,
            Self::Review(_) => RecordKind::Review,
            Self::Approval(_) => RecordKind::Approval,
            Self::ContextEntry(_) => RecordKind::ContextEntry,
            Self::Dependency(_) => RecordKind::Dependency,
        }
    }
}

/// The native meaning declared by a dependency envelope, not proof of that meaning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DependencyKind {
    /// Establish the native authority binding before any subsequent policy facts.
    Enrollment,
    /// Explicit access authorization for an exact input and destination.
    Grant,
    /// Verified immutable input actually consumed by a destination.
    Consumption,
    /// A separately versioned eligibility decision for an exact input.
    Eligibility,
    /// An exact closure and decision vector retained for review.
    ReviewSnapshot,
    /// Required destination intent, synchronized before materializing any consumed input.
    ConsumptionStart,
    /// Required destination acknowledgement of its exact owner-authority consumption record.
    ConsumptionComplete,
    /// Required native publication claim; receipt and authority verification remain separate.
    Publication,
}
impl DependencyKind {
    /// Stable journal and SQL code. Unknown values must refuse, never default.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Enrollment => 0,
            Self::Grant => 1,
            Self::Consumption => 2,
            Self::Eligibility => 3,
            Self::ReviewSnapshot => 4,
            Self::ConsumptionStart => 5,
            Self::ConsumptionComplete => 6,
            Self::Publication => 7,
        }
    }
    /// Decode only explicitly supported native envelope kinds.
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Enrollment),
            1 => Some(Self::Grant),
            2 => Some(Self::Consumption),
            3 => Some(Self::Eligibility),
            4 => Some(Self::ReviewSnapshot),
            5 => Some(Self::ConsumptionStart),
            6 => Some(Self::ConsumptionComplete),
            7 => Some(Self::Publication),
            _ => None,
        }
    }
}

/// Immutable dependency storage envelope. The native validator must verify the referenced
/// canonical payload, authority, exact work/version/grant/closure bindings and its own matching
/// envelope fields before interpreting it. Checksums, index membership and record order are
/// durability facts, never access or publication authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DependencyRecord {
    /// Stable native project authority identity, never an agent-selected path.
    pub authority: RecordDigest,
    /// One-based ledger ordinal, not a per-input eligibility revision or workspace policy epoch.
    pub revision: u64,
    /// Previous envelope's payload identity; zero only for enrollment at revision one.
    pub previous: RecordDigest,
    /// Digest of the complete canonical native payload including this envelope's bindings.
    pub payload: RecordDigest,
    /// Native payload class; interpretation requires validation outside this index.
    pub kind: DependencyKind,
}

/// An operation as the local index holds it.
///
/// Ordering fields are causal, never wall-clock: `actor_sequence` orders one actor's own
/// operations and `parents` carries the causal edges. `hlc_millis` is display and tie-breaking
/// only and decides nothing, which is why it is not part of any index key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationRecord {
    /// The operation's own content digest — a `mesh-types` `ChangeSetId` as it is stored.
    pub id: RecordDigest,
    /// The actor that authored it.
    pub actor: RecordDigest,
    /// The author's own monotonic sequence number. A gap here is a missing operation.
    pub actor_sequence: u64,
    /// The hybrid-logical millisecond, carried for display and tie-breaking.
    pub hlc_millis: u64,
    /// The hybrid-logical counter that breaks ties inside one millisecond.
    pub hlc_counter: u64,
    /// The policy epoch the operation was authored under.
    pub policy_epoch: u64,
    /// The activity session it was authored in.
    pub session: EntityUuid,
    /// The digest of the operation's payload, which lives in the content-addressed store and never
    /// in this database.
    pub payload_digest: RecordDigest,
    /// The operations this one causally follows, in the order the author sealed them.
    pub parents: Vec<RecordDigest>,
}

/// A file manifest as the local index holds it. Chunk *bytes* are never here — plan §6.2 puts them
/// in the content-addressed store, and this task's "out of scope" says so outright.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestRecord {
    /// The manifest's own content digest.
    pub id: RecordDigest,
    /// The reconstructed file's length in bytes.
    pub byte_length: u64,
    /// The digest of the reconstructed bytes, so a reassembly can be verified end to end.
    pub content_digest: RecordDigest,
    /// The chunks, in reconstruction order.
    pub chunks: Vec<ChunkSlice>,
}

/// One chunk's place in a manifest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkSlice {
    /// The chunk's content digest, under which it is named in the content-addressed store.
    pub digest: RecordDigest,
    /// Where the chunk starts in the reconstructed file.
    pub byte_offset: u64,
    /// How many bytes of the reconstructed file the chunk supplies.
    pub byte_length: u64,
}

/// A peer joining this workspace's replication set.
///
/// This record exists so the outbox is reconstructable. An outbox row is "operation O is not yet
/// at peer P", which needs the set of peers; a set of peers held only in a mutable table would be
/// a fact with no immutable source, and the rule would put it outside this database.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeerRecord {
    /// The peer, named by its actor identifier.
    pub peer: RecordDigest,
    /// The operation at which it joined, which is what makes joining causally ordered.
    pub joined_at: RecordDigest,
}

/// A peer's acknowledgement that it holds an actor's operations up to a sequence number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AckRecord {
    /// The acknowledging peer.
    pub peer: RecordDigest,
    /// Whose operations are being acknowledged.
    pub actor: RecordDigest,
    /// The highest contiguous actor sequence the peer holds.
    pub actor_sequence: u64,
}

/// A review bundle opened over an operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReviewRecord {
    /// The bundle's own content digest.
    pub bundle: RecordDigest,
    /// The operation under review.
    pub subject_operation: RecordDigest,
    /// The actor that opened the review.
    pub opened_by: RecordDigest,
}

/// An approval envelope over a review bundle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApprovalRecord {
    /// The approval's own content digest.
    pub approval: RecordDigest,
    /// The bundle it decides.
    pub bundle: RecordDigest,
    /// The actor that decided.
    pub approver: RecordDigest,
    /// What was decided.
    pub verdict: ReviewVerdict,
}

/// What an approver decided.
///
/// Stored as the integer code below rather than as text, because a `STRICT` table with a
/// `CHECK (verdict IN (…))` turns the enumeration into something SQLite refuses to violate, and
/// because keeping every record-derived column integer or blob is what lets the writer render SQL
/// without ever quoting a string.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReviewVerdict {
    /// The bundle was approved.
    Approved,
    /// Changes were requested.
    ChangesRequested,
    /// The approval was withdrawn by a later envelope.
    Withdrawn,
}

impl ReviewVerdict {
    /// The integer the `verdict` column holds.
    #[must_use]
    pub const fn code(self) -> i64 {
        match self {
            Self::Approved => 0,
            Self::ChangesRequested => 1,
            Self::Withdrawn => 2,
        }
    }

    /// Every verdict, in code order.
    pub const ALL: &'static [Self] = &[Self::Approved, Self::ChangesRequested, Self::Withdrawn];
}

/// How a session touched an operation's content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ContextAccess {
    /// The session read the content.
    Read,
    /// The session wrote the content.
    Wrote,
    /// The session referenced the content without reading its bytes.
    Referenced,
}

impl ContextAccess {
    /// The integer the `access` column holds.
    #[must_use]
    pub const fn code(self) -> i64 {
        match self {
            Self::Read => 0,
            Self::Wrote => 1,
            Self::Referenced => 2,
        }
    }

    /// Every access, in code order.
    pub const ALL: &'static [Self] = &[Self::Read, Self::Wrote, Self::Referenced];
}

/// One context-ledger entry: which session touched which operation's content, and how.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextRecord {
    /// The entry's own content digest.
    pub entry: RecordDigest,
    /// The session that did the touching.
    pub session: EntityUuid,
    /// The operation whose content was touched.
    pub operation: RecordDigest,
    /// How it was touched.
    pub access: ContextAccess,
    /// How many bytes were involved.
    pub byte_length: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_publication_kind_has_a_distinct_required_code() {
        let publication = DependencyKind::from_code(7)
            .expect("native publication requires its own durable envelope kind");
        assert_eq!(publication.code(), 7);
        assert!((0..7).all(|code| DependencyKind::from_code(code) != Some(publication)));
        assert!(DependencyKind::from_code(8).is_none());
    }

    fn digest(seed: u8) -> RecordDigest {
        RecordDigest::from_bytes([seed; 32])
    }

    #[test]
    fn every_kind_is_reachable_from_a_record() {
        let records = [
            StoredRecord::Operation(OperationRecord {
                id: digest(1),
                actor: digest(2),
                actor_sequence: 1,
                hlc_millis: 0,
                hlc_counter: 0,
                policy_epoch: 0,
                session: EntityUuid::from_bytes([3; 16]),
                payload_digest: digest(4),
                parents: Vec::new(),
            }),
            StoredRecord::Manifest(ManifestRecord {
                id: digest(5),
                byte_length: 0,
                content_digest: digest(6),
                chunks: Vec::new(),
            }),
            StoredRecord::Peer(PeerRecord {
                peer: digest(7),
                joined_at: digest(1),
            }),
            StoredRecord::Acknowledgement(AckRecord {
                peer: digest(7),
                actor: digest(2),
                actor_sequence: 1,
            }),
            StoredRecord::Review(ReviewRecord {
                bundle: digest(8),
                subject_operation: digest(1),
                opened_by: digest(2),
            }),
            StoredRecord::Approval(ApprovalRecord {
                approval: digest(9),
                bundle: digest(8),
                approver: digest(2),
                verdict: ReviewVerdict::Approved,
            }),
            StoredRecord::ContextEntry(ContextRecord {
                entry: digest(10),
                session: EntityUuid::from_bytes([3; 16]),
                operation: digest(1),
                access: ContextAccess::Read,
                byte_length: 12,
            }),
            StoredRecord::Dependency(DependencyRecord {
                authority: digest(20),
                revision: 1,
                previous: digest(0),
                payload: digest(21),
                kind: DependencyKind::Enrollment,
            }),
        ];
        let seen: Vec<RecordKind> = records.iter().map(StoredRecord::kind).collect();
        assert_eq!(seen, RecordKind::ALL.to_vec());
    }

    /// A code collision would silently merge two meanings inside a `CHECK` constraint.
    #[test]
    fn enumeration_codes_are_distinct_and_contiguous() {
        let verdicts: Vec<i64> = ReviewVerdict::ALL.iter().map(|v| v.code()).collect();
        assert_eq!(verdicts, vec![0, 1, 2]);
        let accesses: Vec<i64> = ContextAccess::ALL.iter().map(|a| a.code()).collect();
        assert_eq!(accesses, vec![0, 1, 2]);
    }
}
