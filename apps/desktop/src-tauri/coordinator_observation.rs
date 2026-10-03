//! Explicit native coordinator commands; no renderer or worker supplies configuration.
pub(crate) mod panel;
mod receiving;
mod reconnecting;
mod recovering;
mod starting;
use mesh_crypto::KeyCustody as _;
use mesh_daemon::{
    fleet::{
        catalog::NativeFleetDirectory,
        service::{RemoteObservationKind, RemoteObservationOutcome},
        NativeSshDestination, NativeWorkerInstallation,
    },
    ipc::Json,
    CheckpointRuntimeParameters, ProtectedWorkspaceRoot, TrustedReviewers,
};
use mesh_keychain::AppleActorCustody;
use mesh_types::PublicKey;
use std::{
    io,
    path::{Path, PathBuf},
    time::Duration,
};
const UNAVAILABLE: &str = "The coordinator observation is unavailable or requires reconciliation";
#[derive(Debug, PartialEq, Eq)]
enum Action {
    Status,
    Execution,
    InputInspection,
    Results(u64),
    Receive,
    ReconnectInput,
    RecoverOriginal,
    Start,
    Created,
}
fn parse(args: &[String]) -> Result<Option<(Action, PathBuf)>, String> {
    if args.first().map(String::as_str) != Some("--coordinator") {
        return Ok(None);
    }
    let action = match args.get(1).map(String::as_str) {
        Some("status") if args.len() == 3 => Action::Status,
        Some("execution") if args.len() == 3 => Action::Execution,
        Some("inspect-input") if args.len() == 3 => Action::InputInspection,
        Some("receive") if args.len() == 3 => Action::Receive,
        Some("reconnect-input") if args.len() == 3 => Action::ReconnectInput,
        Some("recover-original") if args.len() == 3 => Action::RecoverOriginal,
        Some("start") if args.len() == 3 => Action::Start,
        Some("created") if args.len() == 3 => Action::Created,
        Some("results") if args.len() == 4 => {
            let after: u64 = args[3].parse().map_err(|_| UNAVAILABLE)?;
            if after > 4096 || after.to_string() != args[3] { return Err(UNAVAILABLE.into()); }
            Action::Results(after)
        }
        _ => return Err("Use --coordinator status <absolute-config> or --coordinator results <absolute-config> <after> or --coordinator receive|start|created|reconnect-input|inspect-input|execution|recover-original <absolute-config>".into()),
    };
    let path = PathBuf::from(&args[2]);
    if !path.is_absolute() {
        return Err(UNAVAILABLE.into());
    }
    Ok(Some((action, path)))
}
struct Configuration {
    connection: ConnectionConfiguration,
    objective: String,
    lane: String,
    run: String,
}
struct ConnectionConfiguration {
    installation: PathBuf,
    fleets: PathBuf,
    host: String,
    account: String,
    port: u16,
    identity: PathBuf,
    known_hosts: PathBuf,
    worker: PublicKey,
}
fn config(value: Json) -> Result<Configuration, String> {
    let fields = [
        "schema",
        "installation",
        "fleets",
        "objective",
        "lane",
        "run",
        "host",
        "account",
        "port",
        "identity",
        "known_hosts",
        "worker",
    ];
    let Json::Object(pairs) = &value else {
        return Err(UNAVAILABLE.into());
    };
    if pairs.len() != fields.len()
        || fields
            .iter()
            .any(|name| pairs.iter().filter(|(key, _)| key == name).count() != 1)
    {
        return Err(UNAVAILABLE.into());
    }
    let text = |name| {
        value
            .get(name)
            .and_then(Json::as_text)
            .filter(|s| !s.is_empty() && s.len() <= 4096 && !s.contains('\0'))
            .ok_or(UNAVAILABLE)
    };
    if text("schema")? != "mesh.coordinator-observation-config/v1" {
        return Err(UNAVAILABLE.into());
    }
    for name in ["objective", "lane", "run"] {
        let id = text(name)?;
        if id.len() > 128
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err(UNAVAILABLE.into());
        }
    }
    Ok(Configuration {
        connection: connection_config(&value)?,
        objective: text("objective")?.into(),
        lane: text("lane")?.into(),
        run: text("run")?.into(),
    })
}
fn connection_config(value: &Json) -> Result<ConnectionConfiguration, String> {
    let text = |name| {
        value
            .get(name)
            .and_then(Json::as_text)
            .filter(|s| !s.is_empty() && s.len() <= 4096 && !s.contains('\0'))
            .ok_or(UNAVAILABLE)
    };
    let path = |name| -> Result<PathBuf, String> {
        let p = PathBuf::from(text(name)?);
        if !p.is_absolute() {
            return Err(UNAVAILABLE.into());
        }
        Ok(p)
    };
    let key = text("worker")?;
    if key.len() != 64
        || !key
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(UNAVAILABLE.into());
    }
    let mut worker = [0; 32];
    for (i, b) in worker.iter_mut().enumerate() {
        *b = u8::from_str_radix(&key[i * 2..i * 2 + 2], 16).map_err(|_| UNAVAILABLE)?;
    }
    let port = value
        .get("port")
        .and_then(Json::as_u64)
        .and_then(|p| u16::try_from(p).ok())
        .filter(|p| *p != 0)
        .ok_or(UNAVAILABLE)?;
    Ok(ConnectionConfiguration {
        installation: path("installation")?,
        fleets: path("fleets")?,
        host: text("host")?.into(),
        account: text("account")?.into(),
        port,
        identity: path("identity")?,
        known_hosts: path("known_hosts")?,
        worker: PublicKey::from_bytes(worker),
    })
}
pub fn run_if_requested() -> Option<Result<(), String>> {
    match parse(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Ok(None) => None,
        Err(e) => Some(Err(e)),
        Ok(Some((action, path))) => Some(run(action, &path)),
    }
}
fn run(action: Action, path: &Path) -> Result<(), String> {
    let failure = output_failure(&action);
    let result = execute(action, path)?;
    write_result(&mut io::stdout().lock(), &result, failure)
}

// Native callers consume a verified result directly. Presentation/output failure must never
// dispatch, renew, receive or reconnect a second time. This is not a renderer command and accepts
// only the existing privately loaded native configuration; no arbitrary runtime escapes.
fn execute(action: Action, path: &Path) -> Result<Json, String> {
    AppleActorCustody::availability()
        .map_err(|_| "Coordinator identity requires an eligible signed Mesh application")?;
    if action == Action::Start || action == Action::Created {
        return starting::execute(path, action == Action::Created);
    }
    if action == Action::RecoverOriginal {
        return recovering::execute(path);
    }
    if action == Action::ReconnectInput {
        return reconnecting::execute(path);
    }
    if action == Action::Receive {
        return receiving::execute(path);
    }
    let config = config(crate::worker_service::load_private_json(path)?)?;
    let NativeContext {
        installation,
        custody,
        peer,
        directory,
    } = open_context(&config.connection)?;
    let coordinator = installation.identity().map_err(|_| UNAVAILABLE)?.worker();
    let history = directory
        .history(&config.objective)
        .map_err(|_| UNAVAILABLE)?;
    let kind = match action {
        Action::Status => RemoteObservationKind::CurrentLease,
        Action::Execution => RemoteObservationKind::Execution,
        Action::InputInspection => RemoteObservationKind::InputInspection,
        Action::Results(after) => RemoteObservationKind::Results { after },
        Action::Receive
        | Action::Start
        | Action::Created
        | Action::ReconnectInput
        | Action::RecoverOriginal => {
            unreachable!("receiving uses its closed native operation")
        }
    };
    let observation = history
        .prepare_remote_observation(
            &config.lane,
            &config.run,
            coordinator,
            config.connection.worker,
            kind,
            |payload| {
                installation.identity().map_err(|_| UNAVAILABLE)?;
                custody.sign(payload).map_err(|_| UNAVAILABLE.into())
            },
        )
        .map_err(|_| UNAVAILABLE)?;
    let result = observation
        .read_over_ssh(&peer, Duration::from_secs(25))
        .map_err(|_| UNAVAILABLE)?;
    installation.identity().map_err(|_| UNAVAILABLE)?;
    Ok(render(result))
}

fn output_failure(action: &Action) -> &'static str {
    match action {
        Action::Status | Action::Execution | Action::InputInspection | Action::Results(_) => "Verified observation output could not be written",
        Action::Start | Action::Created => "Start output unavailable; retain the same configuration and inspect --coordinator created",
        Action::Receive => "Saved result output could not be written; retain the same receive configuration for explicit recovery",
        Action::ReconnectInput => "Input reconnect output unavailable; inspect the original retained assignment",
        Action::RecoverOriginal => "Recovery output unavailable; inspect the original retained assignment before any further action",
    }
}

fn write_result(
    writer: &mut impl io::Write,
    result: &Json,
    failure: &'static str,
) -> Result<(), String> {
    writeln!(writer, "{}", result.encode())
        .and_then(|()| writer.flush())
        .map_err(|_| failure.into())
}
fn render(result: RemoteObservationOutcome) -> Json {
    match result {
        RemoteObservationOutcome::CurrentLease(receipt) => Json::object([
            ("schema", Json::text("mesh.coordinator-status/v1")),
            ("target", receipt.target().clone()),
            ("observed_ms", Json::Number(receipt.observed_ms())),
            ("facts", receipt.facts().clone()),
        ]),
        RemoteObservationOutcome::Execution(receipt) => Json::object([
            ("schema", Json::text("mesh.coordinator-execution/v1")),
            ("target", receipt.target().clone()),
            ("observed_ms", Json::Number(receipt.observed_ms())),
            ("facts", receipt.facts().clone()),
        ]),
        RemoteObservationOutcome::InputInspection(receipt) => Json::object([
            ("schema", Json::text("mesh.coordinator-input-inspection/v1")),
            ("target", receipt.target().clone()),
            ("observed_ms", Json::Number(receipt.observed_ms())),
            ("facts", receipt.facts().clone()),
        ]),
        RemoteObservationOutcome::Results(page) => Json::object([
            ("schema", Json::text("mesh.coordinator-results/v1")),
            (
                "page",
                page.map_or(Json::Null, |p| {
                    Json::object([
                        ("revision", Json::Number(p.revision)),
                        ("after", Json::Number(p.after)),
                        ("has_more", Json::Bool(p.has_more)),
                        (
                            "offers",
                            Json::Array(
                                p.offers
                                    .into_iter()
                                    .map(|o| Json::text(o.encode()))
                                    .collect(),
                            ),
                        ),
                    ])
                }),
            ),
        ]),
    }
}
#[cfg(test)]
mod tests;

struct NativeContext {
    installation: NativeWorkerInstallation,
    custody: AppleActorCustody,
    peer: NativeSshDestination,
    directory: NativeFleetDirectory,
}
struct NativeConnection {
    installation: NativeWorkerInstallation,
    custody: AppleActorCustody,
    peer: NativeSshDestination,
}
fn open_connection(config: &ConnectionConfiguration) -> Result<NativeConnection, String> {
    let expected =
        ProtectedWorkspaceRoot::inspect(&config.installation).map_err(|_| UNAVAILABLE)?;
    let (installation, custody) =
        NativeWorkerInstallation::reopen(&config.installation, expected, &[], |account, key| {
            AppleActorCustody::open(account, key).map_err(|_| io::Error::other(UNAVAILABLE))
        })
        .map_err(|_| UNAVAILABLE)?;
    let peer = NativeSshDestination::admit(
        &config.host,
        &config.account,
        config.port,
        &config.identity,
        &config.known_hosts,
    )
    .map_err(|_| UNAVAILABLE)?;
    Ok(NativeConnection {
        installation,
        custody,
        peer,
    })
}
fn open_context(config: &ConnectionConfiguration) -> Result<NativeContext, String> {
    let NativeConnection {
        installation,
        custody,
        peer,
    } = open_connection(config)?;
    let directory = NativeFleetDirectory::open(
        &config.fleets,
        TrustedReviewers::default(),
        CheckpointRuntimeParameters {
            idle_interval: None,
            maximum_uncheckpointed_bytes: None,
            maximum_uncheckpointed_interval: None,
        },
    )
    .map_err(|_| UNAVAILABLE)?;
    Ok(NativeContext {
        installation,
        custody,
        peer,
        directory,
    })
}
