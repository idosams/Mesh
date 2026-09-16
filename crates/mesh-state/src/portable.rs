//! Which names a volume can hold, and which it cannot.
//!
//! # The one sentence this module is built around
//!
//! **A name mesh cannot represent on the target volume is refused before materialization, with the
//! rule it broke named — never mangled into something that fits.** Substituting `CON_1.txt` for
//! `CON.txt` looks like a courtesy and is a silent rename: the author's name is gone, no operation
//! records its loss, and the next synchronization sees a rename nobody performed.
//!
//! # The volume is the unit, not the platform
//!
//! macOS ships case-insensitive APFS by default and case-sensitive APFS on request. The same host,
//! the same operating system, two different answers to "are these one entry?". So the parameter
//! here is a [`VolumeProfile`] and the named constants are volumes —
//! [`VolumeProfile::APFS_INSENSITIVE`], [`VolumeProfile::APFS_SENSITIVE`] — never "macOS".
//!
//! A profile is **data**, so a volume mesh has not met yet is a new constant rather than a new
//! branch in a function. That is the seam: [`restrictions`] reads the profile's fields and knows
//! nothing about any particular filesystem.
//!
//! # The four rules that surprise people, and why each is here
//!
//! | Rule | Where it bites |
//! |---|---|
//! | Reserved device names | `CON`, `NUL`, `COM1` and their friends are devices on Windows *whatever extension follows*, so `CON.txt` fails too, in any case |
//! | Trailing dot or space | Windows silently strips both at the API boundary — `report.` is created as `report`, which is a rename that reports success |
//! | Two length limits | ext4 counts 255 **bytes**, NTFS counts 255 **UTF-16 code units**; a 200-character Japanese name passes one and fails the other |
//! | `:` on macOS | HFS's path separator, still translated by the Finder, so a name containing it is displayed as something else |
//!
//! # Which names a volume holds as ONE entry
//!
//! That is the other half of portability and it lives in [`VolumeProfile::entry_fold`]: the one
//! fold family this volume applies. `crate::fold` explains why one family per volume is enough and
//! why a pair of "case-sensitive?" and "normalization-sensitive?" flags is not — the two accessors
//! with those names are kept as derived conveniences, and the field is what decides.
//!
//! # What this module does not do
//!
//! **It does not decide paths.** `MAX_PATH` and per-volume path ceilings are a property of a whole
//! path, not of one entry; [`crate::preflight_directory`] is where a set of entries is judged and a
//! path rule belongs to the adapter that knows the mount point.
//!
//! **It does not sanitize.** There is no function here that returns a "fixed" name, on purpose.
//! The repair for an unrepresentable name is a decision only a person or an explicit policy can
//! make, and offering an automatic one is how it stops being a decision.

use std::fmt;

use crate::fold::{NameFold, NameRelation};

/// A filesystem volume's naming rules, as much of them as mesh needs to refuse early.
///
/// Constructed only through the named constants: a caller inventing a profile would be asserting
/// facts about a filesystem it cannot check, and every field here is a claim about a real volume.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VolumeProfile {
    /// What to call it in a message to a person.
    label: &'static str,
    /// The one fold family this volume applies when it decides two names are one entry.
    entry_fold: NameFold,
    /// The ceiling on one entry name in UTF-8 bytes.
    max_name_bytes: usize,
    /// The ceiling on one entry name in UTF-16 code units.
    max_name_utf16: usize,
    /// Characters the volume refuses or reinterprets, beyond `/` and NUL.
    reserved_characters: &'static str,
    /// Whether the Windows device names are reserved here.
    reserves_device_names: bool,
    /// Whether a trailing dot or space is stripped or refused.
    rejects_trailing_dot_or_space: bool,
    /// Whether C0 control characters are refused.
    rejects_control_characters: bool,
}

impl VolumeProfile {
    /// Linux on ext4, xfs or btrfs: everything but `/` and NUL, 255 bytes.
    pub const LINUX: Self = Self {
        label: "Linux ext4/xfs/btrfs",
        entry_fold: NameFold::Exact,
        max_name_bytes: 255,
        max_name_utf16: usize::MAX,
        reserved_characters: "",
        reserves_device_names: false,
        rejects_trailing_dot_or_space: false,
        rejects_control_characters: false,
    };

    /// APFS as macOS formats it by default: case-insensitive and normalization-insensitive.
    pub const APFS_INSENSITIVE: Self = Self {
        label: "APFS (case-insensitive)",
        entry_fold: NameFold::Caseless,
        max_name_bytes: 255,
        max_name_utf16: usize::MAX,
        reserved_characters: ":",
        reserves_device_names: false,
        rejects_trailing_dot_or_space: false,
        rejects_control_characters: false,
    };

    /// APFS formatted case-sensitive, which still folds canonical form and still hides `:`.
    ///
    /// The two APFS profiles differ in exactly one field. Normalization-insensitivity is a
    /// property of the filesystem rather than of the case option — Apple made APFS
    /// normalization-insensitive and normalization-preserving in macOS 10.13, on both variants —
    /// so a case-sensitive volume still holds `café` typed two ways as one entry.
    pub const APFS_SENSITIVE: Self = Self {
        label: "APFS (case-sensitive)",
        entry_fold: NameFold::Canonical,
        max_name_bytes: 255,
        max_name_utf16: usize::MAX,
        reserved_characters: ":",
        reserves_device_names: false,
        rejects_trailing_dot_or_space: false,
        rejects_control_characters: false,
    };

    /// HFS+, which counts UTF-16 units and stores a decomposed form of every name.
    pub const HFS_PLUS: Self = Self {
        label: "HFS+",
        entry_fold: NameFold::Caseless,
        max_name_bytes: usize::MAX,
        max_name_utf16: 255,
        reserved_characters: ":",
        reserves_device_names: false,
        rejects_trailing_dot_or_space: false,
        rejects_control_characters: false,
    };

    /// NTFS through the Win32 API, which is where the device names and the stripping live.
    pub const NTFS: Self = Self {
        label: "NTFS via Win32",
        entry_fold: NameFold::Upcase,
        max_name_bytes: usize::MAX,
        max_name_utf16: 255,
        reserved_characters: "<>:\"\\|?*",
        reserves_device_names: true,
        rejects_trailing_dot_or_space: true,
        rejects_control_characters: true,
    };

    /// Every volume mesh has rules for.
    ///
    /// Published as a list rather than described in prose so that a caller can iterate it, and so
    /// that a test can require a name to be judged against all of them rather than against the
    /// ones somebody remembered.
    pub const EVERY: [Self; 5] = [
        Self::LINUX,
        Self::APFS_INSENSITIVE,
        Self::APFS_SENSITIVE,
        Self::HFS_PLUS,
        Self::NTFS,
    ];

    /// What to call this volume in a message.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        self.label
    }

    /// The one fold family this volume applies.
    #[must_use]
    pub const fn entry_fold(&self) -> NameFold {
        self.entry_fold
    }

    /// Whether two names differing only in case are two entries here.
    #[must_use]
    pub const fn case_sensitive(&self) -> bool {
        matches!(self.entry_fold, NameFold::Exact | NameFold::Canonical)
    }

    /// Whether two names differing only in canonical form are two entries here.
    #[must_use]
    pub const fn normalization_sensitive(&self) -> bool {
        matches!(self.entry_fold, NameFold::Exact | NameFold::Upcase)
    }

    /// Whether this volume would hold two names related this way as one entry.
    #[must_use]
    pub const fn folds(&self, relation: NameRelation) -> bool {
        relation.joined_by(self.entry_fold)
    }
}

/// Why a name cannot be an entry on a volume.
///
/// Carries the measurement, not just the verdict: a person told "too long" reaches for a guess,
/// and a person told "271 of 255 UTF-16 units" reaches for the right number of characters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NameRestriction {
    /// The name was empty.
    Empty,
    /// The name was `.` or `..`, which name a directory rather than an entry in one.
    RelativeName,
    /// The name contained `/`, which no filesystem can hold in an entry.
    PathSeparator,
    /// The name contained a NUL byte, which ends the name at the system-call boundary.
    NulByte,
    /// The name contained a C0 control character or DEL.
    ControlCharacter {
        /// The character found.
        character: char,
    },
    /// The name contained a character this volume refuses or reinterprets.
    ReservedCharacter {
        /// The character found.
        character: char,
    },
    /// The name's stem is a reserved device name, whatever extension follows it.
    ReservedDeviceName {
        /// The device the stem names.
        device: &'static str,
    },
    /// The name ends in a dot, which this volume strips.
    TrailingDot,
    /// The name ends in a space, which this volume strips.
    TrailingSpace,
    /// The name is longer than this volume's byte ceiling.
    TooManyBytes {
        /// What the name measures.
        bytes: usize,
        /// What the volume allows.
        limit: usize,
    },
    /// The name is longer than this volume's UTF-16 ceiling.
    TooManyUtf16Units {
        /// What the name measures.
        units: usize,
        /// What the volume allows.
        limit: usize,
    },
}

impl fmt::Display for NameRestriction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("the name is empty"),
            Self::RelativeName => formatter.write_str("\".\" and \"..\" are not entry names"),
            Self::PathSeparator => formatter.write_str("the name contains \"/\""),
            Self::NulByte => formatter.write_str("the name contains a NUL byte"),
            Self::ControlCharacter { character } => {
                write!(
                    formatter,
                    "the name contains control character U+{:04X}",
                    *character as u32
                )
            }
            Self::ReservedCharacter { character } => {
                write!(
                    formatter,
                    "this volume reserves the character {character:?}"
                )
            }
            Self::ReservedDeviceName { device } => {
                write!(
                    formatter,
                    "{device} names a device on this volume, whatever follows it"
                )
            }
            Self::TrailingDot => formatter.write_str("this volume strips a trailing dot"),
            Self::TrailingSpace => formatter.write_str("this volume strips a trailing space"),
            Self::TooManyBytes { bytes, limit } => {
                write!(
                    formatter,
                    "the name is {bytes} bytes and this volume allows {limit}"
                )
            }
            Self::TooManyUtf16Units { units, limit } => {
                write!(
                    formatter,
                    "the name is {units} UTF-16 units and this volume allows {limit}"
                )
            }
        }
    }
}

impl std::error::Error for NameRestriction {}

/// The device names Windows reserves, in every case and with any extension.
///
/// `COM0` and `LPT0` are on the list because current Windows documentation puts them there, and
/// `CONIN$`/`CONOUT$` because the console handles are reachable by name. A name mesh refuses that
/// Windows would have accepted costs one refusal; the reverse costs a file.
pub const RESERVED_DEVICE_NAMES: [&str; 26] = [
    "CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "COM0", "COM1", "COM2", "COM3", "COM4",
    "COM5", "COM6", "COM7", "COM8", "COM9", "LPT0", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6",
    "LPT7", "LPT8", "LPT9",
];

/// Every rule `name` breaks on `volume`, in a stable order and without repetition.
///
/// An empty result means the volume can hold this name as written. It does **not** mean the
/// directory can hold it — two names that are each representable can still collide with each
/// other, which is [`crate::preflight_directory`]'s question rather than this one's.
#[must_use]
pub fn restrictions(name: &str, volume: &VolumeProfile) -> Vec<NameRestriction> {
    let mut found = Vec::new();

    if name.is_empty() {
        found.push(NameRestriction::Empty);
        return found;
    }
    if name == "." || name == ".." {
        found.push(NameRestriction::RelativeName);
    }
    if name.contains('/') {
        found.push(NameRestriction::PathSeparator);
    }
    if name.contains('\0') {
        found.push(NameRestriction::NulByte);
    }

    if volume.rejects_control_characters {
        if let Some(character) = name.chars().find(|character| is_control(*character)) {
            found.push(NameRestriction::ControlCharacter { character });
        }
    }

    for character in volume.reserved_characters.chars() {
        if name.contains(character) {
            found.push(NameRestriction::ReservedCharacter { character });
        }
    }

    if volume.reserves_device_names {
        if let Some(device) = reserved_device(name) {
            found.push(NameRestriction::ReservedDeviceName { device });
        }
    }

    if volume.rejects_trailing_dot_or_space {
        if name.ends_with('.') {
            found.push(NameRestriction::TrailingDot);
        }
        if name.ends_with(' ') {
            found.push(NameRestriction::TrailingSpace);
        }
    }

    let bytes = name.len();
    if bytes > volume.max_name_bytes {
        found.push(NameRestriction::TooManyBytes {
            bytes,
            limit: volume.max_name_bytes,
        });
    }
    let units = name.chars().map(char::len_utf16).sum::<usize>();
    if units > volume.max_name_utf16 {
        found.push(NameRestriction::TooManyUtf16Units {
            units,
            limit: volume.max_name_utf16,
        });
    }

    found.sort_unstable();
    found.dedup();
    found
}

/// Every rule `name` breaks on any volume in [`VolumeProfile::EVERY`], with the volume named.
///
/// This is the answer a person wants before a workspace is shared with a host they do not own:
/// "which of these names will not survive somewhere the workspace is going".
#[must_use]
pub fn restrictions_everywhere(name: &str) -> Vec<(&'static str, NameRestriction)> {
    let mut found = Vec::new();
    for volume in &VolumeProfile::EVERY {
        for restriction in restrictions(name, volume) {
            found.push((volume.label(), restriction));
        }
    }
    found
}

/// Whether a name is representable on every volume mesh has rules for.
#[must_use]
pub fn is_portable(name: &str) -> bool {
    restrictions_everywhere(name).is_empty()
}

/// The device a name's stem denotes, if it denotes one.
///
/// The stem is everything before the first dot, with trailing spaces removed, compared without
/// regard to case. `CON`, `con.txt`, `CoN.tar.gz` and `con ` all name the console.
fn reserved_device(name: &str) -> Option<&'static str> {
    let stem = name.split('.').next().unwrap_or(name).trim_end_matches(' ');
    RESERVED_DEVICE_NAMES
        .into_iter()
        .find(|device| device.len() == stem.len() && device.eq_ignore_ascii_case(stem))
}

/// Whether a character is a C0 control or DEL.
fn is_control(character: char) -> bool {
    matches!(character, '\u{0}'..='\u{1F}' | '\u{7F}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ordinary_name_is_portable() {
        assert!(is_portable("report.md"));
        assert!(is_portable(".gitignore"));
        assert!(is_portable("\u{6c34}\u{66dc}\u{65e5}.txt"));
    }

    #[test]
    fn the_device_names_are_caught_with_any_extension_and_in_any_case() {
        for name in [
            "CON",
            "con",
            "CoN.txt",
            "nul.tar.gz",
            "com1",
            "LPT9.md",
            "conout$",
        ] {
            assert_eq!(
                restrictions(name, &VolumeProfile::NTFS)
                    .iter()
                    .filter(|restriction| matches!(
                        restriction,
                        NameRestriction::ReservedDeviceName { .. }
                    ))
                    .count(),
                1,
                "{name} was not recognised as a device name"
            );
        }
    }

    #[test]
    fn a_name_that_merely_starts_with_a_device_name_is_not_one() {
        for name in ["CONTRIBUTING.md", "console.log", "nullable.rs", "com10.txt"] {
            assert!(
                restrictions(name, &VolumeProfile::NTFS).is_empty(),
                "{name} was wrongly refused"
            );
        }
    }

    #[test]
    fn a_device_name_is_only_a_restriction_where_devices_are_reserved() {
        assert!(restrictions("CON", &VolumeProfile::LINUX).is_empty());
        assert!(restrictions("CON", &VolumeProfile::APFS_INSENSITIVE).is_empty());
    }

    #[test]
    fn a_trailing_dot_or_space_is_caught_where_it_is_stripped() {
        assert!(
            restrictions("report.", &VolumeProfile::NTFS).contains(&NameRestriction::TrailingDot)
        );
        assert!(
            restrictions("report ", &VolumeProfile::NTFS).contains(&NameRestriction::TrailingSpace)
        );
        assert!(restrictions("report.", &VolumeProfile::LINUX).is_empty());
        // A leading dot is a dotfile, not a trailing dot.
        assert!(restrictions(".gitignore", &VolumeProfile::NTFS).is_empty());
    }

    #[test]
    fn the_two_length_ceilings_are_counted_in_their_own_units() {
        let two_hundred_kanji = "\u{6c34}".repeat(200);
        let on_linux = restrictions(&two_hundred_kanji, &VolumeProfile::LINUX);
        assert_eq!(
            on_linux,
            vec![NameRestriction::TooManyBytes {
                bytes: 600,
                limit: 255
            }]
        );
        assert!(restrictions(&two_hundred_kanji, &VolumeProfile::NTFS).is_empty());

        let three_hundred_ascii = "a".repeat(300);
        assert!(
            restrictions(&three_hundred_ascii, &VolumeProfile::NTFS).contains(
                &NameRestriction::TooManyUtf16Units {
                    units: 300,
                    limit: 255
                }
            )
        );
    }

    #[test]
    fn an_astral_character_counts_two_utf16_units() {
        // U+1F600 is one scalar value, four UTF-8 bytes and two UTF-16 code units.
        let emoji = "\u{1F600}".repeat(128);
        assert!(restrictions(&emoji, &VolumeProfile::NTFS).contains(
            &NameRestriction::TooManyUtf16Units {
                units: 256,
                limit: 255
            }
        ));
    }

    #[test]
    fn the_structurally_impossible_names_are_refused_by_every_volume() {
        for volume in &VolumeProfile::EVERY {
            assert_eq!(restrictions("", volume), vec![NameRestriction::Empty]);
            assert!(restrictions("..", volume).contains(&NameRestriction::RelativeName));
            assert!(restrictions("a/b", volume).contains(&NameRestriction::PathSeparator));
            assert!(restrictions("a\0b", volume).contains(&NameRestriction::NulByte));
        }
    }

    #[test]
    fn a_colon_is_reserved_on_the_apple_volumes_and_on_windows_but_not_on_linux() {
        let colon = NameRestriction::ReservedCharacter { character: ':' };
        assert!(restrictions("12:30.md", &VolumeProfile::APFS_INSENSITIVE).contains(&colon));
        assert!(restrictions("12:30.md", &VolumeProfile::HFS_PLUS).contains(&colon));
        assert!(restrictions("12:30.md", &VolumeProfile::NTFS).contains(&colon));
        assert!(restrictions("12:30.md", &VolumeProfile::LINUX).is_empty());
    }

    #[test]
    fn a_backslash_is_a_separator_on_windows_and_an_ordinary_character_on_linux() {
        assert!(restrictions("a\\b", &VolumeProfile::NTFS)
            .contains(&NameRestriction::ReservedCharacter { character: '\\' }));
        assert!(restrictions("a\\b", &VolumeProfile::LINUX).is_empty());
    }

    #[test]
    fn a_control_character_is_caught_where_it_is_refused() {
        assert!(restrictions("a\tb", &VolumeProfile::NTFS)
            .contains(&NameRestriction::ControlCharacter { character: '\t' }));
        assert!(restrictions("a\tb", &VolumeProfile::LINUX).is_empty());
    }

    #[test]
    fn restrictions_everywhere_names_the_volume_that_refused() {
        let found = restrictions_everywhere("CON.txt");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, VolumeProfile::NTFS.label());
        assert!(!is_portable("CON.txt"));
    }

    #[test]
    fn a_volume_folds_exactly_what_its_two_sensitivities_say() {
        let case = crate::fold::relate("README", "readme");
        let normalization = crate::fold::relate("caf\u{e9}", "cafe\u{301}");
        let identical = crate::fold::relate("notes.md", "notes.md");

        assert!(!VolumeProfile::LINUX.folds(case));
        assert!(VolumeProfile::NTFS.folds(case));
        assert!(!VolumeProfile::NTFS.folds(normalization));
        assert!(VolumeProfile::APFS_INSENSITIVE.folds(normalization));
        assert!(VolumeProfile::APFS_SENSITIVE.folds(normalization));
        assert!(VolumeProfile::APFS_SENSITIVE.folds(identical));
        assert!(!VolumeProfile::APFS_INSENSITIVE.folds(NameRelation::DISTINCT));

        // The two derived accessors follow the one field they are derived from.
        assert!(VolumeProfile::LINUX.case_sensitive());
        assert!(VolumeProfile::LINUX.normalization_sensitive());
        assert!(!VolumeProfile::NTFS.case_sensitive());
        assert!(VolumeProfile::NTFS.normalization_sensitive());
        assert!(VolumeProfile::APFS_SENSITIVE.case_sensitive());
        assert!(!VolumeProfile::APFS_SENSITIVE.normalization_sensitive());
    }
}
