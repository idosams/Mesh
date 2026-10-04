//! Read-only projection of durable handoffs and explicit feedback decisions.
//! One pass over records; no filesystem reads, execution adoption or approval.
use super::{lane_summary, Json};
use crate::fleet::State;
use std::collections::BTreeMap;

#[derive(Default)]
struct Counts {
    pending: u64,
    complete: u64,
    incomplete: u64,
    reviews: u64,
    open_requests: u64,
}

pub(super) fn lanes(state: &State) -> Vec<Json> {
    let mut counts: BTreeMap<_, Counts> = state
        .lanes
        .keys()
        .map(|id| (id, Counts::default()))
        .collect();
    for checkpoint in state.checkpoints.values() {
        let Some(row) = counts.get_mut(&checkpoint.lane) else {
            continue;
        };
        let Some(result) = &checkpoint.result else {
            row.pending += 1;
            continue;
        };
        if state.lanes[&checkpoint.lane].saved != Some(result.version) {
            continue;
        }
        if result.complete {
            row.complete += 1;
            if checkpoint.review.is_some() {
                row.reviews += 1;
            }
        } else {
            row.incomplete += 1;
        }
    }
    for request in state.review_change_requests.values() {
        if state
            .review_change_decisions
            .get(&request.id)
            .is_none_or(|decision| decision.checkpoint.is_none())
        {
            if let Some(row) = counts.get_mut(&request.lane) {
                row.open_requests += 1;
            }
        }
    }
    state
        .lanes
        .values()
        .map(|lane| {
            let row = &counts[&lane.id];
            let Json::Object(mut fields) = lane_summary(lane) else {
                unreachable!("lane summary is an object")
            };
            fields.push((
                "handoff_status".into(),
                Json::object([
                    (
                        "version",
                        lane.saved
                            .map_or(Json::Null, |version| Json::text(version.to_string())),
                    ),
                    ("pending_captures", Json::Number(row.pending)),
                    ("complete_handoffs", Json::Number(row.complete)),
                    ("incomplete_handoffs", Json::Number(row.incomplete)),
                    ("submitted_reviews", Json::Number(row.reviews)),
                    ("open_change_requests", Json::Number(row.open_requests)),
                    ("approval_authority", Json::Bool(false)),
                ]),
            ));
            Json::Object(fields)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::{
        AgentOrigin, Checkpoint, CheckpointResult, Lane, ReviewChangeDecision, ReviewChangeRequest,
    };
    use mesh_store::RecordDigest;
    fn digest(n: char) -> RecordDigest {
        RecordDigest::parse_hex(&n.to_string().repeat(64)).unwrap()
    }
    fn status(state: &State, lane: &str) -> Json {
        lanes(state)
            .into_iter()
            .find(|v| v.get("id").and_then(Json::as_text) == Some(lane))
            .unwrap()
            .get("handoff_status")
            .unwrap()
            .clone()
    }
    #[test]
    fn handoff_counts_bind_the_current_version_and_explicit_feedback_decisions() {
        let mut state = State::default();
        for id in ["one", "two"] {
            state.lanes.insert(
                id.into(),
                Lane {
                    id: id.into(),
                    parent: None,
                    created_by: None,
                    source_project: None,
                    goal: "goal".into(),
                    provider: "codex".into(),
                    base: digest('a'),
                    depth: 0,
                    workspace: None,
                    runs: vec![],
                    saved: Some(digest('b')),
                },
            );
        }
        let checkpoint =
            |version: Option<RecordDigest>, complete: bool, reviewed: bool| Checkpoint {
                lane: "one".into(),
                origin: AgentOrigin {
                    actor: "actor".into(),
                    session: "session".into(),
                    run: "run".into(),
                    generation: "generation".into(),
                },
                input_digest: "private-input".into(),
                result: version.map(|version| CheckpointResult {
                    version,
                    complete,
                    workspace_digest: "private-digest".into(),
                    saved_changes: 1,
                    issue: if complete {
                        None
                    } else {
                        Some("private-issue".into())
                    },
                }),
                review: reviewed.then_some(digest('d')),
            };
        state
            .checkpoints
            .insert("old".into(), checkpoint(Some(digest('a')), true, true));
        state
            .checkpoints
            .insert("current".into(), checkpoint(Some(digest('b')), true, true));
        state.checkpoints.insert(
            "partial".into(),
            checkpoint(Some(digest('b')), false, false),
        );
        state
            .checkpoints
            .insert("pending".into(), checkpoint(None, false, false));
        state.review_change_requests.insert(
            "request".into(),
            ReviewChangeRequest {
                id: "request".into(),
                lane: "one".into(),
                checkpoint: "old".into(),
                version: digest('a'),
                bundle: digest('d'),
                message: "private-feedback".into(),
            },
        );
        let before = state.clone();
        let projection = status(&state, "one");
        assert_eq!(
            projection,
            Json::object([
                ("version", Json::text(digest('b').to_string())),
                ("pending_captures", Json::Number(1)),
                ("complete_handoffs", Json::Number(1)),
                ("incomplete_handoffs", Json::Number(1)),
                ("submitted_reviews", Json::Number(1)),
                ("open_change_requests", Json::Number(1)),
                ("approval_authority", Json::Bool(false)),
            ])
        );
        assert!(!projection.encode().contains("private"));
        assert_eq!(state, before);
        for field in [
            "pending_captures",
            "complete_handoffs",
            "incomplete_handoffs",
            "submitted_reviews",
            "open_change_requests",
        ] {
            assert_eq!(status(&state, "two").get(field), Some(&Json::Number(0)));
        }
        state.review_change_decisions.insert(
            "request".into(),
            ReviewChangeDecision {
                revision: 1,
                checkpoint: Some("current".into()),
            },
        );
        assert_eq!(
            status(&state, "one").get("open_change_requests"),
            Some(&Json::Number(0))
        );
        state
            .review_change_decisions
            .get_mut("request")
            .unwrap()
            .checkpoint = None;
        assert_eq!(
            status(&state, "one").get("open_change_requests"),
            Some(&Json::Number(1))
        );
        state.lanes.get_mut("one").unwrap().saved = Some(digest('c'));
        let newer = status(&state, "one");
        for field in [
            "complete_handoffs",
            "incomplete_handoffs",
            "submitted_reviews",
        ] {
            assert_eq!(newer.get(field), Some(&Json::Number(0)));
        }
        assert_eq!(newer.get("pending_captures"), Some(&Json::Number(1)));
        state.lanes.get_mut("one").unwrap().saved = None;
        assert_eq!(status(&state, "one").get("version"), Some(&Json::Null));
    }
}
