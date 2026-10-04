//! Progress navigation is retained separately from review authority and content.
#![cfg(unix)]
use mesh_daemon::ipc::Json;
use mesh_daemon::project_attachment::{AttachmentStorage, ProgressPinState};
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt as _};
use std::path::PathBuf;
struct Fixture(PathBuf);
impl Fixture {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("mesh-progress-pins-{name}-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        Self(root)
    }
    fn open(&self) -> AttachmentStorage {
        AttachmentStorage::open(&self.0).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn pin(key: u64) -> Json {
    Json::object([
        ("key", Json::text(key.to_string())),
        ("objective", Json::text(format!("fleet-{}", "a".repeat(64)))),
        ("lane", Json::text(format!("lane-{}", "b".repeat(64)))),
        ("version", Json::text(format!("{key:064x}"))),
        ("source", Json::text("c".repeat(64))),
        ("starting", Json::text("d".repeat(64))),
        ("after", Json::Null),
        ("object", Json::text("e".repeat(32))),
        ("layout", Json::text("split")),
    ])
}
#[test]
fn exact_progress_selectors_survive_restart_without_review_state_or_content() {
    let f = Fixture::new("restart");
    let store = f.open();
    assert_eq!(store.load_progress_pins().unwrap().revision, 0);
    let saved = store.save_progress_pins(0, vec![pin(1), pin(2)]).unwrap();
    assert_eq!(saved.revision, 1);
    assert_eq!(f.open().load_progress_pins().unwrap(), saved);
    assert_eq!(store.load_fleet_pins().unwrap().revision, 0);
    assert_eq!(store.load_remote_fleet_pins().unwrap().revision, 0);
    assert_eq!(
        store.save_progress_pins(1, saved.pins.clone()).unwrap(),
        saved
    );
    assert!(store.save_progress_pins(0, vec![]).is_err());
    assert_eq!(store.save_progress_pins(1, vec![]).unwrap().revision, 2);
}
#[test]
fn progress_selector_schema_refuses_authority_fields_duplicates_and_bounds() {
    let f = Fixture::new("schema");
    let store = f.open();
    let state = ProgressPinState {
        revision: 0,
        pins: vec![pin(1)],
    };
    let encoded = state.to_json().encode();
    assert_eq!(ProgressPinState::parse_projection(&encoded).unwrap(), state);
    for (from, to) in [
        (
            "\"layout\":\"split\"",
            "\"layout\":\"split\",\"content\":\"private\"",
        ),
        ("\"key\":\"1\"", "\"key\":\"01\""),
        ("\"after\":null", "\"after\":\"../path\""),
        ("\"layout\":\"split\"", "\"layout\":\"approve\""),
        ("lane-", "wrong-"),
    ] {
        assert!(ProgressPinState::parse_projection(&encoded.replace(from, to)).is_err());
    }
    assert!(store.save_progress_pins(0, vec![pin(1), pin(1)]).is_err());
    assert!(store
        .save_progress_pins(0, (1..=5).map(pin).collect())
        .is_err());
    assert!(!f.0.join("desktop-progress-pins.json").exists());
}
#[test]
fn progress_pin_corruption_copy_links_and_interrupted_write_preserve_evidence() {
    let f = Fixture::new("refusal");
    let store = f.open();
    store.save_progress_pins(0, vec![pin(1)]).unwrap();
    let record = f.0.join("desktop-progress-pins.json");
    let pending = f.0.join("desktop-progress-pins.pending");
    let bytes = fs::read(&record).unwrap();
    let copy = Fixture::new("copy");
    fs::write(copy.0.join("desktop-progress-pins.json"), &bytes).unwrap();
    fs::set_permissions(
        copy.0.join("desktop-progress-pins.json"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    assert!(copy.open().load_progress_pins().is_err());
    fs::write(&pending, b"interrupted").unwrap();
    assert!(store.save_progress_pins(1, vec![]).is_err());
    assert_eq!(store.load_progress_pins().unwrap().pins, vec![pin(1)]);
    assert_eq!(fs::read(&pending).unwrap(), b"interrupted");
    fs::remove_file(&pending).unwrap();
    let alias = f.0.join("alias");
    fs::hard_link(&record, &alias).unwrap();
    assert!(store.load_progress_pins().is_err());
    fs::remove_file(&record).unwrap();
    symlink(&alias, &record).unwrap();
    assert!(store.load_progress_pins().is_err());
    assert_eq!(fs::read(&alias).unwrap(), bytes);
    fs::remove_file(&record).unwrap();
    fs::write(&record, b"{}").unwrap();
    assert!(store.save_progress_pins(1, vec![]).is_err());
    assert_eq!(fs::read(&record).unwrap(), b"{}");
    fs::remove_file(&record).unwrap();
    fs::write(&pending, b"initial interrupted").unwrap();
    assert!(store.load_progress_pins().is_err());
    assert_eq!(fs::read(&pending).unwrap(), b"initial interrupted");
}
#[test]
fn competing_progress_writers_cannot_overwrite_acknowledged_selectors() {
    let f = Fixture::new("writers");
    let barrier = std::sync::Barrier::new(2);
    let results = std::thread::scope(|scope| {
        let a = scope.spawn(|| {
            let s = f.open();
            barrier.wait();
            s.save_progress_pins(0, vec![pin(1)])
        });
        let b = scope.spawn(|| {
            let s = f.open();
            barrier.wait();
            s.save_progress_pins(0, vec![pin(2)])
        });
        [a.join().unwrap(), b.join().unwrap()]
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    let acknowledged = results.into_iter().find_map(Result::ok).unwrap();
    assert_eq!(f.open().load_progress_pins().unwrap(), acknowledged);
}

#[test]
fn full_review_capacity_does_not_displace_progress_pins() {
    use mesh_daemon::project_attachment::FleetPin;
    let f = Fixture::new("independent-capacity");
    let store = f.open();
    let reviews: Vec<_> = (1..=8)
        .map(|n| FleetPin {
            key: n.to_string(),
            objective: format!("fleet-{}", "a".repeat(64)),
            lane: "review-lane".into(),
            checkpoint: format!("checkpoint-{n}"),
            version: "b".repeat(64),
            bundle: "c".repeat(64),
            source_version: "d".repeat(64),
            input_layout: "inline".into(),
            review_mode: "content".into(),
            review_layout: "split".into(),
            input_after: None,
            input_object: None,
            review_object: None,
            input_open: false,
            candidate: None,
        })
        .collect();
    let saved_reviews = store.save_fleet_pins(0, reviews).unwrap();
    let progress = store
        .save_progress_pins(0, (1..=4).map(pin).collect())
        .unwrap();
    assert_eq!(store.load_fleet_pins().unwrap(), saved_reviews);
    assert_eq!(f.open().load_progress_pins().unwrap(), progress);
}
