//! Durable causal admission after the receipt identity boundary.
//!
//! This module deliberately accepts [`OperationRecord`] rather than transport bytes. Callers must
//! first authenticate and identity-bind those bytes through the receipt path. Its job starts at
//! the metadata boundary: persist a valid operation even when its parent is absent, keep it out of
//! the causally-ready projection, and promote the complete closure when the parent arrives.

use core::fmt;
use std::collections::BTreeSet;

use mesh_store::{
    journal_records, Checkpoint, FoldError, OperationRecord, RecordDigest, RecordJournal,
    SqlExecutor, Store, StoreError, StoredRecord,
};

/// What durable causal admission changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InboundOperationReceipt {
    /// The exact operation was already durable, so neither journal nor SQLite was touched.
    Duplicate {
        /// The operation identifier.
        operation: RecordDigest,
        /// Whether its full causal closure is now present.
        causally_ready: bool,
    },
    /// The operation is durable but cannot advance application state yet.
    Buffered {
        /// The operation identifier.
        operation: RecordDigest,
        /// Missing transitive causal leaves.
        missing: BTreeSet<RecordDigest>,
        /// Bytes appended to immutable record truth.
        journal_bytes: u64,
    },
    /// This arrival made one or more operations causally ready.
    Ready {
        /// The operation identifier received by this call.
        operation: RecordDigest,
        /// Deterministic topological order of newly-ready operation identifiers.
        newly_ready: Vec<RecordDigest>,
        /// Bytes appended to immutable record truth.
        journal_bytes: u64,
    },
}

/// Durable inbound admission stopped before source truth and its derived index agreed.
#[derive(Debug)]
pub enum InboundPersistenceError<J, D> {
    /// Causal structure, record-identity consistency, or actor-chain structure is invalid;
    /// nothing was written.
    Preflight(FoldError),
    /// The immutable journal did not durably append the operation; SQLite was untouched.
    Journal(J),
    /// Journal truth is durable, but the derived SQLite update failed or has unknown outcome.
    /// Reopen and rebuild from the journal before retrying.
    IndexAfterJournal(StoreError<D>),
}

impl<J: fmt::Display, D: fmt::Display> fmt::Display for InboundPersistenceError<J, D> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Preflight(error) => error.fmt(formatter),
            Self::Journal(error) => write!(formatter, "inbound operation journal failed: {error}"),
            Self::IndexAfterJournal(error) => write!(
                formatter,
                "inbound operation is durable in the journal but its derived index update failed: \
                 {error}; reopen and rebuild before retrying"
            ),
        }
    }
}

impl<J: fmt::Debug + fmt::Display, D: fmt::Debug + fmt::Display> std::error::Error
    for InboundPersistenceError<J, D>
{
}

/// Persist one identity-admitted operation and update causal readiness in effect once.
///
/// An identical duplicate returns before either durable surface is touched. A missing parent is
/// not an error: the operation is journalled and indexed, but excluded from actor-head/application
/// readiness. Self-parent, duplicate-parent, fork, conflict, and completed-cycle records fail the
/// cloned-index preflight before the journal. The immutable journal is written before SQLite so a
/// crash in between is repaired by ordinary rebuild.
pub fn persist_inbound_operation<E, J>(
    store: &mut Store<E>,
    journal: &mut J,
    operation: OperationRecord,
) -> Result<InboundOperationReceipt, InboundPersistenceError<J::Error, E::Error>>
where
    E: SqlExecutor,
    J: RecordJournal,
{
    if store.index().operation(&operation.id) == Some(&operation) {
        return Ok(InboundOperationReceipt::Duplicate {
            operation: operation.id,
            causally_ready: store.index().is_causally_ready(&operation.id),
        });
    }

    let before_ready: BTreeSet<RecordDigest> = store
        .index()
        .causally_ready_operations()
        .into_iter()
        .collect();
    let mut preflight = store.index().clone();
    preflight
        .apply(StoredRecord::Operation(operation.clone()))
        .map_err(InboundPersistenceError::Preflight)?;
    let missing = preflight
        .unresolved_causal_dependencies(&operation.id)
        .expect("the preflight just inserted this operation");
    let newly_ready = preflight
        .causally_ready_operations()
        .into_iter()
        .filter(|id| !before_ready.contains(id))
        .collect::<Vec<_>>();

    let record = StoredRecord::Operation(operation.clone());
    let journal_bytes =
        journal_records(journal, [&record]).map_err(InboundPersistenceError::Journal)?;
    store
        .commit(&Checkpoint {
            operations: vec![operation.clone()],
            ..Checkpoint::default()
        })
        .map_err(InboundPersistenceError::IndexAfterJournal)?;

    if missing.is_empty() {
        Ok(InboundOperationReceipt::Ready {
            operation: operation.id,
            newly_ready,
            journal_bytes,
        })
    } else {
        Ok(InboundOperationReceipt::Buffered {
            operation: operation.id,
            missing,
            journal_bytes,
        })
    }
}
