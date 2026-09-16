//! Performance counters for the metric families of plan §12.3.
//!
//! # Why the system counts rather than the harness estimating
//!
//! A benchmark that cannot ask the system what it did has to infer it from the outside, and an
//! inference carries its own error into every number built on it. These counters are the inside
//! answer: the process states what it observed, in counts and bytes, and the harness reads them
//! instead of approximating them. They are also the answer to the other half of plan §3.5's
//! OBS-004 — production behaviour is visible from a running daemon, with no benchmark involved.
//!
//! # Deterministic where it can be
//!
//! Counts and bytes, not wall-clock, wherever the metric admits one. Two wall-clock budget
//! assertions on this repository's merge path have already failed under machine load, and a gate
//! that fails on load teaches its readers to re-run rather than read. So a latency metric carries
//! **both** an exact operation count and a nanosecond total, [`catalogue::Unit`] decides
//! [`catalogue::Determinism`], and `Determinism` decides the [`catalogue::Band`] that
//! `crates/mesh-daemon/tests/counters.rs` holds a cross-validation to. Nothing on this surface is
//! a percentage tolerance on a wall-clock number.
//!
//! The conditions a timing number is taken under are not restated here: [`CONDITIONS`] names
//! `benchmarks/runners/README.md`, and every snapshot carries that sentence ahead of its numbers.
//!
//! # Four things, in the order a reader needs them
//!
//! 1. [`catalogue`] — the two tables: plan §12.3's metrics, and the counters behind them.
//! 2. [`coverage`] — the edge between them. [`gaps`] is empty or it says exactly what is missing.
//! 3. [`Counters`] — the registry. Five atomic updates per observation, no allocation, and
//!    an [`Overhead`] that is a number rather than the word "negligible".
//! 4. [`CounterSnapshot`] — one reading of everything, with the collection cost and the list of
//!    counters nothing in this build feeds, next to the values rather than in a footnote.
//!
//! ```
//! use mesh_daemon::counters::{CounterId, Counters};
//!
//! let counters = Counters::new();
//! let id = CounterId::of("workspace_operations.index_reconstruction.ops").expect("in the catalogue");
//! counters.record(id, 1);
//!
//! let snapshot = counters.snapshot();
//! assert_eq!(snapshot.reading(id.spec().key).expect("read back").observations, 1);
//! assert_eq!(snapshot.collection.concurrent_observations, 0);
//! ```
//!
//! # What is deliberately not here
//!
//! **One read-only method on [`crate::ipc::METHODS`].** `performance.counters` publishes
//! [`CounterSnapshot::to_json`] through surface version 4. The daemon contract and desktop mirror
//! are compared for exact equality from both sides, so adding a row to one without the other turns
//! a test red.

pub mod catalogue;
pub mod coverage;
mod registry;
mod snapshot;

pub use catalogue::{
    counter_count, counters, required_metric_count, required_metrics, spec, Band, CounterId,
    CounterSpec, Determinism, Family, Producer, RequiredMetric, Unit,
};
pub use coverage::{
    catalogue_gaps, deterministically_named, gaps, wired_required_metric_count, Gap,
};
pub use registry::{
    Counters, Overhead, ALLOCATIONS_PER_OBSERVATION, ATOMIC_WRITES_PER_GROUP,
    ATOMIC_WRITES_PER_OBSERVATION,
};
pub use snapshot::{Collection, CounterSnapshot, Reading, CONDITIONS, INTEGER_ENCODING};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_documented_example_is_the_behaviour() {
        let counters = Counters::new();
        let id = CounterId::of("workspace_operations.index_reconstruction.ops")
            .expect("the wired counter is in the catalogue");
        counters.record(id, 1);
        let snapshot = counters.snapshot();
        assert_eq!(
            snapshot
                .reading("workspace_operations.index_reconstruction.ops")
                .expect("read back")
                .observations,
            1
        );
        assert_eq!(snapshot.collection.concurrent_observations, 0);
    }

    #[test]
    fn the_catalogue_names_plan_12_3_and_reports_its_unwired_remainder() {
        assert!(catalogue_gaps().is_empty(), "{:?}", catalogue_gaps());
        assert_eq!(deterministically_named(), required_metric_count());
        assert_eq!(wired_required_metric_count(), 4);
        assert_eq!(gaps().len(), required_metric_count() - 4);
        assert!(counter_count() > 0);
    }
}
