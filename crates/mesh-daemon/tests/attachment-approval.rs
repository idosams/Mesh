//! Attachment approval advances only external Mesh main while ordinary work continues.
#![cfg(unix)]
use mesh_approval::{
    ApprovalDecision, ExpectedHumanApproval, HumanApprovalCredential, HumanApprovalReceiptDraft,
};
use mesh_daemon::project_attachment::{
    AttachmentStorage, ObservationLimits, ProvisionedAttachment,
};
use mesh_daemon::{ipc::Json, TrustedReviewers};
use ring::rand::SystemRandom;
use ring::signature::{EcdsaKeyPair, KeyPair as _, ECDSA_P256_SHA256_ASN1_SIGNING};
use std::fs;
use std::io::Write as _;
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
        use ed25519_dalek::{Signer as _, SigningKey};
        fs::write(self.source.join("work.txt"), text).unwrap();
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
        fs::read(
            self.history
                .metadata_path()
                .join(mesh_daemon::RECORD_FILE_NAME),
        )
        .unwrap()
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
fn attached_main_accepts_exact_saved_content_while_capture_and_editor_continue() {
    let f = Fixture::new("roundtrip");
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    f.git(&["init", "--quiet"]);
    let first = f.save("first reviewed content");
    f.git(&["add", "work.txt"]);
    let git_index = fs::read(f.source.join(".git/index")).unwrap();
    let git_head = fs::read(f.source.join(".git/HEAD")).unwrap();
    let bundle = f.request(&first, &trust);
    let receipt = f.receipt(&signer, &trust, &bundle, &first, 1);
    let different_ceremony = f.receipt(&signer, &trust, &bundle, &first, 9);
    let second = f.save("newer private content");
    let pending_bundle = f.request(&second, &trust);
    let stale = f.receipt(&signer, &trust, &pending_bundle, &second, 2);
    let mut editor = fs::OpenOptions::new()
        .append(true)
        .open(f.source.join("work.txt"))
        .unwrap();
    editor.write_all(b" + unsaved live edits").unwrap();
    let git_status = f.git(&["status", "--porcelain=v1"]);
    let main = f
        .history
        .approve_review(&bundle, &first, &receipt, &trust)
        .unwrap();
    assert_eq!(f.history.main_version(&trust).unwrap(), Some(main));
    let after = f.journal();
    assert_eq!(
        f.history
            .approve_review(&bundle, &first, &receipt, &trust)
            .unwrap(),
        main
    );
    assert_eq!(f.journal(), after, "retry appended duplicate authority");
    assert!(f
        .history
        .approve_review(&bundle, &first, &different_ceremony, &trust)
        .is_err());
    assert_eq!(
        f.journal(),
        after,
        "a different ceremony changed retained authority"
    );
    assert!(f
        .history
        .approve_review(&pending_bundle, &second, &stale, &trust)
        .is_err());
    assert!(f
        .history
        .approval_preview(&pending_bundle, &second, &trust)
        .is_err());
    assert_eq!(f.journal(), after, "stale approval changed history");
    assert_eq!(
        fs::read_to_string(f.source.join("work.txt")).unwrap(),
        "newer private content + unsaved live edits"
    );
    assert_eq!(f.git(&["status", "--porcelain=v1"]), git_status);
    assert_eq!(fs::read(f.source.join(".git/index")).unwrap(), git_index);
    assert_eq!(fs::read(f.source.join(".git/HEAD")).unwrap(), git_head);
    editor.write_all(b" + editor still open").unwrap();
    assert!(!f.source.join(".mesh").exists());
    assert!(f
        .history
        .main_version(&TrustedReviewers::default())
        .is_err());
    assert!(
        f.history
            .request_review(&second, mesh_types::PublicKey::from_bytes([3; 32]))
            .is_err(),
        "missing native trust must not silently review against genesis"
    );

    let rebased = f.request(&second, &trust);
    assert_ne!(rebased, pending_bundle);
    let reused = f.receipt(&signer, &trust, &rebased, &second, 1);
    let before = f.journal();
    assert!(f
        .history
        .approve_review(&rebased, &second, &reused, &trust)
        .is_err());
    assert_eq!(
        f.journal(),
        before,
        "challenge reuse must refuse before append"
    );
    let second_receipt = f.receipt(&signer, &trust, &rebased, &second, 3);
    let next = f
        .history
        .approve_review(&rebased, &second, &second_receipt, &trust)
        .unwrap();
    assert_ne!(next, main);
    let reopened = f.storage.reopen(f.history.id()).unwrap();
    assert_eq!(reopened.main_version(&trust).unwrap(), Some(next));
    assert_eq!(
        reopened
            .review_with_trusted_reviewers(&pending_bundle, &second, &trust)
            .unwrap()
            .get("complete"),
        Some(&Json::Bool(true))
    );
    assert_eq!(
        reopened
            .approve_review(&rebased, &second, &second_receipt, &trust)
            .unwrap(),
        next
    );
    assert!(reopened
        .approve_review(&bundle, &first, &receipt, &trust)
        .is_err());
    let third = f.save("continued after publication");
    assert_ne!(third, second);
    assert_eq!(f.history.main_version(&trust).unwrap(), Some(next));
}

#[test]
fn attached_approval_refuses_foreign_untrusted_malformed_and_replaced_inputs_without_append() {
    let f = Fixture::new("refusals");
    let other = Fixture::new("foreign");
    let signer = TestSigner::generate();
    let stranger = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    let target = f.save("exact content");
    let foreign_target = other.save("exact content");
    let bundle = f.request(&target, &trust);
    let foreign_bundle = other.request(&foreign_target, &trust);
    let receipt = f.receipt(&signer, &trust, &bundle, &target, 1);
    let foreign = other.receipt(&signer, &trust, &foreign_bundle, &foreign_target, 2);
    let bad = f.receipt(&stranger, &trust, &bundle, &target, 3);
    let zero = f.receipt(&signer, &trust, &bundle, &target, 0);
    let before = f.journal();
    for bytes in [&foreign[..], &bad[..], &zero[..], b"partial".as_slice()] {
        assert!(f
            .history
            .approve_review(&bundle, &target, bytes, &trust)
            .is_err());
        assert_eq!(f.journal(), before);
    }
    assert!(f
        .history
        .approve_review(&bundle, &foreign_target, &receipt, &trust)
        .is_err());
    assert!(f
        .history
        .approve_review(&bundle, &target, &receipt, &TrustedReviewers::default())
        .is_err());
    assert_eq!(f.journal(), before);
    fs::rename(&f.source, f.root.join("original")).unwrap();
    fs::create_dir(&f.source).unwrap();
    assert!(f
        .history
        .approve_review(&bundle, &target, &receipt, &trust)
        .is_err());
    assert_eq!(f.journal(), before);
}

#[test]
fn concurrent_attachment_approvals_serialize_on_the_same_verified_main() {
    let f = Fixture::new("concurrent");
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    let first = f.save("first");
    let first_bundle = f.request(&first, &trust);
    let first_receipt = f.receipt(&signer, &trust, &first_bundle, &first, 1);
    let second = f.save("second");
    let second_bundle = f.request(&second, &trust);
    let second_receipt = f.receipt(&signer, &trust, &second_bundle, &second, 2);
    let barrier = std::sync::Barrier::new(2);
    let (left, right) = std::thread::scope(|scope| {
        let left = scope.spawn(|| {
            barrier.wait();
            f.history
                .approve_review(&first_bundle, &first, &first_receipt, &trust)
        });
        let right = scope.spawn(|| {
            barrier.wait();
            f.history
                .approve_review(&second_bundle, &second, &second_receipt, &trust)
        });
        (left.join().unwrap(), right.join().unwrap())
    });
    assert_ne!(
        left.is_ok(),
        right.is_ok(),
        "exactly one competing base may advance: {left:?}, {right:?}"
    );
    let winner = left.or(right).unwrap();
    assert_eq!(f.history.main_version(&trust).unwrap(), Some(winner));
    let scan = mesh_store::scan_journal(&f.journal()).unwrap();
    assert_eq!(
        scan.records()
            .iter()
            .filter(|record| matches!(record, mesh_store::StoredRecord::Approval(_)))
            .count(),
        1
    );

    // Contradictory retained authority is unavailable, never a fresh genesis.
    let approval = scan
        .records()
        .iter()
        .find(|record| matches!(record, mesh_store::StoredRecord::Approval(_)))
        .unwrap();
    let journal = f
        .history
        .metadata_path()
        .join(mesh_daemon::RECORD_FILE_NAME);
    let mut file = fs::OpenOptions::new().append(true).open(&journal).unwrap();
    file.write_all(&mesh_store::frame_record(approval)).unwrap();
    file.sync_all().unwrap();
    let poisoned = f.journal();
    assert!(f.history.main_version(&trust).is_err());
    assert!(f
        .history
        .request_review_with_trusted_reviewers(
            &second,
            mesh_types::PublicKey::from_bytes([3; 32]),
            &trust
        )
        .is_err());
    assert!(f
        .history
        .approve_review(&first_bundle, &first, &first_receipt, &trust)
        .is_err());
    assert!(f
        .history
        .approve_review(&second_bundle, &second, &second_receipt, &trust)
        .is_err());
    assert_eq!(f.journal(), poisoned);
}

#[test]
fn retained_attachment_approval_handle_refuses_replaced_private_store() {
    let f = Fixture::new("replaced-store");
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    let target = f.save("reviewed");
    let bundle = f.request(&target, &trust);
    let receipt = f.receipt(&signer, &trust, &bundle, &target, 1);
    let before = f.journal();
    let moved = f.root.join("retained-store");
    fs::rename(f.history.metadata_path(), &moved).unwrap();
    fs::create_dir(f.history.metadata_path()).unwrap();
    assert!(f
        .history
        .approve_review(&bundle, &target, &receipt, &trust)
        .is_err());
    assert_eq!(
        fs::read(moved.join(mesh_daemon::RECORD_FILE_NAME)).unwrap(),
        before
    );
    assert_eq!(fs::read_dir(f.history.metadata_path()).unwrap().count(), 0);
    assert_eq!(
        fs::read_to_string(f.source.join("work.txt")).unwrap(),
        "reviewed"
    );
}
