use super::*;
use crate::fleet::provider::CodexAdapter;
use crate::fleet::remote_admission::launch::tests::Fixture;
use crate::fleet::service::CheckpointSigner;
use crate::fleet::RunState;
use crate::ipc::{ClientMessage, DaemonMessage};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

struct Signer(ed25519_dalek::SigningKey);
impl CheckpointSigner for Signer {
    fn public_key(&self) -> mesh_types::PublicKey {
        mesh_types::PublicKey::from_bytes(self.0.verifying_key().to_bytes())
    }
    fn sign(&self, payload: &mesh_crypto::SigningPayload) -> Result<mesh_types::Signature, String> {
        use ed25519_dalek::Signer as _;
        Ok(mesh_types::Signature::from_bytes(
            self.0.sign(payload.as_bytes()).to_bytes(),
        ))
    }
}
struct Signers;
impl WorkerSignerFactory for Signers {
    fn signer(&self, lane: &str, run: &str) -> Result<Arc<dyn CheckpointSigner>, Unavailable> {
        assert_eq!((lane, run), ("lane", "run"));
        Ok(Arc::new(Signer(ed25519_dalek::SigningKey::from_bytes(
            &[0x79; 32],
        ))))
    }
}
struct RefusingSigner;
impl WorkerSignerFactory for RefusingSigner {
    fn signer(&self, _: &str, _: &str) -> Result<Arc<dyn CheckpointSigner>, Unavailable> {
        Err(unavailable("fixture-signer-refused"))
    }
}

fn adapter(fixture: &Fixture) -> CodexAdapter {
    fs::set_permissions(&fixture.0, fs::Permissions::from_mode(0o700)).unwrap();
    let executable = fixture.0.join("provider");
    fs::write(&executable, b"#!/bin/sh\ncat >/dev/null\nprintf 'one\\n' >> launches.txt\nn=0\nwhile [ ! -f finish ]; do\n n=$((n+1)); [ $n -lt 1000 ] || exit 91\n sleep 0.01\ndone\nprintf '%s\\n' '{\"type\":\"turn.completed\"}'\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    CodexAdapter::new(&executable, &executable).unwrap()
}

fn wait_started(root: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while fs::read_to_string(root.join("launches.txt"))
        .ok()
        .as_deref()
        != Some("one\n")
    {
        assert!(
            Instant::now() < deadline,
            "fixture provider did not acknowledge startup"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

struct Client(BufReader<UnixStream>);
impl Client {
    fn open(path: &Path) -> Self {
        let stream = UnixStream::connect(path).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut client = Self(BufReader::new(stream));
        assert!(matches!(
            client.exchange(ClientMessage::Hello {
                id: 1,
                versions: vec![8],
                session: "received-connection".into(),
            }),
            DaemonMessage::Welcome { .. }
        ));
        client
    }
    fn exchange(&mut self, message: ClientMessage) -> DaemonMessage {
        writeln!(self.0.get_mut(), "{}", message.encode()).unwrap();
        let mut line = String::new();
        self.0.read_line(&mut line).unwrap();
        DaemonMessage::decode(line.trim_end()).unwrap()
    }
    fn call(
        &mut self,
        objective: &str,
        credential: &str,
        action: &str,
        arguments: Json,
    ) -> DaemonMessage {
        self.exchange(ClientMessage::Call {
            id: 2,
            method: "fleet.agent.call".into(),
            version: 8,
            params: Json::object([
                ("objective", Json::text(objective)),
                ("credential", Json::text(credential)),
                ("action", Json::text(action)),
                ("arguments", arguments),
            ]),
        })
    }
}
fn result(value: DaemonMessage) -> Json {
    let DaemonMessage::Result { value, .. } = value else {
        panic!("expected authorized scoped reply")
    };
    value
}

#[test]
fn received_host_keeps_one_attempt_across_connections_and_serves_signed_checkpoint_review() {
    let fixture = Fixture::new();
    let reservation = fixture.session_reservation();
    let root = std::path::PathBuf::from(reservation.workspace().binding().root());
    let receipt = reservation.receipt().clone();
    let worker = ReceivedWorkerHost::start(
        *reservation,
        adapter(&fixture),
        &fixture.0.join("s"),
        Arc::new(Signers),
    )
    .unwrap();
    assert!(worker.receipt() == &receipt);
    let observer = local_fixture_observer(&worker, &fixture);
    verify_saved_review(worker, &fixture, &root, &observer);
}

#[cfg(target_os = "macos")]
#[test]
fn received_saved_result_offer_is_signed_durable_and_replayed_without_resigning() {
    let fixture = Fixture::new();
    let reservation = fixture.result_session_reservation();
    let root = std::path::PathBuf::from(reservation.workspace().binding().root());
    let worker = ReceivedWorkerHost::start(
        *reservation,
        adapter(&fixture),
        &fixture.0.join("s"),
        Arc::new(Signers),
    )
    .unwrap();
    let observer = local_fixture_observer(&worker, &fixture);
    verify_saved_review(worker, &fixture, &root, &observer);
}

fn local_fixture_observer(
    worker: &ReceivedWorkerHost,
    fixture: &Fixture,
) -> crate::fleet::RemoteAdmissionRegistry {
    let admission = worker.receipt().admission();
    crate::fleet::RemoteAdmissionRegistry::new(
        mesh_store::fleet::FleetStore::open(fixture.0.join("worker.sqlite")).unwrap(),
        admission.coordinator(),
        &admission.work().assignment.worker_key,
        admission.objective(),
        crate::fleet::Limits {
            lanes: 1,
            concurrency: 1,
            depth: 0,
            retries: 0,
        },
    )
    .unwrap()
}

fn verify_saved_review(
    mut worker: ReceivedWorkerHost,
    fixture: &Fixture,
    root: &Path,
    observer: &crate::fleet::RemoteAdmissionRegistry,
) {
    #[cfg(target_os = "macos")]
    let mut reopening = None;
    wait_started(root);
    let objective = worker.service.objective().unwrap();
    let credential = worker.host.test_owned_credential("lane").to_owned();
    let mut connection = Client::open(worker.endpoint());
    let forbidden = fixture.0.join("not-a-worker-workspace");
    assert!(matches!(
        connection.exchange(ClientMessage::Call {
            id: 3,
            method: "workspace.open".into(),
            version: 8,
            params: Json::object([("path", Json::text(forbidden.to_str().unwrap()))]),
        }),
        DaemonMessage::Failed { .. }
    ));
    assert!(!forbidden.exists());
    assert!(matches!(
        connection.call(
            "another-objective",
            &credential,
            "context",
            Json::Object(vec![])
        ),
        DaemonMessage::Failed { .. }
    ));
    assert!(matches!(
        connection.call(
            &objective,
            &"00".repeat(32),
            "context",
            Json::Object(vec![])
        ),
        DaemonMessage::Failed { .. }
    ));
    let context = result(connection.call(&objective, &credential, "context", Json::Object(vec![])));
    assert_eq!(context.get("lane").and_then(Json::as_text), Some("lane"));
    assert_eq!(context.get("run").and_then(Json::as_text), Some("run"));
    drop(connection);
    assert_eq!(worker.poll().unwrap().len(), 1);
    assert_eq!(
        worker.service.native_state().unwrap().lanes["lane"].runs[0].state,
        RunState::Running
    );
    let mut reconnected = Client::open(worker.endpoint());
    let recovered =
        result(reconnected.call(&objective, &credential, "context", Json::Object(vec![])));
    assert_eq!(context.get("generation"), recovered.get("generation"));
    fs::write(root.join("note.txt"), "received worker result\n").unwrap();
    let checkpoint = result(reconnected.call(
        &objective,
        &credential,
        "checkpoint",
        Json::object([("request", Json::text("saved"))]),
    ));
    assert_eq!(checkpoint.get("complete"), Some(&Json::Bool(true)));
    result(reconnected.call(
        &objective,
        &credential,
        "submit_review",
        Json::object([("checkpoint", checkpoint.get("checkpoint").unwrap().clone())]),
    ));
    drop(reconnected);
    fs::write(root.join("finish"), b"finish").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let observations = worker.poll().unwrap();
        if let Some(outcome) = observations[0].outcome {
            assert!(outcome);
            break;
        }
        assert!(Instant::now() < deadline, "fixture provider did not finish");
        std::thread::sleep(Duration::from_millis(10));
    }
    let state = worker.service.native_state().unwrap();
    assert_eq!(state.lanes["lane"].runs.len(), 1);
    assert_eq!(state.lanes["lane"].runs[0].state, RunState::Succeeded);
    let admission = worker.receipt().admission();
    let observed = observer
        .execution_observation(&admission.work().assignment.id)
        .unwrap()
        .unwrap();
    assert!(observed.launch() == worker.receipt());
    assert_eq!(observed.revision(), state.revision);
    assert_eq!(
        observed.state(),
        crate::fleet::RemoteExecutionState::Recorded(RunState::Succeeded)
    );
    assert_eq!(
        fs::read_to_string(root.join("launches.txt")).unwrap(),
        "one\n"
    );
    let reviews = worker.service.saved_reviews("lane", None).unwrap();
    assert_eq!(reviews.get("total"), Some(&Json::Number(1)));
    let saved = &reviews.get("reviews").unwrap().as_array().unwrap()[0];
    assert_eq!(saved.get("checkpoint"), checkpoint.get("checkpoint"));
    assert_eq!(saved.get("version"), checkpoint.get("version"));
    let selection = super::super::service::SavedReviewSelection::new(
        "lane",
        saved.get("checkpoint").unwrap().as_text().unwrap(),
        saved.get("version").unwrap().as_text().unwrap(),
        saved.get("bundle").unwrap().as_text().unwrap(),
    )
    .unwrap();
    worker.service.saved_review(&selection).unwrap();
    fs::write(root.join("note.txt"), "later unsaved working bytes\n").unwrap();
    let revision = worker.service.native_state().unwrap().revision;
    let source = worker
        .service
        .prepare_remote_review_input(&selection)
        .unwrap();
    assert_eq!(
        source.manifest().input().to_string(),
        saved.get("version").unwrap().as_text().unwrap()
    );
    let chunks = source
        .manifest()
        .entries()
        .iter()
        .find_map(|entry| match entry {
            crate::fleet::RemoteInputEntry::File { path, chunks, .. } if path == "note.txt" => {
                Some(chunks.clone())
            }
            _ => None,
        })
        .expect("saved note export");
    let exported: Vec<u8> = chunks
        .iter()
        .flat_map(|chunk| source.read_chunk(chunk.digest).unwrap())
        .collect();
    assert_eq!(exported, b"received worker result\n");
    assert_eq!(
        fs::read(root.join("note.txt")).unwrap(),
        b"later unsaved working bytes\n"
    );
    assert_eq!(worker.service.native_state().unwrap().revision, revision);
    let wrong = super::super::service::SavedReviewSelection::new(
        "lane",
        saved.get("checkpoint").unwrap().as_text().unwrap(),
        &worker.receipt().initial_operation().to_string(),
        saved.get("bundle").unwrap().as_text().unwrap(),
    )
    .unwrap();
    assert!(worker.service.prepare_remote_review_input(&wrong).is_err());
    #[cfg(target_os = "macos")]
    {
        let signer = Signer(ed25519_dalek::SigningKey::from_bytes(&[0x73; 32]));
        let key: String = signer
            .public_key()
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if worker.receipt().admission().work().assignment.worker_key == key {
            assert!(worker
                .service
                .sign_remote_saved_review(&selection, |_| Err("fixture refusal".into()))
                .is_err());
            assert!(worker
                .service
                .sign_remote_saved_review(&selection, |p| Signer(
                    ed25519_dalek::SigningKey::from_bytes(&[0x74; 32])
                )
                .sign(p))
                .is_err());
            let wrong = Signer(ed25519_dalek::SigningKey::from_bytes(&[0x74; 32]));
            let publication_time = Instant::now();
            assert!(worker
                .publish_saved_result_at(&wrong, publication_time)
                .unwrap()
                .is_err());
            assert!(
                worker
                    .publish_saved_result_at(&signer, publication_time)
                    .is_none(),
                "retry interval is enforced"
            );
            let publication_time = publication_time + Duration::from_secs(1);
            assert_eq!(
                worker
                    .publish_saved_result_at(&signer, publication_time)
                    .unwrap()
                    .unwrap(),
                saved.get("checkpoint").unwrap().as_text().unwrap()
            );
            assert!(
                worker
                    .publish_saved_result_at(&signer, publication_time)
                    .is_none(),
                "one offer at most per interval"
            );
            let (offer, exported) = worker
                .service
                .sign_remote_saved_review(&selection, |_| {
                    panic!("automatic publication already retained the signature")
                })
                .unwrap();
            assert_eq!(exported.manifest(), source.manifest());
            let encoded = offer.encode();
            let (replayed, _) = worker
                .service
                .sign_remote_saved_review(&selection, |_| {
                    panic!("retained offer must not sign again")
                })
                .unwrap();
            assert_eq!(encoded, replayed.encode());
            let registry = crate::fleet::RemoteAdmissionRegistry::new(
                mesh_store::fleet::FleetStore::open(fixture.0.join("worker.sqlite")).unwrap(),
                &"ab".repeat(32),
                &key,
                "objective",
                crate::fleet::Limits {
                    lanes: 1,
                    concurrency: 1,
                    depth: 0,
                    retries: 0,
                },
            )
            .unwrap();
            assert_eq!(
                registry
                    .saved_result_offer(
                        "assignment",
                        saved.get("checkpoint").unwrap().as_text().unwrap()
                    )
                    .unwrap()
                    .unwrap()
                    .encode(),
                encoded
            );
            assert!(registry
                .saved_result_offer("assignment", "unknown-checkpoint")
                .unwrap()
                .is_none());
            assert_eq!(worker.service.native_state().unwrap().revision, revision);
            let destination = crate::fleet::RemoteInputDestination::admit(
                &fixture.0.join("store"),
                crate::ProtectedWorkspaceRoot::inspect(&fixture.0.join("store")).unwrap(),
                &fixture.0.join("allocations"),
                crate::ProtectedWorkspaceRoot::inspect(&fixture.0.join("allocations")).unwrap(),
                &[],
            )
            .unwrap();
            reopening = Some((registry, destination, encoded));
        }
    }
    // Completion revoked the exact provider session. Reconnection cannot resurrect it.
    let mut connection = Client::open(worker.endpoint());
    assert!(matches!(
        connection.call(&objective, &credential, "context", Json::Object(vec![])),
        DaemonMessage::Failed { .. }
    ));
    drop(connection);
    drop(worker);
    #[cfg(target_os = "macos")]
    let reopened = reopening.as_ref().map(|(registry, destination, encoded)| {
        let checkpoint = saved.get("checkpoint").unwrap().as_text().unwrap();
        let launch = registry.launch_receipt("assignment").unwrap().unwrap();
        let (offer, reopened) = registry
            .reopen_saved_result(
                destination,
                "assignment",
                checkpoint,
                &crate::TrustedReviewers::default(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(offer.encode(), *encoded);
        assert_eq!(reopened.manifest(), source.manifest());
        let export = registry
            .reopen_saved_result_with_correspondence(
                destination,
                "assignment",
                checkpoint,
                &crate::TrustedReviewers::default(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(export.offer.encode(), *encoded);
        assert_eq!(export.source.manifest(), source.manifest());
        let correspondence = Json::parse(export.correspondence.encoded()).unwrap();
        assert_eq!(
            correspondence.get("result_manifest"),
            Some(&Json::text(source.manifest().bundle().to_string()))
        );
        let note = correspondence
            .get("entries")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row.get("path") == Some(&Json::text("note.txt")))
            .unwrap();
        assert_eq!(
            note.get("input_path"),
            Some(&Json::Null),
            "the received input was empty; a new saved file has no original identity"
        );
        assert_eq!(
            fs::read(root.join("note.txt")).unwrap(),
            b"later unsaved working bytes\n"
        );
        let bytes: Vec<u8> = chunks
            .iter()
            .flat_map(|c| reopened.read_chunk(c.digest).unwrap())
            .collect();
        assert_eq!(bytes, b"received worker result\n");
        assert!(registry.launch_receipt("assignment").unwrap().unwrap() == launch);
        let mapping = fixture
            .0
            .join("allocations/input-0123456789abcdef0123456789abcdef/workspace.json");
        let original = fs::read(&mapping).unwrap();
        let mut altered = original.clone();
        altered.push(b' ');
        fs::write(&mapping, altered).unwrap();
        assert!(registry
            .reopen_saved_result(
                destination,
                "assignment",
                checkpoint,
                &crate::TrustedReviewers::default()
            )
            .is_err());
        fs::write(&mapping, original).unwrap();
        assert!(registry
            .reopen_saved_result(
                destination,
                "assignment",
                "unknown-checkpoint",
                &crate::TrustedReviewers::default()
            )
            .unwrap()
            .is_none());
        reopened
    });
    // A read-only export retains native pins, not an execution owner or credential.
    let exported: Vec<u8> = chunks
        .iter()
        .flat_map(|chunk| source.read_chunk(chunk.digest).unwrap())
        .collect();
    assert_eq!(exported, b"received worker result\n");
    let displaced = root.with_file_name("displaced-export-workspace");
    fs::rename(root, &displaced).unwrap();
    fs::create_dir(root).unwrap();
    assert!(source.read_chunk(chunks[0].digest).is_err());
    #[cfg(target_os = "macos")]
    if let Some(reopened) = reopened {
        assert!(reopened.read_chunk(chunks[0].digest).is_err());
        let (registry, destination, _) = reopening.unwrap();
        assert!(registry
            .reopen_saved_result(
                &destination,
                "assignment",
                saved.get("checkpoint").unwrap().as_text().unwrap(),
                &crate::TrustedReviewers::default()
            )
            .is_err());
    }
}

#[test]
fn cancelling_received_host_retains_uncertain_attempt_and_revokes_connection_authority() {
    let fixture = Fixture::new();
    let reservation = fixture.session_reservation();
    let root = std::path::PathBuf::from(reservation.workspace().binding().root());
    let mut worker = ReceivedWorkerHost::start(
        *reservation,
        adapter(&fixture),
        &fixture.0.join("s"),
        Arc::new(Signers),
    )
    .unwrap();
    wait_started(&root);
    let objective = worker.service.objective().unwrap();
    let credential = worker.host.test_owned_credential("lane").to_owned();
    worker.request_cancel().unwrap();
    worker.poll().unwrap();
    let state = worker.service.native_state().unwrap();
    assert_eq!(state.lanes["lane"].runs.len(), 1);
    assert_eq!(state.lanes["lane"].runs[0].state, RunState::Stopping);
    assert!(state.lanes["lane"].runs[0].state.occupies_slot());
    let mut connection = Client::open(worker.endpoint());
    assert!(matches!(
        connection.call(&objective, &credential, "context", Json::Object(vec![])),
        DaemonMessage::Failed { .. }
    ));
    drop(connection);
    worker.poll().unwrap();
    assert_eq!(
        worker.service.native_state().unwrap().lanes["lane"]
            .runs
            .len(),
        1
    );
}

#[test]
fn failed_signer_or_endpoint_does_not_start_or_release_an_attempt() {
    for signer_refuses in [true, false] {
        let fixture = Fixture::new();
        let reservation = fixture.session_reservation();
        let root = std::path::PathBuf::from(reservation.workspace().binding().root());
        let adapter = adapter(&fixture);
        let endpoint = fixture.0.join("s");
        if !signer_refuses {
            fs::write(&endpoint, b"preserved existing file").unwrap();
        }
        let signers: Arc<dyn WorkerSignerFactory> = if signer_refuses {
            Arc::new(RefusingSigner)
        } else {
            Arc::new(Signers)
        };
        assert!(ReceivedWorkerHost::start(*reservation, adapter, &endpoint, signers).is_err());
        assert!(!root.join("launches.txt").exists());
        assert!(root.exists());
        if !signer_refuses {
            assert_eq!(fs::read(&endpoint).unwrap(), b"preserved existing file");
        }
    }
}

fn broker_worker_journey(lose_final_reply: bool) {
    use crate::fleet::receiving_broker::tests::received_handoff;
    use crate::fleet::receiving_session::tests::Setup;
    let setup = Setup::new();
    let handoff = received_handoff(&setup, lose_final_reply);
    let fixture = Fixture::new();
    let worker = ReceivedWorkerHost::start_received(
        *handoff,
        adapter(&fixture),
        &fixture.0.join("s"),
        Arc::new(Signers),
        crate::TrustedReviewers::default(),
        crate::CheckpointRuntimeParameters::selected_defaults(),
    )
    .unwrap();
    let receipt = worker.receipt().clone();
    assert!(receipt.admission().work() == &setup.f.work);
    assert_ne!(receipt.initial_operation(), setup.manifest.input());
    let state = worker.service.native_state().unwrap();
    let binding = state.lanes["lane"].workspace.as_ref().unwrap();
    let root = std::path::PathBuf::from(binding.root());
    assert_eq!(fs::read(root.join("result.txt")).unwrap(), setup.bytes);
    assert_eq!(setup.f.registry().receipts().unwrap().len(), 1);
    assert!(
        setup
            .f
            .registry()
            .launch_receipt("assignment")
            .unwrap()
            .as_ref()
            == Some(&receipt)
    );
    // This reconnects scoped IPC, checkpoints signed history, reviews it, observes completion,
    // and asserts exactly one provider launch and one retained run after every connection is gone.
    verify_saved_review(worker, &fixture, &root, &setup.f.registry());
    assert!(
        setup
            .f
            .registry()
            .launch_receipt("assignment")
            .unwrap()
            .as_ref()
            == Some(&receipt)
    );
}

#[test]
fn broker_handoff_runs_one_provider_and_retains_saved_review_after_connections_close() {
    broker_worker_journey(false);
}

#[test]
fn lost_broker_final_reply_still_runs_original_handoff_once_and_retains_saved_review() {
    broker_worker_journey(true);
}

#[test]
fn changed_broker_input_is_preserved_and_refused_before_launch_intent() {
    use crate::fleet::receiving_broker::tests::received_handoff;
    use crate::fleet::receiving_session::tests::Setup;
    let setup = Setup::new();
    let handoff = received_handoff(&setup, true);
    let input = handoff.allocation.path().join("result.txt");
    fs::write(&input, b"preserved changed input").unwrap();
    let fixture = Fixture::new();
    assert!(ReceivedWorkerHost::start_received(
        *handoff,
        adapter(&fixture),
        &fixture.0.join("s"),
        Arc::new(Signers),
        crate::TrustedReviewers::default(),
        crate::CheckpointRuntimeParameters::selected_defaults(),
    )
    .is_err());
    assert_eq!(fs::read(input).unwrap(), b"preserved changed input");
    assert_eq!(setup.f.registry().receipts().unwrap().len(), 1);
    assert!(setup
        .f
        .registry()
        .launch_receipt("assignment")
        .unwrap()
        .is_none());
    assert!(!fixture.0.join("s").exists());
}

fn resident_launch(
    fixture: &Fixture,
    signers: Arc<dyn WorkerSignerFactory>,
) -> ReceivedWorkerLaunch {
    ReceivedWorkerLaunch {
        adapter: adapter(fixture).into(),
        endpoint: fixture.0.join("s"),
        signers,
        reviewers: crate::TrustedReviewers::default(),
        checkpoint: crate::CheckpointRuntimeParameters::selected_defaults(),
    }
}
fn resident_key(setup: &crate::fleet::receiving_session::tests::Setup) -> mesh_types::PublicKey {
    mesh_types::PublicKey::from_bytes(setup.f.worker.verifying_key().to_bytes())
}
#[test]
fn resident_retains_failed_start_and_polls_independent_provider_after_broker_disconnect() {
    use crate::fleet::{
        receiving_broker::tests::received_handoff, receiving_session::tests::Setup,
    };
    let failed = Setup::new();
    let mut live = Setup::new();
    live.f.work.assignment.id = "second".into();
    let mut extra = Setup::new();
    extra.f.work.assignment.id = "third".into();
    let mut resident = ReceivedWorkerSupervisor::new(2, resident_key(&failed)).unwrap();
    let fixture1 = Fixture::new();
    let fixture2 = Fixture::new();
    assert!(resident
        .start_received(
            *received_handoff(&failed, false),
            resident_launch(&fixture1, Arc::new(RefusingSigner))
        )
        .is_err());
    let failed_receipt = resident.admissions().remove(0);
    let handoff = received_handoff(&live, true);
    let input_root = handoff.allocation.path().to_path_buf();
    let receipt = resident
        .start_received(*handoff, resident_launch(&fixture2, Arc::new(Signers)))
        .unwrap();
    let snapshot = resident.snapshot(&receipt).unwrap();
    let lanes = snapshot.get("lanes").unwrap().as_array().unwrap();
    assert_eq!(lanes.len(), 1);
    let root = std::path::PathBuf::from(
        lanes[0]
            .get("workspace")
            .unwrap()
            .get("root")
            .unwrap()
            .as_text()
            .unwrap(),
    );
    assert_ne!(root, input_root);
    wait_started(&root);
    assert!(resident.snapshot(&failed_receipt).is_err());
    assert!(resident.snapshot(&receipt).is_ok());
    let observations = resident.poll();
    assert_eq!(observations.len(), 2);
    assert!(observations[0].observation.is_err());
    assert_eq!(observations[1].observation.as_ref().unwrap().len(), 1);
    fs::write(root.join("finish"), b"finish").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let observations = resident.poll();
        assert!(observations[0].observation.is_err());
        if let Some(outcome) = observations[1].observation.as_ref().unwrap()[0].outcome {
            assert!(outcome);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "resident provider did not finish"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let extra_handoff = received_handoff(&extra, false);
    let extra_root = extra_handoff.allocation.path().to_path_buf();
    let unknown = extra_handoff.allocation.admission.as_ref().unwrap().clone();
    assert!(resident.request_cancel(&unknown).is_err());
    assert_eq!(
        resident
            .start_received(
                *extra_handoff,
                resident_launch(&Fixture::new(), Arc::new(Signers))
            )
            .err()
            .unwrap()
            .code,
        "remote-supervisor-capacity"
    );
    assert_eq!(resident.admissions().len(), 2);
    assert!(!extra_root.join("launches.txt").exists());
    assert_eq!(
        fs::read_to_string(root.join("launches.txt")).unwrap(),
        "one\n"
    );
    assert!(!input_root.join("launches.txt").exists());
    assert!(!input_root.join("finish").exists());
    assert_eq!(fs::read(input_root.join("result.txt")).unwrap(), live.bytes);
}
#[test]
fn resident_never_retries_retained_identity_or_accepts_another_worker() {
    use crate::fleet::{
        receiving_broker::tests::received_handoff, receiving_session::tests::Setup,
    };
    let setup = Setup::new();
    let replay = Setup::new();
    let mut resident = ReceivedWorkerSupervisor::new(2, resident_key(&setup)).unwrap();
    let fixture = Fixture::new();
    assert!(resident
        .start_received(
            *received_handoff(&setup, false),
            resident_launch(&fixture, Arc::new(RefusingSigner))
        )
        .is_err());
    let handoff = received_handoff(&replay, false);
    let root = handoff.allocation.path().to_path_buf();
    assert!(resident
        .start_received(*handoff, resident_launch(&fixture, Arc::new(Signers)))
        .is_err());
    assert_eq!(resident.admissions().len(), 1);
    assert!(!root.join("launches.txt").exists());
    let other = Setup::new();
    let mut wrong =
        ReceivedWorkerSupervisor::new(1, mesh_types::PublicKey::from_bytes([0; 32])).unwrap();
    assert!(wrong
        .start_received(
            *received_handoff(&other, false),
            resident_launch(&fixture, Arc::new(Signers))
        )
        .is_err());
    assert!(wrong.admissions().is_empty());
    assert!(ReceivedWorkerSupervisor::new(0, resident_key(&setup)).is_err());
    assert!(ReceivedWorkerSupervisor::new(65, resident_key(&setup)).is_err());
}

struct StopResident(Arc<std::sync::atomic::AtomicBool>);
impl Drop for StopResident {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::Release);
    }
}

#[test]
fn resident_loop_observes_completion_after_control_disconnect_and_reply_backpressure() {
    use crate::fleet::{
        receiving_broker::tests::received_handoff, receiving_session::tests::Setup,
    };
    use std::sync::{atomic::AtomicBool, mpsc};
    let setup = Setup::new();
    let fixture = Fixture::new();
    let mut resident = ReceivedWorkerSupervisor::new(1, resident_key(&setup)).unwrap();
    let (control, mailbox) = ReceivedWorkerMailbox::bounded();
    let (observations, observed) = mpsc::sync_channel(1);
    let stop = Arc::new(AtomicBool::new(false));
    std::thread::scope(|scope| {
        let stop_loop = stop.clone();
        let owner = scope.spawn(move || {
            resident.serve(&mailbox, &stop_loop, &observations);
            (resident, mailbox)
        });
        // Also stop on a failed assertion, before the scoped thread join.
        let stop_guard = StopResident(stop.clone());
        let (reply, received) = mpsc::sync_channel(1);
        control
            .try_send(ReceivedWorkerRequest::Start {
                handoff: received_handoff(&setup, true),
                launch: Box::new(resident_launch(&fixture, Arc::new(Signers))),
                reply,
            })
            .unwrap_or_else(|_| panic!("empty native mailbox"));
        let admission = received
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
            .unwrap();
        // Deliberately fill one reply channel. The next native request must still complete even
        // while both that reply and the observation queue cannot accept another sample.
        let (full, _undrained) = mpsc::sync_channel(1);
        full.send(Err(unavailable("fixture-full-reply"))).unwrap();
        control
            .try_send(ReceivedWorkerRequest::Snapshot {
                admission: admission.clone(),
                reply: full,
            })
            .unwrap_or_else(|_| panic!("native mailbox available"));
        let (reply, received) = mpsc::sync_channel(1);
        control
            .try_send(ReceivedWorkerRequest::Snapshot {
                admission: admission.clone(),
                reply,
            })
            .unwrap_or_else(|_| panic!("native mailbox available"));
        let snapshot = received
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
            .unwrap();
        let root = std::path::PathBuf::from(
            snapshot.get("lanes").unwrap().as_array().unwrap()[0]
                .get("workspace")
                .unwrap()
                .get("root")
                .unwrap()
                .as_text()
                .unwrap(),
        );
        wait_started(&root);
        drop(control);
        fs::write(root.join("finish"), b"finish").unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .expect("autonomous completion");
            let sample = observed.recv_timeout(remaining).unwrap();
            if sample.iter().any(|entry| {
                entry.admission == admission
                    && entry.observation.as_ref().is_ok_and(|workers| {
                        workers.iter().any(|worker| worker.outcome == Some(true))
                    })
            }) {
                break;
            }
        }
        // Explicit native stop returns the original ownership; connection loss did not stop it.
        drop(stop_guard);
        let (resident, _mailbox) = owner.join().unwrap();
        assert_eq!(resident.admissions().len(), 1);
        assert!(resident.snapshot(&admission).is_ok());
        assert_eq!(
            fs::read_to_string(root.join("launches.txt")).unwrap(),
            "one\n"
        );
    });
}

#[test]
fn resident_mailbox_returns_original_handoff_when_full_or_disconnected() {
    use crate::fleet::{
        receiving_broker::tests::received_handoff, receiving_session::tests::Setup,
    };
    use std::sync::mpsc::{self, TrySendError};
    let setup = Setup::new();
    let handoff = received_handoff(&setup, false);
    let root = handoff.allocation.path().to_path_buf();
    let admission = handoff.allocation.admission.as_ref().unwrap().clone();
    let (control, mailbox) = ReceivedWorkerMailbox::bounded();
    for _ in 0..32 {
        let (reply, _received) = mpsc::sync_channel(1);
        control
            .try_send(ReceivedWorkerRequest::Snapshot {
                admission: admission.clone(),
                reply,
            })
            .unwrap_or_else(|_| panic!("within fixed mailbox bound"));
    }
    let fixture = Fixture::new();
    let (reply, _received) = mpsc::sync_channel(1);
    let request = ReceivedWorkerRequest::Start {
        handoff,
        launch: Box::new(resident_launch(&fixture, Arc::new(Signers))),
        reply,
    };
    let Err(TrySendError::Full(request)) = control.try_send(request) else {
        panic!("bounded queue must return original request");
    };
    drop(mailbox);
    let Err(TrySendError::Disconnected(ReceivedWorkerRequest::Start { handoff, .. })) =
        control.try_send(request)
    else {
        panic!("disconnected queue must return original handoff");
    };
    assert_eq!(handoff.allocation.path(), root);
    assert!(handoff.allocation.admission.as_ref().unwrap() == &admission);
    assert!(!fixture.0.join("s").exists());
    assert!(!root.join("launches.txt").exists());
}

#[cfg(target_os = "macos")]
pub(in crate::fleet) fn saved_result_fixture(
    fixture: &Fixture,
    reservation: crate::fleet::RemoteLaunchReservation,
    worker_key: ed25519_dalek::SigningKey,
) -> String {
    let root = std::path::PathBuf::from(reservation.workspace().binding().root());
    let mut worker = ReceivedWorkerHost::start(
        reservation,
        adapter(fixture),
        &fixture.0.join("s"),
        Arc::new(Signers),
    )
    .unwrap();
    wait_started(&root);
    worker.poll().unwrap();
    let objective = worker.service.objective().unwrap();
    let credential = worker.host.test_owned_credential("lane").to_owned();
    let mut client = Client::open(worker.endpoint());
    fs::write(root.join("note.txt"), b"saved native worker result\n").unwrap();
    let checkpoint = result(client.call(
        &objective,
        &credential,
        "checkpoint",
        Json::object([("request", Json::text("saved"))]),
    ));
    result(client.call(
        &objective,
        &credential,
        "submit_review",
        Json::object([("checkpoint", checkpoint.get("checkpoint").unwrap().clone())]),
    ));
    drop(client);
    fs::write(root.join("finish"), b"finish").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(outcome) = worker.poll().unwrap()[0].outcome {
            assert!(outcome);
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    worker
        .publish_saved_result_at(&Signer(worker_key), Instant::now())
        .unwrap()
        .unwrap();
    let reviews = worker.service.saved_reviews("lane", None).unwrap();
    let checkpoint = &reviews.get("reviews").unwrap().as_array().unwrap()[0];
    let selection = crate::fleet::service::SavedReviewSelection::new(
        "lane",
        checkpoint.get("checkpoint").unwrap().as_text().unwrap(),
        checkpoint.get("version").unwrap().as_text().unwrap(),
        checkpoint.get("bundle").unwrap().as_text().unwrap(),
    )
    .unwrap();
    let (offer, _) = worker
        .service
        .sign_remote_saved_review(&selection, |_| panic!("already signed"))
        .unwrap();
    fs::write(root.join("note.txt"), b"unsaved later bytes").unwrap();
    assert_eq!(fs::read(root.join("launches.txt")).unwrap(), b"one\n");
    drop(worker);
    offer.encode()
}

mod recovery;
