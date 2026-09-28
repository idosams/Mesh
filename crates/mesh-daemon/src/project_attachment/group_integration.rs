//! One native-confirmed accepted review, with retained per-file exchanges and explicit partial outcomes.
use super::inspection::comparison_entries;
use super::{
    external_store, invalid, ObservationLimits, PreparedMainFileIntegration, ProvisionedAttachment,
};
use crate::ipc::Json;
use crate::root_authority::PinnedWorkspaceRoot;
use crate::workspace::OpenWorkspace;
use crate::TrustedReviewers;
use mesh_types::{Blake3, ContentDigest as _};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Read as _};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

const MAX_FILES: usize = 64;
fn error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
fn digest(value: &Json) -> String {
    Blake3::digest_bytes(value.encode().as_bytes()).to_string()
}

#[derive(PartialEq, Eq)]
struct Plan {
    head: String,
    exclusions: String,
    ready: Vec<String>,
    present: Vec<String>,
}

/// Native-only single-use group. Every changed path in the accepted review must be accounted for.
/// This executor supports regular-file replacement and removal; additions and directory
/// changes refuse the whole group before staging. Applying never claims filesystem-wide atomicity.
pub struct PreparedMainIntegration {
    history: ProvisionedAttachment,
    store: PinnedWorkspaceRoot,
    root: PinnedWorkspaceRoot,
    path: PathBuf,
    proposal: Json,
    plan: Plan,
    files: Vec<PreparedMainFileIntegration>,
    bundle: String,
    target: String,
    limits: ObservationLimits,
}

fn plan(
    history: &ProvisionedAttachment,
    workspace: &OpenWorkspace,
    store: &PinnedWorkspaceRoot,
    bundle: &str,
    target: &str,
    limits: ObservationLimits,
) -> io::Result<Plan> {
    let review = workspace
        .accepted_main_review()
        .map_err(error)?
        .ok_or_else(|| invalid("accepted review unavailable"))?;
    let head = super::approval::main_head(workspace)?.ok_or_else(|| invalid("main unavailable"))?;
    if review.bundle.to_string() != bundle || review.subject_operation.to_string() != target {
        return Err(invalid("accepted main changed"));
    }
    let base = workspace
        .human_approval_context(&review)
        .map_err(error)?
        .expected_canonical_head();
    if base == crate::publication::GENESIS_SHARED_HEAD {
        return Err(invalid("group replacement requires an accepted base"));
    }
    let before = comparison_entries(
        workspace
            .historical_workspace_preview(workspace.review_target_for_head(base).map_err(error)?)
            .map_err(error)?,
    );
    let after = comparison_entries(
        workspace
            .historical_workspace_preview(review.subject_operation)
            .map_err(error)?,
    );
    let captured = history.project().capture_inputs(limits)?;
    history
        .project()
        .history_configuration(store, Some(captured.exclusion_digest()))?;
    let mut result = Plan {
        head: head.to_string(),
        exclusions: captured.exclusion_digest().to_string(),
        ready: vec![],
        present: vec![],
    };
    let paths: BTreeSet<_> = before.keys().chain(after.keys()).collect();
    let mut bytes = 0u64;
    for path in paths {
        let a = before.get(path);
        let b = after.get(path);
        if a == b {
            continue;
        }
        let (a, b) = match (a, b) {
            (Some(a), b) if a.kind == "file" && b.is_none_or(|b| b.kind == "file") => (a, b),
            _ => {
                return Err(invalid(
                    "group contains an addition or directory change requiring another executor",
                ))
            }
        };
        if result.ready.len() + result.present.len() >= MAX_FILES {
            return Err(invalid("integration group exceeds file limit"));
        }
        bytes = bytes
            .checked_add(a.bytes.unwrap_or(u64::MAX))
            .and_then(|n| n.checked_add(b.map_or(0, |b| b.bytes.unwrap_or(u64::MAX))))
            .ok_or_else(|| invalid("group byte budget overflow"))?;
        if bytes > limits.bytes {
            return Err(invalid("integration group exceeds byte limit"));
        }
        if b.is_none()
            && crate::managed_file::retained_replacement::absent_parent(
                &history.project().pinned,
                Path::new(path),
            )?
            .is_some()
        {
            result.present.push(path.clone());
            continue;
        }
        let current = captured
            .files()
            .iter()
            .find(|file| file.path() == Path::new(path))
            .ok_or_else(|| invalid("group member is excluded or unavailable"))?;
        let matches = |entry: &super::inspection::ComparisonEntry| {
            entry
                .digest
                .is_some_and(|d| d.to_string() == current.digest().to_string())
                && entry.executable == Some(current.executable())
                && entry.bytes == Some(current.bytes().len() as u64)
        };
        if b.is_some_and(matches) {
            result.present.push(path.clone());
        } else if matches(a) {
            result.ready.push(path.clone());
        } else {
            return Err(invalid("group member diverged from accepted base"));
        }
    }
    Ok(result)
}

pub(super) fn prepare(
    history: ProvisionedAttachment,
    store: PinnedWorkspaceRoot,
    bundle: &str,
    target: &str,
    recovery_root: &Path,
    trusted: &TrustedReviewers,
    limits: ObservationLimits,
) -> io::Result<PreparedMainIntegration> {
    limits.validate()?;
    let planned = history.project().with_review_history(
        history.metadata_path(),
        store.clone(),
        trusted,
        |workspace, store| plan(&history, workspace, store, bundle, target, limits),
    )?;
    if planned.ready.is_empty() {
        return Err(invalid("accepted files are already present"));
    }
    let recovery = external_store(recovery_root, history.project())?;
    if recovery.identity()?.0 != history.project().device
        || recovery
            .try_clone_directory()?
            .metadata()?
            .permissions()
            .mode()
            & 0o077
            != 0
    {
        return Err(invalid(
            "group recovery must be private and on the source filesystem",
        ));
    }
    let mut nonce = [0; 16];
    File::open("/dev/urandom")?.read_exact(&mut nonce)?;
    let id = format!(
        "integration-group-{}",
        nonce.iter().map(|b| format!("{b:02x}")).collect::<String>()
    );
    let root = recovery.create_child_directory(std::ffi::OsStr::new(&id))?;
    let path = recovery_root.join(&id);
    let files = history.project().with_review_history(
        history.metadata_path(),
        store.clone(),
        trusted,
        |workspace, store| {
            let captured = history.project().capture_inputs(limits)?;
            let mut files = Vec::new();
            for relative in &planned.ready {
                files.push(super::writeback::prepare_captured(
                    &history, workspace, store, bundle, target, relative, &path, limits, &captured,
                    true,
                )?);
            }
            Ok(files)
        },
    )?;
    let mut members = Vec::new();
    for file in &files {
        members.push(Json::object([
            (
                "transaction",
                Json::text(
                    file.recovery_path()
                        .file_name()
                        .and_then(|name| name.to_str())
                        .ok_or_else(|| invalid("unrepresentable transaction"))?,
                ),
            ),
            ("proposal_digest", Json::text(digest(file.proposal()))),
            (
                "path",
                file.proposal()
                    .get("path")
                    .cloned()
                    .ok_or_else(|| invalid("missing member path"))?,
            ),
        ]));
    }
    let proposal = Json::object([
        ("schema", Json::text("mesh.attachment-integration-group/v1")),
        ("project", Json::text(history.id())),
        ("attachment", history.project().receipt()?),
        ("head", Json::text(&planned.head)),
        ("bundle", Json::text(bundle)),
        ("target", Json::text(target)),
        ("exclusions", Json::text(&planned.exclusions)),
        (
            "recovery_identity",
            Json::text(format!(
                "{:016x}:{:016x}",
                root.identity()?.0,
                root.identity()?.1
            )),
        ),
        ("members", Json::Array(members)),
        (
            "already_present",
            Json::Array(planned.present.iter().map(Json::text).collect()),
        ),
        ("automatic_replay", Json::Bool(false)),
        ("filesystem_atomic", Json::Bool(false)),
    ]);
    root.filesystem().write_new_file(
        Path::new("group-prepared.json"),
        proposal.encode().as_bytes(),
        fs::Permissions::from_mode(0o600),
    )?;
    // A changed member, main or exclusion policy invalidates the complete proposal before return.
    history.project().with_review_history(
        history.metadata_path(),
        store.clone(),
        trusted,
        |workspace, store| {
            if plan(&history, workspace, store, bundle, target, limits)? != planned {
                return Err(invalid("group changed during preparation"));
            }
            for file in &files {
                file.validate(workspace, store)?;
            }
            Ok(())
        },
    )?;
    Ok(PreparedMainIntegration {
        history,
        store,
        root,
        path,
        proposal,
        plan: planned,
        files,
        bundle: bundle.into(),
        target: target.into(),
        limits,
    })
}

impl PreparedMainIntegration {
    /// Exact bounded receipt. Its contents confer no write or replay authority.
    pub fn proposal(&self) -> &Json {
        &self.proposal
    }
    /// Each member's complete frozen before/after content is available for native confirmation.
    pub fn files(&self) -> impl Iterator<Item = &PreparedMainFileIntegration> {
        self.files.iter()
    }
    /// External retained group directory, including prepared but unattempted members after failure.
    pub fn recovery_path(&self) -> &Path {
        &self.path
    }
    /// Apply only after explicit native confirmation of the complete group. Never automatically retry.
    pub fn apply(self, trusted: &TrustedReviewers) -> io::Result<Json> {
        self.apply_with_hook(trusted, |_| {})
    }
    fn apply_with_hook(
        self,
        trusted: &TrustedReviewers,
        before_member: impl Fn(usize),
    ) -> io::Result<Json> {
        let history = self.history.clone();
        history.project().with_review_history(
            history.metadata_path(),
            self.store.clone(),
            trusted,
            |workspace, store| {
                self.root.ensure_namespace_identity()?;
                let (receipt, _) = super::recovery::read_json(&self.root, "group-prepared.json")?;
                if receipt != self.proposal
                    || plan(
                        &history,
                        workspace,
                        store,
                        &self.bundle,
                        &self.target,
                        self.limits,
                    )? != self.plan
                {
                    return Err(invalid("integration group changed before apply"));
                }
                for file in &self.files {
                    file.validate(workspace, store)?;
                }
                let mut results = Vec::new();
                let mut stopped = false;
                for (index, file) in self.files.into_iter().enumerate() {
                    let transaction = file
                        .recovery_path()
                        .file_name()
                        .and_then(|name| name.to_str())
                        .ok_or_else(|| invalid("invalid member transaction"))?
                        .to_owned();
                    let status = if stopped {
                        "not-attempted"
                    } else {
                        before_member(index);
                        let attempt = Json::object([
                            (
                                "schema",
                                Json::text("mesh.attachment-integration-group-attempt/v1"),
                            ),
                            ("group_digest", Json::text(digest(&self.proposal))),
                            ("index", Json::Number(index as u64)),
                            ("transaction", Json::text(&transaction)),
                            ("automatic_replay", Json::Bool(false)),
                        ]);
                        // This record precedes the exchange. Its absence proves this executor did not
                        // start that member; its presence alone never proves whether exchange happened.
                        if self
                            .root
                            .filesystem()
                            .write_new_file(
                                Path::new(&format!("attempt-{index:04}.json")),
                                attempt.encode().as_bytes(),
                                fs::Permissions::from_mode(0o600),
                            )
                            .is_err()
                        {
                            stopped = true;
                            "not-attempted"
                        } else {
                            match file
                                .validate(workspace, store)
                                .and_then(|_| file.apply_validated())
                            {
                                Ok(result)
                                    if result.get("status")
                                        == Some(&Json::text("applied-observed")) =>
                                {
                                    "applied-observed"
                                }
                                _ => {
                                    stopped = true;
                                    "reconciliation-required"
                                }
                            }
                        }
                    };
                    results.push(Json::object([
                        ("transaction", Json::text(transaction)),
                        ("status", Json::text(status)),
                    ]));
                }
                let complete = !stopped
                    && plan(
                        &history,
                        workspace,
                        store,
                        &self.bundle,
                        &self.target,
                        self.limits,
                    )
                    .is_ok_and(|current| {
                        current.ready.is_empty()
                            && current.head == self.plan.head
                            && current.exclusions == self.plan.exclusions
                    });
                let result = Json::object([
                    (
                        "schema",
                        Json::text("mesh.attachment-integration-group-result/v1"),
                    ),
                    ("proposal_digest", Json::text(digest(&self.proposal))),
                    (
                        "status",
                        Json::text(if complete {
                            "applied-observed"
                        } else {
                            "reconciliation-required"
                        }),
                    ),
                    ("members", Json::Array(results)),
                    ("observation_final", Json::Bool(false)),
                    ("automatic_replay", Json::Bool(false)),
                    ("displaced_files_retained", Json::Bool(true)),
                ]);
                self.root.filesystem().write_new_file(
                    Path::new("group-observed.json"),
                    result.encode().as_bytes(),
                    fs::Permissions::from_mode(0o600),
                )?;
                Ok(result)
            },
        )
    }
}

/// Read retained group membership and independently verified member recovery evidence. No replay.
pub(super) fn inspect(
    history: &ProvisionedAttachment,
    store: PinnedWorkspaceRoot,
    recovery_root: &Path,
    id: &str,
    trusted: &TrustedReviewers,
    limits: ObservationLimits,
) -> io::Result<Json> {
    use super::recovery::{read_json, text, transaction};
    limits.validate()?;
    let suffix = id
        .strip_prefix("integration-group-")
        .ok_or_else(|| invalid("invalid group identity"))?;
    if suffix.len() != 32
        || !suffix
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid("invalid group identity"));
    }
    let outer = external_store(recovery_root, history.project())?;
    let root = outer.open_child_directory(std::ffi::OsStr::new(id))?;
    if outer
        .try_clone_directory()?
        .metadata()?
        .permissions()
        .mode()
        & 0o077
        != 0
        || root.try_clone_directory()?.metadata()?.permissions().mode() & 0o077 != 0
    {
        return Err(invalid("group recovery is not private"));
    }
    let (proposal, _) = read_json(&root, "group-prepared.json")?;
    let fields = [
        "schema",
        "project",
        "attachment",
        "head",
        "bundle",
        "target",
        "exclusions",
        "recovery_identity",
        "members",
        "already_present",
        "automatic_replay",
        "filesystem_atomic",
    ];
    if !matches!(&proposal, Json::Object(pairs) if pairs.len() == fields.len() && fields.iter().all(|key| proposal.get(key).is_some()))
        || text(&proposal, "schema")? != "mesh.attachment-integration-group/v1"
        || text(&proposal, "project")? != history.id()
        || proposal.get("attachment") != Some(&history.project().receipt()?)
        || text(&proposal, "recovery_identity")?
            != format!("{:016x}:{:016x}", root.identity()?.0, root.identity()?.1)
        || proposal.get("automatic_replay") != Some(&Json::Bool(false))
        || proposal.get("filesystem_atomic") != Some(&Json::Bool(false))
    {
        return Err(invalid("group binding changed"));
    }
    let members = proposal
        .get("members")
        .and_then(Json::as_array)
        .ok_or_else(|| invalid("invalid group members"))?;
    let present = proposal
        .get("already_present")
        .and_then(Json::as_array)
        .ok_or_else(|| invalid("invalid present members"))?;
    if members.is_empty() || members.len() + present.len() > MAX_FILES {
        return Err(invalid("group membership exceeds limit"));
    }
    let mut paths = BTreeSet::new();
    let mut transactions = BTreeSet::new();
    let mut receipts = Vec::new();
    for member in members {
        if !matches!(member, Json::Object(pairs) if pairs.len() == 3) {
            return Err(invalid("invalid group member"));
        }
        let tx = text(member, "transaction")?;
        if !transaction(tx) || !transactions.insert(tx) || !paths.insert(text(member, "path")?) {
            return Err(invalid("ambiguous group membership"));
        }
        let child = root.open_child_directory(std::ffi::OsStr::new(tx))?;
        let (receipt, _) = read_json(&child, "prepared.json")?;
        if digest(&receipt) != text(member, "proposal_digest")?
            || receipt.get("path") != member.get("path")
            || ["head", "bundle", "target", "exclusions"]
                .iter()
                .any(|key| receipt.get(key) != proposal.get(key))
        {
            return Err(invalid("member is not bound to this group"));
        }
        receipts.push(receipt);
    }
    for item in present {
        if !paths.insert(
            item.as_text()
                .ok_or_else(|| invalid("invalid present path"))?,
        ) {
            return Err(invalid("duplicate group path"));
        }
    }
    // Coverage is rederived from verified history, not from the mutable group receipt alone.
    history.project().with_review_history(
        history.metadata_path(),
        store.clone(),
        trusted,
        |workspace, _| {
            for receipt in &receipts {
                super::recovery::verify_history(receipt, workspace, trusted)?;
            }
            let bundle =
                mesh_store::RecordDigest::parse_hex(text(&proposal, "bundle")?).map_err(error)?;
            let review = workspace
                .review(&bundle)
                .ok_or_else(|| invalid("group review unavailable"))?;
            let base = workspace
                .human_approval_context(&review)
                .map_err(error)?
                .expected_canonical_head();
            let before = comparison_entries(
                workspace
                    .historical_workspace_preview(
                        workspace.review_target_for_head(base).map_err(error)?,
                    )
                    .map_err(error)?,
            );
            let after = comparison_entries(
                workspace
                    .historical_workspace_preview(review.subject_operation)
                    .map_err(error)?,
            );
            let all: BTreeSet<_> = before.keys().chain(after.keys()).collect();
            let changed: BTreeSet<_> = all
                .into_iter()
                .filter(|path| before.get(*path) != after.get(*path))
                .map(String::as_str)
                .collect();
            if paths != changed {
                return Err(invalid("group does not cover its complete accepted review"));
            }
            Ok(())
        },
    )?;
    let group_path = recovery_root.join(id);
    let mut observations = Vec::new();
    let mut remaining = limits.bytes;
    for member in members {
        let tx = text(member, "transaction")?;
        let mut bounded = limits;
        bounded.bytes = remaining;
        // Every member inspection revalidates source/store identity and the actual approval.
        let observed = super::recovery::inspect_recovery(
            history,
            store.clone(),
            &group_path,
            Some(tx),
            trusted,
            bounded,
        )?;
        remaining = observed
            .get("live_content_budget_remaining")
            .and_then(Json::as_u64)
            .ok_or_else(|| invalid("member recovery budget unavailable"))?;
        observations.push(Json::object([
            ("transaction", Json::text(tx)),
            ("recovery", observed),
        ]));
    }
    root.ensure_namespace_identity()?;
    outer.ensure_namespace_identity()?;
    if read_json(&root, "group-prepared.json")?.0 != proposal {
        return Err(invalid("group receipt changed during inspection"));
    }
    Ok(Json::object([
        (
            "schema",
            Json::text("mesh.attachment-integration-group-recovery/v1"),
        ),
        ("project", Json::text(history.id())),
        ("group", Json::text(id)),
        ("proposal_digest", Json::text(digest(&proposal))),
        ("members", Json::Array(observations)),
        ("already_present", Json::Array(present.to_vec())),
        ("already_present_is_preparation_evidence", Json::Bool(true)),
        ("observations_are_atomic", Json::Bool(false)),
        ("automatic_replay", Json::Bool(false)),
        ("write_authority", Json::Bool(false)),
    ]))
}

#[cfg(test)]
#[path = "group_integration_tests.rs"]
mod tests;
