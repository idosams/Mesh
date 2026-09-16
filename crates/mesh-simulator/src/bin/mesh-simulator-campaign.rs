//! Run one bounded deterministic simulator campaign and print its canonical report.

use std::io::Write as _;
use std::process::ExitCode;

use mesh_simulator::{run_campaign, CampaignConfig, Seed, SimulationConfig};

const USAGE: &str =
    "Usage: mesh-simulator-campaign <first-seed> <cases> <actors> <steps> <overlap-per-256>\n";

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(problem) => {
            eprintln!("mesh-simulator-campaign: {problem}");
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<bool, String> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.len() != 5 {
        return Err("exactly five numeric arguments are required".to_owned());
    }
    let first_seed = parse(&arguments[0], "first-seed")?;
    let cases = parse(&arguments[1], "cases")?;
    let actors = parse(&arguments[2], "actors")?;
    let steps = parse(&arguments[3], "steps")?;
    let overlap = parse(&arguments[4], "overlap-per-256")?;
    let simulation = SimulationConfig::new(
        u16::try_from(actors).map_err(|_| "actors is outside u16".to_owned())?,
        usize::try_from(steps).map_err(|_| "steps is outside usize".to_owned())?,
        u16::try_from(overlap).map_err(|_| "overlap-per-256 is outside u16".to_owned())?,
    )
    .map_err(|error| error.to_string())?;
    let config = CampaignConfig::new(
        Seed::new(first_seed),
        usize::try_from(cases).map_err(|_| "cases is outside usize".to_owned())?,
        simulation,
    )
    .map_err(|error| error.to_string())?;
    let report = run_campaign(config);
    std::io::stdout()
        .write_all(&report.canonical_bytes())
        .map_err(|error| format!("could not write report: {error}"))?;
    Ok(report.is_clean())
}

fn parse(value: &str, name: &str) -> Result<u64, String> {
    value
        .parse()
        .map_err(|_| format!("{name} must be a non-negative integer"))
}
