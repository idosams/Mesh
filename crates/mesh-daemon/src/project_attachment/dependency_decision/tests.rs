use super::*;
use crate::project_attachment::{AttachmentStorage, ObservationLimits};
use ed25519_dalek::{Signer as _, SigningKey};
use std::{fs, path::PathBuf};
struct Fixture {
    root: PathBuf,
    history: ProvisionedAttachment,
    versions: Vec<SavedAttachmentVersion>,
}
impl Fixture {
    fn new(name: &str, enroll: bool) -> Self {
        let root = std::env::temp_dir().join(format!(
            "mesh-native-input-decision-{name}-{}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("source")).unwrap();
        fs::create_dir(root.join("metadata")).unwrap();
        let storage = AttachmentStorage::open(&root.join("metadata")).unwrap();
        let history = storage.provision(&root.join("source")).unwrap();
        let key = SigningKey::from_bytes(&[67; 32]);
        let mut versions = Vec::new();
        for bytes in ["first", "second"] {
            fs::write(root.join("source/note"), bytes).unwrap();
            let input = history
                .project()
                .capture_inputs(ObservationLimits::default())
                .unwrap();
            versions.push(
                history
                    .project()
                    .save_capture(
                        history.metadata_path(),
                        &input,
                        mesh_types::PublicKey::from_bytes(key.verifying_key().to_bytes()),
                        |payload| {
                            Ok::<_, &'static str>(mesh_types::Signature::from_bytes(
                                key.sign(payload.as_bytes()).to_bytes(),
                            ))
                        },
                    )
                    .unwrap(),
            );
        }
        if enroll {
            history.enroll_dependency_history().unwrap();
        }
        Self {
            root,
            history,
            versions,
        }
    }
    fn journal(&self) -> Vec<u8> {
        fs::read(self.history.metadata_path().join(RECORD_FILE_NAME)).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn request(n: u8) -> RecordDigest {
    RecordDigest::from_bytes([n; 32])
}

#[test]
fn native_decisions_reject_replace_and_revalidate_exact_saved_input_without_rewriting_history() {
    let f = Fixture::new("progression", true);
    let enrollment = f.history.enroll_dependency_history().unwrap();
    let before = f.journal();
    let version = f.versions[0];
    let rejected = f
        .history
        .decide_saved_input(version, SavedInputDecision::Rejected, None, request(1))
        .unwrap();
    assert_eq!(rejected.revision(), 1);
    assert!(f
        .history
        .decide_saved_input(version, SavedInputDecision::Eligible, None, request(2))
        .is_err());
    let replacement = f
        .history
        .decide_saved_input(
            version,
            SavedInputDecision::Replaced(f.versions[1]),
            Some(rejected.record()),
            request(2),
        )
        .unwrap();
    assert_eq!(replacement.revision(), 2);
    let revalidated = f
        .history
        .decide_saved_input(
            version,
            SavedInputDecision::Eligible,
            Some(replacement.record()),
            request(3),
        )
        .unwrap();
    assert_eq!(revalidated.revision(), 3);
    let after = f.journal();
    assert_eq!(f.history.enroll_dependency_history().unwrap(), enrollment);
    assert_eq!(f.journal(), after);
    assert_eq!(&after[..before.len()], before);
    assert_eq!(after.len() - before.len(), 3 * 145);
    assert_eq!(
        f.history
            .decide_saved_input(version, SavedInputDecision::Rejected, None, request(1))
            .unwrap(),
        rejected
    );
    assert_eq!(
        f.journal(),
        after,
        "historical retry must not rewind or duplicate decisions"
    );
    assert_eq!(
        f.history
            .project()
            .saved_file(f.history.metadata_path(), version, "note")
            .unwrap()
            .unwrap(),
        b"first"
    );
    assert_eq!(fs::read(f.root.join("source/note")).unwrap(), b"second");
    let _guard = crate::workspace_custody::lock_workspace_initialization(&f.history.store).unwrap();
    let (_, proof) = f
        .history
        .project()
        .read_configuration(f.history.metadata_path(), &f.history.store)
        .unwrap();
    let proof = proof.unwrap();
    let binding = proof.binding();
    assert_eq!(
        proof
            .policy()
            .native_decision(binding.project, binding.installation, version.operation()),
        Some((3, revalidated.record()))
    );
    assert!(!f.history.metadata_path().join(PENDING).exists());
}

#[test]
fn native_decisions_refuse_foreign_self_replacement_conflicting_requests_and_legacy_input() {
    let f = Fixture::new("refusal", true);
    let foreign = Fixture::new("foreign", false);
    let before = f.journal();
    let version = f.versions[0];
    assert!(f
        .history
        .decide_saved_input(
            foreign.versions[0],
            SavedInputDecision::Rejected,
            None,
            request(1)
        )
        .is_err());
    assert!(f
        .history
        .decide_saved_input(
            version,
            SavedInputDecision::Replaced(foreign.versions[0]),
            None,
            request(1)
        )
        .is_err());
    assert!(f
        .history
        .decide_saved_input(
            version,
            SavedInputDecision::Replaced(version),
            None,
            request(1)
        )
        .is_err());
    assert!(f
        .history
        .decide_saved_input(version, SavedInputDecision::Rejected, None, ZERO)
        .is_err());
    assert!(foreign
        .history
        .decide_saved_input(
            foreign.versions[0],
            SavedInputDecision::Rejected,
            None,
            request(1)
        )
        .is_err());
    assert_eq!(f.journal(), before);
    let rejected = f
        .history
        .decide_saved_input(version, SavedInputDecision::Rejected, None, request(1))
        .unwrap();
    let after = f.journal();
    assert!(f
        .history
        .decide_saved_input(version, SavedInputDecision::Eligible, None, request(1))
        .is_err());
    assert!(f
        .history
        .decide_saved_input(
            version,
            SavedInputDecision::Eligible,
            Some(request(99)),
            request(2)
        )
        .is_err());
    assert_eq!(f.journal(), after);
    assert_eq!(
        f.history
            .decide_saved_input(version, SavedInputDecision::Rejected, None, request(1))
            .unwrap(),
        rejected
    );
}

#[test]
fn every_interrupted_decision_frame_prefix_recovers_only_the_same_native_request() {
    for prefix in 0..=145 {
        let f = Fixture::new(&format!("prefix-{prefix}"), true);
        let before = f.journal();
        let version = f.versions[0];
        let result = f.history.decide_with_io(
            version,
            SavedInputDecision::Rejected,
            None,
            request(1),
            |step, journal, frame| {
                if matches!(step, Step::Staged) {
                    assert_eq!(frame.len(), 145);
                    journal.write_all(&frame[..prefix])?;
                    journal.sync_all()?;
                    return Err(io::Error::other("injected interrupted append"));
                }
                Ok(())
            },
            |file| file.sync_all(),
        );
        assert!(result.is_err());
        let interrupted = f.journal();
        assert_eq!(interrupted.len(), before.len() + prefix);
        if (1..145).contains(&prefix) {
            assert!(f.history.saved_versions().is_err());
        }
        assert!(f
            .history
            .decide_saved_input(version, SavedInputDecision::Eligible, None, request(1))
            .is_err());
        assert!(f
            .history
            .decide_saved_input(version, SavedInputDecision::Rejected, None, request(2))
            .is_err());
        assert_eq!(f.journal(), interrupted);
        let storage = AttachmentStorage::open(&f.root.join("metadata")).unwrap();
        let reopened = storage.reopen(f.history.id()).unwrap();
        let committed = reopened
            .decide_saved_input(version, SavedInputDecision::Rejected, None, request(1))
            .unwrap();
        let after = f.journal();
        assert_eq!(&after[..before.len()], before);
        assert_eq!(after.len(), before.len() + 145);
        assert_eq!(
            reopened
                .decide_saved_input(version, SavedInputDecision::Rejected, None, request(1))
                .unwrap(),
            committed
        );
        assert_eq!(f.journal(), after);
        assert!(reopened.saved_versions().is_ok());
        assert!(!reopened.metadata_path().join(PENDING).exists());
    }
}

#[test]
fn sync_failure_and_lost_acknowledgement_never_acknowledge_or_duplicate_decisions() {
    for mode in ["sync", "ack"] {
        let f = Fixture::new(mode, true);
        let version = f.versions[0];
        let before = f.journal();
        let result = f.history.decide_with_io(
            version,
            SavedInputDecision::Rejected,
            None,
            request(1),
            |step, _, _| {
                if mode == "ack" && matches!(step, Step::Appended) {
                    Err(io::Error::other("lost acknowledgement"))
                } else {
                    Ok(())
                }
            },
            |file| {
                if mode == "sync" {
                    Err(io::Error::other("sync unavailable"))
                } else {
                    file.sync_all()
                }
            },
        );
        assert!(result.is_err());
        assert_eq!(f.journal().len(), before.len() + 145);
        let committed = f
            .history
            .decide_saved_input(version, SavedInputDecision::Rejected, None, request(1))
            .unwrap();
        let after = f.journal();
        let mut called = false;
        assert!(f
            .history
            .decide_with_io(
                version,
                SavedInputDecision::Rejected,
                None,
                request(1),
                |_, _, _| Ok(()),
                |_| {
                    called = true;
                    Err(io::Error::other("retry sync unavailable"))
                }
            )
            .is_err());
        assert!(called);
        assert_eq!(f.journal(), after);
        assert_eq!(
            f.history
                .decide_saved_input(version, SavedInputDecision::Rejected, None, request(1))
                .unwrap(),
            committed
        );
    }
}

#[test]
fn changed_pending_transaction_or_source_is_preserved_without_appending() {
    for mode in [
        "prefix",
        "replacement",
        "intent",
        "source",
        "foreign-suffix",
        "payload",
    ] {
        let f = Fixture::new(&format!("changed-{mode}"), true);
        let version = f.versions[0];
        let before = f.journal();
        assert!(f
            .history
            .decide_with_io(
                version,
                SavedInputDecision::Rejected,
                None,
                request(1),
                |step, _, _| if matches!(step, Step::Staged) {
                    Err(io::Error::other("stop before append"))
                } else {
                    Ok(())
                },
                |file| file.sync_all()
            )
            .is_err());
        let path = f.history.metadata_path().join(RECORD_FILE_NAME);
        let pending = f.history.metadata_path().join(PENDING);
        match mode {
            "prefix" => {
                let mut bytes = before.clone();
                bytes[10] ^= 1;
                fs::write(&path, bytes).unwrap();
            }
            "replacement" => {
                fs::rename(&path, f.root.join("old-journal")).unwrap();
                fs::write(&path, &before).unwrap();
            }
            "intent" => fs::write(&pending, b"unknown pending work").unwrap(),
            "source" => {
                fs::rename(f.root.join("source"), f.root.join("old-source")).unwrap();
                fs::create_dir(f.root.join("source")).unwrap();
            }
            "foreign-suffix" => {
                let mut bytes = before.clone();
                bytes.extend_from_slice(b"unknown work");
                fs::write(&path, bytes).unwrap();
            }
            "payload" => {
                let value = Json::parse(&fs::read_to_string(&pending).unwrap()).unwrap();
                let digest = super::super::dependency_transaction::digest(
                    value.get("payload").unwrap().as_text().unwrap(),
                )
                .unwrap();
                let cas = Cas::<PinnedRootFs, Blake3>::with_filesystem(
                    f.history.metadata_path(),
                    f.history.store.filesystem().read_only(),
                )
                .unwrap();
                fs::write(
                    f.history.metadata_path().join(
                        cas.layout()
                            .chunk_path(&mesh_cas::Digest32::from_bytes(*digest.as_bytes())),
                    ),
                    b"corrupt payload",
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        let observed = fs::read(&path).unwrap();
        let intent = fs::read(&pending).unwrap();
        assert!(
            f.history
                .decide_saved_input(version, SavedInputDecision::Rejected, None, request(1))
                .is_err(),
            "{mode}"
        );
        assert_eq!(fs::read(&path).unwrap(), observed);
        assert_eq!(fs::read(&pending).unwrap(), intent);
    }
}

#[test]
fn last_moment_native_binding_changes_refuse_before_append_and_keep_unknown_intent() {
    for mode in [
        "source-race",
        "fence-race",
        "intent-race",
        "after-append-intent",
    ] {
        let f = Fixture::new(mode, true);
        let before = f.journal();
        let pending = f.history.metadata_path().join(PENDING);
        let result = f.history.decide_with_io(
            f.versions[0],
            SavedInputDecision::Rejected,
            None,
            request(1),
            |step, _, _| {
                if matches!(step, Step::Staged) {
                    match mode {
                        "source-race" => {
                            fs::rename(f.root.join("source"), f.root.join("retained-source"))?;
                            fs::create_dir(f.root.join("source"))?;
                        }
                        "fence-race" => fs::write(
                            f.history
                                .metadata_path()
                                .join(super::super::history::HISTORY),
                            b"unknown history binding",
                        )?,
                        "intent-race" => fs::write(&pending, b"unknown pending intent")?,
                        _ => {}
                    }
                }
                if matches!(step, Step::Appended) && mode == "after-append-intent" {
                    fs::write(&pending, b"unknown pending intent")?;
                }
                Ok(())
            },
            |file| file.sync_all(),
        );
        assert!(result.is_err(), "{mode}");
        if mode == "after-append-intent" {
            assert_eq!(f.journal().len(), before.len() + 145);
        } else {
            assert_eq!(f.journal(), before);
        }
        if mode.contains("intent") {
            assert_eq!(fs::read(&pending).unwrap(), b"unknown pending intent");
        }
    }
}
