//! Pending review inputs survive restart without submitting or approving work.
#![cfg(unix)]
use mesh_daemon::ipc::Json;
use mesh_daemon::project_attachment::{AttachmentStorage, FleetReviewOutbox};
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt as _};
use std::path::PathBuf;
struct Fixture(PathBuf);
impl Fixture {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("mesh-review-outbox-{name}-{}", std::process::id()));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
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
fn entry(operation: char, decision: bool) -> Json {
    let token = operation.to_string().repeat(32);
    let input = if decision {
        Json::object([
            ("operation", Json::text(token)),
            (
                "request",
                Json::text(format!("review-change-{}", "e".repeat(64))),
            ),
            ("expected_revision", Json::text("0")),
            ("checkpoint", Json::Null),
            ("version", Json::Null),
            ("bundle", Json::Null),
        ])
    } else {
        Json::object([
            ("request", Json::text(token)),
            (
                "message",
                Json::text("Please revise this result.\nKeep the original files."),
            ),
        ])
    };
    Json::object([
        (
            "kind",
            Json::text(if decision { "decision" } else { "change" }),
        ),
        ("objective", Json::text(format!("fleet-{}", "a".repeat(64)))),
        (
            "selection",
            Json::object([
                ("lane", Json::text("worker")),
                ("checkpoint", Json::text("saved")),
                ("version", Json::text("b".repeat(64))),
                ("bundle", Json::text("c".repeat(64))),
            ]),
        ),
        ("input", input),
    ])
}
#[test]
fn restart_exact_retry_stale_revision_and_explicit_removal() {
    let f = Fixture::new("restart");
    let store = f.open();
    assert_eq!(store.load_fleet_review_outbox().unwrap().revision, 0);
    let entries = vec![entry('1', false), entry('2', true)];
    let saved = store.save_fleet_review_outbox(0, entries.clone()).unwrap();
    assert_eq!(saved.revision, 1);
    let changed = Json::parse(
        &entry('1', false)
            .encode()
            .replace("Please revise", "Please replace"),
    )
    .unwrap();
    assert!(store.save_fleet_review_outbox(1, vec![changed]).is_err());
    assert_eq!(f.open().load_fleet_review_outbox().unwrap(), saved);
    assert_eq!(store.save_fleet_review_outbox(1, entries).unwrap(), saved);
    assert!(store.save_fleet_review_outbox(0, vec![]).is_err());
    assert_eq!(
        FleetReviewOutbox::parse(&saved.to_json().encode()).unwrap(),
        saved
    );
    assert_eq!(
        store.save_fleet_review_outbox(1, vec![]).unwrap().revision,
        2
    );
    assert!(f
        .open()
        .load_fleet_review_outbox()
        .unwrap()
        .entries
        .is_empty());
    assert_eq!(store.load_fleet_pins().unwrap().revision, 0);
}
#[test]
fn malformed_inputs_and_unknown_authority_fields_refuse() {
    let f = Fixture::new("invalid");
    let store = f.open();
    let valid = entry('1', false);
    for encoded in [
        valid
            .encode()
            .replace("Please revise this result.", "")
            .replace("Keep the original files.", ""),
        valid.encode().replace("worker", "../escape"),
        valid
            .encode()
            .replace("\"kind\":\"change\"", "\"kind\":\"approve\""),
        valid.encode().replace("\"request\":", "\"authority\":"),
    ] {
        let bad = Json::parse(&encoded).unwrap();
        assert!(store.save_fleet_review_outbox(0, vec![bad]).is_err());
    }
    assert!(store
        .save_fleet_review_outbox(0, vec![valid.clone(), valid])
        .is_err());
    let bad = Json::parse(&entry('2', true).encode().replace(
        "\"expected_revision\":\"0\"",
        "\"expected_revision\":\"64\"",
    ))
    .unwrap();
    assert!(store.save_fleet_review_outbox(0, vec![bad]).is_err());
    assert_eq!(store.load_fleet_review_outbox().unwrap().revision, 0);
}
#[test]
fn interrupted_staging_alias_and_catalog_substitution_preserve_evidence() {
    let f = Fixture::new("custody");
    let other = Fixture::new("other");
    let store = f.open();
    let staging = f.0.join("fleet-review-outbox.pending");
    fs::write(&staging, b"partial").unwrap();
    assert!(store.load_fleet_review_outbox().is_err());
    assert!(store
        .save_fleet_review_outbox(0, vec![entry('1', false)])
        .is_err());
    assert_eq!(fs::read(&staging).unwrap(), b"partial");
    fs::remove_file(staging).unwrap();
    let saved = store
        .save_fleet_review_outbox(0, vec![entry('1', false)])
        .unwrap();
    let record = f.0.join("fleet-review-outbox.json");
    let raw = fs::read(&record).unwrap();
    let copied = other.0.join("fleet-review-outbox.json");
    fs::write(&copied, &raw).unwrap();
    fs::set_permissions(&copied, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(other.open().load_fleet_review_outbox().is_err());
    let alias = f.0.join("alias");
    fs::hard_link(&record, &alias).unwrap();
    assert!(store.load_fleet_review_outbox().is_err());
    fs::remove_file(&alias).unwrap();
    fs::rename(&record, &alias).unwrap();
    symlink(&alias, &record).unwrap();
    assert!(store.load_fleet_review_outbox().is_err());
    fs::remove_file(&record).unwrap();
    fs::rename(&alias, &record).unwrap();
    assert_eq!(store.load_fleet_review_outbox().unwrap(), saved);
    fs::write(
        f.0.join("fleet-review-outbox.pending"),
        b"partial next snapshot",
    )
    .unwrap();
    assert_eq!(store.load_fleet_review_outbox().unwrap(), saved);
    assert!(store.save_fleet_review_outbox(1, vec![]).is_err());
    assert_eq!(fs::read(record).unwrap(), raw);
}

#[test]
fn nonprivate_oversized_and_full_outboxes_preserve_acknowledged_inputs() {
    let f = Fixture::new("bounds");
    let store = f.open();
    let entries: Vec<_> = ('1'..='8').map(|token| entry(token, false)).collect();
    let saved = store.save_fleet_review_outbox(0, entries.clone()).unwrap();
    let record = f.0.join("fleet-review-outbox.json");
    let original = fs::read(&record).unwrap();
    let mut overflow = entries;
    overflow.push(entry('9', false));
    assert!(store.save_fleet_review_outbox(1, overflow).is_err());
    assert_eq!(fs::read(&record).unwrap(), original);
    for (bytes, mode) in [(original.clone(), 0o644), (vec![b' '; 131_073], 0o600)] {
        fs::write(&record, &bytes).unwrap();
        fs::set_permissions(&record, fs::Permissions::from_mode(mode)).unwrap();
        assert!(store.load_fleet_review_outbox().is_err());
        assert!(store.save_fleet_review_outbox(1, vec![]).is_err());
        assert_eq!(fs::read(&record).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&record).unwrap().permissions().mode() & 0o777,
            mode
        );
        assert!(!f.0.join("fleet-review-outbox.pending").exists());
    }
    fs::write(&record, original).unwrap();
    fs::set_permissions(&record, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(f.open().load_fleet_review_outbox().unwrap(), saved);
}
