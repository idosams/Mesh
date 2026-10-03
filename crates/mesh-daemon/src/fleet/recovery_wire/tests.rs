use super::*;
use crate::fleet::receiving_session::tests::{
    recovery::{guarded, materialized},
    Setup,
};
use ed25519_dalek::{Signer as _, SigningKey};
use std::os::unix::net::UnixStream;
use std::time::Duration;
fn public(k: &SigningKey) -> PublicKey {
    PublicKey::from_bytes(k.verifying_key().to_bytes())
}
fn sign(k: &SigningKey, p: &SigningPayload) -> Result<Signature, String> {
    Ok(Signature::from_bytes(k.sign(p.as_bytes()).to_bytes()))
}
fn policy(s: &Setup) -> RemoteDispatchPolicy<'static> {
    RemoteDispatchPolicy {
        coordinator: public(&s.f.coordinator),
        worker: public(&s.f.worker),
        provider: "codex",
        maximum: Limits {
            lanes: 2,
            concurrency: 1,
            depth: 1,
            retries: 1,
        },
        max_lease_ms: 180_000,
    }
}
fn ready() -> (Setup, Runtime, RemoteAdmissionRegistry) {
    let s = Setup::new();
    let mut runtime = s.f.runtime(true);
    let (input, registry) = materialized(&s, &mut runtime);
    drop(input);
    (s, runtime, registry)
}
fn client<'a>(s: &Setup, runtime: &'a mut Runtime) -> RemoteRecoveryClientRequest<'a> {
    RemoteRecoveryClientRequest {
        runtime,
        lane: "lane",
        run: "run",
        coordinator: public(&s.f.coordinator),
        worker: public(&s.f.worker),
    }
}
fn captured(s: &Setup, runtime: &mut Runtime) -> RemoteWorkerRecoveryRequest {
    let mut wire = Vec::new();
    assert!(
        recover_remote_worker(client(s, runtime), &b""[..], &mut wire, |p| sign(
            &s.f.coordinator,
            p
        ))
        .is_err()
    );
    RemoteWorkerRecoveryRequest::decode(
        &control(&mut RemoteFrameReader::new(wire.as_slice())).unwrap(),
    )
    .unwrap()
}
fn unopened(s: &Setup) {
    assert!(!s
        .f
        .path
        .join("allocations/input-0123456789abcdef0123456789abcdef/initialization.json")
        .exists());
}
fn pair() -> (UnixStream, UnixStream) {
    let (a, b) = UnixStream::pair().unwrap();
    for s in [&a, &b] {
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        s.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
    }
    (a, b)
}

#[test]
fn request_requires_closed_canonical_encoding_and_independent_policy() {
    let (s, mut runtime, _) = ready();
    let request = captured(&s, &mut runtime);
    let wire = request.encode();
    for malformed in [
        format!(" {wire}"),
        format!("{wire} "),
        wire.replace("request/v1", "request/v2"),
        format!("{{\"extra\":0,{}", &wire[1..]),
        " ".repeat(MAX + 1),
    ] {
        assert!(RemoteWorkerRecoveryRequest::decode(&malformed).is_err());
    }
    for n in 0..4 {
        let mut p = policy(&s);
        match n {
            0 => p.worker = public(&s.f.coordinator),
            1 => p.coordinator = public(&s.f.worker),
            2 => p.provider = "claude",
            _ => p.maximum.lanes = 1,
        };
        assert!(RemoteWorkerRecoveryRequest::decode(&wire)
            .unwrap()
            .verify(&p)
            .is_err());
    }
    let mut body = request.body.clone();
    if let Json::Object(fields) = &mut body {
        fields.push(("ignored".into(), Json::Bool(true)));
    }
    let signature = sign(&s.f.coordinator, &payload(REQUEST_DOMAIN, &body)).unwrap();
    assert!(RemoteWorkerRecoveryRequest::decode(&envelope(REQUEST, &body, &signature)).is_err());
    unopened(&s);
}

#[test]
fn valid_signature_cannot_change_original_registry_limits_or_freshness() {
    let (s, mut runtime, registry) = ready();
    let original = captured(&s, &mut runtime);
    let mut body = original.body.clone();
    if let Json::Object(fields) = &mut body {
        for (name, value) in fields {
            if name == "limits" {
                *value = limits_json(&Limits {
                    lanes: 1,
                    concurrency: 1,
                    depth: 1,
                    retries: 1,
                });
            }
        }
    }
    let signature = sign(&s.f.coordinator, &payload(REQUEST_DOMAIN, &body)).unwrap();
    let verified = RemoteWorkerRecoveryRequest::decode(&envelope(REQUEST, &body, &signature))
        .unwrap()
        .verify(&policy(&s))
        .unwrap();
    assert!(verified.check(&registry).is_err());
    let mut body = original.body.clone();
    let time = now().unwrap();
    if let Json::Object(fields) = &mut body {
        for (name, value) in fields {
            if name == "issued_ms" {
                *value = Json::Number(time - 60_000);
            }
            if name == "expires_ms" {
                *value = Json::Number(time - 30_000);
            }
        }
    }
    let signature = sign(&s.f.coordinator, &payload(REQUEST_DOMAIN, &body)).unwrap();
    assert!(
        RemoteWorkerRecoveryRequest::decode(&envelope(REQUEST, &body, &signature))
            .unwrap()
            .verify(&policy(&s))
            .is_err()
    );
    unopened(&s);
}

struct Corrupt<W> {
    inner: W,
    buffer: Vec<u8>,
    index: usize,
    change: Option<usize>,
}
impl<W: Write> Write for Corrupt<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        let bytes = std::mem::take(&mut self.buffer);
        if self.change == Some(self.index) {
            let encoded = control(&mut RemoteFrameReader::new(bytes.as_slice()))?;
            let value = Json::parse(&encoded).map_err(|_| refused())?;
            let corrupted = envelope(
                text(&value, "schema")?,
                value.get("body").ok_or_else(refused)?,
                &Signature::from_bytes([0; 64]),
            );
            RemoteFrameWriter::new(&mut self.inner).write_frame(&frame(corrupted)?)?;
        } else {
            self.inner.write_all(&bytes)?;
            self.inner.flush()?;
        }
        self.index += 1;
        Ok(())
    }
}
fn worker(
    s: &Setup,
    registry: RemoteAdmissionRegistry,
    server: UnixStream,
    change: Option<usize>,
) -> io::Result<RemoteRecoveryBrokerOutcome> {
    let mut input = server.try_clone()?;
    let request =
        RemoteWorkerRecoveryRequest::decode(&control(&mut RemoteFrameReader::new(&mut input))?)?
            .verify(&policy(s))?;
    serve_remote_recovery(
        RemoteRecoveryWorkerRequest {
            request,
            registry,
            destination: &s.destination,
            reviewers: crate::TrustedReviewers::default(),
            checkpoint: crate::CheckpointRuntimeParameters::selected_defaults(),
        },
        input,
        Corrupt {
            inner: server,
            buffer: Vec::new(),
            index: 0,
            change,
        },
        |p| sign(&s.f.worker, p),
    )
}
#[test]
fn renewed_lease_exchange_preserves_original_mapping_and_never_launches() {
    let (s, mut runtime, mut registry) = ready();
    let admission = registry.receipts().unwrap().remove(0);
    let deadline = admission.work().assignment.lease_until_ms + 30_000;
    registry
        .renew_lease(&admission, 1, deadline, now().unwrap(), 180_000)
        .unwrap();
    runtime
        .record(
            "renew",
            Command::AdvanceRemoteLease {
                lane: "lane".into(),
                run: "run".into(),
                assignment: "assignment".into(),
                worker_key: hex(public(&s.f.worker).as_bytes()),
                expected_sequence: 1,
                lease_until_ms: deadline,
            },
        )
        .unwrap();
    let (client_stream, server) = pair();
    let (receipt, outcome) = std::thread::scope(|scope| {
        let serving = scope.spawn(|| worker(&s, registry, server, None));
        let receipt = recover_remote_worker(
            client(&s, &mut runtime),
            client_stream.try_clone().unwrap(),
            client_stream,
            |p| sign(&s.f.coordinator, p),
        )
        .unwrap();
        (receipt, serving.join().unwrap().unwrap())
    });
    assert!(outcome.reply_written);
    outcome.handoff.workspace.verify().unwrap();
    assert!(outcome.handoff.workspace.admission() == &admission);
    assert_eq!(
        text(receipt.correlation(), "mapping").unwrap(),
        digest(&outcome.handoff.workspace.receipt().encode())
    );
    assert!(outcome
        .handoff
        .registry
        .launch_receipt("assignment")
        .unwrap()
        .is_none());
    assert_eq!(
        outcome
            .handoff
            .registry
            .effective_lease(&admission)
            .unwrap()
            .sequence,
        2
    );
}
#[test]
fn modified_worker_proof_or_final_receipt_cannot_be_accepted() {
    for change in [0, 1] {
        let (s, mut runtime, registry) = ready();
        let (client_stream, server) = pair();
        let outcome = std::thread::scope(|scope| {
            let serving = scope.spawn(|| worker(&s, registry, server, Some(change)));
            assert!(recover_remote_worker(
                client(&s, &mut runtime),
                client_stream.try_clone().unwrap(),
                client_stream,
                |p| sign(&s.f.coordinator, p)
            )
            .is_err());
            serving.join().unwrap()
        });
        if change == 0 {
            assert!(outcome.is_err());
            unopened(&s);
        } else {
            let outcome = outcome.unwrap();
            assert!(outcome.reply_written, "local write is not acceptance");
            outcome.handoff.workspace.verify().unwrap();
            assert!(outcome
                .handoff
                .registry
                .launch_receipt("assignment")
                .unwrap()
                .is_none());
        }
    }
}
#[test]
fn cancellation_during_either_coordinator_signature_sends_no_recovery_commit() {
    for stage in [0, 1] {
        let (s, mut runtime, registry) = ready();
        let (client_stream, server) = pair();
        let mut other = Runtime::open(
            mesh_store::fleet::FleetStore::open(s.f.path.join("coordinator.sqlite")).unwrap(),
            "objective",
        )
        .unwrap();
        let mut calls = 0;
        std::thread::scope(|scope| {
            let serving = scope.spawn(|| worker(&s, registry, server, None));
            assert!(recover_remote_worker(
                client(&s, &mut runtime),
                client_stream.try_clone().unwrap(),
                client_stream,
                |p| {
                    if calls == stage {
                        other
                            .record("cancel-during-wire-sign", Command::Cancel)
                            .unwrap();
                    }
                    calls += 1;
                    sign(&s.f.coordinator, p)
                }
            )
            .is_err());
            assert!(serving.join().unwrap().is_err());
        });
        unopened(&s);
    }
}
#[test]
fn missing_or_wrong_commit_preserves_acknowledged_input() {
    for wrong in [false, true] {
        let (s, mut runtime, registry) = ready();
        let request = captured(&s, &mut runtime);
        let (mut client_stream, server) = pair();
        std::thread::scope(|scope| {
            let serving = scope.spawn(|| worker(&s, registry, server, None));
            RemoteFrameWriter::new(&mut client_stream)
                .write_frame(&frame(request.encode()).unwrap())
                .unwrap();
            let _ = control(&mut RemoteFrameReader::new(&mut client_stream)).unwrap();
            if wrong {
                RemoteFrameWriter::new(&mut client_stream)
                    .write_frame(&frame("{}".into()).unwrap())
                    .unwrap();
            }
            drop(client_stream);
            assert!(serving.join().unwrap().is_err());
        });
        unopened(&s);
        assert_eq!(
            std::fs::read(
                s.f.path
                    .join("allocations/input-0123456789abcdef0123456789abcdef/files/result.txt")
            )
            .unwrap(),
            s.bytes
        );
        assert!(guarded(&s).launch_receipt("assignment").unwrap().is_none());
    }
}

mod process_crash;
