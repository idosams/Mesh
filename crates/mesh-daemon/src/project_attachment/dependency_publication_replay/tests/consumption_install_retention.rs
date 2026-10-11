//! Retained installed/checkpoint material selected before native publication.
use super::*;
use crate::project_attachment::{
    NativeConsumedStartRequest, NativeGrantInspection, NativeInputGrantRequest,
};
use std::path::Path;
pub(super) struct PendingInstalled {
    pub(super) destination: ProvisionedAttachment,
    pub(super) version: crate::project_attachment::SavedAttachmentVersion,
    pub(super) grant: RecordDigest,
    pub(super) journal: PathBuf,
    pub(super) before: Vec<u8>,
    pub(super) frame: Vec<u8>,
    pub(super) checkpoint: Vec<u8>,
}
pub(super) fn prepare(
    storage: &AttachmentStorage,
    owner: &ProvisionedAttachment,
    source: &ProvisionedAttachment,
    version: crate::project_attachment::SavedAttachmentVersion,
) -> PendingInstalled {
    let id = |n| RecordDigest::from_bytes([n; 32]);
    let destination = storage
        .reserve_dependency_lane_with_inputs(owner, source, version, id(220), &[owner])
        .unwrap();
    let grant = storage
        .grant_saved_input_with_inputs(
            owner,
            NativeInputGrantRequest {
                source,
                version,
                destination: &destination,
                allowed: true,
                expected_previous: None,
                request: id(221),
            },
            &[owner],
        )
        .unwrap()
        .record();
    let key = SigningKey::from_bytes(&[209; 32]);
    let actor = PublicKey::from_bytes(key.verifying_key().to_bytes());
    let candidate = storage
        .prepare_consumed_start(
            owner,
            NativeConsumedStartRequest {
                input: NativeGrantInspection {
                    source,
                    version,
                    destination: &destination,
                    grant,
                },
                available: &[owner],
                request: id(222),
                limits: ObservationLimits::default(),
            },
            actor,
            |payload: &SigningPayload| {
                Ok::<_, &'static str>(Signature::from_bytes(
                    key.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .unwrap();
    let staged = candidate.stage(storage).unwrap();
    let journal = destination.metadata_path().join(crate::RECORD_FILE_NAME);
    let before = fs::read(&journal).unwrap();
    candidate.fence_consumption_start(storage, &staged).unwrap();
    let after = fs::read(&journal).unwrap();
    assert!(after.starts_with(&before));
    let frame = after[before.len()..].to_vec();
    assert!(!frame.is_empty());
    candidate
        .commit_fenced_consumed_checkpoint(storage, &staged)
        .unwrap();
    let complete = fs::read(&journal).unwrap();
    let checkpoint = complete[after.len()..].to_vec();
    assert!(!checkpoint.is_empty());
    PendingInstalled {
        destination,
        version,
        grant,
        journal,
        before,
        frame,
        checkpoint,
    }
}
pub(super) fn add_source_entries(root: &Path) {
    fs::write(root.join("source/second"), b"second exact input").unwrap();
    fs::create_dir(root.join("source/tree")).unwrap();
    fs::write(root.join("source/tree/child"), b"nested exact input").unwrap();
}
pub(super) fn inspect(
    storage: &AttachmentStorage,
    owner: &ProvisionedAttachment,
    source: &ProvisionedAttachment,
    pending: PendingInstalled,
    trusted: &crate::TrustedReviewers,
) {
    use crate::project_attachment::dependency_transaction::{digest, hash};
    use std::collections::BTreeSet;
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
    let id = |n| RecordDigest::from_bytes([n; 32]);
    let available = [owner];
    let request = || NativeConsumedStartRequest {
        input: NativeGrantInspection {
            source,
            version: pending.version,
            destination: &pending.destination,
            grant: pending.grant,
        },
        available: &available,
        request: id(222),
        limits: ObservationLimits::default(),
    };
    let inspect =
        || storage.inspect_native_consumption_install_retention(owner, request(), trusted);
    let owner_journal = owner.metadata_path().join(crate::RECORD_FILE_NAME);
    let source_journal = source.metadata_path().join(crate::RECORD_FILE_NAME);
    let owner_before = fs::read(&owner_journal).unwrap();
    let source_before = fs::read(&source_journal).unwrap();
    let notes = [
        owner.project().root().join("note"),
        source.project().root().join("note"),
    ];
    let original_notes = notes
        .iter()
        .map(|p| fs::read(p).unwrap())
        .collect::<Vec<_>>();
    let git = owner.project().root().join(".git");
    fs::create_dir(&git).unwrap();
    fs::write(git.join("HEAD"), b"ref: refs/heads/user-work\n").unwrap();
    fs::write(git.join("index"), b"synthetic dirty index").unwrap();
    let source_inventory = storage
        .inspect_native_history_retention(source.id(), trusted)
        .unwrap();
    let owner_inventory = storage
        .inspect_native_history_retention(owner.id(), trusted)
        .unwrap();
    let start_path = pending
        .destination
        .metadata_path()
        .join("consumption-start.pending");
    let owner_path = pending
        .destination
        .metadata_path()
        .join("consumption-owner.pending");
    let history_path = pending
        .destination
        .metadata_path()
        .join("consumption-history.pending");
    let sidecars = [&start_path, &owner_path, &history_path];
    let raws = sidecars
        .iter()
        .map(|p| fs::read(p).unwrap())
        .collect::<Vec<_>>();
    let intent = Json::parse(std::str::from_utf8(&raws[0]).unwrap()).unwrap();
    let field = |v: &Json, n| digest(v.get(n).and_then(Json::as_text).unwrap()).unwrap();
    let cas = Cas::<_, Blake3>::with_filesystem(
        pending.destination.metadata_path(),
        pending.destination.store.filesystem().read_only(),
    )
    .unwrap();
    let local_path = |id: RecordDigest| {
        pending.destination.metadata_path().join(
            cas.layout()
                .chunk_path(&mesh_cas::Digest32::from_bytes(*id.as_bytes())),
        )
    };
    let read_json = |id| Json::parse(&fs::read_to_string(local_path(id)).unwrap()).unwrap();
    let payload = field(&intent, "payload");
    let start = read_json(payload);
    let body = start.get("body").unwrap();
    let descriptor = field(body, "staged");
    let stage = field(&read_json(descriptor), "stage");
    let stage_json = read_json(stage);
    let frames = field(&stage_json, "frames");
    let operation = field(&stage_json, "operation");
    assert_eq!(hash(&pending.checkpoint), frames);
    let Json::Array(objects) = stage_json.get("objects").unwrap() else {
        panic!("stage objects absent")
    };
    let mut expected = objects
        .iter()
        .map(|v| v.as_text().unwrap().to_owned())
        .collect::<BTreeSet<_>>();
    for value in [
        payload,
        descriptor,
        stage,
        frames,
        field(body, "configuration"),
        field(body, "prospective"),
        field(body, "closure"),
        field(&start, "authority"),
    ] {
        expected.insert(value.to_hex());
    }
    let scan = mesh_store::scan_journal(&pending.before).unwrap();
    let [StoredRecord::Dependency(enrollment)] = scan.records() else {
        panic!("physical enrollment differs")
    };
    expected.insert(enrollment.payload.to_hex());
    let origin = storage
        .lane_origin_bound(&pending.destination)
        .unwrap()
        .unwrap();
    let stage_root = origin
        .allocation
        .open_child_directory(std::ffi::OsStr::new(&format!(
            "consumption-{}",
            id(222).to_hex()
        )))
        .unwrap();
    let (stage_path, _pin) = stage_root.stable_namespace().unwrap();
    let entries = ["note", "second", "tree/child"];
    assert_eq!(
        fs::read(pending.destination.project().root().join("second")).unwrap(),
        b"second exact input"
    );
    assert_eq!(
        fs::read(pending.destination.project().root().join("tree/child")).unwrap(),
        b"nested exact input"
    );
    let installed = entries
        .iter()
        .map(|name| {
            let p = pending.destination.project().root().join(name);
            (
                p.clone(),
                fs::read(&p).unwrap(),
                fs::metadata(&p).unwrap().ino(),
            )
        })
        .collect::<Vec<_>>();
    let preserve = || {
        assert_eq!(fs::read(&owner_journal).unwrap(), owner_before);
        assert_eq!(fs::read(&source_journal).unwrap(), source_before);
        for (p, b) in notes.iter().zip(&original_notes) {
            assert_eq!(fs::read(p).unwrap(), *b);
        }
        assert_eq!(
            fs::read(git.join("HEAD")).unwrap(),
            b"ref: refs/heads/user-work\n"
        );
        assert_eq!(
            fs::read(git.join("index")).unwrap(),
            b"synthetic dirty index"
        );
        for (p, b, inode) in &installed {
            assert_eq!(fs::read(p).unwrap(), *b);
            assert_eq!(fs::metadata(p).unwrap().ino(), *inode);
        }
        for (p, b) in sidecars.iter().zip(&raws) {
            assert_eq!(fs::read(p).unwrap(), *b);
        }
    };
    let mut complete_start = pending.before.clone();
    complete_start.extend_from_slice(&pending.frame);
    for length in [0, 1, pending.checkpoint.len() - 1, pending.checkpoint.len()] {
        let mut observed = complete_start.clone();
        observed.extend_from_slice(&pending.checkpoint[..length]);
        fs::write(&pending.journal, &observed).unwrap();
        let inventory = inspect().unwrap();
        assert_eq!(
            inventory.get("schema"),
            Some(&Json::text("mesh.native-consumption-install-retention/v1"))
        );
        assert_eq!(
            inventory.get("scope"),
            Some(&Json::text("installation-checkpoint-before-owner"))
        );
        assert_eq!(
            inventory.get("collection_authority"),
            Some(&Json::Bool(false))
        );
        let Json::Array(stores) = inventory.get("stores").unwrap() else {
            panic!("stores missing")
        };
        assert!(stores.contains(&source_inventory) && stores.contains(&owner_inventory));
        assert_eq!(stores.len(), if source.id() == owner.id() { 1 } else { 2 });
        let destination = inventory.get("destination").unwrap();
        assert_eq!(destination.get("installed_entries"), Some(&Json::Number(3)));
        assert_eq!(
            destination.get("staged_operations"),
            Some(&Json::Array(vec![Json::text(operation.to_hex())]))
        );
        let Json::Array(roots) = destination.get("payloads").unwrap() else {
            panic!("roots missing")
        };
        assert_eq!(
            roots
                .iter()
                .map(|v| v.as_text().unwrap().to_owned())
                .collect::<BTreeSet<_>>(),
            expected
        );
        let physical = fs::metadata(pending.destination.metadata_path()).unwrap();
        for (name, n) in [("device", physical.dev()), ("inode", physical.ino())] {
            assert_eq!(destination.get(name), Some(&Json::text(format!("{n:x}"))));
        }
        let checkpoint = destination.get("checkpoint").unwrap();
        assert_eq!(checkpoint.get("frames"), Some(&Json::text(frames.to_hex())));
        assert_eq!(
            checkpoint.get("written_frame_bytes"),
            Some(&Json::Number(length as u64))
        );
        let Json::Array(receipts) = destination.get("sidecars").unwrap() else {
            panic!("sidecars missing")
        };
        assert_eq!(receipts.len(), 3);
        for (p, b) in sidecars.iter().zip(&raws) {
            assert!(receipts.contains(&Json::object([
                ("name", Json::text(p.file_name().unwrap().to_str().unwrap())),
                ("digest", Json::text(hash(b).to_hex()))
            ])));
        }
        if length == pending.checkpoint.len() {
            assert_eq!(inspect().unwrap(), inventory);
        }
        assert_eq!(fs::read(&pending.journal).unwrap(), observed);
        preserve();
        assert!(pending.destination.saved_versions().is_err());
        assert!(storage
            .inspect_native_consumption_start_retention(owner, request(), trusted)
            .is_err());
        let mut foreign = observed.clone();
        *foreign.last_mut().unwrap() ^= 1;
        fs::write(&pending.journal, &foreign).unwrap();
        assert!(inspect().is_err());
        assert_eq!(fs::read(&pending.journal).unwrap(), foreign);
        fs::write(&pending.journal, &observed).unwrap();
    }
    let full = fs::read(&pending.journal).unwrap();
    assert!(storage
        .recover_installing_consumed_start(owner, request())
        .is_err());
    assert!(storage
        .inspect_native_consumption_install_retention(
            owner,
            request(),
            &crate::TrustedReviewers::default()
        )
        .is_err());
    let mut wrong = request();
    wrong.request = id(223);
    assert!(storage
        .inspect_native_consumption_install_retention(owner, wrong, trusted)
        .is_err());
    let mut wrong = request();
    wrong.input.grant = id(223);
    assert!(storage
        .inspect_native_consumption_install_retention(owner, wrong, trusted)
        .is_err());
    let mut wrong = request();
    wrong.limits.file_bytes -= 1;
    assert!(storage
        .inspect_native_consumption_install_retention(owner, wrong, trusted)
        .is_err());
    let mut wrong = request();
    wrong.request = RecordDigest::from_bytes([0; 32]);
    assert!(storage
        .inspect_native_consumption_install_retention(owner, wrong, trusted)
        .is_err());
    let foreign_owner = owner.metadata_path().join(format!(
        "consumption-owner-{}-{}.pending",
        id(222).to_hex(),
        id(225).to_hex()
    ));
    fs::write(&foreign_owner, b"unverified owner attempt").unwrap();
    assert!(inspect().is_err());
    assert_eq!(
        fs::read(&foreign_owner).unwrap(),
        b"unverified owner attempt"
    );
    fs::remove_file(&foreign_owner).unwrap();
    for name in [
        "consumption-complete.pending",
        "dependency-decision.pending",
    ] {
        let p = pending.destination.metadata_path().join(name);
        fs::write(&p, b"foreign phase").unwrap();
        assert!(inspect().is_err());
        assert_eq!(fs::read(&p).unwrap(), b"foreign phase");
        fs::remove_file(p).unwrap();
    }
    for p in [
        local_path(frames),
        local_path(operation),
        stage_path.join("frames.mesh"),
        stage_path.join("entry-0/stage.json"),
        owner_path.clone(),
        history_path.clone(),
    ] {
        // Exact retained material must exist and remain immutable after both missing/corrupt refusal.
        let bytes = fs::read(&p).unwrap();
        let permissions = fs::metadata(&p).unwrap().permissions();
        fs::rename(&p, p.with_extension("retained-test-backup")).unwrap();
        assert!(inspect().is_err());
        assert!(!p.exists());
        fs::rename(p.with_extension("retained-test-backup"), &p).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(&p, b"corrupt retained input").unwrap();
        assert!(inspect().is_err());
        assert_eq!(fs::read(&p).unwrap(), b"corrupt retained input");
        fs::write(&p, bytes).unwrap();
        fs::set_permissions(&p, permissions).unwrap();
    }
    let editor = pending.destination.project().root().join("editor-work");
    fs::write(&editor, b"preserve editor").unwrap();
    assert!(inspect().is_err());
    assert_eq!(fs::read(&editor).unwrap(), b"preserve editor");
    fs::remove_file(editor).unwrap();
    preserve();
    assert_eq!(fs::read(&pending.journal).unwrap(), full);
    // Simulate valid interrupted installation by moving the identical native entry back into
    // its retained exchange. This is a physical boundary fixture, not a process-kill claim.
    fs::write(&pending.journal, &complete_start).unwrap();
    fs::remove_file(&history_path).unwrap();
    let installed_only = inspect().unwrap();
    let d = installed_only.get("destination").unwrap();
    assert!(d.get("checkpoint").is_none());
    let Json::Array(roots) = d.get("payloads").unwrap() else {
        panic!("roots missing")
    };
    assert!(!roots.contains(&Json::text(frames.to_hex())));
    let restored = pending.destination.project().root().join("note");
    let exchange = stage_path.join("entry-0/exchange");
    fs::rename(&restored, &exchange).unwrap();
    let partial = inspect().unwrap();
    assert_eq!(
        partial.get("destination").unwrap().get("installed_entries"),
        Some(&Json::Number(2))
    );
    fs::write(&history_path, &raws[2]).unwrap();
    fs::set_permissions(&history_path, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(inspect().is_err());
    fs::remove_file(&history_path).unwrap();
    // An installation intent is retained before any entry or start-frame bytes are installed.
    for (index, name) in [(1, "second"), (2, "tree")] {
        fs::rename(
            pending.destination.project().root().join(name),
            stage_path.join(format!("entry-{index}/exchange")),
        )
        .unwrap();
    }
    fs::write(&pending.journal, &pending.before).unwrap();
    let empty = inspect().unwrap();
    assert_eq!(
        empty.get("destination").unwrap().get("installed_entries"),
        Some(&Json::Number(0))
    );
    assert_eq!(
        empty
            .get("destination")
            .unwrap()
            .get("journal")
            .unwrap()
            .get("written_frame_bytes"),
        Some(&Json::Number(0))
    );
    for (index, name) in [(1, "second"), (2, "tree")] {
        fs::rename(
            stage_path.join(format!("entry-{index}/exchange")),
            pending.destination.project().root().join(name),
        )
        .unwrap();
    }
    fs::rename(&exchange, &restored).unwrap();
    fs::write(&history_path, &raws[2]).unwrap();
    fs::write(&pending.journal, &full).unwrap();
    // Recreated native sidecars must remain private; inspection never repairs their mode.
    fs::set_permissions(&history_path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(inspect().is_err());
    assert_eq!(fs::metadata(&history_path).unwrap().mode() & 0o077, 0o044);
    fs::set_permissions(&history_path, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        inspect()
            .unwrap()
            .get("destination")
            .unwrap()
            .get("installed_entries"),
        Some(&Json::Number(3))
    );
    preserve();
    // Append real validated native policy facts in the fixture. The inspector never appends.
    let owner_cas =
        Cas::<_, Blake3>::with_filesystem(owner.metadata_path(), owner.store.filesystem()).unwrap();
    let append = |kind: DependencyKind, body: Json| {
        let _guard = crate::workspace_custody::lock_workspace_initialization(&owner.store).unwrap();
        let (_, proof) = owner
            .project()
            .read_publication_private_history(owner.metadata_path(), &owner.store)
            .unwrap();
        let (revision, previous) = proof.policy().native_head().unwrap();
        let bytes = Json::object([
            (
                "schema",
                Json::text(if kind == DependencyKind::Grant {
                    "mesh.dependency-policy/v2"
                } else {
                    "mesh.dependency-policy/v1"
                }),
            ),
            ("authority", Json::text(proof.binding().authority.to_hex())),
            ("revision", Json::Number(revision + 1)),
            ("previous", Json::text(previous.to_hex())),
            ("kind", Json::Number(kind.code().into())),
            ("body", body),
        ])
        .encode()
        .into_bytes();
        let record = DependencyRecord {
            authority: proof.binding().authority,
            revision: revision + 1,
            previous,
            payload: hash(&bytes),
            kind,
        };
        proof
            .policy()
            .clone()
            .apply(record, &bytes)
            .unwrap_or_else(|e| panic!("fixture kind {} rejected: {e}", kind.code()));
        owner_cas.promote(bytes).unwrap();
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(&owner_journal)
            .unwrap();
        f.write_all(&mesh_store::frame_record(&StoredRecord::Dependency(record)))
            .unwrap();
        f.sync_all().unwrap();
    };
    let owner_intent = Json::parse(std::str::from_utf8(&raws[1]).unwrap()).unwrap();
    append(
        DependencyKind::Consumption,
        owner_intent.get("body").unwrap().clone(),
    );
    let committed = fs::read(&owner_journal).unwrap();
    assert!(inspect().is_err());
    assert_eq!(fs::read(&owner_journal).unwrap(), committed);
    fs::write(&owner_journal, &owner_before).unwrap();
    let grant_bytes = crate::project_attachment::dependency_transaction::read_payload(
        &owner_cas,
        pending.grant,
        65536,
    )
    .unwrap();
    let mut grant_body = Json::parse(std::str::from_utf8(&grant_bytes).unwrap())
        .unwrap()
        .get("body")
        .unwrap()
        .clone();
    let Json::Object(fields) = &mut grant_body else {
        panic!("grant body missing")
    };
    for (name, value) in fields {
        match name.as_str() {
            "request" => *value = Json::text(id(229).to_hex()),
            "generation" => *value = Json::Number(2),
            "previous" => *value = Json::text(pending.grant.to_hex()),
            "allowed" => *value = Json::Bool(false),
            _ => {}
        }
    }
    append(DependencyKind::Grant, grant_body);
    let revoked = fs::read(&owner_journal).unwrap();
    assert!(inspect().is_err());
    assert_eq!(fs::read(&owner_journal).unwrap(), revoked);
    assert_eq!(fs::read(&pending.journal).unwrap(), full);
}
