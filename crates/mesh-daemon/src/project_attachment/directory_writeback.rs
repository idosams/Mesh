//! Native-only complete approved directory changes and entry conversion with retained recovery.
use super::{external_store, invalid, ObservationLimits, ProvisionedAttachment};
use crate::managed_file::retained_replacement::{
    absent_parent, observe_entry, observe_tree, open_tree, parent_policy, ConversionInput,
    EntryLimits, RetainedConversion, RetainedTreeAddition, RetainedTreeRemoval, TreeInput,
};
use crate::{
    ipc::Json, root_authority::PinnedWorkspaceRoot, workspace::OpenWorkspace, TrustedReviewers,
};
use mesh_types::{Blake3, ContentDigest as _};
use std::os::unix::fs::PermissionsExt as _;
use std::{
    ffi::OsStr,
    fs::{self, File},
    io::{self, Read as _},
    path::{Path, PathBuf},
};

/// Prepared native authority, consumed once after complete explicit confirmation. Preparation only
/// writes private staging; neither an agent nor a renderer can construct this value from a receipt.
pub struct PreparedMainDirectoryChange {
    history: ProvisionedAttachment,
    store: PinnedWorkspaceRoot,
    recovery: PinnedWorkspaceRoot,
    path: PathBuf,
    proposal: Json,
    operation: DirectoryOperation,
    inputs: Vec<TreeInput>,
    before_inputs: Vec<TreeInput>,
}
/// Compatibility name for callers that prepare only additions.
pub type PreparedMainDirectoryAddition = PreparedMainDirectoryChange;

enum DirectoryOperation {
    Add(RetainedTreeAddition),
    Remove(RetainedTreeRemoval),
    Convert(RetainedConversion),
}
impl DirectoryOperation {
    fn validate(&self) -> io::Result<()> {
        match self {
            Self::Add(op) => op.validate(),
            Self::Remove(op) => op.validate(),
            Self::Convert(op) => op.validate(),
        }
    }
    fn apply(self) -> io::Result<bool> {
        match self {
            Self::Add(op) => op.apply(),
            Self::Remove(op) => op.apply(),
            Self::Convert(op) => op.apply(),
        }
    }
}
fn removal(proposal: &Json) -> bool {
    proposal.get("schema") == Some(&Json::text("mesh.attachment-directory-removal/v1"))
}
fn conversion(proposal: &Json) -> bool {
    proposal.get("schema") == Some(&Json::text("mesh.attachment-entry-conversion/v1"))
}
fn result_schema(proposal: &Json) -> &'static str {
    if conversion(proposal) {
        "mesh.attachment-entry-conversion-result/v1"
    } else if removal(proposal) {
        "mesh.attachment-directory-removal-result/v1"
    } else {
        "mesh.attachment-directory-addition-result/v1"
    }
}

fn digest(value: &Json) -> String {
    Blake3::digest_bytes(value.encode().as_bytes()).to_string()
}
fn err(value: impl std::fmt::Display) -> io::Error {
    io::Error::other(value.to_string())
}
fn text<'a>(value: &'a Json, key: &str) -> io::Result<&'a str> {
    super::recovery::text(value, key)
}
fn selected(path: &str, root: &str) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|tail| tail.starts_with('/'))
}
fn canonical(path: &str) -> bool {
    !path.is_empty()
        && !path.split('/').any(|part| {
            part.is_empty() || part == "." || part == ".." || part.eq_ignore_ascii_case(".git")
        })
}

pub(super) fn verify_tree(
    proposal: &Json,
    workspace: &OpenWorkspace,
    trusted: &TrustedReviewers,
) -> io::Result<()> {
    let root = text(proposal, "path")?;
    if !canonical(root) {
        return Err(invalid("invalid approved directory path"));
    }
    let (before, after) = super::recovery::verified_trees(proposal, workspace, trusted)?;
    if conversion(proposal) {
        if !matches!(
            (
                before.get(root).map(|e| e.kind),
                after.get(root).map(|e| e.kind)
            ),
            (Some("file"), Some("folder")) | (Some("folder"), Some("file"))
        ) {
            return Err(invalid(
                "approved conversion does not change file/directory type",
            ));
        }
        verify_evidence(
            proposal
                .get("before_tree")
                .ok_or_else(|| invalid("conversion source evidence missing"))?,
            root,
            &before,
            true,
        )?;
        return verify_evidence(
            proposal
                .get("tree")
                .ok_or_else(|| invalid("conversion target evidence missing"))?,
            root,
            &after,
            false,
        );
    }
    let removed = removal(proposal);
    let (original, result) = if removed {
        (&after, &before)
    } else {
        (&before, &after)
    };
    if original.keys().any(|path| selected(path, root))
        || !result.get(root).is_some_and(|entry| entry.kind == "folder")
    {
        return Err(invalid(
            "approved directory change does not cover a whole subtree",
        ));
    }
    verify_evidence(
        proposal
            .get("tree")
            .ok_or_else(|| invalid("directory evidence missing"))?,
        root,
        result,
        removed,
    )
}
fn verify_evidence(
    value: &Json,
    root: &str,
    result: &std::collections::BTreeMap<String, super::inspection::ComparisonEntry>,
    removed: bool,
) -> io::Result<()> {
    let tree = value
        .as_array()
        .ok_or_else(|| invalid("invalid tree evidence"))?;
    let expected: Vec<_> = result
        .iter()
        .filter(|(path, _)| selected(path, root))
        .collect();
    if tree.is_empty() || tree.len() > 64 || tree.len() != expected.len() {
        return Err(invalid("directory tree coverage mismatch"));
    }
    for (observed, (path, entry)) in tree.iter().zip(expected) {
        let relative = if path == root {
            ""
        } else {
            &path[root.len() + 1..]
        };
        let mode = observed
            .get("mode")
            .and_then(Json::as_u64)
            .ok_or_else(|| invalid("directory tree mode unavailable"))?;
        if observed.get("path") != Some(&Json::text(relative))
            || observed.get("kind")
                != Some(&Json::text(if entry.kind == "folder" {
                    "directory"
                } else {
                    "file"
                }))
            || observed.get("digest")
                != Some(
                    &entry
                        .digest
                        .map_or(Json::Null, |d| Json::text(d.to_string())),
                )
            || observed.get("bytes") != Some(&entry.bytes.map_or(Json::Null, Json::Number))
            || if entry.kind == "folder" {
                mode & !0o042777 != 0 || mode & 0o040000 == 0
            } else {
                mode & !(if removed { 0o100777 } else { 0o100755 }) != 0
                    || mode & 0o100000 == 0
                    || Some(mode & 0o111 != 0) != entry.executable
            }
        {
            return Err(invalid("directory evidence differs from approved tree"));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare(
    history: ProvisionedAttachment,
    store: PinnedWorkspaceRoot,
    bundle: &str,
    target: &str,
    relative: &str,
    recovery_root: &Path,
    trusted: &TrustedReviewers,
    limits: ObservationLimits,
    expected_kind: &str,
) -> io::Result<PreparedMainDirectoryChange> {
    limits.validate()?;
    if !canonical(relative) {
        return Err(invalid("expected a canonical new directory path"));
    }
    history.project().with_review_history(
        history.metadata_path(),
        store.clone(),
        trusted,
        |workspace, store| {
            let capture = history.project().capture_inputs(limits)?;
            prepare_captured(
                &history,
                workspace,
                store,
                bundle,
                target,
                relative,
                recovery_root,
                trusted,
                limits,
                &capture,
                Some(expected_kind),
            )
        },
    )
}
/// Caller retains verified history; all members can share one complete captured input.
#[allow(clippy::too_many_arguments)]
pub(super) fn prepare_captured(
    history: &ProvisionedAttachment,
    workspace: &OpenWorkspace,
    store: &PinnedWorkspaceRoot,
    bundle: &str,
    target: &str,
    relative: &str,
    recovery_root: &Path,
    trusted: &TrustedReviewers,
    limits: ObservationLimits,
    capture: &super::CapturedProjectInput,
    expected_kind: Option<&str>,
) -> io::Result<PreparedMainDirectoryChange> {
    limits.validate()?;
    if !canonical(relative)
        || capture.root() != history.project().root()
        || capture.identity() != (history.project().device, history.project().inode)
        || history.project().current_exclusion_digest()? != capture.exclusion_digest()
    {
        return Err(invalid("directory capture binding changed"));
    }
    let accepted = workspace
        .accepted_main_review()
        .map_err(err)?
        .ok_or_else(|| invalid("accepted review unavailable"))?;
    if accepted.bundle.to_string() != bundle || accepted.subject_operation.to_string() != target {
        return Err(invalid("accepted main changed"));
    }
    let head = super::approval::main_head(workspace)?
        .ok_or_else(|| invalid("accepted main unavailable"))?
        .to_string();
    history
        .project()
        .history_configuration(store, Some(capture.exclusion_digest()))?;
    let binding = Json::object([
        ("head", Json::text(&head)),
        ("bundle", Json::text(bundle)),
        ("target", Json::text(target)),
    ]);
    let (before, after) = super::recovery::verified_trees(&binding, workspace, trusted)?;
    let removed = before
        .get(relative)
        .is_some_and(|entry| entry.kind == "folder")
        && !after.keys().any(|path| selected(path, relative));
    let added = after
        .get(relative)
        .is_some_and(|entry| entry.kind == "folder")
        && !before.keys().any(|path| selected(path, relative));
    let converted = matches!(
        (
            before.get(relative).map(|e| e.kind),
            after.get(relative).map(|e| e.kind)
        ),
        (Some("file"), Some("folder")) | (Some("folder"), Some("file"))
    );
    let actual_kind = if converted {
        "conversion"
    } else if removed {
        "removal"
    } else {
        "addition"
    };
    if (!removed && !added && !converted)
        || expected_kind.is_some_and(|expected| expected != actual_kind)
    {
        return Err(invalid(
            "approved directory operation kind differs from request",
        ));
    }
    let base_target = if removed || converted {
        let base = workspace
            .human_approval_context(&accepted)
            .map_err(err)?
            .expected_canonical_head();
        Some(workspace.review_target_for_head(base).map_err(err)?)
    } else {
        None
    };
    let mut total = 0u64;
    let mut load =
        |tree: &std::collections::BTreeMap<String, super::inspection::ComparisonEntry>,
         operation|
         -> io::Result<Vec<TreeInput>> {
            let entries: Vec<_> = tree
                .iter()
                .filter(|(path, _)| selected(path, relative))
                .collect();
            if entries.len() > 64.min(limits.entries) {
                return Err(invalid("approved tree exceeds entry budget"));
            }
            let mut inputs = Vec::new();
            for (path, entry) in entries {
                if !capture.admits_file_path(path)? {
                    return Err(invalid("approved tree is excluded by current policy"));
                }
                if path == relative && entry.kind == "folder" {
                    continue;
                }
                let name = if path == relative {
                    String::new()
                } else {
                    path[relative.len() + 1..].to_owned()
                };
                if entry.kind == "folder" {
                    inputs.push(TreeInput::Directory(name));
                } else {
                    let size = entry
                        .bytes
                        .ok_or_else(|| invalid("approved file size unavailable"))?;
                    total = total
                        .checked_add(size)
                        .ok_or_else(|| invalid("tree byte budget overflow"))?;
                    if size > limits.file_bytes || total > limits.bytes.min(64 * 1024 * 1024) {
                        return Err(invalid("approved trees exceed byte budget"));
                    }
                    let file = workspace
                        .historical_workspace_file(operation, path)
                        .map_err(err)?
                        .ok_or_else(|| invalid("approved file unavailable"))?;
                    inputs.push(TreeInput::File(name, file.bytes, file.executable));
                }
            }
            Ok(inputs)
        };
    let before_inputs = if converted {
        load(&before, base_target.unwrap())?
    } else {
        vec![]
    };
    let inputs = if removed {
        load(&before, base_target.unwrap())?
    } else {
        load(&after, accepted.subject_operation)?
    };
    let outer = external_store(recovery_root, history.project())?;
    if outer.identity()?.0 != history.project().device
        || outer
            .try_clone_directory()?
            .metadata()?
            .permissions()
            .mode()
            & 0o077
            != 0
    {
        return Err(invalid(
            "directory recovery must be private on the source filesystem",
        ));
    }
    let mut nonce = [0u8; 16];
    File::open("/dev/urandom")?.read_exact(&mut nonce)?;
    let name = format!(
        "directory-{}",
        nonce.iter().map(|b| format!("{b:02x}")).collect::<String>()
    );
    let recovery = outer.create_child_directory(OsStr::new(&name))?;
    let mut proposal = Json::Null;
    let mut record =
        |before_tree: Option<&Json>, tree: &Json, parent: &str, policy: &str, mode: u32| {
            proposal = Json::object([
                (
                    "schema",
                    Json::text(if converted {
                        "mesh.attachment-entry-conversion/v1"
                    } else if removed {
                        "mesh.attachment-directory-removal/v1"
                    } else {
                        "mesh.attachment-directory-addition/v1"
                    }),
                ),
                ("project", Json::text(history.id())),
                ("attachment", history.project().receipt()?),
                ("head", Json::text(&head)),
                ("bundle", Json::text(bundle)),
                ("target", Json::text(target)),
                ("path", Json::text(relative)),
                ("source_parent", Json::text(parent)),
                ("parent_metadata_digest", Json::text(policy)),
                ("parent_mode", Json::Number(mode as u64)),
                ("store_identity", identity(store)?),
                ("recovery_identity", identity(&recovery)?),
                (
                    "exclusions",
                    Json::text(capture.exclusion_digest().to_string()),
                ),
                ("tree", tree.clone()),
                ("automatic_replay", Json::Bool(false)),
            ]);
            if let Some(before_tree) = before_tree {
                let Json::Object(fields) = &mut proposal else {
                    unreachable!()
                };
                fields.push(("before_tree".into(), before_tree.clone()));
            }
            verify_tree(&proposal, workspace, trusted)?;
            if proposal.encode().len() > 65_536 {
                return Err(invalid("directory receipt exceeds limit"));
            }
            recovery.filesystem().write_new_file(
                Path::new("prepared.json"),
                proposal.encode().as_bytes(),
                fs::Permissions::from_mode(0o600),
            )
        };
    let operation = if converted {
        let input = if after
            .get(relative)
            .is_some_and(|entry| entry.kind == "file")
        {
            let [TreeInput::File(path, bytes, executable)] = inputs.as_slice() else {
                return Err(invalid("conversion file inputs unavailable"));
            };
            if !path.is_empty() {
                return Err(invalid("conversion root file unavailable"));
            }
            ConversionInput::File(bytes, *executable)
        } else {
            ConversionInput::Directory(&inputs)
        };
        DirectoryOperation::Convert(RetainedConversion::prepare(
            history.project().pinned.clone(),
            PathBuf::from(relative),
            recovery.clone(),
            input,
            EntryLimits {
                entries: limits.entries,
                bytes: limits.bytes,
                file_bytes: limits.file_bytes,
            },
            |before, after, parent, policy, mode| record(Some(before), after, parent, policy, mode),
        )?)
    } else if removed {
        DirectoryOperation::Remove(RetainedTreeRemoval::prepare(
            history.project().pinned.clone(),
            PathBuf::from(relative),
            recovery.clone(),
            limits.entries,
            limits.bytes,
            limits.file_bytes,
            |tree, parent, policy, mode| record(None, tree, parent, policy, mode),
        )?)
    } else {
        DirectoryOperation::Add(RetainedTreeAddition::prepare(
            history.project().pinned.clone(),
            PathBuf::from(relative),
            &inputs,
            recovery.clone(),
            |tree, parent, policy, mode| record(None, tree, parent, policy, mode),
        )?)
    };
    Ok(PreparedMainDirectoryChange {
        history: history.clone(),
        store: store.clone(),
        recovery,
        path: recovery_root.join(name),
        proposal,
        operation,
        inputs,
        before_inputs,
    })
}

fn identity(root: &PinnedWorkspaceRoot) -> io::Result<Json> {
    let (dev, ino) = root.identity()?;
    Ok(Json::text(format!("{dev:016x}:{ino:016x}")))
}
impl PreparedMainDirectoryChange {
    /// Complete immutable tree evidence. It supplies no replay authority.
    pub fn proposal(&self) -> &Json {
        &self.proposal
    }
    /// Frozen approved file bytes for complete native confirmation; paths are relative to the new root.
    pub fn proposed_files(&self) -> impl Iterator<Item = (&str, &[u8], bool)> {
        self.confirmation_files()
            .filter(|_| !removal(&self.proposal))
    }
    /// Frozen source content for removal, or replacement content for addition/conversion.
    pub fn confirmation_files(&self) -> impl Iterator<Item = (&str, &[u8], bool)> {
        self.inputs.iter().filter_map(|input| match input {
            TreeInput::File(path, bytes, executable) => {
                Some((path.as_str(), bytes.as_slice(), *executable))
            }
            TreeInput::Directory(_) => None,
        })
    }
    /// Frozen original file content for conversion confirmation; never a mutable source reread.
    pub fn current_files(&self) -> impl Iterator<Item = (&str, &[u8], bool)> {
        self.before_inputs.iter().filter_map(|input| match input {
            TreeInput::File(path, bytes, executable) => {
                Some((path.as_str(), bytes.as_slice(), *executable))
            }
            TreeInput::Directory(_) => None,
        })
    }
    /// Retained staging and durable outcome location, owned by the native host.
    pub fn recovery_path(&self) -> &Path {
        &self.path
    }
    /// Explicit native invocation after confirmation of the complete tree. Never retry an uncertain
    /// result automatically; a concurrent destination is preserved and all staging is retained.
    pub fn apply(self, trusted: &TrustedReviewers) -> io::Result<Json> {
        let history = self.history.clone();
        history.project().with_review_history(
            history.metadata_path(),
            self.store.clone(),
            trusted,
            |workspace, store| {
                self.validate(workspace, store, trusted)?;
                self.apply_validated()
            },
        )
    }
    pub(super) fn validate(
        &self,
        workspace: &OpenWorkspace,
        store: &PinnedWorkspaceRoot,
        trusted: &TrustedReviewers,
    ) -> io::Result<()> {
        let current = workspace
            .accepted_main_review()
            .map_err(err)?
            .ok_or_else(|| invalid("accepted main unavailable"))?;
        if current.bundle.to_string() != text(&self.proposal, "bundle")?
            || current.subject_operation.to_string() != text(&self.proposal, "target")?
            || super::approval::main_head(workspace)?
                .map(|head| head.to_string())
                .as_deref()
                != Some(text(&self.proposal, "head")?)
        {
            return Err(invalid("accepted directory main changed"));
        }
        let exclusions = self.history.project().current_exclusion_digest()?;
        self.history
            .project()
            .history_configuration(store, Some(exclusions))?;
        if exclusions.to_string() != text(&self.proposal, "exclusions")? {
            return Err(invalid("directory exclusion policy changed"));
        }
        verify_tree(&self.proposal, workspace, trusted)?;
        self.recovery.ensure_namespace_identity()?;
        if super::recovery::read_json(&self.recovery, "prepared.json")?.0 != self.proposal {
            return Err(invalid("directory receipt changed"));
        }
        self.operation.validate()
    }
    pub(super) fn apply_validated(self) -> io::Result<Json> {
        let observed = self.operation.apply()?;
        let result = Json::object([
            ("schema", Json::text(result_schema(&self.proposal))),
            ("proposal_digest", Json::text(digest(&self.proposal))),
            (
                "status",
                Json::text(if observed {
                    "applied-observed"
                } else {
                    "reconciliation-required"
                }),
            ),
            (
                "displaced_entry_retained",
                Json::Bool(removal(&self.proposal) || conversion(&self.proposal)),
            ),
            ("observation_final", Json::Bool(false)),
        ]);
        self.recovery.filesystem().write_new_file(
            Path::new("observed.json"),
            result.encode().as_bytes(),
            fs::Permissions::from_mode(0o600),
        )?;
        Ok(result)
    }
}

pub(super) fn transaction(value: &str) -> bool {
    value.strip_prefix("directory-").is_some_and(|id| {
        id.len() == 32
            && id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
pub(super) fn validate_receipt(
    value: &Json,
    history: &ProvisionedAttachment,
    store: &PinnedWorkspaceRoot,
    recovery: &PinnedWorkspaceRoot,
) -> io::Result<()> {
    let mut fields = vec![
        "schema",
        "project",
        "attachment",
        "head",
        "bundle",
        "target",
        "path",
        "source_parent",
        "parent_metadata_digest",
        "parent_mode",
        "store_identity",
        "recovery_identity",
        "exclusions",
        "tree",
        "automatic_replay",
    ];
    if conversion(value) {
        fields.push("before_tree");
    }
    if !matches!(value, Json::Object(pairs) if pairs.len() == fields.len() && fields.iter().all(|key| value.get(key).is_some()))
        || !matches!(
            value.get("schema").and_then(Json::as_text),
            Some(
                "mesh.attachment-directory-addition/v1"
                    | "mesh.attachment-directory-removal/v1"
                    | "mesh.attachment-entry-conversion/v1"
            )
        )
        || value.get("project") != Some(&Json::text(history.id()))
        || value.get("attachment") != Some(&history.project().receipt()?)
        || value.get("store_identity") != Some(&identity(store)?)
        || value.get("recovery_identity") != Some(&identity(recovery)?)
        || value.get("automatic_replay") != Some(&Json::Bool(false))
    {
        return Err(invalid("directory recovery binding mismatch"));
    }
    crate::root_authority::ProtectedWorkspaceRoot::from_directory_token(text(
        value,
        "source_parent",
    )?)?;
    mesh_store::RecordDigest::parse_hex(text(value, "parent_metadata_digest")?).map_err(err)?;
    let mode = value
        .get("parent_mode")
        .and_then(Json::as_u64)
        .ok_or_else(|| invalid("parent mode unavailable"))?;
    if mode & !0o047777 != 0 || mode & 0o040000 == 0 {
        return Err(invalid("invalid directory parent mode"));
    }
    let (configuration, _) = history.project().history_configuration(store, None)?;
    if Json::parse(&configuration).map_err(err)?.get("exclusions") != value.get("exclusions") {
        return Err(invalid("directory history policy mismatch"));
    }
    let entries = value
        .get("tree")
        .and_then(Json::as_array)
        .ok_or_else(|| invalid("invalid directory evidence"))?;
    let before_entries = if conversion(value) {
        value
            .get("before_tree")
            .and_then(Json::as_array)
            .ok_or_else(|| invalid("conversion original evidence missing"))?
    } else {
        &[]
    };
    for entry in entries.iter().chain(before_entries) {
        let keys = [
            "path",
            "kind",
            "installation",
            "mode",
            "metadata",
            "digest",
            "bytes",
        ];
        if !matches!(entry, Json::Object(pairs) if pairs.len() == keys.len() && keys.iter().all(|key| entry.get(key).is_some()))
            || !super::recovery::file_identity(text(entry, "installation")?)
        {
            return Err(invalid("invalid directory entry evidence"));
        }
        mesh_store::RecordDigest::parse_hex(text(entry, "metadata")?).map_err(err)?;
    }
    Ok(())
}

/// Read exact historical binding and current source/stage evidence without applying or cleaning up.
pub(super) fn inspect(
    history: &ProvisionedAttachment,
    store: PinnedWorkspaceRoot,
    recovery_root: &Path,
    id: &str,
    trusted: &TrustedReviewers,
    limits: ObservationLimits,
) -> io::Result<Json> {
    limits.validate()?;
    if !transaction(id) {
        return Err(invalid("invalid directory transaction"));
    }
    let outer = external_store(recovery_root, history.project())?;
    let recovery = outer.open_child_directory(OsStr::new(id))?;
    if outer
        .try_clone_directory()?
        .metadata()?
        .permissions()
        .mode()
        & 0o077
        != 0
        || recovery
            .try_clone_directory()?
            .metadata()?
            .permissions()
            .mode()
            & 0o077
            != 0
    {
        return Err(invalid("directory recovery is not private"));
    }
    let (proposal, _) = super::recovery::read_json(&recovery, "prepared.json")?;
    validate_receipt(&proposal, history, &store, &recovery)?;
    history.project().with_review_history(
        history.metadata_path(),
        store.clone(),
        trusted,
        |workspace, _| verify_tree(&proposal, workspace, trusted),
    )?;
    let relative = Path::new(text(&proposal, "path")?);
    let parent_before = parent_policy(&history.project().pinned, relative).ok();
    let mut remaining = limits.bytes;
    let mut observe = |root: &PinnedWorkspaceRoot,
                       path: &Path,
                       expected_parent: Option<&str>|
     -> Json {
        match absent_parent(root, path) {
            Ok(Some(parent)) if expected_parent.is_none_or(|expected| parent == expected) => {
                return Json::object([("state", Json::text("absent")), ("tree", Json::Null)])
            }
            Ok(None) => {}
            _ => return Json::object([("state", Json::text("unavailable")), ("tree", Json::Null)]),
        }
        let observed = if conversion(&proposal) {
            observe_entry(
                root,
                path,
                EntryLimits {
                    entries: limits.entries,
                    bytes: remaining,
                    file_bytes: limits.file_bytes,
                },
            )
        } else {
            open_tree(root, path).and_then(|root| {
                observe_tree(&root, limits.entries.min(64), remaining, limits.file_bytes)
            })
        };
        match observed {
            Ok(tree) => {
                let spent = tree
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|entry| entry.get("bytes").and_then(Json::as_u64))
                    .sum::<u64>();
                remaining = remaining.saturating_sub(spent);
                Json::object([("state", Json::text("observed")), ("tree", tree)])
            }
            Err(_) => {
                // A failed bounded walk can have consumed its entire allowance.
                remaining = 0;
                Json::object([("state", Json::text("unavailable")), ("tree", Json::Null)])
            }
        }
    };
    let source = observe(
        &history.project().pinned,
        relative,
        Some(text(&proposal, "source_parent")?),
    );
    let stage = observe(&recovery, Path::new("exchange"), None);
    let parent_after = parent_policy(&history.project().pinned, relative).ok();
    let stable_parent = parent_before
        .as_ref()
        .filter(|before| Some(*before) == parent_after.as_ref());
    let parent_identity_matches = stable_parent.map(|(_, _, identity)| {
        Some(identity.as_str()) == proposal.get("source_parent").and_then(Json::as_text)
    });
    let parent_policy_matches = stable_parent.map(|(policy, mode, _)| {
        Some(policy.as_str())
            == proposal
                .get("parent_metadata_digest")
                .and_then(Json::as_text)
            && Some(*mode as u64) == proposal.get("parent_mode").and_then(Json::as_u64)
    });
    let state = |value: &Json, wanted| value.get("state") == Some(&Json::text(wanted));
    let removed = removal(&proposal);
    let (installed, private) = if removed {
        (&stage, &source)
    } else {
        (&source, &stage)
    };
    let mut status = if conversion(&proposal) {
        if source.get("tree") == proposal.get("before_tree")
            && stage.get("tree") == proposal.get("tree")
        {
            "prepared-arrangement"
        } else if source.get("tree") == proposal.get("tree")
            && stage.get("tree") == proposal.get("before_tree")
        {
            "applied-arrangement"
        } else if state(&source, "unavailable") || state(&stage, "unavailable") {
            "incomplete-observation"
        } else {
            "changed-entries"
        }
    } else if state(installed, "absent") && private.get("tree") == proposal.get("tree") {
        "prepared-arrangement"
    } else if state(private, "absent") && installed.get("tree") == proposal.get("tree") {
        "applied-arrangement"
    } else if state(&source, "unavailable") || state(&stage, "unavailable") {
        "incomplete-observation"
    } else {
        "changed-entries"
    };
    if parent_identity_matches == Some(false) {
        status = "source-parent-changed";
    } else if parent_policy_matches == Some(false) {
        status = "parent-policy-changed";
    } else if stable_parent.is_none() {
        status = "incomplete-observation";
    }
    let recorded = match super::recovery::read_json(&recovery, "observed.json") {
        Err(error) if error.kind() == io::ErrorKind::NotFound => "absent",
        Err(_) => "invalid",
        Ok((value, _)) => {
            let reported = value.get("status").and_then(Json::as_text).unwrap_or("");
            let expected = Json::object([
                ("schema", Json::text(result_schema(&proposal))),
                ("proposal_digest", Json::text(digest(&proposal))),
                ("status", Json::text(reported)),
                (
                    "displaced_entry_retained",
                    Json::Bool(removed || conversion(&proposal)),
                ),
                ("observation_final", Json::Bool(false)),
            ]);
            if value != expected {
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
    outer.ensure_namespace_identity()?;
    recovery.ensure_namespace_identity()?;
    history.project().ensure_current()?;
    if super::recovery::read_json(&recovery, "prepared.json")?.0 != proposal {
        return Err(invalid("directory receipt changed during inspection"));
    }
    let mut result = Json::object([
        (
            "schema",
            Json::text(if conversion(&proposal) {
                "mesh.attachment-entry-conversion-recovery/v1"
            } else if removed {
                "mesh.attachment-directory-removal-recovery/v1"
            } else {
                "mesh.attachment-directory-addition-recovery/v1"
            }),
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
        ("observation_final", Json::Bool(false)),
        ("automatic_replay", Json::Bool(false)),
        ("write_authority", Json::Bool(false)),
        ("cleanup_authority", Json::Bool(false)),
    ]);
    if conversion(&proposal) {
        let Json::Object(fields) = &mut result else {
            unreachable!()
        };
        for (key, tree) in [("before_kind", "before_tree"), ("after_kind", "tree")] {
            fields.push((
                key.into(),
                proposal.get(tree).and_then(Json::as_array).unwrap()[0]
                    .get("kind")
                    .unwrap()
                    .clone(),
            ));
        }
    }
    Ok(result)
}
