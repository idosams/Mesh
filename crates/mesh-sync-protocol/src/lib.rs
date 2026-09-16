//! The CWP wire message set and the per-peer knowledge-tracking model.
//!
//! # What this crate is, and what it deliberately is not
//!
//! It is the **definition** of how two Mesh peers talk: the eighteen messages of plan §5.3, the
//! fields and preconditions of each, the error cases, the metadata-plane and content-plane
//! separation, what one peer tracks about another, and the reconciliation arithmetic that turns
//! two knowledge sets into "what must I ask you for".
//!
//! It is **not the transport**. Nothing here opens a socket, retries, times out, persists an
//! outbox or schedules anything — that is `mesh-sync-engine`, which owns those resources. This
//! crate is pure: every function is a function of its arguments, which is what lets two actors be
//! driven to convergence in a test whose network is a [`std::vec::Vec`]
//! (`tests/two_actor_exchange.rs`).
//!
//! # Read this before believing anything about authentication
//!
//! **Replication is unauthenticated today.** Not by design — by state of the tree. `mesh-crypto`
//! verifies real Ed25519 signatures and holds no secret half; key custody is a platform backend
//! nobody has built, so **nothing in Mesh can produce a signature in production yet**. A peer
//! therefore cannot present evidence of who it is.
//!
//! What this crate does about that is refuse to hide it. [`PeerAuthenticator`] is the seam a real
//! verifier plugs into; [`NoAuthenticator`] is the only implementation shipped here and it answers
//! [`AuthenticationOutcome::Unverified`] for every peer, never `Verified`. A deployment that sets
//! [`AuthenticationPolicy::RequireVerifiedPeers`] therefore replicates **nothing** on the current
//! tree, which is the correct behaviour for a posture nothing can satisfy. The two-actor exchange
//! runs under [`AuthenticationPolicy::AdmitUnverifiedPeers`] and says so in its own module
//! documentation, so that test is never read as evidence of verified peers.
//!
//! # Records this crate does not own are carried opaque
//!
//! A ChangeSet, a review bundle, a validation receipt and an approval envelope travel as their
//! `mesh-cbor/0` bytes plus their record identifier — never as a second set of fields. `mesh-types`
//! owns those encodings and their published test vectors, and a second description of a signed
//! record is a description that will one day disagree with the bytes the signature covered. The
//! fields spelled out on [`CarriedChangeSet`] are the ones a receiver needs in order to plan its
//! next request, and every one of them is redundant with the body it arrives beside.
//!
//! # Where to start
//!
//! ```
//! use mesh_sync_protocol::{
//!     ActorId, ActorSequence, ChangeSetId, HeadId, KnowledgeSet, ReplicationGap, SyncMessage,
//!     decode_message, encode_message,
//! };
//!
//! // Ido holds three of her own ChangeSets; the agent holds the first one.
//! let ido = (1..=3u8).fold(KnowledgeSet::new(), |known, step| {
//!     known.with_changeset(
//!         ActorId::from_bytes([1; 32]),
//!         ActorSequence::new(u64::from(step)),
//!         ChangeSetId::from_bytes([step; 32]),
//!         HeadId::from_bytes([step; 32]),
//!     )
//! });
//! let agent = KnowledgeSet::new().with_changeset(
//!     ActorId::from_bytes([1; 32]),
//!     ActorSequence::new(1),
//!     ChangeSetId::from_bytes([1; 32]),
//!     HeadId::from_bytes([1; 32]),
//! );
//!
//! // The agent learns what Ido holds from one advertisement, and asks for the difference.
//! let advertisement = ido.advertisement();
//! let bytes = encode_message(&advertisement);
//! let believed = agent.observe(&decode_message(&bytes).unwrap());
//!
//! let gap = ReplicationGap::between(&agent, &believed);
//! let requests = gap.requests(64, 1024);
//! assert_eq!(requests.len(), 1);
//! match &requests[0] {
//!     SyncMessage::RequestOperations { from_sequence, .. } => {
//!         assert_eq!(*from_sequence, ActorSequence::new(1));
//!     }
//!     other => panic!("expected REQUEST_OPERATIONS, got {}", other.kind()),
//! }
//! ```

// Private modules with a flat re-export at the crate root, following `mesh-types`, `mesh-store`
// and `mesh-state`: `docs/protocol.md` §3.10 requires every public item to resolve to a register
// term, and a module path is a second name for the same item that no register row covers.
mod error;
mod gap;
mod ids;
mod knowledge;
mod message;
mod plane;
mod session;
mod summary;
mod wire;

pub use crate::error::{ErrorCode, ProtocolError, ERROR_CODES};
pub use crate::gap::{ActorGap, ReplicationGap};
pub use crate::ids::{
    ActorId, ActorSequence, ApprovalId, ChangeSetId, ContentHash, HeadId, IdError, ManifestId,
    PolicyEpoch, ReviewBundleId,
};
pub use crate::knowledge::{ActorKnowledge, KnowledgeSet};
pub use crate::message::{
    CarriedChangeSet, ChunkPart, ChunkRequest, HeadAdvertisement, PresenceState, SparseChangeSet,
    SyncMessage, MAX_CHUNK_PART_BYTES, MAX_OPERATIONS_PER_BATCH, PROTOCOL_VERSION,
};
pub use crate::plane::{MessageKind, MessagePlane, MESSAGE_KINDS};
pub use crate::session::{
    AuthenticationOutcome, AuthenticationPolicy, NoAuthenticator, PeerAuthenticator, Session,
};
pub use crate::summary::{MerkleSummary, SummaryDigest, SummaryNode, SUMMARY_DOMAIN};
pub use crate::wire::{decode_message, encode_message, WireError, WIRE_FORMAT};

/// The crate's name, so a placeholder still carries one verifiable behaviour.
pub const CRATE_NAME: &str = "mesh-sync-protocol";

/// The encoding profile every record body carried by a CWP message is in.
///
/// It is `mesh-types`' profile, named here as a string because this crate cannot depend on that
/// one. `tests/mesh_types_drift.rs` reads `mesh-types`' source and fails if the two ever stop
/// being the same word — which is the difference between a peer verifying a signature over the
/// bytes the author signed and a peer verifying it over bytes it re-encoded.
pub const RECORD_ENCODING_PROFILE: &str = "mesh-cbor/0";

/// This crate's own manifest, read at compile time by the guard below.
const MANIFEST: &str = include_str!("../Cargo.toml");

/// Whether the manifest declares no dependency of any kind.
///
/// Scans for every table whose name contains `dependencies` and requires every line inside it to
/// be blank or a comment. A dependency added anywhere in this manifest makes this `false`.
///
/// This is the fourth copy of this function in the workspace — `mesh-types`, `mesh-store` and
/// `mesh-state` each carry one. Factoring it out would need a shared crate, and depending on a
/// shared crate is the exact thing it exists to prevent, so the duplication is the cheaper of the
/// two costs and is named here rather than left to be discovered.
const fn manifest_declares_no_dependency(manifest: &str) -> bool {
    let bytes = manifest.as_bytes();
    let len = bytes.len();
    let mut index = 0;
    let mut inside_dependency_table = false;

    while index < len {
        let start = index;
        let mut end = index;
        while end < len && bytes[end] != b'\n' {
            end += 1;
        }

        if end > start && bytes[start] == b'[' {
            inside_dependency_table = range_contains(bytes, start, end, b"dependencies");
        } else if inside_dependency_table && !range_is_blank_or_comment(bytes, start, end) {
            return false;
        }

        index = end + 1;
    }

    true
}

/// Whether `bytes[start..end]` contains `needle`.
const fn range_contains(bytes: &[u8], start: usize, end: usize, needle: &[u8]) -> bool {
    let mut at = start;
    while at + needle.len() <= end {
        let mut offset = 0;
        while offset < needle.len() && bytes[at + offset] == needle[offset] {
            offset += 1;
        }
        if offset == needle.len() {
            return true;
        }
        at += 1;
    }
    false
}

/// Whether `bytes[start..end]` holds only whitespace, or begins a comment.
const fn range_is_blank_or_comment(bytes: &[u8], start: usize, end: usize) -> bool {
    let mut at = start;
    while at < end {
        match bytes[at] {
            b' ' | b'\t' | b'\r' => at += 1,
            other => return other == b'#',
        }
    }
    true
}

const _: () = assert!(
    manifest_declares_no_dependency(MANIFEST),
    "mesh-sync-protocol declares a dependency. Every dependency edge rewrites Cargo.lock, which \
     this repository's declaration gate treats as governance surface a lane escalates rather than \
     writes (ADR-0008, narrowed by ADR-0014 for audited cryptography and nothing else). If this \
     crate needs a type from mesh-types, mirror it in src/ids.rs and hold the mirror with \
     tests/mesh_types_drift.rs, or escalate for the edge."
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_stable() {
        assert_eq!(CRATE_NAME, "mesh-sync-protocol");
    }

    #[test]
    fn the_real_manifest_declares_no_dependency() {
        assert!(manifest_declares_no_dependency(MANIFEST));
    }

    /// The compile-time guard is only worth having if it can fail, and it cannot be observed
    /// failing without failing the build.
    #[test]
    fn the_guard_rejects_every_shape_of_dependency() {
        let rejected = [
            "[package]\nname = \"mesh-sync-protocol\"\n\n[dependencies]\nmesh-types = { path = \"../mesh-types\" }\n",
            "[dev-dependencies]\nproptest = \"1\"\n",
            "[build-dependencies]\ncc = \"1\"\n",
            "[target.'cfg(unix)'.dependencies]\nlibc = \"0.2\"\n",
            "[dependencies]\n# a comment\n\nprost = \"0.12\"\n",
        ];
        for manifest in rejected {
            assert!(
                !manifest_declares_no_dependency(manifest),
                "guard accepted a manifest with a dependency: {manifest:?}"
            );
        }
    }

    #[test]
    fn the_guard_accepts_an_empty_dependency_table() {
        let accepted = [
            "[package]\nname = \"mesh-sync-protocol\"\n\n[dependencies]\n",
            "[dependencies]\n\n# nothing here on purpose\n",
            "[dependencies]\n  \t\n",
            "[dependencies]\n\n[lints]\nworkspace = true\n",
        ];
        for manifest in accepted {
            assert!(
                manifest_declares_no_dependency(manifest),
                "guard rejected a clean manifest: {manifest:?}"
            );
        }
    }
}
