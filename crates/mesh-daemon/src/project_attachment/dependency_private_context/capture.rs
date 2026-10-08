//! Private authoring through complete, explicitly trusted publication history.
use super::*;
use crate::checkpoint_storage::PreparedAuthenticatedCheckpoint;
use crate::project_attachment::history::dependency_capture::{
    absent, receipt_name, retain_receipt, CaptureIntent, MAX_JOURNAL, PENDING,
};
use crate::project_attachment::{CapturedProjectInput, NativeCaptureDraft, SavedAttachmentVersion};
use crate::root_authority::PinnedRootFs;
use crate::TrustedReviewers;
use mesh_cas::{Blake3, Cas, DurableFs as _};
use mesh_crypto::SigningPayload;
use mesh_types::{PublicKey, Signature};
use std::fs::{self, File};
use std::io::{Read as _, Seek as _, Write as _};
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::Path;

/// Signed private progress. No protected-main authority or runtime admission is conferred.
/// Commit reopens complete native history and rechecks the saved basis after signing.
pub struct PreparedVerifiedNativeCapture {
    storage: AttachmentStorage,
    selected: ProvisionedAttachment,
    trusted: TrustedReviewers,
    draft: NativeCaptureDraft,
    prepared: PreparedAuthenticatedCheckpoint,
    request: RecordDigest,
}
fn available(
    owner: &ProvisionedAttachment,
    work: &ProvisionedAttachment,
    request: RecordDigest,
) -> io::Result<()> {
    for selected in [owner, work] {
        super::super::detachment::ensure_attached(&selected.store)?;
        absent(&selected.store, PENDING)?;
        absent(&selected.store, super::super::dependency_decision::PENDING)?;
        absent(&selected.store, super::publication::PENDING)?;
    }
    absent(&work.store, &receipt_name(request))
}
fn journal_bytes(file: &mut File) -> io::Result<Vec<u8>> {
    file.rewind()?;
    let mut bytes = Vec::new();
    (&mut *file)
        .take((MAX_JOURNAL + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_JOURNAL {
        return Err(invalid("capture exceeds native journal bound"));
    }
    Ok(bytes)
}
impl AttachmentStorage {
    /// Inspect exact local capture recovery objects through configured publication trust.
    /// This performs no recovery, acquires no durable pin and cannot authorize collection.
    /// Owner/source stores and other transaction sidecars need their own complete retention proof.
    pub fn inspect_verified_dependency_capture_retention(
        &self,
        selected: &ProvisionedAttachment,
        request: RecordDigest,
        trusted: &TrustedReviewers,
    ) -> io::Result<crate::project_attachment::NativeCaptureRetention> {
        self.exact_registered_work(selected)?;
        self.with_capture_write(
            selected.id(),
            Some(request),
            |context, work, _, guard, recovery| {
                self.exact_registered_work(selected)?;
                let recovery =
                    recovery.ok_or_else(|| invalid("capture retention evidence missing"))?;
                for root in [context.owner, work] {
                    super::super::detachment::ensure_attached(&root.store)?;
                    absent(&root.store, super::super::dependency_decision::PENDING)?;
                    absent(&root.store, super::publication::PENDING)?;
                }
                if context.owner.id() != work.id() {
                    absent(&context.owner.store, PENDING)?;
                }
                let inspect = || {
                    context.with_replayed_history(self, work, trusted, guard, |_, _| Ok(()))?;
                    let snapshot = context.read(work, guard)?;
                    work.inspect_capture_retention_with_context(request, |raw| {
                        if raw != recovery.raw_for(work)? {
                            return Err(invalid("capture retention intent changed"));
                        }
                        Ok((snapshot.configuration.clone(), snapshot.proof.clone()))
                    })
                };
                let retained = inspect()?;
                if inspect()? != retained {
                    return Err(invalid("capture retention changed during inspection"));
                }
                self.exact_registered_work(selected)?;
                Ok(retained)
            },
        )
    }

    /// Prepare a private save with explicitly configured publication trust. The signer executes
    /// outside native custody. This neither enrolls work nor changes accepted main or eligibility.
    pub fn prepare_verified_dependency_capture<F, E>(
        &self,
        selected: &ProvisionedAttachment,
        input: &CapturedProjectInput,
        actor: PublicKey,
        request: RecordDigest,
        trusted: &TrustedReviewers,
        sign: F,
    ) -> io::Result<PreparedVerifiedNativeCapture>
    where
        F: FnOnce(&SigningPayload) -> Result<Signature, E>,
        E: std::fmt::Display,
    {
        if request == RecordDigest::from_bytes([0; 32]) {
            return Err(invalid("missing native capture request"));
        }
        self.exact_registered_work(selected)?;
        let draft = self.with_private_inspection(selected.id(), |context, work, guard| {
            self.exact_registered_work(selected)?;
            available(context.owner, work, request)?;
            work.check_dependency_registration()?;
            work.project().ensure_current()?;
            if input.root() != work.project().root()
                || input.identity() != work.project().pinned.identity()?
            {
                return Err(invalid("capture belongs to a different native attachment"));
            }
            let configuration = &context.read(work, guard)?.configuration;
            work.project().history_configuration_with_previous(
                &work.store,
                Some(input.exclusion_digest()),
                Some(configuration.clone()),
            )?;
            context.with_replayed_history(self, work, trusted, guard, |history, _| {
                history.prepare_capture(configuration, input, actor)
            })
        })?;
        let signature = sign(&draft.signing_payload()).map_err(error)?;
        let prepared = self.with_private_inspection(selected.id(), |context, work, guard| {
            self.exact_registered_work(selected)?;
            available(context.owner, work, request)?;
            if context.read(work, guard)?.configuration != draft.configuration {
                return Err(invalid(
                    "native capture configuration changed while signing",
                ));
            }
            context.with_replayed_history(self, work, trusted, guard, |history, _| {
                history.authenticate_capture(&draft, signature)
            })
        })?;
        Ok(PreparedVerifiedNativeCapture {
            storage: self.clone(),
            selected: selected.clone(),
            trusted: trusted.clone(),
            draft,
            prepared,
            request,
        })
    }
    /// Recover exactly one retained private capture. Configured trust is checked before any
    /// append; historical receipt retries never rewind the current capture position.
    pub fn recover_verified_dependency_capture(
        &self,
        selected: &ProvisionedAttachment,
        request: RecordDigest,
        trusted: &TrustedReviewers,
    ) -> io::Result<SavedAttachmentVersion> {
        self.recover_verified_capture_with_sync(selected, request, trusted, |file| file.sync_all())
    }
    pub(in crate::project_attachment) fn recover_verified_capture_with_sync(
        &self,
        selected: &ProvisionedAttachment,
        request: RecordDigest,
        trusted: &TrustedReviewers,
        mut sync: impl FnMut(&File) -> io::Result<()>,
    ) -> io::Result<SavedAttachmentVersion> {
        self.exact_registered_work(selected)?;
        self.with_capture_write(
            selected.id(),
            Some(request),
            |context, work, works, guard, recovery| {
                self.exact_registered_work(selected)?;
                let recovery =
                    recovery.ok_or_else(|| invalid("capture recovery evidence missing"))?;
                for root in [context.owner, work] {
                    super::super::detachment::ensure_attached(&root.store)?;
                    absent(&root.store, super::super::dependency_decision::PENDING)?;
                    absent(&root.store, super::publication::PENDING)?;
                }
                if context.owner.id() != work.id() {
                    absent(&context.owner.store, PENDING)?;
                }
                work.check_dependency_registration()?;
                let snapshot = context.read(work, guard)?;
                let configuration = &snapshot.configuration;
                let main_before =
                    context.with_replayed_history(self, work, trusted, guard, |history, _| {
                        Ok(history.verified_publication())
                    })?;
                let mut journal = work
                    .store
                    .open_existing_record_file(Path::new(crate::RECORD_FILE_NAME))?;
                let bytes = journal_bytes(&mut journal)?;
                snapshot.proof.verify(&work.store, &journal, &bytes)?;
                recovery.verify(
                    &work.store,
                    &journal,
                    &bytes,
                    snapshot.proof.binding().authority,
                    configuration,
                )?;
                let written = bytes.len() - recovery.intent.before_bytes;
                guard.ensure_current().map_err(error)?;
                if written < recovery.frames.len() {
                    journal.write_all(&recovery.frames[written..])?;
                }
                sync(&journal)?;
                let after = PrivateContext::resolve(self, context.owner, works, guard)?;
                if after.read(work, guard)?.configuration != *configuration {
                    return Err(invalid("capture recovery configuration changed"));
                }
                let (saved, line) =
                    after.with_replayed_history(self, work, trusted, guard, |history, _| {
                        if history.verified_publication() != main_before {
                            return Err(invalid("private capture recovery changed accepted main"));
                        }
                        history.capture_result(
                            recovery.intent.operation,
                            recovery.intent.head,
                            configuration,
                        )
                    })?;
                self.exact_registered_work(selected)?;
                if recovery.pending {
                    if read_private_in_store(&work.store, PENDING)? != recovery.raw {
                        return Err(invalid("capture recovery intent changed after append"));
                    }
                    if line.head == Some(recovery.intent.operation) {
                        line.persist(&work.store, configuration)?;
                    } else if line.head == recovery.intent.head {
                        line.begin(recovery.intent.operation, &work.store, configuration)?;
                        line.finish(recovery.intent.operation, &work.store, configuration)?;
                    } else {
                        return Err(invalid(
                            "capture recovery would rewind a different private line",
                        ));
                    }
                    retain_receipt(&work.store, request, &recovery.raw)?;
                    work.store.filesystem().remove_file(Path::new(PENDING))?;
                    work.store.sync()?;
                }
                Ok(saved)
            },
        )
    }

    // A single write callback under the complete revalidated root set. Unlike private inspection,
    // this callback must never be repeated. The writer verifies its exact before/after journal.
    fn with_capture_write<T>(
        &self,
        work_id: &str,
        recover: Option<RecordDigest>,
        write: impl FnOnce(
            &PrivateContext<'_>,
            &ProvisionedAttachment,
            &[ProvisionedAttachment],
            &WorkspaceInitializationGuard,
            Option<&recovery::VerifiedCapturePrefix>,
        ) -> io::Result<T>,
    ) -> io::Result<T> {
        let owner = self.reopen(&self.candidate_owning_root(work_id)?)?;
        let work = self.reopen(work_id)?;
        let owner_selection = self.prepare_dependency_work(&owner, &work)?;
        let hints = self.catalog_discovery_hints(&owner)?;
        let (discovery, capture) = {
            let guard =
                crate::workspace_custody::lock_workspace_initialization_set(&owner_selection.roots)
                    .map_err(error)?;
            let capture = recover
                .map(|request| recovery::VerifiedCapturePrefix::read(&work, request))
                .transpose()?;
            let owner_recovery = capture
                .as_ref()
                .filter(|proof| proof.applies(&owner))
                .map(OwnerRecovery::Capture);
            let discovery = self.private_discovery_with_recovery(
                &owner,
                work_id,
                &guard,
                hints,
                owner_recovery,
            )?;
            (discovery, capture)
        };
        let works = discovery
            .works
            .keys()
            .map(|id| self.reopen(id))
            .collect::<io::Result<Vec<_>>>()?;
        let available = works.iter().collect::<Vec<_>>();
        let prepared = self.prepare_dependency_graph(
            &owner,
            &work,
            RecordDigest::from_bytes([0; 32]),
            &available,
        )?;
        let guard = crate::workspace_custody::lock_workspace_initialization_set(&prepared.roots)
            .map_err(error)?;
        let owner_recovery = capture
            .as_ref()
            .filter(|proof| proof.applies(&owner))
            .map(OwnerRecovery::Capture);
        if self.private_discovery_with_recovery(
            &owner,
            work_id,
            &guard,
            self.catalog_discovery_hints(&owner)?,
            owner_recovery,
        )? != discovery
        {
            return Err(invalid("native capture discovery changed"));
        }
        let context = PrivateContext::resolve_with_capture(
            self,
            &owner,
            &works,
            &guard,
            owner_recovery,
            capture.as_ref(),
        )?;
        let result = write(&context, &work, &works, &guard, capture.as_ref())?;
        guard.ensure_current().map_err(error)?;
        Ok(result)
    }
}
impl PreparedVerifiedNativeCapture {
    /// Candidate identity, not a durable saved acknowledgement until commit succeeds.
    pub fn operation(&self) -> RecordDigest {
        self.prepared.changeset_id
    }
    /// Append only private authoring records, with journal synchronization as the commit point.
    /// Failed writes retain their exact intent and material for native recovery.
    pub fn commit(self) -> io::Result<SavedAttachmentVersion> {
        self.commit_with_io(|_, _, _| Ok(()), |f| f.sync_all())
    }
    pub(in crate::project_attachment) fn commit_with_io(
        self,
        mut hook: impl FnMut(&str, &mut File, &[u8]) -> io::Result<()>,
        mut sync: impl FnMut(&File) -> io::Result<()>,
    ) -> io::Result<SavedAttachmentVersion> {
        self.storage.with_capture_write(
            self.selected.id(),
            None,
            |context, work, works, guard, _| {
                self.storage.exact_registered_work(&self.selected)?;
                available(context.owner, work, self.request)?;
                work.check_dependency_registration()?;
                let before_snapshot = context.read(work, guard)?;
                let configuration = &before_snapshot.configuration;
                if *configuration != self.draft.configuration {
                    return Err(invalid("native capture configuration changed"));
                }
                let main_before = context.with_replayed_history(
                    &self.storage,
                    work,
                    &self.trusted,
                    guard,
                    |history, _| {
                        history.verify_capture_basis(&self.draft)?;
                        Ok(history.verified_publication())
                    },
                )?;
                let mut journal = work
                    .store
                    .open_existing_record_file(Path::new(crate::RECORD_FILE_NAME))?;
                let before = journal_bytes(&mut journal)?;
                before_snapshot
                    .proof
                    .verify(&work.store, &journal, &before)?;
                let records = self.prepared.checkpoint.records();
                if records.iter().any(|r| {
                    !matches!(
                        r,
                        mesh_store::StoredRecord::Manifest(_)
                            | mesh_store::StoredRecord::Operation(_)
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
                let cas = Cas::<PinnedRootFs, Blake3>::with_filesystem(
                    work.metadata_path(),
                    work.store.filesystem(),
                )
                .map_err(error)?;
                for bytes in &self.prepared.objects {
                    cas.promote(bytes.clone()).map_err(error)?;
                }
                cas.promote(frames.clone()).map_err(error)?;
                let metadata = journal.metadata()?;
                let intent = CaptureIntent {
                    request: self.request,
                    authority: before_snapshot.proof.binding().authority,
                    configuration: hash(configuration.as_bytes()),
                    journal: (metadata.dev(), metadata.ino()),
                    before_bytes: before.len(),
                    before_digest: hash(&before),
                    frames: hash(&frames),
                    operation: self.prepared.changeset_id,
                    head: self.draft.line.head,
                }
                .encode();
                self.draft.line.persist(&work.store, configuration)?;
                work.store.filesystem().write_new_file(
                    Path::new(PENDING),
                    intent.as_bytes(),
                    fs::Permissions::from_mode(0o600),
                )?;
                work.store.sync()?;
                self.draft
                    .line
                    .begin(self.prepared.changeset_id, &work.store, configuration)?;
                hook("staged", &mut journal, &frames)?;
                context.with_replayed_history(
                    &self.storage,
                    work,
                    &self.trusted,
                    guard,
                    |history, _| history.verify_capture_basis(&self.draft),
                )?;
                guard.ensure_current().map_err(error)?;
                if read_private_in_store(&work.store, PENDING)? != intent
                    || journal_bytes(&mut journal)? != before
                    || super::super::dependency_transaction::read_payload(
                        &cas,
                        hash(&frames),
                        MAX_JOURNAL,
                    )? != frames
                {
                    return Err(invalid("native capture changed before append"));
                }
                journal.write_all(&frames)?;
                sync(&journal)?;
                hook("appended", &mut journal, &frames)?;
                let after = PrivateContext::resolve(&self.storage, context.owner, works, guard)?;
                if after.read(work, guard)?.configuration != *configuration {
                    return Err(invalid("native capture configuration changed after append"));
                }
                let saved = after.with_replayed_history(
                    &self.storage,
                    work,
                    &self.trusted,
                    guard,
                    |history, _| {
                        if history.verified_publication() != main_before {
                            return Err(invalid("private capture changed accepted main"));
                        }
                        history.with_saved_input(self.prepared.changeset_id, |input| {
                            for file in input.files() {
                                input.write_file(file.path, &mut io::sink())?;
                            }
                            Ok(())
                        })?;
                        history.saved_version(self.prepared.changeset_id)
                    },
                )?;
                self.storage.exact_registered_work(&self.selected)?;
                if read_private_in_store(&work.store, PENDING)? != intent {
                    return Err(invalid("native capture intent changed"));
                }
                self.draft
                    .line
                    .finish(self.prepared.changeset_id, &work.store, configuration)?;
                retain_receipt(&work.store, self.request, &intent)?;
                work.store.filesystem().remove_file(Path::new(PENDING))?;
                work.store.sync()?;
                Ok(saved)
            },
        )
    }
}

pub(in crate::project_attachment) mod recovery;
