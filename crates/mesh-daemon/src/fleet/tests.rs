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
        starting_version: None,
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

#[test]
fn durable_launch_claim_is_not_regranted_after_restart_or_to_another_host() {
    let fixture = Fixture::new();
    let mut runtime = fixture.runtime();
    send(&mut runtime, start());
    register_lane(&mut runtime, "worker", None);
    send(&mut runtime, dispatch("worker", "run"));
    send(
        &mut runtime,
        Command::ClaimLaunch {
            lane: "worker".into(),
            run: "run".into(),
            owner: "host-one".into(),
        },
    );
    drop(runtime);
    let mut runtime = fixture.runtime();
    assert_eq!(
        runtime.state().lanes["worker"].runs[0]
            .launch_owner
            .as_deref(),
        Some("host-one")
    );
    for owner in ["host-one", "host-two"] {
        refuses(
            &mut runtime,
            Command::ClaimLaunch {
                lane: "worker".into(),
                run: "run".into(),
                owner: owner.into(),
            },
            "launch-needs-reconciliation",
        );
    }
    assert!(runtime.state().lanes["worker"].runs[0]
        .state
        .occupies_slot());
}

#[test]
fn local_starting_version_survives_replay_and_cannot_be_rebound_after_dispatch() {
    let fixture = Fixture::new();
    let mut runtime = fixture.runtime();
    send(&mut runtime, start());
    send(&mut runtime, lane("worker", None));
    let mut original = binding("worker");
    original.starting_version = Some(RecordDigest::from_bytes([2; 32]));
    send(
        &mut runtime,
        Command::BindWorkspace {
            lane: "worker".into(),
            binding: original.clone(),
        },
    );
    send(&mut runtime, dispatch("worker", "run"));
    let state = runtime.state().clone();
    drop(runtime);
    let mut restored = fixture.runtime();
    assert_eq!(restored.state(), &state);
    let mut replacement = original.clone();
    replacement.starting_version = Some(RecordDigest::from_bytes([3; 32]));
    refuses(
        &mut restored,
        Command::BindWorkspace {
            lane: "worker".into(),
            binding: replacement,
        },
        "workspace-already-bound",
    );
    assert_eq!(
        restored.state().lanes["worker"].workspace.as_ref(),
        Some(&original)
    );
}

#[test]
fn legacy_starting_version_remains_absent_after_replay_and_refuses_backfill() {
    let fixture = Fixture::new();
    let mut runtime = fixture.runtime();
    send(&mut runtime, start());
    register_lane(&mut runtime, "worker", None);
    let state = runtime.state().clone();
    drop(runtime);
    let mut restored = fixture.runtime();
    assert_eq!(restored.state(), &state);
    let original = restored.state().lanes["worker"].workspace.as_ref().unwrap();
    assert_eq!(original.starting_version(), None);
    let mut inferred = original.clone();
    inferred.starting_version = Some(inferred.source_version);
    refuses(
        &mut restored,
        Command::BindWorkspace {
            lane: "worker".into(),
            binding: inferred,
        },
        "workspace-already-bound",
    );
    assert_eq!(restored.state(), &state);
    drop(restored);
    assert_eq!(fixture.runtime().state(), &state);
}

fn reviewed_checkpoint(runtime: &mut Runtime) -> ReviewChangeRequest {
    send(runtime, start());
    register_lane(runtime, "worker", None);
    send(runtime, dispatch("worker", "run"));
    send(
        runtime,
        Command::BeginCheckpoint {
            id: "capture".into(),
            lane: "worker".into(),
            origin: AgentOrigin {
                actor: "actor".into(),
                session: "session".into(),
                run: "run".into(),
                generation: "a".repeat(32),
            },
            input_digest: "b".repeat(32),
        },
    );
    let version = RecordDigest::from_bytes([4; 32]);
    send(
        runtime,
        Command::FinishCheckpoint {
            id: "capture".into(),
            result: CheckpointResult {
                complete: true,
                version,
                workspace_digest: "c".repeat(32),
                saved_changes: 1,
                issue: None,
            },
        },
    );
    let bundle = RecordDigest::from_bytes([5; 32]);
    send(
        runtime,
        Command::SubmitReview {
            checkpoint: "capture".into(),
            bundle,
        },
    );
    ReviewChangeRequest {
        id: "feedback".into(),
        lane: "worker".into(),
        checkpoint: "capture".into(),
        version,
        bundle,
        message: "Please preserve the existing opening paragraph.\nAdd the requested example."
            .into(),
    }
}

#[test]
fn review_changes_survive_restart_retry_and_cancel_without_scheduling_or_changing_saved_work() {
    let fixture = Fixture::new();
    let mut runtime = fixture.runtime();
    let request = reviewed_checkpoint(&mut runtime);
    send(&mut runtime, Command::Cancel);
    let before = runtime.state().clone();
    let command = Command::RequestReviewChanges(request.clone());
    let event = runtime.record("feedback", command.clone()).unwrap();
    assert_eq!(runtime.state().lanes, before.lanes);
    assert_eq!(runtime.state().checkpoints, before.checkpoints);
    assert!(runtime.state().cancelled);
    drop(runtime);
    let mut runtime = fixture.runtime();
    assert_eq!(runtime.state().review_change_requests["feedback"], request);
    assert_eq!(runtime.record("feedback", command).unwrap(), event);
    let mut changed = request;
    changed.message = "Different request".into();
    assert!(runtime
        .record("feedback", Command::RequestReviewChanges(changed))
        .is_err());
    assert_eq!(runtime.state().revision, event.revision);
}

#[test]
fn review_changes_refuse_wrong_selection_and_unbounded_or_unsafe_feedback() {
    let fixture = Fixture::new();
    let mut runtime = fixture.runtime();
    let original = reviewed_checkpoint(&mut runtime);
    for field in ["lane", "checkpoint", "version", "bundle"] {
        let mut request = original.clone();
        match field {
            "lane" => request.lane = "other".into(),
            "checkpoint" => request.checkpoint = "other".into(),
            "version" => request.version = RecordDigest::from_bytes([9; 32]),
            _ => request.bundle = RecordDigest::from_bytes([9; 32]),
        }
        let before = runtime.state().clone();
        assert!(runtime
            .record("invalid", Command::RequestReviewChanges(request))
            .is_err());
        assert_eq!(runtime.state(), &before);
    }
    for message in [
        "".into(),
        "  \n".into(),
        "a".repeat(8193),
        "😀".repeat(2049),
        "carriage\rreturn".into(),
        "hidden\u{0085}control".into(),
        "hidden\0text".into(),
        "misleading\u{202e}text".into(),
    ] {
        let mut request = original.clone();
        request.message = message;
        refuses(
            &mut runtime,
            Command::RequestReviewChanges(request),
            "invalid-review-change-message",
        );
    }
    for n in 0..32 {
        let mut request = original.clone();
        request.id = format!("request-{n}");
        send(&mut runtime, Command::RequestReviewChanges(request));
    }
    refuses(
        &mut runtime,
        Command::RequestReviewChanges(original),
        "review-change-request-limit",
    );
    assert_eq!(fixture.runtime().state().review_change_requests.len(), 32);
}

#[test]
fn review_change_wire_is_closed_and_keeps_existing_command_encoding() {
    let fixture = Fixture::new();
    let mut runtime = fixture.runtime();
    let command = Command::RequestReviewChanges(reviewed_checkpoint(&mut runtime));
    let encoded = wire::encode(&command);
    assert_eq!(wire::decode(&encoded).unwrap(), command);
    assert!(
        wire::decode(&encoded.replace("\"message\":", "\"unexpected\":true,\"message\":")).is_err()
    );
    assert!(wire::decode(&encoded.replace("\"schema\":1", "\"schema\":2")).is_err());
    assert_eq!(
        wire::encode(&Command::Cancel),
        "{\"schema\":1,\"kind\":\"cancel\",\"fields\":{}}"
    );
}

#[test]
fn review_change_utf8_boundary_is_preserved_exactly_across_replay() {
    let fixture = Fixture::new();
    let mut runtime = fixture.runtime();
    let mut request = reviewed_checkpoint(&mut runtime);
    request.message = "😀".repeat(2048);
    assert_eq!(request.message.len(), 8192);
    let event = runtime
        .record(
            "unicode-feedback",
            Command::RequestReviewChanges(request.clone()),
        )
        .unwrap();
    drop(runtime);
    let mut restored = fixture.runtime();
    assert_eq!(
        restored.state().review_change_requests[&request.id],
        request
    );
    assert_eq!(
        restored
            .record(
                "unicode-feedback",
                Command::RequestReviewChanges(request.clone())
            )
            .unwrap(),
        event
    );
    request.message.push('!');
    assert!(restored
        .record("oversized-feedback", Command::RequestReviewChanges(request))
        .is_err());
    assert_eq!(restored.state().revision, event.revision);
}

fn response_checkpoint(runtime: &mut Runtime, n: u8) -> Command {
    let checkpoint = format!("revision-{n}");
    let origin = runtime.state().checkpoints["capture"].origin.clone();
    send(
        runtime,
        Command::BeginCheckpoint {
            id: checkpoint.clone(),
            lane: "worker".into(),
            origin: origin.clone(),
            input_digest: "b".repeat(32),
        },
    );
    send(
        runtime,
        Command::FinishCheckpoint {
            id: checkpoint.clone(),
            result: CheckpointResult {
                complete: true,
                version: RecordDigest::from_bytes([10 + n; 32]),
                workspace_digest: "c".repeat(32),
                saved_changes: 1,
                issue: None,
            },
        },
    );
    send(
        runtime,
        Command::SubmitReview {
            checkpoint: checkpoint.clone(),
            bundle: RecordDigest::from_bytes([30 + n; 32]),
        },
    );
    Command::ProposeReviewChangeResult {
        request: "feedback".into(),
        checkpoint,
        origin,
    }
}

#[test]
fn review_change_proposals_replay_retry_and_never_resolve_or_mutate_the_original_review() {
    let fixture = Fixture::new();
    let mut runtime = fixture.runtime();
    let feedback = reviewed_checkpoint(&mut runtime);
    send(
        &mut runtime,
        Command::RequestReviewChanges(feedback.clone()),
    );
    let command = response_checkpoint(&mut runtime, 0);
    let before = runtime.state().clone();
    let receipt = runtime.record("response", command.clone()).unwrap();
    assert_eq!(runtime.state().lanes, before.lanes);
    assert_eq!(runtime.state().checkpoints, before.checkpoints);
    assert_eq!(
        runtime.state().review_change_requests,
        before.review_change_requests
    );
    assert_eq!(
        runtime.state().review_change_responses["feedback"][0].checkpoint,
        "revision-0"
    );
    let encoded = wire::encode(&command);
    assert_eq!(wire::decode(&encoded).unwrap(), command);
    assert!(
        wire::decode(&encoded.replace("\"actor\":", "\"unexpected\":true,\"actor\":")).is_err()
    );
    drop(runtime);
    let mut runtime = fixture.runtime();
    assert_eq!(
        runtime.record("response", command.clone()).unwrap(),
        receipt
    );
    refuses(&mut runtime, command, "review-change-response-exists");
    assert_eq!(runtime.state().review_change_requests["feedback"], feedback);
    for n in 1..8 {
        let command = response_checkpoint(&mut runtime, n);
        send(&mut runtime, command);
    }
    let ninth = response_checkpoint(&mut runtime, 8);
    refuses(&mut runtime, ninth, "review-change-response-limit");
    assert_eq!(
        fixture.runtime().state().review_change_responses["feedback"].len(),
        8
    );
}

#[test]
fn review_change_proposals_refuse_original_checkpoint_wrong_session_and_cancelled_runs() {
    let fixture = Fixture::new();
    let mut runtime = fixture.runtime();
    let feedback = reviewed_checkpoint(&mut runtime);
    send(&mut runtime, Command::RequestReviewChanges(feedback));
    let origin = runtime.state().checkpoints["capture"].origin.clone();
    refuses(
        &mut runtime,
        Command::ProposeReviewChangeResult {
            request: "feedback".into(),
            checkpoint: "capture".into(),
            origin: origin.clone(),
        },
        "review-change-response-unchanged",
    );
    send(
        &mut runtime,
        Command::BeginCheckpoint {
            id: "unfinished".into(),
            lane: "worker".into(),
            origin: origin.clone(),
            input_digest: "b".repeat(32),
        },
    );
    refuses(
        &mut runtime,
        Command::ProposeReviewChangeResult {
            request: "feedback".into(),
            checkpoint: "unfinished".into(),
            origin,
        },
        "checkpoint-incomplete",
    );
    let command = response_checkpoint(&mut runtime, 0);
    let mut wrong = command.clone();
    if let Command::ProposeReviewChangeResult { origin, .. } = &mut wrong {
        origin.session = "substituted".into();
    }
    refuses(&mut runtime, wrong, "review-change-response-not-in-session");
    send(&mut runtime, Command::Cancel);
    refuses(&mut runtime, command, "objective-cancelled");
    assert!(runtime.state().review_change_responses.is_empty());
}

#[test]
fn review_decisions_are_reversible_revision_bound_and_do_not_approve_or_change_work() {
    let fixture = Fixture::new();
    let mut runtime = fixture.runtime();
    let feedback = reviewed_checkpoint(&mut runtime);
    send(&mut runtime, Command::RequestReviewChanges(feedback));
    let proposal = response_checkpoint(&mut runtime, 0);
    runtime.record("proposal", proposal.clone()).unwrap();
    let before = runtime.state().clone();
    let close = Command::SetReviewChangeDecision {
        request: "feedback".into(),
        expected_revision: 0,
        checkpoint: Some("revision-0".into()),
    };
    let receipt = runtime.record("decision", close.clone()).unwrap();
    assert_eq!(runtime.state().lanes, before.lanes);
    assert_eq!(runtime.state().checkpoints, before.checkpoints);
    assert_eq!(
        runtime.state().review_change_requests,
        before.review_change_requests
    );
    assert_eq!(
        runtime.state().review_change_responses,
        before.review_change_responses
    );
    let next_proposal = response_checkpoint(&mut runtime, 1);
    refuses(
        &mut runtime,
        next_proposal,
        "review-change-request-addressed",
    );
    assert!(runtime.record("proposal", proposal).is_ok());
    send(
        &mut runtime,
        Command::SetReviewChangeDecision {
            request: "feedback".into(),
            expected_revision: 1,
            checkpoint: None,
        },
    );
    assert_eq!(
        runtime.state().review_change_decisions["feedback"],
        ReviewChangeDecision {
            revision: 2,
            checkpoint: None
        }
    );
    refuses(&mut runtime, close.clone(), "review-change-decision-stale");
    drop(runtime);
    let mut runtime = fixture.runtime();
    assert_eq!(runtime.record("decision", close.clone()).unwrap(), receipt);
    assert_eq!(
        runtime.state().review_change_decisions["feedback"].revision,
        2
    );
    assert_eq!(wire::decode(&wire::encode(&close)).unwrap(), close);
    refuses(
        &mut runtime,
        Command::SetReviewChangeDecision {
            request: "feedback".into(),
            expected_revision: 2,
            checkpoint: Some("unknown".into()),
        },
        "review-change-proposal-missing",
    );
    send(&mut runtime, Command::Cancel); // Work decisions remain usable after agent cancellation.
    for expected_revision in 2..64 {
        send(
            &mut runtime,
            Command::SetReviewChangeDecision {
                request: "feedback".into(),
                expected_revision,
                checkpoint: (expected_revision % 2 == 0).then(|| "revision-0".into()),
            },
        );
    }
    refuses(
        &mut runtime,
        Command::SetReviewChangeDecision {
            request: "feedback".into(),
            expected_revision: 64,
            checkpoint: Some("revision-0".into()),
        },
        "review-change-decision-limit",
    );
}

fn deletion_intent(path: &str) -> Command {
    Command::BeginFileDeletion {
        id: "delete-one".into(),
        lane: "worker".into(),
        origin: AgentOrigin {
            actor: "actor".into(),
            session: "session".into(),
            run: "run".into(),
            generation: "a".repeat(32),
        },
        input_digest: "b".repeat(32),
        path: path.into(),
        version: RecordDigest::from_bytes([2; 32]),
    }
}
#[test]
fn file_deletion_three_stage_receipts_survive_restart_and_reconcile_after_cancel() {
    let fixture = Fixture::new();
    let mut runtime = fixture.runtime();
    send(&mut runtime, start());
    register_lane(&mut runtime, "worker", None);
    send(&mut runtime, dispatch("worker", "run"));
    let begin = deletion_intent("docs/old.txt");
    let accepted = runtime.record("delete-intent", begin.clone()).unwrap();
    drop(runtime);
    let mut runtime = fixture.runtime();
    assert_eq!(runtime.record("delete-intent", begin).unwrap(), accepted);
    let pending = &runtime.state().file_deletions["delete-one"];
    assert_eq!(pending.path, "docs/old.txt");
    assert!(pending.operation.is_none());
    assert!(pending.result.is_none());
    let operation = RecordDigest::from_bytes([3; 32]);
    let result = FileDeletionResult {
        operation,
        workspace_digest: "c".repeat(32),
        settled: false,
    };
    refuses(
        &mut runtime,
        Command::FinishFileDeletion {
            id: "delete-one".into(),
            result: result.clone(),
        },
        "file-deletion-operation-mismatch",
    );
    let prepared = Command::PrepareFileDeletion {
        id: "delete-one".into(),
        operation,
    };
    let receipt = runtime.record("delete-prepared", prepared.clone()).unwrap();
    drop(runtime);
    let mut runtime = fixture.runtime();
    assert_eq!(
        runtime.record("delete-prepared", prepared.clone()).unwrap(),
        receipt
    );
    refuses(&mut runtime, prepared, "file-deletion-already-prepared");
    let mut wrong = result.clone();
    wrong.operation = RecordDigest::from_bytes([4; 32]);
    refuses(
        &mut runtime,
        Command::FinishFileDeletion {
            id: "delete-one".into(),
            result: wrong,
        },
        "file-deletion-operation-mismatch",
    );
    send(&mut runtime, Command::Cancel);
    let lane = runtime.state().lanes["worker"].clone();
    let finish = Command::FinishFileDeletion {
        id: "delete-one".into(),
        result: result.clone(),
    };
    let acknowledged = runtime.record("delete-finished", finish.clone()).unwrap();
    assert_eq!(
        runtime.record("delete-finished", finish.clone()).unwrap(),
        acknowledged
    );
    refuses(&mut runtime, finish, "file-deletion-already-finished");
    assert_eq!(runtime.state().lanes["worker"], lane);
    assert!(runtime.state().checkpoints.is_empty());
    drop(runtime);
    let runtime = fixture.runtime();
    assert_eq!(
        runtime.state().file_deletions["delete-one"].result,
        Some(result)
    );
    assert!(runtime.state().cancelled);
}
#[test]
fn deletion_intents_reject_bad_paths_stale_runs_replaced_inputs_and_late_preparation() {
    let fixture = Fixture::new();
    let mut runtime = fixture.runtime();
    send(&mut runtime, start());
    register_lane(&mut runtime, "worker", None);
    send(&mut runtime, dispatch("worker", "run"));
    for path in [
        "",
        "/absolute",
        "../outside",
        "folder/../old",
        "folder//old",
        "./old",
        "old\nfile",
        "old\u{202e}file",
    ] {
        refuses(&mut runtime, deletion_intent(path), "invalid-deletion-path");
    }
    let mut stale = deletion_intent("old");
    if let Command::BeginFileDeletion { origin, .. } = &mut stale {
        origin.run = "replaced".into();
    }
    assert!(runtime.record("stale-delete", stale).is_err());
    let accepted = runtime
        .record("delete-intent", deletion_intent("old"))
        .unwrap();
    assert!(runtime
        .record("delete-intent", deletion_intent("other"))
        .is_err());
    assert_eq!(runtime.state().revision, accepted.revision);
    send(&mut runtime, Command::Cancel);
    refuses(
        &mut runtime,
        Command::PrepareFileDeletion {
            id: "delete-one".into(),
            operation: RecordDigest::from_bytes([3; 32]),
        },
        "objective-cancelled",
    );
    assert!(runtime.state().file_deletions["delete-one"]
        .operation
        .is_none());
}
#[test]
fn deletion_command_encoding_is_additive_closed_and_roundtrips_exactly() {
    let commands = [
        deletion_intent("docs/old.txt"),
        Command::PrepareFileDeletion {
            id: "delete-one".into(),
            operation: RecordDigest::from_bytes([3; 32]),
        },
        Command::FinishFileDeletion {
            id: "delete-one".into(),
            result: FileDeletionResult {
                operation: RecordDigest::from_bytes([3; 32]),
                workspace_digest: "c".repeat(32),
                settled: true,
            },
        },
    ];
    for command in commands {
        let encoded = wire::encode(&command);
        assert_eq!(wire::decode(&encoded).unwrap(), command);
        assert!(wire::decode(&encoded.replacen("\"id\":", "\"unknown\":", 1)).is_err());
        assert!(
            wire::decode(&encoded.replacen("\"id\":", "\"authority\":true,\"id\":", 1)).is_err()
        );
    }
    let legacy = start();
    assert_eq!(wire::decode(&wire::encode(&legacy)).unwrap(), legacy);
    let fixture = Fixture::new();
    let mut runtime = fixture.runtime();
    send(&mut runtime, legacy);
    drop(runtime);
    assert!(fixture.runtime().state().file_deletions.is_empty());
}

fn remote_assignment() -> RemoteAssignment {
    RemoteAssignment {
        id: "assignment-one".into(),
        worker_key: "ab".repeat(32),
        input: RecordDigest::from_bytes([1; 32]),
        bundle: RecordDigest::from_bytes([9; 32]),
        lease_sequence: 1,
        lease_until_ms: 1000,
    }
}
fn remote_claim(assignment: RemoteAssignment) -> Command {
    Command::ClaimRemoteLaunch {
        lane: "worker".into(),
        run: "run".into(),
        assignment,
    }
}
fn remote_advance(sequence: u64, until: u64) -> Command {
    Command::AdvanceRemoteLease {
        lane: "worker".into(),
        run: "run".into(),
        assignment: "assignment-one".into(),
        worker_key: "ab".repeat(32),
        expected_sequence: sequence,
        lease_until_ms: until,
    }
}
#[test]
fn remote_assignment_and_exact_lease_ack_survive_restart_without_relaunch() {
    let fixture = Fixture::new();
    let mut runtime = fixture.runtime();
    send(&mut runtime, start());
    register_lane(&mut runtime, "worker", None);
    send(&mut runtime, dispatch("worker", "run"));
    let claim = send(&mut runtime, remote_claim(remote_assignment()));
    send(
        &mut runtime,
        observe("worker", "run", RunState::Reconciling),
    );
    let renewal = send(&mut runtime, remote_advance(1, 2000));
    drop(runtime);
    let mut runtime = fixture.runtime();
    assert_eq!(
        runtime
            .submit(
                claim.revision - 1,
                &claim.request,
                remote_claim(remote_assignment())
            )
            .unwrap(),
        claim
    );
    assert_eq!(
        runtime
            .submit(
                renewal.revision - 1,
                &renewal.request,
                remote_advance(1, 2000)
            )
            .unwrap(),
        renewal
    );
    let run = &runtime.state().lanes["worker"].runs[0];
    assert_eq!(run.state, RunState::Reconciling);
    assert!(run.state.occupies_slot());
    let assignment = run.remote.as_ref().unwrap();
    assert_eq!(assignment.lease_sequence, 2);
    assert_eq!(assignment.lease_until_ms, 2000);
    assert_eq!(assignment.bundle, remote_assignment().bundle);
    refuses(
        &mut runtime,
        Command::ClaimLaunch {
            lane: "worker".into(),
            run: "run".into(),
            owner: "local-host".into(),
        },
        "launch-needs-reconciliation",
    );
    refuses(
        &mut runtime,
        remote_claim(remote_assignment()),
        "remote-assignment-exists",
    );
    let mut other = remote_assignment();
    other.id = "replacement".into();
    refuses(
        &mut runtime,
        remote_claim(other),
        "launch-needs-reconciliation",
    );
    refuses(
        &mut runtime,
        dispatch("worker", "second-run"),
        "run-already-active",
    );
    refuses(&mut runtime, remote_advance(1, 3000), "remote-lease-stale");
    refuses(&mut runtime, remote_advance(2, 2000), "remote-lease-stale");
    send(&mut runtime, remote_advance(2, 3000));
    assert_eq!(runtime.state().lanes["worker"].runs.len(), 1);
}
#[test]
fn remote_assignment_refuses_wrong_inputs_peers_and_cancelled_or_finished_runs() {
    let fixture = Fixture::new();
    let mut runtime = fixture.runtime();
    send(&mut runtime, start());
    register_lane(&mut runtime, "worker", None);
    send(&mut runtime, dispatch("worker", "run"));
    let mut wrong = remote_assignment();
    wrong.input = RecordDigest::from_bytes([2; 32]);
    refuses(&mut runtime, remote_claim(wrong), "remote-input-mismatch");
    for kind in 0..4 {
        let mut invalid = remote_assignment();
        match kind {
            0 => invalid.worker_key = "AB".repeat(32),
            1 => invalid.lease_sequence = 2,
            2 => invalid.lease_until_ms = 0,
            _ => invalid.worker_key = "unknown".into(),
        }
        refuses(
            &mut runtime,
            remote_claim(invalid),
            "remote-assignment-invalid",
        );
    }
    send(&mut runtime, remote_claim(remote_assignment()));
    let mut wrong_peer = remote_advance(1, 2000);
    if let Command::AdvanceRemoteLease { worker_key, .. } = &mut wrong_peer {
        *worker_key = "cd".repeat(32);
    }
    refuses(&mut runtime, wrong_peer, "remote-assignment-mismatch");
    let mut wrong_id = remote_advance(1, 2000);
    if let Command::AdvanceRemoteLease { assignment, .. } = &mut wrong_id {
        *assignment = "other".into();
    }
    refuses(&mut runtime, wrong_id, "remote-assignment-mismatch");
    send(&mut runtime, observe("worker", "run", RunState::Running));
    send(&mut runtime, observe("worker", "run", RunState::Succeeded));
    refuses(
        &mut runtime,
        remote_advance(1, 2000),
        "remote-run-not-active",
    );
    send(&mut runtime, Command::Cancel);
    refuses(&mut runtime, remote_advance(1, 2000), "objective-cancelled");
}
#[test]
fn remote_assignment_cannot_replace_local_claim_or_reuse_another_lanes_identity() {
    let fixture = Fixture::new();
    let mut runtime = fixture.runtime();
    send(&mut runtime, start());
    register_lane(&mut runtime, "worker", None);
    register_lane(&mut runtime, "other", None);
    send(&mut runtime, dispatch("worker", "run"));
    send(&mut runtime, dispatch("other", "other-run"));
    send(&mut runtime, remote_claim(remote_assignment()));
    refuses(
        &mut runtime,
        Command::ClaimRemoteLaunch {
            lane: "other".into(),
            run: "other-run".into(),
            assignment: remote_assignment(),
        },
        "remote-assignment-exists",
    );
    send(
        &mut runtime,
        Command::ClaimLaunch {
            lane: "other".into(),
            run: "other-run".into(),
            owner: "local".into(),
        },
    );
    let mut other = remote_assignment();
    other.id = "other-assignment".into();
    refuses(
        &mut runtime,
        Command::ClaimRemoteLaunch {
            lane: "other".into(),
            run: "other-run".into(),
            assignment: other,
        },
        "launch-needs-reconciliation",
    );
    assert!(runtime.state().lanes["other"].runs[0].remote.is_none());
}
