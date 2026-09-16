//! Deterministic review bundle generation: the artifact a person approves, and its exact name.
//!
//! # One sentence this crate is built around
//!
//! **Only an exact human-reviewed state advances the protected shared version.** That is checkable
//! only if "exactly this" has a name, and a name is only worth anything if two processes computing
//! it from the same state agree to the byte. [`ReviewBundle`] is that artifact and
//! [`ReviewBundle::id`] is that name.
//!
//! # The four things this crate promises, and where each is proved
//!
//! | Promise | Where it is made true | Where it is proved |
//! |---|---|---|
//! | The diff is computed, never declared | [`diff`] takes two states and has no parameter for "what I changed" | `tests/bundle.rs` — an explanation that claims the opposite leaves the change list identical |
//! | The same inputs produce byte-identical bundles | Every collection is ordered by content; no clock, no environment, no hash-map iteration | `tests/bundle.rs` — a **second process** recomputes the identifier, and every field is mutated in turn to show the identifier moves |
//! | Later actor work cannot enter an existing bundle | Every field of [`ReviewBundle`] is private and no method takes `&mut self` | `tests/bundle.rs` — appending work leaves the bundle byte-identical and produces a *different* bundle |
//! | Conflicts and dependency impact are in the bundle | [`compute_bundle`] computes both and absorbs them into the identifier | `tests/bundle.rs` — mutating either moves the identifier |
//! | The rendered diff is a function of the bundle and nothing else | [`present`] takes a change list, sorts it and reads no state | `tests/diff.rs` — a **second process** recomputes [`DiffPresentation::digest`], and a reordered change list renders the same bytes |
//!
//! # What a reviewer is shown
//!
//! [`ReviewBundle::presentation`] turns the change list into [`PresentedChange`] entries. A rename
//! is one entry with both paths — never a removal beside a creation — because [`diff`] follows an
//! object by its identifier. Text arrives as [`TextHunk`]s with [`CONTEXT_LINES`] of context; bytes
//! arrive as size and hash and are never rendered as lines; and anything that cannot be diffed
//! meaningfully — a text above [`MAX_DIFF_LINES`], a version that changed class — arrives as
//! [`ChangeBody::Opaque`] carrying its metadata and the reason, rather than an invented
//! representation.
//!
//! # Refusal is a feature
//!
//! [`compute_bundle`] returns [`BundleRefusal`] rather than approximating. Plan §4.6: a case that
//! cannot be computed deterministically cannot be approved until it can. A refusal is not a
//! rejection of the work — the work stands and the checkpoint stands; what is refused is the claim
//! that this artifact names exact bytes.
//!
//! # Composition boundaries
//!
//! Bundle generation remains pure and deterministic. [`ApprovalReceipt`] is the production
//! composition boundary above it: it binds a bundle and an exact shared-head transition to a
//! reviewer's public identity and decision, then delegates signature verification to
//! `mesh-crypto`'s audited Ed25519 adapter. This crate never holds or returns private key material.
//!
//! **No merge.** [`Conflict`] reports that the canonical head moved over something the actor
//! touched. Deciding what the merged result is belongs to `mesh-conflicts`; a bundle that merged
//! would be showing a person a result nobody computed for the canonical head.
//!
//! **No storage, no clock, no input of any kind.** [`compute_bundle`] is a function of its one
//! argument. A bundle that read anything else would not be reproducible, which is the whole
//! deliverable.
//!
//! # Where to start
//!
//! ```
//! use mesh_approval::{
//!     compute_bundle, ActorId, BundleRequest, Content, HeadId, NormalizedName, ObjectId,
//!     VersionId, WorkspaceState,
//! };
//!
//! let name = |text: &str| NormalizedName::new(text).unwrap();
//! let root = ObjectId::from_bytes([0; 16]);
//! let notes = ObjectId::from_bytes([1; 16]);
//! let text = |byte: u8, line: &str| Content::Text {
//!     version: VersionId::from_bytes([byte; 32]),
//!     lines: vec![line.to_owned()],
//! };
//!
//! // The protected shared version, and the checkpoint an agent is offering.
//! let canonical =
//!     WorkspaceState::new(root).with_file(notes, root, name("notes.md"), text(1, "first"));
//! let actor = canonical
//!     .clone()
//!     .with_file(notes, root, name("journal.md"), text(2, "FIRST"));
//!
//! let request = BundleRequest::new(
//!     canonical.clone(),
//!     canonical.clone(),
//!     HeadId::from_bytes([10; 32]),
//!     actor.clone(),
//!     HeadId::from_bytes([11; 32]),
//!     ActorId::from_bytes([12; 32]),
//! )
//! .with_explanation("tidied the notes");
//!
//! let bundle = compute_bundle(&request).unwrap();
//!
//! // The agent said "tidied"; the bundle says a rename and a rewrite, because it read the states.
//! let effects: Vec<&str> = bundle.changes().iter().map(|c| c.effect().label()).collect();
//! assert_eq!(effects, vec!["renamed", "content-written"]);
//!
//! // Replaying the difference onto the base reproduces exactly what was offered.
//! assert_eq!(bundle.apply_to(&canonical).unwrap(), actor);
//!
//! // What a review surface draws: the rename keeps its object, and the rewrite carries lines.
//! let rendered = bundle.presentation();
//! let rename = &rendered.renames()[0];
//! assert_eq!((rename.path_before(), rename.path_after()), (Some("/notes.md"), Some("/journal.md")));
//! assert_eq!(rendered.entries()[1].hunks()[0].added(), 1);
//! assert_eq!(rendered.digest(), bundle.presentation().digest());
//!
//! // The same states name the same bytes, here and in any other process.
//! assert_eq!(bundle.id(), compute_bundle(&request).unwrap().id());
//! ```

// Private modules with a flat re-export at the crate root, following `mesh-types`, `mesh-store`,
// `mesh-state` and `mesh-conflicts`: `docs/protocol.md` §3.10 requires every public item to resolve
// to a register term, and a module path is a second name for the same item that no register row
// covers.
mod blake3;
mod bundle;
mod conflict;
mod diff;
mod digest;
mod human_receipt;
mod ids;
mod impact;
mod name;
mod presentation;
mod receipt;
mod state;
mod text_diff;
mod validation;

pub use crate::blake3::Blake3Hasher;
pub use crate::bundle::{
    compute_bundle, BundleRefusal, BundleRequest, ReviewBundle, MAX_EXPLANATION_BYTES,
};
pub use crate::conflict::{Conflict, Disposition};
pub use crate::diff::{apply, diff, ApplyError, DiffError, Effect, ObjectChange};
pub use crate::digest::{
    derive_id, Absorb, Blake3, CanonicalRecord, ContentDigest, Digest32, DigestHasher,
    DigestParseError, DigestWriter, DomainTag,
};
pub use crate::human_receipt::{
    verify_human_approval_receipt, ApprovalWorkspaceId, ExpectedHumanApproval,
    HumanApprovalContext, HumanApprovalCredential, HumanApprovalReceipt, HumanApprovalReceiptDraft,
    HumanApprovalReceiptError, VerifiedHumanApprovalReceipt, APPROVAL_ALGORITHM,
    APPROVAL_APPLICATION_SCOPE, APPROVAL_SELECTION, APPROVAL_USER_VERIFICATION,
};
pub use crate::ids::{
    ActorId, HeadId, ObjectId, ReviewBundleId, VersionId, ACTOR_ID_BYTES, HEAD_ID_BYTES,
    OBJECT_ID_BYTES, VERSION_ID_BYTES,
};
pub use crate::impact::{DependencyGraph, StaleOutput};
pub use crate::name::{NameError, NormalizedName, MAX_NAME_BYTES};
pub use crate::presentation::{
    present, ChangeBody, ContentSummary, DiffPresentation, OpaqueReason, PresentedChange,
};
pub use crate::receipt::{
    verify_approval_receipt, verify_approval_receipt_signature, ApprovalDecision, ApprovalReceipt,
    ApprovalReceiptDraft, ApprovalReceiptError, ExpectedAdvance, HumanApprovedAdvance,
    VerifiedApprovalReceipt,
};
pub use crate::state::{Content, ObjectKind, StateObject, WorkspaceState, MAX_PATH_DEPTH};
pub use crate::text_diff::{text_hunks, DiffLine, TextHunk, CONTEXT_LINES, MAX_DIFF_LINES};
pub use crate::validation::{ValidationResult, Verdict};

/// The crate's name, so every build carries one verifiable behaviour that names it.
pub const CRATE_NAME: &str = "mesh-approval";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_stable() {
        assert_eq!(CRATE_NAME, "mesh-approval");
    }
}
