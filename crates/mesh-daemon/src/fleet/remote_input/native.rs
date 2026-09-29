//! Native CAS receipt using pinned directory authority and a single receiving owner.
use super::*;
use crate::fleet::{RemoteInputAllocation, RemoteInputDestination};
use crate::root_authority::PinnedWorkspaceRoot;
use std::fs::File;
use std::io::{self, Read as _};
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

/// Serial native input receipt. Directory ownership lasts until this handle is dropped.
/// Authentication and current assignment/dependency authorization remain caller responsibilities.
/// Nothing here launches a provider, adopts an attempt or approves protected main.
pub struct NativeRemoteInputReceiver<'a> {
    manifest: RemoteInputManifest,
    assignment: RemoteAssignment,
    cas: Cas<ReceivingFs<'a>>,
    destination: &'a RemoteInputDestination,
    _lock: File,
}
impl<'a> NativeRemoteInputReceiver<'a> {
    /// Acquire exclusive receiving ownership of an already admitted private store.
    /// Existing CAS content/partial offsets are retained. Another receiver refuses immediately.
    pub fn new(
        destination: &'a RemoteInputDestination,
        manifest: RemoteInputManifest,
        assignment: &RemoteAssignment,
    ) -> Result<Self, Error> {
        assignment.validate()?;
        if manifest.input() != assignment.input || manifest.bundle() != assignment.bundle {
            return refuse("remote-input-assignment-mismatch");
        }
        let (root, path) = destination.receiving_store().map_err(store_error)?;
        let lock = lock(&root).map_err(|_| Error::Refused("remote-input-store-busy"))?;
        destination.verify().map_err(store_error)?;
        let cas = Cas::with_filesystem(path, ReceivingFs { root, destination })
            .map_err(|_| Error::Refused("remote-input-store"))?;
        destination.verify().map_err(store_error)?;
        Ok(Self {
            manifest,
            assignment: assignment.clone(),
            cas,
            destination,
            _lock: lock,
        })
    }
    fn receiver(&self) -> RemoteInputReceiver<'_, ReceivingFs<'a>> {
        RemoteInputReceiver {
            manifest: Cow::Borrowed(&self.manifest),
            cas: &self.cas,
        }
    }
    /// Read the durable confirmed offset through the retained private directory.
    pub fn status(&mut self, digest: Digest32) -> Result<(u64, bool), Error> {
        self.destination.verify().map_err(store_error)?;
        let status = self.receiver().status(digest)?;
        self.destination.verify().map_err(store_error)?;
        Ok(status)
    }
    /// Accept a declared contiguous bounded part. Acknowledgment follows native identity rechecks.
    pub fn accept(
        &mut self,
        digest: Digest32,
        offset: u64,
        bytes: &[u8],
        final_part: bool,
    ) -> Result<(), Error> {
        self.destination.verify().map_err(store_error)?;
        self.receiver().accept(digest, offset, bytes, final_part)?;
        self.destination.verify().map_err(store_error)
    }
    /// Verify complete file hashes using bounded pinned reads; this is not launch readiness.
    pub fn verify_complete(&mut self) -> Result<(), Error> {
        self.destination.verify().map_err(store_error)?;
        self.receiver().verify_complete()?;
        self.destination.verify().map_err(store_error)
    }
    /// Create and verify a fresh independent working folder while retaining receiving ownership.
    pub fn materialize(&mut self, allocation_id: &str) -> io::Result<RemoteInputAllocation> {
        self.destination.verify()?;
        self.receiver().materialize(self.destination, allocation_id)
    }
    /// Consume the original durable admission reservation. Replay cannot construct this value.
    /// Failure consumes it too: retained partial work requires reconciliation, never reallocation.
    /// This verifies the exact assigned input and native destination, but does not start a provider.
    pub fn materialize_reserved(
        &mut self,
        reservation: crate::fleet::RemoteInputReservation,
    ) -> io::Result<RemoteInputAllocation> {
        let admission = reservation
            .consume(&self.assignment)
            .map_err(|_| io::Error::other("remote input reservation does not match assignment"))?;
        let mut allocation = self.materialize(admission.allocation())?;
        allocation.admission = Some(admission);
        Ok(allocation)
    }
}
fn store_error(_: io::Error) -> Error {
    Error::Refused("remote-input-store-changed")
}

#[allow(unsafe_code)]
fn lock(root: &PinnedWorkspaceRoot) -> io::Result<File> {
    use std::os::fd::AsRawFd as _;
    unsafe extern "C" {
        fn flock(fd: i32, operation: i32) -> i32;
    }
    let file = root.independent_lock_directory()?;
    // SAFETY: file owns a live independently opened directory descriptor. LOCK_EX|LOCK_NB
    // excludes another cooperating native receiver without waiting or sharing clone lock ownership.
    if unsafe { flock(file.as_raw_fd(), 2 | 4) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(file)
}

struct ReceivingFs<'a> {
    root: PinnedWorkspaceRoot,
    destination: &'a RemoteInputDestination,
}
impl ReceivingFs<'_> {
    fn checked<T>(
        &self,
        operation: impl FnOnce(&crate::root_authority::PinnedRootFs) -> io::Result<T>,
    ) -> io::Result<T> {
        self.destination.verify()?;
        let result = operation(&self.root.filesystem())?;
        self.destination.verify()?;
        Ok(result)
    }
    fn regular(&self, path: &Path) -> io::Result<File> {
        let file = self.root.filesystem().read_only().read_file(path)?;
        let metadata = file.metadata()?;
        if metadata.nlink() != 1 || metadata.len() > MAX_CHUNK_BYTES {
            return Err(io::Error::other(
                "receiving object is linked or exceeds its bound",
            ));
        }
        Ok(file)
    }
}
impl DurableFs for ReceivingFs<'_> {
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.checked(|fs| fs.create_dir_all(path))
    }
    fn stage(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.checked(|fs| fs.stage(path, bytes))
    }
    fn append(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.checked(|fs| fs.append_private_regular(path, bytes))
    }
    fn sync_file(&self, path: &Path) -> io::Result<()> {
        self.checked(|fs| fs.sync_file(path))
    }
    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        self.checked(|fs| fs.sync_dir(path))
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.checked(|fs| fs.rename(from, to))
    }
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.checked(|_| {
            let file = self.regular(path)?;
            let mut bytes = Vec::new();
            file.take(MAX_CHUNK_BYTES + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 > MAX_CHUNK_BYTES {
                return Err(io::Error::other("receiving object grew beyond its bound"));
            }
            Ok(bytes)
        })
    }
    fn exists(&self, path: &Path) -> bool {
        // Unknown or unsupported is conservatively present, never permission to overwrite it.
        !matches!(self.checked(|fs| fs.clone().read_only().read_file(path)), Err(error) if error.kind() == io::ErrorKind::NotFound)
    }
    fn file_len(&self, path: &Path) -> io::Result<u64> {
        self.checked(|_| Ok(self.regular(path)?.metadata()?.len()))
    }
    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.checked(|fs| fs.remove_file(path))
    }
    fn list_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        self.checked(|fs| fs.list_dir(path))
    }
}

#[cfg(target_os = "macos")]
mod result;
#[cfg(target_os = "macos")]
pub use result::{NativeRemoteResultReceiver, RemoteResultContentReceipt};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProtectedWorkspaceRoot;
    use std::fs;
    use std::os::unix::fs::{symlink, PermissionsExt as _};
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "mesh-native-receipt-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            for name in [
                "wrapper",
                "wrapper/store",
                "wrapper/allocations",
                "protected",
            ] {
                fs::create_dir(root.join(name)).unwrap();
                fs::set_permissions(root.join(name), fs::Permissions::from_mode(0o700)).unwrap();
            }
            Self(root)
        }
        fn store(&self) -> PathBuf {
            self.0.join("wrapper/store")
        }
        fn destination(&self) -> RemoteInputDestination {
            let store = self.store();
            let parent = self.0.join("wrapper/allocations");
            RemoteInputDestination::admit(
                &store,
                ProtectedWorkspaceRoot::inspect(&store).unwrap(),
                &parent,
                ProtectedWorkspaceRoot::inspect(&parent).unwrap(),
                &[ProtectedWorkspaceRoot::inspect(&self.0.join("protected")).unwrap()],
            )
            .unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn manifest() -> RemoteInputManifest {
        RemoteInputManifest::new(
            RecordDigest::from_bytes([1; 32]),
            vec![
                RemoteInputEntry::Directory {
                    path: "empty".into(),
                },
                RemoteInputEntry::File {
                    path: "note".into(),
                    executable: false,
                    digest: Blake3::digest_bytes(b"hello"),
                    chunks: vec![RemoteInputChunk {
                        digest: Blake3::digest_bytes(b"hello"),
                        bytes: 5,
                    }],
                },
            ],
        )
        .unwrap()
    }
    fn assignment(manifest: &RemoteInputManifest) -> RemoteAssignment {
        RemoteAssignment {
            id: "receipt".into(),
            worker_key: "ab".repeat(32),
            input: manifest.input(),
            bundle: manifest.bundle(),
            lease_sequence: 1,
            lease_until_ms: 1000,
        }
    }
    fn open(destination: &RemoteInputDestination) -> NativeRemoteInputReceiver<'_> {
        let manifest = manifest();
        let assignment = assignment(&manifest);
        NativeRemoteInputReceiver::new(destination, manifest, &assignment).unwrap()
    }
    #[test]
    fn pinned_receipt_resumes_with_one_owner_then_materializes_exact_content() {
        let fixture = Fixture::new();
        let destination = fixture.destination();
        let digest = Blake3::digest_bytes(b"hello");
        let mut first = open(&destination);
        let other = fixture.destination();
        let manifest = manifest();
        assert!(
            NativeRemoteInputReceiver::new(&other, manifest.clone(), &assignment(&manifest))
                .is_err()
        );
        first.accept(digest, 0, b"he", false).unwrap();
        assert_eq!(first.status(digest).unwrap(), (2, false));
        assert!(first.verify_complete().is_err());
        drop(first);
        let mut reopened = open(&destination);
        assert_eq!(reopened.status(digest).unwrap(), (2, false));
        reopened.accept(digest, 2, b"llo", true).unwrap();
        assert_eq!(reopened.status(digest).unwrap(), (5, true));
        reopened.verify_complete().unwrap();
        let allocation = reopened
            .materialize("0123456789abcdef0123456789abcdef")
            .unwrap();
        assert_eq!(fs::read(allocation.path().join("note")).unwrap(), b"hello");
        assert!(allocation.path().join("empty").is_dir());
        allocation.verify().unwrap();
    }

    #[test]
    fn durable_reservation_materializes_once_and_failure_never_regrants() {
        use crate::fleet::{Limits, RemoteAdmissionOutcome, RemoteAdmissionRegistry, RemoteWork};
        use mesh_store::fleet::FleetStore;
        let fixture = Fixture::new();
        let mut registry = RemoteAdmissionRegistry::new(
            FleetStore::open(fixture.0.join("worker.sqlite")).unwrap(),
            &"cd".repeat(32),
            &"ab".repeat(32),
            "objective",
            Limits {
                lanes: 2,
                concurrency: 2,
                depth: 0,
                retries: 0,
            },
        )
        .unwrap();
        let work = RemoteWork {
            lane: "lane".into(),
            run: "run".into(),
            assignment: assignment(&manifest()),
            provider: "codex".into(),
            goal: "Task".into(),
        };
        let id = "0123456789abcdef0123456789abcdef";
        let destination = fixture.destination();
        let mut receiver = open(&destination);
        let RemoteAdmissionOutcome::Reserved(reservation) =
            registry.reserve(work.clone(), id, 100).unwrap()
        else {
            panic!("new admission")
        };
        receiver
            .accept(Blake3::digest_bytes(b"hello"), 0, b"hello", true)
            .unwrap();
        let allocation = receiver.materialize_reserved(reservation).unwrap();
        assert_eq!(fs::read(allocation.path().join("note")).unwrap(), b"hello");
        assert!(matches!(
            registry.reserve(work.clone(), id, 100),
            Ok(RemoteAdmissionOutcome::Retained(_))
        ));

        let mut changed = work;
        changed.assignment.id = "second".into();
        changed.lane = "second-lane".into();
        changed.run = "second-run".into();
        let second_id = "a".repeat(32);
        let RemoteAdmissionOutcome::Reserved(reservation) =
            registry.reserve(changed.clone(), &second_id, 100).unwrap()
        else {
            panic!("second admission")
        };
        // Matching input bytes alone are insufficient: the full attempt must also match.
        assert!(receiver.materialize_reserved(reservation).is_err());
        assert!(!fixture
            .0
            .join("wrapper/allocations")
            .join(format!("input-{second_id}"))
            .exists());
        assert!(matches!(
            registry.reserve(changed, &second_id, 100),
            Ok(RemoteAdmissionOutcome::Retained(_))
        ));
        allocation.verify().unwrap();
    }
    #[test]
    fn replaced_store_refuses_receipt_and_never_writes_into_replacement() {
        let fixture = Fixture::new();
        let destination = fixture.destination();
        let mut receiver = open(&destination);
        let digest = Blake3::digest_bytes(b"hello");
        receiver.accept(digest, 0, b"he", false).unwrap();
        let old = fixture.0.join("old-store");
        fs::rename(fixture.store(), &old).unwrap();
        fs::create_dir(fixture.store()).unwrap();
        fs::set_permissions(fixture.store(), fs::Permissions::from_mode(0o700)).unwrap();
        assert!(receiver.accept(digest, 2, b"llo", true).is_err());
        assert!(receiver.status(digest).is_err());
        assert!(receiver.verify_complete().is_err());
        assert_eq!(fs::read_dir(fixture.store()).unwrap().count(), 0);
        assert_eq!(
            fs::read(mesh_cas::StoreLayout::new(old).incoming_path(&digest)).unwrap(),
            b"he"
        );
    }
    #[test]
    fn linked_and_oversized_partial_objects_refuse_without_mutating_external_files() {
        let fixture = Fixture::new();
        let destination = fixture.destination();
        let mut receiver = open(&destination);
        let digest = Blake3::digest_bytes(b"hello");
        receiver.accept(digest, 0, b"he", false).unwrap();
        let partial = mesh_cas::StoreLayout::new(fixture.store()).incoming_path(&digest);
        let external = fixture.0.join("external");
        fs::write(&external, b"he").unwrap();
        fs::remove_file(&partial).unwrap();
        symlink(&external, &partial).unwrap();
        assert!(receiver.status(digest).is_err());
        assert!(receiver.accept(digest, 2, b"llo", true).is_err());
        assert_eq!(fs::read(&external).unwrap(), b"he");
        fs::remove_file(&partial).unwrap();
        fs::hard_link(&external, &partial).unwrap();
        assert!(receiver.status(digest).is_err());
        assert!(receiver.accept(digest, 2, b"llo", true).is_err());
        assert_eq!(fs::read(&external).unwrap(), b"he");
        fs::remove_file(&partial).unwrap();
        let file = File::create(&partial).unwrap();
        file.set_len(MAX_CHUNK_BYTES + 1).unwrap();
        drop(file);
        assert!(receiver.status(digest).is_err());
        assert!(receiver.accept(digest, 2, b"llo", true).is_err());
        assert_eq!(fs::metadata(&partial).unwrap().len(), MAX_CHUNK_BYTES + 1);
    }
    #[test]
    fn moving_retained_storage_into_a_protected_project_revokes_receipt() {
        let fixture = Fixture::new();
        let destination = fixture.destination();
        let mut receiver = open(&destination);
        let digest = Blake3::digest_bytes(b"hello");
        receiver.accept(digest, 0, b"he", false).unwrap();
        let wrapper = fixture.0.join("wrapper");
        let moved = fixture.0.join("protected/moved");
        fs::rename(&wrapper, &moved).unwrap();
        symlink(&moved, &wrapper).unwrap();
        // Final store/parent object identities still agree through the ancestor alias.
        let root = receiver.cas.filesystem().root.clone();
        root.ensure_namespace_identity().unwrap();
        assert!(receiver.accept(digest, 2, b"llo", true).is_err());
        assert!(receiver.status(digest).is_err());
        assert_eq!(
            fs::read(mesh_cas::StoreLayout::new(moved.join("store")).incoming_path(&digest))
                .unwrap(),
            b"he"
        );
    }
}
