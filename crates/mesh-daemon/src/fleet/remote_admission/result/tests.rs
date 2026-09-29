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
