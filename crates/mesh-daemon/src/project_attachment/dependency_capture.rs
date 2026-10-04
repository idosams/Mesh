//! Dedicated enrolled capture writer. History inspection remains read-only and cannot publish.
use super::*;
use crate::checkpoint_storage::{
    prepare_authenticated_checkpoint, PreparedAuthenticatedCheckpoint,
};
use crate::project_attachment::{
    capture_line::CaptureLine,
    dependency_decision,
    dependency_enrollment::read_private_in_store,
    dependency_transaction::{digest, hash, read_payload, text},
    ProvisionedAttachment,
};
use std::io::{Seek as _, Write as _};

#[path = "consumed_capture.rs"]
mod consumed;
use consumed::{CaptureRead, ConsumedCaptureContext};

const PENDING: &str = "dependency-capture.pending";
const MAX_JOURNAL: usize = 80 * 1024 * 1024;

/// Signed private capture prepared without granting publication or holding native custody.
/// Commit must refresh exact enrollment and history; signing can never reserve the commit order.
pub struct PreparedNativeCapture {
    consumption: Option<ConsumedCaptureContext>,
    attachment: ProvisionedAttachment,
    configuration: String,
    binding: crate::dependency_policy::NativeDependencyBinding,
    basis: ManagedAuthoringBasis,
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
        self.prepare_capture_in(input, actor, request, sign, None)
    }
    fn prepare_capture_in<F, E>(
        &self,
        input: &CapturedProjectInput,
        actor: PublicKey,
        request: RecordDigest,
        sign: F,
        consumption: Option<ConsumedCaptureContext>,
    ) -> io::Result<PreparedNativeCapture>
    where
        F: FnOnce(&SigningPayload) -> Result<Signature, E>,
        E: std::fmt::Display,
    {
        if request == RecordDigest::from_bytes([0; 32]) {
            return Err(invalid("missing native capture request"));
        }
        let (configuration, proof, workspace, line, basis, operations, files) = self
            .with_capture_custody(consumption.as_ref(), |read| {
                self.check_dependency_registration()?;
                self.attachment.ensure_current()?;
                if input.root() != self.attachment.root()
                    || input.identity() != self.attachment.pinned.identity()?
                {
                    return Err(invalid("capture belongs to a different native attachment"));
                }
                absent(&self.store, PENDING)?;
                absent(&self.store, &receipt_name(request))?;
                absent(&self.store, dependency_decision::PENDING)?;
                crate::project_attachment::detachment::ensure_attached(&self.store)?;
                let (configuration, proof) = read(None)?;
                self.attachment.history_configuration_with_previous(
                    &self.store,
                    Some(input.exclusion_digest()),
                    Some(configuration.clone()),
                )?;

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
                Ok((
                    configuration,
                    proof,
                    workspace,
                    line,
                    basis,
                    operations,
                    files,
                ))
            })?;
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
            consumption,
            attachment: self.clone(),
            configuration,
            binding: proof.binding(),
            basis,
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
        self.commit_with_io(|_, _, _| Ok(()), |file| file.sync_all())
    }

    fn commit_with_io(
        self,
        hook: impl FnMut(CaptureStep, &mut fs::File, &[u8]) -> io::Result<()>,
        sync: impl FnMut(&fs::File) -> io::Result<()>,
    ) -> io::Result<SavedAttachmentVersion> {
        let attachment = self.attachment.clone();
        let consumption = self.consumption.clone();
        attachment.with_capture_custody(consumption.as_ref(), |read| {
            self.commit_held(read, hook, sync)
        })
    }
    fn commit_held(
        self,
        read: &CaptureRead<'_>,
        mut hook: impl FnMut(CaptureStep, &mut fs::File, &[u8]) -> io::Result<()>,
        mut sync: impl FnMut(&fs::File) -> io::Result<()>,
    ) -> io::Result<SavedAttachmentVersion> {
        let a = &self.attachment;
        a.check_dependency_registration()?;
        a.attachment.ensure_current()?;
        crate::project_attachment::detachment::ensure_attached(&a.store)?;
        absent(&a.store, PENDING)?;
        absent(&a.store, &receipt_name(self.request))?;
        absent(&a.store, dependency_decision::PENDING)?;
        let (configuration, proof) = read(None)?;
        if configuration != self.configuration || proof.binding() != self.binding {
            return Err(invalid("native capture basis changed while signing"));
        }
        let history = OpenWorkspace::open_attachment_read_history(
            a.metadata_path(),
            a.store.clone(),
            &crate::TrustedReviewers::default(),
            Some(&proof),
        )
        .map_err(error)?;
        verify_history_binding(&history, &configuration)?;
        if CaptureLine::load(&a.store, &history, &configuration)? != self.line {
            return Err(invalid("native capture line changed while signing"));
        }
        // Policy decisions do not change private authoring. Recheck its actual signed basis,
        // including actor advancement outside a stale/rolled-back mutable capture position.
        if let Some(head) = self.line.head {
            let current = history
                .historical_authoring_basis(
                    head,
                    PublicKey::from_bytes(*self.basis.actor_id.as_bytes()),
                )
                .map_err(error)?;
            if current != self.basis {
                return Err(invalid(
                    "native capture authoring basis changed while signing",
                ));
            }
        } else if history.operations() != 0 {
            return Err(invalid("native capture genesis changed while signing"));
        }
        let mut journal = a
            .store
            .open_existing_record_file(Path::new(crate::RECORD_FILE_NAME))?;
        journal.rewind()?;
        let mut before = Vec::new();
        (&mut journal)
            .take((MAX_JOURNAL + 1) as u64)
            .read_to_end(&mut before)?;
        proof.verify(&a.store, &journal, &before)?;
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
        let intent = CaptureIntent {
            request: self.request,
            authority: self.binding.authority,
            configuration: hash(configuration.as_bytes()),
            journal: (metadata.dev(), metadata.ino()),
            before_bytes: before.len(),
            before_digest: hash(&before),
            frames: hash(&frames),
            operation: self.prepared.changeset_id,
            head: self.line.head,
        }
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
        hook(CaptureStep::Staged, &mut journal, &frames)?;
        let (current, after_staging) = read(None)?;
        if current != configuration
            || after_staging != proof
            || read_private_in_store(&a.store, PENDING)? != intent
            || read_payload(&cas, hash(&frames), MAX_JOURNAL)? != frames
        {
            return Err(invalid("native capture changed before append"));
        }
        journal.write_all(&frames)?;
        sync(&journal)?;
        hook(CaptureStep::Appended, &mut journal, &frames)?;
        let (current, proof) = read(None)?;
        if current != configuration {
            return Err(invalid("native capture binding changed"));
        }

        if proof.binding() != self.binding {
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
        verify_capture_bytes(&replay, self.prepared.changeset_id)?;
        if read_private_in_store(&a.store, PENDING)? != intent {
            return Err(invalid("native capture intent changed"));
        }
        self.line
            .finish(self.prepared.changeset_id, &a.store, &configuration)?;
        retain_receipt(&a.store, self.request, &intent)?;
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

#[derive(Clone, Copy)]
enum CaptureStep {
    Staged,
    Appended,
}
fn receipt_name(request: RecordDigest) -> String {
    format!("dependency-capture-{}.json", request.to_hex())
}
pub(in crate::project_attachment) fn ensure_no_pending_capture(
    store: &PinnedWorkspaceRoot,
) -> io::Result<()> {
    absent(store, PENDING)
}

struct CaptureIntent {
    request: RecordDigest,
    authority: RecordDigest,
    configuration: RecordDigest,
    journal: (u64, u64),
    before_bytes: usize,
    before_digest: RecordDigest,
    frames: RecordDigest,
    operation: RecordDigest,
    head: Option<RecordDigest>,
}
impl CaptureIntent {
    fn encode(&self) -> String {
        Json::object([
            ("schema", Json::text("mesh.dependency-capture-intent/v1")),
            ("request", Json::text(self.request.to_hex())),
            ("authority", Json::text(self.authority.to_hex())),
            ("configuration", Json::text(self.configuration.to_hex())),
            (
                "journal_device",
                Json::text(format!("{:016x}", self.journal.0)),
            ),
            (
                "journal_inode",
                Json::text(format!("{:016x}", self.journal.1)),
            ),
            ("before_bytes", Json::Number(self.before_bytes as u64)),
            ("before_digest", Json::text(self.before_digest.to_hex())),
            ("frames", Json::text(self.frames.to_hex())),
            ("operation", Json::text(self.operation.to_hex())),
            (
                "capture_head",
                self.head.map_or(Json::Null, |h| Json::text(h.to_hex())),
            ),
        ])
        .encode()
    }
    fn parse(raw: &str) -> io::Result<Self> {
        let v = Json::parse(raw).map_err(error)?;
        let before_bytes =
            v.get("before_bytes")
                .and_then(Json::as_u64)
                .filter(|n| *n <= MAX_JOURNAL as u64)
                .ok_or_else(|| invalid("invalid capture boundary"))? as usize;
        let value = Self {
            request: digest(text(&v, "request")?)?,
            authority: digest(text(&v, "authority")?)?,
            configuration: digest(text(&v, "configuration")?)?,
            journal: (
                u64::from_str_radix(text(&v, "journal_device")?, 16).map_err(error)?,
                u64::from_str_radix(text(&v, "journal_inode")?, 16).map_err(error)?,
            ),
            before_bytes,
            before_digest: digest(text(&v, "before_digest")?)?,
            frames: digest(text(&v, "frames")?)?,
            operation: digest(text(&v, "operation")?)?,
            head: match v.get("capture_head") {
                Some(Json::Null) => None,
                Some(Json::Text(s)) => Some(digest(s)?),
                _ => return Err(invalid("missing capture predecessor")),
            },
        };
        if value.encode() != raw || value.request == RecordDigest::from_bytes([0; 32]) {
            return Err(invalid("noncanonical native capture intent"));
        }
        Ok(value)
    }
}

fn validate_frames(
    cas: &Cas<crate::root_authority::PinnedRootFs, mesh_cas::Blake3>,
    intent: &CaptureIntent,
) -> io::Result<Vec<u8>> {
    let frames = read_payload(cas, intent.frames, MAX_JOURNAL)?;
    let scan = mesh_store::scan_journal(&frames).map_err(error)?;
    if scan.tail().is_fragment() || scan.records().is_empty() {
        return Err(invalid("incomplete capture frames"));
    }
    let mut manifests = BTreeSet::new();
    for (n, record) in scan.records().iter().enumerate() {
        match record {
            mesh_store::StoredRecord::Manifest(m)
                if n + 1 < scan.records().len() && manifests.insert(m.id) => {}
            mesh_store::StoredRecord::Operation(op)
                if n + 1 == scan.records().len()
                    && op.id == intent.operation
                    && op.payload_digest == intent.operation
                    && op.parents == intent.head.into_iter().collect::<Vec<_>>() =>
            {
                let payload = read_payload(cas, op.payload_digest, 16 * 1024 * 1024)?;
                let signed =
                    crate::authenticated_changeset::AuthenticatedChangeSet::from_canonical_bytes(
                        &payload,
                    )
                    .map_err(error)?;
                if !signed.signed_by(PublicKey::from_bytes(*op.actor.as_bytes())) {
                    return Err(invalid("capture actor binding changed"));
                }
            }
            _ => {
                return Err(invalid(
                    "capture frames contain conflicting or non-authoring records",
                ))
            }
        }
    }
    Ok(frames)
}

// Only native capture recovery can ask the enrollment reader to validate a known prefix. Every
// ordinary reader still sees the whole journal and refuses its unfinished suffix.
pub(in crate::project_attachment) fn capture_prefix(
    cas: &Cas<crate::root_authority::PinnedRootFs, mesh_cas::Blake3>,
    raw: &str,
    journal: (u64, u64),
    bytes: &[u8],
    authority: RecordDigest,
    configuration: &str,
) -> io::Result<usize> {
    let intent = CaptureIntent::parse(raw)?;
    let frames = validate_frames(cas, &intent)?;
    if intent.journal != journal
        || intent.authority != authority
        || intent.configuration != hash(configuration.as_bytes())
        || intent.before_bytes > bytes.len()
        || intent.before_bytes.saturating_add(frames.len()) > MAX_JOURNAL
        || hash(&bytes[..intent.before_bytes]) != intent.before_digest
    {
        return Err(invalid("native capture prefix or installation changed"));
    }
    let suffix = &bytes[intent.before_bytes..];
    if suffix.len() < frames.len() {
        if !frames.starts_with(suffix) {
            return Err(invalid("foreign partial capture suffix"));
        }
        Ok(intent.before_bytes)
    } else {
        if !suffix.starts_with(&frames) {
            return Err(invalid("committed capture differs from its intent"));
        }
        Ok(bytes.len())
    }
}
fn verify_capture_bytes(history: &OpenWorkspace, operation: RecordDigest) -> io::Result<()> {
    let snapshot = history
        .historical_workspace_preview(operation)
        .map_err(error)?;
    for file in &snapshot.files {
        history
            .write_historical_workspace_file(file, &mut io::sink())
            .map_err(|_| invalid("native captured content is missing or changed"))?;
    }
    Ok(())
}
fn retain_receipt(
    store: &PinnedWorkspaceRoot,
    request: RecordDigest,
    intent: &str,
) -> io::Result<()> {
    let name = receipt_name(request);
    match read_private_in_store(store, &name) {
        Ok(existing) if existing == intent => store.filesystem().sync_file(Path::new(&name))?,
        Ok(_) => {
            return Err(invalid(
                "capture request has conflicting completed evidence",
            ))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => store.filesystem().write_new_file(
            Path::new(&name),
            intent.as_bytes(),
            fs::Permissions::from_mode(0o600),
        )?,
        Err(e) => return Err(e),
    }
    store.sync()
}
impl ProvisionedAttachment {
    /// Recover only this exact previously staged native capture request. No new snapshot or
    /// signature is selected. Completed requests return their historical operation without
    /// rewinding the current capture line; unknown/conflicting evidence is retained and refused.
    pub fn recover_dependency_capture(
        &self,
        request: RecordDigest,
    ) -> io::Result<SavedAttachmentVersion> {
        self.recover_capture_with_sync(request, |file| file.sync_all())
    }
    fn recover_capture_with_sync(
        &self,
        request: RecordDigest,
        sync: impl FnMut(&fs::File) -> io::Result<()>,
    ) -> io::Result<SavedAttachmentVersion> {
        self.with_capture_custody(None, |read| self.recover_capture_held(request, read, sync))
    }
    fn recover_capture_held(
        &self,
        request: RecordDigest,
        read: &CaptureRead<'_>,
        mut sync: impl FnMut(&fs::File) -> io::Result<()>,
    ) -> io::Result<SavedAttachmentVersion> {
        if request == RecordDigest::from_bytes([0; 32]) {
            return Err(invalid("missing capture recovery request"));
        }

        self.check_dependency_registration()?;
        crate::project_attachment::detachment::ensure_attached(&self.store)?;
        absent(&self.store, dependency_decision::PENDING)?;
        let (raw, pending) = match read_private_in_store(&self.store, PENDING) {
            Ok(raw) => (raw, true),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (
                read_private_in_store(&self.store, &receipt_name(request))?,
                false,
            ),
            Err(e) => return Err(e),
        };
        let intent = CaptureIntent::parse(&raw)?;
        if intent.request != request {
            return Err(invalid("another capture request owns this recovery"));
        }
        let (configuration, proof) = read(Some(&raw))?;
        let cas = Cas::with_filesystem(self.metadata_path(), self.store.filesystem().read_only())
            .map_err(error)?;
        let frames = validate_frames(&cas, &intent)?;
        let mut journal = self
            .store
            .open_existing_record_file(Path::new(crate::RECORD_FILE_NAME))?;
        journal.rewind()?;
        let mut bytes = Vec::new();
        (&mut journal)
            .take((MAX_JOURNAL + 1) as u64)
            .read_to_end(&mut bytes)?;
        proof.verify(&self.store, &journal, &bytes)?;
        capture_prefix(
            &cas,
            &raw,
            (journal.metadata()?.dev(), journal.metadata()?.ino()),
            &bytes,
            proof.binding().authority,
            &configuration,
        )?;
        let written = bytes.len() - intent.before_bytes;
        if !pending && written < frames.len() {
            return Err(invalid(
                "completed capture receipt has an unfinished journal",
            ));
        }
        let completed_name = receipt_name(request);
        if read_private_in_store(&self.store, if pending { PENDING } else { &completed_name })?
            != raw
        {
            return Err(invalid("capture recovery intent changed before append"));
        }
        if written < frames.len() {
            journal.write_all(&frames[written..])?;
        }
        sync(&journal)?;
        let (after, after_proof) = read(None)?;
        if after != configuration || after_proof.binding() != proof.binding() {
            return Err(invalid("capture recovery binding changed"));
        }
        let history = OpenWorkspace::open_attachment_read_history(
            self.metadata_path(),
            self.store.clone(),
            &crate::TrustedReviewers::default(),
            Some(&after_proof),
        )
        .map_err(error)?;
        verify_history_binding(&history, &configuration)?;
        verify_capture_bytes(&history, intent.operation)?;
        if history
            .linear_history(Some(intent.operation))
            .map_err(error)?
            .iter()
            .rev()
            .nth(1)
            .copied()
            != intent.head
        {
            return Err(invalid("capture operation has a different predecessor"));
        }
        self.check_dependency_registration()?;
        if pending {
            if read_private_in_store(&self.store, PENDING)? != raw {
                return Err(invalid("capture recovery intent changed after append"));
            }
            let line = CaptureLine::load(&self.store, &history, &configuration)?;
            if line.head == Some(intent.operation) {
                line.persist(&self.store, &configuration)?;
            } else if line.head == intent.head {
                line.begin(intent.operation, &self.store, &configuration)?;
                line.finish(intent.operation, &self.store, &configuration)?;
            } else {
                return Err(invalid(
                    "capture recovery would replace a different capture line",
                ));
            }
            retain_receipt(&self.store, request, &raw)?;
            self.store.filesystem().remove_file(Path::new(PENDING))?;
            self.store.sync()?;
        }
        Ok(SavedAttachmentVersion {
            operation: intent.operation,
        })
    }
}

#[path = "dependency_capture_retention.rs"]
mod retention;
pub use retention::NativeCaptureRetention;

#[cfg(test)]
pub(in crate::project_attachment) use consumed::assert_consumed_capture;
