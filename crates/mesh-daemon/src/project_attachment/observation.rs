//! Bounded, descriptor-confined live inventory. This never declares a saved or atomic version.

use super::{invalid, ProjectAttachment};
use crate::exclusions::EffectiveExclusions;
use crate::ipc::Json;
use crate::root_authority::PinnedRootFs;
use mesh_types::{Blake3, ContentDigest as _, Digest32, DigestHasher as _};
use std::fs::Metadata;
use std::io::{self, Read as _};
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

#[cfg(test)]
thread_local! {
    static BEFORE_RECHECK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = std::cell::RefCell::new(None);
}

/// Hard per-observation resource budgets, independent of the size of the user's project.
#[derive(Clone, Copy, Debug)]
pub struct ObservationLimits {
    /// Maximum admitted directory names, plus one overflow probe. Includes exclusions; at most 100,000.
    pub entries: usize,
    /// Content hashing bytes, plus one byte to detect growth. At most 1 GiB. Rule reads are separate.
    pub bytes: u64,
    /// Maximum regular-file size to hash. At most 64 MiB.
    pub file_bytes: u64,
}
impl ObservationLimits {
    pub(super) fn validate(self) -> io::Result<()> {
        if self.entries == 0
            || self.entries > 100_000
            || self.bytes == 0
            || self.bytes > 1024 * 1024 * 1024
            || self.file_bytes == 0
            || self.file_bytes > 64 * 1024 * 1024
        {
            return Err(invalid("invalid attachment observation budget"));
        }
        Ok(())
    }
}

impl Default for ObservationLimits {
    fn default() -> Self {
        Self {
            entries: 10_000,
            bytes: 64 * 1024 * 1024,
            file_bytes: 8 * 1024 * 1024,
        }
    }
}

/// Exact bytes admitted by capture, independent of later changes to the original file.
/// Content is deliberately omitted from Debug and from all observation JSON.
pub struct CapturedFileInput {
    path: PathBuf,
    identity: (u64, u64),
    bytes: Vec<u8>,
    digest: Digest32,
    executable: bool,
}
impl CapturedFileInput {
    /// Confined project-relative name at capture time.
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Device/inode evidence at capture time; not proof of authorship or identity across reuse.
    pub fn identity(&self) -> (u64, u64) {
        self.identity
    }
    /// Immutable captured content; consumers must not reopen the live path to recover these bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Digest of exactly the returned bytes.
    pub fn digest(&self) -> Digest32 {
        self.digest
    }
    /// Portable executable bit admitted alongside the bytes.
    pub fn executable(&self) -> bool {
        self.executable
    }
}
impl std::fmt::Debug for CapturedFileInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CapturedFileInput")
            .field("bytes", &self.bytes.len())
            .finish_non_exhaustive()
    }
}

/// Native input for an explicit durable version commit. This is not an acknowledgment of saved work.
/// Construction requires a complete traversal and recheck; fields cannot be supplied by a renderer.
pub struct CapturedProjectInput {
    root: PathBuf,
    identity: (u64, u64),
    directories: Vec<PathBuf>,
    files: Vec<CapturedFileInput>,
    exclusions: (Option<String>, Option<String>),
}
impl CapturedProjectInput {
    /// Original project location, retained as provenance rather than a source to reread.
    pub fn root(&self) -> &Path {
        &self.root
    }
    /// Native directory identity at admission, not a portable project identifier.
    pub fn identity(&self) -> (u64, u64) {
        self.identity
    }
    /// Captured directory names, including empty directories.
    pub fn directories(&self) -> &[PathBuf] {
        &self.directories
    }
    /// Exact immutable file inputs; source authorship remains unknown.
    pub fn files(&self) -> &[CapturedFileInput] {
        &self.files
    }
    /// Root ignore rules used for this input. A later policy change must not masquerade as deletion.
    pub fn exclusion_rules(&self) -> (Option<&str>, Option<&str>) {
        (self.exclusions.0.as_deref(), self.exclusions.1.as_deref())
    }
    /// Exact policy fingerprint, including the attachment's structural Git exclusion contract.
    pub fn exclusion_digest(&self) -> Digest32 {
        let policy = Json::object([
            ("schema", Json::text("mesh.attachment-exclusions/v1")),
            (
                "gitignore",
                self.exclusions
                    .0
                    .as_deref()
                    .map(Json::text)
                    .unwrap_or(Json::Null),
            ),
            (
                "meshignore",
                self.exclusions
                    .1
                    .as_deref()
                    .map(Json::text)
                    .unwrap_or(Json::Null),
            ),
            ("git_case_variants_excluded", Json::Bool(true)),
        ]);
        Blake3::digest_bytes(policy.encode().as_bytes())
    }
}
impl std::fmt::Debug for CapturedProjectInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CapturedProjectInput")
            .field("files", &self.files.len())
            .field("directories", &self.directories.len())
            .finish_non_exhaustive()
    }
}

struct Scan {
    report: Json,
    captured: CapturedProjectInput,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    device: u64,
    inode: u64,
    size: u64,
    mode: u32,
    mtime: i64,
    mtime_ns: i64,
    ctime: i64,
    ctime_ns: i64,
}
impl From<&Metadata> for Stamp {
    fn from(m: &Metadata) -> Self {
        Self {
            device: m.dev(),
            inode: m.ino(),
            size: m.len(),
            mode: m.mode(),
            mtime: m.mtime(),
            mtime_ns: m.mtime_nsec(),
            ctime: m.ctime(),
            ctime_ns: m.ctime_nsec(),
        }
    }
}

impl ProjectAttachment {
    /// Inspect live content without taking custody, writing the project, or attributing authorship.
    /// Complete means the bounded traversal and rechecks succeeded, not an atomic filesystem snapshot.
    pub fn observe(&self, limits: ObservationLimits) -> io::Result<Json> {
        Ok(self.scan(limits, false)?.report)
    }

    /// Capture exact bounded inputs without taking custody or modifying the user's project.
    /// Any incomplete scan refuses the whole input; no partial project can be passed to a commit.
    /// Returned bytes survive later source edits, but are not durable until a version writer commits.
    pub fn capture_inputs(&self, limits: ObservationLimits) -> io::Result<CapturedProjectInput> {
        let scan = self.scan(limits, true)?;
        if scan.report.get("complete") != Some(&Json::Bool(true)) {
            return Err(invalid("attachment capture is incomplete"));
        }
        Ok(scan.captured)
    }

    fn scan(&self, limits: ObservationLimits, retain_bytes: bool) -> io::Result<Scan> {
        limits.validate()?;
        self.ensure_current()?;
        let filesystem = self.pinned.filesystem();
        let root_before = Stamp::from(&self.pinned.try_clone_directory()?.metadata()?);
        let policy = rules(&filesystem)?;
        let exclusions = EffectiveExclusions::from_texts(
            Some(self.root.join(".gitignore")),
            policy.0.clone(),
            None,
            Some(self.root.join(".meshignore")),
            policy.1.clone(),
        )
        .map_err(|_| invalid("attachment exclusion rules unavailable"))?;
        let mut pending = vec![(PathBuf::new(), 0_usize, false)];
        let mut entries = 0_usize;
        let mut bytes = 0_u64;
        let mut excluded = 0_u64;
        let mut files = Vec::new();
        let mut directories = Vec::new();
        let mut issues = Vec::new();
        let mut stamps = Vec::new();
        let mut captured = CapturedProjectInput {
            root: self.root.clone(),
            identity: (self.device, self.inode),
            directories: Vec::new(),
            files: Vec::new(),
            exclusions: policy.clone(),
        };
        while let Some((directory, depth, excluded_parent)) = pending.pop() {
            let names = match filesystem
                .read_directory_names_bounded(&directory, limits.entries.saturating_sub(entries))
            {
                Ok(names) => names,
                Err(_) => {
                    issue(
                        &mut issues,
                        &directory,
                        "directory-unavailable-or-entry-limit",
                    );
                    break;
                }
            };
            entries += names.len();
            for name in names {
                let relative = directory.join(name);
                let Some(path) = relative.to_str() else {
                    issue(&mut issues, &directory, "unsupported-name");
                    continue;
                };
                // Structural Git metadata is never traversed, even under a negated ignore rule.
                if relative
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.eq_ignore_ascii_case(".git"))
                {
                    excluded += 1;
                    continue;
                }
                let included = exclusions
                    .versions_presented_path(path)
                    .map_err(|_| invalid("invalid observed project path"))?;
                if !included && !exclusions.has_reinclusion_rules() {
                    excluded += 1;
                    continue;
                }
                let mut file = match filesystem.inspect_entry(&relative) {
                    Ok(file) => file,
                    Err(_) => {
                        if included {
                            issue(&mut issues, &relative, "unsupported-or-unavailable-entry");
                        } else {
                            excluded += 1;
                        }
                        continue;
                    }
                };
                let metadata = match file.metadata() {
                    Ok(m) => m,
                    Err(_) => {
                        issue(&mut issues, &relative, "metadata-unavailable");
                        continue;
                    }
                };
                let stamp = Stamp::from(&metadata);
                stamps.push((relative.clone(), stamp.clone()));
                if included && excluded_parent {
                    issue(
                        &mut issues,
                        &relative,
                        "reincluded-under-excluded-directory",
                    );
                    continue;
                }
                if metadata.is_dir() {
                    if depth >= 64 {
                        issue(&mut issues, &relative, "depth-limit");
                        continue;
                    }
                    if included {
                        directories.push(Json::text(path));
                        if retain_bytes {
                            captured.directories.push(relative.clone());
                        }
                    } else {
                        excluded += 1;
                    }
                    pending.push((relative, depth + 1, excluded_parent || !included));
                    continue;
                }
                if !included {
                    excluded += 1;
                    continue;
                }
                if !metadata.is_file() {
                    issue(&mut issues, &relative, "unsupported-entry");
                    continue;
                }
                if bytes >= limits.bytes {
                    issue(&mut issues, &relative, "byte-limit");
                    continue;
                }
                let allowance = limits.file_bytes.min(limits.bytes.saturating_sub(bytes));
                if metadata.len() > allowance {
                    issue(&mut issues, &relative, "byte-limit");
                    continue;
                }
                let mut hasher = Blake3::hasher();
                let mut content = Vec::new();
                let mut total = 0_u64;
                let mut buffer = [0_u8; 65536];
                let mut read_error = false;
                while total <= allowance {
                    let take = buffer.len().min((allowance + 1 - total) as usize);
                    match file.read(&mut buffer[..take]) {
                        Ok(0) => break,
                        Ok(count) => {
                            total += count as u64;
                            hasher.update(&buffer[..count]);
                            if retain_bytes {
                                content.extend_from_slice(&buffer[..count]);
                            }
                        }
                        Err(_) => {
                            read_error = true;
                            break;
                        }
                    }
                }
                bytes = bytes.saturating_add(total);
                if read_error
                    || total > allowance
                    || total != metadata.len()
                    || file
                        .metadata()
                        .map(|m| Stamp::from(&m) != stamp)
                        .unwrap_or(true)
                {
                    issue(&mut issues, &relative, "file-changed-or-unavailable");
                    continue;
                }
                let digest = hasher.finalize();
                let executable = metadata.mode() & 0o111 != 0;
                if retain_bytes {
                    captured.files.push(CapturedFileInput {
                        path: relative.clone(),
                        identity: (stamp.device, stamp.inode),
                        bytes: content,
                        digest,
                        executable,
                    });
                }
                files.push(Json::object([
                    ("path", Json::text(path)),
                    ("bytes", Json::Number(total)),
                    ("digest", Json::text(digest.to_hex())),
                    ("executable", Json::Bool(executable)),
                    ("device", Json::text(format!("{:016x}", stamp.device))),
                    ("inode", Json::text(format!("{:016x}", stamp.inode))),
                    ("attribution", Json::text("unknown")),
                ]));
            }
        }
        #[cfg(test)]
        BEFORE_RECHECK.with(|hook| {
            if let Some(hook) = hook.borrow_mut().take() {
                hook();
            }
        });
        for (relative, before) in stamps {
            if filesystem
                .inspect_entry(&relative)
                .and_then(|file| file.metadata())
                .map(|m| Stamp::from(&m) != before)
                .unwrap_or(true)
            {
                issue(&mut issues, &relative, "entry-changed-during-observation");
            }
        }
        if Stamp::from(&self.pinned.try_clone_directory()?.metadata()?) != root_before {
            issue(
                &mut issues,
                Path::new(""),
                "root-changed-during-observation",
            );
        }
        if rules(&filesystem).ok().as_ref() != Some(&policy) {
            issue(
                &mut issues,
                Path::new(""),
                "exclusions-changed-during-observation",
            );
        }
        self.ensure_current()?;
        Ok(Scan {
            captured,
            report: Json::object([
                ("schema", Json::text("mesh.project-observation/v1")),
                ("complete", Json::Bool(issues.is_empty())),
                ("atomic_snapshot", Json::Bool(false)),
                ("saved_version", Json::Null),
                ("entries_listed", Json::Number(entries as u64)),
                ("bytes_read", Json::Number(bytes)),
                ("excluded_entries", Json::Number(excluded)),
                ("files", Json::Array(files)),
                ("directories", Json::Array(directories)),
                ("issues", Json::Array(issues)),
            ]),
        })
    }
}

fn issue(issues: &mut Vec<Json>, path: &Path, code: &str) {
    issues.push(Json::object([
        ("path", Json::text(path.to_string_lossy())),
        ("code", Json::text(code)),
    ]));
}

fn rules(filesystem: &PinnedRootFs) -> io::Result<(Option<String>, Option<String>)> {
    Ok((
        rule_file(filesystem, ".gitignore")?,
        rule_file(filesystem, ".meshignore")?,
    ))
}
fn rule_file(filesystem: &PinnedRootFs, name: &str) -> io::Result<Option<String>> {
    let file = match filesystem.inspect_entry(Path::new(name)) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    if !file.metadata()?.is_file() {
        return Err(invalid("attachment exclusion file is not regular"));
    }
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err(invalid("attachment exclusion file is too large"));
    }
    Ok(Some(String::from_utf8(bytes).map_err(|_| {
        invalid("attachment exclusion file is not UTF-8")
    })?))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changes_after_hashing_are_reported_as_incomplete() {
        let root =
            std::env::temp_dir().join(format!("mesh-observation-race-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let source = root.join("source");
        let metadata = root.join("metadata");
        std::fs::create_dir(&source).unwrap();
        std::fs::create_dir(&metadata).unwrap();
        std::fs::write(source.join("note.txt"), "before").unwrap();
        let attached = ProjectAttachment::register(&source, &metadata).unwrap();
        let target = source.join("note.txt");
        BEFORE_RECHECK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                std::fs::write(target, "concurrent edit after hashing").unwrap();
            }))
        });
        let observed = attached.observe(ObservationLimits::default()).unwrap();
        assert_eq!(observed.get("complete"), Some(&Json::Bool(false)));
        assert!(observed
            .get("issues")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue.get("code").and_then(Json::as_text)
                == Some("entry-changed-during-observation")));
        assert_eq!(
            std::fs::read_to_string(source.join("note.txt")).unwrap(),
            "concurrent edit after hashing"
        );
        let target = source.join("late-file.txt");
        BEFORE_RECHECK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                std::fs::write(target, "new concurrent file").unwrap();
            }))
        });
        assert!(attached
            .capture_inputs(ObservationLimits::default())
            .is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
