use super::*;
use crate::fleet::remote_admission::authentication::tests::Fixture;
use ed25519_dalek::{Signer as _, SigningKey};
fn public(k: &SigningKey) -> PublicKey {
    PublicKey::from_bytes(k.verifying_key().to_bytes())
}
fn sign(k: &SigningKey, p: &SigningPayload) -> Result<Signature, String> {
    Ok(Signature::from_bytes(k.sign(p.as_bytes()).to_bytes()))
}
fn policy(f: &Fixture) -> RemoteDispatchPolicy<'_> {
    RemoteDispatchPolicy {
        coordinator: public(&f.coordinator),
        worker: public(&f.worker),
        provider: "codex",
        maximum: Limits {
            lanes: 2,
            concurrency: 1,
            depth: 1,
            retries: 1,
        },
        max_lease_ms: 60_000,
    }
}

#[test]
fn catalog_pages_survive_reopen_repair_interrupted_publication_and_reject_corruption() {
    use crate::fleet::{
        receiving_session::tests::Setup, NativeRemoteInputReceiver, NativeWorkerInstallation,
        RemoteLaunchOutcome,
    };
    use crate::{CheckpointRuntimeParameters, ProtectedWorkspaceRoot, TrustedReviewers};
    use std::os::unix::fs::PermissionsExt;
    let s = Setup::new();
    let root = s.f.path.join("installation");
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let (installation, ()) = NativeWorkerInstallation::provision(
        &root,
        ProtectedWorkspaceRoot::inspect(&root).unwrap(),
        &[],
        |_| Ok((public(&s.f.worker), ())),
    )
    .unwrap();
    let mut registry = installation
        .registry(
            &hex(public(&s.f.coordinator).as_bytes()),
            "objective",
            policy(&s.f).maximum,
        )
        .unwrap();
    let RemoteAdmissionOutcome::Reserved(input) = registry
        .reserve(
            s.f.work.clone(),
            "0123456789abcdef0123456789abcdef",
            now().unwrap(),
        )
        .unwrap()
    else {
        panic!("original");
    };
    let mut receiver =
        NativeRemoteInputReceiver::new(&s.destination, s.manifest.clone(), &s.f.work.assignment)
            .unwrap();
    receiver.accept(s.digest, 0, &s.bytes, true).unwrap();
    let workspace = receiver
        .materialize_reserved(input)
        .unwrap()
        .into_worker_workspace(
            TrustedReviewers::default(),
            CheckpointRuntimeParameters::selected_defaults(),
        )
        .unwrap();
    let RemoteLaunchOutcome::Reserved(launch) = registry
        .reserve_launch(workspace, "codex", now().unwrap())
        .unwrap()
    else {
        panic!("launch");
    };
    let receipt = launch.receipt().clone();
    drop(launch);
    let mut registry = installation
        .registry(
            &hex(public(&s.f.coordinator).as_bytes()),
            "objective",
            policy(&s.f).maximum,
        )
        .unwrap();

    let assignment = &s.f.work.assignment.id;
    assert!(registry.saved_result_page("unknown", 0).unwrap().is_none());
    assert!(registry.saved_result_page(assignment, 1).is_err());
    let page = registry.saved_result_page(assignment, 0).unwrap().unwrap();
    assert_eq!(page.revision, 0);
    assert!(page.offers.is_empty());
    let manifest = RemoteInputManifest::new(RecordDigest::from_bytes([0x95; 32]), vec![]).unwrap();
    let mut expected = Vec::new();
    for n in 0..18 {
        let body = RemoteSavedResultOffer::body(
            &receipt,
            &format!("checkpoint-{n}"),
            RecordDigest::from_bytes([0x96; 32]),
            &manifest,
        )
        .unwrap();
        let offer = RemoteSavedResultOffer::sign(body.clone(), |p| sign(&s.f.worker, p)).unwrap();
        if n == 0 {
            // Simulate crash after the original record but before catalog append (also legacy data).
            registry
                .store
                .append(
                    &RemoteSavedResultOffer::stream(&body).unwrap(),
                    0,
                    "offer",
                    &offer.encode(),
                )
                .unwrap();
            assert!(registry
                .saved_result_page(assignment, 0)
                .unwrap()
                .unwrap()
                .offers
                .is_empty());
            assert_eq!(
                registry
                    .saved_result_offer(assignment, "checkpoint-0")
                    .unwrap()
                    .unwrap()
                    .encode(),
                offer.encode()
            );
        }
        let encoded = offer.persist(&mut registry.store).unwrap().encode();
        // Idempotent publication repairs the index without a new signature or duplicate row.
        RemoteSavedResultOffer::decode(&encoded)
            .unwrap()
            .persist(&mut registry.store)
            .unwrap();
        expected.push(encoded);
    }
    drop(registry);
    let mut registry = installation
        .registry(
            &hex(public(&s.f.coordinator).as_bytes()),
            "objective",
            policy(&s.f).maximum,
        )
        .unwrap();
    let page = registry.saved_result_page(assignment, 0).unwrap().unwrap();
    assert_eq!((page.revision, page.after, page.has_more), (18, 16, true));
    assert_eq!(
        page.offers
            .iter()
            .map(RemoteSavedResultOffer::encode)
            .collect::<Vec<_>>(),
        expected[..16]
    );
    let page = registry
        .saved_result_page(assignment, page.after)
        .unwrap()
        .unwrap();
    assert_eq!((page.revision, page.after, page.has_more), (18, 18, false));
    assert_eq!(
        page.offers
            .iter()
            .map(RemoteSavedResultOffer::encode)
            .collect::<Vec<_>>(),
        expected[16..]
    );
    assert!(registry
        .saved_result_page(assignment, 18)
        .unwrap()
        .unwrap()
        .offers
        .is_empty());
    assert!(registry.saved_result_page(assignment, 19).is_err());
    let first = RemoteSavedResultOffer::decode(&expected[0]).unwrap();
    registry
        .store
        .append(
            &stream(&first.body).unwrap(),
            18,
            "wrong-checkpoint",
            &first.encode(),
        )
        .unwrap();
    assert!(registry.saved_result_page(assignment, 18).is_err());
    // Corrupt original history cannot be laundered through an otherwise valid catalog page.
    registry
        .store
        .append(
            &RemoteSavedResultOffer::stream(&first.body).unwrap(),
            1,
            "extra",
            "{}",
        )
        .unwrap();
    assert!(registry.saved_result_page(assignment, 0).is_err());
}
