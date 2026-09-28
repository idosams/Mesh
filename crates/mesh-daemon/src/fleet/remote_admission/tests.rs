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
