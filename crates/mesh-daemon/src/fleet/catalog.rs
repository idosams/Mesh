//! Native macOS fleet catalogue. Restart replays facts without reclaiming processes or lane custody.
use super::service::{FleetService, NativeLaneAllocator};
use super::{Command, Limits, Runtime, State};
use crate::ipc::Json;
use crate::project_attachment::ProvisionedAttachment;
use crate::root_authority::PinnedWorkspaceRoot;
use crate::{CheckpointRuntimeParameters, ProtectedWorkspaceRoot, TrustedReviewers};
use mesh_store::fleet::{FleetStore, FleetStoreAuthority, FleetStoreError};
use mesh_store::RecordDigest;
use mesh_types::{Blake3, ContentDigest as _};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::{File, Permissions};
use std::io::{self, Read as _};
use std::os::darwin::fs::MetadataExt as _;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

mod provider_policy;
pub use provider_policy::FleetProviderPolicy;

const MAX_FLEETS: usize = 16;
const DATABASE: &str = "fleet.sqlite";
const RECEIPT: &str = "allocation.json";
const MAX_RECEIPT: u64 = 65_536;
fn unavailable() -> io::Error {
    io::Error::other("Fleet storage is unavailable or needs reconciliation")
}
fn valid_request(request: &str) -> bool {
    request.len() == 32
        && request
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn objective(request: &str) -> String {
    format!(
        "fleet-{}",
        Blake3::digest_bytes(format!("mesh.fleet.request/v1:{request}").as_bytes())
    )
}
fn directory(root: &PinnedWorkspaceRoot) -> io::Result<ProtectedWorkspaceRoot> {
    let (device, inode) = root.identity()?;
    ProtectedWorkspaceRoot::from_directory_token(&format!("{device:016x}:{inode:016x}"))
}
fn file_identity(file: &File) -> io::Result<String> {
    let m = file.metadata()?;
    if !m.is_file() || m.nlink() != 1 || m.permissions().mode() & 0o077 != 0 {
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
fn read_receipt(root: &PinnedWorkspaceRoot) -> io::Result<String> {
    let file = root.filesystem().inspect_entry(Path::new(RECEIPT))?;
    file_identity(&file)?;
    let mut bytes = Vec::new();
    file.take(MAX_RECEIPT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_RECEIPT {
        return Err(unavailable());
    }
    String::from_utf8(bytes).map_err(|_| unavailable())
}

/// Complete native-approved creation input. This grants no human approval or source writeback.
#[derive(Clone, Debug)]
pub struct AttachedFleetRequest {
    /// Stable 32-character lower-case hexadecimal request, retained across uncertain replies.
    pub request: String,
    /// Work authorized for the coordinator and its bounded descendants.
    pub goal: String,
    /// Exact saved input from the retained attachment.
    pub version: RecordDigest,
    /// Human-authorized objective limits.
    pub limits: Limits,
}
impl AttachedFleetRequest {
    /// Parse closed native input before touching storage or allocating any lane.
    pub fn new(request: &str, goal: &str, version: &str, limits: Limits) -> io::Result<Self> {
        let digest = RecordDigest::parse_hex(version).map_err(|_| unavailable())?;
        if digest.to_string() != version {
            return Err(unavailable());
        }
        let value = Self {
            request: request.into(),
            goal: goal.into(),
            version: digest,
            limits,
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> io::Result<()> {
        if !valid_request(&self.request) {
            return Err(unavailable());
        }
        State::default()
            .apply(&self.start())
            .map_err(|_| unavailable())
    }
    fn start(&self) -> Command {
        Command::Start {
            goal: self.goal.clone(),
            limits: self.limits.clone(),
        }
    }
}

#[derive(Debug)]
struct Owner {
    path: PathBuf,
    root: PinnedWorkspaceRoot,
    // The independent descriptor's lifetime owns the nonblocking directory flock.
    _lock: File,
    uid: u32,
}
impl Owner {
    fn check(&self) -> io::Result<()> {
        self.root.ensure_namespace_identity()?;
        let m = self.root.try_clone_directory()?.metadata()?;
        if m.uid() != self.uid || m.permissions().mode() & 0o077 != 0 {
            return Err(unavailable());
        }
        Ok(())
    }
}
#[allow(unsafe_code)]
fn owner(root: PinnedWorkspaceRoot, path: PathBuf) -> io::Result<Owner> {
    use std::os::fd::AsRawFd as _;
    unsafe extern "C" {
        fn flock(fd: i32, operation: i32) -> i32;
        fn geteuid() -> u32;
    }
    let lock = root.independent_lock_directory()?;
    // SAFETY: geteuid takes no arguments; lock retains a live directory descriptor. LOCK_EX|LOCK_NB
    // refuses a second native owner rather than waiting or taking over another host's workers.
    let uid = unsafe { geteuid() };
    let result = Owner {
        path,
        root,
        _lock: lock,
        uid,
    };
    result.check()?;
    if unsafe { flock(result._lock.as_raw_fd(), 2 | 4) } != 0 {
        return Err(unavailable());
    }
    Ok(result)
}

#[derive(Debug)]
struct LedgerAuthority {
    owner: Arc<Owner>,
    root: PinnedWorkspaceRoot,
    file: File,
    lanes: PinnedWorkspaceRoot,
    receipt: String,
}
impl LedgerAuthority {
    fn verify(&self) -> io::Result<()> {
        self.owner.check()?;
        self.root.ensure_namespace_identity()?;
        self.lanes.ensure_namespace_identity()?;
        let root_meta = self.root.try_clone_directory()?.metadata()?;
        if root_meta.uid() != self.owner.uid || root_meta.permissions().mode() & 0o077 != 0 {
            return Err(unavailable());
        }
        if read_receipt(&self.root)? != self.receipt {
            return Err(unavailable());
        }
        let named = self.root.filesystem().inspect_entry(Path::new(DATABASE))?;
        if file_identity(&named)? != file_identity(&self.file)?
            || named.metadata()?.uid() != self.owner.uid
        {
            return Err(unavailable());
        }
        for suffix in ["-wal", "-shm", "-journal"] {
            match self
                .root
                .filesystem()
                .inspect_entry(Path::new(&format!("{DATABASE}{suffix}")))
            {
                Ok(file) => {
                    file_identity(&file)?;
                    if file.metadata()?.uid() != self.owner.uid {
                        return Err(unavailable());
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}
impl FleetStoreAuthority for LedgerAuthority {
    fn check(&self) -> Result<(), FleetStoreError> {
        self.verify().map_err(|_| FleetStoreError::AuthorityChanged)
    }
}
struct OpenFleet {
    policy: FleetProviderPolicy,
    service: Arc<FleetService>,
    restored: bool,
}
/// One native owner's bounded catalogue at an existing owner-private application directory.
/// Holding a returned service keeps this ownership alive. No renderer path or credential is accepted.
pub struct NativeFleetDirectory {
    owner: Arc<Owner>,
    reviewers: TrustedReviewers,
    checkpoint: CheckpointRuntimeParameters,
    fleets: Mutex<BTreeMap<String, OpenFleet>>,
}
impl NativeFleetDirectory {
    /// Admit the app-owned directory. Another owner's retained lease causes refusal, never takeover.
    pub fn open(
        path: &Path,
        reviewers: TrustedReviewers,
        checkpoint: CheckpointRuntimeParameters,
    ) -> io::Result<Self> {
        if !path.is_absolute() {
            return Err(unavailable());
        }
        let root = PinnedWorkspaceRoot::open(path.to_owned())?;
        let canonical = path.canonicalize()?;
        if PinnedWorkspaceRoot::open(canonical.clone())?.identity()? != root.identity()? {
            return Err(unavailable());
        }
        let owner = Arc::new(owner(root, canonical)?);
        Ok(Self {
            owner,
            reviewers,
            checkpoint,
            fleets: Mutex::new(BTreeMap::new()),
        })
    }

    /// Create or retry an explicitly requested fleet from an exact attached saved version.
    /// Interrupted allocations remain present; this method never recreates a missing ledger.
    pub fn create_attached(
        &self,
        source: &ProvisionedAttachment,
        request: &AttachedFleetRequest,
    ) -> io::Result<Arc<FleetService>> {
        self.create_attached_with_providers(source, request, &FleetProviderPolicy::default())
    }

    /// Allocate an exact provider policy with the request. Retrying cannot widen the allowed set,
    /// change the coordinator, adopt old workers or rewrite an existing allocation receipt.
    pub fn create_attached_with_providers(
        &self,
        source: &ProvisionedAttachment,
        request: &AttachedFleetRequest,
        policy: &FleetProviderPolicy,
    ) -> io::Result<Arc<FleetService>> {
        request.validate()?;
        source.validate_lane_version(&request.version.to_string())?;
        self.owner.check()?;
        if self.owner.root.is_within(source.protected_source()?)? {
            return Err(unavailable());
        }
        let mut fleets = self.fleets.lock().map_err(|_| unavailable())?;
        let id = objective(&request.request);
        if let Some(existing) = fleets.get(&id) {
            let state = existing.service.native_state().map_err(|_| unavailable())?;
            if &existing.policy != policy
                || state.goal.as_deref() != Some(&request.goal)
                || state.limits.as_ref() != Some(&request.limits)
            {
                return Err(unavailable());
            }
            existing
                .service
                .create_root_from_attachment(
                    "root",
                    &request.goal,
                    policy.coordinator(),
                    source,
                    request.version,
                )
                .map_err(|_| unavailable())?;
            return Ok(existing.service.clone());
        }
        let names = self
            .owner
            .root
            .filesystem()
            .read_directory_names_bounded(Path::new(""), MAX_FLEETS)?;
        if names.len() >= MAX_FLEETS && !names.iter().any(|name| name == OsStr::new(&id)) {
            return Err(unavailable());
        }
        let (root, fresh) = match self.owner.root.create_child_directory(OsStr::new(&id)) {
            Ok(root) => (root, true),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (
                self.owner.root.open_child_directory(OsStr::new(&id))?,
                false,
            ),
            Err(error) => return Err(error),
        };
        if fresh {
            root.filesystem().write_new_file(
                Path::new(DATABASE),
                &[],
                Permissions::from_mode(0o600),
            )?;
            let file = root.filesystem().inspect_entry(Path::new(DATABASE))?;
            root.create_child_directory(OsStr::new("lanes"))?;
            let encoded = receipt(&root, &file, source.id(), request, policy)?.encode();
            root.filesystem().write_new_file(
                Path::new(RECEIPT),
                encoded.as_bytes(),
                Permissions::from_mode(0o600),
            )?;
        }
        let stored = read_receipt(&root)?;
        let file = root.filesystem().inspect_entry(Path::new(DATABASE))?;
        if stored != receipt(&root, &file, source.id(), request, policy)?.encode() {
            return Err(unavailable());
        }
        let service = self.open_service(root, stored, fresh, &id, request, policy)?;
        service
            .create_root_from_attachment(
                "root",
                &request.goal,
                policy.coordinator(),
                source,
                request.version,
            )
            .map_err(|_| unavailable())?;
        self.owner.check()?;
        fleets.insert(
            id,
            OpenFleet {
                policy: policy.clone(),
                service: service.clone(),
                restored: !fresh,
            },
        );
        Ok(service)
    }

    fn open_service(
        &self,
        root: PinnedWorkspaceRoot,
        stored: String,
        initialize: bool,
        id: &str,
        request: &AttachedFleetRequest,
        policy: &FleetProviderPolicy,
    ) -> io::Result<Arc<FleetService>> {
        let file = root.filesystem().inspect_entry(Path::new(DATABASE))?;
        let authority = Arc::new(LedgerAuthority {
            owner: self.owner.clone(),
            root: root.clone(),
            lanes: root.open_child_directory(OsStr::new("lanes"))?,
            file,
            receipt: stored,
        });
        authority.verify()?;
        // Stable directory references prevent an ancestor rename from redirecting SQLite's family.
        let path = directory(&root)?.stable_reference()?;
        let store = FleetStore::open_guarded(&path.join(DATABASE), initialize, authority)
            .map_err(|_| unavailable())?;
        let mut runtime = Runtime::open(store, id).map_err(|_| unavailable())?;
        if initialize {
            runtime
                .record("start", request.start())
                .map_err(|_| unavailable())?;
        }
        if runtime.state().goal.as_deref() != Some(&request.goal)
            || runtime.state().limits.as_ref() != Some(&request.limits)
            || runtime.state().lanes.values().any(|lane| {
                !policy.providers.contains(&lane.provider)
                    || (lane.parent.is_none() && lane.provider != policy.coordinator)
            })
        {
            return Err(unavailable());
        }
        let lanes = root.open_child_directory(OsStr::new("lanes"))?;
        let path = self.owner.path.join(id).join("lanes");
        if PinnedWorkspaceRoot::open(path.clone())?.identity()? != lanes.identity()? {
            return Err(unavailable());
        }
        let allocator =
            NativeLaneAllocator::open(&path, self.reviewers.clone(), self.checkpoint, vec![])
                .map_err(|_| unavailable())?;
        FleetService::new(runtime, Arc::new(allocator), policy.providers.clone())
            .map(Arc::new)
            .map_err(|_| unavailable())
    }

    /// Return only a service allocated by this catalogue instance. Discovery after restart is
    /// observation, not permission to adopt a workspace or launch its saved attempts.
    pub fn current_service(&self, objective: &str) -> io::Result<Arc<FleetService>> {
        self.owner.check()?;
        let held = self.fleets.lock().map_err(|_| unavailable())?;
        let entry = held
            .get(objective)
            .filter(|entry| !entry.restored)
            .ok_or_else(unavailable)?;
        entry.service.native_state().map_err(|_| unavailable())?;
        self.owner.check()?;
        Ok(entry.service.clone())
    }

    /// Discover saved history without granting a current-session execution service.
    pub fn history(&self, objective: &str) -> io::Result<super::service::FleetHistory> {
        self.owner.check()?;
        if !self
            .fleets
            .lock()
            .map_err(|_| unavailable())?
            .contains_key(objective)
        {
            self.snapshot()?;
        }
        let held = self.fleets.lock().map_err(|_| unavailable())?;
        let entry = held.get(objective).ok_or_else(unavailable)?;
        entry.service.native_state().map_err(|_| unavailable())?;
        self.owner.check()?;
        Ok(super::service::FleetHistory(entry.service.clone()))
    }

    /// Recover bounded saved facts without requiring source projects online or launching workers.
    /// Unreadable/incomplete entries stay visible; restored lanes have no adopted live context.
    pub fn snapshot(&self) -> io::Result<Json> {
        self.owner.check()?;
        let mut fleets = self.fleets.lock().map_err(|_| unavailable())?;
        let names = self
            .owner
            .root
            .filesystem()
            .read_directory_names_bounded(Path::new(""), MAX_FLEETS)?;
        let mut rows = Vec::new();
        for name in names {
            let Some(id) = name.to_str() else {
                return Err(unavailable());
            };
            if !id.starts_with("fleet-") || id.len() != 70 {
                return Err(unavailable());
            }
            if !fleets.contains_key(id) {
                let recovered = (|| {
                    let root = self.owner.root.open_child_directory(&name)?;
                    let stored = read_receipt(&root)?;
                    let value = Json::parse(&stored).map_err(|_| unavailable())?;
                    let (project, request) = decode_request(&value)?;
                    let policy = FleetProviderPolicy::decode(&value)?;
                    let file = root.filesystem().inspect_entry(Path::new(DATABASE))?;
                    if objective(&request.request) != id
                        || receipt(&root, &file, &project, &request, &policy)?.encode() != stored
                    {
                        return Err(unavailable());
                    }
                    self.open_service(root, stored, false, id, &request, &policy)
                        .map(|service| (service, policy))
                })();
                if let Ok((service, policy)) = recovered {
                    fleets.insert(
                        id.into(),
                        OpenFleet {
                            policy,
                            service,
                            restored: true,
                        },
                    );
                }
            }
            let entry = fleets.get(id);
            let snapshot = entry.and_then(|entry| entry.service.snapshot().ok());
            let available = snapshot.is_some();
            rows.push(Json::object([
                ("objective", Json::text(id)),
                ("state", snapshot.unwrap_or(Json::Null)),
                (
                    "ownership",
                    Json::text(match entry.filter(|_| available) {
                        Some(entry) if !entry.restored => "current-host",
                        Some(_) => "restored-unattached",
                        None => "unavailable",
                    }),
                ),
            ]));
        }
        self.owner.check()?;
        Ok(Json::object([
            ("schema", Json::text("mesh.native-fleets/v1")),
            ("fleets", Json::Array(rows)),
        ]))
    }
}
fn receipt(
    root: &PinnedWorkspaceRoot,
    file: &File,
    project: &str,
    request: &AttachedFleetRequest,
    policy: &FleetProviderPolicy,
) -> io::Result<Json> {
    let mut value = Json::object([
        (
            "schema",
            Json::text(if policy.legacy() {
                "mesh.native-fleet-allocation/v1"
            } else {
                "mesh.native-fleet-allocation/v2"
            }),
        ),
        ("directory", Json::text(directory(root)?.directory_token())),
        ("database", Json::text(file_identity(file)?)),
        (
            "lanes_directory",
            Json::text(
                directory(&root.open_child_directory(OsStr::new("lanes"))?)?.directory_token(),
            ),
        ),
        ("project", Json::text(project)),
        ("request", Json::text(&request.request)),
        ("goal", Json::text(&request.goal)),
        ("version", Json::text(request.version.to_string())),
        ("lanes", Json::Number(request.limits.lanes)),
        ("concurrency", Json::Number(request.limits.concurrency)),
        ("depth", Json::Number(request.limits.depth)),
        ("retries", Json::Number(request.limits.retries)),
    ]);
    if !policy.legacy() {
        let Json::Object(fields) = &mut value else {
            unreachable!()
        };
        fields.push((
            "coordinator_provider".into(),
            Json::text(policy.coordinator()),
        ));
        fields.push((
            "providers".into(),
            Json::Array(policy.providers().map(Json::text).collect()),
        ));
    }
    Ok(value)
}
fn decode_request(value: &Json) -> io::Result<(String, AttachedFleetRequest)> {
    let text = |field| {
        value
            .get(field)
            .and_then(Json::as_text)
            .ok_or_else(unavailable)
    };
    let number = |field| {
        value
            .get(field)
            .and_then(Json::as_u64)
            .ok_or_else(unavailable)
    };
    let project = text("project")?.to_owned();
    if RecordDigest::parse_hex(&project)
        .map_err(|_| unavailable())?
        .to_string()
        != project
    {
        return Err(unavailable());
    }
    let request = AttachedFleetRequest {
        request: text("request")?.into(),
        goal: text("goal")?.into(),
        version: RecordDigest::parse_hex(text("version")?).map_err(|_| unavailable())?,
        limits: Limits {
            lanes: number("lanes")?,
            concurrency: number("concurrency")?,
            depth: number("depth")?,
            retries: number("retries")?,
        },
    };
    request.validate()?;
    Ok((project, request))
}
