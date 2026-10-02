use super::*;
use crate::fleet::{
    catalog::{AttachedFleetRequest, NativeFleetDirectory},
    *,
};
use crate::project_attachment::{AttachmentStorage, ObservationLimits, ProvisionedAttachment};
use ed25519_dalek::{Signer as _, SigningKey};
use std::{
    fs,
    os::unix::{fs::PermissionsExt as _, net::UnixStream},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
fn public(key: &SigningKey) -> PublicKey {
    PublicKey::from_bytes(key.verifying_key().to_bytes())
}
fn sign(key: &SigningKey, body: &SigningPayload) -> Result<Signature, String> {
    Ok(Signature::from_bytes(key.sign(body.as_bytes()).to_bytes()))
}
fn limits() -> Limits {
    Limits {
        lanes: 2,
        concurrency: 1,
        depth: 1,
        retries: 1,
    }
}
struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct Fixture {
    root: PathBuf,
    attachment: ProvisionedAttachment,
    directory: NativeFleetDirectory,
    service: Arc<FleetService>,
    lane: String,
    source: RemoteInputSource,
    coordinator: SigningKey,
    worker: SigningKey,
    creation: AttachedFleetRequest,
    _cleanup: Cleanup,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "mesh-native-start-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        for name in [
            "original",
            "metadata",
            "fleets",
            "installation",
            "store",
            "allocations",
        ] {
            let path = root.join(name);
            fs::create_dir(&path).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        fs::write(root.join("original/work.txt"), b"saved input\n").unwrap();
        let attachment = AttachmentStorage::open(&root.join("metadata"))
            .unwrap()
            .provision(&root.join("original"))
            .unwrap();
        let coordinator = SigningKey::from_bytes(&[68; 32]);
        let worker = SigningKey::from_bytes(&[69; 32]);
        let capture = attachment
            .project()
            .capture_inputs(ObservationLimits::default())
            .unwrap();
        let version = attachment
            .project()
            .save_capture(
                attachment.metadata_path(),
                &capture,
                public(&coordinator),
                |p| sign(&coordinator, p),
            )
            .unwrap()
            .operation();
        let source = attachment
            .prepare_remote_input(&version.to_string())
            .unwrap();
        let directory = NativeFleetDirectory::open(
            &root.join("fleets"),
            TrustedReviewers::default(),
            CheckpointRuntimeParameters::selected_defaults(),
        )
        .unwrap();
        let creation = AttachedFleetRequest::new(
            &"a".repeat(32),
            "Native remote work",
            &version.to_string(),
            limits(),
        )
        .unwrap();
        let service = directory.create_attached(&attachment, &creation).unwrap();
        let lane = service
            .native_state()
            .unwrap()
            .lanes
            .keys()
            .next()
            .unwrap()
            .clone();
        let cleanup = Cleanup(root.clone());
        Self {
            _cleanup: cleanup,
            root,
            attachment,
            directory,
            service,
            lane,
            source,
            coordinator,
            worker,
            creation,
        }
    }
    fn request(&self) -> RemoteNativeStartRequest<'_> {
        RemoteNativeStartRequest {
            lane: &self.lane,
            run: "native-run",
            assignment: RemoteAssignment {
                id: "native-assignment".into(),
                worker_key: RecordDigest::from_bytes(*public(&self.worker).as_bytes()).to_string(),
                input: self.source.manifest().input(),
                bundle: self.source.manifest().bundle(),
                lease_sequence: 1,
                lease_until_ms: received_clock().unwrap() + 60_000,
            },
            source: &self.source,
            coordinator: public(&self.coordinator),
            worker: public(&self.worker),
        }
    }
}
#[test]
fn native_start_transfers_saved_input_unlocked_and_never_replays_a_second_attempt() {
    let f = Fixture::new();
    fs::write(
        f.root.join("original/work.txt"),
        b"new unsaved original work\n",
    )
    .unwrap();
    let (installation, ()) = NativeWorkerInstallation::provision(
        &f.root.join("installation"),
        ProtectedWorkspaceRoot::inspect(&f.root.join("installation")).unwrap(),
        &[],
        |_| Ok((public(&f.worker), ())),
    )
    .unwrap();
    let destination = RemoteInputDestination::admit(
        &f.root.join("store"),
        ProtectedWorkspaceRoot::inspect(&f.root.join("store")).unwrap(),
        &f.root.join("allocations"),
        ProtectedWorkspaceRoot::inspect(&f.root.join("allocations")).unwrap(),
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
    let (client, server) = UnixStream::pair().unwrap();
    for stream in [&client, &server] {
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
    }
    std::thread::scope(|scope| {
        let worker =
            scope.spawn(|| hub.serve(server.try_clone().unwrap(), server, |p| sign(&f.worker, p)));
        let outcome = f
            .service
            .start_remote_input(f.request(), |request, intent| {
                let service = f.service.clone();
                let (send, receive) = std::sync::mpsc::channel();
                let reader = std::thread::spawn(move || send.send(service.native_state()).unwrap());
                receive
                    .recv_timeout(Duration::from_secs(2))
                    .expect("network blocked fleet views")
                    .unwrap();
                reader.join().unwrap();
                crate::fleet::remote_delivery::deliver_for_test(
                    request,
                    intent,
                    client.try_clone().unwrap(),
                    client,
                    |p| sign(&f.coordinator, p),
                )
            })
            .unwrap();
        assert!(matches!(
            outcome,
            RemoteInputTransferOutcome::Materialized(_)
        ));
        assert!(matches!(
            worker.join().unwrap().unwrap(),
            WorkerConnectionOutcome::Materialized { .. }
        ));
    });
    let allocations = fs::read_dir(f.root.join("allocations"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(allocations.len(), 1);
    assert_eq!(
        fs::read(allocations[0].join("files/work.txt")).unwrap(),
        b"saved input\n"
    );
    assert!(f
        .service
        .start_remote_input(f.request(), |_, _| -> io::Result<()> {
            panic!("duplicate start opened transport")
        })
        .is_err());
    let state = f.service.native_state().unwrap();
    assert_eq!(state.lanes[&f.lane].runs.len(), 1);
    assert_eq!(
        state.lanes[&f.lane].runs[0].remote.as_ref().unwrap().bundle,
        f.source.manifest().bundle()
    );
    assert_eq!(
        fs::read(f.root.join("original/work.txt")).unwrap(),
        b"new unsaved original work\n"
    );
    let facts = f
        .directory
        .attached_request_snapshot(&f.creation.request)
        .unwrap();
    assert_eq!(
        facts
            .get("fleet")
            .unwrap()
            .get("objective")
            .and_then(Json::as_text),
        Some(f.service.objective().unwrap().as_str())
    );
}
#[test]
fn invalid_start_refuses_before_dispatch_or_transport_and_failed_transport_retains_one_attempt() {
    let f = Fixture::new();
    for kind in 0..5 {
        let mut request = f.request();
        match kind {
            0 => request.assignment.bundle = RecordDigest::from_bytes([1; 32]),
            1 => request.assignment.worker_key = "00".repeat(32),
            2 => request.assignment.lease_until_ms = 1,
            3 => request.assignment.lease_until_ms = received_clock().unwrap() + 3_700_000,
            _ => request.run = "bad/run",
        }
        assert!(f
            .service
            .start_remote_input(request, |_, _| -> io::Result<()> {
                panic!("invalid start reached transport")
            })
            .is_err());
        assert!(f.service.native_state().unwrap().lanes[&f.lane]
            .runs
            .is_empty());
    }
    assert!(f
        .service
        .start_remote_input(f.request(), |_, _| -> io::Result<()> {
            Err(io::Error::other("injected disconnect before peer proof"))
        })
        .is_err());
    let before = f.service.native_state().unwrap();
    assert_eq!(before.lanes[&f.lane].runs.len(), 1);
    assert!(before.lanes[&f.lane].runs[0].remote.is_none());
    assert!(f
        .service
        .start_remote_input(f.request(), |_, _| -> io::Result<()> {
            panic!("uncertain call replayed")
        })
        .is_err());
    assert_eq!(f.service.native_state().unwrap(), before);
}
#[test]
fn cancellation_and_replaced_catalogue_refuse_initial_dispatch() {
    let f = Fixture::new();
    f.service.native_command("cancel", Command::Cancel).unwrap();
    assert!(f
        .service
        .start_remote_input(f.request(), |_, _| -> io::Result<()> {
            panic!("cancelled start")
        })
        .is_err());
    assert!(f.service.native_state().unwrap().lanes[&f.lane]
        .runs
        .is_empty());
    let f = Fixture::new();
    fs::rename(f.root.join("fleets"), f.root.join("old-fleets")).unwrap();
    fs::create_dir(f.root.join("fleets")).unwrap();
    assert!(f
        .service
        .start_remote_input(f.request(), |_, _| -> io::Result<()> {
            panic!("replaced catalogue")
        })
        .is_err());
    assert_eq!(fs::read_dir(f.root.join("fleets")).unwrap().count(), 0);
}
#[test]
fn request_recovery_after_restart_is_read_only_and_does_not_adopt_execution() {
    let f = Fixture::new();
    let root = f.root.clone();
    let request = f.creation.clone();
    let project = f.attachment.id().to_owned();
    let objective = f.service.objective().unwrap();
    assert!(f.directory.attached_request_snapshot("invalid").is_err());
    assert_eq!(
        f.directory
            .attached_request_snapshot(&"b".repeat(32))
            .unwrap()
            .get("fleet"),
        Some(&Json::Null)
    );
    assert!(f
        .directory
        .verify_outside(&[ProtectedWorkspaceRoot::inspect(&root).unwrap()])
        .is_err());
    f.directory
        .verify_outside(&[ProtectedWorkspaceRoot::inspect(&root.join("original")).unwrap()])
        .unwrap();
    // Keep the fixture's files for the next catalogue, but explicitly release its authority handles.
    let Fixture {
        attachment,
        directory,
        service,
        source,
        lane,
        coordinator,
        worker,
        creation,
        root: _,
        _cleanup,
    } = f;
    drop((
        attachment,
        directory,
        service,
        source,
        lane,
        coordinator,
        worker,
        creation,
    ));
    let reopened = NativeFleetDirectory::open(
        &root.join("fleets"),
        TrustedReviewers::default(),
        CheckpointRuntimeParameters::selected_defaults(),
    )
    .unwrap();
    let facts = reopened
        .attached_request_snapshot(&request.request)
        .unwrap();
    assert_eq!(
        facts
            .get("fleet")
            .unwrap()
            .get("objective")
            .and_then(Json::as_text),
        Some(objective.as_str())
    );
    assert_eq!(
        facts
            .get("fleet")
            .unwrap()
            .get("ownership")
            .and_then(Json::as_text),
        Some("restored-unattached")
    );
    assert!(reopened.current_service(&objective).is_err());
    let attachment = AttachmentStorage::open(&root.join("metadata"))
        .unwrap()
        .reopen(&project)
        .unwrap();
    assert!(
        reopened.create_attached(&attachment, &request).is_err(),
        "recovery must not reattach execution"
    );
    let history = reopened.history(&objective).unwrap();
    assert!(history
        .0
        .native_state()
        .unwrap()
        .lanes
        .values()
        .all(|lane| lane.runs.is_empty()));
    drop((history, attachment, reopened));
    drop(_cleanup);
}

#[test]
fn cancellation_during_transport_is_observable_and_prevents_peer_challenge() {
    let f = Fixture::new();
    assert!(f
        .service
        .start_remote_input(f.request(), |request, intent| -> io::Result<()> {
            f.service
                .native_command("cancel-in-network-wait", Command::Cancel)
                .unwrap();
            let RemoteInputDeliveryIntent::Claim { assignment, .. } = intent else {
                panic!("initial claim required")
            };
            assert!(RemotePeerChallenge::issue(
                request.runtime,
                request.lane,
                request.run,
                assignment,
                request.worker
            )
            .is_err());
            Err(io::Error::other("cancelled before signing"))
        })
        .is_err());
    let state = f.service.native_state().unwrap();
    assert!(state.cancelled);
    assert!(state.lanes[&f.lane].runs[0].remote.is_none());
}
