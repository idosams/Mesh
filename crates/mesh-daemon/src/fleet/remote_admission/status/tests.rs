use super::*;
use crate::fleet::remote_admission::authentication::tests::Fixture;
use ed25519_dalek::{Signer as _, SigningKey};
fn public(k: &SigningKey) -> PublicKey {
    PublicKey::from_bytes(k.verifying_key().to_bytes())
}
fn sign(k: &SigningKey, p: &SigningPayload) -> Result<Signature, String> {
    Ok(Signature::from_bytes(k.sign(p.as_bytes()).to_bytes()))
}
fn policy(f: &Fixture) -> RemoteDispatchPolicy<'static> {
    RemoteDispatchPolicy {
        coordinator: public(&f.coordinator),
        worker: public(&f.worker),
        provider: "codex",
        maximum: Limits {
            lanes: 2,
            concurrency: 1,
            depth: 1,
            retries: 1,
        },
        max_lease_ms: 60_000,
    }
}
fn challenge(f: &Fixture, r: &mut Runtime) -> RemoteWorkerStatusChallenge {
    RemoteWorkerStatusChallenge::issue(r, "lane", "run", public(&f.coordinator), public(&f.worker))
        .unwrap()
}
fn control(frame: RemoteFrame) -> String {
    let RemoteFrame::Control(v) = frame else {
        panic!("control expected")
    };
    String::from_utf8(v).unwrap()
}
#[test]
fn signed_facts_survive_reopen_without_granting_another_reservation() {
    let f = Fixture::new();
    let mut runtime = f.runtime(true);
    let mut registry = f.registry();
    let RemoteAdmissionOutcome::Reserved(original) = registry
        .reserve(
            f.work.clone(),
            "0123456789abcdef0123456789abcdef",
            now().unwrap(),
        )
        .unwrap()
    else {
        panic!("original required")
    };
    drop(original);
    drop(registry);
    let mut registry = f.registry();
    let c = challenge(&f, &mut runtime);
    let q = RemoteWorkerStatusQuery::decode(
        &c.signed_query(&mut runtime, |p| sign(&f.coordinator, p))
            .unwrap()
            .encode(),
    )
    .unwrap()
    .verify(&policy(&f))
    .unwrap();
    let encoded = control(q.reply(&registry, |p| sign(&f.worker, p)).unwrap());
    let receipt = c.verify_reply(&mut runtime, &encoded).unwrap();
    assert_eq!(
        receipt
            .facts()
            .get("admission")
            .unwrap()
            .get("revision")
            .and_then(Json::as_u64),
        Some(1)
    );
    assert!(matches!(receipt.facts().get("launch"), Some(Json::Null)));
    assert_eq!(registry.receipts().unwrap().len(), 1);
    assert!(matches!(
        registry
            .reserve(
                f.work.clone(),
                "0123456789abcdef0123456789abcdef",
                now().unwrap()
            )
            .unwrap(),
        RemoteAdmissionOutcome::Retained(_)
    ));
    assert_eq!(runtime.state().lanes["lane"].runs.len(), 1);
}
#[test]
fn wrong_keys_stale_queries_and_replayed_replies_refuse() {
    let f = Fixture::new();
    let mut runtime = f.runtime(true);
    let registry = f.registry();
    let c = challenge(&f, &mut runtime);
    let q = c
        .signed_query(&mut runtime, |p| sign(&f.coordinator, p))
        .unwrap();
    let encoded = q.encode();
    let mut wrong = policy(&f);
    wrong.coordinator = public(&SigningKey::from_bytes(&[99; 32]));
    assert!(RemoteWorkerStatusQuery::decode(&encoded)
        .unwrap()
        .verify(&wrong)
        .is_err());
    let stale = number(&q.body, "expires_ms").unwrap();
    assert!(RemoteWorkerStatusQuery::decode(&encoded)
        .unwrap()
        .verify_at(&policy(&f), stale)
        .is_err());
    let q = q.verify(&policy(&f)).unwrap();
    assert!(q
        .reply(&registry, |p| sign(&SigningKey::from_bytes(&[98; 32]), p))
        .is_err());
    let reply = control(q.reply(&registry, |p| sign(&f.worker, p)).unwrap());
    let other = challenge(&f, &mut runtime);
    assert!(other.verify_reply(&mut runtime, &reply).is_err());
    let r = c.verify_reply(&mut runtime, &reply).unwrap();
    assert!(matches!(r.facts().get("admission"), Some(Json::Null)));
    assert!(registry.receipts().unwrap().is_empty());
}
#[test]
fn expired_initial_lease_does_not_prevent_fresh_read_only_recovery() {
    for version in [StatusVersion::Initial, StatusVersion::Effective] {
        let f = Fixture::new();
        let mut runtime = f.runtime(true);
        let mut registry = f.registry();
        registry
            .reserve(
                f.work.clone(),
                "0123456789abcdef0123456789abcdef",
                now().unwrap(),
            )
            .unwrap();
        let later = f.work.assignment.lease_until_ms + 1;
        let mut c = RemoteWorkerStatusChallenge::issue_at(
            &mut runtime,
            "lane",
            "run",
            public(&f.coordinator),
            public(&f.worker),
            later,
            [27; 32],
        )
        .unwrap();
        c.version = version;
        let q = RemoteWorkerStatusQuery {
            version,
            signature: sign(&f.coordinator, &payload(version.query_domain(), &c.body)).unwrap(),
            body: c.body.clone(),
        }
        .verify_at(&policy(&f), later)
        .unwrap();
        let reply = control(
            q.reply_at(&registry, |p| sign(&f.worker, p), later)
                .unwrap(),
        );
        let facts = c.verify_at(&mut runtime, &reply, later).unwrap();
        assert_eq!(
            facts.reports_effective_lease(),
            version == StatusVersion::Effective
        );
        if version == StatusVersion::Effective {
            let lease = facts.effective_lease().unwrap();
            assert_eq!(lease.sequence, 1);
            assert!(lease.until_ms < later);
        }
        assert_eq!(
            facts
                .facts()
                .get("admission")
                .unwrap()
                .get("initial_lease_until_ms")
                .and_then(Json::as_u64),
            Some(f.work.assignment.lease_until_ms)
        );
        assert_eq!(
            runtime.state().lanes["lane"]
                .runs
                .last()
                .unwrap()
                .remote
                .as_ref()
                .unwrap()
                .lease_sequence,
            1
        );
    }
}

#[test]
fn changed_context_history_and_unknown_fields_cannot_reuse_status_authority() {
    let f = Fixture::new();
    let mut runtime = f.runtime(true);
    let mut registry = f.registry();
    registry
        .reserve(
            f.work.clone(),
            "0123456789abcdef0123456789abcdef",
            now().unwrap(),
        )
        .unwrap();
    let c = challenge(&f, &mut runtime);
    let q = c
        .signed_query(&mut runtime, |p| sign(&f.coordinator, p))
        .unwrap();
    let Json::Object(mut envelope) = Json::parse(&q.encode()).unwrap() else {
        panic!("object")
    };
    envelope.push(("unknown".into(), Json::Null));
    assert!(RemoteWorkerStatusQuery::decode(&Json::Object(envelope).encode()).is_err());
    let verified = q.verify(&policy(&f)).unwrap();
    let reply = control(verified.reply(&registry, |p| sign(&f.worker, p)).unwrap());
    runtime
        .record("cancel", crate::fleet::Command::Cancel)
        .unwrap();
    assert!(c.verify_reply(&mut runtime, &reply).is_err());
    // Cancellation does not erase history; a new exact-context read remains available.
    let c = challenge(&f, &mut runtime);
    let q = c
        .signed_query(&mut runtime, |p| sign(&f.coordinator, p))
        .unwrap()
        .verify(&policy(&f))
        .unwrap();
    let mut other = f.registry();
    assert!(q
        .reply(&registry, |p| {
            other
                .store
                .append(&other.stream, 1, "changed-history", "{}")
                .unwrap();
            sign(&f.worker, p)
        })
        .is_err());
}

#[test]
fn resident_broker_returns_signed_launch_intent_without_launching_or_changing_ownership() {
    for version in [StatusVersion::Initial, StatusVersion::Effective] {
        use crate::fleet::{
            receiving_session::tests::Setup, NativeRemoteInputReceiver, NativeWorkerConnections,
            NativeWorkerInstallation, RemoteLaunchOutcome, WorkerConnectionOutcome,
        };
        use crate::{CheckpointRuntimeParameters, ProtectedWorkspaceRoot, TrustedReviewers};
        use std::os::unix::{fs::PermissionsExt as _, net::UnixStream};
        let s = Setup::new();
        let mut runtime = s.f.runtime(true);
        let root = s.f.path.join("installation");
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let (installation, ()) = NativeWorkerInstallation::provision(
            &root,
            ProtectedWorkspaceRoot::inspect(&root).unwrap(),
            &[],
            |_| Ok((public(&s.f.worker), ())),
        )
        .unwrap();
        let limits = policy(&s.f).maximum;
        let mut registry = installation
            .registry(
                &hex(public(&s.f.coordinator).as_bytes()),
                "objective",
                limits.clone(),
            )
            .unwrap();
        let RemoteAdmissionOutcome::Reserved(reservation) = registry
            .reserve(
                s.f.work.clone(),
                "0123456789abcdef0123456789abcdef",
                now().unwrap(),
            )
            .unwrap()
        else {
            panic!("reservation")
        };
        let mut receiver = NativeRemoteInputReceiver::new(
            &s.destination,
            s.manifest.clone(),
            &s.f.work.assignment,
        )
        .unwrap();
        receiver.accept(s.digest, 0, &s.bytes, true).unwrap();
        let workspace = receiver
            .materialize_reserved(reservation)
            .unwrap()
            .into_worker_workspace(
                TrustedReviewers::default(),
                CheckpointRuntimeParameters::selected_defaults(),
            )
            .unwrap();
        let RemoteLaunchOutcome::Reserved(launch) = registry
            .reserve_launch(workspace, "codex", now().unwrap())
            .unwrap()
        else {
            panic!("original launch intent")
        };
        let initial = launch.receipt().initial_operation();
        drop(launch);
        drop(receiver);
        let mut hub =
            NativeWorkerConnections::new(&installation, &s.destination, policy(&s.f), 1).unwrap();
        let mut c = challenge(&s.f, &mut runtime);
        c.version = version;
        let q = c
            .signed_query(&mut runtime, |p| sign(&s.f.coordinator, p))
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
        let result = std::thread::scope(|scope| {
            let worker = scope.spawn(|| {
                hub.serve(server.try_clone().unwrap(), server, |p| {
                    sign(&s.f.worker, p)
                })
            });
            let result = exchange(&mut runtime, c, q, client.try_clone().unwrap(), client).unwrap();
            assert!(matches!(
                worker.join().unwrap().unwrap(),
                WorkerConnectionOutcome::StatusReplied
            ));
            result
        });
        assert_eq!(
            result.reports_effective_lease(),
            version == StatusVersion::Effective
        );
        if version == StatusVersion::Effective {
            assert_eq!(result.effective_lease().unwrap().sequence, 1);
        }
        assert_eq!(
            result.target().get("assignment").and_then(Json::as_text),
            Some("assignment")
        );
        assert_eq!(
            result
                .facts()
                .get("launch")
                .unwrap()
                .get("worker_initial")
                .and_then(Json::as_text),
            Some(initial.to_string().as_str())
        );
        assert_eq!(
            runtime.state().lanes["lane"].runs.last().unwrap().state,
            crate::fleet::RunState::Launching
        );
        let registry = installation
            .registry(
                &hex(public(&s.f.coordinator).as_bytes()),
                "objective",
                limits,
            )
            .unwrap();
        assert_eq!(registry.receipts().unwrap().len(), 1);
        assert_eq!(
            registry
                .launch_receipt("assignment")
                .unwrap()
                .unwrap()
                .initial_operation(),
            initial
        );
    }
}

#[test]
fn v2_reports_renewed_lease_while_v1_keeps_initial_history_and_no_state_changes() {
    let f = Fixture::new();
    let mut runtime = f.runtime(true);
    let mut registry = f.registry();
    registry
        .reserve(
            f.work.clone(),
            "0123456789abcdef0123456789abcdef",
            now().unwrap(),
        )
        .unwrap();
    let admission = registry.receipts().unwrap().remove(0);
    let renewed = registry
        .renew_lease(
            &admission,
            1,
            f.work.assignment.lease_until_ms + 30_000,
            now().unwrap(),
            120_000,
        )
        .unwrap();
    drop(registry);
    let registry = f.registry();
    let revision = runtime.state().revision;
    for version in [StatusVersion::Initial, StatusVersion::Effective] {
        let mut c = challenge(&f, &mut runtime);
        c.version = version;
        let query = c
            .signed_query(&mut runtime, |p| sign(&f.coordinator, p))
            .unwrap();
        let verified = RemoteWorkerStatusQuery::decode(&query.encode())
            .unwrap()
            .verify(&policy(&f))
            .unwrap();
        let reply = control(verified.reply(&registry, |p| sign(&f.worker, p)).unwrap());
        let receipt = c.verify_reply(&mut runtime, &reply).unwrap();
        assert_eq!(
            receipt.reports_effective_lease(),
            version == StatusVersion::Effective
        );
        assert_eq!(
            receipt.effective_lease(),
            (version == StatusVersion::Effective).then(|| renewed.clone())
        );
        assert_eq!(
            receipt
                .facts()
                .get("admission")
                .unwrap()
                .get("initial_lease_until_ms")
                .and_then(Json::as_u64),
            Some(f.work.assignment.lease_until_ms)
        );
        assert_eq!(runtime.state().revision, revision);
        assert_eq!(registry.receipts().unwrap().len(), 1);
    }
}

#[test]
fn v2_domain_cannot_be_downgraded_and_null_admission_stays_unknown() {
    let f = Fixture::new();
    let mut runtime = f.runtime(true);
    let registry = f.registry();
    let c = RemoteWorkerStatusChallenge::issue_with_current_lease(
        &mut runtime,
        "lane",
        "run",
        public(&f.coordinator),
        public(&f.worker),
    )
    .unwrap();
    let query = c
        .signed_query(&mut runtime, |p| sign(&f.coordinator, p))
        .unwrap();
    let downgraded = query
        .encode()
        .replace("mesh.worker-status-query/v2", "mesh.worker-status-query/v1");
    assert!(RemoteWorkerStatusQuery::decode(&downgraded)
        .unwrap()
        .verify(&policy(&f))
        .is_err());
    let reply = control(
        query
            .verify(&policy(&f))
            .unwrap()
            .reply(&registry, |p| sign(&f.worker, p))
            .unwrap(),
    );
    let receipt = c.verify_reply(&mut runtime, &reply).unwrap();
    assert!(receipt.reports_effective_lease());
    assert!(receipt.effective_lease().is_none());
    assert!(registry.receipts().unwrap().is_empty());
}

#[test]
fn v2_refuses_a_lease_changed_during_signing() {
    let f = Fixture::new();
    let mut runtime = f.runtime(true);
    let mut registry = f.registry();
    registry
        .reserve(
            f.work.clone(),
            "0123456789abcdef0123456789abcdef",
            now().unwrap(),
        )
        .unwrap();
    let admission = registry.receipts().unwrap().remove(0);
    let c = RemoteWorkerStatusChallenge::issue_with_current_lease(
        &mut runtime,
        "lane",
        "run",
        public(&f.coordinator),
        public(&f.worker),
    )
    .unwrap();
    let query = c
        .signed_query(&mut runtime, |p| sign(&f.coordinator, p))
        .unwrap()
        .verify(&policy(&f))
        .unwrap();
    assert!(query
        .reply(&registry, |p| {
            f.registry()
                .renew_lease(
                    &admission,
                    1,
                    f.work.assignment.lease_until_ms + 30_000,
                    now().unwrap(),
                    120_000,
                )
                .unwrap();
            sign(&f.worker, p)
        })
        .is_err());
    assert_eq!(
        runtime.state().lanes["lane"].runs[0]
            .remote
            .as_ref()
            .unwrap()
            .lease_sequence,
        1
    );
}
