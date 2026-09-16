//! One in-memory backend, and sixteen deliberately broken copies of it.
//!
//! # What this is for
//!
//! A conformance case only ever observed to pass is not evidence that it can fail. Each [`Mutant`]
//! below is a plausible backend defect — not a crash, not a syntax error, the kind of thing a
//! backend author ships by accident — and each names the one case that must catch it. The suite is
//! run against the unmutated adapter (which must be conformant) and against every mutant (each of
//! which must fail *its own* case).
//!
//! # What it is not
//!
//! It is not a reference implementation of a Mesh filesystem, and passing here is not a filesystem
//! result: this is memory, with no mount, no kernel, no concurrency beyond a mutex, and it shares
//! the suite's assumptions. Stated rather than hidden — design `01KZEZGDPMZ5RH7E60WDYDYYEE`
//! §Risks says the same thing, and the first real evidence is the FUSE lane
//! (`01KZC2ZTN3YXE6NM270T001RAK`) passing.
//!
//! # Two implementation notes a backend author may find useful
//!
//! - **Views are leaked on purpose.** `WorkspaceAdapter::view` hands out a borrow of `&self`, so a
//!   backend that mints views at run time needs them at stable addresses. `Box::leak` is the
//!   cheapest way to get that with no `unsafe` and no dependency; a real backend would use an
//!   arena or an append-only store. `release` unregisters an identifier and never frees what a
//!   borrow may still point at, which is what the contract actually requires.
//! - **The capability check comes first, everywhere.** An undeclared capability answers
//!   `Unsupported` even when some other refusal (`ReadOnly`, `NotFound`) would also have been
//!   true, because the published contract says an undeclared capability's only legal answer is
//!   `Unsupported`.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use mesh_materializer::{
    ActorId, AdapterCapability, AdapterDescription, AdapterError, AdapterFixture, BoundaryReason,
    CapabilitySet, CheckpointCandidate, FsEvent, FsEventKind, HeadId, MaterializedView,
    MountedView, NameError, NormalizedName, ObjectId, ObjectKind, OpenHandle, OpenMode,
    PortableMetadata, ViewAccess, ViewEntry, ViewId, WorkspaceAdapter, WorkspaceId, WorkspaceView,
    WORKSPACE_ADAPTER_CONTRACT,
};

// ---------------------------------------------------------------------------------------------
// The sixteen mutants
// ---------------------------------------------------------------------------------------------

/// One plausible defect, and the one case that must catch it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mutant {
    /// `unlink` reports success and removes nothing.
    LookupAfterUnlinkSucceeds,
    /// `write` reports taking every byte and stores half of them.
    WriteSilentlyTruncated,
    /// A non-zero shrink reports success and leaves the old tail readable.
    SetLengthLeavesTail,
    /// Growing a file zeroes its existing non-zero prefix as well as the new range.
    SetLengthGrowthClobbersPrefix,
    /// `rename` resets the object's portable metadata.
    RenameLosesMetadata,
    /// `move_entry` does not check for a cycle.
    MoveIntoOwnSubtreeAllowed,
    /// `create_file` over a bound name replaces the entry instead of refusing.
    CreateOverExistingNameSucceeds,
    /// A read-only view's `write` reports success.
    ReadonlyViewAcceptsAWrite,
    /// `Write` is not declared, and `write` answers `Ok(n)` while dropping the bytes.
    UndeclaredCapabilityReturnsOk,
    /// `Write` is not declared, and `write` unwinds instead of refusing.
    UndeclaredCapabilityPanics,
    /// `Enumerate` is declared and `enumerate` answers `Unsupported`.
    DeclaredCapabilityAnswersUnsupported,
    /// `enumerate` answers in insertion order.
    EnumerateOrderIsInsertionOrder,
    /// `remove_directory` removes a directory that still has entries.
    RemoveNonemptyDirectorySucceeds,
    /// One close offers two candidates, because a flush offers one too.
    BoundaryObservedTwiceForOneClose,
    /// A mountpoint holding `..` is accepted.
    DotdotAcceptedAsAName,
    /// A released view identifier still resolves.
    ReleasedViewStillResolves,
}

impl Mutant {
    /// Every mutant, in the order design `01KZEZGDPMZ5RH7E60WDYDYYEE` Contract 11 lists them.
    pub const ALL: [Self; 16] = [
        Self::LookupAfterUnlinkSucceeds,
        Self::WriteSilentlyTruncated,
        Self::SetLengthLeavesTail,
        Self::SetLengthGrowthClobbersPrefix,
        Self::RenameLosesMetadata,
        Self::MoveIntoOwnSubtreeAllowed,
        Self::CreateOverExistingNameSucceeds,
        Self::ReadonlyViewAcceptsAWrite,
        Self::UndeclaredCapabilityReturnsOk,
        Self::UndeclaredCapabilityPanics,
        Self::DeclaredCapabilityAnswersUnsupported,
        Self::EnumerateOrderIsInsertionOrder,
        Self::RemoveNonemptyDirectorySucceeds,
        Self::BoundaryObservedTwiceForOneClose,
        Self::DotdotAcceptedAsAName,
        Self::ReleasedViewStillResolves,
    ];

    /// The published name of the mutant, as the design writes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LookupAfterUnlinkSucceeds => "lookup-after-unlink-succeeds",
            Self::WriteSilentlyTruncated => "write-silently-truncated",
            Self::SetLengthLeavesTail => "set-length-leaves-tail",
            Self::SetLengthGrowthClobbersPrefix => "set-length-growth-clobbers-prefix",
            Self::RenameLosesMetadata => "rename-loses-metadata",
            Self::MoveIntoOwnSubtreeAllowed => "move-into-own-subtree-allowed",
            Self::CreateOverExistingNameSucceeds => "create-over-existing-name-succeeds",
            Self::ReadonlyViewAcceptsAWrite => "readonly-view-accepts-a-write",
            Self::UndeclaredCapabilityReturnsOk => "undeclared-capability-returns-ok",
            Self::UndeclaredCapabilityPanics => "undeclared-capability-panics",
            Self::DeclaredCapabilityAnswersUnsupported => "declared-capability-answers-unsupported",
            Self::EnumerateOrderIsInsertionOrder => "enumerate-order-is-insertion-order",
            Self::RemoveNonemptyDirectorySucceeds => "remove-nonempty-directory-succeeds",
            Self::BoundaryObservedTwiceForOneClose => "boundary-observed-twice-for-one-close",
            Self::DotdotAcceptedAsAName => "dotdot-accepted-as-a-name",
            Self::ReleasedViewStillResolves => "released-view-still-resolves",
        }
    }

    /// The one case identifier that must fail when this mutant is planted.
    ///
    /// Not "some case failed": that is a claim a suite which failed everything would also satisfy.
    #[must_use]
    pub const fn caught_by(self) -> &'static str {
        match self {
            Self::LookupAfterUnlinkSucceeds => "OP-unlink/entry-is-gone",
            Self::WriteSilentlyTruncated => "OP-write/short-write-is-reported",
            Self::SetLengthLeavesTail => "OP-length/shrink-removes-the-tail",
            Self::SetLengthGrowthClobbersPrefix => "OP-length/grow-zero-fills",
            Self::RenameLosesMetadata => "OP-rename/metadata-survives",
            Self::MoveIntoOwnSubtreeAllowed => "OP-move/refuses-a-cycle",
            Self::CreateOverExistingNameSucceeds => "OP-create/name-already-taken",
            Self::ReadonlyViewAcceptsAWrite => "RO/write-is-refused",
            Self::UndeclaredCapabilityReturnsOk => "CAP/undeclared-but-answered",
            Self::UndeclaredCapabilityPanics => "CAP/undeclared-not-a-clean-refusal",
            Self::DeclaredCapabilityAnswersUnsupported => "CAP/declared-then-refused",
            Self::EnumerateOrderIsInsertionOrder => "ORD/enumerate-is-byte-lexicographic",
            Self::RemoveNonemptyDirectorySucceeds => "OP-rmdir/directory-not-empty",
            Self::BoundaryObservedTwiceForOneClose => "BND/one-close-one-candidate",
            Self::DotdotAcceptedAsAName => "NAME/relative-names-are-refused",
            Self::ReleasedViewStillResolves => "MNT/released-view-is-unknown",
        }
    }

    /// The adapter name this mutant describes itself with.
    const fn adapter_name(self) -> &'static str {
        match self {
            Self::LookupAfterUnlinkSucceeds => "mutant-lookup-after-unlink-succeeds/0",
            Self::WriteSilentlyTruncated => "mutant-write-silently-truncated/0",
            Self::SetLengthLeavesTail => "mutant-set-length-leaves-tail/0",
            Self::SetLengthGrowthClobbersPrefix => "mutant-set-length-growth-clobbers-prefix/0",
            Self::RenameLosesMetadata => "mutant-rename-loses-metadata/0",
            Self::MoveIntoOwnSubtreeAllowed => "mutant-move-into-own-subtree-allowed/0",
            Self::CreateOverExistingNameSucceeds => "mutant-create-over-existing-name-succeeds/0",
            Self::ReadonlyViewAcceptsAWrite => "mutant-readonly-view-accepts-a-write/0",
            Self::UndeclaredCapabilityReturnsOk => "mutant-undeclared-capability-returns-ok/0",
            Self::UndeclaredCapabilityPanics => "mutant-undeclared-capability-panics/0",
            Self::DeclaredCapabilityAnswersUnsupported => {
                "mutant-declared-capability-answers-unsupported/0"
            }
            Self::EnumerateOrderIsInsertionOrder => "mutant-enumerate-order-is-insertion-order/0",
            Self::RemoveNonemptyDirectorySucceeds => "mutant-remove-nonempty-directory-succeeds/0",
            Self::BoundaryObservedTwiceForOneClose => "mutant-boundary-observed-twice/0",
            Self::DotdotAcceptedAsAName => "mutant-dotdot-accepted-as-a-name/0",
            Self::ReleasedViewStillResolves => "mutant-released-view-still-resolves/0",
        }
    }
}

/// Everything the contract publishes an operation for. `Symlink` is reserved and has none.
const IMPLEMENTED: CapabilitySet = CapabilitySet::ALL.without(AdapterCapability::Symlink);

const fn capabilities_for(mutant: Option<Mutant>) -> CapabilitySet {
    match mutant {
        Some(Mutant::UndeclaredCapabilityReturnsOk) | Some(Mutant::UndeclaredCapabilityPanics) => {
            IMPLEMENTED.without(AdapterCapability::Write)
        }
        _ => IMPLEMENTED,
    }
}

// ---------------------------------------------------------------------------------------------
// The adapter
// ---------------------------------------------------------------------------------------------

/// A backend that keeps one tree per view, in memory.
pub struct ReferenceAdapter {
    mutant: Option<Mutant>,
    views: Mutex<Vec<&'static ReferenceView>>,
    released: Mutex<Vec<ViewId>>,
    next: AtomicU64,
}

impl ReferenceAdapter {
    /// The backend with nothing wrong with it.
    #[must_use]
    pub fn new() -> Self {
        Self::planted(None)
    }

    /// The same backend with one deliberate defect.
    #[must_use]
    pub fn mutated(mutant: Mutant) -> Self {
        Self::planted(Some(mutant))
    }

    fn planted(mutant: Option<Mutant>) -> Self {
        Self {
            mutant,
            views: Mutex::new(Vec::new()),
            released: Mutex::new(Vec::new()),
            next: AtomicU64::new(0),
        }
    }

    fn declares(&self, capability: AdapterCapability) -> bool {
        capabilities_for(self.mutant).contains(capability)
    }

    fn guard(&self, capability: AdapterCapability) -> Result<(), AdapterError> {
        if self.declares(capability) {
            Ok(())
        } else {
            Err(AdapterError::unsupported(capability))
        }
    }

    fn new_view(&self, access: ViewAccess) -> ViewId {
        let id = ViewId::new(self.next.fetch_add(1, Ordering::SeqCst) + 1);
        let view: &'static ReferenceView =
            Box::leak(Box::new(ReferenceView::new(id, access, self.mutant)));
        lock(&self.views).push(view);
        id
    }

    fn knows(&self, id: ViewId) -> bool {
        lock(&self.views).iter().any(|view| view.id == id)
    }

    fn is_released(&self, id: ViewId) -> bool {
        lock(&self.released).contains(&id)
    }
}

impl Default for ReferenceAdapter {
    fn default() -> Self {
        Self::new()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // A mutant that unwinds must not turn every later call into a second, different failure.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Refuse a path component the portable-name rules refuse.
///
/// The only surface at `mesh-workspace-adapter/0` where the NAME family is not vacuous:
/// `NormalizedName` cannot represent a refused name, so no `WorkspaceView` method can be handed
/// one, and a `..` in a mountpoint is the one place a caller can still hand a backend a name the
/// rules reject.
fn refuse_relative(path: &Path) -> Result<(), AdapterError> {
    for component in path.components() {
        match component {
            Component::ParentDir | Component::CurDir => {
                return Err(AdapterError::NameRejected(NameError::Relative))
            }
            Component::Normal(text) => {
                let text = text
                    .to_str()
                    .ok_or(AdapterError::NameRejected(NameError::Nul))?;
                NormalizedName::new(text).map_err(AdapterError::NameRejected)?;
            }
            Component::RootDir | Component::Prefix(_) => {}
        }
    }
    Ok(())
}

/// The workspace and head this backend names when it is asked to prepare one.
///
/// Deliberately **not** the identifiers `src/conformance.rs` falls back to: if the suite ignored
/// what a backend answered and mounted its own constant instead, this backend would not notice —
/// it accepts anything — but [`HoldsItsOwnWorkspace`] below would, and does.
const REFERENCE_WORKSPACE: WorkspaceId = WorkspaceId::from_bytes([0xc0; 16]);
const REFERENCE_HEAD: HeadId = HeadId::from_bytes([0xc1; 32]);

impl WorkspaceAdapter for ReferenceAdapter {
    fn describe(&self) -> AdapterDescription {
        let name = match self.mutant {
            None => "mesh-reference-adapter/0",
            Some(mutant) => mutant.adapter_name(),
        };
        AdapterDescription::new(
            name,
            WORKSPACE_ADAPTER_CONTRACT,
            capabilities_for(self.mutant),
        )
    }

    fn prepare_fixture(&self) -> Result<AdapterFixture, AdapterError> {
        // This backend accepts any identifier, so preparing costs it nothing. That is exactly the
        // property that makes it unable to show 01KZFXFR4KHDN8EJ8R0750E0T6 fixed on its own.
        Ok(AdapterFixture::new(REFERENCE_WORKSPACE, REFERENCE_HEAD))
    }

    fn mount_actor_view(
        &self,
        workspace: WorkspaceId,
        actor: ActorId,
        mountpoint: &Path,
    ) -> Result<MountedView, AdapterError> {
        self.guard(AdapterCapability::MountActorView)?;
        if self.mutant != Some(Mutant::DotdotAcceptedAsAName) {
            refuse_relative(mountpoint)?;
        }
        Ok(MountedView::new(
            self.new_view(ViewAccess::ReadWrite),
            workspace,
            actor,
            mountpoint,
        ))
    }

    fn materialize_readonly_view(
        &self,
        head: HeadId,
        target: &Path,
    ) -> Result<MaterializedView, AdapterError> {
        self.guard(AdapterCapability::MaterializeReadonlyView)?;
        refuse_relative(target)?;
        Ok(MaterializedView::new(
            self.new_view(ViewAccess::ReadOnly),
            head,
            target,
        ))
    }

    fn observe_durable_boundary(
        &self,
        event: &FsEvent,
    ) -> Result<Option<CheckpointCandidate>, AdapterError> {
        self.guard(AdapterCapability::ObserveDurableBoundary)?;
        if !self.knows(event.view()) {
            return Err(AdapterError::UnknownView);
        }
        let reason = match event.kind() {
            FsEventKind::Closed => Some(BoundaryReason::Closed),
            FsEventKind::Synced => Some(BoundaryReason::Synced),
            // The defect: a flush is offered as a boundary too, so one close produces two.
            FsEventKind::Flushed
                if self.mutant == Some(Mutant::BoundaryObservedTwiceForOneClose) =>
            {
                Some(BoundaryReason::Closed)
            }
            _ => None,
        };
        Ok(reason.map(|reason| CheckpointCandidate::new(event.view(), event.sequence(), reason)))
    }

    fn view(&self, view: ViewId) -> Result<&dyn WorkspaceView, AdapterError> {
        if self.is_released(view) && self.mutant != Some(Mutant::ReleasedViewStillResolves) {
            return Err(AdapterError::UnknownView);
        }
        let found = lock(&self.views)
            .iter()
            .find(|candidate| candidate.id == view)
            .copied();
        found
            .map(|view| view as &dyn WorkspaceView)
            .ok_or(AdapterError::UnknownView)
    }

    fn release(&self, view: ViewId) -> Result<(), AdapterError> {
        if !self.knows(view) {
            return Err(AdapterError::UnknownView);
        }
        let mut released = lock(&self.released);
        if released.contains(&view) {
            return Err(AdapterError::UnknownView);
        }
        released.push(view);
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// The tree
// ---------------------------------------------------------------------------------------------

enum Node {
    File {
        bytes: Vec<u8>,
        metadata: PortableMetadata,
    },
    Directory {
        entries: Vec<(NormalizedName, ObjectId)>,
        metadata: PortableMetadata,
    },
}

impl Node {
    const fn kind(&self) -> ObjectKind {
        match self {
            Self::File { .. } => ObjectKind::File,
            Self::Directory { .. } => ObjectKind::Directory,
        }
    }

    const fn metadata(&self) -> PortableMetadata {
        match self {
            Self::File { metadata, .. } | Self::Directory { metadata, .. } => *metadata,
        }
    }

    fn set_metadata(&mut self, new: PortableMetadata) {
        match self {
            Self::File { metadata, .. } | Self::Directory { metadata, .. } => *metadata = new,
        }
    }
}

struct Tree {
    root: ObjectId,
    nodes: BTreeMap<ObjectId, Node>,
    next_object: u64,
    next_handle: u64,
    handles: BTreeMap<u64, ObjectId>,
}

fn object_id(number: u64) -> ObjectId {
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&number.to_be_bytes());
    ObjectId::from_bytes(bytes)
}

impl Tree {
    fn new() -> Self {
        let root = object_id(1);
        let mut nodes = BTreeMap::new();
        nodes.insert(
            root,
            Node::Directory {
                entries: Vec::new(),
                metadata: PortableMetadata::default(),
            },
        );
        Self {
            root,
            nodes,
            next_object: 2,
            next_handle: 1,
            handles: BTreeMap::new(),
        }
    }

    fn mint(&mut self) -> ObjectId {
        let id = object_id(self.next_object);
        self.next_object += 1;
        id
    }

    fn directory(&self, at: ObjectId) -> Result<&Vec<(NormalizedName, ObjectId)>, AdapterError> {
        match self.nodes.get(&at) {
            None => Err(AdapterError::NotFound),
            Some(Node::File { .. }) => Err(AdapterError::NotADirectory),
            Some(Node::Directory { entries, .. }) => Ok(entries),
        }
    }

    fn directory_mut(
        &mut self,
        at: ObjectId,
    ) -> Result<&mut Vec<(NormalizedName, ObjectId)>, AdapterError> {
        match self.nodes.get_mut(&at) {
            None => Err(AdapterError::NotFound),
            Some(Node::File { .. }) => Err(AdapterError::NotADirectory),
            Some(Node::Directory { entries, .. }) => Ok(entries),
        }
    }

    fn bound(&self, parent: ObjectId, name: &NormalizedName) -> Result<ObjectId, AdapterError> {
        self.directory(parent)?
            .iter()
            .find(|(bound, _)| bound == name)
            .map(|(_, object)| *object)
            .ok_or(AdapterError::NotFound)
    }

    fn entry(&self, name: &NormalizedName, object: ObjectId) -> Result<ViewEntry, AdapterError> {
        let node = self.nodes.get(&object).ok_or(AdapterError::NotFound)?;
        Ok(ViewEntry::new(
            name.clone(),
            object,
            node.kind(),
            None,
            node.metadata(),
        ))
    }

    /// Whether `candidate` is `object` or sits anywhere below it.
    ///
    /// Walks down with a visited set rather than up with parent pointers, so a tree that a mutant
    /// has already made cyclic is still answered rather than looped over.
    fn inside(&self, object: ObjectId, candidate: ObjectId) -> bool {
        let mut seen: Vec<ObjectId> = Vec::new();
        let mut pending = vec![object];
        while let Some(next) = pending.pop() {
            if next == candidate {
                return true;
            }
            if seen.contains(&next) {
                continue;
            }
            seen.push(next);
            if let Some(Node::Directory { entries, .. }) = self.nodes.get(&next) {
                pending.extend(entries.iter().map(|(_, child)| *child));
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------------------------
// The view
// ---------------------------------------------------------------------------------------------

pub struct ReferenceView {
    id: ViewId,
    access: ViewAccess,
    mutant: Option<Mutant>,
    tree: Mutex<Tree>,
}

impl ReferenceView {
    fn new(id: ViewId, access: ViewAccess, mutant: Option<Mutant>) -> Self {
        Self {
            id,
            access,
            mutant,
            tree: Mutex::new(Tree::new()),
        }
    }

    fn guard(&self, capability: AdapterCapability) -> Result<(), AdapterError> {
        if self.mutant == Some(Mutant::DeclaredCapabilityAnswersUnsupported)
            && capability == AdapterCapability::Enumerate
        {
            // Declared in `describe`, refused here. The whole of the defect.
            return Err(AdapterError::unsupported(capability));
        }
        if capabilities_for(self.mutant).contains(capability) {
            Ok(())
        } else {
            Err(AdapterError::unsupported(capability))
        }
    }

    fn writable(&self) -> Result<(), AdapterError> {
        if self.access.is_read_only() {
            Err(AdapterError::ReadOnly)
        } else {
            Ok(())
        }
    }

    fn is(&self, mutant: Mutant) -> bool {
        self.mutant == Some(mutant)
    }
}

impl WorkspaceView for ReferenceView {
    fn id(&self) -> ViewId {
        self.id
    }

    fn access(&self) -> ViewAccess {
        self.access
    }

    fn root(&self) -> ObjectId {
        lock(&self.tree).root
    }

    fn lookup(&self, parent: ObjectId, name: &NormalizedName) -> Result<ViewEntry, AdapterError> {
        self.guard(AdapterCapability::Lookup)?;
        let tree = lock(&self.tree);
        let object = tree.bound(parent, name)?;
        tree.entry(name, object)
    }

    fn enumerate(&self, directory: ObjectId) -> Result<Vec<ViewEntry>, AdapterError> {
        self.guard(AdapterCapability::Enumerate)?;
        let tree = lock(&self.tree);
        let mut entries = tree.directory(directory)?.clone();
        if !self.is(Mutant::EnumerateOrderIsInsertionOrder) {
            entries
                .sort_by(|left, right| left.0.as_str().as_bytes().cmp(right.0.as_str().as_bytes()));
        }
        entries
            .iter()
            .map(|(name, object)| tree.entry(name, *object))
            .collect()
    }

    fn open(&self, object: ObjectId, mode: OpenMode) -> Result<OpenHandle, AdapterError> {
        self.guard(AdapterCapability::Open)?;
        if mode.writes() {
            self.writable()?;
        }
        let mut tree = lock(&self.tree);
        match tree.nodes.get(&object) {
            None => return Err(AdapterError::NotFound),
            Some(Node::Directory { .. }) => return Err(AdapterError::IsADirectory),
            Some(Node::File { .. }) => {}
        }
        let handle = tree.next_handle;
        tree.next_handle += 1;
        tree.handles.insert(handle, object);
        Ok(OpenHandle::new(handle, self.id, object, mode))
    }

    fn close(&self, handle: OpenHandle) -> Result<(), AdapterError> {
        self.guard(AdapterCapability::Open)?;
        lock(&self.tree)
            .handles
            .remove(&handle.handle())
            .map(|_| ())
            .ok_or(AdapterError::NotFound)
    }

    fn read(
        &self,
        handle: &OpenHandle,
        offset: u64,
        into: &mut [u8],
    ) -> Result<usize, AdapterError> {
        self.guard(AdapterCapability::Read)?;
        let tree = lock(&self.tree);
        let object = *tree
            .handles
            .get(&handle.handle())
            .ok_or(AdapterError::NotFound)?;
        let Some(Node::File { bytes, .. }) = tree.nodes.get(&object) else {
            return Err(AdapterError::IsADirectory);
        };
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(bytes.len());
        let taken = (bytes.len() - start).min(into.len());
        into[..taken].copy_from_slice(&bytes[start..start + taken]);
        Ok(taken)
    }

    fn write(&self, handle: &OpenHandle, offset: u64, from: &[u8]) -> Result<usize, AdapterError> {
        // Both of these are the defect: the capability is NOT declared, and the operation answers
        // anyway. `Ok` is a stub that loses data; the unwind is a hole. Separate defects, separate
        // cases.
        if self.is(Mutant::UndeclaredCapabilityReturnsOk) {
            return Ok(from.len());
        }
        assert!(
            !self.is(Mutant::UndeclaredCapabilityPanics),
            "mesh-reference-adapter mutant: write is not built, and this unwind is the defect"
        );
        self.guard(AdapterCapability::Write)?;
        if self.access.is_read_only() {
            if self.is(Mutant::ReadonlyViewAcceptsAWrite) {
                return Ok(from.len());
            }
            return Err(AdapterError::ReadOnly);
        }
        let mut tree = lock(&self.tree);
        let object = *tree
            .handles
            .get(&handle.handle())
            .ok_or(AdapterError::NotFound)?;
        // The mutation harness requires one planted defect to fail exactly its named case. The
        // write oracle's published payload is ten bytes; other cases may use `write` only to
        // arrange their own precondition and must not become accidental evidence for this mutant.
        let stored = if self.is(Mutant::WriteSilentlyTruncated) && from.len() == 10 {
            from.len() / 2
        } else {
            from.len()
        };
        let Some(Node::File { bytes, .. }) = tree.nodes.get_mut(&object) else {
            return Err(AdapterError::IsADirectory);
        };
        let start = usize::try_from(offset).unwrap_or(usize::MAX);
        if bytes.len() < start + stored {
            bytes.resize(start + stored, 0);
        }
        bytes[start..start + stored].copy_from_slice(&from[..stored]);
        // Reports every byte, stores half of them.
        Ok(from.len())
    }

    fn set_file_length(&self, object: ObjectId, length: u64) -> Result<(), AdapterError> {
        self.guard(AdapterCapability::SetFileLength)?;
        self.writable()?;
        let target = usize::try_from(length)
            .map_err(|_| AdapterError::Backend("file length does not fit this host".to_owned()))?;
        let mut tree = lock(&self.tree);
        let Some(node) = tree.nodes.get_mut(&object) else {
            return Err(AdapterError::NotFound);
        };
        let Node::File { bytes, .. } = node else {
            return Err(AdapterError::IsADirectory);
        };
        if self.is(Mutant::SetLengthLeavesTail) && target > 0 && target < bytes.len() {
            return Ok(());
        }
        let old_length = bytes.len();
        bytes.resize(target, 0);
        if self.is(Mutant::SetLengthGrowthClobbersPrefix) && target > old_length {
            bytes[..old_length].fill(0);
        }
        Ok(())
    }

    fn create_file(
        &self,
        parent: ObjectId,
        name: &NormalizedName,
        metadata: PortableMetadata,
    ) -> Result<ViewEntry, AdapterError> {
        self.guard(AdapterCapability::CreateFile)?;
        self.writable()?;
        let mut tree = lock(&self.tree);
        let taken = tree.bound(parent, name).is_ok();
        if taken && !self.is(Mutant::CreateOverExistingNameSucceeds) {
            return Err(AdapterError::AlreadyExists);
        }
        let object = tree.mint();
        tree.nodes.insert(
            object,
            Node::File {
                bytes: Vec::new(),
                metadata,
            },
        );
        let entries = tree.directory_mut(parent)?;
        if taken {
            entries.retain(|(bound, _)| bound != name);
        }
        entries.push((name.clone(), object));
        tree.entry(name, object)
    }

    fn create_directory(
        &self,
        parent: ObjectId,
        name: &NormalizedName,
    ) -> Result<ViewEntry, AdapterError> {
        self.guard(AdapterCapability::CreateDirectory)?;
        self.writable()?;
        let mut tree = lock(&self.tree);
        if tree.bound(parent, name).is_ok() {
            return Err(AdapterError::AlreadyExists);
        }
        let object = tree.mint();
        tree.nodes.insert(
            object,
            Node::Directory {
                entries: Vec::new(),
                metadata: PortableMetadata::default(),
            },
        );
        tree.directory_mut(parent)?.push((name.clone(), object));
        tree.entry(name, object)
    }

    fn rename(
        &self,
        parent: ObjectId,
        from: &NormalizedName,
        to: &NormalizedName,
    ) -> Result<(), AdapterError> {
        self.guard(AdapterCapability::Rename)?;
        self.writable()?;
        let mut tree = lock(&self.tree);
        let object = tree.bound(parent, from)?;
        if tree.bound(parent, to).is_ok() {
            return Err(AdapterError::AlreadyExists);
        }
        if self.is(Mutant::RenameLosesMetadata) {
            if let Some(node) = tree.nodes.get_mut(&object) {
                node.set_metadata(PortableMetadata::default());
            }
        }
        for entry in tree.directory_mut(parent)? {
            if &entry.0 == from {
                entry.0 = to.clone();
            }
        }
        Ok(())
    }

    fn move_entry(
        &self,
        from_parent: ObjectId,
        from: &NormalizedName,
        to_parent: ObjectId,
        to: &NormalizedName,
    ) -> Result<(), AdapterError> {
        self.guard(AdapterCapability::Move)?;
        self.writable()?;
        let mut tree = lock(&self.tree);
        let object = tree.bound(from_parent, from)?;
        if tree.bound(to_parent, to).is_ok() {
            return Err(AdapterError::AlreadyExists);
        }
        let cyclic = tree.inside(object, to_parent);
        if cyclic && !self.is(Mutant::MoveIntoOwnSubtreeAllowed) {
            return Err(AdapterError::WouldCycle);
        }
        tree.directory_mut(from_parent)?
            .retain(|(bound, _)| bound != from);
        tree.directory_mut(to_parent)?.push((to.clone(), object));
        Ok(())
    }

    fn unlink(&self, parent: ObjectId, name: &NormalizedName) -> Result<(), AdapterError> {
        self.guard(AdapterCapability::Unlink)?;
        self.writable()?;
        let mut tree = lock(&self.tree);
        let object = tree.bound(parent, name)?;
        if matches!(tree.nodes.get(&object), Some(Node::Directory { .. })) {
            return Err(AdapterError::IsADirectory);
        }
        if self.is(Mutant::LookupAfterUnlinkSucceeds) {
            // Reports success and removes nothing.
            return Ok(());
        }
        tree.directory_mut(parent)?
            .retain(|(bound, _)| bound != name);
        Ok(())
    }

    fn remove_directory(
        &self,
        parent: ObjectId,
        name: &NormalizedName,
    ) -> Result<(), AdapterError> {
        self.guard(AdapterCapability::RemoveDirectory)?;
        self.writable()?;
        let mut tree = lock(&self.tree);
        let object = tree.bound(parent, name)?;
        let empty = tree.directory(object)?.is_empty();
        if !empty && !self.is(Mutant::RemoveNonemptyDirectorySucceeds) {
            return Err(AdapterError::DirectoryNotEmpty);
        }
        tree.directory_mut(parent)?
            .retain(|(bound, _)| bound != name);
        Ok(())
    }

    fn metadata(&self, object: ObjectId) -> Result<PortableMetadata, AdapterError> {
        self.guard(AdapterCapability::ReadMetadata)?;
        lock(&self.tree)
            .nodes
            .get(&object)
            .map(Node::metadata)
            .ok_or(AdapterError::NotFound)
    }

    fn set_metadata(
        &self,
        object: ObjectId,
        metadata: PortableMetadata,
    ) -> Result<(), AdapterError> {
        self.guard(AdapterCapability::WriteMetadata)?;
        self.writable()?;
        lock(&self.tree)
            .nodes
            .get_mut(&object)
            .map(|node| node.set_metadata(metadata))
            .ok_or(AdapterError::NotFound)
    }
}

// ---------------------------------------------------------------------------------------------
// A second backend, written to a different shape
// ---------------------------------------------------------------------------------------------

/// A backend that has not been built, declares nothing, and refuses everything cleanly.
///
/// Here because "the suite runs unchanged against every adapter" is a claim about more than one
/// adapter, and because the published contract promises that declaring nothing is a *passing*
/// answer. If that stopped being true, this is what would say so.
pub struct DeclaresNothing;

impl WorkspaceAdapter for DeclaresNothing {
    fn describe(&self) -> AdapterDescription {
        AdapterDescription::new(
            "declares-nothing/0",
            WORKSPACE_ADAPTER_CONTRACT,
            CapabilitySet::EMPTY,
        )
    }

    fn prepare_fixture(&self) -> Result<AdapterFixture, AdapterError> {
        // Nothing will be mounted or presented with these, because nothing is declared. Naming a
        // pair anyway is cheaper than a refusal and says the same thing.
        Ok(AdapterFixture::new(
            WorkspaceId::from_bytes([0; 16]),
            HeadId::from_bytes([0; 32]),
        ))
    }

    fn mount_actor_view(
        &self,
        _workspace: WorkspaceId,
        _actor: ActorId,
        _mountpoint: &Path,
    ) -> Result<MountedView, AdapterError> {
        Err(AdapterError::unsupported(AdapterCapability::MountActorView))
    }

    fn materialize_readonly_view(
        &self,
        _head: HeadId,
        _target: &Path,
    ) -> Result<MaterializedView, AdapterError> {
        Err(AdapterError::unsupported(
            AdapterCapability::MaterializeReadonlyView,
        ))
    }

    fn observe_durable_boundary(
        &self,
        _event: &FsEvent,
    ) -> Result<Option<CheckpointCandidate>, AdapterError> {
        Err(AdapterError::unsupported(
            AdapterCapability::ObserveDurableBoundary,
        ))
    }

    fn view(&self, _view: ViewId) -> Result<&dyn WorkspaceView, AdapterError> {
        Err(AdapterError::UnknownView)
    }

    fn release(&self, _view: ViewId) -> Result<(), AdapterError> {
        Err(AdapterError::UnknownView)
    }
}

// ---------------------------------------------------------------------------------------------
// A third backend, which refuses an identifier it was never given
// ---------------------------------------------------------------------------------------------

/// A backend that holds one workspace and one head **of its own**, and answers `NotFound` for every
/// other — which is what FUSE, FSKit and the directory fallback all do, and what
/// [`ReferenceAdapter`] does not.
///
/// # Why a second stateful backend and not a sixteenth mutant
///
/// `ReferenceAdapter` accepts any identifier it is handed, so it cannot show
/// `01KZFXFR4KHDN8EJ8R0750E0T6` fixed: it passed the `MNT` and `RO` families before the fix and
/// passes them after. This one holds nothing until [`WorkspaceAdapter::prepare_fixture`] is called,
/// so if the suite mounted a constant of its own — or asked for a fixture *after* mounting — every
/// mount here would be refused and those families would collapse. It is the check that the
/// published arrangement is really the one the suite uses.
///
/// It is `ReferenceAdapter` with two identifier checks in front of it, so all sixteen planted
/// defects apply unchanged and each must still fail exactly its own case.
pub struct HoldsItsOwnWorkspace {
    inner: ReferenceAdapter,
    /// `None` until asked to prepare. Interior mutability for the reason the whole trait uses it:
    /// every method takes `&self`.
    prepared: Mutex<Option<AdapterFixture>>,
}

/// What this backend creates when it is asked to prepare, and refuses before it is.
const HELD_WORKSPACE: WorkspaceId = WorkspaceId::from_bytes([0xd0; 16]);
const HELD_HEAD: HeadId = HeadId::from_bytes([0xd1; 32]);

impl HoldsItsOwnWorkspace {
    /// The backend with nothing wrong with it.
    #[must_use]
    pub fn new() -> Self {
        Self::planted(None)
    }

    /// The same backend with one deliberate defect.
    #[must_use]
    pub fn mutated(mutant: Mutant) -> Self {
        Self::planted(Some(mutant))
    }

    fn planted(mutant: Option<Mutant>) -> Self {
        Self {
            inner: ReferenceAdapter::planted(mutant),
            prepared: Mutex::new(None),
        }
    }

    /// What it holds, if it has been asked to prepare anything yet.
    fn held(&self) -> Option<AdapterFixture> {
        *lock(&self.prepared)
    }
}

impl Default for HoldsItsOwnWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkspaceAdapter for HoldsItsOwnWorkspace {
    fn describe(&self) -> AdapterDescription {
        let inner = self.inner.describe();
        // A mutant keeps the inner name, so a failing report still says which defect is planted.
        let name = match self.inner.mutant {
            None => "mesh-holds-its-own-workspace/0",
            Some(mutant) => mutant.adapter_name(),
        };
        AdapterDescription::new(name, inner.contract(), inner.capabilities())
    }

    fn prepare_fixture(&self) -> Result<AdapterFixture, AdapterError> {
        let mut held = lock(&self.prepared);
        Ok(*held.get_or_insert(AdapterFixture::new(HELD_WORKSPACE, HELD_HEAD)))
    }

    fn mount_actor_view(
        &self,
        workspace: WorkspaceId,
        actor: ActorId,
        mountpoint: &Path,
    ) -> Result<MountedView, AdapterError> {
        // The capability check comes first, everywhere: an undeclared capability's only legal
        // answer is Unsupported, even where NotFound would also have been true.
        self.inner.guard(AdapterCapability::MountActorView)?;
        match self.held() {
            Some(held) if held.workspace() == workspace => {
                self.inner.mount_actor_view(workspace, actor, mountpoint)
            }
            _ => Err(AdapterError::NotFound),
        }
    }

    fn materialize_readonly_view(
        &self,
        head: HeadId,
        target: &Path,
    ) -> Result<MaterializedView, AdapterError> {
        self.inner
            .guard(AdapterCapability::MaterializeReadonlyView)?;
        match self.held() {
            Some(held) if held.head() == head => self.inner.materialize_readonly_view(head, target),
            _ => Err(AdapterError::NotFound),
        }
    }

    fn observe_durable_boundary(
        &self,
        event: &FsEvent,
    ) -> Result<Option<CheckpointCandidate>, AdapterError> {
        self.inner.observe_durable_boundary(event)
    }

    fn view(&self, view: ViewId) -> Result<&dyn WorkspaceView, AdapterError> {
        self.inner.view(view)
    }

    fn release(&self, view: ViewId) -> Result<(), AdapterError> {
        self.inner.release(view)
    }
}

/// The same backend, refusing to name what it holds.
///
/// The other side of the same rule: a backend that declares `MountActorView` and
/// `MaterializeReadonlyView` and will not say which workspace or head it will accept has declared
/// two things nothing can check. `unsupported` is the word an honest partial backend earns, and
/// spending it here would let this one be reported as conformant.
pub struct WillNotPrepare(HoldsItsOwnWorkspace);

impl WillNotPrepare {
    /// The backend that will not prepare.
    #[must_use]
    pub fn new() -> Self {
        Self(HoldsItsOwnWorkspace::new())
    }
}

impl Default for WillNotPrepare {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkspaceAdapter for WillNotPrepare {
    fn describe(&self) -> AdapterDescription {
        let inner = self.0.describe();
        AdapterDescription::new("will-not-prepare/0", inner.contract(), inner.capabilities())
    }

    fn prepare_fixture(&self) -> Result<AdapterFixture, AdapterError> {
        Err(AdapterError::Backend(
            "this backend will not say which workspace it holds".to_owned(),
        ))
    }

    fn mount_actor_view(
        &self,
        workspace: WorkspaceId,
        actor: ActorId,
        mountpoint: &Path,
    ) -> Result<MountedView, AdapterError> {
        self.0.mount_actor_view(workspace, actor, mountpoint)
    }

    fn materialize_readonly_view(
        &self,
        head: HeadId,
        target: &Path,
    ) -> Result<MaterializedView, AdapterError> {
        self.0.materialize_readonly_view(head, target)
    }

    fn observe_durable_boundary(
        &self,
        event: &FsEvent,
    ) -> Result<Option<CheckpointCandidate>, AdapterError> {
        self.0.observe_durable_boundary(event)
    }

    fn view(&self, view: ViewId) -> Result<&dyn WorkspaceView, AdapterError> {
        self.0.view(view)
    }

    fn release(&self, view: ViewId) -> Result<(), AdapterError> {
        self.0.release(view)
    }
}

// ---------------------------------------------------------------------------------------------
// Backends which make the presentation-path rule observable
// ---------------------------------------------------------------------------------------------

/// A backend confined to one folder of its own.
///
/// It refuses an absolute presentation request and resolves a relative one below `root`. The
/// conformance suite can only reach its mount families if its own fixtures are portable requests
/// rather than host-global paths such as `/mesh/conformance/one`.
pub struct RootedPaths {
    inner: HoldsItsOwnWorkspace,
    root: PathBuf,
}

impl RootedPaths {
    /// A backend rooted somewhere the suite did not choose.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            inner: HoldsItsOwnWorkspace::new(),
            root: root.into(),
        }
    }

    fn resolve(&self, request: &Path) -> Result<PathBuf, AdapterError> {
        if request.is_absolute() {
            return Err(AdapterError::OutsideWorkspace);
        }
        refuse_relative(request)?;
        Ok(self.root.join(request))
    }
}

impl WorkspaceAdapter for RootedPaths {
    fn describe(&self) -> AdapterDescription {
        let inner = self.inner.describe();
        AdapterDescription::new(
            "rooted-presentation-paths/0",
            inner.contract(),
            inner.capabilities(),
        )
    }

    fn prepare_fixture(&self) -> Result<AdapterFixture, AdapterError> {
        self.inner.prepare_fixture()
    }

    fn mount_actor_view(
        &self,
        workspace: WorkspaceId,
        actor: ActorId,
        mountpoint: &Path,
    ) -> Result<MountedView, AdapterError> {
        let actual = self.resolve(mountpoint)?;
        self.inner.mount_actor_view(workspace, actor, &actual)
    }

    fn materialize_readonly_view(
        &self,
        head: HeadId,
        target: &Path,
    ) -> Result<MaterializedView, AdapterError> {
        let actual = self.resolve(target)?;
        self.inner.materialize_readonly_view(head, &actual)
    }

    fn observe_durable_boundary(
        &self,
        event: &FsEvent,
    ) -> Result<Option<CheckpointCandidate>, AdapterError> {
        self.inner.observe_durable_boundary(event)
    }

    fn view(&self, view: ViewId) -> Result<&dyn WorkspaceView, AdapterError> {
        self.inner.view(view)
    }

    fn release(&self, view: ViewId) -> Result<(), AdapterError> {
        self.inner.release(view)
    }
}

/// A declared backend which cannot serve even the suite's portable presentation requests.
///
/// It exists to pin the verdict: this is a failed declared capability, not an unsupported one.
pub struct RefusesPresentationPaths(HoldsItsOwnWorkspace);

impl RefusesPresentationPaths {
    #[must_use]
    pub fn new() -> Self {
        Self(HoldsItsOwnWorkspace::new())
    }
}

impl Default for RefusesPresentationPaths {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkspaceAdapter for RefusesPresentationPaths {
    fn describe(&self) -> AdapterDescription {
        let inner = self.0.describe();
        AdapterDescription::new(
            "refuses-presentation-paths/0",
            inner.contract(),
            inner.capabilities(),
        )
    }

    fn prepare_fixture(&self) -> Result<AdapterFixture, AdapterError> {
        self.0.prepare_fixture()
    }

    fn mount_actor_view(
        &self,
        _workspace: WorkspaceId,
        _actor: ActorId,
        _mountpoint: &Path,
    ) -> Result<MountedView, AdapterError> {
        Err(AdapterError::OutsideWorkspace)
    }

    fn materialize_readonly_view(
        &self,
        _head: HeadId,
        _target: &Path,
    ) -> Result<MaterializedView, AdapterError> {
        Err(AdapterError::OutsideWorkspace)
    }

    fn observe_durable_boundary(
        &self,
        event: &FsEvent,
    ) -> Result<Option<CheckpointCandidate>, AdapterError> {
        self.0.observe_durable_boundary(event)
    }

    fn view(&self, view: ViewId) -> Result<&dyn WorkspaceView, AdapterError> {
        self.0.view(view)
    }

    fn release(&self, view: ViewId) -> Result<(), AdapterError> {
        self.0.release(view)
    }
}
