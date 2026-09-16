//! Plan §12.3 "Workspace operations": eight required metrics and the counters behind them.
//!
//! Three of the eight are fed by this build: private checkpoint creation, index reconstruction,
//! and crash recovery. The other five still have no producer, and each says so on its own row
//! rather than reading zero next to the operations that are real.

use super::{CounterSpec, Family, Producer, RequiredMetric, Unit};

/// One row, with this file's family baked in.
const fn row(key: &'static str, unit: Unit, what: &'static str, producer: Producer) -> CounterSpec {
    CounterSpec {
        key,
        family: Family::WorkspaceOperations,
        unit,
        what,
        producer,
    }
}

/// Existing product paths that are not yet metric producers. The later workspace-operation
/// collection work owns benchmark composition; calling these operations absent would be false.
const NO_METRIC_PRODUCER: Producer = Producer::NotYet(
    "the operation exists, but no workspace-operation metric producer is wired in this build",
);

/// The counters, deterministic ones first inside each subject.
pub const COUNTERS: &[CounterSpec] = &[
    row(
        "workspace_operations.actor_view_creation.ops",
        Unit::Events,
        "one actor's own view of the workspace created",
        NO_METRIC_PRODUCER,
    ),
    row(
        "workspace_operations.actor_view_creation.ns",
        Unit::Nanoseconds,
        "time inside creating an actor's own view",
        NO_METRIC_PRODUCER,
    ),
    row(
        "workspace_operations.checkpoint_creation.ops",
        Unit::Events,
        "one saved point in an actor's private work",
        Producer::Wired("crate::LiveDaemon, after a private save is durably observed"),
    ),
    row(
        "workspace_operations.checkpoint_creation.bytes",
        Unit::Bytes,
        "new content and payload bytes linked durably by saved points",
        Producer::Wired("crate::LiveDaemon, from the completed CAS promoter"),
    ),
    row(
        "workspace_operations.checkpoint_creation.ns",
        Unit::Nanoseconds,
        "time inside saving a point in an actor's private work",
        NO_METRIC_PRODUCER,
    ),
    row(
        "workspace_operations.review_bundle_calculation.ops",
        Unit::Events,
        "one review bundle calculated",
        NO_METRIC_PRODUCER,
    ),
    row(
        "workspace_operations.review_bundle_calculation.ns",
        Unit::Nanoseconds,
        "time inside calculating a review bundle",
        NO_METRIC_PRODUCER,
    ),
    row(
        "workspace_operations.shadow_view_opening.ops",
        Unit::Events,
        "one read-only view of somebody else's work opened",
        NO_METRIC_PRODUCER,
    ),
    row(
        "workspace_operations.shadow_view_opening.ns",
        Unit::Nanoseconds,
        "time inside opening a read-only view of somebody else's work",
        NO_METRIC_PRODUCER,
    ),
    row(
        "workspace_operations.selective_approval.ops",
        Unit::Events,
        "one approval of part of a review bundle",
        NO_METRIC_PRODUCER,
    ),
    row(
        "workspace_operations.selective_approval.ns",
        Unit::Nanoseconds,
        "time inside approving part of a review bundle",
        NO_METRIC_PRODUCER,
    ),
    row(
        "workspace_operations.canonical_publication.ops",
        Unit::Events,
        "one advance of the protected shared version",
        NO_METRIC_PRODUCER,
    ),
    row(
        "workspace_operations.canonical_publication.ns",
        Unit::Nanoseconds,
        "time inside advancing the protected shared version",
        NO_METRIC_PRODUCER,
    ),
    row(
        "workspace_operations.crash_recovery.ops",
        Unit::Events,
        "one open that had to recover from an interrupted save",
        Producer::Wired("crate::LiveDaemon, when the record file ends in a partial frame"),
    ),
    row(
        "workspace_operations.crash_recovery.records",
        Unit::Events,
        "whole records served past an interrupted save",
        Producer::Wired("crate::LiveDaemon, the durable record count of such an open"),
    ),
    row(
        "workspace_operations.crash_recovery.ns",
        Unit::Nanoseconds,
        "time inside an open that had to recover from an interrupted save",
        Producer::Wired("crate::LiveDaemon, the elapsed time of such an open"),
    ),
    row(
        "workspace_operations.index_reconstruction.ops",
        Unit::Events,
        "one index folded from the records on disk",
        Producer::Wired("crate::LiveDaemon, once per workspace opened"),
    ),
    row(
        "workspace_operations.index_reconstruction.rows",
        Unit::Events,
        "rows the fold produced",
        Producer::Wired("crate::LiveDaemon, the row count the fold reported"),
    ),
    row(
        "workspace_operations.index_reconstruction.ns",
        Unit::Nanoseconds,
        "time inside folding an index from records",
        Producer::Wired("crate::LiveDaemon, the elapsed time of the open that folded it"),
    ),
];

/// Plan §12.3's "Workspace operations" bullets, in the plan's order.
pub const REQUIRED: &[RequiredMetric] = &[
    RequiredMetric {
        metric: "actor view creation",
        family: Family::WorkspaceOperations,
        counters: &[
            "workspace_operations.actor_view_creation.ops",
            "workspace_operations.actor_view_creation.ns",
        ],
    },
    RequiredMetric {
        metric: "checkpoint creation",
        family: Family::WorkspaceOperations,
        counters: &[
            "workspace_operations.checkpoint_creation.ops",
            "workspace_operations.checkpoint_creation.bytes",
            "workspace_operations.checkpoint_creation.ns",
        ],
    },
    RequiredMetric {
        metric: "review-bundle calculation",
        family: Family::WorkspaceOperations,
        counters: &[
            "workspace_operations.review_bundle_calculation.ops",
            "workspace_operations.review_bundle_calculation.ns",
        ],
    },
    RequiredMetric {
        metric: "shadow-view opening",
        family: Family::WorkspaceOperations,
        counters: &[
            "workspace_operations.shadow_view_opening.ops",
            "workspace_operations.shadow_view_opening.ns",
        ],
    },
    RequiredMetric {
        metric: "selective approval",
        family: Family::WorkspaceOperations,
        counters: &[
            "workspace_operations.selective_approval.ops",
            "workspace_operations.selective_approval.ns",
        ],
    },
    RequiredMetric {
        metric: "canonical publication",
        family: Family::WorkspaceOperations,
        counters: &[
            "workspace_operations.canonical_publication.ops",
            "workspace_operations.canonical_publication.ns",
        ],
    },
    RequiredMetric {
        metric: "crash recovery",
        family: Family::WorkspaceOperations,
        counters: &[
            "workspace_operations.crash_recovery.ops",
            "workspace_operations.crash_recovery.records",
            "workspace_operations.crash_recovery.ns",
        ],
    },
    RequiredMetric {
        metric: "index reconstruction",
        family: Family::WorkspaceOperations,
        counters: &[
            "workspace_operations.index_reconstruction.ops",
            "workspace_operations.index_reconstruction.rows",
            "workspace_operations.index_reconstruction.ns",
        ],
    },
];
