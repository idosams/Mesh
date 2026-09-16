//! The event feed: what the daemon pushes to a subscribed client, and how far each one has read.
//!
//! # Why a cursor and not a channel
//!
//! Every entry has a sequence number, and a subscriber is a number rather than a queue. That is
//! what makes a reconnection cheap and honest: a client that comes back says which sequence it had
//! and gets everything after it that is still held — or is told plainly how many it missed, in
//! [`EventBacklog::dropped`], instead of being handed a gap it cannot see.
//!
//! A per-subscriber queue would need the daemon to know when a client is gone before it can stop
//! growing, and a local socket cannot tell "slow" from "gone" quickly. A bounded shared ring plus a
//! cursor has one failure mode, it is measurable, and it is reported.
//!
//! # The sequence number is not an ordering of anybody's work
//!
//! It orders *notifications on this socket* and nothing else. Ordering of work in this program is
//! `lamport → event id → content hash`; a feed sequence establishes none of those and no client
//! may treat it as if it did.

use core::fmt;
use std::sync::Mutex;

use crate::ipc::json::Json;

/// What happened. One variant per thing the daemon actually observes — nothing here is a
/// placeholder for a fact some later crate will produce.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// The daemon finished starting and is serving requests.
    Serving,
    /// A workspace was opened, and this is how many records it held.
    WorkspaceOpened {
        /// How many whole records were on disk.
        records: u64,
    },
    /// A workspace could not be opened.
    WorkspaceRefused {
        /// The machine code from `crate::workspace::OpenFailure::code`.
        code: String,
    },
    /// A process-lost private save could not be verified against rebuilt durable truth.
    CheckpointRecoveryNeedsAttention,
    /// The daemon is shutting down. The last entry any subscriber sees.
    Stopping,
}

impl EventKind {
    /// The stable wire word for this kind.
    #[must_use]
    pub const fn word(&self) -> &'static str {
        match self {
            Self::Serving => "serving",
            Self::WorkspaceOpened { .. } => "workspace-opened",
            Self::WorkspaceRefused { .. } => "workspace-refused",
            Self::CheckpointRecoveryNeedsAttention => "checkpoint-recovery-needs-attention",
            Self::Stopping => "stopping",
        }
    }

    /// The kind's own fields, as the `value` of an event line.
    #[must_use]
    pub fn to_json(&self) -> Json {
        match self {
            Self::Serving | Self::Stopping | Self::CheckpointRecoveryNeedsAttention => {
                Json::empty_object()
            }
            Self::WorkspaceOpened { records } => {
                Json::object([("records", Json::Number(*records))])
            }
            Self::WorkspaceRefused { code } => Json::object([("code", Json::text(code.clone()))]),
        }
    }
}

impl fmt::Display for EventKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.word())
    }
}

/// One entry in the feed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DaemonEvent {
    /// Where this entry sits in the feed. Starts at 1; 0 means "nothing yet".
    pub sequence: u64,
    /// What happened.
    pub kind: EventKind,
}

/// What a subscriber gets when it asks for everything after a cursor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventBacklog {
    /// The entries still held that come after the cursor, oldest first.
    pub entries: Vec<DaemonEvent>,
    /// How many entries after the cursor are gone, because the ring wrapped past them.
    ///
    /// Reported rather than hidden. A subscriber told nothing was dropped when something was is a
    /// subscriber whose view of the workspace is silently wrong.
    pub dropped: u64,
}

/// A bounded, sequence-numbered feed shared by every connection.
#[derive(Debug)]
pub struct EventFeed {
    inner: Mutex<Ring>,
}

/// How many entries are held before the oldest is dropped.
///
/// Bounded because a daemon that runs for a week must not turn its own notifications into the
/// reason it runs out of memory. A client that falls this far behind is told it did.
pub const FEED_CAPACITY: usize = 256;

/// Deliberately no `Default`: a ring whose `next_sequence` is zero would hand out sequence `0`,
/// and `0` is the cursor that means "nothing yet". [`EventFeed::new`] is the one constructor.
#[derive(Debug)]
struct Ring {
    entries: Vec<DaemonEvent>,
    next_sequence: u64,
    first_held: u64,
}

impl Default for EventFeed {
    fn default() -> Self {
        Self::new()
    }
}

impl EventFeed {
    /// An empty feed.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Ring {
                entries: Vec::new(),
                next_sequence: 1,
                first_held: 1,
            }),
        }
    }

    /// Append one entry and return the sequence it was given.
    pub fn publish(&self, kind: EventKind) -> u64 {
        let mut ring = self.lock();
        let sequence = ring.next_sequence;
        ring.next_sequence += 1;
        ring.entries.push(DaemonEvent { sequence, kind });
        if ring.entries.len() > FEED_CAPACITY {
            ring.entries.remove(0);
            ring.first_held += 1;
        }
        sequence
    }

    /// The sequence of the newest entry, or `0` when there is none.
    #[must_use]
    pub fn latest(&self) -> u64 {
        self.lock().next_sequence - 1
    }

    /// Everything after `cursor` that is still held, plus how much after it was dropped.
    #[must_use]
    pub fn since(&self, cursor: u64) -> EventBacklog {
        let ring = self.lock();
        let dropped = ring.first_held.saturating_sub(cursor + 1);
        EventBacklog {
            entries: ring
                .entries
                .iter()
                .filter(|entry| entry.sequence > cursor)
                .cloned()
                .collect(),
            dropped,
        }
    }

    /// A poisoned feed is recovered rather than propagated: a panicked connection thread must not
    /// stop every other connection from hearing what the daemon is doing.
    fn lock(&self) -> std::sync::MutexGuard<'_, Ring> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequences_start_at_one_and_are_contiguous() {
        let feed = EventFeed::new();
        assert_eq!(feed.latest(), 0);
        assert_eq!(feed.publish(EventKind::Serving), 1);
        assert_eq!(feed.publish(EventKind::Stopping), 2);
        assert_eq!(feed.latest(), 2);
    }

    #[test]
    fn a_cursor_gets_only_what_comes_after_it() {
        let feed = EventFeed::new();
        feed.publish(EventKind::Serving);
        feed.publish(EventKind::WorkspaceOpened { records: 3 });
        let backlog = feed.since(1);
        assert_eq!(backlog.entries.len(), 1);
        assert_eq!(backlog.entries[0].sequence, 2);
        assert_eq!(backlog.dropped, 0);
    }

    #[test]
    fn a_subscriber_that_falls_behind_the_ring_is_told_how_much_it_lost() {
        let feed = EventFeed::new();
        for _ in 0..(FEED_CAPACITY + 5) {
            feed.publish(EventKind::Serving);
        }
        let backlog = feed.since(0);
        assert_eq!(backlog.entries.len(), FEED_CAPACITY);
        assert_eq!(backlog.dropped, 5, "five entries fell out of the ring");
    }

    #[test]
    fn every_kind_has_a_distinct_word_and_an_object_body() {
        let kinds = [
            EventKind::Serving,
            EventKind::WorkspaceOpened { records: 0 },
            EventKind::WorkspaceRefused {
                code: "workspace-damaged".to_owned(),
            },
            EventKind::CheckpointRecoveryNeedsAttention,
            EventKind::Stopping,
        ];
        let mut words: Vec<&str> = kinds.iter().map(EventKind::word).collect();
        words.sort_unstable();
        let count = words.len();
        words.dedup();
        assert_eq!(words.len(), count, "two kinds share one wire word");
        for kind in &kinds {
            assert!(kind.to_json().is_object());
        }
    }
}
