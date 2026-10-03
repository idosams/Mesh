//! Original received-import reconstruction. Callers hold allocation ownership and admission.
use super::*;
use std::ffi::OsStr;
use std::os::unix::fs::MetadataExt as _;

pub(crate) struct ReceivedImportHandoff {
    pub(crate) destination: PathBuf,
    pub(crate) working: PinnedWorkspaceRoot,
    pub(crate) storage: PinnedWorkspaceRoot,
}

fn prefix(root: &PinnedWorkspaceRoot, path: &Path, expected: &[u8]) -> io::Result<Option<Vec<u8>>> {
    root.ensure_namespace_identity()?;
    let file = match root.filesystem().read_only().read_file(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.mode() & 0o7777 != 0o600
        || metadata.len() > expected.len() as u64
    {
        return Err(io::Error::other("received ownership record changed"));
    }
    let mut bytes = Vec::new();
    file.take(expected.len() as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 != metadata.len() || !expected.starts_with(&bytes) {
        return Err(io::Error::other("received ownership record differs"));
    }
    root.ensure_namespace_identity()?;
    Ok(Some(bytes))
}

impl PreparedFolderImport {
    pub(crate) fn recover_received_with_parent(
        source: &Path,
        storage_root: &Path,
        protected: &[ProtectedWorkspaceRoot],
        expected_parent: ProtectedWorkspaceRoot,
        revalidate: impl Fn() -> io::Result<()>,
    ) -> Result<ReceivedImportHandoff, FolderImportError> {
        let guard = || {
            revalidate().map_err(|e| {
                FolderImportError::io("revalidate received admission", storage_root, e)
            })
        };
        guard()?;
        let source = validated_source(source)?;
        let source_identity = directory_identity(&source)?;
        let storage_root = absolute_destination(storage_root)?;
        let parent = storage_root
            .parent()
            .ok_or_else(|| unrecognized(&storage_root))?;
        let parent_pin = PinnedWorkspaceRoot::open(parent.to_path_buf())
            .map_err(|e| FolderImportError::io("pin allocation", parent, e))?;
        parent_pin
            .ensure_protected_identity(expected_parent)
            .map_err(|e| FolderImportError::io("verify allocation", parent, e))?;
        for protected in protected {
            if parent_pin
                .is_within(*protected)
                .map_err(|e| FolderImportError::io("check protected root", parent, e))?
            {
                return Err(FolderImportError::DestinationInsideProtectedRoot {
                    destination: storage_root,
                });
            }
        }
        if parent.starts_with(&source) {
            return Err(FolderImportError::DestinationInsideSource {
                source,
                destination: storage_root,
            });
        }
        let leaf = storage_root
            .file_name()
            .ok_or_else(|| unrecognized(&storage_root))?;
        let storage = match parent_pin.open_child_directory(leaf) {
            Ok(pin) => pin,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                guard()?;
                let prepared = Self::prepare_received_with_parent(
                    &source,
                    &storage_root,
                    protected,
                    expected_parent,
                )?;
                let (working, storage) = prepared.presented_handoff_roots()?;
                guard()?;
                let (confirmed, _) = prepared.confirm_into_workspace_without_origin()?;
                guard()?;
                return Ok(ReceivedImportHandoff {
                    destination: confirmed.destination().to_path_buf(),
                    working,
                    storage,
                });
            }
            Err(e) => {
                return Err(FolderImportError::io(
                    "reopen original store",
                    &storage_root,
                    e,
                ))
            }
        };
        let io_error =
            |e| FolderImportError::io("verify original received import", &storage_root, e);
        let layout = Path::new(crate::workspace::PRESENTED_LAYOUT_MARKER_NAME);
        if prefix(
            &storage,
            layout,
            crate::workspace::PRESENTED_LAYOUT_MARKER_BYTES,
        )
        .map_err(io_error)?
        .as_deref()
            != Some(crate::workspace::PRESENTED_LAYOUT_MARKER_BYTES)
        {
            return Err(unrecognized(&storage_root));
        }
        let working = storage
            .open_child_directory(OsStr::new(crate::workspace::PRESENTED_DIRECTORY_NAME))
            .map_err(io_error)?;
        for pin in [&storage, &working] {
            if pin
                .try_clone_directory()
                .and_then(|file| file.metadata())
                .map_err(io_error)?
                .mode()
                & 0o7777
                != 0o700
            {
                return Err(unrecognized(&storage_root));
            }
        }
        let destination = storage_root.join(crate::workspace::PRESENTED_DIRECTORY_NAME);
        let (device, inode) = working.identity().map_err(io_error)?;
        let destination_identity = DirectoryIdentity { device, inode };
        let (device, inode) = storage.identity().map_err(io_error)?;
        let presented_store = PresentedStore {
            root: storage_root.clone(),
            identity: DirectoryIdentity { device, inode },
        };
        let before = import_snapshot_for_layout(&source, false)?;
        let markers = markers(&destination)?;
        let owned = owned_marker_bytes(destination_identity, Some(&presented_store));
        let owned_bytes = prefix(&storage, &markers.owned, owned.as_bytes()).map_err(io_error)?;
        let receipt = encode_receipt(
            ReceiptEncoding::MetadataPresented,
            destination_identity,
            Some(&presented_store),
            &before,
            &before,
            true,
        );
        let receipt_bytes = prefix(&storage, &markers.receipt, &receipt).map_err(io_error)?;
        let claim = prefix(&storage, &markers.claim, CLAIM_MARKER).map_err(io_error)?;
        if let Some(bytes) = &owned_bytes {
            if bytes != owned.as_bytes()
                || claim.as_ref().is_some_and(|value| value != CLAIM_MARKER)
            {
                return Err(unrecognized(&markers.owned));
            }
            guard()?;
            // Only a complete original ownership marker permits continued prefix copying.
            copy_snapshot_with_destination_root(
                &source,
                source_identity,
                &destination,
                destination_identity,
                &working,
                &before,
                ImportPurpose::Received,
            )?;
            guard()?;
            let copied = snapshot_with_pinned_root(
                &destination,
                destination_identity,
                &working,
                None,
                false,
            )?;
            let after = import_snapshot_for_layout(&source, false)?;
            let changed = differences(&before, &after, &copied);
            if !changed.is_empty() {
                return Err(FolderImportError::VerificationMismatch { paths: changed });
            }
            if claim.is_some() {
                storage
                    .filesystem()
                    .remove_file(&markers.claim)
                    .map_err(io_error)?;
                storage.sync().map_err(io_error)?;
            }
            let prepared = Self {
                source,
                source_identity,
                destination: destination.clone(),
                owned_marker: markers.owned,
                receipt: markers.receipt,
                destination_identity,
                destination_pinned: working.clone(),
                destination_parent_pinned: storage.clone(),
                presented_store: Some(presented_store),
                snapshot: before,
                source_private_fence: false,
                active: true,
                purpose: ImportPurpose::Received,
            };
            guard()?;
            prepared.confirm_into_workspace_without_origin()?;
        } else {
            if claim.is_some() || receipt_bytes.as_deref() != Some(receipt.as_slice()) {
                return Err(unrecognized(&markers.receipt));
            }
            // A confirmed workspace may have a populated derived index; never rerun ingestion.
            let copied = snapshot_with_pinned_root(
                &destination,
                destination_identity,
                &working,
                None,
                false,
            )?;
            let after = import_snapshot_for_layout(&source, false)?;
            let changed = differences(&before, &after, &copied);
            if !changed.is_empty() {
                return Err(FolderImportError::VerificationMismatch { paths: changed });
            }
        }
        parent_pin
            .ensure_protected_identity(expected_parent)
            .map_err(io_error)?;
        working
            .ensure_namespace_identity()
            .and_then(|()| storage.ensure_namespace_identity())
            .map_err(io_error)?;
        guard()?;
        Ok(ReceivedImportHandoff {
            destination,
            working,
            storage,
        })
    }
}
