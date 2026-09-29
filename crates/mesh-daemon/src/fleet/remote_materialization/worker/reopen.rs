//! Reopen immutable received history without recreating a session, credential or launch owner.
use super::*;
use crate::fleet::{RemoteInputSource, RemoteLaunchReceipt};
use mesh_store::RecordDigest;
const MAX_RECEIPT: u64 = 1_048_576;
fn retained(root: &PinnedWorkspaceRoot, name: &str) -> io::Result<String> {
    let file = root.filesystem().read_only().read_file(Path::new(name))?;
    let metadata = file.metadata()?;
    if metadata.nlink() != 1
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.len() > MAX_RECEIPT
    {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    file.take(MAX_RECEIPT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_RECEIPT {
        return Err(invalid());
    }
    String::from_utf8(bytes).map_err(|_| invalid())
}
impl RemoteInputDestination {
    pub(in crate::fleet) fn reopen_saved_review(
        &self,
        launch: &RemoteLaunchReceipt,
        review: RecordDigest,
        version: RecordDigest,
        expected_manifest: RecordDigest,
        reviewers: &TrustedReviewers,
    ) -> io::Result<RemoteInputSource> {
        self.verify()?;
        let admission = launch.admission();
        let assignment = &admission.work().assignment;
        let name = format!("input-{}", admission.allocation());
        let allocation = self.parent.open_child_directory(OsStr::new(&name))?;
        private(&allocation)?;
        let receipt_bytes = retained(&allocation, RECEIPT)?;
        if RecordDigest::from_bytes(*Blake3::digest_bytes(receipt_bytes.as_bytes()).as_bytes())
            != launch.workspace_mapping()
        {
            return Err(invalid());
        }
        let receipt = Json::parse(&receipt_bytes).map_err(|_| invalid())?;
        if receipt.encode() != receipt_bytes {
            return Err(invalid());
        }
        let manifest_bytes = retained(&allocation, "manifest.json")?;
        let manifest =
            RemoteInputManifest::decode(&manifest_bytes, assignment.input, assignment.bundle)
                .map_err(|_| invalid())?;
        let files = allocation.open_child_directory(OsStr::new("files"))?;
        let input = RemoteInputAllocation {
            admission: None,
            manifest,
            parent: self.parent.clone(),
            allocation,
            files,
            path: self.parent_path.join(&name).join("files"),
            protected: self.protected.clone(),
        };
        let context = mapping_context(&input, admission)?;
        let intent = Json::object([
            ("schema", Json::text("mesh.received-workspace-intent/v1")),
            ("context", context.clone()),
        ])
        .encode();
        if receipt.get("schema").and_then(Json::as_text) != Some("mesh.received-workspace/v1")
            || receipt.get("context") != Some(&context)
            || receipt
                .get("worker_initial_operation")
                .and_then(Json::as_text)
                != Some(launch.initial_operation().to_string().as_str())
            || retained(&input.allocation, INTENT)? != intent
        {
            return Err(invalid());
        }
        input.verify()?;
        let path = crate::workspace::presented_workspace_path(
            &self.parent_path.join(&name).join("workspace.mesh"),
        )?;
        let workspace = receipt.get("workspace").ok_or_else(invalid)?;
        if workspace.get("root").and_then(Json::as_text) != path.to_str()
            || workspace.get("installation").and_then(Json::as_text) != Some(launch.installation())
        {
            return Err(invalid());
        }
        let open = crate::workspace::OpenWorkspace::reopen_history(
            &path,
            launch.installation(),
            token(&input.allocation)?,
            reviewers,
        )
        .map_err(|_| invalid())?;
        let initial = open
            .historical_workspace_preview(launch.initial_operation())
            .map_err(|_| invalid())?;
        if !input.manifest.matches_saved_content(&initial)
            || !open
                .review(&review)
                .is_some_and(|r| r.bundle == review && r.subject_operation == version)
        {
            return Err(invalid());
        }
        let source = open.remote_input_source(version)?.protecting_allocation(
            vec![self.parent.clone(), input.allocation.clone()],
            token(&input.allocation)?,
        )?;
        if source.manifest().bundle() != expected_manifest {
            return Err(invalid());
        }
        input.verify()?;
        if retained(&input.allocation, RECEIPT)? != receipt_bytes
            || retained(&input.allocation, INTENT)? != intent
        {
            return Err(invalid());
        }
        self.verify()?;
        source.verify_roots()?;
        Ok(source)
    }
}
