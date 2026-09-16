#![allow(dead_code)]
//! Shared test fixtures: a scratch directory, an observable filesystem, and reproducible bytes.
//!
//! # The recording filesystem is evidence, not a mock
//!
//! [`RecordingFs`] delegates every operation to the real one and writes down what it was asked to
//! do. Nothing is simulated, so a test that asserts on the recorded order is asserting about the
//! same syscalls production issues, not about a model of them. [`FailingFs`] adds one thing: it
//! refuses the *n*-th operation, which is how a test reaches an error path a real disk would
//! reach only when it is full or failing. [`RecordingFs::refusing`] is the same instrument aimed
//! by name rather than by ordinal, for the test that means one *particular* operation — the
//! interruption point a durability window is defined by, which an ordinal renames every time the
//! sequence gains a step.
//!
//! What neither can do is prove the platform honours what it was asked. That limit is stated in
//! `DURABILITY.md` and is why the crash tests kill real processes rather than relying on these.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

use mesh_cas::{DurableFs, StdFs};

/// A directory under the system temporary directory that removes itself when dropped.
///
/// Names are unique per process and per construction, so tests running in parallel — and the child
/// processes the crash harness spawns — never share one.
#[derive(Debug)]
pub struct TempRoot {
    path: PathBuf,
}

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

impl TempRoot {
    /// A fresh empty directory, named after `label` so a leftover is traceable to its test.
    pub fn new(label: &str) -> Self {
        let serial = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("mesh-cas-{label}-{}-{serial}", std::process::id()));
        std::fs::create_dir_all(&path).expect("the system temporary directory is writable");
        Self { path }
    }

    /// The directory.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// One filesystem operation, as recorded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Operation {
    /// The [`DurableFs`] method name.
    pub name: &'static str,
    /// The path it was applied to.
    pub path: PathBuf,
}

impl Operation {
    /// Whether this is the named operation applied to the given path.
    pub fn is(&self, name: &str, path: &Path) -> bool {
        self.name == name && self.path == path
    }
}

/// The real filesystem, with every call written down.
///
/// Optionally refusing one chosen call. [`FailingFs`] refuses the *n*-th mutating operation, which
/// is the right instrument for "a failure anywhere leaves no partial chunk" and the wrong one for
/// "a failure *here* leaves this behind": counting to the operation you mean is a number that goes
/// stale the moment the sequence changes. [`RecordingFs::refusing`] names the operation instead,
/// and records what happened either side of it.
#[derive(Debug, Default)]
pub struct RecordingFs {
    inner: StdFs,
    log: Mutex<Vec<Operation>>,
    refuse: Option<Operation>,
}

impl RecordingFs {
    /// A fresh recorder.
    pub fn new() -> Self {
        Self::default()
    }

    /// A recorder that refuses every call of `name` on `path`, and performs everything else.
    ///
    /// The refusal is logged under the name `refused`, so the trace shows where the sequence
    /// stopped and the operation that did not happen is absent from its own name — which is what
    /// lets a test assert that no `sync_dir` of a directory ever took place.
    pub fn refusing(name: &'static str, path: PathBuf) -> Self {
        Self {
            inner: StdFs,
            log: Mutex::new(Vec::new()),
            refuse: Some(Operation { name, path }),
        }
    }

    /// Whether this call is the one being refused. Records the refusal when it is.
    fn refused(&self, name: &'static str, path: &Path) -> bool {
        let refusing = self
            .refuse
            .as_ref()
            .is_some_and(|target| target.is(name, path));
        if refusing {
            self.record("refused", path);
        }
        refusing
    }

    /// The error an injected refusal returns.
    fn refusal() -> io::Error {
        io::Error::other("injected filesystem refusal")
    }

    /// Everything recorded so far, in order.
    pub fn operations(&self) -> Vec<Operation> {
        self.log
            .lock()
            .expect("the log mutex is never poisoned")
            .clone()
    }

    /// The names of everything recorded so far, in order.
    pub fn names(&self) -> Vec<&'static str> {
        self.operations().into_iter().map(|op| op.name).collect()
    }

    /// How many recorded operations have this name.
    pub fn count(&self, name: &str) -> usize {
        self.operations()
            .iter()
            .filter(|op| op.name == name)
            .count()
    }

    /// The index of the first operation matching `name` and `path`, if any.
    pub fn position(&self, name: &str, path: &Path) -> Option<usize> {
        self.operations().iter().position(|op| op.is(name, path))
    }

    /// Every index at which `name` was applied to `path`, in order.
    ///
    /// [`Self::position`] answers "did this happen", which is enough for a pairwise ordering and
    /// not enough for a claim about a *set* of operations being complete: a test that asks only
    /// about the first occurrence cannot tell one sync from three.
    pub fn positions(&self, name: &str, path: &Path) -> Vec<usize> {
        self.operations()
            .iter()
            .enumerate()
            .filter(|(_, op)| op.is(name, path))
            .map(|(index, _)| index)
            .collect()
    }

    /// The paths `name` was applied to, in order, with repeats.
    pub fn paths(&self, name: &str) -> Vec<PathBuf> {
        self.operations()
            .into_iter()
            .filter(|op| op.name == name)
            .map(|op| op.path)
            .collect()
    }

    /// Forget everything recorded, so a test can assert about one phase at a time.
    pub fn clear(&self) {
        self.log
            .lock()
            .expect("the log mutex is never poisoned")
            .clear();
    }

    fn record(&self, name: &'static str, path: &Path) {
        self.log
            .lock()
            .expect("the log mutex is never poisoned")
            .push(Operation {
                name,
                path: path.to_path_buf(),
            });
    }
}

impl DurableFs for RecordingFs {
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        if self.refused("create_dir_all", path) {
            return Err(Self::refusal());
        }
        self.record("create_dir_all", path);
        self.inner.create_dir_all(path)
    }

    fn stage(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        if self.refused("stage", path) {
            return Err(Self::refusal());
        }
        self.record("stage", path);
        self.inner.stage(path, bytes)
    }

    fn append(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        if self.refused("append", path) {
            return Err(Self::refusal());
        }
        self.record("append", path);
        self.inner.append(path, bytes)
    }

    fn sync_file(&self, path: &Path) -> io::Result<()> {
        if self.refused("sync_file", path) {
            return Err(Self::refusal());
        }
        self.record("sync_file", path);
        self.inner.sync_file(path)
    }

    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        if self.refused("sync_dir", path) {
            return Err(Self::refusal());
        }
        self.record("sync_dir", path);
        self.inner.sync_dir(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        if self.refused("rename", from) {
            return Err(Self::refusal());
        }
        self.record("rename", from);
        self.inner.rename(from, to)
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        if self.refused("read", path) {
            return Err(Self::refusal());
        }
        self.record("read", path);
        self.inner.read(path)
    }

    fn exists(&self, path: &Path) -> bool {
        self.record("exists", path);
        self.inner.exists(path)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        if self.refused("remove_file", path) {
            return Err(Self::refusal());
        }
        self.record("remove_file", path);
        self.inner.remove_file(path)
    }

    fn list_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        if self.refused("list_dir", path) {
            return Err(Self::refusal());
        }
        self.record("list_dir", path);
        self.inner.list_dir(path)
    }

    // Overridden rather than left to the trait's default, and the difference is observable: the
    // default reads the whole file, so a recorder that did not override it would log a `read` of
    // every chunk the collector measures and `collection.rs` could not tell a stat from a read.
    fn file_len(&self, path: &Path) -> io::Result<u64> {
        self.record("file_len", path);
        self.inner.file_len(path)
    }
}

/// The real filesystem, refusing one chosen mutating operation.
///
/// Counts only the operations that change the store — reads and existence checks are free — so a
/// test can say "fail the third thing that writes" without counting the bookkeeping around it.
#[derive(Debug)]
pub struct FailingFs {
    inner: StdFs,
    fail_at: usize,
    performed: AtomicUsize,
}

impl FailingFs {
    /// Fail the `fail_at`-th mutating operation, counting from one.
    pub fn new(fail_at: usize) -> Self {
        Self {
            inner: StdFs,
            fail_at,
            performed: AtomicUsize::new(0),
        }
    }

    /// How many mutating operations have been attempted.
    pub fn performed(&self) -> usize {
        self.performed.load(Ordering::SeqCst)
    }

    fn permit(&self) -> io::Result<()> {
        let performed = self.performed.fetch_add(1, Ordering::SeqCst) + 1;
        if performed == self.fail_at {
            return Err(io::Error::other("injected filesystem failure"));
        }
        Ok(())
    }
}

impl DurableFs for FailingFs {
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.permit()?;
        self.inner.create_dir_all(path)
    }

    fn stage(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.permit()?;
        self.inner.stage(path, bytes)
    }

    fn append(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.permit()?;
        self.inner.append(path, bytes)
    }

    fn sync_file(&self, path: &Path) -> io::Result<()> {
        self.permit()?;
        self.inner.sync_file(path)
    }

    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        self.permit()?;
        self.inner.sync_dir(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.permit()?;
        self.inner.rename(from, to)
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.inner.read(path)
    }

    fn exists(&self, path: &Path) -> bool {
        self.inner.exists(path)
    }

    /// Measuring is not mutating, so it is free like `read` and `exists` are.
    fn file_len(&self, path: &Path) -> io::Result<u64> {
        self.inner.file_len(path)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.permit()?;
        self.inner.remove_file(path)
    }

    fn list_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        self.inner.list_dir(path)
    }
}

/// Reproducible pseudorandom bytes.
///
/// A 64-bit xorshift, seeded explicitly, so a property test that fails names a seed a reader can
/// re-run rather than a shape they have to guess at. Not a random number generator for any purpose
/// where randomness matters — nothing here is a key.
#[derive(Clone, Debug)]
pub struct Bytes {
    state: u64,
}

impl Bytes {
    /// A generator with this seed. A zero seed is replaced, since xorshift is stuck at zero.
    pub fn seeded(seed: u64) -> Self {
        Self {
            state: if seed == 0 {
                0x9E37_79B9_7F4A_7C15
            } else {
                seed
            },
        }
    }

    fn next_u64(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }

    /// `length` reproducible bytes.
    pub fn take(&mut self, length: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(length);
        while out.len() < length {
            out.extend_from_slice(&self.next_u64().to_le_bytes());
        }
        out.truncate(length);
        out
    }

    /// A number below `bound`.
    pub fn below(&mut self, bound: u64) -> u64 {
        self.next_u64() % bound.max(1)
    }
}

/// Overwrite one byte of a file on disk, in place, without going through the store.
///
/// This is how a test produces the bit rot the store has to notice: the file keeps its
/// content-addressed name and stops hashing to it.
pub fn flip_byte(path: &Path, offset: usize) {
    let mut bytes = std::fs::read(path).expect("the chunk is readable");
    assert!(offset < bytes.len(), "offset {offset} is inside the chunk");
    bytes[offset] ^= 0x01;
    std::fs::write(path, &bytes).expect("the chunk is writable");
}
