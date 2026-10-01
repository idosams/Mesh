use super::*;
use crate::fleet::*;
use crate::ProtectedWorkspaceRoot;
use ed25519_dalek::{Signer as _, SigningKey};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};
fn public(k: &SigningKey) -> PublicKey {
    PublicKey::from_bytes(k.verifying_key().to_bytes())
}
fn signature(k: &SigningKey, p: &SigningPayload) -> Result<Signature, String> {
    Ok(Signature::from_bytes(k.sign(p.as_bytes()).to_bytes()))
}
fn private(path: &Path) {
    fs::create_dir(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}
struct Native<'a> {
    hub: NativeWorkerConnections<'a>,
    endpoint: NativeWorkerEndpoint,
    root: std::path::PathBuf,
    key: SigningKey,
    calls: usize,
    history: Option<crate::fleet::service::FleetHistory>,
}
impl Native<'_> {
    fn exchange<T: Send>(
        &mut self,
        client: impl FnOnce(NativeWorkerStream, NativeWorkerStream) -> io::Result<T>,
    ) -> io::Result<T> {
        self.calls += 1;
        if let Some(history) = &self.history {
            // A separate reader must finish while the ingestion operation is inside transport.
            let service = history.0.clone();
            let (send, receive) = std::sync::mpsc::channel();
            let reader = std::thread::spawn(move || send.send(service.native_state()).unwrap());
            receive
                .recv_timeout(Duration::from_secs(2))
                .expect("ingestion blocked live views")
                .unwrap();
            reader.join().unwrap();
        }
        let stream = NativeWorkerEndpoint::connect(
            &self.root,
            ProtectedWorkspaceRoot::inspect(&self.root)?,
            Duration::from_secs(10),
        )?;
        let server = self.endpoint.accept(Duration::from_secs(10))?.unwrap();
        let output = server.try_clone()?;
        let key = &self.key;
        let hub = &mut self.hub;
        std::thread::scope(|scope| {
            let worker = scope.spawn(move || hub.serve(server, output, |p| signature(key, p)));
            let result = client(stream.try_clone()?, stream);
            let served = worker.join().unwrap();
            if result.is_ok() {
                served.unwrap();
            }
            result
        })
    }
}
impl Transport for Native<'_> {
    fn content(
        &mut self,
        destination: &RemoteInputDestination,
        request: RemoteWorkerStatusRequest<'_>,
        checkpoint: &str,
        offer: &str,
        sign: &mut Sign<'_>,
    ) -> io::Result<RemoteSavedResultOffer> {
        self.exchange(|input, output| {
            super::super::transfer::receive_remote_saved_result_selected(
                destination,
                request,
                checkpoint,
                Some(offer),
                input,
                output,
                sign,
            )
        })
    }
    fn evidence(
        &mut self,
        request: RemoteResultEvidenceRequest<'_>,
        sign: &mut Sign<'_>,
    ) -> io::Result<AuthenticatedRemoteResultEvidence> {
        self.exchange(|input, output| {
            super::super::evidence::receive_remote_result_evidence(request, input, output, sign)
        })
    }
}
#[test]
fn native_saved_result_ingestion_reopens_offline_and_refuses_replaced_storage() {
    exercise_ingestion(false);
}
#[test]
fn native_history_ingestion_receives_unlocked_and_recovers_offline() {
    exercise_ingestion(true);
}
fn exercise_ingestion(through_history: bool) {
    let fixture = crate::fleet::remote_admission::launch::tests::Fixture::new();
    let mut auth = crate::fleet::remote_admission::authentication::tests::Fixture::new();
    let input = RemoteInputManifest::new(RecordDigest::from_bytes([1; 32]), vec![]).unwrap();
    auth.work.assignment.bundle = input.bundle();
    let mut runtime = auth.runtime(true);
    let install_root = fixture.0.join("installation");
    private(&install_root);
    let (installation, ()) = NativeWorkerInstallation::provision(
        &install_root,
        ProtectedWorkspaceRoot::inspect(&install_root).unwrap(),
        &[],
        |_| Ok((public(&auth.worker), ())),
    )
    .unwrap();
    let limits = Limits {
        lanes: 2,
        concurrency: 1,
        depth: 1,
        retries: 1,
    };
    let coordinator = public(&auth.coordinator)
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let mut registry = installation
        .registry(&coordinator, "objective", limits.clone())
        .unwrap();
    let workspace = fixture.workspace_for(&mut registry, auth.work.clone());
    let RemoteLaunchOutcome::Reserved(reservation) = registry
        .reserve_launch(
            workspace,
            "codex",
            crate::fleet::service::received_clock().unwrap(),
        )
        .unwrap()
    else {
        panic!("fresh launch")
    };
    let offer = crate::fleet::received_host::tests::saved_result_fixture(
        &fixture,
        *reservation,
        auth.worker.clone(),
    );
    let destination = RemoteInputDestination::admit(
        &fixture.0.join("store"),
        ProtectedWorkspaceRoot::inspect(&fixture.0.join("store")).unwrap(),
        &fixture.0.join("allocations"),
        ProtectedWorkspaceRoot::inspect(&fixture.0.join("allocations")).unwrap(),
        &[],
    )
    .unwrap();
    let root = fixture.0.join("bridge");
    private(&root);
    let endpoint =
        NativeWorkerEndpoint::bind(&root, ProtectedWorkspaceRoot::inspect(&root).unwrap(), &[])
            .unwrap();
    let policy = RemoteDispatchPolicy {
        coordinator: public(&auth.coordinator),
        worker: public(&auth.worker),
        provider: "codex",
        maximum: limits,
        max_lease_ms: 60_000,
    };
    let mut transport = Native {
        hub: NativeWorkerConnections::new(&installation, &destination, policy, 1).unwrap(),
        endpoint,
        root,
        key: auth.worker.clone(),
        calls: 0,
        history: through_history.then(|| guarded_history(&auth)),
    };
    for name in ["store", "allocations"] {
        private(&auth.path.join(name));
    }
    let local = RemoteInputDestination::admit(
        &auth.path.join("store"),
        ProtectedWorkspaceRoot::inspect(&auth.path.join("store")).unwrap(),
        &auth.path.join("allocations"),
        ProtectedWorkspaceRoot::inspect(&auth.path.join("allocations")).unwrap(),
        &[],
    )
    .unwrap();
    let trusted = TrustedReviewers::default();
    macro_rules! request {
        () => {
            RemoteResultIngestionRequest {
                status: RemoteWorkerStatusRequest {
                    runtime: &mut runtime,
                    lane: "lane",
                    run: "run",
                    coordinator: public(&auth.coordinator),
                    worker: public(&auth.worker),
                },
                input: &input,
                offer: &offer,
                destination: &local,
                allocation: "11111111111111111111111111111111",
                reviewers: &trusted,
                checkpoint: CheckpointRuntimeParameters::selected_defaults(),
                actor: public(&auth.coordinator),
            }
        };
    }
    // A different, correctly signed offer for the same checkpoint must be rejected before CAS writes.
    let mut other_body = RemoteSavedResultOffer::decode(&offer).unwrap().body;
    let Json::Object(fields) = &mut other_body else {
        panic!("offer body")
    };
    fields
        .iter_mut()
        .find(|(name, _)| name == "review")
        .unwrap()
        .1 = Json::text("ff".repeat(32));
    let other = RemoteSavedResultOffer::sign(other_body, |p| signature(&auth.worker, p))
        .unwrap()
        .encode();
    let before = fs::read_dir(auth.path.join("store")).unwrap().count();
    let mut wrong = request!();
    wrong.offer = &other;
    assert!(ingest(wrong, &mut transport, |p| signature(&auth.coordinator, p)).is_err());
    assert_eq!(
        fs::read_dir(auth.path.join("store")).unwrap().count(),
        before
    );
    assert_eq!(
        fs::read_dir(auth.path.join("allocations")).unwrap().count(),
        0
    );
    let receipt = ingest(request!(), &mut transport, |p| {
        signature(&auth.coordinator, p)
    })
    .unwrap();
    assert_eq!(transport.calls, 3);
    let receiver =
        NativeRemoteResultReceiver::reopen_content_receipt(&local, &offer, request!().status)
            .unwrap()
            .0;
    let source = receipt
        .reopen(&local, receiver.manifest(), &trusted)
        .unwrap();
    let entry = source
        .manifest()
        .entries()
        .iter()
        .find(|e| matches!(e,RemoteInputEntry::File {path,..} if path=="note.txt"))
        .unwrap();
    let RemoteInputEntry::File { chunks, .. } = entry else {
        panic!()
    };
    assert_eq!(
        chunks
            .iter()
            .flat_map(|c| source.read_chunk(c.digest).unwrap())
            .collect::<Vec<_>>(),
        b"saved native worker result\n"
    );
    drop(source);
    drop(receiver);
    transport.history = None;
    drop(runtime);
    runtime = Runtime::open(
        mesh_store::fleet::FleetStore::open(auth.path.join("coordinator.sqlite")).unwrap(),
        "objective",
    )
    .unwrap();
    transport.history = through_history.then(|| guarded_history(&auth));
    assert_eq!(
        ingest(request!(), &mut transport, |_| panic!(
            "offline replay must not sign"
        ))
        .unwrap()
        .digest(),
        receipt.digest()
    );
    assert_eq!(transport.calls, 3);
    fs::rename(
        auth.path.join("allocations"),
        auth.path.join("preserved-allocations"),
    )
    .unwrap();
    private(&auth.path.join("allocations"));
    assert!(ingest(request!(), &mut transport, |_| panic!(
        "replacement must refuse"
    ))
    .is_err());
    assert_eq!(transport.calls, 3);
    assert_eq!(
        fs::read_dir(auth.path.join("allocations")).unwrap().count(),
        0
    );
}

// Exercise the same closed service operation with the existing authenticated native socket fixture.
fn ingest(
    request: RemoteResultIngestionRequest<'_>,
    transport: &mut Native<'_>,
    sign: impl FnMut(&SigningPayload) -> Result<Signature, String>,
) -> io::Result<RemoteLocalReviewReceipt> {
    let Some(history) = transport.history.clone() else {
        return super::ingest(request, transport, sign);
    };
    let request = crate::fleet::service::RemoteHistoryIngestionRequest {
        lane: request.status.lane,
        run: request.status.run,
        coordinator: request.status.coordinator,
        worker: request.status.worker,
        input: request.input,
        offer: request.offer,
        destination: request.destination,
        allocation: request.allocation,
        reviewers: request.reviewers,
        checkpoint: request.checkpoint,
        actor: request.actor,
    };
    history.ingest_selected_result(request, |request| super::ingest(request, transport, sign))
}
fn guarded_history(
    auth: &crate::fleet::remote_admission::authentication::tests::Fixture,
) -> crate::fleet::service::FleetHistory {
    use mesh_store::fleet::{FleetStore, FleetStoreAuthority, FleetStoreError};
    use std::{os::unix::fs::MetadataExt as _, sync::Arc};
    #[derive(Debug)]
    struct Identity {
        path: std::path::PathBuf,
        file: fs::File,
    }
    impl FleetStoreAuthority for Identity {
        fn check(&self) -> Result<(), FleetStoreError> {
            let original = self
                .file
                .metadata()
                .map_err(|_| FleetStoreError::AuthorityChanged)?;
            let current =
                fs::symlink_metadata(&self.path).map_err(|_| FleetStoreError::AuthorityChanged)?;
            if current.is_file()
                && current.dev() == original.dev()
                && current.ino() == original.ino()
                && current.nlink() == 1
            {
                Ok(())
            } else {
                Err(FleetStoreError::AuthorityChanged)
            }
        }
    }
    struct NoAllocation;
    impl crate::fleet::service::LaneAllocator for NoAllocation {
        fn allocate(
            &self,
            _: &str,
            _: &crate::fleet::workspace::VersionInput,
        ) -> Result<crate::fleet::workspace::LaneWorkspace, crate::ipc::Unavailable> {
            panic!("ingestion must not allocate a managed execution lane")
        }
    }
    let path = auth.path.join("coordinator.sqlite").canonicalize().unwrap();
    let authority = Arc::new(Identity {
        file: fs::File::open(&path).unwrap(),
        path: path.clone(),
    });
    let runtime = Runtime::open(
        FleetStore::open_guarded(&path, false, authority).unwrap(),
        "objective",
    )
    .unwrap();
    crate::fleet::service::FleetHistory(Arc::new(
        crate::fleet::service::FleetService::new(
            runtime,
            Arc::new(NoAllocation),
            ["codex".into()].into(),
        )
        .unwrap(),
    ))
}
