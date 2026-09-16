//! The conformance suite, run — and the evidence that it can fail.
//!
//! # What decides whether this file is worth anything
//!
//! Three claims, in descending order of how easy they are to fake:
//!
//! 1. **The reference backend passes.** Easy, and worth almost nothing on its own.
//! 2. **Every one of the sixteen planted defects fails the *one* case that names its rule** —
//!    `failing_ids()` is compared for equality against a single identifier, not searched. A suite
//!    that failed everything would satisfy "some case caught it" and would fail this.
//!    Design `01KZEZGDPMZ5RH7E60WDYDYYEE` Contract 11 is the list; `reference.rs` is the plant.
//!    This is the only reason to believe any green run below.
//! 3. **The published material and the Rust agree in both directions**, which is what makes
//!    `tests/compatibility/adapter/v0/vocabulary.json` the contract rather than a description of
//!    one. That is the `VOC` family, and it closes a gap `src/adapter.rs` names and cannot close:
//!    a capability *appended* to the enumeration after `Symlink` and never added to
//!    `AdapterCapability::ALL` compiles clean under every guard in that file.
//!
//! # Why this file and not `src/`
//!
//! `crates/mesh-materializer/src/` may not name a filesystem (`tests/no_ambient_io.rs`), and the
//! `VOC` family has to read a file. The `CAT` family's on-disk half is here for the same reason:
//! `run_conformance` can check that a citation *is published material*, and only something that
//! can open a file can check that the file exists.
//!
//! Plan §14.3 rule 4: the run that wrote `src/adapter.rs` is not the run that wrote this. It is
//! the same repository and the same session, so verification here is **warm**, not cold
//! (`docs/adr/0004`).

// A subdirectory rather than a sibling `tests/*.rs`, which cargo would turn into a second test
// target that runs nothing. There is no `main.rs` under it, so nothing is auto-discovered.
#[path = "adapter-conformance/reference.rs"]
mod reference;

// The folder-watching fallback of plan §7.4 — task `01KZC2QR9VVJK6Y60PS8D360JT` — used to be a
// module here, because a `WorkspaceAdapter` implementation has to name this crate and no crate in
// the workspace declared the edge. It now lives in `crates/mesh-daemon/src/folder_watch/`, which
// is a crate a person can start, and `crates/mesh-daemon/tests/folder-watch.rs` grades it with
// `run_conformance` — the same oracle, unchanged, called from the other side of the seam. That
// file is also where the two checks a suite cannot make of itself now live: what a backend finds
// by re-reading a folder, and whether the restrictions it works under are the ones the product
// tells a person about.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use mesh_materializer::{
    conformance_catalogue, run_conformance, AdapterCapability, CaseFamily, CaseResult,
    ConformanceReport, WorkspaceAdapter,
};

use reference::{
    DeclaresNothing, HoldsItsOwnWorkspace, Mutant, ReferenceAdapter, RefusesPresentationPaths,
    RootedPaths, WillNotPrepare,
};

// ---------------------------------------------------------------------------------------------
// Where the published material lives
// ---------------------------------------------------------------------------------------------

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the workspace root is two directories above this crate")
        .to_path_buf()
}

fn read(relative: &str) -> String {
    let path = repository_root().join(relative);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("cannot read {path:?}: {error}"))
}

fn vocabulary() -> String {
    read("tests/compatibility/adapter/v0/vocabulary.json")
}

fn adapter_source() -> String {
    read("crates/mesh-materializer/src/adapter.rs")
}

/// Every source file the suite is made of, concatenated.
///
/// The lint below reads this, and it reads the whole suite rather than one file of it: a rule
/// that covered `src/conformance.rs` alone would be satisfied by moving a forbidden name into
/// `src/conformance/cases.rs`, which is where the case bodies actually live.
fn suite_source() -> String {
    let root = repository_root().join("crates/mesh-materializer/src");
    let mut files = vec![root.join("conformance.rs")];
    let children = fs::read_dir(root.join("conformance")).expect("the suite's own directory");
    let mut nested: Vec<PathBuf> = children
        .map(|entry| entry.expect("a readable entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .collect();
    nested.sort();
    assert!(
        !nested.is_empty(),
        "the suite has no case bodies, so this lint is reading the wrong place"
    );
    files.extend(nested);
    files
        .iter()
        .map(|path| fs::read_to_string(path).expect("a readable source file"))
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------------------------
// 1. The suite runs, unchanged, against every adapter (AC-1, AC-4)
// ---------------------------------------------------------------------------------------------

#[test]
fn the_reference_backend_is_conformant() {
    let report = run_conformance(&ReferenceAdapter::new());
    assert!(report.is_conformant(), "{report}");
    let (passed, failed, _) = report.tally();
    assert_eq!(failed, 0);
    assert!(
        passed >= 40,
        "only {passed} cases actually ran against a backend that implements everything; a suite \
         that grades almost nothing would also report zero failures\n{report}"
    );
}

/// The published contract promises that declaring nothing and refusing cleanly is a *passing*
/// answer. A partial backend reporting honestly is not a broken one, and `unsupported` is neither
/// a soft failure nor a pass.
#[test]
fn a_backend_that_declares_nothing_is_conformant() {
    let report = run_conformance(&DeclaresNothing);
    assert!(report.is_conformant(), "{report}");
    let (_, failed, unsupported) = report.tally();
    assert_eq!(failed, 0);
    assert!(
        unsupported > 20,
        "a backend that declares nothing should be graded `unsupported` almost everywhere\n{report}"
    );
}

/// AC-1 and AC-4 together: three backends, one entry point, no file in the suite naming any of
/// them. The suite takes `&dyn WorkspaceAdapter`, so it *cannot* name one.
#[test]
fn one_entry_point_grades_three_different_backends() {
    let backends: [&dyn WorkspaceAdapter; 3] = [
        &ReferenceAdapter::new(),
        &DeclaresNothing,
        &ReferenceAdapter::mutated(Mutant::WriteSilentlyTruncated),
    ];
    let names: Vec<&str> = backends
        .iter()
        .map(|backend| run_conformance(*backend).adapter())
        .collect();
    assert_eq!(names.len(), 3);
    assert_eq!(
        names.iter().collect::<BTreeSet<_>>().len(),
        3,
        "three backends graded and the reports do not tell them apart"
    );

    let suite = suite_source();
    for concrete in [
        "ReferenceAdapter",
        "DeclaresNothing",
        "HoldsItsOwnWorkspace",
        "WillNotPrepare",
        "Mutant",
        "mesh-fuse",
        "mesh-fskit",
    ] {
        assert!(
            !suite.contains(concrete),
            "the suite names {concrete}, so adding a backend would change the suite"
        );
    }
}

/// Contract 10's determinism check. Two fresh backends of the same shape, two runs, two identical
/// rendered reports — which is only possible if nothing in the suite reads a clock, an address or
/// an iteration order that varies.
#[test]
fn the_report_is_deterministic() {
    let first = run_conformance(&ReferenceAdapter::new()).to_string();
    let second = run_conformance(&ReferenceAdapter::new()).to_string();
    assert_eq!(first, second);

    for mutant in Mutant::ALL {
        let one = run_conformance(&ReferenceAdapter::mutated(mutant)).to_string();
        let other = run_conformance(&ReferenceAdapter::mutated(mutant)).to_string();
        assert_eq!(
            one,
            other,
            "{} renders two different reports",
            mutant.as_str()
        );
    }
}

// ---------------------------------------------------------------------------------------------
// 2. The sixteen mutants — the only reason to believe anything above
// ---------------------------------------------------------------------------------------------

#[test]
fn every_mutant_is_caught_by_the_case_that_names_its_rule() {
    for mutant in Mutant::ALL {
        let report = run_conformance(&ReferenceAdapter::mutated(mutant));
        assert!(
            !report.is_conformant(),
            "the planted defect {} was not caught at all\n{report}",
            mutant.as_str()
        );
        assert_eq!(
            report.failing_ids(),
            vec![mutant.caught_by()],
            "the planted defect {} should fail exactly {} and nothing else\n{report}",
            mutant.as_str(),
            mutant.caught_by()
        );
    }
}

/// Sixteen distinct defects and sixteen distinct cases. If two mutants named one case, one of
/// them would be evidence for a case nothing else covers and the count would flatter the suite.
#[test]
fn the_sixteen_mutants_name_sixteen_different_cases() {
    let caught: BTreeSet<&str> = Mutant::ALL.iter().map(|m| m.caught_by()).collect();
    assert_eq!(caught.len(), 16);
    let names: BTreeSet<&str> = Mutant::ALL.iter().map(|m| m.as_str()).collect();
    assert_eq!(names.len(), 16);

    let published: BTreeSet<&str> = conformance_catalogue()
        .into_iter()
        .map(|rule| rule.id())
        .collect();
    for identifier in caught {
        assert!(
            published.contains(identifier),
            "a mutant names {identifier}, which is not a case in the catalogue"
        );
    }
}

/// The decision the whole design turns on, checked directly rather than inferred from a mutant:
/// a capability the backend did not declare is **called**, and the call is graded.
#[test]
fn an_undeclared_capability_is_probed_rather_than_skipped() {
    let report = run_conformance(&ReferenceAdapter::mutated(
        Mutant::UndeclaredCapabilityReturnsOk,
    ));
    let write = report
        .cases()
        .iter()
        .find(|case| {
            case.id() == "CAP/undeclared-but-answered"
                && case.subject() == Some(AdapterCapability::Write)
        })
        .expect("the undeclared Write capability is graded");
    assert_eq!(write.result(), CaseResult::Fail);
    assert!(
        write.detail().contains("data loss"),
        "the failure should say what it costs, not only that it happened: {}",
        write.detail()
    );

    // And the honest partial backend is not punished for the same shape.
    let honest = run_conformance(&ReferenceAdapter::new());
    assert!(honest
        .cases()
        .iter()
        .any(|case| case.subject() == Some(AdapterCapability::Symlink)));
}

// ---------------------------------------------------------------------------------------------
// 2b. MNT and RO against a backend that refuses what it was not given (01KZFXFR4KHDN8EJ8R0750E0T6)
// ---------------------------------------------------------------------------------------------

/// The five cases that were graded `unsupported` — or failed for the wrong reason — against every
/// backend that does not accept an arbitrary identifier.
const MOUNT_AND_READONLY_CASES: [&str; 5] = [
    "MNT/two-actor-views-coexist",
    "MNT/released-view-is-unknown",
    "RO/access-is-read-only",
    "RO/write-is-refused",
    "RO/every-mutating-operation-is-refused",
];

fn case_result(report: &ConformanceReport, id: &str) -> CaseResult {
    report
        .cases()
        .iter()
        .find(|case| case.id() == id)
        .unwrap_or_else(|| panic!("{id} is not in the report"))
        .result()
}

/// The backend is really strict, checked before anything is concluded from its green report.
///
/// Without this, `the_five_mount_and_readonly_cases_grade_against_a_strict_backend` would be
/// satisfied by a second lenient backend, which is the backend the suite already had.
#[test]
fn the_strict_backend_refuses_an_identifier_it_was_never_given() {
    let backend = HoldsItsOwnWorkspace::new();
    let elsewhere = mesh_materializer::WorkspaceId::from_bytes([0x77; 16]);
    let stranger = mesh_materializer::HeadId::from_bytes([0x78; 32]);
    let actor = mesh_materializer::ActorId::from_bytes([0x01; 32]);

    // Before anything is prepared it holds nothing, so even its own identifiers are unknown.
    assert_eq!(
        backend.mount_actor_view(elsewhere, actor, Path::new("/mesh/nowhere")),
        Err(mesh_materializer::AdapterError::NotFound)
    );

    let fixture = backend.prepare_fixture().expect("this backend prepares");
    assert_eq!(
        backend.mount_actor_view(elsewhere, actor, Path::new("/mesh/nowhere")),
        Err(mesh_materializer::AdapterError::NotFound),
        "a workspace this backend does not hold was accepted"
    );
    assert_eq!(
        backend.materialize_readonly_view(stranger, Path::new("/mesh/nowhere")),
        Err(mesh_materializer::AdapterError::NotFound),
        "a head this backend does not hold was accepted"
    );
    assert!(
        backend
            .mount_actor_view(fixture.workspace(), actor, Path::new("/mesh/somewhere"))
            .is_ok(),
        "the workspace it prepared is the one it accepts"
    );
    assert_ne!(fixture.workspace(), elsewhere);
}

/// AC-1 of `01KZFXFR4KHDN8EJ8R0750E0T6`: `pass` or `fail`, never `unsupported`, against a backend
/// that refuses an unknown workspace or head — which is what every real backend does.
///
/// This is the whole point of `WorkspaceAdapter::prepare_fixture`. If the suite mounted a constant
/// of its own, or asked for a fixture after mounting rather than before, every mount here would be
/// `NotFound` and these five would collapse.
#[test]
fn the_five_mount_and_readonly_cases_grade_against_a_strict_backend() {
    let report = run_conformance(&HoldsItsOwnWorkspace::new());
    assert!(report.is_conformant(), "{report}");
    for id in MOUNT_AND_READONLY_CASES {
        assert_eq!(
            case_result(&report, id),
            CaseResult::Pass,
            "{id} did not grade against a backend that prepares its own workspace\n{report}"
        );
    }

    // And the rest of the suite reaches this backend too: a strict backend that could not be
    // mounted would have no view, and every operation family would be `unsupported`.
    let (passed, failed, _) = report.tally();
    assert_eq!(failed, 0);
    assert!(
        passed >= 40,
        "only {passed} cases ran against a strict backend that implements everything\n{report}"
    );
    let lenient = run_conformance(&ReferenceAdapter::new()).tally();
    assert_eq!(
        report.tally(),
        lenient,
        "a backend that holds its own workspace is graded differently from one that accepts any"
    );
}

/// AC-3: every planted defect still fails **exactly** the one case that names its rule, on the
/// backend that does the preparing. Zero collateral, same as on the lenient one.
#[test]
fn every_mutant_is_caught_by_its_own_case_on_a_backend_that_prepares() {
    for mutant in Mutant::ALL {
        let report = run_conformance(&HoldsItsOwnWorkspace::mutated(mutant));
        assert!(
            !report.is_conformant(),
            "the planted defect {} was not caught on a strict backend\n{report}",
            mutant.as_str()
        );
        assert_eq!(
            report.failing_ids(),
            vec![mutant.caught_by()],
            "on a strict backend, {} should fail exactly {} and nothing else\n{report}",
            mutant.as_str(),
            mutant.caught_by()
        );
    }
}

/// The other side of the rule, and the reason `unsupported` is not spent here: a backend that
/// declares mounting and read-only presentation and then will not name a workspace or a head has
/// declared two things nothing can check. It is graded as having failed them.
#[test]
fn a_backend_that_will_not_prepare_is_failed_rather_than_skipped() {
    let report = run_conformance(&WillNotPrepare::new());
    assert!(
        !report.is_conformant(),
        "a backend nothing about mounting could be checked on was reported conformant\n{report}"
    );
    for id in MOUNT_AND_READONLY_CASES {
        assert_eq!(
            case_result(&report, id),
            CaseResult::Fail,
            "{id} was not graded `fail` on a backend that would not prepare\n{report}"
        );
    }
    for case in report.cases() {
        if matches!(
            case.rule().family(),
            CaseFamily::Mount | CaseFamily::ReadOnly
        ) {
            assert_ne!(
                case.result(),
                CaseResult::Unsupported,
                "{} is `unsupported`, which is the word an honest partial backend earns\n{report}",
                case.id()
            );
        }
    }
    assert!(
        report
            .failures()
            .any(|case| case.detail().contains("prepare_fixture()")),
        "the report does not say the backend refused to prepare\n{report}"
    );
}

/// AC-1 and AC-2 of `01KZG5J7H8X16W56RX9AGWMCRJ`: the suite asks for portable relative
/// presentation paths, and a backend may resolve them below a root it owns.
#[test]
fn a_backend_confined_to_its_own_root_is_graded_on_mount_and_readonly() {
    let backend = RootedPaths::new("backend-owned-root");
    let fixture = backend.prepare_fixture().expect("the backend prepares");
    let actor = mesh_materializer::ActorId::from_bytes([0xa5; 32]);
    assert_eq!(
        backend.mount_actor_view(
            fixture.workspace(),
            actor,
            Path::new("/mesh/conformance/one"),
        ),
        Err(mesh_materializer::AdapterError::OutsideWorkspace),
        "the old host-global fixture must be unreachable to this backend"
    );
    assert_eq!(
        backend.materialize_readonly_view(fixture.head(), Path::new("/mesh/conformance/shared"),),
        Err(mesh_materializer::AdapterError::OutsideWorkspace),
        "the old host-global read-only fixture must be unreachable to this backend"
    );
    let mounted = backend
        .mount_actor_view(fixture.workspace(), actor, Path::new("requested/actor"))
        .expect("a relative mount request resolves below the owned root");
    assert_eq!(
        mounted.mountpoint(),
        Path::new("backend-owned-root/requested/actor"),
        "the result must report the path actually used, not the unresolved request"
    );
    backend.release(mounted.id()).expect("the probe releases");

    let readonly = backend
        .materialize_readonly_view(fixture.head(), Path::new("requested/shadow"))
        .expect("a relative read-only request resolves below the owned root");
    assert_eq!(
        readonly.target(),
        Path::new("backend-owned-root/requested/shadow"),
        "the result must report the resolved read-only target"
    );
    backend.release(readonly.id()).expect("the probe releases");

    let report = run_conformance(&backend);
    assert!(report.is_conformant(), "{report}");
    for id in MOUNT_AND_READONLY_CASES {
        assert_eq!(
            case_result(&report, id),
            CaseResult::Pass,
            "{id} did not grade against a backend confined to its own root\n{report}"
        );
    }
}

/// AC-3: a backend which declares the two presentation capabilities but refuses the request is
/// failed on those families. `unsupported` remains reserved for an undeclared capability.
#[test]
fn a_backend_that_refuses_portable_paths_is_failed_rather_than_skipped() {
    let report = run_conformance(&RefusesPresentationPaths::new());
    for id in MOUNT_AND_READONLY_CASES {
        assert_eq!(
            case_result(&report, id),
            CaseResult::Fail,
            "{id} was not graded fail when its presentation path was refused\n{report}"
        );
    }
    for case in report.cases() {
        if matches!(
            case.rule().family(),
            CaseFamily::Mount | CaseFamily::ReadOnly
        ) {
            assert_ne!(
                case.result(),
                CaseResult::Unsupported,
                "{} was skipped after the backend declared its capability\n{report}",
                case.id()
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------
// 3. VOC — the published material and the Rust, in both directions
// ---------------------------------------------------------------------------------------------

/// Every variant of `pub enum <name>` as the source text declares it, in declaration order.
///
/// A source-text lint, with that technique's limits: it reads a `pub enum` body and would miss a
/// variant produced by a macro. `tests/vocabulary_drift.rs` records the same limit for itself, and
/// `src/adapter.rs` says in as many words that the capability enumeration stays a plain
/// declaration so that this check keeps working.
fn declared_variants(source: &str, enumeration: &str) -> Vec<String> {
    let header = format!("pub enum {enumeration} {{");
    let start = source
        .find(&header)
        .unwrap_or_else(|| panic!("{enumeration} is not declared as a plain `pub enum`"));
    let body = &source[start + header.len()..];

    let mut depth = 0usize;
    let mut variants = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if depth == 0 && trimmed.starts_with('}') {
            break;
        }
        if depth == 0
            && !trimmed.is_empty()
            && !trimmed.starts_with("//")
            && !trimmed.starts_with('#')
        {
            let name: String = trimmed
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                variants.push(name);
            }
        }
        depth += line.matches('{').count();
        depth = depth.saturating_sub(line.matches('}').count());
    }
    variants
}

/// The balanced `[...]` that follows `"key":` in a JSON document.
fn json_array<'a>(document: &'a str, key: &str) -> &'a str {
    let anchor = document
        .find(&format!("\"{key}\""))
        .unwrap_or_else(|| panic!("vocabulary.json has no {key} list"));
    let open = document[anchor..]
        .find('[')
        .unwrap_or_else(|| panic!("{key} is not a list"))
        + anchor;

    let bytes = document.as_bytes();
    let (mut depth, mut index, mut in_string, mut escaped) = (0usize, open, false, false);
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
        } else {
            match byte {
                b'"' => in_string = true,
                b'[' => depth += 1,
                b']' => {
                    depth -= 1;
                    if depth == 0 {
                        return &document[open..=index];
                    }
                }
                _ => {}
            }
        }
        index += 1;
    }
    panic!("{key} is not a balanced list");
}

/// Every quoted string in a fragment, in order.
fn json_strings(fragment: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut characters = fragment.char_indices();
    while let Some((start, character)) = characters.next() {
        if character != '"' {
            continue;
        }
        let mut value = String::new();
        let mut escaped = false;
        for (_, next) in characters.by_ref() {
            if escaped {
                value.push(next);
                escaped = false;
            } else if next == '\\' {
                escaped = true;
            } else if next == '"' {
                break;
            } else {
                value.push(next);
            }
        }
        let _ = start;
        found.push(value);
    }
    found
}

/// The value of every `"field": "..."` in a fragment, in order.
fn json_field(fragment: &str, field: &str) -> Vec<String> {
    let needle = format!("\"{field}\"");
    let mut found = Vec::new();
    let mut rest = fragment;
    while let Some(at) = rest.find(&needle) {
        rest = &rest[at + needle.len()..];
        let Some(colon) = rest.find(':') else { break };
        let after = &rest[colon + 1..];
        let Some(quote) = after.find('"') else { break };
        let value = json_strings(&after[quote..]);
        if let Some(first) = value.first() {
            found.push(first.clone());
        }
        rest = after;
    }
    found
}

fn assert_agrees(what: &str, rust: &[String], published: &[String]) {
    let in_rust: BTreeSet<&String> = rust.iter().collect();
    let in_json: BTreeSet<&String> = published.iter().collect();
    let only_rust: Vec<&&String> = in_rust.difference(&in_json).collect();
    let only_json: Vec<&&String> = in_json.difference(&in_rust).collect();
    assert!(
        only_rust.is_empty(),
        "{what} declared in Rust and missing from vocabulary.json: {only_rust:?}"
    );
    assert!(
        only_json.is_empty(),
        "{what} published in vocabulary.json and missing from Rust: {only_json:?}"
    );
    assert_eq!(
        rust, published,
        "{what} agree as sets and not as sequences; the published order is the declaration order"
    );
}

#[test]
fn voc_every_capability_is_in_both_and_in_the_same_order() {
    let published = json_field(json_array(&vocabulary(), "capabilities"), "name");
    let declared = declared_variants(&adapter_source(), "AdapterCapability");
    assert_agrees("capabilities", &declared, &published);

    // The third direction, and the one `src/adapter.rs` says it cannot close from inside: a
    // variant appended to the enumeration and never added to `AdapterCapability::ALL` compiles
    // clean under the fixed length, under the exhaustive `as_str` match and under the
    // declaration-order assertion. It does not survive this.
    let iterated: Vec<String> = AdapterCapability::ALL
        .iter()
        .map(|capability| capability.as_str().to_owned())
        .collect();
    assert_eq!(
        declared, iterated,
        "AdapterCapability declares one list of variants and AdapterCapability::ALL iterates \
         another. A capability that ALL does not carry is a capability the catalogue never grades."
    );
}

#[test]
fn voc_every_error_event_kind_and_boundary_reason_is_in_both() {
    let document = vocabulary();
    let source = adapter_source();
    assert_agrees(
        "error variants",
        &declared_variants(&source, "AdapterError"),
        &json_field(json_array(&document, "errors"), "name"),
    );
    assert_agrees(
        "event kinds",
        &declared_variants(&source, "FsEventKind"),
        &json_strings(json_array(&document, "event_kinds")),
    );
    assert_agrees(
        "boundary reasons",
        &declared_variants(&source, "BoundaryReason"),
        &json_strings(json_array(&document, "boundary_reasons")),
    );
}

#[test]
fn voc_the_three_results_and_the_families_are_in_both() {
    let document = vocabulary();
    let published_results = json_field(json_array(&document, "results"), "name");
    let graded: Vec<String> = [CaseResult::Pass, CaseResult::Fail, CaseResult::Unsupported]
        .iter()
        .map(|result| result.as_str().to_owned())
        .collect();
    assert_agrees("results", &graded, &published_results);

    // `VOC` is published and is not a family `run_conformance` emits: it reads a file, and nothing
    // under `src/` may. Everything else must be on both sides.
    let published_families: BTreeSet<String> = json_field(json_array(&document, "families"), "id")
        .into_iter()
        .collect();
    let emitted: BTreeSet<String> = CaseFamily::ALL
        .iter()
        .map(|family| family.as_str().to_owned())
        .collect();
    let mut expected = emitted.clone();
    expected.insert("VOC".to_owned());
    assert_eq!(
        published_families, expected,
        "vocabulary.json's families and the families the suite emits (plus VOC, which lives in \
         this file) do not agree"
    );

    let used: BTreeSet<String> = conformance_catalogue()
        .into_iter()
        .map(|rule| rule.family().as_str().to_owned())
        .collect();
    assert_eq!(
        used, emitted,
        "a family is declared and no case in the catalogue is in it"
    );
}

#[test]
fn voc_every_published_operation_is_a_method_on_the_seam() {
    let source = adapter_source();
    for operation in json_field(json_array(&vocabulary(), "capabilities"), "operation") {
        if operation == "reserved" {
            continue;
        }
        for method in operation.split(" / ") {
            assert!(
                source.contains(&format!("fn {method}(")),
                "vocabulary.json publishes the operation {method}, and the trait has no such method"
            );
        }
    }
}

/// `prepare_fixture` is published in all three places, or in none.
///
/// It is not a capability, so the `capabilities` list is the wrong home for it and
/// `voc_every_published_operation_is_a_method_on_the_seam` does not see it. This is its VOC case.
#[test]
fn voc_the_fixture_arrangement_is_published_wherever_it_is_stated() {
    let document = vocabulary();
    let source = adapter_source();
    let readme = read("tests/compatibility/adapter/v0/README.md");

    assert!(
        source.contains("fn prepare_fixture("),
        "the trait has no prepare_fixture, and the published material describes one"
    );
    assert!(
        source.contains("pub struct AdapterFixture"),
        "AdapterFixture is not a plain published struct"
    );
    for stated in [
        "\"fixture\"",
        "prepare_fixture",
        "01KZFXFR4KHDN8EJ8R0750E0T6",
    ] {
        assert!(
            document.contains(stated),
            "vocabulary.json does not publish {stated}, so the arrangement is a private agreement \
             between the suite and one backend — which is what 01KZFXFR4KHDN8EJ8R0750E0T6 is about"
        );
    }
    assert!(
        readme.contains("fn prepare_fixture("),
        "README.md section 1 lists a trait that is not the trait"
    );
    assert!(
        readme.contains("AdapterFixture"),
        "README.md never names the type a backend has to return"
    );
}

/// The path meaning is one contract, not a private convention in the rooted test backend.
#[test]
fn voc_the_presentation_path_rule_is_published_and_the_suite_uses_it() {
    for (what, text) in [
        ("crates/mesh-materializer/src/adapter.rs", adapter_source()),
        (
            "tests/compatibility/adapter/v0/README.md",
            read("tests/compatibility/adapter/v0/README.md"),
        ),
        (
            "tests/compatibility/adapter/v0/vocabulary.json",
            vocabulary(),
        ),
    ] {
        for phrase in ["relative", "absolute", "actually used"] {
            assert!(
                text.contains(phrase),
                "{what} does not publish the presentation-path rule phrase {phrase:?}"
            );
        }
    }

    let suite = read("crates/mesh-materializer/src/conformance.rs");
    assert!(
        suite.contains("\"mesh-conformance/one\"") && suite.contains("\"mesh-conformance/shared\""),
        "the suite does not use its two published relative presentation requests"
    );
    assert!(
        !suite.contains("\"/mesh/conformance/"),
        "the suite again requires permission to create a host-global /mesh directory"
    );
}

/// AC-3 of `01KZFXF9N49NHJ7XS3MD0MX3BR`: the Rust, `README.md` and `vocabulary.json` say one thing
/// about where `NameRejected` comes from, and there is no third wording.
///
/// The ruling is that on the view **the type is the enforcement**. What this holds is that all
/// three places say so and all three name the ruling, so a reader of any one of them cannot form a
/// different belief from a reader of another.
#[test]
fn voc_the_name_rejected_ruling_is_stated_in_all_three_places() {
    const RULING: &str = "01KZFXF9N49NHJ7XS3MD0MX3BR";
    for (what, text) in [
        ("crates/mesh-materializer/src/adapter.rs", adapter_source()),
        (
            "tests/compatibility/adapter/v0/README.md",
            read("tests/compatibility/adapter/v0/README.md"),
        ),
        (
            "tests/compatibility/adapter/v0/vocabulary.json",
            vocabulary(),
        ),
    ] {
        assert!(
            text.contains(RULING),
            "{what} does not cite the ruling {RULING} that decided where NameRejected is produced"
        );
        assert!(
            text.contains("the type is the enforcement"),
            "{what} does not state the ruling: on the view, the type is the enforcement"
        );
        assert!(
            text.contains("mount_actor_view") && text.contains("materialize_readonly_view"),
            "{what} does not name the two operations that DO produce NameRejected"
        );
    }

    // And the suite really does grade it there, on both surfaces, rather than only saying so.
    let graded: Vec<&str> = conformance_catalogue()
        .into_iter()
        .filter(|rule| rule.family() == CaseFamily::Name && rule.capability().is_some())
        .map(|rule| rule.id())
        .collect();
    assert_eq!(
        graded,
        vec![
            "NAME/relative-names-are-refused",
            "NAME/relative-target-is-refused"
        ],
        "the NAME family does not grade exactly the two path surfaces the ruling puts it on"
    );
}

/// The parser is only worth having if it can fail, and it cannot be observed failing without
/// failing the build. This runs the same predicate over text that must be rejected.
#[test]
fn the_variant_scan_sees_an_appended_variant() {
    let planted = "pub enum AdapterCapability {\n    Lookup,\n    Symlink,\n    Hardlink,\n}\n";
    assert_eq!(
        declared_variants(planted, "AdapterCapability"),
        vec![
            "Lookup".to_owned(),
            "Symlink".to_owned(),
            "Hardlink".to_owned()
        ],
        "a variant appended after Symlink must be seen, because that is the gap this closes"
    );

    let with_payloads =
        "pub enum AdapterError {\n    /// doc\n    Unsupported {\n        capability: X,\n    },\n    NameRejected(NameError),\n    NotFound,\n}\n";
    assert_eq!(
        declared_variants(with_payloads, "AdapterError"),
        vec![
            "Unsupported".to_owned(),
            "NameRejected".to_owned(),
            "NotFound".to_owned()
        ]
    );
}

// ---------------------------------------------------------------------------------------------
// 4. A backend author reads the contract, not Mesh (Contract 9)
// ---------------------------------------------------------------------------------------------

/// What the suite may not name, because a backend cannot read it.
const FORBIDDEN_INTERNALS: [&str; 12] = [
    "WorkspaceState",
    "materialize",
    "apply_operation",
    "Operation",
    "AppliedChangeSet",
    "causal_order",
    "Effect",
    "Rejection",
    "RejectedOperation",
    "StateDigest",
    "CanonicalAdvance",
    "DirectoryVersion",
];

/// Whether `source` names `needle` as a whole identifier.
///
/// Substring matching alone is wrong in both directions here: `mesh-materializer`, the crate's own
/// name in its own doc comments, contains `materialize`, and so does the adapter contract's
/// `materialize_readonly_view`, which the suite is *required* to call. Neither is a use of the
/// materializer's `materialize`. An identifier boundary on both ends separates the three without
/// an allow-list that would have to be kept honest by hand.
fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn names_identifier(source: &str, needle: &str) -> bool {
    let bytes = source.as_bytes();
    let mut from = 0;
    while let Some(at) = source[from..].find(needle) {
        let start = from + at;
        let end = start + needle.len();
        let opens = start == 0 || !is_identifier_byte(bytes[start - 1]);
        let closes = end == bytes.len() || !is_identifier_byte(bytes[end]);
        if opens && closes {
            return true;
        }
        from = start + 1;
    }
    false
}

fn internals_named_in(source: &str) -> Vec<&'static str> {
    FORBIDDEN_INTERNALS
        .into_iter()
        .filter(|needle| names_identifier(source, needle))
        .collect()
}

#[test]
fn the_suite_names_no_materializer_internal() {
    let named = internals_named_in(&suite_source());
    assert!(
        named.is_empty(),
        "the suite names {named:?}. If the suite cannot be written without this crate's \
         internals, neither can a backend — and the published contract promises otherwise."
    );
}

#[test]
fn the_internals_lint_rejects_what_it_is_looking_for() {
    for planted in [
        "let state = WorkspaceState::default();",
        "let effect = apply_operation(&mut state, op);",
        "for id in causal_order(&sets) {}",
        "fn oracle() -> Materialization { materialize(root, &sets) }",
        "match outcome { Effect::Applied => (), Rejection::Refused => () }",
    ] {
        assert!(
            !internals_named_in(planted).is_empty(),
            "the lint would accept {planted:?}"
        );
    }
    for legitimate in [
        "adapter.materialize_readonly_view(head, target)",
        "let view: MaterializedView = ...;",
        "AdapterCapability::MaterializeReadonlyView",
        "//! nothing under `crates/mesh-materializer/src/` may name a filesystem",
        "use mesh_materializer::{run_conformance, WorkspaceAdapter};",
        "view.create_file(parent, &name, metadata)",
    ] {
        assert!(
            internals_named_in(legitimate).is_empty(),
            "the lint would reject {legitimate:?}"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// 5. CAT — every case cites published material that exists
// ---------------------------------------------------------------------------------------------

#[test]
fn every_citation_resolves_to_a_file_that_exists() {
    let root = repository_root();
    for rule in conformance_catalogue() {
        let cited = rule.citation();
        assert!(
            !cited.starts_with("crates/"),
            "{} cites {cited}, which is this repository's internals",
            rule.id()
        );
        let file = cited.split(" §").next().expect("a path");
        assert!(
            root.join(file).exists(),
            "{} cites {file}, which does not exist",
            rule.id()
        );
    }
}

/// AC-3, made checkable: every capability the vocabulary publishes has at least one case that is
/// about the capability itself rather than about the declaration of it. `Symlink` is the one
/// exception and the reason is published — contract 0 has no symlink operation to grade.
#[test]
fn every_published_capability_has_a_case_of_its_own() {
    let catalogue = conformance_catalogue();
    for capability in AdapterCapability::ALL {
        let cases: Vec<&str> = catalogue
            .iter()
            .filter(|rule| rule.capability() == Some(capability))
            .map(|rule| rule.id())
            .collect();
        assert!(
            !cases.is_empty(),
            "{capability} has no case at all, so nothing about it is ever graded"
        );
        if capability == AdapterCapability::Symlink {
            assert_eq!(cases, vec!["CAP/symlink-is-reserved"]);
            continue;
        }
        assert!(
            cases.iter().any(|id| !id.starts_with("CAP/")),
            "{capability} is only ever graded on whether it was declared, never on what it does: \
             {cases:?}"
        );
    }
}

#[test]
fn no_two_rules_share_an_identifier_with_a_different_rule() {
    for rule in conformance_catalogue() {
        for other in conformance_catalogue() {
            if rule.id() == other.id() {
                assert_eq!(
                    rule,
                    other,
                    "two different rules are both called {}",
                    rule.id()
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// 6. The report a backend author actually reads
// ---------------------------------------------------------------------------------------------

#[test]
fn a_failing_report_prints_the_rule_the_citation_and_the_difference() {
    let report = run_conformance(&ReferenceAdapter::mutated(
        Mutant::RemoveNonemptyDirectorySucceeds,
    ));
    let rendered = report.to_string();
    assert!(rendered.contains("NOT CONFORMANT"), "{rendered}");
    assert!(
        rendered.contains("OP-rmdir/directory-not-empty"),
        "{rendered}"
    );
    assert!(rendered.contains("rule:"), "{rendered}");
    assert!(
        rendered.contains("tests/compatibility/adapter/v0/"),
        "a backend author is pointed at the published rule, not at an assertion\n{rendered}"
    );
    assert!(rendered.contains("found:"), "{rendered}");
    assert!(
        rendered.contains("recursive delete"),
        "the rule text should say what the backend did wrong\n{rendered}"
    );
    assert!(is_conformant_word_free(&run_conformance(
        &ReferenceAdapter::new()
    )));
}

fn is_conformant_word_free(report: &ConformanceReport) -> bool {
    report.to_string().contains("CONFORMANT") && report.is_conformant()
}
