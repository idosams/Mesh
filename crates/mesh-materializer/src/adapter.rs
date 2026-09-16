//! The seam every filesystem backend plugs into.
//!
//! # One sentence this module is built around
//!
//! **An adapter hands out views, and a view is the filesystem.** The adapter owns lifetime and
//! platform; the view owns the operations a tool actually performs. Everything a backend must do
//! is decidable at those two surfaces, in memory, with no mount and no kernel — which is what lets
//! three backends written by three lanes that never speak be graded by one suite.
//!
//! Design `01KZEZGDPMZ5RH7E60WDYDYYEE` is the contract this module implements; task
//! `01KZC2M9305RFTMVSHJPG7HJGT` is where it came from; `tests/compatibility/adapter/v0/` is the
//! published half a backend author reads instead of this crate's internals.
//!
//! # Four shapes here are decisions, not accidents
//!
//! | Shape | Why it is this way |
//! |---|---|
//! | The trait is **synchronous** | A native `async fn` in a trait is not dyn-compatible, which `mesh-daemon` needs because it selects a backend at run time. Keeping the portable seam synchronous also avoids imposing an executor on every backend; a backend that wants one owns it in its own crate and blocks here. |
//! | Every method takes **`&self`** | The epic's exit criterion is "no global actor-workspace lock". `&mut self` would put one in the type system, for every backend at once. Interior mutability is the backend's business. |
//! | [`WorkspaceView::read`] and [`WorkspaceView::write`] **borrow a caller buffer and return a count** | It is the only shape that keeps this crate's stated "no byte of file content" invariant true across the seam; it is `pread`/`pwrite`, which is what both platform halves are handed already; and it makes a **short transfer observable**, which a `Vec<u8>` signature would not. |
//! | [`AdapterError`] carries **no errno** | `ENOTEMPTY` is 39 on Linux and 66 on Darwin. One integer in a portable crate is wrong on one of the two platforms Mesh ships, invisibly, and a Linux-only run would never see it. Each backend projects its own, and `tests/compatibility/adapter/v0/README.md` §5 carries the recommended table as guidance those lanes own. |
//!
//! # What a backend has to arrange, stated rather than discovered
//!
//! [`WorkspaceAdapter::view`] returns `&dyn WorkspaceView` borrowed from `&self`, so a backend
//! stores its views at **stable addresses** — an arena, a leaked box, an append-only vector — and
//! [`WorkspaceAdapter::release`] **unregisters** an identifier rather than freeing the memory
//! behind it. A released [`ViewId`] answers [`AdapterError::UnknownView`] from then on; it never
//! resolves to a stale view. That is the price of handing out a borrow instead of a handle, and it
//! is paid once per backend rather than by every caller.
//!
//! # Nothing here carries a clock
//!
//! [`FsEvent`] has no timestamp field and cannot acquire one: `src/` may not name a clock type at
//! all, and `tests/no_ambient_io.rs` is what says so. Order at this seam is the sequence number the
//! adapter mints, per view, monotone within a view and **not comparable across views** —
//! correlating two views is the daemon's, and this contract gives a backend no way to pretend
//! otherwise.
//!
//! # A backend that can do nothing is still a legal backend
//!
//! ```
//! use mesh_materializer::{
//!     AdapterCapability, AdapterDescription, AdapterError, AdapterFixture, CapabilitySet,
//!     CheckpointCandidate, FsEvent, HeadId, MaterializedView, MountedView, ViewId,
//!     WorkspaceAdapter, WorkspaceId, WorkspaceView, WORKSPACE_ADAPTER_CONTRACT,
//! };
//! use std::path::Path;
//!
//! struct NotYet;
//!
//! impl WorkspaceAdapter for NotYet {
//!     fn describe(&self) -> AdapterDescription {
//!         AdapterDescription::new("not-yet/0", WORKSPACE_ADAPTER_CONTRACT, CapabilitySet::EMPTY)
//!     }
//!     fn prepare_fixture(&self) -> Result<AdapterFixture, AdapterError> {
//!         // Nothing will be mounted with these, because nothing is declared. A backend that
//!         // holds real state names the workspace and the head it actually accepts.
//!         Ok(AdapterFixture::new(
//!             WorkspaceId::from_bytes([0; 16]),
//!             HeadId::from_bytes([0; 32]),
//!         ))
//!     }
//!     fn mount_actor_view(
//!         &self,
//!         _workspace: mesh_materializer::WorkspaceId,
//!         _actor: mesh_materializer::ActorId,
//!         _mountpoint: &Path,
//!     ) -> Result<MountedView, AdapterError> {
//!         Err(AdapterError::unsupported(AdapterCapability::MountActorView))
//!     }
//!     fn materialize_readonly_view(
//!         &self,
//!         _head: mesh_materializer::HeadId,
//!         _target: &Path,
//!     ) -> Result<MaterializedView, AdapterError> {
//!         Err(AdapterError::unsupported(AdapterCapability::MaterializeReadonlyView))
//!     }
//!     fn observe_durable_boundary(
//!         &self,
//!         _event: &FsEvent,
//!     ) -> Result<Option<CheckpointCandidate>, AdapterError> {
//!         Err(AdapterError::unsupported(AdapterCapability::ObserveDurableBoundary))
//!     }
//!     fn view(&self, _view: ViewId) -> Result<&dyn WorkspaceView, AdapterError> {
//!         Err(AdapterError::UnknownView)
//!     }
//!     fn release(&self, _view: ViewId) -> Result<(), AdapterError> {
//!         Err(AdapterError::UnknownView)
//!     }
//! }
//!
//! // Declaring nothing and refusing cleanly is a passing answer, and the seam is dyn-compatible.
//! let adapter: &dyn WorkspaceAdapter = &NotYet;
//! assert_eq!(adapter.describe().contract(), WORKSPACE_ADAPTER_CONTRACT);
//! assert!(adapter.describe().capabilities().is_empty());
//! ```

use core::fmt;
use std::path::{Path, PathBuf};

use crate::ids::{ActorId, HeadId, ObjectId, VersionId, WorkspaceId};
use crate::name::{NameError, NormalizedName, PortableMetadata};
use crate::version::ObjectKind;

/// The contract string every adapter returns from [`WorkspaceAdapter::describe`].
///
/// A version in a string rather than a number in a field: the whole of what a backend claims to
/// implement is one comparable token, and a backend built against a later contract is refused by
/// string inequality rather than by a silently-widened enumeration.
pub const WORKSPACE_ADAPTER_CONTRACT: &str = "mesh-workspace-adapter/0";

/// The opt-in contract that adds replacing rename evidence and the closed durable-boundary list.
///
/// Contract 0 remains available for backends that have not migrated. A backend claims contract 1
/// only when its disposition-aware operations and boundary observations implement the whole
/// extension.
pub const WORKSPACE_ADAPTER_CONTRACT_V1: &str = "mesh-workspace-adapter/1";

// ---------------------------------------------------------------------------------------------
// Identifiers minted by the adapter
// ---------------------------------------------------------------------------------------------

/// The identifier of one view an adapter has handed out.
///
/// Minted by the adapter and meaningful only to it. Two adapters may issue the same number for
/// different views, which is why nothing outside a single adapter compares two of these.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ViewId(u64);

impl ViewId {
    /// The view this number names.
    #[must_use]
    pub const fn new(number: u64) -> Self {
        Self(number)
    }

    /// The number.
    #[must_use]
    pub const fn number(self) -> u64 {
        self.0
    }
}

impl fmt::Display for ViewId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "view {}", self.0)
    }
}

/// One event's position in a view's own event stream.
///
/// Minted by the adapter, monotone **within one view**, and carrying no clock. It is not
/// comparable across views: two views are two independent streams, and the contract offers no
/// operation that would let a backend suggest otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventSequence(u64);

impl EventSequence {
    /// The position this number names.
    #[must_use]
    pub const fn new(number: u64) -> Self {
        Self(number)
    }

    /// The number.
    #[must_use]
    pub const fn number(self) -> u64 {
        self.0
    }
}

impl fmt::Display for EventSequence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, formatter)
    }
}

// ---------------------------------------------------------------------------------------------
// Capabilities
// ---------------------------------------------------------------------------------------------

/// One thing an adapter either does or explicitly refuses.
///
/// The list is closed. A backend declares exactly what it implements in [`AdapterDescription`],
/// and a capability it did not declare is still called once and graded — never asking cannot tell
/// a backend that has not built `write` from one whose `write` answers `Ok(n)` and drops the
/// bytes, and on a filesystem the second one is data loss.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AdapterCapability {
    /// Resolve one entry name inside one directory.
    Lookup,
    /// List a directory's entries, byte-lexicographically by name.
    Enumerate,
    /// Take and release a handle on an object.
    Open,
    /// Fill a caller-supplied buffer from an offset.
    Read,
    /// Take bytes from a caller-supplied slice at an offset.
    Write,
    /// Set a file's exact byte length.
    SetFileLength,
    /// Bind a new file object under a name in a directory.
    CreateFile,
    /// Bind a new directory object under a name in a directory.
    CreateDirectory,
    /// Change an entry's name within one directory.
    Rename,
    /// Move an entry from one directory to another.
    Move,
    /// Remove a name binding for a file.
    Unlink,
    /// Remove a name binding for an empty directory.
    RemoveDirectory,
    /// Read an object's portable metadata.
    ReadMetadata,
    /// Set an object's portable metadata.
    WriteMetadata,
    /// Present one actor's own state at a mountpoint, read-write.
    MountActorView,
    /// Present a named head at a target path, read-only.
    MaterializeReadonlyView,
    /// Decide, from one filesystem event, whether a durable boundary has been reached.
    ObserveDurableBoundary,
    /// Reserved. Declaring it opts a backend into the adversarial symlink corpus, which is a
    /// separate task; a backend that does not declare it refuses symlink creation.
    Symlink,
}

impl AdapterCapability {
    /// Every capability, in declaration order.
    ///
    /// The conformance catalogue is generated by iterating this and matching exhaustively over it,
    /// so a nineteenth variant with no case does not compile rather than leaving a silent gap.
    pub const ALL: [Self; 18] = [
        Self::Lookup,
        Self::Enumerate,
        Self::Open,
        Self::Read,
        Self::Write,
        Self::SetFileLength,
        Self::CreateFile,
        Self::CreateDirectory,
        Self::Rename,
        Self::Move,
        Self::Unlink,
        Self::RemoveDirectory,
        Self::ReadMetadata,
        Self::WriteMetadata,
        Self::MountActorView,
        Self::MaterializeReadonlyView,
        Self::ObserveDurableBoundary,
        Self::Symlink,
    ];

    /// The published name, which is the name in `tests/compatibility/adapter/v0/vocabulary.json`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lookup => "Lookup",
            Self::Enumerate => "Enumerate",
            Self::Open => "Open",
            Self::Read => "Read",
            Self::Write => "Write",
            Self::SetFileLength => "SetFileLength",
            Self::CreateFile => "CreateFile",
            Self::CreateDirectory => "CreateDirectory",
            Self::Rename => "Rename",
            Self::Move => "Move",
            Self::Unlink => "Unlink",
            Self::RemoveDirectory => "RemoveDirectory",
            Self::ReadMetadata => "ReadMetadata",
            Self::WriteMetadata => "WriteMetadata",
            Self::MountActorView => "MountActorView",
            Self::MaterializeReadonlyView => "MaterializeReadonlyView",
            Self::ObserveDurableBoundary => "ObserveDurableBoundary",
            Self::Symlink => "Symlink",
        }
    }

    /// This capability's place in the bitset.
    ///
    /// Eighteen capabilities and thirty-two bits, so widening the list is a compile error at
    /// thirty-three rather than a silently dropped capability.
    const fn bit(self) -> u32 {
        1u32 << (self as u32)
    }
}

impl fmt::Display for AdapterCapability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The set of capabilities one adapter declares.
///
/// A bitset rather than a collection: no allocation, no dependency, and `const`-constructible so a
/// backend can declare its set as an associated constant that no run-time path can widen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CapabilitySet(u32);

impl CapabilitySet {
    /// The set that declares nothing. A backend may legitimately return this; it is graded on the
    /// answers it gives, never on the claim it makes.
    pub const EMPTY: Self = Self(0);

    /// The set that declares everything in [`AdapterCapability::ALL`].
    pub const ALL: Self = {
        let mut bits = 0u32;
        let mut index = 0;
        while index < AdapterCapability::ALL.len() {
            bits |= AdapterCapability::ALL[index].bit();
            index += 1;
        }
        Self(bits)
    };

    /// This set with `capability` added.
    #[must_use]
    pub const fn with(self, capability: AdapterCapability) -> Self {
        Self(self.0 | capability.bit())
    }

    /// This set with `capability` removed.
    #[must_use]
    pub const fn without(self, capability: AdapterCapability) -> Self {
        Self(self.0 & !capability.bit())
    }

    /// Whether `capability` is declared.
    #[must_use]
    pub const fn contains(self, capability: AdapterCapability) -> bool {
        self.0 & capability.bit() != 0
    }

    /// How many capabilities are declared.
    #[must_use]
    pub const fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    /// Whether nothing is declared.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The declared capabilities, in [`AdapterCapability::ALL`] order.
    ///
    /// Declaration order rather than insertion order, so two backends that declare the same set
    /// render the same sequence and a report is comparable between them.
    pub fn iter(self) -> impl Iterator<Item = AdapterCapability> {
        AdapterCapability::ALL
            .into_iter()
            .filter(move |capability| self.contains(*capability))
    }
}

impl fmt::Display for CapabilitySet {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for capability in self.iter() {
            if !first {
                formatter.write_str(", ")?;
            }
            formatter.write_str(capability.as_str())?;
            first = false;
        }
        if first {
            formatter.write_str("(nothing declared)")?;
        }
        Ok(())
    }
}

/// [`AdapterCapability::ALL`] is in declaration order, and the bitset can hold it.
///
/// The conformance catalogue is generated by iterating `ALL`, so a capability missing from that
/// array is a capability nothing is ever graded on — the same silent gap the capability probe
/// exists to close, arriving through the vocabulary instead of through a backend. Two guards were
/// already here and neither closes it: the fixed length `[Self; 17]` catches an entry *removed*
/// from the array, and the exhaustive `match` in [`AdapterCapability::as_str`] catches a variant
/// *added to the enum*, but a variant added to the enum, named in `as_str`, and never added to
/// `ALL` compiles clean under both.
///
/// The first assertion closes most of that. It pins `ALL` to declaration order, so slot `i` holds
/// the variant with discriminant `i`; with the fixed length, `ALL` is then exactly the first
/// eighteen variants in order, and a capability *inserted* anywhere before the end shifts a
/// discriminant and stops the build. Verified by injection rather than assumed: inserting a variant
/// before [`AdapterCapability::Symlink`] and naming it in `as_str` fails `cargo build` with this
/// message.
///
/// **What it does not catch, stated rather than implied:** a variant *appended after* `Symlink`.
/// Nothing in stable Rust counts an enum's variants in a `const`, so no assertion here can compare
/// `ALL.len()` against the enum's real width. A sentinel that compares `ALL.len()` against
/// `Symlink as usize` plus one looks like it would and does not — appending leaves `Symlink` at
/// seventeen — and shipping it would be a guard that reads stronger than it is. That case belongs to
/// the suite's `VOC` family, which holds every declared variant against
/// `tests/compatibility/adapter/v0/vocabulary.json` in both directions (design
/// `01KZEZGDPMZ5RH7E60WDYDYYEE` Contract 8); the suite is owed by a separate run and until it lands
/// this case is unenforced. That lint reads the `pub enum` body as source text, which is also why
/// this enumeration stays a plain declaration and is not generated from a macro — a macro would
/// close this gap at the cost of blinding the check that closes the larger one.
///
/// The second assertion states [`CapabilitySet`]'s real ceiling. It is a `u32`, so a thirty-third
/// capability would shift past the end of the bitset: a panic in a debug build and a wrapped shift
/// in a release one, which is the worst of the three ways to find out.
const _: () = {
    let mut index = 0;
    while index < AdapterCapability::ALL.len() {
        assert!(
            AdapterCapability::ALL[index] as usize == index,
            "AdapterCapability::ALL is not in declaration order"
        );
        index += 1;
    }
    assert!(
        AdapterCapability::ALL.len() <= u32::BITS as usize,
        "CapabilitySet is a u32 and cannot carry this many capabilities"
    );
};

/// What one adapter is, and what it claims to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AdapterDescription {
    adapter: &'static str,
    contract: &'static str,
    capabilities: CapabilitySet,
}

impl AdapterDescription {
    /// The description of an adapter called `adapter`, implementing `contract`, declaring
    /// `capabilities`.
    ///
    /// `contract` is a parameter rather than a fixed constant on purpose: a backend built against
    /// a contract this crate no longer publishes must be able to *say so*, and a constructor that
    /// stamped the current constant on every description would make that case unrepresentable and
    /// the check that looks for it vacuous.
    #[must_use]
    pub const fn new(
        adapter: &'static str,
        contract: &'static str,
        capabilities: CapabilitySet,
    ) -> Self {
        Self {
            adapter,
            contract,
            capabilities,
        }
    }

    /// The adapter's own name, such as `"mesh-fuse/0"`.
    #[must_use]
    pub const fn adapter(&self) -> &'static str {
        self.adapter
    }

    /// The contract the adapter implements. Conformance requires this to equal
    /// [`WORKSPACE_ADAPTER_CONTRACT`].
    #[must_use]
    pub const fn contract(&self) -> &'static str {
        self.contract
    }

    /// What the adapter declares it does.
    #[must_use]
    pub const fn capabilities(&self) -> CapabilitySet {
        self.capabilities
    }
}

impl fmt::Display for AdapterDescription {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} implementing {} [{}]",
            self.adapter, self.contract, self.capabilities
        )
    }
}

// ---------------------------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------------------------

/// Why an adapter refused.
///
/// Closed, and matched exhaustively wherever it is graded, so a thirteenth variant is a compile
/// error at the case builder rather than an unhandled arm. There is **no `errno()`** here and that
/// is a decision: see this module's header, and `tests/compatibility/adapter/v0/README.md` §5 for
/// the per-platform table each backend owns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdapterError {
    /// The capability was not declared. This is the only answer an undeclared capability may give.
    Unsupported {
        /// The capability that was asked for.
        capability: AdapterCapability,
    },
    /// No entry, object or handle by that name or identifier.
    NotFound,
    /// The name is already bound in that directory.
    AlreadyExists,
    /// A directory operation was addressed to a file.
    NotADirectory,
    /// A file operation was addressed to a directory.
    IsADirectory,
    /// `remove_directory` was called on a directory that still has entries.
    DirectoryNotEmpty,
    /// The name broke a portable-name rule.
    ///
    /// Wraps the existing rule rather than restating it. A second writing-down of the name rules
    /// is a second thing that can disagree with the first.
    ///
    /// **Produced from [`WorkspaceAdapter::mount_actor_view`]'s mountpoint and
    /// [`WorkspaceAdapter::materialize_readonly_view`]'s target, and from nowhere else.** Every
    /// [`WorkspaceView`] method takes a [`NormalizedName`], which has one constructor and no way to
    /// hold a refused name, so no view method can ever be handed one and this variant is
    /// unreachable through all fifteen of them. On the view **the type is the enforcement**, which
    /// is stronger than a case: a rule the compiler makes unrepresentable cannot be got wrong by
    /// one backend on one platform. A `NameRejected` arm inside `lookup` or `create_file` is an arm
    /// nothing can reach. Ruled under `01KZFXF9N49NHJ7XS3MD0MX3BR`;
    /// `tests/compatibility/adapter/v0/README.md` §4 and §6 and `vocabulary.json` carry the same
    /// wording, and `tests/adapter-conformance.rs` fails if the three stop agreeing.
    NameRejected(NameError),
    /// The operation would resolve outside the workspace.
    OutsideWorkspace,
    /// A mutating operation was addressed to a view whose access is [`ViewAccess::ReadOnly`].
    ReadOnly,
    /// The [`ViewId`] was never issued, or has been released.
    UnknownView,
    /// The move would place a directory inside its own subtree.
    WouldCycle,
    /// A platform failure the contract does not name.
    ///
    /// The `String` is a message for a human, never file content — this crate carries no byte of
    /// that. It is never the right answer to a conformance case.
    Backend(String),
}

impl AdapterError {
    /// The refusal an undeclared `capability` must give.
    ///
    /// A named constructor because this is the one error a backend writes most often and the one
    /// whose exact shape is graded.
    #[must_use]
    pub const fn unsupported(capability: AdapterCapability) -> Self {
        Self::Unsupported { capability }
    }

    /// The published name of this variant, which is the name in
    /// `tests/compatibility/adapter/v0/vocabulary.json`.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Unsupported { .. } => "Unsupported",
            Self::NotFound => "NotFound",
            Self::AlreadyExists => "AlreadyExists",
            Self::NotADirectory => "NotADirectory",
            Self::IsADirectory => "IsADirectory",
            Self::DirectoryNotEmpty => "DirectoryNotEmpty",
            Self::NameRejected(_) => "NameRejected",
            Self::OutsideWorkspace => "OutsideWorkspace",
            Self::ReadOnly => "ReadOnly",
            Self::UnknownView => "UnknownView",
            Self::WouldCycle => "WouldCycle",
            Self::Backend(_) => "Backend",
        }
    }
}

impl fmt::Display for AdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { capability } => {
                write!(formatter, "this adapter does not do {capability}")
            }
            Self::NotFound => formatter.write_str("no entry, object or handle by that name"),
            Self::AlreadyExists => formatter.write_str("that name is already taken here"),
            Self::NotADirectory => formatter.write_str("that is a file, not a directory"),
            Self::IsADirectory => formatter.write_str("that is a directory, not a file"),
            Self::DirectoryNotEmpty => formatter.write_str("that directory still has entries"),
            Self::NameRejected(inner) => write!(formatter, "{inner}"),
            Self::OutsideWorkspace => formatter.write_str("that resolves outside the workspace"),
            Self::ReadOnly => formatter.write_str("this view is read-only"),
            Self::UnknownView => {
                formatter.write_str("that view was never issued here, or has been released")
            }
            Self::WouldCycle => {
                formatter.write_str("a directory cannot be moved inside its own subtree")
            }
            Self::Backend(message) => write!(formatter, "the backend failed: {message}"),
        }
    }
}

impl std::error::Error for AdapterError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NameRejected(inner) => Some(inner),
            _ => None,
        }
    }
}

impl From<NameError> for AdapterError {
    fn from(error: NameError) -> Self {
        Self::NameRejected(error)
    }
}

// ---------------------------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------------------------

/// Whether a view accepts mutation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ViewAccess {
    /// The view accepts every operation the adapter declares.
    ReadWrite,
    /// The view refuses every mutating operation with [`AdapterError::ReadOnly`].
    ReadOnly,
}

impl ViewAccess {
    /// Whether every mutating operation on this view must be refused.
    #[must_use]
    pub const fn is_read_only(self) -> bool {
        matches!(self, Self::ReadOnly)
    }
}

/// What a handle is being taken for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OpenMode {
    /// Reading only.
    Read,
    /// Writing only.
    Write,
    /// Both.
    ReadWrite,
}

impl OpenMode {
    /// Whether this mode permits [`WorkspaceView::write`].
    #[must_use]
    pub const fn writes(self) -> bool {
        matches!(self, Self::Write | Self::ReadWrite)
    }

    /// Whether this mode permits [`WorkspaceView::read`].
    #[must_use]
    pub const fn reads(self) -> bool {
        matches!(self, Self::Read | Self::ReadWrite)
    }
}

/// One open handle on one object in one view.
///
/// Deliberately **not `Copy`**: [`WorkspaceView::close`] takes it by value, so an ordinary caller
/// cannot use a handle after closing it, and a check that wants to probe use-after-close has to
/// clone it on purpose and thereby say so.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OpenHandle {
    handle: u64,
    view: ViewId,
    object: ObjectId,
    mode: OpenMode,
}

impl OpenHandle {
    /// The handle an adapter has just minted.
    #[must_use]
    pub const fn new(handle: u64, view: ViewId, object: ObjectId, mode: OpenMode) -> Self {
        Self {
            handle,
            view,
            object,
            mode,
        }
    }

    /// The adapter's own number for this handle, unique within its view.
    #[must_use]
    pub const fn handle(&self) -> u64 {
        self.handle
    }

    /// The view that issued it.
    #[must_use]
    pub const fn view(&self) -> ViewId {
        self.view
    }

    /// The object it is open on.
    #[must_use]
    pub const fn object(&self) -> ObjectId {
        self.object
    }

    /// What it was opened for.
    #[must_use]
    pub const fn mode(&self) -> OpenMode {
        self.mode
    }
}

/// One directory entry as a view reports it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ViewEntry {
    name: NormalizedName,
    object: ObjectId,
    kind: ObjectKind,
    version: Option<VersionId>,
    metadata: PortableMetadata,
}

impl ViewEntry {
    /// The entry binding `name` to `object`.
    ///
    /// `version` is `None` for a directory and for a file with no version yet — an entry that
    /// exists and names nothing durable is a real state, not an error, and collapsing it into one
    /// would make a freshly created file indistinguishable from a missing one.
    #[must_use]
    pub const fn new(
        name: NormalizedName,
        object: ObjectId,
        kind: ObjectKind,
        version: Option<VersionId>,
        metadata: PortableMetadata,
    ) -> Self {
        Self {
            name,
            object,
            kind,
            version,
            metadata,
        }
    }

    /// The entry name.
    #[must_use]
    pub const fn name(&self) -> &NormalizedName {
        &self.name
    }

    /// The object the name is bound to.
    #[must_use]
    pub const fn object(&self) -> ObjectId {
        self.object
    }

    /// Whether it is a file or a directory.
    #[must_use]
    pub const fn kind(&self) -> ObjectKind {
        self.kind
    }

    /// The version the entry names, if it names one.
    #[must_use]
    pub const fn version(&self) -> Option<VersionId> {
        self.version
    }

    /// The portable metadata, which survives [`WorkspaceView::rename`] and
    /// [`WorkspaceView::move_entry`] unchanged.
    #[must_use]
    pub const fn metadata(&self) -> PortableMetadata {
        self.metadata
    }
}

/// One actor's own state, mounted read-write.
///
/// A handle, not the filesystem: the operations are on the [`WorkspaceView`] that
/// [`WorkspaceAdapter::view`] resolves this identifier to.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MountedView {
    view: ViewId,
    workspace: WorkspaceId,
    actor: ActorId,
    mountpoint: PathBuf,
}

impl MountedView {
    /// The mount an adapter has just made.
    #[must_use]
    pub fn new(
        view: ViewId,
        workspace: WorkspaceId,
        actor: ActorId,
        mountpoint: impl Into<PathBuf>,
    ) -> Self {
        Self {
            view,
            workspace,
            actor,
            mountpoint: mountpoint.into(),
        }
    }

    /// The identifier [`WorkspaceAdapter::view`] resolves.
    #[must_use]
    pub const fn id(&self) -> ViewId {
        self.view
    }

    /// The workspace mounted.
    #[must_use]
    pub const fn workspace(&self) -> WorkspaceId {
        self.workspace
    }

    /// The actor whose own state this is.
    #[must_use]
    pub const fn actor(&self) -> ActorId {
        self.actor
    }

    /// Where it is mounted.
    #[must_use]
    pub fn mountpoint(&self) -> &Path {
        &self.mountpoint
    }

    /// Read-write, always. A mounted actor view that refused a write would be an actor unable to
    /// work in their own state.
    #[must_use]
    pub const fn access(&self) -> ViewAccess {
        ViewAccess::ReadWrite
    }
}

/// A named exact state, presented read-only.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MaterializedView {
    view: ViewId,
    head: HeadId,
    target: PathBuf,
}

impl MaterializedView {
    /// The read-only view an adapter has just produced.
    #[must_use]
    pub fn new(view: ViewId, head: HeadId, target: impl Into<PathBuf>) -> Self {
        Self {
            view,
            head,
            target: target.into(),
        }
    }

    /// The identifier [`WorkspaceAdapter::view`] resolves.
    #[must_use]
    pub const fn id(&self) -> ViewId {
        self.view
    }

    /// The head this view presents.
    #[must_use]
    pub const fn head(&self) -> HeadId {
        self.head
    }

    /// Where it is presented.
    #[must_use]
    pub fn target(&self) -> &Path {
        &self.target
    }

    /// Read-only by construction, not by a flag somebody might forget to set.
    #[must_use]
    pub const fn access(&self) -> ViewAccess {
        ViewAccess::ReadOnly
    }
}

// ---------------------------------------------------------------------------------------------
// What a backend prepares to be graded against
// ---------------------------------------------------------------------------------------------

/// One workspace this adapter will mount, and one head it will present.
///
/// # Why this exists, stated rather than discovered
///
/// Nothing else on either trait **creates** a workspace or **names** a head a backend already
/// holds. Without this, the only thing a caller who has not been handed one can do is invent an
/// identifier — and a real backend answers [`AdapterError::NotFound`] for a workspace it was never
/// given, which is correct of it and leaves everything downstream of a mount ungraded. That is a
/// backend passing while nothing about mounting, releasing or read-only refusal has been checked
/// at all, and it is why the arrangement is published here instead of being agreed privately
/// between one backend and one caller.
///
/// It carries identifiers and nothing else: no path, no capability, no handle. A backend that
/// holds no state of its own may answer with any pair it accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AdapterFixture {
    workspace: WorkspaceId,
    head: HeadId,
}

impl AdapterFixture {
    /// The workspace and head an adapter has just prepared.
    #[must_use]
    pub const fn new(workspace: WorkspaceId, head: HeadId) -> Self {
        Self { workspace, head }
    }

    /// The workspace [`WorkspaceAdapter::mount_actor_view`] will accept.
    #[must_use]
    pub const fn workspace(&self) -> WorkspaceId {
        self.workspace
    }

    /// The head [`WorkspaceAdapter::materialize_readonly_view`] will present.
    #[must_use]
    pub const fn head(&self) -> HeadId {
        self.head
    }
}

impl fmt::Display for AdapterFixture {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "workspace {} at head {}",
            self.workspace, self.head
        )
    }
}

// ---------------------------------------------------------------------------------------------
// Durable boundaries
// ---------------------------------------------------------------------------------------------

/// What happened at the filesystem.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FsEventKind {
    /// A handle was taken.
    Opened,
    /// Bytes were written through a handle.
    Written,
    /// A write was flushed.
    Flushed,
    /// A write was made durable.
    Synced,
    /// A handle was released.
    Closed,
    /// An entry was renamed or moved.
    Renamed,
    /// A name binding was removed.
    Unlinked,
    /// Portable metadata changed.
    MetadataChanged,
}

impl FsEventKind {
    /// The published name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Opened => "Opened",
            Self::Written => "Written",
            Self::Flushed => "Flushed",
            Self::Synced => "Synced",
            Self::Closed => "Closed",
            Self::Renamed => "Renamed",
            Self::Unlinked => "Unlinked",
            Self::MetadataChanged => "MetadataChanged",
        }
    }
}

/// One thing that happened in one view.
///
/// **No timestamp field, and it cannot acquire one.** This crate may not name a clock type at all;
/// order here is [`EventSequence`], minted per view by the adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FsEvent {
    view: ViewId,
    sequence: EventSequence,
    kind: FsEventKind,
    object: ObjectId,
}

/// What a contract-1 rename does when its destination is already bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RenameDisposition {
    /// Preserve contract-0 behavior and return [`AdapterError::AlreadyExists`].
    Fail,
    /// Atomically replace a destination file with the moved file.
    Replace,
}

/// One exact name binding in rename evidence.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RenameBinding {
    parent: ObjectId,
    name: NormalizedName,
    object: ObjectId,
}

impl RenameBinding {
    /// One binding, without name normalization or case folding beyond [`NormalizedName`].
    #[must_use]
    pub const fn new(parent: ObjectId, name: NormalizedName, object: ObjectId) -> Self {
        Self {
            parent,
            name,
            object,
        }
    }

    /// The exact parent object.
    #[must_use]
    pub const fn parent(&self) -> ObjectId {
        self.parent
    }

    /// The exact UTF-8 name bytes.
    #[must_use]
    pub const fn name(&self) -> &NormalizedName {
        &self.name
    }

    /// The object bound there.
    #[must_use]
    pub const fn object(&self) -> ObjectId {
        self.object
    }
}

/// What occupied the destination before a rename.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DestinationBefore {
    /// The destination name was not bound.
    Unbound,
    /// The destination name was bound to this object.
    Bound(ObjectId),
}

/// Complete binding evidence for one rename event.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RenameBindingEvidence {
    view: ViewId,
    sequence: EventSequence,
    source_before: RenameBinding,
    destination_before: DestinationBefore,
    destination_after: RenameBinding,
}

impl RenameBindingEvidence {
    /// Associate complete binding evidence with exactly one event.
    #[must_use]
    pub const fn new(
        view: ViewId,
        sequence: EventSequence,
        source_before: RenameBinding,
        destination_before: DestinationBefore,
        destination_after: RenameBinding,
    ) -> Self {
        Self {
            view,
            sequence,
            source_before,
            destination_before,
            destination_after,
        }
    }

    /// The event's view.
    #[must_use]
    pub const fn view(&self) -> ViewId {
        self.view
    }

    /// The event's position in that view.
    #[must_use]
    pub const fn sequence(&self) -> EventSequence {
        self.sequence
    }

    /// The source binding before the operation.
    #[must_use]
    pub const fn source_before(&self) -> &RenameBinding {
        &self.source_before
    }

    /// The destination binding before the operation.
    #[must_use]
    pub const fn destination_before(&self) -> DestinationBefore {
        self.destination_before
    }

    /// The destination binding after the operation.
    #[must_use]
    pub const fn destination_after(&self) -> &RenameBinding {
        &self.destination_after
    }

    /// The two independent identity facts consumers use.
    #[must_use]
    pub fn identity(&self) -> RenameIdentity {
        let moved_object = if self.source_before.object == self.destination_after.object {
            MovedObjectIdentity::Preserved
        } else {
            MovedObjectIdentity::Changed
        };
        let destination_binding = match self.destination_before {
            DestinationBefore::Unbound => DestinationBindingOutcome::Created,
            DestinationBefore::Bound(object) if object == self.destination_after.object => {
                DestinationBindingOutcome::Preserved
            }
            DestinationBefore::Bound(_) => DestinationBindingOutcome::Replaced,
        };
        RenameIdentity {
            moved_object,
            destination_binding,
        }
    }

    /// Whether this evidence is the evidence for `event`, including moved-object identity.
    #[must_use]
    pub fn matches(&self, event: &FsEvent) -> bool {
        self.view == event.view
            && self.sequence == event.sequence
            && self.destination_after.object == event.object
    }
}

/// Whether the object at the source survived the rename.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MovedObjectIdentity {
    /// Source-before and destination-after name the same object.
    Preserved,
    /// The object identity changed.
    Changed,
}

/// What happened to the destination name binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DestinationBindingOutcome {
    /// It was unbound and became bound.
    Created,
    /// It remained bound to the same object.
    Preserved,
    /// Its previous object was displaced.
    Replaced,
}

/// The independent moved-object and destination-binding answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RenameIdentity {
    moved_object: MovedObjectIdentity,
    destination_binding: DestinationBindingOutcome,
}

impl RenameIdentity {
    /// The moved object's outcome.
    #[must_use]
    pub const fn moved_object(self) -> MovedObjectIdentity {
        self.moved_object
    }

    /// The destination binding's outcome.
    #[must_use]
    pub const fn destination_binding(self) -> DestinationBindingOutcome {
        self.destination_binding
    }
}

/// One required member that a backend could not prove.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RenameEvidenceField {
    /// `source_before.parent`.
    SourceParent,
    /// `source_before.name`.
    SourceName,
    /// `source_before.object`.
    SourceObject,
    /// `destination_before.state`.
    DestinationBeforeState,
    /// `destination_before.object`.
    DestinationBeforeObject,
    /// `destination_after.parent`.
    DestinationAfterParent,
    /// `destination_after.name`.
    DestinationAfterName,
    /// `destination_after.object`.
    DestinationAfterObject,
}

impl RenameEvidenceField {
    /// Every required member, in contract order.
    pub const ALL: [Self; 8] = [
        Self::SourceParent,
        Self::SourceName,
        Self::SourceObject,
        Self::DestinationBeforeState,
        Self::DestinationBeforeObject,
        Self::DestinationAfterParent,
        Self::DestinationAfterName,
        Self::DestinationAfterObject,
    ];

    /// The executable contract member name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SourceParent => "source_before.parent",
            Self::SourceName => "source_before.name",
            Self::SourceObject => "source_before.object",
            Self::DestinationBeforeState => "destination_before.state",
            Self::DestinationBeforeObject => "destination_before.object",
            Self::DestinationAfterParent => "destination_after.parent",
            Self::DestinationAfterName => "destination_after.name",
            Self::DestinationAfterObject => "destination_after.object",
        }
    }
}

/// A clean, explicit refusal to claim rename identity evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenameEvidenceUnavailable {
    missing: Vec<RenameEvidenceField>,
}

impl RenameEvidenceUnavailable {
    /// Refuse with a non-empty, sorted, duplicate-free missing-member list.
    #[must_use]
    pub fn new(missing: impl IntoIterator<Item = RenameEvidenceField>) -> Option<Self> {
        let mut missing: Vec<_> = missing.into_iter().collect();
        missing.sort();
        missing.dedup();
        (!missing.is_empty()).then_some(Self { missing })
    }

    /// Refuse because no binding member is available.
    #[must_use]
    pub fn all() -> Self {
        Self {
            missing: RenameEvidenceField::ALL.to_vec(),
        }
    }

    /// The exact missing members.
    #[must_use]
    pub fn missing(&self) -> &[RenameEvidenceField] {
        &self.missing
    }

    /// The stable executable refusal code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        "rename-binding-evidence-unavailable"
    }
}

/// Available rename evidence or the explicit clean refusal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenameEvidence {
    /// Complete evidence.
    Available(RenameBindingEvidence),
    /// The backend names every field it cannot prove.
    Unsupported(RenameEvidenceUnavailable),
}

/// Contract-1's closed durable-boundary list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BoundaryReasonV1 {
    /// The last modified handle on the object was released.
    Closed,
    /// An fsync completed successfully.
    Synced,
    /// An atomic replacement completed with matching binding evidence.
    RenamedIntoPlace,
}

impl BoundaryReasonV1 {
    /// The published contract-1 name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Closed => "Closed",
            Self::Synced => "Synced",
            Self::RenamedIntoPlace => "RenamedIntoPlace",
        }
    }
}

/// One contract-1 durable boundary candidate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CheckpointCandidateV1 {
    view: ViewId,
    through: EventSequence,
    reason: BoundaryReasonV1,
}

impl CheckpointCandidateV1 {
    /// A candidate for one event position.
    #[must_use]
    pub const fn new(view: ViewId, through: EventSequence, reason: BoundaryReasonV1) -> Self {
        Self {
            view,
            through,
            reason,
        }
    }

    /// The view.
    #[must_use]
    pub const fn view(self) -> ViewId {
        self.view
    }

    /// The inclusive sequence.
    #[must_use]
    pub const fn through(self) -> EventSequence {
        self.through
    }

    /// The closed-list reason.
    #[must_use]
    pub const fn reason(self) -> BoundaryReasonV1 {
        self.reason
    }
}

/// A contract-1 boundary observation never disguises missing rename evidence as no boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BoundaryObservationV1 {
    /// This event is not a durable boundary.
    None,
    /// A reproducible producer emitted a candidate.
    Candidate(CheckpointCandidateV1),
    /// A rename could not be decided because evidence was unavailable.
    Unsupported(RenameEvidenceUnavailable),
}

impl FsEvent {
    /// The event an adapter observed.
    #[must_use]
    pub const fn new(
        view: ViewId,
        sequence: EventSequence,
        kind: FsEventKind,
        object: ObjectId,
    ) -> Self {
        Self {
            view,
            sequence,
            kind,
            object,
        }
    }

    /// The view it happened in.
    #[must_use]
    pub const fn view(&self) -> ViewId {
        self.view
    }

    /// Where it sits in that view's stream.
    #[must_use]
    pub const fn sequence(&self) -> EventSequence {
        self.sequence
    }

    /// What happened.
    #[must_use]
    pub const fn kind(&self) -> FsEventKind {
        self.kind
    }

    /// What it happened to.
    #[must_use]
    pub const fn object(&self) -> ObjectId {
        self.object
    }
}

/// Why the adapter believes a durable boundary was reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BoundaryReason {
    /// The last handle on the object was released.
    Closed,
    /// A write was made durable.
    Synced,
    /// A temporary file was renamed over its target — the save pattern most editors use.
    RenamedIntoPlace,
    /// Metadata stopped changing.
    MetadataSettled,
}

impl BoundaryReason {
    /// The published name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Closed => "Closed",
            Self::Synced => "Synced",
            Self::RenamedIntoPlace => "RenamedIntoPlace",
            Self::MetadataSettled => "MetadataSettled",
        }
    }
}

/// A point in one view's stream that the adapter offers as durable.
///
/// A *candidate*, not a decision: what to do with it is the daemon's, and an adapter that treated
/// it as a decision would be deciding a protocol question from a platform callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CheckpointCandidate {
    view: ViewId,
    through: EventSequence,
    reason: BoundaryReason,
}

impl CheckpointCandidate {
    /// The candidate an adapter is offering.
    #[must_use]
    pub const fn new(view: ViewId, through: EventSequence, reason: BoundaryReason) -> Self {
        Self {
            view,
            through,
            reason,
        }
    }

    /// The view it belongs to.
    #[must_use]
    pub const fn view(&self) -> ViewId {
        self.view
    }

    /// The position it covers that view's stream through, inclusive.
    #[must_use]
    pub const fn through(&self) -> EventSequence {
        self.through
    }

    /// Why the adapter thinks so.
    #[must_use]
    pub const fn reason(&self) -> BoundaryReason {
        self.reason
    }
}

// ---------------------------------------------------------------------------------------------
// The two traits
// ---------------------------------------------------------------------------------------------

/// A filesystem backend: the thing that hands out views.
///
/// See this module's header for why it is synchronous, why every method takes `&self`, and what a
/// backend has to arrange so [`WorkspaceAdapter::view`] can return a borrow.
pub trait WorkspaceAdapter: Send + Sync {
    /// What this adapter is and what it claims to do.
    ///
    /// Called before anything else, and the answer is held to for the rest of the run: a
    /// declaration that changes between calls makes every subsequent grade meaningless.
    fn describe(&self) -> AdapterDescription;

    /// Name one workspace this adapter will mount and one head it will present, creating them if
    /// that is what it takes.
    ///
    /// **Called once, before anything else, and every later call is made with what it answered.**
    /// A caller that has not been handed a workspace has no other way to obtain one: neither trait
    /// publishes an operation that creates a workspace or enumerates the heads a backend holds, so
    /// without this the only thing left is to invent an identifier and be refused. Answering here
    /// is what makes a backend's mounting, releasing and read-only refusal checkable at all.
    ///
    /// Answer identifiers you will really accept. A backend that answers one workspace and then
    /// refuses it is graded on the refusal, exactly as a capability it declared and then refused
    /// is.
    ///
    /// # Errors
    ///
    /// Whichever refusal the platform reached. Refusing is legal and is not a hidden pass: a
    /// backend that declares [`AdapterCapability::MountActorView`] or
    /// [`AdapterCapability::MaterializeReadonlyView`] and then will not name a workspace or a head
    /// has declared something nothing can check, and is graded as having failed it rather than as
    /// not having it.
    fn prepare_fixture(&self) -> Result<AdapterFixture, AdapterError>;

    /// Present one actor's own state at `mountpoint`, read-write.
    ///
    /// Two actors mount at once. Nothing in this signature serialises them, and nothing in a
    /// backend should.
    ///
    /// `mountpoint` is a presentation request, not a promise that a host-global path already
    /// exists. A backend which owns a writable root may resolve a relative request below that root
    /// instead of interpreting it relative to the process working directory. In that case the
    /// returned [`MountedView::mountpoint`] reports the path actually used. An absolute request is
    /// literal; a backend which cannot serve it refuses it rather than silently rebasing it.
    ///
    /// # Errors
    ///
    /// [`AdapterError::Unsupported`] if [`AdapterCapability::MountActorView`] was not declared;
    /// otherwise whichever refusal the platform reached.
    fn mount_actor_view(
        &self,
        workspace: WorkspaceId,
        actor: ActorId,
        mountpoint: &Path,
    ) -> Result<MountedView, AdapterError>;

    /// Present the exact state named by `head` at `target`, read-only.
    ///
    /// The same path rule as [`WorkspaceAdapter::mount_actor_view`] applies: a backend-owned root
    /// may resolve a relative request, [`MaterializedView::target`] reports the actual path, and an
    /// absolute request is either served literally or refused.
    ///
    /// # Errors
    ///
    /// [`AdapterError::Unsupported`] if [`AdapterCapability::MaterializeReadonlyView`] was not
    /// declared; otherwise whichever refusal the platform reached.
    fn materialize_readonly_view(
        &self,
        head: HeadId,
        target: &Path,
    ) -> Result<MaterializedView, AdapterError>;

    /// Decide whether `event` completes a durable boundary.
    ///
    /// **A pure function of the event stream the adapter has been shown.** Replaying an identical
    /// stream produces an identical sequence of candidates; one release of the last handle offers
    /// at most one candidate. An adapter that consulted anything else — a clock, a file, the size
    /// of a queue — would make the daemon's boundaries a function of when it ran.
    ///
    /// # Errors
    ///
    /// [`AdapterError::Unsupported`] if [`AdapterCapability::ObserveDurableBoundary`] was not
    /// declared; [`AdapterError::UnknownView`] if the event names a view this adapter did not
    /// issue.
    fn observe_durable_boundary(
        &self,
        event: &FsEvent,
    ) -> Result<Option<CheckpointCandidate>, AdapterError>;

    /// Observe a contract-1 durable boundary, carrying rename evidence separately from the event.
    ///
    /// The default preserves contract-0 backends as honest partial implementations: close and
    /// sync candidates migrate, while a rename is an explicit unsupported result. A backend may
    /// claim contract 1 only after overriding this method with its real producers.
    ///
    /// # Errors
    ///
    /// As [`WorkspaceAdapter::observe_durable_boundary`].
    fn observe_durable_boundary_v1(
        &self,
        event: &FsEvent,
        _rename: Option<&RenameEvidence>,
    ) -> Result<BoundaryObservationV1, AdapterError> {
        if event.kind() == FsEventKind::Renamed {
            return Ok(BoundaryObservationV1::Unsupported(
                RenameEvidenceUnavailable::all(),
            ));
        }
        match self.observe_durable_boundary(event)? {
            Some(candidate) if candidate.reason() == BoundaryReason::Closed => Ok(
                BoundaryObservationV1::Candidate(CheckpointCandidateV1::new(
                    candidate.view(),
                    candidate.through(),
                    BoundaryReasonV1::Closed,
                )),
            ),
            Some(candidate) if candidate.reason() == BoundaryReason::Synced => Ok(
                BoundaryObservationV1::Candidate(CheckpointCandidateV1::new(
                    candidate.view(),
                    candidate.through(),
                    BoundaryReasonV1::Synced,
                )),
            ),
            Some(candidate) if candidate.reason() == BoundaryReason::RenamedIntoPlace => Ok(
                BoundaryObservationV1::Unsupported(RenameEvidenceUnavailable::all()),
            ),
            Some(candidate) => Err(AdapterError::Backend(format!(
                "removed-boundary-reason: {}",
                candidate.reason().as_str()
            ))),
            None => Ok(BoundaryObservationV1::None),
        }
    }

    /// Resolve a view identifier to the filesystem it names.
    ///
    /// # Errors
    ///
    /// [`AdapterError::UnknownView`] if the identifier was never issued here or has been released.
    /// A released identifier never resolves to a stale view.
    fn view(&self, view: ViewId) -> Result<&dyn WorkspaceView, AdapterError>;

    /// Give up a view.
    ///
    /// Unregisters the identifier. Whether the memory behind the view is reclaimed is the
    /// backend's business; what the contract requires is that the identifier stops resolving.
    ///
    /// # Errors
    ///
    /// [`AdapterError::UnknownView`] if the identifier was never issued here, or has already been
    /// released — releasing twice is an error, not a silent success, because a caller that
    /// double-releases has lost track of something.
    fn release(&self, view: ViewId) -> Result<(), AdapterError>;
}

/// The filesystem itself: the operations a tool performs, and nothing else.
///
/// Every method takes `&self` for the reason [`WorkspaceAdapter`] does. Every mutating method on a
/// view whose [`WorkspaceView::access`] is [`ViewAccess::ReadOnly`] answers
/// [`AdapterError::ReadOnly`], including [`WorkspaceView::set_metadata`].
pub trait WorkspaceView: Send + Sync {
    /// This view's identifier, the one [`WorkspaceAdapter::view`] resolved.
    fn id(&self) -> ViewId;

    /// Whether this view accepts mutation.
    fn access(&self) -> ViewAccess;

    /// The object at the top of this view.
    fn root(&self) -> ObjectId;

    /// Resolve `name` inside `parent`.
    ///
    /// # Errors
    ///
    /// [`AdapterError::NotFound`] if nothing is bound; [`AdapterError::NotADirectory`] if `parent`
    /// is a file; [`AdapterError::NameRejected`] if the name breaks a portable-name rule.
    fn lookup(&self, parent: ObjectId, name: &NormalizedName) -> Result<ViewEntry, AdapterError>;

    /// Every entry in `directory`, in **ascending byte-lexicographic order of each entry name's
    /// UTF-8 bytes** — not locale order, and not code-point order after normalization. Repeated
    /// calls on an unchanged directory agree.
    ///
    /// # Errors
    ///
    /// [`AdapterError::NotFound`] if there is no such object; [`AdapterError::NotADirectory`] if
    /// it is a file.
    fn enumerate(&self, directory: ObjectId) -> Result<Vec<ViewEntry>, AdapterError>;

    /// Take a handle on `object`.
    ///
    /// # Errors
    ///
    /// [`AdapterError::NotFound`] if there is no such object; [`AdapterError::IsADirectory`] if it
    /// is a directory; [`AdapterError::ReadOnly`] if `mode` writes and this view does not.
    fn open(&self, object: ObjectId, mode: OpenMode) -> Result<OpenHandle, AdapterError>;

    /// Release a handle. Takes it by value, so it cannot be used again by accident.
    ///
    /// # Errors
    ///
    /// [`AdapterError::NotFound`] if the handle was already released, or was never this view's.
    fn close(&self, handle: OpenHandle) -> Result<(), AdapterError>;

    /// Fill `into` from `offset`, and answer **how many bytes were filled**.
    ///
    /// A short read is legal and is reported. Answering `into.len()` while filling fewer is the
    /// defect this shape exists to make visible.
    ///
    /// # Errors
    ///
    /// [`AdapterError::NotFound`] if the handle is not this view's; [`AdapterError::Unsupported`]
    /// if [`AdapterCapability::Read`] was not declared.
    fn read(
        &self,
        handle: &OpenHandle,
        offset: u64,
        into: &mut [u8],
    ) -> Result<usize, AdapterError>;

    /// Take bytes from `from` at `offset`, and answer **how many were taken**.
    ///
    /// A short write is legal and is reported. Answering `from.len()` while storing fewer is data
    /// loss, and it is the single defect this signature was chosen to expose.
    ///
    /// # Errors
    ///
    /// [`AdapterError::ReadOnly`] on a read-only view; [`AdapterError::NotFound`] if the handle is
    /// not this view's; [`AdapterError::Unsupported`] if [`AdapterCapability::Write`] was not
    /// declared.
    fn write(&self, handle: &OpenHandle, offset: u64, from: &[u8]) -> Result<usize, AdapterError>;

    /// Set `object` to exactly `length` bytes.
    ///
    /// Shrinking removes every byte at and after `length`. Growing preserves the existing prefix
    /// and fills every new byte with zero. Setting zero leaves an empty file. This is separate from
    /// [`WorkspaceView::write`]: a write replaces a range and cannot honestly imply that the file
    /// ends after that range.
    ///
    /// # Errors
    ///
    /// [`AdapterError::ReadOnly`] on a read-only view; [`AdapterError::NotFound`] if there is no
    /// such object; [`AdapterError::IsADirectory`] if `object` is a directory;
    /// [`AdapterError::Unsupported`] if [`AdapterCapability::SetFileLength`] was not declared.
    fn set_file_length(&self, _object: ObjectId, _length: u64) -> Result<(), AdapterError> {
        Err(AdapterError::unsupported(AdapterCapability::SetFileLength))
    }

    /// Bind a new file under `name` in `parent`.
    ///
    /// # Errors
    ///
    /// [`AdapterError::AlreadyExists`] if the name is taken; [`AdapterError::NameRejected`] if it
    /// breaks a portable-name rule; [`AdapterError::ReadOnly`] on a read-only view.
    fn create_file(
        &self,
        parent: ObjectId,
        name: &NormalizedName,
        metadata: PortableMetadata,
    ) -> Result<ViewEntry, AdapterError>;

    /// Bind a new directory under `name` in `parent`.
    ///
    /// # Errors
    ///
    /// As [`WorkspaceView::create_file`].
    fn create_directory(
        &self,
        parent: ObjectId,
        name: &NormalizedName,
    ) -> Result<ViewEntry, AdapterError>;

    /// Change an entry's name **within one directory**. Portable metadata survives unchanged.
    ///
    /// # Errors
    ///
    /// [`AdapterError::NotFound`] if `from` is not bound; [`AdapterError::AlreadyExists`] if `to`
    /// is; [`AdapterError::NameRejected`]; [`AdapterError::ReadOnly`].
    fn rename(
        &self,
        parent: ObjectId,
        from: &NormalizedName,
        to: &NormalizedName,
    ) -> Result<(), AdapterError>;

    /// Contract-1 rename: choose replacement behavior and return the separate binding evidence.
    ///
    /// Contract-0 implementors refuse this default rather than synthesizing evidence. A backend
    /// claiming [`WORKSPACE_ADAPTER_CONTRACT_V1`] must override it.
    fn rename_with_evidence(
        &self,
        _sequence: EventSequence,
        _parent: ObjectId,
        _from: &NormalizedName,
        _to: &NormalizedName,
        _disposition: RenameDisposition,
    ) -> Result<RenameBindingEvidence, AdapterError> {
        Err(AdapterError::unsupported(AdapterCapability::Rename))
    }

    /// Move an entry **between two directories**. Portable metadata survives unchanged.
    ///
    /// A separate operation from [`WorkspaceView::rename`] because Mesh models the two separately,
    /// and one call would have to guess which was meant when both parents are equal.
    ///
    /// # Errors
    ///
    /// [`AdapterError::WouldCycle`] if the move would place a directory inside its own subtree;
    /// otherwise as [`WorkspaceView::rename`].
    fn move_entry(
        &self,
        from_parent: ObjectId,
        from: &NormalizedName,
        to_parent: ObjectId,
        to: &NormalizedName,
    ) -> Result<(), AdapterError>;

    /// Contract-1 move: choose replacement behavior and return the separate binding evidence.
    ///
    /// Contract-0 implementors refuse this default rather than guessing an intermediate binding.
    fn move_entry_with_evidence(
        &self,
        _sequence: EventSequence,
        _from_parent: ObjectId,
        _from: &NormalizedName,
        _to_parent: ObjectId,
        _to: &NormalizedName,
        _disposition: RenameDisposition,
    ) -> Result<RenameBindingEvidence, AdapterError> {
        Err(AdapterError::unsupported(AdapterCapability::Move))
    }

    /// Remove a file's name binding.
    ///
    /// # Errors
    ///
    /// [`AdapterError::NotFound`] if nothing is bound; [`AdapterError::IsADirectory`] if the name
    /// is bound to a directory; [`AdapterError::ReadOnly`].
    fn unlink(&self, parent: ObjectId, name: &NormalizedName) -> Result<(), AdapterError>;

    /// Remove an **empty** directory's name binding. Never a recursive delete.
    ///
    /// # Errors
    ///
    /// [`AdapterError::DirectoryNotEmpty`] if it still has entries;
    /// [`AdapterError::NotADirectory`] if the name is bound to a file; [`AdapterError::NotFound`];
    /// [`AdapterError::ReadOnly`].
    fn remove_directory(&self, parent: ObjectId, name: &NormalizedName)
        -> Result<(), AdapterError>;

    /// An object's portable metadata.
    ///
    /// # Errors
    ///
    /// [`AdapterError::NotFound`] if there is no such object.
    fn metadata(&self, object: ObjectId) -> Result<PortableMetadata, AdapterError>;

    /// Set an object's portable metadata.
    ///
    /// # Errors
    ///
    /// [`AdapterError::ReadOnly`] on a read-only view; [`AdapterError::NotFound`].
    fn set_metadata(
        &self,
        object: ObjectId,
        metadata: PortableMetadata,
    ) -> Result<(), AdapterError>;
}

/// Dyn-compatibility, as a build failure rather than a test failure.
///
/// `mesh-daemon` selects a backend at run time and holds it as `&dyn WorkspaceAdapter`. A method
/// that broke that — a generic parameter, a `where Self: Sized`, a native `async fn` — would fail
/// here, at `cargo build`, rather than in whichever crate first tried to store one.
const _: fn(&dyn WorkspaceAdapter) = |_| ();
const _: fn(&dyn WorkspaceView) = |_| ();
