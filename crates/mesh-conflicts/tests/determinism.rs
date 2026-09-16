//! The determinism campaign: one operation set, one resolution, on every peer.
//!
//! # What is being measured
//!
//! Two peers can receive the same changes in any order, and they cannot exchange a message to
//! agree on what the workspace looks like — if they could, the conflict rules would not need to be
//! deterministic. So the property is: **[`resolve`] is a function of the operation *set*, not of
//! the sequence.**
//!
//! Each seed draws a set and then plays it to a hundred simulated peers in a hundred independently
//! shuffled orders, requiring one resolution from all of them — not merely one tree, but one
//! resolution: same outcomes, same dispositions, same reachable versions, same review items.
//!
//! # Why this is the test that catches the worst failure
//!
//! A resolution that differs between peers is worse than a conflict, because nothing surfaces. Two
//! people look at their own screens, each sees a workspace that reads as settled, and the two
//! workspaces disagree. Every rule here is a pure function of [`mesh_conflicts::Stamp`], which is
//! `lamport → event ULID → content hash`, precisely so that no such divergence has anywhere to
//! come from. This file is where "precisely so that" stops being an argument.
//!
//! # The numbers
//!
//! 32 seeds × 22 changes × 100 peers = 3,200 resolutions per assertion, each compared for equality
//! against the peer that resolved it first.

mod support;

use std::collections::BTreeSet;

use mesh_conflicts::{resolve, resolve_tree, Rule};

use support::{generate, workspace, Seeded};

/// How many operation sets to draw.
const SEEDS: u64 = 32;

/// How many changes in each.
const CHANGES: usize = 22;

/// How many peers receive each set, each in its own order.
const PEERS: usize = 100;

#[test]
fn one_operation_set_resolves_the_same_way_on_every_peer() {
    let space = workspace();
    for seed in 0..SEEDS {
        let mut seeded = Seeded::new(seed);
        let changes = generate(&mut seeded, &space, CHANGES);
        let first = resolve(&space.base, &changes);

        for peer in 0..PEERS {
            let shuffled = seeded.shuffled(&changes);
            assert_eq!(
                resolve(&space.base, &shuffled),
                first,
                "seed {seed}, peer {peer}: two peers holding one operation set disagree"
            );
        }
    }
}

#[test]
fn cycle_resolution_is_identical_across_peers() {
    let space = workspace();
    for seed in 0..SEEDS {
        let mut seeded = Seeded::new(seed);
        let changes = generate(&mut seeded, &space, CHANGES);
        let first = resolve_tree(&space.base, &changes);
        assert!(!first.has_cycle(), "seed {seed}: the resolved tree cycles");

        for peer in 0..PEERS {
            let shuffled = seeded.shuffled(&changes);
            let theirs = resolve_tree(&space.base, &shuffled);
            assert!(
                !theirs.has_cycle(),
                "seed {seed}, peer {peer}: the resolved tree cycles"
            );
            assert_eq!(
                theirs.refused_moves(),
                first.refused_moves(),
                "seed {seed}, peer {peer}: the peers refused different moves"
            );
            assert_eq!(
                theirs, first,
                "seed {seed}, peer {peer}: the peers built different trees"
            );
        }
    }
}

#[test]
fn every_path_in_every_resolved_tree_is_the_same_on_every_peer() {
    let space = workspace();
    for seed in 0..SEEDS {
        let mut seeded = Seeded::new(seed);
        let changes = generate(&mut seeded, &space, CHANGES);
        let first = resolve(&space.base, &changes);
        let paths: Vec<Option<String>> = first
            .tree()
            .placements()
            .map(|(object, _)| first.path_of(*object))
            .collect();

        for peer in 0..PEERS {
            let shuffled = seeded.shuffled(&changes);
            let theirs = resolve(&space.base, &shuffled);
            let their_paths: Vec<Option<String>> = theirs
                .tree()
                .placements()
                .map(|(object, _)| theirs.path_of(*object))
                .collect();
            assert_eq!(
                their_paths, paths,
                "seed {seed}, peer {peer}: the peers derive different paths"
            );
        }
    }
}

#[test]
fn the_review_surface_is_the_same_on_every_peer() {
    let space = workspace();
    for seed in 0..SEEDS {
        let mut seeded = Seeded::new(seed);
        let changes = generate(&mut seeded, &space, CHANGES);
        let first = resolve(&space.base, &changes);

        for peer in 0..PEERS {
            let shuffled = seeded.shuffled(&changes);
            let theirs = resolve(&space.base, &shuffled);
            assert_eq!(
                theirs.needs_review(),
                first.needs_review(),
                "seed {seed}, peer {peer}: one peer needs a review and the other does not"
            );
            assert_eq!(
                theirs.review_items(),
                first.review_items(),
                "seed {seed}, peer {peer}: the peers would show a person different conflicts"
            );
        }
    }
}

/// A campaign that never produced a conflict would pass every assertion above and prove nothing.
#[test]
fn the_shuffled_corpus_actually_contains_conflicts() {
    let space = workspace();
    let mut with_review = 0usize;
    let mut fired: BTreeSet<Rule> = BTreeSet::new();
    for seed in 0..SEEDS {
        let mut seeded = Seeded::new(seed);
        let changes = generate(&mut seeded, &space, CHANGES);
        let resolved = resolve(&space.base, &changes);
        if resolved.needs_review() {
            with_review += 1;
        }
        fired.extend(resolved.rules_applied());
    }
    assert!(
        with_review * 2 >= usize::try_from(SEEDS).expect("a seed count"),
        "only {with_review} of {SEEDS} generated sets produced a conflict; the corpus is too tame \
         to be evidence of anything"
    );
    assert!(
        fired.contains(&Rule::DeterministicCycleBreak),
        "no generated set produced a cyclic move, so the cycle campaign proves nothing"
    );
}
