//! Deterministic materialization of an operation set into an exact workspace state.
//!
//! # One sentence this crate is built around
//!
//! **A workspace state is a pure function of the applied causal set, and of nothing else.** Not of
//! delivery order, not of how many times something arrived, not of a clock, and not of who is
//! asking. `docs/protocol.md` §2.1 SG-1 states it as an invariant; this crate is what makes it a
//! fact, and the table below is where each half of it is proved rather than asserted.
//!
//! # The four properties, and where each one is actually checked
//!
//! | Property | Where it is made true | Where it is proved |
//! |---|---|---|
//! | Equivalent operation sets in any valid order materialize to identical state | [`materialize`] never applies the slice it was handed — it applies [`causal_order`] of it, which is depth-then-identifier and a pure function of the set | `tests/order_insensitivity.rs` — every generated set is materialized in its own order and in a seeded shuffle, and the two canonical encodings must be equal |
//! | Materialization is total: no valid operation set panics or produces an undefined state | Every arm of [`apply_operation`] returns [`Effect`] or [`Rejection`]; the four non-state verbs are spelled out rather than caught by a wildcard, so a nineteenth verb is a compile error | `tests/totality.rs` — a corpus built to be hostile (unknown objects, taken names, cycles, self-parents, duplicate identifiers) with every operation required to produce an answer |
//! | Materializing the same set twice produces byte-identical state | [`WorkspaceState`] holds only ordered collections and [`WorkspaceState::canonical_bytes`] frames every value with its length | `tests/determinism.rs` — the encodings, not the digests, compared over the corpus |
//! | The reference oracle and the real implementation agree | — | `tests/differential.rs` — a naive oracle written to a different shape (linear scans, full rescans, no derived index) compared field by field over the generated corpus |
//!
//! # What this crate does not contain, and why
//!
//! **One vocabulary dependency.** `mesh-operations` owns the operation enum, all of its payloads,
//! names, metadata, and shared identifiers. This crate consumes and re-exports those exact types;
//! decoded canonical bytes therefore enter materialization without a conversion or parallel enum.
//! [`StateHash`] remains local because it names materialized state rather than an operation fact.
//!
//! **No digest implementation.** [`StateDigest`] is the seam; the protocol digest is BLAKE3 and the
//! composition root supplies it. The canonical encoding is the primary artifact and the hash is
//! defined over it, so the tests compare bytes rather than probabilities.
//!
//! **No content.** A file version names a [`ManifestId`]; no type here carries a byte of file
//! content, and no path in this crate reads one.
//!
//! **No head derivation.** Materialization *records* an actor head and the canonical head as
//! operations move them; deriving a head from a causal set is `mesh-state`'s
//! (`HeadAdvancement`), and the two use the same causal order so that a head and the state it names
//! can never be computed from two different sequences.
//!
//! **No Unicode normalization form.** `src/name.rs` states what is not pinned and what that costs.
//!
//! # The other surface: the filesystem seam
//!
//! [`WorkspaceAdapter`] and [`WorkspaceView`] are the contract every filesystem backend plugs into
//! — FUSE, FSKit and the directory fallback are three implementations of one trait, graded by one
//! suite. The trait lives here, in `core`, and the backends live in `adapter` crates, because
//! adapters call the core and the core never calls them; nothing in `src/adapter.rs` reaches back
//! the other way, and none of it carries a byte of file content either — `read` and `write` borrow
//! a caller's buffer and answer a count. Design `01KZEZGDPMZ5RH7E60WDYDYYEE` is the contract;
//! `tests/compatibility/adapter/v0/` is the published half a backend author reads instead of this
//! crate's internals.
//!
//! # Where to start
//!
//! ```
//! use mesh_materializer::{
//!     causal_order, materialize, AppliedChangeSet, ChangeSetId, ManifestId, NormalizedName,
//!     ObjectId, Operation, PortableMetadata, VersionId,
//! };
//!
//! let root = ObjectId::from_bytes([0; 16]);
//! let notes = ObjectId::from_bytes([1; 16]);
//! let version = VersionId::from_bytes([2; 32]);
//!
//! let first = AppliedChangeSet::genesis(
//!     ChangeSetId::from_bytes([0xa1; 32]),
//!     vec![
//!         Operation::CreateFile { object_id: notes },
//!         Operation::WriteFileVersion {
//!             object_id: notes,
//!             version_id: version,
//!             parent_versions: vec![],
//!             manifest_id: ManifestId::from_bytes([3; 32]),
//!             portable_metadata: PortableMetadata::default(),
//!         },
//!     ],
//! );
//! let second = AppliedChangeSet::new(
//!     ChangeSetId::from_bytes([0x02; 32]),
//!     vec![first.id()],
//!     vec![Operation::LinkDirectoryEntry {
//!         directory_id: root,
//!         name: NormalizedName::new("notes.md").unwrap(),
//!         object_id: notes,
//!         version_id: version,
//!     }],
//! );
//!
//! // Delivered in one order.
//! let one = materialize(root, &[first.clone(), second.clone()]);
//! // Delivered in the other. The causal order is a function of the set, so the state is too.
//! let other = materialize(root, &[second.clone(), first.clone()]);
//!
//! assert_eq!(one.state().canonical_bytes(), other.state().canonical_bytes());
//! assert_eq!(causal_order(&[second, first.clone()])[0], first.id());
//! assert_eq!(
//!     one.state().path_of(notes),
//!     Some(vec![NormalizedName::new("notes.md").unwrap()])
//! );
//! ```

// Private modules with a flat re-export at the crate root, following `mesh-types`, `mesh-state` and
// `mesh-operations`: `docs/protocol.md` §3.10 requires every public item to resolve to a register
// term, and a module path is a second name for the same item that no register row covers.
mod adapter;
mod apply;
mod conformance;
mod encode;
mod ids;
mod name;
mod operation;
mod order;
mod rejection;
mod restore;
mod state;
mod version;

pub use crate::adapter::{
    AdapterCapability, AdapterDescription, AdapterError, AdapterFixture, BoundaryObservationV1,
    BoundaryReason, BoundaryReasonV1, CapabilitySet, CheckpointCandidate, CheckpointCandidateV1,
    DestinationBefore, DestinationBindingOutcome, EventSequence, FsEvent, FsEventKind,
    MaterializedView, MountedView, MovedObjectIdentity, OpenHandle, OpenMode, RenameBinding,
    RenameBindingEvidence, RenameDisposition, RenameEvidence, RenameEvidenceField,
    RenameEvidenceUnavailable, RenameIdentity, ViewAccess, ViewEntry, ViewId, WorkspaceAdapter,
    WorkspaceView, WORKSPACE_ADAPTER_CONTRACT, WORKSPACE_ADAPTER_CONTRACT_V1,
};
pub use crate::apply::{
    apply_operation, materialize, verbs_outside_the_state_graph, Materialization,
};
pub use crate::conformance::{
    conformance_catalogue, run_conformance, run_conformance_v1, CaseFamily, CaseResult,
    ConformanceCase, ConformanceReport, ConformanceRule,
};
pub use crate::encode::{StateDigest, STATE_DOMAIN};
pub use crate::ids::StateHash;
pub use crate::order::{causal_order, AppliedChangeSet};
pub use crate::rejection::{Effect, RejectedOperation, Rejection};
pub use crate::restore::{plan_file_restore, FileRestorePlan, RestoreRefusal};
pub use crate::state::{CanonicalAdvance, WorkspaceState};
pub use crate::version::{DirectoryEntry, DirectoryVersion, FileVersion, ObjectKind, ObjectRecord};
pub use mesh_operations::{
    ActorId, ApprovalId, AttributionConfidence, ChangeSetId, ContentHash, DerivationId,
    DerivationKind, HeadId, IdError, ManifestId, NameError, NormalizedName, ObjectId, Operation,
    OperationKind, PortableMetadata, PreservedEntry, ReadRegion, ReviewBundleId, SessionId,
    ValidationOutcome, VersionId, WorkspaceId,
};

/// The crate's name, so every crate in this workspace carries one verifiable constant.
pub const CRATE_NAME: &str = "mesh-materializer";

/// This crate's manifest, used to pin the owner dependency in a unit test.
#[cfg(test)]
const MANIFEST: &str = include_str!("../Cargo.toml");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_stable() {
        assert_eq!(CRATE_NAME, "mesh-materializer");
    }

    #[test]
    fn the_manifest_names_the_operation_owner() {
        assert!(MANIFEST
            .contains("mesh-operations = { path = \"../mesh-operations\", version = \"=0.0.0\" }"));
    }
}
