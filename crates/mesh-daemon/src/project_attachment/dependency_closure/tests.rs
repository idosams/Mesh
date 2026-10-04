use super::*;
fn d(n: u8) -> RecordDigest {
    RecordDigest::from_bytes([n; 32])
}
fn i(n: u8) -> Input {
    (d(1), d(2), d(n))
}
fn node(parents: &[u8]) -> Node {
    Node {
        parents: parents.iter().map(|n| i(*n)).collect(),
        manifests: BTreeSet::new(),
        chunks: BTreeSet::new(),
        consumption: None,
    }
}
#[test]
fn diamonds_deduplicate_and_every_bound_refuses_without_truncation() {
    let map = BTreeMap::from([
        (i(3), node(&[])),
        (i(4), node(&[3])),
        (i(5), node(&[3])),
        (i(6), node(&[4, 5])),
    ]);
    let load = |id| map.get(&id).cloned().ok_or_else(|| invalid("missing"));
    let graph = walk(i(6), load, 4, 4, 3, 4096).unwrap();
    assert_eq!(graph.operation_count(), 4);
    assert!(walk(i(6), load, 3, 4, 3, 4096).is_err());
    assert!(walk(i(6), load, 4, 3, 3, 4096).is_err());
    assert!(walk(i(6), load, 4, 4, 2, 4096).is_err());
    assert!(walk(i(6), load, 4, 4, 3, graph.to_json().encode().len() - 1).is_err());
    assert_eq!(
        graph.digest(),
        walk(i(6), load, 4, 4, 3, 4096).unwrap().digest()
    );
}
#[test]
fn missing_parents_cycles_and_incomplete_consumption_lists_refuse() {
    let mut map = BTreeMap::from([(i(3), node(&[4])), (i(4), node(&[3]))]);
    assert!(walk(
        i(3),
        |id| map.get(&id).cloned().ok_or_else(|| invalid("missing")),
        8,
        8,
        8,
        4096
    )
    .is_err());
    map.remove(&i(4));
    assert!(walk(
        i(3),
        |id| map.get(&id).cloned().ok_or_else(|| invalid("missing")),
        8,
        8,
        8,
        4096
    )
    .is_err());
    map = BTreeMap::from([
        (i(3), node(&[])),
        (i(4), node(&[3])),
        (
            i(5),
            Node {
                parents: BTreeSet::from([i(4)]),
                manifests: BTreeSet::new(),
                chunks: BTreeSet::new(),
                consumption: Some(NativeConsumptionFact {
                    record: d(8),
                    grant: d(9),
                    source: i(4),
                    start: i(5),
                    inputs: vec![i(4)],
                    bindings: None,
                }),
            },
        ),
    ]);
    assert!(walk(
        i(5),
        |id| map.get(&id).cloned().ok_or_else(|| invalid("missing")),
        8,
        8,
        8,
        4096
    )
    .is_err());
    map.get_mut(&i(5))
        .unwrap()
        .consumption
        .as_mut()
        .unwrap()
        .inputs = vec![i(3), i(4)];
    assert_eq!(
        walk(
            i(5),
            |id| map.get(&id).cloned().ok_or_else(|| invalid("missing")),
            8,
            8,
            8,
            4096
        )
        .unwrap()
        .operation_count(),
        3
    );
}

#[test]
fn native_saved_graph_survives_reopen_and_ignores_later_editor_bytes() {
    use crate::project_attachment::ObservationLimits;
    use ed25519_dalek::{Signer as _, SigningKey};
    use std::fs;
    let root = std::env::temp_dir().join(format!("mesh-native-graph-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    fs::create_dir(root.join("source")).unwrap();
    fs::create_dir(root.join("metadata")).unwrap();
    fs::write(root.join("source/note"), b"existing editor work").unwrap();
    let storage = AttachmentStorage::open(&root.join("metadata")).unwrap();
    let owner = storage.provision(&root.join("source")).unwrap();
    // Initialize an empty history before enrollment: no invented legacy provenance.
    let input = owner
        .project()
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    let (_, created) = owner
        .project()
        .history_configuration(&owner.store, Some(input.exclusion_digest()))
        .unwrap();
    let empty = crate::workspace::OpenWorkspace::open_attachment_store(
        owner.metadata_path(),
        owner.store.clone(),
        created,
    )
    .unwrap();
    assert_eq!(empty.operations(), 0);
    drop(empty);
    owner.enroll_dependency_history().unwrap();
    let key = SigningKey::from_bytes(&[81; 32]);
    let capture = |n| {
        let input = owner
            .project()
            .capture_inputs(ObservationLimits::default())
            .unwrap();
        owner
            .prepare_dependency_capture(
                &input,
                mesh_types::PublicKey::from_bytes(key.verifying_key().to_bytes()),
                d(n),
                |body| {
                    Ok::<_, &'static str>(mesh_types::Signature::from_bytes(
                        key.sign(body.as_bytes()).to_bytes(),
                    ))
                },
            )
            .unwrap()
            .commit()
            .unwrap()
    };
    let first = capture(1);
    let first_journal = fs::read(owner.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap();
    fs::write(root.join("source/note"), b"second private save").unwrap();
    let second = capture(2);
    let graph = storage
        .inspect_dependency_graph(&owner, &owner, second, &[])
        .unwrap();
    assert_eq!(graph.operation_count(), 2);
    // A fully journaled capture awaiting acknowledgement retains its pending evidence atomically.
    let second_receipt = owner
        .metadata_path()
        .join(format!("dependency-capture-{}.json", d(2).to_hex()));
    let pending_capture_path = owner.metadata_path().join("dependency-capture.pending");
    fs::rename(&second_receipt, &pending_capture_path).unwrap();
    let awaiting_capture = storage
        .inspect_dependency_graph(&owner, &owner, second, &[])
        .unwrap();
    assert_eq!(awaiting_capture.digest(), graph.digest());
    assert!(awaiting_capture
        .retained
        .values()
        .next()
        .unwrap()
        .sidecars
        .contains_key("dependency-capture.pending"));
    let complete_journal_path = owner.metadata_path().join(crate::RECORD_FILE_NAME);
    let complete_journal = fs::read(&complete_journal_path).unwrap();
    let pending_bytes = fs::read(&pending_capture_path).unwrap();
    fs::write(
        &complete_journal_path,
        &complete_journal[..complete_journal.len() - 1],
    )
    .unwrap();
    assert!(
        storage
            .inspect_dependency_graph(&owner, &owner, second, &[])
            .is_err(),
        "a torn journal cannot produce a complete graph"
    );
    let recovery = owner.inspect_dependency_capture_retention(d(2)).unwrap();
    assert!(recovery.pending());
    assert_eq!(recovery.operation(), second.operation());
    assert_eq!(
        fs::read(&complete_journal_path).unwrap(),
        complete_journal[..complete_journal.len() - 1]
    );
    assert_eq!(fs::read(&pending_capture_path).unwrap(), pending_bytes);
    fs::write(&complete_journal_path, &first_journal).unwrap();
    assert!(
        storage
            .inspect_dependency_graph(&owner, &owner, first, &[])
            .is_err(),
        "even a complete older root must refuse an unappended pending capture"
    );
    assert!(owner
        .inspect_dependency_capture_retention(d(2))
        .unwrap()
        .pending());
    assert_eq!(fs::read(&complete_journal_path).unwrap(), first_journal);
    assert_eq!(fs::read(&pending_capture_path).unwrap(), pending_bytes);
    fs::write(&complete_journal_path, complete_journal).unwrap();
    fs::rename(&pending_capture_path, &second_receipt).unwrap();
    assert_eq!(
        graph,
        storage
            .inspect_dependency_graph(&owner, &owner, second, &[])
            .unwrap()
    );
    let first_graph = storage
        .inspect_dependency_graph(&owner, &owner, first, &[])
        .unwrap();
    assert_eq!(first_graph.operation_count(), 1);
    assert_ne!(graph.digest(), first_graph.digest());
    let first_receipt_name = format!("dependency-capture-{}.json", d(1).to_hex());
    let first_receipt_bytes = fs::read(owner.metadata_path().join(&first_receipt_name)).unwrap();
    let receipt = Json::parse(std::str::from_utf8(&first_receipt_bytes).unwrap()).unwrap();
    let first_frames =
        RecordDigest::parse_hex(receipt.get("frames").unwrap().as_text().unwrap()).unwrap();
    let facts = graph.retained.values().next().unwrap();
    assert_eq!(facts.sidecars.len(), 2);
    assert_eq!(
        facts.sidecars[&first_receipt_name],
        hash(&first_receipt_bytes)
    );
    assert!(facts.payloads.contains(&first_frames));
    assert_eq!(
        first_graph.retained.values().next().unwrap().sidecars.len(),
        1
    );
    let receipt_cas = mesh_cas::Cas::<_, mesh_cas::Blake3>::with_filesystem(
        owner.metadata_path(),
        owner.store.filesystem(),
    )
    .unwrap();
    let frame_path = owner.metadata_path().join(
        receipt_cas
            .layout()
            .chunk_path(&mesh_cas::Digest32::from_bytes(*first_frames.as_bytes())),
    );
    let frame_bytes = fs::read(&frame_path).unwrap();
    fs::write(&frame_path, b"changed historical recovery frames").unwrap();
    assert!(storage
        .inspect_dependency_graph(&owner, &owner, second, &[])
        .is_err());
    assert_eq!(
        fs::read(&frame_path).unwrap(),
        b"changed historical recovery frames"
    );
    fs::write(&frame_path, frame_bytes).unwrap();
    let receipt_path = owner.metadata_path().join(&first_receipt_name);
    fs::write(&receipt_path, b"unknown receipt state").unwrap();
    assert!(storage
        .inspect_dependency_graph(&owner, &owner, second, &[])
        .is_err());
    assert_eq!(fs::read(&receipt_path).unwrap(), b"unknown receipt state");
    fs::write(&receipt_path, &first_receipt_bytes).unwrap();
    // A second request may not claim the same completed capture operation.
    let mut duplicate = receipt.clone();
    let Json::Object(fields) = &mut duplicate else {
        panic!("receipt object missing")
    };
    fields
        .iter_mut()
        .find(|(name, _)| name == "request")
        .unwrap()
        .1 = Json::text(d(90).to_hex());
    let duplicate_path = owner
        .metadata_path()
        .join(format!("dependency-capture-{}.json", d(90).to_hex()));
    fs::write(&duplicate_path, duplicate.encode()).unwrap();
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(&duplicate_path, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(
        storage
            .inspect_dependency_graph(&owner, &owner, second, &[])
            .is_err(),
        "two requests cannot claim one completed native capture"
    );
    assert_eq!(
        fs::read_to_string(&duplicate_path).unwrap(),
        duplicate.encode()
    );
    fs::remove_file(duplicate_path).unwrap();

    assert_eq!(
        graph,
        storage
            .inspect_dependency_graph(&owner, &owner, second, &[])
            .unwrap()
    );

    fs::write(root.join("source/note"), b"unsaved later editor bytes").unwrap();
    assert_eq!(
        graph,
        storage
            .inspect_dependency_graph(&owner, &owner, second, &[])
            .unwrap()
    );
    let reopened_storage = AttachmentStorage::open(&root.join("metadata")).unwrap();
    // A fresh registration handle must reproduce the same immutable graph.
    let reopened = reopened_storage.provision(&root.join("source")).unwrap();
    assert_eq!(
        graph,
        reopened_storage
            .inspect_dependency_graph(&reopened, &reopened, second, &[])
            .unwrap()
    );
    // The earlier bytes are no longer in the latest snapshot, but remain dependencies.
    let historical = hash(b"existing editor work");
    assert!(graph.nodes.values().any(|n| n.chunks.contains(&historical)));
    let cas = mesh_cas::Cas::<_, mesh_cas::Blake3>::with_filesystem(
        owner.metadata_path(),
        owner.store.filesystem(),
    )
    .unwrap();
    let path = owner.metadata_path().join(
        cas.layout()
            .chunk_path(&mesh_cas::Digest32::from_bytes(*historical.as_bytes())),
    );
    let original = fs::read(&path).unwrap();
    let corrupt = vec![b'X'; original.len()];
    fs::write(&path, &corrupt).unwrap();
    assert!(storage
        .inspect_dependency_graph(&owner, &owner, second, &[])
        .is_err());
    assert_eq!(
        fs::read(&path).unwrap(),
        corrupt,
        "inspection preserves corrupt evidence"
    );
    fs::write(&path, &original).unwrap();
    assert_eq!(
        graph,
        storage
            .inspect_dependency_graph(&owner, &owner, second, &[])
            .unwrap()
    );
    fs::remove_file(&path).unwrap();
    assert!(storage
        .inspect_dependency_graph(&owner, &owner, second, &[])
        .is_err());
    fs::write(&path, &original).unwrap();
    let before_rejection = fs::read(owner.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap();
    let rejection = owner
        .decide_saved_input(
            first,
            crate::project_attachment::SavedInputDecision::Rejected,
            None,
            d(80),
        )
        .unwrap();
    let after = storage
        .inspect_dependency_graph(&owner, &owner, second, &[])
        .unwrap();
    assert_eq!(
        after.digest(),
        graph.digest(),
        "eligibility cannot rewrite immutable graph identity"
    );
    assert_eq!(after.nodes, graph.nodes);
    // Simulate a committed control frame whose final acknowledgement/sidecar removal was lost.
    use std::os::unix::fs::MetadataExt as _;
    let journal_metadata =
        fs::metadata(owner.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap();
    let pending_control = crate::project_attachment::dependency_decision::transaction_intent(
        mesh_store::DependencyKind::Eligibility,
        d(80),
        (journal_metadata.dev(), journal_metadata.ino()),
        &before_rejection,
        rejection.record(),
    );
    let pending_control_path = owner
        .metadata_path()
        .join(crate::project_attachment::dependency_decision::PENDING);
    fs::write(&pending_control_path, &pending_control).unwrap();
    fs::set_permissions(&pending_control_path, fs::Permissions::from_mode(0o600)).unwrap();
    let awaiting_ack = storage
        .inspect_dependency_graph(&owner, &owner, second, &[])
        .unwrap();
    assert_eq!(awaiting_ack.digest(), after.digest());
    assert_eq!(
        awaiting_ack.retained.values().next().unwrap().sidecars
            [crate::project_attachment::dependency_decision::PENDING],
        hash(pending_control.as_bytes())
    );
    assert_eq!(
        fs::read_to_string(&pending_control_path).unwrap(),
        pending_control
    );
    fs::write(&pending_control_path, b"unknown control recovery state").unwrap();
    assert!(storage
        .inspect_dependency_graph(&owner, &owner, second, &[])
        .is_err());
    assert_eq!(
        fs::read(&pending_control_path).unwrap(),
        b"unknown control recovery state"
    );
    fs::remove_file(pending_control_path).unwrap();
    assert_eq!(
        after,
        storage
            .inspect_dependency_graph(&owner, &owner, second, &[])
            .unwrap()
    );

    assert_eq!(after.retained.len(), 1);
    for (work, before) in &graph.retained {
        let retained = &after.retained[work];
        assert_eq!(retained.physical, before.physical);
        assert_eq!(retained.correlation, before.correlation);
        assert!(before.payloads.is_subset(&retained.payloads));
        assert!(retained.payloads.contains(&rejection.record()));
        assert!(retained.payloads.contains(&historical));
        assert!(retained.payloads.contains(&first.operation()));
        assert_eq!(retained.manifests, before.manifests);
    }
    let reopened_again = AttachmentStorage::open(&root.join("metadata")).unwrap();
    let handle = reopened_again.provision(&root.join("source")).unwrap();
    assert_eq!(
        after,
        reopened_again
            .inspect_dependency_graph(&handle, &handle, second, &[])
            .unwrap()
    );
    let child = storage
        .reserve_dependency_lane(&owner, &owner, second, d(81))
        .unwrap();
    fs::write(child.project().root().join("note"), b"second private save").unwrap();
    let child_input = child
        .project()
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    let child_save = child
        .prepare_dependency_capture(
            &child_input,
            mesh_types::PublicKey::from_bytes(key.verifying_key().to_bytes()),
            d(82),
            |body| {
                Ok::<_, &'static str>(mesh_types::Signature::from_bytes(
                    key.sign(body.as_bytes()).to_bytes(),
                ))
            },
        )
        .unwrap()
        .commit()
        .unwrap();
    let independent = storage
        .inspect_dependency_graph(&owner, &child, child_save, &[])
        .unwrap();
    assert_eq!(
        independent.operation_count(),
        1,
        "reservation identity alone is not consumption"
    );
    assert_eq!(
        independent.retained.len(),
        2,
        "owner authority is retained in its own store"
    );
    let grant = storage
        .grant_saved_input(
            &owner,
            crate::project_attachment::NativeInputGrantRequest {
                source: &owner,
                version: second,
                destination: &child,
                allowed: true,
                expected_previous: None,
                request: d(83),
            },
        )
        .unwrap();
    // Replay fixture only: the production consumption transaction is still unimplemented.
    // Stage a valid owner receipt over real bound native work and signed captures.
    let source_binding = storage.dependency_work_binding(&owner, &owner).unwrap();
    let child_binding = storage.dependency_work_binding(&owner, &child).unwrap();
    let start = (
        child_binding.work(),
        child_binding.installation(),
        child_save.operation(),
    );
    let source = (
        source_binding.work(),
        source_binding.installation(),
        second.operation(),
    );
    let journal_path = owner.metadata_path().join(crate::RECORD_FILE_NAME);
    let before_receipt = fs::read(&journal_path).unwrap();
    append_consumption_fixture(&owner, grant.record(), start, vec![source], d(84));
    assert!(
        storage
            .inspect_dependency_graph(&owner, &child, child_save, &[])
            .is_err(),
        "receipt cannot omit an inherited native parent"
    );
    // Restore only this test fixture's exact pre-injection bytes, then replay the complete receipt.
    fs::write(&journal_path, &before_receipt).unwrap();
    let receipt = append_consumption_fixture(
        &owner,
        grant.record(),
        start,
        graph.nodes.keys().copied().collect(),
        d(84),
    );
    let consumed = storage
        .inspect_dependency_graph(&owner, &child, child_save, &[])
        .unwrap();
    assert_eq!(consumed.operation_count(), 3);
    assert!(consumed.nodes[&start].parents.contains(&source));
    let owner_key = (source_binding.work(), source_binding.installation());
    let child_key = (child_binding.work(), child_binding.installation());
    assert!(consumed.retained[&owner_key]
        .payloads
        .contains(&grant.record()));
    assert!(consumed.retained[&owner_key].payloads.contains(&receipt));
    assert!(
        !consumed.retained[&child_key].payloads.contains(&receipt),
        "owner policy must not be mislabeled as a child CAS object"
    );
    let grandchild = storage
        .reserve_dependency_lane(&owner, &child, child_save, d(85))
        .unwrap();
    fs::write(
        grandchild.project().root().join("note"),
        b"second private save",
    )
    .unwrap();
    let grandchild_input = grandchild
        .project()
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    let grandchild_save = grandchild
        .prepare_dependency_capture(
            &grandchild_input,
            mesh_types::PublicKey::from_bytes(key.verifying_key().to_bytes()),
            d(86),
            |body| {
                Ok::<_, &'static str>(mesh_types::Signature::from_bytes(
                    key.sign(body.as_bytes()).to_bytes(),
                ))
            },
        )
        .unwrap()
        .commit()
        .unwrap();
    let second_grant = storage
        .grant_saved_input(
            &owner,
            crate::project_attachment::NativeInputGrantRequest {
                source: &child,
                version: child_save,
                destination: &grandchild,
                allowed: true,
                expected_previous: None,
                request: d(87),
            },
        )
        .unwrap();
    let grandchild_binding = storage
        .dependency_work_binding(&owner, &grandchild)
        .unwrap();
    let second_start = (
        grandchild_binding.work(),
        grandchild_binding.installation(),
        grandchild_save.operation(),
    );
    append_consumption_fixture(
        &owner,
        second_grant.record(),
        second_start,
        consumed.nodes.keys().copied().collect(),
        d(88),
    );
    assert!(
        storage
            .inspect_dependency_graph(&owner, &grandchild, grandchild_save, &[])
            .is_err(),
        "missing intermediate native work must refuse, not truncate ancestry"
    );
    let transitive = storage
        .inspect_dependency_graph(&owner, &grandchild, grandchild_save, &[&child])
        .unwrap();
    assert_eq!(transitive.operation_count(), 4);
    assert_eq!(transitive.retained.len(), 3);
    assert_eq!(
        transitive,
        storage
            .inspect_dependency_graph(
                &owner,
                &grandchild,
                grandchild_save,
                &[&grandchild, &owner, &child, &child]
            )
            .unwrap(),
        "handle order and duplicates cannot change facts"
    );
    // Attachment readers reconstruct an in-memory index on every open. On-disk cache bytes
    // must neither override that reconstruction nor be destructively repaired by inspection.
    let poison = b"deliberately invalid disposable index";
    for work in [&owner, &child, &grandchild] {
        let path = work.metadata_path().join("metadata.sqlite");
        assert!(
            !path.exists(),
            "native attachment reads must not persist an index"
        );
        fs::write(path, poison).unwrap();
    }
    assert_eq!(
        transitive,
        storage
            .inspect_dependency_graph(&owner, &grandchild, grandchild_save, &[&child])
            .unwrap(),
        "cached index bytes cannot override journal reconstruction"
    );
    for work in [&owner, &child, &grandchild] {
        let path = work.metadata_path().join("metadata.sqlite");
        assert_eq!(
            fs::read(&path).unwrap(),
            poison,
            "inspection must preserve foreign cache evidence"
        );
        fs::remove_file(path).unwrap();
    }
    assert_eq!(
        transitive,
        storage
            .inspect_dependency_graph(&owner, &grandchild, grandchild_save, &[&child])
            .unwrap(),
        "in-memory replay without cached indexes preserves retained closure"
    );
    let original_store = root.join("original-child-store");
    let child_store_path = child.metadata_path().to_path_buf();
    let journal_before = fs::read(child_store_path.join(crate::RECORD_FILE_NAME)).unwrap();
    fs::rename(&child_store_path, &original_store).unwrap();
    copy_fixture_store(&original_store, &child_store_path, &mut 1024);
    assert_eq!(
        fs::read(child_store_path.join(crate::RECORD_FILE_NAME)).unwrap(),
        journal_before
    );
    let substituted = storage.reopen(child.id()).unwrap();
    assert_ne!(
        substituted.store.identity().unwrap(),
        child.store.identity().unwrap()
    );
    assert!(
        storage
            .inspect_dependency_graph(&owner, &grandchild, grandchild_save, &[&substituted])
            .is_err(),
        "byte-identical replacement store cannot inherit the old installation"
    );
    assert!(
        storage
            .inspect_dependency_graph(
                &owner,
                &grandchild,
                grandchild_save,
                &[&child, &substituted]
            )
            .is_err(),
        "same registration with conflicting physical bindings must refuse"
    );
    assert_eq!(
        fs::read(child_store_path.join(crate::RECORD_FILE_NAME)).unwrap(),
        journal_before
    );
    assert_eq!(
        fs::read(original_store.join(crate::RECORD_FILE_NAME)).unwrap(),
        journal_before
    );
    fs::rename(&child_store_path, root.join("substituted-child-store")).unwrap();
    fs::rename(&original_store, &child_store_path).unwrap();
    assert_eq!(
        transitive,
        storage
            .inspect_dependency_graph(&owner, &grandchild, grandchild_save, &[&child])
            .unwrap()
    );
    // Signed native DAG replay, independent of the capture-only linear-history selector.
    let branch_a = append_signed_operation_fixture(&owner, &[second.operation()], 101);
    let branch_b = append_signed_operation_fixture(&owner, &[second.operation()], 102);
    let merged = append_signed_operation_fixture(&owner, &[branch_a, branch_b], 103);
    let dag = storage
        .inspect_dependency_operation_graph(&owner, &owner, merged, &[])
        .unwrap();
    assert_eq!(dag.operation_count(), 5);
    let qualified_merge = (source_binding.work(), source_binding.installation(), merged);
    assert_eq!(dag.nodes[&qualified_merge].parents.len(), 2);
    assert!(dag.nodes.values().any(|n| n.chunks.contains(&historical)));
    let missing = append_signed_operation_fixture(&owner, &[d(99)], 104);
    let journal_before_refusal =
        fs::read(owner.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap();
    assert!(storage
        .inspect_dependency_operation_graph(&owner, &owner, missing, &[])
        .is_err());
    assert_eq!(
        fs::read(owner.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap(),
        journal_before_refusal
    );
    assert_eq!(
        fs::read(root.join("source/note")).unwrap(),
        b"unsaved later editor bytes"
    );
}

#[test]
fn oversized_content_references_refuse_before_loading_more_nodes() {
    let mut root = node(&[4]);
    root.chunks = (0..32).map(d).collect();
    assert!(walk(
        i(3),
        |id| {
            assert_eq!(
                id,
                i(3),
                "reference budget must refuse before loading parent"
            );
            Ok(root.clone())
        },
        8,
        8,
        8,
        1024
    )
    .is_err());
}

fn append_consumption_fixture(
    owner: &ProvisionedAttachment,
    grant: RecordDigest,
    start: Input,
    inputs: Vec<Input>,
    request: RecordDigest,
) -> RecordDigest {
    use std::io::Write as _;
    let _guard = crate::workspace_custody::lock_workspace_initialization(&owner.store).unwrap();
    let (_, proof) = owner
        .project()
        .read_configuration(owner.metadata_path(), &owner.store)
        .unwrap();
    let proof = proof.unwrap();
    let (revision, previous) = proof.policy().native_head().unwrap();
    let input = |i: Input| {
        Json::Array(vec![
            Json::Array(vec![Json::text(i.0.to_hex()), Json::text(i.1.to_hex())]),
            Json::text(i.2.to_hex()),
        ])
    };
    let kind = mesh_store::DependencyKind::Consumption;
    let bytes = Json::object([
        ("schema", Json::text("mesh.dependency-policy/v1")),
        ("authority", Json::text(proof.binding().authority.to_hex())),
        ("revision", Json::Number(revision + 1)),
        ("previous", Json::text(previous.to_hex())),
        ("kind", Json::Number(u64::from(kind.code()))),
        (
            "body",
            Json::object([
                ("request", Json::text(request.to_hex())),
                ("grant", Json::text(grant.to_hex())),
                ("start", input(start)),
                (
                    "inputs",
                    Json::Array(inputs.into_iter().map(input).collect()),
                ),
            ]),
        ),
    ])
    .encode()
    .into_bytes();
    let record = mesh_store::DependencyRecord {
        authority: proof.binding().authority,
        revision: revision + 1,
        previous,
        payload: hash(&bytes),
        kind,
    };
    proof.policy().clone().apply(record, &bytes).unwrap();
    let cas = mesh_cas::Cas::<_, mesh_cas::Blake3>::with_filesystem(
        owner.metadata_path(),
        owner.store.filesystem(),
    )
    .unwrap();
    cas.promote(bytes).unwrap();
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(owner.metadata_path().join(crate::RECORD_FILE_NAME))
        .unwrap();
    file.write_all(&mesh_store::frame_record(
        &mesh_store::StoredRecord::Dependency(record),
    ))
    .unwrap();
    file.sync_all().unwrap();
    record.payload
}

fn copy_fixture_store(from: &std::path::Path, to: &std::path::Path, budget: &mut usize) {
    use std::fs;
    *budget = budget.checked_sub(1).expect("fixture copy bound exceeded");
    fs::create_dir(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let kind = entry.file_type().unwrap();
        if kind.is_dir() {
            copy_fixture_store(&entry.path(), &to.join(entry.file_name()), budget);
        } else {
            assert!(kind.is_file(), "unexpected fixture file type");
            *budget = budget.checked_sub(1).expect("fixture copy bound exceeded");
            fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
    }
    fs::set_permissions(to, fs::metadata(from).unwrap().permissions()).unwrap();
}

#[test]
fn pre_enrollment_input_is_not_silently_declared_dependency_free() {
    use crate::project_attachment::ObservationLimits;
    use ed25519_dalek::{Signer as _, SigningKey};
    use std::fs;
    let root = std::env::temp_dir().join(format!("mesh-legacy-graph-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    fs::create_dir(root.join("source")).unwrap();
    fs::create_dir(root.join("metadata")).unwrap();
    fs::write(root.join("source/note"), b"legacy private work").unwrap();
    let storage = AttachmentStorage::open(&root.join("metadata")).unwrap();
    let owner = storage.provision(&root.join("source")).unwrap();
    let input = owner
        .project()
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    let key = SigningKey::from_bytes(&[82; 32]);
    let saved = owner
        .project()
        .save_capture(
            owner.metadata_path(),
            &input,
            mesh_types::PublicKey::from_bytes(key.verifying_key().to_bytes()),
            |body| {
                Ok::<_, &'static str>(mesh_types::Signature::from_bytes(
                    key.sign(body.as_bytes()).to_bytes(),
                ))
            },
        )
        .unwrap();
    // The old allocator copies source bytes before the child has any operation history.
    let copied = storage
        .open_version_lane(
            &owner,
            &saved.operation().to_hex(),
            "83838383838383838383838383838383",
            ObservationLimits::default(),
        )
        .unwrap();
    let copied_input = copied
        .project()
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    let (_, created) = copied
        .project()
        .history_configuration(&copied.store, Some(copied_input.exclusion_digest()))
        .unwrap();
    let empty = crate::workspace::OpenWorkspace::open_attachment_store(
        copied.metadata_path(),
        copied.store.clone(),
        created,
    )
    .unwrap();
    assert_eq!(empty.operations(), 0);
    drop(empty);
    copied.enroll_dependency_history().unwrap();
    let copied_save = copied
        .prepare_dependency_capture(
            &copied_input,
            mesh_types::PublicKey::from_bytes(key.verifying_key().to_bytes()),
            d(83),
            |body| {
                Ok::<_, &'static str>(mesh_types::Signature::from_bytes(
                    key.sign(body.as_bytes()).to_bytes(),
                ))
            },
        )
        .unwrap()
        .commit()
        .unwrap();
    owner.enroll_dependency_history().unwrap();
    let copied_journal = fs::read(copied.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap();
    let refused = storage
        .inspect_dependency_graph(&owner, &copied, copied_save, &[])
        .unwrap_err();
    assert!(refused.to_string().contains("legacy copied ancestry"));
    assert_eq!(
        fs::read(copied.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap(),
        copied_journal
    );
    assert_eq!(
        fs::read(copied.project().root().join("note")).unwrap(),
        b"legacy private work"
    );
    let journal = fs::read(owner.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap();
    let refused = storage
        .inspect_dependency_graph(&owner, &owner, saved, &[])
        .unwrap_err();
    assert!(refused.to_string().contains("legacy input ancestry"));
    assert_eq!(
        fs::read(owner.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap(),
        journal
    );
    assert_eq!(
        fs::read(root.join("source/note")).unwrap(),
        b"legacy private work"
    );
    assert_eq!(owner.saved_versions().unwrap(), vec![saved]);
}

fn append_signed_operation_fixture(
    owner: &ProvisionedAttachment,
    parents: &[RecordDigest],
    actor: u8,
) -> RecordDigest {
    use crate::checkpoint_storage::{
        operation_checkpoint_signing_body, prepare_authenticated_checkpoint,
        AuthenticatedOperationCheckpointRequest,
    };
    use ed25519_dalek::{Signer as _, SigningKey};
    use mesh_operations::*;
    use mesh_types::{PublicKey, Signature};
    use std::io::Write as _;
    struct Head;
    impl HeadDerivation for Head {
        fn resulting_head(&self, value: &TransitionCommitment) -> HeadId {
            HeadId::from_bytes(*hash(&value.canonical_bytes()).as_bytes())
        }
    }
    let _guard = crate::workspace_custody::lock_workspace_initialization(&owner.store).unwrap();
    let (configuration, proof) = owner
        .project()
        .read_configuration(owner.metadata_path(), &owner.store)
        .unwrap();
    let proof = proof.unwrap();
    let history = crate::workspace::OpenWorkspace::open_attachment_read_history(
        owner.metadata_path(),
        owner.store.clone(),
        &crate::TrustedReviewers::default(),
        Some(&proof),
    )
    .unwrap();
    let key = SigningKey::from_bytes(&[actor; 32]);
    let public = PublicKey::from_bytes(key.verifying_key().to_bytes());
    let causal = CausalParents::after(
        ChangeSetId::from_bytes(*parents[0].as_bytes()),
        parents[1..]
            .iter()
            .map(|p| ChangeSetId::from_bytes(*p.as_bytes()))
            .collect(),
    );
    let request = |signature| {
        AuthenticatedOperationCheckpointRequest::new(
            WorkspaceId::from_bytes(crate::project_attachment::history::short_id(
                configuration.as_bytes(),
            )),
            ActorId::from_bytes(*public.as_bytes()),
            SessionId::from_bytes([actor; 16]),
            ActorSequence::FIRST,
            causal.clone(),
            HeadId::from_bytes([0; 32]),
            PolicyEpoch::new(1),
            Hlc::new(u64::from(actor), 0),
            vec![Operation::CreateDirectory {
                object_id: ObjectId::from_bytes([actor; 16]),
            }],
            public,
            signature,
        )
    };
    let body = operation_checkpoint_signing_body(&request(Signature::from_bytes([0; 64])), &Head);
    let payload = mesh_crypto::SigningPayload::new(
        crate::authenticated_changeset::CHANGESET_SIGNATURE_DOMAIN,
        &body,
    );
    let signature = Signature::from_bytes(key.sign(payload.as_bytes()).to_bytes());
    let prepared =
        prepare_authenticated_checkpoint(&history, request(signature), vec![], &Head).unwrap();
    let operation = prepared.changeset_id;
    let cas = mesh_cas::Cas::<_, mesh_cas::Blake3>::with_filesystem(
        owner.metadata_path(),
        owner.store.filesystem(),
    )
    .unwrap();
    for object in prepared.objects {
        cas.promote(object).unwrap();
    }
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(owner.metadata_path().join(crate::RECORD_FILE_NAME))
        .unwrap();
    for record in prepared.checkpoint.records() {
        file.write_all(&mesh_store::frame_record(&record)).unwrap();
    }
    file.sync_all().unwrap();
    operation
}
