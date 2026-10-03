//! Kill only an owned fixture subprocess at an acknowledged recovery phase, then reopen originals.
use super::*;
use crate::fleet::remote_admission::authentication::tests::Fixture;
use crate::fleet::{RemoteInputChunk, RemoteInputEntry, RemoteInputManifest};
use crate::{CheckpointRuntimeParameters, ProtectedWorkspaceRoot, TrustedReviewers};
use std::os::unix::{fs::MetadataExt, net::UnixListener, process::ExitStatusExt};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command as Process, Stdio},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
fn reopen(path: PathBuf) -> Setup {
    let f = Fixture::reopen_existing(path);
    let bytes = fs::read(
        f.path
            .join("allocations/input-0123456789abcdef0123456789abcdef/files/result.txt"),
    )
    .unwrap();
    let digest = mesh_cas::Blake3::digest_bytes(&bytes);
    let manifest = RemoteInputManifest::new(
        f.work.assignment.input,
        vec![RemoteInputEntry::File {
            path: "result.txt".into(),
            executable: false,
            digest,
            chunks: vec![RemoteInputChunk {
                digest,
                bytes: bytes.len() as u64,
            }],
        }],
    )
    .unwrap();
    assert_eq!(manifest.bundle(), f.work.assignment.bundle);
    let destination = RemoteInputDestination::admit(
        &f.path.join("store"),
        ProtectedWorkspaceRoot::inspect(&f.path.join("store")).unwrap(),
        &f.path.join("allocations"),
        ProtectedWorkspaceRoot::inspect(&f.path.join("allocations")).unwrap(),
        &[],
    )
    .unwrap();
    Setup {
        f,
        destination,
        manifest,
        bytes,
        digest,
    }
}
#[test]
fn worker_child() {
    let Some(root) = std::env::var_os("MESH_RECOVERY_CRASH_TEST_ROOT") else {
        return;
    };
    // The parent owns this fixture and cleanup. Never run Fixture::drop in the child.
    let s = std::mem::ManuallyDrop::new(reopen(PathBuf::from(root)));
    let phase: usize = std::env::var("MESH_RECOVERY_CRASH_TEST_PHASE")
        .unwrap()
        .parse()
        .unwrap();
    assert!([1, 2].contains(&phase));
    let stream =
        UnixStream::connect(std::env::var_os("MESH_RECOVERY_CRASH_TEST_SOCKET").unwrap()).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut input = stream.try_clone().unwrap();
    let request = RemoteWorkerRecoveryRequest::decode(
        &control(&mut RemoteFrameReader::new(&mut input)).unwrap(),
    )
    .unwrap()
    .verify(&policy(&s))
    .unwrap();
    let mut calls = 0;
    let _outcome = serve_remote_recovery(
        RemoteRecoveryWorkerRequest {
            request,
            registry: guarded(&s),
            destination: &s.destination,
            reviewers: TrustedReviewers::default(),
            checkpoint: CheckpointRuntimeParameters::selected_defaults(),
        },
        input,
        stream,
        |p| {
            calls += 1;
            if calls == phase {
                let mut marker = fs::File::create(s.f.path.join("crash-phase-ready")).unwrap();
                marker.write_all(phase.to_string().as_bytes()).unwrap();
                marker.sync_all().unwrap();
                loop {
                    std::thread::park();
                }
            }
            sign(&s.f.worker, p)
        },
    )
    .unwrap();
    panic!("fixture must be killed at its selected phase");
}
struct OwnedChild {
    child: Child,
    socket: PathBuf,
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_file(&self.socket);
    }
}
fn identity(path: &Path) -> (u64, u64) {
    let m = fs::symlink_metadata(path).unwrap();
    (m.dev(), m.ino())
}
#[test]
fn killed_worker_recovers_same_acknowledged_input_and_initial_mapping() {
    for phase in [1, 2] {
        let (s, mut runtime, registry) = ready();
        let admission = registry.receipts().unwrap().remove(0);
        drop(registry);
        let allocation =
            s.f.path
                .join("allocations/input-0123456789abcdef0123456789abcdef");
        let paths = [
            allocation.clone(),
            allocation.join("files"),
            allocation.join("files/result.txt"),
            s.f.path.join("store"),
        ];
        let original: Vec<_> = paths.iter().map(|p| identity(p)).collect();
        let socket = PathBuf::from(format!(
            "/tmp/mesh-rc-{}-{}.sock",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let error_log = fs::File::create(s.f.path.join("child-stderr.log")).unwrap();
        let child = Process::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "fleet::recovery_wire::tests::process_crash::worker_child",
                "--nocapture",
            ])
            .env("MESH_RECOVERY_CRASH_TEST_ROOT", &s.f.path)
            .env("MESH_RECOVERY_CRASH_TEST_PHASE", phase.to_string())
            .env("MESH_RECOVERY_CRASH_TEST_SOCKET", &socket)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(error_log)
            .spawn()
            .unwrap();
        let mut owned = OwnedChild { child, socket };
        let deadline = Instant::now() + Duration::from_secs(15);
        let stream = loop {
            match listener.accept() {
                Ok((s, _)) => break s,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    assert!(
                        owned.child.try_wait().unwrap().is_none(),
                        "child exited before connection"
                    );
                    assert!(Instant::now() < deadline, "child connection deadline");
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(e) => panic!("accept: {e}"),
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(15)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(15)))
            .unwrap();
        std::thread::scope(|scope| {
            let exchange = scope.spawn(|| {
                recover_remote_worker(
                    client(&s, &mut runtime),
                    stream.try_clone().unwrap(),
                    &stream,
                    |p| sign(&s.f.coordinator, p),
                )
            });
            loop {
                if fs::read_to_string(s.f.path.join("crash-phase-ready"))
                    .ok()
                    .as_deref()
                    == Some(&phase.to_string())
                {
                    break;
                }
                if exchange.is_finished() {
                    panic!(
                        "phase {phase}: coordinator ended before kill: {:?}",
                        exchange.join().unwrap().err()
                    );
                }
                assert!(
                    owned.child.try_wait().unwrap().is_none(),
                    "phase {phase}: child exited before phase: {}",
                    fs::read_to_string(s.f.path.join("child-stderr.log")).unwrap()
                );
                assert!(Instant::now() < deadline, "phase deadline");
                std::thread::sleep(Duration::from_millis(5));
            }
            owned.child.kill().unwrap();
            assert_eq!(owned.child.wait().unwrap().signal(), Some(9));
            assert!(
                exchange.join().unwrap().is_err(),
                "a killed exchange cannot be confirmed"
            );
        });
        let mapping = fs::read_to_string(allocation.join("workspace.json")).ok();
        assert_eq!(mapping.is_some(), phase == 2);
        let (a, b) = pair();
        let outcome = std::thread::scope(|scope| {
            let serving = scope.spawn(|| worker(&s, guarded(&s), b, None));
            let receipt =
                recover_remote_worker(client(&s, &mut runtime), a.try_clone().unwrap(), a, |p| {
                    sign(&s.f.coordinator, p)
                })
                .unwrap();
            let outcome = serving.join().unwrap().unwrap();
            assert_eq!(
                text(receipt.correlation(), "mapping").unwrap(),
                digest(&outcome.handoff.workspace.receipt().encode())
            );
            outcome
        });
        outcome.handoff.workspace.verify().unwrap();
        if let Some(mapping) = mapping {
            assert_eq!(outcome.handoff.workspace.receipt().encode(), mapping);
        }
        assert!(outcome.handoff.registry.receipts().unwrap()[0] == admission);
        assert!(outcome
            .handoff
            .registry
            .launch_receipt("assignment")
            .unwrap()
            .is_none());
        assert_eq!(
            fs::read(allocation.join("files/result.txt")).unwrap(),
            s.bytes
        );
        assert_eq!(
            paths.iter().map(|p| identity(p)).collect::<Vec<_>>(),
            original
        );
        assert_eq!(
            fs::read_dir(s.f.path.join("allocations")).unwrap().count(),
            1
        );
    }
}
