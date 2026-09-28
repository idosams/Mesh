//! Durable fleet navigation carries no content, source custody or execution authority.
#![cfg(unix)]
use mesh_daemon::project_attachment::{AttachmentPin, AttachmentStorage, FleetPin, FleetPinState};
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt as _};
use std::path::PathBuf;
struct Fixture(PathBuf);
impl Fixture {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("mesh-fleet-pins-{name}-{}", std::process::id()));
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
fn pin(key: &str) -> FleetPin {
    FleetPin {
        candidate: None,
        key: key.into(),
        objective: format!("fleet-{}", "a".repeat(64)),
        lane: "worker".into(),
        checkpoint: format!("checkpoint-{key}"),
        version: "b".repeat(64),
        bundle: "c".repeat(64),
        source_version: "d".repeat(64),
        input_after: Some("e".repeat(32)),
        input_object: Some("f".repeat(32)),
        input_open: true,
        input_layout: "inline".into(),
        review_object: Some("1".repeat(32)),
        review_mode: "content".into(),
        review_layout: "split".into(),
    }
}
#[test]
fn fleet_pin_roundtrip_removal_and_attachment_namespace_are_independent() {
    let f = Fixture::new("roundtrip");
    let store = f.open();
    assert_eq!(store.load_fleet_pins().unwrap().revision, 0);
    let attachment = store
        .save_comparison_pins(
            0,
            vec![AttachmentPin {
                key: "1".into(),
                project: "2".repeat(64),
                base: "3".repeat(64),
                target: "4".repeat(64),
                after: None,
                path: None,
            }],
        )
        .unwrap();
    let saved = store.save_fleet_pins(0, vec![pin("1"), pin("2")]).unwrap();
    assert_eq!(saved.revision, 1);
    assert_eq!(f.open().load_fleet_pins().unwrap(), saved);
    assert_eq!(
        FleetPinState::parse_projection(&saved.to_json().encode()).unwrap(),
        saved
    );
    assert_eq!(store.save_fleet_pins(1, saved.pins.clone()).unwrap(), saved);
    assert!(store.save_fleet_pins(0, vec![]).is_err());
    assert_eq!(store.save_fleet_pins(1, vec![]).unwrap().revision, 2);
    assert!(f.open().load_fleet_pins().unwrap().pins.is_empty());
    assert_eq!(f.open().load_comparison_pins().unwrap(), attachment);
    assert!(store.registrations().unwrap().is_empty());
    assert_eq!(
        fs::metadata(f.0.join("desktop-fleet-pins.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}
#[test]
fn malformed_selectors_are_refused_without_creating_storage() {
    let f = Fixture::new("schema");
    let store = f.open();
    for mutate in [
        (|p: &mut FleetPin| p.key = "01".into()) as fn(&mut FleetPin),
        |p| p.objective = "other".into(),
        |p| p.lane = "../worker".into(),
        |p| p.source_version = "live".into(),
        |p| p.version = "B".repeat(64),
        |p| p.bundle.clear(),
        |p| p.input_object = Some("/tmp/private".into()),
        |p| p.input_open = false,
        |p| p.review_mode = "approve".into(),
        |p| p.input_layout = "execute".into(),
        |p| p.review_layout = "unified".into(),
    ] {
        let mut value = pin("1");
        mutate(&mut value);
        let state = FleetPinState {
            revision: 0,
            pins: vec![value.clone()],
        };
        assert!(FleetPinState::parse_projection(&state.to_json().encode()).is_err());
        assert!(store.save_fleet_pins(0, vec![value]).is_err());
    }
    let value = pin("1");
    let mut duplicate = value.clone();
    duplicate.key = "2".into();
    assert!(store.save_fleet_pins(0, vec![value, duplicate]).is_err());
    assert!(store
        .save_fleet_pins(0, (1..=9).map(|n| pin(&n.to_string())).collect())
        .is_err());
    let encoded = FleetPinState {
        revision: 0,
        pins: vec![pin("1")],
    }
    .to_json()
    .encode();
    assert!(FleetPinState::parse_projection(&encoded.replacen(
        "\"input_open\":true",
        "\"input_open\":true,\"content\":\"private\"",
        1
    ))
    .is_err());
    assert!(!f.0.join("desktop-fleet-pins.json").exists());
}
#[test]
fn corrupt_copied_linked_and_interrupted_records_are_preserved_and_refused() {
    let f = Fixture::new("refusal");
    let store = f.open();
    store.save_fleet_pins(0, vec![pin("1")]).unwrap();
    let record = f.0.join("desktop-fleet-pins.json");
    let bytes = fs::read(&record).unwrap();
    let copy = Fixture::new("copied");
    fs::write(copy.0.join("desktop-fleet-pins.json"), &bytes).unwrap();
    fs::set_permissions(
        copy.0.join("desktop-fleet-pins.json"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    assert!(copy.open().load_fleet_pins().is_err());
    let pending = f.0.join("desktop-fleet-pins.pending");
    fs::write(&pending, b"interrupted").unwrap();
    assert!(store.save_fleet_pins(1, vec![]).is_err());
    assert_eq!(store.load_fleet_pins().unwrap().pins, vec![pin("1")]);
    assert_eq!(fs::read(&record).unwrap(), bytes);
    assert_eq!(fs::read(&pending).unwrap(), b"interrupted");
    fs::remove_file(&pending).unwrap();
    let alias = f.0.join("alias");
    fs::hard_link(&record, &alias).unwrap();
    assert!(store.load_fleet_pins().is_err());
    assert!(store.save_fleet_pins(1, vec![]).is_err());
    fs::remove_file(&record).unwrap();
    symlink(&alias, &record).unwrap();
    assert!(store.load_fleet_pins().is_err());
    assert!(store.save_fleet_pins(1, vec![]).is_err());
    assert_eq!(fs::read(&alias).unwrap(), bytes);
    fs::remove_file(&record).unwrap();
    fs::write(&record, b"{}").unwrap();
    assert!(store.load_fleet_pins().is_err());
    assert!(store.save_fleet_pins(1, vec![]).is_err());
    assert_eq!(fs::read(&record).unwrap(), b"{}");
    fs::remove_file(&record).unwrap();
    fs::write(&pending, b"first interrupted write").unwrap();
    assert!(store.load_fleet_pins().is_err());
    assert!(store.save_fleet_pins(0, vec![pin("1")]).is_err());
    assert_eq!(fs::read(&pending).unwrap(), b"first interrupted write");
}
#[test]
fn competing_writers_cannot_overwrite_an_acknowledged_snapshot() {
    let f = Fixture::new("concurrent");
    let barrier = std::sync::Barrier::new(2);
    let results = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            let store = f.open();
            barrier.wait();
            store.save_fleet_pins(0, vec![pin("1")])
        });
        let second = scope.spawn(|| {
            let store = f.open();
            barrier.wait();
            store.save_fleet_pins(0, vec![pin("2")])
        });
        [first.join().unwrap(), second.join().unwrap()]
    });
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        &f.open().load_fleet_pins().unwrap(),
        results.iter().find_map(|r| r.as_ref().ok()).unwrap()
    );
}

#[test]
fn exhausted_revision_preserves_the_acknowledged_snapshot_and_never_stages_a_replacement() {
    let f = Fixture::new("exhausted-revision");
    let store = f.open();
    store.save_fleet_pins(0, vec![pin("1")]).unwrap();
    let record = f.0.join("desktop-fleet-pins.json");
    let encoded = fs::read_to_string(&record).unwrap();
    let exhausted = encoded.replacen(
        "\"revision\":\"1\"",
        &format!("\"revision\":\"{}\"", u64::MAX),
        1,
    );
    assert_ne!(exhausted, encoded);
    fs::write(&record, &exhausted).unwrap();
    let saved = store.load_fleet_pins().unwrap();
    assert_eq!(saved.revision, u64::MAX);
    assert_eq!(
        store.save_fleet_pins(u64::MAX, saved.pins.clone()).unwrap(),
        saved
    );
    assert!(store.save_fleet_pins(u64::MAX, vec![pin("2")]).is_err());
    assert!(store.save_fleet_pins(0, vec![]).is_err());
    assert_eq!(fs::read_to_string(&record).unwrap(), exhausted);
    assert!(!f.0.join("desktop-fleet-pins.pending").exists());
    assert_eq!(f.open().load_fleet_pins().unwrap(), saved);
}

#[test]
fn legacy_pins_migrate_only_on_change_and_candidate_inputs_survive_restart() {
    use mesh_daemon::project_attachment::FleetCandidatePin;
    let f = Fixture::new("candidate-migration");
    let store = f.open();
    let saved = store.save_fleet_pins(0, vec![pin("1")]).unwrap();
    let record = f.0.join("desktop-fleet-pins.json");
    let legacy = fs::read_to_string(&record)
        .unwrap()
        .replace("mesh.fleet-pins/v2", "mesh.fleet-pins/v1")
        .replace(",\"candidate\":null", "");
    fs::write(&record, &legacy).unwrap();
    assert_eq!(store.load_fleet_pins().unwrap(), saved);
    assert_eq!(fs::read_to_string(&record).unwrap(), legacy);
    let old_projection = saved
        .to_json()
        .encode()
        .replace(
            "mesh.desktop-fleet-pin-selectors/v2",
            "mesh.desktop-fleet-pin-selectors/v1",
        )
        .replace(",\"candidate\":null", "");
    assert_eq!(
        FleetPinState::parse_projection(&old_projection).unwrap(),
        saved
    );
    let mut pending = pin("1");
    pending.candidate = Some(FleetCandidatePin {
        project: "1".repeat(64),
        request: "2".repeat(32),
        expected_main: Some("3".repeat(64)),
    });
    let stored = store.save_fleet_pins(1, vec![pending.clone()]).unwrap();
    assert_eq!(f.open().load_fleet_pins().unwrap(), stored);
    assert!(fs::read_to_string(&record)
        .unwrap()
        .contains("mesh.fleet-pins/v2"));
    assert!(store.save_fleet_pins(1, vec![pin("1")]).is_err());
    for mutate in [
        (|p: &mut FleetCandidatePin| p.project = "../source".into()) as fn(&mut FleetCandidatePin),
        |p| p.request = "1".repeat(64),
        |p| p.expected_main = Some("latest".into()),
    ] {
        let mut invalid = pending.clone();
        mutate(invalid.candidate.as_mut().unwrap());
        assert!(store.save_fleet_pins(2, vec![invalid]).is_err());
    }
    let encoded = stored.to_json().encode();
    assert!(FleetPinState::parse_projection(
        &encoded.replace("\"candidate\":{", "\"candidate\":{\"content\":\"secret\",")
    )
    .is_err());
    assert!(
        FleetPinState::parse_projection(&encoded.replace("selectors/v2", "selectors/v1")).is_err()
    );
    assert_eq!(f.open().load_fleet_pins().unwrap(), stored);
}
