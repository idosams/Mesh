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
fn history(f: &Fixture) -> FleetHistory {
    FleetHistory(Arc::new(
        FleetService::new(
            f.runtime(true),
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
