//! Current native access inspection, retaining custody through the read callback.
use super::{
    dependency_decision::PENDING, dependency_enrollment::read_private_in_store,
    history::verify_history_binding, invalid, AttachmentStorage, ProvisionedAttachment,
    SavedAttachmentVersion,
};
use crate::{
    workspace::{HistoricalWorkspacePreview, OpenWorkspace},
    TrustedReviewers,
};
use mesh_store::RecordDigest;
use std::io::{self, Write};

/// Exact native selection and expected grant. No caller-provided generation or policy path.
pub struct NativeGrantInspection<'a> {
    /// Native source registration selected within the owning project.
    pub source: &'a ProvisionedAttachment,
    /// Exact immutable source operation.
    pub version: SavedAttachmentVersion,
    /// Native destination registration named by the grant.
    pub destination: &'a ProvisionedAttachment,
    /// Exact currently allowed grant record; historical or revoked records cannot substitute.
    pub grant: RecordDigest,
}
/// Immutable file metadata from the exact admitted saved snapshot.
pub struct NativeGrantedFile<'a> {
    /// Relative saved path, not a path resolved by the renderer or agent.
    pub path: &'a str,
    /// Verified saved byte length.
    pub byte_length: u64,
    /// Whether the saved file is executable.
    pub executable: bool,
}
/// Read-only source view valid only inside the synchronous custody-bound callback.
/// Extracted bytes/metadata are facts; they do not retain access or publication authority.
pub struct NativeGrantedInput<'a> {
    history: &'a OpenWorkspace,
    snapshot: &'a HistoricalWorkspacePreview,
}
impl NativeGrantedInput<'_> {
    pub(super) fn starting_exclusion_rules(&self) -> io::Result<(Option<String>, Option<String>)> {
        let read = |name: &str| -> io::Result<Option<String>> {
            let Some(file) = self.files().find(|file| file.path == name) else {
                return Ok(None);
            };
            if file.byte_length > 65_536 {
                return Err(invalid("starting exclusion rules exceed their bound"));
            }
            let mut bytes = Vec::new();
            self.write_file(name, &mut bytes)?;
            String::from_utf8(bytes).map(Some).map_err(error)
        };
        Ok((read(".gitignore")?, read(".meshignore")?))
    }

    /// Enumerate exact saved files, never current editor contents.
    pub fn files(&self) -> impl Iterator<Item = NativeGrantedFile<'_>> {
        self.snapshot.files.iter().map(|file| NativeGrantedFile {
            path: &file.path,
            byte_length: file.byte_length,
            executable: file.executable,
        })
    }
    /// Enumerate exact saved directory paths, including empty directories.
    pub fn directories(&self) -> impl Iterator<Item = &str> {
        self.snapshot
            .directories
            .iter()
            .map(|directory| directory.path.as_str())
    }
    /// Read only a file named by this exact verified snapshot. Content verification remains native.
    pub fn write_file(&self, path: &str, output: &mut impl Write) -> io::Result<()> {
        let file = self
            .snapshot
            .files
            .iter()
            .find(|file| file.path == path)
            .ok_or_else(|| invalid("file is not in the granted snapshot"))?;
        self.history
            .write_historical_workspace_file(file, output)
            .map_err(|failure| match failure {
                crate::workspace::HistoricalWorkspaceWriteFailure::Output(error) => error,
                crate::workspace::HistoricalWorkspaceWriteFailure::Retained(_) => {
                    invalid("granted saved content verification failed")
                }
            })
    }
}
fn error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
pub(super) struct PreparedNativeGrantInspection<'a> {
    owner: &'a ProvisionedAttachment,
    request: NativeGrantInspection<'a>,
    source: super::dependency_work::PreparedDependencyWork,
    destination: super::dependency_work::PreparedDependencyWork,
    pub(super) roots: Vec<crate::root_authority::PinnedWorkspaceRoot>,
}

impl AttachmentStorage {
    /// Inspect an exact currently granted input while retaining all native custody. The callback
    /// must remain synchronous and bounded; it must not wait for a provider, network or user.
    /// No agent/renderer/CLI route calls this native API. It performs no materialization, records no
    /// consumption and cannot authorize publication. Errors do not roll back callback side effects.
    pub fn with_current_input_grant<T>(
        &self,
        owner: &ProvisionedAttachment,
        request: NativeGrantInspection<'_>,
        read: impl FnOnce(NativeGrantedInput<'_>) -> io::Result<T>,
    ) -> io::Result<T> {
        let prepared = self.prepare_input_grant(owner, request)?;
        let guard = crate::workspace_custody::lock_workspace_initialization_set(&prepared.roots)
            .map_err(error)?;
        self.with_prepared_input_grant(&prepared, &guard, read)
    }

    // Preparation pins identities only. Current authorization is checked under held custody.
    pub(super) fn prepare_input_grant<'a>(
        &self,
        owner: &'a ProvisionedAttachment,
        request: NativeGrantInspection<'a>,
    ) -> io::Result<PreparedNativeGrantInspection<'a>> {
        let source = self.prepare_dependency_work(owner, request.source)?;
        let destination = self.prepare_dependency_work(owner, request.destination)?;
        let mut roots = source.roots.clone();
        roots.extend(destination.roots.iter().cloned());
        Ok(PreparedNativeGrantInspection {
            owner,
            request,
            source,
            destination,
            roots,
        })
    }

    pub(super) fn with_prepared_input_grant<T>(
        &self,
        prepared: &PreparedNativeGrantInspection<'_>,
        guard: &crate::workspace_custody::WorkspaceInitializationGuard,
        read: impl FnOnce(NativeGrantedInput<'_>) -> io::Result<T>,
    ) -> io::Result<T> {
        let context = super::dependency_owner_context::OwnerHistoryContext::current(prepared.owner);
        self.with_input_grant_owner(prepared, guard, &context, read)
    }
    pub(super) fn with_input_grant_owner<T>(
        &self,
        prepared: &PreparedNativeGrantInspection<'_>,
        guard: &crate::workspace_custody::WorkspaceInitializationGuard,
        context: &super::dependency_owner_context::OwnerHistoryContext<'_>,
        read: impl FnOnce(NativeGrantedInput<'_>) -> io::Result<T>,
    ) -> io::Result<T> {
        guard.require_roots(&prepared.roots).map_err(error)?;
        let owner = prepared.owner;
        let request = &prepared.request;
        let source = &prepared.source;
        let destination = &prepared.destination;
        let validate = || -> io::Result<OpenWorkspace> {
            let pending_control = match read_private_in_store(&owner.store, PENDING) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => false,
                Ok(raw) if context.matches_pending_control(&raw) => true,
                Err(error) => return Err(error),
                Ok(_) => {
                    return Err(invalid(
                        "native control must finish recovery before grant admission",
                    ))
                }
            };
            let (configuration, proof) = context.read(owner)?;
            let proof = proof.ok_or_else(|| invalid("owning dependency enrollment is required"))?;
            let history = OpenWorkspace::open_attachment_read_history(
                owner.metadata_path(),
                owner.store.clone(),
                &TrustedReviewers::default(),
                Some(&proof),
            )
            .map_err(error)?;
            verify_history_binding(&history, &configuration)?;
            let source_binding = context.validate(self, source, guard)?;
            let destination_binding = context.validate(self, destination, guard)?;
            let historical = context.historical_input(
                &proof,
                (
                    source_binding.work(),
                    source_binding.installation(),
                    request.version.operation(),
                ),
                (
                    destination_binding.work(),
                    destination_binding.installation(),
                ),
                request.grant,
                (source_binding.correlation, destination_binding.correlation),
            );
            let current = proof.policy().current_bound_grant(
                (
                    source_binding.work(),
                    source_binding.installation(),
                    request.version.operation(),
                ),
                (
                    destination_binding.work(),
                    destination_binding.installation(),
                ),
                request.grant,
                (source_binding.correlation, destination_binding.correlation),
            );
            if !historical && (pending_control || !current) {
                return Err(invalid(
                    "native input grant is stale, revoked, unfinished, unbound or mismatched",
                ));
            }
            guard.ensure_current().map_err(error)?;
            Ok(history)
        };
        let owner_history = validate()?;
        let value = if request.source.id() == owner.id() {
            SavedAttachmentVersion::from_verified_history(
                &owner_history,
                request.version.operation(),
            )?;
            let snapshot = owner_history
                .historical_workspace_preview(request.version.operation())
                .map_err(error)?;
            read(NativeGrantedInput {
                history: &owner_history,
                snapshot: &snapshot,
            })?
        } else if context.has_verified_history(request.source) {
            let (_, _, history) = context.history(request.source)?;
            SavedAttachmentVersion::from_verified_history(&history, request.version.operation())?;
            let snapshot = history
                .historical_workspace_preview(request.version.operation())
                .map_err(error)?;
            read(NativeGrantedInput {
                history: &history,
                snapshot: &snapshot,
            })?
        } else {
            request.source.attachment.inspect_saved(
                request.source.metadata_path(),
                request.source.store.clone(),
                &request.version.operation().to_string(),
                |history, operation| {
                    let snapshot = history
                        .historical_workspace_preview(operation)
                        .map_err(error)?;
                    read(NativeGrantedInput {
                        history,
                        snapshot: &snapshot,
                    })
                },
            )?
        };
        // A callback result is acknowledged only if native associations and current access still agree.
        validate()?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests;
