//! Exact, read-only Git repository inspection.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::isolated_git_command;

const ANCHOR_DOMAIN: &[u8] = b"mesh.git.provenance-anchor/v1\0";
const REPOSITORY_DOMAIN: &[u8] = b"mesh.git.repository-identity/v1\0";

/// Git's object-name algorithm for this repository.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitObjectFormat {
    /// The historical 160-bit Git object format.
    Sha1,
    /// Git's 256-bit object format.
    Sha256,
}

impl GitObjectFormat {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Sha1 => "sha1",
            Self::Sha256 => "sha256",
        }
    }

    const fn hexadecimal_length(self) -> usize {
        match self {
            Self::Sha1 => 40,
            Self::Sha256 => 64,
        }
    }
}

/// One full, validated Git object name.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct GitObjectId(String);

impl GitObjectId {
    /// Lowercase hexadecimal object name exactly as Git returned it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn parse(bytes: &[u8], format: GitObjectFormat) -> Result<Self, GitImportError> {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| GitImportError::InvalidOutput("Git returned a non-UTF-8 object name"))?;
        if text.len() != format.hexadecimal_length()
            || !text.bytes().all(|byte| byte.is_ascii_hexdigit())
            || text.bytes().any(|byte| byte.is_ascii_uppercase())
        {
            return Err(GitImportError::InvalidOutput(
                "Git returned a non-canonical object name",
            ));
        }
        Ok(Self(text.to_owned()))
    }
}

/// A repository-relative path represented as Git's uninterpreted bytes.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct GitPath(Vec<u8>);

impl GitPath {
    /// The exact path bytes emitted by Git, without C-style quoting or a trailing NUL.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    fn new(bytes: &[u8]) -> Result<Self, GitImportError> {
        if bytes.is_empty() || bytes.contains(&0) {
            return Err(GitImportError::InvalidOutput(
                "Git returned an empty or NUL-containing path",
            ));
        }
        Ok(Self(bytes.to_vec()))
    }
}

/// Stable, path-independent identity for the history containing the imported commit.
///
/// Every root commit reachable from `HEAD` participates. Two clones of the same history therefore
/// share an identity, while an unrelated repository at the same filesystem path does not.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryIdentity {
    object_format: GitObjectFormat,
    roots: Vec<GitObjectId>,
}

impl RepositoryIdentity {
    /// Repository object format.
    #[must_use]
    pub const fn object_format(&self) -> GitObjectFormat {
        self.object_format
    }

    /// Sorted, unique root commits reachable from the imported `HEAD`.
    #[must_use]
    pub fn roots(&self) -> &[GitObjectId] {
        &self.roots
    }

    /// Canonical, domain-separated bytes suitable for a Mesh content digest.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = REPOSITORY_DOMAIN.to_vec();
        field(&mut bytes, self.object_format.name().as_bytes());
        count(&mut bytes, self.roots.len());
        for root in &self.roots {
            field(&mut bytes, root.as_str().as_bytes());
        }
        bytes
    }
}

/// One submodule gitlink recorded in the repository index.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Gitlink {
    object: GitObjectId,
    stage: u8,
    path: GitPath,
}

impl Gitlink {
    /// Commit recorded by the gitlink index entry.
    #[must_use]
    pub const fn object(&self) -> &GitObjectId {
        &self.object
    }

    /// Git index stage. Ordinary checked-out gitlinks use stage zero; conflicts may expose more.
    #[must_use]
    pub const fn stage(&self) -> u8 {
        self.stage
    }

    /// Exact repository-relative path bytes.
    #[must_use]
    pub const fn path(&self) -> &GitPath {
        &self.path
    }
}

/// Exact Git state captured before a Mesh folder import.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitProvenanceAnchor {
    source_root: PathBuf,
    repository: RepositoryIdentity,
    head: GitObjectId,
    head_ref: Option<Vec<u8>>,
    status_porcelain_v2_z: Vec<u8>,
    ignored_paths: Vec<GitPath>,
    gitlinks: Vec<Gitlink>,
}

impl GitProvenanceAnchor {
    /// Inspect one exact repository root.
    ///
    /// This operation is read-only. It refuses bare repositories, subdirectory inputs, unborn
    /// `HEAD`, and shallow repositories because none can supply the complete import identity this
    /// version promises.
    ///
    /// # Errors
    ///
    /// Returns a typed failure when the path or any Git answer is unavailable or ambiguous.
    pub fn inspect(root: &Path) -> Result<Self, GitImportError> {
        let source_root = fs::canonicalize(root).map_err(|source| GitImportError::Io {
            context: "canonicalize repository root",
            source,
        })?;
        let metadata = fs::symlink_metadata(&source_root).map_err(|source| GitImportError::Io {
            context: "inspect repository root",
            source,
        })?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(GitImportError::NotRepositoryRoot);
        }

        expect_text(
            &source_root,
            &["rev-parse", "--is-inside-work-tree"],
            "true",
        )?;
        expect_text(
            &source_root,
            &["rev-parse", "--is-bare-repository"],
            "false",
        )?;
        let prefix = command(&source_root, &["rev-parse", "--show-prefix"])?;
        if trim_one_line(&prefix.stdout)? != b"" {
            return Err(GitImportError::NotRepositoryRoot);
        }
        expect_text(
            &source_root,
            &["rev-parse", "--is-shallow-repository"],
            "false",
        )
        .map_err(|error| match error {
            GitImportError::UnexpectedValue { .. } => GitImportError::ShallowRepository,
            other => other,
        })?;

        let object_format = match text_line(command(
            &source_root,
            &["rev-parse", "--show-object-format"],
        )?)?
        .as_str()
        {
            "sha1" => GitObjectFormat::Sha1,
            "sha256" => GitObjectFormat::Sha256,
            _ => {
                return Err(GitImportError::InvalidOutput(
                    "Git returned an unsupported object format",
                ))
            }
        };
        let head = GitObjectId::parse(
            trim_one_line(
                &command(&source_root, &["rev-parse", "--verify", "HEAD^{commit}"])?.stdout,
            )?,
            object_format,
        )?;

        let mut roots =
            lines(&command(&source_root, &["rev-list", "--max-parents=0", "HEAD"])?.stdout)?
                .into_iter()
                .map(|line| GitObjectId::parse(line, object_format))
                .collect::<Result<Vec<_>, _>>()?;
        roots.sort();
        roots.dedup();
        if roots.is_empty() {
            return Err(GitImportError::InvalidOutput(
                "Git returned no reachable root commit",
            ));
        }

        let head_ref = optional_command(&source_root, &["symbolic-ref", "-q", "HEAD"])?
            .map(|output| trim_one_line(&output.stdout).map(ToOwned::to_owned))
            .transpose()?;
        if head_ref.as_ref().is_some_and(Vec::is_empty) {
            return Err(GitImportError::InvalidOutput(
                "Git returned an empty symbolic HEAD",
            ));
        }

        let status_porcelain_v2_z = command(
            &source_root,
            &[
                "status",
                "--porcelain=v2",
                "--branch",
                "--show-stash",
                "--untracked-files=all",
                "--ignored=matching",
                "-z",
            ],
        )?
        .stdout;
        let ignored_paths = ignored_paths(&status_porcelain_v2_z)?;
        let gitlinks = parse_gitlinks(
            &command(&source_root, &["ls-files", "--stage", "-z"])?.stdout,
            object_format,
        )?;

        Ok(Self {
            source_root,
            repository: RepositoryIdentity {
                object_format,
                roots,
            },
            head,
            head_ref,
            status_porcelain_v2_z,
            ignored_paths,
            gitlinks,
        })
    }

    /// Build the installed anchor from status read through the retained Git/worktree descriptors.
    ///
    /// The caller has already copied and verified the expected repository identity and commit,
    /// and supplies the symbolic-or-detached HEAD observed through the exact pinned `.git`
    /// object. Keeping construction here lets the same closed status parser derive ignored paths
    /// without reopening a replaceable workspace name.
    pub(crate) fn from_verified_install(
        source_root: PathBuf,
        expected: &Self,
        observed_head_ref: Option<Vec<u8>>,
        status_porcelain_v2_z: Vec<u8>,
    ) -> Result<Self, GitImportError> {
        let ignored_paths = ignored_paths(&status_porcelain_v2_z)?;
        Ok(Self {
            source_root,
            repository: expected.repository.clone(),
            head: expected.head.clone(),
            head_ref: observed_head_ref,
            status_porcelain_v2_z,
            ignored_paths,
            gitlinks: Vec::new(),
        })
    }

    /// Canonical physical repository root used for this read. This path is not serialized into
    /// the portable provenance identity.
    #[must_use]
    pub fn source_root(&self) -> &Path {
        &self.source_root
    }

    /// Path-independent repository-history identity.
    #[must_use]
    pub const fn repository(&self) -> &RepositoryIdentity {
        &self.repository
    }

    /// Commit checked out at `HEAD` during inspection.
    #[must_use]
    pub const fn head(&self) -> &GitObjectId {
        &self.head
    }

    /// Full symbolic ref bytes, or `None` for a detached `HEAD`.
    #[must_use]
    pub fn head_ref(&self) -> Option<&[u8]> {
        self.head_ref.as_deref()
    }

    /// Git porcelain-v2 `-z` status bytes, including branch headers and every tracked, untracked,
    /// conflicted, and matching ignored entry.
    #[must_use]
    pub fn status_porcelain_v2_z(&self) -> &[u8] {
        &self.status_porcelain_v2_z
    }

    /// Ignored paths extracted from the exact status stream without decoding or normalizing them.
    #[must_use]
    pub fn ignored_paths(&self) -> &[GitPath] {
        &self.ignored_paths
    }

    /// Every stage of every submodule gitlink in the index.
    #[must_use]
    pub fn gitlinks(&self) -> &[Gitlink] {
        &self.gitlinks
    }

    /// Canonical, domain-separated representation of the complete anchor.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = ANCHOR_DOMAIN.to_vec();
        field(&mut bytes, &self.repository.canonical_bytes());
        field(&mut bytes, self.head.as_str().as_bytes());
        optional_field(&mut bytes, self.head_ref.as_deref());
        field(&mut bytes, &self.status_porcelain_v2_z);
        count(&mut bytes, self.ignored_paths.len());
        for path in &self.ignored_paths {
            field(&mut bytes, path.as_bytes());
        }
        count(&mut bytes, self.gitlinks.len());
        for gitlink in &self.gitlinks {
            field(&mut bytes, gitlink.object.as_str().as_bytes());
            bytes.push(gitlink.stage);
            field(&mut bytes, gitlink.path.as_bytes());
        }
        bytes
    }
}

fn command(root: &Path, arguments: &[&str]) -> Result<Output, GitImportError> {
    let output = configured_command(root, arguments)
        .output()
        .map_err(|source| GitImportError::Io {
            context: "run Git inspection",
            source,
        })?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(GitImportError::Git {
            arguments: arguments
                .iter()
                .map(|argument| (*argument).to_owned())
                .collect(),
            status: output.status.code(),
            stderr: bounded_stderr(&output.stderr),
        })
    }
}

fn optional_command(root: &Path, arguments: &[&str]) -> Result<Option<Output>, GitImportError> {
    let output = configured_command(root, arguments)
        .output()
        .map_err(|source| GitImportError::Io {
            context: "run optional Git inspection",
            source,
        })?;
    if output.status.success() {
        Ok(Some(output))
    } else if output.status.code() == Some(1) && output.stderr.is_empty() {
        Ok(None)
    } else {
        Err(GitImportError::Git {
            arguments: arguments
                .iter()
                .map(|argument| (*argument).to_owned())
                .collect(),
            status: output.status.code(),
            stderr: bounded_stderr(&output.stderr),
        })
    }
}

fn configured_command(root: &Path, arguments: &[&str]) -> Command {
    let mut command = isolated_git_command();
    command
        .arg("-c")
        .arg("core.fsmonitor=false")
        .arg("-c")
        .arg("core.untrackedCache=false")
        .arg("-c")
        .arg("core.hooksPath=/dev/null")
        .arg("-C")
        .arg(root)
        .args(arguments)
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

fn expect_text(
    root: &Path,
    arguments: &[&str],
    expected: &'static str,
) -> Result<(), GitImportError> {
    let actual = text_line(command(root, arguments)?)?;
    if actual == expected {
        Ok(())
    } else {
        Err(GitImportError::UnexpectedValue { expected, actual })
    }
}

fn text_line(output: Output) -> Result<String, GitImportError> {
    let bytes = trim_one_line(&output.stdout)?;
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| GitImportError::InvalidOutput("Git returned non-UTF-8 control output"))
}

fn trim_one_line(bytes: &[u8]) -> Result<&[u8], GitImportError> {
    let trimmed = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    let trimmed = trimmed.strip_suffix(b"\r").unwrap_or(trimmed);
    if trimmed.contains(&b'\n') || trimmed.contains(&b'\r') {
        return Err(GitImportError::InvalidOutput(
            "Git returned multiple control-output lines",
        ));
    }
    Ok(trimmed)
}

fn lines(bytes: &[u8]) -> Result<Vec<&[u8]>, GitImportError> {
    let mut result = Vec::new();
    for line in bytes.split(|byte| *byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        if line.contains(&b'\r') {
            return Err(GitImportError::InvalidOutput(
                "Git returned malformed line endings",
            ));
        }
        result.push(line);
    }
    Ok(result)
}

fn ignored_paths(status: &[u8]) -> Result<Vec<GitPath>, GitImportError> {
    let mut ignored = status
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .filter_map(|record| record.strip_prefix(b"! "))
        .map(GitPath::new)
        .collect::<Result<Vec<_>, _>>()?;
    ignored.sort();
    ignored.dedup();
    Ok(ignored)
}

fn parse_gitlinks(bytes: &[u8], format: GitObjectFormat) -> Result<Vec<Gitlink>, GitImportError> {
    let mut links = Vec::new();
    for record in bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let Some(tab) = record.iter().position(|byte| *byte == b'\t') else {
            return Err(GitImportError::InvalidOutput(
                "Git returned a malformed index entry",
            ));
        };
        let (metadata, path_with_tab) = record.split_at(tab);
        let path = &path_with_tab[1..];
        let mut fields = metadata.split(|byte| *byte == b' ');
        let mode = fields.next().unwrap_or_default();
        let object = fields.next().unwrap_or_default();
        let stage = fields.next().unwrap_or_default();
        if fields.next().is_some() || mode.is_empty() || object.is_empty() || stage.is_empty() {
            return Err(GitImportError::InvalidOutput(
                "Git returned a malformed index entry",
            ));
        }
        if mode != b"160000" {
            continue;
        }
        let stage = std::str::from_utf8(stage)
            .ok()
            .and_then(|stage| stage.parse::<u8>().ok())
            .filter(|stage| *stage <= 3)
            .ok_or(GitImportError::InvalidOutput(
                "Git returned an invalid index stage",
            ))?;
        links.push(Gitlink {
            object: GitObjectId::parse(object, format)?,
            stage,
            path: GitPath::new(path)?,
        });
    }
    links.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.stage.cmp(&right.stage))
            .then(left.object.cmp(&right.object))
    });
    Ok(links)
}

fn count(bytes: &mut Vec<u8>, value: usize) {
    let value = u32::try_from(value).expect("Git anchor count exceeds canonical u32 bound");
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn field(bytes: &mut Vec<u8>, value: &[u8]) {
    let length = u64::try_from(value.len()).expect("Git anchor field exceeds canonical u64 bound");
    bytes.extend_from_slice(&length.to_be_bytes());
    bytes.extend_from_slice(value);
}

fn optional_field(bytes: &mut Vec<u8>, value: Option<&[u8]>) {
    match value {
        Some(value) => {
            bytes.push(1);
            field(bytes, value);
        }
        None => bytes.push(0),
    }
}

fn bounded_stderr(stderr: &[u8]) -> String {
    const MAX: usize = 4_096;
    String::from_utf8_lossy(&stderr[..stderr.len().min(MAX)])
        .trim()
        .to_owned()
}

/// Why exact Git import inspection could not produce an anchor.
#[derive(Debug)]
pub enum GitImportError {
    /// Filesystem access failed.
    Io {
        /// Operation that failed.
        context: &'static str,
        /// Operating-system error.
        source: std::io::Error,
    },
    /// The supplied path is not exactly the repository's working-tree root.
    NotRepositoryRoot,
    /// A shallow checkout cannot prove the complete reachable-history identity.
    ShallowRepository,
    /// Git rejected one read-only query.
    Git {
        /// Git arguments, kept separate from shell syntax.
        arguments: Vec<String>,
        /// Process exit status when available.
        status: Option<i32>,
        /// Bounded diagnostic text.
        stderr: String,
    },
    /// Git returned a value different from the required repository shape.
    UnexpectedValue {
        /// Required value.
        expected: &'static str,
        /// Returned value.
        actual: String,
    },
    /// Git returned malformed or unsupported bytes.
    InvalidOutput(&'static str),
}

impl fmt::Display for GitImportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { context, source } => write!(formatter, "{context}: {source}"),
            Self::NotRepositoryRoot => formatter.write_str(
                "the selected folder is not exactly the root of a non-bare Git working tree",
            ),
            Self::ShallowRepository => formatter.write_str(
                "the selected Git repository is shallow, so its complete history identity is unavailable",
            ),
            Self::Git {
                arguments,
                status,
                stderr,
            } => write!(
                formatter,
                "Git inspection {:?} failed with status {status:?}: {stderr}",
                arguments
            ),
            Self::UnexpectedValue { expected, actual } => {
                write!(formatter, "Git returned {actual:?}; expected {expected:?}")
            }
            Self::InvalidOutput(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for GitImportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
