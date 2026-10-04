use super::*;
use mesh_store::RecordDigest;
use std::collections::VecDeque;

fn saved(pending: bool) -> WorkerProgressSave {
    WorkerProgressSave {
        version: RecordDigest::from_bytes([0x45; 32]),
        saved_changes: 0,
        complete: true,
        observation_pending: pending,
        issue: pending.then_some("fleet-progress-observation-pending"),
    }
}
fn step(
    owner: &mut ProgressOwner,
    now: Instant,
    cancelled: bool,
    replies: &mut VecDeque<WorkerProgressSave>,
    calls: &mut usize,
) {
    owner.advance_with(now, true, cancelled, || {
        *calls += 1;
        let reply = replies.pop_front().expect("unexpected save attempt");
        std::thread::Builder::new().spawn(move || Ok(reply))
    });
}
fn await_capture(owner: &ProgressOwner) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !owner.job.as_ref().unwrap().0.is_finished() {
        assert!(Instant::now() < deadline, "capture fixture did not finish");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn final_acknowledgment_waits_then_recovers_before_completion() {
    let mut owner = ProgressOwner::new(true);
    let now = Instant::now();
    let mut replies = VecDeque::from([saved(true), saved(false)]);
    let mut calls = 0;
    step(&mut owner, now, false, &mut replies, &mut calls);
    assert!(!owner.final_observed());
    await_capture(&owner);
    step(&mut owner, now, false, &mut replies, &mut calls);
    assert!(
        !owner.final_observed(),
        "pending acknowledgment ended the session"
    );
    assert_eq!(owner.observation().unwrap().state, "needs-attention");
    step(
        &mut owner,
        now + Duration::from_millis(999),
        false,
        &mut replies,
        &mut calls,
    );
    assert_eq!(calls, 1, "retry ignored backoff");
    step(
        &mut owner,
        now + Duration::from_secs(1),
        false,
        &mut replies,
        &mut calls,
    );
    assert_eq!(calls, 2);
    await_capture(&owner);
    step(
        &mut owner,
        now + Duration::from_secs(1),
        false,
        &mut replies,
        &mut calls,
    );
    assert!(owner.final_observed());
    assert_eq!(owner.observation().unwrap().state, "unchanged");
    assert_eq!(owner.observation().unwrap().issue, None);
}

#[test]
fn persistent_final_acknowledgment_failure_is_bounded_and_visible() {
    let mut owner = ProgressOwner::new(true);
    let now = Instant::now();
    let mut replies = VecDeque::from([saved(true), saved(true), saved(true)]);
    let mut calls = 0;
    for attempt in 0..3 {
        let at = now + Duration::from_secs(attempt);
        step(&mut owner, at, false, &mut replies, &mut calls);
        await_capture(&owner);
        step(&mut owner, at, false, &mut replies, &mut calls);
        assert_eq!(owner.final_observed(), attempt == 2);
    }
    step(
        &mut owner,
        now + Duration::from_secs(100),
        false,
        &mut replies,
        &mut calls,
    );
    assert_eq!(calls, 3);
    assert_eq!(owner.observation().unwrap().state, "needs-attention");
    assert_eq!(
        owner.observation().unwrap().version,
        Some(saved(true).version.to_string())
    );
}

#[test]
fn cancellation_during_final_backoff_prevents_another_save() {
    let mut owner = ProgressOwner::new(true);
    let now = Instant::now();
    let mut replies = VecDeque::from([saved(true)]);
    let mut calls = 0;
    step(&mut owner, now, false, &mut replies, &mut calls);
    await_capture(&owner);
    step(&mut owner, now, false, &mut replies, &mut calls);
    step(
        &mut owner,
        now + Duration::from_secs(100),
        true,
        &mut replies,
        &mut calls,
    );
    assert_eq!(calls, 1);
    assert!(!owner.final_observed());
}

#[test]
fn incomplete_native_capture_is_not_retried_as_an_acknowledgment() {
    let mut owner = ProgressOwner::new(true);
    let now = Instant::now();
    let mut incomplete = saved(false);
    incomplete.complete = false;
    incomplete.issue = Some("checkpoint-missing-files");
    let mut replies = VecDeque::from([incomplete]);
    let mut calls = 0;
    step(&mut owner, now, false, &mut replies, &mut calls);
    await_capture(&owner);
    step(&mut owner, now, false, &mut replies, &mut calls);
    step(
        &mut owner,
        now + Duration::from_secs(100),
        false,
        &mut replies,
        &mut calls,
    );
    assert!(owner.final_observed());
    assert_eq!(calls, 1);
    assert_eq!(owner.observation().unwrap().state, "needs-attention");
}
