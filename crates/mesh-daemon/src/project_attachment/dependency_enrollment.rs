//! Required old-reader fence only; no dependency enrollment or publication capability.
use super::{history::HISTORY, invalid, read_receipt, ProvisionedAttachment};
use crate::ipc::Json;
use crate::workspace::OpenWorkspace;
use crate::workspace_custody::WorkspaceInitializationGuard;
use mesh_cas::DurableFs as _;
use mesh_store::RecordDigest;
use std::fs::Permissions;
use std::io::{self, Read as _};
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::Path;

const SCHEMA: &str = "mesh.attachment-history/v3";
const STAGED: &str = "attachment-dependency.pending";
const MAX_BYTES: u64 = 65_536;

/// Thread-bound preparation guard. Drop releases custody but retains the required history binding.
/// No current renderer, agent or CLI operation invokes this preparation API.
pub struct AttachmentDependencyFence {
    attachment: ProvisionedAttachment,
    marker: String,
    _guard: WorkspaceInitializationGuard,
}

impl ProvisionedAttachment {
    /// Fence older attached-history writers before a future native enrollment transaction.
    /// The authority must be selected by native registration, not imported or agent-provided data.
    /// No journal record, source edit, grant or policy-aware write permission is produced.
    pub fn prepare_dependency_enrollment(
        &self,
        authority: RecordDigest,
    ) -> io::Result<AttachmentDependencyFence> {
        self.prepare_dependency_enrollment_with_sync(authority, |attachment| {
            attachment
                .store
                .filesystem()
                .sync_file(Path::new(HISTORY))?;
            attachment.store.sync()
        })
    }

    fn prepare_dependency_enrollment_with_sync(
        &self,
        authority: RecordDigest,
        sync: impl FnOnce(&Self) -> io::Result<()>,
    ) -> io::Result<AttachmentDependencyFence> {
        if authority == RecordDigest::from_bytes([0; 32]) {
            return Err(invalid("dependency authority is missing"));
        }
        self.attachment.ensure_current()?;
        self.store.ensure_namespace_identity()?;
        let guard = crate::workspace_custody::lock_workspace_initialization(&self.store)
            .map_err(|error| invalid(&error.to_string()))?;
        self.attachment.ensure_current()?;
        self.store.ensure_namespace_identity()?;
        if read_receipt(&self.store)? != self.attachment.receipt()?.encode() {
            return Err(invalid("attachment registration changed"));
        }
        super::detachment::ensure_attached(&self.store)?;
        let previous = read_private(self, HISTORY)?;
        let parsed = Json::parse(&previous).map_err(|_| invalid("invalid history binding"))?;
        let recovering = parsed.get("schema").and_then(Json::as_text) == Some(SCHEMA);
        let basis = if recovering {
            parsed
                .get("previous_binding")
                .and_then(Json::as_text)
                .ok_or_else(|| invalid("missing retained history binding"))?
                .to_owned()
        } else {
            previous.clone()
        };
        let marker = Json::object([
            ("schema", Json::text(SCHEMA)),
            ("previous_binding", Json::text(&basis)),
            ("dependency_authority", Json::text(authority.to_hex())),
        ])
        .encode();
        if marker.len() > MAX_BYTES as usize || (recovering && marker != previous) {
            return Err(invalid(
                "dependency preparation conflicts with retained binding",
            ));
        }
        let (configuration, _) =
            self.attachment
                .history_configuration_with_previous(&self.store, None, Some(basis))?;
        // This preparation supports only history before enrollment. A later complete transaction
        // must validate its required records explicitly; generic history open remains unchanged.
        let workspace =
            OpenWorkspace::open_attachment_store(self.metadata_path(), self.store.clone(), false)
                .map_err(|error| invalid(&error.to_string()))?;
        super::history::verify_history_binding(&workspace, &configuration)?;
        drop(workspace);
        if !recovering {
            let filesystem = self.store.filesystem();
            match read_private(self, STAGED) {
                Ok(staged) if staged == marker => {
                    filesystem.sync_file(Path::new(STAGED))?;
                }
                Ok(_) => return Err(invalid("conflicting dependency preparation stage")),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    filesystem.write_new_file(
                        Path::new(STAGED),
                        marker.as_bytes(),
                        Permissions::from_mode(0o600),
                    )?;
                }
                Err(error) => return Err(error),
            }
            self.attachment.ensure_current()?;
            self.store.ensure_namespace_identity()?;
            if read_private(self, HISTORY)? != previous {
                return Err(invalid("history binding changed during preparation"));
            }
            filesystem.rename(Path::new(STAGED), Path::new(HISTORY))?;
        }
        // Also mandatory for an exact retry after rename succeeded but sync/acknowledgement failed.
        sync(self)?;
        let fence = AttachmentDependencyFence {
            attachment: self.clone(),
            marker,
            _guard: guard,
        };
        fence.ensure_current()?;
        Ok(fence)
    }
}

impl AttachmentDependencyFence {
    /// Verify the retained source/store namespaces and exact required history binding.
    pub fn ensure_current(&self) -> io::Result<()> {
        self.attachment.attachment.ensure_current()?;
        self.attachment.store.ensure_namespace_identity()?;
        if read_receipt(&self.attachment.store)? != self.attachment.attachment.receipt()?.encode()
            || read_private(&self.attachment, HISTORY)? != self.marker
        {
            return Err(invalid("attachment dependency preparation changed"));
        }
        Ok(())
    }
}

pub(super) fn read_private(attachment: &ProvisionedAttachment, name: &str) -> io::Result<String> {
    read_private_in_store(&attachment.store, name)
}

pub(super) fn read_private_in_store(
    store: &crate::root_authority::PinnedWorkspaceRoot,
    name: &str,
) -> io::Result<String> {
    let file = store.filesystem().inspect_entry(Path::new(name))?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.permissions().mode() & 0o777 != 0o600
        || metadata.len() > MAX_BYTES
    {
        return Err(invalid(
            "dependency binding is not a bounded private regular file",
        ));
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() > MAX_BYTES as usize {
        return Err(invalid("dependency binding exceeds its bound"));
    }
    String::from_utf8(bytes).map_err(|_| invalid("invalid dependency binding encoding"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_attachment::{AttachmentStorage, ObservationLimits};
    use ed25519_dalek::{Signer as _, SigningKey};
    #[test]
    fn failed_ack_sync_retains_fence_and_exact_retry_requires_sync() {
        let root =
            std::env::temp_dir().join(format!("mesh-attachment-fence-sync-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let source = root.join("source");
        let metadata = root.join("metadata");
        std::fs::create_dir(&source).unwrap();
        std::fs::create_dir(&metadata).unwrap();
        std::fs::write(source.join("note"), b"saved").unwrap();
        let attachment = AttachmentStorage::open(&metadata)
            .unwrap()
            .provision(&source)
            .unwrap();
        let key = SigningKey::from_bytes(&[67; 32]);
        let input = attachment
            .project()
            .capture_inputs(ObservationLimits::default())
            .unwrap();
        attachment
            .project()
            .save_capture(
                attachment.metadata_path(),
                &input,
                mesh_types::PublicKey::from_bytes(key.verifying_key().to_bytes()),
                |payload| {
                    Ok::<_, &'static str>(mesh_types::Signature::from_bytes(
                        key.sign(payload.as_bytes()).to_bytes(),
                    ))
                },
            )
            .unwrap();
        let authority = RecordDigest::from_bytes([1; 32]);
        for _ in 0..2 {
            let called = std::cell::Cell::new(false);
            assert!(attachment
                .prepare_dependency_enrollment_with_sync(authority, |_| {
                    called.set(true);
                    Err(io::Error::other("injected acknowledgement sync failure"))
                })
                .is_err());
            assert!(called.get());
            assert!(attachment.saved_versions().is_err());
        }
        drop(attachment.prepare_dependency_enrollment(authority).unwrap());
        let before = read_private(&attachment, HISTORY).unwrap();
        std::fs::set_permissions(
            attachment.metadata_path().join(HISTORY),
            Permissions::from_mode(0o644),
        )
        .unwrap();
        assert!(attachment.prepare_dependency_enrollment(authority).is_err());
        std::fs::set_permissions(
            attachment.metadata_path().join(HISTORY),
            Permissions::from_mode(0o600),
        )
        .unwrap();
        assert_eq!(read_private(&attachment, HISTORY).unwrap(), before);
        drop(attachment);
        std::fs::remove_dir_all(root).unwrap();
    }
}
