//! Versioned deterministic command encoding. Unknown fields/versions refuse during replay.
use super::{
    AgentOrigin, CheckpointResult, Command, Error, Limits, ReviewChangeRequest, RunState,
    WorkspaceBinding,
};
use crate::ipc::Json;
use mesh_store::RecordDigest;

pub(super) fn encode(command: &Command) -> String {
    let (kind, fields) = match command {
        Command::RequestReviewChanges(request) => (
            "request-review-changes",
            vec![
                ("id", Json::text(&request.id)),
                ("lane", Json::text(&request.lane)),
                ("checkpoint", Json::text(&request.checkpoint)),
                ("version", Json::text(request.version.to_string())),
                ("bundle", Json::text(request.bundle.to_string())),
                ("message", Json::text(&request.message)),
            ],
        ),
        Command::ClaimLaunch { lane, run, owner } => (
            "claim-launch",
            vec![
                ("lane", Json::text(lane)),
                ("run", Json::text(run)),
                ("owner", Json::text(owner)),
            ],
        ),
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
        Command::CreateAttachedLane {
            id,
            project,
            goal,
            provider,
            base,
        } => (
            "create-attached-lane",
            vec![
                ("id", Json::text(id)),
                ("project", Json::text(project)),
                ("goal", Json::text(goal)),
                ("provider", Json::text(provider)),
                ("base", Json::text(base.to_string())),
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
        Command::BindWorkspace { lane, binding } => {
            let mut fields = vec![
                ("lane", Json::text(lane)),
                ("root", Json::text(&binding.root)),
                (
                    "source_version",
                    Json::text(binding.source_version.to_string()),
                ),
                ("digest", Json::text(&binding.digest)),
                ("installation", Json::text(&binding.installation)),
            ];
            let kind = if let Some(version) = binding.starting_version {
                fields.push(("starting_version", Json::text(version.to_string())));
                "bind-workspace-v2"
            } else {
                "bind-workspace"
            };
            (kind, fields)
        }
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
        Some("request-review-changes") => Command::RequestReviewChanges(ReviewChangeRequest {
            id: text("id")?,
            lane: text("lane")?,
            checkpoint: text("checkpoint")?,
            version: digest("version")?,
            bundle: digest("bundle")?,
            message: text("message")?,
        }),
        Some("claim-launch") => Command::ClaimLaunch {
            lane: text("lane")?,
            run: text("run")?,
            owner: text("owner")?,
        },
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
        Some("create-attached-lane") => Command::CreateAttachedLane {
            id: text("id")?,
            project: text("project")?,
            goal: text("goal")?,
            provider: text("provider")?,
            base: digest("base")?,
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
        Some(kind @ ("bind-workspace" | "bind-workspace-v2")) => Command::BindWorkspace {
            lane: text("lane")?,
            binding: WorkspaceBinding {
                source_version: digest("source_version")?,
                starting_version: if kind == "bind-workspace-v2" {
                    Some(digest("starting_version")?)
                } else {
                    None
                },
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

#[cfg(test)]
mod attachment_tests {
    use super::*;

    #[test]
    fn starting_version_binding_is_additive_closed_and_never_inferred_for_legacy_records() {
        let old = Command::BindWorkspace {
            lane: "lane-one".into(),
            binding: WorkspaceBinding {
                source_version: RecordDigest::from_bytes([1; 32]),
                starting_version: None,
                root: "/native/lane".into(),
                digest: "allocation-digest".into(),
                installation: "allocation-installation".into(),
            },
        };
        let legacy = encode(&old);
        assert!(legacy.contains("\"kind\":\"bind-workspace\""));
        assert!(!legacy.contains("starting_version"));
        assert_eq!(decode(&legacy).unwrap(), old);
        let Command::BindWorkspace { lane, mut binding } = old else {
            unreachable!()
        };
        binding.starting_version = Some(RecordDigest::from_bytes([2; 32]));
        let new = Command::BindWorkspace { lane, binding };
        let encoded = encode(&new);
        assert_eq!(decode(&encoded).unwrap(), new);
        assert!(encoded.contains("bind-workspace-v2"));
        for bad in [
            encoded.replace("bind-workspace-v2", "bind-workspace"),
            legacy.replace("bind-workspace", "bind-workspace-v2"),
            encoded.replace("starting_version", "unknown"),
            encoded.replace(&"02".repeat(32), "not-a-version"),
        ] {
            assert!(decode(&bad).is_err());
        }
    }

    #[test]
    fn attached_lane_encoding_is_additive_canonical_and_closed() {
        let version = RecordDigest::from_bytes([7; 32]);
        let attached = Command::CreateAttachedLane {
            id: "lane-one".into(),
            project: "a".repeat(64),
            goal: "Work".into(),
            provider: "codex".into(),
            base: version,
        };
        let encoded = encode(&attached);
        assert_eq!(encode(&decode(&encoded).unwrap()), encoded);
        assert!(encoded.contains("create-attached-lane"));
        assert!(decode(&encoded.replacen("\"project\":", "\"unknown\":", 1)).is_err());
        assert!(
            decode(&encoded.replacen("create-attached-lane", "create-attached-lane-v2", 1))
                .is_err()
        );
        let legacy = Command::CreateLane {
            id: "lane-one".into(),
            parent: None,
            goal: "Work".into(),
            provider: "codex".into(),
            base: version,
        };
        let old = encode(&legacy);
        assert!(!old.contains("project"));
        assert_eq!(encode(&decode(&old).unwrap()), old);
    }
}
