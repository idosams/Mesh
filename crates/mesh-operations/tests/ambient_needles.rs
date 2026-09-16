//! The compile-time ambient scan looks for the same four module paths `arch-check` does.
//!
//! # What is here and what is not
//!
//! The *scan* is `src/no_ambient_io.rs`, and it is a `const _: () = assert!(…)`: a source that
//! names one of the four fails `cargo build`, so there is nothing for a test to run. What a test
//! can still do is check the **needle list**, which lives in two places for a reason no test can
//! remove — `mesh-operations` declares no dependency, so it cannot import `arch-check`'s list and
//! `arch-check` cannot import this crate's.
//!
//! Two copies of a list is a divergence waiting to happen, and the direction that matters is the
//! silent one: `arch-check` grows a fifth needle, this crate does not, and the in-crate guard is
//! quietly narrower than the check it stands in for while both stay green. This file reads both
//! sources as text and holds them against each other, so that divergence is loud.
//!
//! This replaces the needle check that used to sit at the bottom of `tests/no_ambient_io.rs`. The
//! scan that file also carried is gone rather than moved — one guard, not two that can disagree
//! (`01KZECET9ADD6JGYYGB8THBTZC`).

mod common;

use common::read_repo_file;

const IN_CRATE: &str = "crates/mesh-operations/src/no_ambient_io.rs";
const ARCH_CHECK: &str = "tools/program/arch-check/lib/ambient.mjs";

/// The needles between `opener` and the next `]`, with byte-string and quote syntax stripped.
fn needles_between(source: &str, opener: &str) -> Vec<String> {
    let declared = source
        .split_once(opener)
        .unwrap_or_else(|| panic!("{opener:?} was not found; the declaration has moved"))
        .1
        .split_once(']')
        .expect("the list closes")
        .0;
    declared
        .split(',')
        .map(|item| {
            item.trim()
                .trim_start_matches('b')
                .trim_matches('\'')
                .trim_matches('"')
                .to_owned()
        })
        .filter(|item| !item.is_empty())
        .collect()
}

#[test]
fn the_in_crate_scan_and_the_architecture_check_look_for_the_same_paths() {
    let mine = needles_between(&read_repo_file(IN_CRATE), "const AMBIENT: [&[u8]; 4] = [");
    let theirs = needles_between(&read_repo_file(ARCH_CHECK), "export const AMBIENT = [");

    assert_eq!(
        mine.len(),
        4,
        "the in-crate needle list was read as {mine:?}, which is not the four paths it declares; \
         the parse has broken rather than the list having changed"
    );
    assert_eq!(
        mine, theirs,
        "`{IN_CRATE}`'s ambient scan and `{ARCH_CHECK}`'s have drifted. The in-crate guard fails \
         the build and is therefore the one a lane meets first; a needle only `arch-check` knows \
         about is a door the compile-time guard leaves open."
    );
}

/// The guard is in `src/`, not in `tests/`. That is the whole point of
/// `01KZECET9ADD6JGYYGB8THBTZC`, and a lane moving it back would move it one gate later without
/// any other check noticing.
#[test]
fn the_scan_is_a_compile_time_assertion_over_every_source() {
    let source = read_repo_file(IN_CRATE);
    assert!(
        source.contains("const _: () = assert!("),
        "`{IN_CRATE}` no longer asserts at compile time, so the scan has become something a lane \
         can reach `cargo build` past"
    );
    assert!(
        source.contains("const SOURCES: [(&str, &str); 14] = ["),
        "`{IN_CRATE}` no longer scans fourteen sources. Adding a module without adding it there \
         leaves that module unscanned; the crate's own `every_declared_module_is_scanned` is what \
         says which one."
    );
    let lib = read_repo_file("crates/mesh-operations/src/lib.rs");
    assert!(
        lib.contains("mod no_ambient_io;"),
        "the scan is not declared in lib.rs, so nothing compiles it and the assertion never runs"
    );
}
