//! Real native committed owner -> completion, signer-free process retries and exact historical joins.
use super::*;
use crate::project_attachment::{
    NativeTrustedConsumedStartRequest, NativeTrustedInputGrantRequest, SavedAttachmentVersion,
};
use std::{
    os::unix::fs::MetadataExt as _,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
const ENV: &str = "MESH_NATIVE_CONSUMPTION_COMPLETION_RESTART";
const CHILD: &str = "project_attachment::dependency_publication_replay::tests::native_consumption_completion::consumption_completion_restart_child";
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
    let (destination, grant, prepared, staged) = native_consumption_owner::prepare_checkpoint(
        storage,
        owner,
        source,
        version,
        trusted,
        (231, 232, 233),
    );
    let available = [owner];
    let owner_request = || NativeTrustedConsumedStartRequest {
        source,
        operation: version.operation(),
        destination: &destination,
        grant,
        available: &available,
        request: id(233),
        limits: ObservationLimits::default(),
    };
    let owner_receipt = storage
        .commit_native_consumed_owner_receipt(owner, owner_request(), trusted)
        .unwrap();
    storage
        .grant_native_saved_input(
            owner,
            NativeTrustedInputGrantRequest {
                source,
                operation: version.operation(),
                destination: &destination,
                allowed: false,
                expected_previous: Some(grant),
                request: id(234),
            },
            &[owner],
            trusted,
        )
        .unwrap();
    assert!(storage
        .recover_native_consumed_start(owner, owner_request(), trusted)
        .is_err());
    let owner_history = fs::read(owner.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap();
    assert!(prepared
        .try_ordinary_complete_for_test(storage, &staged)
        .is_err());
    let request = || NativeTrustedConsumedStartRequest {
        source,
        operation: version.operation(),
        destination: &destination,
        grant,
        available: &available,
        request: id(233),
        limits: ObservationLimits::default(),
    };
    let destination_path = destination.metadata_path().join(crate::RECORD_FILE_NAME);
    let before = fs::read(&destination_path).unwrap();
    let pending_saved = root.join("original-completion-pending");
    let mut substituted = None;
    // The first new sidecar is pinned before the hook. Same-byte/mode replacement must not append.
    assert!(storage
        .complete_native_consumption_with_hooks(
            owner,
            &request(),
            trusted,
            (
                |step, _, _| {
                    assert_eq!(step, "complete-staged");
                    let pending = completion_pending(&destination);
                    let bytes = fs::read(&pending)?;
                    let permissions = fs::metadata(&pending)?.permissions();
                    fs::rename(&pending, &pending_saved)?;
                    fs::write(&pending, &bytes)?;
                    fs::set_permissions(&pending, permissions)?;
                    substituted = Some(tree(destination.metadata_path()));
                    Ok(())
                },
                |file| file.sync_all()
            ),
            || Ok(())
        )
        .is_err());
    assert_eq!(fs::read(&destination_path).unwrap(), before);
    assert_eq!(
        tree(destination.metadata_path()),
        substituted.take().unwrap()
    );
    let pending = completion_pending(&destination);
    fs::remove_file(&pending).unwrap();
    fs::rename(&pending_saved, &pending).unwrap();
    // Two matching committed-owner attempts cannot authorize this local completion.
    let owner_pending = owner_attempt(owner, id(233));
    let ambiguous = owner.metadata_path().join(format!(
        "consumption-owner-{}-{}",
        id(233).to_hex(),
        id(255).to_hex()
    ));
    fs::copy(&owner_pending, &ambiguous).unwrap();
    let preserved = (
        tree(owner.metadata_path()),
        tree(destination.metadata_path()),
        tree(destination.project().root()),
    );
    assert!(storage
        .complete_native_consumption_receipt(owner, request(), trusted)
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
    replace_fixture_journal(&pending, b"damaged synthetic completion attempt");
    let preserved = (
        tree(owner.metadata_path()),
        tree(destination.metadata_path()),
        tree(destination.project().root()),
    );
    assert!(storage
        .complete_native_consumption_receipt(owner, request(), trusted)
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
    fs::write(root.join("consumption-completion-public-key"), public).unwrap();
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
    run_child(root, child.clone(), "exit", "complete-staged", 0, 86);
    assert_eq!(fs::read(&destination_path).unwrap(), before);
    run_child(root, child.clone(), "exit", "complete-synced", 0, 86);
    let complete = fs::read(&destination_path).unwrap();
    assert!(complete.starts_with(&before) && complete.len() > before.len());
    let frame = &complete[before.len()..];
    for boundary in [1, frame.len() / 2, frame.len() - 1] {
        replace_fixture_journal(&destination_path, &before);
        run_child(
            root,
            child.clone(),
            "prefix",
            "complete-staged",
            boundary,
            86,
        );
        let mut partial = before.clone();
        partial.extend_from_slice(&frame[..boundary]);
        assert_eq!(fs::read(&destination_path).unwrap(), partial);
        run_child(root, child.clone(), "exit", "complete-synced", 0, 86);
        assert_eq!(fs::read(&destination_path).unwrap(), complete);
    }
    run_child(root, child.clone(), "exit", "custody-released", 0, 86);
    let receipt = storage
        .complete_native_consumption_receipt(owner, request(), trusted)
        .unwrap();
    assert_ne!(receipt, id(0));
    assert_eq!(
        storage
            .complete_native_consumption_receipt(owner, request(), trusted)
            .unwrap(),
        receipt
    );
    assert_eq!(fs::read(&destination_path).unwrap(), complete);
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
        .complete_native_consumption_with_hooks(
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
                substituted = Some(tree(destination.metadata_path()));
                Ok(())
            }
        )
        .is_err());
    assert_eq!(
        tree(destination.metadata_path()),
        substituted.take().unwrap()
    );
    fs::remove_file(&pending).unwrap();
    fs::rename(&pending_saved, &pending).unwrap();
    assert_eq!(
        storage
            .complete_native_consumption_receipt(owner, request(), trusted)
            .unwrap(),
        receipt
    );
    // Acknowledgement also pins the committed owner attempt and all original destination intents.
    // Each replacement has identical bytes and mode; only its inode changes.
    for sidecar in [
        owner_pending.clone(),
        destination
            .metadata_path()
            .join("consumption-start.pending"),
        destination
            .metadata_path()
            .join("consumption-history.pending"),
        destination
            .metadata_path()
            .join("consumption-owner.pending"),
    ] {
        let saved = root.join("original-bound-sidecar");
        let bytes = fs::read(&sidecar).unwrap();
        let permissions = fs::metadata(&sidecar).unwrap().permissions();
        let mut foreign = None;
        assert!(storage
            .complete_native_consumption_with_hooks(
                owner,
                &request(),
                trusted,
                (|_, _, _| Ok(()), |file| file.sync_all()),
                || {
                    fs::rename(&sidecar, &saved)?;
                    fs::write(&sidecar, &bytes)?;
                    fs::set_permissions(&sidecar, permissions)?;
                    foreign = Some((
                        tree(owner.metadata_path()),
                        tree(destination.metadata_path()),
                        tree(destination.project().root()),
                    ));
                    Ok(())
                }
            )
            .is_err());
        assert_eq!(
            (
                tree(owner.metadata_path()),
                tree(destination.metadata_path()),
                tree(destination.project().root())
            ),
            foreign.unwrap()
        );
        fs::remove_file(&sidecar).unwrap();
        fs::rename(&saved, &sidecar).unwrap();
        assert_eq!(
            storage
                .complete_native_consumption_receipt(owner, request(), trusted)
                .unwrap(),
            receipt
        );
        assert_eq!(fs::read(&destination_path).unwrap(), complete);
    }
    let journal_saved = root.join("original-completion-journal");
    let permissions = fs::metadata(&destination_path).unwrap().permissions();
    fs::rename(&destination_path, &journal_saved).unwrap();
    fs::write(&destination_path, &complete).unwrap();
    fs::set_permissions(&destination_path, permissions).unwrap();
    let preserved = tree(destination.metadata_path());
    assert!(storage
        .complete_native_consumption_receipt(owner, request(), trusted)
        .is_err());
    assert_eq!(tree(destination.metadata_path()), preserved);
    fs::remove_file(&destination_path).unwrap();
    fs::rename(&journal_saved, &destination_path).unwrap();
    // An editor arrival after release prevents acknowledgement without repairing that file.
    let late = destination.project().root().join("late-editor");
    assert!(storage
        .complete_native_consumption_with_hooks(
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
            .complete_native_consumption_receipt(owner, request(), trusted)
            .unwrap(),
        receipt
    );
    assert_eq!(
        fs::read(owner.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap(),
        owner_history
    );
    assert_ne!(receipt, owner_receipt);
    let historical = fs::read(&destination_path).unwrap();
    assert_eq!(
        storage
            .complete_native_consumption_receipt(owner, request(), trusted)
            .unwrap(),
        receipt
    );
    assert_eq!(fs::read(&destination_path).unwrap(), historical);
    assert!(storage
        .recover_native_consumed_start(owner, request(), trusted)
        .is_err());
    let (unfinished, unfinished_grant, _, _) = native_consumption_owner::prepare_checkpoint(
        storage,
        owner,
        source,
        version,
        trusted,
        (235, 236, 237),
    );
    let unfinished_request = || NativeTrustedConsumedStartRequest {
        source,
        operation: version.operation(),
        destination: &unfinished,
        grant: unfinished_grant,
        available: &available,
        request: id(237),
        limits: ObservationLimits::default(),
    };
    let preserved = (
        tree(owner.metadata_path()),
        tree(unfinished.metadata_path()),
        tree(unfinished.project().root()),
    );
    assert!(storage
        .complete_native_consumption_receipt(owner, unfinished_request(), trusted)
        .is_err());
    assert_eq!(
        (
            tree(owner.metadata_path()),
            tree(unfinished.metadata_path()),
            tree(unfinished.project().root())
        ),
        preserved
    );
    assert!(storage
        .commit_native_consumed_owner_with_hooks(
            owner,
            &unfinished_request(),
            trusted,
            (
                |step, _, _| {
                    assert_eq!(step, "owner-staged");
                    Err(std::io::Error::other(
                        "synthetic owner attempt before commitment",
                    ))
                },
                |file| file.sync_all()
            ),
            || Ok(())
        )
        .is_err());
    let preserved = (
        tree(owner.metadata_path()),
        tree(unfinished.metadata_path()),
        tree(unfinished.project().root()),
    );
    assert!(storage
        .complete_native_consumption_receipt(owner, unfinished_request(), trusted)
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
fn completion_pending(destination: &ProvisionedAttachment) -> PathBuf {
    destination
        .metadata_path()
        .join("consumption-complete.pending")
}
fn owner_attempt(owner: &ProvisionedAttachment, request: RecordDigest) -> PathBuf {
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
fn consumption_completion_restart_child() {
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
    let public = fs::read(root.join("consumption-completion-public-key")).unwrap();
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
            id(233)
        },
        limits,
    };
    if !["exit", "prefix"].contains(&text("mode")) {
        assert!(storage
            .complete_native_consumption_receipt(&owner, request, &trusted)
            .is_err());
        return;
    }
    storage
        .complete_native_consumption_with_hooks(
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
    panic!("requested native completion boundary was not reached");
}
fn run_child(root: &Path, mut value: Json, mode: &str, step: &str, boundary: usize, expected: i32) {
    let Json::Object(fields) = &mut value else {
        panic!("invalid fixture");
    };
    for (key, val) in [("mode", mode), ("step", step)] {
        fields.push((key.into(), Json::text(val)));
    }
    fields.push(("boundary".into(), Json::Number(boundary as u64)));
    let log = root.join(format!(
        "consumption-completion-{mode}-{step}-{boundary}.log"
    ));
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
                "native completion child deadline: {}",
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
