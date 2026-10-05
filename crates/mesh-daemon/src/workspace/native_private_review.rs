//! Read-only native evidence reconstruction, with no ordinary workspace or main access.
use super::*;
use crate::project_attachment::{NativeDependencyWorkBinding, VerifiedPrivateHistory};
use crate::workspace_custody::WorkspaceInitializationGuard;

/// Only verified durable native receipts can construct this operation mapping.
#[derive(Clone, Copy)]
pub(super) struct VerifiedCanonicalOperation {
    head: mesh_approval::HeadId,
    operation: RecordDigest,
}
impl VerifiedCanonicalOperation {
    pub(super) fn operation_for(
        &self,
        head: mesh_approval::HeadId,
    ) -> Result<RecordDigest, String> {
        if self.head != head {
            return Err("native canonical mapping names another head".into());
        }
        Ok(self.operation)
    }
}

/// This adapter owns a read-only workspace and borrows the complete custody guard. It has no
/// Deref, workspace accessor, main lookup, append, receipt promotion or mutable operation.
pub(crate) struct NativePrivateReviewHistory<'a> {
    history: OpenWorkspace,
    canonical: BTreeMap<mesh_approval::HeadId, VerifiedCanonicalOperation>,
    latest: Option<crate::dependency_policy::NativePublicationClaim>,
    challenges: BTreeSet<RecordDigest>,
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
            canonical: BTreeMap::new(),
            latest: None,
            challenges: BTreeSet::new(),
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

    /// Inspect exact saved input while retaining the private owner/work custody. The callback
    /// cannot obtain a workspace, write journal records or treat these bytes as a current grant.
    pub(crate) fn with_saved_input<T>(
        &self,
        operation: RecordDigest,
        read: impl FnOnce(crate::project_attachment::NativeGrantedInput<'_>) -> io::Result<T>,
    ) -> io::Result<T> {
        self.ensure_current().map_err(io::Error::other)?;
        let result = crate::project_attachment::NativeGrantedInput::with_history(
            &self.history,
            operation,
            read,
        )?;
        self.ensure_current().map_err(io::Error::other)?;
        Ok(result)
    }

    pub(crate) fn saved_version(
        &self,
        operation: RecordDigest,
    ) -> io::Result<crate::project_attachment::SavedAttachmentVersion> {
        self.ensure_current().map_err(io::Error::other)?;
        let version = crate::project_attachment::SavedAttachmentVersion::from_verified_history(
            &self.history,
            operation,
        )?;
        self.ensure_current().map_err(io::Error::other)?;
        Ok(version)
    }

    pub(crate) fn graph_operation(
        &self,
        operation: RecordDigest,
    ) -> Result<super::NativeOperationFact, String> {
        self.ensure_current()?;
        if self.evidence.is_legacy_operation(operation) {
            return Err("legacy input ancestry needs explicit migration evidence".into());
        }
        let fact = self.history.dependency_operation_fact(operation)?;
        self.ensure_current()?;
        Ok(fact)
    }

    pub(crate) fn graph_chunks(
        &self,
        manifest: RecordDigest,
        budget: &mut u64,
    ) -> Result<BTreeSet<RecordDigest>, String> {
        self.ensure_current()?;
        let chunks = self.history.dependency_manifest_chunks(manifest, budget)?;
        self.ensure_current()?;
        Ok(chunks)
    }

    pub(crate) fn verified_publication(
        &self,
    ) -> Option<crate::dependency_policy::NativePublicationClaim> {
        self.latest
    }

    pub(crate) fn current_review_bundle(
        &self,
        evidence: &crate::dependency_policy::NativeReviewEvidence,
    ) -> Result<(mesh_approval::HeadId, RecordDigest), String> {
        self.ensure_current()?;
        let output = evidence.output();
        if (output.0, output.1) != (self.selected.work(), self.selected.installation())
            || evidence.owner() != self.owner.1.binding()
        {
            return Err("private review evidence belongs to another native work".into());
        }
        let canonical = self
            .latest
            .map_or(crate::publication::GENESIS_SHARED_HEAD, |claim| {
                mesh_approval::HeadId::from_bytes(*claim.result.as_bytes())
            });
        let (bundle, _, _) = self.history.publication_review_with_evidence(
            output.2,
            canonical,
            false,
            Some(evidence),
            self.canonical.get(&canonical),
        )?;
        self.ensure_current()?;
        Ok((
            canonical,
            RecordDigest::from_bytes(*bundle.id().digest().as_bytes()),
        ))
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
        let native_base = self.canonical.get(&binding.canonical());
        if binding.canonical() != crate::publication::GENESIS_SHARED_HEAD && native_base.is_none() {
            return Err("verified native canonical ancestry is unavailable".into());
        }
        let (context, _, _, _, _) = self.history.human_approval_preview_with_evidence(
            &binding.review(),
            binding.canonical(),
            Some(binding.evidence()),
            native_base,
        )?;
        self.ensure_current()?;
        Ok(context)
    }
    pub(crate) fn check_receipt(
        &self,
        binding: &crate::dependency_policy::NativeReviewBinding,
        bytes: &[u8],
        trusted: &crate::TrustedReviewers,
    ) -> Result<mesh_approval::ExpectedHumanApproval, String> {
        use mesh_approval::{ExpectedHumanApproval, HumanApprovalReceipt};
        if bytes.is_empty() || bytes.len() > 65_536 {
            return Err("native review receipt exceeds its bound or is empty".into());
        }
        let receipt =
            HumanApprovalReceipt::from_canonical_bytes(bytes).map_err(|e| e.to_string())?;
        let carried = receipt.draft().expected();
        let credential = trusted
            .human_credential(carried.credential().id())
            .ok_or_else(|| "native review receipt credential is not trusted".to_owned())?;
        if carried.challenge() == &[0; 32] {
            return Err("native review receipt challenge is empty".into());
        }
        let expected = ExpectedHumanApproval::new(
            self.approval_context(binding)?,
            credential,
            *carried.challenge(),
        );
        mesh_approval::verify_human_approval_receipt(bytes, &expected)
            .map_err(|e| e.to_string())?;
        Ok(expected)
    }

    pub(crate) fn replay_publication(
        &mut self,
        claim: crate::dependency_policy::NativePublicationClaim,
        graph: &crate::project_attachment::NativeDependencyGraph,
        bytes: &[u8],
        trusted: &crate::TrustedReviewers,
    ) -> Result<(), String> {
        self.ensure_current()?;
        if self
            .owner
            .1
            .policy()
            .publication_claim(claim.record.payload)
            != Some(claim)
        {
            return Err("publication is not in the pinned owner history".into());
        }
        let expected_head = self
            .latest
            .map_or(crate::publication::GENESIS_SHARED_HEAD, |prior| {
                mesh_approval::HeadId::from_bytes(*prior.result.as_bytes())
            });
        let expected_revision = self
            .latest
            .map_or(Some(1), |prior| prior.revision.checked_add(1));
        let expected_previous = self
            .latest
            .map_or(RecordDigest::from_bytes([0; 32]), |prior| {
                prior.record.payload
            });
        if claim.review.canonical() != expected_head
            || Some(claim.revision) != expected_revision
            || claim.previous != expected_previous
            || self.challenges.contains(&claim.challenge)
            || graph.review_output() != claim.review.evidence().output()
            || graph.review_inputs().next().is_some()
            || self
                .owner
                .1
                .policy()
                .review_graph(claim.review.evidence().snapshot())
                != Some(graph.digest())
            || Blake3::digest_bytes(bytes).as_bytes() != claim.receipt.as_bytes()
        {
            return Err("native publication sequence or exact evidence differs".into());
        }
        let verified = self.check_receipt(&claim.review, bytes, trusted)?;
        if verified.context().reviewed_actor_head().as_bytes() != claim.result.as_bytes()
            || verified.credential().id().as_bytes() != claim.credential.as_bytes()
            || verified.challenge() != claim.challenge.as_bytes()
        {
            return Err("native publication claim differs from its verified receipt".into());
        }
        let head = verified.context().reviewed_actor_head();
        if self.canonical.contains_key(&head) {
            return Err("native publication repeats an accepted head".into());
        }
        self.ensure_current()?;
        self.canonical.insert(
            head,
            VerifiedCanonicalOperation {
                head,
                operation: claim.review.evidence().output().2,
            },
        );
        self.challenges.insert(claim.challenge);
        self.latest = Some(claim);
        Ok(())
    }
}
