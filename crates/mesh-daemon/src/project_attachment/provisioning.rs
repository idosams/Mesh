//! Native allocation of external attachment storage. Renderer code never chooses a store path.

use super::{
    external_store, invalid, pin_absolute_directory, read_receipt, AttachmentCaptureService,
    CaptureSchedule, ProjectAttachment,
};
use crate::ipc::Json;
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
    pub(super) pinned: PinnedWorkspaceRoot,
}

/// A registered project and its exact native store authority, ready for capture.
#[derive(Clone)]
pub struct ProvisionedAttachment {
    attachment: ProjectAttachment,
    metadata: PathBuf,
    store: PinnedWorkspaceRoot,
    id: String,
}

/// Persistent registration information; discovery does not require the original project online.
#[derive(Clone)]
pub struct RegisteredAttachment {
    id: String,
    root: PathBuf,
    detached: bool,
}
impl RegisteredAttachment {
    /// Capture was explicitly disabled without removing history.
    pub fn detached(&self) -> bool {
        self.detached
    }
    /// Native identifier bound to the complete registration receipt.
    pub fn id(&self) -> &str {
        &self.id
    }
    /// Original registered location, not permission to adopt a replacement folder.
    pub fn root(&self) -> &Path {
        &self.root
    }
}
fn receipt_id(receipt: &str) -> String {
    Blake3::digest_bytes(receipt.as_bytes())
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
pub(super) fn valid_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn registered_root(receipt: &str) -> io::Result<PathBuf> {
    let parsed = Json::parse(receipt).map_err(|_| invalid("invalid attachment receipt"))?;
    let root = parsed
        .get("root")
        .and_then(Json::as_text)
        .ok_or_else(|| invalid("missing project root"))?;
    if !Path::new(root).is_absolute() {
        return Err(invalid("project root is not absolute"));
    }
    let device = parsed
        .get("device")
        .and_then(Json::as_text)
        .ok_or_else(|| invalid("missing device"))?;
    let inode = parsed
        .get("inode")
        .and_then(Json::as_text)
        .ok_or_else(|| invalid("missing inode"))?;
    if [device, inode].iter().any(|value| {
        value.len() != 16
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }) {
        return Err(invalid("invalid project identity"));
    }
    if Json::object([
        ("schema", Json::text(super::SCHEMA)),
        ("root", Json::text(root)),
        ("device", Json::text(device)),
        ("inode", Json::text(inode)),
    ])
    .encode()
        != receipt
    {
        return Err(invalid("noncanonical attachment receipt"));
    }
    Ok(PathBuf::from(root))
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

    /// Discover bounded canonical registrations without creating stores or requiring sources online.
    /// Invalid or partial registrations are preserved and reported as an error, never silently lost.
    pub fn registrations(&self) -> io::Result<Vec<RegisteredAttachment>> {
        self.pinned.ensure_namespace_identity()?;
        let names = self
            .pinned
            .filesystem()
            .read_directory_names_bounded(Path::new(""), 256)?;
        let mut registrations = Vec::new();
        for name in names {
            let Some(name) = name.to_str() else {
                return Err(invalid("invalid catalog entry name"));
            };
            let Some(id) = name.strip_prefix("project-") else {
                continue;
            };
            if !valid_id(id) {
                return Err(invalid("invalid catalog project identity"));
            }
            let store = self.pinned.open_child_directory(OsStr::new(name))?;
            let receipt = read_receipt(&store)?;
            if receipt_id(&receipt) != id {
                return Err(invalid("catalog receipt identity changed"));
            }
            let root = registered_root(&receipt)?;
            store.ensure_namespace_identity()?;
            registrations.push(RegisteredAttachment {
                id: id.to_owned(),
                root,
                detached: super::detachment::detached(&store)?,
            });
        }
        self.pinned.ensure_namespace_identity()?;
        Ok(registrations)
    }

    /// Set capture opt-out for an existing receipt, even while its source is offline.
    /// Callers stop and join their owned workers before acknowledging detachment.
    pub fn set_detached(&self, id: &str, detached: bool) -> io::Result<()> {
        if !valid_id(id) {
            return Err(invalid("invalid catalog project identity"));
        }
        let name = format!("project-{id}");
        let store = self.pinned.open_child_directory(OsStr::new(&name))?;
        if receipt_id(&read_receipt(&store)?) != id {
            return Err(invalid("catalog receipt identity changed"));
        }
        super::detachment::set_detached(&store, detached)?;
        self.pinned.ensure_namespace_identity()
    }

    /// Reopen only an exact existing registration through the retained catalog directory.
    pub fn reopen(&self, id: &str) -> io::Result<ProvisionedAttachment> {
        if !valid_id(id) {
            return Err(invalid("invalid catalog project identity"));
        }
        let name = format!("project-{id}");
        let store = self.pinned.open_child_directory(OsStr::new(&name))?;
        let receipt = read_receipt(&store)?;
        if receipt_id(&receipt) != id {
            return Err(invalid("catalog receipt identity changed"));
        }
        let metadata = self.path.join(name);
        let attachment = ProjectAttachment::reopen_store(&metadata, store.clone())?;
        self.pinned.ensure_namespace_identity()?;
        Ok(ProvisionedAttachment {
            attachment,
            metadata,
            store,
            id: id.to_owned(),
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
        let id = receipt_id(&receipt);
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

    /// Record a review before any shared main exists; no approval or source-write authority.
    pub fn request_review(&self, target: &str, actor: mesh_types::PublicKey) -> io::Result<Json> {
        self.request_review_with_trusted_reviewers(
            target,
            actor,
            &crate::TrustedReviewers::default(),
        )
    }

    /// Record an exact review against verified main using native reviewer configuration.
    pub fn request_review_with_trusted_reviewers(
        &self,
        target: &str,
        actor: mesh_types::PublicKey,
        trusted: &crate::TrustedReviewers,
    ) -> io::Result<Json> {
        self.attachment.request_saved_review(
            &self.metadata,
            self.store.clone(),
            target,
            actor,
            trusted,
        )
    }

    /// List the bounded review queue without claiming approval authority.
    pub fn reviews(&self) -> io::Result<Json> {
        self.reviews_with_trusted_reviewers(&crate::TrustedReviewers::default())
    }

    /// List review content against verified publication history; omitted cards remain explicit.
    pub fn reviews_with_trusted_reviewers(
        &self,
        trusted: &crate::TrustedReviewers,
    ) -> io::Result<Json> {
        self.attachment
            .saved_reviews(&self.metadata, self.store.clone(), trusted)
    }

    /// Read one exact durable review even outside the overview page, without approval authority.
    pub fn review(&self, bundle: &str, target: &str) -> io::Result<Json> {
        self.review_with_trusted_reviewers(bundle, target, &crate::TrustedReviewers::default())
    }

    /// Read exact review content with native trust for its historical base.
    pub fn review_with_trusted_reviewers(
        &self,
        bundle: &str,
        target: &str,
        trusted: &crate::TrustedReviewers,
    ) -> io::Result<Json> {
        self.attachment
            .saved_review(&self.metadata, self.store.clone(), bundle, target, trusted)
    }

    /// Verify accepted Mesh main independently of the mutable source folder.
    pub fn main_version(
        &self,
        trusted: &crate::TrustedReviewers,
    ) -> io::Result<Option<mesh_approval::HeadId>> {
        self.attachment.with_review_history(
            &self.metadata,
            self.store.clone(),
            trusted,
            |workspace, _| super::approval::main_head(workspace),
        )
    }

    /// Resolve verified Mesh main to its exact saved version and review, independently of queue bounds.
    pub fn accepted_main(&self, trusted: &crate::TrustedReviewers) -> io::Result<Json> {
        self.attachment.with_review_history(
            &self.metadata,
            self.store.clone(),
            trusted,
            |workspace, _| {
                let Some(head) = super::approval::main_head(workspace)? else {
                    return Ok(Json::Null);
                };
                let review = workspace
                    .accepted_main_review()
                    .map_err(io::Error::other)?
                    .ok_or_else(|| invalid("verified main review unavailable"))?;
                Ok(Json::object([
                    ("head", Json::text(head.to_string())),
                    ("bundle", Json::text(review.bundle.to_string())),
                    ("target", Json::text(review.subject_operation.to_string())),
                ]))
            },
        )
    }

    /// Compare current source with the accepted review's exact base and result. This read-only
    /// observation never authorizes writes or claims an atomic snapshot of the working folder.
    pub fn preview_main_integration(
        &self,
        bundle: &str,
        target: &str,
        trusted: &crate::TrustedReviewers,
        limits: super::ObservationLimits,
    ) -> io::Result<Json> {
        self.attachment.preview_main_integration(
            &self.metadata,
            self.store.clone(),
            bundle,
            target,
            trusted,
            limits,
        )
    }

    /// Stage one approved regular-file replacement in native-configured external recovery storage.
    /// This changes no source files. Applying the returned single-use proposal is a separate,
    /// explicit native operation; no agent tool or renderer command exposes it.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare_main_file_integration(
        &self,
        bundle: &str,
        target: &str,
        relative: &str,
        recovery_root: &Path,
        trusted: &crate::TrustedReviewers,
        limits: super::ObservationLimits,
    ) -> io::Result<super::PreparedMainFileIntegration> {
        super::writeback::prepare(
            self.clone(),
            self.store.clone(),
            bundle,
            target,
            relative,
            recovery_root,
            trusted,
            limits,
        )
    }

    /// Inspect retained integration evidence after restart without replay, cleanup or writes.
    /// A selected transaction bypasses the bounded directory overview. The native host supplies
    /// the external recovery root; its pathname never comes from renderer or agent authority.
    pub fn inspect_integration_recovery(
        &self,
        recovery_root: &Path,
        transaction: Option<&str>,
        trusted: &crate::TrustedReviewers,
        limits: super::ObservationLimits,
    ) -> io::Result<Json> {
        super::recovery::inspect_recovery(
            self,
            self.store.clone(),
            recovery_root,
            transaction,
            trusted,
            limits,
        )
    }

    /// Prepare an explicitly confirmed restoration of retained editor work. It creates a new
    /// recovery transaction and grants no approval or main advancement authority.
    pub fn prepare_retained_restoration(
        &self,
        recovery_root: &Path,
        transaction: &str,
        trusted: &crate::TrustedReviewers,
        limits: super::ObservationLimits,
    ) -> io::Result<super::PreparedRetainedRestoration> {
        super::restoration::prepare(
            self,
            self.store.clone(),
            recovery_root,
            transaction,
            trusted,
            limits,
        )
    }

    /// Prepare exact native approval facts. Signing and human confirmation happen outside the lock.
    pub fn approval_preview(
        &self,
        bundle: &str,
        target: &str,
        trusted: &crate::TrustedReviewers,
    ) -> io::Result<crate::HumanApprovalPreview> {
        self.attachment.prepare_approval(
            &self.metadata,
            self.store.clone(),
            bundle,
            target,
            trusted,
        )
    }

    /// Verify an exact human receipt and advance only Mesh main, never source files or Git.
    pub fn approve_review(
        &self,
        bundle: &str,
        target: &str,
        receipt: &[u8],
        trusted: &crate::TrustedReviewers,
    ) -> io::Result<mesh_approval::HeadId> {
        self.attachment.approve_saved_review(
            &self.metadata,
            self.store.clone(),
            bundle,
            target,
            receipt,
            trusted,
        )
    }

    /// Inspect one bounded page of entries in an exact saved version, never live files.
    pub fn inspect_entries(
        &self,
        operation: &str,
        after: Option<&str>,
    ) -> io::Result<crate::ipc::Json> {
        self.attachment
            .inspect_entries(&self.metadata, self.store.clone(), operation, after)
    }

    /// Preview exact saved UTF-8 text up to 256 KiB; binary and larger files return metadata only.
    pub fn inspect_text(&self, operation: &str, path: &str) -> io::Result<crate::ipc::Json> {
        self.attachment
            .inspect_text(&self.metadata, self.store.clone(), operation, path)
    }

    /// Compare exact saved versions by path, content and executable metadata, with bounded paging.
    pub fn compare_versions(
        &self,
        base: &str,
        target: &str,
        after: Option<&str>,
    ) -> io::Result<crate::ipc::Json> {
        self.attachment.compare_saved(
            &self.metadata,
            self.store.clone(),
            base,
            target,
            (after, None),
        )
    }

    /// Resolve one changed path in an exact comparison, independently of the current page.
    pub fn comparison_path(
        &self,
        base: &str,
        target: &str,
        path: &str,
    ) -> io::Result<crate::ipc::Json> {
        self.attachment.compare_saved(
            &self.metadata,
            self.store.clone(),
            base,
            target,
            (None, Some(path)),
        )
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
