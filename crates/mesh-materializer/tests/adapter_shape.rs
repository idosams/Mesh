//! The adapter seam is implementable from outside this crate, and the two shapes that are easy to
//! get wrong are reachable.
//!
//! # What this file is, and what it is deliberately is not
//!
//! **Not the conformance suite.** `crates/mesh-materializer/src/conformance.rs` and
//! `tests/adapter-conformance.rs` are the oracle three backends are graded by, and plan §14.3 puts
//! that oracle with a different run than the one that wrote the trait. Nothing here grades an
//! adapter, nothing here is a reference implementation of the filesystem operations, and the
//! adapter below deliberately implements *none* of them.
//!
//! What it does establish, from a crate that only sees the published API:
//!
//! 1. A backend can be written against the re-exported names alone — if a type needed to construct
//!    one were still private, this file would not compile.
//! 2. **`WorkspaceAdapter::view` returning a borrow is implementable without `unsafe` and without a
//!    dependency**, which is the one signature in the contract whose cost falls on every backend.
//!    The pattern is here so the FUSE and FSKit lanes do not each have to find it.
//! 3. **Declaring nothing and refusing cleanly is a legal backend.** That is the answer the
//!    contract asks a partial backend to give, and a trait that made it awkward would push
//!    backends toward the silent divergence the capability probe exists to catch.
//! 4. Two callers use one adapter from two threads with no lock around the adapter itself.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use mesh_materializer::{
    ActorId, AdapterCapability, AdapterDescription, AdapterError, AdapterFixture, CapabilitySet,
    CheckpointCandidate, FsEvent, HeadId, MaterializedView, MountedView, NormalizedName, ObjectId,
    OpenHandle, OpenMode, PortableMetadata, ViewAccess, ViewEntry, ViewId, WorkspaceAdapter,
    WorkspaceId, WorkspaceView, WORKSPACE_ADAPTER_CONTRACT,
};

/// A backend that has not been built yet, and says so.
///
/// It owns its one view inline, so `view()` hands out a borrow of a field — the whole of what the
/// borrow-returning signature costs a backend with a fixed set of views. A backend with a growing
/// set pays for an arena or an append-only store instead; either way `release` unregisters an
/// identifier rather than freeing what a borrow may still point at.
struct NotBuiltYet {
    view: RefusingView,
    released: AtomicBool,
    mounts_attempted: AtomicU64,
}

impl NotBuiltYet {
    fn new() -> Self {
        Self {
            view: RefusingView { id: ViewId::new(1) },
            released: AtomicBool::new(false),
            mounts_attempted: AtomicU64::new(0),
        }
    }
}

impl WorkspaceAdapter for NotBuiltYet {
    fn describe(&self) -> AdapterDescription {
        AdapterDescription::new(
            "not-built-yet/0",
            WORKSPACE_ADAPTER_CONTRACT,
            CapabilitySet::EMPTY,
        )
    }

    fn prepare_fixture(&self) -> Result<AdapterFixture, AdapterError> {
        // A backend that has nothing can still answer, and answering costs it a line. Nothing is
        // ever mounted or presented with these, because nothing is declared; the point of the
        // method is that a backend which *does* hold state has somewhere to say so.
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
        // `&self`, and the counter moves anyway: the signature does not force a backend to
        // serialise two actors mounting at once, which is the reason it is `&self`.
        self.mounts_attempted.fetch_add(1, Ordering::Relaxed);
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

    fn view(&self, view: ViewId) -> Result<&dyn WorkspaceView, AdapterError> {
        if view != self.view.id || self.released.load(Ordering::SeqCst) {
            return Err(AdapterError::UnknownView);
        }
        Ok(&self.view)
    }

    fn release(&self, view: ViewId) -> Result<(), AdapterError> {
        if view != self.view.id || self.released.swap(true, Ordering::SeqCst) {
            return Err(AdapterError::UnknownView);
        }
        Ok(())
    }
}

/// A view that implements none of the operations and refuses each one by name.
struct RefusingView {
    id: ViewId,
}

impl WorkspaceView for RefusingView {
    fn id(&self) -> ViewId {
        self.id
    }

    fn access(&self) -> ViewAccess {
        ViewAccess::ReadOnly
    }

    fn root(&self) -> ObjectId {
        ObjectId::from_bytes([0; 16])
    }

    fn lookup(&self, _parent: ObjectId, _name: &NormalizedName) -> Result<ViewEntry, AdapterError> {
        Err(AdapterError::unsupported(AdapterCapability::Lookup))
    }

    fn enumerate(&self, _directory: ObjectId) -> Result<Vec<ViewEntry>, AdapterError> {
        Err(AdapterError::unsupported(AdapterCapability::Enumerate))
    }

    fn open(&self, _object: ObjectId, _mode: OpenMode) -> Result<OpenHandle, AdapterError> {
        Err(AdapterError::unsupported(AdapterCapability::Open))
    }

    fn close(&self, _handle: OpenHandle) -> Result<(), AdapterError> {
        Err(AdapterError::unsupported(AdapterCapability::Open))
    }

    fn read(
        &self,
        _handle: &OpenHandle,
        _offset: u64,
        _into: &mut [u8],
    ) -> Result<usize, AdapterError> {
        Err(AdapterError::unsupported(AdapterCapability::Read))
    }

    fn write(
        &self,
        _handle: &OpenHandle,
        _offset: u64,
        _from: &[u8],
    ) -> Result<usize, AdapterError> {
        Err(AdapterError::unsupported(AdapterCapability::Write))
    }

    fn create_file(
        &self,
        _parent: ObjectId,
        _name: &NormalizedName,
        _metadata: PortableMetadata,
    ) -> Result<ViewEntry, AdapterError> {
        Err(AdapterError::unsupported(AdapterCapability::CreateFile))
    }

    fn create_directory(
        &self,
        _parent: ObjectId,
        _name: &NormalizedName,
    ) -> Result<ViewEntry, AdapterError> {
        Err(AdapterError::unsupported(
            AdapterCapability::CreateDirectory,
        ))
    }

    fn rename(
        &self,
        _parent: ObjectId,
        _from: &NormalizedName,
        _to: &NormalizedName,
    ) -> Result<(), AdapterError> {
        Err(AdapterError::unsupported(AdapterCapability::Rename))
    }

    fn move_entry(
        &self,
        _from_parent: ObjectId,
        _from: &NormalizedName,
        _to_parent: ObjectId,
        _to: &NormalizedName,
    ) -> Result<(), AdapterError> {
        Err(AdapterError::unsupported(AdapterCapability::Move))
    }

    fn unlink(&self, _parent: ObjectId, _name: &NormalizedName) -> Result<(), AdapterError> {
        Err(AdapterError::unsupported(AdapterCapability::Unlink))
    }

    fn remove_directory(
        &self,
        _parent: ObjectId,
        _name: &NormalizedName,
    ) -> Result<(), AdapterError> {
        Err(AdapterError::unsupported(
            AdapterCapability::RemoveDirectory,
        ))
    }

    fn metadata(&self, _object: ObjectId) -> Result<PortableMetadata, AdapterError> {
        Err(AdapterError::unsupported(AdapterCapability::ReadMetadata))
    }

    fn set_metadata(
        &self,
        _object: ObjectId,
        _metadata: PortableMetadata,
    ) -> Result<(), AdapterError> {
        Err(AdapterError::unsupported(AdapterCapability::WriteMetadata))
    }
}

/// The daemon holds whichever backend it selected as `&dyn WorkspaceAdapter`. If that stopped
/// working, this line would stop compiling, which is the earliest anything could notice.
fn as_object(adapter: &NotBuiltYet) -> &dyn WorkspaceAdapter {
    adapter
}

#[test]
fn a_backend_that_declares_nothing_still_answers_every_question() {
    let adapter = NotBuiltYet::new();
    let adapter = as_object(&adapter);

    assert_eq!(adapter.describe().contract(), WORKSPACE_ADAPTER_CONTRACT);
    assert!(adapter.describe().capabilities().is_empty());

    assert_eq!(
        adapter.mount_actor_view(
            WorkspaceId::from_bytes([1; 16]),
            ActorId::from_bytes([2; 32]),
            Path::new("/mesh/alice"),
        ),
        Err(AdapterError::unsupported(AdapterCapability::MountActorView))
    );
    assert_eq!(
        adapter.materialize_readonly_view(HeadId::from_bytes([3; 32]), Path::new("/mesh/shared")),
        Err(AdapterError::unsupported(
            AdapterCapability::MaterializeReadonlyView
        ))
    );
}

/// The one signature whose cost falls on every backend, exercised rather than asserted: a borrow
/// comes back out of `&self`, survives being used, and stops resolving after `release`.
#[test]
fn a_view_resolves_to_a_borrow_and_a_released_one_stops_resolving() {
    let adapter = NotBuiltYet::new();

    let view = adapter.view(ViewId::new(1)).expect("the issued view");
    assert_eq!(view.id(), ViewId::new(1));
    assert_eq!(view.access(), ViewAccess::ReadOnly);
    assert_eq!(
        view.enumerate(view.root()),
        Err(AdapterError::unsupported(AdapterCapability::Enumerate))
    );

    // `Result<&dyn WorkspaceView, _>` is not comparable, because the contract does not force a
    // backend to implement `Debug` on its views. The refusal is matched instead of compared.
    assert!(matches!(
        adapter.view(ViewId::new(9)),
        Err(AdapterError::UnknownView)
    ));

    assert_eq!(adapter.release(ViewId::new(1)), Ok(()));
    assert!(
        matches!(adapter.view(ViewId::new(1)), Err(AdapterError::UnknownView)),
        "a released view identifier never resolves to a stale view"
    );
    assert_eq!(
        adapter.release(ViewId::new(1)),
        Err(AdapterError::UnknownView),
        "releasing twice is an error, not a silent success"
    );
}

/// `&self` on every method is the epic's "no global actor-workspace lock" made structural. Two
/// threads share one adapter here with no lock around the adapter at all.
#[test]
fn one_adapter_serves_two_threads_with_no_lock_around_it() {
    let adapter = NotBuiltYet::new();
    let shared: &(dyn WorkspaceAdapter + Sync) = &adapter;

    std::thread::scope(|scope| {
        for actor in 0..2u8 {
            scope.spawn(move || {
                let refusal = shared.mount_actor_view(
                    WorkspaceId::from_bytes([1; 16]),
                    ActorId::from_bytes([actor; 32]),
                    Path::new("/mesh"),
                );
                assert_eq!(
                    refusal,
                    Err(AdapterError::unsupported(AdapterCapability::MountActorView))
                );
            });
        }
    });

    assert_eq!(
        adapter.mounts_attempted.load(Ordering::Relaxed),
        2,
        "both threads reached the backend"
    );
}

/// A backend declares its set as an associated constant, which no run-time path can widen.
#[test]
fn a_capability_set_is_const_constructible() {
    const FUSE_LIKE: CapabilitySet = CapabilitySet::ALL.without(AdapterCapability::Symlink);
    assert!(!FUSE_LIKE.contains(AdapterCapability::Symlink));
    assert_eq!(FUSE_LIKE.len(), AdapterCapability::ALL.len() - 1);
    assert!(FUSE_LIKE.contains(AdapterCapability::Write));
}

/// The refusal a backend gives is the one the caller can act on: which capability, by name.
#[test]
fn an_unsupported_refusal_names_the_capability_it_refused() {
    let adapter = NotBuiltYet::new();
    let view = adapter.view(ViewId::new(1)).expect("the issued view");
    let handle = OpenHandle::new(
        0,
        ViewId::new(1),
        ObjectId::from_bytes([0; 16]),
        OpenMode::ReadWrite,
    );
    let mut buffer = [0u8; 8];

    match view.read(&handle, 0, &mut buffer) {
        Err(AdapterError::Unsupported { capability }) => {
            assert_eq!(capability, AdapterCapability::Read);
        }
        other => panic!("a backend that declared nothing answered {other:?}"),
    }
    assert!(view
        .write(&handle, 0, b"bytes")
        .unwrap_err()
        .to_string()
        .contains("Write"));
    assert_eq!(
        view.set_file_length(view.root(), 0),
        Err(AdapterError::unsupported(AdapterCapability::SetFileLength)),
        "the default keeps an older partial backend honest until it implements the new capability"
    );
}
