# `mesh-workspace-adapter/0`

**Frozen compatibility contract.** New implementations opt into
`mesh-workspace-adapter/1`; contract 0 remains readable and is not silently
reinterpreted.

The contract a Mesh filesystem backend implements, written for someone who is going to write one
and is not going to read Mesh's internals.

If you can implement everything on this page and pass the conformance suite, you have a backend.
Nothing in `crates/mesh-materializer/src/apply.rs` — or anywhere else in Mesh — is required reading,
and the suite carries a lint that fails if that stops being true (design
`01KZEZGDPMZ5RH7E60WDYDYYEE` Contract 9).

**Status.** This directory is the *published* half of the contract, written by design
`01KZEZGDPMZ5RH7E60WDYDYYEE` under task `01KZC2M9305RFTMVSHJPG7HJGT`.

| Half | Where | State |
|---|---|---|
| The trait, the capability declaration, the error set, the boundary types | `crates/mesh-materializer/src/adapter.rs`, re-exported from the crate root | **landed.** You can write a backend against it today. |
| The conformance suite | `crates/mesh-materializer/src/conformance.rs` and `crates/mesh-materializer/tests/adapter-conformance.rs` | **landed.** Written by a different run than the trait, per plan §14.3 rule 4. |

`vocabulary.json` is now held against the Rust **in both directions** by the suite, so a capability,
error, event kind or boundary reason added to one and not the other turns it red. This page is
therefore the contract rather than a description of one.

**What the suite has been observed doing, rather than claimed to do.** Every case in it has been
seen to fail: sixteen deliberate backend defects are planted one at a time
(`crates/mesh-materializer/tests/adapter-conformance/reference.rs`), and each is required to fail
**exactly one** case — the one that names its rule — and nothing else. On the reference backend,
which implements all seventeen operational capabilities, the run is 131 cases: 74 pass, 0 fail, 57
`unsupported`. On a backend that declares nothing it is 15 pass, 0 fail, 116 `unsupported`, and it
is conformant.

The sixteen are planted twice: once on a backend that accepts any identifier it is handed, and
once on a backend that **holds one workspace and one head of its own and answers `NotFound` for
every other** — which is what a real backend does. Both runs are 131 cases, 74 pass, 0 fail, 57
`unsupported`, and on both, each defect fails exactly its own case. That second backend is why §2a
exists; before it the `MNT` and `RO` families graded nothing against anything but an in-memory
backend.

What you can do today, and what `crates/mesh-materializer/tests/adapter_shape.rs` does: implement
`WorkspaceAdapter` and `WorkspaceView` against the published names alone, hand your adapter out as
`&dyn WorkspaceAdapter`, and declare `CapabilitySet::EMPTY` while every operation refuses with
`Unsupported`. That is a legal backend, and it is the honest starting point for one.

---

## 1. The two surfaces

An **adapter** hands out views. A **view** is the filesystem.

```rust
pub const WORKSPACE_ADAPTER_CONTRACT: &str = "mesh-workspace-adapter/0";

pub trait WorkspaceAdapter: Send + Sync {
    fn describe(&self) -> AdapterDescription;
    fn prepare_fixture(&self) -> Result<AdapterFixture, AdapterError>;   // §2a
    fn mount_actor_view(&self, workspace: WorkspaceId, actor: ActorId, mountpoint: &Path)
        -> Result<MountedView, AdapterError>;
    fn materialize_readonly_view(&self, head: HeadId, target: &Path)
        -> Result<MaterializedView, AdapterError>;
    fn observe_durable_boundary(&self, event: &FsEvent)
        -> Result<Option<CheckpointCandidate>, AdapterError>;
    fn view(&self, view: ViewId) -> Result<&dyn WorkspaceView, AdapterError>;
    fn release(&self, view: ViewId) -> Result<(), AdapterError>;
}
```

Three things about that signature are decisions rather than accidents, and a backend depends on all
three:

- **It is not `async`.** The host crate may declare no dependency, so there is no `async-trait`; and
  the trait must stay dyn-compatible because the daemon picks a backend at run time. If your backend
  wants an executor, own one inside your crate and block at this boundary. `fuser` already hands you
  a synchronous per-request callback, and FSKit's Swift half is callback-based across a C ABI that
  cannot carry a Rust future.
- **Every method takes `&self`.** Not `&mut self`. Two actor views must be mountable at once — the
  epic's exit criterion is "no global actor-workspace lock", and `&mut self` would put one in the
  type system for every backend simultaneously. Use interior mutability.
- **The error type is closed.** §4. A closed set is what lets the suite assert *which* refusal
  happened, which is the whole of "declares what it cannot do, explicitly".

```rust
pub trait WorkspaceView: Send + Sync {
    fn id(&self) -> ViewId;
    fn access(&self) -> ViewAccess;                       // ReadWrite | ReadOnly
    fn root(&self) -> ObjectId;

    fn lookup(&self, parent: ObjectId, name: &NormalizedName) -> Result<ViewEntry, AdapterError>;
    fn enumerate(&self, directory: ObjectId) -> Result<Vec<ViewEntry>, AdapterError>;
    fn open(&self, object: ObjectId, mode: OpenMode) -> Result<OpenHandle, AdapterError>;
    fn close(&self, handle: OpenHandle) -> Result<(), AdapterError>;

    fn read(&self, handle: &OpenHandle, offset: u64, into: &mut [u8]) -> Result<usize, AdapterError>;
    fn write(&self, handle: &OpenHandle, offset: u64, from: &[u8]) -> Result<usize, AdapterError>;
    fn set_file_length(&self, object: ObjectId, length: u64) -> Result<(), AdapterError>;

    fn create_file(&self, parent: ObjectId, name: &NormalizedName, metadata: PortableMetadata)
        -> Result<ViewEntry, AdapterError>;
    fn create_directory(&self, parent: ObjectId, name: &NormalizedName)
        -> Result<ViewEntry, AdapterError>;

    fn rename(&self, parent: ObjectId, from: &NormalizedName, to: &NormalizedName)
        -> Result<(), AdapterError>;
    fn move_entry(&self, from_parent: ObjectId, from: &NormalizedName,
                  to_parent: ObjectId, to: &NormalizedName) -> Result<(), AdapterError>;

    fn unlink(&self, parent: ObjectId, name: &NormalizedName) -> Result<(), AdapterError>;
    fn remove_directory(&self, parent: ObjectId, name: &NormalizedName) -> Result<(), AdapterError>;

    fn metadata(&self, object: ObjectId) -> Result<PortableMetadata, AdapterError>;
    fn set_metadata(&self, object: ObjectId, metadata: PortableMetadata) -> Result<(), AdapterError>;
}
```

**`read` and `write` borrow a caller-supplied buffer and return a count.** They never return or take
a `Vec<u8>`. That is `pread`/`pwrite`, which is the shape both FUSE and FSKit hand you already, so
neither platform half has to allocate; and it makes a **short transfer observable**. Returning fewer
bytes than the buffer holds is legal and is checked. Returning `from.len()` while storing fewer is
the defect the suite exists to catch.

**`set_file_length` gives a file an exact end.** Shrinking removes the old tail, growing preserves
the prefix and fills the new range with zero bytes, and zero leaves an empty file. A directory is
`IsADirectory`; an absent object is `NotFound`. This is a separate operation because a range write
cannot honestly imply that the file ends after the range it replaced.

**`rename` and `move_entry` are two operations.** `rename` changes a name inside one directory;
`move_entry` moves an entry between two. They are separate because Mesh models them separately, and
a single call would have to guess which was meant when both parents are equal.

---

## 2. Declaring what you can do

```rust
pub struct AdapterDescription {
    adapter: &'static str,      // your name, e.g. "mesh-fuse/0"
    contract: &'static str,     // must equal WORKSPACE_ADAPTER_CONTRACT
    capabilities: CapabilitySet,
}
```

The eighteen capabilities are in [`vocabulary.json`](vocabulary.json). Declare exactly what you
implement.

**A capability you did not declare is still called, once, and the answer is graded.** This is the
part of the contract most likely to surprise you, and it is deliberate:

| You declared it | The suite does | A wrong answer is |
|---|---|---|
| yes | runs every case for that capability | `fail` |
| yes, and you answer `Unsupported` | — | `fail` |
| no | calls the operation once and requires exactly `Err(AdapterError::Unsupported { capability })` | `fail` |
| no, and you returned `Ok` | — | `fail` |
| no, and you panicked | — | `fail` |
| no, and you refused cleanly | grades that capability's remaining cases `unsupported` | not a failure |

Never asking would be cheaper and would be wrong. It cannot tell a backend that has not built
`write` from one whose `write` returns `Ok(n)` and drops the bytes — and on a filesystem the second
one is data loss. Declaring nothing and refusing cleanly is always a legitimate, passing answer.
Declaring something you cannot honour never helps: you are graded on the answer, not the claim.

---

## 2a. Naming a workspace and a head — the one thing the suite cannot invent

```rust
pub struct AdapterFixture { workspace: WorkspaceId, head: HeadId }

fn prepare_fixture(&self) -> Result<AdapterFixture, AdapterError>;
```

**Called once, before anything else, and everything afterwards is mounted and presented with what
you answered.** Create a workspace and a head if that is what it takes; name ones you already hold
if it is not. If your backend holds no state of its own, answer any pair — nothing will be mounted
with it.

It is here because nothing else in §1 can produce a workspace or name a head. Without it the suite
has one move left, which is to invent an identifier — and your backend will answer `NotFound` for a
workspace it was never given, because that is the correct thing for it to do. Everything downstream
of a mount then goes ungraded, and the report says `unsupported`, which is the same word an honest
partial backend earns. **A backend could be reported conformant with nothing about mounting,
releasing or read-only refusal checked at all.** That is what this method closes.

| What you answer | What happens |
|---|---|
| `Ok(fixture)` | every mount and every read-only presentation below uses it |
| `Ok` with a workspace you then refuse | you are graded on the refusal — `MNT` fails, exactly as a capability you declared and then refused does |
| `Err(_)`, and you declared `MountActorView` or `MaterializeReadonlyView` | those families are **`fail`**, never `unsupported`: you declared something nothing can check |
| `Err(_)`, and you declared neither | nothing changes; those families were already `unsupported` because you did not declare them |

Answering `Ok` costs a backend that holds nothing one line. Refusing is legal, is not a hidden pass,
and the report prints the refusal.

---

## 3. The three results

| Result | Meaning | Affects the verdict |
|---|---|---|
| `pass` | you answered, and the answer matches the rule | — |
| `fail` | you answered and broke a rule, or panicked, or refused a capability you declared | **yes** |
| `unsupported` | the capability was not declared and was refused cleanly | no |

`unsupported` is not a soft `fail` and it is not a `pass`. A partial backend reporting honestly is
not a broken one. These are the same three words `protocol/conformance` grades a CWP client with,
reused on purpose so a reader of one report can read the other.

---

## 4. The error set — closed, and with no errno

```rust
pub enum AdapterError {
    Unsupported { capability: AdapterCapability },
    NotFound,
    AlreadyExists,
    NotADirectory,
    IsADirectory,
    DirectoryNotEmpty,
    NameRejected(NameError),
    OutsideWorkspace,
    ReadOnly,
    UnknownView,
    WouldCycle,
    Backend(String),
}
```

When each one is right is in [`vocabulary.json`](vocabulary.json) `errors`.

`NameRejected` wraps Mesh's existing name rule rather than restating it. A directory entry name is
non-empty, is neither `.` nor `..`, contains no `/` or `\`, and contains no NUL byte. **No Unicode
normalization form is applied**, so two canonically equivalent names are two names — see §6.

**Return it from the two path arguments, and from nowhere else on the view.** `mount_actor_view`'s
mountpoint and `materialize_readonly_view`'s target are caller-supplied bytes you must judge; every
`WorkspaceView` method takes a `&NormalizedName` instead, which cannot be constructed from a name
the rules refuse, so no view method can ever be handed one. Writing a `NameRejected` branch inside
`lookup` or `create_file` is a branch you will never reach — the type has already refused. Ruled
under `01KZFXF9N49NHJ7XS3MD0MX3BR`; §6 and `vocabulary.json` say the same thing.

`Backend(String)` is never the right answer to a conformance case. A case answered with `Backend` is
a failure, not an excuse.

### Presentation paths are requests, not host-global fixtures

The `mountpoint` and read-only `target` arguments say where the caller wants a view presented; they
do not promise that a host-global directory already exists or that the process may create one. A
backend that owns a writable root may resolve a **relative** request below that root. It then
returns the path it actually used from `MountedView::mountpoint()` or
`MaterializedView::target()`. An **absolute** request stays literal: serve it there or return the
typed refusal the platform reached, never silently rebase it.

The conformance suite therefore uses distinct relative requests under `mesh-conformance/`. It is
grading coexistence, release and read-only behaviour, not permission to create `/mesh`. A backend
that refuses even those portable requests is still graded `fail` on `MNT` and `RO`; it is not
reported `unsupported`. Validate every component, including a refused `..`, before resolving a
request below an owned root.

## 5. Errno, which is yours and not the contract's

There is no `errno()` on `AdapterError`, because there cannot be one that is correct: `ENOTEMPTY` is
39 on Linux and 66 on Darwin, `ENOSYS` is 38 and 78. One integer in a portable crate is wrong on one
of the two platforms Mesh ships, invisibly, and a Linux-only CI would never see it.

Project it in your own crate, and test it there. The recommended mapping — **guidance, which the
FUSE and FSKit lanes own and may correct**:

| `AdapterError` | POSIX name |
|---|---|
| `Unsupported` | `ENOSYS` |
| `NotFound` | `ENOENT` |
| `AlreadyExists` | `EEXIST` |
| `NotADirectory` | `ENOTDIR` |
| `IsADirectory` | `EISDIR` |
| `DirectoryNotEmpty` | `ENOTEMPTY` |
| `NameRejected` | `EINVAL` |
| `OutsideWorkspace` | `EPERM` |
| `ReadOnly` | `EROFS` |
| `UnknownView` | `EBADF` |
| `WouldCycle` | `EINVAL` |
| `Backend` | `EIO` |

Use your platform's constants. Do not copy the numbers.

---

## 6. Rules a case will hold you to

- **`enumerate` is byte-lexicographic.** Entries come back in ascending order of the entry name's
  UTF-8 bytes — not locale order, not code-point order after normalization. Repeated calls on an
  unchanged directory agree. This is the same ordering rule
  `protocol/test-vectors/README.md` states for a keyed sequence.
- **A read-only view refuses every mutating operation with `ReadOnly`**, including
  `set_file_length` and `set_metadata`. A `MaterializedView` is read-only by construction.
- **`move_entry` refuses a cycle** with `WouldCycle`: a directory may not be moved inside its own
  subtree.
- **`remove_directory` on a non-empty directory is `DirectoryNotEmpty`**, never a recursive delete.
- **Metadata survives `rename` and `move_entry`.** The portable metadata set is one bit today (the
  executable bit) and widening it is a compatibility event, not a field addition.
- **A path component the name rules refuse is refused**, with `NameRejected` carrying the rule it
  broke. A mountpoint or read-only target holding a `..` is `NameRejected(Relative)`.

  This is the only surface where the name rules are checkable, and that is the ruling rather than a
  gap. `NormalizedName` cannot be constructed from a refused name at all, so **no `WorkspaceView`
  method can ever be handed one** and `NameRejected` is unreachable through every method on the
  view. **On the view the type is the enforcement**, and that is stronger than a case: a rule the
  compiler makes unrepresentable cannot be got wrong by one backend on one platform, and a second
  surface that accepted raw bytes would make every backend re-implement the four rules — which is
  exactly what `NameRejected(NameError)` wrapping the existing rule exists to prevent. The path
  arguments of `mount_actor_view` and `materialize_readonly_view` are where a caller really does
  hand you bytes to judge, so that is where the `NAME` family grades. Ruled under
  `01KZFXF9N49NHJ7XS3MD0MX3BR`; §4 and `vocabulary.json` say the same thing.
- **A released `ViewId` resolves to `UnknownView`**, not to a stale view.
- **Two actor views coexist.** `mount_actor_view` for two different actors, both usable afterwards.
- **Nothing carries a clock.** `FsEvent` has no timestamp field. Order is
  `lamport → event_ulid → content-hash`, never wall-clock, and the sequence number you mint is
  per-view and monotone. It is not comparable across views; correlating two views is the daemon's
  job and this contract gives you no way to pretend otherwise.
- **`observe_durable_boundary` is a pure function of the event stream you have been shown.**
  Replaying one stream produces the identical candidate sequence, and one `Closed` event produces
  at most one candidate.

---

## 7. Running the suite

```rust
#[test]
fn my_backend_is_a_workspace_adapter() {
    let report = mesh_materializer::run_conformance(&MyAdapter::new());
    assert!(report.is_conformant(), "{report}");
}
```

That is the entire integration. **Adding a backend changes no file in the suite** — the suite takes
`&dyn WorkspaceAdapter` and cannot name a concrete one, and a test in the suite asserts that its own
source text names no concrete backend. A failure prints the case, the rule, where the rule is
written down (a path into this directory), and the difference.

Three ordering rules the suite depends on, so that one defect fails one case:

- **`prepare_fixture` is called before anything else**, once, and every mount and every read-only
  presentation afterwards uses what it answered. A backend that expects to be handed a workspace
  before it is asked to name one has the order backwards.
- **The capability check comes first, everywhere.** An undeclared capability answers `Unsupported`
  even where some other refusal (`ReadOnly`, `NotFound`, `IsADirectory`) would also have been true.
  It is the only answer an undeclared capability may give, so it cannot be second in line.
- **Every capability is asked its one question before any case runs.** A capability you declared and
  then refused is recorded as unusable *before* a case that only needed it to observe something else
  tries to use it, so `CAP/declared-then-refused` is the single failure rather than the first of
  many.

---

## 8. What this suite does not establish

- **It is in-memory.** No mount, no kernel, no temporary directory. The failure modes FUSE has and
  memory does not — concurrent opens, a partial write interleaved with a rename, a kernel retry —
  are not reachable from here and belong to each backend's own tests.
- **It is a Rust test, not a process boundary.** Unlike `protocol/conformance`, which spawns a
  client and speaks JSON to it, this suite links to your code. It therefore tests a build rather
  than an implementation-across-a-boundary, and a non-Rust backend cannot run it. `mesh-fskit-ffi`'s
  Swift half is the first thing that will feel this, and its C-ABI seam is where the question
  belongs.
- **It cannot decide Unicode normalization**, because Mesh has not pinned a form. Two backends may
  legitimately differ on whether a composed and a decomposed name are one entry or two. The suite
  does not ask rather than guessing.
- **Nothing here is a performance claim.** The epic's "actor mount under 500 ms" is a benchmark and
  belongs with a number, per plan §2.10.
- **It grades one workspace and one head, the ones you named.** §2a is how it gets them, and that
  closes the gap where two whole families applied only to a backend that accepted an arbitrary
  identifier. What it still does not do is grade *several*: one workspace, one head, three actor
  views. Two workspaces interfering with each other is not reachable from here.
- **`OutsideWorkspace` has no case.** Nothing in §1 can reach it: no method on the view takes a
  path, so no method can resolve outside the workspace. It stays in the error set because a
  backend's own internals will need it. Unlike `NameRejected`, which §4 and §6 place on the two path
  arguments, this one is produced by no published operation at all.

## 9. Reporting a defect

A case you believe is wrong is either a contract bug or a suite bug, and both are worth more than a
workaround. File it as a `kind: bug` task. **A conformance case that a backend fails is either a
contract bug or a backend bug: decide which through protocol review, never by removing the case.**
