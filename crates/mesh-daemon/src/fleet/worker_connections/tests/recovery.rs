use super::*;
use std::time::{Duration, Instant};

fn seed(s: &Setup, installation: &NativeWorkerInstallation, runtime: &mut Runtime) {
    let registry = installation
        .registry(&hex_key(&s.f.coordinator), "objective", policy(s).maximum)
        .unwrap();
    let mut session = RemoteReceivingSession::new(
        registry,
        s.f.work.clone(),
        "0123456789abcdef0123456789abcdef",
        &s.destination,
    );
    let mut connection = session.connect().unwrap();
    let payload = connection
        .proof()
        .unwrap()
        .signing_payload_for(
            runtime,
            "lane",
            "run",
            &public(&s.f.coordinator),
            &public(&s.f.worker),
        )
        .unwrap();
    connection
        .authenticate(&sign(&s.f.coordinator, &payload).unwrap())
        .unwrap();
    connection.receive(s.manifest_frame()).unwrap();
    connection.receive(s.part(0, s.bytes.len())).unwrap();
    drop(connection.materialize().unwrap());
}
fn hex_key(key: &SigningKey) -> String {
    mesh_store::RecordDigest::from_bytes(key.verifying_key().to_bytes()).to_string()
}
struct LostFinal<W> {
    inner: W,
    flushed: bool,
    lose: bool,
}
impl<W: Write> Write for LostFinal<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.lose && self.flushed {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "lost final acknowledgment",
            ));
        }
        self.inner.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()?;
        self.flushed = true;
        Ok(())
    }
}
#[test]
fn signed_native_recovery_delivers_once_even_when_final_reply_is_lost() {
    for lose in [false, true] {
        let s = Setup::new();
        let installation = installation(&s);
        let mut runtime = s.f.runtime(true);
        seed(&s, &installation, &mut runtime);
        let mut hub = NativeWorkerConnections::new(&installation, &s.destination, policy(&s), 1)
            .unwrap()
            .with_recovery_policy(
                crate::TrustedReviewers::default(),
                crate::CheckpointRuntimeParameters::selected_defaults(),
            );
        let root = s.f.path.join("bridge");
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let endpoint = NativeWorkerEndpoint::bind(
            &root,
            crate::ProtectedWorkspaceRoot::inspect(&root).unwrap(),
            &[],
        )
        .unwrap();
        let (client, server) = connect(&root, &endpoint);
        let outcome = std::thread::scope(|scope| {
            let worker = scope.spawn(|| {
                hub.serve(
                    server.try_clone().unwrap(),
                    LostFinal {
                        inner: server,
                        flushed: false,
                        lose,
                    },
                    |p| sign(&s.f.worker, p),
                )
            });
            let receipt = recover_remote_worker(
                RemoteRecoveryClientRequest {
                    runtime: &mut runtime,
                    lane: "lane",
                    run: "run",
                    coordinator: public(&s.f.coordinator),
                    worker: public(&s.f.worker),
                },
                client.try_clone().unwrap(),
                client,
                |p| sign(&s.f.coordinator, p),
            );
            assert_eq!(receipt.is_err(), lose);
            worker.join().unwrap().unwrap()
        });
        let WorkerConnectionOutcome::Recovered {
            admission,
            reply_written,
        } = outcome
        else {
            panic!("recovered handoff")
        };
        assert_eq!(reply_written, !lose);
        assert_eq!(hub.recoveries.len(), 1);
        let working = std::path::PathBuf::from(
            hub.recoveries[0]
                .handoff
                .as_ref()
                .unwrap()
                .workspace
                .binding()
                .root(),
        );
        assert_eq!(fs::read(working.join("result.txt")).unwrap(), s.bytes);
        let (client, server) = connect(&root, &endpoint);
        std::thread::scope(|scope| {
            let worker = scope.spawn(|| {
                hub.serve(server.try_clone().unwrap(), server, |_| {
                    panic!("retained owner must refuse before signing")
                })
            });
            assert!(recover_remote_worker(
                RemoteRecoveryClientRequest {
                    runtime: &mut runtime,
                    lane: "lane",
                    run: "run",
                    coordinator: public(&s.f.coordinator),
                    worker: public(&s.f.worker)
                },
                client.try_clone().unwrap(),
                client,
                |p| sign(&s.f.coordinator, p)
            )
            .is_err());
            assert!(worker.join().unwrap().is_err());
        });
        let (closed, receive) = mpsc::sync_channel(1);
        drop(receive);
        let (reply, _) = mpsc::sync_channel(1);
        assert!(!hub
            .queue_received(&admission, launch(&s), reply, &closed)
            .unwrap());
        assert!(hub.recoveries[0].pending.is_some());
        let (sender, receiver) = mpsc::sync_channel(1);
        assert_eq!(hub.flush_pending(&sender).unwrap(), 1);
        assert_eq!(hub.flush_pending(&sender).unwrap(), 0);
        let ReceivedWorkerRequest::StartRecovered {
            handoff, launch, ..
        } = receiver.recv().unwrap()
        else {
            panic!("native recovered request")
        };
        let mut supervisor = ReceivedWorkerSupervisor::new(1, public(&s.f.worker)).unwrap();
        assert!(supervisor.start_recovered(*handoff, *launch).unwrap() == *admission);
        let deadline = Instant::now() + Duration::from_secs(10);
        while fs::read_to_string(working.join("launches.txt"))
            .ok()
            .as_deref()
            != Some("one\n")
        {
            assert!(Instant::now() < deadline, "fixture startup");
            std::thread::sleep(Duration::from_millis(10));
        }
        fs::write(working.join("finish"), b"done").unwrap();
        loop {
            if supervisor.poll().iter().any(|entry| {
                entry
                    .observation
                    .as_ref()
                    .is_ok_and(|workers| workers.iter().any(|w| w.outcome == Some(true)))
            }) {
                break;
            }
            assert!(Instant::now() < deadline, "fixture completion");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            fs::read_to_string(working.join("launches.txt")).unwrap(),
            "one\n"
        );
        assert_eq!(supervisor.admissions().len(), 1);
    }
}

#[test]
fn recovery_route_requires_explicit_native_opt_in() {
    let s = Setup::new();
    let installation = installation(&s);
    let mut runtime = s.f.runtime(true);
    seed(&s, &installation, &mut runtime);
    let mut hub =
        NativeWorkerConnections::new(&installation, &s.destination, policy(&s), 1).unwrap();
    let (client, server) = std::os::unix::net::UnixStream::pair().unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    server
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            hub.serve(server.try_clone().unwrap(), server, |_| {
                panic!("disabled route cannot sign")
            })
        });
        assert!(recover_remote_worker(
            RemoteRecoveryClientRequest {
                runtime: &mut runtime,
                lane: "lane",
                run: "run",
                coordinator: public(&s.f.coordinator),
                worker: public(&s.f.worker)
            },
            client.try_clone().unwrap(),
            client,
            |p| sign(&s.f.coordinator, p)
        )
        .is_err());
        assert!(worker.join().unwrap().is_err());
    });
    assert!(hub.recoveries.is_empty());
    assert!(!s
        .f
        .path
        .join("allocations/input-0123456789abcdef0123456789abcdef/initialization.json")
        .exists());
}
