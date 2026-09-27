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
        let binding = super::WorkspaceBinding {
            source_version: input.version,
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
