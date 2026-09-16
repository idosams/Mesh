//! This crate's steps 1 to 4 against the crate that owns steps 5 to 11.
//!
//! # What is at stake
//!
//! Plan §6.3 is one sequence with eleven steps. It is implemented by two crates that **cannot refer
//! to each other**: neither may declare a dependency, because any dependency edge rewrites
//! `Cargo.lock`, which is on this repository's governance list
//! (`docs/adr/0008-ship-the-sqlite-schema-without-a-sqlite-driver.md`). So the seam between them is
//! a trait `mesh-store` declares and a caller wires up, and the numbering they share is held
//! together by nothing the compiler can see.
//!
//! That is the failure this file is for. If `mesh-store` renumbered its steps, or this crate
//! stopped covering one of the plan's first four, every crash claim about "before step 4" and
//! "after step 4" would start referring to a different boundary in each crate — silently, and in
//! the one part of the product where a silent divergence means a user is told their work is safe
//! when it is not.
//!
//! # How it fails
//!
//! It reads `mesh-store`'s source by path. Moving or renaming that file makes this test fail
//! loudly — the arity assertions turn "found nothing to compare" into a failure — rather than
//! silently passing over a comparison that no longer happens. It is a lint over source text, with
//! that technique's limits: it recognises one `match` arm shape and would miss a mapping written
//! some other way.

use std::fs;
use std::path::PathBuf;

use mesh_cas::PromotionStep;

/// The steps plan §6.3 gives to the content-addressed store.
const CAS_PLAN_STEPS: [u8; 4] = [1, 2, 3, 4];

fn mesh_store_sequence_source() -> String {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "mesh-store",
        "src",
        "sequence.rs",
    ]
    .iter()
    .collect();
    fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read {} ({error}). mesh-cas holds its steps 1 to 4 against mesh-store's \
             eleven-step sequence by reading its source, because no dependency edge is permitted; \
             if that file has moved, the two crates' numbering is unchecked until this path is \
             corrected.",
            path.display()
        )
    })
}

/// The `Self::Name => <number>,` arms of `SequenceStep::plan_step`, as `(name, number)` pairs.
fn mesh_store_plan_steps(source: &str) -> Vec<(String, u8)> {
    let start = source
        .find("pub const fn plan_step(self) -> u8 {")
        .expect("mesh-store's SequenceStep declares plan_step");
    let body = &source[start..];
    let end = body.find("\n    }").expect("plan_step has a body");
    body[..end]
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix("Self::")?;
            let (name, number) = rest.split_once(" => ")?;
            let number: u8 = number.trim_end_matches(',').parse().ok()?;
            Some((name.trim().to_owned(), number))
        })
        .collect()
}

/// The eleven steps are still eleven, and still numbered one to eleven.
#[test]
fn mesh_store_still_numbers_plan_6_3_as_one_to_eleven() {
    let mapped = mesh_store_plan_steps(&mesh_store_sequence_source());
    assert_eq!(
        mapped.len(),
        11,
        "mesh-store's SequenceStep maps {} steps, not eleven: {mapped:?}",
        mapped.len()
    );
    let numbers: Vec<u8> = mapped.iter().map(|(_, number)| *number).collect();
    assert_eq!(numbers, (1..=11).collect::<Vec<u8>>());
}

/// Every step plan §6.3 gives to the content-addressed store is performed by some promotion step
/// here, and no promotion step claims a number that belongs to the index.
#[test]
fn this_crate_covers_exactly_plan_6_3s_first_four_steps() {
    let mut covered: Vec<u8> = PromotionStep::ORDER
        .into_iter()
        .filter_map(PromotionStep::plan_step)
        .collect();
    covered.sort_unstable();
    covered.dedup();
    assert_eq!(
        covered,
        CAS_PLAN_STEPS.to_vec(),
        "this crate's promotion no longer covers exactly plan §6.3 steps 1 to 4"
    );
}

/// The boundary the whole crash contract turns on: the last step here is plan step 4, and the first
/// step `mesh-store` owns is plan step 5. Nothing is numbered twice and nothing is skipped.
#[test]
fn the_two_crates_meet_at_the_boundary_between_step_four_and_step_five() {
    let last_here = PromotionStep::ORDER
        .into_iter()
        .filter_map(PromotionStep::plan_step)
        .max()
        .expect("this crate performs some numbered step");
    assert_eq!(last_here, 4);

    let mapped = mesh_store_plan_steps(&mesh_store_sequence_source());
    let first_there = mapped
        .iter()
        .map(|(_, number)| *number)
        .filter(|number| *number > 4)
        .min()
        .expect("mesh-store owns some step past four");
    assert_eq!(
        first_there,
        last_here + 1,
        "the two halves of plan §6.3 no longer meet: this crate ends at {last_here} and \
         mesh-store's first step past four is {first_there}"
    );

    // And mesh-store owns none of what this crate does, which is the other direction of the same
    // claim: a step numbered 1 to 4 there would mean the chunk work happens twice or in the wrong
    // place.
    let overlapping: Vec<&(String, u8)> =
        mapped.iter().filter(|(_, number)| *number <= 4).collect();
    assert_eq!(
        overlapping.len(),
        4,
        "mesh-store maps {} steps at or below four; those are the content-addressed store's and \
         are reached through the ChunkPromoter seam: {overlapping:?}",
        overlapping.len()
    );
}

/// The unnumbered step is unnumbered on purpose, and this is what stops it being quietly given a
/// number that shifts every boundary after it.
#[test]
fn recording_an_arrival_belongs_to_no_plan_step() {
    assert_eq!(PromotionStep::RecordArrival.plan_step(), None);
    assert_eq!(
        PromotionStep::ORDER
            .into_iter()
            .filter(|step| step.plan_step().is_none())
            .count(),
        1
    );
}
