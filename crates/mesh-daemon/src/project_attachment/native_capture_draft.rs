//! Pure private-authoring preparation, with no writable history capability.
use super::*;
use crate::checkpoint_storage::{
    prepare_authenticated_checkpoint, PreparedAuthenticatedCheckpoint,
};
use crate::project_attachment::capture_line::CaptureLine;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NativeCaptureDraft {
    pub(in crate::project_attachment) configuration: String,
    pub(in crate::project_attachment) line: CaptureLine,
    basis: ManagedAuthoringBasis,
    operations: Vec<Operation>,
    files: Vec<PreparedCheckpointFile>,
    actor: PublicKey,
}
impl NativeCaptureDraft {
    pub(crate) fn prepare(
        history: &OpenWorkspace,
        store: &PinnedWorkspaceRoot,
        configuration: &str,
        input: &CapturedProjectInput,
        actor: PublicKey,
    ) -> io::Result<Self> {
        verify_history_binding(history, configuration)?;
        let line = CaptureLine::load(store, history, configuration)?;
        let workspace_id = WorkspaceId::from_bytes(short_id(configuration.as_bytes()));
        let basis = if let Some(head) = line.head {
            let basis = history
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
        let (operations, files) = prepare_snapshot(history, input, &basis, line.head)?;
        if operations.is_empty() {
            return Err(invalid("capture contains no new private progress"));
        }
        if let Some(head) = line.head {
            history
                .prepare_historical_operations(head, actor, &operations)
                .map_err(error)?;
        }
        Ok(Self {
            configuration: configuration.to_owned(),
            line,
            basis,
            operations,
            files,
            actor,
        })
    }
    fn request(&self, signature: Signature) -> AuthenticatedOperationCheckpointRequest {
        let b = &self.basis;
        AuthenticatedOperationCheckpointRequest::new(
            b.workspace_id,
            b.actor_id,
            b.session_id,
            b.actor_sequence,
            b.causal_parents.clone(),
            b.base_head,
            b.policy_epoch,
            b.hybrid_logical_time,
            self.operations.clone(),
            self.actor,
            signature,
        )
    }
    pub(in crate::project_attachment) fn signing_payload(&self) -> SigningPayload {
        SigningPayload::new(
            CHANGESET_SIGNATURE_DOMAIN,
            &operation_checkpoint_signing_body(
                &self.request(Signature::from_bytes([0; 64])),
                &CaptureHead,
            ),
        )
    }
    pub(crate) fn verify_basis(
        &self,
        history: &OpenWorkspace,
        store: &PinnedWorkspaceRoot,
    ) -> io::Result<()> {
        verify_history_binding(history, &self.configuration)?;
        if CaptureLine::load(store, history, &self.configuration)? != self.line {
            return Err(invalid("native capture line changed while signing"));
        }
        if let Some(head) = self.line.head {
            if history
                .historical_authoring_basis(head, self.actor)
                .map_err(error)?
                != self.basis
            {
                return Err(invalid(
                    "native capture authoring basis changed while signing",
                ));
            }
        } else if history.operations() != 0 {
            return Err(invalid("native capture genesis changed while signing"));
        }
        Ok(())
    }
    pub(crate) fn authenticate(
        &self,
        history: &OpenWorkspace,
        store: &PinnedWorkspaceRoot,
        signature: Signature,
    ) -> io::Result<PreparedAuthenticatedCheckpoint> {
        self.verify_basis(history, store)?;
        prepare_authenticated_checkpoint(
            history,
            self.request(signature),
            self.files.clone(),
            &CaptureHead,
        )
        .map_err(error)
    }
}
