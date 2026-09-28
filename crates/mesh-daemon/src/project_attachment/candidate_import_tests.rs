use super::*;
use crate::project_attachment::{AttachmentStorage, CandidateAdmission, ObservationLimits};
use crate::CheckpointSigner;
use ed25519_dalek::{Signer as _, SigningKey};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Signer {
    key: SigningKey,
    calls: AtomicUsize,
    refuse: bool,
}
impl CheckpointSigner for Signer {
    fn public_key(&self) -> PublicKey {
        PublicKey::from_bytes(self.key.verifying_key().to_bytes())
    }
    fn sign(&self, payload: &SigningPayload) -> Result<Signature, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.refuse {
            return Err("signing disabled".into());
        }
        Ok(Signature::from_bytes(
            self.key.sign(payload.as_bytes()).to_bytes(),
        ))
    }
}
impl CandidateImportSigner for Signer {
    fn sign_import_provenance(&self, payload: &SigningPayload) -> Result<Signature, String> {
        self.sign(payload)
    }
}
struct Fixture {
    root: PathBuf,
    source: PathBuf,
    history: ProvisionedAttachment,
    base: RecordDigest,
    snapshot: HistoricalWorkspacePreview,
    origins: BTreeMap<String, String>,
    candidate: Json,
    capture: Signer,
}
impl Fixture {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "mesh-import-recovery-{name}-{}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let source = root.join("source");
        let metadata = root.join("metadata");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&metadata).unwrap();
        fs::write(source.join("work.txt"), b"original").unwrap();
        let history = AttachmentStorage::open(&metadata)
            .unwrap()
            .provision(&source)
            .unwrap();
        let capture = Signer {
            key: SigningKey::from_bytes(&[71; 32]),
            calls: AtomicUsize::new(0),
            refuse: false,
        };
        let save = || {
            history
                .project()
                .save_capture(
                    history.metadata_path(),
                    &history
                        .project()
                        .capture_inputs(ObservationLimits::default())
                        .unwrap(),
                    capture.public_key(),
                    |payload| capture.sign(payload),
                )
                .unwrap()
        };
        let base = save().operation();
        fs::write(source.join("work.txt"), b"candidate saved bytes").unwrap();
        let result = save().operation();
        let open = OpenWorkspace::open_attachment_store(
            history.metadata_path(),
            history.store.clone(),
            false,
        )
        .unwrap();
        let snapshot = open.historical_workspace_preview(result).unwrap();
        let origins = snapshot
            .files
            .iter()
            .map(|file| (file.object.to_string(), file.object.to_string()))
            .collect();
        let provenance = Json::object([
            ("source_version", Json::text(base.to_string())),
            ("expected_main", Json::Null),
        ]);
        let candidate = history
            .stage_fleet_candidate(
                &"a".repeat(32),
                &provenance,
                &open,
                &snapshot,
                CandidateAdmission::Stage { main_matches: true },
                || Ok(()),
            )
            .unwrap();
        Self {
            root,
            source,
            history,
            base,
            snapshot,
            origins,
            candidate,
            capture,
        }
    }
    fn plan(&self, actor: PublicKey) -> PreparedProjectCandidateImport {
        let open = OpenWorkspace::open_attachment_store(
            self.history.metadata_path(),
            self.history.store.clone(),
            false,
        )
        .unwrap();
        crate::fleet::project_import::compile(
            &open,
            self.base,
            &open,
            &self.snapshot,
            &self.origins,
            &self.candidate,
            actor,
        )
        .unwrap()
    }
    fn receipt(&self) -> PathBuf {
        self.history
            .metadata_path()
            .join("fleet-candidates")
            .join(text(&self.candidate, "candidate").unwrap())
            .join(RECORD)
    }
    fn inspect(&self, actor: PublicKey) -> io::Result<Option<Json>> {
        self.history.inspect_fleet_import(
            &"a".repeat(32),
            &self.candidate,
            &self.snapshot,
            actor,
            &TrustedReviewers::default(),
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[test]
fn nonprivate_or_oversized_receipts_preserve_pending_work_without_resigning() {
    let f = Fixture::new("receipt-bounds");
    let signer = Signer {
        key: SigningKey::from_bytes(&[75; 32]),
        calls: AtomicUsize::new(0),
        refuse: false,
    };
    let actor = signer.public_key();
    assert!(f
        .history
        .commit_fleet_import_with(
            &"a".repeat(32),
            &f.candidate,
            f.plan(actor),
            &signer,
            &TrustedReviewers::default(),
            || Err(error("interrupted after durable intent"))
        )
        .is_err());
    let original = fs::read(f.receipt()).unwrap();
    let open = OpenWorkspace::open_attachment_store(
        f.history.metadata_path(),
        f.history.store.clone(),
        false,
    )
    .unwrap();
    let journal = open.record_file().to_owned();
    let before = fs::read(&journal).unwrap();
    drop(open);
    let source = fs::read(f.source.join("work.txt")).unwrap();
    for (bytes, mode) in [
        (original.clone(), 0o644),
        (vec![b' '; LIMIT as usize + 1], 0o600),
    ] {
        fs::write(f.receipt(), &bytes).unwrap();
        fs::set_permissions(f.receipt(), fs::Permissions::from_mode(mode)).unwrap();
        assert!(f.inspect(actor).is_err());
        assert!(f
            .history
            .commit_fleet_import(
                &"a".repeat(32),
                &f.candidate,
                f.plan(actor),
                &signer,
                &TrustedReviewers::default()
            )
            .is_err());
        assert_eq!(fs::read(f.receipt()).unwrap(), bytes);
        assert_eq!(
            fs::metadata(f.receipt()).unwrap().permissions().mode() & 0o777,
            mode
        );
        assert_eq!(fs::read(&journal).unwrap(), before);
        assert_eq!(fs::read(f.source.join("work.txt")).unwrap(), source);
        assert_eq!(signer.calls.load(Ordering::SeqCst), 2);
    }
    fs::write(f.receipt(), original).unwrap();
    fs::set_permissions(f.receipt(), fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        text(&f.inspect(actor).unwrap().unwrap(), "state").unwrap(),
        "pending"
    );
    assert_eq!(fs::read(&journal).unwrap(), before);
}
#[test]
fn pending_signed_import_is_read_only_until_explicit_retry_and_never_resigns() {
    let f = Fixture::new("pending");
    let signer = Signer {
        key: SigningKey::from_bytes(&[72; 32]),
        calls: AtomicUsize::new(0),
        refuse: false,
    };
    let actor = signer.public_key();
    let versions = f
        .history
        .project()
        .saved_versions(f.history.metadata_path())
        .unwrap();
    assert!(f.inspect(actor).unwrap().is_none());
    assert!(f
        .history
        .commit_fleet_import_with(
            &"a".repeat(32),
            &f.candidate,
            f.plan(actor),
            &signer,
            &TrustedReviewers::default(),
            || Err(error("interrupted after intent"))
        )
        .is_err());
    assert_eq!(signer.calls.load(Ordering::SeqCst), 2);
    let receipt = fs::read(f.receipt()).unwrap();
    let pending = f.inspect(actor).unwrap().unwrap();
    assert_eq!(text(&pending, "state").unwrap(), "pending");
    assert_eq!(fs::read(f.receipt()).unwrap(), receipt);
    assert_eq!(
        f.history
            .project()
            .saved_versions(f.history.metadata_path())
            .unwrap(),
        versions
    );
    fs::write(f.source.join("work.txt"), b"ongoing editor work").unwrap();
    let retry = Signer {
        key: SigningKey::from_bytes(&[72; 32]),
        calls: AtomicUsize::new(0),
        refuse: true,
    };
    let imported = f
        .history
        .commit_fleet_import(
            &"a".repeat(32),
            &f.candidate,
            f.plan(actor),
            &retry,
            &TrustedReviewers::default(),
        )
        .unwrap();
    assert_eq!(text(&imported, "state").unwrap(), "imported");
    assert_eq!(imported.get("target"), pending.get("target"));
    assert_eq!(retry.calls.load(Ordering::SeqCst), 0);
    assert_eq!(f.inspect(actor).unwrap(), Some(imported.clone()));
    assert_eq!(fs::read(f.receipt()).unwrap(), receipt);
    assert_eq!(
        fs::read(f.source.join("work.txt")).unwrap(),
        b"ongoing editor work"
    );
    assert_eq!(
        f.history
            .project()
            .saved_versions(f.history.metadata_path())
            .unwrap(),
        versions
    );
    let next = f
        .history
        .project()
        .save_capture(
            f.history.metadata_path(),
            &f.history
                .project()
                .capture_inputs(ObservationLimits::default())
                .unwrap(),
            f.capture.public_key(),
            |payload| f.capture.sign(payload),
        )
        .unwrap();
    let open = OpenWorkspace::open_attachment_store(
        f.history.metadata_path(),
        f.history.store.clone(),
        false,
    )
    .unwrap();
    assert_eq!(open.operations(), 4);
    assert_eq!(
        open.linear_history(Some(next.operation())).unwrap(),
        vec![f.base, f.snapshot.operation, next.operation()]
    );
    assert_eq!(
        open.historical_workspace_file(
            RecordDigest::parse_hex(text(&imported, "target").unwrap()).unwrap(),
            "work.txt"
        )
        .unwrap()
        .unwrap()
        .bytes,
        b"candidate saved bytes"
    );
    assert!(open.shared_version().is_none());
}
#[test]
fn invalid_or_aliased_import_receipts_never_resume_or_overwrite() {
    let f = Fixture::new("tamper");
    let signer = Signer {
        key: SigningKey::from_bytes(&[73; 32]),
        calls: AtomicUsize::new(0),
        refuse: false,
    };
    let actor = signer.public_key();
    assert!(f
        .history
        .commit_fleet_import_with(
            &"a".repeat(32),
            &f.candidate,
            f.plan(actor),
            &signer,
            &TrustedReviewers::default(),
            || Err(error("interrupted"))
        )
        .is_err());
    let original = fs::read(f.receipt()).unwrap();
    let alias = f.root.join("receipt-alias");
    fs::hard_link(f.receipt(), &alias).unwrap();
    assert!(f.inspect(actor).is_err());
    assert!(f
        .history
        .commit_fleet_import(
            &"a".repeat(32),
            &f.candidate,
            f.plan(actor),
            &signer,
            &TrustedReviewers::default()
        )
        .is_err());
    fs::remove_file(alias).unwrap();
    fs::write(f.receipt(), b"{partial signed receipt").unwrap();
    assert!(f.inspect(actor).is_err());
    assert!(f
        .history
        .commit_fleet_import(
            &"a".repeat(32),
            &f.candidate,
            f.plan(actor),
            &signer,
            &TrustedReviewers::default()
        )
        .is_err());
    assert_eq!(fs::read(f.receipt()).unwrap(), b"{partial signed receipt");
    assert_eq!(signer.calls.load(Ordering::SeqCst), 2);
    fs::write(f.receipt(), original).unwrap();
    assert!(f
        .inspect(PublicKey::from_bytes(
            SigningKey::from_bytes(&[74; 32]).verifying_key().to_bytes()
        ))
        .is_err());
    assert_eq!(
        text(&f.inspect(actor).unwrap().unwrap(), "state").unwrap(),
        "pending"
    );
    let open = OpenWorkspace::open_attachment_store(
        f.history.metadata_path(),
        f.history.store.clone(),
        false,
    )
    .unwrap();
    assert_eq!(open.operations(), 2);
}
