//! "A migration is forward-only and every migration has a test that runs it against a populated
//! database."
//!
//! # The word doing the work is *populated*
//!
//! A migration tested against an empty database is tested against the easiest input it will ever
//! see. Every migration below runs against a database that the previous migrations built and that
//! the commit path has already written real rows into, and the assertion afterwards is that those
//! rows are still there and still readable. That is the failure a migration test exists to catch:
//! not "does the DDL parse" but "does applying it lose data".
//!
//! The loop is generated from [`MIGRATIONS`] rather than written out, so a migration added without
//! a test is impossible — adding one to the list adds it to every test here.
//!
//! # Forward-only is checked at both levels
//!
//! At the type level there is no reverse migration to run: [`Migration`] has one SQL field. At the
//! runtime level a database from the future is refused, a ledger with a gap is refused, and a
//! migration whose text changed after it ran is refused — the last being the failure that
//! "forward-only" on its own does *not* catch, and the reason `schema_version` carries a
//! fingerprint at all.

mod common;

use common::{Sqlite3, TempDir};
use mesh_store::{
    plan_migrations, AckRecord, Checkpoint, ManifestRecord, MigrationError, OperationRecord,
    PeerRecord, RecordDigest, SqlExecutor, Store, StoreError, CURRENT_VERSION, MIGRATIONS, TABLES,
};

fn digest(seed: u8) -> RecordDigest {
    RecordDigest::from_bytes([seed; 32])
}

fn operation(id: u8, actor: u8, sequence: u64) -> OperationRecord {
    OperationRecord {
        id: digest(id),
        actor: digest(actor),
        actor_sequence: sequence,
        hlc_millis: 1_700_000_000_000,
        hlc_counter: 0,
        policy_epoch: 2,
        session: mesh_store::no_session(),
        payload_digest: digest(id.wrapping_add(100)),
        parents: Vec::new(),
    }
}

/// Rows the commit path writes: enough to touch every table version 1 creates.
fn populating_checkpoint() -> Checkpoint {
    Checkpoint {
        manifests: vec![ManifestRecord {
            id: digest(20),
            byte_length: 30,
            content_digest: digest(21),
            chunks: vec![mesh_store::ChunkSlice {
                digest: digest(22),
                byte_offset: 0,
                byte_length: 30,
            }],
        }],
        operations: vec![operation(1, 2, 1), operation(3, 2, 2), operation(5, 6, 1)],
        peers: vec![PeerRecord {
            peer: digest(30),
            joined_at: digest(1),
        }],
        acknowledgements: vec![AckRecord {
            peer: digest(30),
            actor: digest(2),
            actor_sequence: 1,
        }],
        ..Checkpoint::default()
    }
}

/// Apply migrations 1..=`version` by hand, without the store, so a database can be stood up at any
/// historical version.
fn database_at_version(directory: &TempDir, name: &str, version: u32) -> Sqlite3 {
    let mut executor = Sqlite3::at(directory.join(name));
    let pragmas: String = mesh_store::PRAGMAS
        .iter()
        .map(mesh_store::Pragma::sql)
        .collect::<Vec<_>>()
        .join("\n");
    executor.execute_batch(&pragmas).expect("pragmas apply");

    let plan = plan_migrations(0).expect("plans");
    let mut sql = String::from("BEGIN IMMEDIATE;\n");
    for (index, statement) in plan.statements().iter().enumerate() {
        // `statements()` emits each migration's SQL followed by its ledger row, so two statements
        // per version.
        if u32::try_from(index / 2).unwrap_or(u32::MAX) < version {
            sql.push_str(statement);
            sql.push('\n');
        }
    }
    sql.push_str("COMMIT;\n");
    executor.execute_batch(&sql).expect("the migrations apply");
    executor
}

fn table_names(executor: &Sqlite3) -> Vec<String> {
    executor
        .run("SELECT name FROM sqlite_schema WHERE type = 'table' ORDER BY name;")
        .expect("the schema is readable")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .collect()
}

fn full_schema(executor: &Sqlite3) -> String {
    executor
        .run("SELECT type || ' ' || name || ' ' || COALESCE(sql, '') FROM sqlite_schema ORDER BY type, name;")
        .expect("the schema is readable")
}

// -- Every migration, against a populated database ----------------------------------------------

/// The generated loop. For each migration N: build a database at N-1, populate it through the
/// commit path where the schema allows, apply N, and check that what was there is still there.
#[test]
fn every_migration_runs_against_a_database_the_previous_ones_populated() {
    for migration in MIGRATIONS {
        let directory = TempDir::new(&format!("migration-{}", migration.version));
        let previous = migration.version - 1;

        // A database at exactly the version *before* this migration. `Store::open` cannot be used
        // for this — it migrates all the way to current, which is the thing under test.
        let mut database = database_at_version(&directory, "metadata.sqlite", previous);

        // Populate it. Version 1 creates the tables, so from migration 2 onwards there is
        // something to populate; migration 1 runs against a genuinely empty file because that is
        // what it is for.
        let before = if previous >= 1 {
            database
                .execute_batch(&populate_version_one_sql())
                .expect("the version-1 tables populate");
            let count = database
                .count("SELECT count(*) FROM operation;")
                .expect("counts");
            assert!(
                count > 0,
                "migration {} would have run against an empty database, which is the one input a \
                 migration test must not use",
                migration.version
            );
            count
        } else {
            0
        };

        database
            .execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{}\nINSERT INTO schema_version (version, sql_fingerprint) \
                 VALUES ({}, X'{}');\nCOMMIT;",
                migration.sql,
                migration.version,
                migration.fingerprint().to_hex()
            ))
            .unwrap_or_else(|error| {
                panic!("migration {} failed to apply: {error}", migration.version)
            });

        if previous >= 1 {
            let after = database
                .count("SELECT count(*) FROM operation;")
                .expect("counts");
            assert_eq!(
                before, after,
                "migration {} lost rows from `operation`",
                migration.version
            );
        }

        for name in mesh_store::table_names_in_ddl(migration.sql) {
            assert!(
                database.table_exists(&name).expect("asks"),
                "migration {} claims to create `{name}` and did not",
                migration.version
            );
        }

        // And the store can take it from here, which is what a half-migrated database in the wild
        // would need it to do.
        let store = Store::open(Sqlite3::at(directory.join("metadata.sqlite")))
            .expect("the store finishes the migration");
        assert_eq!(store.schema_version(), CURRENT_VERSION);

        println!(
            "migration {} ({}) applied to a database holding {before} operations",
            migration.version, migration.name
        );
    }
}

/// Insert into every version-1 table with plain SQL, so a version-1 database can be populated
/// without a `Store` (which would migrate it to current and defeat the exercise).
fn populate_version_one_sql() -> String {
    let one = digest(1).to_hex();
    let actor = digest(2).to_hex();
    let peer = digest(30).to_hex();
    let manifest = digest(20).to_hex();
    let zero = "00".repeat(16);
    format!(
        "BEGIN IMMEDIATE;
         INSERT INTO operation VALUES (X'{one}', X'{actor}', 1, 1700000000000, 0, 2, X'{zero}', X'{one}');
         INSERT INTO operation_parent VALUES (X'{one}', 0, X'{one}');
         INSERT INTO manifest VALUES (X'{manifest}', 30, X'{manifest}');
         INSERT INTO manifest_chunk VALUES (X'{manifest}', 0, X'{manifest}', 0, 30);
         INSERT INTO actor_head VALUES (X'{actor}', X'{one}', 1);
         INSERT INTO peer VALUES (X'{peer}', X'{one}');
         INSERT INTO peer_watermark VALUES (X'{peer}', X'{actor}', 0);
         INSERT INTO outbox VALUES (X'{peer}', X'{actor}', 1, X'{one}');
         COMMIT;"
    )
}

/// A database migrated step by step must end up with the same schema as one created in one go.
/// This is what stops a migration and the base schema drifting into two different databases that
/// both call themselves version N.
#[test]
fn a_stepwise_migration_produces_the_same_schema_as_a_fresh_database() {
    let stepwise_dir = TempDir::new("stepwise");
    let stepwise = database_at_version(&stepwise_dir, "metadata.sqlite", 1);
    let mut stepwise_executor = Sqlite3::at(stepwise_dir.join("metadata.sqlite"));
    stepwise_executor
        .execute_batch(&populate_version_one_sql())
        .expect("populates");
    let _ = Store::open(Sqlite3::at(stepwise_dir.join("metadata.sqlite")))
        .expect("the store migrates it forward");

    let fresh_dir = TempDir::new("fresh");
    let _ = Store::open(Sqlite3::at(fresh_dir.join("metadata.sqlite"))).expect("opens fresh");
    let fresh = Sqlite3::at(fresh_dir.join("metadata.sqlite"));

    assert_eq!(table_names(&stepwise), table_names(&fresh));
    assert_eq!(full_schema(&stepwise), full_schema(&fresh));

    // And the rows that were there before the migration are still there afterwards.
    assert_eq!(
        stepwise
            .count("SELECT count(*) FROM operation;")
            .expect("counts"),
        1
    );
}

#[test]
fn a_fresh_database_ends_up_at_the_current_version_with_every_table() {
    let directory = TempDir::new("fresh-complete");
    let store = Store::open(Sqlite3::at(directory.join("metadata.sqlite"))).expect("opens");
    assert_eq!(store.schema_version(), CURRENT_VERSION);

    let executor = Sqlite3::at(directory.join("metadata.sqlite"));
    let present = table_names(&executor);
    for table in TABLES {
        assert!(
            present.contains(&table.name.to_owned()),
            "missing {}",
            table.name
        );
    }
    assert_eq!(
        executor
            .count("SELECT count(*) FROM schema_version;")
            .expect("counts"),
        i64::try_from(MIGRATIONS.len()).expect("fits")
    );
}

#[test]
fn reopening_an_up_to_date_database_changes_nothing() {
    let directory = TempDir::new("reopen");
    let path = directory.join("metadata.sqlite");
    let mut store = Store::open(Sqlite3::at(&path)).expect("opens");
    store.commit(&populating_checkpoint()).expect("commits");

    let before = full_schema(&Sqlite3::at(&path));
    let reopened = Store::open(Sqlite3::at(&path)).expect("reopens");
    assert_eq!(reopened.schema_version(), CURRENT_VERSION);
    assert_eq!(full_schema(&Sqlite3::at(&path)), before);
}

// -- Forward-only -------------------------------------------------------------------------------

/// The failure "forward-only" alone does not catch, against a real database: migration 1's text is
/// changed after it has run, and the store refuses to reopen.
#[test]
fn a_rewritten_migration_is_refused_on_reopen() {
    let directory = TempDir::new("rewritten");
    let path = directory.join("metadata.sqlite");
    let _ = Store::open(Sqlite3::at(&path)).expect("opens");

    let mut surgeon = Sqlite3::at(&path);
    surgeon
        .execute_batch("UPDATE schema_version SET sql_fingerprint = X'00000000000000000000000000000000' WHERE version = 1;")
        .expect("the fingerprint is changed");

    let error = Store::open(Sqlite3::at(&path)).expect_err("the store refuses");
    assert!(
        matches!(
            error,
            StoreError::Migration(MigrationError::Rewritten { version: 1, .. })
        ),
        "{error}"
    );
    assert!(error.to_string().contains("immutable"));
}

/// A database written by a newer build must not be touched by an older one.
#[test]
fn a_database_from_the_future_is_refused() {
    let directory = TempDir::new("future");
    let path = directory.join("metadata.sqlite");
    let _ = Store::open(Sqlite3::at(&path)).expect("opens");

    let mut surgeon = Sqlite3::at(&path);
    surgeon
        .execute_batch(&format!(
            "INSERT INTO schema_version (version, sql_fingerprint) VALUES ({}, X'{}');",
            CURRENT_VERSION + 1,
            "aa".repeat(16)
        ))
        .expect("a future row is written");

    let error = Store::open(Sqlite3::at(&path)).expect_err("the store refuses");
    assert!(
        matches!(
            error,
            StoreError::Migration(MigrationError::FromTheFuture { .. })
        ),
        "{error}"
    );
}

/// A ledger with a hole means a migration never ran, which is not something to migrate past.
#[test]
fn a_ledger_with_a_gap_is_refused() {
    let directory = TempDir::new("gap");
    let path = directory.join("metadata.sqlite");
    let _ = Store::open(Sqlite3::at(&path)).expect("opens");

    let mut surgeon = Sqlite3::at(&path);
    surgeon
        .execute_batch("DELETE FROM schema_version WHERE version = 1;")
        .expect("the hole is made");

    let error = Store::open(Sqlite3::at(&path)).expect_err("the store refuses");
    assert!(
        matches!(error, StoreError::Migration(MigrationError::Gap { .. })),
        "{error}"
    );
}

/// The version table is a real table with a real constraint, not a convention.
#[test]
fn the_version_table_refuses_a_duplicate_or_zero_version() {
    let directory = TempDir::new("version-table");
    let path = directory.join("metadata.sqlite");
    let _ = Store::open(Sqlite3::at(&path)).expect("opens");
    let mut executor = Sqlite3::at(&path);

    let duplicate = executor.execute_batch(&format!(
        "INSERT INTO schema_version (version, sql_fingerprint) VALUES (1, X'{}');",
        "bb".repeat(16)
    ));
    assert!(duplicate.is_err(), "a duplicate version was accepted");

    let zero = executor.execute_batch(&format!(
        "INSERT INTO schema_version (version, sql_fingerprint) VALUES (0, X'{}');",
        "bb".repeat(16)
    ));
    assert!(zero.is_err(), "version zero was accepted");

    let short = executor
        .execute_batch("INSERT INTO schema_version (version, sql_fingerprint) VALUES (99, X'00');");
    assert!(short.is_err(), "a short fingerprint was accepted");
}

/// `STRICT` is what makes the length checks mean anything: without it, a text value in a `BLOB`
/// column would pass a `length()` check on its character count.
#[test]
fn strict_tables_refuse_a_string_where_a_digest_belongs() {
    let directory = TempDir::new("strict");
    let path = directory.join("metadata.sqlite");
    let _ = Store::open(Sqlite3::at(&path)).expect("opens");
    let mut executor = Sqlite3::at(&path);

    // Every string is exactly as long as its own column's length CHECK requires, so every CHECK
    // passes on the character count and `STRICT` is the only refusal left. An earlier version used
    // thirty-two characters everywhere, including in `session_id`, whose CHECK demands sixteen — so
    // the CHECK rejected the row and the test passed with `STRICT` deleted from the migration. A
    // mutation test found that; reading it did not.
    let digest_shaped = "x".repeat(32);
    let uuid_shaped = "y".repeat(16);
    let result = executor.execute_batch(&format!(
        "INSERT INTO operation VALUES ('{digest_shaped}', '{digest_shaped}', 1, 0, 0, 0, \
         '{uuid_shaped}', '{digest_shaped}');"
    ));
    assert!(
        result.is_err(),
        "a STRICT table accepted text in a BLOB column. Every length CHECK on this row passes on \
         the character count, so STRICT was the only refusal left — and there was none."
    );
    assert_eq!(
        executor
            .count("SELECT count(*) FROM operation;")
            .expect("counts"),
        0,
        "the refused row was written anyway"
    );
}

/// Foreign keys are off by default in SQLite, so every `REFERENCES` clause is decoration until the
/// pragma is set. This is the check that `PRAGMAS` really applies it — which means the test must
/// not apply it itself. It used to, which made it a test that SQLite enforces foreign keys and not
/// a test of anything in this crate: turning `foreign_keys` off in `PRAGMAS` left it green.
#[test]
fn foreign_keys_are_enforced_on_a_store_opened_connection() {
    let directory = TempDir::new("foreign-keys");
    let path = directory.join("metadata.sqlite");
    let mut store = Store::open(Sqlite3::at(&path)).expect("opens");
    store.commit(&populating_checkpoint()).expect("commits");

    let orphan = store.executor_mut().execute_batch(&format!(
        "INSERT INTO actor_head VALUES (X'{}', X'{}', 1);",
        digest(99).to_hex(),
        digest(98).to_hex()
    ));
    assert!(
        orphan.is_err(),
        "an actor_head row pointing at no operation was accepted"
    );
}

#[test]
fn consumption_kind_migration_preserves_populated_dependency_rows() {
    let directory = TempDir::new("consumption-kind-upgrade");
    let mut database = database_at_version(&directory, "metadata.sqlite", 3);
    database.execute_batch(&format!(
        "INSERT INTO dependency_record VALUES (X'{}',1,X'{}',X'{}',0); INSERT INTO dependency_record VALUES (X'{}',2,X'{}',X'{}',1);",
        digest(1).to_hex(), digest(0).to_hex(), digest(11).to_hex(), digest(1).to_hex(), digest(11).to_hex(), digest(12).to_hex(),
    )).unwrap();
    let table = TABLES
        .iter()
        .find(|table| table.name == "dependency_record")
        .unwrap();
    let before = database.read_table(table).unwrap();
    assert_eq!(before.len(), 2);
    let store = Store::open(Sqlite3::at(directory.join("metadata.sqlite"))).unwrap();
    assert_eq!(store.schema_version(), CURRENT_VERSION);
    drop(store);
    assert_eq!(database.read_table(table).unwrap(), before);
    assert!(!database
        .table_exists("dependency_record_before_consumption")
        .unwrap());
    assert!(database
        .execute_batch(&format!(
            "INSERT INTO dependency_record VALUES (X'{}',3,X'{}',X'{}',7);",
            digest(1).to_hex(),
            digest(12).to_hex(),
            digest(13).to_hex()
        ))
        .is_err());
    assert_eq!(database.read_table(table).unwrap(), before);
}
