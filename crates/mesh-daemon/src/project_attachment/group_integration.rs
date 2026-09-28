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
mod execution;
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
    trees: BTreeSet<String>,
    removed_trees: BTreeSet<String>,
    converted_trees: BTreeSet<String>,
}

/// Native-only single-use group. Every changed path in the accepted review must be accounted for.
/// Supports regular-file changes, complete directory addition/removal, and file/directory conversion.
/// Applying never claims filesystem-wide atomicity.
pub struct PreparedMainIntegration {
    history: ProvisionedAttachment,
    store: PinnedWorkspaceRoot,
    root: PinnedWorkspaceRoot,
    path: PathBuf,
    proposal: Json,
    plan: Plan,
    files: Vec<Member>,
    bundle: String,
    target: String,
    limits: ObservationLimits,
}

enum Member {
    File(Box<PreparedMainFileIntegration>),
    Directory(Box<super::PreparedMainDirectoryAddition>),
}
impl Member {
    fn proposal(&self) -> &Json {
        match self {
            Self::File(file) => file.proposal(),
            Self::Directory(tree) => tree.proposal(),
        }
    }
    fn recovery_path(&self) -> &Path {
        match self {
            Self::File(file) => file.recovery_path(),
            Self::Directory(tree) => tree.recovery_path(),
        }
    }
    fn validate(
        &self,
        workspace: &OpenWorkspace,
        store: &PinnedWorkspaceRoot,
        trusted: &TrustedReviewers,
    ) -> io::Result<()> {
        match self {
            Self::File(file) => file.validate(workspace, store),
            Self::Directory(tree) => tree.validate(workspace, store, trusted),
        }
    }
    fn apply_validated(self) -> io::Result<Json> {
        match self {
            Self::File(file) => file.apply_validated(),
            Self::Directory(tree) => tree.apply_validated(),
        }
    }
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
    let before = if base == crate::publication::GENESIS_SHARED_HEAD {
        Default::default()
    } else {
        comparison_entries(
            workspace
                .historical_workspace_preview(
                    workspace.review_target_for_head(base).map_err(error)?,
                )
                .map_err(error)?,
        )
    };
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
        trees: BTreeSet::new(),
        removed_trees: BTreeSet::new(),
        converted_trees: BTreeSet::new(),
    };
    let paths: BTreeSet<_> = before.keys().chain(after.keys()).collect();
    let changed: Vec<_> = paths
        .into_iter()
        .filter(|path| before.get(*path) != after.get(*path))
        .collect();
    if changed.len() > MAX_FILES.min(limits.entries) {
        return Err(invalid("integration group exceeds entry limit"));
    }
    let mut bytes = 0u64;
    for path in &changed {
        for entry in [before.get(*path), after.get(*path)].into_iter().flatten() {
            if entry.kind == "file" {
                bytes = bytes
                    .checked_add(
                        entry
                            .bytes
                            .ok_or_else(|| invalid("missing group file size"))?,
                    )
                    .ok_or_else(|| invalid("group byte budget overflow"))?;
            }
        }
        if bytes > limits.bytes {
            return Err(invalid("integration group exceeds byte limit"));
        }
    }
    let mut covered = BTreeSet::new();
    for path in changed {
        if covered.contains(path) {
            continue;
        }
        let a = before.get(path);
        let b = after.get(path);
        if a.is_none() && b.is_some_and(|entry| entry.kind == "folder") {
            let prefix = format!("{path}/");
            let subtree: Vec<_> = after
                .iter()
                .filter(|(member, _)| *member == path || member.starts_with(&prefix))
                .collect();
            for (member, _) in &subtree {
                if before.contains_key(*member) || !captured.admits_file_path(member)? {
                    return Err(invalid("new tree contains existing or excluded members"));
                }
                covered.insert((*member).clone());
            }
            if crate::managed_file::retained_replacement::absent_parent(
                &history.project().pinned,
                Path::new(path),
            )?
            .is_some()
            {
                result.trees.insert(path.clone());
                result.ready.push(path.clone());
            } else {
                for (member, entry) in subtree {
                    let matches = if entry.kind == "folder" {
                        captured
                            .directories()
                            .iter()
                            .any(|directory| directory == Path::new(member))
                    } else {
                        captured.files().iter().any(|file| {
                            file.path() == Path::new(member)
                                && entry
                                    .digest
                                    .is_some_and(|d| d.to_string() == file.digest().to_string())
                                && entry.executable == Some(file.executable())
                                && entry.bytes == Some(file.bytes().len() as u64)
                        })
                    };
                    if !matches {
                        return Err(invalid("new directory destination is partial or divergent"));
                    }
                    result.present.push(member.clone());
                }
            }
            continue;
        }
        if b.is_none() && a.is_some_and(|entry| entry.kind == "folder") {
            let prefix = format!("{path}/");
            let subtree: Vec<_> = before
                .iter()
                .filter(|(member, _)| *member == path || member.starts_with(&prefix))
                .collect();
            for (member, _) in &subtree {
                if after.contains_key(*member) || !captured.admits_file_path(member)? {
                    return Err(invalid(
                        "removed tree contains surviving or excluded members",
                    ));
                }
                covered.insert((*member).clone());
            }
            if crate::managed_file::retained_replacement::absent_parent(
                &history.project().pinned,
                Path::new(path),
            )?
            .is_some()
            {
                result
                    .present
                    .extend(subtree.iter().map(|(path, _)| (*path).clone()));
            } else {
                for (member, entry) in subtree {
                    let matches = if entry.kind == "folder" {
                        captured
                            .directories()
                            .iter()
                            .any(|directory| directory == Path::new(member))
                    } else {
                        captured.files().iter().any(|file| {
                            file.path() == Path::new(member)
                                && entry
                                    .digest
                                    .is_some_and(|d| d.to_string() == file.digest().to_string())
                                && entry.executable == Some(file.executable())
                                && entry.bytes == Some(file.bytes().len() as u64)
                        })
                    };
                    if !matches {
                        return Err(invalid(
                            "removed directory contains divergent or unavailable work",
                        ));
                    }
                }
                result.trees.insert(path.clone());
                result.removed_trees.insert(path.clone());
                result.ready.push(path.clone());
            }
            continue;
        }
        if matches!(
            (a.map(|entry| entry.kind), b.map(|entry| entry.kind)),
            (Some("file"), Some("folder")) | (Some("folder"), Some("file"))
        ) {
            let prefix = format!("{path}/");
            let members: BTreeSet<_> = before
                .keys()
                .chain(after.keys())
                .filter(|member| *member == path || member.starts_with(&prefix))
                .collect();
            for member in members {
                if !captured.admits_file_path(member)? {
                    return Err(invalid("conversion contains excluded members"));
                }
                covered.insert(member.clone());
            }
            let matches_tree =
                |tree: &std::collections::BTreeMap<String, super::inspection::ComparisonEntry>| {
                    tree.iter()
                        .filter(|(member, _)| *member == path || member.starts_with(&prefix))
                        .all(|(member, entry)| {
                            if entry.kind == "folder" {
                                captured
                                    .directories()
                                    .iter()
                                    .any(|directory| directory == Path::new(member))
                            } else {
                                captured.files().iter().any(|file| {
                                    file.path() == Path::new(member)
                                        && entry.digest.is_some_and(|d| {
                                            d.to_string() == file.digest().to_string()
                                        })
                                        && entry.executable == Some(file.executable())
                                        && entry.bytes == Some(file.bytes().len() as u64)
                                })
                            }
                        })
                };
            if matches_tree(&after) {
                result.present.extend(
                    covered
                        .iter()
                        .filter(|member| *member == path || member.starts_with(&prefix))
                        .cloned(),
                );
            } else if matches_tree(&before) {
                result.trees.insert(path.clone());
                result.converted_trees.insert(path.clone());
                result.ready.push(path.clone());
            } else {
                return Err(invalid(
                    "conversion source differs from approved base and result",
                ));
            }
            continue;
        }
        if a.is_some_and(|entry| entry.kind != "file")
            || b.is_some_and(|entry| entry.kind != "file")
        {
            return Err(invalid(
                "group contains a file/directory type change requiring another executor",
            ));
        }
        if !captured.admits_file_path(path)? {
            return Err(invalid("group member is excluded by capture policy"));
        }
        if (a.is_none() || b.is_none())
            && crate::managed_file::retained_replacement::absent_parent(
                &history.project().pinned,
                Path::new(path),
            )?
            .is_some()
        {
            if b.is_none() {
                result.present.push(path.clone());
            } else {
                result.ready.push(path.clone());
            }
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
        } else if a.is_some_and(matches) {
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
                files.push(if planned.trees.contains(relative) {
                    Member::Directory(Box::new(super::directory_writeback::prepare_captured(
                        &history, workspace, store, bundle, target, relative, &path, trusted,
                        limits, &captured, None,
                    )?))
                } else {
                    Member::File(Box::new(super::writeback::prepare_captured(
                        &history, workspace, store, bundle, target, relative, &path, limits,
                        &captured, true,
                    )?))
                });
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
        (
            "schema",
            Json::text(if !planned.converted_trees.is_empty() {
                "mesh.attachment-integration-group/v4"
            } else if !planned.removed_trees.is_empty() {
                "mesh.attachment-integration-group/v3"
            } else if planned.trees.is_empty() {
                "mesh.attachment-integration-group/v1"
            } else {
                "mesh.attachment-integration-group/v2"
            }),
        ),
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
                file.validate(workspace, store, trusted)?;
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
        self.files.iter().filter_map(|member| match member {
            Member::File(file) => Some(file.as_ref()),
            _ => None,
        })
    }
    /// Complete added or removed subtrees, including frozen confirmation content and empty directories.
    pub fn directories(&self) -> impl Iterator<Item = &super::PreparedMainDirectoryChange> {
        self.files.iter().filter_map(|member| match member {
            Member::Directory(tree) => Some(tree.as_ref()),
            _ => None,
        })
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
                    file.validate(workspace, store, trusted)?;
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
                                .validate(workspace, store, trusted)
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
    if !super::recovery::group_identity(id) {
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
        || !matches!(
            text(&proposal, "schema")?,
            "mesh.attachment-integration-group/v1"
                | "mesh.attachment-integration-group/v2"
                | "mesh.attachment-integration-group/v3"
                | "mesh.attachment-integration-group/v4"
        )
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
    let directory_members = text(&proposal, "schema")? != "mesh.attachment-integration-group/v1";
    let mut paths = BTreeSet::new();
    let mut transactions = BTreeSet::new();
    let mut receipts = Vec::new();
    for member in members {
        if !matches!(member, Json::Object(pairs) if pairs.len() == 3) {
            return Err(invalid("invalid group member"));
        }
        let tx = text(member, "transaction")?;
        if !(transaction(tx) || directory_members && super::directory_writeback::transaction(tx))
            || !transactions.insert(tx)
            || !paths.insert(text(member, "path")?.to_owned())
        {
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
                .ok_or_else(|| invalid("invalid present path"))?
                .to_owned(),
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
                if matches!(
                    receipt.get("schema").and_then(Json::as_text),
                    Some(
                        "mesh.attachment-directory-addition/v1"
                            | "mesh.attachment-directory-removal/v1"
                            | "mesh.attachment-entry-conversion/v1"
                    )
                ) {
                    if !directory_members {
                        return Err(invalid("directory member requires group v2"));
                    }
                    if receipt.get("schema")
                        == Some(&Json::text("mesh.attachment-directory-removal/v1"))
                        && !matches!(
                            text(&proposal, "schema")?,
                            "mesh.attachment-integration-group/v3"
                                | "mesh.attachment-integration-group/v4"
                        )
                    {
                        return Err(invalid("directory removal requires group v3"));
                    }
                    if receipt.get("schema")
                        == Some(&Json::text("mesh.attachment-entry-conversion/v1"))
                        && text(&proposal, "schema")? != "mesh.attachment-integration-group/v4"
                    {
                        return Err(invalid("entry conversion requires group v4"));
                    }
                    super::directory_writeback::verify_tree(receipt, workspace, trusted)?;
                    let root = text(receipt, "path")?;
                    let mut member_paths = BTreeSet::new();
                    for tree in [receipt.get("before_tree"), receipt.get("tree")]
                        .into_iter()
                        .flatten()
                    {
                        for entry in tree
                            .as_array()
                            .ok_or_else(|| invalid("invalid conversion coverage"))?
                        {
                            member_paths.insert(text(entry, "path")?);
                        }
                    }
                    for relative in member_paths {
                        if !relative.is_empty() && !paths.insert(format!("{root}/{relative}")) {
                            return Err(invalid("overlapping directory group members"));
                        }
                    }
                } else {
                    super::recovery::verify_history(receipt, workspace, trusted)?;
                }
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
            let before = if base == crate::publication::GENESIS_SHARED_HEAD {
                Default::default()
            } else {
                comparison_entries(
                    workspace
                        .historical_workspace_preview(
                            workspace.review_target_for_head(base).map_err(error)?,
                        )
                        .map_err(error)?,
                )
            };
            let after = comparison_entries(
                workspace
                    .historical_workspace_preview(review.subject_operation)
                    .map_err(error)?,
            );
            let all: BTreeSet<_> = before.keys().chain(after.keys()).collect();
            let changed: BTreeSet<_> = all
                .into_iter()
                .filter(|path| before.get(*path) != after.get(*path))
                .cloned()
                .collect();
            if paths != changed || paths.len() > MAX_FILES.min(limits.entries) {
                return Err(invalid("group does not cover its complete accepted review"));
            }
            Ok(())
        },
    )?;
    let group_path = recovery_root.join(id);
    let execution_before = execution::Snapshot::read(&root, members.len());
    let mut observations = Vec::new();
    let mut remaining = limits.bytes;
    for member in members {
        let tx = text(member, "transaction")?;
        let mut bounded = limits;
        bounded.bytes = remaining;
        // Every member inspection revalidates source/store identity and the actual approval.
        let observed = if super::directory_writeback::transaction(tx) {
            super::directory_writeback::inspect(
                history,
                store.clone(),
                &group_path,
                tx,
                trusted,
                bounded,
            )?
        } else {
            super::recovery::inspect_recovery(
                history,
                store.clone(),
                &group_path,
                Some(tx),
                trusted,
                bounded,
            )?
        };
        remaining = observed
            .get("live_content_budget_remaining")
            .and_then(Json::as_u64)
            .ok_or_else(|| invalid("member recovery budget unavailable"))?;
        observations.push(Json::object([
            ("transaction", Json::text(tx)),
            ("recovery", observed),
        ]));
    }
    let (children, more_restorations) = root
        .filesystem()
        .read_directory_prefix(Path::new(""), 32.min(limits.entries))?;
    let restorations: Vec<_> = children
        .iter()
        .filter_map(|name| name.to_str())
        .filter(|name| name.starts_with("restoration-") && transaction(name))
        .map(Json::text)
        .collect();
    let entry_restorations: Vec<_> = children
        .iter()
        .filter_map(|name| name.to_str())
        .filter(|name| super::entry_restoration::transaction(name))
        .map(Json::text)
        .collect();
    let execution_after = execution::Snapshot::read(&root, members.len());
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
        (
            "execution",
            execution_before.projection(&execution_after, &proposal),
        ),
        ("restoration_references", Json::Array(restorations)),
        (
            "entry_restoration_references",
            Json::Array(entry_restorations),
        ),
        (
            "more_restoration_references_may_exist",
            Json::Bool(more_restorations),
        ),
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
