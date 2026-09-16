//! The handoff to the review bundle, checked rather than commented.
//!
//! `mesh-approval` owns the vocabulary a review bundle absorbs — three words, `passed`, `failed`
//! and `skipped` — and it carries validation results rather than computing them, because plan
//! §8.3.7 keeps a validator off the publication path. This crate is on the other side of that
//! seam and **must not depend on `mesh-approval`**: it carries `canonical-mutation`, and the
//! architecture map's `validators-cannot-mutate-canonical-state` restriction forbids the edge.
//!
//! The dependency is therefore `[dev-dependencies]`, which compiles into this test binary and into
//! no library. What it buys is that the agreement between the two vocabularies is a **test**, so a
//! lane renaming a word on either side finds out here instead of finding out when a bundle carries
//! a verdict nothing recognises.
//!
//! The interesting assertion is the second one: `blocked` and `errored` have no counterpart on the
//! other side, and they narrow to **failed**. A crashed validator arriving in a bundle as
//! "skipped" would tell a reviewer the rule did not apply — a different and false statement, and
//! the exact fail-open this task exists to prevent.

use mesh_approval::{ValidationResult, Verdict};
use mesh_validator::RunVerdict;

#[test]
fn the_three_shared_words_are_the_same_words_on_both_sides() {
    assert_eq!(RunVerdict::Passed.as_str(), Verdict::Passed.label());
    assert_eq!(RunVerdict::Failed.as_str(), Verdict::Failed.label());
    assert_eq!(RunVerdict::Skipped.as_str(), Verdict::Skipped.label());
}

#[test]
fn every_verdict_narrows_to_a_word_the_bundle_knows() {
    let bundle_words = [
        Verdict::Passed.label(),
        Verdict::Failed.label(),
        Verdict::Skipped.label(),
    ];
    for verdict in RunVerdict::ALL {
        assert!(
            bundle_words.contains(&verdict.bundle_word()),
            "`{verdict}` narrows to `{}`, which the bundle does not know",
            verdict.bundle_word()
        );
    }
}

#[test]
fn an_inconclusive_verdict_narrows_to_failed_and_never_to_passed_or_skipped() {
    for verdict in RunVerdict::ALL {
        if !verdict.is_inconclusive() {
            continue;
        }
        assert_eq!(
            verdict.bundle_word(),
            Verdict::Failed.label(),
            "`{verdict}` reached the bundle as something other than a failure"
        );
    }
    assert_eq!(RunVerdict::Blocked.bundle_word(), "failed");
    assert_eq!(RunVerdict::Errored.bundle_word(), "failed");
}

#[test]
fn only_a_pass_narrows_to_the_bundles_pass() {
    let passing: Vec<RunVerdict> = RunVerdict::ALL
        .into_iter()
        .filter(|verdict| verdict.bundle_word() == Verdict::Passed.label())
        .collect();
    assert_eq!(passing, vec![RunVerdict::Passed]);
    for verdict in RunVerdict::ALL {
        assert_eq!(
            verdict.is_pass(),
            verdict.bundle_word() == Verdict::Passed.label()
        );
    }
}

#[test]
fn a_narrowed_verdict_builds_the_result_a_bundle_absorbs() {
    // The end of the handoff: a `RunVerdict` becomes a `mesh_approval::ValidationResult` with no
    // information invented on the way. Parsing the word back is what proves the two vocabularies
    // meet rather than merely resembling each other.
    for verdict in RunVerdict::ALL {
        let word = verdict.bundle_word();
        let bundle_verdict = match word {
            "passed" => Verdict::Passed,
            "failed" => Verdict::Failed,
            "skipped" => Verdict::Skipped,
            other => panic!("`{other}` is not a word the bundle knows"),
        };
        let result = ValidationResult::new("cargo-test", None, bundle_verdict, verdict.as_str());
        assert_eq!(result.verdict().label(), word);
        assert_eq!(
            result.verdict().is_failure(),
            verdict.is_inconclusive() || verdict == RunVerdict::Failed,
            "`{verdict}` reached the reviewer with the wrong weight"
        );
        assert_eq!(result.detail(), verdict.as_str());
    }
}
