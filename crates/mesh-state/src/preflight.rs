//! Judging a whole directory before anything is written to it.
//!
//! # The one sentence this module is built around
//!
//! **Every name a materialization would write is judged before the first one is written, so a
//! refusal costs nothing and a partial write never happens.** Discovering the sixth of ten names
//! is unrepresentable *during* materialization leaves five files on disk and five in the air, and
//! nothing on the volume records which half is which.
//!
//! # Two questions, not one
//!
//! A name can be unrepresentable on its own — [`restrictions`] answers that. A *set* of names can
//! be unrepresentable while every member is fine: `README.md` and `readme.md` are both perfectly
//! good names, and a default macOS volume holds one entry for the two of them. The second question
//! only exists for a set, which is why it is answered here and not in [`crate::portable`].
//!
//! # One key per volume, taken from the volume
//!
//! Every volume mesh models applies exactly one fold family, so grouping is one pass with one key:
//! [`VolumeProfile::entry_fold`] names the family and [`NameFold::key`] computes it. That is not a
//! convenience — it is what stops the grouping and [`VolumeProfile::folds`] drifting apart, since
//! both now read the same field. Two hand-written statements of one rule is how a collision gets
//! reported by one half and missed by the other, and the half that misses it is the one that lets
//! a filesystem pick a winner.
//!
//! `tests/names.rs` still checks the two against each other over the whole generated corpus,
//! because "derived from one field" is an argument and the corpus is evidence.
//!
//! # What a collision report is not
//!
//! It is not a resolution. Nothing here picks a survivor, renames anything or drops anything: the
//! names go in and the same names come out, grouped. Which name would have survived on the volume
//! is a property of the volume's write order rather than of the workspace, and inventing an answer
//! would be inventing the loss the report exists to prevent.

use std::collections::BTreeMap;

use crate::fold::{relate, NameFold, NameRelation};
use crate::portable::{restrictions, NameRestriction, VolumeProfile};

/// One name and one rule it breaks.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NameFinding {
    /// The name, exactly as it was given. Never a repaired version of it.
    name: String,
    /// The rule it breaks on this volume.
    restriction: NameRestriction,
}

impl NameFinding {
    /// The name, byte for byte as it was given.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The rule it breaks.
    #[must_use]
    pub const fn restriction(&self) -> NameRestriction {
        self.restriction
    }
}

/// Two or more names this volume would hold as one directory entry.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntryCollision {
    /// The families that join every pair in the group.
    relation: NameRelation,
    /// Every colliding name, in the order they were given, each exactly as given.
    names: Vec<String>,
}

impl EntryCollision {
    /// The fold families that join **every** pair in this group.
    ///
    /// The intersection over the pairs, so the answer holds for the group rather than for its
    /// luckiest pair: a volume folds this group into one entry exactly when its own family is in
    /// here. For the ordinary group of two it is [`relate`]'s answer for the pair.
    #[must_use]
    pub const fn relation(&self) -> NameRelation {
        self.relation
    }

    /// Every name in the collision, each byte for byte as it was given.
    #[must_use]
    pub fn names(&self) -> &[String] {
        &self.names
    }
}

/// What a volume would refuse about one directory's worth of names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryPreflight {
    /// The volume that was asked.
    volume: &'static str,
    /// Names that break a rule on their own.
    findings: Vec<NameFinding>,
    /// Groups of names the volume would hold as one entry.
    collisions: Vec<EntryCollision>,
}

impl DirectoryPreflight {
    /// The volume that was asked.
    #[must_use]
    pub const fn volume(&self) -> &'static str {
        self.volume
    }

    /// Names that break a rule on their own.
    #[must_use]
    pub fn findings(&self) -> &[NameFinding] {
        &self.findings
    }

    /// Groups of names the volume would hold as one entry.
    #[must_use]
    pub fn collisions(&self) -> &[EntryCollision] {
        &self.collisions
    }

    /// Whether the volume can hold every name given, as given.
    #[must_use]
    pub fn is_representable(&self) -> bool {
        self.findings.is_empty() && self.collisions.is_empty()
    }
}

/// Judge one directory's entry names against one volume.
///
/// Duplicate identical names in the input are a collision of the strongest kind, reported rather
/// than deduplicated: a caller that handed the same name twice is describing two objects, and
/// silently keeping one is the loss.
#[must_use]
pub fn preflight_directory<'a>(
    names: impl IntoIterator<Item = &'a str>,
    volume: &VolumeProfile,
) -> DirectoryPreflight {
    let names: Vec<&str> = names.into_iter().collect();

    let mut findings = Vec::new();
    for name in &names {
        for restriction in restrictions(name, volume) {
            findings.push(NameFinding {
                name: (*name).to_owned(),
                restriction,
            });
        }
    }

    let fold: NameFold = volume.entry_fold();
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for name in &names {
        groups
            .entry(fold.key(name))
            .or_default()
            .push((*name).to_owned());
    }

    let mut collisions: Vec<EntryCollision> = groups
        .into_values()
        .filter(|group| group.len() > 1)
        .map(|names| EntryCollision {
            relation: shared_families(&names),
            names,
        })
        .collect();
    collisions.sort();

    DirectoryPreflight {
        volume: volume.label(),
        findings,
        collisions,
    }
}

/// The fold families that join every pair in the group.
fn shared_families(names: &[String]) -> NameRelation {
    let mut shared = relate(&names[0], &names[0]);
    for (index, left) in names.iter().enumerate() {
        for right in &names[index + 1..] {
            shared = shared.intersect(relate(left, right));
        }
    }
    shared
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preflight(names: &[&str], volume: &VolumeProfile) -> DirectoryPreflight {
        preflight_directory(names.iter().copied(), volume)
    }

    #[test]
    fn an_ordinary_directory_is_representable_everywhere() {
        for volume in &VolumeProfile::EVERY {
            let report = preflight(&["src", "README.md", ".gitignore", "notes.txt"], volume);
            assert!(report.is_representable(), "{} refused", report.volume());
        }
    }

    #[test]
    fn a_case_only_pair_collides_on_the_case_insensitive_volumes_only() {
        let names = ["README.md", "readme.md"];
        assert!(preflight(&names, &VolumeProfile::LINUX).is_representable());
        assert!(preflight(&names, &VolumeProfile::APFS_SENSITIVE).is_representable());
        for volume in [
            VolumeProfile::APFS_INSENSITIVE,
            VolumeProfile::HFS_PLUS,
            VolumeProfile::NTFS,
        ] {
            let report = preflight(&names, &volume);
            assert_eq!(
                report.collisions().len(),
                1,
                "{} missed it",
                report.volume()
            );
            assert!(report.collisions()[0]
                .relation()
                .joined_by(NameFold::Upcase));
            assert!(report.collisions()[0]
                .relation()
                .joined_by(NameFold::Caseless));
            assert!(!report.collisions()[0]
                .relation()
                .joined_by(NameFold::Canonical));
            assert_eq!(report.collisions()[0].names(), &["README.md", "readme.md"]);
        }
    }

    #[test]
    fn a_normalization_only_pair_collides_on_the_apple_volumes_and_not_on_windows() {
        let names = ["caf\u{e9}.md", "cafe\u{301}.md"];
        assert!(preflight(&names, &VolumeProfile::LINUX).is_representable());
        assert!(preflight(&names, &VolumeProfile::NTFS).is_representable());
        for volume in [
            VolumeProfile::APFS_INSENSITIVE,
            VolumeProfile::APFS_SENSITIVE,
            VolumeProfile::HFS_PLUS,
        ] {
            let report = preflight(&names, &volume);
            assert_eq!(
                report.collisions().len(),
                1,
                "{} missed it",
                report.volume()
            );
            assert!(report.collisions()[0]
                .relation()
                .joined_by(NameFold::Canonical));
        }
    }

    #[test]
    fn the_dotless_i_collides_on_windows_and_not_on_a_macos_volume() {
        // The pair that a single "differs only in case" answer gets wrong. NTFS uppercases both
        // to `I`; a macOS volume folds `ı` to itself.
        let names = ["\u{131}.txt", "i.txt"];
        assert!(!preflight(&names, &VolumeProfile::NTFS).is_representable());
        assert!(preflight(&names, &VolumeProfile::APFS_INSENSITIVE).is_representable());
        assert!(preflight(&names, &VolumeProfile::HFS_PLUS).is_representable());
        assert!(preflight(&names, &VolumeProfile::LINUX).is_representable());
    }

    #[test]
    fn the_colliding_names_are_reported_exactly_as_given() {
        let report = preflight(
            &["caf\u{e9}.md", "CAFE\u{301}.md"],
            &VolumeProfile::APFS_INSENSITIVE,
        );
        assert_eq!(report.collisions().len(), 1);
        assert_eq!(
            report.collisions()[0].names(),
            &["caf\u{e9}.md".to_owned(), "CAFE\u{301}.md".to_owned()]
        );
        let relation = report.collisions()[0].relation();
        assert!(relation.joined_by(NameFold::Caseless));
        assert!(!relation.joined_by(NameFold::Canonical));
        assert!(!relation.joined_by(NameFold::Upcase));
    }

    #[test]
    fn an_identical_pair_is_a_collision_on_every_volume() {
        for volume in &VolumeProfile::EVERY {
            let report = preflight(&["notes.md", "notes.md"], volume);
            assert_eq!(
                report.collisions().len(),
                1,
                "{} missed it",
                report.volume()
            );
            assert!(report.collisions()[0].relation().is_identical());
        }
    }

    #[test]
    fn a_restriction_and_a_collision_are_reported_together_rather_than_one_at_a_time() {
        let report = preflight(&["CON.txt", "con.txt", "fine.md"], &VolumeProfile::NTFS);
        assert_eq!(report.findings().len(), 2);
        assert_eq!(report.collisions().len(), 1);
        assert!(!report.is_representable());
        assert!(report.findings().iter().all(|finding| matches!(
            finding.restriction(),
            NameRestriction::ReservedDeviceName { .. }
        )));
    }

    #[test]
    fn the_report_never_repairs_a_name() {
        let report = preflight(
            &["report.", "CON", "a".repeat(300).as_str()],
            &VolumeProfile::NTFS,
        );
        let reported: Vec<&str> = report.findings().iter().map(NameFinding::name).collect();
        assert!(reported.contains(&"report."));
        assert!(reported.contains(&"CON"));
        assert!(report
            .findings()
            .iter()
            .all(|finding| finding.name() != "report"));
    }

    #[test]
    fn a_group_of_three_answers_with_what_every_pair_shares() {
        let report = preflight(
            &["caf\u{e9}.md", "cafe\u{301}.md", "CAF\u{c9}.md"],
            &VolumeProfile::APFS_INSENSITIVE,
        );
        assert_eq!(report.collisions().len(), 1);
        assert_eq!(report.collisions()[0].names().len(), 3);
        let relation = report.collisions()[0].relation();
        // Every pair is joined by the caseless fold; not every pair shares a canonical form.
        assert!(relation.joined_by(NameFold::Caseless));
        assert!(!relation.joined_by(NameFold::Canonical));
        assert!(!relation.joined_by(NameFold::Upcase));
    }
}
