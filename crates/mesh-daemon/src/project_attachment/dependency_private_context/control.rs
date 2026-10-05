//! Trusted native input decisions through complete private publication history.
use super::*;
use crate::{
    project_attachment::{NativeInputDecision, SavedAttachmentVersion, SavedInputDecision},
    TrustedReviewers,
};
use mesh_cas::{Blake3, Cas, DurableFs as _};
use mesh_store::{frame_record, DependencyKind, DependencyRecord, StoredRecord};
use std::{
    fs::File,
    io::{Read as _, Seek as _, Write as _},
    os::unix::fs::{MetadataExt as _, PermissionsExt as _},
    path::Path,
};
pub(in crate::project_attachment) mod recovery;
use super::super::dependency_decision::{NativeControlInput, PENDING};
const MAX_JOURNAL: usize = 64 * 1024 * 1024;
const ZERO: RecordDigest = RecordDigest::from_bytes([0; 32]);
/// Native-host intent naming an exact durable operation, never an access or approval capability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeSavedInputDecision {
    /// Revalidate this input for future dependent publication; old reviews remain historical.
    Eligible,
    /// Refuse future dependent publication without rewriting accepted main.
    Rejected,
    /// Replace it with another saved operation from the same verified native work.
    Replaced(RecordDigest),
}

fn ensure_control_available(
    owner: &ProvisionedAttachment,
    work: &ProvisionedAttachment,
) -> io::Result<()> {
    super::super::detachment::ensure_attached(&owner.store)?;
    super::super::detachment::ensure_attached(&work.store)?;
    match read_private_in_store(&owner.store, super::publication::PENDING) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
        Ok(_) => {
            return Err(invalid(
                "pending publication must recover before native input control",
            ))
        }
    }

    super::super::history::dependency_capture::ensure_no_pending_capture(&owner.store)?;
    Ok(())
}
#[derive(PartialEq, Eq)]
struct Selection {
    record: DependencyRecord,
    payload: Vec<u8>,
    revision: u64,
    existing: bool,
}
impl Selection {
    fn decision(&self) -> NativeInputDecision {
        NativeInputDecision::from_verified_record(self.record.payload, self.revision)
    }
}
#[derive(Clone, Copy)]
pub(in crate::project_attachment) enum Step {
    Staged,
    Appended,
}
fn intent(
    request: RecordDigest,
    identity: (u64, u64),
    before: &[u8],
    payload: RecordDigest,
) -> String {
    super::super::dependency_decision::transaction_intent(
        DependencyKind::Eligibility,
        request,
        identity,
        before,
        payload,
    )
}
fn journal_bytes(file: &mut File) -> io::Result<Vec<u8>> {
    file.rewind()?;
    let mut bytes = Vec::new();
    (&mut *file)
        .take((MAX_JOURNAL + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_JOURNAL {
        return Err(invalid("native decision history exceeds bound"));
    }
    Ok(bytes)
}
impl PrivateContext<'_> {
    #[allow(clippy::too_many_arguments)]
    fn select_native_decision(
        &self,
        storage: &AttachmentStorage,
        work: &ProvisionedAttachment,
        guard: &WorkspaceInitializationGuard,
        version: RecordDigest,
        decision: NativeSavedInputDecision,
        expected_previous: Option<RecordDigest>,
        request: RecordDigest,
        trusted: &TrustedReviewers,
    ) -> io::Result<Selection> {
        self.with_replayed_history(storage, work, trusted, guard, |history, owner| {
            let binding = self.binding(storage, work, guard)?;
            self.graph(storage, work, version, guard)?;
            let version = SavedAttachmentVersion::from_verified_private_history(history, version)?;
            let decision = match decision {
                NativeSavedInputDecision::Eligible => SavedInputDecision::Eligible,
                NativeSavedInputDecision::Rejected => SavedInputDecision::Rejected,
                NativeSavedInputDecision::Replaced(operation) => {
                    self.graph(storage, work, operation, guard)?;
                    SavedInputDecision::Replaced(
                        SavedAttachmentVersion::from_verified_private_history(history, operation)?,
                    )
                }
            };
            let selected = NativeControlInput {
                kind: DependencyKind::Eligibility,
                revision_field: "revision",
                body: super::super::dependency_decision::body(
                    binding.work(),
                    binding.installation(),
                    version,
                    decision,
                    request,
                    0,
                    expected_previous.unwrap_or(ZERO),
                ),
                prior: owner.policy().native_decision(
                    binding.work(),
                    binding.installation(),
                    version.operation(),
                ),
            };
            let cas = Cas::<_, Blake3>::with_filesystem(
                self.owner.metadata_path(),
                self.owner.store.filesystem().read_only(),
            )
            .map_err(error)?;
            if let Some(record) = owner.policy().native_request(request) {
                let payload = super::super::dependency_transaction::read_payload(
                    &cas,
                    record.payload,
                    65536,
                )?;
                let value =
                    Json::parse(std::str::from_utf8(&payload).map_err(error)?).map_err(error)?;
                let revision = value
                    .get("body")
                    .and_then(|v| v.get("revision"))
                    .and_then(Json::as_u64)
                    .ok_or_else(|| invalid("request is not an input decision"))?;
                if record.kind != DependencyKind::Eligibility
                    || value.get("body") != Some(&selected.body_at(revision)?)
                {
                    return Err(invalid(
                        "native decision retry differs from original intent",
                    ));
                }
                return Ok(Selection {
                    record,
                    payload,
                    revision,
                    existing: true,
                });
            }
            if selected.prior.map(|(_, p)| p) != expected_previous {
                return Err(invalid("saved-input decision is stale"));
            }
            let revision = selected
                .prior
                .map_or(Some(1), |(n, _)| n.checked_add(1))
                .ok_or_else(|| invalid("decision revision exhausted"))?;
            let (ordinal, previous) = owner
                .policy()
                .native_head()
                .ok_or_else(|| invalid("native decision enrollment missing"))?;
            let ordinal = ordinal
                .checked_add(1)
                .ok_or_else(|| invalid("native decision history exhausted"))?;
            let payload = Json::object([
                ("schema", Json::text("mesh.dependency-policy/v1")),
                ("authority", Json::text(owner.binding().authority.to_hex())),
                ("revision", Json::Number(ordinal)),
                ("previous", Json::text(previous.to_hex())),
                (
                    "kind",
                    Json::Number(DependencyKind::Eligibility.code().into()),
                ),
                ("body", selected.body_at(revision)?),
            ])
            .encode()
            .into_bytes();
            let record = DependencyRecord {
                authority: owner.binding().authority,
                revision: ordinal,
                previous,
                payload: hash(&payload),
                kind: DependencyKind::Eligibility,
            };
            let mut projected = owner.policy().clone();
            projected.apply(record, &payload).map_err(error)?;
            Ok(Selection {
                record,
                payload,
                revision,
                existing: false,
            })
        })
    }
}
impl AttachmentStorage {
    /// Record an exact native-host eligibility decision after verifying complete accepted history.
    /// This grants no execution, input access or publication authority and has no runtime endpoint.
    /// No caller should wait for a person or provider inside this call. This development API
    /// is not exposed to agents or runtime controls. Exact pending decisions retain their original prefix.
    pub fn decide_native_saved_input(
        &self,
        work_id: &str,
        version: RecordDigest,
        decision: NativeSavedInputDecision,
        expected_previous: Option<RecordDigest>,
        request: RecordDigest,
        trusted: &TrustedReviewers,
    ) -> io::Result<NativeInputDecision> {
        self.decide_native_saved_input_with_io(
            work_id,
            version,
            decision,
            expected_previous,
            request,
            trusted,
            |_, _, _| Ok(()),
            |f| f.sync_all(),
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub(in crate::project_attachment) fn decide_native_saved_input_with_io(
        &self,
        work_id: &str,
        version: RecordDigest,
        decision: NativeSavedInputDecision,
        expected_previous: Option<RecordDigest>,
        request: RecordDigest,
        trusted: &TrustedReviewers,
        mut hook: impl FnMut(Step, &mut File, &[u8]) -> io::Result<()>,
        mut sync: impl FnMut(&File) -> io::Result<()>,
    ) -> io::Result<NativeInputDecision> {
        if request == ZERO || version == ZERO || expected_previous == Some(ZERO) {
            return Err(invalid("invalid native decision request"));
        }
        let owner = self.reopen(&self.candidate_owning_root(work_id)?)?;
        let work = self.reopen(work_id)?;
        let owner_selection = self.prepare_dependency_work(&owner, &owner)?;
        let hints = self.catalog_discovery_hints(&owner)?;
        let (discovery, recovery) = {
            let guard =
                crate::workspace_custody::lock_workspace_initialization_set(&owner_selection.roots)
                    .map_err(error)?;
            let recovery = recovery::VerifiedControlPrefix::read(&owner, request)?;
            let discovery = self.private_discovery_with_recovery(
                &owner,
                work_id,
                &guard,
                hints,
                recovery.as_ref().map(OwnerRecovery::Control),
            )?;
            (discovery, recovery)
        };
        let works = discovery
            .works
            .keys()
            .map(|id| self.reopen(id))
            .collect::<io::Result<Vec<_>>>()?;
        let available = works.iter().collect::<Vec<_>>();
        let prepared = self.prepare_dependency_graph(&owner, &work, ZERO, &available)?;
        let guard = crate::workspace_custody::lock_workspace_initialization_set(&prepared.roots)
            .map_err(error)?;
        let rediscover = || {
            self.private_discovery_with_recovery(
                &owner,
                work_id,
                &guard,
                self.catalog_discovery_hints(&owner)?,
                recovery.as_ref().map(OwnerRecovery::Control),
            )
        };
        if rediscover()? != discovery {
            return Err(invalid("native decision roots changed"));
        }
        ensure_control_available(&owner, &work)?;
        let context = PrivateContext::resolve_with_recovery(
            self,
            &owner,
            &works,
            &guard,
            recovery.as_ref().map(OwnerRecovery::Control),
        )?;
        let selected = context.select_native_decision(
            self,
            &work,
            &guard,
            version,
            decision,
            expected_previous,
            request,
            trusted,
        )?;
        let mut journal = owner
            .store
            .open_existing_record_file(Path::new(crate::RECORD_FILE_NAME))?;
        let observed_before = journal_bytes(&mut journal)?;
        context
            .read(&owner, &guard)?
            .proof
            .verify(&owner.store, &journal, &observed_before)?;
        let before = if let Some(proof) = &recovery {
            let (length, record, payload) =
                proof.verify(&owner.store, &journal, &observed_before)?;
            if selected.existing || selected.record != record || selected.payload != payload {
                return Err(invalid("native decision recovery selection changed"));
            }
            observed_before[..length].to_vec()
        } else {
            observed_before.clone()
        };
        let metadata = journal.metadata()?;
        let identity = (metadata.dev(), metadata.ino());
        let pending = match read_private_in_store(&owner.store, PENDING) {
            Ok(p) => Some(p),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        let frame = frame_record(&StoredRecord::Dependency(selected.record));
        if selected.existing {
            if let Some(raw) = &pending {
                let value = Json::parse(raw).map_err(error)?;
                let n = value
                    .get("journal_bytes")
                    .and_then(Json::as_u64)
                    .filter(|n| *n <= before.len() as u64)
                    .ok_or_else(|| invalid("invalid completed native decision intent"))?
                    as usize;
                if *raw != intent(request, identity, &before[..n], selected.record.payload)
                    || !before[n..].starts_with(&frame)
                {
                    return Err(invalid(
                        "native decision retry has foreign pending evidence",
                    ));
                }
            }
            sync(&journal)?;
            if rediscover()? != discovery {
                return Err(invalid("native decision retry roots changed"));
            }
            if PrivateContext::resolve(self, &owner, &works, &guard)?.select_native_decision(
                self,
                &work,
                &guard,
                version,
                decision,
                expected_previous,
                request,
                trusted,
            )? != selected
            {
                return Err(invalid("native decision retry changed"));
            }
            if let Some(raw) = pending {
                if read_private_in_store(&owner.store, PENDING)? != raw {
                    return Err(invalid("native decision intent changed"));
                }
                owner.store.filesystem().remove_file(Path::new(PENDING))?;
            }
            owner.store.sync()?;
            guard.ensure_current().map_err(error)?;
            return Ok(selected.decision());
        }
        if before.len().saturating_add(frame.len()) > MAX_JOURNAL {
            return Err(invalid("native decision append exceeds bound"));
        }
        let expected = intent(request, identity, &before, selected.record.payload);
        if pending.as_ref().is_some_and(|raw| raw != &expected) {
            return Err(invalid("another native decision requires recovery"));
        }
        let cas =
            Cas::<_, Blake3>::with_filesystem(owner.metadata_path(), owner.store.filesystem())
                .map_err(error)?;
        cas.promote(selected.payload.clone()).map_err(error)?;
        if pending.is_none() {
            owner.store.filesystem().write_new_file(
                Path::new(PENDING),
                expected.as_bytes(),
                std::fs::Permissions::from_mode(0o600),
            )?;
        }
        owner.store.filesystem().sync_file(Path::new(PENDING))?;
        owner.store.sync()?;
        hook(Step::Staged, &mut journal, &frame)?;
        ensure_control_available(&owner, &work)?;
        let after = PrivateContext::resolve_with_recovery(
            self,
            &owner,
            &works,
            &guard,
            recovery.as_ref().map(OwnerRecovery::Control),
        )?;
        if rediscover()? != discovery
            || after.select_native_decision(
                self,
                &work,
                &guard,
                version,
                decision,
                expected_previous,
                request,
                trusted,
            )? != selected
            || read_private_in_store(&owner.store, PENDING)? != expected
            || super::super::dependency_transaction::read_payload(
                &cas,
                selected.record.payload,
                65536,
            )? != selected.payload
        {
            return Err(invalid("native decision evidence changed before append"));
        }
        let observed = journal_bytes(&mut journal)?;
        context
            .read(&owner, &guard)?
            .proof
            .verify(&owner.store, &journal, &observed)?;
        if observed != observed_before {
            return Err(invalid("native decision journal changed before append"));
        }
        guard.ensure_current().map_err(error)?;
        let written = observed_before.len() - before.len();
        if written >= frame.len() || !frame.starts_with(&observed_before[before.len()..]) {
            return Err(invalid(
                "native decision recovery bytes differ from exact frame",
            ));
        }
        journal.write_all(&frame[written..])?;
        // Sole durable input-decision commit point. This never constitutes publication approval.
        sync(&journal)?;
        hook(Step::Appended, &mut journal, &frame)?;
        let replay = PrivateContext::resolve(self, &owner, &works, &guard)?;
        let completed = replay.select_native_decision(
            self,
            &work,
            &guard,
            version,
            decision,
            expected_previous,
            request,
            trusted,
        )?;
        if !completed.existing
            || completed.record != selected.record
            || completed.revision != selected.revision
            || completed.payload != selected.payload
        {
            return Err(invalid("native decision failed exact durable replay"));
        }
        ensure_control_available(&owner, &work)?;
        if read_private_in_store(&owner.store, PENDING)? != expected {
            return Err(invalid("native decision intent changed after append"));
        }
        owner.store.filesystem().remove_file(Path::new(PENDING))?;
        owner.store.sync()?;
        guard.ensure_current().map_err(error)?;
        Ok(selected.decision())
    }
}
