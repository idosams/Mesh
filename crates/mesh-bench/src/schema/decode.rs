//! Decoding a candidate row into a [`BenchmarkResult`], or refusing it by name.

use super::error::SchemaError;
use super::result::{
    BenchmarkResult, BuildProfile, CacheState, Hardware, HostPlatform, LatencySummary, Repository,
    StorageBoundary, StorageFootprint, Verification, WorkloadDescriptor,
};
use crate::json::{Json, JsonObject};

impl BenchmarkResult {
    /// Decodes and fully validates a candidate row.
    ///
    /// Presence first, then types, then cross-field consistency: the caller
    /// gets told about the missing field rather than about the consequence of
    /// the missing field.
    pub fn from_json(value: &Json) -> Result<Self, SchemaError> {
        let root = Reader::root(value)?;
        let repository = root.section("repository")?;
        let hardware = root.section("hardware")?;
        let platform = root.section("platform")?;
        let build = root.section("build")?;
        let workload = root.section("workload")?;
        let latency = root.section("latency")?;
        let verification = root.section("verification")?;

        let decoded = BenchmarkResult {
            schema_version: root.string("schema_version")?,
            benchmark_id: root.string("benchmark_id")?,
            invocation: root.string("invocation")?,
            recorded_at_unix_ms: root.u64("recorded_at_unix_ms")?,
            repository: Repository {
                remote: repository.string("remote")?,
                commit: repository.string("commit")?,
                dirty: repository.bool("dirty")?,
            },
            hardware: Hardware {
                cpu_model: hardware.string("cpu_model")?,
                physical_cores: hardware.u64("physical_cores")?,
                logical_cores: hardware.u64("logical_cores")?,
                memory_bytes: hardware.u64("memory_bytes")?,
            },
            platform: HostPlatform {
                os: platform.string("os")?,
                os_version: platform.string("os_version")?,
                arch: platform.string("arch")?,
                filesystem: platform.string("filesystem")?,
            },
            build: BuildProfile {
                profile: build.string("profile")?,
                opt_level: build.string("opt_level")?,
                debug_info: build.string("debug_info")?,
                rustc_version: build.string("rustc_version")?,
                target_triple: build.string("target_triple")?,
            },
            workload: WorkloadDescriptor {
                generator: workload.string("generator")?,
                generator_version: workload.string("generator_version")?,
                seed: workload.u64("seed")?,
                parameters: workload.object("parameters")?,
            },
            cache_state: root.cache_state("cache_state")?,
            sample_count: root.u64("sample_count")?,
            iterations_attempted: root.u64("iterations_attempted")?,
            failure_count: root.u64("failure_count")?,
            samples_ns: root.u64_array("samples_ns")?,
            latency: LatencySummary {
                min_ns: latency.u64("min_ns")?,
                p50_ns: latency.u64("p50_ns")?,
                p95_ns: latency.u64("p95_ns")?,
                p99_ns: latency.u64("p99_ns")?,
                max_ns: latency.u64("max_ns")?,
                mean_ns: latency.u64("mean_ns")?,
            },
            verification: Verification {
                method: verification.string("method")?,
                expected_digest: verification.string("expected_digest")?,
                observed_digest: verification.string("observed_digest")?,
            },
            // Absent is a legal answer; present-and-incomplete is not. Once the
            // section exists every field inside it is required by name.
            storage: match root.optional_section("storage")? {
                None => None,
                Some(storage) => Some(StorageFootprint {
                    boundary: storage.storage_boundary("boundary")?,
                    granularity: storage.string("granularity")?,
                    admitted_bytes: storage.u64("admitted_bytes")?,
                    distinct_content_bytes: storage.u64("distinct_content_bytes")?,
                    amplification_per_mille: storage.u64("amplification_per_mille")?,
                }),
            },
        };

        // The wire form carries `verified` as well as the two digests. A row
        // whose boolean disagrees with its own digests has been hand-edited.
        let declared = verification.bool("verified")?;
        if declared != decoded.verification.passed() {
            return Err(SchemaError::inconsistent(format!(
                "verification.verified is {declared} but the digests {}",
                if decoded.verification.passed() {
                    "agree"
                } else {
                    "disagree"
                }
            )));
        }

        decoded.validate()?;
        Ok(decoded)
    }

    /// Parses and validates a row from JSON text.
    pub fn from_json_text(text: &str) -> Result<Self, DecodeError> {
        let value = crate::json::parse(text).map_err(DecodeError::Parse)?;
        BenchmarkResult::from_json(&value).map_err(DecodeError::Schema)
    }
}

/// A failure to turn text into a validated result row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// The text was not JSON.
    Parse(crate::json::ParseError),
    /// The JSON was not a result row.
    Schema(SchemaError),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::Parse(error) => write!(f, "{error}"),
            DecodeError::Schema(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// A cursor over one object, carrying the dotted path it was reached by.
struct Reader<'a> {
    object: &'a JsonObject,
    prefix: String,
}

impl<'a> Reader<'a> {
    fn root(value: &'a Json) -> Result<Self, SchemaError> {
        match value.as_object() {
            Some(object) => Ok(Reader {
                object,
                prefix: String::new(),
            }),
            None => Err(SchemaError::TypeMismatch {
                field: "<row>".to_owned(),
                expected: "object",
                found: value.type_name(),
            }),
        }
    }

    fn path(&self, key: &str) -> String {
        if self.prefix.is_empty() {
            key.to_owned()
        } else {
            format!("{}.{key}", self.prefix)
        }
    }

    fn get(&self, key: &str) -> Result<&'a Json, SchemaError> {
        self.object
            .get(key)
            .ok_or_else(|| SchemaError::missing(self.path(key)))
    }

    fn mismatch(&self, key: &str, expected: &'static str, value: &Json) -> SchemaError {
        SchemaError::TypeMismatch {
            field: self.path(key),
            expected,
            found: value.type_name(),
        }
    }

    fn section(&self, key: &str) -> Result<Reader<'a>, SchemaError> {
        let value = self.get(key)?;
        let object = value
            .as_object()
            .ok_or_else(|| self.mismatch(key, "object", value))?;
        Ok(Reader {
            object,
            prefix: self.path(key),
        })
    }

    fn object(&self, key: &str) -> Result<JsonObject, SchemaError> {
        let value = self.get(key)?;
        value
            .as_object()
            .cloned()
            .ok_or_else(|| self.mismatch(key, "object", value))
    }

    fn string(&self, key: &str) -> Result<String, SchemaError> {
        let value = self.get(key)?;
        value
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| self.mismatch(key, "string", value))
    }

    fn bool(&self, key: &str) -> Result<bool, SchemaError> {
        let value = self.get(key)?;
        value
            .as_bool()
            .ok_or_else(|| self.mismatch(key, "boolean", value))
    }

    fn u64(&self, key: &str) -> Result<u64, SchemaError> {
        let value = self.get(key)?;
        value
            .as_u64()
            .ok_or_else(|| self.mismatch(key, "non-negative integer", value))
    }

    fn u64_array(&self, key: &str) -> Result<Vec<u64>, SchemaError> {
        let value = self.get(key)?;
        let items = value
            .as_array()
            .ok_or_else(|| self.mismatch(key, "array", value))?;
        items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                item.as_u64().ok_or_else(|| SchemaError::TypeMismatch {
                    field: format!("{}[{index}]", self.path(key)),
                    expected: "non-negative integer",
                    found: item.type_name(),
                })
            })
            .collect()
    }

    /// A section that may legitimately be absent, but not malformed.
    fn optional_section(&self, key: &str) -> Result<Option<Reader<'a>>, SchemaError> {
        let Some(value) = self.object.get(key) else {
            return Ok(None);
        };
        let object = value
            .as_object()
            .ok_or_else(|| self.mismatch(key, "object", value))?;
        Ok(Some(Reader {
            object,
            prefix: self.path(key),
        }))
    }

    fn storage_boundary(&self, key: &str) -> Result<StorageBoundary, SchemaError> {
        let word = self.string(key)?;
        StorageBoundary::from_word(&word).ok_or(SchemaError::UnknownValue {
            field: self.path(key),
            value: word,
        })
    }

    fn cache_state(&self, key: &str) -> Result<CacheState, SchemaError> {
        let word = self.string(key)?;
        CacheState::from_word(&word).ok_or(SchemaError::UnknownValue {
            field: self.path(key),
            value: word,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::fields::{OPTIONAL_STORAGE_FIELDS, REQUIRED_FIELDS};
    use super::*;
    use crate::testing::{sample_result, sample_storage_result};

    #[test]
    fn a_complete_row_decodes() {
        let row = sample_result();
        let decoded = BenchmarkResult::from_json(&row.to_json()).expect("complete row decodes");
        assert_eq!(decoded, row);
    }

    /// The heart of the schema contract: every declared field, removed one at a
    /// time, must be refused *by its own name*.
    #[test]
    fn every_required_field_is_rejected_when_missing() {
        let complete = sample_result().to_json();
        for field in REQUIRED_FIELDS {
            let incomplete = complete.without_path(field);
            let error = BenchmarkResult::from_json(&incomplete)
                .expect_err(&format!("`{field}` must be required"));
            assert_eq!(
                error,
                SchemaError::missing(*field),
                "removing `{field}` produced the wrong rejection"
            );
        }
    }

    #[test]
    fn a_row_that_is_not_an_object_is_refused() {
        let error = BenchmarkResult::from_json(&Json::Uint(1)).expect_err("not a row");
        assert_eq!(error.field(), Some("<row>"));
    }

    #[test]
    fn type_mismatches_name_the_field() {
        let row = sample_result().to_json();
        let broken = Json::Object(
            row.as_object()
                .expect("object")
                .clone()
                .with("sample_count", Json::string("many")),
        );
        let error = BenchmarkResult::from_json(&broken).expect_err("string is not a count");
        assert_eq!(
            error,
            SchemaError::TypeMismatch {
                field: "sample_count".to_owned(),
                expected: "non-negative integer",
                found: "string",
            }
        );
    }

    #[test]
    fn sample_arrays_report_the_offending_index() {
        let row = sample_result().to_json();
        let broken = Json::Object(
            row.as_object()
                .expect("object")
                .clone()
                .with("samples_ns", Json::array([Json::Uint(1), Json::Float(2.5)])),
        );
        let error = BenchmarkResult::from_json(&broken).expect_err("floats are not nanoseconds");
        assert_eq!(error.field(), Some("samples_ns[1]"));
    }

    #[test]
    fn unknown_cache_words_are_refused() {
        let row = sample_result().to_json();
        let broken = Json::Object(
            row.as_object()
                .expect("object")
                .clone()
                .with("cache_state", Json::string("whatever the machine had")),
        );
        let error = BenchmarkResult::from_json(&broken).expect_err("closed vocabulary");
        assert!(matches!(error, SchemaError::UnknownValue { .. }));
    }

    #[test]
    fn a_hand_edited_verified_flag_is_caught() {
        let row = sample_result().to_json();
        let verification = row
            .get_path("verification")
            .and_then(Json::as_object)
            .expect("section")
            .clone()
            .with("verified", Json::Bool(false));
        let broken = Json::Object(
            row.as_object()
                .expect("object")
                .clone()
                .with("verification", Json::Object(verification)),
        );
        let error = BenchmarkResult::from_json(&broken).expect_err("flag contradicts digests");
        assert!(matches!(error, SchemaError::Inconsistent { .. }));
    }

    #[test]
    fn a_row_with_no_storage_section_decodes() {
        let decoded = BenchmarkResult::from_json(&sample_result().to_json()).expect("decodes");
        assert_eq!(decoded.storage, None);
    }

    /// The optional section's own contract: once `storage` is there, every field
    /// inside it is required, refused one at a time by its own name.
    #[test]
    fn every_storage_field_is_rejected_when_missing() {
        let complete = sample_storage_result().to_json();
        for field in OPTIONAL_STORAGE_FIELDS {
            if *field == "storage" {
                // Removing the whole section is legal — that is what optional means.
                let without = complete.without_path(field);
                let decoded =
                    BenchmarkResult::from_json(&without).expect("optional means optional");
                assert_eq!(decoded.storage, None);
                continue;
            }
            let incomplete = complete.without_path(field);
            let error = BenchmarkResult::from_json(&incomplete).expect_err(&format!(
                "`{field}` must be required once storage is present"
            ));
            assert_eq!(error, SchemaError::missing(*field));
        }
    }

    #[test]
    fn a_hand_edited_amplification_is_caught() {
        let row = sample_storage_result().to_json();
        let storage = row
            .get_path("storage")
            .and_then(Json::as_object)
            .expect("section")
            .clone()
            .with("amplification_per_mille", Json::Uint(1_000));
        let broken = Json::Object(
            row.as_object()
                .expect("object")
                .clone()
                .with("storage", Json::Object(storage)),
        );
        let error = BenchmarkResult::from_json(&broken).expect_err("the ratio is recomputed");
        assert!(matches!(error, SchemaError::Inconsistent { .. }), "{error}");
    }

    #[test]
    fn a_storage_section_that_is_not_an_object_is_refused_rather_than_ignored() {
        let row = sample_result().to_json();
        let broken = Json::Object(
            row.as_object()
                .expect("object")
                .clone()
                .with("storage", Json::string("lots")),
        );
        let error = BenchmarkResult::from_json(&broken).expect_err("a string is not a section");
        assert_eq!(error.field(), Some("storage"));
    }

    #[test]
    fn unknown_storage_boundaries_are_refused() {
        let row = sample_storage_result().to_json();
        let storage = row
            .get_path("storage")
            .and_then(Json::as_object)
            .expect("section")
            .clone()
            .with("boundary", Json::string("somewhere"));
        let broken = Json::Object(
            row.as_object()
                .expect("object")
                .clone()
                .with("storage", Json::Object(storage)),
        );
        let error = BenchmarkResult::from_json(&broken).expect_err("closed vocabulary");
        assert!(matches!(error, SchemaError::UnknownValue { .. }));
    }

    #[test]
    fn text_decoding_reports_parse_failures_separately() {
        let error = BenchmarkResult::from_json_text("{").expect_err("not JSON");
        assert!(matches!(error, DecodeError::Parse(_)));
        let error = BenchmarkResult::from_json_text("{}").expect_err("not a row");
        assert!(matches!(error, DecodeError::Schema(_)));
    }
}
