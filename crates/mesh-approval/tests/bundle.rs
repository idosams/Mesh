//! The four acceptance criteria of task `01KZC2J2DTPRQKP4BX7VYEPPF1`, each as a test that fails if
//! its criterion stops holding.
//!
//! | Criterion | Tests |
//! |---|---|
//! | The exact diff is computed without any actor declaration of what changed | `the_bundle_reports_what_the_states_say_and_not_what_the_actor_says`, `an_actor_that_explains_nothing_still_has_everything_in_its_bundle` |
//! | The same inputs always produce a byte-identical bundle, verified by hash | `a_second_process_computes_the_same_bytes_and_the_same_identifier`, `every_field_of_the_bundle_moves_its_identifier`, `generated_states_produce_one_identifier_however_the_inputs_arrived` |
//! | Later actor work cannot enter an existing bundle, asserted by test | `work_appended_after_the_bundle_exists_leaves_it_byte_identical` |
//! | Conflicts and dependency impact are part of the bundle | `conflicts_and_dependency_impact_are_inside_the_bundle_identifier` |
//!
//! Plus the property the task's *Tests and benchmarks* section names — replaying the bundle onto
//! the base reproduces the actor state exactly, over generated states.
//!
//! # How the cross-process check works
//!
//! `cargo` builds this file into a standard libtest binary. The parent test re-executes *that
//! binary* — `std::env::current_exe()` — filtered to one `#[ignore]`d test, which prints its
//! process identifier and the bundle's whole canonical byte stream as hex. The parent asserts the
//! two process identifiers differ and the two byte streams are equal. Nothing is shared between
//! the runs but the source: a bundle that depended on process state, address-space layout,
//! hash-map seeding or a clock would diverge here.
//!
//! Comparing the *bytes* rather than only the digest is deliberate. "Byte-identical, verified by
//! hash" is the criterion; a digest comparison alone would pass for two implementations that both
//! hashed the same wrong thing.

mod support;

use std::collections::BTreeSet;
use std::process::Command;

use mesh_approval::{
    compute_bundle, BundleRefusal, BundleRequest, DependencyGraph, ReviewBundle, StaleOutput,
    ValidationResult, Verdict, WorkspaceState,
};

use support::{
    actor_id, canonical_bytes, generate, head, hex, mutate, name, object, root, text, Seeded,
};

/// The environment variable that tells the re-executed binary it is the child.
const CHILD_MARKER: &str = "MESH_APPROVAL_BUNDLE_CHILD";

/// The prefix the child prints its answer under.
const ANSWER: &str = "bundle-canonical-bytes=";

/// The prefix the child prints its process identifier under.
const CHILD_PID: &str = "bundle-child-pid=";

/// The name of the test the parent re-executes.
const CHILD_TEST: &str = "the_child_prints_the_bundle_it_computed";

// ---------------------------------------------------------------------------------------------
// The fixture. Every value is a literal: nothing is read from the environment, the filesystem or a
// clock, because a fixture that read any of those would make the cross-process comparison
// meaningless.
// ---------------------------------------------------------------------------------------------

/// The state the actor departed from.
fn fork() -> WorkspaceState {
    WorkspaceState::new(root())
        .with_directory(object(2), root(), name("archive"))
        .with_file(
            object(1),
            root(),
            name("notes.md"),
            text(1, &["one", "two"]),
        )
        .with_file(object(3), root(), name("README.md"), text(2, &["read me"]))
        .with_file(object(4), object(2), name("index.json"), text(3, &["{}"]))
}

/// Where the canonical head is now: somebody else rewrote the README.
fn canonical() -> WorkspaceState {
    fork().with_file(
        object(3),
        root(),
        name("README.md"),
        text(4, &["read me too"]),
    )
}

/// What the actor is offering: the notes moved, renamed and rewritten, the README rewritten again,
/// and the generated index removed.
fn actor() -> WorkspaceState {
    canonical()
        .with_file(
            object(1),
            object(2),
            name("journal.md"),
            text(5, &["ONE", "TWO", "THREE"]),
        )
        .with_file(
            object(3),
            root(),
            name("README.md"),
            text(6, &["read mine"]),
        )
        .without(object(4))
}

/// The index is derived from the notes; the README is derived from the index.
fn dependencies() -> DependencyGraph {
    DependencyGraph::new()
        .with_edge(object(4), object(1))
        .with_edge(object(3), object(4))
}

fn validations() -> Vec<ValidationResult> {
    vec![
        ValidationResult::new("schema", Some(object(1)), Verdict::Passed, ""),
        ValidationResult::new(
            "line-length",
            Some(object(3)),
            Verdict::Failed,
            "line 1 is long",
        ),
        ValidationResult::new("licence", None, Verdict::Skipped, "no licence rule"),
    ]
}

/// The whole fixture, with one part replaceable at a time by the callers below.
fn fixture() -> BundleRequest {
    request(fork(), canonical(), actor())
        .with_dependencies(dependencies())
        .with_validations(validations())
        .with_explanation("renamed the notes and refreshed the readme")
}

/// A request over three states, with the fixture's heads and author and nothing else attached.
fn request(
    fork: WorkspaceState,
    canonical: WorkspaceState,
    actor: WorkspaceState,
) -> BundleRequest {
    BundleRequest::new(fork, canonical, head(10), actor, head(11), actor_id(12))
}

fn fixture_bundle() -> ReviewBundle {
    compute_bundle(&fixture()).expect("the fixture is computable")
}

// ---------------------------------------------------------------------------------------------
// Criterion 1 — the exact diff is computed without any actor declaration of what changed.
// ---------------------------------------------------------------------------------------------

#[test]
fn the_bundle_reports_what_the_states_say_and_not_what_the_actor_says() {
    let honest = compute_bundle(&fixture().with_explanation("I renamed one file.")).unwrap();
    let lying = compute_bundle(&fixture().with_explanation("I changed nothing at all.")).unwrap();
    let silent = compute_bundle(&request(fork(), canonical(), actor())).unwrap();

    // Three different claims, one change list. The actor's words never reach it.
    assert_eq!(honest.changes(), lying.changes());
    assert_eq!(honest.changes(), silent.changes());

    // And the change list is not "one file": it is everything the two states differ by, including
    // the rewrite of a file the actor never mentioned.
    let labels: Vec<&str> = honest
        .changes()
        .iter()
        .map(|change| change.effect().label())
        .collect();
    assert_eq!(
        labels,
        vec![
            "moved",
            "renamed",
            "content-written",
            "content-written",
            "removed"
        ],
        "the diff must report the move, the rename, both rewrites and the removal"
    );
}

#[test]
fn an_actor_that_explains_nothing_still_has_everything_in_its_bundle() {
    let bundle = compute_bundle(&request(fork(), canonical(), actor())).unwrap();
    assert_eq!(
        bundle.touched_objects(),
        BTreeSet::from([object(1), object(3), object(4)]),
        "every object the two states differ by is in the bundle"
    );
    assert_eq!(bundle.explanation(), None);

    // The paths are derived from the states too, so a reviewer reads a location the actor did not
    // supply either.
    let moved = bundle
        .changes()
        .iter()
        .find(|change| change.effect().label() == "moved")
        .expect("the notes moved");
    assert_eq!(moved.path_before(), Some("/notes.md"));
    assert_eq!(moved.path_after(), Some("/archive/journal.md"));
}

// ---------------------------------------------------------------------------------------------
// Criterion 2 — the same inputs always produce a byte-identical bundle, verified by hash.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_second_process_computes_the_same_bytes_and_the_same_identifier() {
    let here = fixture_bundle();
    let mine = canonical_bytes(&here);

    let binary = std::env::current_exe().expect("a test binary knows its own path");
    let output = Command::new(&binary)
        .env(CHILD_MARKER, "1")
        .args(["--exact", "--nocapture", "--ignored", "--test-threads=1"])
        .arg(CHILD_TEST)
        .output()
        .unwrap_or_else(|error| panic!("cannot re-execute {} ({error})", binary.display()));

    let printed = String::from_utf8(output.stdout).expect("the child prints text");
    assert!(
        output.status.success(),
        "the child process failed:\n{printed}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let theirs = after_marker(&printed, ANSWER)
        .unwrap_or_else(|| panic!("the child did not print its answer; it printed:\n{printed}"));
    let child_pid: u32 = after_marker(&printed, CHILD_PID)
        .unwrap_or_else(|| {
            panic!("the child did not print its process identifier; it printed:\n{printed}")
        })
        .parse()
        .expect("a process identifier is a number");

    assert_ne!(
        child_pid,
        std::process::id(),
        "the comparison is only worth anything if the two runs really were two processes"
    );
    assert!(
        mine.len() > 256,
        "only {} bytes were compared; the recorder is not seeing the whole record",
        mine.len()
    );
    assert_eq!(
        hex(&mine),
        theirs,
        "two processes computed different bytes for the same three states, so a bundle identifier \
         does not name exact bytes and no approval can be checked against one"
    );
    assert_eq!(
        here.id().to_hex().len(),
        64,
        "a bundle identifier is 64 hex characters"
    );
}

/// What the child printed after `marker`, wherever on the line it landed.
///
/// libtest prefixes the first line a test prints with the test's own name, so the marker is looked
/// for anywhere in the line rather than at the start of one.
fn after_marker(printed: &str, marker: &str) -> Option<String> {
    printed
        .lines()
        .find_map(|line| {
            line.find(marker)
                .map(|at| line[at + marker.len()..].trim().to_owned())
        })
        .filter(|found| !found.is_empty())
}

/// The child half of the cross-process check. Ignored in a normal run; the parent runs it directly.
#[test]
#[ignore = "re-executed by a_second_process_computes_the_same_bytes_and_the_same_identifier"]
fn the_child_prints_the_bundle_it_computed() {
    assert!(
        std::env::var(CHILD_MARKER).is_ok(),
        "this test is only meaningful as a child process"
    );
    let bundle = fixture_bundle();
    println!("{CHILD_PID}{}", std::process::id());
    println!("{ANSWER}{}", hex(&canonical_bytes(&bundle)));
    println!("bundle-id={}", bundle.id());
}

#[test]
fn every_field_of_the_bundle_moves_its_identifier() {
    let baseline = fixture_bundle();

    // A fork in which the README was already different. Only the *conflicts* change: the diff the
    // bundle reports is still canonical → actor, which this leaves alone.
    let other_fork = WorkspaceState::new(root())
        .with_directory(object(2), root(), name("archive"))
        .with_file(
            object(1),
            root(),
            name("notes.md"),
            text(1, &["one", "two"]),
        )
        .with_file(object(3), root(), name("README.md"), text(9, &["older"]))
        .with_file(object(4), object(2), name("index.json"), text(3, &["{}"]));

    // Each entry mutates exactly one thing the bundle carries. If any of these leaves the
    // identifier where it was, that field is outside the identity and an approval does not cover
    // it.
    let mutations: Vec<(&str, BundleRequest)> = vec![
        (
            "the canonical head",
            BundleRequest::new(
                fork(),
                canonical(),
                head(99),
                actor(),
                head(11),
                actor_id(12),
            ),
        ),
        (
            "the actor head",
            BundleRequest::new(
                fork(),
                canonical(),
                head(10),
                actor(),
                head(99),
                actor_id(12),
            ),
        ),
        (
            "the author",
            BundleRequest::new(
                fork(),
                canonical(),
                head(10),
                actor(),
                head(11),
                actor_id(99),
            ),
        ),
        (
            "one line of one changed file",
            request(
                fork(),
                canonical(),
                actor().with_file(
                    object(1),
                    object(2),
                    name("journal.md"),
                    text(5, &["ONE", "TWO", "FOUR"]),
                ),
            ),
        ),
        (
            "the name one changed file hangs under",
            request(
                fork(),
                canonical(),
                actor().with_file(
                    object(1),
                    object(2),
                    name("diary.md"),
                    text(5, &["ONE", "TWO", "THREE"]),
                ),
            ),
        ),
        (
            "the version identifier of one changed file",
            request(
                fork(),
                canonical(),
                actor().with_file(
                    object(1),
                    object(2),
                    name("journal.md"),
                    text(55, &["ONE", "TWO", "THREE"]),
                ),
            ),
        ),
        (
            "the conflicts, by moving the fork under an object both sides touched",
            request(other_fork, canonical(), actor())
                .with_dependencies(dependencies())
                .with_validations(validations())
                .with_explanation("renamed the notes and refreshed the readme"),
        ),
        (
            "the dependency impact",
            fixture().with_dependencies(dependencies().with_edge(object(5), object(1))),
        ),
        (
            "one validation verdict",
            fixture().with_validations(vec![
                ValidationResult::new("schema", Some(object(1)), Verdict::Failed, ""),
                ValidationResult::new(
                    "line-length",
                    Some(object(3)),
                    Verdict::Failed,
                    "line 1 is long",
                ),
                ValidationResult::new("licence", None, Verdict::Skipped, "no licence rule"),
            ]),
        ),
        (
            "the explanation",
            fixture().with_explanation("something else entirely"),
        ),
    ];

    let mut seen = BTreeSet::from([baseline.id().to_hex()]);
    for (what, request) in mutations {
        let moved = compute_bundle(&request).unwrap_or_else(|refusal| {
            panic!("mutating {what} made the bundle uncomputable: {refusal}")
        });
        assert_ne!(
            moved.id(),
            baseline.id(),
            "changing {what} left the bundle identifier where it was, so an approval that names \
             that identifier does not name that field"
        );
        assert_ne!(
            canonical_bytes(&moved),
            canonical_bytes(&baseline),
            "changing {what} left the bundle's bytes unchanged"
        );
        assert!(
            seen.insert(moved.id().to_hex()),
            "two different bundles share an identifier after changing {what}"
        );
    }
}

#[test]
fn generated_states_produce_one_identifier_however_the_inputs_arrived() {
    let unordered = validations();
    let mut campaigns = 0;
    for seed in 0..64u64 {
        let mut generator = Seeded::new(seed);
        let base = generate(&mut generator, 4, 8);
        let Some(offered) = mutate(&mut generator, &base, 6) else {
            continue;
        };
        let shuffled = generator.shuffled(&unordered);

        let build = |results: Vec<ValidationResult>| {
            request(base.state.clone(), base.state.clone(), offered.clone())
                .with_validations(results)
        };
        let Ok(first) = compute_bundle(&build(unordered.clone())) else {
            continue;
        };
        let second = compute_bundle(&build(shuffled)).expect("the same states are computable");

        assert_eq!(
            canonical_bytes(&first),
            canonical_bytes(&second),
            "seed {seed}: the same set of validation results in a different order produced \
             different bytes"
        );
        assert_eq!(first.id(), second.id(), "seed {seed}");
        campaigns += 1;
    }
    assert!(
        campaigns >= 32,
        "only {campaigns} campaigns ran; the generator is producing states that never differ and \
         this test is asserting nothing"
    );
}

// ---------------------------------------------------------------------------------------------
// Criterion 3 — later actor work cannot enter an existing bundle.
// ---------------------------------------------------------------------------------------------

#[test]
fn work_appended_after_the_bundle_exists_leaves_it_byte_identical() {
    let bundle = fixture_bundle();
    let before_id = bundle.id();
    let before_bytes = canonical_bytes(&bundle);
    let before_changes = bundle.changes().to_vec();

    // The agent keeps working while a person reads: a new file, and another rewrite of a file that
    // is already in the bundle.
    let later = actor()
        .with_file(
            object(7),
            root(),
            name("afterwards.md"),
            text(77, &["later"]),
        )
        .with_file(
            object(1),
            object(2),
            name("journal.md"),
            text(78, &["rewritten", "after", "review", "started"]),
        );

    // The bundle that was read has not moved — not its identifier, not its bytes, not its list.
    assert_eq!(bundle.id(), before_id);
    assert_eq!(canonical_bytes(&bundle), before_bytes);
    assert_eq!(bundle.changes(), before_changes.as_slice());
    assert!(
        !bundle.touched_objects().contains(&object(7)),
        "an object created after the bundle existed appeared inside it"
    );

    // The later work is a *different* bundle, with a different name. Everything else about the
    // request is held constant, so the only reason the identifier moves is the later work.
    let after = compute_bundle(
        &request(fork(), canonical(), later)
            .with_dependencies(dependencies())
            .with_validations(validations())
            .with_explanation("renamed the notes and refreshed the readme"),
    )
    .expect("computable");

    assert_ne!(after.id(), before_id);
    assert!(after.touched_objects().contains(&object(7)));

    // And the bundle a person read still replays onto the base it was computed against, still
    // reproducing the checkpoint that was offered rather than the one that came later.
    assert_eq!(bundle.apply_to(&canonical()).unwrap(), actor());
    assert_ne!(
        bundle.actor_state(),
        after.actor_state(),
        "the two bundles name the same checkpoint, so the later work never happened"
    );
}

// ---------------------------------------------------------------------------------------------
// Criterion 4 — conflicts and dependency impact are part of the bundle.
// ---------------------------------------------------------------------------------------------

#[test]
fn conflicts_and_dependency_impact_are_inside_the_bundle_identifier() {
    let bundle = fixture_bundle();

    // The canonical head rewrote the README; so did the actor. That is one conflict, and it is in
    // the bundle rather than something a reviewing surface has to work out later.
    assert_eq!(bundle.conflicts().len(), 1);
    let conflict = &bundle.conflicts()[0];
    assert_eq!(conflict.object(), object(3));
    assert_eq!(conflict.path(), Some("/README.md"));
    assert_eq!(
        conflict.canonical_effects(),
        &["content-written".to_owned()]
    );
    assert_eq!(conflict.actor_effects(), &["content-written".to_owned()]);
    assert_eq!(
        conflict.preserved_versions().len(),
        3,
        "the fork's version and both later ones stay reachable"
    );

    // The actor changed the notes; the index is derived from the notes and the README from the
    // index. Both are stale, transitively, and both are in the bundle.
    let stale: Vec<_> = bundle
        .stale_outputs()
        .iter()
        .map(StaleOutput::output)
        .collect();
    assert_eq!(stale, vec![object(3), object(4)]);

    // Validation results are carried in canonical order, whatever order they arrived in.
    let validators: Vec<&str> = bundle
        .validations()
        .iter()
        .map(ValidationResult::validator)
        .collect();
    assert_eq!(validators, vec!["licence", "line-length", "schema"]);
}

// ---------------------------------------------------------------------------------------------
// The property the task names: the bundle applied to the base reproduces the actor state exactly.
// ---------------------------------------------------------------------------------------------

#[test]
fn replaying_a_generated_bundle_reproduces_the_actor_state() {
    let mut campaigns = 0;
    for seed in 0..128u64 {
        let mut generator = Seeded::new(seed);
        let base = generate(&mut generator, 4, 10);
        let Some(landed) = mutate(&mut generator, &base, 4) else {
            continue;
        };
        let Some(offered) = mutate(&mut generator, &base, 7) else {
            continue;
        };

        let bundle = match compute_bundle(&request(
            base.state.clone(),
            landed.clone(),
            offered.clone(),
        )) {
            Ok(bundle) => bundle,
            // The only refusal a well-formed generated pair can hit is "these two states are
            // equal", which is not a case this property is about.
            Err(BundleRefusal::NothingToReview) => continue,
            Err(refusal) => panic!("seed {seed}: {refusal}"),
        };

        assert_eq!(
            bundle.apply_to(&landed).expect("the bundle replays"),
            offered,
            "seed {seed}: replaying the bundle onto its base did not reproduce the actor state"
        );
        assert_eq!(bundle.actor_state(), offered.digest(), "seed {seed}");
        assert_eq!(bundle.canonical_state(), landed.digest(), "seed {seed}");
        campaigns += 1;
    }
    assert!(
        campaigns >= 64,
        "only {campaigns} campaigns ran; the generator is not producing reviewable pairs and this \
         property is asserting nothing"
    );
}

// ---------------------------------------------------------------------------------------------
// Refusal — a case that cannot be computed deterministically cannot be approved until it can.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_state_it_cannot_locate_is_refused_rather_than_approximated() {
    // A directory cycle: neither object has a path, so neither has a location a person can review.
    let looped = actor()
        .with_directory(object(20), object(21), name("first"))
        .with_directory(object(21), object(20), name("second"));
    let refusal = compute_bundle(&request(fork(), canonical(), looped))
        .expect_err("a directory cycle is refused");
    assert!(
        matches!(refusal, BundleRefusal::Diff(_)),
        "expected a refusal naming the difference, found {refusal:?}"
    );
    assert!(!refusal.to_string().is_empty());
}

#[test]
fn a_checkpoint_identical_to_the_canonical_head_is_refused() {
    assert_eq!(
        compute_bundle(&request(fork(), canonical(), canonical())),
        Err(BundleRefusal::NothingToReview)
    );
}
