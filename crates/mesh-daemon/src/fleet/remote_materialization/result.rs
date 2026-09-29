//! Independent local history for an authenticated result copy, without worker admission.
use super::*;
use crate::fleet::WorkspaceBinding;
use crate::ipc::{nothing_to_recover, Json, StartupSummary};
use crate::{CheckpointRuntimeParameters, LiveDaemon, TrustedReviewers};
use std::sync::Arc;
const INTENT: &str = "result-initialization.json";
const RECEIPT: &str = "result-workspace.json";

/// Native local history correlated with a retained authenticated remote result.
/// No provider, worker admission, execution session or protected-main approval is created.
pub struct ReceivedResultWorkspace {
    result: RemoteResultAllocation,
    daemon: Arc<LiveDaemon>,
    binding: WorkspaceBinding,
    intent: String,
    receipt: Json,
}
impl ReceivedResultWorkspace {
    /// Local installation and saved operation; remote content is not local DAG ancestry.
    pub fn binding(&self) -> &WorkspaceBinding {
        &self.binding
    }
    /// Digest to retain in the coordinator correlation record before history-only reopening.
    pub fn receipt_digest(&self) -> mesh_store::RecordDigest {
        mesh_store::RecordDigest::from_bytes(
            *Blake3::digest_bytes(self.receipt.encode().as_bytes()).as_bytes(),
        )
    }
    /// Exact durable mapping retained beside the independently imported workspace.
    pub fn receipt(&self) -> &Json {
        &self.receipt
    }
    /// Record an immutable native inspection review for this exact initial result. Repeating
    /// returns the original review; this does not acquire agent custody or approve publication.
    pub fn record_review(
        &self,
        actor: mesh_types::PublicKey,
    ) -> io::Result<mesh_store::RecordDigest> {
        self.verify()?;
        let target = self.binding.starting_version.ok_or_else(invalid)?;
        let bundle = self
            .daemon
            .submit_received_result_review(
                token(&self.result.tree.allocation)?,
                &self.binding.root,
                &self.binding.installation,
                target,
                self.result.manifest(),
                actor,
            )
            .map_err(|_| invalid())?;
        self.verify()?;
        Ok(bundle)
    }
    #[cfg(target_os = "macos")]
    pub(in crate::fleet) fn evidence_receipt(&self) -> mesh_store::RecordDigest {
        self.result.evidence
    }
    #[cfg(target_os = "macos")]
    pub(in crate::fleet) fn allocation_id(&self) -> io::Result<String> {
        self.verify()?;
        let name = self
            .result
            .tree
            .path
            .parent()
            .and_then(Path::file_name)
            .and_then(OsStr::to_str)
            .ok_or_else(invalid)?;
        Ok(name.strip_prefix("result-").ok_or_else(invalid)?.to_owned())
    }
    /// Recheck original copy, physical allocation custody, native history and retained receipts.
    /// This verifies immutable provenance only, not current dependency eligibility or approval.
    pub fn verify(&self) -> io::Result<()> {
        self.result.verify()?;
        self.daemon
            .verify_received_lane_binding(
                token(&self.result.tree.allocation)?,
                &self.binding.root,
                &self.binding.installation,
                self.binding.starting_version.ok_or_else(invalid)?,
                self.result.manifest(),
            )
            .map_err(|_| invalid())?;
        for (name, expected) in [
            (INTENT, self.intent.clone()),
            (RECEIPT, self.receipt.encode()),
        ] {
            let file = self
                .result
                .tree
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
        self.result.verify()
    }
}
impl RemoteResultAllocation {
    /// Initialize independent native history once. The create-only intent precedes import effects;
    /// interrupted or conflicting work remains intact and does not authorize retry or adoption.
    pub fn into_result_workspace(
        self,
        reviewers: TrustedReviewers,
        checkpoint: CheckpointRuntimeParameters,
    ) -> io::Result<ReceivedResultWorkspace> {
        self.verify()?;
        let parent = self.tree.path.parent().ok_or_else(invalid)?;
        let context = context(&self)?;
        let intent = Json::object([
            ("schema", Json::text("mesh.received-result-intent/v1")),
            ("context", context.clone()),
        ])
        .encode();
        self.tree.allocation.filesystem().write_new_file(
            Path::new(INTENT),
            intent.as_bytes(),
            fs::Permissions::from_mode(0o600),
        )?;
        self.tree.allocation.sync()?;
        self.verify()?;
        let daemon = Arc::new(
            LiveDaemon::with_trusted_reviewers_and_checkpoint_runtime(
                StartupSummary::from(&nothing_to_recover()),
                reviewers,
                checkpoint,
            )
            .map_err(|_| invalid())?,
        );
        let prepared = crate::PreparedFolderImport::prepare_received_with_parent(
            self.path(),
            &parent.join("workspace.mesh"),
            &self.tree.protected,
            token(&self.tree.allocation)?,
        )
        .map_err(|_| invalid())?;
        let state = daemon
            .install_received_lane(prepared, self.manifest())
            .map_err(|error| io::Error::other(error.code))?;
        self.verify()?;
        let [initial] = state.workspace_versions.as_slice() else {
            return Err(invalid());
        };
        let binding = WorkspaceBinding {
            source_version: self.manifest().input(),
            starting_version: Some(initial.operation()),
            root: state.root.clone(),
            digest: state.digest.clone(),
            installation: state.installation.clone(),
        };
        let receipt = Json::object([
            ("schema", Json::text("mesh.received-result-workspace/v1")),
            ("context", context),
            (
                "local_initial_operation",
                Json::text(initial.operation().to_string()),
            ),
            ("workspace", state.to_json()),
        ]);
        self.tree.allocation.filesystem().write_new_file(
            Path::new(RECEIPT),
            receipt.encode().as_bytes(),
            fs::Permissions::from_mode(0o600),
        )?;
        self.tree.allocation.sync()?;
        let result = ReceivedResultWorkspace {
            result: self,
            daemon,
            binding,
            intent,
            receipt,
        };
        result.verify()?;
        Ok(result)
    }
}

fn context(result: &RemoteResultAllocation) -> io::Result<Json> {
    Ok(Json::object([
        ("evidence_receipt", Json::text(result.evidence.to_string())),
        (
            "remote_version",
            Json::text(result.manifest().input().to_string()),
        ),
        (
            "remote_manifest",
            Json::text(result.manifest().bundle().to_string()),
        ),
        (
            "allocation",
            Json::text(token(&result.tree.allocation)?.directory_token()),
        ),
        (
            "files",
            Json::text(token(&result.tree.files)?.directory_token()),
        ),
    ]))
}

#[cfg(target_os = "macos")]
mod reopen;
