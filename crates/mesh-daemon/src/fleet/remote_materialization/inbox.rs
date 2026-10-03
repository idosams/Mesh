//! Native create-only result storage with independently retained installation identity.
use super::*;
use crate::ipc::Json;

mod intents;
pub use intents::RemoteReceiptIntent;

const NAME: &str = "remote-results";
const RECEIPT: &str = "inbox.json";

/// Private receiving storage. The native owner must retain `identity()` outside this directory
/// before starting any transfer. Reopening never creates or repairs missing storage.
pub struct RemoteResultInbox {
    root: PinnedWorkspaceRoot,
    destination: RemoteInputDestination,
}
impl RemoteResultInbox {
    /// Create an absent inbox beneath an independently admitted native application directory.
    /// Existing or partially created inboxes refuse and remain untouched for reconciliation.
    pub fn create(
        parent: &Path,
        expected_parent: ProtectedWorkspaceRoot,
        protected: &[ProtectedWorkspaceRoot],
    ) -> io::Result<Self> {
        if !parent.is_absolute() || protected.len() > 64 {
            return Err(invalid());
        }
        let parent_pin = PinnedWorkspaceRoot::open(parent.to_path_buf())?;
        parent_pin.ensure_protected_identity(expected_parent)?;
        private(&parent_pin)?;
        for identity in protected {
            if parent_pin.is_within(*identity)? {
                return Err(invalid());
            }
        }
        let root = parent_pin.create_child_directory(OsStr::new(NAME))?;
        let store = root.create_child_directory(OsStr::new("store"))?;
        let allocations = root.create_child_directory(OsStr::new("allocations"))?;
        let destination = RemoteInputDestination::admit(
            &parent.join(NAME).join("store"),
            token(&store)?,
            &parent.join(NAME).join("allocations"),
            token(&allocations)?,
            protected,
        )?;
        let inbox = Self { root, destination };
        let receipt = inbox.receipt()?.encode();
        inbox.root.filesystem().write_new_file(
            Path::new(RECEIPT),
            receipt.as_bytes(),
            fs::Permissions::from_mode(0o600),
        )?;
        inbox.root.sync()?;
        parent_pin.ensure_protected_identity(expected_parent)?;
        inbox.verify()?;
        Ok(inbox)
    }

    /// Reopen using an identity retained independently by the native owner. A path or self-written
    /// receipt alone cannot adopt another inbox. Newly protected project roots are checked again.
    pub fn open(
        path: &Path,
        expected: ProtectedWorkspaceRoot,
        protected: &[ProtectedWorkspaceRoot],
    ) -> io::Result<Self> {
        if !path.is_absolute() || protected.len() > 64 {
            return Err(invalid());
        }
        let root = PinnedWorkspaceRoot::open(path.to_path_buf())?;
        root.ensure_protected_identity(expected)?;
        private(&root)?;
        let raw = read_receipt(&root)?;
        let receipt = Json::parse(&raw).map_err(|_| invalid())?;
        let identity = |key| {
            ProtectedWorkspaceRoot::from_directory_token(
                receipt
                    .get(key)
                    .and_then(Json::as_text)
                    .ok_or_else(invalid)?,
            )
        };
        let destination = RemoteInputDestination::admit(
            &path.join("store"),
            identity("store")?,
            &path.join("allocations"),
            identity("allocations")?,
            protected,
        )?;
        let inbox = Self { root, destination };
        if inbox.receipt()?.encode() != raw {
            return Err(invalid());
        }
        inbox.verify()?;
        Ok(inbox)
    }

    /// Exact native installation to persist outside the inbox before transport is allowed.
    pub fn identity(&self) -> io::Result<ProtectedWorkspaceRoot> {
        self.verify()?;
        token(&self.root)
    }

    /// Borrow verified receiving authority, without authorizing a peer, transfer, or main approval.
    pub fn destination(&self) -> io::Result<&RemoteInputDestination> {
        self.verify()?;
        Ok(&self.destination)
    }

    fn receipt(&self) -> io::Result<Json> {
        Ok(Json::object([
            ("schema", Json::text("mesh.remote-result-inbox/v1")),
            ("root", Json::text(token(&self.root)?.directory_token())),
            (
                "store",
                Json::text(token(&self.destination.store)?.directory_token()),
            ),
            (
                "allocations",
                Json::text(token(&self.destination.parent)?.directory_token()),
            ),
        ]))
    }
    fn verify(&self) -> io::Result<()> {
        private(&self.root)?;
        self.destination.verify()?;
        if !self.destination.store.is_within(token(&self.root)?)?
            || !self.destination.parent.is_within(token(&self.root)?)?
            || read_receipt(&self.root)? != self.receipt()?.encode()
        {
            return Err(invalid());
        }
        Ok(())
    }
}
fn read_receipt(root: &PinnedWorkspaceRoot) -> io::Result<String> {
    let file = root
        .filesystem()
        .read_only()
        .read_file(Path::new(RECEIPT))?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.len() > 1024
    {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    file.take(1025).read_to_end(&mut bytes)?;
    if bytes.len() > 1024 {
        return Err(invalid());
    }
    String::from_utf8(bytes).map_err(|_| invalid())
}

#[cfg(test)]
mod tests {
    use super::super::tests::Fixture;
    use super::*;

    fn parent(f: &Fixture) -> PathBuf {
        f.0.join("allocations")
    }
    fn create(f: &Fixture) -> RemoteResultInbox {
        let p = parent(f);
        RemoteResultInbox::create(&p, ProtectedWorkspaceRoot::inspect(&p).unwrap(), &[]).unwrap()
    }
    #[test]
    fn create_reopen_and_receive_preserve_exact_storage() {
        let f = Fixture::new();
        let inbox = create(&f);
        let identity = inbox.identity().unwrap();
        let path = parent(&f).join(NAME);
        drop(inbox);
        let reopened = RemoteResultInbox::open(&path, identity, &[]).unwrap();
        assert!(reopened.destination().is_ok());
        assert_eq!(reopened.identity().unwrap(), identity);
        let cas = Cas::open(path.join("store")).unwrap();
        let receiver = super::super::tests::receiver(&cas, true);
        let tree = receiver
            .materialize(
                reopened.destination().unwrap(),
                "0123456789abcdef0123456789abcdef",
            )
            .unwrap();
        tree.verify().unwrap();
        assert_eq!(
            fs::read(tree.path().join("src/tool")).unwrap(),
            b"saved bytes"
        );
        let p = parent(&f);
        assert!(
            RemoteResultInbox::create(&p, ProtectedWorkspaceRoot::inspect(&p).unwrap(), &[])
                .is_err()
        );
        assert!(reopened.destination().is_ok());
    }
    #[test]
    fn replaced_storage_and_new_protection_refuse_without_repair() {
        let f = Fixture::new();
        let inbox = create(&f);
        let identity = inbox.identity().unwrap();
        let path = parent(&f).join(NAME);
        let protected = ProtectedWorkspaceRoot::inspect(&parent(&f)).unwrap();
        assert!(RemoteResultInbox::open(&path, identity, &[protected]).is_err());
        fs::rename(path.join("store"), path.join("saved-store")).unwrap();
        fs::create_dir(path.join("store")).unwrap();
        fs::set_permissions(path.join("store"), fs::Permissions::from_mode(0o700)).unwrap();
        assert!(inbox.destination().is_err());
        assert!(RemoteResultInbox::open(&path, identity, &[]).is_err());
        assert!(path.join("saved-store").is_dir());
    }
    #[test]
    fn wrong_identity_partial_setup_and_protected_parent_never_adopt() {
        let f = Fixture::new();
        let p = parent(&f);
        let identity = ProtectedWorkspaceRoot::inspect(&p).unwrap();
        assert!(RemoteResultInbox::create(&p, identity, &[identity]).is_err());
        assert!(!p.join(NAME).exists());
        fs::create_dir(p.join(NAME)).unwrap();
        fs::write(p.join(NAME).join("unknown"), b"preserve").unwrap();
        assert!(RemoteResultInbox::create(&p, identity, &[]).is_err());
        assert!(RemoteResultInbox::open(&p.join(NAME), identity, &[]).is_err());
        assert_eq!(fs::read(p.join(NAME).join("unknown")).unwrap(), b"preserve");
    }
    #[test]
    fn substituted_root_and_modified_receipt_refuse() {
        let f = Fixture::new();
        let inbox = create(&f);
        let identity = inbox.identity().unwrap();
        let path = parent(&f).join(NAME);
        fs::write(path.join(RECEIPT), b"{}").unwrap();
        assert!(inbox.destination().is_err());
        assert!(RemoteResultInbox::open(&path, identity, &[]).is_err());
        fs::rename(&path, parent(&f).join("preserved-inbox")).unwrap();
        let replacement = create(&f);
        assert_ne!(replacement.identity().unwrap(), identity);
        assert!(RemoteResultInbox::open(&path, identity, &[]).is_err());
    }
}
