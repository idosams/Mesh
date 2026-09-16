//! The edge between plan §12.3 and the counters: who is named, who is live, and who is not.
//!
//! `## Acceptance criteria` bullet 1 is "every required benchmark metric has a counter behind it".
//! A catalogue row with [`Producer::NotYet`](super::catalogue::Producer::NotYet) is not an
//! implemented counter: it is a useful reservation of a stable name, but nothing in this build can
//! produce a measurement for it. [`gaps`] therefore reports both structural catalogue failures and
//! required metrics with no live producer. It becomes empty only when the acceptance criterion is
//! actually true. [`catalogue_gaps`] remains the narrower schema check for callers that only need
//! to know whether the two tables agree.
//!
//! Two directions, deliberately:
//!
//! - a required metric with no counter is an **uncovered metric**, and it means the acceptance
//!   criterion is not met;
//! - a counter no required metric names is an **orphan**, and it means the catalogue grew a row
//!   nobody asked for. An orphan is not harmless: it is how a counter table stops being an answer
//!   to plan §12.3 and starts being an answer to whatever a lane found convenient.
//!
//! A third structural check has teeth the other two do not: a required metric covered **only** by
//! nanosecond counters is reported as [`Gap::NoDeterministicCounter`]. That is the whole point of
//! this task's brief — a metric backed by nothing but wall-clock has no reading that survives a
//! loaded machine, and a gate built on it teaches its readers to re-run rather than read.

use super::catalogue::{counters, required_metrics, spec, Determinism};

/// A way the counter catalogue fails to answer plan §12.3.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Gap {
    /// A required metric names no counter at all.
    UncoveredMetric {
        /// The metric, as plan §12.3 names it.
        metric: &'static str,
    },
    /// A required metric names a counter the catalogue does not have.
    DanglingCounter {
        /// The metric that names it.
        metric: &'static str,
        /// The key that does not resolve.
        key: &'static str,
    },
    /// A required metric is backed only by wall-clock counters.
    NoDeterministicCounter {
        /// The metric with no reading that survives a loaded machine.
        metric: &'static str,
    },
    /// A required metric has catalogue rows, but none has a producer in this build.
    NoWiredCounter {
        /// The metric that cannot currently produce a measurement.
        metric: &'static str,
    },
    /// A counter no required metric names.
    OrphanCounter {
        /// The key nothing asked for.
        key: &'static str,
    },
}

impl core::fmt::Display for Gap {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UncoveredMetric { metric } => {
                write!(formatter, "plan 12.3 `{metric}` has no counter behind it")
            }
            Self::DanglingCounter { metric, key } => write!(
                formatter,
                "plan 12.3 `{metric}` names `{key}`, which is not in the counter catalogue"
            ),
            Self::NoDeterministicCounter { metric } => write!(
                formatter,
                "plan 12.3 `{metric}` is backed only by wall-clock counters, so it has no reading \
                 that survives a loaded machine"
            ),
            Self::NoWiredCounter { metric } => write!(
                formatter,
                "plan 12.3 `{metric}` has catalogue names but no counter producer in this build"
            ),
            Self::OrphanCounter { key } => {
                write!(formatter, "counter `{key}` is named by no plan 12.3 metric")
            }
        }
    }
}

/// Structural disagreement between the required-metric and counter tables.
///
/// This deliberately ignores whether a counter has a producer. Use [`gaps`] for the task's actual
/// completion oracle.
#[must_use]
pub fn catalogue_gaps() -> Vec<Gap> {
    let mut found = Vec::new();
    let mut named: Vec<&'static str> = Vec::new();

    for required in required_metrics() {
        if required.counters.is_empty() {
            found.push(Gap::UncoveredMetric {
                metric: required.metric,
            });
            continue;
        }
        let mut deterministic = 0_usize;
        for key in required.counters {
            named.push(key);
            match spec(key) {
                None => found.push(Gap::DanglingCounter {
                    metric: required.metric,
                    key,
                }),
                Some(entry) => {
                    if entry.determinism() == Determinism::Deterministic {
                        deterministic += 1;
                    }
                }
            }
        }
        if deterministic == 0 {
            found.push(Gap::NoDeterministicCounter {
                metric: required.metric,
            });
        }
    }

    for entry in counters() {
        if !named.contains(&entry.key) {
            found.push(Gap::OrphanCounter { key: entry.key });
        }
    }
    found
}

/// Every way this build fails to provide a usable counter for every required metric.
///
/// Empty only when the catalogue is structurally sound **and** every required metric has at least
/// one counter with a live producer.
#[must_use]
pub fn gaps() -> Vec<Gap> {
    let mut found = catalogue_gaps();
    for required in required_metrics() {
        let has_wired_counter = required
            .counters
            .iter()
            .filter_map(|key| spec(key))
            .any(|entry| entry.producer.is_wired());
        if !has_wired_counter {
            found.push(Gap::NoWiredCounter {
                metric: required.metric,
            });
        }
    }
    found
}

/// How many required metrics name at least one deterministic counter specification.
///
/// This is a catalogue-shape count, not a live-producer count. Use
/// [`wired_required_metric_count`] for the latter.
#[must_use]
pub fn deterministically_named() -> usize {
    required_metrics()
        .filter(|required| {
            required
                .counters
                .iter()
                .filter_map(|key| spec(key))
                .any(|entry| entry.determinism() == Determinism::Deterministic)
        })
        .count()
}

/// How many required metrics have at least one counter with a live producer in this build.
#[must_use]
pub fn wired_required_metric_count() -> usize {
    required_metrics()
        .filter(|required| {
            required
                .counters
                .iter()
                .filter_map(|key| spec(key))
                .any(|entry| entry.producer.is_wired())
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::counters::catalogue::{counter_count, required_metric_count};

    #[test]
    fn every_required_metric_has_a_catalogue_row_and_no_row_is_an_orphan() {
        let found = catalogue_gaps();
        assert!(
            found.is_empty(),
            "the counter catalogue does not answer plan 12.3:\n{}",
            found
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    #[test]
    fn completion_remains_red_while_required_metrics_have_no_live_producer() {
        let found = gaps();
        assert_eq!(wired_required_metric_count(), 4);
        assert_eq!(found.len(), required_metric_count() - 4);
        assert!(
            found
                .iter()
                .all(|gap| matches!(gap, Gap::NoWiredCounter { .. })),
            "the catalogue shape is sound; the remaining gaps must be live-producer gaps: {found:?}"
        );
    }

    #[test]
    fn every_required_metric_keeps_a_reading_that_survives_a_loaded_machine() {
        assert_eq!(
            deterministically_named(),
            required_metric_count(),
            "a metric backed only by wall-clock is a metric with no gate-safe reading"
        );
    }

    #[test]
    fn the_two_tables_are_the_size_the_plan_makes_them() {
        // 13 local filesystem + 8 workspace operations + 8 synchronization + 7 context.
        // Plan 12.3 writes "random/sequential read", "random/sequential write" and
        // "relay CPU/storage" as one bullet each; they are two metrics each here, because one row
        // would have had to pick a unit and lose the other.
        assert_eq!(required_metric_count(), 36);
        assert!(
            counter_count() >= required_metric_count(),
            "fewer counters than metrics means at least one metric shares a counter"
        );
    }

    #[test]
    fn every_gap_reads_as_a_sentence_naming_what_is_missing() {
        let samples = [
            Gap::UncoveredMetric { metric: "lookup" },
            Gap::DanglingCounter {
                metric: "lookup",
                key: "nope",
            },
            Gap::NoDeterministicCounter { metric: "lookup" },
            Gap::NoWiredCounter { metric: "lookup" },
            Gap::OrphanCounter { key: "nope" },
        ];
        for gap in samples {
            let sentence = gap.to_string();
            assert!(
                sentence.contains("lookup") || sentence.contains("nope"),
                "{sentence}"
            );
        }
    }
}
