//! Private native-host memory of the managed workspaces the person opened recently.
//!
//! This is navigation state, never workspace authority. The daemon still reopens and validates
//! the durable journal before the UI may show anything. A malformed, linked, shared-permission,
//! relative, or oversized record is refused instead of becoming a path the host follows.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::os::fd::AsRawFd as _;
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use mesh_daemon::{ipc::Json, ProtectedWorkspaceRoot};

const RECORD_NAME: &str = "recent-workspace.json";
const RECORD_LOCK_NAME: &str = ".recent-workspace.lock";
const SCHEMA_V1: &str = "mesh-desktop-recent-workspace/v1";
const SCHEMA_V2: &str = "mesh-desktop-recent-workspaces/v2";
const SCHEMA_V3: &str = "mesh-desktop-recent-workspaces/v3";
const SCHEMA_V4: &str = "mesh-desktop-recent-workspaces/v4";
const SCHEMA_V5: &str = "mesh-desktop-recent-workspaces/v5";
const SCHEMA_V6: &str = "mesh-desktop-recent-workspaces/v6";
const SCHEMA_V7: &str = "mesh-desktop-recent-workspaces/v7";
const SCHEMA_V8: &str = "mesh-desktop-recent-workspaces/v8";
const SCHEMA_V9: &str = "mesh-desktop-recent-workspaces/v9";
const MAX_RECENT_WORKSPACES: usize = 8;
const MAX_RECORD_BYTES: u64 = 16 * 1024;
static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(1);

/// The native-host-owned record beside the application's other local data.
#[derive(Clone, Debug)]
pub struct RecentWorkspace {
    directory: PathBuf,
    record: PathBuf,
}

#[derive(Debug)]
struct RecentWorkspaceLock {
    _file: File,
}

/// Process-shared exclusion held while one native managed-workspace mutation runs.
///
/// Dropping the guard releases the operating-system lock, including during unwinding. Callers must
/// acquire this guard before entering any daemon workspace/checkpoint/managed-edit lock. Code that
/// already holds a daemon mutation lock must never attempt to acquire the recent-workspace lock.
#[derive(Debug)]
pub struct ManagedWorkspaceMutationGuard {
    _lock: RecentWorkspaceLock,
}

/// One workspace navigation hint and its optional ordinary export destination.
///
/// Neither path grants filesystem authority. The daemon re-verifies the workspace before use and
/// export separately previews and pins the destination before any write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentWorkspaceEntry {
    path: PathBuf,
    export_root: Option<PathBuf>,
    project_root: Option<PathBuf>,
    agent_handoff_installation: Option<String>,
    agent_handoff_directory: Option<String>,
    agent_handoff_generation: Option<String>,
    source_point_ordinal: Option<u64>,
    original_update_version: Option<String>,
}

impl RecentWorkspaceEntry {
    /// The remembered managed workspace.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The ordinary folder last associated with this workspace for explicit export.
    #[must_use]
    pub fn export_root(&self) -> Option<&Path> {
        self.export_root.as_deref()
    }

    /// The first ordinary project folder associated with this workspace family.
    ///
    /// Unlike [`Self::export_root`], this display hint does not change when the person pulls a
    /// saved version back to another destination.
    #[must_use]
    pub fn project_root(&self) -> Option<&Path> {
        self.project_root.as_deref()
    }

    /// The exact physical workspace installation that may already be open in an agent.
    ///
    /// This is a conservative navigation warning, never authority or proof that a process is
    /// still running. Binding the hint to the installation prevents a replacement directory or
    /// another saved version from inheriting it merely because a pathname was reused.
    #[must_use]
    pub fn agent_handoff_installation(&self) -> Option<&str> {
        self.agent_handoff_installation.as_deref()
    }

    /// The exact real directory object handed to the agent, independent of later renames.
    #[must_use]
    pub fn agent_handoff_directory(&self) -> Option<&str> {
        self.agent_handoff_directory.as_deref()
    }

    /// Opaque identity of the most recently acquired agent custody for this workspace.
    ///
    /// The value is retained after release so a stale renderer cannot clear a later reacquisition
    /// that happens to use the same physical workspace installation and directory.
    #[must_use]
    pub fn agent_handoff_generation(&self) -> Option<&str> {
        self.agent_handoff_generation.as_deref()
    }

    /// One-based point in the source workspace from which this independent copy was opened.
    ///
    /// This is a presentation hint, never workspace authority. The daemon still verifies the
    /// destination and its durable history before it can be selected.
    #[must_use]
    pub const fn source_point_ordinal(&self) -> Option<u64> {
        self.source_point_ordinal
    }

    /// The shared version last fully reconciled to this workspace family's original folder.
    ///
    /// This is a navigation reminder, never export authority. Every later update still requires
    /// a fresh exact preview and explicit confirmation against the ordinary folder.
    #[must_use]
    pub fn original_update_version(&self) -> Option<&str> {
        self.original_update_version.as_deref()
    }
}

impl RecentWorkspace {
    /// Point at one application data directory without reading or creating it yet.
    #[must_use]
    pub fn new(directory: PathBuf) -> Self {
        let record = directory.join(RECORD_NAME);
        Self { directory, record }
    }

    /// The private application-data directory that owns this navigation record.
    ///
    /// Exposing the path does not grant authority; callers use it only to prevent a user-selected
    /// export destination from writing review copies into Mesh's private state.
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Serialize every read-modify-publish transaction across application processes.
    ///
    /// The durable agent-custody marker is a safety boundary: two Mesh processes must not both
    /// observe an unassigned workspace and launch into the same writable directory. `flock` is
    /// released by the operating system if a process exits, so a crash cannot strand the record.
    fn acquire_lock(&self) -> Result<RecentWorkspaceLock, RecentWorkspaceError> {
        fs::create_dir_all(&self.directory)
            .map_err(|error| RecentWorkspaceError::io("create application data", error))?;
        let directory_metadata = fs::symlink_metadata(&self.directory)
            .map_err(|error| RecentWorkspaceError::io("inspect application data", error))?;
        if !directory_metadata.file_type().is_dir() || directory_metadata.file_type().is_symlink() {
            return Err(RecentWorkspaceError::Invalid(
                "application data is not a real directory",
            ));
        }
        fs::set_permissions(&self.directory, fs::Permissions::from_mode(0o700))
            .map_err(|error| RecentWorkspaceError::io("secure application data", error))?;
        validate_private_directory(&self.directory)?;

        let path = self.directory.join(RECORD_LOCK_NAME);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&path)
            .map_err(|error| RecentWorkspaceError::io("open recent workspace lock", error))?;
        let named = fs::symlink_metadata(&path)
            .map_err(|error| RecentWorkspaceError::io("inspect recent workspace lock", error))?;
        let opened = file.metadata().map_err(|error| {
            RecentWorkspaceError::io("inspect opened recent workspace lock", error)
        })?;
        if !named.file_type().is_file()
            || named.file_type().is_symlink()
            || !opened.is_file()
            || named.dev() != opened.dev()
            || named.ino() != opened.ino()
            || opened.permissions().mode() & 0o077 != 0
        {
            return Err(RecentWorkspaceError::Invalid(
                "recent workspace lock is not a stable owner-only file",
            ));
        }
        lock_exclusive(&file)?;
        Ok(RecentWorkspaceLock { _file: file })
    }

    /// Exclude agent-custody acquisition while one exact managed working-copy mutation runs.
    ///
    /// The renderer's custody projection is only a usability hint and can become stale when a
    /// second Mesh process assigns the same physical workspace to an agent. This guard reloads the
    /// shared record while holding its exclusive file lock and refuses an active assignment before
    /// the caller enters the daemon mutation. Agent acquisition uses this same lock, so either the
    /// mutation completes before custody is published or custody wins and the mutation never runs.
    ///
    /// Global native lock order: recent-workspace record, then daemon workspace/checkpoint/managed
    /// mutation locks. Keeping that order prevents a process waiting on custody while holding a
    /// daemon mutation lock needed by the process currently holding this guard.
    pub fn lock_managed_workspace_mutation(
        &self,
        path: &Path,
        installation: &str,
    ) -> Result<ManagedWorkspaceMutationGuard, RecentWorkspaceError> {
        if installation.is_empty() {
            return Err(RecentWorkspaceError::Invalid(
                "managed workspace installation is empty",
            ));
        }
        let lock = self.acquire_lock()?;
        let canonical = canonical_workspace(path)?;
        let directory = ProtectedWorkspaceRoot::inspect(&canonical)
            .map_err(|error| RecentWorkspaceError::io("inspect managed workspace", error))?;
        let entries = self.load_entries()?;
        for entry in entries
            .iter()
            .filter(|entry| entry.agent_handoff_installation.is_some())
        {
            if entry.path == canonical || active_custody_matches_directory(entry, &directory)? {
                return Err(RecentWorkspaceError::Invalid(
                    "workspace is assigned to an agent; finish the exact handoff before changing its managed working copy",
                ));
            }
        }
        Ok(ManagedWorkspaceMutationGuard { _lock: lock })
    }

    /// Read the bounded newest-first workspace paths.
    #[cfg(test)]
    pub fn load_all(&self) -> Result<Vec<PathBuf>, RecentWorkspaceError> {
        self.load_entries()
            .map(|entries| entries.into_iter().map(|entry| entry.path).collect())
    }

    fn read_record_bytes(&self) -> Result<Option<Vec<u8>>, RecentWorkspaceError> {
        let metadata = match fs::symlink_metadata(&self.record) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(RecentWorkspaceError::io("read record metadata", error)),
        };
        validate_private_directory(&self.directory)?;
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            return Err(RecentWorkspaceError::Invalid(
                "record is not a regular file",
            ));
        }
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(RecentWorkspaceError::Invalid("record is not owner-only"));
        }
        if metadata.len() > MAX_RECORD_BYTES {
            return Err(RecentWorkspaceError::InvalidContent("record is too large"));
        }
        let file = File::open(&self.record)
            .map_err(|error| RecentWorkspaceError::io("open record", error))?;
        let opened = file
            .metadata()
            .map_err(|error| RecentWorkspaceError::io("inspect opened record", error))?;
        if !opened.is_file()
            || opened.dev() != metadata.dev()
            || opened.ino() != metadata.ino()
            || opened.permissions().mode() & 0o077 != 0
        {
            return Err(RecentWorkspaceError::Invalid(
                "record changed while it was opened",
            ));
        }
        let mut bytes = Vec::new();
        file.take(MAX_RECORD_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| RecentWorkspaceError::io("read record", error))?;
        if bytes.len() as u64 > MAX_RECORD_BYTES {
            return Err(RecentWorkspaceError::Invalid(
                "record grew while it was read",
            ));
        }
        Ok(Some(bytes))
    }

    /// Read the bounded newest-first navigation history and optional export hints.
    ///
    /// The v1 single-path and v2 path-list shapes remain readable and are upgraded on the next
    /// successful write. A remembered folder or export destination may itself be missing: both
    /// are navigation hints, and their respective daemon operations perform fresh verification.
    pub fn load_entries(&self) -> Result<Vec<RecentWorkspaceEntry>, RecentWorkspaceError> {
        let Some(bytes) = self.read_record_bytes()? else {
            return Ok(Vec::new());
        };
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| RecentWorkspaceError::InvalidContent("record is not UTF-8"))?;
        let parsed = Json::parse(text)
            .map_err(|_| RecentWorkspaceError::InvalidContent("record is not canonical JSON"))?;
        let entries = match parsed.get("schema").and_then(Json::as_text) {
            Some(SCHEMA_V1) => {
                let Some(path) = parsed.get("path").and_then(Json::as_text) else {
                    return Err(RecentWorkspaceError::InvalidContent("record has no path"));
                };
                let expected = Json::object([
                    ("schema", Json::text(SCHEMA_V1)),
                    ("path", Json::text(path)),
                ]);
                if parsed != expected || text != expected.encode() {
                    return Err(RecentWorkspaceError::InvalidContent(
                        "record shape is not canonical",
                    ));
                }
                vec![RecentWorkspaceEntry {
                    path: PathBuf::from(path),
                    export_root: None,
                    project_root: None,
                    agent_handoff_installation: None,
                    agent_handoff_directory: None,
                    agent_handoff_generation: None,
                    source_point_ordinal: None,
                    original_update_version: None,
                }]
            }
            Some(SCHEMA_V2) => {
                let Some(values) = parsed.get("paths").and_then(Json::as_array) else {
                    return Err(RecentWorkspaceError::InvalidContent(
                        "record has no path list",
                    ));
                };
                if values.is_empty() || values.len() > MAX_RECENT_WORKSPACES {
                    return Err(RecentWorkspaceError::InvalidContent(
                        "record path count is outside the supported range",
                    ));
                }
                let mut paths = Vec::with_capacity(values.len());
                let mut encoded_paths = Vec::with_capacity(values.len());
                for value in values {
                    let Some(path) = value.as_text() else {
                        return Err(RecentWorkspaceError::InvalidContent(
                            "record path is not text",
                        ));
                    };
                    let path = PathBuf::from(path);
                    if !path.is_absolute() {
                        return Err(RecentWorkspaceError::InvalidContent(
                            "remembered path is not absolute",
                        ));
                    }
                    if paths.contains(&path) {
                        return Err(RecentWorkspaceError::InvalidContent(
                            "record repeats a workspace path",
                        ));
                    }
                    encoded_paths.push(Json::text(path.to_string_lossy()));
                    paths.push(path);
                }
                let expected = Json::object([
                    ("schema", Json::text(SCHEMA_V2)),
                    ("paths", Json::Array(encoded_paths)),
                ]);
                if parsed != expected || text != expected.encode() {
                    return Err(RecentWorkspaceError::InvalidContent(
                        "record shape is not canonical",
                    ));
                }
                paths
                    .into_iter()
                    .map(|path| RecentWorkspaceEntry {
                        path,
                        export_root: None,
                        project_root: None,
                        agent_handoff_installation: None,
                        agent_handoff_directory: None,
                        agent_handoff_generation: None,
                        source_point_ordinal: None,
                        original_update_version: None,
                    })
                    .collect()
            }
            Some(SCHEMA_V3) | Some(SCHEMA_V4) | Some(SCHEMA_V5) | Some(SCHEMA_V6)
            | Some(SCHEMA_V7) | Some(SCHEMA_V8) | Some(SCHEMA_V9) => {
                let schema = parsed.get("schema").and_then(Json::as_text);
                let has_project_root = matches!(
                    schema,
                    Some(SCHEMA_V4)
                        | Some(SCHEMA_V5)
                        | Some(SCHEMA_V6)
                        | Some(SCHEMA_V7)
                        | Some(SCHEMA_V8)
                        | Some(SCHEMA_V9)
                );
                let has_agent_handoff = matches!(
                    schema,
                    Some(SCHEMA_V5)
                        | Some(SCHEMA_V6)
                        | Some(SCHEMA_V7)
                        | Some(SCHEMA_V8)
                        | Some(SCHEMA_V9)
                );
                let has_source_point_ordinal = matches!(
                    schema,
                    Some(SCHEMA_V6) | Some(SCHEMA_V7) | Some(SCHEMA_V8) | Some(SCHEMA_V9)
                );
                let has_original_update_version =
                    matches!(schema, Some(SCHEMA_V7) | Some(SCHEMA_V8) | Some(SCHEMA_V9));
                let has_agent_handoff_directory =
                    matches!(schema, Some(SCHEMA_V8) | Some(SCHEMA_V9));
                let has_agent_handoff_generation = schema == Some(SCHEMA_V9);
                let Some(values) = parsed.get("workspaces").and_then(Json::as_array) else {
                    return Err(RecentWorkspaceError::InvalidContent(
                        "record has no workspace list",
                    ));
                };
                if values.is_empty() || values.len() > MAX_RECENT_WORKSPACES {
                    return Err(RecentWorkspaceError::InvalidContent(
                        "record workspace count is outside the supported range",
                    ));
                }
                let mut entries = Vec::with_capacity(values.len());
                let mut encoded_entries = Vec::with_capacity(values.len());
                for value in values {
                    let Some(path) = value.get("path").and_then(Json::as_text) else {
                        return Err(RecentWorkspaceError::InvalidContent(
                            "record workspace path is not text",
                        ));
                    };
                    let path = PathBuf::from(path);
                    validate_absolute_utf8_hint(&path, "remembered path is not absolute")?;
                    if entries
                        .iter()
                        .any(|entry: &RecentWorkspaceEntry| entry.path == path)
                    {
                        return Err(RecentWorkspaceError::InvalidContent(
                            "record repeats a workspace path",
                        ));
                    }
                    let export_value =
                        value
                            .get("export_root")
                            .ok_or(RecentWorkspaceError::InvalidContent(
                                "record workspace has no export destination",
                            ))?;
                    let export_root = if export_value == &Json::Null {
                        None
                    } else {
                        let Some(export_root) = export_value.as_text() else {
                            return Err(RecentWorkspaceError::InvalidContent(
                                "record export destination is not text or null",
                            ));
                        };
                        let export_root = PathBuf::from(export_root);
                        validate_absolute_utf8_hint(
                            &export_root,
                            "remembered export destination is not absolute",
                        )?;
                        Some(export_root)
                    };
                    let project_root = if has_project_root {
                        let project_value = value.get("project_root").ok_or(
                            RecentWorkspaceError::InvalidContent(
                                "record workspace has no stable project folder",
                            ),
                        )?;
                        if project_value == &Json::Null {
                            None
                        } else {
                            let Some(project_root) = project_value.as_text() else {
                                return Err(RecentWorkspaceError::InvalidContent(
                                    "record stable project folder is not text or null",
                                ));
                            };
                            let project_root = PathBuf::from(project_root);
                            validate_absolute_utf8_hint(
                                &project_root,
                                "remembered stable project folder is not absolute",
                            )?;
                            Some(project_root)
                        }
                    } else {
                        // v3 had only the mutable pull-back destination. Preserve its last value
                        // as the initial stable label when upgrading; future destination changes
                        // no longer rename this workspace family.
                        export_root.clone()
                    };
                    let mut expected_fields = vec![
                        ("path", Json::text(path.to_string_lossy())),
                        (
                            "export_root",
                            export_root
                                .as_ref()
                                .map_or(Json::Null, |root| Json::text(root.to_string_lossy())),
                        ),
                    ];
                    if has_project_root {
                        expected_fields.push((
                            "project_root",
                            project_root
                                .as_ref()
                                .map_or(Json::Null, |root| Json::text(root.to_string_lossy())),
                        ));
                    }
                    let agent_handoff_installation = if has_agent_handoff {
                        let handoff_value = value.get("agent_handoff_installation").ok_or(
                            RecentWorkspaceError::InvalidContent(
                                "record workspace has no agent handoff installation",
                            ),
                        )?;
                        if handoff_value == &Json::Null {
                            None
                        } else {
                            let Some(installation) = handoff_value.as_text() else {
                                return Err(RecentWorkspaceError::InvalidContent(
                                    "record agent handoff installation is not text or null",
                                ));
                            };
                            if installation.is_empty() {
                                return Err(RecentWorkspaceError::InvalidContent(
                                    "record agent handoff installation is empty",
                                ));
                            }
                            Some(installation.to_owned())
                        }
                    } else {
                        None
                    };
                    if has_agent_handoff {
                        expected_fields.push((
                            "agent_handoff_installation",
                            agent_handoff_installation
                                .as_ref()
                                .map_or(Json::Null, Json::text),
                        ));
                    }
                    let agent_handoff_directory = if has_agent_handoff_directory {
                        let directory_value = value.get("agent_handoff_directory").ok_or(
                            RecentWorkspaceError::InvalidContent(
                                "record workspace has no agent handoff directory",
                            ),
                        )?;
                        if directory_value == &Json::Null {
                            None
                        } else {
                            let Some(directory) = directory_value.as_text() else {
                                return Err(RecentWorkspaceError::InvalidContent(
                                    "record agent handoff directory is not text or null",
                                ));
                            };
                            ProtectedWorkspaceRoot::from_directory_token(directory).map_err(
                                |_| {
                                    RecentWorkspaceError::InvalidContent(
                                        "record agent handoff directory is invalid",
                                    )
                                },
                            )?;
                            Some(directory.to_owned())
                        }
                    } else {
                        None
                    };
                    if has_agent_handoff_directory {
                        if agent_handoff_directory.is_some() != agent_handoff_installation.is_some()
                        {
                            return Err(RecentWorkspaceError::InvalidContent(
                                "record agent handoff identities disagree",
                            ));
                        }
                        expected_fields.push((
                            "agent_handoff_directory",
                            agent_handoff_directory
                                .as_ref()
                                .map_or(Json::Null, Json::text),
                        ));
                    }
                    let agent_handoff_generation = if has_agent_handoff_generation {
                        let generation_value = value.get("agent_handoff_generation").ok_or(
                            RecentWorkspaceError::InvalidContent(
                                "record workspace has no agent handoff generation",
                            ),
                        )?;
                        if generation_value == &Json::Null {
                            None
                        } else {
                            let generation = generation_value.as_text().ok_or(
                                RecentWorkspaceError::InvalidContent(
                                    "record agent handoff generation is not text or null",
                                ),
                            )?;
                            if !valid_agent_handoff_generation(generation) {
                                return Err(RecentWorkspaceError::InvalidContent(
                                    "record agent handoff generation is invalid",
                                ));
                            }
                            Some(generation.to_owned())
                        }
                    } else {
                        agent_handoff_installation
                            .as_ref()
                            .map(|_| "legacy-v8".to_owned())
                    };
                    if agent_handoff_installation.is_some() && agent_handoff_generation.is_none() {
                        return Err(RecentWorkspaceError::InvalidContent(
                            "record active agent handoff has no generation",
                        ));
                    }
                    if has_agent_handoff_generation {
                        expected_fields.push((
                            "agent_handoff_generation",
                            agent_handoff_generation
                                .as_ref()
                                .map_or(Json::Null, Json::text),
                        ));
                    }
                    let source_point_ordinal = if has_source_point_ordinal {
                        let ordinal_value = value.get("source_point_ordinal").ok_or(
                            RecentWorkspaceError::InvalidContent(
                                "record workspace has no source point ordinal",
                            ),
                        )?;
                        if ordinal_value == &Json::Null {
                            None
                        } else {
                            let ordinal = ordinal_value.as_u64().ok_or(
                                RecentWorkspaceError::InvalidContent(
                                    "record source point ordinal is not a number or null",
                                ),
                            )?;
                            if ordinal == 0 {
                                return Err(RecentWorkspaceError::InvalidContent(
                                    "record source point ordinal is zero",
                                ));
                            }
                            Some(ordinal)
                        }
                    } else {
                        None
                    };
                    if has_source_point_ordinal {
                        expected_fields.push((
                            "source_point_ordinal",
                            source_point_ordinal.map_or(Json::Null, Json::Number),
                        ));
                    }
                    let original_update_version = if has_original_update_version {
                        let version_value = value.get("original_update_version").ok_or(
                            RecentWorkspaceError::InvalidContent(
                                "record workspace has no original update version",
                            ),
                        )?;
                        if version_value == &Json::Null {
                            None
                        } else {
                            let Some(version) = version_value.as_text() else {
                                return Err(RecentWorkspaceError::InvalidContent(
                                    "record original update version is not text or null",
                                ));
                            };
                            if version.is_empty() {
                                return Err(RecentWorkspaceError::InvalidContent(
                                    "record original update version is empty",
                                ));
                            }
                            Some(version.to_owned())
                        }
                    } else {
                        None
                    };
                    if has_original_update_version {
                        expected_fields.push((
                            "original_update_version",
                            original_update_version
                                .as_ref()
                                .map_or(Json::Null, Json::text),
                        ));
                    }
                    let expected = Json::object(expected_fields);
                    if value != &expected {
                        return Err(RecentWorkspaceError::InvalidContent(
                            "record workspace shape is not canonical",
                        ));
                    }
                    encoded_entries.push(expected);
                    entries.push(RecentWorkspaceEntry {
                        path,
                        export_root,
                        project_root,
                        agent_handoff_installation,
                        agent_handoff_directory,
                        agent_handoff_generation,
                        source_point_ordinal,
                        original_update_version,
                    });
                }
                let expected = Json::object([
                    ("schema", Json::text(schema.expect("matched schema"))),
                    ("workspaces", Json::Array(encoded_entries)),
                ]);
                if parsed != expected || text != expected.encode() {
                    return Err(RecentWorkspaceError::InvalidContent(
                        "record shape is not canonical",
                    ));
                }
                entries
            }
            _ => {
                return Err(RecentWorkspaceError::InvalidContent(
                    "record schema is unsupported",
                ))
            }
        };
        if entries.iter().any(|entry| !entry.path.is_absolute()) {
            return Err(RecentWorkspaceError::InvalidContent(
                "remembered path is not absolute",
            ));
        }
        Ok(entries)
    }

    /// Atomically remember one existing directory by its canonical operating-system path.
    pub fn remember(&self, path: &Path) -> Result<PathBuf, RecentWorkspaceError> {
        self.remember_with_export_root(path, None)
    }

    /// Refuse before a native operation creates or opens another workspace when every bounded
    /// navigation slot is still the durable custody record for an active agent folder.
    pub fn ensure_can_remember_another(&self) -> Result<(), RecentWorkspaceError> {
        let entries = self.load_entries()?;
        if entries.len() < MAX_RECENT_WORKSPACES
            || entries
                .iter()
                .any(|entry| entry.agent_handoff_installation.is_none())
        {
            return Ok(());
        }
        Err(active_agent_capacity_error())
    }

    /// An exact workspace already in the record consumes no additional bounded slot.
    pub fn ensure_can_remember(&self, path: &Path) -> Result<(), RecentWorkspaceError> {
        let canonical = canonical_workspace(path)?;
        let entries = self.load_entries()?;
        if entries.iter().any(|entry| entry.path == canonical) {
            return Ok(());
        }
        if entries.len() < MAX_RECENT_WORKSPACES
            || entries
                .iter()
                .any(|entry| entry.agent_handoff_installation.is_none())
        {
            return Ok(());
        }
        Err(active_agent_capacity_error())
    }

    /// Remember a workspace together with an optional ordinary export destination hint.
    ///
    /// A supplied destination must be an absolute UTF-8 path, but need not remain present. It is
    /// only prefilled for the person's next explicit preview; it never authorizes a write.
    pub fn remember_with_export_root(
        &self,
        path: &Path,
        export_root: Option<&Path>,
    ) -> Result<PathBuf, RecentWorkspaceError> {
        self.remember_with_navigation_hints(path, export_root, None)
    }

    /// Remember navigation while optionally inheriting a stable project label from a source
    /// workspace. The stable label is write-once for an existing entry; the export destination
    /// remains an independently mutable convenience hint.
    pub fn remember_with_navigation_hints(
        &self,
        path: &Path,
        export_root: Option<&Path>,
        inherited_project_root: Option<&Path>,
    ) -> Result<PathBuf, RecentWorkspaceError> {
        self.remember_with_navigation_hints_at_point(
            path,
            export_root,
            inherited_project_root,
            None,
        )
    }

    /// Remember navigation and the authoritative source point used for an independent copy.
    pub fn remember_with_navigation_hints_at_point(
        &self,
        path: &Path,
        export_root: Option<&Path>,
        inherited_project_root: Option<&Path>,
        source_point_ordinal: Option<u64>,
    ) -> Result<PathBuf, RecentWorkspaceError> {
        if source_point_ordinal == Some(0) {
            return Err(RecentWorkspaceError::Invalid(
                "source point ordinal is zero",
            ));
        }
        let _lock = self.acquire_lock()?;
        let canonical = canonical_workspace(path)?;
        let directory = ProtectedWorkspaceRoot::inspect(&canonical)
            .map_err(|error| RecentWorkspaceError::io("inspect remembered workspace", error))?;
        let supplied_export_root = export_root.map(normalized_export_root_hint).transpose()?;
        let supplied_project_root = inherited_project_root
            .map(normalized_export_root_hint)
            .transpose()?;
        let mut entries = self.load_entries()?;
        let exact = entries
            .iter()
            .find(|entry| entry.path == canonical)
            .cloned();
        let custody = active_custody_for_directory(&entries, &directory)?.cloned();
        if exact
            .as_ref()
            .is_some_and(|entry| entry.agent_handoff_installation.is_some())
            && custody
                .as_ref()
                .is_some_and(|entry| entry.path != canonical)
        {
            return Err(RecentWorkspaceError::InvalidContent(
                "remembered path and physical directory carry different active agent custody",
            ));
        }
        let retained_export_root = exact
            .as_ref()
            .and_then(|entry| entry.export_root.clone())
            .or_else(|| custody.as_ref().and_then(|entry| entry.export_root.clone()));
        let retained_project_root = exact
            .as_ref()
            .and_then(|entry| entry.project_root.clone())
            .or_else(|| {
                custody
                    .as_ref()
                    .and_then(|entry| entry.project_root.clone())
            });
        let custody_or_exact = custody.as_ref().or(exact.as_ref());
        let retained_agent_handoff =
            custody_or_exact.and_then(|entry| entry.agent_handoff_installation.clone());
        let retained_agent_handoff_directory =
            custody_or_exact.and_then(|entry| entry.agent_handoff_directory.clone());
        let retained_agent_handoff_generation =
            custody_or_exact.and_then(|entry| entry.agent_handoff_generation.clone());
        let retained_source_point_ordinal = exact
            .as_ref()
            .and_then(|entry| entry.source_point_ordinal)
            .or_else(|| {
                custody
                    .as_ref()
                    .and_then(|entry| entry.source_point_ordinal)
            });
        let project_root = retained_project_root
            .or(supplied_project_root)
            .or_else(|| supplied_export_root.clone())
            .or_else(|| retained_export_root.clone());
        let original_update_version = exact
            .as_ref()
            .and_then(|entry| entry.original_update_version.clone())
            .or_else(|| {
                custody
                    .as_ref()
                    .and_then(|entry| entry.original_update_version.clone())
            })
            .or_else(|| {
                entries
                    .iter()
                    .find(|entry| entry.project_root.as_ref() == project_root.as_ref())
                    .and_then(|entry| entry.original_update_version.clone())
            });
        let custody_path = custody.as_ref().map(|entry| entry.path.clone());
        entries.retain(|entry| {
            entry.path != canonical && custody_path.as_ref().is_none_or(|path| entry.path != *path)
        });
        entries.insert(
            0,
            RecentWorkspaceEntry {
                path: canonical.clone(),
                export_root: supplied_export_root.or(retained_export_root),
                project_root,
                agent_handoff_installation: retained_agent_handoff,
                agent_handoff_directory: retained_agent_handoff_directory,
                agent_handoff_generation: retained_agent_handoff_generation,
                source_point_ordinal: source_point_ordinal.or(retained_source_point_ordinal),
                original_update_version,
            },
        );
        bound_recent_history_preserving_agent_custody(&mut entries)?;
        self.publish(&entries)?;
        debug_assert_eq!(entries[0].path, canonical);
        Ok(canonical)
    }

    /// Remember a daemon-verified workspace and replace only malformed app-owned record content.
    ///
    /// Ownership, file type, directory, race and I/O failures remain refusals. Callers may use
    /// this recovery path only after the live daemon has verified the workspace being selected.
    /// Invalid historic navigation hints are convenience data and cannot be safely recovered, so
    /// repair starts a new one-entry history and reports that fact to the person.
    pub fn remember_repairing_invalid_content_at_point(
        &self,
        path: &Path,
        export_root: Option<&Path>,
        inherited_project_root: Option<&Path>,
        source_point_ordinal: Option<u64>,
    ) -> Result<(PathBuf, bool), RecentWorkspaceError> {
        if source_point_ordinal == Some(0) {
            return Err(RecentWorkspaceError::Invalid(
                "source point ordinal is zero",
            ));
        }
        let _lock = self.acquire_lock()?;
        let canonical = canonical_workspace(path)?;
        let directory = ProtectedWorkspaceRoot::inspect(&canonical)
            .map_err(|error| RecentWorkspaceError::io("inspect remembered workspace", error))?;
        let supplied_export_root = export_root.map(normalized_export_root_hint).transpose()?;
        let supplied_project_root = inherited_project_root
            .map(normalized_export_root_hint)
            .transpose()?;
        let mut entries = match self.load_entries() {
            Ok(entries) => entries,
            Err(error @ RecentWorkspaceError::InvalidContent(_)) => {
                if malformed_record_may_hold_active_agent_custody(self)? {
                    return Err(error);
                }
                self.publish(&[RecentWorkspaceEntry {
                    path: canonical.clone(),
                    project_root: supplied_project_root,
                    export_root: supplied_export_root,
                    agent_handoff_installation: None,
                    agent_handoff_directory: None,
                    agent_handoff_generation: None,
                    source_point_ordinal,
                    original_update_version: None,
                }])?;
                return Ok((canonical, true));
            }
            Err(error) => return Err(error),
        };
        let exact = entries
            .iter()
            .find(|entry| entry.path == canonical)
            .cloned();
        let custody = active_custody_for_directory(&entries, &directory)?.cloned();
        if exact
            .as_ref()
            .is_some_and(|entry| entry.agent_handoff_installation.is_some())
            && custody
                .as_ref()
                .is_some_and(|entry| entry.path != canonical)
        {
            return Err(RecentWorkspaceError::InvalidContent(
                "remembered path and physical directory carry different active agent custody",
            ));
        }
        let retained_export_root = exact
            .as_ref()
            .and_then(|entry| entry.export_root.clone())
            .or_else(|| custody.as_ref().and_then(|entry| entry.export_root.clone()));
        let retained_project_root = exact
            .as_ref()
            .and_then(|entry| entry.project_root.clone())
            .or_else(|| {
                custody
                    .as_ref()
                    .and_then(|entry| entry.project_root.clone())
            });
        let custody_or_exact = custody.as_ref().or(exact.as_ref());
        let retained_agent_handoff =
            custody_or_exact.and_then(|entry| entry.agent_handoff_installation.clone());
        let retained_agent_handoff_directory =
            custody_or_exact.and_then(|entry| entry.agent_handoff_directory.clone());
        let retained_agent_handoff_generation =
            custody_or_exact.and_then(|entry| entry.agent_handoff_generation.clone());
        let retained_source_point_ordinal = exact
            .as_ref()
            .and_then(|entry| entry.source_point_ordinal)
            .or_else(|| {
                custody
                    .as_ref()
                    .and_then(|entry| entry.source_point_ordinal)
            });
        // This recovery-capable navigation path is used by the native host after ordinary opens
        // and exports. An export destination is convenience, not proof of project ancestry: only
        // import or version-fork code may supply an inherited stable project root. This prevents
        // forget/re-remember from relabelling an arbitrary destination as the original project.
        let project_root = retained_project_root.or(supplied_project_root);
        let original_update_version = exact
            .as_ref()
            .and_then(|entry| entry.original_update_version.clone())
            .or_else(|| {
                custody
                    .as_ref()
                    .and_then(|entry| entry.original_update_version.clone())
            })
            .or_else(|| {
                entries
                    .iter()
                    .find(|entry| entry.project_root.as_ref() == project_root.as_ref())
                    .and_then(|entry| entry.original_update_version.clone())
            });
        let custody_path = custody.as_ref().map(|entry| entry.path.clone());
        entries.retain(|entry| {
            entry.path != canonical && custody_path.as_ref().is_none_or(|path| entry.path != *path)
        });
        entries.insert(
            0,
            RecentWorkspaceEntry {
                path: canonical.clone(),
                export_root: supplied_export_root.or(retained_export_root),
                project_root,
                agent_handoff_installation: retained_agent_handoff,
                agent_handoff_directory: retained_agent_handoff_directory,
                agent_handoff_generation: retained_agent_handoff_generation,
                source_point_ordinal: source_point_ordinal.or(retained_source_point_ordinal),
                original_update_version,
            },
        );
        bound_recent_history_preserving_agent_custody(&mut entries)?;
        self.publish(&entries)?;
        Ok((canonical, false))
    }

    /// Record that one exact shared version was fully reconciled to the original ordinary folder.
    ///
    /// The marker follows every remembered copy in the same workspace family. It remains only a
    /// reminder: callers must independently verify the live workspace and complete the exact
    /// ordinary-folder preview before invoking this method.
    pub fn record_original_update(
        &self,
        path: &Path,
        version: &str,
    ) -> Result<(), RecentWorkspaceError> {
        if version.is_empty() || version.len() > 512 {
            return Err(RecentWorkspaceError::Invalid(
                "original update version is empty or too large",
            ));
        }
        let canonical = canonical_workspace(path)?;
        let _lock = self.acquire_lock()?;
        let mut entries = self.load_entries()?;
        let current = entries.iter().find(|entry| entry.path == canonical).ok_or(
            RecentWorkspaceError::Invalid("workspace is not in recent history"),
        )?;
        let project_root = current
            .project_root
            .clone()
            .ok_or(RecentWorkspaceError::Invalid(
                "workspace has no original project folder",
            ))?;
        if current.export_root.as_ref() != Some(&project_root) {
            return Err(RecentWorkspaceError::Invalid(
                "current update destination is not the original project folder",
            ));
        }
        for entry in &mut entries {
            if entry.project_root.as_ref() == Some(&project_root) {
                entry.original_update_version = Some(version.to_owned());
            }
        }
        self.publish(&entries)
    }

    fn publish(&self, entries: &[RecentWorkspaceEntry]) -> Result<(), RecentWorkspaceError> {
        if entries.is_empty() || entries.len() > MAX_RECENT_WORKSPACES {
            return Err(RecentWorkspaceError::Invalid(
                "workspace history size is outside the supported range",
            ));
        }
        let mut encoded_entries = Vec::with_capacity(entries.len());
        for entry in entries {
            if entry.agent_handoff_installation.is_some() != entry.agent_handoff_directory.is_some()
            {
                return Err(RecentWorkspaceError::Invalid(
                    "legacy agent custody must be cleared or re-recorded before navigation changes",
                ));
            }
            let Some(path) = entry.path.to_str() else {
                return Err(RecentWorkspaceError::Invalid("workspace path is not UTF-8"));
            };
            let export_root = match entry.export_root.as_ref() {
                Some(root) => {
                    let Some(root) = root.to_str() else {
                        return Err(RecentWorkspaceError::Invalid(
                            "export destination is not UTF-8",
                        ));
                    };
                    Json::text(root)
                }
                None => Json::Null,
            };
            let project_root = match entry.project_root.as_ref() {
                Some(root) => {
                    let Some(root) = root.to_str() else {
                        return Err(RecentWorkspaceError::Invalid(
                            "stable project folder is not UTF-8",
                        ));
                    };
                    Json::text(root)
                }
                None => Json::Null,
            };
            let agent_handoff_installation = entry
                .agent_handoff_installation
                .as_ref()
                .map_or(Json::Null, Json::text);
            let agent_handoff_directory = entry
                .agent_handoff_directory
                .as_ref()
                .map_or(Json::Null, Json::text);
            let source_point_ordinal = entry.source_point_ordinal.map_or(Json::Null, Json::Number);
            encoded_entries.push(Json::object([
                ("path", Json::text(path)),
                ("export_root", export_root),
                ("project_root", project_root),
                ("agent_handoff_installation", agent_handoff_installation),
                ("agent_handoff_directory", agent_handoff_directory),
                (
                    "agent_handoff_generation",
                    entry
                        .agent_handoff_generation
                        .as_ref()
                        .map_or(Json::Null, Json::text),
                ),
                ("source_point_ordinal", source_point_ordinal),
                (
                    "original_update_version",
                    entry
                        .original_update_version
                        .as_ref()
                        .map_or(Json::Null, Json::text),
                ),
            ]));
        }
        let encoded = Json::object([
            ("schema", Json::text(SCHEMA_V9)),
            ("workspaces", Json::Array(encoded_entries)),
        ])
        .encode();
        if encoded.len() as u64 > MAX_RECORD_BYTES {
            return Err(RecentWorkspaceError::Invalid(
                "workspace history is too large",
            ));
        }

        fs::create_dir_all(&self.directory)
            .map_err(|error| RecentWorkspaceError::io("create application data", error))?;
        let directory_metadata = fs::symlink_metadata(&self.directory)
            .map_err(|error| RecentWorkspaceError::io("inspect application data", error))?;
        if !directory_metadata.file_type().is_dir() || directory_metadata.file_type().is_symlink() {
            return Err(RecentWorkspaceError::Invalid(
                "application data is not a real directory",
            ));
        }
        fs::set_permissions(&self.directory, fs::Permissions::from_mode(0o700))
            .map_err(|error| RecentWorkspaceError::io("secure application data", error))?;
        validate_private_directory(&self.directory)?;

        let temporary = self.directory.join(format!(
            ".recent-workspace-{}-{}.tmp",
            std::process::id(),
            NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)
                .map_err(|error| RecentWorkspaceError::io("create temporary record", error))?;
            file.write_all(encoded.as_bytes())
                .map_err(|error| RecentWorkspaceError::io("write temporary record", error))?;
            file.sync_all()
                .map_err(|error| RecentWorkspaceError::io("sync temporary record", error))?;
            fs::rename(&temporary, &self.record)
                .map_err(|error| RecentWorkspaceError::io("publish record", error))?;
            sync_directory(&self.directory)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    /// Conservatively remember that an exact physical workspace was handed to an agent.
    ///
    /// Callers record this before invoking the external launcher. A launcher failure can therefore
    /// leave an extra confirmation prompt, but a crash cannot erase the collision warning after an
    /// agent may have started. The workspace must already be in the app-owned navigation history.
    #[cfg(test)]
    pub fn record_agent_handoff(
        &self,
        path: &Path,
        installation: &str,
        expected_directory: &str,
    ) -> Result<String, RecentWorkspaceError> {
        self.record_agent_handoff_with_reopen(path, installation, expected_directory, false, None)
    }

    /// Acquire exact agent custody, optionally after the person explicitly confirmed a reopen.
    ///
    /// This transaction reloads and checks the durable record while holding the process-shared
    /// lock. A stale renderer or a simultaneous application process therefore cannot turn a first
    /// launch into an unconfirmed second agent in the same writable folder.
    #[cfg(test)]
    pub fn record_agent_handoff_with_reopen(
        &self,
        path: &Path,
        installation: &str,
        expected_directory: &str,
        confirmed_reopen: bool,
        expected_generation: Option<&str>,
    ) -> Result<String, RecentWorkspaceError> {
        self.record_agent_handoff_with_authority(
            path,
            installation,
            expected_directory,
            confirmed_reopen,
            expected_generation,
            new_agent_handoff_generation,
        )
    }

    /// Mirror one workspace-native custody transaction while holding Recent's process lock.
    ///
    /// The callback must acquire only the shared workspace custody and daemon locks; it must never
    /// wait on this RecentWorkspace again. A callback success followed by a projection failure is
    /// deliberately fail-safe because workspace custody, not this cache, remains authoritative.
    pub fn record_agent_handoff_with_authority(
        &self,
        path: &Path,
        installation: &str,
        expected_directory: &str,
        confirmed_reopen: bool,
        expected_generation: Option<&str>,
        authority: impl FnOnce() -> Result<String, RecentWorkspaceError>,
    ) -> Result<String, RecentWorkspaceError> {
        if installation.is_empty() {
            return Err(RecentWorkspaceError::Invalid(
                "agent handoff installation is empty",
            ));
        }
        match (confirmed_reopen, expected_generation) {
            (false, None) => {}
            (true, Some(generation)) if valid_agent_handoff_generation(generation) => {}
            (true, Some(_)) => {
                return Err(RecentWorkspaceError::Invalid(
                    "confirmed agent reopen generation is invalid",
                ))
            }
            (true, None) => {
                return Err(RecentWorkspaceError::Invalid(
                    "confirmed agent reopen requires the exact displayed custody generation",
                ))
            }
            (false, Some(_)) => {
                return Err(RecentWorkspaceError::Invalid(
                    "a first agent handoff cannot carry reopen authority",
                ))
            }
        }
        let canonical = canonical_workspace(path)?;
        let directory = ProtectedWorkspaceRoot::inspect(&canonical)
            .map_err(|error| RecentWorkspaceError::io("inspect agent workspace", error))?;
        let expected_directory = ProtectedWorkspaceRoot::from_directory_token(expected_directory)
            .map_err(|_| {
            RecentWorkspaceError::Invalid("agent handoff directory identity is invalid")
        })?;
        if directory != expected_directory {
            return Err(RecentWorkspaceError::Invalid(
                "agent handoff workspace was replaced before custody was recorded",
            ));
        }
        let expected_directory_token = expected_directory.directory_token();
        let _lock = self.acquire_lock()?;
        let mut entries = self.load_entries()?;
        let Some(entry) = entries.iter_mut().find(|entry| entry.path == canonical) else {
            return Err(RecentWorkspaceError::Invalid(
                "agent handoff workspace is not remembered",
            ));
        };
        match entry.agent_handoff_installation.as_deref() {
            Some(recorded)
                if recorded == installation
                    && entry.agent_handoff_directory.as_deref()
                        == Some(expected_directory_token.as_str()) =>
            {
                if confirmed_reopen {
                    if entry.agent_handoff_generation.as_deref() != expected_generation {
                        return Err(RecentWorkspaceError::Invalid(
                            "agent folder assignment changed; refresh and confirm the current assignment",
                        ));
                    }
                    let generation = authority()?;
                    if !valid_agent_handoff_generation(&generation) {
                        return Err(RecentWorkspaceError::InvalidContent(
                            "workspace custody returned an invalid generation",
                        ));
                    }
                    entry.agent_handoff_generation = Some(generation.clone());
                    self.publish(&entries)?;
                    return Ok(generation);
                }
                return Err(RecentWorkspaceError::Invalid(
                    "workspace is already assigned to an agent; confirm a reopen or create an independent copy",
                ));
            }
            Some(_) => {
                return Err(RecentWorkspaceError::Invalid(
                    "workspace is already assigned to another agent installation",
                ))
            }
            None if confirmed_reopen => {
                return Err(RecentWorkspaceError::Invalid(
                    "agent folder is no longer assigned; refresh before starting another agent",
                ))
            }
            None => {}
        }
        let generation = authority()?;
        if !valid_agent_handoff_generation(&generation) {
            return Err(RecentWorkspaceError::InvalidContent(
                "workspace custody returned an invalid generation",
            ));
        }
        entry.agent_handoff_installation = Some(installation.to_owned());
        entry.agent_handoff_directory = Some(expected_directory_token);
        entry.agent_handoff_generation = Some(generation.clone());
        self.publish(&entries)?;
        Ok(generation)
    }

    /// Refresh this navigation cache from the workspace-native custody authority.
    ///
    /// Recent's lock is held while the callback reads shared authority, preserving the only order
    /// allowed when both stores participate. The callback result is a projection only: every
    /// mutation still rechecks workspace-native custody independently.
    pub fn reconcile_agent_handoff_from_authority(
        &self,
        path: &Path,
        installation: &str,
        expected_directory: &str,
        authority: impl FnOnce(bool) -> Result<Option<String>, RecentWorkspaceError>,
    ) -> Result<(), RecentWorkspaceError> {
        let canonical = canonical_workspace(path)?;
        let directory = ProtectedWorkspaceRoot::inspect(&canonical)
            .map_err(|error| RecentWorkspaceError::io("inspect agent workspace", error))?;
        let expected_directory = ProtectedWorkspaceRoot::from_directory_token(expected_directory)
            .map_err(|_| {
            RecentWorkspaceError::Invalid("agent handoff directory identity is invalid")
        })?;
        if directory != expected_directory {
            return Err(RecentWorkspaceError::Invalid(
                "agent handoff workspace was replaced before custody refresh",
            ));
        }
        let expected_directory_token = expected_directory.directory_token();
        let _lock = self.acquire_lock()?;
        let mut entries = self.load_entries()?;
        let Some(entry) = entries.iter_mut().find(|entry| entry.path == canonical) else {
            return Err(RecentWorkspaceError::Invalid(
                "agent handoff workspace is not remembered",
            ));
        };
        let projected_active = match entry.agent_handoff_installation.as_deref() {
            Some(recorded)
                if recorded == installation
                    && entry.agent_handoff_directory.as_deref()
                        == Some(expected_directory_token.as_str()) =>
            {
                true
            }
            Some(_) => {
                return Err(RecentWorkspaceError::InvalidContent(
                    "Recent agent custody conflicts with this workspace installation",
                ));
            }
            None => false,
        };
        match authority(projected_active)? {
            Some(generation) if valid_agent_handoff_generation(&generation) => {
                entry.agent_handoff_installation = Some(installation.to_owned());
                entry.agent_handoff_directory = Some(expected_directory_token);
                entry.agent_handoff_generation = Some(generation);
            }
            Some(_) => {
                return Err(RecentWorkspaceError::InvalidContent(
                    "workspace custody returned an invalid generation",
                ));
            }
            None => {
                entry.agent_handoff_installation = None;
                entry.agent_handoff_directory = None;
                // Keep the last generation as an inactive tombstone. Projection treats only the
                // installation+directory pair as active, while the next acquisition must replace
                // this generation before a stale release can match an active assignment again.
            }
        }
        self.publish(&entries)
    }

    /// Clear only the handoff for the exact physical workspace installation the person ended.
    ///
    /// A stale browser or another application process cannot clear the warning for a replacement
    /// installation at the same path. Repeating an already completed clear is an idempotent no-op.
    #[cfg(test)]
    pub fn clear_agent_handoff(
        &self,
        path: &Path,
        installation: &str,
        expected_generation: &str,
    ) -> Result<bool, RecentWorkspaceError> {
        self.clear_agent_handoff_with_authority(path, installation, expected_generation, || {
            Ok(true)
        })
    }

    /// Clear workspace-native custody and then mirror that release into Recent while one lock is held.
    pub fn clear_agent_handoff_with_authority(
        &self,
        path: &Path,
        installation: &str,
        expected_generation: &str,
        authority: impl FnOnce() -> Result<bool, RecentWorkspaceError>,
    ) -> Result<bool, RecentWorkspaceError> {
        if installation.is_empty() {
            return Err(RecentWorkspaceError::Invalid(
                "agent handoff installation is empty",
            ));
        }
        if !valid_agent_handoff_generation(expected_generation) {
            return Err(RecentWorkspaceError::Invalid(
                "agent handoff generation is invalid",
            ));
        }
        let canonical = canonical_workspace(path)?;
        let current_directory = ProtectedWorkspaceRoot::inspect(&canonical)
            .map_err(|error| RecentWorkspaceError::io("inspect agent workspace", error))?;
        let _lock = self.acquire_lock()?;
        let mut entries = self.load_entries()?;
        let Some(entry) = entries.iter_mut().find(|entry| entry.path == canonical) else {
            return Err(RecentWorkspaceError::Invalid(
                "agent handoff workspace is not remembered",
            ));
        };
        if entry.agent_handoff_generation.as_deref() != Some(expected_generation) {
            return Err(RecentWorkspaceError::Invalid(
                "agent handoff generation changed after this release was prepared",
            ));
        }
        match entry.agent_handoff_installation.as_deref() {
            Some(recorded) if recorded == installation => {
                if let Some(recorded_directory) = entry.agent_handoff_directory.as_deref() {
                    let recorded_directory = ProtectedWorkspaceRoot::from_directory_token(
                        recorded_directory,
                    )
                    .map_err(|_| {
                        RecentWorkspaceError::InvalidContent(
                            "record agent handoff directory is invalid",
                        )
                    })?;
                    if current_directory != recorded_directory {
                        return Err(RecentWorkspaceError::Invalid(
                            "agent handoff workspace was replaced",
                        ));
                    }
                }
                let cleared = authority()?;
                entry.agent_handoff_installation = None;
                entry.agent_handoff_directory = None;
                // The generation is deliberately retained as an inactive anti-ABA tombstone.
                // No navigation or reopen path projects custody without the active pair above.
                self.publish(&entries)?;
                Ok(cleared)
            }
            Some(_) => Err(RecentWorkspaceError::Invalid(
                "agent handoff belongs to another workspace installation",
            )),
            None => Ok(false),
        }
    }

    /// Forget only the exact workspace currently recorded.
    pub fn forget_if_matches(&self, path: &Path) -> Result<bool, RecentWorkspaceError> {
        let _lock = self.acquire_lock()?;
        let mut remembered = self.load_entries()?;
        if remembered.iter().any(|candidate| {
            candidate.path == path && candidate.agent_handoff_installation.is_some()
        }) {
            return Err(RecentWorkspaceError::Invalid(
                "workspace is still assigned to an agent",
            ));
        }
        let before = remembered.len();
        remembered.retain(|candidate| candidate.path != path);
        if remembered.len() == before {
            return Ok(false);
        }
        if remembered.is_empty() {
            fs::remove_file(&self.record)
                .map_err(|error| RecentWorkspaceError::io("remove record", error))?;
        } else {
            self.publish(&remembered)?;
        }
        sync_directory(&self.directory)?;
        Ok(true)
    }
}

fn active_custody_matches_directory(
    entry: &RecentWorkspaceEntry,
    directory: &ProtectedWorkspaceRoot,
) -> Result<bool, RecentWorkspaceError> {
    if entry.agent_handoff_installation.is_none() {
        return Ok(false);
    }
    let token =
        entry
            .agent_handoff_directory
            .as_deref()
            .ok_or(RecentWorkspaceError::InvalidContent(
                "legacy active agent custody has no physical directory identity",
            ))?;
    let recorded = ProtectedWorkspaceRoot::from_directory_token(token).map_err(|_| {
        RecentWorkspaceError::InvalidContent("record active agent handoff directory is invalid")
    })?;
    Ok(&recorded == directory)
}

fn active_custody_for_directory<'a>(
    entries: &'a [RecentWorkspaceEntry],
    directory: &ProtectedWorkspaceRoot,
) -> Result<Option<&'a RecentWorkspaceEntry>, RecentWorkspaceError> {
    let mut matched = None;
    for entry in entries
        .iter()
        .filter(|entry| entry.agent_handoff_installation.is_some())
    {
        if active_custody_matches_directory(entry, directory)? {
            if matched.is_some() {
                return Err(RecentWorkspaceError::InvalidContent(
                    "record repeats one active agent directory identity",
                ));
            }
            matched = Some(entry);
        }
    }
    Ok(matched)
}

fn malformed_record_may_hold_active_agent_custody(
    recent: &RecentWorkspace,
) -> Result<bool, RecentWorkspaceError> {
    let Some(bytes) = recent.read_record_bytes()? else {
        return Ok(true);
    };
    let text = std::str::from_utf8(&bytes).map_err(|_| {
        RecentWorkspaceError::InvalidContent(
            "malformed record may hide an active agent directory identity",
        )
    })?;
    let Ok(parsed) = Json::parse(text) else {
        return Ok(text.contains("\"agent_handoff_installation\"")
            || text.contains("\"agent_handoff_directory\"")
            || text.contains("\"agent_handoff_generation\""));
    };
    let canonical_workspace_list_required = matches!(
        parsed.get("schema").and_then(Json::as_text),
        Some(SCHEMA_V2)
            | Some(SCHEMA_V3)
            | Some(SCHEMA_V4)
            | Some(SCHEMA_V5)
            | Some(SCHEMA_V6)
            | Some(SCHEMA_V7)
            | Some(SCHEMA_V8)
            | Some(SCHEMA_V9)
    );
    let canonical_workspace_list_missing = canonical_workspace_list_required
        && parsed.get("workspaces").and_then(Json::as_array).is_none();
    Ok(canonical_workspace_list_missing || json_may_hold_agent_custody(&parsed))
}

fn json_may_hold_agent_custody(value: &Json) -> bool {
    match value {
        Json::Object(fields) => fields.iter().any(|(key, value)| {
            (matches!(
                key.as_str(),
                "agent_handoff_installation"
                    | "agent_handoff_directory"
                    | "agent_handoff_generation"
            ) && value != &Json::Null)
                || json_may_hold_agent_custody(value)
        }),
        Json::Array(values) => values.iter().any(json_may_hold_agent_custody),
        Json::Null | Json::Bool(_) | Json::Number(_) | Json::Text(_) => false,
    }
}

fn valid_agent_handoff_generation(value: &str) -> bool {
    value == "legacy-v8"
        || (value.len() == 32
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
}

#[cfg(test)]
fn new_agent_handoff_generation() -> Result<String, RecentWorkspaceError> {
    let mut random = [0_u8; 16];
    File::open("/dev/urandom")
        .and_then(|mut source| source.read_exact(&mut random))
        .map_err(|error| RecentWorkspaceError::io("create agent handoff generation", error))?;
    let mut encoded = String::with_capacity(32);
    for byte in random {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing into a string cannot fail");
    }
    Ok(encoded)
}

fn bound_recent_history_preserving_agent_custody(
    entries: &mut Vec<RecentWorkspaceEntry>,
) -> Result<(), RecentWorkspaceError> {
    while entries.len() > MAX_RECENT_WORKSPACES {
        let Some(eviction) = (1..entries.len())
            .rev()
            .find(|index| entries[*index].agent_handoff_installation.is_none())
        else {
            return Err(active_agent_capacity_error());
        };
        entries.remove(eviction);
    }
    Ok(())
}

fn active_agent_capacity_error() -> RecentWorkspaceError {
    RecentWorkspaceError::Invalid(
        "recent workspace capacity is held by active agent folders; finish one agent before opening another workspace",
    )
}

fn canonical_workspace(path: &Path) -> Result<PathBuf, RecentWorkspaceError> {
    let canonical = fs::canonicalize(path)
        .map_err(|error| RecentWorkspaceError::io("resolve workspace", error))?;
    let metadata = fs::metadata(&canonical)
        .map_err(|error| RecentWorkspaceError::io("inspect workspace", error))?;
    if !metadata.is_dir() {
        return Err(RecentWorkspaceError::Invalid(
            "workspace is not a directory",
        ));
    }
    if canonical.to_str().is_none() {
        return Err(RecentWorkspaceError::Invalid("workspace path is not UTF-8"));
    }
    Ok(canonical)
}

fn validate_absolute_utf8_hint(
    path: &Path,
    relative_reason: &'static str,
) -> Result<(), RecentWorkspaceError> {
    if !path.is_absolute() {
        return Err(RecentWorkspaceError::InvalidContent(relative_reason));
    }
    if path.to_str().is_none() {
        return Err(RecentWorkspaceError::InvalidContent(
            "remembered path is not UTF-8",
        ));
    }
    Ok(())
}

fn normalized_export_root_hint(path: &Path) -> Result<PathBuf, RecentWorkspaceError> {
    validate_absolute_utf8_hint(path, "remembered export destination is not absolute")?;
    match fs::canonicalize(path) {
        Ok(canonical) => {
            if !fs::metadata(&canonical)
                .map_err(|error| RecentWorkspaceError::io("inspect export destination", error))?
                .is_dir()
            {
                return Err(RecentWorkspaceError::InvalidContent(
                    "remembered export destination is not a directory",
                ));
            }
            if canonical.to_str().is_none() {
                return Err(RecentWorkspaceError::InvalidContent(
                    "remembered export destination is not UTF-8",
                ));
            }
            Ok(canonical)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(path.to_path_buf()),
        Err(error) => Err(RecentWorkspaceError::io(
            "resolve export destination",
            error,
        )),
    }
}

fn validate_private_directory(path: &Path) -> Result<(), RecentWorkspaceError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| RecentWorkspaceError::io("inspect application data", error))?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(RecentWorkspaceError::Invalid(
            "application data is not a real directory",
        ));
    }
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(RecentWorkspaceError::Invalid(
            "application data is not owner-only",
        ));
    }
    Ok(())
}

#[allow(unsafe_code)]
fn lock_exclusive(file: &File) -> Result<(), RecentWorkspaceError> {
    const LOCK_EXCLUSIVE: std::ffi::c_int = 2;
    extern "C" {
        fn flock(descriptor: std::ffi::c_int, operation: std::ffi::c_int) -> std::ffi::c_int;
    }
    // SAFETY: the descriptor is borrowed from a live File. flock retains no pointer, and the
    // operating system releases the advisory lock when the owning file description is closed.
    if unsafe { flock(file.as_raw_fd(), LOCK_EXCLUSIVE) } == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    Err(RecentWorkspaceError::io(
        "lock recent workspace record",
        error,
    ))
}

fn sync_directory(path: &Path) -> Result<(), RecentWorkspaceError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| RecentWorkspaceError::io("sync application data", error))
}

/// Why the host refused or could not update its private navigation record.
#[derive(Debug)]
pub enum RecentWorkspaceError {
    /// A bounded shape or ownership rule failed.
    Invalid(&'static str),
    /// The app-owned record passed its filesystem authority checks but its bounded content did not.
    InvalidContent(&'static str),
    /// A workspace-native authority refused while Recent's transaction lock was held.
    Authority(String),
    /// One local filesystem operation failed.
    Io {
        action: &'static str,
        source: std::io::Error,
    },
}

impl RecentWorkspaceError {
    fn io(action: &'static str, source: std::io::Error) -> Self {
        Self::Io { action, source }
    }
}

impl fmt::Display for RecentWorkspaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(reason) | Self::InvalidContent(reason) => {
                write!(formatter, "recent workspace refused: {reason}")
            }
            Self::Authority(reason) => formatter.write_str(reason),
            Self::Io { action, source } => {
                write!(formatter, "could not {action}: {source}")
            }
        }
    }
}

impl std::error::Error for RecentWorkspaceError {}

#[cfg(test)]
mod tests {
    use super::*;

    use std::os::unix::fs::symlink;
    use std::sync::{Arc, Barrier};

    fn scratch(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("mesh-desktop-recent-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("scratch");
        path
    }

    #[test]
    fn canonical_owner_only_record_round_trips_and_survives_a_stale_workspace() {
        let root = scratch("roundtrip");
        let app = root.join("app");
        let workspace = root.join("workspace");
        fs::create_dir(&workspace).expect("workspace");
        let recent = RecentWorkspace::new(app.clone());

        let canonical = recent.remember(&workspace).expect("remember");
        assert_eq!(recent.load_all().expect("load"), vec![canonical.clone()]);
        let metadata = fs::metadata(app.join(RECORD_NAME)).expect("metadata");
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        let lock_metadata = fs::metadata(app.join(RECORD_LOCK_NAME)).expect("lock metadata");
        assert_eq!(lock_metadata.permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::read_dir(&app).expect("read app").count(), 2);

        fs::remove_dir(&workspace).expect("make remembered workspace stale");
        assert_eq!(recent.load_all().expect("load stale"), vec![canonical]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn history_is_newest_first_deduplicated_bounded_and_migrates_v1() {
        let root = scratch("history");
        let app = root.join("app");
        fs::create_dir(&app).expect("app");
        fs::set_permissions(&app, fs::Permissions::from_mode(0o700)).expect("secure app");
        let legacy = root.join("workspace-legacy");
        fs::create_dir(&legacy).expect("legacy workspace");
        let legacy_record = Json::object([
            ("schema", Json::text(SCHEMA_V1)),
            ("path", Json::text(legacy.to_string_lossy())),
        ])
        .encode();
        fs::write(app.join(RECORD_NAME), legacy_record).expect("write legacy record");
        fs::set_permissions(app.join(RECORD_NAME), fs::Permissions::from_mode(0o600))
            .expect("secure legacy record");
        let recent = RecentWorkspace::new(app.clone());
        assert_eq!(recent.load_all().expect("load v1"), vec![legacy.clone()]);

        let mut workspaces = Vec::new();
        for index in 0..=MAX_RECENT_WORKSPACES {
            let workspace = root.join(format!("workspace-{index}"));
            fs::create_dir(&workspace).expect("workspace");
            recent.remember(&workspace).expect("remember workspace");
            workspaces.push(fs::canonicalize(workspace).expect("canonical workspace"));
        }
        let expected: Vec<_> = workspaces
            .iter()
            .rev()
            .take(MAX_RECENT_WORKSPACES)
            .cloned()
            .collect();
        assert_eq!(recent.load_all().expect("bounded history"), expected);

        let promoted = workspaces[3].clone();
        recent.remember(&promoted).expect("promote existing");
        let promoted_history = recent.load_all().expect("promoted history");
        assert_eq!(promoted_history[0], promoted);
        assert_eq!(
            promoted_history
                .iter()
                .filter(|candidate| *candidate == &promoted)
                .count(),
            1
        );
        let parsed =
            Json::parse(&fs::read_to_string(app.join(RECORD_NAME)).expect("read migrated record"))
                .expect("parse migrated record");
        assert_eq!(
            parsed.get("schema").and_then(Json::as_text),
            Some(SCHEMA_V9)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn export_destination_is_scoped_to_each_workspace_and_survives_reordering() {
        let root = scratch("export-destination");
        let app = root.join("app");
        let original = root.join("ordinary-project");
        let release_copy = root.join("release-copy");
        let workspace = root.join("workspace.mesh");
        let version = root.join("workspace-version.mesh");
        let unrelated = root.join("unrelated.mesh");
        for path in [&original, &release_copy, &workspace, &version, &unrelated] {
            fs::create_dir(path).expect("test directory");
        }
        let original = fs::canonicalize(original).expect("canonical ordinary project");
        let release_copy = fs::canonicalize(release_copy).expect("canonical release copy");
        let recent = RecentWorkspace::new(app);
        let workspace = recent
            .remember_with_export_root(&workspace, Some(&original))
            .expect("remember imported workspace");
        recent
            .record_original_update(&workspace, "shared-version-one")
            .expect("record exact original update");
        recent
            .remember_with_export_root(&workspace, Some(&release_copy))
            .expect("change only the pull-back destination");
        let relabeled = recent.load_entries().expect("load changed destination");
        assert_eq!(relabeled[0].export_root(), Some(release_copy.as_path()));
        assert_eq!(relabeled[0].project_root(), Some(original.as_path()));
        assert_eq!(
            relabeled[0].original_update_version(),
            Some("shared-version-one")
        );
        recent.remember(&unrelated).expect("remember unrelated");
        let source_entry = recent
            .load_entries()
            .expect("load imported workspace")
            .into_iter()
            .find(|entry| entry.path() == workspace)
            .expect("source entry");
        let inherited_export = source_entry.export_root().map(Path::to_path_buf);
        let inherited_project = source_entry.project_root().map(Path::to_path_buf);
        let version = recent
            .remember_with_navigation_hints_at_point(
                &version,
                inherited_export.as_deref(),
                inherited_project.as_deref(),
                Some(1),
            )
            .expect("remember version with inherited destination");

        let entries = recent.load_entries().expect("load version history");
        assert_eq!(entries[0].path(), version);
        assert_eq!(entries[0].export_root(), Some(release_copy.as_path()));
        assert_eq!(entries[0].project_root(), Some(original.as_path()));
        assert_eq!(entries[0].source_point_ordinal(), Some(1));
        assert_eq!(
            entries[0].original_update_version(),
            Some("shared-version-one")
        );
        assert_eq!(entries[1].export_root(), None);
        assert_eq!(entries[2].path(), workspace);
        assert_eq!(entries[2].export_root(), Some(release_copy.as_path()));
        assert_eq!(entries[2].project_root(), Some(original.as_path()));
        assert!(recent
            .record_original_update(&version, "redirected-version")
            .is_err());
        assert_eq!(
            recent.load_entries().expect("redirected refusal is no-op"),
            entries
        );

        recent
            .remember_with_export_root(&version, Some(&original))
            .expect("restore original destination");
        recent
            .record_original_update(&version, "shared-version-two")
            .expect("record family update");
        let reconciled = recent.load_entries().expect("load reconciled family");
        assert!(reconciled
            .iter()
            .filter(|entry| entry.project_root() == Some(original.as_path()))
            .all(|entry| entry.original_update_version() == Some("shared-version-two")));

        recent
            .remember(&workspace)
            .expect("reopen preserves destination");
        let reopened = recent.load_entries().expect("reopened history");
        assert_eq!(reopened[0].path(), workspace);
        assert_eq!(reopened[0].export_root(), Some(release_copy.as_path()));
        assert_eq!(reopened[0].project_root(), Some(original.as_path()));
        assert_eq!(reopened[0].source_point_ordinal(), None);
        assert!(recent
            .remember_with_export_root(&workspace, Some(Path::new("relative")))
            .is_err());
        assert_eq!(
            recent.load_entries().expect("relative refusal is no-op"),
            reopened
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn v3_export_destination_becomes_a_stable_project_label_on_upgrade() {
        let root = scratch("v3-project-label");
        let app = root.join("app");
        let workspace = root.join("workspace.mesh");
        let original = root.join("ordinary-project");
        let release_copy = root.join("release-copy");
        for path in [&app, &workspace, &original, &release_copy] {
            fs::create_dir(path).expect("test directory");
        }
        fs::set_permissions(&app, fs::Permissions::from_mode(0o700)).expect("private app");
        let workspace = fs::canonicalize(workspace).expect("canonical workspace");
        let original = fs::canonicalize(original).expect("canonical original");
        let release_copy = fs::canonicalize(release_copy).expect("canonical release copy");
        let record = app.join(RECORD_NAME);
        fs::write(
            &record,
            Json::object([
                ("schema", Json::text(SCHEMA_V3)),
                (
                    "workspaces",
                    Json::Array(vec![Json::object([
                        ("path", Json::text(workspace.to_string_lossy())),
                        ("export_root", Json::text(original.to_string_lossy())),
                    ])]),
                ),
            ])
            .encode(),
        )
        .expect("v3 record");
        fs::set_permissions(&record, fs::Permissions::from_mode(0o600)).expect("private record");

        let recent = RecentWorkspace::new(app);
        let legacy = recent.load_entries().expect("read v3 record");
        assert_eq!(legacy[0].project_root(), Some(original.as_path()));
        assert_eq!(legacy[0].source_point_ordinal(), None);
        recent
            .remember_with_export_root(&workspace, Some(&release_copy))
            .expect("upgrade while changing destination");
        let upgraded = recent.load_entries().expect("read v6 record");
        assert_eq!(upgraded[0].export_root(), Some(release_copy.as_path()));
        assert_eq!(upgraded[0].project_root(), Some(original.as_path()));
        assert_eq!(
            Json::parse(&fs::read_to_string(record).expect("read upgraded record"))
                .expect("parse upgraded record")
                .get("schema")
                .and_then(Json::as_text),
            Some(SCHEMA_V9),
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn v5_navigation_remains_readable_and_gains_no_invented_point() {
        let root = scratch("v5-point-migration");
        let app = root.join("app");
        let workspace = root.join("point-abcdef123456.mesh");
        fs::create_dir(&app).expect("app");
        fs::create_dir(&workspace).expect("workspace");
        fs::set_permissions(&app, fs::Permissions::from_mode(0o700)).expect("private app");
        let workspace = fs::canonicalize(workspace).expect("canonical workspace");
        let record = app.join(RECORD_NAME);
        fs::write(
            &record,
            Json::object([
                ("schema", Json::text(SCHEMA_V5)),
                (
                    "workspaces",
                    Json::Array(vec![Json::object([
                        ("path", Json::text(workspace.to_string_lossy())),
                        ("export_root", Json::Null),
                        ("project_root", Json::Null),
                        ("agent_handoff_installation", Json::Null),
                    ])]),
                ),
            ])
            .encode(),
        )
        .expect("v5 record");
        fs::set_permissions(&record, fs::Permissions::from_mode(0o600)).expect("private record");

        let recent = RecentWorkspace::new(app);
        let legacy = recent.load_entries().expect("read v5 record");
        assert_eq!(legacy[0].source_point_ordinal(), None);
        recent.remember(&workspace).expect("upgrade v5 record");
        assert_eq!(
            recent.load_entries().expect("read v6 record")[0].source_point_ordinal(),
            None,
            "an old private path must not become a false saved-point number"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn legacy_agent_custody_cannot_be_rewritten_without_a_physical_identity() {
        let root = scratch("legacy-agent-directory");
        let app = root.join("app");
        let assigned = root.join("assigned.mesh");
        let other = root.join("other.mesh");
        for path in [&app, &assigned, &other] {
            fs::create_dir(path).expect("test directory");
        }
        fs::set_permissions(&app, fs::Permissions::from_mode(0o700)).expect("private app");
        let assigned = fs::canonicalize(assigned).expect("canonical assigned workspace");
        let record = app.join(RECORD_NAME);
        let legacy = Json::object([
            ("schema", Json::text(SCHEMA_V7)),
            (
                "workspaces",
                Json::Array(vec![Json::object([
                    ("path", Json::text(assigned.to_string_lossy())),
                    ("export_root", Json::Null),
                    ("project_root", Json::Null),
                    (
                        "agent_handoff_installation",
                        Json::text("blake3:legacy-assignment"),
                    ),
                    ("source_point_ordinal", Json::Null),
                    ("original_update_version", Json::Null),
                ])]),
            ),
        ])
        .encode();
        fs::write(&record, &legacy).expect("legacy record");
        fs::set_permissions(&record, fs::Permissions::from_mode(0o600)).expect("private record");

        let recent = RecentWorkspace::new(app);
        let entry = recent.load_entries().expect("read legacy assignment");
        assert_eq!(
            entry[0].agent_handoff_installation(),
            Some("blake3:legacy-assignment")
        );
        assert_eq!(entry[0].agent_handoff_directory(), None);
        assert!(
            recent.remember(&other).is_err(),
            "navigation cannot publish a v8 record with invented custody"
        );
        assert_eq!(
            fs::read_to_string(&record).expect("preserved legacy record"),
            legacy
        );
        assert!(recent
            .clear_agent_handoff(&assigned, "blake3:legacy-assignment", "legacy-v8")
            .expect("explicit legacy finish remains available"));
        let cleared = recent.load_entries().expect("read upgraded clear");
        assert_eq!(cleared[0].agent_handoff_installation(), None);
        assert_eq!(cleared[0].agent_handoff_directory(), None);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn agent_handoff_cannot_bind_a_replacement_directory_to_an_older_installation() {
        let root = scratch("agent-handoff-replacement");
        let app = root.join("app");
        let workspace = root.join("workspace.mesh");
        let displaced = root.join("workspace-displaced.mesh");
        fs::create_dir(&workspace).expect("workspace");
        let recent = RecentWorkspace::new(app);
        let workspace = recent.remember(&workspace).expect("remember workspace");
        let admitted_directory = ProtectedWorkspaceRoot::inspect(&workspace)
            .expect("admitted directory identity")
            .directory_token();
        let record_before = fs::read(&recent.record).expect("record before replacement");

        fs::rename(&workspace, &displaced).expect("displace admitted workspace");
        fs::create_dir(&workspace).expect("replacement workspace");
        let error = recent
            .record_agent_handoff(
                &workspace,
                "blake3:admitted-installation",
                &admitted_directory,
            )
            .expect_err("replacement cannot inherit an older logical installation");
        assert!(
            error.to_string().contains("replaced before custody"),
            "{error}"
        );
        assert_eq!(
            fs::read(&recent.record).expect("record after replacement refusal"),
            record_before,
            "a refused replacement changed durable agent custody"
        );
        assert_eq!(
            recent.load_entries().expect("load unchanged record")[0].agent_handoff_installation(),
            None
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn exact_agent_handoff_survives_restart_without_spreading_to_other_workspaces() {
        let root = scratch("agent-handoff");
        let app = root.join("app");
        let first = root.join("first.mesh");
        let second = root.join("second.mesh");
        let unknown = root.join("unknown.mesh");
        for path in [&first, &second, &unknown] {
            fs::create_dir(path).expect("workspace");
        }
        let recent = RecentWorkspace::new(app.clone());
        let first = recent.remember(&first).expect("remember first");
        let second = recent.remember(&second).expect("remember second");
        let first_directory = ProtectedWorkspaceRoot::inspect(&first)
            .expect("first directory identity")
            .directory_token();
        let second_directory = ProtectedWorkspaceRoot::inspect(&second)
            .expect("second directory identity")
            .directory_token();
        let unknown_directory = ProtectedWorkspaceRoot::inspect(&unknown)
            .expect("unknown directory identity")
            .directory_token();

        let first_generation = recent
            .record_agent_handoff(&first, "installation-first", &first_directory)
            .expect("record exact handoff");
        assert!(recent
            .record_agent_handoff(&second, "", &second_directory)
            .is_err());
        assert!(recent
            .record_agent_handoff(&unknown, "installation-unknown", &unknown_directory)
            .is_err());

        let restarted = RecentWorkspace::new(app.clone());
        let entries = restarted.load_entries().expect("restart read");
        let first_entry = entries
            .iter()
            .find(|entry| entry.path() == first)
            .expect("first entry");
        let second_entry = entries
            .iter()
            .find(|entry| entry.path() == second)
            .expect("second entry");
        assert_eq!(
            first_entry.agent_handoff_installation(),
            Some("installation-first")
        );
        assert!(
            first_entry.agent_handoff_directory().is_some(),
            "the physical directory identity survives restart"
        );
        assert_eq!(second_entry.agent_handoff_installation(), None);
        assert!(restarted
            .clear_agent_handoff(&first, "installation-replaced", &first_generation)
            .is_err());
        assert_eq!(
            restarted
                .load_entries()
                .expect("mismatched clear is a no-op")
                .iter()
                .find(|entry| entry.path() == first)
                .and_then(RecentWorkspaceEntry::agent_handoff_installation),
            Some("installation-first")
        );
        let displaced = root.join("first-displaced.mesh");
        fs::rename(&first, &displaced).expect("move assigned workspace");
        fs::create_dir(&first).expect("replacement workspace");
        assert!(restarted
            .clear_agent_handoff(&first, "installation-first", &first_generation)
            .is_err());
        assert_eq!(
            restarted
                .load_entries()
                .expect("replacement refusal is a no-op")
                .iter()
                .find(|entry| entry.path() == first)
                .and_then(RecentWorkspaceEntry::agent_handoff_installation),
            Some("installation-first")
        );
        fs::remove_dir(&first).expect("remove replacement");
        fs::rename(&displaced, &first).expect("restore assigned workspace");
        assert!(restarted
            .clear_agent_handoff(&first, "installation-first", &first_generation)
            .expect("clear exact handoff"));
        assert!(!restarted
            .clear_agent_handoff(&first, "installation-first", &first_generation)
            .expect("repeat clear is a no-op"));
        assert_eq!(
            RecentWorkspace::new(app.clone())
                .load_entries()
                .expect("cleared handoff survives restart")
                .iter()
                .find(|entry| entry.path() == first)
                .and_then(RecentWorkspaceEntry::agent_handoff_installation),
            None
        );
        assert_eq!(
            Json::parse(&fs::read_to_string(restarted.record).expect("read handoff record"))
                .expect("parse handoff record")
                .get("schema")
                .and_then(Json::as_text),
            Some(SCHEMA_V9)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn concurrent_process_handles_allow_only_one_first_agent_handoff() {
        let root = scratch("concurrent-agent-handoff");
        let app = root.join("app");
        let workspace = root.join("workspace.mesh");
        fs::create_dir(&workspace).expect("workspace");
        let admitted = RecentWorkspace::new(app.clone())
            .remember(&workspace)
            .expect("remember workspace");
        let directory = ProtectedWorkspaceRoot::inspect(&admitted)
            .expect("workspace directory identity")
            .directory_token();
        let barrier = Arc::new(Barrier::new(3));
        let mut workers = Vec::new();
        for _ in 0..2 {
            let recent = RecentWorkspace::new(app.clone());
            let workspace = admitted.clone();
            let directory = directory.clone();
            let barrier = Arc::clone(&barrier);
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                recent.record_agent_handoff(&workspace, "installation-concurrent", &directory)
            }));
        }
        barrier.wait();
        let outcomes: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().expect("handoff worker"))
            .collect();
        assert_eq!(
            outcomes.iter().filter(|outcome| outcome.is_ok()).count(),
            1,
            "two first acquisitions reached the same writable agent folder: {outcomes:?}",
        );
        assert!(
            outcomes
                .iter()
                .filter_map(|outcome| outcome.as_ref().err())
                .all(|error| {
                    let message = error.to_string();
                    message.contains("already assigned")
                }),
            "the losing acquisition did not fail closed: {outcomes:?}",
        );
        let first_generation = outcomes
            .iter()
            .find_map(|outcome| outcome.as_ref().ok())
            .cloned()
            .expect("one successful first generation");

        let restarted = RecentWorkspace::new(app);
        assert!(
            restarted
                .record_agent_handoff(&admitted, "installation-concurrent", &directory,)
                .is_err(),
            "a fresh process handle silently reopened an assigned folder",
        );
        let reopened_generation = restarted
            .record_agent_handoff_with_reopen(
                &admitted,
                "installation-concurrent",
                &directory,
                true,
                Some(&first_generation),
            )
            .expect("explicitly confirmed reopen");
        assert_ne!(reopened_generation, first_generation);
        assert_eq!(
            restarted.load_entries().expect("reload custody")[0].agent_handoff_installation(),
            Some("installation-concurrent"),
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn managed_mutation_and_agent_handoff_are_linearized_in_both_orders() {
        use std::sync::mpsc;
        use std::time::Duration;

        let root = scratch("managed-mutation-agent-handoff");
        let app = root.join("app");
        let workspace = root.join("workspace.mesh");
        fs::create_dir(&workspace).expect("workspace");
        let mutating = RecentWorkspace::new(app.clone());
        let admitted = mutating.remember(&workspace).expect("remember workspace");
        let installation = "installation-linearized";
        let directory = ProtectedWorkspaceRoot::inspect(&admitted)
            .expect("workspace directory identity")
            .directory_token();

        // Mutation wins: custody acquisition reaches the shared lock but cannot publish until the
        // managed mutation guard is dropped.
        let mutation = mutating
            .lock_managed_workspace_mutation(&admitted, installation)
            .expect("unassigned workspace mutation guard");
        let acquiring = RecentWorkspace::new(app.clone());
        let acquired_workspace = admitted.clone();
        let acquired_directory = directory.clone();
        let (started_tx, started_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            started_tx.send(()).expect("announce acquisition attempt");
            let result = acquiring.record_agent_handoff(
                &acquired_workspace,
                installation,
                &acquired_directory,
            );
            finished_tx.send(result).expect("return acquisition result");
        });
        started_rx.recv().expect("acquisition thread started");
        assert!(
            finished_rx
                .recv_timeout(Duration::from_millis(100))
                .is_err(),
            "agent custody published while a managed mutation guard was active",
        );
        drop(mutation);
        finished_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("acquisition completes after mutation")
            .expect("custody succeeds after mutation");
        thread.join().expect("acquisition thread");

        // Custody wins: a stale process cannot enter any managed mutation closure for the same
        // remembered workspace until that exact assignment is finished.
        let refusal = RecentWorkspace::new(app.clone())
            .lock_managed_workspace_mutation(&admitted, installation)
            .expect_err("active custody must refuse managed mutation");
        assert!(
            refusal.to_string().contains("assigned to an agent"),
            "{refusal}"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn renamed_agent_workspace_keeps_custody_when_remembered_at_its_new_path() {
        let root = scratch("renamed-agent-workspace-mutation");
        let app = root.join("app");
        let old_path = root.join("old.mesh");
        let new_path = root.join("new.mesh");
        fs::create_dir(&old_path).expect("workspace");
        let recent = RecentWorkspace::new(app.clone());
        let admitted = recent.remember(&old_path).expect("remember old path");
        let installation = "installation-renamed-agent-workspace";
        let directory = ProtectedWorkspaceRoot::inspect(&admitted)
            .expect("workspace directory identity")
            .directory_token();
        let generation = recent
            .record_agent_handoff(&admitted, installation, &directory)
            .expect("agent custody");

        fs::rename(&old_path, &new_path).expect("rename assigned workspace");
        let physical_refusal = RecentWorkspace::new(app.clone())
            .lock_managed_workspace_mutation(&new_path, installation)
            .expect_err("directory identity must preserve custody before navigation catches up");
        assert!(
            physical_refusal
                .to_string()
                .contains("assigned to an agent"),
            "{physical_refusal}"
        );
        let remembered = RecentWorkspace::new(app.clone())
            .remember(&new_path)
            .expect("remember renamed workspace");
        let new_path = fs::canonicalize(&new_path).expect("canonical renamed workspace");
        assert_eq!(remembered, new_path);
        let restarted = RecentWorkspace::new(app.clone());
        let entries = restarted
            .load_entries()
            .expect("renamed restart navigation");
        assert_eq!(entries.len(), 1, "old pathname retained duplicate custody");
        assert_eq!(entries[0].path(), new_path);
        assert_eq!(entries[0].agent_handoff_installation(), Some(installation));
        assert_eq!(
            entries[0].agent_handoff_directory(),
            Some(directory.as_str())
        );
        assert_eq!(
            entries[0].agent_handoff_generation(),
            Some(generation.as_str())
        );

        let refusal = RecentWorkspace::new(app)
            .lock_managed_workspace_mutation(&new_path, installation)
            .expect_err("renamed assigned workspace must still refuse mutation");
        assert!(
            refusal.to_string().contains("assigned to an agent"),
            "{refusal}"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn stale_confirmed_reopen_cannot_replace_newer_agent_custody() {
        let root = scratch("stale-confirmed-agent-reopen");
        let app = root.join("app");
        let workspace = root.join("workspace.mesh");
        fs::create_dir(&workspace).expect("workspace");
        let first_process = RecentWorkspace::new(app.clone());
        let admitted = first_process
            .remember(&workspace)
            .expect("remember workspace");
        let directory = ProtectedWorkspaceRoot::inspect(&admitted)
            .expect("workspace directory identity")
            .directory_token();
        let first_generation = first_process
            .record_agent_handoff(&admitted, "installation-stale-reopen", &directory)
            .expect("first custody");

        let second_process = RecentWorkspace::new(app.clone());
        let second_generation = second_process
            .record_agent_handoff_with_reopen(
                &admitted,
                "installation-stale-reopen",
                &directory,
                true,
                Some(&first_generation),
            )
            .expect("newer confirmed custody");
        assert_ne!(second_generation, first_generation);

        assert!(
            first_process
                .record_agent_handoff_with_reopen(
                    &admitted,
                    "installation-stale-reopen",
                    &directory,
                    true,
                    Some(&first_generation),
                )
                .is_err(),
            "a confirmation captured for the first custody replaced the newer assignment",
        );
        assert_eq!(
            RecentWorkspace::new(app)
                .load_entries()
                .expect("newer custody remains")[0]
                .agent_handoff_generation(),
            Some(second_generation.as_str()),
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn malformed_shared_and_linked_records_fail_closed() {
        let root = scratch("refusals");
        let app = root.join("app");
        fs::create_dir(&app).expect("app");
        fs::set_permissions(&app, fs::Permissions::from_mode(0o700)).expect("secure app");
        let record = app.join(RECORD_NAME);
        fs::write(&record, br#"{"schema":"wrong","path":"relative"}"#).expect("write bad");
        fs::set_permissions(&record, fs::Permissions::from_mode(0o600)).expect("secure bad");
        let recent = RecentWorkspace::new(app.clone());
        assert!(recent.load_all().is_err());

        let workspace = root.join("workspace");
        fs::create_dir(&workspace).expect("workspace");
        fs::write(
            &record,
            Json::object([
                ("schema", Json::text(SCHEMA_V3)),
                (
                    "workspaces",
                    Json::Array(vec![Json::object([
                        ("path", Json::text(workspace.to_string_lossy())),
                        ("export_root", Json::text("relative-destination")),
                    ])]),
                ),
            ])
            .encode(),
        )
        .expect("relative export destination");
        assert!(recent.load_entries().is_err());

        fs::write(
            &record,
            Json::object([
                ("schema", Json::text(SCHEMA_V6)),
                (
                    "workspaces",
                    Json::Array(vec![Json::object([
                        ("path", Json::text(workspace.to_string_lossy())),
                        ("export_root", Json::Null),
                        ("project_root", Json::Null),
                        ("agent_handoff_installation", Json::Null),
                        ("source_point_ordinal", Json::Number(0)),
                    ])]),
                ),
            ])
            .encode(),
        )
        .expect("zero source point ordinal");
        assert!(recent.load_entries().is_err());

        fs::set_permissions(&record, fs::Permissions::from_mode(0o644)).expect("share bad");
        assert!(recent.load_all().is_err());

        fs::remove_file(&record).expect("remove bad");
        let target = root.join("target");
        fs::write(&target, "anything").expect("target");
        symlink(&target, &record).expect("link record");
        assert!(recent.load_all().is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn verified_reselection_repairs_content_but_never_record_authority() {
        let root = scratch("verified-repair");
        let app = root.join("app");
        let workspace = root.join("workspace");
        fs::create_dir(&app).expect("app");
        fs::set_permissions(&app, fs::Permissions::from_mode(0o700)).expect("secure app");
        fs::create_dir(&workspace).expect("workspace");
        let record = app.join(RECORD_NAME);
        fs::write(&record, "malformed navigation content").expect("malformed record");
        fs::set_permissions(&record, fs::Permissions::from_mode(0o600)).expect("private record");
        let recent = RecentWorkspace::new(app.clone());

        assert!(recent.remember(&workspace).is_err());
        let (canonical, repaired) = recent
            .remember_repairing_invalid_content_at_point(&workspace, None, None, None)
            .expect("verified repair");
        assert!(repaired);
        assert_eq!(
            recent.load_all().expect("repaired history"),
            vec![canonical]
        );

        fs::write(&record, "shared malformed record").expect("shared record");
        fs::set_permissions(&record, fs::Permissions::from_mode(0o644)).expect("share record");
        let before = fs::read(&record).unwrap();
        assert!(recent
            .remember_repairing_invalid_content_at_point(&workspace, None, None, None)
            .is_err());
        assert_eq!(fs::read(&record).unwrap(), before);

        fs::remove_file(&record).expect("remove shared record");
        let outside = root.join("outside");
        fs::write(&outside, "outside must remain").expect("outside");
        symlink(&outside, &record).expect("linked record");
        assert!(recent
            .remember_repairing_invalid_content_at_point(&workspace, None, None, None)
            .is_err());
        assert_eq!(fs::read_to_string(&outside).unwrap(), "outside must remain");
        assert!(fs::symlink_metadata(&record)
            .unwrap()
            .file_type()
            .is_symlink());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn verified_reselection_never_repairs_away_malformed_active_agent_custody() {
        let root = scratch("malformed-active-agent-custody");
        let app = root.join("app");
        let workspace = root.join("workspace");
        fs::create_dir(&workspace).expect("workspace");
        let recent = RecentWorkspace::new(app.clone());
        let admitted = recent.remember(&workspace).expect("remember workspace");
        let directory = ProtectedWorkspaceRoot::inspect(&admitted)
            .expect("workspace directory identity")
            .directory_token();
        recent
            .record_agent_handoff(&admitted, "installation-malformed-custody", &directory)
            .expect("agent custody");
        let record = app.join(RECORD_NAME);
        let malformed = fs::read_to_string(&record)
            .expect("custody record")
            .replace(&directory, "malformed-active-directory-token");
        fs::write(&record, &malformed).expect("malformed custody identity");

        let refusal = recent
            .remember_repairing_invalid_content_at_point(&admitted, None, None, None)
            .expect_err("verified navigation must not erase malformed active custody");
        assert!(refusal.to_string().contains("handoff directory is invalid"));
        assert_eq!(
            fs::read_to_string(&record).expect("unchanged malformed record"),
            malformed,
            "repair erased or rewrote the only active-custody warning",
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn verified_reselection_never_erases_custody_hidden_by_a_renamed_container_key() {
        let root = scratch("renamed-active-custody-container");
        let app = root.join("app");
        let workspace = root.join("workspace");
        fs::create_dir(&workspace).expect("workspace");
        let recent = RecentWorkspace::new(app.clone());
        let admitted = recent.remember(&workspace).expect("remember workspace");
        let directory = ProtectedWorkspaceRoot::inspect(&admitted)
            .expect("workspace directory identity")
            .directory_token();
        recent
            .record_agent_handoff(&admitted, "installation-hidden-custody", &directory)
            .expect("agent custody");
        let record = app.join(RECORD_NAME);
        let malformed = fs::read_to_string(&record)
            .expect("custody record")
            .replace("\"workspaces\":", "\"workspacez\":");
        fs::write(&record, &malformed).expect("rename custody container key");

        let refusal = recent
            .remember_repairing_invalid_content_at_point(&admitted, None, None, None)
            .expect_err("navigation repair must find custody outside the expected container");
        assert!(refusal.to_string().contains("workspace list"));
        assert_eq!(
            fs::read_to_string(&record).expect("unchanged malformed record"),
            malformed,
            "repair erased active custody hidden behind a malformed container key",
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn shared_or_linked_application_data_fails_closed() {
        let root = scratch("application-data-refusals");
        let private = root.join("private");
        let workspace = root.join("workspace");
        fs::create_dir(&workspace).expect("workspace");
        let recent = RecentWorkspace::new(private.clone());
        recent.remember(&workspace).expect("remember");

        fs::set_permissions(&private, fs::Permissions::from_mode(0o755)).expect("share app data");
        assert!(recent.load_all().is_err());

        fs::set_permissions(&private, fs::Permissions::from_mode(0o700)).expect("secure app data");
        let linked = root.join("linked");
        symlink(&private, &linked).expect("link app data");
        assert!(RecentWorkspace::new(linked).load_all().is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn forget_requires_the_exact_remembered_workspace() {
        let root = scratch("forget");
        let app = root.join("app");
        let workspace = root.join("workspace");
        let other = root.join("other");
        let unknown = root.join("unknown");
        fs::create_dir(&workspace).expect("workspace");
        fs::create_dir(&other).expect("other");
        fs::create_dir(&unknown).expect("unknown");
        let recent = RecentWorkspace::new(app);
        let other_canonical = recent.remember(&other).expect("remember other");
        let canonical = recent.remember(&workspace).expect("remember");

        assert!(!recent.forget_if_matches(&unknown).expect("wrong path"));
        assert_eq!(recent.load_all().expect("still remembered")[0], canonical);
        assert!(recent.forget_if_matches(&canonical).expect("exact path"));
        assert_eq!(
            recent.load_all().expect("fallback remembered"),
            vec![other_canonical.clone()]
        );
        assert!(recent
            .forget_if_matches(&other_canonical)
            .expect("forget final path"));
        assert!(recent.load_all().expect("forgotten").is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn forget_refuses_an_agent_assigned_workspace_until_the_handoff_is_cleared() {
        let root = scratch("forget-agent-assigned");
        let app = root.join("app");
        let workspace = root.join("workspace");
        fs::create_dir(&workspace).expect("workspace");
        let recent = RecentWorkspace::new(app);
        let canonical = recent.remember(&workspace).expect("remember");
        let directory = ProtectedWorkspaceRoot::inspect(&canonical)
            .expect("active directory identity")
            .directory_token();
        let generation = recent
            .record_agent_handoff(&canonical, "installation-active", &directory)
            .expect("record handoff");
        let record_before = fs::read(&recent.record).expect("record before refusal");

        let error = recent
            .forget_if_matches(&canonical)
            .expect_err("active handoff must retain recent custody");
        assert!(error.to_string().contains("still assigned to an agent"));
        assert_eq!(
            fs::read(&recent.record).expect("record after refusal"),
            record_before,
            "a refused forget changed the durable custody record"
        );
        assert_eq!(
            recent.load_entries().expect("retained entries")[0].agent_handoff_installation(),
            Some("installation-active")
        );

        assert!(recent
            .clear_agent_handoff(&canonical, "installation-active", &generation)
            .expect("clear handoff"));
        assert!(recent
            .forget_if_matches(&canonical)
            .expect("forget after release"));
        assert!(recent.load_all().expect("forgotten").is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn stale_release_cannot_clear_a_new_handoff_for_the_same_installation() {
        let root = scratch("agent-handoff-generation");
        let application = root.join("app");
        let workspace = root.join("workspace");
        fs::create_dir(&workspace).expect("workspace");
        let first_process = RecentWorkspace::new(application.clone());
        let second_process = RecentWorkspace::new(application);
        let canonical = first_process
            .remember(&workspace)
            .expect("remember workspace");
        let directory = ProtectedWorkspaceRoot::inspect(&canonical)
            .expect("workspace directory identity")
            .directory_token();

        let first_generation = first_process
            .record_agent_handoff(&canonical, "installation-shared", &directory)
            .expect("first acquisition");
        assert!(second_process
            .clear_agent_handoff(&canonical, "installation-shared", &first_generation,)
            .expect("second process releases first acquisition"));
        let second_generation = second_process
            .record_agent_handoff(&canonical, "installation-shared", &directory)
            .expect("second acquisition");
        assert_ne!(first_generation, second_generation);

        let stale = first_process
            .clear_agent_handoff(&canonical, "installation-shared", &first_generation)
            .expect_err("stale release must not clear the second acquisition");
        assert!(stale.to_string().contains("generation"), "{stale}");
        let retained = second_process
            .load_entries()
            .expect("retained second acquisition");
        assert_eq!(
            retained[0].agent_handoff_installation(),
            Some("installation-shared")
        );
        assert_eq!(
            retained[0].agent_handoff_generation(),
            Some(second_generation.as_str())
        );

        second_process
            .clear_agent_handoff(&canonical, "installation-shared", &second_generation)
            .expect("release second acquisition");
        second_process
            .forget_if_matches(&canonical)
            .expect("forget released workspace");
        let canonical = second_process
            .remember(&workspace)
            .expect("remember the same physical workspace again");
        let third_generation = second_process
            .record_agent_handoff(&canonical, "installation-shared", &directory)
            .expect("acquire after forget and remember");
        assert!(
            first_process
                .clear_agent_handoff(
                    &canonical,
                    "installation-shared",
                    &first_generation,
                )
                .is_err(),
            "an old release cleared a same-path reacquisition after the history entry was recreated",
        );
        assert_ne!(third_generation, first_generation);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn inactive_generation_tombstone_is_not_projected_or_reused() {
        let root = scratch("agent-handoff-inactive-tombstone");
        let application = root.join("app");
        let workspace = root.join("workspace");
        fs::create_dir(&workspace).expect("workspace");
        let recent = RecentWorkspace::new(application);
        let canonical = recent.remember(&workspace).expect("remember workspace");
        let directory = ProtectedWorkspaceRoot::inspect(&canonical)
            .expect("workspace directory identity")
            .directory_token();
        let first = recent
            .record_agent_handoff(&canonical, "installation", &directory)
            .expect("first acquisition");

        recent
            .reconcile_agent_handoff_from_authority(
                &canonical,
                "installation",
                &directory,
                |projected_active| {
                    assert!(projected_active);
                    Ok(None)
                },
            )
            .expect("project inactive authority");
        let inactive = recent.load_entries().expect("inactive projection");
        assert_eq!(inactive[0].agent_handoff_installation(), None);
        assert_eq!(inactive[0].agent_handoff_directory(), None);
        assert_eq!(inactive[0].agent_handoff_generation(), Some(first.as_str()));

        let second = recent
            .record_agent_handoff_with_authority(
                &canonical,
                "installation",
                &directory,
                false,
                None,
                || Ok("0123456789abcdef0123456789abcdef".to_owned()),
            )
            .expect("fresh acquisition replaces tombstone");
        assert_ne!(second, first);
        assert!(recent
            .clear_agent_handoff(&canonical, "installation", &first)
            .is_err());
        let active = recent.load_entries().expect("active projection");
        assert_eq!(active[0].agent_handoff_installation(), Some("installation"));
        assert_eq!(active[0].agent_handoff_generation(), Some(second.as_str()));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn bounded_history_never_evicts_an_agent_assigned_workspace() {
        let root = scratch("bounded-agent-custody");
        let recent = RecentWorkspace::new(root.join("app"));
        let mut assigned = Vec::new();
        for index in 0..MAX_RECENT_WORKSPACES {
            let workspace = root.join(format!("assigned-{index}"));
            fs::create_dir(&workspace).expect("assigned workspace");
            let canonical = recent.remember(&workspace).expect("remember assigned");
            let directory = ProtectedWorkspaceRoot::inspect(&canonical)
                .expect("assigned directory identity")
                .directory_token();
            let generation = recent
                .record_agent_handoff(&canonical, &format!("installation-{index}"), &directory)
                .expect("record assigned handoff");
            assigned.push((canonical, generation));
        }
        let record_before = fs::read(&recent.record).expect("record before capacity refusal");
        let next = root.join("next-workspace");
        fs::create_dir(&next).expect("next workspace");

        assert!(recent.ensure_can_remember_another().is_err());
        recent
            .ensure_can_remember(&assigned[0].0)
            .expect("an already tracked agent workspace consumes no new slot");
        let error = recent
            .remember(&next)
            .expect_err("active agent custody must win over bounded navigation");
        assert!(error.to_string().contains("active agent"), "{error}");
        assert_eq!(
            fs::read(&recent.record).expect("record after capacity refusal"),
            record_before,
            "capacity refusal rewrote the active-agent custody record"
        );
        let retained = recent.load_entries().expect("retained assigned workspaces");
        assert_eq!(retained.len(), MAX_RECENT_WORKSPACES);
        for (path, _) in &assigned {
            assert!(retained.iter().any(|entry| {
                entry.path() == *path && entry.agent_handoff_installation().is_some()
            }));
        }

        recent
            .clear_agent_handoff(&assigned[0].0, "installation-0", &assigned[0].1)
            .expect("release oldest handoff");
        let remembered_next = recent.remember(&next).expect("reuse released capacity");
        let after_release = recent.load_entries().expect("history after release");
        assert_eq!(after_release.len(), MAX_RECENT_WORKSPACES);
        assert_eq!(after_release[0].path(), remembered_next);
        assert!(!after_release
            .iter()
            .any(|entry| entry.path() == assigned[0].0));
        for (path, _) in &assigned[1..] {
            assert!(after_release.iter().any(|entry| {
                entry.path() == *path && entry.agent_handoff_installation().is_some()
            }));
        }
        let _ = fs::remove_dir_all(root);
    }
}
