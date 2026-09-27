//! Read-only three-way divergence evidence. No preview conveys source-write authority.
use super::inspection::{comparison_entries, ComparisonEntry};
use super::{invalid, ObservationLimits, ProjectAttachment};
use crate::ipc::Json;
use crate::root_authority::PinnedWorkspaceRoot;
use crate::TrustedReviewers;
use mesh_store::RecordDigest;
use mesh_types::{Blake3, ContentDigest as _};
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

const PAGE_SIZE: usize = 200;

fn error(problem: impl std::fmt::Display) -> io::Error {
    io::Error::other(problem.to_string())
}

struct Difference<'a> {
    base: Option<&'a ComparisonEntry>,
    target: Option<&'a ComparisonEntry>,
    current: Option<&'a ComparisonEntry>,
    status: &'static str,
    reason: Option<&'static str>,
}

impl ProjectAttachment {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn preview_main_integration(
        &self,
        metadata: &Path,
        store: PinnedWorkspaceRoot,
        bundle: &str,
        target: &str,
        trusted: &TrustedReviewers,
        limits: ObservationLimits,
    ) -> io::Result<Json> {
        limits.validate()?;
        self.with_review_history(metadata, store, trusted, |workspace, store| {
            let head = super::approval::main_head(workspace)?
                .ok_or_else(|| invalid("Mesh main has no approved version"))?;
            let review = workspace
                .accepted_main_review()
                .map_err(error)?
                .ok_or_else(|| invalid("accepted review unavailable"))?;
            if review.bundle.to_string() != bundle || review.subject_operation.to_string() != target
            {
                return Err(invalid("Mesh main changed; refresh its exact review"));
            }
            let context = workspace.human_approval_context(&review).map_err(error)?;
            let base_head = context.expected_canonical_head();
            let base = if base_head == crate::publication::GENESIS_SHARED_HEAD {
                BTreeMap::new()
            } else {
                comparison_entries(
                    workspace
                        .historical_workspace_preview(
                            workspace.review_target_for_head(base_head).map_err(error)?,
                        )
                        .map_err(error)?,
                )
            };
            let proposed = comparison_entries(
                workspace
                    .historical_workspace_preview(review.subject_operation)
                    .map_err(error)?,
            );
            let captured = self.capture_inputs(limits)?;
            // Exclusion changes cannot masquerade as missing files or authorize a destructive diff.
            self.history_configuration(store, Some(captured.exclusion_digest()))?;
            let mut current = BTreeMap::new();
            for path in captured.directories() {
                let path = path
                    .to_str()
                    .ok_or_else(|| invalid("unrepresentable current directory"))?;
                current.insert(
                    path.to_owned(),
                    ComparisonEntry {
                        kind: "folder",
                        bytes: None,
                        digest: None,
                        executable: None,
                    },
                );
            }
            for file in captured.files() {
                let path = file
                    .path()
                    .to_str()
                    .ok_or_else(|| invalid("unrepresentable current file"))?;
                current.insert(
                    path.to_owned(),
                    ComparisonEntry {
                        kind: "file",
                        bytes: Some(file.bytes().len() as u64),
                        digest: Some(RecordDigest::from_bytes(*file.digest().as_bytes())),
                        executable: Some(file.executable()),
                    },
                );
            }
            let observed = Json::object([
                ("attachment", self.receipt()?),
                (
                    "exclusions",
                    Json::text(captured.exclusion_digest().to_string()),
                ),
                (
                    "entries",
                    Json::Array(
                        current
                            .iter()
                            .map(|(path, entry)| {
                                Json::object([("path", Json::text(path)), ("entry", entry.json())])
                            })
                            .collect(),
                    ),
                ),
            ]);
            let observed_digest = Blake3::digest_bytes(observed.encode().as_bytes()).to_string();
            let paths: BTreeSet<_> = base
                .keys()
                .chain(proposed.keys())
                .chain(current.keys())
                .collect();
            let mut differences = BTreeMap::new();
            for path in paths {
                let before = base.get(path);
                let after = proposed.get(path);
                let live = current.get(path);
                if before == after && after == live {
                    continue;
                }
                let status = if before == after {
                    "preserve-current"
                } else if live == after {
                    "already-present"
                } else if live == before {
                    "matches-base"
                } else {
                    "conflict"
                };
                differences.insert(
                    path.as_str(),
                    Difference {
                        base: before,
                        target: after,
                        current: live,
                        status,
                        reason: (status == "conflict").then_some("current-content-diverged"),
                    },
                );
            }

            // Folder metadata alone cannot prove that its contents are replaceable. Inspect every
            // directory being removed, including ignored immediate names, without reading ignored
            // content. A child absent from capture blocks the destructive ancestor.
            let destructive: Vec<_> = differences
                .iter()
                .filter_map(|(path, diff)| {
                    (diff.status == "matches-base"
                        && diff.current.is_some_and(|entry| entry.kind == "folder")
                        && !diff.target.is_some_and(|entry| entry.kind == "folder"))
                    .then_some(*path)
                })
                .collect();
            let mut enumerated = 0usize;
            for path in destructive.into_iter().rev() {
                let prefix = format!("{path}/");
                let diverged_child = current
                    .keys()
                    .filter(|child| child.starts_with(&prefix))
                    .any(|child| {
                        differences
                            .get(child.as_str())
                            .is_none_or(|diff| diff.status != "matches-base")
                    });
                let names = self.pinned.filesystem().read_directory_names_bounded(
                    Path::new(path),
                    limits.entries.saturating_sub(enumerated).saturating_add(1),
                )?;
                enumerated = enumerated.saturating_add(names.len());
                if enumerated > limits.entries {
                    return Err(invalid(
                        "integration directory inspection exceeded its budget",
                    ));
                }
                let unobserved_child = names.iter().any(|name| {
                    name.to_str()
                        .is_none_or(|name| !current.contains_key(&format!("{path}/{name}")))
                });
                if diverged_child || unobserved_child {
                    let diff = differences
                        .get_mut(path)
                        .expect("destructive difference exists");
                    diff.status = "conflict";
                    diff.reason = Some(if unobserved_child {
                        "contains-unobserved-entry"
                    } else {
                        "contains-current-work"
                    });
                }
            }
            // A source parent removed or replaced by an ordinary tool must not be recreated just
            // to apply a child. Grouped directory changes also wait for any conflicting ancestor.
            let changed: Vec<_> = differences.keys().copied().collect();
            for path in changed {
                let diff = &differences[path];
                if diff.status != "matches-base" {
                    continue;
                }
                let needs_parent = diff.target.is_some();
                let blocked = Path::new(path)
                    .ancestors()
                    .skip(1)
                    .filter_map(|parent| parent.to_str())
                    .filter(|parent| !parent.is_empty())
                    .any(|parent| {
                        let parent_diff = differences.get(parent);
                        parent_diff
                            .is_some_and(|diff| matches!(diff.status, "conflict" | "blocked"))
                            || (needs_parent
                                && !current
                                    .get(parent)
                                    .is_some_and(|entry| entry.kind == "folder")
                                && !parent_diff.is_some_and(|diff| diff.status == "matches-base"))
                    });
                if blocked {
                    let diff = differences.get_mut(path).expect("difference exists");
                    diff.status = "blocked";
                    diff.reason = Some("parent-change-conflicts");
                }
            }
            let count = |status| {
                differences
                    .values()
                    .filter(|diff| diff.status == status)
                    .count() as u64
            };
            let entries = differences
                .iter()
                .take(PAGE_SIZE)
                .map(|(path, diff)| {
                    Json::object([
                        ("path", Json::text(*path)),
                        ("status", Json::text(diff.status)),
                        ("reason", diff.reason.map_or(Json::Null, Json::text)),
                        ("base", diff.base.map_or(Json::Null, ComparisonEntry::json)),
                        (
                            "target",
                            diff.target.map_or(Json::Null, ComparisonEntry::json),
                        ),
                        (
                            "current",
                            diff.current.map_or(Json::Null, ComparisonEntry::json),
                        ),
                    ])
                })
                .collect();
            Ok(Json::object([
                (
                    "schema",
                    Json::text("mesh.attachment-integration-preview/v1"),
                ),
                ("head", Json::text(head.to_string())),
                ("bundle", Json::text(bundle)),
                ("target", Json::text(target)),
                ("base_head", Json::text(base_head.to_string())),
                ("observed_digest", Json::text(observed_digest)),
                ("atomic_snapshot", Json::Bool(false)),
                ("write_authority", Json::Bool(false)),
                ("matches_base", Json::Number(count("matches-base"))),
                ("already_present", Json::Number(count("already-present"))),
                ("preserve_current", Json::Number(count("preserve-current"))),
                ("conflicts", Json::Number(count("conflict"))),
                ("blocked", Json::Number(count("blocked"))),
                ("entries", Json::Array(entries)),
                (
                    "not_listed",
                    Json::Number(differences.len().saturating_sub(PAGE_SIZE) as u64),
                ),
            ]))
        })
    }
}
