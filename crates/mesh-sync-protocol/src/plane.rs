//! The message names, and the plane each one travels on.
//!
//! The plane split is the whole point of plan §5.2: a peer must be able to learn that an actor
//! changed a file before the bytes of that file arrive. That is only true if the two kinds of
//! message are separable — separable in the type system, so a transport cannot accidentally put a
//! chunk batch on the same queue as a head update and make visibility wait on content.
//!
//! [`MessageKind`] is the name alone. It exists apart from [`crate::SyncMessage`] so that the
//! plane table, the wire tag and the error's "about which message" field can all be stated without
//! constructing a payload.

use core::fmt;

/// Which plane carries a message.
///
/// Exactly one of `handshake`, `metadata` and `content`. The plan names two planes; the third
/// value is the session-establishment traffic that belongs to neither, and naming it is what stops
/// `HELLO` from being filed under `metadata` and quietly making the metadata plane's latency claim
/// include a handshake.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MessagePlane {
    /// Session establishment: who is speaking, under which protocol version, and what went wrong.
    Handshake,
    /// Everything except chunk bytes — ChangeSets, heads, manifests, availability, approvals.
    Metadata,
    /// Chunk bytes, their resumable offsets and their integrity information.
    Content,
}

impl MessagePlane {
    /// The wire name of the plane.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Handshake => "handshake",
            Self::Metadata => "metadata",
            Self::Content => "content",
        }
    }
}

impl fmt::Display for MessagePlane {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The name of one CWP message, without its fields.
///
/// The eighteen names are plan §5.3's set, unchanged and in its order. A name is protocol: its
/// wire tag is assigned once by [`MessageKind::tag`] and is never reused for a different message,
/// exactly as a protobuf field number is never renumbered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MessageKind {
    /// Opens a session and states the protocol version and encoding profile.
    Hello,
    /// Offers a signature over a challenge, binding an actor identity to the session.
    Authenticate,
    /// Offers this peer's actor heads and how much of each actor's sequence it holds.
    AdvertiseFrontier,
    /// Asks for ChangeSets an advertisement showed to be missing.
    RequestOperations,
    /// Carries ChangeSets, each as its canonical bytes plus the fields delivery needs.
    OperationsBatch,
    /// Reports how much of an actor's sequence the sender now holds.
    AckOperations,
    /// Offers the file manifests this peer holds.
    AdvertiseManifests,
    /// Asks for chunk bytes, each from a resumable offset.
    RequestChunks,
    /// Carries chunk bytes.
    ChunkBatch,
    /// Reports which chunks the sender has verified on receipt.
    AckChunks,
    /// Announces a new actor head for one actor.
    UpdateActorHead,
    /// Announces a new canonical head and the policy epoch it advanced under.
    UpdateCanonicalHead,
    /// Announces that an actor is currently reachable, for a bounded time.
    Presence,
    /// Carries a review bundle.
    ReviewBundle,
    /// Carries a validation receipt about one ChangeSet.
    ValidationReceipt,
    /// Carries a signed approval envelope.
    ApprovalEnvelope,
    /// Offers a summary of an actor's history so two peers can find where they diverge.
    AntiEntropySummary,
    /// Refuses a message, naming which one and why.
    Error,
}

/// Every message name, in plan §5.3's order.
///
/// Public so that a conformance harness can enumerate the set rather than hard-coding it, and so
/// that adding a nineteenth message to [`MessageKind`] without adding it here is a test failure
/// rather than a silent omission.
pub const MESSAGE_KINDS: [MessageKind; 18] = [
    MessageKind::Hello,
    MessageKind::Authenticate,
    MessageKind::AdvertiseFrontier,
    MessageKind::RequestOperations,
    MessageKind::OperationsBatch,
    MessageKind::AckOperations,
    MessageKind::AdvertiseManifests,
    MessageKind::RequestChunks,
    MessageKind::ChunkBatch,
    MessageKind::AckChunks,
    MessageKind::UpdateActorHead,
    MessageKind::UpdateCanonicalHead,
    MessageKind::Presence,
    MessageKind::ReviewBundle,
    MessageKind::ValidationReceipt,
    MessageKind::ApprovalEnvelope,
    MessageKind::AntiEntropySummary,
    MessageKind::Error,
];

impl MessageKind {
    /// The wire tag. Assigned once, never reused, never renumbered.
    #[must_use]
    pub const fn tag(self) -> u64 {
        match self {
            Self::Hello => 1,
            Self::Authenticate => 2,
            Self::AdvertiseFrontier => 3,
            Self::RequestOperations => 4,
            Self::OperationsBatch => 5,
            Self::AckOperations => 6,
            Self::AdvertiseManifests => 7,
            Self::RequestChunks => 8,
            Self::ChunkBatch => 9,
            Self::AckChunks => 10,
            Self::UpdateActorHead => 11,
            Self::UpdateCanonicalHead => 12,
            Self::Presence => 13,
            Self::ReviewBundle => 14,
            Self::ValidationReceipt => 15,
            Self::ApprovalEnvelope => 16,
            Self::AntiEntropySummary => 17,
            Self::Error => 18,
        }
    }

    /// The message a wire tag names, or `None` for a tag this version does not define.
    ///
    /// A peer that receives an undefined tag has met a newer protocol version and must say so with
    /// [`crate::ErrorCode::UnknownMessage`] rather than guess.
    #[must_use]
    pub fn from_tag(tag: u64) -> Option<Self> {
        MESSAGE_KINDS.into_iter().find(|kind| kind.tag() == tag)
    }

    /// The screaming-snake-case name plan §5.3 uses.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hello => "HELLO",
            Self::Authenticate => "AUTHENTICATE",
            Self::AdvertiseFrontier => "ADVERTISE_FRONTIER",
            Self::RequestOperations => "REQUEST_OPERATIONS",
            Self::OperationsBatch => "OPERATIONS_BATCH",
            Self::AckOperations => "ACK_OPERATIONS",
            Self::AdvertiseManifests => "ADVERTISE_MANIFESTS",
            Self::RequestChunks => "REQUEST_CHUNKS",
            Self::ChunkBatch => "CHUNK_BATCH",
            Self::AckChunks => "ACK_CHUNKS",
            Self::UpdateActorHead => "UPDATE_ACTOR_HEAD",
            Self::UpdateCanonicalHead => "UPDATE_CANONICAL_HEAD",
            Self::Presence => "PRESENCE",
            Self::ReviewBundle => "REVIEW_BUNDLE",
            Self::ValidationReceipt => "VALIDATION_RECEIPT",
            Self::ApprovalEnvelope => "APPROVAL_ENVELOPE",
            Self::AntiEntropySummary => "ANTI_ENTROPY_SUMMARY",
            Self::Error => "ERROR",
        }
    }

    /// Which plane carries it.
    ///
    /// The table, in one place. `ADVERTISE_MANIFESTS` is metadata and not content on purpose: it
    /// says which bytes *exist*, which is exactly the knowledge a peer needs before any byte moves.
    #[must_use]
    pub const fn plane(self) -> MessagePlane {
        match self {
            Self::Hello | Self::Authenticate | Self::Error => MessagePlane::Handshake,
            Self::RequestChunks | Self::ChunkBatch | Self::AckChunks => MessagePlane::Content,
            Self::AdvertiseFrontier
            | Self::RequestOperations
            | Self::OperationsBatch
            | Self::AckOperations
            | Self::AdvertiseManifests
            | Self::UpdateActorHead
            | Self::UpdateCanonicalHead
            | Self::Presence
            | Self::ReviewBundle
            | Self::ValidationReceipt
            | Self::ApprovalEnvelope
            | Self::AntiEntropySummary => MessagePlane::Metadata,
        }
    }

    /// Whether a session must be authenticated before this message may be sent.
    ///
    /// `HELLO`, `AUTHENTICATE` and `ERROR` are how a session becomes authenticated or fails to, so
    /// they are exempt by construction. Everything else carries or requests workspace state.
    #[must_use]
    pub const fn needs_established_session(self) -> bool {
        !matches!(self, Self::Hello | Self::Authenticate | Self::Error)
    }
}

impl fmt::Display for MessageKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn the_set_is_the_eighteen_the_plan_names() {
        let names: BTreeSet<&str> = MESSAGE_KINDS.iter().map(|kind| kind.as_str()).collect();
        assert_eq!(names.len(), 18);
        for expected in [
            "HELLO",
            "AUTHENTICATE",
            "ADVERTISE_FRONTIER",
            "REQUEST_OPERATIONS",
            "OPERATIONS_BATCH",
            "ACK_OPERATIONS",
            "ADVERTISE_MANIFESTS",
            "REQUEST_CHUNKS",
            "CHUNK_BATCH",
            "ACK_CHUNKS",
            "UPDATE_ACTOR_HEAD",
            "UPDATE_CANONICAL_HEAD",
            "PRESENCE",
            "REVIEW_BUNDLE",
            "VALIDATION_RECEIPT",
            "APPROVAL_ENVELOPE",
            "ANTI_ENTROPY_SUMMARY",
            "ERROR",
        ] {
            assert!(names.contains(expected), "plan §5.3 names {expected}");
        }
    }

    #[test]
    fn every_tag_is_distinct_and_round_trips() {
        let tags: BTreeSet<u64> = MESSAGE_KINDS.iter().map(|kind| kind.tag()).collect();
        assert_eq!(tags.len(), MESSAGE_KINDS.len());
        for kind in MESSAGE_KINDS {
            assert_eq!(MessageKind::from_tag(kind.tag()), Some(kind));
        }
    }

    #[test]
    fn an_undefined_tag_resolves_to_nothing() {
        assert_eq!(MessageKind::from_tag(0), None);
        assert_eq!(MessageKind::from_tag(19), None);
        assert_eq!(MessageKind::from_tag(u64::MAX), None);
    }

    /// The separation claim, as a test: no message carries chunk bytes on the metadata plane, and
    /// the content plane carries nothing else.
    #[test]
    fn only_the_three_chunk_messages_are_on_the_content_plane() {
        let content: BTreeSet<&str> = MESSAGE_KINDS
            .iter()
            .filter(|kind| kind.plane() == MessagePlane::Content)
            .map(|kind| kind.as_str())
            .collect();
        assert_eq!(
            content,
            BTreeSet::from(["REQUEST_CHUNKS", "CHUNK_BATCH", "ACK_CHUNKS"])
        );
    }

    #[test]
    fn the_handshake_plane_is_exactly_the_three_messages_that_establish_a_session() {
        for kind in MESSAGE_KINDS {
            assert_eq!(
                kind.plane() == MessagePlane::Handshake,
                !kind.needs_established_session(),
                "{kind} disagrees about whether it establishes a session"
            );
        }
    }

    #[test]
    fn a_plane_prints_its_wire_name() {
        assert_eq!(MessagePlane::Metadata.to_string(), "metadata");
        assert_eq!(MessageKind::ChunkBatch.to_string(), "CHUNK_BATCH");
    }
}
