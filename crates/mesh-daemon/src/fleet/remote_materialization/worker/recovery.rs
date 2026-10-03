//! Reconstruct initialization only under fresh authenticated registry and original allocation custody.
use super::*;
use crate::fleet::RemoteMaterializationReceipt;

impl RemoteInputDestination {
    pub(in crate::fleet) fn recover_worker_workspace(
        &self,
        materialization: &RemoteMaterializationReceipt,
        reviewers: TrustedReviewers,
        checkpoint: CheckpointRuntimeParameters,
        revalidate: impl Fn() -> io::Result<()>,
    ) -> io::Result<ReceivedWorkerWorkspace> {
        revalidate()?;
        self.verify()?;
        let identities = materialization.directory_identities();
        let expected =
            |index: usize| ProtectedWorkspaceRoot::from_directory_token(&identities[index]);
        self.parent.ensure_protected_identity(expected(0)?)?;
        let admission = materialization.admission().clone();
        let name = format!("input-{}", admission.allocation());
        let allocation = self.parent.open_child_directory(OsStr::new(&name))?;
        allocation.ensure_protected_identity(expected(1)?)?;
        let initialization_owner = InitializationOwner::acquire(&allocation)?;
        revalidate()?;
        let files = allocation.open_child_directory(OsStr::new("files"))?;
        files.ensure_protected_identity(expected(2)?)?;
        private(&files)?;
        let manifest_bytes = retained(&allocation, "manifest.json")?;
        let assignment = &admission.work().assignment;
        let manifest =
            RemoteInputManifest::decode(&manifest_bytes, assignment.input, assignment.bundle)
                .map_err(|_| invalid())?;
        let input = RemoteInputAllocation {
            admission: None,
            manifest,
            parent: self.parent.clone(),
            allocation,
            files,
            path: self.parent_path.join(&name).join("files"),
            protected: self.protected.clone(),
        };
        if input.retained_identity()? != *identities {
            return Err(invalid());
        }
        input.verify()?;
        let guard = || {
            revalidate()?;
            self.verify()?;
            initialization_owner.verify()?;
            input.verify_roots()
        };
        guard()?;
        let context = mapping_context(&input, &admission)?;
        let intent = Json::object([
            ("schema", Json::text("mesh.received-workspace-intent/v1")),
            ("context", context.clone()),
        ])
        .encode();
        crate::folder_import::received_record::finish(
            &input.allocation,
            Path::new(INTENT),
            intent.as_bytes(),
        )?;
        guard()?;
        let handoff = crate::PreparedFolderImport::recover_received_with_parent(
            &input.path,
            &self.parent_path.join(&name).join("workspace.mesh"),
            &input.protected,
            token(&input.allocation)?,
            guard,
        )
        .map_err(|_| invalid())?;
        guard()?;
        input.verify()?;
        let daemon = Arc::new(
            LiveDaemon::with_trusted_reviewers_and_checkpoint_runtime(
                StartupSummary::from(&nothing_to_recover()),
                reviewers,
                checkpoint,
            )
            .map_err(|_| invalid())?,
        );
        let state = daemon
            .install_recovered_received_lane(handoff, &input.manifest)
            .map_err(|error| io::Error::other(error.code))?;
        guard()?;
        let [initial] = state.workspace_versions.as_slice() else {
            return Err(invalid());
        };
        let binding = WorkspaceBinding {
            source_version: input.manifest.input(),
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
        crate::folder_import::received_record::finish(
            &input.allocation,
            Path::new(RECEIPT),
            receipt.encode().as_bytes(),
        )?;
        guard()?;
        input.verify()?;
        let result = ReceivedWorkerWorkspace {
            input,
            daemon,
            binding,
            receipt,
            intent,
            admission,
            initialization_owner,
        };
        result.verify()?;
        revalidate()?;
        Ok(result)
    }
}
