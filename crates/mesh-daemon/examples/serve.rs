//! Run the Mesh background service on a local endpoint, so that something can connect to it.
//!
//! ```text
//! cargo run -p mesh-daemon --example serve
//! cargo run -p mesh-daemon --example serve -- --endpoint /tmp/mesh/daemon.sock --seconds 30
//! ```
//!
//! # Why this is an example and not a binary
//!
//! `crates/mesh-daemon` has no `[[bin]]` today and the task that gives it one is a different lane
//! working in the same directory this round. An `examples/` target is auto-discovered by Cargo, so
//! this file adds a runnable process without touching `Cargo.toml`, `src/lib.rs` or `src/ipc/`,
//! and it therefore cannot collide with the binary when it lands. The day it does land, the
//! desktop window in `apps/desktop` connects to it without a change: what the two ends agree on is
//! the endpoint path and `crates/mesh-daemon/ipc-contract.json`, neither of which is defined here.
//!
//! # What it actually serves, stated rather than implied
//!
//! Every byte on the socket is the real surface: [`mesh_daemon::ipc::server::IpcServer`] binds it,
//! [`mesh_daemon::ipc::surface::Conversation`] answers it, and the three methods in
//! [`mesh_daemon::ipc::surface::METHODS`] are the whole catalogue. What is NOT real is the
//! workspace behind it: there is no store on disk yet, so start-up recovery has nothing to
//! recover and `startup.report` says so — `0 saved changes were read back` is a measurement of an
//! empty machine, not a placeholder. `elapsed_ms` is measured on this process.
//!
//! # The endpoint
//!
//! `--endpoint <path>`, else `MESH_DAEMON_ENDPOINT`, else `$HOME/.mesh/run/daemon.sock`. The same
//! three rules, in the same order, are implemented in `apps/desktop/src/app/endpoint.ts`, which is
//! how the window finds this process with no configuration. The containing directory is made
//! owner-only by `IpcServer::bind`; that is the whole of the access control, and
//! `crates/mesh-daemon/src/ipc/server.rs` says what it does and does not buy.

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    unix::run()
}

#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    // `crates/mesh-daemon/src/ipc/server.rs` is `unix`-only and a named-pipe backend is not
    // written yet. Saying so and exiting is better than a process that binds nothing and looks up.
    eprintln!("The Mesh background service runs on a Unix-domain socket, which this platform does not have.");
    std::process::ExitCode::from(2)
}

#[cfg(unix)]
mod unix {
    use std::path::PathBuf;
    use std::process::ExitCode;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use mesh_daemon::ipc::server::IpcServer;
    use mesh_daemon::ipc::surface::{
        nothing_to_recover, Operations, RecoveredDaemon, StartupSummary, METHODS,
    };
    use mesh_daemon::ipc::SURFACE_VERSION;
    use mesh_daemon::{RecoveryDiagnostic, RECOVERY_BUDGET};

    /// How long the idle loop sleeps between checks when no time limit was given.
    const IDLE_TICK: Duration = Duration::from_millis(200);

    /// What the process was asked to do.
    struct Options {
        endpoint: PathBuf,
        seconds: Option<u64>,
        help: bool,
    }

    impl Options {
        /// Read the arguments after the program name.
        fn parse(arguments: &[String]) -> Result<Self, String> {
            let mut endpoint: Option<PathBuf> = None;
            let mut seconds: Option<u64> = None;
            let mut help = false;
            let mut rest = arguments.iter();
            while let Some(argument) = rest.next() {
                match argument.as_str() {
                    "--help" | "-h" => help = true,
                    "--endpoint" => {
                        let value = rest.next().ok_or("`--endpoint` needs a path")?;
                        endpoint = Some(PathBuf::from(value));
                    }
                    "--seconds" => {
                        let value = rest.next().ok_or("`--seconds` needs a whole number")?;
                        seconds =
                            Some(value.parse::<u64>().map_err(|_| {
                                format!("`{value}` is not a whole number of seconds")
                            })?);
                    }
                    other => {
                        return Err(format!("`{other}` is not an argument this example takes"))
                    }
                }
            }
            Ok(Self {
                endpoint: endpoint.unwrap_or_else(default_endpoint),
                seconds,
                help,
            })
        }
    }

    /// Where the service listens when nothing says otherwise.
    ///
    /// `MESH_DAEMON_ENDPOINT` first, so one shell variable can point a window and a service at the
    /// same socket; then `$HOME/.mesh/run/daemon.sock`, which is predictable enough that the two
    /// commands in the README need no arguments at all. The temporary directory is the last
    /// resort, for an environment with no home.
    fn default_endpoint() -> PathBuf {
        if let Some(explicit) = std::env::var_os("MESH_DAEMON_ENDPOINT") {
            if !explicit.is_empty() {
                return PathBuf::from(explicit);
            }
        }
        let base = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        base.join(".mesh").join("run").join("daemon.sock")
    }

    /// Serve until the time limit runs out, or until the process is stopped.
    pub fn run() -> ExitCode {
        let arguments: Vec<String> = std::env::args().skip(1).collect();
        let options = match Options::parse(&arguments) {
            Ok(options) => options,
            Err(reason) => {
                eprintln!("{reason}");
                usage();
                return ExitCode::from(2);
            }
        };
        if options.help {
            usage();
            return ExitCode::SUCCESS;
        }

        // The start-up story the surface reports, measured on this process rather than asserted.
        // There is no store on disk, so the outcome is "nothing to recover" and the elapsed time
        // is however long that took — which is the honest answer while the daemon composes one
        // crate.
        let started = Instant::now();
        let diagnostic = RecoveryDiagnostic::new(
            nothing_to_recover().outcome().clone(),
            started.elapsed(),
            RECOVERY_BUDGET,
        );
        let summary = StartupSummary::from(&diagnostic);
        let operations: Arc<dyn Operations> = Arc::new(RecoveredDaemon::new(diagnostic));

        let server = match IpcServer::bind(&options.endpoint) {
            Ok(server) => server,
            Err(error) => {
                eprintln!(
                    "Could not open the local endpoint at {}: {error}",
                    options.endpoint.display()
                );
                return ExitCode::from(1);
            }
        };
        let handle = match server.spawn(operations) {
            Ok(handle) => handle,
            Err(error) => {
                eprintln!("Could not start serving: {error}");
                return ExitCode::from(1);
            }
        };

        announce(handle.endpoint(), &summary);

        match options.seconds {
            Some(limit) => std::thread::sleep(Duration::from_secs(limit)),
            None => loop {
                std::thread::sleep(IDLE_TICK);
            },
        }
        let served = handle.connections_served();
        handle.shutdown();
        println!("The Mesh background service has stopped. It served {served} connections.");
        ExitCode::SUCCESS
    }

    /// Print what a person needs to see once the endpoint is open.
    fn announce(endpoint: &std::path::Path, summary: &StartupSummary) {
        println!("The Mesh background service is running.");
        println!();
        println!("  local endpoint       {}", endpoint.display());
        println!("  service interface    version {SURFACE_VERSION}");
        println!("  serving requests     {}", yes_or_no(summary.serving));
        println!("  start-up took        {} ms", summary.elapsed_ms);
        println!("  start-up said        {}", summary.sentence);
        println!();
        println!("  it can answer:");
        for entry in METHODS {
            println!("    {:<18} {}", entry.name, entry.summary);
        }
        println!();
        println!("Open the Mesh window on it with:");
        println!(
            "  npm --prefix apps/desktop start -- --endpoint {}",
            endpoint.display()
        );
        println!();
        println!("Stop this service with Ctrl-C.");
    }

    /// A word rather than a boolean, because this line is read by a person.
    const fn yes_or_no(value: bool) -> &'static str {
        if value {
            "yes"
        } else {
            "no"
        }
    }

    /// What the arguments are.
    fn usage() {
        println!("Run the Mesh background service on a local endpoint.");
        println!();
        println!("  cargo run -p mesh-daemon --example serve [-- OPTIONS]");
        println!();
        println!("  --endpoint <path>   where to listen; defaults to MESH_DAEMON_ENDPOINT,");
        println!("                      then to $HOME/.mesh/run/daemon.sock");
        println!("  --seconds <n>       stop after n seconds instead of running until stopped");
        println!("  --help              this text");
    }
}
