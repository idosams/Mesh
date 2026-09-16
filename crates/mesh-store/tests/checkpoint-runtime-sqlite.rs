//! Durable SQLite ownership tests for the non-default checkpoint runtime.

use std::fs;

use mesh_store::{
    CheckpointCoordinator, CheckpointRuntimeParameters, RecordDigest, RecoveryEventUlid,
    RecoveryPreserved, RecoverySequence, RecoverySnapshot, RecoveryStamp,
    RecoveryStatePersistence as _, RecoveryTrigger, SqlExecutor as _, Sqlite, SqliteRecoveryState,
    Store, RECOVERY_DATABASE_FILE_NAME,
};

fn scratch(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "mesh-checkpoint-sqlite-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ))
}

fn parameters() -> CheckpointRuntimeParameters {
    CheckpointRuntimeParameters {
        idle_interval: Some(std::time::Duration::from_millis(8)),
        maximum_uncheckpointed_bytes: Some(16),
        maximum_uncheckpointed_interval: Some(std::time::Duration::from_millis(20)),
    }
}

fn stamp(byte: u8) -> RecoveryStamp {
    RecoveryStamp::new(
        u64::from(byte),
        RecoveryEventUlid::from_bytes([byte; 16]),
        RecordDigest::from_bytes([byte; 32]),
    )
}

fn persisted_recovery(database: &std::path::Path, view: &[u8], byte: u8) {
    let sequence = RecoverySequence::new(u64::from(byte)).expect("non-zero sequence");
    let mut runtime = CheckpointCoordinator::open(
        SqliteRecoveryState::open(database, view).expect("state owner"),
        parameters(),
    )
    .expect("runtime");
    runtime.observe(sequence, 1).expect("event");
    runtime
        .preserve(
            RecoveryTrigger::ActorDisconnected,
            RecoveryPreserved::from_verified_bytes(
                stamp(byte),
                sequence,
                vec![byte],
                RecordDigest::from_bytes([byte; 32]),
            )
            .expect("verified"),
        )
        .expect("preserved");
}

fn recovery_snapshot(byte: u8) -> RecoverySnapshot {
    let root = scratch(&format!("snapshot-{byte}"));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("snapshot scratch");
    let database = root.join(RECOVERY_DATABASE_FILE_NAME);
    let sequence = RecoverySequence::new(u64::from(byte)).expect("non-zero sequence");
    let mut runtime = CheckpointCoordinator::open(
        SqliteRecoveryState::open(&database, b"snapshot").expect("temporary owner"),
        parameters(),
    )
    .expect("temporary runtime");
    runtime.observe(sequence, 1).expect("event");
    runtime
        .preserve(
            RecoveryTrigger::ActorDisconnected,
            RecoveryPreserved::from_verified_bytes(
                stamp(byte),
                sequence,
                vec![byte],
                RecordDigest::from_bytes([byte; 32]),
            )
            .expect("verified"),
        )
        .expect("preserved");
    let snapshot = runtime.machine().snapshot().clone();
    drop(runtime);
    let _ = fs::remove_dir_all(&root);
    snapshot
}

#[test]
fn sqlite_owner_restores_recovery_and_open_window_after_reopen() {
    let root = scratch("reopen");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("scratch");
    let database = root.join("metadata.sqlite");
    Store::open(Sqlite::open(&database).expect("sqlite")).expect("schema");

    let mut runtime = CheckpointCoordinator::open(
        SqliteRecoveryState::open(&database, b"actor-view").expect("state owner"),
        parameters(),
    )
    .expect("runtime");
    let first = RecoverySequence::new(1).expect("non-zero");
    let last = RecoverySequence::new(2).expect("non-zero");
    runtime.observe(first, 4).expect("first event");
    runtime.observe(last, 4).expect("last event");
    let recovery = RecoveryPreserved::from_verified_bytes(
        stamp(7),
        last,
        b"recovery bytes".to_vec(),
        RecordDigest::from_bytes([7; 32]),
    )
    .expect("verified");
    runtime
        .preserve(RecoveryTrigger::IntegratedAgentRequestsFlush, recovery)
        .expect("preserved");
    drop(runtime);

    let reopened = CheckpointCoordinator::open(
        SqliteRecoveryState::open(&database, b"actor-view").expect("reopen state"),
        parameters(),
    )
    .expect("reopened runtime");
    let snapshot = reopened.machine().snapshot();
    assert_eq!(snapshot.open_window().expect("window").from(), first);
    assert_eq!(snapshot.open_window().expect("window").last(), last);
    assert_eq!(
        snapshot.latest_recovery().expect("recovery").bytes(),
        b"recovery bytes"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn record_index_rebuild_leaves_runtime_state_intact() {
    let root = scratch("rebuild");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("scratch");
    let database = root.join("metadata.sqlite");
    Store::open(Sqlite::open(&database).expect("sqlite")).expect("schema");

    let sequence = RecoverySequence::new(3).expect("non-zero");
    let mut runtime = CheckpointCoordinator::open(
        SqliteRecoveryState::open(&database, b"actor-view").expect("state owner"),
        parameters(),
    )
    .expect("runtime");
    runtime.observe(sequence, 1).expect("event");
    runtime
        .preserve(
            RecoveryTrigger::ActorDisconnected,
            RecoveryPreserved::from_verified_bytes(
                stamp(8),
                sequence,
                vec![8],
                RecordDigest::from_bytes([8; 32]),
            )
            .expect("verified"),
        )
        .expect("preserved");
    drop(runtime);

    let mut store = Store::open(Sqlite::open(&database).expect("store reopen")).expect("store");
    store.rebuild_from(Vec::new()).expect("routine replay");
    drop(store);

    let mut state = SqliteRecoveryState::open(&database, b"actor-view").expect("state reopen");
    let snapshot = state.load().expect("load").expect("admitted state");
    assert_eq!(snapshot.open_window().expect("window").last(), sequence);
    assert_eq!(snapshot.latest_recovery().expect("recovery").bytes(), &[8]);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn views_are_isolated_and_empty_keys_are_refused() {
    let root = scratch("views");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("scratch");
    let database = root.join("metadata.sqlite");
    Store::open(Sqlite::open(&database).expect("sqlite")).expect("schema");
    assert!(SqliteRecoveryState::open(&database, []).is_err());
    let mut first = SqliteRecoveryState::open(&database, b"first").expect("first");
    let mut second = SqliteRecoveryState::open(&database, b"second").expect("second");
    assert!(first.load().expect("first load").is_none());
    assert!(second.load().expect("second load").is_none());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn stale_recovery_writer_cannot_replace_a_newer_admitted_snapshot() {
    let root = scratch("stale-writer");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("scratch");
    let database = root.join(RECOVERY_DATABASE_FILE_NAME);
    let mut winner = CheckpointCoordinator::open(
        SqliteRecoveryState::open(&database, b"actor-view").expect("winner state owner"),
        parameters(),
    )
    .expect("winner runtime");
    let mut duplicate = CheckpointCoordinator::open(
        SqliteRecoveryState::open(&database, b"actor-view").expect("duplicate state owner"),
        parameters(),
    )
    .expect("duplicate runtime");
    let mut stale = CheckpointCoordinator::open(
        SqliteRecoveryState::open(&database, b"actor-view").expect("stale state owner"),
        parameters(),
    )
    .expect("stale runtime");

    let winner_sequence = RecoverySequence::new(7).expect("non-zero");
    winner.observe(winner_sequence, 1).expect("winner event");
    winner
        .preserve(
            RecoveryTrigger::ActorDisconnected,
            RecoveryPreserved::from_verified_bytes(
                stamp(7),
                winner_sequence,
                vec![7],
                RecordDigest::from_bytes([7; 32]),
            )
            .expect("winner recovery"),
        )
        .expect("winner admitted");
    duplicate
        .observe(winner_sequence, 1)
        .expect("duplicate event");
    duplicate
        .preserve(
            RecoveryTrigger::ActorDisconnected,
            RecoveryPreserved::from_verified_bytes(
                stamp(7),
                winner_sequence,
                vec![7],
                RecordDigest::from_bytes([7; 32]),
            )
            .expect("duplicate recovery"),
        )
        .expect("an exact concurrent duplicate is idempotent");

    let stale_sequence = RecoverySequence::new(8).expect("non-zero");
    stale.observe(stale_sequence, 1).expect("stale event");
    let refused = stale
        .preserve(
            RecoveryTrigger::ActorDisconnected,
            RecoveryPreserved::from_verified_bytes(
                stamp(8),
                stale_sequence,
                vec![8],
                RecordDigest::from_bytes([8; 32]),
            )
            .expect("stale recovery"),
        )
        .expect_err("a stale writer must not replace newer durable recovery truth");
    assert!(
        refused.to_string().contains("changed by another writer"),
        "the refusal explains the recovery ownership conflict: {refused}"
    );

    drop(winner);
    drop(duplicate);
    drop(stale);
    let reopened = CheckpointCoordinator::open(
        SqliteRecoveryState::open(&database, b"actor-view").expect("reopened state owner"),
        parameters(),
    )
    .expect("reopened runtime");
    let snapshot = reopened.machine().snapshot();
    assert_eq!(
        snapshot.latest_recovery().expect("winner recovery").bytes(),
        &[7],
        "the first admitted recovery snapshot remains authoritative"
    );
    assert_eq!(
        snapshot.open_window().expect("winner window").last(),
        winner_sequence
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn journal_backed_observation_wins_restart_then_primary_persist_clears_it() {
    let root = scratch("observation-recovery");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("scratch");
    let database = root.join(RECOVERY_DATABASE_FILE_NAME);
    let primary = recovery_snapshot(7);
    let recovered = recovery_snapshot(8);

    let mut owner = SqliteRecoveryState::open(&database, b"actor-view").expect("owner");
    assert!(owner.load().expect("initial load").is_none());
    owner.persist(&primary).expect("primary snapshot");
    owner
        .persist_observation_recovery(&recovered)
        .expect("journal-backed recovery snapshot");
    drop(owner);

    let mut restarted = SqliteRecoveryState::open(&database, b"actor-view").expect("restart");
    let loaded = restarted.load().expect("load").expect("recovery wins");
    assert_eq!(
        loaded.latest_recovery().expect("recovery").bytes(),
        &[8],
        "restart prefers the post-journal recovery slot over stale primary state"
    );
    restarted
        .persist(&loaded)
        .expect("normal persistence folds recovery into primary");
    drop(restarted);

    let connection = rusqlite::Connection::open(&database).expect("inspect database");
    let pending: i64 = connection
        .query_row(
            "SELECT count(*) FROM mesh_recovery_observation WHERE view_key = ?1",
            [b"actor-view".as_slice()],
            |row| row.get(0),
        )
        .expect("pending recovery count");
    assert_eq!(
        pending, 0,
        "primary persistence atomically retires recovery"
    );
    drop(connection);

    let mut final_owner = SqliteRecoveryState::open(&database, b"actor-view").expect("final");
    assert_eq!(
        final_owner
            .load()
            .expect("final load")
            .expect("primary")
            .latest_recovery()
            .expect("recovery")
            .bytes(),
        &[8]
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn conflicting_journal_backed_observation_is_refused_without_replacing_the_first() {
    let root = scratch("observation-conflict");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("scratch");
    let database = root.join(RECOVERY_DATABASE_FILE_NAME);
    let mut owner = SqliteRecoveryState::open(&database, b"actor-view").expect("owner");
    assert!(owner.load().expect("initial load").is_none());
    owner
        .persist_observation_recovery(&recovery_snapshot(7))
        .expect("first recovery");
    let refused = owner
        .persist_observation_recovery(&recovery_snapshot(8))
        .expect_err("a different unresolved observation cannot replace the first");
    assert!(
        refused.to_string().contains("conflicts"),
        "the refusal names the pending-observation conflict: {refused}"
    );
    drop(owner);

    let mut restarted = SqliteRecoveryState::open(&database, b"actor-view").expect("restart");
    assert_eq!(
        restarted
            .load()
            .expect("load")
            .expect("first recovery remains")
            .latest_recovery()
            .expect("recovery")
            .bytes(),
        &[7]
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn stale_writers_cannot_publish_or_erase_a_concurrent_observation_recovery() {
    let root = scratch("observation-races");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("scratch");
    let database = root.join(RECOVERY_DATABASE_FILE_NAME);

    let mut winner = SqliteRecoveryState::open(&database, b"actor-view").expect("winner");
    let mut stale = SqliteRecoveryState::open(&database, b"actor-view").expect("stale");
    assert!(winner.load().expect("winner load").is_none());
    assert!(stale.load().expect("stale load").is_none());
    winner
        .persist(&recovery_snapshot(7))
        .expect("winner advances primary");
    assert!(stale
        .persist_observation_recovery(&recovery_snapshot(8))
        .expect_err("stale fallback must not override newer primary")
        .to_string()
        .contains("changed by another writer"));

    let mut primary_writer = SqliteRecoveryState::open(&database, b"actor-view").expect("primary");
    let mut recovery_writer =
        SqliteRecoveryState::open(&database, b"actor-view").expect("recovery");
    primary_writer.load().expect("primary observes winner");
    recovery_writer.load().expect("recovery observes winner");
    recovery_writer
        .persist_observation_recovery(&recovery_snapshot(8))
        .expect("concurrent journal-backed recovery");
    assert!(primary_writer
        .persist(&recovery_snapshot(9))
        .expect_err("stale primary persist must not erase fallback")
        .to_string()
        .contains("journal-backed recovery snapshot changed"));
    drop(winner);
    drop(stale);
    drop(primary_writer);
    drop(recovery_writer);

    let mut restarted = SqliteRecoveryState::open(&database, b"actor-view").expect("restart");
    assert_eq!(
        restarted
            .load()
            .expect("load")
            .expect("recovery remains")
            .latest_recovery()
            .expect("recovery")
            .bytes(),
        &[8]
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn legacy_copy_forward_retries_an_empty_target_and_then_prefers_the_isolated_row() {
    let root = scratch("copy-forward");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("scratch");
    let legacy = root.join("metadata.sqlite");
    let isolated = root.join(RECOVERY_DATABASE_FILE_NAME);
    Store::open(Sqlite::open(&legacy).expect("sqlite")).expect("schema");
    persisted_recovery(&legacy, b"actor-view", 7);

    // A process can stop after creating the destination schema but before copying the row. An
    // absent destination row is deliberately not a completed migration marker.
    drop(SqliteRecoveryState::open(&isolated, b"actor-view").expect("empty destination"));
    let mut migrated = SqliteRecoveryState::open_isolated(&isolated, &legacy, b"actor-view")
        .expect("copy retries");
    let snapshot = migrated.load().expect("load").expect("copied row");
    assert_eq!(snapshot.latest_recovery().expect("recovery").bytes(), &[7]);
    drop(migrated);

    // Once the isolated row exists it is authoritative; a stale legacy row cannot replace it.
    let mut legacy_state = SqliteRecoveryState::open(&legacy, b"actor-view").expect("legacy");
    legacy_state
        .persist(&RecoverySnapshot::default())
        .expect("make legacy stale");
    drop(legacy_state);
    let mut reopened = SqliteRecoveryState::open_isolated(&isolated, &legacy, b"actor-view")
        .expect("isolated reopen");
    let snapshot = reopened.load().expect("load").expect("isolated row");
    assert_eq!(snapshot.latest_recovery().expect("recovery").bytes(), &[7]);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn preservation_without_runtime_configuration_only_creates_an_owner_for_existing_truth() {
    let root = scratch("copy-forward-only-when-present");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("scratch");
    let legacy = root.join("metadata.sqlite");
    let isolated = root.join(RECOVERY_DATABASE_FILE_NAME);
    Store::open(Sqlite::open(&legacy).expect("sqlite")).expect("schema");

    assert!(
        !SqliteRecoveryState::preserve_legacy_if_present(&isolated, &legacy, b"actor-view")
            .expect("absence is observable")
    );
    assert!(
        !isolated.exists(),
        "a workspace that never persisted recovery acquired a private owner"
    );

    persisted_recovery(&legacy, b"actor-view", 9);
    assert!(
        SqliteRecoveryState::preserve_legacy_if_present(&isolated, &legacy, b"actor-view")
            .expect("legacy truth is preserved")
    );
    let mut reopened = SqliteRecoveryState::open(&isolated, b"actor-view").expect("isolated");
    assert_eq!(
        reopened
            .load()
            .expect("load")
            .expect("copied row")
            .latest_recovery()
            .expect("recovery")
            .bytes(),
        &[9]
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn failed_copy_preserves_legacy_and_a_corrupt_isolated_database_never_falls_back() {
    let root = scratch("copy-failure");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("scratch");
    let legacy = root.join("metadata.sqlite");
    let isolated = root.join(RECOVERY_DATABASE_FILE_NAME);
    Store::open(Sqlite::open(&legacy).expect("sqlite")).expect("schema");
    persisted_recovery(&legacy, b"actor-view", 8);

    drop(SqliteRecoveryState::open(&isolated, b"actor-view").expect("destination schema"));
    let mut destination = Sqlite::open(&isolated).expect("destination connection");
    destination
        .execute_batch(
            "CREATE TRIGGER refuse_migration BEFORE INSERT ON mesh_recovery_state
             BEGIN SELECT RAISE(ABORT, 'injected migration failure'); END;",
        )
        .expect("failure trigger");
    drop(destination);
    assert!(
        SqliteRecoveryState::open_isolated(&isolated, &legacy, b"actor-view").is_err(),
        "the injected copy failure must fail closed"
    );
    let mut legacy_state = SqliteRecoveryState::open(&legacy, b"actor-view").expect("legacy");
    assert_eq!(
        legacy_state
            .load()
            .expect("legacy load")
            .expect("legacy preserved")
            .latest_recovery()
            .expect("recovery")
            .bytes(),
        &[8]
    );

    fs::remove_file(&isolated).expect("remove injected database");
    let _ = fs::remove_file(format!("{}-wal", isolated.display()));
    let _ = fs::remove_file(format!("{}-shm", isolated.display()));
    fs::write(&isolated, b"not a sqlite database").expect("corrupt isolated owner");
    assert!(
        SqliteRecoveryState::open_isolated(&isolated, &legacy, b"actor-view").is_err(),
        "a corrupt authoritative owner must not silently fall back to stale legacy state"
    );
    let _ = fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn isolated_owner_refuses_a_hard_link_to_the_disposable_index() {
    let root = scratch("hard-link-alias");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("scratch");
    let legacy = root.join("metadata.sqlite");
    let isolated = root.join(RECOVERY_DATABASE_FILE_NAME);
    Store::open(Sqlite::open(&legacy).expect("sqlite")).expect("schema");
    fs::hard_link(&legacy, &isolated).expect("hard-link recovery path to index");
    let before = fs::read(&legacy).expect("index bytes before refusal");

    let refused = SqliteRecoveryState::open_isolated(&isolated, &legacy, b"actor-view")
        .expect_err("the recovery owner must be a physically separate file");
    assert!(
        refused.to_string().contains("separate"),
        "the refusal explains the physical-isolation requirement: {refused}"
    );

    assert_eq!(
        fs::read(&legacy).expect("index bytes after refusal"),
        before,
        "refusal happens before SQLite can modify the disposable index through the alias"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn isolated_owner_refuses_a_parent_component_alias_to_the_disposable_index() {
    let root = scratch("parent-component-alias");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("alias")).expect("scratch alias directory");
    let legacy = root.join("metadata.sqlite");
    let aliased = root.join("alias").join("..").join("metadata.sqlite");
    Store::open(Sqlite::open(&legacy).expect("sqlite")).expect("schema");
    let before = fs::read(&legacy).expect("index bytes before refusal");

    for refused in [
        SqliteRecoveryState::inspect_isolated_read_only(&aliased, &legacy, b"actor-view")
            .expect_err("read-only inspection must reject the path alias"),
        SqliteRecoveryState::preserve_legacy_if_present(&aliased, &legacy, b"actor-view")
            .expect_err("copy-forward must reject the path alias"),
        SqliteRecoveryState::open_isolated(&aliased, &legacy, b"actor-view")
            .expect_err("the recovery owner must not resolve to the disposable index"),
    ] {
        assert!(
            refused.to_string().contains("separate"),
            "the refusal explains the physical-isolation requirement: {refused}"
        );
    }
    assert_eq!(
        fs::read(&legacy).expect("index bytes after refusal"),
        before,
        "refusal happens before recovery schema writes reach the disposable index"
    );
    let _ = fs::remove_dir_all(&root);
}
