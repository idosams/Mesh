//! The four acceptance criteria of task `01KZC2MWV2D761BBV4KW04WQYS`, each as a test that fails if
//! its criterion stops holding.
//!
//! | Criterion | Tests |
//! |---|---|
//! | A rename is presented as a rename, never as a delete plus a create | `a_rename_is_one_entry_and_never_a_removal_beside_a_creation`, `the_identity_corpus_never_turns_a_rename_into_a_delete_and_a_create`, `a_move_and_a_rename_together_stay_one_object` |
//! | Text diffs match a reference differ on the test corpus | `the_corpus_renders_exactly_what_a_unified_differ_renders`, `generated_texts_produce_a_minimal_edit_script`, `the_reference_oracle_agrees_with_brute_force` |
//! | Binary changes are presented with size and hash, never a garbled text diff | `a_binary_change_carries_its_size_and_hash_and_no_line`, `no_generated_binary_version_is_ever_rendered_as_a_line` |
//! | Diff output is stable: the same bundle always renders the same diff | `a_second_process_renders_the_same_bytes`, `the_order_the_changes_arrive_in_does_not_change_the_rendering`, `a_bundle_renders_the_same_bytes_every_time_it_is_asked` |
//!
//! Plus the task's failure-and-recovery clause — a file class that cannot be diffed meaningfully is
//! presented as an opaque change with its metadata — in
//! `a_text_above_the_ceiling_is_opaque_with_its_metadata` and
//! `a_version_that_changed_class_is_opaque_with_both_sides`.
//!
//! # What "matches a reference differ" is checked against, and what it is not
//!
//! Two things, neither of which is a live external tool:
//!
//! 1. **A fixed corpus with literal expected output.** [`CORPUS`] holds three pairs of texts and the
//!    exact unified rendering each produces. Those literals were taken from `diff -U3` over the same
//!    inputs and are compared byte for byte here. A corpus is small by nature; it is the part of
//!    this criterion that is checked against an outside tool at all.
//! 2. **Minimality, over generated inputs.** For every generated pair, the number of added plus
//!    removed lines is required to equal `before + after - 2 × LCS`, the size of a minimal
//!    line-level edit script, computed by an oracle written independently below and itself checked
//!    against exhaustive enumeration on small inputs.
//!
//! What is deliberately *not* claimed: that this differ's output is byte-identical to GNU `diff` on
//! every input. Two minimal edit scripts can differ in how they break a tie — which run of equal
//! lines an insertion attaches to — and no test here would catch that. What is claimed is that the
//! output is *a* minimal edit script, that it replays exactly onto the earlier version, and that it
//! is the same one every time.

mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

use mesh_approval::{
    diff, present, text_hunks, ChangeBody, Content, DiffLine, DiffPresentation, OpaqueReason,
    PresentedChange, TextHunk, WorkspaceState, CONTEXT_LINES, MAX_DIFF_LINES,
};

use support::{
    binary, generate, hex, mutate, name, object, presentation_bytes, root, text, version, Seeded,
};

/// The environment variable that tells the re-executed binary it is the child.
const CHILD_MARKER: &str = "MESH_APPROVAL_DIFF_CHILD";

/// The prefix the child prints its answer under.
const ANSWER: &str = "presentation-bytes=";

/// The prefix the child prints its process identifier under.
const CHILD_PID: &str = "presentation-child-pid=";

/// The name of the test the parent re-executes.
const CHILD_TEST: &str = "the_child_prints_the_diff_it_rendered";

// ---------------------------------------------------------------------------------------------
// The reference differ: an oracle written the other way round from the one under test.
// ---------------------------------------------------------------------------------------------

/// The length of the longest common subsequence, by a forward dynamic program.
///
/// Deliberately not the walk `text_hunks` performs: forward rather than backward, row by row, and
/// with no traceback at all. It answers only "how many lines survive", which is enough to say what
/// the smallest possible edit script costs, and it cannot be wrong in the same way the differ is.
fn longest_common_subsequence(before: &[String], after: &[String]) -> usize {
    let mut previous = vec![0usize; after.len() + 1];
    let mut current = vec![0usize; after.len() + 1];
    for earlier in before {
        for (index, later) in after.iter().enumerate() {
            current[index + 1] = if earlier == later {
                previous[index] + 1
            } else {
                previous[index + 1].max(current[index])
            };
        }
        std::mem::swap(&mut previous, &mut current);
        current.iter_mut().for_each(|cell| *cell = 0);
    }
    previous[after.len()]
}

/// The same answer by exhaustive enumeration, for inputs small enough to enumerate.
///
/// Every subset of the earlier lines, in order, tested for being a subsequence of the later ones.
/// Exponential and useless above a dozen lines, which is why it exists only to check the oracle.
fn longest_common_subsequence_by_enumeration(before: &[String], after: &[String]) -> usize {
    assert!(before.len() <= 12, "enumeration is exponential");
    let mut best = 0;
    for mask in 0u32..(1u32 << before.len()) {
        let candidate: Vec<&String> = before
            .iter()
            .enumerate()
            .filter(|(index, _)| mask & (1 << index) != 0)
            .map(|(_, line)| line)
            .collect();
        if candidate.len() <= best {
            continue;
        }
        let mut at = 0usize;
        for line in &candidate {
            match after[at..].iter().position(|later| later == *line) {
                Some(found) => at += found + 1,
                None => {
                    at = usize::MAX;
                    break;
                }
            }
        }
        if at != usize::MAX {
            best = candidate.len();
        }
    }
    best
}

/// The hunks rendered the way a unified differ renders them, headers included.
///
/// Only the body: the `---`/`+++` file header carries a path and a modification time, neither of
/// which this crate has or wants.
fn unified(hunks: &[TextHunk]) -> String {
    let mut out = String::new();
    for hunk in hunks {
        out.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            hunk.before_start(),
            hunk.before_len(),
            hunk.after_start(),
            hunk.after_len()
        ));
        for line in hunk.lines() {
            let marker = match line {
                DiffLine::Context { .. } => ' ',
                DiffLine::Removed { .. } => '-',
                DiffLine::Added { .. } => '+',
            };
            out.push(marker);
            out.push_str(line.text());
            out.push('\n');
        }
    }
    out
}

/// Replay hunks onto the earlier version. Hunks that cannot be replayed are a picture, not a diff.
fn replay(before: &[String], hunks: &[TextHunk]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cursor = 0usize;
    for hunk in hunks {
        for line in hunk.lines() {
            match line {
                DiffLine::Context { before: at, .. } => {
                    while cursor + 1 < *at {
                        out.push(before[cursor].clone());
                        cursor += 1;
                    }
                    out.push(before[cursor].clone());
                    cursor += 1;
                }
                DiffLine::Removed { .. } => cursor += 1,
                DiffLine::Added { text, .. } => out.push(text.clone()),
            }
        }
    }
    out.extend_from_slice(&before[cursor..]);
    out
}

fn lines(text: &[&str]) -> Vec<String> {
    text.iter().map(|line| (*line).to_owned()).collect()
}

/// Three pairs of texts and the exact unified body `diff -U3` produces for each.
///
/// The literals were taken from that tool over these inputs and are compared here byte for byte, so
/// this crate's rendering is held against something outside it without depending on it at run time.
type Case = (
    &'static [&'static str],
    &'static [&'static str],
    &'static str,
);

const CORPUS: [Case; 3] = [
    (
        &[
            "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta",
        ],
        &[
            "alpha", "beta", "GAMMA", "delta", "epsilon", "zeta", "eta", "theta",
        ],
        "@@ -1,6 +1,6 @@\n alpha\n beta\n-gamma\n+GAMMA\n delta\n epsilon\n zeta\n",
    ),
    (
        &[
            "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
            "eleven", "twelve",
        ],
        &[
            "one", "TWO", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
            "ELEVEN", "twelve",
        ],
        "@@ -1,5 +1,5 @@\n one\n-two\n+TWO\n three\n four\n five\n@@ -8,5 +8,5 @@\n eight\n \
         nine\n ten\n-eleven\n+ELEVEN\n twelve\n",
    ),
    (
        &["a", "b", "c"],
        &["a", "b", "b2", "c"],
        "@@ -1,3 +1,4 @@\n a\n b\n+b2\n c\n",
    ),
];

// ---------------------------------------------------------------------------------------------
// Criterion: text diffs match a reference differ on the test corpus.
// ---------------------------------------------------------------------------------------------

#[test]
fn the_corpus_renders_exactly_what_a_unified_differ_renders() {
    for (index, (before, after, expected)) in CORPUS.iter().enumerate() {
        let earlier = lines(before);
        let later = lines(after);
        let hunks = text_hunks(&earlier, &later).expect("a corpus case is under the ceiling");
        assert_eq!(
            unified(&hunks),
            *expected,
            "corpus case {index} renders differently from the reference differ"
        );
        assert_eq!(replay(&earlier, &hunks), later);
    }
}

#[test]
fn the_reference_oracle_agrees_with_brute_force() {
    let mut seed = Seeded::new(0x5EED_0051);
    for _ in 0..200 {
        let before = random_lines(&mut seed, 8, 4);
        let after = random_lines(&mut seed, 8, 4);
        assert_eq!(
            longest_common_subsequence(&before, &after),
            longest_common_subsequence_by_enumeration(&before, &after),
            "the oracle disagrees with exhaustive enumeration on {before:?} → {after:?}"
        );
    }
}

#[test]
fn generated_texts_produce_a_minimal_edit_script() {
    let mut seed = Seeded::new(0x1010_0051);
    let mut compared = 0;
    for _ in 0..400 {
        let before = random_lines(&mut seed, 24, 6);
        let after = random_lines(&mut seed, 24, 6);
        let hunks = text_hunks(&before, &after).expect("a generated text is under the ceiling");

        assert_eq!(
            replay(&before, &hunks),
            after,
            "the hunks do not replay onto the earlier version: {before:?} → {after:?}"
        );

        let changed: usize = hunks.iter().map(|hunk| hunk.removed() + hunk.added()).sum();
        let minimal = before.len() + after.len() - 2 * longest_common_subsequence(&before, &after);
        assert_eq!(
            changed, minimal,
            "the edit script is not minimal for {before:?} → {after:?}"
        );
        compared += 1;
    }
    assert_eq!(compared, 400, "the campaign did not run");
}

/// A vector of at most `most` lines drawn from an alphabet of `alphabet` distinct lines.
///
/// A small alphabet is deliberate: it produces repeated lines, which is where a differ's tie-breaks
/// live and where a wrong one stops being minimal.
fn random_lines(seed: &mut Seeded, most: usize, alphabet: usize) -> Vec<String> {
    let count = seed.below(most + 1);
    (0..count)
        .map(|_| format!("line{}", seed.below(alphabet)))
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Criterion: a rename is presented as a rename, never as a delete plus a create.
// ---------------------------------------------------------------------------------------------

fn base_state() -> WorkspaceState {
    WorkspaceState::new(root())
        .with_directory(object(2), root(), name("archive"))
        .with_file(
            object(1),
            root(),
            name("notes.md"),
            text(7, &["one", "two", "three"]),
        )
        .with_file(object(3), root(), name("cover.png"), binary(9, 4096))
}

fn rendered(before: &WorkspaceState, after: &WorkspaceState) -> DiffPresentation {
    present(&diff(before, after).expect("the fixture states are two versions of one workspace"))
}

#[test]
fn a_rename_is_one_entry_and_never_a_removal_beside_a_creation() {
    let after = base_state().with_file(
        object(1),
        root(),
        name("journal.md"),
        text(7, &["one", "two", "three"]),
    );
    let view = rendered(&base_state(), &after);

    assert_eq!(view.len(), 1);
    let entry = &view.entries()[0];
    assert!(entry.is_rename());
    assert_eq!(entry.object(), object(1));
    assert_eq!(entry.path_before(), Some("/notes.md"));
    assert_eq!(entry.path_after(), Some("/journal.md"));
    assert!(effects(&view).is_disjoint(&BTreeSet::from(["created", "removed"])));
}

#[test]
fn a_move_and_a_rename_together_stay_one_object() {
    let after = base_state().with_file(
        object(1),
        object(2),
        name("journal.md"),
        text(7, &["one", "two", "three"]),
    );
    let view = rendered(&base_state(), &after);
    let objects: BTreeSet<_> = view.entries().iter().map(PresentedChange::object).collect();
    assert_eq!(objects, BTreeSet::from([object(1)]));
    assert_eq!(
        view.entries()
            .iter()
            .map(PresentedChange::effect)
            .collect::<Vec<_>>(),
        vec!["moved", "renamed"]
    );
    assert_eq!(view.entries()[1].path_after(), Some("/archive/journal.md"));
    // The version never moved, so neither entry claims a content change.
    assert!(view
        .entries()
        .iter()
        .all(|entry| entry.body() == &ChangeBody::Placement));
}

#[test]
fn the_identity_corpus_never_turns_a_rename_into_a_delete_and_a_create() {
    let mut campaigns = 0;
    for raw in 0..64u64 {
        let mut seed = Seeded::new(0x00C0_FFEE_0000 + raw);
        let generated = generate(&mut seed, 4, 9);
        let Some(later) = mutate(&mut seed, &generated, 6) else {
            continue;
        };
        let Ok(changes) = diff(&generated.state, &later) else {
            continue;
        };
        let view = present(&changes);

        // What the two states actually say about identity, computed here and not read from the
        // presentation: which objects are new, which are gone, which kept their identity.
        let earlier_objects: BTreeSet<_> = generated
            .state
            .objects()
            .map(|(object, _)| *object)
            .collect();
        let later_objects: BTreeSet<_> = later.objects().map(|(object, _)| *object).collect();

        let created: BTreeSet<_> = view
            .entries()
            .iter()
            .filter(|entry| entry.effect() == "created")
            .map(PresentedChange::object)
            .collect();
        let removed: BTreeSet<_> = view
            .entries()
            .iter()
            .filter(|entry| entry.effect() == "removed")
            .map(PresentedChange::object)
            .collect();

        assert_eq!(
            created,
            later_objects
                .difference(&earlier_objects)
                .copied()
                .collect::<BTreeSet<_>>(),
            "seed {raw}: something was presented as created that already existed"
        );
        assert_eq!(
            removed,
            earlier_objects
                .difference(&later_objects)
                .copied()
                .collect::<BTreeSet<_>>(),
            "seed {raw}: something was presented as removed that still exists"
        );
        assert!(
            created.is_disjoint(&removed),
            "seed {raw}: one object was presented as both created and removed"
        );

        // Every object that survived under a different name is a rename, exactly once.
        for object_id in earlier_objects.intersection(&later_objects) {
            let was = generated
                .state
                .object(*object_id)
                .and_then(|held| held.name());
            let now = later.object(*object_id).and_then(|held| held.name());
            if was == now {
                continue;
            }
            let renames = view
                .entries()
                .iter()
                .filter(|entry| entry.object() == *object_id && entry.is_rename())
                .count();
            assert_eq!(
                renames, 1,
                "seed {raw}: object {object_id} changed name and produced {renames} rename entries"
            );
        }
        campaigns += 1;
    }
    assert!(
        campaigns >= 32,
        "only {campaigns} corpora produced a diff; the generator is not exercising the property"
    );
}

/// Every effect word the rendering uses.
fn effects(view: &DiffPresentation) -> BTreeSet<&'static str> {
    view.entries().iter().map(PresentedChange::effect).collect()
}

// ---------------------------------------------------------------------------------------------
// Criterion: binary changes carry size and hash, never a garbled text diff.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_binary_change_carries_its_size_and_hash_and_no_line() {
    let after = base_state().with_file(object(3), root(), name("cover.png"), binary(11, 8192));
    let view = rendered(&base_state(), &after);
    assert_eq!(view.len(), 1);
    let entry = &view.entries()[0];

    assert_eq!(entry.body(), &ChangeBody::Binary);
    assert!(entry.hunks().is_empty());
    assert_eq!(entry.before().unwrap().byte_length(), Some(4096));
    assert_eq!(entry.after().unwrap().byte_length(), Some(8192));
    assert!(entry.before().unwrap().digest().is_some());
    assert_ne!(
        entry.before().unwrap().digest(),
        entry.after().unwrap().digest(),
        "two different binary versions must not report the same hash"
    );
    assert_eq!(entry.after().unwrap().line_count(), None);
}

#[test]
fn no_generated_binary_version_is_ever_rendered_as_a_line() {
    let mut seen_binary = 0;
    for raw in 0..64u64 {
        let mut seed = Seeded::new(0x00B1_0000 + raw);
        let generated = generate(&mut seed, 3, 10);
        let Some(later) = mutate(&mut seed, &generated, 5) else {
            continue;
        };
        let Ok(changes) = diff(&generated.state, &later) else {
            continue;
        };
        for entry in present(&changes).entries() {
            let touches_binary = entry.before().is_some_and(|summary| summary.is_binary())
                || entry.after().is_some_and(|summary| summary.is_binary());
            if !touches_binary {
                continue;
            }
            seen_binary += 1;
            assert!(
                entry.hunks().is_empty(),
                "seed {raw}: a binary version was rendered as {} lines",
                entry.hunks().len()
            );
            assert!(
                matches!(
                    entry.body(),
                    ChangeBody::Binary | ChangeBody::Opaque { .. } | ChangeBody::Placement
                ),
                "seed {raw}: a binary version was given a {} body",
                entry.body().label()
            );
        }
    }
    assert!(
        seen_binary > 0,
        "no generated corpus contained a binary version; the property was not exercised"
    );
}

// ---------------------------------------------------------------------------------------------
// The failure-and-recovery clause: refuse to invent a representation.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_text_above_the_ceiling_is_opaque_with_its_metadata() {
    let long: Vec<String> = (0..=MAX_DIFF_LINES).map(|at| at.to_string()).collect();
    let after = base_state().with_file(
        object(1),
        root(),
        name("notes.md"),
        Content::Text {
            version: version(12),
            lines: long,
        },
    );
    let view = rendered(&base_state(), &after);
    let entry = &view.entries()[0];
    assert_eq!(
        entry.body(),
        &ChangeBody::Opaque {
            reason: OpaqueReason::AboveLineCeiling
        }
    );
    assert!(entry.hunks().is_empty());
    assert_eq!(entry.before().unwrap().line_count(), Some(3));
    assert_eq!(
        entry.after().unwrap().line_count(),
        Some(MAX_DIFF_LINES as u64 + 1)
    );
    assert_eq!(entry.after().unwrap().version(), version(12));
}

#[test]
fn a_version_that_changed_class_is_opaque_with_both_sides() {
    let after = base_state().with_file(object(1), root(), name("notes.md"), binary(13, 77));
    let view = rendered(&base_state(), &after);
    let entry = &view.entries()[0];
    assert_eq!(
        entry.body(),
        &ChangeBody::Opaque {
            reason: OpaqueReason::ContentClassChanged
        }
    );
    assert!(entry.hunks().is_empty());
    assert_eq!(entry.before().unwrap().line_count(), Some(3));
    assert_eq!(entry.after().unwrap().byte_length(), Some(77));
    assert_eq!(
        OpaqueReason::ContentClassChanged.label(),
        "content-class-changed"
    );
}

// ---------------------------------------------------------------------------------------------
// Criterion: the same bundle always renders the same diff.
// ---------------------------------------------------------------------------------------------

/// The fixture the cross-process check renders. Every value is a literal: a fixture that read the
/// environment, the filesystem or a clock would make the comparison meaningless.
fn fixture() -> DiffPresentation {
    let after = base_state()
        .with_file(
            object(1),
            object(2),
            name("journal.md"),
            text(8, &["one", "TWO", "three", "four"]),
        )
        .with_file(object(3), root(), name("cover.png"), binary(11, 8192))
        .with_file(object(4), object(2), name("new.md"), text(9, &["fresh"]));
    rendered(&base_state(), &after)
}

#[test]
fn a_second_process_renders_the_same_bytes() {
    let mine = presentation_bytes(&fixture());

    let binary_path = std::env::current_exe().expect("a test binary knows its own path");
    let output = Command::new(&binary_path)
        .env(CHILD_MARKER, "1")
        .args(["--exact", "--nocapture", "--ignored", "--test-threads=1"])
        .arg(CHILD_TEST)
        .output()
        .unwrap_or_else(|error| panic!("cannot re-execute {} ({error})", binary_path.display()));

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
        "only {} bytes were compared; the recorder is not seeing the whole rendering",
        mine.len()
    );
    assert_eq!(
        hex(&mine),
        theirs,
        "two processes rendered different bytes for the same change list, so 'the human approved \
         exactly this diff' is not a checkable statement"
    );
}

/// The child half of the cross-process check. Ignored in a normal run; the parent runs it directly.
#[test]
#[ignore = "re-executed by a_second_process_renders_the_same_bytes"]
fn the_child_prints_the_diff_it_rendered() {
    assert!(
        std::env::var(CHILD_MARKER).is_ok(),
        "this test is only meaningful as a child process"
    );
    let view = fixture();
    println!("{CHILD_PID}{}", std::process::id());
    println!("{ANSWER}{}", hex(&presentation_bytes(&view)));
    println!("presentation-digest={}", view.digest());
}

/// What the child printed after `marker`, wherever on the line it landed.
fn after_marker(printed: &str, marker: &str) -> Option<String> {
    printed
        .lines()
        .find_map(|line| {
            line.find(marker)
                .map(|at| line[at + marker.len()..].trim().to_owned())
        })
        .filter(|found| !found.is_empty())
}

#[test]
fn the_order_the_changes_arrive_in_does_not_change_the_rendering() {
    let mut campaigns = 0;
    for raw in 0..48u64 {
        let mut seed = Seeded::new(0x0ADE_0BEDu64.wrapping_add(raw));
        let generated = generate(&mut seed, 4, 8);
        let Some(later) = mutate(&mut seed, &generated, 5) else {
            continue;
        };
        let Ok(changes) = diff(&generated.state, &later) else {
            continue;
        };
        let shuffled = seed.shuffled(&changes);
        assert_eq!(
            presentation_bytes(&present(&changes)),
            presentation_bytes(&present(&shuffled)),
            "seed {raw}: the order the changes arrived in changed the rendered bytes"
        );
        campaigns += 1;
    }
    assert!(campaigns >= 24, "only {campaigns} corpora were compared");
}

#[test]
fn a_bundle_renders_the_same_bytes_every_time_it_is_asked() {
    let view = fixture();
    let again = fixture();
    assert_eq!(presentation_bytes(&view), presentation_bytes(&again));
    assert_eq!(view.digest(), again.digest());
    assert_eq!(view.digest().to_hex().len(), 64);
}

#[test]
fn one_changed_context_line_moves_the_rendering_digest() {
    let baseline = fixture();
    let altered = {
        let after = base_state()
            .with_file(
                object(1),
                object(2),
                name("journal.md"),
                text(8, &["one", "TWO", "three", "FOUR"]),
            )
            .with_file(object(3), root(), name("cover.png"), binary(11, 8192))
            .with_file(object(4), object(2), name("new.md"), text(9, &["fresh"]));
        rendered(&base_state(), &after)
    };
    assert_ne!(baseline.digest(), altered.digest());
}

#[test]
fn every_entry_names_an_object_the_change_list_names() {
    let view = fixture();
    let counts: BTreeMap<_, usize> = view.entries().iter().fold(BTreeMap::new(), |mut acc, e| {
        *acc.entry(e.object()).or_default() += 1;
        acc
    });
    assert!(counts.contains_key(&object(1)));
    assert!(counts.contains_key(&object(3)));
    assert!(counts.contains_key(&object(4)));
    assert_eq!(view.renames().len(), 1);
    assert_eq!(
        view.entries()
            .iter()
            .filter(|entry| entry.body().label() == "text")
            .count(),
        2,
        "the notes rewrite and the new file are the two text bodies"
    );
    assert_eq!(CONTEXT_LINES, 3);
}
