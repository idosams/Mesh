//! W6 — failure workload.
//!
//! ```text
//! Network partitions
//! Duplicate messages
//! Message reorder
//! Process crashes
//! Disk full
//! Corrupt chunks
//! Old peer reconnect
//! ```
//!
//! The workload behind every durability and convergence claim. It is not a
//! corpus with a fault rate bolted on; it is a **schedule**, and the schedule is
//! the artifact — a replayable, seed-addressable sequence of injected failures
//! that a simulator campaign, a convergence test and a crash-recovery benchmark
//! can all be pointed at and get the identical run.
//!
//! Two rules shape it:
//!
//! 1. **All seven kinds appear, always.** The first seven faults are one of
//!    each, in the order plan §12.2 lists them, before the weighted draw starts.
//!    A schedule that happens to omit disk-full because the dice said so is a
//!    schedule that silently stops testing disk-full, and the acceptance
//!    criterion for this workload is that all seven are injected.
//! 2. **Position is a logical step, never a clock reading.** Faults carry a
//!    monotonically increasing step index. Replaying a schedule on a faster
//!    machine must produce the same run, and anything keyed to elapsed time
//!    would not.
//!
//! The small base corpus exists so W6 can be run on its own. It is also
//! designed to be replayed *against* another workload's corpus — nothing in the
//! schedule names a path from the base tree.

use super::plan::{ContentKind, FaultKind, FaultSpec, FileSpec, Item, FAULT_KINDS};
use super::rng::{stream, SplitMix64};
use super::shape::{ShapeFact, ShapeReport};
use super::tree::{path_for, size_ladder, SizeBand};
use super::{Generator, Scale, Tally, WorkloadId};
use crate::json::{Json, JsonObject};
use std::collections::BTreeSet;

/// The base tree: small, textual, and only there so the schedule has something
/// to be unfaithful to.
const BANDS: [SizeBand; 1] = [SizeBand {
    share_permille: 1000,
    min_bytes: 512,
    max_bytes: 32_768,
    kind: ContentKind::Text,
    extension: "txt",
}];

/// W6's stated shape at one scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FailureWorkloadParameters {
    /// How many files the base tree holds.
    pub base_files: usize,
    /// The base tree's total logical size, in bytes.
    pub base_bytes: u64,
    /// How many faults the schedule injects.
    pub faults: u64,
    /// How many logical steps the schedule spans.
    pub steps: u64,
    /// How many peers the schedule addresses.
    pub peers: u64,
}

impl FailureWorkloadParameters {
    /// A long campaign, a laptop campaign and a test campaign.
    ///
    /// Plan §12.2 states W6 as a list of failure kinds rather than as
    /// magnitudes, so unlike W1–W5 there is no published number to be faithful
    /// to here. These are this generator's choices, stated as parameters so
    /// that a campaign that needs different ones changes a value rather than
    /// forking the generator.
    #[must_use]
    pub const fn for_scale(scale: Scale) -> Self {
        match scale {
            Scale::Full => FailureWorkloadParameters {
                base_files: 1_024,
                base_bytes: 16_000_000,
                faults: 512,
                steps: 8_192,
                peers: 8,
            },
            Scale::Reduced => FailureWorkloadParameters {
                base_files: 256,
                base_bytes: 4_000_000,
                faults: 128,
                steps: 2_048,
                peers: 5,
            },
            Scale::Smoke => FailureWorkloadParameters {
                base_files: 32,
                base_bytes: 200_000,
                faults: 21,
                steps: 128,
                peers: 3,
            },
        }
    }

    /// The parameter object recorded in descriptors and result rows.
    #[must_use]
    pub fn to_json(self) -> JsonObject {
        JsonObject::new()
            .with("base_files", Json::Uint(self.base_files as u64))
            .with("base_bytes", Json::Uint(self.base_bytes))
            .with("faults", Json::Uint(self.faults))
            .with("steps", Json::Uint(self.steps))
            .with("peers", Json::Uint(self.peers))
            .with("fault_kinds", Json::Uint(FAULT_KINDS.len() as u64))
    }
}

/// The magnitude range each kind draws from, and what the number means.
const fn magnitude_range(kind: FaultKind) -> (u64, u64) {
    match kind {
        // Peers cut off from the rest.
        FaultKind::NetworkPartition => (1, 3),
        // How many times the message is replayed.
        FaultKind::DuplicateMessage => (1, 4),
        // How many messages are held back before release.
        FaultKind::MessageReorder => (2, 16),
        // How many steps the process stays down.
        FaultKind::ProcessCrash => (1, 32),
        // Bytes the volume refuses.
        FaultKind::DiskFull => (4_096, 1_048_576),
        // Bytes corrupted in the chunk.
        FaultKind::CorruptChunk => (1, 4_096),
        // Steps the peer was away for.
        FaultKind::StalePeerReconnect => (64, 4_096),
    }
}

/// The W6 generator.
#[derive(Clone, Debug)]
pub struct FailureWorkload {
    seed: u64,
    scale: Scale,
    parameters: FailureWorkloadParameters,
    sizes: Vec<u64>,
}

impl FailureWorkload {
    /// Builds the generator for a seed and a scale.
    #[must_use]
    pub fn new(seed: u64, scale: Scale) -> Self {
        let parameters = FailureWorkloadParameters::for_scale(scale);
        let sizes = size_ladder(
            seed,
            "w6/size",
            parameters.base_files,
            &BANDS,
            parameters.base_bytes,
        );
        FailureWorkload {
            seed,
            scale,
            parameters,
            sizes,
        }
    }

    fn file(&self, index: usize) -> FileSpec {
        FileSpec {
            path: path_for("failure", index, BANDS[0].extension),
            bytes: self.sizes[index],
            kind: BANDS[0].kind,
            stream: stream(self.seed, "w6/content", index as u64),
        }
    }

    /// The schedule: one of every kind, then a weighted draw, at rising steps.
    fn schedule(&self) -> Vec<Item> {
        let parameters = self.parameters;
        let mut source = SplitMix64::derived(self.seed, "w6/schedule", 0);
        let mut faults = Vec::with_capacity(parameters.faults as usize);
        // Steps rise by a drawn gap so faults are not evenly spaced, and the
        // last one still lands inside the schedule's span.
        let gap = (parameters.steps / parameters.faults.max(1)).max(1);
        let mut step = 0_u64;

        for ordinal in 0..parameters.faults {
            let kind = if (ordinal as usize) < FAULT_KINDS.len() {
                FAULT_KINDS[ordinal as usize]
            } else {
                FAULT_KINDS[(source.next_u64() % FAULT_KINDS.len() as u64) as usize]
            };
            let (low, high) = magnitude_range(kind);
            let peer = source.in_range(0, parameters.peers - 1);
            faults.push(Item::Fault(FaultSpec {
                step,
                kind,
                subject: format!("peer-{peer}"),
                magnitude: source.in_range(low, high),
            }));
            step += source.in_range(1, gap.max(1) * 2);
        }
        faults
    }
}

impl Generator for FailureWorkload {
    fn id(&self) -> WorkloadId {
        WorkloadId::W6
    }

    fn scale(&self) -> Scale {
        self.scale
    }

    fn seed(&self) -> u64 {
        self.seed
    }

    fn parameters(&self) -> JsonObject {
        self.parameters.to_json()
    }

    fn items(&self) -> Box<dyn Iterator<Item = Item> + '_> {
        let files = (0..self.parameters.base_files).map(move |index| Item::File(self.file(index)));
        Box::new(files.chain(self.schedule()))
    }

    fn shape(&self) -> ShapeReport {
        let parameters = self.parameters;
        let tally = Tally::of(self.items());
        let faults: Vec<FaultSpec> = self
            .items()
            .filter_map(|item| match item {
                Item::Fault(fault) => Some(fault),
                _ => None,
            })
            .collect();
        let kinds: BTreeSet<FaultKind> = faults.iter().map(|fault| fault.kind).collect();
        let monotonic = faults.windows(2).all(|pair| pair[0].step <= pair[1].step);

        let mut report = ShapeReport::new()
            .with(ShapeFact::count(
                "base_file_count",
                parameters.base_files as u64,
                tally.files,
            ))
            .with(ShapeFact::bytes(
                "base_bytes",
                parameters.base_bytes,
                tally.logical_bytes,
            ))
            .with(ShapeFact::count(
                "fault_count",
                parameters.faults,
                tally.faults,
            ))
            .with(ShapeFact::count(
                "distinct_fault_kinds",
                FAULT_KINDS.len() as u64,
                kinds.len() as u64,
            ))
            .with(ShapeFact::present("steps_are_monotonic", monotonic));
        for kind in FAULT_KINDS {
            report = report.with(ShapeFact::present(
                format!("injects_{}", kind.name()),
                kinds.contains(&kind),
            ));
        }
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::CANONICAL_SEED;

    #[test]
    fn all_seven_kinds_are_injected_at_every_scale_and_every_seed() {
        for scale in [Scale::Full, Scale::Reduced, Scale::Smoke] {
            for seed in 0..8 {
                let generator = FailureWorkload::new(seed, scale);
                let kinds: BTreeSet<FaultKind> = generator
                    .items()
                    .filter_map(|item| match item {
                        Item::Fault(fault) => Some(fault.kind),
                        _ => None,
                    })
                    .collect();
                assert_eq!(
                    kinds.len(),
                    7,
                    "{} seed {seed} injected only {:?}",
                    scale.name(),
                    kinds
                );
            }
        }
    }

    #[test]
    fn the_first_seven_faults_are_one_of_each_in_plan_order() {
        let generator = FailureWorkload::new(CANONICAL_SEED, Scale::Smoke);
        let kinds: Vec<FaultKind> = generator
            .items()
            .filter_map(|item| match item {
                Item::Fault(fault) => Some(fault.kind),
                _ => None,
            })
            .take(7)
            .collect();
        assert_eq!(kinds, FAULT_KINDS.to_vec());
    }

    #[test]
    fn steps_never_go_backwards() {
        let generator = FailureWorkload::new(CANONICAL_SEED, Scale::Full);
        let steps: Vec<u64> = generator
            .items()
            .filter_map(|item| match item {
                Item::Fault(fault) => Some(fault.step),
                _ => None,
            })
            .collect();
        assert!(steps.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(steps.len() > 1);
    }

    #[test]
    fn magnitudes_stay_inside_their_kind_range() {
        let generator = FailureWorkload::new(CANONICAL_SEED, Scale::Full);
        for item in generator.items() {
            if let Item::Fault(fault) = item {
                let (low, high) = magnitude_range(fault.kind);
                assert!(
                    (low..=high).contains(&fault.magnitude),
                    "{} magnitude {} outside {low}..={high}",
                    fault.kind.name(),
                    fault.magnitude
                );
            }
        }
    }

    #[test]
    fn every_fault_names_a_peer_that_exists() {
        let parameters = FailureWorkloadParameters::for_scale(Scale::Reduced);
        let generator = FailureWorkload::new(CANONICAL_SEED, Scale::Reduced);
        for item in generator.items() {
            if let Item::Fault(fault) = item {
                let peer = fault
                    .subject
                    .strip_prefix("peer-")
                    .and_then(|index| index.parse::<u64>().ok())
                    .expect("a peer index");
                assert!(peer < parameters.peers, "{} is out of range", fault.subject);
            }
        }
    }

    #[test]
    fn the_fault_count_is_exact() {
        for scale in [Scale::Full, Scale::Reduced, Scale::Smoke] {
            let parameters = FailureWorkloadParameters::for_scale(scale);
            let tally = Tally::of(FailureWorkload::new(CANONICAL_SEED, scale).items());
            assert_eq!(tally.faults, parameters.faults, "{}", scale.name());
        }
    }

    #[test]
    fn the_base_tree_is_present_so_the_workload_runs_alone() {
        let tally = Tally::of(FailureWorkload::new(CANONICAL_SEED, Scale::Smoke).items());
        assert_eq!(tally.files, 32);
        assert_eq!(tally.logical_bytes, 200_000);
    }

    #[test]
    fn the_schedule_names_no_path_from_the_base_tree() {
        // W6 is meant to be replayable against any other workload's corpus.
        let generator = FailureWorkload::new(CANONICAL_SEED, Scale::Smoke);
        for item in generator.items() {
            if let Item::Fault(fault) = item {
                assert!(
                    !fault.subject.starts_with("failure/"),
                    "{} ties the schedule to this corpus",
                    fault.subject
                );
            }
        }
    }

    #[test]
    fn the_shape_report_holds_at_every_scale() {
        for scale in [Scale::Full, Scale::Reduced, Scale::Smoke] {
            let report = FailureWorkload::new(CANONICAL_SEED, scale).shape();
            assert!(report.holds(), "{} {report:?}", scale.name());
        }
    }

    #[test]
    fn the_generator_is_reproducible() {
        let left = FailureWorkload::new(CANONICAL_SEED, Scale::Smoke);
        let right = FailureWorkload::new(CANONICAL_SEED, Scale::Smoke);
        assert_eq!(left.plan_digest(), right.plan_digest());
        assert_eq!(left.content_digest(), right.content_digest());
    }
}
