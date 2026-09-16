//! The clocks the harness reads, behind traits so tests are not timing-dependent.
//!
//! Two separate clocks on purpose. [`Clock`] is monotonic and is the only thing
//! allowed near a timed section. [`WallClock`] is wall-clock and is used for one
//! descriptive metadata field; nothing in Mesh orders anything by it.

use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// A monotonic source of nanoseconds.
///
/// Implementations must never go backwards — the harness treats a backwards
/// step as a hard error rather than clamping it, because a clamped negative
/// duration is a fabricated measurement.
pub trait Clock {
    /// Nanoseconds since an arbitrary but fixed origin.
    fn now_nanos(&self) -> u64;
}

/// The real monotonic clock, anchored at construction.
#[derive(Clone, Debug)]
pub struct MonotonicClock {
    origin: Instant,
}

impl MonotonicClock {
    /// Anchors a new clock at the current instant.
    pub fn new() -> Self {
        MonotonicClock {
            origin: Instant::now(),
        }
    }
}

impl Default for MonotonicClock {
    fn default() -> Self {
        MonotonicClock::new()
    }
}

impl Clock for MonotonicClock {
    fn now_nanos(&self) -> u64 {
        u64::try_from(self.origin.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
}

/// A source of wall-clock time, for the descriptive capture stamp only.
pub trait WallClock {
    /// Milliseconds since the Unix epoch.
    fn unix_millis(&self) -> u64;
}

/// The host's wall clock.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemWallClock;

impl WallClock for SystemWallClock {
    fn unix_millis(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_monotonic_clock_does_not_go_backwards() {
        let clock = MonotonicClock::new();
        let first = clock.now_nanos();
        let second = clock.now_nanos();
        assert!(second >= first);
    }

    #[test]
    fn the_wall_clock_is_after_the_projects_start() {
        // 2020-01-01T00:00:00Z in milliseconds — a stamp below this means the
        // host clock is unusable, and the schema rejects a zero stamp anyway.
        assert!(SystemWallClock.unix_millis() > 1_577_836_800_000);
    }
}
