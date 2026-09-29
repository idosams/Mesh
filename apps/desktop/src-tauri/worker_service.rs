//! Explicit signed-application resident worker. Configuration is local native input, never IPC.
use mesh_crypto::{KeyCustody as _, SigningPayload};
use mesh_daemon::fleet::{
    self,
    provider::{ClaudeAdapter, CodexAdapter, NativeAdapter},
    *,
};
use mesh_daemon::{
    ipc::{Json, Unavailable},
    CheckpointSigner, ProtectedWorkspaceRoot,
};
use mesh_keychain::AppleActorCustody;
use mesh_types::{PublicKey, Signature};
use std::io::{self, Read, Write};
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::time::Duration;
const UNAVAILABLE: &str = "The resident worker is unavailable or requires reconciliation";
const BUDGET: Duration = Duration::from_secs(60);
struct Configuration {
    installation: PathBuf,
    endpoint: PathBuf,
    store: PathBuf,
    allocations: PathBuf,
    provider: String,
    executable: PathBuf,
    coordinator: PublicKey,
    capacity: usize,
    protected: Vec<ProtectedWorkspaceRoot>,
}
fn config(value: Json) -> Result<Configuration, String> {
    let fields = [
        "schema",
        "installation",
        "endpoint",
        "store",
        "allocations",
        "provider",
        "executable",
        "coordinator",
        "capacity",
        "protected",
    ];
    let Json::Object(pairs) = &value else {
        return Err(UNAVAILABLE.into());
    };
    if pairs.len() != fields.len()
        || fields
            .iter()
            .any(|name| pairs.iter().filter(|(key, _)| key == name).count() != 1)
    {
        return Err(UNAVAILABLE.into());
    }
    let text = |name: &str| {
        value
            .get(name)
            .and_then(Json::as_text)
            .ok_or_else(|| UNAVAILABLE.to_owned())
    };
    let path = |name: &str| -> Result<PathBuf, String> {
        let path = PathBuf::from(text(name)?);
        if !path.is_absolute() {
            return Err(UNAVAILABLE.into());
        }
        Ok(path)
    };
    if text("schema")? != "mesh.worker-config/v1" {
        return Err(UNAVAILABLE.into());
    }
    let provider = text("provider")?.to_owned();
    if !matches!(provider.as_str(), "codex" | "claude") {
        return Err(UNAVAILABLE.into());
    }
    let key = text("coordinator")?;
    if key.len() != 64
        || !key
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(UNAVAILABLE.into());
    }
    let mut coordinator = [0; 32];
    for (index, byte) in coordinator.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&key[index * 2..index * 2 + 2], 16).map_err(|_| UNAVAILABLE)?;
    }
    let Some(Json::Number(capacity @ 1..=64)) = value.get("capacity") else {
        return Err(UNAVAILABLE.into());
    };
    let Some(Json::Array(protected)) = value.get("protected") else {
        return Err(UNAVAILABLE.into());
    };
    if protected.len() > 64 {
        return Err(UNAVAILABLE.into());
    }
    let protected = protected
        .iter()
        .map(|item| {
            let path = PathBuf::from(item.as_text().ok_or(UNAVAILABLE)?);
            if !path.is_absolute() {
                return Err(UNAVAILABLE);
            }
            ProtectedWorkspaceRoot::inspect(&path).map_err(|_| UNAVAILABLE)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Configuration {
        installation: path("installation")?,
        endpoint: path("endpoint")?,
        store: path("store")?,
        allocations: path("allocations")?,
        executable: path("executable")?,
        provider,
        coordinator: PublicKey::from_bytes(coordinator),
        capacity: *capacity as usize,
        protected,
    })
}
#[allow(unsafe_code)]
fn load(path: &Path) -> Result<Configuration, String> {
    unsafe extern "C" {
        fn geteuid() -> u32;
    }
    // SAFETY: no arguments, pointers or side effects.
    let uid = unsafe { geteuid() };
    let parent = path.parent().ok_or(UNAVAILABLE)?;
    let metadata = std::fs::symlink_metadata(parent).map_err(|_| UNAVAILABLE)?;
    if !path.is_absolute()
        || !metadata.is_dir()
        || metadata.uid() != uid
        || metadata.mode() & 0o077 != 0
    {
        return Err(UNAVAILABLE.into());
    }
    let token = ProtectedWorkspaceRoot::inspect(parent).map_err(|_| UNAVAILABLE)?;
    let stable = token
        .stable_reference()
        .map_err(|_| UNAVAILABLE)?
        .join(path.file_name().ok_or(UNAVAILABLE)?);
    // Darwin O_NOFOLLOW | O_NONBLOCK; classify before reading to reject special files.
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(0x100 | 0x4)
        .open(stable)
        .map_err(|_| UNAVAILABLE)?;
    let metadata = file.metadata().map_err(|_| UNAVAILABLE)?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != uid
        || metadata.mode() & 0o077 != 0
        || metadata.len() > 16_384
    {
        return Err(UNAVAILABLE.into());
    }
    let mut bytes = Vec::new();
    file.take(16_385)
        .read_to_end(&mut bytes)
        .map_err(|_| UNAVAILABLE)?;
    if bytes.len() > 16_384
        || ProtectedWorkspaceRoot::inspect(parent).map_err(|_| UNAVAILABLE)? != token
    {
        return Err(UNAVAILABLE.into());
    }
    config(
        Json::parse(std::str::from_utf8(&bytes).map_err(|_| UNAVAILABLE)?)
            .map_err(|_| UNAVAILABLE)?,
    )
}
struct Actor(Arc<AppleActorCustody>);
impl CheckpointSigner for Actor {
    fn public_key(&self) -> PublicKey {
        self.0.public_key().public_key()
    }
    fn sign(&self, payload: &SigningPayload) -> Result<Signature, String> {
        self.0.sign(payload).map_err(|_| UNAVAILABLE.into())
    }
}
struct Signers(Arc<AppleActorCustody>);
impl fleet::host::WorkerSignerFactory for Signers {
    fn signer(&self, _: &str, _: &str) -> Result<Arc<dyn CheckpointSigner>, Unavailable> {
        Ok(Arc::new(Actor(self.0.clone())))
    }
}
pub(crate) fn serve(path: &Path) -> Result<(), String> {
    let config = load(path)?;
    let token =
        |path: &Path| ProtectedWorkspaceRoot::inspect(path).map_err(|_| UNAVAILABLE.to_owned());
    let (installation_id, endpoint_id, store_id, allocation_id) = (
        token(&config.installation)?,
        token(&config.endpoint)?,
        token(&config.store)?,
        token(&config.allocations)?,
    );
    let mut installation_protected = config.protected.clone();
    installation_protected.extend([endpoint_id, store_id, allocation_id]);
    let (installation, custody) = NativeWorkerInstallation::reopen(
        &config.installation,
        installation_id,
        &installation_protected,
        |account, key| {
            AppleActorCustody::open(account, key).map_err(|_| io::Error::other(UNAVAILABLE))
        },
    )
    .map_err(|_| UNAVAILABLE)?;
    let custody = Arc::new(custody);
    let mut destination_protected = config.protected.clone();
    destination_protected.extend([installation_id, endpoint_id]);
    let destination = RemoteInputDestination::admit(
        &config.store,
        store_id,
        &config.allocations,
        allocation_id,
        &destination_protected,
    )
    .map_err(|_| UNAVAILABLE)?;
    let desktop = std::env::current_exe().map_err(|_| UNAVAILABLE)?;
    let adapter: NativeAdapter = match config.provider.as_str() {
        "codex" => CodexAdapter::with_desktop_bridge(&config.executable, &desktop)
            .map_err(|_| UNAVAILABLE)?
            .into(),
        "claude" => ClaudeAdapter::with_desktop_bridge(&config.executable, &desktop)
            .map_err(|_| UNAVAILABLE)?
            .into(),
        _ => return Err(UNAVAILABLE.into()),
    };
    let key = installation.identity().map_err(|_| UNAVAILABLE)?.worker();
    let policy = RemoteDispatchPolicy {
        coordinator: config.coordinator,
        worker: key,
        provider: adapter.provider(),
        maximum: Limits {
            lanes: 64,
            concurrency: config.capacity as u64,
            depth: 8,
            retries: 8,
        },
        max_lease_ms: 3_600_000,
    };
    let mut connections =
        NativeWorkerConnections::new(&installation, &destination, policy, config.capacity)
            .map_err(|_| UNAVAILABLE)?;
    let mut endpoint_protected = config.protected.clone();
    endpoint_protected.extend([installation_id, store_id, allocation_id]);
    let endpoint = NativeWorkerEndpoint::bind(&config.endpoint, endpoint_id, &endpoint_protected)
        .map_err(|_| UNAVAILABLE)?;
    let endpoints = endpoint_id.stable_reference().map_err(|_| UNAVAILABLE)?;
    let mut supervisor =
        ReceivedWorkerSupervisor::new(config.capacity, key).map_err(|_| UNAVAILABLE)?;
    let (sender, mailbox) = ReceivedWorkerMailbox::bounded();
    let (observations, observed) = mpsc::sync_channel(1);
    let stop = AtomicBool::new(false);
    let signers = Arc::new(Signers(custody.clone()));
    let publication_signer = Actor(custody.clone());
    std::thread::scope(|scope| {
        let stop_ref = &stop;
        let supervisor_ref = &mut supervisor;
        let observer = std::thread::Builder::new()
            .name("mesh-worker-owner".into())
            .spawn_scoped(scope, move || {
                supervisor_ref.serve_with_result_publication(
                    &mailbox,
                    stop_ref,
                    &observations,
                    &publication_signer,
                )
            })
            .map_err(|_| UNAVAILABLE)?;
        let result = (|| -> Result<(), String> {
            loop {
                if observer.is_finished() {
                    return Err(UNAVAILABLE.into());
                }
                installation.verify().map_err(|_| UNAVAILABLE)?;
                endpoint.verify().map_err(|_| UNAVAILABLE)?;
                while observed.try_recv().is_ok() {}
                connections
                    .flush_pending(&sender)
                    .map_err(|_| UNAVAILABLE)?;
                let Some(stream) = endpoint.accept(BUDGET).map_err(|_| UNAVAILABLE)? else {
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                };
                let input = stream.try_clone().map_err(|_| UNAVAILABLE)?;
                // A refused/lost connection leaves its original state held by `connections`.
                let outcome = connections.serve(input, stream, |payload| {
                    custody.sign(payload).map_err(|_| UNAVAILABLE.into())
                });
                if let Ok(WorkerConnectionOutcome::Materialized { admission, .. }) = outcome {
                    let (reply, _response) = mpsc::sync_channel(1);
                    let launch = ReceivedWorkerLaunch {
                        adapter: adapter.clone(),
                        endpoint: endpoints.join(format!("p-{}.sock", admission.allocation())),
                        signers: signers.clone(),
                        reviewers: mesh_daemon::TrustedReviewers::default(),
                        checkpoint: mesh_daemon::CheckpointRuntimeParameters::selected_defaults(),
                    };
                    connections
                        .queue_received(&admission, launch, reply, &sender)
                        .map_err(|_| UNAVAILABLE)?;
                }
            }
        })();
        stop.store(true, Ordering::Release);
        observer.join().map_err(|_| UNAVAILABLE)?;
        result
    })
}
pub(crate) fn connect(root: &Path) -> Result<(), String> {
    let expected = ProtectedWorkspaceRoot::inspect(root).map_err(|_| UNAVAILABLE)?;
    let mut stream =
        NativeWorkerEndpoint::connect(root, expected, BUDGET).map_err(|_| UNAVAILABLE)?;
    let mut incoming = stream.try_clone().map_err(|_| UNAVAILABLE)?;
    // The explicit bridge process exits after output EOF/error. A blocked stdin copier does not own
    // the resident service or its shutdown flag. Socket I/O retains the fixed connection deadline.
    std::thread::spawn(move || {
        let _ = io::copy(&mut io::stdin().lock(), &mut incoming);
        let _ = incoming.shutdown_write();
    });
    io::copy(&mut stream, &mut io::stdout().lock()).map_err(|_| UNAVAILABLE)?;
    io::stdout().flush().map_err(|_| UNAVAILABLE.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn value() -> Json {
        Json::object([
            ("schema", Json::text("mesh.worker-config/v1")),
            ("installation", Json::text("/private/tmp/installation")),
            ("endpoint", Json::text("/private/tmp/endpoint")),
            ("store", Json::text("/private/tmp/store")),
            ("allocations", Json::text("/private/tmp/allocations")),
            ("provider", Json::text("codex")),
            ("executable", Json::text("/usr/bin/true")),
            ("coordinator", Json::text("ab".repeat(32))),
            ("capacity", Json::Number(4)),
            ("protected", Json::Array(vec![])),
        ])
    }
    #[test]
    fn worker_config_is_closed_bounded_and_requires_native_paths() {
        assert_eq!(config(value()).unwrap().capacity, 4);
        for (field, replacement) in [
            ("capacity", Json::Number(0)),
            ("capacity", Json::Number(65)),
            ("provider", Json::text("unadmitted")),
            ("endpoint", Json::text("relative")),
            ("coordinator", Json::text("AB".repeat(32))),
            ("schema", Json::text("mesh.worker-config/v2")),
        ] {
            let Json::Object(mut fields) = value() else {
                unreachable!()
            };
            fields.iter_mut().find(|(name, _)| name == field).unwrap().1 = replacement;
            assert!(config(Json::Object(fields)).is_err());
        }
        let Json::Object(mut fields) = value() else {
            unreachable!()
        };
        fields.push(("command".into(), Json::text("arbitrary")));
        assert!(config(Json::Object(fields)).is_err());
        let Json::Object(mut fields) = value() else {
            unreachable!()
        };
        fields[0] = ("capacity".into(), Json::Number(1));
        assert!(config(Json::Object(fields)).is_err());
    }
    #[test]
    fn configuration_file_must_be_private_regular_bounded_and_not_a_symlink() {
        use std::os::unix::fs::{symlink, PermissionsExt as _};
        let root =
            std::env::temp_dir().join(format!("mesh-worker-config-test-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = root.join("worker.json");
        std::fs::write(&path, value().encode()).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(load(&path).unwrap().capacity, 4);
        let link = root.join("linked.json");
        symlink(&path, &link).unwrap();
        assert!(load(&link).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(load(&path).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::write(&path, vec![b' '; 16_385]).unwrap();
        assert!(load(&path).is_err());
        assert!(load(&root).is_err());
    }
}
