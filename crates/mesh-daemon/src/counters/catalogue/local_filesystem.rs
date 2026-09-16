//! Plan §12.3 "Local filesystem": thirteen required metrics and the counters behind them.
//!
//! Every latency bullet in this group is backed by an exact operation count as well as a
//! nanosecond total, so a reader who does not trust the machine's load can still read the count.
//! The one bullet with no exact form at all is `build/test slowdown`, which is a ratio of two
//! wall-clock numbers; it is backed by both of them separately rather than by a precomputed ratio,
//! so a reader can see what was divided by what.

use super::{CounterSpec, Family, Producer, RequiredMetric, Unit};

/// One row, with this file's family baked in.
const fn row(key: &'static str, unit: Unit, what: &'static str, producer: Producer) -> CounterSpec {
    CounterSpec {
        key,
        family: Family::LocalFilesystem,
        unit,
        what,
        producer,
    }
}

/// The product performs managed filesystem work, but the operation-level measurement suite is a
/// separate task and those paths are not yet wired into this registry.
const NO_FILESYSTEM_PATH: Producer = Producer::NotYet(
    "the filesystem operation exists, but no operation-level metric producer is wired in this build",
);

/// The counters, deterministic ones first inside each subject.
pub const COUNTERS: &[CounterSpec] = &[
    row(
        "local_filesystem.lookup.ops",
        Unit::Events,
        "one path resolution",
        NO_FILESYSTEM_PATH,
    ),
    row(
        "local_filesystem.lookup.ns",
        Unit::Nanoseconds,
        "time inside path resolution",
        NO_FILESYSTEM_PATH,
    ),
    row(
        "local_filesystem.open.ops",
        Unit::Events,
        "one file opened for a caller",
        NO_FILESYSTEM_PATH,
    ),
    row(
        "local_filesystem.open.ns",
        Unit::Nanoseconds,
        "time inside opening a file",
        NO_FILESYSTEM_PATH,
    ),
    row(
        "local_filesystem.create.ops",
        Unit::Events,
        "one file created for a caller",
        NO_FILESYSTEM_PATH,
    ),
    row(
        "local_filesystem.create.ns",
        Unit::Nanoseconds,
        "time inside creating a file",
        NO_FILESYSTEM_PATH,
    ),
    row(
        "local_filesystem.rename.ops",
        Unit::Events,
        "one rename or move",
        NO_FILESYSTEM_PATH,
    ),
    row(
        "local_filesystem.rename.ns",
        Unit::Nanoseconds,
        "time inside a rename or move",
        NO_FILESYSTEM_PATH,
    ),
    row(
        "local_filesystem.directory_enumeration.ops",
        Unit::Events,
        "one directory listing",
        NO_FILESYSTEM_PATH,
    ),
    row(
        "local_filesystem.directory_enumeration.entries",
        Unit::Events,
        "entries returned by a directory listing",
        NO_FILESYSTEM_PATH,
    ),
    row(
        "local_filesystem.directory_enumeration.ns",
        Unit::Nanoseconds,
        "time inside a directory listing",
        NO_FILESYSTEM_PATH,
    ),
    row(
        "local_filesystem.random_read.ops",
        Unit::Events,
        "one read at an arbitrary offset",
        NO_FILESYSTEM_PATH,
    ),
    row(
        "local_filesystem.random_read.bytes",
        Unit::Bytes,
        "bytes returned by reads at an arbitrary offset",
        NO_FILESYSTEM_PATH,
    ),
    row(
        "local_filesystem.random_read.ns",
        Unit::Nanoseconds,
        "time inside reads at an arbitrary offset",
        NO_FILESYSTEM_PATH,
    ),
    row(
        "local_filesystem.sequential_read.ops",
        Unit::Events,
        "one read of a whole file from its start",
        Producer::Wired("crate::LiveDaemon, once per workspace opened"),
    ),
    row(
        "local_filesystem.sequential_read.bytes",
        Unit::Bytes,
        "bytes read from the start of a file",
        Producer::Wired("crate::LiveDaemon, the size of the record file it read"),
    ),
    row(
        "local_filesystem.sequential_read.ns",
        Unit::Nanoseconds,
        "time inside reading a whole file",
        // Deliberately unfed even though the two counters above it are fed. The daemon times one
        // span that covers the read AND the fold that follows it; charging that span to both would
        // report the same nanoseconds twice under two names, which is the double count a report
        // built on these numbers would then multiply. The whole span is recorded once, on
        // `workspace_operations.index_reconstruction.ns`.
        Producer::NotYet(
            "the read is not timed apart from the fold that follows it; the whole open is recorded \
             on workspace_operations.index_reconstruction.ns",
        ),
    ),
    row(
        "local_filesystem.random_write.ops",
        Unit::Events,
        "one write at an arbitrary offset",
        NO_FILESYSTEM_PATH,
    ),
    row(
        "local_filesystem.random_write.bytes",
        Unit::Bytes,
        "bytes written at an arbitrary offset",
        NO_FILESYSTEM_PATH,
    ),
    row(
        "local_filesystem.random_write.ns",
        Unit::Nanoseconds,
        "time inside writes at an arbitrary offset",
        NO_FILESYSTEM_PATH,
    ),
    row(
        "local_filesystem.sequential_write.ops",
        Unit::Events,
        "one append",
        NO_FILESYSTEM_PATH,
    ),
    row(
        "local_filesystem.sequential_write.bytes",
        Unit::Bytes,
        "bytes appended",
        NO_FILESYSTEM_PATH,
    ),
    row(
        "local_filesystem.sequential_write.ns",
        Unit::Nanoseconds,
        "time inside an append, including the force to disk",
        NO_FILESYSTEM_PATH,
    ),
    row(
        "local_filesystem.build_test.runs",
        Unit::Events,
        "one build or test run measured against a baseline",
        Producer::NotYet("no build or test runs inside a Mesh workspace are measured here"),
    ),
    row(
        "local_filesystem.build_test.ns",
        Unit::Nanoseconds,
        "time a build or test run took inside a Mesh workspace",
        Producer::NotYet("no build or test runs inside a Mesh workspace are measured here"),
    ),
    row(
        "local_filesystem.build_test.baseline_ns",
        Unit::Nanoseconds,
        "time the same build or test run took on the native filesystem",
        Producer::NotYet("no build or test runs inside a Mesh workspace are measured here"),
    ),
    row(
        "local_filesystem.cpu.samples",
        Unit::Events,
        "one processor-usage sample",
        Producer::NotYet("no process accounting is read on any platform in this build"),
    ),
    row(
        "local_filesystem.cpu.busy_ns",
        Unit::Nanoseconds,
        "processor time this process was on a core, summed over samples",
        Producer::NotYet("no process accounting is read on any platform in this build"),
    ),
    row(
        "local_filesystem.memory.samples",
        Unit::Events,
        "one memory sample",
        Producer::NotYet("no process accounting is read on any platform in this build"),
    ),
    row(
        "local_filesystem.memory.resident_bytes",
        Unit::Bytes,
        "resident memory summed over samples; divide by the sample count for the mean",
        Producer::NotYet("no process accounting is read on any platform in this build"),
    ),
    row(
        "local_filesystem.context_switches.samples",
        Unit::Events,
        "one context-switch sample",
        Producer::NotYet("no process accounting is read on any platform in this build"),
    ),
    row(
        "local_filesystem.context_switches.count",
        Unit::Events,
        "context switches counted across samples",
        Producer::NotYet("no process accounting is read on any platform in this build"),
    ),
];

/// Plan §12.3's "Local filesystem" bullets, in the plan's order.
pub const REQUIRED: &[RequiredMetric] = &[
    RequiredMetric {
        metric: "lookup",
        family: Family::LocalFilesystem,
        counters: &["local_filesystem.lookup.ops", "local_filesystem.lookup.ns"],
    },
    RequiredMetric {
        metric: "open",
        family: Family::LocalFilesystem,
        counters: &["local_filesystem.open.ops", "local_filesystem.open.ns"],
    },
    RequiredMetric {
        metric: "create",
        family: Family::LocalFilesystem,
        counters: &["local_filesystem.create.ops", "local_filesystem.create.ns"],
    },
    RequiredMetric {
        metric: "rename",
        family: Family::LocalFilesystem,
        counters: &["local_filesystem.rename.ops", "local_filesystem.rename.ns"],
    },
    RequiredMetric {
        metric: "directory enumeration",
        family: Family::LocalFilesystem,
        counters: &[
            "local_filesystem.directory_enumeration.ops",
            "local_filesystem.directory_enumeration.entries",
            "local_filesystem.directory_enumeration.ns",
        ],
    },
    RequiredMetric {
        metric: "random read",
        family: Family::LocalFilesystem,
        counters: &[
            "local_filesystem.random_read.ops",
            "local_filesystem.random_read.bytes",
            "local_filesystem.random_read.ns",
        ],
    },
    RequiredMetric {
        metric: "sequential read",
        family: Family::LocalFilesystem,
        counters: &[
            "local_filesystem.sequential_read.ops",
            "local_filesystem.sequential_read.bytes",
            "local_filesystem.sequential_read.ns",
        ],
    },
    RequiredMetric {
        metric: "random write",
        family: Family::LocalFilesystem,
        counters: &[
            "local_filesystem.random_write.ops",
            "local_filesystem.random_write.bytes",
            "local_filesystem.random_write.ns",
        ],
    },
    RequiredMetric {
        metric: "sequential write",
        family: Family::LocalFilesystem,
        counters: &[
            "local_filesystem.sequential_write.ops",
            "local_filesystem.sequential_write.bytes",
            "local_filesystem.sequential_write.ns",
        ],
    },
    RequiredMetric {
        metric: "build/test slowdown",
        family: Family::LocalFilesystem,
        counters: &[
            "local_filesystem.build_test.runs",
            "local_filesystem.build_test.ns",
            "local_filesystem.build_test.baseline_ns",
        ],
    },
    RequiredMetric {
        metric: "CPU",
        family: Family::LocalFilesystem,
        counters: &[
            "local_filesystem.cpu.samples",
            "local_filesystem.cpu.busy_ns",
        ],
    },
    RequiredMetric {
        metric: "memory",
        family: Family::LocalFilesystem,
        counters: &[
            "local_filesystem.memory.samples",
            "local_filesystem.memory.resident_bytes",
        ],
    },
    RequiredMetric {
        metric: "context switches",
        family: Family::LocalFilesystem,
        counters: &[
            "local_filesystem.context_switches.samples",
            "local_filesystem.context_switches.count",
        ],
    },
];
