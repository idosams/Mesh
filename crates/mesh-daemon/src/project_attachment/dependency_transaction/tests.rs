use super::*;
use crate::project_attachment::{AttachmentStorage, ObservationLimits};
use ed25519_dalek::{Signer as _, SigningKey};
use std::path::PathBuf;
struct Fixture {
    root: PathBuf,
    source: PathBuf,
    storage: AttachmentStorage,
    attachment: ProvisionedAttachment,
}
impl Fixture {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "mesh-native-enrollment-{name}-{}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        let source = root.join("source");
        let metadata = root.join("metadata");
        std::fs::create_dir(&source).unwrap();
        std::fs::create_dir(&metadata).unwrap();
        std::fs::write(source.join("note"), b"original project").unwrap();
        let storage = AttachmentStorage::open(&metadata).unwrap();
        let attachment = storage.provision(&source).unwrap();
        let key = SigningKey::from_bytes(&[67; 32]);
        let input = attachment
            .project()
            .capture_inputs(ObservationLimits::default())
            .unwrap();
        attachment
            .project()
            .save_capture(
                attachment.metadata_path(),
                &input,
                mesh_types::PublicKey::from_bytes(key.verifying_key().to_bytes()),
                |payload| {
                    Ok::<_, &'static str>(mesh_types::Signature::from_bytes(
                        key.sign(payload.as_bytes()).to_bytes(),
                    ))
                },
            )
            .unwrap();
        Self {
            root,
            source,
            storage,
            attachment,
        }
    }
    fn journal_path(&self) -> PathBuf {
        self.attachment.metadata_path().join(RECORD_FILE_NAME)
    }
    fn journal(&self) -> Vec<u8> {
        std::fs::read(self.journal_path()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn native_enrollment_replays_exactly_and_preserves_original_history_and_source() {
    let f = Fixture::new("roundtrip");
    let before = f.journal();
    let enrolled = f.attachment.enroll_dependency_history().unwrap();
    assert_eq!(enrolled.binding().project.to_hex(), f.attachment.id());
    let after = f.journal();
    assert_eq!(&after[..before.len()], before);
    assert_eq!(after.len() - before.len(), 145);
    let reopened = f.storage.reopen(f.attachment.id()).unwrap();
    assert_eq!(reopened.enroll_dependency_history().unwrap(), enrolled);
    assert_eq!(f.journal(), after);
    assert!(!reopened.saved_versions().unwrap().is_empty());
    assert_eq!(
        std::fs::read(f.source.join("note")).unwrap(),
        b"original project"
    );
    let scan = scan_journal(&after).unwrap();
    assert_eq!(
        scan.records()
            .iter()
            .filter(|r| matches!(r, StoredRecord::Dependency(_)))
            .count(),
        1
    );
}

#[test]
fn every_interrupted_enrollment_frame_prefix_recovers_without_duplicate_append() {
    for prefix in 0..=145 {
        let f = Fixture::new(&format!("prefix-{prefix}"));
        let before = f.journal();
        let failed = f
            .attachment
            .enroll_dependency_with_hook(|step, file, frame| {
                if matches!(step, Step::Fenced) {
                    assert_eq!(frame.len(), 145);
                    file.write_all(&frame[..prefix])?;
                    file.sync_all()?;
                    return Err(io::Error::other("injected process stop after frame prefix"));
                }
                Ok(())
            });
        assert!(failed.is_err());
        assert_eq!(f.journal().len(), before.len() + prefix);
        let reopened = f.storage.reopen(f.attachment.id()).unwrap();
        let enrolled = reopened.enroll_dependency_history().unwrap();
        let after = f.journal();
        assert_eq!(&after[..before.len()], before);
        assert_eq!(after.len(), before.len() + 145);
        assert_eq!(reopened.enroll_dependency_history().unwrap(), enrolled);
        assert_eq!(f.journal(), after);
        assert_eq!(
            std::fs::read(f.source.join("note")).unwrap(),
            b"original project"
        );
    }
}

#[test]
fn staging_and_lost_acknowledgement_failures_resume_the_same_enrollment() {
    for (name, stop) in [
        ("staged", Step::Staged),
        ("managed-fence", Step::ManagedFenced),
        ("ack", Step::Appended),
    ] {
        let f = Fixture::new(name);
        let before = f.journal();
        let mut expected = None;
        assert!(f
            .attachment
            .enroll_dependency_with_hook(|step, _, frame| {
                if std::mem::discriminant(&step) == std::mem::discriminant(&stop) {
                    expected = Some(frame.to_vec());
                    return Err(io::Error::other("injected stop"));
                }
                Ok(())
            })
            .is_err());
        if !matches!(stop, Step::Staged) {
            assert!(crate::workspace_custody::require_unassigned_path(
                f.attachment.metadata_path()
            )
            .is_err());
        }
        let enrolled = f.attachment.enroll_dependency_history().unwrap();
        let after = f.journal();
        assert_eq!(&after[..before.len()], before);
        assert_eq!(&after[before.len()..], expected.unwrap());
        assert_eq!(f.attachment.enroll_dependency_history().unwrap(), enrolled);
        assert_eq!(f.journal(), after);
    }
}

#[test]
fn unknown_fragments_changed_prefix_and_identical_journal_replacement_refuse_unchanged() {
    for mode in [
        "legacy-fragment",
        "prefix",
        "replacement",
        "foreign-suffix",
        "intent",
    ] {
        let f = Fixture::new(mode);
        if mode == "legacy-fragment" {
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(f.journal_path())
                .unwrap();
            file.write_all(b"partial").unwrap();
            file.sync_all().unwrap();
        } else {
            assert!(f
                .attachment
                .enroll_dependency_with_hook(|step, _, _| {
                    if matches!(step, Step::Fenced) {
                        return Err(io::Error::other("stop before append"));
                    }
                    Ok(())
                })
                .is_err());
            match mode {
                "prefix" => {
                    let mut bytes = f.journal();
                    let last = bytes.len() - 1;
                    bytes[last] ^= 1;
                    std::fs::write(f.journal_path(), bytes).unwrap();
                }
                "replacement" => {
                    let bytes = f.journal();
                    std::fs::rename(f.journal_path(), f.root.join("retained-journal")).unwrap();
                    std::fs::write(f.journal_path(), bytes).unwrap();
                }
                "foreign-suffix" => {
                    let mut file = std::fs::OpenOptions::new()
                        .append(true)
                        .open(f.journal_path())
                        .unwrap();
                    file.write_all(b"unrelated work").unwrap();
                }
                "intent" => {
                    let marker =
                        Json::parse(&read_private(&f.attachment, HISTORY).unwrap()).unwrap();
                    let id = digest(text(&marker, "dependency_authority").unwrap()).unwrap();
                    let layout = mesh_cas::StoreLayout::new(f.attachment.metadata_path());
                    std::fs::write(
                        layout.chunk_path(&Digest32::from_bytes(*id.as_bytes())),
                        b"corrupt intent",
                    )
                    .unwrap();
                }
                _ => unreachable!(),
            }
        }
        let journal = f.journal();
        let marker = read_private(&f.attachment, HISTORY).unwrap();
        assert!(f.attachment.enroll_dependency_history().is_err(), "{mode}");
        assert_eq!(f.journal(), journal);
        assert_eq!(read_private(&f.attachment, HISTORY).unwrap(), marker);
        assert_eq!(
            std::fs::read(f.source.join("note")).unwrap(),
            b"original project"
        );
    }
}

#[test]
fn enrollment_sync_failure_never_acknowledges_and_retry_resynchronizes() {
    let f = Fixture::new("sync-error");
    for _ in 0..2 {
        let called = std::cell::Cell::new(false);
        assert!(f
            .attachment
            .enroll_dependency_with_io(
                |_, _, _| Ok(()),
                |_| {
                    called.set(true);
                    Err(io::Error::other("injected journal sync failure"))
                }
            )
            .is_err());
        assert!(called.get());
    }
    let before = f.journal();
    f.attachment.enroll_dependency_history().unwrap();
    assert_eq!(f.journal(), before);
}
