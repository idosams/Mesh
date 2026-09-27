//! Versioned deterministic command encoding. Unknown fields/versions refuse during replay.
use super::{AgentOrigin, CheckpointResult, Command, Error, Limits, RunState, WorkspaceBinding};
use crate::ipc::Json;
use mesh_store::RecordDigest;

pub(super) fn encode(command: &Command) -> String {
    let (kind, fields) = match command {
        Command::SubmitReview { checkpoint, bundle } => (
            "submit-review",
            vec![
                ("checkpoint", Json::text(checkpoint)),
                ("bundle", Json::text(bundle.to_string())),
            ],
        ),
        Command::BeginCheckpoint {
            id,
            lane,
            origin,
            input_digest,
        } => (
            "begin-checkpoint",
            vec![
                ("id", Json::text(id)),
                ("lane", Json::text(lane)),
                ("actor", Json::text(&origin.actor)),
                ("session", Json::text(&origin.session)),
                ("run", Json::text(&origin.run)),
                ("generation", Json::text(&origin.generation)),
                ("input_digest", Json::text(input_digest.to_string())),
            ],
        ),
        Command::FinishCheckpoint { id, result } => (
            "finish-checkpoint",
            vec![
                ("id", Json::text(id)),
                ("complete", Json::Bool(result.complete)),
                ("version", Json::text(result.version.to_string())),
                (
                    "workspace_digest",
                    Json::text(result.workspace_digest.to_string()),
                ),
                ("saved_changes", Json::Number(result.saved_changes)),
                (
                    "issue",
                    result.issue.as_ref().map(Json::text).unwrap_or(Json::Null),
                ),
            ],
        ),
        Command::Start { goal, limits } => (
            "start",
            vec![
                ("goal", Json::text(goal)),
                ("lanes", Json::Number(limits.lanes)),
                ("concurrency", Json::Number(limits.concurrency)),
                ("depth", Json::Number(limits.depth)),
                ("retries", Json::Number(limits.retries)),
            ],
        ),
        Command::CreateLane {
            id,
            parent,
            goal,
            provider,
            base,
        } => (
            "create-lane",
            vec![
                ("id", Json::text(id)),
                (
                    "parent",
                    parent.as_ref().map(Json::text).unwrap_or(Json::Null),
                ),
                ("goal", Json::text(goal)),
                ("provider", Json::text(provider)),
                ("base", Json::text(base.to_string())),
            ],
        ),
        Command::Delegate {
            id,
            parent,
            goal,
            provider,
            base,
            origin,
        } => (
            "delegate",
            vec![
                ("id", Json::text(id)),
                ("parent", Json::text(parent)),
                ("goal", Json::text(goal)),
                ("provider", Json::text(provider)),
                ("base", Json::text(base.to_string())),
                ("actor", Json::text(&origin.actor)),
                ("session", Json::text(&origin.session)),
                ("run", Json::text(&origin.run)),
                ("generation", Json::text(&origin.generation)),
            ],
        ),
        Command::BindWorkspace { lane, binding } => (
            "bind-workspace",
            vec![
                ("lane", Json::text(lane)),
                ("root", Json::text(&binding.root)),
                (
                    "source_version",
                    Json::text(binding.source_version.to_string()),
                ),
                ("digest", Json::text(&binding.digest)),
                ("installation", Json::text(&binding.installation)),
            ],
        ),
        Command::Dispatch { lane, run } => (
            "dispatch",
            vec![("lane", Json::text(lane)), ("run", Json::text(run))],
        ),
        Command::Observe { lane, run, state } => (
            "observe",
            vec![
                ("lane", Json::text(lane)),
                ("run", Json::text(run)),
                ("state", Json::text(state_word(*state))),
            ],
        ),
        Command::Saved { lane, run, version } => (
            "saved",
            vec![
                ("lane", Json::text(lane)),
                ("run", Json::text(run)),
                ("version", Json::text(version.to_string())),
            ],
        ),
        Command::Cancel => ("cancel", vec![]),
    };
    Json::object([
        ("schema", Json::Number(1)),
        ("kind", Json::text(kind)),
        ("fields", Json::object(fields)),
    ])
    .encode()
}

pub(super) fn decode(payload: &str) -> Result<Command, Error> {
    let json = Json::parse(payload).map_err(|_| Error::InvalidHistory)?;
    if json.get("schema").and_then(Json::as_u64) != Some(1) {
        return Err(Error::InvalidHistory);
    }
    let fields = json.get("fields").ok_or(Error::InvalidHistory)?;
    let text = |key| {
        fields
            .get(key)
            .and_then(Json::as_text)
            .map(str::to_owned)
            .ok_or(Error::InvalidHistory)
    };
    let number = |key| {
        fields
            .get(key)
            .and_then(Json::as_u64)
            .ok_or(Error::InvalidHistory)
    };
    let digest = |key| RecordDigest::parse_hex(&text(key)?).map_err(|_| Error::InvalidHistory);
    let command = match json.get("kind").and_then(Json::as_text) {
        Some("submit-review") => Command::SubmitReview {
            checkpoint: text("checkpoint")?,
            bundle: digest("bundle")?,
        },
        Some("begin-checkpoint") => Command::BeginCheckpoint {
            id: text("id")?,
            lane: text("lane")?,
            input_digest: text("input_digest")?,
            origin: AgentOrigin {
                actor: text("actor")?,
                session: text("session")?,
                run: text("run")?,
                generation: text("generation")?,
            },
        },
        Some("finish-checkpoint") => Command::FinishCheckpoint {
            id: text("id")?,
            result: CheckpointResult {
                complete: fields
                    .get("complete")
                    .and_then(Json::as_bool)
                    .ok_or(Error::InvalidHistory)?,
                version: digest("version")?,
                workspace_digest: text("workspace_digest")?,
                saved_changes: number("saved_changes")?,
                issue: match fields.get("issue") {
                    Some(Json::Null) => None,
                    Some(Json::Text(value)) => Some(value.clone()),
                    _ => return Err(Error::InvalidHistory),
                },
            },
        },
        Some("start") => Command::Start {
            goal: text("goal")?,
            limits: Limits {
                lanes: number("lanes")?,
                concurrency: number("concurrency")?,
                depth: number("depth")?,
                retries: number("retries")?,
            },
        },
        Some("create-lane") => Command::CreateLane {
            id: text("id")?,
            parent: match fields.get("parent") {
                Some(Json::Null) => None,
                Some(Json::Text(value)) => Some(value.clone()),
                _ => return Err(Error::InvalidHistory),
            },
            goal: text("goal")?,
            provider: text("provider")?,
            base: digest("base")?,
        },
        Some("delegate") => Command::Delegate {
            id: text("id")?,
            parent: text("parent")?,
            goal: text("goal")?,
            provider: text("provider")?,
            base: digest("base")?,
            origin: AgentOrigin {
                actor: text("actor")?,
                session: text("session")?,
                run: text("run")?,
                generation: text("generation")?,
            },
        },
        Some("bind-workspace") => Command::BindWorkspace {
            lane: text("lane")?,
            binding: WorkspaceBinding {
                source_version: digest("source_version")?,
                root: text("root")?,
                digest: text("digest")?,
                installation: text("installation")?,
            },
        },
        Some("dispatch") => Command::Dispatch {
            lane: text("lane")?,
            run: text("run")?,
        },
        Some("observe") => Command::Observe {
            lane: text("lane")?,
            run: text("run")?,
            state: parse_state(&text("state")?)?,
        },
        Some("saved") => Command::Saved {
            lane: text("lane")?,
            run: text("run")?,
            version: digest("version")?,
        },
        Some("cancel") => Command::Cancel,
        _ => return Err(Error::InvalidHistory),
    };
    // Storage only accepts this encoder's form. Refuse ignored fields or ambiguous future shapes.
    if encode(&command) != payload {
        return Err(Error::InvalidHistory);
    }
    Ok(command)
}
pub(super) fn state_word(state: RunState) -> &'static str {
    match state {
        RunState::Launching => "launching",
        RunState::Running => "running",
        RunState::Waiting => "waiting",
        RunState::Reconciling => "reconciling",
        RunState::Stopping => "stopping",
        RunState::Succeeded => "succeeded",
        RunState::Failed => "failed",
        RunState::Cancelled => "cancelled",
    }
}
fn parse_state(value: &str) -> Result<RunState, Error> {
    match value {
        "launching" => Ok(RunState::Launching),
        "running" => Ok(RunState::Running),
        "waiting" => Ok(RunState::Waiting),
        "reconciling" => Ok(RunState::Reconciling),
        "stopping" => Ok(RunState::Stopping),
        "succeeded" => Ok(RunState::Succeeded),
        "failed" => Ok(RunState::Failed),
        "cancelled" => Ok(RunState::Cancelled),
        _ => Err(Error::InvalidHistory),
    }
}
