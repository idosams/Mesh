//! Storage evidence only: envelopes and checksums do not authorize private access or publication.
mod common;
use common::{Sqlite3, TempDir};
use mesh_store::{
    frame_record, read_all_tables, rebuild, scan_journal, Checkpoint, DependencyKind,
    DependencyRecord, Index, Reachability, RecordDigest, RetainedRoots, RetentionError,
    RetentionPolicy, Store, StoredRecord,
};
fn digest(byte: u8) -> RecordDigest {
    RecordDigest::from_bytes([byte; 32])
}
fn history() -> Vec<DependencyRecord> {
    [
        DependencyKind::Enrollment,
        DependencyKind::Grant,
        DependencyKind::Consumption,
        DependencyKind::Eligibility,
        DependencyKind::ReviewSnapshot,
    ]
    .into_iter()
    .enumerate()
    .map(|(i, kind)| DependencyRecord {
        authority: digest(1),
        revision: (i + 1) as u64,
        previous: if i == 0 {
            digest(0)
        } else {
            digest(10 + i as u8)
        },
        payload: digest(11 + i as u8),
        kind,
    })
    .collect()
}
#[test]
fn dependency_envelope_has_a_fixed_required_tag_and_exact_canonical_body() {
    let record = history()[0];
    let frame = frame_record(&StoredRecord::Dependency(record));
    assert_eq!(&frame[..8], &[b'M', b'J', 1, 8, 0, 0, 0, 105]);
    let mut expected = vec![1; 32];
    expected.extend([0, 0, 0, 0, 0, 0, 0, 1]);
    expected.extend([0; 32]);
    expected.extend([11; 32]);
    expected.push(0);
    assert_eq!(&frame[24..129], expected);
    assert_eq!(frame.len(), 145);
    assert_eq!(
        scan_journal(&frame).unwrap().records(),
        &[StoredRecord::Dependency(record)]
    );
    println!(
        "dependency-enrollment-frame={}",
        frame.iter().map(|b| format!("{b:02x}")).collect::<String>()
    );
    for length in 0..frame.len() {
        let scan = scan_journal(&frame[..length]).unwrap();
        assert!(scan.records().is_empty());
        assert_eq!(scan.boundary().byte_offset, 0);
    }
    for (kind, code) in [
        (DependencyKind::Enrollment, 0),
        (DependencyKind::Grant, 1),
        (DependencyKind::Consumption, 2),
        (DependencyKind::Eligibility, 3),
        (DependencyKind::ReviewSnapshot, 4),
        (DependencyKind::ConsumptionStart, 5),
        (DependencyKind::ConsumptionComplete, 6),
    ] {
        assert_eq!(kind.code(), code);
        assert_eq!(DependencyKind::from_code(code), Some(kind));
    }
    assert_eq!(DependencyKind::from_code(7), None);
    assert_eq!(DependencyKind::from_code(255), None);
}
#[test]
fn ordered_replay_is_idempotent_without_rewinding_the_authority_head() {
    let records = history();
    let mut index = Index::new();
    for record in &records {
        index.apply(StoredRecord::Dependency(*record)).unwrap();
    }
    let before = index.clone();
    for record in &records {
        index.apply(StoredRecord::Dependency(*record)).unwrap();
    }
    assert_eq!(index, before);
    index
        .apply(StoredRecord::Dependency(DependencyRecord {
            revision: 6,
            previous: records[4].payload,
            payload: digest(16),
            kind: DependencyKind::Eligibility,
            ..records[4]
        }))
        .unwrap();
    assert_eq!(index.dependency_records().count(), 6);
    assert!(index.named_content().contains(&digest(16)));
}
#[test]
fn invalid_or_conflicting_envelopes_refuse_without_changing_any_index_state() {
    let records = history();
    let mut index = Index::new();
    index.apply(StoredRecord::Dependency(records[0])).unwrap();
    let before = index.clone();
    let invalid = [
        DependencyRecord {
            authority: digest(0),
            ..records[1]
        },
        DependencyRecord {
            payload: digest(0),
            ..records[1]
        },
        DependencyRecord {
            previous: digest(0),
            ..records[1]
        },
        DependencyRecord {
            revision: 3,
            ..records[1]
        },
        DependencyRecord {
            revision: 0,
            ..records[1]
        },
        DependencyRecord {
            revision: u64::MAX,
            ..records[1]
        },
        DependencyRecord {
            kind: DependencyKind::Enrollment,
            ..records[1]
        },
        DependencyRecord {
            kind: DependencyKind::Grant,
            ..records[0]
        },
        DependencyRecord {
            authority: digest(2),
            ..records[1]
        },
    ];
    for record in invalid {
        assert!(index.apply(StoredRecord::Dependency(record)).is_err());
        assert_eq!(
            index, before,
            "a refused envelope partially changed replay state"
        );
    }
    assert!(Index::new()
        .apply(StoredRecord::Dependency(records[1]))
        .is_err());
}
#[test]
fn policy_history_reconstructs_identically_through_sqlite_and_raw_journal() {
    let dir = TempDir::new("dependency-records");
    let path = dir.join("metadata.sqlite");
    let checkpoint = Checkpoint {
        dependencies: history(),
        ..Checkpoint::default()
    };
    let mut store = Store::open(Sqlite3::at(&path)).unwrap();
    let expected = store.commit(&checkpoint).unwrap();
    let bytes: Vec<u8> = checkpoint.records().iter().flat_map(frame_record).collect();
    let scan = scan_journal(&bytes).unwrap();
    let ledger = store.index().rows("schema_version").unwrap();
    let (replayed, report) = rebuild(scan.records().iter().cloned(), ledger).unwrap();
    assert_eq!(report.digest, expected);
    assert_eq!(&replayed, store.index());
    let mut db = Sqlite3::at(&path);
    for (name, rows) in read_all_tables(&mut db).unwrap() {
        assert_eq!(Some(rows), replayed.rows(name));
    }
    drop(store);
    let mut reopened = Store::open(Sqlite3::at(&path)).unwrap();
    assert_eq!(
        reopened
            .rebuild_from(scan.records().iter().cloned())
            .unwrap(),
        expected
    );
    assert_eq!(reopened.index().dependency_records().count(), 5);
}
#[test]
fn unvalidated_dependency_history_refuses_collection_instead_of_dropping_unknown_roots() {
    let (index, _) = rebuild(history().into_iter().map(StoredRecord::Dependency), vec![]).unwrap();
    let roots = RetainedRoots::conservative(&index, RetentionPolicy::default());
    assert_eq!(
        Reachability::compute(&index, &roots),
        Err(RetentionError::UnvalidatedDependencies)
    );
    for record in history() {
        assert!(index.named_content().contains(&record.payload));
    }
}

#[test]
fn consumed_start_and_completion_replay_rebuild_and_retain_as_required_records() {
    let dir = TempDir::new("consumption-required-records");
    let mut records = history();
    for (offset, kind) in [
        DependencyKind::ConsumptionStart,
        DependencyKind::ConsumptionComplete,
    ]
    .into_iter()
    .enumerate()
    {
        let previous = records.last().unwrap();
        records.push(DependencyRecord {
            authority: previous.authority,
            revision: previous.revision + 1,
            previous: previous.payload,
            payload: digest(40 + offset as u8),
            kind,
        });
    }
    let checkpoint = Checkpoint {
        dependencies: records.clone(),
        ..Checkpoint::default()
    };
    let path = dir.join("metadata.sqlite");
    let mut store = Store::open(Sqlite3::at(&path)).unwrap();
    let expected = store.commit(&checkpoint).unwrap();
    let bytes = checkpoint
        .records()
        .iter()
        .flat_map(frame_record)
        .collect::<Vec<_>>();
    let scan = scan_journal(&bytes).unwrap();
    assert_eq!(scan.records(), checkpoint.records());
    assert_eq!(
        store.rebuild_from(scan.records().iter().cloned()).unwrap(),
        expected
    );
    let expected_rows = store.index().rows("dependency_record").unwrap();
    assert_eq!(expected_rows.len(), 7);
    drop(store);
    let mut reopened = Store::open(Sqlite3::at(&path)).unwrap();
    let persisted = read_all_tables(&mut Sqlite3::at(&path)).unwrap();
    let (_, persisted_rows) = persisted
        .iter()
        .find(|(name, _)| *name == "dependency_record")
        .unwrap();
    assert_eq!(*persisted_rows, expected_rows);
    assert_eq!(
        reopened
            .rebuild_from(scan.records().iter().cloned())
            .unwrap(),
        expected
    );
    assert_eq!(
        reopened
            .index()
            .dependency_records()
            .copied()
            .collect::<Vec<_>>(),
        records
    );
    let retained = RetainedRoots::conservative(reopened.index(), RetentionPolicy::default());
    assert_eq!(
        Reachability::compute(reopened.index(), &retained),
        Err(RetentionError::UnvalidatedDependencies)
    );
    for record in &records[5..] {
        let frame = frame_record(&StoredRecord::Dependency(*record));
        assert_eq!(
            frame[3], 8,
            "required dependency envelope tag must not change"
        );
        for length in 0..frame.len() {
            let partial = scan_journal(&frame[..length]).unwrap();
            assert!(partial.records().is_empty());
            assert_eq!(partial.boundary().byte_offset, 0);
        }
    }
}
