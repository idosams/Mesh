//! Independent complete-graph coverage, separate from the long recovery campaign.
use super::*;
use std::{fs, path::PathBuf};
#[test]
fn private_chained_graph_matches_saved_content_before_and_after_capture() {
    use crate::project_attachment::{
        NativeConsumedStartRequest, NativeGrantInspection, NativeInputGrantRequest,
        ObservationLimits,
    };
    use crate::workspace::OpenWorkspace;
    use ed25519_dalek::{Signer as _, SigningKey};
    use mesh_crypto::SigningPayload;
    use mesh_types::{PublicKey, Signature};
    struct Cleanup(PathBuf, bool);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            if !self.1 {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
    }
    let root =
        std::env::temp_dir().join(format!("mesh-private-chained-graph-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let _cleanup = Cleanup(root.clone(), false);
    fs::create_dir(root.join("source")).unwrap();
    fs::create_dir(root.join("metadata")).unwrap();
    fs::write(root.join("source/note"), b"exact native input").unwrap();
    let storage = AttachmentStorage::open(&root.join("metadata")).unwrap();
    let owner = storage.provision(&root.join("source")).unwrap();
    let input = owner
        .project()
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    let (_, created) = owner
        .project()
        .history_configuration(&owner.store, Some(input.exclusion_digest()))
        .unwrap();
    drop(
        OpenWorkspace::open_attachment_store(owner.metadata_path(), owner.store.clone(), created)
            .unwrap(),
    );
    owner.enroll_dependency_history().unwrap();
    let key = SigningKey::from_bytes(&[179; 32]);
    let actor = PublicKey::from_bytes(key.verifying_key().to_bytes());
    let id = |n| RecordDigest::from_bytes([n; 32]);
    let sign = |payload: &SigningPayload| {
        Ok::<_, &'static str>(Signature::from_bytes(
            key.sign(payload.as_bytes()).to_bytes(),
        ))
    };
    let version = owner
        .prepare_dependency_capture(&input, actor, id(1), sign)
        .unwrap()
        .commit()
        .unwrap();
    let destination = storage
        .reserve_dependency_lane(&owner, &owner, version, id(2))
        .unwrap();
    let grant = storage
        .grant_saved_input(
            &owner,
            NativeInputGrantRequest {
                source: &owner,
                version,
                destination: &destination,
                allowed: true,
                expected_previous: None,
                request: id(3),
            },
        )
        .unwrap();
    let candidate = storage
        .prepare_consumed_start(
            &owner,
            NativeConsumedStartRequest {
                input: NativeGrantInspection {
                    source: &owner,
                    version,
                    destination: &destination,
                    grant: grant.record(),
                },
                available: &[],
                request: id(4),
                limits: ObservationLimits::default(),
            },
            actor,
            sign,
        )
        .unwrap();
    let staged = candidate.stage(&storage).unwrap();
    candidate
        .complete_fenced_consumption(&storage, &staged)
        .unwrap();
    let consumed = storage
        .registered_dependency_versions(destination.id())
        .unwrap()[0];

    let child = storage
        .reserve_dependency_lane_with_inputs(&owner, &destination, consumed, id(20), &[&owner])
        .unwrap();
    let child_grant = storage
        .grant_saved_input_with_inputs(
            &owner,
            NativeInputGrantRequest {
                source: &destination,
                version: consumed,
                destination: &child,
                allowed: true,
                expected_previous: None,
                request: id(21),
            },
            &[&owner],
        )
        .unwrap();
    let child_candidate = storage
        .prepare_consumed_start(
            &owner,
            NativeConsumedStartRequest {
                input: NativeGrantInspection {
                    source: &destination,
                    version: consumed,
                    destination: &child,
                    grant: child_grant.record(),
                },
                available: &[&owner],
                request: id(22),
                limits: ObservationLimits::default(),
            },
            actor,
            sign,
        )
        .unwrap();
    let staged = child_candidate.stage(&storage).unwrap();
    child_candidate
        .complete_fenced_consumption(&storage, &staged)
        .unwrap();
    let first = storage.registered_dependency_versions(child.id()).unwrap()[0];
    let graph = storage
        .inspect_dependency_graph(&owner, &child, first, &[&destination, &owner])
        .unwrap();
    let before = [&owner, &destination, &child]
        .map(|w| fs::read(w.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap());
    assert_eq!(
        storage
            .inspect_private_dependency_graph(child.id(), first.operation())
            .unwrap(),
        graph.to_json(),
        "private discovery must resolve the intermediate consumed lane"
    );
    assert_eq!(
        [&owner, &destination, &child]
            .map(|w| fs::read(w.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap()),
        before
    );
    fs::write(child.project().root().join("note"), b"new child progress").unwrap();
    let capture = child
        .project()
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    let next = storage
        .prepare_registered_dependency_capture(&child, &capture, actor, id(23), sign)
        .unwrap()
        .commit()
        .unwrap();
    let next_graph = storage
        .inspect_dependency_graph(&owner, &child, next, &[&destination, &owner])
        .unwrap();
    assert_eq!(next_graph.operation_count(), graph.operation_count() + 1);
    let before = [&owner, &destination, &child]
        .map(|w| fs::read(w.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap());
    assert_eq!(
        storage
            .inspect_private_dependency_graph(child.id(), next.operation())
            .unwrap(),
        next_graph.to_json(),
        "private discovery must retain consumed ancestry after later capture"
    );
    assert_eq!(
        storage
            .inspect_private_dependency_graph(child.id(), first.operation())
            .unwrap(),
        graph.to_json(),
        "later capture must preserve the earlier exact graph"
    );
    assert_eq!(
        [&owner, &destination, &child]
            .map(|w| fs::read(w.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap()),
        before
    );
}
