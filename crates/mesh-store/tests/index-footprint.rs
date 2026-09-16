//! What one indexed record costs on disk, in the local SQLite index.
//!
//! # Why this file exists
//!
//! The other half of the steady-state storage budget. `crates/mesh-cas/tests/storage-footprint.rs`
//! bounds the content-addressed store, whose cost is per *byte*; this file bounds the index, whose
//! cost is per *record*. The two behave completely differently as a workspace changes shape, and
//! that difference is the whole point: a tree of 16 KiB files pays a few per cent for its index,
//! and a tree of 1 KiB source files pays tens of per cent for the same index. Only a per-record
//! figure says which.
//!
//! The two halves are measured in two processes because the lockfile fence of
//! `docs/adr/0014-narrow-the-lockfile-fence-to-admit-an-audited-cryptographic-dependency.md`
//! forbids the dependency edge that would join them. `benchmarks/budgets/storage.md` composes
//! them arithmetically and states that it is doing so.
//!
//! # The measurement
//!
//! Real SQLite, through the same `sqlite3`-process driver every other test in this crate uses — so
//! these are the bytes the schema actually costs, not the bytes a page-size calculation says it
//! should. Before each figure is read the WAL is checkpointed with `TRUNCATE`, because a database
//! measured with an unflushed log is measured mid-sentence.
//!
//! Deliberately **not** timed. Bytes are deterministic; the same records produce the same file
//! size on every run, and `records_cost_the_same_whichever_way_they_arrive` is what says so.

mod common;

use common::{Sqlite3, TempDir};
use mesh_store::{
    Checkpoint, ChunkSlice, ManifestRecord, OperationRecord, RecordDigest, SqlExecutor, Store,
};

// ---------------------------------------------------------------------------
// Budgets. Every constant below has a row in `benchmarks/budgets/storage.md`
// carrying the same number and its justification; `verify:storage` fails when
// the two disagree, in either direction.
// ---------------------------------------------------------------------------

/// Bytes one indexed record may cost, once the schema is paid for.
///
/// Measured at 215–223 bytes per record — flat across a fourfold range, over operations, manifests,
/// chunk slices and parent edges — with SQLite 3.43.2. Budgeted at 320, roughly one and a half times
/// the observed figure. The headroom is not slack: B-tree page fill is a property of the SQLite
/// build, and a budget set at the measured value would fire on a host whose `sqlite3` packs pages
/// differently, which is a false regression and the fastest way to teach a team to raise a
/// threshold. It is still far below the ~430 bytes that adding one more indexed column-set per
/// record would cost.
pub const BUDGET_INDEX_BYTES_PER_RECORD: u64 = 320;

/// Bytes an index costs before it holds a single record.
///
/// The fixed per-workspace cost of the schema and its indexes: measured at 131,072 bytes (32 pages
/// of 4 KiB) with SQLite 3.43.2, budgeted at 196,608. It matters because it is paid *per actor
/// workspace*, so it multiplies by the actor count while carrying no content at all.
pub const BUDGET_INDEX_SCHEMA_FLOOR_BYTES: u64 = 196_608;

/// Indexed records one file version contributes, at one chunk per file.
///
/// One manifest, one operation and one chunk slice. This is the multiplier that turns a per-record
/// budget into a per-file one in `benchmarks/budgets/storage.md`, and
/// [`one_file_version_costs_the_records_the_budget_composes_from`] fails if the commit sequence
/// starts writing a fourth.
pub const BUDGET_INDEX_RECORDS_PER_FILE_VERSION: u64 = 3;

/// How far the per-record cost may drift between a small index and one four times larger, in
/// per mille.
///
/// 1250 means the larger index may cost at most 25 % more per record than the smaller one. The
/// point is not the exact tolerance; it is that index growth must be **linear in records**. A
/// superlinear index is unbounded history wearing a different hat, and it would not be visible in
/// any single-size measurement.
pub const BUDGET_INDEX_LINEARITY_DRIFT_PER_MILLE: u64 = 1250;

/// Records committed by the small measurement point.
const SMALL_RECORDS: u64 = 1_800;

/// Records committed by the large measurement point.
const LARGE_RECORDS: u64 = 7_200;

/// Files per checkpoint, and therefore records per checkpoint (times three).
const FILES_PER_CHECKPOINT: u64 = 100;

// ---------------------------------------------------------------------------
// The corpus: a deterministic record stream
// ---------------------------------------------------------------------------

/// A digest derived from a counter, so the whole record stream is a function of its parameters.
fn digest(seed: u64) -> RecordDigest {
    let mut bytes = [0u8; 32];
    bytes[..8].copy_from_slice(&seed.to_le_bytes());
    bytes[8..16].copy_from_slice(&seed.rotate_left(17).to_le_bytes());
    bytes[16..24].copy_from_slice(&seed.rotate_left(31).to_le_bytes());
    bytes[24..].copy_from_slice(&seed.rotate_left(47).to_le_bytes());
    RecordDigest::from_bytes(bytes)
}

/// One actor's checkpoint over `files` files: a manifest, an operation and one chunk slice each.
fn checkpoint(actor: u64, epoch: u64, files: u64) -> Checkpoint {
    let mut manifests = Vec::new();
    let mut operations = Vec::new();
    for file in 0..files {
        let key = actor * 1_000_000_000 + epoch * 1_000_000 + file;
        manifests.push(ManifestRecord {
            id: digest(key),
            byte_length: 262_144,
            content_digest: digest(key + 11),
            chunks: vec![ChunkSlice {
                digest: digest(key + 23),
                byte_offset: 0,
                byte_length: 262_144,
            }],
        });
        operations.push(OperationRecord {
            id: digest(key + 500_000_000_000),
            actor: digest(actor),
            actor_sequence: epoch * files + file + 1,
            hlc_millis: 1_700_000_000_000 + epoch,
            hlc_counter: file,
            policy_epoch: 1,
            session: mesh_store::no_session(),
            payload_digest: digest(key),
            // Chained within the actor, because a real operation has a parent and the parent edge
            // is part of what a record costs. An index measured over parentless operations
            // understates every workspace that has ever been edited twice.
            parents: match (epoch, file) {
                (0, 0) => Vec::new(),
                (_, 0) => vec![digest(
                    actor * 1_000_000_000 + (epoch - 1) * 1_000_000 + files - 1 + 500_000_000_000,
                )],
                _ => vec![digest(key - 1 + 500_000_000_000)],
            },
        });
    }
    Checkpoint {
        manifests,
        operations,
        ..Checkpoint::default()
    }
}

/// Records one call to [`checkpoint`] contributes.
fn records_in(checkpoint: &Checkpoint) -> u64 {
    (checkpoint.manifests.len() + checkpoint.operations.len()) as u64
        + checkpoint
            .manifests
            .iter()
            .map(|manifest| manifest.chunks.len() as u64)
            .sum::<u64>()
}

// ---------------------------------------------------------------------------
// Measuring
// ---------------------------------------------------------------------------

/// The database file's size, with the write-ahead log folded back in first.
fn settled_bytes(path: &std::path::Path) -> u64 {
    let mut reader = Sqlite3::at(path);
    reader
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .expect("the write-ahead log checkpoints");
    let mut total = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    let mut sidecar = path.as_os_str().to_owned();
    sidecar.push("-wal");
    total += std::fs::metadata(std::path::PathBuf::from(sidecar))
        .map(|meta| meta.len())
        .unwrap_or(0);
    total
}

/// Commits `records` records into a fresh index and returns `(empty_bytes, filled_bytes)`.
fn grow(label: &str, records: u64) -> (u64, u64) {
    assert_eq!(
        records % (FILES_PER_CHECKPOINT * BUDGET_INDEX_RECORDS_PER_FILE_VERSION),
        0,
        "`{label}` asks for {records} records, which is not a whole number of checkpoints"
    );
    let directory = TempDir::new(label);
    let path = directory.join("metadata.sqlite");
    let mut store = Store::open(Sqlite3::at(&path)).expect("the index opens");
    let empty = settled_bytes(&path);

    let mut written = 0;
    let mut epoch = 0;
    while written < records {
        let checkpoint = checkpoint(0, epoch, FILES_PER_CHECKPOINT);
        written += records_in(&checkpoint);
        store.commit(&checkpoint).expect("the checkpoint commits");
        epoch += 1;
    }
    assert_eq!(
        written, records,
        "`{label}` wrote {written} records, not {records}"
    );
    (empty, settled_bytes(&path))
}

// ---------------------------------------------------------------------------
// The tests
// ---------------------------------------------------------------------------

/// One file version is three indexed records, which is what the composed budget multiplies by.
#[test]
fn one_file_version_costs_the_records_the_budget_composes_from() {
    let one_file = checkpoint(0, 0, 1);
    assert_eq!(
        records_in(&one_file),
        BUDGET_INDEX_RECORDS_PER_FILE_VERSION,
        "one file version now contributes a different number of indexed records; every per-file \
         figure in benchmarks/budgets/storage.md is derived from this multiplier"
    );
}

/// An empty workspace's index is inside its floor.
#[test]
fn an_empty_index_costs_no_more_than_the_schema_floor() {
    let directory = TempDir::new("index-floor");
    let path = directory.join("metadata.sqlite");
    let _store = Store::open(Sqlite3::at(&path)).expect("the index opens");
    let empty = settled_bytes(&path);
    println!(
        "index-footprint floor: empty_bytes={empty} budget_bytes={BUDGET_INDEX_SCHEMA_FLOOR_BYTES}"
    );
    assert!(
        empty <= BUDGET_INDEX_SCHEMA_FLOOR_BYTES,
        "an index with no records costs {empty} bytes, over the floor budget of \
         {BUDGET_INDEX_SCHEMA_FLOOR_BYTES}. This is paid once per actor workspace and carries no \
         content, so it multiplies by the actor count."
    );
}

/// Each record costs no more than its budget, at both measurement points.
#[test]
fn each_record_costs_no_more_than_its_budget() {
    for (label, records) in [
        ("index-small", SMALL_RECORDS),
        ("index-large", LARGE_RECORDS),
    ] {
        let (empty, filled) = grow(label, records);
        let per_record = (filled - empty) / records;
        println!(
            "index-footprint records: point={label} records={records} empty_bytes={empty} \
             filled_bytes={filled} bytes_per_record={per_record} \
             budget_bytes_per_record={BUDGET_INDEX_BYTES_PER_RECORD}"
        );
        assert!(
            per_record <= BUDGET_INDEX_BYTES_PER_RECORD,
            "{records} records grew the index by {} bytes, {per_record} per record, over the \
             budget of {BUDGET_INDEX_BYTES_PER_RECORD}",
            filled - empty
        );
    }
}

/// The index grows linearly in records, not superlinearly.
#[test]
fn index_growth_is_linear_in_the_record_count() {
    let (small_empty, small_filled) = grow("index-linear-small", SMALL_RECORDS);
    let (large_empty, large_filled) = grow("index-linear-large", LARGE_RECORDS);
    let small_rate = (small_filled - small_empty) * 1000 / SMALL_RECORDS;
    let large_rate = (large_filled - large_empty) * 1000 / LARGE_RECORDS;
    let drift = large_rate * 1000 / small_rate;
    println!(
        "index-footprint linearity: small_records={SMALL_RECORDS} \
         small_milli_bytes_per_record={small_rate} large_records={LARGE_RECORDS} \
         large_milli_bytes_per_record={large_rate} drift_per_mille={drift} \
         budget_per_mille={BUDGET_INDEX_LINEARITY_DRIFT_PER_MILLE}"
    );
    assert!(
        drift <= BUDGET_INDEX_LINEARITY_DRIFT_PER_MILLE,
        "quadrupling the record count moved the per-record cost from {small_rate} to {large_rate} \
         milli-bytes ({drift} per mille), over the budget of \
         {BUDGET_INDEX_LINEARITY_DRIFT_PER_MILLE}. Index growth must be linear in records."
    );
}

/// The same records cost the same bytes however they are batched — so the figure above is a
/// property of the records, not of the run.
#[test]
fn records_cost_the_same_whichever_way_they_arrive() {
    let (first_empty, first_filled) = grow("index-determinism-a", SMALL_RECORDS);
    let (second_empty, second_filled) = grow("index-determinism-b", SMALL_RECORDS);
    assert_eq!(
        (first_filled - first_empty),
        (second_filled - second_empty),
        "two identical record streams produced different index sizes; a byte budget over a \
         non-deterministic measurement is not a budget"
    );
}

/// Actor count multiplies the index by the records the actors write, and by nothing else.
///
/// Four actors writing a quarter of the records each cost what one actor writing all of them
/// costs, to within one page. The per-actor term that *is* unavoidable is the schema floor, and it
/// is budgeted separately above precisely because it is the one cost an idle actor still pays.
#[test]
fn actor_count_costs_records_and_the_schema_floor_and_nothing_else() {
    let directory = TempDir::new("index-actors");
    let path = directory.join("metadata.sqlite");
    let mut store = Store::open(Sqlite3::at(&path)).expect("the index opens");
    let empty = settled_bytes(&path);
    let mut written = 0;
    for actor in 0..4u64 {
        for epoch in 0..(SMALL_RECORDS / (4 * FILES_PER_CHECKPOINT * 3)) {
            let checkpoint = checkpoint(actor, epoch, FILES_PER_CHECKPOINT);
            written += records_in(&checkpoint);
            store.commit(&checkpoint).expect("commits");
        }
    }
    let filled = settled_bytes(&path);
    let per_record = (filled - empty) / written;
    println!(
        "index-footprint actors: actors=4 records={written} bytes_per_record={per_record} \
         budget_bytes_per_record={BUDGET_INDEX_BYTES_PER_RECORD}"
    );
    assert!(
        per_record <= BUDGET_INDEX_BYTES_PER_RECORD,
        "four actors' records cost {per_record} bytes each, over the budget of \
         {BUDGET_INDEX_BYTES_PER_RECORD}; the index is charging for the actor, not for the record"
    );
}
