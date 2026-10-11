//! Local completion handshake. Cross-store ordinary admission remains a separate verified boundary.
use super::*;
use crate::{
    project_attachment::{consumption_complete, dependency_transaction::read_payload},
    root_authority::PinnedRootFs,
};
use mesh_cas::{Blake3, Cas};
use mesh_store::{frame_record, DependencyRecord, StoredRecord};
use std::io::{Seek as _, Write as _};
pub(super) struct CompletionSelection {
    pub(super) start: DependencyRecord,
    pub(super) owner: RecordDigest,
    pub(super) prefix: usize,
}
impl PreparedNativeConsumedStart {
    /// Durably complete the local start/checkpoint/owner-receipt handshake. This receipt is not
    /// ordinary history or runtime permission; configuration reconciliation/admission remain gated.
    pub fn complete_fenced_consumption(
        &self,
        storage: &AttachmentStorage,
        staged: &StagedNativeConsumedStart,
    ) -> io::Result<RecordDigest> {
        self.commit_phase_with_io(
            storage,
            staged,
            true,
            CommitPhase::Complete,
            |_, _, _| Ok(()),
            |f| f.sync_all(),
        )
    }
    pub(super) fn append_completion(
        &self,
        journal: &mut fs::File,
        start_intent: &str,
        selection: CompletionSelection,
        hook: impl FnMut(&str, &mut fs::File, &[u8]) -> io::Result<()>,
        sync: impl FnMut(&fs::File) -> io::Result<()>,
    ) -> io::Result<RecordDigest> {
        self.append_completion_held(
            journal,
            start_intent,
            selection,
            (|| Ok(()), |_| Ok(())),
            (hook, sync),
        )
    }
    fn append_completion_held(
        &self,
        journal: &mut fs::File,
        start_intent: &str,
        selection: CompletionSelection,
        verify: (
            impl Fn() -> io::Result<()>,
            impl Fn(DependencyRecord) -> io::Result<()>,
        ),
        io: (
            impl FnMut(&str, &mut fs::File, &[u8]) -> io::Result<()>,
            impl FnMut(&fs::File) -> io::Result<()>,
        ),
    ) -> io::Result<RecordDigest> {
        let (verify_before, verify_after) = verify;
        let (mut hook, mut sync) = io;
        verify_before()?;
        let read_journal = |file: &mut fs::File| -> io::Result<Vec<u8>> {
            file.rewind()?;
            let mut bytes = Vec::new();
            (&mut *file)
                .take(80 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() > 80 * 1024 * 1024 {
                return Err(invalid("completion journal exceeds bound"));
            }
            Ok(bytes)
        };
        let observed = read_journal(journal)?;
        let prefix = selection.prefix;
        if prefix > observed.len() {
            return Err(invalid("completion checkpoint missing"));
        }
        let (configuration, facts) = self.destination.project().read_native_facts(
            self.destination.metadata_path(),
            &self.destination.store,
            Some(start_intent),
            None,
        )?;
        let facts = facts.ok_or_else(|| invalid("completion enrollment missing"))?;
        facts.verify(&self.destination.store, journal, &observed)?;
        if configuration != self.basis.configuration || facts.binding() != self.basis.enrollment {
            return Err(invalid("completion configuration changed"));
        }
        let cas = Cas::<PinnedRootFs, Blake3>::with_filesystem(
            self.destination.metadata_path(),
            self.destination.store.filesystem(),
        )
        .map_err(error)?;
        let (record, payload) = consumption_complete::payload(selection.start, selection.owner)?;
        let mut policy = facts.policy().clone();
        policy
            .apply(
                selection.start,
                &read_payload(&cas, selection.start.payload, 65536)?,
            )
            .map_err(error)?;
        policy.apply(record, &payload).map_err(error)?;
        let frame = frame_record(&StoredRecord::Dependency(record));
        if prefix.saturating_add(frame.len()) > 80 * 1024 * 1024
            || !frame.starts_with(&observed[prefix..])
        {
            return Err(invalid("foreign local completion bytes"));
        }
        let metadata = journal.metadata()?;
        let intent = consumption_complete::intent(
            self.request,
            (metadata.dev(), metadata.ino()),
            &observed[..prefix],
            record.payload,
        );
        cas.promote(payload).map_err(error)?;
        match read_private_in_store(&self.destination.store, consumption_complete::PENDING) {
            Ok(raw) if raw == intent => {}
            Ok(_) => return Err(invalid("another local completion owns recovery")),
            Err(e) if e.kind() == io::ErrorKind::NotFound => write(
                &self.destination.store,
                consumption_complete::PENDING,
                intent.as_bytes(),
            )?,
            Err(e) => return Err(e),
        }
        self.destination
            .store
            .filesystem()
            .sync_file(Path::new(consumption_complete::PENDING))?;
        self.destination.store.sync()?;
        let pending_metadata = self
            .destination
            .store
            .filesystem()
            .inspect_entry(Path::new(consumption_complete::PENDING))?
            .metadata()?;
        let pending_identity = (pending_metadata.dev(), pending_metadata.ino());
        let verify_pending = || -> io::Result<()> {
            let current = self
                .destination
                .store
                .filesystem()
                .inspect_entry(Path::new(consumption_complete::PENDING))?
                .metadata()?;
            if (current.dev(), current.ino()) != pending_identity {
                return Err(invalid("completion pending file identity changed"));
            }
            Ok(())
        };
        hook("complete-staged", journal, &frame)?;
        verify_before()?;
        verify_pending()?;
        let current = read_journal(journal)?;
        facts.verify(&self.destination.store, journal, &current)?;
        if current != observed
            || read_private_in_store(&self.destination.store, consumption_complete::PENDING)?
                != intent
            || read_private_in_store(&self.destination.store, super::super::START_PENDING)?
                != start_intent
        {
            return Err(invalid("completion changed before append"));
        }
        journal.write_all(&frame[observed.len() - prefix..])?;
        sync(journal)?;
        self.destination.store.sync()?;
        hook("complete-synced", journal, &frame)?;
        verify_after(record)?;
        verify_pending()?;
        let current = read_journal(journal)?;
        let (after_configuration, after) = self.destination.project().read_native_facts(
            self.destination.metadata_path(),
            &self.destination.store,
            None,
            None,
        )?;
        let after = after.ok_or_else(|| invalid("completed enrollment missing"))?;
        after.verify(&self.destination.store, journal, &current)?;
        if current.len() != prefix + frame.len()
            || current[..prefix] != observed[..prefix]
            || current[prefix..] != frame
            || after_configuration != configuration
            || after.policy().native_head() != Some((record.revision, record.payload))
            || read_private_in_store(&self.destination.store, consumption_complete::PENDING)?
                != intent
            || read_private_in_store(&self.destination.store, super::super::START_PENDING)?
                != start_intent
        {
            return Err(invalid("completion did not replay exactly"));
        }
        Ok(record.payload)
    }
}

// Immutable local facts only. The native proof independently joins the committed owner receipt
// and complete configured-trust source history; these facts cannot grant ordinary admission.
#[derive(Clone, PartialEq, Eq)]
pub(in crate::project_attachment) struct NativeCompletionMaterial {
    pub(in crate::project_attachment) start: DependencyRecord,
    pub(in crate::project_attachment) owner: RecordDigest,
    pub(in crate::project_attachment) prefix: usize,
    pub(in crate::project_attachment) start_intent: String,
    history_intent: String,
    owner_intent: String,
    journal: (u64, u64),
    sidecars: [(u64, u64); 3],
    completion: Option<(String, (u64, u64))>,
    pub(in crate::project_attachment) receipt: Option<RecordDigest>,
}
impl NativeCompletionMaterial {
    pub(in crate::project_attachment) fn same_origin(&self, other: &Self) -> bool {
        self.start == other.start
            && self.owner == other.owner
            && self.prefix == other.prefix
            && self.start_intent == other.start_intent
            && self.history_intent == other.history_intent
            && self.owner_intent == other.owner_intent
            && self.journal == other.journal
            && self.sidecars == other.sidecars
            && self
                .completion
                .as_ref()
                .is_none_or(|selected| other.completion.as_ref() == Some(selected))
    }
}
impl PreparedNativeConsumedStart {
    pub(in crate::project_attachment) fn native_completion_material_held(
        &self,
        graph: &crate::project_attachment::NativeDependencyGraph,
        owner_receipt: RecordDigest,
        owner_body: &Json,
        guard: &crate::workspace_custody::WorkspaceInitializationGuard,
    ) -> io::Result<NativeCompletionMaterial> {
        use crate::project_attachment::consumption_history;
        guard
            .require_roots(&[
                self.owner.store.clone(),
                self.destination.store.clone(),
                self.destination.project().pinned.clone(),
            ])
            .map_err(error)?;
        let start_intent =
            read_private_in_store(&self.destination.store, super::super::START_PENDING)?;
        let history_intent =
            read_private_in_store(&self.destination.store, consumption_history::PENDING)?;
        let owner_intent =
            read_private_in_store(&self.destination.store, super::installation::OWNER_INTENT)?;
        let (configuration, facts) = self.destination.project().read_native_facts(
            self.destination.metadata_path(),
            &self.destination.store,
            Some(&start_intent),
            None,
        )?;
        let facts = facts.ok_or_else(|| invalid("native completion enrollment missing"))?;
        let (prefix, start) = facts
            .pending()
            .ok_or_else(|| invalid("native completion original start missing"))?;
        if configuration != self.basis.configuration
            || facts.binding() != self.basis.enrollment
            || start.kind != mesh_store::DependencyKind::ConsumptionStart
            || start.revision != 2
            || graph.digest() != self.basis.graph
            || owner_intent != self.owner_consumption_intent(graph, start.payload)
            || Json::parse(&owner_intent).map_err(error)?.get("body") != Some(owner_body)
        {
            return Err(invalid("native completion original selection differs"));
        }
        let mut file = self
            .destination
            .store
            .filesystem()
            .read_only()
            .read_file(Path::new(crate::RECORD_FILE_NAME))?;
        let mut observed = Vec::new();
        (&mut file)
            .take(80 * 1024 * 1024 + 1)
            .read_to_end(&mut observed)?;
        if observed.len() > 80 * 1024 * 1024 {
            return Err(invalid("native completion journal exceeds bound"));
        }
        facts.verify(&self.destination.store, &file, &observed)?;
        let metadata = file.metadata()?;
        let journal = (metadata.dev(), metadata.ino());
        let cas = Cas::<PinnedRootFs, Blake3>::with_filesystem(
            self.destination.metadata_path(),
            self.destination.store.filesystem().read_only(),
        )
        .map_err(error)?;
        let frames = self.frames();
        let end = prefix
            .checked_add(frame_record(&StoredRecord::Dependency(start)).len())
            .and_then(|begin| begin.checked_add(frames.len()))
            .filter(|end| *end <= observed.len())
            .ok_or_else(|| invalid("native completion checkpoint missing"))?;
        let begin = end - frames.len();
        if observed[begin..end] != frames
            || read_payload(&cas, hash(&frames), frames.len())? != frames
        {
            return Err(invalid("native completion signed checkpoint differs"));
        }
        for object in &self.checkpoint.objects {
            if read_payload(&cas, hash(object), object.len())? != *object {
                return Err(invalid(
                    "native completion retained checkpoint object differs",
                ));
            }
        }
        let sidecar_identity = |name: &str| -> io::Result<(u64, u64)> {
            let metadata = self
                .destination
                .store
                .filesystem()
                .inspect_entry(Path::new(name))?
                .metadata()?;
            Ok((metadata.dev(), metadata.ino()))
        };
        let sidecars = [
            sidecar_identity(super::super::START_PENDING)?,
            sidecar_identity(consumption_history::PENDING)?,
            sidecar_identity(super::installation::OWNER_INTENT)?,
        ];
        let (record, _) = consumption_complete::payload(start, owner_receipt)?;
        let frame = frame_record(&StoredRecord::Dependency(record));
        let completion =
            match read_private_in_store(&self.destination.store, consumption_complete::PENDING) {
                Ok(raw) => {
                    if raw
                        != consumption_complete::intent(
                            self.request,
                            journal,
                            &observed[..end],
                            record.payload,
                        )
                        || consumption_complete::verify_prefix(
                            &cas,
                            self.request,
                            journal,
                            &observed,
                            end,
                            start,
                            &raw,
                        )? != record
                    {
                        return Err(invalid(
                            "native completion differs from its committed owner receipt",
                        ));
                    }
                    Some((raw, sidecar_identity(consumption_complete::PENDING)?))
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => None,
                Err(e) => return Err(e),
            };
        if consumption_history::pending_prefix(
            &cas,
            &start_intent,
            &history_intent,
            journal,
            &observed,
            completion.as_ref().map(|(raw, _)| raw.as_str()),
        )?
        .0 != prefix
            || (completion.is_none() && observed.len() != end)
            || !frame.starts_with(&observed[end..])
        {
            return Err(invalid("native completion has a foreign journal suffix"));
        }
        let receipt = (observed.len() == end + frame.len()).then_some(record.payload);
        if receipt.is_some() {
            let (full_configuration, full) = self.destination.project().read_native_facts(
                self.destination.metadata_path(),
                &self.destination.store,
                None,
                None,
            )?;
            let full = full.ok_or_else(|| invalid("native completion full history missing"))?;
            full.verify(&self.destination.store, &file, &observed)?;
            if full_configuration != configuration
                || full.policy().completed_consumption_records()
                    != Some((start, record, owner_receipt))
            {
                return Err(invalid("native completion owner join differs"));
            }
        }
        guard.ensure_current().map_err(error)?;
        Ok(NativeCompletionMaterial {
            start,
            owner: owner_receipt,
            prefix: end,
            start_intent,
            history_intent,
            owner_intent,
            journal,
            sidecars,
            completion,
            receipt,
        })
    }
    pub(in crate::project_attachment) fn commit_native_completion_material_held(
        &self,
        proof: &crate::project_attachment::dependency_private_context::NativeCompletionConsumptionProof<'_>,
        io: (
            impl FnMut(&str, &mut fs::File, &[u8]) -> io::Result<()>,
            impl FnMut(&fs::File) -> io::Result<()>,
        ),
    ) -> io::Result<RecordDigest> {
        proof.verify_candidate(self)?;
        proof.verify_before()?;
        let material = proof.material();
        let mut journal = self
            .destination
            .store
            .open_existing_record_file(Path::new(crate::RECORD_FILE_NAME))?;
        self.append_completion_held(
            &mut journal,
            &material.start_intent,
            CompletionSelection {
                start: material.start,
                owner: material.owner,
                prefix: material.prefix,
            },
            (
                || proof.verify_before(),
                |record| proof.verify_after(record),
            ),
            io,
        )
    }
}

#[cfg(test)]
pub(super) fn assert_complete(
    prepared: &PreparedNativeConsumedStart,
    storage: &AttachmentStorage,
    staged: &StagedNativeConsumedStart,
    start: RecordDigest,
    campaign: super::RecoveryCampaign,
) {
    use crate::project_attachment::{
        consumption_history,
        dependency_transaction::{digest, text},
    };
    let path = prepared
        .destination
        .metadata_path()
        .join(crate::RECORD_FILE_NAME);
    let before = fs::read(&path).unwrap();
    let owner_path = prepared.owner.metadata_path().join(crate::RECORD_FILE_NAME);
    let owner_before = fs::read(&owner_path).unwrap();
    let mut frame = Vec::new();
    let failure = prepared
        .commit_phase_with_io(
            storage,
            staged,
            true,
            CommitPhase::Complete,
            |step, _, bytes| {
                if step == "complete-staged" {
                    frame = bytes.to_vec();
                    Err(io::Error::other("completion staged before append"))
                } else {
                    Ok(())
                }
            },
            |file| file.sync_all(),
        )
        .unwrap_err();
    assert_eq!(failure.to_string(), "completion staged before append");
    super::completed_read::assert_incomplete_read_refused(prepared, storage);
    assert!(!frame.is_empty());
    assert_eq!(fs::read(&path).unwrap(), before);
    let raw =
        read_private_in_store(&prepared.destination.store, consumption_complete::PENDING).unwrap();
    let start_intent =
        read_private_in_store(&prepared.destination.store, super::super::START_PENDING).unwrap();
    let history =
        read_private_in_store(&prepared.destination.store, consumption_history::PENDING).unwrap();
    let cas = Cas::<PinnedRootFs, Blake3>::with_filesystem(
        prepared.destination.metadata_path(),
        prepared.destination.store.filesystem(),
    )
    .unwrap();
    let metadata = fs::metadata(&path).unwrap();
    let identity_pair = (metadata.dev(), metadata.ino());
    if campaign == super::RecoveryCampaign::Completion {
        for length in 0..=frame.len() {
            let mut bytes = before.clone();
            bytes.extend_from_slice(&frame[..length]);
            assert!(
                consumption_history::pending_prefix(
                    &cas,
                    &start_intent,
                    &history,
                    identity_pair,
                    &bytes,
                    Some(&raw)
                )
                .is_ok(),
                "completion prefix {length}"
            );
            if length > 0 {
                *bytes.last_mut().unwrap() ^= 1;
                assert!(
                    consumption_history::pending_prefix(
                        &cas,
                        &start_intent,
                        &history,
                        identity_pair,
                        &bytes,
                        Some(&raw)
                    )
                    .is_err(),
                    "foreign completion prefix {length}"
                );
            }
        }
        // The Start fixture retains all subsequent completion/restart assertions.
        return;
    }
    // A canonical local completion that names a different owner receipt cannot finish this transaction.
    let (_, record, _) = consumption_history::pending_prefix(
        &cas,
        &start_intent,
        &history,
        identity_pair,
        &before,
        Some(&raw),
    )
    .unwrap();
    let (wrong, payload) =
        consumption_complete::payload(record, RecordDigest::from_bytes([107; 32])).unwrap();
    cas.promote(payload).unwrap();
    let wrong_intent =
        consumption_complete::intent(prepared.request, identity_pair, &before, wrong.payload);
    let intent_path = prepared
        .destination
        .metadata_path()
        .join(consumption_complete::PENDING);
    fs::write(&intent_path, &wrong_intent).unwrap();
    assert!(prepared
        .complete_fenced_consumption(storage, staged)
        .is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(fs::read_to_string(&intent_path).unwrap(), wrong_intent);
    fs::write(&intent_path, &raw).unwrap();
    let installed = fs::read_dir(prepared.destination.project().root())
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            (e.file_name(), e.metadata().unwrap().ino())
        })
        .collect::<BTreeMap<_, _>>();
    let mut completed = None;
    for mode in [
        "complete-partial",
        "complete-lost",
        "complete-complete",
        "complete-complete",
    ] {
        let value = Json::object([
            ("storage", Json::text(storage.path.to_string_lossy())),
            ("owner", Json::text(prepared.owner.id())),
            ("source", Json::text(prepared.source.id())),
            ("destination", Json::text(prepared.destination.id())),
            ("version", Json::text(prepared.version.operation().to_hex())),
            ("request", Json::text(prepared.request.to_hex())),
            ("grant", Json::text(prepared.grant.to_hex())),
            ("stage", Json::text(staged.receipt.to_hex())),
            ("physical", Json::text(identity(&staged.root).unwrap())),
            ("operation", Json::text(prepared.operation().to_hex())),
            ("receipt", Json::text(start.to_hex())),
            ("mode", Json::text(mode)),
        ]);
        let child=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","project_attachment::consumption_prepare::tests::saved_ignore_rules_bind_candidate_without_changing_empty_reservation","--nocapture"])
            .env_remove("MESH_PRIVATE_STAGE_RESTART").env("MESH_FENCED_START_RESTART",value.encode()).output().unwrap();
        assert_eq!(
            child.status.code(),
            Some(if mode == "complete-complete" { 0 } else { 75 }),
            "{mode}: {} {}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
        let current = fs::read(&path).unwrap();
        assert!(current.starts_with(&before));
        assert_eq!(
            &current[before.len()..],
            if mode == "complete-partial" {
                &frame[..1]
            } else {
                &frame
            }
        );
        if mode == "complete-partial" {
            super::completed_read::assert_incomplete_read_refused(prepared, storage);
        }
        if mode != "complete-partial" {
            if let Some(ref previous) = completed {
                assert_eq!(&current, previous);
            }
            completed = Some(current);
        }
        assert_eq!(fs::read(&owner_path).unwrap(), owner_before);
        for (name, inode) in &installed {
            assert_eq!(
                fs::metadata(prepared.destination.project().root().join(name))
                    .unwrap()
                    .ino(),
                *inode
            );
        }
        assert!(prepared.destination.saved_versions().is_err());
    }
    let result = prepared
        .complete_fenced_consumption(storage, staged)
        .unwrap();
    let expected = digest(text(&Json::parse(&raw).unwrap(), "payload").unwrap()).unwrap();
    assert_eq!(result, expected);
    assert!(prepared
        .commit_fenced_owner_receipt(storage, staged)
        .is_err());
    assert!(prepared
        .commit_fenced_consumed_checkpoint(storage, staged)
        .is_err());
    let mut syncs = 0;
    let failure = prepared
        .commit_phase_with_io(
            storage,
            staged,
            true,
            CommitPhase::Complete,
            |_, _, _| Ok(()),
            |file| {
                syncs += 1;
                if syncs == 4 {
                    Err(io::Error::other("completion sync refused"))
                } else {
                    file.sync_all()
                }
            },
        )
        .unwrap_err();
    assert_eq!(failure.to_string(), "completion sync refused");
    assert_eq!(fs::read(&path).unwrap(), completed.unwrap());
    super::completed_read::assert_completed_read(prepared, storage);
}
