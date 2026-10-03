use super::*;
use crate::fleet::remote_admission::launch::tests::Fixture;
use crate::fleet::Runtime;

#[test]
fn restarted_execution_observations_preserve_exact_intent_and_never_mutate_history() {
    for terminal in [RunState::Succeeded, RunState::Failed, RunState::Cancelled] {
        let fixture = Fixture::new();
        let reservation = fixture.session_reservation();
        let launch = reservation.receipt().clone();
        let service = reservation.into_native_session().unwrap();
        let scope = service.objective().unwrap();
        drop(service);
        let mut runtime = Runtime::open(
            FleetStore::open(fixture.0.join("worker.sqlite")).unwrap(),
            &scope,
        )
        .unwrap();
        for (request, state) in [("running", RunState::Running), ("terminal", terminal)] {
            runtime
                .record(
                    request,
                    Command::Observe {
                        lane: "lane".into(),
                        run: "run".into(),
                        state,
                    },
                )
                .unwrap();
        }
        let before = runtime.store.events(&scope, 0, 256).unwrap();
        drop(runtime);
        let mut registry = fixture.registry();
        let observation = registry
            .execution_observation("assignment")
            .unwrap()
            .unwrap();
        assert!(observation.launch() == &launch);
        assert_eq!(observation.revision(), 6);
        assert_eq!(
            observation.state(),
            RemoteExecutionState::Recorded(terminal)
        );
        assert_eq!(registry.store.events(&scope, 0, 256).unwrap(), before);
        assert!(registry.launch_receipt("assignment").unwrap().as_ref() == Some(&launch));
        assert_eq!(registry.receipts().unwrap().len(), 1);
        let mut next = launch.admission().work().clone();
        next.assignment.id = "another-assignment".into();
        next.lane = "another-lane".into();
        next.run = "another-run".into();
        assert!(matches!(
            registry.reserve(next, "11111111111111111111111111111111", 100),
            Err(Error::Refused("remote-admission-capacity"))
        ));
        assert!(registry
            .execution_observation("another-assignment")
            .is_err());
    }
}

#[test]
fn every_original_setup_prefix_remains_explicitly_incomplete_without_repair() {
    let fixture = Fixture::new();
    let reservation = fixture.session_reservation();
    let binding = reservation.workspace().binding().clone();
    let launch = reservation.receipt().clone();
    drop(reservation);
    let mut registry = fixture.registry();
    let scope = session_scope(&registry.launch_stream("assignment"));
    let work = launch.admission().work();
    let commands = [
        (
            "start",
            Command::Start {
                goal: work.goal.clone(),
                limits: session_limits(),
            },
        ),
        (
            "lane",
            Command::CreateLane {
                id: work.lane.clone(),
                parent: None,
                goal: work.goal.clone(),
                provider: work.provider.clone(),
                base: work.assignment.input,
            },
        ),
        (
            "workspace",
            Command::BindWorkspace {
                lane: work.lane.clone(),
                binding,
            },
        ),
        (
            "dispatch",
            Command::Dispatch {
                lane: work.lane.clone(),
                run: work.run.clone(),
            },
        ),
    ];
    for prefix in 0..=4 {
        let observation = registry
            .execution_observation("assignment")
            .unwrap()
            .unwrap();
        assert_eq!(observation.revision(), prefix as u64);
        assert_eq!(
            observation.state(),
            match prefix {
                0 => RemoteExecutionState::Unrecorded,
                4 => RemoteExecutionState::Recorded(RunState::Launching),
                _ => RemoteExecutionState::SetupIncomplete,
            }
        );
        assert_eq!(registry.store.revision(&scope).unwrap(), prefix as u64);
        if let Some((request, command)) = commands.get(prefix) {
            registry
                .store
                .append(&scope, prefix as u64, request, &wire::encode(command))
                .unwrap();
        }
    }
}

#[test]
fn cancellation_stays_stopping_and_malformed_or_wrong_setup_never_becomes_absence() {
    let fixture = Fixture::new();
    let service = fixture.session_reservation().into_native_session().unwrap();
    let scope = service.objective().unwrap();
    drop(service);
    let mut runtime = Runtime::open(
        FleetStore::open(fixture.0.join("worker.sqlite")).unwrap(),
        &scope,
    )
    .unwrap();
    runtime.record("cancel", Command::Cancel).unwrap();
    assert_eq!(
        fixture
            .registry()
            .execution_observation("assignment")
            .unwrap()
            .unwrap()
            .state(),
        RemoteExecutionState::Recorded(RunState::Stopping)
    );
    runtime.store.append(&scope, 5, "unknown", "{}").unwrap();
    assert!(fixture
        .registry()
        .execution_observation("assignment")
        .is_err());

    for wrong_request in [false, true] {
        let fixture = Fixture::new();
        drop(fixture.session_reservation());
        let mut registry = fixture.registry();
        let scope = session_scope(&registry.launch_stream("assignment"));
        let command = Command::Start {
            goal: if wrong_request {
                "Private work"
            } else {
                "Different work"
            }
            .into(),
            limits: session_limits(),
        };
        registry
            .store
            .append(
                &scope,
                0,
                if wrong_request {
                    "other-start"
                } else {
                    "start"
                },
                &wire::encode(&command),
            )
            .unwrap();
        assert!(registry.execution_observation("assignment").is_err());
        assert_eq!(registry.store.revision(&scope).unwrap(), 1);
    }
}

#[test]
fn observation_refuses_revoked_native_ledger_authority() {
    use mesh_store::fleet::{FleetStoreAuthority, FleetStoreError};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    #[derive(Debug)]
    struct Authority(AtomicBool);
    impl FleetStoreAuthority for Authority {
        fn check(&self) -> Result<(), FleetStoreError> {
            if self.0.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err(FleetStoreError::AuthorityChanged)
            }
        }
    }
    let fixture = Fixture::new();
    let authority = Arc::new(Authority(AtomicBool::new(true)));
    let store = FleetStore::open_guarded(
        &fixture.0.canonicalize().unwrap().join("worker.sqlite"),
        true,
        authority.clone(),
    )
    .unwrap();
    let registry = RemoteAdmissionRegistry::new(
        store,
        &"ab".repeat(32),
        &"cd".repeat(32),
        "objective",
        session_limits(),
    )
    .unwrap();
    let service = fixture
        .session_reservation_with(registry)
        .into_native_session()
        .unwrap();
    drop(service);
    let store = FleetStore::open_guarded(
        &fixture.0.canonicalize().unwrap().join("worker.sqlite"),
        false,
        authority.clone(),
    )
    .unwrap();
    let registry = RemoteAdmissionRegistry::new(
        store,
        &"ab".repeat(32),
        &"cd".repeat(32),
        "objective",
        session_limits(),
    )
    .unwrap();
    assert!(registry
        .execution_observation("assignment")
        .unwrap()
        .is_some());
    authority.0.store(false, Ordering::SeqCst);
    assert!(registry.execution_observation("assignment").is_err());
}

#[test]
fn observation_reads_completion_beyond_the_first_event_page() {
    let fixture = Fixture::new();
    let service = fixture.session_reservation().into_native_session().unwrap();
    let scope = service.objective().unwrap();
    drop(service);
    let mut runtime = Runtime::open(
        FleetStore::open(fixture.0.join("worker.sqlite")).unwrap(),
        &scope,
    )
    .unwrap();
    for index in 0..MAX_FLEET_EVENT_PAGE {
        runtime
            .record(
                &format!("running-{index}"),
                Command::Observe {
                    lane: "lane".into(),
                    run: "run".into(),
                    state: RunState::Running,
                },
            )
            .unwrap();
    }
    runtime
        .record(
            "completion",
            Command::Observe {
                lane: "lane".into(),
                run: "run".into(),
                state: RunState::Succeeded,
            },
        )
        .unwrap();
    let revision = runtime.state().revision;
    drop(runtime);
    let observation = fixture
        .registry()
        .execution_observation("assignment")
        .unwrap()
        .unwrap();
    assert_eq!(observation.revision(), revision);
    assert_eq!(
        observation.state(),
        RemoteExecutionState::Recorded(RunState::Succeeded)
    );
}
