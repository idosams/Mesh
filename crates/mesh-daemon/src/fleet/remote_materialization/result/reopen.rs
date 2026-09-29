//! Read-only reopening of a retained local result; no daemon, credential or execution owner.
use super::*;
use crate::fleet::{RemoteInputSource, RemoteResultEvidenceReceipt};
use mesh_store::RecordDigest;
fn retained(root: &PinnedWorkspaceRoot, name: &str) -> io::Result<String> {
    let file = root.filesystem().read_only().read_file(Path::new(name))?;
    let metadata = file.metadata()?;
    if metadata.nlink() != 1
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.len() > 1_048_576
    {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    file.take(1_048_577).read_to_end(&mut bytes)?;
    if bytes.len() > 1_048_576 {
        return Err(invalid());
    }
    String::from_utf8(bytes).map_err(|_| invalid())
}
impl RemoteInputDestination {
    /// Reopen exact saved local bytes using a separately retained mapping digest and authenticated
    /// evidence receipt. Missing/changed state refuses without repair or execution adoption.
    pub fn reopen_result_history(
        &self,
        allocation_id: &str,
        mapping: RecordDigest,
        evidence: &RemoteResultEvidenceReceipt,
        manifest: &RemoteInputManifest,
        reviewers: &TrustedReviewers,
    ) -> io::Result<RemoteInputSource> {
        self.reopen_result_history_checked(
            allocation_id,
            mapping,
            evidence.digest(),
            manifest,
            reviewers,
            None,
        )
    }
    pub(in crate::fleet) fn reopen_result_history_checked(
        &self,
        allocation_id: &str,
        mapping: RecordDigest,
        evidence: RecordDigest,
        manifest: &RemoteInputManifest,
        reviewers: &TrustedReviewers,
        review: Option<(RecordDigest, RecordDigest)>,
    ) -> io::Result<RemoteInputSource> {
        self.reopen_result_review_history(
            allocation_id,
            mapping,
            evidence,
            manifest,
            reviewers,
            review,
        )
        .map(|(_, source)| source)
    }
    pub(in crate::fleet) fn reopen_result_review_history(
        &self,
        allocation_id: &str,
        mapping: RecordDigest,
        evidence: RecordDigest,
        manifest: &RemoteInputManifest,
        reviewers: &TrustedReviewers,
        review: Option<(RecordDigest, RecordDigest)>,
    ) -> io::Result<(crate::workspace::OpenWorkspace, RemoteInputSource)> {
        self.verify()?;
        if allocation_id.len() != 32
            || !allocation_id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid());
        }
        let name = format!("result-{allocation_id}");
        let allocation = self.parent.open_child_directory(OsStr::new(&name))?;
        private(&allocation)?;
        let raw = retained(&allocation, RECEIPT)?;
        if RecordDigest::from_bytes(*Blake3::digest_bytes(raw.as_bytes()).as_bytes()) != mapping {
            return Err(invalid());
        }
        let receipt = Json::parse(&raw).map_err(|_| invalid())?;
        if receipt.encode() != raw || retained(&allocation, "manifest.json")? != manifest.encoded()
        {
            return Err(invalid());
        }
        let files = allocation.open_child_directory(OsStr::new("files"))?;
        let result = RemoteResultAllocation {
            tree: RemoteInputAllocation {
                admission: None,
                manifest: manifest.clone(),
                parent: self.parent.clone(),
                allocation,
                files,
                path: self.parent_path.join(&name).join("files"),
                protected: self.protected.clone(),
            },
            evidence,
        };
        result.verify()?;
        let context = context(&result)?;
        let intent = Json::object([
            ("schema", Json::text("mesh.received-result-intent/v1")),
            ("context", context.clone()),
        ])
        .encode();
        if receipt.get("schema").and_then(Json::as_text)
            != Some("mesh.received-result-workspace/v1")
            || receipt.get("context") != Some(&context)
            || retained(&result.tree.allocation, INTENT)? != intent
        {
            return Err(invalid());
        }
        let initial = RecordDigest::parse_hex(
            receipt
                .get("local_initial_operation")
                .and_then(Json::as_text)
                .ok_or_else(invalid)?,
        )
        .map_err(|_| invalid())?;
        let workspace = receipt.get("workspace").ok_or_else(invalid)?;
        let installation = workspace
            .get("installation")
            .and_then(Json::as_text)
            .ok_or_else(invalid)?;
        let path = crate::workspace::presented_workspace_path(
            &self.parent_path.join(&name).join("workspace.mesh"),
        )?;
        if workspace.get("root").and_then(Json::as_text) != path.to_str() {
            return Err(invalid());
        }
        let open = crate::workspace::OpenWorkspace::reopen_history(
            &path,
            installation,
            token(&result.tree.allocation)?,
            reviewers,
        )
        .map_err(|_| invalid())?;
        let saved = open
            .historical_workspace_preview(initial)
            .map_err(|_| invalid())?;
        if !manifest.matches_saved_content(&saved) {
            return Err(invalid());
        }
        if let Some((version, bundle)) = review {
            if version != initial
                || !open
                    .review(&bundle)
                    .is_some_and(|r| r.subject_operation == version && r.bundle == bundle)
            {
                return Err(invalid());
            }
        }
        let source = open.remote_input_source(initial)?.protecting_allocation(
            vec![self.parent.clone(), result.tree.allocation.clone()],
            token(&result.tree.allocation)?,
        )?;
        result.verify()?;
        if retained(&result.tree.allocation, RECEIPT)? != raw
            || retained(&result.tree.allocation, INTENT)? != intent
        {
            return Err(invalid());
        }
        self.verify()?;
        source.verify_roots()?;
        Ok((open, source))
    }
}
