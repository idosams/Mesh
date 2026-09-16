//! Three-way text merge — conflict rows four and five.
//!
//! # The rule, and the direction it errs in
//!
//! Row four says *attempt* a three-way merge for non-overlapping text changes; row five says
//! preserve multiple versions when they overlap. The word that carries the weight is *attempt*:
//! this module is allowed to fail, and failing costs a person one review. Merging two edits that
//! should not have been merged costs a person a file that says something neither of them wrote,
//! silently. So every judgement call below is resolved toward [`TextMerge::Overlapping`]:
//!
//! * two edits touching the same base lines overlap, even if a cleverer merge exists;
//! * two insertions at the same point overlap, because nothing in the operation set says which
//!   goes first and a guess would resolve differently on two peers;
//! * inputs above [`MAX_MERGE_LINES`] overlap by declaration rather than being diffed.
//!
//! Two *identical* concurrent edits are not a conflict: the same replacement of the same base
//! range is one edit that happened twice, and preserving it twice would surface a review a person
//! has nothing to decide in.
//!
//! # What "no version is lost" means for a clean merge
//!
//! A clean merge produces new content, and the caller writes it as a new version. The versions it
//! merged are still durable and still reachable —
//! [`Resolution::reachable_versions`](crate::Resolution::reachable_versions) carries them whether
//! the merge was clean or not. An automatic merge is not permission to forget the inputs.

/// The largest input this module will diff, in lines, per side.
///
/// The diff is a quadratic dynamic program. Above this the cost stops being bounded in a way a
/// merge inside an interactive review can absorb, so the answer becomes "preserve both" — which is
/// always a correct answer, just a less convenient one.
pub const MAX_MERGE_LINES: usize = 4096;

/// One contiguous replacement of a base line range.
///
/// `base_start == base_end` is an insertion at that point; an empty `replacement` is a deletion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    base_start: usize,
    base_end: usize,
    replacement: Vec<String>,
}

impl Hunk {
    /// The first base line this hunk replaces.
    #[must_use]
    pub const fn base_start(&self) -> usize {
        self.base_start
    }

    /// One past the last base line this hunk replaces.
    #[must_use]
    pub const fn base_end(&self) -> usize {
        self.base_end
    }

    /// What replaces those lines.
    #[must_use]
    pub fn replacement(&self) -> &[String] {
        &self.replacement
    }

    /// Whether this hunk inserts without replacing anything.
    #[must_use]
    pub const fn is_insertion(&self) -> bool {
        self.base_start == self.base_end
    }

    /// Whether this hunk and another cannot both be applied without a decision nothing records.
    #[must_use]
    pub fn overlaps(&self, other: &Self) -> bool {
        if self == other {
            return false;
        }
        match (self.is_insertion(), other.is_insertion()) {
            (true, true) => self.base_start == other.base_start,
            (true, false) => other.base_start < self.base_start && self.base_start < other.base_end,
            (false, true) => self.base_start < other.base_start && other.base_start < self.base_end,
            (false, false) => self.base_start < other.base_end && other.base_start < self.base_end,
        }
    }
}

/// A place two or more concurrent edits cannot be reconciled from the operation set alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverlapRegion {
    base_start: usize,
    base_end: usize,
    sides: Vec<usize>,
}

impl OverlapRegion {
    /// The first base line the overlap covers.
    #[must_use]
    pub const fn base_start(&self) -> usize {
        self.base_start
    }

    /// One past the last base line the overlap covers.
    #[must_use]
    pub const fn base_end(&self) -> usize {
        self.base_end
    }

    /// Which sides, by their index in the input, take part.
    #[must_use]
    pub fn sides(&self) -> &[usize] {
        &self.sides
    }
}

/// What a three-way merge produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextMerge {
    /// Every edit landed. The merged lines.
    Clean(Vec<String>),
    /// At least two edits could not both land. Where, and whose.
    Overlapping(Vec<OverlapRegion>),
}

/// The edits `side` makes to `base`, as a list of non-adjacent replacements in base order.
///
/// The diff is a longest-common-subsequence walk, so a hunk boundary falls where the two texts
/// agree — not where a line looks like a heading or a brace. Nothing here reads the content of a
/// line for anything but equality, which is what keeps the result the same for prose, for code and
/// for a file format nobody has invented yet.
#[must_use]
pub fn hunks(base: &[String], side: &[String]) -> Vec<Hunk> {
    let (rows, columns) = (base.len(), side.len());
    let width = columns + 1;
    let mut lengths = vec![0u32; (rows + 1) * width];
    for row in (0..rows).rev() {
        for column in (0..columns).rev() {
            let at = row * width + column;
            lengths[at] = if base[row] == side[column] {
                lengths[(row + 1) * width + column + 1] + 1
            } else {
                lengths[(row + 1) * width + column].max(lengths[row * width + column + 1])
            };
        }
    }

    let mut found = Vec::new();
    let mut pending: Option<Hunk> = None;
    let (mut row, mut column) = (0usize, 0usize);
    while row < rows || column < columns {
        let matched = row < rows && column < columns && base[row] == side[column];
        if matched {
            if let Some(hunk) = pending.take() {
                found.push(hunk);
            }
            row += 1;
            column += 1;
            continue;
        }
        let inserting = column < columns
            && (row == rows
                || lengths[row * width + column + 1] >= lengths[(row + 1) * width + column]);
        let hunk = pending.get_or_insert(Hunk {
            base_start: row,
            base_end: row,
            replacement: Vec::new(),
        });
        if inserting {
            hunk.replacement.push(side[column].clone());
            column += 1;
        } else {
            row += 1;
            hunk.base_end = row;
        }
    }
    if let Some(hunk) = pending.take() {
        found.push(hunk);
    }
    found
}

/// Merge every side against one base, or report where they cannot be merged.
///
/// Deterministic in the strong sense the task contract asks for: the output is a function of the
/// base and the multiset of sides, and reordering the sides changes only the side indices reported
/// inside an [`OverlapRegion`], never whether the merge was clean and never the merged lines.
#[must_use]
pub fn three_way(base: &[String], sides: &[&[String]]) -> TextMerge {
    if base.len() > MAX_MERGE_LINES || sides.iter().any(|side| side.len() > MAX_MERGE_LINES) {
        return TextMerge::Overlapping(vec![OverlapRegion {
            base_start: 0,
            base_end: base.len(),
            sides: (0..sides.len()).collect(),
        }]);
    }

    let per_side: Vec<Vec<Hunk>> = sides.iter().map(|side| hunks(base, side)).collect();

    let mut regions = Vec::new();
    for (left_index, left_hunks) in per_side.iter().enumerate() {
        for (right_index, right_hunks) in per_side.iter().enumerate().skip(left_index + 1) {
            for left in left_hunks {
                for right in right_hunks {
                    if left.overlaps(right) {
                        regions.push(OverlapRegion {
                            base_start: left.base_start.min(right.base_start),
                            base_end: left.base_end.max(right.base_end),
                            sides: vec![left_index, right_index],
                        });
                    }
                }
            }
        }
    }
    if !regions.is_empty() {
        regions.sort_by_key(|region| (region.base_start, region.base_end, region.sides.clone()));
        regions.dedup();
        return TextMerge::Overlapping(regions);
    }

    let mut ordered: Vec<Hunk> = per_side.into_iter().flatten().collect();
    ordered.sort_by(|left, right| {
        (left.base_start, left.base_end, &left.replacement).cmp(&(
            right.base_start,
            right.base_end,
            &right.replacement,
        ))
    });
    ordered.dedup();

    let mut merged = Vec::new();
    let mut cursor = 0usize;
    for hunk in &ordered {
        if hunk.base_start < cursor {
            return TextMerge::Overlapping(vec![OverlapRegion {
                base_start: hunk.base_start,
                base_end: hunk.base_end,
                sides: (0..sides.len()).collect(),
            }]);
        }
        merged.extend_from_slice(&base[cursor..hunk.base_start]);
        merged.extend(hunk.replacement.iter().cloned());
        cursor = hunk.base_end;
    }
    merged.extend_from_slice(&base[cursor..]);
    TextMerge::Clean(merged)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &[&str]) -> Vec<String> {
        text.iter().map(|line| (*line).to_owned()).collect()
    }

    #[test]
    fn an_unchanged_side_produces_no_hunk() {
        let base = lines(&["a", "b", "c"]);
        assert!(hunks(&base, &base).is_empty());
    }

    #[test]
    fn a_replacement_is_one_hunk_over_the_replaced_range() {
        let base = lines(&["a", "b", "c"]);
        let side = lines(&["a", "B", "c"]);
        let found = hunks(&base, &side);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].base_start(), found[0].base_end()), (1, 2));
        assert_eq!(found[0].replacement(), &["B".to_owned()]);
    }

    #[test]
    fn an_insertion_is_an_empty_base_range() {
        let base = lines(&["a", "c"]);
        let side = lines(&["a", "b", "c"]);
        let found = hunks(&base, &side);
        assert_eq!(found.len(), 1);
        assert!(found[0].is_insertion());
        assert_eq!(found[0].replacement(), &["b".to_owned()]);
    }

    #[test]
    fn a_deletion_is_an_empty_replacement() {
        let base = lines(&["a", "b", "c"]);
        let side = lines(&["a", "c"]);
        let found = hunks(&base, &side);
        assert_eq!(found.len(), 1);
        assert!(found[0].replacement().is_empty());
        assert_eq!((found[0].base_start(), found[0].base_end()), (1, 2));
    }

    #[test]
    fn distant_edits_merge_into_one_text() {
        let base = lines(&["a", "b", "c", "d", "e"]);
        let left = lines(&["A", "b", "c", "d", "e"]);
        let right = lines(&["a", "b", "c", "d", "E"]);
        assert_eq!(
            three_way(&base, &[&left, &right]),
            TextMerge::Clean(lines(&["A", "b", "c", "d", "E"]))
        );
    }

    #[test]
    fn edits_to_the_same_line_do_not_merge() {
        let base = lines(&["a", "b", "c"]);
        let left = lines(&["a", "LEFT", "c"]);
        let right = lines(&["a", "RIGHT", "c"]);
        let merged = three_way(&base, &[&left, &right]);
        let TextMerge::Overlapping(regions) = merged else {
            panic!("two edits to line 1 merged: {merged:?}");
        };
        assert_eq!(regions[0].sides(), &[0, 1]);
    }

    #[test]
    fn two_insertions_at_one_point_do_not_merge() {
        let base = lines(&["a", "z"]);
        let left = lines(&["a", "left", "z"]);
        let right = lines(&["a", "right", "z"]);
        assert!(matches!(
            three_way(&base, &[&left, &right]),
            TextMerge::Overlapping(_)
        ));
    }

    #[test]
    fn an_identical_edit_made_twice_is_not_a_conflict() {
        let base = lines(&["a", "b", "c"]);
        let side = lines(&["a", "SAME", "c"]);
        assert_eq!(
            three_way(&base, &[&side, &side]),
            TextMerge::Clean(lines(&["a", "SAME", "c"]))
        );
    }

    #[test]
    fn a_deletion_concurrent_with_an_edit_of_the_same_line_does_not_merge() {
        let base = lines(&["a", "b", "c"]);
        let deleted = lines(&["a", "c"]);
        let edited = lines(&["a", "B", "c"]);
        assert!(matches!(
            three_way(&base, &[&deleted, &edited]),
            TextMerge::Overlapping(_)
        ));
    }

    #[test]
    fn side_order_changes_nothing_about_the_merged_text() {
        let base = lines(&["a", "b", "c", "d"]);
        let left = lines(&["A", "b", "c", "d"]);
        let right = lines(&["a", "b", "c", "D"]);
        assert_eq!(
            three_way(&base, &[&left, &right]),
            three_way(&base, &[&right, &left])
        );
    }

    #[test]
    fn three_sides_all_land_when_none_of_them_touch() {
        let base = lines(&["1", "2", "3", "4", "5", "6", "7"]);
        let first = lines(&["ONE", "2", "3", "4", "5", "6", "7"]);
        let second = lines(&["1", "2", "3", "FOUR", "5", "6", "7"]);
        let third = lines(&["1", "2", "3", "4", "5", "6", "SEVEN"]);
        assert_eq!(
            three_way(&base, &[&first, &second, &third]),
            TextMerge::Clean(lines(&["ONE", "2", "3", "FOUR", "5", "6", "SEVEN"]))
        );
    }

    #[test]
    fn an_input_above_the_ceiling_is_preserved_rather_than_diffed() {
        let base: Vec<String> = (0..=MAX_MERGE_LINES).map(|at| at.to_string()).collect();
        let side = base.clone();
        assert!(matches!(
            three_way(&base, &[&side]),
            TextMerge::Overlapping(_)
        ));
    }

    #[test]
    fn a_single_side_merges_to_itself() {
        let base = lines(&["a", "b"]);
        let side = lines(&["a", "b", "c"]);
        assert_eq!(three_way(&base, &[&side]), TextMerge::Clean(side));
    }
}
