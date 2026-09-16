//! Actor heads, causal parent resolution and head advancement.
//!
//! # One sentence this crate is built around
//!
//! **An actor head is derived from the applied causal set and from nothing else — not from arrival
//! order, not from how many times something arrived, not from a clock, and not from what an author
//! claimed.** Everything here follows from that, including the parts that look like restrictions.
//!
//! # The four properties, and where each one is actually checked
//!
//! | Property | Where it is made true | Where it is proved |
//! |---|---|---|
//! | Reapplying a delivered ChangeSet has no additional effect | [`HeadAdvancement::deliver`] answers a known identifier [`Reception::AlreadyApplied`] before recomputing anything | `tests/heads.rs` — the head is compared before and after; `tests/delivery.rs` — every ChangeSet in a generated history is delivered up to four times in a shuffled stream |
//! | A ChangeSet arriving before a causal parent is buffered and applied once the parent arrives, never dropped | [`Reception::Buffered`] holds it; [`HeadAdvancement::known_missing`] keeps it visible; nothing in this crate evicts | `tests/heads.rs` — reversed delivery of a chain, and a chain whose first ChangeSet is never delivered at all |
//! | Head advancement never consults wall-clock time | No source file here can name a clock: `src/no_ambient_input.rs` is a `const` scan asserted at compile time | `tests/heads.rs` — the same causal set delivered twice with the hybrid logical time set to what a badly-set machine produces, requiring byte-identical heads |
//! | Two actors applying the same causal set reach the same head | The causal order is *depth, then identifier* — both pure functions of the set ([`HeadAdvancement::applied`]) | `tests/delivery.rs` — a generated concurrent history delivered to several actors in independently shuffled orders, requiring one head |
//!
//! # What this crate does not contain, and why
//!
//! **No dependency at all** — not even a path dependency on `mesh-types`, because any dependency
//! edge rewrites `Cargo.lock` and this repository's declaration gate refuses a lane that write.
//! `mesh-store` hit the fence first; the reasoning and its five alternatives are in
//! `docs/adr/0008-ship-the-sqlite-schema-without-a-sqlite-driver.md`. `src/ids.rs` therefore
//! mirrors three `mesh-types` identifiers under their own names, built to be deleted the day the
//! edge is allowed, and `tests/mesh_types_drift.rs` fails if any of them stops being declared
//! there.
//!
//! **No digest implementation.** [`HeadDigest`] is the seam; the protocol digest is BLAKE3 and the
//! composition root supplies it. [`digest`](crate::HeadDigest) states why shipping a default here
//! would be worse than shipping none.
//!
//! **No operations and no materialization.** A ChangeSet reaches this crate as
//! [`DeliveredChangeSet`] — an identifier, an author, causal parents and the two heads its author
//! claims. What the operations *do* is `mesh-materializer`'s, and a head that read them would be
//! coupled to the whole operation vocabulary.
//!
//! **No retrievability axis.** `head state` is the review axis and lives here; `availability
//! state` is the retrievability axis and lives in `mesh-sync-engine` (`docs/protocol.md` §2.1 and
//! the register's home column). `src/head.rs` states why the two are never one enumeration.
//!
//! **No signature check and no identifier check.** This fold keys on [`ChangeSetId`] and treats
//! two records carrying one identifier as one ChangeSet. That is correct exactly as far as the
//! identifier really is the digest of the record — which `mesh-types` derives and `mesh-crypto`
//! signs, and neither of which this crate can reach. So the ceiling, stated rather than implied:
//! **a caller that hands this crate an identifier it did not verify gets a head derived from
//! records it did not verify.** Verification belongs at the boundary that has the bytes.
//!
//! # The second thing this crate owns: object identity
//!
//! `docs/protocol.md` SG-6 and plan §2.6: **an object's identity is not a function of its path.**
//! [`ObjectRegister`] is where that is made true. It stores a *directory entry* per object — the
//! name and the directory it hangs in — and never a path, so a path is derived by walking that
//! entry up to the root and a moved ancestor changes every descendant's path with no descendant
//! record touched at all.
//!
//! That shape is the whole of plan §11's subtree-move budget — a million descendants under 100 ms
//! and under 10 KiB of metadata. A register that stored a path per object would have to rewrite a
//! million records for one move; this one performs two map operations and appends one history
//! record, and nothing in its write path can even name a descendant.
//! `tests/identity.rs` measures it at five subtree sizes up to a million and prints the numbers.
//!
//! Placement and content are separate last-writer-wins facets over [`Stamp`], whose order is
//! `lamport → event ULID → content hash`. That is why a rename concurrent with an edit produces one
//! object carrying both changes rather than one of them winning. `src/identity.rs` states the one
//! case this register does **not** claim to converge on.
//!
//! # The third thing this crate owns: whether a name survives the trip
//!
//! A workspace synchronized between macOS, Linux and eventually Windows meets names that are two
//! entries on one host and one entry on another. **The answer is never to rename anything.** A
//! rename no operation records is data loss with a friendly face — the author's `README.md` comes
//! back as `readme.md`, nothing reports it, and the next synchronization sees a change nobody made.
//!
//! So the name is stored as the bytes the author typed and three things are derived from it:
//!
//! | Question | Where |
//! |---|---|
//! | Are these two names one entry, and on which volume? | [`relate`] and [`NameFold`] — `src/fold.rs` |
//! | Can this volume hold this name at all? | [`restrictions`] and [`VolumeProfile`] — `src/portable.rs` |
//! | Can this volume hold this whole directory, before anything is written? | [`preflight_directory`] — `src/preflight.rs` |
//!
//! and the metadata contract every filesystem adapter is written against — what mesh carries and
//! what it deliberately drops, each field with its reason — is [`PreservedMetadata`] and
//! [`DroppedMetadata`] in `src/metadata.rs`.
//!
//! `tests/names.rs` holds all of it against a corpus generated from the Unicode Character
//! Database, so the answers come from a published standard rather than from the code that produced
//! them. The ceiling, stated rather than implied: **no filesystem is touched anywhere here.** The
//! volume profiles are a written model of real filesystems; holding the model against the real
//! thing needs an adapter, and none exists on this tree.
//!
//! # Where to start
//!
//! ```
//! use mesh_state::{
//!     EventId, IdentityChange, Lamport, NormalizedName, ObjectId, ObjectKind, ObjectRegister,
//!     Stamp, VersionId,
//! };
//!
//! let at = |lamport: u64| {
//!     Stamp::new(Lamport::new(lamport), EventId::from_bytes([lamport as u8; 16]), [0; 32])
//! };
//! let name = |text: &str| NormalizedName::new(text).unwrap();
//! let root = ObjectId::from_bytes([0; 16]);
//! let notes = ObjectId::from_bytes([1; 16]);
//! let archive = ObjectId::from_bytes([2; 16]);
//!
//! let register = ObjectRegister::new(root, at(0));
//! let (register, _) = register.apply(
//!     &IdentityChange::Create { object: archive, kind: ObjectKind::Directory }, at(1));
//! let (register, _) = register.apply(
//!     &IdentityChange::Link { object: archive, directory: root, name: name("archive") }, at(2));
//! let (register, _) = register.apply(
//!     &IdentityChange::Create { object: notes, kind: ObjectKind::File }, at(3));
//! let (register, _) = register.apply(
//!     &IdentityChange::Link { object: notes, directory: root, name: name("notes.md") }, at(4));
//! assert_eq!(register.path_of(notes).unwrap().to_string(), "/notes.md");
//!
//! // A move and an edit, concurrent, on the same object. Both land; the identity does not move.
//! let (register, _) = register.apply(
//!     &IdentityChange::Move {
//!         object: notes,
//!         from_directory: root,
//!         from_name: name("notes.md"),
//!         to_directory: archive,
//!         to_name: name("2026.md"),
//!     },
//!     at(5),
//! );
//! let concurrently = Stamp::new(Lamport::new(5), EventId::from_bytes([0xee; 16]), [0; 32]);
//! let (register, _) = register.apply(
//!     &IdentityChange::WriteVersion { object: notes, version: VersionId::from_bytes([7; 32]) },
//!     concurrently,
//! );
//! assert_eq!(register.path_of(notes).unwrap().to_string(), "/archive/2026.md");
//! assert_eq!(register.version_of(notes), Some(VersionId::from_bytes([7; 32])));
//!
//! // The path it had before the move is still derivable, from the same records.
//! assert_eq!(register.path_at(notes, at(4)).unwrap().to_string(), "/notes.md");
//! ```
//!
//! ```
//! use mesh_state::{ActorId, ChangeSetId, HeadDigest, HeadAdvancement, HeadId, Reception};
//!
//! // The digest seam. A composition root supplies BLAKE3; an example supplies this.
//! struct ExampleDigest(u128);
//! impl HeadDigest for ExampleDigest {
//!     fn start() -> Self { Self(0x6c62_272e_07bb_0142_62b8_2175_6295_c58d) }
//!     fn absorb(&mut self, bytes: &[u8]) {
//!         for byte in bytes {
//!             self.0 = (self.0 ^ u128::from(*byte)).wrapping_mul(0x0100_0000_0000_0000_0000_013b);
//!         }
//!     }
//!     fn finish(self) -> HeadId {
//!         let mut out = [0u8; 32];
//!         out[..16].copy_from_slice(&self.0.to_be_bytes());
//!         out[16..].copy_from_slice(&self.0.rotate_left(37).to_be_bytes());
//!         HeadId::from_bytes(out)
//!     }
//! }
//!
//! let ido = HeadAdvancement::<ExampleDigest>::new(ActorId::from_bytes([1; 32]));
//!
//! // Ido authors one ChangeSet. Its causal parents and both its heads are derived, never supplied.
//! let (ido, first) = ido.author(ChangeSetId::from_bytes([0xa1; 32])).unwrap();
//! assert!(first.parents().is_genesis());
//! assert_eq!(ido.head(), first.resulting_head());
//!
//! // An agent that has applied the same one ChangeSet holds the same head.
//! let agent = HeadAdvancement::<ExampleDigest>::new(ActorId::from_bytes([2; 32]));
//! let (agent, reception) = agent.deliver(first.clone());
//! assert!(matches!(reception, Reception::Applied { .. }));
//! assert_eq!(agent.head(), ido.head());
//!
//! // Delivering it again changes nothing at all.
//! let (agent_again, reception) = agent.deliver(first);
//! assert_eq!(reception, Reception::AlreadyApplied);
//! assert_eq!(agent_again.head(), agent.head());
//! ```

// Private modules with a flat re-export at the crate root, following `mesh-types` and
// `mesh-store`: `docs/protocol.md` §3.10 requires every public item to resolve to a register term,
// and a module path is a second name for the same item that no register row covers.
mod advance;
mod changeset;
mod digest;
mod fold;
mod head;
mod identity;
mod ids;
mod metadata;
mod name;
mod no_ambient_input;
mod object;
mod parents;
mod placement;
mod portable;
mod preflight;
mod reception;
mod stamp;

pub use crate::advance::HeadAdvancement;
pub use crate::changeset::DeliveredChangeSet;
pub use crate::digest::{HeadDigest, HEAD_DOMAIN};
pub use crate::fold::{
    canonical_key, case_key, caseless_key, combining_class, relate, upcase_key, NameFold,
    NameRelation, UNICODE_VERSION,
};
pub use crate::head::{ActorHead, HeadState};
pub use crate::identity::{ObjectRegister, MAX_PATH_DEPTH};
pub use crate::ids::{ActorId, ChangeSetId, HeadId, IdError, VersionId};
pub use crate::metadata::{DroppedMetadata, PreservedMetadata};
pub use crate::name::{NameError, NormalizedName, WorkspacePath};
pub use crate::object::{ObjectId, ObjectKind};
pub use crate::parents::CausalParents;
pub use crate::placement::{
    IdentityChange, IdentityOutcome, IdentityRefusal, Placement, PlacementRecord, VersionRecord,
};
pub use crate::portable::{
    is_portable, restrictions, restrictions_everywhere, NameRestriction, VolumeProfile,
    RESERVED_DEVICE_NAMES,
};
pub use crate::preflight::{preflight_directory, DirectoryPreflight, EntryCollision, NameFinding};
pub use crate::reception::{KnownMissing, Reception, Refusal};
pub use crate::stamp::{EventId, Lamport, Stamp};

/// The crate's name, so a placeholder still carries one verifiable behaviour.
pub const CRATE_NAME: &str = "mesh-state";

/// This crate's own manifest, read at compile time by the guard below.
const MANIFEST: &str = include_str!("../Cargo.toml");

/// Whether the manifest declares no dependency of any kind.
///
/// Scans for every table whose name contains `dependencies` and requires every line inside it to
/// be blank or a comment. A dependency added anywhere in this manifest makes this `false`.
///
/// This function is the third copy of itself in the workspace — `mesh-types` and `mesh-store` each
/// carry one. Factoring it out would need a shared crate, and depending on a shared crate is the
/// exact thing it exists to prevent, so the duplication is the cheaper of the two costs and is
/// named here rather than left to be discovered.
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
    "mesh-state declares a dependency. Every dependency edge rewrites Cargo.lock, which this \
     repository's declaration gate treats as governance surface a lane escalates rather than \
     writes. If this crate needs a type from mesh-types, mirror it in src/ids.rs and hold the \
     mirror with tests/mesh_types_drift.rs, or escalate for the edge."
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_stable() {
        assert_eq!(CRATE_NAME, "mesh-state");
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
            "[package]\nname = \"mesh-state\"\n\n[dependencies]\nmesh-types = { path = \"../mesh-types\" }\n",
            "[dev-dependencies]\nproptest = \"1\"\n",
            "[build-dependencies]\ncc = \"1\"\n",
            "[target.'cfg(unix)'.dependencies]\nlibc = \"0.2\"\n",
            "[dependencies]\n# a comment\n\nblake3 = \"1\"\n",
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
            "[package]\nname = \"mesh-state\"\n\n[dependencies]\n",
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
