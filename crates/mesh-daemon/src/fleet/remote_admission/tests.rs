use super::*;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Barrier,
};

static NEXT: AtomicU64 = AtomicU64::new(0);
const ALLOCATION: &str = "0123456789abcdef0123456789abcdef";
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "mesh-remote-admission-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn registry(&self, capacity: u64) -> RemoteAdmissionRegistry {
        RemoteAdmissionRegistry::new(
            FleetStore::open(self.0.join("worker.sqlite")).unwrap(),
            &"ab".repeat(32),
            &"cd".repeat(32),
            "objective",
            limits(capacity),
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn limits(concurrency: u64) -> Limits {
    Limits {
        lanes: 4,
        concurrency,
        depth: 1,
        retries: 1,
    }
}
fn work() -> RemoteWork {
    RemoteWork {
        lane: "lane".into(),
        run: "run".into(),
        provider: "codex".into(),
        goal: "Independent task".into(),
        assignment: RemoteAssignment {
            id: "assignment".into(),
            worker_key: "cd".repeat(32),
            input: RecordDigest::from_bytes([1; 32]),
            bundle: RecordDigest::from_bytes([2; 32]),
            lease_sequence: 1,
            lease_until_ms: 1000,
        },
    }
}

#[test]
fn identical_connections_get_one_reservation_and_restart_gets_only_receipt() {
    let fixture = Fixture::new();
    let registries = [fixture.registry(2), fixture.registry(2)];
    let barrier = Arc::new(Barrier::new(2));
    let handles = registries.map(|mut registry| {
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            registry.reserve(work(), ALLOCATION, 100).unwrap()
        })
    });
    let results = handles.map(|handle| handle.join().unwrap());
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, RemoteAdmissionOutcome::Reserved(_)))
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, RemoteAdmissionOutcome::Retained(_)))
            .count(),
        1
    );
    // Losing/dropping the original grant (including before any filesystem write) does not prove
    // nothing happened. Restart and an expired lease retain the claim and its slot.
    drop(results);
    let mut reopened = fixture.registry(2);
    let RemoteAdmissionOutcome::Retained(receipt) =
        reopened.reserve(work(), ALLOCATION, 5000).unwrap()
    else {
        panic!("restart must not manufacture a reservation")
    };
    assert_eq!(receipt.revision(), 1);
    assert_eq!(receipt.allocation(), ALLOCATION);
    assert!(receipt.work() == &work());
    assert_eq!(reopened.receipts().unwrap().len(), 1);
}

#[test]
fn changed_work_or_allocation_cannot_evade_assignment_uniqueness() {
    let fixture = Fixture::new();
    let mut registry = fixture.registry(4);
    registry.reserve(work(), ALLOCATION, 100).unwrap();
    let mut variants = Vec::new();
    let mut changed = work();
    changed.lane = "other".into();
    variants.push(changed);
    let mut changed = work();
    changed.run = "other".into();
    variants.push(changed);
    let mut changed = work();
    changed.provider = "claude".into();
    variants.push(changed);
    let mut changed = work();
    changed.goal = "Changed task".into();
    variants.push(changed);
    let mut changed = work();
    changed.assignment.input = RecordDigest::from_bytes([3; 32]);
    variants.push(changed);
    let mut changed = work();
    changed.assignment.bundle = RecordDigest::from_bytes([3; 32]);
    variants.push(changed);
    let mut changed = work();
    changed.assignment.worker_key = "ef".repeat(32);
    variants.push(changed);
    let mut changed = work();
    changed.assignment.lease_until_ms += 1;
    variants.push(changed);
    let mut changed = work();
    changed.assignment.lease_sequence += 1;
    variants.push(changed);
    for changed in variants {
        assert!(registry.reserve(changed, ALLOCATION, 100).is_err());
    }
    assert!(registry.reserve(work(), &"a".repeat(32), 100).is_err());
    assert_eq!(registry.receipts().unwrap().len(), 1);
    // A new assignment identity cannot reuse the same lane, run or native allocation either.
    for retained in 0..3 {
        let mut changed = work();
        changed.assignment.id = "other-assignment".into();
        if retained != 0 {
            changed.lane = "other-lane".into();
        }
        if retained != 1 {
            changed.run = "other-run".into();
        }
        let allocation = if retained == 2 {
            ALLOCATION.to_owned()
        } else {
            "b".repeat(32)
        };
        assert!(registry.reserve(changed, &allocation, 100).is_err());
    }
}

#[test]
fn concurrent_distinct_requests_cannot_overbook_last_slot_or_release_by_expiry() {
    let fixture = Fixture::new();
    let registries = [fixture.registry(1), fixture.registry(1)];
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = registries
        .into_iter()
        .enumerate()
        .map(|(index, mut registry)| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut work = work();
                work.assignment.id = format!("assignment-{index}");
                work.lane = format!("lane-{index}");
                work.run = format!("run-{index}");
                barrier.wait();
                registry.reserve(work, &format!("{index:032x}"), 100)
            })
        })
        .collect();
    let results: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Ok(RemoteAdmissionOutcome::Reserved(_))))
            .count(),
        1
    );
    assert_eq!(results.iter().filter(|r| r.is_err()).count(), 1);
    let mut reopened = fixture.registry(1);
    let mut later = work();
    later.assignment.lease_until_ms = 10_000;
    assert!(matches!(
        reopened.reserve(later, ALLOCATION, 5000),
        Err(Error::Refused("remote-admission-capacity"))
    ));
    assert_eq!(reopened.receipts().unwrap().len(), 1);
}

#[test]
fn invalid_admission_and_changed_configuration_preserve_history() {
    let fixture = Fixture::new();
    let mut registry = fixture.registry(2);
    for (allocation, clock) in [("../elsewhere", 100), (ALLOCATION, 0), (ALLOCATION, 1000)] {
        assert!(registry.reserve(work(), allocation, clock).is_err());
    }
    assert!(registry.receipts().unwrap().is_empty());
    registry.reserve(work(), ALLOCATION, 100).unwrap();
    for (worker, limits) in [("ef".repeat(32), limits(2)), ("cd".repeat(32), limits(3))] {
        assert!(RemoteAdmissionRegistry::new(
            FleetStore::open(fixture.0.join("worker.sqlite")).unwrap(),
            &"ab".repeat(32),
            &worker,
            "objective",
            limits
        )
        .is_err());
    }
    assert_eq!(fixture.registry(2).receipts().unwrap().len(), 1);
}

#[test]
fn canonical_receipts_refuse_unknown_schema_fields_and_changed_stream_context() {
    let fixture = Fixture::new();
    let registry = fixture.registry(2);
    let payload = registry.encode(&work(), ALLOCATION);
    let event = FleetEvent {
        stream: registry.stream.clone(),
        revision: 1,
        request: "assignment".into(),
        payload: payload.clone(),
    };
    assert!(registry.decode(&event).is_ok());
    for payload in [
        payload.replace("admission/v1", "admission/v2"),
        payload.replacen('{', "{\"unknown\":0,", 1),
        payload.replace("\"objective\":\"objective\"", "\"objective\":\"other\""),
        payload.replace("\"concurrency\":2", "\"concurrency\":3"),
        payload.replace("\"lease_sequence\":1", "\"lease_sequence\":1.0"),
    ] {
        assert!(registry
            .decode(&FleetEvent {
                payload,
                ..event.clone()
            })
            .is_err());
    }
    assert!(registry
        .decode(&FleetEvent {
            request: "other".into(),
            ..event.clone()
        })
        .is_err());
    assert!(registry
        .decode(&FleetEvent {
            stream: "other".into(),
            ..event
        })
        .is_err());
}

#[test]
fn identical_work_in_different_objectives_keeps_distinct_receipt_scope() {
    let fixture = Fixture::new();
    let mut first = fixture.registry(2);
    first.reserve(work(), ALLOCATION, 100).unwrap();
    let mut other = RemoteAdmissionRegistry::new(
        FleetStore::open(fixture.0.join("worker.sqlite")).unwrap(),
        &"ab".repeat(32),
        &"cd".repeat(32),
        "another-objective",
        limits(2),
    )
    .unwrap();
    other.reserve(work(), ALLOCATION, 100).unwrap();
    let first = first.receipts().unwrap().remove(0);
    let other = other.receipts().unwrap().remove(0);
    assert!(first != other);
    assert_eq!(first.objective(), "objective");
    assert_eq!(other.objective(), "another-objective");
    assert_eq!(first.coordinator(), "ab".repeat(32));
    assert!(first.work() == other.work());
}

#[test]
fn worker_renewal_is_durable_exact_replay_and_never_another_reservation() {
    let fixture = Fixture::new();
    let mut registry = fixture.registry(1);
    let RemoteAdmissionOutcome::Reserved(reservation) =
        registry.reserve(work(), ALLOCATION, 100).unwrap()
    else {
        panic!("original reservation");
    };
    let receipt = reservation.receipt().clone();
    let renewed = registry.renew_lease(&receipt, 1, 2000, 500, 2000).unwrap();
    assert_eq!(renewed.sequence, 2);
    assert_eq!(renewed.until_ms, 2000);
    drop(registry);
    let mut reopened = fixture.registry(1);
    assert_eq!(reopened.effective_lease(&receipt).unwrap(), renewed);
    assert_eq!(
        reopened.renew_lease(&receipt, 1, 2000, 9000, 1).unwrap(),
        renewed
    );
    assert!(matches!(
        reopened.reserve(work(), ALLOCATION, 9000).unwrap(),
        RemoteAdmissionOutcome::Retained(_)
    ));
    assert_eq!(reopened.receipts().unwrap().len(), 1);
    assert_eq!(receipt.work().assignment.lease_until_ms, 1000);
    assert!(reopened.renew_lease(&receipt, 2, 3000, 2000, 4000).is_err());
    assert_eq!(reopened.effective_lease(&receipt).unwrap(), renewed);
}

#[test]
fn worker_renewal_refuses_conflicting_scope_sequence_clock_and_cap() {
    let fixture = Fixture::new();
    let mut registry = fixture.registry(1);
    let RemoteAdmissionOutcome::Reserved(reservation) =
        registry.reserve(work(), ALLOCATION, 100).unwrap()
    else {
        panic!("original reservation");
    };
    let receipt = reservation.receipt().clone();
    let original = registry.effective_lease(&receipt).unwrap();
    for (sequence, until, now, cap) in [
        (0, 2000, 500, 2000),
        (2, 2000, 500, 2000),
        (1, 1000, 500, 2000),
        (1, 2000, 0, 2000),
        (1, 2000, 1000, 2000),
        (1, 2000, 500, 1000),
    ] {
        assert!(registry
            .renew_lease(&receipt, sequence, until, now, cap)
            .is_err());
        assert_eq!(registry.effective_lease(&receipt).unwrap(), original);
    }
    let mut wrong = receipt.clone();
    wrong.objective = "another-objective".into();
    assert!(registry.renew_lease(&wrong, 1, 2000, 500, 2000).is_err());
    let renewed = registry.renew_lease(&receipt, 1, 2000, 500, 2000).unwrap();
    assert!(registry.renew_lease(&receipt, 1, 2100, 600, 3000).is_err());
    assert!(registry.renew_lease(&receipt, 2, 3000, 400, 4000).is_err());
    assert_eq!(registry.effective_lease(&receipt).unwrap(), renewed);
    let next = registry.renew_lease(&receipt, 2, 3000, 1000, 3000).unwrap();
    assert_eq!(next.sequence, 3);
    assert_eq!(
        registry.renew_lease(&receipt, 1, 2000, 9000, 1).unwrap(),
        renewed
    );
    assert_eq!(registry.effective_lease(&receipt).unwrap(), next);
}

#[test]
fn competing_worker_renewals_commit_one_deadline() {
    let fixture = Fixture::new();
    let mut registry = fixture.registry(1);
    let RemoteAdmissionOutcome::Reserved(reservation) =
        registry.reserve(work(), ALLOCATION, 100).unwrap()
    else {
        panic!("reservation");
    };
    let receipt = reservation.receipt().clone();
    let barrier = Arc::new(Barrier::new(2));
    let handles =
        [(fixture.registry(1), 2000), (fixture.registry(1), 2100)].map(|(mut registry, until)| {
            let barrier = barrier.clone();
            let receipt = receipt.clone();
            std::thread::spawn(move || {
                barrier.wait();
                registry.renew_lease(&receipt, 1, until, 500, 3000)
            })
        });
    let results = handles.map(|handle| handle.join().unwrap());
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let current = registry.effective_lease(&receipt).unwrap();
    assert_eq!(current.sequence, 2);
    assert_eq!(
        &current,
        results.iter().find_map(|r| r.as_ref().ok()).unwrap()
    );
    assert_eq!(registry.receipts().unwrap().len(), 1);
}

#[test]
fn renewal_history_crosses_store_page_boundary_and_refuses_corrupt_tail() {
    let fixture = Fixture::new();
    let mut registry = fixture.registry(1);
    let RemoteAdmissionOutcome::Reserved(reservation) =
        registry.reserve(work(), ALLOCATION, 100).unwrap()
    else {
        panic!("reservation");
    };
    let receipt = reservation.receipt().clone();
    for sequence in 1..=257 {
        registry
            .renew_lease(&receipt, sequence, 1000 + sequence, 500, 3000)
            .unwrap();
    }
    let current = fixture.registry(1).effective_lease(&receipt).unwrap();
    assert_eq!(current.sequence, 258);
    assert_eq!(current.until_ms, 1257);
    // Reproduce a noncanonical tail through the raw store, never through the validated API.
    let event = registry
        .store
        .events(&registry.stream, 0, 1)
        .unwrap()
        .remove(0);
    let identity = Json::object([
        ("stream", Json::text(&event.stream)),
        ("admission", Json::text(&event.payload)),
        ("revision", Json::Number(event.revision)),
    ])
    .encode();
    let stream = format!(
        "remote-leases-{}",
        Blake3::digest_bytes(identity.as_bytes())
    );
    registry
        .store
        .append_with_outcome(&stream, 257, "258", "{}")
        .unwrap();
    assert!(registry.effective_lease(&receipt).is_err());
    assert!(registry
        .renew_lease(&receipt, 258, 2000, 600, 3000)
        .is_err());
    assert_eq!(registry.receipts().unwrap().len(), 1);
}
