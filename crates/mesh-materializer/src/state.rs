//! A workspace state: the complete materialization of one workspace at one point in its history.
//!
//! # What is in here, and what is deliberately not
//!
//! `docs/protocol.md` §2.1 defines a workspace state as the root directory version and, transitively,
//! every directory version, file version, file manifest and chunk reference reachable from it. The
//! first three are here. The last two are **not**: a manifest is named by its
//! [`crate::ManifestId`] and a chunk by its content hash, and both live in the content plane that
//! `mesh-cas` owns. Materialization names content; it never holds bytes, and no type in this crate
//! carries any.
//!
//! # The tree is a tree, and it is one because linking says so
//!
//! SG-5 requires directory ancestry inside a state node to be acyclic. This crate makes the
//! stronger structural promise that every object is bound under **at most one name in at most one
//! directory**, held in the [`WorkspaceState::parent_of`] index. Two consequences worth stating
//! because they are choices, not accidents:
//!
//! * A second [`crate::Operation::LinkDirectoryEntry`] for an already-linked object is
//!   [`crate::Rejection::AlreadyLinked`], so there are no hard links. Plan §4.3 does not ask for
//!   them, and a multiply-linked object makes "the name of this object" — which rename, move and
//!   name-conflict resolution all need — ambiguous.
//! * The cycle check is an upward walk of that index, so it costs the depth of the tree rather than
//!   the size of the moved subtree. Moving a million descendants stays one operation and one
//!   entry-map edit; see `docs/plan/execution-plan.md` §11 for the published budget and
//!   `crates/mesh-operations/tests/subtree_move_is_constant_cost.rs` for its vocabulary half. This
//!   crate carries no benchmark and claims no wall-clock number.
//!
//! # Every collection is ordered, and the index is derived
//!
//! `parent_of` is a function of the directory versions and is rebuilt, never trusted:
//! `tests/state_invariants.rs` recomputes it from a full scan after every generated corpus set and
//! requires the two to agree. It is therefore excluded from
//! [`WorkspaceState::canonical_bytes`] — encoding a derived index would let a state hash depend on
//! a cache.

use std::collections::BTreeMap;

use crate::ids::{ActorId, ApprovalId, HeadId, ObjectId, VersionId};
use crate::name::NormalizedName;
use crate::version::{DirectoryVersion, FileVersion, ObjectRecord};

/// The protected shared version, and the approval envelope that moved it there.
///
/// The two are one value because SG-7 makes the canonical head sequence totally ordered and plan
/// §4.7 makes every advance require an approval. A canonical head with no approval beside it is not
/// representable here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanonicalAdvance {
    head: HeadId,
    approval: ApprovalId,
}

impl CanonicalAdvance {
    /// A canonical head and the approval that authorised it.
    #[must_use]
    pub const fn new(head: HeadId, approval: ApprovalId) -> Self {
        Self { head, approval }
    }

    /// The protected shared version.
    #[must_use]
    pub const fn head(&self) -> HeadId {
        self.head
    }

    /// The approval envelope behind it.
    #[must_use]
    pub const fn approval(&self) -> ApprovalId {
        self.approval
    }
}

/// The complete immutable materialization of one workspace at one point in its history.
///
/// Constructed only by [`crate::materialize`]. There is no public mutator: a state a caller could
/// edit would be a state whose hash no longer stands for the operation set that produced it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceState {
    root: ObjectId,
    objects: BTreeMap<ObjectId, ObjectRecord>,
    directories: BTreeMap<ObjectId, DirectoryVersion>,
    file_versions: BTreeMap<VersionId, FileVersion>,
    parent: BTreeMap<ObjectId, ObjectId>,
    actor_heads: BTreeMap<ActorId, HeadId>,
    canonical: Option<CanonicalAdvance>,
}

impl WorkspaceState {
    /// The state of a workspace nothing has happened in: an empty root directory.
    ///
    /// The root exists before any ChangeSet does, so its [`ObjectRecord::created_by`] is `None`.
    #[must_use]
    pub fn empty(root: ObjectId) -> Self {
        let mut objects = BTreeMap::new();
        objects.insert(root, ObjectRecord::root());
        let mut directories = BTreeMap::new();
        directories.insert(root, DirectoryVersion::empty());
        Self {
            root,
            objects,
            directories,
            file_versions: BTreeMap::new(),
            parent: BTreeMap::new(),
            actor_heads: BTreeMap::new(),
            canonical: None,
        }
    }

    /// The workspace root directory object.
    #[must_use]
    pub const fn root(&self) -> ObjectId {
        self.root
    }

    /// The root directory version — the node `docs/protocol.md` §2.1 names a state by.
    ///
    /// Always present: the root is created with the state and no operation can remove an object.
    #[must_use]
    pub fn root_directory(&self) -> &DirectoryVersion {
        self.directories
            .get(&self.root)
            .unwrap_or(&EMPTY_DIRECTORY_FALLBACK)
    }

    /// Every object, in identifier order.
    #[must_use]
    pub const fn objects(&self) -> &BTreeMap<ObjectId, ObjectRecord> {
        &self.objects
    }

    /// One object, if it exists.
    #[must_use]
    pub fn object(&self, id: ObjectId) -> Option<&ObjectRecord> {
        self.objects.get(&id)
    }

    /// Every directory version, in object order.
    #[must_use]
    pub const fn directories(&self) -> &BTreeMap<ObjectId, DirectoryVersion> {
        &self.directories
    }

    /// One directory version, if that object is a directory.
    #[must_use]
    pub fn directory(&self, id: ObjectId) -> Option<&DirectoryVersion> {
        self.directories.get(&id)
    }

    /// Every file version, in version order.
    #[must_use]
    pub const fn file_versions(&self) -> &BTreeMap<VersionId, FileVersion> {
        &self.file_versions
    }

    /// One file version, if it exists.
    #[must_use]
    pub fn file_version(&self, id: VersionId) -> Option<&FileVersion> {
        self.file_versions.get(&id)
    }

    /// The directory `object` is bound in, if it is bound anywhere.
    #[must_use]
    pub fn parent_of(&self, object: ObjectId) -> Option<ObjectId> {
        self.parent.get(&object).copied()
    }

    /// The name `object` is bound under, together with its directory.
    #[must_use]
    pub fn binding_of(&self, object: ObjectId) -> Option<(ObjectId, NormalizedName)> {
        let directory = self.parent_of(object)?;
        let name = self.directories.get(&directory)?.name_of(object)?;
        Some((directory, name))
    }

    /// Every actor head this state has seen advance, in actor order.
    #[must_use]
    pub const fn actor_heads(&self) -> &BTreeMap<ActorId, HeadId> {
        &self.actor_heads
    }

    /// One actor's head, if this state has seen it advance.
    #[must_use]
    pub fn actor_head(&self, actor: ActorId) -> Option<HeadId> {
        self.actor_heads.get(&actor).copied()
    }

    /// The protected shared version, if it has ever been advanced.
    #[must_use]
    pub const fn canonical_head(&self) -> Option<CanonicalAdvance> {
        self.canonical
    }

    /// Whether `candidate` is `object` or one of its ancestors.
    ///
    /// The upward walk that keeps the tree a tree. Bounded by the number of objects, so a state
    /// that somehow held a cycle would still terminate rather than hang — totality is a property of
    /// this function, not of its input.
    #[must_use]
    pub fn is_self_or_ancestor(&self, candidate: ObjectId, object: ObjectId) -> bool {
        let mut at = object;
        let mut steps = 0;
        loop {
            if at == candidate {
                return true;
            }
            let Some(next) = self.parent_of(at) else {
                return false;
            };
            steps += 1;
            if steps > self.objects.len() {
                return true;
            }
            at = next;
        }
    }

    /// The names on the path from the root to `object`, root-first, if it is reachable.
    ///
    /// `None` for an object that is not linked into the tree — a created-but-unlinked object is in
    /// the state and is not at any path, and those are different facts.
    #[must_use]
    pub fn path_of(&self, object: ObjectId) -> Option<Vec<NormalizedName>> {
        if object == self.root {
            return Some(Vec::new());
        }
        let mut names = Vec::new();
        let mut at = object;
        while at != self.root {
            let (directory, name) = self.binding_of(at)?;
            names.push(name);
            if names.len() > self.objects.len() {
                return None;
            }
            at = directory;
        }
        names.reverse();
        Some(names)
    }

    // The mutators below are crate-private. `apply.rs` owns a working copy inside `materialize`
    // and hands out only the finished value, so the type is immutable everywhere it is observable.

    pub(crate) fn insert_object(&mut self, id: ObjectId, record: ObjectRecord) {
        self.objects.insert(id, record);
    }

    pub(crate) fn insert_directory(&mut self, id: ObjectId, version: DirectoryVersion) {
        self.directories.insert(id, version);
    }

    pub(crate) fn insert_file_version(&mut self, id: VersionId, version: FileVersion) {
        self.file_versions.insert(id, version);
    }

    pub(crate) fn bind(
        &mut self,
        directory: ObjectId,
        name: NormalizedName,
        entry: crate::version::DirectoryEntry,
    ) {
        let updated = self
            .directories
            .get(&directory)
            .map_or_else(DirectoryVersion::empty, |current| {
                current.with_entry(name, entry)
            });
        self.directories.insert(directory, updated);
        self.parent.insert(entry.object_id(), directory);
    }

    pub(crate) fn unbind(&mut self, directory: ObjectId, name: &NormalizedName) {
        let Some(current) = self.directories.get(&directory) else {
            return;
        };
        let removed = current.entry(name).map(|entry| entry.object_id());
        let updated = current.without_entry(name);
        self.directories.insert(directory, updated);
        if let Some(object) = removed {
            self.parent.remove(&object);
        }
    }

    pub(crate) fn set_actor_head(&mut self, actor: ActorId, head: HeadId) {
        self.actor_heads.insert(actor, head);
    }

    pub(crate) fn set_canonical(&mut self, advance: CanonicalAdvance) {
        self.canonical = Some(advance);
    }
}

/// A borrowable empty directory, so [`WorkspaceState::root_directory`] never needs to panic or to
/// allocate. The root is always present, so this is unreachable in practice; returning it rather
/// than unwrapping is what makes that statement checked instead of assumed.
static EMPTY_DIRECTORY_FALLBACK: DirectoryVersion = DirectoryVersion::empty();

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ChangeSetId;
    use crate::version::{DirectoryEntry, ObjectKind};

    fn object(byte: u8) -> ObjectId {
        ObjectId::from_bytes([byte; 16])
    }

    fn name(text: &str) -> NormalizedName {
        NormalizedName::new(text).unwrap()
    }

    fn version(byte: u8) -> VersionId {
        VersionId::from_bytes([byte; 32])
    }

    #[test]
    fn an_empty_state_has_a_root_directory_and_nothing_else() {
        let state = WorkspaceState::empty(object(0));
        assert_eq!(state.root(), object(0));
        assert!(state.root_directory().is_empty());
        assert_eq!(state.objects().len(), 1);
        assert_eq!(state.object(object(0)).unwrap().created_by(), None);
        assert_eq!(state.canonical_head(), None);
        assert_eq!(state.path_of(object(0)), Some(Vec::new()));
    }

    #[test]
    fn binding_and_unbinding_keep_the_parent_index_in_step() {
        let mut state = WorkspaceState::empty(object(0));
        state.insert_object(
            object(1),
            ObjectRecord::minted(ObjectKind::File, ChangeSetId::from_bytes([1; 32])),
        );
        state.bind(
            object(0),
            name("a.txt"),
            DirectoryEntry::new(object(1), version(1)),
        );
        assert_eq!(state.parent_of(object(1)), Some(object(0)));
        assert_eq!(
            state.binding_of(object(1)),
            Some((object(0), name("a.txt")))
        );
        assert_eq!(state.path_of(object(1)), Some(vec![name("a.txt")]));

        state.unbind(object(0), &name("a.txt"));
        assert_eq!(state.parent_of(object(1)), None);
        assert_eq!(state.path_of(object(1)), None);
        assert_eq!(state.object(object(1)).unwrap().kind(), ObjectKind::File);
    }

    #[test]
    fn ancestry_answers_both_directions_and_terminates() {
        let mut state = WorkspaceState::empty(object(0));
        for byte in 1..=3u8 {
            state.insert_object(
                object(byte),
                ObjectRecord::minted(ObjectKind::Directory, ChangeSetId::from_bytes([1; 32])),
            );
            state.insert_directory(object(byte), DirectoryVersion::empty());
        }
        state.bind(
            object(0),
            name("a"),
            DirectoryEntry::new(object(1), version(1)),
        );
        state.bind(
            object(1),
            name("b"),
            DirectoryEntry::new(object(2), version(2)),
        );
        state.bind(
            object(2),
            name("c"),
            DirectoryEntry::new(object(3), version(3)),
        );

        assert!(state.is_self_or_ancestor(object(1), object(3)));
        assert!(state.is_self_or_ancestor(object(3), object(3)));
        assert!(!state.is_self_or_ancestor(object(3), object(1)));
        assert_eq!(
            state.path_of(object(3)),
            Some(vec![name("a"), name("b"), name("c")])
        );
    }
}
