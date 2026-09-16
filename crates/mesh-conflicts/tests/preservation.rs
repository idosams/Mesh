//! The preservation campaign: no generated interleaving loses a durable version.
//!
//! # What is being measured
//!
//! The task contract's first acceptance criterion is *"No conflict resolution deletes a durable
//! version; every version stays reachable."* That is a universally quantified statement, and the
//! eleven targeted tests in `conflicts.rs` are eleven instances of it. This file is the campaign
//! that goes after the quantifier.
//!
//! Each seed builds a set of concurrent changes over a six-object workspace — creates, renames,
//! moves, text writes, byte writes and removals, drawn from three Lamport counters so most of any
//! generated set is genuinely concurrent. For every set, and for a hundred interleavings of every
//! set, four things are required:
//!
//! 1. `reachable_versions` is **exactly** the base versions plus every version any change wrote.
//!    Not a superset — equality, so a resolution cannot pass by inventing versions either.
//! 2. Interleaving changes nothing: shuffling the set produces an equal resolution.
//! 3. The resolved tree is acyclic and every placed object has a derivable path.
//! 4. Where a resolution reports a current version for an object, that version is one somebody
//!    actually wrote. A resolution may decline to pick; it may not make one up.
//!
//! # The numbers
//!
//! 256 seeds × 18 changes each, with 8 of the seeds additionally shuffled 100 ways. Plan §2.10
//! requires a number behind a reliability claim, and these are the numbers behind this one. The
//! whole file runs in well under a second, so it stays in the default suite rather than behind a
//! feature.
//!
//! # What it does not establish
//!
//! The generator draws from a fixed six-object workspace and a fixed effect vocabulary. It cannot
//! find a rule that is wrong for a shape it never generates — deep directory nesting, files above
//! [`mesh_conflicts::MAX_MERGE_LINES`], or a base that already holds a cycle. Those are covered,
//! where they are covered, by the targeted tests.

mod support;

use std::collections::BTreeSet;

use mesh_conflicts::{resolve, Change, Disposition, VersionId};

use support::{generate, workspace, Seeded, Workspace};

/// How many operation sets to draw.
const SEEDS: u64 = 256;

/// How many changes in each.
const CHANGES: usize = 18;

/// How many interleavings of a set to check.
const INTERLEAVINGS: usize = 100;

/// Every version that went into a resolution: the base's, plus every version any change wrote.
fn versions_in(space: &Workspace, changes: &[Change]) -> BTreeSet<VersionId> {
    let mut all: BTreeSet<VersionId> = space.base.versions().into_iter().collect();
    for change in changes {
        if let Some(version) = change.effect().version() {
            all.insert(version);
        }
    }
    all
}

#[test]
fn no_generated_operation_set_loses_a_durable_version() {
    let space = workspace();
    for seed in 0..SEEDS {
        let mut seeded = Seeded::new(seed);
        let changes = generate(&mut seeded, &space, CHANGES);
        let expected = versions_in(&space, &changes);
        let resolved = resolve(&space.base, &changes);

        assert_eq!(
            resolved.reachable_versions(),
            &expected,
            "seed {seed}: the versions that went in are not the versions that came out"
        );
    }
}

#[test]
fn no_interleaving_of_a_generated_set_loses_a_durable_version() {
    let space = workspace();
    for seed in 0..8u64 {
        let mut seeded = Seeded::new(seed);
        let changes = generate(&mut seeded, &space, CHANGES);
        let expected = versions_in(&space, &changes);

        for interleaving in 0..INTERLEAVINGS {
            let shuffled = seeded.shuffled(&changes);
            let resolved = resolve(&space.base, &shuffled);
            assert_eq!(
                resolved.reachable_versions(),
                &expected,
                "seed {seed}, interleaving {interleaving}: a version was lost by reordering"
            );
        }
    }
}

#[test]
fn every_preserved_outcome_names_versions_that_are_reachable() {
    let space = workspace();
    for seed in 0..SEEDS {
        let mut seeded = Seeded::new(seed);
        let changes = generate(&mut seeded, &space, CHANGES);
        let resolved = resolve(&space.base, &changes);

        for outcome in resolved.outcomes() {
            for version in outcome.disposition().versions() {
                assert!(
                    resolved.reachable_versions().contains(&version),
                    "seed {seed}: outcome {:?} names version {version} that is not reachable",
                    outcome.rule()
                );
            }
        }
    }
}

#[test]
fn a_resolution_never_reports_a_version_nobody_wrote() {
    let space = workspace();
    for seed in 0..SEEDS {
        let mut seeded = Seeded::new(seed);
        let changes = generate(&mut seeded, &space, CHANGES);
        let expected = versions_in(&space, &changes);
        let resolved = resolve(&space.base, &changes);

        for (object, _) in resolved.tree().placements() {
            if let Some(version) = resolved.version_of(*object) {
                assert!(
                    expected.contains(&version),
                    "seed {seed}: object {object} reports version {version}, which nobody wrote"
                );
            }
        }
    }
}

#[test]
fn a_resolution_that_picks_a_version_had_only_one_to_pick() {
    let space = workspace();
    for seed in 0..SEEDS {
        let mut seeded = Seeded::new(seed);
        let changes = generate(&mut seeded, &space, CHANGES);
        let resolved = resolve(&space.base, &changes);

        for outcome in resolved.outcomes() {
            let Disposition::Applied {
                version: Some(picked),
            } = outcome.disposition()
            else {
                continue;
            };
            let written: BTreeSet<VersionId> = changes
                .iter()
                .filter(|change| change.object() == outcome.object())
                .filter_map(|change| change.effect().version())
                .collect();
            assert!(
                written.len() <= 1,
                "seed {seed}: object {} settled on {picked} while {} concurrent versions existed",
                outcome.object(),
                written.len()
            );
        }
    }
}

#[test]
fn every_generated_resolution_is_a_tree() {
    let space = workspace();
    for seed in 0..SEEDS {
        let mut seeded = Seeded::new(seed);
        let changes = generate(&mut seeded, &space, CHANGES);
        let resolved = resolve(&space.base, &changes);

        assert!(
            !resolved.tree().has_cycle(),
            "seed {seed}: the resolved tree contains a cycle"
        );
        for (object, _) in resolved.tree().placements() {
            assert!(
                resolved.path_of(*object).is_some(),
                "seed {seed}: object {object} is placed but has no path"
            );
        }
    }
}

#[test]
fn a_tombstoned_object_still_has_its_versions_reachable() {
    let space = workspace();
    for seed in 0..SEEDS {
        let mut seeded = Seeded::new(seed);
        let changes = generate(&mut seeded, &space, CHANGES);
        let resolved = resolve(&space.base, &changes);

        for object in resolved.tombstoned() {
            if let Some(version) = space.base.version_of(*object) {
                assert!(
                    resolved.reachable_versions().contains(&version),
                    "seed {seed}: removing {object} dropped its base version {version}"
                );
            }
            for version in changes
                .iter()
                .filter(|change| change.object() == *object)
                .filter_map(|change| change.effect().version())
            {
                assert!(
                    resolved.reachable_versions().contains(&version),
                    "seed {seed}: removing {object} dropped concurrently written {version}"
                );
            }
        }
    }
}

/// The campaign is only evidence if it reaches the rules it claims to cover.
///
/// A generator that never produced a conflict would pass every assertion above and prove nothing.
/// This asserts the generated corpus fires the object-graph rows — the nine rows [`resolve`]
/// decides; rows ten and eleven have their own entry points and their own tests.
#[test]
fn the_generated_corpus_reaches_the_object_graph_rows() {
    let space = workspace();
    let mut seen = BTreeSet::new();
    for seed in 0..SEEDS {
        let mut seeded = Seeded::new(seed);
        let changes = generate(&mut seeded, &space, CHANGES);
        seen.extend(resolve(&space.base, &changes).rules_applied());
    }
    for rule in [
        mesh_conflicts::Rule::EditFollowsIdentity,
        mesh_conflicts::Rule::ChildStaysAttached,
        mesh_conflicts::Rule::IndependentFilesMerge,
        mesh_conflicts::Rule::ThreeWayTextMerge,
        mesh_conflicts::Rule::PreserveOverlappingText,
        mesh_conflicts::Rule::PreserveBinaryVersions,
        mesh_conflicts::Rule::PreserveTombstoneAndEdit,
        mesh_conflicts::Rule::RetainBothIdentities,
        mesh_conflicts::Rule::DeterministicCycleBreak,
    ] {
        assert!(
            seen.contains(&rule),
            "the generated corpus never fired {rule}, so the campaign says nothing about it"
        );
    }
}
