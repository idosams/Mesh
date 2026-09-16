//! The declared required-field contract.
//!
//! This list is the published one: `mesh-bench fields` prints it, the schema
//! self-check walks it, and the tests remove each entry in turn and assert the
//! decoder refuses the row by name. Adding a field to the row without adding it
//! here fails the encoder/contract test in [`super::encode`]; adding it here
//! without teaching the decoder fails the rejection test. The two-sided check is
//! the point — the contract cannot rot quietly.

/// Every path a valid result row must carry, in wire order.
///
/// Section names (`repository`, `hardware`, …) are listed alongside their
/// leaves: dropping a whole section is as much a missing-metadata failure as
/// dropping one field, and it is rejected by the section's own name.
pub const REQUIRED_FIELDS: &[&str] = &[
    "schema_version",
    "benchmark_id",
    "invocation",
    "recorded_at_unix_ms",
    "repository",
    "repository.remote",
    "repository.commit",
    "repository.dirty",
    "hardware",
    "hardware.cpu_model",
    "hardware.physical_cores",
    "hardware.logical_cores",
    "hardware.memory_bytes",
    "platform",
    "platform.os",
    "platform.os_version",
    "platform.arch",
    "platform.filesystem",
    "build",
    "build.profile",
    "build.opt_level",
    "build.debug_info",
    "build.rustc_version",
    "build.target_triple",
    "workload",
    "workload.generator",
    "workload.generator_version",
    "workload.seed",
    "workload.parameters",
    "cache_state",
    "sample_count",
    "iterations_attempted",
    "failure_count",
    "samples_ns",
    "latency",
    "latency.min_ns",
    "latency.p50_ns",
    "latency.p95_ns",
    "latency.p99_ns",
    "latency.max_ns",
    "latency.mean_ns",
    "verification",
    "verification.method",
    "verification.expected_digest",
    "verification.observed_digest",
    "verification.verified",
];

/// The one optional section, and every field it must carry once it is present.
///
/// "Optional" is about the *section*, never about a field inside it. A workload
/// that admits no bytes omits `storage` entirely; a workload that admits bytes
/// and then omits `storage.distinct_content_bytes` has published a numerator
/// with no denominator, and the decoder refuses it by name exactly as it refuses
/// a missing percentile. The tests below and in [`super::decode`] check both
/// halves, because an optional tier that quietly tolerates half a section is how
/// the required tier stops meaning anything.
pub const OPTIONAL_STORAGE_FIELDS: &[&str] = &[
    "storage",
    "storage.boundary",
    "storage.granularity",
    "storage.admitted_bytes",
    "storage.distinct_content_bytes",
    "storage.amplification_per_mille",
];

/// The leaves of [`OPTIONAL_STORAGE_FIELDS`], in wire order.
pub fn optional_storage_leaf_fields() -> impl Iterator<Item = &'static str> {
    OPTIONAL_STORAGE_FIELDS
        .iter()
        .copied()
        .filter(|path| path.contains('.'))
}

/// The subset of [`REQUIRED_FIELDS`] that are leaves rather than sections.
pub fn required_leaf_fields() -> impl Iterator<Item = &'static str> {
    REQUIRED_FIELDS
        .iter()
        .copied()
        .filter(|path| !REQUIRED_FIELDS.iter().any(|other| is_child(other, path)))
}

fn is_child(candidate: &str, parent: &str) -> bool {
    candidate
        .strip_prefix(parent)
        .is_some_and(|rest| rest.starts_with('.'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn the_contract_has_no_duplicates() {
        let unique: BTreeSet<&str> = REQUIRED_FIELDS.iter().copied().collect();
        assert_eq!(unique.len(), REQUIRED_FIELDS.len());
    }

    #[test]
    fn every_leaf_names_a_declared_section() {
        for path in REQUIRED_FIELDS {
            if let Some((section, _)) = path.rsplit_once('.') {
                assert!(
                    REQUIRED_FIELDS.contains(&section),
                    "`{path}` sits under undeclared section `{section}`"
                );
            }
        }
    }

    #[test]
    fn leaves_exclude_sections() {
        let leaves: Vec<&str> = required_leaf_fields().collect();
        assert!(!leaves.contains(&"repository"));
        assert!(leaves.contains(&"repository.commit"));
        assert!(leaves.contains(&"cache_state"));
    }

    #[test]
    fn the_optional_section_is_not_also_required() {
        for path in OPTIONAL_STORAGE_FIELDS {
            assert!(
                !REQUIRED_FIELDS.contains(path),
                "`{path}` cannot be optional and required at once"
            );
        }
    }

    #[test]
    fn the_optional_section_declares_its_own_leaves() {
        let leaves: Vec<&str> = optional_storage_leaf_fields().collect();
        assert!(!leaves.contains(&"storage"));
        assert!(leaves.contains(&"storage.admitted_bytes"));
        assert!(leaves.contains(&"storage.distinct_content_bytes"));
        assert_eq!(leaves.len(), OPTIONAL_STORAGE_FIELDS.len() - 1);
    }

    #[test]
    fn the_contract_covers_every_publishable_dimension() {
        for required in [
            "repository.commit",
            "hardware.cpu_model",
            "platform.os",
            "platform.filesystem",
            "build.profile",
            "workload.generator",
            "cache_state",
            "sample_count",
            "samples_ns",
            "latency.p50_ns",
            "latency.p95_ns",
            "latency.p99_ns",
            "failure_count",
            "verification.verified",
        ] {
            assert!(
                REQUIRED_FIELDS.contains(&required),
                "`{required}` dropped out of the published contract"
            );
        }
    }
}
