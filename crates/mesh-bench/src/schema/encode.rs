//! Encoding a [`BenchmarkResult`] into its wire form.
//!
//! Field order is fixed and matches [`REQUIRED_FIELDS`](super::fields::REQUIRED_FIELDS),
//! so two identical runs produce byte-identical rows and a diff between two
//! rows shows only what actually differed.

use super::result::{BenchmarkResult, StorageFootprint};
use crate::json::{to_string_compact, to_string_pretty, Json, JsonObject};

impl BenchmarkResult {
    /// The row as a JSON value.
    pub fn to_json(&self) -> Json {
        let row = Json::Object(
            JsonObject::new()
                .with("schema_version", Json::string(&self.schema_version))
                .with("benchmark_id", Json::string(&self.benchmark_id))
                .with("invocation", Json::string(&self.invocation))
                .with("recorded_at_unix_ms", Json::Uint(self.recorded_at_unix_ms))
                .with("repository", self.repository_json())
                .with("hardware", self.hardware_json())
                .with("platform", self.platform_json())
                .with("build", self.build_json())
                .with("workload", self.workload_json())
                .with("cache_state", Json::string(self.cache_state.as_word()))
                .with("sample_count", Json::Uint(self.sample_count))
                .with(
                    "iterations_attempted",
                    Json::Uint(self.iterations_attempted),
                )
                .with("failure_count", Json::Uint(self.failure_count))
                .with(
                    "samples_ns",
                    Json::array(self.samples_ns.iter().copied().map(Json::Uint)),
                )
                .with("latency", self.latency_json())
                .with("verification", self.verification_json()),
        );
        // Appended rather than interleaved: the required contract's wire order
        // is a published fact, and a section that appears only sometimes must
        // not be able to shift the position of one that always does.
        match &self.storage {
            None => row,
            Some(storage) => match row {
                Json::Object(object) => Json::Object(object.with("storage", storage_json(storage))),
                other => other,
            },
        }
    }

    /// The row as one line of JSON — the form appended to a results file.
    pub fn to_json_line(&self) -> String {
        to_string_compact(&self.to_json())
    }

    /// The row as indented JSON — the form printed to a terminal.
    pub fn to_json_pretty(&self) -> String {
        to_string_pretty(&self.to_json())
    }

    fn repository_json(&self) -> Json {
        Json::Object(
            JsonObject::new()
                .with("remote", Json::string(&self.repository.remote))
                .with("commit", Json::string(&self.repository.commit))
                .with("dirty", Json::Bool(self.repository.dirty)),
        )
    }

    fn hardware_json(&self) -> Json {
        Json::Object(
            JsonObject::new()
                .with("cpu_model", Json::string(&self.hardware.cpu_model))
                .with("physical_cores", Json::Uint(self.hardware.physical_cores))
                .with("logical_cores", Json::Uint(self.hardware.logical_cores))
                .with("memory_bytes", Json::Uint(self.hardware.memory_bytes)),
        )
    }

    fn platform_json(&self) -> Json {
        Json::Object(
            JsonObject::new()
                .with("os", Json::string(&self.platform.os))
                .with("os_version", Json::string(&self.platform.os_version))
                .with("arch", Json::string(&self.platform.arch))
                .with("filesystem", Json::string(&self.platform.filesystem)),
        )
    }

    fn build_json(&self) -> Json {
        Json::Object(
            JsonObject::new()
                .with("profile", Json::string(&self.build.profile))
                .with("opt_level", Json::string(&self.build.opt_level))
                .with("debug_info", Json::string(&self.build.debug_info))
                .with("rustc_version", Json::string(&self.build.rustc_version))
                .with("target_triple", Json::string(&self.build.target_triple)),
        )
    }

    fn workload_json(&self) -> Json {
        Json::Object(
            JsonObject::new()
                .with("generator", Json::string(&self.workload.generator))
                .with(
                    "generator_version",
                    Json::string(&self.workload.generator_version),
                )
                .with("seed", Json::Uint(self.workload.seed))
                .with("parameters", Json::Object(self.workload.parameters.clone())),
        )
    }

    fn latency_json(&self) -> Json {
        Json::Object(
            JsonObject::new()
                .with("min_ns", Json::Uint(self.latency.min_ns))
                .with("p50_ns", Json::Uint(self.latency.p50_ns))
                .with("p95_ns", Json::Uint(self.latency.p95_ns))
                .with("p99_ns", Json::Uint(self.latency.p99_ns))
                .with("max_ns", Json::Uint(self.latency.max_ns))
                .with("mean_ns", Json::Uint(self.latency.mean_ns)),
        )
    }

    fn verification_json(&self) -> Json {
        Json::Object(
            JsonObject::new()
                .with("method", Json::string(&self.verification.method))
                .with(
                    "expected_digest",
                    Json::string(&self.verification.expected_digest),
                )
                .with(
                    "observed_digest",
                    Json::string(&self.verification.observed_digest),
                )
                .with("verified", Json::Bool(self.verification.passed())),
        )
    }
}

fn storage_json(storage: &StorageFootprint) -> Json {
    Json::Object(
        JsonObject::new()
            .with("boundary", Json::string(storage.boundary.as_word()))
            .with("granularity", Json::string(&storage.granularity))
            .with("admitted_bytes", Json::Uint(storage.admitted_bytes))
            .with(
                "distinct_content_bytes",
                Json::Uint(storage.distinct_content_bytes),
            )
            .with(
                "amplification_per_mille",
                Json::Uint(storage.amplification_per_mille),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::super::fields::{
        optional_storage_leaf_fields, required_leaf_fields, REQUIRED_FIELDS,
    };
    use super::*;
    use crate::testing::{sample_result, sample_storage_result};

    fn leaf_paths(value: &Json, prefix: &str, out: &mut Vec<String>) {
        match value {
            Json::Object(object) => {
                for (key, child) in object.entries() {
                    let path = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{prefix}.{key}")
                    };
                    // `workload.parameters` is an open sub-document: it is
                    // required, but its contents are generator-specific.
                    if path == "workload.parameters" {
                        out.push(path);
                    } else {
                        leaf_paths(child, &path, out);
                    }
                }
            }
            _ => out.push(prefix.to_owned()),
        }
    }

    #[test]
    fn the_encoder_emits_exactly_the_declared_contract() {
        let mut emitted = Vec::new();
        leaf_paths(&sample_result().to_json(), "", &mut emitted);
        let declared: Vec<String> = required_leaf_fields().map(str::to_owned).collect();
        assert_eq!(emitted, declared);
    }

    #[test]
    fn every_declared_path_resolves_in_an_encoded_row() {
        let row = sample_result().to_json();
        for field in REQUIRED_FIELDS {
            assert!(
                row.get_path(field).is_some(),
                "encoder never emits declared field `{field}`"
            );
        }
    }

    #[test]
    fn encoding_is_byte_stable() {
        let row = sample_result();
        assert_eq!(row.to_json_line(), row.clone().to_json_line());
    }

    #[test]
    fn rows_round_trip_through_text() {
        let row = sample_result();
        let decoded = BenchmarkResult::from_json_text(&row.to_json_line()).expect("round trip");
        assert_eq!(decoded, row);
        assert_eq!(decoded.to_json_line(), row.to_json_line());
    }

    #[test]
    fn a_row_with_no_storage_section_emits_none() {
        assert!(sample_result().to_json().get_path("storage").is_none());
    }

    #[test]
    fn a_storage_row_emits_the_contract_plus_exactly_the_optional_leaves() {
        let mut emitted = Vec::new();
        leaf_paths(&sample_storage_result().to_json(), "", &mut emitted);
        let mut declared: Vec<String> = required_leaf_fields().map(str::to_owned).collect();
        declared.extend(optional_storage_leaf_fields().map(str::to_owned));
        assert_eq!(emitted, declared);
    }

    #[test]
    fn a_storage_row_round_trips_through_text() {
        let row = sample_storage_result();
        let decoded = BenchmarkResult::from_json_text(&row.to_json_line()).expect("round trip");
        assert_eq!(decoded, row);
    }

    #[test]
    fn the_verified_flag_is_derived_not_stored() {
        let row = sample_result();
        assert_eq!(
            row.to_json().get_path("verification.verified"),
            Some(&Json::Bool(true))
        );
    }
}
