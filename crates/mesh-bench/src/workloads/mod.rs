//! Built-in workloads and the registry that names them.
//!
//! The registry is the extension seam: a benchmark crate registers its workload
//! under a name, the runner builds it by that name from a seed and a parameter
//! object, and the harness never learns what any of them do. Nothing here is
//! special-cased for the built-in workload.

pub mod blob_scan;
pub mod digest;
pub mod storage_amplification;

use crate::json::JsonObject;
use crate::workload::{Workload, WorkloadError};
use blob_scan::{BlobScan, BlobScanParameters};
use std::collections::BTreeMap;
use storage_amplification::{StorageAmplification, StorageAmplificationParameters, WORKLOAD_NAME};

/// How a workload is built: from a seed and its own parameter object.
pub type WorkloadFactory = fn(u64, &JsonObject) -> Result<Box<dyn Workload>, WorkloadError>;

/// The name-to-factory map the runner resolves `--workload` against.
#[derive(Clone, Default)]
pub struct WorkloadRegistry {
    factories: BTreeMap<String, WorkloadFactory>,
}

impl std::fmt::Debug for WorkloadRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkloadRegistry")
            .field("names", &self.names())
            .finish()
    }
}

impl WorkloadRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        WorkloadRegistry::default()
    }

    /// The registry the runner ships with.
    pub fn builtin() -> Self {
        WorkloadRegistry::new()
            .register("blob-scan", |seed, parameters| {
                let parameters = if parameters.is_empty() {
                    BlobScanParameters::default()
                } else {
                    BlobScanParameters::from_json(parameters)?
                };
                Ok(Box::new(BlobScan::new(seed, parameters)))
            })
            .register(WORKLOAD_NAME, |seed, parameters| {
                Ok(Box::new(StorageAmplification::new(
                    seed,
                    StorageAmplificationParameters::from_json(parameters)?,
                )))
            })
    }

    /// Returns a registry with `name` added.
    #[must_use]
    pub fn register(mut self, name: impl Into<String>, factory: WorkloadFactory) -> Self {
        self.factories.insert(name.into(), factory);
        self
    }

    /// The registered names, sorted.
    pub fn names(&self) -> Vec<&str> {
        self.factories.keys().map(String::as_str).collect()
    }

    /// Builds a workload by name.
    pub fn build(
        &self,
        name: &str,
        seed: u64,
        parameters: &JsonObject,
    ) -> Result<Box<dyn Workload>, WorkloadError> {
        let factory = self.factories.get(name).ok_or_else(|| {
            WorkloadError::new(format!(
                "unknown workload `{name}`; registered workloads: {}",
                self.names().join(", ")
            ))
        })?;
        factory(seed, parameters)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::Json;

    #[test]
    fn the_builtin_registry_exposes_the_reference_workload() {
        let registry = WorkloadRegistry::builtin();
        assert_eq!(registry.names(), vec!["blob-scan", "storage-amplification"]);
        let mut workload = registry
            .build("blob-scan", 1, &JsonObject::new())
            .expect("defaults apply when no parameters are given");
        assert!(workload.verify().expect("verifies").passed());
    }

    #[test]
    fn unknown_names_list_what_is_available() {
        let Err(error) = WorkloadRegistry::builtin().build("nope", 1, &JsonObject::new()) else {
            panic!("an unknown workload must not build");
        };
        assert!(error.to_string().contains("blob-scan"));
    }

    #[test]
    fn registration_extends_without_touching_the_core() {
        let registry = WorkloadRegistry::builtin().register("blob-scan-tiny", |seed, _| {
            Ok(Box::new(BlobScan::new(
                seed,
                BlobScanParameters {
                    blob_count: 1,
                    blob_bytes: 8,
                },
            )))
        });
        assert_eq!(
            registry.names(),
            vec!["blob-scan", "blob-scan-tiny", "storage-amplification"]
        );
    }

    #[test]
    fn parameters_reach_the_workload() {
        let parameters = JsonObject::new()
            .with("blob_count", Json::Uint(2))
            .with("blob_bytes", Json::Uint(16));
        let workload = WorkloadRegistry::builtin()
            .build("blob-scan", 5, &parameters)
            .expect("builds");
        let descriptor = workload.descriptor();
        assert_eq!(descriptor.seed, 5);
        assert_eq!(
            descriptor
                .parameters
                .get("blob_bytes")
                .and_then(Json::as_u64),
            Some(16)
        );
    }

    #[test]
    fn the_storage_workload_is_reachable_from_the_binary_s_registry() {
        // Criterion five of 01KZE5FDN0NPGJ6NQ1NBYRFVH0: the storage row goes
        // through the same door as every other row, so it inherits the same
        // refusals. A workload that needed its own entry point would not.
        let registry = WorkloadRegistry::builtin();
        let workload = registry
            .build(
                "storage-amplification",
                42,
                &JsonObject::new().with("scale", Json::string("smoke")),
            )
            .expect("the storage workload is registered");
        assert_eq!(workload.descriptor().generator, "mesh-bench/corpus/W7");
    }

    #[test]
    fn malformed_parameters_are_rejected_at_build_time() {
        let parameters = JsonObject::new().with("blob_count", Json::string("many"));
        let Err(error) = WorkloadRegistry::builtin().build("blob-scan", 5, &parameters) else {
            panic!("a string is not a blob count");
        };
        assert!(error.to_string().contains("blob_count"));
    }
}
