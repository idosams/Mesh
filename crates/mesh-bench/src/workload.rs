//! The workload contract every benchmark implements.
//!
//! The ordering rule lives here rather than in each benchmark: [`Workload::verify`]
//! runs before any call to [`Workload::iterate`] is timed, and the harness is
//! the only caller of either. A workload that cannot prove it computed the right
//! answer does not get to report how fast it computed it.

use crate::schema::{CacheState, StorageFootprint, Verification, WorkloadDescriptor};
use std::fmt;

/// A benchmark's measurable unit of work, plus the metadata that describes it.
pub trait Workload {
    /// The generator, its version, its seed and its parameters.
    ///
    /// This is what lets a third party regenerate the same bytes; it is copied
    /// verbatim into the result row.
    fn descriptor(&self) -> WorkloadDescriptor;

    /// Establishes that the workload computes the right answer.
    ///
    /// Called once, before any timing, and never inside a timed section. The
    /// returned digests are compared by the harness — a workload reports what it
    /// observed and what it expected, and does not get to decide the verdict.
    fn verify(&mut self) -> Result<Verification, WorkloadError>;

    /// Puts the workload's caches into the requested state.
    ///
    /// `Cold` must discard everything the previous iteration warmed (page
    /// cache, derived indexes, memoised state); `Warm` must leave the workload
    /// primed. Called outside every timed section.
    fn prepare(&mut self, cache_state: CacheState) -> Result<(), WorkloadError>;

    /// One iteration — the only thing that is ever timed.
    fn iterate(&mut self) -> Result<(), WorkloadError>;

    /// What the workload left in durable storage, if it stores anything.
    ///
    /// Read once, **after** the last timed iteration, and never inside a timed
    /// section — counting bytes on disk is I/O, and a workload that paid for it
    /// inside `iterate` would be timing the instrument.
    ///
    /// The default is `None`, which is the honest answer for a workload that
    /// admits nothing: a section of zeroes would be a measurement nobody took.
    fn storage(&self) -> Option<StorageFootprint> {
        None
    }
}

/// A workload failed to set up, verify or run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkloadError {
    /// What went wrong, in the workload's own words.
    pub message: String,
}

impl WorkloadError {
    /// Builds an error from a message.
    pub fn new(message: impl Into<String>) -> Self {
        WorkloadError {
            message: message.into(),
        }
    }
}

impl fmt::Display for WorkloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "workload failed: {}", self.message)
    }
}

impl std::error::Error for WorkloadError {}
