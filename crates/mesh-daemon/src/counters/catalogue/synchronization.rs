//! Plan §12.3 "Synchronization": eight required metrics and the counters behind them.
//!
//! Plan §12.3 writes "relay CPU/storage" as one bullet. It is two here, because a processor number
//! and a storage number are not the same measurement and a single row would have had to pick a
//! unit and lose the other. Every other bullet keeps the plan's wording.
//!
//! Nothing in this group is fed by this build. The daemon opens no network transport of any kind —
//! `crates/mesh-daemon/ipc-contract.json` publishes `network_listener: false` and two tests hold it
//! there — so every row below is honest about having no producer rather than reporting a zero that
//! would read like "nothing was transferred".

use super::{CounterSpec, Family, Producer, RequiredMetric, Unit};

/// One row, with this file's family baked in.
const fn row(key: &'static str, unit: Unit, what: &'static str, producer: Producer) -> CounterSpec {
    CounterSpec {
        key,
        family: Family::Synchronization,
        unit,
        what,
        producer,
    }
}

/// The daemon speaks one Unix-domain socket and no network transport at all.
const NO_TRANSPORT: Producer = Producer::NotYet(
    "this build opens no network transport, so nothing here is ever observed in it",
);

/// The counters, deterministic ones first inside each subject.
pub const COUNTERS: &[CounterSpec] = &[
    row(
        "synchronization.remote_metadata_visibility.ops",
        Unit::Events,
        "one remote change becoming visible locally",
        NO_TRANSPORT,
    ),
    row(
        "synchronization.remote_metadata_visibility.ns",
        Unit::Nanoseconds,
        "time between a remote change and its local visibility",
        NO_TRANSPORT,
    ),
    row(
        "synchronization.remote_small_file_availability.ops",
        Unit::Events,
        "one small remote file becoming readable locally",
        NO_TRANSPORT,
    ),
    row(
        "synchronization.remote_small_file_availability.ns",
        Unit::Nanoseconds,
        "time between a small remote file appearing and its contents being readable",
        NO_TRANSPORT,
    ),
    row(
        "synchronization.throughput.transfers",
        Unit::Events,
        "one bulk transfer",
        NO_TRANSPORT,
    ),
    row(
        "synchronization.throughput.bytes",
        Unit::Bytes,
        "bytes moved by bulk transfers",
        NO_TRANSPORT,
    ),
    row(
        "synchronization.throughput.ns",
        Unit::Nanoseconds,
        "time inside bulk transfers; divide the bytes by it for a rate",
        NO_TRANSPORT,
    ),
    row(
        "synchronization.transferred.messages",
        Unit::Events,
        "one message put on the wire",
        NO_TRANSPORT,
    ),
    row(
        "synchronization.transferred.bytes",
        Unit::Bytes,
        "bytes put on the wire, payload and framing together",
        NO_TRANSPORT,
    ),
    row(
        "synchronization.reconnect_convergence.ops",
        Unit::Events,
        "one reconnection that reached agreement again",
        NO_TRANSPORT,
    ),
    row(
        "synchronization.reconnect_convergence.ns",
        Unit::Nanoseconds,
        "time between reconnecting and reaching agreement again",
        NO_TRANSPORT,
    ),
    row(
        "synchronization.relay.cpu_samples",
        Unit::Events,
        "one processor-usage sample taken on a relay",
        NO_TRANSPORT,
    ),
    row(
        "synchronization.relay.cpu_busy_ns",
        Unit::Nanoseconds,
        "processor time a relay spent, summed over samples",
        NO_TRANSPORT,
    ),
    row(
        "synchronization.relay.storage_samples",
        Unit::Events,
        "one storage sample taken on a relay",
        NO_TRANSPORT,
    ),
    row(
        "synchronization.relay.storage_bytes",
        Unit::Bytes,
        "bytes a relay held, summed over samples; divide by the sample count for the mean",
        NO_TRANSPORT,
    ),
    row(
        "synchronization.duplicate_operations.count",
        Unit::Events,
        "one operation received that was already held",
        NO_TRANSPORT,
    ),
    row(
        "synchronization.duplicate_operations.bytes",
        Unit::Bytes,
        "bytes spent receiving operations that were already held",
        NO_TRANSPORT,
    ),
];

/// Plan §12.3's "Synchronization" bullets, in the plan's order.
pub const REQUIRED: &[RequiredMetric] = &[
    RequiredMetric {
        metric: "remote metadata visibility",
        family: Family::Synchronization,
        counters: &[
            "synchronization.remote_metadata_visibility.ops",
            "synchronization.remote_metadata_visibility.ns",
        ],
    },
    RequiredMetric {
        metric: "remote small-file availability",
        family: Family::Synchronization,
        counters: &[
            "synchronization.remote_small_file_availability.ops",
            "synchronization.remote_small_file_availability.ns",
        ],
    },
    RequiredMetric {
        metric: "throughput",
        family: Family::Synchronization,
        counters: &[
            "synchronization.throughput.transfers",
            "synchronization.throughput.bytes",
            "synchronization.throughput.ns",
        ],
    },
    RequiredMetric {
        metric: "transferred bytes",
        family: Family::Synchronization,
        counters: &[
            "synchronization.transferred.messages",
            "synchronization.transferred.bytes",
        ],
    },
    RequiredMetric {
        metric: "reconnect convergence",
        family: Family::Synchronization,
        counters: &[
            "synchronization.reconnect_convergence.ops",
            "synchronization.reconnect_convergence.ns",
        ],
    },
    RequiredMetric {
        metric: "relay CPU",
        family: Family::Synchronization,
        counters: &[
            "synchronization.relay.cpu_samples",
            "synchronization.relay.cpu_busy_ns",
        ],
    },
    RequiredMetric {
        metric: "relay storage",
        family: Family::Synchronization,
        counters: &[
            "synchronization.relay.storage_samples",
            "synchronization.relay.storage_bytes",
        ],
    },
    RequiredMetric {
        metric: "duplicate operation overhead",
        family: Family::Synchronization,
        counters: &[
            "synchronization.duplicate_operations.count",
            "synchronization.duplicate_operations.bytes",
        ],
    },
];
