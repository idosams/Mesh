use super::*;
use crate::fleet::receiving_session::tests::Setup;
use crate::fleet::{
    Command, NativeRemoteInputReceiver, RemoteAdmissionOutcome, RemoteLaunchOutcome,
};
use ed25519_dalek::{Signer as _, SigningKey};
const ID: &str = "0123456789abcdef0123456789abcdef";
fn public(key: &SigningKey) -> PublicKey {
    PublicKey::from_bytes(key.verifying_key().to_bytes())
}
fn sign(key: &SigningKey, payload: &SigningPayload) -> Result<Signature, String> {
    Ok(Signature::from_bytes(
        key.sign(payload.as_bytes()).to_bytes(),
    ))
}
fn encoded(frame: RemoteFrame) -> String {
    let RemoteFrame::Control(bytes) = frame else {
        panic!("control");
    };
    String::from_utf8(bytes).unwrap()
}
fn prepare(s: &Setup, phase: &str) -> Option<Runtime> {
    if phase == "absent" {
        return None;
    }
    let mut registry = s.f.registry();
    let RemoteAdmissionOutcome::Reserved(input) = registry
        .reserve(s.f.work.clone(), ID, now().unwrap())
        .unwrap()
    else {
        panic!("original input");
    };
    if phase == "admitted" {
        return None;
    }
    let mut receiver =
        NativeRemoteInputReceiver::new(&s.destination, s.manifest.clone(), &s.f.work.assignment)
            .unwrap();
    receiver.accept(s.digest, 0, &s.bytes, true).unwrap();
    let workspace = receiver
        .materialize_reserved(input)
        .unwrap()
        .into_worker_workspace(
            crate::TrustedReviewers::default(),
            crate::CheckpointRuntimeParameters::selected_defaults(),
        )
        .unwrap();
    let RemoteLaunchOutcome::Reserved(reservation) = registry
        .reserve_launch(workspace, "codex", now().unwrap())
        .unwrap()
    else {
        panic!("original launch");
    };
    if phase == "unrecorded" {
        return None;
    }
    let service = reservation.into_native_session().unwrap();
    let scope = service.objective().unwrap();
    drop(service);
    let mut runtime = Runtime::open(
        FleetStore::open(s.f.path.join("worker.sqlite")).unwrap(),
        &scope,
    )
    .unwrap();
    if phase != "launching" {
        runtime
            .record(
                "running",
                Command::Observe {
                    lane: "lane".into(),
                    run: "run".into(),
                    state: RunState::Running,
                },
            )
            .unwrap();
    }
    match phase {
        "succeeded" => {
            runtime
                .record(
                    "complete",
                    Command::Observe {
                        lane: "lane".into(),
                        run: "run".into(),
                        state: RunState::Succeeded,
                    },
                )
                .unwrap();
        }
        "stopping" => {
            runtime.record("cancel", Command::Cancel).unwrap();
        }
        _ => (),
    }
    Some(runtime)
}
fn query(
    s: &Setup,
    runtime: &mut Runtime,
    version: StatusVersion,
) -> (RemoteWorkerStatusChallenge, VerifiedRemoteWorkerStatusQuery) {
    let mut challenge = RemoteWorkerStatusChallenge::issue_with_execution_observation(
        runtime,
        "lane",
        "run",
        public(&s.f.coordinator),
        public(&s.f.worker),
    )
    .unwrap();
    challenge.version = version;
    let signed = challenge
        .signed_query(runtime, |p| sign(&s.f.coordinator, p))
        .unwrap()
        .encode();
    let policy = RemoteDispatchPolicy {
        coordinator: public(&s.f.coordinator),
        worker: public(&s.f.worker),
        provider: "codex",
        maximum: Limits {
            lanes: 2,
            concurrency: 1,
            depth: 1,
            retries: 1,
        },
        max_lease_ms: 60_000,
    };
    let verified = RemoteWorkerStatusQuery::decode(&signed)
        .unwrap()
        .verify(&policy)
        .unwrap();
    (challenge, verified)
}
#[test]
fn fresh_signed_execution_reply_correlates_restarted_original_state_without_mutation() {
    for phase in [
        "absent",
        "admitted",
        "unrecorded",
        "launching",
        "running",
        "succeeded",
        "stopping",
    ] {
        let s = Setup::new();
        let worker = prepare(&s, phase);
        let expected = match phase {
            "absent" | "admitted" => None,
            "unrecorded" => Some(RemoteRecordedExecution {
                revision: 0,
                state: RemoteExecutionState::Unrecorded,
            }),
            other => Some(RemoteRecordedExecution {
                revision: worker.as_ref().unwrap().state().revision,
                state: RemoteExecutionState::Recorded(match other {
                    "launching" => RunState::Launching,
                    "running" => RunState::Running,
                    "succeeded" => RunState::Succeeded,
                    _ => RunState::Stopping,
                }),
            }),
        };
        drop(worker);
        let registry = s.f.registry();
        let mut runtime = s.f.runtime(true);
        let before = runtime.state().clone();
        let (challenge, verified) = query(&s, &mut runtime, StatusVersion::Execution);
        let reply = encoded(verified.reply(&registry, |p| sign(&s.f.worker, p)).unwrap());
        let receipt = challenge.verify_reply(&mut runtime, &reply).unwrap();
        assert!(receipt.reports_execution());
        assert_eq!(receipt.recorded_execution(), expected);
        assert_eq!(receipt.input_inspection(), None);
        assert_eq!(runtime.state(), &before);
        if phase != "absent" {
            let before = registry.execution_observation("assignment").unwrap();
            let after = s.f.registry().execution_observation("assignment").unwrap();
            assert!(before == after);
            assert_eq!(registry.receipts().unwrap().len(), 1);
        }
    }
}
#[test]
fn worker_advancement_during_signing_suppresses_stale_execution_reply() {
    let s = Setup::new();
    let mut worker = prepare(&s, "running").unwrap();
    let mut runtime = s.f.runtime(true);
    let (_, verified) = query(&s, &mut runtime, StatusVersion::Execution);
    assert!(verified
        .reply(&s.f.registry(), |p| {
            worker
                .record(
                    "complete",
                    Command::Observe {
                        lane: "lane".into(),
                        run: "run".into(),
                        state: RunState::Succeeded,
                    },
                )
                .unwrap();
            sign(&s.f.worker, p)
        })
        .is_err());
    assert_eq!(
        worker.state().lanes["lane"].runs[0].state,
        RunState::Succeeded
    );
}
#[test]
fn wrong_signer_old_version_expiry_and_changed_coordinator_cannot_confirm_execution() {
    let s = Setup::new();
    prepare(&s, "running");
    let mut runtime = s.f.runtime(true);
    let (_, verified) = query(&s, &mut runtime, StatusVersion::Execution);
    assert!(verified
        .reply(&s.f.registry(), |_| Err("fixture signer failure".into()))
        .is_err());
    assert!(verified
        .reply(&s.f.registry(), |p| sign(
            &SigningKey::from_bytes(&[0x44; 32]),
            p
        ))
        .is_err());
    let (old, verified) = query(&s, &mut runtime, StatusVersion::Effective);
    let reply = encoded(
        verified
            .reply(&s.f.registry(), |p| sign(&s.f.worker, p))
            .unwrap(),
    );
    let (new, _) = query(&s, &mut runtime, StatusVersion::Execution);
    assert!(new.verify_reply(&mut runtime, &reply).is_err());
    let old = old.verify_reply(&mut runtime, &reply).unwrap();
    assert!(!old.reports_execution());
    assert_eq!(old.recorded_execution(), None);
    let (expired, verified) = query(&s, &mut runtime, StatusVersion::Execution);
    let reply = encoded(
        verified
            .reply(&s.f.registry(), |p| sign(&s.f.worker, p))
            .unwrap(),
    );
    let expiry = number(&expired.body, "expires_ms").unwrap();
    assert!(expired.verify_at(&mut runtime, &reply, expiry).is_err());
    let (changed, verified) = query(&s, &mut runtime, StatusVersion::Execution);
    let reply = encoded(
        verified
            .reply(&s.f.registry(), |p| sign(&s.f.worker, p))
            .unwrap(),
    );
    runtime.record("cancel", Command::Cancel).unwrap();
    assert!(changed.verify_reply(&mut runtime, &reply).is_err());
}
#[test]
fn execution_schema_is_closed_and_revision_state_combinations_are_exact() {
    for (revision, state) in [
        (0, "running"),
        (1, "succeeded"),
        (4, "running"),
        (4, "unrecorded"),
        (4, "setup-incomplete"),
        (u64::MAX, "running"),
        (4, "complete"),
        (4, "Running"),
    ] {
        assert!(parse_execution(&Json::object([
            ("revision", Json::Number(revision)),
            ("state", Json::text(state))
        ]))
        .is_err());
    }
    for revision in 1..=3 {
        assert_eq!(
            parse_execution(&Json::object([
                ("revision", Json::Number(revision)),
                ("state", Json::text("setup-incomplete"))
            ]))
            .unwrap()
            .state,
            RemoteExecutionState::SetupIncomplete
        );
    }
    let s = Setup::new();
    prepare(&s, "succeeded");
    let mut runtime = s.f.runtime(true);
    let (_, verified) = query(&s, &mut runtime, StatusVersion::Execution);
    let facts = verified
        .execution_facts(&s.f.registry(), verified.facts(&s.f.registry()).unwrap())
        .unwrap();
    assert!(canonical_execution_facts(&facts).is_ok());
    let Json::Object(mut fields) = facts.clone() else {
        panic!("object");
    };
    fields.push(("extra".into(), Json::Bool(true)));
    assert!(canonical_execution_facts(&Json::Object(fields)).is_err());
    let without_launch = Json::object([
        ("admission", facts.get("admission").unwrap().clone()),
        ("launch", Json::Null),
        (
            "effective_lease",
            facts.get("effective_lease").unwrap().clone(),
        ),
        ("execution", facts.get("execution").unwrap().clone()),
    ]);
    assert!(canonical_execution_facts(&without_launch).is_err());
}
