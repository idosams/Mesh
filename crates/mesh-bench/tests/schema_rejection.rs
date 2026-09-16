//! A result missing any required metadata field is rejected at write time.
//!
//! The unit tests check the decoder; these check the *door* — the public API a
//! benchmark actually uses. A row that never reaches the writer is the only
//! acceptable outcome for every one of these cases.

use mesh_bench::json::{Json, JsonObject};
use mesh_bench::schema::{BenchmarkResult, SchemaError, REQUIRED_FIELDS};
use mesh_bench::sink::{MemoryWriter, SinkError, SinkPolicy, ValidatingSink};
use mesh_bench::testing::{sample_result, sample_result_with_samples};

fn publishing_sink() -> ValidatingSink<MemoryWriter> {
    ValidatingSink::new(MemoryWriter::new(), SinkPolicy::PUBLISHABLE)
}

fn with_section_field(row: &Json, section: &str, key: &str, value: Json) -> Json {
    let updated = row
        .get_path(section)
        .and_then(Json::as_object)
        .cloned()
        .unwrap_or_default()
        .with(key, value);
    Json::Object(
        row.as_object()
            .cloned()
            .unwrap_or_default()
            .with(section, Json::Object(updated)),
    )
}

fn with_field(row: &Json, key: &str, value: Json) -> Json {
    Json::Object(
        row.as_object()
            .cloned()
            .unwrap_or_default()
            .with(key, value),
    )
}

#[test]
fn every_required_field_is_rejected_by_name_and_nothing_is_written() {
    let complete = sample_result().to_json();
    assert!(
        REQUIRED_FIELDS.len() >= 40,
        "the contract should not have shrunk: {} fields",
        REQUIRED_FIELDS.len()
    );

    for field in REQUIRED_FIELDS {
        let mut sink = publishing_sink();
        let outcome = sink.accept_json(&complete.without_path(field));

        match outcome {
            Err(SinkError::Schema(SchemaError::MissingField { field: named })) => {
                assert_eq!(named, *field, "wrong field named");
            }
            Err(other) => panic!("removing `{field}` produced the wrong rejection: {other}"),
            Ok(_) => panic!("a row missing `{field}` was accepted"),
        }
        assert!(
            sink.writer().lines().is_empty(),
            "a row missing `{field}` reached the writer"
        );
    }
}

#[test]
fn the_contract_covers_the_dimensions_the_task_requires() {
    for (dimension, path) in [
        ("repository commit", "repository.commit"),
        ("hardware", "hardware.cpu_model"),
        ("operating system", "platform.os"),
        ("filesystem", "platform.filesystem"),
        ("build profile", "build.profile"),
        ("workload data generator", "workload.generator"),
        ("warm/cold cache state", "cache_state"),
        ("sample count", "sample_count"),
        ("raw results", "samples_ns"),
        ("p50", "latency.p50_ns"),
        ("p95", "latency.p95_ns"),
        ("p99", "latency.p99_ns"),
        ("failure count", "failure_count"),
        ("correctness verification", "verification.verified"),
    ] {
        assert!(
            REQUIRED_FIELDS.contains(&path),
            "`{dimension}` is not required (`{path}`)"
        );
        let mut sink = publishing_sink();
        assert!(
            sink.accept_json(&sample_result().to_json().without_path(path))
                .is_err(),
            "a row without `{dimension}` was accepted"
        );
    }
}

#[test]
fn a_complete_row_is_accepted_and_written_once() {
    let mut sink = publishing_sink();
    let row = sample_result();
    sink.accept(&row).expect("a complete row is publishable");
    assert_eq!(sink.writer().lines(), &[row.to_json_line()]);
}

#[test]
fn percentiles_must_be_recomputable_from_the_raw_samples() {
    let row = sample_result_with_samples(&[10, 20, 30, 40, 50]);
    let tampered = with_section_field(&row.to_json(), "latency", "p99_ns", Json::Uint(11));
    let mut sink = ValidatingSink::new(MemoryWriter::new(), SinkPolicy::EXPLORATORY);
    let error = sink
        .accept_json(&tampered)
        .expect_err("a hand-edited percentile is not a measurement");
    assert!(matches!(
        error,
        SinkError::Schema(SchemaError::Inconsistent { .. })
    ));
}

#[test]
fn the_sample_count_must_match_the_raw_samples() {
    let row = sample_result_with_samples(&[10, 20, 30]);
    let tampered = with_field(&row.to_json(), "sample_count", Json::Uint(300));
    let mut sink = ValidatingSink::new(MemoryWriter::new(), SinkPolicy::EXPLORATORY);
    assert!(sink.accept_json(&tampered).is_err());
}

#[test]
fn attempted_iterations_must_account_for_every_failure() {
    let row = sample_result_with_samples(&[10, 20, 30]);
    let tampered = with_field(&row.to_json(), "failure_count", Json::Uint(2));
    let mut sink = ValidatingSink::new(MemoryWriter::new(), SinkPolicy::EXPLORATORY);
    assert!(
        sink.accept_json(&tampered).is_err(),
        "3 samples + 2 failures cannot be 3 attempts"
    );
}

#[test]
fn a_row_whose_verification_failed_carries_no_timing() {
    let row = sample_result();
    let unverified = with_section_field(
        &row.to_json(),
        "verification",
        "observed_digest",
        Json::string("fnv1a64:0000000000000000"),
    );
    let unverified = with_section_field(&unverified, "verification", "verified", Json::Bool(false));

    let mut sink = ValidatingSink::new(MemoryWriter::new(), SinkPolicy::EXPLORATORY);
    let error = sink
        .accept_json(&unverified)
        .expect_err("a failed verification produces no timing number");
    assert!(matches!(
        error,
        SinkError::Schema(SchemaError::UnverifiedRun { .. })
    ));
    assert!(sink.writer().lines().is_empty());
}

#[test]
fn an_unknown_schema_version_is_refused() {
    let tampered = with_field(
        &sample_result().to_json(),
        "schema_version",
        Json::string("mesh-bench/result/v0"),
    );
    let mut sink = ValidatingSink::new(MemoryWriter::new(), SinkPolicy::EXPLORATORY);
    assert!(matches!(
        sink.accept_json(&tampered),
        Err(SinkError::Schema(
            SchemaError::UnsupportedSchemaVersion { .. }
        ))
    ));
}

#[test]
fn an_empty_object_names_the_first_missing_field_rather_than_crashing() {
    let error = BenchmarkResult::from_json(&Json::Object(JsonObject::new()))
        .expect_err("an empty object is not a row");
    assert!(matches!(error, SchemaError::MissingField { .. }));
}

#[test]
fn an_external_party_can_rebuild_the_row_from_its_own_text() {
    // "Reproducible from the result row alone" starts with: the row survives
    // the trip through a file and still says exactly the same thing.
    let row = sample_result();
    let line = row.to_json_line();
    let decoded = BenchmarkResult::from_json_text(&line).expect("published rows decode");

    assert_eq!(decoded, row);
    assert_eq!(decoded.to_json_line(), line);
    assert_eq!(decoded.repository.commit.len(), 40);
    assert!(!decoded.repository.remote.is_empty());
    assert_eq!(decoded.samples_ns.len(), decoded.sample_count as usize);
    assert!(decoded.invocation.contains("mesh-bench"));
}
