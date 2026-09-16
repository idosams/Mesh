//! The counter catalogue: one row per counter, one row per required benchmark metric.
//!
//! # Two tables, and why they are not one
//!
//! [`REQUIRED_METRICS`] is plan §12.3 transcribed — one row per bullet, in the plan's own order.
//! [`COUNTERS`] is what this build actually increments. They are separate because the interesting
//! question is the *edge between them*: which required metric has a counter behind it, which
//! counter is orphaned, and which required metric is served only by a wall-clock number.
//! `crates/mesh-daemon/tests/counters.rs` walks both directions of that edge, so neither table can
//! grow a row the other does not account for.
//!
//! # Deterministic where it can be
//!
//! A counter is a **count** or a **byte total** wherever the metric admits one, and nanoseconds
//! only where it does not. That is not a stylistic preference: two wall-clock budget assertions on
//! this repository's merge path have already failed under machine load, and a gate that fails on
//! load teaches its readers to re-run rather than read. So a latency metric such as `lookup` is
//! backed by **two** counters — `local_filesystem.lookup.ops`, which is exact on any machine under
//! any load, and `local_filesystem.lookup.ns`, which is not and says so.
//!
//! [`Unit`] decides [`Determinism`] and [`Band`], in that order, so a row cannot declare itself
//! deterministic and then be measured in nanoseconds. The band is what
//! `crates/mesh-daemon/tests/counters.rs` holds each family to:
//!
//! | Determinism | Band | What the cross-validation asserts |
//! |---|---|---|
//! | `Deterministic` | [`Band::Exact`] | counter **equals** an independently computed value |
//! | `LoadDependent` | [`Band::Enclosed`] | counter is **at most** an independent measurement that encloses the same section |
//!
//! `Enclosed` is a real assertion and not a weakened one: it holds under any load, on any machine,
//! and it fails if the counter is measuring something other than the section it claims.
//!
//! # A counter with nothing behind it says so
//!
//! [`Producer`] is on every row. `Wired` names what feeds the counter in this build; `NotYet` names
//! why nothing does. A `NotYet` counter reads zero, and a zero that means "nothing measured this"
//! is reported as such by [`crate::counters::CounterSnapshot::not_yet`] rather than rendered next
//! to a zero that means "this happened no times". Those are different facts and a surface that
//! renders both the same way has told the reader the second when the truth was the first.

mod context;
mod local_filesystem;
mod synchronization;
mod workspace;

/// Which of plan §12.3's four metric groups a row belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Family {
    /// Plan §12.3 "Local filesystem".
    LocalFilesystem,
    /// Plan §12.3 "Workspace operations".
    WorkspaceOperations,
    /// Plan §12.3 "Synchronization".
    Synchronization,
    /// Plan §12.3 "Context".
    Context,
}

impl Family {
    /// Every family, in plan §12.3's order.
    pub const ALL: &'static [Self] = &[
        Self::LocalFilesystem,
        Self::WorkspaceOperations,
        Self::Synchronization,
        Self::Context,
    ];

    /// The stable machine word for this family.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::LocalFilesystem => "local_filesystem",
            Self::WorkspaceOperations => "workspace_operations",
            Self::Synchronization => "synchronization",
            Self::Context => "context",
        }
    }
}

/// What one observation of a counter adds to its total.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    /// A count of things that happened. Exact on any machine under any load.
    Events,
    /// A number of bytes. Exact on any machine under any load.
    Bytes,
    /// Elapsed nanoseconds. Not exact, and the only unit on this surface that is not.
    Nanoseconds,
}

impl Unit {
    /// The stable machine word for this unit.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Events => "events",
            Self::Bytes => "bytes",
            Self::Nanoseconds => "nanoseconds",
        }
    }

    /// Whether the same work produces the same total on a second run.
    #[must_use]
    pub const fn determinism(self) -> Determinism {
        match self {
            Self::Events | Self::Bytes => Determinism::Deterministic,
            Self::Nanoseconds => Determinism::LoadDependent,
        }
    }
}

/// Whether a counter's total is reproducible from the work alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Determinism {
    /// The same work gives the same total, on any machine, under any load.
    Deterministic,
    /// The total depends on what else the machine was doing.
    LoadDependent,
}

impl Determinism {
    /// The stable machine word.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Deterministic => "deterministic",
            Self::LoadDependent => "load-dependent",
        }
    }

    /// How close an independent measurement of the same work has to land.
    #[must_use]
    pub const fn band(self) -> Band {
        match self {
            Self::Deterministic => Band::Exact,
            Self::LoadDependent => Band::Enclosed,
        }
    }
}

/// The stated band a counter is cross-validated against.
///
/// Stated here rather than chosen per test, because a band picked after seeing the numbers is not
/// a band. Neither of these is a tolerance percentage: a percentage on a wall-clock number is what
/// fails under load, and there is no percentage on this surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    /// The counter must equal the independently computed value. No tolerance at all.
    Exact,
    /// The counter times an inner section, so an independent measurement that encloses that
    /// section must be at least as large. True under any load; false if the counter is timing
    /// something other than what it names.
    Enclosed,
}

impl Band {
    /// The stable machine word.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Enclosed => "enclosed",
        }
    }
}

/// Whether anything in this build feeds a counter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Producer {
    /// Something in this build records into it, and this names what.
    Wired(&'static str),
    /// Nothing in this build records into it, and this is why.
    NotYet(&'static str),
}

impl Producer {
    /// Whether this build feeds the counter.
    #[must_use]
    pub const fn is_wired(self) -> bool {
        matches!(self, Self::Wired(_))
    }

    /// The sentence behind either arm.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::Wired(detail) | Self::NotYet(detail) => detail,
        }
    }
}

/// One counter: what it is called, what it counts, and whether anything feeds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CounterSpec {
    /// The stable dotted key, `<family>.<subject>.<unit-suffix>`.
    pub key: &'static str,
    /// Which of plan §12.3's groups it belongs to.
    pub family: Family,
    /// What one observation adds.
    pub unit: Unit,
    /// One sentence naming what an observation means.
    pub what: &'static str,
    /// Whether this build feeds it.
    pub producer: Producer,
}

impl CounterSpec {
    /// Whether the same work gives the same total twice.
    #[must_use]
    pub const fn determinism(&self) -> Determinism {
        self.unit.determinism()
    }

    /// The band an independent measurement of the same work is held to.
    #[must_use]
    pub const fn band(&self) -> Band {
        self.unit.determinism().band()
    }
}

/// One bullet of plan §12.3, and the counters that stand behind it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequiredMetric {
    /// The metric as plan §12.3 names it.
    pub metric: &'static str,
    /// Which group the plan lists it under.
    pub family: Family,
    /// The counters behind it, deterministic ones first.
    pub counters: &'static [&'static str],
}

/// Every counter, family by family, in plan §12.3's order.
///
/// Held as one block per family rather than one flat array because a const slice cannot be
/// concatenated at compile time and a hand-maintained flat copy would be a second source of truth.
/// [`counters`] flattens it, and the flattened order is stable, which is what makes
/// [`CounterId`] an index rather than a name lookup on the hot path.
const FAMILY_BLOCKS: &[&[CounterSpec]] = &[
    local_filesystem::COUNTERS,
    workspace::COUNTERS,
    synchronization::COUNTERS,
    context::COUNTERS,
];

/// Every required metric, family by family, in plan §12.3's order.
const REQUIRED_BLOCKS: &[&[RequiredMetric]] = &[
    local_filesystem::REQUIRED,
    workspace::REQUIRED,
    synchronization::REQUIRED,
    context::REQUIRED,
];

/// Every counter this build knows, in a stable order.
pub fn counters() -> impl Iterator<Item = &'static CounterSpec> {
    FAMILY_BLOCKS.iter().copied().flatten()
}

/// How many counters there are.
#[must_use]
pub fn counter_count() -> usize {
    FAMILY_BLOCKS.iter().map(|block| block.len()).sum()
}

/// Every required benchmark metric of plan §12.3, in the plan's order.
pub fn required_metrics() -> impl Iterator<Item = &'static RequiredMetric> {
    REQUIRED_BLOCKS.iter().copied().flatten()
}

/// How many required metrics plan §12.3 lists.
#[must_use]
pub fn required_metric_count() -> usize {
    REQUIRED_BLOCKS.iter().map(|block| block.len()).sum()
}

/// A resolved position in the flattened catalogue.
///
/// Resolution is a linear scan and recording is not: a caller resolves once, off the hot path,
/// and then records against an index. [`crate::counters::Counters::record`] takes one of these and
/// never a string, so the cost of naming a counter is paid where it is visible.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CounterId(usize);

impl CounterId {
    /// The counter with this key, when the catalogue has one.
    #[must_use]
    pub fn of(key: &str) -> Option<Self> {
        counters().position(|spec| spec.key == key).map(Self)
    }

    /// This identifier's position in the flattened catalogue.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0
    }

    /// The identifier at `index`, when the catalogue is that long.
    #[must_use]
    pub fn at(index: usize) -> Option<Self> {
        (index < counter_count()).then_some(Self(index))
    }

    /// The row this identifier points at.
    #[must_use]
    pub fn spec(self) -> &'static CounterSpec {
        counters()
            .nth(self.0)
            .expect("a CounterId is only ever built from a position inside the catalogue")
    }
}

/// The counter with this key, when the catalogue has one.
#[must_use]
pub fn spec(key: &str) -> Option<&'static CounterSpec> {
    counters().find(|entry| entry.key == key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn every_key_is_unique_and_starts_with_its_family() {
        let mut seen = BTreeSet::new();
        for entry in counters() {
            assert!(seen.insert(entry.key), "two counters share `{}`", entry.key);
            assert!(
                entry.key.starts_with(entry.family.word()),
                "`{}` is filed under {} but is not named for it",
                entry.key,
                entry.family.word()
            );
            assert!(!entry.what.is_empty(), "`{}` says nothing", entry.key);
            assert!(
                !entry.producer.reason().is_empty(),
                "`{}` has a producer with no reason",
                entry.key
            );
        }
        assert_eq!(seen.len(), counter_count());
    }

    #[test]
    fn a_nanosecond_counter_is_the_only_load_dependent_one() {
        for entry in counters() {
            let expected = match entry.unit {
                Unit::Nanoseconds => Determinism::LoadDependent,
                Unit::Events | Unit::Bytes => Determinism::Deterministic,
            };
            assert_eq!(entry.determinism(), expected, "`{}`", entry.key);
        }
    }

    #[test]
    fn the_band_follows_the_determinism_and_not_the_other_way_round() {
        assert_eq!(Determinism::Deterministic.band(), Band::Exact);
        assert_eq!(Determinism::LoadDependent.band(), Band::Enclosed);
        for entry in counters() {
            assert_eq!(entry.band(), entry.determinism().band(), "`{}`", entry.key);
        }
    }

    #[test]
    fn identifiers_round_trip_through_their_keys() {
        for (index, entry) in counters().enumerate() {
            let id = CounterId::of(entry.key).expect("the catalogue holds its own keys");
            assert_eq!(id.index(), index);
            assert_eq!(id.spec().key, entry.key);
            assert_eq!(CounterId::at(index), Some(id));
        }
        assert_eq!(CounterId::of("nothing.at.all"), None);
        assert_eq!(CounterId::at(counter_count()), None);
        assert_eq!(spec("nothing.at.all"), None);
    }

    #[test]
    fn families_are_listed_in_the_plans_order_and_have_distinct_words() {
        let words: BTreeSet<&str> = Family::ALL.iter().map(|family| family.word()).collect();
        assert_eq!(words.len(), Family::ALL.len());
        let mut previous: Option<Family> = None;
        for entry in counters() {
            if previous != Some(entry.family) {
                assert!(
                    previous.is_none_or(|before| before < entry.family),
                    "family blocks are out of plan order at `{}`",
                    entry.key
                );
                previous = Some(entry.family);
            }
        }
    }
}
