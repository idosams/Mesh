//! The collector's decision: which candidate chunks nothing retains, and why.
//!
//! # This module decides; it never deletes
//!
//! Splitting the two is not tidiness. A plan is a *value* — pure, comparable, printable — so the
//! dry run and the real run are the same computation and cannot drift: [`CollectionPlan::compute`]
//! takes no mode at all, and [`CollectionPlan::report`] renders exactly the list the deleter is
//! handed. A dry run that took a different path through the code would be a description of a
//! program nobody runs.
//!
//! The bytes are `mesh-cas`'s, and the two crates share no types because neither declares a
//! dependency (`docs/adr/0008-…`). The handoff is [`CollectionPlan::doomed_digests`], a list of
//! `[u8; 32]`, and `mesh-cas`'s `Cas::collect` demands a reference oracle of its own so that even
//! a caller who hands it the wrong list cannot delete something referenced.
//!
//! # Why an empty root set is refused rather than obeyed
//!
//! "Retain nothing" is never what a caller meant; it is what a caller gets when a root set failed
//! to load. Obeying it would delete the workspace. So [`CollectionPlan::compute`] returns
//! [`RetentionError::NoRetainedRoots`] and the only way past it is to name at least one root —
//! which [`crate::RetainedRoots::conservative`] does from the index itself.

use crate::ids::RecordDigest;
use crate::index::Index;
use crate::retention::{Reachability, RetainedRoot, RetainedRoots, RetentionError};

/// Why a candidate is collectable.
///
/// Two reasons, and the difference is worth printing: one says the workspace never committed a
/// reference to these bytes, the other says it did and has since moved on. A dry run that reported
/// only "collectable" would hide a store full of the second kind, which is the case where a
/// retained root has been dropped by mistake.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CollectionReason {
    /// No record in the index names this digest at all — plan §6.3's window between step 4 and
    /// step 9, left behind by a crash or an abandoned transaction.
    NeverReferenced,
    /// A record names it, but no retained root reaches that record.
    NoRetainedRootReaches,
}

impl core::fmt::Display for CollectionReason {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(match self {
            Self::NeverReferenced => "no record references it",
            Self::NoRetainedRootReaches => "referenced, but no retained root reaches it",
        })
    }
}

/// One candidate the plan would delete.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Doomed {
    /// The chunk.
    pub digest: RecordDigest,
    /// Why it is collectable.
    pub reason: CollectionReason,
}

/// One candidate the plan keeps, with the root that keeps it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Kept {
    /// The chunk.
    pub digest: RecordDigest,
    /// The root that reaches it.
    pub root: RetainedRoot,
}

/// What a collection would do, before anything does it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectionPlan {
    doomed: Vec<Doomed>,
    kept: Vec<Kept>,
    root_count: usize,
    retained_content_count: usize,
}

impl CollectionPlan {
    /// Partition a candidate set into what nothing retains and what something does.
    ///
    /// `candidates` is whatever the content-addressed store offers — its arrival journal on the
    /// cheap path, a full sweep on the backstop path. The plan is a function of the candidates and
    /// the reachability alone, so feeding it a *larger* candidate set can only find more garbage
    /// and can never endanger anything: a retained digest is kept whichever list it arrives on.
    ///
    /// # Errors
    ///
    /// [`RetentionError::NoRetainedRoots`] when the root set is empty.
    pub fn compute(
        index: &Index,
        roots: &RetainedRoots,
        reachability: &Reachability,
        candidates: impl IntoIterator<Item = RecordDigest>,
    ) -> Result<Self, RetentionError> {
        if roots.is_empty() {
            return Err(RetentionError::NoRetainedRoots);
        }
        let named = index.named_content();

        let mut doomed = Vec::new();
        let mut kept = Vec::new();
        for digest in candidates {
            match reachability.why_retained(&digest) {
                Some(root) => kept.push(Kept {
                    digest,
                    root: root.clone(),
                }),
                None => doomed.push(Doomed {
                    digest,
                    reason: if named.contains(&digest) {
                        CollectionReason::NoRetainedRootReaches
                    } else {
                        CollectionReason::NeverReferenced
                    },
                }),
            }
        }
        doomed.sort_unstable();
        doomed.dedup();
        kept.sort();
        kept.dedup();

        Ok(Self {
            doomed,
            kept,
            root_count: roots.len(),
            retained_content_count: reachability.retained_content_count(),
        })
    }

    /// What would be deleted, in digest order.
    #[must_use]
    pub fn doomed(&self) -> &[Doomed] {
        &self.doomed
    }

    /// Which candidates are kept, and by which root.
    #[must_use]
    pub fn kept(&self) -> &[Kept] {
        &self.kept
    }

    /// The doomed set as raw digests, which is what `mesh-cas` takes.
    #[must_use]
    pub fn doomed_digests(&self) -> Vec<[u8; 32]> {
        self.doomed
            .iter()
            .map(|entry| *entry.digest.as_bytes())
            .collect()
    }

    /// Whether this plan would free nothing.
    ///
    /// Named so a caller can *assert* on it. "The collector reported exactly what it would delete"
    /// is satisfied by a collector that reports the empty set, so a store that ought to have
    /// garbage and yields `true` here is a finding, not a pass.
    #[must_use]
    pub fn frees_nothing(&self) -> bool {
        self.doomed.is_empty()
    }

    /// How many roots the plan was computed against.
    #[must_use]
    pub fn root_count(&self) -> usize {
        self.root_count
    }

    /// How many content digests those roots retain in total — not only among the candidates.
    #[must_use]
    pub fn retained_content_count(&self) -> usize {
        self.retained_content_count
    }

    /// The dry run, as text: every candidate, whether it goes, and why.
    ///
    /// Kept candidates name their root, because "why is my disk still full" is the question a dry
    /// run is actually asked, and an answer that lists only deletions cannot answer it.
    #[must_use]
    pub fn report(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "retained roots: {}\nretained content digests: {}\ncandidates: {}\n\
             to delete: {}\nto keep: {}\n",
            self.root_count,
            self.retained_content_count,
            self.doomed.len() + self.kept.len(),
            self.doomed.len(),
            self.kept.len()
        ));
        for entry in &self.doomed {
            out.push_str(&format!("delete {} — {}\n", entry.digest, entry.reason));
        }
        for entry in &self.kept {
            out.push_str(&format!("keep   {} — {}\n", entry.digest, entry.root));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{OperationRecord, StoredRecord};
    use crate::retention::RetentionPolicy;
    use crate::EntityUuid;

    fn digest(seed: u8) -> RecordDigest {
        RecordDigest::from_bytes([seed; 32])
    }

    fn index_with_one_operation() -> Index {
        let mut index = Index::new();
        index
            .apply(StoredRecord::Operation(OperationRecord {
                id: digest(1),
                actor: digest(100),
                actor_sequence: 1,
                hlc_millis: 0,
                hlc_counter: 0,
                policy_epoch: 1,
                session: EntityUuid::from_bytes([0; 16]),
                payload_digest: digest(10),
                parents: Vec::new(),
            }))
            .unwrap();
        index
    }

    #[test]
    fn an_empty_root_set_is_refused() {
        let index = index_with_one_operation();
        let roots = RetainedRoots::new(RetentionPolicy::default());
        let reach = Reachability::compute(&index, &roots).unwrap();
        assert_eq!(
            CollectionPlan::compute(&index, &roots, &reach, [digest(10)]),
            Err(RetentionError::NoRetainedRoots)
        );
    }

    #[test]
    fn a_referenced_payload_is_kept_and_an_orphan_is_doomed() {
        let index = index_with_one_operation();
        let roots = RetainedRoots::conservative(&index, RetentionPolicy::default());
        let reach = Reachability::compute(&index, &roots).unwrap();
        let plan =
            CollectionPlan::compute(&index, &roots, &reach, [digest(10), digest(200)]).unwrap();
        assert_eq!(
            plan.doomed(),
            [Doomed {
                digest: digest(200),
                reason: CollectionReason::NeverReferenced,
            }]
        );
        assert_eq!(plan.kept().len(), 1);
        assert_eq!(plan.kept()[0].digest, digest(10));
        assert!(!plan.frees_nothing());
    }

    #[test]
    fn dropping_the_only_root_makes_referenced_content_collectable_with_the_second_reason() {
        let index = index_with_one_operation();
        let roots =
            RetainedRoots::new(RetentionPolicy::default()).with(RetainedRoot::RetentionWindow {
                actor: digest(100),
                from_sequence: 99,
            });
        let reach = Reachability::compute(&index, &roots).unwrap();
        let plan = CollectionPlan::compute(&index, &roots, &reach, [digest(10)]).unwrap();
        assert_eq!(
            plan.doomed(),
            [Doomed {
                digest: digest(10),
                reason: CollectionReason::NoRetainedRootReaches,
            }]
        );
    }

    #[test]
    fn the_report_names_both_the_deletions_and_the_root_behind_each_survivor() {
        let index = index_with_one_operation();
        let roots = RetainedRoots::conservative(&index, RetentionPolicy::default());
        let reach = Reachability::compute(&index, &roots).unwrap();
        let plan =
            CollectionPlan::compute(&index, &roots, &reach, [digest(10), digest(200)]).unwrap();
        let report = plan.report();
        assert!(report.contains(&format!("delete {}", digest(200))));
        assert!(report.contains("no record references it"));
        assert!(report.contains(&format!("keep   {}", digest(10))));
        assert!(report.contains("head of actor"));
    }

    #[test]
    fn the_handoff_is_raw_bytes_because_the_two_crates_share_no_types() {
        let index = index_with_one_operation();
        let roots = RetainedRoots::conservative(&index, RetentionPolicy::default());
        let reach = Reachability::compute(&index, &roots).unwrap();
        let plan = CollectionPlan::compute(&index, &roots, &reach, [digest(200)]).unwrap();
        assert_eq!(plan.doomed_digests(), vec![[200u8; 32]]);
    }
}
