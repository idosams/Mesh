//! A replaceable, offline transport model and deterministic fault campaigns.
//!
//! A transport emits events over schedule indexes. It has no socket, clock,
//! thread, filesystem or retry timer. The executor is the shared subsystem: it
//! enforces connection state, queues disconnected deliveries, applies delivered
//! changes through the real state fold, and checks eventual coverage,
//! convergence and duplicate idempotence.

use std::collections::BTreeSet;

use crate::{
    minimize_failure, CampaignConfig, FailureRecord, Schedule, Seed, SIMULATOR_PROTOCOL_VERSION,
};

const TRANSPORT_REPORT_VERSION: &str = "mesh-simulator-transport/0";

/// One event emitted by a replaceable transport implementation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportEvent {
    /// Deliver one source schedule entry while connected.
    Deliver(usize),
    /// Lose one attempted source schedule entry.
    Drop(usize),
    /// Enter the disconnected state.
    Disconnect,
    /// Hold one source schedule entry while disconnected.
    Queue(usize),
    /// Reconnect and flush queued entries in their exact queued order.
    Reconnect,
}

/// A replaceable deterministic source of transport events.
pub trait Transport {
    /// Stable report name for this implementation.
    fn name(&self) -> &'static str;

    /// Complete deterministic event sequence for one generated schedule.
    fn events(&self, schedule: &Schedule) -> Vec<TransportEvent>;
}

/// Fault families the standing transport campaign exercises.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TransportFault {
    /// One attempt is lost and retried after the remaining traffic.
    Loss,
    /// One exact delivery is repeated.
    Duplication,
    /// One adjacent pair is delivered in reverse order.
    Reordering,
    /// The second half is queued while disconnected and flushed on reconnect.
    DisconnectReconnect,
}

impl TransportFault {
    /// Every standing family in canonical report order.
    pub const ALL: [Self; 4] = [
        Self::Loss,
        Self::Duplication,
        Self::Reordering,
        Self::DisconnectReconnect,
    ];

    /// Stable report word.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Loss => "loss",
            Self::Duplication => "duplication",
            Self::Reordering => "reordering",
            Self::DisconnectReconnect => "disconnect-reconnect",
        }
    }
}

/// Built-in deterministic transport for one standing fault family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeterministicTransport {
    fault: TransportFault,
    defect: Option<TransportDefect>,
}

impl DeterministicTransport {
    /// Construct the correct recovery behavior for `fault`.
    #[must_use]
    pub const fn new(fault: TransportFault) -> Self {
        Self {
            fault,
            defect: None,
        }
    }

    const fn mutated(defect: TransportDefect) -> Self {
        Self {
            fault: defect.fault(),
            defect: Some(defect),
        }
    }
}

impl Transport for DeterministicTransport {
    fn name(&self) -> &'static str {
        self.fault.as_str()
    }

    fn events(&self, schedule: &Schedule) -> Vec<TransportEvent> {
        let length = schedule.changes().len();
        if length == 0 {
            return Vec::new();
        }
        let portable_length = u64::try_from(length).expect("campaign bounds fit into u64");
        let selected = (schedule.seed().value() % portable_length) as usize;
        match self.fault {
            TransportFault::Loss => {
                let mut events = Vec::with_capacity(length + 1);
                for delivery in 0..length {
                    if delivery == selected {
                        events.push(TransportEvent::Drop(delivery));
                    } else {
                        events.push(TransportEvent::Deliver(delivery));
                    }
                }
                if self.defect != Some(TransportDefect::ForgetLossRetry) {
                    events.push(TransportEvent::Deliver(selected));
                }
                events
            }
            TransportFault::Duplication => {
                let mut events = Vec::with_capacity(length + 1);
                for delivery in 0..length {
                    events.push(TransportEvent::Deliver(delivery));
                    if delivery == selected {
                        events.push(TransportEvent::Deliver(delivery));
                    }
                }
                events
            }
            TransportFault::Reordering => {
                if length < 2 {
                    return (0..length).map(TransportEvent::Deliver).collect();
                }
                let first = selected.min(length - 2);
                let mut order: Vec<_> = (0..length).collect();
                order.swap(first, first + 1);
                if self.defect == Some(TransportDefect::DiscardReorderedDelivery) {
                    order.remove(first);
                }
                order.into_iter().map(TransportEvent::Deliver).collect()
            }
            TransportFault::DisconnectReconnect => {
                let split = (length / 2).max(1);
                let mut events = Vec::with_capacity(length + 2);
                events.extend((0..split).map(TransportEvent::Deliver));
                events.push(TransportEvent::Disconnect);
                events.extend((split..length).map(TransportEvent::Queue));
                if self.defect != Some(TransportDefect::DiscardReconnectBacklog) {
                    events.push(TransportEvent::Reconnect);
                }
                events
            }
        }
    }
}

/// Planted recovery defects used to prove each fault campaign can fail and minimize.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportDefect {
    /// A lost attempt is never retried.
    ForgetLossRetry,
    /// An exact duplicate is reported as newly applied work.
    TreatDuplicateAsNew,
    /// One member of the reordered pair is discarded.
    DiscardReorderedDelivery,
    /// Reconnection never flushes the queued backlog.
    DiscardReconnectBacklog,
}

impl TransportDefect {
    /// Every planted defect in fault-family order.
    pub const ALL: [Self; 4] = [
        Self::ForgetLossRetry,
        Self::TreatDuplicateAsNew,
        Self::DiscardReorderedDelivery,
        Self::DiscardReconnectBacklog,
    ];

    /// Fault family this defect weakens.
    #[must_use]
    pub const fn fault(self) -> TransportFault {
        match self {
            Self::ForgetLossRetry => TransportFault::Loss,
            Self::TreatDuplicateAsNew => TransportFault::Duplication,
            Self::DiscardReorderedDelivery => TransportFault::Reordering,
            Self::DiscardReconnectBacklog => TransportFault::DisconnectReconnect,
        }
    }
}

/// One violated transport property.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TransportViolation {
    /// An event named an index outside the source schedule.
    DeliveryOutOfRange,
    /// A delivery was attempted while the transport was disconnected.
    DeliveryWhileDisconnected,
    /// Disconnect, queue or reconnect appeared in an impossible state.
    InvalidConnectionTransition,
    /// The event sequence ended with an unflushed disconnected backlog.
    LeftDisconnected,
    /// At least one source delivery never reached the state fold.
    MissingDelivery,
    /// The eventual state differs from direct delivery of the source schedule.
    DivergentState,
    /// An exact repeated delivery was admitted to the state fold twice.
    DuplicateNotIdempotent,
    /// A replaceable transport exceeded the deterministic event budget.
    EventBudgetExceeded,
}

impl TransportViolation {
    /// Stable report word.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DeliveryOutOfRange => "delivery-out-of-range",
            Self::DeliveryWhileDisconnected => "delivery-while-disconnected",
            Self::InvalidConnectionTransition => "invalid-connection-transition",
            Self::LeftDisconnected => "left-disconnected",
            Self::MissingDelivery => "missing-delivery",
            Self::DivergentState => "divergent-state",
            Self::DuplicateNotIdempotent => "duplicate-not-idempotent",
            Self::EventBudgetExceeded => "event-budget-exceeded",
        }
    }
}

/// One failing seed/transport pair with its one-minimal reproduction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransportFailure {
    transport: &'static str,
    seed: Seed,
    violations: Vec<TransportViolation>,
    reproduction: FailureRecord,
}

impl TransportFailure {
    /// Stable transport name.
    #[must_use]
    pub const fn transport(&self) -> &'static str {
        self.transport
    }

    /// Failing generated seed.
    #[must_use]
    pub const fn seed(&self) -> Seed {
        self.seed
    }

    /// Violations in canonical declaration order.
    #[must_use]
    pub fn violations(&self) -> &[TransportViolation] {
        &self.violations
    }

    /// Exact source schedule and minimized delivery selection.
    #[must_use]
    pub const fn reproduction(&self) -> &FailureRecord {
        &self.reproduction
    }
}

/// Deterministic result across one or more replaceable transports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransportCampaignReport {
    config: CampaignConfig,
    transports: Vec<&'static str>,
    failures: Vec<TransportFailure>,
}

impl TransportCampaignReport {
    /// Validated campaign inputs.
    #[must_use]
    pub const fn config(&self) -> CampaignConfig {
        self.config
    }

    /// Replaceable transport names in execution order.
    #[must_use]
    pub fn transports(&self) -> &[&'static str] {
        &self.transports
    }

    /// Whether every transport converged for every seed.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.failures.is_empty()
    }

    /// Failures ordered by transport declaration, then seed.
    #[must_use]
    pub fn failures(&self) -> &[TransportFailure] {
        &self.failures
    }

    /// Stable platform-independent campaign report.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let simulation = self.config.simulation();
        let mut text = format!(
            "{TRANSPORT_REPORT_VERSION}\n\
             simulator_protocol={SIMULATOR_PROTOCOL_VERSION}\n\
             transports={}\n\
             first_seed={}\n\
             cases={}\n\
             actors={}\n\
             steps={}\n\
             overlap_per_256={}\n\
             deliveries={}\n\
             status={}\n\
             failure_count={}\n",
            self.transports.join(","),
            self.config.first_seed().value(),
            self.config.case_count(),
            simulation.actor_count(),
            simulation.steps(),
            simulation.overlap_per_256(),
            self.config.delivery_count() * self.transports.len(),
            if self.is_clean() { "clean" } else { "failed" },
            self.failures.len(),
        );
        for failure in &self.failures {
            text.push_str("failure=transport:");
            text.push_str(failure.transport());
            text.push_str("|seed:");
            text.push_str(&failure.seed().value().to_string());
            text.push_str("|violations:");
            for (index, violation) in failure.violations().iter().enumerate() {
                if index > 0 {
                    text.push(',');
                }
                text.push_str(violation.as_str());
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

/// Run every standing fault family for every configured seed.
#[must_use]
pub fn run_fault_campaign(config: CampaignConfig) -> TransportCampaignReport {
    let transports = TransportFault::ALL.map(DeterministicTransport::new);
    let references: Vec<&dyn Transport> = transports
        .iter()
        .map(|transport| transport as &dyn Transport)
        .collect();
    run_transports(config, &references, None)
}

/// Run one replaceable transport implementation for every configured seed.
#[must_use]
pub fn run_transport_campaign(
    config: CampaignConfig,
    transport: &dyn Transport,
) -> TransportCampaignReport {
    run_transports(config, &[transport], None)
}

/// Run the fault family corresponding to one planted recovery defect.
#[must_use]
pub fn run_mutated_fault_campaign(
    config: CampaignConfig,
    defect: TransportDefect,
) -> TransportCampaignReport {
    let transport = DeterministicTransport::mutated(defect);
    run_transports(config, &[&transport], Some(defect))
}

fn run_transports(
    config: CampaignConfig,
    transports: &[&dyn Transport],
    defect: Option<TransportDefect>,
) -> TransportCampaignReport {
    let mut failures = Vec::new();
    for transport in transports {
        for offset in 0..config.case_count() {
            let seed = Seed::new(config.first_seed().value() + offset as u64);
            let schedule = Schedule::generate(seed, config.simulation());
            let violations = audit_transport(&schedule, *transport, defect);
            if violations.is_empty() {
                continue;
            }
            let reproduction = minimize_failure(&schedule, |candidate| {
                !audit_transport(candidate, *transport, defect).is_empty()
            })
            .expect("the same transport failure was checked before minimization");
            failures.push(TransportFailure {
                transport: transport.name(),
                seed,
                violations,
                reproduction,
            });
        }
    }
    TransportCampaignReport {
        config,
        transports: transports
            .iter()
            .map(|transport| transport.name())
            .collect(),
        failures,
    }
}

fn audit_transport(
    schedule: &Schedule,
    transport: &dyn Transport,
    defect: Option<TransportDefect>,
) -> Vec<TransportViolation> {
    let events = transport.events(schedule);
    let event_budget = schedule.changes().len().saturating_mul(2).saturating_add(4);
    if events.len() > event_budget {
        return vec![TransportViolation::EventBudgetExceeded];
    }

    let mut connected = true;
    let mut pending = Vec::new();
    let mut delivered = Vec::new();
    let mut accepted = BTreeSet::new();
    let mut violations = Vec::new();
    for event in events {
        match event {
            TransportEvent::Deliver(delivery) => {
                if !valid(schedule, delivery, &mut violations) {
                    continue;
                }
                if connected {
                    accept(
                        delivery,
                        defect,
                        &mut accepted,
                        &mut delivered,
                        &mut violations,
                    );
                } else {
                    violations.push(TransportViolation::DeliveryWhileDisconnected);
                }
            }
            TransportEvent::Drop(delivery) => {
                valid(schedule, delivery, &mut violations);
            }
            TransportEvent::Disconnect => {
                if connected {
                    connected = false;
                } else {
                    violations.push(TransportViolation::InvalidConnectionTransition);
                }
            }
            TransportEvent::Queue(delivery) => {
                if !valid(schedule, delivery, &mut violations) {
                    continue;
                }
                if connected {
                    violations.push(TransportViolation::InvalidConnectionTransition);
                } else {
                    pending.push(delivery);
                }
            }
            TransportEvent::Reconnect => {
                if connected {
                    violations.push(TransportViolation::InvalidConnectionTransition);
                } else {
                    connected = true;
                    for delivery in pending.drain(..) {
                        accept(
                            delivery,
                            defect,
                            &mut accepted,
                            &mut delivered,
                            &mut violations,
                        );
                    }
                }
            }
        }
    }
    if !connected {
        violations.push(TransportViolation::LeftDisconnected);
    }

    let delivered_set: BTreeSet<_> = delivered.iter().copied().collect();
    if delivered_set.len() != schedule.changes().len() {
        violations.push(TransportViolation::MissingDelivery);
    }
    let transported = schedule
        .in_delivery_order(&delivered)
        .expect("only validated delivery indexes enter the fold");
    let result = transported.run();
    if result.state() != schedule.run().state() {
        violations.push(TransportViolation::DivergentState);
    }
    violations.sort_unstable();
    violations.dedup();
    violations
}

fn accept(
    delivery: usize,
    defect: Option<TransportDefect>,
    accepted: &mut BTreeSet<usize>,
    delivered: &mut Vec<usize>,
    violations: &mut Vec<TransportViolation>,
) {
    if accepted.insert(delivery) {
        delivered.push(delivery);
    } else if defect == Some(TransportDefect::TreatDuplicateAsNew) {
        delivered.push(delivery);
        violations.push(TransportViolation::DuplicateNotIdempotent);
    }
}

fn valid(schedule: &Schedule, delivery: usize, violations: &mut Vec<TransportViolation>) -> bool {
    if delivery < schedule.changes().len() {
        true
    } else {
        violations.push(TransportViolation::DeliveryOutOfRange);
        false
    }
}
