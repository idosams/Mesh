//! Workspace-native agent custody shared by every local Mesh process.
//!
//! The record lives in the private workspace storage root, never in the materialized folder or
//! journal-derived metadata. Its descriptor-pinned lock linearizes agent acquisition/release with
//! every managed mutation, including daemon IPC and local `meshctl` callers.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{File, Permissions};
use std::io::{self, Read as _};
use std::marker::PhantomData;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::rc::Rc;

use mesh_cas::DurableFs as _;

use crate::ipc::Json;
use crate::root_authority::{PinnedRootFs, PinnedWorkspaceRoot};
#[cfg(test)]
use crate::workspace::OpenWorkspace;
use crate::workspace::{workspace_installation, workspace_record_file, workspace_storage_root};

mod enrollment;
pub use enrollment::DependencyEnrollmentFence;

const SCHEMA: &str = "mesh.workspace-agent-custody/v1";
/// Reserved compatibility name from the first custody prototype.
///
/// It is never synchronization authority: the pinned physical workspace directory is flocked so
/// unlinking or replacing a named file cannot split the lock between processes.
pub(crate) const LOCK_FILE: &str = ".workspace-agent-custody.lock";
pub(crate) const RECORD_FILE: &str = ".workspace-agent-custody.json";
pub(crate) const TEMP_FILE: &str = ".workspace-agent-custody.next";
const MAX_RECORD_BYTES: u64 = 1024;

/// A closed snapshot of the workspace-native custody authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceAgentCustody {
    generation: Option<String>,
}

impl WorkspaceAgentCustody {
    /// Current opaque acquisition generation, or `None` when managed mutation is allowed.
    #[must_use]
    pub fn generation(&self) -> Option<&str> {
        self.generation.as_deref()
    }

    /// Whether an agent currently owns the exact managed working directory.
    #[must_use]
    pub const fn is_assigned(&self) -> bool {
        self.generation.is_some()
    }
}

/// A fail-closed custody refusal or private-store I/O failure.
#[derive(Debug)]
pub struct WorkspaceAgentCustodyError {
    detail: String,
    stale_workspace: bool,
}

impl WorkspaceAgentCustodyError {
    fn invalid(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
            stale_workspace: false,
        }
    }

    fn stale(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
            stale_workspace: true,
        }
    }

    fn io(action: &str, error: io::Error) -> Self {
        Self::invalid(format!("{action}: {error}"))
    }

    pub(crate) const fn is_stale_workspace(&self) -> bool {
        self.stale_workspace
    }
}

impl fmt::Display for WorkspaceAgentCustodyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for WorkspaceAgentCustodyError {}

/// Held process-shared proof that the workspace was unassigned when a mutation began.
///
/// Dropping this guard releases the kernel lock. The global order is documented at the daemon
/// composition root; this guard must never be retained while waiting for desktop Recent state.
pub(crate) struct UnassignedWorkspaceGuard {
    _authority: Option<Authority>,
    _lock: CustodyLock,
}

/// Held process-shared proof that one exact agent generation still owns the workspace.
///
/// This guard is intentionally distinct from [`UnassignedWorkspaceGuard`]. It grants no mutation
/// authority; the only consumer is the closed finish-preflight reader, which must inspect while
/// custody remains active and then release through the separate exact-generation transaction.
pub(crate) struct AssignedWorkspaceGuard {
    _authority: Authority,
    _lock: CustodyLock,
}

/// Native directory serials held while initial/restart opens select or create private state.
/// This guard is thread-confined and is not mutation or publication authority.
pub(crate) struct WorkspaceInitializationGuard {
    roots: Vec<PinnedWorkspaceRoot>,
    _locks: Vec<CustodyLock>,
}

struct Authority {
    physical: PinnedWorkspaceRoot,
    storage: PinnedWorkspaceRoot,
    filesystem: PinnedRootFs,
    installation: String,
    directory: String,
}

impl Authority {
    fn from_path(
        root: &Path,
        expected_installation: &str,
    ) -> Result<Self, WorkspaceAgentCustodyError> {
        let authority = Self::from_path_unbound(root)?;
        if authority.installation != expected_installation {
            return Err(WorkspaceAgentCustodyError::stale(
                "workspace custody installation changed",
            ));
        }
        Ok(authority)
    }

    fn from_path_unbound(root: &Path) -> Result<Self, WorkspaceAgentCustodyError> {
        let root = std::fs::canonicalize(root).map_err(|error| {
            WorkspaceAgentCustodyError::io("resolve workspace custody root", error)
        })?;
        let physical = PinnedWorkspaceRoot::open(root.clone())
            .map_err(|error| WorkspaceAgentCustodyError::io("pin workspace custody root", error))?;
        let storage_path = workspace_storage_root(&root)
            .map_err(|error| WorkspaceAgentCustodyError::io("resolve custody storage", error))?;
        let storage = PinnedWorkspaceRoot::open(storage_path)
            .map_err(|error| WorkspaceAgentCustodyError::io("pin custody storage", error))?;
        physical
            .ensure_namespace_identity()
            .and_then(|()| storage.ensure_namespace_identity())
            .map_err(|error| WorkspaceAgentCustodyError::io("verify custody namespace", error))?;
        let physical_identity = physical
            .identity()
            .map_err(|error| WorkspaceAgentCustodyError::io("inspect custody root", error))?;
        let storage_identity = storage
            .identity()
            .map_err(|error| WorkspaceAgentCustodyError::io("inspect custody storage", error))?;
        let installation = workspace_installation(physical_identity, storage_identity);
        Ok(Self {
            physical,
            storage: storage.clone(),
            filesystem: storage.filesystem(),
            installation,
            directory: format!("{:016x}:{:016x}", physical_identity.0, physical_identity.1),
        })
    }

    fn lock(&self) -> Result<CustodyLock, WorkspaceAgentCustodyError> {
        // The physical workspace directory is the one stable inode that exists before private
        // storage is initialized and cannot be unlinked/recreated by replacing a named lock file.
        // Every local participant flocks this descriptor, then validates private storage while it
        // is held. This also lets a still-plain confirmed import serialize with its first open.
        let lock = lock_physical_workspace(&self.physical)?;
        // The descriptors were pinned before a possibly blocking flock. A rollback could have
        // removed their names while this process waited, so revalidate both namespaces only after
        // winning the shared serial and before reading or publishing authority.
        self.physical
            .ensure_namespace_identity()
            .and_then(|()| self.storage.ensure_namespace_identity())
            .map_err(|error| {
                WorkspaceAgentCustodyError::io("revalidate custody after lock wait", error)
            })?;
        Ok(lock)
    }

    fn read(&self) -> Result<WorkspaceAgentCustody, WorkspaceAgentCustodyError> {
        let mut file = match self.filesystem.read_file(Path::new(RECORD_FILE)) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(WorkspaceAgentCustody { generation: None });
            }
            Err(error) => {
                return Err(WorkspaceAgentCustodyError::io(
                    "read workspace custody record",
                    error,
                ));
            }
        };
        let metadata = file
            .metadata()
            .map_err(|error| WorkspaceAgentCustodyError::io("inspect custody record", error))?;
        if !metadata.is_file() || metadata.permissions().mode() & 0o777 != 0o600 {
            return Err(WorkspaceAgentCustodyError::invalid(
                "workspace custody record is not an owner-only regular file",
            ));
        }
        if metadata.len() > MAX_RECORD_BYTES {
            return Err(WorkspaceAgentCustodyError::invalid(
                "workspace custody record exceeds its bound",
            ));
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.read_to_end(&mut bytes)
            .map_err(|error| WorkspaceAgentCustodyError::io("read custody bytes", error))?;
        let text = std::str::from_utf8(&bytes).map_err(|_| {
            WorkspaceAgentCustodyError::invalid("workspace custody record is not UTF-8")
        })?;
        let parsed = Json::parse(text).map_err(|_| {
            WorkspaceAgentCustodyError::invalid("workspace custody record is invalid JSON")
        })?;
        let Json::Object(fields) = &parsed else {
            return Err(WorkspaceAgentCustodyError::invalid(
                "workspace custody record is not an object",
            ));
        };
        let expected_keys = [
            "schema",
            "workspace_installation",
            "workspace_directory",
            "generation",
        ];
        if fields.len() != expected_keys.len()
            || fields
                .iter()
                .zip(expected_keys)
                .any(|((actual, _), expected)| actual != expected)
            || parsed.get("schema").and_then(Json::as_text) != Some(SCHEMA)
            || parsed.get("workspace_installation").and_then(Json::as_text)
                != Some(self.installation.as_str())
            || parsed.get("workspace_directory").and_then(Json::as_text)
                != Some(self.directory.as_str())
        {
            return Err(WorkspaceAgentCustodyError::invalid(
                "workspace custody record does not bind this exact installation",
            ));
        }
        let generation = parsed
            .get("generation")
            .and_then(Json::as_text)
            .filter(|value| valid_generation(value))
            .ok_or_else(|| {
                WorkspaceAgentCustodyError::invalid(
                    "workspace custody record has an invalid generation",
                )
            })?;
        if parsed.encode() != text {
            return Err(WorkspaceAgentCustodyError::invalid(
                "workspace custody record is not canonical",
            ));
        }
        Ok(WorkspaceAgentCustody {
            generation: Some(generation.to_owned()),
        })
    }

    fn publish(&self, generation: &str) -> Result<(), WorkspaceAgentCustodyError> {
        let record = Json::object([
            ("schema", Json::text(SCHEMA)),
            ("workspace_installation", Json::text(&self.installation)),
            ("workspace_directory", Json::text(&self.directory)),
            ("generation", Json::text(generation)),
        ])
        .encode();
        self.publish_record(&record)
    }

    fn publish_record(&self, record: &str) -> Result<(), WorkspaceAgentCustodyError> {
        match self.filesystem.remove_file(Path::new(TEMP_FILE)) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(WorkspaceAgentCustodyError::io(
                    "remove interrupted custody publication",
                    error,
                ));
            }
        }
        self.filesystem
            .write_new_file(
                Path::new(TEMP_FILE),
                record.as_bytes(),
                Permissions::from_mode(0o600),
            )
            .map_err(|error| WorkspaceAgentCustodyError::io("stage custody record", error))?;
        self.filesystem
            .rename(Path::new(TEMP_FILE), Path::new(RECORD_FILE))
            .map_err(|error| WorkspaceAgentCustodyError::io("publish custody record", error))?;
        self.filesystem
            .sync_dir(Path::new(""))
            .map_err(|error| WorkspaceAgentCustodyError::io("sync custody record", error))
    }

    fn clear(&self) -> Result<(), WorkspaceAgentCustodyError> {
        self.filesystem
            .remove_file(Path::new(RECORD_FILE))
            .map_err(|error| WorkspaceAgentCustodyError::io("clear custody record", error))?;
        self.filesystem
            .sync_dir(Path::new(""))
            .map_err(|error| WorkspaceAgentCustodyError::io("sync custody release", error))
    }
}

pub(crate) struct LockedAuthority {
    authority: Authority,
    _lock: CustodyLock,
}

impl LockedAuthority {
    pub(crate) fn status(&self) -> Result<WorkspaceAgentCustody, WorkspaceAgentCustodyError> {
        self.authority.read()
    }

    pub(crate) fn acquire(
        &self,
        confirmed_reopen: bool,
        expected_generation: Option<&str>,
    ) -> Result<String, WorkspaceAgentCustodyError> {
        let current = self.authority.read()?;
        match (current.generation(), confirmed_reopen, expected_generation) {
            (None, false, None) => {}
            (Some(current), true, Some(expected)) if current == expected => {}
            (Some(_), false, None) => {
                return Err(WorkspaceAgentCustodyError::invalid(
                    "workspace is already assigned to an agent",
                ));
            }
            (None, true, _) => {
                return Err(WorkspaceAgentCustodyError::invalid(
                    "workspace agent custody was already released",
                ));
            }
            (Some(_), true, _) => {
                return Err(WorkspaceAgentCustodyError::invalid(
                    "workspace agent custody changed before reopen",
                ));
            }
            _ => {
                return Err(WorkspaceAgentCustodyError::invalid(
                    "workspace agent custody authority is inconsistent",
                ));
            }
        }
        let generation = new_generation()?;
        self.authority.publish(&generation)?;
        Ok(generation)
    }

    pub(crate) fn release(
        &self,
        expected_generation: &str,
    ) -> Result<bool, WorkspaceAgentCustodyError> {
        if !valid_generation(expected_generation) {
            return Err(WorkspaceAgentCustodyError::invalid(
                "workspace agent custody generation is invalid",
            ));
        }
        let current = self.authority.read()?;
        match current.generation() {
            None => Ok(false),
            Some(current) if current == expected_generation => {
                self.authority.clear()?;
                Ok(true)
            }
            Some(_) => Err(WorkspaceAgentCustodyError::invalid(
                "workspace agent custody changed before release",
            )),
        }
    }

    pub(crate) fn require_unassigned(
        self,
    ) -> Result<UnassignedWorkspaceGuard, WorkspaceAgentCustodyError> {
        if self.authority.read()?.is_assigned() {
            return Err(WorkspaceAgentCustodyError::invalid(
                "workspace is assigned to an agent; finish the agent before changing it",
            ));
        }
        let Self {
            authority,
            _lock: lock,
        } = self;
        Ok(UnassignedWorkspaceGuard {
            _authority: Some(authority),
            _lock: lock,
        })
    }

    pub(crate) fn require_generation(
        self,
        expected_generation: &str,
    ) -> Result<AssignedWorkspaceGuard, WorkspaceAgentCustodyError> {
        if !valid_generation(expected_generation) {
            return Err(WorkspaceAgentCustodyError::invalid(
                "workspace agent custody generation is invalid",
            ));
        }
        if self.authority.read()?.generation() != Some(expected_generation) {
            return Err(WorkspaceAgentCustodyError::invalid(
                "workspace agent custody changed before finish inspection",
            ));
        }
        let Self {
            authority,
            _lock: lock,
        } = self;
        Ok(AssignedWorkspaceGuard {
            _authority: authority,
            _lock: lock,
        })
    }
}

pub(crate) fn lock_for_workspace_path(
    root: &Path,
    expected_installation: &str,
) -> Result<LockedAuthority, WorkspaceAgentCustodyError> {
    let authority = Authority::from_path(root, expected_installation)?;
    let lock = authority.lock()?;
    Ok(LockedAuthority {
        authority,
        _lock: lock,
    })
}

/// Try the same pinned authority without waiting for another native writer.
pub(crate) fn try_lock_for_workspace_path(
    root: &Path,
    expected_installation: &str,
) -> Result<Option<LockedAuthority>, WorkspaceAgentCustodyError> {
    let authority = Authority::from_path(root, expected_installation)?;
    let Some(lock) = lock_physical_workspace_mode(&authority.physical, true)? else {
        return Ok(None);
    };
    authority
        .physical
        .ensure_namespace_identity()
        .and_then(|()| authority.storage.ensure_namespace_identity())
        .map_err(|error| {
            WorkspaceAgentCustodyError::io("revalidate custody after try lock", error)
        })?;
    Ok(Some(LockedAuthority {
        authority,
        _lock: lock,
    }))
}

pub(crate) fn require_unassigned_path(
    root: &Path,
) -> Result<UnassignedWorkspaceGuard, WorkspaceAgentCustodyError> {
    let root = std::fs::canonicalize(root).map_err(|error| {
        WorkspaceAgentCustodyError::io("resolve workspace mutation root", error)
    })?;
    let physical = PinnedWorkspaceRoot::open(root.clone())
        .map_err(|error| WorkspaceAgentCustodyError::io("pin workspace mutation root", error))?;
    let lock = lock_physical_workspace(&physical)?;
    let record = workspace_record_file(&root)
        .map_err(|error| WorkspaceAgentCustodyError::io("classify workspace storage", error))?;
    match std::fs::symlink_metadata(record) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(UnassignedWorkspaceGuard {
                _authority: None,
                _lock: lock,
            });
        }
        Ok(_) => {}
        Err(error) => {
            return Err(WorkspaceAgentCustodyError::io(
                "inspect workspace storage",
                error,
            ));
        }
    }
    let authority = Authority::from_path_unbound(&root)?;
    let authority_identity = authority
        .physical
        .identity()
        .map_err(|error| WorkspaceAgentCustodyError::io("verify workspace mutation root", error))?;
    let locked_identity = physical
        .identity()
        .map_err(|error| WorkspaceAgentCustodyError::io("verify locked mutation root", error))?;
    if authority_identity != locked_identity {
        return Err(WorkspaceAgentCustodyError::stale(
            "workspace changed while mutation custody was acquired",
        ));
    }
    LockedAuthority {
        authority,
        _lock: lock,
    }
    .require_unassigned()
}

fn lock_physical_workspace(
    physical: &PinnedWorkspaceRoot,
) -> Result<CustodyLock, WorkspaceAgentCustodyError> {
    lock_physical_workspace_mode(physical, false)?.ok_or_else(|| {
        WorkspaceAgentCustodyError::invalid("blocking custody lock unexpectedly deferred")
    })
}

fn lock_physical_workspace_mode(
    physical: &PinnedWorkspaceRoot,
    nonblocking: bool,
) -> Result<Option<CustodyLock>, WorkspaceAgentCustodyError> {
    if custody_is_held() {
        return Err(WorkspaceAgentCustodyError::invalid(
            "nested workspace custody acquisition was refused",
        ));
    }
    lock_physical_workspace_in_set(physical, nonblocking)
}

// Only the bounded, sorted set entry point may acquire another directory while a lock is held.
// Ordinary mutation entry points must continue through lock_physical_workspace_mode.
fn lock_physical_workspace_in_set(
    physical: &PinnedWorkspaceRoot,
    nonblocking: bool,
) -> Result<Option<CustodyLock>, WorkspaceAgentCustodyError> {
    let identity = physical
        .identity()
        .map_err(|error| WorkspaceAgentCustodyError::io("inspect workspace custody root", error))?;
    let file = physical.independent_lock_directory().map_err(|error| {
        WorkspaceAgentCustodyError::io("open workspace custody directory", error)
    })?;
    let lock = match lock_exclusive(file, identity, nonblocking) {
        Ok(lock) => lock,
        Err(error) if nonblocking && error.kind() == io::ErrorKind::WouldBlock => return Ok(None),
        Err(error) => {
            return Err(WorkspaceAgentCustodyError::io(
                "lock workspace agent custody",
                error,
            ))
        }
    };
    physical.ensure_namespace_identity().map_err(|error| {
        WorkspaceAgentCustodyError::io("revalidate workspace after lock wait", error)
    })?;
    Ok(Some(lock))
}

/// Maximum requested roots, counted before deduplication. Never truncate a requested barrier.
const MAX_INITIALIZATION_ROOTS: usize = 32;

impl WorkspaceInitializationGuard {
    /// Require every requested physical root to belong to this still-held guard.
    /// This does not acquire or extend custody, and validates each requested namespace spelling.
    pub(crate) fn require_roots(
        &self,
        roots: &[PinnedWorkspaceRoot],
    ) -> Result<(), WorkspaceAgentCustodyError> {
        self.ensure_current()?;
        for root in roots {
            root.ensure_namespace_identity().map_err(|error| {
                WorkspaceAgentCustodyError::io("verify requested custody namespace", error)
            })?;
            let identity = root.identity().map_err(|error| {
                WorkspaceAgentCustodyError::io("inspect requested custody identity", error)
            })?;
            if !self
                .roots
                .iter()
                .any(|held| held.identity().ok() == Some(identity))
            {
                return Err(WorkspaceAgentCustodyError::invalid(
                    "requested root is outside this custody set",
                ));
            }
        }
        Ok(())
    }
    /// Verify pinned namespaces and membership in this thread's still-held custody set.
    pub(crate) fn ensure_current(&self) -> Result<(), WorkspaceAgentCustodyError> {
        for root in &self.roots {
            root.ensure_namespace_identity().map_err(|error| {
                WorkspaceAgentCustodyError::io("verify initialized workspace namespace", error)
            })?;
            let identity = root.identity().map_err(|error| {
                WorkspaceAgentCustodyError::io("verify initialized workspace identity", error)
            })?;
            if !custody_contains(identity) {
                return Err(WorkspaceAgentCustodyError::invalid(
                    "workspace initialization custody is no longer held",
                ));
            }
        }
        Ok(())
    }
}

/// Acquire a bounded native-only directory barrier in physical-identity order.
/// This serializes history access; it grants no mutation, agent, grant or approval authority.
/// Call before daemon view locks, and never extend an already-held set.
pub(crate) fn lock_workspace_initialization_set(
    roots: &[PinnedWorkspaceRoot],
) -> Result<WorkspaceInitializationGuard, WorkspaceAgentCustodyError> {
    if roots.is_empty() || roots.len() > MAX_INITIALIZATION_ROOTS || custody_is_held() {
        return Err(WorkspaceAgentCustodyError::invalid(
            "workspace custody set is empty, oversized or nested",
        ));
    }
    let mut ordered = BTreeMap::new();
    for root in roots {
        root.ensure_namespace_identity().map_err(|error| {
            WorkspaceAgentCustodyError::io("verify workspace custody set root", error)
        })?;
        let identity = root.identity().map_err(|error| {
            WorkspaceAgentCustodyError::io("inspect workspace custody set root", error)
        })?;
        ordered.entry(identity).or_insert_with(|| root.clone());
    }
    let mut locks = Vec::with_capacity(ordered.len());
    for root in ordered.values() {
        locks.push(lock_physical_workspace_in_set(root, false)?.ok_or_else(|| {
            WorkspaceAgentCustodyError::invalid("blocking custody set lock unexpectedly deferred")
        })?);
    }
    let guard = WorkspaceInitializationGuard {
        // Retain and revalidate every admitted spelling, including duplicate identities.
        roots: roots.to_vec(),
        _locks: locks,
    };
    guard.ensure_current()?;
    Ok(guard)
}

pub(crate) fn lock_workspace_initialization(
    physical: &PinnedWorkspaceRoot,
) -> Result<WorkspaceInitializationGuard, WorkspaceAgentCustodyError> {
    let identity = physical
        .identity()
        .map_err(|error| WorkspaceAgentCustodyError::io("inspect workspace open root", error))?;
    if custody_contains(identity) {
        let guard = WorkspaceInitializationGuard {
            roots: vec![physical.clone()],
            _locks: Vec::new(),
        };
        guard.ensure_current()?;
        Ok(guard)
    } else if custody_is_held() {
        Err(WorkspaceAgentCustodyError::invalid(
            "workspace open attempted under different custody authority",
        ))
    } else {
        lock_workspace_initialization_set(std::slice::from_ref(physical))
    }
}

pub(crate) fn lock_workspace_path_initialization(
    root: &Path,
    create_missing: bool,
) -> Result<WorkspaceInitializationGuard, WorkspaceAgentCustodyError> {
    if create_missing {
        std::fs::create_dir_all(root)
            .map_err(|error| WorkspaceAgentCustodyError::io("create workspace root", error))?;
    }
    let root = std::fs::canonicalize(root)
        .map_err(|error| WorkspaceAgentCustodyError::io("resolve workspace open root", error))?;
    let physical = PinnedWorkspaceRoot::open(root)
        .map_err(|error| WorkspaceAgentCustodyError::io("pin workspace open root", error))?;
    lock_workspace_initialization(&physical)
}

/// Require an initialized workspace to be unassigned while this thread already holds its exact
/// physical-directory initialization guard.
///
/// Reopening a reusable historical checkout must keep the global directory-flock-before-daemon
/// lock order, but reacquiring the same flock through a new descriptor is neither necessary nor
/// safe. This verifier binds the presented root and opaque installation to the directory identity
/// recorded by the outer guard, then reads the workspace-native custody record while every other
/// local Mesh process remains excluded.
pub(crate) fn require_unassigned_while_initialized(
    root: &Path,
    expected_installation: &str,
) -> Result<(), WorkspaceAgentCustodyError> {
    let authority = Authority::from_path(root, expected_installation)?;
    let identity = authority
        .physical
        .identity()
        .map_err(|error| WorkspaceAgentCustodyError::io("inspect initialized workspace", error))?;
    if !custody_contains(identity) {
        return Err(WorkspaceAgentCustodyError::invalid(
            "workspace custody verification requires its exact initialization guard",
        ));
    }
    if authority.read()?.is_assigned() {
        return Err(WorkspaceAgentCustodyError::invalid(
            "workspace is assigned to an agent; allocate a fresh saved-version checkout",
        ));
    }
    Ok(())
}

fn valid_generation(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn new_generation() -> Result<String, WorkspaceAgentCustodyError> {
    let mut file = File::open("/dev/urandom")
        .map_err(|error| WorkspaceAgentCustodyError::io("open generation source", error))?;
    let mut bytes = [0_u8; 16];
    file.read_exact(&mut bytes)
        .map_err(|error| WorkspaceAgentCustodyError::io("read generation source", error))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

thread_local! {
    static HELD_CUSTODY: RefCell<BTreeSet<(u64, u64)>> = const { RefCell::new(BTreeSet::new()) };
}

fn custody_is_held() -> bool {
    HELD_CUSTODY.with(|held| !held.borrow().is_empty())
}

fn custody_contains(identity: (u64, u64)) -> bool {
    HELD_CUSTODY.with(|held| held.borrow().contains(&identity))
}

struct CustodyLock {
    file: File,
    identity: (u64, u64),
    // Membership is thread-local; moving a live guard to another thread would split authority.
    _same_thread: PhantomData<Rc<()>>,
}

impl Drop for CustodyLock {
    fn drop(&mut self) {
        let _ = unlock(&self.file);
        HELD_CUSTODY.with(|held| held.borrow_mut().remove(&self.identity));
    }
}

#[allow(unsafe_code)]
fn lock_exclusive(file: File, identity: (u64, u64), nonblocking: bool) -> io::Result<CustodyLock> {
    unsafe extern "C" {
        fn flock(fd: std::os::raw::c_int, operation: std::os::raw::c_int) -> std::os::raw::c_int;
    }
    use std::os::fd::AsRawFd as _;
    const LOCK_EX: std::os::raw::c_int = 2;
    const LOCK_NB: std::os::raw::c_int = 4;
    let operation = LOCK_EX | if nonblocking { LOCK_NB } else { 0 };
    // SAFETY: `file` owns a valid descriptor for the lifetime of the returned guard.
    if unsafe { flock(file.as_raw_fd(), operation) } == 0 {
        HELD_CUSTODY.with(|held| held.borrow_mut().insert(identity));
        Ok(CustodyLock {
            file,
            identity,
            _same_thread: PhantomData,
        })
    } else {
        Err(io::Error::last_os_error())
    }
}

#[allow(unsafe_code)]
fn unlock(file: &File) -> io::Result<()> {
    unsafe extern "C" {
        fn flock(fd: std::os::raw::c_int, operation: std::os::raw::c_int) -> std::os::raw::c_int;
    }
    use std::os::fd::AsRawFd as _;
    const LOCK_UN: std::os::raw::c_int = 8;
    // SAFETY: the guard still owns a live descriptor.
    if unsafe { flock(file.as_raw_fd(), LOCK_UN) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::{mpsc, Arc, Barrier};
    use std::time::Duration;

    fn scratch(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "mesh-workspace-custody-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("scratch workspace");
        root
    }

    fn set_fixture(name: &str) -> (std::path::PathBuf, Vec<PinnedWorkspaceRoot>) {
        let base = scratch(name);
        let roots = ["a", "b", "outside"].map(|name| {
            let path = base.join(name);
            std::fs::create_dir(&path).unwrap();
            PinnedWorkspaceRoot::open(path).unwrap()
        });
        (base, roots.to_vec())
    }

    #[test]
    fn initialization_set_is_bounded_deduplicated_and_cannot_expand() {
        let (base, roots) = set_fixture("set-scope");
        assert!(lock_workspace_initialization_set(&[]).is_err());
        assert!(lock_workspace_initialization_set(&vec![roots[0].clone(); 33]).is_err());
        let guard = lock_workspace_initialization_set(&[
            roots[1].clone(),
            roots[0].clone(),
            roots[1].clone(),
        ])
        .expect("two exact roots, requested in reverse order with a duplicate");
        assert_eq!(guard._locks.len(), 2);
        for root in &roots[..2] {
            lock_workspace_initialization(root)
                .unwrap()
                .ensure_current()
                .unwrap();
            assert!(
                lock_physical_workspace_mode(root, true).is_err(),
                "ordinary mutation may not acquire a nested lock"
            );
        }
        assert!(lock_workspace_initialization(&roots[2]).is_err());
        assert!(lock_workspace_initialization_set(&roots[..2]).is_err());
        let borrowed = lock_workspace_initialization(&roots[0]).unwrap();
        drop(guard);
        assert!(
            borrowed.ensure_current().is_err(),
            "a borrowed guard cannot outlive actual custody"
        );
        assert!(!custody_is_held());
        for root in &roots {
            drop(lock_physical_workspace_mode(root, true).unwrap().unwrap());
        }
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn initialization_set_refuses_expansion_from_single_root_custody() {
        let (base, roots) = set_fixture("set-nested");
        let guard = lock_workspace_initialization(&roots[0]).unwrap();
        assert!(lock_workspace_initialization_set(&roots[..2]).is_err());
        assert!(lock_workspace_initialization(&roots[1]).is_err());
        guard.ensure_current().unwrap();
        drop(guard);
        lock_workspace_initialization_set(&roots[..2])
            .unwrap()
            .ensure_current()
            .unwrap();
        assert!(!custody_is_held());
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn initialization_set_releases_partial_acquisition_after_substitution() {
        let (base, mut roots) = set_fixture("set-partial");
        roots.truncate(2);
        roots.sort_by_key(|root| root.identity().unwrap());
        let blocker = lock_workspace_initialization(&roots[1]).unwrap();
        let waiting_roots = roots.clone();
        let (done_tx, done_rx) = mpsc::channel();
        let contender = std::thread::spawn(move || {
            let refused = lock_workspace_initialization_set(&waiting_roots).is_err();
            assert!(
                !custody_is_held(),
                "failed set must release thread membership"
            );
            done_tx.send(refused).unwrap();
        });
        let first = roots[0].clone();
        let probe = std::thread::spawn(move || {
            let end = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                if lock_physical_workspace_mode(&first, true)
                    .unwrap()
                    .is_none()
                {
                    return;
                }
                assert!(
                    std::time::Instant::now() < end,
                    "set never acquired its first root"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        probe.join().unwrap();
        let a_identity = PinnedWorkspaceRoot::open(base.join("a"))
            .unwrap()
            .identity()
            .unwrap();
        let old_path = base.join(if roots[1].identity().unwrap() == a_identity {
            "a"
        } else {
            "b"
        });
        std::fs::rename(&old_path, base.join("displaced")).unwrap();
        std::fs::create_dir(&old_path).unwrap();
        drop(blocker);
        assert!(done_rx.recv_timeout(Duration::from_secs(5)).unwrap());
        contender.join().unwrap();
        drop(
            lock_physical_workspace_mode(&roots[0], true)
                .unwrap()
                .expect("first root released"),
        );
        let replacement = PinnedWorkspaceRoot::open(old_path).unwrap();
        drop(lock_workspace_initialization(&replacement).unwrap());
        std::fs::remove_dir_all(base).unwrap();
    }

    // Executed only by the separate-process test below. No provider or private account is involved.
    #[test]
    fn initialization_set_process_worker() {
        let Some(base) = std::env::var_os("MESH_TEST_CUSTODY_SET_ROOT") else {
            return;
        };
        let base = std::path::PathBuf::from(base);
        let mode = std::env::var("MESH_TEST_CUSTODY_SET_MODE").unwrap();
        let a = PinnedWorkspaceRoot::open(base.join("a")).unwrap();
        let b = PinnedWorkspaceRoot::open(base.join("b")).unwrap();
        std::fs::write(base.join("ready"), b"ready").unwrap();
        let _guard = match mode.as_str() {
            "a" => lock_workspace_initialization(&a),
            "b" => lock_workspace_initialization(&b),
            "set" => lock_workspace_initialization_set(&[b, a]),
            _ => panic!("unknown test mode"),
        }
        .unwrap();
        std::fs::write(base.join("acquired"), b"acquired").unwrap();
    }

    #[test]
    fn initialization_set_contends_with_single_roots_across_processes_in_both_orders() {
        use std::process::{Command, Stdio};
        for (case, parent_set, child_mode) in [
            ("set-first-a", true, "a"),
            ("set-first-b", true, "b"),
            ("set-first-set", true, "set"),
            ("single-first-set", false, "set"),
        ] {
            let (base, roots) = set_fixture(case);
            let guard = if parent_set {
                lock_workspace_initialization_set(&roots[..2]).unwrap()
            } else {
                lock_workspace_initialization(&roots[0]).unwrap()
            };
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "workspace_custody::tests::initialization_set_process_worker",
                    "--nocapture",
                ])
                .env("MESH_TEST_CUSTODY_SET_ROOT", &base)
                .env("MESH_TEST_CUSTODY_SET_MODE", child_mode)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(File::create(base.join("worker.stderr")).unwrap())
                .spawn()
                .unwrap();
            let end = std::time::Instant::now() + Duration::from_secs(5);
            while !base.join("ready").exists() {
                if std::time::Instant::now() >= end {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("custody child did not become ready: {case}");
                }
                assert!(
                    child.try_wait().unwrap().is_none(),
                    "custody child exited before ready: {case}"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
            std::thread::sleep(Duration::from_millis(100));
            let acquired_early = base.join("acquired").exists();
            drop(guard);
            let end = std::time::Instant::now() + Duration::from_secs(5);
            let status = loop {
                if let Some(status) = child.try_wait().unwrap() {
                    break status;
                }
                if std::time::Instant::now() >= end {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("custody child did not resume after release: {case}");
                }
                std::thread::sleep(Duration::from_millis(5));
            };
            assert!(status.success(), "custody child failed: {case}");
            assert!(
                !acquired_early,
                "a held root did not exclude the other process: {case}"
            );
            assert!(base.join("acquired").exists());
            assert!(!custody_is_held());
            std::fs::remove_dir_all(base).unwrap();
        }
    }

    #[test]
    fn custody_survives_restart_and_release_restores_mutation() {
        let root = scratch("restart");
        let first = OpenWorkspace::open(&root).expect("open workspace");
        let installation = first.installation();
        let locked = lock_for_workspace_path(first.physical_root().as_path(), &installation)
            .expect("lock custody");
        let generation = locked.acquire(false, None).expect("first acquire");
        drop(locked);
        drop(first);

        let restarted = OpenWorkspace::open(&root).expect("restart workspace");
        let locked = lock_for_workspace_path(restarted.physical_root().as_path(), &installation)
            .expect("lock after restart");
        assert_eq!(
            locked.status().unwrap().generation(),
            Some(generation.as_str())
        );
        assert!(locked.require_unassigned().is_err());
        let locked = lock_for_workspace_path(restarted.physical_root().as_path(), &installation)
            .expect("relock for release");
        assert!(locked.release(&generation).expect("exact release"));
        drop(locked);
        let locked = lock_for_workspace_path(restarted.physical_root().as_path(), &installation)
            .expect("relock after release");
        let _mutation = locked.require_unassigned().expect("mutation restored");
        drop(_mutation);
        drop(restarted);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_record_fails_closed_for_acquire_and_mutation() {
        let root = scratch("malformed");
        let open = OpenWorkspace::open(&root).expect("open workspace");
        let installation = open.installation();
        std::fs::write(open.storage_root().as_path().join(RECORD_FILE), b"{broken")
            .expect("malformed custody");
        std::fs::set_permissions(
            open.storage_root().as_path().join(RECORD_FILE),
            Permissions::from_mode(0o600),
        )
        .unwrap();
        let locked = lock_for_workspace_path(open.physical_root().as_path(), &installation)
            .expect("lock malformed custody");
        assert!(locked.status().is_err());
        assert!(locked.acquire(false, None).is_err());
        assert!(locked.require_unassigned().is_err());
        drop(open);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cloned_pinned_roots_do_not_share_lock_ownership() {
        let root = scratch("cloned-root-lock");
        let physical = PinnedWorkspaceRoot::open(root.clone()).expect("pin");
        let clone = physical.clone();
        let guard = lock_workspace_initialization(&physical).expect("first lock");
        let (started, entered) = mpsc::channel();
        let (acquired, result) = mpsc::channel();
        let contender = std::thread::spawn(move || {
            started.send(()).unwrap();
            let _guard = lock_workspace_initialization(&clone).expect("second lock");
            acquired.send(()).unwrap();
        });
        entered
            .recv_timeout(Duration::from_secs(2))
            .expect("contender started");
        assert!(
            result.recv_timeout(Duration::from_millis(100)).is_err(),
            "a cloned descriptor must not inherit the first caller's lock ownership"
        );
        drop(guard);
        result
            .recv_timeout(Duration::from_secs(2))
            .expect("contender acquired after release");
        contender.join().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn physical_rename_preserves_exact_custody_generation() {
        let root = scratch("rename");
        let renamed = root.with_extension("renamed");
        let _ = std::fs::remove_dir_all(&renamed);
        let first = OpenWorkspace::open(&root).expect("open workspace");
        let installation = first.installation();
        let locked = lock_for_workspace_path(first.physical_root().as_path(), &installation)
            .expect("lock custody");
        let generation = locked.acquire(false, None).expect("acquire");
        drop(locked);
        drop(first);
        std::fs::rename(&root, &renamed).expect("rename physical workspace");
        let reopened = OpenWorkspace::open(&renamed).expect("open renamed workspace");
        assert_eq!(reopened.installation(), installation);
        let locked = lock_for_workspace_path(reopened.physical_root().as_path(), &installation)
            .expect("lock renamed custody");
        assert_eq!(
            locked.status().unwrap().generation(),
            Some(generation.as_str())
        );
        assert!(locked.release(&generation).unwrap());
        drop(reopened);
        std::fs::remove_dir_all(renamed).unwrap();
    }

    #[test]
    fn acquire_and_mutation_are_linearized_in_both_orders() {
        let root = scratch("linearized");
        let open = OpenWorkspace::open(&root).expect("open workspace");
        let installation = open.installation();
        let path = open.physical_root().as_path().to_path_buf();
        drop(open);

        let admitted = lock_for_workspace_path(&path, &installation)
            .unwrap()
            .require_unassigned()
            .expect("mutation first");
        let started = Arc::new(Barrier::new(2));
        let joining = Arc::clone(&started);
        let acquire_path = path.clone();
        let acquire_installation = installation.clone();
        let acquiring = std::thread::spawn(move || {
            joining.wait();
            lock_for_workspace_path(&acquire_path, &acquire_installation)
                .unwrap()
                .acquire(false, None)
        });
        started.wait();
        std::thread::sleep(Duration::from_millis(20));
        drop(admitted);
        let generation = acquiring.join().unwrap().expect("acquire waits then wins");

        let refused = lock_for_workspace_path(&path, &installation)
            .unwrap()
            .require_unassigned();
        assert!(refused.is_err(), "custody-first mutation must refuse");
        lock_for_workspace_path(&path, &installation)
            .unwrap()
            .release(&generation)
            .unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn replacing_the_legacy_named_lock_cannot_split_the_physical_workspace_serial() {
        let root = scratch("named-lock-replacement");
        let open = OpenWorkspace::open(&root).expect("open workspace");
        let installation = open.installation();
        let path = open.physical_root().as_path().to_path_buf();
        let legacy_lock = open.storage_root().as_path().join(LOCK_FILE);
        std::fs::write(&legacy_lock, b"first inode").expect("legacy named lock");
        let held = lock_for_workspace_path(&path, &installation)
            .unwrap()
            .require_unassigned()
            .expect("first mutation owns physical serial");

        let waiting_authority = Authority::from_path(&path, &installation).unwrap();
        let ready = Arc::new(Barrier::new(2));
        let joining = Arc::clone(&ready);
        let (result_tx, result_rx) = mpsc::channel();
        let waiting = std::thread::spawn(move || {
            joining.wait();
            let result = waiting_authority.lock().map(|lock| LockedAuthority {
                authority: waiting_authority,
                _lock: lock,
            });
            result_tx
                .send(result.and_then(|locked| locked.acquire(false, None)))
                .unwrap();
        });
        ready.wait();
        std::thread::sleep(Duration::from_millis(20));
        std::fs::remove_file(&legacy_lock).expect("unlink named lock");
        std::fs::write(&legacy_lock, b"replacement inode").expect("replace named lock");
        assert!(
            result_rx.recv_timeout(Duration::from_millis(30)).is_err(),
            "a named file replacement must not release the directory serial"
        );
        drop(held);
        let generation = result_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("waiter wakes after physical serial release")
            .expect("replacing irrelevant named file cannot invalidate workspace");
        waiting.join().unwrap();
        lock_for_workspace_path(&path, &installation)
            .unwrap()
            .release(&generation)
            .unwrap();
        drop(open);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn acquire_waiting_behind_rollback_cannot_publish_into_deleted_workspace() {
        let root = scratch("rollback-acquire");
        let open = OpenWorkspace::open(&root).expect("open workspace");
        let installation = open.installation();
        let path = open.physical_root().as_path().to_path_buf();
        drop(open);

        let rollback_authority = Authority::from_path(&path, &installation).unwrap();
        let rollback_lock = rollback_authority
            .lock()
            .expect("rollback owns shared serial");
        let waiting_authority = Authority::from_path(&path, &installation).unwrap();
        let ready = Arc::new(Barrier::new(2));
        let joining = Arc::clone(&ready);
        let acquire = std::thread::spawn(move || {
            joining.wait();
            let lock = waiting_authority.lock()?;
            LockedAuthority {
                authority: waiting_authority,
                _lock: lock,
            }
            .acquire(false, None)
        });
        ready.wait();
        std::thread::sleep(Duration::from_millis(20));
        std::fs::remove_dir_all(&root).expect("rollback removes exact workspace");
        drop(rollback_lock);
        let refusal = acquire.join().unwrap();
        assert!(refusal.is_err(), "stale pinned authority must not publish");
        assert!(!root.exists());
    }
}
