//! Destination history commit behind the required consumption fence.
use super::*;
use crate::project_attachment::consumption_history;
use crate::root_authority::PinnedRootFs;
use mesh_cas::{Blake3, Cas};
use std::io::{Seek as _, Write as _};
impl PreparedNativeConsumedStart {
    /// Install and durably append the exact original signed starting checkpoint.
    /// This is not acknowledgement of owner consumption and does not admit history or runtime.
    pub fn commit_fenced_consumed_checkpoint(
        &self,
        storage: &AttachmentStorage,
        staged: &StagedNativeConsumedStart,
    ) -> io::Result<RecordDigest> {
        self.commit_phase_with_io(
            storage,
            staged,
            true,
            true,
            |_, _, _| Ok(()),
            |f| f.sync_all(),
        )?;
        Ok(self.operation())
    }
    pub(super) fn append_consumption_checkpoint(
        &self,
        journal: &mut fs::File,
        start_intent: &str,
        start: RecordDigest,
        prefix: usize,
        mut hook: impl FnMut(&str, &mut fs::File, &[u8]) -> io::Result<()>,
        mut sync: impl FnMut(&fs::File) -> io::Result<()>,
    ) -> io::Result<()> {
        const MAX: usize = 80 * 1024 * 1024;
        let read_journal = |file: &mut fs::File| -> io::Result<Vec<u8>> {
            file.rewind()?;
            let mut bytes = Vec::new();
            (&mut *file).take(MAX as u64 + 1).read_to_end(&mut bytes)?;
            if bytes.len() > MAX {
                return Err(invalid("consumption history exceeds bound"));
            }
            Ok(bytes)
        };
        let observed = read_journal(journal)?;
        let frames = self.frames();
        if prefix > observed.len()
            || prefix.saturating_add(frames.len()) > MAX
            || !frames.starts_with(&observed[prefix..])
        {
            return Err(invalid("consumption checkpoint bytes differ"));
        }
        let identity = (journal.metadata()?.dev(), journal.metadata()?.ino());
        let intent = consumption_history::intent(
            self.request,
            identity,
            &observed[..prefix],
            start,
            hash(&frames),
        );
        let cas = Cas::<PinnedRootFs, Blake3>::with_filesystem(
            self.destination.metadata_path(),
            self.destination.store.filesystem(),
        )
        .map_err(error)?;
        cas.promote(frames.clone()).map_err(error)?;
        match read_private_in_store(&self.destination.store, consumption_history::PENDING) {
            Ok(raw) if raw == intent => {}
            Ok(_) => return Err(invalid("another consumption checkpoint owns recovery")),
            Err(e) if e.kind() == io::ErrorKind::NotFound => write(
                &self.destination.store,
                consumption_history::PENDING,
                intent.as_bytes(),
            )?,
            Err(e) => return Err(e),
        }
        self.destination
            .store
            .filesystem()
            .sync_file(Path::new(consumption_history::PENDING))?;
        self.destination.store.sync()?;
        hook("checkpoint-staged", journal, &frames)?;
        let current = read_journal(journal)?;
        let (_, facts) = self.destination.project().read_native_facts(
            self.destination.metadata_path(),
            &self.destination.store,
            Some(start_intent),
            None,
        )?;
        facts
            .ok_or_else(|| invalid("consumption enrollment missing"))?
            .verify(&self.destination.store, journal, &current)?;
        if current != observed
            || read_private_in_store(&self.destination.store, super::super::START_PENDING)?
                != start_intent
            || read_private_in_store(&self.destination.store, consumption_history::PENDING)?
                != intent
        {
            return Err(invalid("checkpoint changed before append"));
        }
        journal.write_all(&frames[observed.len() - prefix..])?;
        sync(journal)?;
        self.destination.store.sync()?;
        hook("checkpoint-synced", journal, &frames)?;
        let current = read_journal(journal)?;
        let (_, facts) = self.destination.project().read_native_facts(
            self.destination.metadata_path(),
            &self.destination.store,
            Some(start_intent),
            None,
        )?;
        facts
            .ok_or_else(|| invalid("consumption enrollment missing"))?
            .verify(&self.destination.store, journal, &current)?;
        if current.len() != prefix + frames.len()
            || current[..prefix] != observed[..prefix]
            || current[prefix..] != frames
            || read_private_in_store(&self.destination.store, super::super::START_PENDING)?
                != start_intent
            || read_private_in_store(&self.destination.store, consumption_history::PENDING)?
                != intent
        {
            return Err(invalid("consumption checkpoint did not commit exactly"));
        }
        Ok(())
    }
}

#[cfg(test)]
pub(super) fn assert_checkpoint(
    prepared: &PreparedNativeConsumedStart,
    storage: &AttachmentStorage,
    staged: &StagedNativeConsumedStart,
    receipt: RecordDigest,
) {
    let path = prepared
        .destination
        .metadata_path()
        .join(crate::RECORD_FILE_NAME);
    let before = fs::read(&path).unwrap();
    let owner_path = prepared.owner.metadata_path().join(crate::RECORD_FILE_NAME);
    let owner_before = fs::read(&owner_path).unwrap();
    let frames = prepared.frames();
    let installed = fs::read_dir(prepared.destination.project().root())
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            (e.file_name(), e.metadata().unwrap().ino())
        })
        .collect::<BTreeMap<_, _>>();
    for mode in [
        "checkpoint-partial",
        "checkpoint-lost",
        "checkpoint-complete",
        "checkpoint-complete",
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
            ("receipt", Json::text(receipt.to_hex())),
            ("mode", Json::text(mode)),
        ]);
        let child=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","project_attachment::consumption_prepare::tests::saved_ignore_rules_bind_candidate_without_changing_empty_reservation","--nocapture"])
            .env_remove("MESH_PRIVATE_STAGE_RESTART").env("MESH_FENCED_START_RESTART",value.encode()).output().unwrap();
        assert_eq!(
            child.status.code(),
            Some(if mode == "checkpoint-complete" { 0 } else { 75 }),
            "{mode}: {} {}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
        let current = fs::read(&path).unwrap();
        assert!(current.starts_with(&before));
        assert_eq!(
            &current[before.len()..],
            if mode == "checkpoint-partial" {
                &frames[..1]
            } else {
                &frames
            }
        );
        assert_eq!(fs::read(&owner_path).unwrap(), owner_before);
        assert!(prepared.destination.saved_versions().is_err());
        for (name, ino) in &installed {
            assert_eq!(
                fs::metadata(prepared.destination.project().root().join(name))
                    .unwrap()
                    .ino(),
                *ino
            );
        }
        if mode == "checkpoint-partial" {
            let raw =
                read_private_in_store(&prepared.destination.store, consumption_history::PENDING)
                    .unwrap();
            let start =
                read_private_in_store(&prepared.destination.store, super::super::START_PENDING)
                    .unwrap();
            let cas = Cas::<PinnedRootFs, Blake3>::with_filesystem(
                prepared.destination.metadata_path(),
                prepared.destination.store.filesystem().read_only(),
            )
            .unwrap();
            let m = fs::metadata(&path).unwrap();
            for length in 0..=frames.len() {
                let mut bytes = before.clone();
                bytes.extend_from_slice(&frames[..length]);
                assert!(
                    consumption_history::pending_prefix(
                        &cas,
                        &start,
                        &raw,
                        (m.dev(), m.ino()),
                        &bytes
                    )
                    .is_ok(),
                    "prefix {length}"
                );
                if length > 0 {
                    *bytes.last_mut().unwrap() ^= 1;
                    assert!(
                        consumption_history::pending_prefix(
                            &cas,
                            &start,
                            &raw,
                            (m.dev(), m.ino()),
                            &bytes
                        )
                        .is_err(),
                        "foreign prefix {length}"
                    );
                }
            }
        }
    }
    let complete = fs::read(&path).unwrap();
    let mut syncs = 0;
    let failure = prepared
        .commit_phase_with_io(
            storage,
            staged,
            true,
            true,
            |_, _, _| Ok(()),
            |file| {
                syncs += 1;
                if syncs == 2 {
                    Err(io::Error::other("checkpoint sync refused"))
                } else {
                    file.sync_all()
                }
            },
        )
        .unwrap_err();
    assert_eq!(failure.to_string(), "checkpoint sync refused");
    assert_eq!(fs::read(&path).unwrap(), complete);
    assert_eq!(
        prepared
            .commit_fenced_consumed_checkpoint(storage, staged)
            .unwrap(),
        prepared.operation()
    );
    let mut foreign = complete.clone();
    foreign.push(0x7f);
    fs::write(&path, &foreign).unwrap();
    assert!(prepared
        .commit_fenced_consumed_checkpoint(storage, staged)
        .is_err());
    assert_eq!(fs::read(&path).unwrap(), foreign);
    fs::write(&path, complete).unwrap();
}
