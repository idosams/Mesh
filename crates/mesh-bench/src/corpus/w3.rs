//! W3 — agent swarm.
//!
//! ```text
//! 100 actors
//! 10 files changed per actor
//! 20% overlap probability
//! Frequent checkpoints
//! ```
//!
//! The workload two structural-win claims rest on: a hundred actors
//! checkpointing without a global lock, and concurrent private states that
//! genuinely conflict often enough to be worth reconciling.
//!
//! # How the overlap probability is realised
//!
//! Naively, "20% overlap" is a coin flip per change against the set of files
//! other actors have already touched. That has a defect that only shows up in
//! the numbers: the first actor has nobody to overlap with, so its ten changes
//! are all private, and the measured rate comes out under the stated one by
//! roughly `1 / actors`. The bias is small at a hundred actors and glaring at
//! four, which is exactly the scale a test runs at.
//!
//! So the pool comes first. A **shared pool** of paths is generated up front;
//! each change draws from it with the stated probability and from the actor's
//! own private pool otherwise. Every actor faces the same distribution from its
//! first change, and `overlap_ratio` measures what it says it measures.
//!
//! It stays a genuine Bernoulli draw rather than an exact quota, because the
//! plan says *probability*. The consequence is that the observed rate lands near
//! the stated one rather than on it, so the shape fact carries a band derived
//! from the number of draws — three sigma of a binomial, via
//! [`binomial_tolerance`](super::shape::binomial_tolerance) — instead of a
//! percentage somebody picked.

use super::plan::{ChangeSpec, CheckpointSpec, ContentKind, FileSpec, Item};
use super::rng::{stream, SplitMix64};
use super::shape::{binomial_tolerance, ShapeFact, ShapeReport};
use super::{Generator, Scale, WorkloadId};
use crate::json::{Json, JsonObject};
use std::collections::{BTreeMap, BTreeSet};

/// Bytes an actor rewrites in one change, drawn from this range.
const CHANGE_BYTES: (u64, u64) = (256, 32_768);

/// Size range of the files in either pool.
const FILE_BYTES: (u64, u64) = (1_024, 65_536);

/// W3's stated shape at one scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgentSwarmParameters {
    /// How many actors work concurrently.
    pub actors: usize,
    /// How many files each actor changes.
    pub changes_per_actor: usize,
    /// The probability a change lands on a shared path, in parts per thousand.
    pub overlap_permille: u32,
    /// How many paths every actor can reach.
    pub shared_pool_files: usize,
    /// How many private paths each actor has.
    pub private_pool_files: usize,
    /// How many changes an actor makes between checkpoints.
    pub changes_per_checkpoint: usize,
}

impl AgentSwarmParameters {
    /// The parameters plan §12.2 states, and the two reductions of them.
    ///
    /// `shared_pool_files` is a quarter of the expected shared draws, which is
    /// what makes the pool small enough for those draws to actually collide —
    /// a "shared" pool with one path per draw produces no overlap at all, and
    /// the parameter is stated here rather than hidden so a reader can see the
    /// contention it buys.
    #[must_use]
    pub const fn for_scale(scale: Scale) -> Self {
        match scale {
            // Plan §12.2: 100 actors, 10 files each, 20% overlap.
            Scale::Full => AgentSwarmParameters {
                actors: 100,
                changes_per_actor: 10,
                overlap_permille: 200,
                shared_pool_files: 50,
                private_pool_files: 20,
                changes_per_checkpoint: 2,
            },
            Scale::Reduced => AgentSwarmParameters {
                actors: 20,
                changes_per_actor: 10,
                overlap_permille: 200,
                shared_pool_files: 10,
                private_pool_files: 20,
                changes_per_checkpoint: 2,
            },
            Scale::Smoke => AgentSwarmParameters {
                actors: 4,
                changes_per_actor: 5,
                overlap_permille: 200,
                shared_pool_files: 3,
                private_pool_files: 10,
                changes_per_checkpoint: 2,
            },
        }
    }

    /// Total changes across every actor.
    #[must_use]
    pub const fn total_changes(self) -> u64 {
        (self.actors * self.changes_per_actor) as u64
    }

    /// Checkpoints one actor takes.
    #[must_use]
    pub const fn checkpoints_per_actor(self) -> usize {
        self.changes_per_actor.div_ceil(self.changes_per_checkpoint)
    }

    /// The parameter object recorded in descriptors and result rows.
    #[must_use]
    pub fn to_json(self) -> JsonObject {
        JsonObject::new()
            .with("actors", Json::Uint(self.actors as u64))
            .with(
                "changes_per_actor",
                Json::Uint(self.changes_per_actor as u64),
            )
            .with(
                "overlap_permille",
                Json::Uint(u64::from(self.overlap_permille)),
            )
            .with(
                "shared_pool_files",
                Json::Uint(self.shared_pool_files as u64),
            )
            .with(
                "private_pool_files",
                Json::Uint(self.private_pool_files as u64),
            )
            .with(
                "changes_per_checkpoint",
                Json::Uint(self.changes_per_checkpoint as u64),
            )
    }
}

/// The name of actor `index`, zero-padded so lexical order is numeric order.
#[must_use]
pub fn actor_name(index: usize) -> String {
    format!("actor-{index:04}")
}

/// The W3 generator.
#[derive(Clone, Debug)]
pub struct AgentSwarm {
    seed: u64,
    scale: Scale,
    parameters: AgentSwarmParameters,
}

impl AgentSwarm {
    /// Builds the generator for a seed and a scale.
    #[must_use]
    pub const fn new(seed: u64, scale: Scale) -> Self {
        AgentSwarm {
            seed,
            scale,
            parameters: AgentSwarmParameters::for_scale(scale),
        }
    }

    fn shared_file(&self, index: usize) -> FileSpec {
        let mut source = SplitMix64::derived(self.seed, "w3/shared-size", index as u64);
        FileSpec {
            path: format!("swarm/shared/s{index:05}.rs"),
            bytes: source.in_range(FILE_BYTES.0, FILE_BYTES.1),
            kind: ContentKind::Text,
            stream: stream(self.seed, "w3/shared-content", index as u64),
        }
    }

    fn private_file(&self, actor: usize, index: usize) -> FileSpec {
        let ordinal = (actor * self.parameters.private_pool_files + index) as u64;
        let mut source = SplitMix64::derived(self.seed, "w3/private-size", ordinal);
        FileSpec {
            path: format!("swarm/{}/p{index:05}.rs", actor_name(actor)),
            bytes: source.in_range(FILE_BYTES.0, FILE_BYTES.1),
            kind: ContentKind::Text,
            stream: stream(self.seed, "w3/private-content", ordinal),
        }
    }

    /// The corpus: the shared pool, then every actor's private pool.
    fn files(&self) -> impl Iterator<Item = Item> + '_ {
        let shared = (0..self.parameters.shared_pool_files)
            .map(move |index| Item::File(self.shared_file(index)));
        let private = (0..self.parameters.actors).flat_map(move |actor| {
            (0..self.parameters.private_pool_files)
                .map(move |index| Item::File(self.private_file(actor, index)))
        });
        shared.chain(private)
    }

    /// One actor's changes, with a checkpoint after every run of them.
    fn activity_of(&self, actor: usize) -> Vec<Item> {
        let parameters = self.parameters;
        let mut source = SplitMix64::derived(self.seed, "w3/change", actor as u64);
        let mut items = Vec::with_capacity(parameters.changes_per_actor * 2);
        let mut since_checkpoint = 0_u64;
        for _ in 0..parameters.changes_per_actor {
            let shared = source.chance(parameters.overlap_permille);
            let file = if shared {
                let index = source.in_range(0, parameters.shared_pool_files as u64 - 1);
                self.shared_file(index as usize)
            } else {
                let index = source.in_range(0, parameters.private_pool_files as u64 - 1);
                self.private_file(actor, index as usize)
            };
            items.push(Item::Change(ChangeSpec {
                actor: actor_name(actor),
                path: file.path,
                shared,
                bytes: source.in_range(CHANGE_BYTES.0, CHANGE_BYTES.1),
            }));
            since_checkpoint += 1;
            if since_checkpoint as usize == parameters.changes_per_checkpoint {
                items.push(Item::Checkpoint(CheckpointSpec {
                    actor: actor_name(actor),
                    covers_changes: since_checkpoint,
                }));
                since_checkpoint = 0;
            }
        }
        if since_checkpoint > 0 {
            items.push(Item::Checkpoint(CheckpointSpec {
                actor: actor_name(actor),
                covers_changes: since_checkpoint,
            }));
        }
        items
    }
}

impl Generator for AgentSwarm {
    fn id(&self) -> WorkloadId {
        WorkloadId::W3
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
        let activity =
            (0..self.parameters.actors).flat_map(move |actor| self.activity_of(actor).into_iter());
        Box::new(self.files().chain(activity))
    }

    fn shape(&self) -> ShapeReport {
        let parameters = self.parameters;
        let mut per_actor: BTreeMap<String, u64> = BTreeMap::new();
        let mut checkpoints: BTreeMap<String, u64> = BTreeMap::new();
        let mut actors_touching: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut shared_changes = 0_u64;
        let mut total_changes = 0_u64;
        let mut files = 0_u64;

        for item in self.items() {
            match item {
                Item::File(_) => files += 1,
                Item::Change(change) => {
                    total_changes += 1;
                    if change.shared {
                        shared_changes += 1;
                    }
                    *per_actor.entry(change.actor.clone()).or_default() += 1;
                    actors_touching
                        .entry(change.path)
                        .or_default()
                        .insert(change.actor);
                }
                Item::Checkpoint(checkpoint) => {
                    *checkpoints.entry(checkpoint.actor).or_default() += 1;
                }
                _ => {}
            }
        }

        let stated_overlap = f64::from(parameters.overlap_permille) / 1000.0;
        let observed_overlap = if total_changes == 0 {
            0.0
        } else {
            shared_changes as f64 / total_changes as f64
        };
        let contended = actors_touching
            .values()
            .filter(|actors| actors.len() > 1)
            .count() as u64;

        ShapeReport::new()
            .with(ShapeFact::count(
                "actor_count",
                parameters.actors as u64,
                per_actor.len() as u64,
            ))
            .with(ShapeFact::count(
                "changes_per_actor",
                parameters.changes_per_actor as u64,
                per_actor.values().copied().min().unwrap_or(0),
            ))
            .with(ShapeFact::count(
                "changes_per_actor_max",
                parameters.changes_per_actor as u64,
                per_actor.values().copied().max().unwrap_or(0),
            ))
            .with(ShapeFact::count(
                "total_changes",
                parameters.total_changes(),
                total_changes,
            ))
            .with(ShapeFact::ratio(
                "overlap_ratio",
                stated_overlap,
                observed_overlap,
                binomial_tolerance(stated_overlap, total_changes),
            ))
            .with(ShapeFact::count(
                "checkpoints_per_actor",
                parameters.checkpoints_per_actor() as u64,
                checkpoints.values().copied().min().unwrap_or(0),
            ))
            .with(ShapeFact::count(
                "corpus_files",
                (parameters.shared_pool_files + parameters.actors * parameters.private_pool_files)
                    as u64,
                files,
            ))
            .with(ShapeFact::present("has_contended_paths", contended > 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::{Tally, CANONICAL_SEED};

    #[test]
    fn the_full_scale_is_the_plan_figure() {
        let parameters = AgentSwarmParameters::for_scale(Scale::Full);
        assert_eq!(parameters.actors, 100, "plan 12.2 W3: 100 actors");
        assert_eq!(
            parameters.changes_per_actor, 10,
            "plan 12.2 W3: 10 files changed per actor"
        );
        assert_eq!(
            parameters.overlap_permille, 200,
            "plan 12.2 W3: 20% overlap probability"
        );
    }

    #[test]
    fn every_actor_makes_exactly_its_stated_number_of_changes() {
        let generator = AgentSwarm::new(CANONICAL_SEED, Scale::Full);
        let mut per_actor: BTreeMap<String, u64> = BTreeMap::new();
        for item in generator.items() {
            if let Item::Change(change) = item {
                *per_actor.entry(change.actor).or_default() += 1;
            }
        }
        assert_eq!(per_actor.len(), 100);
        assert!(
            per_actor.values().all(|count| *count == 10),
            "{per_actor:?}"
        );
    }

    #[test]
    fn the_overlap_rate_lands_inside_its_stated_band() {
        let generator = AgentSwarm::new(CANONICAL_SEED, Scale::Full);
        let report = generator.shape();
        let overlap = report.fact("overlap_ratio").expect("the fact exists");
        assert!(
            overlap.holds(),
            "observed {} against stated {} +/- {}",
            overlap.observed,
            overlap.stated,
            overlap.tolerance
        );
        assert!(overlap.observed > 0.0, "no change ever hit the shared pool");
    }

    #[test]
    fn the_overlap_rate_is_unbiased_across_seeds() {
        // The defect the shared pool exists to avoid: with the naive "overlap
        // with whoever went before" scheme the mean lands below the stated
        // rate. Twenty seeds is enough to see a systematic 1/actors bias at
        // this scale, and not enough to be flaky about noise.
        let rates: Vec<f64> = (0..20)
            .map(|seed| {
                AgentSwarm::new(seed, Scale::Reduced)
                    .shape()
                    .fact("overlap_ratio")
                    .expect("the fact exists")
                    .observed
            })
            .collect();
        let mean = rates.iter().sum::<f64>() / rates.len() as f64;
        assert!(
            (mean - 0.2).abs() < 0.03,
            "mean overlap {mean} over 20 seeds"
        );
    }

    #[test]
    fn actors_actually_collide_on_shared_paths() {
        let generator = AgentSwarm::new(CANONICAL_SEED, Scale::Full);
        let mut actors_touching: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for item in generator.items() {
            if let Item::Change(change) = item {
                actors_touching
                    .entry(change.path)
                    .or_default()
                    .insert(change.actor);
            }
        }
        let contended = actors_touching
            .values()
            .filter(|actors| actors.len() > 1)
            .count();
        assert!(contended > 0, "a swarm with no contention is not a swarm");
    }

    #[test]
    fn a_private_path_is_never_touched_by_two_actors() {
        let generator = AgentSwarm::new(CANONICAL_SEED, Scale::Reduced);
        for item in generator.items() {
            if let Item::Change(change) = item {
                if !change.shared {
                    assert!(
                        change.path.starts_with(&format!("swarm/{}/", change.actor)),
                        "{} is not private to {}",
                        change.path,
                        change.actor
                    );
                }
            }
        }
    }

    #[test]
    fn every_changed_path_exists_in_the_corpus() {
        let generator = AgentSwarm::new(CANONICAL_SEED, Scale::Reduced);
        let files: BTreeSet<String> = generator
            .items()
            .filter_map(|item| item.as_file().map(|file| file.path.clone()))
            .collect();
        for item in generator.items() {
            if let Item::Change(change) = item {
                assert!(files.contains(&change.path), "{} is missing", change.path);
            }
        }
    }

    #[test]
    fn checkpoints_are_frequent_and_cover_every_change() {
        let generator = AgentSwarm::new(CANONICAL_SEED, Scale::Full);
        let tally = Tally::of(generator.items());
        assert_eq!(tally.checkpoints, 100 * 5, "10 changes, one every 2");
        let covered: u64 = generator
            .items()
            .filter_map(|item| match item {
                Item::Checkpoint(checkpoint) => Some(checkpoint.covers_changes),
                _ => None,
            })
            .sum();
        assert_eq!(covered, tally.changes);
    }

    #[test]
    fn an_odd_change_count_still_checkpoints_its_remainder() {
        // Smoke scale is 5 changes with a checkpoint every 2: the trailing
        // change must not be left uncovered.
        let generator = AgentSwarm::new(CANONICAL_SEED, Scale::Smoke);
        let tally = Tally::of(generator.items());
        let covered: u64 = generator
            .items()
            .filter_map(|item| match item {
                Item::Checkpoint(checkpoint) => Some(checkpoint.covers_changes),
                _ => None,
            })
            .sum();
        assert_eq!(covered, tally.changes);
        assert_eq!(tally.checkpoints, 4 * 3);
    }

    #[test]
    fn the_shape_report_holds_at_every_scale() {
        for scale in [Scale::Full, Scale::Reduced, Scale::Smoke] {
            let report = AgentSwarm::new(CANONICAL_SEED, scale).shape();
            assert!(report.holds(), "{} {report:?}", scale.name());
        }
    }

    #[test]
    fn actor_names_sort_numerically() {
        let mut names: Vec<String> = (0..12).map(actor_name).collect();
        let ordered = names.clone();
        names.sort();
        assert_eq!(names, ordered);
    }

    #[test]
    fn the_generator_is_reproducible() {
        let left = AgentSwarm::new(CANONICAL_SEED, Scale::Smoke);
        let right = AgentSwarm::new(CANONICAL_SEED, Scale::Smoke);
        assert_eq!(left.plan_digest(), right.plan_digest());
        assert_eq!(left.content_digest(), right.content_digest());
    }
}
