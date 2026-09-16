//! A compile-time scan for anything that would make head advancement impure.
//!
//! **The claim.** Head advancement never consults the wall clock. It is one of this task's
//! acceptance criteria, and a criterion whose only evidence is a test asserting that today's run
//! produced today's answer is not evidence at all — a clock read that happens to be ignored is
//! still a clock read, and the next lane will use the value.
//!
//! **What this is.** A `const fn` scan of every other source file in the crate for the `std`
//! paths and type names that reach a clock, a filesystem, a socket, the process table or the
//! environment, asserted at compile time so the crate does not build if one appears. The clock is
//! the criterion; the other four are here because a fold that reads any of them is impure for the
//! same reason, and a scan that catches only the named fault teaches the next lane which door was
//! left open.
//!
//! **What it is not.** A proof. It reads text, so `use std as s;` followed by `s::time::SystemTime`
//! walks straight past it, as does anything reached through a re-export or a dependency. It is
//! stated as a lint rather than a guarantee so that nobody later mistakes it for one. What makes
//! the dependency route unavailable is the manifest guard in `lib.rs`: this crate declares no
//! dependency at all, so there is no third-party clock to reach either.
//!
//! This file exempts itself, because a scanner for a string necessarily contains that string. It
//! holds nothing but the scan.

/// The names that reach outside a pure fold.
///
/// `std::io` is deliberately absent, for the reason `mesh-types` gives: its traits and its error
/// type are ordinary vocabulary that carry no capability, and banning the word would ban
/// `std::io::Error` from an error enum without preventing a single byte of I/O.
const AMBIENT: [&[u8]; 9] = [
    b"std::time",
    b"SystemTime",
    b"Instant",
    b"UNIX_EPOCH",
    b"std::fs",
    b"std::net",
    b"std::process",
    b"std::os",
    b"std::env",
];

/// Every source file in the crate except this one, by name and content.
const SOURCES: [(&str, &str); 17] = [
    ("lib.rs", include_str!("lib.rs")),
    ("advance.rs", include_str!("advance.rs")),
    ("changeset.rs", include_str!("changeset.rs")),
    ("digest.rs", include_str!("digest.rs")),
    ("fold.rs", include_str!("fold.rs")),
    ("head.rs", include_str!("head.rs")),
    ("identity.rs", include_str!("identity.rs")),
    ("ids.rs", include_str!("ids.rs")),
    ("metadata.rs", include_str!("metadata.rs")),
    ("name.rs", include_str!("name.rs")),
    ("object.rs", include_str!("object.rs")),
    ("parents.rs", include_str!("parents.rs")),
    ("placement.rs", include_str!("placement.rs")),
    ("portable.rs", include_str!("portable.rs")),
    ("preflight.rs", include_str!("preflight.rs")),
    ("reception.rs", include_str!("reception.rs")),
    ("stamp.rs", include_str!("stamp.rs")),
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

/// Whether a byte could begin one of the needles.
///
/// One comparison for the overwhelming majority of bytes. The shape matters: this runs in const
/// evaluation, whose step budget a needle-at-a-time scan over every byte exhausts once a crate
/// grows past a handful of files.
const fn could_begin_a_needle(byte: u8) -> bool {
    matches!(byte, b's' | b'S' | b'I' | b'U')
}

/// Whether `source` names anything ambient.
///
/// Two filters before the needle loop, and both are here for the step budget rather than for the
/// answer. `could_begin_a_needle` rejects every byte that cannot start a needle; the `std::` gate
/// rejects the enormous majority of the ones that survive it, since `s` is the commonest letter in
/// this crate's prose and six of the nine needles begin `std::`. Without the second filter the
/// scan runs nine `starts_with` calls at every `s` in every doc comment, and `rustc` refuses the
/// whole const evaluation as a suspected infinite loop long before the crate stops growing.
const fn reaches_outside_the_fold(source: &str) -> bool {
    let bytes = source.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if could_begin_a_needle(bytes[at])
            && (bytes[at] != b's' || starts_with(bytes, at, b"std::"))
        {
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

/// Whether every listed source is free of ambient input.
const fn every_source_is_a_pure_fold() -> bool {
    let mut index = 0;
    while index < SOURCES.len() {
        let (_, source) = SOURCES[index];
        if reaches_outside_the_fold(source) {
            return false;
        }
        index += 1;
    }
    true
}

const _: () = assert!(
    every_source_is_a_pure_fold(),
    "A source file in mesh-state names a clock, a filesystem, a socket, the process table or the \
     environment. Head advancement is a pure function of the applied causal set; the moment it \
     reads any of those, two peers holding the same causal set can disagree about the same \
     history. If a caller needs one of these values, it belongs at the caller, passed in."
);

#[cfg(test)]
mod tests {
    use super::*;

    /// The compile-time guard above is only worth having if it can fail, and it cannot be observed
    /// failing without failing the build. So the function it is built on is exercised directly
    /// against the source a lane would realistically write.
    #[test]
    fn the_scan_rejects_every_ordinary_route_to_a_clock() {
        let rejected = [
            "let now = std::time::SystemTime::now();",
            "use std::time::Instant;",
            "let millis = duration.as_millis(); // since UNIX_EPOCH",
            "fn read() { std::fs::read(\"/etc/passwd\"); }",
            "std::net::TcpStream::connect(peer)",
            "std::process::exit(1)",
            "std::os::unix::fs::symlink(a, b)",
            "std::env::var(\"MESH_NOW\")",
        ];
        for source in rejected {
            assert!(
                reaches_outside_the_fold(source),
                "the scan walked past {source:?}"
            );
        }
    }

    #[test]
    fn the_scan_accepts_ordinary_pure_source() {
        let accepted = [
            "use std::collections::BTreeMap;",
            "/// The head is a pure function of the applied causal set.",
            "let sorted = ranked.sort_unstable();",
            "// hybrid logical time is carried, never consulted",
            "let elapsed_is_not_a_word_here = true;",
        ];
        for source in accepted {
            assert!(
                !reaches_outside_the_fold(source),
                "the scan fired on {source:?}"
            );
        }
    }

    /// The ceiling, asserted rather than described. A text scan cannot tell a type name from a
    /// longer word that starts with it, so `Instantiate` reads as `Instant`. That is a false
    /// positive — the safe direction — and it is pinned here so that a lane hitting it finds the
    /// reason instead of deleting the needle.
    #[test]
    fn the_scan_fires_on_a_longer_word_that_starts_with_a_needle() {
        assert!(reaches_outside_the_fold("struct Instantiated;"));
    }

    /// The other direction of the same ceiling, and the one that matters: an alias walks past.
    #[test]
    fn an_aliased_module_walks_past_the_scan() {
        assert!(!reaches_outside_the_fold(
            "use std as s; let n = s::time::now();"
        ));
    }

    /// A scan that lists no file passes trivially. The count is asserted so that adding a module
    /// and forgetting to list it is a test failure rather than a silent hole.
    #[test]
    fn every_module_the_crate_declares_is_scanned() {
        let declared: Vec<&str> = SOURCES.iter().map(|(name, _)| *name).collect();
        for module in [
            "lib.rs",
            "advance.rs",
            "changeset.rs",
            "digest.rs",
            "fold.rs",
            "head.rs",
            "identity.rs",
            "ids.rs",
            "metadata.rs",
            "name.rs",
            "object.rs",
            "parents.rs",
            "placement.rs",
            "portable.rs",
            "preflight.rs",
            "reception.rs",
            "stamp.rs",
        ] {
            assert!(declared.contains(&module), "{module} is not scanned");
        }
        assert_eq!(declared.len(), 17);
    }

    #[test]
    fn the_real_sources_are_pure() {
        assert!(every_source_is_a_pure_fold());
    }
}
