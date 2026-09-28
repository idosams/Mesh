//! Lost completion recovery against a real native lane and durable fleet journal.
use super::*;
use crate::ipc::{nothing_to_recover, Operations, StartupSummary};
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Signer {
    key: ed25519_dalek::SigningKey,
    calls: AtomicUsize,
}
impl CheckpointSigner for Signer {
    fn public_key(&self) -> mesh_types::PublicKey {
        mesh_types::PublicKey::from_bytes(self.key.verifying_key().to_bytes())
    }
    fn sign(&self, payload: &mesh_crypto::SigningPayload) -> Result<mesh_types::Signature, String> {
        use ed25519_dalek::Signer as _;
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(mesh_types::Signature::from_bytes(
            self.key.sign(payload.as_bytes()).to_bytes(),
        ))
    }
}

#[test]
fn appended_deletion_without_completion_recovers_without_signing_or_touching_later_work() {
    let path = std::env::temp_dir().join(format!(
        "mesh-delete-lost-completion-{}",
        std::process::id()
    ));
    fs::create_dir(&path).unwrap();
    let original = path.join("original");
    fs::create_dir(&original).unwrap();
    fs::write(original.join("note.txt"), b"original\n").unwrap();
    let parameters = CheckpointRuntimeParameters {
        idle_interval: Some(std::time::Duration::from_millis(10)),
        maximum_uncheckpointed_bytes: Some(65_536),
        maximum_uncheckpointed_interval: Some(std::time::Duration::from_secs(60)),
    };
    let desktop = crate::LiveDaemon::with_checkpoint_runtime(
        StartupSummary::from(&nothing_to_recover()),
        parameters,
    )
    .unwrap();
    let preview = desktop
        .preview_folder_import(original.to_str().unwrap())
        .unwrap();
    desktop
        .confirm_folder_import(
            original.to_str().unwrap(),
            path.join("source.mesh").to_str().unwrap(),
            preview.get("summary").unwrap().as_text().unwrap(),
        )
        .unwrap();
    let source = desktop.workspace_state().unwrap();
    let input = VersionInput {
        root: source.root,
        digest: source.digest,
        installation: source.installation,
        version: source.workspace_versions[0].operation(),
    };
    let mut runtime = Runtime::open(
        mesh_store::fleet::FleetStore::open(path.join("fleet.sqlite")).unwrap(),
        "objective",
    )
    .unwrap();
    runtime
        .record(
            "start",
            Command::Start {
                goal: "Recover deletion".into(),
                limits: super::super::Limits {
                    lanes: 2,
                    concurrency: 1,
                    depth: 1,
                    retries: 1,
                },
            },
        )
        .unwrap();
    let allocation = path.join("allocations");
    fs::create_dir(&allocation).unwrap();
    fs::set_permissions(&allocation, fs::Permissions::from_mode(0o700)).unwrap();
    let allocator = Arc::new(
        NativeLaneAllocator::open(&allocation, TrustedReviewers::default(), parameters, vec![])
            .unwrap(),
    );
    let service = FleetService::new(runtime, allocator, BTreeSet::from(["codex".into()])).unwrap();
    let lane = service
        .create_root("root", "Resolve deletion", "codex", &input)
        .unwrap();
    service
        .native_command(
            "dispatch",
            Command::Dispatch {
                lane: lane.clone(),
                run: "run".into(),
            },
        )
        .unwrap();
    let signer = Arc::new(Signer {
        key: ed25519_dalek::SigningKey::from_bytes(&[0x65; 32]),
        calls: AtomicUsize::new(0),
    });
    let credential = service
        .grant_with_signer(&lane, "run", "session", signer.clone())
        .unwrap();
    let id = format!(
        "file-deletion-{}",
        &lane_identity("objective", &lane, "delete").unwrap()[5..]
    );
    let (workspace, version, operation) = {
        let mut inner = service.lock().unwrap();
        let grant = inner
            .grants
            .get(&token_key(credential.transport_value()))
            .unwrap()
            .clone();
        let workspace = inner.workspaces[&lane].clone();
        let state = exact_state(&workspace).unwrap();
        let version = workspace
            .daemon()
            .inspect_managed_file("note.txt")
            .unwrap()
            .current_version()
            .to_owned();
        fs::remove_file(Path::new(&state.root).join("note.txt")).unwrap();
        inner
            .runtime
            .record(
                &format!("begin-{id}"),
                Command::BeginFileDeletion {
                    id: id.clone(),
                    lane: lane.clone(),
                    origin: super::super::AgentOrigin {
                        actor: grant.actor,
                        session: grant.session,
                        run: grant.run,
                        generation: grant.generation.clone(),
                    },
                    input_digest: state.digest.clone(),
                    path: "note.txt".into(),
                    version: RecordDigest::parse_hex(&version).unwrap(),
                },
            )
            .unwrap();
        let receipt = workspace
            .daemon()
            .checkpoint_agent_file_deletion_prepared(
                crate::AgentWorkspaceCheckpointRequest {
                    root: &state.root,
                    digest: &state.digest,
                    installation: &state.installation,
                    generation: &grant.generation,
                },
                "note.txt",
                &version,
                signer.public_key(),
                |payload| signer.sign(payload),
                |operation| {
                    inner
                        .runtime
                        .record(
                            &format!("prepare-{id}"),
                            Command::PrepareFileDeletion {
                                id: id.clone(),
                                operation,
                            },
                        )
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                },
            )
            .unwrap();
        // Model interruption after the workspace append, before the fleet completion record.
        assert!(inner.runtime.state().file_deletions[&id].result.is_none());
        (workspace, version, receipt.changeset().to_owned())
    };
    let checkpoint = service
        .agent_call(
            credential.transport_value(),
            "checkpoint",
            &Json::object([("request", Json::text("empty-result"))]),
        )
        .unwrap();
    let review = service
        .agent_call(
            credential.transport_value(),
            "submit_review",
            &Json::object([("checkpoint", checkpoint.get("checkpoint").unwrap().clone())]),
        )
        .unwrap();
    let target =
        RecordDigest::parse_hex(checkpoint.get("version").unwrap().as_text().unwrap()).unwrap();
    let bundle = RecordDigest::parse_hex(review.get("bundle").unwrap().as_text().unwrap()).unwrap();
    let identity = exact_state(&workspace).unwrap();
    workspace
        .daemon()
        .with_recorded_lane_review(
            &identity.root,
            &identity.installation,
            bundle,
            target,
            |open| {
                let recorded = open.review(&bundle).unwrap();
                assert!(open.human_approval_context(&recorded).is_err());
                assert!(open.human_approval_preview(&recorded).is_err());
                assert!(open.saved_publication_review_bundle(target).is_err());
                assert_eq!(open.saved_agent_inspection_bundle(target).unwrap(), bundle);
                Ok(())
            },
        )
        .unwrap();
    let before = exact_state(&workspace).unwrap();
    fs::write(
        Path::new(&before.root).join("note.txt"),
        b"later user work\n",
    )
    .unwrap();
    let calls = signer.calls.load(Ordering::SeqCst);
    let args = Json::object([
        ("request", Json::text("delete")),
        ("path", Json::text("note.txt")),
        ("version", Json::text(version)),
    ]);
    let receipt = service
        .agent_call(credential.transport_value(), "resolve_file_deletion", &args)
        .unwrap();
    assert_eq!(
        receipt.get("operation").unwrap().as_text(),
        Some(operation.as_str())
    );
    assert_eq!(receipt.get("settled"), Some(&Json::Bool(false)));
    assert_eq!(
        service
            .agent_call(credential.transport_value(), "resolve_file_deletion", &args)
            .unwrap(),
        receipt
    );
    assert_eq!(signer.calls.load(Ordering::SeqCst), calls);
    assert_eq!(exact_state(&workspace).unwrap().digest, before.digest);
    assert_eq!(
        fs::read(Path::new(&before.root).join("note.txt")).unwrap(),
        b"later user work\n"
    );
    assert_eq!(fs::read(original.join("note.txt")).unwrap(), b"original\n");
    let replay = Runtime::open(
        mesh_store::fleet::FleetStore::open(path.join("fleet.sqlite")).unwrap(),
        "objective",
    )
    .unwrap();
    assert_eq!(replay.state().file_deletions.len(), 1);
    assert!(replay.state().file_deletions[&id].result.is_some());
    fs::remove_dir_all(path).unwrap();
}
