//! Replaceable transport, deterministic faults, recovery and minimized failure coverage.

use std::process::Command;

use mesh_simulator::{
    run_fault_campaign, run_mutated_fault_campaign, run_transport_campaign, CampaignConfig,
    DeterministicTransport, Schedule, Seed, SimulationConfig, Transport, TransportDefect,
    TransportEvent, TransportFault, TransportViolation,
};

fn config(first_seed: u64, cases: usize, steps: usize) -> CampaignConfig {
    CampaignConfig::new(
        Seed::new(first_seed),
        cases,
        SimulationConfig::new(4, steps, 128).expect("valid schedule configuration"),
    )
    .expect("bounded campaign")
}

#[test]
fn standing_fault_campaign_converges_across_seeded_schedules() {
    let config = config(0, 32, 128);
    let first = run_fault_campaign(config);
    let second = run_fault_campaign(config);

    assert!(first.is_clean(), "{:?}", first.failures());
    assert_eq!(first, second);
    assert_eq!(first.canonical_bytes(), second.canonical_bytes());
    assert_eq!(
        first.transports(),
        ["loss", "duplication", "reordering", "disconnect-reconnect"]
    );
}

#[test]
fn each_fault_has_one_exact_deterministic_event_script() {
    let schedule = Schedule::generate(Seed::new(7), config(7, 1, 8).simulation());
    let cases = [
        (
            TransportFault::Loss,
            vec![
                TransportEvent::Deliver(0),
                TransportEvent::Deliver(1),
                TransportEvent::Deliver(2),
                TransportEvent::Deliver(3),
                TransportEvent::Deliver(4),
                TransportEvent::Deliver(5),
                TransportEvent::Deliver(6),
                TransportEvent::Drop(7),
                TransportEvent::Deliver(7),
            ],
        ),
        (
            TransportFault::Duplication,
            vec![
                TransportEvent::Deliver(0),
                TransportEvent::Deliver(1),
                TransportEvent::Deliver(2),
                TransportEvent::Deliver(3),
                TransportEvent::Deliver(4),
                TransportEvent::Deliver(5),
                TransportEvent::Deliver(6),
                TransportEvent::Deliver(7),
                TransportEvent::Deliver(7),
            ],
        ),
        (
            TransportFault::Reordering,
            vec![
                TransportEvent::Deliver(0),
                TransportEvent::Deliver(1),
                TransportEvent::Deliver(2),
                TransportEvent::Deliver(3),
                TransportEvent::Deliver(4),
                TransportEvent::Deliver(5),
                TransportEvent::Deliver(7),
                TransportEvent::Deliver(6),
            ],
        ),
        (
            TransportFault::DisconnectReconnect,
            vec![
                TransportEvent::Deliver(0),
                TransportEvent::Deliver(1),
                TransportEvent::Deliver(2),
                TransportEvent::Deliver(3),
                TransportEvent::Disconnect,
                TransportEvent::Queue(4),
                TransportEvent::Queue(5),
                TransportEvent::Queue(6),
                TransportEvent::Queue(7),
                TransportEvent::Reconnect,
            ],
        ),
    ];

    for (fault, expected) in cases {
        assert_eq!(
            DeterministicTransport::new(fault).events(&schedule),
            expected
        );
    }
}

#[test]
fn every_planted_recovery_defect_is_detected_and_minimized() {
    let config = config(11, 2, 64);
    let expected_minimal_lengths = [1, 1, 2, 1];
    for (defect, expected_length) in TransportDefect::ALL
        .into_iter()
        .zip(expected_minimal_lengths)
    {
        let first = run_mutated_fault_campaign(config, defect);
        let second = run_mutated_fault_campaign(config, defect);
        assert_eq!(first, second, "{defect:?}");
        assert_eq!(first.failures().len(), 2, "{defect:?}");
        for failure in first.failures() {
            assert_eq!(failure.transport(), defect.fault().as_str());
            assert_eq!(
                failure.reproduction().minimal_reproduction().len(),
                expected_length,
                "{defect:?}"
            );
            let source = Schedule::generate(failure.seed(), config.simulation());
            failure
                .reproduction()
                .reproduction(&source)
                .expect("minimized transport failure replays from exact source");
        }
    }
}

#[derive(Debug)]
struct OutOfRange;

impl Transport for OutOfRange {
    fn name(&self) -> &'static str {
        "out-of-range-test"
    }

    fn events(&self, schedule: &Schedule) -> Vec<TransportEvent> {
        vec![TransportEvent::Deliver(schedule.changes().len())]
    }
}

#[derive(Debug)]
struct Amplifying;

impl Transport for Amplifying {
    fn name(&self) -> &'static str {
        "amplifying-test"
    }

    fn events(&self, schedule: &Schedule) -> Vec<TransportEvent> {
        vec![TransportEvent::Deliver(0); schedule.changes().len() * 2 + 5]
    }
}

#[test]
fn replacement_transport_fails_closed_on_invalid_events() {
    let report = run_transport_campaign(config(3, 1, 16), &OutOfRange);
    assert!(!report.is_clean());
    let violations = report.failures()[0].violations();
    assert!(violations.contains(&TransportViolation::DeliveryOutOfRange));
    assert!(violations.contains(&TransportViolation::MissingDelivery));

    let amplified = run_transport_campaign(config(3, 1, 16), &Amplifying);
    assert_eq!(
        amplified.failures()[0].violations(),
        [TransportViolation::EventBudgetExceeded]
    );
}

#[test]
fn canonical_report_and_real_binary_are_byte_identical() {
    let config = CampaignConfig::new(
        Seed::new(7),
        1,
        SimulationConfig::new(3, 8, 96).expect("valid schedule configuration"),
    )
    .expect("bounded campaign");
    let report = run_fault_campaign(config);
    assert_eq!(
        report.canonical_bytes(),
        b"mesh-simulator-transport/0\n\
          simulator_protocol=mesh-simulator/1\n\
          transports=loss,duplication,reordering,disconnect-reconnect\n\
          first_seed=7\n\
          cases=1\n\
          actors=3\n\
          steps=8\n\
          overlap_per_256=96\n\
          deliveries=32\n\
          status=clean\n\
          failure_count=0\n"
    );

    let output = Command::new(env!("CARGO_BIN_EXE_mesh-simulator-transport"))
        .args(["7", "1", "3", "8", "96"])
        .output()
        .expect("transport campaign binary runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, report.canonical_bytes());
    assert!(output.stderr.is_empty());
}
