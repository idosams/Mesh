//! Attachment approval advances only external Mesh main while ordinary work continues.
#![cfg(unix)]
use crate::project_attachment::{AttachmentStorage, ObservationLimits, ProvisionedAttachment};
use crate::{ipc::Json, TrustedReviewers};
use mesh_approval::{
    ApprovalDecision, ExpectedHumanApproval, HumanApprovalCredential, HumanApprovalReceiptDraft,
};
use ring::rand::SystemRandom;
use ring::signature::{EcdsaKeyPair, KeyPair as _, ECDSA_P256_SHA256_ASN1_SIGNING};
use std::fs;
use std::path::PathBuf;
struct TestSigner {
    key: EcdsaKeyPair,
    credential: HumanApprovalCredential,
}

impl TestSigner {
    fn generate() -> Self {
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &rng)
            .expect("test P-256 key");
        let key = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &rng)
            .expect("parse test key");
        let public_key: [u8; 65] = key
            .public_key()
            .as_ref()
            .try_into()
            .expect("uncompressed P-256 key");
        let credential = HumanApprovalCredential::from_public_key(public_key).expect("credential");
        Self { key, credential }
    }

    fn sign(&self, expected: ExpectedHumanApproval) -> Vec<u8> {
        let draft = HumanApprovalReceiptDraft::new(expected, ApprovalDecision::Approve);
        let signature = self
            .key
            .sign(&SystemRandom::new(), &draft.canonical_bytes())
            .expect("ES256 signature");
        draft
            .with_signature(signature.as_ref().to_vec())
            .expect("DER receipt")
            .canonical_bytes()
    }
}

struct Fixture {
    root: PathBuf,
    source: PathBuf,
    storage: AttachmentStorage,
    history: ProvisionedAttachment,
}
impl Fixture {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "mesh-attached-approval-{name}-{}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let source = root.join("project");
        fs::create_dir(&source).unwrap();
        let metadata = root.join("metadata");
        fs::create_dir(&metadata).unwrap();
        let storage = AttachmentStorage::open(&metadata).unwrap();
        let history = storage.provision(&source).unwrap();
        Self {
            root,
            source,
            storage,
            history,
        }
    }
    fn save(&self, text: &str) -> String {
        fs::write(self.source.join("work.txt"), text).unwrap();
        self.capture()
    }
    fn capture(&self) -> String {
        use ed25519_dalek::{Signer as _, SigningKey};
        let key = SigningKey::from_bytes(&[67; 32]);
        let input = self
            .history
            .project()
            .capture_inputs(ObservationLimits::default())
            .unwrap();
        self.history
            .project()
            .save_capture(
                self.history.metadata_path(),
                &input,
                mesh_types::PublicKey::from_bytes(key.verifying_key().to_bytes()),
                |payload| {
                    Ok::<_, &'static str>(mesh_types::Signature::from_bytes(
                        key.sign(payload.as_bytes()).to_bytes(),
                    ))
                },
            )
            .unwrap()
            .operation()
            .to_string()
    }
    fn git(&self, args: &[&str]) -> Vec<u8> {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&self.source)
            .args(args)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .output()
            .unwrap();
        assert!(output.status.success());
        output.stdout
    }

    fn journal(&self) -> Vec<u8> {
        fs::read(self.history.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap()
    }
    fn request(&self, target: &str, trust: &TrustedReviewers) -> String {
        let card = self
            .history
            .request_review_with_trusted_reviewers(
                target,
                mesh_types::PublicKey::from_bytes([3; 32]),
                trust,
            )
            .unwrap();
        card.get("bundle")
            .and_then(Json::as_text)
            .unwrap()
            .to_owned()
    }
    fn receipt(
        &self,
        signer: &TestSigner,
        trust: &TrustedReviewers,
        bundle: &str,
        target: &str,
        challenge: u8,
    ) -> Vec<u8> {
        let preview = self
            .history
            .approval_preview(bundle, target, trust)
            .unwrap();
        signer.sign(ExpectedHumanApproval::new(
            preview.context().clone(),
            signer.credential.clone(),
            [challenge; 32],
        ))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn group_stops_after_mid_apply_change_and_retains_every_member() {
    let f = Fixture::new("group-mid-apply");
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    for name in ["a.txt", "b.txt"] {
        fs::write(f.source.join(name), "base").unwrap();
    }
    let first = f.save("base");
    let first_bundle = f.request(&first, &trust);
    f.history
        .approve_review(
            &first_bundle,
            &first,
            &f.receipt(&signer, &trust, &first_bundle, &first, 1),
            &trust,
        )
        .unwrap();
    for name in ["a.txt", "b.txt"] {
        fs::write(f.source.join(name), "accepted").unwrap();
    }
    let target = f.save("accepted");
    let bundle = f.request(&target, &trust);
    f.history
        .approve_review(
            &bundle,
            &target,
            &f.receipt(&signer, &trust, &bundle, &target, 2),
            &trust,
        )
        .unwrap();
    for name in ["a.txt", "b.txt", "work.txt"] {
        fs::write(f.source.join(name), "base").unwrap();
    }
    f.git(&["init", "--quiet"]);
    let journal = f.journal();
    let root = f.history.file_recovery_root(true).unwrap().unwrap();
    let group = f
        .history
        .prepare_main_integration(
            &bundle,
            &target,
            &root,
            &trust,
            ObservationLimits::default(),
        )
        .unwrap();
    let group_path = group.recovery_path().to_owned();
    let member_paths: Vec<_> = group
        .files()
        .map(|file| file.recovery_path().to_owned())
        .collect();
    let outcome = group
        .apply_with_hook(&trust, |index| {
            if index == 1 {
                fs::write(f.source.join("b.txt"), "concurrent user work").unwrap();
            }
        })
        .unwrap();
    assert_eq!(
        outcome.get("status"),
        Some(&Json::text("reconciliation-required"))
    );
    let members = outcome.get("members").unwrap().as_array().unwrap();
    assert_eq!(
        members[0].get("status"),
        Some(&Json::text("applied-observed"))
    );
    assert_eq!(
        members[1].get("status"),
        Some(&Json::text("reconciliation-required"))
    );
    assert_eq!(members[2].get("status"), Some(&Json::text("not-attempted")));
    assert_eq!(fs::read(f.source.join("a.txt")).unwrap(), b"accepted");
    assert_eq!(
        fs::read(f.source.join("b.txt")).unwrap(),
        b"concurrent user work"
    );
    assert_eq!(fs::read(f.source.join("work.txt")).unwrap(), b"base");
    assert_eq!(fs::read(member_paths[0].join("exchange")).unwrap(), b"base");
    assert_eq!(
        fs::read(member_paths[1].join("exchange")).unwrap(),
        b"accepted"
    );
    assert_eq!(
        fs::read(member_paths[2].join("exchange")).unwrap(),
        b"accepted"
    );
    assert!(group_path.join("attempt-0000.json").exists());
    assert!(group_path.join("attempt-0001.json").exists());
    assert!(!group_path.join("attempt-0002.json").exists());
    let reopened = f.storage.reopen(f.history.id()).unwrap();
    let recovered = reopened
        .inspect_main_integration_group(
            &root,
            group_path.file_name().unwrap().to_str().unwrap(),
            &trust,
            ObservationLimits::default(),
        )
        .unwrap();
    assert_eq!(
        recovered.get("members").unwrap().as_array().unwrap().len(),
        3
    );
    assert_eq!(f.journal(), journal);
    assert_eq!(
        fs::read(f.source.join("b.txt")).unwrap(),
        b"concurrent user work"
    );
}

#[test]
fn full_project_capture_count_stays_constant_as_replacement_group_grows() {
    use super::super::observation::CAPTURE_COUNT;
    for count in [2, 24] {
        let f = Fixture::new(&format!("group-capture-count-{count}"));
        let signer = TestSigner::generate();
        let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
        for index in 0..count {
            fs::write(f.source.join(format!("file-{index:02}")), "base").unwrap();
        }
        let first = f.capture();
        let first_bundle = f.request(&first, &trust);
        f.history
            .approve_review(
                &first_bundle,
                &first,
                &f.receipt(&signer, &trust, &first_bundle, &first, 1),
                &trust,
            )
            .unwrap();
        for index in 0..count {
            fs::write(f.source.join(format!("file-{index:02}")), "accepted").unwrap();
        }
        let target = f.capture();
        let bundle = f.request(&target, &trust);
        f.history
            .approve_review(
                &bundle,
                &target,
                &f.receipt(&signer, &trust, &bundle, &target, 2),
                &trust,
            )
            .unwrap();
        for index in 0..count {
            fs::write(f.source.join(format!("file-{index:02}")), "base").unwrap();
        }
        // Unrelated content must not be hashed once for each changed member.
        fs::write(f.source.join("unrelated.bin"), vec![0x3a; 1024 * 1024]).unwrap();
        let recovery = f.history.file_recovery_root(true).unwrap().unwrap();
        CAPTURE_COUNT.with(|counter| counter.set(0));
        let prepared = f
            .history
            .prepare_main_integration(
                &bundle,
                &target,
                &recovery,
                &trust,
                ObservationLimits::default(),
            )
            .unwrap();
        assert!(CAPTURE_COUNT.with(|counter| counter.get()) <= 3);
        assert_eq!(prepared.files().count(), count);
        CAPTURE_COUNT.with(|counter| counter.set(0));
        assert_eq!(
            prepared.apply(&trust).unwrap().get("status"),
            Some(&Json::text("applied-observed"))
        );
        assert!(CAPTURE_COUNT.with(|counter| counter.get()) <= 2);
        for index in 0..count {
            assert_eq!(
                fs::read(f.source.join(format!("file-{index:02}"))).unwrap(),
                b"accepted"
            );
        }
        assert_eq!(
            fs::read(f.source.join("unrelated.bin")).unwrap(),
            vec![0x3a; 1024 * 1024]
        );
    }
}

#[test]
fn changed_ignore_rules_between_group_members_stop_further_exchanges() {
    let f = Fixture::new("group-policy-change");
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    fs::write(f.source.join("a.txt"), "base").unwrap();
    let first = f.save("base");
    let first_bundle = f.request(&first, &trust);
    f.history
        .approve_review(
            &first_bundle,
            &first,
            &f.receipt(&signer, &trust, &first_bundle, &first, 1),
            &trust,
        )
        .unwrap();
    fs::write(f.source.join("a.txt"), "accepted").unwrap();
    let target = f.save("accepted");
    let bundle = f.request(&target, &trust);
    f.history
        .approve_review(
            &bundle,
            &target,
            &f.receipt(&signer, &trust, &bundle, &target, 2),
            &trust,
        )
        .unwrap();
    fs::write(f.source.join("a.txt"), "base").unwrap();
    fs::write(f.source.join("work.txt"), "base").unwrap();
    let recovery = f.history.file_recovery_root(true).unwrap().unwrap();
    let prepared = f
        .history
        .prepare_main_integration(
            &bundle,
            &target,
            &recovery,
            &trust,
            ObservationLimits::default(),
        )
        .unwrap();
    let result = prepared
        .apply_with_hook(&trust, |index| {
            if index == 1 {
                fs::write(f.source.join(".meshignore"), "work.txt\n").unwrap();
            }
        })
        .unwrap();
    assert_eq!(
        result.get("status"),
        Some(&Json::text("reconciliation-required"))
    );
    assert_eq!(fs::read(f.source.join("a.txt")).unwrap(), b"accepted");
    assert_eq!(fs::read(f.source.join("work.txt")).unwrap(), b"base");
}
