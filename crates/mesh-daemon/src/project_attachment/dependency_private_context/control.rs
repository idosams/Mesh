//! Trusted native input decisions and saved reviews through complete publication history.
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
mod snapshot;
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

#[derive(Clone, Copy)]
enum ControlRequest {
    Input {
        version: RecordDigest,
        decision: NativeSavedInputDecision,
        previous: Option<RecordDigest>,
    },
    Snapshot {
        operation: RecordDigest,
    },
    Review {
        snapshot: RecordDigest,
        opener: RecordDigest,
    },
}
impl ControlRequest {
    fn kind(self) -> DependencyKind {
        match self {
            Self::Input { .. } => DependencyKind::Eligibility,
            Self::Review { .. } | Self::Snapshot { .. } => DependencyKind::ReviewSnapshot,
        }
    }
    fn previous(self) -> Option<RecordDigest> {
        match self {
            Self::Input { previous, .. } => previous,
            Self::Review { .. } | Self::Snapshot { .. } => None,
        }
    }
    fn valid(self) -> bool {
        match self {
            Self::Input {
                version, previous, ..
            } => version != ZERO && previous != Some(ZERO),
            Self::Review { snapshot, opener } => snapshot != ZERO && opener != ZERO,
            Self::Snapshot { operation } => operation != ZERO,
        }
    }
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
    graph: Option<snapshot::RetainedGraph>,
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
    kind: DependencyKind,
    request: RecordDigest,
    identity: (u64, u64),
    before: &[u8],
    payload: RecordDigest,
) -> String {
    super::super::dependency_decision::transaction_intent(kind, request, identity, before, payload)
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
    fn select_native_control(
        &self,
        storage: &AttachmentStorage,
        work: &ProvisionedAttachment,
        guard: &WorkspaceInitializationGuard,
        command: ControlRequest,
        request: RecordDigest,
        trusted: &TrustedReviewers,
    ) -> io::Result<Selection> {
        self.with_replayed_history(storage, work, trusted, guard, |history, owner| {
            let mut retained_graph = None;
            let selected = match command {
                ControlRequest::Input {
                    version,
                    decision,
                    previous,
                } => {
                    let binding = self.binding(storage, work, guard)?;
                    self.graph(storage, work, version, guard)?;
                    let version =
                        SavedAttachmentVersion::from_verified_private_history(history, version)?;
                    let decision = match decision {
                        NativeSavedInputDecision::Eligible => SavedInputDecision::Eligible,
                        NativeSavedInputDecision::Rejected => SavedInputDecision::Rejected,
                        NativeSavedInputDecision::Replaced(operation) => {
                            self.graph(storage, work, operation, guard)?;
                            SavedInputDecision::Replaced(
                                SavedAttachmentVersion::from_verified_private_history(
                                    history, operation,
                                )?,
                            )
                        }
                    };
                    NativeControlInput {
                        kind: DependencyKind::Eligibility,
                        revision_field: "revision",
                        body: super::super::dependency_decision::body(
                            binding.work(),
                            binding.installation(),
                            version,
                            decision,
                            request,
                            0,
                            previous.unwrap_or(ZERO),
                        ),
                        prior: owner.policy().native_decision(
                            binding.work(),
                            binding.installation(),
                            version.operation(),
                        ),
                    }
                }
                ControlRequest::Snapshot { operation } => {
                    let (input, graph) =
                        snapshot::select(self, storage, work, guard, owner, operation, request)?;
                    retained_graph = Some(graph);
                    input
                }
                ControlRequest::Review { snapshot, opener } => {
                    let evidence = owner
                        .policy()
                        .review_evidence(snapshot)
                        .ok_or_else(|| invalid("native review snapshot is unavailable"))?;
                    let binding = self.binding(storage, work, guard)?;
                    let output = evidence.output();
                    if (output.0, output.1) != (binding.work(), binding.installation()) {
                        return Err(invalid("native review snapshot belongs to another work"));
                    }
                    let graph = self.graph(storage, work, output.2, guard)?;
                    if owner.policy().review_graph(snapshot) != Some(graph.digest()) {
                        return Err(invalid(
                            "native review snapshot differs from complete graph",
                        ));
                    }
                    let (canonical, bundle) = if owner.policy().native_request(request).is_some() {
                        let prior = owner
                            .policy()
                            .review_binding_body(request)
                            .ok_or_else(|| invalid("request is not a saved review"))?;
                        let canonical = mesh_approval::HeadId::from_bytes(
                            *digest(super::super::dependency_transaction::text(
                                &prior,
                                "canonical",
                            )?)?
                            .as_bytes(),
                        );
                        (
                            canonical,
                            history
                                .review_bundle_at(&evidence, canonical)
                                .map_err(error)?,
                        )
                    } else {
                        history.current_review_bundle(&evidence).map_err(error)?
                    };
                    NativeControlInput {
                        kind: DependencyKind::ReviewSnapshot,
                        revision_field: "revision",
                        prior: None,
                        body: Json::object([
                            ("request", Json::text(request.to_hex())),
                            ("revision", Json::Number(1)),
                            ("snapshot", Json::text(snapshot.to_hex())),
                            (
                                "output",
                                Json::Array(vec![
                                    Json::Array(vec![
                                        Json::text(output.0.to_hex()),
                                        Json::text(output.1.to_hex()),
                                    ]),
                                    Json::text(output.2.to_hex()),
                                ]),
                            ),
                            (
                                "canonical",
                                Json::text(
                                    RecordDigest::from_bytes(*canonical.as_bytes()).to_hex(),
                                ),
                            ),
                            ("bundle", Json::text(bundle.to_hex())),
                            ("opener", Json::text(opener.to_hex())),
                        ]),
                    }
                }
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
                if record.kind != selected.kind
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
                    graph: retained_graph,
                });
            }
            if selected.prior.map(|(_, p)| p) != command.previous() {
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
                (
                    "schema",
                    Json::text(match command {
                        ControlRequest::Input { .. } => "mesh.dependency-policy/v1",
                        ControlRequest::Snapshot { .. } => "mesh.dependency-policy/v3",
                        ControlRequest::Review { .. } => "mesh.dependency-policy/v4",
                    }),
                ),
                ("authority", Json::text(owner.binding().authority.to_hex())),
                ("revision", Json::Number(ordinal)),
                ("previous", Json::text(previous.to_hex())),
                ("kind", Json::Number(selected.kind.code().into())),
                ("body", selected.body_at(revision)?),
            ])
            .encode()
            .into_bytes();
            let record = DependencyRecord {
                authority: owner.binding().authority,
                revision: ordinal,
                previous,
                payload: hash(&payload),
                kind: selected.kind,
            };
            let mut projected = owner.policy().clone();
            projected.apply(record, &payload).map_err(error)?;
            Ok(Selection {
                record,
                payload,
                revision,
                existing: false,
                graph: retained_graph,
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
        hook: impl FnMut(Step, &mut File, &[u8]) -> io::Result<()>,
        sync: impl FnMut(&File) -> io::Result<()>,
    ) -> io::Result<NativeInputDecision> {
        self.write_native_control(
            work_id,
            ControlRequest::Input {
                version,
                decision,
                previous: expected_previous,
            },
            request,
            trusted,
            hook,
            sync,
        )
        .map(|selected| selected.decision())
    }

    /// Freeze exact saved content and current eligible decisions through verified publication history.
    /// A retained snapshot is historical evidence, never approval or permission to consume inputs.
    pub fn save_native_review_snapshot(
        &self,
        work_id: &str,
        operation: RecordDigest,
        request: RecordDigest,
        trusted: &TrustedReviewers,
    ) -> io::Result<super::super::NativeDependencyReviewSnapshot> {
        self.save_native_review_snapshot_with_io(
            work_id,
            operation,
            request,
            trusted,
            |_, _, _| Ok(()),
            |file| file.sync_all(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::project_attachment) fn save_native_review_snapshot_with_io(
        &self,
        work_id: &str,
        operation: RecordDigest,
        request: RecordDigest,
        trusted: &TrustedReviewers,
        hook: impl FnMut(Step, &mut File, &[u8]) -> io::Result<()>,
        sync: impl FnMut(&File) -> io::Result<()>,
    ) -> io::Result<super::super::NativeDependencyReviewSnapshot> {
        let selected = self.write_native_control(
            work_id,
            ControlRequest::Snapshot { operation },
            request,
            trusted,
            hook,
            sync,
        )?;
        let value =
            Json::parse(std::str::from_utf8(&selected.payload).map_err(error)?).map_err(error)?;
        let body = value
            .get("body")
            .ok_or_else(|| invalid("native snapshot body missing"))?;
        let read_id = |key| digest(super::super::dependency_transaction::text(body, key)?);
        Ok(
            super::super::NativeDependencyReviewSnapshot::from_verified_record(
                selected.record.payload,
                read_id("graph")?,
                read_id("validation")?,
            ),
        )
    }

    /// Save an immutable review against fully verified native publication history.
    /// This records historical evidence, never approval or ordinary workspace admission.
    pub fn save_native_review(
        &self,
        work_id: &str,
        snapshot: RecordDigest,
        opener: RecordDigest,
        request: RecordDigest,
        trusted: &TrustedReviewers,
    ) -> io::Result<super::super::NativeSavedDependencyReview> {
        self.save_native_review_with_io(
            work_id,
            snapshot,
            opener,
            request,
            trusted,
            |_, _, _| Ok(()),
            |file| file.sync_all(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::project_attachment) fn save_native_review_with_io(
        &self,
        work_id: &str,
        snapshot: RecordDigest,
        opener: RecordDigest,
        request: RecordDigest,
        trusted: &TrustedReviewers,
        hook: impl FnMut(Step, &mut File, &[u8]) -> io::Result<()>,
        sync: impl FnMut(&File) -> io::Result<()>,
    ) -> io::Result<super::super::NativeSavedDependencyReview> {
        let selected = self.write_native_control(
            work_id,
            ControlRequest::Review { snapshot, opener },
            request,
            trusted,
            hook,
            sync,
        )?;
        let value =
            Json::parse(std::str::from_utf8(&selected.payload).map_err(error)?).map_err(error)?;
        let body = value
            .get("body")
            .ok_or_else(|| invalid("native saved review body missing"))?;
        let bundle = digest(super::super::dependency_transaction::text(body, "bundle")?)?;
        Ok(
            super::super::NativeSavedDependencyReview::from_verified_record(
                selected.record.payload,
                bundle,
            ),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn write_native_control(
        &self,
        work_id: &str,
        command: ControlRequest,
        request: RecordDigest,
        trusted: &TrustedReviewers,
        mut hook: impl FnMut(Step, &mut File, &[u8]) -> io::Result<()>,
        mut sync: impl FnMut(&File) -> io::Result<()>,
    ) -> io::Result<Selection> {
        if request == ZERO || !command.valid() {
            return Err(invalid("invalid native control request"));
        }
        let owner = self.reopen(&self.candidate_owning_root(work_id)?)?;
        let work = self.reopen(work_id)?;
        let owner_selection = self.prepare_dependency_work(&owner, &owner)?;
        let hints = self.catalog_discovery_hints(&owner)?;
        let (discovery, recovery) = {
            let guard =
                crate::workspace_custody::lock_workspace_initialization_set(&owner_selection.roots)
                    .map_err(error)?;
            let recovery = recovery::VerifiedControlPrefix::read(&owner, request, command.kind())?;
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
        let selected =
            context.select_native_control(self, &work, &guard, command, request, trusted)?;
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
                if *raw
                    != intent(
                        command.kind(),
                        request,
                        identity,
                        &before[..n],
                        selected.record.payload,
                    )
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
            if PrivateContext::resolve(self, &owner, &works, &guard)?
                .select_native_control(self, &work, &guard, command, request, trusted)?
                != selected
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
            return Ok(selected);
        }
        if before.len().saturating_add(frame.len()) > MAX_JOURNAL {
            return Err(invalid("native decision append exceeds bound"));
        }
        let expected = intent(
            command.kind(),
            request,
            identity,
            &before,
            selected.record.payload,
        );
        if pending.as_ref().is_some_and(|raw| raw != &expected) {
            return Err(invalid("another native decision requires recovery"));
        }
        let cas =
            Cas::<_, Blake3>::with_filesystem(owner.metadata_path(), owner.store.filesystem())
                .map_err(error)?;
        if let Some(graph) = &selected.graph {
            // Recovery must not silently recreate graph evidence that was lost after staging.
            if pending.is_none() {
                graph.promote(&owner)?;
            }
            graph.verify(&owner)?;
        }
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
            || after.select_native_control(self, &work, &guard, command, request, trusted)?
                != selected
            || read_private_in_store(&owner.store, PENDING)? != expected
            || super::super::dependency_transaction::read_payload(
                &cas,
                selected.record.payload,
                65536,
            )? != selected.payload
        {
            return Err(invalid("native decision evidence changed before append"));
        }
        if let Some(graph) = &selected.graph {
            graph.verify(&owner)?;
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
        // Sole durable native-control commit point. This never constitutes publication approval.
        sync(&journal)?;
        hook(Step::Appended, &mut journal, &frame)?;
        let replay = PrivateContext::resolve(self, &owner, &works, &guard)?;
        let completed =
            replay.select_native_control(self, &work, &guard, command, request, trusted)?;
        if !completed.existing
            || completed.record != selected.record
            || completed.revision != selected.revision
            || completed.payload != selected.payload
            || completed.graph != selected.graph
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
        Ok(selected)
    }
}
