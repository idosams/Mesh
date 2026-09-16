//! The backend itself: the thing that hands out views.
//!
//! # Seventeen capabilities, including exact file length
//!
//! Everything in [`AdapterCapability::ALL`] except [`AdapterCapability::Symlink`], which is
//! reserved and has no operation at `mesh-workspace-adapter/1`.
//! The one that matters is
//! [`AdapterCapability::ObserveDurableBoundary`], which the folder-watching fallback honestly
//! refuses: re-reading a folder never shows an open, a flush, a close or an `fsync`. **A FUSE
//! session is handed all four by the kernel**, so this is the first backend for which declaring
//! it is a claim rather than a guess, and the `BND` family grades `pass` here where it grades
//! `unsupported` there.
//!
//! # A mountpoint is a name, not a place
//!
//! The folder-watching backend is confined to a folder and had to interpret the suite's
//! `/mesh/conformance/one` inside it, which it publishes as the `confined-to-its-folder`
//! restriction. This backend holds its state in memory and **touches no filesystem at all**: a
//! mountpoint is recorded and nothing is created at it. That removes the confinement finding at
//! this layer and moves it wholly to the kernel binding, which is not in this crate — see
//! `lib.rs` under "What this crate is not".
//!
//! # Two actors, two trees, and what that costs
//!
//! State is keyed by **actor**, so two mounts of one actor are two views onto one [`Tree`] — which
//! is what a second mountpoint of one filesystem is on any operating system — and two actors share
//! no lock. The tree registry is a `Mutex`, taken **only when a view is mounted**, never on the
//! path of a `read`, a `write` or a `lookup`. `tests/concurrency.rs` measures that rather than
//! asserting it.

use std::collections::HashMap;
use std::path::{Component, Path};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard};

use mesh_materializer::{
    ActorId, AdapterCapability, AdapterDescription, AdapterError, AdapterFixture,
    BoundaryObservationV1, BoundaryReason, BoundaryReasonV1, CapabilitySet, CheckpointCandidate,
    CheckpointCandidateV1, DestinationBindingOutcome, FsEvent, FsEventKind, HeadId,
    MaterializedView, MountedView, MovedObjectIdentity, NameError, NormalizedName, ObjectKind,
    PortableMetadata, RenameEvidence, RenameEvidenceUnavailable, ViewAccess, ViewId,
    WorkspaceAdapter, WorkspaceId, WorkspaceView, WORKSPACE_ADAPTER_CONTRACT_V1,
};

use crate::tree::{Tree, DEFAULT_MAX_WRITE};
use crate::view::FuseView;

/// The backend's own name, as it describes itself.
pub const ADAPTER_NAME: &str = "mesh-fuse/1";

/// Everything this backend implements.
///
/// `Symlink` is absent because contract 1 publishes no symlink operation.
pub const DECLARED: CapabilitySet = CapabilitySet::ALL.without(AdapterCapability::Symlink);

/// The file a presented head holds, so a read-only view has something in it to refuse a write to.
const PRESENTED_ENTRY: &str = "earlier-version.txt";

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn shared<T>(guard: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    guard.read().unwrap_or_else(PoisonError::into_inner)
}

/// Refuse a path component the portable-name rules refuse.
///
/// The one surface at `mesh-workspace-adapter/1` where the `NAME` family is not vacuous: every
/// [`WorkspaceView`] method takes a [`NormalizedName`], which cannot hold a refused name, so a
/// `..` in a mountpoint or a target is the only place a caller can still hand a backend one.
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

/// The Linux FUSE adapter's semantic half: everything a FUSE session decides, minus the kernel.
pub struct FuseAdapter {
    workspace: WorkspaceId,
    head: HeadId,
    max_write: usize,
    prepared: AtomicBool,
    serial: AtomicU32,
    next_view: AtomicU64,
    /// One tree per actor. Taken only when a view is mounted.
    actors: Mutex<HashMap<[u8; 32], &'static Tree>>,
    /// The presented head. Built once, by `prepare_fixture`.
    presented: Mutex<Option<&'static Tree>>,
    /// Append-only. Resolving a view identifier takes a shared read of this and nothing else.
    views: RwLock<Vec<&'static FuseView>>,
}

impl Default for FuseAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl FuseAdapter {
    /// A backend holding the workspace and head it will name when it is asked to prepare.
    ///
    /// The identifiers are fixed constants of this backend rather than random, so two runs of the
    /// conformance suite against two fresh adapters produce identical reports — which is what
    /// makes running it twice a determinism check rather than a coincidence.
    #[must_use]
    pub fn new() -> Self {
        Self::with_max_write(DEFAULT_MAX_WRITE)
    }

    /// A backend that takes at most `max_write` bytes per write.
    ///
    /// FUSE negotiates this at mount time and 128 KiB is Linux's default; a small value here is
    /// how `tests/kernel-retry.rs` makes the kernel's re-issue loop run for a payload small enough
    /// to reason about.
    #[must_use]
    pub fn with_max_write(max_write: usize) -> Self {
        Self {
            workspace: WorkspaceId::from_bytes([0xf5; 16]),
            head: HeadId::from_bytes([0xf6; 32]),
            max_write,
            prepared: AtomicBool::new(false),
            serial: AtomicU32::new(1),
            next_view: AtomicU64::new(1),
            actors: Mutex::new(HashMap::new()),
            presented: Mutex::new(None),
            views: RwLock::new(Vec::new()),
        }
    }

    /// The workspace identifier this backend accepts, and no other.
    #[must_use]
    pub const fn workspace(&self) -> WorkspaceId {
        self.workspace
    }

    /// The head identifier this backend presents, and no other.
    #[must_use]
    pub const fn head(&self) -> HeadId {
        self.head
    }

    /// How many views have been handed out, released or not.
    #[must_use]
    pub fn issued_views(&self) -> usize {
        shared(&self.views).len()
    }

    fn guard(&self, capability: AdapterCapability) -> Result<(), AdapterError> {
        if DECLARED.contains(capability) {
            Ok(())
        } else {
            Err(AdapterError::unsupported(capability))
        }
    }

    fn new_tree(&self) -> &'static Tree {
        let serial = self.serial.fetch_add(1, Ordering::SeqCst);
        Box::leak(Box::new(Tree::new(serial, self.max_write)))
    }

    fn new_view(&self, tree: &'static Tree, access: ViewAccess) -> ViewId {
        let id = ViewId::new(self.next_view.fetch_add(1, Ordering::SeqCst));
        let view: &'static FuseView = Box::leak(Box::new(FuseView::new(id, access, tree)));
        self.views
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .push(view);
        id
    }

    fn find(&self, id: ViewId) -> Option<&'static FuseView> {
        shared(&self.views)
            .iter()
            .find(|view| view.id() == id)
            .copied()
    }
}

impl WorkspaceAdapter for FuseAdapter {
    fn describe(&self) -> AdapterDescription {
        AdapterDescription::new(ADAPTER_NAME, WORKSPACE_ADAPTER_CONTRACT_V1, DECLARED)
    }

    fn prepare_fixture(&self) -> Result<AdapterFixture, AdapterError> {
        let mut presented = lock(&self.presented);
        if presented.is_none() {
            let tree = self.new_tree();
            let name = NormalizedName::new(PRESENTED_ENTRY).map_err(AdapterError::NameRejected)?;
            let entry = tree.create(
                tree.root(),
                &name,
                ObjectKind::File,
                PortableMetadata::default(),
            )?;
            tree.write_at(entry.object(), 0, b"an earlier version of this workspace\n")?;
            *presented = Some(tree);
        }
        self.prepared.store(true, Ordering::SeqCst);
        Ok(AdapterFixture::new(self.workspace, self.head))
    }

    fn mount_actor_view(
        &self,
        workspace: WorkspaceId,
        actor: ActorId,
        mountpoint: &Path,
    ) -> Result<MountedView, AdapterError> {
        self.guard(AdapterCapability::MountActorView)?;
        refuse_relative(mountpoint)?;
        if !self.prepared.load(Ordering::SeqCst) || workspace != self.workspace {
            // A real backend does not hold a workspace it was never given. This is what makes
            // `prepare_fixture` load-bearing rather than decorative.
            return Err(AdapterError::NotFound);
        }
        let tree = {
            let mut actors = lock(&self.actors);
            match actors.get(actor.as_bytes()) {
                Some(tree) => *tree,
                None => {
                    let tree = self.new_tree();
                    actors.insert(*actor.as_bytes(), tree);
                    tree
                }
            }
        };
        let id = self.new_view(tree, ViewAccess::ReadWrite);
        Ok(MountedView::new(id, workspace, actor, mountpoint))
    }

    fn materialize_readonly_view(
        &self,
        head: HeadId,
        target: &Path,
    ) -> Result<MaterializedView, AdapterError> {
        self.guard(AdapterCapability::MaterializeReadonlyView)?;
        refuse_relative(target)?;
        if !self.prepared.load(Ordering::SeqCst) || head != self.head {
            return Err(AdapterError::NotFound);
        }
        let tree = (*lock(&self.presented)).ok_or(AdapterError::NotFound)?;
        let id = self.new_view(tree, ViewAccess::ReadOnly);
        Ok(MaterializedView::new(id, head, target))
    }

    fn observe_durable_boundary(
        &self,
        event: &FsEvent,
    ) -> Result<Option<CheckpointCandidate>, AdapterError> {
        self.guard(AdapterCapability::ObserveDurableBoundary)?;
        let Some(view) = self.find(event.view()) else {
            return Err(AdapterError::UnknownView);
        };
        if view.is_released() {
            return Err(AdapterError::UnknownView);
        }
        // A function of THIS EVENT, and of nothing the adapter has accumulated. The contract calls
        // it "a pure function of the event stream the adapter has been shown", and the suite
        // replays one stream twice without resetting anything, so an implementation that
        // remembered the first pass would answer differently on the second and be right about
        // neither. Per-event purity is the only reading that survives its own test.
        let reason = match event.kind() {
            FsEventKind::Closed => Some(BoundaryReason::Closed),
            FsEventKind::Synced => Some(BoundaryReason::Synced),
            FsEventKind::Renamed => Some(BoundaryReason::RenamedIntoPlace),
            // An open, a write and a flush are all mid-save. `MetadataSettled` needs to know that
            // metadata STOPPED changing, which no single event can say, so it is never offered
            // from here rather than being guessed.
            FsEventKind::Opened
            | FsEventKind::Written
            | FsEventKind::Flushed
            | FsEventKind::Unlinked
            | FsEventKind::MetadataChanged => None,
        };
        Ok(reason.map(|reason| CheckpointCandidate::new(event.view(), event.sequence(), reason)))
    }

    fn observe_durable_boundary_v1(
        &self,
        event: &FsEvent,
        rename: Option<&RenameEvidence>,
    ) -> Result<BoundaryObservationV1, AdapterError> {
        self.guard(AdapterCapability::ObserveDurableBoundary)?;
        let Some(view) = self.find(event.view()) else {
            return Err(AdapterError::UnknownView);
        };
        if view.is_released() {
            return Err(AdapterError::UnknownView);
        }
        let candidate = |reason| {
            BoundaryObservationV1::Candidate(CheckpointCandidateV1::new(
                event.view(),
                event.sequence(),
                reason,
            ))
        };
        Ok(match event.kind() {
            FsEventKind::Closed if view.tree().modified_handle_count(event.object())? == 0 => {
                candidate(BoundaryReasonV1::Closed)
            }
            FsEventKind::Closed => BoundaryObservationV1::None,
            FsEventKind::Synced => candidate(BoundaryReasonV1::Synced),
            FsEventKind::Renamed => match rename {
                None => BoundaryObservationV1::Unsupported(RenameEvidenceUnavailable::all()),
                Some(RenameEvidence::Unsupported(unavailable)) => {
                    BoundaryObservationV1::Unsupported(unavailable.clone())
                }
                Some(RenameEvidence::Available(evidence)) => {
                    if !evidence.matches(event) {
                        return Err(AdapterError::Backend(
                            "rename evidence does not match the event identity".to_owned(),
                        ));
                    }
                    let identity = evidence.identity();
                    if identity.moved_object() != MovedObjectIdentity::Preserved {
                        return Err(AdapterError::Backend(
                            "rename evidence changes moved-object identity".to_owned(),
                        ));
                    }
                    if identity.destination_binding() == DestinationBindingOutcome::Replaced {
                        candidate(BoundaryReasonV1::RenamedIntoPlace)
                    } else {
                        BoundaryObservationV1::None
                    }
                }
            },
            FsEventKind::Opened
            | FsEventKind::Written
            | FsEventKind::Flushed
            | FsEventKind::Unlinked
            | FsEventKind::MetadataChanged => BoundaryObservationV1::None,
        })
    }

    fn view(&self, view: ViewId) -> Result<&dyn WorkspaceView, AdapterError> {
        let found = self.find(view).ok_or(AdapterError::UnknownView)?;
        if found.is_released() {
            return Err(AdapterError::UnknownView);
        }
        Ok(found as &dyn WorkspaceView)
    }

    fn release(&self, view: ViewId) -> Result<(), AdapterError> {
        let found = self.find(view).ok_or(AdapterError::UnknownView)?;
        if found.take_release() {
            Ok(())
        } else {
            Err(AdapterError::UnknownView)
        }
    }
}
