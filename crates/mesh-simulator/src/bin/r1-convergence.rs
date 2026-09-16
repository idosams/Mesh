//! Deterministic command-line replay for one R1 seed or an explicit larger campaign.

#[path = "../../tests/support/r1.rs"]
mod r1;

use std::env;
use std::process::ExitCode;
use std::time::Instant;

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("r1-convergence: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: Vec<String>) -> Result<(), String> {
    match arguments.as_slice() {
        [flag, seed] if flag == "--seed" => {
            let seed = parse(seed, "seed")?;
            run_seed(seed, 30, 5, 8)
        }
        [flag, seed, n_flag, count, actors_flag, actors, peers_flag, peers]
            if flag == "--seed"
                && n_flag == "--n"
                && actors_flag == "--actors"
                && peers_flag == "--peers" =>
        {
            run_seed(
                parse(seed, "seed")?,
                parse(count, "n")?,
                parse(actors, "actors")?,
                parse(peers, "peers")?,
            )
        }
        [flag, n, seeds, actors, peers] if flag == "--campaign" => run_campaign(
            parse(n, "n")?,
            parse(seeds, "seed count")?,
            parse(actors, "actors")?,
            parse(peers, "peers")?,
        ),
        [] => run_campaign(30, 64, 5, 8),
        _ => Err(
            "usage: r1-convergence [--seed SEED [--n N --actors N --peers N] | --campaign N SEEDS ACTORS PEERS]"
                .to_owned(),
        ),
    }
}

fn run_seed(seed: u64, count: usize, actors: usize, peers: usize) -> Result<(), String> {
    let outcome = r1::randomized_case(seed, count, actors, peers)
        .map_err(|error| format!("seed {seed}: {error}"))?;
    println!(
        "seed={seed} n={count} actors={actors} peers={peers} head={} applied={}",
        outcome.head,
        outcome.applied.len()
    );
    Ok(())
}

fn run_campaign(count: usize, seeds: usize, actors: usize, peers: usize) -> Result<(), String> {
    let started = Instant::now();
    for seed in 0..seeds {
        run_seed(seed as u64, count, actors, peers)?;
    }
    eprintln!(
        "campaign n={count} seeds={seeds} actors={actors} peers={peers} elapsed={:?}",
        started.elapsed()
    );
    Ok(())
}

fn parse<T>(text: &str, name: &str) -> Result<T, String>
where
    T: std::str::FromStr,
{
    text.parse().map_err(|_| format!("invalid {name}: {text}"))
}
