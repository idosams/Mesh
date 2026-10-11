//! Actual native publication -> native preparation/stage -> consumption fence/checkpoint.
use super::*;
use crate::project_attachment::consumption_prepare::NativeConsumedCommitPhase as Phase;
use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
const ENV: &str = "MESH_NATIVE_CONSUMPTION_CHECKPOINT_RESTART";
const CHILD: &str = "project_attachment::dependency_publication_replay::tests::native_consumption_checkpoint::consumption_checkpoint_restart_child";
use crate::project_attachment::{
    NativeTrustedConsumedStartRequest, NativeTrustedInputGrantRequest, SavedAttachmentVersion,
};
fn id(n: u8) -> RecordDigest {
    RecordDigest::from_bytes([n; 32])
}
pub(super) fn inspect(
    storage: &AttachmentStorage,
    owner: &ProvisionedAttachment,
    source: &ProvisionedAttachment,
    version: SavedAttachmentVersion,
    trusted: &crate::TrustedReviewers,
    public: &[u8],
    saved_note: &[u8],
) {
    let destination = storage
        .reserve_native_dependency_lane(
            owner,
            source,
            version.operation(),
            id(239),
            &[owner],
            trusted,
        )
        .unwrap();
    let grant = storage
        .grant_native_saved_input(
            owner,
            NativeTrustedInputGrantRequest {
                source,
                operation: version.operation(),
                destination: &destination,
                allowed: true,
                expected_previous: None,
                request: id(240),
            },
            &[owner],
            trusted,
        )
        .unwrap()
        .record();
    fs::create_dir_all(source.project().root().join(".git")).unwrap();
    fs::write(
        source.project().root().join(".git/HEAD"),
        b"ref: refs/heads/existing\n",
    )
    .unwrap();
    fs::write(
        source.project().root().join(".git/index"),
        b"synthetic unchanged index",
    )
    .unwrap();
    fs::write(source.project().root().join("note"), b"later editor bytes").unwrap();
    fs::write(
        source.project().root().join("untracked"),
        b"later untracked work",
    )
    .unwrap();
    let source_before = tree(source.project().root());
    let owner_before = fs::read(owner.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap();
    let source_journal = fs::read(source.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap();
    let key = SigningKey::from_bytes(&[238; 32]);
    let prepared = storage
        .prepare_native_consumed_start(
            owner,
            NativeTrustedConsumedStartRequest {
                source,
                operation: version.operation(),
                destination: &destination,
                grant,
                available: &[owner],
                request: id(241),
                limits: ObservationLimits::default(),
            },
            PublicKey::from_bytes(key.verifying_key().to_bytes()),
            trusted,
            |payload: &SigningPayload| {
                Ok::<_, &'static str>(Signature::from_bytes(
                    key.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .unwrap();
    let staged = prepared.stage(storage).unwrap();
    assert!(prepared
        .try_ordinary_fence_for_test(storage, &staged)
        .is_err());
    let root = owner.project().root().parent().unwrap();
    fs::write(root.join("consumption-checkpoint-public-key"), public).unwrap();
    let child = Json::object([
        ("root", Json::text(root.to_str().unwrap())),
        ("owner", Json::text(owner.id())),
        ("source", Json::text(source.id())),
        ("destination", Json::text(destination.id())),
        ("operation", Json::text(version.operation().to_hex())),
        ("grant", Json::text(grant.to_hex())),
    ]);
    let journal_path = destination.metadata_path().join(crate::RECORD_FILE_NAME);
    let enrollment = fs::read(&journal_path).unwrap();
    // Establish the durable intent without appending a start frame, then all subsequent child
    // reconstructions use only retained authenticated material and never a signer.
    assert!(prepared
        .commit_native_with_io(
            storage,
            &staged,
            Phase::Fence,
            |step, _, _| {
                assert_eq!(step, "staged");
                Err(std::io::Error::other(
                    "fixture stops after durable start intent",
                ))
            },
            |file| file.sync_all()
        )
        .is_err());
    assert_eq!(fs::read(&journal_path).unwrap(), enrollment);
    for mode in [
        "missing-trust",
        "foreign-operation",
        "foreign-grant",
        "foreign-request",
    ] {
        let before = tree(destination.metadata_path());
        run_child(root, child.clone(), mode, "fence", "", 0);
        assert_eq!(tree(destination.metadata_path()), before);
    }
    run_child(root, child.clone(), "exit", "fence", "staged", 86);
    assert_eq!(fs::read(&journal_path).unwrap(), enrollment);
    run_child(root, child.clone(), "exit", "fence", "synced", 86);
    let complete_start = fs::read(&journal_path).unwrap();
    let start_frame = &complete_start[enrollment.len()..];
    assert!(!start_frame.is_empty());
    for boundary in [1, start_frame.len() / 2, start_frame.len() - 1] {
        let mut partial = enrollment.clone();
        partial.extend_from_slice(&start_frame[..boundary]);
        replace_fixture_journal(&journal_path, &partial);
        let start = prepared.fence_consumption_start(storage, &staged).unwrap();
        assert_ne!(start, id(0));
        assert_eq!(fs::read(&journal_path).unwrap(), complete_start);
    }
    let start = prepared.fence_consumption_start(storage, &staged).unwrap();
    // A late editor entry after custody release prevents acknowledgment and stays untouched.
    assert!(prepared
        .commit_native_with_hooks(
            storage,
            &staged,
            Phase::Fence,
            (|_, _, _| Ok(()), |file| file.sync_all()),
            || {
                fs::write(
                    destination.project().root().join("late-editor"),
                    b"preserve this editor work",
                )
            }
        )
        .is_err());
    assert_eq!(
        fs::read(destination.project().root().join("late-editor")).unwrap(),
        b"preserve this editor work"
    );
    fs::remove_file(destination.project().root().join("late-editor")).unwrap();

    assert_eq!(
        prepared.fence_consumption_start(storage, &staged).unwrap(),
        start
    );
    assert!(destination
        .project()
        .root()
        .read_dir()
        .unwrap()
        .next()
        .is_none());
    assert!(destination.saved_versions().is_err());
    let reopened =
        AttachmentStorage::open(&owner.project().root().parent().unwrap().join("metadata"))
            .unwrap();
    let reopened_owner = reopened.reopen(owner.id()).unwrap();
    let reopened_source = reopened.reopen(source.id()).unwrap();
    let reopened_destination = reopened.reopen(destination.id()).unwrap();
    let available = [&reopened_owner];
    let request = || NativeTrustedConsumedStartRequest {
        source: &reopened_source,
        operation: version.operation(),
        destination: &reopened_destination,
        grant,
        available: &available,
        request: id(241),
        limits: ObservationLimits::default(),
    };
    assert!(reopened
        .recover_native_consumed_start(
            &reopened_owner,
            request(),
            &crate::TrustedReviewers::default()
        )
        .is_err());
    let (recovered, recovered_stage) = reopened
        .recover_native_consumed_start(&reopened_owner, request(), trusted)
        .unwrap();
    assert_eq!(recovered.operation(), prepared.operation());
    assert_eq!(
        recovered_stage.receipt().unwrap(),
        staged.receipt().unwrap()
    );
    run_child(
        root,
        child.clone(),
        "exit",
        "install",
        "installed-entry",
        86,
    );
    run_child(root, child.clone(), "exit", "install", "installed", 86);
    assert_eq!(
        prepared
            .install_fenced_consumed_start(storage, &staged)
            .unwrap(),
        start
    );
    assert!(destination.saved_versions().is_err());
    let checkpoint_prefix = fs::read(&journal_path).unwrap();
    run_child(
        root,
        child.clone(),
        "exit",
        "checkpoint",
        "checkpoint-staged",
        86,
    );
    assert_eq!(fs::read(&journal_path).unwrap(), checkpoint_prefix);
    let origin = storage.lane_origin_bound(&destination).unwrap().unwrap();
    let frames = fs::read(
        origin
            .allocation
            .stable_namespace()
            .unwrap()
            .0
            .join(format!("consumption-{}/frames.mesh", id(241).to_hex())),
    )
    .unwrap();
    for boundary in [1, frames.len() / 2, frames.len() - 1] {
        let mut partial = checkpoint_prefix.clone();
        partial.extend_from_slice(&frames[..boundary]);
        replace_fixture_journal(&journal_path, &partial);
        let before = tree(destination.metadata_path());
        let (recovered, recovered_stage) = reopened
            .recover_native_consumed_start(&reopened_owner, request(), trusted)
            .unwrap();
        assert_eq!(tree(destination.metadata_path()), before);
        assert_eq!(
            recovered
                .commit_fenced_consumed_checkpoint(&reopened, &recovered_stage)
                .unwrap(),
            prepared.operation()
        );
        let mut expected = checkpoint_prefix.clone();
        expected.extend_from_slice(&frames);
        assert_eq!(fs::read(&journal_path).unwrap(), expected);
    }
    run_child(
        root,
        child.clone(),
        "exit",
        "checkpoint",
        "checkpoint-synced",
        86,
    );
    run_child(
        root,
        child.clone(),
        "exit",
        "checkpoint",
        "custody-released",
        86,
    );
    assert_eq!(
        prepared
            .commit_fenced_consumed_checkpoint(storage, &staged)
            .unwrap(),
        prepared.operation()
    );
    assert!(destination.saved_versions().is_err());
    assert_eq!(
        fs::read(destination.project().root().join("note")).unwrap(),
        saved_note
    );
    assert!(!destination.project().root().join("untracked").exists());
    assert!(!destination.project().root().join(".git").exists());
    assert_eq!(tree(source.project().root()), source_before);
    assert_eq!(
        fs::read(owner.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap(),
        owner_before
    );
    assert_eq!(
        fs::read(source.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap(),
        source_journal
    );
    let complete = fs::read(&journal_path).unwrap();
    let mut foreign = complete.clone();
    foreign.push(255);
    replace_fixture_journal(&journal_path, &foreign);
    let before = tree(destination.metadata_path());
    assert!(reopened
        .recover_native_consumed_start(&reopened_owner, request(), trusted)
        .is_err());
    assert_eq!(tree(destination.metadata_path()), before);
    replace_fixture_journal(&journal_path, &complete);
    let note = destination.project().root().join("note");
    let original = root.join("consumption-checkpoint-original-note");
    let original_permissions = fs::metadata(&note).unwrap().permissions();
    fs::rename(&note, &original).unwrap();
    fs::write(&note, saved_note).unwrap();
    fs::set_permissions(&note, original_permissions).unwrap();
    let before = tree(destination.project().root());
    assert!(reopened
        .recover_native_consumed_start(&reopened_owner, request(), trusted)
        .is_err());
    assert_eq!(tree(destination.project().root()), before);
    fs::remove_file(&note).unwrap();
    fs::rename(&original, &note).unwrap();
    let (recovered, recovered_stage) = reopened
        .recover_native_consumed_start(&reopened_owner, request(), trusted)
        .unwrap();
    assert_eq!(recovered.operation(), prepared.operation());
    assert_eq!(
        recovered
            .commit_fenced_consumed_checkpoint(&reopened, &recovered_stage)
            .unwrap(),
        prepared.operation()
    );
    assert!(reopened_destination.saved_versions().is_err());
    assert!(prepared
        .commit_native_with_hooks(
            storage,
            &staged,
            Phase::Checkpoint,
            (|_, _, _| Ok(()), |file| file.sync_all()),
            || {
                storage.grant_native_saved_input(
                    owner,
                    NativeTrustedInputGrantRequest {
                        source,
                        operation: version.operation(),
                        destination: &destination,
                        allowed: false,
                        expected_previous: Some(grant),
                        request: id(242),
                    },
                    &[owner],
                    trusted,
                )?;
                Ok(())
            }
        )
        .is_err());
    let before = tree(destination.metadata_path());
    assert!(reopened
        .recover_native_consumed_start(&reopened_owner, request(), trusted)
        .is_err());
    assert_eq!(tree(destination.metadata_path()), before);
    assert_eq!(tree(source.project().root()), source_before);
}

// Synthetic fixture tree, including identities and modes, proves refusal/retry cannot repair it.
type FixtureTree = std::collections::BTreeMap<PathBuf, (u64, u64, u32, Vec<u8>)>;
fn tree(root: &std::path::Path) -> FixtureTree {
    use std::os::unix::fs::MetadataExt as _;
    fn walk(root: &std::path::Path, path: &std::path::Path, out: &mut FixtureTree) {
        let meta = fs::symlink_metadata(path).unwrap();
        assert!(!meta.file_type().is_symlink());
        out.insert(
            path.strip_prefix(root).unwrap().to_path_buf(),
            (
                meta.dev(),
                meta.ino(),
                meta.mode(),
                if meta.is_file() {
                    fs::read(path).unwrap()
                } else {
                    vec![]
                },
            ),
        );
        if meta.is_dir() {
            for child in fs::read_dir(path).unwrap() {
                walk(root, &child.unwrap().path(), out);
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn replace_fixture_journal(path: &Path, bytes: &[u8]) {
    use std::io::Write as _;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}
#[test]
fn consumption_checkpoint_restart_child() {
    let Ok(raw) = std::env::var(ENV) else {
        return;
    };
    let value = Json::parse(&raw).unwrap();
    let text = |key| value.get(key).and_then(Json::as_text).unwrap();
    let root = Path::new(text("root"));
    let storage = AttachmentStorage::open(&root.join("metadata")).unwrap();
    let owner = storage.reopen(text("owner")).unwrap();
    let source = storage.reopen(text("source")).unwrap();
    let destination = storage.reopen(text("destination")).unwrap();
    let public = fs::read(root.join("consumption-checkpoint-public-key")).unwrap();
    let trusted = if text("mode") == "missing-trust" {
        crate::TrustedReviewers::default()
    } else {
        crate::TrustedReviewers::with_human_credentials([HumanApprovalCredential::from_public_key(
            public.as_slice().try_into().unwrap(),
        )
        .unwrap()])
    };
    let available = [&owner];
    let result = storage.recover_native_consumed_start(
        &owner,
        NativeTrustedConsumedStartRequest {
            source: &source,
            operation: if text("mode") == "foreign-operation" {
                id(250)
            } else {
                RecordDigest::parse_hex(text("operation")).unwrap()
            },
            destination: &destination,
            grant: if text("mode") == "foreign-grant" {
                id(250)
            } else {
                RecordDigest::parse_hex(text("grant")).unwrap()
            },
            available: &available,
            request: if text("mode") == "foreign-request" {
                id(250)
            } else {
                id(241)
            },
            limits: ObservationLimits::default(),
        },
        &trusted,
    );
    if text("mode") != "exit" {
        assert!(result.is_err());
        return;
    }
    let (prepared, staged) = result.unwrap();
    let phase = match text("phase") {
        "fence" => Phase::Fence,
        "install" => Phase::Install,
        "checkpoint" => Phase::Checkpoint,
        _ => panic!("unknown fixture phase"),
    };
    prepared
        .commit_native_with_hooks(
            &storage,
            &staged,
            phase,
            (
                |step, _, _| {
                    if step == text("step") {
                        std::process::exit(86);
                    }
                    Ok(())
                },
                |file| file.sync_all(),
            ),
            || {
                if text("step") == "custody-released" {
                    std::process::exit(86);
                }
                Ok(())
            },
        )
        .unwrap();
    panic!("requested actual native boundary was not reached");
}
fn run_child(root: &Path, mut value: Json, mode: &str, phase: &str, step: &str, expected: i32) {
    let Json::Object(fields) = &mut value else {
        panic!("invalid fixture");
    };
    for (key, val) in [("mode", mode), ("phase", phase), ("step", step)] {
        fields.push((key.into(), Json::text(val)));
    }
    let log = root.join(format!("consumption-checkpoint-{mode}-{phase}-{step}.log"));
    let output = fs::File::create(&log).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", CHILD, "--nocapture"])
        .env(ENV, value.encode())
        .stdout(Stdio::from(output.try_clone().unwrap()))
        .stderr(Stdio::from(output))
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
                "native checkpoint child deadline: {}",
                fs::read_to_string(&log).unwrap()
            );
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    assert_eq!(
        status.code(),
        Some(expected),
        "{}",
        fs::read_to_string(log).unwrap()
    );
}
