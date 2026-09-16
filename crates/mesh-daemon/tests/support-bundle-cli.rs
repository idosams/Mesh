//! Real-process support preview must never turn an import source into a Mesh workspace.

use std::fs;
use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use mesh_daemon::ipc::{ClientMessage, DaemonMessage, Json, SURFACE_VERSION};
use mesh_daemon::{OpenWorkspace, SupportBundle};

fn scratch(name: &str) -> PathBuf {
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    let serial = SERIAL.fetch_add(1, Ordering::SeqCst);
    let mut root = PathBuf::from(std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".to_owned()));
    root.push(format!("msb-{name}-{}-{serial}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("scratch directory");
    root
}

struct Daemon(Child);

impl Daemon {
    fn start(workspace: &Path, endpoint: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_meshd"))
            .arg("--workspace")
            .arg(workspace)
            .arg("--endpoint")
            .arg(endpoint)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("meshd starts");
        let mut ready = String::new();
        BufReader::new(child.stdout.as_mut().expect("stdout"))
            .read_line(&mut ready)
            .expect("ready line");
        assert!(ready.contains("\"ready\":true"), "{ready}");
        Self(child)
    }

    fn stop(mut self) {
        drop(self.0.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if self.0.try_wait().expect("daemon status").is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.0.kill();
        panic!("meshd did not stop");
    }
}

fn tree_bytes(root: &Path) -> Vec<(PathBuf, Option<Vec<u8>>)> {
    fn visit(root: &Path, relative: &Path, found: &mut Vec<(PathBuf, Option<Vec<u8>>)>) {
        let mut entries: Vec<_> = fs::read_dir(root.join(relative))
            .expect("read directory")
            .map(|entry| entry.expect("directory entry"))
            .collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let child = relative.join(entry.file_name());
            if entry.file_type().expect("entry type").is_dir() {
                found.push((child.clone(), None));
                visit(root, &child, found);
            } else {
                found.push((
                    child.clone(),
                    Some(fs::read(root.join(&child)).expect("file bytes")),
                ));
            }
        }
    }

    let mut found = Vec::new();
    visit(root, Path::new(""), &mut found);
    found
}

#[test]
fn support_bundle_process_is_read_only_over_a_plain_import_source() {
    let root = std::env::temp_dir().join(format!(
        "mesh-support-bundle-process-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src/empty")).expect("source directories");
    fs::write(root.join("README.md"), b"keep these bytes\n").expect("source file");
    let before = tree_bytes(&root);

    let output = Command::new(env!("CARGO_BIN_EXE_meshctl"))
        .arg("support-bundle")
        .arg(&root)
        .output()
        .expect("meshctl starts");

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let preview = std::str::from_utf8(&output.stdout).expect("utf8 output");
    let document = mesh_daemon::ipc::Json::parse(preview.trim()).expect("one JSON document");
    assert_eq!(
        document
            .get("crash-diagnostics")
            .and_then(|value| value.get("serving"))
            .and_then(mesh_daemon::ipc::Json::as_bool),
        Some(false)
    );
    assert_eq!(
        tree_bytes(&root),
        before,
        "meshctl mutated the import source"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn live_support_bundle_uses_the_matching_daemons_verified_state() {
    let root = scratch("live");
    let workspace = root.join("workspace");
    let endpoint = root.join("daemon.sock");
    fs::create_dir_all(&workspace).expect("workspace");
    let daemon = Daemon::start(&workspace, &endpoint);

    let output = Command::new(env!("CARGO_BIN_EXE_meshctl"))
        .arg("--endpoint")
        .arg(&endpoint)
        .arg("support-bundle")
        .arg(&workspace)
        .output()
        .expect("meshctl starts");
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let text = std::str::from_utf8(&output.stdout).expect("utf8");
    assert_eq!(text.lines().count(), 1, "one support document: {text}");
    assert!(
        !text.contains(workspace.to_str().expect("utf8 path")),
        "private workspace path leaked: {text}"
    );
    let document = mesh_daemon::ipc::Json::parse(text.trim()).expect("support document");
    let live_correlation = document
        .get("workspace_correlation")
        .and_then(mesh_daemon::ipc::Json::as_text)
        .expect("live correlation")
        .to_owned();
    assert_eq!(
        document
            .get("crash-diagnostics")
            .and_then(|value| value.get("serving"))
            .and_then(mesh_daemon::ipc::Json::as_bool),
        Some(true),
        "a healthy live WAL family was mislabeled: {text}"
    );

    let other = root.join("other");
    fs::create_dir(&other).expect("other workspace");
    drop(OpenWorkspace::open(&other).expect("initialize other workspace"));
    let mismatch = Command::new(env!("CARGO_BIN_EXE_meshctl"))
        .arg("--endpoint")
        .arg(&endpoint)
        .arg("support-bundle")
        .arg(&other)
        .output()
        .expect("mismatched meshctl starts");
    assert_eq!(mismatch.status.code(), Some(2), "{mismatch:?}");
    assert!(mismatch.stdout.is_empty(), "{mismatch:?}");
    assert!(
        String::from_utf8_lossy(&mismatch.stderr).contains("different workspace open"),
        "{mismatch:?}"
    );

    daemon.stop();
    let stopped = Command::new(env!("CARGO_BIN_EXE_meshctl"))
        .arg("support-bundle")
        .arg(&workspace)
        .output()
        .expect("stopped meshctl starts");
    assert_eq!(stopped.status.code(), Some(0), "{stopped:?}");
    let stopped_document = mesh_daemon::ipc::Json::parse(
        std::str::from_utf8(&stopped.stdout)
            .expect("utf8 stopped output")
            .trim(),
    )
    .expect("stopped support document");
    assert_eq!(
        stopped_document
            .get("workspace_correlation")
            .and_then(mesh_daemon::ipc::Json::as_text),
        Some(live_correlation.as_str()),
        "live and stopped previews must identify the same journal generation"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn live_support_bundle_rejects_unallowlisted_daemon_data_before_stdout() {
    let root = scratch("live-unallowlisted");
    let workspace = root.join("workspace");
    let endpoint = root.join("fake.sock");
    fs::create_dir(&workspace).expect("workspace");
    drop(OpenWorkspace::open(&workspace).expect("initialize workspace"));

    let mut pairs = match SupportBundle::collect(&workspace).document().clone() {
        Json::Object(pairs) => pairs,
        _ => panic!("support producer did not return an object"),
    };
    pairs.push(("file-content".to_owned(), Json::text("planted-content")));
    let unsafe_bundle = Json::Object(pairs);

    let listener = UnixListener::bind(&endpoint).expect("fake endpoint");
    let child = Command::new(env!("CARGO_BIN_EXE_meshctl"))
        .arg("--endpoint")
        .arg(&endpoint)
        .arg("support-bundle")
        .arg(&workspace)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("meshctl starts");

    let (mut stream, _) = listener.accept().expect("client connected");
    let mut reader = BufReader::new(stream.try_clone().expect("reader clone"));
    let mut line = String::new();
    reader.read_line(&mut line).expect("hello line");
    let hello = ClientMessage::decode(line.trim_end()).expect("hello");
    let ClientMessage::Hello { id, session, .. } = hello else {
        panic!("first message was not hello");
    };
    writeln!(
        stream,
        "{}",
        DaemonMessage::Welcome {
            id,
            version: SURFACE_VERSION,
            session,
            resumed: false,
            surface_version: SURFACE_VERSION,
        }
        .encode()
    )
    .expect("welcome");
    stream.flush().expect("welcome flush");

    line.clear();
    reader.read_line(&mut line).expect("call line");
    let call = ClientMessage::decode(line.trim_end()).expect("call");
    let ClientMessage::Call { id, method, .. } = call else {
        panic!("second message was not a call");
    };
    assert_eq!(method, "workspace.state");
    writeln!(
        stream,
        "{}",
        DaemonMessage::Result {
            id,
            value: Json::object([
                ("root", Json::text(workspace.display().to_string())),
                ("support_bundle", unsafe_bundle),
            ]),
        }
        .encode()
    )
    .expect("result");
    stream.flush().expect("result flush");
    drop(stream);

    let output = child.wait_with_output().expect("meshctl exits");
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(
        output.stdout.is_empty(),
        "unsafe bundle printed: {output:?}"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("safe support preview"),
        "{output:?}"
    );
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("planted-content"),
        "untrusted value was reflected: {output:?}"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn live_support_bundle_never_prints_untrusted_failure_messages() {
    let root = scratch("uf");
    let workspace = root.join("workspace");
    fs::create_dir(&workspace).expect("workspace");
    drop(OpenWorkspace::open(&workspace).expect("initialize workspace"));

    for (case, socket, at_welcome, refused) in [
        ("welcome-refused", "w.sock", true, true),
        ("call-refused", "r.sock", false, true),
        ("call-failed", "f.sock", false, false),
    ] {
        let endpoint = root.join(socket);
        let listener = UnixListener::bind(&endpoint).expect("fake endpoint");
        let child = Command::new(env!("CARGO_BIN_EXE_meshctl"))
            .arg("--endpoint")
            .arg(&endpoint)
            .arg("support-bundle")
            .arg(&workspace)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("meshctl starts");

        let (mut stream, _) = listener.accept().expect("client connected");
        let mut reader = BufReader::new(stream.try_clone().expect("reader clone"));
        let mut line = String::new();
        reader.read_line(&mut line).expect("hello line");
        let ClientMessage::Hello { id, session, .. } =
            ClientMessage::decode(line.trim_end()).expect("hello")
        else {
            panic!("first message was not hello");
        };
        if at_welcome {
            writeln!(
                stream,
                "{}",
                DaemonMessage::Refused {
                    id,
                    code: "planted-code".to_owned(),
                    message: "file-content: planted-content".to_owned(),
                    supported: vec![SURFACE_VERSION],
                }
                .encode()
            )
            .expect("unsafe welcome refusal");
            stream.flush().expect("unsafe welcome refusal flush");
        } else {
            writeln!(
                stream,
                "{}",
                DaemonMessage::Welcome {
                    id,
                    version: SURFACE_VERSION,
                    session,
                    resumed: false,
                    surface_version: SURFACE_VERSION,
                }
                .encode()
            )
            .expect("welcome");
            stream.flush().expect("welcome flush");
        }

        if !at_welcome {
            line.clear();
            reader.read_line(&mut line).expect("call line");
            let ClientMessage::Call { id, method, .. } =
                ClientMessage::decode(line.trim_end()).expect("call")
            else {
                panic!("second message was not a call");
            };
            assert_eq!(method, "workspace.state");
            let reply = if refused {
                DaemonMessage::Refused {
                    id,
                    code: "planted-code".to_owned(),
                    message: "file-content: planted-content".to_owned(),
                    supported: vec![SURFACE_VERSION],
                }
            } else {
                DaemonMessage::Failed {
                    id,
                    code: "planted-code".to_owned(),
                    message: "key-material: planted-content".to_owned(),
                }
            };
            writeln!(stream, "{}", reply.encode()).expect("unsafe reply");
            stream.flush().expect("unsafe reply flush");
        }
        drop(stream);

        let output = child.wait_with_output().expect("meshctl exits");
        assert_eq!(output.status.code(), Some(2), "{case}: {output:?}");
        assert!(
            output.stdout.is_empty(),
            "{case} reply printed untrusted data: {output:?}"
        );
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains("planted-content"),
            "{case} reply reflected untrusted data: {output:?}"
        );
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn live_support_bundle_never_reflects_an_untrusted_daemon_root() {
    let root = scratch("live-untrusted-root");
    let workspace = root.join("workspace");
    let untrusted_root = root.join("planted-content-from-daemon");
    let endpoint = root.join("fake.sock");
    fs::create_dir(&workspace).expect("workspace");
    fs::create_dir(&untrusted_root).expect("untrusted root");
    drop(OpenWorkspace::open(&workspace).expect("initialize workspace"));
    let safe_bundle = SupportBundle::collect(&workspace).document().clone();

    let listener = UnixListener::bind(&endpoint).expect("fake endpoint");
    let child = Command::new(env!("CARGO_BIN_EXE_meshctl"))
        .arg("--endpoint")
        .arg(&endpoint)
        .arg("support-bundle")
        .arg(&workspace)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("meshctl starts");

    let (mut stream, _) = listener.accept().expect("client connected");
    let mut reader = BufReader::new(stream.try_clone().expect("reader clone"));
    let mut line = String::new();
    reader.read_line(&mut line).expect("hello line");
    let ClientMessage::Hello { id, session, .. } =
        ClientMessage::decode(line.trim_end()).expect("hello")
    else {
        panic!("first message was not hello");
    };
    writeln!(
        stream,
        "{}",
        DaemonMessage::Welcome {
            id,
            version: SURFACE_VERSION,
            session,
            resumed: false,
            surface_version: SURFACE_VERSION,
        }
        .encode()
    )
    .expect("welcome");
    stream.flush().expect("welcome flush");

    line.clear();
    reader.read_line(&mut line).expect("call line");
    let ClientMessage::Call { id, method, .. } =
        ClientMessage::decode(line.trim_end()).expect("call")
    else {
        panic!("second message was not a call");
    };
    assert_eq!(method, "workspace.state");
    writeln!(
        stream,
        "{}",
        DaemonMessage::Result {
            id,
            value: Json::object([
                ("root", Json::text(untrusted_root.display().to_string()),),
                ("support_bundle", safe_bundle),
            ]),
        }
        .encode()
    )
    .expect("result");
    stream.flush().expect("result flush");
    drop(stream);

    let output = child.wait_with_output().expect("meshctl exits");
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(
        output.stdout.is_empty(),
        "untrusted reply printed: {output:?}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("different workspace open"), "{output:?}");
    assert!(
        !stderr.contains("planted-content-from-daemon"),
        "untrusted daemon value was reflected: {output:?}"
    );
    let _ = fs::remove_dir_all(root);
}

/// A pathname checked only after the reply is not an identity. Before the client pinned the
/// requested journal, a local retarget could make workspace A's bundle pass while the command had
/// named workspace B when it started.
#[test]
fn live_support_bundle_binds_the_requested_journal_before_connecting() {
    use std::os::unix::fs::symlink;

    let root = scratch("retarget");
    let workspace_a = root.join("workspace-a");
    let workspace_b = root.join("workspace-b");
    let requested = root.join("requested");
    let endpoint = root.join("fake.sock");
    fs::create_dir(&workspace_a).expect("workspace a");
    fs::create_dir(&workspace_b).expect("workspace b");
    drop(OpenWorkspace::open(&workspace_a).expect("initialize a"));
    drop(OpenWorkspace::open(&workspace_b).expect("initialize b"));
    symlink(&workspace_b, &requested).expect("requested initially names b");

    let bundle_a = SupportBundle::collect(&workspace_a).document().clone();
    let listener = UnixListener::bind(&endpoint).expect("fake endpoint");
    let child = Command::new(env!("CARGO_BIN_EXE_meshctl"))
        .arg("--endpoint")
        .arg(&endpoint)
        .arg("support-bundle")
        .arg(&requested)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("meshctl starts");

    let (mut stream, _) = listener.accept().expect("client connected after pinning b");
    let mut reader = BufReader::new(stream.try_clone().expect("reader clone"));
    let mut line = String::new();
    reader.read_line(&mut line).expect("hello line");
    let hello = ClientMessage::decode(line.trim_end()).expect("hello");
    let ClientMessage::Hello { id, session, .. } = hello else {
        panic!("first message was not hello");
    };
    writeln!(
        stream,
        "{}",
        DaemonMessage::Welcome {
            id,
            version: SURFACE_VERSION,
            session,
            resumed: false,
            surface_version: SURFACE_VERSION,
        }
        .encode()
    )
    .expect("welcome");
    stream.flush().expect("welcome flush");

    line.clear();
    reader.read_line(&mut line).expect("call line");
    let call = ClientMessage::decode(line.trim_end()).expect("call");
    let ClientMessage::Call { id, method, .. } = call else {
        panic!("second message was not a call");
    };
    assert_eq!(method, "workspace.state");

    // The old post-reply pathname check now saw A on both sides and accepted A's bundle, even
    // though the command pinned no evidence that the requested path named A when it began.
    fs::remove_file(&requested).expect("remove b alias");
    symlink(&workspace_a, &requested).expect("retarget requested alias to a");
    writeln!(
        stream,
        "{}",
        DaemonMessage::Result {
            id,
            value: Json::object([
                ("root", Json::text(workspace_a.display().to_string())),
                ("support_bundle", bundle_a),
            ]),
        }
        .encode()
    )
    .expect("result");
    stream.flush().expect("result flush");
    drop(stream);

    let output = child.wait_with_output().expect("meshctl exits");
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(
        output.stdout.is_empty(),
        "a mismatched bundle printed: {output:?}"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("different workspace open"),
        "{output:?}"
    );
    let _ = fs::remove_dir_all(root);
}
