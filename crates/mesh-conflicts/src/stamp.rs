//! Where a change sits in the workspace's total order.
//!
//! # The order is `lamport → event ULID → content hash`, and nothing else
//!
//! Every rule in this crate that has to pick an order — which move is applied first, which of two
//! same-name creates keeps the plain name — reads a [`Stamp`] and reads nothing else. Wall-clock
//! time is not available here and could not be used if it were: a laptop with a badly set clock
//! would win every contest it entered, and the same two changes replayed a year later would
//! resolve differently. A conflict resolution that is not identical on every peer is not a
//! resolution.
//!
//! The field order of [`Stamp`] **is** the tiebreak order: `Ord` is derived, and a derived `Ord`
//! compares fields in declaration order. Reordering the fields would silently reorder the
//! protocol, which is why `tests/conflicts.rs` pins each of the three tiebreaks separately.
//!
//! A ULID's leading forty-eight bits are a millisecond timestamp, so a reader could mistake
//! [`EventId`] for a clock. It is not used as one: the comparison is `memcmp` over sixteen opaque
//! bytes, and it only ever runs when two Lamport counters are already equal — which is exactly the
//! concurrent case, where no clock has any authority at all.
//!
//! This is `mesh-state`'s `Stamp`, mirrored for the reason `src/ids.rs` states.

use core::fmt;

/// A Lamport counter: how many changes this one causally follows.
///
/// Two changes with equal counters are concurrent, which is the only case where the later
/// tiebreaks run — and the only case where this crate has anything to decide.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Lamport(u64);

impl Lamport {
    /// The counter before anything has happened.
    pub const ORIGIN: Self = Self(0);

    /// The counter with this value.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// The value.
    #[must_use]
    pub const fn value(&self) -> u64 {
        self.0
    }

    /// The counter a change authored after observing `self` carries.
    ///
    /// Saturating rather than wrapping: a wrapped counter would reorder the whole history.
    #[must_use]
    pub const fn next(&self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

impl fmt::Display for Lamport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// The ULID naming one event.
///
/// Sixteen opaque bytes, compared by `memcmp`. See the module header for why the timestamp inside
/// a ULID is not a clock reading here.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventId([u8; 16]);

impl EventId {
    /// How many bytes an event identifier occupies.
    pub const BYTE_WIDTH: usize = 16;

    /// Wrap bytes some other layer already minted.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// The raw bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl fmt::Debug for EventId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("EventId(")?;
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        formatter.write_str(")")
    }
}

/// The total order position of one change: `lamport`, then `event`, then `content`.
///
/// Derived `Ord` compares the fields in declaration order, so the declaration order below is the
/// protocol's tiebreak order. Nothing here is a clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Stamp {
    lamport: Lamport,
    event: EventId,
    content: [u8; 32],
}

impl Stamp {
    /// The stamp with these three components.
    #[must_use]
    pub const fn new(lamport: Lamport, event: EventId, content: [u8; 32]) -> Self {
        Self {
            lamport,
            event,
            content,
        }
    }

    /// The Lamport counter.
    #[must_use]
    pub const fn lamport(&self) -> Lamport {
        self.lamport
    }

    /// The event identifier.
    #[must_use]
    pub const fn event(&self) -> EventId {
        self.event
    }

    /// The content hash of the record this stamp came from.
    #[must_use]
    pub const fn content(&self) -> &[u8; 32] {
        &self.content
    }

    /// Whether the two stamps are concurrent — equal Lamport counters.
    ///
    /// This is the only question the conflict table ever asks about a pair of changes. Everything
    /// downstream of it is which rule row applies, never which change is newer in real time.
    #[must_use]
    pub fn is_concurrent_with(&self, other: &Self) -> bool {
        self.lamport == other.lamport
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp(lamport: u64, event: u8, content: u8) -> Stamp {
        Stamp::new(
            Lamport::new(lamport),
            EventId::from_bytes([event; 16]),
            [content; 32],
        )
    }

    #[test]
    fn the_lamport_counter_is_compared_first() {
        assert!(stamp(1, 0xff, 0xff) < stamp(2, 0x00, 0x00));
    }

    #[test]
    fn the_event_identifier_breaks_an_equal_counter() {
        assert!(stamp(1, 0x01, 0xff) < stamp(1, 0x02, 0x00));
    }

    #[test]
    fn the_content_hash_breaks_an_equal_counter_and_event() {
        assert!(stamp(1, 0x01, 0x01) < stamp(1, 0x01, 0x02));
    }

    #[test]
    fn equal_counters_are_concurrent_and_unequal_ones_are_not() {
        assert!(stamp(4, 1, 1).is_concurrent_with(&stamp(4, 2, 2)));
        assert!(!stamp(4, 1, 1).is_concurrent_with(&stamp(5, 1, 1)));
    }

    #[test]
    fn the_counter_advances_without_wrapping() {
        assert_eq!(Lamport::ORIGIN.next(), Lamport::new(1));
        assert_eq!(Lamport::new(u64::MAX).next(), Lamport::new(u64::MAX));
    }
}
