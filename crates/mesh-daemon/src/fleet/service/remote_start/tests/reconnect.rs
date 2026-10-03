use super::*;
use std::io::Write;
struct FailSecondChunk {
    stream: UnixStream,
    chunks: usize,
}
impl Write for FailSecondChunk {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() == 10 && &bytes[..4] == b"MSHR" && bytes[5] == 3 {
            self.chunks += 1;
            if self.chunks == 2 {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "injected second-chunk loss",
                ));
            }
        }
        self.stream.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}
fn pair() -> (UnixStream, UnixStream) {
    let pair = UnixStream::pair().unwrap();
    for stream in [&pair.0, &pair.1] {
        stream
            .set_read_timeout(Some(Duration::from_secs(15)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(15)))
            .unwrap();
    }
    pair
}
#[test]
fn native_input_reconnect_survives_coordinator_restart_without_adoption_or_new_attempt() {
    let f = Fixture::with_bytes(&vec![0xfa; 140_000]);
    let root = f.root.clone();
    let objective = f.service.objective().unwrap();
    let (installation, ()) = NativeWorkerInstallation::provision(
        &root.join("installation"),
        ProtectedWorkspaceRoot::inspect(&root.join("installation")).unwrap(),
        &[],
        |_| Ok((public(&f.worker), ())),
    )
    .unwrap();
    let destination = RemoteInputDestination::admit(
        &root.join("store"),
        ProtectedWorkspaceRoot::inspect(&root.join("store")).unwrap(),
        &root.join("allocations"),
        ProtectedWorkspaceRoot::inspect(&root.join("allocations")).unwrap(),
        &[],
    )
    .unwrap();
    let mut hub = NativeWorkerConnections::new(
        &installation,
        &destination,
        RemoteDispatchPolicy {
            coordinator: public(&f.coordinator),
            worker: public(&f.worker),
            provider: "codex",
            maximum: limits(),
            max_lease_ms: 60_000,
        },
        1,
    )
    .unwrap();
    let worker_key = f.worker.clone();
    let (client1, server1) = pair();
    let (client2, server2) = pair();
    let cleanup = std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            assert!(matches!(
                hub.serve(server1.try_clone().unwrap(), server1, |p| sign(
                    &worker_key,
                    p
                ))
                .unwrap(),
                WorkerConnectionOutcome::Disconnected
            ));
            hub.serve(server2.try_clone().unwrap(), server2, |p| {
                sign(&worker_key, p)
            })
            .unwrap()
        });
        assert!(f
            .service
            .start_remote_input(f.request(), |request, intent| {
                crate::fleet::remote_delivery::deliver_for_test(
                    request,
                    intent,
                    client1.try_clone().unwrap(),
                    FailSecondChunk {
                        stream: client1,
                        chunks: 0,
                    },
                    |p| sign(&f.coordinator, p),
                )
            })
            .is_err());
        let before = f.service.native_state().unwrap();
        assert!(before.lanes[&f.lane].runs[0].remote.is_some());
        let Fixture {
            attachment,
            directory,
            service,
            source,
            lane,
            coordinator,
            worker: _,
            creation: _,
            root: _,
            _cleanup,
        } = f;
        // Only the saved source is retained. Release every coordinator catalogue/service handle.
        drop((attachment, directory, service));
        fs::write(root.join("original/work.txt"), b"new external work\n").unwrap();
        let reopened = NativeFleetDirectory::open(
            &root.join("fleets"),
            TrustedReviewers::default(),
            CheckpointRuntimeParameters::selected_defaults(),
        )
        .unwrap();
        let history = reopened.history(&objective).unwrap();
        assert!(reopened.current_service(&objective).is_err());
        let wrong_worker = public(&SigningKey::from_bytes(&[70; 32]));
        assert!(history
            .reconnect_remote_input(
                RemoteHistoryInputRequest {
                    lane: &lane,
                    run: "native-run",
                    source: &source,
                    coordinator: public(&coordinator),
                    worker: wrong_worker,
                },
                |request| crate::fleet::remote_delivery::deliver_for_test(
                    request,
                    RemoteInputDeliveryIntent::Reconnect,
                    io::empty(),
                    io::sink(),
                    |_| panic!("wrong worker must refuse before signing")
                )
            )
            .is_err());
        assert_eq!(history.0.native_state().unwrap(), before);
        let outcome = history
            .reconnect_remote_input(
                RemoteHistoryInputRequest {
                    lane: &lane,
                    run: "native-run",
                    source: &source,
                    coordinator: public(&coordinator),
                    worker: public(&worker_key),
                },
                |request| {
                    let service = history.0.clone();
                    let (send, receive) = std::sync::mpsc::channel();
                    let reader =
                        std::thread::spawn(move || send.send(service.native_state()).unwrap());
                    assert_eq!(
                        receive
                            .recv_timeout(Duration::from_secs(2))
                            .expect("reconnect blocked live fleet reads")
                            .unwrap(),
                        before
                    );
                    reader.join().unwrap();
                    crate::fleet::remote_delivery::deliver_for_test(
                        request,
                        RemoteInputDeliveryIntent::Reconnect,
                        client2.try_clone().unwrap(),
                        client2,
                        |p| sign(&coordinator, p),
                    )
                },
            )
            .unwrap();
        assert!(matches!(
            outcome,
            RemoteInputTransferOutcome::Materialized(_)
        ));
        let WorkerConnectionOutcome::Materialized { admission, .. } = worker.join().unwrap() else {
            panic!("one materialization required")
        };
        assert_eq!(
            admission.work().assignment,
            before.lanes[&lane].runs[0].remote.clone().unwrap()
        );
        assert_eq!(
            installation
                .registry(admission.coordinator(), admission.objective(), limits())
                .unwrap()
                .receipts()
                .unwrap()
                .len(),
            1
        );
        let allocations = fs::read_dir(root.join("allocations"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(allocations.len(), 1);
        assert_eq!(
            fs::read(allocations[0].join("files/work.txt")).unwrap(),
            vec![0xfa; 140_000]
        );
        assert_eq!(
            fs::read(root.join("original/work.txt")).unwrap(),
            b"new external work\n"
        );
        assert_eq!(
            history.0.native_state().unwrap(),
            before,
            "successful reconnect cannot dispatch or renew"
        );
        assert!(reopened.current_service(&objective).is_err());
        drop((history, reopened, source));
        // These worker handles still pin their own files until the enclosing fixture finishes.
        _cleanup
    });
    drop(hub);
    drop((destination, installation));
    drop(cleanup);
}
#[test]
fn native_input_reconnect_without_claim_and_after_catalogue_replacement_refuses() {
    let f = Fixture::new();
    let objective = f.service.objective().unwrap();
    let history = f.directory.history(&objective).unwrap();
    let request = || RemoteHistoryInputRequest {
        lane: &f.lane,
        run: "native-run",
        source: &f.source,
        coordinator: public(&f.coordinator),
        worker: public(&f.worker),
    };
    let before = f.service.native_state().unwrap();
    assert!(history
        .reconnect_remote_input(request(), |request| {
            crate::fleet::remote_delivery::deliver_for_test(
                request,
                RemoteInputDeliveryIntent::Reconnect,
                io::empty(),
                io::sink(),
                |_| panic!("unclaimed work must not sign"),
            )
        })
        .is_err());
    assert_eq!(f.service.native_state().unwrap(), before);
    fs::rename(f.root.join("fleets"), f.root.join("preserved-fleets")).unwrap();
    fs::create_dir(f.root.join("fleets")).unwrap();
    assert!(history
        .reconnect_remote_input(request(), |_| -> io::Result<()> {
            panic!("changed catalogue opened transport")
        })
        .is_err());
}

#[test]
fn native_history_reconnect_refuses_stale_or_conflicting_retained_authority_before_signing() {
    // These are explicit synthetic negative ledger states. The separate restart test proves the
    // positive authenticated admission/transfer; none of these records claims a real worker ran.
    for kind in [
        "expired",
        "renewed",
        "cancelled",
        "running",
        "local",
        "wrong-input",
        "wrong-run",
    ] {
        let f = Fixture::new();
        f.service
            .native_command(
                "dispatch-fixture",
                Command::Dispatch {
                    lane: f.lane.clone(),
                    run: "native-run".into(),
                },
            )
            .unwrap();
        let mut assignment = f.request().assignment;
        if kind == "expired" {
            assignment.lease_until_ms = 1;
        }
        if kind == "wrong-input" {
            assignment.bundle = RecordDigest::from_bytes([81; 32]);
        }
        if kind == "local" {
            f.service
                .native_command(
                    "claim-fixture",
                    Command::ClaimLaunch {
                        lane: f.lane.clone(),
                        run: "native-run".into(),
                        owner: "native-local-host".into(),
                    },
                )
                .unwrap();
        } else {
            f.service
                .native_command(
                    "claim-fixture",
                    Command::ClaimRemoteLaunch {
                        lane: f.lane.clone(),
                        run: "native-run".into(),
                        assignment: assignment.clone(),
                    },
                )
                .unwrap();
        }
        match kind {
            "renewed" => f
                .service
                .native_command(
                    "renew-fixture",
                    Command::AdvanceRemoteLease {
                        lane: f.lane.clone(),
                        run: "native-run".into(),
                        assignment: assignment.id,
                        worker_key: assignment.worker_key,
                        expected_sequence: 1,
                        lease_until_ms: assignment.lease_until_ms + 1,
                    },
                )
                .unwrap(),
            "cancelled" => f
                .service
                .native_command("cancel-fixture", Command::Cancel)
                .unwrap(),
            "running" => f
                .service
                .native_command(
                    "observe-fixture",
                    Command::Observe {
                        lane: f.lane.clone(),
                        run: "native-run".into(),
                        state: RunState::Running,
                    },
                )
                .unwrap(),
            _ => (),
        }
        let before = f.service.native_state().unwrap();
        let history = f
            .directory
            .history(&f.service.objective().unwrap())
            .unwrap();
        assert!(
            history
                .reconnect_remote_input(
                    RemoteHistoryInputRequest {
                        lane: &f.lane,
                        run: if kind == "wrong-run" {
                            "another-run"
                        } else {
                            "native-run"
                        },
                        source: &f.source,
                        coordinator: public(&f.coordinator),
                        worker: public(&f.worker),
                    },
                    |request| crate::fleet::remote_delivery::deliver_for_test(
                        request,
                        RemoteInputDeliveryIntent::Reconnect,
                        io::empty(),
                        io::sink(),
                        |_| panic!("{kind} must refuse before signing")
                    )
                )
                .is_err(),
            "{kind}"
        );
        assert_eq!(f.service.native_state().unwrap(), before, "{kind}");
    }
}
