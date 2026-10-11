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
    let frame = mesh_store::frame_record(&StoredRecord::Dependency(record));
    let mut complete = owner_before.clone();
    complete.extend_from_slice(&frame);
    fs::write(&owner_journal, &complete).unwrap();
    // Test-native-policy staging: owner-first, then a valid later revocation.
    let grant_bytes =
        crate::project_attachment::dependency_transaction::read_payload(&cas, pending.grant, 65536)
            .unwrap();
    let mut body = Json::parse(std::str::from_utf8(&grant_bytes).unwrap())
        .unwrap()
        .get("body")
        .unwrap()
        .clone();
    let Json::Object(fields) = &mut body else {
        panic!("grant body missing")
    };
    for (name, value) in fields {
        match name.as_str() {
            "request" => *value = Json::text(RecordDigest::from_bytes([229; 32]).to_hex()),
            "generation" => *value = Json::Number(2),
            "previous" => *value = Json::text(pending.grant.to_hex()),
            "allowed" => *value = Json::Bool(false),
            _ => {}
        }
    }
    let (revoked, revocation) = {
        let _guard = crate::workspace_custody::lock_workspace_initialization(&owner.store).unwrap();
        let (_, proof) = owner
            .project()
            .read_publication_private_history(owner.metadata_path(), &owner.store)
            .unwrap();
        assert_eq!(proof.policy().native_request(request_id), Some(record));
        let (ordinal, previous) = proof.policy().native_head().unwrap();
        let bytes = Json::object([
            ("schema", Json::text("mesh.dependency-policy/v2")),
            ("authority", Json::text(proof.binding().authority.to_hex())),
            ("revision", Json::Number(ordinal + 1)),
            ("previous", Json::text(previous.to_hex())),
            ("kind", Json::Number(DependencyKind::Grant.code().into())),
            ("body", body),
        ])
        .encode()
        .into_bytes();
        let revoked_record = DependencyRecord {
            authority: proof.binding().authority,
            revision: ordinal + 1,
            previous,
            payload: hash(&bytes),
            kind: DependencyKind::Grant,
        };
        let mut policy = proof.policy().clone();
        policy.apply(revoked_record, &bytes).unwrap();
        assert_eq!(policy.native_request(request_id), Some(record));
        cas.promote(bytes).unwrap();
        let mut revoked = complete.clone();
        revoked.extend_from_slice(&mesh_store::frame_record(&StoredRecord::Dependency(
            revoked_record,
        )));
        fs::write(&owner_journal, &revoked).unwrap();
        (revoked, revoked_record.payload)
    };
    let inspect =
        || storage.inspect_native_consumption_committed_owner_retention(owner, request(), trusted);
    let destination_before = fs::read(&pending.journal).unwrap();
    let source_before = fs::read(source.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap();
    let saved = [
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
    assert_eq!(saved[3].2, b"second exact input");
    assert_eq!(saved[4].2, b"nested exact input");
    let git = owner.project().root().join(".git");
    fs::create_dir(&git).unwrap();
    fs::write(
        git.join("HEAD"),
        b"ref: refs/heads/preserve-owner-history\n",
    )
    .unwrap();
    fs::write(git.join("index"), b"synthetic staged user bytes").unwrap();
    let untracked = owner.project().root().join("untracked-owner-history");
    fs::write(&untracked, b"preserve untracked work").unwrap();
    let preserve = |expected: &[u8]| {
        assert_eq!(fs::read(&owner_journal).unwrap(), expected);
        assert_eq!(fs::read(&pending.journal).unwrap(), destination_before);
        assert_eq!(fs::read(&sidecar).unwrap(), raw.as_bytes());
        if source.id() != owner.id() {
            assert_eq!(
                fs::read(source.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap(),
                source_before
            );
        }
        assert_eq!(
            fs::read(git.join("HEAD")).unwrap(),
            b"ref: refs/heads/preserve-owner-history\n"
        );
        assert_eq!(
            fs::read(git.join("index")).unwrap(),
            b"synthetic staged user bytes"
        );
        assert_eq!(fs::read(&untracked).unwrap(), b"preserve untracked work");
        for (path, identity, bytes) in &saved {
            let m = fs::metadata(path).unwrap();
            assert_eq!((m.dev(), m.ino()), *identity);
            assert_eq!(fs::read(path).unwrap(), *bytes);
        }
    };
    let roots = |value: &Json, key: &str| {
        let Json::Array(values) = value.get(key).unwrap() else {
            panic!("missing {key}")
        };
        values
            .iter()
            .map(|v| v.as_text().unwrap().to_owned())
            .collect::<std::collections::BTreeSet<_>>()
    };
    let Json::Array(old_stores) = before.get("stores").unwrap() else {
        panic!("old stores absent")
    };
    let identity = owner.store.identity().unwrap();
    let is_owner = |store: &Json| {
        store.get("device") == Some(&Json::text(format!("{:x}", identity.0)))
            && store.get("inode") == Some(&Json::text(format!("{:x}", identity.1)))
    };
    let old_owner = old_stores.iter().find(|store| is_owner(store)).unwrap();
    // A retained intent and partial frame are not a historical completed receipt. These are
    // simulated byte boundaries, not process-kill or product owner-writer evidence.
    for written in [0, 1, frame.len() - 1] {
        let mut partial = owner_before.clone();
        partial.extend_from_slice(&frame[..written]);
        fs::write(&owner_journal, &partial).unwrap();
        assert!(inspect().is_err());
        preserve(&partial);
        assert!(storage
            .inspect_native_consumption_owner_retention(owner, request(), trusted)
            .is_ok());
    }
    for (observed, later) in [(&complete, None), (&revoked, Some(revocation))] {
        fs::write(&owner_journal, observed).unwrap();
        let facts = inspect().unwrap();
        assert_eq!(inspect().unwrap(), facts);
        assert_eq!(
            facts.get("schema"),
            Some(&Json::text(
                "mesh.native-consumption-committed-owner-retention/v1"
            ))
        );
        assert_eq!(
            facts.get("scope"),
            Some(&Json::text(
                "historical-owner-receipt-before-destination-completion"
            ))
        );
        assert_eq!(facts.get("collection_authority"), Some(&Json::Bool(false)));
        assert_eq!(facts.get("destination"), before.get("destination"));
        let Json::Array(stores) = facts.get("stores").unwrap() else {
            panic!("stores absent")
        };
        assert_eq!(stores.len(), old_stores.len());
        for old in old_stores {
            if !is_owner(old) {
                assert!(stores.contains(old));
            }
        }
        let current = stores.iter().find(|store| is_owner(store)).unwrap();
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
        assert_eq!(
            current.get("schema"),
            Some(&Json::text(
                "mesh.native-history-committed-consumption-owner-retention/v1"
            ))
        );
        let mut expected = roots(old_owner, "payloads");
        expected.insert(record.payload.to_hex());
        if let Some(id) = later {
            expected.insert(id.to_hex());
        }
        assert_eq!(roots(current, "payloads"), expected);
        let Json::Array(old_sidecars) = old_owner.get("sidecars").unwrap() else {
            panic!("old sidecars absent")
        };
        let Json::Array(sidecars) = current.get("sidecars").unwrap() else {
            panic!("sidecars absent")
        };
        assert_eq!(sidecars.len(), old_sidecars.len() + 1);
        for entry in old_sidecars {
            assert!(sidecars.contains(entry));
        }
        assert!(sidecars.contains(&Json::object([
            ("name", Json::text(&name)),
            ("digest", Json::text(hash(raw.as_bytes()).to_hex()))
        ])));
        let receipt = current.get("consumption_receipt").unwrap();
        assert_eq!(
            receipt.get("payload"),
            Some(&Json::text(record.payload.to_hex()))
        );
        assert_eq!(
            receipt.get("request"),
            Some(&Json::text(request_id.to_hex()))
        );
        assert_eq!(receipt.get("sidecar"), Some(&Json::text(&name)));
        assert_eq!(
            receipt.get("sidecar_digest"),
            Some(&Json::text(hash(raw.as_bytes()).to_hex()))
        );
        assert_eq!(
            receipt.get("journal_digest"),
            Some(&Json::text(hash(observed).to_hex()))
        );
        assert_eq!(
            receipt.get("prefix_digest"),
            Some(&Json::text(hash(&owner_before).to_hex()))
        );
        assert_eq!(
            receipt.get("prefix_bytes"),
            Some(&Json::Number(owner_before.len() as u64))
        );
        assert_eq!(
            receipt.get("frame_bytes"),
            Some(&Json::Number(frame.len() as u64))
        );
        assert_eq!(
            receipt.get("later_journal_bytes"),
            Some(&Json::Number((observed.len() - complete.len()) as u64))
        );
        assert!(owner.saved_versions().is_err());
        assert!(pending.destination.saved_versions().is_err());
        assert!(storage
            .recover_installing_consumed_start(owner, request())
            .is_err());
        if later.is_some() {
            assert!(storage
                .inspect_native_consumption_owner_retention(owner, request(), trusted)
                .is_err());
        }
        preserve(observed);
    }
    fs::write(&owner_journal, &revoked).unwrap();
    assert!(storage
        .inspect_native_consumption_committed_owner_retention(
            owner,
            request(),
            &crate::TrustedReviewers::default()
        )
        .is_err());
    let mut wrong = request();
    wrong.request = RecordDigest::from_bytes([0; 32]);
    assert!(storage
        .inspect_native_consumption_committed_owner_retention(owner, wrong, trusted)
        .is_err());
    let mut wrong = request();
    wrong.request = RecordDigest::from_bytes([223; 32]);
    assert!(storage
        .inspect_native_consumption_committed_owner_retention(owner, wrong, trusted)
        .is_err());
    let mut wrong = request();
    wrong.input.grant = RecordDigest::from_bytes([223; 32]);
    assert!(storage
        .inspect_native_consumption_committed_owner_retention(owner, wrong, trusted)
        .is_err());
    let mut wrong = request();
    wrong.limits.file_bytes -= 1;
    assert!(storage
        .inspect_native_consumption_committed_owner_retention(owner, wrong, trusted)
        .is_err());
    for name in [
        crate::project_attachment::consumption_complete::PENDING,
        crate::project_attachment::consumption_history::PENDING,
    ] {
        let path = pending.destination.metadata_path().join(name);
        let original = fs::read(&path).ok();
        let permissions = fs::metadata(&path).ok().map(|m| m.permissions());
        fs::write(&path, b"foreign destination phase").unwrap();
        assert!(inspect().is_err());
        assert_eq!(fs::read(&path).unwrap(), b"foreign destination phase");
        if let Some(bytes) = original {
            fs::write(&path, bytes).unwrap();
            fs::set_permissions(&path, permissions.unwrap()).unwrap();
        } else {
            fs::remove_file(path).unwrap();
        }
    }
    let ambiguous = owner.metadata_path().join(format!(
        "consumption-owner-{}-{}.pending",
        request_id.to_hex(),
        RecordDigest::from_bytes([224; 32]).to_hex()
    ));
    fs::write(&ambiguous, &raw).unwrap();
    fs::set_permissions(&ambiguous, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(inspect().is_err());
    assert_eq!(fs::read(&ambiguous).unwrap(), raw.as_bytes());
    fs::remove_file(ambiguous).unwrap();
    let owner_payload = owner.metadata_path().join(
        cas.layout()
            .chunk_path(&mesh_cas::Digest32::from_bytes(*record.payload.as_bytes())),
    );
    let later_payload = owner.metadata_path().join(
        cas.layout()
            .chunk_path(&mesh_cas::Digest32::from_bytes(*revocation.as_bytes())),
    );
    let history_path = pending
        .destination
        .metadata_path()
        .join(crate::project_attachment::consumption_history::PENDING);
    let install_path = pending
        .destination
        .metadata_path()
        .join("consumption-owner.pending");
    for path in [
        &sidecar,
        &owner_payload,
        &later_payload,
        &history_path,
        &install_path,
    ] {
        let bytes = fs::read(path).unwrap();
        let permissions = fs::metadata(path).unwrap().permissions();
        let backup = path.with_extension("historical-owner-backup");
        fs::rename(path, &backup).unwrap();
        assert!(inspect().is_err());
        assert!(!path.exists());
        fs::rename(&backup, path).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(path, b"corrupt retained receipt").unwrap();
        assert!(inspect().is_err());
        assert_eq!(fs::read(path).unwrap(), b"corrupt retained receipt");
        fs::write(path, bytes).unwrap();
        fs::set_permissions(path, permissions).unwrap();
    }
    let old_id =
        RecordDigest::parse_hex(roots(old_owner, "payloads").iter().next().unwrap()).unwrap();
    let old_path = owner.metadata_path().join(
        cas.layout()
            .chunk_path(&mesh_cas::Digest32::from_bytes(*old_id.as_bytes())),
    );
    let backup = old_path.with_extension("historical-root-backup");
    fs::rename(&old_path, &backup).unwrap();
    assert!(inspect().is_err());
    assert!(!old_path.exists());
    fs::rename(&backup, &old_path).unwrap();
    let noncanonical = format!("{raw}\n");
    fs::write(&sidecar, &noncanonical).unwrap();
    assert!(inspect().is_err());
    assert_eq!(fs::read(&sidecar).unwrap(), noncanonical.as_bytes());
    fs::write(&sidecar, &raw).unwrap();
    fs::set_permissions(&sidecar, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(inspect().is_err());
    assert_eq!(fs::metadata(&sidecar).unwrap().mode() & 0o077, 0o044);
    fs::set_permissions(&sidecar, fs::Permissions::from_mode(0o600)).unwrap();
    let alias = owner.metadata_path().join("historical-owner-alias");
    fs::hard_link(&sidecar, &alias).unwrap();
    assert!(inspect().is_err());
    assert_eq!(fs::metadata(&sidecar).unwrap().nlink(), 2);
    fs::remove_file(alias).unwrap();
    let mut foreign = revoked.clone();
    foreign.extend_from_slice(b"foreign suffix");
    fs::write(&owner_journal, &foreign).unwrap();
    assert!(inspect().is_err());
    assert_eq!(fs::read(&owner_journal).unwrap(), foreign);
    fs::write(&owner_journal, &revoked).unwrap();
    let backup = owner_journal.with_extension("historical-original");
    fs::rename(&owner_journal, &backup).unwrap();
    fs::write(&owner_journal, &revoked).unwrap();
    assert!(inspect().is_err());
    assert_eq!(fs::read(&owner_journal).unwrap(), revoked);
    fs::remove_file(&owner_journal).unwrap();
    fs::rename(&backup, &owner_journal).unwrap();
    let mut incomplete = pending.before.clone();
    incomplete.extend_from_slice(&pending.frame);
    incomplete.extend_from_slice(&pending.checkpoint[..pending.checkpoint.len() - 1]);
    fs::write(&pending.journal, &incomplete).unwrap();
    assert!(inspect().is_err());
    assert_eq!(fs::read(&pending.journal).unwrap(), incomplete);
    fs::write(&pending.journal, &destination_before).unwrap();
    let editor = pending
        .destination
        .project()
        .root()
        .join("editor-historical-work");
    fs::write(&editor, b"preserve editor bytes").unwrap();
    assert!(inspect().is_err());
    assert_eq!(fs::read(&editor).unwrap(), b"preserve editor bytes");
    fs::remove_file(editor).unwrap();
    preserve(&revoked);
    assert!(inspect().is_ok());
}
