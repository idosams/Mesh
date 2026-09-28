//! Per-lane native contexts. Desktop selection never routes these operations.

use std::path::Path;
use std::sync::Arc;

use crate::ipc::{
    nothing_to_recover, Json, Operations, StartupSummary, Unavailable, WorkspaceSummary,
};
use crate::{
    CheckpointRuntimeParameters, LiveDaemon, TrustedReviewers, WorkspaceVersionForkRequest,
};
use mesh_store::RecordDigest;

/// Native-verified source workspace and exact immutable version to allocate from.
///
/// The service authorizes this input before calling the allocator. Supplying these fields is not
/// proof of actor authority. Reopening and forking check all identities against native state.
#[derive(Clone, Debug)]
pub struct VersionInput {
    /// Exact source working root, selected by the native service.
    pub root: String,
    /// Source workspace digest.
    pub digest: String,
    /// Source installation identity; protects against a substituted folder at the same path.
    pub installation: String,
    /// Immutable saved operation to materialize.
    pub version: RecordDigest,
}

/// Retained native history with no live daemon, credentials, checkpoint timers or execution custody.
/// Only the native allocator can construct this read context.
pub struct LaneHistory {
    pub(super) open: crate::workspace::OpenWorkspace,
    pub(super) parents: Vec<crate::root_authority::PinnedWorkspaceRoot>,
    pub(super) allocation: crate::ProtectedWorkspaceRoot,
}
impl LaneHistory {
    pub(super) fn verify(&self) -> Result<(), Unavailable> {
        for parent in &self.parents {
            parent.ensure_namespace_identity().map_err(|_| {
                Unavailable::new(
                    "fleet-history-root-changed",
                    "The retained lane directory changed.",
                )
            })?;
        }
        for root in [self.open.pinned_root(), self.open.storage_pinned_root()] {
            if !root.is_within(self.allocation).unwrap_or(false) {
                return Err(Unavailable::new(
                    "fleet-history-root-changed",
                    "The retained history left its allocated lane.",
                ));
            }
        }
        self.open.ensure_physical_root().map_err(|_| {
            Unavailable::new(
                "fleet-history-root-changed",
                "The retained lane directory changed.",
            )
        })
    }
}

/// A lane's independent service instance and the native receipt describing its allocation.
///
/// Allocation does not launch a process or acquire agent custody. The scheduler must do those
/// steps, durably bind this receipt to the lane, and only then expose the folder to a worker.
#[derive(Debug)]
pub struct LaneWorkspace {
    daemon: Arc<LiveDaemon>,
    receipt: Json,
    binding: super::WorkspaceBinding,
}
impl LaneWorkspace {
    /// Create an independent folder using the existing exact-version native transaction.
    ///
    /// `destination` and `protected_roots` are service-owned path decisions, never agent or
    /// renderer authority. Create-only allocation preserves an existing destination on failure.
    /// A crash after native creation requires receipt reconciliation, not destructive retry.
    pub fn fork(
        input: &VersionInput,
        destination: &Path,
        protected_roots: &[crate::ProtectedWorkspaceRoot],
        reviewers: TrustedReviewers,
        checkpoint: CheckpointRuntimeParameters,
    ) -> Result<Self, Unavailable> {
        Self::fork_inner(
            input,
            destination,
            protected_roots,
            reviewers,
            checkpoint,
            None,
        )
    }

    pub(super) fn fork_inner(
        input: &VersionInput,
        destination: &Path,
        protected_roots: &[crate::ProtectedWorkspaceRoot],
        reviewers: TrustedReviewers,
        checkpoint: CheckpointRuntimeParameters,
        parent: Option<crate::ProtectedWorkspaceRoot>,
    ) -> Result<Self, Unavailable> {
        let daemon = Arc::new(
            LiveDaemon::with_trusted_reviewers_and_checkpoint_runtime(
                StartupSummary::from(&nothing_to_recover()),
                reviewers,
                checkpoint,
            )
            .map_err(|_| {
                Unavailable::new(
                    "fleet-checkpoint-configuration",
                    "The lane checkpoint configuration is invalid.",
                )
            })?,
        );
        // Reopen, never initialize: a removed source must not turn into an empty workspace.
        daemon
            .reopen_at_start(Path::new(&input.root))
            .map_err(|_| {
                Unavailable::new(
                    "fleet-source-unavailable",
                    "The exact source workspace is unavailable.",
                )
            })?;
        let version = input.version.to_string();
        let destination = destination.to_str().ok_or_else(|| {
            Unavailable::new(
                "fleet-destination-invalid",
                "The lane destination cannot be represented.",
            )
        })?;
        let request = WorkspaceVersionForkRequest::new(
            &version,
            destination,
            &input.root,
            &input.digest,
            &input.installation,
            None,
        )
        .protecting(protected_roots);
        let request = if let Some(parent) = parent {
            request.within_parent(parent)
        } else {
            request
        };
        let receipt = daemon.fork_workspace_version_protected(request)?;
        let state = daemon.workspace_state()?;
        let [initial] = state.workspace_versions.as_slice() else {
            return Err(Unavailable::new(
                "fleet-initial-version-unavailable",
                "The lane initial version could not be bound.",
            ));
        };
        let starting_version = initial.operation();
        let binding = super::WorkspaceBinding {
            source_version: input.version,
            starting_version: Some(starting_version),
            root: state.root,
            digest: state.digest,
            installation: state.installation,
        };
        Ok(Self {
            daemon,
            receipt,
            binding,
        })
    }

    pub(super) fn from_attachment(
        source: &crate::project_attachment::ProvisionedAttachment,
        version: RecordDigest,
        allocation: &crate::root_authority::PinnedWorkspaceRoot,
        path: &Path,
        reviewers: TrustedReviewers,
        checkpoint: CheckpointRuntimeParameters,
        protected: &[crate::ProtectedWorkspaceRoot],
    ) -> Result<Self, Unavailable> {
        let refusal = || {
            Unavailable::new("fleet-attachment-allocation-unavailable", "The attached version could not be allocated. Retained files require recovery before retrying.")
        };
        let files = allocation
            .create_child_directory(std::ffi::OsStr::new("source"))
            .map_err(|_| refusal())?;
        let snapshot_path = path.join("source");
        let snapshot = source
            .materialize_saved_version(&version.to_string(), &files, &snapshot_path)
            .map_err(|_| refusal())?;
        let (device, inode) = allocation.identity().map_err(|_| refusal())?;
        let parent = crate::ProtectedWorkspaceRoot::from_directory_token(&format!(
            "{device:016x}:{inode:016x}"
        ))
        .map_err(|_| refusal())?;
        let prepared = crate::PreparedFolderImport::prepare_presented_with_parent(
            &snapshot_path,
            &path.join("workspace.mesh"),
            protected,
            Some(parent),
        )
        .map_err(|_| refusal())?;
        let daemon = Arc::new(
            LiveDaemon::with_trusted_reviewers_and_checkpoint_runtime(
                StartupSummary::from(&nothing_to_recover()),
                reviewers,
                checkpoint,
            )
            .map_err(|_| refusal())?,
        );
        let state = daemon.install_attached_lane(prepared, &snapshot)?;
        source.protected_source().map_err(|_| refusal())?;
        files.ensure_namespace_identity().map_err(|_| refusal())?;
        allocation
            .ensure_namespace_identity()
            .map_err(|_| refusal())?;
        let receipt = Json::object([
            ("schema", Json::text("mesh.fleet-attachment-allocation/v1")),
            ("source_project", Json::text(source.id())),
            ("source_version", Json::text(version.to_string())),
            ("workspace", state.to_json()),
        ]);
        let [initial] = state.workspace_versions.as_slice() else {
            return Err(Unavailable::new(
                "fleet-initial-version-unavailable",
                "The lane initial version could not be bound.",
            ));
        };
        let starting_version = initial.operation();
        let binding = super::WorkspaceBinding {
            source_version: version,
            starting_version: Some(starting_version),
            root: state.root,
            digest: state.digest,
            installation: state.installation,
        };
        Ok(Self {
            daemon,
            receipt,
            binding,
        })
    }

    /// Verified allocation identity to commit before any dispatch intent.
    pub fn binding(&self) -> &super::WorkspaceBinding {
        &self.binding
    }

    /// Independent lane service. Navigation in another daemon cannot change this workspace.
    pub fn daemon(&self) -> &Arc<LiveDaemon> {
        &self.daemon
    }

    /// Exact native allocation receipt to bind to durable lane state before worker dispatch.
    pub fn receipt(&self) -> &Json {
        &self.receipt
    }

    /// Current lane state without consulting the desktop-selected workspace.
    pub fn state(&self) -> Result<WorkspaceSummary, Unavailable> {
        self.daemon.workspace_state()
    }
}

#[cfg(test)]
mod attachment_tests {
    use super::*;
    use crate::project_attachment::{AttachmentStorage, ObservationLimits};
    use crate::root_authority::PinnedWorkspaceRoot;
    use ed25519_dalek::{Signer as _, SigningKey};
    use std::{fs, time::Duration};

    #[test]
    fn retained_history_refuses_ancestor_alias_after_directory_leaves_its_lane() {
        use crate::workspace::OpenWorkspace;
        let root =
            std::env::temp_dir().join(format!("mesh-history-ancestor-{}", std::process::id()));
        let lane = root.join("lane");
        let wrapper = lane.join("wrapper");
        let working = wrapper.join("working");
        fs::create_dir_all(&working).unwrap();
        let initialized = OpenWorkspace::open(&working).unwrap();
        let installation = initialized.installation();
        drop(initialized);
        let pinned = PinnedWorkspaceRoot::open(lane.clone()).unwrap();
        let (device, inode) = pinned.identity().unwrap();
        let allocation = crate::ProtectedWorkspaceRoot::from_directory_token(&format!(
            "{device:016x}:{inode:016x}"
        ))
        .unwrap();
        let history = LaneHistory {
            open: OpenWorkspace::reopen_history(
                &working,
                &installation,
                allocation,
                &TrustedReviewers::default(),
            )
            .unwrap(),
            parents: vec![pinned],
            allocation,
        };
        history.verify().unwrap();
        let outside = root.join("outside");
        fs::rename(&wrapper, &outside).unwrap();
        std::os::unix::fs::symlink(&outside, &wrapper).unwrap();
        assert!(
            history.open.ensure_physical_root().is_ok(),
            "final directory identities alone do not prove retained ancestry"
        );
        assert!(history.verify().is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn changed_staging_is_preserved_but_never_admitted_as_the_saved_input() {
        let root =
            std::env::temp_dir().join(format!("mesh-fleet-staging-race-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let original = root.join("original");
        let metadata = root.join("metadata");
        let staging = root.join("staging");
        for path in [&original, &metadata, &staging] {
            fs::create_dir(path).unwrap();
        }
        fs::write(original.join("work"), "saved input").unwrap();
        let source = AttachmentStorage::open(&metadata)
            .unwrap()
            .provision(&original)
            .unwrap();
        let inputs = source
            .project()
            .capture_inputs(ObservationLimits::default())
            .unwrap();
        let key = SigningKey::from_bytes(&[19; 32]);
        let version = source
            .project()
            .save_capture(
                source.metadata_path(),
                &inputs,
                mesh_types::PublicKey::from_bytes(key.verifying_key().to_bytes()),
                |body| {
                    Ok::<_, String>(mesh_types::Signature::from_bytes(
                        key.sign(body.as_bytes()).to_bytes(),
                    ))
                },
            )
            .unwrap()
            .operation()
            .to_string();
        let pinned = PinnedWorkspaceRoot::open(staging.clone()).unwrap();
        let snapshot = source
            .materialize_saved_version(&version, &pinned, &staging)
            .unwrap();
        fs::write(staging.join("work"), "concurrent staging change").unwrap();
        let prepared = crate::PreparedFolderImport::prepare_presented_with_parent(
            &staging,
            &root.join("workspace.mesh"),
            &[],
            None,
        )
        .unwrap();
        let daemon = LiveDaemon::with_checkpoint_runtime(
            StartupSummary::from(&nothing_to_recover()),
            CheckpointRuntimeParameters {
                idle_interval: Some(Duration::from_millis(10)),
                maximum_uncheckpointed_bytes: Some(65_536),
                maximum_uncheckpointed_interval: Some(Duration::from_secs(60)),
            },
        )
        .unwrap();
        let result = daemon.install_attached_lane(prepared, &snapshot);
        assert!(
            result.is_err(),
            "a valid import is insufficient: it must match the saved attachment"
        );
        assert!(daemon.workspace_state().is_err());
        assert_eq!(
            fs::read(staging.join("work")).unwrap(),
            b"concurrent staging change"
        );
        assert_eq!(fs::read(original.join("work")).unwrap(), b"saved input");
        assert!(root.join("workspace.mesh").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
