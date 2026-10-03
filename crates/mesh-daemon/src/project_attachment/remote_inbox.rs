//! Retain receiving storage identity outside that storage, under the existing native catalogue.
use super::*;
use crate::fleet::RemoteResultInbox;
use std::os::unix::fs::MetadataExt as _;
const RECORD: &str = "desktop-remote-inbox.json";
const LIMIT: u64 = 4096;
fn unavailable() -> io::Error {
    invalid("Remote receiving storage needs reconciliation")
}
fn identity(root: &PinnedWorkspaceRoot) -> io::Result<ProtectedWorkspaceRoot> {
    let (device, inode) = root.identity()?;
    ProtectedWorkspaceRoot::from_directory_token(&format!("{device:016x}:{inode:016x}"))
}
fn private(root: &PinnedWorkspaceRoot) -> io::Result<()> {
    root.ensure_namespace_identity()?;
    if root.try_clone_directory()?.metadata()?.permissions().mode() & 0o077 != 0 {
        return Err(unavailable());
    }
    Ok(())
}
impl AttachmentStorage {
    /// Open the exact native result inbox, optionally provisioning it on an explicit first receipt.
    /// The native application supplies its admitted parent and extra protected roots. All retained
    /// project identities (including detached/offline registrations) and this catalogue are added
    /// automatically. The binding is saved outside the inbox before returning receiving authority.
    /// A partial binding or an inbox without a binding refuses; no orphan is adopted or deleted.
    pub fn remote_result_inbox(
        &self,
        parent: &Path,
        expected_parent: ProtectedWorkspaceRoot,
        additional_protected: &[ProtectedWorkspaceRoot],
        create: bool,
    ) -> io::Result<Option<RemoteResultInbox>> {
        if !parent.is_absolute() || additional_protected.len() > 62 {
            return Err(unavailable());
        }
        let parent_pin = PinnedWorkspaceRoot::open(parent.to_path_buf())?;
        parent_pin.ensure_protected_identity(expected_parent)?;
        parent_pin.ensure_namespace_identity()?;
        if parent_pin
            .try_clone_directory()?
            .metadata()?
            .permissions()
            .mode()
            & 0o022
            != 0
        {
            return Err(unavailable());
        }
        private(&self.pinned)?;
        let _guard = crate::workspace_custody::lock_workspace_initialization(&self.pinned)
            .map_err(|_| unavailable())?;
        let mut protected = additional_protected.to_vec();
        protected.push(identity(&self.pinned)?);
        protected.extend(
            self.registrations()?
                .iter()
                .map(|registration| registration.identity),
        );
        protected.sort();
        protected.dedup();
        if protected.len() > 64 {
            return Err(unavailable());
        }
        let path = parent.join("remote-results");
        let saved = self.read_inbox_binding()?;
        let inbox = match saved.as_deref() {
            Some(raw) => {
                let value = Json::parse(raw).map_err(|_| unavailable())?;
                let expected = ProtectedWorkspaceRoot::from_directory_token(
                    value
                        .get("inbox")
                        .and_then(Json::as_text)
                        .ok_or_else(unavailable)?,
                )?;
                let inbox = RemoteResultInbox::open(&path, expected, &protected)?;
                if self.inbox_binding(parent, expected_parent, &inbox)? != raw {
                    return Err(unavailable());
                }
                inbox
            }
            None => {
                // Missing catalogue state cannot authorize an existing, possibly partial inbox.
                match parent_pin
                    .filesystem()
                    .inspect_entry(Path::new("remote-results"))
                {
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    _ => return Err(unavailable()),
                }
                if !create {
                    parent_pin.ensure_protected_identity(expected_parent)?;
                    self.pinned.ensure_namespace_identity()?;
                    return Ok(None);
                }
                let inbox = RemoteResultInbox::create(parent, expected_parent, &protected)?;
                let raw = self.inbox_binding(parent, expected_parent, &inbox)?;
                self.pinned.filesystem().write_new_file(
                    Path::new(RECORD),
                    raw.as_bytes(),
                    fs::Permissions::from_mode(0o600),
                )?;
                self.pinned.sync()?;
                if self.read_inbox_binding()?.as_deref() != Some(raw.as_str()) {
                    return Err(unavailable());
                }
                inbox
            }
        };
        parent_pin.ensure_protected_identity(expected_parent)?;
        private(&self.pinned)?;
        inbox.destination()?;
        Ok(Some(inbox))
    }
    fn inbox_binding(
        &self,
        parent: &Path,
        expected_parent: ProtectedWorkspaceRoot,
        inbox: &RemoteResultInbox,
    ) -> io::Result<String> {
        let raw = Json::object([
            ("schema", Json::text("mesh.native-result-inbox-binding/v1")),
            (
                "catalogue",
                Json::text(identity(&self.pinned)?.directory_token()),
            ),
            (
                "parent",
                Json::text(parent.to_str().ok_or_else(unavailable)?),
            ),
            (
                "parent_identity",
                Json::text(expected_parent.directory_token()),
            ),
            ("inbox", Json::text(inbox.identity()?.directory_token())),
        ])
        .encode();
        if raw.len() as u64 > LIMIT {
            return Err(unavailable());
        }
        Ok(raw)
    }
    fn read_inbox_binding(&self) -> io::Result<Option<String>> {
        self.pinned.ensure_namespace_identity()?;
        let file = match self
            .pinned
            .filesystem()
            .read_only()
            .read_file(Path::new(RECORD))
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.nlink() != 1
            || metadata.len() > LIMIT
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(unavailable());
        }
        let mut bytes = Vec::new();
        file.take(LIMIT + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > LIMIT {
            return Err(unavailable());
        }
        Ok(Some(String::from_utf8(bytes).map_err(|_| unavailable())?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "mesh-inbox-catalogue-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
            for name in ["catalogue", "project"] {
                fs::create_dir(root.join(name)).unwrap();
                fs::set_permissions(root.join(name), fs::Permissions::from_mode(0o700)).unwrap();
            }
            Self(root)
        }
        fn store(&self) -> AttachmentStorage {
            AttachmentStorage::open(&self.0.join("catalogue")).unwrap()
        }
        fn inbox(
            &self,
            store: &AttachmentStorage,
            create: bool,
        ) -> io::Result<Option<RemoteResultInbox>> {
            store.remote_result_inbox(
                &self.0,
                ProtectedWorkspaceRoot::inspect(&self.0).unwrap(),
                &[],
                create,
            )
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn native_inbox_binding_is_lazy_and_survives_catalogue_restart() {
        let f = Fixture::new();
        let store = f.store();
        let registration = store.provision(&f.0.join("project")).unwrap();
        assert!(f.inbox(&store, false).unwrap().is_none());
        let inbox = f.inbox(&store, true).unwrap().unwrap();
        let identity = inbox.identity().unwrap();
        assert!(f.0.join("catalogue").join(RECORD).is_file());
        drop(inbox);
        drop(store);
        let reopened = f.inbox(&f.store(), false).unwrap().unwrap();
        assert_eq!(reopened.identity().unwrap(), identity);
        assert_eq!(
            f.inbox(&f.store(), true)
                .unwrap()
                .unwrap()
                .identity()
                .unwrap(),
            identity
        );
        assert!(registration.project().ensure_current().is_ok());
        assert_eq!(fs::read_dir(f.0.join("project")).unwrap().count(), 0);
    }
    #[test]
    fn retained_detached_project_identity_blocks_receiving_inside_renamed_source() {
        let f = Fixture::new();
        let store = f.store();
        let registered = store.provision(&f.0.join("project")).unwrap();
        store.set_detached(registered.id(), true).unwrap();
        let moved = f.0.join("moved-project");
        fs::rename(f.0.join("project"), &moved).unwrap();
        assert!(store
            .remote_result_inbox(
                &moved,
                ProtectedWorkspaceRoot::inspect(&moved).unwrap(),
                &[],
                true
            )
            .is_err());
        assert!(!moved.join("remote-results").exists());
        assert!(!f.0.join("catalogue").join(RECORD).exists());
    }
    #[test]
    fn unbound_partial_or_substituted_inbox_is_preserved_and_never_adopted() {
        let f = Fixture::new();
        let store = f.store();
        let inbox = f.inbox(&store, true).unwrap().unwrap();
        drop(inbox);
        let record = f.0.join("catalogue").join(RECORD);
        let bytes = fs::read(&record).unwrap();
        fs::write(&record, b"partial").unwrap();
        assert!(f.inbox(&store, false).is_err());
        assert!(f.inbox(&store, true).is_err());
        assert_eq!(fs::read(&record).unwrap(), b"partial");
        fs::write(&record, &bytes).unwrap();
        fs::rename(&record, record.with_extension("saved")).unwrap();
        assert!(f.inbox(&store, false).is_err());
        assert!(f.inbox(&store, true).is_err());
        fs::write(&record, &bytes).unwrap();
        fs::set_permissions(&record, fs::Permissions::from_mode(0o600)).unwrap();
        let path = f.0.join("remote-results");
        fs::rename(&path, f.0.join("original-inbox")).unwrap();
        let replacement =
            RemoteResultInbox::create(&f.0, ProtectedWorkspaceRoot::inspect(&f.0).unwrap(), &[])
                .unwrap();
        assert!(f.inbox(&store, true).is_err());
        assert!(replacement.destination().is_ok());
        assert_eq!(fs::read(&record).unwrap(), bytes);
    }
    #[test]
    fn copied_catalogue_binding_never_adopts_another_catalogues_inbox() {
        let f = Fixture::new();
        let original = f.inbox(&f.store(), true).unwrap().unwrap();
        let other_path = f.0.join("other-catalogue");
        fs::create_dir(&other_path).unwrap();
        fs::set_permissions(&other_path, fs::Permissions::from_mode(0o700)).unwrap();
        fs::copy(f.0.join("catalogue").join(RECORD), other_path.join(RECORD)).unwrap();
        let other = AttachmentStorage::open(&other_path).unwrap();
        assert!(f.inbox(&other, false).is_err());
        assert!(f.inbox(&other, true).is_err());
        assert!(original.destination().is_ok());
        assert!(other_path.join(RECORD).is_file());
    }
    #[test]
    fn ordinary_application_parent_keeps_inbox_private_and_writable_parents_refuse() {
        let f = Fixture::new();
        fs::set_permissions(&f.0, fs::Permissions::from_mode(0o755)).unwrap();
        let inbox = f.inbox(&f.store(), true).unwrap().unwrap();
        let identity = inbox.identity().unwrap();
        for name in [
            "remote-results",
            "remote-results/store",
            "remote-results/allocations",
        ] {
            assert_eq!(
                fs::metadata(f.0.join(name)).unwrap().permissions().mode() & 0o077,
                0
            );
        }
        assert_eq!(
            f.inbox(&f.store(), false)
                .unwrap()
                .unwrap()
                .identity()
                .unwrap(),
            identity
        );
        for mode in [0o770, 0o777] {
            let unsafe_parent = Fixture::new();
            fs::set_permissions(&unsafe_parent.0, fs::Permissions::from_mode(mode)).unwrap();
            assert!(unsafe_parent.inbox(&unsafe_parent.store(), true).is_err());
            assert!(!unsafe_parent.0.join("remote-results").exists());
            assert!(!unsafe_parent.0.join("catalogue").join(RECORD).exists());
        }
    }
}
