//! Real completed native lanes: immutable versions/files, independent capture and process reopen.
use super::*;
use crate::project_attachment::{
    NativeTrustedConsumedStartRequest, NativeTrustedInputGrantRequest, SavedAttachmentVersion,
};
use std::{
    collections::BTreeMap,
    os::unix::fs::MetadataExt as _,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
const ENV: &str = "MESH_NATIVE_COMPLETED_HISTORY_RESTART";
const CHILD: &str = "project_attachment::dependency_publication_replay::tests::native_completed_history::completed_history_restart_child";
fn id(n: u8) -> RecordDigest {
    RecordDigest::from_bytes([n; 32])
}
type Tree = BTreeMap<PathBuf, (u64, u64, u32, Vec<u8>)>;
fn tree(root: &Path) -> Tree {
    fn walk(root: &Path, path: &Path, out: &mut Tree) {
        let metadata = fs::symlink_metadata(path).unwrap();
        assert!(!metadata.file_type().is_symlink());
        out.insert(
            path.strip_prefix(root).unwrap().to_path_buf(),
            (
                metadata.dev(),
                metadata.ino(),
                metadata.mode(),
                if metadata.is_file() {
                    fs::read(path).unwrap()
                } else {
                    vec![]
                },
            ),
        );
        if metadata.is_dir() {
            for child in fs::read_dir(path).unwrap() {
                walk(root, &child.unwrap().path(), out);
            }
        }
    }
    let mut result = BTreeMap::new();
    walk(root, root, &mut result);
    result
}
#[derive(Debug, PartialEq, Eq)]
struct Preserved {
    owner: Tree,
    source: Tree,
    destination: Tree,
    source_files: Tree,
    destination_files: Tree,
}
fn preserved(
    owner: &ProvisionedAttachment,
    source: &ProvisionedAttachment,
    destination: &ProvisionedAttachment,
) -> Preserved {
    Preserved {
        owner: tree(owner.metadata_path()),
        source: tree(source.metadata_path()),
        destination: tree(destination.metadata_path()),
        source_files: tree(source.project().root()),
        destination_files: tree(destination.project().root()),
    }
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
    fs::create_dir_all(source.project().root().join(".git")).unwrap();
    fs::write(
        source.project().root().join(".git/HEAD"),
        b"ref: refs/heads/existing\n",
    )
    .unwrap();
    fs::write(
        source.project().root().join(".git/index"),
        b"synthetic existing Git index",
    )
    .unwrap();
    fs::write(
        source.project().root().join("note"),
        b"newer original editor bytes",
    )
    .unwrap();
    fs::write(
        source.project().root().join("untracked"),
        b"original untracked bytes",
    )
    .unwrap();
    let original_files = tree(source.project().root());
    let (destination, grant, prepared, _staged) = native_consumption_owner::prepare_checkpoint(
        storage,
        owner,
        source,
        version,
        trusted,
        (211, 212, 213),
    );
    let available = [owner];
    let request = || NativeTrustedConsumedStartRequest {
        source,
        operation: version.operation(),
        destination: &destination,
        grant,
        available: &available,
        request: id(213),
        limits: ObservationLimits::default(),
    };
    let pending = preserved(owner, source, &destination);
    assert!(storage
        .inspect_native_saved_versions(destination.id(), trusted)
        .is_err());
    assert!(storage
        .inspect_native_saved_file(destination.id(), version, "note", trusted)
        .is_err());
    assert_eq!(
        preserved(owner, source, &destination),
        pending,
        "unfinished lane gained saved-history access or was repaired"
    );
    storage
        .commit_native_consumed_owner_receipt(owner, request(), trusted)
        .unwrap();
    storage
        .complete_native_consumption_receipt(owner, request(), trusted)
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
                request: id(214),
            },
            &[owner],
            trusted,
        )
        .unwrap();
    assert!(storage
        .recover_native_consumed_start(owner, request(), trusted)
        .is_err());
    let operation = prepared.operation();
    let saved = storage
        .inspect_native_saved_version(destination.id(), operation, trusted)
        .unwrap();
    assert_eq!(saved.operation(), operation);
    assert!(storage
        .registered_dependency_versions(destination.id())
        .is_err());
    assert!(destination.saved_versions().is_err());
    let owner_history = tree(owner.metadata_path());
    let source_history = tree(source.metadata_path());
    let publication = storage
        .inspect_native_publication_history(destination.id(), trusted)
        .unwrap();
    fs::write(
        destination.project().root().join("note"),
        b"newer destination editor work",
    )
    .unwrap();
    fs::write(
        destination.project().root().join("untracked"),
        b"untracked destination work",
    )
    .unwrap();
    fs::create_dir_all(destination.project().root().join(".git")).unwrap();
    fs::write(
        destination.project().root().join(".git/HEAD"),
        b"ref: refs/heads/native-lane\n",
    )
    .unwrap();
    fs::write(
        destination.project().root().join(".git/index"),
        b"synthetic independent lane index",
    )
    .unwrap();
    fs::write(
        destination.project().root().join("raw.bin"),
        [0, 255, 17, 0, 128],
    )
    .unwrap();
    let initial = preserved(owner, source, &destination);
    assert_eq!(
        storage
            .inspect_native_saved_versions(destination.id(), trusted)
            .unwrap(),
        vec![saved]
    );
    assert_eq!(
        storage
            .inspect_native_saved_file(destination.id(), saved, "note", trusted)
            .unwrap(),
        Some(saved_note.to_vec())
    );
    assert_eq!(
        storage
            .inspect_native_saved_file(destination.id(), saved, "raw.bin", trusted)
            .unwrap(),
        None
    );
    assert!(storage
        .inspect_native_saved_versions(destination.id(), &crate::TrustedReviewers::default())
        .is_err());
    assert!(storage
        .inspect_native_saved_file(
            destination.id(),
            saved,
            "note",
            &crate::TrustedReviewers::default()
        )
        .is_err());
    assert!(storage
        .inspect_native_saved_file(destination.id(), version, "note", trusted)
        .is_err());
    assert!(storage
        .inspect_native_saved_file(owner.id(), saved, "note", trusted)
        .is_err());
    assert_eq!(
        preserved(owner, source, &destination),
        initial,
        "saved reads changed native evidence or editor/Git bytes"
    );
    let spawn = |versions: &[SavedAttachmentVersion], mode: &str| {
        let value = Json::object([
            ("storage", Json::text(storage.path.to_string_lossy())),
            ("work", Json::text(destination.id())),
            ("operation", Json::text(saved.operation().to_hex())),
            (
                "versions",
                Json::Array(
                    versions
                        .iter()
                        .map(|version| Json::text(version.operation().to_hex()))
                        .collect(),
                ),
            ),
            ("public", Json::text(hex(public))),
            ("note", Json::text(hex(saved_note))),
            ("mode", Json::text(mode)),
        ]);
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", CHILD, "--nocapture"])
            .env_remove("MESH_PRIVATE_STAGE_RESTART")
            .env_remove("MESH_FENCED_START_RESTART")
            .env_remove("MESH_NATIVE_CONSUMPTION_OWNER_RESTART")
            .env_remove("MESH_NATIVE_CONSUMPTION_COMPLETION_RESTART")
            .env(ENV, value.encode())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                let output = child.wait_with_output().unwrap();
                panic!(
                    "fixture child deadline: {} {}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    };
    spawn(&[saved], "read");
    assert_eq!(preserved(owner, source, &destination), initial);
    let pending = destination
        .metadata_path()
        .join(crate::project_attachment::consumption_complete::PENDING);
    let canonical = fs::read(&pending).unwrap();
    fs::write(&pending, b"foreign completion recovery evidence").unwrap();
    let damaged = preserved(owner, source, &destination);
    assert!(storage
        .inspect_native_saved_versions(destination.id(), trusted)
        .is_err());
    assert!(storage
        .inspect_native_saved_file(destination.id(), saved, "note", trusted)
        .is_err());
    spawn(&[saved], "refuse");
    assert_eq!(
        preserved(owner, source, &destination),
        damaged,
        "read repaired foreign completion evidence"
    );
    fs::write(&pending, canonical).unwrap();
    assert_eq!(preserved(owner, source, &destination), initial);
    // A separate private save follows editor work. Reading old content cannot rewind it or main.
    let key = SigningKey::from_bytes(&[210; 32]);
    let actor = PublicKey::from_bytes(key.verifying_key().to_bytes());
    let input = destination
        .project()
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    let newer = storage
        .save_verified_dependency_capture(
            &destination,
            &input,
            actor,
            trusted,
            |payload: &SigningPayload| {
                Ok::<_, &'static str>(Signature::from_bytes(
                    key.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .unwrap();
    assert_ne!(newer, saved);
    let captured = preserved(owner, source, &destination);
    assert_eq!(
        storage
            .inspect_native_saved_versions(destination.id(), trusted)
            .unwrap(),
        vec![saved, newer]
    );
    assert_eq!(
        storage
            .inspect_native_saved_file(destination.id(), saved, "note", trusted)
            .unwrap(),
        Some(saved_note.to_vec())
    );
    assert_eq!(
        storage
            .inspect_native_saved_file(destination.id(), newer, "note", trusted)
            .unwrap(),
        Some(b"newer destination editor work".to_vec())
    );
    assert_eq!(
        storage
            .inspect_native_saved_file(destination.id(), newer, "raw.bin", trusted)
            .unwrap(),
        Some(vec![0, 255, 17, 0, 128])
    );
    assert_eq!(
        storage
            .inspect_native_saved_file(destination.id(), newer, ".git/HEAD", trusted)
            .unwrap(),
        None
    );
    spawn(&[saved, newer], "read");
    assert_eq!(
        storage
            .inspect_native_publication_history(destination.id(), trusted)
            .unwrap(),
        publication
    );
    assert_eq!(
        preserved(owner, source, &destination),
        captured,
        "history inspection modified later capture or live files"
    );
    assert_eq!(tree(owner.metadata_path()), owner_history);
    assert_eq!(tree(source.metadata_path()), source_history);
    assert_eq!(tree(source.project().root()), original_files);
    assert!(storage
        .registered_dependency_versions(destination.id())
        .is_err());
    assert!(destination.saved_versions().is_err());
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn unhex(raw: &str) -> Vec<u8> {
    assert!(raw.len().is_multiple_of(2));
    raw.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}
#[test]
fn completed_history_restart_child() {
    let Ok(raw) = std::env::var(ENV) else {
        return;
    };
    let value = Json::parse(&raw).unwrap();
    let text = |key| value.get(key).and_then(Json::as_text).unwrap();
    let storage = AttachmentStorage::open(Path::new(text("storage"))).unwrap();
    let public = unhex(text("public"));
    let trusted = crate::TrustedReviewers::with_human_credentials([
        HumanApprovalCredential::from_public_key(public.as_slice().try_into().unwrap()).unwrap(),
    ]);
    if text("mode") == "refuse" {
        assert!(storage
            .inspect_native_saved_versions(text("work"), &trusted)
            .is_err());
        return;
    }
    assert_eq!(text("mode"), "read");
    let versions = storage
        .inspect_native_saved_versions(text("work"), &trusted)
        .unwrap();
    let Json::Array(expected) = value.get("versions").unwrap() else {
        panic!("fixture version vector missing");
    };
    assert_eq!(
        versions
            .iter()
            .map(|version| version.operation())
            .collect::<Vec<_>>(),
        expected
            .iter()
            .map(|item| RecordDigest::parse_hex(item.as_text().unwrap()).unwrap())
            .collect::<Vec<_>>()
    );
    let saved = *versions
        .iter()
        .find(|version| version.operation().to_hex() == text("operation"))
        .unwrap();
    assert_eq!(
        storage
            .inspect_native_saved_file(text("work"), saved, "note", &trusted)
            .unwrap(),
        Some(unhex(text("note")))
    );
}
