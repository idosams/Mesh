//! Where a change sits in the workspace's total order.
//!
//! # The order is `lamport → event ULID → content hash`, and nothing else
//!
//! Two actors can rename the same entry without ever having heard of each other. Something has to
//! decide which name the entry ends up with, and that decision must come out the same on every
//! machine that has seen the same two changes — otherwise two peers holding one causal set show
//! their humans two different trees.
//!
//! Wall-clock time cannot be that decider. A laptop with a badly set clock would win every contest
//! it entered, and the same two changes replayed a year later would resolve differently. So a
//! [`Stamp`] carries a Lamport counter first, the event's ULID second and the content hash of the
//! record third — three values every peer already has, none of which is a clock reading.
//!
//! The field order of [`Stamp`] **is** the tiebreak order: `Ord` is derived, and a derived `Ord` on
//! a struct compares fields in declaration order. Reordering the fields would silently reorder the
//! protocol, which is why `tests/identity.rs` pins each of the three tiebreaks separately.
//!
//! A ULID's leading forty-eight bits are a millisecond timestamp, so a reader could mistake the
//! second component for a clock. It is not used as one: the comparison is `memcmp` over sixteen
//! opaque bytes, and it only ever runs when two Lamport counters are already equal — which is
//! exactly the concurrent case, where no clock has any authority at all.

use core::fmt;

/// A Lamport counter: how many changes this one causally follows.
///
/// Monotone per actor and advanced past every counter an actor has observed. Two changes with
/// equal counters are concurrent, which is the only case where the later tiebreaks run.
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
    /// Saturating rather than wrapping: a wrapped counter would reorder the whole history, and a
    /// workspace that has authored `u64::MAX` changes has a bigger problem than a stuck counter.
    #[must_use]
    pub const fn next(&self) -> Self {
        Self(self.0.saturating_add(1))
    }

    /// The counter that causally follows both.
    #[must_use]
    pub const fn merge(&self, other: Self) -> Self {
        if self.0 >= other.0 {
            *self
        } else {
            other
        }
    }
}

impl fmt::Display for Lamport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// The ULID naming one event.
///
/// Sixteen opaque bytes, compared by `memcmp`. See the module header for why the timestamp inside a
/// ULID is not a clock reading here.
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
    /// How many bytes a stamp occupies: eight for the counter, sixteen for the event, thirty-two
    /// for the content hash.
    pub const BYTE_WIDTH: usize = 8 + EventId::BYTE_WIDTH + 32;

    /// The stamp with this position.
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

    /// The content hash of the record that carried the change.
    #[must_use]
    pub const fn content(&self) -> &[u8; 32] {
        &self.content
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
    fn the_counter_decides_first() {
        // A lower counter loses even with a higher event identifier and a higher content hash.
        assert!(stamp(1, 0xff, 0xff) < stamp(2, 0x00, 0x00));
    }

    #[test]
    fn the_event_identifier_decides_when_the_counters_tie() {
        assert!(stamp(7, 0x01, 0xff) < stamp(7, 0x02, 0x00));
    }

    #[test]
    fn the_content_hash_decides_when_the_counter_and_the_event_both_tie() {
        assert!(stamp(7, 0x02, 0x01) < stamp(7, 0x02, 0x02));
    }

    #[test]
    fn a_counter_advances_past_everything_it_has_observed() {
        assert_eq!(Lamport::ORIGIN.next(), Lamport::new(1));
        assert_eq!(Lamport::new(3).merge(Lamport::new(9)), Lamport::new(9));
        assert_eq!(Lamport::new(9).merge(Lamport::new(3)), Lamport::new(9));
        assert_eq!(Lamport::new(u64::MAX).next(), Lamport::new(u64::MAX));
    }

    #[test]
    fn a_stamp_is_fifty_six_bytes() {
        assert_eq!(Stamp::BYTE_WIDTH, 56);
    }

    #[test]
    fn debug_renders_the_event_as_hex() {
        let text = format!("{:?}", EventId::from_bytes([0xab; 16]));
        assert_eq!(text, format!("EventId({})", "ab".repeat(16)));
    }
}
