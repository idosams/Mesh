//! Test-native-policy staging after real publication, not a supported product owner writer.
use super::consumption_install_retention::PendingInstalled;
use super::*;
use crate::project_attachment::dependency_transaction::hash;
use crate::project_attachment::{NativeConsumedStartRequest, NativeGrantInspection};
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

pub(super) fn inspect(
    storage: &AttachmentStorage,
    owner: &ProvisionedAttachment,
    source: &ProvisionedAttachment,
    pending: PendingInstalled,
    trusted: &crate::TrustedReviewers,
) {
    let request_id = RecordDigest::from_bytes([222; 32]);
    let available = [owner];
    let request = || NativeConsumedStartRequest {
        input: NativeGrantInspection {
            source,
            version: pending.version,
            destination: &pending.destination,
            grant: pending.grant,
        },
        available: &available,
        request: request_id,
        limits: ObservationLimits::default(),
    };
    let before = storage
        .inspect_native_consumption_install_retention(owner, request(), trusted)
        .unwrap();
    assert_eq!(before.get("collection_authority"), Some(&Json::Bool(false)));
    // The existing ordinary owner writer cannot verify native publication history. Build only
    // exact persisted fixture bytes with the actual policy validator and canonical intent encoder.
    let owner_journal = owner.metadata_path().join(crate::RECORD_FILE_NAME);
    let owner_before = fs::read(&owner_journal).unwrap();
    let (record, bytes) = {
        let _guard = crate::workspace_custody::lock_workspace_initialization(&owner.store).unwrap();
        let (_, proof) = owner
            .project()
            .read_publication_private_history(owner.metadata_path(), &owner.store)
            .unwrap();
        let (ordinal, previous) = proof.policy().native_head().unwrap();
        let installation = Json::parse(
            &fs::read_to_string(
                pending
                    .destination
                    .metadata_path()
                    .join("consumption-owner.pending"),
            )
            .unwrap(),
        )
        .unwrap();
        let bytes = Json::object([
            ("schema", Json::text("mesh.dependency-policy/v1")),
            ("authority", Json::text(proof.binding().authority.to_hex())),
            ("revision", Json::Number(ordinal + 1)),
            ("previous", Json::text(previous.to_hex())),
            (
                "kind",
                Json::Number(DependencyKind::Consumption.code().into()),
            ),
            ("body", installation.get("body").unwrap().clone()),
        ])
        .encode()
        .into_bytes();
        let record = DependencyRecord {
            authority: proof.binding().authority,
            revision: ordinal + 1,
            previous,
            payload: hash(&bytes),
            kind: DependencyKind::Consumption,
        };
        proof.policy().clone().apply(record, &bytes).unwrap();
        (record, bytes)
    };
    let cas =
        Cas::<_, Blake3>::with_filesystem(owner.metadata_path(), owner.store.filesystem()).unwrap();
    cas.promote(bytes).unwrap();
    let metadata = fs::metadata(&owner_journal).unwrap();
    let raw = crate::project_attachment::dependency_decision::transaction_intent(
        DependencyKind::Consumption,
        request_id,
        (metadata.dev(), metadata.ino()),
        &owner_before,
        record.payload,
    );
    let name = crate::project_attachment::dependency_owner_context::pending_name(
        request_id,
        record.payload,
    );
    let sidecar = owner.metadata_path().join(&name);
    fs::write(&sidecar, &raw).unwrap();
    fs::set_permissions(&sidecar, fs::Permissions::from_mode(0o600)).unwrap();
    let inspect = || storage.inspect_native_consumption_owner_retention(owner, request(), trusted);
    let owner_frame = mesh_store::frame_record(&StoredRecord::Dependency(record));
    let destination_before = fs::read(&pending.journal).unwrap();
    let saved_files = [
        owner.project().root().join("note"),
        source.project().root().join("note"),
        pending.destination.project().root().join("note"),
        pending.destination.project().root().join("second"),
        pending.destination.project().root().join("tree/child"),
    ]
    .into_iter()
    .map(|path| {
        let m = fs::metadata(&path).unwrap();
        let bytes = fs::read(&path).unwrap();
        (path, (m.dev(), m.ino()), bytes)
    })
    .collect::<Vec<_>>();
    assert_eq!(saved_files[3].2, b"second exact input");
    assert_eq!(saved_files[4].2, b"nested exact input");
    let git = owner.project().root().join(".git");
    fs::create_dir(&git).unwrap();
    fs::write(git.join("HEAD"), b"ref: refs/heads/preserve-dirty\n").unwrap();
    fs::write(git.join("index"), b"synthetic staged user index").unwrap();
    let untracked = owner.project().root().join("untracked-owner-work");
    fs::write(&untracked, b"preserve untracked owner work").unwrap();
    let source_before = fs::read(source.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap();
    let preserve = |expected_owner: &[u8]| {
        assert_eq!(fs::read(&owner_journal).unwrap(), expected_owner);
        if source.id() != owner.id() {
            assert_eq!(
                fs::read(source.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap(),
                source_before
            );
        }
        assert_eq!(fs::read(&pending.journal).unwrap(), destination_before);
        assert_eq!(fs::read(&sidecar).unwrap(), raw.as_bytes());
        assert_eq!(
            fs::read(git.join("HEAD")).unwrap(),
            b"ref: refs/heads/preserve-dirty\n"
        );
        assert_eq!(
            fs::read(git.join("index")).unwrap(),
            b"synthetic staged user index"
        );
        assert_eq!(
            fs::read(&untracked).unwrap(),
            b"preserve untracked owner work"
        );
        for (path, identity, bytes) in &saved_files {
            let m = fs::metadata(path).unwrap();
            assert_eq!((m.dev(), m.ino()), *identity);
            assert_eq!(fs::read(path).unwrap(), *bytes);
        }
    };
    let array_set = |value: &Json, key: &str| {
        let Json::Array(values) = value.get(key).unwrap() else {
            panic!("array {key} missing")
        };
        values
            .iter()
            .map(|v| v.as_text().unwrap().to_owned())
            .collect::<std::collections::BTreeSet<_>>()
    };
    let Json::Array(old_stores) = before.get("stores").unwrap() else {
        panic!("old stores missing")
    };
    let owner_identity = owner.store.identity().unwrap();
    let is_owner = |store: &Json| {
        store.get("device") == Some(&Json::text(format!("{:x}", owner_identity.0)))
            && store.get("inode") == Some(&Json::text(format!("{:x}", owner_identity.1)))
    };
    let old_owner = old_stores.iter().find(|store| is_owner(store)).unwrap();
    let mut expected_owner_payloads = array_set(old_owner, "payloads");
    expected_owner_payloads.insert(record.payload.to_hex());
    // Byte-prefix truncations simulate boundaries. No process-kill or writable owner API claim.
    for written in [0, 1, owner_frame.len() - 1, owner_frame.len()] {
        let mut observed = owner_before.clone();
        observed.extend_from_slice(&owner_frame[..written]);
        fs::write(&owner_journal, &observed).unwrap();
        let facts = inspect().unwrap();
        assert_eq!(inspect().unwrap(), facts);
        assert_eq!(
            facts.get("schema"),
            Some(&Json::text("mesh.native-consumption-owner-retention/v1"))
        );
        assert_eq!(
            facts.get("scope"),
            Some(&Json::text("owner-receipt-before-destination-completion"))
        );
        assert_eq!(facts.get("collection_authority"), Some(&Json::Bool(false)));
        assert_eq!(facts.get("destination"), before.get("destination"));
        let Json::Array(stores) = facts.get("stores").unwrap() else {
            panic!("stores missing")
        };
        assert_eq!(stores.len(), old_stores.len());
        for old in old_stores {
            if !is_owner(old) {
                assert!(stores.contains(old));
            }
        }
        let current = stores.iter().find(|store| is_owner(store)).unwrap();
        assert_eq!(
            current.get("schema"),
            Some(&Json::text(
                "mesh.native-history-consumption-owner-retention/v1"
            ))
        );
        assert_eq!(
            current.get("collection_authority"),
            Some(&Json::Bool(false))
        );
        for field in [
            "work",
            "installation",
            "device",
            "inode",
            "operations",
            "manifests",
        ] {
            assert_eq!(current.get(field), old_owner.get(field), "{field}");
        }
        assert_eq!(array_set(current, "payloads"), expected_owner_payloads);
        let frame = current.get("consumption").unwrap();
        assert_eq!(
            frame.get("payload"),
            Some(&Json::text(record.payload.to_hex()))
        );
        assert_eq!(frame.get("request"), Some(&Json::text(request_id.to_hex())));
        assert_eq!(frame.get("sidecar"), Some(&Json::text(&name)));
        assert_eq!(
            frame.get("sidecar_digest"),
            Some(&Json::text(hash(raw.as_bytes()).to_hex()))
        );
        assert_eq!(
            frame.get("journal_digest"),
            Some(&Json::text(hash(&observed).to_hex()))
        );
        assert_eq!(
            frame.get("prefix_bytes"),
            Some(&Json::Number(owner_before.len() as u64))
        );
        assert_eq!(
            frame.get("written_frame_bytes"),
            Some(&Json::Number(written as u64))
        );
        assert_eq!(
            frame.get("frame_bytes"),
            Some(&Json::Number(owner_frame.len() as u64))
        );
        let Json::Array(old_sidecars) = old_owner.get("sidecars").unwrap() else {
            panic!("old sidecars missing")
        };
        let Json::Array(sidecars) = current.get("sidecars").unwrap() else {
            panic!("sidecars missing")
        };
        assert_eq!(sidecars.len(), old_sidecars.len() + 1);
        for receipt in old_sidecars {
            assert!(sidecars.contains(receipt));
        }
        assert!(sidecars.contains(&Json::object([
            ("name", Json::text(&name)),
            ("digest", Json::text(hash(raw.as_bytes()).to_hex()))
        ])));
        assert!(owner.saved_versions().is_err());
        assert!(pending.destination.saved_versions().is_err());
        assert!(storage
            .inspect_native_consumption_start_retention(owner, request(), trusted)
            .is_err());
        assert!(storage
            .inspect_native_consumption_install_retention(owner, request(), trusted)
            .is_err());
        assert!(storage
            .recover_installing_consumed_start(owner, request())
            .is_err());
        preserve(&observed);
        if written > 0 {
            let mut foreign = observed.clone();
            *foreign.last_mut().unwrap() ^= 1;
            fs::write(&owner_journal, &foreign).unwrap();
            assert!(inspect().is_err());
            assert_eq!(fs::read(&owner_journal).unwrap(), foreign);
            fs::write(&owner_journal, &observed).unwrap();
        }
    }
    let mut complete = owner_before.clone();
    complete.extend_from_slice(&owner_frame);
    fs::write(&owner_journal, &complete).unwrap();
    assert!(storage
        .inspect_native_consumption_owner_retention(
            owner,
            request(),
            &crate::TrustedReviewers::default()
        )
        .is_err());
    let mut wrong = request();
    wrong.request = RecordDigest::from_bytes([0; 32]);
    assert!(storage
        .inspect_native_consumption_owner_retention(owner, wrong, trusted)
        .is_err());
    let mut wrong = request();
    wrong.request = RecordDigest::from_bytes([223; 32]);
    assert!(storage
        .inspect_native_consumption_owner_retention(owner, wrong, trusted)
        .is_err());
    let mut wrong = request();
    wrong.input.grant = RecordDigest::from_bytes([223; 32]);
    assert!(storage
        .inspect_native_consumption_owner_retention(owner, wrong, trusted)
        .is_err());
    let mut wrong = request();
    wrong.limits.file_bytes -= 1;
    assert!(storage
        .inspect_native_consumption_owner_retention(owner, wrong, trusted)
        .is_err());
    preserve(&complete);
    let ambiguous = owner.metadata_path().join(format!(
        "consumption-owner-{}-{}.pending",
        request_id.to_hex(),
        RecordDigest::from_bytes([224; 32]).to_hex()
    ));
    fs::write(&ambiguous, &raw).unwrap();
    fs::set_permissions(&ambiguous, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(inspect().is_err());
    assert_eq!(fs::read(&ambiguous).unwrap(), raw.as_bytes());
    fs::remove_file(&ambiguous).unwrap();
    for name in [
        crate::project_attachment::consumption_complete::PENDING,
        crate::project_attachment::dependency_decision::PENDING,
        crate::project_attachment::history::dependency_capture::PENDING,
        crate::project_attachment::dependency_private_context::publication::PENDING,
    ] {
        let path = pending.destination.metadata_path().join(name);
        fs::write(&path, b"foreign phase").unwrap();
        assert!(inspect().is_err());
        assert_eq!(fs::read(&path).unwrap(), b"foreign phase");
        fs::remove_file(&path).unwrap();
    }
    let owner_payload = owner.metadata_path().join(
        cas.layout()
            .chunk_path(&mesh_cas::Digest32::from_bytes(*record.payload.as_bytes())),
    );
    let history_path = pending
        .destination
        .metadata_path()
        .join(crate::project_attachment::consumption_history::PENDING);
    let installation_path = pending
        .destination
        .metadata_path()
        .join("consumption-owner.pending");
    for path in [&sidecar, &owner_payload, &history_path, &installation_path] {
        let bytes = fs::read(path).unwrap();
        let permissions = fs::metadata(path).unwrap().permissions();
        let backup = path.with_extension("owner-test-backup");
        fs::rename(path, &backup).unwrap();
        assert!(inspect().is_err());
        assert!(!path.exists());
        fs::rename(&backup, path).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(path, b"corrupt retained owner input").unwrap();
        assert!(inspect().is_err());
        assert_eq!(fs::read(path).unwrap(), b"corrupt retained owner input");
        fs::write(path, bytes).unwrap();
        fs::set_permissions(path, permissions).unwrap();
    }
    fs::set_permissions(&sidecar, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(inspect().is_err());
    assert_eq!(fs::metadata(&sidecar).unwrap().mode() & 0o077, 0o044);
    fs::set_permissions(&sidecar, fs::Permissions::from_mode(0o600)).unwrap();
    let alias = owner.metadata_path().join("owner-intent-alias");
    fs::hard_link(&sidecar, &alias).unwrap();
    assert!(inspect().is_err());
    assert_eq!(fs::metadata(&sidecar).unwrap().nlink(), 2);
    assert_eq!(fs::read(&alias).unwrap(), raw.as_bytes());
    fs::remove_file(&alias).unwrap();
    // Parsing whitespace is insufficient: the exact persisted intent must be canonical.
    let noncanonical = format!("{raw}\n");
    fs::write(&sidecar, &noncanonical).unwrap();
    assert!(inspect().is_err());
    assert_eq!(fs::read(&sidecar).unwrap(), noncanonical.as_bytes());
    fs::write(&sidecar, &raw).unwrap();
    // Historical authoring content is independently retained, not merely the new policy frame.
    let old_id = RecordDigest::parse_hex(
        expected_owner_payloads
            .iter()
            .find(|id| *id != &record.payload.to_hex() && *id != &pending.grant.to_hex())
            .unwrap(),
    )
    .unwrap();
    let old_path = owner.metadata_path().join(
        cas.layout()
            .chunk_path(&mesh_cas::Digest32::from_bytes(*old_id.as_bytes())),
    );
    let backup = old_path.with_extension("old-owner-root-backup");
    fs::rename(&old_path, &backup).unwrap();
    assert!(inspect().is_err());
    assert!(!old_path.exists());
    fs::rename(&backup, &old_path).unwrap();
    // A substituted journal cannot reuse the sidecar's pinned original identity.
    let backup = owner_journal.with_extension("owner-test-original");
    fs::rename(&owner_journal, &backup).unwrap();
    fs::write(&owner_journal, &complete).unwrap();
    assert!(inspect().is_err());
    assert_eq!(fs::read(&owner_journal).unwrap(), complete);
    fs::remove_file(&owner_journal).unwrap();
    fs::rename(&backup, &owner_journal).unwrap();
    // Owner acknowledgement is only valid after complete installed checkpoint, not its prefix.
    let mut partial_destination = pending.before.clone();
    partial_destination.extend_from_slice(&pending.frame);
    partial_destination.extend_from_slice(&pending.checkpoint[..pending.checkpoint.len() - 1]);
    fs::write(&pending.journal, &partial_destination).unwrap();
    assert!(inspect().is_err());
    assert_eq!(fs::read(&pending.journal).unwrap(), partial_destination);
    fs::write(&pending.journal, &destination_before).unwrap();
    let editor = pending.destination.project().root().join("editor-work");
    fs::write(&editor, b"preserve live editor").unwrap();
    assert!(inspect().is_err());
    assert_eq!(fs::read(&editor).unwrap(), b"preserve live editor");
    fs::remove_file(editor).unwrap();
    preserve(&complete);
    assert!(inspect().is_ok());
    // Revocation-first: stage the same exact receipt against a new, independently validated
    // native grant generation. Native policy refuses the obsolete receipt, and inspection
    // must preserve that refusal. This does not claim historical owner-first recovery.
    fs::remove_file(&sidecar).unwrap();
    fs::write(&owner_journal, &owner_before).unwrap();
    let grant_bytes =
        crate::project_attachment::dependency_transaction::read_payload(&cas, pending.grant, 65536)
            .unwrap();
    let mut grant_body = Json::parse(std::str::from_utf8(&grant_bytes).unwrap())
        .unwrap()
        .get("body")
        .unwrap()
        .clone();
    let Json::Object(fields) = &mut grant_body else {
        panic!("grant body missing")
    };
    for (field, value) in fields {
        match field.as_str() {
            "request" => *value = Json::text(RecordDigest::from_bytes([229; 32]).to_hex()),
            "generation" => *value = Json::Number(2),
            "previous" => *value = Json::text(pending.grant.to_hex()),
            "allowed" => *value = Json::Bool(false),
            _ => {}
        }
    }
    let (revoked, next_record, next_bytes) = {
        let _guard = crate::workspace_custody::lock_workspace_initialization(&owner.store).unwrap();
        let (_, proof) = owner
            .project()
            .read_publication_private_history(owner.metadata_path(), &owner.store)
            .unwrap();
        let (ordinal, previous) = proof.policy().native_head().unwrap();
        let grant_payload = Json::object([
            ("schema", Json::text("mesh.dependency-policy/v2")),
            ("authority", Json::text(proof.binding().authority.to_hex())),
            ("revision", Json::Number(ordinal + 1)),
            ("previous", Json::text(previous.to_hex())),
            ("kind", Json::Number(DependencyKind::Grant.code().into())),
            ("body", grant_body),
        ])
        .encode()
        .into_bytes();
        let grant_record = DependencyRecord {
            authority: proof.binding().authority,
            revision: ordinal + 1,
            previous,
            payload: hash(&grant_payload),
            kind: DependencyKind::Grant,
        };
        let mut policy = proof.policy().clone();
        policy.apply(grant_record, &grant_payload).unwrap();
        cas.promote(grant_payload).unwrap();
        let mut revoked = owner_before.clone();
        revoked.extend_from_slice(&mesh_store::frame_record(&StoredRecord::Dependency(
            grant_record,
        )));
        fs::write(&owner_journal, &revoked).unwrap();
        let next_bytes = Json::object([
            ("schema", Json::text("mesh.dependency-policy/v1")),
            ("authority", Json::text(record.authority.to_hex())),
            ("revision", Json::Number(ordinal + 2)),
            ("previous", Json::text(grant_record.payload.to_hex())),
            (
                "kind",
                Json::Number(DependencyKind::Consumption.code().into()),
            ),
            (
                "body",
                Json::parse(
                    std::str::from_utf8(
                        &crate::project_attachment::dependency_transaction::read_payload(
                            &cas,
                            record.payload,
                            65536,
                        )
                        .unwrap(),
                    )
                    .unwrap(),
                )
                .unwrap()
                .get("body")
                .unwrap()
                .clone(),
            ),
        ])
        .encode()
        .into_bytes();
        let next_record = DependencyRecord {
            authority: record.authority,
            revision: ordinal + 2,
            previous: grant_record.payload,
            payload: hash(&next_bytes),
            kind: DependencyKind::Consumption,
        };
        // Current policy may itself refuse an old grant; either native refusal remains intact.
        assert!(
            policy.apply(next_record, &next_bytes).is_err(),
            "revocation must refuse this receipt at the native policy boundary"
        );
        (revoked, next_record, next_bytes)
    };
    cas.promote(next_bytes).unwrap();
    let m = fs::metadata(&owner_journal).unwrap();
    let next_raw = crate::project_attachment::dependency_decision::transaction_intent(
        DependencyKind::Consumption,
        request_id,
        (m.dev(), m.ino()),
        &revoked,
        next_record.payload,
    );
    let next_sidecar = owner.metadata_path().join(
        crate::project_attachment::dependency_owner_context::pending_name(
            request_id,
            next_record.payload,
        ),
    );
    fs::write(&next_sidecar, &next_raw).unwrap();
    fs::set_permissions(&next_sidecar, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(inspect().is_err());
    assert_eq!(fs::read(&owner_journal).unwrap(), revoked);
    assert_eq!(fs::read(&next_sidecar).unwrap(), next_raw.as_bytes());
    assert_eq!(fs::read(&pending.journal).unwrap(), destination_before);
}
