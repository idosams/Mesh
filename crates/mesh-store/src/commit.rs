//! Transaction boundaries: plan §6.3's commit sequence, as SQL in a fixed order.
//!
//! Plan §6.3 numbers ten steps. Steps 1 to 4 belong to the content-addressed store — write chunk
//! bytes to temporary files, flush, verify hashes, promote — and this crate never sees them. Steps
//! 5 to 9 are this module, and they are the whole of what "atomic local metadata transactions"
//! means:
//!
//! ```text
//! 5. Begin SQLite transaction.        -> CommitStep::Begin
//! 6. Insert manifests and immutable operations. -> CommitStep::InsertImmutable
//! 7. Advance actor head.              -> CommitStep::AdvanceHeads
//! 8. Add outbox records.              -> CommitStep::FillOutbox
//! 9. Commit SQLite transaction.       -> CommitStep::Commit
//! ```
//!
//! # Why the order is a value and not a convention
//!
//! Every statement carries the [`CommitStep`] it belongs to, so "the head advances after the
//! operation exists" is something a test reads off the plan rather than something a reader infers
//! from the order of some function's body. `tests/commit_sequence.rs` asserts the step sequence is
//! non-decreasing and then executes the whole plan against a real SQLite database with foreign
//! keys on — which is the other half of the proof, because the foreign keys are what make a
//! wrongly-ordered plan fail rather than merely look wrong.
//!
//! # `BEGIN IMMEDIATE`, not `BEGIN`
//!
//! A deferred transaction takes its write lock at the first write, so two writers can both begin,
//! both read, and one then fail at its first write with `SQLITE_BUSY` — after it has already
//! decided what to write from a snapshot that is no longer current. `BEGIN IMMEDIATE` takes the
//! write lock up front, which turns that race into a wait. Under WAL this costs readers nothing;
//! that is measured in `tests/wal_concurrency.rs`, not assumed.

use crate::index::{FoldError, Index};
use crate::record::{
    AckRecord, ApprovalRecord, ContextRecord, ManifestRecord, OperationRecord, PeerRecord,
    ReviewRecord, StoredRecord,
};
use crate::row::{Row, Value};
use crate::schema::{Provenance, Table, TABLES};

/// Which of plan §6.3's steps a statement belongs to.
///
/// Ordered, so a plan's steps can be checked to be non-decreasing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CommitStep {
    /// §6.3 step 5 — begin the transaction.
    Begin,
    /// §6.3 step 6 — insert manifests and immutable operations.
    InsertImmutable,
    /// §6.3 step 7 — advance the actor head, and the peer watermarks that move with it.
    AdvanceHeads,
    /// §6.3 step 8 — add outbox records.
    FillOutbox,
    /// §6.3 step 9 — commit.
    Commit,
}

impl CommitStep {
    /// The plan §6.3 step number this maps to.
    #[must_use]
    pub const fn plan_step(self) -> u8 {
        match self {
            Self::Begin => 5,
            Self::InsertImmutable => 6,
            Self::AdvanceHeads => 7,
            Self::FillOutbox => 8,
            Self::Commit => 9,
        }
    }
}

/// One statement, and the step it belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Statement {
    /// Which §6.3 step this is part of.
    pub step: CommitStep,
    /// The SQL, complete and with every value already a literal — [`Value`] renders integers bare
    /// and blobs as `X'…'`, so there is no parameter binding and no quoting.
    pub sql: String,
}

impl Statement {
    fn new(step: CommitStep, sql: impl Into<String>) -> Self {
        Self {
            step,
            sql: sql.into(),
        }
    }
}

/// What one local save contributes to the index.
///
/// Chunk bytes are absent on purpose: plan §6.2 puts them in the content-addressed store, and
/// §6.3 steps 1 to 4 have already promoted them before this type is built. A checkpoint that
/// mentioned bytes would be a checkpoint that could be built before the bytes were durable.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Checkpoint {
    /// Manifests written by this save.
    pub manifests: Vec<ManifestRecord>,
    /// Operations written by this save.
    pub operations: Vec<OperationRecord>,
    /// Peers that joined.
    pub peers: Vec<PeerRecord>,
    /// Acknowledgements received.
    pub acknowledgements: Vec<AckRecord>,
    /// Review bundles opened.
    pub reviews: Vec<ReviewRecord>,
    /// Approvals received.
    pub approvals: Vec<ApprovalRecord>,
    /// Context ledger entries.
    pub context_entries: Vec<ContextRecord>,
}

impl Checkpoint {
    /// The records this checkpoint contributes, in an order every reference resolves in.
    ///
    /// Manifests first because nothing refers to them; operations next because everything else
    /// does; then peers, acknowledgements, reviews, approvals and context entries, each after what
    /// it names. The fold refuses a dangling reference, so getting this order wrong is a test
    /// failure rather than a half-written index.
    #[must_use]
    pub fn records(&self) -> Vec<StoredRecord> {
        let mut records = Vec::new();
        records.extend(self.manifests.iter().cloned().map(StoredRecord::Manifest));
        records.extend(self.operations.iter().cloned().map(StoredRecord::Operation));
        records.extend(self.peers.iter().copied().map(StoredRecord::Peer));
        records.extend(
            self.acknowledgements
                .iter()
                .copied()
                .map(StoredRecord::Acknowledgement),
        );
        records.extend(self.reviews.iter().copied().map(StoredRecord::Review));
        records.extend(self.approvals.iter().copied().map(StoredRecord::Approval));
        records.extend(
            self.context_entries
                .iter()
                .copied()
                .map(StoredRecord::ContextEntry),
        );
        records
    }

    /// Whether this checkpoint would write nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records().is_empty()
    }
}

/// An ordered SQL plan for one transaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitPlan {
    statements: Vec<Statement>,
}

impl CommitPlan {
    /// The statements, in execution order.
    #[must_use]
    pub fn statements(&self) -> &[Statement] {
        &self.statements
    }

    /// The SQL, joined for a batch execution.
    #[must_use]
    pub fn sql(&self) -> String {
        self.statements
            .iter()
            .map(|statement| statement.sql.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The statements belonging to one step, in execution order.
    ///
    /// This is what lets [`crate::DurableCommit`] append the transaction a step at a time without
    /// holding a second opinion about which SQL belongs to which of plan §6.3's steps. There is one
    /// definition of that, and it is [`Statement::step`].
    #[must_use]
    pub fn statements_for(&self, step: CommitStep) -> Vec<Statement> {
        self.statements
            .iter()
            .filter(|statement| statement.step == step)
            .cloned()
            .collect()
    }

    /// The step of each statement, in order.
    #[must_use]
    pub fn steps(&self) -> Vec<CommitStep> {
        self.statements
            .iter()
            .map(|statement| statement.step)
            .collect()
    }

    /// Fold a checkpoint into `before` and produce the transaction that makes a database holding
    /// `before` hold the result.
    ///
    /// # Errors
    ///
    /// [`FoldError`] when the checkpoint contradicts what the index already holds. Nothing is
    /// emitted in that case, so a rejected checkpoint leaves no partial plan to run by mistake.
    pub fn stage(before: &Index, checkpoint: &Checkpoint) -> Result<(Self, Index), FoldError> {
        let mut after = before.clone();
        for record in checkpoint.records() {
            after.apply(record)?;
        }

        let mut statements = vec![Statement::new(CommitStep::Begin, "BEGIN IMMEDIATE;")];

        for table in immutable_tables() {
            let existing = before.rows(table.name).unwrap_or_default();
            for row in added_rows(&existing, &after.rows(table.name).unwrap_or_default()) {
                statements.push(Statement::new(
                    CommitStep::InsertImmutable,
                    insert_sql(table, &row),
                ));
            }
        }

        for row in after.rows("actor_head").unwrap_or_default() {
            statements.push(Statement::new(
                CommitStep::AdvanceHeads,
                advance_head_sql(&row),
            ));
        }
        for row in after.rows("peer_watermark").unwrap_or_default() {
            statements.push(Statement::new(
                CommitStep::AdvanceHeads,
                advance_watermark_sql(&row),
            ));
        }

        for row in after.rows("outbox").unwrap_or_default() {
            statements.push(Statement::new(
                CommitStep::FillOutbox,
                format!(
                    "INSERT OR IGNORE INTO outbox (peer_id, actor_id, actor_sequence, \
                     operation_id) VALUES ({});",
                    row.to_sql_literals()
                ),
            ));
        }
        for acknowledgement in &checkpoint.acknowledgements {
            statements.push(Statement::new(
                CommitStep::FillOutbox,
                format!(
                    "DELETE FROM outbox WHERE peer_id = {} AND actor_id = {} AND actor_sequence \
                     <= {};",
                    Value::blob(acknowledgement.peer.as_bytes()).to_sql_literal(),
                    Value::blob(acknowledgement.actor.as_bytes()).to_sql_literal(),
                    Value::count(acknowledgement.actor_sequence).to_sql_literal(),
                ),
            ));
        }

        statements.push(Statement::new(CommitStep::Commit, "COMMIT;"));
        Ok((Self { statements }, after))
    }

    /// The transaction that makes an empty database hold exactly `index`.
    ///
    /// This is plan §6.3's index-corruption recovery, as SQL: every record-derived table is
    /// emptied and rewritten from the rebuilt index. `schema_version` is untouched, because the
    /// migration ledger is a fact about the file rather than about the workspace and rewriting it
    /// would erase the fingerprints that make an applied migration immutable.
    #[must_use]
    pub fn full_rebuild(index: &Index) -> Self {
        let mut statements = vec![Statement::new(CommitStep::Begin, "BEGIN IMMEDIATE;")];

        for table in record_derived_tables().rev() {
            statements.push(Statement::new(
                CommitStep::InsertImmutable,
                format!("DELETE FROM {};", table.name),
            ));
        }
        for table in record_derived_tables() {
            let step = step_for(table.name);
            for row in index.rows(table.name).unwrap_or_default() {
                statements.push(Statement::new(step, insert_sql(table, &row)));
            }
        }

        statements.push(Statement::new(CommitStep::Commit, "COMMIT;"));
        Self { statements }
    }
}

/// The tables that hold immutable rows: everything a record produces once and never revises.
fn immutable_tables() -> impl Iterator<Item = &'static Table> {
    record_derived_tables().filter(|table| !is_derived_projection(table.name))
}

/// Every table that is a fold over records, in [`TABLES`] order.
fn record_derived_tables() -> impl DoubleEndedIterator<Item = &'static Table> {
    TABLES
        .iter()
        .filter(|table| table.provenance != Provenance::MigrationLedger)
}

/// Whether a table is a moving projection rather than an append-only one. These three are the
/// tables whose rows change as later records arrive, which is why they are upserted and the rest
/// are inserted.
fn is_derived_projection(name: &str) -> bool {
    matches!(name, "actor_head" | "peer_watermark" | "outbox")
}

fn step_for(name: &str) -> CommitStep {
    match name {
        "actor_head" | "peer_watermark" => CommitStep::AdvanceHeads,
        "outbox" => CommitStep::FillOutbox,
        _ => CommitStep::InsertImmutable,
    }
}

fn insert_sql(table: &Table, row: &Row) -> String {
    format!(
        "INSERT INTO {} ({}) VALUES ({});",
        table.name,
        table.column_names().join(", "),
        row.to_sql_literals()
    )
}

/// A head never moves backwards, which the `WHERE` clause on the conflict branch is what enforces.
fn advance_head_sql(row: &Row) -> String {
    format!(
        "INSERT INTO actor_head (actor_id, operation_id, actor_sequence) VALUES ({}) \
         ON CONFLICT(actor_id) DO UPDATE SET operation_id = excluded.operation_id, \
         actor_sequence = excluded.actor_sequence \
         WHERE excluded.actor_sequence > actor_head.actor_sequence;",
        row.to_sql_literals()
    )
}

/// A watermark never moves backwards either, for the same reason and by the same means.
fn advance_watermark_sql(row: &Row) -> String {
    format!(
        "INSERT INTO peer_watermark (peer_id, actor_id, actor_sequence) VALUES ({}) \
         ON CONFLICT(peer_id, actor_id) DO UPDATE SET actor_sequence = excluded.actor_sequence \
         WHERE excluded.actor_sequence > peer_watermark.actor_sequence;",
        row.to_sql_literals()
    )
}

/// The rows in `after` that are not already in `before`.
fn added_rows(before: &[Row], after: &[Row]) -> Vec<Row> {
    after
        .iter()
        .filter(|row| !before.contains(row))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::RecordDigest;
    use crate::index::no_session;

    fn digest(seed: u8) -> RecordDigest {
        RecordDigest::from_bytes([seed; 32])
    }

    fn operation(id: u8, actor: u8, sequence: u64) -> OperationRecord {
        OperationRecord {
            id: digest(id),
            actor: digest(actor),
            actor_sequence: sequence,
            hlc_millis: 11,
            hlc_counter: 0,
            policy_epoch: 1,
            session: no_session(),
            payload_digest: digest(id.wrapping_add(100)),
            parents: Vec::new(),
        }
    }

    fn checkpoint() -> Checkpoint {
        Checkpoint {
            operations: vec![operation(1, 2, 1)],
            ..Checkpoint::default()
        }
    }

    #[test]
    fn a_plan_begins_and_commits() {
        let (plan, _) = CommitPlan::stage(&Index::new(), &checkpoint()).expect("stages");
        let statements = plan.statements();
        assert_eq!(statements.first().unwrap().sql, "BEGIN IMMEDIATE;");
        assert_eq!(statements.last().unwrap().sql, "COMMIT;");
    }

    /// The plan §6.3 ordering, read off the plan rather than inferred from the code.
    #[test]
    fn the_steps_never_go_backwards() {
        let (plan, _) = CommitPlan::stage(&Index::new(), &checkpoint()).expect("stages");
        let steps = plan.steps();
        assert!(steps.windows(2).all(|pair| pair[0] <= pair[1]), "{steps:?}");
    }

    #[test]
    fn the_steps_map_onto_plan_6_3() {
        assert_eq!(CommitStep::Begin.plan_step(), 5);
        assert_eq!(CommitStep::InsertImmutable.plan_step(), 6);
        assert_eq!(CommitStep::AdvanceHeads.plan_step(), 7);
        assert_eq!(CommitStep::FillOutbox.plan_step(), 8);
        assert_eq!(CommitStep::Commit.plan_step(), 9);
    }

    /// Step 7 comes after step 6 for a reason the schema enforces: `actor_head.operation_id` is a
    /// foreign key into `operation`.
    #[test]
    fn the_head_advances_after_the_operation_is_inserted() {
        let (plan, _) = CommitPlan::stage(&Index::new(), &checkpoint()).expect("stages");
        let insert = plan
            .statements()
            .iter()
            .position(|statement| statement.sql.starts_with("INSERT INTO operation "))
            .expect("the operation is inserted");
        let advance = plan
            .statements()
            .iter()
            .position(|statement| statement.sql.contains("INSERT INTO actor_head"))
            .expect("the head advances");
        assert!(insert < advance);
    }

    #[test]
    fn an_empty_checkpoint_still_produces_a_transaction_and_nothing_else() {
        let (plan, _) = CommitPlan::stage(&Index::new(), &Checkpoint::default()).expect("stages");
        assert_eq!(plan.statements().len(), 2);
        assert!(Checkpoint::default().is_empty());
    }

    #[test]
    fn a_rejected_checkpoint_produces_no_plan() {
        let mut index = Index::new();
        index
            .apply(StoredRecord::Operation(operation(1, 2, 1)))
            .expect("applies");
        let clashing = Checkpoint {
            operations: vec![operation(9, 2, 1)],
            ..Checkpoint::default()
        };
        assert!(matches!(
            CommitPlan::stage(&index, &clashing),
            Err(FoldError::ForkedActorChain { .. })
        ));
    }

    /// A second checkpoint must not re-insert what the first already wrote, or every save after
    /// the first would fail on a primary-key conflict.
    #[test]
    fn a_second_checkpoint_inserts_only_what_is_new() {
        let (_, after) = CommitPlan::stage(&Index::new(), &checkpoint()).expect("stages");
        let second = Checkpoint {
            operations: vec![operation(3, 2, 2)],
            ..Checkpoint::default()
        };
        let (plan, _) = CommitPlan::stage(&after, &second).expect("stages");
        let inserts: Vec<&Statement> = plan
            .statements()
            .iter()
            .filter(|statement| statement.sql.starts_with("INSERT INTO operation "))
            .collect();
        assert_eq!(inserts.len(), 1);
        assert!(inserts[0].sql.contains(&digest(3).to_hex()));
    }

    #[test]
    fn a_full_rebuild_empties_before_it_writes() {
        let mut index = Index::new();
        index
            .apply(StoredRecord::Operation(operation(1, 2, 1)))
            .expect("applies");
        let plan = CommitPlan::full_rebuild(&index);
        let first_delete = plan
            .statements()
            .iter()
            .position(|statement| statement.sql.starts_with("DELETE FROM"))
            .expect("deletes");
        let first_insert = plan
            .statements()
            .iter()
            .position(|statement| statement.sql.starts_with("INSERT INTO"))
            .expect("inserts");
        assert!(first_delete < first_insert);
    }

    /// The migration ledger is not part of a rebuild. Erasing it would erase the fingerprints that
    /// make an applied migration immutable.
    #[test]
    fn a_full_rebuild_never_touches_the_migration_ledger() {
        let plan = CommitPlan::full_rebuild(&Index::new());
        assert!(!plan.sql().contains("schema_version"));
    }

    #[test]
    fn a_full_rebuild_deletes_every_record_derived_table() {
        let plan = CommitPlan::full_rebuild(&Index::new());
        for table in record_derived_tables() {
            assert!(
                plan.sql().contains(&format!("DELETE FROM {};", table.name)),
                "{} is never emptied",
                table.name
            );
        }
    }

    /// Deletes run child-table-first, so a foreign key never blocks the emptying.
    #[test]
    fn a_full_rebuild_deletes_children_before_parents() {
        let plan = CommitPlan::full_rebuild(&Index::new());
        let sql = plan.sql();
        let parent = sql.find("DELETE FROM operation;").expect("operation");
        let child = sql
            .find("DELETE FROM operation_parent;")
            .expect("operation_parent");
        assert!(child < parent);
    }

    #[test]
    fn an_acknowledgement_clears_the_outbox_rows_it_covers() {
        let mut index = Index::new();
        for record in [
            StoredRecord::Operation(operation(1, 2, 1)),
            StoredRecord::Peer(PeerRecord {
                peer: digest(7),
                joined_at: digest(1),
            }),
        ] {
            index.apply(record).expect("applies");
        }
        let acknowledging = Checkpoint {
            acknowledgements: vec![AckRecord {
                peer: digest(7),
                actor: digest(2),
                actor_sequence: 1,
            }],
            ..Checkpoint::default()
        };
        let (plan, after) = CommitPlan::stage(&index, &acknowledging).expect("stages");
        assert!(plan.sql().contains("DELETE FROM outbox WHERE peer_id"));
        assert!(after.rows("outbox").unwrap().is_empty());
    }

    #[test]
    fn every_generated_statement_ends_in_a_semicolon() {
        let (plan, _) = CommitPlan::stage(&Index::new(), &checkpoint()).expect("stages");
        for statement in plan.statements() {
            assert!(statement.sql.trim_end().ends_with(';'), "{}", statement.sql);
        }
    }
}
