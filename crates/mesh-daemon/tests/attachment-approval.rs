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

#[test]
fn accepted_main_resolves_its_saved_review_after_restart_and_outside_the_queue() {
    let f = Fixture::new("main-lookup");
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    let mut requests = Vec::new();
    for index in 0..33 {
        let target = f.save(&format!("saved content {index}"));
        let bundle = f.request(&target, &trust);
        requests.push((bundle, target));
    }
    assert_eq!(f.history.accepted_main(&trust).unwrap(), Json::Null);
    // The overview is sorted by bundle identity and bounded to 32. Approve the omitted request.
    requests.sort();
    let (bundle, target) = requests.pop().unwrap();
    let receipt = f.receipt(&signer, &trust, &bundle, &target, 1);
    let head = f
        .history
        .approve_review(&bundle, &target, &receipt, &trust)
        .unwrap();
    let queue = f.history.reviews_with_trusted_reviewers(&trust).unwrap();
    let Json::Array(cards) = queue.get("reviews").unwrap() else {
        panic!("review queue")
    };
    assert_eq!(cards.len(), 32);
    assert!(cards
        .iter()
        .all(|card| card.get("bundle") != Some(&Json::text(&bundle))));
    let reopened = f.storage.reopen(f.history.id()).unwrap();
    let main = reopened.accepted_main(&trust).unwrap();
    assert_eq!(main.get("head"), Some(&Json::text(head.to_string())));
    assert_eq!(main.get("bundle"), Some(&Json::text(&bundle)));
    assert_eq!(main.get("target"), Some(&Json::text(&target)));
    assert!(reopened
        .accepted_main(&TrustedReviewers::default())
        .is_err());
    let card = reopened
        .review_with_trusted_reviewers(&bundle, &target, &trust)
        .unwrap();
    assert_eq!(card.get("complete"), Some(&Json::Bool(true)));
}

fn accept(
    f: &Fixture,
    signer: &TestSigner,
    trust: &TrustedReviewers,
    target: &str,
    challenge: u8,
) -> String {
    let bundle = f.request(target, trust);
    let receipt = f.receipt(signer, trust, &bundle, target, challenge);
    f.history
        .approve_review(&bundle, target, &receipt, trust)
        .unwrap();
    bundle
}
fn status_at<'a>(preview: &'a Json, path: &str) -> &'a str {
    let Json::Array(entries) = preview.get("entries").unwrap() else {
        panic!("preview entries")
    };
    entries
        .iter()
        .find(|entry| entry.get("path") == Some(&Json::text(path)))
        .and_then(|entry| entry.get("status"))
        .and_then(Json::as_text)
        .expect("path status")
}

#[test]
fn integration_preview_preserves_divergence_and_blocks_destructive_directory_dependencies() {
    let f = Fixture::new("integration-divergence");
    f.git(&["init", "--quiet"]);
    fs::write(f.source.join(".gitignore"), "secret.tmp\n").unwrap();
    fs::write(f.source.join("untouched.txt"), "base").unwrap();
    fs::write(f.source.join("gone.txt"), "base").unwrap();
    fs::create_dir_all(f.source.join("outer/dir")).unwrap();
    fs::write(f.source.join("outer/dir/child"), "base").unwrap();
    fs::create_dir(f.source.join("stable")).unwrap();
    fs::write(f.source.join("stable/keep"), "base").unwrap();
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    let first = f.save("base");
    accept(&f, &signer, &trust, &first, 1);
    f.git(&["add", "."]);
    fs::remove_file(f.source.join("gone.txt")).unwrap();
    fs::remove_dir_all(f.source.join("outer")).unwrap();
    fs::write(f.source.join("new.txt"), "approved addition").unwrap();
    fs::write(f.source.join("stable/new"), "approved addition").unwrap();
    fs::create_dir(f.source.join("newdir")).unwrap();
    fs::write(f.source.join("newdir/new"), "approved addition").unwrap();
    let target = f.save("approved change");
    let bundle = accept(&f, &signer, &trust, &target, 2);
    let retained = f.history.inspect_text(&first, "outer/dir/child").unwrap();
    assert_eq!(retained.get("text"), Some(&Json::text("base")));
    assert!(f.history.inspect_text(&target, "outer/dir/child").is_err());
    let card = f
        .history
        .review_with_trusted_reviewers(&bundle, &target, &trust)
        .unwrap();
    let Json::Array(changes) = card.get("changes").unwrap() else {
        panic!("review changes")
    };
    assert!(
        changes.iter().any(
            |change| change.get("before") == Some(&Json::text("outer/dir/child"))
                && change.get("after") == Some(&Json::Null)
        ),
        "unlinked file must be reviewed as a relative-path removal: {}",
        card.encode()
    );

    fs::write(f.source.join("work.txt"), "live divergent work").unwrap();
    fs::write(f.source.join("untouched.txt"), "live unrelated work").unwrap();
    fs::write(f.source.join("gone.txt"), "base").unwrap();
    fs::create_dir_all(f.source.join("outer/dir")).unwrap();
    fs::write(f.source.join("outer/dir/child"), "base").unwrap();
    fs::write(
        f.source.join("outer/dir/secret.tmp"),
        "ignored private work",
    )
    .unwrap();
    fs::remove_dir_all(f.source.join("stable")).unwrap();
    fs::write(f.source.join("stable"), "user replaced directory").unwrap();
    fs::remove_dir_all(f.source.join("newdir")).unwrap();
    let index = fs::read(f.source.join(".git/index")).unwrap();
    let git_head = fs::read(f.source.join(".git/HEAD")).unwrap();
    let git_status = f.git(&["status", "--porcelain=v1"]);
    let journal = f.journal();
    let preview = f
        .history
        .preview_main_integration(&bundle, &target, &trust, ObservationLimits::default())
        .unwrap();
    assert_eq!(status_at(&preview, "work.txt"), "conflict");
    assert_eq!(status_at(&preview, "untouched.txt"), "preserve-current");
    assert_eq!(status_at(&preview, "gone.txt"), "matches-base");
    assert_eq!(status_at(&preview, "new.txt"), "already-present");
    assert_eq!(status_at(&preview, "outer"), "conflict");
    assert_eq!(status_at(&preview, "outer/dir"), "conflict");
    assert_eq!(status_at(&preview, "outer/dir/child"), "blocked");
    assert_eq!(status_at(&preview, "stable"), "preserve-current");
    assert_eq!(status_at(&preview, "stable/new"), "blocked");
    assert_eq!(status_at(&preview, "newdir"), "matches-base");
    assert_eq!(status_at(&preview, "newdir/new"), "matches-base");
    assert_eq!(preview.get("write_authority"), Some(&Json::Bool(false)));
    assert_eq!(preview.get("atomic_snapshot"), Some(&Json::Bool(false)));
    assert!(!preview.encode().contains("ignored private work"));
    assert_eq!(f.journal(), journal);
    assert_eq!(f.git(&["status", "--porcelain=v1"]), git_status);
    assert_eq!(fs::read(f.source.join(".git/index")).unwrap(), index);
    assert_eq!(fs::read(f.source.join(".git/HEAD")).unwrap(), git_head);
    assert_eq!(
        fs::read_to_string(f.source.join("work.txt")).unwrap(),
        "live divergent work"
    );
    assert_eq!(
        fs::read_to_string(f.source.join("outer/dir/secret.tmp")).unwrap(),
        "ignored private work"
    );
    fs::write(f.source.join("work.txt"), "base").unwrap();
    let refreshed = f
        .history
        .preview_main_integration(&bundle, &target, &trust, ObservationLimits::default())
        .unwrap();
    assert_ne!(
        preview.get("observed_digest"),
        refreshed.get("observed_digest")
    );
    assert_eq!(status_at(&refreshed, "work.txt"), "matches-base");
    assert_eq!(
        status_at(&preview, "work.txt"),
        "conflict",
        "old observation must stay fixed"
    );
    assert!(f
        .history
        .preview_main_integration(&bundle, &first, &trust, ObservationLimits::default())
        .is_err());
    assert!(f
        .history
        .preview_main_integration(
            &bundle,
            &target,
            &TrustedReviewers::default(),
            ObservationLimits::default()
        )
        .is_err());
    fs::write(f.source.join(".gitignore"), "changed-policy\n").unwrap();
    assert!(f
        .history
        .preview_main_integration(&bundle, &target, &trust, ObservationLimits::default())
        .is_err());
    assert_eq!(f.journal(), journal);
}

#[test]
fn integration_preview_bounds_output_and_refuses_incomplete_or_unsafe_observations() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new("integration-bounds");
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    let first = f.save("base");
    let initial_bundle = accept(&f, &signer, &trust, &first, 1);
    for index in 0..205 {
        fs::write(f.source.join(format!("item-{index:03}")), "addition").unwrap();
    }
    let target = f.capture();
    let bundle = accept(&f, &signer, &trust, &target, 2);
    for index in 0..205 {
        fs::remove_file(f.source.join(format!("item-{index:03}"))).unwrap();
    }
    let preview = f
        .history
        .preview_main_integration(&bundle, &target, &trust, ObservationLimits::default())
        .unwrap();
    let Json::Array(entries) = preview.get("entries").unwrap() else {
        panic!("entries")
    };
    assert_eq!(entries.len(), 200);
    assert_eq!(preview.get("not_listed"), Some(&Json::Number(5)));
    assert_eq!(preview.get("matches_base"), Some(&Json::Number(205)));
    assert_eq!(preview.get("conflicts"), Some(&Json::Number(0)));
    let journal = f.journal();
    assert!(f
        .history
        .preview_main_integration(
            &initial_bundle,
            &first,
            &trust,
            ObservationLimits::default()
        )
        .is_err());
    let small = ObservationLimits {
        bytes: 1,
        ..ObservationLimits::default()
    };
    assert!(f
        .history
        .preview_main_integration(&bundle, &target, &trust, small)
        .is_err());
    symlink(f.root.join("outside"), f.source.join("link")).unwrap();
    assert!(f
        .history
        .preview_main_integration(&bundle, &target, &trust, ObservationLimits::default())
        .is_err());
    assert_eq!(f.journal(), journal);
    fs::remove_file(f.source.join("link")).unwrap();
    fs::rename(&f.source, f.root.join("moved-source")).unwrap();
    fs::create_dir(&f.source).unwrap();
    assert!(f
        .history
        .preview_main_integration(&bundle, &target, &trust, ObservationLimits::default())
        .is_err());
    assert_eq!(f.journal(), journal);
}
