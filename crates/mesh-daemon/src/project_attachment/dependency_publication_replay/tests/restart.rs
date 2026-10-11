//! Separate-process publication crash fixtures. Software credentials are test data only.
use super::super::super::dependency_private_context::publication::Step;
#[path = "restart/publication_retention.rs"]
mod publication_retention;
use super::*;
use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
const ENV: &str = "MESH_PUBLICATION_RESTART_FIXTURE";
const CHILD: &str =
    "project_attachment::dependency_publication_replay::tests::restart::publication_restart_child";

#[test]
fn publication_restart_child() {
    let Ok(raw) = std::env::var(ENV) else {
        return;
    };
    let v = Json::parse(&raw).unwrap();
    let text = |key| v.get(key).and_then(Json::as_text).unwrap();
    let root = Path::new(text("root"));
    let storage = AttachmentStorage::open(&root.join("metadata")).unwrap();
    let request = RecordDigest::parse_hex(text("request")).unwrap();
    let public = fs::read(root.join("restart-public-key")).unwrap();
    let credential =
        HumanApprovalCredential::from_public_key(public.as_slice().try_into().unwrap()).unwrap();
    let mode = text("mode");
    let trust = if mode == "missing-trust" {
        crate::TrustedReviewers::default()
    } else {
        crate::TrustedReviewers::with_human_credentials([credential])
    };
    if matches!(
        v.get("kind").and_then(Json::as_text),
        Some("input-decision" | "saved-review" | "snapshot")
    ) {
        use super::super::super::dependency_private_context::control::Step as ControlStep;
        let operation = RecordDigest::parse_hex(text("operation")).unwrap();
        let previous = RecordDigest::parse_hex(text("previous")).unwrap();
        let hook = |step, file: &mut fs::File, frame: &[u8]| {
            if matches!(step, ControlStep::Staged) {
                if mode == "staged" {
                    std::process::exit(85);
                }
                if mode == "partial" {
                    let cut = match text("cut") {
                        "first" => 1,
                        "last" => frame.len() - 1,
                        _ => frame.len() / 2,
                    };
                    file.write_all(&frame[..cut])?;
                    file.sync_all()?;
                    std::process::exit(86);
                }
            }
            if matches!(step, ControlStep::Appended) && mode == "lost-ack" {
                std::process::exit(87);
            }
            Ok(())
        };
        let exact_request = if mode == "foreign-request" {
            RecordDigest::from_bytes([96; 32])
        } else {
            request
        };
        let result = if text("kind") == "snapshot" {
            storage
                .save_native_review_snapshot_with_io(
                    text("work"),
                    operation,
                    exact_request,
                    &trust,
                    hook,
                    |f| f.sync_all(),
                )
                .map(|result| {
                    Json::object([
                        ("record", Json::text(result.record().to_hex())),
                        ("graph", Json::text(result.graph().to_hex())),
                        ("validation", Json::text(result.validation().to_hex())),
                    ])
                })
        } else if text("kind") == "saved-review" {
            storage
                .save_native_review_with_io(
                    text("work"),
                    operation,
                    previous,
                    exact_request,
                    &trust,
                    hook,
                    |f| f.sync_all(),
                )
                .map(|result| {
                    Json::object([
                        ("record", Json::text(result.record().to_hex())),
                        ("bundle", Json::text(result.bundle().to_hex())),
                    ])
                })
        } else {
            storage
                .decide_native_saved_input_with_io(
                    text("work"),
                    operation,
                    crate::project_attachment::NativeSavedInputDecision::Rejected,
                    Some(previous),
                    exact_request,
                    &trust,
                    hook,
                    |f| f.sync_all(),
                )
                .map(|result| {
                    Json::object([
                        ("record", Json::text(result.record().to_hex())),
                        ("revision", Json::Number(result.revision())),
                    ])
                })
        };
        if mode == "missing-trust" || mode == "foreign-request" {
            assert!(result.is_err());
        } else {
            let result = result.unwrap();
            fs::write(root.join("decision-restart-result"), result.encode()).unwrap();
        }
        return;
    }
    let review = RecordDigest::parse_hex(text("review")).unwrap();
    let receipt = fs::read(root.join("restart-receipt")).unwrap();
    let result = storage.commit_native_publication_with_io(
        text("work"),
        if mode == "foreign-request" {
            RecordDigest::from_bytes([99; 32])
        } else {
            request
        },
        review,
        &receipt,
        &trust,
        |step, file, frame| {
            if matches!(step, Step::Staged) {
                if mode == "staged" {
                    std::process::exit(75);
                }
                if mode == "partial" {
                    let cut = match text("cut") {
                        "first" => 1,
                        "last" => frame.len() - 1,
                        _ => frame.len() / 2,
                    };
                    file.write_all(&frame[..cut])?;
                    file.sync_all()?;
                    std::process::exit(76);
                }
            }
            if matches!(step, Step::Appended) && mode == "lost-ack" {
                std::process::exit(77);
            }
            Ok(())
        },
        |file| file.sync_all(),
    );
    if mode == "missing-trust" || mode == "foreign-request" {
        assert!(result.is_err());
    } else {
        let result = result.unwrap();
        fs::write(
            root.join("restart-result"),
            Json::object([
                ("record", Json::text(result.record().to_hex())),
                ("head", Json::text(result.head().to_hex())),
                ("revision", Json::Number(result.revision())),
            ])
            .encode(),
        )
        .unwrap();
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn interrupted_publication(
    root: &Path,
    work: &str,
    request: RecordDigest,
    review: RecordDigest,
    receipt: &[u8],
    public: &[u8],
    journal: &Path,
    cut: &str,
    after_staging: impl FnOnce(),
) -> Json {
    fs::write(root.join("restart-public-key"), public).unwrap();
    fs::write(root.join("restart-receipt"), receipt).unwrap();
    let run = |mode: &str, expected: i32| {
        let value = Json::object([
            ("root", Json::text(root.to_string_lossy())),
            ("work", Json::text(work)),
            ("request", Json::text(request.to_hex())),
            ("review", Json::text(review.to_hex())),
            ("mode", Json::text(mode)),
            ("cut", Json::text(cut)),
        ]);
        run_child(root, mode, value, expected);
    };
    let baseline = publication_retention::baseline(root, work, review);
    let before = fs::read(journal).unwrap();
    run("staged", 75);
    assert_eq!(fs::read(journal).unwrap(), before);
    publication_retention::inspect(root, work, request, review, receipt, &baseline, "staged");
    after_staging();
    assert_eq!(fs::read(journal).unwrap(), before);
    run("partial", 76);
    let torn = fs::read(journal).unwrap();
    assert!(torn.starts_with(&before) && torn.len() > before.len());
    publication_retention::inspect(root, work, request, review, receipt, &baseline, "partial");
    run("missing-trust", 0);
    assert_eq!(fs::read(journal).unwrap(), torn);
    run("foreign-request", 0);
    assert_eq!(fs::read(journal).unwrap(), torn);
    run("lost-ack", 77);
    let complete = fs::read(journal).unwrap();
    assert!(complete.starts_with(&torn) && complete.len() > torn.len());
    publication_retention::inspect(root, work, request, review, receipt, &baseline, "lost-ack");
    run("retry", 0);
    assert_eq!(fs::read(journal).unwrap(), complete);
    publication_retention::finished(root, work, request, review, receipt);
    let result = fs::read_to_string(root.join("restart-result")).unwrap();
    run("retry", 0);
    assert_eq!(fs::read(journal).unwrap(), complete);
    assert_eq!(
        fs::read_to_string(root.join("restart-result")).unwrap(),
        result
    );
    Json::parse(&result).unwrap()
}

fn run_child(root: &Path, mode: &str, value: Json, expected: i32) {
    let output = root.join(format!("restart-{mode}.log"));
    let stdout = fs::File::create(&output).unwrap();
    let stderr = stdout.try_clone().unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", CHILD, "--nocapture"])
        .env(ENV, value.encode())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!(
                "publication restart {mode} timed out: {}",
                fs::read_to_string(&output).unwrap()
            );
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    assert_eq!(
        status.code(),
        Some(expected),
        "{}",
        fs::read_to_string(&output).unwrap()
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn interrupted_native_decision(
    root: &Path,
    work: &str,
    operation: RecordDigest,
    previous: RecordDigest,
    request: RecordDigest,
    public: &[u8],
    journal: &Path,
    published: bool,
    cut: &str,
) -> Json {
    interrupted_native_control(
        root,
        work,
        operation,
        previous,
        request,
        public,
        journal,
        published,
        cut,
        "input-decision",
        |_| {},
    )
}
#[allow(clippy::too_many_arguments)]
pub(super) fn interrupted_native_review(
    root: &Path,
    work: &str,
    snapshot: RecordDigest,
    opener: RecordDigest,
    request: RecordDigest,
    public: &[u8],
    journal: &Path,
) -> Json {
    interrupted_native_control(
        root,
        work,
        snapshot,
        opener,
        request,
        public,
        journal,
        true,
        "mid",
        "saved-review",
        |_| {},
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn interrupted_native_snapshot(
    root: &Path,
    work: &str,
    operation: RecordDigest,
    request: RecordDigest,
    public: &[u8],
    journal: &Path,
    inspect_prefix: impl Fn(&str),
) -> Json {
    interrupted_native_control(
        root,
        work,
        operation,
        RecordDigest::from_bytes([0; 32]),
        request,
        public,
        journal,
        true,
        "last",
        "snapshot",
        inspect_prefix,
    )
}

fn assert_complete_control_inventory(
    storage: &AttachmentStorage,
    owner: &crate::project_attachment::ProvisionedAttachment,
    request: RecordDigest,
    trust: &crate::TrustedReviewers,
    facts: &Json,
    completed: &Json,
) -> Json {
    let inventory = storage
        .inspect_native_history_retention_during_control(owner, request, trust)
        .unwrap();
    assert_eq!(
        inventory.get("schema"),
        Some(&Json::text("mesh.native-history-control-retention/v1"))
    );
    assert_eq!(
        inventory.get("scope"),
        Some(&Json::text("owner-store-control-recovery"))
    );
    assert_eq!(
        inventory.get("collection_authority"),
        Some(&Json::Bool(false))
    );
    assert_eq!(inventory.get("control"), facts.get("control"));
    for name in ["work", "installation", "device", "inode"] {
        assert_eq!(inventory.get(name), facts.get(name));
    }
    for name in ["operations", "manifests", "payloads", "sidecars"] {
        let Json::Array(previous) = completed.get(name).unwrap() else {
            panic!("completed roots missing")
        };
        let Json::Array(current) = inventory.get(name).unwrap() else {
            panic!("control history roots missing")
        };
        assert!(
            previous.iter().all(|r| current.contains(r)),
            "completed {name} roots lost during control recovery"
        );
    }
    for name in ["operations", "manifests"] {
        assert_eq!(
            inventory.get(name),
            completed.get(name),
            "a control frame cannot create authored versions"
        );
    }
    let exact = facts.get("control").unwrap();
    let Json::Array(current) = inventory.get("payloads").unwrap() else {
        panic!("payload roots missing")
    };
    let Json::Array(recovery) = exact.get("payloads").unwrap() else {
        panic!("control roots missing")
    };
    assert!(
        recovery.iter().all(|r| current.contains(r)),
        "exact control objects missing from history"
    );
    let Json::Array(sidecars) = inventory.get("sidecars").unwrap() else {
        panic!("sidecar roots missing")
    };
    assert!(sidecars.contains(&Json::object([
        ("name", exact.get("sidecar").unwrap().clone()),
        ("digest", exact.get("sidecar_digest").unwrap().clone()),
    ])));
    inventory
}

fn assert_trusted_control_inventory(
    root: &Path,
    work: &str,
    request: RecordDigest,
    phase: &str,
    completed: &Json,
) {
    use std::os::unix::fs::MetadataExt as _;
    let storage = AttachmentStorage::open(&root.join("metadata")).unwrap();
    let owner = storage
        .reopen(&storage.candidate_owning_root(work).unwrap())
        .unwrap();
    let public = fs::read(root.join("restart-public-key")).unwrap();
    let credential =
        HumanApprovalCredential::from_public_key(public.as_slice().try_into().unwrap()).unwrap();
    let trust = crate::TrustedReviewers::with_human_credentials([credential]);
    let journal = owner.metadata_path().join(crate::RECORD_FILE_NAME);
    let sidecar = owner.metadata_path().join("dependency-decision.pending");
    let before = fs::read(&journal).unwrap();
    let pending = fs::read(&sidecar).unwrap();
    let note = fs::read(owner.project().root().join("note")).unwrap();
    let intent = Json::parse(std::str::from_utf8(&pending).unwrap()).unwrap();
    let facts = storage
        .inspect_verified_dependency_control_retention(&owner, request, &trust)
        .unwrap();
    assert_eq!(
        facts.get("schema"),
        Some(&Json::text("mesh.trusted-native-control-retention/v1"))
    );
    assert_eq!(
        facts.get("scope"),
        Some(&Json::text("owner-store-control-recovery"))
    );
    assert_eq!(facts.get("collection_authority"), Some(&Json::Bool(false)));
    let inventory =
        assert_complete_control_inventory(&storage, &owner, request, &trust, &facts, completed);
    let exact = facts.get("control").unwrap();
    assert_eq!(exact.get("request"), Some(&Json::text(request.to_hex())));
    assert_eq!(
        exact.get("sidecar_digest"),
        Some(&Json::text(
            super::super::super::dependency_transaction::hash(&pending).to_hex()
        ))
    );
    assert_eq!(
        exact.get("journal_digest"),
        Some(&Json::text(
            super::super::super::dependency_transaction::hash(&before).to_hex()
        ))
    );
    let physical = fs::metadata(owner.metadata_path()).unwrap();
    for (name, value) in [("device", physical.dev()), ("inode", physical.ino())] {
        assert_eq!(facts.get(name), Some(&Json::text(format!("{value:x}"))));
        assert_eq!(facts.get(name), exact.get(name));
    }
    let written = exact
        .get("written_frame_bytes")
        .and_then(Json::as_u64)
        .unwrap();
    assert_eq!(written == 0, phase == "staged");
    let Json::Array(payloads) = exact.get("payloads").unwrap() else {
        panic!("control roots missing")
    };
    let payload = intent.get("payload").unwrap();
    assert!(payloads.contains(payload));
    let digest = RecordDigest::parse_hex(payload.as_text().unwrap()).unwrap();
    let cas = Cas::<_, Blake3>::with_filesystem(
        owner.metadata_path(),
        owner.store.filesystem().read_only(),
    )
    .unwrap();
    let path = owner.metadata_path().join(
        cas.layout()
            .chunk_path(&mesh_cas::Digest32::from_bytes(*digest.as_bytes())),
    );
    let original = fs::read(&path).unwrap();
    let decoded = Json::parse(std::str::from_utf8(&original).unwrap()).unwrap();
    let body = decoded.get("body").unwrap();
    if let Some(graph) = body.get("graph") {
        assert!(payloads.contains(graph));
    }
    for name in ["canonical", "bundle"] {
        if let Some(identity) = body.get(name) {
            assert!(!payloads.contains(identity));
        }
    }
    assert!(
        owner
            .inspect_pending_dependency_control_retention(request)
            .is_err(),
        "ordinary reader admitted publication history"
    );
    assert!(storage
        .inspect_native_history_retention(owner.id(), &trust)
        .is_err());
    assert!(storage
        .inspect_verified_dependency_control_retention(
            &owner,
            RecordDigest::from_bytes([96; 32]),
            &trust
        )
        .is_err());
    assert!(storage
        .inspect_native_history_retention_during_control(
            &owner,
            RecordDigest::from_bytes([96; 32]),
            &trust
        )
        .is_err());
    assert!(storage
        .inspect_verified_dependency_control_retention(
            &owner,
            request,
            &crate::TrustedReviewers::default()
        )
        .is_err());
    assert!(storage
        .inspect_native_history_retention_during_control(
            &owner,
            request,
            &crate::TrustedReviewers::default()
        )
        .is_err());
    if work != owner.id() {
        let child = storage.reopen(work).unwrap();
        assert!(storage
            .inspect_verified_dependency_control_retention(&child, request, &trust)
            .is_err());
        assert!(storage
            .inspect_native_history_retention_during_control(&child, request, &trust)
            .is_err());
    }
    if phase == "staged" {
        let foreign = std::str::from_utf8(&pending).unwrap().replacen(
            &request.to_hex(),
            &RecordDigest::from_bytes([96; 32]).to_hex(),
            1,
        );
        assert_ne!(foreign.as_bytes(), pending.as_slice());
        fs::write(&sidecar, foreign.as_bytes()).unwrap();
        assert!(storage
            .inspect_verified_dependency_control_retention(&owner, request, &trust)
            .is_err());
        assert!(storage
            .inspect_native_history_retention_during_control(&owner, request, &trust)
            .is_err());
        assert_eq!(fs::read(&sidecar).unwrap(), foreign.as_bytes());
        fs::write(&sidecar, &pending).unwrap();
        fs::write(&path, b"corrupt control payload").unwrap();
        assert!(storage
            .inspect_verified_dependency_control_retention(&owner, request, &trust)
            .is_err());
        assert!(storage
            .inspect_native_history_retention_during_control(&owner, request, &trust)
            .is_err());
        assert_eq!(fs::read(&path).unwrap(), b"corrupt control payload");
        fs::remove_file(&path).unwrap();
        assert!(storage
            .inspect_verified_dependency_control_retention(&owner, request, &trust)
            .is_err());
        assert!(storage
            .inspect_native_history_retention_during_control(&owner, request, &trust)
            .is_err());
        assert!(!path.exists());
        fs::write(&path, &original).unwrap();
        if let Some(graph) = body.get("graph").and_then(Json::as_text) {
            let graph = RecordDigest::parse_hex(graph).unwrap();
            let graph_path = owner.metadata_path().join(
                cas.layout()
                    .chunk_path(&mesh_cas::Digest32::from_bytes(*graph.as_bytes())),
            );
            let saved_graph = fs::read(&graph_path).unwrap();
            fs::write(&graph_path, b"corrupt saved review graph").unwrap();
            assert!(storage
                .inspect_verified_dependency_control_retention(&owner, request, &trust)
                .is_err());
            assert!(storage
                .inspect_native_history_retention_during_control(&owner, request, &trust)
                .is_err());
            assert_eq!(
                fs::read(&graph_path).unwrap(),
                b"corrupt saved review graph"
            );
            fs::remove_file(&graph_path).unwrap();
            assert!(storage
                .inspect_verified_dependency_control_retention(&owner, request, &trust)
                .is_err());
            assert!(storage
                .inspect_native_history_retention_during_control(&owner, request, &trust)
                .is_err());
            assert!(!graph_path.exists());
            fs::write(&graph_path, saved_graph).unwrap();
        }
        assert_eq!(
            storage
                .inspect_verified_dependency_control_retention(&owner, request, &trust)
                .unwrap(),
            facts
        );
        let Json::Array(previous_sidecars) = completed.get("sidecars").unwrap() else {
            panic!("completed sidecars missing")
        };
        let receipt_name = previous_sidecars
            .iter()
            .find_map(|s| {
                s.get("name")
                    .and_then(Json::as_text)
                    .filter(|name| name.starts_with("dependency-capture-"))
            })
            .unwrap();
        let receipt =
            Json::parse(&fs::read_to_string(owner.metadata_path().join(receipt_name)).unwrap())
                .unwrap();
        let frames =
            RecordDigest::parse_hex(receipt.get("frames").and_then(Json::as_text).unwrap())
                .unwrap();
        let scan = mesh_store::scan_journal(&before).unwrap();
        let old_operation = scan
            .records()
            .iter()
            .find_map(|record| match record {
                StoredRecord::Operation(operation) => Some(operation.payload_digest),
                _ => None,
            })
            .unwrap();
        let old_chunk = scan
            .records()
            .iter()
            .find_map(|record| match record {
                StoredRecord::Manifest(manifest) => manifest.chunks.first().map(|c| c.digest),
                _ => None,
            })
            .unwrap();
        let Json::Array(previous_payloads) = completed.get("payloads").unwrap() else {
            panic!("completed payloads missing")
        };
        for digest in [frames, old_operation, old_chunk] {
            assert!(previous_payloads.contains(&Json::text(digest.to_hex())));
            let object_path = owner.metadata_path().join(
                cas.layout()
                    .chunk_path(&mesh_cas::Digest32::from_bytes(*digest.as_bytes())),
            );
            let original = fs::read(&object_path).unwrap();
            fs::write(&object_path, b"corrupt retained history object").unwrap();
            assert!(storage
                .inspect_native_history_retention_during_control(&owner, request, &trust)
                .is_err());
            assert_eq!(
                fs::read(&object_path).unwrap(),
                b"corrupt retained history object"
            );
            fs::remove_file(&object_path).unwrap();
            assert!(storage
                .inspect_native_history_retention_during_control(&owner, request, &trust)
                .is_err());
            assert!(!object_path.exists());
            fs::write(&object_path, original).unwrap();
        }
        assert_eq!(
            storage
                .inspect_native_history_retention_during_control(&owner, request, &trust)
                .unwrap(),
            inventory
        );
    }
    assert_eq!(fs::read(&journal).unwrap(), before);
    assert_eq!(fs::read(&sidecar).unwrap(), pending);
    assert_eq!(fs::read(owner.project().root().join("note")).unwrap(), note);
}

#[allow(clippy::too_many_arguments)]
fn interrupted_native_control(
    root: &Path,
    work: &str,
    operation: RecordDigest,
    previous: RecordDigest,
    request: RecordDigest,
    public: &[u8],
    journal: &Path,
    published: bool,
    cut: &str,
    kind: &str,
    inspect_prefix: impl Fn(&str),
) -> Json {
    fs::write(root.join("restart-public-key"), public).unwrap();
    let run = |mode: &str, expected: i32| {
        let value = Json::object([
            ("kind", Json::text(kind)),
            ("root", Json::text(root.to_string_lossy())),
            ("work", Json::text(work)),
            ("operation", Json::text(operation.to_hex())),
            ("previous", Json::text(previous.to_hex())),
            ("request", Json::text(request.to_hex())),
            ("mode", Json::text(mode)),
            ("cut", Json::text(cut)),
        ]);
        run_child(root, mode, value, expected);
    };
    let completed = if published {
        let storage = AttachmentStorage::open(&root.join("metadata")).unwrap();
        let owner = storage
            .reopen(&storage.candidate_owning_root(work).unwrap())
            .unwrap();
        let credential =
            HumanApprovalCredential::from_public_key(public.try_into().unwrap()).unwrap();
        let trust = crate::TrustedReviewers::with_human_credentials([credential]);
        Some(
            storage
                .inspect_native_history_retention(owner.id(), &trust)
                .unwrap(),
        )
    } else {
        None
    };
    let before = fs::read(journal).unwrap();
    run("staged", 85);
    assert_eq!(fs::read(journal).unwrap(), before);
    inspect_prefix("staged");
    if published {
        assert_trusted_control_inventory(
            root,
            work,
            request,
            "staged",
            completed.as_ref().unwrap(),
        );
    }
    assert_eq!(fs::read(journal).unwrap(), before);
    run("partial", 86);
    let torn = fs::read(journal).unwrap();
    assert!(torn.starts_with(&before) && torn.len() > before.len());
    inspect_prefix("partial");
    if published {
        assert_trusted_control_inventory(
            root,
            work,
            request,
            "partial",
            completed.as_ref().unwrap(),
        );
    }
    assert_eq!(fs::read(journal).unwrap(), torn);
    if published {
        run("missing-trust", 0);
        assert_eq!(fs::read(journal).unwrap(), torn);
    }
    run("foreign-request", 0);
    assert_eq!(fs::read(journal).unwrap(), torn);
    run("lost-ack", 87);
    let complete = fs::read(journal).unwrap();
    assert!(complete.starts_with(&torn) && complete.len() > torn.len());
    if published {
        assert_trusted_control_inventory(
            root,
            work,
            request,
            "lost-ack",
            completed.as_ref().unwrap(),
        );
    }
    run("retry", 0);
    assert_eq!(fs::read(journal).unwrap(), complete);
    let result = fs::read_to_string(root.join("decision-restart-result")).unwrap();
    run("retry", 0);
    assert_eq!(fs::read(journal).unwrap(), complete);
    assert_eq!(
        fs::read_to_string(root.join("decision-restart-result")).unwrap(),
        result
    );
    if published {
        let storage = AttachmentStorage::open(&root.join("metadata")).unwrap();
        let owner = storage
            .reopen(&storage.candidate_owning_root(work).unwrap())
            .unwrap();
        let public = fs::read(root.join("restart-public-key")).unwrap();
        let credential =
            HumanApprovalCredential::from_public_key(public.as_slice().try_into().unwrap())
                .unwrap();
        let trust = crate::TrustedReviewers::with_human_credentials([credential]);
        let sidecar = owner.metadata_path().join("dependency-decision.pending");
        assert!(!sidecar.exists());
        assert!(storage
            .inspect_verified_dependency_control_retention(&owner, request, &trust)
            .is_err());
        assert!(storage
            .inspect_native_history_retention_during_control(&owner, request, &trust)
            .is_err());
        assert!(!sidecar.exists());
        assert_eq!(fs::read(journal).unwrap(), complete);
    }
    Json::parse(&result).unwrap()
}
