//! Real retained start material selected before, and inspected after, native publication.
use super::*;
use crate::project_attachment::{
    NativeConsumedStartRequest, NativeGrantInspection, NativeInputGrantRequest,
};
pub(super) struct PendingStart {
    destination: ProvisionedAttachment,
    version: crate::project_attachment::SavedAttachmentVersion,
    grant: RecordDigest,
    journal: PathBuf,
    before: Vec<u8>,
    frame: Vec<u8>,
}
pub(super) fn prepare(
    storage: &AttachmentStorage,
    owner: &ProvisionedAttachment,
    source: &ProvisionedAttachment,
    version: crate::project_attachment::SavedAttachmentVersion,
) -> PendingStart {
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
    PendingStart {
        destination,
        version,
        grant,
        journal,
        before,
        frame,
    }
}
pub(super) fn inspect(
    storage: &AttachmentStorage,
    owner: &ProvisionedAttachment,
    source: &ProvisionedAttachment,
    pending: PendingStart,
    trusted: &crate::TrustedReviewers,
) {
    use crate::project_attachment::dependency_transaction::{digest, hash};
    use std::collections::BTreeSet;
    use std::os::unix::fs::MetadataExt as _;
    let available = [owner];
    let request = || NativeConsumedStartRequest {
        input: NativeGrantInspection {
            source,
            version: pending.version,
            destination: &pending.destination,
            grant: pending.grant,
        },
        available: &available,
        request: RecordDigest::from_bytes([222; 32]),
        limits: ObservationLimits::default(),
    };
    let inspect = || storage.inspect_native_consumption_start_retention(owner, request(), trusted);
    let owner_journal = owner.metadata_path().join(crate::RECORD_FILE_NAME);
    let owner_before = fs::read(&owner_journal).unwrap();
    let source_journal = source.metadata_path().join(crate::RECORD_FILE_NAME);
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
    fs::write(git.join("index"), b"synthetic dirty user index").unwrap();
    let source_inventory = storage
        .inspect_native_history_retention(source.id(), trusted)
        .unwrap();
    let owner_inventory = storage
        .inspect_native_history_retention(owner.id(), trusted)
        .unwrap();
    let sidecar = pending
        .destination
        .metadata_path()
        .join("consumption-start.pending");
    let raw = fs::read(&sidecar).unwrap();
    let intent = Json::parse(std::str::from_utf8(&raw).unwrap()).unwrap();
    let payload = digest(intent.get("payload").and_then(Json::as_text).unwrap()).unwrap();
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
    let start = read_json(payload);
    let body = start.get("body").unwrap();
    let field_id =
        |value: &Json, name| digest(value.get(name).and_then(Json::as_text).unwrap()).unwrap();
    let descriptor = field_id(body, "staged");
    let stage = field_id(&read_json(descriptor), "stage");
    let stage_json = read_json(stage);
    let Json::Array(objects) = stage_json.get("objects").unwrap() else {
        panic!("staged objects absent")
    };
    let operation = field_id(&stage_json, "operation");
    let mut expected = objects.to_vec();
    for id in [
        payload,
        descriptor,
        stage,
        field_id(body, "configuration"),
        field_id(body, "prospective"),
        field_id(body, "closure"),
        field_id(&start, "authority"),
    ] {
        expected.push(Json::text(id.to_hex()));
    }
    let scan = mesh_store::scan_journal(&pending.before).unwrap();
    let [StoredRecord::Dependency(enrollment)] = scan.records() else {
        panic!("physical enrollment prefix differs")
    };
    expected.push(Json::text(enrollment.payload.to_hex()));
    let expected = expected
        .iter()
        .map(|id| id.as_text().unwrap().to_owned())
        .collect::<BTreeSet<_>>();
    // Use the exact pinned allocation's published stage, rather than guessing its location.
    let origin = storage
        .lane_origin_bound(&pending.destination)
        .unwrap()
        .unwrap();
    let stage_root = origin
        .allocation
        .open_child_directory(std::ffi::OsStr::new(&format!(
            "consumption-{}",
            request().request.to_hex()
        )))
        .unwrap();
    let (stage_path, _namespace) = stage_root.stable_namespace().unwrap();
    let original_frames = fs::read(stage_path.join("frames.mesh")).unwrap();
    let preserve = || {
        assert_eq!(fs::read(&owner_journal).unwrap(), owner_before);
        assert_eq!(fs::read(&source_journal).unwrap(), source_before);
        assert_eq!(fs::read(&sidecar).unwrap(), raw);
        for (p, bytes) in notes.iter().zip(&original_notes) {
            assert_eq!(fs::read(p).unwrap(), *bytes);
        }
        assert_eq!(
            fs::read(git.join("HEAD")).unwrap(),
            b"ref: refs/heads/user-work\n"
        );
        assert_eq!(
            fs::read(git.join("index")).unwrap(),
            b"synthetic dirty user index"
        );
        assert_eq!(
            fs::read_dir(pending.destination.project().root())
                .unwrap()
                .count(),
            0
        );
    };
    for length in [0, 1, pending.frame.len() - 1, pending.frame.len()] {
        let mut observed = pending.before.clone();
        observed.extend_from_slice(&pending.frame[..length]);
        fs::write(&pending.journal, &observed).unwrap();
        let inventory = inspect().unwrap();
        assert_eq!(
            inventory.get("schema"),
            Some(&Json::text("mesh.native-consumption-start-retention/v1"))
        );
        assert_eq!(
            inventory.get("collection_authority"),
            Some(&Json::Bool(false))
        );
        let Json::Array(stores) = inventory.get("stores").unwrap() else {
            panic!("store facts absent")
        };
        assert!(stores.contains(&owner_inventory) && stores.contains(&source_inventory));
        assert_eq!(stores.len(), if source.id() == owner.id() { 1 } else { 2 });
        let destination = inventory.get("destination").unwrap();
        assert_eq!(
            destination.get("collection_authority"),
            Some(&Json::Bool(false))
        );
        let physical = fs::metadata(pending.destination.metadata_path()).unwrap();
        for (name, value) in [("device", physical.dev()), ("inode", physical.ino())] {
            assert_eq!(
                destination.get(name),
                Some(&Json::text(format!("{value:x}")))
            );
        }
        assert_ne!(
            destination.get("installation"),
            owner_inventory.get("installation")
        );
        let Json::Array(payloads) = destination.get("payloads").unwrap() else {
            panic!("payload facts absent")
        };
        assert_eq!(
            payloads
                .iter()
                .map(|id| id.as_text().unwrap().to_owned())
                .collect::<BTreeSet<_>>(),
            expected
        );
        assert_eq!(
            destination.get("staged_operations"),
            Some(&Json::Array(vec![Json::text(operation.to_hex())]))
        );
        assert_eq!(
            destination.get("sidecars"),
            Some(&Json::Array(vec![Json::object([
                ("name", Json::text("consumption-start.pending")),
                ("digest", Json::text(hash(&raw).to_hex()))
            ])]))
        );
        let selection = inventory.get("source").unwrap();
        assert_eq!(
            selection.get("operation"),
            Some(&Json::text(pending.version.operation().to_hex()))
        );
        assert_eq!(
            selection.get("grant"),
            Some(&Json::text(pending.grant.to_hex()))
        );
        let (d, i) = origin.allocation.identity().unwrap();
        assert_eq!(
            destination.get("stage").unwrap().get("allocation_identity"),
            Some(&Json::text(format!("{d:016x}:{i:016x}")))
        );
        let journal = destination.get("journal").unwrap();
        assert_eq!(
            journal.get("digest"),
            Some(&Json::text(hash(&observed).to_hex()))
        );
        assert_eq!(
            journal.get("written_frame_bytes"),
            Some(&Json::Number(length as u64))
        );
        assert_eq!(
            journal.get("prefix_bytes"),
            Some(&Json::Number(pending.before.len() as u64))
        );
        assert_eq!(
            journal.get("frame_bytes"),
            Some(&Json::Number(pending.frame.len() as u64))
        );
        assert_eq!(fs::read(&pending.journal).unwrap(), observed);
        if length == 0 {
            // Existing staged-only readers may return empty history; they admit no start version.
            assert!(pending.destination.saved_versions().unwrap().is_empty());
        } else {
            assert!(pending.destination.saved_versions().is_err());
        }
        assert!(
            storage
                .recover_fenced_consumed_start(owner, request())
                .is_err(),
            "ordinary/default-trust published recovery must remain fenced"
        );
        preserve();
        if length == 0 {
            assert!(storage
                .inspect_native_consumption_start_retention(
                    owner,
                    request(),
                    &crate::TrustedReviewers::default()
                )
                .is_err());
            for mode in 0..7 {
                let mut wrong = request();
                match mode {
                    0 => wrong.request = RecordDigest::from_bytes([0; 32]),
                    1 => wrong.request = RecordDigest::from_bytes([223; 32]),
                    2 => wrong.input.grant = RecordDigest::from_bytes([224; 32]),
                    3 => wrong.limits.entries -= 1,
                    4 => wrong.input.destination = source,
                    5 => wrong.limits.entries = 0,
                    _ => wrong.input.source = &pending.destination,
                }
                assert!(storage
                    .inspect_native_consumption_start_retention(owner, wrong, trusted)
                    .is_err());
            }
            let excessive = vec![owner; 257];
            let mut wrong = request();
            wrong.available = &excessive;
            assert!(storage
                .inspect_native_consumption_start_retention(owner, wrong, trusted)
                .is_err());
            let owner_for_wrong = &pending.destination;
            assert!(storage
                .inspect_native_consumption_start_retention(owner_for_wrong, request(), trusted)
                .is_err());
            for id in [payload, descriptor, stage, operation] {
                let path = local_path(id);
                let bytes = fs::read(&path).unwrap();
                let permissions = fs::metadata(&path).unwrap().permissions();
                fs::write(&path, b"corrupt retained start object").unwrap();
                assert!(inspect().is_err());
                assert_eq!(fs::read(&path).unwrap(), b"corrupt retained start object");
                fs::remove_file(&path).unwrap();
                assert!(inspect().is_err());
                assert!(!path.exists());
                fs::write(&path, &bytes).unwrap();
                fs::set_permissions(&path, permissions).unwrap();
                assert_eq!(inspect().unwrap(), inventory);
            }
            let frames_path = stage_path.join("frames.mesh");
            let frame_permissions = fs::metadata(&frames_path).unwrap().permissions();
            fs::write(&frames_path, b"corrupt staged frames").unwrap();
            assert!(inspect().is_err());
            assert_eq!(fs::read(&frames_path).unwrap(), b"corrupt staged frames");
            fs::remove_file(&frames_path).unwrap();
            assert!(inspect().is_err());
            assert!(!frames_path.exists());
            fs::write(&frames_path, &original_frames).unwrap();
            fs::set_permissions(&frames_path, frame_permissions).unwrap();
            assert_eq!(inspect().unwrap(), inventory);
            let source_cas = Cas::<_, Blake3>::with_filesystem(
                source.metadata_path(),
                source.store.filesystem().read_only(),
            )
            .unwrap();
            for name in ["operations", "payloads"] {
                let Json::Array(roots) = source_inventory.get(name).unwrap() else {
                    panic!("source roots absent")
                };
                let id = digest(roots.first().unwrap().as_text().unwrap()).unwrap();
                let path = source.metadata_path().join(
                    source_cas
                        .layout()
                        .chunk_path(&mesh_cas::Digest32::from_bytes(*id.as_bytes())),
                );
                let bytes = fs::read(&path).unwrap();
                let permissions = fs::metadata(&path).unwrap().permissions();
                fs::write(&path, b"corrupt historical source").unwrap();
                assert!(inspect().is_err());
                assert_eq!(fs::read(&path).unwrap(), b"corrupt historical source");
                fs::remove_file(&path).unwrap();
                assert!(inspect().is_err());
                assert!(!path.exists());
                fs::write(&path, &bytes).unwrap();
                fs::set_permissions(&path, permissions).unwrap();
                assert_eq!(inspect().unwrap(), inventory);
            }
            fs::write(&sidecar, b"foreign start intent").unwrap();
            assert!(inspect().is_err());
            assert_eq!(fs::read(&sidecar).unwrap(), b"foreign start intent");
            fs::write(&sidecar, &raw).unwrap();
            let foreign = pending
                .destination
                .metadata_path()
                .join("dependency-decision.pending");
            fs::write(&foreign, b"foreign control").unwrap();
            assert!(inspect().is_err());
            assert_eq!(fs::read(&foreign).unwrap(), b"foreign control");
            fs::remove_file(foreign).unwrap();
            let editor = pending
                .destination
                .project()
                .root()
                .join("user-editor-work");
            fs::write(&editor, b"preserve editor bytes").unwrap();
            assert!(inspect().is_err());
            assert_eq!(fs::read(&editor).unwrap(), b"preserve editor bytes");
            fs::remove_file(editor).unwrap();
            assert_eq!(inspect().unwrap(), inventory);
            preserve();
        }
        let mut foreign = observed.clone();
        foreign.push(if length < pending.frame.len() {
            pending.frame[length] ^ 0xff
        } else {
            0xff
        });
        fs::write(&pending.journal, &foreign).unwrap();
        assert!(inspect().is_err());
        assert_eq!(fs::read(&pending.journal).unwrap(), foreign);
        fs::write(&pending.journal, &observed).unwrap();
        assert_eq!(inspect().unwrap(), inventory);
        preserve();
    }
}
