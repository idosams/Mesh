//! Local stdio entry point for default workspace inspection or native-scoped fleet tools.

use mesh_mcp::{serve, DaemonWorkspaceState};
use std::env;
use std::io::{self, BufReader};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "Usage: mesh-mcp [--endpoint <absolute-unix-socket>]\n\
\n\
Expose the currently open Mesh workspace as one read-only MCP tool.\n\
The endpoint defaults to MESH_DAEMON_ENDPOINT, then the platform Mesh app socket.\n";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(problem) => {
            eprintln!("mesh-mcp: {problem}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() == 1 && matches!(args[0].as_str(), "-h" | "--help") {
        print!("{USAGE}");
        return Ok(());
    }
    let environment: Vec<(String, String)> = env::vars().collect();
    let endpoint = endpoint_from(&args, &environment)?;
    let objective = env::var("MESH_FLEET_OBJECTIVE").ok();
    let credential = env::var("MESH_FLEET_CREDENTIAL").ok();
    let provider = match (objective, credential) {
        (Some(objective), Some(credential)) => {
            DaemonWorkspaceState::fleet(endpoint, objective, credential)?
        }
        (None, None) => DaemonWorkspaceState::new(endpoint),
        _ => {
            return Err(
                "The native fleet objective and credential must be supplied together".into(),
            )
        }
    };
    serve(
        BufReader::new(io::stdin().lock()),
        io::stdout().lock(),
        &provider,
    )
}

fn endpoint_from(args: &[String], environment: &[(String, String)]) -> Result<PathBuf, String> {
    let mut endpoint = None;
    let mut index = 0;
    while index < args.len() {
        if args[index] != "--endpoint" {
            return Err(format!("unknown argument `{}`\n{USAGE}", args[index]));
        }
        if endpoint.is_some() {
            return Err("`--endpoint` may be supplied only once".to_owned());
        }
        let value = args
            .get(index + 1)
            .filter(|value| !value.is_empty() && !value.starts_with('-'))
            .ok_or("`--endpoint` requires a non-empty path value")?;
        endpoint = Some(PathBuf::from(value));
        index += 2;
    }
    if let Some(endpoint) = endpoint {
        return Ok(endpoint);
    }
    if let Some(value) = environment
        .iter()
        .find_map(|(name, value)| (name == "MESH_DAEMON_ENDPOINT").then_some(value))
        .filter(|value| !value.is_empty())
    {
        return Ok(PathBuf::from(value));
    }
    let home = environment
        .iter()
        .find_map(|(name, value)| (name == "HOME").then_some(value))
        .filter(|value| !value.is_empty())
        .ok_or("HOME is unavailable; pass --endpoint explicitly")?;
    #[cfg(target_os = "macos")]
    let endpoint = PathBuf::from(home)
        .join("Library/Application Support/dev.mesh.desktop/runtime/daemon.sock");
    #[cfg(not(target_os = "macos"))]
    let endpoint = PathBuf::from(home).join(".mesh/run/daemon.sock");
    Ok(endpoint)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(entries: &[(&str, &str)]) -> Vec<(String, String)> {
        entries
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn explicit_endpoint_wins() {
        let endpoint = endpoint_from(
            &["--endpoint".to_owned(), "/real/mesh.sock".to_owned()],
            &values(&[("HOME", "/home/me"), ("MESH_DAEMON_ENDPOINT", "/env.sock")]),
        )
        .expect("endpoint");
        assert_eq!(endpoint, PathBuf::from("/real/mesh.sock"));
    }

    #[test]
    fn missing_repeated_and_option_values_are_refused() {
        assert!(endpoint_from(&["--endpoint".to_owned()], &[]).is_err());
        assert!(endpoint_from(
            &[
                "--endpoint".to_owned(),
                "/one".to_owned(),
                "--endpoint".to_owned(),
                "/two".to_owned(),
            ],
            &[]
        )
        .is_err());
        assert!(endpoint_from(&["--endpoint".to_owned(), "--help".to_owned()], &[]).is_err());
    }
}
