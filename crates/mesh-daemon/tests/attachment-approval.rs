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

fn recovery_root(f: &Fixture) -> PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    let root = f.root.join("recovery");
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    root
}

#[test]
fn approved_file_integration_retains_late_editor_writes_and_exact_receipts() {
    use std::os::unix::fs::MetadataExt as _;
    let f = Fixture::new("retained-file");
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    let first = f.save("first");
    accept(&f, &signer, &trust, &first, 1);
    let second = f.save("approved second");
    let bundle = accept(&f, &signer, &trust, &second, 2);
    fs::write(f.source.join("work.txt"), b"first").unwrap();
    fs::write(f.source.join("unrelated.txt"), b"current work").unwrap();
    f.git(&["init", "--quiet"]);
    f.git(&["add", "."]);
    let index = fs::read(f.source.join(".git/index")).unwrap();
    let head = fs::read(f.source.join(".git/HEAD")).unwrap();
    let journal = f.journal();
    let root = recovery_root(&f);
    let mut editor = fs::OpenOptions::new()
        .append(true)
        .open(f.source.join("work.txt"))
        .unwrap();
    let original_inode = editor.metadata().unwrap().ino();
    let prepared = f
        .history
        .prepare_main_file_integration(
            &bundle,
            &second,
            "work.txt",
            &root,
            &trust,
            ObservationLimits::default(),
        )
        .unwrap();
    let recovery = prepared.recovery_path().to_owned();
    let proposal = prepared.proposal().clone();
    assert_eq!(proposal.get("automatic_replay"), Some(&Json::Bool(false)));
    assert_eq!(proposal.get("bundle"), Some(&Json::text(&bundle)));
    assert_eq!(fs::read(f.source.join("work.txt")).unwrap(), b"first");
    assert_eq!(
        fs::read(recovery.join("exchange")).unwrap(),
        b"approved second"
    );
    assert_eq!(
        fs::read_to_string(recovery.join("prepared.json")).unwrap(),
        proposal.encode()
    );
    let result = prepared.apply(&trust).unwrap();
    assert_eq!(result.get("status"), Some(&Json::text("applied-observed")));
    editor.write_all(b" + late editor work").unwrap();
    editor.sync_all().unwrap();
    assert_eq!(
        fs::metadata(recovery.join("exchange")).unwrap().ino(),
        original_inode
    );
    assert_eq!(
        fs::read(recovery.join("exchange")).unwrap(),
        b"first + late editor work"
    );
    assert_eq!(
        fs::read(f.source.join("work.txt")).unwrap(),
        b"approved second"
    );
    assert_eq!(
        fs::read(f.source.join("unrelated.txt")).unwrap(),
        b"current work"
    );
    assert_eq!(fs::read(f.source.join(".git/index")).unwrap(), index);
    assert_eq!(fs::read(f.source.join(".git/HEAD")).unwrap(), head);
    assert_eq!(f.journal(), journal);
    assert_eq!(
        fs::read_to_string(recovery.join("observed.json")).unwrap(),
        result.encode()
    );
    // Restart can recover both durable receipts and the displaced inode without replaying apply.
    let reopened = f.storage.reopen(f.history.id()).unwrap();
    assert_eq!(
        reopened.accepted_main(&trust).unwrap(),
        f.history.accepted_main(&trust).unwrap()
    );
    assert_eq!(
        fs::read(recovery.join("exchange")).unwrap(),
        b"first + late editor work"
    );
}

#[test]
fn file_integration_revalidates_trust_main_source_exclusions_and_preparation() {
    let f = Fixture::new("retained-refusal");
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    let first = f.save("base");
    accept(&f, &signer, &trust, &first, 1);
    let second = f.save("accepted");
    let bundle = accept(&f, &signer, &trust, &second, 2);
    let root = recovery_root(&f);
    let prepare = || {
        f.history.prepare_main_file_integration(
            &bundle,
            &second,
            "work.txt",
            &root,
            &trust,
            ObservationLimits::default(),
        )
    };
    assert!(prepare().is_err()); // Already-present content is not a base-matching replacement.
    fs::write(f.source.join("work.txt"), b"base").unwrap();
    assert!(f
        .history
        .prepare_main_file_integration(
            &bundle,
            &second,
            "../work.txt",
            &root,
            &trust,
            ObservationLimits::default()
        )
        .is_err());
    assert!(f
        .history
        .prepare_main_file_integration(
            &bundle,
            &second,
            "work.txt",
            &f.source,
            &trust,
            ObservationLimits::default()
        )
        .is_err());
    let prepared = prepare().unwrap();
    assert!(prepared.apply(&TrustedReviewers::default()).is_err());
    assert_eq!(fs::read(f.source.join("work.txt")).unwrap(), b"base");
    let prepared = prepare().unwrap();
    fs::write(f.source.join("work.txt"), b"new live edits").unwrap();
    assert!(prepared.apply(&trust).is_err());
    assert_eq!(
        fs::read(f.source.join("work.txt")).unwrap(),
        b"new live edits"
    );
    fs::write(f.source.join("work.txt"), b"base").unwrap();
    let prepared = prepare().unwrap();
    fs::write(prepared.recovery_path().join("prepared.json"), b"{}").unwrap();
    assert!(prepared.apply(&trust).is_err());
    let prepared = prepare().unwrap();
    fs::write(f.source.join(".meshignore"), b"work.txt\n").unwrap();
    assert!(prepared.apply(&trust).is_err());
    fs::remove_file(f.source.join(".meshignore")).unwrap();
    let prepared = prepare().unwrap();
    let third = f.save("new main");
    accept(&f, &signer, &trust, &third, 3);
    fs::write(f.source.join("work.txt"), b"base").unwrap();
    assert!(prepared.apply(&trust).is_err());
    assert_eq!(fs::read(f.source.join("work.txt")).unwrap(), b"base");
}

fn recovery_entry(value: &Json) -> &Json {
    value
        .get("entries")
        .and_then(Json::as_array)
        .unwrap()
        .first()
        .unwrap()
}
fn recovery_status(value: &Json) -> &str {
    recovery_entry(value)
        .get("status")
        .and_then(Json::as_text)
        .unwrap()
}
fn replace_json(value: &Json, field: &str, replacement: Json) -> Json {
    let mut value = value.clone();
    let Json::Object(fields) = &mut value else {
        panic!("object")
    };
    fields.iter_mut().find(|(key, _)| key == field).unwrap().1 = replacement;
    value
}

#[test]
fn recovery_inspection_tracks_restart_arrangements_and_late_work_after_main_advances() {
    let f = Fixture::new("recovery-arrangements");
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    let first = f.save("base");
    accept(&f, &signer, &trust, &first, 1);
    let target = f.save("approved");
    let bundle = accept(&f, &signer, &trust, &target, 2);
    fs::write(f.source.join("work.txt"), b"base").unwrap();
    let root = recovery_root(&f);
    let mut editor = fs::OpenOptions::new()
        .append(true)
        .open(f.source.join("work.txt"))
        .unwrap();
    let prepared = f
        .history
        .prepare_main_file_integration(
            &bundle,
            &target,
            "work.txt",
            &root,
            &trust,
            ObservationLimits::default(),
        )
        .unwrap();
    let path = prepared.recovery_path().to_owned();
    let id = path.file_name().unwrap().to_str().unwrap();
    let inspect = || {
        f.storage
            .reopen(f.history.id())
            .unwrap()
            .inspect_integration_recovery(&root, Some(id), &trust, ObservationLimits::default())
            .unwrap()
    };
    let journal = f.journal();
    assert_eq!(recovery_status(&inspect()), "prepared-arrangement");
    assert_eq!(f.journal(), journal);
    prepared.apply(&trust).unwrap();
    assert_eq!(recovery_status(&inspect()), "applied-arrangement");
    let outcome = fs::read(path.join("observed.json")).unwrap();
    fs::remove_file(path.join("observed.json")).unwrap();
    let lost_reply = inspect();
    assert_eq!(recovery_status(&lost_reply), "applied-arrangement");
    assert_eq!(
        recovery_entry(&lost_reply).get("attention_required"),
        Some(&Json::Bool(true))
    );
    assert_eq!(
        recovery_entry(&lost_reply)
            .get("details")
            .unwrap()
            .get("recorded_outcome"),
        Some(&Json::text("absent"))
    );
    fs::write(path.join("observed.json"), &outcome).unwrap();
    editor.write_all(b" + late editor work").unwrap();
    editor.sync_all().unwrap();
    assert_eq!(recovery_status(&inspect()), "changed-files");
    let third = f.save("new accepted main");
    accept(&f, &signer, &trust, &third, 3);
    let journal = f.journal();
    let historical = inspect();
    assert_eq!(recovery_status(&historical), "changed-files");
    assert_eq!(
        recovery_entry(&historical)
            .get("details")
            .unwrap()
            .get("is_current_main"),
        Some(&Json::Bool(false))
    );
    assert_eq!(
        fs::read(path.join("exchange")).unwrap(),
        b"base + late editor work"
    );
    assert_eq!(
        fs::read(f.source.join("work.txt")).unwrap(),
        b"new accepted main"
    );
    assert_eq!(f.journal(), journal);
    assert_eq!(fs::read(path.join("observed.json")).unwrap(), outcome);
    assert_eq!(historical.get("automatic_replay"), Some(&Json::Bool(false)));
    assert_eq!(
        recovery_entry(&historical).get("cleanup_authority"),
        Some(&Json::Bool(false))
    );
}

#[test]
fn recovery_inspection_refuses_tampering_and_untrusted_receipts_without_reading_claimed_files() {
    let f = Fixture::new("recovery-tampering");
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    let first = f.save("base");
    accept(&f, &signer, &trust, &first, 1);
    let target = f.save("approved");
    let bundle = accept(&f, &signer, &trust, &target, 2);
    fs::write(f.source.join("work.txt"), b"base").unwrap();
    let root = recovery_root(&f);
    let prepared = f
        .history
        .prepare_main_file_integration(
            &bundle,
            &target,
            "work.txt",
            &root,
            &trust,
            ObservationLimits::default(),
        )
        .unwrap();
    let path = prepared.recovery_path().to_owned();
    let id = path.file_name().unwrap().to_str().unwrap();
    let proposal = prepared.proposal().clone();
    drop(prepared);
    let inspect = |trust: &TrustedReviewers| {
        f.history
            .inspect_integration_recovery(&root, Some(id), trust, ObservationLimits::default())
            .unwrap()
    };
    assert_eq!(
        recovery_status(&inspect(&TrustedReviewers::default())),
        "unverified-history"
    );
    for (field, value, status) in [
        (
            "schema",
            Json::text("mesh.attachment-file-integration/v2"),
            "invalid-receipt",
        ),
        ("path", Json::text("../outside"), "invalid-receipt"),
        (
            "source_file",
            Json::text(format!(
                "{}:{}:{}",
                "0".repeat(16),
                "0".repeat(16),
                "é".repeat(17)
            )),
            "invalid-receipt",
        ),
        ("store_inode", Json::text("0".repeat(16)), "invalid-receipt"),
        (
            "source_digest",
            Json::text("f".repeat(64)),
            "unverified-history",
        ),
        (
            "installed_digest",
            Json::text("f".repeat(64)),
            "unverified-history",
        ),
        ("automatic_replay", Json::Bool(true), "invalid-receipt"),
    ] {
        fs::write(
            path.join("prepared.json"),
            replace_json(&proposal, field, value).encode(),
        )
        .unwrap();
        let report = inspect(&trust);
        assert_eq!(recovery_status(&report), status, "{field}");
        assert_eq!(
            report.get("live_content_budget_remaining"),
            Some(&Json::Number(ObservationLimits::default().bytes))
        );
    }
    let mut unknown = proposal.clone();
    let Json::Object(fields) = &mut unknown else {
        panic!("object")
    };
    fields.push(("extra".to_owned(), Json::Bool(true)));
    fs::write(path.join("prepared.json"), unknown.encode()).unwrap();
    assert_eq!(recovery_status(&inspect(&trust)), "invalid-receipt");
    fs::write(path.join("prepared.json"), proposal.encode()).unwrap();
    fs::write(path.join("observed.json"), "{").unwrap();
    assert_eq!(recovery_status(&inspect(&trust)), "invalid-outcome");
    fs::remove_file(path.join("observed.json")).unwrap();
    assert_eq!(recovery_status(&inspect(&trust)), "prepared-arrangement");
    assert_eq!(fs::read(f.source.join("work.txt")).unwrap(), b"base");
    assert_eq!(fs::read(path.join("exchange")).unwrap(), b"approved");
    // A byte-identical new source inode must not inherit the receipt's source identity.
    fs::rename(f.source.join("work.txt"), f.source.join("old.txt")).unwrap();
    fs::write(f.source.join("work.txt"), b"base").unwrap();
    assert_eq!(recovery_status(&inspect(&trust)), "identity-mismatch");
}

#[test]
fn recovery_catalog_bounds_work_and_preserves_unsafe_or_incomplete_entries() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new("recovery-bounds");
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    let first = f.save("base");
    accept(&f, &signer, &trust, &first, 1);
    let target = f.save("approved");
    let bundle = accept(&f, &signer, &trust, &target, 2);
    fs::write(f.source.join("work.txt"), b"base").unwrap();
    let root = recovery_root(&f);
    let prepared = f
        .history
        .prepare_main_file_integration(
            &bundle,
            &target,
            "work.txt",
            &root,
            &trust,
            ObservationLimits::default(),
        )
        .unwrap();
    let path = prepared.recovery_path().to_owned();
    let id = path.file_name().unwrap().to_str().unwrap();
    drop(prepared);
    for index in 0..34 {
        fs::create_dir(root.join(format!("integration-{index:032x}"))).unwrap();
    }
    let catalog = f
        .history
        .inspect_integration_recovery(&root, None, &trust, ObservationLimits::default())
        .unwrap();
    assert_eq!(
        catalog.get("entries").unwrap().as_array().unwrap().len(),
        32
    );
    assert_eq!(catalog.get("more"), Some(&Json::Bool(true)));
    let inspect = |limits| {
        f.history
            .inspect_integration_recovery(&root, Some(id), &trust, limits)
            .unwrap()
    };
    assert_eq!(
        recovery_status(&inspect(ObservationLimits::default())),
        "prepared-arrangement"
    );
    assert_eq!(
        recovery_status(&inspect(ObservationLimits {
            bytes: 1,
            ..ObservationLimits::default()
        })),
        "incomplete-observation"
    );
    fs::rename(path.join("exchange"), path.join("original-exchange")).unwrap();
    symlink(f.source.join("work.txt"), path.join("exchange")).unwrap();
    assert_eq!(
        recovery_status(&inspect(ObservationLimits::default())),
        "incomplete-observation"
    );
    assert!(fs::symlink_metadata(path.join("exchange"))
        .unwrap()
        .file_type()
        .is_symlink());
    fs::remove_file(path.join("exchange")).unwrap();
    assert!(std::process::Command::new("mkfifo")
        .arg(path.join("exchange"))
        .status()
        .unwrap()
        .success());
    assert_eq!(
        recovery_status(&inspect(ObservationLimits::default())),
        "incomplete-observation"
    );
    assert_eq!(
        fs::read(path.join("original-exchange")).unwrap(),
        b"approved"
    );
    assert_eq!(fs::read(f.source.join("work.txt")).unwrap(), b"base");
    assert!(f
        .history
        .inspect_integration_recovery(
            &root,
            Some("../outside"),
            &trust,
            ObservationLimits::default()
        )
        .is_err());
}

#[test]
fn retained_restoration_preserves_both_editor_streams_and_supports_a_new_undo_transaction() {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
    let f = Fixture::new("restore-retained");
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    let first = f.save("base");
    accept(&f, &signer, &trust, &first, 1);
    let target = f.save("approved");
    let bundle = accept(&f, &signer, &trust, &target, 2);
    fs::write(f.source.join("work.txt"), b"base").unwrap();
    f.git(&["init", "--quiet"]);
    f.git(&["add", "work.txt"]);
    let index = fs::read(f.source.join(".git/index")).unwrap();
    let head = fs::read(f.source.join(".git/HEAD")).unwrap();
    let root = f.history.file_recovery_root(true).unwrap().unwrap();
    let mut original_editor = fs::OpenOptions::new()
        .append(true)
        .open(f.source.join("work.txt"))
        .unwrap();
    let apply = f
        .history
        .prepare_main_file_integration(
            &bundle,
            &target,
            "work.txt",
            &root,
            &trust,
            ObservationLimits::default(),
        )
        .unwrap();
    let origin = apply.recovery_path().to_owned();
    let origin_id = origin.file_name().unwrap().to_str().unwrap();
    apply.apply(&trust).unwrap();
    original_editor.write_all(b" + late original work").unwrap();
    fs::set_permissions(origin.join("exchange"), fs::Permissions::from_mode(0o600)).unwrap();
    let mut current_editor = fs::OpenOptions::new()
        .append(true)
        .open(f.source.join("work.txt"))
        .unwrap();
    current_editor.write_all(b" + current work").unwrap();
    let current_inode = current_editor.metadata().unwrap().ino();
    let journal = f.journal();
    let main = f.history.accepted_main(&trust).unwrap();
    let restore = f
        .history
        .prepare_retained_restoration(&root, origin_id, &trust, ObservationLimits::default())
        .unwrap();
    assert_eq!(restore.current_content(), b"approved + current work");
    assert_eq!(restore.restored_content(), b"base + late original work");
    assert_eq!(
        fs::read(f.source.join("work.txt")).unwrap(),
        b"approved + current work"
    );
    let recovery = restore.recovery_path().to_owned();
    let id = recovery.file_name().unwrap().to_str().unwrap();
    let inspect = |id| {
        f.storage
            .reopen(f.history.id())
            .unwrap()
            .inspect_integration_recovery(&root, Some(id), &trust, ObservationLimits::default())
            .unwrap()
    };
    assert_eq!(recovery_status(&inspect(id)), "prepared-arrangement");
    assert_eq!(
        restore.apply(&trust).unwrap().get("status"),
        Some(&Json::text("applied-observed"))
    );
    assert_eq!(
        fs::read(f.source.join("work.txt")).unwrap(),
        b"base + late original work"
    );
    assert_eq!(
        fs::metadata(f.source.join("work.txt"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(recovery.join("exchange")).unwrap().ino(),
        current_inode
    );
    let inspection = inspect(id);
    assert_eq!(recovery_status(&inspection), "applied-arrangement");
    assert_eq!(
        recovery_entry(&inspection)
            .get("details")
            .unwrap()
            .get("content_is_approved_main"),
        Some(&Json::Bool(false))
    );
    current_editor.write_all(b" + later current work").unwrap();
    original_editor
        .write_all(b" + later original work")
        .unwrap();
    assert_eq!(
        fs::read(origin.join("exchange")).unwrap(),
        b"base + late original work + later original work"
    );
    assert_eq!(
        fs::read(recovery.join("exchange")).unwrap(),
        b"approved + current work + later current work"
    );
    assert_eq!(recovery_status(&inspect(id)), "changed-files");
    let undo = f
        .history
        .prepare_retained_restoration(&root, id, &trust, ObservationLimits::default())
        .unwrap();
    let undo_path = undo.recovery_path().to_owned();
    let undo_id = undo_path.file_name().unwrap().to_str().unwrap();
    assert_eq!(
        undo.restored_content(),
        b"approved + current work + later current work"
    );
    undo.apply(&trust).unwrap();
    assert_eq!(recovery_status(&inspect(undo_id)), "applied-arrangement");
    assert_eq!(
        fs::read(undo_path.join("exchange")).unwrap(),
        b"base + late original work"
    );
    assert_eq!(
        fs::read(f.source.join("work.txt")).unwrap(),
        b"approved + current work + later current work"
    );
    assert_eq!(f.history.accepted_main(&trust).unwrap(), main);
    assert_eq!(f.journal(), journal);
    assert_eq!(fs::read(f.source.join(".git/index")).unwrap(), index);
    assert_eq!(fs::read(f.source.join(".git/HEAD")).unwrap(), head);
}

#[test]
fn retained_restoration_refuses_changed_inputs_and_never_replays_an_unused_stage() {
    let f = Fixture::new("restore-refusals");
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    let first = f.save("base");
    accept(&f, &signer, &trust, &first, 1);
    let target = f.save("approved");
    let bundle = accept(&f, &signer, &trust, &target, 2);
    fs::write(f.source.join("work.txt"), b"base").unwrap();
    let root = recovery_root(&f);
    let mut editor = fs::OpenOptions::new()
        .append(true)
        .open(f.source.join("work.txt"))
        .unwrap();
    let apply = f
        .history
        .prepare_main_file_integration(
            &bundle,
            &target,
            "work.txt",
            &root,
            &trust,
            ObservationLimits::default(),
        )
        .unwrap();
    let origin = apply.recovery_path().to_owned();
    let id = origin.file_name().unwrap().to_str().unwrap();
    let prepare = || {
        f.history
            .prepare_retained_restoration(&root, id, &trust, ObservationLimits::default())
    };
    assert!(prepare().is_err());
    apply.apply(&trust).unwrap();
    let restore = prepare().unwrap();
    let recovery = restore.recovery_path().to_owned();
    editor.write_all(b" + changed retained work").unwrap();
    assert_eq!(restore.restored_content(), b"base");
    assert!(restore.apply(&trust).is_err());
    assert_eq!(fs::read(f.source.join("work.txt")).unwrap(), b"approved");
    assert_eq!(fs::read(recovery.join("exchange")).unwrap(), b"base");
    let restore = prepare().unwrap();
    fs::write(f.source.join("work.txt"), b"changed current work").unwrap();
    assert!(restore.apply(&trust).is_err());
    assert_eq!(
        fs::read(f.source.join("work.txt")).unwrap(),
        b"changed current work"
    );
    assert!(prepare()
        .unwrap()
        .apply(&TrustedReviewers::default())
        .is_err());
    let restore = prepare().unwrap();
    fs::write(f.source.join(".meshignore"), b"work.txt\n").unwrap();
    assert!(restore.apply(&trust).is_err());
    fs::remove_file(f.source.join(".meshignore")).unwrap();
    let restore = prepare().unwrap();
    fs::write(restore.recovery_path().join("prepared.json"), b"{}").unwrap();
    assert!(restore.apply(&trust).is_err());
    let restore = prepare().unwrap();
    fs::write(origin.join("prepared.json"), b"{}").unwrap();
    assert!(restore.apply(&trust).is_err());
    assert!(prepare().is_err());
    assert_eq!(
        fs::read(f.source.join("work.txt")).unwrap(),
        b"changed current work"
    );
    assert_eq!(
        fs::read(origin.join("exchange")).unwrap(),
        b"base + changed retained work"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn retained_restoration_uses_retained_metadata_and_keeps_current_metadata_in_recovery() {
    let f = Fixture::new("restore-metadata");
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    let first = f.save("base");
    accept(&f, &signer, &trust, &first, 1);
    let target = f.save("approved");
    let bundle = accept(&f, &signer, &trust, &target, 2);
    fs::write(f.source.join("work.txt"), b"base").unwrap();
    let root = recovery_root(&f);
    let apply = f
        .history
        .prepare_main_file_integration(
            &bundle,
            &target,
            "work.txt",
            &root,
            &trust,
            ObservationLimits::default(),
        )
        .unwrap();
    let origin = apply.recovery_path().to_owned();
    apply.apply(&trust).unwrap();
    let set = |path, value| {
        assert!(std::process::Command::new("/usr/bin/xattr")
            .args(["-w", "user.mesh-restore", value])
            .arg(path)
            .status()
            .unwrap()
            .success())
    };
    let get = |path| {
        std::process::Command::new("/usr/bin/xattr")
            .args(["-p", "user.mesh-restore"])
            .arg(path)
            .output()
            .unwrap()
            .stdout
    };
    assert!(std::process::Command::new("/bin/chmod")
        .args(["+a", "everyone allow read"])
        .arg(origin.join("exchange"))
        .status()
        .unwrap()
        .success());
    set(origin.join("exchange"), "retained metadata");
    set(f.source.join("work.txt"), "current metadata");
    let restore = f
        .history
        .prepare_retained_restoration(
            &root,
            origin.file_name().unwrap().to_str().unwrap(),
            &trust,
            ObservationLimits::default(),
        )
        .unwrap();
    let path = restore.recovery_path().to_owned();
    restore.apply(&trust).unwrap();
    assert_eq!(get(f.source.join("work.txt")), b"retained metadata\n");
    assert_eq!(get(path.join("exchange")), b"current metadata\n");
    assert_eq!(get(origin.join("exchange")), b"retained metadata\n");
}

#[test]
fn restoration_ancestry_is_bounded_and_tampering_never_authorizes_replay() {
    let f = Fixture::new("restore-ancestry");
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    let first = f.save("base");
    accept(&f, &signer, &trust, &first, 1);
    let target = f.save("approved");
    let bundle = accept(&f, &signer, &trust, &target, 2);
    fs::write(f.source.join("work.txt"), b"base").unwrap();
    let root = recovery_root(&f);
    let apply = f
        .history
        .prepare_main_file_integration(
            &bundle,
            &target,
            "work.txt",
            &root,
            &trust,
            ObservationLimits::default(),
        )
        .unwrap();
    let original = apply.recovery_path().to_owned();
    let mut previous = original.clone();
    apply.apply(&trust).unwrap();
    let journal = f.journal();
    for _ in 0..15 {
        let restore = f
            .history
            .prepare_retained_restoration(
                &root,
                previous.file_name().unwrap().to_str().unwrap(),
                &trust,
                ObservationLimits::default(),
            )
            .unwrap();
        previous = restore.recovery_path().to_owned();
        restore.apply(&trust).unwrap();
    }
    let id = previous.file_name().unwrap().to_str().unwrap();
    let inspect = || {
        f.history
            .inspect_integration_recovery(&root, Some(id), &trust, ObservationLimits::default())
            .unwrap()
    };
    assert_eq!(recovery_status(&inspect()), "applied-arrangement");
    let count = fs::read_dir(&root).unwrap().count();
    assert!(f
        .history
        .prepare_retained_restoration(&root, id, &trust, ObservationLimits::default())
        .is_err());
    assert_eq!(fs::read_dir(&root).unwrap().count(), count);
    let source = fs::read(f.source.join("work.txt")).unwrap();
    let retained = fs::read(previous.join("exchange")).unwrap();
    fs::write(original.join("prepared.json"), "{}").unwrap();
    assert_eq!(recovery_status(&inspect()), "unverified-history");
    assert!(f
        .history
        .prepare_retained_restoration(&root, id, &trust, ObservationLimits::default())
        .is_err());
    assert_eq!(fs::read(f.source.join("work.txt")).unwrap(), source);
    assert_eq!(fs::read(previous.join("exchange")).unwrap(), retained);
    assert_eq!(f.journal(), journal);
}

#[test]
fn native_file_recovery_allocation_is_external_private_and_never_follows_replacements() {
    use std::os::unix::fs::{symlink, PermissionsExt as _};
    let f = Fixture::new("native-recovery-root");
    assert!(f.history.file_recovery_root(false).unwrap().is_none());
    let root = f.history.file_recovery_root(true).unwrap().unwrap();
    assert!(root.starts_with(f.history.metadata_path()));
    assert!(!root.starts_with(&f.source));
    assert_eq!(fs::metadata(&root).unwrap().permissions().mode() & 0o077, 0);
    assert_eq!(
        f.history.file_recovery_root(false).unwrap(),
        Some(root.clone())
    );
    fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(f.history.file_recovery_root(false).is_err());
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let kept = f.root.join("retained-recovery-root");
    fs::rename(&root, &kept).unwrap();
    symlink(&kept, &root).unwrap();
    assert!(f.history.file_recovery_root(false).is_err());
    assert!(f.history.file_recovery_root(true).is_err());
    fs::remove_file(&root).unwrap();
    fs::rename(&kept, &root).unwrap();
    let metadata = f.history.metadata_path();
    fs::rename(metadata, f.root.join("retained-metadata")).unwrap();
    fs::create_dir(metadata).unwrap();
    assert!(f.history.file_recovery_root(true).is_err());
    assert!(!metadata.join("file-recovery").exists());
}

#[test]
fn integration_group_preflights_all_members_and_preserves_editor_handles_and_restart_evidence() {
    use std::io::Write as _;
    let f = Fixture::new("group-replacement");
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    fs::write(f.source.join("other.txt"), "other base").unwrap();
    let first = f.save("work base");
    accept(&f, &signer, &trust, &first, 1);
    fs::write(f.source.join("other.txt"), "other accepted").unwrap();
    let second = f.save("work accepted");
    let bundle = accept(&f, &signer, &trust, &second, 2);
    fs::write(f.source.join("other.txt"), "other base").unwrap();
    fs::write(f.source.join("work.txt"), "work base").unwrap();
    let root = recovery_root(&f);
    let failed = f
        .history
        .prepare_main_integration(
            &bundle,
            &second,
            &root,
            &trust,
            ObservationLimits::default(),
        )
        .unwrap();
    fs::write(f.source.join("work.txt"), "later work").unwrap();
    assert!(failed.apply(&trust).is_err());
    assert_eq!(fs::read(f.source.join("other.txt")).unwrap(), b"other base");
    assert_eq!(fs::read(f.source.join("work.txt")).unwrap(), b"later work");
    fs::write(f.source.join("work.txt"), "work base").unwrap();
    let mut editor = fs::OpenOptions::new()
        .append(true)
        .open(f.source.join("other.txt"))
        .unwrap();
    let prepared = f
        .history
        .prepare_main_integration(
            &bundle,
            &second,
            &root,
            &trust,
            ObservationLimits::default(),
        )
        .unwrap();
    assert_eq!(prepared.files().count(), 2);
    let retained = prepared
        .files()
        .find(|file| file.proposal().get("path") == Some(&Json::text("other.txt")))
        .unwrap()
        .recovery_path()
        .join("exchange");
    let group = prepared
        .recovery_path()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let journal = f.journal();
    let result = prepared.apply(&trust).unwrap();
    assert_eq!(result.get("status"), Some(&Json::text("applied-observed")));
    assert_eq!(
        fs::read(f.source.join("other.txt")).unwrap(),
        b"other accepted"
    );
    assert_eq!(
        fs::read(f.source.join("work.txt")).unwrap(),
        b"work accepted"
    );
    editor.write_all(b" later descriptor edit").unwrap();
    editor.sync_all().unwrap();
    assert_eq!(
        fs::read(retained).unwrap(),
        b"other base later descriptor edit"
    );
    let reopened = f.storage.reopen(f.history.id()).unwrap();
    let inspected = reopened
        .inspect_main_integration_group(&root, &group, &trust, ObservationLimits::default())
        .unwrap();
    assert_eq!(
        inspected.get("members").unwrap().as_array().unwrap().len(),
        2
    );
    assert_eq!(inspected.get("automatic_replay"), Some(&Json::Bool(false)));
    assert_eq!(inspected.get("write_authority"), Some(&Json::Bool(false)));
    assert_eq!(f.journal(), journal);
    // Altering membership cannot turn a partial list into evidence for the accepted review.
    let receipt_path = root.join(&group).join("group-prepared.json");
    let original = fs::read_to_string(&receipt_path).unwrap();
    let proposal = Json::parse(&original).unwrap();
    let one = proposal.get("members").unwrap().as_array().unwrap()[0].clone();
    fs::write(
        &receipt_path,
        replace_json(&proposal, "members", Json::Array(vec![one])).encode(),
    )
    .unwrap();
    assert!(reopened
        .inspect_main_integration_group(&root, &group, &trust, ObservationLimits::default())
        .is_err());
}

#[test]
fn integration_group_does_not_rewrite_already_present_files_or_skip_divergent_directory_changes() {
    use std::os::unix::fs::MetadataExt as _;
    let f = Fixture::new("group-coverage");
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    fs::write(f.source.join("other.txt"), "base").unwrap();
    let first = f.save("base");
    accept(&f, &signer, &trust, &first, 1);
    fs::write(f.source.join("other.txt"), "accepted").unwrap();
    let second = f.save("accepted");
    let bundle = accept(&f, &signer, &trust, &second, 2);
    fs::write(f.source.join("work.txt"), "base").unwrap();
    let inode = fs::metadata(f.source.join("other.txt")).unwrap().ino();
    let root = recovery_root(&f);
    let prepared = f
        .history
        .prepare_main_integration(
            &bundle,
            &second,
            &root,
            &trust,
            ObservationLimits::default(),
        )
        .unwrap();
    assert_eq!(prepared.files().count(), 1);
    assert_eq!(
        prepared.proposal().get("already_present"),
        Some(&Json::Array(vec![Json::text("other.txt")]))
    );
    prepared.apply(&trust).unwrap();
    assert_eq!(
        fs::metadata(f.source.join("other.txt")).unwrap().ino(),
        inode
    );
    fs::create_dir(f.source.join("new-folder")).unwrap();
    fs::write(f.source.join("new-folder/new.txt"), "new accepted").unwrap();
    let third = f.save("third");
    let third_bundle = accept(&f, &signer, &trust, &third, 3);
    fs::write(f.source.join("work.txt"), "accepted").unwrap();
    fs::remove_file(f.source.join("new-folder/new.txt")).unwrap();
    fs::remove_dir(f.source.join("new-folder")).unwrap();
    let prepared = f
        .history
        .prepare_main_integration(
            &third_bundle,
            &third,
            &root,
            &trust,
            ObservationLimits::default(),
        )
        .unwrap();
    assert_eq!(prepared.directories().count(), 1);
    assert_eq!(prepared.files().count(), 1);
    let group_id = prepared
        .recovery_path()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let before = f
        .history
        .inspect_main_integration_group(&root, &group_id, &trust, ObservationLimits::default())
        .unwrap();
    assert_eq!(
        before
            .get("members")
            .and_then(Json::as_array)
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        prepared.apply(&trust).unwrap().get("status"),
        Some(&Json::text("applied-observed"))
    );
    assert_eq!(
        fs::read(f.source.join("new-folder/new.txt")).unwrap(),
        b"new accepted"
    );
    assert_eq!(fs::read(f.source.join("work.txt")).unwrap(), b"third");
    f.history
        .inspect_main_integration_group(&root, &group_id, &trust, ObservationLimits::default())
        .unwrap();
    // Conversion cannot discard divergent working content; never skip it.
    fs::remove_dir_all(f.source.join("new-folder")).unwrap();
    fs::write(f.source.join("new-folder"), "converted to file").unwrap();
    let fourth = f.save("fourth");
    let fourth_bundle = accept(&f, &signer, &trust, &fourth, 4);
    fs::remove_file(f.source.join("new-folder")).unwrap();
    fs::create_dir(f.source.join("new-folder")).unwrap();
    fs::write(f.source.join("new-folder/new.txt"), "new accepted").unwrap();
    fs::write(f.source.join("work.txt"), "third").unwrap();
    fs::write(f.source.join("new-folder/new.txt"), "unreviewed user work").unwrap();
    let count = fs::read_dir(&root).unwrap().count();
    assert!(f
        .history
        .prepare_main_integration(
            &fourth_bundle,
            &fourth,
            &root,
            &trust,
            ObservationLimits::default()
        )
        .is_err());
    assert_eq!(fs::read_dir(root).unwrap().count(), count);
    assert_eq!(fs::read(f.source.join("work.txt")).unwrap(), b"third");
}

#[test]
fn integration_group_removes_approved_file_retains_editor_work_and_inspects_restart() {
    use std::io::Write as _;
    let f = Fixture::new("group-removal");
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    fs::write(f.source.join("old.txt"), "old base").unwrap();
    let first = f.save("base");
    accept(&f, &signer, &trust, &first, 1);
    fs::remove_file(f.source.join("old.txt")).unwrap();
    let target = f.save("accepted");
    let bundle = accept(&f, &signer, &trust, &target, 2);
    fs::write(f.source.join("old.txt"), "old base").unwrap();
    fs::write(f.source.join("work.txt"), "base").unwrap();
    let root = recovery_root(&f);
    assert!(f
        .history
        .prepare_main_file_integration(
            &bundle,
            &target,
            "old.txt",
            &root,
            &trust,
            ObservationLimits::default()
        )
        .is_err());
    let prepared = f
        .history
        .prepare_main_integration(
            &bundle,
            &target,
            &root,
            &trust,
            ObservationLimits::default(),
        )
        .unwrap();
    let group_path = prepared.recovery_path().to_owned();
    let group = group_path.file_name().unwrap().to_str().unwrap();
    let removal = prepared.files().find(|file| file.removes_path()).unwrap();
    assert_eq!(removal.current_content(), b"old base");
    assert_eq!(removal.proposed_content(), b"");
    assert_eq!(
        removal.proposal().get("schema"),
        Some(&Json::text("mesh.attachment-file-removal/v1"))
    );
    assert_eq!(removal.proposal().get("installed_file"), Some(&Json::Null));
    let transaction = removal
        .recovery_path()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let retained = removal.recovery_path().join("exchange");
    let inspect_status = |history: &ProvisionedAttachment, limits| {
        let result = history
            .inspect_integration_recovery(&group_path, Some(&transaction), &trust, limits)
            .unwrap();
        result.get("entries").unwrap().as_array().unwrap()[0]
            .get("status")
            .unwrap()
            .clone()
    };
    assert_eq!(
        inspect_status(&f.history, ObservationLimits::default()),
        Json::text("prepared-arrangement")
    );
    let mut editor = fs::OpenOptions::new()
        .append(true)
        .open(f.source.join("old.txt"))
        .unwrap();
    let journal = f.journal();
    assert_eq!(
        prepared.apply(&trust).unwrap().get("status"),
        Some(&Json::text("applied-observed"))
    );
    assert!(!f.source.join("old.txt").exists());
    assert_eq!(fs::read(f.source.join("work.txt")).unwrap(), b"accepted");
    let reopened = f.storage.reopen(f.history.id()).unwrap();
    assert_eq!(
        inspect_status(&reopened, ObservationLimits::default()),
        Json::text("applied-arrangement")
    );
    let small = ObservationLimits {
        file_bytes: 1,
        ..ObservationLimits::default()
    };
    assert_eq!(
        inspect_status(&reopened, small),
        Json::text("incomplete-observation")
    );
    assert_eq!(
        reopened
            .inspect_main_integration_group(&root, group, &trust, ObservationLimits::default())
            .unwrap()
            .get("automatic_replay"),
        Some(&Json::Bool(false))
    );
    // An outcome is evidence only: losing it never replays the removal, and malformed receipts
    // cannot reinterpret absence as an empty replacement or inherit an installed file identity.
    let receipt_path = group_path.join(&transaction).join("prepared.json");
    let original_receipt = fs::read_to_string(&receipt_path).unwrap();
    let receipt = Json::parse(&original_receipt).unwrap();
    fs::write(
        &receipt_path,
        replace_json(
            &receipt,
            "installed_file",
            receipt.get("source_file").unwrap().clone(),
        )
        .encode(),
    )
    .unwrap();
    assert_eq!(
        inspect_status(&reopened, ObservationLimits::default()),
        Json::text("invalid-receipt")
    );
    fs::write(&receipt_path, original_receipt).unwrap();
    fs::remove_file(group_path.join(&transaction).join("observed.json")).unwrap();
    assert_eq!(
        inspect_status(&reopened, ObservationLimits::default()),
        Json::text("applied-arrangement")
    );
    assert!(!f.source.join("old.txt").exists());
    editor.write_all(b" late work").unwrap();
    editor.sync_all().unwrap();
    assert_eq!(fs::read(&retained).unwrap(), b"old base late work");
    assert_eq!(
        inspect_status(&reopened, ObservationLimits::default()),
        Json::text("changed-files")
    );
    fs::write(f.source.join("old.txt"), "recreated user work").unwrap();
    assert_eq!(
        inspect_status(&reopened, ObservationLimits::default()),
        Json::text("changed-files")
    );
    assert_eq!(
        fs::read(f.source.join("old.txt")).unwrap(),
        b"recreated user work"
    );
    assert_eq!(f.journal(), journal);
}

#[test]
fn integration_group_already_absent_removal_is_not_confused_with_empty_or_missing_parent() {
    let f = Fixture::new("group-already-removed");
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    fs::create_dir(f.source.join("nested")).unwrap();
    fs::write(f.source.join("nested/old.txt"), "old").unwrap();
    let first = f.save("base");
    accept(&f, &signer, &trust, &first, 1);
    fs::remove_file(f.source.join("nested/old.txt")).unwrap();
    let target = f.save("accepted");
    let bundle = accept(&f, &signer, &trust, &target, 2);
    fs::write(f.source.join("work.txt"), "base").unwrap();
    let root = recovery_root(&f);
    let prepare = || {
        f.history.prepare_main_integration(
            &bundle,
            &target,
            &root,
            &trust,
            ObservationLimits::default(),
        )
    };
    let prepared = prepare().unwrap();
    assert_eq!(prepared.files().count(), 1);
    assert_eq!(
        prepared.proposal().get("already_present"),
        Some(&Json::Array(vec![Json::text("nested/old.txt")]))
    );
    fs::write(f.source.join("nested/old.txt"), "").unwrap();
    assert!(prepared.apply(&trust).is_err());
    assert!(prepare().is_err());
    assert_eq!(fs::read(f.source.join("work.txt")).unwrap(), b"base");
    fs::remove_file(f.source.join("nested/old.txt")).unwrap();
    fs::remove_dir(f.source.join("nested")).unwrap();
    assert!(prepare().is_err());
    fs::create_dir(f.source.join("nested")).unwrap();
    prepare().unwrap().apply(&trust).unwrap();
    assert_eq!(fs::read(f.source.join("work.txt")).unwrap(), b"accepted");
}

#[test]
fn integration_group_adds_replaces_and_removes_without_rewriting_already_present_additions() {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
    let f = Fixture::new("group-addition");
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    fs::write(f.source.join("old.txt"), "old base").unwrap();
    let first = f.save("base");
    accept(&f, &signer, &trust, &first, 1);
    fs::remove_file(f.source.join("old.txt")).unwrap();
    fs::write(f.source.join("new.txt"), "approved new file").unwrap();
    fs::set_permissions(f.source.join("new.txt"), fs::Permissions::from_mode(0o755)).unwrap();
    let target = f.save("accepted");
    let bundle = accept(&f, &signer, &trust, &target, 2);
    fs::remove_file(f.source.join("new.txt")).unwrap();
    fs::write(f.source.join("old.txt"), "old base").unwrap();
    fs::write(f.source.join("work.txt"), "base").unwrap();
    let root = recovery_root(&f);
    assert!(f
        .history
        .prepare_main_file_integration(
            &bundle,
            &target,
            "new.txt",
            &root,
            &trust,
            ObservationLimits::default()
        )
        .is_err());
    let prepared = f
        .history
        .prepare_main_integration(
            &bundle,
            &target,
            &root,
            &trust,
            ObservationLimits::default(),
        )
        .unwrap();
    assert_eq!(prepared.files().count(), 3);
    let group_path = prepared.recovery_path().to_owned();
    let group = group_path.file_name().unwrap().to_str().unwrap();
    let addition = prepared.files().find(|file| file.adds_path()).unwrap();
    assert!(!addition.removes_path());
    assert_eq!(addition.current_content(), b"");
    assert_eq!(addition.proposed_content(), b"approved new file");
    assert_eq!(
        addition.proposal().get("schema"),
        Some(&Json::text("mesh.attachment-file-addition/v2"))
    );
    for field in [
        "source_file",
        "source_digest",
        "source_mode",
        "source_executable",
    ] {
        assert_eq!(addition.proposal().get(field), Some(&Json::Null));
    }
    let transaction = addition
        .recovery_path()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let retained = addition.recovery_path().join("exchange");
    let inspect = |history: &ProvisionedAttachment| {
        let result = history
            .inspect_integration_recovery(
                &group_path,
                Some(&transaction),
                &trust,
                ObservationLimits::default(),
            )
            .unwrap();
        result.get("entries").unwrap().as_array().unwrap()[0].clone()
    };
    assert_eq!(
        inspect(&f.history).get("status"),
        Some(&Json::text("prepared-arrangement"))
    );
    let journal = f.journal();
    assert_eq!(
        prepared.apply(&trust).unwrap().get("status"),
        Some(&Json::text("applied-observed"))
    );
    assert!(!f.source.join("old.txt").exists());
    assert!(!retained.exists());
    assert_eq!(
        fs::read(f.source.join("new.txt")).unwrap(),
        b"approved new file"
    );
    assert_eq!(fs::read(f.source.join("work.txt")).unwrap(), b"accepted");
    let reopened = f.storage.reopen(f.history.id()).unwrap();
    assert_eq!(
        inspect(&reopened).get("status"),
        Some(&Json::text("applied-arrangement"))
    );
    reopened
        .inspect_main_integration_group(&root, group, &trust, ObservationLimits::default())
        .unwrap();
    let receipt_path = group_path.join(&transaction).join("prepared.json");
    let raw = fs::read_to_string(&receipt_path).unwrap();
    let receipt = Json::parse(&raw).unwrap();
    fs::write(
        &receipt_path,
        replace_json(
            &receipt,
            "source_file",
            receipt.get("installed_file").unwrap().clone(),
        )
        .encode(),
    )
    .unwrap();
    assert_eq!(
        inspect(&reopened).get("status"),
        Some(&Json::text("invalid-receipt"))
    );
    fs::write(&receipt_path, raw).unwrap();
    fs::remove_file(group_path.join(&transaction).join("observed.json")).unwrap();
    assert_eq!(
        inspect(&reopened).get("status"),
        Some(&Json::text("applied-arrangement"))
    );
    assert_eq!(
        inspect(&reopened).get("attention_required"),
        Some(&Json::Bool(true))
    );
    let inode = fs::metadata(f.source.join("new.txt")).unwrap().ino();
    fs::write(f.source.join("work.txt"), "base").unwrap();
    let again = reopened
        .prepare_main_integration(
            &bundle,
            &target,
            &root,
            &trust,
            ObservationLimits::default(),
        )
        .unwrap();
    assert_eq!(again.files().count(), 1);
    again.apply(&trust).unwrap();
    assert_eq!(fs::metadata(f.source.join("new.txt")).unwrap().ino(), inode);
    fs::write(f.source.join("new.txt"), "later user work").unwrap();
    assert_eq!(
        inspect(&reopened).get("status"),
        Some(&Json::text("changed-files"))
    );
    assert_eq!(
        fs::read(f.source.join("new.txt")).unwrap(),
        b"later user work"
    );
    fs::remove_file(f.source.join("new.txt")).unwrap();
    assert_eq!(
        inspect(&reopened).get("status"),
        Some(&Json::text("incomplete-observation"))
    );
    assert_eq!(f.journal(), journal);
}

#[test]
fn initial_approved_main_can_add_regular_files_with_verified_restart_evidence() {
    let f = Fixture::new("group-initial-addition");
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    let target = f.save("first accepted");
    let bundle = accept(&f, &signer, &trust, &target, 1);
    fs::remove_file(f.source.join("work.txt")).unwrap();
    let root = recovery_root(&f);
    let prepared = f
        .history
        .prepare_main_integration(
            &bundle,
            &target,
            &root,
            &trust,
            ObservationLimits::default(),
        )
        .unwrap();
    let group = prepared
        .recovery_path()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    assert!(prepared.files().next().unwrap().adds_path());
    assert_eq!(
        prepared.apply(&trust).unwrap().get("status"),
        Some(&Json::text("applied-observed"))
    );
    let reopened = f.storage.reopen(f.history.id()).unwrap();
    reopened
        .inspect_main_integration_group(&root, &group, &trust, ObservationLimits::default())
        .unwrap();
    assert_eq!(
        fs::read(f.source.join("work.txt")).unwrap(),
        b"first accepted"
    );
}

#[test]
fn addition_recovery_reads_v1_evidence_and_reports_v2_parent_policy_changes_without_replay() {
    use std::os::unix::fs::PermissionsExt as _;
    let f = Fixture::new("addition-policy-recovery");
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    let first = f.save("base");
    accept(&f, &signer, &trust, &first, 1);
    fs::write(f.source.join("new.txt"), "new approved").unwrap();
    let target = f.capture();
    let bundle = accept(&f, &signer, &trust, &target, 2);
    fs::remove_file(f.source.join("new.txt")).unwrap();
    let root = recovery_root(&f);
    let prepared = f
        .history
        .prepare_main_integration(
            &bundle,
            &target,
            &root,
            &trust,
            ObservationLimits::default(),
        )
        .unwrap();
    let group_path = prepared.recovery_path().to_owned();
    let addition = prepared.files().next().unwrap();
    let transaction = addition
        .recovery_path()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let receipt_path = addition.recovery_path().join("prepared.json");
    let raw = fs::read_to_string(&receipt_path).unwrap();
    let original = Json::parse(&raw).unwrap();
    let inspect = || {
        let reopened = f.storage.reopen(f.history.id()).unwrap();
        let result = reopened
            .inspect_integration_recovery(
                &group_path,
                Some(&transaction),
                &trust,
                ObservationLimits::default(),
            )
            .unwrap();
        result.get("entries").unwrap().as_array().unwrap()[0].clone()
    };
    assert_eq!(
        inspect()
            .get("details")
            .unwrap()
            .get("parent_policy_matches"),
        Some(&Json::Bool(true))
    );
    // V1 never claimed destination inheritance. Its original field order and null source facts
    // remain readable; no upgrade invents parent policy or gains replay authority.
    let mut old = replace_json(
        &original,
        "schema",
        Json::text("mesh.attachment-file-addition/v1"),
    );
    if let Json::Object(fields) = &mut old {
        fields.retain(|(key, _)| !matches!(key.as_str(), "parent_metadata_digest" | "parent_mode"));
    }
    fs::write(&receipt_path, old.encode()).unwrap();
    let inspected = inspect();
    assert_eq!(
        inspected.get("status"),
        Some(&Json::text("prepared-arrangement"))
    );
    assert!(inspected
        .get("details")
        .unwrap()
        .get("parent_policy_matches")
        .is_none());
    assert_eq!(inspected.get("automatic_replay"), Some(&Json::Bool(false)));
    let malformed = replace_json(&original, "parent_mode", Json::Number(0o100644));
    fs::write(&receipt_path, malformed.encode()).unwrap();
    assert_eq!(
        inspect().get("status"),
        Some(&Json::text("invalid-receipt"))
    );
    fs::write(&receipt_path, &raw).unwrap();
    let mode = fs::metadata(&f.source).unwrap().permissions().mode();
    fs::set_permissions(&f.source, fs::Permissions::from_mode(mode ^ 0o010)).unwrap();
    assert_eq!(
        inspect()
            .get("details")
            .unwrap()
            .get("parent_policy_matches"),
        Some(&Json::Bool(false))
    );
    assert_eq!(inspect().get("attention_required"), Some(&Json::Bool(true)));
    assert!(prepared.apply(&trust).is_err());
    assert!(!f.source.join("new.txt").exists());
    fs::set_permissions(&f.source, fs::Permissions::from_mode(mode)).unwrap();
    let prepared = f
        .history
        .prepare_main_integration(
            &bundle,
            &target,
            &root,
            &trust,
            ObservationLimits::default(),
        )
        .unwrap();
    assert_eq!(
        prepared.apply(&trust).unwrap().get("status"),
        Some(&Json::text("applied-observed"))
    );
    assert_eq!(fs::read(f.source.join("new.txt")).unwrap(), b"new approved");
}

fn removed_file_for_restoration(name: &str) -> (Fixture, TrustedReviewers, PathBuf, String) {
    let f = Fixture::new(name);
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    fs::create_dir(f.source.join("folder")).unwrap();
    fs::write(f.source.join("folder/old.txt"), "retained base").unwrap();
    let first = f.save("unchanged");
    accept(&f, &signer, &trust, &first, 1);
    fs::remove_file(f.source.join("folder/old.txt")).unwrap();
    let target = f.capture();
    let bundle = accept(&f, &signer, &trust, &target, 2);
    fs::write(f.source.join("folder/old.txt"), "retained base").unwrap();
    let prepared = f
        .history
        .prepare_main_integration(
            &bundle,
            &target,
            &recovery_root(&f),
            &trust,
            ObservationLimits::default(),
        )
        .unwrap();
    let root = prepared.recovery_path().to_owned();
    let id = prepared
        .files()
        .next()
        .unwrap()
        .recovery_path()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    prepared.apply(&trust).unwrap();
    (f, trust, root, id)
}

#[test]
fn absent_retained_restoration_preserves_original_inode_metadata_and_late_editor_work() {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
    let (f, trust, root, id) = removed_file_for_restoration("restore-absent");
    let original = root.join(&id).join("exchange");
    let old_inode = fs::metadata(&original).unwrap().ino();
    let mut editor = fs::OpenOptions::new().append(true).open(&original).unwrap();
    editor.write_all(b" + private work").unwrap();
    fs::set_permissions(&original, fs::Permissions::from_mode(0o640)).unwrap();
    #[cfg(target_os = "macos")]
    {
        assert!(std::process::Command::new("/usr/bin/xattr")
            .args(["-w", "user.mesh.restore", "retained metadata"])
            .arg(&original)
            .status()
            .unwrap()
            .success());
        assert!(std::process::Command::new("/bin/chmod")
            .args(["+a", "everyone allow read"])
            .arg(&original)
            .status()
            .unwrap()
            .success());
    }
    let restored = f.source.join("folder/old.txt");
    let journal = f.journal();
    let prepared = f
        .history
        .prepare_retained_restoration(&root, &id, &trust, ObservationLimits::default())
        .unwrap();
    assert!(prepared.adds_path());
    assert_eq!(prepared.current_content(), b"");
    assert_eq!(prepared.restored_content(), b"retained base + private work");
    assert!(!restored.exists());
    let transaction = prepared
        .recovery_path()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let staged = prepared.recovery_path().join("exchange");
    let staged_inode = fs::metadata(&staged).unwrap().ino();
    let inspect = || {
        f.storage
            .reopen(f.history.id())
            .unwrap()
            .inspect_integration_recovery(
                &root,
                Some(&transaction),
                &trust,
                ObservationLimits::default(),
            )
            .unwrap()
    };
    assert_eq!(recovery_status(&inspect()), "prepared-arrangement");
    let outcome = prepared.apply(&trust).unwrap();
    assert_eq!(outcome.get("status"), Some(&Json::text("applied-observed")));
    assert_eq!(
        outcome.get("displaced_file_retained"),
        Some(&Json::Bool(false))
    );
    assert_eq!(fs::metadata(&restored).unwrap().ino(), staged_inode);
    assert_eq!(fs::metadata(&restored).unwrap().mode() & 0o777, 0o640);
    assert_eq!(fs::metadata(&original).unwrap().ino(), old_inode);
    assert!(!staged.exists());
    assert_eq!(recovery_status(&inspect()), "applied-arrangement");
    #[cfg(target_os = "macos")]
    {
        let result = std::process::Command::new("/usr/bin/xattr")
            .args(["-p", "user.mesh.restore"])
            .arg(&restored)
            .output()
            .unwrap();
        assert!(result.status.success());
        assert_eq!(result.stdout, b"retained metadata\n");
        let receipt = Json::parse(
            &fs::read_to_string(root.join(&transaction).join("prepared.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            receipt.get("origin_metadata_digest"),
            receipt.get("installed_metadata_digest")
        );
    }
    editor.write_all(b" + later editor work").unwrap();
    editor.sync_all().unwrap();
    assert_eq!(
        fs::read(&restored).unwrap(),
        b"retained base + private work"
    );
    assert_eq!(
        fs::read(&original).unwrap(),
        b"retained base + private work + later editor work"
    );
    fs::remove_file(root.join(&transaction).join("observed.json")).unwrap();
    assert_eq!(recovery_status(&inspect()), "applied-arrangement");
    // This creation displaced nothing, so it cannot be used as an automatic delete/undo authority.
    assert!(f
        .history
        .prepare_retained_restoration(&root, &transaction, &trust, ObservationLimits::default())
        .is_err());
    assert_eq!(f.journal(), journal);
}

#[test]
fn absent_restoration_refuses_collisions_changed_retained_work_and_unavailable_parents() {
    use std::os::unix::fs::{symlink, PermissionsExt as _};
    let (f, trust, root, id) = removed_file_for_restoration("restore-absent-refusals");
    let original = root.join(&id).join("exchange");
    let destination = f.source.join("folder/old.txt");
    let prepare = || {
        f.history
            .prepare_retained_restoration(&root, &id, &trust, ObservationLimits::default())
    };
    let journal = f.journal();
    let prepared = prepare().unwrap();
    let stage = prepared.recovery_path().join("exchange");
    fs::write(&destination, "new user work").unwrap();
    assert!(prepared.apply(&trust).is_err());
    assert_eq!(fs::read(&destination).unwrap(), b"new user work");
    assert_eq!(fs::read(&stage).unwrap(), b"retained base");
    fs::remove_file(&destination).unwrap();
    let prepared = prepare().unwrap();
    symlink(&original, &destination).unwrap();
    assert!(prepared.apply(&trust).is_err());
    assert!(prepare().is_err());
    fs::remove_file(&destination).unwrap();
    let prepared = prepare().unwrap();
    fs::write(&original, "later retained work").unwrap();
    assert!(prepared.apply(&trust).is_err());
    assert!(!destination.exists());
    let prepared = prepare().unwrap();
    let parent = f.source.join("folder");
    let mode = fs::metadata(&parent).unwrap().permissions().mode();
    fs::set_permissions(&parent, fs::Permissions::from_mode(mode ^ 0o010)).unwrap();
    assert!(prepared.apply(&trust).is_err());
    fs::set_permissions(&parent, fs::Permissions::from_mode(mode)).unwrap();
    let prepared = prepare().unwrap();
    fs::rename(&parent, f.source.join("moved-folder")).unwrap();
    assert!(prepared.apply(&trust).is_err());
    assert!(prepare().is_err());
    fs::rename(f.source.join("moved-folder"), &parent).unwrap();
    let prepared = prepare().unwrap();
    fs::write(f.source.join(".meshignore"), "folder/old.txt\n").unwrap();
    assert!(prepared.apply(&trust).is_err());
    assert!(prepare().is_err());
    fs::remove_file(f.source.join(".meshignore")).unwrap();
    assert!(prepare()
        .unwrap()
        .apply(&TrustedReviewers::default())
        .is_err());
    assert!(f
        .history
        .prepare_retained_restoration(
            &root,
            &id,
            &trust,
            ObservationLimits {
                file_bytes: 1,
                ..ObservationLimits::default()
            }
        )
        .is_err());
    assert!(!destination.exists());
    assert_eq!(fs::read(&original).unwrap(), b"later retained work");
    assert_eq!(f.journal(), journal);
}

#[test]
fn absent_restoration_recovery_checks_null_source_policy_and_ancestry_without_replay() {
    use std::os::unix::fs::PermissionsExt as _;
    let (f, trust, root, id) = removed_file_for_restoration("restore-absent-evidence");
    let prepared = f
        .history
        .prepare_retained_restoration(&root, &id, &trust, ObservationLimits::default())
        .unwrap();
    let path = prepared.recovery_path().to_owned();
    let transaction = path.file_name().unwrap().to_str().unwrap();
    let receipt_path = path.join("prepared.json");
    let receipt = prepared.proposal().clone();
    let inspect = || {
        f.storage
            .reopen(f.history.id())
            .unwrap()
            .inspect_integration_recovery(
                &root,
                Some(transaction),
                &trust,
                ObservationLimits::default(),
            )
            .unwrap()
    };
    for (field, value) in [
        ("source_digest", Json::text("0".repeat(64))),
        (
            "source_file",
            receipt.get("installed_file").unwrap().clone(),
        ),
        ("source_mode", Json::Number(0o100600)),
        ("source_executable", Json::Bool(false)),
        ("parent_mode", Json::Number(0o100755)),
        ("parent_metadata_digest", Json::Null),
        ("installed_metadata_digest", Json::text("0".repeat(64))),
    ] {
        fs::write(&receipt_path, replace_json(&receipt, field, value).encode()).unwrap();
        assert_eq!(recovery_status(&inspect()), "invalid-receipt", "{field}");
    }
    fs::write(
        &receipt_path,
        replace_json(
            &receipt,
            "origin_proposal_digest",
            Json::text("0".repeat(64)),
        )
        .encode(),
    )
    .unwrap();
    assert_eq!(recovery_status(&inspect()), "unverified-history");
    fs::write(&receipt_path, receipt.encode()).unwrap();
    let parent = f.source.join("folder");
    let mode = fs::metadata(&parent).unwrap().permissions().mode();
    fs::set_permissions(&parent, fs::Permissions::from_mode(mode ^ 0o010)).unwrap();
    let inspection = inspect();
    assert_eq!(recovery_status(&inspection), "prepared-arrangement");
    let entry = &inspection.get("entries").unwrap().as_array().unwrap()[0];
    assert_eq!(entry.get("attention_required"), Some(&Json::Bool(true)));
    assert_eq!(
        entry.get("details").unwrap().get("parent_policy_matches"),
        Some(&Json::Bool(false))
    );
    assert_eq!(
        entry
            .get("details")
            .unwrap()
            .get("content_is_approved_main"),
        Some(&Json::Bool(false))
    );
    fs::set_permissions(&parent, fs::Permissions::from_mode(mode)).unwrap();
    fs::rename(&parent, f.source.join("moved")).unwrap();
    assert_eq!(recovery_status(&inspect()), "incomplete-observation");
    fs::rename(f.source.join("moved"), &parent).unwrap();
    let outcome = prepared.apply(&trust).unwrap();
    fs::write(
        path.join("observed.json"),
        replace_json(&outcome, "displaced_file_retained", Json::Bool(true)).encode(),
    )
    .unwrap();
    assert_eq!(recovery_status(&inspect()), "invalid-outcome");
    fs::write(path.join("observed.json"), outcome.encode()).unwrap();
    fs::write(f.source.join("folder/old.txt"), "new private edits").unwrap();
    assert_eq!(recovery_status(&inspect()), "changed-files");
    assert_eq!(
        fs::read(f.source.join("folder/old.txt")).unwrap(),
        b"new private edits"
    );
    assert_eq!(
        fs::read(root.join(&id).join("exchange")).unwrap(),
        b"retained base"
    );
}

#[test]
fn replacement_retained_work_can_be_restored_after_the_installed_path_disappears() {
    let f = Fixture::new("restore-replacement-absent");
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    let first = f.save("base");
    accept(&f, &signer, &trust, &first, 1);
    let target = f.save("approved");
    let bundle = accept(&f, &signer, &trust, &target, 2);
    fs::write(f.source.join("work.txt"), "base").unwrap();
    let root = recovery_root(&f);
    let integration = f
        .history
        .prepare_main_file_integration(
            &bundle,
            &target,
            "work.txt",
            &root,
            &trust,
            ObservationLimits::default(),
        )
        .unwrap();
    let id = integration
        .recovery_path()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    integration.apply(&trust).unwrap();
    fs::remove_file(f.source.join("work.txt")).unwrap();
    let restore = f
        .history
        .prepare_retained_restoration(&root, &id, &trust, ObservationLimits::default())
        .unwrap();
    assert!(restore.adds_path());
    assert_eq!(
        restore.apply(&trust).unwrap().get("status"),
        Some(&Json::text("applied-observed"))
    );
    assert_eq!(fs::read(f.source.join("work.txt")).unwrap(), b"base");
    assert_eq!(fs::read(root.join(id).join("exchange")).unwrap(), b"base");
}

#[test]
fn group_catalogue_is_only_a_reference_and_native_member_evidence_identifies_displaced_files() {
    let (f, trust, root, id) = removed_file_for_restoration("group-catalogue");
    let outer = root.parent().unwrap();
    let group = root.file_name().unwrap().to_str().unwrap();
    let catalogue = f
        .history
        .inspect_integration_recovery(outer, None, &trust, ObservationLimits::default())
        .unwrap();
    let reference = catalogue
        .get("entries")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry.get("transaction") == Some(&Json::text(group)))
        .unwrap();
    assert_eq!(
        reference.get("status"),
        Some(&Json::text("group-reference"))
    );
    assert_eq!(reference.get("details"), Some(&Json::Null));
    assert_eq!(reference.get("write_authority"), Some(&Json::Bool(false)));
    let evidence = f
        .history
        .inspect_main_integration_group(outer, group, &trust, ObservationLimits::default())
        .unwrap();
    let member = &evidence.get("members").unwrap().as_array().unwrap()[0];
    let entry = &member
        .get("recovery")
        .unwrap()
        .get("entries")
        .unwrap()
        .as_array()
        .unwrap()[0];
    assert_eq!(
        entry
            .get("details")
            .unwrap()
            .get("retained_file_is_displaced"),
        Some(&Json::Bool(true))
    );
    let restored = f
        .history
        .prepare_retained_restoration(&root, &id, &trust, ObservationLimits::default())
        .unwrap();
    let tx = restored
        .recovery_path()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let inspect = || {
        f.history
            .inspect_integration_recovery(&root, Some(&tx), &trust, ObservationLimits::default())
            .unwrap()
    };
    let staged = inspect();
    let entry = &staged.get("entries").unwrap().as_array().unwrap()[0];
    assert_eq!(
        entry
            .get("details")
            .unwrap()
            .get("retained_file_is_displaced"),
        Some(&Json::Bool(false))
    );
    restored.apply(&trust).unwrap();
    assert_eq!(recovery_status(&inspect()), "applied-arrangement");
    let rediscovered = f
        .storage
        .reopen(f.history.id())
        .unwrap()
        .inspect_main_integration_group(outer, group, &trust, ObservationLimits::default())
        .unwrap();
    assert!(rediscovered
        .get("restoration_references")
        .unwrap()
        .as_array()
        .unwrap()
        .contains(&Json::text(&tx)));
    assert!(f
        .history
        .inspect_main_integration_group(outer, "../outside", &trust, ObservationLimits::default())
        .is_err());
}

fn approved_new_directory(
    name: &str,
) -> (
    Fixture,
    TestSigner,
    TrustedReviewers,
    String,
    String,
    PathBuf,
) {
    use std::os::unix::fs::PermissionsExt as _;
    let f = Fixture::new(name);
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    let first = f.save("original work");
    accept(&f, &signer, &trust, &first, 1);
    fs::create_dir_all(f.source.join("new/sub/empty")).unwrap();
    fs::write(f.source.join("new/sub/run"), b"approved executable").unwrap();
    fs::set_permissions(
        f.source.join("new/sub/run"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    fs::write(f.source.join("new/data"), [0u8, 255, 12]).unwrap();
    let target = f.capture();
    let bundle = accept(&f, &signer, &trust, &target, 2);
    fs::remove_dir_all(f.source.join("new")).unwrap();
    let root = recovery_root(&f);
    (f, signer, trust, bundle, target, root)
}

#[test]
fn approved_directory_addition_stages_complete_tree_and_recovers_without_replay() {
    use std::os::unix::fs::PermissionsExt as _;
    let (f, _, trust, bundle, target, root) = approved_new_directory("directory-complete");
    f.git(&["init", "--quiet"]);
    let git = f.git(&["status", "--porcelain"]);
    let journal = f.journal();
    let prepared = f
        .history
        .prepare_main_directory_addition(
            &bundle,
            &target,
            "new",
            &root,
            &trust,
            ObservationLimits::default(),
        )
        .unwrap();
    let path = prepared.recovery_path().to_owned();
    let id = path.file_name().unwrap().to_str().unwrap();
    assert!(!f.source.join("new").exists());
    assert_eq!(f.git(&["status", "--porcelain"]), git);
    assert_eq!(
        prepared
            .proposal()
            .get("tree")
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        5
    );
    assert!(path.join("exchange/sub/empty").is_dir());
    assert_eq!(fs::read(path.join("exchange/data")).unwrap(), [0, 255, 12]);
    let inspect = |history: &ProvisionedAttachment| {
        history
            .inspect_directory_addition(&root, id, &trust, ObservationLimits::default())
            .unwrap()
    };
    assert_eq!(
        inspect(&f.history).get("status"),
        Some(&Json::text("prepared-arrangement"))
    );
    assert_eq!(
        prepared.apply(&trust).unwrap().get("status"),
        Some(&Json::text("applied-observed"))
    );
    assert_eq!(fs::read(f.source.join("new/data")).unwrap(), [0, 255, 12]);
    assert!(f.source.join("new/sub/empty").is_dir());
    assert_ne!(
        fs::metadata(f.source.join("new/sub/run"))
            .unwrap()
            .permissions()
            .mode()
            & 0o111,
        0
    );
    assert!(!path.join("exchange").exists());
    let reopened = f.storage.reopen(f.history.id()).unwrap();
    assert_eq!(
        inspect(&reopened).get("status"),
        Some(&Json::text("applied-arrangement"))
    );
    fs::write(f.source.join("new/sub/run"), b"later editor work").unwrap();
    fs::write(f.source.join("new/user"), b"additional work").unwrap();
    assert_ne!(
        inspect(&reopened).get("status"),
        Some(&Json::text("applied-arrangement"))
    );
    fs::remove_file(path.join("observed.json")).unwrap();
    assert_eq!(
        inspect(&reopened).get("recorded_outcome"),
        Some(&Json::text("absent"))
    );
    assert_eq!(
        fs::read(f.source.join("new/sub/run")).unwrap(),
        b"later editor work"
    );
    assert_eq!(
        fs::read(f.source.join("new/user")).unwrap(),
        b"additional work"
    );
    assert_eq!(
        fs::read(f.source.join("work.txt")).unwrap(),
        b"original work"
    );
    assert_eq!(f.journal(), journal);
}

#[test]
fn directory_addition_refuses_concurrent_destination_or_changed_stage_without_cleanup() {
    for variant in [
        "directory",
        "file",
        "symlink",
        "staged-file",
        "extra-ignored",
        "stage-link",
        "policy",
    ] {
        let (f, _, trust, bundle, target, root) =
            approved_new_directory(&format!("directory-{variant}"));
        let prepared = f
            .history
            .prepare_main_directory_addition(
                &bundle,
                &target,
                "new",
                &root,
                &trust,
                ObservationLimits::default(),
            )
            .unwrap();
        let stage = prepared.recovery_path().join("exchange");
        match variant {
            "directory" => {
                fs::create_dir(f.source.join("new")).unwrap();
                fs::write(f.source.join("new/user"), b"keep").unwrap();
            }
            "file" => fs::write(f.source.join("new"), b"keep").unwrap(),
            "symlink" => std::os::unix::fs::symlink(&f.source, f.source.join("new")).unwrap(),
            "staged-file" => fs::write(stage.join("data"), b"changed stage").unwrap(),
            "extra-ignored" => {
                fs::create_dir(stage.join(".git")).unwrap();
                fs::write(stage.join(".git/config"), b"keep extra").unwrap();
            }
            "stage-link" => {
                fs::remove_file(stage.join("data")).unwrap();
                std::os::unix::fs::symlink(f.source.join("work.txt"), stage.join("data")).unwrap();
            }
            "policy" => {
                use std::os::unix::fs::PermissionsExt as _;
                let mode = fs::metadata(&f.source).unwrap().permissions().mode() ^ 0o010;
                fs::set_permissions(&f.source, fs::Permissions::from_mode(mode)).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(prepared.apply(&trust).is_err(), "{variant}");
        assert!(stage.exists(), "staging was cleaned up for {variant}");
        assert_eq!(
            fs::read(f.source.join("work.txt")).unwrap(),
            b"original work"
        );
        if variant == "directory" {
            assert_eq!(fs::read(f.source.join("new/user")).unwrap(), b"keep");
        }
        if variant == "file" {
            assert_eq!(fs::read(f.source.join("new")).unwrap(), b"keep");
        }
    }
}

#[test]
fn directory_addition_binds_approval_policy_budget_and_receipts() {
    let (f, signer, trust, bundle, target, root) = approved_new_directory("directory-bindings");
    let prepare = |trust: &TrustedReviewers| {
        f.history.prepare_main_directory_addition(
            &bundle,
            &target,
            "new",
            &root,
            trust,
            ObservationLimits::default(),
        )
    };
    assert!(prepare(&TrustedReviewers::default()).is_err());
    assert!(f
        .history
        .prepare_main_directory_addition(
            &bundle,
            &target,
            "../new",
            &root,
            &trust,
            ObservationLimits::default()
        )
        .is_err());
    assert!(f
        .history
        .prepare_main_directory_addition(
            &bundle,
            &target,
            "new",
            &root,
            &trust,
            ObservationLimits {
                file_bytes: 2,
                ..ObservationLimits::default()
            }
        )
        .is_err());
    let staged = prepare(&trust).unwrap();
    let path = staged.recovery_path().to_owned();
    let id = path.file_name().unwrap().to_str().unwrap();
    fs::write(path.join("prepared.json"), "{}").unwrap();
    assert!(staged.apply(&trust).is_err());
    assert!(f
        .history
        .inspect_directory_addition(&root, id, &trust, ObservationLimits::default())
        .is_err());
    let staged = prepare(&trust).unwrap();
    fs::write(f.source.join(".meshignore"), "new\n").unwrap();
    assert!(staged.apply(&trust).is_err());
    fs::remove_file(f.source.join(".meshignore")).unwrap();
    let staged = prepare(&trust).unwrap();
    let later = f.save("later approved main");
    accept(&f, &signer, &trust, &later, 3);
    assert!(staged.apply(&trust).is_err());
    assert!(!f.source.join("new").exists());
}

#[test]
fn directory_recovery_distinguishes_parent_replacement_and_policy_changes() {
    use std::os::unix::fs::PermissionsExt as _;
    let f = Fixture::new("tree-parent-recovery");
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    fs::create_dir(f.source.join("parent")).unwrap();
    let first = f.save("original");
    accept(&f, &signer, &trust, &first, 1);
    fs::create_dir(f.source.join("parent/new")).unwrap();
    fs::write(f.source.join("parent/new/file"), b"accepted").unwrap();
    let target = f.capture();
    let bundle = accept(&f, &signer, &trust, &target, 2);
    fs::remove_dir_all(f.source.join("parent/new")).unwrap();
    let root = recovery_root(&f);
    let prepared = f
        .history
        .prepare_main_directory_addition(
            &bundle,
            &target,
            "parent/new",
            &root,
            &trust,
            ObservationLimits::default(),
        )
        .unwrap();
    let id = prepared
        .recovery_path()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    prepared.apply(&trust).unwrap();
    let inspect = || {
        f.history
            .inspect_directory_addition(&root, &id, &trust, ObservationLimits::default())
            .unwrap()
    };
    let mode = fs::metadata(f.source.join("parent"))
        .unwrap()
        .permissions()
        .mode();
    fs::set_permissions(
        f.source.join("parent"),
        fs::Permissions::from_mode(mode ^ 0o010),
    )
    .unwrap();
    let bounded = f
        .history
        .inspect_directory_addition(
            &root,
            &id,
            &trust,
            ObservationLimits {
                file_bytes: 1,
                ..ObservationLimits::default()
            },
        )
        .unwrap();
    assert_eq!(
        bounded.get("source").unwrap().get("state"),
        Some(&Json::text("unavailable"))
    );
    assert_eq!(
        bounded.get("live_content_budget_remaining"),
        Some(&Json::Number(0))
    );
    let policy = inspect();
    assert_eq!(
        policy.get("status"),
        Some(&Json::text("parent-policy-changed"))
    );
    assert_eq!(
        policy.get("parent_identity_matches"),
        Some(&Json::Bool(true))
    );
    assert_eq!(
        policy.get("parent_policy_matches"),
        Some(&Json::Bool(false))
    );
    fs::set_permissions(f.source.join("parent"), fs::Permissions::from_mode(mode)).unwrap();
    fs::rename(f.source.join("parent"), f.source.join("former")).unwrap();
    fs::create_dir(f.source.join("parent")).unwrap();
    fs::rename(f.source.join("former/new"), f.source.join("parent/new")).unwrap();
    let moved = inspect();
    assert_eq!(
        moved.get("status"),
        Some(&Json::text("source-parent-changed"))
    );
    assert_eq!(
        moved.get("parent_identity_matches"),
        Some(&Json::Bool(false))
    );
    assert_eq!(
        fs::read(f.source.join("parent/new/file")).unwrap(),
        b"accepted"
    );
}

#[test]
fn directory_group_recovers_complete_coverage_and_preserves_concurrent_destination() {
    let (f, _, trust, bundle, target, root) = approved_new_directory("directory-group-race");
    let prepared = f
        .history
        .prepare_main_integration(
            &bundle,
            &target,
            &root,
            &trust,
            ObservationLimits::default(),
        )
        .unwrap();
    assert_eq!(
        prepared.proposal().get("schema"),
        Some(&Json::text("mesh.attachment-integration-group/v2"))
    );
    assert_eq!(prepared.directories().count(), 1);
    assert_eq!(prepared.files().count(), 0);
    let directory = prepared.directories().next().unwrap();
    assert_eq!(directory.proposed_files().count(), 2);
    let retained = directory.recovery_path().join("exchange");
    let group = prepared
        .recovery_path()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    fs::create_dir(f.source.join("new")).unwrap();
    fs::write(f.source.join("new/user"), b"concurrent").unwrap();
    assert!(prepared.apply(&trust).is_err());
    assert_eq!(fs::read(f.source.join("new/user")).unwrap(), b"concurrent");
    assert_eq!(fs::read(retained.join("data")).unwrap(), [0, 255, 12]);
    let reopened = f.storage.reopen(f.history.id()).unwrap();
    let recovery = reopened
        .inspect_main_integration_group(&root, &group, &trust, ObservationLimits::default())
        .unwrap();
    assert_eq!(
        recovery
            .get("members")
            .and_then(Json::as_array)
            .unwrap()
            .len(),
        1
    );
    let record = root.join(&group).join("group-prepared.json");
    let original = fs::read_to_string(&record).unwrap();
    // A v1 reader must never interpret a tree member as a regular-file operation.
    fs::write(
        &record,
        original.replace(
            "mesh.attachment-integration-group/v2",
            "mesh.attachment-integration-group/v1",
        ),
    )
    .unwrap();
    assert!(reopened
        .inspect_main_integration_group(&root, &group, &trust, ObservationLimits::default())
        .is_err());
    fs::write(&record, &original).unwrap();
    let proposal = Json::parse(&original).unwrap();
    let Json::Object(pairs) = proposal else {
        panic!("proposal object")
    };
    let changed = Json::object(pairs.iter().map(|(key, value)| {
        (
            key.clone(),
            if key == "already_present" {
                Json::Array(vec![Json::text("new/sub/run")])
            } else {
                value.clone()
            },
        )
    }));
    fs::write(&record, changed.encode()).unwrap();
    assert!(reopened
        .inspect_main_integration_group(&root, &group, &trust, ObservationLimits::default())
        .is_err());
}

fn approved_directory_removal(
    name: &str,
) -> (
    Fixture,
    TestSigner,
    TrustedReviewers,
    String,
    String,
    PathBuf,
) {
    let f = Fixture::new(name);
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    fs::create_dir_all(f.source.join("old/sub/empty")).unwrap();
    fs::write(f.source.join("old/sub/file"), "original tree").unwrap();
    let first = f.save("base");
    accept(&f, &signer, &trust, &first, 1);
    fs::remove_dir_all(f.source.join("old")).unwrap();
    let target = f.save("accepted");
    let bundle = accept(&f, &signer, &trust, &target, 2);
    fs::create_dir_all(f.source.join("old/sub/empty")).unwrap();
    fs::write(f.source.join("old/sub/file"), "original tree").unwrap();
    fs::write(f.source.join("work.txt"), "base").unwrap();
    let root = recovery_root(&f);
    (f, signer, trust, bundle, target, root)
}

#[test]
fn approved_directory_removal_group_retains_late_writes_and_reopens_without_replay() {
    let (f, _, trust, bundle, target, root) = approved_directory_removal("remove-tree-group");
    f.git(&["init", "--quiet"]);
    let git_before = f.git(&["status", "--porcelain=v1", "--untracked-files=all"]);
    let journal = f.journal();
    let mut editor = fs::OpenOptions::new()
        .append(true)
        .open(f.source.join("old/sub/file"))
        .unwrap();
    let prepared = f
        .history
        .prepare_main_integration(
            &bundle,
            &target,
            &root,
            &trust,
            ObservationLimits::default(),
        )
        .unwrap();
    assert_eq!(
        prepared.proposal().get("schema"),
        Some(&Json::text("mesh.attachment-integration-group/v3"))
    );
    let tree = prepared.directories().next().unwrap();
    assert_eq!(tree.confirmation_files().count(), 1);
    assert_eq!(tree.proposed_files().count(), 0);
    let retained = tree.recovery_path().join("exchange");
    let group = prepared
        .recovery_path()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    assert!(!retained.exists());
    let prepared_observation = f
        .history
        .inspect_main_integration_group(&root, &group, &trust, ObservationLimits::default())
        .unwrap();
    let members = prepared_observation
        .get("members")
        .and_then(Json::as_array)
        .unwrap();
    assert_eq!(
        members[0].get("recovery").unwrap().get("status"),
        Some(&Json::text("prepared-arrangement"))
    );
    assert_eq!(
        f.git(&["status", "--porcelain=v1", "--untracked-files=all"]),
        git_before
    );
    let result = prepared.apply(&trust).unwrap();
    assert_eq!(result.get("status"), Some(&Json::text("applied-observed")));
    assert!(!f.source.join("old").exists());
    assert!(retained.join("sub/empty").is_dir());
    assert_eq!(fs::read(f.source.join("work.txt")).unwrap(), b"accepted");
    let reopened = f.storage.reopen(f.history.id()).unwrap();
    let inspect = || {
        reopened
            .inspect_main_integration_group(&root, &group, &trust, ObservationLimits::default())
            .unwrap()
    };
    assert_eq!(
        inspect().get("members").and_then(Json::as_array).unwrap()[0]
            .get("recovery")
            .unwrap()
            .get("status"),
        Some(&Json::text("applied-arrangement"))
    );
    editor.write_all(b" late editor").unwrap();
    assert_eq!(
        fs::read(retained.join("sub/file")).unwrap(),
        b"original tree late editor"
    );
    assert_eq!(
        inspect().get("members").and_then(Json::as_array).unwrap()[0]
            .get("recovery")
            .unwrap()
            .get("status"),
        Some(&Json::text("changed-entries"))
    );
    assert_eq!(f.journal(), journal);
    let receipt = root.join(group).join("group-prepared.json");
    let text = fs::read_to_string(&receipt).unwrap();
    fs::write(
        &receipt,
        text.replace(
            "mesh.attachment-integration-group/v3",
            "mesh.attachment-integration-group/v2",
        ),
    )
    .unwrap();
    assert!(reopened
        .inspect_main_integration_group(
            &root,
            receipt
                .parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_str()
                .unwrap(),
            &trust,
            ObservationLimits::default()
        )
        .is_err());
}

#[test]
fn directory_removal_refuses_ignored_extra_changed_and_linked_work() {
    for variant in ["extra", "ignored", "symlink", "changed", "after-prepare"] {
        let (f, _, trust, bundle, target, root) =
            approved_directory_removal(&format!("remove-tree-{variant}"));
        let change = || match variant {
            "ignored" => {
                fs::create_dir(f.source.join("old/.git")).unwrap();
                fs::write(f.source.join("old/.git/config"), b"keep").unwrap();
            }
            "symlink" => {
                std::os::unix::fs::symlink(f.source.join("work.txt"), f.source.join("old/link"))
                    .unwrap()
            }
            "changed" => fs::write(f.source.join("old/sub/file"), b"user edit").unwrap(),
            _ => fs::write(f.source.join("old/user"), b"keep").unwrap(),
        };
        if variant != "after-prepare" {
            change();
        }
        let prepared = f.history.prepare_main_integration(
            &bundle,
            &target,
            &root,
            &trust,
            ObservationLimits::default(),
        );
        if variant == "after-prepare" {
            let prepared = prepared.unwrap();
            change();
            assert!(prepared.apply(&trust).is_err());
        } else {
            assert!(prepared.is_err(), "{variant}");
        }
        assert!(f.source.join("old/sub/empty").is_dir());
        assert_eq!(fs::read(f.source.join("work.txt")).unwrap(), b"base");
    }
}

#[test]
fn native_directory_removal_checks_operation_kind_and_retention_receipt() {
    let (f, signer, trust, bundle, target, root) = approved_directory_removal("remove-tree-native");
    assert!(f
        .history
        .prepare_main_directory_addition(
            &bundle,
            &target,
            "old",
            &root,
            &trust,
            ObservationLimits::default()
        )
        .is_err());
    assert!(f
        .history
        .prepare_main_directory_removal(
            &bundle,
            &target,
            "old",
            &root,
            &TrustedReviewers::new([]),
            ObservationLimits::default()
        )
        .is_err());
    let prepared = f
        .history
        .prepare_main_directory_removal(
            &bundle,
            &target,
            "old",
            &root,
            &trust,
            ObservationLimits::default(),
        )
        .unwrap();
    let record = prepared.recovery_path().to_owned();
    let id = record.file_name().unwrap().to_str().unwrap();
    let outcome = prepared.apply(&trust).unwrap();
    assert_eq!(
        outcome.get("displaced_entry_retained"),
        Some(&Json::Bool(true))
    );
    assert_eq!(
        outcome.get("schema"),
        Some(&Json::text("mesh.attachment-directory-removal-result/v1"))
    );
    // Historical read remains valid after a later accepted main, and never replays removal.
    let newer = f.save("new accepted");
    accept(&f, &signer, &trust, &newer, 3);
    let inspect = || {
        f.history
            .inspect_directory_change(&root, id, &trust, ObservationLimits::default())
            .unwrap()
    };
    assert_eq!(
        inspect().get("status"),
        Some(&Json::text("applied-arrangement"))
    );
    fs::write(
        record.join("observed.json"),
        outcome.encode().replace(
            "\"displaced_entry_retained\":true",
            "\"displaced_entry_retained\":false",
        ),
    )
    .unwrap();
    assert_eq!(
        inspect().get("status"),
        Some(&Json::text("invalid-outcome"))
    );
    fs::remove_file(record.join("observed.json")).unwrap();
    assert_eq!(
        inspect().get("recorded_outcome"),
        Some(&Json::text("absent"))
    );
    fs::create_dir(f.source.join("old")).unwrap();
    fs::write(f.source.join("old/user"), b"keep").unwrap();
    inspect();
    assert_eq!(fs::read(f.source.join("old/user")).unwrap(), b"keep");
    assert_eq!(
        fs::read(record.join("exchange/sub/file")).unwrap(),
        b"original tree"
    );
}

fn approved_entry_conversion(
    name: &str,
    directory_before: bool,
) -> (
    Fixture,
    TestSigner,
    TrustedReviewers,
    String,
    String,
    PathBuf,
) {
    let f = Fixture::new(name);
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    let set_entry = |directory: bool, text: &str| {
        if directory {
            fs::create_dir_all(f.source.join("entry/empty")).unwrap();
            fs::write(f.source.join("entry/file"), text).unwrap();
        } else {
            fs::write(f.source.join("entry"), text).unwrap();
        }
    };
    let remove_entry = |directory: bool| {
        if directory {
            fs::remove_dir_all(f.source.join("entry")).unwrap();
        } else {
            fs::remove_file(f.source.join("entry")).unwrap();
        }
    };
    set_entry(directory_before, "original");
    let base = f.save("stable");
    accept(&f, &signer, &trust, &base, 1);
    remove_entry(directory_before);
    set_entry(!directory_before, "accepted");
    let target = f.capture();
    let bundle = accept(&f, &signer, &trust, &target, 2);
    remove_entry(!directory_before);
    set_entry(directory_before, "original");
    let root = recovery_root(&f);
    (f, signer, trust, bundle, target, root)
}

#[test]
fn approved_type_conversion_binds_both_sides_and_retains_late_work() {
    for directory_before in [false, true] {
        let (f, signer, trust, bundle, target, root) = approved_entry_conversion(
            &format!("native-conversion-{directory_before}"),
            directory_before,
        );
        assert!(f
            .history
            .prepare_main_directory_addition(
                &bundle,
                &target,
                "entry",
                &root,
                &trust,
                ObservationLimits::default()
            )
            .is_err());
        let journal = f.journal();
        let prepared = f
            .history
            .prepare_main_entry_conversion(
                &bundle,
                &target,
                "entry",
                &root,
                &trust,
                ObservationLimits::default(),
            )
            .unwrap();
        assert_eq!(prepared.current_files().count(), 1);
        assert_eq!(prepared.proposed_files().count(), 1);
        let record = prepared.recovery_path().to_owned();
        let tx = record.file_name().unwrap().to_str().unwrap();
        let inspect = || {
            f.history
                .inspect_directory_change(&root, tx, &trust, ObservationLimits::default())
                .unwrap()
        };
        assert_eq!(
            inspect().get("status"),
            Some(&Json::text("prepared-arrangement"))
        );
        let mut editor = fs::OpenOptions::new()
            .append(true)
            .open(if directory_before {
                f.source.join("entry/file")
            } else {
                f.source.join("entry")
            })
            .unwrap();
        let outcome = prepared.apply(&trust).unwrap();
        assert_eq!(outcome.get("status"), Some(&Json::text("applied-observed")));
        assert_eq!(
            outcome.get("displaced_entry_retained"),
            Some(&Json::Bool(true))
        );
        assert_eq!(
            inspect().get("status"),
            Some(&Json::text("applied-arrangement"))
        );
        editor.write_all(b" late").unwrap();
        assert_eq!(
            fs::read(if directory_before {
                record.join("exchange/file")
            } else {
                record.join("exchange")
            })
            .unwrap(),
            b"original late"
        );
        assert_eq!(
            fs::read(if directory_before {
                f.source.join("entry")
            } else {
                f.source.join("entry/file")
            })
            .unwrap(),
            b"accepted"
        );
        assert_eq!(
            inspect().get("status"),
            Some(&Json::text("changed-entries"))
        );
        assert_eq!(f.journal(), journal);
        let next = f.save("new main");
        accept(&f, &signer, &trust, &next, 3);
        inspect();
    }
}

#[test]
fn conversion_groups_cover_both_sides_and_refuse_legacy_or_overlapping_membership() {
    for directory_before in [false, true] {
        let (f, _, trust, bundle, target, root) = approved_entry_conversion(
            &format!("group-conversion-{directory_before}"),
            directory_before,
        );
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
        assert_eq!(
            group.proposal().get("schema"),
            Some(&Json::text("mesh.attachment-integration-group/v4"))
        );
        assert_eq!(group.files().count(), 0);
        assert_eq!(group.directories().count(), 1);
        let path = group.recovery_path().to_owned();
        let id = path.file_name().unwrap().to_str().unwrap();
        let inspect = || {
            f.history
                .inspect_main_integration_group(&root, id, &trust, ObservationLimits::default())
                .unwrap()
        };
        inspect();
        assert_eq!(
            group.apply(&trust).unwrap().get("status"),
            Some(&Json::text("applied-observed"))
        );
        let observed = inspect();
        let member = &observed.get("members").and_then(Json::as_array).unwrap()[0];
        assert_eq!(
            member.get("recovery").unwrap().get("status"),
            Some(&Json::text("applied-arrangement"))
        );
        assert_eq!(
            member.get("recovery").unwrap().get("before_kind"),
            Some(&Json::text(if directory_before {
                "directory"
            } else {
                "file"
            }))
        );
        let receipt = path.join("group-prepared.json");
        let original = fs::read_to_string(&receipt).unwrap();
        for version in ["v1", "v2", "v3"] {
            fs::write(
                &receipt,
                original.replace(
                    "mesh.attachment-integration-group/v4",
                    &format!("mesh.attachment-integration-group/{version}"),
                ),
            )
            .unwrap();
            assert!(f
                .history
                .inspect_main_integration_group(&root, id, &trust, ObservationLimits::default())
                .is_err());
        }
        fs::write(&receipt, &original).unwrap();
        let Json::Object(fields) = Json::parse(&original).unwrap() else {
            panic!("group object")
        };
        let overlapping = Json::object(fields.into_iter().map(|(key, value)| {
            let value = if key == "already_present" {
                Json::Array(vec![Json::text("entry/empty")])
            } else {
                value
            };
            (key, value)
        }));
        fs::write(&receipt, overlapping.encode()).unwrap();
        assert!(f
            .history
            .inspect_main_integration_group(&root, id, &trust, ObservationLimits::default())
            .is_err());
    }
}

#[test]
fn conversion_refuses_unknown_source_entries_changed_stage_and_untrusted_history() {
    for directory_before in [false, true] {
        let (f, _, trust, bundle, target, root) = approved_entry_conversion(
            &format!("conversion-refusal-{directory_before}"),
            directory_before,
        );
        assert!(f
            .history
            .prepare_main_entry_conversion(
                &bundle,
                &target,
                "entry",
                &root,
                &TrustedReviewers::new([]),
                ObservationLimits::default()
            )
            .is_err());
        assert!(f
            .history
            .prepare_main_entry_conversion(
                &bundle,
                &target,
                "entry",
                &root,
                &trust,
                ObservationLimits {
                    bytes: 1,
                    file_bytes: 1,
                    ..ObservationLimits::default()
                }
            )
            .is_err());
        let prepared = f
            .history
            .prepare_main_entry_conversion(
                &bundle,
                &target,
                "entry",
                &root,
                &trust,
                ObservationLimits::default(),
            )
            .unwrap();
        let stage = prepared.recovery_path().join("exchange");
        fs::write(
            if directory_before {
                stage.clone()
            } else {
                stage.join("file")
            },
            b"unreviewed stage",
        )
        .unwrap();
        assert!(prepared.apply(&trust).is_err());
        assert_eq!(
            fs::read(if directory_before {
                f.source.join("entry/file")
            } else {
                f.source.join("entry")
            })
            .unwrap(),
            b"original"
        );
        if directory_before {
            fs::create_dir(f.source.join("entry/.git")).unwrap();
            fs::write(f.source.join("entry/.git/config"), b"ignored work").unwrap();
        } else {
            fs::write(f.source.join("entry"), b"new source").unwrap();
        }
        assert!(f
            .history
            .prepare_main_integration(
                &bundle,
                &target,
                &root,
                &trust,
                ObservationLimits::default()
            )
            .is_err());
        assert!(stage.exists());
    }
}

#[test]
fn whole_entry_restoration_copies_late_retained_work_and_preserves_both_generations() {
    for directory_before in [false, true] {
        let (f, signer, trust, bundle, target, root) = approved_entry_conversion(
            &format!("restore-entry-{directory_before}"),
            directory_before,
        );
        let conversion = f
            .history
            .prepare_main_entry_conversion(
                &bundle,
                &target,
                "entry",
                &root,
                &trust,
                ObservationLimits::default(),
            )
            .unwrap();
        let origin = conversion.recovery_path().to_owned();
        let id = origin.file_name().unwrap().to_str().unwrap();
        conversion.apply(&trust).unwrap();
        let retained = origin.join(if directory_before {
            "exchange/file"
        } else {
            "exchange"
        });
        fs::write(&retained, b"late retained work").unwrap();
        if directory_before {
            fs::write(origin.join("exchange/extra"), b"new retained child").unwrap();
        }
        let current = f.source.join(if directory_before {
            "entry"
        } else {
            "entry/file"
        });
        fs::write(&current, b"new current work").unwrap();
        let mut editor = fs::OpenOptions::new().append(true).open(&current).unwrap();
        let next = f.save("main advances independently");
        accept(&f, &signer, &trust, &next, 3);
        let journal = f.journal();
        let restore = f
            .history
            .prepare_retained_entry_restoration(&root, id, &trust, ObservationLimits::default())
            .unwrap();
        assert_eq!(
            restore
                .restored_files()
                .find(|(_, bytes, _)| *bytes == b"late retained work")
                .unwrap()
                .1,
            b"late retained work"
        );
        assert_eq!(
            restore.current_files().next().unwrap().1,
            b"new current work"
        );
        let recovery = restore.recovery_path().to_owned();
        let inspect = || {
            f.history
                .inspect_retained_entry_restoration(
                    &root,
                    recovery.file_name().unwrap().to_str().unwrap(),
                    &trust,
                    ObservationLimits::default(),
                )
                .unwrap()
        };
        assert_eq!(
            inspect().get("status"),
            Some(&Json::text("prepared-arrangement"))
        );
        let outcome = restore.apply(&trust).unwrap();
        assert_eq!(outcome.get("status"), Some(&Json::text("applied-observed")));
        assert_eq!(
            inspect().get("status"),
            Some(&Json::text("applied-arrangement"))
        );
        fs::remove_file(recovery.join("observed.json")).unwrap();
        assert_eq!(
            inspect().get("recorded_outcome"),
            Some(&Json::text("absent"))
        );
        assert_eq!(
            inspect().get("status"),
            Some(&Json::text("applied-arrangement"))
        );
        assert_eq!(fs::read(&retained).unwrap(), b"late retained work");
        editor.write_all(b" after restoration").unwrap();
        assert_eq!(
            inspect().get("status"),
            Some(&Json::text("changed-entries"))
        );
        assert_eq!(
            fs::read(recovery.join(if directory_before {
                "exchange"
            } else {
                "exchange/file"
            }))
            .unwrap(),
            b"new current work after restoration"
        );
        assert_eq!(
            fs::read(f.source.join(if directory_before {
                "entry/file"
            } else {
                "entry"
            }))
            .unwrap(),
            b"late retained work"
        );
        if directory_before {
            assert!(f.source.join("entry/empty").is_dir());
            assert_eq!(
                fs::read(f.source.join("entry/extra")).unwrap(),
                b"new retained child"
            );
        }
        assert_eq!(f.journal(), journal);
        // Undo selects the new transaction's retained entry, including late editor work. It does
        // not reverse either exchange and can be prepared after reopening attachment storage.
        let reopened = f.storage.reopen(f.history.id()).unwrap();
        let undo = reopened
            .prepare_retained_entry_restoration(
                &root,
                recovery.file_name().unwrap().to_str().unwrap(),
                &trust,
                ObservationLimits::default(),
            )
            .unwrap();
        undo.apply(&trust).unwrap();
        assert_eq!(
            fs::read(current).unwrap(),
            b"new current work after restoration"
        );
        assert_eq!(fs::read(retained).unwrap(), b"late retained work");
        assert_eq!(f.journal(), journal);
    }
}

#[test]
fn retained_removed_tree_restores_to_absence_without_consuming_the_original_tree() {
    let (f, _, trust, bundle, target, root) = approved_directory_removal("restore-removed-tree");
    let removal = f
        .history
        .prepare_main_directory_removal(
            &bundle,
            &target,
            "old",
            &root,
            &trust,
            ObservationLimits::default(),
        )
        .unwrap();
    let origin = removal.recovery_path().to_owned();
    removal.apply(&trust).unwrap();
    fs::write(origin.join("exchange/sub/new"), b"late retained child").unwrap();
    let restore = f
        .history
        .prepare_retained_entry_restoration(
            &root,
            origin.file_name().unwrap().to_str().unwrap(),
            &trust,
            ObservationLimits::default(),
        )
        .unwrap();
    assert_eq!(restore.proposal().get("current_tree"), Some(&Json::Null));
    assert_eq!(restore.current_files().count(), 0);
    let recovery = restore.recovery_path().to_owned();
    assert_eq!(
        restore
            .apply(&trust)
            .unwrap()
            .get("displaced_entry_retained"),
        Some(&Json::Bool(false))
    );
    assert_eq!(
        fs::read(f.source.join("old/sub/new")).unwrap(),
        b"late retained child"
    );
    assert!(f.source.join("old/sub/empty").is_dir());
    assert!(origin.join("exchange/sub/file").exists());
    assert!(!recovery.join("exchange").exists());
    let id = recovery.file_name().unwrap().to_str().unwrap();
    let report = f
        .history
        .inspect_retained_entry_restoration(&root, id, &trust, ObservationLimits::default())
        .unwrap();
    assert_eq!(
        report.get("status"),
        Some(&Json::text("applied-arrangement"))
    );
    assert_eq!(report.get("write_authority"), Some(&Json::Bool(false)));
    assert!(f
        .history
        .prepare_retained_entry_restoration(&root, id, &trust, ObservationLimits::default())
        .is_err());
    fs::write(origin.join("exchange/sub/new"), b"later origin").unwrap();
    assert_eq!(
        f.history
            .inspect_retained_entry_restoration(&root, id, &trust, ObservationLimits::default())
            .unwrap()
            .get("status"),
        Some(&Json::text("origin-changed"))
    );
    fs::write(recovery.join("observed.json"), b"{}").unwrap();
    assert_eq!(
        f.history
            .inspect_retained_entry_restoration(&root, id, &trust, ObservationLimits::default())
            .unwrap()
            .get("status"),
        Some(&Json::text("invalid-outcome"))
    );
}

#[test]
fn entry_restoration_refuses_changed_private_work_staging_and_untrusted_origins() {
    for changed in [
        "origin",
        "destination",
        "stage",
        "receipt",
        "unknown",
        "identity",
    ] {
        let (f, _, trust, bundle, target, root) =
            approved_entry_conversion(&format!("restore-entry-refuse-{changed}"), true);
        let conversion = f
            .history
            .prepare_main_entry_conversion(
                &bundle,
                &target,
                "entry",
                &root,
                &trust,
                ObservationLimits::default(),
            )
            .unwrap();
        let origin = conversion.recovery_path().to_owned();
        conversion.apply(&trust).unwrap();
        let id = origin.file_name().unwrap().to_str().unwrap();
        assert!(f
            .history
            .prepare_retained_entry_restoration(
                &root,
                id,
                &TrustedReviewers::new([]),
                ObservationLimits::default()
            )
            .is_err());
        if changed == "unknown" {
            fs::create_dir(origin.join("exchange/.git")).unwrap();
            fs::write(origin.join("exchange/.git/config"), b"private").unwrap();
            assert!(f
                .history
                .prepare_retained_entry_restoration(&root, id, &trust, ObservationLimits::default())
                .is_err());
            for entry in fs::read_dir(&root).unwrap() {
                let entry = entry.unwrap();
                if entry
                    .file_name()
                    .to_str()
                    .unwrap()
                    .starts_with("entry-restoration-")
                {
                    assert!(
                        !entry.path().join("exchange").exists(),
                        "excluded content must not be copied into a new stage"
                    );
                }
            }
            assert_eq!(fs::read(f.source.join("entry")).unwrap(), b"accepted");
            continue;
        }
        if changed == "identity" {
            fs::rename(origin.join("exchange"), origin.join("moved")).unwrap();
            fs::create_dir_all(origin.join("exchange/empty")).unwrap();
            fs::write(origin.join("exchange/file"), b"original").unwrap();
            assert!(f
                .history
                .prepare_retained_entry_restoration(&root, id, &trust, ObservationLimits::default())
                .is_err());
            continue;
        }
        let restore = f
            .history
            .prepare_retained_entry_restoration(&root, id, &trust, ObservationLimits::default())
            .unwrap();
        let recovery = restore.recovery_path().to_owned();
        let changed_path = match changed {
            "origin" => origin.join("exchange/file"),
            "destination" => f.source.join("entry"),
            "stage" => recovery.join("exchange/file"),
            _ => origin.join("prepared.json"),
        };
        fs::write(&changed_path, b"changed work").unwrap();
        assert!(restore.apply(&trust).is_err());
        assert_eq!(fs::read(&changed_path).unwrap(), b"changed work");
        assert!(origin.join("exchange/file").exists());
        assert!(recovery.join("exchange/file").exists());
        if changed != "destination" {
            assert_eq!(fs::read(f.source.join("entry")).unwrap(), b"accepted");
        }
    }
}

#[test]
fn whole_entry_discovery_reopens_exact_records_without_treating_names_as_authority() {
    for grouped in [false, true] {
        let (f, _, trust, bundle, target, outer) =
            approved_entry_conversion(&format!("entry-discovery-{grouped}"), false);
        let (root, origin, group_id) = if grouped {
            let prepared = f
                .history
                .prepare_main_integration(
                    &bundle,
                    &target,
                    &outer,
                    &trust,
                    ObservationLimits::default(),
                )
                .unwrap();
            let root = prepared.recovery_path().to_owned();
            let origin = prepared
                .directories()
                .next()
                .unwrap()
                .recovery_path()
                .to_owned();
            let id = root.file_name().unwrap().to_str().unwrap().to_owned();
            prepared.apply(&trust).unwrap();
            (root, origin, Some(id))
        } else {
            let prepared = f
                .history
                .prepare_main_entry_conversion(
                    &bundle,
                    &target,
                    "entry",
                    &outer,
                    &trust,
                    ObservationLimits::default(),
                )
                .unwrap();
            let origin = prepared.recovery_path().to_owned();
            prepared.apply(&trust).unwrap();
            (outer.clone(), origin, None)
        };
        let original_id = origin.file_name().unwrap().to_str().unwrap();
        let journal = f.journal();
        let prepared = f
            .history
            .prepare_retained_entry_restoration(
                &root,
                original_id,
                &trust,
                ObservationLimits::default(),
            )
            .unwrap();
        let restored = prepared.recovery_path().to_owned();
        let restored_id = restored.file_name().unwrap().to_str().unwrap();
        prepared.apply(&trust).unwrap();
        let page = f
            .history
            .inspect_integration_recovery(&root, None, &trust, ObservationLimits::default())
            .unwrap();
        let references = page
            .get("entry_references")
            .and_then(Json::as_array)
            .unwrap();
        assert!(references.contains(&Json::text(original_id)));
        assert!(references.contains(&Json::text(restored_id)));
        assert_eq!(page.get("write_authority"), Some(&Json::Bool(false)));
        for (id, schema) in [
            (original_id, "mesh.attachment-entry-conversion-recovery/v1"),
            (restored_id, "mesh.attachment-entry-restoration-recovery/v1"),
        ] {
            let selected = f
                .history
                .inspect_integration_recovery(&root, Some(id), &trust, ObservationLimits::default())
                .unwrap();
            assert_eq!(selected.get("schema"), Some(&Json::text(schema)));
            assert_eq!(selected.get("write_authority"), Some(&Json::Bool(false)));
        }
        if let Some(group) = group_id {
            let reopened = f
                .history
                .inspect_main_integration_group(
                    &outer,
                    &group,
                    &trust,
                    ObservationLimits::default(),
                )
                .unwrap();
            assert!(reopened
                .get("entry_restoration_references")
                .and_then(Json::as_array)
                .unwrap()
                .contains(&Json::text(restored_id)));
            assert!(!reopened
                .get("restoration_references")
                .and_then(Json::as_array)
                .unwrap()
                .contains(&Json::text(restored_id)));
        }
        let fake = format!("entry-restoration-{}", "f".repeat(32));
        fs::create_dir(root.join(&fake)).unwrap();
        let page = f
            .history
            .inspect_integration_recovery(&root, None, &trust, ObservationLimits::default())
            .unwrap();
        assert!(page
            .get("entry_references")
            .and_then(Json::as_array)
            .unwrap()
            .contains(&Json::text(&fake)));
        for id in [
            fake.as_str(),
            "../entry-restoration",
            "entry-restoration-INVALID",
        ] {
            assert!(f
                .history
                .inspect_integration_recovery(&root, Some(id), &trust, ObservationLimits::default())
                .is_err());
        }
        let bounded = f
            .history
            .inspect_integration_recovery(
                &root,
                None,
                &trust,
                ObservationLimits {
                    entries: 1,
                    ..ObservationLimits::default()
                },
            )
            .unwrap();
        assert_eq!(bounded.get("more"), Some(&Json::Bool(true)));
        assert!(
            bounded
                .get("entries")
                .and_then(Json::as_array)
                .unwrap()
                .len()
                + bounded
                    .get("entry_references")
                    .and_then(Json::as_array)
                    .unwrap()
                    .len()
                <= 1
        );
        assert_eq!(f.journal(), journal);
        assert!(origin.join("exchange").exists());
        assert!(restored.join("exchange").exists());
    }
}

#[test]
fn dependency_preparation_fences_valid_attached_approval_and_capture_without_source_changes() {
    let f = Fixture::new("dependency-preparation");
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    let target = f.save("exact attached input");
    let bundle = f.request(&target, &trust);
    let receipt = f.receipt(&signer, &trust, &bundle, &target, 1);
    let binding = f.history.metadata_path().join("attachment-history.json");
    let original = fs::read_to_string(&binding).unwrap();
    let journal = f.journal();
    let authority = mesh_store::RecordDigest::from_bytes([1; 32]);
    let fence = f.history.prepare_dependency_enrollment(authority).unwrap();
    fence.ensure_current().unwrap();
    let required = fs::read(&binding).unwrap();
    assert_eq!(
        Json::parse(std::str::from_utf8(&required).unwrap())
            .unwrap()
            .get("previous_binding"),
        Some(&Json::text(original))
    );
    drop(fence);
    assert!(f
        .history
        .approve_review(&bundle, &target, &receipt, &trust)
        .is_err());
    assert!(f
        .history
        .approve_review(&bundle, &target, &receipt, &trust)
        .is_err());
    assert_eq!(f.journal(), journal);
    let input = f
        .history
        .project()
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    let called = std::cell::Cell::new(false);
    assert!(f
        .history
        .project()
        .save_capture(
            f.history.metadata_path(),
            &input,
            mesh_types::PublicKey::from_bytes([7; 32]),
            |_| {
                called.set(true);
                Ok::<_, &'static str>(mesh_types::Signature::from_bytes([0; 64]))
            },
        )
        .is_err());
    assert!(!called.get());
    assert_eq!(f.journal(), journal);
    assert_eq!(
        fs::read(f.source.join("work.txt")).unwrap(),
        b"exact attached input"
    );
    // Ordinary editors remain usable even though unsupported Mesh history writers are fenced.
    fs::write(f.source.join("work.txt"), b"editor continues").unwrap();
    drop(f.history.prepare_dependency_enrollment(authority).unwrap());
    assert_eq!(fs::read(&binding).unwrap(), required);
    assert!(f
        .history
        .prepare_dependency_enrollment(mesh_store::RecordDigest::from_bytes([2; 32]))
        .is_err());
    assert_eq!(fs::read(&binding).unwrap(), required);
    assert_eq!(f.journal(), journal);
    assert_eq!(
        fs::read(f.source.join("work.txt")).unwrap(),
        b"editor continues"
    );
}

#[test]
fn attachment_dependency_fence_rejects_changed_binding_and_replaced_source() {
    let f = Fixture::new("dependency-replacement");
    f.save("retained");
    let authority = mesh_store::RecordDigest::from_bytes([1; 32]);
    let fence = f.history.prepare_dependency_enrollment(authority).unwrap();
    let binding = f.history.metadata_path().join("attachment-history.json");
    let bytes = fs::read(&binding).unwrap();
    fs::write(&binding, b"conflicting work").unwrap();
    assert!(fence.ensure_current().is_err());
    fs::write(&binding, &bytes).unwrap();
    fence.ensure_current().unwrap();
    fs::rename(&f.source, f.root.join("parked")).unwrap();
    fs::create_dir(&f.source).unwrap();
    assert!(fence.ensure_current().is_err());
    drop(fence);
    assert!(f.history.prepare_dependency_enrollment(authority).is_err());
    assert_eq!(fs::read(&binding).unwrap(), bytes);
}
