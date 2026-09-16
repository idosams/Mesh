//! The conflict table itself — plan §4.8, transcribed rather than re-derived.
//!
//! Eleven rows. Each is a [`Rule`], each carries the plan's own two columns verbatim, and
//! `tests/conflicts.rs` has one targeted test per row that fails if the row's behaviour is removed.
//!
//! Two columns are added here that the plan states in prose elsewhere and that a caller needs
//! mechanically:
//!
//! * [`Rule::preserves_every_version`] — whether the row can be satisfied without keeping every
//!   durable version reachable. Ten rows say no. Row nine says yes, and says it for a reason worth
//!   reading: a refused directory move drops no content, so nothing it decides can lose an actor's
//!   work.
//! * [`Rule::needs_review`] — whether the row's outcome is one a person has to look at. Plan §2.5
//!   is that no valid work is silently discarded; the surface that makes it true is this flag,
//!   because work preserved somewhere nobody is told about is discarded in every sense that
//!   matters to the person who did it.

use core::fmt;

/// One row of plan §4.8.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rule {
    /// Rename + edit same object → edit follows stable object ID.
    EditFollowsIdentity,
    /// Move directory + child edit → child remains attached to object graph.
    ChildStaysAttached,
    /// Independent files → merge automatically.
    IndependentFilesMerge,
    /// Non-overlapping text changes → attempt three-way merge.
    ThreeWayTextMerge,
    /// Overlapping text changes → preserve multiple versions.
    PreserveOverlappingText,
    /// Binary changes → preserve both versions.
    PreserveBinaryVersions,
    /// Delete + edit → preserve tombstone and edited version.
    PreserveTombstoneAndEdit,
    /// Same-name create → retain both object IDs; expose naming conflict.
    RetainBothIdentities,
    /// Concurrent cyclic directory moves → deterministic cycle-free resolution.
    DeterministicCycleBreak,
    /// Canonical head changed after review → replay safely or require re-review.
    HeadMovedAfterReview,
    /// Context input changed → mark affected outputs stale.
    ContextInputChanged,
}

impl Rule {
    /// Every row of the table, in the order plan §4.8 prints them.
    pub const TABLE: [Self; 11] = [
        Self::EditFollowsIdentity,
        Self::ChildStaysAttached,
        Self::IndependentFilesMerge,
        Self::ThreeWayTextMerge,
        Self::PreserveOverlappingText,
        Self::PreserveBinaryVersions,
        Self::PreserveTombstoneAndEdit,
        Self::RetainBothIdentities,
        Self::DeterministicCycleBreak,
        Self::HeadMovedAfterReview,
        Self::ContextInputChanged,
    ];

    /// The plan's left-hand column: the concurrent operations this row is about.
    #[must_use]
    pub const fn concurrent_operations(&self) -> &'static str {
        match self {
            Self::EditFollowsIdentity => "Rename + edit same object",
            Self::ChildStaysAttached => "Move directory + child edit",
            Self::IndependentFilesMerge => "Independent files",
            Self::ThreeWayTextMerge => "Non-overlapping text changes",
            Self::PreserveOverlappingText => "Overlapping text changes",
            Self::PreserveBinaryVersions => "Binary changes",
            Self::PreserveTombstoneAndEdit => "Delete + edit",
            Self::RetainBothIdentities => "Same-name create",
            Self::DeterministicCycleBreak => "Concurrent cyclic directory moves",
            Self::HeadMovedAfterReview => "Canonical head changed after review",
            Self::ContextInputChanged => "Context input changed",
        }
    }

    /// The plan's right-hand column: what this row resolves to.
    #[must_use]
    pub const fn result(&self) -> &'static str {
        match self {
            Self::EditFollowsIdentity => "Edit follows stable object ID",
            Self::ChildStaysAttached => "Child remains attached to object graph",
            Self::IndependentFilesMerge => "Merge automatically",
            Self::ThreeWayTextMerge => "Attempt three-way merge",
            Self::PreserveOverlappingText => "Preserve multiple versions",
            Self::PreserveBinaryVersions => "Preserve both versions",
            Self::PreserveTombstoneAndEdit => "Preserve tombstone and edited version",
            Self::RetainBothIdentities => "Retain both object IDs; expose naming conflict",
            Self::DeterministicCycleBreak => "Deterministic cycle-free resolution",
            Self::HeadMovedAfterReview => "Replay safely or require re-review",
            Self::ContextInputChanged => "Mark affected outputs stale",
        }
    }

    /// Whether satisfying this row requires every durable version to stay reachable.
    ///
    /// True for every row that decides anything about content. False only for
    /// [`Rule::DeterministicCycleBreak`], which decides where an object hangs and never what it
    /// holds.
    #[must_use]
    pub const fn preserves_every_version(&self) -> bool {
        !matches!(self, Self::DeterministicCycleBreak)
    }

    /// Whether this row's outcome is one a person has to look at before it can be published.
    #[must_use]
    pub const fn needs_review(&self) -> bool {
        matches!(
            self,
            Self::PreserveOverlappingText
                | Self::PreserveBinaryVersions
                | Self::PreserveTombstoneAndEdit
                | Self::RetainBothIdentities
                | Self::DeterministicCycleBreak
                | Self::HeadMovedAfterReview
                | Self::ContextInputChanged
        )
    }

    /// Whether this row's outcome is reached with no human involvement at all.
    #[must_use]
    pub const fn is_automatic(&self) -> bool {
        !self.needs_review()
    }
}

impl fmt::Display for Rule {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} → {}",
            self.concurrent_operations(),
            self.result()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_has_the_plan_s_eleven_rows_and_no_duplicates() {
        assert_eq!(Rule::TABLE.len(), 11);
        let mut left: Vec<&str> = Rule::TABLE
            .iter()
            .map(Rule::concurrent_operations)
            .collect();
        left.sort_unstable();
        left.dedup();
        assert_eq!(left.len(), 11);
    }

    #[test]
    fn four_rows_resolve_with_no_human_involvement() {
        let automatic: Vec<Rule> = Rule::TABLE.into_iter().filter(Rule::is_automatic).collect();
        assert_eq!(
            automatic,
            vec![
                Rule::EditFollowsIdentity,
                Rule::ChildStaysAttached,
                Rule::IndependentFilesMerge,
                Rule::ThreeWayTextMerge,
            ]
        );
    }

    #[test]
    fn only_the_cycle_break_is_exempt_from_the_preservation_promise() {
        let exempt: Vec<Rule> = Rule::TABLE
            .into_iter()
            .filter(|rule| !rule.preserves_every_version())
            .collect();
        assert_eq!(exempt, vec![Rule::DeterministicCycleBreak]);
    }

    #[test]
    fn a_row_prints_both_of_the_plan_s_columns() {
        assert_eq!(
            Rule::PreserveBinaryVersions.to_string(),
            "Binary changes → Preserve both versions"
        );
    }
}
