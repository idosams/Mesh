//! Selected receiving uses native retained input, never a caller-authored manifest or per-file output path.
use super::*;
use mesh_daemon::ManagedContentDigest as RecordDigest;
use mesh_daemon::{
    fleet::{
        service::{FleetHistory, RemoteHistoryIngestionRequest, SavedReviewSelection},
        RemoteInputDestination, RemoteInputSource,
    },
    project_attachment::AttachmentStorage,
};

enum Input {
    Project {
        storage: PathBuf,
        project: String,
        version: String,
    },
    Review(SavedReviewSelection),
}
struct ReceiveConfiguration {
    connection: Configuration,
    input: Input,
    store: PathBuf,
    allocations: PathBuf,
    allocation: String,
    offer: String,
}
pub(super) fn closed(value: &Json, names: &[&str]) -> Result<(), String> {
    let Json::Object(fields) = value else {
        return Err(UNAVAILABLE.into());
    };
    if fields.len() != names.len()
        || names
            .iter()
            .any(|n| fields.iter().filter(|(k, _)| k == n).count() != 1)
    {
        return Err(UNAVAILABLE.into());
    }
    Ok(())
}
pub(super) fn text<'a>(value: &'a Json, name: &str) -> Result<&'a str, String> {
    value
        .get(name)
        .and_then(Json::as_text)
        .filter(|s| !s.is_empty() && !s.contains('\0'))
        .ok_or_else(|| UNAVAILABLE.into())
}
pub(super) fn path(value: &Json, name: &str) -> Result<PathBuf, String> {
    let p = PathBuf::from(text(value, name)?);
    if !p.is_absolute() {
        return Err(UNAVAILABLE.into());
    }
    Ok(p)
}
pub(super) fn hexadecimal(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn configuration(value: Json) -> Result<ReceiveConfiguration, String> {
    closed(
        &value,
        &[
            "schema",
            "connection",
            "input",
            "store",
            "allocations",
            "allocation",
            "offer",
        ],
    )?;
    if text(&value, "schema")? != "mesh.coordinator-receive-config/v1" {
        return Err(UNAVAILABLE.into());
    }
    let connection = config(value.get("connection").ok_or(UNAVAILABLE)?.clone())?;
    let selected = value.get("input").ok_or(UNAVAILABLE)?;
    let input = match text(selected, "kind")? {
        "project" => {
            closed(selected, &["kind", "storage", "project", "version"])?;
            let project = text(selected, "project")?;
            let version = text(selected, "version")?;
            if !hexadecimal(project, 64) || !hexadecimal(version, 64) {
                return Err(UNAVAILABLE.into());
            }
            Input::Project {
                storage: path(selected, "storage")?,
                project: project.into(),
                version: version.into(),
            }
        }
        "review" => {
            closed(
                selected,
                &["kind", "lane", "checkpoint", "version", "bundle"],
            )?;
            Input::Review(
                SavedReviewSelection::new(
                    text(selected, "lane")?,
                    text(selected, "checkpoint")?,
                    text(selected, "version")?,
                    text(selected, "bundle")?,
                )
                .map_err(|_| UNAVAILABLE)?,
            )
        }
        _ => return Err(UNAVAILABLE.into()),
    };
    let allocation = text(&value, "allocation")?;
    if !hexadecimal(allocation, 32) {
        return Err(UNAVAILABLE.into());
    }
    let offer = text(&value, "offer")?;
    // Signature and exact assignment are checked by native ingestion before any receiving writes.
    if offer.len() > 8192 || Json::parse(offer).is_err() {
        return Err(UNAVAILABLE.into());
    }
    Ok(ReceiveConfiguration {
        connection,
        input,
        store: path(&value, "store")?,
        allocations: path(&value, "allocations")?,
        allocation: allocation.into(),
        offer: offer.into(),
    })
}
fn input_source(
    input: &Input,
    history: &FleetHistory,
    protected: &mut Vec<ProtectedWorkspaceRoot>,
) -> Result<RemoteInputSource, String> {
    match input {
        Input::Project {
            storage,
            project,
            version,
        } => project_input(storage, project, version, protected),
        Input::Review(selection) => history
            .prepare_remote_review_input(selection)
            .map_err(|_| UNAVAILABLE.into()),
    }
}
fn project_input(
    storage: &Path,
    project: &str,
    version: &str,
    protected: &mut Vec<ProtectedWorkspaceRoot>,
) -> Result<RemoteInputSource, String> {
    let attachment = AttachmentStorage::open(storage)
        .and_then(|s| s.reopen(project))
        .map_err(|_| UNAVAILABLE)?;
    for path in [
        storage,
        attachment.project().root(),
        attachment.metadata_path(),
    ] {
        protected.push(ProtectedWorkspaceRoot::inspect(path).map_err(|_| UNAVAILABLE)?);
    }
    attachment
        .prepare_remote_input(version)
        .map_err(|_| UNAVAILABLE.into())
}
pub(super) fn run(path: &Path) -> Result<(), String> {
    let selected = configuration(crate::worker_service::load_private_json(path)?)?;
    let NativeContext {
        installation,
        custody,
        peer,
        directory,
    } = open_context(&selected.connection.connection)?;
    let coordinator = installation.identity().map_err(|_| UNAVAILABLE)?.worker();
    let history = directory
        .history(&selected.connection.objective)
        .map_err(|_| UNAVAILABLE)?;
    let mut protected = Vec::new();
    for path in [
        &selected.connection.connection.installation,
        &selected.connection.connection.fleets,
    ] {
        protected.push(ProtectedWorkspaceRoot::inspect(path).map_err(|_| UNAVAILABLE)?);
    }
    let input = input_source(&selected.input, &history, &mut protected)?;
    let destination = RemoteInputDestination::admit(
        &selected.store,
        ProtectedWorkspaceRoot::inspect(&selected.store).map_err(|_| UNAVAILABLE)?,
        &selected.allocations,
        ProtectedWorkspaceRoot::inspect(&selected.allocations).map_err(|_| UNAVAILABLE)?,
        &protected,
    )
    .map_err(|_| UNAVAILABLE)?;
    let trusted = TrustedReviewers::default();
    let receipt = history
        .ingest_remote_result_over_ssh(
            &peer,
            RemoteHistoryIngestionRequest {
                lane: &selected.connection.lane,
                run: &selected.connection.run,
                coordinator,
                worker: selected.connection.connection.worker,
                input: input.manifest(),
                offer: &selected.offer,
                destination: &destination,
                allocation: &selected.allocation,
                reviewers: &trusted,
                checkpoint: CheckpointRuntimeParameters::selected_defaults(),
                actor: coordinator,
            },
            Duration::from_secs(25),
            |payload| {
                installation.identity().map_err(|_| UNAVAILABLE)?;
                custody.sign(payload).map_err(|_| UNAVAILABLE.into())
            },
        )
        .map_err(|_| UNAVAILABLE)?;
    installation.identity().map_err(|_| UNAVAILABLE)?;
    let output = outcome(receipt.digest(), receipt.version(), receipt.review());
    let mut stdout = io::stdout().lock();
    writeln!(stdout,"{}",output.encode()).and_then(|()|stdout.flush()).map_err(|_|"Saved result output could not be written; retain the same receive configuration for explicit recovery".into())
}
fn outcome(correlation: RecordDigest, version: RecordDigest, review: RecordDigest) -> Json {
    Json::object([
        ("schema", Json::text("mesh.coordinator-received-result/v1")),
        ("correlation", Json::text(correlation.to_string())),
        ("version", Json::text(version.to_string())),
        ("review", Json::text(review.to_string())),
    ])
}
#[cfg(test)]
mod tests;
