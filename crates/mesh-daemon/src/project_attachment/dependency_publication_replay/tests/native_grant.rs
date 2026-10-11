//! Actual grant writer and fresh-process recovery after real native publication.
use super::*;
use crate::project_attachment::dependency_private_context::control::Step;
use crate::project_attachment::{NativeTrustedInputGrantRequest, SavedAttachmentVersion};
use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
const ENV: &str = "MESH_NATIVE_GRANT_RESTART_FIXTURE";
const CHILD: &str =
    "project_attachment::dependency_publication_replay::tests::native_grant::grant_restart_child";
fn id(n: u8) -> RecordDigest {
    RecordDigest::from_bytes([n; 32])
}
pub(super) fn prepare(
    storage: &AttachmentStorage,
    owner: &ProvisionedAttachment,
    source: &ProvisionedAttachment,
    version: SavedAttachmentVersion,
) -> ProvisionedAttachment {
    storage
        .reserve_dependency_lane_with_inputs(owner, source, version, id(230), &[owner])
        .unwrap()
}
#[test]
fn grant_restart_child() {
    let Ok(raw) = std::env::var(ENV) else {
        return;
    };
    let v = Json::parse(&raw).unwrap();
    let text = |key| v.get(key).and_then(Json::as_text).unwrap();
    let root = Path::new(text("root"));
    let storage = AttachmentStorage::open(&root.join("metadata")).unwrap();
    let owner = storage.reopen(text("owner")).unwrap();
    let source = storage.reopen(text("source")).unwrap();
    let destination = storage.reopen(text("destination")).unwrap();
    let credential = HumanApprovalCredential::from_public_key(
        fs::read(root.join("grant-restart-public-key"))
            .unwrap()
            .as_slice()
            .try_into()
            .unwrap(),
    )
    .unwrap();
    let mode = text("mode");
    let trust = if mode == "missing-trust" {
        crate::TrustedReviewers::default()
    } else {
        crate::TrustedReviewers::with_human_credentials([credential])
    };
    let previous = RecordDigest::parse_hex(text("previous")).unwrap();
    let result = storage.grant_native_saved_input_with_io(
        &owner,
        NativeTrustedInputGrantRequest {
            source: &source,
            operation: RecordDigest::parse_hex(text("operation")).unwrap(),
            destination: &destination,
            allowed: v.get("allowed").and_then(Json::as_bool).unwrap(),
            expected_previous: Some(previous),
            request: if mode == "foreign-request" {
                id(249)
            } else {
                RecordDigest::parse_hex(text("request")).unwrap()
            },
        },
        &[&owner],
        &trust,
        |step, file, frame| {
            if matches!(step, Step::Staged) {
                if mode == "staged" {
                    std::process::exit(85);
                }
                if matches!(mode, "first" | "last") {
                    let cut = if mode == "first" { 1 } else { frame.len() - 1 };
                    file.write_all(&frame[..cut])?;
                    file.sync_all()?;
                    std::process::exit(86);
                }
            }
            if matches!(step, Step::Appended) && mode == "lost-ack" {
                std::process::exit(87);
            }
            Ok(())
        },
        |file| file.sync_all(),
    );
    if matches!(mode, "missing-trust" | "foreign-request") {
        assert!(result.is_err());
    } else {
        let value = result.unwrap();
        fs::write(
            root.join("grant-restart-result"),
            Json::object([
                ("record", Json::text(value.record().to_hex())),
                ("generation", Json::Number(value.generation())),
            ])
            .encode(),
        )
        .unwrap();
    }
}
fn run_child(root: &Path, mode: &str, mut value: Json, expected: i32) {
    let Json::Object(fields) = &mut value else {
        panic!("invalid child fixture")
    };
    fields.push(("mode".into(), Json::text(mode)));
    let output = root.join(format!("grant-restart-{mode}.log"));
    let stdout = fs::File::create(&output).unwrap();
    let stderr = stdout.try_clone().unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", CHILD, "--nocapture"])
        .env(ENV, value.encode())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!(
                "grant restart {mode} timed out: {}",
                fs::read_to_string(&output).unwrap()
            );
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    assert_eq!(
        status.code(),
        Some(expected),
        "{}",
        fs::read_to_string(output).unwrap()
    );
}
pub(super) fn inspect(
    storage: &AttachmentStorage,
    owner: &ProvisionedAttachment,
    source: &ProvisionedAttachment,
    version: SavedAttachmentVersion,
    destination: &ProvisionedAttachment,
    trusted: &crate::TrustedReviewers,
    public: &[u8],
) {
    let request = |allowed, previous, number| NativeTrustedInputGrantRequest {
        source,
        operation: version.operation(),
        destination,
        allowed,
        expected_previous: previous,
        request: id(number),
    };
    let available = [owner];
    let journal = owner.metadata_path().join(crate::RECORD_FILE_NAME);
    let pending = owner.metadata_path().join("dependency-decision.pending");
    let initial = fs::read(&journal).unwrap();
    let source_journal = source.metadata_path().join(crate::RECORD_FILE_NAME);
    let source_before = fs::read(&source_journal).unwrap();
    let destination_journal = destination.metadata_path().join(crate::RECORD_FILE_NAME);
    let destination_before = fs::read(&destination_journal).unwrap();
    let notes = [
        owner.project().root().join("note"),
        source.project().root().join("note"),
    ]
    .map(|p| {
        let bytes = fs::read(&p).unwrap();
        (p, bytes)
    });
    let git = owner.project().root().join(".git");
    fs::create_dir(&git).unwrap();
    fs::write(git.join("HEAD"), b"ref: refs/heads/keep-user-grants\n").unwrap();
    fs::write(git.join("index"), b"synthetic dirty staged user data").unwrap();
    let untracked = owner.project().root().join("untracked-grant-work");
    fs::write(&untracked, b"preserve untracked editor work").unwrap();
    let preserve = || {
        assert_eq!(fs::read(&destination_journal).unwrap(), destination_before);
        if source.id() != owner.id() {
            assert_eq!(fs::read(&source_journal).unwrap(), source_before);
        }
        for (p, bytes) in &notes {
            assert_eq!(fs::read(p).unwrap(), *bytes);
        }
        assert_eq!(
            fs::read(git.join("HEAD")).unwrap(),
            b"ref: refs/heads/keep-user-grants\n"
        );
        assert_eq!(
            fs::read(git.join("index")).unwrap(),
            b"synthetic dirty staged user data"
        );
        assert_eq!(
            fs::read(&untracked).unwrap(),
            b"preserve untracked editor work"
        );
        assert_eq!(
            fs::read_dir(destination.project().root()).unwrap().count(),
            0
        );
    };
    assert!(storage
        .grant_native_saved_input(
            owner,
            request(true, None, 231),
            &available,
            &crate::TrustedReviewers::default()
        )
        .is_err());
    assert_eq!(fs::read(&journal).unwrap(), initial);
    assert!(!pending.exists());
    preserve();
    let first = storage
        .grant_native_saved_input(owner, request(true, None, 231), &available, trusted)
        .unwrap();
    assert_eq!(first.generation(), 1);
    let committed = fs::read(&journal).unwrap();
    assert!(committed.starts_with(&initial));
    assert!(!pending.exists());
    assert_eq!(
        storage
            .grant_native_saved_input(owner, request(true, None, 231), &available, trusted)
            .unwrap(),
        first
    );
    for wrong in [
        request(false, None, 231),
        request(true, None, 232),
        request(true, Some(id(250)), 232),
        request(true, Some(id(0)), 232),
        request(true, Some(first.record()), 0),
    ] {
        assert!(storage
            .grant_native_saved_input(owner, wrong, &available, trusted)
            .is_err());
        assert_eq!(fs::read(&journal).unwrap(), committed);
        preserve();
    }
    let mut wrong = request(true, Some(first.record()), 232);
    wrong.operation = id(250);
    assert!(storage
        .grant_native_saved_input(owner, wrong, &available, trusted)
        .is_err());
    let mut wrong = request(true, Some(first.record()), 232);
    wrong.destination = source;
    assert!(storage
        .grant_native_saved_input(owner, wrong, &available, trusted)
        .is_err());
    if source.id() != owner.id() {
        assert!(storage
            .grant_native_saved_input(
                source,
                request(true, Some(first.record()), 232),
                &available,
                trusted
            )
            .is_err());
    }
    // A provided pending destination is never accepted as source context.
    assert!(storage
        .grant_native_saved_input(
            owner,
            request(true, Some(first.record()), 232),
            &[destination],
            trusted
        )
        .is_err());
    assert!(storage
        .grant_native_saved_input(
            owner,
            request(true, Some(first.record()), 232),
            &vec![owner; 257],
            trusted
        )
        .is_err());
    assert_eq!(fs::read(&journal).unwrap(), committed);
    preserve();
    let revoked = storage
        .grant_native_saved_input(
            owner,
            request(false, Some(first.record()), 233),
            &available,
            trusted,
        )
        .unwrap();
    assert_eq!(revoked.generation(), 2);
    let renewed = storage
        .grant_native_saved_input(
            owner,
            request(true, Some(revoked.record()), 234),
            &available,
            trusted,
        )
        .unwrap();
    assert_eq!(renewed.generation(), 3);
    let after = fs::read(&journal).unwrap();
    assert_eq!(
        storage
            .grant_native_saved_input(owner, request(true, None, 231), &available, trusted)
            .unwrap(),
        first
    );
    assert_eq!(
        storage
            .grant_native_saved_input(
                owner,
                request(false, Some(first.record()), 233),
                &available,
                trusted
            )
            .unwrap(),
        revoked
    );
    assert_eq!(fs::read(&journal).unwrap(), after);
    preserve();
    let root = owner.project().root().parent().unwrap();
    fs::write(root.join("grant-restart-public-key"), public).unwrap();
    let mut previous = renewed.record();
    for (index, cut) in ["first", "last"].into_iter().enumerate() {
        let generation = 4 + index as u64;
        let value = Json::object([
            ("root", Json::text(root.to_string_lossy())),
            ("owner", Json::text(owner.id())),
            ("source", Json::text(source.id())),
            ("destination", Json::text(destination.id())),
            ("operation", Json::text(version.operation().to_hex())),
            ("previous", Json::text(previous.to_hex())),
            ("request", Json::text(id(240 + index as u8).to_hex())),
            ("allowed", Json::Bool(index != 0)),
        ]);
        let before = fs::read(&journal).unwrap();
        run_child(root, "staged", value.clone(), 85);
        assert_eq!(fs::read(&journal).unwrap(), before);
        let raw = fs::read(&pending).unwrap();
        assert!(storage
            .inspect_native_history_retention_during_control(owner, id(240 + index as u8), trusted)
            .is_err());
        assert_eq!(fs::read(&journal).unwrap(), before);
        assert_eq!(fs::read(&pending).unwrap(), raw);
        let mut noncanonical = raw.clone();
        noncanonical.push(b'\n');
        fs::write(&pending, &noncanonical).unwrap();
        assert!(storage
            .grant_native_saved_input(
                owner,
                request(index != 0, Some(previous), 240 + index as u8),
                &available,
                trusted
            )
            .is_err());
        assert_eq!(fs::read(&pending).unwrap(), noncanonical);
        assert_eq!(fs::read(&journal).unwrap(), before);
        fs::write(&pending, &raw).unwrap();
        let intent = Json::parse(std::str::from_utf8(&raw).unwrap()).unwrap();
        let payload =
            RecordDigest::parse_hex(intent.get("payload").and_then(Json::as_text).unwrap())
                .unwrap();
        let cas = Cas::<_, Blake3>::with_filesystem(
            owner.metadata_path(),
            owner.store.filesystem().read_only(),
        )
        .unwrap();
        let payload_path = owner.metadata_path().join(
            cas.layout()
                .chunk_path(&mesh_cas::Digest32::from_bytes(*payload.as_bytes())),
        );
        let backup = payload_path.with_extension("grant-retained-backup");
        fs::rename(&payload_path, &backup).unwrap();
        assert!(storage
            .grant_native_saved_input(
                owner,
                request(index != 0, Some(previous), 240 + index as u8),
                &available,
                trusted
            )
            .is_err());
        assert!(!payload_path.exists());
        assert_eq!(fs::read(&journal).unwrap(), before);
        assert_eq!(fs::read(&pending).unwrap(), raw);
        fs::rename(&backup, &payload_path).unwrap();
        for mode in ["missing-trust", "foreign-request"] {
            run_child(root, mode, value.clone(), 0);
            assert_eq!(fs::read(&journal).unwrap(), before);
            assert_eq!(fs::read(&pending).unwrap(), raw);
            preserve();
        }
        run_child(root, cut, value.clone(), 86);
        let torn = fs::read(&journal).unwrap();
        assert!(torn.starts_with(&before) && torn.len() > before.len());
        assert!(storage
            .inspect_native_publication_history(source.id(), trusted)
            .is_err());
        for mode in ["missing-trust", "foreign-request"] {
            run_child(root, mode, value.clone(), 0);
            assert_eq!(fs::read(&journal).unwrap(), torn);
            assert_eq!(fs::read(&pending).unwrap(), raw);
            preserve();
        }
        run_child(root, "lost-ack", value.clone(), 87);
        let complete = fs::read(&journal).unwrap();
        assert!(complete.starts_with(&torn));
        assert!(complete.len() > torn.len());
        assert_eq!(fs::read(&pending).unwrap(), raw);
        run_child(root, "retry", value.clone(), 0);
        assert_eq!(fs::read(&journal).unwrap(), complete);
        assert!(!pending.exists());
        let result =
            Json::parse(&fs::read_to_string(root.join("grant-restart-result")).unwrap()).unwrap();
        assert_eq!(result.get("generation"), Some(&Json::Number(generation)));
        previous =
            RecordDigest::parse_hex(result.get("record").and_then(Json::as_text).unwrap()).unwrap();
        run_child(root, "retry", value, 0);
        assert_eq!(fs::read(&journal).unwrap(), complete);
        preserve();
    }
    // A fully written frame with a failed durability acknowledgment must be retried exactly.
    let failed = storage
        .grant_native_saved_input_with_io(
            owner,
            request(false, Some(previous), 245),
            &available,
            trusted,
            |_, _, _| Ok(()),
            |_| Err(std::io::Error::other("test grant sync failed")),
        )
        .unwrap_err();
    assert!(failed.to_string().contains("test grant sync failed"));
    assert!(pending.exists());
    let complete = fs::read(&journal).unwrap();
    let last = storage
        .grant_native_saved_input(
            owner,
            request(false, Some(previous), 245),
            &available,
            trusted,
        )
        .unwrap();
    assert_eq!(last.generation(), 6);
    assert_eq!(fs::read(&journal).unwrap(), complete);
    assert!(!pending.exists());
    assert_eq!(
        storage
            .grant_native_saved_input(owner, request(true, None, 231), &available, trusted)
            .unwrap(),
        first
    );
    assert_eq!(fs::read(&journal).unwrap(), complete);
    preserve();
    let destination_path = destination.project().root().to_owned();
    let original = destination_path.with_extension("grant-original");
    let foreign = destination_path.with_extension("grant-foreign");
    let before_swap = fs::read(&journal).unwrap();
    let failed = storage.grant_native_saved_input_with_io(
        owner,
        request(true, Some(last.record()), 246),
        &available,
        trusted,
        |step, _, _| {
            if matches!(step, Step::Staged) {
                fs::rename(&destination_path, &original)?;
                fs::create_dir(&destination_path)?;
                fs::write(
                    destination_path.join("foreign-editor-work"),
                    b"preserve substituted destination",
                )?;
            }
            Ok(())
        },
        |file| file.sync_all(),
    );
    assert!(failed.is_err());
    assert_eq!(fs::read(&journal).unwrap(), before_swap);
    assert!(pending.exists());
    assert_eq!(
        fs::read(destination_path.join("foreign-editor-work")).unwrap(),
        b"preserve substituted destination"
    );
    fs::rename(&destination_path, &foreign).unwrap();
    fs::rename(&original, &destination_path).unwrap();
    let recovered = storage
        .grant_native_saved_input(
            owner,
            request(true, Some(last.record()), 246),
            &available,
            trusted,
        )
        .unwrap();
    assert_eq!(recovered.generation(), 7);
    assert_eq!(
        fs::read(foreign.join("foreign-editor-work")).unwrap(),
        b"preserve substituted destination"
    );
    let complete = fs::read(&journal).unwrap();
    assert!(!pending.exists());
    preserve();
    assert!(owner.saved_versions().is_err());
    assert!(storage
        .prepare_consumed_start(
            owner,
            crate::project_attachment::NativeConsumedStartRequest {
                input: crate::project_attachment::NativeGrantInspection {
                    source,
                    version,
                    destination,
                    grant: first.record()
                },
                available: &available,
                request: id(248),
                limits: ObservationLimits::default()
            },
            PublicKey::from_bytes([1; 32]),
            |_| Ok::<_, &'static str>(Signature::from_bytes([1; 64]))
        )
        .is_err());
    assert_eq!(fs::read(&journal).unwrap(), complete);
    preserve();
}
