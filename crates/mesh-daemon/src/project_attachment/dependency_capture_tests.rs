use super::*;
use crate::project_attachment::{AttachmentStorage, ObservationLimits, SavedInputDecision};
use ed25519_dalek::{Signer as _, SigningKey};
struct Fixture {
    root: PathBuf,
    a: ProvisionedAttachment,
    key: SigningKey,
    initial: SavedAttachmentVersion,
}
impl Fixture {
    fn new(name: &str, enrolled: bool) -> Self {
        let root =
            std::env::temp_dir().join(format!("mesh-native-capture-{name}-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("source")).unwrap();
        fs::create_dir(root.join("metadata")).unwrap();
        fs::write(root.join("source/note"), b"initial").unwrap();
        let storage = AttachmentStorage::open(&root.join("metadata")).unwrap();
        let a = storage.provision(&root.join("source")).unwrap();
        let key = SigningKey::from_bytes(&[69; 32]);
        let input = a
            .project()
            .capture_inputs(ObservationLimits::default())
            .unwrap();
        let initial = a
            .project()
            .save_capture(a.metadata_path(), &input, public(&key), |p| sign(&key, p))
            .unwrap();
        if enrolled {
            a.enroll_dependency_history().unwrap();
        }
        Self {
            root,
            a,
            key,
            initial,
        }
    }
    fn input(&self, bytes: &[u8]) -> CapturedProjectInput {
        fs::write(self.a.project().root().join("note"), bytes).unwrap();
        self.a
            .project()
            .capture_inputs(ObservationLimits::default())
            .unwrap()
    }
    fn journal(&self) -> Vec<u8> {
        fs::read(self.a.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap()
    }
    fn prepare(&self, input: &CapturedProjectInput, n: u8) -> io::Result<PreparedNativeCapture> {
        self.a
            .prepare_dependency_capture(input, public(&self.key), id(n), |p| sign(&self.key, p))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn id(n: u8) -> RecordDigest {
    RecordDigest::from_bytes([n; 32])
}
fn public(key: &SigningKey) -> PublicKey {
    PublicKey::from_bytes(key.verifying_key().to_bytes())
}
fn sign(key: &SigningKey, p: &SigningPayload) -> io::Result<Signature> {
    Ok(Signature::from_bytes(key.sign(p.as_bytes()).to_bytes()))
}
#[test]
fn enrolled_capture_saves_exact_snapshot_and_keeps_legacy_writers_fenced() {
    let f = Fixture::new("roundtrip", true);
    let input = f.input(b"saved while enrolled");
    let before = f.journal();
    let prepared = f.prepare(&input, 1).unwrap();
    let exact = prepared.operation();
    assert_eq!(f.journal(), before);
    fs::write(f.a.project().root().join("note"), b"later editor bytes").unwrap();
    let saved = prepared.commit().unwrap();
    assert_eq!(saved.operation(), exact);
    assert_eq!(
        fs::read(f.a.project().root().join("note")).unwrap(),
        b"later editor bytes"
    );
    assert_eq!(
        f.a.project()
            .saved_file(f.a.metadata_path(), saved, "note")
            .unwrap()
            .unwrap(),
        b"saved while enrolled"
    );
    assert!(
        OpenWorkspace::open_attachment_store(f.a.metadata_path(), f.a.store.clone(), false)
            .is_err()
    );
    assert!(f
        .a
        .project()
        .save_capture(f.a.metadata_path(), &input, public(&f.key), |p| sign(
            &f.key, p
        ))
        .is_err());
    let second = f
        .prepare(&f.input(b"next private progress"), 2)
        .unwrap()
        .commit()
        .unwrap();
    assert_ne!(saved, second);
    assert_eq!(
        f.a.project()
            .saved_versions(f.a.metadata_path())
            .unwrap()
            .len(),
        3
    );
    assert!(!f.a.metadata_path().join(PENDING).exists());
}
#[test]
fn signer_has_no_custody_and_policy_activity_does_not_block_private_capture() {
    let f = Fixture::new("signer-race", true);
    let input = f.input(b"prepared version");
    let prepared =
        f.a.prepare_dependency_capture(&input, public(&f.key), id(1), |p| {
            let root = f.a.store.clone();
            assert!(std::thread::spawn(move || {
                crate::workspace_custody::lock_workspace_initialization(&root).is_ok()
            })
            .join()
            .unwrap());
            f.a.decide_saved_input(f.initial, SavedInputDecision::Rejected, None, id(2))
                .unwrap();
            sign(&f.key, p)
        })
        .unwrap();
    let after_policy = f.journal();
    let saved = prepared.commit().unwrap();
    assert!(f.journal().starts_with(&after_policy));
    assert_eq!(
        f.a.project()
            .saved_file(f.a.metadata_path(), saved, "note")
            .unwrap()
            .unwrap(),
        b"prepared version"
    );
    assert!(!f.a.metadata_path().join(PENDING).exists());
}
#[test]
fn competing_preparations_cannot_overwrite_the_committed_capture_basis() {
    let f = Fixture::new("competing", true);
    let first = f.prepare(&f.input(b"first candidate"), 1).unwrap();
    let second = f.prepare(&f.input(b"second candidate"), 2).unwrap();
    first.commit().unwrap();
    let before = f.journal();
    assert!(second.commit().is_err());
    assert_eq!(f.journal(), before);
    assert_eq!(
        fs::read(f.a.project().root().join("note")).unwrap(),
        b"second candidate"
    );
}
#[test]
fn unenrolled_and_pending_native_state_refuse_without_signing_or_journal_changes() {
    let legacy = Fixture::new("legacy", false);
    let input = legacy.input(b"new");
    assert!(legacy
        .a
        .prepare_dependency_capture(
            &input,
            public(&legacy.key),
            id(1),
            |_| -> io::Result<Signature> { panic!("signer must not run") }
        )
        .is_err());
    let f = Fixture::new("pending", true);
    let input = f.input(b"new");
    let before = f.journal();
    f.a.store
        .filesystem()
        .write_new_file(
            Path::new(PENDING),
            b"unknown retained evidence",
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
    assert!(f
        .a
        .prepare_dependency_capture(
            &input,
            public(&f.key),
            id(1),
            |_| -> io::Result<Signature> { panic!("signer must not run") }
        )
        .is_err());
    assert_eq!(f.journal(), before);
    assert_eq!(
        fs::read(f.a.metadata_path().join(PENDING)).unwrap(),
        b"unknown retained evidence"
    );
}

#[test]
fn every_capture_frame_prefix_recovers_one_exact_saved_operation() {
    let f = Fixture::new("every-prefix", true);
    let before = f.journal();
    let input = f.input(b"fixed private capture");
    let prepared = f.prepare(&input, 1).unwrap();
    let expected = prepared.operation();
    let mut frames = Vec::new();
    assert!(prepared
        .commit_with_io(
            |step, _, bytes| {
                if matches!(step, CaptureStep::Staged) {
                    frames = bytes.to_vec();
                    return Err(io::Error::other("interrupted"));
                }
                Ok(())
            },
            |file| file.sync_all()
        )
        .is_err());
    let pending = fs::read(f.a.metadata_path().join(PENDING)).unwrap();
    let capture_line = f.a.metadata_path().join("attachment-capture-line.json");
    let line = fs::read(&capture_line).unwrap();
    let receipt = f.a.metadata_path().join(receipt_name(id(1)));
    let journal = f.a.metadata_path().join(crate::RECORD_FILE_NAME);
    let mut completed = before.clone();
    completed.extend_from_slice(&frames);
    fs::write(f.a.project().root().join("note"), b"later user edits").unwrap();
    for length in 0..=frames.len() {
        let mut partial = before.clone();
        partial.extend_from_slice(&frames[..length]);
        fs::write(&journal, &partial).unwrap();
        fs::write(f.a.metadata_path().join(PENDING), &pending).unwrap();
        fs::set_permissions(
            f.a.metadata_path().join(PENDING),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        fs::write(&capture_line, &line).unwrap();
        if receipt.exists() {
            fs::remove_file(&receipt).unwrap();
        }
        if mesh_store::scan_journal(&partial)
            .unwrap()
            .tail()
            .is_fragment()
        {
            assert!(f.a.project().saved_versions(f.a.metadata_path()).is_err());
        }
        let retained =
            f.a.inspect_dependency_capture_retention(id(1))
                .unwrap_or_else(|e| panic!("retention prefix {length}: {e}"));
        assert_eq!(retained.operation(), expected);
        assert!(retained.pending());
        assert_eq!(
            f.journal(),
            partial,
            "retention inspection altered prefix {length}"
        );
        let saved =
            f.a.recover_dependency_capture(id(1))
                .unwrap_or_else(|e| panic!("prefix {length}: {e}"));
        assert_eq!(saved.operation(), expected, "prefix {length}");
        assert_eq!(f.journal(), completed, "prefix {length}");
        assert_eq!(
            f.a.project()
                .saved_versions(f.a.metadata_path())
                .unwrap()
                .len(),
            2
        );
        assert!(!f.a.metadata_path().join(PENDING).exists());
    }
    assert_eq!(
        fs::read(f.a.project().root().join("note")).unwrap(),
        b"later user edits"
    );
    assert_eq!(
        f.a.project()
            .saved_file(
                f.a.metadata_path(),
                SavedAttachmentVersion {
                    operation: expected
                },
                "note"
            )
            .unwrap()
            .unwrap(),
        b"fixed private capture"
    );
}

#[test]
fn lost_acknowledgement_and_historical_request_recovery_never_rewind_or_duplicate() {
    let f = Fixture::new("lost-ack", true);
    let prepared = f.prepare(&f.input(b"first retained capture"), 1).unwrap();
    let expected = prepared.operation();
    assert!(prepared
        .commit_with_io(
            |step, _, _| {
                if matches!(step, CaptureStep::Appended) {
                    Err(io::Error::other("lost acknowledgement"))
                } else {
                    Ok(())
                }
            },
            |file| file.sync_all()
        )
        .is_err());
    let before = f.journal();
    assert!(f
        .a
        .decide_saved_input(f.initial, SavedInputDecision::Rejected, None, id(8))
        .is_err());
    assert_eq!(f.journal(), before);
    assert_eq!(
        f.a.recover_dependency_capture(id(1)).unwrap().operation(),
        expected
    );
    assert_eq!(f.journal(), before);
    let next = f
        .prepare(&f.input(b"later progress"), 2)
        .unwrap()
        .commit()
        .unwrap();
    let later = f.journal();
    assert_eq!(
        f.a.recover_dependency_capture(id(1)).unwrap().operation(),
        expected
    );
    assert_eq!(f.journal(), later);
    assert_eq!(
        f.a.project()
            .saved_versions(f.a.metadata_path())
            .unwrap()
            .last(),
        Some(&next)
    );
    assert!(f.prepare(&f.input(b"conflicting reuse"), 1).is_err());
    assert_eq!(f.journal(), later);
}

#[test]
fn failed_capture_sync_and_failed_recovery_sync_cannot_acknowledge() {
    let f = Fixture::new("sync", true);
    let prepared = f.prepare(&f.input(b"sync-bound capture"), 1).unwrap();
    let expected = prepared.operation();
    assert!(prepared
        .commit_with_io(|_, _, _| Ok(()), |_| Err(io::Error::other("sync failed")))
        .is_err());
    assert!(f.a.metadata_path().join(PENDING).exists());
    assert!(f
        .a
        .recover_capture_with_sync(id(1), |_| Err(io::Error::other("recovery sync failed")))
        .is_err());
    assert!(f.a.metadata_path().join(PENDING).exists());
    assert_eq!(
        f.a.recover_dependency_capture(id(1)).unwrap().operation(),
        expected
    );
    assert!(!f.a.metadata_path().join(PENDING).exists());
}

#[test]
fn foreign_suffix_and_wrong_request_recovery_preserve_all_evidence() {
    let f = Fixture::new("foreign-tail", true);
    let prepared = f.prepare(&f.input(b"retained capture"), 1).unwrap();
    assert!(prepared
        .commit_with_io(
            |step, journal, frames| {
                if matches!(step, CaptureStep::Staged) {
                    journal.write_all(&[frames[0] ^ 0xff])?;
                    journal.sync_all()?;
                    return Err(io::Error::other("foreign suffix"));
                }
                Ok(())
            },
            |file| file.sync_all()
        )
        .is_err());
    let before = f.journal();
    let pending = fs::read(f.a.metadata_path().join(PENDING)).unwrap();
    assert!(f.a.recover_dependency_capture(id(2)).is_err());
    assert!(f.a.recover_dependency_capture(id(1)).is_err());
    assert_eq!(f.journal(), before);
    assert_eq!(
        fs::read(f.a.metadata_path().join(PENDING)).unwrap(),
        pending
    );
}

#[test]
fn replaced_source_refuses_recovery_and_original_identity_can_resume() {
    let f = Fixture::new("replacement", true);
    let prepared = f.prepare(&f.input(b"retained original input"), 1).unwrap();
    let expected = prepared.operation();
    assert!(prepared
        .commit_with_io(
            |step, _, _| {
                if matches!(step, CaptureStep::Staged) {
                    Err(io::Error::other("interrupted before append"))
                } else {
                    Ok(())
                }
            },
            |file| file.sync_all()
        )
        .is_err());
    let source = f.a.project().root().to_path_buf();
    let displaced = f.root.join("displaced-original");
    fs::rename(&source, &displaced).unwrap();
    fs::create_dir(&source).unwrap();
    fs::write(source.join("note"), b"replacement folder").unwrap();
    let before = f.journal();
    let pending = fs::read(f.a.metadata_path().join(PENDING)).unwrap();
    assert!(f.a.recover_dependency_capture(id(1)).is_err());
    assert_eq!(f.journal(), before);
    assert_eq!(
        fs::read(f.a.metadata_path().join(PENDING)).unwrap(),
        pending
    );
    assert_eq!(
        fs::read(source.join("note")).unwrap(),
        b"replacement folder"
    );
    fs::remove_dir_all(&source).unwrap();
    fs::rename(&displaced, &source).unwrap();
    assert_eq!(
        f.a.recover_dependency_capture(id(1)).unwrap().operation(),
        expected
    );
}

#[test]
fn restoring_an_old_capture_line_cannot_reuse_an_advanced_actor_basis() {
    let f = Fixture::new("actor-basis", true);
    let line = f.a.metadata_path().join("attachment-capture-line.json");
    let original = fs::read(&line).unwrap();
    let first = f.prepare(&f.input(b"first signed candidate"), 1).unwrap();
    let second = f.prepare(&f.input(b"second signed candidate"), 2).unwrap();
    first.commit().unwrap();
    let before = f.journal();
    fs::write(&line, original).unwrap();
    assert!(second.commit().is_err());
    assert_eq!(f.journal(), before);
    assert!(!f.a.metadata_path().join(PENDING).exists());
}

#[test]
fn capture_retention_reads_staged_prefixes_without_recovery_and_keeps_completed_roots() {
    let f = Fixture::new("retention-prefixes", true);
    let before = f.journal();
    let input = f.input(b"fixed staged capture");
    let prepared = f.prepare(&input, 91).unwrap();
    let expected = prepared.operation();
    let mut frames = Vec::new();
    assert!(prepared
        .commit_with_io(
            |step, _, bytes| {
                if matches!(step, CaptureStep::Staged) {
                    frames = bytes.to_vec();
                    return Err(io::Error::other("stopped after staging"));
                }
                Ok(())
            },
            |file| file.sync_all()
        )
        .is_err());
    let pending = fs::read(f.a.metadata_path().join(PENDING)).unwrap();
    let line_path = f.a.metadata_path().join("attachment-capture-line.json");
    let line = fs::read(&line_path).unwrap();
    let journal_path = f.a.metadata_path().join(crate::RECORD_FILE_NAME);
    fs::write(f.a.project().root().join("note"), b"newer editor work").unwrap();
    let required = [
        hash(&frames),
        expected,
        hash(b"fixed staged capture"),
        hash(b"initial"),
    ];
    for length in [0, 1, frames.len() / 2, frames.len() - 1, frames.len()] {
        let mut partial = before.clone();
        partial.extend_from_slice(&frames[..length]);
        fs::write(&journal_path, &partial).unwrap();
        let facts =
            f.a.inspect_dependency_capture_retention(id(91))
                .unwrap_or_else(|e| panic!("prefix {length}: {e}"));
        assert!(facts.pending());
        assert_eq!(facts.operation(), expected);
        let value = facts.to_json();
        let Json::Array(roots) = value.get("payloads").unwrap() else {
            panic!("roots absent")
        };
        for digest in required {
            assert!(roots.contains(&Json::text(digest.to_hex())));
        }
        assert_eq!(
            f.journal(),
            partial,
            "read-only retention must not finish a journal prefix"
        );
        assert_eq!(
            fs::read(f.a.metadata_path().join(PENDING)).unwrap(),
            pending
        );
        assert_eq!(fs::read(&line_path).unwrap(), line);
        assert!(f.a.inspect_dependency_capture_retention(id(92)).is_err());
    }
    f.a.recover_dependency_capture(id(91)).unwrap();
    let complete = f.a.inspect_dependency_capture_retention(id(91)).unwrap();
    assert!(!complete.pending());
    assert_eq!(complete.operation(), expected);
    let cas =
        Cas::<_, mesh_cas::Blake3>::with_filesystem(f.a.metadata_path(), f.a.store.filesystem())
            .unwrap();
    let path = f.a.metadata_path().join(
        cas.layout()
            .chunk_path(&mesh_cas::Digest32::from_bytes(*hash(&frames).as_bytes())),
    );
    let exact = fs::read(&path).unwrap();
    fs::write(&path, b"corrupt retained frames").unwrap();
    assert!(f.a.inspect_dependency_capture_retention(id(91)).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"corrupt retained frames");
    fs::write(&path, exact).unwrap();
    assert_eq!(
        complete,
        f.a.inspect_dependency_capture_retention(id(91)).unwrap()
    );
    let later = f
        .prepare(&f.input(b"later separately saved work"), 92)
        .unwrap()
        .commit()
        .unwrap();
    let historical = f.a.inspect_dependency_capture_retention(id(91)).unwrap();
    assert!(!historical.pending());
    assert_eq!(historical.operation(), expected);
    let value = historical.to_json();
    let Json::Array(roots) = value.get("payloads").unwrap() else {
        panic!("roots absent")
    };
    for digest in required {
        assert!(roots.contains(&Json::text(digest.to_hex())));
    }
    assert!(roots.contains(&Json::text(later.operation().to_hex())));
    fs::write(f.a.project().root().join("note"), b"newer editor work").unwrap();

    assert_eq!(
        fs::read(f.a.project().root().join("note")).unwrap(),
        b"newer editor work"
    );
}

#[test]
fn canonical_consumption_records_alone_cannot_admit_native_capture_or_control() {
    use mesh_store::{DependencyKind, DependencyRecord, StoredRecord};
    let f = Fixture::new("consumption-record-fence", true);
    let input = f.input(b"unexpected editor work remains untouched");
    let (binding, mut policy) = {
        let _guard = crate::workspace_custody::lock_workspace_initialization(&f.a.store).unwrap();
        let (_, proof) =
            f.a.project()
                .read_configuration(f.a.metadata_path(), &f.a.store)
                .unwrap();
        let proof = proof.unwrap();
        (proof.binding(), proof.policy().clone())
    };
    let j = |n| Json::text(id(n).to_hex());
    let mut start: Option<String> = None;
    for kind in [
        DependencyKind::ConsumptionStart,
        DependencyKind::ConsumptionComplete,
    ] {
        let (revision, previous) = policy.native_head().unwrap();
        let body = if kind == DependencyKind::ConsumptionStart {
            Json::object([
                ("request", j(60)),
                ("owner", Json::Array(vec![j(70), j(71), j(72)])),
                (
                    "destination",
                    Json::Array(vec![j(20), Json::text(binding.installation.to_hex())]),
                ),
                (
                    "source",
                    Json::Array(vec![Json::Array(vec![j(10), j(11)]), j(12)]),
                ),
                ("grant", j(61)),
                ("bindings", Json::Array(vec![j(62), j(63)])),
                ("configuration", j(64)),
                ("prospective", j(65)),
                ("closure", j(66)),
                ("operation", j(67)),
                ("staged", j(68)),
            ])
        } else {
            Json::object([
                ("start", Json::text(start.as_deref().unwrap())),
                ("owner_receipt", j(69)),
            ])
        };
        let bytes = Json::object([
            ("schema", Json::text("mesh.dependency-policy/v1")),
            ("authority", Json::text(binding.authority.to_hex())),
            ("revision", Json::Number(revision + 1)),
            ("previous", Json::text(previous.to_hex())),
            ("kind", Json::Number(u64::from(kind.code()))),
            ("body", body),
        ])
        .encode()
        .into_bytes();
        let record = DependencyRecord {
            authority: binding.authority,
            revision: revision + 1,
            previous,
            payload: hash(&bytes),
            kind,
        };
        // Both records are valid canonical projections. Neither proves staged content,
        // owner authority, cross-store acknowledgement, or a valid initial operation.
        policy.apply(record, &bytes).unwrap();
        if kind == DependencyKind::ConsumptionStart {
            start = Some(record.payload.to_hex());
        }
        {
            let _guard =
                crate::workspace_custody::lock_workspace_initialization(&f.a.store).unwrap();
            let cas = Cas::<_, mesh_cas::Blake3>::with_filesystem(
                f.a.metadata_path(),
                f.a.store.filesystem(),
            )
            .unwrap();
            cas.promote(bytes.clone()).unwrap();
            let mut journal = fs::OpenOptions::new()
                .append(true)
                .open(f.a.metadata_path().join(crate::RECORD_FILE_NAME))
                .unwrap();
            journal
                .write_all(&mesh_store::frame_record(&StoredRecord::Dependency(record)))
                .unwrap();
            journal.sync_all().unwrap();
        }
        let before = f.journal();
        let called = std::cell::Cell::new(false);
        assert!(f
            .a
            .prepare_dependency_capture(&input, public(&f.key), id(80), |p| {
                called.set(true);
                sign(&f.key, p)
            })
            .is_err());
        assert!(
            !called.get(),
            "unverified consumption must refuse before signing"
        );
        assert!(f
            .a
            .decide_saved_input(f.initial, SavedInputDecision::Rejected, None, id(81))
            .is_err());
        assert_eq!(f.journal(), before);
        assert_eq!(
            fs::read(f.a.project().root().join("note")).unwrap(),
            b"unexpected editor work remains untouched"
        );
        let cas = Cas::<_, mesh_cas::Blake3>::with_filesystem(
            f.a.metadata_path(),
            f.a.store.filesystem().read_only(),
        )
        .unwrap();
        assert_eq!(read_payload(&cas, record.payload, 65_536).unwrap(), bytes);
    }
}
