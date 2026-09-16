//! The contract self-check, shipped inside the binary.
//!
//! The rejection rules are asserted by the test suite, but a test suite is not
//! present on the machine that publishes a number. This runs the same
//! assertions from the built artefact, so `mesh-bench verify-schema` — and
//! `cargo bench --bench smoke -- --verify-schema` — prove on *this* build, on
//! *this* host, that an incomplete row is still refused.

use crate::json::Json;
use crate::schema::{optional_storage_leaf_fields, BenchmarkResult, SchemaError, REQUIRED_FIELDS};
use crate::sink::{MemoryWriter, SinkError, SinkPolicy, ValidatingSink};
use crate::testing::{sample_result, sample_result_with_samples, sample_storage_result};
use std::fmt;

/// One assertion about the schema contract.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckOutcome {
    /// What was asserted.
    pub name: String,
    /// Whether the assertion held.
    pub passed: bool,
    /// What was observed.
    pub detail: String,
}

/// The result of running every assertion.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct SelfCheckReport {
    /// Every assertion, in execution order.
    pub checks: Vec<CheckOutcome>,
}

impl SelfCheckReport {
    /// Whether every assertion held.
    pub fn passed(&self) -> bool {
        self.checks.iter().all(|check| check.passed)
    }

    /// The assertions that did not hold.
    pub fn failures(&self) -> Vec<&CheckOutcome> {
        self.checks.iter().filter(|check| !check.passed).collect()
    }

    fn record(&mut self, name: impl Into<String>, passed: bool, detail: impl Into<String>) {
        self.checks.push(CheckOutcome {
            name: name.into(),
            passed,
            detail: detail.into(),
        });
    }
}

impl fmt::Display for SelfCheckReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for check in &self.checks {
            writeln!(
                f,
                "{} {} — {}",
                if check.passed { "ok  " } else { "FAIL" },
                check.name,
                check.detail
            )?;
        }
        write!(
            f,
            "{} checks, {} failed",
            self.checks.len(),
            self.failures().len()
        )
    }
}

/// Runs the whole contract self-check.
pub fn run() -> SelfCheckReport {
    let mut report = SelfCheckReport::default();
    check_complete_row(&mut report);
    check_every_required_field(&mut report);
    check_derived_latency(&mut report);
    check_optional_storage_section(&mut report);
    check_unverified_row(&mut report);
    check_publishing_policy(&mut report);
    report
}

fn check_complete_row(report: &mut SelfCheckReport) {
    let row = sample_result();
    match row.validate() {
        Ok(()) => report.record(
            "complete-row-accepted",
            true,
            format!("{} fields present", REQUIRED_FIELDS.len()),
        ),
        Err(error) => report.record("complete-row-accepted", false, error.to_string()),
    }

    match BenchmarkResult::from_json_text(&row.to_json_line()) {
        Ok(decoded) if decoded == row => {
            report.record("row-round-trips", true, "encode -> decode is lossless")
        }
        Ok(_) => report.record(
            "row-round-trips",
            false,
            "decoded row differs from the original",
        ),
        Err(error) => report.record("row-round-trips", false, error.to_string()),
    }
}

fn check_every_required_field(report: &mut SelfCheckReport) {
    let complete = sample_result().to_json();
    for field in REQUIRED_FIELDS {
        let incomplete = complete.without_path(field);
        let mut sink = ValidatingSink::new(MemoryWriter::new(), SinkPolicy::PUBLISHABLE);
        let outcome = sink.accept_json(&incomplete);
        let name = format!("reject-missing:{field}");
        match outcome {
            Err(SinkError::Schema(SchemaError::MissingField { field: named }))
                if named == *field =>
            {
                let unwritten = sink.writer().lines().is_empty();
                report.record(
                    name,
                    unwritten,
                    if unwritten {
                        "refused by name, nothing written".to_owned()
                    } else {
                        "refused but still written".to_owned()
                    },
                );
            }
            Err(other) => report.record(name, false, format!("wrong rejection: {other}")),
            Ok(_) => report.record(name, false, "an incomplete row was accepted".to_owned()),
        }
    }
}

fn check_derived_latency(report: &mut SelfCheckReport) {
    let row = sample_result_with_samples(&[10, 20, 30, 40, 50]);
    let tampered = Json::Object(
        row.to_json().as_object().cloned().unwrap_or_default().with(
            "latency",
            Json::Object(
                row.to_json()
                    .get_path("latency")
                    .and_then(Json::as_object)
                    .cloned()
                    .unwrap_or_default()
                    .with("p99_ns", Json::Uint(1)),
            ),
        ),
    );
    let mut sink = ValidatingSink::new(MemoryWriter::new(), SinkPolicy::EXPLORATORY);
    match sink.accept_json(&tampered) {
        Err(SinkError::Schema(SchemaError::Inconsistent { .. })) => report.record(
            "reject-percentiles-that-contradict-samples",
            true,
            "declared p99 must be recomputable from samples_ns",
        ),
        Err(other) => report.record(
            "reject-percentiles-that-contradict-samples",
            false,
            format!("wrong rejection: {other}"),
        ),
        Ok(_) => report.record(
            "reject-percentiles-that-contradict-samples",
            false,
            "a tampered percentile was accepted",
        ),
    }
}

/// The one optional section: absent is legal, present-and-incomplete is not, and
/// its ratio is recomputed on read exactly as the percentiles are.
fn check_optional_storage_section(report: &mut SelfCheckReport) {
    let row = sample_storage_result();
    match row.validate() {
        Ok(()) => report.record(
            "accept-storage-section",
            true,
            "a row that admitted bytes carries them".to_owned(),
        ),
        Err(error) => report.record("accept-storage-section", false, error.to_string()),
    }

    let complete = row.to_json();
    let without = complete.without_path("storage");
    let mut sink = ValidatingSink::new(MemoryWriter::new(), SinkPolicy::PUBLISHABLE);
    match sink.accept_json(&without) {
        Ok(_) => report.record(
            "accept-row-with-no-storage-section",
            true,
            "optional means the section may be absent".to_owned(),
        ),
        Err(error) => report.record(
            "accept-row-with-no-storage-section",
            false,
            error.to_string(),
        ),
    }

    for field in optional_storage_leaf_fields() {
        let incomplete = complete.without_path(field);
        let mut sink = ValidatingSink::new(MemoryWriter::new(), SinkPolicy::PUBLISHABLE);
        let name = format!("reject-missing:{field}");
        match sink.accept_json(&incomplete) {
            Err(SinkError::Schema(SchemaError::MissingField { field: named }))
                if named == field =>
            {
                let unwritten = sink.writer().lines().is_empty();
                report.record(
                    name,
                    unwritten,
                    if unwritten {
                        "refused by name, nothing written".to_owned()
                    } else {
                        "refused but still written".to_owned()
                    },
                );
            }
            Err(other) => report.record(name, false, format!("wrong rejection: {other}")),
            Ok(_) => report.record(
                name,
                false,
                "a storage section missing a field was accepted".to_owned(),
            ),
        }
    }

    let storage = complete
        .get_path("storage")
        .and_then(Json::as_object)
        .cloned()
        .unwrap_or_default()
        .with("amplification_per_mille", Json::Uint(1_000));
    let tampered = Json::Object(
        complete
            .as_object()
            .cloned()
            .unwrap_or_default()
            .with("storage", Json::Object(storage)),
    );
    let mut sink = ValidatingSink::new(MemoryWriter::new(), SinkPolicy::PUBLISHABLE);
    match sink.accept_json(&tampered) {
        Err(SinkError::Schema(SchemaError::Inconsistent { .. })) => report.record(
            "reject-amplification-that-contradicts-its-counts",
            sink.writer().lines().is_empty(),
            "the ratio is recomputed from admitted_bytes and distinct_content_bytes".to_owned(),
        ),
        Err(other) => report.record(
            "reject-amplification-that-contradicts-its-counts",
            false,
            format!("wrong rejection: {other}"),
        ),
        Ok(_) => report.record(
            "reject-amplification-that-contradicts-its-counts",
            false,
            "a hand-edited ratio was accepted".to_owned(),
        ),
    }
}

fn check_unverified_row(report: &mut SelfCheckReport) {
    let row = sample_result();
    let verification = row
        .to_json()
        .get_path("verification")
        .and_then(Json::as_object)
        .cloned()
        .unwrap_or_default()
        .with("observed_digest", Json::string("fnv1a64:deadbeefdeadbeef"))
        .with("verified", Json::Bool(false));
    let unverified = Json::Object(
        row.to_json()
            .as_object()
            .cloned()
            .unwrap_or_default()
            .with("verification", Json::Object(verification)),
    );
    let mut sink = ValidatingSink::new(MemoryWriter::new(), SinkPolicy::EXPLORATORY);
    match sink.accept_json(&unverified) {
        Err(SinkError::Schema(SchemaError::UnverifiedRun { .. })) => report.record(
            "reject-timing-without-verification",
            true,
            "a failed verification produces no timing number",
        ),
        Err(other) => report.record(
            "reject-timing-without-verification",
            false,
            format!("wrong rejection: {other}"),
        ),
        Ok(_) => report.record(
            "reject-timing-without-verification",
            false,
            "an unverified run was accepted",
        ),
    }
}

fn check_publishing_policy(report: &mut SelfCheckReport) {
    let mut dirty = sample_result();
    dirty.repository.dirty = true;
    let mut sink = ValidatingSink::new(MemoryWriter::new(), SinkPolicy::PUBLISHABLE);
    match sink.accept(&dirty) {
        Err(SinkError::Policy { .. }) => report.record(
            "reject-dirty-worktree",
            true,
            "a dirty worktree is not publishable",
        ),
        Err(other) => report.record(
            "reject-dirty-worktree",
            false,
            format!("wrong rejection: {other}"),
        ),
        Ok(()) => report.record(
            "reject-dirty-worktree",
            false,
            "a dirty-worktree run was accepted",
        ),
    }

    let thin = sample_result_with_samples(&[10, 20, 30]);
    let mut sink = ValidatingSink::new(MemoryWriter::new(), SinkPolicy::PUBLISHABLE);
    match sink.accept(&thin) {
        Err(SinkError::Policy { .. }) => report.record(
            "reject-too-few-samples",
            true,
            format!(
                "fewer than {} samples is not publishable",
                SinkPolicy::PUBLISHABLE.min_sample_count
            ),
        ),
        Err(other) => report.record(
            "reject-too-few-samples",
            false,
            format!("wrong rejection: {other}"),
        ),
        Ok(()) => report.record(
            "reject-too-few-samples",
            false,
            "a three-sample run was accepted",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_self_check_passes_on_this_build() {
        let report = run();
        assert!(report.passed(), "{report}");
    }

    #[test]
    fn the_self_check_covers_the_optional_storage_section_too() {
        let report = run();
        let names: Vec<&str> = report
            .checks
            .iter()
            .map(|check| check.name.as_str())
            .collect();
        for expected in [
            "accept-storage-section",
            "accept-row-with-no-storage-section",
            "reject-missing:storage.distinct_content_bytes",
            "reject-amplification-that-contradicts-its-counts",
        ] {
            assert!(
                names.contains(&expected),
                "`{expected}` is not in the published self-check"
            );
        }
        assert!(report.passed(), "{report}");
    }

    #[test]
    fn the_self_check_covers_every_required_field() {
        let report = run();
        for field in REQUIRED_FIELDS {
            let name = format!("reject-missing:{field}");
            assert!(
                report.checks.iter().any(|check| check.name == name),
                "`{field}` is not covered by the self-check"
            );
        }
    }

    #[test]
    fn the_report_renders_its_failures() {
        let mut report = SelfCheckReport::default();
        report.record("invented", false, "for the renderer");
        assert!(!report.passed());
        assert!(report.to_string().contains("FAIL invented"));
        assert_eq!(report.failures().len(), 1);
    }
}
