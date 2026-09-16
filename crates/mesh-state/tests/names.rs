//! The cross-platform name corpus, and the four properties the fold has to hold.
//!
//! # Why the answers come from outside this repository
//!
//! `tests/name-corpus.txt` was generated from the Unicode Character Database through Python's
//! `unicodedata`. Every row carries the relation the UCD says the pair holds, so a test here is
//! mesh being held against a published standard rather than against the same code that produced
//! the answer. A corpus mesh wrote for itself would agree with mesh about everything, including
//! about its mistakes.
//!
//! # The four properties
//!
//! | Property | Why a violation is a defect rather than a curiosity |
//! |---|---|
//! | Every key function is idempotent | A key that changes when applied twice makes a collision index that depends on how many times it was rebuilt |
//! | The relation is symmetric | `relate(a, b)` and `relate(b, a)` disagreeing means the answer depends on which name arrived first, and arrival order is not a fact about a name |
//! | Nothing rewrites a name | Every function here reads; a name that came back different from the one that went in is a rename nobody performed |
//! | The volume grouping and the relation agree | Two statements of one rule, and the one that under-reports lets the filesystem pick a winner |
//!
//! # What is NOT proved here, stated because a reader will assume otherwise
//!
//! **No filesystem is touched.** These are pure functions over strings, run on whatever host the
//! suite runs on, and they say nothing about what a real APFS or NTFS volume does with the names.
//! The profiles in `src/portable.rs` are a written model of those filesystems; holding the model
//! against the real thing needs a filesystem adapter, and none exists on this tree.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use mesh_state::{
    canonical_key, case_key, caseless_key, is_portable, preflight_directory, relate, restrictions,
    upcase_key, DroppedMetadata, NameFold, NameRelation, NameRestriction, NormalizedName,
    PreservedMetadata, VolumeProfile, RESERVED_DEVICE_NAMES, UNICODE_VERSION,
};

/// One row of the corpus.
struct Pair {
    relation: NameRelation,
    left: String,
    right: String,
    note: String,
}

/// The relation a corpus row's fold list describes.
fn relation_of(field: &str) -> NameRelation {
    let mut relation = NameRelation::DISTINCT;
    for family in field.split(',') {
        let fold = match family {
            "none" => continue,
            "exact" => NameFold::Exact,
            "canonical" => NameFold::Canonical,
            "upcase" => NameFold::Upcase,
            "caseless" => NameFold::Caseless,
            other => panic!("the corpus names an unknown fold family {other:?}"),
        };
        relation = relation.with(fold);
    }
    relation
}

/// Every row of the corpus, decoded from its hexadecimal code points.
fn corpus() -> Vec<Pair> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/name-corpus.txt");
    let text = fs::read_to_string(&path).expect("the corpus is beside this test");
    let mut pairs = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split(';');
        let relation = relation_of(fields.next().expect("a fold list"));
        let left = decode(fields.next().expect("a left name"));
        let right = decode(fields.next().expect("a right name"));
        let note = fields.next().unwrap_or("").to_owned();
        pairs.push(Pair {
            relation,
            left,
            right,
            note,
        });
    }
    assert!(
        pairs.len() > 600,
        "the corpus shrank to {} rows",
        pairs.len()
    );
    pairs
}

/// A name written as space-separated hexadecimal code points.
fn decode(field: &str) -> String {
    field
        .split(' ')
        .filter(|part| !part.is_empty())
        .map(|part| {
            u32::from_str_radix(part, 16)
                .ok()
                .and_then(char::from_u32)
                .unwrap_or_else(|| panic!("the corpus holds {part:?}, which is not a code point"))
        })
        .collect()
}

#[test]
fn every_corpus_pair_relates_the_way_the_character_database_says_it_does() {
    let mut wrong = Vec::new();
    for pair in corpus() {
        let got = relate(&pair.left, &pair.right);
        if got != pair.relation {
            wrong.push(format!(
                "{}: expected {:?}, got {got:?}",
                pair.note, pair.relation
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} rows disagree:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

#[test]
fn the_relation_is_symmetric_and_reflexive() {
    for pair in corpus() {
        assert_eq!(
            relate(&pair.left, &pair.right),
            relate(&pair.right, &pair.left),
            "asymmetric on {}",
            pair.note
        );
        assert!(relate(&pair.left, &pair.left).is_identical());
        assert!(relate(&pair.right, &pair.right).is_identical());
    }
}

#[test]
fn every_key_function_is_idempotent() {
    for pair in corpus() {
        for name in [&pair.left, &pair.right] {
            let canonical = canonical_key(name);
            assert_eq!(
                canonical_key(&canonical),
                canonical,
                "canonical, {}",
                pair.note
            );
            let case = case_key(name);
            assert_eq!(case_key(&case), case, "case, {}", pair.note);
            let caseless = caseless_key(name);
            assert_eq!(caseless_key(&caseless), caseless, "caseless, {}", pair.note);
        }
    }
}

#[test]
fn the_families_nest_the_way_the_corpus_header_says_they_do() {
    // `exact` implies all three; `canonical` implies `caseless`; `upcase` implies NEITHER, and
    // U+0131 against `i` is the row that proves the last clause rather than asserting it.
    for pair in corpus() {
        let relation = relate(&pair.left, &pair.right);
        if relation.is_identical() {
            for fold in NameFold::EVERY {
                assert!(
                    relation.joined_by(fold),
                    "exact did not imply {fold}: {}",
                    pair.note
                );
            }
        }
        if relation.joined_by(NameFold::Canonical) {
            assert!(
                relation.joined_by(NameFold::Caseless),
                "canonical did not imply caseless: {}",
                pair.note
            );
        }
        // And every family's key really is the equality it claims to be.
        assert_eq!(
            relation.joined_by(NameFold::Canonical),
            canonical_key(&pair.left) == canonical_key(&pair.right),
            "{}",
            pair.note
        );
        assert_eq!(
            relation.joined_by(NameFold::Upcase),
            upcase_key(&pair.left) == upcase_key(&pair.right),
            "{}",
            pair.note
        );
        assert_eq!(
            relation.joined_by(NameFold::Caseless),
            caseless_key(&pair.left) == caseless_key(&pair.right),
            "{}",
            pair.note
        );
    }
}

#[test]
fn the_dotless_i_is_the_pair_that_needs_a_set_rather_than_a_verdict() {
    // Load-bearing: it is the reason `NameRelation` is a set. NTFS uppercases both to `I`; a
    // macOS volume folds the dotless one to itself.
    let relation = relate("\u{131}.txt", "i.txt");
    assert!(relation.joined_by(NameFold::Upcase));
    assert!(!relation.joined_by(NameFold::Caseless));
    assert!(
        !preflight_directory(["\u{131}.txt", "i.txt"], &VolumeProfile::NTFS).is_representable()
    );
    assert!(
        preflight_directory(["\u{131}.txt", "i.txt"], &VolumeProfile::APFS_INSENSITIVE)
            .is_representable()
    );
}

#[test]
fn no_function_here_rewrites_a_name() {
    for pair in corpus() {
        for name in [&pair.left, &pair.right] {
            // Every restriction check reads. The proof it reads is that the name it reports is the
            // name it was given, byte for byte.
            for volume in &VolumeProfile::EVERY {
                let report = preflight_directory([name.as_str()], volume);
                for finding in report.findings() {
                    assert_eq!(finding.name(), name.as_str(), "a finding renamed its name");
                }
            }
            // And that the register hands back exactly what it accepted.
            if let Ok(accepted) = NormalizedName::new(name.as_str()) {
                assert_eq!(accepted.as_str(), name.as_str());
                assert_eq!(accepted.byte_len(), name.len());
            }
        }
    }
}

#[test]
fn the_volume_grouping_and_the_relation_never_disagree() {
    // `preflight_directory` groups by the volume's own fold family; `VolumeProfile::folds`
    // answers the same question from the relation. They read one field, and this is the evidence
    // that reading one field is enough.
    for pair in corpus() {
        if pair.left == pair.right {
            continue;
        }
        let relation = relate(&pair.left, &pair.right);
        for volume in &VolumeProfile::EVERY {
            let report = preflight_directory([pair.left.as_str(), pair.right.as_str()], volume);
            let grouped = !report.collisions().is_empty();
            let predicted = volume.folds(relation);
            assert_eq!(
                grouped,
                predicted,
                "{} on {}: grouping says {grouped}, the relation says {predicted}",
                pair.note,
                volume.label()
            );
            if grouped {
                assert_eq!(report.collisions()[0].relation(), relation);
                assert_eq!(report.collisions()[0].names().len(), 2);
            }
        }
    }
}

#[test]
fn a_case_only_difference_is_never_silently_one_entry_on_a_case_insensitive_volume() {
    // The acceptance criterion, stated as narrowly as it can be: the pair is detected, both names
    // are carried, and neither is renamed.
    let report = preflight_directory(["README.md", "readme.md"], &VolumeProfile::APFS_INSENSITIVE);
    assert!(!report.is_representable());
    assert_eq!(report.collisions().len(), 1);
    assert!(report.collisions()[0]
        .relation()
        .joined_by(NameFold::Caseless));
    assert!(!report.collisions()[0].relation().is_identical());
    assert_eq!(
        report.collisions()[0].names(),
        &["README.md".to_owned(), "readme.md".to_owned()]
    );
}

#[test]
fn a_normalization_only_difference_is_never_silently_one_entry_on_an_apple_volume() {
    let report = preflight_directory(
        ["caf\u{e9}.md", "cafe\u{301}.md"],
        &VolumeProfile::APFS_SENSITIVE,
    );
    assert!(!report.is_representable());
    assert!(report.collisions()[0]
        .relation()
        .joined_by(NameFold::Canonical));
    assert!(!report.collisions()[0]
        .relation()
        .joined_by(NameFold::Upcase));
    assert_eq!(
        report.collisions()[0].names(),
        &["caf\u{e9}.md".to_owned(), "cafe\u{301}.md".to_owned()]
    );
}

#[test]
fn the_awkward_windows_names_are_surfaced_before_materialization() {
    let awkward = [
        ("CON", "a device"),
        ("con.txt", "a device with an extension"),
        ("PRN.tar.gz", "a device with two extensions"),
        ("COM1", "a serial port"),
        ("LPT9.md", "a printer port"),
        ("report.", "a trailing dot"),
        ("report ", "a trailing space"),
        ("a:b.md", "a reserved character"),
        ("a\tb.md", "a control character"),
    ];
    for (name, why) in awkward {
        assert!(
            !restrictions(name, &VolumeProfile::NTFS).is_empty(),
            "{name:?} ({why}) reached materialization unflagged"
        );
        assert!(
            !is_portable(name),
            "{name:?} ({why}) is reported as portable"
        );
    }
    // And each of them is a perfectly ordinary Linux name, which is why the check is per volume.
    for (name, _) in awkward {
        if name == "a\tb.md" || name == "a:b.md" {
            assert!(restrictions(name, &VolumeProfile::LINUX).is_empty());
        }
    }
}

#[test]
fn every_reserved_device_name_is_caught_in_every_case_and_with_any_extension() {
    for device in RESERVED_DEVICE_NAMES {
        for name in [
            device.to_owned(),
            device.to_lowercase(),
            format!("{device}.txt"),
            format!("{}.tar.gz", device.to_lowercase()),
        ] {
            let found = restrictions(&name, &VolumeProfile::NTFS);
            assert!(
                found.iter().any(|restriction| matches!(
                    restriction,
                    NameRestriction::ReservedDeviceName { .. }
                )),
                "{name:?} was not recognised as {device}"
            );
        }
    }
}

#[test]
fn the_length_ceilings_are_counted_in_the_unit_each_volume_counts_in() {
    let kanji = "\u{6c34}".repeat(100);
    // 300 bytes, 100 UTF-16 units: too long for ext4, fine for NTFS.
    assert!(
        restrictions(&kanji, &VolumeProfile::LINUX).contains(&NameRestriction::TooManyBytes {
            bytes: 300,
            limit: 255
        })
    );
    assert!(restrictions(&kanji, &VolumeProfile::NTFS).is_empty());

    let ascii = "a".repeat(256);
    // 256 bytes, 256 UTF-16 units: too long for both, by one.
    assert!(!restrictions(&ascii, &VolumeProfile::LINUX).is_empty());
    assert!(!restrictions(&ascii, &VolumeProfile::NTFS).is_empty());
    assert!(restrictions(&"a".repeat(255), &VolumeProfile::LINUX).is_empty());
    assert!(restrictions(&"a".repeat(255), &VolumeProfile::NTFS).is_empty());
}

#[test]
fn the_portable_metadata_set_is_published_whole() {
    // The set mesh keeps and the set it drops, each field with a reason, and no field in both.
    let preserved: BTreeSet<String> = PreservedMetadata::EVERY
        .iter()
        .map(ToString::to_string)
        .collect();
    let dropped: BTreeSet<String> = DroppedMetadata::EVERY
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(preserved.len(), PreservedMetadata::EVERY.len());
    assert_eq!(dropped.len(), DroppedMetadata::EVERY.len());
    assert!(preserved.is_disjoint(&dropped));
    for field in PreservedMetadata::EVERY {
        assert!(field.why().len() > 30, "{field} has a token reason");
    }
    for field in DroppedMetadata::EVERY {
        assert!(field.why().len() > 30, "{field} has a token reason");
    }
    // The two rules the plan fixes rather than leaves to taste.
    assert!(dropped.contains("modification time"));
    assert!(dropped.contains("owner"));
    assert!(preserved.contains("content bytes"));
    assert!(preserved.contains("entry name"));
}

#[test]
fn the_fold_table_names_the_unicode_version_it_came_from() {
    assert_eq!(UNICODE_VERSION, "15.0.0");
    let corpus_header =
        fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/name-corpus.txt"))
            .expect("the corpus is readable");
    assert!(
        corpus_header.contains(UNICODE_VERSION),
        "the corpus and the fold table were generated from different Unicode versions"
    );
}

/// A deterministic generator, so a failure here is reproducible from the seed printed with it.
struct Xorshift(u64);

impl Xorshift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn pick<T: Copy>(&mut self, from: &[T]) -> T {
        from[(self.next() % from.len() as u64) as usize]
    }
}

#[test]
fn the_properties_hold_over_generated_names_too() {
    // The corpus covers the cases somebody thought of. This covers the ones nobody did: names
    // built from the pieces that make folding hard, in combinations no reviewer would write.
    const ALPHABET: [char; 20] = [
        'a', 'A', 'z', 'Z', '.', ' ', '\u{e9}', '\u{c9}', '\u{301}', '\u{327}', '\u{300}',
        '\u{D55C}', '\u{1112}', '\u{1161}', '\u{212B}', '\u{c5}', '\u{30A}', '\u{df}', '\u{1E9E}',
        '\u{6c34}',
    ];
    let mut generator = Xorshift(0x5EED_1234_ABCD_0001);
    for round in 0..2000_u32 {
        let length = 1 + (generator.next() % 8) as usize;
        let left: String = (0..length).map(|_| generator.pick(&ALPHABET)).collect();
        let right: String = if generator.next() % 3 == 0 {
            // A third of the time, produce a name that really is related to the first.
            canonical_key(&left)
        } else {
            (0..length).map(|_| generator.pick(&ALPHABET)).collect()
        };

        // Idempotence.
        for name in [&left, &right] {
            let canonical = canonical_key(name);
            assert_eq!(canonical_key(&canonical), canonical, "round {round}");
            let caseless = caseless_key(name);
            assert_eq!(caseless_key(&caseless), caseless, "round {round}");
            assert_eq!(case_key(&case_key(name)), case_key(name), "round {round}");
        }

        // Symmetry.
        let relation = relate(&left, &right);
        assert_eq!(relation, relate(&right, &left), "round {round}");

        // The relation and the per-volume grouping, again, on names nobody chose.
        for volume in &VolumeProfile::EVERY {
            if left == right {
                continue;
            }
            let report = preflight_directory([left.as_str(), right.as_str()], volume);
            assert_eq!(
                !report.collisions().is_empty(),
                volume.folds(relation),
                "round {round} on {}: {left:?} against {right:?}",
                volume.label()
            );
        }
    }
}
