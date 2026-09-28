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
    let mut worker = ReceivedWorkerHost::start(
        *reservation,
        adapter(&fixture),
        &fixture.0.join("s"),
        Arc::new(Signers),
    )
    .unwrap();
    assert!(worker.receipt() == &receipt);
    wait_started(&root);
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
    // Completion revoked the exact provider session. Reconnection cannot resurrect it.
    let mut connection = Client::open(worker.endpoint());
    assert!(matches!(
        connection.call(&objective, &credential, "context", Json::Object(vec![])),
        DaemonMessage::Failed { .. }
    ));
    drop(connection);
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
