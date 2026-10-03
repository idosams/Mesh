use super::*;
use mesh_store::fleet::{FleetStoreAuthority, FleetStoreError};
use std::sync::atomic::{AtomicBool, Ordering};
#[derive(Debug)]
struct Authority {
    path: std::path::PathBuf,
    held: std::fs::File,
    active: AtomicBool,
}
impl FleetStoreAuthority for Authority {
    fn check(&self) -> Result<(), FleetStoreError> {
        use std::os::unix::fs::MetadataExt;
        let ok = (|| -> std::io::Result<bool> {
            let current = std::fs::symlink_metadata(&self.path)?;
            let held = self.held.metadata()?;
            Ok(self.active.load(Ordering::SeqCst)
                && current.is_file()
                && !current.file_type().is_symlink()
                && current.nlink() == 1
                && (current.dev(), current.ino()) == (held.dev(), held.ino()))
        })()
        .unwrap_or(false);
        if ok {
            Ok(())
        } else {
            Err(FleetStoreError::AuthorityChanged)
        }
    }
}
fn guarded(f: &Fixture) -> (Runtime, Arc<Authority>) {
    drop(f.runtime(true));
    let path = f.path.canonicalize().unwrap().join("coordinator.sqlite");
    let authority = Arc::new(Authority {
        held: std::fs::File::open(&path).unwrap(),
        path: path.clone(),
        active: AtomicBool::new(true),
    });
    let runtime = Runtime::open(
        crate::fleet::FleetStore::open_guarded(&path, false, authority.clone()).unwrap(),
        "objective",
    )
    .unwrap();
    (runtime, authority)
}
pub(super) fn guarded_runtime(f: &Fixture) -> Runtime {
    guarded(f).0
}
fn kind(index: usize) -> RemoteObservationKind {
    match index {
        0 => RemoteObservationKind::CurrentLease,
        1 => RemoteObservationKind::InputInspection,
        _ => RemoteObservationKind::Results { after: 0 },
    }
}
#[test]
fn signing_does_not_hold_fleet_mutex_and_preserves_parallel_observation() {
    for index in 0..3 {
        let setup = crate::fleet::receiving_session::tests::Setup::new();
        let f = &setup.f;
        let h = history(f);
        let before = h.0.native_state().unwrap();
        let query = h
            .prepare_remote_observation(
                "lane",
                "run",
                public(&f.coordinator),
                public(&f.worker),
                kind(index),
                |p| {
                    assert!(
                        h.0.inner.try_lock().is_ok(),
                        "signing must release the shared fleet mutex"
                    );
                    assert_eq!(h.0.native_state().unwrap(), before);
                    sign(&f.coordinator, p)
                },
            )
            .unwrap();
        let outcome = query
            .exchange(|frame| {
                if index != 1 {
                    return Ok(reply(f, frame, index == 2));
                }
                let RemoteFrame::Control(bytes) = frame else {
                    panic!("control");
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
                Ok(
                    RemoteWorkerStatusQuery::decode(std::str::from_utf8(&bytes).unwrap())
                        .unwrap()
                        .verify(&policy)
                        .unwrap()
                        .reply_with_input_inspection(&f.registry(), &setup.destination, |p| {
                            sign(&f.worker, p)
                        })
                        .unwrap(),
                )
            })
            .unwrap();
        assert!(matches!(
            (index, outcome),
            (0, RemoteObservationOutcome::CurrentLease(_))
                | (1, RemoteObservationOutcome::InputInspection(_))
                | (2, RemoteObservationOutcome::Results(None))
        ));
        assert_eq!(h.0.native_state().unwrap(), before);
    }
}
#[test]
fn cancellation_during_signing_suppresses_every_prepared_query() {
    for index in 0..3 {
        let f = Fixture::new();
        let h = history(&f);
        assert!(h
            .prepare_remote_observation(
                "lane",
                "run",
                public(&f.coordinator),
                public(&f.worker),
                kind(index),
                |p| {
                    assert!(h.0.inner.try_lock().is_ok());
                    h.0.native_command("cancel-during-signing", Command::Cancel)
                        .unwrap();
                    sign(&f.coordinator, p)
                }
            )
            .is_err());
        let state = h.0.native_state().unwrap();
        assert!(state.cancelled);
        assert_eq!(state.lanes["lane"].runs.len(), 1);
    }
}
#[test]
fn signing_failure_and_revoked_guard_never_produce_a_query() {
    for index in 0..3 {
        let f = Fixture::new();
        let (runtime, authority) = guarded(&f);
        let h = FleetHistory(Arc::new(
            FleetService::new(runtime, Arc::new(NoAllocation), ["codex".into()].into()).unwrap(),
        ));
        let before = h.0.native_state().unwrap();
        assert!(h
            .prepare_remote_observation(
                "lane",
                "run",
                public(&f.coordinator),
                public(&f.worker),
                kind(index),
                |_| Err("signer unavailable".into())
            )
            .is_err());
        assert_eq!(h.0.native_state().unwrap(), before);
        assert!(h
            .prepare_remote_observation(
                "lane",
                "run",
                public(&f.coordinator),
                public(&f.worker),
                kind(index),
                |p| {
                    authority.active.store(false, Ordering::SeqCst);
                    sign(&f.coordinator, p)
                }
            )
            .is_err());
        assert!(h
            .prepare_remote_observation(
                "lane",
                "run",
                public(&f.coordinator),
                public(&f.worker),
                kind(index),
                |_| panic!("revoked history must refuse before signing")
            )
            .is_err());
    }
}
#[test]
fn unguarded_observation_history_is_not_promoted_to_native_authority() {
    let f = Fixture::new();
    let h = FleetHistory(Arc::new(
        FleetService::new(
            f.runtime(true),
            Arc::new(NoAllocation),
            ["codex".into()].into(),
        )
        .unwrap(),
    ));
    assert!(h
        .prepare_remote_observation(
            "lane",
            "run",
            public(&f.coordinator),
            public(&f.worker),
            kind(0),
            |_| panic!("unguarded history must refuse before signing")
        )
        .is_err());
}
