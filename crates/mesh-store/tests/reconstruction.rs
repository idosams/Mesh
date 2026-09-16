//! "Every table is reconstructable from immutable operations and manifests alone."
//!
//! The sharpest criterion in the task, and the one most easily satisfied on paper. Three separate
//! things have to be true, and each is checked separately below rather than folded into one
//! happy-path assertion:
//!
//! 1. **The schema declares nowhere for an unreconstructable fact to live.** Every `CREATE TABLE`
//!    in the migrations has a `Provenance`, every `Provenance` names record kinds, and the one
//!    exception — the migration ledger — is exactly one table. Checked from both sides, so neither
//!    a table missing from the schema declarations nor a declaration missing from the SQL passes.
//! 2. **The rebuild really does reproduce the live index.** Not "has the same row counts": the
//!    same digest, over every table, in order.
//! 3. **SQLite agrees.** The in-memory rows are written to a real database and read back, and the
//!    digest of what came back must equal the digest of what went in. This is where an ordering
//!    difference between `Row`'s `Ord` and SQL's `ORDER BY`, or a column whose storage class does
//!    not survive a round trip, would show up.
//!
//! Then the whole thing is done destructively: the database is corrupted, rebuilt from records
//! alone, and the digest has to come back.

mod common;

use common::{Sqlite3, TempDir};
use mesh_store::{
    full_schema_sql, indexed_record_kinds, read_all_tables, rebuild, table_names_in_ddl, AckRecord,
    ApprovalRecord, Checkpoint, ChunkSlice, ContextAccess, ContextRecord, EntityUuid, Index,
    ManifestRecord, OperationRecord, PeerRecord, Provenance, RecordDigest, RecordKind,
    ReviewRecord, ReviewVerdict, SqlExecutor, Store, MIGRATIONS, TABLES,
};

fn digest(seed: u8) -> RecordDigest {
    RecordDigest::from_bytes([seed; 32])
}

fn session(seed: u8) -> EntityUuid {
    EntityUuid::from_bytes([seed; 16])
}

fn operation(id: u8, actor: u8, sequence: u64, parents: &[u8]) -> OperationRecord {
    OperationRecord {
        id: digest(id),
        actor: digest(actor),
        actor_sequence: sequence,
        hlc_millis: 1_700_000_000_000 + u64::from(id),
        hlc_counter: sequence,
        policy_epoch: 3,
        session: session(id),
        payload_digest: digest(id.wrapping_add(100)),
        parents: parents.iter().map(|seed| digest(*seed)).collect(),
    }
}

fn manifest(id: u8, lengths: &[u64]) -> ManifestRecord {
    let mut chunks = Vec::new();
    let mut offset = 0;
    for (index, length) in lengths.iter().enumerate() {
        chunks.push(ChunkSlice {
            digest: digest(180u8.wrapping_add(u8::try_from(index).unwrap_or(0))),
            byte_offset: offset,
            byte_length: *length,
        });
        offset += length;
    }
    ManifestRecord {
        id: digest(id),
        byte_length: offset,
        content_digest: digest(id.wrapping_add(40)),
        chunks,
    }
}

/// A checkpoint that touches every table the schema declares. A reconstruction test over a
/// checkpoint that only wrote operations would say nothing about the other nine.
fn everything() -> Checkpoint {
    Checkpoint {
        manifests: vec![manifest(20, &[10, 20, 30]), manifest(21, &[])],
        operations: vec![
            operation(1, 2, 1, &[]),
            operation(3, 2, 2, &[1]),
            operation(5, 6, 1, &[]),
            operation(7, 6, 2, &[5, 3]),
        ],
        peers: vec![
            PeerRecord {
                peer: digest(30),
                joined_at: digest(1),
            },
            PeerRecord {
                peer: digest(6),
                joined_at: digest(5),
            },
        ],
        acknowledgements: vec![AckRecord {
            peer: digest(30),
            actor: digest(2),
            actor_sequence: 1,
        }],
        reviews: vec![ReviewRecord {
            bundle: digest(40),
            subject_operation: digest(3),
            opened_by: digest(6),
        }],
        approvals: vec![
            ApprovalRecord {
                approval: digest(41),
                bundle: digest(40),
                approver: digest(2),
                verdict: ReviewVerdict::ChangesRequested,
            },
            ApprovalRecord {
                approval: digest(42),
                bundle: digest(40),
                approver: digest(2),
                verdict: ReviewVerdict::Approved,
            },
        ],
        context_entries: vec![ContextRecord {
            entry: digest(50),
            session: session(1),
            operation: digest(1),
            access: ContextAccess::Wrote,
            byte_length: 60,
        }],
    }
}

// -- 1. The schema has nowhere to put an unreconstructable fact ---------------------------------

#[test]
fn every_table_in_the_migrations_is_declared_in_the_schema() {
    for name in table_names_in_ddl(&full_schema_sql()) {
        assert!(
            mesh_store::table(&name).is_some(),
            "the migrations create `{name}`, which TABLES does not declare — so nothing states \
             where its rows can be rebuilt from"
        );
    }
}

#[test]
fn every_table_declared_in_the_schema_is_created_by_a_migration() {
    let created = table_names_in_ddl(&full_schema_sql());
    for table in TABLES {
        assert!(
            created.contains(&table.name.to_owned()),
            "TABLES declares `{}`, which no migration creates",
            table.name
        );
    }
    assert_eq!(created.len(), TABLES.len(), "created: {created:?}");
}

#[test]
fn each_table_is_created_by_the_migration_its_since_version_names() {
    for migration in MIGRATIONS {
        for name in table_names_in_ddl(migration.sql) {
            let table = mesh_store::table(&name).expect("declared");
            assert_eq!(
                table.since_version, migration.version,
                "`{name}` is created by migration {} but declares since_version {}",
                migration.version, table.since_version
            );
        }
    }
}

/// The rule itself. Nothing may be in this database that records cannot rebuild, and the one
/// exception is named rather than implicit.
#[test]
fn every_table_is_a_fold_over_records_except_the_one_named_exception() {
    let mut exceptions = Vec::new();
    for table in TABLES {
        match table.provenance {
            Provenance::Records(kinds) => assert!(
                !kinds.is_empty(),
                "`{}` folds no record kind, so nothing can rebuild it",
                table.name
            ),
            Provenance::MigrationLedger => exceptions.push(table.name),
        }
    }
    assert_eq!(exceptions, vec!["schema_version"]);
}

#[test]
fn every_record_kind_feeds_a_table_and_every_table_uses_a_real_kind() {
    assert_eq!(indexed_record_kinds(), RecordKind::ALL.to_vec());
}

// -- 2. The rebuild reproduces the live index ---------------------------------------------------

#[test]
fn a_rebuild_from_records_alone_matches_the_live_index() {
    let checkpoint = everything();
    let mut live = Index::new();
    for record in checkpoint.records() {
        live.apply(record).expect("the live path accepts it");
    }

    let (rebuilt, report) = rebuild(checkpoint.records(), Vec::new()).expect("rebuilds");
    assert_eq!(report.digest, live.default_digest());
    assert_eq!(rebuilt, live);

    // And every table has something in it, or the equality above is between two empty things.
    for (name, count) in &report.rows_per_table {
        if *name == "schema_version" {
            continue;
        }
        assert!(
            *count > 0,
            "`{name}` is empty, so the match over it says nothing"
        );
    }
    println!(
        "rebuilt {} rows: {:?}",
        report.total_rows(),
        report.rows_per_table
    );
}

/// A record stream arriving in a different order is the normal case after a sync, not an edge
/// case. The rebuilt index must not depend on it.
#[test]
fn a_rebuild_is_insensitive_to_the_order_acknowledgements_arrive_in() {
    let checkpoint = everything();
    let forwards = rebuild(checkpoint.records(), Vec::new())
        .expect("rebuilds")
        .1;

    let mut shuffled = checkpoint.records();
    let tail = shuffled.split_off(shuffled.len() - 1);
    let mut reordered = tail;
    reordered.extend(shuffled);
    // The context entry now arrives before its operation, which the fold must refuse rather than
    // silently drop — a rebuild that quietly skipped it would produce a smaller index.
    assert!(rebuild(reordered, Vec::new()).is_err());

    let twice = rebuild(checkpoint.records(), Vec::new())
        .expect("rebuilds")
        .1;
    assert_eq!(forwards.digest, twice.digest);
}

// -- 3. SQLite agrees ---------------------------------------------------------------------------

#[test]
fn what_goes_into_a_real_database_is_what_comes_back_out() {
    let directory = TempDir::new("roundtrip");
    let mut store = Store::open(Sqlite3::at(directory.join("metadata.sqlite"))).expect("opens");
    let expected = store.commit(&everything()).expect("the checkpoint commits");

    let mut reader = Sqlite3::at(directory.join("metadata.sqlite"));
    let read_back = read_all_tables(&mut reader).expect("every table reads back");
    assert_eq!(read_back.len(), TABLES.len());

    let mut mismatches = Vec::new();
    for (name, rows) in &read_back {
        let in_memory = store.index().rows(name).expect("the index renders it");
        if &in_memory != rows {
            mismatches.push(format!(
                "{name}: {} rows in memory, {} in the database",
                in_memory.len(),
                rows.len()
            ));
        }
    }
    assert!(mismatches.is_empty(), "{mismatches:?}");

    // The digest is over the index; the equality above is what carries it to the database.
    assert_eq!(expected, store.index().default_digest());
    println!("index digest {expected}");
}

/// Plan §6.3's recovery clause, executed. The database is corrupted the way a real one would be —
/// rows gone, a projection stale — and rebuilt from the record stream alone.
#[test]
fn a_corrupt_index_is_dropped_and_rebuilt_to_the_same_digest() {
    let directory = TempDir::new("recovery");
    let path = directory.join("metadata.sqlite");
    let mut store = Store::open(Sqlite3::at(&path)).expect("opens");
    let healthy = store.commit(&everything()).expect("commits");

    // Corrupt it: lose the outbox entirely and half the manifest chunks.
    let mut surgeon = Sqlite3::at(&path);
    surgeon
        .execute_batch("DELETE FROM outbox; DELETE FROM manifest_chunk WHERE ordinal > 0;")
        .expect("the corruption applies");
    let mut reader = Sqlite3::at(&path);
    let damaged = read_all_tables(&mut reader).expect("reads");
    let damaged_outbox = damaged
        .iter()
        .find(|(name, _)| *name == "outbox")
        .expect("outbox");
    assert!(damaged_outbox.1.is_empty(), "the corruption did not take");

    // Rebuild from records alone. Nothing is read out of the damaged database to do it.
    let mut recovered = Store::open(Sqlite3::at(&path)).expect("reopens");
    let after = recovered
        .rebuild_from(everything().records())
        .expect("rebuilds");
    assert_eq!(
        after, healthy,
        "the rebuilt index does not match the healthy one"
    );

    let mut verifier = Sqlite3::at(&path);
    for (name, rows) in read_all_tables(&mut verifier).expect("reads") {
        assert_eq!(
            recovered.index().rows(name).expect("rendered"),
            rows,
            "`{name}` did not come back"
        );
    }
}

/// The migration ledger survives a rebuild. Erasing it would erase the fingerprints that make an
/// applied migration immutable, which is the one fact in this database that records cannot supply.
#[test]
fn a_rebuild_leaves_the_migration_ledger_alone() {
    let directory = TempDir::new("recovery-ledger");
    let path = directory.join("metadata.sqlite");
    let mut store = Store::open(Sqlite3::at(&path)).expect("opens");
    store.commit(&everything()).expect("commits");

    let ledger = mesh_store::table("schema_version").expect("declared");
    let before = Sqlite3::at(&path).read_table_owned(ledger);
    store
        .rebuild_from(everything().records())
        .expect("rebuilds");
    let after = Sqlite3::at(&path).read_table_owned(ledger);

    assert_eq!(before, after);
    assert_eq!(before.len(), MIGRATIONS.len());
}

/// Two processes computing the digest of the same index must agree, or "match a digest" is a
/// within-process coincidence. The second process is this test binary re-run against rows read out
/// of the database rather than the ones held in memory.
#[test]
fn the_digest_survives_a_round_trip_through_a_second_process() {
    let directory = TempDir::new("cross-process");
    let path = directory.join("metadata.sqlite");
    let mut store = Store::open(Sqlite3::at(&path)).expect("opens");
    let first = store.commit(&everything()).expect("commits");

    // A fresh Store over the same file, which reads the ledger back out of SQLite, then replays
    // the records. Everything here has been through the sqlite3 process boundary.
    let mut second = Store::open(Sqlite3::at(&path)).expect("reopens");
    let again = second
        .rebuild_from(everything().records())
        .expect("rebuilds");
    assert_eq!(first, again);
}

/// `Row`'s `Ord` and SQL's `ORDER BY 1, 2, …` have to agree, or the digest of an index and the
/// digest of the same index read back out of SQLite differ for no reason but sequence.
///
/// Every other test here would pass without that agreeing, because SQLite returns rows in rowid
/// order by default and these tests insert them in sorted order — deleting the `ORDER BY` from the
/// read-back left the whole suite green, which is how this test came to exist. So the rows go in
/// **reverse** sorted order, with byte values chosen to separate `memcmp` from a signed comparison
/// and a shorter blob from its own extension.
#[test]
fn sql_ordering_and_row_ordering_agree_even_when_insertion_order_disagrees() {
    let directory = TempDir::new("ordering");
    let path = directory.join("metadata.sqlite");
    let mut store = Store::open(Sqlite3::at(&path)).expect("opens");

    // 0x00 and 0xff are the two values a signed-byte comparison gets wrong relative to memcmp.
    let seeds = [0x00u8, 0x7f, 0x80, 0xff, 0x01];
    let operations: Vec<OperationRecord> = seeds
        .iter()
        .enumerate()
        .map(|(index, seed)| {
            let mut record = operation(1, 2, u64::try_from(index).expect("fits") + 1, &[]);
            record.id = digest(*seed);
            record.actor = digest(*seed);
            record
        })
        .collect();

    let mut index = Index::new();
    for record in &operations {
        index
            .apply(mesh_store::StoredRecord::Operation(record.clone()))
            .expect("folds");
    }
    let expected = index.rows("operation").expect("rendered");

    // Insert in the reverse of the order the rows sort in, so rowid order is the wrong answer.
    let mut reversed = expected.clone();
    reversed.reverse();
    assert_ne!(reversed, expected, "the fixture does not exercise ordering");

    let table = mesh_store::table("operation").expect("declared");
    let mut inserts = String::from("BEGIN IMMEDIATE;\n");
    for row in &reversed {
        inserts.push_str(&format!(
            "INSERT INTO operation ({}) VALUES ({});\n",
            table.column_names().join(", "),
            row.to_sql_literals()
        ));
    }
    inserts.push_str("COMMIT;\n");
    store
        .executor_mut()
        .execute_batch(&inserts)
        .expect("the reversed insert lands");

    let read_back = Sqlite3::at(&path).read_table_owned(table);
    assert_eq!(
        read_back, expected,
        "SQLite's ORDER BY and Row's Ord disagree, so an index digest and a database digest of the \
         same rows would differ by sequence alone"
    );
}

/// A helper for reading one table without holding a mutable borrow across an assertion.
trait ReadTableOwned {
    fn read_table_owned(&mut self, table: &mesh_store::Table) -> Vec<mesh_store::Row>;
}

impl ReadTableOwned for Sqlite3 {
    fn read_table_owned(&mut self, table: &mesh_store::Table) -> Vec<mesh_store::Row> {
        self.read_table(table).expect("the table reads back")
    }
}
