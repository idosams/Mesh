//! Durable sender-side delivery progress over mesh-store's derived outbox.
//!
//! The immutable acknowledgement record is written first. SQLite is an index over that truth, so
//! a crash between the two operations makes delivery retry, never disappear. Duplicate and lower
//! reordered receipts write nothing; the watermark is monotonic and the outbox remains the set
//! difference between authored operations and that watermark.

use core::fmt;

use mesh_store::{
    journal_records, AckRecord, Checkpoint, FoldError, RecordDigest, RecordJournal, SqlExecutor,
    Store, StoreError, StoredRecord,
};

/// What persisting one remote delivery acknowledgement changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryReceipt {
    /// Durable truth and its derived index advanced to a higher contiguous sequence.
    Advanced {
        /// Previous contiguous sequence.
        from: u64,
        /// New contiguous sequence.
        through: u64,
        /// Bytes durably appended to the immutable record journal.
        journal_bytes: u64,
        /// Operations still owed to this peer after the advance.
        remaining_operations: usize,
    },
    /// This exact or a later receipt was already durable; no journal or SQLite write occurred.
    AlreadyAcknowledged {
        /// Current contiguous sequence, which is at least the requested value.
        through: u64,
        /// Operations still owed to this peer.
        remaining_operations: usize,
    },
}

/// Durable receipt persistence stopped before both source truth and derived index agreed.
#[derive(Debug)]
pub enum DeliveryPersistenceError<J, D> {
    /// The peer is not in the durable replication set; nothing was written.
    UnknownPeer(RecordDigest),
    /// The receipt claims an actor sequence this sender does not hold; nothing was written.
    UnsendableSequence {
        /// Actor named by the receipt.
        actor: RecordDigest,
        /// Contiguous sequence claimed by the peer.
        requested: u64,
        /// Highest operation the local record index can have delivered.
        available: Option<u64>,
    },
    /// The acknowledgement contradicted the existing record fold; nothing was written.
    Preflight(FoldError),
    /// The immutable journal did not durably append the receipt; SQLite was not touched.
    Journal(J),
    /// The journal is durable, but the derived SQLite update failed or has unknown outcome.
    /// Reopen and rebuild from the journal before retrying.
    IndexAfterJournal(StoreError<D>),
}

impl<J: fmt::Display, D: fmt::Display> fmt::Display for DeliveryPersistenceError<J, D> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownPeer(peer) => {
                write!(formatter, "delivery receipt names unknown peer {peer}")
            }
            Self::UnsendableSequence {
                actor,
                requested,
                available,
            } => write!(
                formatter,
                "delivery receipt claims {actor} sequence {requested}, but this sender holds only {}",
                available.map_or_else(|| "no operation".to_owned(), |value| value.to_string())
            ),
            Self::Preflight(error) => error.fmt(formatter),
            Self::Journal(error) => {
                write!(formatter, "delivery receipt journal append failed: {error}")
            }
            Self::IndexAfterJournal(error) => write!(
                formatter,
                "delivery receipt is durable in the journal but its derived index update failed: \
                 {error}; reopen and rebuild before retrying"
            ),
        }
    }
}

impl<J: fmt::Debug + fmt::Display, D: fmt::Debug + fmt::Display> std::error::Error
    for DeliveryPersistenceError<J, D>
{
}

/// Persist one peer's highest contiguous receipt and drain the derived outbox in effect once.
///
/// The journal append happens before SQLite because the journal is source of truth and SQLite is
/// reconstructable. If the process dies after append, restart sees the acknowledgement and drains
/// the same outbox rows. If SQLite fails after append, the error explicitly requires reopen/rebuild.
/// An exact duplicate or a lower reordered retry observes the monotonic watermark and returns
/// without touching either durable surface.
pub fn persist_delivery_acknowledgement<E, J>(
    store: &mut Store<E>,
    journal: &mut J,
    peer: RecordDigest,
    actor: RecordDigest,
    through: u64,
) -> Result<DeliveryReceipt, DeliveryPersistenceError<J::Error, E::Error>>
where
    E: SqlExecutor,
    J: RecordJournal,
{
    let Some(current) = store.index().acknowledged_through(&peer, &actor) else {
        return Err(DeliveryPersistenceError::UnknownPeer(peer));
    };
    if through <= current {
        return Ok(DeliveryReceipt::AlreadyAcknowledged {
            through: current,
            remaining_operations: store.index().owed_to_peer(&peer).len(),
        });
    }
    let available = store
        .index()
        .actor_head(&actor)
        .map(|operation| operation.actor_sequence);
    if available.is_none_or(|available| through > available) {
        return Err(DeliveryPersistenceError::UnsendableSequence {
            actor,
            requested: through,
            available,
        });
    }

    let acknowledgement = AckRecord {
        peer,
        actor,
        actor_sequence: through,
    };
    let record = StoredRecord::Acknowledgement(acknowledgement);
    let mut preflight = store.index().clone();
    preflight
        .apply(record.clone())
        .map_err(DeliveryPersistenceError::Preflight)?;

    let journal_bytes =
        journal_records(journal, [&record]).map_err(DeliveryPersistenceError::Journal)?;
    store
        .commit(&Checkpoint {
            acknowledgements: vec![acknowledgement],
            ..Checkpoint::default()
        })
        .map_err(DeliveryPersistenceError::IndexAfterJournal)?;

    Ok(DeliveryReceipt::Advanced {
        from: current,
        through,
        journal_bytes,
        remaining_operations: store.index().owed_to_peer(&peer).len(),
    })
}
