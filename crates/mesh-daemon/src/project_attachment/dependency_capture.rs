//! Dedicated enrolled capture writer. History inspection remains read-only and cannot publish.
use super::*;
use crate::checkpoint_storage::{
    prepare_authenticated_checkpoint, PreparedAuthenticatedCheckpoint,
};
use crate::project_attachment::{
    capture_line::CaptureLine,
    dependency_decision,
    dependency_enrollment::read_private_in_store,
    dependency_read::VerifiedDependencyRead,
    dependency_transaction::{hash, read_payload},
    ProvisionedAttachment,
};
use std::io::{Seek as _, Write as _};

const PENDING: &str = "dependency-capture.pending";
const MAX_JOURNAL: usize = 80 * 1024 * 1024;

/// Signed private capture prepared without granting publication or holding native custody.
/// Commit must refresh exact enrollment and history; signing can never reserve the commit order.
pub struct PreparedNativeCapture {
    attachment: ProvisionedAttachment,
    configuration: String,
    proof: VerifiedDependencyRead,
    line: CaptureLine,
    request: RecordDigest,
    prepared: PreparedAuthenticatedCheckpoint,
}

fn absent(store: &PinnedWorkspaceRoot, name: &str) -> io::Result<()> {
    match read_private_in_store(store, name) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
        Ok(_) => Err(invalid("pending native work requires exact recovery")),
    }
}

impl ProvisionedAttachment {
    /// Prepare authenticated private progress after explicit native enrollment. This native API
    /// does not enroll a project, grant an input, record consumption or authorize publication.
    /// The signer runs without a custody guard. No source or history bytes are written here.
    pub fn prepare_dependency_capture<F, E>(
        &self,
        input: &CapturedProjectInput,
        actor: PublicKey,
        request: RecordDigest,
        sign: F,
    ) -> io::Result<PreparedNativeCapture>
    where
        F: FnOnce(&SigningPayload) -> Result<Signature, E>,
        E: std::fmt::Display,
    {
        if request == RecordDigest::from_bytes([0; 32]) {
            return Err(invalid("missing native capture request"));
        }
        let (configuration, proof, workspace, line, basis, operations, files) = {
            let _guard = crate::workspace_custody::lock_workspace_initialization(&self.store)
                .map_err(error)?;
            self.check_dependency_registration()?;
            self.attachment.ensure_current()?;
            if input.root() != self.attachment.root()
                || input.identity() != self.attachment.pinned.identity()?
            {
                return Err(invalid("capture belongs to a different native attachment"));
            }
            absent(&self.store, PENDING)?;
            absent(&self.store, dependency_decision::PENDING)?;
            crate::project_attachment::detachment::ensure_attached(&self.store)?;
            let (configuration, proof) = self
                .attachment
                .read_configuration(self.metadata_path(), &self.store)?;
            self.attachment.history_configuration_with_previous(
                &self.store,
                Some(input.exclusion_digest()),
                Some(configuration.clone()),
            )?;
            let proof = proof.ok_or_else(|| invalid("native dependency enrollment is required"))?;
            let workspace = OpenWorkspace::open_attachment_read_history(
                self.metadata_path(),
                self.store.clone(),
                &crate::TrustedReviewers::default(),
                Some(&proof),
            )
            .map_err(error)?;
            verify_history_binding(&workspace, &configuration)?;
            let line = CaptureLine::load(&self.store, &workspace, &configuration)?;
            let workspace_id = WorkspaceId::from_bytes(short_id(configuration.as_bytes()));
            let basis = if let Some(head) = line.head {
                let basis = workspace
                    .historical_authoring_basis(head, actor)
                    .map_err(error)?;
                if basis.workspace_id != workspace_id {
                    return Err(invalid("capture history identity changed"));
                }
                basis
            } else {
                ManagedAuthoringBasis {
                    workspace_id,
                    actor_id: ActorId::from_bytes(*actor.as_bytes()),
                    session_id: SessionId::from_bytes(short_id(actor.as_bytes())),
                    actor_sequence: ActorSequence::FIRST,
                    causal_parents: CausalParents::genesis(),
                    base_head: HeadId::from_bytes([0; 32]),
                    policy_epoch: PolicyEpoch::new(1),
                    hybrid_logical_time: Hlc::new(0, 0),
                }
            };
            let (operations, files) = prepare_snapshot(&workspace, input, &basis, line.head)?;
            if operations.is_empty() {
                return Err(invalid("capture contains no new private progress"));
            }
            if let Some(head) = line.head {
                workspace
                    .prepare_historical_operations(head, actor, &operations)
                    .map_err(error)?;
            }
            (
                configuration,
                proof,
                workspace,
                line,
                basis,
                operations,
                files,
            )
        };
        let make_request = |signature| {
            AuthenticatedOperationCheckpointRequest::new(
                basis.workspace_id,
                basis.actor_id,
                basis.session_id,
                basis.actor_sequence,
                basis.causal_parents.clone(),
                basis.base_head,
                basis.policy_epoch,
                basis.hybrid_logical_time,
                operations.clone(),
                actor,
                signature,
            )
        };
        let unsigned = make_request(Signature::from_bytes([0; 64]));
        let signature = sign(&SigningPayload::new(
            CHANGESET_SIGNATURE_DOMAIN,
            &operation_checkpoint_signing_body(&unsigned, &CaptureHead),
        ))
        .map_err(error)?;
        let prepared = prepare_authenticated_checkpoint(
            &workspace,
            make_request(signature),
            files,
            &CaptureHead,
        )
        .map_err(error)?;
        Ok(PreparedNativeCapture {
            attachment: self.clone(),
            configuration,
            proof,
            line,
            request,
            prepared,
        })
    }
}

impl PreparedNativeCapture {
    /// Exact signed candidate identity. It is not a saved acknowledgement until commit succeeds.
    pub fn operation(&self) -> RecordDigest {
        self.prepared.changeset_id
    }

    /// Commit exact private capture records. A failed append retains its intent and staged content;
    /// no approval, integration, actor authorization or consumption record can be appended here.
    pub fn commit(self) -> io::Result<SavedAttachmentVersion> {
        let a = &self.attachment;
        let _guard =
            crate::workspace_custody::lock_workspace_initialization(&a.store).map_err(error)?;
        a.check_dependency_registration()?;
        a.attachment.ensure_current()?;
        crate::project_attachment::detachment::ensure_attached(&a.store)?;
        absent(&a.store, PENDING)?;
        absent(&a.store, dependency_decision::PENDING)?;
        let (configuration, proof) = a
            .attachment
            .read_configuration(a.metadata_path(), &a.store)?;
        if configuration != self.configuration || proof.as_ref() != Some(&self.proof) {
            return Err(invalid("native capture basis changed while signing"));
        }
        let history = OpenWorkspace::open_attachment_read_history(
            a.metadata_path(),
            a.store.clone(),
            &crate::TrustedReviewers::default(),
            Some(&self.proof),
        )
        .map_err(error)?;
        verify_history_binding(&history, &configuration)?;
        if CaptureLine::load(&a.store, &history, &configuration)? != self.line {
            return Err(invalid("native capture line changed while signing"));
        }
        let mut journal = a
            .store
            .open_existing_record_file(Path::new(crate::RECORD_FILE_NAME))?;
        journal.rewind()?;
        let mut before = Vec::new();
        (&mut journal)
            .take((MAX_JOURNAL + 1) as u64)
            .read_to_end(&mut before)?;
        self.proof.verify(&a.store, &journal, &before)?;
        let records = self.prepared.checkpoint.records();
        if records.iter().any(|r| {
            !matches!(
                r,
                mesh_store::StoredRecord::Manifest(_) | mesh_store::StoredRecord::Operation(_)
            )
        }) {
            return Err(invalid("capture contains non-authoring records"));
        }
        let frames = records
            .iter()
            .flat_map(mesh_store::frame_record)
            .collect::<Vec<_>>();
        if before.len().saturating_add(frames.len()) > MAX_JOURNAL {
            return Err(invalid("capture exceeds native journal bound"));
        }
        let cas = Cas::with_filesystem(a.metadata_path(), a.store.filesystem()).map_err(error)?;
        for bytes in self.prepared.objects {
            cas.promote(bytes).map_err(error)?;
        }
        cas.promote(frames.clone()).map_err(error)?;
        let metadata = journal.metadata()?;
        let intent = Json::object([
            ("schema", Json::text("mesh.dependency-capture-intent/v1")),
            ("request", Json::text(self.request.to_hex())),
            (
                "authority",
                Json::text(self.proof.binding().authority.to_hex()),
            ),
            (
                "configuration",
                Json::text(hash(configuration.as_bytes()).to_hex()),
            ),
            (
                "journal_device",
                Json::text(format!("{:016x}", metadata.dev())),
            ),
            (
                "journal_inode",
                Json::text(format!("{:016x}", metadata.ino())),
            ),
            ("before_bytes", Json::Number(before.len() as u64)),
            ("before_digest", Json::text(hash(&before).to_hex())),
            ("frames", Json::text(hash(&frames).to_hex())),
            ("operation", Json::text(self.prepared.changeset_id.to_hex())),
        ])
        .encode();
        self.line.persist(&a.store, &configuration)?;
        a.store.filesystem().write_new_file(
            Path::new(PENDING),
            intent.as_bytes(),
            fs::Permissions::from_mode(0o600),
        )?;
        a.store.sync()?;
        self.line
            .begin(self.prepared.changeset_id, &a.store, &configuration)?;
        let (current, proof) = a
            .attachment
            .read_configuration(a.metadata_path(), &a.store)?;
        if current != configuration
            || proof.as_ref() != Some(&self.proof)
            || read_private_in_store(&a.store, PENDING)? != intent
            || read_payload(&cas, hash(&frames), MAX_JOURNAL)? != frames
        {
            return Err(invalid("native capture changed before append"));
        }
        journal.write_all(&frames)?;
        journal.sync_all()?;
        let (current, proof) = a
            .attachment
            .read_configuration(a.metadata_path(), &a.store)?;
        if current != configuration {
            return Err(invalid("native capture binding changed"));
        }
        let proof = proof.ok_or_else(|| invalid("native capture enrollment disappeared"))?;
        if proof.binding() != self.proof.binding() {
            return Err(invalid("native capture authority changed after append"));
        }
        a.check_dependency_registration()?;
        let replay = OpenWorkspace::open_attachment_read_history(
            a.metadata_path(),
            a.store.clone(),
            &crate::TrustedReviewers::default(),
            Some(&proof),
        )
        .map_err(error)?;
        verify_history_binding(&replay, &configuration)?;
        replay
            .historical_workspace_preview(self.prepared.changeset_id)
            .map_err(error)?;
        if read_private_in_store(&a.store, PENDING)? != intent {
            return Err(invalid("native capture intent changed"));
        }
        self.line
            .finish(self.prepared.changeset_id, &a.store, &configuration)?;
        a.store.filesystem().remove_file(Path::new(PENDING))?;
        a.store.sync()?;
        Ok(SavedAttachmentVersion {
            operation: self.prepared.changeset_id,
        })
    }
}

#[cfg(test)]
#[path = "dependency_capture_tests.rs"]
mod tests;
