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
    assert_eq!(
        fs::read(root.join("source/note")).unwrap(),
        b"unsaved later editor bytes"
    );
}
