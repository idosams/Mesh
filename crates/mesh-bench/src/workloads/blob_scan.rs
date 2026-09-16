//! The reference workload: scan generated blobs and digest them.
//!
//! It exists to exercise the harness end to end — cold and warm preparation, a
//! correctness gate that can be made to fail on demand, a stable descriptor —
//! not to say anything about Mesh's performance. Real Mesh benchmarks implement
//! the same [`Workload`] trait; this one is the instrument's self-test, and the
//! only workload whose numbers mean nothing.
//!
//! Cache semantics, stated plainly, because the earlier wording here said the
//! opposite of what the code does and a benchmark's cache story is not a place
//! to be approximately right.
//!
//! The data lives in memory. [`CacheState::Cold`] drops the resident blobs in
//! `prepare`, so the next [`Workload::iterate`] finds nothing and regenerates
//! them — **inside the timed section**, which is the whole point: a cold sample
//! is meant to include the cost of making the data available. [`CacheState::Warm`]
//! generates once in `prepare` and every timed sample then rescans the same
//! resident bytes.
//!
//! So a cold sample is legitimately *slower* than a warm one here, exactly as a
//! storage-backed workload's cold sample pays the page-cache miss inside its
//! timed section. That workload implements `prepare(Cold)` by evicting the page
//! cache rather than dropping a `Vec`; the harness contract is identical, and
//! `cold_pays_regeneration_inside_the_timed_section` below pins it.

use super::digest::{digest_blobs, Fnv1a, SeededGenerator, GENERATOR_NAME, GENERATOR_VERSION};
use crate::json::{Json, JsonObject};
use crate::schema::{CacheState, Verification, WorkloadDescriptor};
use crate::workload::{Workload, WorkloadError};

/// Shape of the generated data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlobScanParameters {
    /// How many blobs to generate.
    pub blob_count: usize,
    /// How many bytes each blob carries.
    pub blob_bytes: usize,
}

impl Default for BlobScanParameters {
    fn default() -> Self {
        BlobScanParameters {
            blob_count: 64,
            blob_bytes: 4096,
        }
    }
}

impl BlobScanParameters {
    /// Reads the parameters out of a generic parameter object.
    pub fn from_json(parameters: &JsonObject) -> Result<Self, WorkloadError> {
        Ok(BlobScanParameters {
            blob_count: usize_field(parameters, "blob_count")?,
            blob_bytes: usize_field(parameters, "blob_bytes")?,
        })
    }

    /// The parameter object recorded in the result row.
    pub fn to_json(self) -> JsonObject {
        JsonObject::new()
            .with("blob_count", Json::Uint(self.blob_count as u64))
            .with("blob_bytes", Json::Uint(self.blob_bytes as u64))
    }
}

fn usize_field(parameters: &JsonObject, key: &str) -> Result<usize, WorkloadError> {
    let value = parameters
        .get(key)
        .ok_or_else(|| WorkloadError::new(format!("missing workload parameter `{key}`")))?
        .as_u64()
        .ok_or_else(|| {
            WorkloadError::new(format!("workload parameter `{key}` must be a whole number"))
        })?;
    usize::try_from(value)
        .map_err(|_| WorkloadError::new(format!("workload parameter `{key}` is too large")))
}

/// Scans deterministic blobs and folds them into a digest.
#[derive(Clone, Debug)]
pub struct BlobScan {
    seed: u64,
    parameters: BlobScanParameters,
    blobs: Option<Vec<Vec<u8>>>,
    corrupt: bool,
    /// How many times the data has been generated.
    ///
    /// Observability, not state the workload reads: it is what lets a test say
    /// *where* regeneration happens rather than inferring it from a duration,
    /// which on a shared machine is a coin toss.
    generations: u64,
}

impl BlobScan {
    /// Builds the workload for a seed and a data shape.
    pub fn new(seed: u64, parameters: BlobScanParameters) -> Self {
        BlobScan {
            seed,
            parameters,
            blobs: None,
            corrupt: false,
            generations: 0,
        }
    }

    /// How many times this workload has generated its data.
    pub fn generations(&self) -> u64 {
        self.generations
    }

    /// Returns a copy that computes the wrong answer.
    ///
    /// Used to prove the correctness gate actually gates: a corrupt workload
    /// must never produce a timing number.
    #[must_use]
    pub fn corrupted(self) -> Self {
        BlobScan {
            corrupt: true,
            ..self
        }
    }

    fn generate(&mut self) -> Vec<Vec<u8>> {
        self.generations += 1;
        SeededGenerator::new(self.seed)
            .blobs(self.parameters.blob_count, self.parameters.blob_bytes)
    }

    /// The timed routine: a streaming fold over the resident blobs.
    fn scan(&self, blobs: &[Vec<u8>]) -> String {
        let mut digest = Fnv1a::new();
        for blob in blobs {
            for chunk in blob.chunks(64) {
                digest.update(chunk);
            }
        }
        if self.corrupt {
            digest.update(b"corrupt");
        }
        digest.finish_hex()
    }

    fn resident(&mut self) -> &[Vec<u8>] {
        if self.blobs.is_none() {
            let blobs = self.generate();
            self.blobs = Some(blobs);
        }
        self.blobs.as_deref().unwrap_or_default()
    }
}

impl Workload for BlobScan {
    fn descriptor(&self) -> WorkloadDescriptor {
        WorkloadDescriptor {
            generator: GENERATOR_NAME.to_owned(),
            generator_version: GENERATOR_VERSION.to_owned(),
            seed: self.seed,
            parameters: self.parameters.to_json(),
        }
    }

    fn verify(&mut self) -> Result<Verification, WorkloadError> {
        let blobs = self.generate();
        // Expected: the straightforward whole-blob digest. Observed: the
        // chunked routine the benchmark actually times. Two independent paths
        // over the same bytes — if they disagree, the fast path is wrong and
        // its speed is irrelevant.
        let expected = digest_blobs(&blobs);
        let observed = self.scan(&blobs);
        Ok(Verification::new("blob-digest-fnv1a64", expected, observed))
    }

    fn prepare(&mut self, cache_state: CacheState) -> Result<(), WorkloadError> {
        match cache_state {
            CacheState::Cold => self.blobs = None,
            CacheState::Warm => {
                let _ = self.resident();
            }
        }
        Ok(())
    }

    fn iterate(&mut self) -> Result<(), WorkloadError> {
        // Cold: `prepare` left nothing resident, so this regenerates — and the
        // caller has already read the clock. That cost is deliberately inside
        // the sample; see the module comment.
        let blobs = match std::mem::take(&mut self.blobs) {
            Some(blobs) => blobs,
            None => self.generate(),
        };
        let digest = self.scan(&blobs);
        self.blobs = Some(blobs);
        if digest.is_empty() {
            return Err(WorkloadError::new("digest routine produced nothing"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workload() -> BlobScan {
        BlobScan::new(
            7,
            BlobScanParameters {
                blob_count: 4,
                blob_bytes: 128,
            },
        )
    }

    #[test]
    fn the_reference_workload_verifies() {
        let verification = workload().verify().expect("verification runs");
        assert!(verification.passed(), "{verification:?}");
        assert_eq!(verification.method, "blob-digest-fnv1a64");
    }

    #[test]
    fn a_corrupted_workload_fails_verification() {
        let verification = workload().corrupted().verify().expect("verification runs");
        assert!(!verification.passed());
        assert_ne!(verification.expected_digest, verification.observed_digest);
    }

    #[test]
    fn cold_preparation_drops_the_resident_data() {
        let mut scan = workload();
        scan.prepare(CacheState::Warm).expect("warm");
        assert!(scan.blobs.is_some());
        scan.prepare(CacheState::Cold).expect("cold");
        assert!(scan.blobs.is_none());
    }

    #[test]
    fn iterating_from_cold_regenerates_the_data() {
        let mut scan = workload();
        scan.prepare(CacheState::Cold).expect("cold");
        scan.iterate().expect("iteration succeeds");
        assert!(scan.blobs.is_some());
    }

    /// The module comment used to claim cold paid regeneration *outside* the
    /// timed section. It never did. This pins the true semantics so the comment
    /// and the code cannot drift apart again silently.
    #[test]
    fn cold_pays_regeneration_inside_the_timed_section() {
        let mut scan = workload();
        scan.prepare(CacheState::Cold).expect("cold");
        let before = scan.generations();
        scan.iterate().expect("iteration succeeds");
        assert_eq!(
            scan.generations(),
            before + 1,
            "a cold sample must regenerate inside iterate(), which is what the clock brackets"
        );
    }

    #[test]
    fn warm_pays_regeneration_outside_the_timed_section() {
        let mut scan = workload();
        scan.prepare(CacheState::Warm).expect("warm");
        let before = scan.generations();
        for _ in 0..3 {
            scan.iterate().expect("iteration succeeds");
        }
        assert_eq!(
            scan.generations(),
            before,
            "warm samples rescan resident bytes; generating inside one would time the wrong thing"
        );
    }

    #[test]
    fn preparing_warm_generates_exactly_once() {
        let mut scan = workload();
        let before = scan.generations();
        scan.prepare(CacheState::Warm).expect("warm");
        scan.prepare(CacheState::Warm).expect("warm again");
        assert_eq!(scan.generations(), before + 1);
    }

    #[test]
    fn parameters_round_trip_through_json() {
        let parameters = BlobScanParameters {
            blob_count: 3,
            blob_bytes: 17,
        };
        let decoded = BlobScanParameters::from_json(&parameters.to_json()).expect("round trip");
        assert_eq!(decoded, parameters);
    }

    #[test]
    fn missing_parameters_are_named() {
        let error =
            BlobScanParameters::from_json(&JsonObject::new().with("blob_count", Json::Uint(1)))
                .expect_err("blob_bytes is required");
        assert!(error.to_string().contains("blob_bytes"));
    }

    #[test]
    fn the_descriptor_carries_the_generator_identity() {
        let descriptor = workload().descriptor();
        assert_eq!(descriptor.generator, GENERATOR_NAME);
        assert_eq!(descriptor.generator_version, GENERATOR_VERSION);
        assert_eq!(descriptor.seed, 7);
        assert_eq!(
            descriptor
                .parameters
                .get("blob_count")
                .and_then(Json::as_u64),
            Some(4)
        );
    }
}
