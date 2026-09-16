//! This crate reads no clock, no filesystem, no socket and no process table, and this is the scan
//! that says so.
//!
//! # Why it matters here specifically
//!
//! Materialization must be a pure function of the operation set. A clock read anywhere in `src/`
//! would make it a function of when it ran, and SG-1 — *materializing the same causal set on any
//! peer, in any delivery order, produces the same state hash* — would become a statement about one
//! machine at one moment. The other three are the plan §8.3 rule for a pure-core crate: no storage,
//! no network, no platform adapter, and `std` supplies all of them with no dependency at all.
//!
//! # Why a test and not a compile-time assertion
//!
//! `mesh-types` and `mesh-state` make this a `const` scan inside `src/`, which is stronger — it
//! fails at `cargo build`. A `src/` file that spells the module paths it searches for needs an
//! `ambientScanExempt` entry in `tools/program/arch-check/architecture.json`, and that path is
//! outside this task's allowed paths. `mesh-operations` hit the same fence and made it a test; this
//! follows it, and the cost is stated rather than hidden: this fails at `cargo nextest run` rather
//! than at `cargo build`.

use std::fs;
use std::path::{Path, PathBuf};

/// What no source file in `src/` may name.
///
/// `std::time` is on the list and the other crates' scans have it for the same reason: a materialized
/// state that read a clock would be a different state tomorrow.
const FORBIDDEN: [&str; 8] = [
    "std::fs",
    "std::net",
    "std::process",
    "std::os",
    "std::time",
    "SystemTime",
    "Instant",
    "std::env",
];

fn source_directory() -> PathBuf {
    [env!("CARGO_MANIFEST_DIR"), "src"].iter().collect()
}

fn rust_sources(at: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let entries = fs::read_dir(at).unwrap_or_else(|error| panic!("cannot read {at:?} ({error})"));
    for entry in entries {
        let path = entry.expect("a readable directory entry").path();
        if path.is_dir() {
            found.extend(rust_sources(&path));
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
    found.sort();
    found
}

#[test]
fn no_source_file_reaches_for_ambient_input() {
    let sources = rust_sources(&source_directory());
    assert!(
        sources.len() >= 8,
        "found only {} source files, so this scan is looking at the wrong place: {sources:?}",
        sources.len()
    );

    for path in &sources {
        let text = fs::read_to_string(path).expect("a readable source file");
        for (number, line) in text.lines().enumerate() {
            // The doc comment above names every needle, and it is in this file rather than in
            // `src/`, so nothing here needs an exemption.
            for needle in FORBIDDEN {
                assert!(
                    !line.contains(needle),
                    "{}:{} names `{needle}`. Materialization is a pure function of the operation \
                     set; a clock, a filesystem, a socket, a process table or an environment \
                     variable in `src/` makes it a function of something else. If a type here \
                     needs one, the type is in the wrong crate.",
                    path.display(),
                    number + 1
                );
            }
        }
    }
}

/// The scan is only worth having if it can fail, and it cannot be observed failing without failing
/// the build. This runs the same predicate over text that should be rejected.
#[test]
fn the_scan_rejects_what_it_is_looking_for() {
    for offending in [
        "use std::fs;",
        "    let now = std::time::SystemTime::now();",
        "fn connect() -> std::net::TcpStream { todo!() }",
        "let home = std::env::var(\"HOME\");",
    ] {
        assert!(
            FORBIDDEN.iter().any(|needle| offending.contains(needle)),
            "the scan would accept {offending:?}"
        );
    }
    for legitimate in [
        "use std::collections::BTreeMap;",
        "/// The order is causal depth, then identifier.",
        "let bytes = value.to_be_bytes();",
    ] {
        assert!(
            !FORBIDDEN.iter().any(|needle| legitimate.contains(needle)),
            "the scan would reject {legitimate:?}"
        );
    }
}
