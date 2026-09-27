use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "mesh-fleet-runtime-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn store(&self) -> FleetStore {
        FleetStore::open(self.0.join("fleet.sqlite")).unwrap()
    }
    fn runtime(&self) -> Runtime {
        Runtime::open(self.store(), "objective").unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn start() -> Command {
    Command::Start {
        goal: "Implement the requested outcome".into(),
        limits: Limits {
            lanes: 4,
            concurrency: 2,
            depth: 1,
            retries: 1,
        },
    }
}
fn lane(id: &str, parent: Option<&str>) -> Command {
    Command::CreateLane {
        id: id.into(),
        parent: parent.map(str::to_owned),
        goal: format!("Work on {id}"),
        provider: "fixture".into(),
        base: RecordDigest::from_bytes([1; 32]),
    }
}
fn register_lane(r: &mut Runtime, id: &str, parent: Option<&str>) {
    send(r, lane(id, parent));
    send(
        r,
        Command::BindWorkspace {
            lane: id.into(),
            binding: binding(id),
        },
    );
}
fn binding(id: &str) -> WorkspaceBinding {
    WorkspaceBinding {
        source_version: RecordDigest::from_bytes([1; 32]),
        root: format!("/verified/{id}"),
        digest: format!("digest-{id}"),
        installation: format!("installation-{id}"),
    }
}
fn dispatch(lane: &str, run: &str) -> Command {
    Command::Dispatch {
        lane: lane.into(),
        run: run.into(),
    }
}
fn observe(lane: &str, run: &str, state: RunState) -> Command {
    Command::Observe {
        lane: lane.into(),
        run: run.into(),
        state,
    }
}
fn send(runtime: &mut Runtime, command: Command) -> FleetEvent {
    let revision = runtime.state().revision;
    runtime
        .submit(revision, &format!("request-{revision}"), command)
        .unwrap()
}
fn refuses(runtime: &mut Runtime, command: Command, code: &str) {
    let before = runtime.state().clone();
    assert!(
        matches!(runtime.submit(before.revision, "refused", command), Err(Error::Refused(actual)) if actual == code)
    );
    assert_eq!(runtime.state(), &before);
}

#[test]
fn two_workers_survive_restart_and_lost_acknowledgments() {
    let fixture = Fixture::new();
    let mut runtime = fixture.runtime();
    send(&mut runtime, start());
    register_lane(&mut runtime, "coordinator", None);
    register_lane(&mut runtime, "first", Some("coordinator"));
    register_lane(&mut runtime, "second", Some("coordinator"));
    let launch = send(&mut runtime, dispatch("first", "run-1"));
    send(&mut runtime, dispatch("second", "run-2"));
    send(&mut runtime, observe("first", "run-1", RunState::Running));
    let before = runtime.state().clone();
    drop(runtime);
    let mut reopened = fixture.runtime();
    assert_eq!(reopened.state(), &before);
    assert_eq!(
        reopened
            .submit(
                launch.revision - 1,
                &launch.request,
                dispatch("first", "run-1")
            )
            .unwrap(),
        launch
    );
    assert_eq!(reopened.state().lanes["first"].runs.len(), 1);
    assert_eq!(
        reopened.state().lanes["second"].runs[0].state,
        RunState::Launching
    );
}

#[test]
fn uncertain_process_keeps_its_slot_until_reconciled() {
    let fixture = Fixture::new();
    let mut r = fixture.runtime();
    send(&mut r, start());
    for id in ["a", "b", "c"] {
        register_lane(&mut r, id, None);
    }
    send(&mut r, dispatch("a", "a1"));
    send(&mut r, dispatch("b", "b1"));
    send(&mut r, observe("a", "a1", RunState::Reconciling));
    refuses(&mut r, dispatch("c", "c1"), "concurrency-limit");
    send(&mut r, observe("a", "a1", RunState::Failed));
    send(&mut r, dispatch("c", "c1"));
}

#[test]
fn cancel_blocks_new_work_and_does_not_claim_workers_stopped() {
    let fixture = Fixture::new();
    let mut r = fixture.runtime();
    send(&mut r, start());
    register_lane(&mut r, "a", None);
    send(&mut r, dispatch("a", "a1"));
    send(&mut r, Command::Cancel);
    assert_eq!(r.state().lanes["a"].runs[0].state, RunState::Stopping);
    refuses(&mut r, lane("b", Some("a")), "objective-cancelled");
    refuses(&mut r, dispatch("a", "a2"), "objective-cancelled");
    refuses(
        &mut r,
        observe("a", "a1", RunState::Running),
        "invalid-run-transition",
    );
    send(&mut r, observe("a", "a1", RunState::Cancelled));
    drop(r);
    assert!(fixture.runtime().state().cancelled);
}

#[test]
fn retries_are_bounded_and_old_attempt_cannot_complete_new_attempt() {
    let fixture = Fixture::new();
    let mut r = fixture.runtime();
    send(&mut r, start());
    register_lane(&mut r, "a", None);
    send(&mut r, dispatch("a", "a1"));
    send(&mut r, observe("a", "a1", RunState::Failed));
    send(&mut r, dispatch("a", "a2"));
    refuses(&mut r, observe("a", "a1", RunState::Succeeded), "stale-run");
    send(&mut r, observe("a", "a2", RunState::Failed));
    refuses(&mut r, dispatch("a", "a3"), "retry-limit");
    assert_eq!(r.state().lanes["a"].runs.len(), 2);
}

#[test]
fn children_cannot_expand_objective_limits_or_create_cycles() {
    let fixture = Fixture::new();
    let mut r = fixture.runtime();
    send(&mut r, start());
    register_lane(&mut r, "a", None);
    register_lane(&mut r, "b", Some("a"));
    refuses(&mut r, lane("c", Some("b")), "delegation-limit");
    refuses(&mut r, lane("a", Some("b")), "lane-exists");
    refuses(&mut r, lane("c", Some("c")), "parent-missing");
    register_lane(&mut r, "c", None);
    register_lane(&mut r, "d", None);
    refuses(&mut r, lane("e", None), "lane-limit");
    refuses(&mut r, start(), "already-started");
}

#[test]
fn saved_work_is_distinct_from_execution_and_survives_replay() {
    let fixture = Fixture::new();
    let mut r = fixture.runtime();
    send(&mut r, start());
    register_lane(&mut r, "a", None);
    send(&mut r, dispatch("a", "a1"));
    let save = Command::Saved {
        lane: "a".into(),
        run: "a1".into(),
        version: RecordDigest::from_bytes([2; 32]),
    };
    refuses(&mut r, save.clone(), "run-not-saveable");
    send(&mut r, observe("a", "a1", RunState::Running));
    send(&mut r, save);
    assert_eq!(r.state().lanes["a"].runs[0].state, RunState::Running);
    send(&mut r, observe("a", "a1", RunState::Succeeded));
    assert_eq!(fixture.runtime().state(), r.state());
    assert_eq!(
        r.state().lanes["a"].saved,
        Some(RecordDigest::from_bytes([2; 32]))
    );
}

#[test]
fn independent_runtimes_reject_stale_decisions_and_refresh() {
    let fixture = Fixture::new();
    let mut a = fixture.runtime();
    let mut b = fixture.runtime();
    send(&mut a, start());
    assert!(matches!(
        b.submit(0, "different-start", start()),
        Err(Error::Store(FleetStoreError::StaleRevision { actual: 1 }))
    ));
    b.refresh().unwrap();
    register_lane(&mut b, "b", None);
    a.refresh().unwrap();
    assert_eq!(a.state(), b.state());
}

#[test]
fn unknown_persisted_commands_fail_closed_without_repair() {
    let fixture = Fixture::new();
    let mut store = fixture.store();
    store
        .append("objective", 0, "future-command", "{\"schema\":99}")
        .unwrap();
    assert!(matches!(
        Runtime::open(store, "objective"),
        Err(Error::InvalidHistory)
    ));
    assert_eq!(fixture.store().revision("objective").unwrap(), 1);
}

#[test]
fn command_encoding_has_an_explicit_version_and_rejects_extra_fields() {
    let encoded = wire::encode(&start());
    assert_eq!(
        encoded,
        r#"{"schema":1,"kind":"start","fields":{"goal":"Implement the requested outcome","lanes":4,"concurrency":2,"depth":1,"retries":1}}"#
    );
    assert_eq!(wire::decode(&encoded).unwrap(), start());
    let extra = encoded.replacen("{", "{\"unexpected\":true,", 1);
    assert!(matches!(wire::decode(&extra), Err(Error::InvalidHistory)));
}

#[test]
fn dispatch_requires_native_allocation_and_cannot_share_another_lanes_folder() {
    let fixture = Fixture::new();
    let mut r = fixture.runtime();
    send(&mut r, start());
    send(&mut r, lane("a", None));
    refuses(&mut r, dispatch("a", "a1"), "lane-not-allocated");
    send(
        &mut r,
        Command::BindWorkspace {
            lane: "a".into(),
            binding: binding("a"),
        },
    );
    send(&mut r, lane("b", None));
    refuses(
        &mut r,
        Command::BindWorkspace {
            lane: "b".into(),
            binding: binding("a"),
        },
        "workspace-already-bound",
    );
    refuses(
        &mut r,
        Command::BindWorkspace {
            lane: "a".into(),
            binding: binding("different"),
        },
        "lane-already-allocated",
    );
    send(&mut r, dispatch("a", "a1"));
}

#[test]
fn replay_crosses_event_page_boundaries_without_losing_state() {
    let fixture = Fixture::new();
    let mut r = fixture.runtime();
    send(&mut r, start());
    register_lane(&mut r, "a", None);
    send(&mut r, dispatch("a", "a1"));
    send(&mut r, observe("a", "a1", RunState::Running));
    for index in 0..260 {
        let state = if index % 2 == 0 {
            RunState::Waiting
        } else {
            RunState::Running
        };
        send(&mut r, observe("a", "a1", state));
    }
    assert_eq!(fixture.runtime().state(), r.state());
}

#[test]
fn allocation_must_match_the_lanes_exact_input_version() {
    let fixture = Fixture::new();
    let mut r = fixture.runtime();
    send(&mut r, start());
    send(&mut r, lane("a", None));
    let mut wrong = binding("a");
    wrong.source_version = RecordDigest::from_bytes([9; 32]);
    refuses(
        &mut r,
        Command::BindWorkspace {
            lane: "a".into(),
            binding: wrong,
        },
        "allocation-version-mismatch",
    );
}

#[test]
fn checkpoint_intent_survives_restart_and_completion_is_immutable_after_cancel() {
    let fixture = Fixture::new();
    let mut runtime = fixture.runtime();
    send(&mut runtime, start());
    register_lane(&mut runtime, "worker", None);
    send(&mut runtime, dispatch("worker", "run"));
    let begin = Command::BeginCheckpoint {
        id: "capture".into(),
        lane: "worker".into(),
        origin: AgentOrigin {
            actor: "actor".into(),
            session: "session".into(),
            run: "run".into(),
            generation: "a".repeat(32),
        },
        input_digest: "b".repeat(32),
    };
    let event = runtime.record("begin-capture", begin.clone()).unwrap();
    drop(runtime);
    let mut runtime = fixture.runtime();
    assert!(runtime.state().checkpoints["capture"].result.is_none());
    assert_eq!(runtime.record("begin-capture", begin).unwrap(), event);
    send(&mut runtime, Command::Cancel);
    let result = CheckpointResult {
        complete: true,
        version: RecordDigest::from_bytes([4; 32]),
        workspace_digest: "c".repeat(32),
        saved_changes: 2,
        issue: None,
    };
    let finish = Command::FinishCheckpoint {
        id: "capture".into(),
        result: result.clone(),
    };
    let finished = runtime.record("finish-capture", finish.clone()).unwrap();
    assert_eq!(
        runtime.record("finish-capture", finish.clone()).unwrap(),
        finished
    );
    refuses(&mut runtime, finish, "checkpoint-already-finished");
    let bundle = RecordDigest::from_bytes([5; 32]);
    let submitted = Command::SubmitReview {
        checkpoint: "capture".into(),
        bundle,
    };
    let review_event = runtime.record("review-capture", submitted.clone()).unwrap();
    assert_eq!(
        runtime.record("review-capture", submitted.clone()).unwrap(),
        review_event
    );
    refuses(&mut runtime, submitted, "checkpoint-review-exists");
    drop(runtime);
    let runtime = fixture.runtime();
    assert_eq!(runtime.state().checkpoints["capture"].result, Some(result));
    assert_eq!(runtime.state().checkpoints["capture"].review, Some(bundle));
    assert!(runtime.state().cancelled);
}

#[test]
fn checkpoint_refuses_invalid_completion_and_stale_run_without_advancing_history() {
    let fixture = Fixture::new();
    let mut runtime = fixture.runtime();
    send(&mut runtime, start());
    register_lane(&mut runtime, "worker", None);
    send(&mut runtime, dispatch("worker", "run"));
    let mut origin = AgentOrigin {
        actor: "actor".into(),
        session: "session".into(),
        run: "other-run".into(),
        generation: "a".repeat(32),
    };
    refuses(
        &mut runtime,
        Command::BeginCheckpoint {
            id: "capture".into(),
            lane: "worker".into(),
            origin: origin.clone(),
            input_digest: "b".repeat(32),
        },
        "stale-run",
    );
    origin.run = "run".into();
    send(
        &mut runtime,
        Command::BeginCheckpoint {
            id: "capture".into(),
            lane: "worker".into(),
            origin,
            input_digest: "b".repeat(32),
        },
    );
    refuses(
        &mut runtime,
        Command::FinishCheckpoint {
            id: "capture".into(),
            result: CheckpointResult {
                complete: true,
                version: RecordDigest::from_bytes([4; 32]),
                workspace_digest: "c".repeat(32),
                saved_changes: 2,
                issue: Some("capture-failed".into()),
            },
        },
        "invalid-checkpoint-result",
    );
    assert!(runtime.state().checkpoints["capture"].result.is_none());
    refuses(
        &mut runtime,
        Command::SubmitReview {
            checkpoint: "capture".into(),
            bundle: RecordDigest::from_bytes([5; 32]),
        },
        "checkpoint-incomplete",
    );
}
