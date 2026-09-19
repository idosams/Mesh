//! Project-scoped Codex context for one exact native Mesh workspace.

use std::fmt;
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::Write as _;
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const CONFIG_DIRECTORY: &str = ".codex";
const CONFIG_FILE: &str = "config.toml";
const PRIVATE_INTEGRATION_DIRECTORY: &str = "integrations/codex";
const LEGACY_PROJECT_LINK_TARGET: &str = "../integrations/codex";

/// Result of ensuring that Codex can start the bundled read-only Mesh bridge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodexProjectConfig {
    /// Mesh created a new private compatibility configuration for a legacy project link.
    Created,
    /// Mesh refreshed its own private binding for a later agent session.
    Refreshed,
    /// The exact expected configuration already existed.
    Current,
}

/// Exact command-line configuration supplied to `codex app` for one agent session.
///
/// Codex currently loads MCP servers from its user configuration and explicit `-c` overrides;
/// merely placing a `config.toml` below the opened project is not sufficient. Keeping these
/// values separate from the persisted compatibility file lets the desktop make the active
/// session truthful without changing global Codex settings.
pub fn codex_launch_overrides(
    executable: &Path,
    endpoint: &Path,
    workspace: &Path,
    workspace_installation: &str,
) -> Result<Vec<String>, CodexProjectConfigError> {
    let settings = codex_settings(executable, endpoint, workspace, workspace_installation)?;
    Ok(vec![
        format!("mcp_servers.mesh.command={}", settings.command),
        format!("mcp_servers.mesh.args=[{}]", settings.args.join(", ")),
        "mcp_servers.mesh.required=false".to_owned(),
        "mcp_servers.mesh.enabled_tools=[\"mesh_workspace_state\"]".to_owned(),
        "mcp_servers.mesh.default_tools_approval_mode=\"auto\"".to_owned(),
        "mcp_servers.mesh.startup_timeout_sec=5".to_owned(),
        "mcp_servers.mesh.tool_timeout_sec=5".to_owned(),
    ])
}

/// Refresh an exact legacy project-scoped Mesh MCP configuration without touching global Codex
/// settings or making new machine-local configuration part of workspace content.
///
/// Current Codex sessions receive the binding through [`codex_launch_overrides`]. New native
/// workspaces therefore remain byte-for-byte ordinary project folders: Mesh does not add a
/// `.codex` entry that would make a Git checkout appear dirty before the agent changes anything.
/// An exact link created by an older Mesh alpha remains supported and is refreshed in place.
#[cfg(test)]
pub fn ensure_codex_project_config(
    workspace: &Path,
    executable: &Path,
    endpoint: &Path,
    workspace_installation: &str,
) -> Result<CodexProjectConfig, CodexProjectConfigError> {
    let workspace = fs::canonicalize(workspace)
        .map_err(|source| CodexProjectConfigError::io("verify workspace", source))?;
    let storage = workspace
        .parent()
        .ok_or(CodexProjectConfigError::Invalid(
            "native workspace has no private storage parent",
        ))?
        .to_path_buf();
    ensure_codex_project_config_at_references(
        &workspace,
        &workspace,
        &storage,
        executable,
        endpoint,
        workspace_installation,
    )
}

/// Refresh legacy private Codex context through descriptor-derived stable directory references.
///
/// `workspace_display` is used only to prove the expected Mesh layout name. All inspection and
/// writes use the admitted workspace/storage identities, so a replacement at the displayed path
/// cannot redirect private configuration or the workspace authority encoded into it.
pub fn ensure_codex_project_config_at_references(
    workspace_display: &Path,
    workspace: &Path,
    storage: &Path,
    executable: &Path,
    endpoint: &Path,
    workspace_installation: &str,
) -> Result<CodexProjectConfig, CodexProjectConfigError> {
    validate_absolute_file(executable, "Mesh application executable")?;
    if !endpoint.is_absolute() {
        return Err(CodexProjectConfigError::Invalid(
            "Mesh daemon endpoint is not absolute",
        ));
    }
    validate_real_directory(workspace, "workspace")?;
    if !workspace_display
        .file_name()
        .is_some_and(mesh_daemon::workspace::is_presented_directory_name)
    {
        return Err(CodexProjectConfigError::Invalid(
            "workspace is not an isolated native Mesh presentation",
        ));
    }
    validate_real_directory(storage, "private workspace storage")?;
    // The daemon protocol identifies this workspace by its canonical logical root. Stable
    // references are exclusively filesystem capabilities: persisting one as the protocol root
    // would make a healthy daemon state look like a different workspace.
    let expected = encode_config(
        executable,
        endpoint,
        workspace_display,
        workspace_installation,
    )?;
    let project_link = workspace.join(CONFIG_DIRECTORY);
    match fs::symlink_metadata(&project_link) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            if fs::read_link(&project_link).map_err(|source| {
                CodexProjectConfigError::io("inspect project .codex link", source)
            })? != Path::new(LEGACY_PROJECT_LINK_TARGET)
            {
                return Err(CodexProjectConfigError::ExistingConfig(
                    project_link.join(CONFIG_FILE),
                ));
            }
        }
        Ok(_) => {
            return Err(CodexProjectConfigError::ExistingConfig(
                project_link.join(CONFIG_FILE),
            ));
        }
        // Explicit launch overrides are the current authority. Do not create an otherwise
        // untracked project entry merely to persist a compatibility file Codex does not load.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CodexProjectConfig::Current);
        }
        Err(source) => {
            return Err(CodexProjectConfigError::io(
                "inspect project .codex path",
                source,
            ));
        }
    }

    let private_directory = storage.join(PRIVATE_INTEGRATION_DIRECTORY);
    create_private_directories(storage, &private_directory)?;
    let config = private_directory.join(CONFIG_FILE);
    let mut created = false;
    let mut refreshed = false;
    match fs::symlink_metadata(&config) {
        Ok(metadata) => {
            if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
                return Err(CodexProjectConfigError::Invalid(
                    "private Codex configuration is not a regular file",
                ));
            }
            let current = fs::read(&config).map_err(|source| {
                CodexProjectConfigError::io("read private Codex configuration", source)
            })?;
            if current != expected.as_bytes() {
                write_config_atomically(&private_directory, &config, expected.as_bytes())?;
                refreshed = true;
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            write_config_atomically(&private_directory, &config, expected.as_bytes())?;
            created = true;
        }
        Err(source) => {
            return Err(CodexProjectConfigError::io(
                "inspect private Codex configuration",
                source,
            ));
        }
    }
    let workspace_parent = fs::metadata(workspace.join(".."))
        .map_err(|source| CodexProjectConfigError::io("inspect workspace parent", source))?;
    let storage_metadata = fs::metadata(storage).map_err(|source| {
        CodexProjectConfigError::io("inspect private workspace storage", source)
    })?;
    if (workspace_parent.dev(), workspace_parent.ino())
        != (storage_metadata.dev(), storage_metadata.ino())
    {
        return Err(CodexProjectConfigError::Invalid(
            "native workspace no longer belongs to its admitted private storage",
        ));
    }
    validate_project_link(&project_link, &private_directory)?;
    Ok(if created {
        CodexProjectConfig::Created
    } else if refreshed {
        CodexProjectConfig::Refreshed
    } else {
        CodexProjectConfig::Current
    })
}

fn create_private_directories(
    storage: &Path,
    target: &Path,
) -> Result<(), CodexProjectConfigError> {
    let integrations = storage.join("integrations");
    for (directory, context) in [
        (
            integrations.as_path(),
            "create private integrations directory",
        ),
        (target, "create private Codex directory"),
    ] {
        match fs::symlink_metadata(directory) {
            Ok(_) => validate_real_directory(directory, "private integration directory")?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                DirBuilder::new()
                    .mode(0o700)
                    .create(directory)
                    .map_err(|source| CodexProjectConfigError::io(context, source))?;
                fs::File::open(directory.parent().expect("private directory parent"))
                    .and_then(|parent| parent.sync_all())
                    .map_err(|source| CodexProjectConfigError::io(context, source))?;
            }
            Err(source) => return Err(CodexProjectConfigError::io(context, source)),
        }
    }
    Ok(())
}

fn write_config_atomically(
    directory: &Path,
    destination: &Path,
    bytes: &[u8],
) -> Result<(), CodexProjectConfigError> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| CodexProjectConfigError::Invalid("system clock predates Unix time"))?
        .as_nanos();
    let temporary = directory.join(format!("config.toml.next-{}-{nonce}", std::process::id()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&temporary)
        .map_err(|source| {
            CodexProjectConfigError::io("create private Codex configuration", source)
        })?;
    let result = file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .and_then(|()| fs::rename(&temporary, destination))
        .and_then(|()| fs::File::open(directory).and_then(|parent| parent.sync_all()));
    if let Err(source) = result {
        let _ = fs::remove_file(&temporary);
        return Err(CodexProjectConfigError::io(
            "persist private Codex configuration",
            source,
        ));
    }
    Ok(())
}

fn validate_project_link(link: &Path, private: &Path) -> Result<(), CodexProjectConfigError> {
    // Do not canonicalize a macOS `/.vol/<device>/<inode>` reference: lexical cleanup of `..`
    // happens before the magic vnode is resolved and can name the wrong directory. Following the
    // already-inspected exact relative symlink and comparing physical identities stays bound to
    // the admitted workspace and private storage even after their displayed namespace is moved.
    let resolved = fs::metadata(link)
        .map_err(|source| CodexProjectConfigError::io("resolve project .codex link", source))?;
    let private = fs::metadata(private).map_err(|source| {
        CodexProjectConfigError::io("resolve private Codex configuration", source)
    })?;
    if (resolved.dev(), resolved.ino()) != (private.dev(), private.ino()) {
        return Err(CodexProjectConfigError::Invalid(
            "project .codex link no longer names Mesh private configuration",
        ));
    }
    Ok(())
}

fn validate_absolute_file(path: &Path, label: &'static str) -> Result<(), CodexProjectConfigError> {
    if !path.is_absolute() {
        return Err(CodexProjectConfigError::Invalid(match label {
            "Mesh application executable" => "Mesh application executable is not absolute",
            _ => "required file is not absolute",
        }));
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|source| CodexProjectConfigError::io("inspect Mesh executable", source))?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(CodexProjectConfigError::Invalid(
            "Mesh application executable is not a regular file",
        ));
    }
    Ok(())
}

fn validate_real_directory(
    path: &Path,
    label: &'static str,
) -> Result<(), CodexProjectConfigError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|source| CodexProjectConfigError::io("inspect directory", source))?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(CodexProjectConfigError::Invalid(match label {
            "workspace" => "workspace is not a real directory",
            _ => "project .codex path is not a real directory",
        }));
    }
    Ok(())
}

fn encode_config(
    executable: &Path,
    endpoint: &Path,
    workspace: &Path,
    installation: &str,
) -> Result<String, CodexProjectConfigError> {
    let settings = codex_settings(executable, endpoint, workspace, installation)?;
    Ok(format!(
        "# Managed by Mesh for this exact native workspace version.\n\
[mcp_servers.mesh]\n\
command = {}\n\
args = [{}]\n\
# The native workspace must remain usable when Mesh is closed. This bridge is read-only context,
# so an unavailable daemon removes the tool for that session instead of blocking Codex startup.
required = false\n\
enabled_tools = [\"mesh_workspace_state\"]\n\
default_tools_approval_mode = \"auto\"\n\
startup_timeout_sec = 5\n\
tool_timeout_sec = 5\n",
        settings.command,
        settings.args.join(", "),
    ))
}

struct CodexSettings {
    command: String,
    args: Vec<String>,
}

fn codex_settings(
    executable: &Path,
    endpoint: &Path,
    workspace: &Path,
    installation: &str,
) -> Result<CodexSettings, CodexProjectConfigError> {
    validate_absolute_file(executable, "Mesh application executable")?;
    if !endpoint.is_absolute() {
        return Err(CodexProjectConfigError::Invalid(
            "Mesh daemon endpoint is not absolute",
        ));
    }
    let endpoint = endpoint.to_str().ok_or(CodexProjectConfigError::Invalid(
        "Mesh daemon endpoint is not valid UTF-8",
    ))?;
    let workspace = workspace.to_str().ok_or(CodexProjectConfigError::Invalid(
        "workspace path is not valid UTF-8",
    ))?;
    let executable = executable.to_str().ok_or(CodexProjectConfigError::Invalid(
        "Mesh application executable path is not valid UTF-8",
    ))?;
    let values = [
        "--mesh-mcp",
        "--endpoint",
        endpoint,
        "--expected-workspace-root",
        workspace,
        "--expected-workspace-installation",
        installation,
    ];
    let args = values
        .iter()
        .map(|value| toml_string(value))
        .collect::<Vec<_>>();
    Ok(CodexSettings {
        command: toml_string(executable),
        args,
    })
}

fn toml_string(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len() + 2);
    encoded.push('"');
    for character in value.chars() {
        match character {
            '\\' => encoded.push_str("\\\\"),
            '"' => encoded.push_str("\\\""),
            '\n' => encoded.push_str("\\n"),
            '\r' => encoded.push_str("\\r"),
            '\t' => encoded.push_str("\\t"),
            character if character.is_control() => {
                use std::fmt::Write as _;
                write!(encoded, "\\u{:04X}", character as u32).expect("string write");
            }
            character => encoded.push(character),
        }
    }
    encoded.push('"');
    encoded
}

/// Why Mesh could not install its project-scoped Codex bridge.
#[derive(Debug)]
pub enum CodexProjectConfigError {
    Invalid(&'static str),
    ExistingConfig(PathBuf),
    Io {
        context: &'static str,
        source: std::io::Error,
    },
}

impl CodexProjectConfigError {
    fn io(context: &'static str, source: std::io::Error) -> Self {
        Self::Io { context, source }
    }
}

impl fmt::Display for CodexProjectConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(reason) => write!(formatter, "Codex setup refused: {reason}"),
            Self::ExistingConfig(path) => write!(
                formatter,
                "Codex setup refused: {} already contains different settings. Mesh did not overwrite them; add the Mesh server there explicitly or use Open agent terminal.",
                path.display()
            ),
            Self::Io { context, source } => {
                write!(formatter, "Codex setup refused: {context}: {source}")
            }
        }
    }
}

impl std::error::Error for CodexProjectConfigError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt as _};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "mesh-codex-project-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[cfg(target_os = "macos")]
    fn stable_reference(path: &Path) -> PathBuf {
        let metadata = fs::symlink_metadata(path).expect("reference metadata");
        PathBuf::from(format!("/.vol/{}/{}", metadata.dev(), metadata.ino()))
    }

    #[test]
    fn current_launch_binding_never_adds_workspace_content() {
        let storage = scratch("create");
        let root = storage.join("mounts");
        fs::create_dir_all(&root).expect("root");
        fs::write(root.join("tracked.txt"), b"ordinary project content\n").expect("project file");
        let executable = storage.join("Mesh executable");
        fs::write(&executable, b"binary").expect("executable");
        let endpoint = root.join("runtime/daemon.sock");
        let first =
            ensure_codex_project_config(&root, &executable, &endpoint, "installation-value")
                .expect("first install");
        assert_eq!(first, CodexProjectConfig::Current);
        assert!(matches!(
            fs::symlink_metadata(root.join(".codex")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound
        ));
        assert_eq!(
            fs::read_dir(&root)
                .expect("project entries")
                .map(|entry| entry.expect("project entry").file_name())
                .collect::<Vec<_>>(),
            vec![std::ffi::OsString::from("tracked.txt")]
        );
        assert!(matches!(
            fs::symlink_metadata(storage.join(PRIVATE_INTEGRATION_DIRECTORY)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound
        ));
        assert_eq!(
            ensure_codex_project_config(&root, &executable, &endpoint, "installation-value",)
                .expect("repeat"),
            CodexProjectConfig::Current
        );

        let overrides = codex_launch_overrides(&executable, &endpoint, &root, "installation-two")
            .expect("launch-scoped settings");
        assert_eq!(overrides.len(), 7);
        assert_eq!(
            overrides[0],
            format!(
                "mcp_servers.mesh.command={}",
                toml_string(executable.to_str().expect("executable path"))
            )
        );
        assert!(overrides[1].contains("--expected-workspace-root"));
        assert!(overrides[1].contains("installation-two"));
        assert_eq!(
            overrides[3],
            "mcp_servers.mesh.enabled_tools=[\"mesh_workspace_state\"]"
        );
        let _ = fs::remove_dir_all(storage);
    }

    #[test]
    fn exact_legacy_project_binding_remains_refreshable() {
        use std::os::unix::fs::symlink;

        let storage = scratch("legacy");
        let root = storage.join("mounts");
        let private = storage.join(PRIVATE_INTEGRATION_DIRECTORY);
        fs::create_dir_all(&root).expect("root");
        fs::create_dir_all(&private).expect("private configuration directory");
        fs::set_permissions(
            storage.join("integrations"),
            fs::Permissions::from_mode(0o700),
        )
        .expect("private integrations permissions");
        fs::set_permissions(&private, fs::Permissions::from_mode(0o700))
            .expect("private Codex permissions");
        symlink(LEGACY_PROJECT_LINK_TARGET, root.join(CONFIG_DIRECTORY))
            .expect("legacy project link");
        let executable = storage.join("mesh");
        fs::write(&executable, b"binary").expect("executable");
        let endpoint = storage.join("runtime/daemon.sock");

        assert_eq!(
            ensure_codex_project_config(&root, &executable, &endpoint, "installation-one")
                .expect("create legacy private config"),
            CodexProjectConfig::Created
        );
        let config = private.join(CONFIG_FILE);
        let first = fs::read_to_string(&config).expect("legacy config");
        assert!(first.contains("installation-one"));
        assert_eq!(
            fs::metadata(&config).unwrap().permissions().mode() & 0o077,
            0
        );
        assert_eq!(
            ensure_codex_project_config(&root, &executable, &endpoint, "installation-two")
                .expect("refresh legacy private config"),
            CodexProjectConfig::Refreshed
        );
        assert!(fs::read_to_string(config)
            .expect("refreshed legacy config")
            .contains("installation-two"));
        assert_eq!(
            fs::read_link(root.join(CONFIG_DIRECTORY)).expect("legacy target"),
            PathBuf::from(LEGACY_PROJECT_LINK_TARGET)
        );
        let _ = fs::remove_dir_all(storage);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn legacy_context_refresh_follows_pinned_references_not_a_replacement_namespace() {
        let storage = scratch("legacy-reference");
        let displaced = storage.with_extension("displaced");
        let root = storage.join("mounts");
        let private = storage.join(PRIVATE_INTEGRATION_DIRECTORY);
        fs::create_dir_all(&root).expect("root");
        fs::create_dir_all(&private).expect("private configuration directory");
        fs::set_permissions(
            storage.join("integrations"),
            fs::Permissions::from_mode(0o700),
        )
        .expect("private integrations permissions");
        fs::set_permissions(&private, fs::Permissions::from_mode(0o700))
            .expect("private Codex permissions");
        symlink(LEGACY_PROJECT_LINK_TARGET, root.join(CONFIG_DIRECTORY))
            .expect("legacy project link");
        let workspace_reference = stable_reference(&root);
        let storage_reference = stable_reference(&storage);

        fs::rename(&storage, &displaced).expect("displace admitted storage");
        fs::create_dir_all(storage.join("mounts")).expect("replacement storage");
        fs::write(storage.join("replacement.txt"), b"replacement\n").expect("replacement marker");
        let executable = std::env::current_exe().expect("test executable");
        let endpoint = std::env::temp_dir().join("mesh-codex-reference.sock");
        assert_eq!(
            ensure_codex_project_config_at_references(
                &root,
                &workspace_reference,
                &storage_reference,
                &executable,
                &endpoint,
                "installation-reference",
            )
            .expect("refresh through pinned references"),
            CodexProjectConfig::Created
        );
        assert!(
            displaced
                .join(PRIVATE_INTEGRATION_DIRECTORY)
                .join(CONFIG_FILE)
                .is_file(),
            "the admitted private storage receives the compatibility config"
        );
        let config = fs::read_to_string(
            displaced
                .join(PRIVATE_INTEGRATION_DIRECTORY)
                .join(CONFIG_FILE),
        )
        .expect("pinned compatibility config");
        assert!(
            config.contains(&root.display().to_string()),
            "the persisted MCP binding keeps the daemon's logical root"
        );
        assert!(
            !config.contains(&workspace_reference.display().to_string()),
            "the filesystem reference must not replace the MCP logical identity"
        );
        assert!(
            !storage.join(PRIVATE_INTEGRATION_DIRECTORY).exists(),
            "replacement storage must remain untouched"
        );
        let _ = fs::remove_dir_all(storage);
        let _ = fs::remove_dir_all(displaced);
    }

    #[test]
    fn existing_or_linked_configuration_is_never_overwritten() {
        let storage = scratch("refuse");
        let root = storage.join("mounts");
        fs::create_dir_all(&root).expect("root");
        let executable = storage.join("mesh");
        fs::write(&executable, b"binary").expect("executable");
        fs::create_dir(root.join(".codex")).expect("codex");
        fs::write(
            root.join(".codex/config.toml"),
            b"model = \"user-choice\"\n",
        )
        .expect("user config");
        let error = ensure_codex_project_config(
            &root,
            &executable,
            &root.join("daemon.sock"),
            "installation",
        )
        .expect_err("different config");
        assert!(matches!(error, CodexProjectConfigError::ExistingConfig(_)));
        assert_eq!(
            fs::read_to_string(root.join(".codex/config.toml")).unwrap(),
            "model = \"user-choice\"\n"
        );
        let outside = storage.join("outside");
        fs::create_dir(&outside).expect("outside");
        fs::remove_dir_all(root.join(".codex")).expect("remove user config");
        symlink(&outside, root.join(".codex")).expect("linked config");
        assert!(ensure_codex_project_config(
            &root,
            &executable,
            &root.join("daemon.sock"),
            "installation",
        )
        .is_err());
        assert!(outside.is_dir());
        let _ = fs::remove_dir_all(storage);
    }
}
