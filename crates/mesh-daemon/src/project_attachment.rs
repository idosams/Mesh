//! Persistent, non-exclusive attachment to an existing project. Observation never writes source.
//!
//! The native caller chooses a private metadata directory outside the source project. This record
//! establishes observation identity only. Explicit signed saves use external history and grant no
//! source custody, approval or write-back authority. Explicit native integration and restoration
//! are separate single-use operations that preserve displaced work; restoration does not publish.

use std::fs;
use std::io::{self, Read as _};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use crate::ipc::Json;
use crate::root_authority::{PinnedWorkspaceRoot, ProtectedWorkspaceRoot};

const RECEIPT: &str = "attachment.json";
const SCHEMA: &str = "mesh.project-attachment/v1";
const MAX_RECEIPT_BYTES: u64 = 65_536;

mod dependency_closure;
pub use dependency_closure::NativeDependencyGraph;
mod dependency_reservation;
mod detachment;
mod fleet_pins;
mod progress_pins;
pub use progress_pins::ProgressPinState;
mod remote_connections;
mod remote_starts;
pub use remote_starts::RemoteStartRequest;
mod remote_fleet_pins;
mod remote_inbox;
pub use remote_connections::{RemoteConnectionSettings, RemoteConnectionSettingsState};
pub use remote_fleet_pins::RemoteFleetPinState;
mod review_outbox;
pub use review_outbox::FleetReviewOutbox;
mod pins;
pub use fleet_pins::{FleetCandidatePin, FleetPin, FleetPinState};
pub use pins::{AttachmentPin, AttachmentPinState};

mod provisioning;
pub use provisioning::{AttachmentStorage, ProvisionedAttachment, RegisteredAttachment};

mod background;
mod candidate_import;
mod candidate_review;
mod candidates;
mod capture_line;
pub use background::{
    AttachmentCaptureService, CaptureOutcome, CapturePhase, CaptureSchedule, CaptureStatus,
    NativeSignalState,
};
pub(crate) use candidates::CandidateAdmission;

mod approval;
mod dependency_decision;
mod dependency_grant;
mod grant_admission;
pub use grant_admission::{NativeGrantInspection, NativeGrantedFile, NativeGrantedInput};
mod work_decision;
pub use dependency_grant::{NativeInputGrant, NativeInputGrantRequest};
pub use work_decision::NativeWorkDecisionRequest;
mod dependency_work;
pub use dependency_work::NativeDependencyWorkBinding;
mod dependency_enrollment;
mod dependency_read;
mod dependency_transaction;
pub use dependency_decision::{NativeInputDecision, SavedInputDecision};
pub(crate) use dependency_read::VerifiedDependencyRead;
pub use dependency_transaction::NativeDependencyEnrollment;
mod group_integration;
mod history;
pub use dependency_enrollment::AttachmentDependencyFence;
pub use history::{NativeCaptureRetention, PreparedNativeCapture};
mod inspection;
mod integration;
mod remote_input;
pub use group_integration::PreparedMainIntegration;
mod lanes;
mod recovery;
mod restoration;
pub use restoration::PreparedRetainedRestoration;
mod entry_restoration;
pub use entry_restoration::PreparedRetainedEntryRestoration;
mod writeback;
pub use writeback::PreparedMainFileIntegration;
mod directory_writeback;
pub use directory_writeback::{PreparedMainDirectoryAddition, PreparedMainDirectoryChange};
mod reviews;
pub use history::SavedAttachmentVersion;

mod observation;
pub use observation::{CapturedFileInput, CapturedProjectInput, ObservationLimits};

/// An admitted existing folder. It remains writable by the user's ordinary tools.
#[derive(Clone)]
pub struct ProjectAttachment {
    root: PathBuf,
    pinned: PinnedWorkspaceRoot,
    device: u64,
    inode: u64,
}

impl ProjectAttachment {
    /// Register a project in an existing private metadata directory, without modifying the project.
    /// Repeating the same registration is idempotent; an existing different receipt is preserved.
    pub fn register(root: &Path, metadata: &Path) -> io::Result<Self> {
        let attached = Self::admit(root)?;
        let store = external_store(metadata, &attached)?;
        attached.register_in_store(&store)?;
        Ok(attached)
    }

    fn register_in_store(&self, store: &PinnedWorkspaceRoot) -> io::Result<()> {
        let encoded = self.receipt()?.encode();
        self.ensure_current()?;
        match store.filesystem().write_new_file(
            Path::new(RECEIPT),
            encoded.as_bytes(),
            fs::Permissions::from_mode(0o600),
        ) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                if read_receipt(store)? != encoded {
                    return Err(invalid(
                        "attachment metadata already belongs to another project",
                    ));
                }
            }
            Err(error) => return Err(error),
        }
        // A failed final identity check preserves the receipt for explicit recovery, never deletion.
        store.ensure_namespace_identity()?;
        self.ensure_current()?;
        Ok(())
    }

    /// Reopen an existing registration and verify the named folder still has its admitted identity.
    /// Missing source folders are never recreated or silently rebound to a replacement.
    pub fn reopen(metadata: &Path) -> io::Result<Self> {
        Self::reopen_store(metadata, pin_absolute_directory(metadata)?)
    }

    fn reopen_store(metadata: &Path, store: PinnedWorkspaceRoot) -> io::Result<Self> {
        let encoded = read_receipt(&store)?;
        let receipt = Json::parse(&encoded).map_err(|_| invalid("invalid attachment receipt"))?;
        if receipt.get("schema").and_then(Json::as_text) != Some(SCHEMA) {
            return Err(invalid("unsupported attachment receipt"));
        }
        let root = receipt
            .get("root")
            .and_then(Json::as_text)
            .ok_or_else(|| invalid("missing attachment root"))?;
        let attached = Self::admit(Path::new(root))?;
        // Canonical equality checks every identity field and rejects unknown fields/schema changes.
        if attached.receipt()?.encode() != encoded {
            return Err(invalid("attached project identity changed"));
        }
        let checked = external_store(metadata, &attached)?;
        if checked.identity()? != store.identity()? {
            return Err(invalid("attachment metadata identity changed"));
        }
        store.ensure_namespace_identity()?;
        attached.ensure_current()?;
        Ok(attached)
    }

    fn admit(root: &Path) -> io::Result<Self> {
        let pinned = pin_absolute_directory(root)?;
        let (device, inode) = pinned.identity()?;
        let root = root.canonicalize()?;
        let canonical = pin_absolute_directory(&root)?;
        canonical.ensure_identity(device, inode)?;
        pinned.ensure_namespace_identity()?;
        Ok(Self {
            root,
            pinned: canonical,
            device,
            inode,
        })
    }

    /// The original project location, not a newly provisioned working copy.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Check both the retained descriptor and its current pathname before observing the project.
    pub fn ensure_current(&self) -> io::Result<()> {
        self.pinned.ensure_identity(self.device, self.inode)?;
        self.pinned.ensure_namespace_identity()
    }

    /// Native-only stable folder reference for an OS opener. No renderer-selected path is used.
    pub fn native_folder_reference(&self) -> io::Result<PathBuf> {
        self.ensure_current()?;
        let reference = ProtectedWorkspaceRoot::from_directory_token(&format!(
            "{:016x}:{:016x}",
            self.device, self.inode,
        ))?
        .stable_reference()?;
        self.ensure_current()?;
        Ok(reference)
    }

    /// Native registration status. Observation/capture availability is explicit, not inferred.
    pub fn status(&self) -> io::Result<Json> {
        self.ensure_current()?;
        Ok(Json::object([
            ("schema", Json::text(SCHEMA)),
            (
                "root",
                Json::text(
                    self.root
                        .to_str()
                        .ok_or_else(|| invalid("project path is not UTF-8"))?,
                ),
            ),
            ("mode", Json::text("non-exclusive")),
            ("registered", Json::Bool(true)),
            ("observation", Json::text("not-started")),
            ("saved_version", Json::Null),
            ("exclusive_custody", Json::Bool(false)),
        ]))
    }

    fn receipt(&self) -> io::Result<Json> {
        Ok(Json::object([
            ("schema", Json::text(SCHEMA)),
            (
                "root",
                Json::text(
                    self.root
                        .to_str()
                        .ok_or_else(|| invalid("project path is not UTF-8"))?,
                ),
            ),
            ("device", Json::text(format!("{:016x}", self.device))),
            ("inode", Json::text(format!("{:016x}", self.inode))),
        ]))
    }
}

fn pin_absolute_directory(path: &Path) -> io::Result<PinnedWorkspaceRoot> {
    if !path.is_absolute() || fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(invalid(
            "attachment directories must be absolute real directories",
        ));
    }
    let pinned = PinnedWorkspaceRoot::open(path.to_path_buf())?;
    pinned.ensure_namespace_identity()?;
    Ok(pinned)
}

fn external_store(path: &Path, attached: &ProjectAttachment) -> io::Result<PinnedWorkspaceRoot> {
    let store = pin_absolute_directory(path)?;
    let source = ProtectedWorkspaceRoot::from_directory_token(&format!(
        "{:016x}:{:016x}",
        attached.device, attached.inode
    ))?;
    if store.is_within(source)? {
        return Err(invalid(
            "attachment metadata must remain outside the project",
        ));
    }
    Ok(store)
}

fn read_receipt(store: &PinnedWorkspaceRoot) -> io::Result<String> {
    let file = store.filesystem().inspect_entry(Path::new(RECEIPT))?;
    if !file.metadata()?.is_file() {
        return Err(invalid("attachment receipt is not a regular file"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_RECEIPT_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_RECEIPT_BYTES {
        return Err(invalid("attachment receipt is too large"));
    }
    String::from_utf8(bytes).map_err(|_| invalid("attachment receipt is not UTF-8"))
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

mod remote_project_outbox;
pub use remote_project_outbox::{validate_remote_project_action, RemoteProjectOutbox};
