//! The workload generators: plan §12.2's W1–W6, and the W7 this repository
//! added for the storage-amplification measurement.
//!
//! Six datasets carry the whole benchmark argument — small codebase, large
//! monorepo, agent swarm, mixed business workspace, large binary, failure
//! workload — and every later number, every baseline comparison and every
//! structural-win claim is measured against them. So the deliverable of this
//! module is not speed and it is not fidelity. It is **reproducibility**: the
//! same seed produces the same workload, in this process, in the next process,
//! and on somebody else's machine, or the numbers measured on it prove nothing.
//!
//! # How that is made true, and how it is checked
//!
//! * Nothing here reads a clock, draws entropy, or looks at the environment.
//!   Every value comes from the seed through [`rng::stream`].
//! * Nothing here depends on iteration order of a hash map, on pointer values,
//!   or on floating point — the size rescale is integer arithmetic for exactly
//!   that reason.
//! * A generated workload is an ordered stream of [`plan::Item`], and that
//!   stream folds to a [`digest`] that a third party can compare against
//!   `benchmarks/workloads/manifest.json` without regenerating a byte.
//! * `crates/mesh-bench/tests/workload-generators.rs` re-runs the generators in
//!   **separate processes** and compares. Inspection is not evidence of
//!   cross-process determinism; a second process is.
//!
//! # Description first, bytes second
//!
//! W2 is a hundred gigabytes. Verifying it by writing it is not a test anyone
//! will run, so a generator produces a *description* first — paths, sizes, byte
//! profiles, edits, faults — and [`materialize`] turns that into bytes only
//! when a benchmark actually needs them. The description is cheap enough to
//! digest at full scale; the bytes are a deterministic function of it.
//!
//! What that costs, stated rather than buried: a plan digest proves two
//! machines agree on *what the corpus is*, not that they wrote identical bytes.
//! The content digest proves the second, and it costs what the corpus costs.
//! `benchmarks/workloads/manifest.json` records which of the two was measured
//! at which scale, and never lets one stand in for the other.
//!
//! # Scales
//!
//! [`Scale::Full`] is plan §12.2 exactly. [`Scale::Reduced`] is the same shape
//! on a laptop, and [`Scale::Smoke`] is small enough for a test. A result
//! measured at a reduced scale is published as a reduced-scale result — the
//! task's failure clause is explicit that a scale that could not be reached is
//! stated rather than extrapolated from.

pub mod content;
pub mod digest;
pub mod materialize;
pub mod plan;
pub mod rng;
pub mod shape;
pub mod tree;

mod w1;
mod w2;
mod w3;
mod w4;
mod w5;
mod w6;
mod w7;

pub use w1::SmallCodebase;
pub use w2::LargeMonorepo;
pub use w3::AgentSwarm;
pub use w4::BusinessWorkspace;
pub use w5::LargeBinary;
pub use w6::FailureWorkload;
pub use w7::{
    AgentWeek, AgentWeekParameters, NEAR_THRESHOLD_FILE_BYTES, PLAN_WHOLE_FILE_THRESHOLD_BYTES,
};

use crate::json::{Json, JsonObject};
use crate::schema::WorkloadDescriptor;
use plan::Item;
use shape::ShapeReport;

/// The generator family's identity, recorded in every row measured against it.
pub const GENERATOR_FAMILY: &str = "mesh-bench/corpus";

/// The generator family's algorithm version.
///
/// **Bumping this invalidates every published digest and every comparison
/// across the bump.** Any change to path construction, size distribution,
/// content bytes, item order or digest framing is a bump; nothing else is.
pub const GENERATOR_VERSION: &str = "1";

/// The six workloads of plan §12.2, and the one this repository added.
///
/// W1–W6 are the plan's, with the plan's own magnitudes. **W7 is not in the
/// plan**: it was added by `01KZE5FDN0NPGJ6NQ1NBYRFVH0` because none of plan
/// §12.4's release targets budgets a byte on disk, so nothing in W1–W6 produces
/// the repeated-edit stream a storage-amplification number has to be measured
/// over. [`PLAN_WORKLOADS`] is the plan's six on their own, so a caller that
/// means "what the plan asked for" can still say exactly that.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum WorkloadId {
    /// Small codebase: 20,000 files, 1.5 GB, mostly text.
    W1,
    /// Large monorepo: 1,000,000 files, 100 GB, only 5% accessed.
    W2,
    /// Agent swarm: 100 actors, 10 files each, 20% overlap, frequent checkpoints.
    W3,
    /// Mixed business workspace: 5,000 files of office formats, 20 GB.
    W4,
    /// Large binary: a 1 GiB file under insertion, append and random rewrites.
    W5,
    /// Failure workload: partitions, duplicates, reorders, crashes, disk full,
    /// corruption and stale reconnects.
    W6,
    /// Agent week: seven days of repeated edits across the 1 MiB whole-file
    /// threshold. Added here, not by plan §12.2.
    W7,
}

/// Every workload this crate generates, in order.
pub const WORKLOADS: [WorkloadId; 7] = [
    WorkloadId::W1,
    WorkloadId::W2,
    WorkloadId::W3,
    WorkloadId::W4,
    WorkloadId::W5,
    WorkloadId::W6,
    WorkloadId::W7,
];

/// The six plan §12.2 named, without the one this repository added.
pub const PLAN_WORKLOADS: [WorkloadId; 6] = [
    WorkloadId::W1,
    WorkloadId::W2,
    WorkloadId::W3,
    WorkloadId::W4,
    WorkloadId::W5,
    WorkloadId::W6,
];

impl WorkloadId {
    /// The plan's code: `W1` … `W6`.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            WorkloadId::W1 => "W1",
            WorkloadId::W2 => "W2",
            WorkloadId::W3 => "W3",
            WorkloadId::W4 => "W4",
            WorkloadId::W5 => "W5",
            WorkloadId::W6 => "W6",
            WorkloadId::W7 => "W7",
        }
    }

    /// The workload's name, as plan §12.2 titles it.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            WorkloadId::W1 => "small codebase",
            WorkloadId::W2 => "large monorepo",
            WorkloadId::W3 => "agent swarm",
            WorkloadId::W4 => "mixed business workspace",
            WorkloadId::W5 => "large binary",
            WorkloadId::W6 => "failure workload",
            WorkloadId::W7 => "agent week",
        }
    }

    /// Whether plan §12.2 names this workload.
    #[must_use]
    pub fn is_from_the_plan(self) -> bool {
        PLAN_WORKLOADS.contains(&self)
    }

    /// Parses `W1`…`W6`, case-insensitively.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        WORKLOADS
            .into_iter()
            .find(|workload| workload.code().eq_ignore_ascii_case(text))
    }

    /// The generator name recorded in a result row.
    #[must_use]
    pub fn generator_name(self) -> String {
        format!("{GENERATOR_FAMILY}/{}", self.code())
    }
}

/// How much of a workload to generate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scale {
    /// Plan §12.2 as written.
    Full,
    /// The same shape, small enough to generate on a developer machine.
    Reduced,
    /// Small enough for a test.
    Smoke,
}

/// Every scale, largest first.
pub const SCALES: [Scale; 3] = [Scale::Full, Scale::Reduced, Scale::Smoke];

impl Scale {
    /// The scale's name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Scale::Full => "full",
            Scale::Reduced => "reduced",
            Scale::Smoke => "smoke",
        }
    }

    /// Parses a scale name, case-insensitively.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        SCALES
            .into_iter()
            .find(|scale| scale.name().eq_ignore_ascii_case(text))
    }
}

/// The seed every published manifest row is generated from.
///
/// A single canonical seed is what makes two runs comparable at all; any other
/// seed produces a different corpus and therefore a different benchmark.
pub const CANONICAL_SEED: u64 = 42;

/// A deterministic workload dataset.
pub trait Generator {
    /// Which of the six this is.
    fn id(&self) -> WorkloadId;

    /// Which scale it was built at.
    fn scale(&self) -> Scale;

    /// The seed it was built from.
    fn seed(&self) -> u64;

    /// The parameters that, with the seed and the version, determine everything.
    fn parameters(&self) -> JsonObject;

    /// The workload, as one ordered stream.
    ///
    /// Lazy: a full-scale W2 must not need a million items in memory at once.
    fn items(&self) -> Box<dyn Iterator<Item = Item> + '_>;

    /// What the generator intended, and what it produced.
    fn shape(&self) -> ShapeReport;

    /// The description-only digest.
    fn plan_digest(&self) -> String {
        digest::plan_digest(self.items())
    }

    /// The digest that includes every generated byte.
    ///
    /// Costs what the corpus costs; see the module comment.
    fn content_digest(&self) -> String {
        digest::content_digest(self.items())
    }

    /// The descriptor a benchmark row carries so the corpus can be regenerated.
    fn descriptor(&self) -> WorkloadDescriptor {
        WorkloadDescriptor {
            generator: self.id().generator_name(),
            generator_version: GENERATOR_VERSION.to_owned(),
            seed: self.seed(),
            parameters: self
                .parameters()
                .with("scale", Json::string(self.scale().name())),
        }
    }

    /// The full description, for `mesh-bench corpus describe`.
    fn describe(&self) -> Json {
        Json::Object(
            JsonObject::new()
                .with("workload", Json::string(self.id().code()))
                .with("title", Json::string(self.id().title()))
                .with("scale", Json::string(self.scale().name()))
                .with("seed", Json::Uint(self.seed()))
                .with("generator", Json::string(self.id().generator_name()))
                .with("generator_version", Json::string(GENERATOR_VERSION))
                .with("parameters", Json::Object(self.parameters()))
                .with("shape", self.shape().to_json())
                .with("plan_digest", Json::string(self.plan_digest())),
        )
    }
}

/// Builds a workload at a scale from a seed.
///
/// The one entry point: the CLI, the tests and every downstream benchmark all
/// come through here, so no caller can accidentally construct a variant nobody
/// else can reproduce.
#[must_use]
pub fn build(id: WorkloadId, scale: Scale, seed: u64) -> Box<dyn Generator> {
    match id {
        WorkloadId::W1 => Box::new(SmallCodebase::new(seed, scale)),
        WorkloadId::W2 => Box::new(LargeMonorepo::new(seed, scale)),
        WorkloadId::W3 => Box::new(AgentSwarm::new(seed, scale)),
        WorkloadId::W4 => Box::new(BusinessWorkspace::new(seed, scale)),
        WorkloadId::W5 => Box::new(LargeBinary::new(seed, scale)),
        WorkloadId::W6 => Box::new(FailureWorkload::new(seed, scale)),
        WorkloadId::W7 => Box::new(AgentWeek::new(seed, scale)),
    }
}

/// Counts what an item stream contains, without generating any content.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    /// How many files.
    pub files: u64,
    /// Their total logical size.
    pub logical_bytes: u64,
    /// How many of them carry a text profile.
    pub text_files: u64,
    /// How many access items.
    pub accesses: u64,
    /// How many activity items.
    pub activities: u64,
    /// How many actor changes.
    pub changes: u64,
    /// How many checkpoints.
    pub checkpoints: u64,
    /// How many edits.
    pub edits: u64,
    /// How many faults.
    pub faults: u64,
}

impl Tally {
    /// Walks a stream and counts it.
    #[must_use]
    pub fn of(items: impl Iterator<Item = Item>) -> Self {
        let mut tally = Tally::default();
        for item in items {
            match item {
                Item::File(file) => {
                    tally.files += 1;
                    tally.logical_bytes += file.bytes;
                    if file.kind.is_text() {
                        tally.text_files += 1;
                    }
                }
                Item::Access { .. } => tally.accesses += 1,
                Item::Activity { .. } => tally.activities += 1,
                Item::Change(_) => tally.changes += 1,
                Item::Checkpoint(_) => tally.checkpoints += 1,
                Item::Edit { .. } => tally.edits += 1,
                Item::Fault(_) => tally.faults += 1,
            }
        }
        tally
    }

    /// The share of files carrying a text profile, or zero when there are none.
    #[must_use]
    pub fn text_ratio(&self) -> f64 {
        if self.files == 0 {
            0.0
        } else {
            self.text_files as f64 / self.files as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_workload_has_a_distinct_code_and_title() {
        let mut codes: Vec<&str> = WORKLOADS.iter().map(|id| id.code()).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), WORKLOADS.len());
        let mut titles: Vec<&str> = WORKLOADS.iter().map(|id| id.title()).collect();
        titles.sort_unstable();
        titles.dedup();
        assert_eq!(titles.len(), WORKLOADS.len());
    }

    #[test]
    fn codes_round_trip_through_parsing() {
        for id in WORKLOADS {
            assert_eq!(WorkloadId::parse(id.code()), Some(id));
            assert_eq!(WorkloadId::parse(&id.code().to_lowercase()), Some(id));
        }
        assert_eq!(WorkloadId::parse("W8"), None);
    }

    #[test]
    fn scales_round_trip_through_parsing() {
        for scale in SCALES {
            assert_eq!(Scale::parse(scale.name()), Some(scale));
            assert_eq!(Scale::parse(&scale.name().to_uppercase()), Some(scale));
        }
        assert_eq!(Scale::parse("enormous"), None);
    }

    #[test]
    fn every_workload_builds_at_every_scale_and_holds_its_shape() {
        for id in WORKLOADS {
            for scale in SCALES {
                let generator = build(id, scale, CANONICAL_SEED);
                let report = generator.shape();
                assert!(
                    report.holds(),
                    "{} at {} violates {:?}",
                    id.code(),
                    scale.name(),
                    report
                        .violations()
                        .iter()
                        .map(|fact| fact.name.as_str())
                        .collect::<Vec<_>>()
                );
            }
        }
    }

    #[test]
    fn every_workload_is_deterministic_within_one_process() {
        for id in WORKLOADS {
            let first = build(id, Scale::Smoke, CANONICAL_SEED).plan_digest();
            let second = build(id, Scale::Smoke, CANONICAL_SEED).plan_digest();
            assert_eq!(first, second, "{} drifted", id.code());
        }
    }

    #[test]
    fn every_workload_has_content_that_is_deterministic_at_smoke_scale() {
        for id in WORKLOADS {
            let first = build(id, Scale::Smoke, CANONICAL_SEED).content_digest();
            let second = build(id, Scale::Smoke, CANONICAL_SEED).content_digest();
            assert_eq!(first, second, "{} content drifted", id.code());
        }
    }

    #[test]
    fn no_two_workloads_generate_the_same_corpus() {
        let mut digests: Vec<String> = WORKLOADS
            .iter()
            .map(|id| build(*id, Scale::Smoke, CANONICAL_SEED).plan_digest())
            .collect();
        digests.sort();
        digests.dedup();
        assert_eq!(digests.len(), WORKLOADS.len());
    }

    /// The plan's six stay the plan's six. A future workload added here must not
    /// be able to slide into a list that says it came from plan 12.2.
    #[test]
    fn the_plan_workloads_are_the_plan_s_and_w7_is_not_one_of_them() {
        assert_eq!(PLAN_WORKLOADS.len(), 6);
        for id in PLAN_WORKLOADS {
            assert!(id.is_from_the_plan(), "{} left the plan list", id.code());
            assert!(WORKLOADS.contains(&id));
        }
        assert!(!WorkloadId::W7.is_from_the_plan());
        assert_eq!(WorkloadId::W7.title(), "agent week");
    }

    #[test]
    fn a_different_seed_gives_a_different_corpus() {
        for id in WORKLOADS {
            let left = build(id, Scale::Smoke, 1).plan_digest();
            let right = build(id, Scale::Smoke, 2).plan_digest();
            assert_ne!(left, right, "{} ignores its seed", id.code());
        }
    }

    #[test]
    fn a_different_scale_gives_a_different_corpus() {
        for id in WORKLOADS {
            let smoke = build(id, Scale::Smoke, CANONICAL_SEED).plan_digest();
            let reduced = build(id, Scale::Reduced, CANONICAL_SEED).plan_digest();
            assert_ne!(smoke, reduced, "{} ignores its scale", id.code());
        }
    }

    #[test]
    fn the_descriptor_carries_everything_needed_to_regenerate() {
        let generator = build(WorkloadId::W1, Scale::Smoke, 7);
        let descriptor = generator.descriptor();
        assert_eq!(descriptor.generator, "mesh-bench/corpus/W1");
        assert_eq!(descriptor.generator_version, GENERATOR_VERSION);
        assert_eq!(descriptor.seed, 7);
        assert_eq!(
            descriptor.parameters.get("scale").and_then(Json::as_str),
            Some("smoke")
        );
    }

    #[test]
    fn describe_carries_the_shape_and_the_digest() {
        let described = build(WorkloadId::W3, Scale::Smoke, CANONICAL_SEED).describe();
        let object = described.as_object().expect("an object");
        assert_eq!(object.get("workload").and_then(Json::as_str), Some("W3"));
        assert!(object.get("shape").is_some());
        assert!(object
            .get("plan_digest")
            .and_then(Json::as_str)
            .is_some_and(|digest| digest.starts_with("fnv1a64:")));
    }

    #[test]
    fn every_file_path_is_relative_and_unique_at_smoke_scale() {
        for id in WORKLOADS {
            let generator = build(id, Scale::Smoke, CANONICAL_SEED);
            let mut paths: Vec<String> = generator
                .items()
                .filter_map(|item| item.as_file().map(|file| file.path.clone()))
                .collect();
            assert!(
                paths.iter().all(|path| !path.starts_with('/')),
                "{} produced an absolute path",
                id.code()
            );
            assert!(
                paths.iter().all(|path| !path.contains("..")),
                "{} produced a traversing path",
                id.code()
            );
            let total = paths.len();
            paths.sort();
            paths.dedup();
            assert_eq!(paths.len(), total, "{} repeated a path", id.code());
        }
    }

    #[test]
    fn a_tally_counts_every_variant() {
        let generator = build(WorkloadId::W3, Scale::Smoke, CANONICAL_SEED);
        let tally = Tally::of(generator.items());
        assert!(tally.files > 0);
        assert!(tally.changes > 0);
        assert!(tally.checkpoints > 0);
        assert!(tally.logical_bytes > 0);
    }

    #[test]
    fn an_empty_tally_reports_no_text_rather_than_dividing_by_zero() {
        assert_eq!(Tally::default().text_ratio(), 0.0);
    }
}
