//! The canonical entity model: identity, actors, objects, versions and ChangeSets.
//!
//! Every signed record downstream is encoded against these types, so two properties matter more
//! than anything else in the crate.
//!
//! # Identity is split in two, and the split is load-bearing
//!
//! Plan §4.2: **UUIDv7 for mutable entity identity, BLAKE3-derived identifiers for immutable
//! canonical records.**
//!
//! * [`WorkspaceId`], [`SessionId`], [`ObjectId`] and [`CapabilityId`] name things whose content
//!   changes. Naming them by their content would rename them on every change, so they are minted:
//!   a version-7 UUID, time-ordered, carrying nothing but the millisecond it was minted in. There
//!   is no constructor in this crate that turns bytes into one of them.
//! * [`VersionId`], [`ManifestId`], [`ChangeSetId`], [`HeadId`], [`ReviewBundleId`], [`ApprovalId`],
//!   [`ContentHash`] and [`ActorId`] name things that never change. They are the digest of what
//!   they name, which is what makes those records **self-verifying**: hold the record, recompute
//!   the identifier, and you know whether the bytes are the bytes that were promised. No registry
//!   lookup, no trusted party.
//!
//! The two families are different types with no conversion between them, so the distinction cannot
//! decay into a convention downstream crates remember to follow.
//!
//! # This crate has no dependency at all, and that is checked at compile time
//!
//! `[dependencies]` in `crates/mesh-types/Cargo.toml` is empty, and
//! `const _: () = assert!(…)` below reads the manifest with [`include_str`] and fails the build if
//! it ever stops being. It fails at `cargo build`, not at `cargo test`.
//!
//! Declaring no dependency is not by itself the plan §8.3 rule, because `std` supplies a
//! filesystem and a network for free. The companion scan in `src/no_ambient_io.rs` covers that
//! route — as a lint over the crate's own source text, with its limits stated there rather than
//! claimed away.
//!
//! BLAKE3 is therefore implemented here rather than taken from the `blake3` crate. That was a real
//! decision with real costs; it is recorded in
//! `docs/adr/0002-derive-record-ids-with-an-in-crate-blake3.md` and the swap is a one-type change
//! behind [`ContentDigest`].
//!
//! # Where to start
//!
//! ```
//! use mesh_types::{
//!     Blake3, ChunkRef, Digest32, FileManifest, ManifestId, derive_id,
//! };
//!
//! let manifest = FileManifest::new(
//!     6,
//!     Digest32::from_bytes([0xab; 32]),
//!     vec![ChunkRef::new(Digest32::from_bytes([0xcd; 32]), 0, 6)],
//! );
//! assert!(manifest.chunks_are_contiguous());
//!
//! let id: ManifestId = derive_id::<Blake3, _>(&manifest);
//! assert_eq!(id, derive_id::<Blake3, _>(&manifest));
//! ```

// The modules are private and every public item is re-exported at the crate root. A flat surface
// is deliberate: `docs/protocol.md` §3.10 requires every public item to resolve to a register
// term, and a module path is a second name for the same item that no register row covers.
mod actor;
mod blake3;
mod canonical;
mod cbor;
mod cbor_reader;
mod changeset;
mod digest;
mod entity_id;
#[cfg(feature = "vectors")]
mod json;
mod manifest;
mod no_ambient_io;
mod object;
mod record_id;
mod session;
mod uuid;
#[cfg(feature = "vectors")]
mod vectors;

pub use crate::actor::{Actor, ActorKind, Hlc, PublicKey, Signature, Timestamp};
pub use crate::blake3::Blake3Hasher;
pub use crate::canonical::{
    canonical_digest, decode_canonical, encode_canonical, schema_violations, CanonicalEncode,
    CanonicalType, CanonicalValue, DecodeError, FieldSchema, RecordSchema, SchemaViolation,
    SCHEMA_FORMAT,
};
pub use crate::cbor::{CborWriter, CBOR_PROFILE};
pub use crate::cbor_reader::{CborError, CborReader, MAX_NESTING};
pub use crate::changeset::{
    ActorSequence, CausalParents, ChangeSet, ChangeSetDraft, PolicyEpoch, Unset,
};
pub use crate::digest::{
    derive_id, Absorb, Blake3, CanonicalRecord, ContentDigest, Digest32, DigestHasher,
    DigestParseError, DigestWriter, DomainTag,
};
pub use crate::entity_id::{CapabilityId, EntityIdError, ObjectId, SessionId, WorkspaceId};
pub use crate::manifest::{ChunkRef, FileManifest};
pub use crate::object::{
    DirectoryEntry, DirectoryVersion, FileVersion, NameError, NormalizedName, Object, ObjectKind,
    PortableMetadata,
};
pub use crate::record_id::{
    ActorId, ApprovalId, ChangeSetId, ContentHash, HeadId, ManifestId, ReviewBundleId, VersionId,
};
pub use crate::session::ActivitySession;
pub use crate::uuid::{Uuid, UuidParseError};
#[cfg(feature = "vectors")]
pub use crate::vectors::{published_documents, PublishedDocument, VECTOR_FORMAT};

/// The crate's name, so a placeholder still carries one verifiable behaviour.
pub const CRATE_NAME: &str = "mesh-types";

/// This crate's own manifest, read at compile time by the guard below.
const MANIFEST: &str = include_str!("../Cargo.toml");

/// Whether the manifest declares no dependency of any kind.
///
/// Scans for every table whose name contains `dependencies` — `[dependencies]`,
/// `[dev-dependencies]`, `[build-dependencies]`, `[target.…​.dependencies]` — and requires every
/// line inside it to be blank or a comment. A dependency added anywhere in this manifest makes
/// this `false`.
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
    "mesh-types declares a dependency. Plan §8.3 gives this crate no storage and no network \
     dependency, and it currently has none of any kind. If a type here needs one, the type is in \
     the wrong crate — move it rather than adding the dependency."
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_stable() {
        assert_eq!(CRATE_NAME, "mesh-types");
    }

    #[test]
    fn the_real_manifest_declares_no_dependency() {
        assert!(manifest_declares_no_dependency(MANIFEST));
    }

    /// The compile-time guard above is only worth having if it can fail. It cannot be observed
    /// failing without failing the build, so the function it is built on is exercised directly
    /// against manifests that a lane might realistically write.
    #[test]
    fn the_guard_rejects_every_shape_of_dependency() {
        let rejected = [
            "[package]\nname = \"mesh-types\"\n\n[dependencies]\nblake3 = \"1\"\n",
            "[dependencies]\nuuid = { version = \"1\", features = [\"v7\"] }\n",
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
            "[package]\nname = \"mesh-types\"\n\n[dependencies]\n",
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

    /// A table that merely mentions the word must not switch the guard off for what follows.
    #[test]
    fn a_later_table_ends_the_dependency_table() {
        let manifest = "[dependencies]\n\n[lints]\nworkspace = true\n";
        assert!(manifest_declares_no_dependency(manifest));
    }
}
