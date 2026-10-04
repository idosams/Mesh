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
    fs::write(root.join("source/note"), b"second private save").unwrap();
    let second = capture(2);
    let graph = storage
        .inspect_dependency_graph(&owner, &owner, second, &[])
        .unwrap();
    assert_eq!(graph.operation_count(), 2);
    let first_graph = storage
        .inspect_dependency_graph(&owner, &owner, first, &[])
        .unwrap();
    assert_eq!(first_graph.operation_count(), 1);
    assert_ne!(graph.digest(), first_graph.digest());
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
