//! What `crates/mesh-fuse/src/` may not name.
//!
//! # The three bans, and what each one is protecting
//!
//! | Banned in `src/` | Why |
//! |---|---|
//! | `std::time` and every clock | Order at this seam is `EventSequence`, minted per view. The plan's rule is `lamport → event_ulid → content-hash`, **never wall-clock**, and a durable-boundary decision taken from a timer would make Mesh's checkpoints a function of when the machine was busy. |
//! | `std::fs`, `std::net`, `std::process`, `std::env` | This crate's adapter is the *semantic* half of a FUSE backend. It holds state in memory and reaches nothing. The moment it opens a file, "the mountpoint is a name and not a place" stops being true and the confinement finding the folder-watching fallback published comes back here without anybody noticing. |
//! | Anything under `unsafe` | `lib.rs` carries `#![forbid(unsafe_code)]`, so this is belt and braces — but plan §14.3 rule 5 puts `unsafe` behind a named human reviewer, and a lint that says so where a reader is looking is cheaper than one that only fires at review time. |
//!
//! # What this is not
//!
//! A lint over source text, not a proof. `crates/mesh-types/src/no_ambient_io.rs` states the same
//! limit and it is inherited here: a module path reached through a re-export, or a dependency that
//! reads a clock on this crate's behalf, is invisible to a string search. This crate has exactly
//! one dependency and it is `mesh-materializer`, which carries the same ban and enforces it the
//! same way, so the closure this checks over is two crates deep and both of them check.
//!
//! Verification is **warm** (`docs/adr/0004`).

use std::fs;
use std::path::{Path, PathBuf};

/// Every `.rs` file under this crate's `src/`.
fn sources() -> Vec<PathBuf> {
    fn walk(at: &Path, into: &mut Vec<PathBuf>) {
        let listing = fs::read_dir(at).expect("this crate's own source directory is readable");
        let mut entries: Vec<PathBuf> = listing.flatten().map(|entry| entry.path()).collect();
        // Sorted, so a failure names the same file every time rather than whichever the directory
        // happened to hand back first.
        entries.sort();
        for path in entries {
            if path.is_dir() {
                walk(&path, into);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                into.push(path);
            }
        }
    }
    let mut found = Vec::new();
    walk(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut found,
    );
    assert!(
        found.len() >= 5,
        "only {} source files were found, so this lint is scanning the wrong place",
        found.len()
    );
    found
}

#[test]
fn nothing_in_src_names_a_clock() {
    for path in sources() {
        let text = fs::read_to_string(&path).expect("a readable source file");
        for banned in [
            "std::time",
            "Instant",
            "SystemTime",
            "UNIX_EPOCH",
            "Duration",
        ] {
            assert!(
                !text.contains(banned),
                "{} names {banned}. Order at this seam is EventSequence, minted per view; a \
                 durable boundary decided from a clock is a boundary that moves when the machine \
                 is busy",
                path.display()
            );
        }
    }
}

#[test]
fn nothing_in_src_reaches_the_filesystem_the_network_or_the_process_table() {
    for path in sources() {
        let text = fs::read_to_string(&path).expect("a readable source file");
        for banned in [
            "std::fs",
            "std::net",
            "std::process",
            "std::env",
            "std::os::",
        ] {
            assert!(
                !text.contains(banned),
                "{} names {banned}. The adapter half of this crate holds its state in memory and \
                 touches nothing; the day it does, the mountpoint stops being a name and the \
                 confinement restriction comes back",
                path.display()
            );
        }
    }
}

#[test]
fn nothing_in_src_is_unsafe() {
    for path in sources() {
        let text = fs::read_to_string(&path).expect("a readable source file");
        // `lib.rs` carries the attribute, so it is the one file allowed to say the word.
        let allowed = path.ends_with("lib.rs");
        assert!(
            allowed || !text.contains("unsafe"),
            "{} names unsafe. Plan §14.3 rule 5 puts that behind a named human reviewer",
            path.display()
        );
    }
}

/// The one dependency, and the evidence that adding a second is a visible act.
///
/// The manifest is read rather than trusted: `fuser` arriving here is the change that turns three
/// of this task's acceptance criteria from unmet into measurable, and it is also the change
/// ADR-0014's fence does not admit. Whether it widens is `01KZHAPMMK7YGF9AJ8QGPV3EP7`; either way
/// it must not happen quietly.
#[test]
fn the_manifest_declares_one_dependency_and_it_is_a_workspace_path_edge() {
    let manifest = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("this crate's own manifest");
    let dependencies = manifest
        .split_once("[dependencies]")
        .expect("the manifest declares dependencies")
        .1;
    let declared: Vec<&str> = dependencies
        .lines()
        .filter(|line| line.contains(" = "))
        .collect();
    assert_eq!(declared.len(), 1, "declared: {declared:?}");
    assert!(
        declared[0].starts_with("mesh-materializer = { path = "),
        "the one edge is not the intra-workspace path edge ADR-0014 admits: {}",
        declared[0]
    );
    for third_party in ["fuser", "libc", "nix", "tokio", "log"] {
        assert!(
            !dependencies.contains(&format!("\n{third_party} =")),
            "{third_party} is a dependency now. ADR-0014's fence admits an audited cryptographic \
             dependency and nothing else, so this is an escalation rather than a lane's decision"
        );
    }
}
