//! The nodes of a workspace state: objects, directory versions and file versions.
//!
//! These are plan §4.2's records, with one field added to each and none removed. The added field is
//! `created_by`, which the plan already lists on `Object` and `FileVersion` and which
//! materialization can always supply because every operation reaches it inside a ChangeSet.
//!
//! # Why a directory version carries no identifier of its own
//!
//! Plan §4.2 writes `DirectoryVersion { object_id, entries }` and gives it no `VersionId` field,
//! and that is kept exactly. A directory version's identity is a *digest of its content*, so
//! storing an identifier beside the content would make it possible for the two to disagree —
//! [`crate::WorkspaceState::state_hash`] derives every such name from the canonical encoding
//! instead, which is the same reason a file's manifest identifier is carried and a file version's
//! own identifier is not stored twice.
//!
//! # Every collection here is ordered
//!
//! `BTreeMap` and sorted `Vec`, never a hash map and never insertion order. Materialization must
//! produce byte-identical output for the same operation set, and an iteration order that depends on
//! a hash seed or on arrival order would break that before any encoder saw it.

use std::collections::BTreeMap;

use crate::ids::{ChangeSetId, ManifestId, ObjectId, VersionId};
use crate::name::{NormalizedName, PortableMetadata};

/// Whether an object is a file or a directory.
///
/// Fixed when the object is minted and never changed: an object that could change kind would make
/// every entry binding it ambiguous about what it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ObjectKind {
    /// A file.
    File,
    /// A directory.
    Directory,
}

impl ObjectKind {
    /// The published name.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::File => "File",
            Self::Directory => "Directory",
        }
    }
}

/// A stable object: an identity independent of any path.
///
/// Plan §4.2's `Object`, plus the two facts materialization derives about it — whether it is
/// deleted, and which version is current. Deletion is a **state**, never an erasure: a deleted
/// object keeps every version it ever had, which is what makes
/// [`crate::Operation::RestoreObject`] able to name one.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectRecord {
    kind: ObjectKind,
    created_by: Option<ChangeSetId>,
    deleted: bool,
    current_version: Option<VersionId>,
}

impl ObjectRecord {
    /// A freshly minted object: not deleted, with no version yet.
    #[must_use]
    pub const fn minted(kind: ObjectKind, created_by: ChangeSetId) -> Self {
        Self {
            kind,
            created_by: Some(created_by),
            deleted: false,
            current_version: None,
        }
    }

    /// The workspace root directory, which exists before any ChangeSet does.
    ///
    /// `created_by` is `None` rather than a zero identifier, because a sentinel identifier would be
    /// a real `ChangeSetId` that no ChangeSet has, and something downstream would eventually look
    /// it up.
    #[must_use]
    pub const fn root() -> Self {
        Self {
            kind: ObjectKind::Directory,
            created_by: None,
            deleted: false,
            current_version: None,
        }
    }

    /// Whether this is a file or a directory.
    #[must_use]
    pub const fn kind(&self) -> ObjectKind {
        self.kind
    }

    /// The ChangeSet that minted it, or `None` for the workspace root.
    #[must_use]
    pub const fn created_by(&self) -> Option<ChangeSetId> {
        self.created_by
    }

    /// Whether it is currently deleted.
    #[must_use]
    pub const fn is_deleted(&self) -> bool {
        self.deleted
    }

    /// The version materialization holds as current, if any.
    #[must_use]
    pub const fn current_version(&self) -> Option<VersionId> {
        self.current_version
    }

    /// The same object, marked deleted or not.
    #[must_use]
    pub fn with_deleted(&self, deleted: bool) -> Self {
        Self {
            deleted,
            ..self.clone()
        }
    }

    /// The same object at a different current version.
    #[must_use]
    pub fn with_current_version(&self, current_version: Option<VersionId>) -> Self {
        Self {
            current_version,
            ..self.clone()
        }
    }
}

/// One name binding inside a directory version: which object, at which version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DirectoryEntry {
    object_id: ObjectId,
    version_id: VersionId,
}

impl DirectoryEntry {
    /// A binding of a name to an object at a version.
    #[must_use]
    pub const fn new(object_id: ObjectId, version_id: VersionId) -> Self {
        Self {
            object_id,
            version_id,
        }
    }

    /// The object the name is bound to.
    #[must_use]
    pub const fn object_id(&self) -> ObjectId {
        self.object_id
    }

    /// The version the name is bound at.
    #[must_use]
    pub const fn version_id(&self) -> VersionId {
        self.version_id
    }
}

/// Plan §4.2's `DirectoryVersion`: an object, and the names it binds.
///
/// Immutable. Every operation on it returns a new value, so a directory version that an approval
/// already names cannot be edited underneath it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DirectoryVersion {
    entries: BTreeMap<NormalizedName, DirectoryEntry>,
}

impl DirectoryVersion {
    /// An empty directory version.
    ///
    /// `const` so a state can hold one as a `static` fallback rather than allocating one to return
    /// a reference to.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    /// Every binding, in name order.
    #[must_use]
    pub const fn entries(&self) -> &BTreeMap<NormalizedName, DirectoryEntry> {
        &self.entries
    }

    /// What `name` is bound to here, if anything.
    #[must_use]
    pub fn entry(&self, name: &NormalizedName) -> Option<DirectoryEntry> {
        self.entries.get(name).copied()
    }

    /// The name this directory binds to `object`, if it binds one.
    ///
    /// A directory binds an object under at most one name — [`crate::Rejection::NameTaken`] and the
    /// link rules keep it that way — so the first match is the only match.
    #[must_use]
    pub fn name_of(&self, object: ObjectId) -> Option<NormalizedName> {
        self.entries
            .iter()
            .find(|(_, entry)| entry.object_id == object)
            .map(|(name, _)| name.clone())
    }

    /// The same directory version with `name` bound to `entry`.
    #[must_use]
    pub fn with_entry(&self, name: NormalizedName, entry: DirectoryEntry) -> Self {
        let mut entries = self.entries.clone();
        entries.insert(name, entry);
        Self { entries }
    }

    /// The same directory version with `name` unbound.
    #[must_use]
    pub fn without_entry(&self, name: &NormalizedName) -> Self {
        let mut entries = self.entries.clone();
        entries.remove(name);
        Self { entries }
    }

    /// How many names it binds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether it binds nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Plan §4.2's `FileVersion`: one immutable version of one file.
///
/// `parent_versions` is a **set**, held sorted and deduplicated, because it is the record of what
/// this version supersedes and two spellings of one supersession set would give one version two
/// canonical encodings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileVersion {
    object_id: ObjectId,
    parent_versions: Vec<VersionId>,
    manifest_id: ManifestId,
    portable_metadata: PortableMetadata,
    created_by: ChangeSetId,
}

impl FileVersion {
    /// A file version. `parent_versions` is sorted and deduplicated on the way in.
    #[must_use]
    pub fn new(
        object_id: ObjectId,
        parent_versions: Vec<VersionId>,
        manifest_id: ManifestId,
        portable_metadata: PortableMetadata,
        created_by: ChangeSetId,
    ) -> Self {
        Self {
            object_id,
            parent_versions: sorted_unique(parent_versions),
            manifest_id,
            portable_metadata,
            created_by,
        }
    }

    /// The object this is a version of.
    #[must_use]
    pub const fn object_id(&self) -> ObjectId {
        self.object_id
    }

    /// The versions it supersedes, sorted and deduplicated.
    #[must_use]
    pub fn parent_versions(&self) -> &[VersionId] {
        &self.parent_versions
    }

    /// The content-addressed manifest.
    #[must_use]
    pub const fn manifest_id(&self) -> ManifestId {
        self.manifest_id
    }

    /// The portable metadata.
    #[must_use]
    pub const fn portable_metadata(&self) -> PortableMetadata {
        self.portable_metadata
    }

    /// The ChangeSet that recorded it.
    #[must_use]
    pub const fn created_by(&self) -> ChangeSetId {
        self.created_by
    }

    /// The same version with different portable metadata.
    #[must_use]
    pub fn with_portable_metadata(&self, portable_metadata: PortableMetadata) -> Self {
        Self {
            portable_metadata,
            ..self.clone()
        }
    }

    /// Whether two records describe the same version, ignoring which ChangeSet recorded it.
    ///
    /// A version identifier is a content digest, so two actors that independently produce the same
    /// content produce the same identifier under two different ChangeSets. That is a re-record, not
    /// a collision, and materialization keeps the first writer in causal order rather than refusing
    /// the second. Comparing `created_by` here would turn convergence into a rejection.
    #[must_use]
    pub fn has_same_content(&self, other: &Self) -> bool {
        self.object_id == other.object_id
            && self.parent_versions == other.parent_versions
            && self.manifest_id == other.manifest_id
            && self.portable_metadata == other.portable_metadata
    }
}

/// Sort and deduplicate, so a set has one spelling.
fn sorted_unique(mut values: Vec<VersionId>) -> Vec<VersionId> {
    values.sort_unstable();
    values.dedup();
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(byte: u8) -> VersionId {
        VersionId::from_bytes([byte; 32])
    }

    fn name(text: &str) -> NormalizedName {
        NormalizedName::new(text).unwrap()
    }

    #[test]
    fn a_minted_object_is_alive_and_has_no_version() {
        let object = ObjectRecord::minted(ObjectKind::File, ChangeSetId::from_bytes([1; 32]));
        assert!(!object.is_deleted());
        assert_eq!(object.current_version(), None);
        assert_eq!(object.kind(), ObjectKind::File);
    }

    #[test]
    fn deletion_preserves_everything_else_about_the_object() {
        let object = ObjectRecord::minted(ObjectKind::File, ChangeSetId::from_bytes([1; 32]))
            .with_current_version(Some(version(9)));
        let deleted = object.with_deleted(true);
        assert!(deleted.is_deleted());
        assert_eq!(deleted.current_version(), Some(version(9)));
        assert_eq!(deleted.created_by(), object.created_by());
        // The original is untouched: every operation returns a new value.
        assert!(!object.is_deleted());
    }

    #[test]
    fn a_parent_version_set_has_one_spelling() {
        let one = FileVersion::new(
            ObjectId::from_bytes([1; 16]),
            vec![version(3), version(1), version(3)],
            ManifestId::from_bytes([2; 32]),
            PortableMetadata::default(),
            ChangeSetId::from_bytes([4; 32]),
        );
        let other = FileVersion::new(
            ObjectId::from_bytes([1; 16]),
            vec![version(1), version(3)],
            ManifestId::from_bytes([2; 32]),
            PortableMetadata::default(),
            ChangeSetId::from_bytes([4; 32]),
        );
        assert_eq!(one, other);
        assert_eq!(one.parent_versions(), &[version(1), version(3)]);
    }

    #[test]
    fn two_actors_recording_one_version_record_the_same_version() {
        let mine = FileVersion::new(
            ObjectId::from_bytes([1; 16]),
            vec![version(1)],
            ManifestId::from_bytes([2; 32]),
            PortableMetadata::new(true),
            ChangeSetId::from_bytes([4; 32]),
        );
        let theirs = FileVersion::new(
            ObjectId::from_bytes([1; 16]),
            vec![version(1)],
            ManifestId::from_bytes([2; 32]),
            PortableMetadata::new(true),
            ChangeSetId::from_bytes([9; 32]),
        );
        assert!(mine.has_same_content(&theirs));
        assert_ne!(mine, theirs);

        let different = FileVersion::new(
            ObjectId::from_bytes([1; 16]),
            vec![version(1)],
            ManifestId::from_bytes([3; 32]),
            PortableMetadata::new(true),
            ChangeSetId::from_bytes([4; 32]),
        );
        assert!(!mine.has_same_content(&different));
    }

    #[test]
    fn a_directory_version_is_ordered_by_name_and_answers_the_reverse_question() {
        let directory = DirectoryVersion::empty()
            .with_entry(
                name("b"),
                DirectoryEntry::new(ObjectId::from_bytes([2; 16]), version(2)),
            )
            .with_entry(
                name("a"),
                DirectoryEntry::new(ObjectId::from_bytes([1; 16]), version(1)),
            );
        let names: Vec<&str> = directory
            .entries()
            .keys()
            .map(NormalizedName::as_str)
            .collect();
        assert_eq!(names, vec!["a", "b"]);
        assert_eq!(
            directory.name_of(ObjectId::from_bytes([2; 16])),
            Some(name("b"))
        );
        assert_eq!(directory.name_of(ObjectId::from_bytes([9; 16])), None);
        assert_eq!(directory.without_entry(&name("a")).len(), 1);
        assert!(DirectoryVersion::empty().is_empty());
    }
}
