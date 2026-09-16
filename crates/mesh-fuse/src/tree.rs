//! One actor's own state, in the shape a FUSE daemon has to keep it.
//!
//! # One sentence this module is built around
//!
//! **An object here is an inode number and a generation, because that is the pair the kernel
//! holds and the pair a FUSE daemon has to be able to invalidate.** Everything else follows:
//! [`mesh_materializer::ObjectId`] is sixteen bytes, so a tree serial, an inode number and a
//! generation fit inside it exactly, and an identifier the kernel cached across a reclaim is
//! answered [`AdapterError::NotFound`] by comparison rather than by luck.
//!
//! # The lock, stated rather than discovered
//!
//! One [`RwLock`] per tree, and **a tree is one actor's state**. That is the whole granularity
//! claim: two actors are two trees and two locks, so nothing an actor does can make another actor
//! wait — which is the epic's "no global actor-workspace lock" exit criterion, measured in
//! `tests/concurrency.rs` rather than asserted here. Inside one tree, readers share and a writer
//! excludes; two `read`s of one actor's files run at once and a `create` does not. That is a real
//! cost and it is not the criterion: it is per-actor serialisation, and finer sharding is
//! follow-up work rather than a claim this crate makes.
//!
//! No method here takes two locks, and no method calls another method that takes one. Deadlock is
//! therefore structural rather than avoided by discipline.
//!
//! # No clock, no filesystem, no environment
//!
//! Nothing in this file reads one. The order at this seam is
//! [`mesh_materializer::EventSequence`], minted per view, exactly as the contract requires.

use std::collections::{BTreeMap, HashMap};
use std::sync::{PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use mesh_materializer::{
    AdapterError, DestinationBefore, EventSequence, NormalizedName, ObjectId, ObjectKind,
    PortableMetadata, RenameBinding, RenameBindingEvidence, RenameDisposition, ViewEntry, ViewId,
};

/// What Linux negotiates as `max_write` by default: 128 KiB.
///
/// A write larger than this is **taken in part**, and the count is reported — which is not a
/// defect but the protocol: the kernel re-issues the remainder at the advanced offset. `kernel.rs`
/// runs that loop and `tests/kernel-retry.rs` grades it.
pub const DEFAULT_MAX_WRITE: usize = 128 * 1024;

/// Split an object identifier back into the three numbers it carries.
///
/// `serial ‖ ino ‖ generation`, big-endian, 4 ‖ 8 ‖ 4 bytes. Big-endian so that the byte order of
/// the identifier is the numeric order of the inode within one tree, which makes a sorted dump of
/// object identifiers readable rather than shuffled.
#[must_use]
pub fn split(object: ObjectId) -> (u32, u64, u32) {
    let bytes = object.as_bytes();
    let mut serial = [0u8; 4];
    let mut ino = [0u8; 8];
    let mut generation = [0u8; 4];
    serial.copy_from_slice(&bytes[0..4]);
    ino.copy_from_slice(&bytes[4..12]);
    generation.copy_from_slice(&bytes[12..16]);
    (
        u32::from_be_bytes(serial),
        u64::from_be_bytes(ino),
        u32::from_be_bytes(generation),
    )
}

/// Build an object identifier from the three numbers a FUSE daemon holds.
#[must_use]
pub fn join(serial: u32, ino: u64, generation: u32) -> ObjectId {
    let mut bytes = [0u8; 16];
    bytes[0..4].copy_from_slice(&serial.to_be_bytes());
    bytes[4..12].copy_from_slice(&ino.to_be_bytes());
    bytes[12..16].copy_from_slice(&generation.to_be_bytes());
    ObjectId::from_bytes(bytes)
}

/// One object.
#[derive(Clone, Debug)]
struct Node {
    generation: u32,
    kind: ObjectKind,
    metadata: PortableMetadata,
    /// Byte-lexicographic by construction: Rust orders `String` by its UTF-8 bytes, which is the
    /// order [`mesh_materializer::WorkspaceView::enumerate`] publishes. A `HashMap` plus a sort at
    /// read time would answer the same sequence and would let an unsorted answer through the day
    /// somebody forgot the sort.
    entries: BTreeMap<String, u64>,
    content: Vec<u8>,
    /// Whether a name still binds this object. An unlinked file with open handles stays reachable
    /// through its handles and unreachable by name, which is POSIX and is what an editor writing
    /// over an open file depends on.
    bound: bool,
    handles: usize,
    modified_handles: usize,
}

impl Node {
    fn directory(generation: u32) -> Self {
        Self {
            generation,
            kind: ObjectKind::Directory,
            metadata: PortableMetadata::default(),
            entries: BTreeMap::new(),
            content: Vec::new(),
            bound: true,
            handles: 0,
            modified_handles: 0,
        }
    }

    fn of(generation: u32, kind: ObjectKind, metadata: PortableMetadata) -> Self {
        Self {
            generation,
            kind,
            metadata,
            entries: BTreeMap::new(),
            content: Vec::new(),
            bound: true,
            handles: 0,
            modified_handles: 0,
        }
    }
}

/// Everything one tree holds, behind one lock.
#[derive(Debug)]
struct Nodes {
    by_ino: HashMap<u64, Node>,
    next_ino: u64,
    /// Inode numbers a reclaim gave back, each with the generation its next occupant gets.
    free: Vec<(u64, u32)>,
}

/// One actor's own state, or one head presented read-only.
#[derive(Debug)]
pub struct Tree {
    serial: u32,
    root_ino: u64,
    max_write: usize,
    nodes: RwLock<Nodes>,
}

fn read(nodes: &RwLock<Nodes>) -> RwLockReadGuard<'_, Nodes> {
    nodes.read().unwrap_or_else(PoisonError::into_inner)
}

fn write(nodes: &RwLock<Nodes>) -> RwLockWriteGuard<'_, Nodes> {
    nodes.write().unwrap_or_else(PoisonError::into_inner)
}

impl Nodes {
    fn get(&self, serial: u32, object: ObjectId) -> Result<&Node, AdapterError> {
        let (tree, ino, generation) = split(object);
        if tree != serial {
            return Err(AdapterError::NotFound);
        }
        match self.by_ino.get(&ino) {
            // The generation is the whole point of carrying one: an identifier the kernel cached
            // before a reclaim names an inode that exists and an object that does not.
            Some(node) if node.generation == generation => Ok(node),
            _ => Err(AdapterError::NotFound),
        }
    }

    fn get_mut(&mut self, serial: u32, object: ObjectId) -> Result<&mut Node, AdapterError> {
        let (tree, ino, generation) = split(object);
        if tree != serial {
            return Err(AdapterError::NotFound);
        }
        match self.by_ino.get_mut(&ino) {
            Some(node) if node.generation == generation => Ok(node),
            _ => Err(AdapterError::NotFound),
        }
    }

    fn directory(&self, serial: u32, object: ObjectId) -> Result<&Node, AdapterError> {
        let node = self.get(serial, object)?;
        if node.kind == ObjectKind::Directory {
            Ok(node)
        } else {
            Err(AdapterError::NotADirectory)
        }
    }

    fn allocate(&mut self, make: impl FnOnce(u32) -> Node) -> (u64, u32) {
        let (ino, generation) = match self.free.pop() {
            Some(reused) => reused,
            None => {
                let ino = self.next_ino;
                self.next_ino += 1;
                (ino, 1)
            }
        };
        self.by_ino.insert(ino, make(generation));
        (ino, generation)
    }

    /// Give an inode number back, bumping the generation its next occupant will carry.
    ///
    /// A saturating add rather than a wrap: at `u32::MAX` the number is retired instead of
    /// re-issued, because a wrapped generation is a stale identifier that resolves.
    fn reclaim(&mut self, ino: u64, generation: u32) {
        self.by_ino.remove(&ino);
        if let Some(next) = generation.checked_add(1) {
            self.free.push((ino, next));
        }
    }

    fn entry(&self, serial: u32, name: &str, ino: u64) -> Result<ViewEntry, AdapterError> {
        let node = self.by_ino.get(&ino).ok_or(AdapterError::NotFound)?;
        let name = NormalizedName::new(name).map_err(AdapterError::NameRejected)?;
        Ok(ViewEntry::new(
            name,
            join(serial, ino, node.generation),
            node.kind,
            // No version: nothing here has been checkpointed, and a file that exists and names
            // nothing durable is a real state rather than an error.
            None,
            node.metadata,
        ))
    }
}

impl Tree {
    /// An empty tree whose root is a directory.
    #[must_use]
    pub fn new(serial: u32, max_write: usize) -> Self {
        let mut nodes = Nodes {
            by_ino: HashMap::new(),
            next_ino: 1,
            free: Vec::new(),
        };
        let (root_ino, _) = nodes.allocate(Node::directory);
        Self {
            serial,
            root_ino,
            max_write: max_write.max(1),
            nodes: RwLock::new(nodes),
        }
    }

    /// The object at the top of this tree.
    #[must_use]
    pub fn root(&self) -> ObjectId {
        let nodes = read(&self.nodes);
        let generation = nodes
            .by_ino
            .get(&self.root_ino)
            .map_or(1, |node| node.generation);
        join(self.serial, self.root_ino, generation)
    }

    /// The largest number of bytes one write is taken in — FUSE's negotiated `max_write`.
    #[must_use]
    pub const fn max_write(&self) -> usize {
        self.max_write
    }

    /// Resolve `name` inside `parent`.
    ///
    /// # Errors
    ///
    /// `NotFound` if nothing is bound, `NotADirectory` if `parent` is a file.
    pub fn lookup(
        &self,
        parent: ObjectId,
        name: &NormalizedName,
    ) -> Result<ViewEntry, AdapterError> {
        let nodes = read(&self.nodes);
        let directory = nodes.directory(self.serial, parent)?;
        let ino = *directory
            .entries
            .get(name.as_str())
            .ok_or(AdapterError::NotFound)?;
        nodes.entry(self.serial, name.as_str(), ino)
    }

    /// Every entry in `directory`, in ascending byte-lexicographic order of the entry names.
    ///
    /// # Errors
    ///
    /// `NotFound` if there is no such object, `NotADirectory` if it is a file.
    pub fn enumerate(&self, directory: ObjectId) -> Result<Vec<ViewEntry>, AdapterError> {
        let nodes = read(&self.nodes);
        let node = nodes.directory(self.serial, directory)?;
        node.entries
            .iter()
            .map(|(name, ino)| nodes.entry(self.serial, name, *ino))
            .collect()
    }

    /// What kind of object this is.
    ///
    /// # Errors
    ///
    /// `NotFound` if there is no such object.
    pub fn kind(&self, object: ObjectId) -> Result<ObjectKind, AdapterError> {
        Ok(read(&self.nodes).get(self.serial, object)?.kind)
    }

    /// The portable metadata.
    ///
    /// # Errors
    ///
    /// `NotFound` if there is no such object.
    pub fn metadata(&self, object: ObjectId) -> Result<PortableMetadata, AdapterError> {
        Ok(read(&self.nodes).get(self.serial, object)?.metadata)
    }

    /// Set the portable metadata.
    ///
    /// # Errors
    ///
    /// `NotFound` if there is no such object.
    pub fn set_metadata(
        &self,
        object: ObjectId,
        metadata: PortableMetadata,
    ) -> Result<(), AdapterError> {
        write(&self.nodes).get_mut(self.serial, object)?.metadata = metadata;
        Ok(())
    }

    /// Note that a handle has been taken, so an unlink cannot reclaim the object underneath it.
    ///
    /// # Errors
    ///
    /// `NotFound` if there is no such object, `IsADirectory` if it is a directory.
    pub fn acquire(&self, object: ObjectId) -> Result<(), AdapterError> {
        let mut nodes = write(&self.nodes);
        let node = nodes.get_mut(self.serial, object)?;
        if node.kind == ObjectKind::Directory {
            return Err(AdapterError::IsADirectory);
        }
        node.handles += 1;
        Ok(())
    }

    /// Mark one newly modified handle on `object`.
    ///
    /// # Errors
    ///
    /// `NotFound` if there is no such object.
    pub fn mark_handle_modified(&self, object: ObjectId) -> Result<(), AdapterError> {
        let mut nodes = write(&self.nodes);
        nodes.get_mut(self.serial, object)?.modified_handles += 1;
        Ok(())
    }

    /// Modified handles across every view of this tree.
    ///
    /// # Errors
    ///
    /// `NotFound` if there is no such object.
    pub fn modified_handle_count(&self, object: ObjectId) -> Result<usize, AdapterError> {
        Ok(read(&self.nodes).get(self.serial, object)?.modified_handles)
    }

    /// Give a handle up, reclaiming the object if nothing binds it and nothing else holds it.
    ///
    /// A missing object is not an error here: the caller is giving up a handle on something that
    /// is already gone, which is the ordinary end of "unlink an open file, then close it".
    pub fn release(&self, object: ObjectId, modified: bool) {
        let mut nodes = write(&self.nodes);
        let (_, ino, generation) = split(object);
        let Ok(node) = nodes.get_mut(self.serial, object) else {
            return;
        };
        node.handles = node.handles.saturating_sub(1);
        if modified {
            node.modified_handles = node.modified_handles.saturating_sub(1);
        }
        let reclaim = node.handles == 0 && !node.bound;
        if reclaim {
            nodes.reclaim(ino, generation);
        }
    }

    /// Fill `into` from `offset`, answering how many bytes were filled.
    ///
    /// # Errors
    ///
    /// `NotFound` if there is no such object, `IsADirectory` if it is a directory.
    pub fn read_at(
        &self,
        object: ObjectId,
        offset: u64,
        into: &mut [u8],
    ) -> Result<usize, AdapterError> {
        let nodes = read(&self.nodes);
        let node = nodes.get(self.serial, object)?;
        if node.kind == ObjectKind::Directory {
            return Err(AdapterError::IsADirectory);
        }
        let Ok(start) = usize::try_from(offset) else {
            return Ok(0);
        };
        if start >= node.content.len() {
            return Ok(0);
        }
        let available = &node.content[start..];
        let filled = available.len().min(into.len());
        into[..filled].copy_from_slice(&available[..filled]);
        Ok(filled)
    }

    /// Take bytes from `from` at `offset`, answering how many were taken.
    ///
    /// **At most [`Tree::max_write`] bytes.** A larger write is a short write and says so; the
    /// kernel issues the remainder at the advanced offset, and `kernel.rs` runs that loop.
    ///
    /// # Errors
    ///
    /// `NotFound` if there is no such object, `IsADirectory` if it is a directory.
    pub fn write_at(
        &self,
        object: ObjectId,
        offset: u64,
        from: &[u8],
    ) -> Result<usize, AdapterError> {
        let limit = self.max_write;
        let mut nodes = write(&self.nodes);
        let serial = self.serial;
        let node = nodes.get_mut(serial, object)?;
        if node.kind == ObjectKind::Directory {
            return Err(AdapterError::IsADirectory);
        }
        let Ok(start) = usize::try_from(offset) else {
            return Err(AdapterError::Backend(
                "an offset that does not fit this machine's address space".to_owned(),
            ));
        };
        let taken = from.len().min(limit);
        let end = start + taken;
        if node.content.len() < end {
            node.content.resize(end, 0);
        }
        node.content[start..end].copy_from_slice(&from[..taken]);
        Ok(taken)
    }

    /// Set a file to exactly `length` bytes.
    ///
    /// Shrinking discards the old tail. Growing preserves the existing prefix and fills the new
    /// range with zeroes, matching the workspace-adapter contract and ordinary filesystem
    /// semantics.
    ///
    /// # Errors
    ///
    /// `NotFound` if there is no such object, `IsADirectory` if it is a directory, or `Backend`
    /// when this host cannot address the requested length.
    pub fn set_file_length(&self, object: ObjectId, length: u64) -> Result<(), AdapterError> {
        let target = usize::try_from(length)
            .map_err(|_| AdapterError::Backend("file length does not fit this host".to_owned()))?;
        let mut nodes = write(&self.nodes);
        let node = nodes.get_mut(self.serial, object)?;
        if node.kind == ObjectKind::Directory {
            return Err(AdapterError::IsADirectory);
        }
        node.content.resize(target, 0);
        Ok(())
    }

    /// Bind a new object under `name` in `parent`.
    ///
    /// # Errors
    ///
    /// `AlreadyExists` if the name is taken, `NotFound`/`NotADirectory` for the parent.
    pub fn create(
        &self,
        parent: ObjectId,
        name: &NormalizedName,
        kind: ObjectKind,
        metadata: PortableMetadata,
    ) -> Result<ViewEntry, AdapterError> {
        let mut nodes = write(&self.nodes);
        let serial = self.serial;
        let (_, parent_ino, _) = split(parent);
        let taken = nodes
            .directory(serial, parent)?
            .entries
            .contains_key(name.as_str());
        if taken {
            return Err(AdapterError::AlreadyExists);
        }
        let (ino, _) = nodes.allocate(|generation| Node::of(generation, kind, metadata));
        if let Some(node) = nodes.by_ino.get_mut(&parent_ino) {
            node.entries.insert(name.as_str().to_owned(), ino);
        }
        nodes.entry(serial, name.as_str(), ino)
    }

    /// Change an entry's name within one directory.
    ///
    /// # Errors
    ///
    /// `NotFound` if `from` is not bound, `AlreadyExists` if `to` is.
    pub fn rename(
        &self,
        parent: ObjectId,
        from: &NormalizedName,
        to: &NormalizedName,
    ) -> Result<(), AdapterError> {
        self.rename_with_evidence(
            ViewId::new(0),
            EventSequence::new(0),
            parent,
            from,
            parent,
            to,
            RenameDisposition::Fail,
        )
        .map(|_| ())
    }

    /// Move an entry between two directories.
    ///
    /// # Errors
    ///
    /// `WouldCycle` if the move would place a directory inside its own subtree; otherwise as
    /// [`Tree::rename`].
    pub fn move_entry(
        &self,
        from_parent: ObjectId,
        from: &NormalizedName,
        to_parent: ObjectId,
        to: &NormalizedName,
    ) -> Result<(), AdapterError> {
        self.rename_with_evidence(
            ViewId::new(0),
            EventSequence::new(0),
            from_parent,
            from,
            to_parent,
            to,
            RenameDisposition::Fail,
        )
        .map(|_| ())
    }

    /// Rename or move with contract-1 replacement semantics and exact binding evidence.
    ///
    /// The one write guard makes replacement atomic to every reader of this tree.
    #[allow(clippy::too_many_arguments)]
    pub fn rename_with_evidence(
        &self,
        view: ViewId,
        sequence: EventSequence,
        from_parent: ObjectId,
        from: &NormalizedName,
        to_parent: ObjectId,
        to: &NormalizedName,
        disposition: RenameDisposition,
    ) -> Result<RenameBindingEvidence, AdapterError> {
        let mut nodes = write(&self.nodes);
        let serial = self.serial;
        let (_, from_ino, _) = split(from_parent);
        let (_, to_ino, _) = split(to_parent);
        let moving = *nodes
            .directory(serial, from_parent)?
            .entries
            .get(from.as_str())
            .ok_or(AdapterError::NotFound)?;
        let moving_node = nodes.by_ino.get(&moving).ok_or(AdapterError::NotFound)?;
        let moving_object = join(serial, moving, moving_node.generation);
        let moving_a_directory = moving_node.kind == ObjectKind::Directory;
        let destination = nodes
            .directory(serial, to_parent)?
            .entries
            .get(to.as_str())
            .copied();
        // The cycle test first: a move that would swallow its own subtree is refused whatever the
        // destination name is, and reporting `AlreadyExists` for it would send the caller off to
        // pick another name for a move that can never succeed.
        if moving_a_directory && is_within(&nodes, moving, to_ino) {
            return Err(AdapterError::WouldCycle);
        }
        let same_binding = from_ino == to_ino && from.as_str() == to.as_str();
        if same_binding {
            let binding = RenameBinding::new(from_parent, from.clone(), moving_object);
            return Ok(RenameBindingEvidence::new(
                view,
                sequence,
                binding.clone(),
                DestinationBefore::Bound(moving_object),
                binding,
            ));
        }

        let destination_object = match destination {
            Some(ino) => Some(join(
                serial,
                ino,
                nodes
                    .by_ino
                    .get(&ino)
                    .ok_or(AdapterError::NotFound)?
                    .generation,
            )),
            None => None,
        };
        if destination.is_some() && disposition == RenameDisposition::Fail {
            return Err(AdapterError::AlreadyExists);
        }
        if let Some(displaced) = destination {
            let displaced_kind = nodes
                .by_ino
                .get(&displaced)
                .ok_or(AdapterError::NotFound)?
                .kind;
            if displaced_kind == ObjectKind::Directory {
                return Err(AdapterError::IsADirectory);
            }
            if moving_a_directory {
                return Err(AdapterError::NotADirectory);
            }
        }
        if let Some(node) = nodes.by_ino.get_mut(&from_ino) {
            node.entries.remove(from.as_str());
        }
        if let Some(node) = nodes.by_ino.get_mut(&to_ino) {
            node.entries.insert(to.as_str().to_owned(), moving);
        }

        if let Some(displaced) = destination.filter(|displaced| *displaced != moving) {
            let displaced_state = nodes
                .by_ino
                .get(&displaced)
                .map(|node| (node.generation, node.handles == 0));
            if let Some((generation, true)) = displaced_state {
                nodes.reclaim(displaced, generation);
            } else if let Some(node) = nodes.by_ino.get_mut(&displaced) {
                node.bound = false;
            }
        }

        Ok(RenameBindingEvidence::new(
            view,
            sequence,
            RenameBinding::new(from_parent, from.clone(), moving_object),
            destination_object.map_or(DestinationBefore::Unbound, DestinationBefore::Bound),
            RenameBinding::new(to_parent, to.clone(), moving_object),
        ))
    }

    /// Remove a file's name binding.
    ///
    /// # Errors
    ///
    /// `NotFound` if nothing is bound, `IsADirectory` if the name is bound to a directory.
    pub fn unlink(&self, parent: ObjectId, name: &NormalizedName) -> Result<(), AdapterError> {
        self.unbind(parent, name, ObjectKind::File)
    }

    /// Remove an empty directory's name binding. Never a recursive delete.
    ///
    /// # Errors
    ///
    /// `DirectoryNotEmpty` if it still has entries, `NotADirectory` if the name is bound to a
    /// file, `NotFound` if nothing is bound.
    pub fn remove_directory(
        &self,
        parent: ObjectId,
        name: &NormalizedName,
    ) -> Result<(), AdapterError> {
        self.unbind(parent, name, ObjectKind::Directory)
    }

    fn unbind(
        &self,
        parent: ObjectId,
        name: &NormalizedName,
        wanted: ObjectKind,
    ) -> Result<(), AdapterError> {
        let mut nodes = write(&self.nodes);
        let serial = self.serial;
        let ino = *nodes
            .directory(serial, parent)?
            .entries
            .get(name.as_str())
            .ok_or(AdapterError::NotFound)?;
        let node = nodes.by_ino.get(&ino).ok_or(AdapterError::NotFound)?;
        if node.kind != wanted {
            return Err(match wanted {
                ObjectKind::File => AdapterError::IsADirectory,
                ObjectKind::Directory => AdapterError::NotADirectory,
            });
        }
        if wanted == ObjectKind::Directory && !node.entries.is_empty() {
            return Err(AdapterError::DirectoryNotEmpty);
        }
        let (generation, orphaned) = (node.generation, node.handles == 0);
        let (_, parent_ino, _) = split(parent);
        if let Some(directory) = nodes.by_ino.get_mut(&parent_ino) {
            directory.entries.remove(name.as_str());
        }
        if orphaned {
            nodes.reclaim(ino, generation);
        } else if let Some(node) = nodes.by_ino.get_mut(&ino) {
            // Unlinked with a handle still open: unreachable by name, readable through the handle.
            node.bound = false;
        }
        Ok(())
    }
}

/// Whether `candidate` is `ancestor` or lives inside its subtree.
///
/// Walks down from the ancestor rather than up from the candidate, because a node carries its
/// entries and not its parent — one direction is enough, and keeping one removes the parent
/// pointer that a move would otherwise have to keep in step with the entry map.
fn is_within(nodes: &Nodes, ancestor: u64, candidate: u64) -> bool {
    let mut frontier = vec![ancestor];
    let mut seen = Vec::new();
    while let Some(ino) = frontier.pop() {
        if ino == candidate {
            return true;
        }
        if seen.contains(&ino) {
            continue;
        }
        seen.push(ino);
        if let Some(node) = nodes.by_ino.get(&ino) {
            frontier.extend(node.entries.values().copied());
        }
    }
    false
}
