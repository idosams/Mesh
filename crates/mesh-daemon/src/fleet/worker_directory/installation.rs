//! Create-only native installation binding, outside the ledger's closed namespace.
use super::*;
use mesh_types::PublicKey;
use std::ffi::OsStr;

const INTENT: &str = "intent.json";
const IDENTITY: &str = "identity.json";
const LEDGER: &str = "ledger";

/// Public identity facts from a natively verified installation, not execution authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkerInstallationIdentity {
    installation: [u8; 16],
    worker: PublicKey,
}
impl WorkerInstallationIdentity {
    /// Native-generated custody account identifier; never accepted from a network request.
    pub fn installation(&self) -> [u8; 16] {
        self.installation
    }
    /// Public execution identity, never a human approval credential.
    pub fn worker(&self) -> PublicKey {
        self.worker
    }
}
fn account(id: [u8; 16]) -> String {
    id.iter().map(|b| format!("{b:02x}")).collect()
}
fn decode_account(text: &str) -> io::Result<[u8; 16]> {
    if text.len() != 32
        || !text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(unavailable());
    }
    let mut id = [0; 16];
    for (i, byte) in id.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).map_err(|_| unavailable())?;
    }
    Ok(id)
}
fn directory_identity(root: &PinnedWorkspaceRoot, uid: u32) -> io::Result<String> {
    root.ensure_namespace_identity()?;
    let m = root.try_clone_directory()?.metadata()?;
    if !m.is_dir() || m.uid() != uid || m.permissions().mode() & 0o077 != 0 {
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
fn directory_token(root: &PinnedWorkspaceRoot) -> io::Result<ProtectedWorkspaceRoot> {
    let (device, inode) = root.identity()?;
    ProtectedWorkspaceRoot::from_directory_token(&format!("{device:016x}:{inode:016x}"))
}
fn read(owner: &Owner, name: &str) -> io::Result<String> {
    let fs = owner.root.filesystem();
    let file = fs.read_only().read_file(Path::new(name))?;
    file_identity(&file, owner.uid)?;
    let mut bytes = Vec::new();
    file.take(MAX_RECEIPT + 1).read_to_end(&mut bytes)?;
    if bytes.len() > MAX_RECEIPT as usize {
        return Err(unavailable());
    }
    String::from_utf8(bytes).map_err(|_| unavailable())
}
fn intent(owner: &Owner, id: [u8; 16]) -> io::Result<String> {
    Ok(Json::object([
        ("schema", Json::text("mesh.worker-provision-intent/v1")),
        (
            "root",
            Json::text(directory_identity(&owner.root, owner.uid)?),
        ),
        ("installation", Json::text(account(id))),
    ])
    .encode())
}
fn identity(
    owner: &Owner,
    ledger: &PinnedWorkspaceRoot,
    value: WorkerInstallationIdentity,
) -> io::Result<String> {
    Ok(Json::object([
        ("schema", Json::text("mesh.worker-installation/v1")),
        (
            "root",
            Json::text(directory_identity(&owner.root, owner.uid)?),
        ),
        ("installation", Json::text(account(value.installation))),
        (
            "worker",
            Json::text(RecordDigest::from_bytes(*value.worker.as_bytes()).to_string()),
        ),
        ("ledger", Json::text(directory_identity(ledger, owner.uid)?)),
    ])
    .encode())
}
#[derive(Debug)]
struct InstallationAuthority {
    owner: Arc<Owner>,
    ledger: PinnedWorkspaceRoot,
    ledger_identity: String,
    intent: File,
    identity: File,
    intent_encoded: String,
    identity_encoded: String,
}
impl InstallationAuthority {
    fn verify(&self) -> io::Result<()> {
        self.owner.verify()?;
        let fs = self.owner.root.filesystem();
        let names = fs.read_directory_names_bounded(Path::new(""), 3)?;
        if names.len() != 3
            || names.iter().any(|name| {
                ![INTENT, IDENTITY, LEDGER]
                    .iter()
                    .any(|n| name == OsStr::new(n))
            })
        {
            return Err(unavailable());
        }
        for (name, retained, encoded) in [
            (INTENT, &self.intent, &self.intent_encoded),
            (IDENTITY, &self.identity, &self.identity_encoded),
        ] {
            if file_identity(&fs.inspect_entry(Path::new(name))?, self.owner.uid)?
                != file_identity(retained, self.owner.uid)?
                || read(&self.owner, name)? != *encoded
            {
                return Err(unavailable());
            }
        }
        if directory_identity(&self.ledger, self.owner.uid)? != self.ledger_identity {
            return Err(unavailable());
        }
        self.owner.verify()
    }
}
impl FleetStoreAuthority for InstallationAuthority {
    fn check(&self) -> Result<(), FleetStoreError> {
        self.verify().map_err(|_| FleetStoreError::AuthorityChanged)
    }
}

/// Owns one private installation and its ledger. Retained registries keep both directory locks and
/// revalidate the parent identity before/after every store operation, even after this wrapper drops.
/// No process is adopted or launched. Partial initialization is retained and never repaired here.
pub struct NativeWorkerInstallation {
    directory: NativeRemoteWorkerDirectory,
    identity: WorkerInstallationIdentity,
}
impl NativeWorkerInstallation {
    /// Provision an existing empty private directory. Persist native random identity intent before
    /// calling the native custody creator. Any failure retains created files/key evidence; retrying
    /// the same directory refuses before another custody call. The callback returns only public
    /// identity plus its opaque native handle; this module never receives secret material.
    pub fn provision<T>(
        path: &Path,
        expected: ProtectedWorkspaceRoot,
        protected: &[ProtectedWorkspaceRoot],
        create: impl FnOnce([u8; 16]) -> io::Result<(PublicKey, T)>,
    ) -> io::Result<(Self, T)> {
        let owner = owner(path, expected, protected, None)?;
        let fs = owner.root.filesystem();
        if !fs
            .read_directory_names_bounded(Path::new(""), 1)?
            .is_empty()
        {
            return Err(unavailable());
        }
        let mut installation = [0; 16];
        File::open("/dev/urandom")?.read_exact(&mut installation)?;
        let intent_encoded = intent(&owner, installation)?;
        fs.write_new_file(
            Path::new(INTENT),
            intent_encoded.as_bytes(),
            Permissions::from_mode(0o600),
        )?;
        let original_intent = fs.inspect_entry(Path::new(INTENT))?;
        owner.root.sync()?;
        owner.verify()?;
        let (worker, custody) = create(installation)?;
        owner.verify()?;
        if fs.read_directory_names_bounded(Path::new(""), 1)?
            != vec![std::ffi::OsString::from(INTENT)]
            || file_identity(&fs.inspect_entry(Path::new(INTENT))?, owner.uid)?
                != file_identity(&original_intent, owner.uid)?
            || read(&owner, INTENT)? != intent_encoded
        {
            return Err(unavailable());
        }
        let value = WorkerInstallationIdentity {
            installation,
            worker,
        };
        let ledger = owner.root.create_child_directory(OsStr::new(LEDGER))?;
        let encoded = identity(&owner, &ledger, value)?;
        fs.write_new_file(
            Path::new(IDENTITY),
            encoded.as_bytes(),
            Permissions::from_mode(0o600),
        )?;
        owner.root.sync()?;
        let result = Self::finish(owner, ledger, value, true)?;
        Ok((result, custody))
    }

    /// Reopen complete native state and then load the exact expected custody identity. Neither the
    /// ledger nor the callback may create a replacement. Corrupt/partial state refuses first.
    pub fn reopen<T>(
        path: &Path,
        expected: ProtectedWorkspaceRoot,
        protected: &[ProtectedWorkspaceRoot],
        load: impl FnOnce([u8; 16], PublicKey) -> io::Result<T>,
    ) -> io::Result<(Self, T)> {
        let owner = owner(path, expected, protected, None)?;
        let encoded = read(&owner, IDENTITY)?;
        let json = Json::parse(&encoded).map_err(|_| unavailable())?;
        let installation = decode_account(
            json.get("installation")
                .and_then(Json::as_text)
                .ok_or_else(unavailable)?,
        )?;
        let worker = RecordDigest::parse_hex(
            json.get("worker")
                .and_then(Json::as_text)
                .ok_or_else(unavailable)?,
        )
        .map_err(|_| unavailable())?;
        let value = WorkerInstallationIdentity {
            installation,
            worker: PublicKey::from_bytes(*worker.as_bytes()),
        };
        let ledger = owner.root.open_child_directory(OsStr::new(LEDGER))?;
        let result = Self::finish(owner, ledger, value, false)?;
        let custody = load(installation, value.worker)?;
        result.verify()?;
        Ok((result, custody))
    }
    fn finish(
        owner: Arc<Owner>,
        ledger: PinnedWorkspaceRoot,
        value: WorkerInstallationIdentity,
        initialize: bool,
    ) -> io::Result<Self> {
        let fs = owner.root.filesystem();
        let ledger_token = directory_token(&ledger)?;
        let authority = Arc::new(InstallationAuthority {
            intent_encoded: intent(&owner, value.installation)?,
            identity_encoded: identity(&owner, &ledger, value)?,
            ledger_identity: directory_identity(&ledger, owner.uid)?,
            intent: fs.inspect_entry(Path::new(INTENT))?,
            identity: fs.inspect_entry(Path::new(IDENTITY))?,
            owner,
            ledger,
        });
        authority.verify()?;
        let directory = NativeRemoteWorkerDirectory::open_inner(
            &ledger_token.stable_reference()?,
            ledger_token,
            &RecordDigest::from_bytes(*value.worker.as_bytes()).to_string(),
            &authority.owner.protected,
            initialize,
            Some(authority.clone()),
        )?;
        authority.verify()?;
        Ok(Self {
            directory,
            identity: value,
        })
    }
    /// Revalidate all native installation facts, including before/after custody use by an embedder.
    pub fn verify(&self) -> io::Result<()> {
        self.directory.authority.verify()
    }
    /// Exact native public identity after current physical/receipt verification.
    pub fn identity(&self) -> io::Result<WorkerInstallationIdentity> {
        self.verify()?;
        Ok(self.identity)
    }
    /// Native objective view; its retained store checks include this installation's authority.
    pub fn registry(
        &self,
        coordinator: &str,
        objective: &str,
        limits: Limits,
    ) -> io::Result<RemoteAdmissionRegistry> {
        self.verify()?;
        self.directory.registry(coordinator, objective, limits)
    }
}

#[cfg(test)]
mod tests;
