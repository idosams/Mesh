//! The presentation contract: what a review surface renders, derived from the change list.
//!
//! # Why this is a separate value and not a method on the bundle
//!
//! [`ReviewBundle`](crate::ReviewBundle) names exact bytes, and its identifier is the digest of the
//! record a person approved. A presentation is a *view* of that record — the same facts, arranged
//! for reading. Deriving it here rather than storing it inside the bundle has two consequences the
//! task contract asks for:
//!
//! * The bundle's identifier does not move when the rendering changes. A UI that gains a column
//!   does not invalidate every approval ever made.
//! * The presentation is a pure function of the change list, so "the same bundle always renders the
//!   same diff" is a statement with a proof rather than a hope: [`DiffPresentation::digest`] names
//!   the rendered bytes, and `tests/diff.rs` computes it in a second process.
//!
//! # The three things a reviewer is shown, and the fourth that refuses
//!
//! | Body | When | What a surface shows |
//! |---|---|---|
//! | [`ChangeBody::Placement`] | a move, a rename, a directory | where it was and where it is — there is no content to render |
//! | [`ChangeBody::Text`] | both versions are text, both within [`MAX_DIFF_LINES`](crate::MAX_DIFF_LINES) | hunks with [`CONTEXT_LINES`](crate::CONTEXT_LINES) of context |
//! | [`ChangeBody::Binary`] | either version is bytes | the size and the hash of each side, and never a rendered line |
//! | [`ChangeBody::Opaque`] | the text is above the ceiling, or a version changed class | the metadata, and the reason it is metadata |
//!
//! The last row is this task's failure-and-recovery clause: a file class that cannot be diffed
//! meaningfully is presented as an opaque change with its metadata rather than given an invented
//! representation. A garbled text rendering of a PNG is worse than no rendering, because a person
//! approves it.
//!
//! # A rename is a rename here too
//!
//! [`crate::diff`] follows an object by its identifier, so a rename arrives as one
//! [`Effect::Renamed`](crate::Effect) and never as a removal plus a creation. This module carries
//! that through: one entry, one object, both paths. Nothing here re-keys by path, so there is no
//! place for the pairing to be lost.

use crate::diff::{Effect, ObjectChange};
use crate::digest::{
    Absorb, Blake3, ContentDigest, Digest32, DigestHasher, DigestWriter, DomainTag,
};
use crate::ids::{ObjectId, VersionId};
use crate::state::Content;
use crate::text_diff::{text_hunks, TextHunk};

/// The domain a rendered diff's digest is derived in.
///
/// Distinct from the bundle's, because these are different bytes about the same facts and a value
/// that could be mistaken for the other would let an approval name a rendering.
const PRESENTATION_DOMAIN: DomainTag = DomainTag::new("mesh.v0.diff-presentation");

/// One version, summarised: enough to identify and size it, never enough to render it.
///
/// This is what "presented as a change with size and hash" means for bytes. For text it is the
/// version and the line count, so a surface can say *how much* changed before it has drawn a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentSummary {
    /// A text version.
    Text {
        /// Its identity.
        version: VersionId,
        /// How many lines it holds.
        lines: u64,
    },
    /// A binary version.
    Binary {
        /// Its identity.
        version: VersionId,
        /// The content hash of the bytes.
        digest: Digest32,
        /// How many bytes there are.
        byte_length: u64,
    },
}

impl ContentSummary {
    /// Summarise one version.
    #[must_use]
    pub fn of(content: &Content) -> Self {
        match content {
            Content::Text { version, lines } => Self::Text {
                version: *version,
                lines: lines.len() as u64,
            },
            Content::Binary {
                version,
                digest,
                byte_length,
            } => Self::Binary {
                version: *version,
                digest: Digest32::from_bytes(*digest),
                byte_length: *byte_length,
            },
        }
    }

    /// The version this summarises.
    #[must_use]
    pub const fn version(&self) -> VersionId {
        match self {
            Self::Text { version, .. } | Self::Binary { version, .. } => *version,
        }
    }

    /// Whether the version is opaque bytes.
    #[must_use]
    pub const fn is_binary(&self) -> bool {
        matches!(self, Self::Binary { .. })
    }

    /// The line count, when the version is text.
    #[must_use]
    pub const fn line_count(&self) -> Option<u64> {
        match self {
            Self::Text { lines, .. } => Some(*lines),
            Self::Binary { .. } => None,
        }
    }

    /// The byte length, when the version is bytes.
    #[must_use]
    pub const fn byte_length(&self) -> Option<u64> {
        match self {
            Self::Text { .. } => None,
            Self::Binary { byte_length, .. } => Some(*byte_length),
        }
    }

    /// The content hash, when the version is bytes.
    #[must_use]
    pub const fn digest(&self) -> Option<Digest32> {
        match self {
            Self::Text { .. } => None,
            Self::Binary { digest, .. } => Some(*digest),
        }
    }
}

impl Absorb for ContentSummary {
    fn absorb<H: DigestHasher>(&self, writer: &mut DigestWriter<H>) {
        match self {
            Self::Text { version, lines } => {
                writer.u64(0);
                writer.bytes(version.as_bytes());
                writer.u64(*lines);
            }
            Self::Binary {
                version,
                digest,
                byte_length,
            } => {
                writer.u64(1);
                writer.bytes(version.as_bytes());
                writer.digest(digest);
                writer.u64(*byte_length);
            }
        }
    }
}

/// Why a change carries metadata instead of a diff.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpaqueReason {
    /// One side holds more than [`MAX_DIFF_LINES`](crate::MAX_DIFF_LINES) lines. The ceiling is
    /// `mesh-conflicts`' merge ceiling, inherited rather than re-decided.
    AboveLineCeiling,
    /// One version is text and the other is bytes. There is no line correspondence across that
    /// boundary, and inventing one would render bytes as text in the artifact a person approves.
    ContentClassChanged,
}

impl OpaqueReason {
    /// A short, stable word for a surface that renders one.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::AboveLineCeiling => "above-line-ceiling",
            Self::ContentClassChanged => "content-class-changed",
        }
    }

    /// The tag this reason is absorbed under.
    #[must_use]
    pub const fn rank(&self) -> u64 {
        match self {
            Self::AboveLineCeiling => 0,
            Self::ContentClassChanged => 1,
        }
    }
}

/// What a surface renders under one entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChangeBody {
    /// Where the object hangs changed, or it is a directory. There is no content to render.
    Placement,
    /// Text, with the runs of changed lines and their context.
    Text {
        /// The hunks, in earlier-version order. Empty only when the two versions are equal, which
        /// [`crate::diff`] does not emit a change for.
        hunks: Vec<TextHunk>,
    },
    /// Bytes. A surface shows the size and the hash from the entry's summaries and never a line.
    Binary,
    /// Not diffable. The entry's summaries are the whole of what can be shown.
    Opaque {
        /// Why.
        reason: OpaqueReason,
    },
}

impl ChangeBody {
    /// A short, stable word for a surface that renders one.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Placement => "placement",
            Self::Text { .. } => "text",
            Self::Binary => "binary",
            Self::Opaque { .. } => "opaque",
        }
    }

    /// The tag this body is absorbed under.
    #[must_use]
    pub const fn rank(&self) -> u64 {
        match self {
            Self::Placement => 0,
            Self::Text { .. } => 1,
            Self::Binary => 2,
            Self::Opaque { .. } => 3,
        }
    }
}

impl Absorb for ChangeBody {
    fn absorb<H: DigestHasher>(&self, writer: &mut DigestWriter<H>) {
        writer.u64(self.rank());
        match self {
            Self::Placement | Self::Binary => {}
            Self::Text { hunks } => {
                writer.sequence(hunks, |writer, hunk| hunk.absorb(writer));
            }
            Self::Opaque { reason } => {
                writer.u64(reason.rank());
            }
        }
    }
}

/// One entry a review surface renders: one effect on one object, with what to show for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresentedChange {
    object: ObjectId,
    path_before: Option<String>,
    path_after: Option<String>,
    effect: &'static str,
    rank: u64,
    before: Option<ContentSummary>,
    after: Option<ContentSummary>,
    body: ChangeBody,
}

impl PresentedChange {
    /// The object this happened to. A rename does not change it, which is why the pairing survives.
    #[must_use]
    pub const fn object(&self) -> ObjectId {
        self.object
    }

    /// Where the object was, or [`None`] when it did not exist in the earlier state.
    #[must_use]
    pub fn path_before(&self) -> Option<&str> {
        self.path_before.as_deref()
    }

    /// Where the object is, or [`None`] when it does not exist in the later state.
    #[must_use]
    pub fn path_after(&self) -> Option<&str> {
        self.path_after.as_deref()
    }

    /// The effect's stable word — the same one [`Effect::label`](crate::Effect::label) returns.
    #[must_use]
    pub const fn effect(&self) -> &'static str {
        self.effect
    }

    /// The effect's rank, which is the order entries for one object are shown in.
    #[must_use]
    pub const fn rank(&self) -> u64 {
        self.rank
    }

    /// The version the object held, when it held one.
    #[must_use]
    pub const fn before(&self) -> Option<ContentSummary> {
        self.before
    }

    /// The version the object holds, when it holds one.
    #[must_use]
    pub const fn after(&self) -> Option<ContentSummary> {
        self.after
    }

    /// What to render.
    #[must_use]
    pub const fn body(&self) -> &ChangeBody {
        &self.body
    }

    /// The hunks, or an empty slice when this entry is not a text change.
    #[must_use]
    pub fn hunks(&self) -> &[TextHunk] {
        match &self.body {
            ChangeBody::Text { hunks } => hunks,
            ChangeBody::Placement | ChangeBody::Binary | ChangeBody::Opaque { .. } => &[],
        }
    }

    /// Whether this entry is the object arriving at a new name in the same directory.
    #[must_use]
    pub fn is_rename(&self) -> bool {
        self.effect == "renamed"
    }
}

impl Absorb for PresentedChange {
    fn absorb<H: DigestHasher>(&self, writer: &mut DigestWriter<H>) {
        writer.bytes(self.object.as_bytes());
        writer.option(self.path_before.as_ref(), |writer, path| {
            writer.text(path);
        });
        writer.option(self.path_after.as_ref(), |writer, path| {
            writer.text(path);
        });
        writer.text(self.effect);
        writer.u64(self.rank);
        writer.option(self.before.as_ref(), |writer, summary| {
            summary.absorb(writer);
        });
        writer.option(self.after.as_ref(), |writer, summary| {
            summary.absorb(writer);
        });
        self.body.absorb(writer);
    }
}

/// Every entry a review surface renders, in a fixed order.
///
/// Ordered by object identifier and then by effect rank — the order [`crate::diff`] already emits,
/// re-established here so that the presentation is a function of the *set* of changes and not of
/// the order a caller happened to hold them in.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct DiffPresentation {
    entries: Vec<PresentedChange>,
}

impl DiffPresentation {
    /// The entries.
    #[must_use]
    pub fn entries(&self) -> &[PresentedChange] {
        &self.entries
    }

    /// How many entries there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there is nothing to render.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every entry that is a rename, in order.
    ///
    /// Offered because "a rename is presented as a rename" is the acceptance criterion a surface is
    /// most likely to get wrong by re-deriving it from paths.
    #[must_use]
    pub fn renames(&self) -> Vec<&PresentedChange> {
        self.entries
            .iter()
            .filter(|entry| entry.is_rename())
            .collect()
    }

    /// The 32 bytes that name this rendering.
    ///
    /// A pure function of the entries. Two processes rendering the same change list compute the
    /// same value; a rendering that differs anywhere — one line number, one context line, one
    /// opaque reason — computes a different one.
    #[must_use]
    pub fn digest(&self) -> Digest32 {
        let mut writer = DigestWriter::new(PRESENTATION_DOMAIN, Blake3::hasher());
        self.absorb(&mut writer);
        writer.finish()
    }
}

impl Absorb for DiffPresentation {
    fn absorb<H: DigestHasher>(&self, writer: &mut DigestWriter<H>) {
        writer.sequence(&self.entries, |writer, entry| entry.absorb(writer));
    }
}

/// Render a change list for review.
///
/// A pure function of `changes`: no state is read, nothing is looked up, and the result is sorted
/// by object identifier and effect rank, so handing the same changes in a different order renders
/// the same bytes.
#[must_use]
pub fn present(changes: &[ObjectChange]) -> DiffPresentation {
    let mut entries: Vec<PresentedChange> = changes.iter().map(entry_for).collect();
    entries.sort_by_key(|entry| (entry.object, entry.rank));
    DiffPresentation { entries }
}

/// One change as one entry.
fn entry_for(change: &ObjectChange) -> PresentedChange {
    let (before, after) = versions(change.effect());
    let body = body_for(before, after);
    PresentedChange {
        object: change.object(),
        path_before: change.path_before().map(str::to_owned),
        path_after: change.path_after().map(str::to_owned),
        effect: change.effect().label(),
        rank: change.effect().rank(),
        before: before.map(ContentSummary::of),
        after: after.map(ContentSummary::of),
        body,
    }
}

/// The two versions an effect stands between.
///
/// A move and a rename stand between none: the version is untouched, and repeating it under a
/// placement change would show a reviewer a content change that did not happen.
fn versions(effect: &Effect) -> (Option<&Content>, Option<&Content>) {
    match effect {
        Effect::Created { content, .. } => (None, content.as_ref()),
        Effect::Removed { content, .. } => (content.as_ref(), None),
        Effect::Moved { .. } | Effect::Renamed { .. } => (None, None),
        Effect::ContentWritten { from, to } => (from.as_ref(), Some(to)),
        Effect::ContentCleared { from } => (Some(from), None),
    }
}

/// What to render between two versions.
fn body_for(before: Option<&Content>, after: Option<&Content>) -> ChangeBody {
    let empty: Vec<String> = Vec::new();
    match (before, after) {
        (None, None) => ChangeBody::Placement,
        (Some(content), None) | (None, Some(content)) if content.is_binary() => ChangeBody::Binary,
        (Some(before), Some(after)) if before.is_binary() || after.is_binary() => {
            if before.is_binary() && after.is_binary() {
                ChangeBody::Binary
            } else {
                ChangeBody::Opaque {
                    reason: OpaqueReason::ContentClassChanged,
                }
            }
        }
        _ => {
            let before_lines = before.and_then(Content::lines).unwrap_or(&empty);
            let after_lines = after.and_then(Content::lines).unwrap_or(&empty);
            text_hunks(before_lines, after_lines).map_or(
                ChangeBody::Opaque {
                    reason: OpaqueReason::AboveLineCeiling,
                },
                |hunks| ChangeBody::Text { hunks },
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::diff;
    use crate::name::NormalizedName;
    use crate::state::WorkspaceState;

    fn name(text: &str) -> NormalizedName {
        NormalizedName::new(text).unwrap()
    }

    fn text(byte: u8, lines: &[&str]) -> Content {
        Content::Text {
            version: VersionId::from_bytes([byte; 32]),
            lines: lines.iter().map(|line| (*line).to_owned()).collect(),
        }
    }

    fn binary(byte: u8, length: u64) -> Content {
        Content::Binary {
            version: VersionId::from_bytes([byte; 32]),
            digest: [byte; 32],
            byte_length: length,
        }
    }

    fn root() -> ObjectId {
        ObjectId::from_bytes([0; 16])
    }

    fn notes() -> ObjectId {
        ObjectId::from_bytes([1; 16])
    }

    fn base() -> WorkspaceState {
        WorkspaceState::new(root())
            .with_directory(ObjectId::from_bytes([2; 16]), root(), name("archive"))
            .with_file(notes(), root(), name("notes.md"), text(7, &["one", "two"]))
    }

    fn presented(after: &WorkspaceState) -> DiffPresentation {
        present(&diff(&base(), after).unwrap())
    }

    #[test]
    fn a_rewritten_text_carries_hunks() {
        let after = base().with_file(notes(), root(), name("notes.md"), text(8, &["one", "TWO"]));
        let rendered = presented(&after);
        assert_eq!(rendered.len(), 1);
        let entry = &rendered.entries()[0];
        assert_eq!(entry.effect(), "content-written");
        assert_eq!(entry.body().label(), "text");
        assert_eq!(entry.hunks().len(), 1);
        assert_eq!(entry.hunks()[0].removed(), 1);
        assert_eq!(entry.hunks()[0].added(), 1);
        assert_eq!(entry.before().unwrap().line_count(), Some(2));
        assert_eq!(entry.after().unwrap().line_count(), Some(2));
    }

    #[test]
    fn a_rename_is_one_entry_with_both_paths_and_no_content_body() {
        let after = base().with_file(
            notes(),
            root(),
            name("journal.md"),
            text(7, &["one", "two"]),
        );
        let rendered = presented(&after);
        assert_eq!(rendered.len(), 1);
        let entry = &rendered.entries()[0];
        assert!(entry.is_rename());
        assert_eq!(entry.path_before(), Some("/notes.md"));
        assert_eq!(entry.path_after(), Some("/journal.md"));
        assert_eq!(entry.body(), &ChangeBody::Placement);
        assert!(entry.hunks().is_empty());
        assert_eq!(rendered.renames().len(), 1);
    }

    #[test]
    fn a_binary_change_is_size_and_hash_and_never_a_line() {
        let picture = ObjectId::from_bytes([3; 16]);
        let with_picture = base().with_file(picture, root(), name("cover.png"), binary(9, 2048));
        let rendered = present(&diff(&base(), &with_picture).unwrap());
        let entry = &rendered.entries()[0];
        assert_eq!(entry.body(), &ChangeBody::Binary);
        assert!(entry.hunks().is_empty());
        let summary = entry.after().unwrap();
        assert!(summary.is_binary());
        assert_eq!(summary.byte_length(), Some(2048));
        assert_eq!(summary.digest(), Some(Digest32::from_bytes([9; 32])));
        assert_eq!(summary.line_count(), None);
    }

    #[test]
    fn text_becoming_bytes_is_opaque_rather_than_a_rendered_line() {
        let after = base().with_file(notes(), root(), name("notes.md"), binary(9, 64));
        let rendered = presented(&after);
        let entry = &rendered.entries()[0];
        assert_eq!(
            entry.body(),
            &ChangeBody::Opaque {
                reason: OpaqueReason::ContentClassChanged
            }
        );
        assert!(entry.hunks().is_empty());
        assert_eq!(entry.before().unwrap().line_count(), Some(2));
        assert_eq!(entry.after().unwrap().byte_length(), Some(64));
    }

    #[test]
    fn a_text_above_the_ceiling_is_opaque_with_its_line_counts() {
        let long: Vec<String> = (0..=crate::text_diff::MAX_DIFF_LINES)
            .map(|at| at.to_string())
            .collect();
        let after = base().with_file(
            notes(),
            root(),
            name("notes.md"),
            Content::Text {
                version: VersionId::from_bytes([8; 32]),
                lines: long,
            },
        );
        let entry = &presented(&after).entries()[0].clone();
        assert_eq!(
            entry.body(),
            &ChangeBody::Opaque {
                reason: OpaqueReason::AboveLineCeiling
            }
        );
        assert_eq!(
            entry.after().unwrap().line_count(),
            Some(crate::text_diff::MAX_DIFF_LINES as u64 + 1)
        );
    }

    #[test]
    fn the_order_a_caller_holds_the_changes_in_changes_nothing() {
        let after = base()
            .with_file(notes(), root(), name("journal.md"), text(8, &["ONE"]))
            .with_file(
                ObjectId::from_bytes([4; 16]),
                root(),
                name("new.md"),
                text(9, &["fresh"]),
            );
        let changes = diff(&base(), &after).unwrap();
        let mut reversed = changes.clone();
        reversed.reverse();
        assert_eq!(present(&changes), present(&reversed));
        assert_eq!(present(&changes).digest(), present(&reversed).digest());
    }

    #[test]
    fn the_digest_moves_when_one_rendered_line_moves() {
        let one = base().with_file(notes(), root(), name("notes.md"), text(8, &["one", "TWO"]));
        let other = base().with_file(notes(), root(), name("notes.md"), text(8, &["one", "Two"]));
        assert_ne!(presented(&one).digest(), presented(&other).digest());
    }

    #[test]
    fn an_empty_change_list_renders_nothing() {
        let rendered = present(&[]);
        assert!(rendered.is_empty());
        assert_eq!(rendered.len(), 0);
        assert_eq!(rendered, DiffPresentation::default());
    }

    #[test]
    fn every_body_and_reason_has_a_label_and_a_distinct_rank() {
        let bodies = [
            ChangeBody::Placement,
            ChangeBody::Text { hunks: Vec::new() },
            ChangeBody::Binary,
            ChangeBody::Opaque {
                reason: OpaqueReason::AboveLineCeiling,
            },
        ];
        let ranks: Vec<u64> = bodies.iter().map(ChangeBody::rank).collect();
        assert_eq!(ranks, vec![0, 1, 2, 3]);
        for body in &bodies {
            assert!(!body.label().is_empty());
        }
        assert_ne!(
            OpaqueReason::AboveLineCeiling.rank(),
            OpaqueReason::ContentClassChanged.rank()
        );
        assert!(!OpaqueReason::ContentClassChanged.label().is_empty());
    }

    #[test]
    fn a_removed_text_is_all_removals_and_a_removed_binary_is_not() {
        let removed_text = presented(&base().without(notes()));
        assert_eq!(removed_text.entries()[0].body().label(), "text");
        assert_eq!(removed_text.entries()[0].hunks()[0].removed(), 2);

        let picture = ObjectId::from_bytes([3; 16]);
        let with_picture = base().with_file(picture, root(), name("cover.png"), binary(9, 8));
        let rendered =
            present(&diff(&with_picture, &with_picture.clone().without(picture)).unwrap());
        assert_eq!(rendered.entries()[0].body(), &ChangeBody::Binary);
        assert_eq!(
            rendered.entries()[0].before().unwrap().byte_length(),
            Some(8)
        );
    }
}
