//! Validation results, carried into the bundle rather than re-run at review time.
//!
//! # Why they are carried and not computed
//!
//! Validators are `mesh-validator`'s, and plan §8.3.7 says a validator cannot mutate canonical
//! state — running one from here would put a validator on the publication path. So a result is
//! evidence handed to this crate, and what this crate guarantees is narrower and checkable: the
//! results are **ordered canonically** and **absorbed into the bundle's identity**, so a bundle
//! whose validation results differ in any way is a different bundle with a different name.
//!
//! # Order is imposed, not accepted
//!
//! A caller that collected results by walking a hash map hands them over in an order that varies
//! between processes. [`ValidationResult`] therefore has a total order derived from its content,
//! and [`crate::compute_bundle`] sorts and de-duplicates before absorbing. The same *set* of results
//! produces the same bytes however it arrived.

use core::cmp::Ordering;
use core::fmt;

use crate::digest::{Absorb, DigestHasher, DigestWriter};
use crate::ids::ObjectId;

/// What a validator concluded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Verdict {
    /// The subject satisfies the rule.
    Passed,
    /// The rule does not apply to this subject.
    Skipped,
    /// The subject violates the rule.
    Failed,
}

impl Verdict {
    /// The value this verdict is absorbed and ordered by.
    #[must_use]
    pub const fn rank(self) -> u64 {
        match self {
            Self::Passed => 0,
            Self::Skipped => 1,
            Self::Failed => 2,
        }
    }

    /// Whether this verdict is one a reviewer has to weigh.
    #[must_use]
    pub const fn is_failure(self) -> bool {
        matches!(self, Self::Failed)
    }

    /// A short, stable word for a surface that renders one.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Skipped => "skipped",
            Self::Failed => "failed",
        }
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label())
    }
}

/// One validator's conclusion about one subject.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationResult {
    validator: String,
    subject: Option<ObjectId>,
    verdict: Verdict,
    detail: String,
}

impl ValidationResult {
    /// One result.
    ///
    /// `subject` is absent for a validator that judged the whole state rather than one object.
    #[must_use]
    pub fn new(validator: &str, subject: Option<ObjectId>, verdict: Verdict, detail: &str) -> Self {
        Self {
            validator: validator.to_owned(),
            subject,
            verdict,
            detail: detail.to_owned(),
        }
    }

    /// Which validator concluded it.
    #[must_use]
    pub fn validator(&self) -> &str {
        &self.validator
    }

    /// What it concluded about, when it concluded about one object.
    #[must_use]
    pub const fn subject(&self) -> Option<ObjectId> {
        self.subject
    }

    /// What it concluded.
    #[must_use]
    pub const fn verdict(&self) -> Verdict {
        self.verdict
    }

    /// What it said, for a person to read.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl Ord for ValidationResult {
    fn cmp(&self, other: &Self) -> Ordering {
        self.validator
            .cmp(&other.validator)
            .then_with(|| self.subject.cmp(&other.subject))
            .then_with(|| self.verdict.rank().cmp(&other.verdict.rank()))
            .then_with(|| self.detail.cmp(&other.detail))
    }
}

impl PartialOrd for ValidationResult {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Absorb for ValidationResult {
    fn absorb<H: DigestHasher>(&self, writer: &mut DigestWriter<H>) {
        writer.text(&self.validator);
        writer.option(self.subject.as_ref(), |writer, subject| {
            writer.bytes(subject.as_bytes());
        });
        writer.u64(self.verdict.rank());
        writer.text(&self.detail);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(byte: u8) -> ObjectId {
        ObjectId::from_bytes([byte; 16])
    }

    #[test]
    fn results_order_by_validator_then_subject_then_verdict_then_detail() {
        let mut results = [
            ValidationResult::new("schema", Some(object(2)), Verdict::Passed, ""),
            ValidationResult::new("schema", Some(object(1)), Verdict::Failed, "b"),
            ValidationResult::new("schema", Some(object(1)), Verdict::Failed, "a"),
            ValidationResult::new("lint", None, Verdict::Skipped, ""),
        ];
        results.sort();
        let rendered: Vec<String> = results
            .iter()
            .map(|result| format!("{}:{}", result.validator(), result.detail()))
            .collect();
        assert_eq!(rendered, vec!["lint:", "schema:a", "schema:b", "schema:"]);
    }

    #[test]
    fn a_result_reports_everything_it_was_given() {
        let result = ValidationResult::new("schema", Some(object(1)), Verdict::Failed, "bad shape");
        assert_eq!(result.validator(), "schema");
        assert_eq!(result.subject(), Some(object(1)));
        assert_eq!(result.verdict(), Verdict::Failed);
        assert_eq!(result.detail(), "bad shape");
        assert!(result.verdict().is_failure());
    }

    #[test]
    fn the_three_verdicts_have_distinct_ranks_and_words() {
        assert_eq!(Verdict::Passed.rank(), 0);
        assert_eq!(Verdict::Skipped.rank(), 1);
        assert_eq!(Verdict::Failed.rank(), 2);
        assert_eq!(Verdict::Passed.to_string(), "passed");
        assert!(!Verdict::Skipped.is_failure());
    }
}
