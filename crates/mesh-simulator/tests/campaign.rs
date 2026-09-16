//! Deterministic campaign execution, reporting, bounds and binary coverage.

use std::process::Command;

use mesh_simulator::{
    run_campaign, run_mutated_campaign, CampaignConfig, CampaignConfigError, Schedule, Seed,
    SimulationConfig, StateMutant, MAX_CAMPAIGN_DELIVERIES,
};

fn config(first_seed: u64, cases: usize, steps: usize) -> CampaignConfig {
    CampaignConfig::new(
        Seed::new(first_seed),
        cases,
        SimulationConfig::new(4, steps, 128).expect("valid schedule configuration"),
    )
    .expect("bounded campaign configuration")
}

#[test]
fn production_campaign_is_clean_and_byte_deterministic() {
    let config = config(40, 32, 128);
    let first = run_campaign(config);
    let second = run_campaign(config);

    assert!(first.is_clean());
    assert_eq!(first, second);
    assert_eq!(first.canonical_bytes(), second.canonical_bytes());
    let report = String::from_utf8(first.canonical_bytes()).expect("ASCII report");
    assert!(report.contains("first_seed=40\ncases=32"), "{report}");
    assert!(report.contains("deliveries=4096\nstatus=clean"), "{report}");
}

#[test]
fn canonical_clean_report_has_one_exact_portable_shape() {
    let report = run_campaign(config_with(7, 1, 3, 8, 96));
    assert_eq!(
        report.canonical_bytes(),
        b"mesh-simulator-campaign/0\n\
          simulator_protocol=mesh-simulator/1\n\
          mode=audit\n\
          first_seed=7\n\
          cases=1\n\
          actors=3\n\
          steps=8\n\
          overlap_per_256=96\n\
          deliveries=8\n\
          status=clean\n\
          failure_count=0\n"
    );
}

#[test]
fn every_planted_defect_is_reported_and_replays_from_the_exact_source() {
    let config = config(9, 3, 64);
    for mutant in StateMutant::ALL {
        let report = run_mutated_campaign(config, mutant);
        assert_eq!(report.failures().len(), 3, "{mutant:?}");
        for failure in report.failures() {
            let source = Schedule::generate(failure.seed(), config.simulation());
            let minimal = failure
                .reproduction()
                .reproduction(&source)
                .expect("campaign failure replays against its exact schedule");
            assert!(!minimal.audit_mutated(mutant).is_clean(), "{mutant:?}");
            for removed in 0..failure.reproduction().minimal_reproduction().len() {
                let mut indexes = failure.reproduction().minimal_reproduction().to_vec();
                indexes.remove(removed);
                let smaller = source
                    .reproduction(&indexes)
                    .expect("a subset of canonical indexes remains canonical");
                assert!(
                    smaller.audit_mutated(mutant).is_clean(),
                    "{mutant:?} was not one-minimal"
                );
            }
        }
        let text = String::from_utf8(report.canonical_bytes()).expect("ASCII report");
        assert!(
            text.contains(&format!("mode={}", mutant.as_str())),
            "{text}"
        );
        assert!(text.contains("status=failed\nfailure_count=3"), "{text}");
    }
}

#[test]
fn campaign_work_and_seed_ranges_fail_closed_before_execution() {
    let simulation = SimulationConfig::new(2, 100, 0).expect("valid schedule configuration");
    assert_eq!(
        CampaignConfig::new(Seed::new(0), 0, simulation),
        Err(CampaignConfigError::Empty)
    );
    assert_eq!(
        CampaignConfig::new(Seed::new(u64::MAX), 2, simulation),
        Err(CampaignConfigError::SeedRangeOverflow)
    );
    assert_eq!(
        CampaignConfig::new(
            Seed::new(0),
            MAX_CAMPAIGN_DELIVERIES / simulation.steps() + 1,
            simulation,
        ),
        Err(CampaignConfigError::TooManyDeliveries)
    );
}

#[test]
fn real_binary_prints_the_same_canonical_report() {
    let output = Command::new(env!("CARGO_BIN_EXE_mesh-simulator-campaign"))
        .args(["7", "4", "3", "32", "96"])
        .output()
        .expect("campaign binary runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        run_campaign(config_with(7, 4, 3, 32, 96)).canonical_bytes()
    );
    assert!(output.stderr.is_empty());
}

fn config_with(
    first_seed: u64,
    cases: usize,
    actors: u16,
    steps: usize,
    overlap: u16,
) -> CampaignConfig {
    CampaignConfig::new(
        Seed::new(first_seed),
        cases,
        SimulationConfig::new(actors, steps, overlap).expect("valid schedule configuration"),
    )
    .expect("bounded campaign configuration")
}
