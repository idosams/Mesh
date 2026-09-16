//! A compile-time scan of this crate's own source for ambient storage, network, process and OS use.
//!
//! Plan §8.3 puts `mesh-operations` in the pure core: no storage, no network, no platform adapter.
//! Two guards carry that claim and they close different holes.
//!
//! * The manifest guard in `lib.rs` proves this crate declares **no dependency**. That is the guard
//!   that makes "no platform-adapter dependency" true, and it is the one
//!   `tools/program/arch-check` checks from the outside.
//! * *No dependency* is not the same claim as *no I/O*: the standard library ships a filesystem, a
//!   network stack and a process table, so a lane could open a file here without touching
//!   `Cargo.toml` and the manifest guard would say nothing. This module closes that route.
//!
//! **What it is.** A `const fn` scan of every other source file in the crate for the four module
//! paths that reach the filesystem, the network, the process table or the operating system,
//! asserted at compile time so the crate does not build if one appears.
//!
//! **What it is not.** A proof. It reads text, so aliasing the standard library and reaching
//! through the alias walks straight past it, as does anything reached through a re-export. It
//! catches the ordinary way I/O arrives in a crate that is not supposed to have any — a lane that
//! needed a file and did not know the rule — and it is stated here as a lint rather than a
//! guarantee so that nobody later mistakes it for one.
//!
//! # Why this is a `const` assertion and not a test
//!
//! It was a test, `tests/no_ambient_io.rs`, for one PR: a `src/` file that names the four module
//! paths in order to search for them trips `arch-check`'s own rule 7, and the fix the checker
//! itself names — an `ambientScanExempt` entry in `tools/program/arch-check/architecture.json` —
//! was outside the allowed paths of the task that wrote the crate. The entry now exists, so the
//! guard sits where `mesh-types` and `mesh-state` keep theirs and fires one gate earlier: at
//! `cargo build` rather than at `cargo nextest run` (`01KZECET9ADD6JGYYGB8THBTZC`).
//!
//! This file exempts itself, because a scanner for a string necessarily contains that string. It
//! holds nothing but the scan.

/// The `std` module paths that reach outside the process.
///
/// `std::io` is deliberately absent: its traits and its error type are ordinary vocabulary that
/// carry no capability, and banning the word would ban `std::io::Error` from an error enum without
/// preventing a single byte of I/O. The same four `arch-check` scans for, held against
/// `tools/program/arch-check/lib/ambient.mjs` by `tests/ambient_needles.rs` so the two cannot
/// drift.
const AMBIENT: [&[u8]; 4] = [b"std::fs", b"std::net", b"std::process", b"std::os"];

/// Every source file in the crate except this one, by name and content.
///
/// Listed unconditionally. A module that is only scanned under some configuration is a module a
/// lane can add I/O to by changing that configuration.
const SOURCES: [(&str, &str); 14] = [
    ("lib.rs", include_str!("lib.rs")),
    ("canonical.rs", include_str!("canonical.rs")),
    ("cbor.rs", include_str!("cbor.rs")),
    ("cbor_reader.rs", include_str!("cbor_reader.rs")),
    ("changeset.rs", include_str!("changeset.rs")),
    ("coalesce.rs", include_str!("coalesce.rs")),
    ("context.rs", include_str!("context.rs")),
    ("corpus.rs", include_str!("corpus.rs")),
    ("encoding.rs", include_str!("encoding.rs")),
    ("head.rs", include_str!("head.rs")),
    ("ids.rs", include_str!("ids.rs")),
    ("name.rs", include_str!("name.rs")),
    ("operation.rs", include_str!("operation.rs")),
    ("sequence.rs", include_str!("sequence.rs")),
];

/// Whether `bytes` matches `needle` starting at `at`.
const fn starts_with(bytes: &[u8], at: usize, needle: &[u8]) -> bool {
    if needle.is_empty() || at + needle.len() > bytes.len() {
        return false;
    }
    let mut offset = 0;
    while offset < needle.len() {
        if bytes[at + offset] != needle[offset] {
            return false;
        }
        offset += 1;
    }
    true
}

/// The byte every needle in [`AMBIENT`] begins with, checked by a test rather than assumed.
const AMBIENT_FIRST_BYTE: u8 = b's';

/// Whether `source` names any ambient module.
///
/// One pass over the bytes, trying the needles only where a byte could begin one. Every needle
/// starts with the same byte, so the ordinary byte costs a single comparison. The shape matters:
/// this runs in const evaluation, whose step budget `mesh-types`' needle-at-a-time scan exhausted
/// the moment that crate passed eleven source files (`error: constant evaluation is taking a long
/// time`). This crate has fourteen and 210 KiB of source, so the single-pass shape is required
/// rather than preferred. A scan that stops building is a scan that gets deleted.
const fn reaches_outside_the_process(source: &str) -> bool {
    let bytes = source.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == AMBIENT_FIRST_BYTE {
            let mut needle = 0;
            while needle < AMBIENT.len() {
                if starts_with(bytes, at, AMBIENT[needle]) {
                    return true;
                }
                needle += 1;
            }
        }
        at += 1;
    }
    false
}

/// Whether no scanned source names an ambient `std` module.
const fn no_source_reaches_outside_the_process() -> bool {
    let mut file = 0;
    while file < SOURCES.len() {
        let (_, source) = SOURCES[file];
        if reaches_outside_the_process(source) {
            return false;
        }
        file += 1;
    }
    true
}

const _: () = assert!(
    no_source_reaches_outside_the_process(),
    "mesh-operations reaches the filesystem, the network, the process table or the operating \
     system. Plan §8.3 gives this crate none of them, and it needs no dependency to break that \
     rule — the standard library supplies all four. If an operation cannot be expressed without \
     platform knowledge, it belongs in an adapter, not the vocabulary."
);

#[cfg(test)]
mod tests {
    use super::{
        no_source_reaches_outside_the_process, reaches_outside_the_process, starts_with, AMBIENT,
        AMBIENT_FIRST_BYTE, SOURCES,
    };

    #[test]
    fn the_real_sources_reach_nothing_outside_the_process() {
        assert!(no_source_reaches_outside_the_process());
    }

    /// Every module the crate declares must be scanned. A module added to `lib.rs` and forgotten
    /// here would be unscanned and nobody would notice, so the list is checked against the `mod`
    /// declarations rather than trusted.
    #[test]
    fn every_declared_module_is_scanned() {
        let lib = SOURCES[0].1;
        let declared: Vec<String> = lib
            .lines()
            .filter_map(|line| line.strip_prefix("mod "))
            .filter_map(|rest| rest.strip_suffix(';'))
            .map(|name| format!("{name}.rs"))
            .collect();
        assert!(
            !declared.is_empty(),
            "no module declarations found in lib.rs"
        );

        let scanned: Vec<&str> = SOURCES.iter().map(|(name, _)| *name).collect();
        for module in &declared {
            if module == "no_ambient_io.rs" {
                continue;
            }
            assert!(
                scanned.contains(&module.as_str()),
                "{module} is declared in lib.rs but not scanned"
            );
        }
        // Every declared module except this one, plus `lib.rs` itself.
        assert_eq!(
            scanned.len(),
            declared.len(),
            "the scanned list and the module list have drifted: {scanned:?} against {declared:?}"
        );
        assert!(
            declared.contains(&"no_ambient_io.rs".to_owned()),
            "this module must be declared for the arithmetic above to hold"
        );
    }

    /// The scan is only worth having if it can fail.
    #[test]
    fn the_scan_rejects_each_ambient_module() {
        for needle in AMBIENT {
            let source = format!(
                "use {}::something;\nfn main() {{}}\n",
                core::str::from_utf8(needle).expect("the needles are ASCII")
            );
            assert!(
                reaches_outside_the_process(&source),
                "{needle:?} was not found"
            );
        }
        assert!(!reaches_outside_the_process("use core::fmt;"));
        assert!(!reaches_outside_the_process(""));
        assert!(
            !reaches_outside_the_process("std::f"),
            "a truncated prefix must not match"
        );
        // The needle may begin anywhere, not only at the start of the text.
        assert!(reaches_outside_the_process("    let file = std::fs::File;"));
    }

    /// The single-pass scan only tries a needle where a byte could begin one. If a needle ever
    /// started with something other than that byte, the skip would make it invisible.
    #[test]
    fn every_needle_begins_with_the_byte_the_scan_skips_on() {
        for needle in AMBIENT {
            assert_eq!(
                needle[0], AMBIENT_FIRST_BYTE,
                "{needle:?} does not begin with the byte the scan looks for"
            );
        }
    }

    #[test]
    fn a_prefix_match_respects_the_end_of_the_text() {
        assert!(starts_with(b"std::fs", 0, b"std::fs"));
        assert!(!starts_with(b"std::f", 0, b"std::fs"));
        assert!(!starts_with(b"std::fs", 1, b"std::fs"));
        assert!(!starts_with(b"std::fs", 0, b""));
    }
}
