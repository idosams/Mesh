//! W7 — a week of agent work.
//!
//! ```text
//! A source tree, seven days, several actors
//! Files that sit just below the 1 MiB whole-file threshold of plan §6.2
//! Files above it, so the two halves of the chunk policy are both exercised
//! Repeated small edits, checkpointed per actor per day
//! ```
//!
//! # Why this is not one of plan §12.2's six
//!
//! W1–W6 are the plan's own workloads and their parameters are quoted from it.
//! W7 is not in the plan: it was added by task `01KZE5FDN0NPGJ6NQ1NBYRFVH0` to
//! answer a question plan §12.4 never asks. Every one of the plan's eighteen
//! release targets is a latency, a throughput, a transfer size or a count of
//! **one** operation; none of them budgets how many bytes a workspace occupies
//! after a week of use. Plan §2.5 makes that number grow monotonically by
//! construction, so it is the cost side of the headline feature, and until this
//! generator existed there was no reproducible input to measure it over.
//!
//! So the shape here is chosen rather than quoted, and the parameters say so.
//! What is *not* chosen is the one number that decides the answer.
//!
//! # The 1 MiB threshold is the whole point
//!
//! Plan §6.2 stores a file of 1 MiB or fewer as **one object named by the digest
//! of the whole file**. Every edit to such a file therefore admits a complete
//! new copy, however few bytes changed. Above the threshold, content-defined
//! chunking admits only the chunks the edit disturbed.
//!
//! A storage workload that only used small files would report a number nobody
//! can generalise from, and one that only used large files would report the
//! chunker's best case. This one deliberately straddles: `near_threshold_files`
//! sit a byte below the threshold — the worst case the policy has — and
//! `large_files` sit well above it. The first `near_threshold_files` edits of
//! the week are aimed one at each of the near-threshold files before the drawn
//! schedule starts, the same rule and for the same reason as W6's "all seven
//! fault kinds first": a workload that happened not to edit the expensive file
//! because the dice said so would silently stop measuring the expensive case.
//!
//! # A description, not bytes
//!
//! Like every generator here this one emits an ordered [`Item`] stream and
//! generates no content until a benchmark asks for it. The edits are described
//! and never applied: applying them is what the storage benchmark *measures*.

use super::plan::{ChangeSpec, CheckpointSpec, ContentKind, EditOp, FileSpec, Item};
use super::rng::{stream, SplitMix64};
use super::shape::{ShapeFact, ShapeReport};
use super::tree::{path_for, size_ladder, SizeBand};
use super::{Generator, Scale, Tally, WorkloadId};
use crate::json::{Json, JsonObject};
use std::collections::BTreeSet;

/// Where the near-threshold files live.
const NEAR_ROOT: &str = "week/near-threshold";
/// Where the files above the threshold live.
const LARGE_ROOT: &str = "week/large";
/// Where the ordinary source files live.
const SOURCE_ROOT: &str = "week/src";

/// Plan §6.2's whole-file threshold, in bytes.
///
/// Spelled here as well as in `mesh-chunking` on purpose: this crate declares no
/// dependency on that one, and the generator's job is to place files *relative*
/// to the threshold. A test asserts the two stay one byte apart in the direction
/// that matters — see [`the_near_threshold_file_is_below_the_plan_threshold`].
pub const PLAN_WHOLE_FILE_THRESHOLD_BYTES: u64 = 1024 * 1024;

/// The size a near-threshold file is given: one byte under the threshold.
pub const NEAR_THRESHOLD_FILE_BYTES: u64 = PLAN_WHOLE_FILE_THRESHOLD_BYTES - 1;

/// A week is seven days. Not a parameter, because the question is week-shaped.
pub const DAYS: u64 = 7;

/// The ordinary source files' size distribution.
const SOURCE_BANDS: [SizeBand; 2] = [
    SizeBand {
        share_permille: 850,
        min_bytes: 512,
        max_bytes: 24_576,
        kind: ContentKind::Text,
        extension: "rs",
    },
    SizeBand {
        share_permille: 150,
        min_bytes: 24_576,
        max_bytes: 262_144,
        kind: ContentKind::Text,
        extension: "md",
    },
];

/// W7's stated shape at one scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgentWeekParameters {
    /// How many ordinary source files the tree holds.
    pub source_files: usize,
    /// The ordinary source files' total logical size, in bytes.
    pub source_bytes: u64,
    /// How many files sit one byte below the whole-file threshold.
    pub near_threshold_files: usize,
    /// How many files sit above it.
    pub large_files: usize,
    /// The exact size of each file above the threshold, in bytes.
    pub large_file_bytes: u64,
    /// How many actors work during the week.
    pub actors: u64,
    /// How many edits each actor makes each day.
    pub edits_per_actor_day: u64,
    /// The share of a file's bytes one edit rewrites, in parts per thousand.
    pub edit_fraction_permille: u64,
}

impl AgentWeekParameters {
    /// A week at three sizes.
    ///
    /// Plan §12.2 states no W7, so unlike W1–W5 there is no published magnitude
    /// to be faithful to. These are this generator's choices, stated as
    /// parameters so a campaign that needs different ones changes a value rather
    /// than forking the generator. Only two things are fixed: the week is seven
    /// days, and the near-threshold files sit one byte below plan §6.2's
    /// threshold at every scale — a reduced scale that moved them would stop
    /// measuring the thing the workload exists for.
    #[must_use]
    pub const fn for_scale(scale: Scale) -> Self {
        match scale {
            Scale::Full => AgentWeekParameters {
                source_files: 2_048,
                source_bytes: 48_000_000,
                near_threshold_files: 4,
                large_files: 4,
                large_file_bytes: 8 * 1024 * 1024,
                actors: 8,
                edits_per_actor_day: 24,
                edit_fraction_permille: 10,
            },
            Scale::Reduced => AgentWeekParameters {
                source_files: 256,
                source_bytes: 6_000_000,
                near_threshold_files: 2,
                large_files: 2,
                large_file_bytes: 4 * 1024 * 1024,
                actors: 4,
                edits_per_actor_day: 8,
                edit_fraction_permille: 10,
            },
            Scale::Smoke => AgentWeekParameters {
                source_files: 30,
                source_bytes: 450_000,
                near_threshold_files: 1,
                large_files: 1,
                large_file_bytes: 2 * 1024 * 1024,
                actors: 2,
                edits_per_actor_day: 2,
                edit_fraction_permille: 10,
            },
        }
    }

    /// Every file in the tree.
    #[must_use]
    pub const fn file_count(self) -> usize {
        self.near_threshold_files + self.large_files + self.source_files
    }

    /// The tree's total logical size before a single edit.
    #[must_use]
    pub const fn base_bytes(self) -> u64 {
        self.near_threshold_files as u64 * NEAR_THRESHOLD_FILE_BYTES
            + self.large_files as u64 * self.large_file_bytes
            + self.source_bytes
    }

    /// How many edits the week contains.
    #[must_use]
    pub const fn edit_count(self) -> u64 {
        self.actors * DAYS * self.edits_per_actor_day
    }

    /// How many checkpoints the week contains: one per actor per day.
    #[must_use]
    pub const fn checkpoint_count(self) -> u64 {
        self.actors * DAYS
    }

    /// The parameter object recorded in descriptors and result rows.
    #[must_use]
    pub fn to_json(self) -> JsonObject {
        JsonObject::new()
            .with("source_files", Json::Uint(self.source_files as u64))
            .with("source_bytes", Json::Uint(self.source_bytes))
            .with(
                "near_threshold_files",
                Json::Uint(self.near_threshold_files as u64),
            )
            .with(
                "near_threshold_file_bytes",
                Json::Uint(NEAR_THRESHOLD_FILE_BYTES),
            )
            .with("large_files", Json::Uint(self.large_files as u64))
            .with("large_file_bytes", Json::Uint(self.large_file_bytes))
            .with(
                "whole_file_threshold_bytes",
                Json::Uint(PLAN_WHOLE_FILE_THRESHOLD_BYTES),
            )
            .with("actors", Json::Uint(self.actors))
            .with("days", Json::Uint(DAYS))
            .with("edits_per_actor_day", Json::Uint(self.edits_per_actor_day))
            .with(
                "edit_fraction_permille",
                Json::Uint(self.edit_fraction_permille),
            )
            .with("file_count", Json::Uint(self.file_count() as u64))
            .with("base_bytes", Json::Uint(self.base_bytes()))
            .with("edit_count", Json::Uint(self.edit_count()))
    }
}

/// The W7 generator.
#[derive(Clone, Debug)]
pub struct AgentWeek {
    seed: u64,
    scale: Scale,
    parameters: AgentWeekParameters,
    /// Every file's size, in stream order: near-threshold, then large, then source.
    sizes: Vec<u64>,
}

impl AgentWeek {
    /// Builds the generator for a seed and a scale.
    #[must_use]
    pub fn new(seed: u64, scale: Scale) -> Self {
        AgentWeek::from_parameters(seed, scale, AgentWeekParameters::for_scale(scale))
    }

    /// Builds the generator over an explicit shape rather than a scale's preset.
    ///
    /// The scale is still recorded, because a row has to say which of the three
    /// published shapes it is comparable with — and a run over parameters that
    /// are *not* one of them says so by carrying them, since every consumer
    /// records the whole parameter object rather than the scale's name alone.
    #[must_use]
    pub fn from_parameters(seed: u64, scale: Scale, parameters: AgentWeekParameters) -> Self {
        let source = size_ladder(
            seed,
            "w7/size",
            parameters.source_files,
            &SOURCE_BANDS,
            parameters.source_bytes,
        );
        let mut sizes = Vec::with_capacity(parameters.file_count());
        sizes.extend(std::iter::repeat_n(
            NEAR_THRESHOLD_FILE_BYTES,
            parameters.near_threshold_files,
        ));
        sizes.extend(std::iter::repeat_n(
            parameters.large_file_bytes,
            parameters.large_files,
        ));
        sizes.extend(source);
        AgentWeek {
            seed,
            scale,
            parameters,
            sizes,
        }
    }

    /// The parameters this generator was built with.
    #[must_use]
    pub const fn parameters_struct(&self) -> AgentWeekParameters {
        self.parameters
    }

    /// The path, profile and extension of file `index`.
    fn file(&self, index: usize) -> FileSpec {
        let parameters = self.parameters;
        let (root, ordinal, kind, extension) = if index < parameters.near_threshold_files {
            (NEAR_ROOT, index, ContentKind::Text, "rs")
        } else if index < parameters.near_threshold_files + parameters.large_files {
            (
                LARGE_ROOT,
                index - parameters.near_threshold_files,
                ContentKind::Binary,
                "bin",
            )
        } else {
            let ordinal = index - parameters.near_threshold_files - parameters.large_files;
            let band = &SOURCE_BANDS
                [super::tree::band_of(ordinal, parameters.source_files, &SOURCE_BANDS)];
            (SOURCE_ROOT, ordinal, band.kind, band.extension)
        };
        FileSpec {
            path: path_for(root, ordinal, extension),
            bytes: self.sizes[index],
            kind,
            stream: stream(self.seed, "w7/content", index as u64),
        }
    }

    /// Every file, in stream order.
    fn files(&self) -> Vec<FileSpec> {
        (0..self.parameters.file_count())
            .map(|index| self.file(index))
            .collect()
    }

    /// Which file the `ordinal`-th edit of the week targets.
    ///
    /// The first `near_threshold_files` edits go one to each near-threshold
    /// file, in order, before anything is drawn. See the module comment.
    fn target_of(&self, ordinal: u64) -> usize {
        let parameters = self.parameters;
        if (ordinal as usize) < parameters.near_threshold_files {
            return ordinal as usize;
        }
        let mut source = SplitMix64::derived(self.seed, "w7/target", ordinal);
        source.in_range(0, parameters.file_count() as u64 - 1) as usize
    }

    /// The whole week: edits and checkpoints, in replay order.
    ///
    /// Order is day, then actor, then that actor's edits for the day, with the
    /// actor's checkpoint closing each day. Position is an index into this
    /// stream — a logical step — and never a clock reading.
    fn week(&self) -> Vec<Item> {
        let parameters = self.parameters;
        let files = self.files();
        let mut items =
            Vec::with_capacity((parameters.edit_count() + parameters.checkpoint_count()) as usize);
        let mut ordinal = 0_u64;

        for _day in 0..DAYS {
            for actor in 0..parameters.actors {
                for _ in 0..parameters.edits_per_actor_day {
                    let target = self.target_of(ordinal);
                    let file = &files[target];
                    let length = edit_length(file.bytes, parameters.edit_fraction_permille);
                    let mut source = SplitMix64::derived(self.seed, "w7/offset", ordinal);
                    let offset = source.in_range(0, file.bytes.saturating_sub(length));
                    items.push(Item::Change(ChangeSpec {
                        actor: actor_name(actor),
                        path: file.path.clone(),
                        shared: target < parameters.near_threshold_files,
                        bytes: length,
                    }));
                    items.push(Item::Edit {
                        path: file.path.clone(),
                        op: EditOp::Overwrite { offset, length },
                        stream: stream(self.seed, "w7/edit", ordinal),
                    });
                    ordinal += 1;
                }
                items.push(Item::Checkpoint(CheckpointSpec {
                    actor: actor_name(actor),
                    covers_changes: parameters.edits_per_actor_day,
                }));
            }
        }
        items
    }
}

/// `actor-0000`-style, so the name sorts with the index.
fn actor_name(actor: u64) -> String {
    format!("actor-{actor:04}")
}

/// How many bytes one edit rewrites: a share of the file, never zero, never
/// more than the file.
fn edit_length(file_bytes: u64, fraction_permille: u64) -> u64 {
    let scaled = file_bytes.saturating_mul(fraction_permille) / 1000;
    scaled.clamp(1, file_bytes.max(1))
}

impl Generator for AgentWeek {
    fn id(&self) -> WorkloadId {
        WorkloadId::W7
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
        let files = self
            .files()
            .into_iter()
            .map(Item::File)
            .collect::<Vec<Item>>();
        Box::new(files.into_iter().chain(self.week()))
    }

    fn shape(&self) -> ShapeReport {
        let parameters = self.parameters;
        let tally = Tally::of(self.items());
        let files = self.files();

        let mut edited: BTreeSet<String> = BTreeSet::new();
        let mut inside = true;
        for item in self.items() {
            if let Item::Edit {
                path,
                op: EditOp::Overwrite { offset, length },
                ..
            } = item
            {
                let size = files
                    .iter()
                    .find(|file| file.path == path)
                    .map_or(0, |file| file.bytes);
                inside &= offset + length <= size;
                edited.insert(path);
            }
        }
        let near_threshold_edited = edited
            .iter()
            .filter(|path| path.starts_with(NEAR_ROOT))
            .count() as u64;
        let near_threshold_sized = files
            .iter()
            .filter(|file| file.bytes == NEAR_THRESHOLD_FILE_BYTES)
            .count() as u64;
        let above_threshold = files
            .iter()
            .filter(|file| file.bytes > PLAN_WHOLE_FILE_THRESHOLD_BYTES)
            .count() as u64;

        ShapeReport::new()
            .with(ShapeFact::count(
                "file_count",
                parameters.file_count() as u64,
                tally.files,
            ))
            .with(ShapeFact::bytes(
                "base_bytes",
                parameters.base_bytes(),
                tally.logical_bytes,
            ))
            .with(ShapeFact::count(
                "edit_count",
                parameters.edit_count(),
                tally.edits,
            ))
            .with(ShapeFact::count(
                "checkpoint_count",
                parameters.checkpoint_count(),
                tally.checkpoints,
            ))
            .with(ShapeFact::count(
                "near_threshold_file_count",
                parameters.near_threshold_files as u64,
                near_threshold_sized,
            ))
            .with(ShapeFact::count(
                "above_threshold_file_count",
                parameters.large_files as u64,
                above_threshold,
            ))
            .with(ShapeFact::count(
                "near_threshold_files_edited",
                parameters.near_threshold_files as u64,
                near_threshold_edited,
            ))
            .with(ShapeFact::present(
                "every_edit_lands_inside_its_file",
                inside,
            ))
            .with(ShapeFact::present(
                "the_week_edits_more_than_one_file",
                edited.len() > 1,
            ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::CANONICAL_SEED;

    #[test]
    fn a_week_is_seven_days_at_every_scale() {
        assert_eq!(DAYS, 7);
        for scale in [Scale::Full, Scale::Reduced, Scale::Smoke] {
            let parameters = AgentWeekParameters::for_scale(scale);
            assert_eq!(
                parameters.edit_count(),
                parameters.actors * 7 * parameters.edits_per_actor_day,
                "{}",
                scale.name()
            );
        }
    }

    /// The criterion this workload exists for: a file just below the threshold.
    #[test]
    fn the_near_threshold_file_is_below_the_plan_threshold() {
        assert_eq!(
            PLAN_WHOLE_FILE_THRESHOLD_BYTES, 1_048_576,
            "plan 6.2: 1 MiB"
        );
        assert_eq!(NEAR_THRESHOLD_FILE_BYTES, 1_048_576 - 1);
        const { assert!(NEAR_THRESHOLD_FILE_BYTES < PLAN_WHOLE_FILE_THRESHOLD_BYTES) };
    }

    #[test]
    fn every_scale_carries_a_near_threshold_file_and_one_above_it() {
        for scale in [Scale::Full, Scale::Reduced, Scale::Smoke] {
            let generator = AgentWeek::new(CANONICAL_SEED, scale);
            let sizes: Vec<u64> = generator
                .items()
                .filter_map(|item| item.as_file().map(|file| file.bytes))
                .collect();
            assert!(
                sizes.contains(&NEAR_THRESHOLD_FILE_BYTES),
                "{} has no file just below the threshold",
                scale.name()
            );
            assert!(
                sizes
                    .iter()
                    .any(|size| *size > PLAN_WHOLE_FILE_THRESHOLD_BYTES),
                "{} has no file above the threshold, so the chunked arm is never exercised",
                scale.name()
            );
        }
    }

    #[test]
    fn the_first_edits_of_the_week_land_on_the_near_threshold_files() {
        for scale in [Scale::Full, Scale::Reduced, Scale::Smoke] {
            let parameters = AgentWeekParameters::for_scale(scale);
            let generator = AgentWeek::new(CANONICAL_SEED, scale);
            let first: Vec<String> = generator
                .items()
                .filter_map(|item| match item {
                    Item::Edit { path, .. } => Some(path),
                    _ => None,
                })
                .take(parameters.near_threshold_files)
                .collect();
            for (ordinal, path) in first.iter().enumerate() {
                assert_eq!(
                    *path,
                    path_for(NEAR_ROOT, ordinal, "rs"),
                    "{} edit {ordinal} missed its near-threshold file",
                    scale.name()
                );
            }
        }
    }

    #[test]
    fn the_near_threshold_rule_holds_for_every_seed() {
        for seed in 0..8 {
            let parameters = AgentWeekParameters::for_scale(Scale::Smoke);
            let generator = AgentWeek::new(seed, Scale::Smoke);
            let edited: BTreeSet<String> = generator
                .items()
                .filter_map(|item| match item {
                    Item::Edit { path, .. } => Some(path),
                    _ => None,
                })
                .filter(|path| path.starts_with(NEAR_ROOT))
                .collect();
            assert_eq!(
                edited.len(),
                parameters.near_threshold_files,
                "seed {seed} left a near-threshold file unedited"
            );
        }
    }

    #[test]
    fn every_edit_lands_inside_its_file() {
        for scale in [Scale::Full, Scale::Reduced, Scale::Smoke] {
            let generator = AgentWeek::new(CANONICAL_SEED, scale);
            let sizes: std::collections::BTreeMap<String, u64> = generator
                .items()
                .filter_map(|item| item.as_file().map(|file| (file.path.clone(), file.bytes)))
                .collect();
            for item in generator.items() {
                if let Item::Edit {
                    path,
                    op: EditOp::Overwrite { offset, length },
                    ..
                } = item
                {
                    let size = *sizes.get(&path).expect("an edit names a file in the tree");
                    assert!(
                        offset + length <= size,
                        "{}: {offset}+{length} exceeds {size} in {path}",
                        scale.name()
                    );
                    assert!(length >= 1, "a zero-byte edit stores nothing");
                }
            }
        }
    }

    #[test]
    fn the_base_tree_totals_exactly_its_stated_size() {
        for scale in [Scale::Full, Scale::Reduced, Scale::Smoke] {
            let parameters = AgentWeekParameters::for_scale(scale);
            let tally = Tally::of(AgentWeek::new(CANONICAL_SEED, scale).items());
            assert_eq!(
                tally.logical_bytes,
                parameters.base_bytes(),
                "{}",
                scale.name()
            );
            assert_eq!(tally.files, parameters.file_count() as u64);
        }
    }

    #[test]
    fn every_actor_checkpoints_once_a_day() {
        let parameters = AgentWeekParameters::for_scale(Scale::Reduced);
        let generator = AgentWeek::new(CANONICAL_SEED, Scale::Reduced);
        let mut per_actor: std::collections::BTreeMap<String, u64> =
            std::collections::BTreeMap::new();
        for item in generator.items() {
            if let Item::Checkpoint(checkpoint) = item {
                *per_actor.entry(checkpoint.actor).or_default() += 1;
            }
        }
        assert_eq!(per_actor.len() as u64, parameters.actors);
        assert!(per_actor.values().all(|count| *count == DAYS));
    }

    #[test]
    fn every_edit_carries_its_own_content_stream() {
        let generator = AgentWeek::new(CANONICAL_SEED, Scale::Smoke);
        let streams: Vec<u64> = generator
            .items()
            .filter_map(|item| match item {
                Item::Edit { stream, .. } => Some(stream),
                _ => None,
            })
            .collect();
        let unique: BTreeSet<u64> = streams.iter().copied().collect();
        assert_eq!(
            unique.len(),
            streams.len(),
            "two edits write the same bytes"
        );
    }

    #[test]
    fn the_shape_report_holds_at_every_scale() {
        for scale in [Scale::Full, Scale::Reduced, Scale::Smoke] {
            let report = AgentWeek::new(CANONICAL_SEED, scale).shape();
            assert!(report.holds(), "{} {report:?}", scale.name());
        }
    }

    #[test]
    fn the_generator_is_reproducible() {
        let left = AgentWeek::new(CANONICAL_SEED, Scale::Smoke);
        let right = AgentWeek::new(CANONICAL_SEED, Scale::Smoke);
        assert_eq!(left.plan_digest(), right.plan_digest());
        assert_eq!(left.content_digest(), right.content_digest());
    }

    #[test]
    fn an_edit_never_shrinks_to_nothing_and_never_exceeds_the_file() {
        assert_eq!(edit_length(1_000, 10), 10);
        assert_eq!(edit_length(10, 10), 1, "a tenth of a percent of 10 bytes");
        assert_eq!(edit_length(0, 10), 1);
        assert_eq!(edit_length(100, 5_000), 100, "clamped to the whole file");
    }
}
