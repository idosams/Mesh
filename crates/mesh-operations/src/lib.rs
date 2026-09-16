//! The operation vocabulary and ChangeSet construction. No platform-adapter dependency.
//!
//! # Three sentences this crate is built around
//!
//! **An operation is a meaningful transition, not a syscall.** Plan §4.3's eighteen verbs are the
//! whole vocabulary, and a raw `write()` is not one of them: raw writes are accumulated locally by
//! [`WriteCoalescer`] and replaced by one [`Operation::WriteFileVersion`] naming content that is
//! already content-addressed. No type here carries file bytes.
//!
//! **A ChangeSet cannot exist without its causal context.** [`ChangeSetDraft`] carries the causal
//! parents, the base head and the policy epoch as type parameters, each starting as the zero-sized
//! [`Unset`], and [`ChangeSetDraft::seal`] exists only on the draft that has all three. The
//! negative case is a compile error with its own `compile_fail` test.
//!
//! **A resulting head is derived, never supplied.** [`ChangeSetDraft::seal`] has no resulting-head
//! parameter at all; it asks a [`HeadDerivation`] about a [`TransitionCommitment`] over the
//! operations and the base head. A record arriving from a peer carries a head its author claims, so
//! that path is [`ReceivedChangeSet`], and the only way out of it is
//! [`ReceivedChangeSet::verify`].
//!
//! # Moving a directory costs one operation
//!
//! [`Operation::MoveEntry`] names the entry, never a descendant. Plan §11's published budget — a
//! million descendants moved in under 100 ms, under 10 KiB of metadata for a subtree move — is
//! therefore a property of the vocabulary's shape rather than of an optimization somebody has to
//! remember to write. `tests/subtree_move_is_constant_cost.rs` measures it at four subtree sizes,
//! up to a million nodes, and checks that every descendant's path really moved.
//!
//! # This crate has no dependency at all, and that is checked at compile time
//!
//! `[dependencies]` in `crates/mesh-operations/Cargo.toml` is empty, and the `const _: () =
//! assert!(…)` below reads the manifest with [`include_str`] and fails the build if it ever stops
//! being. `src/no_ambient_io.rs` adds the complementary scan over this crate's own source, because
//! the standard library supplies a filesystem and a network without any dependency at all. It is a
//! `const` assertion, the same shape `mesh-types` and `mesh-state` carry, so adding a filesystem
//! import to any source here fails `cargo build` rather than a later test run. It spent one PR as
//! `tests/no_ambient_io.rs` because a `src/` file that spells the module paths it searches for
//! needs an `ambientScanExempt` entry in `tools/program/arch-check/architecture.json`, which the
//! task that wrote this crate could not write; that entry now exists
//! (`01KZECET9ADD6JGYYGB8THBTZC`).
//!
//! Not importing `mesh-types` is a **fence, not a preference**, and it was measured rather than
//! assumed: adding `mesh-types = { path = "../mesh-types" }` here appends three lines to
//! `Cargo.lock`, and `tools/program/contract/declaration-gate.mjs` reports `forbidden-path-write`
//! for that file whatever the PR body says. `src/ids.rs` states what that costs and
//! `tests/mesh_types_drift.rs` is what keeps the cost from becoming a silent divergence.
//!
//! The mirror is **not** a second encoding, though. `mesh-cbor/0` is reimplemented here and then
//! held against `protocol/test-vectors/v0/`, which `mesh-types` produced: two independent
//! implementations agreeing on published bytes is the evidence those vectors exist to provide, and
//! it is stronger than one implementation agreeing with itself.
//!
//! # Where to start
//!
//! ```
//! use mesh_operations::{
//!     decode_operation, encode_canonical, one_of_every_operation, NormalizedName, ObjectId,
//!     Operation, OperationKind,
//! };
//!
//! // Eighteen verbs, each with a published schema and a domain tag.
//! assert_eq!(OperationKind::ALL.len(), 18);
//! assert_eq!(OperationKind::MoveEntry.domain(), "mesh.v0.op.move-entry");
//!
//! // Every one of them round-trips through the canonical encoding.
//! for operation in one_of_every_operation() {
//!     assert_eq!(decode_operation(&encode_canonical(&operation)).unwrap(), operation);
//! }
//!
//! // Moving a subtree is one operation, whatever is under it.
//! let move_ops = Operation::move_subtree(
//!     ObjectId::from_bytes([1; 16]),
//!     NormalizedName::new("src").unwrap(),
//!     ObjectId::from_bytes([2; 16]),
//!     NormalizedName::new("source").unwrap(),
//!     ObjectId::from_bytes([3; 16]),
//! );
//! assert_eq!(move_ops.len(), 1);
//! ```

// The modules are private and every public item is re-exported at the crate root, following
// `mesh-types`, `mesh-store` and `mesh-state`: `docs/protocol.md` §3.10 requires every public item
// to resolve to a register term, and a module path is a second name for the same item that no
// register row covers.
mod canonical;
mod cbor;
mod cbor_reader;
mod changeset;
mod coalesce;
mod context;
mod corpus;
mod encoding;
mod head;
mod ids;
mod name;
mod no_ambient_io;
mod operation;
mod sequence;

pub use crate::canonical::{
    decode_canonical, encode_canonical, peek_domain, schema_violations, CanonicalEncode,
    CanonicalType, CanonicalValue, DecodeError, FieldSchema, RecordSchema, SCHEMA_FORMAT,
};
pub use crate::cbor::{CborWriter, CBOR_PROFILE};
pub use crate::cbor_reader::{CborError, CborReader, MAX_NESTING};
pub use crate::changeset::{
    ChangeSet, ChangeSetDraft, HeadRefused, ReceivedChangeSet, Unset, CHANGESET_DOMAIN,
    CHANGESET_SCHEMA,
};
pub use crate::coalesce::{CheckpointTrigger, NotCheckpointed, RawWrite, WriteCoalescer};
pub use crate::context::{CausalParents, Hlc, PolicyEpoch, Signature};
pub use crate::corpus::{
    corpus_covers_the_vocabulary, one_of_every_operation, variable_length_operations,
};
pub use crate::encoding::{
    decode_operation, encode_operations, operation_schemas, OPERATION_DOMAIN_PREFIX,
};
pub use crate::head::{HeadDerivation, TransitionCommitment, TRANSITION_DOMAIN, TRANSITION_SCHEMA};
pub use crate::ids::{
    ActorId, ApprovalId, ChangeSetId, ContentHash, DerivationId, HeadId, IdError, ManifestId,
    ObjectId, ReviewBundleId, SessionId, VersionId, WorkspaceId,
};
pub use crate::name::{NameError, NormalizedName, PortableMetadata};
pub use crate::operation::{
    AttributionConfidence, DerivationKind, Operation, OperationKind, PreservedEntry, ReadRegion,
    ValidationOutcome,
};
pub use crate::sequence::{
    ActorSequence, SequenceExhausted, SequenceLedger, SequenceObservation, SequenceWitness,
};

/// The crate's name, so a placeholder still carries one verifiable behaviour.
pub const CRATE_NAME: &str = "mesh-operations";

/// This crate's own manifest, read at compile time by the guard below.
const MANIFEST: &str = include_str!("../Cargo.toml");

/// Whether the manifest declares no dependency of any kind.
///
/// Scans for every table whose name contains `dependencies` — `[dependencies]`,
/// `[dev-dependencies]`, `[build-dependencies]`, `[target.….dependencies]` — and requires every
/// line inside it to be blank or a comment.
///
/// This function is the fourth copy of itself in the workspace. `mesh-types`, `mesh-store` and
/// `mesh-state` each carry one, for the same reason and with the same shape: a crate that may not
/// declare a dependency cannot import a helper for checking that it declares no dependency.
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
    "mesh-operations declares a dependency. Plan §8.3 gives this crate no storage, no network and \
     no platform adapter, and `Cargo.lock` is governance surface this task's allowed paths do not \
     cover — a dependency edge here fails the declaration gate before it fails a review. If an \
     operation needs a platform fact, it belongs in an adapter, not the vocabulary."
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_stable() {
        assert_eq!(CRATE_NAME, "mesh-operations");
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
            "[package]\nname = \"mesh-operations\"\n\n[dependencies]\nmesh-types = { path = \"../mesh-types\" }\n",
            "[dependencies]\nserde = \"1\"\n",
            "[dev-dependencies]\nproptest = \"1\"\n",
            "[build-dependencies]\ncc = \"1\"\n",
            "[target.'cfg(unix)'.dependencies]\nlibc = \"0.2\"\n",
            "[dependencies]\n# a comment\n\nrusqlite = \"0.31\"\n",
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
            "[package]\nname = \"mesh-operations\"\n\n[dependencies]\n",
            "[dependencies]\n\n# nothing here on purpose\n",
            "[dependencies]\n  \t\n",
            "[package]\nname = \"x\"\n",
        ];
        for manifest in accepted {
            assert!(
                manifest_declares_no_dependency(manifest),
                "guard rejected a clean manifest: {manifest:?}"
            );
        }
    }

    #[test]
    fn a_later_table_ends_the_dependency_table() {
        assert!(manifest_declares_no_dependency(
            "[dependencies]\n\n[lints]\nworkspace = true\n"
        ));
    }
}
