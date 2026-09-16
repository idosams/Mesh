//! Plan §12.3 "Context": seven required metrics and the counters behind them.
//!
//! This is the group with the most to gain from counting rather than timing. Six of the seven
//! bullets are ratios or counts — hit rate, unique against repeated tokens, detections against
//! checks, accepted against rejected — and every one of them is recorded here as its **two raw
//! counts** rather than as a precomputed ratio. A ratio cannot be added across runs and cannot be
//! re-derived once the denominator is thrown away; two counts can be, and a reader can see what
//! was divided by what.
//!
//! Nothing in this group is fed by this build: `mesh-daemon` composes one crate and none of the
//! context machinery is reachable from it.

use super::{CounterSpec, Family, Producer, RequiredMetric, Unit};

/// One row, with this file's family baked in.
const fn row(key: &'static str, unit: Unit, what: &'static str, producer: Producer) -> CounterSpec {
    CounterSpec {
        key,
        family: Family::Context,
        unit,
        what,
        producer,
    }
}

/// No context assembly happens in this process.
const NO_CONTEXT_PATH: Producer = Producer::NotYet(
    "this build assembles no agent context, so nothing here is ever observed in it",
);

/// The counters, deterministic ones first inside each subject.
pub const COUNTERS: &[CounterSpec] = &[
    row(
        "context.harness.assemblies",
        Unit::Events,
        "one context assembled for an agent",
        NO_CONTEXT_PATH,
    ),
    row(
        "context.harness.fixed_bytes",
        Unit::Bytes,
        "bytes of fixed scaffolding included in assembled contexts",
        NO_CONTEXT_PATH,
    ),
    row(
        "context.tokens.unique",
        Unit::Events,
        "tokens of content included for the first time",
        NO_CONTEXT_PATH,
    ),
    row(
        "context.tokens.repeated",
        Unit::Events,
        "tokens of content included again after having been included before",
        NO_CONTEXT_PATH,
    ),
    row(
        "context.stale_input.checks",
        Unit::Events,
        "one check of whether an input had moved on",
        NO_CONTEXT_PATH,
    ),
    row(
        "context.stale_input.detected",
        Unit::Events,
        "one check that found an input had moved on",
        NO_CONTEXT_PATH,
    ),
    row(
        "context.cache.lookups",
        Unit::Events,
        "one lookup in the assembled-context cache",
        NO_CONTEXT_PATH,
    ),
    row(
        "context.cache.hits",
        Unit::Events,
        "one lookup that was answered from the cache; divide by the lookups for the hit rate",
        NO_CONTEXT_PATH,
    ),
    row(
        "context.task.completions",
        Unit::Events,
        "one agent task carried to an answer",
        NO_CONTEXT_PATH,
    ),
    row(
        "context.task.ns",
        Unit::Nanoseconds,
        "time inside agent tasks, summed",
        NO_CONTEXT_PATH,
    ),
    row(
        "context.output.accepted",
        Unit::Events,
        "one agent output a person accepted",
        NO_CONTEXT_PATH,
    ),
    row(
        "context.output.rejected",
        Unit::Events,
        "one agent output a person did not accept",
        NO_CONTEXT_PATH,
    ),
];

/// Plan §12.3's "Context" bullets, in the plan's order.
pub const REQUIRED: &[RequiredMetric] = &[
    RequiredMetric {
        metric: "fixed harness overhead",
        family: Family::Context,
        counters: &["context.harness.assemblies", "context.harness.fixed_bytes"],
    },
    RequiredMetric {
        metric: "unique content tokens",
        family: Family::Context,
        counters: &["context.tokens.unique"],
    },
    RequiredMetric {
        metric: "repeated content tokens",
        family: Family::Context,
        counters: &["context.tokens.repeated"],
    },
    RequiredMetric {
        metric: "stale-input detection",
        family: Family::Context,
        counters: &["context.stale_input.checks", "context.stale_input.detected"],
    },
    RequiredMetric {
        metric: "context cache hit rate",
        family: Family::Context,
        counters: &["context.cache.lookups", "context.cache.hits"],
    },
    RequiredMetric {
        metric: "task latency",
        family: Family::Context,
        counters: &["context.task.completions", "context.task.ns"],
    },
    RequiredMetric {
        metric: "accepted output quality",
        family: Family::Context,
        counters: &["context.output.accepted", "context.output.rejected"],
    },
];
