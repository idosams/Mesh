//! When two directory entry names are the same name, and on which volume.
//!
//! # The one sentence this module is built around
//!
//! **A name is stored as the bytes the author typed, and folding happens only in the index that
//! answers "would this volume hold these two as one entry?".** Nothing here rewrites a name.
//! A rename nobody asked for is data loss with a friendly face: the author's `README.md` comes
//! back as `readme.md`, no operation records the change, and no conflict is raised because the
//! system believes it did the right thing.
//!
//! # Four fold families, and why not two
//!
//! | Family | Two names are one entry when | The volumes that apply it |
//! |---|---|---|
//! | [`NameFold::Exact`] | the bytes are equal | every volume |
//! | [`NameFold::Canonical`] | their NFD forms are equal | APFS, both variants |
//! | [`NameFold::Upcase`] | their simple uppercase mappings are equal | NTFS |
//! | [`NameFold::Caseless`] | their canonical caseless forms are equal | case-insensitive APFS, HFS+ |
//!
//! The obvious model — one "case-sensitive?" flag and one "normalization-sensitive?" flag — is
//! **wrong**, and two real pairs of characters prove it. It was written first here, and these are
//! the two rows of `tests/name-corpus.txt` that broke it:
//!
//! * **U+0131 dotless ı against `i`.** Simple uppercase sends both to `I`, so NTFS holds them as
//!   one entry. Case folding sends `ı` to itself, so a macOS volume holds them as two. A single
//!   "these differ only in case" answer has to be wrong about one of the two.
//! * **U+1FBE prosgegrammeni against U+03B9 iota.** They are canonically equivalent *and* they
//!   share an uppercase mapping, so both the Apple volumes and NTFS fold them — while the pair
//!   differs by normalization, which NTFS is supposed to preserve.
//!
//! So a [`NameRelation`] is a **set** of families rather than one verdict, and a volume names the
//! one family it applies. `Exact` implies all three others and `Canonical` implies `Caseless`;
//! `Upcase` implies nothing and nothing implies it.
//!
//! # Where the tables come from
//!
//! `unicode-fold.txt`, generated from the Unicode Character Database: canonical combining class,
//! full canonical decomposition, simple case folding and simple uppercase, each carrying only the
//! code points where the mapping differs from the code point itself. Hangul syllables are absent
//! on purpose — their decomposition is arithmetic (UAX #15 §16), and 11172 hand-carried rows would
//! be 11172 chances to mistype one.
//!
//! # What this is not
//!
//! **Not NFC.** Nothing here composes. Every key is a decomposed form, because the question is
//! only ever equality of keys and composition would add a second table for no gain.
//!
//! **Not compatibility folding.** `ﬁle.txt` and `file.txt` are joined by nothing, and neither are
//! the mathematical alphanumerics. NFKD equates strings that no filesystem equates, so folding
//! them here would invent collisions that no platform has.
//!
//! **Not full case folding.** The table carries *simple* folding — `CaseFolding.txt` status C and
//! S, one code point in, one code point out. Full folding maps `ß` to `ss`, and no filesystem
//! does: `straße.txt` and `strasse.txt` are two files on NTFS, on APFS and on HFS+.
//!
//! **Not Windows' `UpcaseTable` itself.** NTFS folds case with a table stored in the volume,
//! fixed at format time, built from the same simple uppercase mapping. The two agree on every
//! script in common use and can disagree on characters added to Unicode after the volume was
//! formatted. That residual is real, is not closed here, and is stated so that a lane meeting it
//! knows it was known.
//!
//! # This file exists twice
//!
//! `crates/mesh-state/src/fold.rs` and `crates/mesh-conflicts/src/fold.rs` are byte-identical, as
//! are the two copies of `unicode-fold.txt` beside them. No crate in this workspace declares a
//! dependency on another — a dependency edge rewrites `Cargo.lock`, which the repository treats as
//! governance surface a lane escalates rather than writes
//! (`docs/adr/0008-ship-the-sqlite-schema-without-a-sqlite-driver.md`) — and a name register that
//! folds differently from the conflict engine detecting collisions in it would be worse than
//! either alone: one would report a collision the other had already resolved away.
//!
//! `crates/mesh-conflicts/tests/mesh_state_drift.rs` compares the copies byte for byte and fails
//! if they diverge, which is why the duplication is safe to have and cheap to remove the day the
//! dependency edge is allowed.

use core::fmt;
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// The generated fold table.
const TABLE: &str = include_str!("unicode-fold.txt");

/// The Unicode version the table was generated from.
///
/// A fold rule is only reproducible against a stated version: two peers running different tables
/// would disagree about whether two names collide, which is the one thing this module exists to
/// make them agree on.
pub const UNICODE_VERSION: &str = "15.0.0";

/// One way a volume can decide that two names are one directory entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NameFold {
    /// The bytes are equal. Every volume folds this much and no volume folds less.
    Exact,
    /// The canonical decompositions are equal. APFS, on both its case variants.
    Canonical,
    /// The simple uppercase mappings are equal. NTFS, whose volume table is built from them.
    Upcase,
    /// The canonical caseless forms are equal. Case-insensitive APFS and HFS+.
    Caseless,
}

impl NameFold {
    /// Every family, so a caller can iterate rather than remember.
    pub const EVERY: [Self; 4] = [Self::Exact, Self::Canonical, Self::Upcase, Self::Caseless];

    /// The key two names share exactly when this family joins them.
    #[must_use]
    pub fn key(self, name: &str) -> String {
        match self {
            Self::Exact => name.to_owned(),
            Self::Canonical => canonical_key(name),
            Self::Upcase => upcase_key(name),
            Self::Caseless => caseless_key(name),
        }
    }
}

impl fmt::Display for NameFold {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Exact => "exact",
            Self::Canonical => "canonical",
            Self::Upcase => "upcase",
            Self::Caseless => "caseless",
        })
    }
}

/// Which fold families make two names one directory entry.
///
/// A set rather than a verdict, for the reason the module documentation gives. The empty set means
/// two different names on every volume mesh supports.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NameRelation(u8);

impl NameRelation {
    /// Two names no fold joins.
    pub const DISTINCT: Self = Self(0);

    /// The bit for one family.
    const fn bit(fold: NameFold) -> u8 {
        1 << fold as u8
    }

    /// Whether this family joins the pair.
    #[must_use]
    pub const fn joined_by(self, fold: NameFold) -> bool {
        self.0 & Self::bit(fold) != 0
    }

    /// Whether the two names are the same bytes.
    #[must_use]
    pub const fn is_identical(self) -> bool {
        self.joined_by(NameFold::Exact)
    }

    /// Whether no volume mesh supports would hold the two as one entry.
    #[must_use]
    pub const fn is_distinct(self) -> bool {
        self.0 == 0
    }

    /// Every family that joins the pair, in declaration order.
    #[must_use]
    pub fn families(self) -> Vec<NameFold> {
        NameFold::EVERY
            .into_iter()
            .filter(|fold| self.joined_by(*fold))
            .collect()
    }

    /// This relation with one more family in it.
    ///
    /// For a caller building a relation from a written answer — a corpus row, a fixture — rather
    /// than from two names. [`relate`] is how one is built from names.
    #[must_use]
    pub const fn with(self, fold: NameFold) -> Self {
        Self(self.0 | Self::bit(fold))
    }

    /// The families that join **both** pairs.
    ///
    /// The intersection, which is what a group of more than two names answers with: the group is
    /// one entry on a volume exactly when that volume's family joins every member.
    #[must_use]
    pub const fn intersect(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// The relation holding every family, which is what a name has with itself.
    const fn everything() -> Self {
        Self(
            Self::bit(NameFold::Exact)
                | Self::bit(NameFold::Canonical)
                | Self::bit(NameFold::Upcase)
                | Self::bit(NameFold::Caseless),
        )
    }
}

impl fmt::Debug for NameRelation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_distinct() {
            return formatter.write_str("none");
        }
        let mut first = true;
        for fold in self.families() {
            if !first {
                formatter.write_str(",")?;
            }
            write!(formatter, "{fold}")?;
            first = false;
        }
        Ok(())
    }
}

impl fmt::Display for NameRelation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, formatter)
    }
}

/// Which fold families join these two names.
///
/// Reflexive, symmetric, and a pure function of the two strings.
#[must_use]
pub fn relate(left: &str, right: &str) -> NameRelation {
    if left == right {
        return NameRelation::everything();
    }
    let mut relation = 0;
    if canonical_key(left) == canonical_key(right) {
        relation |= NameRelation::bit(NameFold::Canonical);
    }
    if upcase_key(left) == upcase_key(right) {
        relation |= NameRelation::bit(NameFold::Upcase);
    }
    if caseless_key(left) == caseless_key(right) {
        relation |= NameRelation::bit(NameFold::Caseless);
    }
    NameRelation(relation)
}

/// The canonical decomposition of `name`, canonically ordered — NFD.
///
/// Two names with one canonical key are one name to APFS.
#[must_use]
pub fn canonical_key(name: &str) -> String {
    let mut chars = Vec::with_capacity(name.len());
    for character in name.chars() {
        decompose_into(character, &mut chars);
    }
    canonically_order(&mut chars);
    chars.into_iter().collect()
}

/// The simple uppercase mapping of `name`, with no normalization at all.
///
/// Two names with one uppercase key are one name to NTFS, which folds case with a table built
/// from this mapping and preserves the code points it was handed.
#[must_use]
pub fn upcase_key(name: &str) -> String {
    let mut chars = Vec::with_capacity(name.len());
    for character in name.chars() {
        map_into(&tables().upcase, character, &mut chars);
    }
    chars.into_iter().collect()
}

/// The simple case folding of `name`, with no normalization at all.
///
/// Exposed because the canonical caseless form is built from it and a caller comparing against
/// another implementation needs the intermediate. No volume mesh models applies this alone.
#[must_use]
pub fn case_key(name: &str) -> String {
    let mut chars = Vec::with_capacity(name.len());
    for character in name.chars() {
        map_into(&tables().case_fold, character, &mut chars);
    }
    chars.into_iter().collect()
}

/// The canonical caseless form of `name` — `NFD(simple_fold(NFD(name)))`.
///
/// Two names with one caseless key are one name to a case-insensitive Apple volume, which folds
/// case after normalizing.
#[must_use]
pub fn caseless_key(name: &str) -> String {
    let mut decomposed = Vec::with_capacity(name.len());
    for character in name.chars() {
        decompose_into(character, &mut decomposed);
    }
    canonically_order(&mut decomposed);

    let mut folded = Vec::with_capacity(decomposed.len());
    for character in decomposed {
        map_into(&tables().case_fold, character, &mut folded);
    }

    let mut settled = Vec::with_capacity(folded.len());
    for character in folded {
        decompose_into(character, &mut settled);
    }
    canonically_order(&mut settled);
    settled.into_iter().collect()
}

/// The canonical combining class of `character`, zero for everything that is not a combining mark.
#[must_use]
pub fn combining_class(character: char) -> u8 {
    tables()
        .combining
        .get(&(character as u32))
        .copied()
        .unwrap_or(0)
}

/// Append the full canonical decomposition of `character` to `out`.
fn decompose_into(character: char, out: &mut Vec<char>) {
    if push_hangul_jamo(character as u32, out) {
        return;
    }
    match tables().decomposition.get(&(character as u32)) {
        Some(sequence) => out.extend_from_slice(sequence),
        None => out.push(character),
    }
}

/// Append `character`'s entry in `table` to `out`, or the character when it has none.
fn map_into(table: &BTreeMap<u32, Box<[char]>>, character: char, out: &mut Vec<char>) {
    match table.get(&(character as u32)) {
        Some(sequence) => out.extend_from_slice(sequence),
        None => out.push(character),
    }
}

/// Reorder combining marks by canonical combining class, in place.
///
/// A stable insertion pass, which is what UAX #15 specifies: marks of one class keep the order
/// they were written in, because for two marks of one class the order is meaningful. `a` with an
/// acute then a grave is a different string from `a` with a grave then an acute — both marks sit
/// above the letter, and which one is nearer is the difference.
fn canonically_order(chars: &mut [char]) {
    let mut at = 1;
    while at < chars.len() {
        let here = combining_class(chars[at]);
        let before = combining_class(chars[at - 1]);
        if here != 0 && before > here {
            chars.swap(at - 1, at);
            at = if at > 1 { at - 1 } else { 1 };
        } else {
            at += 1;
        }
    }
}

/// The first code point of a Hangul syllable block.
const HANGUL_SYLLABLE_BASE: u32 = 0xAC00;
/// The first leading jamo.
const HANGUL_LEAD_BASE: u32 = 0x1100;
/// The first vowel jamo.
const HANGUL_VOWEL_BASE: u32 = 0x1161;
/// One below the first trailing jamo, so that a trail index of zero means "no trailing jamo".
const HANGUL_TRAIL_BASE: u32 = 0x11A7;
/// How many vowel jamo there are.
const HANGUL_VOWEL_COUNT: u32 = 21;
/// How many trailing jamo there are, counting the absent one.
const HANGUL_TRAIL_COUNT: u32 = 28;
/// How many syllables one leading jamo spans.
const HANGUL_BLOCK: u32 = HANGUL_VOWEL_COUNT * HANGUL_TRAIL_COUNT;
/// How many precomposed Hangul syllables there are.
const HANGUL_SYLLABLE_COUNT: u32 = 19 * HANGUL_BLOCK;

/// Decompose a precomposed Hangul syllable arithmetically, returning whether it was one.
fn push_hangul_jamo(code_point: u32, out: &mut Vec<char>) -> bool {
    let Some(index) = code_point.checked_sub(HANGUL_SYLLABLE_BASE) else {
        return false;
    };
    if index >= HANGUL_SYLLABLE_COUNT {
        return false;
    }
    out.push(code_point_of(HANGUL_LEAD_BASE + index / HANGUL_BLOCK));
    out.push(code_point_of(
        HANGUL_VOWEL_BASE + (index % HANGUL_BLOCK) / HANGUL_TRAIL_COUNT,
    ));
    let trail = index % HANGUL_TRAIL_COUNT;
    if trail != 0 {
        out.push(code_point_of(HANGUL_TRAIL_BASE + trail));
    }
    true
}

/// A scalar value from a code point known to be one.
fn code_point_of(code_point: u32) -> char {
    char::from_u32(code_point)
        .expect("every jamo and every table entry is a Unicode scalar value by construction")
}

/// The four parsed sections of `unicode-fold.txt`.
struct Tables {
    /// Canonical combining class, non-zero entries only.
    combining: BTreeMap<u32, u8>,
    /// Full canonical decomposition, where it differs from the code point.
    decomposition: BTreeMap<u32, Box<[char]>>,
    /// Simple case folding, where it differs from the code point.
    case_fold: BTreeMap<u32, Box<[char]>>,
    /// Simple uppercase, where it differs from the code point.
    upcase: BTreeMap<u32, Box<[char]>>,
}

/// Why the embedded table could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
struct TableError {
    /// The one-based line the fault is on.
    line: usize,
    /// What is wrong with it.
    what: &'static str,
}

/// The parsed tables, built once.
fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| {
        parse_tables(TABLE).unwrap_or_else(|error| {
            panic!(
                "unicode-fold.txt is malformed at line {}: {}. It is generated data — regenerate \
                 it rather than editing a row.",
                error.line, error.what
            )
        })
    })
}

/// Parse the four sections.
fn parse_tables(text: &str) -> Result<Tables, TableError> {
    let mut combining = BTreeMap::new();
    let mut decomposition = BTreeMap::new();
    let mut case_fold = BTreeMap::new();
    let mut upcase = BTreeMap::new();
    let mut section = "";

    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        let at = index + 1;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('@') {
            section = match name {
                "ccc" | "nfd" | "casefold" | "upcase" => name,
                _ => {
                    return Err(TableError {
                        line: at,
                        what: "unknown section",
                    })
                }
            };
            continue;
        }
        let mut fields = line.split(' ');
        let key = fields
            .next()
            .and_then(|field| u32::from_str_radix(field, 16).ok())
            .ok_or(TableError {
                line: at,
                what: "the row does not begin with a code point",
            })?;

        match section {
            "ccc" => {
                let class = fields
                    .next()
                    .and_then(|field| field.parse::<u8>().ok())
                    .ok_or(TableError {
                        line: at,
                        what: "no combining class",
                    })?;
                combining.insert(key, class);
            }
            "nfd" | "casefold" | "upcase" => {
                let mut sequence = Vec::new();
                for field in fields {
                    let value = u32::from_str_radix(field, 16)
                        .ok()
                        .and_then(char::from_u32)
                        .ok_or(TableError {
                            line: at,
                            what: "not a Unicode scalar value",
                        })?;
                    sequence.push(value);
                }
                if sequence.is_empty() {
                    return Err(TableError {
                        line: at,
                        what: "an empty mapping",
                    });
                }
                let target = match section {
                    "nfd" => &mut decomposition,
                    "casefold" => &mut case_fold,
                    _ => &mut upcase,
                };
                target.insert(key, sequence.into_boxed_slice());
            }
            _ => {
                return Err(TableError {
                    line: at,
                    what: "a row before any section header",
                })
            }
        }
    }

    Ok(Tables {
        combining,
        decomposition,
        case_fold,
        upcase,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_table_parses_and_is_not_empty() {
        let parsed = parse_tables(TABLE).expect("the embedded table parses");
        assert!(
            parsed.combining.len() > 800,
            "combining classes went missing"
        );
        assert!(
            parsed.decomposition.len() > 2000,
            "decompositions went missing"
        );
        assert!(parsed.case_fold.len() > 1400, "case foldings went missing");
        assert!(
            parsed.upcase.len() > 1300,
            "uppercase mappings went missing"
        );
        assert!(
            parsed.case_fold.values().all(|mapped| mapped.len() == 1),
            "a case folding maps one code point to several, which is FULL folding"
        );
        assert!(
            parsed.upcase.values().all(|mapped| mapped.len() == 1),
            "an uppercase mapping produces several code points, which is FULL uppercasing"
        );
    }

    #[test]
    fn the_table_names_the_unicode_version_it_was_generated_from() {
        assert!(
            TABLE.contains(UNICODE_VERSION),
            "UNICODE_VERSION and the generated header disagree"
        );
    }

    #[test]
    fn a_malformed_row_is_an_error_rather_than_a_wrong_answer() {
        assert!(parse_tables("@nope\n0041 0061\n").is_err());
        assert!(parse_tables("@ccc\nzzzz 230\n").is_err());
        assert!(parse_tables("@ccc\n0300\n").is_err());
        assert!(parse_tables("@nfd\n00C0\n").is_err());
        assert!(parse_tables("@nfd\n00C0 D800\n").is_err());
        assert!(parse_tables("0041 0061\n").is_err());
    }

    #[test]
    fn hangul_decomposes_arithmetically() {
        // U+D55C HANGUL SYLLABLE HAN -> U+1112 U+1161 U+11AB.
        assert_eq!(canonical_key("\u{D55C}"), "\u{1112}\u{1161}\u{11AB}");
        // U+AC00 HANGUL SYLLABLE GA has no trailing jamo.
        assert_eq!(canonical_key("\u{AC00}"), "\u{1100}\u{1161}");
        // The syllable block ends at U+D7A3; U+D7A4 is not a syllable and must survive intact.
        assert_eq!(canonical_key("\u{D7A4}"), "\u{D7A4}");
    }

    #[test]
    fn combining_marks_are_reordered_by_class_and_never_within_a_class() {
        // Cedilla is class 202, acute is 230, so the cedilla sorts first either way round.
        assert_eq!(canonical_key("a\u{0327}\u{0301}"), "a\u{0327}\u{0301}");
        assert_eq!(canonical_key("a\u{0301}\u{0327}"), "a\u{0327}\u{0301}");
        // Two marks of one class keep the order they were written in.
        assert_ne!(
            canonical_key("a\u{0301}\u{0300}"),
            canonical_key("a\u{0300}\u{0301}")
        );
    }

    #[test]
    fn a_name_relates_to_itself_by_every_family() {
        let relation = relate("notes.md", "notes.md");
        assert!(relation.is_identical());
        assert_eq!(relation.families(), NameFold::EVERY.to_vec());
    }

    #[test]
    fn the_everyday_pairs_land_in_the_families_they_belong_to() {
        // NFC against NFD: the Apple volumes fold it, NTFS does not.
        let normalization = relate("caf\u{e9}", "cafe\u{301}");
        assert!(normalization.joined_by(NameFold::Canonical));
        assert!(normalization.joined_by(NameFold::Caseless));
        assert!(!normalization.joined_by(NameFold::Upcase));
        assert!(!normalization.is_identical());

        // Case alone: NTFS and the case-insensitive Apple volumes fold it.
        let case = relate("README", "readme");
        assert!(case.joined_by(NameFold::Upcase));
        assert!(case.joined_by(NameFold::Caseless));
        assert!(!case.joined_by(NameFold::Canonical));

        // Both at once: only a volume that folds case AND normalization holds them as one.
        let both = relate("CAFE\u{301}", "caf\u{e9}");
        assert!(both.joined_by(NameFold::Caseless));
        assert!(!both.joined_by(NameFold::Canonical));
        assert!(!both.joined_by(NameFold::Upcase));

        assert!(relate("README", "LICENSE").is_distinct());
    }

    #[test]
    fn the_two_pairs_that_broke_the_two_flag_model_are_carried_correctly() {
        // Dotless i: NTFS holds it with `i`, a macOS volume does not.
        let dotless = relate("\u{131}", "i");
        assert!(dotless.joined_by(NameFold::Upcase));
        assert!(!dotless.joined_by(NameFold::Caseless));
        assert!(!dotless.joined_by(NameFold::Canonical));

        // Prosgegrammeni against iota: canonically equivalent AND sharing an uppercase mapping.
        let iota = relate("\u{1FBE}", "\u{3B9}");
        assert!(iota.joined_by(NameFold::Canonical));
        assert!(iota.joined_by(NameFold::Upcase));
        assert!(iota.joined_by(NameFold::Caseless));
        assert!(!iota.is_identical());
    }

    #[test]
    fn compatibility_equivalence_is_not_folded() {
        assert!(relate("\u{FB01}le", "file").is_distinct());
        assert!(relate("\u{1D400}", "A").is_distinct());
    }

    #[test]
    fn case_folding_is_simple_rather_than_full() {
        // Full folding would make these one name. No filesystem does.
        assert!(relate("stra\u{df}e", "strasse").is_distinct());
        // The capital sharp s has a simple fold and a simple uppercase, so it does collide.
        assert!(relate("\u{1E9E}", "\u{df}").joined_by(NameFold::Caseless));
    }

    #[test]
    fn an_intersection_keeps_only_what_both_pairs_share() {
        let case = relate("README", "readme");
        let normalization = relate("caf\u{e9}", "cafe\u{301}");
        let shared = case.intersect(normalization);
        assert!(shared.joined_by(NameFold::Caseless));
        assert!(!shared.joined_by(NameFold::Upcase));
        assert!(!shared.joined_by(NameFold::Canonical));
        assert!(NameRelation::DISTINCT.intersect(case).is_distinct());
    }

    #[test]
    fn a_relation_renders_as_the_families_that_join_it() {
        assert_eq!(relate("README", "readme").to_string(), "upcase,caseless");
        assert_eq!(relate("a", "b").to_string(), "none");
        assert_eq!(
            relate("a", "a").to_string(),
            "exact,canonical,upcase,caseless"
        );
    }

    #[test]
    fn every_family_keys_the_way_its_own_function_does() {
        for name in ["README.md", "caf\u{e9}", "\u{131}", "\u{D55C}"] {
            assert_eq!(NameFold::Exact.key(name), name);
            assert_eq!(NameFold::Canonical.key(name), canonical_key(name));
            assert_eq!(NameFold::Upcase.key(name), upcase_key(name));
            assert_eq!(NameFold::Caseless.key(name), caseless_key(name));
        }
    }
}
