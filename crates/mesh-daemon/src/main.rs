//! `meshd` — the Mesh background service, as a process you can start.
//!
//! # What it does
//!
//! Binds a Unix-domain socket under a runtime directory, opens a workspace when it was told which
//! one, and answers the local IPC surface in [`mesh_daemon::ipc`] until it is asked to stop.
//! Everything it says about a workspace is folded from the records actually on disk by
//! `mesh-store`; nothing here invents an answer.
//!
//! ```text
//! cargo run -p mesh-daemon -- --workspace ./my-workspace
//! ```
//!
//! # How it stops, and why that is standard input
//!
//! The daemon stops when its standard input reaches end of file, or when a line reading `stop`
//! arrives on it. In a terminal that is Ctrl-D; from a parent process it is closing the pipe.
//!
//! A signal handler would be the conventional answer and it is not available: installing one needs
//! `libc`, `Cargo.lock` is governance surface, and
//! `docs/adr/0014-narrow-the-lockfile-fence-to-admit-an-audited-cryptographic-dependency.md`
//! admits exactly one category of new dependency, which this is not. The standard-input channel
//! needs no dependency, is deterministic, and is something a supervising parent already holds. A
//! daemon killed by a signal instead leaves its socket file behind, and `IpcServer::bind` removes
//! a stale one on the next start, so the uncovered path is recovered rather than merely
//! unhandled.
//!
//! # What it deliberately will not do
//!
//! It will not write a record, because advancing canonical state before `mesh-policy` has been
//! consulted is not something a local socket should offer, and `mesh-policy` is not consulted
//! anywhere on this surface yet. It reports this replica's private version from the complete set
//! of saved changes. It materialises file names and folders from verified saved payloads without
//! reading the working directory; a missing payload is a recoverable partial-answer condition.
//! It reports the shared version only after the journal carries both a canonical approval receipt
//! and mesh-policy's durable HumanHeld publication decision. No record carries that decision yet,
//! so `workspace.state` keeps the obstacle visible even when reviewer public keys are configured.
//!
//! # Platforms
//!
//! Unix only, and it says so rather than pretending. The transport is
//! `mesh_daemon::ipc::server`, which is `cfg(unix)`; a Windows named-pipe backend behind the same
//! `Operations` seam is not in this pass and is not stubbed.

use std::process::ExitCode;

#[cfg(not(unix))]
fn main() -> ExitCode {
    eprintln!(
        "meshd: this build has no local transport for this platform and nothing was started. \
         The Mesh background service listens on a Unix-domain socket, and the named-pipe backend \
         is not implemented."
    );
    ExitCode::from(2)
}

#[cfg(unix)]
fn main() -> ExitCode {
    use std::io::Write as _;

    match serve::run(std::env::args().skip(1).collect()) {
        Ok(code) => code,
        Err(problem) => {
            let mut error = std::io::stderr();
            let _ = writeln!(error, "meshd: {problem}");
            let _ = writeln!(
                error,
                "meshd: nothing was changed. `meshd --help` lists the options."
            );
            ExitCode::from(2)
        }
    }
}

#[cfg(unix)]
mod serve {
    use std::io::{BufRead as _, IsTerminal as _, Write as _};
    use std::path::{Path, PathBuf};
    use std::process::ExitCode;
    use std::sync::Arc;

    use mesh_daemon::ipc::server::IpcServer;
    use mesh_daemon::ipc::surface::{nothing_to_recover, Operations, StartupSummary};
    use mesh_daemon::ipc::SURFACE_VERSION;
    use mesh_daemon::{choose_backend, Availability, CrashReport, LiveDaemon, TrustedReviewers};
    use mesh_store::CheckpointRuntimeParameters;
    use mesh_types::PublicKey;

    /// The runtime directory's name under whichever base the platform gives us.
    const RUNTIME_DIRECTORY: &str = "mesh";

    /// The socket file's name inside it.
    const SOCKET_FILE: &str = "daemon.sock";

    /// The longest a Unix-domain socket path may be on the platforms this targets.
    ///
    /// macOS allows 104 bytes and Linux 108, terminator included. Checked here rather than left to
    /// `bind`, whose error for an over-long path is an unhelpful `invalid argument`.
    const MAX_ENDPOINT_BYTES: usize = 100;

    /// How long the shutdown waits before closing the socket, so a subscribed connection is handed
    /// the closing entry first. One accept poll plus one read poll, with room to spare.
    const FAREWELL: std::time::Duration = std::time::Duration::from_millis(120);

    /// Everything the process does, with the exit code as a value rather than as a side effect.
    ///
    /// # Errors
    ///
    /// A sentence naming what could not be done, which the caller prints. Every one of them leaves
    /// nothing started and nothing changed.
    pub fn run(arguments: Vec<String>) -> Result<ExitCode, String> {
        let options = Options::parse(&arguments)?;
        if options.help {
            print!("{USAGE}");
            return Ok(ExitCode::SUCCESS);
        }
        if options.version {
            println!("meshd {}", env!("CARGO_PKG_VERSION"));
            return Ok(ExitCode::SUCCESS);
        }
        let checkpoint_parameters = options.checkpoint_parameters()?;

        let endpoint = match options.endpoint {
            Some(path) => path,
            None => default_endpoint()?,
        };
        if endpoint.as_os_str().len() > MAX_ENDPOINT_BYTES {
            return Err(format!(
                "the endpoint path is {} bytes and the platform limit is {MAX_ENDPOINT_BYTES}: {}",
                endpoint.as_os_str().len(),
                endpoint.display()
            ));
        }

        // Which mechanism this service will present a folder through, decided before anything is
        // opened and said out loud before anything is served. `choose_backend` is a total function
        // of `Availability` with no argument that could prefer the fallback over an available
        // direct connection, and `Availability::probe` answers from what is linked into this
        // binary rather than from the platform's name — acceptance criterion 4 of task
        // `01KZC2QR9VVJK6Y60PS8D360JT`.
        let backend = choose_backend(Availability::probe());
        debug_assert!(!backend.is_silent_fallback());
        let _ = writeln!(std::io::stderr(), "meshd: {backend}");

        // The start-up summary before any workspace: truthful, and not a workspace.
        let trusted_reviewers =
            TrustedReviewers::new(options.trusted_reviewer_keys.iter().copied());
        let startup = StartupSummary::from(&nothing_to_recover());
        let daemon = Arc::new(match checkpoint_parameters {
            Some(parameters) => LiveDaemon::with_trusted_reviewers_and_checkpoint_runtime(
                startup,
                trusted_reviewers,
                parameters,
            )
            .map_err(|error| format!("checkpoint configuration was refused: {error}"))?,
            None => LiveDaemon::with_trusted_reviewers(startup, trusted_reviewers),
        });
        let mut opened: Option<String> = None;
        if let Some(root) = &options.workspace {
            let started = std::time::Instant::now();
            match daemon.open_at_start(root) {
                Ok(summary) => {
                    opened = Some(summary.root);
                    // The last durable boundary, on the one path where somebody goes looking for
                    // it: the start after a crash. Standard error, so the ready line on standard
                    // output stays the single line a parent process reads.
                    if let Some(report) = daemon.crash_report() {
                        let _ = writeln!(std::io::stderr(), "meshd: {report}");
                    }
                }
                Err(failure) => {
                    // Reported, not fatal. A daemon that refuses to start is a daemon that cannot
                    // tell anybody what is wrong with the folder they asked for.
                    let mut error = std::io::stderr();
                    let _ = writeln!(
                        error,
                        "meshd: the workspace at {} could not be opened ({}): {failure}",
                        root.display(),
                        failure.code()
                    );
                    let _ = writeln!(
                        error,
                        "meshd: {}",
                        CrashReport::of_failure(&failure, started.elapsed())
                    );
                }
            }
        }
        // A save whose journal/index commit survived but whose process died before the configured
        // idle boundary resumes without a user action. The worker revalidates the exact reopened
        // workspace before reconstructing the lost acknowledgement.
        let _checkpoint_resume = daemon.schedule_pending_checkpoint();

        let server = IpcServer::bind(&endpoint)
            .map_err(|error| format!("could not listen at {}: {error}", endpoint.display()))?;
        let handle = server
            .spawn(Arc::clone(&daemon) as Arc<dyn Operations>)
            .map_err(|error| {
                format!("could not start serving at {}: {error}", endpoint.display())
            })?;

        announce(&endpoint, opened.as_deref(), daemon.as_ref(), backend);
        wait_for_stop();

        daemon.announce_stopping();
        std::thread::sleep(FAREWELL);
        handle.shutdown();
        Ok(ExitCode::SUCCESS)
    }

    /// Print one machine-readable line saying where to connect, then, on a terminal, how to stop.
    ///
    /// That line is the only thing on standard output, so a parent process can read it with a
    /// single `readline` and know the daemon is ready. Everything written for a person goes to
    /// standard error, where it cannot corrupt that contract.
    ///
    /// `backend` is on it as well as in the sentence above, and that is not duplication: a
    /// supervising process reads this line and never reads standard error, so a mechanism that
    /// appeared only in the prose would be invisible to every caller that is not a terminal.
    fn announce(
        endpoint: &Path,
        workspace: Option<&str>,
        daemon: &LiveDaemon,
        backend: mesh_daemon::BackendChoice,
    ) {
        let mut out = std::io::stdout();
        let workspace_field = workspace.map_or_else(
            || "null".to_owned(),
            |root| format!("\"{}\"", root.escape_debug()),
        );
        let _ = writeln!(
            out,
            "{{\"ready\":true,\"endpoint\":\"{}\",\"surface_version\":{SURFACE_VERSION},\
             \"workspace\":{workspace_field},\"serving\":{},\"backend\":\"{}\",\
             \"authoritative\":{}}}",
            endpoint.display().to_string().escape_debug(),
            daemon.serving(),
            backend.backend().as_str(),
            backend.backend().is_authoritative(),
        );
        let _ = out.flush();

        if std::io::stdin().is_terminal() {
            let _ = writeln!(
                std::io::stderr(),
                "Mesh is running. Press Ctrl-D to stop it."
            );
        }
    }

    /// Block until standard input reaches end of file or sends `stop`.
    fn wait_for_stop() {
        let stdin = std::io::stdin();
        let mut line = String::new();
        loop {
            line.clear();
            match stdin.lock().read_line(&mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => {
                    if line.trim().eq_ignore_ascii_case("stop") {
                        return;
                    }
                }
            }
        }
    }

    /// Where the socket goes when nobody said.
    ///
    /// `XDG_RUNTIME_DIR` when the platform sets it — per-user, owner-only and cleaned on logout,
    /// which is exactly what a local socket wants. Otherwise the temporary directory, with the
    /// user name in the path so two people on one machine do not collide. `IpcServer::bind`
    /// tightens the containing directory to owner-only either way.
    fn default_endpoint() -> Result<PathBuf, String> {
        let base = std::env::var_os("XDG_RUNTIME_DIR").map_or_else(
            || {
                let mut path = std::env::temp_dir();
                let user = std::env::var("USER").unwrap_or_else(|_| "mesh".to_owned());
                path.push(format!("{RUNTIME_DIRECTORY}-{user}"));
                path
            },
            |value| {
                let mut path = PathBuf::from(value);
                path.push(RUNTIME_DIRECTORY);
                path
            },
        );
        std::fs::create_dir_all(&base).map_err(|error| {
            format!(
                "could not make the runtime folder {}: {error}",
                base.display()
            )
        })?;
        Ok(base.join(SOCKET_FILE))
    }

    /// What the command line accepts.
    #[derive(Debug, Default, PartialEq, Eq)]
    struct Options {
        endpoint: Option<PathBuf>,
        workspace: Option<PathBuf>,
        trusted_reviewer_keys: Vec<PublicKey>,
        checkpoint_idle_ms: Option<u64>,
        checkpoint_maximum_bytes: Option<u64>,
        checkpoint_maximum_interval_ms: Option<u64>,
        help: bool,
        version: bool,
    }

    impl Options {
        /// Read the arguments, refusing anything unrecognised rather than ignoring it.
        ///
        /// An ignored flag is how a person ends up believing they passed a folder to a parser that
        /// silently dropped it.
        fn parse(arguments: &[String]) -> Result<Self, String> {
            let mut options = Self::default();
            let mut rest = arguments.iter();
            while let Some(argument) = rest.next() {
                match argument.as_str() {
                    "-h" | "--help" => options.help = true,
                    "-V" | "--version" => options.version = true,
                    "--endpoint" => {
                        let value = path_value(
                            "--endpoint",
                            rest.next().ok_or("`--endpoint` needs a path after it")?,
                        )?;
                        set_once(&mut options.endpoint, "--endpoint", PathBuf::from(value))?;
                    }
                    "--workspace" => {
                        let value = path_value(
                            "--workspace",
                            rest.next().ok_or("`--workspace` needs a folder after it")?,
                        )?;
                        set_once(&mut options.workspace, "--workspace", PathBuf::from(value))?;
                    }
                    "--trusted-reviewer-key" => {
                        let value = rest.next().ok_or(
                            "`--trusted-reviewer-key` needs a 64-character hex key after it",
                        )?;
                        options.trusted_reviewer_keys.push(parse_public_key(value)?);
                    }
                    "--checkpoint-idle-ms" => {
                        let value = parse_positive_number(
                            "--checkpoint-idle-ms",
                            rest.next().ok_or(
                                "`--checkpoint-idle-ms` needs a positive integer after it",
                            )?,
                        )?;
                        set_once(
                            &mut options.checkpoint_idle_ms,
                            "--checkpoint-idle-ms",
                            value,
                        )?;
                    }
                    "--checkpoint-maximum-bytes" => {
                        let value = parse_positive_number(
                            "--checkpoint-maximum-bytes",
                            rest.next().ok_or(
                                "`--checkpoint-maximum-bytes` needs a positive integer after it",
                            )?,
                        )?;
                        set_once(
                            &mut options.checkpoint_maximum_bytes,
                            "--checkpoint-maximum-bytes",
                            value,
                        )?;
                    }
                    "--checkpoint-maximum-interval-ms" => {
                        let value = parse_positive_number(
                            "--checkpoint-maximum-interval-ms",
                            rest.next().ok_or(
                                "`--checkpoint-maximum-interval-ms` needs a positive integer after it",
                            )?,
                        )?;
                        set_once(
                            &mut options.checkpoint_maximum_interval_ms,
                            "--checkpoint-maximum-interval-ms",
                            value,
                        )?;
                    }
                    other => {
                        if let Some(value) = other.strip_prefix("--endpoint=") {
                            set_once(
                                &mut options.endpoint,
                                "--endpoint",
                                PathBuf::from(path_value("--endpoint", value)?),
                            )?;
                        } else if let Some(value) = other.strip_prefix("--workspace=") {
                            set_once(
                                &mut options.workspace,
                                "--workspace",
                                PathBuf::from(path_value("--workspace", value)?),
                            )?;
                        } else if let Some(value) = other.strip_prefix("--trusted-reviewer-key=") {
                            options.trusted_reviewer_keys.push(parse_public_key(value)?);
                        } else if let Some(value) = other.strip_prefix("--checkpoint-idle-ms=") {
                            let value = parse_positive_number("--checkpoint-idle-ms", value)?;
                            set_once(
                                &mut options.checkpoint_idle_ms,
                                "--checkpoint-idle-ms",
                                value,
                            )?;
                        } else if let Some(value) =
                            other.strip_prefix("--checkpoint-maximum-bytes=")
                        {
                            let value = parse_positive_number("--checkpoint-maximum-bytes", value)?;
                            set_once(
                                &mut options.checkpoint_maximum_bytes,
                                "--checkpoint-maximum-bytes",
                                value,
                            )?;
                        } else if let Some(value) =
                            other.strip_prefix("--checkpoint-maximum-interval-ms=")
                        {
                            let value =
                                parse_positive_number("--checkpoint-maximum-interval-ms", value)?;
                            set_once(
                                &mut options.checkpoint_maximum_interval_ms,
                                "--checkpoint-maximum-interval-ms",
                                value,
                            )?;
                        } else {
                            return Err(format!("`{other}` is not an option this service has"));
                        }
                    }
                }
            }
            Ok(options)
        }

        fn checkpoint_parameters(&self) -> Result<Option<CheckpointRuntimeParameters>, String> {
            match (
                self.checkpoint_idle_ms,
                self.checkpoint_maximum_bytes,
                self.checkpoint_maximum_interval_ms,
            ) {
                (None, None, None) => Ok(Some(CheckpointRuntimeParameters::selected_defaults())),
                (Some(idle), Some(bytes), Some(maximum)) => Ok(Some(CheckpointRuntimeParameters {
                    idle_interval: Some(std::time::Duration::from_millis(idle)),
                    maximum_uncheckpointed_bytes: Some(bytes),
                    maximum_uncheckpointed_interval: Some(std::time::Duration::from_millis(
                        maximum,
                    )),
                })),
                _ => Err(
                    "automatic checkpointing requires all three of `--checkpoint-idle-ms`, \
                         `--checkpoint-maximum-bytes`, and \
                         `--checkpoint-maximum-interval-ms`; omit all three to use the measured \
                         ADR-0042 defaults"
                        .to_owned(),
                ),
            }
        }
    }

    fn path_value<'a>(option: &str, value: &'a str) -> Result<&'a str, String> {
        if value.is_empty() {
            return Err(format!("`{option}` needs a non-empty path"));
        }
        if value.starts_with('-') {
            return Err(format!(
                "`{option}` needs a path; `{value}` starts another option. Use `{option}=./{value}` when that relative path is intended"
            ));
        }
        Ok(value)
    }

    fn set_once<T>(slot: &mut Option<T>, option: &str, value: T) -> Result<(), String> {
        if slot.is_some() {
            return Err(format!("`{option}` may be supplied only once"));
        }
        *slot = Some(value);
        Ok(())
    }

    fn parse_positive_number(option: &str, text: &str) -> Result<u64, String> {
        let value = text
            .parse::<u64>()
            .map_err(|_| format!("`{option}` needs a positive integer, found `{text}`"))?;
        if value == 0 {
            return Err(format!("`{option}` must be greater than zero"));
        }
        Ok(value)
    }

    fn parse_public_key(text: &str) -> Result<PublicKey, String> {
        if text.len() != 64 {
            return Err(format!(
                "`--trusted-reviewer-key` needs exactly 64 hex characters, found {}",
                text.len()
            ));
        }
        let mut bytes = [0u8; 32];
        for (slot, pair) in bytes.iter_mut().zip(text.as_bytes().chunks_exact(2)) {
            let high = hex_value(pair[0])
                .ok_or("`--trusted-reviewer-key` contains a character outside hexadecimal")?;
            let low = hex_value(pair[1])
                .ok_or("`--trusted-reviewer-key` contains a character outside hexadecimal")?;
            *slot = (high << 4) | low;
        }
        Ok(PublicKey::from_bytes(bytes))
    }

    const fn hex_value(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }

    /// The help text, which is also the specification of the command line.
    const USAGE: &str = "\
meshd — the Mesh background service

Usage: meshd [--workspace <folder>] [--endpoint <path>] [--trusted-reviewer-key <hex> ...]

  --workspace <folder>   Open this folder as a workspace before serving.
  --endpoint <path>      Listen here instead of the default runtime location.
  --trusted-reviewer-key Trust this human Ed25519 public key for shared publication.
                         Repeat the option to trust more than one reviewer.
  --checkpoint-idle-ms <n>
                         Override the measured 50 ms checkpoint idle interval.
  --checkpoint-maximum-bytes <n>
                         Override the measured 65,536-byte recovery bound.
  --checkpoint-maximum-interval-ms <n>
                         Override the measured 25 ms recovery interval.
                         Supply all three overrides or none; omission uses ADR-0042 defaults.
  -h, --help             Show this message.
  -V, --version          Show the version.

It prints one line of JSON when it is ready to answer, then serves until its input
ends. In a terminal that is Ctrl-D.
";

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_options_are_read_in_either_spelling() {
            let parsed = Options::parse(&[
                "--workspace".to_owned(),
                "/tmp/w".to_owned(),
                "--endpoint=/tmp/e.sock".to_owned(),
                format!("--trusted-reviewer-key={}", "07".repeat(32)),
                "--trusted-reviewer-key".to_owned(),
                "08".repeat(32),
                "--checkpoint-idle-ms=10".to_owned(),
                "--checkpoint-maximum-bytes".to_owned(),
                "4096".to_owned(),
                "--checkpoint-maximum-interval-ms=100".to_owned(),
            ])
            .expect("parses");
            assert_eq!(parsed.workspace, Some(PathBuf::from("/tmp/w")));
            assert_eq!(parsed.endpoint, Some(PathBuf::from("/tmp/e.sock")));
            assert_eq!(parsed.trusted_reviewer_keys.len(), 2);
            assert_eq!(parsed.trusted_reviewer_keys[0].as_bytes(), &[7; 32]);
            assert_eq!(parsed.trusted_reviewer_keys[1].as_bytes(), &[8; 32]);
            assert!(parsed.checkpoint_parameters().expect("complete").is_some());
        }

        #[test]
        fn an_option_with_no_value_is_refused_rather_than_dropped() {
            assert!(Options::parse(&["--workspace".to_owned()]).is_err());
            assert!(Options::parse(&["--endpoint".to_owned()]).is_err());
            assert!(Options::parse(&["--trusted-reviewer-key".to_owned()]).is_err());
            assert!(Options::parse(&["--checkpoint-idle-ms".to_owned()]).is_err());
            assert!(
                Options::parse(&["--trusted-reviewer-key".to_owned(), "not-hex".to_owned()])
                    .is_err()
            );
        }

        #[test]
        fn option_tokens_empty_paths_and_repeated_singletons_are_refused() {
            for arguments in [
                vec!["--endpoint", "--help"],
                vec!["--workspace", "--version"],
                vec!["--endpoint="],
                vec!["--workspace="],
            ] {
                let arguments: Vec<_> = arguments.into_iter().map(str::to_owned).collect();
                assert!(Options::parse(&arguments).is_err(), "{arguments:?}");
            }

            for arguments in [
                vec!["--endpoint", "/tmp/a", "--endpoint=/tmp/b"],
                vec!["--workspace=/tmp/a", "--workspace", "/tmp/b"],
                vec!["--checkpoint-idle-ms=10", "--checkpoint-idle-ms", "20"],
                vec![
                    "--checkpoint-maximum-bytes",
                    "10",
                    "--checkpoint-maximum-bytes=20",
                ],
                vec![
                    "--checkpoint-maximum-interval-ms=10",
                    "--checkpoint-maximum-interval-ms=20",
                ],
            ] {
                let arguments: Vec<_> = arguments.into_iter().map(str::to_owned).collect();
                let problem = Options::parse(&arguments).expect_err("singleton cannot repeat");
                assert!(problem.contains("only once"), "{arguments:?}: {problem}");
            }

            let explicit = Options::parse(&[
                "--workspace=./--help".to_owned(),
                "--endpoint=./--version".to_owned(),
            ])
            .expect("explicitly path-shaped dash names remain available");
            assert_eq!(explicit.workspace, Some(PathBuf::from("./--help")));
            assert_eq!(explicit.endpoint, Some(PathBuf::from("./--version")));
        }

        #[test]
        fn an_unknown_option_is_refused_and_named() {
            let problem = Options::parse(&["--mount".to_owned()]).expect_err("refused");
            assert!(problem.contains("--mount"), "{problem}");
        }

        #[test]
        fn checkpoint_configuration_is_all_or_none_and_strictly_positive() {
            let partial =
                Options::parse(&["--checkpoint-idle-ms=10".to_owned()]).expect("one flag parses");
            assert!(partial.checkpoint_parameters().is_err());
            assert!(Options::parse(&["--checkpoint-maximum-bytes=0".to_owned()]).is_err());
            assert_eq!(
                Options::default()
                    .checkpoint_parameters()
                    .expect("absent uses measured defaults"),
                Some(CheckpointRuntimeParameters::selected_defaults())
            );
        }

        #[test]
        fn help_and_version_are_recognised_in_both_spellings() {
            assert!(Options::parse(&["-h".to_owned()]).expect("parses").help);
            assert!(Options::parse(&["--help".to_owned()]).expect("parses").help);
            assert!(Options::parse(&["-V".to_owned()]).expect("parses").version);
            assert!(
                Options::parse(&["--version".to_owned()])
                    .expect("parses")
                    .version
            );
        }

        #[test]
        fn the_usage_text_names_every_option_the_parser_accepts() {
            for flag in [
                "--workspace",
                "--endpoint",
                "--trusted-reviewer-key",
                "--checkpoint-idle-ms",
                "--checkpoint-maximum-bytes",
                "--checkpoint-maximum-interval-ms",
                "--help",
                "--version",
            ] {
                assert!(USAGE.contains(flag), "`{flag}` is undocumented");
            }
        }

        #[test]
        fn the_default_endpoint_is_inside_a_runtime_folder_that_exists() {
            let endpoint = default_endpoint().expect("a runtime folder");
            assert_eq!(
                endpoint.file_name().and_then(|n| n.to_str()),
                Some(SOCKET_FILE)
            );
            assert!(endpoint.parent().is_some_and(Path::exists));
            assert!(
                endpoint.as_os_str().len() <= MAX_ENDPOINT_BYTES,
                "the default endpoint is over the platform limit: {}",
                endpoint.display()
            );
        }
    }
}
