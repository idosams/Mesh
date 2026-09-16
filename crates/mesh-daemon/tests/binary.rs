//! The daemon as a **process**: started, talked to, and stopped.
//!
//! ```text
//! cargo nextest run -p mesh-daemon --test binary
//! ```
//!
//! # Why this file is different from `tests/ipc.rs`
//!
//! `tests/ipc.rs` binds the transport in-process and checks what the surface answers. Everything
//! in it would still pass if `crates/mesh-daemon/src/main.rs` did not exist, which is exactly the
//! state this repository was in: fourteen crates of behaviour and no way to start any of it. What
//! is under test **here** is the process — that `meshd` binds, prints where it bound, answers a
//! client that is a different process, and shuts down leaving nothing behind.
//!
//! The binaries are located with `env!("CARGO_BIN_EXE_…")`, which Cargo sets to the path of the
//! binary it just built for this test. So the thing under test is the artifact a person gets from
//! `cargo run`, not a rebuild that might differ from it.
//!
//! # Shutting the daemon down
//!
//! By closing its standard input. `src/main.rs` documents why that is the channel rather than a
//! signal: a signal handler needs `libc`, and `Cargo.lock` is governance surface. Dropping the
//! child's stdin here is the same thing a supervising parent does.

#![cfg(unix)]

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use mesh_daemon::workspace::RecordFile;
use mesh_daemon::{PreparedFolderImport, RECORD_FILE_NAME};
use mesh_operations::{
    ActorId, ActorSequence, CausalParents, ChangeSetDraft, HeadDerivation, HeadId, Hlc, ManifestId,
    NormalizedName, ObjectId, Operation, PolicyEpoch, PortableMetadata, SessionId, Signature,
    TransitionCommitment, VersionId, WorkspaceId,
};
use mesh_store::{journal_records, no_session, OperationRecord, RecordDigest, StoredRecord};

use ed25519_dalek::SigningKey;

/// How long to wait for a child to exit after its input is closed.
///
/// The daemon waits one accept poll plus one read poll before closing the socket, so anything past
/// a second means it is wedged rather than finishing.
const EXIT_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// A directory for one test's socket and workspace.
///
/// Deliberately short. A Unix-domain socket path has a hard platform limit near 104 bytes, and the
/// default temporary directory on macOS is long enough that a descriptive name spends it.
fn scratch(name: &str) -> PathBuf {
    let mut path = PathBuf::from(std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".to_owned()));
    path.push(format!("mx{name}{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("a scratch directory");
    path
}

/// A running `meshd`, and the line it printed when it was ready.
struct Daemon {
    child: Child,
    ready: String,
}

impl Daemon {
    /// Start the daemon and block until it says it is serving.
    fn start(arguments: &[&str]) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_meshd"))
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("meshd starts");

        let mut ready = String::new();
        let stdout = child.stdout.as_mut().expect("stdout is piped");
        BufReader::new(stdout)
            .read_line(&mut ready)
            .expect("meshd prints one ready line");
        if ready.is_empty() {
            let mut error = String::new();
            child
                .stderr
                .as_mut()
                .expect("stderr is piped")
                .read_to_string(&mut error)
                .expect("read startup error");
            panic!("meshd exited before ready: {error}");
        }
        assert!(
            ready.contains("\"ready\":true"),
            "meshd's first line is not a ready line: {ready}"
        );
        Self { child, ready }
    }

    /// The endpoint it printed, read out of the ready line.
    fn endpoint(&self) -> String {
        field(&self.ready, "endpoint")
    }

    /// Close its input and wait for it to finish, returning the exit code and what it said.
    fn stop(mut self) -> (Option<i32>, String) {
        drop(self.child.stdin.take());
        let deadline = std::time::Instant::now() + EXIT_WAIT;
        loop {
            match self.child.try_wait().expect("wait") {
                Some(status) => {
                    let mut said = String::new();
                    if let Some(mut err) = self.child.stderr.take() {
                        let _ = err.read_to_string(&mut said);
                    }
                    return (status.code(), said);
                }
                None if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                None => {
                    let _ = self.child.kill();
                    panic!("meshd did not stop when its input closed");
                }
            }
        }
    }
}

/// Read one string field out of a line of the daemon's own JSON. Small on purpose: this test must
/// not need a parser to check that the daemon printed a path.
fn field(line: &str, key: &str) -> String {
    let needle = format!("\"{key}\":\"");
    let start = line
        .find(&needle)
        .unwrap_or_else(|| panic!("`{key}` is not in {line}"))
        + needle.len();
    let rest = &line[start..];
    let end = rest.find('"').expect("an unterminated string");
    rest[..end].to_owned()
}

/// Run `meshctl` against an endpoint and return its exit code and its output lines.
fn meshctl(endpoint: &str, arguments: &[&str]) -> (Option<i32>, Vec<String>) {
    let output = Command::new(env!("CARGO_BIN_EXE_meshctl"))
        .arg("--endpoint")
        .arg(endpoint)
        .args(arguments)
        .output()
        .expect("meshctl runs");
    let lines = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .collect();
    (output.status.code(), lines)
}

/// Run one command whose answer comes from local durable state rather than the daemon socket.
fn meshctl_local(arguments: &[&str]) -> (Option<i32>, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_meshctl"))
        .args(arguments)
        .output()
        .expect("meshctl runs");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Import one ordinary file through the production transaction so review computation has exact
/// journal, manifest and CAS bytes rather than a hand-built identifier-only fixture.
fn save_reviewable_import(folder: &Path) -> RecordDigest {
    let source = folder.with_extension("source");
    std::fs::create_dir_all(&source).expect("review source");
    std::fs::write(source.join("notes.txt"), b"review these exact bytes\n")
        .expect("review source file");
    let prepared = PreparedFolderImport::prepare(&source, folder).expect("prepare review import");
    let (confirmed, imported) = prepared
        .confirm_into_workspace()
        .expect("confirm review import");
    drop(confirmed);
    imported.operation()
}

/// Two operations by one author and the peer they replicate with, saved to a folder for real.
fn save_three_records(folder: &Path) {
    let mut file = RecordFile::open(&folder.join(RECORD_FILE_NAME)).expect("the record file");
    let records = [
        StoredRecord::Operation(OperationRecord {
            id: RecordDigest::from_bytes([1; 32]),
            actor: RecordDigest::from_bytes([2; 32]),
            actor_sequence: 1,
            hlc_millis: 1_700_000_000_000,
            hlc_counter: 0,
            policy_epoch: 1,
            session: no_session(),
            payload_digest: RecordDigest::from_bytes([3; 32]),
            parents: Vec::new(),
        }),
        StoredRecord::Operation(OperationRecord {
            id: RecordDigest::from_bytes([4; 32]),
            actor: RecordDigest::from_bytes([2; 32]),
            actor_sequence: 2,
            hlc_millis: 1_700_000_000_001,
            hlc_counter: 0,
            policy_epoch: 1,
            session: no_session(),
            payload_digest: RecordDigest::from_bytes([5; 32]),
            parents: vec![RecordDigest::from_bytes([1; 32])],
        }),
        StoredRecord::Peer(mesh_store::PeerRecord {
            peer: RecordDigest::from_bytes([30; 32]),
            joined_at: RecordDigest::from_bytes([1; 32]),
        }),
    ];
    journal_records(&mut file, records.iter()).expect("append");
}

struct FixtureHead;

impl HeadDerivation for FixtureHead {
    fn resulting_head(&self, _commitment: &TransitionCommitment) -> HeadId {
        HeadId::from_bytes([0x70; 32])
    }
}

fn save_restore_history(
    folder: &Path,
) -> (
    ObjectId,
    VersionId,
    VersionId,
    ManifestId,
    ManifestId,
    RecordDigest,
) {
    let root = ObjectId::from_bytes([0x10; 16]);
    let file = ObjectId::from_bytes([0x20; 16]);
    let first = VersionId::from_bytes([0x31; 32]);
    let current = VersionId::from_bytes([0x32; 32]);
    let first_manifest = ManifestId::from_bytes([0x41; 32]);
    let current_manifest = ManifestId::from_bytes([0x42; 32]);
    let changeset = ChangeSetDraft::new(
        WorkspaceId::from_bytes([1; 16]),
        ActorId::from_bytes([2; 32]),
        SessionId::from_bytes([3; 16]),
        ActorSequence::new(1),
        Hlc::new(1_700_000_000_000, 0),
    )
    .causal_parents(CausalParents::genesis())
    .base_head(HeadId::from_bytes([4; 32]))
    .policy_epoch(PolicyEpoch::new(1))
    .seal(
        vec![
            Operation::CreateFile { object_id: file },
            Operation::WriteFileVersion {
                object_id: file,
                version_id: first,
                parent_versions: Vec::new(),
                manifest_id: first_manifest,
                portable_metadata: PortableMetadata::new(false),
            },
            Operation::WriteFileVersion {
                object_id: file,
                version_id: current,
                parent_versions: vec![first],
                manifest_id: current_manifest,
                portable_metadata: PortableMetadata::new(true),
            },
            Operation::LinkDirectoryEntry {
                directory_id: root,
                name: NormalizedName::new("notes.txt").expect("portable name"),
                object_id: file,
                version_id: current,
            },
        ],
        &FixtureHead,
        Signature::from_bytes([0; 64]),
    );
    let cas = mesh_cas::Cas::open(folder).expect("payload store");
    let payload = cas
        .promote(mesh_operations::encode_canonical(&changeset))
        .expect("saved ChangeSet payload")
        .digest();
    let payload = RecordDigest::from_bytes(*payload.as_bytes());
    let record = StoredRecord::Operation(OperationRecord {
        id: payload,
        actor: RecordDigest::from_bytes([2; 32]),
        actor_sequence: 1,
        hlc_millis: 1_700_000_000_000,
        hlc_counter: 0,
        policy_epoch: 1,
        session: mesh_store::EntityUuid::from_bytes([3; 16]),
        payload_digest: payload,
        parents: Vec::new(),
    });
    let mut journal = RecordFile::open(&folder.join(RECORD_FILE_NAME)).expect("record file");
    journal_records(&mut journal, [&record]).expect("durable history");
    (
        file,
        first,
        current,
        first_manifest,
        current_manifest,
        payload,
    )
}

#[test]
fn restore_preview_is_exact_read_only_restart_stable_and_refuses_a_noop() {
    let scratch = scratch("restore-preview");
    let workspace = scratch.join("w");
    let (file, target, current, target_manifest, current_manifest, _) =
        save_restore_history(&workspace);
    let record_file = workspace.join(RECORD_FILE_NAME);
    let journal_before = std::fs::read(&record_file).expect("journal before preview");
    let arguments = [
        "restore-preview",
        workspace.to_str().expect("utf8 path"),
        &file.to_string(),
        &target.to_string(),
    ];

    let (code, first, error) = meshctl_local(&arguments);
    assert_eq!(code, Some(0), "preview failed: {error}");
    assert!(first.contains("\"schema\":\"mesh.restore-preview/v1\""));
    assert!(first.contains(&format!("\"version_id\":\"{current}\"")));
    assert!(first.contains(&format!("\"manifest_id\":\"{current_manifest}\"")));
    assert!(first.contains(&format!("\"version_id\":\"{target}\"")));
    assert!(first.contains(&format!("\"manifest_id\":\"{target_manifest}\"")));
    let mut after = 0;
    for kind in [
        "UnlinkDirectoryEntry",
        "DeleteObject",
        "RestoreObject",
        "LinkDirectoryEntry",
    ] {
        let needle = format!("\"kind\":\"{kind}\"");
        let found = first[after..]
            .find(&needle)
            .unwrap_or_else(|| panic!("{kind} was absent or out of order: {first}"));
        after += found + needle.len();
    }
    assert!(first.contains("\"undo_possible\":true"));
    assert!(first.contains("\"canonical_hex\":"));
    assert!(first.contains("\"canonical_state_read_only\":true"));
    assert!(first.contains("\"execution_authorized\":false"));

    let (code, restarted, error) = meshctl_local(&arguments);
    assert_eq!(code, Some(0), "restart preview failed: {error}");
    assert_eq!(
        restarted, first,
        "the same journal must project identically"
    );
    assert_eq!(
        std::fs::read(&record_file).expect("journal after preview"),
        journal_before,
        "preview must never append canonical state"
    );

    let current_text = current.to_string();
    let refusal_arguments = [
        "restore-preview",
        workspace.to_str().expect("utf8 path"),
        &file.to_string(),
        &current_text,
    ];
    let (code, refusal, error) = meshctl_local(&refusal_arguments);
    assert_eq!(
        code,
        Some(1),
        "no-op refusal was not a product refusal: {error}"
    );
    assert!(refusal.contains("\"refused\":true"), "{refusal}");
    assert!(refusal.contains("already visible"), "{refusal}");
    assert_eq!(
        std::fs::read(&record_file).expect("journal after refusal"),
        journal_before,
        "a refusal must not append canonical state"
    );
    let _ = std::fs::remove_dir_all(scratch);
}

#[test]
fn an_open_review_projects_the_verified_bundle_without_approval_authority() {
    let scratch = scratch("review-card");
    let socket = scratch.join("d.sock");
    let workspace = scratch.join("w");
    let target = save_reviewable_import(&workspace);
    let key_path = scratch.join("reviewer.key");
    std::fs::write(&key_path, [21u8; 32]).expect("write review-only actor seed");
    std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))
        .expect("protect actor seed fixture");

    let daemon = Daemon::start(&[
        "--endpoint",
        &socket.display().to_string(),
        "--workspace",
        &workspace.display().to_string(),
    ]);
    let endpoint = daemon.endpoint();
    let (code, opened) = meshctl(
        &endpoint,
        &["review-current", &key_path.display().to_string()],
    );
    assert_eq!(code, Some(0), "open review: {opened:?}");
    let answer = &opened[1];
    let bundle = field(answer, "bundle");
    assert!(answer.contains("\"review_items\":[{"), "{answer}");
    assert!(
        answer.contains(&format!("\"bundle\":\"{bundle}\"")),
        "{answer}"
    );
    assert!(
        answer.contains(&format!("\"subject_operation\":\"{target}\"")),
        "{answer}"
    );
    assert!(answer.contains("\"actor_sequence\":\"1\""), "{answer}");
    assert!(answer.contains("\"kind\":\"CreateFile\""), "{answer}");
    assert!(answer.contains("\"kind\":\"WriteFileVersion\""), "{answer}");
    assert!(answer.contains("\"presentation_digest\":"), "{answer}");
    assert!(answer.contains("\"bundle_changes\":[{"), "{answer}");
    assert!(answer.contains("\"path_after\":\"/notes.txt\""), "{answer}");
    assert!(answer.contains("\"body\":\"binary\""), "{answer}");
    assert!(answer.contains("\"verified_text\":{"), "{answer}");
    assert!(
        answer.contains("\"text\":\"review these exact bytes\""),
        "{answer}"
    );
    assert!(answer.contains("\"content_complete\":true"), "{answer}");
    assert!(
        answer.contains("\"projection_authorizes_approval\":false"),
        "{answer}"
    );

    let (code, restarted) = daemon.stop();
    assert_eq!(code, Some(0), "meshd did not exit cleanly: {restarted}");
    let daemon = Daemon::start(&[
        "--endpoint",
        &socket.display().to_string(),
        "--workspace",
        &workspace.display().to_string(),
    ]);
    let endpoint = daemon.endpoint();
    let (code, state) = meshctl(&endpoint, &["state"]);
    assert_eq!(code, Some(0), "restart state: {state:?}");
    assert!(state[1].contains(&format!("\"bundle\":\"{bundle}\"")));
    assert!(state[1].contains("\"content_complete\":true"));
    assert!(state[1].contains("\"text\":\"review these exact bytes\""));

    let (code, output) = daemon.stop();
    assert_eq!(code, Some(0), "meshd did not exit cleanly: {output}");
    let _ = std::fs::remove_dir_all(scratch);
}

#[test]
fn the_daemon_starts_answers_a_separate_process_and_stops_cleanly() {
    let scratch = scratch("run");
    let socket = scratch.join("d.sock");
    let workspace = scratch.join("w");
    std::fs::create_dir_all(&workspace).expect("the workspace folder");
    save_three_records(&workspace);

    let daemon = Daemon::start(&[
        "--endpoint",
        &socket.display().to_string(),
        "--workspace",
        &workspace.display().to_string(),
    ]);
    let endpoint = daemon.endpoint();
    assert_eq!(endpoint, socket.display().to_string());
    assert!(Path::new(&endpoint).exists(), "nothing is listening");
    assert!(
        daemon.ready.contains("\"serving\":true"),
        "the daemon did not say it was serving: {}",
        daemon.ready
    );

    // Health, from a different process entirely.
    let (code, lines) = meshctl(&endpoint, &["status"]);
    assert_eq!(code, Some(0), "meshctl status: {lines:?}");
    assert!(lines[0].starts_with("{\"t\":\"welcome\""), "{lines:?}");
    assert!(lines[1].contains("\"serving\":true"), "{lines:?}");

    // The workspace it was started with, with the records that are really on disk.
    let (code, lines) = meshctl(&endpoint, &["state"]);
    assert_eq!(code, Some(0), "meshctl state: {lines:?}");
    assert!(lines[1].contains("\"records\":3"), "{lines:?}");
    assert!(lines[1].contains("\"operations\":2"), "{lines:?}");
    assert!(lines[1].contains("\"peers\":1"), "{lines:?}");
    assert!(
        lines[1].contains("\"not_yet\""),
        "the honest refusals are not on the wire: {lines:?}"
    );

    // Opening a second, empty folder over the socket.
    let second = scratch.join("w2");
    let (code, lines) = meshctl(&endpoint, &["open", &second.display().to_string()]);
    assert_eq!(code, Some(0), "meshctl open: {lines:?}");
    assert!(lines[1].contains("\"records\":0"), "{lines:?}");
    assert!(
        second.join(".mesh").join(RECORD_FILE_NAME).exists(),
        "opening a new folder did not create the file that holds saved work"
    );

    let (code, output) = daemon.stop();
    assert_eq!(code, Some(0), "meshd did not exit cleanly: {output}");
    assert!(
        !Path::new(&endpoint).exists(),
        "the socket file outlived the daemon"
    );
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn configured_reviewer_key_does_not_claim_humanheld_publication_authority() {
    let scratch = scratch("trusted");
    let socket = scratch.join("d.sock");
    let workspace = scratch.join("w");
    let key = "07".repeat(32);
    let daemon = Daemon::start(&[
        "--endpoint",
        &socket.display().to_string(),
        "--workspace",
        &workspace.display().to_string(),
        "--trusted-reviewer-key",
        &key,
    ]);
    let endpoint = daemon.endpoint();

    let (code, lines) = meshctl(&endpoint, &["state"]);
    assert_eq!(code, Some(0), "meshctl state: {lines:?}");
    assert!(lines[1].contains("\"shared_version\":null"), "{lines:?}");
    assert!(
        lines[1].contains("\"subject\":\"shared version\""),
        "a configured public key is not HumanHeld authority: {lines:?}"
    );

    let (code, output) = daemon.stop();
    assert_eq!(code, Some(0), "meshd did not exit cleanly: {output}");
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn a_software_seed_can_open_a_review_but_cannot_advance_shared_version() {
    let scratch = scratch("publish");
    let socket = scratch.join("d.sock");
    let workspace = scratch.join("w");
    let target = save_reviewable_import(&workspace);

    let key_path = scratch.join("reviewer.key");
    std::fs::write(&key_path, [11u8; 32]).expect("write reviewer seed");
    std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))
        .expect("protect reviewer seed");
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let public_key = hex(signing_key.verifying_key().as_bytes());

    let daemon = Daemon::start(&[
        "--endpoint",
        &socket.display().to_string(),
        "--workspace",
        &workspace.display().to_string(),
        "--trusted-reviewer-key",
        &public_key,
    ]);
    let endpoint = daemon.endpoint();

    let (code, lines) = meshctl(
        &endpoint,
        &["review-current", &key_path.display().to_string()],
    );
    assert_eq!(code, Some(0), "open review: {lines:?}");
    let bundle = field(&lines[1], "bundle");
    assert!(lines[1].contains("\"reviews\":1"), "{lines:?}");
    assert!(
        lines[1].contains("\"content_complete\":true"),
        "the generated review did not project its exact durable content: {lines:?}"
    );

    let missing_key = scratch.join("must-not-be-opened.key");
    let (code, _stdout, stderr) = meshctl_local(&[
        "--endpoint",
        &endpoint,
        "approve",
        &bundle,
        &target.to_string(),
        "genesis",
        &missing_key.display().to_string(),
    ]);
    assert_eq!(code, Some(2), "software approval must be unavailable");
    assert!(
        stderr.contains("no verified human-held signing authority"),
        "{stderr}"
    );
    assert!(!missing_key.exists(), "the unavailable path opened a key");

    let (code, lines) = meshctl(&endpoint, &["state"]);
    assert_eq!(code, Some(0), "state: {lines:?}");
    assert!(lines[1].contains("\"shared_version\":null"), "{lines:?}");
    assert!(
        lines[1].contains("\"subject\":\"shared version\""),
        "{lines:?}"
    );

    let (code, output) = daemon.stop();
    assert_eq!(code, Some(0), "meshd did not exit cleanly: {output}");
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn meshctl_refuses_an_exposed_reviewer_key_before_connecting() {
    let scratch = scratch("keymode");
    let key_path = scratch.join("reviewer.key");
    std::fs::write(&key_path, [12u8; 32]).expect("write reviewer seed");
    std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o644))
        .expect("expose reviewer seed fixture");
    let output = Command::new(env!("CARGO_BIN_EXE_meshctl"))
        .args([
            "--endpoint",
            "/tmp/mesh-does-not-exist.sock",
            "review-current",
            &key_path.display().to_string(),
        ])
        .output()
        .expect("meshctl runs");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("permissions 0600"), "{stderr}");
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn a_subscribed_client_is_told_what_the_daemon_does_next() {
    let scratch = scratch("watch");
    let socket = scratch.join("d.sock");
    let daemon = Daemon::start(&["--endpoint", &socket.display().to_string()]);
    let endpoint = daemon.endpoint();

    // Two entries are already in the feed for a late subscriber: the daemon started serving, and
    // — after this call — a folder was opened. `watch 2` waits for both.
    let opener = std::thread::spawn({
        let endpoint = endpoint.clone();
        let folder = scratch.join("w");
        move || {
            std::thread::sleep(std::time::Duration::from_millis(150));
            meshctl(&endpoint, &["open", &folder.display().to_string()])
        }
    });

    let (code, lines) = meshctl(&endpoint, &["watch", "2"]);
    assert_eq!(code, Some(0), "meshctl watch: {lines:?}");
    let (open_code, open_lines) = opener.join().expect("the opening thread");
    assert_eq!(open_code, Some(0), "meshctl open: {open_lines:?}");

    let events: Vec<&String> = lines
        .iter()
        .filter(|line| line.starts_with("{\"t\":\"event\""))
        .collect();
    assert_eq!(events.len(), 2, "expected two pushed lines: {lines:?}");
    assert!(events[0].contains("\"kind\":\"serving\""), "{events:?}");
    assert!(
        events[1].contains("\"kind\":\"workspace-opened\""),
        "{events:?}"
    );

    let (code, _) = daemon.stop();
    assert_eq!(code, Some(0));
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn a_folder_that_cannot_be_read_is_refused_with_a_code_and_the_daemon_keeps_serving() {
    let scratch = scratch("bad");
    let socket = scratch.join("d.sock");
    let broken = scratch.join("w");
    std::fs::create_dir_all(&broken).expect("mkdir");
    let mut framed = mesh_store::frame_record(&StoredRecord::Peer(mesh_store::PeerRecord {
        peer: RecordDigest::from_bytes([7; 32]),
        joined_at: RecordDigest::from_bytes([1; 32]),
    }));
    let last = framed.len() - 1;
    framed[last] ^= 0xFF;
    std::fs::write(broken.join(RECORD_FILE_NAME), &framed).expect("write");

    let daemon = Daemon::start(&["--endpoint", &socket.display().to_string()]);
    let endpoint = daemon.endpoint();

    let (code, lines) = meshctl(&endpoint, &["open", &broken.display().to_string()]);
    assert_eq!(code, Some(1), "a refusal is exit 1, not a crash: {lines:?}");
    assert!(
        lines[1].contains("\"code\":\"workspace-damaged\""),
        "{lines:?}"
    );

    // Still serving. A daemon that falls over when one folder is bad is a daemon that cannot tell
    // anybody which folder was bad.
    let (code, lines) = meshctl(&endpoint, &["status"]);
    assert_eq!(code, Some(0), "{lines:?}");
    assert!(lines[1].contains("\"serving\":true"), "{lines:?}");

    let (code, _) = daemon.stop();
    assert_eq!(code, Some(0));
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn a_daemon_started_on_a_stale_socket_file_binds_anyway() {
    let scratch = scratch("stale");
    let socket = scratch.join("d.sock");
    // What a daemon killed with a signal leaves behind.
    let listener = std::os::unix::net::UnixListener::bind(&socket).expect("a socket in the way");
    drop(listener);

    let daemon = Daemon::start(&["--endpoint", &socket.display().to_string()]);
    let endpoint = daemon.endpoint();
    let (code, lines) = meshctl(&endpoint, &["status"]);
    assert_eq!(code, Some(0), "{lines:?}");

    let (code, _) = daemon.stop();
    assert_eq!(code, Some(0));
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn the_daemon_stops_on_the_word_stop_as_well_as_on_end_of_input() {
    let scratch = scratch("word");
    let socket = scratch.join("d.sock");
    let mut daemon = Daemon::start(&["--endpoint", &socket.display().to_string()]);
    let mut stdin = daemon.child.stdin.take().expect("stdin is piped");
    writeln!(stdin, "stop").expect("write");
    stdin.flush().expect("flush");

    let deadline = std::time::Instant::now() + EXIT_WAIT;
    loop {
        match daemon.child.try_wait().expect("wait") {
            Some(status) => {
                assert_eq!(status.code(), Some(0));
                break;
            }
            None if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            None => {
                let _ = daemon.child.kill();
                panic!("meshd ignored `stop`");
            }
        }
    }
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn the_help_text_and_the_version_are_answered_without_starting_anything() {
    for binary in [env!("CARGO_BIN_EXE_meshd"), env!("CARGO_BIN_EXE_meshctl")] {
        let output = Command::new(binary).arg("--help").output().expect("runs");
        assert_eq!(output.status.code(), Some(0), "{binary} --help");
        assert!(
            !output.stdout.is_empty(),
            "{binary} --help printed nothing at all"
        );
    }
    let version = Command::new(env!("CARGO_BIN_EXE_meshd"))
        .arg("--version")
        .output()
        .expect("runs");
    assert_eq!(version.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&version.stdout).starts_with("meshd "));
}

#[test]
fn an_option_the_service_does_not_have_stops_it_before_it_listens() {
    let output = Command::new(env!("CARGO_BIN_EXE_meshd"))
        .arg("--mount")
        .stdin(Stdio::null())
        .output()
        .expect("runs");
    assert_eq!(output.status.code(), Some(2), "an unknown option is exit 2");
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(said.contains("--mount"), "{said}");
    assert!(said.contains("nothing was changed"), "{said}");
}

#[test]
fn duplicate_singleton_options_stop_the_daemon_before_either_endpoint_is_bound() {
    let scratch = scratch("duplicate-option");
    let first = scratch.join("first.sock");
    let second = scratch.join("second.sock");
    let output = Command::new(env!("CARGO_BIN_EXE_meshd"))
        .arg("--endpoint")
        .arg(&first)
        .arg(format!("--endpoint={}", second.display()))
        .output()
        .expect("meshd runs");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty(), "no ready line may be emitted");
    let problem = String::from_utf8_lossy(&output.stderr);
    assert!(problem.contains("--endpoint"), "{problem}");
    assert!(problem.contains("only once"), "{problem}");
    assert!(!first.exists(), "the first endpoint was bound");
    assert!(!second.exists(), "the second endpoint was bound");
}

#[test]
fn a_client_pointed_at_nothing_says_so_rather_than_hanging() {
    let scratch = scratch("absent");
    let (code, lines) = meshctl(
        &scratch.join("nothing.sock").display().to_string(),
        &["status"],
    );
    assert_eq!(code, Some(2), "unreachable is exit 2: {lines:?}");
    let _ = std::fs::remove_dir_all(&scratch);
}

/// The exclusion report is answered by the binary a person actually runs, with no service up.
///
/// This is the acceptance criterion in its literal form: *"a user never has to read a configuration
/// file by hand to learn why a path is not versioned."* The assertion below is that the printed
/// line names the source and the rule, and that the whole thing works with no `--endpoint` and no
/// daemon anywhere.
#[test]
fn the_exclusion_report_is_answered_by_the_binary_with_no_service_running() {
    let workspace = scratch("excl");
    std::fs::create_dir_all(workspace.join("mounts")).expect("a mount root");
    std::fs::write(workspace.join("mounts/.gitignore"), "build/\n").expect("ignore rules");
    std::fs::write(workspace.join(".meshignore"), "target/\n!build/keep\n").expect("mesh rules");

    let run = |arguments: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_meshctl"))
            .args(arguments)
            .output()
            .expect("runs");
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).to_string(),
            String::from_utf8_lossy(&output.stderr).to_string(),
        )
    };
    let folder = workspace.display().to_string();

    // The whole effective set, with every source named even when it supplied nothing.
    let (code, printed, complained) = run(&["exclusions", &folder]);
    assert_eq!(code, Some(0), "stdout={printed} stderr={complained}");
    for expected in [
        ".meshignore",
        "repository ignore rules",
        "workspace configuration",
    ] {
        assert!(
            printed.contains(expected),
            "{expected} missing from {printed}"
        );
    }

    // One path, and which source said so — the compiler's output, excluded by the Mesh-native file.
    let (code, printed, _) = run(&["exclusions", &folder, "target/debug/mesh"]);
    assert_eq!(code, Some(0));
    assert!(printed.contains("not-versioned"), "{printed}");
    assert!(printed.contains(".meshignore"), "{printed}");

    // A lower-precedence source, still named exactly.
    let (_, printed, _) = run(&["exclusions", &folder, "build/out/a.o"]);
    assert!(printed.contains("repository ignore rules"), "{printed}");

    // And the re-inclusion, which is the case a user is most likely to be confused by.
    let (_, printed, _) = run(&["exclusions", &folder, "build/keep/note.md"]);
    assert!(printed.contains("put back by"), "{printed}");

    let _ = std::fs::remove_dir_all(&workspace);
}

/// Acceptance criterion 1 of task `01KZC2QR9VVJK6Y60PS8D360JT`, at the surface a person uses.
///
/// Everything else about the folder-watching fallback is checked inside the process by
/// `tests/folder-watch.rs`. What is under test here is the only thing that decides whether a
/// person ever learns any of it: **that starting the service says which mechanism it chose, and
/// that a terminal can list what that mechanism cannot see without a service running at all.**
///
/// Both halves are asserted against `FallbackRestriction::ALL` rather than against seven strings
/// written out here, so a restriction added to the product without a way to reach a person fails
/// this test rather than passing it quietly.
#[test]
fn starting_the_service_says_which_mechanism_it_chose_and_what_it_misses() {
    let socket = scratch("bk").join("d.sock");
    let daemon = Daemon::start(&["--endpoint", &socket.display().to_string()]);

    // On the machine line a supervising parent reads. It never reads standard error, so a
    // mechanism announced only in prose is a mechanism half the callers never see.
    assert!(
        daemon.ready.contains("\"backend\":\"folder-watch\""),
        "the ready line does not say which mechanism was chosen: {}",
        daemon.ready
    );
    assert!(
        daemon.ready.contains("\"authoritative\":false"),
        "the fallback was presented as the authoritative mechanism: {}",
        daemon.ready
    );

    let (_, said) = daemon.stop();
    assert!(
        said.contains(mesh_daemon::user_messages::FALLBACK_IN_USE),
        "starting the service never said the fallback was in use: {said}"
    );
    for restriction in mesh_daemon::FallbackRestriction::ALL {
        assert!(
            said.contains(restriction.headline()),
            "{} was never said to the person who started the service: {said}",
            restriction.id()
        );
    }

    // And from a terminal, with nothing running: `meshctl restrictions` needs no `--endpoint`,
    // because a person deciding whether to trust Mesh with a folder should not have to start a
    // service to be told what it cannot see.
    let output = Command::new(env!("CARGO_BIN_EXE_meshctl"))
        .arg("restrictions")
        .output()
        .expect("meshctl runs");
    assert_eq!(output.status.code(), Some(0));
    let printed = String::from_utf8_lossy(&output.stdout).to_string();
    assert!(
        printed.contains("\"backend\":\"folder-watch\""),
        "{printed}"
    );
    assert!(printed.contains("\"authoritative\":false"), "{printed}");
    for restriction in mesh_daemon::FallbackRestriction::ALL {
        assert!(
            printed.contains(restriction.id()),
            "{} is not on the list a person can read: {printed}",
            restriction.id()
        );
        assert!(
            printed.contains(restriction.headline()),
            "{} reaches a person as a machine name with no sentence: {printed}",
            restriction.id()
        );
    }
}
