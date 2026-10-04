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
        mut hook: impl FnMut(&str, &mut fs::File, &[u8]) -> io::Result<()>,
        mut sync: impl FnMut(&fs::File) -> io::Result<()>,
    ) -> io::Result<RecordDigest> {
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
        hook("complete-staged", journal, &frame)?;
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

#[cfg(test)]
pub(super) fn assert_complete(
    prepared: &PreparedNativeConsumedStart,
    storage: &AttachmentStorage,
    staged: &StagedNativeConsumedStart,
    start: RecordDigest,
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
}
