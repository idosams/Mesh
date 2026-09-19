#![cfg(unix)]

//! Real daemon IPC coverage for the desktop local-folder management journey.

use std::fs;
use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use mesh_daemon::ipc::{
    nothing_to_recover, ClientMessage, DaemonMessage, IpcServer, Json, StartupSummary,
    SURFACE_VERSION,
};
use mesh_daemon::LiveDaemon;

struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next: u64,
}

impl Client {
    fn connect(endpoint: &Path) -> Self {
        let stream = UnixStream::connect(endpoint).expect("connect desktop client");
        let mut client = Self {
            reader: BufReader::new(stream.try_clone().expect("clone socket")),
            writer: stream,
            next: 1,
        };
        let hello = ClientMessage::Hello {
            id: 1,
            versions: vec![SURFACE_VERSION],
            session: "desktop-management-test".to_owned(),
        };
        assert!(matches!(
            client.exchange(hello),
            DaemonMessage::Welcome {
                version: SURFACE_VERSION,
                ..
            }
        ));
        client.next = 2;
        client
    }

    fn call(&mut self, method: &str, params: Json) -> Result<Json, (String, String)> {
        let id = self.next;
        self.next += 1;
        match self.exchange(ClientMessage::Call {
            id,
            method: method.to_owned(),
            version: SURFACE_VERSION,
            params,
        }) {
            DaemonMessage::Result { id: answer, value } if answer == id => Ok(value),
            DaemonMessage::Failed {
                id: answer,
                code,
                message,
            } if answer == id => Err((code, message)),
            other => panic!("unexpected desktop answer: {other:?}"),
        }
    }

    fn exchange(&mut self, message: ClientMessage) -> DaemonMessage {
        writeln!(self.writer, "{}", message.encode()).expect("write request");
        self.writer.flush().expect("flush request");
        let mut line = String::new();
        self.reader.read_line(&mut line).expect("read reply");
        DaemonMessage::decode(line.trim_end()).expect("decode reply")
    }
}

fn scratch(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    // Unix-domain sockets have a small path limit on macOS. Keep this root under `/tmp` instead
    // of the much longer per-user `TMPDIR` path so the real transport is what the test measures.
    let root = PathBuf::from("/tmp").join(format!(
        "mesh-desktop-{label}-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("scratch root");
    root
}

fn start(root: &Path) -> (mesh_daemon::ipc::ServerHandle, PathBuf) {
    let endpoint = root.join("daemon.sock");
    let daemon = Arc::new(LiveDaemon::new(StartupSummary::from(&nothing_to_recover())));
    let handle = IpcServer::bind(&endpoint)
        .expect("bind")
        .spawn(daemon)
        .expect("spawn");
    (handle, endpoint)
}

#[test]
fn desktop_ipc_imports_reopens_and_rolls_back_without_touching_the_original() {
    let root = scratch("journey");
    let source = root.join("original");
    let managed = root.join("managed");
    fs::create_dir_all(source.join("docs")).unwrap();
    fs::write(source.join("README.md"), b"hello from Mesh\n").unwrap();
    fs::write(source.join("docs/notes.txt"), b"private notes\n").unwrap();
    let original_before = fs::read(source.join("README.md")).unwrap();

    let (server, endpoint) = start(&root);
    let mut client = Client::connect(&endpoint);
    let preview = client
        .call(
            "folder.import.preview",
            Json::object([("source", Json::text(source.to_string_lossy()))]),
        )
        .expect("preview");
    assert_eq!(preview.get("files").and_then(Json::as_u64), Some(2));
    let summary = preview
        .get("summary")
        .and_then(Json::as_text)
        .unwrap()
        .to_owned();
    let confirmed = client
        .call(
            "folder.import.confirm",
            Json::object([
                ("source", Json::text(source.to_string_lossy())),
                ("destination", Json::text(managed.to_string_lossy())),
                ("summary", Json::text(summary)),
            ]),
        )
        .expect("confirm");
    assert_eq!(
        confirmed.get("private_history").and_then(Json::as_bool),
        Some(true)
    );
    let presented = PathBuf::from(
        confirmed
            .get("destination")
            .and_then(Json::as_text)
            .expect("presented working folder"),
    );
    assert!(managed.join(mesh_daemon::RECORD_FILE_NAME).is_file());
    assert!(!presented.join(mesh_daemon::RECORD_FILE_NAME).exists());
    assert_eq!(
        presented,
        managed.join(mesh_daemon::workspace::PRESENTED_DIRECTORY_NAME)
    );
    assert_eq!(fs::read(source.join("README.md")).unwrap(), original_before);

    drop(client);
    server.shutdown();
    let (server, endpoint) = start(&root);
    let mut restarted = Client::connect(&endpoint);
    let state = restarted
        .call(
            "workspace.open",
            Json::object([("path", Json::text(presented.to_string_lossy()))]),
        )
        .expect("reopen durable workspace");
    assert_eq!(state.get("records").and_then(Json::as_u64), Some(3));
    assert_eq!(
        state.get("entries").and_then(Json::as_array).unwrap().len(),
        3
    );
    let histories = state
        .get("file_histories")
        .and_then(Json::as_array)
        .expect("discoverable retained histories");
    assert_eq!(histories.len(), 2, "one row per imported regular file");
    for history in histories {
        assert!(history.get("object_id").and_then(Json::as_text).is_some());
        assert!(!matches!(history.get("current"), None | Some(Json::Null)));
        assert_eq!(
            history
                .get("retained_versions")
                .and_then(Json::as_array)
                .map(<[_]>::len),
            Some(1),
            "the imported version is retained and discoverable after restart"
        );
    }
    let custody_daemon = LiveDaemon::new(StartupSummary::from(&nothing_to_recover()));
    let custody_workspace = custody_daemon
        .reopen_existing_workspace(&presented)
        .expect("open endpoint workspace for shared custody");
    let custody_generation = custody_daemon
        .acquire_workspace_agent_custody(
            &custody_workspace.root,
            &custody_workspace.digest,
            &custody_workspace.installation,
            false,
            None,
        )
        .expect("acquire endpoint-visible custody");
    let assigned = restarted
        .call(
            "folder.import.rollback",
            Json::object([("destination", Json::text(presented.to_string_lossy()))]),
        )
        .expect_err("endpoint rollback must honor another process's custody");
    assert_eq!(assigned.0, "workspace-agent-custody-active");
    assert!(managed.is_dir());
    custody_daemon
        .release_workspace_agent_custody(
            &custody_workspace.root,
            &custody_workspace.digest,
            &custody_workspace.installation,
            &custody_generation,
        )
        .expect("release endpoint-visible custody");
    let rolled_back = restarted
        .call(
            "folder.import.rollback",
            Json::object([("destination", Json::text(presented.to_string_lossy()))]),
        )
        .expect("rollback");
    assert_eq!(
        rolled_back
            .get("original_preserved")
            .and_then(Json::as_bool),
        Some(true)
    );
    assert!(!managed.exists());
    assert_eq!(fs::read(source.join("README.md")).unwrap(), original_before);
    drop(restarted);
    server.shutdown();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn desktop_ipc_refuses_a_stale_preview_and_removes_the_unconfirmed_copy() {
    let root = scratch("stale");
    let source = root.join("original");
    let managed = root.join("managed");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("file.txt"), b"first\n").unwrap();
    let (server, endpoint) = start(&root);
    let mut client = Client::connect(&endpoint);
    let failure = client
        .call(
            "folder.import.confirm",
            Json::object([
                ("source", Json::text(source.to_string_lossy())),
                ("destination", Json::text(managed.to_string_lossy())),
                ("summary", Json::text("00".repeat(32))),
            ]),
        )
        .expect_err("stale summary must fail");
    assert_eq!(failure.0, "folder-import-preview-changed");
    assert!(!managed.exists());
    assert!(source.join("file.txt").is_file());
    drop(client);
    server.shutdown();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn desktop_ipc_refuses_changed_workspace_rollback_and_keeps_it_open() {
    let root = scratch("changed");
    let source = root.join("original");
    let managed = root.join("managed");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("file.txt"), b"first\n").unwrap();
    let (server, endpoint) = start(&root);
    let mut client = Client::connect(&endpoint);
    let preview = client
        .call(
            "folder.import.preview",
            Json::object([("source", Json::text(source.to_string_lossy()))]),
        )
        .expect("preview");
    let summary = preview
        .get("summary")
        .and_then(Json::as_text)
        .expect("summary")
        .to_owned();
    let confirmed = client
        .call(
            "folder.import.confirm",
            Json::object([
                ("source", Json::text(source.to_string_lossy())),
                ("destination", Json::text(managed.to_string_lossy())),
                ("summary", Json::text(summary)),
            ]),
        )
        .expect("confirm");
    let presented = PathBuf::from(
        confirmed
            .get("destination")
            .and_then(Json::as_text)
            .expect("presented working folder"),
    );
    fs::write(presented.join("file.txt"), b"changed managed work\n").unwrap();

    let failure = client
        .call(
            "folder.import.rollback",
            Json::object([("destination", Json::text(presented.to_string_lossy()))]),
        )
        .expect_err("changed managed work must not be deleted");
    assert_eq!(failure.0, "folder-import-refused");
    assert!(presented.join("file.txt").is_file());
    let state = client
        .call("workspace.state", Json::empty_object())
        .expect("failed rollback keeps the workspace open");
    assert_eq!(
        state.get("root").and_then(Json::as_text),
        Some(presented.to_string_lossy().as_ref())
    );

    drop(client);
    server.shutdown();
    fs::remove_dir_all(root).unwrap();
}
