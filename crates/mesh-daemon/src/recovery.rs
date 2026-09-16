//! Start-up recovery, its budget, and the crash diagnostics feed.
//!
//! # What this module is, and what it deliberately is not
//!
//! `mesh-store` owns the recovery itself: the frame layout, the scan that finds the last durable
//! boundary, the fold, and the comparison against the digest the acknowledgement carried. None of
//! that is repeated here, because a second implementation of a recovery is a second answer to
//! "did the index come back".
//!
//! What the daemon owns is the two things a library cannot: **when recovery runs** — once, before
//! anything is served — and **what is said about it afterwards**. Plan §3.5's OBS-002 and OBS-003
//! ask for a crash diagnostics feed; this is that feed's first entry, and it is the one entry that
//! is written on a path where the process has just been killed.
//!
//! # The budget is judged, not measured, inside this crate
//!
//! Plan §6.3 sets recovery at under five seconds. [`RecoveryBudget::judge`] is a pure comparison
//! over a [`Duration`] a caller supplies, so every assertion about the budget is reproducible and
//! none of them depends on the machine a test happens to run on. [`recover_on_start`] is the one
//! place a clock is read, through [`Instant`], which is monotonic: it measures an interval and it
//! never orders an event. Ordering in this program is `lamport → event id → content hash` and is
//! never wall-clock.
//!
//! # Recovery does not fail the daemon; it reports
//!
//! [`recover_on_start`] returns a [`RecoveryDiagnostic`] in every case, including the unrecoverable
//! one. A daemon that panicked on a damaged journal would take the only surface that can *tell*
//! anybody about the damage down with it, and the operator would be left with a process that will
//! not start and no sentence explaining why. The diagnostic carries the sentence, and
//! [`RecoveryDiagnostic::is_serving`] is what a caller reads to decide whether to open for
//! business.

use core::fmt;
use std::time::{Duration, Instant};

use mesh_store::{Digest16, RecordJournal, RecoveryReport, SqlExecutor, Store};

/// Plan §6.3: *"recovery after daemon crash: under five seconds"*.
pub const RECOVERY_BUDGET: RecoveryBudget = RecoveryBudget {
    limit: Duration::from_secs(5),
};

/// How long recovery is allowed to take before it is a defect rather than a delay.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecoveryBudget {
    /// The limit.
    pub limit: Duration,
}

impl RecoveryBudget {
    /// Whether an elapsed recovery is inside the budget.
    ///
    /// A recovery that takes exactly the limit is over it. The boundary is stated rather than left
    /// to a `<=` nobody reads: a budget met exactly is a budget that will be missed on the next
    /// machine.
    #[must_use]
    pub const fn judge(self, elapsed: Duration) -> BudgetVerdict {
        if elapsed.as_nanos() < self.limit.as_nanos() {
            BudgetVerdict::Inside
        } else {
            BudgetVerdict::Over
        }
    }
}

/// Whether a recovery met its budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BudgetVerdict {
    /// Inside the budget.
    Inside,
    /// At or over it.
    Over,
}

impl fmt::Display for BudgetVerdict {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Inside => "inside budget",
            Self::Over => "over budget",
        })
    }
}

/// How serious a diagnostic is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Nothing to act on.
    Routine,
    /// Worth looking at; the workspace is usable.
    Notable,
    /// The workspace is not usable until somebody acts.
    Blocking,
}

impl fmt::Display for Severity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Routine => "routine",
            Self::Notable => "notable",
            Self::Blocking => "blocking",
        })
    }
}

/// What start-up recovery found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecoveryOutcome {
    /// The journal ended on a whole record and the index was rebuilt from it.
    Rebuilt {
        /// How many records were replayed.
        records: u64,
        /// How many rows the rebuilt index holds.
        rows: usize,
        /// The digest of the index that came back.
        digest: Digest16,
    },
    /// The same, and the journal ended mid-record: a save was interrupted before it was whole.
    RebuiltAfterAnInterruptedSave {
        /// How many records were replayed.
        records: u64,
        /// How many rows the rebuilt index holds.
        rows: usize,
        /// The digest of the index that came back.
        digest: Digest16,
        /// How many bytes of an unfinished record were discarded.
        discarded_bytes: u64,
    },
    /// There are bytes and not one whole record among them: no boundary exists to recover to.
    ///
    /// Kept apart from [`Self::RebuiltAfterAnInterruptedSave`] with zero records, which is the
    /// same arithmetic and a different sentence. That variant says "everything you were told about
    /// is here, and one save was in flight"; this one has nothing before the unfinished bytes to
    /// stand behind that, and rendering it as a rebuild of zero records is indistinguishable from
    /// the report a brand-new empty folder produces. A crash that reads as a clean start is the
    /// one failure this whole module exists to prevent, so the two are separate values and the
    /// severity is [`Severity::Blocking`].
    NothingDurableToRecover {
        /// How many bytes are in the journal with no whole record among them.
        unfinished_bytes: u64,
    },
    /// Nothing was rebuilt, and the reason is reported rather than swallowed.
    Unrecoverable {
        /// What the store said, verbatim.
        detail: String,
    },
}

impl RecoveryOutcome {
    /// Whether the workspace can be served after this.
    #[must_use]
    pub const fn is_serving(&self) -> bool {
        !matches!(
            self,
            Self::Unrecoverable { .. } | Self::NothingDurableToRecover { .. }
        )
    }

    /// How serious this is on its own, before the budget is taken into account.
    #[must_use]
    pub const fn severity(&self) -> Severity {
        match self {
            Self::Rebuilt { .. } => Severity::Routine,
            Self::RebuiltAfterAnInterruptedSave { .. } => Severity::Notable,
            Self::NothingDurableToRecover { .. } | Self::Unrecoverable { .. } => Severity::Blocking,
        }
    }
}

/// One entry in the crash diagnostics feed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryDiagnostic {
    outcome: RecoveryOutcome,
    elapsed: Duration,
    verdict: BudgetVerdict,
}

impl RecoveryDiagnostic {
    /// Build one from an outcome and how long it took.
    #[must_use]
    pub const fn new(outcome: RecoveryOutcome, elapsed: Duration, budget: RecoveryBudget) -> Self {
        Self {
            outcome,
            elapsed,
            verdict: budget.judge(elapsed),
        }
    }

    /// What was found.
    #[must_use]
    pub const fn outcome(&self) -> &RecoveryOutcome {
        &self.outcome
    }

    /// How long it took.
    #[must_use]
    pub const fn elapsed(&self) -> Duration {
        self.elapsed
    }

    /// Whether it met the budget.
    #[must_use]
    pub const fn verdict(&self) -> BudgetVerdict {
        self.verdict
    }

    /// Whether the workspace can be served after this.
    #[must_use]
    pub const fn is_serving(&self) -> bool {
        self.outcome.is_serving()
    }

    /// How serious it is: a recovery that overran its budget is notable even when it worked.
    #[must_use]
    pub fn severity(&self) -> Severity {
        match (self.outcome.severity(), self.verdict) {
            (Severity::Routine, BudgetVerdict::Over) => Severity::Notable,
            (severity, _) => severity,
        }
    }
}

impl fmt::Display for RecoveryDiagnostic {
    /// One line, in the vocabulary the product uses. A diagnostic a user might see says what
    /// happened to their work and never how the storage layer is built.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "[{}] ", self.severity())?;
        match &self.outcome {
            RecoveryOutcome::Rebuilt {
                records,
                rows,
                digest,
            } => write!(
                formatter,
                "workspace restored from {records} saved records into {rows} rows, digest {digest}"
            )?,
            RecoveryOutcome::RebuiltAfterAnInterruptedSave {
                records,
                rows,
                digest,
                discarded_bytes,
            } => write!(
                formatter,
                "workspace restored from {records} saved records into {rows} rows, digest \
                 {digest}; one save was interrupted before it was complete and {discarded_bytes} \
                 unfinished bytes were set aside — nothing that was reported as saved privately is \
                 affected"
            )?,
            RecoveryOutcome::NothingDurableToRecover { unfinished_bytes } => write!(
                formatter,
                "the workspace holds {unfinished_bytes} bytes of an unfinished save and not one \
                 whole record, so there is no last durable boundary to restore to and nothing was \
                 changed; this is not an empty workspace and is not reported as one"
            )?,
            RecoveryOutcome::Unrecoverable { detail } => write!(
                formatter,
                "the workspace could not be restored and nothing was changed: {detail}"
            )?,
        }
        write!(
            formatter,
            " ({} ms, {})",
            self.elapsed.as_millis(),
            self.verdict
        )
    }
}

/// The crash diagnostics feed: what start-up recovery reported, oldest first.
///
/// Append-only and bounded. It is bounded because a daemon that restarts in a loop would otherwise
/// turn its own diagnostics into the reason it runs out of memory, and the entries that matter in
/// that loop are the most recent ones.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiagnosticsFeed {
    entries: Vec<RecoveryDiagnostic>,
    dropped: u64,
}

impl DiagnosticsFeed {
    /// How many entries are kept before the oldest is dropped.
    pub const CAPACITY: usize = 64;

    /// An empty feed.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one entry, dropping the oldest if the feed is full.
    pub fn record(&mut self, diagnostic: RecoveryDiagnostic) {
        if self.entries.len() == Self::CAPACITY {
            self.entries.remove(0);
            self.dropped += 1;
        }
        self.entries.push(diagnostic);
    }

    /// Every entry that is still held, oldest first.
    #[must_use]
    pub fn entries(&self) -> &[RecoveryDiagnostic] {
        &self.entries
    }

    /// The most recent entry.
    #[must_use]
    pub fn latest(&self) -> Option<&RecoveryDiagnostic> {
        self.entries.last()
    }

    /// How many entries have been dropped to stay inside [`Self::CAPACITY`].
    ///
    /// Reported rather than hidden: a feed that silently forgets is a feed that lies about how
    /// often the daemon has been restarting.
    #[must_use]
    pub const fn dropped(&self) -> u64 {
        self.dropped
    }
}

/// Recover the index before the daemon serves anything, and say what happened.
///
/// `expected` is the digest the last acknowledgement carried, when the daemon has one. Supplying it
/// turns the recovery into the `rebuild --verify` equivalent: a rebuild that lands anywhere else is
/// reported as unrecoverable rather than served.
pub fn recover_on_start<E: SqlExecutor, J: RecordJournal>(
    store: &mut Store<E>,
    journal: &mut J,
    expected: Option<Digest16>,
) -> RecoveryDiagnostic
where
    E::Error: fmt::Display,
    J::Error: fmt::Display,
{
    let started = Instant::now();
    let result = match expected {
        Some(digest) => store.recover_verified(journal, digest),
        None => store.recover(journal),
    };
    let elapsed = started.elapsed();

    let outcome = match result {
        Ok(report) => outcome_of(&report),
        Err(error) => RecoveryOutcome::Unrecoverable {
            detail: error.to_string(),
        },
    };
    RecoveryDiagnostic::new(outcome, elapsed, RECOVERY_BUDGET)
}

/// Read a successful recovery as an outcome. Separated from the timing so it can be tested without
/// one.
///
/// The three-way split is over the two facts the scan already decided — how many whole records
/// precede the boundary, and whether anything lies after it — and nothing here recomputes either.
#[must_use]
pub fn outcome_of(report: &RecoveryReport) -> RecoveryOutcome {
    let records = report.boundary().records;
    let rows = report.rebuild().total_rows();
    let digest = report.digest();
    let discarded_bytes = report.tail().discarded_bytes();
    match (records, report.interrupted()) {
        // Bytes, and no boundary among them. Reported as itself; see the variant.
        (0, true) => RecoveryOutcome::NothingDurableToRecover {
            unfinished_bytes: discarded_bytes,
        },
        (_, true) => RecoveryOutcome::RebuiltAfterAnInterruptedSave {
            records,
            rows,
            digest,
            discarded_bytes,
        },
        (_, false) => RecoveryOutcome::Rebuilt {
            records,
            rows,
            digest,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest() -> Digest16 {
        Digest16::from_bytes([7; 16])
    }

    fn rebuilt() -> RecoveryOutcome {
        RecoveryOutcome::Rebuilt {
            records: 12,
            rows: 40,
            digest: digest(),
        }
    }

    #[test]
    fn the_budget_is_plan_six_threes_five_seconds() {
        assert_eq!(RECOVERY_BUDGET.limit, Duration::from_secs(5));
    }

    #[test]
    fn the_budget_boundary_is_exclusive_and_says_so() {
        assert_eq!(
            RECOVERY_BUDGET.judge(Duration::from_millis(4_999)),
            BudgetVerdict::Inside
        );
        assert_eq!(
            RECOVERY_BUDGET.judge(Duration::from_secs(5)),
            BudgetVerdict::Over
        );
        assert_eq!(
            RECOVERY_BUDGET.judge(Duration::from_secs(6)),
            BudgetVerdict::Over
        );
    }

    #[test]
    fn a_clean_recovery_is_routine_and_serving() {
        let diagnostic =
            RecoveryDiagnostic::new(rebuilt(), Duration::from_millis(200), RECOVERY_BUDGET);
        assert_eq!(diagnostic.severity(), Severity::Routine);
        assert!(diagnostic.is_serving());
        assert!(diagnostic.to_string().contains("workspace restored"));
        assert!(diagnostic.to_string().contains("200 ms"));
    }

    /// A recovery that worked and overran is not routine, or the budget is a number nobody acts on.
    #[test]
    fn a_recovery_that_overruns_its_budget_is_notable_even_when_it_worked() {
        let diagnostic =
            RecoveryDiagnostic::new(rebuilt(), Duration::from_secs(9), RECOVERY_BUDGET);
        assert_eq!(diagnostic.verdict(), BudgetVerdict::Over);
        assert_eq!(diagnostic.severity(), Severity::Notable);
        assert!(diagnostic.is_serving());
        assert!(diagnostic.to_string().contains("over budget"));
    }

    #[test]
    fn an_interrupted_save_is_notable_and_says_nothing_acknowledged_was_lost() {
        let diagnostic = RecoveryDiagnostic::new(
            RecoveryOutcome::RebuiltAfterAnInterruptedSave {
                records: 12,
                rows: 40,
                digest: digest(),
                discarded_bytes: 88,
            },
            Duration::from_millis(30),
            RECOVERY_BUDGET,
        );
        assert_eq!(diagnostic.severity(), Severity::Notable);
        assert!(diagnostic.is_serving());
        let line = diagnostic.to_string();
        assert!(line.contains("88 unfinished bytes"));
        assert!(line.contains("saved privately"));
    }

    /// The criterion in its exact words: *a crash with no recoverable boundary is reported loudly
    /// rather than as a clean start*. Loud here is measurable — a different variant, a blocking
    /// severity, a refusal to serve, and a line that does not read like the one an empty folder
    /// produces.
    #[test]
    fn a_crash_with_no_recoverable_boundary_is_not_a_clean_start() {
        let nothing = RecoveryDiagnostic::new(
            RecoveryOutcome::NothingDurableToRecover {
                unfinished_bytes: 41,
            },
            Duration::from_millis(2),
            RECOVERY_BUDGET,
        );
        let clean = RecoveryDiagnostic::new(
            RecoveryOutcome::Rebuilt {
                records: 0,
                rows: 0,
                digest: digest(),
            },
            Duration::from_millis(2),
            RECOVERY_BUDGET,
        );

        assert_eq!(nothing.severity(), Severity::Blocking);
        assert!(!nothing.is_serving());
        assert!(clean.is_serving());
        assert_ne!(
            nothing.to_string(),
            clean.to_string(),
            "an unfinished-only workspace read exactly like an empty one"
        );
        assert!(nothing.to_string().contains("41 bytes"));
        assert!(nothing.to_string().contains("not an empty workspace"));
    }

    #[test]
    fn an_unrecoverable_journal_blocks_serving_and_carries_the_reason() {
        let diagnostic = RecoveryDiagnostic::new(
            RecoveryOutcome::Unrecoverable {
                detail: "record 3 at byte 400 is unrecoverable".to_owned(),
            },
            Duration::from_millis(4),
            RECOVERY_BUDGET,
        );
        assert_eq!(diagnostic.severity(), Severity::Blocking);
        assert!(!diagnostic.is_serving());
        assert!(diagnostic.to_string().contains("nothing was changed"));
        assert!(diagnostic.to_string().contains("record 3 at byte 400"));
    }

    /// The user-facing sentence is held to plan §3.4's vocabulary: none of the storage words a
    /// reader would have to be an engineer to understand may appear in it.
    #[test]
    fn no_diagnostic_line_uses_a_banned_word() {
        let banned = [
            "DAG",
            "frontier",
            "vector clock",
            "branch",
            "commit",
            "rebase",
            "staging",
            " ref ",
            "operation log",
        ];
        let lines = [
            RecoveryDiagnostic::new(rebuilt(), Duration::from_millis(1), RECOVERY_BUDGET),
            RecoveryDiagnostic::new(
                RecoveryOutcome::RebuiltAfterAnInterruptedSave {
                    records: 1,
                    rows: 2,
                    digest: digest(),
                    discarded_bytes: 3,
                },
                Duration::from_millis(1),
                RECOVERY_BUDGET,
            ),
            RecoveryDiagnostic::new(
                RecoveryOutcome::NothingDurableToRecover {
                    unfinished_bytes: 4,
                },
                Duration::from_millis(1),
                RECOVERY_BUDGET,
            ),
            RecoveryDiagnostic::new(
                RecoveryOutcome::Unrecoverable {
                    detail: "the journal is unreadable".to_owned(),
                },
                Duration::from_millis(1),
                RECOVERY_BUDGET,
            ),
        ];
        for line in lines {
            let rendered = line.to_string().to_lowercase();
            for word in banned {
                assert!(
                    !rendered.contains(&word.to_lowercase()),
                    "{word:?} appears in {rendered:?}"
                );
            }
        }
    }

    #[test]
    fn the_feed_keeps_the_most_recent_entries_and_counts_what_it_dropped() {
        let mut feed = DiagnosticsFeed::new();
        assert!(feed.latest().is_none());

        for index in 0..DiagnosticsFeed::CAPACITY + 5 {
            feed.record(RecoveryDiagnostic::new(
                RecoveryOutcome::Rebuilt {
                    records: index as u64,
                    rows: 0,
                    digest: digest(),
                },
                Duration::from_millis(1),
                RECOVERY_BUDGET,
            ));
        }
        assert_eq!(feed.entries().len(), DiagnosticsFeed::CAPACITY);
        assert_eq!(feed.dropped(), 5);
        assert_eq!(
            feed.latest().expect("an entry").outcome(),
            &RecoveryOutcome::Rebuilt {
                records: (DiagnosticsFeed::CAPACITY + 4) as u64,
                rows: 0,
                digest: digest(),
            }
        );
    }
}
