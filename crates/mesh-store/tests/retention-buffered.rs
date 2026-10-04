//! Conservative retention must cover durable history even before it can advance actor state.
use mesh_store::{
    CollectionPlan, EntityUuid, Index, OperationRecord, Reachability, RecordDigest, RetainedRoot,
    RetainedRoots, RetentionError, RetentionPolicy, StoredRecord,
};

fn id(value: u8) -> RecordDigest {
    RecordDigest::from_bytes([value; 32])
}
fn operation(sequence: u8, parents: &[u8]) -> StoredRecord {
    StoredRecord::Operation(OperationRecord {
        id: id(sequence),
        actor: id(100),
        actor_sequence: u64::from(sequence),
        hlc_millis: 0,
        hlc_counter: 0,
        policy_epoch: 1,
        session: EntityUuid::from_bytes([0; 16]),
        payload_digest: id(sequence + 10),
        parents: parents.iter().map(|value| id(*value)).collect(),
    })
}
fn assert_only_orphan_collectable(index: &Index, payloads: &[RecordDigest]) {
    let roots = RetainedRoots::conservative(index, RetentionPolicy::default());
    let reach = Reachability::compute(index, &roots).expect("known buffered actors resolve");
    for payload in payloads {
        assert!(
            reach.retains(payload),
            "recorded payload {payload} was omitted"
        );
    }
    let plan = CollectionPlan::compute(
        index,
        &roots,
        &reach,
        payloads.iter().copied().chain([id(250)]),
    )
    .unwrap();
    assert_eq!(
        plan.doomed_digests(),
        vec![[250; 32]],
        "only the unreferenced orphan may go"
    );
    assert_eq!(plan.kept().len(), payloads.len());
}

#[test]
fn conservative_retention_keeps_buffered_payload_before_and_after_missing_parent_arrives() {
    let mut index = Index::new();
    index.apply(operation(1, &[])).unwrap();
    index.apply(operation(3, &[2])).unwrap();
    assert_eq!(index.actor_head(&id(100)).unwrap().id, id(1));
    assert_eq!(index.causally_ready_operations(), vec![id(1)]);
    assert_only_orphan_collectable(&index, &[id(11), id(13)]);
    index.apply(operation(2, &[1])).unwrap();
    assert_eq!(index.actor_head(&id(100)).unwrap().id, id(3));
    assert_only_orphan_collectable(&index, &[id(11), id(12), id(13)]);
}

#[test]
fn a_known_actor_with_only_buffered_work_has_resolvable_retention_windows() {
    let mut index = Index::new();
    index.apply(operation(3, &[2])).unwrap();
    assert!(index.actor_head(&id(100)).is_none());
    assert_only_orphan_collectable(&index, &[id(13)]);
    let roots =
        RetainedRoots::new(RetentionPolicy::default()).with(RetainedRoot::RetentionWindow {
            actor: id(100),
            from_sequence: 3,
        });
    let reach = Reachability::compute(&index, &roots).unwrap();
    assert!(reach.retains(&id(13)));
    assert_eq!(
        reach.dangling_parents().copied().collect::<Vec<_>>(),
        vec![id(2)]
    );
    let unknown =
        RetainedRoots::new(RetentionPolicy::default()).with(RetainedRoot::RetentionWindow {
            actor: id(200),
            from_sequence: 0,
        });
    assert!(matches!(
        Reachability::compute(&index, &unknown),
        Err(RetentionError::UnknownActor { .. })
    ));
    // An explicit head still means a causally ready head; buffered work does not become ready.
    let head =
        RetainedRoots::new(RetentionPolicy::default()).with(RetainedRoot::ActorHead(id(100)));
    assert!(matches!(
        Reachability::compute(&index, &head),
        Err(RetentionError::UnknownActor { .. })
    ));
}

#[test]
fn disconnected_recorded_history_survives_conservative_but_not_explicitly_narrowed_policy() {
    let mut index = Index::new();
    index.apply(operation(1, &[])).unwrap();
    index.apply(operation(3, &[])).unwrap();
    assert_eq!(index.actor_head(&id(100)).unwrap().id, id(3));
    assert_only_orphan_collectable(&index, &[id(11), id(13)]);
    let window = RetainedRoot::RetentionWindow {
        actor: id(100),
        from_sequence: 0,
    };
    let narrowed = RetainedRoots::conservative(&index, RetentionPolicy::default()).without(&window);
    let reach = Reachability::compute(&index, &narrowed).unwrap();
    assert!(
        !reach.retains(&id(11)),
        "removing the history window leaves only the head closure"
    );
    assert!(reach.retains(&id(13)));
}
