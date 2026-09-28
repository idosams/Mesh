//! Create-only receiving-side trees. Native admission, never peer paths, supplies storage authority.
use super::{RemoteInputEntry, RemoteInputManifest};
use crate::managed_file::retained_replacement::observe_file;
use crate::root_authority::PinnedWorkspaceRoot;
use crate::ProtectedWorkspaceRoot;
use mesh_cas::{Blake3, Cas, ContentDigest as _, DurableFs as _, StoreLayout};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Read as _, Write as _};
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

fn invalid() -> io::Error {
    io::Error::other("remote input storage or allocation is unavailable or changed")
}
fn token(root: &PinnedWorkspaceRoot) -> io::Result<ProtectedWorkspaceRoot> {
    let (device, inode) = root.identity()?;
    ProtectedWorkspaceRoot::from_directory_token(&format!("{device:016x}:{inode:016x}"))
}
fn private(root: &PinnedWorkspaceRoot) -> io::Result<()> {
    root.ensure_namespace_identity()?;
    if root.try_clone_directory()?.metadata()?.permissions().mode() & 0o077 != 0 {
        return Err(invalid());
    }
    Ok(())
}

/// Native-selected existing private storage. No renderer or peer can admit this capability.
/// The native owner supplies exact retained identities and all protected user-project roots.
/// Admission does not authenticate a peer, authorize a dependency, or exclude another receiver;
/// the native transport must establish those facts before receiving or materializing input.
pub struct RemoteInputDestination {
    store: PinnedWorkspaceRoot,
    store_path: PathBuf,
    parent: PinnedWorkspaceRoot,
    parent_path: PathBuf,
    protected: Vec<ProtectedWorkspaceRoot>,
}
impl RemoteInputDestination {
    /// Admit existing private directories against native-retained installation identities.
    /// Neither directory may overlap the other or reside in a protected user's working tree.
    pub fn admit(
        store: &Path,
        expected_store: ProtectedWorkspaceRoot,
        parent: &Path,
        expected_parent: ProtectedWorkspaceRoot,
        protected: &[ProtectedWorkspaceRoot],
    ) -> io::Result<Self> {
        if !store.is_absolute() || !parent.is_absolute() {
            return Err(invalid());
        }
        let store_pin = PinnedWorkspaceRoot::open(store.to_path_buf())?;
        let parent_pin = PinnedWorkspaceRoot::open(parent.to_path_buf())?;
        store_pin.ensure_protected_identity(expected_store)?;
        parent_pin.ensure_protected_identity(expected_parent)?;
        let admitted = Self {
            store: store_pin,
            store_path: store.to_path_buf(),
            parent: parent_pin,
            parent_path: parent.to_path_buf(),
            protected: protected.to_vec(),
        };
        admitted.verify()?;
        Ok(admitted)
    }
    pub(super) fn verify(&self) -> io::Result<()> {
        private(&self.store)?;
        private(&self.parent)?;
        if self.store.is_within(token(&self.parent)?)?
            || self.parent.is_within(token(&self.store)?)?
        {
            return Err(invalid());
        }
        for protected in &self.protected {
            if self.store.is_within(*protected)? || self.parent.is_within(*protected)? {
                return Err(invalid());
            }
        }
        Ok(())
    }
    pub(super) fn receiving_store(&self) -> io::Result<(PinnedWorkspaceRoot, PathBuf)> {
        self.verify()?;
        Ok((self.store.clone(), self.store_path.clone()))
    }

    pub(super) fn materialize<F: mesh_cas::DurableFs>(
        &self,
        manifest: &RemoteInputManifest,
        cas: &Cas<F>,
        allocation_id: &str,
    ) -> io::Result<RemoteInputAllocation> {
        self.verify()?;
        if cas.layout().root() != self.store_path
            || allocation_id.len() != 32
            || !allocation_id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid());
        }
        // Create-only. A failed/crashed attempt remains named and must be reconciled explicitly.
        let name = format!("input-{allocation_id}");
        let allocation = self.parent.create_child_directory(OsStr::new(&name))?;
        allocation.filesystem().write_new_file(
            Path::new("manifest.json"),
            manifest.encoded().as_bytes(),
            fs::Permissions::from_mode(0o600),
        )?;
        let files = allocation.create_child_directory(OsStr::new("files"))?;
        let result = RemoteInputAllocation {
            manifest: manifest.clone(),
            parent: self.parent.clone(),
            allocation,
            files,
            path: self.parent_path.join(name).join("files"),
            protected: self.protected.clone(),
        };
        for entry in manifest.entries() {
            self.verify()?;
            result.verify_roots()?;
            match entry {
                RemoteInputEntry::Directory { path } => {
                    result.files.filesystem().create_dir_all(Path::new(path))?;
                }
                RemoteInputEntry::File {
                    path,
                    executable,
                    chunks,
                    ..
                } => {
                    result.files.filesystem().write_new_file_with(
                        Path::new(path),
                        fs::Permissions::from_mode(if *executable { 0o700 } else { 0o600 }),
                        |output| {
                            for chunk in chunks {
                                self.verify()?;
                                result.verify_roots()?;
                                let relative =
                                    StoreLayout::new(PathBuf::new()).chunk_path(&chunk.digest);
                                let file =
                                    self.store.filesystem().read_only().read_file(&relative)?;
                                let mut bytes = Vec::with_capacity(chunk.bytes as usize + 1);
                                file.take(chunk.bytes + 1).read_to_end(&mut bytes)?;
                                if bytes.len() as u64 != chunk.bytes
                                    || Blake3::digest_bytes(&bytes) != chunk.digest
                                {
                                    return Err(invalid());
                                }
                                self.verify()?;
                                output.write_all(&bytes)?;
                            }
                            Ok(())
                        },
                    )?;
                }
            }
        }
        self.verify()?;
        result.verify()?;
        result.files.sync()?;
        result.allocation.sync()?;
        result.verify_roots()?;
        Ok(result)
    }
}

/// Exact newly materialized input. This is a verification handle, not a durable launch receipt.
/// Keep this handle and recheck immediately before any separately authorized executor claim.
/// Dropping it never deletes partial or completed work. Restart requires native reconciliation.
pub struct RemoteInputAllocation {
    manifest: RemoteInputManifest,
    parent: PinnedWorkspaceRoot,
    allocation: PinnedWorkspaceRoot,
    files: PinnedWorkspaceRoot,
    path: PathBuf,
    protected: Vec<ProtectedWorkspaceRoot>,
}
impl RemoteInputAllocation {
    /// Native-owned working folder. A path alone conveys no execution or filesystem authority.
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Exact immutable input and transfer bundle that this tree must still match.
    pub fn manifest(&self) -> &RemoteInputManifest {
        &self.manifest
    }
    fn verify_roots(&self) -> io::Result<()> {
        for root in [&self.parent, &self.allocation, &self.files] {
            private(root)?;
        }
        if !self.allocation.is_within(token(&self.parent)?)?
            || !self.files.is_within(token(&self.allocation)?)?
        {
            return Err(invalid());
        }
        for protected in &self.protected {
            if self.files.is_within(*protected)? {
                return Err(invalid());
            }
        }
        Ok(())
    }
    /// Revalidate namespace/physical custody, complete bounded inventory, file bytes and modes.
    /// Changed, extra, missing, aliased or unsupported entries refuse without repairing anything.
    pub fn verify(&self) -> io::Result<()> {
        self.verify_roots()?;
        let mut expected = BTreeMap::new();
        for entry in self.manifest.entries() {
            let (path, directory) = match entry {
                RemoteInputEntry::Directory { path } => (path, true),
                RemoteInputEntry::File { path, .. } => (path, false),
            };
            expected.insert(path.clone(), directory);
        }
        let mut found = BTreeMap::new();
        let mut pending = vec![PathBuf::new()];
        while let Some(directory) = pending.pop() {
            let names = self.files.filesystem().read_directory_names_bounded(
                &directory,
                expected.len().saturating_sub(found.len()),
            )?;
            for name in names {
                let path = directory.join(name);
                let text = path.to_str().ok_or_else(invalid)?.to_owned();
                let metadata = self.files.filesystem().inspect_entry(&path)?.metadata()?;
                if found.len() >= expected.len()
                    || (!metadata.is_file() && !metadata.is_dir())
                    || (metadata.is_file() && metadata.nlink() != 1)
                    || found.insert(text, metadata.is_dir()).is_some()
                {
                    return Err(invalid());
                }
                if metadata.is_dir() {
                    pending.push(path);
                }
            }
        }
        if found != expected {
            return Err(invalid());
        }
        for entry in self.manifest.entries() {
            if let RemoteInputEntry::File {
                path,
                executable,
                digest,
                chunks,
            } = entry
            {
                let length: u64 = chunks.iter().map(|chunk| chunk.bytes).sum();
                let observed = observe_file(&self.files, Path::new(path), length)?;
                if observed.bytes != length
                    || observed.digest != digest.to_string()
                    || (observed.mode & 0o111 != 0) != *executable
                {
                    return Err(invalid());
                }
            }
        }
        self.verify_roots()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::{RemoteAssignment, RemoteInputChunk, RemoteInputReceiver};
    use mesh_store::RecordDigest;
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    const ID: &str = "0123456789abcdef0123456789abcdef";
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "mesh-remote-tree-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            for name in ["store", "allocations", "protected"] {
                fs::create_dir(root.join(name)).unwrap();
                fs::set_permissions(root.join(name), fs::Permissions::from_mode(0o700)).unwrap();
            }
            Self(root)
        }
        fn destination(&self) -> RemoteInputDestination {
            let store = self.0.join("store");
            let parent = self.0.join("allocations");
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
        let digest = Blake3::digest_bytes(b"saved bytes");
        RemoteInputManifest::new(
            RecordDigest::from_bytes([7; 32]),
            vec![
                RemoteInputEntry::Directory { path: "src".into() },
                RemoteInputEntry::Directory {
                    path: "empty-dir".into(),
                },
                RemoteInputEntry::File {
                    path: "empty".into(),
                    executable: false,
                    digest: Blake3::digest_bytes(b""),
                    chunks: vec![],
                },
                RemoteInputEntry::File {
                    path: "src/tool".into(),
                    executable: true,
                    digest,
                    chunks: vec![RemoteInputChunk { digest, bytes: 11 }],
                },
            ],
        )
        .unwrap()
    }
    fn receiver(cas: &Cas, complete: bool) -> RemoteInputReceiver<'_> {
        let manifest = manifest();
        let assignment = RemoteAssignment {
            id: "assigned".into(),
            worker_key: "ab".repeat(32),
            input: manifest.input(),
            bundle: manifest.bundle(),
            lease_sequence: 1,
            lease_until_ms: 1000,
        };
        let mut receiver = RemoteInputReceiver::new(manifest, &assignment, cas).unwrap();
        if complete {
            receiver
                .accept(
                    Blake3::digest_bytes(b"saved bytes"),
                    0,
                    b"saved bytes",
                    true,
                )
                .unwrap();
        }
        receiver
    }
    #[test]
    fn creates_exact_private_tree_and_refuses_changed_output_without_repair() {
        let fixture = Fixture::new();
        let cas = Cas::open(fixture.0.join("store")).unwrap();
        let receiver = receiver(&cas, true);
        let destination = fixture.destination();
        let tree = receiver.materialize(&destination, ID).unwrap();
        assert_eq!(tree.manifest(), &manifest());
        assert!(tree.path().join("empty-dir").is_dir());
        assert_eq!(fs::read(tree.path().join("empty")).unwrap(), b"");
        assert_eq!(
            fs::read(tree.path().join("src/tool")).unwrap(),
            b"saved bytes"
        );
        assert_eq!(
            fs::metadata(tree.path().join("src/tool"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        tree.verify().unwrap();
        assert!(receiver.materialize(&destination, ID).is_err());
        fs::write(tree.path().join("src/tool"), b"other bytes").unwrap();
        assert!(tree.verify().is_err());
        assert_eq!(
            fs::read(tree.path().join("src/tool")).unwrap(),
            b"other bytes"
        );
        fs::write(tree.path().join("src/tool"), b"saved bytes").unwrap();
        fs::write(tree.path().join("extra"), b"keep").unwrap();
        assert!(tree.verify().is_err());
        fs::remove_file(tree.path().join("extra")).unwrap();
        fs::set_permissions(
            tree.path().join("src/tool"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        assert!(tree.verify().is_err());
    }
    #[test]
    fn incomplete_or_corrupt_input_preserves_partial_allocation_and_refuses_retry() {
        let fixture = Fixture::new();
        let cas = Cas::open(fixture.0.join("store")).unwrap();
        let incomplete = receiver(&cas, false);
        let destination = fixture.destination();
        assert!(incomplete.materialize(&destination, ID).is_err());
        let partial = fixture.0.join("allocations").join(format!("input-{ID}"));
        assert!(partial.join("manifest.json").is_file());
        let complete = receiver(&cas, true);
        assert!(complete.materialize(&destination, ID).is_err());
        let chunk = cas
            .layout()
            .chunk_path(&Blake3::digest_bytes(b"saved bytes"));
        fs::write(&chunk, b"wrong bytes").unwrap();
        assert!(complete
            .materialize(&destination, "11111111111111111111111111111111")
            .is_err());
        assert_eq!(fs::read(&chunk).unwrap(), b"wrong bytes");
        // A sparse oversized stored object must be bounded, not buffered or quarantined.
        fs::OpenOptions::new()
            .write(true)
            .open(&chunk)
            .unwrap()
            .set_len(32 * 1024 * 1024)
            .unwrap();
        assert!(complete
            .materialize(&destination, "22222222222222222222222222222222")
            .is_err());
        assert_eq!(fs::metadata(&chunk).unwrap().len(), 32 * 1024 * 1024);
        fs::remove_file(&chunk).unwrap();
        fs::write(fixture.0.join("external"), b"saved bytes").unwrap();
        symlink(fixture.0.join("external"), &chunk).unwrap();
        assert!(complete
            .materialize(&destination, "33333333333333333333333333333333")
            .is_err());
        assert!(fs::symlink_metadata(chunk)
            .unwrap()
            .file_type()
            .is_symlink());
    }
    #[test]
    fn complete_chunks_do_not_substitute_for_complete_file_integrity() {
        let fixture = Fixture::new();
        let cas = Cas::open(fixture.0.join("store")).unwrap();
        let _complete = receiver(&cas, true);
        let mut entries = manifest().entries().to_vec();
        for entry in &mut entries {
            if let RemoteInputEntry::File { path, digest, .. } = entry {
                if path == "src/tool" {
                    *digest = Blake3::digest_bytes(b"different");
                }
            }
        }
        let manifest =
            RemoteInputManifest::new(RecordDigest::from_bytes([7; 32]), entries).unwrap();
        let assignment = RemoteAssignment {
            id: "assigned".into(),
            worker_key: "ab".repeat(32),
            input: manifest.input(),
            bundle: manifest.bundle(),
            lease_sequence: 1,
            lease_until_ms: 1000,
        };
        let receiver = RemoteInputReceiver::new(manifest, &assignment, &cas).unwrap();
        assert!(receiver.materialize(&fixture.destination(), ID).is_err());
        assert_eq!(
            fs::read(
                fixture
                    .0
                    .join("allocations")
                    .join(format!("input-{ID}/files/src/tool"))
            )
            .unwrap(),
            b"saved bytes"
        );
    }
    #[test]
    fn retained_tree_refuses_alias_escape_and_hardlinked_content() {
        let fixture = Fixture::new();
        let cas = Cas::open(fixture.0.join("store")).unwrap();
        let tree = receiver(&cas, true)
            .materialize(&fixture.destination(), ID)
            .unwrap();
        fs::hard_link(tree.path().join("src/tool"), fixture.0.join("alias")).unwrap();
        assert!(tree.verify().is_err());
        fs::remove_file(fixture.0.join("alias")).unwrap();
        tree.verify().unwrap();
        let allocation = tree.path().parent().unwrap();
        let moved = fixture.0.join("moved");
        fs::rename(allocation, &moved).unwrap();
        symlink(&moved, allocation).unwrap();
        assert!(tree.verify().is_err());
        assert_eq!(
            fs::read(moved.join("files/src/tool")).unwrap(),
            b"saved bytes"
        );
    }
    #[test]
    fn admission_refuses_substitution_shared_permissions_overlap_and_protected_roots() {
        let fixture = Fixture::new();
        let store = fixture.0.join("store");
        let parent = fixture.0.join("allocations");
        let store_id = ProtectedWorkspaceRoot::inspect(&store).unwrap();
        let parent_id = ProtectedWorkspaceRoot::inspect(&parent).unwrap();
        assert!(RemoteInputDestination::admit(&store, parent_id, &parent, parent_id, &[]).is_err());
        assert!(RemoteInputDestination::admit(&store, store_id, &store, store_id, &[]).is_err());
        assert!(
            RemoteInputDestination::admit(&store, store_id, &parent, parent_id, &[parent_id])
                .is_err()
        );
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(RemoteInputDestination::admit(&store, store_id, &parent, parent_id, &[]).is_err());
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
        let destination = fixture.destination();
        let cas = Cas::open(&store).unwrap();
        let receiver = receiver(&cas, true);
        fs::rename(&store, fixture.0.join("old-store")).unwrap();
        fs::create_dir(&store).unwrap();
        fs::set_permissions(&store, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(receiver.materialize(&destination, ID).is_err());
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
    }
}
