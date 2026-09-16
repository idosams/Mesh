//! The folder-watching fallback backend — the first `WorkspaceAdapter` that touches a real
//! filesystem.
//!
//! # One sentence this module is built around
//!
//! **This backend owns a folder, and everything it answers it answers by reading that folder.**
//! No in-memory tree stands in for the filesystem: an object identity is the device and inode the
//! kernel reports, a directory listing is `read_dir`, a read is `pread`, and the index this
//! backend keeps is a *cache* that a re-reading of the folder repairs. That is what makes running
//! `mesh_materializer::run_conformance_v1` against it is evidence the seam works outside
//! memory — plan §7.4's raw watcher fallback, task `01KZC2QR9VVJK6Y60PS8D360JT`.
//!
//! # Why it is here, and where it used to be
//!
//! It used to be a module of `crates/mesh-materializer/tests/adapter-conformance.rs`, because a
//! `WorkspaceAdapter` implementation has to name `mesh_materializer` and no crate in this
//! workspace declared that edge. The consequence was not cosmetic: the backend compiled only
//! inside a test binary, so **no process a person can start could reach it**, and the product half
//! next door in [`crate::fallback`] was not reachable either — `lib.rs` never declared the module,
//! so `fallback.rs` was source text the compiler never saw. Both are fixed here. `mesh-daemon`
//! declares a `path` edge onto `mesh-materializer`, which
//! `docs/adr/0014-narrow-the-lockfile-fence-to-admit-an-audited-cryptographic-dependency.md`
//! admits between two members of this workspace without its five conditions, and the two halves
//! are now one crate that `meshd` and `meshctl` link against.
//!
//! # What it declares, and the three things it does not
//!
//! Sixteen of the eighteen capabilities. Not [`AdapterCapability::Symlink`], which is reserved
//! and has no operation at contract 1. And **not
//! [`AdapterCapability::ObserveDurableBoundary`]**,
//! which is the honest half of this backend: re-reading a folder never shows an open, a flush, a
//! close or an `fsync`, so this backend cannot decide when an application finished writing.
//! Declaring it and answering `Ok(None)` to everything would pass the `BND` family while claiming
//! a judgement it has no way to make. It refuses instead, and the `BND` family grades
//! `unsupported` — which is the word an honest partial backend earns.
//!
//! # Confinement, and what that costs against the conformance suite
//!
//! The suite mounts at `/mesh/conformance/one` and presents at `/mesh/conformance/shared`. No
//! process that is not root can create `/mesh` on macOS or Linux, so a backend that took those
//! paths literally would answer a platform failure and leave every family below the mount
//! ungraded. This backend is **rooted**: it owns one folder, and a path it is handed names a
//! location *inside* that folder. That is the `confined-to-its-folder` restriction, it is
//! published in [`crate::fallback::FallbackRestriction`] and said to a person in
//! [`crate::user_messages`], and it is the finding this run owes the suite's author:
//! [`AdapterFixture`] fixed the workspace and the head, and the mountpoint and the target are the
//! same gap one layer out. Filed as `01KZG5J7H8X16W56RX9AGWMCRJ`.
//!
//! # Unix only
//!
//! An object identity here is the device and inode the kernel reports, and the portable metadata
//! is a mode bit. `crate::ipc::server` is gated the same way and for the same reason. On any other
//! platform the fallback is absent, which is also the product answer: a degraded backend presented
//! as though it were the real one is exactly what plan §7.4 forbids.

use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use mesh_materializer::{
    ActorId, AdapterCapability, AdapterDescription, AdapterError, AdapterFixture, CapabilitySet,
    CheckpointCandidate, FsEvent, HeadId, MaterializedView, MountedView, NameError, NormalizedName,
    ObjectId, PortableMetadata, ViewAccess, ViewId, WorkspaceAdapter, WorkspaceId, WorkspaceView,
    WORKSPACE_ADAPTER_CONTRACT_V1,
};

use crate::fallback::FallbackRestriction;

pub mod view;
pub mod watch;

pub use crate::folder_watch::view::DirectoryView;

/// The backend's own name, as it describes itself.
pub const ADAPTER_NAME: &str = "mesh-directory-fallback/1";

/// Everything this backend implements.
///
/// `ObserveDurableBoundary` and `Symlink` are absent; the header says why.
pub const DECLARED: CapabilitySet = CapabilitySet::ALL
    .without(AdapterCapability::ObserveDurableBoundary)
    .without(AdapterCapability::Symlink);

/// The identifiers of the restrictions this backend works under.
///
/// **Not a second list.** It is [`FallbackRestriction::ALL`] mapped through
/// [`FallbackRestriction::id`], so a restriction added to the product enumeration is added here by
/// the compiler and one removed from it cannot be left behind. Until the workspace edge landed
/// these were two hand-kept arrays in two crates held together by a lint that compared source
/// text; a lint of that shape reports only what somebody remembered to write down twice.
pub const DECLARED_RESTRICTIONS: [&str; FallbackRestriction::ALL.len()] = declared_restrictions();

/// Build [`DECLARED_RESTRICTIONS`] at compile time.
const fn declared_restrictions() -> [&'static str; FallbackRestriction::ALL.len()] {
    let mut ids = [""; FallbackRestriction::ALL.len()];
    let mut index = 0;
    while index < FallbackRestriction::ALL.len() {
        ids[index] = FallbackRestriction::ALL[index].id();
        index += 1;
    }
    ids
}

// ---------------------------------------------------------------------------------------------
// Shared helpers — the two functions `watch.rs` needs as well
// ---------------------------------------------------------------------------------------------

/// The identity of an object on a real filesystem: the device it lives on and its inode.
///
/// **Chosen because it survives a rename and a move**, which is what lets a re-reading of the
/// folder work out that a file it now sees under a new name is the file it saw before. A name
/// would not survive one; a digest of the bytes would not survive an edit. [`watch::Snapshot`]
/// privately carries the kernel's inode-allocation discriminator where macOS or Linux supplies
/// one, so an immediately recycled inode cannot impersonate a move. The public identity remains
/// this stable pair, and [`watch::reconcile`] pairs only an unambiguous matching incarnation.
#[must_use]
pub fn object_of(metadata: &fs::Metadata) -> ObjectId {
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&metadata.dev().to_be_bytes());
    bytes[8..].copy_from_slice(&metadata.ino().to_be_bytes());
    ObjectId::from_bytes(bytes)
}

/// The portable metadata a real file carries.
///
/// One bit, because [`PortableMetadata`] has one field. A directory always answers not-executable:
/// the search bit on a directory is not the same fact as the execute bit on a file, and reporting
/// one as the other would make every directory in a listing look like a program.
#[must_use]
pub fn portable_of(metadata: &fs::Metadata) -> PortableMetadata {
    PortableMetadata::new(metadata.is_file() && metadata.mode() & 0o111 != 0)
}

fn mode_for(portable: PortableMetadata, is_directory: bool) -> u32 {
    // A directory needs its search bit to be enterable at all, so it takes the same mode as an
    // executable file for a different reason. Written as one condition rather than two identical
    // arms, which is what the reader — and clippy — should see.
    if is_directory || portable.is_executable() {
        0o755
    } else {
        0o644
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A platform failure the contract does not name.
///
/// The message is for a human and never a byte of file content, which is what
/// [`AdapterError::Backend`] is for.
fn backend(error: &io::Error) -> AdapterError {
    AdapterError::Backend(format!("{error}"))
}

/// `NotFound` for an absent entry, and only for an absent entry.
///
/// A permission failure is not an absent file, and answering `NotFound` for one would tell a
/// caller the work is gone when it is merely unreadable.
fn missing_or_backend(error: &io::Error) -> AdapterError {
    if error.kind() == io::ErrorKind::NotFound {
        AdapterError::NotFound
    } else {
        backend(error)
    }
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

/// Interpret `requested` inside `base`.
///
/// The `confined-to-its-folder` restriction, in one function: a leading `/` names the top of *this
/// backend's* folder, not the top of the machine. [`refuse_relative`] has already rejected every
/// `..`, so no component here can climb out.
fn confine(base: &Path, requested: &Path) -> PathBuf {
    let mut at = base.to_path_buf();
    for component in requested.components() {
        if let Component::Normal(text) = component {
            at.push(text);
        }
    }
    at
}

/// A folder under the system scratch directory that nothing else is using.
///
/// `mesh-materializer` may declare no dependency, so there is no `tempfile` here. The process
/// identifier and a counter are what `crates/mesh-store/tests/common/mod.rs` and
/// `crates/mesh-cas/tests/support/mod.rs` already use for the same reason.
fn scratch(label: &str) -> PathBuf {
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    let serial = SERIAL.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "mesh-directory-fallback-{label}-{}-{serial}",
        std::process::id()
    ))
}

/// Fold a path into a fixed-width identifier this backend will recognise again.
fn derive(seed: &Path, salt: u8, into: &mut [u8]) {
    let text = seed.to_string_lossy();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325 ^ u64::from(salt);
    for (index, slot) in into.iter_mut().enumerate() {
        for byte in text.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash = hash.wrapping_add(index as u64 + 1);
        *slot = (hash >> 24) as u8;
    }
}

// ---------------------------------------------------------------------------------------------
// The adapter
// ---------------------------------------------------------------------------------------------

/// A backend that serves one folder on the real filesystem.
pub struct DirectoryAdapter {
    root: PathBuf,
    remove_root_on_drop: bool,
    workspace: WorkspaceId,
    head: HeadId,
    prepared: Mutex<bool>,
    views: Mutex<Vec<&'static DirectoryView>>,
    released: Mutex<Vec<ViewId>>,
    next: AtomicU64,
}

impl DirectoryAdapter {
    /// A backend rooted at a scratch folder of its own, which it removes when it is dropped.
    #[must_use]
    pub fn in_scratch(label: &str) -> Self {
        Self::with_root(scratch(label), true)
    }

    /// A backend rooted at caller-owned `root`, which remains on disk when this value is dropped.
    #[must_use]
    pub fn at(root: PathBuf) -> Self {
        Self::with_root(root, false)
    }

    fn with_root(root: PathBuf, remove_root_on_drop: bool) -> Self {
        let mut workspace = [0u8; 16];
        let mut head = [0u8; 32];
        derive(&root, 0x11, &mut workspace);
        derive(&root, 0x22, &mut head);
        Self {
            root,
            remove_root_on_drop,
            workspace: WorkspaceId::from_bytes(workspace),
            head: HeadId::from_bytes(head),
            prepared: Mutex::new(false),
            views: Mutex::new(Vec::new()),
            released: Mutex::new(Vec::new()),
            next: AtomicU64::new(0),
        }
    }

    /// The folder this backend owns.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where an actor's own work is kept.
    #[must_use]
    pub fn workspace_root(&self) -> PathBuf {
        self.root.join("workspace")
    }

    /// Where the earlier version this backend can present is kept.
    #[must_use]
    pub fn head_root(&self) -> PathBuf {
        self.root.join("head")
    }

    /// The workspace identifier this backend accepts, and no other.
    #[must_use]
    pub const fn workspace(&self) -> WorkspaceId {
        self.workspace
    }

    /// The head identifier this backend accepts, and no other.
    #[must_use]
    pub const fn head(&self) -> HeadId {
        self.head
    }

    fn guard(&self, capability: AdapterCapability) -> Result<(), AdapterError> {
        if DECLARED.contains(capability) {
            Ok(())
        } else {
            Err(AdapterError::unsupported(capability))
        }
    }

    fn new_view(&self, at: PathBuf, access: ViewAccess) -> Result<ViewId, AdapterError> {
        let metadata = fs::symlink_metadata(&at).map_err(|error| backend(&error))?;
        let id = ViewId::new(self.next.fetch_add(1, Ordering::SeqCst) + 1);
        let view: &'static DirectoryView =
            Box::leak(Box::new(DirectoryView::new(id, access, at, &metadata)));
        lock(&self.views).push(view);
        Ok(id)
    }

    fn knows(&self, id: ViewId) -> bool {
        lock(&self.views).iter().any(|view| view.id() == id)
    }

    fn is_released(&self, id: ViewId) -> bool {
        lock(&self.released).contains(&id)
    }
}

impl Drop for DirectoryAdapter {
    fn drop(&mut self) {
        if self.remove_root_on_drop {
            // Best effort. A scratch folder left behind is untidy; a test that failed because
            // tidying it failed would be worse. Caller-owned roots must survive an adapter restart.
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

impl WorkspaceAdapter for DirectoryAdapter {
    fn describe(&self) -> AdapterDescription {
        AdapterDescription::new(ADAPTER_NAME, WORKSPACE_ADAPTER_CONTRACT_V1, DECLARED)
    }

    fn prepare_fixture(&self) -> Result<AdapterFixture, AdapterError> {
        fs::create_dir_all(self.workspace_root()).map_err(|error| backend(&error))?;
        fs::create_dir_all(self.head_root()).map_err(|error| backend(&error))?;
        fs::write(
            self.head_root().join("earlier-version.txt"),
            b"an earlier version of this workspace\n",
        )
        .map_err(|error| backend(&error))?;
        *lock(&self.prepared) = true;
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
        if !*lock(&self.prepared) || workspace != self.workspace {
            // A real backend does not hold a workspace it was never given. This is what makes
            // `prepare_fixture` load-bearing rather than decorative.
            return Err(AdapterError::NotFound);
        }
        let at = confine(&self.workspace_root(), mountpoint);
        fs::create_dir_all(&at).map_err(|error| backend(&error))?;
        let id = self.new_view(at, ViewAccess::ReadWrite)?;
        Ok(MountedView::new(id, workspace, actor, mountpoint))
    }

    fn materialize_readonly_view(
        &self,
        head: HeadId,
        target: &Path,
    ) -> Result<MaterializedView, AdapterError> {
        self.guard(AdapterCapability::MaterializeReadonlyView)?;
        refuse_relative(target)?;
        if !*lock(&self.prepared) || head != self.head {
            return Err(AdapterError::NotFound);
        }
        let at = confine(&self.root.join("presented"), target);
        fs::create_dir_all(&at).map_err(|error| backend(&error))?;
        let listing = fs::read_dir(self.head_root()).map_err(|error| backend(&error))?;
        for entry in listing.flatten() {
            if entry.path().is_file() {
                fs::copy(entry.path(), at.join(entry.file_name()))
                    .map_err(|error| backend(&error))?;
            }
        }
        let id = self.new_view(at, ViewAccess::ReadOnly)?;
        Ok(MaterializedView::new(id, head, target))
    }

    fn observe_durable_boundary(
        &self,
        _event: &FsEvent,
    ) -> Result<Option<CheckpointCandidate>, AdapterError> {
        // Not declared, so this is the only answer the contract allows — and it is the truth:
        // re-reading a folder shows a file's bytes, never the moment an application stopped
        // writing them.
        Err(AdapterError::unsupported(
            AdapterCapability::ObserveDurableBoundary,
        ))
    }

    fn view(&self, view: ViewId) -> Result<&dyn WorkspaceView, AdapterError> {
        if self.is_released(view) {
            return Err(AdapterError::UnknownView);
        }
        let found = lock(&self.views)
            .iter()
            .find(|candidate| candidate.id() == view)
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
