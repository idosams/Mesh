//! Index reconstruction: plan §6.3's answer to a corrupt index.
//!
//! > index corruption: rebuild from immutable operations and manifests.
//!
//! There is no repair path and there is deliberately not going to be one. A repair has to decide
//! which of two disagreeing surfaces is right, and this database is never the right one — plan
//! §6.1 makes it an index and never the source of truth. So the recovery is total: throw the file
//! away, replay the records, compare digests.
//!
//! Because that is the recovery, it is also the *test*. [`rebuild`] is not a rarely-exercised
//! disaster path; `tests/reconstruction.rs` runs it against every index the other tests build, and
//! a table whose rebuild disagrees with the live path by one row is a red test rather than an
//! outage nobody sees until a crash.

use crate::digest::Digest16;
use crate::index::{FoldError, Index};
use crate::record::{RecordKind, StoredRecord};
use crate::row::Row;
use crate::schema::{Provenance, TABLES};

/// What a rebuild did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RebuildReport {
    /// How many records were replayed.
    pub records_replayed: usize,
    /// How many rows each table ended up with, in [`TABLES`] order.
    pub rows_per_table: Vec<(&'static str, usize)>,
    /// The digest of the rebuilt index.
    pub digest: Digest16,
}

impl RebuildReport {
    /// The total number of rows across every table.
    #[must_use]
    pub fn total_rows(&self) -> usize {
        self.rows_per_table.iter().map(|(_, count)| count).sum()
    }
}

/// Replay immutable records into a fresh index.
///
/// `ledger_rows` carries the migration ledger, which is the one table that is not a fold over
/// records — see [`Provenance::MigrationLedger`]. It is a parameter rather than a hidden default
/// so that a caller cannot forget it and get a digest that silently differs from the live one.
///
/// # Errors
///
/// [`FoldError`] when a record contradicts one already replayed, or refers to something the stream
/// has not supplied.
pub fn rebuild<I>(records: I, ledger_rows: Vec<Row>) -> Result<(Index, RebuildReport), FoldError>
where
    I: IntoIterator<Item = StoredRecord>,
{
    let mut index = Index::new();
    index.set_ledger_rows(ledger_rows);

    let mut records_replayed = 0;
    for record in records {
        index.apply(record)?;
        records_replayed += 1;
    }

    let rows_per_table = TABLES
        .iter()
        .map(|table| {
            (
                table.name,
                index.rows(table.name).map_or(0, |rows| rows.len()),
            )
        })
        .collect();

    let digest = index.default_digest();
    Ok((
        index,
        RebuildReport {
            records_replayed,
            rows_per_table,
            digest,
        },
    ))
}

/// Every record kind some table folds, in [`RecordKind::ALL`] order.
///
/// A kind that appears here but is never produced is a record nobody writes; a kind that appears
/// in [`RecordKind::ALL`] but not here feeds no table and is dead weight. `tests/reconstruction.rs`
/// holds the two lists against each other, which is how a record kind cannot be added without a
/// table that uses it.
#[must_use]
pub fn indexed_record_kinds() -> Vec<RecordKind> {
    let mut kinds: Vec<RecordKind> = TABLES
        .iter()
        .flat_map(|table| table.provenance.record_kinds().iter().copied())
        .collect();
    kinds.sort();
    kinds.dedup();
    kinds
}

/// The tables a given record kind contributes to.
#[must_use]
pub fn tables_fed_by(kind: RecordKind) -> Vec<&'static str> {
    TABLES
        .iter()
        .filter(|table| table.provenance.record_kinds().contains(&kind))
        .map(|table| table.name)
        .collect()
}

/// The tables that are not a fold over records at all.
#[must_use]
pub fn tables_outside_the_fold() -> Vec<&'static str> {
    TABLES
        .iter()
        .filter(|table| table.provenance == Provenance::MigrationLedger)
        .map(|table| table.name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::RecordDigest;
    use crate::index::no_session;
    use crate::record::{AckRecord, OperationRecord, PeerRecord};
    use crate::row::Value;

    fn digest(seed: u8) -> RecordDigest {
        RecordDigest::from_bytes([seed; 32])
    }

    fn operation(id: u8, actor: u8, sequence: u64) -> StoredRecord {
        StoredRecord::Operation(OperationRecord {
            id: digest(id),
            actor: digest(actor),
            actor_sequence: sequence,
            hlc_millis: 7,
            hlc_counter: 0,
            policy_epoch: 1,
            session: no_session(),
            payload_digest: digest(id.wrapping_add(100)),
            parents: Vec::new(),
        })
    }

    fn stream() -> Vec<StoredRecord> {
        vec![
            operation(1, 2, 1),
            operation(3, 2, 2),
            StoredRecord::Peer(PeerRecord {
                peer: digest(7),
                joined_at: digest(1),
            }),
            StoredRecord::Acknowledgement(AckRecord {
                peer: digest(7),
                actor: digest(2),
                actor_sequence: 1,
            }),
        ]
    }

    #[test]
    fn a_rebuild_replays_every_record() {
        let (_, report) = rebuild(stream(), Vec::new()).expect("rebuilds");
        assert_eq!(report.records_replayed, 4);
    }

    /// The load-bearing equality: the live path and a from-scratch replay produce one index.
    #[test]
    fn a_rebuild_matches_the_index_the_live_path_built() {
        let mut live = Index::new();
        for record in stream() {
            live.apply(record).expect("applies");
        }
        let (rebuilt, report) = rebuild(stream(), Vec::new()).expect("rebuilds");
        assert_eq!(report.digest, live.default_digest());
        assert_eq!(rebuilt, live);
    }

    #[test]
    fn a_rebuild_reports_every_table_even_the_empty_ones() {
        let (_, report) = rebuild(stream(), Vec::new()).expect("rebuilds");
        assert_eq!(report.rows_per_table.len(), TABLES.len());
        let names: Vec<&str> = report
            .rows_per_table
            .iter()
            .map(|(name, _)| *name)
            .collect();
        let declared: Vec<&str> = TABLES.iter().map(|table| table.name).collect();
        assert_eq!(names, declared);
    }

    #[test]
    fn the_ledger_rows_reach_the_rebuilt_index() {
        let rows = vec![Row::new(vec![Value::Integer(1), Value::blob([3; 16])])];
        let (index, report) = rebuild(stream(), rows.clone()).expect("rebuilds");
        assert_eq!(index.rows("schema_version"), Some(rows));
        assert!(report.total_rows() > report.records_replayed);
    }

    #[test]
    fn a_contradictory_stream_fails_the_rebuild_rather_than_producing_half_an_index() {
        let mut broken = stream();
        broken.push(operation(9, 2, 1));
        assert!(matches!(
            rebuild(broken, Vec::new()),
            Err(FoldError::ForkedActorChain { .. })
        ));
    }

    /// Every record kind feeds at least one table, and every table's kinds are real kinds.
    #[test]
    fn the_record_kinds_and_the_tables_cover_each_other() {
        assert_eq!(indexed_record_kinds(), RecordKind::ALL.to_vec());
        for kind in RecordKind::ALL {
            assert!(
                !tables_fed_by(*kind).is_empty(),
                "{kind:?} feeds no table, so nothing reads it"
            );
        }
    }

    #[test]
    fn only_the_migration_ledger_sits_outside_the_fold() {
        assert_eq!(tables_outside_the_fold(), vec!["schema_version"]);
    }
}
