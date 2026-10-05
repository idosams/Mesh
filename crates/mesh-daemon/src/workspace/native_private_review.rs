//! Read-only native evidence reconstruction, with no ordinary workspace or main access.
use super::*;
use crate::project_attachment::{NativeDependencyWorkBinding, VerifiedPrivateHistory};
use crate::workspace_custody::WorkspaceInitializationGuard;

/// This adapter owns a read-only workspace and borrows the complete custody guard. It has no
/// Deref, workspace accessor, main lookup, append, receipt promotion or mutable operation.
pub(crate) struct NativePrivateReviewHistory<'a> {
    history: OpenWorkspace,
    pinned: PinnedWorkspaceRoot,
    evidence: &'a VerifiedPrivateHistory,
    owner: (&'a PinnedWorkspaceRoot, &'a VerifiedPrivateHistory),
    selected: &'a NativeDependencyWorkBinding,
    guard: &'a WorkspaceInitializationGuard,
}
impl<'a> NativePrivateReviewHistory<'a> {
    pub(crate) fn open(
        metadata: &Path,
        pinned: PinnedWorkspaceRoot,
        evidence: &'a VerifiedPrivateHistory,
        owner: (&'a PinnedWorkspaceRoot, &'a VerifiedPrivateHistory),
        expected_workspace: mesh_operations::WorkspaceId,
        selected: &'a NativeDependencyWorkBinding,
        guard: &'a WorkspaceInitializationGuard,
    ) -> Result<Self, String> {
        guard
            .require_roots(std::slice::from_ref(&pinned))
            .map_err(|e| e.to_string())?;
        if evidence.binding().installation != selected.installation()
            || owner.1.binding().authority != selected.authority()
            || owner.1.binding().project != selected.project()
        {
            return Err("private review history belongs to another installation".into());
        }
        let history = OpenWorkspace::open_layout_inner(
            metadata,
            Some(metadata),
            &crate::TrustedReviewers::default(),
            false,
            Some(PreparedWorkspaceAuthority::transient(
                pinned.clone(),
                pinned.clone(),
            )),
            WorkspaceOpenRecovery::PrivateDependencyHistory(evidence),
        )
        .map_err(|e| e.to_string())?;
        if history.operations() > 0 && history.journal_workspace_id()? != expected_workspace {
            return Err("private review history differs from its native configuration".into());
        }
        guard.ensure_current().map_err(|e| e.to_string())?;
        let result = Self {
            history,
            pinned,
            evidence,
            owner,
            selected,
            guard,
        };
        result.ensure_current()?;
        Ok(result)
    }

    fn ensure_current(&self) -> Result<(), String> {
        self.guard
            .require_roots(&[self.pinned.clone(), self.owner.0.clone()])
            .map_err(|e| e.to_string())?;
        self.evidence
            .verify_current(&self.pinned)
            .map_err(|e| e.to_string())?;
        self.owner
            .1
            .verify_current(self.owner.0)
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub(crate) fn approval_context(
        &self,
        binding: &crate::dependency_policy::NativeReviewBinding,
    ) -> Result<mesh_approval::HumanApprovalContext, String> {
        self.ensure_current()?;
        let output = binding.evidence().output();
        if (output.0, output.1) != (self.selected.work(), self.selected.installation())
            || binding.evidence().owner() != self.owner.1.binding()
        {
            return Err("private review binding belongs to another native work".into());
        }
        // Private history cannot resolve a canonical head through ordinary child reviews or
        // interpret absent configured trust as genesis. Nonzero ancestry needs sealed replay.
        if binding.canonical() != crate::publication::GENESIS_SHARED_HEAD {
            return Err("verified native canonical ancestry is unavailable".into());
        }
        let (context, _, _, _, _) = self.history.native_human_approval_preview(binding)?;
        self.ensure_current()?;
        Ok(context)
    }
}
