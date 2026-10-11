//! Explicitly configured native history; ordinary candidates never acquire this admission.
use super::*;
use crate::{
    project_attachment::dependency_private_context::{
        NativeConsumedStartBasis, NativeConsumptionWriteProof,
    },
    root_authority::PinnedWorkspaceRoot,
    TrustedReviewers,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::project_attachment) enum NativeConsumedCommitPhase {
    Fence,
    Install,
    Checkpoint,
}

/// Native-host selectors. Each phase independently authenticates the saved operation and grant.
/// These IDs and handles are not permission, publication approval or a runnable destination.
pub struct NativeTrustedConsumedStartRequest<'a> {
    /// Original source handle, including its registration and physical namespace.
    pub source: &'a ProvisionedAttachment,
    /// Exact saved operation; no caller-provided saved-version proof is accepted.
    pub operation: RecordDigest,
    /// Original correlated destination; initial preparation requires it to be empty.
    pub destination: &'a ProvisionedAttachment,
    /// Exact current bound owner grant for this source operation and destination.
    pub grant: RecordDigest,
    /// Native discovery hints, independently completed and bounded at every phase.
    pub available: &'a [&'a ProvisionedAttachment],
    /// Stable nonzero transaction request.
    pub request: RecordDigest,
    /// Snapshot admission bounds.
    pub limits: ObservationLimits,
}

/// Immutable signed candidate for configured native history. It carries no continuing permission.
/// Its private inner candidate cannot escape into the ordinary installation or commit APIs.
pub struct PreparedNativeTrustedConsumedStart {
    inner: PreparedNativeConsumedStart,
    trusted: TrustedReviewers,
}
impl AttachmentStorage {
    /// Build from exact saved bytes under complete native history, then sign outside custody.
    /// Revalidation after the signer is mandatory. This writes no destination or private stage.
    pub fn prepare_native_consumed_start<F, E>(
        &self,
        owner: &ProvisionedAttachment,
        request: NativeTrustedConsumedStartRequest<'_>,
        actor: PublicKey,
        trusted: &TrustedReviewers,
        sign: F,
    ) -> io::Result<PreparedNativeTrustedConsumedStart>
    where
        F: FnOnce(&SigningPayload) -> Result<Signature, E>,
        E: std::fmt::Display,
    {
        let (basis, snapshot, history, version) =
            self.with_native_consumed_start_basis(owner, &request, actor, trusted, |material| {
                Ok((
                    material.basis,
                    material.snapshot,
                    material.history,
                    material.version,
                ))
            })?;
        // with_native_consumed_start_basis has released its complete guard before external code.
        let inner = self.sign_consumed_start_candidate(
            owner,
            NativeConsumedStartRequest {
                input: NativeGrantInspection {
                    source: request.source,
                    version,
                    destination: request.destination,
                    grant: request.grant,
                },
                available: request.available,
                request: request.request,
                limits: request.limits,
            },
            actor,
            (basis, snapshot, history),
            sign,
        )?;
        let prepared = PreparedNativeTrustedConsumedStart {
            inner,
            trusted: trusted.clone(),
        };
        prepared.revalidate(self)?;
        Ok(prepared)
    }
    /// Reconstruct exact retained uncommitted native input without invoking a signer.
    /// Current publication trust and the exact bound grant remain mandatory.
    /// This does not acknowledge consumption, admit runtime or advance protected main.
    pub fn recover_native_consumed_start(
        &self,
        owner: &ProvisionedAttachment,
        request: NativeTrustedConsumedStartRequest<'_>,
        trusted: &TrustedReviewers,
    ) -> io::Result<(
        PreparedNativeTrustedConsumedStart,
        StagedNativeConsumedStart,
    )> {
        let recover = || {
            self.with_native_consumption_write_proof(owner, &request, trusted, |proof| {
                proof.recover(RecoveryPhase::Installed, |inner, staged, _, _| {
                    Ok((
                        PreparedNativeTrustedConsumedStart {
                            inner,
                            trusted: trusted.clone(),
                        },
                        staged,
                    ))
                })
            })
        };
        let (prepared, staged) = recover()?;
        // Verify again under a fresh complete phase after releasing the first custody set.
        let (fresh, fresh_stage) = recover()?;
        if fresh.operation() != prepared.operation()
            || fresh_stage.receipt()? != staged.receipt()?
            || fresh_stage.root.identity()? != staged.root.identity()?
        {
            return Err(invalid(
                "native consumption recovery changed before acknowledgement",
            ));
        }
        Ok((prepared, staged))
    }
}
impl PreparedNativeTrustedConsumedStart {
    /// Authenticated candidate identity, not an appended or acknowledged saved version.
    pub fn operation(&self) -> RecordDigest {
        self.inner.operation()
    }
    /// Stable request identity.
    pub fn request(&self) -> RecordDigest {
        self.inner.request()
    }
    /// Exact inherited source closure identity.
    pub fn dependency_digest(&self) -> RecordDigest {
        self.inner.dependency_digest()
    }
    /// Validate preparation/private staging only. Fenced destinations need phase-specific recovery.
    /// This grants no continuing mutation or input permission.
    pub fn revalidate(&self, storage: &AttachmentStorage) -> io::Result<()> {
        self.with_revalidated(storage, |_| Ok(()))
    }
    fn with_revalidated<T>(
        &self,
        storage: &AttachmentStorage,
        apply: impl FnOnce(NativeConsumedStartBasis<'_>) -> io::Result<T>,
    ) -> io::Result<T> {
        let available = self.inner.available.iter().collect::<Vec<_>>();
        let request = NativeTrustedConsumedStartRequest {
            source: &self.inner.source,
            operation: self.inner.version.operation(),
            destination: &self.inner.destination,
            grant: self.inner.grant,
            available: &available,
            request: self.inner.request,
            limits: self.inner.limits,
        };
        storage.with_native_consumed_start_basis(
            &self.inner.owner,
            &request,
            self.inner.actor,
            &self.trusted,
            |material| {
                self.inner.verify_prepared_basis(&material.basis)?;
                if material.version != self.inner.version {
                    return Err(invalid("native saved source identity changed"));
                }
                apply(material)
            },
        )
    }
    /// Durably fence this exact signed input. No destination file is installed or runnable.
    pub fn fence_consumption_start(
        &self,
        storage: &AttachmentStorage,
        staged: &StagedNativeConsumedStart,
    ) -> io::Result<RecordDigest> {
        self.commit_native_with_io(
            storage,
            staged,
            NativeConsumedCommitPhase::Fence,
            |_, _, _| Ok(()),
            |file| file.sync_all(),
        )
    }
    /// Install the exact retained input beneath its fence. History and runtime stay blocked.
    pub fn install_fenced_consumed_start(
        &self,
        storage: &AttachmentStorage,
        staged: &StagedNativeConsumedStart,
    ) -> io::Result<RecordDigest> {
        self.commit_native_with_io(
            storage,
            staged,
            NativeConsumedCommitPhase::Install,
            |_, _, _| Ok(()),
            |file| file.sync_all(),
        )
    }
    /// Durably append the original signed checkpoint. Owner and completion remain unfinished.
    pub fn commit_fenced_consumed_checkpoint(
        &self,
        storage: &AttachmentStorage,
        staged: &StagedNativeConsumedStart,
    ) -> io::Result<RecordDigest> {
        self.commit_native_with_io(
            storage,
            staged,
            NativeConsumedCommitPhase::Checkpoint,
            |_, _, _| Ok(()),
            |file| file.sync_all(),
        )?;
        Ok(self.operation())
    }
    fn with_write_proof<T>(
        &self,
        storage: &AttachmentStorage,
        apply: impl FnOnce(NativeConsumptionWriteProof<'_>) -> io::Result<T>,
    ) -> io::Result<T> {
        let available = self.inner.available.iter().collect::<Vec<_>>();
        storage.with_native_consumption_write_proof(
            &self.inner.owner,
            &NativeTrustedConsumedStartRequest {
                source: &self.inner.source,
                operation: self.inner.version.operation(),
                destination: &self.inner.destination,
                grant: self.inner.grant,
                available: &available,
                request: self.inner.request,
                limits: self.inner.limits,
            },
            &self.trusted,
            |proof| {
                if proof.version() != self.inner.version
                    || proof.destination() != &self.inner.basis.destination
                    || proof.graph().digest() != self.inner.basis.graph
                {
                    return Err(invalid("native consumption candidate selection changed"));
                }
                apply(proof)
            },
        )
    }
    pub(in crate::project_attachment) fn commit_native_with_io(
        &self,
        storage: &AttachmentStorage,
        staged: &StagedNativeConsumedStart,
        phase: NativeConsumedCommitPhase,
        hook: impl FnMut(&str, &mut std::fs::File, &[u8]) -> io::Result<()>,
        sync: impl FnMut(&std::fs::File) -> io::Result<()>,
    ) -> io::Result<RecordDigest> {
        self.commit_native_with_hooks(storage, staged, phase, (hook, sync), || Ok(()))
    }
    pub(in crate::project_attachment) fn commit_native_with_hooks(
        &self,
        storage: &AttachmentStorage,
        staged: &StagedNativeConsumedStart,
        phase: NativeConsumedCommitPhase,
        io: (
            impl FnMut(&str, &mut std::fs::File, &[u8]) -> io::Result<()>,
            impl FnMut(&std::fs::File) -> io::Result<()>,
        ),
        after_custody: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<RecordDigest> {
        let (hook, sync) = io;
        let result = self.with_write_proof(storage, |proof| {
            self.inner
                .commit_native_material_held(storage, staged, phase, &proof, hook, sync)
        })?;
        after_custody()?;
        self.with_write_proof(storage, |proof| {
            proof.recover(
                if phase == NativeConsumedCommitPhase::Fence {
                    RecoveryPhase::Start
                } else {
                    RecoveryPhase::Installed
                },
                |candidate, current, _, _| {
                    if candidate.operation() != self.operation()
                        || current.receipt()? != staged.receipt()?
                        || current.root.identity()? != staged.root.identity()?
                    {
                        return Err(invalid("native consumption changed before acknowledgement"));
                    }
                    Ok(())
                },
            )
        })?;
        Ok(result)
    }
    #[cfg(test)]
    pub(in crate::project_attachment) fn try_ordinary_owner_for_test(
        &self,
        storage: &AttachmentStorage,
        staged: &StagedNativeConsumedStart,
    ) -> io::Result<RecordDigest> {
        self.inner.commit_fenced_owner_receipt(storage, staged)
    }
    #[cfg(test)]
    pub(in crate::project_attachment) fn try_ordinary_fence_for_test(
        &self,
        storage: &AttachmentStorage,
        staged: &StagedNativeConsumedStart,
    ) -> io::Result<RecordDigest> {
        self.inner.fence_consumption_start(storage, staged)
    }
    /// Preserve exact signed material privately. This does not install, commit or start a lane.
    pub fn stage(&self, storage: &AttachmentStorage) -> io::Result<StagedNativeConsumedStart> {
        self.stage_with_hook(storage, |_, _| Ok(()))
    }
    pub(in crate::project_attachment) fn stage_with_hook(
        &self,
        storage: &AttachmentStorage,
        mut hook: impl FnMut(&str, &PinnedWorkspaceRoot) -> io::Result<()>,
    ) -> io::Result<StagedNativeConsumedStart> {
        let staged = self.with_revalidated(storage, |material| {
            self.inner.stage_material_held(
                material.graph,
                material.allocation,
                material.guard,
                &mut hook,
            )
        })?;
        hook("custody-released", &staged.root)?;
        // Fresh complete history and all staged bytes are checked after custody release as well.
        self.with_revalidated(storage, |material| {
            self.inner.verify_native_stage_held(
                &staged,
                material.graph,
                material.allocation,
                material.guard,
            )
        })?;
        Ok(staged)
    }
}
