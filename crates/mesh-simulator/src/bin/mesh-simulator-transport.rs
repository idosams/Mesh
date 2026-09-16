//! Run the bounded offline transport fault campaign and print its canonical report.

use std::io::Write as _;
use std::process::ExitCode;

use mesh_simulator::{run_fault_campaign, CampaignConfig, Seed, SimulationConfig};

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(problem) => {
            eprintln!("mesh-simulator-transport: {problem}");
            eprintln!(
                "Usage: mesh-simulator-transport <first-seed> <cases> <actors> <steps> <overlap-per-256>"
            );
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<bool, String> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.len() != 5 {
        return Err("exactly five numeric arguments are required".to_owned());
    }
    let values: Result<Vec<u64>, _> = arguments.iter().map(|value| value.parse::<u64>()).collect();
    let values = values.map_err(|_| "every argument must be a non-negative integer".to_owned())?;
    let simulation = SimulationConfig::new(
        u16::try_from(values[2]).map_err(|_| "actors is outside u16".to_owned())?,
        usize::try_from(values[3]).map_err(|_| "steps is outside usize".to_owned())?,
        u16::try_from(values[4]).map_err(|_| "overlap is outside u16".to_owned())?,
    )
    .map_err(|error| error.to_string())?;
    let config = CampaignConfig::new(
        Seed::new(values[0]),
        usize::try_from(values[1]).map_err(|_| "cases is outside usize".to_owned())?,
        simulation,
    )
    .map_err(|error| error.to_string())?;
    let report = run_fault_campaign(config);
    std::io::stdout()
        .write_all(&report.canonical_bytes())
        .map_err(|error| format!("could not write report: {error}"))?;
    Ok(report.is_clean())
}
