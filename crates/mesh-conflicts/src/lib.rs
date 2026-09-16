//! The conflict rules table: merge, preserve-both, tombstone, deterministic cycle resolution.
//!
//! # One sentence this crate is built around
//!
//! **A conflict never destroys a version — it records that two exist.** Everything here follows
//! from that, including the parts that look like restrictions: why an automatic merge still
//! carries its inputs, why a removal concurrent with an edit is not a removal, and why any binary
//! side sends the whole object to "preserve both" rather than to a merge that might be clever.
//!
//! # The eleven rows of plan §4.8, and where each is made true
//!
//! | Concurrent operations | Result | Where |
//! |---|---|---|
//! | Rename + edit same object | Edit follows stable object ID | `src/change.rs` — the vocabulary cannot say "write to this path" |
//! | Move directory + child edit | Child remains attached to object graph | `src/snapshot.rs` — a path is derived from directory entries, never stored |
//! | Independent files | Merge automatically | [`resolve`] — one write, no removal |
//! | Non-overlapping text changes | Attempt three-way merge | [`three_way`] |
//! | Overlapping text changes | Preserve multiple versions | [`three_way`] returning [`TextMerge::Overlapping`] |
//! | Binary changes | Preserve both versions | [`resolve`] — any binary side |
//! | Delete + edit | Preserve tombstone and edited version | [`resolve`] — [`Disposition::TombstonedAndPreserved`] |
//! | Same-name create | Retain both object IDs; expose naming conflict | [`resolve_tree`] — [`NameCollision`] |
//! | Same-name create *on the target volume only* | Retain both object IDs; expose the collision, rename neither | [`resolve_tree`] — [`PortabilityCollision`] |
//! | Concurrent cyclic directory moves | Deterministic cycle-free resolution | [`resolve_tree`] — [`RefusedMove`] |
//! | Canonical head changed after review | Replay safely or require re-review | [`head_movement`] |
//! | Context input changed | Mark affected outputs stale | [`ContextLedger::stale_outputs`] |
//!
//! `tests/conflicts.rs` carries one targeted test per row, each written to fail if that row's
//! behaviour is removed. `tests/preservation.rs` and `tests/determinism.rs` carry the two property
//! campaigns.
//!
//! # The two properties, and where each one is proved
//!
//! | Property | Where it is made true | Where it is proved |
//! |---|---|---|
//! | No resolution loses a durable version | [`Resolution::reachable_versions`] is seeded from the base and every change before any rule runs, and no rule can remove from it | `tests/preservation.rs` — generated operation sets, every interleaving, set equality against the versions that went in |
//! | Two peers holding one operation set reach one resolution | Every ordering decision reads [`Stamp`], which is `lamport → event ULID → content hash` | `tests/determinism.rs` — one set shuffled a hundred ways, one resolution required |
//!
//! # What this crate does not contain, and why
//!
//! **No dependency at all** — not even a path dependency on `mesh-state`, because any dependency
//! edge rewrites `Cargo.lock` and this repository's declaration gate treats that as governance
//! surface a lane escalates rather than writes.
//! `docs/adr/0008-ship-the-sqlite-schema-without-a-sqlite-driver.md` records the reasoning;
//! `mesh-state` and `mesh-store` hit it first. `src/ids.rs` therefore mirrors four identifiers
//! under their own names, and `tests/mesh_state_drift.rs` fails if any of them stops being
//! declared there.
//!
//! **No version identifier for a merged text.** Minting one means hashing the merged bytes, and a
//! crate with no dependency has no hash. [`Disposition::Merged`] carries the lines and the
//! versions they came from; the layer that writes the merge is the one that names it.
//!
//! **No retention policy.** Plan §4.1 permits deletion only under an explicit retention policy,
//! and none exists on this tree — decision `01KZE5EMT8B82T38KRF64ZTYWP` is where it will be
//! decided. Until then the floor is everything, and nothing here can drop a version even if a
//! policy said it could.
//!
//! # The row that is not in plan §4.8, and why it is here
//!
//! `README.md` and `readme.md` are two names. They are two entries on Linux and one entry on a
//! default macOS volume, and `café.md` typed two ways is one entry there too. Neither pair is a
//! same-name create, so row eight does not fire and nothing would have reported them — the
//! filesystem would have picked a winner at materialization time, and no operation would record
//! which name was lost.
//!
//! [`PortabilityCollision`] is that pair, detected here and **renamed nowhere**: both names stand
//! in the tree, and the layer that knows which volume it is writing to refuses before the first
//! byte lands. `src/fold.rs` — byte-identical to `mesh-state`'s copy — decides which names fold
//! together, and `tests/compatibility-names.rs` holds it against a corpus generated from the
//! Unicode Character Database.
//!
//! **No storage, no clock, no input of any kind.** [`resolve`] is a function of its two arguments.
//! A resolution that read anything else would not be reproducible from the operation set, which is
//! the fifth acceptance criterion of the task that built this.
//!
//! # Where to start
//!
//! ```
//! use std::collections::BTreeSet;
//! use mesh_conflicts::{
//!     ActorId, Change, Content, Effect, EventId, Lamport, NormalizedName, ObjectId, Rule,
//!     Snapshot, Stamp, VersionId, resolve,
//! };
//!
//! let at = |lamport: u64, event: u8| {
//!     Stamp::new(Lamport::new(lamport), EventId::from_bytes([event; 16]), [0; 32])
//! };
//! let name = |text: &str| NormalizedName::new(text).unwrap();
//! let root = ObjectId::from_bytes([0; 16]);
//! let notes = ObjectId::from_bytes([1; 16]);
//!
//! // One file, three lines, one durable version.
//! let base = Snapshot::new(root).with_file(
//!     notes,
//!     root,
//!     name("notes.md"),
//!     Content::Text {
//!         version: VersionId::from_bytes([1; 32]),
//!         lines: vec!["first".into(), "second".into(), "third".into()],
//!     },
//! );
//!
//! // Ido renames it. An agent rewrites the middle line. Neither has heard of the other.
//! let ido = ActorId::from_bytes([1; 32]);
//! let agent = ActorId::from_bytes([2; 32]);
//! let changes = [
//!     Change::new(at(5, 1), ido, Effect::Rename { object: notes, name: name("journal.md") }),
//!     Change::new(
//!         at(5, 2),
//!         agent,
//!         Effect::WriteText {
//!             object: notes,
//!             version: VersionId::from_bytes([2; 32]),
//!             lines: vec!["first".into(), "SECOND".into(), "third".into()],
//!         },
//!     ),
//! ];
//!
//! let resolved = resolve(&base, &changes);
//!
//! // The rename landed and the edit landed, because the edit was never keyed by the path.
//! assert_eq!(resolved.path_of(notes).unwrap(), "/journal.md");
//! assert_eq!(resolved.version_of(notes), Some(VersionId::from_bytes([2; 32])));
//! assert!(resolved.rules_applied().contains(&Rule::EditFollowsIdentity));
//! assert!(!resolved.needs_review());
//!
//! // Both versions are still reachable. That holds for every row of the table.
//! assert_eq!(
//!     resolved.reachable_versions(),
//!     &BTreeSet::from([VersionId::from_bytes([1; 32]), VersionId::from_bytes([2; 32])]),
//! );
//! ```

// Private modules with a flat re-export at the crate root, following `mesh-types`, `mesh-store`
// and `mesh-state`: `docs/protocol.md` §3.10 requires every public item to resolve to a register
// term, and a module path is a second name for the same item that no register row covers.
mod change;
mod context;
mod fold;
mod ids;
mod name;
mod object;
mod outcome;
mod resolve;
mod review;
mod rules;
mod snapshot;
mod stamp;
mod text;
mod tree;

pub use crate::change::{Change, Effect};
pub use crate::context::ContextLedger;
pub use crate::fold::{
    canonical_key, case_key, caseless_key, combining_class, relate, upcase_key, NameFold,
    NameRelation, UNICODE_VERSION,
};
pub use crate::ids::{
    ActorId, HeadId, ObjectId, VersionId, ACTOR_ID_BYTES, HEAD_ID_BYTES, OBJECT_ID_BYTES,
    VERSION_ID_BYTES,
};
pub use crate::name::{NameError, NormalizedName, MAX_NAME_BYTES};
pub use crate::object::{Content, ObjectKind};
pub use crate::outcome::{Disposition, Outcome, Resolution};
pub use crate::resolve::resolve;
pub use crate::review::{head_movement, HeadMovement};
pub use crate::rules::Rule;
pub use crate::snapshot::{BaseObject, Snapshot, MAX_PATH_DEPTH};
pub use crate::stamp::{EventId, Lamport, Stamp};
pub use crate::text::{hunks, three_way, Hunk, OverlapRegion, TextMerge, MAX_MERGE_LINES};
pub use crate::tree::{
    resolve_tree, NameCollision, Placement, PortabilityCollision, RefusalReason, RefusedMove,
    TreeResolution,
};

/// The crate's name, so a placeholder still carries one verifiable behaviour.
pub const CRATE_NAME: &str = "mesh-conflicts";

/// This crate's own manifest, read at compile time by the guard below.
const MANIFEST: &str = include_str!("../Cargo.toml");

/// Whether the manifest declares no dependency of any kind.
///
/// Scans for every table whose name contains `dependencies` and requires every line inside it to
/// be blank or a comment. A dependency added anywhere in this manifest makes this `false`.
///
/// This function is the fourth copy of itself in the workspace — `mesh-types`, `mesh-store` and
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
    "mesh-conflicts declares a dependency. Every dependency edge rewrites Cargo.lock, which this \
     repository's declaration gate treats as governance surface a lane escalates rather than \
     writes. If this crate needs a type from mesh-state, mirror it in src/ids.rs and hold the \
     mirror with tests/mesh_state_drift.rs, or escalate for the edge."
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_stable() {
        assert_eq!(CRATE_NAME, "mesh-conflicts");
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
            "[package]\nname = \"mesh-conflicts\"\n\n[dependencies]\nmesh-state = { path = \"../mesh-state\" }\n",
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
            "[package]\nname = \"mesh-conflicts\"\n\n[dependencies]\n",
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
