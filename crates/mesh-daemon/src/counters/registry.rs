//! The registry: where an observation goes, and what it costs.
//!
//! # The whole of the hot path
//!
//! [`Counters::record`] does five atomic updates and nothing else. No allocation, no lock, no
//! syscall, no clock read, no formatting, no branch on a string. That is stated here and it is
//! **measured** twice: [`Counters::overhead`] reports the deterministic part of the cost — the
//! updates, the allocations, the bytes of state — and `cargo bench -p mesh-bench --bench
//! counter-overhead` reports the wall-clock part with its sample count and its conditions.
//! Plan §2.10 does not accept "negligible" as a number, and neither does this module.
//!
//! Five updates and not three: the fourth and fifth maintain an in-flight writer count around the
//! three counter updates. A snapshot can otherwise observe the count before the matching total
//! while the independent tally remains unchanged, and incorrectly report a clean collection.
//! Related product events use [`Counters::record_group_by_key`], which adds one outer increment
//! and decrement around their ordinary observations. That prevents a snapshot between, for
//! example, checkpoint count and checkpoint bytes from calling the half-event consistent; the two
//! additional writes are published as [`Overhead::atomic_writes_per_group`].
//! The third counter update is [`Collection::observations_recorded`], a tally kept
//! independently of the per-counter ones. It exists so that the registry can be cross-validated
//! against itself — with nothing else recording, the sum of the per-counter observation counts must
//! equal it — which is the cheapest independent measurement available and the one that catches an
//! indexing mistake that a per-counter assertion cannot see.
//!
//! # One global order for a clean snapshot verdict
//!
//! A counter is a statistic and never a product synchronisation primitive; nothing in this crate
//! reads one to decide what workspace operation to perform. Snapshot consistency is still a
//! synchronisation claim of its own, however. Per-location coherence from [`Ordering::Relaxed`]
//! cannot prove that a reader which saw one counter slot also saw the independent tally or the
//! in-flight writer which brackets it. That matters on weakly ordered CPUs: the eighty-one atomic
//! locations do not otherwise share one observation order.
//!
//! All atomics that participate in an observation or snapshot therefore use
//! [`Ordering::SeqCst`]. [`Counters::snapshot`] reads the independent tally, the active-writer
//! count, every per-counter value, the active-writer count again, and finally the tally again in
//! that single global order. [`Collection::concurrent_observations`] is the difference between the
//! tally reads; [`Collection::writers_in_flight_before_readings`] and
//! [`Collection::writers_in_flight_after_readings`] expose observations that straddled the reading
//! pass. [`Collection::snapshot_consistent`] is true only when neither kind of overlap happened
//! and the independently tallied observation count equals the sum of the per-counter counts.
//! Every published value saturates at [`u64::MAX`] rather than wrapping. Once an observation count
//! or measured total reaches that ceiling, `snapshot_consistent` stays false: a cardinality can no
//! longer prove that no observation was hidden, and a total can no longer prove that no magnitude
//! was lost. Below saturation it is true on one thread every time. Under concurrency or saturation
//! the uncertainty is reported next to the values, which is what `## Failure and recovery` in this
//! task's contract asks for. The cost of the stronger ordering remains part of the checked
//! benchmark rather than being assumed free.
//!
//! An earlier version of this file asserted that the independent tally was never below the
//! per-counter sum. It is not, on any machine anybody is likely to run, and it is not guaranteed by
//! anything — the trial merge of this change found it failing on the first concurrent run. The
//! invariant that survives is stated above; the one that did not is not restated here as a comment
//! claiming it holds.

use std::sync::atomic::{AtomicU64, Ordering};

use super::catalogue::{counter_count, CounterId};
use super::snapshot::{Collection, CounterSnapshot, Reading};

/// How many atomic updates one observation performs.
pub const ATOMIC_WRITES_PER_OBSERVATION: u64 = 5;

/// Atomic updates that keep a related group of counter observations snapshot-coherent.
pub const ATOMIC_WRITES_PER_GROUP: u64 = 2;

/// How many heap allocations one observation performs.
pub const ALLOCATIONS_PER_OBSERVATION: u64 = 0;

/// How many slots of state each counter occupies: its observation count and its total.
const SLOTS_PER_COUNTER: usize = 2;

/// Every counter in plan §12.3, and the tally that cross-validates them.
///
/// Cheap to hold — one flat array of `u64`-sized atomics, sized at construction and never resized —
/// and cheap to share, because every method takes `&self`. A daemon keeps one of these for its
/// whole lifetime.
#[derive(Debug)]
pub struct Counters {
    /// `[observations, total]` per counter, in catalogue order.
    slots: Box<[AtomicU64]>,
    /// Observations recorded across all counters, tallied independently of `slots`.
    observations: AtomicU64,
    /// Writers or related groups currently between their first and final counter update.
    in_flight: AtomicU64,
    /// How many snapshots have been taken. Collection's own footprint, reported with the values.
    snapshots: AtomicU64,
}

impl Default for Counters {
    fn default() -> Self {
        Self::new()
    }
}

impl Counters {
    /// A registry with every catalogue counter at zero.
    #[must_use]
    pub fn new() -> Self {
        let slots = (0..counter_count() * SLOTS_PER_COUNTER)
            .map(|_| AtomicU64::new(0))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            slots,
            observations: AtomicU64::new(0),
            in_flight: AtomicU64::new(0),
            snapshots: AtomicU64::new(0),
        }
    }

    /// Record one observation of `id` worth `value` in that counter's unit.
    ///
    /// `value` may be zero: an observation with nothing behind it is still an observation, and
    /// dropping it would make the observation count disagree with what happened.
    ///
    /// Saturating rather than wrapping. A counter that wraps reports a number smaller than the
    /// truth and gives no sign that it did; a counter pinned at [`u64::MAX`] is obviously wrong,
    /// and `u64::MAX` bytes is 16 exbibytes, so reaching it means something else broke first.
    pub fn record(&self, id: CounterId, value: u64) {
        self.record_with(id, value, || {});
    }

    fn record_with<F: FnOnce()>(&self, id: CounterId, value: u64, between_count_and_total: F) {
        add_saturating(&self.in_flight, 1);
        let _writer = InFlightWriter(&self.in_flight);
        // The independent tally goes first in the global order. Together with the active-writer
        // bracket, that lets a snapshot distinguish a complete observation from an overlapping one
        // without relying on timing or per-location coherence.
        add_saturating(&self.observations, 1);
        let base = id.index() * SLOTS_PER_COUNTER;
        add_saturating(&self.slots[base], 1);
        between_count_and_total();
        add_saturating(&self.slots[base + 1], value);
    }

    /// Record one observation of the counter named `key`, when the catalogue has one.
    ///
    /// Returns whether the key resolved. A linear scan of the catalogue, so this belongs at a call
    /// site that happens at human frequency — opening a workspace, finishing a recovery — and never
    /// on a path anybody measures. [`Counters::record`] is the one that is cheap.
    pub fn record_by_key(&self, key: &str, value: u64) -> bool {
        match CounterId::of(key) {
            Some(id) => {
                self.record(id, value);
                true
            }
            None => false,
        }
    }

    /// Record a related group without allowing a quiet-looking snapshot between its members.
    ///
    /// Every name is resolved before the first mutation. An unknown name therefore refuses the
    /// whole group instead of publishing a prefix. The outer writer bracket remains active while
    /// the ordinary observations land, so a concurrent snapshot reports itself inconsistent
    /// rather than presenting half of one product event as a clean reading.
    pub fn record_group_by_key(&self, entries: &[(&str, u64)]) -> bool {
        self.record_group_with(entries, || {})
    }

    fn record_group_with<F: FnOnce()>(&self, entries: &[(&str, u64)], after_first: F) -> bool {
        if entries.is_empty() || entries.iter().any(|(key, _)| CounterId::of(key).is_none()) {
            return false;
        }
        add_saturating(&self.in_flight, 1);
        let _group = InFlightWriter(&self.in_flight);
        let mut after_first = Some(after_first);
        for (index, (key, value)) in entries.iter().enumerate() {
            let id = CounterId::of(key).expect("the complete group was resolved before mutation");
            self.record(id, *value);
            if index == 0 {
                after_first.take().expect("the hook runs once")();
            }
        }
        true
    }

    /// How many observations `id` has taken.
    #[must_use]
    pub fn observations_of(&self, id: CounterId) -> u64 {
        self.slots[id.index() * SLOTS_PER_COUNTER].load(Ordering::SeqCst)
    }

    /// The total `id` has accumulated, in its own unit.
    #[must_use]
    pub fn total_of(&self, id: CounterId) -> u64 {
        self.slots[id.index() * SLOTS_PER_COUNTER + 1].load(Ordering::SeqCst)
    }

    /// The deterministic part of what collection costs, with no clock involved.
    #[must_use]
    pub fn overhead(&self) -> Overhead {
        Overhead {
            // `self` includes the boxed-slice owner (pointer and length) plus the three inline
            // atomics. The allocation holds the per-counter slots. Counting only the allocation
            // and inline atomics omits the owner metadata and understates the live footprint.
            state_bytes: core::mem::size_of_val(self) as u64
                + (self.slots.len() * core::mem::size_of::<AtomicU64>()) as u64,
            atomic_writes_per_observation: ATOMIC_WRITES_PER_OBSERVATION,
            atomic_writes_per_group: ATOMIC_WRITES_PER_GROUP,
            allocations_per_observation: ALLOCATIONS_PER_OBSERVATION,
            counters: counter_count() as u64,
        }
    }

    /// Every counter's current reading, plus what collection itself did.
    ///
    /// The independent tally is read **before and after** the per-counter pass, and
    /// [`Collection::concurrent_observations`] is the saturating difference between those two reads.
    /// Both come from one atomic in the registry's global order, so the difference cannot be
    /// negative; at the cardinality ceiling it is only a lower bound and the consistency verdict
    /// below fails closed. Allocates once, for the readings.
    #[must_use]
    pub fn snapshot(&self) -> CounterSnapshot {
        let before = self.observations.load(Ordering::SeqCst);
        let writers_before = self.in_flight.load(Ordering::SeqCst);
        let readings: Vec<Reading> = (0..counter_count())
            .filter_map(CounterId::at)
            .map(|id| Reading::of(id, self.observations_of(id), self.total_of(id)))
            .collect();
        let summed = readings.iter().fold(0_u64, |total, reading| {
            total.saturating_add(reading.observations)
        });
        let writers_after = self.in_flight.load(Ordering::SeqCst);
        let after = self.observations.load(Ordering::SeqCst);
        let snapshots = add_saturating(&self.snapshots, 1);
        let measurement_saturated = after == u64::MAX
            || summed == u64::MAX
            || readings
                .iter()
                .any(|reading| reading.observations == u64::MAX || reading.total == u64::MAX);

        CounterSnapshot {
            readings,
            collection: Collection {
                observations_recorded: after,
                observations_summed: summed,
                // Before saturation, `after >= before` by modification-order coherence on one
                // location. At the ceiling the difference is only a lower bound, which is why the
                // consistency verdict below fails closed.
                concurrent_observations: after.saturating_sub(before),
                writers_in_flight_before_readings: writers_before,
                writers_in_flight_after_readings: writers_after,
                snapshot_consistent: before == after
                    && writers_before == 0
                    && writers_after == 0
                    && after == summed
                    && !measurement_saturated,
                snapshots_taken: snapshots,
                overhead: self.overhead(),
            },
        }
    }
}

/// Balance the in-flight writer signal even if a future recording hook panics.
struct InFlightWriter<'a>(&'a AtomicU64);

impl Drop for InFlightWriter<'_> {
    fn drop(&mut self) {
        decrement_in_flight(self.0);
    }
}

/// The deterministic cost of collection: no clock, no machine, no load.
///
/// Every field here is the same number on every host, which is the point. It is the half of
/// `## Acceptance criteria` bullet 2 that a merge gate can hold, and the wall-clock half lives in
/// `cargo bench -p mesh-bench --bench counter-overhead` where a number that moves with the machine
/// belongs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Overhead {
    /// Bytes of counter state the registry owns, including its value and heap allocation.
    pub state_bytes: u64,
    /// Atomic updates one observation performs.
    pub atomic_writes_per_observation: u64,
    /// Atomic updates added once around a related multi-counter product event.
    pub atomic_writes_per_group: u64,
    /// Heap allocations one observation performs.
    pub allocations_per_observation: u64,
    /// How many counters that state covers.
    pub counters: u64,
}

/// Add without wrapping. See [`Counters::record`] for why saturation beats wrapping here.
fn add_saturating(slot: &AtomicU64, value: u64) -> u64 {
    let mut current = slot.load(Ordering::SeqCst);
    loop {
        let next = current.saturating_add(value);
        match slot.compare_exchange_weak(current, next, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return next,
            Err(seen) => current = seen,
        }
    }
}

/// Remove one active writer without ever turning a saturated count back into an exact claim.
///
/// Once the count reaches [`u64::MAX`], the registry cannot know how many writers were hidden by
/// saturation. Pinning it at the ceiling keeps every later snapshot fail-closed. Below the ceiling
/// this is the ordinary fifth atomic update described by [`ATOMIC_WRITES_PER_OBSERVATION`].
fn decrement_in_flight(slot: &AtomicU64) {
    let mut current = slot.load(Ordering::SeqCst);
    loop {
        if current == u64::MAX {
            return;
        }
        debug_assert!(
            current > 0,
            "an in-flight writer guard must balance an increment"
        );
        let next = current.saturating_sub(1);
        match slot.compare_exchange_weak(current, next, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return,
            Err(seen) => current = seen,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::counters::catalogue::counters;

    fn first_key() -> &'static str {
        counters().next().expect("the catalogue is not empty").key
    }

    #[test]
    fn a_fresh_registry_reads_zero_everywhere() {
        let registry = Counters::new();
        let snapshot = registry.snapshot();
        assert_eq!(snapshot.readings.len(), counter_count());
        assert!(snapshot
            .readings
            .iter()
            .all(|reading| reading.observations == 0 && reading.total == 0));
        assert_eq!(snapshot.collection.observations_recorded, 0);
        assert_eq!(snapshot.collection.concurrent_observations, 0);
        assert_eq!(snapshot.collection.snapshots_taken, 1);
    }

    #[test]
    fn observations_and_totals_are_kept_apart() {
        let registry = Counters::new();
        let id = CounterId::of(first_key()).expect("resolves");
        registry.record(id, 7);
        registry.record(id, 0);
        registry.record(id, 5);
        assert_eq!(
            registry.observations_of(id),
            3,
            "a zero-value observation counts"
        );
        assert_eq!(registry.total_of(id), 12);
    }

    #[test]
    fn the_independent_tally_agrees_with_the_sum_on_one_thread() {
        let registry = Counters::new();
        let mut expected = 0;
        for (index, _) in counters().enumerate() {
            let id = CounterId::at(index).expect("in range");
            for _ in 0..=(index % 3) {
                registry.record(id, index as u64);
                expected += 1;
            }
        }
        let snapshot = registry.snapshot();
        assert_eq!(snapshot.collection.observations_recorded, expected);
        assert_eq!(snapshot.collection.observations_summed, expected);
        assert_eq!(snapshot.collection.concurrent_observations, 0);
    }

    #[test]
    fn a_quiet_tally_mismatch_is_not_published_as_consistent() {
        let registry = Counters::new();
        registry.observations.store(1, Ordering::SeqCst);

        let snapshot = registry.snapshot();
        assert_eq!(snapshot.collection.observations_recorded, 1);
        assert_eq!(snapshot.collection.observations_summed, 0);
        assert!(
            !snapshot.collection.snapshot_consistent,
            "the independent tally found missing counter state but the verdict called it consistent"
        );
    }

    #[test]
    fn recording_by_key_refuses_a_name_the_catalogue_does_not_have() {
        let registry = Counters::new();
        assert!(registry.record_by_key(first_key(), 3));
        assert!(!registry.record_by_key("not.a.counter", 3));
        assert_eq!(registry.snapshot().collection.observations_recorded, 1);
    }

    #[test]
    fn a_total_saturates_rather_than_wrapping_past_the_truth() {
        let registry = Counters::new();
        let id = CounterId::of(first_key()).expect("resolves");
        registry.record(id, u64::MAX);
        registry.record(id, 1);
        let snapshot = registry.snapshot();
        assert_eq!(
            registry.total_of(id),
            u64::MAX,
            "a wrapped total reads lower than the truth"
        );
        assert_eq!(registry.observations_of(id), 2);
        assert!(
            !snapshot.collection.snapshot_consistent,
            "a saturated total lost information but the exact counter plane called it consistent"
        );
    }

    #[test]
    fn every_published_cardinality_saturates_instead_of_wrapping() {
        let registry = Counters::new();
        let id = CounterId::of(first_key()).expect("resolves");
        let base = id.index() * SLOTS_PER_COUNTER;
        registry.observations.store(u64::MAX, Ordering::SeqCst);
        registry.slots[base].store(u64::MAX, Ordering::SeqCst);
        registry.snapshots.store(u64::MAX, Ordering::SeqCst);

        registry.record(id, 1);
        let snapshot = registry.snapshot();

        assert_eq!(registry.observations_of(id), u64::MAX);
        assert_eq!(snapshot.collection.observations_recorded, u64::MAX);
        assert_eq!(snapshot.collection.snapshots_taken, u64::MAX);
        assert!(
            !snapshot.collection.snapshot_consistent,
            "a saturated snapshot cannot claim that no observation was hidden"
        );
    }

    #[test]
    fn the_published_observation_sum_saturates_instead_of_panicking_or_wrapping() {
        let registry = Counters::new();
        let first = CounterId::of(first_key()).expect("resolves");
        let second = CounterId::at(1).expect("the catalogue has several counters");
        registry.slots[first.index() * SLOTS_PER_COUNTER].store(u64::MAX, Ordering::SeqCst);
        registry.slots[second.index() * SLOTS_PER_COUNTER].store(1, Ordering::SeqCst);

        let snapshot = registry.snapshot();

        assert_eq!(snapshot.collection.observations_summed, u64::MAX);
        assert!(!snapshot.collection.snapshot_consistent);
    }

    #[test]
    fn taking_a_snapshot_changes_no_counter() {
        let registry = Counters::new();
        let id = CounterId::of(first_key()).expect("resolves");
        registry.record(id, 4);
        let before = registry.snapshot();
        let after = registry.snapshot();
        assert_eq!(
            before.readings, after.readings,
            "snapshotting perturbed a counter"
        );
        assert_eq!(after.collection.observations_recorded, 1);
        assert_eq!(
            after.collection.snapshots_taken, 2,
            "collection's own count is reported"
        );
    }

    #[test]
    fn the_deterministic_overhead_is_a_number_and_not_an_adjective() {
        let registry = Counters::new();
        let overhead = registry.overhead();
        assert_eq!(overhead.counters, counter_count() as u64);
        assert_eq!(overhead.atomic_writes_per_observation, 5);
        assert_eq!(overhead.atomic_writes_per_group, 2);
        assert_eq!(overhead.allocations_per_observation, 0);
        assert_eq!(
            overhead.state_bytes,
            core::mem::size_of_val(&registry) as u64
                + (counter_count() * SLOTS_PER_COUNTER * core::mem::size_of::<AtomicU64>()) as u64,
            "published state bytes must include both the owned allocation and registry metadata"
        );
    }

    #[test]
    fn a_writer_between_count_and_total_makes_the_snapshot_explicitly_inconsistent() {
        use std::sync::{Arc, Barrier};

        let registry = Arc::new(Counters::new());
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let id = CounterId::of(first_key()).expect("resolves");
        let writer = {
            let registry = Arc::clone(&registry);
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            std::thread::spawn(move || {
                registry.record_with(id, 9, || {
                    entered.wait();
                    release.wait();
                });
            })
        };

        entered.wait();
        let overlapping = registry.snapshot();
        assert!(!overlapping.collection.snapshot_consistent);
        assert_eq!(overlapping.collection.writers_in_flight_before_readings, 1);
        assert_eq!(overlapping.collection.writers_in_flight_after_readings, 1);
        release.wait();
        writer.join().expect("writer completes");

        let settled = registry.snapshot();
        assert!(settled.collection.snapshot_consistent);
        assert_eq!(settled.collection.writers_in_flight_before_readings, 0);
        assert_eq!(settled.collection.writers_in_flight_after_readings, 0);
        assert_eq!(registry.observations_of(id), 1);
        assert_eq!(registry.total_of(id), 9);
    }

    #[test]
    fn a_related_group_cannot_publish_a_clean_half_event() {
        use std::sync::{Arc, Barrier};

        let registry = Arc::new(Counters::new());
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let first = first_key();
        let second = counters().nth(1).expect("several counters").key;
        let writer = {
            let registry = Arc::clone(&registry);
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            std::thread::spawn(move || {
                assert!(registry.record_group_with(&[(first, 1), (second, 9)], || {
                    entered.wait();
                    release.wait();
                }));
            })
        };

        entered.wait();
        let half = registry.snapshot();
        assert_eq!(half.reading(first).expect("first").total, 1);
        assert_eq!(half.reading(second).expect("second").total, 0);
        assert!(
            !half.collection.snapshot_consistent,
            "a snapshot between related counter updates called the half-event consistent"
        );
        assert!(half.collection.writers_in_flight_before_readings > 0);
        assert!(half.collection.writers_in_flight_after_readings > 0);

        release.wait();
        writer.join().expect("group completes");
        let complete = registry.snapshot();
        assert!(complete.collection.snapshot_consistent);
        assert_eq!(complete.reading(first).expect("first").total, 1);
        assert_eq!(complete.reading(second).expect("second").total, 9);
    }

    #[test]
    fn an_unknown_group_member_refuses_the_whole_group() {
        let registry = Counters::new();
        assert!(!registry.record_group_by_key(&[]));
        assert!(!registry.record_group_by_key(&[(first_key(), 1), ("not.a.counter", 2)]));
        assert_eq!(registry.snapshot().collection.observations_recorded, 0);
    }

    #[test]
    fn a_saturated_writer_count_stays_fail_closed_during_an_observation() {
        use std::sync::{Arc, Barrier};

        let registry = Arc::new(Counters::new());
        registry.in_flight.store(u64::MAX, Ordering::SeqCst);
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let id = CounterId::of(first_key()).expect("resolves");
        let writer = {
            let registry = Arc::clone(&registry);
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            std::thread::spawn(move || {
                registry.record_with(id, 9, || {
                    entered.wait();
                    release.wait();
                });
            })
        };

        entered.wait();
        let overlapping = registry.snapshot();
        assert_eq!(
            overlapping.collection.writers_in_flight_before_readings,
            u64::MAX,
            "a published writer cardinality must saturate rather than wrap to zero"
        );
        assert_eq!(
            overlapping.collection.writers_in_flight_after_readings,
            u64::MAX
        );
        assert!(
            !overlapping.collection.snapshot_consistent,
            "a saturated writer count can no longer prove a clean snapshot"
        );

        release.wait();
        writer.join().expect("writer completes");
        let settled = registry.snapshot();
        assert_eq!(
            settled.collection.writers_in_flight_before_readings,
            u64::MAX
        );
        assert_eq!(
            settled.collection.writers_in_flight_after_readings,
            u64::MAX
        );
        assert!(!settled.collection.snapshot_consistent);
    }

    #[test]
    fn concurrent_recording_lands_every_observation() {
        let registry = std::sync::Arc::new(Counters::new());
        let id = CounterId::of(first_key()).expect("resolves");
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let registry = std::sync::Arc::clone(&registry);
                std::thread::spawn(move || {
                    for _ in 0..250 {
                        registry.record(id, 2);
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().expect("no recorder panics");
        }
        assert_eq!(registry.observations_of(id), 1_000);
        assert_eq!(registry.total_of(id), 2_000);
        assert_eq!(registry.snapshot().collection.observations_recorded, 1_000);
    }
}
