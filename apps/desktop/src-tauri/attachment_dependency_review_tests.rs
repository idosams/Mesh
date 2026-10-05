use super::*;
use mesh_daemon::project_attachment::{
    NativeConsumedStartRequest, NativeGrantInspection, NativeInputGrantRequest,
    NativeWorkDecisionRequest, ObservationLimits, SavedInputDecision,
};
use mesh_daemon::ManagedContentDigest;
use std::fs;

struct Fixture(PathBuf, bool);
impl Drop for Fixture {
    fn drop(&mut self) {
        if !self.1 {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
struct Restore(PathBuf, PathBuf);
impl Drop for Restore {
    fn drop(&mut self) {
        fs::rename(&self.1, &self.0).expect("restore selected source");
    }
}
fn text_preview(host: &AttachmentHost, id: &str, operation: &str) -> String {
    let value = Json::parse(&host.inspect(id, operation, Some("note.txt"), None).unwrap()).unwrap();
    value
        .get("inspection")
        .unwrap()
        .get("text")
        .and_then(Json::as_text)
        .unwrap()
        .to_owned()
}
#[test]
fn desktop_reopens_consumed_review_and_preserves_ordinary_projects() {
    let root = std::env::temp_dir().join(format!(
        "mesh-desktop-consumed-review-{}",
        std::process::id()
    ));
    fs::create_dir(&root).unwrap();
    let mut fixture = Fixture(root.clone(), false);
    let source = root.join("source");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("note.txt"), b"saved original").unwrap();
    let metadata = root.join("attached-projects");
    fs::create_dir(&metadata).unwrap();
    let storage = AttachmentStorage::open(&metadata).unwrap();
    let owner = storage.provision(&source).unwrap();
    let signer = NativeCaptureSigner::generate().unwrap();
    let input = owner
        .project()
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    let initial = owner
        .project()
        .save_capture(
            owner.metadata_path(),
            &input,
            signer.public_key(),
            |payload| signer.sign(payload),
        )
        .unwrap();
    let initial_id = initial.operation().to_string();
    let ordinary = AttachmentHost::new(&root);
    ordinary.projects().unwrap();
    assert_eq!(
        text_preview(&ordinary, owner.id(), &initial_id),
        "saved original"
    );
    assert!(ordinary
        .versions(owner.id(), None)
        .unwrap()
        .contains(&initial_id));
    assert!(ordinary
        .inspect(owner.id(), &initial_id, None, Some("missing"))
        .is_err());
    drop(ordinary);

    owner.enroll_dependency_history().unwrap();
    let request_id = |n| ManagedContentDigest::from_bytes([n; 32]);
    // Existing legacy saves remain readable, but are not invented native input provenance.
    // A newly reserved lane starts with enrolled empty history and captures fresh native work.
    let native_source = storage
        .reserve_dependency_lane(&owner, &owner, initial, request_id(200))
        .unwrap();
    fs::write(
        native_source.project().root().join("note.txt"),
        b"saved original",
    )
    .unwrap();
    let input = native_source
        .project()
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    let initial = storage
        .prepare_registered_dependency_capture(
            &native_source,
            &input,
            signer.public_key(),
            request_id(205),
            |payload| signer.sign(payload),
        )
        .unwrap()
        .commit()
        .unwrap();
    let child = storage
        .reserve_dependency_lane(&owner, &native_source, initial, request_id(201))
        .unwrap();
    let grant = storage
        .grant_saved_input_with_inputs(
            &owner,
            NativeInputGrantRequest {
                source: &native_source,
                version: initial,
                destination: &child,
                allowed: true,
                expected_previous: None,
                request: request_id(202),
            },
            &[],
        )
        .unwrap();
    let start = || NativeConsumedStartRequest {
        input: NativeGrantInspection {
            source: &native_source,
            version: initial,
            destination: &child,
            grant: grant.record(),
        },
        available: &[],
        request: request_id(203),
        limits: ObservationLimits::default(),
    };
    let candidate = storage
        .prepare_consumed_start(&owner, start(), signer.public_key(), |payload| {
            signer.sign(payload)
        })
        .unwrap();
    let staged = candidate.stage(&storage).unwrap();
    candidate
        .complete_fenced_consumption(&storage, &staged)
        .unwrap();
    let first = storage.registered_dependency_versions(child.id()).unwrap()[0];
    let first_id = first.operation().to_string();
    let host = AttachmentHost::new(&root);
    host.projects().unwrap();
    assert_eq!(text_preview(&host, child.id(), &first_id), "saved original");
    fs::write(
        child.project().root().join("note.txt"),
        b"later saved progress",
    )
    .unwrap();
    let generation = host.state.lock().unwrap().projects[child.id()]
        .generation
        .to_string();
    host.control(child.id(), &generation, "resume").unwrap();
    let service = host
        .state
        .lock()
        .unwrap()
        .projects
        .get_mut(child.id())
        .unwrap()
        .service
        .take()
        .unwrap();
    let wait = |attempt| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        loop {
            let status = service.status();
            if status.attempts >= attempt && status.phase == CapturePhase::Waiting {
                break status;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "capture did not complete: {status:?}"
            );
            service.wait_for_update(status.revision, std::time::Duration::from_millis(100));
        }
    };
    let first_capture = wait(1);
    assert_eq!(first_capture.last_outcome, CaptureOutcome::Saved);
    let later = first_capture.saved_version.unwrap();
    assert!(service.request_capture());
    let unchanged = wait(first_capture.attempts + 1);
    assert_eq!(unchanged.last_outcome, CaptureOutcome::Unchanged);
    assert_eq!(unchanged.saved_version, Some(later));
    assert_eq!(unchanged.versions_saved, 1);
    let stopped = service.stop_and_join().unwrap();
    assert_eq!(stopped.phase, CapturePhase::Stopped);
    host.state
        .lock()
        .unwrap()
        .projects
        .get_mut(child.id())
        .unwrap()
        .recovered = stopped;
    let later_id = later.operation().to_string();
    assert_eq!(text_preview(&host, child.id(), &first_id), "saved original");
    assert_eq!(
        text_preview(&host, child.id(), &later_id),
        "later saved progress"
    );
    assert!(host
        .compare(child.id(), &first_id, &later_id, None)
        .unwrap()
        .contains("modified"));
    assert!(host
        .inspect(child.id(), &later_id, None, None)
        .unwrap()
        .contains("note.txt"));
    let selected_change = host
        .comparison_path(child.id(), &first_id, &later_id, "note.txt")
        .unwrap();
    assert!(selected_change.contains("modified"));
    assert!(host
        .comparison_path(child.id(), &first_id, &later_id, "missing")
        .is_err());
    drop(host);

    let reopened = AttachmentHost::new(&root);
    reopened.projects().unwrap();
    let state = reopened.state.lock().unwrap();
    let recovered = state.projects.get(child.id()).unwrap();
    assert_eq!(recovered.recovery, Some("restored-stopped"));
    assert!(recovered.service.is_none());
    drop(state);
    assert_eq!(
        text_preview(&reopened, child.id(), &first_id),
        "saved original"
    );
    let versions = reopened.versions(child.id(), None).unwrap();
    assert!(versions.contains(&first_id) && versions.contains(&later_id));
    assert!(reopened.versions(child.id(), Some("unknown")).is_err());
    let harness_args = |action: &str| {
        vec![
            "--mesh-registered-attachment".to_owned(),
            action.to_owned(),
            metadata.to_str().unwrap().to_owned(),
            child.id().to_owned(),
        ]
    };
    let harness_preview = |operation: &str, path: &str| {
        let mut args = harness_args("preview");
        args.extend([operation.to_owned(), path.to_owned()]);
        let mut output = Vec::new();
        crate::attachment_capture::test_run(
            &args,
            std::io::Cursor::new(Vec::<u8>::new()),
            &mut output,
        )?;
        Json::parse(std::str::from_utf8(&output).unwrap().trim()).map_err(|e| e.to_string())
    };
    let harness = |action: &str| {
        let mut output = Vec::new();
        crate::attachment_capture::test_run(
            &harness_args(action),
            std::io::Cursor::new(Vec::<u8>::new()),
            &mut output,
        )?;
        Ok::<_, String>(output)
    };
    // The command parser and dispatcher share the same saved identities with desktop review.
    let unchanged_output = harness("capture").unwrap();
    let unchanged = Json::parse(std::str::from_utf8(&unchanged_output).unwrap().trim()).unwrap();
    assert_eq!(unchanged.get("saved_version"), Some(&Json::text(&later_id)));
    fs::write(
        child.project().root().join("note.txt"),
        b"harness saved progress",
    )
    .unwrap();
    let captured = harness("capture").unwrap();
    let captured = Json::parse(std::str::from_utf8(&captured).unwrap().trim()).unwrap();
    let harness_id = captured
        .get("saved_version")
        .and_then(Json::as_text)
        .unwrap();
    assert_ne!(harness_id, later_id);
    assert_eq!(
        text_preview(&reopened, child.id(), harness_id),
        "harness saved progress"
    );
    let owner_before_preview =
        fs::read(owner.metadata_path().join(mesh_daemon::RECORD_FILE_NAME)).unwrap();
    let child_before_preview =
        fs::read(child.metadata_path().join(mesh_daemon::RECORD_FILE_NAME)).unwrap();
    for (operation, expected) in [
        (first_id.as_str(), "saved original"),
        (later_id.as_str(), "later saved progress"),
        (harness_id, "harness saved progress"),
    ] {
        let preview = harness_preview(operation, "note.txt").unwrap();
        assert_eq!(preview.get("operation"), Some(&Json::text(operation)));
        assert_eq!(preview.get("text"), Some(&Json::text(expected)));
    }
    assert!(harness_preview(&"0".repeat(64), "note.txt").is_err());
    assert!(harness_preview(&first_id, "missing").is_err());
    assert!(harness_preview(&first_id, "../note.txt").is_err());
    assert_eq!(
        fs::read(owner.metadata_path().join(mesh_daemon::RECORD_FILE_NAME)).unwrap(),
        owner_before_preview
    );
    assert_eq!(
        fs::read(child.metadata_path().join(mesh_daemon::RECORD_FILE_NAME)).unwrap(),
        child_before_preview
    );
    let listed = harness("versions").unwrap();
    let listed = Json::parse(std::str::from_utf8(&listed).unwrap().trim()).unwrap();
    assert_eq!(
        listed
            .get("versions")
            .and_then(Json::as_array)
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        text_preview(&reopened, child.id(), &first_id),
        "saved original"
    );

    use std::io::Write as _;
    let (mut controls, input) = std::os::unix::net::UnixStream::pair().unwrap();
    let args = harness_args("watch");
    let watcher = std::thread::spawn(move || {
        let mut output = Vec::new();
        let result = crate::attachment_capture::test_run(&args, input, &mut output);
        (result, output)
    });
    fs::write(child.project().root().join("note.txt"), b"watched progress").unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    let watched = loop {
        let versions = storage.registered_review_versions(&child).unwrap();
        if versions.len() == 4 {
            break Some(*versions.last().unwrap());
        }
        if std::time::Instant::now() >= deadline {
            break None;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    controls.write_all(b"stop\n").unwrap();
    let (result, output) = watcher.join().unwrap();
    result.unwrap();
    let watched = watched.expect("registered watch must capture new input");
    let final_status = Json::parse(
        std::str::from_utf8(&output)
            .unwrap()
            .lines()
            .last()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(final_status.get("phase"), Some(&Json::text("stopped")));
    assert_eq!(
        text_preview(&reopened, child.id(), &watched.operation().to_string()),
        "watched progress"
    );
    assert_eq!(
        fs::read(source.join("note.txt")).unwrap(),
        b"saved original"
    );

    let moved = root.join("source-offline");
    fs::rename(&source, &moved).unwrap();
    let restore = Restore(source.clone(), moved);
    let refused_versions = reopened.versions(child.id(), None);
    let refused_preview = reopened.inspect(child.id(), &first_id, Some("note.txt"), None);
    let refused_harness_preview = harness_preview(&first_id, "note.txt");
    let refused_comparison = reopened.compare(child.id(), &first_id, &later_id, None);
    let refused_path = reopened.comparison_path(child.id(), &first_id, &later_id, "note.txt");
    let input = child
        .project()
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    let signed = std::cell::Cell::new(false);
    let refused_capture =
        storage.save_registered_capture(&child, &input, signer.public_key(), |payload| {
            signed.set(true);
            signer.sign(payload)
        });
    let refused_harness_capture = harness("capture");
    let refused_harness_versions = harness("versions");
    drop(restore);
    assert!(refused_harness_capture.is_err() && refused_harness_versions.is_err());
    assert!(refused_capture.is_err());
    assert!(!signed.get(), "missing owner must refuse before signing");
    assert!(refused_path.is_err());
    assert!(refused_versions.is_err() && refused_preview.is_err() && refused_comparison.is_err());
    assert!(refused_harness_preview.is_err());
    assert_eq!(
        text_preview(&reopened, child.id(), &first_id),
        "saved original"
    );

    // Eligibility control must read consumed history without falling back to legacy capture.
    let request = |decision, previous, number| NativeWorkDecisionRequest {
        source: &child,
        version: later,
        decision,
        expected_previous: previous,
        request: request_id(number),
    };
    let child_journal = child.metadata_path().join("records.mesh");
    let before_child = fs::read(&child_journal).unwrap();
    assert!(
        storage
            .decide_work_input(&owner, request(SavedInputDecision::Rejected, None, 210))
            .is_err(),
        "missing consumed input context must refuse"
    );
    let rejected = storage
        .decide_work_input_with_inputs(
            &owner,
            request(SavedInputDecision::Rejected, None, 210),
            &[&native_source],
        )
        .expect("complete native context must permit a consumed-child decision");
    assert_eq!(rejected.revision(), 1);
    let owner_journal = owner.metadata_path().join("records.mesh");
    let acknowledged = fs::read(&owner_journal).unwrap();
    let repeated = storage
        .decide_work_input_with_inputs(
            &owner,
            request(SavedInputDecision::Rejected, None, 210),
            &[&native_source],
        )
        .unwrap();
    assert_eq!(repeated, rejected);
    assert_eq!(fs::read(&owner_journal).unwrap(), acknowledged);
    assert!(
        storage
            .decide_work_input_with_inputs(
                &owner,
                request(SavedInputDecision::Eligible, None, 211),
                &[&native_source],
            )
            .is_err(),
        "stale decision cannot replace current rejection"
    );
    assert_eq!(fs::read(&owner_journal).unwrap(), acknowledged);
    let replaced = storage
        .decide_work_input_with_inputs(
            &owner,
            request(
                SavedInputDecision::Replaced(first),
                Some(rejected.record()),
                212,
            ),
            &[&native_source],
        )
        .unwrap();
    assert_eq!(replaced.revision(), 2);
    let eligible = storage
        .decide_work_input_with_inputs(
            &owner,
            request(SavedInputDecision::Eligible, Some(replaced.record()), 213),
            &[&native_source],
        )
        .unwrap();
    assert_eq!(eligible.revision(), 3);
    assert_eq!(fs::read(&child_journal).unwrap(), before_child);
    assert_eq!(
        text_preview(&reopened, child.id(), &first_id),
        "saved original"
    );
    assert_eq!(
        text_preview(&reopened, child.id(), &later_id),
        "later saved progress"
    );
    // An explicit executable-proof run may retain this already-verified fixture. Ordinary
    // test runs still clean it up, and no assertion above is skipped by exporting it.
    if let Some(path) = std::env::var_os("MESH_REGISTERED_CAPTURE_FIXTURE") {
        let manifest = Json::object([
            ("schema", Json::text("mesh.registered-capture-fixture/v1")),
            ("root", Json::text(root.to_str().unwrap())),
            ("storage", Json::text(metadata.to_str().unwrap())),
            (
                "project",
                Json::text(child.project().root().to_str().unwrap()),
            ),
            ("registration", Json::text(child.id())),
            (
                "journal",
                Json::text(
                    child
                        .metadata_path()
                        .join(mesh_daemon::RECORD_FILE_NAME)
                        .to_str()
                        .unwrap(),
                ),
            ),
            ("owner", Json::text(source.to_str().unwrap())),
        ])
        .encode();
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        file.write_all(manifest.as_bytes()).unwrap();
        file.sync_all().unwrap();
        fixture.1 = true;
    }
}
