use super::*;
use mesh_daemon::project_attachment::{
    NativeConsumedStartRequest, NativeGrantInspection, NativeInputGrantRequest, ObservationLimits,
};
use mesh_daemon::ManagedContentDigest;
use std::fs;

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
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
    let _fixture = Fixture(root.clone());
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
    let initial = native_source
        .prepare_dependency_capture(&input, signer.public_key(), request_id(205), |payload| {
            signer.sign(payload)
        })
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
    let input = child
        .project()
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    let later = storage
        .prepare_consumed_capture(
            &owner,
            start(),
            &input,
            signer.public_key(),
            request_id(204),
            |payload| signer.sign(payload),
        )
        .unwrap()
        .commit()
        .unwrap();
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
    let moved = root.join("source-offline");
    fs::rename(&source, &moved).unwrap();
    let restore = Restore(source.clone(), moved);
    let refused_versions = reopened.versions(child.id(), None);
    let refused_preview = reopened.inspect(child.id(), &first_id, Some("note.txt"), None);
    let refused_comparison = reopened.compare(child.id(), &first_id, &later_id, None);
    drop(restore);
    assert!(refused_versions.is_err() && refused_preview.is_err() && refused_comparison.is_err());
    assert_eq!(
        text_preview(&reopened, child.id(), &first_id),
        "saved original"
    );
}
