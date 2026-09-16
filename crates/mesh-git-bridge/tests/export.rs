//! Real Git and real ES256 coverage for approved-state export.

use mesh_approval::{
    compute_bundle, ActorId, ApprovalWorkspaceId, Blake3, BundleRequest, Content, ContentDigest,
    ExpectedHumanApproval, HeadId, HumanApprovalContext, HumanApprovalCredential, NormalizedName,
    ObjectId, ReviewBundle, VersionId, WorkspaceState,
};
use mesh_git_bridge::{
    confirm_approved_git_export, inspect_approved_git_export, preview_approved_git_export,
    ApprovedGitExportSource, GitExportError, GitProvenanceAnchor,
};
use mesh_types::PolicyEpoch;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "mesh-git-export-test-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("scratch");
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn git(root: &Path, arguments: &[&str]) -> Vec<u8> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn forged_commit(root: &Path, tree: &str, parent: &str, message: &str) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["commit-tree", tree, "-p", parent, "-m", message])
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Substituted author")
        .env("GIT_AUTHOR_EMAIL", "substituted@example.invalid")
        .env("GIT_COMMITTER_NAME", "Substituted committer")
        .env("GIT_COMMITTER_EMAIL", "substituted@example.invalid")
        .env("LC_ALL", "C")
        .output()
        .expect("forge commit identity");
    assert!(
        output.status.success(),
        "forge commit: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("commit id")
        .trim()
        .to_owned()
}

fn decode_hex(encoded: &str) -> Vec<u8> {
    assert_eq!(encoded.len() % 2, 0);
    encoded
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).expect("fixture hex"), 16)
                .expect("fixture byte")
        })
        .collect()
}

fn repository(label: &str) -> Scratch {
    let root = Scratch::new(label);
    git(&root.0, &["init", "-b", "main"]);
    git(&root.0, &["config", "user.name", "Original user"]);
    git(
        &root.0,
        &["config", "user.email", "original@example.invalid"],
    );
    fs::write(root.0.join("README.md"), b"original\n").expect("baseline");
    git(&root.0, &["add", "README.md"]);
    git(&root.0, &["commit", "-m", "baseline"]);
    root
}

fn approved_materialization(label: &str) -> Scratch {
    let root = Scratch::new(label);
    fs::create_dir(root.0.join("src")).expect("src");
    fs::write(root.0.join("README.md"), b"approved\n").expect("readme");
    fs::write(root.0.join("src/main.rs"), b"fn main() {}\n").expect("source");
    fs::write(root.0.join(".gitignore"), b"generated.bin\n").expect("ignore");
    fs::write(root.0.join("generated.bin"), b"approved ignored bytes\n").expect("ignored");
    fs::create_dir(root.0.join(".git")).expect("private git");
    fs::write(root.0.join(".git/private"), b"must not export\n").expect("private metadata");
    root
}

fn expected_tree() -> (Scratch, String) {
    let root = Scratch::new("expected-tree");
    git(&root.0, &["init", "-b", "main"]);
    fs::create_dir(root.0.join("src")).expect("src");
    fs::write(root.0.join("README.md"), b"approved\n").expect("readme");
    fs::write(root.0.join("src/main.rs"), b"fn main() {}\n").expect("source");
    fs::write(root.0.join(".gitignore"), b"generated.bin\n").expect("ignore");
    fs::write(root.0.join("generated.bin"), b"approved ignored bytes\n").expect("ignored");
    git(&root.0, &["add", "--force", "--all"]);
    let tree = String::from_utf8(git(&root.0, &["write-tree"]))
        .expect("tree")
        .trim()
        .to_owned();
    (root, tree)
}

struct Approval {
    expected: ExpectedHumanApproval,
    receipt: Vec<u8>,
    agent: ActorId,
    bundle: ReviewBundle,
    state: WorkspaceState,
}

impl Approval {
    fn source(&self) -> ApprovedGitExportSource<'_> {
        ApprovedGitExportSource::new(
            &self.receipt,
            &self.expected,
            &self.bundle,
            &self.state,
            std::slice::from_ref(&self.agent),
        )
    }
}

fn approval() -> Approval {
    let public_key: [u8; 65] = decode_hex(concat!(
        "04a760824a7c71c898c6d9e0e4e9e8dc8d9b275812a235713155fb99dfadb6fb",
        "fff104c70a12ce9a6c19d6c60b17cf7d68d783ef11d9a744d83d8fe0276b83b38f"
    ))
    .try_into()
    .expect("fixed public key");
    let credential = HumanApprovalCredential::from_public_key(public_key).expect("credential");
    let root = ObjectId::from_bytes([0; 16]);
    let author = ActorId::from_bytes([7; 32]);
    let agent = ActorId::from_bytes([8; 32]);
    let canonical = WorkspaceState::new(root);
    let binary = |version: u8, bytes: &[u8]| Content::Binary {
        version: VersionId::from_bytes([version; 32]),
        digest: *Blake3::digest_bytes(bytes).as_bytes(),
        byte_length: u64::try_from(bytes.len()).expect("fixture length"),
    };
    let src = ObjectId::from_bytes([1; 16]);
    let actor_state = canonical
        .clone()
        .with_directory(src, root, NormalizedName::new("src").expect("name"))
        .with_file(
            ObjectId::from_bytes([2; 16]),
            root,
            NormalizedName::new("README.md").expect("name"),
            binary(2, b"approved\n"),
        )
        .with_file(
            ObjectId::from_bytes([3; 16]),
            root,
            NormalizedName::new(".gitignore").expect("name"),
            binary(3, b"generated.bin\n"),
        )
        .with_file(
            ObjectId::from_bytes([4; 16]),
            root,
            NormalizedName::new("generated.bin").expect("name"),
            binary(4, b"approved ignored bytes\n"),
        )
        .with_file(
            ObjectId::from_bytes([5; 16]),
            src,
            NormalizedName::new("main.rs").expect("name"),
            binary(5, b"fn main() {}\n"),
        );
    let bundle = compute_bundle(&BundleRequest::new(
        canonical.clone(),
        canonical,
        HeadId::from_bytes([1; 32]),
        actor_state.clone(),
        HeadId::from_bytes([5; 32]),
        author,
    ))
    .expect("bundle");
    let context = HumanApprovalContext::from_bundle(
        ApprovalWorkspaceId::from_bytes([4; 16]),
        PolicyEpoch::new(1),
        &bundle,
    )
    .expect("context");
    let expected = ExpectedHumanApproval::new(context, credential, [9; 32]);
    let receipt = decode_hex(
        "9178186d6573682e76312e617070726f76616c2d726563656970745004040404040404040404040404040404582001010101010101010101010101010101010101010101010101010101010101015820050505050505050505050505050505050505050505050505050505050505050558209002bb9fa2e88df4e06cd614ac7717eec19eaf906d31cf4487de4cc592e4cfcd5820d0c4487a2b215075980d5f9b8071e617be4d0daac0d5f1d4788475e88f5c51bb582077998f56427866f602c4413a49febdfcb41ac15806b092c2bddb56b7d72a4b3e58201c696897c3d942160b6f84763e833e51738d04f99cae4473605a870df7bf403d015820c8539497ae98e7864a1a29cd3d2dd7eaacb25557c4554f22f7f49da1d74f65c4656573323536584104a760824a7c71c898c6d9e0e4e9e8dc8d9b275812a235713155fb99dfadb6fbfff104c70a12ce9a6c19d6c60b17cf7d68d783ef11d9a744d83d8fe0276b83b38f706465762e6d6573682e6465736b746f706d757365722d70726573656e63655820090909090909090909090909090909090909090909090909090909090909090967617070726f766558483046022100fa82ba312a3edf87853d1ac083ed7d4190859c0ebaafecbe14083fff07f057a6022100c76736c7d1ddc9c126d95cee2e67c84d89b4c7b6e7e2ca1c00c6292b697710cd",
    );
    Approval {
        expected,
        receipt,
        agent,
        bundle,
        state: actor_state,
    }
}

#[test]
fn exports_exact_tree_trailers_and_resolvable_receipt_without_touching_checkout() {
    let target = repository("happy-target");
    let anchor = GitProvenanceAnchor::inspect(&target.0).expect("anchor");
    let head_before = git(&target.0, &["rev-parse", "HEAD"]);
    let status_before = git(&target.0, &["status", "--porcelain=v2", "-z"]);
    let materialized = approved_materialization("happy-materialized");
    let (_expected_repository, expected_tree) = expected_tree();
    let approval = approval();

    let preview =
        preview_approved_git_export(&materialized.0, &target.0, &anchor, &approval.source())
            .expect("preview");
    assert_eq!(
        inspect_approved_git_export(&preview, &anchor, &approval.source())
            .expect("inspect absent export"),
        None,
    );
    assert_eq!(preview.state(), "05".repeat(32));
    assert_eq!(preview.tree(), expected_tree);
    assert_eq!(preview.actors(), &["07".repeat(32), "08".repeat(32)]);
    assert_eq!(
        preview.branch(),
        format!("mesh/approved/{}", "05".repeat(32))
    );

    let exported =
        confirm_approved_git_export(&preview, &anchor, &approval.source()).expect("confirm");
    assert!(!exported.already_present());
    assert_eq!(exported.tree(), preview.tree());
    assert_eq!(git(&target.0, &["rev-parse", "HEAD"]), head_before);
    assert_eq!(
        git(&target.0, &["status", "--porcelain=v2", "-z"]),
        status_before
    );
    assert_eq!(
        fs::read(target.0.join("README.md")).expect("checkout"),
        b"original\n"
    );

    let commit = String::from_utf8(git(
        &target.0,
        &["show", "-s", "--format=%B", exported.commit()],
    ))
    .expect("message");
    assert!(commit.contains(&format!("Mesh-Approval: {}", preview.approval())));
    assert!(commit.contains(&format!("Mesh-State: {}", preview.state())));
    assert!(commit.contains(&format!("Mesh-Actors: {}", preview.actors().join(","))));
    let author = String::from_utf8(git(
        &target.0,
        &["show", "-s", "--format=%an <%ae>", exported.commit()],
    ))
    .expect("author");
    assert_eq!(
        author.trim(),
        format!(
            "Mesh actor {} <actor-{}@mesh.invalid>",
            "07".repeat(6),
            "07".repeat(32)
        )
    );
    let names = git(
        &target.0,
        &["ls-tree", "-r", "--name-only", exported.commit()],
    );
    assert!(names
        .windows(b"generated.bin".len())
        .any(|window| window == b"generated.bin"));
    assert!(!names
        .windows(b".git/private".len())
        .any(|window| window == b".git/private"));
    assert_eq!(
        git(&target.0, &["cat-file", "blob", exported.approval_ref()]),
        approval.receipt
    );
    assert_eq!(
        String::from_utf8(git(
            &target.0,
            &["show", "-s", "--format=%P", exported.commit()]
        ))
        .expect("parent")
        .trim(),
        anchor.head().as_str()
    );

    let retry = confirm_approved_git_export(&preview, &anchor, &approval.source())
        .expect("idempotent retry");
    assert!(retry.already_present());
    assert_eq!(retry.commit(), exported.commit());
    let inspected = inspect_approved_git_export(&preview, &anchor, &approval.source())
        .expect("inspect installed export")
        .expect("installed export");
    assert!(inspected.already_present());
    assert_eq!(inspected, retry);

    let message = String::from_utf8(git(
        &target.0,
        &["show", "-s", "--format=%B", exported.commit()],
    ))
    .expect("export message");
    let substituted = forged_commit(
        &target.0,
        exported.tree(),
        anchor.head().as_str(),
        message.trim_end(),
    );
    git(
        &target.0,
        &[
            "update-ref",
            &format!("refs/heads/{}", exported.branch()),
            &substituted,
        ],
    );
    assert!(matches!(
        inspect_approved_git_export(&preview, &anchor, &approval.source()),
        Err(GitExportError::BranchConflict)
    ));
    assert!(matches!(
        confirm_approved_git_export(&preview, &anchor, &approval.source()),
        Err(GitExportError::BranchConflict)
    ));
}

#[test]
fn export_preserves_parent_modes_and_does_not_trust_unapproved_worktree_modes() {
    let target = repository("mode-target");
    fs::set_permissions(
        target.0.join("README.md"),
        fs::Permissions::from_mode(0o755),
    )
    .expect("make parent file executable");
    git(&target.0, &["add", "README.md"]);
    git(&target.0, &["commit", "-m", "approved parent mode"]);
    let anchor = GitProvenanceAnchor::inspect(&target.0).expect("anchor");
    let materialized = approved_materialization("mode-materialized");
    fs::set_permissions(
        materialized.0.join("README.md"),
        fs::Permissions::from_mode(0o644),
    )
    .expect("change existing materialized mode");
    fs::set_permissions(
        materialized.0.join("src/main.rs"),
        fs::Permissions::from_mode(0o755),
    )
    .expect("make new materialized file executable");
    let approval = approval();
    let preview =
        preview_approved_git_export(&materialized.0, &target.0, &anchor, &approval.source())
            .expect("preview");
    let exported =
        confirm_approved_git_export(&preview, &anchor, &approval.source()).expect("confirm");
    let modes = String::from_utf8(git(
        &target.0,
        &[
            "ls-tree",
            "-r",
            exported.commit(),
            "--",
            "README.md",
            "src/main.rs",
        ],
    ))
    .expect("tree modes");
    assert!(modes
        .lines()
        .any(|line| line.starts_with("100755 blob") && line.ends_with("\tREADME.md")));
    assert!(modes
        .lines()
        .any(|line| line.starts_with("100644 blob") && line.ends_with("\tsrc/main.rs")));
}

#[test]
fn refuses_changed_materialization_before_any_ref_is_created() {
    let target = repository("changed-source-target");
    let anchor = GitProvenanceAnchor::inspect(&target.0).expect("anchor");
    let materialized = approved_materialization("changed-source");
    let approval = approval();
    let preview =
        preview_approved_git_export(&materialized.0, &target.0, &anchor, &approval.source())
            .expect("preview");
    fs::write(materialized.0.join("README.md"), b"changed after preview\n").expect("mutate");
    assert!(matches!(
        confirm_approved_git_export(&preview, &anchor, &approval.source()),
        Err(GitExportError::MaterializationMismatch)
    ));
    let branch_ref = format!("refs/heads/{}", preview.branch());
    let status = Command::new("git")
        .arg("-C")
        .arg(&target.0)
        .args(["rev-parse", "--verify", &branch_ref])
        .output()
        .expect("probe ref")
        .status;
    assert!(!status.success());
}

#[test]
fn refuses_wrong_receipt_target_drift_and_unsupported_entries() {
    let target = repository("refusal-target");
    let anchor = GitProvenanceAnchor::inspect(&target.0).expect("anchor");
    let materialized = approved_materialization("refusal-materialized");
    let exact = approval();
    let mut other_receipt = exact.receipt.clone();
    *other_receipt.last_mut().expect("receipt byte") ^= 1;
    assert!(matches!(
        preview_approved_git_export(
            &materialized.0,
            &target.0,
            &anchor,
            &ApprovedGitExportSource::new(
                &other_receipt,
                &exact.expected,
                &exact.bundle,
                &exact.state,
                std::slice::from_ref(&exact.agent)
            )
        ),
        Err(GitExportError::ApprovalInvalid)
    ));

    let preview = preview_approved_git_export(&materialized.0, &target.0, &anchor, &exact.source())
        .expect("preview");
    fs::write(target.0.join("later.txt"), b"later\n").expect("later file");
    git(&target.0, &["add", "later.txt"]);
    git(&target.0, &["commit", "-m", "target moved"]);
    assert!(matches!(
        confirm_approved_git_export(&preview, &anchor, &exact.source()),
        Err(GitExportError::TargetChanged)
    ));

    let primary = repository("linked-primary");
    let linked = Scratch::new("linked-target");
    fs::remove_dir(&linked.0).expect("remove empty linked-worktree target");
    git(
        &primary.0,
        &[
            "worktree",
            "add",
            "-b",
            "linked-export-test",
            linked.0.to_str().expect("UTF-8 test path"),
        ],
    );
    let linked_anchor = GitProvenanceAnchor::inspect(&linked.0).expect("linked anchor");
    assert!(matches!(
        preview_approved_git_export(&materialized.0, &linked.0, &linked_anchor, &exact.source()),
        Err(GitExportError::LinkedWorktreeUnsupported)
    ));

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let target = repository("symlink-target");
        let anchor = GitProvenanceAnchor::inspect(&target.0).expect("anchor");
        let materialized = approved_materialization("symlink-materialized");
        symlink("README.md", materialized.0.join("alias")).expect("symlink");
        assert!(matches!(
            preview_approved_git_export(&materialized.0, &target.0, &anchor, &exact.source()),
            Err(GitExportError::UnsupportedEntry(_))
        ));
    }

    let target = repository("empty-target");
    let anchor = GitProvenanceAnchor::inspect(&target.0).expect("anchor");
    let materialized = approved_materialization("empty-materialized");
    fs::create_dir(materialized.0.join("empty")).expect("empty directory");
    assert!(matches!(
        preview_approved_git_export(&materialized.0, &target.0, &anchor, &exact.source()),
        Err(GitExportError::EmptyDirectory(_))
    ));
}

#[test]
fn refuses_state_and_ref_substitution_without_partial_publication() {
    let target = repository("substitution-target");
    let anchor = GitProvenanceAnchor::inspect(&target.0).expect("anchor");
    let materialized = approved_materialization("substitution-materialized");
    let exact = approval();
    let wrong_state = WorkspaceState::new(ObjectId::from_bytes([0; 16]));
    assert!(matches!(
        preview_approved_git_export(
            &materialized.0,
            &target.0,
            &anchor,
            &ApprovedGitExportSource::new(
                &exact.receipt,
                &exact.expected,
                &exact.bundle,
                &wrong_state,
                std::slice::from_ref(&exact.agent)
            )
        ),
        Err(GitExportError::StateInvalid)
    ));

    let preview = preview_approved_git_export(&materialized.0, &target.0, &anchor, &exact.source())
        .expect("preview");
    let approval_ref = format!("refs/mesh/approvals/{}", preview.approval());
    git(&target.0, &["update-ref", &approval_ref, "HEAD"]);
    assert!(matches!(
        confirm_approved_git_export(&preview, &anchor, &exact.source()),
        Err(GitExportError::ApprovalRefConflict)
    ));
    let branch_ref = format!("refs/heads/{}", preview.branch());
    let branch = Command::new("git")
        .arg("-C")
        .arg(&target.0)
        .args(["rev-parse", "--verify", &branch_ref])
        .output()
        .expect("branch probe");
    assert!(
        !branch.status.success(),
        "approval conflict must not create branch"
    );

    let target = repository("branch-conflict-target");
    let anchor = GitProvenanceAnchor::inspect(&target.0).expect("anchor");
    let preview = preview_approved_git_export(&materialized.0, &target.0, &anchor, &exact.source())
        .expect("preview");
    let branch_ref = format!("refs/heads/{}", preview.branch());
    git(&target.0, &["update-ref", &branch_ref, "HEAD"]);
    assert!(matches!(
        confirm_approved_git_export(&preview, &anchor, &exact.source()),
        Err(GitExportError::BranchConflict)
    ));
    assert!(matches!(
        inspect_approved_git_export(&preview, &anchor, &exact.source()),
        Err(GitExportError::BranchConflict)
    ));
    let approval_ref = format!("refs/mesh/approvals/{}", preview.approval());
    let receipt = Command::new("git")
        .arg("-C")
        .arg(&target.0)
        .args(["rev-parse", "--verify", &approval_ref])
        .output()
        .expect("receipt probe");
    assert!(
        !receipt.status.success(),
        "branch conflict must not create receipt ref"
    );

    let target = repository("directory-substitution-target");
    let anchor = GitProvenanceAnchor::inspect(&target.0).expect("anchor");
    let preview = preview_approved_git_export(&materialized.0, &target.0, &anchor, &exact.source())
        .expect("preview");
    let displaced = Scratch::new("directory-substitution-original");
    fs::remove_dir(&displaced.0).expect("remove empty displacement target");
    fs::rename(&target.0, &displaced.0).expect("displace original repository");
    let clone = Command::new("git")
        .args(["clone", "--no-local"])
        .arg(&displaced.0)
        .arg(&target.0)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .output()
        .expect("clone substituted repository");
    assert!(
        clone.status.success(),
        "clone substituted repository: {}",
        String::from_utf8_lossy(&clone.stderr)
    );
    assert!(matches!(
        confirm_approved_git_export(&preview, &anchor, &exact.source()),
        Err(GitExportError::TargetChanged)
    ));
    let branch_ref = format!("refs/heads/{}", preview.branch());
    for repository in [&target.0, &displaced.0] {
        let branch = Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(["rev-parse", "--verify", &branch_ref])
            .output()
            .expect("branch probe");
        assert!(
            !branch.status.success(),
            "directory substitution must not publish into either repository"
        );
    }
}
