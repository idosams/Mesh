//! A compile-time scan asserting this crate cannot reach outside the process.
//!
//! # Why a validator crate in particular
//!
//! The load-bearing property of this crate is that a validator cannot influence what it validates.
//! The manifest half of that is in `Cargo.toml`: no dependency here carries `storage`, `network`
//! or `canonical-mutation`, and `tools/program/arch-check` enforces it. But `std` ships a
//! filesystem, a socket, a process table and an operating-system surface, so a lane could open a
//! file or spawn a process here without touching `Cargo.toml` at all, and the manifest half would
//! say nothing. This module closes the ordinary route.
//!
//! The consequence is the interesting part. Because this crate cannot spawn a process, it cannot
//! run a validator command **even if it wanted to** — which is why
//! [`ValidatorExecutor`](crate::ValidatorExecutor) is a port and not an implementation, and why
//! "no detected command executes before the user approves the profile" is a property of a
//! function that has no way to execute anything at all.
//!
//! # What it is not
//!
//! A proof. It reads text, so `use std as s;` followed by `s::process::Command` walks straight
//! past it, as does anything reached through a re-export or a macro. It catches the ordinary way
//! I/O enters a crate that is not supposed to have any: a lane that needed a file and did not know
//! the rule. It is a lint over source text, stated as one so nobody later mistakes it for a
//! guarantee.
//!
//! Two deliberate narrowings, carried over from `mesh-types`' copy of this scan:
//!
//! * `src/` only. An integration test that names one of these paths does not give the library the
//!   capability.
//! * `std::io` is absent. Its traits and its error type carry no capability, and banning the word
//!   would ban `std::io::Error` from an error enum without preventing a byte of I/O.
//!
//! This file exempts itself, because a scanner for a string necessarily contains that string.

/// The `std` module paths that reach outside the process.
const AMBIENT: [&[u8]; 4] = [b"std::fs", b"std::net", b"std::process", b"std::os"];

/// The byte every needle in [`AMBIENT`] begins with, checked by a test rather than assumed.
const AMBIENT_FIRST_BYTE: u8 = b's';

/// Every source file in the crate except this one, by name and content.
const SOURCES: [(&str, &str); 11] = [
    ("lib.rs", include_str!("lib.rs")),
    ("change.rs", include_str!("change.rs")),
    ("command.rs", include_str!("command.rs")),
    ("profile.rs", include_str!("profile.rs")),
    ("record.rs", include_str!("record.rs")),
    ("registry.rs", include_str!("registry.rs")),
    ("run.rs", include_str!("run.rs")),
    ("selection.rs", include_str!("selection.rs")),
    ("tooling.rs", include_str!("tooling.rs")),
    ("trigger.rs", include_str!("trigger.rs")),
    ("workspace.rs", include_str!("workspace.rs")),
];

/// The sources the list above deliberately omits. Checked by a test, so a module added to `lib.rs`
/// and forgotten in [`SOURCES`] fails rather than going unscanned. Exactly one file is here: this
/// one, which necessarily contains the strings it searches for.
#[cfg(test)]
const UNSCANNED: [&str; 1] = ["no_ambient_io.rs"];

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

/// Whether `source` names any ambient module.
///
/// One pass over the bytes, trying the needles only where a byte could begin one. The shape
/// matters: this runs in const evaluation, whose step budget a needle-at-a-time scan exhausts once
/// a crate grows past a dozen files.
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
    "mesh-validator reaches the filesystem, the network, the process table or the operating \
     system. A validator that can open a file or spawn a process is a validator that can change \
     what it is validating, and this crate's whole contract is that it cannot. The execution seam \
     is `ValidatorExecutor`, which the service layer implements — put the I/O there."
);

#[cfg(test)]
mod tests {
    use super::{
        no_source_reaches_outside_the_process, reaches_outside_the_process, starts_with, AMBIENT,
        AMBIENT_FIRST_BYTE, SOURCES, UNSCANNED,
    };

    #[test]
    fn the_real_sources_reach_nothing_outside_the_process() {
        assert!(no_source_reaches_outside_the_process());
    }

    /// A module added to `lib.rs` and forgotten here would be unscanned and nobody would notice,
    /// so the list is checked against the `mod` declarations rather than trusted. The one
    /// exemption is named in `UNSCANNED` and is asserted to be a real module, so a stale
    /// exemption fails too.
    #[test]
    fn every_declared_module_is_either_scanned_or_exempted_by_name() {
        let lib = SOURCES[0].1;
        let declared: Vec<String> = lib
            .lines()
            .filter_map(|line| {
                line.strip_prefix("pub mod ")
                    .or_else(|| line.strip_prefix("mod "))
            })
            .filter_map(|rest| rest.strip_suffix(';'))
            .map(|name| format!("{name}.rs"))
            .collect();
        assert!(
            !declared.is_empty(),
            "no module declarations found in lib.rs"
        );

        let scanned: Vec<&str> = SOURCES.iter().map(|(name, _)| *name).collect();
        for module in &declared {
            assert!(
                scanned.contains(&module.as_str()) || UNSCANNED.contains(&module.as_str()),
                "{module} is declared in lib.rs and is neither scanned nor exempted"
            );
        }
        // Every declared module accounted for exactly once, plus `lib.rs` itself in `SOURCES`.
        assert_eq!(scanned.len() + UNSCANNED.len(), declared.len() + 1);
        for exempt in UNSCANNED {
            assert!(
                declared.contains(&exempt.to_owned()),
                "{exempt} is exempted and is not a module"
            );
            assert!(
                !scanned.contains(&exempt),
                "{exempt} is both scanned and exempted"
            );
        }
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
        assert!(reaches_outside_the_process(
            "    let output = std::process::Command::new(\"cargo\");"
        ));
    }

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
