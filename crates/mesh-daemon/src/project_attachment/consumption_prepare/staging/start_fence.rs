//! Required start publication. No files are installed and no consumption is acknowledged here.
use super::*;
use crate::project_attachment::dependency_decision::transaction_intent;
use crate::root_authority::PinnedRootFs;
use mesh_cas::{Blake3, Cas};
use mesh_store::{frame_record, scan_journal, DependencyKind, DependencyRecord, StoredRecord};
use std::io::{Seek as _, Write as _};
const PENDING: &str = super::super::START_PENDING;
const MAX_JOURNAL: usize = 80 * 1024 * 1024;
fn j(value: RecordDigest) -> Json {
    Json::text(value.to_hex())
}
fn work(value: &NativeDependencyWorkBinding) -> Json {
    Json::Array(vec![j(value.work()), j(value.installation())])
}
impl PreparedNativeConsumedStart {
    pub(super) fn start_body(
        &self,
        owner: crate::dependency_policy::NativeDependencyBinding,
        source: &NativeDependencyWorkBinding,
        destination: &NativeDependencyWorkBinding,
        descriptor: RecordDigest,
    ) -> Json {
        Json::object([
            ("request", j(self.request)),
            (
                "owner",
                Json::Array(vec![
                    j(owner.authority),
                    j(owner.project),
                    j(owner.installation),
                ]),
            ),
            ("destination", work(destination)),
            (
                "source",
                Json::Array(vec![work(source), j(self.version.operation())]),
            ),
            ("grant", j(self.grant)),
            (
                "bindings",
                Json::Array(vec![j(source.correlation), j(destination.correlation)]),
            ),
            (
                "configuration",
                j(hash(self.basis.configuration.as_bytes())),
            ),
            ("prospective", j(hash(self.basis.prospective.as_bytes()))),
            ("closure", j(self.basis.graph)),
            ("operation", j(self.operation())),
            ("staged", j(descriptor)),
        ])
    }

    /// Durably fence this exact staged starting version before any destination installation.
    /// The returned record is not consumption acknowledgement, a saved version or run permission.
    /// Fresh-process recovery can reconstruct this candidate; this method installs no files.
    pub fn fence_consumption_start(
        &self,
        storage: &AttachmentStorage,
        staged: &StagedNativeConsumedStart,
    ) -> io::Result<RecordDigest> {
        self.fence_with_io(storage, staged, |_, _, _| Ok(()), |file| file.sync_all())
    }
    pub(super) fn fence_with_io(
        &self,
        storage: &AttachmentStorage,
        staged: &StagedNativeConsumedStart,
        hook: impl FnMut(&str, &mut fs::File, &[u8]) -> io::Result<()>,
        sync: impl FnMut(&fs::File) -> io::Result<()>,
    ) -> io::Result<RecordDigest> {
        self.fence_and_install_with_io(storage, staged, false, hook, sync)
    }
    pub(super) fn fence_and_install_with_io(
        &self,
        storage: &AttachmentStorage,
        staged: &StagedNativeConsumedStart,
        install: bool,
        hook: impl FnMut(&str, &mut fs::File, &[u8]) -> io::Result<()>,
        sync: impl FnMut(&fs::File) -> io::Result<()>,
    ) -> io::Result<RecordDigest> {
        self.commit_phase_with_io(storage, staged, install, CommitPhase::Start, hook, sync)
    }
    pub(super) fn commit_phase_with_io(
        &self,
        storage: &AttachmentStorage,
        staged: &StagedNativeConsumedStart,
        install: bool,
        phase: CommitPhase,
        mut hook: impl FnMut(&str, &mut fs::File, &[u8]) -> io::Result<()>,
        mut sync: impl FnMut(&fs::File) -> io::Result<()>,
    ) -> io::Result<RecordDigest> {
        if phase != CommitPhase::Complete {
            absent(
                &self.destination,
                crate::project_attachment::consumption_complete::PENDING,
            )?;
        }
        if phase == CommitPhase::Start {
            absent(
                &self.destination,
                crate::project_attachment::consumption_history::PENDING,
            )?;
        }
        let available = self.available.iter().collect::<Vec<_>>();
        let graph = storage.prepare_dependency_graph(
            &self.owner,
            &self.source,
            self.version.operation(),
            &available,
        )?;
        let grant = storage.prepare_input_grant(
            &self.owner,
            NativeGrantInspection {
                source: &self.source,
                version: self.version,
                destination: &self.destination,
                grant: self.grant,
            },
        )?;
        let source = storage.prepare_dependency_work(&self.owner, &self.source)?;
        let destination = storage.prepare_dependency_work(&self.owner, &self.destination)?;
        let roots = graph
            .roots
            .iter()
            .chain(&grant.roots)
            .chain(&source.roots)
            .chain(&destination.roots)
            .map(|root| Ok((root.identity()?, root.clone())))
            .collect::<io::Result<BTreeMap<_, _>>>()?
            .into_values()
            .collect::<Vec<_>>();
        let guard =
            crate::workspace_custody::lock_workspace_initialization_set(&roots).map_err(error)?;
        let context = if matches!(phase, CommitPhase::Owner | CommitPhase::Complete) {
            crate::project_attachment::dependency_owner_context::OwnerHistoryContext::recovering(
                &self.owner,
                self.request,
            )?
            .for_operation(self.operation())
        } else {
            crate::project_attachment::dependency_owner_context::OwnerHistoryContext::current(
                &self.owner,
            )
        };
        let works = std::iter::once(&self.source)
            .chain(available.iter().copied())
            .collect::<Vec<_>>();
        let context = storage.resolve_consumed_histories(&self.owner, &works, &guard, context)?;
        let current_graph =
            storage.inspect_dependency_graph_with_owner(&graph, &guard, &context)?;
        let source = context.validate(storage, &source, &guard)?;
        let destination = context.validate(storage, &destination, &guard)?;
        if destination != self.basis.destination
            || current_graph.digest() != self.basis.graph
            || current_graph.operation_count() > 256
            || staged.request != self.request
        {
            return Err(invalid("consumption fence selection changed"));
        }
        storage.with_input_grant_owner(&grant, &guard, &context, |_| Ok(()))?;
        let origin = storage
            .lane_origin_bound(&self.destination)?
            .ok_or_else(|| invalid("reservation missing"))?;
        if !crate::project_attachment::dependency_reservation::is_reservation(&origin.value) {
            return Err(invalid("consumption fence requires a reservation"));
        }
        let empty = || -> io::Result<()> {
            if !install
                && !self
                    .destination
                    .attachment
                    .pinned
                    .filesystem()
                    .read_directory_names_bounded(Path::new(""), 1)?
                    .is_empty()
            {
                return Err(invalid("consumption destination contains unexpected work"));
            }
            absent(&self.destination, "dependency-capture.pending")?;
            absent(
                &self.destination,
                crate::project_attachment::dependency_decision::PENDING,
            )
        };
        empty()?;
        let stage_bytes = read(&staged.root, RECEIPT, MAX_RECEIPT)?;
        if hash(&stage_bytes) != staged.receipt {
            return Err(invalid("staged receipt changed"));
        }
        let stage_json =
            Json::parse(std::str::from_utf8(&stage_bytes).map_err(error)?).map_err(error)?;
        let original_graph = stage_json
            .get("graph")
            .ok_or_else(|| invalid("staged graph missing"))?;
        if original_graph.get("closure") != Some(&current_graph.to_json())
            || self.verify_stage_state(
                &staged.root,
                original_graph,
                &origin.allocation,
                &guard,
                install,
            )? != staged.receipt
            || super::super::super::consumption_plan::InitialPlan::verify(
                &self.checkpoint,
                WorkspaceId::from_bytes(short_id(self.basis.prospective.as_bytes())),
                self.limits,
            )? != self.plan
        {
            return Err(invalid("staged consumption material changed"));
        }
        let (_, owner) = context.read(&self.owner)?;
        let owner = owner
            .ok_or_else(|| invalid("owner enrollment missing"))?
            .binding();
        let descriptor = Json::object([
            (
                "schema",
                Json::text("mesh.native-consumption-transaction/v1"),
            ),
            ("request", j(self.request)),
            ("stage", j(staged.receipt)),
            (
                "limits",
                Json::Array(vec![
                    Json::Number(self.limits.entries as u64),
                    Json::Number(self.limits.bytes),
                    Json::Number(self.limits.file_bytes),
                ]),
            ),
        ])
        .encode()
        .into_bytes();
        let body = self.start_body(owner, &source, &destination, hash(&descriptor));
        let pending = match read_private_in_store(&self.destination.store, PENDING) {
            Ok(raw) => Some(raw),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        let (configuration, facts) = self.destination.project().read_native_facts(
            self.destination.metadata_path(),
            &self.destination.store,
            pending.as_deref(),
            None,
        )?;
        let facts = facts.ok_or_else(|| invalid("destination enrollment missing"))?;
        if configuration != self.basis.configuration || facts.binding() != self.basis.enrollment {
            return Err(invalid("destination configuration changed"));
        }
        let mut journal = self
            .destination
            .store
            .open_existing_record_file(Path::new(crate::RECORD_FILE_NAME))?;
        let mut observed = Vec::new();
        (&mut journal)
            .take(MAX_JOURNAL as u64 + 1)
            .read_to_end(&mut observed)?;
        if observed.len() > MAX_JOURNAL {
            return Err(invalid("consumption history exceeds bound"));
        }
        facts.verify(&self.destination.store, &journal, &observed)?;
        let before = &observed[..facts.pending().map_or(observed.len(), |(length, _)| length)];
        let scan = scan_journal(before).map_err(error)?;
        if scan.tail().is_fragment()
            || !matches!(scan.records(), [StoredRecord::Dependency(r)] if r.kind == DependencyKind::Enrollment)
        {
            return Err(invalid(
                "consumption must start from empty enrolled history",
            ));
        }
        let (ordinal, previous) = facts
            .policy()
            .native_head()
            .ok_or_else(|| invalid("enrollment missing"))?;
        if ordinal != 1 {
            return Err(invalid("destination policy already advanced"));
        }
        let payload = Json::object([
            ("schema", Json::text("mesh.dependency-policy/v1")),
            ("authority", j(facts.binding().authority)),
            ("revision", Json::Number(2)),
            ("previous", j(previous)),
            (
                "kind",
                Json::Number(u64::from(DependencyKind::ConsumptionStart.code())),
            ),
            ("body", body),
        ])
        .encode()
        .into_bytes();
        let record = DependencyRecord {
            authority: facts.binding().authority,
            revision: 2,
            previous,
            payload: hash(&payload),
            kind: DependencyKind::ConsumptionStart,
        };
        facts
            .policy()
            .clone()
            .apply(record, &payload)
            .map_err(error)?;
        let metadata = journal.metadata()?;
        let intent = transaction_intent(
            record.kind,
            self.request,
            (metadata.dev(), metadata.ino()),
            before,
            record.payload,
        );
        let frame = frame_record(&StoredRecord::Dependency(record));
        if before.len().saturating_add(frame.len()) > MAX_JOURNAL {
            return Err(invalid("consumption append exceeds bound"));
        }
        if pending.as_ref().is_some_and(|p| p != &intent)
            || facts.pending().is_some_and(|(_, r)| r != record)
        {
            return Err(invalid("another consumption intent owns recovery"));
        }
        let owner_intent = if install {
            self.verify_install_names()?;
            let occupied = !self
                .destination
                .attachment
                .pinned
                .filesystem()
                .read_directory_names_bounded(Path::new(""), self.plan.entries.len())?
                .is_empty();
            if occupied && (pending.is_none() || observed.len() < before.len() + frame.len()) {
                return Err(invalid("installed entries lack a complete durable start"));
            }
            Some(self.retain_owner_consumption_intent(&current_graph, record.payload)?)
        } else {
            None
        };
        let cas = Cas::<PinnedRootFs, Blake3>::with_filesystem(
            self.destination.metadata_path(),
            self.destination.store.filesystem(),
        )
        .map_err(error)?;
        for object in self.checkpoint.objects.iter().cloned().chain([
            stage_bytes,
            descriptor,
            payload,
            self.basis.configuration.as_bytes().to_vec(),
            self.basis.prospective.as_bytes().to_vec(),
            current_graph.to_json().encode().into_bytes(),
        ]) {
            cas.promote(object).map_err(error)?;
        }
        if pending.is_none() {
            write(&self.destination.store, PENDING, intent.as_bytes())?;
        } else {
            self.destination
                .store
                .filesystem()
                .sync_file(Path::new(PENDING))?;
        }
        self.destination.store.sync()?;
        hook("staged", &mut journal, &frame)?;
        empty()?;
        storage.with_input_grant_owner(&grant, &guard, &context, |_| Ok(()))?;
        guard.ensure_current().map_err(error)?;
        if read_private_in_store(&self.destination.store, PENDING)? != intent {
            return Err(invalid("consumption intent changed"));
        }
        journal.rewind()?;
        let mut current = Vec::new();
        (&mut journal)
            .take(MAX_JOURNAL as u64 + 1)
            .read_to_end(&mut current)?;
        facts.verify(&self.destination.store, &journal, &current)?;
        if current != observed {
            return Err(invalid("consumption journal changed"));
        }
        let written = (observed.len() - before.len()).min(frame.len());
        if !frame.starts_with(&observed[before.len()..before.len() + written]) {
            return Err(invalid("foreign consumption suffix"));
        }
        journal.write_all(&frame[written..])?;
        sync(&journal)?;
        self.destination.store.sync()?;
        hook("synced", &mut journal, &frame)?;
        let (after_configuration, after) = self.destination.project().read_native_facts(
            self.destination.metadata_path(),
            &self.destination.store,
            Some(&intent),
            None,
        )?;
        let after = after.ok_or_else(|| invalid("consumption enrollment disappeared"))?;
        let mut expected = before.to_vec();
        expected.extend_from_slice(&frame);
        if observed.len() > expected.len() {
            expected.extend_from_slice(&observed[expected.len()..]);
        }
        after.verify(&self.destination.store, &journal, &expected)?;
        if after_configuration != configuration
            || after.pending() != Some((before.len(), record))
            || read_private_in_store(&self.destination.store, PENDING)? != intent
        {
            return Err(invalid("consumption start did not replay exactly"));
        }
        guard.ensure_current().map_err(error)?;
        if install {
            self.install_entries(staged, &origin.allocation, &guard, |step| {
                hook(step, &mut journal, &frame)
            })?;
            if self.verify_stage_state(
                &staged.root,
                original_graph,
                &origin.allocation,
                &guard,
                true,
            )? != staged.receipt
                || read_private_in_store(
                    &self.destination.store,
                    super::installation::OWNER_INTENT,
                )? != *owner_intent
                    .as_ref()
                    .ok_or_else(|| invalid("owner intent missing"))?
                || read_private_in_store(&self.destination.store, PENDING)? != intent
            {
                return Err(invalid("consumption installation intent changed"));
            }
            let (current_configuration, current_facts) =
                self.destination.project().read_native_facts(
                    self.destination.metadata_path(),
                    &self.destination.store,
                    Some(&intent),
                    None,
                )?;
            if current_configuration != configuration || current_facts.as_ref() != Some(&after) {
                return Err(invalid("consumption journal changed during installation"));
            }
            storage.with_input_grant_owner(&grant, &guard, &context, |_| Ok(()))?;
            guard.ensure_current().map_err(error)?;
        }
        if phase != CommitPhase::Start {
            self.append_consumption_checkpoint(
                &mut journal,
                &intent,
                record.payload,
                before.len() + frame.len(),
                |step, file, frames| hook(step, file, frames),
                |file| sync(file),
            )?;
            if self.verify_stage_state(
                &staged.root,
                original_graph,
                &origin.allocation,
                &guard,
                true,
            )? != staged.receipt
                || read_private_in_store(
                    &self.destination.store,
                    super::installation::OWNER_INTENT,
                )? != *owner_intent
                    .as_ref()
                    .ok_or_else(|| invalid("owner intent missing"))?
            {
                return Err(invalid("checkpoint installation evidence changed"));
            }
            storage.with_input_grant_owner(&grant, &guard, &context, |_| Ok(()))?;
            guard.ensure_current().map_err(error)?;
        }
        if matches!(phase, CommitPhase::Owner | CommitPhase::Complete) {
            let receipt = self.commit_owner_receipt(
                &context,
                owner_intent
                    .as_ref()
                    .ok_or_else(|| invalid("owner intent missing"))?,
                |step, file, frame| hook(step, file, frame),
                |file| sync(file),
            )?;
            let result = if phase == CommitPhase::Complete {
                self.append_completion(
                    &mut journal,
                    &intent,
                    super::complete::CompletionSelection {
                        start: record,
                        owner: receipt,
                        prefix: before.len() + frame.len() + self.frames().len(),
                    },
                    |step, file, frame| hook(step, file, frame),
                    |file| sync(file),
                )?
            } else {
                receipt
            };
            guard.ensure_current().map_err(error)?;
            return Ok(result);
        }
        Ok(record.payload)
    }
}

#[cfg(test)]
pub(super) fn assert_start_fence(
    prepared: &PreparedNativeConsumedStart,
    storage: &AttachmentStorage,
) {
    let staged = prepared.stage(storage).unwrap();
    let journal_path = prepared
        .destination
        .metadata_path()
        .join(crate::RECORD_FILE_NAME);
    let before = fs::read(&journal_path).unwrap();
    let owner_before =
        fs::read(prepared.owner.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap();
    let mut length = 0;
    assert!(prepared
        .fence_with_io(
            storage,
            &staged,
            |step, _, frame| {
                assert_eq!(step, "staged");
                length = frame.len();
                Err(io::Error::other("crash after durable intent, before start"))
            },
            |file| file.sync_all()
        )
        .is_err());
    assert!(length > 0);
    assert_eq!(fs::read(&journal_path).unwrap(), before);
    let intent = read_private_in_store(&prepared.destination.store, PENDING).unwrap();
    super::recovery::assert_reloaded(prepared, storage, &staged);
    for boundary in 1..=length {
        let failure = prepared
            .fence_with_io(
                storage,
                &staged,
                |step, file, frame| {
                    assert_eq!(step, "staged");
                    file.write_all(&frame[boundary - 1..boundary])?;
                    file.sync_all()?;
                    Err(io::Error::other("interrupted required start append"))
                },
                |file| file.sync_all(),
            )
            .unwrap_err();
        assert_eq!(
            failure.to_string(),
            "interrupted required start append",
            "boundary {boundary}"
        );
        let partial = fs::read(&journal_path).unwrap();
        assert_eq!(partial.len(), before.len() + boundary);
        assert!(partial.starts_with(&before));
        assert_eq!(
            read_private_in_store(&prepared.destination.store, PENDING).unwrap(),
            intent
        );
        assert!(prepared.destination.saved_versions().is_err());
        assert_eq!(
            fs::read_dir(prepared.destination.project().root())
                .unwrap()
                .count(),
            0
        );
    }
    let complete = fs::read(&journal_path).unwrap();
    assert!(prepared
        .fence_with_io(
            storage,
            &staged,
            |_, _, _| Ok(()),
            |_| { Err(io::Error::other("start synchronization failed")) }
        )
        .is_err());
    assert_eq!(fs::read(&journal_path).unwrap(), complete);
    assert!(prepared
        .fence_with_io(
            storage,
            &staged,
            |step, _, _| {
                if step == "synced" {
                    return Err(io::Error::other("lost start reply"));
                }
                Ok(())
            },
            |file| file.sync_all()
        )
        .is_err());
    let receipt = prepared.fence_consumption_start(storage, &staged).unwrap();
    assert_eq!(
        prepared.fence_consumption_start(storage, &staged).unwrap(),
        receipt
    );
    assert_eq!(fs::read(&journal_path).unwrap(), complete);
    assert_eq!(
        fs::read(prepared.owner.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap(),
        owner_before
    );
    let unexpected = prepared
        .destination
        .project()
        .root()
        .join("late-fence-editor-work");
    fs::write(&unexpected, b"keep this work").unwrap();
    assert!(prepared.fence_consumption_start(storage, &staged).is_err());
    assert_eq!(fs::read(&unexpected).unwrap(), b"keep this work");
    fs::remove_file(unexpected).unwrap();
    let mut foreign = complete.clone();
    *foreign.last_mut().unwrap() ^= 1;
    fs::write(&journal_path, &foreign).unwrap();
    assert!(prepared.fence_consumption_start(storage, &staged).is_err());
    assert_eq!(fs::read(&journal_path).unwrap(), foreign);
    fs::write(&journal_path, &complete).unwrap();
    super::installation::assert_installation(prepared, storage, &staged, receipt);
    super::checkpoint::assert_checkpoint(prepared, storage, &staged, receipt);
    let owner_receipt = super::owner::assert_owner(prepared, storage, &staged, receipt);
    let complete = fs::read(&journal_path).unwrap();
    storage
        .grant_saved_input(
            &prepared.owner,
            crate::project_attachment::NativeInputGrantRequest {
                source: &prepared.source,
                version: prepared.version,
                destination: &prepared.destination,
                allowed: false,
                expected_previous: Some(prepared.grant),
                request: RecordDigest::from_bytes([92; 32]),
            },
        )
        .unwrap();
    assert!(prepared
        .install_fenced_consumed_start(storage, &staged)
        .is_err());
    assert!(prepared.fence_consumption_start(storage, &staged).is_err());
    let available = prepared.available.iter().collect::<Vec<_>>();
    assert!(storage
        .recover_fenced_consumed_start(
            &prepared.owner,
            NativeConsumedStartRequest {
                input: NativeGrantInspection {
                    source: &prepared.source,
                    version: prepared.version,
                    destination: &prepared.destination,
                    grant: prepared.grant
                },
                available: &available,
                request: prepared.request,
                limits: prepared.limits,
            }
        )
        .is_err());
    assert_eq!(
        prepared
            .commit_fenced_owner_receipt(storage, &staged)
            .unwrap(),
        owner_receipt
    );
    super::owner::assert_historical_binding(prepared);
    assert_eq!(fs::read(&journal_path).unwrap(), complete);
    assert!(prepared.destination.saved_versions().is_err());
    assert_eq!(
        fs::read_dir(prepared.destination.project().root())
            .unwrap()
            .count(),
        prepared.top_entries().len()
    );
    super::complete::assert_complete(prepared, storage, &staged, receipt);
}
