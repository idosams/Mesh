//! Bounded, deterministic campaigns over the simulator's public audit surface.

use core::fmt;

use crate::{
    minimize_failure, FailureRecord, InvariantReport, InvariantViolation, Schedule, Seed,
    SimulationConfig, StateMutant, SIMULATOR_PROTOCOL_VERSION,
};

/// The most schedule deliveries one campaign may materialize.
///
/// The runner has no ambient timeout or memory probe, so a deterministic work bound
/// is part of its input contract rather than a machine-dependent runtime decision.
pub const MAX_CAMPAIGN_DELIVERIES: usize = 1_000_000;

/// Validated inputs for one contiguous seed campaign.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CampaignConfig {
    first_seed: Seed,
    case_count: usize,
    simulation: SimulationConfig,
}

impl CampaignConfig {
    /// Validate a contiguous campaign range and its total work.
    ///
    /// # Errors
    ///
    /// Refuses an empty range, a seed range that would wrap `u64`, or a
    /// campaign exceeding [`MAX_CAMPAIGN_DELIVERIES`].
    pub fn new(
        first_seed: Seed,
        case_count: usize,
        simulation: SimulationConfig,
    ) -> Result<Self, CampaignConfigError> {
        if case_count == 0 {
            return Err(CampaignConfigError::Empty);
        }
        let last_offset =
            u64::try_from(case_count - 1).map_err(|_| CampaignConfigError::SeedRangeOverflow)?;
        first_seed
            .value()
            .checked_add(last_offset)
            .ok_or(CampaignConfigError::SeedRangeOverflow)?;
        let deliveries = case_count
            .checked_mul(simulation.steps())
            .ok_or(CampaignConfigError::TooManyDeliveries)?;
        if deliveries > MAX_CAMPAIGN_DELIVERIES {
            return Err(CampaignConfigError::TooManyDeliveries);
        }
        Ok(Self {
            first_seed,
            case_count,
            simulation,
        })
    }

    /// First seed in the inclusive contiguous range.
    #[must_use]
    pub const fn first_seed(self) -> Seed {
        self.first_seed
    }

    /// Number of schedules in the campaign.
    #[must_use]
    pub const fn case_count(self) -> usize {
        self.case_count
    }

    /// Generation configuration shared by every schedule.
    #[must_use]
    pub const fn simulation(self) -> SimulationConfig {
        self.simulation
    }

    /// Exact number of delivered operations the campaign will audit.
    #[must_use]
    pub const fn delivery_count(self) -> usize {
        self.case_count * self.simulation.steps()
    }
}

/// Why a campaign configuration was refused before any schedule ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CampaignConfigError {
    /// At least one seed must be selected.
    Empty,
    /// The inclusive contiguous seed range would wrap `u64`.
    SeedRangeOverflow,
    /// The campaign exceeds its deterministic delivery budget.
    TooManyDeliveries,
}

impl fmt::Display for CampaignConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Empty => "a campaign contains at least one seed",
            Self::SeedRangeOverflow => "the contiguous seed range must not wrap u64",
            Self::TooManyDeliveries => "the campaign exceeds its delivery budget",
        })
    }
}

impl std::error::Error for CampaignConfigError {}

/// Which fold a campaign audited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CampaignMode {
    /// The unmodified production state fold.
    Audit,
    /// One named diagnostic defect, used to prove the campaign detects and minimizes failures.
    Planted(StateMutant),
}

impl CampaignMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Audit => "audit",
            Self::Planted(mutant) => mutant.as_str(),
        }
    }
}

/// One failing seed and its replayable one-minimal input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CampaignFailure {
    seed: Seed,
    violations: Vec<InvariantViolation>,
    reproduction: FailureRecord,
}

impl CampaignFailure {
    /// Seed whose generated schedule failed.
    #[must_use]
    pub const fn seed(&self) -> Seed {
        self.seed
    }

    /// Findings from the complete generated schedule, in canonical audit order.
    #[must_use]
    pub fn violations(&self) -> &[InvariantViolation] {
        &self.violations
    }

    /// Exact source schedule and one-minimal delivery selection.
    #[must_use]
    pub const fn reproduction(&self) -> &FailureRecord {
        &self.reproduction
    }
}

/// Typed and canonically encodable result of one campaign.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CampaignReport {
    config: CampaignConfig,
    mode: CampaignMode,
    failures: Vec<CampaignFailure>,
}

impl CampaignReport {
    /// Validated campaign inputs.
    #[must_use]
    pub const fn config(&self) -> CampaignConfig {
        self.config
    }

    /// Fold that was audited.
    #[must_use]
    pub const fn mode(&self) -> CampaignMode {
        self.mode
    }

    /// Whether every generated schedule satisfied every standing invariant.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.failures.is_empty()
    }

    /// Failures in ascending seed order.
    #[must_use]
    pub fn failures(&self) -> &[CampaignFailure] {
        &self.failures
    }

    /// Stable, platform-independent line record for CI output and artifact comparison.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let simulation = self.config.simulation();
        let mut text = format!(
            "mesh-simulator-campaign/0\n\
             simulator_protocol={SIMULATOR_PROTOCOL_VERSION}\n\
             mode={}\n\
             first_seed={}\n\
             cases={}\n\
             actors={}\n\
             steps={}\n\
             overlap_per_256={}\n\
             deliveries={}\n\
             status={}\n\
             failure_count={}\n",
            self.mode.as_str(),
            self.config.first_seed().value(),
            self.config.case_count(),
            simulation.actor_count(),
            simulation.steps(),
            simulation.overlap_per_256(),
            self.config.delivery_count(),
            if self.is_clean() { "clean" } else { "failed" },
            self.failures.len(),
        );
        for failure in &self.failures {
            text.push_str("failure=seed:");
            text.push_str(&failure.seed().value().to_string());
            text.push_str("|violations:");
            for (index, violation) in failure.violations().iter().enumerate() {
                if index > 0 {
                    text.push(',');
                }
                text.push_str(&violation.delivery().to_string());
                text.push(':');
                text.push_str(violation.invariant().as_str());
            }
            text.push_str("|minimal:");
            for (index, delivery) in failure
                .reproduction()
                .minimal_reproduction()
                .iter()
                .enumerate()
            {
                if index > 0 {
                    text.push(',');
                }
                text.push_str(&delivery.to_string());
            }
            text.push('\n');
        }
        text.into_bytes()
    }
}

/// Run the production invariant campaign without consulting ambient input.
#[must_use]
pub fn run_campaign(config: CampaignConfig) -> CampaignReport {
    run(config, CampaignMode::Audit)
}

/// Run a diagnostic campaign with one planted defect.
///
/// This is an executable check of the campaign itself: each detected failure is
/// reduced through the same public minimizer used for a production finding.
#[must_use]
pub fn run_mutated_campaign(config: CampaignConfig, mutant: StateMutant) -> CampaignReport {
    run(config, CampaignMode::Planted(mutant))
}

fn run(config: CampaignConfig, mode: CampaignMode) -> CampaignReport {
    let mut failures = Vec::new();
    for offset in 0..config.case_count() {
        let seed = Seed::new(config.first_seed().value() + offset as u64);
        let schedule = Schedule::generate(seed, config.simulation());
        let report = audit(&schedule, mode);
        if report.is_clean() {
            continue;
        }
        let reproduction =
            minimize_failure(&schedule, |candidate| !audit(candidate, mode).is_clean())
                .expect("the same failing predicate was checked immediately before minimization");
        failures.push(CampaignFailure {
            seed,
            violations: report.violations().to_vec(),
            reproduction,
        });
    }
    CampaignReport {
        config,
        mode,
        failures,
    }
}

fn audit(schedule: &Schedule, mode: CampaignMode) -> InvariantReport {
    match mode {
        CampaignMode::Audit => schedule.audit(),
        CampaignMode::Planted(mutant) => schedule.audit_mutated(mutant),
    }
}
