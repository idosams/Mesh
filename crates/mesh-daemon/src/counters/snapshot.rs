//! One reading of every counter, and the shape it is published in.
//!
//! # A snapshot is queryable without running a benchmark
//!
//! [`CounterSnapshot`] is produced by [`crate::counters::Counters::snapshot`] from live process
//! state. Nothing here starts a workload, reads a corpus, or needs a benchmark harness: a caller
//! that holds a running daemon holds its counters, which is the half of `## Intent` about making
//! production behaviour visible without a benchmark run.
//!
//! # Every reading says how much to trust it
//!
//! A reading carries its unit, its determinism and its band, not only its number. A reader who
//! sees `local_filesystem.sequential_read.bytes` knows it is exact; a reader who sees
//! `…​.sequential_read.ns` knows it is not, and [`CONDITIONS`] names where the conditions for a
//! wall-clock number on this program are written down.
//!
//! # A zero that means nothing measured this is not a zero that means it happened no times
//!
//! [`CounterSnapshot::not_yet`] lists every counter with no producer in this build, with the
//! reason, in the same shape [`crate::ipc::surface::WorkspaceSummary`] uses for the same problem.
//! Rendering the two kinds of zero identically tells the reader the second when the truth was the
//! first, and a benchmark report built on that mistake reports a system that did nothing as a
//! system that did nothing wrong.
//!
//! # Counter integers are decimal strings on the IPC wire
//!
//! The desktop consumer is JavaScript, whose `Number` type cannot distinguish every `u64` value.
//! Every measured integer is therefore encoded as its canonical base-10 string. Internal APIs keep
//! typed `u64` values; only the v4 wire representation changes shape to preserve exactness.

use crate::ipc::json::Json;

use super::catalogue::{Band, CounterId, CounterSpec, Determinism, Family, Unit};
use super::registry::Overhead;

/// Encode a counter integer without sending it through JavaScript's lossy `Number` type.
///
/// The v4 IPC consumer is JavaScript, whose largest exactly representable integer is 2^53 - 1.
/// Counters are `u64` internally and can legitimately exceed that bound, especially byte and
/// nanosecond totals. A canonical base-10 string keeps the complete value on every client.
fn decimal_u64(value: u64) -> Json {
    Json::text(value.to_string())
}

/// Where the conditions for a wall-clock number on this program are written down.
///
/// Named rather than restated, so a load-dependent reading and a published benchmark row point at
/// one document instead of two prose copies that will drift.
pub const CONDITIONS: &str = "a load-dependent reading is wall-clock nanoseconds and moves with \
                              machine load; the conditions a timing number is taken under are \
                              benchmarks/runners/README.md, `Getting a trustworthy number`";

/// Machine-readable declaration for every integer field in the v4 snapshot object.
pub const INTEGER_ENCODING: &str = "decimal-u64";

/// One counter's current value, with everything needed to judge it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reading {
    /// The catalogue row this reads.
    pub spec: &'static CounterSpec,
    /// How many observations have been recorded.
    pub observations: u64,
    /// The sum of the observed values, in [`Reading::unit`].
    pub total: u64,
}

impl Reading {
    /// A reading of `id` at these values.
    #[must_use]
    pub fn of(id: CounterId, observations: u64, total: u64) -> Self {
        Self {
            spec: id.spec(),
            observations,
            total,
        }
    }

    /// The counter's stable key.
    #[must_use]
    pub const fn key(&self) -> &'static str {
        self.spec.key
    }

    /// Which of plan §12.3's groups it belongs to.
    #[must_use]
    pub const fn family(&self) -> Family {
        self.spec.family
    }

    /// What one observation adds.
    #[must_use]
    pub const fn unit(&self) -> Unit {
        self.spec.unit
    }

    /// Whether the same work gives this total twice.
    #[must_use]
    pub const fn determinism(&self) -> Determinism {
        self.spec.determinism()
    }

    /// The band an independent measurement of the same work is held to.
    #[must_use]
    pub const fn band(&self) -> Band {
        self.spec.band()
    }

    /// Whether anything in this build feeds this counter.
    #[must_use]
    pub const fn produced(&self) -> bool {
        self.spec.producer.is_wired()
    }

    /// This reading as one object.
    ///
    /// Key order: `key`, `family`, `unit`, `determinism`, `band`, `observations`, `total`,
    /// `produced`. Fixed, because the encoder in [`crate::ipc::json`] preserves insertion order and
    /// a fixed order is what makes an object comparable byte for byte.
    #[must_use]
    pub fn to_json(&self) -> Json {
        Json::object([
            ("key", Json::text(self.key())),
            ("family", Json::text(self.family().word())),
            ("unit", Json::text(self.unit().word())),
            ("determinism", Json::text(self.determinism().word())),
            ("band", Json::text(self.band().word())),
            ("observations", decimal_u64(self.observations)),
            ("total", decimal_u64(self.total)),
            ("produced", Json::Bool(self.produced())),
        ])
    }
}

/// What collection itself did, reported next to what it collected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Collection {
    /// Observations recorded across all counters, tallied independently of the per-counter ones.
    pub observations_recorded: u64,
    /// The sum of the per-counter observation counts this snapshot read.
    ///
    /// With nothing else recording this equals [`Self::observations_recorded`] exactly, and that
    /// equality is the registry's cheapest self-check — it catches an indexing mistake that no
    /// per-counter assertion can see. While something *is* recording, the two are read at different
    /// instants from different atomics and may differ **in either direction**;
    /// [`Self::concurrent_observations`] says how wide the window was.
    pub observations_summed: u64,
    /// Observations that landed while this snapshot was being taken.
    ///
    /// Two reads of one atomic, so it can never be negative. Before the tally saturates it is the
    /// exact size of the window the other two numbers were read across. At `u64::MAX` it becomes a
    /// lower bound and [`Self::snapshot_consistent`] fails closed instead of claiming exactness.
    /// Non-zero is not an error: it reports overlap rather than smoothing it away.
    pub concurrent_observations: u64,
    /// Writers or related product groups active immediately before the per-counter reading pass.
    pub writers_in_flight_before_readings: u64,
    /// Writers or related product groups active immediately after the per-counter reading pass.
    pub writers_in_flight_after_readings: u64,
    /// Whether the reading pass overlapped no observation, no published count or total saturated,
    /// the independent tally equals the summed per-counter counts, and the readings are therefore
    /// internally consistent.
    pub snapshot_consistent: bool,
    /// How many snapshots have been taken, including this one.
    pub snapshots_taken: u64,
    /// The deterministic cost of collection.
    pub overhead: Overhead,
}

impl Collection {
    /// This block as one object.
    ///
    /// Key order: `observations_recorded`, `observations_summed`, `concurrent_observations`,
    /// `writers_in_flight_before_readings`, `writers_in_flight_after_readings`,
    /// `snapshot_consistent`, `snapshots_taken`, `state_bytes`, `counters`,
    /// `atomic_writes_per_observation`, `atomic_writes_per_group`,
    /// `allocations_per_observation`.
    #[must_use]
    pub fn to_json(&self) -> Json {
        Json::object([
            (
                "observations_recorded",
                decimal_u64(self.observations_recorded),
            ),
            ("observations_summed", decimal_u64(self.observations_summed)),
            (
                "concurrent_observations",
                decimal_u64(self.concurrent_observations),
            ),
            (
                "writers_in_flight_before_readings",
                decimal_u64(self.writers_in_flight_before_readings),
            ),
            (
                "writers_in_flight_after_readings",
                decimal_u64(self.writers_in_flight_after_readings),
            ),
            ("snapshot_consistent", Json::Bool(self.snapshot_consistent)),
            ("snapshots_taken", decimal_u64(self.snapshots_taken)),
            ("state_bytes", decimal_u64(self.overhead.state_bytes)),
            ("counters", decimal_u64(self.overhead.counters)),
            (
                "atomic_writes_per_observation",
                decimal_u64(self.overhead.atomic_writes_per_observation),
            ),
            (
                "atomic_writes_per_group",
                decimal_u64(self.overhead.atomic_writes_per_group),
            ),
            (
                "allocations_per_observation",
                decimal_u64(self.overhead.allocations_per_observation),
            ),
        ])
    }
}

/// Every counter's reading at one moment, plus what collection cost to take it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CounterSnapshot {
    /// One entry per catalogue counter, in catalogue order.
    pub readings: Vec<Reading>,
    /// What collection itself did.
    pub collection: Collection,
}

impl CounterSnapshot {
    /// The reading for `key`, when the catalogue has one.
    #[must_use]
    pub fn reading(&self, key: &str) -> Option<&Reading> {
        self.readings.iter().find(|reading| reading.key() == key)
    }

    /// Every reading in one of plan §12.3's groups.
    pub fn family(&self, family: Family) -> impl Iterator<Item = &Reading> {
        self.readings
            .iter()
            .filter(move |reading| reading.family() == family)
    }

    /// Every counter nothing in this build feeds, and why, as `(key, reason)`.
    #[must_use]
    pub fn not_yet(&self) -> Vec<(&'static str, &'static str)> {
        self.readings
            .iter()
            .filter(|reading| !reading.produced())
            .map(|reading| (reading.key(), reading.spec.producer.reason()))
            .collect()
    }

    /// How many counters something in this build feeds.
    #[must_use]
    pub fn produced_count(&self) -> usize {
        self.readings
            .iter()
            .filter(|reading| reading.produced())
            .count()
    }

    /// This snapshot as one object.
    ///
    /// Key order: `conditions`, `integer_encoding`, `collection`, `counters`, `not_yet`; and within
    /// a `not_yet` entry, `key`, `reason`. `conditions` comes first because it is the sentence a
    /// reader needs before the measurements, not after them.
    #[must_use]
    pub fn to_json(&self) -> Json {
        Json::object([
            ("conditions", Json::text(CONDITIONS)),
            ("integer_encoding", Json::text(INTEGER_ENCODING)),
            ("collection", self.collection.to_json()),
            (
                "counters",
                Json::Array(self.readings.iter().map(Reading::to_json).collect()),
            ),
            (
                "not_yet",
                Json::Array(
                    self.not_yet()
                        .into_iter()
                        .map(|(key, reason)| {
                            Json::object([("key", Json::text(key)), ("reason", Json::text(reason))])
                        })
                        .collect(),
                ),
            ),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::counters::catalogue::{counter_count, counters};
    use crate::counters::Counters;

    #[test]
    fn a_reading_publishes_its_own_trustworthiness() {
        let registry = Counters::new();
        let snapshot = registry.snapshot();
        for reading in &snapshot.readings {
            let object = reading.to_json();
            assert_eq!(
                object.get("determinism").and_then(Json::as_text),
                Some(reading.determinism().word())
            );
            assert_eq!(
                object.get("band").and_then(Json::as_text),
                Some(reading.band().word())
            );
            assert_eq!(
                object.get("unit").and_then(Json::as_text),
                Some(reading.unit().word())
            );
        }
    }

    #[test]
    fn the_snapshot_object_carries_its_conditions_before_its_numbers() {
        let snapshot = Counters::new().snapshot();
        let object = snapshot.to_json();
        let Json::Object(pairs) = &object else {
            panic!("a snapshot is an object");
        };
        assert_eq!(pairs[0].0, "conditions");
        assert_eq!(
            object.get("conditions").and_then(Json::as_text),
            Some(CONDITIONS)
        );
        assert_eq!(pairs[1].0, "integer_encoding");
        assert_eq!(
            object.get("integer_encoding").and_then(Json::as_text),
            Some(INTEGER_ENCODING)
        );
        assert_eq!(
            object
                .get("counters")
                .and_then(Json::as_array)
                .map(<[Json]>::len),
            Some(counter_count())
        );
    }

    #[test]
    fn an_unfed_counter_is_named_with_its_reason_rather_than_left_at_zero() {
        let snapshot = Counters::new().snapshot();
        let not_yet = snapshot.not_yet();
        assert_eq!(not_yet.len() + snapshot.produced_count(), counter_count());
        assert!(
            !not_yet.is_empty(),
            "this build feeds only part of the catalogue"
        );
        for (key, reason) in &not_yet {
            assert!(!reason.is_empty(), "`{key}` is unfed and does not say why");
        }
        let wired = snapshot.produced_count();
        assert_eq!(
            wired,
            counters().filter(|spec| spec.producer.is_wired()).count()
        );
    }

    #[test]
    fn readings_can_be_found_by_key_and_grouped_by_family() {
        let snapshot = Counters::new().snapshot();
        for spec in counters() {
            assert!(
                snapshot.reading(spec.key).is_some(),
                "`{}` is missing",
                spec.key
            );
        }
        assert_eq!(snapshot.reading("not.a.counter"), None);
        let grouped: usize = Family::ALL
            .iter()
            .map(|family| snapshot.family(*family).count())
            .sum();
        assert_eq!(
            grouped,
            counter_count(),
            "a reading fell outside every family"
        );
    }

    #[test]
    fn wire_integers_remain_exact_past_javascript_number_precision() {
        const FIRST_UNSAFE_JAVASCRIPT_INTEGER: u64 = 9_007_199_254_740_993;

        assert_eq!(
            FIRST_UNSAFE_JAVASCRIPT_INTEGER as f64,
            (FIRST_UNSAFE_JAVASCRIPT_INTEGER - 1) as f64,
            "the planted value must demonstrate JavaScript Number precision loss"
        );

        let id = CounterId::at(0).expect("the catalogue is not empty");
        let encoded = Reading::of(id, FIRST_UNSAFE_JAVASCRIPT_INTEGER, u64::MAX).to_json();
        assert_eq!(
            encoded.get("observations").and_then(Json::as_text),
            Some("9007199254740993")
        );
        assert_eq!(
            encoded.get("total").and_then(Json::as_text),
            Some("18446744073709551615")
        );

        let round_trip = Json::parse(&encoded.encode()).expect("canonical IPC JSON parses");
        assert_eq!(
            round_trip
                .get("observations")
                .and_then(Json::as_text)
                .and_then(|value| value.parse::<u64>().ok()),
            Some(FIRST_UNSAFE_JAVASCRIPT_INTEGER)
        );
        assert_eq!(
            round_trip
                .get("total")
                .and_then(Json::as_text)
                .and_then(|value| value.parse::<u64>().ok()),
            Some(u64::MAX)
        );

        let collection = Collection {
            observations_recorded: u64::MAX,
            observations_summed: u64::MAX,
            concurrent_observations: u64::MAX,
            writers_in_flight_before_readings: u64::MAX,
            writers_in_flight_after_readings: u64::MAX,
            snapshot_consistent: false,
            snapshots_taken: u64::MAX,
            overhead: Overhead {
                state_bytes: u64::MAX,
                atomic_writes_per_observation: u64::MAX,
                atomic_writes_per_group: u64::MAX,
                allocations_per_observation: u64::MAX,
                counters: u64::MAX,
            },
        }
        .to_json();
        for field in [
            "observations_recorded",
            "observations_summed",
            "concurrent_observations",
            "writers_in_flight_before_readings",
            "writers_in_flight_after_readings",
            "snapshots_taken",
            "state_bytes",
            "counters",
            "atomic_writes_per_observation",
            "atomic_writes_per_group",
            "allocations_per_observation",
        ] {
            assert_eq!(
                collection.get(field).and_then(Json::as_text),
                Some("18446744073709551615"),
                "{field} must not cross the JavaScript Number boundary"
            );
        }
    }
}
