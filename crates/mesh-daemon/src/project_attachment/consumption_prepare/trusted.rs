//! Explicitly configured native history; ordinary candidates never acquire this admission.
use super::*;
use crate::{
    project_attachment::dependency_private_context::NativeConsumedStartBasis,
    root_authority::PinnedWorkspaceRoot, TrustedReviewers,
};

/// Native-host selectors. Each phase independently authenticates the saved operation and grant.
/// These IDs and handles are not permission, publication approval or a runnable destination.
pub struct NativeTrustedConsumedStartRequest<'a> {
    /// Original source handle, including its registration and physical namespace.
    pub source: &'a ProvisionedAttachment,
    /// Exact saved operation; no caller-provided saved-version proof is accepted.
    pub operation: RecordDigest,
    /// Original empty correlated destination.
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
    /// Fresh point-in-time validation; no continuing mutation or input permission.
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
