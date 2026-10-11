//! Real native checkpoint -> owner receipt, fresh process retries and historical permission split.
use super::*;
use crate::project_attachment::{
    NativeTrustedConsumedStartRequest, NativeTrustedInputGrantRequest,
    PreparedNativeTrustedConsumedStart, SavedAttachmentVersion, StagedNativeConsumedStart,
};
use std::{
    os::unix::fs::MetadataExt as _,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
const ENV: &str = "MESH_NATIVE_CONSUMPTION_OWNER_RESTART";
const CHILD: &str = "project_attachment::dependency_publication_replay::tests::native_consumption_owner::consumption_owner_restart_child";
fn id(n: u8) -> RecordDigest {
    RecordDigest::from_bytes([n; 32])
}
fn prepare_checkpoint(
    storage: &AttachmentStorage,
    owner: &ProvisionedAttachment,
    source: &ProvisionedAttachment,
    version: SavedAttachmentVersion,
    trusted: &crate::TrustedReviewers,
    ids: (u8, u8, u8),
) -> (
    ProvisionedAttachment,
    RecordDigest,
    PreparedNativeTrustedConsumedStart,
    StagedNativeConsumedStart,
) {
    let destination = storage
        .reserve_native_dependency_lane(
            owner,
            source,
            version.operation(),
            id(ids.0),
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
                request: id(ids.1),
            },
            &[owner],
            trusted,
        )
        .unwrap()
        .record();
    let key = SigningKey::from_bytes(&[244; 32]);
    let prepared = storage
        .prepare_native_consumed_start(
            owner,
            NativeTrustedConsumedStartRequest {
                source,
                operation: version.operation(),
                destination: &destination,
                grant,
                available: &[owner],
                request: id(ids.2),
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
    prepared.fence_consumption_start(storage, &staged).unwrap();
    prepared
        .install_fenced_consumed_start(storage, &staged)
        .unwrap();
    prepared
        .commit_fenced_consumed_checkpoint(storage, &staged)
        .unwrap();
    assert!(destination.saved_versions().is_err());
    (destination, grant, prepared, staged)
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
    let root = owner.project().root().parent().unwrap();
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
    fs::write(source.project().root().join("note"), b"newer editor work").unwrap();
    fs::write(
        source.project().root().join("untracked"),
        b"newer untracked work",
    )
    .unwrap();
    let source_before = tree(source.project().root());
    let source_journal = fs::read(source.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap();
    let (destination, grant, prepared, staged) =
        prepare_checkpoint(storage, owner, source, version, trusted, (245, 246, 247));
    assert!(prepared
        .try_ordinary_owner_for_test(storage, &staged)
        .is_err());
    let available = [owner];
    let request = || NativeTrustedConsumedStartRequest {
        source,
        operation: version.operation(),
        destination: &destination,
        grant,
        available: &available,
        request: id(247),
        limits: ObservationLimits::default(),
    };
    let owner_path = owner.metadata_path().join(crate::RECORD_FILE_NAME);
    let before = fs::read(&owner_path).unwrap();
    let pending_saved = root.join("original-owner-pending");
    let mut substituted = None;
    // The first new sidecar is pinned before the hook. Same-byte/mode replacement must not append.
    assert!(storage
        .commit_native_consumed_owner_with_hooks(
            owner,
            &request(),
            trusted,
            (
                |step, _, _| {
                    assert_eq!(step, "owner-staged");
                    let pending = attempt(owner, id(247));
                    let bytes = fs::read(&pending)?;
                    let permissions = fs::metadata(&pending)?.permissions();
                    fs::rename(&pending, &pending_saved)?;
                    fs::write(&pending, &bytes)?;
                    fs::set_permissions(&pending, permissions)?;
                    substituted = Some(tree(owner.metadata_path()));
                    Ok(())
                },
                |file| file.sync_all()
            ),
            || Ok(())
        )
        .is_err());
    assert_eq!(fs::read(&owner_path).unwrap(), before);
    assert_eq!(tree(owner.metadata_path()), substituted.take().unwrap());
    let pending = attempt(owner, id(247));
    fs::remove_file(&pending).unwrap();
    fs::rename(&pending_saved, &pending).unwrap();
    // Ambiguous and damaged retained evidence must refuse without repairing either copy.
    let ambiguous = owner.metadata_path().join(format!(
        "consumption-owner-{}-{}",
        id(247).to_hex(),
        id(255).to_hex()
    ));
    fs::copy(&pending, &ambiguous).unwrap();
    let preserved = (
        tree(owner.metadata_path()),
        tree(destination.metadata_path()),
        tree(destination.project().root()),
    );
    assert!(storage
        .commit_native_consumed_owner_receipt(owner, request(), trusted)
        .is_err());
    assert_eq!(
        (
            tree(owner.metadata_path()),
            tree(destination.metadata_path()),
            tree(destination.project().root())
        ),
        preserved
    );
    fs::remove_file(&ambiguous).unwrap();
    let pending_bytes = fs::read(&pending).unwrap();
    replace_fixture_journal(&pending, b"damaged synthetic owner receipt attempt");
    let preserved = (
        tree(owner.metadata_path()),
        tree(destination.metadata_path()),
        tree(destination.project().root()),
    );
    assert!(storage
        .commit_native_consumed_owner_receipt(owner, request(), trusted)
        .is_err());
    assert_eq!(
        (
            tree(owner.metadata_path()),
            tree(destination.metadata_path()),
            tree(destination.project().root())
        ),
        preserved
    );
    replace_fixture_journal(&pending, &pending_bytes);
    fs::write(root.join("consumption-owner-public-key"), public).unwrap();
    let child = Json::object([
        ("root", Json::text(root.to_str().unwrap())),
        ("owner", Json::text(owner.id())),
        ("source", Json::text(source.id())),
        ("destination", Json::text(destination.id())),
        ("operation", Json::text(version.operation().to_hex())),
        ("grant", Json::text(grant.to_hex())),
    ]);
    for mode in [
        "missing-trust",
        "foreign-operation",
        "foreign-grant",
        "foreign-request",
        "foreign-source",
        "foreign-limits",
    ] {
        let preserved = (
            tree(owner.metadata_path()),
            tree(destination.metadata_path()),
            tree(destination.project().root()),
        );
        run_child(root, child.clone(), mode, "", 0, 0);
        assert_eq!(
            (
                tree(owner.metadata_path()),
                tree(destination.metadata_path()),
                tree(destination.project().root())
            ),
            preserved
        );
    }
    run_child(root, child.clone(), "exit", "owner-staged", 0, 86);
    assert_eq!(fs::read(&owner_path).unwrap(), before);
    run_child(root, child.clone(), "exit", "owner-synced", 0, 86);
    let complete = fs::read(&owner_path).unwrap();
    assert!(complete.starts_with(&before) && complete.len() > before.len());
    let frame = &complete[before.len()..];
    for boundary in [1, frame.len() / 2, frame.len() - 1] {
        replace_fixture_journal(&owner_path, &before);
        run_child(root, child.clone(), "prefix", "owner-staged", boundary, 86);
        let mut partial = before.clone();
        partial.extend_from_slice(&frame[..boundary]);
        assert_eq!(fs::read(&owner_path).unwrap(), partial);
        run_child(root, child.clone(), "exit", "owner-synced", 0, 86);
        assert_eq!(fs::read(&owner_path).unwrap(), complete);
    }
    run_child(root, child.clone(), "exit", "custody-released", 0, 86);
    let receipt = storage
        .commit_native_consumed_owner_receipt(owner, request(), trusted)
        .unwrap();
    assert_ne!(receipt, id(0));
    assert_eq!(
        storage
            .commit_native_consumed_owner_receipt(owner, request(), trusted)
            .unwrap(),
        receipt
    );
    assert_eq!(fs::read(&owner_path).unwrap(), complete);
    assert!(destination.saved_versions().is_err());
    assert_eq!(
        fs::read(destination.project().root().join("note")).unwrap(),
        saved_note
    );
    assert!(!destination.project().root().join(".git").exists());
    assert!(!destination.project().root().join("untracked").exists());
    assert_eq!(tree(source.project().root()), source_before);
    if source.id() != owner.id() {
        assert_eq!(
            fs::read(source.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap(),
            source_journal
        );
    }
    // A historical acknowledgement is still bound to its original pending-file identity during
    // this call. Fresh recovery does not invent a durable sidecar-inode field in the old format.
    assert!(storage
        .commit_native_consumed_owner_with_hooks(
            owner,
            &request(),
            trusted,
            (|_, _, _| Ok(()), |file| file.sync_all()),
            || {
                let bytes = fs::read(&pending)?;
                let permissions = fs::metadata(&pending)?.permissions();
                fs::rename(&pending, &pending_saved)?;
                fs::write(&pending, &bytes)?;
                fs::set_permissions(&pending, permissions)?;
                substituted = Some(tree(owner.metadata_path()));
                Ok(())
            }
        )
        .is_err());
    assert_eq!(tree(owner.metadata_path()), substituted.take().unwrap());
    fs::remove_file(&pending).unwrap();
    fs::rename(&pending_saved, &pending).unwrap();
    assert_eq!(
        storage
            .commit_native_consumed_owner_receipt(owner, request(), trusted)
            .unwrap(),
        receipt
    );
    let journal_saved = root.join("original-owner-journal");
    let permissions = fs::metadata(&owner_path).unwrap().permissions();
    fs::rename(&owner_path, &journal_saved).unwrap();
    fs::write(&owner_path, &complete).unwrap();
    fs::set_permissions(&owner_path, permissions).unwrap();
    let preserved = tree(owner.metadata_path());
    assert!(storage
        .commit_native_consumed_owner_receipt(owner, request(), trusted)
        .is_err());
    assert_eq!(tree(owner.metadata_path()), preserved);
    fs::remove_file(&owner_path).unwrap();
    fs::rename(&journal_saved, &owner_path).unwrap();
    // An editor arrival after release prevents acknowledgement without repairing that file.
    let late = destination.project().root().join("late-editor");
    assert!(storage
        .commit_native_consumed_owner_with_hooks(
            owner,
            &request(),
            trusted,
            (|_, _, _| Ok(()), |file| file.sync_all()),
            || fs::write(&late, b"preserve editor arrival")
        )
        .is_err());
    assert_eq!(fs::read(&late).unwrap(), b"preserve editor arrival");
    fs::remove_file(&late).unwrap();
    assert_eq!(
        storage
            .commit_native_consumed_owner_receipt(owner, request(), trusted)
            .unwrap(),
        receipt
    );
    let revoked = storage
        .grant_native_saved_input(
            owner,
            NativeTrustedInputGrantRequest {
                source,
                operation: version.operation(),
                destination: &destination,
                allowed: false,
                expected_previous: Some(grant),
                request: id(248),
            },
            &[owner],
            trusted,
        )
        .unwrap()
        .record();
    assert_ne!(revoked, grant);
    let historical = fs::read(&owner_path).unwrap();
    assert_eq!(
        storage
            .commit_native_consumed_owner_receipt(owner, request(), trusted)
            .unwrap(),
        receipt
    );
    assert_eq!(fs::read(&owner_path).unwrap(), historical);
    assert!(storage
        .recover_native_consumed_start(owner, request(), trusted)
        .is_err());
    let (unfinished, unfinished_grant, _, _) =
        prepare_checkpoint(storage, owner, source, version, trusted, (251, 252, 253));
    storage
        .grant_native_saved_input(
            owner,
            NativeTrustedInputGrantRequest {
                source,
                operation: version.operation(),
                destination: &unfinished,
                allowed: false,
                expected_previous: Some(unfinished_grant),
                request: id(254),
            },
            &[owner],
            trusted,
        )
        .unwrap();
    let preserved = (
        tree(owner.metadata_path()),
        tree(unfinished.metadata_path()),
        tree(unfinished.project().root()),
    );
    assert!(storage
        .commit_native_consumed_owner_receipt(
            owner,
            NativeTrustedConsumedStartRequest {
                source,
                operation: version.operation(),
                destination: &unfinished,
                grant: unfinished_grant,
                available: &available,
                request: id(253),
                limits: ObservationLimits::default(),
            },
            trusted
        )
        .is_err());
    assert_eq!(
        (
            tree(owner.metadata_path()),
            tree(unfinished.metadata_path()),
            tree(unfinished.project().root())
        ),
        preserved
    );
    assert!(unfinished.saved_versions().is_err());
    assert!(destination.saved_versions().is_err());
    assert_eq!(tree(source.project().root()), source_before);
}
fn attempt(owner: &ProvisionedAttachment, request: RecordDigest) -> PathBuf {
    let prefix = format!("consumption-owner-{}-", request.to_hex());
    let paths = fs::read_dir(owner.metadata_path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with(&prefix)
        })
        .collect::<Vec<_>>();
    assert_eq!(paths.len(), 1);
    paths[0].clone()
}
type FixtureTree = std::collections::BTreeMap<PathBuf, (u64, u64, u32, Vec<u8>)>;
fn tree(root: &Path) -> FixtureTree {
    fn walk(root: &Path, path: &Path, out: &mut FixtureTree) {
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
fn consumption_owner_restart_child() {
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
    let public = fs::read(root.join("consumption-owner-public-key")).unwrap();
    let trusted = if text("mode") == "missing-trust" {
        crate::TrustedReviewers::default()
    } else {
        crate::TrustedReviewers::with_human_credentials([HumanApprovalCredential::from_public_key(
            public.as_slice().try_into().unwrap(),
        )
        .unwrap()])
    };
    let available = [&owner];
    let mut limits = ObservationLimits::default();
    if text("mode") == "foreign-limits" {
        limits.file_bytes += 1;
    }
    let request = NativeTrustedConsumedStartRequest {
        source: if text("mode") == "foreign-source" {
            &destination
        } else {
            &source
        },
        operation: if text("mode") == "foreign-operation" {
            id(255)
        } else {
            RecordDigest::parse_hex(text("operation")).unwrap()
        },
        destination: &destination,
        grant: if text("mode") == "foreign-grant" {
            id(255)
        } else {
            RecordDigest::parse_hex(text("grant")).unwrap()
        },
        available: &available,
        request: if text("mode") == "foreign-request" {
            id(255)
        } else {
            id(247)
        },
        limits,
    };
    if !["exit", "prefix"].contains(&text("mode")) {
        assert!(storage
            .commit_native_consumed_owner_receipt(&owner, request, &trusted)
            .is_err());
        return;
    }
    storage
        .commit_native_consumed_owner_with_hooks(
            &owner,
            &request,
            &trusted,
            (
                |step, file, frame| {
                    if step == text("step") {
                        if text("mode") == "prefix" {
                            use std::io::Write as _;
                            let n = value.get("boundary").and_then(Json::as_u64).unwrap() as usize;
                            assert!(n > 0 && n < frame.len());
                            file.write_all(&frame[..n])?;
                            file.sync_all()?;
                        }
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
    panic!("requested native owner boundary was not reached");
}
fn run_child(root: &Path, mut value: Json, mode: &str, step: &str, boundary: usize, expected: i32) {
    let Json::Object(fields) = &mut value else {
        panic!("invalid fixture");
    };
    for (key, val) in [("mode", mode), ("step", step)] {
        fields.push((key.into(), Json::text(val)));
    }
    fields.push(("boundary".into(), Json::Number(boundary as u64)));
    let log = root.join(format!("consumption-owner-{mode}-{step}-{boundary}.log"));
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
                "native owner child deadline: {}",
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
