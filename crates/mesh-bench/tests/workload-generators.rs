//! The determinism oracle for the workload generators — plan §12.2's W1–W6 and
//! the W7 this repository added.
//!
//! The task this file discharges names the standard directly: *"the same seed
//! must produce the same workload, across processes and across machines. Assert
//! that with a test that runs a generator twice in separate processes and
//! compares, not by inspection."*
//!
//! So nothing here inspects a generator. Three layers, each strictly stronger
//! than the one above it:
//!
//! 1. **Within one process** — build twice, compare. Catches a generator that
//!    reads mutable state it should not.
//! 2. **Across processes** — spawn the real `mesh-bench` binary twice and
//!    compare its output, and compare that against the in-process value. Catches
//!    a generator that depends on address-space layout, hash-map iteration
//!    order, allocator behaviour, a clock or an environment variable. Those are
//!    exactly the defects an in-process test cannot see, and the reason this
//!    file exists rather than another `#[test]` next to the code.
//! 3. **Across machines** — compare against `benchmarks/workloads/manifest.json`.
//!    Read this layer precisely, because it is the one that is easy to overclaim.
//!    Every digest in that file was computed on **one** machine, the Apple M2 Pro
//!    named in `benchmarks/workloads/README.md`. Run on that machine, layer 3
//!    proves only that the generator has not drifted since the digests were
//!    published — it says nothing whatever about another architecture. Run on any
//!    other machine, the same assertion becomes the cross-machine check, and the
//!    first such run is the first evidence that exists. Nothing a process can do
//!    substitutes for it: the property is *published here and observed
//!    elsewhere*, and until somebody runs it elsewhere it is unproven.
//!
//! Layer 3 has a property worth naming: it is the test that fails when a
//! generator is *improved*. That is intended. A corpus that changes silently
//! invalidates every number ever measured against it, so the manifest is meant
//! to be annoying to change, and changing it is meant to appear in a diff.

use mesh_bench::corpus::{build, Scale, Tally, WorkloadId, CANONICAL_SEED, SCALES, WORKLOADS};
use mesh_bench::json::{parse, Json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The binary cargo built for this test — a genuinely separate process.
const BINARY: &str = env!("CARGO_BIN_EXE_mesh-bench");

/// Where the cross-machine digests live.
fn manifest_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("benchmarks")
        .join("workloads")
        .join("manifest.json")
}

/// Runs `mesh-bench corpus …` in a child process and returns its parsed stdout.
fn corpus_command(arguments: &[&str]) -> Json {
    let output = Command::new(BINARY)
        .arg("corpus")
        .args(arguments)
        .output()
        .unwrap_or_else(|error| panic!("cannot run {BINARY}: {error}"));
    assert!(
        output.status.success(),
        "`corpus {}` exited {:?}: {}",
        arguments.join(" "),
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    parse(&String::from_utf8_lossy(&output.stdout)).unwrap_or_else(|error| {
        panic!(
            "`corpus {}` did not print JSON: {error}",
            arguments.join(" ")
        )
    })
}

/// A field of a child process's JSON output.
fn field(value: &Json, name: &str) -> String {
    value
        .as_object()
        .and_then(|object| object.get(name))
        .and_then(Json::as_str)
        .unwrap_or_else(|| panic!("the child process printed no `{name}`"))
        .to_owned()
}

/// The plan digest, computed in a child process.
fn child_plan_digest(id: WorkloadId, scale: Scale) -> String {
    let value = corpus_command(&[
        "digest",
        "--workload",
        id.code(),
        "--scale",
        scale.name(),
        "--seed",
        &CANONICAL_SEED.to_string(),
    ]);
    field(&value, "plan_digest")
}

/// The content digest, computed in a child process.
fn child_content_digest(id: WorkloadId, scale: Scale) -> String {
    let value = corpus_command(&[
        "digest",
        "--workload",
        id.code(),
        "--scale",
        scale.name(),
        "--seed",
        &CANONICAL_SEED.to_string(),
        "--content",
    ]);
    field(&value, "content_digest")
}

/// The scales cheap enough to run through a child process on every `npm test`.
const CHILD_SCALES: [Scale; 2] = [Scale::Reduced, Scale::Smoke];

#[test]
fn two_separate_processes_generate_the_same_plan_from_the_same_seed() {
    for id in WORKLOADS {
        for scale in CHILD_SCALES {
            let first = child_plan_digest(id, scale);
            let second = child_plan_digest(id, scale);
            assert_eq!(
                first,
                second,
                "{} at {} differs between two processes",
                id.code(),
                scale.name()
            );
            assert!(first.starts_with("fnv1a64:"), "{first} is not a digest");
        }
    }
}

#[test]
fn two_separate_processes_generate_the_same_bytes_from_the_same_seed() {
    for id in WORKLOADS {
        let first = child_content_digest(id, Scale::Smoke);
        let second = child_content_digest(id, Scale::Smoke);
        assert_eq!(
            first,
            second,
            "{} content differs between two processes",
            id.code()
        );
    }
}

#[test]
fn a_child_process_agrees_with_this_one() {
    // The layer that catches a generator reading something process-local: if
    // the child and the parent disagree, one of them is not a pure function of
    // the seed, and which one is beside the point.
    for id in WORKLOADS {
        for scale in CHILD_SCALES {
            let here = build(id, scale, CANONICAL_SEED).plan_digest();
            assert_eq!(
                here,
                child_plan_digest(id, scale),
                "{} at {}: in-process and child disagree",
                id.code(),
                scale.name()
            );
        }
        let here = build(id, Scale::Smoke, CANONICAL_SEED).content_digest();
        assert_eq!(
            here,
            child_content_digest(id, Scale::Smoke),
            "{} content: in-process and child disagree",
            id.code()
        );
    }
}

/// The published manifest still describes what the generators produce.
///
/// The name says *cross-machine* because that is what the manifest is published
/// **for**, not because this assertion has ever observed two machines. On the
/// publishing host it is a drift check; on any other host it is the cross-machine
/// check, and no run on the publishing host can promote it to one. See the
/// module doc, layer 3.
#[test]
fn every_workload_matches_the_published_cross_machine_manifest() {
    let text = std::fs::read_to_string(manifest_path())
        .unwrap_or_else(|error| panic!("{}: {error}", manifest_path().display()));
    let manifest = parse(&text).expect("the manifest is JSON");
    let rows = manifest
        .as_object()
        .and_then(|object| object.get("rows"))
        .and_then(Json::as_array)
        .expect("the manifest has rows");
    assert_eq!(
        rows.len(),
        WORKLOADS.len() * SCALES.len(),
        "the manifest does not cover every workload at every scale"
    );

    let mut seen = 0;
    for row in rows {
        let object = row.as_object().expect("a manifest row is an object");
        let code = object
            .get("workload")
            .and_then(Json::as_str)
            .expect("workload");
        let scale_name = object.get("scale").and_then(Json::as_str).expect("scale");
        let id = WorkloadId::parse(code).unwrap_or_else(|| panic!("unknown workload {code}"));
        let scale =
            Scale::parse(scale_name).unwrap_or_else(|| panic!("unknown scale {scale_name}"));
        let seed = object.get("seed").and_then(Json::as_u64).expect("seed");
        let published = object
            .get("plan_digest")
            .and_then(Json::as_str)
            .expect("plan_digest");

        let generator = build(id, scale, seed);
        assert_eq!(
            generator.plan_digest(),
            published,
            "{code} at {scale_name} no longer generates the published corpus. \
             If the generator changed on purpose, bump \
             `mesh_bench::corpus::GENERATOR_VERSION` and re-emit the manifest \
             with `node benchmarks/workloads/verify.mjs --all --content --emit`; \
             every number ever measured against the old corpus is invalidated by it."
        );
        assert!(
            generator.shape().holds(),
            "{code} at {scale_name} does not match its own stated shape"
        );
        seen += 1;
    }
    assert_eq!(seen, rows.len());
}

#[test]
fn the_published_shape_facts_are_still_produced() {
    let text = std::fs::read_to_string(manifest_path()).expect("the manifest is readable");
    let manifest = parse(&text).expect("the manifest is JSON");
    let rows = manifest
        .as_object()
        .and_then(|object| object.get("rows"))
        .and_then(Json::as_array)
        .expect("rows");
    for row in rows {
        let object = row.as_object().expect("an object");
        let code = object.get("workload").and_then(Json::as_str).expect("code");
        let scale_name = object.get("scale").and_then(Json::as_str).expect("scale");
        let id = WorkloadId::parse(code).expect("a known workload");
        let scale = Scale::parse(scale_name).expect("a known scale");
        let published = object
            .get("facts")
            .and_then(Json::as_object)
            .expect("published facts");
        let report = build(id, scale, CANONICAL_SEED).shape();
        for (name, _) in published.entries() {
            assert!(
                report.fact(name).is_some(),
                "{code} at {scale_name} no longer reports the published fact `{name}`"
            );
        }
    }
}

#[test]
fn two_separate_processes_write_byte_identical_corpora() {
    // The strongest form of the claim, and the one a benchmark actually
    // depends on: not that two runs agree about a digest, but that two runs
    // put the same bytes in the same paths on a real filesystem.
    let scratch = mesh_bench::testing::TempDir::new("workload-generators");
    for id in [WorkloadId::W1, WorkloadId::W4, WorkloadId::W6] {
        let first = scratch.path().join(format!("{}-first", id.code()));
        let second = scratch.path().join(format!("{}-second", id.code()));
        for root in [&first, &second] {
            corpus_command(&[
                "materialize",
                "--workload",
                id.code(),
                "--scale",
                "smoke",
                "--seed",
                &CANONICAL_SEED.to_string(),
                "--root",
                &root.display().to_string(),
            ]);
        }
        let left = read_tree(&first);
        let right = read_tree(&second);
        assert!(!left.is_empty(), "{} wrote nothing", id.code());
        assert_eq!(
            left.keys().collect::<Vec<_>>(),
            right.keys().collect::<Vec<_>>(),
            "{} produced different paths in two processes",
            id.code()
        );
        for (path, bytes) in &left {
            assert_eq!(
                bytes,
                right.get(path).expect("the path exists in both"),
                "{} differs at {path}",
                id.code()
            );
        }
    }
}

#[test]
fn a_materialised_corpus_is_the_corpus_that_was_described() {
    let scratch = mesh_bench::testing::TempDir::new("workload-materialised");
    let root = scratch.path().join("w1");
    corpus_command(&[
        "materialize",
        "--workload",
        "W1",
        "--scale",
        "smoke",
        "--seed",
        &CANONICAL_SEED.to_string(),
        "--root",
        &root.display().to_string(),
    ]);
    let written = read_tree(&root);
    let generator = build(WorkloadId::W1, Scale::Smoke, CANONICAL_SEED);
    let tally = Tally::of(generator.items());
    assert_eq!(written.len() as u64, tally.files);
    let on_disk: u64 = written.values().map(|bytes| bytes.len() as u64).sum();
    assert_eq!(on_disk, tally.logical_bytes);
}

#[test]
fn a_corpus_that_is_not_asked_for_is_refused_rather_than_guessed() {
    let cases: [(&[&str], i32); 4] = [
        (&["digest"], 2),
        (&["digest", "--workload", "W9", "--scale", "smoke"], 2),
        (&["digest", "--workload", "W1", "--scale", "enormous"], 2),
        (&["materialize", "--workload", "W1", "--scale", "smoke"], 2),
    ];
    for (arguments, expected) in cases {
        let output = Command::new(BINARY)
            .arg("corpus")
            .args(arguments)
            .output()
            .expect("the binary runs");
        assert_eq!(
            output.status.code(),
            Some(expected),
            "`corpus {}` should exit {expected}: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.stdout.is_empty(),
            "`corpus {}` printed a corpus it refused to build",
            arguments.join(" ")
        );
    }
}

#[test]
fn materialising_over_an_existing_corpus_is_refused() {
    let scratch = mesh_bench::testing::TempDir::new("workload-occupied");
    let root = scratch.path().join("w6");
    let arguments = [
        "materialize",
        "--workload",
        "W6",
        "--scale",
        "smoke",
        "--seed",
        "42",
        "--root",
        &root.display().to_string(),
    ];
    corpus_command(&arguments);
    let output = Command::new(BINARY)
        .arg("corpus")
        .args(arguments)
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("not empty"));
}

#[test]
fn the_listing_names_every_workload_the_plan_defines() {
    let output = Command::new(BINARY)
        .args(["corpus", "list"])
        .output()
        .expect("the binary runs");
    assert!(output.status.success());
    let listing = String::from_utf8_lossy(&output.stdout);
    for id in WORKLOADS {
        assert!(listing.contains(id.code()), "{} is not listed", id.code());
        assert!(listing.contains(id.title()), "{} has no title", id.code());
    }
}

/// Every file under `root`, keyed by its path relative to it.
fn read_tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = std::fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("{}: {error}", directory.display()));
        for entry in entries {
            let entry = entry.expect("a directory entry");
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .expect("inside the root")
                    .to_string_lossy()
                    .into_owned();
                files.insert(relative, std::fs::read(&path).expect("readable"));
            }
        }
    }
    files
}
