//! Native-only whole-entry restoration. Frozen private work never becomes approved main.
use super::{external_store, invalid, recovery, ObservationLimits, ProvisionedAttachment};
use crate::managed_file::retained_replacement::{EntryLimits, RetainedEntryRestoration};
use crate::{ipc::Json, root_authority::PinnedWorkspaceRoot, TrustedReviewers};
use mesh_types::{Blake3, ContentDigest as _};
use std::os::unix::fs::PermissionsExt as _;
use std::{
    ffi::OsStr,
    fs::{self, File},
    io::{self, Read as _},
    path::{Path, PathBuf},
};

const SCHEMA: &str = "mesh.attachment-entry-restoration/v1";
const KEYS: &[&str] = &[
    "schema",
    "project",
    "attachment",
    "store_identity",
    "recovery_identity",
    "path",
    "origin_transaction",
    "origin_proposal_digest",
    "origin_identity",
    "origin_tree",
    "current_tree",
    "installed_tree",
    "source_parent",
    "parent_metadata_digest",
    "parent_mode",
    "exclusions",
    "automatic_replay",
];
fn digest(value: &Json) -> String {
    Blake3::digest_bytes(value.encode().as_bytes()).to_string()
}
fn text<'a>(value: &'a Json, key: &str) -> io::Result<&'a str> {
    recovery::text(value, key)
}
fn identity(root: &PinnedWorkspaceRoot) -> io::Result<Json> {
    let (dev, ino) = root.identity()?;
    Ok(Json::text(format!("{dev:016x}:{ino:016x}")))
}
fn private(root: &PinnedWorkspaceRoot) -> io::Result<()> {
    root.ensure_namespace_identity()?;
    if root.try_clone_directory()?.metadata()?.permissions().mode() & 0o077 != 0 {
        return Err(invalid("entry restoration storage is not private"));
    }
    Ok(())
}
fn transaction(value: &str) -> bool {
    value.strip_prefix("entry-restoration-").is_some_and(|id| {
        id.len() == 32
            && id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
fn tree(value: &Json) -> io::Result<&[Json]> {
    let entries = value
        .as_array()
        .ok_or_else(|| invalid("entry evidence unavailable"))?;
    if entries.is_empty() || entries.len() > 64 {
        return Err(invalid("entry evidence exceeds limit"));
    }
    let mut previous = None;
    let mut directories = std::collections::BTreeSet::new();
    for entry in entries {
        let keys = [
            "path",
            "kind",
            "installation",
            "mode",
            "metadata",
            "digest",
            "bytes",
        ];
        if !matches!(entry, Json::Object(fields) if fields.len() == keys.len() && keys.iter().all(|key| entry.get(key).is_some()))
        {
            return Err(invalid("invalid entry evidence fields"));
        }
        let path = text(entry, "path")?;
        if previous.is_some_and(|prev| prev >= path)
            || (previous.is_none() && !path.is_empty())
            || (!path.is_empty()
                && path.split('/').any(|part| {
                    part.is_empty()
                        || part == "."
                        || part == ".."
                        || part.eq_ignore_ascii_case(".git")
                }))
        {
            return Err(invalid("invalid entry evidence path"));
        }
        if !path.is_empty()
            && !directories.contains(path.rsplit_once('/').map_or("", |(parent, _)| parent))
        {
            return Err(invalid("entry parent evidence missing"));
        }
        if !recovery::file_identity(text(entry, "installation")?) {
            return Err(invalid("invalid entry identity"));
        }
        mesh_store::RecordDigest::parse_hex(text(entry, "metadata")?)
            .map_err(|e| invalid(&e.to_string()))?;
        let mode = entry
            .get("mode")
            .and_then(Json::as_u64)
            .ok_or_else(|| invalid("entry mode missing"))?;
        match text(entry, "kind")? {
            "directory"
                if mode & !0o040777 == 0
                    && mode & 0o040000 != 0
                    && entry.get("digest") == Some(&Json::Null)
                    && entry.get("bytes") == Some(&Json::Null) =>
            {
                directories.insert(path);
            }
            "file"
                if mode & !0o100777 == 0
                    && mode & 0o100000 != 0
                    && entry
                        .get("bytes")
                        .and_then(Json::as_u64)
                        .is_some_and(|bytes| bytes <= 64 * 1024 * 1024) =>
            {
                mesh_store::RecordDigest::parse_hex(text(entry, "digest")?)
                    .map_err(|e| invalid(&e.to_string()))?;
            }
            _ => return Err(invalid("unsupported entry evidence")),
        }
        previous = Some(path);
    }
    Ok(entries)
}
fn root_identity(value: &Json) -> io::Result<&Json> {
    tree(value)?
        .first()
        .and_then(|root| root.get("installation"))
        .ok_or_else(|| invalid("entry root identity missing"))
}

/// Revalidate an origin chain against actual trusted history. Reading never replays old operations.
#[allow(clippy::too_many_arguments)]
fn origin_evidence(
    history: &ProvisionedAttachment,
    store: &PinnedWorkspaceRoot,
    outer: &PinnedWorkspaceRoot,
    origin: &PinnedWorkspaceRoot,
    id: &str,
    proposal: &Json,
    workspace: &crate::workspace::OpenWorkspace,
    trusted: &TrustedReviewers,
    depth: usize,
) -> io::Result<Json> {
    if depth > 8 {
        return Err(invalid("entry restoration ancestry exceeds limit"));
    }
    private(origin)?;
    if super::directory_writeback::transaction(id) {
        super::directory_writeback::validate_receipt(proposal, history, store, origin)?;
        super::directory_writeback::verify_tree(proposal, workspace, trusted)?;
        return match proposal.get("schema").and_then(Json::as_text) {
            Some("mesh.attachment-directory-removal/v1") => {
                Ok(proposal.get("tree").unwrap().clone())
            }
            Some("mesh.attachment-entry-conversion/v1") => {
                Ok(proposal.get("before_tree").unwrap().clone())
            }
            _ => Err(invalid("origin has no displaced entry")),
        };
    }
    if !transaction(id)
        || !matches!(proposal, Json::Object(fields) if fields.len() == KEYS.len() && KEYS.iter().all(|key| proposal.get(key).is_some()))
        || proposal.get("schema") != Some(&Json::text(SCHEMA))
        || proposal.get("project") != Some(&Json::text(history.id()))
        || proposal.get("attachment") != Some(&history.project().receipt()?)
        || proposal.get("store_identity") != Some(&identity(store)?)
        || proposal.get("recovery_identity") != Some(&identity(origin)?)
        || proposal.get("automatic_replay") != Some(&Json::Bool(false))
    {
        return Err(invalid("entry restoration binding mismatch"));
    }
    crate::root_authority::ProtectedWorkspaceRoot::from_directory_token(text(
        proposal,
        "source_parent",
    )?)?;
    mesh_store::RecordDigest::parse_hex(text(proposal, "parent_metadata_digest")?)
        .map_err(|e| invalid(&e.to_string()))?;
    if !proposal
        .get("parent_mode")
        .and_then(Json::as_u64)
        .is_some_and(|mode| mode & !0o047777 == 0 && mode & 0o040000 != 0)
    {
        return Err(invalid("invalid restoration parent mode"));
    }
    let (configuration, _) = history.project().history_configuration(store, None)?;
    if Json::parse(&configuration)
        .map_err(|e| invalid(&e.to_string()))?
        .get("exclusions")
        != proposal.get("exclusions")
    {
        return Err(invalid("entry restoration history policy mismatch"));
    }
    let parent_id = text(proposal, "origin_transaction")?;
    if !transaction(parent_id) && !super::directory_writeback::transaction(parent_id) {
        return Err(invalid("invalid entry restoration origin"));
    }
    let parent = outer.open_child_directory(OsStr::new(parent_id))?;
    let (parent_receipt, _) = recovery::read_json(&parent, "prepared.json")?;
    if proposal.get("origin_identity") != Some(&identity(&parent)?)
        || text(proposal, "origin_proposal_digest")? != digest(&parent_receipt)
        || proposal.get("path") != parent_receipt.get("path")
    {
        return Err(invalid("entry restoration ancestry changed"));
    }
    let expected = origin_evidence(
        history,
        store,
        outer,
        &parent,
        parent_id,
        &parent_receipt,
        workspace,
        trusted,
        depth + 1,
    )?;
    let frozen = proposal
        .get("origin_tree")
        .ok_or_else(|| invalid("restoration origin tree missing"))?;
    if root_identity(frozen)? != root_identity(&expected)? {
        return Err(invalid("retained root allocation differs from origin"));
    }
    let installed = tree(
        proposal
            .get("installed_tree")
            .ok_or_else(|| invalid("installed tree missing"))?,
    )?;
    let frozen = tree(frozen)?;
    if installed.len() != frozen.len()
        || installed.iter().zip(frozen).any(|(a, b)| {
            ["path", "kind", "mode", "metadata", "digest", "bytes"]
                .iter()
                .any(|key| a.get(key) != b.get(key))
        })
    {
        return Err(invalid("restoration copy differs from frozen origin"));
    }
    let current = proposal
        .get("current_tree")
        .ok_or_else(|| invalid("restoration source missing"))?;
    if *current != Json::Null {
        tree(current)?;
    }
    Ok(current.clone())
}

/// Native-only single-use whole-entry authority; serialized receipts grant no write authority.
pub struct PreparedRetainedEntryRestoration {
    history: ProvisionedAttachment,
    store: PinnedWorkspaceRoot,
    outer: PinnedWorkspaceRoot,
    origin: PinnedWorkspaceRoot,
    origin_id: String,
    origin_receipt: Json,
    recovery: PinnedWorkspaceRoot,
    path: PathBuf,
    proposal: Json,
    operation: RetainedEntryRestoration,
    limits: ObservationLimits,
}
pub(super) fn prepare(
    history: &ProvisionedAttachment,
    store: PinnedWorkspaceRoot,
    recovery_root: &Path,
    id: &str,
    trusted: &TrustedReviewers,
    limits: ObservationLimits,
) -> io::Result<PreparedRetainedEntryRestoration> {
    limits.validate()?;
    if !transaction(id) && !super::directory_writeback::transaction(id) {
        return Err(invalid("invalid retained entry transaction"));
    }
    let outer = external_store(recovery_root, history.project())?;
    private(&outer)?;
    history.project().with_review_history(
        history.metadata_path(),
        store,
        trusted,
        |workspace, store| {
            let origin = outer.open_child_directory(OsStr::new(id))?;
            let (receipt, _) = recovery::read_json(&origin, "prepared.json")?;
            let expected = origin_evidence(
                history, store, &outer, &origin, id, &receipt, workspace, trusted, 1,
            )?;
            let relative = text(&receipt, "path")?;
            let capture = history.project().capture_inputs(limits)?;
            history
                .project()
                .history_configuration(store, Some(capture.exclusion_digest()))?;
            let mut random = [0u8; 16];
            File::open("/dev/urandom")?.read_exact(&mut random)?;
            let name = format!(
                "entry-restoration-{}",
                random
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            );
            let recovery = outer.create_child_directory(OsStr::new(&name))?;
            let operation = RetainedEntryRestoration::prepare(
                history.project().pinned.clone(),
                PathBuf::from(relative),
                origin.clone(),
                recovery.clone(),
                EntryLimits {
                    entries: limits.entries,
                    bytes: limits.bytes,
                    file_bytes: limits.file_bytes,
                },
                |original, current| {
                    if root_identity(original)? != root_identity(&expected)? {
                        return Err(invalid("origin no longer retains its displaced entry"));
                    }
                    admit(&capture, relative, original, current)
                },
            )?;
            let proposal = Json::object([
                ("schema", Json::text(SCHEMA)),
                ("project", Json::text(history.id())),
                ("attachment", history.project().receipt()?),
                ("store_identity", identity(store)?),
                ("recovery_identity", identity(&recovery)?),
                ("path", Json::text(relative)),
                ("origin_transaction", Json::text(id)),
                ("origin_proposal_digest", Json::text(digest(&receipt))),
                ("origin_identity", identity(&origin)?),
                ("origin_tree", operation.original.evidence.clone()),
                (
                    "current_tree",
                    operation
                        .current
                        .as_ref()
                        .map_or(Json::Null, |current| current.evidence.clone()),
                ),
                ("installed_tree", operation.staged.clone()),
                ("source_parent", Json::text(&operation.parent.2)),
                ("parent_metadata_digest", Json::text(&operation.parent.0)),
                ("parent_mode", Json::Number(operation.parent.1 as u64)),
                (
                    "exclusions",
                    Json::text(capture.exclusion_digest().to_string()),
                ),
                ("automatic_replay", Json::Bool(false)),
            ]);
            let encoded = proposal.encode();
            if encoded.len() > 65_536 {
                return Err(invalid("entry restoration receipt exceeds limit"));
            }
            recovery.filesystem().write_new_file(
                Path::new("prepared.json"),
                encoded.as_bytes(),
                fs::Permissions::from_mode(0o600),
            )?;
            operation.validate()?;
            Ok(PreparedRetainedEntryRestoration {
                history: history.clone(),
                store: store.clone(),
                outer: outer.clone(),
                origin,
                origin_id: id.to_owned(),
                origin_receipt: receipt,
                recovery,
                path: recovery_root.join(name),
                proposal,
                operation,
                limits,
            })
        },
    )
}
fn admit(
    capture: &super::CapturedProjectInput,
    root: &str,
    original: &Json,
    current: Option<&Json>,
) -> io::Result<()> {
    for evidence in std::iter::once(original).chain(current) {
        for entry in tree(evidence)? {
            let relative = text(entry, "path")?;
            let path = if relative.is_empty() {
                root.to_owned()
            } else {
                format!("{root}/{relative}")
            };
            if !capture.admits_file_path(&path)? {
                return Err(invalid("entry restoration includes excluded work"));
            }
        }
    }
    Ok(())
}
impl PreparedRetainedEntryRestoration {
    /// Exact frozen native facts for complete confirmation; never serialized authority.
    pub fn proposal(&self) -> &Json {
        &self.proposal
    }
    /// New recovery transaction retaining displaced work independently of the origin.
    pub fn recovery_path(&self) -> &Path {
        &self.path
    }
    /// Complete frozen files for confirmation, relative to the restored entry root.
    pub fn restored_files(&self) -> impl Iterator<Item = (&str, &[u8], bool)> {
        self.operation.original.files()
    }
    /// Complete frozen destination files which will be retained by this new transaction.
    pub fn current_files(&self) -> impl Iterator<Item = (&str, &[u8], bool)> {
        self.operation
            .current
            .iter()
            .flat_map(|entry| entry.files())
    }
    /// Invoke only after explicit native confirmation of both complete frozen trees.
    pub fn apply(self, trusted: &TrustedReviewers) -> io::Result<Json> {
        self.history.project().with_review_history(
            self.history.metadata_path(),
            self.store.clone(),
            trusted,
            |workspace, store| {
                private(&self.outer)?;
                private(&self.origin)?;
                private(&self.recovery)?;
                let (origin, _) = recovery::read_json(&self.origin, "prepared.json")?;
                if origin != self.origin_receipt
                    || recovery::read_json(&self.recovery, "prepared.json")?.0 != self.proposal
                {
                    return Err(invalid("entry restoration receipt changed"));
                }
                origin_evidence(
                    &self.history,
                    store,
                    &self.outer,
                    &self.origin,
                    &self.origin_id,
                    &origin,
                    workspace,
                    trusted,
                    1,
                )?;
                let capture = self.history.project().capture_inputs(self.limits)?;
                self.history
                    .project()
                    .history_configuration(store, Some(capture.exclusion_digest()))?;
                if self.proposal.get("exclusions")
                    != Some(&Json::text(capture.exclusion_digest().to_string()))
                {
                    return Err(invalid("entry restoration policy changed"));
                }
                admit(
                    &capture,
                    text(&self.proposal, "path")?,
                    &self.operation.original.evidence,
                    self.operation
                        .current
                        .as_ref()
                        .map(|current| &current.evidence),
                )?;
                let displaced = self.operation.current.is_some();
                let applied = self.operation.apply()?;
                let result = Json::object([
                    (
                        "schema",
                        Json::text("mesh.attachment-entry-restoration-result/v1"),
                    ),
                    ("proposal_digest", Json::text(digest(&self.proposal))),
                    (
                        "status",
                        Json::text(if applied {
                            "applied-observed"
                        } else {
                            "reconciliation-required"
                        }),
                    ),
                    ("displaced_entry_retained", Json::Bool(displaced)),
                    ("origin_entry_retained", Json::Bool(true)),
                    ("observation_final", Json::Bool(false)),
                ]);
                self.recovery.filesystem().write_new_file(
                    Path::new("observed.json"),
                    result.encode().as_bytes(),
                    fs::Permissions::from_mode(0o600),
                )?;
                Ok(result)
            },
        )
    }
}

/// Independent current observations, not an atomic tree snapshot or write capability.
pub(super) fn inspect(
    history: &ProvisionedAttachment,
    store: PinnedWorkspaceRoot,
    recovery_root: &Path,
    id: &str,
    trusted: &TrustedReviewers,
    limits: ObservationLimits,
) -> io::Result<Json> {
    use crate::managed_file::retained_replacement::{absent_parent, observe_entry, parent_policy};
    limits.validate()?;
    if !transaction(id) {
        return Err(invalid("invalid entry restoration transaction"));
    }
    let outer = external_store(recovery_root, history.project())?;
    private(&outer)?;
    let selected = outer.open_child_directory(OsStr::new(id))?;
    let (proposal, _) = recovery::read_json(&selected, "prepared.json")?;
    history.project().with_review_history(
        history.metadata_path(),
        store.clone(),
        trusted,
        |workspace, store| {
            origin_evidence(
                history, store, &outer, &selected, id, &proposal, workspace, trusted, 0,
            )
            .map(|_| ())
        },
    )?;
    let original =
        outer.open_child_directory(OsStr::new(text(&proposal, "origin_transaction")?))?;
    let original_receipt = recovery::read_json(&original, "prepared.json")?.0;
    if digest(&original_receipt) != text(&proposal, "origin_proposal_digest")? {
        return Err(invalid("entry restoration origin changed"));
    }
    let relative = Path::new(text(&proposal, "path")?);
    let before_parent = parent_policy(&history.project().pinned, relative).ok();
    let mut remaining = limits.bytes.min(64 * 1024 * 1024);
    let mut observe = |root: &PinnedWorkspaceRoot, path: &Path| -> Json {
        match absent_parent(root, path) {
            Ok(Some(_)) => {
                return Json::object([("state", Json::text("absent")), ("tree", Json::Null)])
            }
            Ok(None) => {}
            Err(_) => {
                return Json::object([("state", Json::text("unavailable")), ("tree", Json::Null)])
            }
        }
        match observe_entry(
            root,
            path,
            EntryLimits {
                entries: limits.entries,
                bytes: remaining,
                file_bytes: limits.file_bytes,
            },
        ) {
            Ok(evidence) => {
                let spent: u64 = evidence
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|entry| entry.get("bytes").and_then(Json::as_u64))
                    .sum();
                remaining = remaining.saturating_sub(spent);
                Json::object([("state", Json::text("observed")), ("tree", evidence)])
            }
            Err(_) => {
                remaining = 0;
                Json::object([("state", Json::text("unavailable")), ("tree", Json::Null)])
            }
        }
    };
    let source = observe(&history.project().pinned, relative);
    let stage = observe(&selected, Path::new("exchange"));
    let origin = observe(&original, Path::new("exchange"));
    let after_parent = parent_policy(&history.project().pinned, relative).ok();
    let matches = |observation: &Json, expected: &Json| {
        if *expected == Json::Null {
            observation.get("state") == Some(&Json::text("absent"))
        } else {
            observation.get("state") == Some(&Json::text("observed"))
                && observation.get("tree") == Some(expected)
        }
    };
    let stable = before_parent
        .as_ref()
        .filter(|before| Some(*before) == after_parent.as_ref());
    let parent_identity_matches = stable.map(|(_, _, identity)| {
        Some(identity.as_str()) == proposal.get("source_parent").and_then(Json::as_text)
    });
    let parent_policy_matches = stable.map(|(policy, mode, _)| {
        Some(policy.as_str())
            == proposal
                .get("parent_metadata_digest")
                .and_then(Json::as_text)
            && Some(*mode as u64) == proposal.get("parent_mode").and_then(Json::as_u64)
    });
    let mut status = if [&source, &stage, &origin]
        .iter()
        .any(|item| item.get("state") == Some(&Json::text("unavailable")))
    {
        "incomplete-observation"
    } else if !matches(&origin, proposal.get("origin_tree").unwrap()) {
        "origin-changed"
    } else if matches(&source, proposal.get("current_tree").unwrap())
        && matches(&stage, proposal.get("installed_tree").unwrap())
    {
        "prepared-arrangement"
    } else if matches(&source, proposal.get("installed_tree").unwrap())
        && matches(&stage, proposal.get("current_tree").unwrap())
    {
        "applied-arrangement"
    } else {
        "changed-entries"
    };
    if parent_identity_matches == Some(false) {
        status = "source-parent-changed";
    } else if parent_policy_matches == Some(false) {
        status = "parent-policy-changed";
    } else if stable.is_none() {
        status = "incomplete-observation";
    }
    let recorded = match recovery::read_json(&selected, "observed.json") {
        Err(error) if error.kind() == io::ErrorKind::NotFound => "absent",
        Err(_) => "invalid",
        Ok((result, _)) => {
            let reported = result.get("status").and_then(Json::as_text).unwrap_or("");
            let expected = Json::object([
                (
                    "schema",
                    Json::text("mesh.attachment-entry-restoration-result/v1"),
                ),
                ("proposal_digest", Json::text(digest(&proposal))),
                ("status", Json::text(reported)),
                (
                    "displaced_entry_retained",
                    Json::Bool(proposal.get("current_tree") != Some(&Json::Null)),
                ),
                ("origin_entry_retained", Json::Bool(true)),
                ("observation_final", Json::Bool(false)),
            ]);
            if result != expected {
                "invalid"
            } else {
                match reported {
                    "applied-observed" => "applied-observed",
                    "reconciliation-required" => "reconciliation-required",
                    _ => "invalid",
                }
            }
        }
    };
    if recorded == "invalid" {
        status = "invalid-outcome";
    } else if recorded == "applied-observed" && status == "prepared-arrangement" {
        status = "contradictory-outcome";
    }
    private(&outer)?;
    private(&selected)?;
    private(&original)?;
    history.project().ensure_current()?;
    if recovery::read_json(&selected, "prepared.json")?.0 != proposal
        || recovery::read_json(&original, "prepared.json")?.0 != original_receipt
    {
        return Err(invalid(
            "entry restoration evidence changed during inspection",
        ));
    }
    Ok(Json::object([
        (
            "schema",
            Json::text("mesh.attachment-entry-restoration-recovery/v1"),
        ),
        ("project", Json::text(history.id())),
        ("transaction", Json::text(id)),
        ("path", proposal.get("path").unwrap().clone()),
        ("status", Json::text(status)),
        ("recorded_outcome", Json::text(recorded)),
        (
            "parent_identity_matches",
            parent_identity_matches.map_or(Json::Null, Json::Bool),
        ),
        (
            "parent_policy_matches",
            parent_policy_matches.map_or(Json::Null, Json::Bool),
        ),
        ("live_content_budget_remaining", Json::Number(remaining)),
        ("source", source),
        ("stage", stage),
        ("origin", origin),
        ("observation_final", Json::Bool(false)),
        ("automatic_replay", Json::Bool(false)),
        ("write_authority", Json::Bool(false)),
        ("cleanup_authority", Json::Bool(false)),
    ]))
}
