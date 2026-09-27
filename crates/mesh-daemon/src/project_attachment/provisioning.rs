//! Native allocation of external attachment storage. Renderer code never chooses a store path.

use super::{
    external_store, invalid, pin_absolute_directory, read_receipt, AttachmentCaptureService,
    CaptureSchedule, ProjectAttachment,
};
use crate::root_authority::PinnedWorkspaceRoot;
use crate::CheckpointSigner;
use mesh_types::{Blake3, ContentDigest as _};
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// An existing host-owned storage directory, retained by native directory identity.
/// The host supplies this directory from its own configuration, never a renderer-selected path.
pub struct AttachmentStorage {
    path: PathBuf,
    pinned: PinnedWorkspaceRoot,
}

/// A registered project and its exact native store authority, ready for capture.
#[derive(Clone)]
pub struct ProvisionedAttachment {
    attachment: ProjectAttachment,
    metadata: PathBuf,
    store: PinnedWorkspaceRoot,
    id: String,
}

impl AttachmentStorage {
    /// Pin an existing native host storage root without creating or adopting another folder.
    pub fn open(path: &Path) -> io::Result<Self> {
        let pinned = pin_absolute_directory(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            pinned,
        })
    }

    /// Provision or recover storage for the exact source identity. No source files are changed.
    /// The deterministic identifier is a lookup key, not evidence of authorship or authorization.
    pub fn provision(&self, source: &Path) -> io::Result<ProvisionedAttachment> {
        let attachment = ProjectAttachment::admit(source)?;
        self.pinned.ensure_namespace_identity()?;
        let checked = external_store(&self.path, &attachment)?;
        if checked.identity()? != self.pinned.identity()? {
            return Err(invalid("attachment storage identity changed"));
        }
        let receipt = attachment.receipt()?.encode();
        let digest = Blake3::digest_bytes(receipt.as_bytes());
        let id: String = digest
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let name = format!("project-{id}");
        let store = match self.pinned.create_child_directory(OsStr::new(&name)) {
            Ok(store) => {
                attachment.register_in_store(&store)?;
                store
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let store = self.pinned.open_child_directory(OsStr::new(&name))?;
                // A partial or foreign directory is recovery evidence, never silently initialized.
                if read_receipt(&store)? != receipt {
                    return Err(invalid("provisioned attachment receipt does not match"));
                }
                store
            }
            Err(error) => return Err(error),
        };
        self.pinned.ensure_namespace_identity()?;
        store.ensure_namespace_identity()?;
        attachment.ensure_current()?;
        Ok(ProvisionedAttachment {
            attachment,
            metadata: self.path.join(name),
            store,
            id,
        })
    }
}

impl ProvisionedAttachment {
    /// Stable opaque lookup identifier derived from the complete registration receipt.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Native diagnostics only; graphical callers should retain the opaque project identifier.
    pub fn metadata_path(&self) -> &Path {
        &self.metadata
    }

    /// The admitted original project, still writable by ordinary tools.
    pub fn project(&self) -> &ProjectAttachment {
        &self.attachment
    }

    /// Read exact saved versions through the store admitted during provisioning.
    pub fn saved_versions(&self) -> io::Result<Vec<super::SavedAttachmentVersion>> {
        self.attachment
            .saved_versions_in_store(&self.metadata, self.store.clone())
    }

    /// Share the admitted source and store descriptors directly with the capture worker.
    pub fn start_capture(
        &self,
        signer: Arc<dyn CheckpointSigner>,
        schedule: CaptureSchedule,
    ) -> io::Result<AttachmentCaptureService> {
        AttachmentCaptureService::start_pinned(
            &self.metadata,
            self.attachment.clone(),
            self.store.clone(),
            signer,
            schedule,
        )
    }
}
