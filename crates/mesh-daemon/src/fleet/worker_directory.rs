//! Native macOS ownership of the shared receiving ledger. No network-selected paths or keys.
use super::{Limits, RemoteAdmissionRegistry};
use crate::ipc::Json;
use crate::root_authority::PinnedWorkspaceRoot;
use crate::ProtectedWorkspaceRoot;
use mesh_store::fleet::{FleetStore, FleetStoreAuthority, FleetStoreError};
use mesh_store::RecordDigest;
use std::fs::{File, Permissions};
use std::io::{self, Read as _};
use std::os::darwin::fs::MetadataExt as _;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const DATABASE: &str = "worker.sqlite";
const RECEIPT: &str = "worker.json";
const MAX_RECEIPT: u64 = 4096;
const NAMES: [&str; 5] = [
    DATABASE,
    RECEIPT,
    "worker.sqlite-wal",
    "worker.sqlite-shm",
    "worker.sqlite-journal",
];

fn unavailable() -> io::Error {
    io::Error::other("remote worker ledger is unavailable or needs reconciliation")
}
fn file_identity(file: &File, uid: u32) -> io::Result<String> {
    let m = file.metadata()?;
    if !m.is_file() || m.nlink() != 1 || m.uid() != uid || m.permissions().mode() & 0o077 != 0 {
        return Err(unavailable());
    }
    Ok(format!(
        "{:016x}:{:016x}:{}:{}",
        m.dev(),
        m.ino(),
        m.st_birthtime(),
        m.st_birthtime_nsec()
    ))
}

#[derive(Debug)]
struct Owner {
    root: PinnedWorkspaceRoot,
    token: ProtectedWorkspaceRoot,
    protected: Vec<ProtectedWorkspaceRoot>,
    uid: u32,
    _lock: File,
}
impl Owner {
    fn verify(&self) -> io::Result<()> {
        self.root.ensure_protected_identity(self.token)?;
        self.root.ensure_namespace_identity()?;
        let m = self.root.try_clone_directory()?.metadata()?;
        if m.uid() != self.uid || m.permissions().mode() & 0o077 != 0 {
            return Err(unavailable());
        }
        for protected in &self.protected {
            if self.root.is_within(*protected)? {
                return Err(unavailable());
            }
        }
        Ok(())
    }
}

#[allow(unsafe_code)]
fn owner(
    path: &Path,
    token: ProtectedWorkspaceRoot,
    protected: &[ProtectedWorkspaceRoot],
) -> io::Result<Arc<Owner>> {
    use std::os::fd::AsRawFd as _;
    unsafe extern "C" {
        fn flock(fd: i32, operation: i32) -> i32;
        fn geteuid() -> u32;
    }
    if !path.is_absolute() {
        return Err(unavailable());
    }
    let root = PinnedWorkspaceRoot::open(path.to_owned())?;
    let lock = root.independent_lock_directory()?;
    // SAFETY: geteuid has no arguments and lock owns a live independent directory descriptor.
    let value = Arc::new(Owner {
        root,
        token,
        protected: protected.to_vec(),
        uid: unsafe { geteuid() },
        _lock: lock,
    });
    value.verify()?;
    // SAFETY: the descriptor stays live with Owner. LOCK_EX|LOCK_NB refuses another native owner.
    if unsafe { flock(value._lock.as_raw_fd(), 2 | 4) } != 0 {
        return Err(unavailable());
    }
    value.verify()?;
    Ok(value)
}

#[derive(Debug)]
struct Authority {
    owner: Arc<Owner>,
    database: File,
    receipt: File,
    encoded: String,
}
impl Authority {
    fn verify(&self) -> io::Result<()> {
        self.owner.verify()?;
        let fs = self.owner.root.filesystem();
        for name in fs.read_directory_names_bounded(Path::new(""), NAMES.len())? {
            if !NAMES
                .iter()
                .any(|allowed| name == std::ffi::OsStr::new(allowed))
            {
                return Err(unavailable());
            }
            file_identity(&fs.inspect_entry(Path::new(&name))?, self.owner.uid)?;
        }
        for (name, retained) in [(DATABASE, &self.database), (RECEIPT, &self.receipt)] {
            let named = fs.inspect_entry(Path::new(name))?;
            if file_identity(&named, self.owner.uid)? != file_identity(retained, self.owner.uid)? {
                return Err(unavailable());
            }
        }
        let file = fs.read_only().read_file(Path::new(RECEIPT))?;
        let mut bytes = Vec::new();
        file.take(MAX_RECEIPT + 1).read_to_end(&mut bytes)?;
        if bytes != self.encoded.as_bytes() {
            return Err(unavailable());
        }
        self.owner.verify()
    }
}
impl FleetStoreAuthority for Authority {
    fn check(&self) -> Result<(), FleetStoreError> {
        self.verify().map_err(|_| FleetStoreError::AuthorityChanged)
    }
}

/// One native macOS worker's shared private admission ledger, independent of input allocations.
/// The configured worker key is correlation; this does not authenticate a coordinator or launch.
/// Registry handles retain ownership, so dropping this wrapper cannot release a live connection's
/// directory lock. No missing file, partial initialization or old process is adopted on reopen.
pub struct NativeRemoteWorkerDirectory {
    authority: Arc<Authority>,
    worker: String,
    database_path: PathBuf,
}
impl NativeRemoteWorkerDirectory {
    /// Explicit first provisioning in an existing empty native-owned private directory. Every
    /// entry created before failure is retained. Calling create again on a partial directory refuses.
    pub fn create(
        path: &Path,
        expected: ProtectedWorkspaceRoot,
        worker_key: &str,
        protected: &[ProtectedWorkspaceRoot],
    ) -> io::Result<Self> {
        Self::open_inner(path, expected, worker_key, protected, true)
    }
    /// Reopen this exact provisioned directory and worker identity. Never initializes absent state.
    pub fn reopen(
        path: &Path,
        expected: ProtectedWorkspaceRoot,
        worker_key: &str,
        protected: &[ProtectedWorkspaceRoot],
    ) -> io::Result<Self> {
        Self::open_inner(path, expected, worker_key, protected, false)
    }
    fn open_inner(
        path: &Path,
        expected: ProtectedWorkspaceRoot,
        worker_key: &str,
        protected: &[ProtectedWorkspaceRoot],
        initialize: bool,
    ) -> io::Result<Self> {
        if !RecordDigest::parse_hex(worker_key).is_ok_and(|key| key.to_string() == worker_key) {
            return Err(unavailable());
        }
        let owner = owner(path, expected, protected)?;
        // SQLite resolves a whole family of filenames. The macOS persistent directory reference
        // prevents an ancestor rename from redirecting those writes to a substituted namespace.
        let database_path = expected.stable_reference()?.join(DATABASE);
        let fs = owner.root.filesystem();
        if initialize {
            if !fs
                .read_directory_names_bounded(Path::new(""), 1)?
                .is_empty()
            {
                return Err(unavailable());
            }
            owner.verify()?;
            fs.write_new_file(Path::new(DATABASE), &[], Permissions::from_mode(0o600))?;
        }
        let database = fs.inspect_entry(Path::new(DATABASE))?;
        let encoded = Json::object([
            ("schema", Json::text("mesh.remote-worker-directory/v1")),
            ("root", Json::text(expected.directory_token())),
            ("worker", Json::text(worker_key)),
            ("database", Json::text(file_identity(&database, owner.uid)?)),
        ])
        .encode();
        if initialize {
            owner.verify()?;
            fs.write_new_file(
                Path::new(RECEIPT),
                encoded.as_bytes(),
                Permissions::from_mode(0o600),
            )?;
            owner.root.sync()?;
        }
        let receipt = fs.inspect_entry(Path::new(RECEIPT))?;
        let authority = Arc::new(Authority {
            owner,
            database,
            receipt,
            encoded,
        });
        authority.verify()?;
        // Schema initialization happens only during explicit creation, after the physical receipt
        // exists. A crash here leaves evidence that reopen refuses, rather than an empty new history.
        drop(
            FleetStore::open_guarded(&database_path, initialize, authority.clone())
                .map_err(|_| unavailable())?,
        );
        authority.verify()?;
        Ok(Self {
            authority,
            worker: worker_key.into(),
            database_path,
        })
    }
    /// Open an objective view through this worker's shared ledger, retaining native authority.
    /// The supervisor must independently authenticate the coordinator and authorize these limits.
    pub fn registry(
        &self,
        coordinator: &str,
        objective: &str,
        limits: Limits,
    ) -> io::Result<RemoteAdmissionRegistry> {
        self.authority.verify()?;
        let store = FleetStore::open_guarded(&self.database_path, false, self.authority.clone())
            .map_err(|_| unavailable())?;
        let registry =
            RemoteAdmissionRegistry::new(store, coordinator, &self.worker, objective, limits)
                .map_err(|_| unavailable())?;
        self.authority.verify()?;
        Ok(registry)
    }
}

#[cfg(test)]
mod tests;
