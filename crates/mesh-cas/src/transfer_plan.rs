//! Deterministic bounded requests from manifest-required digests and verified local availability.
//!
//! Presence at a content name is not availability: [`crate::Cas::contains`] deliberately performs
//! no integrity check. This planner therefore asks [`crate::Cas::begin_receive`] for every unique
//! required digest. That verifies promoted bytes, reads the durable partial offset for absent
//! content, and quarantines a corrupt promoted chunk before the planner requests it again from
//! zero. The output is sorted and bounded so it can be projected directly into content-plane
//! request batches without making transport order another source of truth.

use core::fmt;
use std::path::PathBuf;

use crate::{Cas, CasError, ContentDigest, Digest32, DurableFs, ReceiveError};

/// One missing chunk request, ready to project onto the content-plane wire type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlannedChunkRequest {
    digest: Digest32,
    from_offset: u64,
    max_bytes: u64,
}

impl PlannedChunkRequest {
    /// The required content digest.
    #[must_use]
    pub const fn digest(self) -> Digest32 {
        self.digest
    }

    /// The durable offset after which the peer should continue.
    #[must_use]
    pub const fn from_offset(self) -> u64 {
        self.from_offset
    }

    /// The largest part this receiver asked for.
    #[must_use]
    pub const fn max_bytes(self) -> u64 {
        self.max_bytes
    }
}

/// A request batch whose length is bounded by the caller's negotiated transport limit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferBatch {
    requests: Vec<PlannedChunkRequest>,
}

impl TransferBatch {
    /// The sorted requests in this batch.
    #[must_use]
    pub fn requests(&self) -> &[PlannedChunkRequest] {
        &self.requests
    }
}

/// A promoted chunk found under the right name with the wrong bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetectedCorruption {
    digest: Digest32,
    found: Digest32,
    quarantined: PathBuf,
}

impl DetectedCorruption {
    /// The content name whose bytes failed verification.
    #[must_use]
    pub const fn digest(&self) -> Digest32 {
        self.digest
    }

    /// The digest the stored bytes actually produced.
    #[must_use]
    pub const fn found(&self) -> Digest32 {
        self.found
    }

    /// The retained sample, so repeated corruption is an observable series rather than a loop.
    #[must_use]
    pub fn quarantined(&self) -> &std::path::Path {
        &self.quarantined
    }
}

/// Verified local availability and bounded requests for everything else.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferPlan {
    verified: Vec<Digest32>,
    corruptions: Vec<DetectedCorruption>,
    batches: Vec<TransferBatch>,
}

impl TransferPlan {
    /// Required chunks whose promoted bytes verified locally.
    #[must_use]
    pub fn verified(&self) -> &[Digest32] {
        &self.verified
    }

    /// Corrupt promoted chunks quarantined while this plan was computed.
    #[must_use]
    pub fn corruptions(&self) -> &[DetectedCorruption] {
        &self.corruptions
    }

    /// Missing requests split into deterministic bounded batches.
    #[must_use]
    pub fn batches(&self) -> &[TransferBatch] {
        &self.batches
    }

    /// Total missing unique chunks across all batches.
    #[must_use]
    pub fn missing_chunks(&self) -> usize {
        self.batches.iter().map(|batch| batch.requests.len()).sum()
    }
}

impl<F: DurableFs, D: ContentDigest> Cas<F, D> {
    /// Plan bounded resumable requests for the unique digests a manifest set requires.
    ///
    /// Every promoted candidate is read and hashed. A corrupt candidate is quarantined, its stale
    /// partial (if a crash left one) is discarded, and the plan both records the corruption and
    /// requests the digest from zero. Missing candidates resume at their durable partial length.
    ///
    /// # Errors
    ///
    /// Zero bounds are refused before the store is inspected. Store failures are surfaced rather
    /// than converted into missing content.
    pub fn plan_missing_chunks(
        &self,
        required: impl IntoIterator<Item = Digest32>,
        max_requests_per_batch: usize,
        max_bytes_per_part: u64,
    ) -> Result<TransferPlan, TransferPlanError> {
        if max_requests_per_batch == 0 {
            return Err(TransferPlanError::ZeroRequestsPerBatch);
        }
        if max_bytes_per_part == 0 {
            return Err(TransferPlanError::ZeroBytesPerPart);
        }

        let mut required: Vec<Digest32> = required.into_iter().collect();
        required.sort_unstable();
        required.dedup();

        let mut verified = Vec::new();
        let mut corruptions = Vec::new();
        let mut requests = Vec::new();
        for digest in required {
            match self.begin_receive(digest) {
                Ok(receiver) if receiver.is_complete() => verified.push(digest),
                Ok(receiver) => requests.push(PlannedChunkRequest {
                    digest,
                    from_offset: receiver.next_offset(),
                    max_bytes: max_bytes_per_part,
                }),
                Err(ReceiveError::Store(CasError::Corrupt {
                    digest,
                    found,
                    quarantined,
                })) => {
                    let incoming = self.layout().incoming_path(&digest);
                    if self.filesystem().exists(&incoming) {
                        self.remove_incoming(&incoming)
                            .map_err(TransferPlanError::Receive)?;
                    }
                    corruptions.push(DetectedCorruption {
                        digest,
                        found,
                        quarantined,
                    });
                    requests.push(PlannedChunkRequest {
                        digest,
                        from_offset: 0,
                        max_bytes: max_bytes_per_part,
                    });
                }
                Err(error) => return Err(TransferPlanError::Receive(error)),
            }
        }

        let batches = requests
            .chunks(max_requests_per_batch)
            .map(|requests| TransferBatch {
                requests: requests.to_vec(),
            })
            .collect();
        Ok(TransferPlan {
            verified,
            corruptions,
            batches,
        })
    }
}

/// Why a transfer plan could not be built.
#[derive(Debug)]
pub enum TransferPlanError {
    /// A zero-size request batch can never make progress.
    ZeroRequestsPerBatch,
    /// A request for zero bytes can never make progress.
    ZeroBytesPerPart,
    /// Integrity or storage inspection refused the plan.
    Receive(ReceiveError),
}

impl fmt::Display for TransferPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroRequestsPerBatch => {
                formatter.write_str("a transfer batch must allow at least one request")
            }
            Self::ZeroBytesPerPart => {
                formatter.write_str("a transfer request must allow at least one byte")
            }
            Self::Receive(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for TransferPlanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Receive(error) => Some(error),
            _ => None,
        }
    }
}
