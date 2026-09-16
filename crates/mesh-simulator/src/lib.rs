//! Deterministic operation schedules over the real core identity model.
//!
//! The simulator owns no clock, entropy source, filesystem or network. A [`Seed`] and a
//! [`SimulationConfig`] are the complete input to [`Schedule::generate`], and both the generated
//! schedule and its [`SimulationResult`] have a canonical, platform-independent byte form. This
//! is the reproducible foundation for the larger convergence campaigns: a failure can name the
//! exact input instead of relying on the machine that happened to find it.

mod campaign;
mod invariant;
mod minimize;
mod schedule;
mod transport;

pub use crate::campaign::{
    run_campaign, run_mutated_campaign, CampaignConfig, CampaignConfigError, CampaignFailure,
    CampaignMode, CampaignReport, MAX_CAMPAIGN_DELIVERIES,
};
pub use crate::invariant::{Invariant, InvariantReport, InvariantViolation};
pub use crate::minimize::minimize_failure;
pub use crate::schedule::{
    FailureRecord, OperationFamily, ReproductionError, Schedule, ScheduledChange, Seed,
    SimulationConfig, SimulationConfigError, SimulationResult, StateMutant, MIN_SMOKE_STEPS,
    SIMULATOR_PROTOCOL_VERSION,
};
pub use crate::transport::{
    run_fault_campaign, run_mutated_fault_campaign, run_transport_campaign, DeterministicTransport,
    Transport, TransportCampaignReport, TransportDefect, TransportEvent, TransportFailure,
    TransportFault, TransportViolation,
};

/// The crate's name.
pub const CRATE_NAME: &str = "mesh-simulator";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_stable() {
        assert_eq!(CRATE_NAME, "mesh-simulator");
    }
}
