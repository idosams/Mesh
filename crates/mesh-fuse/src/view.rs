//! One mountpoint's worth of filesystem: the [`WorkspaceView`] a FUSE session serves.
//!
//! # A view is a mountpoint, a tree is the state
//!
//! Two mounts of one actor are **two views onto one [`Tree`]**, which is what a second mountpoint
//! of one filesystem is on any operating system. Handles are per view, because
//! [`OpenHandle`] names the view that issued it and a handle taken through one mount is not a
//! handle the other mount may close.
//!
//! # The refusal order is a decision
//!
//! [`ViewAccess::ReadOnly`] is checked **before** the handle is looked at, in every mutating
//! method. `RO/write-is-refused` hands a read-only view a handle it never issued and requires
//! exactly [`AdapterError::ReadOnly`]; a backend that validated the handle first would answer
//! `NotFound` and tell the caller the file is missing when the truth is that the view will not
//! take a write. The refusal has to say why the view refused, not why the handle was odd.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use mesh_materializer::{
    AdapterError, EventSequence, NormalizedName, ObjectId, ObjectKind, OpenHandle, OpenMode,
    PortableMetadata, RenameBindingEvidence, RenameDisposition, ViewAccess, ViewEntry, ViewId,
    WorkspaceView,
};

use crate::tree::Tree;

/// One open handle, as this view remembers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Held {
    object: ObjectId,
    mode: OpenMode,
    modified: bool,
}

/// One mounted or presented filesystem.
#[derive(Debug)]
pub struct FuseView {
    id: ViewId,
    access: ViewAccess,
    tree: &'static Tree,
    handles: Mutex<HashMap<u64, Held>>,
    next_handle: AtomicU64,
    /// Whether [`crate::FuseAdapter::release`] has given this view up.
    ///
    /// An atomic on the view rather than a set on the adapter, so resolving a view identifier
    /// takes one shared read of an append-only vector and no exclusive lock at all — the
    /// registry must not become the global lock the tree granularity was chosen to avoid.
    released: AtomicBool,
}

fn held(handles: &Mutex<HashMap<u64, Held>>) -> MutexGuard<'_, HashMap<u64, Held>> {
    handles.lock().unwrap_or_else(PoisonError::into_inner)
}

impl FuseView {
    /// A view of `tree` at `id`, accepting mutation or not.
    #[must_use]
    pub fn new(id: ViewId, access: ViewAccess, tree: &'static Tree) -> Self {
        Self {
            id,
            access,
            tree,
            handles: Mutex::new(HashMap::new()),
            next_handle: AtomicU64::new(1),
            released: AtomicBool::new(false),
        }
    }

    /// The state this view presents.
    #[must_use]
    pub const fn tree(&self) -> &'static Tree {
        self.tree
    }

    /// Whether this view has been given up.
    #[must_use]
    pub fn is_released(&self) -> bool {
        self.released.load(Ordering::SeqCst)
    }

    /// Give this view up, answering whether this call was the one that did it.
    ///
    /// A compare-and-exchange rather than a store, so two callers releasing at once produce one
    /// success and one [`AdapterError::UnknownView`] — releasing twice is an error, and a caller
    /// that double-releases has lost track of something.
    pub fn take_release(&self) -> bool {
        self.released
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    /// How many handles are open through this view.
    #[must_use]
    pub fn open_handles(&self) -> usize {
        held(&self.handles).len()
    }

    /// `Ok` when this view accepts mutation.
    fn writable(&self) -> Result<(), AdapterError> {
        if self.access.is_read_only() {
            Err(AdapterError::ReadOnly)
        } else {
            Ok(())
        }
    }

    /// The object a handle this view issued is open on.
    ///
    /// A handle from another view, or one already closed, is `NotFound`: it names nothing here.
    fn resolve(&self, handle: &OpenHandle) -> Result<Held, AdapterError> {
        if handle.view() != self.id {
            return Err(AdapterError::NotFound);
        }
        held(&self.handles)
            .get(&handle.handle())
            .copied()
            .ok_or(AdapterError::NotFound)
    }
}

impl WorkspaceView for FuseView {
    fn id(&self) -> ViewId {
        self.id
    }

    fn access(&self) -> ViewAccess {
        self.access
    }

    fn root(&self) -> ObjectId {
        self.tree.root()
    }

    fn lookup(&self, parent: ObjectId, name: &NormalizedName) -> Result<ViewEntry, AdapterError> {
        self.tree.lookup(parent, name)
    }

    fn enumerate(&self, directory: ObjectId) -> Result<Vec<ViewEntry>, AdapterError> {
        self.tree.enumerate(directory)
    }

    fn open(&self, object: ObjectId, mode: OpenMode) -> Result<OpenHandle, AdapterError> {
        if mode.writes() {
            self.writable()?;
        }
        self.tree.acquire(object)?;
        let handle = self.next_handle.fetch_add(1, Ordering::SeqCst);
        held(&self.handles).insert(
            handle,
            Held {
                object,
                mode,
                modified: false,
            },
        );
        Ok(OpenHandle::new(handle, self.id, object, mode))
    }

    fn close(&self, handle: OpenHandle) -> Result<(), AdapterError> {
        if handle.view() != self.id {
            return Err(AdapterError::NotFound);
        }
        let removed = held(&self.handles)
            .remove(&handle.handle())
            .ok_or(AdapterError::NotFound)?;
        self.tree.release(removed.object, removed.modified);
        Ok(())
    }

    fn read(
        &self,
        handle: &OpenHandle,
        offset: u64,
        into: &mut [u8],
    ) -> Result<usize, AdapterError> {
        let open = self.resolve(handle)?;
        if !open.mode.reads() {
            return Err(AdapterError::NotFound);
        }
        self.tree.read_at(open.object, offset, into)
    }

    fn write(&self, handle: &OpenHandle, offset: u64, from: &[u8]) -> Result<usize, AdapterError> {
        self.writable()?;
        let open = self.resolve(handle)?;
        if !open.mode.writes() {
            return Err(AdapterError::NotFound);
        }
        let taken = self.tree.write_at(open.object, offset, from)?;
        if taken > 0 {
            if let Some(held) = held(&self.handles).get_mut(&handle.handle()) {
                if !held.modified {
                    self.tree.mark_handle_modified(open.object)?;
                    held.modified = true;
                }
            }
        }
        Ok(taken)
    }

    fn set_file_length(&self, object: ObjectId, length: u64) -> Result<(), AdapterError> {
        self.writable()?;
        self.tree.set_file_length(object, length)
    }

    fn create_file(
        &self,
        parent: ObjectId,
        name: &NormalizedName,
        metadata: PortableMetadata,
    ) -> Result<ViewEntry, AdapterError> {
        self.writable()?;
        self.tree.create(parent, name, ObjectKind::File, metadata)
    }

    fn create_directory(
        &self,
        parent: ObjectId,
        name: &NormalizedName,
    ) -> Result<ViewEntry, AdapterError> {
        self.writable()?;
        self.tree.create(
            parent,
            name,
            ObjectKind::Directory,
            PortableMetadata::default(),
        )
    }

    fn rename(
        &self,
        parent: ObjectId,
        from: &NormalizedName,
        to: &NormalizedName,
    ) -> Result<(), AdapterError> {
        self.writable()?;
        self.tree.rename(parent, from, to)
    }

    fn rename_with_evidence(
        &self,
        sequence: EventSequence,
        parent: ObjectId,
        from: &NormalizedName,
        to: &NormalizedName,
        disposition: RenameDisposition,
    ) -> Result<RenameBindingEvidence, AdapterError> {
        self.writable()?;
        self.tree
            .rename_with_evidence(self.id, sequence, parent, from, parent, to, disposition)
    }

    fn move_entry(
        &self,
        from_parent: ObjectId,
        from: &NormalizedName,
        to_parent: ObjectId,
        to: &NormalizedName,
    ) -> Result<(), AdapterError> {
        self.writable()?;
        self.tree.move_entry(from_parent, from, to_parent, to)
    }

    fn move_entry_with_evidence(
        &self,
        sequence: EventSequence,
        from_parent: ObjectId,
        from: &NormalizedName,
        to_parent: ObjectId,
        to: &NormalizedName,
        disposition: RenameDisposition,
    ) -> Result<RenameBindingEvidence, AdapterError> {
        self.writable()?;
        self.tree.rename_with_evidence(
            self.id,
            sequence,
            from_parent,
            from,
            to_parent,
            to,
            disposition,
        )
    }

    fn unlink(&self, parent: ObjectId, name: &NormalizedName) -> Result<(), AdapterError> {
        self.writable()?;
        self.tree.unlink(parent, name)
    }

    fn remove_directory(
        &self,
        parent: ObjectId,
        name: &NormalizedName,
    ) -> Result<(), AdapterError> {
        self.writable()?;
        self.tree.remove_directory(parent, name)
    }

    fn metadata(&self, object: ObjectId) -> Result<PortableMetadata, AdapterError> {
        self.tree.metadata(object)
    }

    fn set_metadata(
        &self,
        object: ObjectId,
        metadata: PortableMetadata,
    ) -> Result<(), AdapterError> {
        self.writable()?;
        self.tree.set_metadata(object, metadata)
    }
}
