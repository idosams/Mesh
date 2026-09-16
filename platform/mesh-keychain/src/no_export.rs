//! The **build-time** scan for the accessor this crate must never grow.
//!
//! Two scanners guard the same absence and they are not interchangeable:
//!
//! * **This module is the build-time control.** `const _: () = assert!(…)` below is evaluated
//!   while the crate compiles, so a source that trips it fails `cargo build`, `cargo check`,
//!   `cargo clippy` and every downstream build. There is no test run to skip and no filter that
//!   deselects it.
//! * **`tests/key_isolation.rs::no_public_method_in_this_crate_hands_a_caller_the_private_half`
//!   is the test-time control.** It runs under `cargo nextest`, reports *which* line offends
//!   rather than only that one does, and is exercised against synthetic sources so a scan that
//!   stopped matching is caught. It can be deselected; this one cannot.
//!
//! The same shape as `crates/mesh-crypto/src/no_secret_material.rs`, and needed here for the
//! opposite reason: that crate is scanned because it must never hold a secret, and this one is
//! scanned because it does. The claim [`crate::SoftwareCustody`] makes is that no code path in the
//! workspace exports the scalar, and the way that claim becomes false is a lane in a hurry adding
//! `pub fn secret_bytes(&self)` to close a serialization ticket.
//!
//! **Neither scanner is a proof, and this one is not either.** Both read text. The bypasses below
//! were each measured rather than guessed, but **this list is what has been measured so far and
//! not a proof of completeness** — the three at the end were found by a later pass that probed
//! these same rules, which is evidence that the list grows when somebody looks again:
//!
//! * an alias — `use ed25519_dalek::SigningKey as Inner;` and then `pub fn signer(&self) -> &Inner`
//!   — walks past both, because neither resolves a name;
//! * a macro that assembles the signature, and a re-export through a third crate, likewise;
//! * a `Deref` reached through a type parameter, where no line of this crate names the target;
//! * a `pub(crate) use` group spanning lines, because [`opens_a_public_use`] recognises only the
//!   fully public spelling. A `pub(crate)` re-export is not reachable outside the crate, which is
//!   why it is left rather than chased;
//! * a banned identifier in a spelling this list does not carry case-folded — `SecretBytes` is not
//!   `secret_bytes`, and both scanners compare bytes;
//! * anything a debugger reads out of this process, which is `GUARANTEES.md` §3, not this file;
//! * **a trait method signature.** `pub trait Leak {` opens a header and the `{` on that same
//!   line closes it, so `fn signer(&self) -> &SigningKey;` on the next line is read by neither
//!   scanner. This is the same class as the conversion impls above — an export path not spelled
//!   `pub fn` — and it is **open**, filed as `01KZHNN6VKX5VHC814SMSCMNDB`;
//! * a header wrapped past [`HEADER_WINDOW_LINES`], which closes the window before the return
//!   type is reached. Filed with the same task;
//! * a line whose first non-space bytes are a block comment — `/* */ pub fn signer(…)` — which
//!   [`is_comment_or_blank`] does not classify as a comment and [`opens_with_word`] does not
//!   classify as a header, so no window opens. Filed with the same task.
//!
//! **The build-time scan is not a superset of the test-time one, and the difference was measured.**
//! `pub fn raw(&self) -> [u8; 32] { self.signing.to_bytes() }` — a full export of the seed — returns
//! `contains_banned=false, exports_the_key_type=false` here and is caught only by
//! `tests/key_isolation.rs`, whose `BANNED_IN_A_PUBLIC_SIGNATURE` carries `to_bytes`. This list
//! cannot carry it outright: `software.rs` calls `verifying_key().to_bytes()` on the **public** half
//! and `sign(…).to_bytes()` on a signature, and banning the word would fire on both. So the control
//! that cannot be deselected is the weaker of the two for that one spelling. That is stated rather
//! than papered over, and it is filed rather than fixed in passing.
//! What is **no longer** a bypass, because it was measured to be one and then closed: an item
//! header split across lines. `pub fn f(\n &self,\n) -> &SigningKey {` names the type on a line
//! that opens with `)`, so a rule over single lines could not see it, and it is what `rustfmt`
//! produces from any signature past the width limit — the scan therefore runs over an item
//! **header**, from the opening `pub` / `impl` / `type` to the `{` or `;` that ends it, and not
//! over one line at a time. See [`exports_the_key_type`].
//!
//! The guarantee they back is that `KeyCustody` has no export method and this crate adds none of
//! its own; the limit of that guarantee is written in `software.rs` under "The honest limit".
//!
//! Comment lines are skipped so the rule can be written down, and this file exempts itself,
//! because a scanner for a string necessarily contains that string.

/// Every source in the crate except this one.
const SOURCES: [(&str, &str); 5] = [
    ("lib.rs", include_str!("lib.rs")),
    ("entropy.rs", include_str!("entropy.rs")),
    ("secure_enclave.rs", include_str!("secure_enclave.rs")),
    ("software.rs", include_str!("software.rs")),
    ("support.rs", include_str!("support.rs")),
];

/// The spellings a lane would actually reach for, banned wherever they appear.
///
/// `SigningKey` is deliberately absent from *this* list — unlike next door, this crate's whole job
/// is to hold one, and banning the type it is built on outright would be a lint that fires on
/// correct code such as `use ed25519_dalek::SigningKey;` and `signing: SigningKey`. The rule that
/// covers the type name is [`SIGNING_KEY`] below, which fires only where the name can only mean
/// export.
const BANNED: [&[u8]; 9] = [
    b"secret_bytes",
    b"secret_key",
    b"private_key",
    b"private_bytes",
    b"expose_secret",
    b"as_secret",
    b"to_secret",
    b"seed_bytes",
    b"into_bytes",
];

/// The private half's type, normalised: lowercase, alphanumerics only.
///
/// Matched against a line with its casing folded and every non-alphanumeric character skipped, so
/// `SigningKey`, `signing_key`, `&SigningKey`, `ed25519_dalek::SigningKey` and
/// `Deref<Target = SigningKey>` all reduce to it. `ed25519_dalek::SigningKey::to_bytes` returns
/// the 32-byte seed, so handing out a `&SigningKey` is a full export of the private half — the
/// reference form is not weaker than the owned one.
const SIGNING_KEY: &[u8] = b"signingkey";

const fn is_ident_char(byte: u8) -> bool {
    matches!(byte, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_')
}

const fn is_alphanumeric(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
}

const fn lowercase(byte: u8) -> u8 {
    if byte.is_ascii_uppercase() {
        byte + 32
    } else {
        byte
    }
}

const fn is_needle_start(byte: u8) -> bool {
    matches!(byte, b's' | b'p' | b'e' | b'a' | b't' | b'i')
}

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

/// The first byte of `bytes[start..end]` that is not leading whitespace.
const fn first_non_space(bytes: &[u8], start: usize, end: usize) -> usize {
    let mut at = start;
    while at < end && matches!(bytes[at], b' ' | b'\t') {
        at += 1;
    }
    at
}

/// Whether `word` opens `bytes[start..end]` as a whole word.
const fn opens_with_word(bytes: &[u8], start: usize, end: usize, word: &[u8]) -> bool {
    let at = first_non_space(bytes, start, end);
    if !starts_with(bytes, at, word) || at + word.len() > end {
        return false;
    }
    let after = at + word.len();
    after >= end || !is_ident_char(bytes[after])
}

/// Whether `needle` appears in `bytes[start..end]` once casing and every non-alphanumeric
/// character in the haystack are ignored. `needle` must already be lowercase alphanumerics.
const fn contains_normalised(bytes: &[u8], start: usize, end: usize, needle: &[u8]) -> bool {
    let mut from = start;
    while from < end {
        let mut at = from;
        let mut matched = 0;
        while at < end && matched < needle.len() {
            let byte = bytes[at];
            if is_alphanumeric(byte) {
                if lowercase(byte) != needle[matched] {
                    break;
                }
                matched += 1;
            }
            at += 1;
        }
        if matched == needle.len() {
            return true;
        }
        from += 1;
    }
    false
}

/// Whether a line **opens** one of the three item headers in which the private half's *type name*
/// can only mean export.
///
/// * `pub …` — a public item. Covers `pub fn signer(&self) -> &SigningKey`, `pub(crate) fn`, a
///   `pub signing: SigningKey` field and a `pub use ed25519_dalek::SigningKey` re-export.
/// * `impl …` — a conversion impl. `impl AsRef<SigningKey>`, `impl Borrow<SigningKey>` and
///   `impl Into<SigningKey>` are export paths that are not spelled `pub fn` at all, which is why a
///   rule over function signatures alone cannot see them. The word test is what keeps
///   `implemented: bool` in `support.rs` from tripping it.
/// * `type …` — an associated or alias type. `impl Deref<Target = SigningKey>` is not how Rust
///   spells `Deref`; the real spelling puts `type Target = SigningKey;` on its own line inside an
///   `impl Deref` block whose header never names the type, so without this class the `Deref` export
///   walks past the `impl` rule.
const fn opens_an_item_header(bytes: &[u8], start: usize, end: usize) -> bool {
    opens_with_word(bytes, start, end, b"pub")
        || opens_with_word(bytes, start, end, b"impl")
        || opens_with_word(bytes, start, end, b"type")
}

/// Whether the header opened here is a fully public `use`, whose braces are a group and not a body.
///
/// `pub use ed25519_dalek::{` opens a brace that a `pub fn` header's `{` does not: the item runs on
/// to the `;`. Treating `{` as its terminator would end the window before the group's contents were
/// read, and a multi-line re-export of the private half's type is exactly what `rustfmt` produces
/// from a group of three names. `pub(crate) use` is deliberately not matched — the `(` is not a
/// space, so the second word test fails — because a crate-private re-export is not reachable from
/// outside this crate and so is not an export path.
const fn opens_a_public_use(bytes: &[u8], start: usize, end: usize) -> bool {
    if !opens_with_word(bytes, start, end, b"pub") {
        return false;
    }
    let after = first_non_space(bytes, start, end) + 3;
    after <= end && opens_with_word(bytes, after, end, b"use")
}

/// Whether `byte` appears in `bytes[start..end]`.
const fn contains_byte(bytes: &[u8], start: usize, end: usize, byte: u8) -> bool {
    let mut at = start;
    while at < end {
        if bytes[at] == byte {
            return true;
        }
        at += 1;
    }
    false
}

/// Whether this line ends the item header opened earlier.
///
/// A header runs to the `{` that opens its body or the `;` that ends it. `}` is there for the
/// truncated-source case, where a window that never met either would otherwise run on.
const fn closes_an_item_header(bytes: &[u8], start: usize, end: usize, public_use: bool) -> bool {
    if public_use {
        return contains_byte(bytes, start, end, b';');
    }
    contains_byte(bytes, start, end, b'{')
        || contains_byte(bytes, start, end, b';')
        || contains_byte(bytes, start, end, b'}')
}

/// The most lines an item header may span before the window closes regardless.
///
/// A bound rather than an open window, so a header that never reaches its terminator — which is not
/// legal Rust, but is what a truncated file looks like — cannot drag a whole source into the scan
/// and turn the rule into the outright type ban this crate cannot have.
const HEADER_WINDOW_LINES: usize = 16;

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

/// Whether `source` names the private half's type where the name can only mean export.
///
/// **The unit is the item header, not the line**, and that is the difference between this scan and
/// the one it replaced. `rustfmt` wraps any signature past the width limit, so
/// `pub fn signer(\n    &self,\n) -> &SigningKey {` is not a hypothetical spelling — it is what the
/// formatter produces, and its only line naming the type opens with `)`. A rule over single lines
/// reads that line, finds no `pub`, `impl` or `type`, and passes it. Measured before this change:
/// both scanners returned false for it, and `cargo fmt --check` was satisfied.
///
/// So a header opens at `pub`, `impl` or `type` and stays open until the `{` or `;` that ends it
/// (bounded by [`HEADER_WINDOW_LINES`]), and every line inside it is read for the type name. A
/// body is outside every window, which is what keeps `signing: SigningKey` in `software.rs` and
/// `let signing = SigningKey::from_bytes(&seed)` from firing — the exclusion the rule depends on.
const fn exports_the_key_type(source: &str) -> bool {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut index = 0;
    let mut inside_header = false;
    let mut public_use = false;
    let mut spanned = 0;
    while index < len {
        let start = index;
        let mut end = index;
        while end < len && bytes[end] != b'\n' {
            end += 1;
        }
        if !is_comment_or_blank(bytes, start, end) {
            if !inside_header && opens_an_item_header(bytes, start, end) {
                inside_header = true;
                public_use = opens_a_public_use(bytes, start, end);
                spanned = 0;
            }
            if inside_header {
                if contains_normalised(bytes, start, end, SIGNING_KEY) {
                    return true;
                }
                spanned += 1;
                if closes_an_item_header(bytes, start, end, public_use)
                    || spanned >= HEADER_WINDOW_LINES
                {
                    inside_header = false;
                }
            }
        }
        index = end + 1;
    }
    false
}

const fn every_source_is_clean() -> bool {
    let mut index = 0;
    while index < SOURCES.len() {
        let (_, source) = SOURCES[index];
        if contains_banned(source) || exports_the_key_type(source) {
            return false;
        }
        index += 1;
    }
    true
}

const _: () = assert!(
    every_source_is_clean(),
    "mesh-keychain grew an accessor that hands a caller the private half. There is no legitimate \
     one: custody signs. If a key genuinely has to leave this process it is a protocol change with \
     a threat model, not an accessor."
);

#[cfg(test)]
mod tests {
    use super::*;

    /// Run one of the two line scans over a whole source, the way the const assert does.
    fn offends(source: &str) -> bool {
        contains_banned(source) || exports_the_key_type(source)
    }

    #[test]
    fn the_real_sources_are_clean() {
        assert!(every_source_is_clean());
    }

    #[test]
    fn the_scan_rejects_what_it_claims_to() {
        for source in [
            "pub fn secret_bytes(&self) -> [u8; 32] { self.0 }",
            "    let raw = key.expose_secret();",
            "pub fn into_bytes(self) -> [u8; 32] { self.signing.to_bytes() }",
            "fn as_secret(&self) -> &[u8] { &self.0 }",
            // The four spellings measured as missed before this rule existed. Each names the
            // private half's type in a position where the name can only mean export.
            "pub fn signer(&self) -> &SigningKey {",
            "pub fn signer(&self) -> ed25519_dalek::SigningKey {",
            "impl AsRef<SigningKey> for SoftwareCustody<P> {",
            "impl Deref<Target = SigningKey> for SoftwareCustody<P> {",
            // …and the shapes the four imply once they are written the way Rust actually spells
            // them, which the `pub fn`-prefix filter could not reach either.
            "impl Borrow<SigningKey> for SoftwareCustody<P> {",
            "impl Into<SigningKey> for SoftwareCustody<P> {",
            "    type Target = SigningKey;",
            "pub signing: SigningKey,",
            "pub use ed25519_dalek::SigningKey;",
            "pub(crate) fn signing_key(&self) -> &SigningKey {",
            "pub fn SIGNING_KEY(&self) -> &ed25519_dalek::SigningKey {",
        ] {
            assert!(offends(source), "the scan missed: {source}");
        }
    }

    /// The spelling the widened *line* rule still missed, and the reason the unit is a header.
    ///
    /// Measured on the line-at-a-time rule before this change: every case below returned false from
    /// both scanners, and `cargo fmt --check` accepted all of them, because this is what `rustfmt`
    /// emits for a signature past the width limit rather than something a lane has to contrive.
    #[test]
    fn a_header_split_across_lines_is_not_a_way_out() {
        for source in [
            // `rustfmt` wraps any signature past the width limit. The only line naming the type
            // opens with `)`, so no rule over single lines can see it.
            "pub fn signer_for_the_currently_enrolled_actor(\n    &self,\n) -> &ed25519_dalek::SigningKey {",
            // The same for a conversion impl whose header wraps at the generic parameter.
            "impl<P: KeyPurpose>\n    AsRef<SigningKey> for SoftwareCustody<P>\n{",
            // A re-export group. `{` is not this header's terminator — `;` is — so the group's
            // contents are inside the window.
            "pub use ed25519_dalek::{\n    Signature, SigningKey, VerifyingKey,\n};",
            // A wrapped return type behind a `where` clause.
            "pub fn signer<P>(&self) -> Result<\n    &SigningKey,\n    CustodyError,\n> {",
        ] {
            assert!(offends(source), "the header scan missed: {source}");
        }
    }

    /// A body is outside every window, which is the whole of why the type-name rule is usable.
    ///
    /// If the window did not close at the `{` or `;` that ends a header, the rule would degrade
    /// into the outright ban on `SigningKey` that this crate cannot have — it holds one.
    #[test]
    fn a_window_closes_at_the_end_of_its_header_and_does_not_read_the_body() {
        for source in [
            "pub struct SoftwareCustody<P: KeyPurpose> {\n    signing: SigningKey,\n}",
            "impl<P: KeyPurpose> SoftwareCustody<P> {\n    fn generate() {\n        let signing = SigningKey::from_bytes(&seed);\n    }\n}",
            "pub use crate::support::{\n    reachable_today, KeyStoreSupport, Platform,\n};",
            "pub const CRATE_NAME: &str = \"mesh-keychain\";\nlet signing = SigningKey::from_bytes(&seed);",
        ] {
            assert!(!offends(source), "the header scan fired on correct code: {source}");
        }
    }

    /// The exclusion that makes the type-name rule usable: holding one is this crate's job.
    #[test]
    fn the_type_name_rule_does_not_fire_on_holding_the_key() {
        for source in [
            "use ed25519_dalek::{Signer as _, SigningKey};",
            "    signing: SigningKey,",
            "        let signing = SigningKey::from_bytes(&seed);",
            "impl<P: KeyPurpose> KeyCustody<P> for SoftwareCustody<P> {",
            "    implemented: bool,",
            "    implemented: false,",
        ] {
            assert!(!offends(source), "the scan fired on correct code: {source}");
        }
    }

    #[test]
    fn the_scan_ignores_comments_so_the_rule_can_be_written_down() {
        assert!(!contains_banned(
            "// there is no secret_bytes here, and that is the point"
        ));
        assert!(!exports_the_key_type(
            "// nor a pub fn signer(&self) -> &SigningKey"
        ));
    }

    /// A module the scan does not read is a module the rule does not cover.
    #[test]
    fn every_module_declared_in_lib_rs_is_scanned() {
        let lib = include_str!("lib.rs");
        for line in lib.lines() {
            let trimmed = line.trim();
            let name = trimmed
                .strip_prefix("mod ")
                .or_else(|| trimmed.strip_prefix("pub mod "))
                .and_then(|rest| rest.strip_suffix(';'));
            let Some(name) = name else { continue };
            if name == "no_export" {
                continue;
            }
            let file = format!("{name}.rs");
            assert!(
                SOURCES.iter().any(|(listed, _)| *listed == file),
                "{file} is a module of this crate and the scan does not read it"
            );
        }
    }
}
