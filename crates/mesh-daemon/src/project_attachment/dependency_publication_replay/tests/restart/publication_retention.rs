//! Actual native root/consumed publication interruption; no human-presence claim.
use super::*;
use crate::project_attachment::dependency_transaction::hash;
use std::collections::BTreeSet;

pub(super) struct Baseline {
    completed: Json,
    output: (RecordDigest, RecordDigest, RecordDigest),
    graph: RecordDigest,
}
fn trust(root: &Path) -> crate::TrustedReviewers {
    let public = fs::read(root.join("restart-public-key")).unwrap();
    let credential =
        HumanApprovalCredential::from_public_key(public.as_slice().try_into().unwrap()).unwrap();
    crate::TrustedReviewers::with_human_credentials([credential])
}
pub(super) fn baseline(root: &Path, work: &str, review: RecordDigest) -> Baseline {
    let storage = AttachmentStorage::open(&root.join("metadata")).unwrap();
    let owner = storage
        .reopen(&storage.candidate_owning_root(work).unwrap())
        .unwrap();
    let completed = storage
        .inspect_native_history_retention(owner.id(), &trust(root))
        .unwrap();
    let _guard = crate::workspace_custody::lock_workspace_initialization(&owner.store).unwrap();
    let (_, proof) = owner
        .project()
        .read_publication_private_history(owner.metadata_path(), &owner.store)
        .unwrap();
    let binding = proof.policy().bound_review(review).unwrap();
    Baseline {
        completed,
        output: binding.evidence().output(),
        graph: proof
            .policy()
            .review_graph(binding.evidence().snapshot())
            .unwrap(),
    }
}
pub(super) fn inspect(
    root: &Path,
    work: &str,
    request: RecordDigest,
    review: RecordDigest,
    receipt: &[u8],
    baseline: &Baseline,
    phase: &str,
) {
    use std::os::unix::fs::MetadataExt as _;
    let storage = AttachmentStorage::open(&root.join("metadata")).unwrap();
    let owner = storage
        .reopen(&storage.candidate_owning_root(work).unwrap())
        .unwrap();
    let selected = storage.reopen(work).unwrap();
    let trusted = trust(root);
    let journal = owner.metadata_path().join(crate::RECORD_FILE_NAME);
    let sidecar = owner.metadata_path().join("native-publication.pending");
    let before = fs::read(&journal).unwrap();
    let pending = fs::read(&sidecar).unwrap();
    let notes = [
        owner.project().root().join("note"),
        selected.project().root().join("note"),
    ];
    let source = notes
        .iter()
        .map(|p| fs::read(p).unwrap())
        .collect::<Vec<_>>();
    let inspect = || {
        storage.inspect_native_history_retention_during_publication(
            work, request, review, receipt, &trusted,
        )
    };
    let facts = inspect().unwrap();
    assert_eq!(
        facts.get("schema"),
        Some(&Json::text("mesh.native-history-publication-retention/v1"))
    );
    assert_eq!(
        facts.get("scope"),
        Some(&Json::text("owner-store-publication-recovery"))
    );
    assert_eq!(facts.get("collection_authority"), Some(&Json::Bool(false)));
    for name in ["work", "installation", "device", "inode"] {
        assert_eq!(facts.get(name), baseline.completed.get(name));
    }
    let physical = fs::metadata(owner.metadata_path()).unwrap();
    for (name, value) in [("device", physical.dev()), ("inode", physical.ino())] {
        assert_eq!(facts.get(name), Some(&Json::text(format!("{value:x}"))));
    }
    let output = facts.get("output").unwrap();
    for (name, id) in [
        ("work", baseline.output.0),
        ("installation", baseline.output.1),
        ("operation", baseline.output.2),
    ] {
        assert_eq!(output.get(name), Some(&Json::text(id.to_hex())));
    }
    for name in ["operations", "manifests", "payloads", "sidecars"] {
        let Json::Array(previous) = baseline.completed.get(name).unwrap() else {
            panic!("completed roots missing")
        };
        let Json::Array(current) = facts.get(name).unwrap() else {
            panic!("pending roots missing")
        };
        assert!(
            previous.iter().all(|root| current.contains(root)),
            "completed {name} roots were lost"
        );
    }
    for name in ["operations", "manifests"] {
        assert_eq!(facts.get(name), baseline.completed.get(name));
    }
    let exact = facts.get("publication").unwrap();
    for (name, id) in [
        ("request", request),
        ("review", review),
        ("receipt", hash(receipt)),
        ("journal_digest", hash(&before)),
        ("sidecar_digest", hash(&pending)),
    ] {
        assert_eq!(exact.get(name), Some(&Json::text(id.to_hex())));
    }
    assert_eq!(
        exact.get("sidecar"),
        Some(&Json::text("native-publication.pending"))
    );
    assert_eq!(
        exact
            .get("written_frame_bytes")
            .and_then(Json::as_u64)
            .unwrap()
            == 0,
        phase == "staged"
    );
    let intent = Json::parse(std::str::from_utf8(&pending).unwrap()).unwrap();
    let payload =
        RecordDigest::parse_hex(intent.get("payload").and_then(Json::as_text).unwrap()).unwrap();
    let Json::Array(current) = facts.get("payloads").unwrap() else {
        panic!("payloads missing")
    };
    let Json::Array(previous) = baseline.completed.get("payloads").unwrap() else {
        panic!("completed payloads missing")
    };
    for id in [payload, hash(receipt)] {
        assert!(current.contains(&Json::text(id.to_hex())));
    }
    assert!(current.iter().all(|root| previous.contains(root)
        || [
            Json::text(payload.to_hex()),
            Json::text(hash(receipt).to_hex())
        ]
        .contains(root)));
    let Json::Array(sidecars) = facts.get("sidecars").unwrap() else {
        panic!("sidecars missing")
    };
    assert!(sidecars.contains(&Json::object([
        ("name", Json::text("native-publication.pending")),
        ("digest", Json::text(hash(&pending).to_hex()))
    ])));
    assert!(storage
        .inspect_native_history_retention(owner.id(), &trusted)
        .is_err());
    assert!(storage
        .inspect_native_history_retention_during_publication(
            work,
            request,
            review,
            receipt,
            &crate::TrustedReviewers::default()
        )
        .is_err());
    let oversized_receipt = vec![0; 65537];
    for (candidate, req, rev, bytes) in [
        (work, RecordDigest::from_bytes([96; 32]), review, receipt),
        (work, request, RecordDigest::from_bytes([97; 32]), receipt),
        (work, request, review, b"wrong receipt".as_slice()),
        (work, RecordDigest::from_bytes([0; 32]), review, receipt),
        (work, request, RecordDigest::from_bytes([0; 32]), receipt),
        (work, request, review, b"".as_slice()),
        (work, request, review, oversized_receipt.as_slice()),
        (
            if work == owner.id() {
                "missing-publication-output"
            } else {
                owner.id()
            },
            request,
            review,
            receipt,
        ),
    ] {
        assert!(storage
            .inspect_native_history_retention_during_publication(
                candidate, req, rev, bytes, &trusted
            )
            .is_err());
    }
    if phase == "staged" {
        let cas = Cas::<_, Blake3>::with_filesystem(
            owner.metadata_path(),
            owner.store.filesystem().read_only(),
        )
        .unwrap();
        let path = |digest: RecordDigest| {
            owner.metadata_path().join(
                cas.layout()
                    .chunk_path(&mesh_cas::Digest32::from_bytes(*digest.as_bytes())),
            )
        };
        let decoded = Json::parse(&fs::read_to_string(path(payload)).unwrap()).unwrap();
        let body = decoded.get("body").unwrap();
        for name in ["credential", "challenge"] {
            assert!(!current.contains(body.get(name).unwrap()));
        }
        let scan = mesh_store::scan_journal(&before).unwrap();
        let old_operation = scan
            .records()
            .iter()
            .find_map(|r| match r {
                StoredRecord::Operation(r) => Some(r.payload_digest),
                _ => None,
            })
            .unwrap();
        let old_chunk = scan
            .records()
            .iter()
            .find_map(|r| match r {
                StoredRecord::Manifest(r) => r.chunks.first().map(|c| c.digest),
                _ => None,
            })
            .unwrap();
        let Json::Array(previous_sidecars) = baseline.completed.get("sidecars").unwrap() else {
            panic!("completed sidecars missing")
        };
        let name = previous_sidecars
            .iter()
            .find_map(|s| {
                s.get("name")
                    .and_then(Json::as_text)
                    .filter(|n| n.starts_with("dependency-capture-"))
            })
            .unwrap();
        let capture =
            Json::parse(&fs::read_to_string(owner.metadata_path().join(name)).unwrap()).unwrap();
        let frames =
            RecordDigest::parse_hex(capture.get("frames").and_then(Json::as_text).unwrap())
                .unwrap();
        let roots = BTreeSet::from([
            payload,
            hash(receipt),
            review,
            baseline.graph,
            old_operation,
            old_chunk,
            frames,
        ]);
        for digest in roots {
            let object = path(digest);
            let original = fs::read(&object).unwrap();
            fs::write(&object, b"corrupt retained publication object").unwrap();
            assert!(inspect().is_err());
            assert_eq!(
                fs::read(&object).unwrap(),
                b"corrupt retained publication object"
            );
            fs::remove_file(&object).unwrap();
            assert!(inspect().is_err());
            assert!(!object.exists());
            fs::write(&object, original).unwrap();
        }
        let foreign = std::str::from_utf8(&pending).unwrap().replacen(
            &request.to_hex(),
            &RecordDigest::from_bytes([95; 32]).to_hex(),
            1,
        );
        fs::write(&sidecar, foreign.as_bytes()).unwrap();
        assert!(inspect().is_err());
        assert_eq!(fs::read(&sidecar).unwrap(), foreign.as_bytes());
        fs::write(&sidecar, &pending).unwrap();
        let mut foreign = before.clone();
        foreign.extend_from_slice(b"unrecognized publication suffix");
        fs::write(&journal, &foreign).unwrap();
        assert!(inspect().is_err());
        assert_eq!(fs::read(&journal).unwrap(), foreign);
        fs::write(&journal, &before).unwrap();
        assert_eq!(inspect().unwrap(), facts);
    }
    assert_eq!(fs::read(&journal).unwrap(), before);
    assert_eq!(fs::read(&sidecar).unwrap(), pending);
    for (path, bytes) in notes.iter().zip(source) {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}
pub(super) fn finished(
    root: &Path,
    work: &str,
    request: RecordDigest,
    review: RecordDigest,
    receipt: &[u8],
) {
    let storage = AttachmentStorage::open(&root.join("metadata")).unwrap();
    let owner = storage
        .reopen(&storage.candidate_owning_root(work).unwrap())
        .unwrap();
    let sidecar = owner.metadata_path().join("native-publication.pending");
    assert!(!sidecar.exists());
    assert!(storage
        .inspect_native_history_retention_during_publication(
            work,
            request,
            review,
            receipt,
            &trust(root)
        )
        .is_err());
    assert!(!sidecar.exists());
}
