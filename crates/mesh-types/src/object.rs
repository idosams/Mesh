//! Stable objects and their immutable versions.

use core::fmt;
use std::collections::BTreeMap;

use crate::canonical::{CanonicalEncode, CanonicalType, CanonicalValue, FieldSchema, RecordSchema};
use crate::digest::{Absorb, CanonicalRecord, DigestHasher, DigestWriter, DomainTag};
use crate::entity_id::ObjectId;
use crate::record_id::{ChangeSetId, ManifestId, VersionId};

/// One directory entry, as it appears inside a directory version's `entries` sequence.
///
/// The sequence is in ascending byte-lexicographic order of `name`'s UTF-8 bytes — the order a
/// [`BTreeMap`] keyed by [`NormalizedName`] iterates in, which is the same order on every
/// platform. An external implementer sorts by UTF-8 bytes: not by locale, and not by code point
/// after any normalization form.
const DIRECTORY_ENTRY_FIELDS: &[FieldSchema] = &[
    FieldSchema::new("name", CanonicalType::Text),
    FieldSchema::new("object_id", CanonicalType::Bytes(Some(16))),
    FieldSchema::new("version_id", CanonicalType::Bytes(Some(32))),
];

/// The portable metadata group a file version binds.
const PORTABLE_METADATA_FIELDS: &[FieldSchema] =
    &[FieldSchema::new("executable", CanonicalType::Bool)];

/// What a stable object is.
///
/// A symlink is its own kind rather than a file with a flag, because WSP-007 makes safe symlink
/// handling a P0 requirement and a kind the type system can see is harder to forget than a flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ObjectKind {
    /// A regular file.
    File,
    /// A directory.
    Directory,
    /// A symbolic link.
    Symlink,
}

impl ObjectKind {
    /// Every kind.
    pub const ALL: [Self; 3] = [Self::File, Self::Directory, Self::Symlink];

    /// The wire spelling.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Directory => "directory",
            Self::Symlink => "symlink",
        }
    }
}

impl fmt::Display for ObjectKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The stable identity of a file or directory, independent of any path it has ever had.
///
/// Charter P6: paths are not file identities. A rename changes a [`DirectoryEntry`], never this.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Object {
    id: ObjectId,
    kind: ObjectKind,
    created_by: ChangeSetId,
}

impl Object {
    /// A stable object.
    #[must_use]
    pub const fn new(id: ObjectId, kind: ObjectKind, created_by: ChangeSetId) -> Self {
        Self {
            id,
            kind,
            created_by,
        }
    }

    /// The object identifier.
    #[must_use]
    pub const fn id(&self) -> ObjectId {
        self.id
    }

    /// What kind of object this is.
    #[must_use]
    pub const fn kind(&self) -> ObjectKind {
        self.kind
    }

    /// The ChangeSet that created this object.
    #[must_use]
    pub const fn created_by(&self) -> ChangeSetId {
        self.created_by
    }
}

/// A directory entry name that is structurally usable as one.
///
/// This type rejects the names that cannot be an entry on any platform: the empty string, a name
/// containing a path separator or a NUL byte, and the two relative names. It does **not** apply a
/// Unicode normalization form — the plan calls for filename normalization but does not specify
/// which form, and inventing one here would pin a protocol-visible rule this crate has no
/// authority to pin. When `mesh-materializer` specifies it, this constructor is where it is
/// enforced, and widening what is accepted is a compatibility event.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NormalizedName(String);

impl NormalizedName {
    /// Accept a name, rejecting the structurally impossible ones.
    ///
    /// # Errors
    ///
    /// [`NameError`] naming which rule the input broke.
    pub fn new(name: impl Into<String>) -> Result<Self, NameError> {
        let name = name.into();
        if name.is_empty() {
            return Err(NameError::Empty);
        }
        if name == "." || name == ".." {
            return Err(NameError::Relative);
        }
        if name.contains('/') || name.contains('\\') {
            return Err(NameError::Separator);
        }
        if name.contains('\0') {
            return Err(NameError::Nul);
        }
        Ok(Self(name))
    }

    /// The name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NormalizedName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Why a directory entry name was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameError {
    /// The name was empty.
    Empty,
    /// The name was `.` or `..`.
    Relative,
    /// The name contained a path separator.
    Separator,
    /// The name contained a NUL byte.
    Nul,
}

impl fmt::Display for NameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Empty => "a directory entry name is not empty",
            Self::Relative => "a directory entry name is neither \".\" nor \"..\"",
            Self::Separator => "a directory entry name contains no path separator",
            Self::Nul => "a directory entry name contains no NUL byte",
        })
    }
}

impl std::error::Error for NameError {}

/// The binding of a name to a child object inside a directory version.
///
/// Renames and moves operate on entries, never on objects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DirectoryEntry {
    object_id: ObjectId,
    version_id: VersionId,
}

impl DirectoryEntry {
    /// Bind a child object at a version.
    #[must_use]
    pub const fn new(object_id: ObjectId, version_id: VersionId) -> Self {
        Self {
            object_id,
            version_id,
        }
    }

    /// The child object.
    #[must_use]
    pub const fn object_id(&self) -> ObjectId {
        self.object_id
    }

    /// The child's version.
    #[must_use]
    pub const fn version_id(&self) -> VersionId {
        self.version_id
    }
}

/// A version of a directory object: its set of directory entries.
///
/// Entries live in a [`BTreeMap`], so iteration order is the sorted name order on every machine —
/// which is what makes [`VersionId`] reproducible without a separate sorting step that somebody
/// could forget.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryVersion {
    object_id: ObjectId,
    entries: BTreeMap<NormalizedName, DirectoryEntry>,
}

impl DirectoryVersion {
    /// A directory version over `entries`.
    #[must_use]
    pub fn new(object_id: ObjectId, entries: BTreeMap<NormalizedName, DirectoryEntry>) -> Self {
        Self { object_id, entries }
    }

    /// An empty directory version.
    #[must_use]
    pub fn empty(object_id: ObjectId) -> Self {
        Self::new(object_id, BTreeMap::new())
    }

    /// The directory object.
    #[must_use]
    pub const fn object_id(&self) -> ObjectId {
        self.object_id
    }

    /// The entries, in sorted name order.
    #[must_use]
    pub const fn entries(&self) -> &BTreeMap<NormalizedName, DirectoryEntry> {
        &self.entries
    }
}

impl CanonicalRecord for DirectoryVersion {
    type Id = VersionId;

    const DOMAIN: DomainTag = DomainTag::new("mesh.v0.directory-version");
}

impl Absorb for DirectoryVersion {
    fn absorb<H: DigestHasher>(&self, writer: &mut DigestWriter<H>) {
        writer.bytes(self.object_id.uuid().as_bytes());
        writer.u64(self.entries.len() as u64);
        for (name, entry) in &self.entries {
            writer.text(name.as_str());
            writer.bytes(entry.object_id().uuid().as_bytes());
            writer.digest(entry.version_id().digest());
        }
    }
}

impl CanonicalEncode for DirectoryVersion {
    const SCHEMA: RecordSchema = RecordSchema::new(
        <Self as CanonicalRecord>::DOMAIN,
        &[
            FieldSchema::new("object_id", CanonicalType::Bytes(Some(16))),
            FieldSchema::new(
                "entries",
                CanonicalType::Sequence(&CanonicalType::Group(DIRECTORY_ENTRY_FIELDS)),
            ),
        ],
    );

    fn canonical_fields(&self) -> Vec<CanonicalValue> {
        vec![
            CanonicalValue::from_array(*self.object_id.uuid().as_bytes()),
            CanonicalValue::Sequence(
                self.entries
                    .iter()
                    .map(|(name, entry)| {
                        CanonicalValue::Group(vec![
                            CanonicalValue::Text(name.as_str().to_owned()),
                            CanonicalValue::from_array(*entry.object_id().uuid().as_bytes()),
                            CanonicalValue::from_digest(entry.version_id().digest()),
                        ])
                    })
                    .collect(),
            ),
        ]
    }
}

/// The subset of file metadata Mesh carries across platforms without loss.
///
/// One field today. The set is `mesh-materializer`'s to define and widening it moves every
/// [`VersionId`] that binds it, which makes widening a compatibility event under this task's
/// failure-and-recovery clause rather than a field addition.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PortableMetadata {
    executable: bool,
}

impl PortableMetadata {
    /// Portable metadata with the executable bit set as given.
    #[must_use]
    pub const fn new(executable: bool) -> Self {
        Self { executable }
    }

    /// Whether the file is executable.
    #[must_use]
    pub const fn is_executable(&self) -> bool {
        self.executable
    }
}

/// A version of a file object: its file manifest, its portable metadata and its provenance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileVersion {
    object_id: ObjectId,
    parent_versions: Vec<VersionId>,
    manifest_id: ManifestId,
    portable_metadata: PortableMetadata,
    created_by: ChangeSetId,
}

impl FileVersion {
    /// A file version. `parent_versions` is empty for the first version of an object and carries
    /// more than one entry where a merge produced it.
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
            parent_versions,
            manifest_id,
            portable_metadata,
            created_by,
        }
    }

    /// The file object.
    #[must_use]
    pub const fn object_id(&self) -> ObjectId {
        self.object_id
    }

    /// The versions this one supersedes.
    #[must_use]
    pub fn parent_versions(&self) -> &[VersionId] {
        &self.parent_versions
    }

    /// The manifest that reconstructs the bytes.
    #[must_use]
    pub const fn manifest_id(&self) -> ManifestId {
        self.manifest_id
    }

    /// The portable metadata.
    #[must_use]
    pub const fn portable_metadata(&self) -> PortableMetadata {
        self.portable_metadata
    }

    /// The ChangeSet that produced this version.
    #[must_use]
    pub const fn created_by(&self) -> ChangeSetId {
        self.created_by
    }
}

impl CanonicalRecord for FileVersion {
    type Id = VersionId;

    const DOMAIN: DomainTag = DomainTag::new("mesh.v0.file-version");
}

impl Absorb for FileVersion {
    fn absorb<H: DigestHasher>(&self, writer: &mut DigestWriter<H>) {
        writer.bytes(self.object_id.uuid().as_bytes());
        writer.sequence(&self.parent_versions, |writer, parent| {
            writer.digest(parent.digest());
        });
        writer.digest(self.manifest_id.digest());
        writer.bool(self.portable_metadata.is_executable());
        writer.digest(self.created_by.digest());
    }
}

impl CanonicalEncode for FileVersion {
    const SCHEMA: RecordSchema = RecordSchema::new(
        <Self as CanonicalRecord>::DOMAIN,
        &[
            FieldSchema::new("object_id", CanonicalType::Bytes(Some(16))),
            FieldSchema::new(
                "parent_versions",
                CanonicalType::Sequence(&CanonicalType::Bytes(Some(32))),
            ),
            FieldSchema::new("manifest_id", CanonicalType::Bytes(Some(32))),
            FieldSchema::new(
                "portable_metadata",
                CanonicalType::Group(PORTABLE_METADATA_FIELDS),
            ),
            FieldSchema::new("created_by", CanonicalType::Bytes(Some(32))),
        ],
    );

    fn canonical_fields(&self) -> Vec<CanonicalValue> {
        vec![
            CanonicalValue::from_array(*self.object_id.uuid().as_bytes()),
            CanonicalValue::Sequence(
                self.parent_versions
                    .iter()
                    .map(|parent| CanonicalValue::from_digest(parent.digest()))
                    .collect(),
            ),
            CanonicalValue::from_digest(self.manifest_id.digest()),
            CanonicalValue::Group(vec![CanonicalValue::Bool(
                self.portable_metadata.is_executable(),
            )]),
            CanonicalValue::from_digest(self.created_by.digest()),
        ]
    }
}
