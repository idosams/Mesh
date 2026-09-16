//! The line-level body of a text change: hunks, with context, computed from the two versions.
//!
//! # What this module is for
//!
//! [`crate::diff`] answers *which object changed and how* — a rename is a rename, a rewrite is a
//! rewrite. It does not answer *which lines*. A reviewer approving a rewrite of a 400-line file
//! needs the four lines that moved, not the 400 that did not, so this module turns a pair of
//! [`Content::Text`](crate::Content::Text) versions into [`TextHunk`]s: runs of changed lines, each
//! carrying [`CONTEXT_LINES`] unchanged lines on either side.
//!
//! # The algorithm, and why it is this one
//!
//! A longest-common-subsequence walk, line by line, comparing lines only for equality. It is the
//! same walk `mesh-conflicts`' `hunks` performs, transcribed rather than called, for the reason
//! [`crate::ids`] gives: this crate declares no dependency, so a shared judgement is mirrored and
//! held by `tests/mesh_conflicts_drift.rs`. Transcribing it rather than inventing a second one is
//! deliberate: the merge engine and the review surface must agree about where a hunk boundary
//! falls, or a person approves a boundary the merge does not honour.
//!
//! Nothing here reads the *content* of a line for anything but equality — no heading detection, no
//! brace matching, no language awareness. That is what keeps the answer the same for prose, for
//! code, and for a file format nobody has invented yet.
//!
//! # The ceiling, inherited rather than re-decided
//!
//! [`MAX_DIFF_LINES`] is `mesh-conflicts`' `MAX_MERGE_LINES`, mirrored at the same value for the
//! same reason: the walk is a quadratic dynamic program, and above that width its cost stops being
//! something an interactive review can absorb. Above the ceiling this module returns [`None`] and
//! the caller presents the change opaquely, with its metadata — which is the task's
//! failure-and-recovery clause, and is always a correct answer, just a less convenient one.
//!
//! # Determinism
//!
//! Every value here is a function of the two line slices. No clock, no environment, no hash map,
//! no allocation-address-dependent ordering. `tests/diff.rs` re-executes its own binary in a second
//! process and requires the whole rendered byte stream to match.

use crate::digest::{Absorb, DigestHasher, DigestWriter};

/// The largest text this module will diff, in lines, per side.
///
/// Mirrors `mesh_conflicts::MAX_MERGE_LINES`. The two constants are held equal by
/// `tests/mesh_conflicts_drift.rs`, because a review surface that diffed a file the merge engine
/// refuses to diff would show a person a resolution nothing downstream can honour.
pub const MAX_DIFF_LINES: usize = 4096;

/// How many unchanged lines are shown on each side of a run of changed lines.
///
/// Three, the long-standing default of every unified diff a reviewer has ever read. It is part of
/// the rendered bytes, so changing it changes [`crate::DiffPresentation::digest`] for every diff
/// that has any context at all — a presentation change, not a preference.
pub const CONTEXT_LINES: usize = 3;

/// One line as a reviewer sees it, with the line numbers it carries on each side.
///
/// Line numbers are 1-based, because they are read by a person next to an editor that counts from
/// one. A removed line has no number in the later version and an added line has none in the
/// earlier one, which is why they are separate variants rather than two `Option` fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffLine {
    /// Present in both versions, unchanged. Shown for context only.
    Context {
        /// Its 1-based number in the earlier version.
        before: usize,
        /// Its 1-based number in the later version.
        after: usize,
        /// The line.
        text: String,
    },
    /// Present in the earlier version and not in the later one.
    Removed {
        /// Its 1-based number in the earlier version.
        before: usize,
        /// The line.
        text: String,
    },
    /// Present in the later version and not in the earlier one.
    Added {
        /// Its 1-based number in the later version.
        after: usize,
        /// The line.
        text: String,
    },
}

impl DiffLine {
    /// The line itself, newline excluded.
    #[must_use]
    pub fn text(&self) -> &str {
        match self {
            Self::Context { text, .. } | Self::Removed { text, .. } | Self::Added { text, .. } => {
                text
            }
        }
    }

    /// A short, stable word a surface can render or key a style off.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Context { .. } => "context",
            Self::Removed { .. } => "removed",
            Self::Added { .. } => "added",
        }
    }

    /// The tag this line is absorbed under, and the order the three kinds sort in.
    #[must_use]
    pub const fn rank(&self) -> u64 {
        match self {
            Self::Context { .. } => 0,
            Self::Removed { .. } => 1,
            Self::Added { .. } => 2,
        }
    }
}

impl Absorb for DiffLine {
    fn absorb<H: DigestHasher>(&self, writer: &mut DigestWriter<H>) {
        writer.u64(self.rank());
        match self {
            Self::Context {
                before,
                after,
                text,
            } => {
                writer.u64(*before as u64);
                writer.u64(*after as u64);
                writer.text(text);
            }
            Self::Removed { before, text } => {
                writer.u64(*before as u64);
                writer.text(text);
            }
            Self::Added { after, text } => {
                writer.u64(*after as u64);
                writer.text(text);
            }
        }
    }
}

/// One run of changed lines together with the unchanged lines around it.
///
/// The four numbers are the header a unified diff writes as `@@ -before_start,before_len
/// +after_start,after_len @@`, carried as numbers rather than as text so that a surface renders
/// them and this crate never has to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextHunk {
    before_start: usize,
    before_len: usize,
    after_start: usize,
    after_len: usize,
    lines: Vec<DiffLine>,
}

impl TextHunk {
    /// The 1-based number of the first earlier-version line this hunk covers.
    ///
    /// When the hunk covers none — the earlier version was empty — this is the position the change
    /// sits at, which is `1`.
    #[must_use]
    pub const fn before_start(&self) -> usize {
        self.before_start
    }

    /// How many earlier-version lines this hunk covers, context included.
    #[must_use]
    pub const fn before_len(&self) -> usize {
        self.before_len
    }

    /// The 1-based number of the first later-version line this hunk covers.
    #[must_use]
    pub const fn after_start(&self) -> usize {
        self.after_start
    }

    /// How many later-version lines this hunk covers, context included.
    #[must_use]
    pub const fn after_len(&self) -> usize {
        self.after_len
    }

    /// The lines, in reading order: context, then removals, then additions, then context.
    #[must_use]
    pub fn lines(&self) -> &[DiffLine] {
        &self.lines
    }

    /// How many lines this hunk removes.
    #[must_use]
    pub fn removed(&self) -> usize {
        self.lines
            .iter()
            .filter(|line| matches!(line, DiffLine::Removed { .. }))
            .count()
    }

    /// How many lines this hunk adds.
    #[must_use]
    pub fn added(&self) -> usize {
        self.lines
            .iter()
            .filter(|line| matches!(line, DiffLine::Added { .. }))
            .count()
    }
}

impl Absorb for TextHunk {
    fn absorb<H: DigestHasher>(&self, writer: &mut DigestWriter<H>) {
        writer.u64(self.before_start as u64);
        writer.u64(self.before_len as u64);
        writer.u64(self.after_start as u64);
        writer.u64(self.after_len as u64);
        writer.sequence(&self.lines, |writer, line| line.absorb(writer));
    }
}

/// One contiguous replacement: earlier lines `before_start..before_end` became later lines
/// `after_start..after_end`. Half-open, 0-based, and internal — the presented form is [`TextHunk`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Edit {
    before_start: usize,
    before_end: usize,
    after_start: usize,
    after_end: usize,
}

/// The hunks between two texts, or [`None`] when either side exceeds [`MAX_DIFF_LINES`].
///
/// An empty vector means the two texts are identical — a distinct answer from [`None`], which means
/// this module declined to look.
#[must_use]
pub fn text_hunks(before: &[String], after: &[String]) -> Option<Vec<TextHunk>> {
    if before.len() > MAX_DIFF_LINES || after.len() > MAX_DIFF_LINES {
        return None;
    }
    let edits = edits(before, after);
    Some(
        group(&edits)
            .iter()
            .map(|run| render(run, before, after))
            .collect(),
    )
}

/// The replacements that turn `before` into `after`, in earlier-version order.
///
/// The longest-common-subsequence walk transcribed from `mesh-conflicts`' `hunks`, extended to
/// carry the later-version range alongside the earlier one so a hunk can be numbered on both sides.
fn edits(before: &[String], after: &[String]) -> Vec<Edit> {
    let (rows, columns) = (before.len(), after.len());
    let width = columns + 1;
    let mut lengths = vec![0u32; (rows + 1) * width];
    for row in (0..rows).rev() {
        for column in (0..columns).rev() {
            let at = row * width + column;
            lengths[at] = if before[row] == after[column] {
                lengths[(row + 1) * width + column + 1] + 1
            } else {
                lengths[(row + 1) * width + column].max(lengths[row * width + column + 1])
            };
        }
    }

    let mut found = Vec::new();
    let mut pending: Option<Edit> = None;
    let (mut row, mut column) = (0usize, 0usize);
    while row < rows || column < columns {
        if row < rows && column < columns && before[row] == after[column] {
            if let Some(edit) = pending.take() {
                found.push(edit);
            }
            row += 1;
            column += 1;
            continue;
        }
        let inserting = column < columns
            && (row == rows
                || lengths[row * width + column + 1] >= lengths[(row + 1) * width + column]);
        let edit = pending.get_or_insert(Edit {
            before_start: row,
            before_end: row,
            after_start: column,
            after_end: column,
        });
        if inserting {
            column += 1;
            edit.after_end = column;
        } else {
            row += 1;
            edit.before_end = row;
        }
    }
    if let Some(edit) = pending.take() {
        found.push(edit);
    }
    found
}

/// Edits whose context windows touch or overlap, grouped into one hunk each.
///
/// Two edits separated by at most `2 * CONTEXT_LINES` unchanged lines would otherwise be rendered
/// as two hunks that repeat the same lines between them, which reads as two changes where there is
/// one region.
fn group(edits: &[Edit]) -> Vec<Vec<Edit>> {
    let mut groups: Vec<Vec<Edit>> = Vec::new();
    for edit in edits {
        let joins = groups.last().is_some_and(|run: &Vec<Edit>| {
            run.last()
                .is_some_and(|last| edit.before_start - last.before_end <= 2 * CONTEXT_LINES)
        });
        if joins {
            groups
                .last_mut()
                .expect("a group exists when one joins it")
                .push(*edit);
        } else {
            groups.push(vec![*edit]);
        }
    }
    groups
}

/// One group of edits rendered as a hunk with its context.
fn render(run: &[Edit], before: &[String], after: &[String]) -> TextHunk {
    let first = run.first().expect("a group holds at least one edit");
    let last = run.last().expect("a group holds at least one edit");

    let context_start = first.before_start.saturating_sub(CONTEXT_LINES);
    let context_end = (last.before_end + CONTEXT_LINES).min(before.len());
    // The lines between an edit and its context match one for one on both sides, so the later
    // version's window is the earlier window's offsets carried across.
    let after_context_start = first.after_start - (first.before_start - context_start);
    let after_context_end = last.after_end + (context_end - last.before_end);

    let mut lines = Vec::new();
    let mut at_before = context_start;
    let mut at_after = after_context_start;
    for edit in run {
        while at_before < edit.before_start {
            lines.push(DiffLine::Context {
                before: at_before + 1,
                after: at_after + 1,
                text: before[at_before].clone(),
            });
            at_before += 1;
            at_after += 1;
        }
        for (offset, text) in before[edit.before_start..edit.before_end]
            .iter()
            .enumerate()
        {
            lines.push(DiffLine::Removed {
                before: edit.before_start + offset + 1,
                text: text.clone(),
            });
        }
        for (offset, text) in after[edit.after_start..edit.after_end].iter().enumerate() {
            lines.push(DiffLine::Added {
                after: edit.after_start + offset + 1,
                text: text.clone(),
            });
        }
        at_before = edit.before_end;
        at_after = edit.after_end;
    }
    while at_before < context_end {
        lines.push(DiffLine::Context {
            before: at_before + 1,
            after: at_after + 1,
            text: before[at_before].clone(),
        });
        at_before += 1;
        at_after += 1;
    }

    TextHunk {
        before_start: context_start + 1,
        before_len: context_end - context_start,
        after_start: after_context_start + 1,
        after_len: after_context_end - after_context_start,
        lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &[&str]) -> Vec<String> {
        text.iter().map(|line| (*line).to_owned()).collect()
    }

    /// Rebuild the later version from the earlier one and the hunks. The property every other test
    /// in this module leans on: hunks that cannot be replayed are a picture, not a diff.
    fn replay(before: &[String], hunks: &[TextHunk]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut cursor = 0usize;
        for hunk in hunks {
            for line in &hunk.lines {
                match line {
                    DiffLine::Context {
                        before: at, text, ..
                    } => {
                        while cursor + 1 < *at {
                            out.push(before[cursor].clone());
                            cursor += 1;
                        }
                        out.push(text.clone());
                        cursor += 1;
                    }
                    DiffLine::Removed { .. } => cursor += 1,
                    DiffLine::Added { text, .. } => out.push(text.clone()),
                }
            }
        }
        out.extend_from_slice(&before[cursor..]);
        out
    }

    /// The earlier-version number a line carries, when it carries one.
    fn before_number(line: &DiffLine) -> Option<usize> {
        match line {
            DiffLine::Context { before, .. } | DiffLine::Removed { before, .. } => Some(*before),
            DiffLine::Added { .. } => None,
        }
    }

    #[test]
    fn two_identical_texts_produce_no_hunk() {
        let text = lines(&["a", "b", "c"]);
        assert_eq!(text_hunks(&text, &text), Some(Vec::new()));
    }

    #[test]
    fn a_replaced_line_is_one_removal_and_one_addition_with_context() {
        let before = lines(&["a", "b", "c"]);
        let after = lines(&["a", "B", "c"]);
        let hunks = text_hunks(&before, &after).unwrap();
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].removed(), 1);
        assert_eq!(hunks[0].added(), 1);
        assert_eq!(hunks[0].before_start(), 1);
        assert_eq!(hunks[0].before_len(), 3);
        assert_eq!(hunks[0].after_start(), 1);
        assert_eq!(hunks[0].after_len(), 3);
        assert_eq!(replay(&before, &hunks), after);
    }

    #[test]
    fn context_is_bounded_at_three_lines_on_each_side() {
        let before: Vec<String> = (0..40).map(|at| at.to_string()).collect();
        let mut after = before.clone();
        after[20] = "changed".to_owned();
        let hunks = text_hunks(&before, &after).unwrap();
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].lines().len(), CONTEXT_LINES * 2 + 2);
        assert_eq!(hunks[0].before_start(), 18);
        assert_eq!(hunks[0].before_len(), CONTEXT_LINES * 2 + 1);
        assert_eq!(replay(&before, &hunks), after);
    }

    #[test]
    fn two_distant_changes_are_two_hunks_and_two_near_ones_are_one() {
        let before: Vec<String> = (0..60).map(|at| at.to_string()).collect();
        let mut distant = before.clone();
        distant[5] = "x".to_owned();
        distant[50] = "y".to_owned();
        assert_eq!(text_hunks(&before, &distant).unwrap().len(), 2);

        let mut near = before.clone();
        near[5] = "x".to_owned();
        near[10] = "y".to_owned();
        let hunks = text_hunks(&before, &near).unwrap();
        assert_eq!(hunks.len(), 1);
        assert_eq!(replay(&before, &hunks), near);
    }

    #[test]
    fn an_insertion_into_an_empty_text_is_all_additions() {
        let before: Vec<String> = Vec::new();
        let after = lines(&["one", "two"]);
        let hunks = text_hunks(&before, &after).unwrap();
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].added(), 2);
        assert_eq!(hunks[0].removed(), 0);
        assert_eq!(hunks[0].before_len(), 0);
        assert_eq!(hunks[0].before_start(), 1);
        assert_eq!(hunks[0].after_len(), 2);
        assert_eq!(replay(&before, &hunks), after);
    }

    #[test]
    fn clearing_a_text_is_all_removals() {
        let before = lines(&["one", "two"]);
        let after: Vec<String> = Vec::new();
        let hunks = text_hunks(&before, &after).unwrap();
        assert_eq!(hunks[0].removed(), 2);
        assert_eq!(hunks[0].added(), 0);
        assert_eq!(hunks[0].after_len(), 0);
        assert_eq!(replay(&before, &hunks), after);
    }

    #[test]
    fn line_numbers_ascend_within_a_hunk_on_the_earlier_side() {
        let before: Vec<String> = (0..20).map(|at| at.to_string()).collect();
        let mut after = before.clone();
        after.remove(9);
        after.insert(9, "replacement".to_owned());
        let hunks = text_hunks(&before, &after).unwrap();
        let numbers: Vec<usize> = hunks[0].lines().iter().filter_map(before_number).collect();
        assert!(numbers.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(numbers.first(), Some(&hunks[0].before_start()));
    }

    #[test]
    fn a_text_above_the_ceiling_is_not_diffed() {
        let long: Vec<String> = (0..=MAX_DIFF_LINES).map(|at| at.to_string()).collect();
        let short = lines(&["a"]);
        assert_eq!(text_hunks(&long, &short), None);
        assert_eq!(text_hunks(&short, &long), None);
    }

    #[test]
    fn a_text_at_the_ceiling_is_still_diffed() {
        let before: Vec<String> = (0..MAX_DIFF_LINES).map(|at| at.to_string()).collect();
        let mut after = before.clone();
        after[0] = "changed".to_owned();
        let hunks = text_hunks(&before, &after).unwrap();
        assert_eq!(hunks.len(), 1);
        assert_eq!(replay(&before, &hunks), after);
    }

    #[test]
    fn every_line_kind_has_a_label_and_a_distinct_rank() {
        let sample = [
            DiffLine::Context {
                before: 1,
                after: 1,
                text: "a".to_owned(),
            },
            DiffLine::Removed {
                before: 2,
                text: "b".to_owned(),
            },
            DiffLine::Added {
                after: 2,
                text: "c".to_owned(),
            },
        ];
        let ranks: Vec<u64> = sample.iter().map(DiffLine::rank).collect();
        assert_eq!(ranks, vec![0, 1, 2]);
        for line in &sample {
            assert!(!line.label().is_empty());
            assert!(!line.text().is_empty());
        }
    }
}
