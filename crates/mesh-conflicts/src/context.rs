//! Conflict row eleven: a context input changed, so its outputs are stale.
//!
//! # The row exists because agents are not people
//!
//! An agent read four files and wrote a fifth. If one of the four moves on, the fifth is a
//! statement about a version of the workspace that no longer exists. Nothing about it is corrupt,
//! and nothing about it is wrong to keep — it is *stale*, which is a claim about its currency and
//! not about its validity. Plan §4.9's read ledger is what records the four; this module is what
//! turns a version change into a verdict about the fifth.
//!
//! # Staleness is transitive, and closing over it is the point
//!
//! An output built from a stale output is stale. Computing only the direct hit would mark one file
//! in a chain of four and leave a person believing the other three are current, which is a worse
//! answer than not checking at all. [`ContextLedger::stale_outputs`] runs the closure to a
//! fixpoint.
//!
//! # Marking, never deleting
//!
//! Nothing here removes an output, and nothing here removes a version. The entire vocabulary of
//! this module is one set of identifiers a review bundle can badge.

use std::collections::{BTreeMap, BTreeSet};

use crate::ids::{ObjectId, VersionId};
use crate::rules::Rule;

/// Which versions of which objects an output was built from.
///
/// A projection of plan §4.9's `ReadObservation` down to the two fields staleness needs. The
/// region and confidence fields it drops are what would let a finer rule say "this output read
/// lines 1–10 and only line 400 changed" — a rule this crate deliberately does not have, because
/// getting it wrong marks work current that is not.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContextLedger {
    reads: BTreeMap<ObjectId, BTreeMap<ObjectId, VersionId>>,
}

impl ContextLedger {
    /// An empty ledger.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// This ledger with one recorded read.
    #[must_use]
    pub fn with_read(self, output: ObjectId, input: ObjectId, version: VersionId) -> Self {
        let mut reads = self.reads;
        reads.entry(output).or_default().insert(input, version);
        Self { reads }
    }

    /// Every output this ledger knows about, in identifier order.
    pub fn outputs(&self) -> impl Iterator<Item = ObjectId> + '_ {
        self.reads.keys().copied()
    }

    /// What one output read.
    #[must_use]
    pub fn inputs_of(&self, output: ObjectId) -> Option<&BTreeMap<ObjectId, VersionId>> {
        self.reads.get(&output)
    }

    /// Every output that is no longer a statement about the current workspace.
    ///
    /// An output is stale when any input it read now holds a different version, when an input it
    /// read is no longer present at all, or when any input it read is itself a stale output.
    ///
    /// The missing-input case is the one worth stating: an input that was removed has *changed* in
    /// every sense the reader cares about, and treating absence as "unchanged" would mark an
    /// output current precisely when its ground truth is gone.
    #[must_use]
    pub fn stale_outputs(&self, current: &BTreeMap<ObjectId, VersionId>) -> BTreeSet<ObjectId> {
        let mut stale: BTreeSet<ObjectId> = self
            .reads
            .iter()
            .filter(|(_, inputs)| {
                inputs
                    .iter()
                    .any(|(input, read)| current.get(input) != Some(read))
            })
            .map(|(output, _)| *output)
            .collect();

        loop {
            let grown: BTreeSet<ObjectId> = self
                .reads
                .iter()
                .filter(|(output, inputs)| {
                    !stale.contains(*output) && inputs.keys().any(|input| stale.contains(input))
                })
                .map(|(output, _)| *output)
                .collect();
            if grown.is_empty() {
                return stale;
            }
            stale.extend(grown);
        }
    }

    /// The row of plan §4.8 this ledger implements.
    #[must_use]
    pub const fn rule(&self) -> Rule {
        Rule::ContextInputChanged
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(byte: u8) -> ObjectId {
        ObjectId::from_bytes([byte; 16])
    }

    fn version(byte: u8) -> VersionId {
        VersionId::from_bytes([byte; 32])
    }

    fn current(pairs: &[(u8, u8)]) -> BTreeMap<ObjectId, VersionId> {
        pairs
            .iter()
            .map(|(id, at)| (object(*id), version(*at)))
            .collect()
    }

    #[test]
    fn an_unchanged_input_leaves_its_output_current() {
        let ledger = ContextLedger::new().with_read(object(9), object(1), version(1));
        assert!(ledger.stale_outputs(&current(&[(1, 1)])).is_empty());
    }

    #[test]
    fn a_changed_input_marks_its_output_stale() {
        let ledger = ContextLedger::new().with_read(object(9), object(1), version(1));
        assert_eq!(
            ledger.stale_outputs(&current(&[(1, 2)])),
            BTreeSet::from([object(9)])
        );
    }

    #[test]
    fn a_removed_input_marks_its_output_stale() {
        let ledger = ContextLedger::new().with_read(object(9), object(1), version(1));
        assert_eq!(
            ledger.stale_outputs(&current(&[])),
            BTreeSet::from([object(9)])
        );
    }

    #[test]
    fn staleness_travels_the_whole_chain() {
        let ledger = ContextLedger::new()
            .with_read(object(2), object(1), version(1))
            .with_read(object(3), object(2), version(2))
            .with_read(object(4), object(3), version(3));
        let stale = ledger.stale_outputs(&current(&[(1, 9), (2, 2), (3, 3)]));
        assert_eq!(stale, BTreeSet::from([object(2), object(3), object(4)]));
    }

    #[test]
    fn a_cycle_in_the_ledger_terminates() {
        let ledger = ContextLedger::new()
            .with_read(object(1), object(2), version(2))
            .with_read(object(2), object(1), version(1));
        assert_eq!(
            ledger.stale_outputs(&current(&[(1, 1), (2, 9)])),
            BTreeSet::from([object(1), object(2)])
        );
    }

    #[test]
    fn an_output_nothing_touched_stays_current() {
        let ledger = ContextLedger::new()
            .with_read(object(8), object(1), version(1))
            .with_read(object(9), object(2), version(2));
        assert_eq!(
            ledger.stale_outputs(&current(&[(1, 1), (2, 7)])),
            BTreeSet::from([object(9)])
        );
    }

    #[test]
    fn the_ledger_names_its_row() {
        assert_eq!(ContextLedger::new().rule(), Rule::ContextInputChanged);
    }
}
