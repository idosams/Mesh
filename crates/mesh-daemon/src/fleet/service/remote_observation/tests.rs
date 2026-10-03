use super::*;
use crate::fleet::remote_admission::authentication::tests::Fixture;
use crate::fleet::{RemoteDispatchPolicy, RemoteResultDiscoveryQuery, RemoteWorkerStatusQuery};
use ed25519_dalek::{Signer as _, SigningKey};
struct NoAllocation;
impl LaneAllocator for NoAllocation {
    fn allocate(&self, _: &str, _: &VersionInput) -> Result<LaneWorkspace, Unavailable> {
        panic!("read-only observation must never allocate")
    }
}
fn public(key: &SigningKey) -> PublicKey {
    PublicKey::from_bytes(key.verifying_key().to_bytes())
}
fn sign(key: &SigningKey, payload: &SigningPayload) -> Result<Signature, String> {
    Ok(Signature::from_bytes(
        key.sign(payload.as_bytes()).to_bytes(),
    ))
}
use signing::guarded_runtime;
fn history(f: &Fixture) -> FleetHistory {
    FleetHistory(Arc::new(
        FleetService::new(
            guarded_runtime(f),
            Arc::new(NoAllocation),
            ["codex".into()].into(),
        )
        .unwrap(),
    ))
}
fn prepare(h: &FleetHistory, f: &Fixture, results: bool) -> RemoteObservation {
    h.prepare_remote_observation(
        "lane",
        "run",
        public(&f.coordinator),
        public(&f.worker),
        if results {
            RemoteObservationKind::Results { after: 0 }
        } else {
            RemoteObservationKind::CurrentLease
        },
        |p| sign(&f.coordinator, p),
    )
    .unwrap()
}
fn reply(f: &Fixture, frame: RemoteFrame, results: bool) -> RemoteFrame {
    let RemoteFrame::Control(bytes) = frame else {
        panic!("control required")
    };
    let encoded = std::str::from_utf8(&bytes).unwrap();
    let policy = RemoteDispatchPolicy {
        coordinator: public(&f.coordinator),
        worker: public(&f.worker),
        provider: "codex",
        maximum: crate::fleet::Limits {
            lanes: 2,
            concurrency: 1,
            depth: 1,
            retries: 1,
        },
        max_lease_ms: 60_000,
    };
    let registry = f.registry();
    if results {
        RemoteResultDiscoveryQuery::decode(encoded)
            .unwrap()
            .verify(&policy)
            .unwrap()
            .reply(&registry, |p| sign(&f.worker, p))
            .unwrap()
    } else {
        RemoteWorkerStatusQuery::decode(encoded)
            .unwrap()
            .verify(&policy)
            .unwrap()
            .reply(&registry, |p| sign(&f.worker, p))
            .unwrap()
    }
}
#[test]
fn observation_releases_service_lock_and_never_changes_history() {
    for results in [false, true] {
        let f = Fixture::new();
        let h = history(&f);
        let before = h.0.native_state().unwrap();
        let read = prepare(&h, &f, results);
        let outcome = read
            .exchange(|frame| {
                assert!(
                    h.0.inner.try_lock().is_ok(),
                    "network must not retain service lock"
                );
                assert_eq!(h.0.native_state().unwrap(), before);
                Ok(reply(&f, frame, results))
            })
            .unwrap();
        assert!(matches!(
            (results, outcome),
            (false, RemoteObservationOutcome::CurrentLease(_))
                | (true, RemoteObservationOutcome::Results(None))
        ));
        assert_eq!(h.0.native_state().unwrap(), before);
        drop(h);
        // Reopen only committed history; never re-run the fixture launch setup.
        let reopened = Runtime::open(
            crate::fleet::FleetStore::open(f.path.join("coordinator.sqlite")).unwrap(),
            "objective",
        )
        .unwrap();
        assert_eq!(reopened.state(), &before);
    }
}
#[test]
fn changed_assignment_context_and_transport_failure_never_adopt_a_reply() {
    for results in [false, true] {
        let f = Fixture::new();
        let h = history(&f);
        let read = prepare(&h, &f, results);
        assert!(read
            .exchange(|frame| {
                assert!(h.0.inner.try_lock().is_ok());
                let response = reply(&f, frame, results);
                h.0.native_command(
                    "observed-while-offline",
                    Command::Observe {
                        lane: "lane".into(),
                        run: "run".into(),
                        state: RunState::Running,
                    },
                )
                .unwrap();
                Ok(response)
            })
            .is_err());
        let before = h.0.native_state().unwrap();
        assert!(prepare(&h, &f, results)
            .exchange(|_| Err(io::Error::other("disconnected")))
            .is_err());
        assert!(prepare(&h, &f, results)
            .exchange(|_| Ok(RemoteFrame::Control(b"invalid reply".to_vec())))
            .is_err());
        assert_eq!(h.0.native_state().unwrap(), before);
        assert_eq!(before.lanes["lane"].runs.len(), 1);
    }
}
#[test]
fn native_identity_and_cursor_refusals_happen_before_signing() {
    let f = Fixture::new();
    let h = history(&f);
    for (worker, kind) in [
        (
            PublicKey::from_bytes([99; 32]),
            RemoteObservationKind::CurrentLease,
        ),
        (
            public(&f.worker),
            RemoteObservationKind::Results { after: 4097 },
        ),
    ] {
        assert!(h
            .prepare_remote_observation(
                "lane",
                "run",
                public(&f.coordinator),
                worker,
                kind,
                |_| panic!("invalid input must not sign")
            )
            .is_err());
    }
}

#[test]
fn explicit_input_inspection_releases_lock_and_revalidates_the_original_attempt() {
    use crate::fleet::receiving_session::tests::Setup;
    for change in [false, true] {
        let setup = Setup::new();
        let f = &setup.f;
        let h = history(f);
        let before = h.0.native_state().unwrap();
        let read = h
            .prepare_remote_observation(
                "lane",
                "run",
                public(&f.coordinator),
                public(&f.worker),
                RemoteObservationKind::InputInspection,
                |p| sign(&f.coordinator, p),
            )
            .unwrap();
        let result = read.exchange(|frame| {
            assert!(h.0.inner.try_lock().is_ok());
            let RemoteFrame::Control(bytes) = frame else {
                panic!("control")
            };
            let policy = RemoteDispatchPolicy {
                coordinator: public(&f.coordinator),
                worker: public(&f.worker),
                provider: "codex",
                maximum: crate::fleet::Limits {
                    lanes: 2,
                    concurrency: 1,
                    depth: 1,
                    retries: 1,
                },
                max_lease_ms: 60_000,
            };
            let reply = RemoteWorkerStatusQuery::decode(std::str::from_utf8(&bytes).unwrap())
                .unwrap()
                .verify(&policy)
                .unwrap()
                .reply_with_input_inspection(&f.registry(), &setup.destination, |p| {
                    sign(&f.worker, p)
                })
                .unwrap();
            if change {
                h.0.native_command(
                    "changed-during-inspection",
                    Command::Observe {
                        lane: "lane".into(),
                        run: "run".into(),
                        state: RunState::Running,
                    },
                )
                .unwrap();
            }
            Ok(reply)
        });
        if change {
            assert!(result.is_err());
        } else {
            let RemoteObservationOutcome::InputInspection(receipt) = result.unwrap() else {
                panic!("inspection")
            };
            assert_eq!(receipt.input_inspection(), Some("unrecorded"));
            assert_eq!(h.0.native_state().unwrap(), before);
        }
        assert_eq!(h.0.native_state().unwrap().lanes["lane"].runs.len(), 1);
        assert_eq!(
            std::fs::read_dir(f.path.join("allocations"))
                .unwrap()
                .count(),
            0
        );
    }
}

#[test]
fn execution_observation_is_explicit_read_only_and_context_bound() {
    let f = Fixture::new();
    let h = history(&f);
    let before = h.0.native_state().unwrap();
    let prepare_execution = || {
        h.prepare_remote_observation(
            "lane",
            "run",
            public(&f.coordinator),
            public(&f.worker),
            RemoteObservationKind::Execution,
            |p| sign(&f.coordinator, p),
        )
        .unwrap()
    };
    let observed = prepare_execution()
        .exchange(|frame| {
            assert!(h.0.inner.try_lock().is_ok());
            let RemoteFrame::Control(ref bytes) = frame else {
                panic!("control");
            };
            assert!(std::str::from_utf8(bytes).unwrap().contains("query/v4"));
            Ok(reply(&f, frame, false))
        })
        .unwrap();
    let RemoteObservationOutcome::Execution(receipt) = observed else {
        panic!("execution");
    };
    assert!(receipt.reports_execution());
    assert_eq!(receipt.recorded_execution(), None);
    assert_eq!(h.0.native_state().unwrap(), before);
    assert!(prepare_execution()
        .exchange(|_| Err(io::Error::other("offline")))
        .is_err());
    assert!(prepare_execution()
        .exchange(|_| Ok(RemoteFrame::Control(b"invalid".to_vec())))
        .is_err());
    assert_eq!(h.0.native_state().unwrap(), before);
    assert!(prepare_execution()
        .exchange(|frame| {
            let signed = reply(&f, frame, false);
            h.0.native_command(
                "context-advanced",
                Command::Observe {
                    lane: "lane".into(),
                    run: "run".into(),
                    state: RunState::Running,
                },
            )
            .unwrap();
            Ok(signed)
        })
        .is_err());
    assert_eq!(h.0.native_state().unwrap().lanes["lane"].runs.len(), 1);
}

#[test]
fn execution_service_reads_original_worker_completion_without_adopting_it() {
    use crate::fleet::{
        NativeRemoteInputReceiver, RemoteAdmissionOutcome, RemoteExecutionState,
        RemoteLaunchOutcome,
    };
    let s = crate::fleet::receiving_session::tests::Setup::new();
    let mut registry = s.f.registry();
    let RemoteAdmissionOutcome::Reserved(input) = registry
        .reserve(
            s.f.work.clone(),
            "0123456789abcdef0123456789abcdef",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
        )
        .unwrap()
    else {
        panic!("input");
    };
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
        .reserve_launch(
            workspace,
            "codex",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
        )
        .unwrap()
    else {
        panic!("launch");
    };
    let worker = reservation.into_native_session().unwrap();
    let h = history(&s.f);
    let coordinator_before = h.0.native_state().unwrap();
    for (request, state) in [
        ("worker-running", RunState::Running),
        ("worker-done", RunState::Succeeded),
    ] {
        worker
            .native_command(
                request,
                Command::Observe {
                    lane: "lane".into(),
                    run: "run".into(),
                    state,
                },
            )
            .unwrap();
        let original = worker.native_state().unwrap();
        let observation = h
            .prepare_remote_observation(
                "lane",
                "run",
                public(&s.f.coordinator),
                public(&s.f.worker),
                RemoteObservationKind::Execution,
                |p| sign(&s.f.coordinator, p),
            )
            .unwrap();
        let RemoteObservationOutcome::Execution(receipt) = observation
            .exchange(|frame| Ok(reply(&s.f, frame, false)))
            .unwrap()
        else {
            panic!("execution");
        };
        let recorded = receipt.recorded_execution().unwrap();
        assert_eq!(recorded.state, RemoteExecutionState::Recorded(state));
        assert_eq!(recorded.revision, original.revision);
        assert_eq!(worker.native_state().unwrap(), original);
        assert_eq!(h.0.native_state().unwrap(), coordinator_before);
    }
}

mod signing;
