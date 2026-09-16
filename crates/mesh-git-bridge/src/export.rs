//! Export one exact, human-approved Mesh state into an ordinary Git repository.
//!
//! Export is deliberately ref-only. It writes Git objects and atomically creates a new branch,
//! but it never checks out that branch, changes `HEAD`, touches the index, or edits the user's
//! working tree. The canonical human receipt is retained as a Git blob under a stable receipt ref,
//! so every `Mesh-Approval` trailer resolves locally without a Mesh daemon.
//!
//! The v1 approval state binds paths and bytes, but not Git executable bits. Export therefore
//! retains modes already present in the pinned parent tree and assigns ordinary `100644` mode to
//! new paths. It never promotes a mutable native-folder permission bit into approved Git history.

use crate::{
    isolated_git_command,
    workspace::{open_directory, remove_directory_contents},
    GitObjectFormat, GitProvenanceAnchor,
};
use mesh_approval::{
    verify_human_approval_receipt, ActorId, Blake3, Content, ContentDigest, ExpectedHumanApproval,
    HeadId, ObjectKind, ReviewBundle, WorkspaceState,
};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::os::fd::AsRawFd as _;
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_EXPORT: AtomicU64 = AtomicU64::new(1);

#[cfg(test)]
thread_local! {
    static BEFORE_TEMPORARY_REPOSITORY_REMOVAL: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        std::cell::RefCell::new(None);
}

#[cfg(target_os = "macos")]
const OPEN_DIRECTORY_FLAGS: i32 = 0x0110_0100;
#[cfg(target_os = "linux")]
const OPEN_DIRECTORY_FLAGS: i32 = 0x000b_0000;

/// Exact authority and state facts consumed by one Git export.
///
/// The bridge borrows these facts because their durable owner is the publication layer. It never
/// persists a caller-supplied substitute or extends their lifetime.
pub struct ApprovedGitExportSource<'a> {
    receipt_bytes: &'a [u8],
    expected_approval: &'a ExpectedHumanApproval,
    review_bundle: &'a ReviewBundle,
    approved_state: &'a WorkspaceState,
    agents: &'a [ActorId],
}

impl<'a> ApprovedGitExportSource<'a> {
    /// Bind the signed receipt to the exact reviewed state and its participating agents.
    #[must_use]
    pub const fn new(
        receipt_bytes: &'a [u8],
        expected_approval: &'a ExpectedHumanApproval,
        review_bundle: &'a ReviewBundle,
        approved_state: &'a WorkspaceState,
        agents: &'a [ActorId],
    ) -> Self {
        Self {
            receipt_bytes,
            expected_approval,
            review_bundle,
            approved_state,
            agents,
        }
    }
}

/// A stable, read-only preview of an approved Git export.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitExportPreview {
    materialized_root: PathBuf,
    materialized_identity: (u64, u64),
    target_repository: PathBuf,
    target_identity: (u64, u64),
    target_git_directory: PathBuf,
    target_git_identity: (u64, u64),
    expected_target_head: String,
    tree: String,
    approval: String,
    state: String,
    actors: Vec<String>,
    author: String,
    branch: String,
    message: Vec<u8>,
    approved_state_digest: String,
    review_bundle: String,
}

impl GitExportPreview {
    /// Exact Git tree object produced from the materialized Mesh folder.
    #[must_use]
    pub fn tree(&self) -> &str {
        &self.tree
    }

    /// BLAKE3 name of the canonical, verified approval receipt.
    #[must_use]
    pub fn approval(&self) -> &str {
        &self.approval
    }

    /// Mesh canonical state named by the approval.
    #[must_use]
    pub fn state(&self) -> &str {
        &self.state
    }

    /// Sorted, unique actor identities retained in the commit trailer.
    #[must_use]
    pub fn actors(&self) -> &[String] {
        &self.actors
    }

    /// User-visible branch that confirmation will create without switching the checkout.
    #[must_use]
    pub fn branch(&self) -> &str {
        &self.branch
    }
}

/// The immutable Git objects and refs created by a confirmed export.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovedGitExport {
    tree: String,
    commit: String,
    branch: String,
    approval: String,
    approval_ref: String,
    already_present: bool,
}

impl ApprovedGitExport {
    /// Exact exported Git tree.
    #[must_use]
    pub fn tree(&self) -> &str {
        &self.tree
    }

    /// Exact exported Git commit.
    #[must_use]
    pub fn commit(&self) -> &str {
        &self.commit
    }

    /// Branch created for the approved state.
    #[must_use]
    pub fn branch(&self) -> &str {
        &self.branch
    }

    /// Receipt identity carried by the commit.
    #[must_use]
    pub fn approval(&self) -> &str {
        &self.approval
    }

    /// Git ref whose target is the exact canonical receipt blob.
    #[must_use]
    pub fn approval_ref(&self) -> &str {
        &self.approval_ref
    }

    /// Whether confirmation found the exact export already installed.
    #[must_use]
    pub const fn already_present(&self) -> bool {
        self.already_present
    }
}

/// Preview a ref-only export of one exact approved materialization.
///
/// The signed receipt is verified against caller-independent expected approval facts before any
/// Git object is accepted. The target repository must still have the imported repository identity
/// and `HEAD`; its dirty working files may differ because export never reads or changes them.
///
/// # Errors
///
/// Returns a typed refusal for invalid authority, target drift, unsupported materialized entries,
/// or any non-deterministic tree read.
pub fn preview_approved_git_export(
    materialized_root: &Path,
    target_repository: &Path,
    expected_target: &GitProvenanceAnchor,
    approved: &ApprovedGitExportSource<'_>,
) -> Result<GitExportPreview, GitExportError> {
    verify_human_approval_receipt(approved.receipt_bytes, approved.expected_approval)
        .map_err(|_| GitExportError::ApprovalInvalid)?;
    verify_approved_state(
        approved.expected_approval,
        approved.review_bundle,
        approved.approved_state,
    )?;

    let materialized_root = canonical_real_directory(materialized_root)?;
    let target_repository = canonical_real_directory(target_repository)?;
    if materialized_root == target_repository {
        return Err(GitExportError::SameWorkspace);
    }
    verify_target(&target_repository, expected_target)?;
    let target_git_directory = canonical_target_git_directory(&target_repository)?;
    let materialized_identity = directory_identity(&materialized_root)?;
    let target_identity = directory_identity(&target_repository)?;
    let target_git_identity = directory_identity(&target_git_directory)?;
    validate_materialization(&materialized_root, approved.approved_state)?;

    let first = build_tree(
        &materialized_root,
        &target_repository,
        expected_target.head().as_str(),
        expected_target.repository().object_format(),
    )?;
    let second = build_tree(
        &materialized_root,
        &target_repository,
        expected_target.head().as_str(),
        expected_target.repository().object_format(),
    )?;
    if first != second {
        return Err(GitExportError::MaterializationChanged);
    }

    let approval = Blake3::digest_bytes(approved.receipt_bytes).to_hex();
    let state = approved
        .expected_approval
        .context()
        .reviewed_actor_head()
        .to_string();
    let author = approved.review_bundle.author();
    let mut actors = approved.agents.to_vec();
    actors.push(author);
    actors.sort();
    actors.dedup();
    let actors = actors
        .into_iter()
        .map(|actor| actor.to_string())
        .collect::<Vec<_>>();
    let author = author.to_string();
    let approved_state_digest = approved.approved_state.digest().to_hex();
    let review_bundle = approved.review_bundle.id().to_hex();
    let branch = format!("mesh/approved/{state}");
    let message = commit_message(&approval, &state, &actors);

    Ok(GitExportPreview {
        materialized_root,
        materialized_identity,
        target_repository,
        target_identity,
        target_git_directory,
        target_git_identity,
        expected_target_head: expected_target.head().as_str().to_owned(),
        tree: first,
        approval,
        state,
        actors,
        author,
        branch,
        message,
        approved_state_digest,
        review_bundle,
    })
}

/// Confirm a preview by atomically creating its approval ref and approved-state branch.
///
/// Confirmation repeats receipt verification, target checks, and tree construction. The target's
/// current checkout and current branch are never changed. Retrying an exactly installed export is
/// idempotent; any conflicting branch or receipt ref is refused.
///
/// # Errors
///
/// Returns a typed refusal without updating either ref when any preview fact changed.
pub fn confirm_approved_git_export(
    preview: &GitExportPreview,
    expected_target: &GitProvenanceAnchor,
    approved: &ApprovedGitExportSource<'_>,
) -> Result<ApprovedGitExport, GitExportError> {
    verify_human_approval_receipt(approved.receipt_bytes, approved.expected_approval)
        .map_err(|_| GitExportError::ApprovalInvalid)?;
    verify_approved_state(
        approved.expected_approval,
        approved.review_bundle,
        approved.approved_state,
    )?;
    let approval = Blake3::digest_bytes(approved.receipt_bytes).to_hex();
    let state = approved
        .expected_approval
        .context()
        .reviewed_actor_head()
        .to_string();
    if approval != preview.approval
        || state != preview.state
        || approved.approved_state.digest().to_hex() != preview.approved_state_digest
        || approved.review_bundle.id().to_hex() != preview.review_bundle
        || expected_target.head().as_str() != preview.expected_target_head
    {
        return Err(GitExportError::PreviewChanged);
    }
    let materialized =
        PinnedDirectory::open(&preview.materialized_root, preview.materialized_identity)?
            .ok_or(GitExportError::MaterializationChanged)?;
    let target = PinnedDirectory::open(&preview.target_repository, preview.target_identity)?
        .ok_or(GitExportError::TargetChanged)?;
    let target_git =
        PinnedDirectory::open(&preview.target_git_directory, preview.target_git_identity)?
            .ok_or(GitExportError::TargetChanged)?;
    verify_pinned_target(&target, &target_git, expected_target)?;
    validate_pinned_materialization(&materialized, approved.approved_state)?;

    let artifact = build_artifact(
        &preview.materialized_root,
        &preview.target_repository,
        expected_target.repository().object_format(),
        &preview.expected_target_head,
        approved.receipt_bytes,
        &preview.message,
        &preview.author,
    )?;
    if artifact.tree != preview.tree {
        return Err(GitExportError::MaterializationChanged);
    }
    let rechecked = build_tree(
        &preview.materialized_root,
        &preview.target_repository,
        &preview.expected_target_head,
        expected_target.repository().object_format(),
    )?;
    if rechecked != preview.tree {
        return Err(GitExportError::MaterializationChanged);
    }
    verify_pinned_target(&target, &target_git, expected_target)?;

    let branch_ref = format!("refs/heads/{}", preview.branch);
    let approval_ref = format!("refs/mesh/approvals/{}", preview.approval);
    if let Some(existing) = read_pinned_ref(&target_git, &branch_ref)? {
        return existing_export(
            preview,
            &target,
            &target_git,
            &branch_ref,
            &approval_ref,
            &existing,
            approved.receipt_bytes,
        );
    }

    pinned_git(
        &target_git,
        [
            OsStr::new("fetch"),
            OsStr::new("--no-tags"),
            OsStr::new("--no-write-fetch-head"),
            artifact.repository.path().as_os_str(),
            OsStr::new("refs/mesh/export"),
            OsStr::new("refs/mesh/approval"),
        ],
        None,
    )?;
    verify_pinned_target(&target, &target_git, expected_target)?;

    let existing_approval = read_pinned_ref(&target_git, &approval_ref)?;
    if existing_approval
        .as_ref()
        .is_some_and(|existing| existing != &artifact.receipt_blob)
    {
        return Err(GitExportError::ApprovalRefConflict);
    }
    let mut transaction = format!("start\nverify HEAD {}\n", preview.expected_target_head);
    if existing_approval.is_none() {
        transaction.push_str(&format!(
            "create {approval_ref} {}\n",
            artifact.receipt_blob
        ));
    }
    transaction.push_str(&format!(
        "create {branch_ref} {}\nprepare\ncommit\n",
        artifact.commit
    ));
    pinned_git(
        &target_git,
        [OsStr::new("update-ref"), OsStr::new("--stdin")],
        Some(transaction.into_bytes()),
    )?;
    let installed_branch = read_pinned_ref(&target_git, &branch_ref)?;
    let installed_approval = read_pinned_ref(&target_git, &approval_ref)?;
    if installed_branch.as_deref() != Some(artifact.commit.as_str())
        || installed_approval.as_deref() != Some(artifact.receipt_blob.as_str())
    {
        return Err(GitExportError::TargetChanged);
    }

    Ok(ApprovedGitExport {
        tree: artifact.tree.clone(),
        commit: artifact.commit.clone(),
        branch: preview.branch.clone(),
        approval: preview.approval.clone(),
        approval_ref,
        already_present: false,
    })
}

/// Inspect whether an exact approved export is already installed without creating any Git ref.
///
/// This is the recovery half of confirmation. It repeats receipt, workspace, materialization, and
/// target verification, then accepts only the exact branch and approval-ref pair that confirmation
/// would have installed. An absent branch returns `None`; a conflicting branch or receipt fails
/// closed. The target checkout and refs are never changed.
///
/// # Errors
///
/// Returns a typed refusal when approval or preview facts changed, the materialization no longer
/// equals the approved state, or an installed ref conflicts with the exact export.
pub fn inspect_approved_git_export(
    preview: &GitExportPreview,
    expected_target: &GitProvenanceAnchor,
    approved: &ApprovedGitExportSource<'_>,
) -> Result<Option<ApprovedGitExport>, GitExportError> {
    verify_human_approval_receipt(approved.receipt_bytes, approved.expected_approval)
        .map_err(|_| GitExportError::ApprovalInvalid)?;
    verify_approved_state(
        approved.expected_approval,
        approved.review_bundle,
        approved.approved_state,
    )?;
    let approval = Blake3::digest_bytes(approved.receipt_bytes).to_hex();
    let state = approved
        .expected_approval
        .context()
        .reviewed_actor_head()
        .to_string();
    if approval != preview.approval
        || state != preview.state
        || approved.approved_state.digest().to_hex() != preview.approved_state_digest
        || approved.review_bundle.id().to_hex() != preview.review_bundle
        || expected_target.head().as_str() != preview.expected_target_head
    {
        return Err(GitExportError::PreviewChanged);
    }

    let materialized =
        PinnedDirectory::open(&preview.materialized_root, preview.materialized_identity)?
            .ok_or(GitExportError::MaterializationChanged)?;
    let target = PinnedDirectory::open(&preview.target_repository, preview.target_identity)?
        .ok_or(GitExportError::TargetChanged)?;
    let target_git =
        PinnedDirectory::open(&preview.target_git_directory, preview.target_git_identity)?
            .ok_or(GitExportError::TargetChanged)?;
    verify_pinned_target(&target, &target_git, expected_target)?;
    validate_pinned_materialization(&materialized, approved.approved_state)?;
    for _ in 0..2 {
        let tree = build_tree(
            &preview.materialized_root,
            &preview.target_repository,
            &preview.expected_target_head,
            expected_target.repository().object_format(),
        )?;
        if tree != preview.tree {
            return Err(GitExportError::MaterializationChanged);
        }
    }
    verify_pinned_target(&target, &target_git, expected_target)?;

    let branch_ref = format!("refs/heads/{}", preview.branch);
    let approval_ref = format!("refs/mesh/approvals/{}", preview.approval);
    let Some(existing) = read_pinned_ref(&target_git, &branch_ref)? else {
        if let Some(existing_approval) = read_pinned_ref(&target_git, &approval_ref)? {
            let receipt = pinned_git(
                &target_git,
                [
                    OsStr::new("cat-file"),
                    OsStr::new("blob"),
                    OsStr::new(&existing_approval),
                ],
                None,
            )?
            .stdout;
            if receipt != approved.receipt_bytes {
                return Err(GitExportError::ApprovalRefConflict);
            }
        }
        verify_pinned_directory(&target, GitExportError::TargetChanged)?;
        return Ok(None);
    };
    existing_export(
        preview,
        &target,
        &target_git,
        &branch_ref,
        &approval_ref,
        &existing,
        approved.receipt_bytes,
    )
    .map(Some)
}

fn existing_export(
    preview: &GitExportPreview,
    target: &PinnedDirectory,
    target_git: &PinnedDirectory,
    branch_ref: &str,
    approval_ref: &str,
    existing: &str,
    receipt_bytes: &[u8],
) -> Result<ApprovedGitExport, GitExportError> {
    verify_pinned_directory(target, GitExportError::TargetChanged)?;
    let tree = one_line(pinned_git(
        target_git,
        [
            OsStr::new("show"),
            OsStr::new("-s"),
            OsStr::new("--format=%T"),
            OsStr::new(existing),
        ],
        None,
    )?)?;
    let parent = control_line(pinned_git(
        target_git,
        [
            OsStr::new("show"),
            OsStr::new("-s"),
            OsStr::new("--format=%P"),
            OsStr::new(existing),
        ],
        None,
    )?)?;
    let message = pinned_git(
        target_git,
        [
            OsStr::new("show"),
            OsStr::new("-s"),
            OsStr::new("--format=%B"),
            OsStr::new(existing),
        ],
        None,
    )?
    .stdout;
    let identity = control_line(pinned_git(
        target_git,
        [
            OsStr::new("show"),
            OsStr::new("-s"),
            OsStr::new("--format=%an%x00%ae%x00%cn%x00%ce"),
            OsStr::new(existing),
        ],
        None,
    )?)?;
    let expected_identity = format!(
        "mesh actor {}\0actor-{}@mesh.invalid\0mesh approval export\0approval@mesh.invalid",
        &preview.author[..12],
        preview.author
    );
    if tree != preview.tree
        || parent != preview.expected_target_head
        || trim_line_endings(&message) != trim_line_endings(&preview.message)
        || identity != expected_identity
    {
        return Err(GitExportError::BranchConflict);
    }
    let approval_target =
        read_pinned_ref(target_git, approval_ref)?.ok_or(GitExportError::ApprovalRefConflict)?;
    let receipt = pinned_git(
        target_git,
        [
            OsStr::new("cat-file"),
            OsStr::new("blob"),
            OsStr::new(&approval_target),
        ],
        None,
    )?
    .stdout;
    if receipt != receipt_bytes {
        return Err(GitExportError::ApprovalRefConflict);
    }
    verify_pinned_directory(target, GitExportError::TargetChanged)?;
    Ok(ApprovedGitExport {
        tree,
        commit: existing.to_owned(),
        branch: branch_ref.trim_start_matches("refs/heads/").to_owned(),
        approval: preview.approval.clone(),
        approval_ref: approval_ref.to_owned(),
        already_present: true,
    })
}

fn verify_target(target: &Path, expected: &GitProvenanceAnchor) -> Result<(), GitExportError> {
    let actual = GitProvenanceAnchor::inspect(target).map_err(GitExportError::Import)?;
    if actual.repository() != expected.repository() || actual.head() != expected.head() {
        return Err(GitExportError::TargetChanged);
    }
    if !actual.gitlinks().is_empty() || !expected.gitlinks().is_empty() {
        return Err(GitExportError::SubmodulesUnsupported);
    }
    Ok(())
}

fn verify_approved_state(
    expected: &ExpectedHumanApproval,
    bundle: &ReviewBundle,
    state: &WorkspaceState,
) -> Result<(), GitExportError> {
    let context = expected.context();
    if context.reviewed_actor_head() == HeadId::from_bytes([0; 32])
        || context.reviewed_actor_head() != bundle.actor_head()
        || context.review_bundle() != bundle.id()
        || bundle.actor_state() != state.digest()
    {
        return Err(GitExportError::StateInvalid);
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ExpectedEntry {
    Directory,
    File { digest: [u8; 32], length: u64 },
}

fn validate_materialization(
    root: &Path,
    approved_state: &WorkspaceState,
) -> Result<(), GitExportError> {
    let mut expected = BTreeMap::new();
    for (object, held) in approved_state.objects() {
        if *object == approved_state.root() {
            continue;
        }
        let path = approved_state
            .path_of(*object)
            .and_then(|path| path.strip_prefix('/').map(str::to_owned))
            .filter(|path| !path.is_empty())
            .ok_or(GitExportError::StateInvalid)?;
        let entry = match held.kind() {
            ObjectKind::Directory if held.content().is_none() => ExpectedEntry::Directory,
            ObjectKind::File => match held.content() {
                Some(Content::Binary {
                    digest,
                    byte_length,
                    ..
                }) => ExpectedEntry::File {
                    digest: *digest,
                    length: *byte_length,
                },
                _ => return Err(GitExportError::StateInvalid),
            },
            _ => return Err(GitExportError::StateInvalid),
        };
        if expected.insert(path, entry).is_some() {
            return Err(GitExportError::StateInvalid);
        }
    }

    let mut actual = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let mut entries = fs::read_dir(&directory)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(fs::DirEntry::file_name);
        let mut exportable_entries = 0_usize;
        for entry in entries {
            let path = entry.path();
            if directory == root && entry.file_name() == OsStr::new(".git") {
                let metadata = fs::symlink_metadata(&path)?;
                if !metadata.is_dir() || metadata.file_type().is_symlink() {
                    return Err(GitExportError::GitEntryInvalid);
                }
                continue;
            }
            exportable_entries += 1;
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() || !(metadata.is_dir() || metadata.is_file()) {
                return Err(GitExportError::UnsupportedEntry(path));
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|_| GitExportError::StateInvalid)?
                .to_str()
                .ok_or(GitExportError::StateInvalid)?
                .replace(std::path::MAIN_SEPARATOR, "/");
            if metadata.is_dir() {
                actual.insert(relative, ExpectedEntry::Directory);
                pending.push(path);
            } else {
                let bytes = fs::read(&path)?;
                actual.insert(
                    relative,
                    ExpectedEntry::File {
                        digest: *Blake3::digest_bytes(&bytes).as_bytes(),
                        length: u64::try_from(bytes.len())
                            .map_err(|_| GitExportError::StateInvalid)?,
                    },
                );
            }
        }
        if directory != root && exportable_entries == 0 {
            return Err(GitExportError::EmptyDirectory(directory));
        }
    }
    if actual != expected {
        return Err(GitExportError::MaterializationMismatch);
    }
    Ok(())
}

fn build_tree(
    root: &Path,
    target_repository: &Path,
    parent: &str,
    format: GitObjectFormat,
) -> Result<String, GitExportError> {
    let artifact = TemporaryRepository::new(format)?;
    target_git(
        artifact.path(),
        [
            OsStr::new("fetch"),
            OsStr::new("--no-tags"),
            OsStr::new("--no-write-fetch-head"),
            target_repository.as_os_str(),
            OsStr::new(parent),
        ],
        None,
    )?;
    artifact.add_tree(root, parent)
}

fn build_artifact(
    root: &Path,
    target_repository: &Path,
    format: GitObjectFormat,
    parent: &str,
    receipt: &[u8],
    message: &[u8],
    author: &str,
) -> Result<Artifact, GitExportError> {
    let repository = TemporaryRepository::new(format)?;
    target_git(
        repository.path(),
        [
            OsStr::new("fetch"),
            OsStr::new("--no-tags"),
            OsStr::new("--no-write-fetch-head"),
            target_repository.as_os_str(),
            OsStr::new(parent),
        ],
        None,
    )?;
    let tree = repository.add_tree(root, parent)?;
    let receipt_blob = one_line(target_git(
        repository.path(),
        [
            OsStr::new("hash-object"),
            OsStr::new("-w"),
            OsStr::new("--stdin"),
        ],
        Some(receipt.to_vec()),
    )?)?;
    target_git(
        repository.path(),
        [
            OsStr::new("update-ref"),
            OsStr::new("refs/mesh/approval"),
            OsStr::new(&receipt_blob),
        ],
        None,
    )?;
    let name = format!("Mesh actor {}", &author[..12]);
    let email = format!("actor-{author}@mesh.invalid");
    let commit = one_line(target_git_with_identity(
        repository.path(),
        [
            OsStr::new("commit-tree"),
            OsStr::new(&tree),
            OsStr::new("-p"),
            OsStr::new(parent),
        ],
        message.to_vec(),
        &name,
        &email,
    )?)?;
    target_git(
        repository.path(),
        [
            OsStr::new("update-ref"),
            OsStr::new("refs/mesh/export"),
            OsStr::new(&commit),
        ],
        None,
    )?;
    Ok(Artifact {
        repository,
        tree,
        commit,
        receipt_blob,
    })
}

fn commit_message(approval: &str, state: &str, actors: &[String]) -> Vec<u8> {
    format!(
        "Export approved Mesh state\n\nMesh-Approval: {approval}\nMesh-State: {state}\nMesh-Actors: {}\n",
        actors.join(",")
    )
    .into_bytes()
}

struct Artifact {
    repository: TemporaryRepository,
    tree: String,
    commit: String,
    receipt_blob: String,
}

struct TemporaryRepository {
    path: PathBuf,
    directory: File,
    #[cfg(unix)]
    identity: (u64, u64),
}

impl TemporaryRepository {
    fn new(format: GitObjectFormat) -> Result<Self, GitExportError> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| GitExportError::ClockUnavailable)?
            .as_nanos();
        let serial = NEXT_EXPORT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "mesh-git-export-{}-{nonce}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        #[cfg(unix)]
        let identity = {
            use std::os::unix::fs::MetadataExt as _;
            let metadata = fs::symlink_metadata(&path)?;
            (metadata.dev(), metadata.ino())
        };
        let repository = Self {
            directory: open_directory(&path)?,
            path,
            #[cfg(unix)]
            identity,
        };
        target_git(
            repository.path(),
            [
                OsStr::new("init"),
                OsStr::new("--bare"),
                OsStr::new(match format {
                    GitObjectFormat::Sha1 => "--object-format=sha1",
                    GitObjectFormat::Sha256 => "--object-format=sha256",
                }),
                repository.path().as_os_str(),
            ],
            None,
        )?;
        Ok(repository)
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn add_tree(&self, root: &Path, parent: &str) -> Result<String, GitExportError> {
        let index = self.path.join("mesh-export.index");
        target_git_with_index(
            &self.path,
            root,
            &index,
            [OsStr::new("read-tree"), OsStr::new(parent)],
        )?;
        target_git_with_index(
            &self.path,
            root,
            &index,
            [
                OsStr::new("add"),
                OsStr::new("--force"),
                OsStr::new("--all"),
                OsStr::new("--"),
                OsStr::new("."),
                OsStr::new(":(exclude).git"),
                OsStr::new(":(exclude).git/**"),
            ],
        )?;
        one_line(target_git_with_index(
            &self.path,
            root,
            &index,
            [OsStr::new("write-tree")],
        )?)
    }
}

impl Drop for TemporaryRepository {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt as _;
            let Ok(metadata) = fs::symlink_metadata(&self.path) else {
                return;
            };
            if metadata.file_type().is_symlink()
                || !metadata.is_dir()
                || (metadata.dev(), metadata.ino()) != self.identity
            {
                return;
            }
        }
        #[cfg(test)]
        BEFORE_TEMPORARY_REPOSITORY_REMOVAL.with(|hook| {
            if let Some(hook) = hook.borrow_mut().take() {
                hook();
            }
        });
        if remove_directory_contents(&self.directory).is_err() {
            return;
        }
        let Ok(metadata) = fs::symlink_metadata(&self.path) else {
            return;
        };
        if metadata.file_type().is_dir()
            && !metadata.file_type().is_symlink()
            && (metadata.dev(), metadata.ino()) == self.identity
        {
            let _ = fs::remove_dir(&self.path);
        }
    }
}

#[cfg(test)]
mod temporary_repository_tests {
    use super::*;

    #[test]
    fn cleanup_never_follows_a_replaced_temporary_repository() {
        let repository = TemporaryRepository::new(GitObjectFormat::Sha1).expect("repository");
        let path = repository.path().to_path_buf();
        let displaced = path.with_extension("displaced");
        let replacement = path.with_extension("replacement");
        fs::create_dir(&replacement).expect("replacement");
        fs::write(replacement.join("outside.txt"), b"must survive\n").expect("outside sentinel");
        let path_for_hook = path.clone();
        let displaced_for_hook = displaced.clone();
        let replacement_for_hook = replacement.clone();
        BEFORE_TEMPORARY_REPOSITORY_REMOVAL.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                fs::rename(&path_for_hook, &displaced_for_hook).expect("displace repository");
                fs::rename(&replacement_for_hook, &path_for_hook).expect("install replacement");
            }));
        });
        drop(repository);
        assert_eq!(
            fs::read(path.join("outside.txt")).expect("outside sentinel retained"),
            b"must survive\n"
        );
        let _ = fs::remove_dir_all(path);
        let _ = fs::remove_dir_all(displaced);
    }
}

struct PinnedDirectory {
    path: PathBuf,
    directory: File,
    identity: (u64, u64),
}

impl PinnedDirectory {
    fn open(path: &Path, expected: (u64, u64)) -> io::Result<Option<Self>> {
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(OPEN_DIRECTORY_FLAGS)
            .open(path)?;
        let metadata = directory.metadata()?;
        let pinned = Self {
            path: path.to_path_buf(),
            directory,
            identity: (metadata.dev(), metadata.ino()),
        };
        if pinned.identity != expected || !pinned.is_current()? {
            return Ok(None);
        }
        Ok(Some(pinned))
    }

    fn is_current(&self) -> io::Result<bool> {
        let retained = self.directory.metadata()?;
        let named = fs::symlink_metadata(&self.path)?;
        Ok(retained.is_dir()
            && named.is_dir()
            && !named.file_type().is_symlink()
            && (retained.dev(), retained.ino()) == self.identity
            && (named.dev(), named.ino()) == self.identity)
    }
}

fn verify_pinned_directory(
    directory: &PinnedDirectory,
    changed: GitExportError,
) -> Result<(), GitExportError> {
    if directory.is_current()? {
        Ok(())
    } else {
        Err(changed)
    }
}

fn verify_pinned_target(
    target: &PinnedDirectory,
    git: &PinnedDirectory,
    expected: &GitProvenanceAnchor,
) -> Result<(), GitExportError> {
    verify_pinned_directory(target, GitExportError::TargetChanged)?;
    verify_pinned_directory(git, GitExportError::TargetChanged)?;
    verify_target(&target.path, expected)?;
    if canonical_target_git_directory(&target.path)? != git.path {
        return Err(GitExportError::TargetChanged);
    }
    verify_pinned_directory(target, GitExportError::TargetChanged)?;
    verify_pinned_directory(git, GitExportError::TargetChanged)
}

fn validate_pinned_materialization(
    materialized: &PinnedDirectory,
    approved_state: &WorkspaceState,
) -> Result<(), GitExportError> {
    verify_pinned_directory(materialized, GitExportError::MaterializationChanged)?;
    validate_materialization(&materialized.path, approved_state)?;
    verify_pinned_directory(materialized, GitExportError::MaterializationChanged)
}

fn directory_identity(path: &Path) -> Result<(u64, u64), GitExportError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(GitExportError::NotRealDirectory);
    }
    Ok((metadata.dev(), metadata.ino()))
}

fn canonical_target_git_directory(target: &Path) -> Result<PathBuf, GitExportError> {
    let output = target_git(
        target,
        [OsStr::new("rev-parse"), OsStr::new("--absolute-git-dir")],
        None,
    )?;
    let bytes = trim_final_newline(&output.stdout);
    if bytes.is_empty() || bytes.contains(&b'\n') || bytes.contains(&b'\r') {
        return Err(GitExportError::GitOutputInvalid);
    }
    let path = std::str::from_utf8(bytes).map_err(|_| GitExportError::GitOutputInvalid)?;
    let git = canonical_real_directory(Path::new(path))?;
    let common_output = target_git(
        target,
        [OsStr::new("rev-parse"), OsStr::new("--git-common-dir")],
        None,
    )?;
    let common_bytes = trim_final_newline(&common_output.stdout);
    if common_bytes.is_empty() || common_bytes.contains(&b'\n') || common_bytes.contains(&b'\r') {
        return Err(GitExportError::GitOutputInvalid);
    }
    let common_text =
        std::str::from_utf8(common_bytes).map_err(|_| GitExportError::GitOutputInvalid)?;
    let common_path = Path::new(common_text);
    let common_candidate = if common_path.is_absolute() {
        common_path.to_path_buf()
    } else {
        target.join(common_path)
    };
    let common = canonical_real_directory(&common_candidate)?;
    if common != git {
        return Err(GitExportError::LinkedWorktreeUnsupported);
    }
    Ok(git)
}

fn canonical_real_directory(path: &Path) -> Result<PathBuf, GitExportError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(GitExportError::NotRealDirectory);
    }
    fs::canonicalize(path).map_err(GitExportError::Io)
}

fn target_git<'a>(
    root: &Path,
    arguments: impl IntoIterator<Item = &'a OsStr>,
    input: Option<Vec<u8>>,
) -> Result<Output, GitExportError> {
    let mut command = safe_git();
    command.arg("-C").arg(root).args(arguments);
    run(command, input)
}

fn pinned_git<'a>(
    git: &PinnedDirectory,
    arguments: impl IntoIterator<Item = &'a OsStr>,
    input: Option<Vec<u8>>,
) -> Result<Output, GitExportError> {
    let mut command = safe_git();
    command.arg("--git-dir").arg(".").args(arguments);
    pin_command_directory(&mut command, &git.directory)?;
    run(command, input)
}

#[allow(unsafe_code)]
fn pin_command_directory(command: &mut Command, directory: &File) -> io::Result<()> {
    let directory = directory.try_clone()?;
    // SAFETY: `pre_exec` runs in the forked child before exec. The captured descriptor remains
    // owned by that child and `fchdir` changes only the child's current directory.
    unsafe {
        command.pre_exec(move || {
            unsafe extern "C" {
                fn fchdir(descriptor: i32) -> i32;
            }
            if fchdir(directory.as_raw_fd()) == 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        });
    }
    Ok(())
}

fn target_git_with_index<'a>(
    git: &Path,
    worktree: &Path,
    index: &Path,
    arguments: impl IntoIterator<Item = &'a OsStr>,
) -> Result<Output, GitExportError> {
    let mut command = safe_git();
    command
        .arg("--git-dir")
        .arg(git)
        .arg("--work-tree")
        .arg(worktree)
        .args(arguments)
        .env("GIT_INDEX_FILE", index);
    run(command, None)
}

fn target_git_with_identity<'a>(
    git: &Path,
    arguments: impl IntoIterator<Item = &'a OsStr>,
    input: Vec<u8>,
    name: &str,
    email: &str,
) -> Result<Output, GitExportError> {
    let mut command = safe_git();
    command
        .arg("-C")
        .arg(git)
        .args(arguments)
        .env("GIT_AUTHOR_NAME", name)
        .env("GIT_AUTHOR_EMAIL", email)
        .env("GIT_COMMITTER_NAME", "Mesh approval export")
        .env("GIT_COMMITTER_EMAIL", "approval@mesh.invalid");
    run(command, Some(input))
}

fn safe_git() -> Command {
    let mut command = isolated_git_command();
    command
        .arg("-c")
        .arg("core.fsmonitor=false")
        .arg("-c")
        .arg("core.untrackedCache=false")
        .arg("-c")
        // The signed v1 approval state does not carry executable metadata. Starting the index at
        // the pinned parent and ignoring worktree mode changes preserves already-reviewed Git
        // modes while new files receive Git's ordinary non-executable mode.
        .arg("core.fileMode=false")
        .arg("-c")
        .arg("core.hooksPath=/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_PAGER", "cat")
        .env("LC_ALL", "C")
        .env("LANG", "C");
    command
}

fn run(mut command: Command, input: Option<Vec<u8>>) -> Result<Output, GitExportError> {
    if let Some(input) = input {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn()?;
        let mut stdin = child.stdin.take().ok_or_else(|| {
            GitExportError::Io(std::io::Error::other("Git command supplied no stdin pipe"))
        })?;
        let writer = std::thread::spawn(move || stdin.write_all(&input));
        let output = child.wait_with_output()?;
        let write_result = writer.join().map_err(|_| {
            GitExportError::Io(std::io::Error::other(
                "Git stdin writer terminated unexpectedly",
            ))
        })?;
        if output.status.success() {
            write_result?;
            return Ok(output);
        }
        return Err(git_failure(output));
    }
    let output = command.output()?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(git_failure(output))
    }
}

fn git_failure(output: Output) -> GitExportError {
    GitExportError::Git {
        status: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr[..output.stderr.len().min(4_096)])
            .trim()
            .to_owned(),
    }
}

fn read_pinned_ref(
    git: &PinnedDirectory,
    reference: &str,
) -> Result<Option<String>, GitExportError> {
    let output = pinned_git(
        git,
        [
            OsStr::new("rev-parse"),
            OsStr::new("--verify"),
            OsStr::new(reference),
        ],
        None,
    );
    match output {
        Ok(output) => one_line(output).map(Some),
        Err(GitExportError::Git {
            status: Some(128), ..
        }) => Ok(None),
        Err(error) => Err(error),
    }
}

fn one_line(output: Output) -> Result<String, GitExportError> {
    let text = control_line(output)?;
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(GitExportError::GitOutputInvalid);
    }
    Ok(text)
}

fn control_line(output: Output) -> Result<String, GitExportError> {
    let bytes = trim_final_newline(&output.stdout);
    if bytes.contains(&b'\n') || bytes.contains(&b'\r') {
        return Err(GitExportError::GitOutputInvalid);
    }
    std::str::from_utf8(bytes)
        .map(str::to_ascii_lowercase)
        .map_err(|_| GitExportError::GitOutputInvalid)
}

fn trim_final_newline(bytes: &[u8]) -> &[u8] {
    let without_lf = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    without_lf.strip_suffix(b"\r").unwrap_or(without_lf)
}

fn trim_line_endings(mut bytes: &[u8]) -> &[u8] {
    while let Some(stripped) = bytes
        .strip_suffix(b"\n")
        .or_else(|| bytes.strip_suffix(b"\r"))
    {
        bytes = stripped;
    }
    bytes
}

/// Why an approved Git export was refused.
#[derive(Debug)]
pub enum GitExportError {
    /// Signed approval bytes were invalid or did not match expected workspace truth.
    ApprovalInvalid,
    /// The receipt named the reserved all-zero state.
    StateInvalid,
    /// Git import inspection failed.
    Import(crate::GitImportError),
    /// Filesystem I/O failed.
    Io(std::io::Error),
    /// A supplied directory was absent, a symlink, or not a directory.
    NotRealDirectory,
    /// The materialization and target were the same directory.
    SameWorkspace,
    /// The target repository identity or HEAD moved after import.
    TargetChanged,
    /// Gitlinks require nested repository custody not supplied by this export.
    SubmodulesUnsupported,
    /// Linked worktrees share mutable Git metadata outside the reviewed repository directory.
    LinkedWorktreeUnsupported,
    /// Top-level `.git` was not the expected private directory.
    GitEntryInvalid,
    /// The materialization contained a symlink or special entry.
    UnsupportedEntry(PathBuf),
    /// Git cannot represent an empty directory exactly.
    EmptyDirectory(PathBuf),
    /// Repeated exact tree construction disagreed.
    MaterializationChanged,
    /// Materialized paths or bytes did not equal the exact approved workspace state.
    MaterializationMismatch,
    /// Approval, state, or target facts no longer matched the preview.
    PreviewChanged,
    /// The approval ref already names different bytes.
    ApprovalRefConflict,
    /// The branch already exists with different provenance or content.
    BranchConflict,
    /// System time was unavailable for a private temporary name.
    ClockUnavailable,
    /// Git returned malformed control output.
    GitOutputInvalid,
    /// Git refused an operation.
    Git {
        /// Exit status when available.
        status: Option<i32>,
        /// Bounded diagnostic.
        stderr: String,
    },
}

impl From<std::io::Error> for GitExportError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl fmt::Display for GitExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApprovalInvalid => formatter.write_str("Git export approval is invalid"),
            Self::StateInvalid => formatter.write_str("Git export state identity is invalid"),
            Self::Import(error) => {
                write!(formatter, "Git export target inspection failed: {error}")
            }
            Self::Io(error) => write!(formatter, "Git export filesystem operation failed: {error}"),
            Self::NotRealDirectory => formatter.write_str("Git export requires real directories"),
            Self::SameWorkspace => {
                formatter.write_str("Git export source and target are the same directory")
            }
            Self::TargetChanged => {
                formatter.write_str("Git export target moved from its imported history")
            }
            Self::SubmodulesUnsupported => {
                formatter.write_str("Git export does not yet support submodules")
            }
            Self::LinkedWorktreeUnsupported => {
                formatter.write_str("Git export does not yet support linked Git worktrees")
            }
            Self::GitEntryInvalid => {
                formatter.write_str("Git export materialization has an invalid .git entry")
            }
            Self::UnsupportedEntry(path) => write!(
                formatter,
                "Git export materialization contains an unsupported entry: {}",
                path.display()
            ),
            Self::EmptyDirectory(path) => write!(
                formatter,
                "Git export cannot represent an empty directory exactly: {}",
                path.display()
            ),
            Self::MaterializationChanged => {
                formatter.write_str("Git export materialization changed during verification")
            }
            Self::MaterializationMismatch => formatter.write_str(
                "Git export materialization does not equal the approved workspace state",
            ),
            Self::PreviewChanged => formatter.write_str("Git export facts changed after preview"),
            Self::ApprovalRefConflict => {
                formatter.write_str("Git export approval ref already names different bytes")
            }
            Self::BranchConflict => formatter
                .write_str("Git export branch already names different content or provenance"),
            Self::ClockUnavailable => {
                formatter.write_str("Git export could not name private temporary storage")
            }
            Self::GitOutputInvalid => formatter.write_str("Git export received invalid Git output"),
            Self::Git { status, stderr } => write!(
                formatter,
                "Git export command failed with status {status:?}: {stderr}"
            ),
        }
    }
}

impl std::error::Error for GitExportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Import(error) => Some(error),
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}
