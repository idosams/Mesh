//! Owning-authority receipt commit. Destination completion and admission remain separate.
use super::*;
use crate::project_attachment::{
    dependency_decision::transaction_intent,
    dependency_owner_context::{pending_name, OwnerHistoryContext},
    dependency_transaction::read_payload,
};
use crate::root_authority::PinnedRootFs;
use mesh_cas::{Blake3, Cas};
use mesh_store::{frame_record, DependencyKind, DependencyRecord, StoredRecord};
use std::io::{Seek as _, Write as _};
impl PreparedNativeConsumedStart {
    /// Durably commit the exact owner consumption receipt after the signed destination checkpoint.
    /// This historical receipt is not local completion, ordinary history or run permission.
    pub fn commit_fenced_owner_receipt(
        &self,
        storage: &AttachmentStorage,
        staged: &StagedNativeConsumedStart,
    ) -> io::Result<RecordDigest> {
        self.commit_phase_with_io(
            storage,
            staged,
            true,
            CommitPhase::Owner,
            |_, _, _| Ok(()),
            |f| f.sync_all(),
        )
    }
    pub(super) fn commit_owner_receipt(
        &self,
        context: &OwnerHistoryContext<'_>,
        stable_intent: &str,
        mut hook: impl FnMut(&str, &mut fs::File, &[u8]) -> io::Result<()>,
        mut sync: impl FnMut(&fs::File) -> io::Result<()>,
    ) -> io::Result<RecordDigest> {
        let value = Json::parse(stable_intent).map_err(error)?;
        let body = value
            .get("body")
            .ok_or_else(|| invalid("owner consumption body missing"))?;
        let (configuration, proof) = context.read(&self.owner)?;
        let proof = proof.ok_or_else(|| invalid("owner enrollment missing"))?;
        let cas = Cas::<PinnedRootFs, Blake3>::with_filesystem(
            self.owner.metadata_path(),
            self.owner.store.filesystem(),
        )
        .map_err(error)?;
        let mut journal = self
            .owner
            .store
            .open_existing_record_file(Path::new(crate::RECORD_FILE_NAME))?;
        let read_journal = |f: &mut fs::File| -> io::Result<Vec<u8>> {
            f.rewind()?;
            let mut bytes = Vec::new();
            (&mut *f)
                .take(80 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() > 80 * 1024 * 1024 {
                return Err(invalid("owner history exceeds bound"));
            }
            Ok(bytes)
        };
        let observed = read_journal(&mut journal)?;
        proof.verify(&self.owner.store, &journal, &observed)?;
        if let Some(record) = proof.policy().native_request(self.request) {
            let bytes = read_payload(&cas, record.payload, 65536)?;
            let existing =
                Json::parse(std::str::from_utf8(&bytes).map_err(error)?).map_err(error)?;
            if record.kind != DependencyKind::Consumption || existing.get("body") != Some(body) {
                return Err(invalid("owner receipt request was reused"));
            }
            sync(&journal)?;
            self.owner.store.sync()?;
            let current = read_journal(&mut journal)?;
            proof.verify(&self.owner.store, &journal, &current)?;
            return Ok(record.payload);
        }
        let prefix = proof.pending().map_or(observed.len(), |(n, _)| n);
        let before = &observed[..prefix];
        let (revision, previous) = proof
            .policy()
            .native_head()
            .ok_or_else(|| invalid("owner head missing"))?;
        let revision = revision
            .checked_add(1)
            .ok_or_else(|| invalid("owner history exhausted"))?;
        let payload = Json::object([
            ("schema", Json::text("mesh.dependency-policy/v1")),
            ("authority", Json::text(proof.binding().authority.to_hex())),
            ("revision", Json::Number(revision)),
            ("previous", Json::text(previous.to_hex())),
            (
                "kind",
                Json::Number(u64::from(DependencyKind::Consumption.code())),
            ),
            ("body", body.clone()),
        ])
        .encode()
        .into_bytes();
        let record = DependencyRecord {
            authority: proof.binding().authority,
            revision,
            previous,
            payload: hash(&payload),
            kind: DependencyKind::Consumption,
        };
        proof
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
        if before.len().saturating_add(frame.len()) > 80 * 1024 * 1024
            || !frame.starts_with(&observed[prefix..])
        {
            return Err(invalid("owner receipt suffix differs"));
        }
        let name = pending_name(self.request, record.payload);
        match read_private_in_store(&self.owner.store, &name) {
            Ok(raw) if raw == intent => {}
            Ok(_) => {
                return Err(invalid(
                    "owner pending receipt conflicts with current history",
                ))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                cas.promote(payload).map_err(error)?;
                write(&self.owner.store, &name, intent.as_bytes())?;
            }
            Err(e) => return Err(e),
        }
        self.owner.store.filesystem().sync_file(Path::new(&name))?;
        self.owner.store.sync()?;
        hook("owner-staged", &mut journal, &frame)?;
        let current = read_journal(&mut journal)?;
        proof.verify(&self.owner.store, &journal, &current)?;
        if current != observed || read_private_in_store(&self.owner.store, &name)? != intent {
            return Err(invalid("owner receipt changed before append"));
        }
        journal.write_all(&frame[observed.len() - prefix..])?;
        sync(&journal)?;
        self.owner.store.sync()?;
        hook("owner-synced", &mut journal, &frame)?;
        let current = read_journal(&mut journal)?;
        let (after_configuration, after) = self
            .owner
            .project()
            .read_configuration(self.owner.metadata_path(), &self.owner.store)?;
        let after = after.ok_or_else(|| invalid("owner enrollment disappeared"))?;
        after.verify(&self.owner.store, &journal, &current)?;
        if current.len() != prefix + frame.len()
            || current[..prefix] != *before
            || current[prefix..] != frame
            || after_configuration != configuration
            || after.policy().native_request(self.request) != Some(record)
            || read_private_in_store(&self.owner.store, &name)? != intent
        {
            return Err(invalid("owner receipt did not commit exactly"));
        }
        Ok(record.payload)
    }
}

#[cfg(test)]
pub(super) fn assert_owner(
    prepared: &PreparedNativeConsumedStart,
    storage: &AttachmentStorage,
    staged: &StagedNativeConsumedStart,
    start: RecordDigest,
) -> RecordDigest {
    let path = prepared.owner.metadata_path().join(crate::RECORD_FILE_NAME);
    let before = fs::read(&path).unwrap();
    let destination_path = prepared
        .destination
        .metadata_path()
        .join(crate::RECORD_FILE_NAME);
    let destination_before = fs::read(&destination_path).unwrap();
    let error = prepared
        .commit_phase_with_io(
            storage,
            staged,
            true,
            CommitPhase::Owner,
            |step, _, _| {
                if step == "owner-staged" {
                    Err(io::Error::other("owner intent staged before append"))
                } else {
                    Ok(())
                }
            },
            |f| f.sync_all(),
        )
        .unwrap_err();
    assert_eq!(error.to_string(), "owner intent staged before append");
    assert_eq!(fs::read(&path).unwrap(), before);
    let prefix = format!("consumption-owner-{}-", prepared.request.to_hex());
    let attempts = || {
        fs::read_dir(prepared.owner.metadata_path())
            .unwrap()
            .filter_map(|e| {
                let e = e.unwrap();
                e.file_name()
                    .to_str()
                    .filter(|n| n.starts_with(&prefix))
                    .map(|_| e.path())
            })
            .map(|p| {
                let b = fs::read(&p).unwrap();
                (p, b)
            })
            .collect::<BTreeMap<_, _>>()
    };
    let original_attempts = attempts();
    assert_eq!(original_attempts.len(), 1);
    // An unrelated valid decision advances owner history between staging and the first byte.
    prepared
        .owner
        .decide_saved_input(
            prepared.version,
            crate::project_attachment::SavedInputDecision::Eligible,
            None,
            RecordDigest::from_bytes([96; 32]),
        )
        .unwrap();
    let advanced = fs::read(&path).unwrap();
    assert!(advanced.starts_with(&before));
    assert!(advanced.len() > before.len());
    let mut committed = None;
    for mode in [
        "owner-partial",
        "owner-lost",
        "owner-complete",
        "owner-complete",
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
            Some(if mode == "owner-complete" { 0 } else { 75 }),
            "{mode}: {} {}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
        let current = fs::read(&path).unwrap();
        assert!(current.starts_with(&advanced));
        if mode == "owner-partial" {
            assert_eq!(current.len(), advanced.len() + 1);
            assert!(prepared.owner.saved_versions().is_err());
            let selected = storage
                .prepare_dependency_graph(
                    &prepared.owner,
                    &prepared.source,
                    prepared.version.operation(),
                    &[],
                )
                .unwrap();
            let _guard =
                crate::workspace_custody::lock_workspace_initialization_set(&selected.roots)
                    .unwrap();
            let context =
                OwnerHistoryContext::recovering(&prepared.owner, prepared.request).unwrap();
            let (name, _, payload) = context.pending_roots(&prepared.owner).unwrap().unwrap();
            let graph = storage
                .inspect_dependency_graph_with_owner(&selected, &_guard, &context)
                .unwrap();
            let retained = graph.retained_content_json().encode();
            assert!(retained.contains(&name));
            assert!(retained.contains(&payload.to_hex()));
        } else {
            if let Some(ref prior) = committed {
                assert_eq!(&current, prior);
            }
            committed = Some(current);
        }
        assert_eq!(fs::read(&destination_path).unwrap(), destination_before);
        for (p, b) in &original_attempts {
            assert_eq!(fs::read(p).unwrap(), *b);
        }
        assert_eq!(attempts().len(), 2);
        assert!(prepared.destination.saved_versions().is_err());
    }
    let result = prepared
        .commit_fenced_owner_receipt(storage, staged)
        .unwrap();
    let complete = fs::read(&path).unwrap();
    let mut syncs = 0;
    let failure = prepared
        .commit_phase_with_io(
            storage,
            staged,
            true,
            CommitPhase::Owner,
            |_, _, _| Ok(()),
            |file| {
                syncs += 1;
                if syncs == 3 {
                    Err(io::Error::other("owner sync refused"))
                } else {
                    file.sync_all()
                }
            },
        )
        .unwrap_err();
    assert_eq!(failure.to_string(), "owner sync refused");
    assert_eq!(fs::read(&path).unwrap(), complete);
    let _guard =
        crate::workspace_custody::lock_workspace_initialization(&prepared.owner.store).unwrap();
    let (_, proof) = prepared
        .owner
        .project()
        .read_configuration(prepared.owner.metadata_path(), &prepared.owner.store)
        .unwrap();
    let proof = proof.unwrap();
    let fact = proof
        .policy()
        .consumption_facts()
        .into_iter()
        .find(|f| f.record == result)
        .unwrap();
    assert_eq!(fact.start.2, prepared.operation());
    assert_eq!(fact.grant, prepared.grant);
    result
}

#[cfg(test)]
pub(super) fn assert_historical_binding(prepared: &PreparedNativeConsumedStart) {
    let _guard =
        crate::workspace_custody::lock_workspace_initialization(&prepared.owner.store).unwrap();
    let (_, proof) = prepared
        .owner
        .project()
        .read_configuration(prepared.owner.metadata_path(), &prepared.owner.store)
        .unwrap();
    let proof = proof.unwrap();
    let record = proof.policy().native_request(prepared.request).unwrap();
    let fact = proof
        .policy()
        .consumption_facts()
        .into_iter()
        .find(|f| f.record == record.payload)
        .unwrap();
    let permits = |operation| {
        OwnerHistoryContext::recovering(&prepared.owner, prepared.request)
            .unwrap()
            .for_operation(operation)
            .historical_input(
                &proof,
                fact.source,
                (fact.start.0, fact.start.1),
                fact.grant,
                fact.bindings.unwrap(),
            )
    };
    assert!(permits(prepared.operation()));
    assert!(!permits(RecordDigest::from_bytes([101; 32])));
}
