//! The eighteen CWP messages, their fields and the preconditions each one carries on its own.
//!
//! # Records this crate does not own are carried opaque
//!
//! A ChangeSet, a review bundle, a validation receipt and an approval envelope all have exactly
//! one encoding, and it is `mesh-types`' `mesh-cbor/0` — the encoding their signatures were made
//! over. So this crate carries each of them as **its canonical bytes plus its record identifier**,
//! and never as a second set of fields.
//!
//! That is not laziness, it is the only way the wire format cannot fork from the signed format. A
//! message set that re-declared a ChangeSet's fields would be a second description of a signed
//! record; the day the two disagreed, a peer would verify a signature over bytes it reconstructed
//! rather than over the bytes the author signed. The fields that *are* spelled out on
//! [`CarriedChangeSet`] are the ones delivery needs in order to decide what to ask for next, and
//! every one of them is redundant with the body — [`crate::ReplicationGap`] uses them to plan, and
//! the receiver re-derives them from the body before believing anything.
//!
//! # Preconditions
//!
//! [`SyncMessage::check`] holds the preconditions a message can be judged against *alone*: a batch
//! is non-empty, an offset is inside its chunk, a sparse set names nothing already covered by the
//! contiguous sequence. The preconditions that need a session — protocol version, an established
//! and admitted peer — are [`crate::Session`]'s, because they are not properties of the message.

use crate::error::{ErrorCode, ProtocolError};
use crate::ids::{
    ActorId, ActorSequence, ApprovalId, ChangeSetId, ContentHash, HeadId, ManifestId, PolicyEpoch,
    ReviewBundleId,
};
use crate::plane::{MessageKind, MessagePlane};
use crate::summary::MerkleSummary;

/// The version of the CWP message set this crate implements.
///
/// A peer that receives a `HELLO` naming a different major version answers
/// [`ErrorCode::UnsupportedVersion`] rather than negotiating: there is one version today, and
/// pretending to negotiate before a second one exists is untested code on the security boundary.
pub const PROTOCOL_VERSION: u32 = 0;

/// The largest number of ChangeSets one `OPERATIONS_BATCH` may carry.
///
/// A bound the *receiver* enforces, so a peer cannot make a receiver allocate without limit by
/// claiming a large batch. It is deliberately small enough that a batch is a unit of progress
/// rather than a unit of history.
pub const MAX_OPERATIONS_PER_BATCH: usize = 256;

/// The largest number of chunk bytes one `CHUNK_BATCH` element may carry.
///
/// One mebibyte. Resumable transfer means a large chunk arrives as several parts, so this bounds
/// the receiver's buffer without bounding the chunk.
pub const MAX_CHUNK_PART_BYTES: usize = 1024 * 1024;

/// One ChangeSet a peer holds beyond its contiguous run, with the sequence it sits at.
///
/// The sequence travels with the identifier because without it the receiver cannot tell *which
/// hole* the ChangeSet is beyond, and so cannot ask for the ones between.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SparseChangeSet {
    /// Where it sits in its author's sequence.
    pub sequence: ActorSequence,
    /// Which ChangeSet it is.
    pub id: ChangeSetId,
}

/// What one peer advertises about one actor: the head it holds and how much of that actor's
/// sequence it holds contiguously, plus anything it holds beyond the contiguous part.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HeadAdvertisement {
    /// The actor whose head this is.
    pub actor: ActorId,
    /// The actor head the advertiser holds for that actor.
    pub head: HeadId,
    /// The highest sequence number for which every preceding sequence is also held.
    pub contiguous_through: ActorSequence,
    /// ChangeSets held beyond the contiguous run, ascending by sequence, each still missing a
    /// predecessor. Ordered so that two peers holding the same set advertise the same bytes.
    pub sparse: Vec<SparseChangeSet>,
}

/// One ChangeSet as it travels the metadata plane: its canonical bytes, plus the fields a receiver
/// needs before it has decoded them.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CarriedChangeSet {
    /// The ChangeSet's record identifier.
    pub id: ChangeSetId,
    /// The actor that authored it.
    pub author: ActorId,
    /// Its position in that actor's sequence.
    pub sequence: ActorSequence,
    /// Its causal parents, sorted.
    pub parents: Vec<ChangeSetId>,
    /// The head its operations were computed against.
    pub base_head: HeadId,
    /// The head its operations produce. Never trusted from its author; re-derived on receipt.
    pub resulting_head: HeadId,
    /// The policy epoch it was authored under.
    pub policy_epoch: PolicyEpoch,
    /// The ChangeSet's `mesh-cbor/0` bytes, exactly as its author signed them.
    pub body: Vec<u8>,
}

/// A request for chunk bytes, resumable from an offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChunkRequest {
    /// The chunk wanted, named by the content hash its bytes must produce.
    pub content: ContentHash,
    /// The byte offset to resume from. Zero for a transfer that has not started.
    pub from_offset: u64,
    /// The largest number of bytes the requester will accept in one part.
    pub max_bytes: u64,
}

/// A part of one chunk's bytes.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChunkPart {
    /// The chunk these bytes belong to.
    pub content: ContentHash,
    /// Where these bytes start within the chunk.
    pub offset: u64,
    /// The bytes.
    pub bytes: Vec<u8>,
    /// Whether these bytes complete the chunk, so the receiver knows when to verify the hash.
    pub is_final: bool,
}

/// Whether an actor is currently working, present but idle, or gone.
///
/// Exactly one of `active`, `idle` and `away`. Presence is ephemeral and TTL-bounded: it never
/// enters canonical history, and losing it loses nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PresenceState {
    /// Reachable and producing work.
    Active,
    /// Reachable and not producing work.
    Idle,
    /// Not reachable. Sent so a peer learns of a departure rather than waiting out the timeout.
    Away,
}

impl PresenceState {
    /// The wire tag. Assigned once, never reused.
    #[must_use]
    pub const fn tag(self) -> u64 {
        match self {
            Self::Active => 1,
            Self::Idle => 2,
            Self::Away => 3,
        }
    }

    /// The state a wire tag names, or `None` for a tag this version does not define.
    #[must_use]
    pub const fn from_tag(tag: u64) -> Option<Self> {
        match tag {
            1 => Some(Self::Active),
            2 => Some(Self::Idle),
            3 => Some(Self::Away),
            _ => None,
        }
    }
}

/// One CWP message.
///
/// The set is plan §5.3's, and [`MessageKind`] carries the names and the plane table. Each variant
/// below states the fields; the preconditions are in [`SyncMessage::check`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncMessage {
    /// Opens a session.
    Hello {
        /// The protocol version the sender speaks.
        protocol_version: u32,
        /// The encoding profile the sender's record bodies are in, so a mismatch is a refusal
        /// rather than a signature that fails for no visible reason.
        encoding_profile: String,
        /// The actor the sender claims to speak for. A claim, not yet evidence: `AUTHENTICATE`
        /// is what turns it into evidence, and today nothing can produce that evidence.
        actor: ActorId,
        /// A random challenge the peer must sign to become verified.
        challenge: [u8; 32],
    },
    /// Offers a signature over the peer's challenge.
    Authenticate {
        /// The actor the signature is claimed to be from.
        actor: ActorId,
        /// The challenge that was signed, echoed so a signature can never be replayed against a
        /// different challenge without the mismatch being visible.
        challenge: [u8; 32],
        /// The signature bytes, checked by a [`crate::PeerAuthenticator`] and by nothing here.
        signature: Vec<u8>,
    },
    /// Offers this peer's actor heads.
    AdvertiseFrontier {
        /// One entry per actor the sender holds anything for, sorted by actor.
        heads: Vec<HeadAdvertisement>,
        /// The canonical head the sender holds, if it holds one.
        canonical_head: Option<HeadId>,
        /// The policy epoch the sender is operating under.
        policy_epoch: PolicyEpoch,
    },
    /// Asks for ChangeSets.
    RequestOperations {
        /// The actor whose sequence is wanted.
        actor: ActorId,
        /// The sequence the requester already holds contiguously; the answer starts after it.
        from_sequence: ActorSequence,
        /// Specific ChangeSets wanted regardless of sequence, sorted.
        specific: Vec<ChangeSetId>,
        /// The largest batch the requester will accept.
        max_count: u32,
    },
    /// Carries ChangeSets.
    OperationsBatch {
        /// The ChangeSets, in the order the sender believes they can be applied.
        changesets: Vec<CarriedChangeSet>,
    },
    /// Reports what the sender now holds of one actor's sequence.
    AckOperations {
        /// The actor being acknowledged.
        actor: ActorId,
        /// The highest sequence for which every preceding sequence is held.
        contiguous_through: ActorSequence,
        /// Held beyond the contiguous run, ascending by sequence.
        sparse: Vec<SparseChangeSet>,
    },
    /// Offers the file manifests the sender holds.
    AdvertiseManifests {
        /// The manifests, sorted.
        manifests: Vec<ManifestId>,
    },
    /// Asks for chunk bytes.
    RequestChunks {
        /// The requests, sorted by content hash.
        requests: Vec<ChunkRequest>,
    },
    /// Carries chunk bytes.
    ChunkBatch {
        /// The parts.
        parts: Vec<ChunkPart>,
    },
    /// Reports which chunks the sender has verified on receipt.
    AckChunks {
        /// The chunks whose bytes hashed to the content hash that named them, sorted.
        verified: Vec<ContentHash>,
    },
    /// Announces a new actor head.
    UpdateActorHead {
        /// The actor whose head advanced.
        actor: ActorId,
        /// The new head.
        head: HeadId,
        /// The sequence the head is derived through.
        sequence: ActorSequence,
    },
    /// Announces a new canonical head.
    UpdateCanonicalHead {
        /// The new canonical head.
        head: HeadId,
        /// The policy epoch the transition advanced under.
        policy_epoch: PolicyEpoch,
        /// The publication receipt's identifier.
        receipt: ApprovalId,
    },
    /// Announces that an actor is reachable, for a bounded time.
    Presence {
        /// The actor.
        actor: ActorId,
        /// Its state.
        state: PresenceState,
        /// How long this notice is good for. After it expires the receiver forgets, which is why
        /// presence loss costs nothing.
        expires_after_millis: u64,
    },
    /// Carries a review bundle.
    ReviewBundle {
        /// The bundle's identifier.
        id: ReviewBundleId,
        /// The bundle's `mesh-cbor/0` bytes.
        body: Vec<u8>,
    },
    /// Carries a validation receipt about one ChangeSet.
    ValidationReceipt {
        /// The ChangeSet the receipt is about.
        about: ChangeSetId,
        /// The receipt's `mesh-cbor/0` bytes.
        body: Vec<u8>,
    },
    /// Carries a signed approval envelope.
    ApprovalEnvelope {
        /// The envelope's identifier.
        id: ApprovalId,
        /// The envelope's `mesh-cbor/0` bytes.
        body: Vec<u8>,
    },
    /// Offers a summary of one actor's history.
    AntiEntropySummary {
        /// The actor summarized.
        actor: ActorId,
        /// The summary.
        summary: MerkleSummary,
    },
    /// Refuses a message.
    Error(ProtocolError),
}

impl SyncMessage {
    /// Which message this is.
    #[must_use]
    pub const fn kind(&self) -> MessageKind {
        match self {
            Self::Hello { .. } => MessageKind::Hello,
            Self::Authenticate { .. } => MessageKind::Authenticate,
            Self::AdvertiseFrontier { .. } => MessageKind::AdvertiseFrontier,
            Self::RequestOperations { .. } => MessageKind::RequestOperations,
            Self::OperationsBatch { .. } => MessageKind::OperationsBatch,
            Self::AckOperations { .. } => MessageKind::AckOperations,
            Self::AdvertiseManifests { .. } => MessageKind::AdvertiseManifests,
            Self::RequestChunks { .. } => MessageKind::RequestChunks,
            Self::ChunkBatch { .. } => MessageKind::ChunkBatch,
            Self::AckChunks { .. } => MessageKind::AckChunks,
            Self::UpdateActorHead { .. } => MessageKind::UpdateActorHead,
            Self::UpdateCanonicalHead { .. } => MessageKind::UpdateCanonicalHead,
            Self::Presence { .. } => MessageKind::Presence,
            Self::ReviewBundle { .. } => MessageKind::ReviewBundle,
            Self::ValidationReceipt { .. } => MessageKind::ValidationReceipt,
            Self::ApprovalEnvelope { .. } => MessageKind::ApprovalEnvelope,
            Self::AntiEntropySummary { .. } => MessageKind::AntiEntropySummary,
            Self::Error(_) => MessageKind::Error,
        }
    }

    /// Which plane carries it.
    #[must_use]
    pub const fn plane(&self) -> MessagePlane {
        self.kind().plane()
    }

    /// The preconditions this message can be judged against on its own.
    ///
    /// # Errors
    ///
    /// [`ProtocolError`] naming the code a receiver answers with. A sender that calls this before
    /// sending never makes the receiver do it, which is the point: the same function decides on
    /// both sides, so the two cannot disagree about what is well formed.
    pub fn check(&self) -> Result<(), ProtocolError> {
        match self {
            Self::Hello {
                protocol_version,
                encoding_profile,
                ..
            } => {
                if *protocol_version != PROTOCOL_VERSION {
                    return Err(self.refuse(
                        ErrorCode::UnsupportedVersion,
                        format!("this peer speaks version {PROTOCOL_VERSION}"),
                    ));
                }
                if encoding_profile != crate::RECORD_ENCODING_PROFILE {
                    return Err(self.refuse(
                        ErrorCode::UnsupportedVersion,
                        format!("record bodies must be {}", crate::RECORD_ENCODING_PROFILE),
                    ));
                }
                Ok(())
            }
            Self::Authenticate { signature, .. } => {
                if signature.is_empty() {
                    return Err(self.refuse(
                        ErrorCode::MalformedMessage,
                        "an empty signature is not a signature",
                    ));
                }
                Ok(())
            }
            Self::AdvertiseFrontier { heads, .. } => {
                if !is_sorted_by(heads, |entry| entry.actor) {
                    return Err(self.refuse(
                        ErrorCode::MalformedMessage,
                        "heads are sorted by actor, so two peers holding the same set send the \
                         same bytes",
                    ));
                }
                for entry in heads {
                    check_sparse(self, entry.contiguous_through, &entry.sparse)?;
                }
                Ok(())
            }
            Self::RequestOperations {
                specific,
                max_count,
                ..
            } => {
                if *max_count == 0 {
                    return Err(self.refuse(
                        ErrorCode::MalformedMessage,
                        "a request for zero ChangeSets is not a request",
                    ));
                }
                check_sorted_ids(self, specific)
            }
            Self::OperationsBatch { changesets } => {
                check_non_empty(self, changesets.is_empty(), "ChangeSets")?;
                if changesets.len() > MAX_OPERATIONS_PER_BATCH {
                    return Err(self.refuse(
                        ErrorCode::BatchTooLarge,
                        format!("at most {MAX_OPERATIONS_PER_BATCH} ChangeSets per batch"),
                    ));
                }
                for carried in changesets {
                    if carried.body.is_empty() {
                        return Err(self.refuse(
                            ErrorCode::MalformedMessage,
                            "a ChangeSet with no canonical bytes cannot be verified",
                        ));
                    }
                    if carried.sequence == ActorSequence::NONE {
                        return Err(self.refuse(
                            ErrorCode::MalformedMessage,
                            "sequence zero is the position before an actor's first ChangeSet",
                        ));
                    }
                    check_sorted_ids(self, &carried.parents)?;
                }
                Ok(())
            }
            Self::AckOperations {
                contiguous_through,
                sparse,
                ..
            } => check_sparse(self, *contiguous_through, sparse),
            Self::AdvertiseManifests { manifests } => {
                if !is_sorted(manifests) {
                    return Err(self.refuse(ErrorCode::MalformedMessage, "manifests are sorted"));
                }
                Ok(())
            }
            Self::RequestChunks { requests } => {
                check_non_empty(self, requests.is_empty(), "chunk requests")?;
                if !is_sorted_by(requests, |request| request.content) {
                    return Err(self.refuse(
                        ErrorCode::MalformedMessage,
                        "chunk requests are sorted by content hash",
                    ));
                }
                for request in requests {
                    if request.max_bytes == 0 {
                        return Err(self.refuse(
                            ErrorCode::MalformedMessage,
                            "a request for zero bytes is not a request",
                        ));
                    }
                }
                Ok(())
            }
            Self::ChunkBatch { parts } => {
                check_non_empty(self, parts.is_empty(), "chunk parts")?;
                for part in parts {
                    if part.bytes.is_empty() {
                        return Err(self.refuse(
                            ErrorCode::MalformedMessage,
                            "a part with no bytes moves no content",
                        ));
                    }
                    if part.bytes.len() > MAX_CHUNK_PART_BYTES {
                        return Err(self.refuse(
                            ErrorCode::BatchTooLarge,
                            format!("at most {MAX_CHUNK_PART_BYTES} bytes per part"),
                        ));
                    }
                    if part.offset.checked_add(part.bytes.len() as u64).is_none() {
                        return Err(self.refuse(
                            ErrorCode::OffsetPastEnd,
                            "the part ends past the largest representable offset",
                        ));
                    }
                }
                Ok(())
            }
            Self::AckChunks { verified } => {
                if !is_sorted(verified) {
                    return Err(
                        self.refuse(ErrorCode::MalformedMessage, "verified chunks are sorted")
                    );
                }
                Ok(())
            }
            Self::UpdateActorHead { sequence, .. } => {
                if *sequence == ActorSequence::NONE {
                    return Err(self.refuse(
                        ErrorCode::MalformedMessage,
                        "an actor with no ChangeSets has no head to announce",
                    ));
                }
                Ok(())
            }
            Self::Presence {
                expires_after_millis,
                ..
            } => {
                if *expires_after_millis == 0 {
                    return Err(self.refuse(
                        ErrorCode::MalformedMessage,
                        "presence that expires immediately says nothing",
                    ));
                }
                Ok(())
            }
            Self::ReviewBundle { body, .. }
            | Self::ValidationReceipt { body, .. }
            | Self::ApprovalEnvelope { body, .. } => {
                if body.is_empty() {
                    return Err(self.refuse(
                        ErrorCode::MalformedMessage,
                        "a record with no canonical bytes cannot be verified",
                    ));
                }
                Ok(())
            }
            Self::AntiEntropySummary { summary, .. } => {
                if summary.is_well_formed() {
                    Ok(())
                } else {
                    Err(self.refuse(
                        ErrorCode::MalformedMessage,
                        "summary nodes are contiguous, ascending and non-empty",
                    ))
                }
            }
            Self::UpdateCanonicalHead { .. } | Self::Error(_) => Ok(()),
        }
    }

    /// A refusal of this message.
    fn refuse(&self, code: ErrorCode, detail: impl Into<String>) -> ProtocolError {
        ProtocolError::new(code, self.kind(), detail)
    }
}

fn check_non_empty(message: &SyncMessage, empty: bool, what: &str) -> Result<(), ProtocolError> {
    if empty {
        return Err(message.refuse(
            ErrorCode::MalformedMessage,
            format!("a batch with no {what} is not progress"),
        ));
    }
    Ok(())
}

fn check_sorted_ids(message: &SyncMessage, ids: &[ChangeSetId]) -> Result<(), ProtocolError> {
    if is_sorted(ids) {
        Ok(())
    } else {
        Err(message.refuse(
            ErrorCode::MalformedMessage,
            "identifier lists are sorted and hold no duplicate, so one set has one encoding",
        ))
    }
}

/// The sparse set is ascending by sequence, and every entry really is beyond the contiguous run.
///
/// An entry at or below `contiguous_through` is not a hole, it is a contradiction: the sender says
/// in one field that it holds every sequence through *n* and in another that *m ≤ n* is an island.
/// Accepting it would let a peer advertise a state no store can be in.
fn check_sparse(
    message: &SyncMessage,
    contiguous_through: ActorSequence,
    sparse: &[SparseChangeSet],
) -> Result<(), ProtocolError> {
    if !is_sorted_by(sparse, |entry| entry.sequence) {
        return Err(message.refuse(
            ErrorCode::MalformedMessage,
            "the sparse set ascends by sequence and holds no duplicate, so one set has one \
             encoding",
        ));
    }
    if sparse
        .iter()
        .any(|entry| entry.sequence <= contiguous_through)
    {
        return Err(message.refuse(
            ErrorCode::SequenceGap,
            "a sparse entry at or below the contiguous sequence is not beyond it",
        ));
    }
    Ok(())
}

/// Strictly ascending: sorted, and holding no duplicate.
fn is_sorted<T: Ord>(values: &[T]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

/// Strictly ascending by a key.
fn is_sorted_by<T, K: Ord>(values: &[T], key: impl Fn(&T) -> K) -> bool {
    values.windows(2).all(|pair| key(&pair[0]) < key(&pair[1]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actor(byte: u8) -> ActorId {
        ActorId::from_bytes([byte; 32])
    }

    fn changeset(byte: u8) -> ChangeSetId {
        ChangeSetId::from_bytes([byte; 32])
    }

    fn carried(sequence: u64) -> CarriedChangeSet {
        CarriedChangeSet {
            id: changeset(1),
            author: actor(1),
            sequence: ActorSequence::new(sequence),
            parents: Vec::new(),
            base_head: HeadId::from_bytes([0; 32]),
            resulting_head: HeadId::from_bytes([9; 32]),
            policy_epoch: PolicyEpoch::INITIAL,
            body: vec![0x80],
        }
    }

    fn code_of(message: &SyncMessage) -> ErrorCode {
        message.check().expect_err("expected a refusal").code()
    }

    #[test]
    fn a_hello_at_this_version_and_profile_is_accepted() {
        let hello = SyncMessage::Hello {
            protocol_version: PROTOCOL_VERSION,
            encoding_profile: crate::RECORD_ENCODING_PROFILE.to_owned(),
            actor: actor(1),
            challenge: [7; 32],
        };
        assert_eq!(hello.check(), Ok(()));
        assert_eq!(hello.plane(), MessagePlane::Handshake);
    }

    #[test]
    fn a_hello_at_another_version_is_refused_rather_than_negotiated() {
        let hello = SyncMessage::Hello {
            protocol_version: PROTOCOL_VERSION + 1,
            encoding_profile: crate::RECORD_ENCODING_PROFILE.to_owned(),
            actor: actor(1),
            challenge: [7; 32],
        };
        assert_eq!(code_of(&hello), ErrorCode::UnsupportedVersion);
    }

    #[test]
    fn a_hello_naming_another_record_encoding_is_refused() {
        let hello = SyncMessage::Hello {
            protocol_version: PROTOCOL_VERSION,
            encoding_profile: "protobuf".to_owned(),
            actor: actor(1),
            challenge: [7; 32],
        };
        assert_eq!(code_of(&hello), ErrorCode::UnsupportedVersion);
    }

    #[test]
    fn an_empty_batch_is_refused() {
        let batch = SyncMessage::OperationsBatch {
            changesets: Vec::new(),
        };
        assert_eq!(code_of(&batch), ErrorCode::MalformedMessage);
    }

    #[test]
    fn an_oversized_batch_is_refused_by_count() {
        let batch = SyncMessage::OperationsBatch {
            changesets: (0..=MAX_OPERATIONS_PER_BATCH)
                .map(|index| carried(index as u64 + 1))
                .collect(),
        };
        assert_eq!(code_of(&batch), ErrorCode::BatchTooLarge);
    }

    #[test]
    fn a_changeset_at_sequence_zero_is_refused() {
        let batch = SyncMessage::OperationsBatch {
            changesets: vec![carried(0)],
        };
        assert_eq!(code_of(&batch), ErrorCode::MalformedMessage);
    }

    #[test]
    fn a_changeset_with_no_canonical_bytes_is_refused() {
        let mut one = carried(1);
        one.body.clear();
        let batch = SyncMessage::OperationsBatch {
            changesets: vec![one],
        };
        assert_eq!(code_of(&batch), ErrorCode::MalformedMessage);
    }

    #[test]
    fn an_unsorted_identifier_list_is_refused() {
        let mut one = carried(1);
        one.parents = vec![changeset(9), changeset(1)];
        let batch = SyncMessage::OperationsBatch {
            changesets: vec![one],
        };
        assert_eq!(code_of(&batch), ErrorCode::MalformedMessage);
    }

    #[test]
    fn a_duplicate_in_an_identifier_list_is_refused() {
        let request = SyncMessage::RequestOperations {
            actor: actor(1),
            from_sequence: ActorSequence::new(2),
            specific: vec![changeset(4), changeset(4)],
            max_count: 8,
        };
        assert_eq!(code_of(&request), ErrorCode::MalformedMessage);
    }

    #[test]
    fn a_sparse_set_that_descends_is_refused() {
        let ack = SyncMessage::AckOperations {
            actor: actor(1),
            contiguous_through: ActorSequence::new(2),
            sparse: vec![
                SparseChangeSet {
                    sequence: ActorSequence::new(9),
                    id: changeset(9),
                },
                SparseChangeSet {
                    sequence: ActorSequence::new(4),
                    id: changeset(4),
                },
            ],
        };
        assert_eq!(code_of(&ack), ErrorCode::MalformedMessage);
    }

    /// The contradiction test: "I hold everything through 5" and "6 is an island" cannot both be
    /// true of any store.
    #[test]
    fn a_sparse_entry_inside_the_contiguous_run_is_refused() {
        let ack = SyncMessage::AckOperations {
            actor: actor(1),
            contiguous_through: ActorSequence::new(5),
            sparse: vec![SparseChangeSet {
                sequence: ActorSequence::new(5),
                id: changeset(5),
            }],
        };
        assert_eq!(code_of(&ack), ErrorCode::SequenceGap);
    }

    #[test]
    fn unsorted_advertised_heads_are_refused() {
        let advertise = SyncMessage::AdvertiseFrontier {
            heads: vec![
                HeadAdvertisement {
                    actor: actor(9),
                    head: HeadId::from_bytes([1; 32]),
                    contiguous_through: ActorSequence::new(1),
                    sparse: Vec::new(),
                },
                HeadAdvertisement {
                    actor: actor(1),
                    head: HeadId::from_bytes([2; 32]),
                    contiguous_through: ActorSequence::new(1),
                    sparse: Vec::new(),
                },
            ],
            canonical_head: None,
            policy_epoch: PolicyEpoch::INITIAL,
        };
        assert_eq!(code_of(&advertise), ErrorCode::MalformedMessage);
    }

    #[test]
    fn an_oversized_chunk_part_is_refused() {
        let batch = SyncMessage::ChunkBatch {
            parts: vec![ChunkPart {
                content: ContentHash::from_bytes([1; 32]),
                offset: 0,
                bytes: vec![0; MAX_CHUNK_PART_BYTES + 1],
                is_final: true,
            }],
        };
        assert_eq!(code_of(&batch), ErrorCode::BatchTooLarge);
    }

    #[test]
    fn a_part_that_ends_past_the_representable_offset_is_refused() {
        let batch = SyncMessage::ChunkBatch {
            parts: vec![ChunkPart {
                content: ContentHash::from_bytes([1; 32]),
                offset: u64::MAX,
                bytes: vec![0; 8],
                is_final: true,
            }],
        };
        assert_eq!(code_of(&batch), ErrorCode::OffsetPastEnd);
    }

    #[test]
    fn presence_that_expires_immediately_is_refused() {
        let presence = SyncMessage::Presence {
            actor: actor(1),
            state: PresenceState::Active,
            expires_after_millis: 0,
        };
        assert_eq!(code_of(&presence), ErrorCode::MalformedMessage);
    }

    #[test]
    fn a_presence_state_tag_round_trips_and_stops_at_the_set() {
        for state in [
            PresenceState::Active,
            PresenceState::Idle,
            PresenceState::Away,
        ] {
            assert_eq!(PresenceState::from_tag(state.tag()), Some(state));
        }
        assert_eq!(PresenceState::from_tag(0), None);
        assert_eq!(PresenceState::from_tag(4), None);
    }

    #[test]
    fn a_refusal_names_the_message_it_is_about() {
        let request = SyncMessage::RequestOperations {
            actor: actor(1),
            from_sequence: ActorSequence::NONE,
            specific: Vec::new(),
            max_count: 0,
        };
        let refusal = request.check().expect_err("zero is refused");
        assert_eq!(refusal.about(), MessageKind::RequestOperations);
    }
}
