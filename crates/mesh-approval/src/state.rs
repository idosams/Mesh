//! The workspace state a bundle is computed *between* — and the digest that names one.
//!
//! A review bundle is the difference between two states, so it needs a value that *is* a state.
//! [`WorkspaceState`] mirrors `mesh-conflicts`' base snapshot: one directory entry per object,
//! never a path. `docs/protocol.md` SG-6 and plan §2.6 — an object's identity is not a function of
//! its path, so a moved ancestor changes every descendant's path with no descendant record touched.
//!
//! # Two properties this module exists to hold
//!
//! **A state is immutable.** Every method that changes one consumes it and returns a new one. A
//! state that could be mutated under a bundle in progress would make the bundle a function of when
//! it was read, and a bundle that depends on when it was read cannot be reproduced.
//!
//! **A state has one digest.** [`WorkspaceState::digest`] absorbs the objects in identifier order
//! through the framing in [`crate::digest`], so two processes holding the same objects compute the
//! same 32 bytes, and a bundle can bind "the exact base and actor state" by value rather than by
//! reference to something that may have moved.

use std::collections::BTreeMap;

use crate::digest::{
    Absorb, Blake3, ContentDigest, Digest32, DigestHasher, DigestWriter, DomainTag,
};
use crate::ids::{ObjectId, VersionId};
use crate::name::NormalizedName;

/// How deep a path may be before the walk gives up.
///
/// A state handed in from outside can contain a directory cycle this crate did not create. The
/// guard is what keeps [`WorkspaceState::path_of`] terminating on one — and a path that cannot be
/// derived is a refusal in [`crate::compute_bundle`], never an approximation.
pub const MAX_PATH_DEPTH: usize = 1024;

/// The domain a workspace state's digest is derived in.
const STATE_DOMAIN: DomainTag = DomainTag::new("mesh.v0.workspace-state");

/// Whether an object holds content or other objects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ObjectKind {
    /// Holds content.
    File,
    /// Holds other objects.
    Directory,
}

impl ObjectKind {
    /// The byte this kind is absorbed as.
    const fn tag(self) -> u64 {
        match self {
            Self::File => 0,
            Self::Directory => 1,
        }
    }
}

/// One durable version of one object's content.
///
/// Text and bytes are different kinds of reviewable thing, and which one a version is was decided
/// by whoever wrote it. This crate never guesses: a heuristic that guessed wrong would render bytes
/// as text in the artifact a person approves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Content {
    /// Lines of text, newlines excluded.
    Text {
        /// The identity of this version.
        version: VersionId,
        /// The lines.
        lines: Vec<String>,
    },
    /// Opaque bytes, identified by their content hash.
    Binary {
        /// The identity of this version.
        version: VersionId,
        /// The content hash of the bytes.
        digest: [u8; 32],
        /// How many bytes there are.
        byte_length: u64,
    },
}

impl Content {
    /// The identity of this version.
    #[must_use]
    pub const fn version(&self) -> VersionId {
        match self {
            Self::Text { version, .. } | Self::Binary { version, .. } => *version,
        }
    }

    /// The lines, when this version is text.
    #[must_use]
    pub fn lines(&self) -> Option<&[String]> {
        match self {
            Self::Text { lines, .. } => Some(lines),
            Self::Binary { .. } => None,
        }
    }

    /// Whether this version is opaque bytes.
    #[must_use]
    pub const fn is_binary(&self) -> bool {
        matches!(self, Self::Binary { .. })
    }
}

impl Absorb for Content {
    fn absorb<H: DigestHasher>(&self, writer: &mut DigestWriter<H>) {
        match self {
            Self::Text { version, lines } => {
                writer.u64(0);
                writer.bytes(version.as_bytes());
                writer.sequence(lines, |writer, line| {
                    writer.text(line);
                });
            }
            Self::Binary {
                version,
                digest,
                byte_length,
            } => {
                writer.u64(1);
                writer.bytes(version.as_bytes());
                writer.bytes(digest);
                writer.u64(*byte_length);
            }
        }
    }
}

/// One object as a state holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateObject {
    kind: ObjectKind,
    directory: Option<ObjectId>,
    name: Option<NormalizedName>,
    content: Option<Content>,
}

impl StateObject {
    /// One object as a state holds it.
    ///
    /// `directory` and `name` are absent only for the root; [`crate::compute_bundle`] refuses a
    /// state in which any other object is missing either, because such an object has no derivable
    /// path and nothing that cannot be located can be reviewed.
    #[must_use]
    pub const fn new(
        kind: ObjectKind,
        directory: Option<ObjectId>,
        name: Option<NormalizedName>,
        content: Option<Content>,
    ) -> Self {
        Self {
            kind,
            directory,
            name,
            content,
        }
    }

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

impl Absorb for StateObject {
    fn absorb<H: DigestHasher>(&self, writer: &mut DigestWriter<H>) {
        writer.u64(self.kind.tag());
        writer.option(self.directory.as_ref(), |writer, directory| {
            writer.bytes(directory.as_bytes());
        });
        writer.option(self.name.as_ref(), |writer, name| {
            writer.text(name.as_str());
        });
        writer.option(self.content.as_ref(), |writer, content| {
            content.absorb(writer);
        });
    }
}

/// A workspace state: what every object is, where it hangs and which version it holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceState {
    root: ObjectId,
    objects: BTreeMap<ObjectId, StateObject>,
}

impl WorkspaceState {
    /// An empty workspace with this root directory.
    #[must_use]
    pub fn new(root: ObjectId) -> Self {
        let mut objects = BTreeMap::new();
        objects.insert(
            root,
            StateObject {
                kind: ObjectKind::Directory,
                directory: None,
                name: None,
                content: None,
            },
        );
        Self { root, objects }
    }

    /// This state with a directory added or replaced.
    #[must_use]
    pub fn with_directory(
        self,
        object: ObjectId,
        directory: ObjectId,
        name: NormalizedName,
    ) -> Self {
        self.with_object(object, ObjectKind::Directory, directory, name, None)
    }

    /// This state with a file added or replaced, holding the given content.
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

    /// This state with one object record added or replaced wholesale.
    ///
    /// The exact inverse of [`WorkspaceState::object`], which is what lets a bundle's change list be
    /// replayed onto a base and reproduce the state it was computed from.
    #[must_use]
    pub fn with_record(self, object: ObjectId, record: StateObject) -> Self {
        let mut objects = self.objects;
        objects.insert(object, record);
        Self {
            root: self.root,
            objects,
        }
    }

    /// This state with an object removed. The root cannot be removed.
    #[must_use]
    pub fn without(self, object: ObjectId) -> Self {
        if object == self.root {
            return self;
        }
        let mut objects = self.objects;
        objects.remove(&object);
        Self {
            root: self.root,
            objects,
        }
    }

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
            StateObject {
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

    /// One object, if the state knows it.
    #[must_use]
    pub fn object(&self, object: ObjectId) -> Option<&StateObject> {
        self.objects.get(&object)
    }

    /// How many objects the state holds, the root included.
    #[must_use]
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    /// Whether the state holds nothing but its root.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.objects.len() <= 1
    }

    /// Every object the state holds, in identifier order.
    pub fn objects(&self) -> impl Iterator<Item = (&ObjectId, &StateObject)> {
        self.objects.iter()
    }

    /// The version this object holds, if it holds one.
    #[must_use]
    pub fn version_of(&self, object: ObjectId) -> Option<VersionId> {
        self.objects
            .get(&object)
            .and_then(|found| found.content.as_ref())
            .map(Content::version)
    }

    /// The path this object has, derived by walking directory entries to the root.
    ///
    /// `None` when the object is unknown, when an ancestor is unknown, or when the walk exceeds
    /// [`MAX_PATH_DEPTH`] — which is what a directory cycle looks like from here.
    #[must_use]
    pub fn path_of(&self, object: ObjectId) -> Option<String> {
        let mut segments: Vec<&str> = Vec::new();
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
                    segments.push(name.as_str());
                    at = parent;
                }
                (Some(_), None) => return None,
            }
        }
        None
    }

    /// The 32 bytes that name this state.
    ///
    /// A pure function of the objects and the root, absorbed in identifier order. Two processes
    /// holding the same state compute the same value; a state that differs anywhere computes a
    /// different one.
    #[must_use]
    pub fn digest(&self) -> Digest32 {
        let mut writer = DigestWriter::new(STATE_DOMAIN, Blake3::hasher());
        self.absorb(&mut writer);
        writer.finish()
    }
}

impl Absorb for WorkspaceState {
    fn absorb<H: DigestHasher>(&self, writer: &mut DigestWriter<H>) {
        writer.bytes(self.root.as_bytes());
        writer.u64(self.objects.len() as u64);
        for (object, held) in &self.objects {
            writer.bytes(object.as_bytes());
            held.absorb(writer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(text: &str) -> NormalizedName {
        NormalizedName::new(text).unwrap()
    }

    fn fixture() -> (WorkspaceState, ObjectId, ObjectId, ObjectId) {
        let root = ObjectId::from_bytes([0; 16]);
        let archive = ObjectId::from_bytes([2; 16]);
        let notes = ObjectId::from_bytes([1; 16]);
        let state = WorkspaceState::new(root)
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
        (state, root, archive, notes)
    }

    #[test]
    fn a_path_is_derived_from_directory_entries() {
        let (state, root, archive, notes) = fixture();
        assert_eq!(state.path_of(root).unwrap(), "/");
        assert_eq!(state.path_of(archive).unwrap(), "/archive");
        assert_eq!(state.path_of(notes).unwrap(), "/archive/notes.md");
    }

    #[test]
    fn an_unknown_object_has_no_path() {
        let (state, _, _, _) = fixture();
        assert_eq!(state.path_of(ObjectId::from_bytes([9; 16])), None);
    }

    #[test]
    fn a_directory_cycle_terminates_without_a_path() {
        let root = ObjectId::from_bytes([0; 16]);
        let first = ObjectId::from_bytes([1; 16]);
        let second = ObjectId::from_bytes([2; 16]);
        let state = WorkspaceState::new(root)
            .with_directory(first, second, name("first"))
            .with_directory(second, first, name("second"));
        assert_eq!(state.path_of(first), None);
    }

    #[test]
    fn adding_an_object_leaves_the_state_it_came_from_untouched() {
        let (state, root, _, _) = fixture();
        let extra = ObjectId::from_bytes([5; 16]);
        let grown = state.clone().with_directory(extra, root, name("later"));
        assert!(state.object(extra).is_none());
        assert!(grown.object(extra).is_some());
        assert_ne!(state.digest(), grown.digest());
    }

    #[test]
    fn the_root_cannot_be_removed() {
        let (state, root, _, _) = fixture();
        assert_eq!(state.clone().without(root), state);
    }

    #[test]
    fn the_digest_is_a_function_of_the_state_and_nothing_else() {
        let (state, _, _, _) = fixture();
        let rebuilt = fixture().0;
        assert_eq!(state.digest(), rebuilt.digest());
    }

    #[test]
    fn a_changed_line_moves_the_digest() {
        let (state, _, archive, notes) = fixture();
        let edited = state.clone().with_file(
            notes,
            archive,
            name("notes.md"),
            Content::Text {
                version: VersionId::from_bytes([7; 32]),
                lines: vec!["two".into()],
            },
        );
        assert_ne!(state.digest(), edited.digest());
    }

    #[test]
    fn text_and_binary_report_their_versions() {
        let text = Content::Text {
            version: VersionId::from_bytes([1; 32]),
            lines: vec!["one".into()],
        };
        let binary = Content::Binary {
            version: VersionId::from_bytes([2; 32]),
            digest: [0; 32],
            byte_length: 4,
        };
        assert_eq!(text.version(), VersionId::from_bytes([1; 32]));
        assert_eq!(text.lines(), Some(&["one".to_owned()][..]));
        assert!(!text.is_binary());
        assert_eq!(binary.lines(), None);
        assert!(binary.is_binary());
    }

    #[test]
    fn an_empty_state_holds_only_its_root() {
        let state = WorkspaceState::new(ObjectId::from_bytes([0; 16]));
        assert!(state.is_empty());
        assert_eq!(state.len(), 1);
    }
}
