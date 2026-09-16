//! A compile-time scan for the two things this crate must never grow.
//!
//! **What it is.** A `const fn` over every other source file in the crate, asserted at compile
//! time, so the crate does not build if either appears:
//!
//! 1. **An accessor that hands back secret key material, or a type that holds one.** The claim in
//!    [`crate::custody`] is that there is no way to export a private key from this crate because
//!    there is no function that does it. That claim is one merge away from being false, and it will
//!    be a lane in a hurry that breaks it. Every spelling of the accessor a lane would actually
//!    reach for is listed in [`BANNED`], and so is `SigningKey` — the dependency's own secret type,
//!    which is in scope in this crate the moment `ed25519-dalek` is a dependency. Verification needs
//!    that dependency; *naming its secret half* is a different act and this crate never performs it.
//!    The signer that this crate's tests use lives under `tests/`, which is a separate crate.
//! 2. **Ambient storage, network or environment access.** A crypto crate that can open a file can
//!    write a key to one. `std::env` is here as well as `std::fs`: an environment variable is the
//!    most common place a key ends up by accident, and the second most common place it ends up in
//!    a support bundle.
//!
//! The tests below add four more, and they are **tests, not build failures**: an implementation
//! guard on [`crate::SignatureScheme`], one on [`crate::KeyCustody`] and [`crate::HumanKeyCustody`],
//! a coverage guard that every module of the crate is in [`SOURCES`], and a guard on the aliases
//! that would let a rename walk past the first two. They fail `cargo test -p mesh-crypto`. The
//! banned-identifier scan above fails the **build**. Rounding the second group up to the first is
//! how a reader ends up believing the crate cannot compile with a stub in it, and it can.
//!
//! **What it is not.** A proof — and on the crate the programme's publication claim rests on, the
//! distance between a lint and a proof is worth stating exactly rather than gesturing at.
//!
//! Every check in this file reads **source text**, one line at a time, with `//` lines skipped.
//! What that cannot see:
//!
//! * **A rename.** `use std as s;` then `s::fs::read` walks straight past the banned-identifier
//!   scan. The same move against the implementation guard — `use crate::SignatureScheme as S;` then
//!   `impl S for X` — is refused by [`tests::no_source_renames_a_guarded_trait`], which raises the
//!   cost of that one spelling without removing the class: a rename of the *module* on the way to
//!   the trait, or a two-hop alias, is still text this file does not follow.
//! * **A macro.** Anything an expansion produces is invisible; the scan reads what is written.
//! * **A block comment.** Only `//` is treated as a comment, so `/* impl SignatureScheme for X */`
//!   is scanned (a false positive, which fails closed) and text a block comment *hides* is not.
//! * **A `#[path]` attribute**, which severs a module's name from its file. Rather than follow it,
//!   [`tests::every_module_declared_in_the_crate_is_scanned`] refuses one outright.
//! * **A string literal** that happens to spell a declaration. It is read as one and fails closed.
//!
//! What does not depend on text, and is what a reader should actually cite: [`crate::KeyCustody`]
//! declares no method returning a secret, so no caller can invoke one; [`crate::SignatureScheme`]'s
//! `verify` has no default body, so no implementor inherits an answer; [`crate::DelegatedAction`]
//! has no canonical-advance variant, so an agent's capability cannot name the act. The compiler
//! checks those. This file catches the ordinary way a banned shape enters a crate that is not
//! supposed to have one — a lane that needed a file and did not know the rule — and it is stated
//! as a lint so that nobody later mistakes it for the control.
//!
//! Comment lines are skipped, so this file's own prose and every doc comment in the crate can name
//! the things it bans. That is deliberate: a lint that stopped the documentation from explaining
//! the rule would be traded away the first time somebody needed to write it down.
//!
//! This file exempts itself, because a scanner for a string necessarily contains that string.

/// Every source file in the crate except this one.
const SOURCES: [(&str, &str); 11] = [
    ("lib.rs", include_str!("lib.rs")),
    ("capability.rs", include_str!("capability.rs")),
    ("conformance_impl.rs", include_str!("conformance_impl.rs")),
    ("custody.rs", include_str!("custody.rs")),
    ("domain.rs", include_str!("domain.rs")),
    ("ed25519.rs", include_str!("ed25519.rs")),
    ("keys.rs", include_str!("keys.rs")),
    ("parts.rs", include_str!("parts.rs")),
    ("rotation.rs", include_str!("rotation.rs")),
    ("scheme.rs", include_str!("scheme.rs")),
    ("token.rs", include_str!("token.rs")),
];

/// The one file allowed to implement [`crate::SignatureScheme`].
///
/// Named rather than counted. "At most one implementation" is satisfied by deleting the real one
/// and adding a permissive one, which is the substitution this guard exists to stop.
#[cfg(test)]
const SIGNATURE_SCHEME_HOME: &str = "ed25519.rs";

/// Every banned spelling, in one array so the scan makes a single pass.
///
/// The banned *type* names are `SigningKey` and `SecretKey`, and the snake-case `signing_key` is
/// deliberately **not** here: [`crate::KeyRing::signing_key`] is the ring's current *public* key —
/// the one still allowed to sign — and banning the word would fire on correct code. A lint that
/// fires on correct code is a lint somebody deletes. What carries a secret in Rust is a type, and
/// the types are what this list names.
const BANNED: [&[u8]; 15] = [
    b"SigningKey",
    b"SecretKey",
    b"secret_bytes",
    b"secret_key",
    b"private_key",
    b"private_bytes",
    b"expose_secret",
    b"as_secret",
    b"to_secret",
    b"seed_bytes",
    b"std::fs",
    b"std::net",
    b"std::process",
    b"std::env",
    b"std::os",
];

/// Whether `byte` can begin an identifier, so a needle is only tested at a word boundary. Without
/// this the scan is quadratic enough to trip `long_running_const_eval`, which is a real limit on
/// how much a compile-time lint may cost.
const fn is_ident_char(byte: u8) -> bool {
    matches!(byte, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_')
}

/// The first byte of some banned spelling.
///
/// `S` is here for `SigningKey` and `SecretKey`. It costs a handful of extra comparisons on every
/// `Signature`, `SignatureScheme` and `SigningPayload` in the crate, which is cheap, and the
/// alternative — a case-insensitive scan — would fire on the prose in this very file.
const fn is_needle_start(byte: u8) -> bool {
    matches!(byte, b's' | b'p' | b'e' | b'a' | b't' | b'S')
}

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

/// Whether the line `bytes[start..end]` is blank or begins a comment.
const fn is_comment_or_blank(bytes: &[u8], start: usize, end: usize) -> bool {
    let mut at = start;
    while at < end {
        match bytes[at] {
            b' ' | b'\t' | b'\r' => at += 1,
            b'/' => return at + 1 < end && bytes[at + 1] == b'/',
            _ => return false,
        }
    }
    true
}

/// Whether any banned spelling appears on a non-comment line of `source`.
const fn contains_banned(source: &str) -> bool {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut index = 0;

    while index < len {
        let start = index;
        let mut end = index;
        while end < len && bytes[end] != b'\n' {
            end += 1;
        }

        if !is_comment_or_blank(bytes, start, end) {
            let mut at = start;
            while at < end {
                if is_needle_start(bytes[at]) && (at == 0 || !is_ident_char(bytes[at - 1])) {
                    let mut which = 0;
                    while which < BANNED.len() {
                        if starts_with(bytes, at, BANNED[which]) {
                            return true;
                        }
                        which += 1;
                    }
                }
                at += 1;
            }
        }

        index = end + 1;
    }

    false
}

/// Whether every source in the crate is free of both banned families.
const fn every_source_is_clean() -> bool {
    let mut index = 0;
    while index < SOURCES.len() {
        let (_, source) = SOURCES[index];
        if contains_banned(source) {
            return false;
        }
        index += 1;
    }
    true
}

const _: () = assert!(
    every_source_is_clean(),
    "mesh-crypto grew either an accessor that returns private key material or ambient filesystem, \
     network or environment access. Neither belongs here: a secret is reached only through \
     KeyCustody, which signs and never exports, and this crate reads nothing outside its own \
     process. If a backend genuinely needs one of these, it belongs in the backend crate, behind \
     the custody trait."
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_real_sources_are_clean() {
        assert!(every_source_is_clean());
    }

    /// The compile-time guard cannot be observed failing without failing the build, so the
    /// function under it is exercised directly against the shapes a lane would actually write.
    #[test]
    fn the_scan_rejects_what_it_claims_to() {
        for source in [
            "pub fn secret_bytes(&self) -> [u8; 32] { self.0 }",
            "    let raw = key.expose_secret();",
            "use std::fs::File;",
            "let signing = ed25519_dalek::SigningKey::from_bytes(seed);",
            "struct Holder { current: SecretKey }",
            "std::env::var(\"MESH_APPROVAL_KEY\")",
            "let s = std::net::TcpStream::connect(addr);",
        ] {
            assert!(contains_banned(source), "the scan missed: {source}");
        }
    }

    #[test]
    fn the_scan_ignores_comments_so_the_rule_can_be_written_down() {
        for source in [
            "// there is no secret_bytes here, and that is the point",
            "//! a SigningKey belongs in a custody backend, never here",
            "//! `std::fs` is banned in this crate",
            "    /// no expose_secret, ever",
        ] {
            assert!(
                !contains_banned(source),
                "the scan fired on a comment: {source}"
            );
        }
    }

    /// `text` with a leading visibility removed, so `pub mod`, `pub(crate) mod`, `pub(in a::b) mod`
    /// and a bare `mod` become one case instead of four.
    ///
    /// The predecessor of this function did not exist: the coverage guard matched the literal prefix
    /// `"mod "`, so a single `pub` keyword took a module out of coverage entirely — it was neither
    /// scanned nor required to be listed. That is the second half of TASK-214.
    fn without_visibility(text: &str) -> &str {
        let Some(rest) = text.strip_prefix("pub") else {
            return text;
        };
        let rest = match rest.strip_prefix('(') {
            Some(restricted) => match restricted.find(')') {
                Some(close) => &restricted[close + 1..],
                // Unbalanced: not a visibility this reader understands, so hand back the original
                // and let the declaration miss. A `#[path]` and a malformed line are both refused
                // by the guard below rather than parsed here.
                None => return text,
            },
            None => rest,
        };
        // `pubfoo` is an identifier, not a visibility.
        if rest.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
            return text;
        }
        rest.trim_start()
    }

    /// Every module `source` declares that lives in **its own file**.
    ///
    /// A module with a body — `pub mod conformance {` in `lib.rs` — is deliberately not returned:
    /// its text is inside the file being read, which is already in [`SOURCES`], so requiring a
    /// separate entry for it would demand a file that does not exist.
    fn file_modules_declared_in(source: &str) -> Vec<&str> {
        source
            .lines()
            .map(str::trim)
            .filter(|line| !line.starts_with("//"))
            .filter_map(|line| without_visibility(line).strip_prefix("mod "))
            .filter_map(|rest| rest.trim().strip_suffix(';'))
            .map(str::trim)
            .filter(|name| {
                !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            })
            .collect()
    }

    /// Every source file must be listed. A module the scan does not read is a module the rule does
    /// not cover, and adding one is exactly how that happens.
    ///
    /// Three things are asserted, because the module declaration is reachable three ways. A file
    /// module of the crate root must be in [`SOURCES`], at any visibility. No *other* source may
    /// declare a file module at all: `mod evil;` inside `capability.rs` resolves to
    /// `src/capability/evil.rs`, which no entry in [`SOURCES`] has the shape to name, so the honest
    /// answer is to refuse it and make whoever needs one extend this list on purpose. And no source
    /// may carry a `#[path]` attribute, which severs a module's name from the file it reads.
    #[test]
    fn every_module_declared_in_the_crate_is_scanned() {
        for name in file_modules_declared_in(include_str!("lib.rs")) {
            if name == "no_secret_material" {
                continue;
            }
            let file = format!("{name}.rs");
            assert!(
                SOURCES.iter().any(|(listed, _)| *listed == file),
                "{file} is a module of this crate and the scan does not read it"
            );
        }

        for (name, source) in SOURCES {
            if name == "lib.rs" {
                continue;
            }
            assert_eq!(
                file_modules_declared_in(source),
                Vec::<&str>::new(),
                "{name} declares a submodule; it would live under src/{}/ and this scan has no \
                 entry shape for that path",
                name.trim_end_matches(".rs")
            );
            // Space-insensitive, because `# [ path = "…" ]` is the same attribute.
            assert!(
                !flattened(source).replace(' ', "").contains("#[path"),
                "{name} carries a #[path] attribute: a module's name no longer tells this scan \
                 which file it reads"
            );
        }
    }

    /// The declaration spellings that walked past the coverage guard, and the one that must not
    /// trip it.
    ///
    /// Watched failing against the guard as merged: with `strip_prefix("mod ")` in place of
    /// [`without_visibility`], every `pub`-prefixed case here returns nothing.
    #[test]
    fn the_coverage_guard_sees_a_module_at_every_visibility() {
        let mut wrong = Vec::new();
        for (source, expected) in [
            ("mod backdoor;", vec!["backdoor"]),
            ("pub mod backdoor;", vec!["backdoor"]),
            ("pub(crate) mod backdoor;", vec!["backdoor"]),
            ("pub(super) mod backdoor;", vec!["backdoor"]),
            ("pub(in crate::keys) mod backdoor;", vec!["backdoor"]),
            ("    pub mod backdoor;", vec!["backdoor"]),
            ("pub mod a;\nmod b;\npub(crate) mod c;", vec!["a", "b", "c"]),
            // A module with a body has no file of its own, and neither of these declares one.
            ("pub mod conformance {", vec![]),
            ("#[cfg(test)]\nmod tests {", vec![]),
            ("// pub mod backdoor;", vec![]),
            ("pub use crate::keys::ActorKey;", vec![]),
            ("pubmod backdoor;", vec![]),
        ] {
            let seen = file_modules_declared_in(source);
            if seen != expected {
                wrong.push(format!(
                    "{source:?}: read {seen:?}, should read {expected:?}"
                ));
            }
        }
        assert_eq!(
            wrong,
            Vec::<String>::new(),
            "the coverage guard misread {} declarations",
            wrong.len()
        );
    }

    /// TG-3 restated as a property of the source text rather than of the values. `DelegatedAction`
    /// is the vocabulary an agent's capability draws from; a canonical-advance variant appearing in
    /// it would make the whole tier separation decorative.
    #[test]
    fn the_delegable_action_vocabulary_names_nothing_canonical() {
        let source = include_str!("capability.rs");
        let start = source
            .find("pub enum DelegatedAction {")
            .expect("DelegatedAction is declared in capability.rs");
        let body = &source[start..];
        let end = body.find("\n}").expect("the enum closes at column zero");
        let body = &body[..end];

        for line in body.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") {
                continue;
            }
            assert!(
                !trimmed.to_ascii_lowercase().contains("canonical"),
                "a delegable action names canonical state: {trimmed}"
            );
        }
    }

    /// This crate ships no key custody. An implementation here would be an in-process software key
    /// store, which is the single most attractive thing in the programme to attack.
    ///
    /// Checked by the same flattening as the scheme guard, so a path-qualified or line-broken
    /// `impl crate::KeyCustody for …` is caught rather than skipped.
    #[test]
    fn the_crate_ships_no_custody_implementation() {
        for trait_name in ["KeyCustody", "HumanKeyCustody"] {
            assert_eq!(
                implementors_of(trait_name),
                Vec::<&str>::new(),
                "a source in this crate implements {trait_name}"
            );
        }
    }

    /// Non-comment source text with every whitespace run collapsed to one space.
    ///
    /// The collapse is what makes the scan resistant to the spellings that walked past its
    /// predecessor: `impl crate::scheme::SignatureScheme for X` and a header broken across lines
    /// both become the literal `SignatureScheme for`, and `impl<T> SignatureScheme for T` does too.
    /// A `use … as Alias` rename still evades it — this is a lint, not a proof, and the structural
    /// control is that `SignatureScheme::verify` returns `Result` and has no default body.
    fn flattened(source: &str) -> String {
        let mut out = String::with_capacity(source.len());
        for line in source.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") {
                continue;
            }
            for word in trimmed.split_whitespace() {
                out.push_str(word);
                out.push(' ');
            }
        }
        out
    }

    /// Whether `source` implements `needle`, decided on the impl's **target trait** rather than on
    /// the prefix of a line.
    ///
    /// Taking the text as an argument is the point: it is what lets the spellings that evaded the
    /// predecessor of this guard be exercised as literals in a test, instead of being argued about.
    fn implements(source: &str, needle: &str) -> bool {
        flattened(source).contains(&format!("{needle} for "))
    }

    /// Which sources implement `needle`, by the flattened `<Trait> for` spelling.
    fn implementors_of(needle: &str) -> Vec<&'static str> {
        SOURCES
            .iter()
            .filter(|(_, source)| implements(source, needle))
            .map(|(name, _)| *name)
            .collect()
    }

    /// The impl headers that walked past the line-prefix guard, one literal each.
    ///
    /// Watched failing against the guard as merged, whose test was
    /// `trimmed.starts_with("impl SignatureScheme")`: every case here except the last is a header
    /// that decision returns `false` for, and each one ships an always-accept backend or an
    /// in-process software key store in a crate whose doc comments say it has neither.
    #[test]
    fn the_implementation_guard_catches_every_header_that_evaded_a_line_prefix() {
        let mut missed = Vec::new();
        for source in [
            "impl crate::SignatureScheme for StubScheme {",
            "impl crate::scheme::SignatureScheme for StubScheme {",
            "impl\n    SignatureScheme\nfor StubScheme\n{",
            "impl<T: Sized> SignatureScheme for T {",
            "impl<T> crate::SignatureScheme\n    for T\n{",
            "    impl SignatureScheme for StubScheme {",
            "impl SignatureScheme for StubScheme {",
        ] {
            if !implements(source, "SignatureScheme") {
                missed.push(source.to_owned());
            }
        }

        for trait_name in ["KeyCustody", "HumanKeyCustody"] {
            for source in [
                format!("impl crate::{trait_name} for SoftwareStore {{"),
                format!("impl crate::custody::{trait_name} for SoftwareStore {{"),
                format!("impl\n    {trait_name}\nfor SoftwareStore\n{{"),
                format!("impl<T: Send> {trait_name} for T {{"),
            ] {
                if !implements(&source, trait_name) {
                    missed.push(source);
                }
            }
        }

        // Every miss, not the first: a guard reported one spelling at a time is a guard repaired
        // one spelling at a time, which is how this file came to have two decision procedures.
        assert_eq!(
            missed,
            Vec::<String>::new(),
            "{} impl headers walked past the guard",
            missed.len()
        );
    }

    /// The guard has to stay quiet on the declarations and the prose, or it gets deleted.
    #[test]
    fn the_implementation_guard_leaves_declarations_and_bounds_alone() {
        for source in [
            "pub trait SignatureScheme {",
            "// an impl SignatureScheme for a second backend belongs in its own crate",
            "/// a KeyCustody for a smartcard lives behind the trait, not here",
            "pub fn check<S: SignatureScheme>(key: &ActorKey) -> bool {",
            "where C: KeyCustody,",
        ] {
            for trait_name in ["SignatureScheme", "KeyCustody"] {
                assert!(
                    !implements(source, trait_name),
                    "the guard fired on correct source: {source:?}"
                );
            }
        }
    }

    /// A rename defeats every text guard in this file at once, so the rename itself is refused.
    ///
    /// `use crate::SignatureScheme as S;` followed by `impl S for X` leaves no occurrence of
    /// `SignatureScheme for` anywhere in the file, and the impl guard above is looking for exactly
    /// that. Refusing the alias raises the cost of the one spelling; it does not remove the class,
    /// and the module docs say so rather than implying otherwise.
    #[test]
    fn no_source_renames_a_guarded_trait() {
        for (name, source) in SOURCES {
            let text = flattened(source);
            for trait_name in ["SignatureScheme", "KeyCustody", "HumanKeyCustody"] {
                assert!(
                    !text.contains(&format!("{trait_name} as ")),
                    "{name} renames {trait_name}; every guard in this file looks for the name"
                );
            }
        }

        // The shape being refused, so the assertion above is not vacuous.
        let evasion = flattened("use crate::SignatureScheme as S;\nimpl S for StubScheme {");
        assert!(!implements(&evasion, "SignatureScheme"));
        assert!(evasion.contains("SignatureScheme as "));
    }

    /// Exactly one `SignatureScheme`, in exactly one file.
    ///
    /// Both halves are the assertion. A *second* implementation is how a permissive stub arrives —
    /// beside the real one, named `TestScheme`, reachable from production code because everything in
    /// `src/` is. A *zero*th is how the real one leaves. ADR-0009 refuses a hand-rolled Ed25519 and a
    /// stub by name; this is the test that notices either.
    #[test]
    fn exactly_one_source_implements_a_signature_scheme_and_it_is_the_audited_adapter() {
        assert_eq!(
            implementors_of("SignatureScheme"),
            vec![SIGNATURE_SCHEME_HOME],
            "the set of files implementing SignatureScheme is not exactly [{SIGNATURE_SCHEME_HOME}]"
        );
    }

    /// `SignatureScheme::verify` must have **no default body**.
    ///
    /// A trait method with a default is a method every future implementor gets for free without
    /// writing it, and the free version of "check this signature" that somebody reaches for under
    /// deadline is `Ok(())`. The absence of a body is what makes an implementor state, in their own
    /// source, what their check is. Asserted on the text because there is no way to ask the compiler
    /// whether a method has a default.
    #[test]
    fn the_signature_seam_gives_no_implementor_a_free_answer() {
        let declaration = flattened(include_str!("scheme.rs"));
        let at = declaration
            .find("fn verify(")
            .expect("SignatureScheme declares verify");
        let tail = &declaration[at..];
        let ends = tail
            .find("-> Result<(), VerifyError>")
            .expect("verify returns Result<(), VerifyError>");
        let after = tail[ends..].trim_start_matches("-> Result<(), VerifyError>");
        assert!(
            after.trim_start().starts_with(';'),
            "SignatureScheme::verify grew a default body: a future backend now verifies by \
             inheriting one, and the inherited answer is whatever this default says"
        );
        assert!(
            !declaration.contains("impl<") && !declaration.contains("impl <"),
            "scheme.rs grew a blanket implementation"
        );
    }

    /// The audited adapter has to stay an adapter, and it has to stay the strict one.
    ///
    /// `verify_strict` is what rejects a low-order public key, a low-order commitment `R`, and
    /// verifies cofactorlessly, so that two Mesh peers cannot disagree about the same bytes.
    /// `VerifyingKey::verify` is the permissive sibling, one word away, and swapping them is the
    /// edit somebody makes to close an interoperability complaint. It has to fail a test.
    #[test]
    fn the_audited_adapter_delegates_and_stays_strict() {
        let source = include_str!("ed25519.rs");
        let text = flattened(source);
        assert!(
            text.contains("ed25519_dalek::"),
            "the adapter no longer names the audited implementation it is supposed to delegate to"
        );
        assert!(
            text.contains("verify_strict("),
            "the adapter no longer calls verify_strict; the permissive check accepts low-order keys"
        );
        assert!(
            text.contains("is_weak()"),
            "the adapter no longer rejects a low-order public key at its own boundary"
        );
        assert!(
            text.contains("scalar_is_canonical("),
            "the adapter no longer checks S < L itself; malleability now depends on a feature flag"
        );
    }

    /// The banned-identifier scan reads the adapter too, and the adapter is where a secret would
    /// first be plausible — it is the only file that imports a cryptographic library.
    #[test]
    fn the_audited_adapter_is_covered_by_the_compile_time_scan() {
        assert!(
            SOURCES
                .iter()
                .any(|(name, _)| *name == SIGNATURE_SCHEME_HOME),
            "{SIGNATURE_SCHEME_HOME} is not scanned"
        );
        assert!(!contains_banned(include_str!("ed25519.rs")));
    }
}
