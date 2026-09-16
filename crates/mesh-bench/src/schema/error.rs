//! The rejection vocabulary of the result schema.
//!
//! Every rejection names the offending field by its dotted path, because the
//! only useful thing to tell someone whose benchmark row was refused is which
//! field to go and capture.

use std::fmt;

/// Why a candidate result row is not a result row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SchemaError {
    /// A required field is absent.
    MissingField {
        /// Dotted path of the missing field, e.g. `hardware.cpu_model`.
        field: String,
    },
    /// A required field is present but has the wrong JSON type.
    TypeMismatch {
        /// Dotted path of the field.
        field: String,
        /// The JSON type the schema requires.
        expected: &'static str,
        /// The JSON type actually found.
        found: &'static str,
    },
    /// A field is well-typed but carries an impossible value.
    OutOfRange {
        /// Dotted path of the field.
        field: String,
        /// What was wrong with the value.
        detail: String,
    },
    /// An enumerated field carries a word outside its vocabulary.
    UnknownValue {
        /// Dotted path of the field.
        field: String,
        /// The word that was found.
        value: String,
    },
    /// Two fields contradict each other (declared percentiles versus raw
    /// samples, sample count versus array length, and so on).
    Inconsistent {
        /// What contradicts what.
        detail: String,
    },
    /// The row carries timing numbers for a run whose correctness verification
    /// did not pass. This can only happen if timing code ran ahead of, or
    /// instead of, verification — the row is refused rather than filed.
    UnverifiedRun {
        /// How correctness was checked.
        method: String,
        /// The digest the workload was supposed to produce.
        expected_digest: String,
        /// The digest it actually produced.
        observed_digest: String,
    },
    /// The row declares a schema version this build cannot interpret.
    UnsupportedSchemaVersion {
        /// The version found in the row.
        found: String,
        /// The version this build writes and reads.
        expected: &'static str,
    },
}

impl SchemaError {
    /// The dotted field path this error is about, when it is about one field.
    pub fn field(&self) -> Option<&str> {
        match self {
            SchemaError::MissingField { field }
            | SchemaError::TypeMismatch { field, .. }
            | SchemaError::OutOfRange { field, .. }
            | SchemaError::UnknownValue { field, .. } => Some(field),
            SchemaError::Inconsistent { .. }
            | SchemaError::UnverifiedRun { .. }
            | SchemaError::UnsupportedSchemaVersion { .. } => None,
        }
    }

    /// Builds a [`SchemaError::MissingField`] for `field`.
    pub fn missing(field: impl Into<String>) -> Self {
        SchemaError::MissingField {
            field: field.into(),
        }
    }

    /// Builds a [`SchemaError::OutOfRange`] for `field`.
    pub fn out_of_range(field: impl Into<String>, detail: impl Into<String>) -> Self {
        SchemaError::OutOfRange {
            field: field.into(),
            detail: detail.into(),
        }
    }

    /// Builds a [`SchemaError::Inconsistent`] error.
    pub fn inconsistent(detail: impl Into<String>) -> Self {
        SchemaError::Inconsistent {
            detail: detail.into(),
        }
    }
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SchemaError::MissingField { field } => {
                write!(f, "required field `{field}` is missing")
            }
            SchemaError::TypeMismatch {
                field,
                expected,
                found,
            } => write!(f, "field `{field}` must be a {expected}, found a {found}"),
            SchemaError::OutOfRange { field, detail } => {
                write!(f, "field `{field}` is out of range: {detail}")
            }
            SchemaError::UnknownValue { field, value } => {
                write!(f, "field `{field}` carries unknown value `{value}`")
            }
            SchemaError::Inconsistent { detail } => {
                write!(f, "row is self-contradictory: {detail}")
            }
            SchemaError::UnverifiedRun {
                method,
                expected_digest,
                observed_digest,
            } => write!(
                f,
                "correctness verification `{method}` failed \
                 (expected `{expected_digest}`, observed `{observed_digest}`): \
                 a failed verification must produce no timing number"
            ),
            SchemaError::UnsupportedSchemaVersion { found, expected } => {
                write!(f, "schema version `{found}` is not `{expected}`")
            }
        }
    }
}

impl std::error::Error for SchemaError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_paths_survive_display() {
        let error = SchemaError::missing("hardware.cpu_model");
        assert_eq!(error.field(), Some("hardware.cpu_model"));
        assert!(error.to_string().contains("hardware.cpu_model"));
    }

    #[test]
    fn cross_field_errors_have_no_single_field() {
        assert_eq!(SchemaError::inconsistent("counts disagree").field(), None);
    }

    #[test]
    fn unverified_runs_say_why_there_is_no_number() {
        let error = SchemaError::UnverifiedRun {
            method: "digest".to_owned(),
            expected_digest: "a".to_owned(),
            observed_digest: "b".to_owned(),
        };
        assert!(error.to_string().contains("no timing number"));
    }
}
