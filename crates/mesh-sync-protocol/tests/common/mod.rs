//! One instance of every CWP message, shared by the integration tests.
//!
//! The corpus is deliberately *one* list: `protocol-vectors.rs` pins its bytes and
//! `two_actor_exchange.rs` reuses the same constructors, so a message whose shape changes cannot
//! stay pinned in one test and drift in the other.

// Each test binary compiles this module separately and uses a different part of it, so anything
// the *other* binary uses reads as dead here. The same allowance is in every other crate's test
// support module in this workspace, for the same reason.
#![allow(dead_code)]

use mesh_sync_protocol::{
    ActorId, ActorSequence, ApprovalId, CarriedChangeSet, ChangeSetId, ChunkPart, ChunkRequest,
    ContentHash, ErrorCode, HeadAdvertisement, HeadId, ManifestId, MerkleSummary, MessageKind,
    PolicyEpoch, PresenceState, ProtocolError, ReviewBundleId, SparseChangeSet, SummaryDigest,
    SummaryNode, SyncMessage, PROTOCOL_VERSION, RECORD_ENCODING_PROFILE,
};

/// A digest for the tests alone: FNV-1a widened to thirty-two bytes.
///
/// It is not a protocol digest and must never be one. [`SummaryDigest`] is a seam precisely so
/// that the composition root supplies BLAKE3; a test needs only that identical inputs agree and
/// different ones do not.
pub struct TestDigest(u128);

impl SummaryDigest for TestDigest {
    fn start() -> Self {
        Self(0x6c62_272e_07bb_0142_62b8_2175_6295_c58d)
    }

    fn absorb(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u128::from(*byte)).wrapping_mul(0x0100_0000_0000_0000_0000_013b);
        }
    }

    fn finish(self) -> [u8; 32] {
        let mut out = [0u8; 32];
        out[..16].copy_from_slice(&self.0.to_be_bytes());
        out[16..].copy_from_slice(&self.0.rotate_left(37).to_be_bytes());
        out
    }
}

/// An actor identifier whose bytes are all `byte`.
#[must_use]
pub fn actor(byte: u8) -> ActorId {
    ActorId::from_bytes([byte; 32])
}

/// A ChangeSet identifier whose bytes are all `byte`.
#[must_use]
pub fn changeset(byte: u8) -> ChangeSetId {
    ChangeSetId::from_bytes([byte; 32])
}

/// A head identifier whose bytes are all `byte`.
#[must_use]
pub fn head(byte: u8) -> HeadId {
    HeadId::from_bytes([byte; 32])
}

/// A content hash whose bytes are all `byte`.
#[must_use]
pub fn chunk(byte: u8) -> ContentHash {
    ContentHash::from_bytes([byte; 32])
}

/// One ChangeSet as it travels, authored by `author` at `sequence`.
#[must_use]
pub fn carried(
    author: ActorId,
    sequence: u64,
    id: ChangeSetId,
    resulting: HeadId,
) -> CarriedChangeSet {
    CarriedChangeSet {
        id,
        author,
        sequence: ActorSequence::new(sequence),
        parents: if sequence > 1 {
            vec![changeset(u8::try_from(sequence - 1).unwrap_or(0))]
        } else {
            Vec::new()
        },
        base_head: head(u8::try_from(sequence.saturating_sub(1)).unwrap_or(0)),
        resulting_head: resulting,
        policy_epoch: PolicyEpoch::INITIAL,
        body: vec![0x83, 0x01, 0x02, 0x03],
    }
}

/// One instance of every message in plan §5.3's order, each with fields set to values that
/// exercise the encoding rather than to defaults.
#[must_use]
pub fn every_message() -> Vec<SyncMessage> {
    vec![
        SyncMessage::Hello {
            protocol_version: PROTOCOL_VERSION,
            encoding_profile: RECORD_ENCODING_PROFILE.to_owned(),
            actor: actor(0x11),
            challenge: [0x22; 32],
        },
        SyncMessage::Authenticate {
            actor: actor(0x11),
            challenge: [0x22; 32],
            signature: vec![0x33; 64],
        },
        SyncMessage::AdvertiseFrontier {
            heads: vec![
                HeadAdvertisement {
                    actor: actor(0x11),
                    head: head(0x44),
                    contiguous_through: ActorSequence::new(3),
                    sparse: vec![SparseChangeSet {
                        sequence: ActorSequence::new(9),
                        id: changeset(0x55),
                    }],
                },
                HeadAdvertisement {
                    actor: actor(0x66),
                    head: head(0x77),
                    contiguous_through: ActorSequence::new(1),
                    sparse: Vec::new(),
                },
            ],
            canonical_head: Some(head(0x88)),
            policy_epoch: PolicyEpoch::new(2),
        },
        SyncMessage::RequestOperations {
            actor: actor(0x11),
            from_sequence: ActorSequence::new(3),
            specific: vec![changeset(0x55), changeset(0x99)],
            max_count: 64,
        },
        SyncMessage::OperationsBatch {
            changesets: vec![
                carried(actor(0x11), 4, changeset(0x04), head(0x04)),
                carried(actor(0x11), 5, changeset(0x05), head(0x05)),
            ],
        },
        SyncMessage::AckOperations {
            actor: actor(0x11),
            contiguous_through: ActorSequence::new(5),
            sparse: vec![SparseChangeSet {
                sequence: ActorSequence::new(9),
                id: changeset(0x55),
            }],
        },
        SyncMessage::AdvertiseManifests {
            manifests: vec![ManifestId::from_bytes([0xaa; 32])],
        },
        SyncMessage::RequestChunks {
            requests: vec![ChunkRequest {
                content: chunk(0xbb),
                from_offset: 4096,
                max_bytes: 65_536,
            }],
        },
        SyncMessage::ChunkBatch {
            parts: vec![ChunkPart {
                content: chunk(0xbb),
                offset: 4096,
                bytes: vec![0xcc; 8],
                is_final: true,
            }],
        },
        SyncMessage::AckChunks {
            verified: vec![chunk(0xbb)],
        },
        SyncMessage::UpdateActorHead {
            actor: actor(0x11),
            head: head(0x44),
            sequence: ActorSequence::new(5),
        },
        SyncMessage::UpdateCanonicalHead {
            head: head(0x88),
            policy_epoch: PolicyEpoch::new(2),
            receipt: ApprovalId::from_bytes([0xdd; 32]),
        },
        SyncMessage::Presence {
            actor: actor(0x11),
            state: PresenceState::Active,
            expires_after_millis: 30_000,
        },
        SyncMessage::ReviewBundle {
            id: ReviewBundleId::from_bytes([0xee; 32]),
            body: vec![0x81, 0x00],
        },
        SyncMessage::ValidationReceipt {
            about: changeset(0x04),
            body: vec![0x81, 0x01],
        },
        SyncMessage::ApprovalEnvelope {
            id: ApprovalId::from_bytes([0xdd; 32]),
            body: vec![0x81, 0x02],
        },
        SyncMessage::AntiEntropySummary {
            actor: actor(0x11),
            summary: MerkleSummary::of::<TestDigest>(
                ActorSequence::new(1),
                &[changeset(1), changeset(2), changeset(3), changeset(4)],
                2,
            ),
        },
        SyncMessage::Error(ProtocolError::new(
            ErrorCode::SequenceGap,
            MessageKind::RequestOperations,
            "asked from 9 while holding through 5",
        )),
    ]
}

/// A summary node, so a test can build one without reaching for the digest.
#[must_use]
pub fn summary_node(first: u64, last: u64, digest: u8) -> SummaryNode {
    SummaryNode::new(
        ActorSequence::new(first),
        ActorSequence::new(last),
        [digest; 32],
    )
}

/// Lowercase hex of some bytes.
#[must_use]
pub fn to_hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}
