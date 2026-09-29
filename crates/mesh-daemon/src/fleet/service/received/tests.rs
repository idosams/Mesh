use super::*;
use crate::fleet::provider::CodexAdapter;
use crate::fleet::remote_admission::launch::tests::Fixture;
use std::fs;
use std::os::unix::fs::PermissionsExt;

#[cfg(target_os = "macos")]
#[test]
fn native_session_retains_worker_directory_lock_until_last_service_owner_is_dropped() {
    use crate::fleet::NativeRemoteWorkerDirectory;
    use crate::ProtectedWorkspaceRoot;
    let fixture = Fixture::new();
    let path = fixture.0.join("worker");
    let token = ProtectedWorkspaceRoot::inspect(&path).unwrap();
    let directory =
        NativeRemoteWorkerDirectory::create(&path, token, &"cd".repeat(32), &[]).unwrap();
    let registry = directory
        .registry(&"ab".repeat(32), "objective", limits())
        .unwrap();
    let service = Arc::new(
        fixture
            .session_reservation_with(registry)
            .into_native_session()
            .unwrap(),
    );
    let another_owner = service.clone();
    drop(directory);
    drop(service);
    assert!(NativeRemoteWorkerDirectory::reopen(&path, token, &"cd".repeat(32), &[]).is_err());
    another_owner.native_state().unwrap();
    drop(another_owner);
    let directory =
        NativeRemoteWorkerDirectory::reopen(&path, token, &"cd".repeat(32), &[]).unwrap();
    assert!(directory
        .registry(&"ab".repeat(32), "objective", limits())
        .unwrap()
        .launch_receipt("assignment")
        .unwrap()
        .is_some());
}

#[test]
fn received_session_retains_original_daemon_and_exact_independent_version_binding() {
    let fixture = Fixture::new();
    let reservation = fixture.session_reservation();
    let receipt = reservation.receipt().clone();
    let daemon = reservation.workspace().daemon().clone();
    let binding = reservation.workspace().binding().clone();
    let service = reservation.into_native_session().unwrap();
    let scope = service.objective().unwrap();
    assert_ne!(scope, receipt.admission().objective());
    let state = service.native_state().unwrap();
    assert_eq!(state.limits, Some(limits()));
    assert_eq!(state.lanes.len(), 1);
    let lane = &state.lanes["lane"];
    assert_eq!(lane.base, receipt.admission().work().assignment.input);
    assert_eq!(lane.workspace.as_ref(), Some(&binding));
    assert_eq!(
        binding.starting_version(),
        Some(receipt.initial_operation())
    );
    assert_eq!(lane.runs.len(), 1);
    assert_eq!(lane.runs[0].id, "run");
    assert_eq!(lane.runs[0].state, RunState::Launching);
    assert!(lane.runs[0].launch_owner.is_none());
    {
        let inner = service.lock().unwrap();
        assert!(Arc::ptr_eq(inner.workspaces["lane"].daemon(), &daemon));
    }
    drop(service);
    let recovered = Runtime::open(
        FleetStore::open(fixture.0.join("worker.sqlite")).unwrap(),
        &scope,
    )
    .unwrap();
    assert_eq!(recovered.state().revision, state.revision);
    assert_eq!(
        recovered.state().lanes["lane"].workspace.as_ref(),
        Some(&binding)
    );
}

#[test]
fn received_session_cannot_delegate_or_retry_outside_the_coordinator_budget() {
    let fixture = Fixture::new();
    let service = fixture.session_reservation().into_native_session().unwrap();
    let credential = service
        .grant("lane", "run", "native-actor", "native-session")
        .unwrap();
    let initial = service.native_state().unwrap().lanes["lane"]
        .workspace
        .as_ref()
        .unwrap()
        .starting_version()
        .unwrap();
    assert!(service
        .agent_call(
            credential.transport_value(),
            "delegate",
            &Json::object([
                ("request", Json::text("child")),
                ("goal", Json::text("Child task")),
                ("provider", Json::text("codex")),
                ("version", Json::text(initial.to_string())),
            ])
        )
        .is_err());
    service
        .native_command(
            "failed",
            Command::Observe {
                lane: "lane".into(),
                run: "run".into(),
                state: RunState::Failed,
            },
        )
        .unwrap();
    assert!(service
        .native_command(
            "retry",
            Command::Dispatch {
                lane: "lane".into(),
                run: "another".into()
            }
        )
        .is_err());
    let state = service.native_state().unwrap();
    assert_eq!(state.lanes.len(), 1);
    assert_eq!(state.lanes["lane"].runs.len(), 1);
}

#[test]
fn prelaunch_validation_rechecks_lease_provider_and_exact_attempt_without_losing_observation() {
    let fixture = Fixture::new();
    let service = fixture.session_reservation().into_native_session().unwrap();
    let inner = service.lock().unwrap();
    let received = inner.received.as_ref().unwrap();
    let expiry = received
        .receipt
        .admission()
        .work()
        .assignment
        .lease_until_ms;
    received
        .verify_launch(&inner.runtime, "lane", "run", "codex", expiry - 1)
        .unwrap();
    for now in [0, expiry, expiry + 1] {
        assert_eq!(
            received
                .verify_launch(&inner.runtime, "lane", "run", "codex", now)
                .unwrap_err()
                .code,
            "remote-session-lease-expired"
        );
    }
    assert!(received
        .verify_launch(&inner.runtime, "other", "run", "codex", expiry - 1)
        .is_err());
    assert!(received
        .verify_launch(&inner.runtime, "lane", "other", "codex", expiry - 1)
        .is_err());
    assert!(received
        .verify_launch(&inner.runtime, "lane", "run", "claude", expiry - 1)
        .is_err());
    // Resource/history verification is deliberately independent of launch lease validity.
    received.verify(&inner.runtime).unwrap();
}

#[test]
fn changed_input_and_extra_launch_history_refuse_without_erasing_session_state() {
    for corrupt_input in [false, true] {
        let fixture = Fixture::new();
        let service = fixture.session_reservation().into_native_session().unwrap();
        let scope = service.objective().unwrap();
        let revision = service.native_state().unwrap().revision;
        if corrupt_input {
            fs::write(
                fixture
                    .0
                    .join("allocations/input-0123456789abcdef0123456789abcdef/files/changed"),
                b"changed",
            )
            .unwrap();
        } else {
            let launch_stream = service
                .lock()
                .unwrap()
                .received
                .as_ref()
                .unwrap()
                .launch
                .stream
                .clone();
            FleetStore::open(fixture.0.join("worker.sqlite"))
                .unwrap()
                .append(&launch_stream, 1, "extra", "{}")
                .unwrap();
        }
        assert!(service.native_state().is_err());
        assert!(service.grant("lane", "run", "actor", "session").is_err());
        drop(service);
        let recovered = Runtime::open(
            FleetStore::open(fixture.0.join("worker.sqlite")).unwrap(),
            &scope,
        )
        .unwrap();
        assert_eq!(recovered.state().revision, revision);
    }
}

#[test]
fn received_session_uses_existing_native_provider_launch_and_refuses_second_spawn() {
    let fixture = Fixture::new();
    let service = fixture.session_reservation().into_native_session().unwrap();
    let executable = fixture.0.join("provider.sh");
    fs::write(&executable, b"#!/bin/sh\ncat >/dev/null\nprintf 'one\\n' >> provider-launch-count.txt\nprintf '%s\\n' '{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}'\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let adapter = CodexAdapter::new(&executable, &executable).unwrap();
    let credential = service.grant("lane", "run", "actor", "session").unwrap();
    let endpoint = fixture.0.join("native.sock");
    let mut process = service
        .start_codex(&credential, &adapter, &endpoint)
        .unwrap();
    let state = service.native_state().unwrap();
    assert_eq!(state.lanes["lane"].runs[0].state, RunState::Running);
    assert!(state.lanes["lane"].runs[0].launch_owner.is_some());
    assert!(service
        .start_codex(&credential, &adapter, &endpoint)
        .is_err());
    service.revoke(&credential).unwrap();
    let retained = Arc::downgrade(&service.inner);
    drop(service);
    assert!(
        retained.upgrade().is_some(),
        "process must retain the native session resources"
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(success) = process.poll().unwrap().1 {
            assert!(success);
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "fixture provider did not exit"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let root = state.lanes["lane"].workspace.as_ref().unwrap().root();
    assert_eq!(
        fs::read_to_string(Path::new(root).join("provider-launch-count.txt")).unwrap(),
        "one\n"
    );
    // This is actual process composition with a protocol fixture, not a real-provider/remote proof.
    drop(process);
    assert!(retained.upgrade().is_none());
}

#[test]
fn received_session_reads_renewed_lease_without_changing_original_attempt() {
    let fixture = Fixture::new();
    let reservation = fixture.session_reservation();
    let admission = reservation.receipt().admission().clone();
    let expiry = admission.work().assignment.lease_until_ms;
    let service = reservation.into_native_session().unwrap();
    let mut registry = fixture.registry();
    let inner = service.lock().unwrap();
    let received = inner.received.as_ref().unwrap();
    assert!(received
        .verify_launch(&inner.runtime, "lane", "run", "codex", expiry)
        .is_err());
    registry
        .renew_lease(&admission, 1, expiry + 1000, expiry - 1, 2000)
        .unwrap();
    received
        .verify_launch(&inner.runtime, "lane", "run", "codex", expiry + 1)
        .unwrap();
    assert!(received
        .verify_launch(&inner.runtime, "lane", "run", "codex", expiry + 1000)
        .is_err());
    assert_eq!(inner.runtime.state().lanes["lane"].runs.len(), 1);
    assert_eq!(registry.receipts().unwrap().len(), 1);
}
