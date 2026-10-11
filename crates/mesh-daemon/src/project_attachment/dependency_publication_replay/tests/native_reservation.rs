//! Actual reservation writer and separate-process recovery after real native publication.
use super::*;
use crate::project_attachment::{NativeTrustedInputGrantRequest, SavedAttachmentVersion};
use std::{
    os::unix::fs::PermissionsExt as _,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
const ENV: &str = "MESH_NATIVE_RESERVATION_RESTART_FIXTURE";
const CHILD: &str = "project_attachment::dependency_publication_replay::tests::native_reservation::reservation_restart_child";
fn id(n: u8) -> RecordDigest {
    RecordDigest::from_bytes([n; 32])
}
#[test]
fn reservation_restart_child() {
    let Ok(raw) = std::env::var(ENV) else {
        return;
    };
    let v = Json::parse(&raw).unwrap();
    let text = |k| v.get(k).and_then(Json::as_text).unwrap();
    let root = Path::new(text("root"));
    let storage = AttachmentStorage::open(&root.join("metadata")).unwrap();
    let owner = storage.reopen(text("owner")).unwrap();
    let source = storage.reopen(text("source")).unwrap();
    let credential = HumanApprovalCredential::from_public_key(
        fs::read(root.join("reservation-public-key"))
            .unwrap()
            .as_slice()
            .try_into()
            .unwrap(),
    )
    .unwrap();
    let mode = text("mode");
    let trusted = if mode == "missing-trust" {
        crate::TrustedReviewers::default()
    } else {
        crate::TrustedReviewers::with_human_credentials([credential])
    };
    let operation = if mode == "foreign-operation" {
        id(250)
    } else {
        RecordDigest::parse_hex(text("operation")).unwrap()
    };
    let result = storage.reserve_native_dependency_lane_with_hook(
        &owner,
        &source,
        operation,
        RecordDigest::parse_hex(text("request")).unwrap(),
        &[&owner],
        &trusted,
        |phase| {
            if phase == mode {
                std::process::exit(85);
            }
            Ok(())
        },
    );
    if matches!(mode, "missing-trust" | "foreign-operation" | "refused") {
        assert!(result.is_err());
    } else {
        let work = result.unwrap();
        fs::write(root.join("reservation-result"), work.id()).unwrap();
    }
}
fn run_child(root: &Path, mode: &str, mut value: Json, expected: i32) {
    let Json::Object(fields) = &mut value else {
        panic!("invalid fixture");
    };
    fields.push(("mode".into(), Json::text(mode)));
    let log = root.join(format!("reservation-{mode}.log"));
    let out = fs::File::create(&log).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", CHILD, "--nocapture"])
        .env(ENV, value.encode())
        .stdout(Stdio::from(out.try_clone().unwrap()))
        .stderr(Stdio::from(out))
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
                "reservation {mode} timeout: {}",
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
pub(super) fn inspect(
    storage: &AttachmentStorage,
    owner: &ProvisionedAttachment,
    source: &ProvisionedAttachment,
    version: SavedAttachmentVersion,
    trusted: &crate::TrustedReviewers,
    public: &[u8],
) {
    let reserve = |operation, request, trust: &crate::TrustedReviewers| {
        storage.reserve_native_dependency_lane(owner, source, operation, request, &[owner], trust)
    };
    let journal = owner.metadata_path().join(crate::RECORD_FILE_NAME);
    let source_journal = source.metadata_path().join(crate::RECORD_FILE_NAME);
    let initial = fs::read(&journal).unwrap();
    let source_initial = fs::read(&source_journal).unwrap();
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
    fs::write(git.join("HEAD"), b"ref: refs/heads/user-reservation\n").unwrap();
    fs::write(git.join("index"), b"synthetic staged editor work").unwrap();
    let untracked = owner.project().root().join("untracked-reservation-work");
    fs::write(&untracked, b"preserve user work").unwrap();
    let preserve = || {
        assert_eq!(fs::read(&journal).unwrap(), initial);
        assert_eq!(fs::read(&source_journal).unwrap(), source_initial);
        for (p, b) in &notes {
            assert_eq!(fs::read(p).unwrap(), *b);
        }
        assert_eq!(
            fs::read(git.join("HEAD")).unwrap(),
            b"ref: refs/heads/user-reservation\n"
        );
        assert_eq!(
            fs::read(git.join("index")).unwrap(),
            b"synthetic staged editor work"
        );
        assert_eq!(fs::read(&untracked).unwrap(), b"preserve user work");
    };
    let before = storage.registrations().unwrap().len();
    assert!(reserve(
        version.operation(),
        id(230),
        &crate::TrustedReviewers::default()
    )
    .is_err());
    assert!(reserve(id(0), id(230), trusted).is_err());
    assert!(reserve(id(250), id(230), trusted).is_err());
    assert!(reserve(version.operation(), id(0), trusted).is_err());
    assert!(storage
        .reserve_native_dependency_lane(
            owner,
            source,
            version.operation(),
            id(230),
            &vec![owner; 257],
            trusted
        )
        .is_err());
    if source.id() != owner.id() {
        assert!(storage
            .reserve_native_dependency_lane(
                source,
                source,
                version.operation(),
                id(230),
                &[],
                trusted
            )
            .is_err());
    }
    assert_eq!(storage.registrations().unwrap().len(), before);
    for pending in [
        "dependency-capture.pending",
        "dependency-decision.pending",
        "native-publication.pending",
    ] {
        let path = owner.metadata_path().join(pending);
        assert!(!path.exists());
        fs::write(&path, b"unknown unfinished transaction").unwrap();
        assert!(reserve(version.operation(), id(230), trusted).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"unknown unfinished transaction");
        fs::remove_file(path).unwrap();
        assert_eq!(storage.registrations().unwrap().len(), before);
    }
    let destination = reserve(version.operation(), id(230), trusted).unwrap();
    let retry = reserve(version.operation(), id(230), trusted).unwrap();
    assert_eq!(destination.id(), retry.id());
    assert_eq!(
        destination.store.identity().unwrap(),
        retry.store.identity().unwrap()
    );
    assert_eq!(
        fs::read_dir(destination.project().root()).unwrap().count(),
        0
    );
    assert!(destination.saved_versions().unwrap().is_empty());
    assert_eq!(storage.registrations().unwrap().len(), before + 1);
    let allocation = destination.project().root().parent().unwrap();
    let intent = allocation.join("intent.json");
    let raw = fs::read(&intent).unwrap();
    let mut bad = raw.clone();
    bad.push(b' ');
    fs::write(&intent, &bad).unwrap();
    assert!(reserve(version.operation(), id(230), trusted).is_err());
    assert_eq!(fs::read(&intent).unwrap(), bad);
    fs::write(&intent, &raw).unwrap();
    let permissions = fs::metadata(&intent).unwrap().permissions();
    fs::set_permissions(&intent, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(reserve(version.operation(), id(230), trusted).is_err());
    fs::set_permissions(&intent, permissions).unwrap();
    let hardlink = allocation.join("intent-hardlink");
    fs::hard_link(&intent, &hardlink).unwrap();
    assert!(reserve(version.operation(), id(230), trusted).is_err());
    fs::remove_file(&hardlink).unwrap();
    let path = destination.project().root().to_path_buf();
    let retained = allocation.join("retained-files");
    fs::rename(&path, &retained).unwrap();
    fs::create_dir(&path).unwrap();
    fs::write(path.join("user-work"), b"foreign editor bytes").unwrap();
    assert!(reserve(version.operation(), id(230), trusted).is_err());
    assert_eq!(
        fs::read(path.join("user-work")).unwrap(),
        b"foreign editor bytes"
    );
    fs::rename(&path, allocation.join("foreign-files")).unwrap();
    fs::rename(&retained, &path).unwrap();
    assert_eq!(
        reserve(version.operation(), id(230), trusted).unwrap().id(),
        destination.id()
    );
    preserve();
    let root = owner.project().root().parent().unwrap();
    fs::write(root.join("reservation-public-key"), public).unwrap();
    for (index, phase) in ["initialized", "fenced", "published", "recorded"]
        .into_iter()
        .enumerate()
    {
        let request = id(231 + index as u8);
        let value = Json::object([
            ("root", Json::text(root.to_string_lossy())),
            ("owner", Json::text(owner.id())),
            ("source", Json::text(source.id())),
            ("operation", Json::text(version.operation().to_hex())),
            ("request", Json::text(request.to_hex())),
        ]);
        run_child(root, phase, value.clone(), 85);
        let count = storage.registrations().unwrap().len();
        assert_eq!(
            count,
            before + 1 + index + usize::from(matches!(phase, "published" | "recorded"))
        );
        run_child(root, "missing-trust", value.clone(), 0);
        run_child(root, "foreign-operation", value.clone(), 0);
        assert_eq!(storage.registrations().unwrap().len(), count);
        if phase == "initialized" {
            let allocation = fs::read_dir(root.join("metadata/work-lanes"))
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .find(|path| {
                    let raw = fs::read_to_string(path.join("intent.json")).unwrap();
                    Json::parse(&raw)
                        .unwrap()
                        .get("request")
                        .and_then(Json::as_text)
                        == Some(request.to_hex().as_str())
                })
                .unwrap();
            let receipt = allocation.join("store.json");
            let bytes = fs::read(&receipt).unwrap();
            fs::rename(&receipt, allocation.join("retained-store-receipt")).unwrap();
            run_child(root, "refused", value.clone(), 0);
            assert!(!receipt.exists());
            assert_eq!(
                fs::read(allocation.join("retained-store-receipt")).unwrap(),
                bytes
            );
            fs::rename(allocation.join("retained-store-receipt"), &receipt).unwrap();
            let editor = allocation.join("files/editor-work");
            fs::write(&editor, b"preserve interrupted editor work").unwrap();
            run_child(root, "refused", value.clone(), 0);
            assert_eq!(
                fs::read(&editor).unwrap(),
                b"preserve interrupted editor work"
            );
            fs::rename(&editor, allocation.join("retained-editor-work")).unwrap();
        }
        run_child(root, "recover", value.clone(), 0);
        let recovered = fs::read_to_string(root.join("reservation-result")).unwrap();
        let work = storage.reopen(&recovered).unwrap();
        let identity = work.store.identity().unwrap();
        run_child(root, "retry", value, 0);
        assert_eq!(
            fs::read_to_string(root.join("reservation-result")).unwrap(),
            recovered
        );
        assert_eq!(
            storage
                .reopen(&recovered)
                .unwrap()
                .store
                .identity()
                .unwrap(),
            identity
        );
        assert_eq!(storage.registrations().unwrap().len(), before + 2 + index);
        assert_eq!(fs::read_dir(work.project().root()).unwrap().count(), 0);
        assert!(work.saved_versions().unwrap().is_empty());
        preserve();
    }
    let source_path = source.project().root().to_path_buf();
    let retained_source = source_path.with_extension("reservation-original");
    assert!(storage
        .reserve_native_dependency_lane_with_hook(
            owner,
            source,
            version.operation(),
            id(236),
            &[owner],
            trusted,
            |phase| {
                if phase == "fenced" {
                    fs::rename(&source_path, &retained_source)?;
                    fs::create_dir(&source_path)?;
                    fs::write(
                        source_path.join("foreign-editor"),
                        b"keep replacement source",
                    )?;
                }
                Ok(())
            }
        )
        .is_err());
    assert_eq!(
        fs::read(source_path.join("foreign-editor")).unwrap(),
        b"keep replacement source"
    );
    fs::rename(
        &source_path,
        source_path.with_extension("reservation-foreign"),
    )
    .unwrap();
    fs::rename(&retained_source, &source_path).unwrap();
    let recovered = reserve(version.operation(), id(236), trusted).unwrap();
    assert_eq!(fs::read_dir(recovered.project().root()).unwrap().count(), 0);
    preserve();
    // The actual trusted grant writer can bind the destination allocated after publication.
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
        .unwrap();
    assert_eq!(grant.generation(), 1);
    let historical = reserve(version.operation(), id(230), trusted).unwrap();
    assert_eq!(historical.id(), destination.id());
    assert_eq!(
        historical.store.identity().unwrap(),
        destination.store.identity().unwrap()
    );
    assert!(owner.saved_versions().is_err()); // ordinary admission remains fenced
    assert_eq!(
        fs::read_dir(destination.project().root()).unwrap().count(),
        0
    );
    for (p, b) in notes {
        assert_eq!(fs::read(p).unwrap(), b);
    }
}
