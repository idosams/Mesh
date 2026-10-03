use super::*;
use crate::fleet::remote_admission::authentication::tests::Fixture;
use ed25519_dalek::{Signer as _, SigningKey};
fn public(key: &SigningKey) -> PublicKey {
    PublicKey::from_bytes(key.verifying_key().to_bytes())
}
fn sign(key: &SigningKey, p: &SigningPayload) -> Result<Signature, String> {
    Ok(Signature::from_bytes(key.sign(p.as_bytes()).to_bytes()))
}
fn fixture_body(f: &Fixture, runtime: &mut Runtime) -> (Json, RemoteInputManifest) {
    let query = RemoteWorkerStatusChallenge::issue(
        runtime,
        "lane",
        "run",
        public(&f.coordinator),
        public(&f.worker),
    )
    .unwrap();
    let manifest = RemoteInputManifest::new(RecordDigest::from_bytes([0x91; 32]), vec![]).unwrap();
    (
        Json::object([
            ("target", query.body.get("target").unwrap().clone()),
            ("owner", Json::text("12".repeat(32))),
            ("mapping", Json::text("13".repeat(32))),
            ("initial", Json::text("14".repeat(32))),
            ("installation", Json::text("fixture-installation")),
            ("checkpoint", Json::text("saved-checkpoint")),
            ("review", Json::text("15".repeat(32))),
            ("version", Json::text(manifest.input().to_string())),
            ("manifest", Json::text(manifest.bundle().to_string())),
        ]),
        manifest,
    )
}
#[test]
fn saved_result_verification_binds_exact_context_manifest_and_domain_without_advancing_state() {
    let f = Fixture::new();
    let mut runtime = f.runtime(true);
    let (body, manifest) = fixture_body(&f, &mut runtime);
    let before = runtime.state().revision;
    let offer = RemoteSavedResultOffer::sign(body.clone(), |p| sign(&f.worker, p)).unwrap();
    let encoded = offer.encode();
    let verify = |r: &mut Runtime, input: &str, m: &RemoteInputManifest| {
        RemoteSavedResultOffer::verify(
            r,
            "lane",
            "run",
            public(&f.coordinator),
            public(&f.worker),
            input,
            m,
        )
    };
    assert_eq!(
        verify(&mut runtime, &encoded, &manifest).unwrap().encode(),
        encoded
    );
    let changed = RemoteInputManifest::new(RecordDigest::from_bytes([0x92; 32]), vec![]).unwrap();
    assert!(verify(&mut runtime, &encoded, &changed).is_err());
    assert!(RemoteSavedResultOffer::verify(
        &mut runtime,
        "lane",
        "wrong-run",
        public(&f.coordinator),
        public(&f.worker),
        &encoded,
        &manifest
    )
    .is_err());
    assert!(RemoteSavedResultOffer::verify(
        &mut runtime,
        "lane",
        "run",
        public(&SigningKey::from_bytes(&[88; 32])),
        public(&f.worker),
        &encoded,
        &manifest
    )
    .is_err());
    let other_domain = sign(&f.worker, &payload(REPLY_DOMAIN, &body)).unwrap();
    assert!(verify(
        &mut runtime,
        &envelope(SCHEMA, body.clone(), &other_domain),
        &manifest
    )
    .is_err());
    assert!(RemoteSavedResultOffer::sign(body, |p| sign(&f.coordinator, p)).is_err());
    assert!(verify(&mut runtime, &format!("{encoded} "), &manifest).is_err());
    assert!(verify(&mut runtime, &"x".repeat(MAX + 1), &manifest).is_err());
    assert_eq!(runtime.state().revision, before);
    assert_eq!(runtime.state().lanes["lane"].runs.len(), 1);
}
#[test]
fn saved_result_persistence_recovers_exact_bytes_and_refuses_conflicts_or_extra_history() {
    let f = Fixture::new();
    let mut runtime = f.runtime(true);
    let (body, _) = fixture_body(&f, &mut runtime);
    let mut registry = f.registry();
    let encoded = RemoteSavedResultOffer::sign(body.clone(), |p| sign(&f.worker, p))
        .unwrap()
        .persist(&mut registry.store)
        .unwrap()
        .encode();
    drop(registry);
    let mut registry = f.registry();
    assert_eq!(
        RemoteSavedResultOffer::retained(&registry.store, &body)
            .unwrap()
            .unwrap()
            .encode(),
        encoded
    );
    let Json::Object(mut fields) = body.clone() else {
        panic!("object");
    };
    fields.iter_mut().find(|(k, _)| k == "review").unwrap().1 = Json::text("16".repeat(32));
    let changed = Json::Object(fields);
    assert!(
        RemoteSavedResultOffer::sign(changed, |p| sign(&f.worker, p))
            .unwrap()
            .persist(&mut registry.store)
            .is_err()
    );
    assert_eq!(
        RemoteSavedResultOffer::retained(&registry.store, &body)
            .unwrap()
            .unwrap()
            .encode(),
        encoded
    );
    registry
        .store
        .append_with_outcome(
            &RemoteSavedResultOffer::stream(&body).unwrap(),
            1,
            "extra",
            "{}",
        )
        .unwrap();
    assert!(RemoteSavedResultOffer::retained(&registry.store, &body).is_err());
}

#[test]
fn native_result_receipt_resumes_verified_content_without_input_admission_or_working_files() {
    use crate::fleet::{
        NativeRemoteResultReceiver, RemoteInputChunk, RemoteInputDestination, RemoteInputEntry,
        RemoteWorkerStatusRequest,
    };
    use crate::ProtectedWorkspaceRoot;
    use mesh_cas::{ContentDigest as _, Digest32};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let mut runtime = f.runtime(true);
    let initial_revision = runtime.state().revision;
    let (mut body, _) = fixture_body(&f, &mut runtime);
    let bytes = vec![b'x'; 100_000];
    let digest = mesh_cas::Blake3::digest_bytes(&bytes);
    let manifest = RemoteInputManifest::new(
        RecordDigest::from_bytes([0x93; 32]),
        vec![RemoteInputEntry::File {
            path: "result.txt".into(),
            executable: false,
            digest,
            chunks: vec![RemoteInputChunk {
                digest,
                bytes: bytes.len() as u64,
            }],
        }],
    )
    .unwrap();
    let Json::Object(fields) = &mut body else {
        panic!("object");
    };
    fields.iter_mut().find(|(k, _)| k == "version").unwrap().1 =
        Json::text(manifest.input().to_string());
    fields.iter_mut().find(|(k, _)| k == "manifest").unwrap().1 =
        Json::text(manifest.bundle().to_string());
    let encoded = RemoteSavedResultOffer::sign(body, |p| sign(&f.worker, p))
        .unwrap()
        .encode();
    let store = f.path.join("result-store");
    let allocations = f.path.join("result-allocations");
    for path in [&store, &allocations] {
        fs::create_dir(path).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let destination = RemoteInputDestination::admit(
        &store,
        ProtectedWorkspaceRoot::inspect(&store).unwrap(),
        &allocations,
        ProtectedWorkspaceRoot::inspect(&allocations).unwrap(),
        &[],
    )
    .unwrap();
    fn request<'a>(f: &Fixture, runtime: &'a mut Runtime) -> RemoteWorkerStatusRequest<'a> {
        RemoteWorkerStatusRequest {
            runtime,
            lane: "lane",
            run: "run",
            coordinator: public(&f.coordinator),
            worker: public(&f.worker),
        }
    }
    let wrong = RemoteInputManifest::new(RecordDigest::from_bytes([0x94; 32]), vec![]).unwrap();
    assert!(NativeRemoteResultReceiver::new(
        &destination,
        wrong,
        &encoded,
        request(&f, &mut runtime)
    )
    .is_err());
    assert_eq!(
        fs::read_dir(&store).unwrap().count(),
        0,
        "invalid identity must not initialize a store"
    );
    let mut receiver = NativeRemoteResultReceiver::new(
        &destination,
        manifest.clone(),
        &encoded,
        request(&f, &mut runtime),
    )
    .unwrap();
    assert!(
        NativeRemoteResultReceiver::new(
            &destination,
            manifest.clone(),
            &encoded,
            request(&f, &mut runtime)
        )
        .is_err(),
        "one receiving owner"
    );
    assert_eq!(receiver.status(&mut runtime, digest).unwrap(), (0, false));
    assert!(receiver
        .status(&mut runtime, Digest32::from_bytes([9; 32]))
        .is_err());
    receiver
        .accept(&mut runtime, digest, 0, &bytes[..50_000], false)
        .unwrap();
    assert!(receiver.verify_complete(&mut runtime).is_err());
    assert!(receiver.record_content_receipt(&mut runtime).is_err());
    drop(receiver);
    let mut receiver = NativeRemoteResultReceiver::new(
        &destination,
        manifest.clone(),
        &encoded,
        request(&f, &mut runtime),
    )
    .unwrap();
    assert_eq!(
        receiver.status(&mut runtime, digest).unwrap(),
        (50_000, false)
    );
    assert!(receiver
        .accept(&mut runtime, digest, 0, &bytes[..1], false)
        .is_err());
    receiver
        .accept(&mut runtime, digest, 50_000, &bytes[50_000..], true)
        .unwrap();
    receiver.verify_complete(&mut runtime).unwrap();
    assert_eq!(
        receiver.status(&mut runtime, digest).unwrap(),
        (100_000, true)
    );
    assert_eq!(receiver.manifest(), &manifest);
    // A partial retained manifest is never overwritten or interpreted as a durable receipt.
    let manifest_path = store.join(format!("result-manifest-{}.json", manifest.bundle()));
    fs::write(&manifest_path, b"partial preserved manifest").unwrap();
    fs::set_permissions(&manifest_path, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(receiver.record_content_receipt(&mut runtime).is_err());
    assert_eq!(
        fs::read(&manifest_path).unwrap(),
        b"partial preserved manifest"
    );
    fs::rename(&manifest_path, store.join("preserved-partial-manifest")).unwrap();
    let receipt = receiver.record_content_receipt(&mut runtime).unwrap();
    fs::rename(&manifest_path, store.join("preserved-recorded-manifest")).unwrap();
    assert!(receiver.record_content_receipt(&mut runtime).is_err());
    assert!(
        !manifest_path.exists(),
        "an existing receipt cannot silently recreate missing metadata"
    );
    fs::rename(store.join("preserved-recorded-manifest"), &manifest_path).unwrap();
    assert_eq!(
        receiver
            .record_content_receipt(&mut runtime)
            .unwrap()
            .digest(),
        receipt.digest()
    );
    drop(receiver);
    drop(runtime);
    let mut runtime = Runtime::open(
        FleetStore::open(f.path.join("coordinator.sqlite")).unwrap(),
        "objective",
    )
    .unwrap();
    let (receiver, reopened) = NativeRemoteResultReceiver::reopen_content_receipt(
        &destination,
        &encoded,
        request(&f, &mut runtime),
    )
    .unwrap();
    assert_eq!(reopened.digest(), receipt.digest());
    receiver.verify_complete(&mut runtime).unwrap();
    drop(receiver);
    let original = fs::read(&manifest_path).unwrap();
    fs::write(&manifest_path, b"changed retained manifest").unwrap();
    assert!(NativeRemoteResultReceiver::reopen_content_receipt(
        &destination,
        &encoded,
        request(&f, &mut runtime)
    )
    .is_err());
    assert_eq!(
        fs::read(&manifest_path).unwrap(),
        b"changed retained manifest"
    );
    fs::write(&manifest_path, original).unwrap();
    let chunk_path = mesh_cas::StoreLayout::new(store.clone()).chunk_path(&digest);
    fs::write(&chunk_path, b"changed content").unwrap();
    assert!(NativeRemoteResultReceiver::reopen_content_receipt(
        &destination,
        &encoded,
        request(&f, &mut runtime)
    )
    .is_err());
    fs::write(&chunk_path, &bytes).unwrap();
    let (receiver, _) = NativeRemoteResultReceiver::reopen_content_receipt(
        &destination,
        &encoded,
        request(&f, &mut runtime),
    )
    .unwrap();
    runtime
        .store
        .append_with_outcome(
            &format!("result-content-{}", receipt.digest()),
            1,
            "extra",
            "{}",
        )
        .unwrap();
    assert!(receiver.record_content_receipt(&mut runtime).is_err());
    assert_eq!(runtime.state().revision, initial_revision);
    assert_eq!(runtime.state().lanes["lane"].runs.len(), 1);
    assert_eq!(
        fs::read_dir(&allocations).unwrap().count(),
        0,
        "receipt must not materialize working files"
    );
    fs::rename(&store, f.path.join("displaced-result-store")).unwrap();
    fs::create_dir(&store).unwrap();
    fs::set_permissions(&store, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(receiver.verify_complete(&mut runtime).is_err());
    assert!(receiver.status(&mut runtime, digest).is_err());
    drop(receiver);
    assert!(NativeRemoteResultReceiver::new(
        &destination,
        manifest,
        &encoded,
        request(&f, &mut runtime)
    )
    .is_err());
}

#[test]
fn result_summary_exposes_only_exact_public_saved_identities() {
    let f = Fixture::new();
    let mut runtime = f.runtime(true);
    let (body, manifest) = fixture_body(&f, &mut runtime);
    let offer = RemoteSavedResultOffer::sign(body.clone(), |p| sign(&f.worker, p)).unwrap();
    let summary = offer.public_summary();
    assert_eq!(
        summary.get("version"),
        Some(&Json::text(manifest.input().to_string()))
    );
    assert_eq!(
        summary.get("manifest"),
        Some(&Json::text(manifest.bundle().to_string()))
    );
    assert_eq!(summary.get("checkpoint"), body.get("checkpoint"));
    assert_eq!(summary.get("review"), body.get("review"));
    assert_eq!(
        summary.get("offer"),
        Some(&Json::text(
            Blake3::digest_bytes(offer.encode().as_bytes()).to_string()
        ))
    );
    let Json::Object(fields) = summary else {
        panic!("summary object")
    };
    assert_eq!(fields.len(), 5);
    for private in [
        "target",
        "owner",
        "mapping",
        "initial",
        "installation",
        "signature",
    ] {
        assert!(fields.iter().all(|(key, _)| key != private));
    }
}
