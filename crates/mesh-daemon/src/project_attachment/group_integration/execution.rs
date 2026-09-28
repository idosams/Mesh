//! Historical execution records are evidence only, never authority to resume an interrupted group.
use super::{digest, Json, PinnedWorkspaceRoot};
use crate::project_attachment::recovery::{read_json, text};
use std::io;

#[derive(Clone, PartialEq, Eq)]
enum Record {
    Absent,
    Invalid,
    Present(Json),
}
fn read(root: &PinnedWorkspaceRoot, name: &str) -> Record {
    match read_json(root, name) {
        Ok((value, _)) => Record::Present(value),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Record::Absent,
        Err(_) => Record::Invalid,
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Snapshot {
    attempts: Vec<Record>,
    extra: Record,
    outcome: Record,
}
impl Snapshot {
    pub(super) fn read(root: &PinnedWorkspaceRoot, count: usize) -> Self {
        Self {
            attempts: (0..count)
                .map(|index| read(root, &format!("attempt-{index:04}.json")))
                .collect(),
            extra: read(root, &format!("attempt-{count:04}.json")),
            outcome: read(root, "group-observed.json"),
        }
    }
    pub(super) fn projection(&self, after: &Self, proposal: &Json) -> Json {
        let members = proposal.get("members").and_then(Json::as_array).unwrap();
        let proposal_digest = digest(proposal);
        let mut gap = false;
        let mut valid = self.extra == Record::Absent;
        let mut attempted = Vec::new();
        let attempts = self
            .attempts
            .iter()
            .zip(members)
            .enumerate()
            .map(|(index, (record, member))| {
                let transaction = text(member, "transaction").unwrap();
                let expected = Json::object([
                    (
                        "schema",
                        Json::text("mesh.attachment-integration-group-attempt/v1"),
                    ),
                    ("group_digest", Json::text(&proposal_digest)),
                    ("index", Json::Number(index as u64)),
                    ("transaction", Json::text(transaction)),
                    ("automatic_replay", Json::Bool(false)),
                ]);
                let status = match record {
                    Record::Absent => {
                        gap = true;
                        "absent"
                    }
                    Record::Present(value) if *value == expected => {
                        valid &= !gap;
                        "recorded"
                    }
                    _ => {
                        valid = false;
                        "invalid"
                    }
                };
                attempted.push(status == "recorded");
                Json::object([
                    ("transaction", Json::text(transaction)),
                    ("status", Json::text(status)),
                ])
            })
            .collect();
        let outcome = match &self.outcome {
            Record::Absent => None,
            Record::Present(value)
                if valid_outcome(value, members, &attempted, &proposal_digest) =>
            {
                Some(value.clone())
            }
            _ => {
                valid = false;
                None
            }
        };
        let status = if self != after {
            "changed"
        } else if !valid {
            "invalid"
        } else if outcome.is_some() {
            "recorded"
        } else {
            "no-outcome"
        };
        Json::object([
            (
                "schema",
                Json::text("mesh.attachment-integration-group-execution/v1"),
            ),
            ("status", Json::text(status)),
            ("attempts", Json::Array(attempts)),
            (
                "outcome",
                if status == "recorded" {
                    outcome.unwrap()
                } else {
                    Json::Null
                },
            ),
            ("historical", Json::Bool(true)),
            ("observation_final", Json::Bool(false)),
            ("automatic_replay", Json::Bool(false)),
            ("write_authority", Json::Bool(false)),
        ])
    }
}

fn valid_outcome(value: &Json, members: &[Json], attempted: &[bool], digest: &str) -> bool {
    let Some(status) = value.get("status").and_then(Json::as_text) else {
        return false;
    };
    let Some(results) = value.get("members").and_then(Json::as_array) else {
        return false;
    };
    if !matches!(status, "applied-observed" | "reconciliation-required")
        || results.len() != members.len()
    {
        return false;
    }
    let expected = Json::object([
        (
            "schema",
            Json::text("mesh.attachment-integration-group-result/v1"),
        ),
        ("proposal_digest", Json::text(digest)),
        ("status", Json::text(status)),
        ("members", Json::Array(results.to_vec())),
        ("observation_final", Json::Bool(false)),
        ("automatic_replay", Json::Bool(false)),
        ("displaced_files_retained", Json::Bool(true)),
    ]);
    if *value != expected {
        return false;
    }
    let mut stopped = false;
    for ((result, member), attempted) in results.iter().zip(members).zip(attempted) {
        let Some(member_status) = result.get("status").and_then(Json::as_text) else {
            return false;
        };
        let expected = Json::object([
            ("transaction", member.get("transaction").unwrap().clone()),
            ("status", Json::text(member_status)),
        ]);
        if *result != expected {
            return false;
        }
        match member_status {
            "applied-observed" if !stopped && *attempted => {}
            "reconciliation-required" if !stopped && *attempted && status != "applied-observed" => {
                stopped = true
            }
            // A failed durable marker write can leave a marker even though the executor did not
            // invoke its file operation. Do not confuse marker presence with an actual exchange.
            "not-attempted" if status != "applied-observed" && (!stopped || !*attempted) => {
                stopped = true
            }
            _ => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Json, Snapshot) {
        let members: Vec<_> = (0..3)
            .map(|index| {
                Json::object([(
                    "transaction",
                    Json::text(format!("integration-{index:032x}")),
                )])
            })
            .collect();
        let proposal = Json::object([("members", Json::Array(members.clone()))]);
        let attempts = members
            .iter()
            .enumerate()
            .map(|(index, member)| {
                Record::Present(Json::object([
                    (
                        "schema",
                        Json::text("mesh.attachment-integration-group-attempt/v1"),
                    ),
                    ("group_digest", Json::text(digest(&proposal))),
                    ("index", Json::Number(index as u64)),
                    ("transaction", member.get("transaction").unwrap().clone()),
                    ("automatic_replay", Json::Bool(false)),
                ]))
            })
            .collect();
        (
            proposal,
            Snapshot {
                attempts,
                extra: Record::Absent,
                outcome: Record::Absent,
            },
        )
    }
    fn outcome(proposal: &Json, statuses: &[&str], status: &str) -> Json {
        let members = proposal.get("members").unwrap().as_array().unwrap();
        Json::object([
            (
                "schema",
                Json::text("mesh.attachment-integration-group-result/v1"),
            ),
            ("proposal_digest", Json::text(digest(proposal))),
            ("status", Json::text(status)),
            (
                "members",
                Json::Array(
                    members
                        .iter()
                        .zip(statuses)
                        .map(|(member, status)| {
                            Json::object([
                                ("transaction", member.get("transaction").unwrap().clone()),
                                ("status", Json::text(*status)),
                            ])
                        })
                        .collect(),
                ),
            ),
            ("observation_final", Json::Bool(false)),
            ("automatic_replay", Json::Bool(false)),
            ("displaced_files_retained", Json::Bool(true)),
        ])
    }
    fn state(snapshot: &Snapshot, proposal: &Json) -> String {
        text(&snapshot.projection(snapshot, proposal), "status")
            .unwrap()
            .into()
    }

    #[test]
    fn changing_or_contradictory_execution_never_supplies_a_verified_outcome() {
        let (proposal, mut snapshot) = fixture();
        let success = outcome(&proposal, &["applied-observed"; 3], "applied-observed");
        snapshot.outcome = Record::Present(success.clone());
        assert_eq!(state(&snapshot, &proposal), "recorded");
        let mut after = snapshot.clone();
        after.outcome = Record::Absent;
        let changed = snapshot.projection(&after, &proposal);
        assert_eq!(changed.get("status"), Some(&Json::text("changed")));
        assert_eq!(changed.get("outcome"), Some(&Json::Null));
        for statuses in [
            [
                "applied-observed",
                "reconciliation-required",
                "applied-observed",
            ],
            ["not-attempted", "applied-observed", "not-attempted"],
            [
                "applied-observed",
                "reconciliation-required",
                "not-attempted",
            ],
        ] {
            snapshot.outcome =
                Record::Present(outcome(&proposal, &statuses, "reconciliation-required"));
            // The last sequence would be valid only without an attempt after the stop.
            assert_eq!(state(&snapshot, &proposal), "invalid");
        }
        snapshot.outcome = Record::Present(success);
        snapshot.attempts[1] = Record::Absent;
        assert_eq!(state(&snapshot, &proposal), "invalid");
        snapshot.outcome = Record::Absent;
        assert_eq!(state(&snapshot, &proposal), "invalid");
    }

    #[test]
    fn missing_final_record_and_failed_marker_write_remain_distinct_from_a_file_change() {
        let (proposal, mut snapshot) = fixture();
        snapshot.attempts[2] = Record::Absent;
        assert_eq!(state(&snapshot, &proposal), "no-outcome");
        snapshot.outcome = Record::Present(outcome(
            &proposal,
            &["applied-observed", "not-attempted", "not-attempted"],
            "reconciliation-required",
        ));
        // A failed marker fsync can leave the second record without calling its file executor.
        assert_eq!(state(&snapshot, &proposal), "recorded");
        snapshot.extra = snapshot.attempts[0].clone();
        assert_eq!(state(&snapshot, &proposal), "invalid");
        snapshot.extra = Record::Absent;
        snapshot.attempts[1] = snapshot.attempts[0].clone();
        assert_eq!(state(&snapshot, &proposal), "invalid");
    }
}
