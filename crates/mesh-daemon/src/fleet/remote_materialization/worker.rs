//! Independent native workspace from one originally reserved, fully verified remote input.
use super::*;
use crate::fleet::{RemoteAdmissionReceipt, WorkspaceBinding};
use crate::ipc::{nothing_to_recover, Json, Operations as _, StartupSummary};
use crate::{CheckpointRuntimeParameters, LiveDaemon, TrustedReviewers};
use std::sync::Arc;

const INTENT: &str = "initialization.json";
const RECEIPT: &str = "workspace.json";

/// Received source and independently initialized worker history, with durable explicit provenance.
/// This is not a process launch permit. Dropping the handle leaves all files and receipts intact.
pub struct ReceivedWorkerWorkspace {
    input: RemoteInputAllocation,
    daemon: Arc<LiveDaemon>,
    binding: WorkspaceBinding,
    receipt: Json,
    intent: String,
    admission: RemoteAdmissionReceipt,
}
impl ReceivedWorkerWorkspace {
    /// Retained coordinator/objective/assignment attribution from the original input reservation.
    pub fn admission(&self) -> &RemoteAdmissionReceipt {
        &self.admission
    }
    /// Worker installation and independent initial operation; never the remote source operation.
    pub fn binding(&self) -> &WorkspaceBinding {
        &self.binding
    }
    /// Independent native service. A supervisor must separately authorize credentials and launch.
    pub fn daemon(&self) -> &Arc<LiveDaemon> {
        &self.daemon
    }
    /// Complete durable mapping, also retained in the private allocation's workspace.json.
    pub fn receipt(&self) -> &Json {
        &self.receipt
    }
    /// Recheck the original input, allocation custody, current installation and retained initial
    /// operation. Later working edits do not rewrite the original source-to-worker mapping.
    pub fn verify(&self) -> io::Result<()> {
        self.input.verify()?;
        let state = self.daemon.workspace_state().map_err(|_| invalid())?;
        if state.root != self.binding.root
            || state.installation != self.binding.installation
            || !state
                .workspace_versions
                .iter()
                .any(|version| Some(version.operation()) == self.binding.starting_version)
        {
            return Err(invalid());
        }
        self.daemon
            .verify_received_lane_binding(
                token(&self.input.allocation)?,
                &self.binding.root,
                &self.binding.installation,
                self.binding.starting_version.ok_or_else(invalid)?,
                &self.input.manifest,
            )
            .map_err(|_| invalid())?;
        let receipt = self.receipt.encode();
        for (name, expected) in [(INTENT, self.intent.as_str()), (RECEIPT, receipt.as_str())] {
            let file = self
                .input
                .allocation
                .filesystem()
                .read_only()
                .read_file(Path::new(name))?;
            let metadata = file.metadata()?;
            if metadata.nlink() != 1 || metadata.permissions().mode() & 0o077 != 0 {
                return Err(invalid());
            }
            let mut bytes = Vec::new();
            file.take(expected.len() as u64 + 1)
                .read_to_end(&mut bytes)?;
            if bytes != expected.as_bytes() {
                return Err(invalid());
            }
        }
        self.input.verify_roots()
    }
}

impl RemoteInputAllocation {
    /// Consume an originally reserved input and initialize one fixed, independent worker workspace.
    /// Generic materializations without a retained admission cannot take this route. A create-only
    /// intent is synced before import, then an exact source/worker mapping after native verification.
    /// Failed or interrupted work is preserved; no retry, process adoption or launch is authorized.
    pub fn into_worker_workspace(
        mut self,
        reviewers: TrustedReviewers,
        checkpoint: CheckpointRuntimeParameters,
    ) -> io::Result<ReceivedWorkerWorkspace> {
        self.verify()?;
        let admission = self.admission.take().ok_or_else(invalid)?;
        let assignment = &admission.work().assignment;
        if assignment.input != self.manifest.input() || assignment.bundle != self.manifest.bundle()
        {
            return Err(invalid());
        }
        let parent_path = self.path.parent().ok_or_else(invalid)?;
        if parent_path.file_name() != Some(OsStr::new(&format!("input-{}", admission.allocation())))
        {
            return Err(invalid());
        }
        let daemon = Arc::new(
            LiveDaemon::with_trusted_reviewers_and_checkpoint_runtime(
                StartupSummary::from(&nothing_to_recover()),
                reviewers,
                checkpoint,
            )
            .map_err(|_| invalid())?,
        );
        let context = Json::object([
            ("coordinator", Json::text(admission.coordinator())),
            ("objective", Json::text(admission.objective())),
            ("assignment", Json::text(&assignment.id)),
            ("worker", Json::text(&assignment.worker_key)),
            ("lane", Json::text(&admission.work().lane)),
            ("run", Json::text(&admission.work().run)),
            ("admission_revision", Json::Number(admission.revision())),
            ("allocation", Json::text(admission.allocation())),
            (
                "allocation_installation",
                Json::text(token(&self.allocation)?.directory_token()),
            ),
            (
                "input_installation",
                Json::text(token(&self.files)?.directory_token()),
            ),
            (
                "source_input",
                Json::text(self.manifest.input().to_string()),
            ),
            (
                "source_bundle",
                Json::text(self.manifest.bundle().to_string()),
            ),
        ]);
        let intent = Json::object([
            ("schema", Json::text("mesh.received-workspace-intent/v1")),
            ("context", context.clone()),
        ])
        .encode();
        self.allocation.filesystem().write_new_file(
            Path::new(INTENT),
            intent.as_bytes(),
            fs::Permissions::from_mode(0o600),
        )?;
        self.allocation.sync()?;
        self.verify()?;
        let prepared = crate::PreparedFolderImport::prepare_received_with_parent(
            &self.path,
            &parent_path.join("workspace.mesh"),
            &self.protected,
            token(&self.allocation)?,
        )
        .map_err(|_| invalid())?;
        let state = daemon
            .install_received_lane(prepared, &self.manifest)
            .map_err(|error| io::Error::other(error.code))?;
        self.verify()?;
        let [initial] = state.workspace_versions.as_slice() else {
            return Err(invalid());
        };
        let binding = WorkspaceBinding {
            source_version: self.manifest.input(),
            starting_version: Some(initial.operation()),
            root: state.root.clone(),
            digest: state.digest.clone(),
            installation: state.installation.clone(),
        };
        let receipt = Json::object([
            ("schema", Json::text("mesh.received-workspace/v1")),
            ("context", context),
            (
                "worker_initial_operation",
                Json::text(initial.operation().to_string()),
            ),
            ("workspace", state.to_json()),
        ]);
        self.allocation.filesystem().write_new_file(
            Path::new(RECEIPT),
            receipt.encode().as_bytes(),
            fs::Permissions::from_mode(0o600),
        )?;
        self.allocation.sync()?;
        let result = ReceivedWorkerWorkspace {
            input: self,
            daemon,
            binding,
            receipt,
            intent,
            admission,
        };
        result.verify()?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
