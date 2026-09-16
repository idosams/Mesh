//! The result schema: what a Mesh benchmark number must carry to exist.
//!
//! The rule the rest of the crate is built around: **a row missing any required
//! field is rejected at write time**, by name, before it can reach a file, a
//! report or a slide. A benchmark corpus degrades one convenient omission at a
//! time, so there is no "optional metadata" tier and no way to file a partial
//! row "for now".

mod decode;
mod encode;
mod error;
mod fields;
mod result;

pub use decode::DecodeError;
pub use error::SchemaError;
pub use fields::{
    optional_storage_leaf_fields, required_leaf_fields, OPTIONAL_STORAGE_FIELDS, REQUIRED_FIELDS,
};
pub use result::{
    amplification_per_mille, BenchmarkResult, BuildProfile, CacheState, Hardware, HostPlatform,
    LatencySummary, Repository, StorageBoundary, StorageFootprint, Verification,
    WorkloadDescriptor, SCHEMA_VERSION,
};
