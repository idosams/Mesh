//! The state the concurrent changes departed from.
//!
//! A three-way merge needs a base, and so does every other row of the table: "preserve both" means
//! nothing without a third thing that both of them came from. [`Snapshot`] is that base — the
//! common ancestor state, as `mesh-state`'s object register would report it.
//!
//! # It stores a directory entry per object, never a path
//!
//! `docs/protocol.md` SG-6 and plan §2.6: an object's identity is not a function of its path. So
//! [`Snapshot`] holds, per object, the directory it hangs in and the name it hangs under, and
//! derives a path by walking that up to the root. A moved ancestor changes every descendant's path
//! with no descendant record touched at all, which is conflict row two holding by construction.
//!
//! Every method that changes a snapshot consumes it and returns a new one. A base that could be
//! mutated under a resolution in progress would make the resolution a function of when it was
//! read.

use std::collections::BTreeMap;

use crate::ids::{ObjectId, VersionId};
use crate::name::NormalizedName;
use crate::object::{Content, ObjectKind};

/// How deep a path may be before the walk gives up.
///
/// A base built by hand can contain a cycle this crate did not create — the guard is what keeps
/// [`Snapshot::path_of`] terminating on one instead of hanging. Resolution output cannot contain a
/// cycle; see `src/tree.rs`.
pub const MAX_PATH_DEPTH: usize = 1024;

/// One object as the base state holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BaseObject {
    kind: ObjectKind,
    directory: Option<ObjectId>,
    name: Option<NormalizedName>,
    content: Option<Content>,
}

impl BaseObject {
    /// Whether it holds content or other objects.
    #[must_use]
    pub const fn kind(&self) -> ObjectKind {
        self.kind
    }

    /// The directory it hangs in, absent only for the root.
    #[must_use]
    pub const fn directory(&self) -> Option<ObjectId> {
        self.directory
    }

    /// The name it hangs under, absent only for the root.
    #[must_use]
    pub const fn name(&self) -> Option<&NormalizedName> {
        self.name.as_ref()
    }

    /// The version it holds, absent for a directory and for a file with no content yet.
    #[must_use]
    pub const fn content(&self) -> Option<&Content> {
        self.content.as_ref()
    }
}

/// The common ancestor state a set of concurrent changes departed from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    root: ObjectId,
    objects: BTreeMap<ObjectId, BaseObject>,
}

impl Snapshot {
    /// An empty workspace with this root directory.
    #[must_use]
    pub fn new(root: ObjectId) -> Self {
        let mut objects = BTreeMap::new();
        objects.insert(
            root,
            BaseObject {
                kind: ObjectKind::Directory,
                directory: None,
                name: None,
                content: None,
            },
        );
        Self { root, objects }
    }

    /// This snapshot with a directory added.
    #[must_use]
    pub fn with_directory(
        self,
        object: ObjectId,
        directory: ObjectId,
        name: NormalizedName,
    ) -> Self {
        self.with_object(object, ObjectKind::Directory, directory, name, None)
    }

    /// This snapshot with a file added, holding the given content.
    #[must_use]
    pub fn with_file(
        self,
        object: ObjectId,
        directory: ObjectId,
        name: NormalizedName,
        content: Content,
    ) -> Self {
        self.with_object(object, ObjectKind::File, directory, name, Some(content))
    }

    /// This snapshot with an object added.
    #[must_use]
    fn with_object(
        self,
        object: ObjectId,
        kind: ObjectKind,
        directory: ObjectId,
        name: NormalizedName,
        content: Option<Content>,
    ) -> Self {
        let mut objects = self.objects;
        objects.insert(
            object,
            BaseObject {
                kind,
                directory: Some(directory),
                name: Some(name),
                content,
            },
        );
        Self {
            root: self.root,
            objects,
        }
    }

    /// The root directory.
    #[must_use]
    pub const fn root(&self) -> ObjectId {
        self.root
    }

    /// One object, if the base knows it.
    #[must_use]
    pub fn object(&self, object: ObjectId) -> Option<&BaseObject> {
        self.objects.get(&object)
    }

    /// Every object the base knows, in identifier order.
    pub fn objects(&self) -> impl Iterator<Item = (&ObjectId, &BaseObject)> {
        self.objects.iter()
    }

    /// The version this object held in the base, if it held one.
    #[must_use]
    pub fn version_of(&self, object: ObjectId) -> Option<VersionId> {
        self.objects
            .get(&object)
            .and_then(|found| found.content.as_ref())
            .map(Content::version)
    }

    /// Every version the base holds. The floor of what a resolution must keep reachable.
    #[must_use]
    pub fn versions(&self) -> Vec<VersionId> {
        self.objects
            .values()
            .filter_map(|object| object.content.as_ref().map(Content::version))
            .collect()
    }

    /// The path this object had in the base, derived by walking directory entries to the root.
    #[must_use]
    pub fn path_of(&self, object: ObjectId) -> Option<String> {
        let mut segments: Vec<String> = Vec::new();
        let mut at = object;
        for _ in 0..MAX_PATH_DEPTH {
            let found = self.objects.get(&at)?;
            match (found.directory, found.name.as_ref()) {
                (None, _) => {
                    let mut path = String::new();
                    for segment in segments.iter().rev() {
                        path.push('/');
                        path.push_str(segment);
                    }
                    if path.is_empty() {
                        path.push('/');
                    }
                    return Some(path);
                }
                (Some(parent), Some(name)) => {
                    segments.push(name.as_str().to_owned());
                    at = parent;
                }
                (Some(_), None) => return None,
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(text: &str) -> NormalizedName {
        NormalizedName::new(text).unwrap()
    }

    fn fixture() -> (Snapshot, ObjectId, ObjectId, ObjectId) {
        let root = ObjectId::from_bytes([0; 16]);
        let notes = ObjectId::from_bytes([1; 16]);
        let archive = ObjectId::from_bytes([2; 16]);
        let snapshot = Snapshot::new(root)
            .with_directory(archive, root, name("archive"))
            .with_file(
                notes,
                archive,
                name("notes.md"),
                Content::Text {
                    version: VersionId::from_bytes([7; 32]),
                    lines: vec!["one".into()],
                },
            );
        (snapshot, root, archive, notes)
    }

    #[test]
    fn a_path_is_derived_from_directory_entries() {
        let (snapshot, root, archive, notes) = fixture();
        assert_eq!(snapshot.path_of(root).unwrap(), "/");
        assert_eq!(snapshot.path_of(archive).unwrap(), "/archive");
        assert_eq!(snapshot.path_of(notes).unwrap(), "/archive/notes.md");
    }

    #[test]
    fn an_unknown_object_has_no_path() {
        let (snapshot, _, _, _) = fixture();
        assert_eq!(snapshot.path_of(ObjectId::from_bytes([9; 16])), None);
    }

    #[test]
    fn the_base_reports_every_version_it_holds() {
        let (snapshot, _, _, notes) = fixture();
        assert_eq!(snapshot.versions(), vec![VersionId::from_bytes([7; 32])]);
        assert_eq!(
            snapshot.version_of(notes),
            Some(VersionId::from_bytes([7; 32]))
        );
    }

    #[test]
    fn adding_an_object_leaves_the_snapshot_it_came_from_untouched() {
        let (snapshot, root, _, _) = fixture();
        let extra = ObjectId::from_bytes([5; 16]);
        let grown = snapshot.clone().with_directory(extra, root, name("later"));
        assert!(snapshot.object(extra).is_none());
        assert!(grown.object(extra).is_some());
    }
}
